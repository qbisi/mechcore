use crate::adapter;
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
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    env, fs,
    os::unix::fs::FileTypeExt,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    process::{Child, Command},
    sync::{Mutex, watch},
    task::JoinHandle,
    time::{Instant, sleep, timeout_at},
};

const ADAPTER_ENV: &str = "MECHCORE_ADAPTER";
const STATUS_URI: &str = "mechcore://status";
const GAME_EXECUTABLE: &str = "/Users/qbisi/Library/Application Support/Steam/steamapps/common/Mechabellum/Mechabellum.app/Contents/MacOS/Mechabellum";
const STATUS_INTERVAL: Duration = Duration::from_millis(100);
const ADAPTER_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);
const TRANSITION_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ApplyLayoutParameters {
    sides: Value,
}

struct Shared {
    adapter_path: PathBuf,
    socket_path: PathBuf,
    adapter: Mutex<Option<adapter::Client>>,
    child: Mutex<Option<Child>>,
    last_exit_code: Mutex<Option<i32>>,
    operation: Mutex<()>,
    status: watch::Sender<Value>,
}

impl Shared {
    fn new() -> Result<Arc<Self>, String> {
        let adapter_path = match env::var_os(ADAPTER_ENV) {
            Some(path) => PathBuf::from(path),
            None => env::current_exe()
                .map_err(|error| format!("cannot resolve current executable: {error}"))?
                .parent()
                .ok_or_else(|| "current executable has no parent directory".to_owned())?
                .join("libmechcore_adapter.dylib"),
        };
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("system clock precedes Unix epoch: {error}"))?
            .as_nanos();
        let socket_path = PathBuf::from(format!(
            "/tmp/mechcore-mcp-{}-{nonce:x}.sock",
            std::process::id()
        ));
        let (status, _) = watch::channel(json!({"status": "game_off"}));
        Ok(Arc::new(Self {
            adapter_path,
            socket_path,
            adapter: Mutex::new(None),
            child: Mutex::new(None),
            last_exit_code: Mutex::new(None),
            operation: Mutex::new(()),
            status,
        }))
    }

    fn current_status(&self) -> Value {
        self.status.borrow().clone()
    }

    fn publish(&self, status: Value) {
        if *self.status.borrow() != status {
            self.status.send_replace(status);
        }
    }

    async fn adapter_request(&self, operation: &str, arguments: Value) -> Result<Value, String> {
        let mut adapter = self.adapter.lock().await;
        let client = adapter
            .as_mut()
            .ok_or_else(|| "game adapter is not connected; call start_game first".to_owned())?;
        match tokio::time::timeout(
            ADAPTER_REQUEST_TIMEOUT,
            client.request(operation, arguments),
        )
        .await
        {
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
        let status = self.adapter_request("status", json!({})).await?;
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

    async fn reap_child(&self) -> Result<Option<i32>, String> {
        let mut child = self.child.lock().await;
        let Some(process) = child.as_mut() else {
            return Ok(*self.last_exit_code.lock().await);
        };
        let Some(status) = process
            .try_wait()
            .map_err(|error| format!("cannot inspect game process: {error}"))?
        else {
            return Ok(None);
        };
        let code = status.code().unwrap_or(-1);
        *child = None;
        *self.adapter.lock().await = None;
        *self.last_exit_code.lock().await = Some(code);
        self.publish(json!({"status": "game_off"}));
        Ok(Some(code))
    }

    async fn start_game(&self) -> Result<Value, String> {
        let _operation = self.operation.lock().await;
        self.reap_child().await?;
        if self.child.lock().await.is_some() {
            return Err("an MCP-owned game process is already running".into());
        }
        let adapter_path = canonical_file(&self.adapter_path, "adapter")?;
        let game_path = canonical_file(Path::new(GAME_EXECUTABLE), "game executable")?;
        let mut command = Command::new(game_path);
        command
            .env("DYLD_INSERT_LIBRARIES", adapter_path)
            .env("MECHCORE_ADAPTER_SOCKET", &self.socket_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let child = command
            .spawn()
            .map_err(|error| format!("cannot launch Mechabellum: {error}"))?;
        *self.last_exit_code.lock().await = None;
        *self.child.lock().await = Some(child);
        self.publish(json!({"status": "starting_game"}));

        let readiness = async {
            let deadline = Instant::now() + CONNECT_TIMEOUT;
            loop {
                if let Some(code) = self.reap_child().await? {
                    return Err(format!(
                        "game exited with code {code} before adapter readiness"
                    ));
                }
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
                    return Err(
                        "timed out waiting for the adapter socket and hello handshake".into(),
                    );
                }
                sleep(STATUS_INTERVAL).await;
            }

            let status = self.refresh_status().await?;
            if is_status(&status, "main_menu") {
                Ok(status)
            } else {
                self.wait_status("main menu after start_game", CONNECT_TIMEOUT, |value| {
                    is_status(value, "main_menu")
                })
                .await
            }
        }
        .await;
        match readiness {
            Ok(status) => Ok(json!({"started": true, "status": status})),
            Err(error) => match self.stop_owned_game().await {
                Ok(()) => Err(error),
                Err(stop_error) => Err(format!(
                    "{error}; additionally failed to stop the owned game: {stop_error}"
                )),
            },
        }
    }

    async fn start_test(&self) -> Result<Value, String> {
        let _operation = self.operation.lock().await;
        self.require_status("main_menu").await?;
        let result = self.adapter_request("start_test", json!({})).await?;
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
        let _operation = self.operation.lock().await;
        self.require_training_deployment(1).await?;
        let result = self.adapter_request("apply_layout", layout).await?;
        if result.get("applied").and_then(Value::as_bool) != Some(true) {
            return Err(format!(
                "adapter did not confirm layout application: {result}"
            ));
        }
        let status = self.refresh_status().await?;
        if !is_training_state(&status, 1, true, false) {
            return Err(format!(
                "layout completed outside round-one deployment: {status}"
            ));
        }
        Ok(json!({"operation": result, "status": status}))
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
        let result = self.adapter_request("toggle_fight", json!({})).await?;
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
        let result = self.adapter_request("speed_up", json!({})).await?;
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
        let result = self.adapter_request("quit_match", json!({})).await?;
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
        let result = self.adapter_request("quit_game", json!({})).await?;
        let status = self
            .wait_status(
                "game process exit after quit_game",
                TRANSITION_TIMEOUT,
                |value| is_status(value, "game_off"),
            )
            .await?;
        let exit_code = self
            .last_exit_code
            .lock()
            .await
            .ok_or_else(|| "game_off was observed without an owned process exit".to_owned())?;
        if exit_code != 0 {
            return Err(format!("game exited with code {exit_code}"));
        }
        Ok(json!({
            "operation": result,
            "exit_code": exit_code,
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

    async fn stop_owned_game(&self) -> Result<(), String> {
        let mut child = self.child.lock().await;
        let Some(process) = child.as_mut() else {
            return Ok(());
        };
        process
            .start_kill()
            .map_err(|error| format!("cannot stop owned game process: {error}"))?;
        let status = process
            .wait()
            .await
            .map_err(|error| format!("cannot wait for owned game process: {error}"))?;
        let code = status.code().unwrap_or(-1);
        *child = None;
        *self.adapter.lock().await = None;
        *self.last_exit_code.lock().await = Some(code);
        self.publish(json!({"status": "game_off"}));
        Ok(())
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

    #[tool(description = "Launch Mechabellum with the adapter and wait for the main menu")]
    async fn start_game(&self) -> Result<CallToolResult, ErrorData> {
        Ok(tool_result(self.shared.start_game().await))
    }

    #[tool(
        description = "Create the fixed Training Ground test mode and wait for round-one deployment"
    )]
    async fn start_test(&self) -> Result<CallToolResult, ErrorData> {
        Ok(tool_result(self.shared.start_test().await))
    }

    #[tool(description = "Apply a complete layout during round-one deployment")]
    async fn apply_layout(
        &self,
        Parameters(parameters): Parameters<ApplyLayoutParameters>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(tool_result(
            self.shared
                .apply_layout(json!({"sides": parameters.sides}))
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

    #[tool(description = "Quit the MCP-owned game from the main menu and wait for process exit")]
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
                "Use start_game, start_test, apply_layout, toggle_fight, speed_up, quit_match, and quit_game in lifecycle order. Subscribe to mechcore://status for state changes."
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
    let shared = Shared::new()?;
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
    let stop_result = shared.stop_owned_game().await;
    cleanup_socket(&shared.socket_path);
    stop_result?;
    result.map(drop)
}

async fn monitor(shared: Arc<Shared>) {
    loop {
        match shared.reap_child().await {
            Ok(Some(_)) => {}
            Ok(None) => {
                let connected = shared.adapter.lock().await.is_some();
                if connected {
                    match shared.adapter_request("status", json!({})).await {
                        Ok(status) => shared.publish(status),
                        Err(_) => shared.publish(json!({"status": "unknown"})),
                    }
                }
            }
            Err(_) => shared.publish(json!({"status": "unknown"})),
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

fn canonical_file(path: &Path, label: &str) -> Result<PathBuf, String> {
    let canonical = fs::canonicalize(path)
        .map_err(|error| format!("cannot resolve {label} {}: {error}", path.display()))?;
    if !canonical.is_file() {
        return Err(format!("{label} is not a file: {}", canonical.display()));
    }
    Ok(canonical)
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

fn cleanup_socket(path: &Path) {
    if path
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_socket())
    {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_exposes_exact_tool_surface() {
        let shared = Shared::new().unwrap();
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
                "quit_game",
                "quit_match",
                "speed_up",
                "start_game",
                "start_test",
                "status",
                "toggle_fight",
            ]
        );
    }

    #[test]
    fn default_adapter_is_a_release_sibling() {
        if env::var_os(ADAPTER_ENV).is_some() {
            return;
        }
        let shared = Shared::new().unwrap();
        assert_eq!(
            shared
                .adapter_path
                .file_name()
                .and_then(|name| name.to_str()),
            Some("libmechcore_adapter.dylib")
        );
        assert_eq!(
            shared.adapter_path.parent(),
            env::current_exe().unwrap().parent()
        );
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
}
