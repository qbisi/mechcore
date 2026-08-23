mod adapter;
mod mcp;
mod sim;

use std::process::ExitCode;

fn usage(program: &str) {
    eprintln!("usage: {program} mcp");
    eprintln!(
        "       {program} sim <layout.yaml> [--seed <i32>] [--output <battle.mcfr>] [--config <directory>]"
    );
}

fn main() -> ExitCode {
    let mut arguments = std::env::args();
    let program = arguments.next().unwrap_or_else(|| "mechcore".into());
    match arguments.next().as_deref() {
        Some("mcp") if arguments.next().is_none() => match mcp::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("mechcore mcp: {error}");
                ExitCode::FAILURE
            }
        },
        Some("sim") => match sim::run(arguments) {
            Ok(()) => ExitCode::SUCCESS,
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
