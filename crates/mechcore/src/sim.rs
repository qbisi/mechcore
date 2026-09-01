use std::{ffi::OsString, path::PathBuf};

use mechcore_mcfr::McfrReader;
use mechcore_simulation::{
    SimulationComparison, compare_recording_with_config, simulate_layout_with_config,
};
use serde::Serialize;

pub(crate) fn run(arguments: impl Iterator<Item = String>) -> Result<bool, String> {
    let arguments = arguments.collect::<Vec<_>>();
    if arguments.first().map(String::as_str) == Some("compare") {
        return run_compare(CompareOptions::parse(arguments.into_iter().skip(1))?);
    }
    let options = Options::parse(arguments.into_iter())?;
    let result = simulate_layout_with_config(
        &options.layout,
        options.output.as_deref(),
        options.seed,
        options.config.as_deref(),
    )
    .map_err(|error| error.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&result)
            .map_err(|error| format!("cannot serialize result: {error}"))?
    );
    Ok(true)
}

fn run_compare(options: CompareOptions) -> Result<bool, String> {
    let mut comparisons = Vec::with_capacity(options.recordings.len());
    for recording_path in options.recordings {
        let recording = McfrReader::open(&recording_path)
            .map_err(|error| format!("cannot open {}: {error}", recording_path.display()))?;
        let comparison = compare_recording_with_config(&recording, options.config.as_deref())
            .map_err(|error| format!("{}: {error}", recording_path.display()))?;
        comparisons.push(CaseComparison {
            recording_path: recording_path.display().to_string(),
            comparison,
        });
    }
    let equal = comparisons
        .iter()
        .all(|comparison| comparison.comparison.equal);
    let report = CompareReport {
        schema: "mechcore.sim-compare-batch-result.v1",
        equal,
        comparisons,
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&report)
            .map_err(|error| format!("cannot serialize comparison: {error}"))?
    );
    Ok(equal)
}

#[derive(Serialize)]
struct CompareReport {
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

#[derive(Debug, PartialEq, Eq)]
struct CompareOptions {
    recordings: Vec<PathBuf>,
    config: Option<PathBuf>,
}

impl CompareOptions {
    fn parse(arguments: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut arguments = arguments;
        let mut recordings = Vec::new();
        let mut config = None;
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--config" if config.is_none() => {
                    config =
                        Some(PathBuf::from(arguments.next().ok_or_else(|| {
                            "option --config requires a value".to_owned()
                        })?));
                }
                "--config" => {
                    return Err(format!("option {argument} is duplicated"));
                }
                _ if argument.starts_with('-') => {
                    return Err(format!("unknown option {argument:?}"));
                }
                _ => recordings.push(PathBuf::from(argument)),
            }
        }
        if recordings.is_empty() {
            return Err("expected at least one recording.mcfr".into());
        }
        Ok(Self { recordings, config })
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Options {
    layout: PathBuf,
    seed: Option<i32>,
    output: Option<PathBuf>,
    config: Option<PathBuf>,
}

impl Options {
    fn parse(arguments: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut arguments = arguments;
        let layout = arguments
            .next()
            .filter(|value| !value.starts_with('-'))
            .map(PathBuf::from)
            .ok_or_else(|| "expected layout.yaml as the first argument".to_owned())?;
        let mut options = Self {
            layout,
            seed: None,
            output: None,
            config: None,
        };
        while let Some(option) = arguments.next() {
            let value = arguments
                .next()
                .ok_or_else(|| format!("option {option} requires a value"))?;
            match option.as_str() {
                "--seed" if options.seed.is_none() => {
                    options.seed = Some(
                        value
                            .parse()
                            .map_err(|_| format!("seed {value:?} is not an i32"))?,
                    );
                }
                "--output" if options.output.is_none() => {
                    options.output = Some(PathBuf::from(OsString::from(value)));
                }
                "--config" if options.config.is_none() => {
                    options.config = Some(PathBuf::from(OsString::from(value)));
                }
                "--seed" | "--output" | "--config" => {
                    return Err(format!("option {option} is duplicated"));
                }
                _ => return Err(format!("unknown option {option:?}")),
            }
        }
        Ok(options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_documented_sim_command() {
        let options = Options::parse(
            [
                "battle.yaml",
                "--seed",
                "-17",
                "--output",
                "battle.mcfr",
                "--config",
                "config",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(options.seed, Some(-17));
        assert_eq!(options.output, Some(PathBuf::from("battle.mcfr")));
        assert_eq!(options.config, Some(PathBuf::from("config")));
    }

    #[test]
    fn parses_multiple_compare_recordings() {
        let options = CompareOptions::parse(
            ["one.mcfr", "two.mcfr", "--config", "config"]
                .into_iter()
                .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(
            options,
            CompareOptions {
                recordings: vec![PathBuf::from("one.mcfr"), PathBuf::from("two.mcfr")],
                config: Some(PathBuf::from("config")),
            }
        );
    }
}
