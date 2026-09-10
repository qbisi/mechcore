use crate::il2cpp::{Api, Error as Il2CppError, Object, argument, object_argument};
use crate::layout::{
    self, BattleSkill, NativeFormation, Placement, Position as LayoutPosition, SidePlan, Techs,
    Terrain,
};
use crate::runtime::Runtime;
use mechcore_document::{
    ENERGY_TOWER_POSITION, FIGHT_VISIBLE_ENERGY_TOWER_SKILLS, MAX_TOWER_STRENGTHEN_LEVEL,
    RESEARCH_CENTER_POSITION,
};
use mechcore_protocol::{GameStatus, Operation, Request, Response};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct MapVector {
    x: i32,
    y: i32,
}

#[repr(transparent)]
#[derive(Clone, Copy)]
struct FPoint(i64);

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Vector2Int {
    x: i32,
    y: i32,
}

#[derive(Debug)]
struct UnitReadback {
    id: i32,
    level: i32,
    exp: i32,
    position: MapVector,
    rotated: bool,
    travelling: bool,
}

struct ContraptionReleaseBaseline {
    player: *mut Object,
    manager: *mut Object,
    item: *mut Object,
    recorder: *mut Object,
    id: i32,
    expected_cost: i32,
    supply: i32,
    remain_count: i32,
    record_count: i32,
    position: MapVector,
}

const ENERGY_TOWER_KIND: i32 = 1;
const RESEARCH_CENTER_KIND: i32 = 2;
const TRAINING_GROUND_SUPPLY: i32 = 10_000;
const DEFAULT_TRAINING_GROUND_MAP_ID: i32 = 1021;
const FIXED_ONE_RAW: i64 = 1_i64 << 32;
const OIL_COMMANDER_SKILL_ID: i32 = 400_002;
const OIL_RANGE_ITEM_TYPE: i32 = 1;
const OIL_RADIUS_RAW: i64 = 30 * FIXED_ONE_RAW;
const OIL_GRID_SIZE: u32 = 12;
const OIL_GRID_MASK: u32 = (1 << OIL_GRID_SIZE) - 1;
const RETAINED_OIL_ROUND: i32 = 1;

#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct StartTestArguments {
    seed: Option<i32>,
    map_id: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LayoutExecutionStage {
    Prepare,
    Activation,
}

#[derive(Clone)]
pub(crate) enum InternalOperation {
    Status,
    StartCapture {
        mode: crate::capture::CaptureStartMode,
        visual: bool,
        speed_up: bool,
        instrumentation_profile: Option<crate::capture::CaptureInstrumentationProfile>,
        rvo_scope: Option<crate::capture::RvoCaptureScope>,
    },
    StopCapture,
    ReplayFastDeployment,
    ExpireDeployment(i32),
    ResetDeployment(i32),
    FinishPreparation(i32),
}

pub fn execute(runtime: &mut Runtime, request: &Request) -> Response<Value> {
    operation_response(request.id, execute_inner(runtime, request))
}

pub(crate) fn execute_internal(
    runtime: &mut Runtime,
    request_id: u64,
    operation: InternalOperation,
) -> Response<Value> {
    let result = match operation {
        InternalOperation::Status => Ok(status(runtime)),
        InternalOperation::StartCapture {
            mode,
            visual,
            speed_up,
            instrumentation_profile,
            rvo_scope,
        } => crate::capture::start(
            runtime,
            mode,
            visual,
            speed_up,
            instrumentation_profile,
            rvo_scope,
        )
        .map(|()| json!({"started": true}))
        .map_err(OperationError::InvalidState),
        InternalOperation::StopCapture => crate::capture::stop()
            .map(|()| json!({"stopped": true}))
            .map_err(OperationError::Rejected),
        InternalOperation::ReplayFastDeployment => replay_fast_deployment(runtime),
        InternalOperation::ExpireDeployment(round) => expire_deployment(runtime, round),
        InternalOperation::ResetDeployment(round) => reset_deployment(runtime, round),
        InternalOperation::FinishPreparation(round) => finish_preparation(runtime, round),
    };
    operation_response(request_id, result)
}

pub(crate) fn execute_layout_stage(
    runtime: &mut Runtime,
    request_id: u64,
    plan: &layout::Plan,
    stage: LayoutExecutionStage,
) -> Response<Value> {
    operation_response(request_id, apply_layout_stage(runtime, plan, stage))
}

fn operation_response(id: u64, result: Result<Value, OperationError>) -> Response<Value> {
    match result {
        Ok(result) => Response::success(id, result),
        Err(OperationError::InvalidArguments(message)) => {
            Response::failure(id, "invalid_arguments", message)
        }
        Err(OperationError::InvalidState(message)) => {
            Response::failure(id, "invalid_game_state", message)
        }
        Err(OperationError::Rejected(message)) => {
            Response::failure(id, "game_rejected_operation", message)
        }
        Err(OperationError::Il2Cpp(error)) => {
            Response::failure(id, "il2cpp_error", error.to_string())
        }
        Err(OperationError::Il2CppContext(message)) => {
            Response::failure(id, "il2cpp_error", message)
        }
    }
}

#[derive(Debug)]
enum OperationError {
    InvalidArguments(String),
    InvalidState(String),
    Rejected(String),
    Il2Cpp(Il2CppError),
    Il2CppContext(String),
}

impl std::fmt::Display for OperationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidArguments(message)
            | Self::InvalidState(message)
            | Self::Rejected(message)
            | Self::Il2CppContext(message) => f.write_str(message),
            Self::Il2Cpp(error) => error.fmt(f),
        }
    }
}

impl OperationError {
    fn context(self, context: &str) -> Self {
        match self {
            Self::InvalidArguments(message) => {
                Self::InvalidArguments(format!("{context}: {message}"))
            }
            Self::InvalidState(message) => Self::InvalidState(format!("{context}: {message}")),
            Self::Rejected(message) => Self::Rejected(format!("{context}: {message}")),
            Self::Il2Cpp(error) => Self::Il2CppContext(format!("{context}: {error}")),
            Self::Il2CppContext(message) => Self::Il2CppContext(format!("{context}: {message}")),
        }
    }
}

impl From<Il2CppError> for OperationError {
    fn from(value: Il2CppError) -> Self {
        Self::Il2Cpp(value)
    }
}

fn execute_inner(runtime: &mut Runtime, request: &Request) -> Result<Value, OperationError> {
    match request.operation {
        Operation::Status => Ok(status(runtime)),
        Operation::StartTest => start_test(runtime, &request.arguments),
        Operation::RecordBattle => Err(OperationError::InvalidState(
            "record_battle requires the runtime capture coordinator".into(),
        )),
        Operation::RecordReplayRound => Err(OperationError::InvalidState(
            "record_replay_round requires the runtime capture coordinator".into(),
        )),
        Operation::ToggleFight => invoke_match_void(runtime, "ChangeProcessState"),
        Operation::SpeedUp => speed_up(runtime),
        Operation::QuitMatch => quit_match(runtime),
        Operation::QuitGame => quit_game(runtime),
        Operation::ApplyLayout => Err(OperationError::InvalidState(
            "apply_layout requires the runtime round-series coordinator".into(),
        )),
    }
}

fn status(runtime: &Runtime) -> Value {
    let api = runtime.api;
    let scene = runtime.active_scene_name().ok();
    let current_match = runtime.current_match();

    let status = if current_match.is_null() {
        if scene.as_deref().is_some_and(is_main_menu_scene) {
            GameStatus::MainMenu
        } else {
            GameStatus::Unknown
        }
    } else if classify_replay(api, current_match) == Some(true) {
        return match_status(runtime, current_match, GameStatus::Replay);
    } else if api
        .invoke_value::<bool>(current_match, "IsTestMatch", &mut [])
        .ok()
        == Some(true)
    {
        return match_status(runtime, current_match, GameStatus::TrainingGround);
    } else {
        GameStatus::Unknown
    };

    json!({"status": status})
}

fn match_status(runtime: &Runtime, current_match: *mut Object, status: GameStatus) -> Value {
    let api = runtime.api;
    let fight = runtime.current_fight();
    let deploying = (!fight.is_null())
        .then(|| api.invoke_value::<bool>(fight, "IsDeploying", &mut []).ok())
        .flatten();
    let fighting = (!fight.is_null())
        .then(|| api.invoke_value::<bool>(fight, "IsFighting", &mut []).ok())
        .flatten();
    let round_count = api
        .invoke_value::<i32>(current_match, "get_RoundCount", &mut [])
        .ok();
    let match_seed = api
        .invoke(current_match, "GetRandom", &mut [])
        .ok()
        .filter(|random| !random.is_null())
        .and_then(|random| api.invoke_value::<i32>(random, "GetSeed", &mut []).ok());
    json!({
        "status": status,
        "round_count": round_count,
        "deploying": deploying,
        "fighting": fighting,
        "match_seed": match_seed
    })
}

fn is_main_menu_scene(scene: &str) -> bool {
    let normalized: String = scene
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect();
    normalized == "mainmenu" || normalized == "mainscene"
}

fn classify_replay(api: Api, object: *mut Object) -> Option<bool> {
    let mut class = api.object_class(object)?;
    let mut replay = false;
    let mut derives_from_match = false;
    loop {
        let namespace = api.class_namespace(class);
        let name = api.class_name(class);
        if namespace == "GameRiver.Client" && name == "ReplayMatchBase" {
            replay = true;
        }
        if namespace == "GameRiver" && name == "Match" {
            derives_from_match = true;
        }
        let Some(parent) = api.class_parent(class) else {
            break;
        };
        class = parent;
    }
    derives_from_match.then_some(replay)
}

fn start_test(runtime: &Runtime, arguments: &Value) -> Result<Value, OperationError> {
    let requested = parse_start_test_arguments(arguments)?;
    let requested_seed = requested.seed;
    if !runtime.current_match().is_null() {
        return Err(OperationError::InvalidState(
            "a match is already active".into(),
        ));
    }
    let api = runtime.api;
    let ai = api.class("GRClient.dll", "GameRiver.Client", "AIUtility")?;
    let agent_class = api.class("GRClient.dll", "GameRiver.Client", "ClientAgent")?;
    let mut match_type = 3_i32;
    let setting = api.invoke_static(
        ai,
        "CreateBattleSettingForCustomMatch",
        &mut [argument(&mut match_type)],
    )?;
    if setting.is_null() {
        return Err(OperationError::Rejected(
            "game did not create battle setting".into(),
        ));
    }
    let mut seed = requested_seed.unwrap_or(0);
    let mut map_id = start_test_map_id(&requested);
    let config = config_instance(runtime)?;
    let map = api.invoke(
        config,
        "GetMatchSettingOrNull",
        &mut [argument(&mut map_id)],
    )?;
    if map.is_null() {
        return Err(OperationError::InvalidArguments(format!(
            "unknown map_id {map_id}"
        )));
    }
    api.invoke_void(setting, "set_MapID", &mut [argument(&mut map_id)])?;
    let map_id = api.invoke_value::<i32>(setting, "get_MapID", &mut [])?;
    api.invoke_void(setting, "set_SystemSeed", &mut [argument(&mut seed)])?;
    let mut optional_features = false;
    for setter in [
        "set_EnableAdvanceTeam",
        "set_EnableReinforcement",
        "set_EnableUnitReinforcement",
    ] {
        api.invoke_void(setting, setter, &mut [argument(&mut optional_features)])?;
    }
    let mut constructions = true;
    api.invoke_void(
        setting,
        "set_EnableConstruction",
        &mut [argument(&mut constructions)],
    )?;
    configure_training_ground_supply(api, setting)?;
    let agent = api.invoke_static(agent_class, "get_Instance", &mut [])?;
    if agent.is_null() {
        return Err(OperationError::InvalidState(
            "ClientAgent is unavailable".into(),
        ));
    }
    let host = api.string("127.0.0.1")?;
    let created = api.invoke_value::<bool>(
        agent,
        "CreateHost",
        &mut [object_argument(setting), object_argument(host)],
    )?;
    if !created {
        return Err(OperationError::Rejected(
            "CreateHost rejected the setting".into(),
        ));
    }
    Ok(json!({
        "created": true,
        "initial_supply": TRAINING_GROUND_SUPPLY,
        "requested_seed": requested_seed,
        "map_id": map_id
    }))
}

fn parse_start_test_arguments(arguments: &Value) -> Result<StartTestArguments, OperationError> {
    let arguments = serde_json::from_value::<Option<StartTestArguments>>(arguments.clone())
        .map_err(|error| OperationError::InvalidArguments(error.to_string()))?
        .unwrap_or_default();
    if arguments.map_id.is_some_and(|id| id <= 0) {
        return Err(OperationError::InvalidArguments(
            "map_id must be positive".into(),
        ));
    }
    Ok(arguments)
}

fn start_test_map_id(arguments: &StartTestArguments) -> i32 {
    arguments.map_id.unwrap_or(DEFAULT_TRAINING_GROUND_MAP_ID)
}

fn configure_training_ground_supply(api: Api, setting: *mut Object) -> Result<(), OperationError> {
    let player_count = api.invoke_value::<i32>(setting, "GetPlayerDataCount", &mut [])?;
    if player_count != 2 {
        return Err(OperationError::InvalidState(format!(
            "training-ground battle setting contains {player_count} players, expected 2"
        )));
    }
    for mut index in 0..player_count {
        let player = api.invoke(setting, "GetPlayerData", &mut [argument(&mut index)])?;
        if player.is_null() {
            return Err(OperationError::InvalidState(format!(
                "training-ground player setting {index} is unavailable"
            )));
        }
        let mut supply = TRAINING_GROUND_SUPPLY;
        api.invoke_void(player, "SetFirstRoundSupply", &mut [argument(&mut supply)])?;
        api.invoke_void(player, "SetMaxRoundSupply", &mut [argument(&mut supply)])?;
        let first = api.invoke_value::<i32>(player, "GetFirstRoundSupply", &mut [])?;
        let maximum = api.invoke_value::<i32>(player, "GetMaxRoundSupply", &mut [])?;
        if first != TRAINING_GROUND_SUPPLY || maximum != TRAINING_GROUND_SUPPLY {
            return Err(OperationError::Rejected(format!(
                "training-ground player setting {index} supply readback did not match"
            )));
        }
    }
    Ok(())
}

fn require_match(runtime: &Runtime) -> Result<*mut Object, OperationError> {
    let current = runtime.current_match();
    if current.is_null() {
        Err(OperationError::InvalidState("no active match".into()))
    } else {
        Ok(current)
    }
}

pub(crate) fn load_replay(
    runtime: &Runtime,
    request_id: u64,
    path: &Path,
    native_start_round: i32,
) -> Response<Value> {
    operation_response(
        request_id,
        load_replay_inner(runtime, path, native_start_round),
    )
}

fn load_replay_inner(
    runtime: &Runtime,
    path: &Path,
    native_start_round: i32,
) -> Result<Value, OperationError> {
    if !runtime.current_match().is_null() {
        return Err(OperationError::InvalidState(
            "record_replay_round requires main_menu with no active match".into(),
        ));
    }
    let path = path
        .to_str()
        .ok_or_else(|| OperationError::InvalidArguments("grbr path is not valid UTF-8".into()))?;
    let api = runtime.api;
    let managed_path = api.string(path)?;
    let utility = api.class("GRCore.dll", "GameRiver", "MatchUtility")?;
    let load = api.class_method_with_parameter_types(utility, "LoadReplay", &["System.String"])?;
    let replay = api.invoke_raw(
        load,
        std::ptr::null_mut(),
        &mut [object_argument(managed_path)],
    )?;
    if replay.is_null() {
        return Err(OperationError::Rejected(format!(
            "the game could not load replay {path}"
        )));
    }
    let command_class = api.class("GRClient.dll", "GameRiver.Client", "PlayReplayCommand")?;
    let command = api.new_object(command_class)?;
    let execute = api.method_with_parameter_types(
        command,
        "Execute",
        &["GameRiver.IReplay", "System.Int32"],
    )?;
    let mut start_round = native_start_round;
    api.invoke_raw(
        execute,
        command.cast(),
        &mut [object_argument(replay), argument(&mut start_round)],
    )?;
    Ok(json!({
        "loaded": true,
        "native_start_round": native_start_round,
    }))
}

fn player_controller(
    runtime: &Runtime,
    current_match: *mut Object,
) -> Result<*mut Object, OperationError> {
    let controller = runtime
        .api
        .invoke(current_match, "GetLocalPlayerController", &mut [])?;
    if controller.is_null() {
        Err(OperationError::InvalidState(
            "local player controller is unavailable".into(),
        ))
    } else {
        Ok(controller)
    }
}

fn invoke_match_void(runtime: &Runtime, method: &str) -> Result<Value, OperationError> {
    let current = require_match(runtime)?;
    runtime.api.invoke_void(current, method, &mut [])?;
    Ok(json!({"performed": true}))
}

fn expire_deployment(runtime: &Runtime, expected_round: i32) -> Result<Value, OperationError> {
    let (current, process) = require_deployment_process(runtime, expected_round)?;
    // This is the synchronous body of StandaloneMatchActionController's delayed
    // ExitCurrentMatchState action; queuing that action can expire the next round.
    let mut state_over_time =
        runtime
            .api
            .invoke_value::<FPoint>(current, "CalculateStateOverTime", &mut [])?;
    runtime.api.invoke_void(
        process,
        "SetStateTime",
        &mut [argument(&mut state_over_time)],
    )?;
    Ok(json!({"performed": true, "round": expected_round}))
}

fn reset_deployment(runtime: &Runtime, expected_round: i32) -> Result<Value, OperationError> {
    let (_, process) = require_deployment_process(runtime, expected_round)?;
    let mut state_time = FPoint(0);
    runtime
        .api
        .invoke_void(process, "SetStateTime", &mut [argument(&mut state_time)])?;
    Ok(json!({"performed": true, "round": expected_round}))
}

fn require_deployment_process(
    runtime: &Runtime,
    expected_round: i32,
) -> Result<(*mut Object, *mut Object), OperationError> {
    require_layout_deployment(runtime, expected_round)?;
    let current = require_match(runtime)?;
    let process = runtime
        .api
        .invoke(current, "GetProcessController", &mut [])?;
    if process.is_null()
        || !runtime
            .api
            .invoke_value::<bool>(process, "IsDeployState", &mut [])?
    {
        return Err(OperationError::InvalidState(
            "layout round is not in the native deployment process state".into(),
        ));
    }
    Ok((current, process))
}

fn finish_preparation(runtime: &Runtime, expected_round: i32) -> Result<Value, OperationError> {
    if expected_round <= 0 {
        return Err(OperationError::InvalidArguments(
            "expected_round must be positive".into(),
        ));
    }
    let current = require_match(runtime)?;
    let round = runtime
        .api
        .invoke_value::<i32>(current, "get_RoundCount", &mut [])?;
    if round == expected_round + 1 {
        return Ok(json!({"performed": false, "already_finished": true, "round": round}));
    }
    let fight = runtime.current_fight();
    if fight.is_null() || round != expected_round {
        return Err(OperationError::InvalidState(
            "round or fight controller does not match".into(),
        ));
    }
    let fighting = runtime
        .api
        .invoke_value::<bool>(fight, "IsFighting", &mut [])?;
    if !fighting {
        return Err(OperationError::InvalidState(
            "game does not admit preparation finish".into(),
        ));
    }
    runtime
        .api
        .invoke_void(current, "ChangeProcessState", &mut [])?;
    Ok(json!({"performed": true, "already_finished": false, "round": round}))
}

fn speed_up(runtime: &Runtime) -> Result<Value, OperationError> {
    let current = require_match(runtime)?;
    let controller = runtime
        .api
        .invoke(current, "GetMatchActionController", &mut [])?;
    if controller.is_null() {
        return Err(OperationError::InvalidState(
            "match action controller is unavailable".into(),
        ));
    }
    runtime
        .api
        .invoke_void(controller, "RequestSpeedUp", &mut [])?;
    Ok(json!({"requested": true}))
}

fn replay_fast_deployment(runtime: &Runtime) -> Result<Value, OperationError> {
    let current = require_match(runtime)?;
    if classify_replay(runtime.api, current) != Some(true) {
        return Err(OperationError::InvalidState(
            "fast deployment requires an active replay".into(),
        ));
    }
    let method = runtime.api.method_with_parameter_types(
        current,
        "SetReplayTime",
        &["System.Boolean", "System.Single"],
    )?;
    let mut is_real_time = false;
    let mut step_time = 0.0_f32;
    runtime.api.invoke_raw(
        method,
        current.cast(),
        &mut [argument(&mut is_real_time), argument(&mut step_time)],
    )?;
    Ok(json!({
        "enabled": true,
        "is_real_time": false,
        "step_time": 0.0,
    }))
}

fn quit_match(runtime: &Runtime) -> Result<Value, OperationError> {
    let current = require_match(runtime)?;
    let mut quit_game = false;
    runtime
        .api
        .invoke_void(current, "Quit", &mut [argument(&mut quit_game)])?;
    Ok(json!({"performed": true}))
}

fn quit_game(runtime: &Runtime) -> Result<Value, OperationError> {
    if !runtime.current_match().is_null()
        || runtime
            .active_scene_name()
            .ok()
            .as_deref()
            .is_none_or(|scene| !is_main_menu_scene(scene))
    {
        return Err(OperationError::InvalidState(
            "quit_game requires the main menu".into(),
        ));
    }
    let application =
        runtime
            .api
            .class("UnityEngine.CoreModule.dll", "UnityEngine", "Application")?;
    let mut exit_code = 0_i32;
    runtime
        .api
        .invoke_static(application, "Quit", &mut [argument(&mut exit_code)])?;
    Ok(json!({"requested": true, "exit_code": exit_code}))
}

fn perform_sync(
    api: Api,
    controller: *mut Object,
    action: *mut Object,
) -> Result<(), OperationError> {
    let performed = api.invoke_value::<bool>(
        controller,
        "TryPerformActionSync",
        &mut [object_argument(action)],
    )?;
    if performed {
        Ok(())
    } else {
        Err(OperationError::Rejected(
            "TryPerformActionSync returned false".into(),
        ))
    }
}

fn check_action(
    api: Api,
    controller: *mut Object,
    action: *mut Object,
) -> Result<i32, OperationError> {
    let mut result = -1_i32;
    let accepted = api.invoke_value::<bool>(
        controller,
        "CheckAction",
        &mut [object_argument(action), argument(&mut result)],
    )?;
    if accepted && result == 0 {
        Ok(result)
    } else {
        Err(OperationError::Rejected(format!(
            "CheckAction rejected with {result}"
        )))
    }
}

fn perform_test(
    api: Api,
    current_match: *mut Object,
    action: *mut Object,
) -> Result<(), OperationError> {
    let performed = api.invoke_value::<bool>(
        current_match,
        "TryPerformTestCommand",
        &mut [object_argument(action)],
    )?;
    if performed {
        Ok(())
    } else {
        Err(OperationError::Rejected(
            "TryPerformTestCommand returned false".into(),
        ))
    }
}

fn core_action(api: Api, name: &str) -> Result<*mut Object, OperationError> {
    let class = api.class("GRCore.dll", "GameRiver", name)?;
    Ok(api.new_object(class)?)
}

fn new_player_test_action(
    api: Api,
    class_name: &str,
    controller: *mut Object,
) -> Result<*mut Object, OperationError> {
    let player = api.invoke(controller, "GetPlayer", &mut [])?;
    let mut player_index = api.invoke_value::<i32>(player, "GetRoomIndex", &mut [])?;
    let action = core_action(api, class_name)?;
    api.invoke_void(action, "set_PIDX", &mut [argument(&mut player_index)])?;
    Ok(action)
}

fn move_unit(
    runtime: &Runtime,
    mut unit_index: i32,
    mut position: MapVector,
    mut rotate: bool,
) -> Result<(), OperationError> {
    let current = require_match(runtime)?;
    let controller = player_controller(runtime, current)?;
    let manager = runtime.api.invoke(controller, "GetUnitManager", &mut [])?;
    let mut unit: *mut Object = std::ptr::null_mut();
    let found = runtime.api.invoke_value::<bool>(
        manager,
        "TryGetUnit",
        &mut [argument(&mut unit_index), argument(&mut unit)],
    )?;
    if !found || unit.is_null() {
        return Err(OperationError::InvalidArguments(
            "unit_index was not found".into(),
        ));
    }
    let action = core_action(runtime.api, "PAD_MoveUnit")?;
    runtime.api.invoke_void(
        action,
        "AddMoveData",
        &mut [
            object_argument(controller),
            object_argument(unit),
            argument(&mut position),
            argument(&mut rotate),
        ],
    )?;
    check_action(runtime.api, controller, action)?;
    perform_sync(runtime.api, controller, action)
}

fn create_unit(
    runtime: &Runtime,
    current: *mut Object,
    unit_id: i32,
    displayed_level: i32,
    unit_index: i32,
) -> Result<i32, OperationError> {
    if !(1..=9).contains(&displayed_level) {
        return Err(OperationError::InvalidArguments(
            "displayed_level must be 1..=9".into(),
        ));
    }
    let api = runtime.api;
    let controller = player_controller(runtime, current)?;
    let player = api.invoke(controller, "GetPlayer", &mut [])?;
    let territory = api.invoke(controller, "GetTerritoryManager", &mut [])?;
    let region = api.invoke(territory, "GetFocusRegion", &mut [])?;
    let mut player_index = api.invoke_value::<i32>(player, "GetRoomIndex", &mut [])?;
    let mut region_id = api.invoke_value::<i32>(region, "get_ID", &mut [])?;
    let mut unit_index = unit_index;
    let action = core_action(api, "MAD_AddUnit")?;
    let mut unit_id = unit_id;
    let mut level = displayed_level - 1;
    let mut fixed = false;
    let mut position = MapVector::default();
    let mut rotate = false;
    let mut sell_supply = -1_i32;
    for (setter, value) in [
        ("set_PIDX", argument(&mut player_index)),
        ("set_UID", argument(&mut unit_id)),
        ("set_Level", argument(&mut level)),
        ("set_UIDX", argument(&mut unit_index)),
        ("set_IsFixedPosition", argument(&mut fixed)),
        ("set_Position", argument(&mut position)),
        ("set_IsRotate", argument(&mut rotate)),
        ("set_SellSupply", argument(&mut sell_supply)),
        ("set_RegionID", argument(&mut region_id)),
    ] {
        api.invoke_void(action, setter, &mut [value])?;
    }
    perform_test(api, current, action)?;
    Ok(unit_index)
}

fn add_unit(
    runtime: &Runtime,
    unit_id: i32,
    level: i32,
    unit_index: i32,
    position: MapVector,
    rotate: bool,
) -> Result<i32, OperationError> {
    let current = require_match(runtime)?;
    let index = create_unit(runtime, current, unit_id, level, unit_index)?;
    move_unit(runtime, index, position, rotate)?;
    Ok(index)
}

fn set_unit_travelling(
    runtime: &Runtime,
    mut unit_index: i32,
    position: MapVector,
    mut travelling: bool,
) -> Result<(), OperationError> {
    let current = require_match(runtime)?;
    let controller = player_controller(runtime, current)?;
    let (_manager, unit) = find_unit(runtime, controller, &mut unit_index)?;
    let territory_manager = runtime
        .api
        .invoke(controller, "GetTerritoryManager", &mut [])?;
    let territory = runtime
        .api
        .invoke(territory_manager, "GetTerritory", &mut [])?;
    let mut position = position;
    let region = runtime.api.invoke(
        territory,
        "GetRegionForPosition",
        &mut [argument(&mut position)],
    )?;
    if region.is_null() {
        return Err(OperationError::InvalidState(
            "final unit position has no map region".into(),
        ));
    }
    let mut auto_refresh = false;
    runtime.api.invoke_void(
        territory_manager,
        "RefreshSuperDeploymentStatus",
        &mut [
            object_argument(unit),
            object_argument(region),
            argument(&mut auto_refresh),
            argument(&mut travelling),
        ],
    )?;
    Ok(())
}

fn apply_layout_stage(
    runtime: &Runtime,
    plan: &layout::Plan,
    stage: LayoutExecutionStage,
) -> Result<Value, OperationError> {
    let expected_round = match stage {
        LayoutExecutionStage::Prepare => 1,
        LayoutExecutionStage::Activation => plan.round,
    };
    require_layout_deployment(runtime, expected_round)?;

    if stage == LayoutExecutionStage::Prepare {
        validate_layout_positions(plan)?;
        let neutral_crystals = inspect_neutral_crystals(runtime)?;
        let current = require_match(runtime)?;
        validate_side_layout_catalog(runtime, &plan.blue)?;
        clear_current_side(runtime, current, &plan.blue.constructions, false)?;
        switch_player(runtime, current)?;
        let red = (|| {
            validate_side_layout_catalog(runtime, &plan.red)?;
            clear_current_side(runtime, current, &plan.red.constructions, true)
        })();
        if let Err(error) = red {
            return Err(restore_player_after_error(runtime, current, error));
        }
        switch_player(runtime, current)?;
        return Ok(json!({
            "stage": "prepare",
            "round": expected_round,
            "target_round": plan.round,
            "formation_count": plan.formation_count(),
            "construction_count": plan.construction_count(),
            "contraption_count": plan.contraption_count(),
            "cleared": {
                "cleared": true,
                "both_sides": true,
                "constructions": "reconciled"
            },
            "retained": {"neutral_crystals": neutral_crystals},
        }));
    }

    let current = require_match(runtime)?;
    let blue = apply_side_layout_stage(runtime, current, &plan.blue, false, stage)?;
    switch_player(runtime, current)?;
    let red = match apply_side_layout_stage(runtime, current, &plan.red, true, stage) {
        Ok(red) => red,
        Err(error) => {
            return Err(restore_player_after_error(runtime, current, error));
        }
    };
    switch_player(runtime, current)?;

    Ok(json!({
        "stage": match stage {
            LayoutExecutionStage::Prepare => "prepare",
            LayoutExecutionStage::Activation => "activation",
        },
        "round": expected_round,
        "target_round": plan.round,
        "formation_count": plan.formation_count(),
        "construction_count": plan.construction_count(),
        "contraption_count": plan.contraption_count(),
        "sides": {
            "blue": blue,
            "red": red
        }
    }))
}

// Neutral crystals belong to the native map, not either layout side. Even
// peripheral crystals affect RVO tree construction, so preserve their owners,
// agents and indexes. Merely being neutral does not make them test-only objects.
fn inspect_neutral_crystals(runtime: &Runtime) -> Result<Value, OperationError> {
    require_layout_deployment(runtime, 1)?;
    let api = runtime.api;
    let system = find_match_module(
        runtime,
        runtime.current_fight(),
        "GameRiver.Fight",
        "BuildingSystem",
    )?;
    let buildings = api.invoke(system, "GetBuildings", &mut [])?;
    let crystal_class = api.class("GRFight.dll", "GameRiver.Fight", "FightCrystal")?;
    let team_field = api.field(crystal_class, "currentTeamController")?;
    let origin_field = api.field(crystal_class, "originTeamController")?;
    let count = list_count(api, buildings)?;
    let mut rvo_controller_count = 0;
    for index in 0..count {
        let crystal = list_item(api, buildings, index)?;
        let team: *mut Object = api.field_value(crystal, team_field)?;
        let origin: *mut Object = api.field_value(crystal, origin_field)?;
        if api.object_class(crystal) != Some(crystal_class) || !team.is_null() || !origin.is_null()
        {
            return Err(OperationError::InvalidState(
                "neutral crystal inspection found a non-neutral or derived global building".into(),
            ));
        }
        let controller = api.invoke(crystal, "GetRVOController", &mut [])?;
        rvo_controller_count += usize::from(!controller.is_null());
    }
    Ok(json!({"retained_count": count, "rvo_controller_count": rvo_controller_count}))
}

fn switch_player(runtime: &Runtime, current: *mut Object) -> Result<(), OperationError> {
    runtime
        .api
        .invoke_void(current, "SwitchToNextPlayer", &mut [])
        .map_err(OperationError::from)
}

fn restore_player_after_error(
    runtime: &Runtime,
    current: *mut Object,
    error: OperationError,
) -> OperationError {
    match switch_player(runtime, current) {
        Ok(()) => error,
        Err(restore) => error.context(&format!("selected-side restore also failed: {restore}")),
    }
}

fn validate_side_layout_catalog(runtime: &Runtime, side: &SidePlan) -> Result<(), OperationError> {
    let config = config_instance(runtime)?;
    for placement in side
        .formations
        .iter()
        .chain(&side.constructions)
        .chain(&side.contraptions)
    {
        let (method, mut id) = match placement.native {
            NativeFormation::Unit(id) => ("GetUnitData", id),
            NativeFormation::Construction(id) => ("GetConstructionData", id),
            NativeFormation::Contraption(mut id) => {
                let current = require_match(runtime)?;
                let controller = player_controller(runtime, current)?;
                let manager = runtime
                    .api
                    .invoke(controller, "GetContraptionManager", &mut [])?;
                let data =
                    runtime
                        .api
                        .invoke(manager, "GetContraption", &mut [argument(&mut id)])?;
                if data.is_null() {
                    return Err(OperationError::InvalidArguments(format!(
                        "placement type {:?} at local position ({}, {}) is absent from the runtime catalog",
                        placement.type_name, placement.position.x, placement.position.y
                    )));
                }
                continue;
            }
        };
        let data = runtime
            .api
            .invoke(config, method, &mut [argument(&mut id)])?;
        if data.is_null() {
            return Err(OperationError::InvalidArguments(format!(
                "placement type {:?} at local position ({}, {}) is absent from the runtime catalog",
                placement.type_name, placement.position.x, placement.position.y
            )));
        }
        if let Some(equipment_id) = placement.equipment {
            validate_equipment_catalog(runtime, config, placement, equipment_id)?;
        }
    }
    validate_tech_catalog(runtime, config, &side.techs)?;
    validate_fixed_tower_positions(runtime)?;
    for &skill in &side.energy_tower_skills {
        require_energy_tower_skill(runtime, skill)?;
    }
    validate_battle_skill_catalog(runtime, &side.battle_skills)?;
    Ok(())
}

fn validate_battle_skill_catalog(
    runtime: &Runtime,
    skills: &[BattleSkill],
) -> Result<(), OperationError> {
    let config = config_instance(runtime)?;
    for skill in skills {
        let mut id = skill.commander_skill_id;
        let data = runtime
            .api
            .invoke(config, "GetCommanderSkill", &mut [argument(&mut id)])?;
        if data.is_null()
            || runtime.api.invoke_value::<i32>(data, "GetID", &mut [])? != skill.commander_skill_id
        {
            return Err(OperationError::InvalidArguments(format!(
                "battle skill type {:?} is absent from the runtime catalog",
                skill.type_name
            )));
        }
    }
    Ok(())
}

fn validate_tech_catalog(
    runtime: &Runtime,
    config: *mut Object,
    techs: &Techs,
) -> Result<(), OperationError> {
    for &officer_id in &techs.officers {
        let mut id = officer_id;
        let data = runtime
            .api
            .invoke(config, "GetOfficerData", &mut [argument(&mut id)])?;
        if data.is_null() {
            return Err(OperationError::InvalidArguments(format!(
                "officer ID {officer_id} is absent from the runtime catalog"
            )));
        }
    }
    for &technology_id in &techs.units {
        let mut id = technology_id;
        let technology =
            runtime
                .api
                .invoke(config, "GetTechnologyByID", &mut [argument(&mut id)])?;
        if technology.is_null() {
            return Err(OperationError::InvalidArguments(format!(
                "unit technology ID {technology_id} is absent from the runtime catalog"
            )));
        }
        technology_owner(runtime, config, technology_id)?;
    }
    Ok(())
}

#[cfg(test)]
fn encoded_technology_owner(technology_id: i32) -> Result<i32, OperationError> {
    // Build 2227 encodes the owning ordinary unit ID in the final two decimal
    // digits of every unit TechnologyData.ID.
    let unit_id = technology_id % 100;
    if (1..=31).contains(&unit_id) {
        Ok(unit_id)
    } else {
        Err(OperationError::InvalidArguments(format!(
            "unit technology ID {technology_id} has no encoded ordinary-unit owner"
        )))
    }
}

fn technology_owner(
    runtime: &Runtime,
    config: *mut Object,
    technology_id: i32,
) -> Result<i32, OperationError> {
    let mut owners = Vec::new();
    for unit_id in 1..=31 {
        let mut candidate = unit_id;
        let unit = runtime
            .api
            .invoke(config, "GetUnitData", &mut [argument(&mut candidate)])?;
        if unit.is_null() {
            continue;
        }
        let mut id = technology_id;
        if runtime
            .api
            .invoke_value::<bool>(unit, "HaveTechnology", &mut [argument(&mut id)])?
        {
            owners.push(unit_id);
        }
    }
    match owners.as_slice() {
        [unit_id] => Ok(*unit_id),
        [] => Err(OperationError::InvalidArguments(format!(
            "unit technology ID {technology_id} has no ordinary-unit owner in the runtime catalog"
        ))),
        _ => Err(OperationError::Rejected(format!(
            "unit technology ID {technology_id} belongs to multiple runtime units {owners:?}"
        ))),
    }
}

fn validate_equipment_catalog(
    runtime: &Runtime,
    config: *mut Object,
    placement: &Placement,
    equipment_id: i32,
) -> Result<(), OperationError> {
    let equipment = runtime.api.invoke(config, "GetEquipmentDatas", &mut [])?;
    for index in 0..list_count(runtime.api, equipment)? {
        let data = list_item(runtime.api, equipment, index)?;
        if runtime.api.invoke_value::<i32>(data, "GetID", &mut [])? == equipment_id {
            return Ok(());
        }
    }
    Err(OperationError::InvalidArguments(format!(
        "equipment {equipment_id} for formation type {:?} at local position ({}, {}) is absent from the runtime catalog",
        placement.type_name, placement.position.x, placement.position.y
    )))
}

fn require_layout_deployment(runtime: &Runtime, expected_round: i32) -> Result<(), OperationError> {
    let state = status(runtime);
    let ready = state.get("status").and_then(Value::as_str) == Some("training_ground")
        && state.get("round_count").and_then(Value::as_i64) == Some(i64::from(expected_round))
        && state.get("deploying").and_then(Value::as_bool) == Some(true)
        && state.get("fighting").and_then(Value::as_bool) == Some(false);
    if ready {
        Ok(())
    } else {
        Err(OperationError::InvalidState(format!(
            "layout stage requires Training Ground round {expected_round} deployment; current state: {state}"
        )))
    }
}

fn validate_layout_positions(plan: &layout::Plan) -> Result<(), OperationError> {
    for placement in &plan.blue.formations {
        layout_world_position(placement, false)?;
    }
    for placement in &plan.blue.constructions {
        layout_world_position(placement, false)?;
    }
    for placement in &plan.blue.contraptions {
        layout_world_position(placement, false)?;
    }
    for placement in &plan.red.formations {
        layout_world_position(placement, true)?;
    }
    for placement in &plan.red.constructions {
        layout_world_position(placement, true)?;
    }
    for placement in &plan.red.contraptions {
        layout_world_position(placement, true)?;
    }
    for &position in &plan.blue.airdrop_shields {
        position_to_world(position, false, "airdrop shield center")?;
    }
    for &position in &plan.red.airdrop_shields {
        position_to_world(position, true, "airdrop shield center")?;
    }
    for terrain in &plan.blue.terrains {
        terrain_world_positions(terrain, false)?;
    }
    for terrain in &plan.red.terrains {
        terrain_world_positions(terrain, true)?;
    }
    for skill in &plan.blue.battle_skills {
        battle_skill_world_positions(skill, false)?;
    }
    for skill in &plan.red.battle_skills {
        battle_skill_world_positions(skill, true)?;
    }
    Ok(())
}

fn apply_side_layout_stage(
    runtime: &Runtime,
    current: *mut Object,
    side: &SidePlan,
    rotate_to_world: bool,
    stage: LayoutExecutionStage,
) -> Result<Value, OperationError> {
    if stage == LayoutExecutionStage::Prepare {
        return Err(OperationError::InvalidState(
            "prepare stage cannot apply side layout state".into(),
        ));
    }
    let formations = apply_formations(runtime, current, &side.formations, rotate_to_world)?;
    let constructions = apply_formations(runtime, current, &side.constructions, rotate_to_world)?;
    let contraptions = apply_formations(runtime, current, &side.contraptions, rotate_to_world)?;
    let result = match stage {
        LayoutExecutionStage::Prepare => unreachable!("prepare returned before side application"),
        LayoutExecutionStage::Activation => json!({
            "techs": apply_techs(runtime, current, &side.techs)?,
            "energy_tower_skills": apply_energy_tower_skills(
                runtime,
                &side.energy_tower_skills,
            )?,
            "tower_strengthen_levels": apply_tower_strengthen_levels(
                runtime,
                &side.tower_strengthen_levels,
            )?,
            "formations": formations,
            "constructions": constructions,
            "contraptions": contraptions,
            "airdrop_shields": apply_airdrop_shields(
                runtime,
                &side.airdrop_shields,
                rotate_to_world,
            )?,
            "terrains": apply_terrains(runtime, &side.terrains, rotate_to_world)?,
            "battle_skills": apply_battle_skills(
                runtime,
                &side.battle_skills,
                rotate_to_world,
            )?,
        }),
    };
    if runtime.current_match() != current {
        return Err(OperationError::InvalidState(
            "active match changed while applying side layout".into(),
        ));
    }
    Ok(result)
}

/// Restores the Shield Airdrops earlier rounds left standing, in declaration
/// order. They are existing world objects, so this adds no commander inventory
/// and no release record.
fn apply_airdrop_shields(
    runtime: &Runtime,
    shields: &[LayoutPosition],
    rotate_to_world: bool,
) -> Result<Vec<Value>, OperationError> {
    shields
        .iter()
        .map(|&position| {
            let world = position_to_world(position, rotate_to_world, "airdrop shield center")?;
            let order = restore_airdrop_shield(runtime, world)?;
            Ok(json!({
                "native_shield_order": order,
                "position": {"x": position.x, "y": position.y}
            }))
        })
        .collect()
}

fn apply_terrains(
    runtime: &Runtime,
    terrains: &[Terrain],
    rotate_to_world: bool,
) -> Result<Vec<Value>, OperationError> {
    if terrains.is_empty() {
        return Ok(Vec::new());
    }

    terrains
        .iter()
        .map(|terrain| {
            let source = oil_terrain_source(runtime.api)?;
            let handle = runtime.api.gc_handle(source)?;
            let result = restore_oil_terrain(runtime, terrain, rotate_to_world, source);
            runtime.api.free_gc_handle(handle);
            result
        })
        .collect()
}

fn terrain_world_positions(
    terrain: &Terrain,
    rotate_to_world: bool,
) -> Result<Vec<MapVector>, OperationError> {
    terrain
        .control_points
        .iter()
        .copied()
        .map(|position| position_to_world(position, rotate_to_world, "terrain control point"))
        .collect()
}

pub(crate) fn rotate_terrain_grid_rows(rows: &[u32]) -> Vec<u32> {
    rows.iter()
        .rev()
        .map(|row| (row & OIL_GRID_MASK).reverse_bits() >> (u32::BITS - OIL_GRID_SIZE))
        .collect()
}

#[allow(clippy::too_many_lines)]
fn restore_oil_terrain(
    runtime: &Runtime,
    terrain: &Terrain,
    rotate_to_world: bool,
    source: *mut Object,
) -> Result<Value, OperationError> {
    if terrain.terrain_type != layout::TerrainType::Oil {
        return Err(OperationError::InvalidArguments(
            "only oil terrain is supported by build-2259 native restoration".into(),
        ));
    }
    let current = require_training_deploying(runtime)?;
    let api = runtime.api;
    let player = player_controller(runtime, current)?;
    let team_controller = api.invoke(player, "GetFightTeamController", &mut [])?;
    let control_points = terrain_world_positions(terrain, rotate_to_world)?;
    let centers = calculate_oil_terrain_positions(api, &control_points, source)?;
    let point_count = api.invoke_value::<i32>(source, "GetSubEffectCount", &mut [])?;
    if point_count != 7 || centers.len() != usize::try_from(point_count).unwrap_or_default() {
        return Err(OperationError::Rejected(format!(
            "sticky-oil generated {} positions for native point count {point_count}, expected seven",
            centers.len()
        )));
    }
    let expected_start = [
        i64::from(control_points[0].x) << 32,
        0,
        i64::from(control_points[0].y) << 32,
    ];
    let expected_end = [
        i64::from(control_points[1].x) << 32,
        0,
        i64::from(control_points[1].y) << 32,
    ];
    if centers.first() != Some(&expected_start) || centers.last() != Some(&expected_end) {
        return Err(OperationError::Rejected(format!(
            "sticky-oil generated endpoints differ: expected {expected_start:?}..{expected_end:?}, got {:?}..{:?}",
            centers.first(),
            centers.last()
        )));
    }
    let system = find_match_module(
        runtime,
        runtime.current_fight(),
        "GameRiver.Fight",
        "RangeItemSystem",
    )?;
    let mut range_item_type = OIL_RANGE_ITEM_TYPE;
    let controller = api.invoke(
        system,
        "GetRangeItemController",
        &mut [argument(&mut range_item_type)],
    )?;
    let items = api.invoke(controller, "GetItems", &mut [])?;
    let active_points = if terrain.grid_rows.is_empty() {
        (0..centers.len())
            .map(|index| {
                (
                    u32::try_from(index).expect("seven points fit u32"),
                    Vec::new(),
                )
            })
            .collect::<Vec<_>>()
    } else {
        terrain
            .grid_rows
            .iter()
            .map(|(&index, rows)| (index, rows.clone()))
            .collect::<Vec<_>>()
    };
    let mut restored = Vec::with_capacity(active_points.len());
    for (point_index, local_rows) in active_points {
        let center_index = usize::try_from(point_index).map_err(|_| {
            OperationError::InvalidArguments("terrain point index exceeds usize".into())
        })?;
        let mut center = *centers.get(center_index).ok_or_else(|| {
            OperationError::InvalidArguments(format!(
                "terrain point index {point_index} exceeds generated position count {}",
                centers.len()
            ))
        })?;
        if center[1] != 0 {
            return Err(OperationError::Rejected(format!(
                "sticky-oil generated non-ground height {} at point {point_index}",
                center[1]
            )));
        }
        let mut native_index = i32::try_from(point_index).map_err(|_| {
            OperationError::InvalidArguments("terrain point index exceeds native i32".into())
        })?;
        let mut round = RETAINED_OIL_ROUND;
        let mut use_grid = !local_rows.is_empty();
        let no_masks: *mut Object = std::ptr::null_mut();
        // Build 2259 exposes exactly one eight-argument AddItem overload. Its
        // closed generic Queue<ByteMask> parameter has no stable reflection
        // spelling through this IL2CPP runtime, so bind the unique arity.
        let method = api.method(
            api.object_class(system).ok_or_else(|| {
                OperationError::InvalidState("RangeItemSystem has no runtime class".into())
            })?,
            "AddItem",
            8,
        )?;
        let before_count = list_count(api, items)?;
        api.invoke_raw(
            method,
            system.cast(),
            &mut [
                argument(&mut range_item_type),
                object_argument(source),
                argument(&mut center),
                object_argument(team_controller),
                argument(&mut native_index),
                argument(&mut round),
                object_argument(no_masks),
                argument(&mut use_grid),
            ],
        )?;
        if list_count(api, items)? != before_count + 1 {
            return Err(OperationError::Rejected(
                "RangeItemSystem.AddItem did not register one oil terrain".into(),
            ));
        }
        let item = list_item(api, items, before_count)?;
        if api.invoke(item, "GetProvider", &mut [])? != source
            || api.invoke(item, "GetTeamController", &mut [])? != team_controller
            || api.invoke_value::<i32>(item, "GetRangeItemType", &mut [])? != OIL_RANGE_ITEM_TYPE
            || api.invoke_value::<[i64; 3]>(item, "GetPosition", &mut [])? != center
            || api.invoke_value::<FPoint>(item, "GetRange", &mut [])?.0 != OIL_RADIUS_RAW
            || api.invoke_value::<i32>(item, "get_Index", &mut [])? != native_index
            || api.invoke_value::<i32>(item, "get_Round", &mut [])? != round
            || api.invoke_value::<bool>(item, "IsGridMode", &mut [])? != use_grid
        {
            return Err(OperationError::Rejected(
                "restored oil terrain native readback did not match".into(),
            ));
        }

        let expected_rows = if rotate_to_world {
            rotate_terrain_grid_rows(&local_rows)
        } else {
            local_rows.clone()
        };
        if use_grid {
            overwrite_terrain_grid(api, item, &expected_rows)?;
            let readback = read_terrain_grid_rows(api, item)?;
            if readback != expected_rows {
                return Err(OperationError::Rejected(format!(
                    "restored oil terrain grid readback differs: expected {expected_rows:?}, got {readback:?}"
                )));
            }
        }
        restored.push(json!({
            "native_index": native_index,
            "position_raw": {"x": center[0], "y": center[2]},
            "grid_rows": local_rows,
        }));
    }
    Ok(json!({
        "type": "oil",
        "control_points": terrain.control_points.iter().map(|position| {
            json!({"x": position.x, "y": position.y})
        }).collect::<Vec<_>>(),
        "active_points": restored,
    }))
}

fn calculate_oil_terrain_positions(
    api: Api,
    control_points: &[MapVector],
    source: *mut Object,
) -> Result<Vec<[i64; 3]>, OperationError> {
    if control_points.len() != 2 {
        return Err(OperationError::InvalidArguments(
            "sticky-oil terrain requires exactly two control points".into(),
        ));
    }
    let point_count = api.invoke_value::<i32>(source, "GetSubEffectCount", &mut [])?;
    if point_count != 7 {
        return Err(OperationError::Rejected(format!(
            "sticky-oil native point count is {point_count}, expected seven"
        )));
    }

    // CalculateAttackPositions is private and absent from build 2259's runtime
    // method table. Reproduce its line branch with the same public FixedMath
    // primitives, preserving their exact Q32.32 rounding behavior.
    let vector_class = api.class("GRUtility.dll", "FixedMath", "FVector3")?;
    let point_class = api.class("GRUtility.dll", "FixedMath", "FPoint")?;
    let subtract = api.class_method_with_parameter_types(
        vector_class,
        "op_Subtraction",
        &["FixedMath.FVector3", "FixedMath.FVector3"],
    )?;
    let clamp = api.class_method_with_parameter_types(
        vector_class,
        "ClampMagnitude",
        &["FixedMath.FVector3", "FixedMath.FPoint"],
    )?;
    let add = api.class_method_with_parameter_types(
        vector_class,
        "op_Addition",
        &["FixedMath.FVector3", "FixedMath.FVector3"],
    )?;
    let dot = api.class_method_with_parameter_types(
        vector_class,
        "Dot",
        &["FixedMath.FVector3", "FixedMath.FVector3"],
    )?;
    let sqrt = api.class_method_with_parameter_types(point_class, "Sqrt", &["FixedMath.FPoint"])?;
    let divide = api.class_method_with_parameter_types(
        point_class,
        "op_Division",
        &["FixedMath.FPoint", "FixedMath.FPoint"],
    )?;
    let mut start = [
        i64::from(control_points[0].x) << 32,
        0,
        i64::from(control_points[0].y) << 32,
    ];
    let mut end = [
        i64::from(control_points[1].x) << 32,
        0,
        i64::from(control_points[1].y) << 32,
    ];
    let boxed_direction = api.invoke_raw(
        subtract,
        std::ptr::null_mut(),
        &mut [argument(&mut end), argument(&mut start)],
    )?;
    let direction = api.unbox::<[i64; 3]>(boxed_direction, "FVector3 subtraction")?;
    let mut dot_left = direction;
    let mut dot_right = direction;
    let boxed_squared = api.invoke_raw(
        dot,
        std::ptr::null_mut(),
        &mut [argument(&mut dot_left), argument(&mut dot_right)],
    )?;
    let mut squared = api.unbox::<FPoint>(boxed_squared, "FVector3 dot")?;
    let boxed_magnitude =
        api.invoke_raw(sqrt, std::ptr::null_mut(), &mut [argument(&mut squared)])?;
    let magnitude = api.unbox::<FPoint>(boxed_magnitude, "FPoint square root")?;
    let mut magnitude_argument = magnitude;
    let mut divisor = FPoint(i64::from(point_count - 1) << 32);
    let boxed_step = api.invoke_raw(
        divide,
        std::ptr::null_mut(),
        &mut [argument(&mut magnitude_argument), argument(&mut divisor)],
    )?;
    let step = api.unbox::<FPoint>(boxed_step, "FPoint division")?;

    let mut centers = Vec::with_capacity(usize::try_from(point_count).unwrap_or_default());
    for index in 0..point_count {
        let mut direction_argument = direction;
        let mut max_length = FPoint(step.0.checked_mul(i64::from(index)).ok_or_else(|| {
            OperationError::InvalidArguments("sticky-oil step length overflows Q32.32".into())
        })?);
        let boxed_offset = api.invoke_raw(
            clamp,
            std::ptr::null_mut(),
            &mut [argument(&mut direction_argument), argument(&mut max_length)],
        )?;
        let mut offset = api.unbox::<[i64; 3]>(boxed_offset, "FVector3 clamp")?;
        let boxed_center = api.invoke_raw(
            add,
            std::ptr::null_mut(),
            &mut [argument(&mut start), argument(&mut offset)],
        )?;
        centers.push(api.unbox::<[i64; 3]>(boxed_center, "FVector3 addition")?);
    }
    Ok(centers)
}

fn oil_terrain_source(api: Api) -> Result<*mut Object, OperationError> {
    let factory = api.class("GRCore.dll", "GameRiver", "CommanderSkillFactory")?;
    let create = api.class_method_with_parameter_types(factory, "Create", &["System.Int32"])?;
    let mut id = OIL_COMMANDER_SKILL_ID;
    let source = api.invoke_raw(create, std::ptr::null_mut(), &mut [argument(&mut id)])?;
    let expected = api.class("GRCore.dll", "GameRiver", "CS_Oil")?;
    if api.object_class(source) != Some(expected)
        || api.invoke_value::<i32>(source, "GetID", &mut [])? != id
        || api.invoke_value::<i32>(source, "GetRangeItemType", &mut [])? != OIL_RANGE_ITEM_TYPE
        || api
            .invoke_value::<FPoint>(source, "GetSubEffectRange", &mut [])?
            .0
            != OIL_RADIUS_RAW
    {
        return Err(OperationError::Rejected(
            "sticky-oil range item provider readback did not match".into(),
        ));
    }
    Ok(source)
}

fn overwrite_terrain_grid(api: Api, item: *mut Object, rows: &[u32]) -> Result<(), OperationError> {
    let grid = api.invoke(item, "GetGridBlock", &mut [])?;
    let class = api
        .object_class(grid)
        .ok_or_else(|| OperationError::InvalidState("terrain grid has no runtime class".into()))?;
    let size = api.invoke_value::<Vector2Int>(grid, "get_Size", &mut [])?;
    if size
        != (Vector2Int {
            x: OIL_GRID_SIZE.cast_signed(),
            y: OIL_GRID_SIZE.cast_signed(),
        })
    {
        return Err(OperationError::Rejected(format!(
            "native oil terrain grid is {}x{}, expected {OIL_GRID_SIZE}x{OIL_GRID_SIZE}",
            size.x, size.y
        )));
    }
    // Keep the RangeItem's serialized mask queue in sync as well as its live
    // GridBlock, so a later native round snapshot would preserve this shape.
    let masks = api.invoke(item, "get_DetailMasks", &mut [])?;
    if masks.is_null() {
        return Err(OperationError::Rejected(
            "grid-mode oil terrain has no native DetailMasks queue".into(),
        ));
    }
    api.invoke_void(masks, "Clear", &mut [])?;
    let mask_class = api.class("GRUtility.dll", "GameRiver", "ByteMask")?;
    let mut mask = api.new_object(mask_class)?;
    api.invoke_void(masks, "Enqueue", &mut [object_argument(mask)])?;
    for x in 0..OIL_GRID_SIZE {
        for row in rows.iter().take(OIL_GRID_SIZE as usize) {
            if api.invoke_value::<bool>(mask, "IsFull", &mut [])? {
                mask = api.new_object(mask_class)?;
                api.invoke_void(masks, "Enqueue", &mut [object_argument(mask)])?;
            }
            let mut active = row & (1_u32 << x) != 0;
            api.invoke_void(mask, "Push", &mut [argument(&mut active)])?;
        }
    }
    let mut columns = vec![0_u32; 32];
    for (x, column) in columns.iter_mut().take(OIL_GRID_SIZE as usize).enumerate() {
        for (y, row) in rows.iter().enumerate() {
            if row & (1_u32 << x) != 0 {
                *column |= 1_u32 << (u32::BITS as usize - 1 - y);
            }
        }
    }
    let columns_array: *mut Object = api.field_value(grid, api.field(class, "grids")?)?;
    api.overwrite_value_array(columns_array, &columns)?;
    Ok(())
}

fn read_terrain_grid_rows(api: Api, item: *mut Object) -> Result<Vec<u32>, OperationError> {
    let grid = api.invoke(item, "GetGridBlock", &mut [])?;
    let class = api
        .object_class(grid)
        .ok_or_else(|| OperationError::InvalidState("terrain grid has no runtime class".into()))?;
    let size = api.invoke_value::<Vector2Int>(grid, "get_Size", &mut [])?;
    let columns_array: *mut Object = api.field_value(grid, api.field(class, "grids")?)?;
    let columns = api.value_array::<u32>(columns_array, 32)?;
    crate::capture::terrain_grid_rows_from_native_columns(
        &columns,
        u32::try_from(size.x).map_err(|_| {
            OperationError::InvalidState(format!("negative terrain grid width {}", size.x))
        })?,
        u32::try_from(size.y).map_err(|_| {
            OperationError::InvalidState(format!("negative terrain grid height {}", size.y))
        })?,
    )
    .map_err(OperationError::Rejected)
}

fn apply_battle_skills(
    runtime: &Runtime,
    skills: &[BattleSkill],
    rotate_to_world: bool,
) -> Result<Vec<Value>, OperationError> {
    skills
        .iter()
        .map(|skill| apply_battle_skill(runtime, skill, rotate_to_world))
        .collect()
}

#[allow(clippy::too_many_lines)]
fn apply_battle_skill(
    runtime: &Runtime,
    desired: &BattleSkill,
    rotate_to_world: bool,
) -> Result<Value, OperationError> {
    let context = describe_battle_skill(desired);
    let mut world_positions = battle_skill_world_positions(desired, rotate_to_world)?;
    let current = require_training_deploying(runtime)?;
    let controller = player_controller(runtime, current)?;
    let manager = runtime
        .api
        .invoke(controller, "GetCommanderSkillManager", &mut [])?;
    let mut id = desired.commander_skill_id;
    let before = runtime
        .api
        .invoke(manager, "GetCommanderSkillByID", &mut [argument(&mut id)])?;
    if !before.is_null() {
        return Err(OperationError::Rejected(format!(
            "{context} already exists before layout application"
        )));
    }
    let before_count = runtime
        .api
        .invoke_value::<i32>(manager, "GetSkillCount", &mut [])?;
    if !(0..=16_384).contains(&before_count) {
        return Err(OperationError::InvalidState(format!(
            "invalid commander skill count {before_count}"
        )));
    }

    add_test_inventory(runtime, desired.commander_skill_id, "MAD_AddCommanderSkill")
        .map_err(|error| error.context(&format!("provision {context}")))?;

    let provisioned =
        runtime
            .api
            .invoke(manager, "GetCommanderSkillByID", &mut [argument(&mut id)])?;
    let after_count = runtime
        .api
        .invoke_value::<i32>(manager, "GetSkillCount", &mut [])?;
    if provisioned.is_null()
        || runtime
            .api
            .invoke_value::<i32>(provisioned, "GetID", &mut [])?
            != desired.commander_skill_id
        || runtime
            .api
            .invoke_value::<bool>(provisioned, "get_IsActive", &mut [])?
        || after_count != before_count + 1
    {
        return Err(OperationError::Rejected(format!(
            "{context} provisioning readback did not match"
        )));
    }
    let manager_index = runtime.api.invoke_value::<i32>(
        manager,
        "GetCommanderSkillIndex",
        &mut [object_argument(provisioned)],
    )?;
    if manager_index < 0 {
        return Err(OperationError::Rejected(format!(
            "{context} has no stable manager index"
        )));
    }

    release_battle_skill(runtime, desired.commander_skill_id, &mut world_positions)
        .map_err(|error| error.context(&format!("release {context}")))?;

    let skill_after =
        runtime
            .api
            .invoke(manager, "GetCommanderSkillByID", &mut [argument(&mut id)])?;
    let index_after = runtime.api.invoke_value::<i32>(
        manager,
        "GetCommanderSkillIndex",
        &mut [object_argument(skill_after)],
    )?;
    let active = runtime
        .api
        .invoke_value::<bool>(skill_after, "get_IsActive", &mut [])?;
    if skill_after != provisioned || index_after != manager_index || !active {
        return Err(OperationError::Rejected(format!(
            "{context} active skill readback did not match"
        )));
    }

    let mut release_data: *mut Object = std::ptr::null_mut();
    let found = runtime.api.invoke_value::<bool>(
        manager,
        "TryGetReleaseCommanderSkillData",
        &mut [object_argument(skill_after), argument(&mut release_data)],
    )?;
    if !found || release_data.is_null() {
        return Err(OperationError::Rejected(format!(
            "{context} has no release-data readback"
        )));
    }
    let release_skill = runtime.api.invoke(
        release_data,
        "GameRiver.Fight.IReleaseCommanderSkillInfo.GetSkill",
        &mut [],
    )?;
    let release_positions = runtime.api.invoke(
        release_data,
        "GameRiver.Fight.IReleaseCommanderSkillInfo.GetPositions",
        &mut [],
    )?;
    let release_round = runtime.api.invoke_value::<i32>(
        release_data,
        "GameRiver.Fight.IReleaseCommanderSkillInfo.GetRound",
        &mut [],
    )?;
    if release_skill != skill_after
        || !map_vector_list_matches(runtime.api, release_positions, &world_positions)?
    {
        return Err(OperationError::Rejected(format!(
            "{context} release-data readback did not match"
        )));
    }

    Ok(json!({
        "type": desired.type_name,
        "positions": desired.positions.iter().map(|position| {
            json!({"x": position.x, "y": position.y})
        }).collect::<Vec<_>>(),
        "active": active,
        "round": release_round
    }))
}

fn map_vector_list_matches(
    api: Api,
    list: *mut Object,
    expected: &[MapVector],
) -> Result<bool, OperationError> {
    if list_count(api, list)? != i32::try_from(expected.len()).unwrap_or(-1) {
        return Ok(false);
    }
    for (index, expected) in expected.iter().enumerate() {
        let mut index = i32::try_from(index).map_err(|_| {
            OperationError::InvalidState("battle skill position index overflow".into())
        })?;
        let actual =
            api.invoke_value::<MapVector>(list, "get_Item", &mut [argument(&mut index)])?;
        if actual.x != expected.x || actual.y != expected.y {
            return Ok(false);
        }
    }
    Ok(true)
}

fn apply_techs(
    runtime: &Runtime,
    current: *mut Object,
    desired: &Techs,
) -> Result<Value, OperationError> {
    let controller = player_controller(runtime, current)?;
    let officer_manager = runtime
        .api
        .invoke(controller, "GetOfficerManager", &mut [])?;
    let mut officers = Vec::with_capacity(desired.officers.len());
    for &officer_id in &desired.officers {
        let mut id = officer_id;
        let before = runtime
            .api
            .invoke(officer_manager, "GetOfficer", &mut [argument(&mut id)])?;
        if !before.is_null() {
            return Err(OperationError::Rejected(format!(
                "officer ID {officer_id} already exists before layout application"
            )));
        }
        let action = new_player_test_action(runtime.api, "MAD_AddOfficer", controller)?;
        runtime
            .api
            .invoke_void(action, "set_ID", &mut [argument(&mut id)])?;
        perform_test(runtime.api, current, action)
            .map_err(|error| error.context(&format!("add officer ID {officer_id}")))?;
        let readback =
            runtime
                .api
                .invoke(officer_manager, "GetOfficer", &mut [argument(&mut id)])?;
        if readback.is_null()
            || runtime
                .api
                .invoke_value::<i32>(readback, "GetID", &mut [])?
                != officer_id
        {
            return Err(OperationError::Rejected(format!(
                "officer ID {officer_id} readback mismatch"
            )));
        }
        officers.push(officer_id);
    }

    let mut units = Vec::with_capacity(desired.units.len());
    let config = config_instance(runtime)?;
    for &technology_id in &desired.units {
        let unit_id = technology_owner(runtime, config, technology_id)?;
        if read_unit_technology(runtime, controller, unit_id, technology_id)?.is_some() {
            return Err(OperationError::Rejected(format!(
                "unit technology ID {technology_id} already exists before layout application"
            )));
        }
        change_technology(runtime, unit_id, technology_id, "MAD_AddTechnology")
            .map_err(|error| error.context(&format!("add unit technology ID {technology_id}")))?;
        if read_unit_technology(runtime, controller, unit_id, technology_id)?.is_none() {
            return Err(OperationError::Rejected(format!(
                "unit technology ID {technology_id} was not present after add"
            )));
        }
        change_technology(runtime, unit_id, technology_id, "MAD_ActiveTechnology").map_err(
            |error| error.context(&format!("activate unit technology ID {technology_id}")),
        )?;
        if read_unit_technology(runtime, controller, unit_id, technology_id)? != Some(true) {
            return Err(OperationError::Rejected(format!(
                "unit technology ID {technology_id} did not become active"
            )));
        }
        units.push(json!({
            "technology_id": technology_id,
            "unit_id": unit_id,
            "active": true
        }));
    }
    Ok(json!({"officers": officers, "units": units}))
}

fn read_unit_technology(
    runtime: &Runtime,
    controller: *mut Object,
    mut unit_id: i32,
    mut technology_id: i32,
) -> Result<Option<bool>, OperationError> {
    let manager = runtime
        .api
        .invoke(controller, "GetTechnologyManager", &mut [])?;
    let technology = runtime.api.invoke(
        manager,
        "GetTechnology",
        &mut [argument(&mut unit_id), argument(&mut technology_id)],
    )?;
    if technology.is_null() {
        return Ok(None);
    }
    let readback_id = runtime
        .api
        .invoke_value::<i32>(technology, "GetID", &mut [])?;
    if readback_id != technology_id {
        return Err(OperationError::Rejected(format!(
            "unit technology readback returned ID {readback_id} for requested ID {technology_id}"
        )));
    }
    Ok(Some(runtime.api.invoke_value::<bool>(
        technology,
        "IsActive",
        &mut [],
    )?))
}

/// Applies each fixed tower's strengthening level, keyed by its position.
///
/// A layout keys `tower_strengthen_levels` the way `PAD_StrengthenTower.Index`
/// does, so the position in the list is the building-manager index to act on.
/// An empty list leaves every tower where a fresh scene puts it, at level 0.
fn apply_tower_strengthen_levels(
    runtime: &Runtime,
    levels: &[i32],
) -> Result<Value, OperationError> {
    validate_fixed_tower_positions(runtime)?;
    for (position, &level) in levels.iter().enumerate() {
        let index = i32::try_from(position).map_err(|_| {
            OperationError::InvalidArguments("tower position does not fit an index".into())
        })?;
        apply_tower_strength(runtime, index, level, &format!("tower {position}"))?;
    }
    Ok(json!(levels))
}

/// Activates the Energy Tower skills a round released, and only those.
///
/// A layout carries the skills whose effect a fight can see, so each of them is
/// either activated here or required to be inactive. The rest of the tower's
/// skills buy supply or discount shopping, which no layout describes.
fn apply_energy_tower_skills(runtime: &Runtime, desired: &[i32]) -> Result<Value, OperationError> {
    for id in FIGHT_VISIBLE_ENERGY_TOWER_SKILLS {
        apply_energy_tower_enhancement(runtime, id, desired.contains(&id))?;
    }
    Ok(json!(desired))
}

/// Checks that each fixed tower sits where a layout says its level is keyed.
///
/// `docs/state.md` measures the mapping: position 0 is the Research Center and
/// position 1 the Energy Tower. It is still checked here on every apply, so a
/// build that reorders its buildings fails loudly rather than silently
/// strengthening the wrong tower.
fn validate_fixed_tower_positions(runtime: &Runtime) -> Result<(), OperationError> {
    for (kind, position) in [
        (ENERGY_TOWER_KIND, ENERGY_TOWER_POSITION),
        (RESEARCH_CENTER_KIND, RESEARCH_CENTER_POSITION),
    ] {
        let index = resolve_core_tower(runtime, kind)?;
        let expected = i32::try_from(position).expect("a tower position fits an index");
        if index != expected {
            return Err(OperationError::InvalidState(format!(
                "core tower kind {kind} sits at building-manager position {index}, and a layout keys its level by {expected}"
            )));
        }
    }
    Ok(())
}

fn apply_tower_strength(
    runtime: &Runtime,
    manager_index: i32,
    target_level: i32,
    kind: &str,
) -> Result<(), OperationError> {
    let level = strengthen_tower(runtime, manager_index, target_level)
        .map_err(|error| error.context(&format!("strengthen {kind}")))?;
    if level == target_level {
        Ok(())
    } else {
        Err(OperationError::Rejected(format!(
            "{kind} strength readback {level} did not match target {target_level}"
        )))
    }
}

fn apply_energy_tower_enhancement(
    runtime: &Runtime,
    skill_id: i32,
    desired: bool,
) -> Result<(), OperationError> {
    let skill = require_energy_tower_skill(runtime, skill_id)?;
    let before = runtime
        .api
        .invoke_value::<bool>(skill, "IsActive", &mut [])?;
    if desired {
        if before {
            return Err(OperationError::Rejected(format!(
                "energy_tower skill {skill_id} was already active"
            )));
        }
        activate_energy_tower_skill(runtime, skill_id)
            .map_err(|error| error.context(&format!("activate energy_tower skill {skill_id}")))?;
    } else if before {
        return Err(OperationError::Rejected(format!(
            "undeclared energy_tower skill {skill_id} is active"
        )));
    }
    Ok(())
}

fn require_energy_tower_skill(
    runtime: &Runtime,
    mut id: i32,
) -> Result<*mut Object, OperationError> {
    let current = require_training_deploying(runtime)?;
    let controller = player_controller(runtime, current)?;
    let manager = runtime
        .api
        .invoke(controller, "GetEnergyTowerManager", &mut [])?;
    let skill = runtime
        .api
        .invoke(manager, "GetSkill", &mut [argument(&mut id)])?;
    if skill.is_null() {
        Err(OperationError::InvalidArguments(format!(
            "energy_tower skill {id} is absent from the runtime catalog"
        )))
    } else {
        Ok(skill)
    }
}

fn resolve_core_tower(runtime: &Runtime, expected_kind: i32) -> Result<i32, OperationError> {
    let current = require_training_deploying(runtime)?;
    let controller = player_controller(runtime, current)?;
    let manager = runtime
        .api
        .invoke(controller, "GetBuildingManager", &mut [])?;
    let buildings = runtime.api.invoke(manager, "GetBuildings", &mut [])?;
    let mut resolved = None;
    for list_index in 0..list_count(runtime.api, buildings)? {
        let building = list_item(runtime.api, buildings, list_index)?;
        let data = runtime.api.invoke(building, "GetBuildingData", &mut [])?;
        let kind = runtime
            .api
            .invoke_value::<i32>(data, "get_BuildingType", &mut [])?;
        if kind != expected_kind {
            continue;
        }
        if resolved.is_some() {
            return Err(OperationError::InvalidState(format!(
                "multiple core towers have building kind {expected_kind}"
            )));
        }
        let index = runtime.api.invoke_value::<i32>(
            manager,
            "GetBuildingIndex",
            &mut [object_argument(building)],
        )?;
        let mut lookup_index = index;
        let readback = runtime.api.invoke(
            manager,
            "GetBuildingByIndex",
            &mut [argument(&mut lookup_index)],
        )?;
        if index < 0 || readback != building {
            return Err(OperationError::InvalidState(format!(
                "core tower kind {expected_kind} has no stable manager index"
            )));
        }
        resolved = Some(index);
    }
    resolved.ok_or_else(|| {
        OperationError::InvalidState(format!("core tower kind {expected_kind} is unavailable"))
    })
}

fn apply_formations(
    runtime: &Runtime,
    current: *mut Object,
    placements: &[Placement],
    rotate_to_world: bool,
) -> Result<Vec<Value>, OperationError> {
    let result: Result<Vec<Value>, OperationError> = (|| -> Result<Vec<Value>, OperationError> {
        let mut applied = Vec::new();
        for placement in placements {
            // MAD_AddUnit.UIDX assigns the requested index directly. Missing
            // indices do not create temporary units or native RVO agents.
            applied.push(apply_formation(runtime, placement, rotate_to_world)?);
        }
        Ok(applied)
    })()
    .map_err(|error| {
        error.context(if rotate_to_world {
            "apply red side"
        } else {
            "apply blue side"
        })
    });
    result.and_then(|formations| {
        if runtime.current_match() == current {
            Ok(formations)
        } else {
            Err(OperationError::InvalidState(
                "active match changed while applying layout".into(),
            ))
        }
    })
}

fn apply_formation(
    runtime: &Runtime,
    placement: &Placement,
    rotate_to_world: bool,
) -> Result<Value, OperationError> {
    let world_position = layout_world_position(placement, rotate_to_world)?;
    match placement.native {
        NativeFormation::Unit(unit_id) => {
            apply_unit_formation(runtime, placement, unit_id, world_position)
        }
        NativeFormation::Construction(construction_id) => {
            apply_construction_formation(runtime, placement, construction_id, world_position)
        }
        NativeFormation::Contraption(contraption_id) => {
            apply_contraption_formation(runtime, placement, contraption_id, world_position)
        }
    }
}

fn apply_unit_formation(
    runtime: &Runtime,
    placement: &Placement,
    unit_id: i32,
    world_position: MapVector,
) -> Result<Value, OperationError> {
    let level = placement.level.ok_or_else(|| {
        OperationError::InvalidState(format!(
            "{} has no unit level",
            describe_placement(placement)
        ))
    })?;
    let unit_index = placement.index.ok_or_else(|| {
        OperationError::InvalidState(format!(
            "{} has no stable unit index",
            describe_placement(placement)
        ))
    })?;
    let exp = placement.exp.ok_or_else(|| {
        OperationError::InvalidState(format!(
            "{} has no unit experience",
            describe_placement(placement)
        ))
    })?;
    let created_index = add_unit(
        runtime,
        unit_id,
        level,
        unit_index,
        world_position,
        placement.rotated,
    )
    .map_err(|error| error.context(&format!("place {}", describe_placement(placement))))?;
    if created_index != unit_index {
        return Err(OperationError::Rejected(format!(
            "{} requested unit index {unit_index}, created {created_index}",
            describe_placement(placement)
        )));
    }
    let mut readback = unit_status(runtime, unit_index)
        .map_err(|error| error.context(&format!("read {}", describe_placement(placement))))?;
    let mut changed = false;
    if readback.travelling != placement.travelling {
        set_unit_travelling(runtime, unit_index, world_position, placement.travelling).map_err(
            |error| {
                error.context(&format!(
                    "set travelling for {}",
                    describe_placement(placement)
                ))
            },
        )?;
        changed = true;
    }
    if readback.exp != exp {
        set_unit_exp(runtime, unit_index, exp).map_err(|error| {
            error.context(&format!("set exp for {}", describe_placement(placement)))
        })?;
        changed = true;
    }
    if changed {
        readback = unit_status(runtime, unit_index)
            .map_err(|error| error.context(&format!("read {}", describe_placement(placement))))?;
    }
    verify_unit_readback(placement, unit_id, level, world_position, &readback)?;
    if let Some(equipment_id) = placement.equipment {
        add_test_inventory(runtime, equipment_id, "MAD_AddEquipment").map_err(|error| {
            error.context(&format!(
                "add equipment for {}",
                describe_placement(placement)
            ))
        })?;
        equip_unit(runtime, equipment_id, unit_index)
            .map_err(|error| error.context(&format!("equip {}", describe_placement(placement))))?;
    }
    Ok(json!({
        "type": placement.type_name,
        "unit_index": unit_index,
        "level": level,
        "exp": readback.exp,
        "position": {"x": placement.position.x, "y": placement.position.y},
        "rotated": placement.rotated,
        "travelling": readback.travelling,
        "equipment": placement.equipment
    }))
}

fn apply_construction_formation(
    runtime: &Runtime,
    placement: &Placement,
    construction_id: i32,
    world_position: MapVector,
) -> Result<Value, OperationError> {
    let current = require_training_deploying(runtime)?;
    let controller = player_controller(runtime, current)?;
    let manager = runtime
        .api
        .invoke(controller, "GetConstructionManager", &mut [])?;
    let elements = runtime
        .api
        .invoke(manager, "GetConstructionElements", &mut [])?;
    let mut retained_index = None;
    for offset in 0..list_count(runtime.api, elements)? {
        let element = list_item(runtime.api, elements, offset)?;
        let data = runtime
            .api
            .invoke(element, "GetConstructionData", &mut [])?;
        let id = runtime.api.invoke_value::<i32>(data, "GetID", &mut [])?;
        let position = runtime
            .api
            .invoke_value::<MapVector>(element, "GetPosition", &mut [])?;
        if id == construction_id && position == world_position {
            let index = runtime.api.invoke_value::<i32>(
                manager,
                "GetConstructionIndex",
                &mut [object_argument(element)],
            )?;
            if index < 0 || retained_index.replace(index).is_some() {
                return Err(OperationError::Rejected(
                    "ambiguous retained construction".into(),
                ));
            }
        }
    }
    let declared_index = declared_index(placement, "construction")?;
    let retained = retained_index.is_some();
    let construction_index = match retained_index {
        Some(index) => {
            if index != declared_index {
                return Err(OperationError::Rejected(format!(
                    "retained construction {} carries native index {index}, layout declares {declared_index}",
                    describe_placement(placement)
                )));
            }
            index
        }
        None => construction(runtime, construction_id, world_position, declared_index)
            .map_err(|error| error.context(&format!("place {}", describe_placement(placement))))?,
    };
    Ok(json!({
        "type": placement.type_name,
        "construction_index": construction_index,
        "position": {"x": placement.position.x, "y": placement.position.y},
        "retained": retained
    }))
}

fn apply_contraption_formation(
    runtime: &Runtime,
    placement: &Placement,
    contraption_id: i32,
    world_position: MapVector,
) -> Result<Value, OperationError> {
    let declared_index = declared_index(placement, "contraption")?;
    let contraption_index = contraption(
        runtime,
        contraption_id,
        world_position,
        None,
        declared_index,
    )
    .map_err(|error| error.context(&format!("place {}", describe_placement(placement))))?;
    if contraption_index != declared_index {
        return Err(OperationError::Rejected(format!(
            "contraption {} was recorded under index {contraption_index}, layout declares {declared_index}",
            describe_placement(placement)
        )));
    }
    Ok(json!({
        "type": placement.type_name,
        "contraption_index": contraption_index,
        "position": {"x": placement.position.x, "y": placement.position.y}
    }))
}

/// Construct a data source without adding inventory or scheduling a new skill.
pub(crate) fn airdrop_shield_source(api: Api) -> Result<*mut Object, Il2CppError> {
    let factory = api.class("GRCore.dll", "GameRiver", "CommanderSkillFactory")?;
    let create = api.class_method_with_parameter_types(factory, "Create", &["System.Int32"])?;
    let mut id = 800_001_i32;
    let source = api.invoke_raw(create, std::ptr::null_mut(), &mut [argument(&mut id)])?;
    let expected = api.class("GRCore.dll", "GameRiver", "CS_EnergyShield")?;
    if api.object_class(source) != Some(expected)
        || api.invoke_value::<i32>(source, "GetID", &mut [])? != id
    {
        return Err(Il2CppError::InvalidValue(
            "airdrop shield data source mismatch".into(),
        ));
    }
    Ok(source)
}

fn restore_airdrop_shield(runtime: &Runtime, position: MapVector) -> Result<i32, OperationError> {
    let current = require_training_deploying(runtime)?;
    let api = runtime.api;
    let controller = player_controller(runtime, current)?;
    let team_controller = api.invoke(controller, "GetFightTeamController", &mut [])?;
    let team = api.invoke(team_controller, "GetTeam", &mut [])?;
    let group = api.invoke(team, "GetFightGroup", &mut [])?;
    let system = find_match_module(
        runtime,
        runtime.current_fight(),
        "GameRiver.Fight",
        "AdvancedEnergyShieldSystem",
    )?;
    let all = api.invoke(system, "GetEnergyShields", &mut [object_argument(group)])?;
    let active = api.invoke(
        system,
        "GetActiveEnergyShields",
        &mut [object_argument(group)],
    )?;
    let before_all = list_count(api, all)?;
    let before_active = list_count(api, active)?;
    let source = airdrop_shield_source(api)?;
    let handle = api.gc_handle(source)?;
    let result = (|| {
        // FVector3 consists of three Q32.32 values; no floating-point conversion.
        let mut center = [i64::from(position.x) << 32, 0, i64::from(position.y) << 32];
        // This native overload registers the object, activates it with reset=true,
        // and dispatches OnAddEnergyShield. Calling FightEnergyShield.Active alone
        // would not insert it into the manager's active collection.
        api.invoke_void(
            system,
            "Create",
            &mut [
                object_argument(source),
                argument(&mut center),
                object_argument(team_controller),
            ],
        )?;
        if list_count(api, all)? != before_all + 1 || list_count(api, active)? != before_active + 1
        {
            return Err(OperationError::Rejected(
                "airdrop shield was not registered and activated".into(),
            ));
        }
        let shield = list_item(api, all, before_all)?;
        let transform = api.invoke(shield, "GetFightTransform", &mut [])?;
        let radius = api
            .invoke_value::<FPoint>(source, "GetSubEffectRange", &mut [])?
            .0;
        let energy = api.invoke_value::<i32>(
            source,
            "GameRiver.IAdvancedEnergyShieldDataSource.GetAdvancedEnergyShieldValue",
            &mut [],
        )?;
        if list_item(api, active, before_active)? != shield
            || api.invoke(shield, "get_EnergyShieldData", &mut [])? != source
            || api.invoke(shield, "GetTeamController", &mut [])? != team_controller
            || !api.invoke(shield, "GetOwner", &mut [])?.is_null()
            || !api.invoke_value::<bool>(shield, "get_IsActive", &mut [])?
            || !api.invoke_value::<bool>(shield, "IsResetNextRound", &mut [])?
            || api.invoke_value::<bool>(shield, "IsShortLifeTime", &mut [])?
            || api.invoke_value::<[i64; 3]>(transform, "GetPositionInt3D", &mut [])? != center
            || radius <= 0
            || api.invoke_value::<FPoint>(shield, "GetRadius", &mut [])?.0 != radius
            || energy <= 0
            || api.invoke_value::<i32>(shield, "GetMaxEnergy", &mut [])? != energy
            || api.invoke_value::<i32>(shield, "GetEnergy", &mut [])? != energy
        {
            return Err(OperationError::Rejected(
                "airdrop shield native readback did not match".into(),
            ));
        }
        Ok(before_all)
    })();
    api.free_gc_handle(handle);
    result
}

fn layout_world_position(
    placement: &Placement,
    rotate_to_world: bool,
) -> Result<MapVector, OperationError> {
    position_to_world(
        placement.position,
        rotate_to_world,
        &describe_placement(placement),
    )
}

fn battle_skill_world_positions(
    skill: &BattleSkill,
    rotate_to_world: bool,
) -> Result<Vec<MapVector>, OperationError> {
    let context = describe_battle_skill(skill);
    skill
        .positions
        .iter()
        .copied()
        .map(|position| position_to_world(position, rotate_to_world, &context))
        .collect()
}

fn position_to_world(
    position: layout::Position,
    rotate_to_world: bool,
    context: &str,
) -> Result<MapVector, OperationError> {
    if !rotate_to_world {
        return Ok(MapVector {
            x: position.x,
            y: position.y,
        });
    }
    let x = position.x.checked_neg().ok_or_else(|| {
        OperationError::InvalidArguments(format!(
            "{context} x coordinate cannot be rotated into world coordinates"
        ))
    })?;
    let y = position.y.checked_neg().ok_or_else(|| {
        OperationError::InvalidArguments(format!(
            "{context} y coordinate cannot be rotated into world coordinates"
        ))
    })?;
    Ok(MapVector { x, y })
}

fn verify_unit_readback(
    placement: &Placement,
    unit_id: i32,
    level: i32,
    world_position: MapVector,
    readback: &UnitReadback,
) -> Result<(), OperationError> {
    let matches = readback.id == unit_id
        && readback.level == level - 1
        && readback.exp == placement.exp.unwrap_or(0)
        && readback.position == world_position
        && readback.rotated == placement.rotated
        && readback.travelling == placement.travelling;
    if matches {
        Ok(())
    } else {
        Err(OperationError::Rejected(format!(
            "{} readback mismatch: {readback:?}",
            describe_placement(placement)
        )))
    }
}

fn describe_placement(placement: &Placement) -> String {
    let kind = match placement.native {
        NativeFormation::Unit(_) => "formation",
        NativeFormation::Construction(_) => "construction",
        NativeFormation::Contraption(_) => "contraption",
    };
    format!(
        "{kind} type {:?} at local position ({}, {})",
        placement.type_name, placement.position.x, placement.position.y
    )
}

fn describe_battle_skill(skill: &BattleSkill) -> String {
    let positions = skill
        .positions
        .iter()
        .map(|position| json!({"x": position.x, "y": position.y}))
        .collect::<Vec<_>>();
    format!(
        "battle skill type {:?} at local positions {}",
        skill.type_name,
        Value::Array(positions)
    )
}

fn change_technology(
    runtime: &Runtime,
    mut unit_id: i32,
    mut technology_id: i32,
    class_name: &str,
) -> Result<(), OperationError> {
    let current = require_match(runtime)?;
    let controller = player_controller(runtime, current)?;
    let action = new_player_test_action(runtime.api, class_name, controller)?;
    runtime
        .api
        .invoke_void(action, "set_UnitID", &mut [argument(&mut unit_id)])?;
    runtime
        .api
        .invoke_void(action, "set_TechID", &mut [argument(&mut technology_id)])?;
    perform_test(runtime.api, current, action)
}

fn find_unit(
    runtime: &Runtime,
    controller: *mut Object,
    unit_index: &mut i32,
) -> Result<(*mut Object, *mut Object), OperationError> {
    let manager = runtime.api.invoke(controller, "GetUnitManager", &mut [])?;
    let mut unit: *mut Object = std::ptr::null_mut();
    let found = runtime.api.invoke_value::<bool>(
        manager,
        "TryGetUnit",
        &mut [argument(unit_index), argument(&mut unit)],
    )?;
    if !found || unit.is_null() {
        return Err(OperationError::InvalidArguments(
            "unit_index was not found".into(),
        ));
    }
    Ok((manager, unit))
}

fn unit_status(runtime: &Runtime, mut unit_index: i32) -> Result<UnitReadback, OperationError> {
    let current = require_match(runtime)?;
    let controller = player_controller(runtime, current)?;
    let (_manager, unit) = find_unit(runtime, controller, &mut unit_index)?;
    let id = runtime.api.invoke_value::<i32>(unit, "GetID", &mut [])?;
    let level = runtime.api.invoke_value::<i32>(unit, "GetLevel", &mut [])?;
    let mech_team = runtime.api.invoke(unit, "GetMechTeam", &mut [])?;
    let exp = runtime
        .api
        .invoke_value::<i32>(mech_team, "GetExpInt", &mut [])?;
    let element = runtime.api.invoke(unit, "GetMapElement", &mut [])?;
    let position = runtime
        .api
        .invoke_value::<MapVector>(element, "GetPosition", &mut [])?;
    let rotated = runtime
        .api
        .invoke_value::<bool>(element, "IsRotate", &mut [])?;
    let super_deployment =
        find_match_module(runtime, current, "GameRiver", "SuperDeploymentSystem")?;
    let travelling = runtime.api.invoke_value::<bool>(
        super_deployment,
        "IsTravellingUnit",
        &mut [object_argument(unit)],
    )?;
    Ok(UnitReadback {
        id,
        level,
        exp,
        position,
        rotated,
        travelling,
    })
}

fn set_unit_exp(
    runtime: &Runtime,
    mut unit_index: i32,
    mut exp: i32,
) -> Result<(), OperationError> {
    let current = require_match(runtime)?;
    let controller = player_controller(runtime, current)?;
    let (_manager, unit) = find_unit(runtime, controller, &mut unit_index)?;
    let mech_team = runtime.api.invoke(unit, "GetMechTeam", &mut [])?;
    runtime
        .api
        .invoke_void(mech_team, "SetExpInt", &mut [argument(&mut exp)])?;
    Ok(())
}

fn find_match_module(
    runtime: &Runtime,
    current: *mut Object,
    namespace: &str,
    name: &str,
) -> Result<*mut Object, OperationError> {
    let modules = runtime.api.invoke(current, "GetModules", &mut [])?;
    let mut found: *mut Object = std::ptr::null_mut();
    for index in 0..list_count(runtime.api, modules)? {
        let module = list_item(runtime.api, modules, index)?;
        let class = runtime.api.object_class(module).ok_or_else(|| {
            OperationError::InvalidState("match module has no runtime class".into())
        })?;
        if runtime.api.class_namespace(class) != namespace || runtime.api.class_name(class) != name
        {
            continue;
        }
        if !found.is_null() {
            return Err(OperationError::InvalidState(format!(
                "duplicate match module {namespace}.{name}"
            )));
        }
        found = module;
    }
    if found.is_null() {
        Err(OperationError::InvalidState(format!(
            "match module {namespace}.{name} is absent"
        )))
    } else {
        Ok(found)
    }
}

fn clear_current_side(
    runtime: &Runtime,
    current: *mut Object,
    desired_constructions: &[Placement],
    rotate_to_world: bool,
) -> Result<(), OperationError> {
    let controller = player_controller(runtime, current)?;
    for class in [
        "MAD_ClearUnit",
        "MAD_ClearContraptionEffect",
        "MAD_ClearEquipment",
        "MAD_ClearCommanderSkillEffect",
        "MAD_ClearCommanderSkill",
        "MAD_ClearTechnology",
    ] {
        let action = new_player_test_action(runtime.api, class, controller)
            .map_err(|error| error.context(&format!("build {class}")))?;
        perform_test(runtime.api, current, action)
            .map_err(|error| error.context(&format!("perform {class}")))?;
    }
    let officer = new_player_test_action(runtime.api, "MAD_ClearOfficer", controller)
        .map_err(|error| error.context("build MAD_ClearOfficer"))?;
    let mut all = true;
    runtime
        .api
        .invoke_void(officer, "set_IsAllClear", &mut [argument(&mut all)])
        .map_err(|error| OperationError::from(error).context("configure MAD_ClearOfficer"))?;
    perform_test(runtime.api, current, officer)
        .map_err(|error| error.context("perform MAD_ClearOfficer"))?;
    reconcile_opening_constructions(runtime, controller, desired_constructions, rotate_to_world)
}

fn reconcile_opening_constructions(
    runtime: &Runtime,
    controller: *mut Object,
    desired: &[Placement],
    rotate_to_world: bool,
) -> Result<(), OperationError> {
    let manager = runtime
        .api
        .invoke(controller, "GetConstructionManager", &mut [])?;
    if manager.is_null() {
        return Err(OperationError::InvalidState(
            "ConstructionManager is unavailable while reconciling opening state".into(),
        ));
    }
    let elements = runtime
        .api
        .invoke(manager, "GetConstructionElements", &mut [])?;
    let mut existing = Vec::new();
    for list_index in 0..list_count(runtime.api, elements)? {
        let element = list_item(runtime.api, elements, list_index)?;
        let index = runtime.api.invoke_value::<i32>(
            manager,
            "GetConstructionIndex",
            &mut [object_argument(element)],
        )?;
        if index < 0 || existing.iter().any(|(seen, _, _)| *seen == index) {
            return Err(OperationError::Rejected(format!(
                "opening construction index {index} is invalid or duplicated"
            )));
        }
        let data = runtime
            .api
            .invoke(element, "GetConstructionData", &mut [])?;
        let id = runtime.api.invoke_value::<i32>(data, "GetID", &mut [])?;
        let position = runtime
            .api
            .invoke_value::<MapVector>(element, "GetPosition", &mut [])?;
        existing.push((index, id, position));
    }
    existing.sort_by_key(|(index, _, _)| *index);

    let desired = desired
        .iter()
        .map(|placement| {
            let NativeFormation::Construction(id) = placement.native else {
                return Err(OperationError::InvalidState(
                    "expected construction placement".into(),
                ));
            };
            Ok((id, layout_world_position(placement, rotate_to_world)?))
        })
        .collect::<Result<Vec<_>, OperationError>>()?;
    let mut retained = 0_i32;
    for (index, id, position) in existing {
        if desired.contains(&(id, position)) {
            retained += 1;
        } else {
            remove_construction(runtime, index).map_err(|error| {
                error.context(&format!(
                    "remove unmatched opening construction index {index}"
                ))
            })?;
        }
    }

    let remaining = runtime
        .api
        .invoke(manager, "GetConstructionElements", &mut [])?;
    let remaining_count = runtime
        .api
        .invoke_value::<i32>(remaining, "get_Count", &mut [])?;
    if remaining_count != retained {
        return Err(OperationError::Rejected(format!(
            "opening construction reconciliation retained {remaining_count}, expected {retained}"
        )));
    }
    Ok(())
}

fn require_training_deploying(runtime: &Runtime) -> Result<*mut Object, OperationError> {
    let current = require_match(runtime)?;
    let test = runtime
        .api
        .invoke_value::<bool>(current, "IsTestMatch", &mut [])?;
    let fight = runtime.current_fight();
    if fight.is_null() {
        return Err(OperationError::InvalidState(
            "fight controller is unavailable".into(),
        ));
    }
    let deploying = runtime
        .api
        .invoke_value::<bool>(fight, "IsDeploying", &mut [])?;
    let fighting = runtime
        .api
        .invoke_value::<bool>(fight, "IsFighting", &mut [])?;
    if test && deploying && !fighting {
        Ok(current)
    } else {
        Err(OperationError::InvalidState(
            "operation requires training-ground deployment".into(),
        ))
    }
}

fn strengthen_tower(
    runtime: &Runtime,
    mut manager_index: i32,
    target_level: i32,
) -> Result<i32, OperationError> {
    if !(0..=MAX_TOWER_STRENGTHEN_LEVEL).contains(&target_level) {
        return Err(OperationError::InvalidArguments(format!(
            "target_level must be 0..={MAX_TOWER_STRENGTHEN_LEVEL}"
        )));
    }
    let current = require_training_deploying(runtime)?;
    let controller = player_controller(runtime, current)?;
    let manager = runtime
        .api
        .invoke(controller, "GetBuildingManager", &mut [])?;
    let tower = runtime.api.invoke(
        manager,
        "GetBuildingByIndex",
        &mut [argument(&mut manager_index)],
    )?;
    if tower.is_null() {
        return Err(OperationError::InvalidArguments(
            "manager_index did not resolve a tower".into(),
        ));
    }
    let mut current_level = tower_strengthen_level(runtime.api, tower)?;
    if current_level > target_level {
        return Err(OperationError::Rejected(format!(
            "tower is already at level {current_level}, above target {target_level}"
        )));
    }
    while current_level < target_level {
        let action = core_action(runtime.api, "PAD_StrengthenTower")?;
        runtime
            .api
            .invoke_void(action, "set_Index", &mut [argument(&mut manager_index)])?;
        check_action(runtime.api, controller, action)?;
        perform_sync(runtime.api, controller, action)?;
        let after = tower_strengthen_level(runtime.api, tower)?;
        if after != current_level + 1 {
            return Err(OperationError::Rejected(
                "tower strengthen readback did not advance".into(),
            ));
        }
        current_level = after;
    }
    if current_level != target_level {
        return Err(OperationError::Rejected(
            "tower strengthen readback did not match target".into(),
        ));
    }
    Ok(current_level)
}

fn tower_strengthen_level(api: Api, tower: *mut Object) -> Result<i32, OperationError> {
    let data = api.invoke(tower, "GetTowerStrengthenData", &mut [])?;
    if data.is_null() {
        Ok(0)
    } else {
        Ok(api.invoke_value::<i32>(data, "GetLevel", &mut [])?)
    }
}

fn add_test_inventory(
    runtime: &Runtime,
    mut id: i32,
    class_name: &str,
) -> Result<(), OperationError> {
    let current = require_training_deploying(runtime)?;
    let controller = player_controller(runtime, current)?;
    let action = new_player_test_action(runtime.api, class_name, controller)?;
    runtime
        .api
        .invoke_void(action, "set_ID", &mut [argument(&mut id)])?;
    perform_test(runtime.api, current, action)
}

fn activate_energy_tower_skill(runtime: &Runtime, mut id: i32) -> Result<(), OperationError> {
    let current = require_training_deploying(runtime)?;
    let controller = player_controller(runtime, current)?;
    let manager = runtime
        .api
        .invoke(controller, "GetEnergyTowerManager", &mut [])?;
    let skill = runtime
        .api
        .invoke(manager, "GetSkill", &mut [argument(&mut id)])?;
    if skill.is_null() {
        return Err(OperationError::InvalidArguments(
            "skill_id was not found".into(),
        ));
    }
    let before = runtime
        .api
        .invoke_value::<bool>(skill, "IsActive", &mut [])?;
    if before {
        return Err(OperationError::Rejected(
            "energy tower skill is already active".into(),
        ));
    }
    let action = core_action(runtime.api, "PAD_ActiveEnergyTowerSkill")?;
    runtime
        .api
        .invoke_void(action, "set_SkillID", &mut [argument(&mut id)])?;
    check_action(runtime.api, controller, action)?;
    perform_sync(runtime.api, controller, action)?;
    let after = runtime
        .api
        .invoke_value::<bool>(skill, "IsActive", &mut [])?;
    if !after {
        return Err(OperationError::Rejected(
            "energy tower skill did not become active".into(),
        ));
    }
    Ok(())
}

fn equip_unit(
    runtime: &Runtime,
    mut equipment_id: i32,
    mut unit_index: i32,
) -> Result<(), OperationError> {
    let current = require_training_deploying(runtime)?;
    let controller = player_controller(runtime, current)?;
    let (_unit_manager, unit) = find_unit(runtime, controller, &mut unit_index)?;
    let manager = runtime
        .api
        .invoke(controller, "GetEquipmentManager", &mut [])?;
    let equipment = runtime.api.invoke(
        manager,
        "GetEquipment",
        &mut [argument(&mut equipment_id), std::ptr::null_mut()],
    )?;
    if equipment.is_null() {
        return Err(OperationError::InvalidArguments(
            "unused equipment was not found".into(),
        ));
    }
    let can_use = runtime.api.invoke_value::<i32>(
        manager,
        "CanUseEquipment",
        &mut [object_argument(equipment), object_argument(unit)],
    )?;
    if can_use != 0 {
        return Err(OperationError::Rejected(format!(
            "CanUseEquipment returned {can_use}"
        )));
    }
    let action = core_action(runtime.api, "PAD_UseEquipment")?;
    runtime.api.invoke_void(
        action,
        "set_EquipmentID",
        &mut [argument(&mut equipment_id)],
    )?;
    runtime
        .api
        .invoke_void(action, "set_UnitIndex", &mut [argument(&mut unit_index)])?;
    check_action(runtime.api, controller, action)?;
    perform_sync(runtime.api, controller, action)?;
    let has = runtime
        .api
        .invoke_value::<bool>(unit, "HasEquipment", &mut [])?;
    let after = runtime.api.invoke(unit, "GetEquipment", &mut [])?;
    if !has || after != equipment {
        return Err(OperationError::Rejected(
            "equipment ownership readback did not match".into(),
        ));
    }
    Ok(())
}

fn contraption(
    runtime: &Runtime,
    mut id: i32,
    mut position: MapVector,
    extra: Option<MapVector>,
    mut wanted_index: i32,
) -> Result<i32, OperationError> {
    let has_extra = extra.is_some();
    let mut extra = extra.unwrap_or_default();
    let current = require_training_deploying(runtime)?;
    let controller = player_controller(runtime, current)?;
    let manager = runtime
        .api
        .invoke(controller, "GetContraptionManager", &mut [])?;
    let item = runtime
        .api
        .invoke(manager, "GetContraption", &mut [argument(&mut id)])?;
    if item.is_null() {
        return Err(OperationError::InvalidArguments(
            "contraption_id was not found".into(),
        ));
    }
    let baseline = contraption_release_baseline(runtime, controller, manager, item, id, position)?;
    let target_type = runtime
        .api
        .invoke_value::<i32>(item, "GetSkillTargetType", &mut [])?;
    if (target_type == 1) != has_extra {
        return Err(OperationError::InvalidArguments(
            "extra_position presence does not match target type".into(),
        ));
    }
    let mut force = false;
    let ready = runtime.api.invoke_value::<i32>(
        manager,
        "IsReadyToRelease",
        &mut [object_argument(item), argument(&mut force)],
    )?;
    let primary = runtime.api.invoke_value::<i32>(
        manager,
        "CanRelease",
        &mut [
            object_argument(item),
            argument(&mut position),
            argument(&mut force),
        ],
    )?;
    if ready != 0 || primary != 0 {
        return Err(OperationError::Rejected(format!(
            "contraption readiness={ready}, placement={primary}"
        )));
    }
    if has_extra {
        let extra_result = runtime.api.invoke_value::<i32>(
            manager,
            "CanRelease",
            &mut [
                object_argument(item),
                argument(&mut extra),
                argument(&mut force),
            ],
        )?;
        if extra_result != 0 {
            return Err(OperationError::Rejected(format!(
                "extra placement returned {extra_result}"
            )));
        }
    }
    // A contraption release carries no index of its own: the record takes
    // whatever the recorder's allocator holds. Point the allocator at the
    // identity the layout declares, which is also how a gap left by a sold or
    // destroyed object is reproduced.
    runtime.api.invoke_void(
        baseline.recorder,
        "set_NextObjectIndex",
        &mut [argument(&mut wanted_index)],
    )?;
    let action = core_action(runtime.api, "PAD_ReleaseContraption")?;
    runtime
        .api
        .invoke_void(action, "set_ContraptionID", &mut [argument(&mut id)])?;
    runtime
        .api
        .invoke_void(action, "set_Position", &mut [argument(&mut position)])?;
    runtime
        .api
        .invoke_void(action, "set_ExtraPosition", &mut [argument(&mut extra)])?;
    check_action(runtime.api, controller, action)?;
    perform_sync(runtime.api, controller, action)?;
    verify_contraption_readback(runtime, &baseline)
}

fn contraption_release_baseline(
    runtime: &Runtime,
    controller: *mut Object,
    manager: *mut Object,
    item: *mut Object,
    id: i32,
    position: MapVector,
) -> Result<ContraptionReleaseBaseline, OperationError> {
    let resolved_id = runtime.api.invoke_value::<i32>(item, "get_ID", &mut [])?;
    if resolved_id != id {
        return Err(OperationError::Rejected(
            "contraption catalog returned a different ID".into(),
        ));
    }
    let player = runtime.api.invoke(controller, "GetPlayer", &mut [])?;
    let expected_cost = runtime
        .api
        .invoke_value::<i32>(item, "GetSupply", &mut [])?;
    let supply = runtime
        .api
        .invoke_value::<i32>(player, "GetSupply", &mut [])?;
    let remain_count = runtime
        .api
        .invoke_value::<i32>(manager, "get_RemainCount", &mut [])?;
    let recorder = runtime
        .api
        .invoke(manager, "GetFightObjectRecorder", &mut [])?;
    let records = runtime.api.invoke(recorder, "GeRecords", &mut [])?;
    let record_count = list_count(runtime.api, records)?;
    Ok(ContraptionReleaseBaseline {
        player,
        manager,
        item,
        recorder,
        id,
        expected_cost,
        supply,
        remain_count,
        record_count,
        position,
    })
}

fn verify_contraption_readback(
    runtime: &Runtime,
    baseline: &ContraptionReleaseBaseline,
) -> Result<i32, OperationError> {
    let after_supply = runtime
        .api
        .invoke_value::<i32>(baseline.player, "GetSupply", &mut [])?;
    let after_remain =
        runtime
            .api
            .invoke_value::<i32>(baseline.manager, "get_RemainCount", &mut [])?;
    let records = runtime
        .api
        .invoke(baseline.recorder, "GeRecords", &mut [])?;
    let after_record_count = list_count(runtime.api, records)?;
    let record = runtime
        .api
        .invoke(baseline.recorder, "GetLastRecord", &mut [])?;
    if record.is_null() {
        return Err(OperationError::Rejected(
            "contraption recorder returned no release record".into(),
        ));
    }
    let source = runtime.api.invoke(record, "get_RecordSource", &mut [])?;
    if source.is_null() {
        return Err(OperationError::Rejected(
            "contraption release record has no source".into(),
        ));
    }
    let source_id = runtime.api.invoke_value::<i32>(source, "get_ID", &mut [])?;
    let positions = runtime.api.invoke(record, "get_Positions", &mut [])?;
    let position_count = list_count(runtime.api, positions)?;
    if position_count == 0 {
        return Err(OperationError::Rejected(
            "contraption release record has no position".into(),
        ));
    }
    let mut first = 0;
    let recorded_position = runtime.api.invoke_value::<MapVector>(
        positions,
        "get_Item",
        &mut [argument(&mut first)],
    )?;
    let index = runtime
        .api
        .invoke_value::<i32>(record, "get_Index", &mut [])?;

    let matches = baseline.expected_cost >= 0
        && baseline.remain_count > 0
        && after_supply == baseline.supply - baseline.expected_cost
        && after_remain == baseline.remain_count - 1
        && after_record_count == baseline.record_count + 1
        && source == baseline.item
        && source_id == baseline.id
        && position_count > 0
        && index >= 0
        && recorded_position.x == baseline.position.x
        && recorded_position.y == baseline.position.y;
    if matches {
        Ok(index)
    } else {
        Err(OperationError::Rejected(
            "contraption release readback did not match".into(),
        ))
    }
}

fn declared_index(placement: &Placement, kind: &str) -> Result<i32, OperationError> {
    placement.index.ok_or_else(|| {
        OperationError::InvalidArguments(format!(
            "{kind} {} has no declared deployment index",
            describe_placement(placement)
        ))
    })
}

fn construction(
    runtime: &Runtime,
    mut id: i32,
    mut position: MapVector,
    mut wanted_index: i32,
) -> Result<i32, OperationError> {
    let current = require_training_deploying(runtime)?;
    let controller = player_controller(runtime, current)?;
    let manager = runtime
        .api
        .invoke(controller, "GetConstructionManager", &mut [])?;
    let before_elements = runtime
        .api
        .invoke(manager, "GetConstructionElements", &mut [])?;
    let before_count = runtime
        .api
        .invoke_value::<i32>(before_elements, "get_Count", &mut [])?;

    let config_class = runtime.api.class("GRCore.dll", "GameRiver", "Config")?;
    let singleton = runtime
        .api
        .class_parent(config_class)
        .ok_or_else(|| OperationError::InvalidState("Config singleton base is missing".into()))?;
    let config = runtime
        .api
        .invoke_static(singleton, "get_Instance", &mut [])?;
    let data = runtime
        .api
        .invoke(config, "GetConstructionData", &mut [argument(&mut id)])?;
    if data.is_null() {
        return Err(OperationError::InvalidArguments(
            "construction_id was not found".into(),
        ));
    }
    let element_class = runtime
        .api
        .class("GRCore.dll", "GameRiver", "ConstructionElement")?;
    let element = runtime.api.new_object(element_class)?;
    let constructor = runtime.api.method_with_parameter_types(
        element,
        ".ctor",
        &["GameRiver.ConstructionData"],
    )?;
    runtime
        .api
        .invoke_raw(constructor, element.cast(), &mut [object_argument(data)])?;
    let release_controller = runtime.api.class(
        "GRClient.dll",
        "GameRiver.Client",
        "ReleaseConstructionController",
    )?;
    let mut testing = true;
    let builder = runtime.api.invoke_static(
        release_controller,
        "Create",
        &mut [object_argument(element), argument(&mut testing)],
    )?;
    if builder.is_null() {
        return Err(OperationError::Rejected(
            "construction release controller was not created".into(),
        ));
    }
    let action =
        runtime
            .api
            .invoke(builder, "CreateReleaseInfo", &mut [argument(&mut position)])?;
    if action.is_null() {
        return Err(OperationError::Rejected(
            "construction action was not created".into(),
        ));
    }
    let echoed_id = runtime
        .api
        .invoke_value::<i32>(action, "get_ConstructionID", &mut [])?;
    let echoed_position = runtime
        .api
        .invoke_value::<MapVector>(action, "get_Position", &mut [])?;
    if echoed_id != id || echoed_position.x != position.x || echoed_position.y != position.y {
        return Err(OperationError::Rejected(
            "construction action did not preserve its type and position".into(),
        ));
    }
    // The release controller fills IDX from the manager's allocator. A layout
    // carries the identity the object had in the recorded match instead, and
    // ConstructionManager keys its elements by exactly this value.
    runtime
        .api
        .invoke_void(action, "set_IDX", &mut [argument(&mut wanted_index)])?;
    check_action(runtime.api, controller, action)?;
    let construction_index = runtime
        .api
        .invoke_value::<i32>(action, "get_IDX", &mut [])?;
    if construction_index != wanted_index {
        return Err(OperationError::Rejected(format!(
            "construction action kept index {construction_index} after requesting {wanted_index}"
        )));
    }
    perform_sync(runtime.api, controller, action)?;
    verify_construction_readback(
        runtime,
        manager,
        id,
        construction_index,
        position,
        before_count,
    )?;
    Ok(construction_index)
}

fn verify_construction_readback(
    runtime: &Runtime,
    manager: *mut Object,
    construction_id: i32,
    mut construction_index: i32,
    position: MapVector,
    before_count: i32,
) -> Result<(), OperationError> {
    if construction_index < 0 {
        return Err(OperationError::Rejected(
            "construction action returned an invalid index".into(),
        ));
    }
    let mut element: *mut Object = std::ptr::null_mut();
    let found = runtime.api.invoke_value::<bool>(
        manager,
        "TryGetConstructionElement",
        &mut [argument(&mut construction_index), argument(&mut element)],
    )?;
    if !found || element.is_null() {
        return Err(OperationError::Rejected(
            "released construction was not found".into(),
        ));
    }
    let data = runtime
        .api
        .invoke(element, "GetConstructionData", &mut [])?;
    let id = runtime.api.invoke_value::<i32>(data, "GetID", &mut [])?;
    let index = runtime.api.invoke_value::<i32>(
        manager,
        "GetConstructionIndex",
        &mut [object_argument(element)],
    )?;
    let actual_position = runtime
        .api
        .invoke_value::<MapVector>(element, "GetPosition", &mut [])?;
    let elements = runtime
        .api
        .invoke(manager, "GetConstructionElements", &mut [])?;
    let count = runtime
        .api
        .invoke_value::<i32>(elements, "get_Count", &mut [])?;
    if id == construction_id
        && index == construction_index
        && actual_position.x == position.x
        && actual_position.y == position.y
        && count == before_count + 1
    {
        Ok(())
    } else {
        Err(OperationError::Rejected(
            "construction readback did not match".into(),
        ))
    }
}

fn remove_construction(
    runtime: &Runtime,
    mut construction_index: i32,
) -> Result<(), OperationError> {
    let current = require_match(runtime)?;
    let controller = player_controller(runtime, current)?;
    let manager = runtime
        .api
        .invoke(controller, "GetConstructionManager", &mut [])?;
    let before_elements = runtime
        .api
        .invoke(manager, "GetConstructionElements", &mut [])?;
    let before_count = runtime
        .api
        .invoke_value::<i32>(before_elements, "get_Count", &mut [])?;
    let action = new_player_test_action(runtime.api, "MAD_RemoveConstruction", controller)?;
    runtime
        .api
        .invoke_void(action, "set_IDX", &mut [argument(&mut construction_index)])?;
    perform_test(runtime.api, current, action)?;

    let mut removed: *mut Object = std::ptr::null_mut();
    let found = runtime.api.invoke_value::<bool>(
        manager,
        "TryGetConstructionElement",
        &mut [argument(&mut construction_index), argument(&mut removed)],
    )?;
    let after_elements = runtime
        .api
        .invoke(manager, "GetConstructionElements", &mut [])?;
    let after_count = runtime
        .api
        .invoke_value::<i32>(after_elements, "get_Count", &mut [])?;
    if found || !removed.is_null() || after_count != before_count - 1 {
        return Err(OperationError::Rejected(format!(
            "removed construction index {construction_index} remains deployed"
        )));
    }
    Ok(())
}

fn release_battle_skill(
    runtime: &Runtime,
    mut id: i32,
    positions: &mut [MapVector],
) -> Result<(), OperationError> {
    if positions.is_empty() || positions.len() > 64 {
        return Err(OperationError::InvalidArguments(
            "positions must contain 1..=64 entries".into(),
        ));
    }
    let current = require_training_deploying(runtime)?;
    let controller = player_controller(runtime, current)?;
    let manager = runtime
        .api
        .invoke(controller, "GetCommanderSkillManager", &mut [])?;
    let skill = runtime
        .api
        .invoke(manager, "GetCommanderSkillByID", &mut [argument(&mut id)])?;
    if skill.is_null() {
        return Err(OperationError::InvalidArguments(
            "commander_skill_id was not found".into(),
        ));
    }
    let mut manager_index = runtime.api.invoke_value::<i32>(
        manager,
        "GetCommanderSkillIndex",
        &mut [object_argument(skill)],
    )?;
    let expected_count =
        runtime
            .api
            .invoke_value::<i32>(skill, "GetEffectPositionCount", &mut [])?;
    if expected_count != i32::try_from(positions.len()).unwrap_or(-1) {
        return Err(OperationError::InvalidArguments(
            "positions length does not match skill".into(),
        ));
    }
    for position in positions.iter_mut() {
        let result = runtime.api.invoke_value::<i32>(
            manager,
            "CanReleaseCommanderSkill",
            &mut [object_argument(skill), argument(position)],
        )?;
        if result != 0 {
            return Err(OperationError::Rejected(format!(
                "skill position rejected with {result}"
            )));
        }
    }

    let action = core_action(runtime.api, "PAD_ReleaseCommanderSkill")?;
    let action_positions = runtime.api.invoke(action, "get_Positions", &mut [])?;
    for position in positions {
        runtime
            .api
            .invoke_void(action_positions, "Add", &mut [argument(position)])?;
    }
    let mut unit_index = -1;
    let mut construction_index = -1;
    runtime
        .api
        .invoke_void(action, "set_ID", &mut [argument(&mut id)])?;
    runtime.api.invoke_void(
        action,
        "set_SkillIndex",
        &mut [argument(&mut manager_index)],
    )?;
    runtime
        .api
        .invoke_void(action, "set_UnitIndex", &mut [argument(&mut unit_index)])?;
    runtime.api.invoke_void(
        action,
        "set_ConstructionIndex",
        &mut [argument(&mut construction_index)],
    )?;
    check_action(runtime.api, controller, action)?;
    perform_sync(runtime.api, controller, action)
}

fn list_count(api: Api, list: *mut Object) -> Result<i32, OperationError> {
    if list.is_null() {
        return Err(OperationError::InvalidState("managed list is null".into()));
    }
    let count = api.invoke_value::<i32>(list, "get_Count", &mut [])?;
    if (0..=16_384).contains(&count) {
        Ok(count)
    } else {
        Err(OperationError::InvalidState(format!(
            "invalid managed list count {count}"
        )))
    }
}

fn list_item(api: Api, list: *mut Object, index: i32) -> Result<*mut Object, OperationError> {
    let mut index = index;
    Ok(api.invoke(list, "get_Item", &mut [argument(&mut index)])?)
}

fn config_instance(runtime: &Runtime) -> Result<*mut Object, OperationError> {
    let config_class = runtime.api.class("GRCore.dll", "GameRiver", "Config")?;
    let singleton = runtime
        .api
        .class_parent(config_class)
        .ok_or_else(|| OperationError::InvalidState("Config singleton base is missing".into()))?;
    let config = runtime
        .api
        .invoke_static(singleton, "get_Instance", &mut [])?;
    if config.is_null() {
        Err(OperationError::InvalidState(
            "Config singleton is unavailable".into(),
        ))
    } else {
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_menu_scene_requires_positive_name() {
        assert!(is_main_menu_scene("MainMenu"));
        assert!(is_main_menu_scene("Main Scene"));
        assert!(!is_main_menu_scene("Loading"));
        assert!(!is_main_menu_scene(""));
    }

    #[test]
    fn start_test_seed_is_optional_i32_with_native_zero_semantics() {
        assert_eq!(parse_start_test_arguments(&Value::Null).unwrap().seed, None);
        assert_eq!(parse_start_test_arguments(&json!({})).unwrap().seed, None);
        assert_eq!(
            parse_start_test_arguments(&json!({"seed": 0}))
                .unwrap()
                .seed,
            Some(0)
        );
        assert_eq!(
            parse_start_test_arguments(&json!({"seed": 42}))
                .unwrap()
                .seed,
            Some(42)
        );
        assert!(parse_start_test_arguments(&json!({"unknown": 42})).is_err());
        assert_eq!(
            parse_start_test_arguments(&json!({"map_id": 1021}))
                .unwrap()
                .map_id,
            Some(1021)
        );
        for id in [
            json!(0),
            json!(-1),
            json!(1.5),
            json!("1021"),
            json!(2_147_483_648_i64),
        ] {
            assert!(parse_start_test_arguments(&json!({"map_id": id})).is_err());
        }
    }

    #[test]
    fn decodes_build_2227_unit_technology_owners() {
        for (technology_id, unit_id) in [
            (10213, 13),
            (10202, 2),
            (10209, 9),
            (10206, 6),
            (10215, 15),
            (180_110, 10),
        ] {
            assert_eq!(encoded_technology_owner(technology_id).unwrap(), unit_id);
        }
        assert!(encoded_technology_owner(10200).is_err());
        assert!(encoded_technology_owner(10232).is_err());
    }

    #[test]
    fn layout_positions_are_rotated_for_red_only() {
        let plan = layout::compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {"formations": [{
                    "type": "marksman", "index": 0, "position": {"x": 20, "y": -50}
                }]},
                "red": {"formations": [{
                    "type": "marksman", "index": 0, "position": {"x": 20, "y": -50}
                }]}
            }
        }))
        .unwrap();

        let blue = layout_world_position(&plan.blue.formations[0], false).unwrap();
        let red = layout_world_position(&plan.red.formations[0], true).unwrap();
        assert_eq!((blue.x, blue.y), (20, -50));
        assert_eq!((red.x, red.y), (-20, 50));
        assert_eq!(
            describe_placement(&plan.red.formations[0]),
            "formation type \"marksman\" at local position (20, -50)"
        );
    }

    #[test]
    fn battle_skill_positions_use_the_same_side_local_rotation() {
        let plan = layout::compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {
                    "formations": [{"type": "marksman", "index": 0, "position": {"x": 0, "y": -50}}],
                    "battle_skills": [{
                        "type": "missile_strike",
                        "positions": [{"x": 55, "y": 60}]
                    }]
                },
                "red": {
                    "formations": [{"type": "fang", "index": 0, "position": {"x": -55, "y": -60}}],
                    "battle_skills": [{
                        "type": "mobile_beacon",
                        "positions": [
                            {"x": -55, "y": -60},
                            {"x": -105, "y": -90},
                            {"x": -105, "y": 20}
                        ]
                    }]
                }
            }
        }))
        .unwrap();

        let blue = battle_skill_world_positions(&plan.blue.battle_skills[0], false).unwrap();
        let red = battle_skill_world_positions(&plan.red.battle_skills[0], true).unwrap();
        assert_eq!(
            blue.iter()
                .map(|position| (position.x, position.y))
                .collect::<Vec<_>>(),
            [(55, 60)]
        );
        assert_eq!(
            red.iter()
                .map(|position| (position.x, position.y))
                .collect::<Vec<_>>(),
            [(55, 60), (105, 90), (105, -20)]
        );
    }

    #[test]
    fn rejects_unrepresentable_red_world_position() {
        let placement = Placement {
            type_name: "marksman".into(),
            native: NativeFormation::Unit(2),
            footprint: Some((20, 20)),
            position: layout::Position {
                x: i32::MIN,
                y: -50,
            },
            index: Some(0),
            level: Some(1),
            exp: Some(0),
            rotated: false,
            equipment: None,
            travelling: false,
        };
        let Err(error) = layout_world_position(&placement, true) else {
            panic!("red coordinate rotation unexpectedly succeeded")
        };
        assert!(error.to_string().contains("cannot be rotated"));
    }

    #[test]
    fn terrain_grid_rotation_is_a_twelve_bit_involution() {
        let rows = [
            0b0000_0000_0001,
            0b0000_0000_0011,
            0b0000_0000_0111,
            0b0000_0000_1111,
            0b0000_0001_1111,
            0b0000_0011_1111,
            0b0000_0111_1111,
            0b0000_1111_1111,
            0b0001_1111_1111,
            0b0011_1111_1111,
            0b0111_1111_1111,
            0b1111_1111_1111,
        ];
        let rotated = rotate_terrain_grid_rows(&rows);
        assert_eq!(rotate_terrain_grid_rows(&rotated), rows);
        assert_eq!(rotated[0], 0x0fff);
        assert_eq!(rotated[11], 0x0800);
    }
}
