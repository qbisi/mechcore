//! Runs one match between two persistent JSON Lines clients.
use crate::{
    cli::{Args, Failure, Outcome, Verdict},
    r#match::Game,
    turn::Side,
};
use mechcore_document::r#match::{Action, Rules, TurnActions};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{HashSet, VecDeque},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdin, ChildStdout, Command},
    task::JoinSet,
};

pub(crate) fn run(mut args: Args) -> Outcome {
    let command = |args: &mut Args, option| {
        args.value(option)?
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| Failure::usage(format!("expected {option} <command>")))
    };
    let blue = command(&mut args, "--blue")?;
    let red = command(&mut args, "--red")?;
    let seed = args.parsed("--seed", "a match seed")?;
    let map = args.parsed("--map", "a map ID")?;
    let rules = Rules {
        reinforcements: !args.flag("--no-reinforce")?,
        full_tech_loadout: args.flag("--full-tech")?,
    };
    let seconds = args
        .parsed::<f64>("--timeout", "seconds per problem")?
        .unwrap_or(100.0);
    args.finish()?;
    let duration = Duration::try_from_secs_f64(seconds)
        .map_err(|_| Failure::usage("timeout must be finite and nonnegative"))?;
    let timeout = (!duration.is_zero()).then_some(duration);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(io_error)?;
    let game = runtime.block_on(async {
        let names = [name(&blue, timeout).await?, name(&red, timeout).await?];
        let mut game = Arc::new(Game::arena(seed, map, names, rules)?);
        let mut bots = [Bot::start(&blue)?, Bot::start(&red)?];
        while !game.over() {
            let problem = game.problem_yaml()?;
            let mut answers = TurnActions::default();
            for side in Side::BOTH {
                let bot = &mut bots[side.seat()];
                let play = bot.answer(Arc::clone(&game), side, &problem);
                let actions = match timeout {
                    Some(bound) => match tokio::time::timeout(bound, play).await {
                        Ok(answer) => answer?,
                        Err(_) if game.round() > 0 => vec![Action::Concede],
                        Err(_) => {
                            return Err(Failure::failed(format!(
                                "{} timed out during opening",
                                side.name()
                            )));
                        }
                    },
                    None => play.await?,
                };
                let conceded = actions.contains(&Action::Concede);
                if side.red() {
                    answers.red = actions;
                } else {
                    answers.blue = actions;
                }
                if conceded {
                    break;
                }
            }
            Arc::make_mut(&mut game).advance_answers(answers)?;
        }
        for bot in &mut bots {
            bot.finish().await;
        }
        game.document()
    })?;
    print!("{game}");
    Ok(Verdict::Yes)
}

// Used directly by Result::map_err at each subprocess I/O boundary.
#[allow(clippy::needless_pass_by_value)]
fn io_error(error: std::io::Error) -> Failure {
    Failure::failed(error.to_string())
}

fn command(cmd: &str) -> Command {
    let mut command = Command::new("sh");
    #[cfg(unix)]
    command.process_group(0);
    command
        .args(["-c", cmd])
        .kill_on_drop(true)
        .stderr(Stdio::inherit());
    command
}

/// The command is a shell and may start descendants that inherit the protocol pipes.
struct ProcessGroup(Option<u32>);

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(id) = self.0.and_then(|id| i32::try_from(id).ok()) {
            // Each child starts its own process group. A negative PID addresses that group.
            unsafe { libc::kill(-id, libc::SIGKILL) };
        }
    }
}

async fn name(cmd: &str, timeout: Option<Duration>) -> Result<String, Failure> {
    let mut query = command(&format!("{cmd} --name"));
    query.stdin(Stdio::null()).stdout(Stdio::piped());
    let child = query.spawn().map_err(io_error)?;
    let mut group = ProcessGroup(child.id());
    let output = child.wait_with_output();
    let output = match timeout {
        Some(bound) => tokio::time::timeout(bound, output)
            .await
            .map_err(|_| Failure::failed("bot name query timed out"))?,
        None => output.await,
    }
    .map_err(io_error)?;
    group.0 = None;
    let name = std::str::from_utf8(&output.stdout)
        .map_err(|_| Failure::failed("bot name must be UTF-8"))?
        .trim();
    if !output.status.success() || name.is_empty() || name.chars().any(char::is_control) {
        return Err(Failure::failed(format!(
            "{cmd} --name must print one name and exit successfully"
        )));
    }
    Ok(name.into())
}

struct Bot {
    group: ProcessGroup,
    child: Child,
    input: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct Actions {
    blue: Vec<Value>,
    red: Vec<Value>,
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Act {
        id: u64,
        actions: Actions,
    },
    Simulate {
        id: u64,
        actions: Actions,
        #[serde(default)]
        until: Option<Value>,
        #[serde(default)]
        collect: Option<Value>,
    },
    Commit {
        id: u64,
        actions: Vec<Value>,
    },
}
impl Request {
    const fn id(&self) -> u64 {
        match self {
            Self::Act { id, .. } | Self::Simulate { id, .. } | Self::Commit { id, .. } => *id,
        }
    }
}

struct Cancel(Arc<AtomicBool>);
impl Drop for Cancel {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

impl Bot {
    fn start(cmd: &str) -> Result<Self, Failure> {
        let mut child = command(cmd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(io_error)?;
        Ok(Self {
            group: ProcessGroup(child.id()),
            input: child.stdin.take().expect("piped stdin"),
            lines: BufReader::new(child.stdout.take().expect("piped stdout")).lines(),
            child,
        })
    }

    async fn send(&mut self, value: &Value) -> Result<(), Failure> {
        let mut line = serde_json::to_vec(value).map_err(|e| Failure::failed(e.to_string()))?;
        line.push(b'\n');
        self.input.write_all(&line).await.map_err(io_error)?;
        self.input.flush().await.map_err(io_error)
    }

    async fn finish(&mut self) {
        let _ = self.input.shutdown().await;
        if tokio::time::timeout(Duration::from_secs(2), self.child.wait())
            .await
            .is_err()
        {
            drop(ProcessGroup(self.group.0.take()));
            let _ = self.child.kill().await;
        } else {
            self.group.0 = None;
        }
    }

    async fn answer(
        &mut self,
        game: Arc<Game>,
        side: Side,
        problem: &str,
    ) -> Result<Vec<Action>, Failure> {
        let round = game.round();
        self.send(&json!({"op":"problem", "round":round, "side":side, "match_yaml":problem}))
            .await?;
        let cancel = Cancel(Arc::new(AtomicBool::new(false)));
        let width = std::thread::available_parallelism().map_or(1, usize::from);
        let mut jobs = JoinSet::new();
        let mut queue = VecDeque::new();
        let mut ids = HashSet::new();
        loop {
            while jobs.len() < width {
                let Some(request) = queue.pop_front() else {
                    break;
                };
                let game = Arc::clone(&game);
                let cancel = Arc::clone(&cancel.0);
                jobs.spawn_blocking(move || trial(&game, request, &cancel));
            }
            tokio::select! {
                result = jobs.join_next(), if !jobs.is_empty() => {
                    let (id, result) = result.expect("pending job")
                        .map_err(|e| Failure::failed(e.to_string()))?;
                    self.reply(round, &json!(id), result).await?;
                }
                line = self.lines.next_line() => {
                    let line = line.map_err(io_error)?.ok_or_else(||
                        Failure::failed(format!("{} closed its output during round {round}", side.name())))?;
                    if let Some(actions) = self.requests(&line, &game, side, &mut ids, &mut queue).await? {
                        return Ok(actions);
                    }
                }
            }
        }
    }

    async fn requests(
        &mut self,
        line: &str,
        game: &Game,
        side: Side,
        ids: &mut HashSet<u64>,
        queue: &mut VecDeque<Request>,
    ) -> Result<Option<Vec<Action>>, Failure> {
        let round = game.round();
        let value = match serde_json::from_str::<Value>(line) {
            Ok(value) => value,
            Err(e) => {
                self.reply(round, &Value::Null, Err(Failure::usage(e.to_string())))
                    .await?;
                return Ok(None);
            }
        };
        let batch = value.is_array();
        let items = match value {
            Value::Array(items) => items,
            item => vec![item],
        };
        if items.is_empty() {
            self.reply(round, &Value::Null, Err(Failure::usage("empty batch")))
                .await?;
        }
        for item in items {
            let id = item["id"].clone();
            let request = match serde_json::from_value::<Request>(item) {
                Ok(request) => request,
                Err(e) => {
                    self.reply(round, &id, Err(Failure::usage(e.to_string())))
                        .await?;
                    continue;
                }
            };
            if !ids.insert(request.id()) {
                self.reply(round, &id, Err(Failure::usage("duplicate request id")))
                    .await?;
                continue;
            }
            if let Request::Commit { actions, .. } = request {
                if batch {
                    self.reply(round, &id, Err(Failure::usage("send commit separately")))
                        .await?;
                    continue;
                }
                match game.answer_actions(side, &actions) {
                    Ok((actions, _)) => {
                        self.reply(round, &id, Ok(json!({"committed":true})))
                            .await?;
                        return Ok(Some(actions));
                    }
                    Err(e) => self.reply(round, &id, Err(e)).await?,
                }
            } else {
                queue.push_back(request);
            }
        }
        Ok(None)
    }

    async fn reply(
        &mut self,
        round: i32,
        id: &Value,
        result: Result<Value, Failure>,
    ) -> Result<(), Failure> {
        let value = match result {
            Ok(result) => json!({"round":round,"id":id,"result":result}),
            Err(error) => json!({"round":round,"id":id,"error":error.object("arena")}),
        };
        self.send(&value).await
    }
}

fn trial(game: &Game, request: Request, cancel: &AtomicBool) -> (u64, Result<Value, Failure>) {
    let id = request.id();
    let (actions, simulation) = match request {
        Request::Act { actions, .. } => (actions, None),
        Request::Simulate {
            actions,
            until,
            collect,
            ..
        } => (actions, Some((until, collect))),
        Request::Commit { .. } => unreachable!("commit runs on its seat"),
    };
    let run = || {
        if cancel.load(Ordering::Relaxed) {
            return Err(Failure::refused("arena trial cancelled"));
        }
        let (blue, blue_state) = game.answer_actions(Side::Blue, &actions.blue)?;
        let (red, red_state) = game.answer_actions(Side::Red, &actions.red)?;
        if blue.contains(&Action::Concede) && red.contains(&Action::Concede) {
            return Err(Failure::refused("a round ends on its first concession"));
        }
        let Some((until, collect)) = simulation else {
            return Ok(json!({"state":game.trial_state(blue_state,red_state)?}));
        };
        let plan =
            mechcore_simulation::ObservationPlan::parse(until, collect).map_err(Failure::usage)?;
        if game.round() == 0 || blue.contains(&Action::Concede) || red.contains(&Action::Concede) {
            return Ok(json!({"stop":"no_fight","tick":0,"settled":false,"collect":{}}));
        }
        let layout = game
            .fight_layout(&TurnActions { blue, red })
            .map_err(Failure::refused)?;
        crate::arena_simulation::run(&layout, plan, cancel)
    };
    (id, run())
}
