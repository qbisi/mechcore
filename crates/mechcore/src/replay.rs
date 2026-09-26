use std::fs;

use serde::Serialize;

use crate::cli::{Args, Failure, Format, Outcome, Verdict};

/// How many unequal leaves to name before counting the rest.
const FAILURES_SHOWN: usize = 5;

/// Dispatches one of the namespace's verbs.
///
/// # Errors
///
/// Returns a usage failure for a verb this namespace does not hold, and
/// whatever the verb returns otherwise.
pub(crate) fn run(mut arguments: Args) -> Outcome {
    match arguments.operand("a verb: convert")?.as_str() {
        "convert" => convert(arguments),
        other => Err(Failure::usage(format!(
            "replay has no verb {other:?}; it has convert"
        ))),
    }
}

/// What a conversion wrote, and how much of it the rules predict.
#[derive(Serialize)]
struct ConvertReport {
    schema: &'static str,
    battle: String,
    map_id: i32,
    seed: i32,
    rounds: usize,
    actions: usize,
    coverage: mechcore_document::coverage::Counts,
    #[serde(skip_serializing_if = "Option::is_none")]
    reinforcement_deal: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    unequal: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    further_unequal: Option<usize>,
}

/// Converts between a replay and a document, in the direction the source
/// names: a `.grbr` replay becomes a battle document, and a layout becomes a
/// `.grbr` replay the game fights.
///
/// The source is read, never written, and the destination is refused when it
/// already exists: a conversion that silently replaced a file would make the
/// document's provenance unrecoverable.
fn convert(mut arguments: Args) -> Outcome {
    let format = arguments.format()?;
    let force = arguments.flag("--force")?;
    let seed = arguments.parsed::<i32>("--seed", "a signed 32-bit integer")?;
    let source = arguments.path("a replay or a layout to read")?;
    let destination = arguments.path("a battle document or a replay to write")?;
    arguments.finish()?;
    if !force && destination.exists() {
        return Err(Failure::refused(format!(
            "{} already exists; pass --force to replace it",
            destination.display()
        )));
    }
    let is_grbr = |path: &std::path::Path| {
        path.extension()
            .is_some_and(|extension| extension == "grbr")
    };
    match (is_grbr(&source), is_grbr(&destination)) {
        (true, false) if seed.is_none() => {}
        (true, false) => {
            return Err(Failure::usage(
                "--seed belongs to a layout; a replay carries its own",
            ));
        }
        (false, true) => return layout_to_replay(format, seed, &source, &destination),
        _ => {
            return Err(Failure::usage(
                "convert reads a .grbr replay into a battle document, or a layout into a .grbr replay",
            ));
        }
    }

    let grbr = fs::read(&source)
        .map_err(|error| Failure::failed(format!("cannot read {}: {error}", source.display())))?;
    let economy = mechcore_document::economy::Economy::embedded().map_err(Failure::failed)?;
    // The document is measured as it was written, the way `verify` reads it.
    let mechcore_document::convert::Converted {
        battle,
        yaml,
        stated,
    } = mechcore_document::convert::document(&economy, &grbr).map_err(Failure::refused)?;
    let deal = mechcore_document::opening::verify(&economy, &stated)
        .and_then(|opening| mechcore_document::reinforcement::verify(&economy, &stated, &opening));
    let coverage = mechcore_document::coverage::measure(
        &economy,
        &stated,
        deal.as_ref().map_err(String::as_str),
    );
    fs::write(&destination, &yaml).map_err(|error| {
        Failure::failed(format!("cannot write {}: {error}", destination.display()))
    })?;

    let report = ConvertReport {
        schema: "mechcore.replay-convert-result.v1",
        battle: destination.display().to_string(),
        map_id: battle.map_id,
        seed: battle.seed,
        rounds: battle.turns.len(),
        actions: battle
            .turns
            .iter()
            .map(|turn| turn.actions.blue.len() + turn.actions.red.len())
            .sum(),
        coverage: coverage.total,
        reinforcement_deal: deal.err(),
        unequal: coverage
            .unequal
            .iter()
            .take(FAILURES_SHOWN)
            .map(|difference| {
                format!(
                    "round {} {} {}: predicted {} and holds {}",
                    difference.round,
                    difference.side,
                    difference.path,
                    difference.predicted.as_deref().unwrap_or("nothing"),
                    difference.recorded.as_deref().unwrap_or("nothing")
                )
            })
            .collect(),
        further_unequal: coverage
            .unequal
            .len()
            .checked_sub(FAILURES_SHOWN)
            .filter(|further| *further > 0),
    };
    if format == Format::Text {
        print_text(&report);
    } else {
        crate::cli::emit(&report, format)?;
    }
    Ok(Verdict::Yes)
}

/// What a layout was written as.
#[derive(Serialize)]
struct LayoutReplayReport {
    schema: &'static str,
    replay: String,
    map_id: i32,
    seed: i32,
    round: i32,
}

/// Writes a layout as a replay the game fights: one deployment round whose
/// snapshot is the layout, which `game record_replay_round` records at that
/// round. The layout is compiled first, so a layout the Training Ground
/// would refuse is refused here too.
fn layout_to_replay(
    format: Format,
    seed: Option<i32>,
    source: &std::path::Path,
    destination: &std::path::Path,
) -> Outcome {
    let text = fs::read_to_string(source)
        .map_err(|error| Failure::failed(format!("cannot read {}: {error}", source.display())))?;
    let layout: serde_json::Value = serde_yaml::from_str(&text)
        .map_err(|error| Failure::refused(format!("cannot parse {}: {error}", source.display())))?;
    let mut plan = mechcore_document::compile(&layout).map_err(Failure::refused)?;
    plan.seed = seed.or(plan.seed);
    let replay =
        mechcore_document::layout_replay::layout_replay(&plan, mechcore_document::game_build())
            .map_err(Failure::refused)?;
    fs::write(destination, replay).map_err(|error| {
        Failure::failed(format!("cannot write {}: {error}", destination.display()))
    })?;
    let report = LayoutReplayReport {
        schema: "mechcore.replay-convert-layout-result.v1",
        replay: destination.display().to_string(),
        map_id: plan
            .map_id
            .unwrap_or(mechcore_document::layout_replay::DEFAULT_MAP_ID),
        seed: plan.seed.unwrap_or_default(),
        round: plan.round,
    };
    if format == Format::Text {
        println!(
            "{} map {} seed {} round {}",
            report.replay, report.map_id, report.seed, report.round
        );
    } else {
        crate::cli::emit(&report, format)?;
    }
    Ok(Verdict::Yes)
}

/// The same report, as a person reads it.
fn print_text(report: &ConvertReport) {
    println!(
        "{} map {} seed {} rounds {} actions {}",
        report.battle, report.map_id, report.seed, report.rounds, report.actions
    );
    let counts = &report.coverage;
    println!(
        "  transitions: {} leaves equal, {} unequal, {} unimplemented, {} the fight's",
        counts.equal, counts.unequal, counts.unimplemented, counts.fight
    );
    if let Some(error) = &report.reinforcement_deal {
        println!("    reinforcement deal: {error}");
    }
    for difference in &report.unequal {
        println!("    {difference}");
    }
    if let Some(further) = report.further_unequal {
        println!("    and {further} more");
    }
}
