//! The `game` namespace: the running game process.
//!
//! Its verbs are the adapter's operations, under the names
//! `docs/spec/adapter/adapter.md` gives them, and one argument reader serves
//! both the shell and a one-shot command. How a process acquires the game is
//! `docs/spec/mechcore/session.md`: `launch` starts one and leaves it for the
//! commands after it, and every other verb joins one somebody started.

use std::{path::PathBuf, sync::Arc};

use serde_json::Value;

use crate::acquire::{Launch, Mode, Ownership};
use crate::cli::{Args, Failure, Outcome, Verdict};
use crate::session::Session;

/// The verbs that act on a game, for the help the shell prints.
pub(crate) const OPERATIONS: &[&str] = &[
    "status",
    "start_test",
    "apply_layout",
    "record",
    "toggle_fight",
    "speed_up",
    "quit_match",
    "quit_game",
];

/// One operation, acquired for the length of the command.
///
/// A command is one operation and then an exit, so it joins a game somebody
/// else is keeping alive, except `launch`, which starts one and leaves it to
/// linger for the next command.
///
/// # Errors
///
/// Returns a usage failure for a verb this namespace does not hold or an
/// acquisition it cannot make, an unavailable failure when no game answers,
/// and whatever the operation refuses.
pub(crate) fn run(mut arguments: Args) -> Outcome {
    let verb = arguments.operand("a verb: status, apply_layout, record, ...")?;
    one(&verb, arguments).map_err(|failure| failure.at(format!("game.{verb}")))
}

/// One operation, once the verb is known.
fn one(verb: &str, mut arguments: Args) -> Outcome {
    if matches!(verb, "attach" | "detach") {
        return Err(Failure::usage(format!(
            "{verb} holds a game for longer than one command; acquire in \
             `mechcore shell`, or start one with `game launch`, and name the operation here"
        )));
    }
    let level = crate::acquire::level(&mut arguments)?;
    if verb == "launch" {
        let how = Launch {
            headless: arguments.flag("--headless")?,
            offline: arguments.flag("--offline")?,
        };
        arguments.finish()?;
        return launch(how, level);
    }
    if verb == "record" {
        // What is recorded is decided, and refused, before any game is
        // reached.
        let force = force(&mut arguments)?;
        let record = Record::read(&mut arguments)?;
        arguments.finish()?;
        return record_attached(record, force, level);
    }
    if arguments.flag("--launch")? {
        return Err(Failure::usage(
            "a command joins a game somebody started; start one with `game launch`, \
             and it lingers for 30 s after each client leaves",
        ));
    }
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| Failure::failed(format!("cannot create async runtime: {error}")))?
        .block_on(attached(verb, arguments, level))
}

/// Starts a game and leaves it at the main menu for the commands after this
/// one, which it outlives by [`crate::acquire::LINGER`] each. A game already
/// running is joined instead, as the session matrix says, and lingers as it
/// would.
///
/// # Errors
///
/// Returns `unavailable` when the game cannot be started or reached.
fn launch(how: Launch, level: u8) -> Outcome {
    let (ownership, endpoint) = with_session(Mode::Launch(how), level, async |session| {
        session.endpoint().display().to_string()
    })?;
    let log = match &ownership {
        Ownership::Launched { log } => Value::String(log.display().to_string()),
        Ownership::Attached => Value::Null,
    };
    println!(
        "{}",
        serde_json::json!({
            "launched": ownership.is_launched(),
            "endpoint": endpoint,
            "log": log,
            "linger_seconds": crate::acquire::LINGER.as_secs(),
        })
    );
    Ok(Verdict::Yes)
}

/// Records `record` in a game somebody started, as a command joins one: the
/// game backend of `convert`.
///
/// # Errors
///
/// Returns `unavailable` when no game answers, and what the recording
/// refuses.
pub(crate) fn record_attached(record: Record, force: bool, level: u8) -> Outcome {
    let value = with_game(level, async |session| record.run(session, force).await)??;
    println!("{value}");
    Ok(Verdict::Yes)
}

/// Runs `work` against a game somebody started, attached for its whole
/// length, so a batch holds the game once rather than once per file.
///
/// # Errors
///
/// Returns `unavailable` when no game answers.
pub(crate) fn with_game<T>(
    level: u8,
    work: impl AsyncFnOnce(&Arc<Session>) -> T,
) -> Result<T, Failure> {
    with_session(Mode::Attach, level, work).map(|(_, answer)| answer)
}

/// Runs `work` against a game acquired as `mode` says, and leaves it.
fn with_session<T>(
    mode: Mode,
    level: u8,
    work: impl AsyncFnOnce(&Arc<Session>) -> T,
) -> Result<(Ownership, T), Failure> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| Failure::failed(format!("cannot create async runtime: {error}")))?
        .block_on(async move {
            let session = Session::new();
            let monitor = tokio::spawn(Session::monitor_status(session.clone()));
            let answered = match session.acquire(mode, level).await {
                Ok(ownership) => {
                    let answer = work(&session).await;
                    session.release().await;
                    Ok((ownership, answer))
                }
                Err(failure) => Err(Failure::unavailable(failure)),
            };
            monitor.abort();
            answered
        })
}

/// Attaches, runs the one operation, and leaves the game to its owner.
async fn attached(verb: &str, arguments: Args, level: u8) -> Outcome {
    let session = Session::new();
    let monitor = tokio::spawn(Session::monitor_status(session.clone()));
    let answered = match session.acquire(Mode::Attach, level).await {
        Ok(_) => {
            let answer = operate(verb, arguments, &session).await;
            session.release().await;
            answer
        }
        Err(failure) => Err(Failure::unavailable(failure)),
    };
    monitor.abort();
    let value = answered?;
    println!("{value}");
    Ok(Verdict::Yes)
}

fn read_layout(path: &std::path::Path) -> Result<Value, Failure> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| Failure::failed(format!("cannot read {}: {error}", path.display())))?;
    serde_yaml::from_str(&text)
        .map_err(|error| Failure::refused(format!("cannot parse {}: {error}", path.display())))
}

/// Runs one operation against an acquired game.
///
/// # Errors
///
/// Returns a usage failure for a verb this namespace does not hold or
/// arguments it cannot read, and a refusal for an operation the game does not
/// carry out.
pub(crate) async fn operate(
    verb: &str,
    mut arguments: Args,
    session: &Arc<Session>,
) -> Result<Value, Failure> {
    match verb {
        "status" => {
            arguments.finish()?;
            Ok(session.current_status())
        }
        "start_test" => {
            let map_id = arguments.parsed::<i32>("--map-id", "an integer")?;
            let seed = seed(&mut arguments)?;
            arguments.finish()?;
            session.start_test(seed, map_id).await.map_err(refusal)
        }
        "apply_layout" => {
            let path = arguments.path("a layout to apply")?;
            let seed = seed(&mut arguments)?;
            arguments.finish()?;
            let layout = read_layout(&path)?;
            session.apply_layout(layout, seed).await.map_err(refusal)
        }
        "record" => {
            let force = force(&mut arguments)?;
            let record = Record::read(&mut arguments)?;
            arguments.finish()?;
            record.run(session, force).await
        }
        "toggle_fight" => {
            arguments.finish()?;
            session.toggle_fight().await.map_err(refusal)
        }
        "speed_up" => {
            arguments.finish()?;
            session.speed_up().await.map_err(refusal)
        }
        "quit_match" => {
            arguments.finish()?;
            session.quit_match().await.map_err(refusal)
        }
        "quit_game" => {
            arguments.finish()?;
            session.quit_game().await.map_err(refusal)
        }
        other => Err(Failure::usage(format!(
            "game has no verb {other:?}; it has {}",
            OPERATIONS.join(", ")
        ))),
    }
}

/// Where a file is fought into a recording, which `game record` does not do.
pub(crate) const FILES_ARE_CONVERTED: &str = "game record records the scene or a watched match; \
     a layout, a fight or a replay's round is fought into a recording by \
     `convert <in> --to mcfr --backend game`";

/// What `game record` records, which its input decides.
///
/// No input is the fight staged in the current scene; a layout is fought
/// without a scene; a replay is fought at one of its rounds; and `--watch`
/// records a live match the server makes. The input's kind is read from what
/// it holds, as every file verb reads it.
pub(crate) enum Record {
    Scene {
        output: PathBuf,
        video: Option<PathBuf>,
        speed_up: Option<bool>,
        instrument: Vec<mechcore_protocol::InstrumentChannel>,
    },
    Layout {
        layout: Value,
        seed: Option<i32>,
        output: PathBuf,
        instrument: Vec<mechcore_protocol::InstrumentChannel>,
    },
    Replay {
        grbr: PathBuf,
        round: i32,
        output: PathBuf,
        instrument: Vec<mechcore_protocol::InstrumentChannel>,
    },
    Watch {
        output_dir: Option<PathBuf>,
        wait_for_scene_seconds: u64,
        match_timeout_seconds: u64,
    },
}

/// What `game record` reads, before the input is looked at.
#[derive(Default)]
pub(crate) struct RecordRequest {
    pub(crate) input: Option<PathBuf>,
    pub(crate) output: Option<PathBuf>,
    pub(crate) seed: Option<i32>,
    pub(crate) round: Option<i32>,
    pub(crate) video: Option<PathBuf>,
    pub(crate) speed_up: Option<bool>,
    pub(crate) instrument: Vec<mechcore_protocol::InstrumentChannel>,
    pub(crate) watch: bool,
    pub(crate) output_dir: Option<PathBuf>,
    pub(crate) wait_for_scene_seconds: Option<u64>,
    pub(crate) match_timeout_seconds: Option<u64>,
}

/// Refuses the first option given that `what` does not take.
fn refuse_given(options: &[(bool, &str)], what: &str) -> Result<(), Failure> {
    match options.iter().find(|(given, _)| *given) {
        Some((_, option)) => Err(Failure::usage(format!("{option} does not apply to {what}"))),
        None => Ok(()),
    }
}

impl RecordRequest {
    /// Decides what is recorded, refusing what the input does not take.
    ///
    /// # Errors
    ///
    /// Returns a usage failure for an option the input does not take or a
    /// missing one it needs, and a refusal for an input kind that is not
    /// fought.
    pub(crate) fn decide(self) -> Result<Record, Failure> {
        if self.watch {
            return self.watch();
        }
        refuse_given(
            &[
                (self.output_dir.is_some(), "an output directory"),
                (self.wait_for_scene_seconds.is_some(), "a scene wait"),
                (self.match_timeout_seconds.is_some(), "a match timeout"),
            ],
            "anything but a watched match",
        )?;
        let output = self
            .output
            .clone()
            .ok_or_else(|| Failure::usage("expected the recording to write"))?;
        let layout = match self.input.clone() {
            Some(input) => {
                let (kind, bytes) = crate::kind::Kind::read(&input)?;
                match kind {
                    crate::kind::Kind::Layout => parse_layout(&input, &bytes)?,
                    crate::kind::Kind::Fight => {
                        self.fight(&mechcore_document::fight::parse_yaml(&bytes).map_err(
                            |error| Failure::refused(format!("{}: {error}", input.display())),
                        )?)?
                    }
                    crate::kind::Kind::Grbr => return self.replay(input, output),
                    other => {
                        return Err(Failure::refused(format!(
                            "game record fights a layout, a fight's layout or a replay's \
                             round, not a {} file",
                            other.name()
                        )));
                    }
                }
            }
            None => return self.scene(output),
        };
        refuse_given(
            &[
                (self.round.is_some(), "a round"),
                (self.video.is_some(), "a video"),
                (self.speed_up.is_some(), "a speed-up"),
            ],
            "a layout fought without a scene",
        )?;
        Ok(Record::Layout {
            layout,
            seed: self.seed,
            output,
            instrument: self.instrument,
        })
    }

    /// The layout a fight document states, fought with the seed it states:
    /// what it records is the fight the document is the result of, so one
    /// file both records a fixture and verifies it.
    fn fight(&self, fight: &mechcore_document::Fight) -> Result<Value, Failure> {
        refuse_given(
            &[(self.seed.is_some(), "a seed")],
            "a fight, which states the seed its result is one of",
        )?;
        serde_json::to_value(mechcore_document::fight::project(fight))
            .map_err(|error| Failure::failed(format!("cannot write the fight's layout: {error}")))
    }

    fn watch(self) -> Result<Record, Failure> {
        refuse_given(
            &[
                (self.input.is_some(), "an input"),
                (self.output.is_some(), "an output"),
                (self.seed.is_some(), "a seed"),
                (self.round.is_some(), "a round"),
                (self.video.is_some(), "a video"),
                (self.speed_up.is_some(), "a speed-up"),
                (!self.instrument.is_empty(), "an instrument"),
            ],
            "a watched match, which the server makes",
        )?;
        Ok(Record::Watch {
            output_dir: self.output_dir,
            wait_for_scene_seconds: self
                .wait_for_scene_seconds
                .unwrap_or(mechcore_protocol::DEFAULT_WATCH_SCENE_WAIT_SECONDS),
            match_timeout_seconds: self
                .match_timeout_seconds
                .unwrap_or(mechcore_protocol::DEFAULT_WATCH_MATCH_TIMEOUT_SECONDS),
        })
    }

    fn replay(self, grbr: PathBuf, output: PathBuf) -> Result<Record, Failure> {
        refuse_given(
            &[
                (self.seed.is_some(), "a seed"),
                (self.video.is_some(), "a video"),
                (self.speed_up.is_some(), "a speed-up"),
            ],
            "a replay's round, which the replay seeds and stages",
        )?;
        let round = self.round.ok_or_else(|| {
            Failure::usage("a replay holds several rounds; name one with --round")
        })?;
        Ok(Record::Replay {
            grbr,
            round,
            output,
            instrument: self.instrument,
        })
    }

    fn scene(self, output: PathBuf) -> Result<Record, Failure> {
        refuse_given(
            &[
                (self.seed.is_some(), "a seed"),
                (self.round.is_some(), "a round"),
            ],
            "the current scene, which was staged with both",
        )?;
        Ok(Record::Scene {
            output,
            video: self.video,
            speed_up: self.speed_up,
            instrument: self.instrument,
        })
    }
}

impl Record {
    /// Reads `game record [<input>] <out.mcfr>` or `game record --watch`.
    fn read(arguments: &mut Args) -> Result<Self, Failure> {
        let mut request = RecordRequest {
            watch: arguments.flag("--watch")?,
            seed: seed(arguments)?,
            round: arguments.parsed::<i32>("--round", "a round number")?,
            video: arguments.value("--video")?.map(PathBuf::from),
            speed_up: arguments.flag("--no-speed-up")?.then_some(false),
            instrument: instrument(arguments)?,
            output_dir: arguments.value("--output-dir")?.map(PathBuf::from),
            wait_for_scene_seconds: arguments
                .parsed::<u64>("--wait-for-scene-seconds", "a number of seconds")?,
            match_timeout_seconds: arguments
                .parsed::<u64>("--match-timeout-seconds", "a number of seconds")?,
            ..RecordRequest::default()
        };
        let mut operands = arguments.operands()?.into_iter().map(PathBuf::from);
        match (operands.next(), operands.next()) {
            (None, _) => {}
            (Some(output), None) => request.output = Some(output),
            (Some(_), Some(_)) => return Err(Failure::usage(FILES_ARE_CONVERTED)),
        }
        request.decide()
    }

    /// Records it, replacing existing outputs when `force` says so.
    ///
    /// # Errors
    ///
    /// Returns a refusal for whatever the game or the session would not do.
    pub(crate) async fn run(self, session: &Session, force: bool) -> Result<Value, Failure> {
        match self {
            Self::Scene {
                output,
                video,
                speed_up,
                instrument,
            } => session
                .record_fight(output, video, speed_up, force, instrument)
                .await
                .map_err(|value| Failure::refused(crate::shell::render(&value))),
            Self::Layout {
                layout,
                seed,
                output,
                instrument,
            } => session
                .record_layout(layout, seed, output, force, instrument)
                .await
                .map_err(refusal),
            Self::Replay {
                grbr,
                round,
                output,
                instrument,
            } => session
                .record_replay_round(grbr, round, output, force, instrument)
                .await
                .map_err(refusal),
            Self::Watch {
                output_dir,
                wait_for_scene_seconds,
                match_timeout_seconds,
            } => session
                .record_watch_replay(output_dir, wait_for_scene_seconds, match_timeout_seconds)
                .await
                .map_err(refusal),
        }
    }
}

fn parse_layout(path: &std::path::Path, bytes: &[u8]) -> Result<Value, Failure> {
    serde_yaml::from_slice(bytes)
        .map_err(|error| Failure::refused(format!("cannot parse {}: {error}", path.display())))
}

/// A seed is an operand of its own, so `--seed` names it rather than position.
fn seed(arguments: &mut Args) -> Result<Option<i32>, Failure> {
    arguments.parsed::<i32>("--seed", "a signed 32-bit integer")
}

/// `--instrument a,b`: the instrument channels a recording carries.
pub(crate) fn instrument(
    arguments: &mut Args,
) -> Result<Vec<mechcore_protocol::InstrumentChannel>, Failure> {
    let Some(list) = arguments.value("--instrument")? else {
        return Ok(Vec::new());
    };
    let mut channels = list
        .split(',')
        .map(|name| {
            serde_json::from_value(Value::String(name.trim().to_owned())).map_err(|_| {
                Failure::usage(format!(
                    "{name:?} is not an instrument channel; the channels are {}",
                    mechcore_protocol::InstrumentChannel::ALL
                        .map(mechcore_protocol::InstrumentChannel::as_str)
                        .join(", ")
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    channels.sort_unstable();
    channels.dedup();
    Ok(channels)
}

fn force(arguments: &mut Args) -> Result<bool, Failure> {
    Ok(arguments.flag("--force")? || arguments.flag("-f")?)
}

/// What the game would not do is the request's, not the environment's.
fn refusal(reason: String) -> Failure {
    Failure::refused(reason)
}

#[cfg(test)]
mod tests {
    use super::{Record, RecordRequest, force, seed};
    use crate::cli::Args;

    fn args(items: &[&str]) -> Args {
        Args::new(items.iter().map(|item| (*item).to_owned()))
    }

    /// `game record`'s options are independent of each other and of where
    /// they stand, and a repeated one is a mistake rather than the same answer.
    #[test]
    fn a_recording_reads_its_options_in_any_order() {
        let mut line = args(&[
            "--video",
            "/tmp/a.mov",
            "/tmp/a.mcfr",
            "--no-speed-up",
            "-f",
        ]);
        assert_eq!(
            line.value("--video").unwrap().as_deref(),
            Some("/tmp/a.mov")
        );
        assert!(force(&mut line).unwrap());
        assert_eq!(
            line.flag("--no-speed-up").unwrap().then_some(false),
            Some(false)
        );
        assert_eq!(
            line.path("a recording").unwrap().display().to_string(),
            "/tmp/a.mcfr"
        );
        line.finish().unwrap();

        let mut plain = args(&["/tmp/a.mcfr"]);
        assert!(!force(&mut plain).unwrap());
        assert_eq!(plain.flag("--no-speed-up").unwrap().then_some(false), None);

        assert!(args(&["--video"]).value("--video").is_err());
        assert!(force(&mut args(&["-f", "-f"])).is_err());
        assert!(
            args(&["--seed", "x"])
                .parsed::<i32>("--seed", "an integer")
                .is_err()
        );
        assert_eq!(seed(&mut args(&["--seed", "-17"])).unwrap(), Some(-17));
    }

    /// Launching is its own verb: an operation joins a game and never starts
    /// one, and attaching or detaching outlives no command. None of these
    /// calls reaches a game: a test that attached would take a running game
    /// from whatever holds it.
    #[test]
    fn only_launch_starts_a_game() {
        assert!(super::run(args(&["status", "--launch"])).is_err());
        assert!(super::run(args(&["attach"])).is_err());
        assert!(super::run(args(&["launch", "--windowed"])).is_err());
    }

    /// What `game record` records is its input's to say, and an option that
    /// belongs to another input is refused rather than ignored.
    #[test]
    fn a_recording_is_decided_by_its_input() {
        let output = || Some(std::path::PathBuf::from("/tmp/a.mcfr"));
        let directory = tempfile::tempdir().unwrap();
        let file = |name: &str, text: &str| {
            let path = directory.path().join(name);
            std::fs::write(&path, text).unwrap();
            Some(path)
        };
        let layout_file = file(
            "layout.yaml",
            "kind: layout\nround: 1\nblue:\n  units: []\nred:\n  units: []\n",
        );
        let scene = RecordRequest {
            output: output(),
            ..RecordRequest::default()
        };
        assert!(matches!(scene.decide(), Ok(Record::Scene { .. })));
        let layout = RecordRequest {
            input: layout_file.clone(),
            seed: Some(7),
            output: output(),
            ..RecordRequest::default()
        };
        assert!(matches!(
            layout.decide(),
            Ok(Record::Layout { seed: Some(7), .. })
        ));
        let watch = RecordRequest {
            watch: true,
            ..RecordRequest::default()
        };
        assert!(matches!(watch.decide(), Ok(Record::Watch { .. })));

        // A fight is fought as its layout, with the seed its result is one of.
        let fight = file(
            "fight.yaml",
            "kind: fight\nseed: 4242\nround: 1\nsource: game\nticks: 1\n\
             hash: 23:0000000000000000000000000000000000000000000000000000000000000000\n\
             blue:\n  units: [{name: marksman, index: 0, position: {x: 0, y: -50}, exp: 0/10/650}]\n\
             red:\n  core_damage: 3\n  units: [{name: arclight, index: 0, position: {x: 0, y: -50}}]\n",
        );
        let fought = RecordRequest {
            input: fight.clone(),
            output: output(),
            ..RecordRequest::default()
        };
        let Ok(Record::Layout { layout, seed, .. }) = fought.decide() else {
            panic!("a fight is recorded as its layout");
        };
        assert_eq!(seed, None);
        assert_eq!(layout["kind"], "layout");
        assert_eq!(layout["seed"], 4242);
        assert!(layout["red"].get("core_damage").is_none(), "{layout}");
        assert!(layout["blue"]["units"][0].get("exp").is_none(), "{layout}");

        for refused in [
            RecordRequest {
                input: fight,
                seed: Some(7),
                output: output(),
                ..RecordRequest::default()
            },
            RecordRequest {
                seed: Some(7),
                output: output(),
                ..RecordRequest::default()
            },
            RecordRequest {
                input: layout_file,
                round: Some(2),
                output: output(),
                ..RecordRequest::default()
            },
            RecordRequest {
                watch: true,
                output: output(),
                ..RecordRequest::default()
            },
            RecordRequest {
                output_dir: Some("/tmp".into()),
                output: output(),
                ..RecordRequest::default()
            },
            RecordRequest::default(),
        ] {
            assert!(refused.decide().is_err());
        }
    }
}
