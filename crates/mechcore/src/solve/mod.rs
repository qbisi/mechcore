//! `solve`: profile-driven decision search over a match round.
//!
//! Loads one deployment round from a match document, runs the plugin pipeline
//! configured by `--profile <profile.yaml>`, and emits the top-ranked legal
//! action sequences together with their decision trace.

mod model;
mod operators;
mod predictor;
mod profile;

use std::collections::BTreeSet;

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
    BranchId, KernelSeams, LineageStep, Operator, OpponentContext, PrunedCandidate, StageTrace,
    Valuation,
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
    score: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    delta_vs_baseline: Option<i32>,
    own_core_damage: i32,
    opponent_core_damage: i32,
    ticks: u32,
    winner: &'static str,
    normalized: bool,
    actions: Vec<Action>,
    lineage: Vec<LineageStep>,
}

#[derive(Serialize)]
struct TraceSummary {
    simulations: usize,
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
        opponent: opponent_plugin,
        operators,
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

    let opponent_deployed = opponent_plugin.resolve(&OpponentContext {
        economy,
        opponent_side,
        opponent_opening,
        opponent_actions,
        offers: turn.state.reinforce_offers.as_ref(),
    })?;

    let mut seams = KernelSeams::new(
        economy,
        turn.round,
        side,
        stated.map_id,
        stated.seed,
        &turn.state,
        own_actions,
        opponent_deployed,
    );

    let baseline = evaluate_baseline(&mut seams)?;
    let (beam, stages) = run_pipeline(&mut seams, &operators)?;
    let solutions = build_solutions(&seams, beam, baseline)?;

    let output = SolveOutput {
        schema: OUTPUT_SCHEMA,
        r#match: match_path.display().to_string(),
        round: turn.round,
        side: side.name(),
        profile: profile.name,
        baseline,
        solutions,
        trace: TraceSummary {
            simulations: seams.simulations,
            stages,
            pruned_top: seams.pruned_top,
        },
    };
    emit(&output, format)?;
    Ok(Verdict::Yes)
}

fn run_pipeline(
    seams: &mut KernelSeams<'_>,
    operators: &[Box<dyn Operator>],
) -> Result<(Vec<BranchId>, Vec<StageTrace>), Failure> {
    let own_opening = seams.own_opening();
    let root_unpinned: BTreeSet<i32> = own_opening
        .active_units()
        .filter(|unit| unit.movable(own_opening))
        .map(|unit| unit.index)
        .collect();
    let root = seams.arena.root(own_opening.clone(), root_unpinned);
    let mut beam = vec![root];
    let mut stages = Vec::with_capacity(operators.len());

    for (index, operator) in operators.iter().enumerate() {
        let in_beam = beam.len();
        let sims_before = seams.simulations;
        beam = operator.apply(seams, beam)?;
        let best_score = beam
            .iter()
            .filter_map(|&id| seams.arena.get(id).valuation.map(|val| val.core_delta))
            .max();
        stages.push(StageTrace {
            index,
            operator: operator.name().to_owned(),
            in_beam,
            out_beam: beam.len(),
            simulations: seams.simulations - sims_before,
            best_score,
        });
    }

    if beam
        .iter()
        .any(|&id| seams.arena.get(id).valuation.is_none())
    {
        seams.evaluate_batch(&beam)?;
    }
    Ok((beam, stages))
}

fn build_solutions(
    seams: &KernelSeams<'_>,
    beam: Vec<BranchId>,
    baseline: Option<Valuation>,
) -> Result<Vec<Solution>, Failure> {
    let mut solutions = Vec::with_capacity(beam.len());
    for (idx, id) in beam.into_iter().enumerate() {
        let branch = seams.arena.get(id);
        let valuation = branch.valuation.expect("evaluated before output");
        let (actions, normalized) = seams.compile_actions(branch)?;
        let lineage = seams.arena.lineage(id);
        solutions.push(Solution {
            rank: idx + 1,
            score: valuation.core_delta,
            delta_vs_baseline: baseline.map(|base| valuation.core_delta - base.core_delta),
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

fn evaluate_baseline(seams: &mut KernelSeams<'_>) -> Result<Option<Valuation>, Failure> {
    let effective: Vec<Action> = seams
        .own_actions
        .iter()
        .filter(|action| !matches!(action, Action::Concede))
        .cloned()
        .collect();
    let baseline_state = transition::deployed(
        seams.economy,
        seams.own_opening(),
        &effective,
        seams.side.red(),
        seams.offers(),
    )
    .map_err(|error| {
        Failure::refused(format!(
            "cannot replay {} baseline actions: {error}",
            seams.side.name()
        ))
    })?;
    seams.evaluate_cached(&baseline_state).map(Some)
}
