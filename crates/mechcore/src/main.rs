mod acquire;
mod adapter;
mod layout;
mod mcfr;
mod mcp;
mod session;
mod shell;
mod sim;

use std::process::ExitCode;

fn usage(program: &str) {
    eprintln!("usage: {program} shell [--launch | --attach]");
    eprintln!("       {program} mcp");
    eprintln!("       {program} mcfr compare <left.mcfr> <right.mcfr>");
    eprintln!("       {program} layout verify <layout.yaml>");
    eprintln!("       {program} layout format <layout.yaml> [--write]");
    eprintln!("       {program} layout diff <left.yaml> <right.yaml>");
    eprintln!(
        "       {program} sim <layout.yaml> [--seed <i32>] [--output <battle.mcfr>] [--config <directory>]"
    );
    eprintln!("       {program} sim compare <recording.mcfr>... [--config <directory>]");
}

fn main() -> ExitCode {
    let mut arguments = std::env::args();
    let program = arguments.next().unwrap_or_else(|| "mechcore".into());
    match arguments.next().as_deref() {
        Some("shell") => match shell_mode(arguments) {
            Ok(mode) => match shell::run(mode) {
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
        Some("mcp") if arguments.next().is_none() => match mcp::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("mechcore mcp: {error}");
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
        Some("layout") => match layout::run(arguments) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::FAILURE,
            Err(error) => {
                eprintln!("mechcore layout: {error}");
                ExitCode::FAILURE
            }
        },
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

/// Acquisition is declared, never inferred: at most one of the two flags.
fn shell_mode(
    arguments: impl Iterator<Item = String>,
) -> Result<Option<acquire::Mode>, String> {
    let mut mode = None;
    for argument in arguments {
        let requested = match argument.as_str() {
            "--launch" => acquire::Mode::Launch,
            "--attach" => acquire::Mode::Attach,
            other => return Err(format!("unexpected argument {other}")),
        };
        if mode.is_some() {
            return Err("--launch and --attach are mutually exclusive".into());
        }
        mode = Some(requested);
    }
    Ok(mode)
}
