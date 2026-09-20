//! Interactive REPL frontend.
//!
//! A line is a command with the program name dropped, so what works here works
//! on a command line and in a run document. The shell owns the game process
//! when it launched one, and never when it attached to one.
//!
//! Acquiring the game is an operation rather than an option: a shell opens
//! offline and takes the game with `game launch` or `game attach`, each
//! carrying the level it claims at. A prompt is a session, and a session
//! acquires by saying so. See `docs/spec/mechcore/session.md`.

use crate::acquire::{Mode, Ownership};
use crate::cli::Args;
use crate::session::Session;
use serde_json::Value;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const HELP: &str = "\
A line is a command with `mechcore` dropped, so `doc verify x.yaml` here and
`mechcore doc verify x.yaml` outside are the same command.

the game
  game launch [--level 0-4]       start a game and own it
  game attach [--level 0-4]       join a running game, leaving it to its owner
  game detach                     release an attached game
  game status                     current status snapshot
  game start_test [--seed s] [--map-id id]
  game apply_layout <layout.yaml> [--seed s]
  game record_battle <out.mcfr> [--video <out.mov>] [--no-speed-up] [-f]
  game record_replay_round <in.grbr> <round> <out.mcfr> [-f]
  game record_watch_replay [--output-dir <dir>]
  game toggle_fight               start the current fight
  game speed_up                   request battle speed-up
  game quit_match                 leave the active test, replay or watch
  game quit_game                  shut the game down
offline
  doc verify | format | diff      documents on disk
  replay convert                  a native replay
  fight run | compare | verify    one fight
  man [<topic>]                   the manual this binary carries
shell
  help                            this list
  quit | exit                     leave the shell";

pub(crate) fn run() -> Result<(), String> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("cannot create async runtime: {error}"))?
        .block_on(run_async())
}

async fn run_async() -> Result<(), String> {
    let session = Session::new();
    let monitor = tokio::spawn(Session::monitor_status(session.clone()));
    let mut ownership: Option<Ownership> = None;

    let mut out = tokio::io::stdout();
    write(
        &mut out,
        "offline shell; `game launch` or `game attach` to acquire one, `help` for commands\n",
    )
    .await;

    // Never leave this function without running shut_down: an owned game is
    // only shut down here, and a dropped Child does not terminate it.
    let looped = repl(&session, &mut ownership, &mut out).await;
    let closed = session.release(ownership).await;
    monitor.abort();
    looped.and(closed)
}

async fn repl(
    session: &Arc<Session>,
    ownership: &mut Option<Ownership>,
    out: &mut tokio::io::Stdout,
) -> Result<(), String> {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    loop {
        write(out, &prompt(ownership.as_ref(), session)).await;
        let Some(line) = lines
            .next_line()
            .await
            .map_err(|error| format!("cannot read input: {error}"))?
        else {
            return Ok(());
        };
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match dispatch(line, session, ownership, out).await {
            Flow::Continue => {}
            Flow::Quit => return Ok(()),
        }
    }
}

enum Flow {
    Continue,
    Quit,
}

async fn dispatch(
    line: &str,
    session: &Arc<Session>,
    ownership: &mut Option<Ownership>,
    out: &mut tokio::io::Stdout,
) -> Flow {
    let mut words = line.split_whitespace().map(str::to_owned);
    let namespace = words.next().unwrap_or_default();
    let mut arguments = Args::new(words);

    match namespace.as_str() {
        "help" => {
            write(out, &format!("{HELP}\n")).await;
            Flow::Continue
        }
        "quit" | "exit" => Flow::Quit,
        "game" => {
            game(session, ownership, out, arguments).await;
            Flow::Continue
        }
        "doc" | "replay" | "fight" | "man" => {
            let outcome = match namespace.as_str() {
                "doc" => crate::doc::run(arguments),
                "replay" => crate::replay::run(arguments),
                "fight" => crate::fight::run(arguments),
                _ => crate::man::run(arguments),
            };
            if let Err(failure) = outcome {
                failure.write(&namespace);
            }
            Flow::Continue
        }
        other => {
            let _ = arguments.operands();
            write(
                out,
                &format!("{other} is not a command here; `help` lists them\n"),
            )
            .await;
            Flow::Continue
        }
    }
}

/// The game's own namespace, which is the only one holding a session.
async fn game(
    session: &Arc<Session>,
    ownership: &mut Option<Ownership>,
    out: &mut tokio::io::Stdout,
    mut arguments: Args,
) {
    let verb = match arguments.operand("a verb") {
        Ok(verb) => verb,
        Err(failure) => return failure.write("game"),
    };
    match verb.as_str() {
        "launch" | "attach" => {
            let level = match crate::acquire::level(&mut arguments) {
                Ok(level) => level,
                Err(failure) => return failure.write(&format!("game.{verb}")),
            };
            let mode = if verb == "launch" {
                Mode::Launch
            } else {
                Mode::Attach
            };
            if ownership.is_some() {
                write(out, "already holding a game; detach or quit first\n").await;
                return;
            }
            match session.acquire(mode, level).await {
                Ok(owned) => {
                    write(out, &banner(&owned, session)).await;
                    *ownership = Some(owned);
                }
                Err(failure) => write(out, &format!("{failure}\n")).await,
            }
        }
        "detach" => match ownership.take() {
            None => write(out, "not holding a game\n").await,
            Some(owned @ Ownership::Owned { .. }) => {
                // Refuse silently dropping a game we started: quitting is
                // the explicit path, so the user cannot orphan it here.
                *ownership = Some(owned);
                write(
                    out,
                    "this shell owns the game; use game quit_game then quit, \
                     or quit to shut it down\n",
                )
                .await;
            }
            Some(Ownership::Attached) => {
                session.disconnect_adapter().await;
                write(out, "detached; the game keeps running\n").await;
            }
        },
        _ if ownership.is_none() => {
            write(
                out,
                &format!("game {verb} needs a game; run `game launch` or `game attach` first\n"),
            )
            .await;
        }
        _ => match crate::game::operate(&verb, arguments, session).await {
            Ok(value) => write(out, &format!("{}\n", render(&value))).await,
            Err(failure) => failure.write("game"),
        },
    }
}

fn banner(ownership: &Ownership, session: &Arc<Session>) -> String {
    let endpoint = session.endpoint().display();
    match ownership.log() {
        Some(log) => format!(
            "launched game at {endpoint} (owned); quit will shut it down\n\
             game output: {}\n",
            log.display()
        ),
        None => format!("attached at {endpoint} (not owned); quit leaves it running\n"),
    }
}

/// The prompt doubles as the status display, so no polling command is needed.
fn prompt(ownership: Option<&Ownership>, session: &Arc<Session>) -> String {
    let Some(ownership) = ownership else {
        return "offline> ".into();
    };
    let status = session.current_status();
    let state = status
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let mut label = state.to_owned();
    if state == "training_ground" {
        if let Some(round) = status.get("round_count").and_then(Value::as_i64) {
            use std::fmt::Write;
            let _ = write!(label, " r{round}");
        }
        if status.get("fighting").and_then(Value::as_bool) == Some(true) {
            label.push_str(" fight");
        } else if status.get("deploying").and_then(Value::as_bool) == Some(true) {
            label.push_str(" deploy");
        }
    }
    if !ownership.is_owned() {
        label.push('*');
    }
    format!("{label}> ")
}

pub(crate) fn render(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

async fn write(out: &mut tokio::io::Stdout, text: &str) {
    let _ = out.write_all(text.as_bytes()).await;
    let _ = out.flush().await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn offline_prompt_does_not_claim_a_game() {
        let session = Session::new();
        assert_eq!(prompt(None, &session), "offline> ");
    }

    #[test]
    fn prompt_reports_training_phase_and_marks_attached_sessions() {
        let session = Session::new();
        session.publish(json!({
            "status": "training_ground", "round_count": 2,
            "deploying": true, "fighting": false
        }));
        assert_eq!(
            prompt(Some(&Ownership::Attached), &session),
            "training_ground r2 deploy*> "
        );
        session.publish(json!({
            "status": "training_ground", "round_count": 2,
            "deploying": false, "fighting": true
        }));
        assert_eq!(
            prompt(Some(&Ownership::Attached), &session),
            "training_ground r2 fight*> "
        );
    }

    /// Help lists what a line may say: every game operation, every offline
    /// namespace, and the shell's own two words.
    #[test]
    fn help_lists_every_command_a_line_may_be() {
        for command in crate::game::OPERATIONS {
            assert!(HELP.contains(command), "{command} missing from help");
        }
        for command in [
            "game launch",
            "game attach",
            "game detach",
            "doc",
            "replay",
            "fight",
            "man",
            "help",
            "quit",
        ] {
            assert!(HELP.contains(command), "{command} missing from help");
        }
    }
}
