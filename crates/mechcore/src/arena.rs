//! The `arena` namespace: matches played between programs.
//!
//! `docs/spec/mechcore/cli.md` is the contract. An arena deals a match, hands
//! one player blue and the other red, and answers each player's requests on
//! the side it was given, which `requests` answers exactly as `shell --json`
//! does. A player never opens the match document or its turn file, so what it
//! knows is what its own view gave it.

use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use mechcore_document::r#match::{Action, DEFAULT_DEPLOY_TIME};
use serde::Serialize;
use serde_json::Value;

use crate::{
    cli::{Args, Failure, Outcome, Verdict, emit},
    turn::Side,
};

/// How often a player's server looks up from its pipe, to see whether the
/// arena has stopped it.
const LOOK: Duration = Duration::from_millis(100);

/// How long a player has to leave by itself once its match is over, before it
/// is killed.
const GRACE: Duration = Duration::from_secs(2);

/// Dispatches the namespace's one verb.
///
/// # Errors
///
/// Returns a usage failure for a verb this namespace does not hold, and
/// whatever the verb returns otherwise.
pub(crate) fn run(mut arguments: Args) -> Outcome {
    let verb = arguments.operand("a verb: run")?;
    match verb.as_str() {
        "run" => play(arguments),
        other => Err(Failure::usage(format!(
            "arena has no verb {other:?}; it has run"
        ))),
    }
    .map_err(|failure| failure.at(format!("arena.{verb}")))
}

/// How a match is dealt and its players run, which every match of a batch
/// shares but its seed.
struct Rules {
    blue: String,
    red: String,
    map: Option<i32>,
    deploy_time: Option<i32>,
    request_timeout: Duration,
}

fn play(mut arguments: Args) -> Outcome {
    let format = arguments.format()?;
    let required = |name: &str, value: Option<String>| {
        value.ok_or_else(|| Failure::usage(format!("expected {name} <command>")))
    };
    let blue = required("--blue", arguments.value("--blue")?)?;
    let red = required("--red", arguments.value("--red")?)?;
    let matches = arguments.parsed::<u32>("--matches", "a number of matches")?;
    let seed = arguments.parsed::<i32>("--seed", "an integer")?;
    let map = arguments.parsed::<i32>("--map", "a map ID")?;
    let deploy_time = arguments.parsed::<i32>("--deploy-time", "a number of seconds")?;
    let request_timeout = arguments.parsed::<f64>("--request-timeout", "a number of seconds")?;
    let target = arguments.path("a match document, or the directory a batch writes into")?;
    arguments.finish()?;

    // A player that sits on one request for as long as a round deploys has
    // lost that round anyway, so a bound nobody chose is the round's.
    let request_timeout =
        request_timeout.unwrap_or_else(|| f64::from(deploy_time.unwrap_or(DEFAULT_DEPLOY_TIME)));
    let request_timeout = Duration::try_from_secs_f64(request_timeout).map_err(|_| {
        Failure::usage(format!(
            "--request-timeout {request_timeout} is not a number of seconds"
        ))
    })?;
    let rules = Rules {
        blue,
        red,
        map,
        deploy_time,
        request_timeout,
    };

    let Some(matches) = matches else {
        emit(&fight(&target, &rules, seed)?, format)?;
        return Ok(Verdict::Yes);
    };
    if matches == 0 {
        return Err(Failure::usage("--matches is at least one"));
    }
    fs::create_dir_all(&target)
        .map_err(|error| Failure::failed(format!("cannot create {}: {error}", target.display())))?;
    let mut summary = Summary::default();
    for at in 0..matches {
        let path = target.join(format!("match-{at:04}.yaml"));
        // A batch is a comparison rather than one match played again, so
        // each match is dealt from a seed of its own: the next one after a
        // named seed, or one drawn.
        let seed =
            seed.map(|seed| seed.wrapping_add(i32::try_from(at).unwrap_or(i32::MAX)) & i32::MAX);
        let played = fight(&path, &rules, seed)?;
        summary.count(&played);
        println!("{}", serde_json::to_string(&played).unwrap_or_default());
    }
    println!("{}", serde_json::to_string(&summary).unwrap_or_default());
    Ok(Verdict::Yes)
}

/// Deals one match, plays it to its end and answers how it ended.
fn fight(path: &Path, rules: &Rules, seed: Option<i32>) -> Result<Played, Failure> {
    // `match new` on a document that is there joins it, and an arena deals
    // the matches it plays rather than taking a seat in somebody's.
    if path.exists() {
        return Err(Failure::refused(format!(
            "{} exists; an arena deals the match it plays",
            path.display()
        )));
    }
    let document = path.display().to_string();
    let mut deal = vec!["new".to_owned(), document.clone()];
    for (name, value) in [
        ("--seed", seed),
        ("--map", rules.map),
        ("--deploy-time", rules.deploy_time),
    ] {
        if let Some(value) = value {
            deal.extend([name.to_owned(), value.to_string()]);
        }
    }
    let blue = crate::r#match::answer(Args::new(deal))?;
    let red = crate::r#match::answer(Args::new(["new".to_owned(), document]))?;
    if blue["side"] != "blue" || red["side"] != "red" {
        return Err(Failure::failed(
            "the deal handed out the sides out of order",
        ));
    }
    // The document path the deal wrote is the one every request names.
    let path = PathBuf::from(blue["match"].as_str().unwrap_or_default());

    let stop = Arc::new(AtomicBool::new(false));
    let (ended, endings) = mpsc::channel();
    let mut servers = Vec::new();
    for (side, command) in [(Side::Blue, &rules.blue), (Side::Red, &rules.red)] {
        let log = path.with_extension(format!("{}.log", side.name()));
        let child = start(command, &log)?;
        let server = Server {
            path: path.clone(),
            side,
            timeout: rules.request_timeout,
            stop: stop.clone(),
        };
        let ended = ended.clone();
        servers.push(std::thread::spawn(move || {
            let player = server.serve(child, &log);
            let _ = ended.send(side);
            player
        }));
    }
    drop(ended);

    // A player that leaves stops committing and the clock ends the match
    // against it, so the other plays on. The opening has no clock, and a
    // fight nothing resolves waits on a gap rather than on a player, so
    // either of those, or a match that is over, stops the arena's players.
    for _ in endings {
        let view = omniscient(&path)?;
        let phase = view["phase"].as_str().unwrap_or_default();
        if phase != "deploy" {
            stop.store(true, Ordering::SeqCst);
        }
    }
    let mut players = Vec::new();
    for server in servers {
        players.push(
            server
                .join()
                .map_err(|_| Failure::failed("a player's server panicked"))?,
        );
    }
    // Both players gone in a round being deployed leaves the clock, which an
    // operation settles once it has run.
    let mut view = omniscient(&path)?;
    while view["phase"] == "deploy" {
        let remaining = view["remaining"].as_f64().unwrap_or(0.0).max(0.0);
        std::thread::sleep(Duration::from_secs_f64(remaining) + LOOK);
        view = omniscient(&path)?;
    }
    let [blue, red] = [players.remove(0), players.remove(0)];
    Played::read(&path, &view, Players { blue, red })
}

/// Starts a player, with its standard error kept in a log beside the match.
fn start(command: &str, log: &Path) -> Result<Child, Failure> {
    let log_file = fs::File::create(log)
        .map_err(|error| Failure::failed(format!("cannot create {}: {error}", log.display())))?;
    Command::new("sh")
        .args(["-c", command])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(log_file)
        .spawn()
        .map_err(|error| Failure::failed(format!("cannot start {command:?}: {error}")))
}

/// Both sides of the match in full, which is the arena's view and no
/// player's.
fn omniscient(path: &Path) -> Result<Value, Failure> {
    crate::r#match::answer(Args::new([
        "show".to_owned(),
        path.display().to_string(),
        "--omniscient".to_owned(),
    ]))
}

/// Answers one player's requests on the side it was given.
struct Server {
    path: PathBuf,
    side: Side,
    timeout: Duration,
    stop: Arc<AtomicBool>,
}

impl Server {
    fn serve(&self, mut child: Child, log: &Path) -> Player {
        let ended = self.answer(&mut child);
        // Closing the pipe tells a player there is nothing more to ask, and
        // one that does not leave by itself is made to.
        drop(child.stdin.take());
        let since = Instant::now();
        let mut status = child.try_wait().ok().flatten();
        while status.is_none() && ended != Ended::TimedOut && since.elapsed() < GRACE {
            std::thread::sleep(LOOK);
            status = child.try_wait().ok().flatten();
        }
        if status.is_none() {
            let _ = child.kill();
            status = child.wait().ok();
        }
        Player {
            ended,
            code: status.and_then(|status| status.code()),
            log: log.display().to_string(),
        }
    }

    /// Answers requests until the player leaves, stops answering, the match
    /// is over or the arena stops it.
    fn answer(&self, child: &mut Child) -> Ended {
        let (Some(mut input), Some(output)) = (child.stdin.take(), child.stdout.take()) else {
            return Ended::Exited;
        };
        let (lines, requests) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else { break };
                if lines.send(line).is_err() {
                    break;
                }
            }
        });
        let stopped = || self.stop.load(Ordering::SeqCst);
        let mut asked_by = Instant::now() + self.timeout;
        let ended = loop {
            if stopped() {
                break Ended::Stopped;
            }
            let line = match requests.recv_timeout(LOOK) {
                Ok(line) => line,
                Err(mpsc::RecvTimeoutError::Disconnected) => break Ended::Exited,
                Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() >= asked_by => {
                    break Ended::TimedOut;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
            };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let answer = crate::requests::answer(&self.path, self.side, line, &stopped);
            if writeln!(input, "{answer}")
                .and_then(|()| input.flush())
                .is_err()
            {
                break Ended::Exited;
            }
            if answer["phase"] == "over" {
                break Ended::Over;
            }
            asked_by = Instant::now() + self.timeout;
        };
        child.stdin = Some(input);
        ended
    }
}

/// How a player's part in a match ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Ended {
    /// It was answered with the match over.
    Over,
    /// It closed its output, or stopped reading its input.
    Exited,
    /// It asked nothing for longer than the request timeout, and was killed.
    TimedOut,
    /// The arena stopped it: the match ended without it, or cannot go on.
    Stopped,
}

#[derive(Serialize)]
struct Player {
    ended: Ended,
    /// Its exit code, which a player killed by a signal has none of.
    code: Option<i32>,
    /// Where its standard error was kept.
    log: String,
}

#[derive(Serialize)]
struct Players {
    blue: Player,
    red: Player,
}

#[derive(Serialize)]
struct Cores {
    blue: i32,
    red: i32,
}

/// How one match ended.
#[derive(Serialize)]
struct Played {
    schema: &'static str,
    #[serde(rename = "match")]
    path: String,
    seed: i32,
    map_id: i32,
    /// The last round the match reached.
    round: i32,
    phase: String,
    reactor_core: Cores,
    /// The side that won, which a match that is not over, and one both sides
    /// lost in the same round, has none of.
    winner: Option<Side>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unresolved: Option<String>,
    players: Players,
}

impl Played {
    /// Reads how a match ended from its document, which is the record of it.
    fn read(path: &Path, view: &Value, players: Players) -> Result<Self, Failure> {
        let bytes = fs::read(path)
            .map_err(|error| Failure::failed(format!("cannot read {}: {error}", path.display())))?;
        let played = mechcore_document::r#match::read(&bytes)
            .map_err(Failure::failed)?
            .ok_or_else(|| Failure::failed(format!("{} is not a match", path.display())))?;
        let last = played.turns.last();
        let core = |side: Side| {
            last.map_or_else(
                || {
                    view.pointer(&format!("/sides/{}/position/reactor_core", side.name()))
                        .and_then(Value::as_i64)
                        .and_then(|core| i32::try_from(core).ok())
                        .unwrap_or_default()
                },
                |turn| match side {
                    Side::Blue => turn.state.blue.reactor_core,
                    Side::Red => turn.state.red.reactor_core,
                },
            )
        };
        let phase = view["phase"].as_str().unwrap_or_default().to_owned();
        // A side loses by its reactor core reaching zero or by giving up,
        // which is also how a side that ran out of time is written.
        let beaten = |side: Side| {
            let conceded = last.is_some_and(|turn| {
                let actions = match side {
                    Side::Blue => &turn.actions.blue,
                    Side::Red => &turn.actions.red,
                };
                actions.contains(&Action::Concede)
            });
            conceded || core(side) <= 0
        };
        let winner = match (phase.as_str(), beaten(Side::Blue), beaten(Side::Red)) {
            ("over", true, false) => Some(Side::Red),
            ("over", false, true) => Some(Side::Blue),
            _ => None,
        };
        Ok(Self {
            schema: "mechcore.arena",
            path: path.display().to_string(),
            seed: played.seed,
            map_id: played.map_id,
            round: last.map_or(0, |turn| turn.round),
            phase,
            reactor_core: Cores {
                blue: core(Side::Blue),
                red: core(Side::Red),
            },
            winner,
            unresolved: view["unresolved"].as_str().map(str::to_owned),
            players,
        })
    }
}

/// What a batch came to, counted from its matches.
#[derive(Default, Serialize)]
struct Summary {
    schema: &'static str,
    matches: u32,
    blue: u32,
    red: u32,
    /// Matches that are over with no winner.
    drawn: u32,
    /// Matches that stopped before they were over.
    unfinished: u32,
}

impl Summary {
    fn count(&mut self, played: &Played) {
        self.schema = "mechcore.arena-summary";
        self.matches += 1;
        match (played.phase.as_str(), played.winner) {
            (_, Some(Side::Blue)) => self.blue += 1,
            (_, Some(Side::Red)) => self.red += 1,
            ("over", None) => self.drawn += 1,
            _ => self.unfinished += 1,
        }
    }
}
