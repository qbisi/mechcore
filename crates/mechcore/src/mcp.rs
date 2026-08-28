use crate::adapter;
use mechcore_protocol::Operation;
use rmcp::{
    ErrorData, RoleServer, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolResult, Implementation, ListResourcesResult, PaginatedRequestParams, RawResource,
        ReadResourceRequestParams, ReadResourceResult, Resource, ResourceContents,
        ResourceUpdatedNotificationParam, ServerCapabilities, ServerInfo, SubscribeRequestParams,
        UnsubscribeRequestParams,
    },
    schemars::JsonSchema,
    service::RequestContext,
    tool, tool_handler, tool_router,
    transport::stdio,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    sync::{Mutex, watch},
    task::JoinHandle,
    time::{Instant, sleep, timeout_at},
};

const STATUS_URI: &str = "mechcore://status";
const STATUS_INTERVAL: Duration = Duration::from_millis(100);
const ADAPTER_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);
const TRANSITION_TIMEOUT: Duration = Duration::from_secs(60);

type ApplyLayoutParameters = mechcore_layout::Layout;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct StartTestParameters {
    /// Native match seed. Zero or omission lets the game generate one.
    seed: Option<i32>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RecordBattleParameters {
    /// Absolute destination path for the new `.mcfr` file.
    output: PathBuf,
    /// Optional absolute destination for a logic-frame-aligned `QuickTime` MJPEG `.mov`.
    video_output: Option<PathBuf>,
    /// Optional temporary Adapter-native research sidecar.
    instrumentation: Option<RecordBattleInstrumentationParameters>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RecordBattleInstrumentationParameters {
    /// Absolute destination path for the new HDF5 instrumentation sidecar.
    output: PathBuf,
    /// Adapter-defined temporary research profile name.
    profile: String,
}

struct Shared {
    socket_path: PathBuf,
    adapter: Mutex<Option<adapter::Client>>,
    operation: Mutex<()>,
    last_applied_layout: Mutex<Option<Value>>,
    status: watch::Sender<Value>,
}

impl Shared {
    fn new() -> Arc<Self> {
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

    fn current_status(&self) -> Value {
        self.status.borrow().clone()
    }

    fn publish(&self, status: Value) {
        if *self.status.borrow() != status {
            self.status.send_replace(status);
        }
    }

    async fn disconnect_adapter(&self) {
        *self.adapter.lock().await = None;
        self.publish(json!({"status": "game_off"}));
    }

    async fn adapter_request(
        &self,
        operation: Operation,
        arguments: Value,
    ) -> Result<Value, String> {
        let mut adapter = self.adapter.lock().await;
        let client = adapter.as_mut().ok_or_else(|| {
            "game adapter is not connected; call connect_adapter first".to_owned()
        })?;
        let request_timeout = if operation == Operation::RecordBattle {
            Duration::from_secs(180)
        } else {
            ADAPTER_REQUEST_TIMEOUT
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

    async fn refresh_status(&self) -> Result<Value, String> {
        let status = self.adapter_request(Operation::Status, json!({})).await?;
        self.publish(status.clone());
        Ok(status)
    }

    async fn wait_status(
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

    async fn connect_adapter(&self) -> Result<Value, String> {
        let _operation = self.operation.lock().await;
        if self.adapter.lock().await.is_some() {
            return Err("game adapter is already connected".into());
        }
        let deadline = Instant::now() + CONNECT_TIMEOUT;
        loop {
            let attempt = tokio::time::timeout(
                Duration::from_secs(1),
                adapter::Client::connect(&self.socket_path),
            )
            .await;
            if let Ok(Ok(client)) = attempt {
                *self.adapter.lock().await = Some(client);
                break;
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "timed out waiting for the Adapter at {}",
                    self.socket_path.display()
                ));
            }
            sleep(STATUS_INTERVAL).await;
        }
        let status = self.refresh_status().await?;
        Ok(json!({"connected": true, "status": status}))
    }

    async fn start_test(&self, seed: Option<i32>) -> Result<Value, String> {
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

    async fn apply_layout(&self, layout: Value) -> Result<Value, String> {
        let plan = mechcore_layout::compile(&layout)?;
        let activation_round = i64::from(plan.round);
        let _operation = self.operation.lock().await;
        self.require_training_deployment(1).await?;
        *self.last_applied_layout.lock().await = None;
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
        Ok(json!({"operation": result, "status": status}))
    }

    async fn record_battle(
        &self,
        output: PathBuf,
        video_output: Option<PathBuf>,
        instrumentation: Option<RecordBattleInstrumentationParameters>,
    ) -> Result<Value, Value> {
        let _operation = self.operation.lock().await;
        let before = self.refresh_status().await.map_err(tool_error)?;
        if !is_training_deployment(&before) {
            return Err(tool_error(format!(
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
                tool_error(
                    "record_battle requires a successfully applied layout in this test session",
                )
            })?;
        let result = match self
            .adapter_request(
                Operation::RecordBattle,
                json!({
                    "output": output,
                    "video_output": video_output,
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

    async fn finish_recording_match(&self) -> Result<Value, String> {
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

    async fn toggle_fight(&self) -> Result<Value, String> {
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

    async fn speed_up(&self) -> Result<Value, String> {
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

    async fn quit_match(&self) -> Result<Value, String> {
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

    async fn leave_active_match(&self) -> Result<Value, String> {
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

    async fn quit_game(&self) -> Result<Value, String> {
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

    async fn require_status(&self, expected: &str) -> Result<Value, String> {
        let status = self.refresh_status().await?;
        if is_status(&status, expected) {
            Ok(status)
        } else {
            Err(format!(
                "operation requires {expected}; current status: {status}"
            ))
        }
    }

    async fn require_training_deployment(&self, expected_round: i64) -> Result<Value, String> {
        let status = self.refresh_status().await?;
        if is_training_state(&status, expected_round, true, false) {
            Ok(status)
        } else {
            Err(format!(
                "operation requires round-{expected_round} Training Ground deployment: {status}"
            ))
        }
    }
}

#[derive(Clone)]
struct MechcoreMcp {
    shared: Arc<Shared>,
    tool_router: ToolRouter<Self>,
    subscription: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl MechcoreMcp {
    fn new(shared: Arc<Shared>) -> Self {
        Self {
            shared,
            tool_router: Self::tool_router(),
            subscription: Arc::new(Mutex::new(None)),
        }
    }
}

#[tool_router]
impl MechcoreMcp {
    #[tool(description = "Return the current Mechabellum status snapshot")]
    async fn status(&self) -> Result<CallToolResult, ErrorData> {
        Ok(tool_result(Ok(self.shared.current_status())))
    }

    #[tool(description = "Connect to the Adapter in an externally launched Mechabellum process")]
    async fn connect_adapter(&self) -> Result<CallToolResult, ErrorData> {
        Ok(tool_result(self.shared.connect_adapter().await))
    }

    #[tool(
        description = "Create the fixed Training Ground test mode with an optional native match seed and wait for round-one deployment; zero or omission lets the game generate one"
    )]
    async fn start_test(
        &self,
        Parameters(parameters): Parameters<StartTestParameters>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(tool_result(self.shared.start_test(parameters.seed).await))
    }

    #[tool(description = "Apply a staged layout and advance to its activation-round deployment")]
    async fn apply_layout(
        &self,
        Parameters(parameters): Parameters<ApplyLayoutParameters>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(tool_result(
            self.shared
                .apply_layout(json!({"round": parameters.round, "sides": parameters.sides}))
                .await,
        ))
    }

    #[tool(
        description = "Record and verify the deployed Training Ground battle, then leave the completed match and return only from main_menu. Optional video uses render-synchronized pacing. This tool never quits the game; continuous captures reuse the main-menu process with start_test, while quit_game ends the session."
    )]
    async fn record_battle(
        &self,
        Parameters(parameters): Parameters<RecordBattleParameters>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(record_tool_result(
            self.shared
                .record_battle(
                    parameters.output,
                    parameters.video_output,
                    parameters.instrumentation,
                )
                .await,
        ))
    }

    #[tool(description = "Start the current Training Ground fight and wait for the transition")]
    async fn toggle_fight(&self) -> Result<CallToolResult, ErrorData> {
        Ok(tool_result(self.shared.toggle_fight().await))
    }

    #[tool(description = "Request battle speed-up after confirming that a fight is active")]
    async fn speed_up(&self) -> Result<CallToolResult, ErrorData> {
        Ok(tool_result(self.shared.speed_up().await))
    }

    #[tool(description = "Leave the active test or replay and wait for the main menu")]
    async fn quit_match(&self) -> Result<CallToolResult, ErrorData> {
        Ok(tool_result(self.shared.quit_match().await))
    }

    #[tool(
        description = "Request game shutdown from the main menu and wait for Adapter disconnect"
    )]
    async fn quit_game(&self) -> Result<CallToolResult, ErrorData> {
        Ok(tool_result(self.shared.quit_game().await))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for MechcoreMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_resources_subscribe()
                .build(),
            instructions: Some(
                "Launch Mechabellum with the Adapter outside MCP and call connect_adapter. Each capture uses start_test, apply_layout, then record_battle; a successful record_battle guarantees main_menu without quitting the game, so start_test may begin the next capture. Use quit_match only to leave a manually active test/replay or to recover a reported cleanup obligation, and use quit_game only when the capture session ends. Subscribe to mechcore://status for state changes."
                    .into(),
            ),
            server_info: Implementation {
                name: "mechcore".into(),
                title: Some("Mechcore MCP".into()),
                version: env!("CARGO_PKG_VERSION").into(),
                description: Some("Mechabellum layout-test lifecycle server".into()),
                icons: None,
                website_url: None,
            },
            ..Default::default()
        }
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let mut raw = RawResource::new(STATUS_URI, "Mechabellum status");
        raw.description = Some("Continuously updated authoritative game status snapshot".into());
        raw.mime_type = Some("application/json".into());
        Ok(ListResourcesResult::with_all_items(vec![Resource::new(
            raw, None,
        )]))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, ErrorData> {
        validate_status_uri(&request.uri)?;
        let text = serde_json::to_string(&self.shared.current_status())
            .map_err(|error| ErrorData::internal_error(error.to_string(), None))?;
        Ok(ReadResourceResult {
            contents: vec![ResourceContents::TextResourceContents {
                uri: STATUS_URI.into(),
                mime_type: Some("application/json".into()),
                text,
                meta: None,
            }],
        })
    }

    async fn subscribe(
        &self,
        request: SubscribeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        validate_status_uri(&request.uri)?;
        let mut receiver = self.shared.status.subscribe();
        let peer = context.peer;
        let mut subscription = self.subscription.lock().await;
        if let Some(previous) = subscription.take() {
            previous.abort();
        }
        *subscription = Some(tokio::spawn(async move {
            if peer
                .notify_resource_updated(ResourceUpdatedNotificationParam {
                    uri: STATUS_URI.into(),
                })
                .await
                .is_err()
            {
                return;
            }
            while receiver.changed().await.is_ok() {
                if peer
                    .notify_resource_updated(ResourceUpdatedNotificationParam {
                        uri: STATUS_URI.into(),
                    })
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }));
        Ok(())
    }

    async fn unsubscribe(
        &self,
        request: UnsubscribeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        validate_status_uri(&request.uri)?;
        if let Some(subscription) = self.subscription.lock().await.take() {
            subscription.abort();
        }
        Ok(())
    }
}

pub fn run() -> Result<(), String> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("cannot create async runtime: {error}"))?
        .block_on(run_async())
}

async fn run_async() -> Result<(), String> {
    let shared = Shared::new();
    let monitor = tokio::spawn(monitor(shared.clone()));
    let server = MechcoreMcp::new(shared.clone())
        .serve(stdio())
        .await
        .map_err(|error| format!("cannot start MCP stdio service: {error}"))?;
    let result = server
        .waiting()
        .await
        .map_err(|error| format!("MCP service failed: {error}"));
    monitor.abort();
    result.map(drop)
}

async fn monitor(shared: Arc<Shared>) {
    loop {
        let connected = shared.adapter.lock().await.is_some();
        if connected {
            match shared.adapter_request(Operation::Status, json!({})).await {
                Ok(status) => shared.publish(status),
                Err(_) => {
                    shared.disconnect_adapter().await;
                }
            }
        }
        sleep(STATUS_INTERVAL).await;
    }
}

fn tool_result(result: Result<Value, String>) -> CallToolResult {
    match result {
        Ok(value) => CallToolResult::structured(value),
        Err(error) => CallToolResult::structured_error(json!({"error": error})),
    }
}

fn record_tool_result(result: Result<Value, Value>) -> CallToolResult {
    match result {
        Ok(value) => CallToolResult::structured(value),
        Err(error) => CallToolResult::structured_error(error),
    }
}

fn tool_error(error: impl Into<String>) -> Value {
    json!({"error": error.into()})
}

fn validate_record_outputs(
    output: &Path,
    video_output: Option<&Path>,
    instrumentation: Option<&RecordBattleInstrumentationParameters>,
) -> Result<(), Value> {
    if !output.is_absolute() {
        return Err(tool_error("record_battle output must be an absolute path"));
    }
    if output.extension().and_then(|value| value.to_str()) != Some("mcfr") {
        return Err(tool_error(
            "record_battle output must use the .mcfr extension",
        ));
    }
    if output.exists() {
        return Err(tool_error(format!(
            "record_battle refuses to overwrite {}",
            output.display()
        )));
    }
    if let Some(video_output) = video_output {
        if !video_output.is_absolute() {
            return Err(tool_error(
                "record_battle video_output must be an absolute path",
            ));
        }
        if video_output.extension().and_then(|value| value.to_str()) != Some("mov") {
            return Err(tool_error(
                "record_battle video_output must use the .mov extension",
            ));
        }
        if video_output == output {
            return Err(tool_error(
                "record_battle output and video_output must differ",
            ));
        }
        if video_output.exists() {
            return Err(tool_error(format!(
                "record_battle refuses to overwrite {}",
                video_output.display()
            )));
        }
    }
    if let Some(instrumentation) = instrumentation {
        if instrumentation.profile.trim().is_empty() || instrumentation.profile.contains('\0') {
            return Err(tool_error(
                "record_battle instrumentation profile is invalid",
            ));
        }
        if !instrumentation.output.is_absolute() {
            return Err(tool_error(
                "record_battle instrumentation output must be absolute",
            ));
        }
        if instrumentation
            .output
            .extension()
            .and_then(|value| value.to_str())
            != Some("h5")
        {
            return Err(tool_error(
                "record_battle instrumentation output must use the .h5 extension",
            ));
        }
        if instrumentation.output == output
            || video_output == Some(instrumentation.output.as_path())
        {
            return Err(tool_error("record_battle output paths must differ"));
        }
        if instrumentation.output.exists() {
            return Err(tool_error(format!(
                "record_battle refuses to overwrite {}",
                instrumentation.output.display()
            )));
        }
    }
    Ok(())
}

fn record_battle_failure(
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

fn validate_status_uri(uri: &str) -> Result<(), ErrorData> {
    if uri == STATUS_URI {
        Ok(())
    } else {
        Err(ErrorData::invalid_params(
            format!("unknown resource URI: {uri}"),
            None,
        ))
    }
}

fn default_adapter_socket() -> PathBuf {
    // SAFETY: geteuid has no preconditions.
    let uid = unsafe { libc::geteuid() };
    PathBuf::from(format!("/tmp/mechcore-adapter-{uid}.sock"))
}

fn is_status(value: &Value, expected: &str) -> bool {
    value.get("status").and_then(Value::as_str) == Some(expected)
}

fn is_training_deployment(value: &Value) -> bool {
    is_status(value, "training_ground")
        && value.get("deploying").and_then(Value::as_bool) == Some(true)
        && value.get("fighting").and_then(Value::as_bool) == Some(false)
}

fn is_training_state(value: &Value, round: i64, deploying: bool, fighting: bool) -> bool {
    is_status(value, "training_ground")
        && value.get("round_count").and_then(Value::as_i64) == Some(round)
        && value.get("deploying").and_then(Value::as_bool) == Some(deploying)
        && value.get("fighting").and_then(Value::as_bool) == Some(fighting)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_exposes_exact_tool_surface() {
        let shared = Shared::new();
        let mut names: Vec<_> = MechcoreMcp::new(shared)
            .tool_router
            .list_all()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "apply_layout",
                "connect_adapter",
                "quit_game",
                "quit_match",
                "record_battle",
                "speed_up",
                "start_test",
                "status",
                "toggle_fight",
            ]
        );
    }

    #[test]
    fn record_battle_tool_declares_its_main_menu_postcondition() {
        let shared = Shared::new();
        let tool = MechcoreMcp::new(shared)
            .tool_router
            .list_all()
            .into_iter()
            .find(|tool| tool.name == "record_battle")
            .expect("record_battle tool exists");
        let description = tool.description.expect("record_battle has a description");
        assert!(description.contains("main_menu"));
        assert!(description.contains("never quits the game"));
        assert!(description.contains("continuous captures"));
    }

    #[test]
    fn start_test_tool_exposes_optional_seed() {
        let shared = Shared::new();
        let tool = MechcoreMcp::new(shared)
            .tool_router
            .list_all()
            .into_iter()
            .find(|tool| tool.name == "start_test")
            .expect("start_test tool exists");
        let schema = Value::Object(tool.input_schema.as_ref().clone());

        assert!(schema.pointer("/properties/seed").is_some());
        assert!(
            schema
                .get("required")
                .and_then(Value::as_array)
                .is_none_or(|required| !required.contains(&json!("seed")))
        );
    }

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

    #[test]
    fn apply_layout_parameters_require_an_activation_round() {
        assert!(serde_json::from_value::<ApplyLayoutParameters>(json!({"sides": {}})).is_err());
        let parameters = serde_json::from_value::<ApplyLayoutParameters>(json!({
            "round": 3,
            "sides": {
                "blue": {"formations": [{"type": "marksman", "x": 0, "y": -50}]},
                "red": {"formations": [{"type": "arclight", "x": 0, "y": -50}]}
            }
        }))
        .unwrap();
        assert_eq!(parameters.round, 3);
    }

    #[tokio::test]
    async fn apply_layout_runs_shared_validation_before_game_state_checks() {
        let error = Shared::new()
            .apply_layout(json!({
                "round": 1,
                "sides": {
                    "blue": {"formations": [{"type": "unknown", "x": 0, "y": -50}]},
                    "red": {"formations": [{"type": "marksman", "x": 0, "y": -50}]}
                }
            }))
            .await
            .unwrap_err();

        assert!(error.contains("formation type \"unknown\""));
    }

    #[test]
    fn apply_layout_tool_schema_describes_both_sides_and_formations() {
        let shared = Shared::new();
        let tool = MechcoreMcp::new(shared)
            .tool_router
            .list_all()
            .into_iter()
            .find(|tool| tool.name == "apply_layout")
            .expect("apply_layout tool exists");
        let schema = Value::Object(tool.input_schema.as_ref().clone());

        assert_eq!(
            schema
                .pointer("/properties/sides/$ref")
                .and_then(Value::as_str),
            Some("#/$defs/Sides")
        );
        let required_sides = schema
            .pointer("/$defs/Sides/required")
            .and_then(Value::as_array)
            .expect("sides has a required list");
        assert_eq!(required_sides.as_slice(), [json!("blue"), json!("red")]);
        assert!(
            schema
                .pointer("/$defs/Side/properties/formations")
                .is_some()
        );
        assert!(schema.pointer("/$defs/Formation/properties/type").is_some());
    }
}
