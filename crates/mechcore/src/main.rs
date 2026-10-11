mod acquire;
mod adapter;
mod arena;
mod buildings;
mod cli;
mod convert;
mod diff;
mod difference;
mod format;
mod game;
mod generate;
mod instant;
mod kind;
mod man;
mod r#match;
mod outcome;
mod play;
mod profile;
mod query;
mod requests;
mod scene;
mod schema;
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
        "       {program} convert <file> --to <kind> [<out>] [--seed <i32>] [--round <n>] [--profile <svg>] [--force]"
    );
    eprintln!("       {program} diff <left> <right> [--fields <group>,...] [--tick <n>]");
    eprintln!("       {program} show <recording.mcfr> --view outcome|stats|buildings [--tick <n>]");
    eprintln!(
        "       {program} play <layout|fight|recording> [<page.html>] [--seed <i32>] [--no-open]"
    );
    eprintln!("       {program} format <document.yaml> [--write]");
    eprintln!(
        "       {program} generate --seed <u64> --count <n> <dir> | --seed <u64> --index <i>"
    );
    eprintln!("       {program} schema <layout|fight|match|state|action>...");
    eprintln!("       {program} match new <match.yaml> [--seed <i32>] [--map <i32>]");
    eprintln!("       {program} match show <match.yaml> --side blue|red [--wait [<seconds>]]");
    eprintln!("       {program} match act <match.yaml> --side blue|red <decision> [--dry-run]");
    eprintln!("       {program} match commit <match.yaml> --side blue|red");
    eprintln!(
        "       {program} arena run <match.yaml|dir> --blue <command> --red <command> [--matches <n>]"
    );
    eprintln!("       {program} game launch [--headless] [--offline] [--level <0-4>]");
    eprintln!("       {program} game <operation> [--level <0-4>]");
    eprintln!("       {program} man [<topic>|<kind>] [--lang <code>]");
    eprintln!("       {program} shell [<match.yaml> --side blue|red [--json]]");
    eprintln!();
    eprintln!("A file's kind is read from what it holds; `man <kind>` lists the verbs it takes.");
    eprintln!("Every command takes --format json|yaml|text and answers on standard output.");
    eprintln!("The contract is docs/spec/mechcore/cli.md, which `mechcore man cli` reads back;");
    eprintln!("--force replaces a file a command would write instead of refusing.");
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
        "query" => query::run(arguments),
        "play" => play::run(arguments),
        "format" => format::run(arguments),
        "generate" => generate::run(arguments),
        "schema" => schema::run(arguments),
        "match" => r#match::run(arguments),
        "arena" => arena::run(arguments),
        "game" => game::run(arguments),
        "man" => man::run(arguments),
        "shell" => run_shell(arguments),
        _ => return None,
    })
}

/// Opens the prompt, which starts without a game, on the match and side it
/// names if it names one.
///
/// Acquiring the game is an operation rather than an option, so a shell takes
/// one with `game launch` or `game attach` once it is open. `--json` makes the
/// prompt the request stream a player speaks, which plays the match it was
/// opened on.
fn run_shell(mut arguments: Args) -> Outcome {
    let json = arguments.flag("--json")?;
    let side = arguments.value("--side")?;
    let path = if arguments.is_empty() {
        None
    } else {
        Some(arguments.path("a match document")?)
    };
    arguments.finish()?;
    let bound = match (path, side) {
        (Some(path), Some(side)) => Some((path, turn::Side::parse(&side)?)),
        (None, None) => None,
        (Some(_), None) => {
            return Err(Failure::usage(
                "a shell opened on a match names the side it plays, with --side blue|red",
            ));
        }
        (None, Some(_)) => {
            return Err(Failure::usage(
                "--side names the side of a match the shell opens",
            ));
        }
    };
    let ran = match (json, bound) {
        (true, Some(bound)) => shell::run_json(bound),
        (true, None) => {
            return Err(Failure::usage(
                "a request stream plays one match: shell <match.yaml> --side blue|red --json",
            ));
        }
        (false, bound) => shell::run(bound),
    };
    ran.map_err(Failure::unavailable).map(|()| Verdict::Yes)
}
