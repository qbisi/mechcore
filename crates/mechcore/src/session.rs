//! Game session orchestration shared by every `mechcore` frontend.
//!
//! This layer owns the Adapter connection, the status stream, mutation
//! serialization, and the pre/post conditions of each native operation. It
//! speaks only `serde_json::Value` and `String`, so no frontend protocol
//! leaks into it. See `docs/session.md` for the acquisition contract.

use crate::acquire::{self, Mode, Ownership};
use crate::adapter;
use mechcore_protocol::Operation;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    sync::{Mutex, watch},
    time::{Instant, sleep, timeout_at},
};

pub(crate) const STATUS_INTERVAL: Duration = Duration::from_millis(100);
pub(crate) const ADAPTER_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
pub(crate) const TRANSITION_TIMEOUT: Duration = Duration::from_secs(60);
/// A cold start must reach the main menu within this budget.
const READY_TIMEOUT: Duration = Duration::from_secs(180);

/// Structured error body carried by a failed operation result.
pub(crate) fn error_body(error: impl Into<String>) -> Value {
    json!({"error": error.into()})
}

/// Research-only instrumentation request accepted by the recording operations.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecordBattleInstrumentationParameters {
    /// Absolute destination path for the new HDF5 instrumentation sidecar.
    pub(crate) output: PathBuf,
    /// Adapter-defined temporary research profile name.
    pub(crate) profile: String,
    /// Bound RVO detail to selected MCFR units and combat update-start ticks.
    pub(crate) rvo_scope: Option<RvoCaptureScopeParameters>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RvoCaptureScopeParameters {
    pub(crate) start_tick: u64,
    pub(crate) end_tick: u64,
    pub(crate) unit_ids: Vec<u64>,
}

pub(crate) struct Session {
    socket_path: PathBuf,
    adapter: Mutex<Option<adapter::Client>>,
    operation: Mutex<()>,
    last_applied_layout: Mutex<Option<Value>>,
    status: watch::Sender<Value>,
}
impl Session {
    pub(crate) fn new() -> Arc<Self> {
        let socket_path = default_adapter_socket();
        let (status, _) = watch::channel(json!({"status": "game_off"}));
        Arc::new(Self {
            socket_path,
            adapter: Mutex::new(None),
            operation: Mutex::new(()),
            last_applied_layout: Mutex::new(None),
            status,
        })
    }

    /// Poll the Adapter's status until the connection drops.
    ///
    /// Every frontend needs the same stream, so it lives here rather than in
    /// whichever frontend happens to spawn it.
    pub(crate) async fn monitor_status(self: Arc<Self>) {
        loop {
            if self.is_connected().await {
                match self.adapter_request(Operation::Status, json!({})).await {
                    Ok(status) => self.publish(status),
                    Err(_) => self.disconnect_adapter().await,
                }
            }
            sleep(STATUS_INTERVAL).await;
        }
    }

    pub(crate) fn current_status(&self) -> Value {
        self.status.borrow().clone()
    }


    /// Whether an Adapter connection is currently held.
    pub(crate) async fn is_connected(&self) -> bool {
        self.adapter.lock().await.is_some()
    }

    pub(crate) fn publish(&self, status: Value) {
        if *self.status.borrow() != status {
            self.status.send_replace(status);
        }
    }

    pub(crate) async fn disconnect_adapter(&self) {
        *self.adapter.lock().await = None;
        self.publish(json!({"status": "game_off"}));
    }

    pub(crate) async fn adapter_request(
        &self,
        operation: Operation,
        arguments: Value,
    ) -> Result<Value, String> {
        let mut adapter = self.adapter.lock().await;
        let client = adapter.as_mut().ok_or_else(|| {
            "game adapter is not connected; acquire the game with launch or attach".to_owned()
        })?;
        let request_timeout = match operation {
            Operation::RecordBattle => Duration::from_secs(180),
            Operation::RecordReplayRound => Duration::from_secs(330),
            _ => ADAPTER_REQUEST_TIMEOUT,
        };
        match tokio::time::timeout(request_timeout, client.request(operation, arguments)).await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => {
                let fatal = error.is_fatal();
                let message = error.to_string();
                if fatal {
                    *adapter = None;
                    self.publish(json!({"status": "unknown"}));
                }
                Err(message)
            }
            Err(_) => {
                *adapter = None;
                self.publish(json!({"status": "unknown"}));
                Err(format!(
                    "adapter request {operation} timed out; its outcome is unknown and it was not retried"
                ))
            }
        }
    }

    pub(crate) async fn refresh_status(&self) -> Result<Value, String> {
        let status = self.adapter_request(Operation::Status, json!({})).await?;
        self.publish(status.clone());
        Ok(status)
    }

    pub(crate) async fn wait_status(
        &self,
        description: &str,
        timeout: Duration,
        predicate: impl Fn(&Value) -> bool,
    ) -> Result<Value, String> {
        let deadline = Instant::now() + timeout;
        let mut receiver = self.status.subscribe();
        loop {
            let current = receiver.borrow_and_update().clone();
            if predicate(&current) {
                return Ok(current);
            }
            timeout_at(deadline, receiver.changed())
                .await
                .map_err(|_| {
                    format!("timed out waiting for {description}; last status: {current}")
                })?
                .map_err(|_| "status monitor stopped".to_owned())?;
        }
    }

    /// The endpoint this session resolves to.
    pub(crate) fn endpoint(&self) -> &Path {
        &self.socket_path
    }

    /// Acquire the game as declared and wait until it can accept work.
    ///
    /// A launched game's endpoint appears well before the title finishes
    /// booting, so connecting is not readiness. Every native operation begins
    /// at the main menu; returning earlier would let a caller's first step race
    /// the loading screen.
    pub(crate) async fn acquire(self: &Arc<Self>, mode: Mode) -> Result<Ownership, String> {
        let (client, ownership) = acquire::acquire(mode, self.endpoint())
            .await
            .map_err(|failure| format!("{} refused, {failure}", mode.as_str()))?;
        self.install_client(client).await?;
        self.wait_status("the main menu", READY_TIMEOUT, |status| {
            is_status(status, "main_menu")
        })
        .await?;
        Ok(ownership)
    }

    /// Release the game according to ownership.
    ///
    /// An attached game is left to its owner. An owned game is asked to quit
    /// and then awaited; if the request itself failed the game was never told
    /// to exit, so awaiting it would hang and it is terminated instead.
    pub(crate) async fn release(&self, ownership: Option<Ownership>) -> Result<(), String> {
        let Some(Ownership::Owned { mut child, .. }) = ownership else {
            return Ok(());
        };
        match self.quit_game().await {
            Ok(_) => match child.wait().await {
                Ok(status) if status.success() => Ok(()),
                Ok(status) => Err(format!("game exited with {status}")),
                Err(error) => Err(format!("cannot await the game process: {error}")),
            },
            Err(error) => {
                let killed = child.kill().await;
                Err(format!(
                    "quit_game failed ({error}); the launched game was terminated{}",
                    match killed {
                        Ok(()) => String::new(),
                        Err(problem) => format!(" unsuccessfully: {problem}"),
                    }
                ))
            }
        }
    }

    /// Adopt a client that acquisition already connected and greeted.
    ///
    /// Acquisition must keep the connection it probed with: dropping it and
    /// reconnecting would race another client into the single serving slot.
    pub(crate) async fn install_client(&self, client: adapter::Client) -> Result<Value, String> {
        let _operation = self.operation.lock().await;
        let mut adapter = self.adapter.lock().await;
        if adapter.is_some() {
            return Err("game adapter is already connected".into());
        }
        *adapter = Some(client);
        drop(adapter);
        let status = self.refresh_status().await?;
        Ok(json!({"connected": true, "status": status}))
    }


    pub(crate) async fn start_test(&self, seed: Option<i32>) -> Result<Value, String> {
        let _operation = self.operation.lock().await;
        self.require_status("main_menu").await?;
        *self.last_applied_layout.lock().await = None;
        let result = self
            .adapter_request(Operation::StartTest, json!({"seed": seed}))
            .await?;
        let status = self
            .wait_status(
                "round-one deployment after start_test",
                TRANSITION_TIMEOUT,
                |value| is_training_state(value, 1, true, false),
            )
            .await?;
        Ok(json!({"operation": result, "status": status}))
    }

    /// Create the Training Ground and bring it to the layout's activation round.
    ///
    /// This owns the whole transaction from the main menu, because a layout
    /// already carries the seed and the round that `start_test` would otherwise
    /// be handed separately. `seed` overrides the layout's own value, which is
    /// what lets one layout be recorded under many seeds.
    pub(crate) async fn apply_layout(
        &self,
        layout: Value,
        seed: Option<i32>,
    ) -> Result<Value, String> {
        let plan = mechcore_layout::compile(&layout)?;
        let activation_round = i64::from(plan.round);
        let seed = seed.unwrap_or(plan.seed);
        let _operation = self.operation.lock().await;
        let status = self.current_status();
        if !is_status(&status, "main_menu") {
            return Err(format!(
                "apply_layout creates the Training Ground itself and starts from the main menu, \
                 so it must not follow start_test; observed {status}"
            ));
        }
        *self.last_applied_layout.lock().await = None;

        let created = self
            .adapter_request(Operation::StartTest, json!({"seed": seed}))
            .await?;
        self.wait_status(
            "round-one deployment after start_test",
            TRANSITION_TIMEOUT,
            |value| is_training_state(value, 1, true, false),
        )
        .await?;

        let result = self
            .adapter_request(Operation::ApplyLayout, layout.clone())
            .await?;
        if result.get("applied").and_then(Value::as_bool) != Some(true) {
            return Err(format!(
                "adapter did not confirm layout application: {result}"
            ));
        }
        if result.get("round").and_then(Value::as_i64) != Some(activation_round) {
            return Err(format!(
                "adapter completed layout for an unexpected round: {result}"
            ));
        }
        let status = self.refresh_status().await?;
        if !is_training_state(&status, activation_round, true, false) {
            return Err(format!(
                "layout completed outside activation-round deployment: {status}"
            ));
        }
        *self.last_applied_layout.lock().await = Some(layout.clone());
        Ok(json!({"operation": result, "test": created, "status": status}))
    }

    pub(crate) async fn record_battle(
        &self,
        output: PathBuf,
        video_output: Option<PathBuf>,
        speed_up: Option<bool>,
        instrumentation: Option<RecordBattleInstrumentationParameters>,
    ) -> Result<Value, Value> {
        let _operation = self.operation.lock().await;
        let before = self.refresh_status().await.map_err(error_body)?;
        if !is_training_deployment(&before) {
            return Err(error_body(format!(
                "record_battle requires completed Training Ground deployment: {before}"
            )));
        }
        validate_record_outputs(&output, video_output.as_deref(), instrumentation.as_ref())?;
        let layout_input = self
            .last_applied_layout
            .lock()
            .await
            .clone()
            .ok_or_else(|| {
                error_body(
                    "record_battle requires a successfully applied layout in this test session",
                )
            })?;
        let result = match self
            .adapter_request(
                Operation::RecordBattle,
                json!({
                    "output": output,
                    "video_output": video_output,
                    "speed_up": speed_up,
                    "instrumentation": instrumentation,
                }),
            )
            .await
        {
            Ok(result) => result,
            Err(error) => {
                *self.last_applied_layout.lock().await = None;
                let cleanup = self.finish_recording_match().await;
                return Err(record_battle_failure(
                    &error,
                    &Value::Null,
                    &layout_input,
                    cleanup,
                    self.current_status(),
                ));
            }
        };
        if result.get("recorded").and_then(Value::as_bool) != Some(true) {
            *self.last_applied_layout.lock().await = None;
            let cleanup = self.finish_recording_match().await;
            let error = format!("adapter did not confirm recording: {result}");
            return Err(record_battle_failure(
                &error,
                &result,
                &layout_input,
                cleanup,
                self.current_status(),
            ));
        }
        *self.last_applied_layout.lock().await = None;
        let cleanup = match self.finish_recording_match().await {
            Ok(cleanup) => cleanup,
            Err(error) => {
                let message = format!(
                    "record_battle published artifacts but could not return to main_menu: {error}"
                );
                return Err(record_battle_failure(
                    &message,
                    &result,
                    &layout_input,
                    Err(error),
                    self.current_status(),
                ));
            }
        };
        let status = cleanup["status"].clone();
        Ok(json!({
            "operation": result,
            "layout_input": layout_input,
            "cleanup": {
                "match_exited": true,
                "game_reusable": true,
                "operation": cleanup["operation"].clone(),
            },
            "status": status,
            "next": {
                "required": Value::Null,
                "allowed": ["start_test", "quit_game"],
            },
        }))
    }

    pub(crate) async fn finish_recording_match(&self) -> Result<Value, String> {
        let status = self.refresh_status().await?;
        if is_status(&status, "main_menu") {
            return Ok(json!({"operation": Value::Null, "status": status}));
        }
        if !matches!(
            status.get("status").and_then(Value::as_str),
            Some("training_ground" | "replay")
        ) {
            return Err(format!(
                "recording cleanup requires an active match or main_menu: {status}"
            ));
        }
        self.leave_active_match().await
    }

    pub(crate) async fn record_replay_round(
        &self,
        grbr: PathBuf,
        round: i32,
        output: PathBuf,
        speed_up: Option<bool>,
        instrumentation: Option<RecordBattleInstrumentationParameters>,
    ) -> Result<Value, String> {
        let _operation = self.operation.lock().await;
        self.require_status("main_menu").await?;
        if !grbr.is_absolute() {
            return Err("record_replay_round grbr must be an absolute path".into());
        }
        if grbr.extension().and_then(|value| value.to_str()) != Some("grbr") {
            return Err("record_replay_round input must use the .grbr extension".into());
        }
        if !grbr.is_file() {
            return Err(format!("replay file does not exist: {}", grbr.display()));
        }
        if !(1..=mechcore_protocol::MAX_ACTIVATION_ROUND).contains(&round) {
            return Err(format!(
                "record_replay_round round must be between 1 and {}",
                mechcore_protocol::MAX_ACTIVATION_ROUND
            ));
        }
        if !output.is_absolute()
            || output.extension().and_then(|value| value.to_str()) != Some("mcfr")
            || output.exists()
        {
            return Err("record_replay_round output must be a new absolute .mcfr path".into());
        }
        *self.last_applied_layout.lock().await = None;
        validate_record_outputs(&output, None, instrumentation.as_ref())
            .map_err(|error| error.to_string())?;
        let result = self
            .adapter_request(
                Operation::RecordReplayRound,
                json!({
                    "grbr": grbr,
                    "round": round,
                    "output": output,
                    "speed_up": speed_up,
                    "instrumentation": instrumentation,
                }),
            )
            .await?;
        if result.get("recorded").and_then(Value::as_bool) != Some(true) {
            return Err(format!(
                "adapter did not confirm replay round recording: {result}"
            ));
        }
        let status = self.refresh_status().await?;
        if !is_status(&status, "main_menu") {
            return Err(format!(
                "record_replay_round completed outside main_menu: {status}"
            ));
        }
        Ok(json!({"operation": result, "status": status}))
    }

    pub(crate) async fn toggle_fight(&self) -> Result<Value, String> {
        let _operation = self.operation.lock().await;
        let before = self.refresh_status().await?;
        if !is_training_deployment(&before) {
            return Err(format!(
                "toggle_fight requires Training Ground deployment: {before}"
            ));
        }
        let round = before
            .get("round_count")
            .and_then(Value::as_i64)
            .ok_or_else(|| "Training Ground status omitted round_count".to_owned())?;
        let result = self
            .adapter_request(Operation::ToggleFight, json!({}))
            .await?;
        let status = self
            .wait_status(
                "fight transition after toggle_fight",
                TRANSITION_TIMEOUT,
                |value| {
                    value.get("status").and_then(Value::as_str) == Some("training_ground")
                        && (value.get("fighting").and_then(Value::as_bool) == Some(true)
                            || value
                                .get("round_count")
                                .and_then(Value::as_i64)
                                .is_some_and(|current| current > round))
                },
            )
            .await?;
        Ok(json!({"operation": result, "status": status}))
    }

    pub(crate) async fn speed_up(&self) -> Result<Value, String> {
        let _operation = self.operation.lock().await;
        let status = self.refresh_status().await?;
        if status.get("status").and_then(Value::as_str) != Some("training_ground")
            || status.get("fighting").and_then(Value::as_bool) != Some(true)
        {
            return Err(format!(
                "speed_up requires an active Training Ground fight: {status}"
            ));
        }
        let result = self.adapter_request(Operation::SpeedUp, json!({})).await?;
        if result.get("requested").and_then(Value::as_bool) != Some(true) {
            return Err(format!("adapter did not confirm speed_up: {result}"));
        }
        Ok(json!({"operation": result, "status": self.current_status()}))
    }

    pub(crate) async fn quit_match(&self) -> Result<Value, String> {
        let _operation = self.operation.lock().await;
        let status = self.refresh_status().await?;
        if !matches!(
            status.get("status").and_then(Value::as_str),
            Some("training_ground" | "replay")
        ) {
            return Err(format!("quit_match requires an active match: {status}"));
        }
        self.leave_active_match().await
    }

    pub(crate) async fn leave_active_match(&self) -> Result<Value, String> {
        let result = self
            .adapter_request(Operation::QuitMatch, json!({}))
            .await?;
        let status = self
            .wait_status("main menu after quit_match", TRANSITION_TIMEOUT, |value| {
                is_status(value, "main_menu")
            })
            .await?;
        Ok(json!({"operation": result, "status": status}))
    }

    pub(crate) async fn quit_game(&self) -> Result<Value, String> {
        let _operation = self.operation.lock().await;
        self.require_status("main_menu").await?;
        let result = self.adapter_request(Operation::QuitGame, json!({})).await?;
        let status = self
            .wait_status(
                "Adapter disconnect after quit_game",
                TRANSITION_TIMEOUT,
                |value| is_status(value, "game_off"),
            )
            .await?;
        Ok(json!({
            "operation": result,
            "status": status,
        }))
    }

    pub(crate) async fn require_status(&self, expected: &str) -> Result<Value, String> {
        let status = self.refresh_status().await?;
        if is_status(&status, expected) {
            Ok(status)
        } else {
            Err(format!(
                "operation requires {expected}; current status: {status}"
            ))
        }
    }

}

pub(crate) fn validate_record_outputs(
    output: &Path,
    video_output: Option<&Path>,
    instrumentation: Option<&RecordBattleInstrumentationParameters>,
) -> Result<(), Value> {
    if !output.is_absolute() {
        return Err(error_body("record_battle output must be an absolute path"));
    }
    if output.extension().and_then(|value| value.to_str()) != Some("mcfr") {
        return Err(error_body(
            "record_battle output must use the .mcfr extension",
        ));
    }
    if output.exists() {
        return Err(error_body(format!(
            "record_battle refuses to overwrite {}",
            output.display()
        )));
    }
    if let Some(video_output) = video_output {
        if !video_output.is_absolute() {
            return Err(error_body(
                "record_battle video_output must be an absolute path",
            ));
        }
        if video_output.extension().and_then(|value| value.to_str()) != Some("mov") {
            return Err(error_body(
                "record_battle video_output must use the .mov extension",
            ));
        }
        if video_output == output {
            return Err(error_body(
                "record_battle output and video_output must differ",
            ));
        }
        if video_output.exists() {
            return Err(error_body(format!(
                "record_battle refuses to overwrite {}",
                video_output.display()
            )));
        }
    }
    if let Some(instrumentation) = instrumentation {
        if let Some(scope) = &instrumentation.rvo_scope
            && (instrumentation.profile != "target_refs_rvo_v1"
                || scope.start_tick == 0
                || scope.start_tick > scope.end_tick
                || scope.end_tick - scope.start_tick >= 64
                || scope.unit_ids.is_empty()
                || scope.unit_ids.len() > 8
                || scope.unit_ids.contains(&0)
                || scope
                    .unit_ids
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    != scope.unit_ids.len())
        {
            return Err(error_body(
                "rvo_scope requires target_refs_rvo_v1, 1..=8 unique positive MCFR unit_ids, and 1..=64 inclusive positive MCFR ticks",
            ));
        }
        if instrumentation.profile.trim().is_empty() || instrumentation.profile.contains('\0') {
            return Err(error_body(
                "record_battle instrumentation profile is invalid",
            ));
        }
        if !instrumentation.output.is_absolute() {
            return Err(error_body(
                "record_battle instrumentation output must be absolute",
            ));
        }
        if instrumentation
            .output
            .extension()
            .and_then(|value| value.to_str())
            != Some("h5")
        {
            return Err(error_body(
                "record_battle instrumentation output must use the .h5 extension",
            ));
        }
        if instrumentation.output == output
            || video_output == Some(instrumentation.output.as_path())
        {
            return Err(error_body("record_battle output paths must differ"));
        }
        if instrumentation.output.exists() {
            return Err(error_body(format!(
                "record_battle refuses to overwrite {}",
                instrumentation.output.display()
            )));
        }
    }
    Ok(())
}
pub(crate) fn record_battle_failure(
    error: &str,
    operation: &Value,
    layout_input: &Value,
    cleanup: Result<Value, String>,
    observed_status: Value,
) -> Value {
    let recorded = operation.get("recorded").cloned().unwrap_or(Value::Null);
    let (cleanup, status, required, allowed, external_resolution_required) = match cleanup {
        Ok(cleanup) => (
            json!({
                "match_exited": true,
                "game_reusable": true,
                "operation": cleanup["operation"].clone(),
            }),
            cleanup["status"].clone(),
            Value::Null,
            json!(["start_test", "quit_game"]),
            false,
        ),
        Err(cleanup_error) => {
            let at_main_menu = is_status(&observed_status, "main_menu");
            let in_match = matches!(
                observed_status.get("status").and_then(Value::as_str),
                Some("training_ground" | "replay")
            );
            let required = if in_match {
                json!("quit_match")
            } else {
                Value::Null
            };
            let allowed = if at_main_menu {
                json!(["start_test", "quit_game"])
            } else if in_match {
                json!(["quit_match"])
            } else {
                json!(["status"])
            };
            (
                json!({
                    "match_exited": at_main_menu,
                    "game_reusable": at_main_menu,
                    "error": cleanup_error,
                }),
                observed_status,
                required,
                allowed,
                !at_main_menu && !in_match,
            )
        }
    };
    json!({
        "error": error,
        "recorded": recorded,
        "operation": operation,
        "layout_input": layout_input,
        "cleanup": cleanup,
        "status": status,
        "next": {
            "required": required,
            "allowed": allowed,
            "external_resolution_required": external_resolution_required,
        },
        "retry_safe": false,
    })
}
/// `MECHCORE_ADAPTER_SOCKET`, then the user-scoped default.
///
/// Both sides resolve the endpoint the same way, so an override moves the
/// Adapter and every client together. See `docs/session.md`.
pub(crate) fn default_adapter_socket() -> PathBuf {
    if let Some(configured) = std::env::var_os("MECHCORE_ADAPTER_SOCKET") {
        let path = PathBuf::from(configured);
        if path.is_absolute() && path.as_os_str().as_encoded_bytes().len() <= 100 {
            return path;
        }
    }
    // SAFETY: geteuid has no preconditions.
    let uid = unsafe { libc::geteuid() };
    PathBuf::from(format!("/tmp/mechcore-adapter-{uid}.sock"))
}

pub(crate) fn is_status(value: &Value, expected: &str) -> bool {
    value.get("status").and_then(Value::as_str) == Some(expected)
}

pub(crate) fn is_training_deployment(value: &Value) -> bool {
    is_status(value, "training_ground")
        && value.get("deploying").and_then(Value::as_bool) == Some(true)
        && value.get("fighting").and_then(Value::as_bool) == Some(false)
}

pub(crate) fn is_training_state(value: &Value, round: i64, deploying: bool, fighting: bool) -> bool {
    is_status(value, "training_ground")
        && value.get("round_count").and_then(Value::as_i64) == Some(round)
        && value.get("deploying").and_then(Value::as_bool) == Some(deploying)
        && value.get("fighting").and_then(Value::as_bool) == Some(fighting)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_battle_failure_explains_required_match_cleanup() {
        let failure = record_battle_failure(
            "cleanup failed",
            &json!({"recorded": true}),
            &json!({"round": 1, "sides": {}}),
            Err("native exit failed".into()),
            json!({"status": "training_ground", "fighting": false}),
        );
        assert_eq!(failure["recorded"], true);
        assert_eq!(failure["cleanup"]["match_exited"], false);
        assert_eq!(failure["next"]["required"], "quit_match");
        assert_eq!(failure["next"]["allowed"], json!(["quit_match"]));
        assert_eq!(failure["retry_safe"], false);
    }

    #[test]
    fn default_socket_is_absolute_and_user_scoped() {
        let path = default_adapter_socket();
        assert!(path.is_absolute());
        assert!(path.starts_with("/tmp"));
    }

    #[test]
    fn recognizes_training_states() {
        let deployment = json!({
            "status": "training_ground",
            "round_count": 1,
            "deploying": true,
            "fighting": false,
        });
        assert!(is_training_deployment(&deployment));
        assert!(is_training_state(&deployment, 1, true, false));
        assert!(!is_training_state(&deployment, 2, true, false));
    }

    #[tokio::test]
    async fn apply_layout_runs_shared_validation_before_game_state_checks() {
        let error = Session::new()
            .apply_layout(json!({
                "round": 1,
                "sides": {
                    "blue": {"formations": [{"type": "unknown", "x": 0, "y": -50}]},
                    "red": {"formations": [{"type": "marksman", "x": 0, "y": -50}]}
                }
            }), None)
            .await
            .unwrap_err();

        assert!(error.contains("formation type \"unknown\""));
    }

    #[tokio::test]
    async fn apply_layout_accepts_nonempty_terrains_before_game_state_checks() {
        let error = Session::new()
            .apply_layout(json!({
                "round": 1,
                "sides": {
                    "blue": {
                        "formations": [{"type": "marksman", "x": 0, "y": -50}],
                        "terrains": [{
                            "type": "oil",
                            "positions": [{"x": -60, "y": 40}, {"x": 60, "y": 40}]
                        }]
                    },
                    "red": {"formations": [{"type": "marksman", "x": 0, "y": -50}]}
                }
            }), None)
            .await
            .unwrap_err();

        // Reaching the game-state check is the point: a non-empty terrain list
        // is compiled, not rejected as unsupported.
        assert!(
            error.contains("starts from the main menu"),
            "expected the state precondition, got {error}"
        );
    }
}
