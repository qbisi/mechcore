//! The `game` namespace: the running game process.
//!
//! Its verbs are the adapter's operations, under the names
//! `docs/spec/adapter/adapter.md` gives them, and one argument reader serves
//! both the shell and a one-shot command. How a process acquires the game is
//! `docs/spec/mechcore/session.md`; nothing here acquires one by itself.

use std::{path::PathBuf, sync::Arc};

use serde_json::Value;

use crate::acquire::Mode;
use crate::cli::{Args, Failure, Outcome, Verdict};
use crate::session::Session;

/// The verbs that act on a game, for the help the shell prints.
pub(crate) const OPERATIONS: &[&str] = &[
    "status",
    "start_test",
    "apply_layout",
    "record_battle",
    "record_replay_round",
    "record_watch_replay",
    "toggle_fight",
    "speed_up",
    "quit_match",
    "quit_game",
];

/// One operation, acquired for the length of the command.
///
/// A command is one operation and then an exit, so it joins a game somebody
/// else is keeping alive; there is nothing else it could do, which is why it
/// says so by running rather than by declaring it. Launching belongs to a
/// session that outlives one operation, which is the shell and a run document.
///
/// # Errors
///
/// Returns a usage failure for a verb this namespace does not hold or an
/// acquisition it cannot make, an unavailable failure when no game answers,
/// and whatever the operation refuses.
pub(crate) fn run(mut arguments: Args) -> Outcome {
    let verb = arguments.operand("a verb: status, apply_layout, record_battle, ...")?;
    one(&verb, arguments).map_err(|failure| failure.at(format!("game.{verb}")))
}

/// One operation, once the verb is known.
fn one(verb: &str, mut arguments: Args) -> Outcome {
    if matches!(verb, "launch" | "attach" | "detach") {
        return Err(Failure::usage(format!(
            "{verb} holds a game for longer than one command; acquire in \
             `mechcore shell` or a run document, and name the operation here"
        )));
    }
    let level = crate::acquire::level(&mut arguments)?;
    if arguments.flag("--launch")? {
        return Err(Failure::usage(
            "a command cannot outlive the game it launches; run it against a game \
             somebody is keeping alive, or launch one in `mechcore shell` or a run \
             document",
        ));
    }
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| Failure::failed(format!("cannot create async runtime: {error}")))?
        .block_on(attached(verb, arguments, level))
}

/// Attaches, runs the one operation, and leaves the game to its owner.
async fn attached(verb: &str, arguments: Args, level: u8) -> Outcome {
    let session = Session::new();
    let monitor = tokio::spawn(Session::monitor_status(session.clone()));
    let ownership = session
        .acquire(Mode::Attach, level)
        .await
        .map_err(Failure::unavailable);
    let answered = match ownership {
        Ok(ownership) => {
            let answer = operate(verb, arguments, &session).await;
            let released = session.release(Some(ownership)).await;
            answer.and_then(|value| released.map_err(Failure::failed).map(|()| value))
        }
        Err(failure) => Err(failure),
    };
    monitor.abort();
    let value = answered?;
    println!("{value}");
    Ok(Verdict::Yes)
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
            let text = std::fs::read_to_string(&path).map_err(|error| {
                Failure::failed(format!("cannot read {}: {error}", path.display()))
            })?;
            let layout: Value = serde_yaml::from_str(&text).map_err(|error| {
                Failure::refused(format!("cannot parse {}: {error}", path.display()))
            })?;
            session.apply_layout(layout, seed).await.map_err(refusal)
        }
        "record_battle" => {
            let video = arguments.value("--video")?.map(PathBuf::from);
            let force = force(&mut arguments)?;
            let speed_up = arguments.flag("--no-speed-up")?.then_some(false);
            let output = arguments.path("a recording to write")?;
            arguments.finish()?;
            session
                .record_battle(output, video, speed_up, force, None)
                .await
                .map_err(|value| Failure::refused(crate::shell::render(&value)))
        }
        "record_replay_round" => {
            let force = force(&mut arguments)?;
            let grbr = arguments.path("a replay to read")?;
            let round = arguments
                .operand("the round to record")?
                .parse::<i32>()
                .map_err(|_| Failure::usage("the round is an integer"))?;
            let output = arguments.path("a recording to write")?;
            arguments.finish()?;
            session
                .record_replay_round(grbr, round, output, None, force, None)
                .await
                .map_err(refusal)
        }
        "record_watch_replay" => {
            let output_dir = arguments.value("--output-dir")?.map(PathBuf::from);
            let wait = arguments
                .parsed::<u64>("--wait-for-scene-seconds", "a number of seconds")?
                .unwrap_or(mechcore_protocol::DEFAULT_WATCH_SCENE_WAIT_SECONDS);
            let timeout = arguments
                .parsed::<u64>("--match-timeout-seconds", "a number of seconds")?
                .unwrap_or(mechcore_protocol::DEFAULT_WATCH_MATCH_TIMEOUT_SECONDS);
            arguments.finish()?;
            session
                .record_watch_replay(output_dir, wait, timeout)
                .await
                .map_err(refusal)
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

/// A seed is an operand of its own, so `--seed` names it rather than position.
fn seed(arguments: &mut Args) -> Result<Option<i32>, Failure> {
    arguments.parsed::<i32>("--seed", "a signed 32-bit integer")
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
    use super::{force, seed};
    use crate::cli::Args;

    fn args(items: &[&str]) -> Args {
        Args::new(items.iter().map(|item| (*item).to_owned()))
    }

    /// `record_battle`'s options are independent of each other and of where
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

    /// Acquisition holds a game for longer than one command, so a command
    /// refuses to launch one and says where launching belongs.
    #[test]
    fn a_command_attaches_and_never_launches() {
        assert!(super::run(args(&["launch"])).is_err());
        assert!(super::run(args(&["status", "--launch"])).is_err());
        assert!(super::run(args(&["status"])).is_err());
    }
}
