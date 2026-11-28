//! Runs one match between two persistent JSON-RPC 2.0 bots via continuous `solve` dispatch.

use std::{num::NonZeroUsize, sync::Arc, time::Duration};

use mechcore_document::r#match::{Action, TurnActions};
use tokio::sync::Semaphore;

use crate::{
    cli::{Args, Failure, Outcome, Verdict},
    r#match::{Game, MatchOptions},
    rpc::{
        engine::StateContext,
        session::{BotProcess, SolveOutcome, WireLog},
    },
    turn::Side,
};

pub(crate) fn run(mut args: Args) -> Outcome {
    let log_path = args.value("--log")?;
    let parallel = args
        .parsed::<NonZeroUsize>("--parallel", "a positive number of concurrent trials")?
        .unwrap_or_else(|| std::thread::available_parallelism().unwrap_or(NonZeroUsize::MIN))
        .get();
    let verb = args.operand("an arena verb: new or load")?;
    if !matches!(verb.as_str(), "new" | "load") {
        return Err(Failure::usage(format!(
            "arena has no verb {verb:?}; expected new or load"
        )));
    }
    let command = |args: &mut Args, option| {
        args.value(option)?
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| Failure::usage(format!("expected {option} <command>")))
    };
    let blue = command(&mut args, "--blue")?;
    let red = command(&mut args, "--red")?;
    let seconds = args
        .parsed::<f64>("--timeout", "seconds per initialize or solve")?
        .unwrap_or(30.0);
    let duration = Duration::try_from_secs_f64(seconds)
        .map_err(|_| Failure::usage("timeout must be finite and nonnegative"))?;
    let timeout = (!duration.is_zero()).then_some(duration);
    let mut game = open_game(&verb, args)?;
    let log = log_path.as_deref().map(WireLog::open).transpose()?;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| Failure::failed(e.to_string()))?;

    let document = runtime.block_on(play_match(
        &mut game,
        &blue,
        &red,
        timeout,
        parallel,
        log.as_ref(),
    ))?;

    print!("{document}");
    Ok(Verdict::Yes)
}

async fn play_match(
    game: &mut Game,
    blue: &str,
    red: &str,
    timeout: Option<Duration>,
    parallel: usize,
    log: Option<&WireLog>,
) -> Result<String, Failure> {
    let mut bots = [
        BotProcess::start(blue, Side::Blue, log)?,
        BotProcess::start(red, Side::Red, log)?,
    ];

    let blue_name = match bots[0].initialize(parallel, timeout).await {
        Ok(name) => name,
        Err(err) => {
            kill_all(&mut bots).await;
            return Err(err);
        }
    };
    let red_name = match bots[1].initialize(parallel, timeout).await {
        Ok(name) => name,
        Err(err) => {
            kill_all(&mut bots).await;
            return Err(err);
        }
    };
    game.set_arena_names([blue_name, red_name]);
    let slots = Arc::new(Semaphore::new(parallel.max(1)));

    while !game.over() {
        if let Err(err) = play_round(game, &mut bots, timeout, parallel, &slots).await {
            kill_all(&mut bots).await;
            return Err(err);
        }
    }

    for bot in &mut bots {
        bot.finish().await;
    }
    game.document()
}

async fn play_round(
    game: &mut Game,
    bots: &mut [BotProcess<'_>; 2],
    timeout: Option<Duration>,
    parallel: usize,
    slots: &Arc<Semaphore>,
) -> Result<(), Failure> {
    let round = game.round();
    let ref_id = format!("arena:r{round}");
    let standalone = game.standalone_opening()?;
    let ctx = Arc::new(StateContext::from_standalone(standalone)?);
    let problem = game.problem_json(&ref_id)?;

    let mut answers = TurnActions::default();
    for side in Side::BOTH {
        let outcome = bots[side.seat()]
            .solve(
                &problem,
                &ref_id,
                Arc::clone(&ctx),
                timeout,
                parallel,
                Arc::clone(slots),
            )
            .await;
        let actions = match outcome {
            SolveOutcome::Failed(err) => return Err(err),
            SolveOutcome::TimedOut => {
                if round == 0 {
                    return Err(Failure::failed(format!(
                        "{} timed out during opening",
                        side.name()
                    )));
                }
                eprintln!(
                    "warning: {} timed out on round {round}; adjudicating as concede",
                    side.name()
                );
                vec![Action::Concede]
            }
            SolveOutcome::Answered { raw_actions, .. } => {
                match ctx.validate_side_actions("solve", side, &raw_actions) {
                    Ok((actions, _, _)) => actions,
                    Err(rpc_err) => {
                        if round == 0 {
                            return Err(Failure::failed(format!(
                                "{} returned illegal opening actions: {}",
                                side.name(),
                                rpc_err.failure.reason()
                            )));
                        }
                        let detail = rpc_err.failure.object("act");
                        eprintln!(
                            "warning: {} returned illegal actions on round {round} ({detail}); adjudicating as concede",
                            side.name()
                        );
                        vec![Action::Concede]
                    }
                }
            }
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

    game.advance_answers(answers)
}

async fn kill_all(bots: &mut [BotProcess<'_>; 2]) {
    for bot in bots {
        bot.kill_now().await;
    }
}

fn open_game(verb: &str, mut args: Args) -> Result<Game, Failure> {
    if verb == "new" {
        let seed = args.parsed("--seed", "a match seed")?;
        let map = args.parsed("--map", "a map ID")?;
        let max_round = args.parsed("--max-round", "a positive round limit")?;
        let base_core = args.parsed("--base-core", "a positive base reactor core")?;
        let options = MatchOptions {
            reinforce: !args.flag("--no-reinforce")?,
            full_tech_loadout: args.flag("--full-tech")?,
            max_round,
            base_core,
        };
        args.finish()?;
        Game::arena_new(seed, map, options)
    } else {
        let round = args
            .parsed::<i32>("--round", "a round number")?
            .unwrap_or(0);
        let max_round = args.parsed("--max-round", "a positive round limit")?;
        let base_core = args.parsed("--base-core", "a positive base reactor core")?;
        let path = args.path("a source match document")?;
        args.finish()?;
        Game::arena_load(&path, round, max_round, base_core)
    }
}
