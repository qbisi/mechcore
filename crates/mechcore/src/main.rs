mod acquire;
mod adapter;
mod cli;
mod doc;
mod fight;
mod game;
mod instant;
mod man;
mod r#match;
mod outcome;
mod replay;
mod scene;
mod script;
mod session;
mod shell;
mod stats;
mod turn;

use std::process::ExitCode;

use cli::{Args, Failure, Outcome, Verdict};

fn usage(program: &str) {
    eprintln!("usage: {program} doc verify <document>... | paths on stdin");
    eprintln!("       {program} doc format <document.yaml> [--write]");
    eprintln!("       {program} doc diff <left.yaml> <right.yaml>");
    eprintln!("       {program} doc project <battle.yaml> --round <n> [--output <layout.yaml>]");
    eprintln!("       {program} doc schema <layout|battle|state|action>...");
    eprintln!("       {program} replay convert <replay.grbr> <battle.yaml> [--force]");
    eprintln!("       {program} fight run <layout.yaml> [--seed <i32>] [--output <battle.mcfr>]");
    eprintln!("       {program} fight outcome <recording.mcfr>");
    eprintln!("       {program} fight stats <recording.mcfr> [--tick <n>]");
    eprintln!("       {program} fight compare <left.mcfr> <right.mcfr>");
    eprintln!("       {program} fight verify <recording.mcfr>...");
    eprintln!("       {program} match new <match.yaml> [--seed <i32>] [--map <i32>]");
    eprintln!("       {program} match show <match.yaml> --side blue|red [--wait [<seconds>]]");
    eprintln!("       {program} match act <match.yaml> --side blue|red <decision> [--dry-run]");
    eprintln!("       {program} match commit <match.yaml> --side blue|red");
    eprintln!("       {program} game <operation> [--level <0-4>]");
    eprintln!("       {program} man [<topic>] [--lang <code>]");
    eprintln!("       {program} run <script.mcscript> [--check] [--force]");
    eprintln!("       {program} shell");
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
        Some("match") => cli::exit("match", r#match::run(rest)),
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

/// Opens the prompt, which starts offline.
///
/// Acquiring the game is an operation rather than an option, so a shell takes
/// one with `game launch` or `game attach` once it is open.
fn run_shell(arguments: Args) -> Outcome {
    arguments.finish()?;
    shell::run()
        .map_err(Failure::unavailable)
        .map(|()| Verdict::Yes)
}
