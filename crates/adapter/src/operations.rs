use crate::il2cpp::{Api, Error as Il2CppError, Object, argument, object_argument};
use crate::protocol::{CAPABILITIES, GameStatus, Request, Response};
use crate::runtime::Runtime;
use serde_json::{Value, json};
use std::path::Path;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct MapVector {
    x: i32,
    y: i32,
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
const RANGE_ENHANCEMENT_SKILL: i32 = 5;
const MOVEMENT_ENHANCEMENT_SKILL: i32 = 6;
const TRAINING_GROUND_SUPPLY: i32 = 10_000;

pub fn execute(runtime: &mut Runtime, request: &Request) -> Response<Value> {
    if !CAPABILITIES.contains(&request.operation.as_str()) {
        return Response::failure(
            request.id,
            "invalid_arguments",
            format!("unknown operation {}", request.operation),
        );
    }
    match execute_inner(runtime, request) {
        Ok(result) => Response::success(request.id, result),
        Err(OperationError::InvalidArguments(message)) => {
            Response::failure(request.id, "invalid_arguments", message)
        }
        Err(OperationError::InvalidState(message)) => {
            Response::failure(request.id, "invalid_game_state", message)
        }
        Err(OperationError::Rejected(message)) => {
            Response::failure(request.id, "game_rejected_operation", message)
        }
        Err(OperationError::Il2Cpp(error)) => {
            Response::failure(request.id, "il2cpp_error", error.to_string())
        }
        Err(OperationError::Il2CppContext(message)) => {
            Response::failure(request.id, "il2cpp_error", message)
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
    match request.operation.as_str() {
        "status" => status(runtime),
        "start_test" => start_test(runtime),
        "toggle_fight" => invoke_match_void(runtime, "ChangeProcessState"),
        "speed_up" => speed_up(runtime),
        "quit_match" => quit_match(runtime),
        "quit_game" => quit_game(runtime),
        _ => Err(OperationError::InvalidArguments(format!(
            "unknown operation {}",
            request.operation
        ))),
    }
}

fn status(runtime: &Runtime) -> Result<Value, OperationError> {
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
        GameStatus::Replay
    } else if api
        .invoke_value::<bool>(current_match, "IsTestMatch", &mut [])
        .ok()
        == Some(true)
    {
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
        return Ok(json!({
            "status": GameStatus::TrainingGround,
            "round_count": round_count,
            "deploying": deploying,
            "fighting": fighting
        }));
    } else {
        GameStatus::Unknown
    };

    Ok(json!({"status": status}))
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

fn start_test(runtime: &Runtime) -> Result<Value, OperationError> {
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
    let mut seed = 0_i32;
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
        "initial_supply": TRAINING_GROUND_SUPPLY
    }))
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

fn finish_preparation(runtime: &Runtime, arguments: &Value) -> Result<Value, OperationError> {
    let expected_round = required_i32(arguments, "expected_round")?;
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

fn choose_reinforcement(runtime: &Runtime, arguments: &Value) -> Result<Value, OperationError> {
    let mut index = required_i32(arguments, "index")?;
    let current = require_match(runtime)?;
    let controller = player_controller(runtime, current)?;
    let manager = runtime
        .api
        .invoke(controller, "GetReinforcementManager", &mut [])?;
    let items = runtime.api.invoke(manager, "GetReinforceItems", &mut [])?;
    let mut item_index = index;
    let item = runtime
        .api
        .invoke(items, "get_Item", &mut [argument(&mut item_index)])?;
    let mut id = runtime.api.invoke_value::<i32>(item, "GetID", &mut [])?;
    let action = core_action(runtime.api, "PAD_ChooseReinforceItem")?;
    runtime
        .api
        .invoke_void(action, "set_ID", &mut [argument(&mut id)])?;
    runtime
        .api
        .invoke_void(action, "set_Index", &mut [argument(&mut index)])?;
    perform_sync(runtime.api, controller, action)?;
    Ok(json!({"selected": true, "index": index, "id": id}))
}

fn choose_opening(runtime: &Runtime, arguments: &Value) -> Result<Value, OperationError> {
    let mut index = required_i32(arguments, "index")?;
    let current = require_match(runtime)?;
    let controller = player_controller(runtime, current)?;
    let opening_controller = runtime
        .api
        .invoke(current, "GetBattleOpeningController", &mut [])?;
    let opening = runtime.api.invoke(
        opening_controller,
        "GetOpeningData",
        &mut [object_argument(controller), argument(&mut index)],
    )?;
    let team = runtime.api.invoke(opening, "GetAdvanceTeam", &mut [])?;
    let mut id = runtime.api.invoke_value::<i32>(team, "get_ID", &mut [])?;
    let action = core_action(runtime.api, "PAD_ChooseAdvanceTeam")?;
    runtime
        .api
        .invoke_void(action, "set_ID", &mut [argument(&mut id)])?;
    runtime
        .api
        .invoke_void(action, "set_Index", &mut [argument(&mut index)])?;
    perform_sync(runtime.api, controller, action)?;
    Ok(json!({"selected": true, "index": index, "id": id}))
}

fn unlock_unit(runtime: &Runtime, arguments: &Value) -> Result<Value, OperationError> {
    let mut unit_id = positive_i32(arguments, "unit_id")?;
    let current = require_match(runtime)?;
    let controller = player_controller(runtime, current)?;
    let shop = runtime.api.invoke(controller, "GetShopManager", &mut [])?;
    let check =
        runtime
            .api
            .invoke_value::<i32>(shop, "CanUnlockUnit", &mut [argument(&mut unit_id)])?;
    if check != 0 {
        return Err(OperationError::Rejected(format!(
            "CanUnlockUnit returned {check}"
        )));
    }
    let action = core_action(runtime.api, "PAD_UnlockUnit")?;
    runtime
        .api
        .invoke_void(action, "set_UID", &mut [argument(&mut unit_id)])?;
    perform_sync(runtime.api, controller, action)?;
    Ok(json!({"unlocked": true, "unit_id": unit_id}))
}

fn move_unit(runtime: &Runtime, arguments: &Value, mutate: bool) -> Result<Value, OperationError> {
    let mut unit_index = required_i32(arguments, "unit_index")?;
    let mut position = map_vector(arguments, "position")?;
    let mut rotate = required_bool(arguments, "rotate")?;
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
    let check_result = check_action(runtime.api, controller, action)?;
    if mutate {
        perform_sync(runtime.api, controller, action)?;
    }
    Ok(json!({
        "legal": true,
        "performed": mutate,
        "check_result": check_result,
        "unit_index": unit_index
    }))
}

fn create_unit(
    runtime: &Runtime,
    current: *mut Object,
    unit_id: i32,
    displayed_level: i32,
) -> Result<i32, OperationError> {
    if !(1..=9).contains(&displayed_level) {
        return Err(OperationError::InvalidArguments(
            "displayed_level must be 1..=9".into(),
        ));
    }
    let api = runtime.api;
    let controller = player_controller(runtime, current)?;
    let player = api.invoke(controller, "GetPlayer", &mut [])?;
    let manager = api.invoke(controller, "GetUnitManager", &mut [])?;
    let territory = api.invoke(controller, "GetTerritoryManager", &mut [])?;
    let region = api.invoke(territory, "GetFocusRegion", &mut [])?;
    let units = api.invoke(manager, "GetUnits", &mut [])?;
    let mut player_index = api.invoke_value::<i32>(player, "GetRoomIndex", &mut [])?;
    let mut region_id = api.invoke_value::<i32>(region, "get_ID", &mut [])?;
    let count = api.invoke_value::<i32>(units, "get_Count", &mut [])?;
    if !(0..=1024).contains(&count) {
        return Err(OperationError::InvalidState(
            "invalid deployed unit count".into(),
        ));
    }
    let mut unit_index = 0;
    for item_index in 0..count {
        let mut item_index = item_index;
        let unit = api.invoke(units, "get_Item", &mut [argument(&mut item_index)])?;
        let existing =
            api.invoke_value::<i32>(manager, "GetUnitIndex", &mut [object_argument(unit)])?;
        unit_index = unit_index.max(existing + 1);
    }
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

fn add_unit(runtime: &Runtime, arguments: &Value) -> Result<Value, OperationError> {
    let unit_id = positive_i32(arguments, "unit_id")?;
    let level = optional_i32(arguments, "displayed_level").unwrap_or(1);
    let position = map_vector(arguments, "position")?;
    let rotate = required_bool(arguments, "rotate")?;
    let current = require_match(runtime)?;
    let index = create_unit(runtime, current, unit_id, level)?;
    let move_arguments = json!({
        "unit_index": index,
        "position": {"x": position.x, "y": position.y},
        "rotate": rotate
    });
    match move_unit(runtime, &move_arguments, true) {
        Ok(mut result) => {
            result["added"] = Value::Bool(true);
            Ok(result)
        }
        Err(error) => Err(error),
    }
}

fn change_technology(
    runtime: &Runtime,
    arguments: &Value,
    class_name: &str,
) -> Result<Value, OperationError> {
    let mut unit_id = positive_i32(arguments, "unit_id")?;
    let mut technology_id = positive_i32(arguments, "technology_id")?;
    let current = require_match(runtime)?;
    let controller = player_controller(runtime, current)?;
    let action = new_player_test_action(runtime.api, class_name, controller)?;
    runtime
        .api
        .invoke_void(action, "set_UnitID", &mut [argument(&mut unit_id)])?;
    runtime
        .api
        .invoke_void(action, "set_TechID", &mut [argument(&mut technology_id)])?;
    perform_test(runtime.api, current, action)?;
    Ok(json!({"changed": true, "unit_id": unit_id, "technology_id": technology_id}))
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

fn unit_status(runtime: &Runtime, arguments: &Value) -> Result<Value, OperationError> {
    let mut unit_index = required_i32(arguments, "unit_index")?;
    let current = require_match(runtime)?;
    let controller = player_controller(runtime, current)?;
    let (_manager, unit) = find_unit(runtime, controller, &mut unit_index)?;
    let id = runtime.api.invoke_value::<i32>(unit, "GetID", &mut [])?;
    let level = runtime.api.invoke_value::<i32>(unit, "GetLevel", &mut [])?;
    let round_count = runtime
        .api
        .invoke_value::<i32>(unit, "GetRoundCount", &mut [])?;
    let old = runtime
        .api
        .invoke_value::<bool>(unit, "IsOldUnit", &mut [])?;
    let element = runtime.api.invoke(unit, "GetMapElement", &mut [])?;
    let position = runtime
        .api
        .invoke_value::<MapVector>(element, "GetPosition", &mut [])?;
    let rotated = runtime
        .api
        .invoke_value::<bool>(element, "IsRotate", &mut [])?;
    Ok(json!({
        "unit_index": unit_index,
        "unit_id": id,
        "level": level,
        "round_count": round_count,
        "is_old_unit": old,
        "position": {"x": position.x, "y": position.y},
        "rotated": rotated
    }))
}

fn remove_unit(runtime: &Runtime, arguments: &Value) -> Result<Value, OperationError> {
    let mut unit_index = required_i32(arguments, "unit_index")?;
    let current = require_match(runtime)?;
    let controller = player_controller(runtime, current)?;
    let (manager, unit) = find_unit(runtime, controller, &mut unit_index)?;
    let mut id = runtime.api.invoke_value::<i32>(unit, "GetID", &mut [])?;
    let action = new_player_test_action(runtime.api, "MAD_RemoveUnit", controller)?;
    runtime
        .api
        .invoke_void(action, "set_IDX", &mut [argument(&mut unit_index)])?;
    runtime
        .api
        .invoke_void(action, "set_ID", &mut [argument(&mut id)])?;
    perform_test(runtime.api, current, action)?;
    let mut after: *mut Object = std::ptr::null_mut();
    let found_after = runtime.api.invoke_value::<bool>(
        manager,
        "TryGetUnit",
        &mut [argument(&mut unit_index), argument(&mut after)],
    )?;
    if found_after {
        return Err(OperationError::Rejected(
            "unit remained after remove action".into(),
        ));
    }
    Ok(json!({"removed": true, "unit_index": unit_index, "unit_id": id}))
}

fn clear_both_sides(runtime: &Runtime) -> Result<Value, OperationError> {
    let current = require_match(runtime)?;
    clear_current_side(runtime, current)?;
    runtime
        .api
        .invoke_void(current, "SwitchToNextPlayer", &mut [])?;
    let second = clear_current_side(runtime, current);
    let restore = runtime
        .api
        .invoke_void(current, "SwitchToNextPlayer", &mut []);
    second?;
    restore?;
    Ok(json!({"cleared": true, "both_sides": true}))
}

fn clear_current_side(runtime: &Runtime, current: *mut Object) -> Result<(), OperationError> {
    let controller = player_controller(runtime, current)?;
    for class in [
        "MAD_ClearUnit",
        "MAD_ClearConstruction",
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
    let construction_manager = runtime
        .api
        .invoke(controller, "GetConstructionManager", &mut [])?;
    if construction_manager.is_null() {
        return Err(OperationError::InvalidState(
            "ConstructionManager is unavailable after clearing".into(),
        ));
    }
    let constructions =
        runtime
            .api
            .invoke(construction_manager, "GetConstructionElements", &mut [])?;
    let construction_count =
        runtime
            .api
            .invoke_value::<i32>(constructions, "get_Count", &mut [])?;
    if construction_count != 0 {
        return Err(OperationError::Rejected(format!(
            "construction clear left {construction_count} elements"
        )));
    }
    Ok(())
}

fn set_both_player_data(
    runtime: &Runtime,
    arguments: &Value,
    data_type: i32,
) -> Result<Value, OperationError> {
    let value = positive_i32(arguments, "value")?;
    let current = require_match(runtime)?;
    let first = set_current_player_data(runtime, current, value, data_type)?;
    runtime
        .api
        .invoke_void(current, "SwitchToNextPlayer", &mut [])?;
    let second_result = set_current_player_data(runtime, current, value, data_type);
    let restore_result = runtime
        .api
        .invoke_void(current, "SwitchToNextPlayer", &mut []);
    let second = second_result?;
    restore_result?;
    if first == second {
        return Err(OperationError::Rejected(
            "player switch did not change player identity".into(),
        ));
    }
    Ok(json!({"changed": true, "value": value, "players": [first, second]}))
}

fn set_current_player_data(
    runtime: &Runtime,
    current: *mut Object,
    value: i32,
    data_type: i32,
) -> Result<i32, OperationError> {
    let controller = player_controller(runtime, current)?;
    let player = runtime.api.invoke(controller, "GetPlayer", &mut [])?;
    let player_index = runtime
        .api
        .invoke_value::<i32>(player, "GetRoomIndex", &mut [])?;
    let getter = if data_type == 0 {
        "GetReactorCore"
    } else {
        "GetSupply"
    };
    let before = runtime.api.invoke_value::<i32>(player, getter, &mut [])?;
    if before != value {
        let action = new_player_test_action(runtime.api, "MAD_ChangePlayerData", controller)?;
        let mut data_type = data_type;
        let mut value = value;
        let mut float_value = 0.0_f32;
        runtime
            .api
            .invoke_void(action, "set_DataType", &mut [argument(&mut data_type)])?;
        runtime
            .api
            .invoke_void(action, "set_DataInt", &mut [argument(&mut value)])?;
        runtime
            .api
            .invoke_void(action, "set_DataFloat", &mut [argument(&mut float_value)])?;
        perform_test(runtime.api, current, action)?;
    }
    let after = runtime.api.invoke_value::<i32>(player, getter, &mut [])?;
    if after != value {
        return Err(OperationError::Rejected(format!(
            "{getter} readback did not match"
        )));
    }
    Ok(player_index)
}

fn upgrade_unit(runtime: &Runtime, arguments: &Value) -> Result<Value, OperationError> {
    let mut unit_index = required_i32(arguments, "unit_index")?;
    let target_level = required_i32(arguments, "target_level")?;
    if !(2..=9).contains(&target_level) {
        return Err(OperationError::InvalidArguments(
            "target_level must be 2..=9".into(),
        ));
    }
    let current = require_match(runtime)?;
    let controller = player_controller(runtime, current)?;
    let (_manager, unit) = find_unit(runtime, controller, &mut unit_index)?;
    let mut unit_id = runtime.api.invoke_value::<i32>(unit, "GetID", &mut [])?;
    let before = runtime.api.invoke_value::<i32>(unit, "GetLevel", &mut [])?;
    if target_level != before + 2 {
        return Err(OperationError::InvalidArguments(format!(
            "target_level must equal internal level {before} plus two"
        )));
    }
    let action = core_action(runtime.api, "PAD_UpgradeUnit")?;
    runtime
        .api
        .invoke_void(action, "set_UIDX", &mut [argument(&mut unit_index)])?;
    runtime
        .api
        .invoke_void(action, "set_UID", &mut [argument(&mut unit_id)])?;
    check_action(runtime.api, controller, action)?;
    perform_sync(runtime.api, controller, action)?;
    let after = runtime.api.invoke_value::<i32>(unit, "GetLevel", &mut [])?;
    if after != target_level {
        return Err(OperationError::Rejected(
            "upgrade readback did not match target".into(),
        ));
    }
    Ok(
        json!({"upgraded": true, "unit_index": unit_index, "before_level": before, "after_level": after}),
    )
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

fn strengthen_tower(runtime: &Runtime, arguments: &Value) -> Result<Value, OperationError> {
    let mut manager_index = required_i32(arguments, "manager_index")?;
    let target_level = required_i32(arguments, "target_level")?;
    if !(0..=2).contains(&target_level) {
        return Err(OperationError::InvalidArguments(
            "target_level must be 0..=2".into(),
        ));
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
    Ok(json!({"strengthened": true, "manager_index": manager_index, "level": current_level}))
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
    arguments: &Value,
    id_name: &str,
    class_name: &str,
) -> Result<Value, OperationError> {
    let mut id = positive_i32(arguments, id_name)?;
    let current = require_training_deploying(runtime)?;
    let controller = player_controller(runtime, current)?;
    let action = new_player_test_action(runtime.api, class_name, controller)?;
    runtime
        .api
        .invoke_void(action, "set_ID", &mut [argument(&mut id)])?;
    perform_test(runtime.api, current, action)?;
    Ok(json!({"added": true, id_name: id}))
}

fn energy_tower_skill(
    runtime: &Runtime,
    arguments: &Value,
    mutate: bool,
) -> Result<Value, OperationError> {
    let mut id = positive_i32(arguments, "skill_id")?;
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
    let check = check_action(runtime.api, controller, action)?;
    if mutate {
        perform_sync(runtime.api, controller, action)?;
        let after = runtime
            .api
            .invoke_value::<bool>(skill, "IsActive", &mut [])?;
        if !after {
            return Err(OperationError::Rejected(
                "energy tower skill did not become active".into(),
            ));
        }
    }
    Ok(json!({"legal": true, "performed": mutate, "skill_id": id, "check_result": check}))
}

fn equipment(runtime: &Runtime, arguments: &Value, mutate: bool) -> Result<Value, OperationError> {
    let mut equipment_id = positive_i32(arguments, "equipment_id")?;
    let mut unit_index = required_i32(arguments, "unit_index")?;
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
    let check = check_action(runtime.api, controller, action)?;
    if mutate {
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
    }
    Ok(
        json!({"legal": true, "performed": mutate, "equipment_id": equipment_id, "unit_index": unit_index, "check_result": check}),
    )
}

fn contraption(
    runtime: &Runtime,
    arguments: &Value,
    mutate: bool,
) -> Result<Value, OperationError> {
    let mut id = positive_i32(arguments, "contraption_id")?;
    let mut position = map_vector(arguments, "position")?;
    let has_extra = arguments.get("extra_position").is_some();
    let mut extra = if has_extra {
        map_vector(arguments, "extra_position")?
    } else {
        MapVector::default()
    };
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
    let check = check_action(runtime.api, controller, action)?;
    let mut contraption_index = None;
    if mutate {
        perform_sync(runtime.api, controller, action)?;
        contraption_index = Some(verify_contraption_readback(runtime, &baseline)?);
    }
    Ok(json!({
        "legal": true,
        "performed": mutate,
        "contraption_id": id,
        "contraption_index": contraption_index,
        "position": {"x": position.x, "y": position.y},
        "check_result": check
    }))
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

fn research_blueprint(runtime: &Runtime, arguments: &Value) -> Result<Value, OperationError> {
    let mut id = positive_i32(arguments, "blueprint_id")?;
    let current = require_training_deploying(runtime)?;
    let controller = player_controller(runtime, current)?;
    let manager = runtime
        .api
        .invoke(controller, "GetBlueprintManager", &mut [])?;
    let blueprint = runtime
        .api
        .invoke(manager, "GetBlueprint", &mut [argument(&mut id)])?;
    if blueprint.is_null() {
        return Err(OperationError::InvalidArguments(
            "blueprint_id was not found".into(),
        ));
    }
    let before_active = runtime
        .api
        .invoke_value::<bool>(blueprint, "IsActive", &mut [])?;
    let before_researching =
        runtime
            .api
            .invoke_value::<bool>(manager, "IsResearching", &mut [argument(&mut id)])?;
    if before_active || before_researching {
        return Err(OperationError::Rejected(
            "blueprint is already active or researching".into(),
        ));
    }
    let action = core_action(runtime.api, "PAD_ActiveBlueprint")?;
    runtime
        .api
        .invoke_void(action, "set_ID", &mut [argument(&mut id)])?;
    check_action(runtime.api, controller, action)?;
    perform_sync(runtime.api, controller, action)?;
    let active = runtime
        .api
        .invoke_value::<bool>(blueprint, "IsActive", &mut [])?;
    let researching =
        runtime
            .api
            .invoke_value::<bool>(manager, "IsResearching", &mut [argument(&mut id)])?;
    if !active && !researching {
        return Err(OperationError::Rejected(
            "blueprint state did not change".into(),
        ));
    }
    Ok(json!({"performed": true, "blueprint_id": id, "active": active, "researching": researching}))
}

fn construction(
    runtime: &Runtime,
    arguments: &Value,
    mutate: bool,
) -> Result<Value, OperationError> {
    let mut id = positive_i32(arguments, "construction_id")?;
    let mut position = map_vector(arguments, "position")?;
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
    let check = check_action(runtime.api, controller, action)?;
    let construction_index = runtime
        .api
        .invoke_value::<i32>(action, "get_IDX", &mut [])?;
    if mutate {
        perform_sync(runtime.api, controller, action)?;
        verify_construction_readback(
            runtime,
            manager,
            id,
            construction_index,
            position,
            before_count,
        )?;
    }
    Ok(json!({
        "legal": true,
        "performed": mutate,
        "construction_id": id,
        "construction_index": construction_index,
        "position": {"x": position.x, "y": position.y},
        "check_result": check
    }))
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

#[allow(clippy::too_many_lines)]
fn battle_skill(
    runtime: &Runtime,
    arguments: &Value,
    mutate: bool,
) -> Result<Value, OperationError> {
    let mut id = positive_i32(arguments, "commander_skill_id")?;
    let target_kind = arguments
        .get("target_kind")
        .and_then(Value::as_str)
        .ok_or_else(|| OperationError::InvalidArguments("missing target_kind".into()))?;
    if !matches!(target_kind, "none" | "unit" | "construction") {
        return Err(OperationError::InvalidArguments(
            "target_kind is invalid".into(),
        ));
    }
    let mut target_index = required_i32(arguments, "target_index")?;
    let positions_value = arguments
        .get("positions")
        .and_then(Value::as_array)
        .ok_or_else(|| OperationError::InvalidArguments("missing positions array".into()))?;
    if positions_value.is_empty() || positions_value.len() > 64 {
        return Err(OperationError::InvalidArguments(
            "positions must contain 1..=64 entries".into(),
        ));
    }
    let mut positions = positions_value
        .iter()
        .map(|value| map_vector(&json!({"position": value}), "position"))
        .collect::<Result<Vec<_>, _>>()?;
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
    let target = match target_kind {
        "unit" => {
            let unit_manager = runtime.api.invoke(controller, "GetUnitManager", &mut [])?;
            let mut target: *mut Object = std::ptr::null_mut();
            let found = runtime.api.invoke_value::<bool>(
                unit_manager,
                "TryGetUnit",
                &mut [argument(&mut target_index), argument(&mut target)],
            )?;
            if !found {
                return Err(OperationError::InvalidArguments(
                    "target unit was not found".into(),
                ));
            }
            target
        }
        "construction" => {
            let construction_manager =
                runtime
                    .api
                    .invoke(controller, "GetConstructionManager", &mut [])?;
            let mut target: *mut Object = std::ptr::null_mut();
            let found = runtime.api.invoke_value::<bool>(
                construction_manager,
                "TryGetConstructionElement",
                &mut [argument(&mut target_index), argument(&mut target)],
            )?;
            if !found {
                return Err(OperationError::InvalidArguments(
                    "target construction was not found".into(),
                ));
            }
            target
        }
        _ => std::ptr::null_mut(),
    };

    if target_kind == "none" {
        for position in &mut positions {
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
    } else {
        let result = runtime.api.invoke_value::<i32>(
            manager,
            "CanReleaseCommanderSkill",
            &mut [
                object_argument(skill),
                argument(&mut positions[0]),
                object_argument(target),
            ],
        )?;
        if result != 0 {
            return Err(OperationError::Rejected(format!(
                "skill target rejected with {result}"
            )));
        }
    }

    let action = core_action(runtime.api, "PAD_ReleaseCommanderSkill")?;
    let action_positions = runtime.api.invoke(action, "get_Positions", &mut [])?;
    for position in &mut positions {
        runtime
            .api
            .invoke_void(action_positions, "Add", &mut [argument(position)])?;
    }
    let mut unit_index = if target_kind == "unit" {
        target_index
    } else {
        -1
    };
    let mut construction_index = if target_kind == "construction" {
        target_index
    } else {
        -1
    };
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
    let check = check_action(runtime.api, controller, action)?;
    if mutate {
        perform_sync(runtime.api, controller, action)?;
    }
    Ok(json!({"legal": true, "performed": mutate, "commander_skill_id": id, "check_result": check}))
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

fn replay_record(runtime: &Runtime, current: *mut Object) -> Result<*mut Object, OperationError> {
    let modules = runtime.api.invoke(current, "GetModules", &mut [])?;
    for index in 0..list_count(runtime.api, modules)? {
        let module = list_item(runtime.api, modules, index)?;
        if runtime.api.object_class_name(module) == "ReplaySystem" {
            let recorder = runtime.api.invoke(module, "GetBattleRecorder", &mut [])?;
            let record = runtime.api.invoke(recorder, "GetCurrentRecord", &mut [])?;
            if !record.is_null() {
                return Ok(record);
            }
        }
    }
    Err(OperationError::InvalidState(
        "training-ground replay record is unavailable".into(),
    ))
}

fn training_ground_seed_bundle(runtime: &Runtime) -> Result<Value, OperationError> {
    let current = require_training_deploying(runtime)?;
    let record = replay_record(runtime, current)?;
    let round_count = runtime
        .api
        .invoke_value::<i32>(record, "GetRoundCount", &mut [])?;
    let player_count = runtime
        .api
        .invoke_value::<i32>(record, "GetPlayerRecordCount", &mut [])?;
    if round_count <= 0 || player_count != 2 {
        return Err(OperationError::InvalidState(
            "training record is incomplete".into(),
        ));
    }
    let info = runtime.api.invoke(record, "get_BattleInfo", &mut [])?;
    let system_seed = runtime
        .api
        .invoke_value::<i32>(info, "get_SystemSeed", &mut [])?;
    let mut seeds = [0_i32; 2];
    for (index, seed) in seeds.iter_mut().enumerate() {
        let mut player_index = i32::try_from(index).expect("two player indexes fit i32");
        let found = runtime.api.invoke_value::<bool>(
            record,
            "TryGetPlayerSeed",
            &mut [argument(&mut player_index), argument(seed)],
        )?;
        if !found {
            return Err(OperationError::InvalidState(format!(
                "player seed {index} is unavailable"
            )));
        }
    }
    Ok(json!({
        "system_seed": system_seed,
        "player_seeds": seeds,
        "round_count": round_count
    }))
}

fn replay_open(runtime: &Runtime, arguments: &Value) -> Result<Value, OperationError> {
    if !runtime.current_match().is_null() {
        return Err(OperationError::InvalidState(
            "a match is already active".into(),
        ));
    }
    if arguments
        .as_object()
        .is_none_or(|object| object.len() != 1 || !object.contains_key("path"))
    {
        return Err(OperationError::InvalidArguments(
            "replay_open requires only path".into(),
        ));
    }
    let path = required_path(arguments, "path")?;
    let utility = runtime
        .api
        .class("GRCore.dll", "GameRiver", "MatchUtility")?;
    let path_text = path
        .to_str()
        .ok_or_else(|| OperationError::InvalidArguments("path must be UTF-8".into()))?;
    let managed_path = runtime.api.string(path_text)?;
    let replay =
        runtime
            .api
            .invoke_static(utility, "LoadReplay", &mut [object_argument(managed_path)])?;
    if replay.is_null() {
        return Err(OperationError::Rejected("LoadReplay returned null".into()));
    }
    let command_class =
        runtime
            .api
            .class("GRClient.dll", "GameRiver.Client", "PlayReplayCommand")?;
    let command = runtime.api.new_object(command_class)?;
    let mut start_round = -1_i32;
    runtime.api.invoke_void(
        command,
        "Execute",
        &mut [object_argument(replay), argument(&mut start_round)],
    )?;
    Ok(json!({"opened": true}))
}

fn replay_goto(runtime: &Runtime, arguments: &Value) -> Result<Value, OperationError> {
    let mut round = positive_i32(arguments, "round")?;
    let current = require_match(runtime)?;
    if classify_replay(runtime.api, current) != Some(true) {
        return Err(OperationError::InvalidState(
            "active match is not a replay".into(),
        ));
    }
    let match_class = runtime
        .api
        .class("GRClient.dll", "GameRiver.Client", "MatchClient")?;
    let setting = runtime
        .api
        .invoke_static(match_class, "get_BattleSetting", &mut [])?;
    let record = runtime.api.invoke(setting, "GetBattleRecord", &mut [])?;
    let available = runtime.api.invoke_value::<bool>(
        record,
        "IsAvaliableRound",
        &mut [argument(&mut round)],
    )?;
    if !available {
        return Err(OperationError::Rejected(
            "requested round is unavailable".into(),
        ));
    }
    let command_class =
        runtime
            .api
            .class("GRClient.dll", "GameRiver.Client", "ReplayMatchGotoCommand")?;
    let command = runtime.api.new_object(command_class)?;
    runtime.api.invoke_void(command, ".ctor", &mut [])?;
    let kind = runtime.api.string("round")?;
    runtime.api.invoke_void(
        command,
        "Execute",
        &mut [object_argument(kind), argument(&mut round)],
    )?;
    Ok(json!({"performed": true, "round": round}))
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

fn tower_strengthen_catalog(runtime: &Runtime) -> Result<Value, OperationError> {
    require_training_deploying(runtime)?;
    let config = config_instance(runtime)?;
    let items = runtime
        .api
        .invoke(config, "GetTowerStrengthenDatas", &mut [])?;
    let mut result = Vec::new();
    for index in 0..list_count(runtime.api, items)? {
        let item = list_item(runtime.api, items, index)?;
        result.push(json!({
            "level": runtime.api.invoke_value::<i32>(item, "GetLevel", &mut [])?,
            "supply": runtime.api.invoke_value::<i32>(item, "GetSupply", &mut [])?
        }));
    }
    Ok(json!({"items": result}))
}

fn technology_catalog(runtime: &Runtime, arguments: &Value) -> Result<Value, OperationError> {
    let unit_id = positive_i32(arguments, "unit_id")?;
    let current = require_match(runtime)?;
    let controller = player_controller(runtime, current)?;
    let manager = runtime.api.invoke(controller, "GetUnitManager", &mut [])?;
    let units = runtime.api.invoke(manager, "GetUnits", &mut [])?;
    let mut unit_data = std::ptr::null_mut();
    for index in 0..list_count(runtime.api, units)? {
        let unit = list_item(runtime.api, units, index)?;
        if runtime.api.invoke_value::<i32>(unit, "GetID", &mut [])? == unit_id {
            unit_data = runtime.api.invoke(unit, "GetUnitData", &mut [])?;
            break;
        }
    }
    if unit_data.is_null() {
        return Err(OperationError::InvalidArguments(
            "requested unit must be deployed".into(),
        ));
    }
    let available = runtime.api.invoke(unit_data, "GetTechnologies", &mut [])?;
    let fight = runtime.current_fight();
    if fight.is_null() {
        return Err(OperationError::InvalidState(
            "fight controller is unavailable".into(),
        ));
    }
    let setting = runtime.api.invoke(fight, "GetFightSetting", &mut [])?;
    let mut technologies = Vec::new();
    for index in 0..list_count(runtime.api, available)? {
        let boxed = list_item(runtime.api, available, index)?;
        let mut id = runtime.api.unbox::<i32>(boxed, "technology id")?;
        let data = runtime
            .api
            .invoke(setting, "GetTechnologyByID", &mut [argument(&mut id)])?;
        if data.is_null() {
            technologies.push(json!({"technology_id": id, "available": false}));
            continue;
        }
        technologies.push(json!({
            "technology_id": id,
            "available": true,
            "runtime_type": runtime.api.object_class_name(data),
            "supply": runtime.api.invoke_value::<i32>(data, "get_Supply", &mut [])?,
            "previous_technology_id": runtime.api.invoke_value::<i32>(data, "get_PreviousTechID", &mut [])?,
            "active_level_internal": runtime.api.invoke_value::<i32>(data, "get_ActiveLevel", &mut [])?,
            "unlock_cost": runtime.api.invoke_value::<i32>(data, "get_UnlockCost", &mut [])?,
            "target_skill_id": runtime.api.invoke_value::<i32>(data, "GetTargetSkillID", &mut [])?
        }));
    }
    Ok(json!({"unit_id": unit_id, "technologies": technologies}))
}

fn required_path(arguments: &Value, name: &str) -> Result<std::path::PathBuf, OperationError> {
    let raw = arguments
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| OperationError::InvalidArguments(format!("missing path {name}")))?;
    let path = Path::new(raw);
    if !path.is_absolute() || raw.contains('\0') {
        return Err(OperationError::InvalidArguments(format!(
            "{name} must be an absolute path"
        )));
    }
    Ok(path.to_path_buf())
}

fn replay_quick_deploy(runtime: &Runtime, arguments: &Value) -> Result<Value, OperationError> {
    let enabled = required_bool(arguments, "enabled")?;
    let current = require_match(runtime)?;
    if classify_replay(runtime.api, current) != Some(true) {
        return Err(OperationError::InvalidState(
            "active match is not a replay".into(),
        ));
    }
    let mut real_time = !enabled;
    let mut step_time = if enabled { 0.0_f32 } else { -1.0_f32 };
    runtime.api.invoke_void(
        current,
        "SetReplayTime",
        &mut [argument(&mut real_time), argument(&mut step_time)],
    )?;
    let after_real_time = runtime
        .api
        .invoke_value::<bool>(current, "get_IsRealTime", &mut [])?;
    let after_step = runtime
        .api
        .invoke_value::<f32>(current, "get_StepTime", &mut [])?;
    #[allow(clippy::float_cmp)]
    let timing_matches = after_real_time == real_time && after_step == step_time;
    if !timing_matches {
        return Err(OperationError::Rejected(
            "replay timing readback did not match".into(),
        ));
    }
    Ok(json!({"enabled": enabled, "is_real_time": after_real_time, "step_time": after_step}))
}

fn required_i32(arguments: &Value, name: &str) -> Result<i32, OperationError> {
    let value = arguments
        .get(name)
        .and_then(Value::as_i64)
        .ok_or_else(|| OperationError::InvalidArguments(format!("missing integer {name}")))?;
    i32::try_from(value)
        .map_err(|_| OperationError::InvalidArguments(format!("{name} is outside i32 range")))
}

fn positive_i32(arguments: &Value, name: &str) -> Result<i32, OperationError> {
    let value = required_i32(arguments, name)?;
    if value > 0 {
        Ok(value)
    } else {
        Err(OperationError::InvalidArguments(format!(
            "{name} must be positive"
        )))
    }
}

fn optional_i32(arguments: &Value, name: &str) -> Option<i32> {
    arguments
        .get(name)
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
}

fn required_bool(arguments: &Value, name: &str) -> Result<bool, OperationError> {
    arguments
        .get(name)
        .and_then(Value::as_bool)
        .ok_or_else(|| OperationError::InvalidArguments(format!("missing boolean {name}")))
}

fn map_vector(arguments: &Value, name: &str) -> Result<MapVector, OperationError> {
    let value = arguments
        .get(name)
        .ok_or_else(|| OperationError::InvalidArguments(format!("missing object {name}")))?;
    Ok(MapVector {
        x: required_i32(value, "x")?,
        y: required_i32(value, "y")?,
    })
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
    fn parses_map_vector() {
        let value = json!({"position": {"x": 12, "y": -3}});
        let position = map_vector(&value, "position").unwrap();
        assert_eq!((position.x, position.y), (12, -3));
    }
}
