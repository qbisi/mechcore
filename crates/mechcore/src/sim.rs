use std::{ffi::OsString, path::PathBuf};

use mechcore_simulation::simulate_layout_with_config;

pub(crate) fn run(arguments: impl Iterator<Item = String>) -> Result<(), String> {
    let options = Options::parse(arguments)?;
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
    Ok(())
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
}
