//! Interactive REPL frontend.
//!
//! The shell owns the game process when it launched one, and never when it
//! attached to one. Acquisition is declared up front with `--launch` or
//! `--attach`, or performed later from the prompt; a shell started with
//! neither is offline and refuses native commands. See `docs/session.md`.

use crate::acquire::{Mode, Ownership};
use crate::session::{RecordBattleInstrumentationParameters, Session};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const HELP: &str = "\
acquisition
  launch                          start a game and own it
  attach                          join a running game, leaving it to its owner
  detach                          release an attached game
native
  status                          current status snapshot
  start_test [seed]               create the layout-test Training Ground
  apply_layout <layout.yaml> [seed]
                                  create the test and reach the layout's round
  record_battle <out.mcfr> [--video <out.mov>] [--no-speed-up]
  record_replay_round <in.grbr> <round> <out.mcfr>
  toggle_fight                    start the current fight
  speed_up                        request battle speed-up
  quit_match                      leave the active test or replay
  quit_game                       shut the game down
shell
  help                            this list
  quit | exit                     leave the shell";

pub(crate) fn run(mode: Option<Mode>) -> Result<(), String> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("cannot create async runtime: {error}"))?
        .block_on(run_async(mode))
}

async fn run_async(mode: Option<Mode>) -> Result<(), String> {
    let session = Session::new();
    let monitor = tokio::spawn(Session::monitor_status(session.clone()));
    let mut ownership: Option<Ownership> = None;

    let mut out = tokio::io::stdout();
    if let Some(mode) = mode {
        match session.acquire(mode).await {
            Ok(owned) => {
                write(&mut out, &banner(&owned, &session)).await;
                ownership = Some(owned);
            }
            Err(failure) => {
                monitor.abort();
                return Err(failure);
            }
        }
    } else {
        write(
            &mut out,
            "offline shell; `launch` or `attach` to acquire a game, `help` for commands\n",
        )
        .await;
    }

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
    let mut words = line.split_whitespace();
    let command = words.next().unwrap_or_default();
    let arguments: Vec<&str> = words.collect();

    match command {
        "help" => {
            write(out, &format!("{HELP}\n")).await;
            return Flow::Continue;
        }
        "quit" | "exit" => return Flow::Quit,
        "launch" | "attach" => {
            let mode = if command == "launch" {
                Mode::Launch
            } else {
                Mode::Attach
            };
            if ownership.is_some() {
                write(out, "already holding a game; detach or quit first\n").await;
                return Flow::Continue;
            }
            match session.acquire(mode).await {
                Ok(owned) => {
                    write(out, &banner(&owned, session)).await;
                    *ownership = Some(owned);
                }
                Err(failure) => write(out, &format!("{failure}\n")).await,
            }
            return Flow::Continue;
        }
        "detach" => {
            match ownership.take() {
                None => write(out, "not holding a game\n").await,
                Some(owned @ Ownership::Owned { .. }) => {
                    // Refuse silently dropping a game we started: quitting is
                    // the explicit path, so the user cannot orphan it here.
                    *ownership = Some(owned);
                    write(
                        out,
                        "this shell owns the game; use quit_game then quit, or quit to shut it down\n",
                    )
                    .await;
                }
                Some(Ownership::Attached) => {
                    session.disconnect_adapter().await;
                    write(out, "detached; the game keeps running\n").await;
                }
            }
            return Flow::Continue;
        }
        _ => {}
    }

    if ownership.is_none() {
        write(
            out,
            &format!("{command} needs a game; run `launch` or `attach` first\n"),
        )
        .await;
        return Flow::Continue;
    }

    let result = native(command, &arguments, session).await;
    match result {
        Ok(value) => write(out, &format!("{}\n", render(&value))).await,
        Err(message) => write(out, &format!("error: {message}\n")).await,
    }
    Flow::Continue
}

async fn native(
    command: &str,
    arguments: &[&str],
    session: &Arc<Session>,
) -> Result<Value, String> {
    match command {
        "status" => Ok(session.current_status()),
        "start_test" => {
            let seed = match arguments {
                [] => None,
                [value] => Some(
                    value
                        .parse::<i32>()
                        .map_err(|_| format!("seed must be a signed 32-bit integer: {value}"))?,
                ),
                _ => return Err("usage: start_test [seed]".into()),
            };
            session.start_test(seed).await
        }
        "apply_layout" => {
            let (path, seed) = match arguments {
                [path] => (path, None),
                [path, seed] => (
                    path,
                    Some(
                        seed.parse::<i32>()
                            .map_err(|_| format!("seed must be a signed 32-bit integer: {seed}"))?,
                    ),
                ),
                _ => return Err("usage: apply_layout <layout.yaml> [seed]".into()),
            };
            let text = std::fs::read_to_string(path)
                .map_err(|error| format!("cannot read {path}: {error}"))?;
            let layout: Value = serde_yaml::from_str(&text)
                .map_err(|error| format!("cannot parse {path}: {error}"))?;
            session.apply_layout(layout, seed).await
        }
        "record_battle" => {
            let (output, video, speed_up) = parse_record_battle(arguments)?;
            session
                .record_battle(
                    output,
                    video,
                    speed_up,
                    None::<RecordBattleInstrumentationParameters>,
                )
                .await
                .map_err(|value| render(&value))
        }
        "record_replay_round" => {
            let [grbr, round, output] = arguments else {
                return Err("usage: record_replay_round <in.grbr> <round> <out.mcfr>".into());
            };
            let round = round
                .parse::<i32>()
                .map_err(|_| format!("round must be an integer: {round}"))?;
            session
                .record_replay_round(
                    PathBuf::from(grbr),
                    round,
                    PathBuf::from(output),
                    None,
                    None,
                )
                .await
        }
        "toggle_fight" => session.toggle_fight().await,
        "speed_up" => session.speed_up().await,
        "quit_match" => session.quit_match().await,
        "quit_game" => session.quit_game().await,
        other => Err(format!("unknown command {other}; try `help`")),
    }
}

type RecordBattleArgs = (PathBuf, Option<PathBuf>, Option<bool>);

/// `record_battle <out.mcfr> [--video <out.mov>] [--no-speed-up]`
///
/// The two options are independent: video and speed-up coexist.
fn parse_record_battle(arguments: &[&str]) -> Result<RecordBattleArgs, String> {
    const USAGE: &str = "usage: record_battle <out.mcfr> [--video <out.mov>] [--no-speed-up]";
    let [output, rest @ ..] = arguments else {
        return Err(USAGE.into());
    };
    let mut video = None;
    let mut speed_up = None;
    let mut rest = rest.iter();
    while let Some(argument) = rest.next() {
        match *argument {
            "--video" if video.is_none() => {
                video = Some(PathBuf::from(rest.next().ok_or(USAGE)?));
            }
            "--no-speed-up" if speed_up.is_none() => speed_up = Some(false),
            _ => return Err(USAGE.into()),
        }
    }
    Ok((PathBuf::from(output), video, speed_up))
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

fn render(value: &Value) -> String {
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

    #[test]
    fn record_battle_arguments_accept_only_the_documented_forms() {
        // Omitting the flag leaves the default to the adapter, which speeds up.
        assert_eq!(
            parse_record_battle(&["/tmp/a.mcfr"]).unwrap(),
            (PathBuf::from("/tmp/a.mcfr"), None, None)
        );
        assert_eq!(
            parse_record_battle(&["/tmp/a.mcfr", "--no-speed-up"]).unwrap(),
            (PathBuf::from("/tmp/a.mcfr"), None, Some(false))
        );
        assert_eq!(
            parse_record_battle(&["/tmp/a.mcfr", "--video", "/tmp/a.mov"]).unwrap(),
            (
                PathBuf::from("/tmp/a.mcfr"),
                Some(PathBuf::from("/tmp/a.mov")),
                None
            )
        );
        // The options are independent, in either order.
        assert_eq!(
            parse_record_battle(&["/tmp/a.mcfr", "--video", "/tmp/a.mov", "--no-speed-up"])
                .unwrap(),
            (
                PathBuf::from("/tmp/a.mcfr"),
                Some(PathBuf::from("/tmp/a.mov")),
                Some(false)
            )
        );
        assert!(parse_record_battle(&["/tmp/a.mcfr", "--no-speed-up", "--no-speed-up"]).is_err());
        assert!(parse_record_battle(&[]).is_err());
        assert!(parse_record_battle(&["/tmp/a.mcfr", "--video"]).is_err());
    }

    #[test]
    fn help_lists_every_native_command_the_shell_dispatches() {
        for command in [
            "status",
            "start_test",
            "apply_layout",
            "record_battle",
            "record_replay_round",
            "toggle_fight",
            "speed_up",
            "quit_match",
            "quit_game",
        ] {
            assert!(HELP.contains(command), "{command} missing from help");
        }
    }
}
