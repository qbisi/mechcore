//! Game session orchestration shared by every `mechcore` frontend.
//!
//! This layer owns the Adapter connection, the status stream, mutation
//! serialization, and the pre/post conditions of each native operation. It
//! speaks only `serde_json::Value` and `String`, so no frontend protocol
//! leaks into it. See `docs/session.md` for the acquisition contract.

use crate::acquire::{self, Mode, Ownership};
use crate::adapter;
use mechcore_protocol::{
    CaptureInstrumentationProfile, MAX_WATCH_MATCH_TIMEOUT_SECONDS, MAX_WATCH_SCENE_WAIT_SECONDS,
    Operation, RecordBattleArguments, RecordBattleInstrumentation, RecordReplayRoundArguments,
    RecordWatchReplayArguments, StartTestArguments,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
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

/// Encodes typed protocol arguments for one adapter request.
///
/// Requests travel as JSON, but their shape is the protocol's, not this
/// module's. Building them from the shared types is what keeps a field this
/// side invents from reaching an adapter that rejects unknown fields, which is
/// otherwise only discoverable with the game running.
fn arguments<T: Serialize>(value: &T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| format!("cannot encode request arguments: {error}"))
}

pub(crate) struct Session {
    socket_path: PathBuf,
    adapter: Mutex<Option<adapter::Client>>,
    operation: Mutex<()>,
    last_applied_layout: Mutex<Option<Value>>,
    status: watch::Sender<Value>,
    /// Whether the game went to a higher claim.
    ///
    /// It is the one disconnection that must not be answered by shutting the
    /// game down: the adapter is holding that process at the main menu for
    /// whoever claimed it.
    evicted: AtomicBool,
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
            evicted: AtomicBool::new(false),
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

    /// Whether this session lost the game to a higher claim.
    pub(crate) fn was_evicted(&self) -> bool {
        self.evicted.load(Ordering::SeqCst)
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
            Operation::RecordWatchReplay => {
                let scene = arguments
                    .get("wait_for_scene_seconds")
                    .and_then(Value::as_u64)
                    .unwrap_or(MAX_WATCH_SCENE_WAIT_SECONDS)
                    .min(MAX_WATCH_SCENE_WAIT_SECONDS);
                let battle = arguments
                    .get("match_timeout_seconds")
                    .and_then(Value::as_u64)
                    .unwrap_or(MAX_WATCH_MATCH_TIMEOUT_SECONDS)
                    .min(MAX_WATCH_MATCH_TIMEOUT_SECONDS);
                Duration::from_secs(scene.saturating_add(battle).saturating_add(480))
            }
            _ => ADAPTER_REQUEST_TIMEOUT,
        };
        match tokio::time::timeout(request_timeout, client.request(operation, arguments)).await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => {
                let fatal = error.is_fatal();
                if error.is_evicted() {
                    self.evicted.store(true, Ordering::SeqCst);
                }
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
    pub(crate) async fn acquire(
        self: &Arc<Self>,
        mode: Mode,
        level: u8,
    ) -> Result<Ownership, String> {
        let (client, ownership) = acquire::acquire(mode, level, self.endpoint())
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
    ///
    /// An evicted session releases nothing. The adapter took the game and is
    /// keeping that process at the main menu for the client that claimed it,
    /// so quitting or terminating it here would destroy someone else's game.
    pub(crate) async fn release(&self, ownership: Option<Ownership>) -> Result<(), String> {
        let Some(Ownership::Owned { mut child, .. }) = ownership else {
            return Ok(());
        };
        if self.was_evicted() {
            return Ok(());
        }
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

    pub(crate) async fn start_test(
        &self,
        seed: Option<i32>,
        map_id: Option<i32>,
    ) -> Result<Value, String> {
        let _operation = self.operation.lock().await;
        self.require_status("main_menu").await?;
        *self.last_applied_layout.lock().await = None;
        let result = self
            .adapter_request(
                Operation::StartTest,
                arguments(&StartTestArguments { seed, map_id })?,
            )
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
        let plan = mechcore_document::compile(&layout)?;
        if plan.round > mechcore_protocol::MAX_STAGED_ROUND {
            return Err(format!(
                "apply_layout advances through every earlier round inside one timeout budget \
                 and stages at most round {}, so round {} cannot be reached",
                mechcore_protocol::MAX_STAGED_ROUND,
                plan.round
            ));
        }
        let activation_round = i64::from(plan.round);
        let seed = seed.or(plan.seed);
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
            .adapter_request(
                Operation::StartTest,
                arguments(&StartTestArguments {
                    seed,
                    map_id: plan.map_id,
                })?,
            )
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
        force: bool,
        instrumentation: Option<RecordBattleInstrumentation>,
    ) -> Result<Value, Value> {
        let _operation = self.operation.lock().await;
        let before = self.refresh_status().await.map_err(error_body)?;
        if !is_training_deployment(&before) {
            return Err(error_body(format!(
                "record_battle requires completed Training Ground deployment: {before}"
            )));
        }
        validate_record_outputs(
            "record_battle",
            &output,
            video_output.as_deref(),
            instrumentation.as_ref(),
            force,
        )?;
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
                arguments(&RecordBattleArguments {
                    output: output.clone(),
                    video_output: video_output.clone(),
                    speed_up,
                    instrumentation,
                })?,
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
            Some("training_ground" | "replay" | "spectating")
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
        force: bool,
        instrumentation: Option<RecordBattleInstrumentation>,
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
        if round < 1 {
            return Err("record_replay_round round must be at least 1".into());
        }
        if !output.is_absolute()
            || output.extension().and_then(|value| value.to_str()) != Some("mcfr")
        {
            return Err("record_replay_round output must be an absolute .mcfr path".into());
        }
        *self.last_applied_layout.lock().await = None;
        validate_record_outputs(
            "record_replay_round",
            &output,
            None,
            instrumentation.as_ref(),
            force,
        )
        .map_err(|error| error.to_string())?;
        let result = self
            .adapter_request(
                Operation::RecordReplayRound,
                arguments(&RecordReplayRoundArguments {
                    grbr: grbr.clone(),
                    round,
                    output: output.clone(),
                    speed_up,
                    instrumentation,
                })?,
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

    /// Watch one live round-one matchmaking battle and publish its native GRBR.
    ///
    /// Scene selection, saving and cleanup are one adapter transaction, and the
    /// file it names is the one the game itself wrote. The batch cannot start
    /// its next iteration until this operation is back at the main menu.
    pub(crate) async fn record_watch_replay(
        &self,
        output_dir: Option<PathBuf>,
        wait_for_scene_seconds: u64,
        match_timeout_seconds: u64,
    ) -> Result<Value, String> {
        let output_dir = prepare_watch_output(
            output_dir.as_deref(),
            wait_for_scene_seconds,
            match_timeout_seconds,
        )?;
        let _operation = self.operation.lock().await;
        self.require_status("main_menu").await?;
        *self.last_applied_layout.lock().await = None;

        let result = self
            .adapter_request(
                Operation::RecordWatchReplay,
                arguments(&RecordWatchReplayArguments {
                    output_dir: output_dir.clone(),
                    wait_for_scene_seconds,
                    match_timeout_seconds,
                })?,
            )
            .await?;
        if result.get("recorded").and_then(Value::as_bool) != Some(true) {
            return Err(format!(
                "adapter did not confirm watched replay recording: {result}"
            ));
        }
        let status = self.refresh_status().await?;
        if !is_status(&status, "main_menu") {
            return Err(format!(
                "record_watch_replay completed outside main_menu: {status}"
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
            Some("training_ground" | "replay" | "spectating")
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

fn prepare_watch_output(
    output_dir: Option<&Path>,
    wait_for_scene_seconds: u64,
    match_timeout_seconds: u64,
) -> Result<Option<PathBuf>, String> {
    if !(1..=MAX_WATCH_SCENE_WAIT_SECONDS).contains(&wait_for_scene_seconds) {
        return Err(format!(
            "record_watch_replay wait_for_scene_seconds must be 1..={MAX_WATCH_SCENE_WAIT_SECONDS}"
        ));
    }
    if !(60..=MAX_WATCH_MATCH_TIMEOUT_SECONDS).contains(&match_timeout_seconds) {
        return Err(format!(
            "record_watch_replay match_timeout_seconds must be 60..={MAX_WATCH_MATCH_TIMEOUT_SECONDS}"
        ));
    }
    let Some(output_dir) = output_dir else {
        return Ok(None);
    };
    if !output_dir.is_absolute() {
        return Err("record_watch_replay output_dir must be absolute".into());
    }
    std::fs::create_dir_all(output_dir).map_err(|error| {
        format!(
            "cannot create GRBR corpus directory {}: {error}",
            output_dir.display()
        )
    })?;
    let output_dir = output_dir.canonicalize().map_err(|error| {
        format!(
            "cannot resolve GRBR corpus directory {}: {error}",
            output_dir.display()
        )
    })?;
    if !output_dir.is_dir() {
        return Err(format!(
            "record_watch_replay output_dir is not a directory: {}",
            output_dir.display()
        ));
    }
    Ok(Some(output_dir))
}

/// Refuse an existing destination, or remove it when the caller asked to.
fn remove_existing_output(path: &Path, force: bool, what: &str) -> Result<(), Value> {
    if !path.exists() {
        return Ok(());
    }
    if !force {
        return Err(error_body(format!(
            "{what} refuses to overwrite {}; pass force to replace it",
            path.display()
        )));
    }
    if !path.is_file() {
        return Err(error_body(format!(
            "{what} {} exists and is not a file",
            path.display()
        )));
    }
    std::fs::remove_file(path)
        .map_err(|error| error_body(format!("cannot replace {}: {error}", path.display())))
}

/// Validate the destinations a recording will publish.
///
/// `force` removes an existing destination here rather than relaxing the
/// Adapter, which keeps refusing to overwrite. Deleting is the caller's
/// declared intent; overwriting would be the Adapter deciding on its own.
pub(crate) fn validate_record_outputs(
    operation: &str,
    output: &Path,
    video_output: Option<&Path>,
    instrumentation: Option<&RecordBattleInstrumentation>,
    force: bool,
) -> Result<(), Value> {
    if !output.is_absolute() {
        return Err(error_body(format!(
            "{operation} output must be an absolute path"
        )));
    }
    if output.extension().and_then(|value| value.to_str()) != Some("mcfr") {
        return Err(error_body(format!(
            "{operation} output must use the .mcfr extension"
        )));
    }
    remove_existing_output(output, force, &format!("{operation} output"))?;
    if let Some(video_output) = video_output {
        if !video_output.is_absolute() {
            return Err(error_body(format!(
                "{operation} video_output must be an absolute path"
            )));
        }
        if video_output.extension().and_then(|value| value.to_str()) != Some("mov") {
            return Err(error_body(format!(
                "{operation} video_output must use the .mov extension"
            )));
        }
        if video_output == output {
            return Err(error_body(format!(
                "{operation} output and video_output must differ"
            )));
        }
        remove_existing_output(video_output, force, &format!("{operation} video_output"))?;
    }
    if let Some(instrumentation) = instrumentation {
        if let Some(scope) = &instrumentation.rvo_scope
            && (instrumentation.profile != CaptureInstrumentationProfile::TargetRefsRvoV1
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
                Some("training_ground" | "replay" | "spectating")
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

pub(crate) fn is_training_state(
    value: &Value,
    round: i64,
    deploying: bool,
    fighting: bool,
) -> bool {
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
            &json!({"kind": "layout", "round": 1, "sides": {}}),
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

    #[test]
    fn an_existing_output_is_refused_unless_the_caller_forces_it() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("battle.mcfr");
        std::fs::write(&output, b"existing").unwrap();

        let refused =
            validate_record_outputs("record_battle", &output, None, None, false).unwrap_err();
        assert!(
            refused["error"]
                .as_str()
                .unwrap()
                .contains("refuses to overwrite"),
            "{refused}"
        );
        assert!(output.exists(), "a refusal must leave the file alone");

        validate_record_outputs("record_battle", &output, None, None, true).unwrap();
        assert!(!output.exists(), "force removes the destination up front");
    }

    #[tokio::test]
    async fn apply_layout_runs_shared_validation_before_game_state_checks() {
        let error = Session::new()
            .apply_layout(
                json!({
                    "kind": "layout",
                    "round": 1,
                    "sides": {
                        "blue": {"formations": [{"type": "unknown", "index": 0, "position": {"x": 0, "y": -50}}]},
                        "red": {"formations": [{"type": "marksman", "index": 0, "position": {"x": 0, "y": -50}}]}
                    }
                }),
                None,
            )
            .await
            .unwrap_err();

        assert!(error.contains("formation type \"unknown\""));
    }

    #[tokio::test]
    async fn apply_layout_accepts_nonempty_terrains_before_game_state_checks() {
        let error = Session::new()
            .apply_layout(
                json!({
                    "kind": "layout",
                    "round": 1,
                    "sides": {
                        "blue": {
                            "formations": [{"type": "marksman", "index": 0, "position": {"x": 0, "y": -50}}],
                            "terrains": [{
                                "type": "oil",
                                "control_points": [{"x": -60, "y": 40}, {"x": 60, "y": 40}]
                            }]
                        },
                        "red": {"formations": [{"type": "marksman", "index": 0, "position": {"x": 0, "y": -50}}]}
                    }
                }),
                None,
            )
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
