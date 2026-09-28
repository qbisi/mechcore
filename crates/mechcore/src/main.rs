mod acquire;
mod adapter;
mod buildings;
mod cli;
mod convert;
mod diff;
mod difference;
mod format;
mod game;
mod instant;
mod kind;
mod man;
mod r#match;
mod outcome;
mod scene;
mod schema;
mod script;
mod session;
mod shell;
mod show;
mod stats;
mod turn;
mod verify;

use std::process::ExitCode;

use cli::{Args, Failure, Outcome, Verdict};

fn usage(program: &str) {
    eprintln!("usage: {program} verify <file>... | paths on stdin");
    eprintln!(
        "       {program} convert <file> --to <kind> [<out>] [--seed <i32>] [--round <n>] [--force]"
    );
    eprintln!("       {program} diff <left> <right> [--fields <group>,...] [--tick <n>]");
    eprintln!("       {program} show <recording.mcfr> --view outcome|stats|buildings [--tick <n>]");
    eprintln!("       {program} format <document.yaml> [--write]");
    eprintln!("       {program} schema <layout|fight|match|state|action>...");
    eprintln!("       {program} match new <match.yaml> [--seed <i32>] [--map <i32>]");
    eprintln!("       {program} match show <match.yaml> --side blue|red [--wait [<seconds>]]");
    eprintln!("       {program} match act <match.yaml> --side blue|red <decision> [--dry-run]");
    eprintln!("       {program} match commit <match.yaml> --side blue|red");
    eprintln!("       {program} game <operation> [--level <0-4>]");
    eprintln!("       {program} man [<topic>|<kind>] [--lang <code>]");
    eprintln!("       {program} run <script.mcscript> [--check] [--force]");
    eprintln!("       {program} shell");
    eprintln!();
    eprintln!("A file's kind is read from what it holds; `man <kind>` lists the verbs it takes.");
    eprintln!("Every command takes --format json|yaml|text and answers on standard output.");
    eprintln!("The contract is docs/spec/mechcore/cli.md, which `mechcore man cli` reads back;");
    eprintln!("run --check validates a script without touching the game;");
    eprintln!("--force replaces every file a step would write instead of asking.");
}

fn main() -> ExitCode {
    let mut arguments = std::env::args();
    let program = arguments.next().unwrap_or_else(|| "mechcore".into());
    let command = arguments.next();
    let rest = Args::new(arguments);
    let Some(command) = command else {
        usage(&program);
        return ExitCode::from(2);
    };
    if let Some(outcome) = dispatch(&command, rest) {
        return cli::exit(&command, outcome);
    }
    usage(&program);
    ExitCode::from(2)
}

/// Runs a command that needs no session: a file verb, a stateful namespace
/// other than the game's, or the manual. Answers nothing for a word that
/// names no such command.
///
/// A prompt dispatches its lines here too, so a line and a command are the
/// same text.
pub(crate) fn dispatch(command: &str, arguments: Args) -> Option<Outcome> {
    Some(match command {
        "verify" => verify::run(arguments),
        "convert" => convert::run(arguments),
        "diff" => diff::run(arguments),
        "show" => show::run(arguments),
        "format" => format::run(arguments),
        "schema" => schema::run(arguments),
        "match" => r#match::run(arguments),
        "game" => game::run(arguments),
        "man" => man::run(arguments),
        "run" => run_script(arguments),
        "shell" => run_shell(arguments),
        _ => return None,
    })
}

/// Executes a run document, which owns its own acquisition and reporting.
fn run_script(arguments: Args) -> Outcome {
    script::run(arguments.into_strings())
        .map_err(Failure::failed)
        .map(Verdict::from)
}

/// Opens the prompt, which starts without a game.
///
/// Acquiring the game is an operation rather than an option, so a shell takes
/// one with `game launch` or `game attach` once it is open.
fn run_shell(arguments: Args) -> Outcome {
    arguments.finish()?;
    shell::run()
        .map_err(Failure::unavailable)
        .map(|()| Verdict::Yes)
}
