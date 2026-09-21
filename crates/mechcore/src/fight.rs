//! The `fight` namespace: one fight, simulated or compared.
//!
//! `run` simulates a layout, `outcome` reads what a recorded fight decided,
//! `stats` reads a unit's numbers and the corrections behind them, `buildings`
//! reads the towers and constructions standing in it, `compare` puts two
//! recordings side by side, and `verify` simulates a recording's own layout
//! again and compares the result with what the recording holds.

use std::path::{Path, PathBuf};

use mechcore_mcfr::McfrReader;
use mechcore_simulation::{SimulationComparison, compare_recording, simulate_layout};
use serde::Serialize;

use crate::cli::{Args, Failure, Format, Outcome, Verdict};
use crate::difference::{self, Selection};

/// Dispatches one of the namespace's verbs.
///
/// # Errors
///
/// Returns a usage failure for a verb this namespace does not hold, and
/// whatever the verb returns otherwise.
pub(crate) fn run(mut arguments: Args) -> Outcome {
    let verb = arguments.operand("a verb: run, outcome, stats, buildings, compare or verify")?;
    let outcome = match verb.as_str() {
        "run" => simulate(arguments),
        "outcome" => outcome(arguments),
        "stats" => stats(arguments),
        "buildings" => buildings(arguments),
        "compare" => compare_recordings(arguments),
        "verify" => verify(arguments),
        other => Err(Failure::usage(format!(
            "fight has no verb {other:?}; it has run, outcome, stats, buildings, compare \
             and verify"
        ))),
    };
    outcome.map_err(|failure| failure.at(format!("fight.{verb}")))
}

/// Answers what a recorded fight decided.
///
/// The five fields a fight decides are `battle.md`'s, and this reads a
/// recording for as much of them as it holds. What no rule and no recording
/// answers is named in `unresolved` rather than approximated, which is why the
/// verdict is no when anything is: the fight was read, and the answer is that
/// it does not settle the round.
fn outcome(mut arguments: Args) -> Outcome {
    let format = arguments.format()?;
    let recording = arguments.path("a recording of a fight")?;
    arguments.finish()?;
    let outcome = crate::outcome::read(&recording)?;
    let settled = outcome.unresolved.is_empty();
    crate::cli::emit(&outcome, format)?;
    Ok(settled.into())
}

/// Answers what a recording holds about a unit's numbers at one tick.
///
/// A correction is an input to a fight rather than something it decided, and
/// so is the number derived from it, which is why neither belongs in
/// `outcome`. Both halves are here because a capture reads them together.
fn stats(mut arguments: Args) -> Outcome {
    let format = arguments.format()?;
    let tick = arguments.parsed::<u32>("--tick", "a tick the recording holds")?;
    let recording = arguments.path("a recording of a fight")?;
    arguments.finish()?;
    let written = crate::stats::read(&recording, tick)?;
    crate::cli::emit(&written, format)?;
    Ok(Verdict::Yes)
}

/// Answers what is standing in a recording at one tick.
///
/// A construction is several objects rather than one, and a recording says
/// what each of them is without saying which construction released it, so this
/// reads the rows back against the layout the recording embeds.
fn buildings(mut arguments: Args) -> Outcome {
    let format = arguments.format()?;
    let tick = arguments.parsed::<u32>("--tick", "a tick the recording holds")?;
    let recording = arguments.path("a recording of a fight")?;
    arguments.finish()?;
    let standing = crate::buildings::read(&recording, tick)?;
    crate::cli::emit(&standing, format)?;
    Ok(Verdict::Yes)
}

/// Simulates one fight from a layout.
fn simulate(mut arguments: Args) -> Outcome {
    let format = arguments.format()?;
    let seed = arguments.parsed::<i32>("--seed", "a 32-bit integer")?;
    let output = arguments.value("--output")?.map(PathBuf::from);
    let layout = arguments.path("a layout to simulate")?;
    arguments.finish()?;
    let result = simulate_layout(&layout, output.as_deref(), seed)
        .map_err(|error| Failure::refused(error.to_string()))?;
    crate::cli::emit(&result, format)?;
    Ok(Verdict::Yes)
}

/// Compares two recordings of a fight.
fn compare_recordings(mut arguments: Args) -> Outcome {
    let format = arguments.format()?;
    let selection = Selection::of(
        arguments
            .value("--fields")?
            .map(|names| names.split(',').map(str::to_owned).collect::<Vec<_>>())
            .unwrap_or_default(),
    );
    let tick = arguments.parsed::<u32>("--tick", "a tick")?;
    let left = arguments.path("the recording on the left")?;
    let right = arguments.path("the recording on the right")?;
    arguments.finish()?;
    let (verdict, report) = compare(&left, &right, &selection, tick).map_err(Failure::refused)?;
    if format == Format::Text {
        print_comparison(&report);
    } else {
        crate::cli::emit(&report, format)?;
    }
    Ok(verdict.into())
}

/// Simulates each recording's own layout again, and compares the two.
fn verify(mut arguments: Args) -> Outcome {
    let format = arguments.format()?;
    let recordings: Vec<PathBuf> = arguments
        .operands()?
        .into_iter()
        .map(PathBuf::from)
        .collect();
    if recordings.is_empty() {
        return Err(Failure::usage("expected at least one recording.mcfr"));
    }
    let mut comparisons = Vec::with_capacity(recordings.len());
    for path in recordings {
        let recording = McfrReader::open(&path)
            .map_err(|error| Failure::failed(format!("cannot open {}: {error}", path.display())))?;
        let comparison = compare_recording(&recording)
            .map_err(|error| Failure::refused(format!("{}: {error}", path.display())))?;
        comparisons.push(CaseComparison {
            recording_path: path.display().to_string(),
            comparison,
        });
    }
    let equal = comparisons
        .iter()
        .all(|comparison| comparison.comparison.equal);
    let report = VerifyReport {
        schema: "mechcore.fight-verify-result.v1",
        equal,
        comparisons,
    };
    crate::cli::emit(&report, format)?;
    Ok(equal.into())
}

#[derive(Serialize)]
struct VerifyReport {
    schema: &'static str,
    equal: bool,
    comparisons: Vec<CaseComparison>,
}

#[derive(Serialize)]
struct CaseComparison {
    recording_path: String,
    #[serde(flatten)]
    comparison: SimulationComparison,
}

/// Compare two recordings, returning the verdict and the structured report.
///
/// Shared with `mechcore run`, whose `fight.compare` step asserts on the same
/// fields `fight compare` prints. The physics verdict comes from the stored
/// tick hashes, as it always has; `fields` then says, group by group, where
/// the two recordings differ, and `at` explains one tick of it: the first
/// divergence of the selected groups, or the tick asked for.
///
/// The verdict is the physics layer's unless groups are selected, in which
/// case it is whether those groups agree on every tick both recordings hold.
pub(crate) fn compare(
    left_path: &Path,
    right_path: &Path,
    selection: &Selection,
    tick: Option<u32>,
) -> Result<(bool, serde_json::Value), String> {
    let left = McfrReader::open(left_path).map_err(|error| error.to_string())?;
    let right = McfrReader::open(right_path).map_err(|error| error.to_string())?;
    let first_divergence = left
        .first_divergence(&right)
        .map_err(|error| error.to_string())?;
    if first_divergence.is_none()
        && left.hashes().physics_result_hash != right.hashes().physics_result_hash
    {
        return Err(
            "physics result hashes differ although every stored physics tick hash matches".into(),
        );
    }
    let fields = difference::fields(&left, &right, selection)?;
    let at = tick
        .or_else(|| fields.first_divergence())
        .map(|tick| difference::detail(&left, &right, tick, selection))
        .transpose()?;
    let equal = first_divergence.is_none();
    let verdict = if selection.is_everything() {
        equal
    } else {
        fields.equal()
    };
    let report = CompareReport {
        schema: "mechcore.fight-compare-result.v2",
        equal,
        content_equal: left.hashes().content_result_hash == right.hashes().content_result_hash,
        left: RecordingSummary {
            physics_result_hash: &left.hashes().physics_result_hash,
            content_result_hash: &left.hashes().content_result_hash,
            tick_count: left.tick_count(),
        },
        right: RecordingSummary {
            physics_result_hash: &right.hashes().physics_result_hash,
            content_result_hash: &right.hashes().content_result_hash,
            tick_count: right.tick_count(),
        },
        first_divergence,
        compared_ticks: fields.compared_ticks,
        fields_equal: fields.equal(),
        fields: fields.nested(),
        at,
    };
    let report = serde_json::to_value(&report)
        .map_err(|error| format!("cannot serialize comparison: {error}"))?;
    Ok((verdict, report))
}

/// The same report, as a person reads it: the verdicts, each differing group
/// with the ticks it differs on, and the explained tick.
fn print_comparison(report: &serde_json::Value) {
    let agreed = |value: &serde_json::Value| {
        if value.as_bool() == Some(true) {
            "equal"
        } else {
            "different"
        }
    };
    println!(
        "physics {}{}, content {}, {} ticks compared ({} left, {} right)",
        agreed(&report["equal"]),
        report["first_divergence"]
            .as_u64()
            .map_or(String::new(), |tick| format!(" from t{tick}")),
        agreed(&report["content_equal"]),
        report["compared_ticks"],
        report["left"]["tick_count"],
        report["right"]["tick_count"],
    );
    let mut groups = Vec::new();
    collect_groups("", &report["fields"], &mut groups);
    groups.sort_by_key(|(group, first, _, _)| (*first, group.clone()));
    if groups.is_empty() {
        println!("every selected field agrees on every tick");
    }
    for (group, first, last, count) in groups {
        let span = if first == last {
            format!("t{first}")
        } else {
            format!("t{first}..t{last}")
        };
        println!("  {group:<48} {span:<12} {count} tick(s)");
    }
    let at = &report["at"];
    if at.is_null() {
        return;
    }
    println!("\nat t{}:", at["tick"]);
    for shown in at["differences"].as_array().into_iter().flatten() {
        println!(
            "  {} {}: {} | {}",
            shown["object"].as_str().unwrap_or_default(),
            shown["field"].as_str().unwrap_or_default(),
            shown["left"].as_str().unwrap_or_default(),
            shown["right"].as_str().unwrap_or_default(),
        );
    }
    if let Some(further) = at["further"].as_u64() {
        println!("  and {further} more");
    }
    if let Some(references) = at["references"]
        .as_object()
        .filter(|found| !found.is_empty())
    {
        println!("named:");
        for (name, sides) in references {
            println!(
                "  {name}: {} | {}",
                sides["left"].as_str().unwrap_or_default(),
                sides["right"].as_str().unwrap_or_default()
            );
        }
    }
    for side in ["left", "right"] {
        println!("{side} events:");
        for line in at["events"][side].as_array().into_iter().flatten() {
            println!("  {}", line.as_str().unwrap_or_default());
        }
    }
}

/// Flattens the nested `fields` map back into `(group, first, last, ticks)`.
fn collect_groups(path: &str, node: &serde_json::Value, out: &mut Vec<(String, u64, u64, u64)>) {
    let Some(fields) = node.as_object() else {
        return;
    };
    if let (Some(first), Some(last), Some(count)) = (
        fields
            .get("first_divergence")
            .and_then(serde_json::Value::as_u64),
        fields
            .get("last_divergence")
            .and_then(serde_json::Value::as_u64),
        fields
            .get("divergent_ticks")
            .and_then(serde_json::Value::as_u64),
    ) {
        out.push((path.to_owned(), first, last, count));
    }
    for (name, inner) in fields {
        if inner.is_object() {
            let at = if path.is_empty() {
                name.clone()
            } else {
                format!("{path}.{name}")
            };
            collect_groups(&at, inner, out);
        }
    }
}

#[derive(Serialize)]
struct CompareReport<'a> {
    schema: &'static str,
    equal: bool,
    content_equal: bool,
    left: RecordingSummary<'a>,
    right: RecordingSummary<'a>,
    first_divergence: Option<u32>,
    compared_ticks: u32,
    fields_equal: bool,
    fields: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    at: Option<difference::Detail>,
}

#[derive(Serialize)]
struct RecordingSummary<'a> {
    physics_result_hash: &'a str,
    content_result_hash: &'a str,
    tick_count: u32,
}
