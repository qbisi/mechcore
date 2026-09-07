use std::path::PathBuf;

use mechcore_mcfr::{McfrReader, TickSlice};
use serde::Serialize;

pub(crate) fn run(arguments: impl Iterator<Item = String>) -> Result<bool, String> {
    let options = Options::parse(arguments)?;
    let (equal, report) = compare(&options.left, &options.right, true)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&report)
            .map_err(|error| format!("cannot serialize comparison: {error}"))?
    );
    Ok(equal)
}

/// Compare two recordings, returning the verdict and the structured report.
///
/// Shared with `mechcore run`, whose `compare` step asserts on the same fields
/// the CLI prints. `detailed` carries the two divergent tick states, which are
/// whole world snapshots: useful when a person asked for them, and megabytes of
/// noise in a script log that only wanted the verdict.
pub(crate) fn compare(
    left_path: &std::path::Path,
    right_path: &std::path::Path,
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
        schema: "mechcore.mcfr-compare-result.v2",
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

#[derive(Debug, PartialEq, Eq)]
struct Options {
    left: PathBuf,
    right: PathBuf,
}

impl Options {
    fn parse(arguments: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut arguments = arguments;
        if arguments.next().as_deref() != Some("compare") {
            return Err("expected compare <left.mcfr> <right.mcfr>".into());
        }
        let left = arguments
            .next()
            .map(PathBuf::from)
            .ok_or_else(|| "expected left.mcfr".to_owned())?;
        let right = arguments
            .next()
            .map(PathBuf::from)
            .ok_or_else(|| "expected right.mcfr".to_owned())?;
        if let Some(argument) = arguments.next() {
            return Err(format!("unexpected argument {argument:?}"));
        }
        Ok(Self { left, right })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_compare_arguments() {
        assert_eq!(
            Options::parse(
                ["compare", "left.mcfr", "right.mcfr"]
                    .into_iter()
                    .map(str::to_owned)
            )
            .unwrap(),
            Options {
                left: PathBuf::from("left.mcfr"),
                right: PathBuf::from("right.mcfr"),
            }
        );
    }

    #[test]
    fn rejects_extra_arguments() {
        assert!(
            Options::parse(
                ["compare", "left.mcfr", "right.mcfr", "extra"]
                    .into_iter()
                    .map(str::to_owned)
            )
            .is_err()
        );
    }
}
