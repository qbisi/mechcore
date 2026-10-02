use mechcore_document::Layout;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

/// Wire contract name, and the version gate between a client and an adapter.
///
/// A running game keeps the Adapter it was started with, so a rebuilt Adapter
/// and a running game can differ. Naming the contract is what turns that into
/// one clear refusal at connect time instead of a desynchronised stream.
pub const PROTOCOL: &str = "mechcore.adapter.v10";
/// Highest round `apply_layout` will stage.
///
/// This is the executor's timeout budget for advancing through every earlier
/// setup round, not a game rule. Training Ground and ranked matches both run
/// past it, so layout validation, replay decoding, and any recorded round
/// number must not be bounded by this value.
pub const MAX_STAGED_ROUND: i32 = 15;
pub const DEFAULT_WATCH_SCENE_WAIT_SECONDS: u64 = 15 * 60;
pub const DEFAULT_WATCH_MATCH_TIMEOUT_SECONDS: u64 = 2 * 60 * 60;
pub const MAX_WATCH_SCENE_WAIT_SECONDS: u64 = 24 * 60 * 60;
pub const MAX_WATCH_MATCH_TIMEOUT_SECONDS: u64 = 4 * 60 * 60;

/// Highest run level a client may claim.
///
/// Levels order clients, nothing else: a claim strictly above the level of the
/// client being served takes the game from it. Five is enough to separate a
/// background corpus batch from ordinary work, and few enough that a number is
/// still a decision rather than a habit.
pub const MAX_LEVEL: u8 = 4;

/// The level a client runs at when its script does not say.
pub const DEFAULT_LEVEL: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Status,
    StartTest,
    ApplyLayout,
    RecordFight,
    RecordReplayRound,
    RecordWatchReplay,
    ToggleFight,
    SpeedUp,
    QuitMatch,
    QuitGame,
    WatchScenes,
    WatchScene,
    SaveReplay,
}

impl Operation {
    pub const ALL: [Self; 13] = [
        Self::Status,
        Self::StartTest,
        Self::ApplyLayout,
        Self::RecordFight,
        Self::RecordReplayRound,
        Self::RecordWatchReplay,
        Self::ToggleFight,
        Self::SpeedUp,
        Self::QuitMatch,
        Self::QuitGame,
        Self::WatchScenes,
        Self::WatchScene,
        Self::SaveReplay,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::StartTest => "start_test",
            Self::ApplyLayout => "apply_layout",
            Self::RecordFight => "record_fight",
            Self::RecordReplayRound => "record_replay_round",
            Self::RecordWatchReplay => "record_watch_replay",
            Self::ToggleFight => "toggle_fight",
            Self::SpeedUp => "speed_up",
            Self::QuitMatch => "quit_match",
            Self::QuitGame => "quit_game",
            Self::WatchScenes => "watch_scenes",
            Self::WatchScene => "watch_scene",
            Self::SaveReplay => "save_replay",
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
    Spectating,
    Unknown,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Hello {
    pub kind: String,
    pub protocol: String,
    pub capabilities: Vec<Operation>,
    /// The game the client was admitted to, which is how a launch decides
    /// whether a game left running can do its work.
    pub game: GameIdentity,
}

impl Hello {
    #[must_use]
    pub fn current(game: GameIdentity) -> Self {
        Self {
            kind: "hello".into(),
            protocol: PROTOCOL.into(),
            capabilities: Operation::ALL.to_vec(),
            game,
        }
    }
}

/// What a running game is, beyond the operations it answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameIdentity {
    /// BLAKE3 of the Adapter library the game loaded, in hex. A game keeps the
    /// Adapter it started with, so this is what tells a rebuilt one apart.
    pub adapter: String,
    /// Whether the game was started with `-nographics`, and renders nothing.
    pub headless: bool,
    /// Whether the game runs in a sandbox, which is how `mechcore` takes its
    /// network away, Steam's included.
    pub offline: bool,
    /// How long the game waits for its next client before it quits itself, or
    /// `None` for a game that waits for ever. Only a game `mechcore` launched
    /// waits a bounded time.
    pub linger_seconds: Option<u64>,
}

/// The first message a client sends, before it is greeted or refused.
///
/// A client says what it is worth before it asks for anything, because that is
/// what the adapter needs in order to answer: the game goes to the higher
/// level, and the claim is the only place that level is ever stated.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    pub kind: String,
    pub protocol: String,
    pub level: u8,
}

impl Claim {
    #[must_use]
    pub fn current(level: u8) -> Self {
        Self {
            kind: "claim".into(),
            protocol: PROTOCOL.into(),
            level,
        }
    }
}

/// Answer to a claim that does not outrank the client being served.
///
/// `holder_level` is what it lost to. `evicting` says the claim did win and
/// the game is being handed back right now: the client is expected to connect
/// again rather than to give up, because the adapter admits its next client
/// only once the game is at the main menu.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Busy {
    pub kind: String,
    pub protocol: String,
    pub holder_level: u8,
    pub evicting: bool,
}

impl Busy {
    #[must_use]
    pub fn current(holder_level: u8, evicting: bool) -> Self {
        Self {
            kind: "busy".into(),
            protocol: PROTOCOL.into(),
            holder_level,
            evicting,
        }
    }
}

/// Answer to a claim the adapter cannot act on at all.
///
/// A wrong protocol or an out-of-range level is not occupancy, and saying so
/// is what keeps it from being diagnosed as a wedged adapter.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Refused {
    pub kind: String,
    pub protocol: String,
    pub reason: String,
}

impl Refused {
    #[must_use]
    pub fn current(reason: impl Into<String>) -> Self {
        Self {
            kind: "refused".into(),
            protocol: PROTOCOL.into(),
            reason: reason.into(),
        }
    }
}

/// Last message to a client that is losing the game to a higher claim.
///
/// The connection closes immediately after it. It is what separates a taken
/// game from a crashed one, and a client that reads it must leave the game
/// process alone: the adapter is keeping it for whoever claimed it.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evicted {
    pub kind: String,
    pub protocol: String,
    pub by_level: u8,
}

impl Evicted {
    #[must_use]
    pub fn current(by_level: u8) -> Self {
        Self {
            kind: "evicted".into(),
            protocol: PROTOCOL.into(),
            by_level,
        }
    }
}

/// Error code carried by an operation the adapter abandoned for a higher claim.
pub const EVICTED_CODE: &str = "evicted";

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

/// Arguments for [`Operation::RecordFight`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordFightArguments {
    /// Absolute destination for the new MCFR recording.
    pub output: PathBuf,
    pub video_output: Option<PathBuf>,
    /// Request native combat speed-up. `None` leaves the adapter default, which
    /// is on with or without a visual recording.
    pub speed_up: Option<bool>,
    /// Instrument channels to record into the MCFR, outside the hash.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instrument: Vec<InstrumentChannel>,
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
    /// Instrument channels to record into the MCFR, outside the hash.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instrument: Vec<InstrumentChannel>,
}

/// Arguments for [`Operation::RecordWatchReplay`].
///
/// The selection policy is deliberately fixed: a server-provided matchmaking
/// scene must be a normal `VS_1_1` match in round one. This operation is a
/// corpus collector, not a general-purpose custom-room watcher.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordWatchReplayArguments {
    /// Existing absolute directory that receives one newly published `.grbr`.
    ///
    /// When omitted, the game-owned Replay directory is used directly and no
    /// corpus copy is created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_dir: Option<PathBuf>,
    /// Maximum time to wait for an eligible round-one matchmaking scene.
    pub wait_for_scene_seconds: u64,
    /// Maximum time from stable round-one entry until the match finishes.
    pub match_timeout_seconds: u64,
}

/// An instrument channel a recording can carry: a view of the fight's inside,
/// stored in the MCFR beside its tables and not read by the hash. Channels
/// combine freely, so one recording answers every question asked of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstrumentChannel {
    /// Each unit's lock and its main skill's targets and state, every tick.
    TargetRefs,
    /// Every `SkillAttackableChecker.Check` call, with the skill before and after.
    SkillAttackableChecker,
    /// Every target search: its source, path and choice.
    TargetSearch,
    /// Each target search's best-scored candidates, a bounded few per search.
    TargetCandidate,
    /// One row per RVO solve: the agent's inputs, bias, traces and output.
    RvoSolve,
    /// Each solve's neighbour list, at most `maxNeighbours` rows per solve.
    RvoNeighbour,
    /// Each solve's VOs, field by field.
    RvoVo,
    /// Each grouped unit's skills, one row per slot: lock, attack target and state.
    GroupSlots,
    /// Each unit's model as the view animates it: per animator layer, the state,
    /// its progress and the clips it blends, every tick.
    UnitPose,
    /// Each projectile's reach check: how far from its owner it may be and
    /// still land, against where it stands.
    ProjectileReach,
}

impl InstrumentChannel {
    pub const ALL: [Self; 10] = [
        Self::TargetRefs,
        Self::SkillAttackableChecker,
        Self::TargetSearch,
        Self::TargetCandidate,
        Self::RvoSolve,
        Self::RvoNeighbour,
        Self::RvoVo,
        Self::GroupSlots,
        Self::UnitPose,
        Self::ProjectileReach,
    ];

    /// The channel's name, which is also its member's file stem in the MCFR.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TargetRefs => "target_refs",
            Self::SkillAttackableChecker => "skill_attackable_checker",
            Self::TargetSearch => "target_search",
            Self::TargetCandidate => "target_candidate",
            Self::RvoSolve => "rvo_solve",
            Self::RvoNeighbour => "rvo_neighbour",
            Self::RvoVo => "rvo_vo",
            Self::GroupSlots => "group_slots",
            Self::UnitPose => "unit_pose",
            Self::ProjectileReach => "projectile_reach",
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
                "record_fight",
                "record_replay_round",
                "record_watch_replay",
                "toggle_fight",
                "speed_up",
                "quit_match",
                "quit_game",
                "watch_scenes",
                "watch_scene",
                "save_replay",
            ]
        );
    }

    #[test]
    fn status_vocabulary_is_closed() {
        let statuses = [
            GameStatus::MainMenu,
            GameStatus::TrainingGround,
            GameStatus::Replay,
            GameStatus::Spectating,
            GameStatus::Unknown,
        ];
        let values: Vec<_> = statuses
            .iter()
            .map(|status| serde_json::to_value(status).unwrap())
            .collect();
        assert_eq!(
            values,
            [
                "main_menu",
                "training_ground",
                "replay",
                "spectating",
                "unknown",
            ]
            .map(|value| Value::String(value.into()))
        );
    }

    #[test]
    fn hello_names_the_contract_and_the_game() {
        assert_eq!(
            serde_json::to_value(Hello::current(GameIdentity {
                adapter: "ab".into(),
                headless: true,
                offline: true,
                linger_seconds: Some(30),
            }))
            .unwrap(),
            serde_json::json!({
                "kind": "hello",
                "protocol": "mechcore.adapter.v10",
                "capabilities": [
                    "status",
                    "start_test",
                    "apply_layout",
                    "record_fight",
                    "record_replay_round",
                    "record_watch_replay",
                    "toggle_fight",
                    "speed_up",
                    "quit_match",
                    "quit_game",
                    "watch_scenes",
                    "watch_scene",
                    "save_replay",
                ],
                "game": {"adapter": "ab", "headless": true, "offline": true, "linger_seconds": 30},
            })
        );
    }

    #[test]
    fn a_client_states_its_level_before_it_is_greeted_or_refused() {
        assert_eq!(
            serde_json::to_value(Claim::current(DEFAULT_LEVEL)).unwrap(),
            serde_json::json!({
                "kind": "claim",
                "protocol": "mechcore.adapter.v10",
                "level": 1,
            })
        );
        assert_eq!(
            serde_json::to_value(Busy::current(3, true)).unwrap(),
            serde_json::json!({
                "kind": "busy",
                "protocol": "mechcore.adapter.v10",
                "holder_level": 3,
                "evicting": true,
            })
        );
        assert_eq!(
            serde_json::to_value(Evicted::current(4)).unwrap(),
            serde_json::json!({
                "kind": "evicted",
                "protocol": "mechcore.adapter.v10",
                "by_level": 4,
            })
        );
        // A claim that carries anything else is not this message.
        assert!(
            serde_json::from_value::<Claim>(
                serde_json::json!({"kind": "claim", "protocol": PROTOCOL, "level": 1, "force": true})
            )
            .is_err()
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
            instrument: Vec::new(),
        })
        .unwrap();
        assert_eq!(
            replay,
            serde_json::json!({
                "grbr": "/tmp/a.grbr",
                "round": 2,
                "output": "/tmp/a.mcfr"
            })
        );

        let fight = serde_json::to_value(RecordFightArguments {
            output: PathBuf::from("/tmp/b.mcfr"),
            video_output: None,
            speed_up: Some(true),
            instrument: vec![
                InstrumentChannel::TargetRefs,
                InstrumentChannel::SkillAttackableChecker,
            ],
        })
        .unwrap();
        assert_eq!(
            fight,
            serde_json::json!({
                "output": "/tmp/b.mcfr",
                "video_output": null,
                "speed_up": true,
                "instrument": ["target_refs", "skill_attackable_checker"]
            })
        );

        let watch = serde_json::to_value(RecordWatchReplayArguments {
            output_dir: Some(PathBuf::from("/tmp/grbr-corpus")),
            wait_for_scene_seconds: 900,
            match_timeout_seconds: 7_200,
        })
        .unwrap();
        assert_eq!(
            watch,
            serde_json::json!({
                "output_dir": "/tmp/grbr-corpus",
                "wait_for_scene_seconds": 900,
                "match_timeout_seconds": 7200,
            })
        );

        let native_watch = serde_json::to_value(RecordWatchReplayArguments {
            output_dir: None,
            wait_for_scene_seconds: 900,
            match_timeout_seconds: 7_200,
        })
        .unwrap();
        assert_eq!(
            native_watch,
            serde_json::json!({
                "wait_for_scene_seconds": 900,
                "match_timeout_seconds": 7200,
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
