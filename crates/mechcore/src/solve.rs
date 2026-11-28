//! `mechcore solve`: single-problem Bot solving over bidirectional JSON-RPC 2.0.

use std::{num::NonZeroUsize, sync::Arc, time::Duration};

use mechcore_document::{
    r#match::{Action, Match},
    standalone::{StandaloneState, problem_json},
};
use serde::Serialize;
use serde_json::Value;

use crate::{
    cli::{Args, Failure, Outcome, Verdict, emit},
    kind::Kind,
    rpc::{
        engine::StateContext,
        session::{BotProcess, SolveOutcome, WireLog},
    },
    turn::Side,
};

#[derive(Serialize)]
struct SolveResult {
    schema: &'static str,
    bot: String,
    side: Side,
    round: i32,
    actions: Vec<Action>,
    #[serde(skip_serializing_if = "Option::is_none")]
    meta: Option<Value>,
}

/// Runs `mechcore solve <match.yaml|state.yaml> --side blue|red --bot <command> ...`.
pub(crate) fn run(mut args: Args) -> Outcome {
    let format = args.format()?;
    let side_raw = args
        .value("--side")?
        .ok_or_else(|| Failure::usage("solve needs --side blue|red"))?;
    let side = Side::parse(&side_raw)?;
    let bot_cmd = args
        .value("--bot")?
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| Failure::usage("expected --bot <command>"))?;
    let requested_round = args.parsed::<i32>("--round", "a round number")?;
    let seconds = args
        .parsed::<f64>("--timeout", "seconds for initialize and solve")?
        .unwrap_or(30.0);
    let duration = Duration::try_from_secs_f64(seconds)
        .map_err(|_| Failure::usage("timeout must be finite and nonnegative"))?;
    let timeout = (!duration.is_zero()).then_some(duration);
    let parallel = args
        .parsed::<NonZeroUsize>("--parallel", "a positive number of worker threads")?
        .unwrap_or_else(|| std::thread::available_parallelism().unwrap_or(NonZeroUsize::MIN))
        .get();
    let log_path = args.value("--log")?;
    let path = args.path("a match or state document")?;
    args.finish()?;

    let (kind, bytes) = Kind::read(&path)?;
    kind.require("solve")?;

    let (target_round, state, problem, ref_id) = match kind {
        Kind::State => {
            if requested_round.is_some() {
                return Err(Failure::usage(
                    "--round only applies to match documents, not standalone state documents",
                ));
            }
            let standalone = StandaloneState::from_yaml_bytes(&bytes).map_err(Failure::refused)?;
            if standalone.is_terminal() {
                return Err(Failure::refused(format!(
                    "round {} is already terminal (reactor core <= 0)",
                    standalone.round
                )));
            }
            if !standalone.is_round_opening() {
                return Err(Failure::refused(
                    "solve only accepts round-opening state documents, not deployment intermediate states",
                ));
            }
            let ref_id = format!("solve:r{}", standalone.round);
            let problem = standalone
                .problem_json(Some(&ref_id))
                .map_err(Failure::refused)?;
            (standalone.round, standalone, problem, ref_id)
        }
        Kind::Match => prepare_match_problem(&bytes, requested_round)?,
        _ => unreachable!("kind table restricts solve inputs to match or state"),
    };

    let ctx = Arc::new(StateContext::from_standalone(state)?);
    let log = log_path.as_deref().map(WireLog::open).transpose()?;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| Failure::failed(e.to_string()))?;

    let result = runtime.block_on(run_bot_solve(
        &bot_cmd,
        side,
        target_round,
        &problem,
        &ref_id,
        Arc::clone(&ctx),
        timeout,
        parallel,
        log.as_ref(),
    ))?;

    emit(&result, format)?;
    Ok(Verdict::Yes)
}

#[allow(clippy::too_many_arguments)]
async fn run_bot_solve(
    bot_cmd: &str,
    side: Side,
    target_round: i32,
    problem: &Value,
    ref_id: &str,
    ctx: Arc<StateContext>,
    timeout: Option<Duration>,
    parallel: usize,
    log: Option<&WireLog>,
) -> Result<SolveResult, Failure> {
    let mut bot = BotProcess::start(bot_cmd, side, log)?;
    let bot_name = match bot.initialize(parallel, timeout).await {
        Ok(name) => name,
        Err(err) => {
            bot.kill_now().await;
            return Err(err);
        }
    };
    let slots = Arc::new(tokio::sync::Semaphore::new(parallel.max(1)));
    let outcome = bot
        .solve(problem, ref_id, Arc::clone(&ctx), timeout, parallel, slots)
        .await;
    let (raw_actions, meta) = match outcome {
        SolveOutcome::Answered { raw_actions, meta } => {
            bot.finish().await;
            (raw_actions, meta)
        }
        SolveOutcome::TimedOut => {
            bot.kill_now().await;
            return Err(Failure::failed(format!(
                "{} timed out during solve on round {target_round}",
                side.name()
            )));
        }
        SolveOutcome::Failed(err) => {
            bot.kill_now().await;
            return Err(err);
        }
    };

    let (actions, _, _) = ctx
        .validate_side_actions("solve", side, &raw_actions)
        .map_err(|rpc_err| {
            let mut refused = Failure::refused(rpc_err.failure.reason().to_owned());
            if let Some(details) = rpc_err.failure.object("solve").get("details").cloned() {
                refused = refused.with_details(details);
            }
            refused
        })?;

    Ok(SolveResult {
        schema: "mechcore.solve-result",
        bot: bot_name,
        side,
        round: target_round,
        actions,
        meta,
    })
}

fn prepare_match_problem(
    bytes: &[u8],
    requested_round: Option<i32>,
) -> Result<(i32, StandaloneState, Value, String), Failure> {
    let game: Match = mechcore_document::r#match::read(bytes)
        .map_err(Failure::refused)?
        .ok_or_else(|| Failure::refused("input is not a valid match document"))?;
    crate::r#match::warn_build(&game.game_build);

    let target_round = if let Some(r) = requested_round {
        if r == 0 {
            if !game.has_opening {
                return Err(Failure::refused("match has no round 0 opening state"));
            }
        } else if !game.turns.iter().any(|t| t.round == r) {
            return Err(Failure::refused(format!("this match has no round {r}")));
        }
        r
    } else {
        if game.result.is_some() {
            return Err(Failure::refused("cannot solve an already-finished match"));
        }
        if let Some(last) = game.turns.last() {
            let conceded = last.actions.blue.contains(&Action::Concede)
                || last.actions.red.contains(&Action::Concede);
            if conceded || last.state.blue.reactor_core <= 0 || last.state.red.reactor_core <= 0 {
                return Err(Failure::refused(format!(
                    "round {} is already terminal",
                    last.round
                )));
            }
            last.round
        } else if game.has_opening {
            0
        } else {
            return Err(Failure::refused("match holds no playable round"));
        }
    };

    let state = StandaloneState::from_match_round(&game, target_round).map_err(Failure::refused)?;
    if state.is_terminal() {
        return Err(Failure::refused(format!(
            "round {target_round} opens with a finished match (reactor core <= 0)"
        )));
    }
    let ref_id = format!("solve:r{target_round}");
    let problem = problem_json(&game, target_round, Some(&ref_id)).map_err(Failure::refused)?;
    Ok((target_round, state, problem, ref_id))
}
