mod acquire;
mod adapter;
mod convert;
mod document;
mod mcfr;
mod script;
mod session;
mod shell;
mod sim;

use std::process::ExitCode;

fn usage(program: &str) {
    eprintln!("usage: {program} shell [--launch | --attach] [--level <0-4>]");
    eprintln!("       {program} run <script.mcscript> [--check] [--force]");
    eprintln!("       {program} mcfr compare <left.mcfr> <right.mcfr>");
    eprintln!("       {program} verify <document>... | paths on stdin");
    eprintln!("       {program} format <document.yaml> [--write]");
    eprintln!("       {program} diff <left.yaml> <right.yaml>");
    eprintln!("       {program} convert <replay.grbr> <battle.yaml> [--force]");
    eprintln!(
        "       {program} sim <layout.yaml> [--seed <i32>] [--output <battle.mcfr>] [--config <directory>]"
    );
    eprintln!("       {program} sim compare <recording.mcfr>... [--config <directory>]");
    eprintln!();
    eprintln!("run --check validates a script offline, without touching the game;");
    eprintln!("--force replaces existing recordings instead of asking about each.");
    eprintln!("Script steps and their options are documented in docs/spec/mechcore/mcscript.md;");
    eprintln!("shell commands are listed by `help` inside the shell.");
}

fn main() -> ExitCode {
    let mut arguments = std::env::args();
    let program = arguments.next().unwrap_or_else(|| "mechcore".into());
    match arguments.next().as_deref() {
        Some("shell") => match shell_options(arguments) {
            Ok((mode, level)) => match shell::run(mode, level) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("mechcore shell: {error}");
                    ExitCode::FAILURE
                }
            },
            Err(error) => {
                eprintln!("mechcore shell: {error}");
                ExitCode::from(2)
            }
        },
        Some("run") => match script::run(arguments) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::FAILURE,
            Err(error) => {
                eprintln!("mechcore run: {error}");
                ExitCode::FAILURE
            }
        },
        Some("mcfr") => match mcfr::run(arguments) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::FAILURE,
            Err(error) => {
                eprintln!("mechcore mcfr: {error}");
                ExitCode::FAILURE
            }
        },
        Some("verify") => report("verify", document::verify(arguments)),
        Some("format") => report("format", document::format(arguments).map(|()| true)),
        Some("diff") => report("diff", document::diff(arguments)),
        Some("convert") => report("convert", convert::run(arguments).map(|()| true)),
        Some("sim") => match sim::run(arguments) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::FAILURE,
            Err(error) => {
                eprintln!("mechcore sim: {error}");
                ExitCode::FAILURE
            }
        },
        _ => {
            usage(&program);
            ExitCode::from(2)
        }
    }
}

/// Turns a command's outcome into an exit code, naming the command on failure.
fn report(command: &str, outcome: Result<bool, String>) -> ExitCode {
    match outcome {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("mechcore {command}: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Acquisition is declared, never inferred: at most one of the two flags.
///
/// The level is a separate decision from the verb: it says what this session
/// outranks, not how it gets the game.
fn shell_options(
    arguments: impl Iterator<Item = String>,
) -> Result<(Option<acquire::Mode>, u8), String> {
    let mut arguments = arguments.peekable();
    let mut mode = None;
    let mut level = None;
    while let Some(argument) = arguments.next() {
        let requested = match argument.as_str() {
            "--launch" => acquire::Mode::Launch,
            "--attach" => acquire::Mode::Attach,
            "--level" => {
                if level.is_some() {
                    return Err("--level given twice".into());
                }
                let value = arguments.next().ok_or("--level needs a number")?;
                level = Some(acquire::parse_level(&value)?);
                continue;
            }
            other => return Err(format!("unexpected argument {other}")),
        };
        if mode.is_some() {
            return Err("--launch and --attach are mutually exclusive".into());
        }
        mode = Some(requested);
    }
    Ok((mode, level.unwrap_or(mechcore_protocol::DEFAULT_LEVEL)))
}
