use crate::session::{self, Session};
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
    path::PathBuf,
    sync::Arc,
};
use tokio::{
    sync::Mutex,
    task::JoinHandle,
};

const STATUS_URI: &str = "mechcore://status";

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
    /// Request native combat speed-up. Defaults to true, with or without
    /// `video_output`.
    #[serde(default)]
    speed_up: Option<bool>,
    /// Absolute destination path for the new `.mcfr` file.
    output: PathBuf,
    /// Optional absolute destination for a logic-frame-aligned `QuickTime` MJPEG `.mov`.
    video_output: Option<PathBuf>,
    /// Optional temporary Adapter-native research sidecar.
    instrumentation: Option<session::RecordBattleInstrumentationParameters>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RecordReplayRoundParameters {
    /// Request native combat speed-up. Defaults to true.
    #[serde(default)]
    speed_up: Option<bool>,
    /// Absolute path to the source `.grbr` replay.
    grbr: PathBuf,
    /// One-based combat round to capture.
    round: i32,
    /// Absolute destination path for the new `.mcfr` file.
    output: PathBuf,
    /// Optional temporary Adapter-native research sidecar.
    instrumentation: Option<session::RecordBattleInstrumentationParameters>,
}


#[derive(Clone)]
struct MechcoreMcp {
    shared: Arc<Session>,
    tool_router: ToolRouter<Self>,
    subscription: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl MechcoreMcp {
    fn new(shared: Arc<Session>) -> Self {
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
                .apply_layout(
                    json!({
                        "seed": parameters.seed,
                        "round": parameters.round,
                        "sides": parameters.sides
                    }),
                    None,
                )
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
                    parameters.speed_up,
                    parameters.instrumentation,
                )
                .await,
        ))
    }

    #[tool(
        description = "Load a GRBR replay from main_menu, jump to a one-based round, capture its native fighting-to-over MCFR with fast deployment and battle speed-up, then exit the replay and return only from main_menu"
    )]
    async fn record_replay_round(
        &self,
        Parameters(parameters): Parameters<RecordReplayRoundParameters>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(tool_result(
            self.shared
                .record_replay_round(
                    parameters.grbr,
                    parameters.round,
                    parameters.output,
                    parameters.speed_up,
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
                "Launch Mechabellum with the Adapter outside MCP and call connect_adapter. Synthetic captures use start_test, apply_layout, then record_battle. Native replay captures use record_replay_round directly from main_menu. Both recording tools return only from main_menu without quitting the game. Use quit_match only to leave a manually active test/replay or to recover a reported cleanup obligation, and use quit_game only when the capture session ends. Subscribe to mechcore://status for state changes."
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
        let mut receiver = self.shared.subscribe_status();
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
    let shared = Session::new();
    let monitor = tokio::spawn(Session::monitor_status(shared.clone()));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_exposes_exact_tool_surface() {
        let shared = Session::new();
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
                "record_replay_round",
                "speed_up",
                "start_test",
                "status",
                "toggle_fight",
            ]
        );
    }

    #[test]
    fn record_battle_tool_declares_its_main_menu_postcondition() {
        let shared = Session::new();
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
    fn record_replay_round_exposes_closed_transaction_inputs() {
        let shared = Session::new();
        let tool = MechcoreMcp::new(shared)
            .tool_router
            .list_all()
            .into_iter()
            .find(|tool| tool.name == "record_replay_round")
            .expect("record_replay_round tool exists");
        let description = tool.description.expect("tool has a description");
        assert!(description.contains("main_menu"));
        assert!(description.contains("fast deployment"));
        assert!(description.contains("battle speed-up"));
        let schema = Value::Object(tool.input_schema.as_ref().clone());
        assert!(schema.pointer("/properties/instrumentation").is_some());
        assert_eq!(
            schema.get("required"),
            Some(&json!(["grbr", "round", "output"]))
        );
    }

    #[test]
    fn scoped_replay_instrumentation_validates_ids_ticks_and_profile() {
        let mut parameters: RecordReplayRoundParameters = serde_json::from_value(json!({
            "grbr": "/tmp/tuff-rvo.grbr", "round": 7, "output": "/tmp/tuff-rvo.mcfr",
            "instrumentation": {"output": "/tmp/tuff-rvo.h5", "profile": "target_refs_rvo_v1",
                "rvo_scope": {"start_tick": 8, "end_tick": 14, "unit_ids": [124, 246]}}
        }))
        .unwrap();
        assert!(
            session::validate_record_outputs(
                &parameters.output,
                None,
                parameters.instrumentation.as_ref()
            )
            .is_ok()
        );
        parameters
            .instrumentation
            .as_mut()
            .unwrap()
            .rvo_scope
            .as_mut()
            .unwrap()
            .unit_ids
            .push(124);
        assert!(
            session::validate_record_outputs(
                &parameters.output,
                None,
                parameters.instrumentation.as_ref()
            )
            .is_err()
        );
        let config = parameters.instrumentation.as_mut().unwrap();
        config.rvo_scope.as_mut().unwrap().unit_ids.pop();
        config.rvo_scope.as_mut().unwrap().end_tick = 72;
        assert!(
            session::validate_record_outputs(
                &parameters.output,
                None,
                parameters.instrumentation.as_ref()
            )
            .is_err()
        );
        let config = parameters.instrumentation.as_mut().unwrap();
        config.rvo_scope.as_mut().unwrap().end_tick = 14;
        config.profile = "target_refs_v1".into();
        assert!(
            session::validate_record_outputs(
                &parameters.output,
                None,
                parameters.instrumentation.as_ref()
            )
            .is_err()
        );
    }

    #[test]
    fn start_test_tool_exposes_optional_seed() {
        let shared = Session::new();
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
        assert_eq!(parameters.seed, 0);
    }

    #[test]
    fn apply_layout_tool_schema_describes_seed_and_side_placements() {
        let shared = Session::new();
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
        assert!(
            schema
                .pointer("/$defs/Side/properties/constructions")
                .is_some()
        );
        assert!(
            schema
                .pointer("/$defs/Side/properties/contraptions")
                .is_some()
        );
        assert!(schema.pointer("/$defs/Side/properties/terrains").is_some());
        assert_eq!(
            schema
                .pointer("/$defs/TerrainType/enum/0")
                .and_then(Value::as_str),
            Some("oil")
        );
        let required_terrain_fields = schema
            .pointer("/$defs/Terrain/required")
            .and_then(Value::as_array)
            .expect("terrain has a required list");
        for field in ["type", "positions"] {
            assert!(required_terrain_fields.contains(&json!(field)));
        }
        assert!(!required_terrain_fields.contains(&json!("grid_rows")));
        assert!(
            schema
                .pointer("/$defs/Terrain/properties/grid_rows")
                .is_some()
        );
        assert!(schema.pointer("/properties/seed").is_some());
        assert!(schema.pointer("/$defs/Formation/properties/type").is_some());
        assert!(
            schema
                .pointer("/$defs/Formation/properties/index")
                .is_some()
        );
        assert!(schema.pointer("/$defs/Formation/properties/exp").is_some());
        assert!(
            schema
                .pointer("/$defs/ContraptionPlacement/properties/isairdrop")
                .is_some()
        );
        assert!(
            schema
                .pointer("/$defs/StaticPlacement/properties/isairdrop")
                .is_none()
        );
        assert_eq!(
            schema
                .pointer("/$defs/Side/properties/contraptions/items/$ref")
                .and_then(Value::as_str),
            Some("#/$defs/ContraptionPlacement")
        );
        assert!(
            schema
                .pointer("/$defs/StaticPlacement/properties/index")
                .is_none()
        );
        assert_eq!(
            schema
                .pointer("/$defs/Side/properties/constructions/items/$ref")
                .and_then(Value::as_str),
            Some("#/$defs/StaticPlacement")
        );
        assert!(
            schema
                .pointer("/$defs/StaticPlacement/properties/type")
                .is_some()
        );
    }
}
