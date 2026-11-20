//! `solve`: composable plugin-driven decision search over a match round.
//!
//! Loads one deployment round from a match document, mounts the `services`,
//! `proposers`, `guards`, `filters`, `observers`, and named `operators`
//! declared in `--profile <profile.yaml>`, executes the recursive `pipeline`
//! AST, and emits the top-ranked legal action sequences with their trace.

mod hooks;
mod model;
mod operators;
mod predictor;
mod profile;
mod services;

use std::{collections::BTreeSet, sync::Arc};

use mechcore_document::{
    economy::Economy,
    r#match::{Action, Turn},
    opening, transition,
};
use serde::Serialize;

use crate::{
    cli::{Args, Failure, Outcome, Verdict, emit},
    kind::Kind,
    turn::Side,
};

use model::{
    BranchId, LineageStep, Operator, OpponentContext, Problem, PrunedCandidate, SolveContext,
    StageTrace, Valuation,
};
use profile::{BuiltProfile, Profile};

const OUTPUT_SCHEMA: &str = "mechcore.solve";

#[derive(Serialize)]
struct SolveOutput {
    schema: &'static str,
    r#match: String,
    round: i32,
    side: &'static str,
    profile: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    baseline: Option<Valuation>,
    solutions: Vec<Solution>,
    trace: TraceSummary,
}

#[derive(Serialize)]
struct Solution {
    rank: usize,
    score: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    delta_vs_baseline: Option<f64>,
    min_core_delta: i32,
    own_core_damage: f64,
    opponent_core_damage: f64,
    ticks: u32,
    winner: &'static str,
    normalized: bool,
    actions: Vec<Action>,
    lineage: Vec<LineageStep>,
}

#[derive(Serialize)]
struct TraceSummary {
    simulations: usize,
    sweeps: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_reason: Option<String>,
    stages: Vec<StageTrace>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pruned_top: Vec<PrunedCandidate>,
}

/// Entry point for `mechcore solve`.
///
/// # Errors
/// Returns usage, refusal, or I/O failures according to the CLI contract.
pub(crate) fn run(mut arguments: Args) -> Outcome {
    let format = arguments.format()?;
    let round_arg = arguments.value("--round")?;
    let round_requested =
        match round_arg {
            Some(raw) => Some(raw.parse::<i32>().map_err(|_| {
                Failure::usage(format!("--round expects an integer, found {raw:?}"))
            })?),
            None => None,
        };
    let side = match arguments.value("--side")? {
        Some(raw) => Side::parse(&raw)?,
        None => Side::Blue,
    };
    let profile_path = arguments
        .value("--profile")?
        .ok_or_else(|| Failure::usage("solve needs --profile <profile.yaml>"))?;
    let match_path = arguments.path("a match document")?;
    arguments.finish()?;

    let (kind, bytes) = Kind::read(&match_path)?;
    kind.require("solve")?;
    let stated = opening::stated(&bytes)
        .map_err(Failure::refused)?
        .ok_or_else(|| Failure::refused("expected a match document"))?;
    crate::r#match::warn_build(&stated.build);

    let turn = select_turn(&stated, round_requested)?;
    let profile = Profile::load(std::path::Path::new(&profile_path))?;
    let BuiltProfile {
        services,
        hooks,
        pipeline,
        trace,
    } = profile.build()?;

    let economy = Economy::embedded().map_err(Failure::failed)?;
    let opponent_side = side.other();
    let (opponent_opening, own_actions, opponent_actions) = match side {
        Side::Blue => (
            &turn.state.red,
            turn.actions.blue.as_slice(),
            turn.actions.red.as_slice(),
        ),
        Side::Red => (
            &turn.state.blue,
            turn.actions.red.as_slice(),
            turn.actions.blue.as_slice(),
        ),
    };

    let opponent_beliefs = services.opponent.resolve(&OpponentContext {
        economy,
        opponent_side,
        opponent_opening,
        opponent_actions,
        offers: turn.state.reinforce_offers.as_ref(),
    })?;

    let problem = Problem {
        economy,
        round: turn.round,
        side,
        map_id: stated.map_id,
        seed: stated.seed,
        opening: &turn.state,
        own_actions,
    };
    let mut ctx = SolveContext::new(&problem, opponent_beliefs, &services, hooks);

    let baseline = evaluate_baseline(&mut ctx)?;
    let beam = run_pipeline(&mut ctx, &pipeline)?;
    let solutions = build_solutions(&ctx, beam, baseline)?;
    let trace_data = trace
        .lock()
        .map_err(|_| Failure::failed("trace observer lock poisoned"))?
        .clone();

    let output = SolveOutput {
        schema: OUTPUT_SCHEMA,
        r#match: match_path.display().to_string(),
        round: turn.round,
        side: side.name(),
        profile: profile.name,
        baseline,
        solutions,
        trace: TraceSummary {
            simulations: ctx.stats.simulations,
            sweeps: ctx.stats.sweep,
            stop_reason: trace_data.stop_reason.or(ctx.stats.stop_reason),
            stages: trace_data.stages,
            pruned_top: trace_data.pruned_top,
        },
    };
    emit(&output, format)?;
    Ok(Verdict::Yes)
}

fn run_pipeline(
    ctx: &mut SolveContext<'_>,
    pipeline: &[Arc<dyn Operator>],
) -> Result<Vec<BranchId>, Failure> {
    let own_opening = ctx.problem.own_opening();
    let root_unpinned: BTreeSet<i32> = own_opening
        .active_units()
        .filter(|unit| unit.movable(own_opening))
        .map(|unit| unit.index)
        .collect();
    let root = ctx.arena.root(own_opening.clone(), root_unpinned);
    let mut beam = vec![root];

    for stage in pipeline {
        beam = ctx.run_operator(stage.as_ref(), beam)?;
    }

    if beam.iter().any(|&id| ctx.arena.get(id).valuation.is_none()) {
        ctx.evaluate_batch(std::slice::from_mut(&mut beam))?;
    }
    Ok(beam)
}

fn build_solutions(
    ctx: &SolveContext<'_>,
    beam: Vec<BranchId>,
    baseline: Option<Valuation>,
) -> Result<Vec<Solution>, Failure> {
    let mut solutions = Vec::with_capacity(beam.len());
    for (idx, id) in beam.into_iter().enumerate() {
        let branch = ctx.arena.get(id);
        let valuation = branch.valuation.expect("evaluated before output");
        let (actions, normalized) = ctx.compile_actions(branch)?;
        let lineage = ctx.arena.lineage(id);
        solutions.push(Solution {
            rank: idx + 1,
            score: valuation.score,
            delta_vs_baseline: baseline.map(|base| valuation.score - base.score),
            min_core_delta: valuation.min_core_delta,
            own_core_damage: valuation.own_core_damage,
            opponent_core_damage: valuation.opponent_core_damage,
            ticks: valuation.ticks,
            winner: valuation.winner,
            normalized,
            actions,
            lineage,
        });
    }
    Ok(solutions)
}

fn select_turn(stated: &opening::Stated, requested: Option<i32>) -> Result<&Turn, Failure> {
    if let Some(round) = requested {
        return stated
            .turns
            .iter()
            .find(|turn| turn.round == round)
            .ok_or_else(|| Failure::refused(format!("match holds no round {round}")));
    }
    stated
        .turns
        .iter()
        .rev()
        .find(|turn| !turn.actions.blue.is_empty() || !turn.actions.red.is_empty())
        .or_else(|| stated.turns.last())
        .ok_or_else(|| Failure::refused("match holds no deployment rounds"))
}

fn evaluate_baseline(ctx: &mut SolveContext<'_>) -> Result<Option<Valuation>, Failure> {
    let effective: Vec<Action> = ctx
        .problem
        .own_actions
        .iter()
        .filter(|action| !matches!(action, Action::Concede))
        .cloned()
        .collect();
    let baseline_state = transition::deployed(
        ctx.problem.economy,
        ctx.problem.own_opening(),
        &effective,
        ctx.problem.side.red(),
        ctx.problem.offers(),
    )
    .map_err(|error| {
        Failure::refused(format!(
            "cannot replay {} baseline actions: {error}",
            ctx.problem.side.name()
        ))
    })?;
    ctx.evaluate_cached(baseline_state)
}
