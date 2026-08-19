use crate::il2cpp::{Api, Error as Il2CppError, Object};
use crate::protocol::{CAPABILITIES, GameStatus, Request, Response};
use crate::runtime::Runtime;
use serde_json::{Value, json};

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
}
