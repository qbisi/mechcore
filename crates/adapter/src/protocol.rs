use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL: &str = "mechcore.adapter.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameStatus {
    MainMenu,
    TrainingGround,
    Replay,
    Unknown,
}

#[derive(Debug, Serialize)]
pub struct Hello {
    pub kind: &'static str,
    pub protocol: &'static str,
    pub capabilities: &'static [&'static str],
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub id: u64,
    pub operation: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Debug, Serialize)]
pub struct Response<T: Serialize> {
    pub kind: &'static str,
    pub id: u64,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub code: &'static str,
    pub message: String,
}

impl<T: Serialize> Response<T> {
    pub fn success(id: u64, result: T) -> Self {
        Self {
            kind: "response",
            id,
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    pub fn failure(id: u64, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind: "response",
            id,
            ok: false,
            result: None,
            error: Some(ErrorBody {
                code,
                message: message.into(),
            }),
        }
    }
}

pub const CAPABILITIES: &[&str] = &[
    "status",
    "start_test",
    "apply_layout",
    "toggle_fight",
    "speed_up",
    "quit_match",
    "quit_game",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_vocabulary_is_closed() {
        let statuses = [
            GameStatus::MainMenu,
            GameStatus::TrainingGround,
            GameStatus::Replay,
            GameStatus::Unknown,
        ];
        let values: Vec<_> = statuses
            .iter()
            .map(|status| serde_json::to_value(status).unwrap())
            .collect();
        assert_eq!(
            values,
            ["main_menu", "training_ground", "replay", "unknown"]
                .map(|value| Value::String(value.into()))
        );
    }

    #[test]
    fn hello_is_minimal_compatibility_handshake() {
        let hello = Hello {
            kind: "hello",
            protocol: PROTOCOL,
            capabilities: &["status"],
        };
        assert_eq!(
            serde_json::to_value(hello).unwrap(),
            serde_json::json!({
                "kind": "hello",
                "protocol": "mechcore.adapter.v1",
                "capabilities": ["status"],
            })
        );
    }

    #[test]
    fn all_capabilities_have_dispatcher_arms() {
        assert_eq!(CAPABILITIES.len(), 7);
        assert_eq!(
            CAPABILITIES,
            [
                "status",
                "start_test",
                "apply_layout",
                "toggle_fight",
                "speed_up",
                "quit_match",
                "quit_game",
            ]
        );
        let dispatcher = include_str!("operations.rs");
        for capability in CAPABILITIES {
            assert!(
                dispatcher.contains(&format!("\"{capability}\" =>")),
                "missing dispatcher arm for {capability}"
            );
        }
    }
}
