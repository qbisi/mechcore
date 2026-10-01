//! `mechcore-player <recording.mcfr> [<page.html>]`: writes the page that
//! plays a recording, beside it unless told where.

use std::{
    path::{Path, PathBuf},
    process::ExitCode,
};

use mechcore_mcfr::McfrReader;

const USAGE: &str = "usage: mechcore-player <recording.mcfr> [<page.html>]";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let (recording, output) = match arguments.as_slice() {
        [flag] if flag == "-h" || flag == "--help" => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        [recording] => (
            PathBuf::from(recording),
            Path::new(recording).with_extension("html"),
        ),
        [recording, output] => (PathBuf::from(recording), PathBuf::from(output)),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match write(&recording, &output) {
        Ok(()) => {
            println!("{}", output.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("mechcore-player: {error}");
            ExitCode::FAILURE
        }
    }
}

fn write(recording: &Path, output: &Path) -> Result<(), String> {
    let reader = McfrReader::open(recording)
        .map_err(|error| format!("cannot read {}: {error}", recording.display()))?;
    let timeline = mechcore_player::timeline(&reader).map_err(|error| error.to_string())?;
    let title = recording
        .file_stem()
        .map_or_else(|| "recording".into(), |stem| stem.to_string_lossy());
    let page = mechcore_player::page(&timeline, &title).map_err(|error| error.to_string())?;
    std::fs::write(output, page)
        .map_err(|error| format!("cannot write {}: {error}", output.display()))
}
