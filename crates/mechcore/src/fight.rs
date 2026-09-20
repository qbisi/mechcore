//! The `fight` namespace: one fight, simulated or compared.
//!
//! `run` simulates a layout, `compare` puts two recordings side by side, and
//! `verify` simulates a recording's own layout again and compares the result
//! with what the recording holds.

use std::path::{Path, PathBuf};

use mechcore_mcfr::{McfrReader, TickSlice};
use mechcore_simulation::{SimulationComparison, compare_recording, simulate_layout};
use serde::Serialize;

use crate::cli::{Args, Failure, Outcome, Verdict};

/// Dispatches one of the namespace's verbs.
///
/// # Errors
///
/// Returns a usage failure for a verb this namespace does not hold, and
/// whatever the verb returns otherwise.
pub(crate) fn run(mut arguments: Args) -> Outcome {
    match arguments
        .operand("a verb: run, compare or verify")?
        .as_str()
    {
        "run" => simulate(arguments),
        "compare" => compare_recordings(arguments),
        "verify" => verify(arguments),
        other => Err(Failure::usage(format!(
            "fight has no verb {other:?}; it has run, compare and verify"
        ))),
    }
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
    let left = arguments.path("the recording on the left")?;
    let right = arguments.path("the recording on the right")?;
    arguments.finish()?;
    let (equal, report) = compare(&left, &right, true).map_err(Failure::refused)?;
    crate::cli::emit(&report, format)?;
    Ok(equal.into())
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
/// Shared with `mechcore run`, whose `compare` step asserts on the same fields
/// `fight compare` prints. `detailed` carries the two divergent tick states, which are
/// whole world snapshots: useful when a person asked for them, and megabytes of
/// noise in a script log that only wanted the verdict.
pub(crate) fn compare(
    left_path: &Path,
    right_path: &Path,
    detailed: bool,
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
    let divergent_ticks = if let Some(tick) = first_divergence.filter(|_| detailed) {
        Some(DivergentTicks {
            left: read_tick(&left, tick)?,
            right: read_tick(&right, tick)?,
        })
    } else {
        None
    };
    let equal = first_divergence.is_none();
    let content_equal = left.hashes().content_result_hash == right.hashes().content_result_hash;
    let report = CompareReport {
        schema: "mechcore.fight-compare-result.v1",
        equal,
        content_equal,
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
        divergent_ticks,
    };
    let report = serde_json::to_value(&report)
        .map_err(|error| format!("cannot serialize comparison: {error}"))?;
    Ok((equal, report))
}

fn read_tick(reader: &McfrReader, tick: u32) -> Result<Option<TickSlice>, String> {
    if tick > reader.tick_count() {
        return Ok(None);
    }
    reader
        .tick(tick)
        .map(Some)
        .map_err(|error| error.to_string())
}

#[derive(Serialize)]
struct CompareReport<'a> {
    schema: &'static str,
    equal: bool,
    content_equal: bool,
    left: RecordingSummary<'a>,
    right: RecordingSummary<'a>,
    first_divergence: Option<u32>,
    divergent_ticks: Option<DivergentTicks>,
}

#[derive(Serialize)]
struct RecordingSummary<'a> {
    physics_result_hash: &'a str,
    content_result_hash: &'a str,
    tick_count: u32,
}

#[derive(Serialize)]
struct DivergentTicks {
    left: Option<TickSlice>,
    right: Option<TickSlice>,
}
