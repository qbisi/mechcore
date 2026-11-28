//! Shared persistent bot process, JSONL wire logger, and host `initialize`/`solve` loop.

use std::{collections::HashMap, fs::File, io::Write, process::Stdio, sync::Arc, time::Duration};

use serde::Deserialize;
use serde_json::{Value, json, value::RawValue};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Split},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::Semaphore,
};

use crate::{
    cli::Failure,
    rpc::{
        engine::{ComputeJob, ComputePool, RefStore, StateContext},
        wire::{IncomingMessage, ParsedFrame, RequestId, RpcError, ok_response, parse_line},
    },
    turn::Side,
};

/// Instant-flushing JSONL wire logger shared by `mechcore solve` and `mechcore arena`.
pub(crate) struct WireLog(File);

impl WireLog {
    pub(crate) fn open(path: &str) -> Result<Self, Failure> {
        File::create_new(path)
            .map(Self)
            .map_err(|e| Failure::failed(format!("cannot create wire log {path:?}: {e}")))
    }

    pub(crate) fn record_json(
        &self,
        side: Side,
        direction: &'static str,
        phase: &'static str,
        message: &Value,
    ) -> Result<(), Failure> {
        self.write_entry(side, direction, phase, message)
    }

    pub(crate) fn record_received(
        &self,
        side: Side,
        phase: &'static str,
        raw: &[u8],
    ) -> Result<(), Failure> {
        let direction = "bot_to_host";
        let trimmed = raw.strip_suffix(b"\r").unwrap_or(raw);
        match std::str::from_utf8(trimmed) {
            Ok(text) => {
                if let Ok(parsed) = serde_json::from_str::<&RawValue>(text) {
                    self.write_entry(side, direction, phase, parsed)
                } else {
                    self.write_entry(side, direction, phase, &text)
                }
            }
            Err(_) => self.write_entry(side, direction, phase, &json!({ "bytes": raw })),
        }
    }

    fn write_entry<T: serde::Serialize + ?Sized>(
        &self,
        side: Side,
        direction: &'static str,
        phase: &'static str,
        message: &T,
    ) -> Result<(), Failure> {
        #[derive(serde::Serialize)]
        struct Entry<'a, M: ?Sized> {
            side: Side,
            direction: &'static str,
            phase: &'static str,
            message: &'a M,
        }
        let mut row = serde_json::to_vec(&Entry {
            side,
            direction,
            phase,
            message,
        })
        .map_err(|e| Failure::failed(e.to_string()))?;
        row.push(b'\n');
        let mut file = &self.0;
        file.write_all(&row)
            .and_then(|()| file.flush())
            .map_err(|e| Failure::failed(format!("cannot write wire log: {e}")))
    }
}

/// Process group guard that SIGKILLs the spawned shell and all descendants on drop.
struct ProcessGroup(Option<u32>);

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(id) = self.0.and_then(|id| i32::try_from(id).ok()) {
            unsafe { libc::kill(-id, libc::SIGKILL) };
        }
    }
}

pub(crate) enum SolveOutcome {
    Answered {
        raw_actions: Vec<Value>,
        meta: Option<Value>,
    },
    TimedOut,
    Failed(Failure),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CancelParams {
    id: RequestId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SolveResultPayload {
    actions: Vec<Value>,
    #[serde(default)]
    meta: Option<Value>,
}

/// Persistent Bot child process communicating over bidirectional JSON-RPC 2.0 JSONL.
pub(crate) struct BotProcess<'a> {
    side: Side,
    log: Option<&'a WireLog>,
    group: ProcessGroup,
    child: Child,
    input: Option<ChildStdin>,
    lines: Split<BufReader<ChildStdout>>,
}

#[allow(clippy::needless_pass_by_value)]
fn io_failed(error: std::io::Error) -> Failure {
    Failure::failed(error.to_string())
}

impl<'a> BotProcess<'a> {
    pub(crate) fn start(cmd: &str, side: Side, log: Option<&'a WireLog>) -> Result<Self, Failure> {
        let mut command = Command::new("sh");
        #[cfg(unix)]
        command.process_group(0);
        command
            .args(["-c", cmd])
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(std::process::Stdio::inherit());
        let mut child = command.spawn().map_err(io_failed)?;
        Ok(Self {
            side,
            log,
            group: ProcessGroup(child.id()),
            input: Some(child.stdin.take().expect("piped stdin")),
            lines: BufReader::new(child.stdout.take().expect("piped stdout")).split(b'\n'),
            child,
        })
    }

    async fn send_json(&mut self, phase: &'static str, value: &Value) -> Result<(), Failure> {
        if let Some(log) = self.log {
            log.record_json(self.side, "host_to_bot", phase, value)?;
        }
        let mut line = serde_json::to_vec(value).map_err(|e| Failure::failed(e.to_string()))?;
        line.push(b'\n');
        let input = self
            .input
            .as_mut()
            .ok_or_else(|| Failure::failed("bot stdin closed"))?;
        input.write_all(&line).await.map_err(io_failed)?;
        input.flush().await.map_err(io_failed)?;
        Ok(())
    }

    /// Runs the `initialize` handshake and validates the bot's reported display name.
    pub(crate) async fn initialize(
        &mut self,
        parallel: usize,
        timeout: Option<Duration>,
    ) -> Result<String, Failure> {
        let req = json!({
            "jsonrpc": "2.0",
            "id": "init",
            "method": "initialize",
            "params": {
                "protocol": "mechcore-rpc",
                "game_build": mechcore_document::game_build(),
                "capabilities": {
                    "methods": ["convert", "act", "simulate"],
                    "parallel": parallel,
                }
            }
        });
        let handshake = async {
            self.send_json("initialize", &req).await?;
            self.read_initialize_reply().await
        };
        match timeout {
            Some(bound) => match tokio::time::timeout(bound, handshake).await {
                Ok(result) => result,
                Err(_) => Err(Failure::failed(format!(
                    "{} timed out during initialize handshake",
                    self.side.name()
                ))),
            },
            None => handshake.await,
        }
    }

    async fn read_initialize_reply(&mut self) -> Result<String, Failure> {
        loop {
            let Some(line) = self.lines.next_segment().await.map_err(io_failed)? else {
                return Err(Failure::failed(format!(
                    "{} exited before responding to initialize",
                    self.side.name()
                )));
            };
            if let Some(log) = self.log {
                log.record_received(self.side, "initialize", &line)?;
            }
            let ParsedFrame::Items(mut items) = parse_line(&line) else {
                continue;
            };
            if items.len() != 1 {
                return Err(Failure::failed(format!(
                    "{} sent batch during initialize handshake",
                    self.side.name()
                )));
            }
            match items.remove(0) {
                IncomingMessage::Response {
                    id: RequestId::String(ref id),
                    result: Some(Value::Object(mut obj)),
                    error: None,
                } if id == "init" => {
                    let Some(Value::String(name)) = obj.remove("name") else {
                        return Err(Failure::failed(format!(
                            "{} initialize result must contain string \"name\"",
                            self.side.name()
                        )));
                    };
                    if let Some((extra, _)) = obj.iter().next() {
                        return Err(Failure::failed(format!(
                            "{} initialize result contains unexpected field {extra:?}",
                            self.side.name()
                        )));
                    }
                    let trimmed = name.trim();
                    if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
                        return Err(Failure::failed(format!(
                            "{} initialize name must be non-empty without control characters",
                            self.side.name()
                        )));
                    }
                    return Ok(trimmed.to_owned());
                }
                IncomingMessage::Response {
                    error: Some(err), ..
                } => {
                    return Err(Failure::failed(format!(
                        "{} returned error for initialize: {err}",
                        self.side.name()
                    )));
                }
                IncomingMessage::Request { method, .. } => {
                    return Err(Failure::failed(format!(
                        "{} sent reverse RPC request {method:?} during initialize handshake",
                        self.side.name()
                    )));
                }
                other => {
                    return Err(Failure::failed(format!(
                        "{} sent invalid initialize response: {other:?}",
                        self.side.name()
                    )));
                }
            }
        }
    }

    /// Sends one `solve` request and serves reverse stateless RPC calls until a valid
    /// `solve` response arrives, a fatal protocol/child failure occurs, or `timeout` elapses.
    pub(crate) async fn solve(
        &mut self,
        problem: &Value,
        ref_id: &str,
        ctx: Arc<StateContext>,
        timeout: Option<Duration>,
        parallel: usize,
        slots: Arc<Semaphore>,
    ) -> SolveOutcome {
        let solve_id = format!("solve-{}-{}", ctx.round(), self.side.name());
        let req = json!({
            "jsonrpc": "2.0",
            "id": solve_id,
            "method": "solve",
            "params": {
                "side": self.side,
                "problem": problem,
                "limits": {
                    "timeout": timeout.map(|d| d.as_secs_f64()),
                    "parallel": parallel,
                }
            }
        });
        let refs: Arc<RefStore> = Arc::new(HashMap::from([(ref_id.to_owned(), ctx)]));
        let mut pool = ComputePool::with_slots(parallel, slots);

        let fut = async {
            if let Err(err) = self.send_json("solve", &req).await {
                return SolveOutcome::Failed(err);
            }
            self.solve_loop(&solve_id, &refs, &mut pool).await
        };
        let outcome = match timeout {
            Some(bound) => match tokio::time::timeout(bound, fut).await {
                Ok(outcome) => outcome,
                Err(_) => SolveOutcome::TimedOut,
            },
            None => fut.await,
        };
        pool.cancel_all();
        outcome
    }

    async fn solve_loop(
        &mut self,
        solve_id: &str,
        refs: &Arc<RefStore>,
        pool: &mut ComputePool,
    ) -> SolveOutcome {
        loop {
            tokio::select! {
                completed = pool.next_completed(), if !pool.is_running_empty() => {
                    let Some(joined) = completed else { continue };
                    let (id, result) = match joined {
                        Ok(pair) => pair,
                        Err(e) => return SolveOutcome::Failed(Failure::failed(e.to_string())),
                    };
                    let reply = match result {
                        Ok(output) => ok_response(&id, &output),
                        Err(err) => err.to_response(Some(&id)),
                    };
                    if let Err(e) = self.send_json("solve", &reply).await {
                        return SolveOutcome::Failed(e);
                    }
                }
                segment = self.lines.next_segment() => {
                    let line = match segment {
                        Ok(Some(line)) => line,
                        Ok(None) => {
                            return SolveOutcome::Failed(Failure::failed(format!(
                                "{} exited before returning solve result",
                                self.side.name()
                            )));
                        }
                        Err(e) => return SolveOutcome::Failed(io_failed(e)),
                    };
                    if let Some(log) = self.log
                        && let Err(e) = log.record_received(self.side, "solve", &line)
                    {
                        return SolveOutcome::Failed(e);
                    }
                    match parse_line(&line) {
                        ParsedFrame::Empty => {}
                        ParsedFrame::Items(items) => {
                            if let Some(outcome) = self.handle_solve_items(items, solve_id, refs, pool).await {
                                return outcome;
                            }
                        }
                    }
                }
            }
        }
    }

    async fn handle_solve_items(
        &mut self,
        items: Vec<IncomingMessage>,
        solve_id: &str,
        refs: &Arc<RefStore>,
        pool: &mut ComputePool,
    ) -> Option<SolveOutcome> {
        for item in items {
            match item {
                IncomingMessage::Invalid { id, error } => {
                    if let Some(ref req_id) = id
                        && error.code == crate::rpc::wire::code::INVALID_PARAMS
                    {
                        let resp = error.to_response(Some(req_id));
                        if let Err(e) = self.send_json("solve", &resp).await {
                            return Some(SolveOutcome::Failed(e));
                        }
                        continue;
                    }
                    return Some(SolveOutcome::Failed(Failure::failed(format!(
                        "{} sent invalid JSON-RPC message: {}",
                        self.side.name(),
                        error.failure.reason()
                    ))));
                }
                IncomingMessage::Response { id, result, error } => {
                    return Some(parse_solve_response(
                        self.side, solve_id, &id, result, error,
                    ));
                }
                IncomingMessage::Request { id, method, params } => {
                    if id == RequestId::String(solve_id.to_owned()) {
                        let resp = RpcError::invalid_params(
                            method,
                            Failure::usage("cannot reuse solve request id"),
                        )
                        .to_response(Some(&id));
                        if let Err(e) = self.send_json("solve", &resp).await {
                            return Some(SolveOutcome::Failed(e));
                        }
                        continue;
                    }
                    let job = match method.as_str() {
                        "convert" => ComputeJob::Convert { params },
                        "act" => ComputeJob::Act {
                            params,
                            refs: Arc::clone(refs),
                        },
                        "simulate" => ComputeJob::Simulate {
                            params,
                            refs: Arc::clone(refs),
                        },
                        _ => {
                            let resp =
                                RpcError::method_not_found("rpc", &method).to_response(Some(&id));
                            if let Err(e) = self.send_json("solve", &resp).await {
                                return Some(SolveOutcome::Failed(e));
                            }
                            continue;
                        }
                    };
                    if let Some(err) = pool.enqueue(id.clone(), job) {
                        let resp = err.to_response(Some(&id));
                        if let Err(e) = self.send_json("solve", &resp).await {
                            return Some(SolveOutcome::Failed(e));
                        }
                    }
                }
                IncomingMessage::Notification { method, params } => {
                    if method == "$/cancel"
                        && let Some(params_val) = params
                        && let Ok(CancelParams { id }) = serde_json::from_value(params_val)
                        && let Some((cid, err)) = pool.cancel(&id)
                    {
                        let resp = err.to_response(Some(&cid));
                        if let Err(e) = self.send_json("solve", &resp).await {
                            return Some(SolveOutcome::Failed(e));
                        }
                    }
                }
            }
        }
        None
    }

    /// Gracefully closes stdin and waits up to 2 seconds before sending `SIGKILL` to the process group.
    pub(crate) async fn finish(&mut self) {
        drop(self.input.take());
        if tokio::time::timeout(Duration::from_secs(2), self.child.wait())
            .await
            .is_err()
        {
            self.kill_now().await;
        } else {
            self.group.0 = None;
        }
    }

    /// Immediately sends `SIGKILL` to the bot's process group and reaps the child process.
    pub(crate) async fn kill_now(&mut self) {
        drop(ProcessGroup(self.group.0.take()));
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }
}

fn parse_solve_response(
    side: Side,
    solve_id: &str,
    id: &RequestId,
    result: Option<Value>,
    error: Option<Value>,
) -> SolveOutcome {
    if *id != RequestId::String(solve_id.to_owned()) {
        return SolveOutcome::Failed(Failure::failed(format!(
            "{} replied with unexpected response id {id:?} (expected {solve_id:?})",
            side.name()
        )));
    }
    if let Some(err) = error {
        return SolveOutcome::Failed(Failure::failed(format!(
            "{} returned JSON-RPC error for solve: {err}",
            side.name()
        )));
    }
    let Some(output) = result else {
        return SolveOutcome::Failed(Failure::failed(format!(
            "{} solve response missing result",
            side.name()
        )));
    };
    let payload: SolveResultPayload = match serde_json::from_value(output) {
        Ok(p) => p,
        Err(e) => {
            return SolveOutcome::Failed(Failure::failed(format!(
                "{} solve result has invalid shape: {e}",
                side.name()
            )));
        }
    };
    let meta = match payload.meta {
        None | Some(Value::Null) => None,
        Some(Value::Object(map)) if map.is_empty() => None,
        Some(obj @ Value::Object(_)) => Some(obj),
        Some(_) => {
            return SolveOutcome::Failed(Failure::failed(format!(
                "{} solve result.meta must be a JSON object or null",
                side.name()
            )));
        }
    };
    SolveOutcome::Answered {
        raw_actions: payload.actions,
        meta,
    }
}
