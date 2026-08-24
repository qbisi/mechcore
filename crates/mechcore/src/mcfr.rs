use std::path::PathBuf;

use mechcore_mcfr::McfrReader;
use serde::Serialize;

pub(crate) fn run(arguments: impl Iterator<Item = String>) -> Result<bool, String> {
    let options = Options::parse(arguments)?;
    let left = McfrReader::open_verified(&options.left).map_err(|error| error.to_string())?;
    let right = McfrReader::open_verified(&options.right).map_err(|error| error.to_string())?;
    let equal = left.hashes().result_hash == right.hashes().result_hash;
    let first_divergence = if equal {
        None
    } else {
        left.first_divergence(&right)
            .map_err(|error| error.to_string())?
    };
    let report = CompareReport {
        schema: "mechcore.mcfr-compare-result.v1",
        equal,
        scenario_hash: &left.hashes().scenario_hash,
        left: RecordingSummary {
            result_hash: &left.hashes().result_hash,
            tick_count: left.tick_count(),
        },
        right: RecordingSummary {
            result_hash: &right.hashes().result_hash,
            tick_count: right.tick_count(),
        },
        first_divergence,
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&report)
            .map_err(|error| format!("cannot serialize comparison: {error}"))?
    );
    Ok(equal)
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
    scenario_hash: &'a str,
    left: RecordingSummary<'a>,
    right: RecordingSummary<'a>,
    first_divergence: Option<u64>,
}

#[derive(Serialize)]
struct RecordingSummary<'a> {
    result_hash: &'a str,
    tick_count: u64,
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
