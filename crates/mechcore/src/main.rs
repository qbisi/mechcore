mod acquire;
mod adapter;
mod cli;
mod doc;
mod fight;
mod game;
mod man;
mod replay;
mod script;
mod session;
mod shell;

use std::process::ExitCode;

use cli::{Args, Failure, Outcome, Verdict};

fn usage(program: &str) {
    eprintln!("usage: {program} doc verify <document>... | paths on stdin");
    eprintln!("       {program} doc format <document.yaml> [--write]");
    eprintln!("       {program} doc diff <left.yaml> <right.yaml>");
    eprintln!("       {program} replay convert <replay.grbr> <battle.yaml> [--force]");
    eprintln!("       {program} fight run <layout.yaml> [--seed <i32>] [--output <battle.mcfr>]");
    eprintln!("       {program} fight compare <left.mcfr> <right.mcfr>");
    eprintln!("       {program} fight verify <recording.mcfr>...");
    eprintln!("       {program} game <operation> --attach [--level <0-4>]");
    eprintln!("       {program} man [<topic>] [--lang <code>]");
    eprintln!("       {program} run <script.mcscript> [--check] [--force]");
    eprintln!("       {program} shell [--launch | --attach] [--level <0-4>]");
    eprintln!();
    eprintln!("Every command takes --format json|yaml|text and answers on standard output.");
    eprintln!("The contract is docs/spec/mechcore/cli.md, which `mechcore man cli` reads back;");
    eprintln!("run --check validates a script offline, without touching the game;");
    eprintln!("--force replaces existing recordings instead of asking about each.");
}

fn main() -> ExitCode {
    let mut arguments = std::env::args();
    let program = arguments.next().unwrap_or_else(|| "mechcore".into());
    let namespace = arguments.next();
    let rest = Args::new(arguments);
    match namespace.as_deref() {
        Some("doc") => cli::exit("doc", doc::run(rest)),
        Some("replay") => cli::exit("replay", replay::run(rest)),
        Some("fight") => cli::exit("fight", fight::run(rest)),
        Some("game") => cli::exit("game", game::run(rest)),
        Some("man") => cli::exit("man", man::run(rest)),
        Some("run") => cli::exit("run", run_script(rest)),
        Some("shell") => cli::exit("shell", run_shell(rest)),
        _ => {
            usage(&program);
            ExitCode::from(2)
        }
    }
}

/// Executes a run document, which owns its own acquisition and reporting.
fn run_script(arguments: Args) -> Outcome {
    script::run(arguments.into_strings())
        .map_err(Failure::failed)
        .map(Verdict::from)
}

/// Opens the prompt, whose acquisition failures are the environment's.
fn run_shell(mut arguments: Args) -> Outcome {
    let level = match arguments.value("--level")? {
        Some(value) => acquire::parse_level(&value).map_err(Failure::usage)?,
        None => mechcore_protocol::DEFAULT_LEVEL,
    };
    let launch = arguments.flag("--launch")?;
    let attach = arguments.flag("--attach")?;
    arguments.finish()?;
    let mode = match (launch, attach) {
        (true, true) => {
            return Err(Failure::usage(
                "--launch and --attach are mutually exclusive",
            ));
        }
        (true, false) => Some(acquire::Mode::Launch),
        (false, true) => Some(acquire::Mode::Attach),
        (false, false) => None,
    };
    shell::run(mode, level)
        .map_err(Failure::unavailable)
        .map(|()| Verdict::Yes)
}
