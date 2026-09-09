use mechcore_document::Layout;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

pub const PROTOCOL: &str = "mechcore.adapter.v1";
/// Highest round `apply_layout` will stage.
///
/// This is the executor's timeout budget for advancing through every earlier
/// setup round, not a game rule. Training Ground and ranked matches both run
/// past it, so layout validation, replay decoding, and any recorded round
/// number must not be bounded by this value.
pub const MAX_STAGED_ROUND: i32 = 15;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

/// Arguments for [`Operation::StartTest`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartTestArguments {
    /// Requested match seed; `None` asks the game to generate one.
    pub seed: Option<i32>,
    /// Native `MatchSetting` map; `None` selects the Training Ground baseline.
    pub map_id: Option<i32>,
}

/// Arguments for [`Operation::ApplyLayout`].
///
/// The operation carries the layout document itself rather than wrapping it, so
/// this is the same type both sides already validate with `mechcore-document`.
pub type ApplyLayoutArguments = Layout;

/// Arguments for [`Operation::RecordBattle`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordBattleArguments {
    /// Absolute destination for the new MCFR recording.
    pub output: PathBuf,
    pub video_output: Option<PathBuf>,
    /// Request native combat speed-up. `None` leaves the adapter default, which
    /// is on with or without a visual recording.
    pub speed_up: Option<bool>,
    pub instrumentation: Option<RecordBattleInstrumentation>,
}

/// Arguments for [`Operation::RecordReplayRound`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordReplayRoundArguments {
    /// Absolute path to the `.grbr` replay to load.
    pub grbr: PathBuf,
    /// One-based combat round to record from that replay.
    pub round: i32,
    /// Absolute destination for the new MCFR recording.
    pub output: PathBuf,
    pub speed_up: Option<bool>,
    pub instrumentation: Option<RecordBattleInstrumentation>,
}

/// Research-only instrumentation request accepted by the recording operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordBattleInstrumentation {
    /// Absolute destination for the new HDF5 instrumentation sidecar.
    pub output: PathBuf,
    pub profile: CaptureInstrumentationProfile,
    /// Bound RVO detail to selected MCFR units and combat update-start ticks.
    pub rvo_scope: Option<RvoCaptureScope>,
}

/// Temporary research profile selecting which instrumentation channels record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureInstrumentationProfile {
    TargetRefsV1,
    TargetRefsRvoV1,
    SkillAttackableCheckerV1,
    SelectorScoreV1,
    SelectorScoreRvoV1,
}

impl CaptureInstrumentationProfile {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TargetRefsV1 => "target_refs_v1",
            Self::TargetRefsRvoV1 => "target_refs_rvo_v1",
            Self::SkillAttackableCheckerV1 => "skill_attackable_checker_v1",
            Self::SelectorScoreV1 => "selector_score_v1",
            Self::SelectorScoreRvoV1 => "selector_score_rvo_v1",
        }
    }
}

/// Research-only filter using one-based MCFR combat ticks. In build 2259,
/// `FightController.Update` advances the native time counter by 100 per tick.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RvoCaptureScope {
    pub start_tick: u64,
    pub end_tick: u64,
    pub unit_ids: Vec<u64>,
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

    /// The field names are the contract, so state them once outside the types.
    ///
    /// Both ends of the socket compile against these structs, which is what
    /// makes a mismatch impossible rather than merely detectable. This pins the
    /// names the game already accepts, so renaming a field has to be a decision
    /// rather than a rename that still compiles on both sides and fails only
    /// against a running adapter.
    #[test]
    fn request_arguments_carry_the_fields_the_adapter_accepts() {
        let start = serde_json::to_value(StartTestArguments {
            seed: Some(42),
            map_id: Some(1001),
        })
        .unwrap();
        assert_eq!(start, serde_json::json!({"seed": 42, "map_id": 1001}));

        let replay = serde_json::to_value(RecordReplayRoundArguments {
            grbr: PathBuf::from("/tmp/a.grbr"),
            round: 2,
            output: PathBuf::from("/tmp/a.mcfr"),
            speed_up: None,
            instrumentation: None,
        })
        .unwrap();
        assert_eq!(
            replay,
            serde_json::json!({
                "grbr": "/tmp/a.grbr",
                "round": 2,
                "output": "/tmp/a.mcfr",
                "speed_up": null,
                "instrumentation": null
            })
        );

        let battle = serde_json::to_value(RecordBattleArguments {
            output: PathBuf::from("/tmp/b.mcfr"),
            video_output: None,
            speed_up: Some(true),
            instrumentation: Some(RecordBattleInstrumentation {
                output: PathBuf::from("/tmp/b.h5"),
                profile: CaptureInstrumentationProfile::TargetRefsRvoV1,
                rvo_scope: Some(RvoCaptureScope {
                    start_tick: 1,
                    end_tick: 2,
                    unit_ids: vec![7],
                }),
            }),
        })
        .unwrap();
        assert_eq!(
            battle,
            serde_json::json!({
                "output": "/tmp/b.mcfr",
                "video_output": null,
                "speed_up": true,
                "instrumentation": {
                    "output": "/tmp/b.h5",
                    "profile": "target_refs_rvo_v1",
                    "rvo_scope": {"start_tick": 1, "end_tick": 2, "unit_ids": [7]}
                }
            })
        );
    }

    /// An argument these types do not define must not reach the game.
    #[test]
    fn an_undeclared_argument_field_is_refused() {
        let error = serde_json::from_value::<RecordReplayRoundArguments>(serde_json::json!({
            "grbr": "/tmp/a.grbr",
            "kind": "layout",
            "round": 2,
            "output": "/tmp/a.mcfr"
        }))
        .unwrap_err()
        .to_string();
        assert!(error.contains("unknown field `kind`"), "{error}");
    }
}
