//! `play`: writes the page that plays a fight back.
//!
//! A recording is played as it holds the fight. A layout or a fight document is
//! fought by the simulator first, and its timeline is kept in memory and laid
//! out for the page directly: writing a recording only to read it back would
//! cost the encoding and the reading for nothing, and `convert --to mcfr` is
//! what keeps one. The page is `mechcore_player`'s.
//!
//! From the command line the page is opened in the system's browser once it is
//! written, unless `--no-open` says not to.

use std::{
    path::Path,
    process::{Command, Stdio},
};

use mechcore_mcfr::{McfrReader, Recording, UnitPose};
use mechcore_simulation::{Record, SimulationResult};
use serde::Serialize;

use crate::{
    cli::{Args, Failure, Outcome, Verdict},
    kind::Kind,
};

pub(crate) const SCHEMA: &str = "mechcore.play-result.v2";

/// What `play` answers: the page it wrote and the fight on it.
#[derive(Serialize)]
pub(crate) struct Played {
    schema: &'static str,
    page: String,
    /// The kind of the file played.
    kind: &'static str,
    /// Who fought it: `game` or `simulator`.
    producer: &'static str,
    ticks: u32,
    /// The seed the simulator fought with, for a fight it fought here.
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed_source: Option<&'static str>,
    /// Whether the system was asked to open the page and took the request.
    opened: bool,
}

/// Reads `play <file> [<page>] [--seed <i32>] [--no-open]` off a command line,
/// and opens the page it writes unless told not to.
///
/// # Errors
///
/// Returns a usage failure for a command the contract does not define, and
/// whatever [`play`] fails with.
pub(crate) fn run(mut arguments: Args) -> Outcome {
    let format = arguments.format()?;
    let seed = arguments.parsed::<i32>("--seed", "a signed 32-bit integer")?;
    let stay = arguments.flag("--no-open")?;
    let input = arguments.path("a file to play")?;
    let page = if arguments.is_empty() {
        None
    } else {
        Some(arguments.path("the page to write")?)
    };
    arguments.finish()?;
    let mut played = play(&input, page.as_deref(), seed)?;
    if !stay {
        // A page that cannot be opened is still written, and the answer says
        // which: not opening it is no failure of the command.
        match open(Path::new(&played.page)) {
            Ok(()) => played.opened = true,
            Err(error) => eprintln!(
                "mechcore: wrote {} but cannot open it: {error}",
                played.page
            ),
        }
    }
    crate::cli::emit(&played, format)?;
    Ok(Verdict::Yes)
}

/// Writes the page that plays `input` to `page`, or beside the input with an
/// `.html` extension.
///
/// # Errors
///
/// Returns a refusal for a kind `play` does not take or a fight the simulator
/// does not fight, a usage failure for a seed the file does not take, and a
/// failure for a page that cannot be written.
pub(crate) fn play(
    input: &Path,
    page: Option<&Path>,
    seed: Option<i32>,
) -> Result<Played, Failure> {
    let (kind, bytes) = Kind::read(input)?;
    kind.require("play")?;
    let page = page.map_or_else(|| input.with_extension("html"), Path::to_path_buf);
    let fought = |result: Result<SimulationResult, mechcore_simulation::Error>| {
        let result = result.map_err(|error| Failure::refused(error.to_string()))?;
        let recording = result
            .recording
            .ok_or_else(|| Failure::failed("the simulator kept no recording of the fight"))?;
        written(
            &recording,
            None,
            input,
            kind,
            &page,
            Some((result.seed, result.seed_source)),
        )
    };
    match kind {
        Kind::Mcfr => {
            if seed.is_some() {
                return Err(Failure::usage(
                    "a recording was fought already; --seed belongs to a layout",
                ));
            }
            let reader = McfrReader::open(input).map_err(|error| {
                Failure::refused(format!("cannot read {}: {error}", input.display()))
            })?;
            // A recording the game made may hold how it drew each unit.
            let poses = reader.instrument::<UnitPose>().map_err(|error| {
                Failure::refused(format!("cannot read {}'s poses: {error}", input.display()))
            })?;
            written(&reader, poses.as_deref(), input, kind, &page, None)
        }
        Kind::Layout => fought(mechcore_simulation::simulate_layout(
            input,
            Record::Memory,
            seed,
        )),
        Kind::Fight => {
            if seed.is_some() {
                return Err(Failure::usage(
                    "a fight document states its own seed; --seed belongs to a layout",
                ));
            }
            let fight = mechcore_document::fight::parse_yaml(&bytes).map_err(Failure::refused)?;
            let layout =
                mechcore_document::canonical_yaml(mechcore_document::fight::project(&fight))
                    .map_err(Failure::failed)?;
            fought(mechcore_simulation::simulate_document(
                layout.as_bytes(),
                Record::Memory,
                None,
            ))
        }
        other => Err(Failure::refused(format!(
            "play does not take a {} file; {}",
            other.name(),
            other.takes()
        ))),
    }
}

/// Lays a fight out for the page and writes the page.
fn written(
    recording: &dyn Recording,
    poses: Option<&[(u32, UnitPose)]>,
    input: &Path,
    kind: Kind,
    page: &Path,
    fought: Option<(i32, &'static str)>,
) -> Result<Played, Failure> {
    let timeline = mechcore_player::timeline(recording, poses)
        .map_err(|error| Failure::failed(format!("cannot read the fight: {error}")))?;
    let title = input
        .file_stem()
        .map_or_else(|| "fight".into(), |stem| stem.to_string_lossy());
    let html = mechcore_player::page(&timeline, &title)
        .map_err(|error| Failure::failed(format!("cannot write the page: {error}")))?;
    std::fs::write(page, html)
        .map_err(|error| Failure::failed(format!("cannot write {}: {error}", page.display())))?;
    Ok(Played {
        schema: SCHEMA,
        page: page.display().to_string(),
        kind: kind.name(),
        producer: recording.producer().as_str(),
        ticks: recording.terminal_tick(),
        seed: fought.map(|(seed, _)| seed),
        seed_source: fought.map(|(_, source)| source),
        opened: false,
    })
}

/// Asks the system to open the page in its default browser, without waiting
/// for the browser.
fn open(page: &Path) -> std::io::Result<()> {
    let mut command = if cfg!(target_os = "macos") {
        Command::new("open")
    } else if cfg!(target_os = "windows") {
        let mut start = Command::new("cmd");
        start.args(["/C", "start", ""]);
        start
    } else {
        Command::new("xdg-open")
    };
    command
        .arg(page)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(drop)
}
