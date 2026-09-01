use std::{ffi::OsString, fs, path::PathBuf};

use mechcore_mcfr::{MCFR_FORMAT, McfrReader};
use mechcore_simulation::{
    SimulationComparison, compare_layout_to_recording_with_config, simulate_layout_with_config,
};
use serde::{Deserialize, Serialize};

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
    let entries: Vec<RegressionEntry> = serde_yaml::from_slice(
        &fs::read(&options.manifest)
            .map_err(|error| format!("cannot read {}: {error}", options.manifest.display()))?,
    )
    .map_err(|error| format!("cannot parse {}: {error}", options.manifest.display()))?;
    let repository = repository();
    let mut comparisons = Vec::with_capacity(options.recordings.len());
    for recording_path in options.recordings {
        let recording = McfrReader::open(&recording_path)
            .map_err(|error| format!("cannot open {}: {error}", recording_path.display()))?;
        let matches = entries
            .iter()
            .filter(|entry| entry.scenario_hash == recording.hashes().scenario_hash)
            .collect::<Vec<_>>();
        let entry = match matches.as_slice() {
            [entry] => *entry,
            [] => {
                return Err(format!(
                    "{} scenario_hash {} is not present in {}",
                    recording_path.display(),
                    recording.hashes().scenario_hash,
                    options.manifest.display()
                ));
            }
            _ => {
                return Err(format!(
                    "{} contains duplicate scenario_hash {}",
                    options.manifest.display(),
                    recording.hashes().scenario_hash
                ));
            }
        };
        if entry.format != MCFR_FORMAT {
            return Err(format!(
                "manifest case {} uses MCFR format {}, expected {MCFR_FORMAT}",
                entry.name, entry.format
            ));
        }
        if entry.game_build != recording.game_build() {
            return Err(format!(
                "manifest case {} game_build {} differs from recording {}",
                entry.name,
                entry.game_build,
                recording.game_build()
            ));
        }
        let layout = if entry.layout.is_absolute() {
            entry.layout.clone()
        } else {
            repository.join(&entry.layout)
        };
        let comparison = compare_layout_to_recording_with_config(
            &layout,
            entry.seed,
            options.config.as_deref(),
            &recording,
        )
        .map_err(|error| format!("{}: {error}", recording_path.display()))?;
        comparisons.push(CaseComparison {
            name: entry.name.clone(),
            recording_path: recording_path.display().to_string(),
            layout: layout.display().to_string(),
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

fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[derive(Debug, Deserialize)]
struct RegressionEntry {
    name: String,
    layout: PathBuf,
    game_build: String,
    format: String,
    seed: i32,
    scenario_hash: String,
}

#[derive(Serialize)]
struct CompareReport {
    schema: &'static str,
    equal: bool,
    comparisons: Vec<CaseComparison>,
}

#[derive(Serialize)]
struct CaseComparison {
    name: String,
    recording_path: String,
    layout: String,
    #[serde(flatten)]
    comparison: SimulationComparison,
}

#[derive(Debug, PartialEq, Eq)]
struct CompareOptions {
    recordings: Vec<PathBuf>,
    manifest: PathBuf,
    config: Option<PathBuf>,
}

impl CompareOptions {
    fn parse(arguments: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut arguments = arguments;
        let mut recordings = Vec::new();
        let mut manifest = None;
        let mut config = None;
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--manifest" if manifest.is_none() => {
                    manifest =
                        Some(PathBuf::from(arguments.next().ok_or_else(|| {
                            "option --manifest requires a value".to_owned()
                        })?));
                }
                "--config" if config.is_none() => {
                    config =
                        Some(PathBuf::from(arguments.next().ok_or_else(|| {
                            "option --config requires a value".to_owned()
                        })?));
                }
                "--manifest" | "--config" => {
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
        Ok(Self {
            recordings,
            manifest: manifest.unwrap_or_else(|| repository().join("tests/mcfr-regressions.yaml")),
            config,
        })
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
            [
                "one.mcfr",
                "two.mcfr",
                "--manifest",
                "regressions.yaml",
                "--config",
                "config",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(
            options,
            CompareOptions {
                recordings: vec![PathBuf::from("one.mcfr"), PathBuf::from("two.mcfr")],
                manifest: PathBuf::from("regressions.yaml"),
                config: Some(PathBuf::from("config")),
            }
        );
    }
}
