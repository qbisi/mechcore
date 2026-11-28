//! Stateless JSON-RPC 2.0 server (`mechcore rpc`) and shared bidirectional RPC stack.

pub(crate) mod engine;
pub(crate) mod session;
pub(crate) mod wire;

use std::{num::NonZeroUsize, sync::Arc};

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::cli::{Args, Failure, Outcome, Verdict};
use engine::{ComputeJob, ComputePool, RefStore};
use wire::{IncomingMessage, ParsedFrame, RequestId, RpcError, ok_response, parse_line};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CancelParams {
    id: RequestId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InitializeParams {
    #[allow(dead_code)]
    #[serde(default)]
    client: Option<Value>,
}

/// Runs `mechcore rpc [--parallel <N>]`.
pub(crate) fn run(mut args: Args) -> Outcome {
    let parallel = args
        .parsed::<NonZeroUsize>("--parallel", "a positive number of worker threads")?
        .unwrap_or_else(|| std::thread::available_parallelism().unwrap_or(NonZeroUsize::MIN))
        .get();
    args.finish()?;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| Failure::failed(e.to_string()))?;

    runtime.block_on(serve(parallel)).map(|()| Verdict::Yes)
}

async fn write_response(stdout: &mut tokio::io::Stdout, value: &Value) -> Result<(), Failure> {
    let mut line = serde_json::to_vec(value).map_err(|e| Failure::failed(e.to_string()))?;
    line.push(b'\n');
    stdout
        .write_all(&line)
        .await
        .map_err(|e| Failure::failed(e.to_string()))?;
    stdout
        .flush()
        .await
        .map_err(|e| Failure::failed(e.to_string()))?;
    Ok(())
}

async fn serve(parallel: usize) -> Result<(), Failure> {
    let mut lines = BufReader::new(tokio::io::stdin()).split(b'\n');
    let mut stdout = tokio::io::stdout();
    let empty_refs = Arc::new(RefStore::new());
    let mut pool = ComputePool::new(parallel);
    let mut eof = false;

    loop {
        if eof && pool.is_idle() {
            break;
        }
        tokio::select! {
            completed = pool.next_completed(), if !pool.is_running_empty() => {
                let Some(joined) = completed else { continue };
                let (id, result) = joined.map_err(|e| Failure::failed(e.to_string()))?;
                let response = match result {
                    Ok(output) => ok_response(&id, &output),
                    Err(err) => err.to_response(Some(&id)),
                };
                write_response(&mut stdout, &response).await?;
            }
            segment = lines.next_segment(), if !eof => {
                match segment.map_err(|e| Failure::failed(e.to_string()))? {
                    None => eof = true,
                    Some(raw) => {
                        let ParsedFrame::Items(items) = parse_line(&raw) else {
                            continue;
                        };
                        for item in items {
                            handle_item(item, parallel, &empty_refs, &mut pool, &mut stdout).await?;
                        }
                    }
                }
            }
        }
    }
    stdout
        .flush()
        .await
        .map_err(|e| Failure::failed(e.to_string()))?;
    Ok(())
}

async fn handle_item(
    item: IncomingMessage,
    parallel: usize,
    empty_refs: &Arc<RefStore>,
    pool: &mut ComputePool,
    stdout: &mut tokio::io::Stdout,
) -> Result<(), Failure> {
    match item {
        IncomingMessage::Invalid { id, error } => {
            write_response(stdout, &error.to_response(id.as_ref())).await?;
        }
        IncomingMessage::Response { id, .. } => {
            let resp =
                RpcError::invalid_request("rpc", "unexpected JSON-RPC response on rpc stdin")
                    .to_response(Some(&id));
            write_response(stdout, &resp).await?;
        }
        IncomingMessage::Notification { method, params } => {
            if method == "$/cancel"
                && let Some(params_val) = params
                && let Ok(CancelParams { id }) = serde_json::from_value(params_val)
                && let Some((cid, err)) = pool.cancel(&id)
            {
                write_response(stdout, &err.to_response(Some(&cid))).await?;
            }
        }
        IncomingMessage::Request { id, method, params } => {
            if method == "initialize" {
                if pool.contains_id(&id) {
                    let resp = RpcError::invalid_params(
                        "initialize",
                        Failure::usage("duplicate in-flight request id"),
                    )
                    .to_response(Some(&id));
                    write_response(stdout, &resp).await?;
                    return Ok(());
                }
                if let Some(params_val) = params
                    && let Err(e) = serde_json::from_value::<InitializeParams>(params_val)
                {
                    let resp =
                        RpcError::invalid_params("initialize", Failure::usage(e.to_string()))
                            .to_response(Some(&id));
                    write_response(stdout, &resp).await?;
                    return Ok(());
                }
                let result = json!({
                    "protocol": "mechcore-rpc",
                    "game_build": mechcore_document::game_build(),
                    "parallel": parallel,
                    "capabilities": {
                        "methods": ["convert", "act", "simulate"],
                    }
                });
                write_response(stdout, &ok_response(&id, &result)).await?;
                return Ok(());
            }
            let job = match method.as_str() {
                "convert" => ComputeJob::Convert { params },
                "act" => ComputeJob::Act {
                    params,
                    refs: Arc::clone(empty_refs),
                },
                "simulate" => ComputeJob::Simulate {
                    params,
                    refs: Arc::clone(empty_refs),
                },
                _ => {
                    let resp = RpcError::method_not_found("rpc", &method).to_response(Some(&id));
                    write_response(stdout, &resp).await?;
                    return Ok(());
                }
            };
            if let Some(err) = pool.enqueue(id.clone(), job) {
                write_response(stdout, &err.to_response(Some(&id))).await?;
            }
        }
    }
    Ok(())
}
