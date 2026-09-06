use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL: &str = "mechcore.adapter.v1";
pub const MAX_ACTIVATION_ROUND: i32 = 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Status,
    StartTest,
    ApplyLayout,
    RecordBattle,
    RecordReplayRound,
    ToggleFight,
    SpeedUp,
    QuitMatch,
    QuitGame,
}

impl Operation {
    pub const ALL: [Self; 9] = [
        Self::Status,
        Self::StartTest,
        Self::ApplyLayout,
        Self::RecordBattle,
        Self::RecordReplayRound,
        Self::ToggleFight,
        Self::SpeedUp,
        Self::QuitMatch,
        Self::QuitGame,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::StartTest => "start_test",
            Self::ApplyLayout => "apply_layout",
            Self::RecordBattle => "record_battle",
            Self::RecordReplayRound => "record_replay_round",
            Self::ToggleFight => "toggle_fight",
            Self::SpeedUp => "speed_up",
            Self::QuitMatch => "quit_match",
            Self::QuitGame => "quit_game",
        }
    }
}

impl std::fmt::Display for Operation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameStatus {
    MainMenu,
    TrainingGround,
    Replay,
    Unknown,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Hello {
    pub kind: String,
    pub protocol: String,
    pub capabilities: Vec<Operation>,
}

impl Hello {
    #[must_use]
    pub fn current() -> Self {
        Self {
            kind: "hello".into(),
            protocol: PROTOCOL.into(),
            capabilities: Operation::ALL.to_vec(),
        }
    }
}

/// Greeting sent when the endpoint is already serving another client.
///
/// The accept loop serves one client at a time, so a second connection would
/// otherwise wait in the backlog and be indistinguishable from an unresponsive
/// adapter. Answering explicitly keeps occupancy a protocol fact.
#[derive(Debug, Serialize, Deserialize)]
pub struct Busy {
    pub kind: String,
    pub protocol: String,
}

impl Busy {
    #[must_use]
    pub fn current() -> Self {
        Self {
            kind: "busy".into(),
            protocol: PROTOCOL.into(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub id: u64,
    pub operation: Operation,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response<T> {
    pub kind: String,
    pub id: u64,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}

impl<T> Response<T> {
    pub fn success(id: u64, result: T) -> Self {
        Self {
            kind: "response".into(),
            id,
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    pub fn failure(id: u64, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: "response".into(),
            id,
            ok: false,
            result: None,
            error: Some(ErrorBody {
                code: code.into(),
                message: message.into(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_vocabulary_is_closed() {
        assert_eq!(
            Operation::ALL.map(Operation::as_str),
            [
                "status",
                "start_test",
                "apply_layout",
                "record_battle",
                "record_replay_round",
                "toggle_fight",
                "speed_up",
                "quit_match",
                "quit_game",
            ]
        );
    }

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
        assert_eq!(
            serde_json::to_value(Hello::current()).unwrap(),
            serde_json::json!({
                "kind": "hello",
                "protocol": "mechcore.adapter.v1",
                "capabilities": [
                    "status",
                    "start_test",
                    "apply_layout",
                    "record_battle",
                    "record_replay_round",
                    "toggle_fight",
                    "speed_up",
                    "quit_match",
                    "quit_game",
                ],
            })
        );
    }
}
