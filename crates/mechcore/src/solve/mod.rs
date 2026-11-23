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
mod templates;

use std::{collections::BTreeSet, io::Write as _, path::Path, sync::Arc};

use mechcore_document::{
    economy::Economy,
    r#match::{Action, Offers, SideState},
    opening, transition,
};
use serde::Serialize;

use crate::{
    cli::{Args, Failure, Outcome, Verdict, emit},
    kind::Kind,
    play::{SolveHtmlRequest, SolveHtmlVariant, SolveHtmlVariantKind},
    turn::Side,
};

use model::{
    BranchId, LineageStep, Operator, OpponentContext, Problem, PrunedCandidate, SolveContext,
    StageTrace,
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
    html: Option<String>,
    solutions: Vec<Solution>,
    trace: TraceSummary,
}

#[derive(Serialize)]
struct Solution {
    rank: usize,
    score: f64,
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

struct ParsedArgs {
    format: crate::cli::Format,
    round_requested: Option<i32>,
    side: Side,
    profile_path: String,
    html_output: Option<std::path::PathBuf>,
    force: bool,
    match_path: std::path::PathBuf,
}

fn parse_args(mut arguments: Args) -> Result<ParsedArgs, Failure> {
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
    let html_output = arguments.value("--html")?.map(std::path::PathBuf::from);
    let force = arguments.flag("--force")?;
    let match_path = arguments.path("a match document")?;
    arguments.finish()?;
    Ok(ParsedArgs {
        format,
        round_requested,
        side,
        profile_path,
        html_output,
        force,
        match_path,
    })
}

fn stage_html(path: &Path, force: bool) -> Result<tempfile::NamedTempFile, Failure> {
    if path.exists() {
        if !force {
            return Err(Failure::refused(format!(
                "{} already exists; pass --force to replace it",
                path.display()
            )));
        }
        if !path.is_file() {
            return Err(Failure::refused(format!(
                "{} exists and is not a file",
                path.display()
            )));
        }
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    tempfile::NamedTempFile::new_in(parent).map_err(|error| {
        Failure::failed(format!(
            "cannot create output beside {}: {error}",
            path.display()
        ))
    })
}

fn publish_html(
    mut file: tempfile::NamedTempFile,
    path: &Path,
    force: bool,
    html: &str,
) -> Result<(), Failure> {
    let failed = |error| Failure::failed(format!("cannot write {}: {error}", path.display()));
    file.write_all(html.as_bytes()).map_err(failed)?;
    let result = if force {
        file.persist(path)
    } else {
        file.persist_noclobber(path)
    };
    result.map(|_| ()).map_err(|error| {
        if !force && error.error.kind() == std::io::ErrorKind::AlreadyExists {
            Failure::refused(format!(
                "{} already exists; pass --force to replace it",
                path.display()
            ))
        } else {
            failed(error.error)
        }
    })
}

/// Entry point for `mechcore solve`.
///
/// # Errors
/// Returns usage, refusal, or I/O failures according to the CLI contract.
pub(crate) fn run(arguments: Args) -> Outcome {
    let ParsedArgs {
        format,
        round_requested,
        side,
        profile_path,
        html_output,
        force,
        match_path,
    } = parse_args(arguments)?;

    let html_file = html_output
        .as_deref()
        .map(|path| stage_html(path, force).map(|file| (path, file)))
        .transpose()?;

    let (kind, bytes) = Kind::read(&match_path)?;
    kind.require("solve")?;
    let stated = opening::stated(&bytes)
        .map_err(Failure::refused)?
        .ok_or_else(|| Failure::refused("expected a match document"))?;
    crate::r#match::warn_build(&stated.build);

    let turn_index = select_turn_index(&stated, round_requested)?;
    let turn = &stated.turns[turn_index];
    let (
        profile_name,
        BuiltProfile {
            services,
            hooks,
            pipeline,
            trace,
        },
    ) = load_profile(&profile_path)?;

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
        round: turn.round,
        seed: stated.seed,
        opponent_side,
        opponent_opening,
        opponent_actions,
        offers: turn.state.reinforce_offers.as_ref(),
        space: services.space.as_ref(),
        templates: services.templates.as_deref(),
    })?;
    let top_opponent_belief = html_output
        .is_some()
        .then(|| {
            opponent_beliefs
                .iter()
                .max_by(|a, b| a.weight.total_cmp(&b.weight))
                .cloned()
        })
        .flatten();

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

    let beam = run_pipeline(&mut ctx, &pipeline)?;
    let mut output = build_output(&ctx, &trace, &match_path, &profile_name, beam)?;
    if let Some((path, file)) = html_file {
        let html = render_solve_html(
            &problem,
            &stated,
            turn_index,
            opponent_actions,
            top_opponent_belief.as_ref(),
            &output,
        )?;
        publish_html(file, path, force, &html)?;
        output.html = Some(path.display().to_string());
    }
    emit(&output, format)?;
    Ok(Verdict::Yes)
}

fn build_output(
    ctx: &SolveContext<'_>,
    trace: &std::sync::Mutex<hooks::TraceBuffer>,
    match_path: &std::path::Path,
    profile_name: &str,
    beam: Vec<BranchId>,
) -> Result<SolveOutput, Failure> {
    let solutions = build_solutions(ctx, beam)?;
    let trace_data = trace
        .lock()
        .map_err(|_| Failure::failed("trace observer lock poisoned"))?
        .clone();

    Ok(SolveOutput {
        schema: OUTPUT_SCHEMA,
        r#match: match_path.display().to_string(),
        round: ctx.problem.round,
        side: ctx.problem.side.name(),
        profile: profile_name.to_owned(),
        html: None,
        solutions,
        trace: TraceSummary {
            simulations: ctx.stats.simulations,
            sweeps: ctx.stats.sweep,
            stop_reason: trace_data
                .stop_reason
                .or_else(|| ctx.stats.stop_reason.clone()),
            stages: trace_data.stages,
            pruned_top: trace_data.pruned_top,
        },
    })
}

fn load_profile(profile_path: &str) -> Result<(String, BuiltProfile), Failure> {
    let profile_file = std::path::Path::new(profile_path);
    let profile = Profile::load(profile_file)?;
    let base_dir = match profile_file.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => std::path::Path::new("."),
    };
    let built = profile.build(base_dir)?;
    Ok((profile.name, built))
}

fn run_pipeline(
    ctx: &mut SolveContext<'_>,
    pipeline: &[Arc<dyn Operator>],
) -> Result<Vec<BranchId>, Failure> {
    let own_opening = ctx.problem.own_opening();
    let root_unpinned = movable_unit_indices(own_opening);
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

fn build_solutions(ctx: &SolveContext<'_>, beam: Vec<BranchId>) -> Result<Vec<Solution>, Failure> {
    let mut solutions = Vec::with_capacity(beam.len());
    for (idx, id) in beam.into_iter().enumerate() {
        let branch = ctx.arena.get(id);
        let valuation = branch.valuation.expect("evaluated before output");
        let (actions, normalized) = ctx.compile_actions(branch)?;
        let lineage = ctx.arena.lineage(id);
        solutions.push(Solution {
            rank: idx + 1,
            score: valuation.score,
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

fn select_turn_index(stated: &opening::Stated, requested: Option<i32>) -> Result<usize, Failure> {
    if let Some(round) = requested {
        return stated
            .turns
            .iter()
            .position(|turn| turn.round == round)
            .ok_or_else(|| Failure::refused(format!("match holds no round {round}")));
    }
    stated
        .turns
        .iter()
        .rposition(|turn| !turn.actions.blue.is_empty() || !turn.actions.red.is_empty())
        .or_else(|| stated.turns.len().checked_sub(1))
        .ok_or_else(|| Failure::refused("match holds no deployment rounds"))
}

fn strip_concede(actions: &[Action]) -> Vec<Action> {
    actions
        .iter()
        .filter(|action| !matches!(action, Action::Concede))
        .cloned()
        .collect()
}

fn render_solve_html(
    problem: &Problem<'_>,
    stated: &opening::Stated,
    turn_index: usize,
    opponent_actions: &[Action],
    top_opponent_belief: Option<&model::OpponentHypothesis>,
    output: &SolveOutput,
) -> Result<String, Failure> {
    let turn = &stated.turns[turn_index];
    let side = problem.side;
    let open = turn_index + 1 == stated.turns.len() && stated.result.is_none();
    let own_recorded = strip_concede(problem.own_actions);
    let opp_recorded = strip_concede(opponent_actions);
    let include_baseline = !open || !own_recorded.is_empty();
    let (opp_replay, opponent_tag) = if open && opp_recorded.is_empty() {
        let belief = top_opponent_belief
            .ok_or_else(|| Failure::refused("no opponent hypothesis for solve HTML"))?;
        let actions = mechcore_document::normalize::deployment(
            problem.economy,
            problem.opponent_opening(),
            &belief.state,
            problem.offers(),
            side.other().red(),
        )
        .map_err(|error| {
            Failure::refused(format!(
                "cannot replay opponent belief {}: {error}",
                belief.label
            ))
        })?;
        (actions, Some(format!("belief: {}", belief.label)))
    } else {
        (opp_recorded, None)
    };

    let mut variants = Vec::with_capacity(usize::from(include_baseline) + output.solutions.len());
    if include_baseline {
        variants.push(SolveHtmlVariant {
            kind: SolveHtmlVariantKind::Baseline,
            own_actions: own_recorded,
        });
    }
    for sol in &output.solutions {
        variants.push(SolveHtmlVariant {
            kind: SolveHtmlVariantKind::Candidate { rank: sol.rank },
            own_actions: sol.actions.clone(),
        });
    }

    let match_stem = Path::new(&output.r#match).file_stem().map_or_else(
        || "match".to_owned(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    let opp_suffix = opponent_tag
        .map(|tag| format!(" · vs {tag}"))
        .unwrap_or_default();
    let title = format!(
        "{match_stem} · R{} {} solve ({}){opp_suffix}",
        turn.round,
        side.name(),
        output.profile
    );
    crate::play::solve_html(&SolveHtmlRequest {
        title: &title,
        economy: problem.economy,
        stated,
        turn_index,
        side,
        opponent_actions: &opp_replay,
        variants: &variants,
    })
}

/// Replays non-`concede` actions from `opening` and returns the deployed state
/// alongside the filtered action sequence.
pub(super) fn deploy_non_concede(
    economy: &Economy,
    opening: &SideState,
    actions: &[Action],
    red: bool,
    offers: Option<&Offers>,
    error_prefix: &str,
) -> Result<(SideState, Vec<Action>), Failure> {
    let effective = strip_concede(actions);
    let deployed = transition::deployed(economy, opening, &effective, red, offers)
        .map_err(|error| Failure::refused(format!("{error_prefix}: {error}")))?;
    Ok((deployed, effective))
}

/// Returns all active movable formation indices on `state`.
pub(super) fn movable_unit_indices(state: &SideState) -> BTreeSet<i32> {
    state
        .active_units()
        .filter(|unit| unit.movable(state))
        .map(|unit| unit.index)
        .collect()
}

#[cfg(test)]
mod html_tests {
    use super::*;

    #[test]
    fn publishing_respects_files_created_during_search() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("comparison.html");
        let file = stage_html(&path, false).unwrap();
        std::fs::write(&path, "another export").unwrap();
        assert!(publish_html(file, &path, false, "new export").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "another export");
        publish_html(stage_html(&path, true).unwrap(), &path, true, "new export").unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "new export");
    }
}
