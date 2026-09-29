//! `verify`: checks each file against the contract its own kind defines.
//!
//! A layout is checked by the shared static compiler. A match is checked
//! against its seed, by predicting each round's next opening from the one
//! before, and by fighting its rounds in order until the first the simulator
//! refuses or whose result is not the next round's opening. A recording is checked by simulating the layout it embeds again and
//! comparing the result with what it holds. A fight document is checked by
//! fighting its projection with its seed, as `convert --to fight` does, and
//! comparing the result it states with the one the simulator arrives at.
//!
//! With `--backend game` the game fights instead: each fight document and
//! recording is recorded again, headless, and read back as a fight, and
//! `--update` writes back what the game recorded wherever it differs.

use std::{
    io::{IsTerminal, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::Serialize;
use serde_json::Value;

use crate::cli::{Args, Failure, Outcome};
use crate::convert::Backend;
use crate::kind::Kind;
use crate::session::Session;

/// Checks each named file.
///
/// Paths come from the arguments, or from standard input one per line when
/// there are none, so a batch is a pipe rather than a flag:
///
/// ```text
/// mechcore verify layout.yaml
/// ls work/match/*/*.yaml | mechcore verify
/// ```
///
/// One report per input goes to standard output, one JSON object per line, a
/// refusal included. Standard error carries only what stops the run, so a file
/// that cannot be read is a report like any other and the rest of a batch still
/// runs. The exit code says whether every input was valid.
///
/// `--backend game` fights each fight document and recording in a game
/// somebody started, attached once for the batch; a layout or a match, which
/// holds no fight, is checked as without it. `--update` then writes back each
/// file the game's fight differs from, and the report says so.
///
/// # Errors
///
/// Returns an error when no input is named at all, when the list of paths
/// cannot be read, when `--update` is asked of the simulator, and
/// `unavailable` when the game backend finds no game.
pub(crate) fn run(mut arguments: Args) -> Outcome {
    let backend = arguments
        .value("--backend")?
        .map_or(Ok(Backend::Simulator), |name| Backend::parse(&name))?;
    let update = arguments.flag("--update")?;
    let level = crate::acquire::level(&mut arguments)?;
    let paths = inputs(&mut arguments)?;
    arguments.finish()?;
    if update && backend != Backend::Game {
        return Err(Failure::usage(
            "--update writes what the game recorded, and a pin never comes from the \
             simulator; it takes --backend game",
        ));
    }
    let mut valid = true;
    let mut emit = |report: &Report| -> Result<(), Failure> {
        valid &= report.valid;
        println!(
            "{}",
            serde_json::to_string(report)
                .map_err(|error| Failure::failed(format!("cannot write the report: {error}")))?
        );
        Ok(())
    };
    match backend {
        Backend::Simulator => {
            for path in paths {
                emit(&check(&path))?;
            }
        }
        Backend::Game => {
            crate::game::with_game(level, async |session| {
                for path in paths {
                    emit(&in_game(&path, session, update).await)?;
                }
                Ok::<(), Failure>(())
            })??;
        }
    }
    Ok(valid.into())
}

/// Checks one file.
///
/// A file that cannot be read or is of a kind `verify` does not take is a
/// report like any other, so a batch goes on past it.
pub(crate) fn check(path: &Path) -> Report {
    one(path).unwrap_or_else(|error| Report::refused(path, error))
}

/// The paths to check: the arguments, or standard input one per line.
///
/// An empty argument list with a terminal on standard input is a mistake rather
/// than an empty batch, so it is refused instead of succeeding over nothing.
fn inputs(arguments: &mut Args) -> Result<Vec<PathBuf>, Failure> {
    let named: Vec<PathBuf> = arguments
        .operands()?
        .into_iter()
        .map(PathBuf::from)
        .collect();
    if !named.is_empty() {
        return Ok(named);
    }
    if std::io::stdin().is_terminal() {
        return Err(Failure::usage(
            "expected <file>... after `verify`, or paths on standard input",
        ));
    }
    let mut piped = String::new();
    std::io::stdin()
        .read_to_string(&mut piped)
        .map_err(|error| {
            Failure::failed(format!("cannot read paths from standard input: {error}"))
        })?;
    Ok(piped
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect())
}

fn one(path: &Path) -> Result<Report, String> {
    let (kind, bytes) = read(path)?;
    match kind {
        Kind::Match => {
            let stated = mechcore_document::opening::stated(&bytes)?
                .ok_or("the document names itself a match and holds no match stream")?;
            verify_match(path, &stated)
        }
        Kind::Mcfr => verify_recording(path),
        Kind::Fight => verify_fight(path, &bytes),
        _ => verify_layout(path, &bytes),
    }
}

/// Reads a file `verify` takes, and its kind.
fn read(path: &Path) -> Result<(Kind, Vec<u8>), String> {
    // A directory names no file, and expanding one is the shell's job: saying
    // so beats an operating system error about a read that could not have
    // worked.
    if path.is_dir() {
        return Err(format!(
            "{} is a directory; name the files themselves, as a shell glob \
             or on standard input",
            path.display()
        ));
    }
    let bytes =
        std::fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let kind = Kind::of(&bytes)?;
    kind.require("verify")
        .map_err(|failure| failure.reason().to_owned())?;
    Ok((kind, bytes))
}

fn verify_layout(path: &Path, bytes: &[u8]) -> Result<Report, String> {
    let plan = mechcore_document::compile_layout(mechcore_document::parse_yaml(bytes)?)?;
    Ok(Report {
        schema: SCHEMA,
        valid: true,
        kind: "layout",
        path: path.display().to_string(),
        error: None,
        detail: serde_json::json!({
            "seed": plan.seed,
            "map_id": plan.map_id,
            "round": plan.round,
            "unit_count": plan.unit_count(),
            "construction_count": plan.construction_count(),
            "contraption_count": plan.contraption_count(),
            "standing_shield_count": plan.standing_shield_count(),
            "standing_oil_count": plan.standing_oil_count(),
        }),
    })
}

/// Simulates a recording's own layout again, and compares the two.
///
/// The recording is the claim and the simulator the check: they agree when
/// every tick hash does.
fn verify_recording(path: &Path) -> Result<Report, String> {
    let recording = mechcore_mcfr::McfrReader::open(path)
        .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    let comparison =
        mechcore_simulation::compare_recording(&recording).map_err(|error| error.to_string())?;
    Ok(Report {
        schema: SCHEMA,
        valid: comparison.equal,
        kind: "mcfr",
        path: path.display().to_string(),
        error: (!comparison.equal).then(|| {
            comparison.first_divergence.map_or_else(
                || "the simulation's result hash differs from the recording's".to_owned(),
                |tick| format!("the simulation parts from the recording at tick {tick}"),
            )
        }),
        detail: serde_json::json!({ "comparison": comparison }),
    })
}

/// Fights a fight document's projection with its seed, and compares the
/// result the document states with the one the simulator arrives at.
///
/// The simulator's document is written onto the same projection, so the
/// layout fields agree by construction and every difference is a result
/// field. `source` is where each result was read and is not compared; a
/// `replay` result has no trajectory, so only its result fields are.
///
/// A document names no tick of its fight, only the hash of all of them, so a
/// hash that differs says the trajectories part and not where: the recording
/// the document was read from answers that under `verify <mcfr>`.
fn verify_fight(path: &Path, bytes: &[u8]) -> Result<Report, String> {
    let document = mechcore_document::fight::parse_yaml(bytes)?.normalized();
    let trajectory = document.source.has_trajectory();
    let report = |error: Option<String>, differences: Vec<Difference>| Report {
        schema: SCHEMA,
        valid: error.is_none(),
        kind: "fight",
        path: path.display().to_string(),
        error,
        detail: serde_json::json!({
            "source": document.source.as_str(),
            "seed": document.seed,
            "round": document.round,
            "compared": if trajectory { &["result", "trajectory"][..] } else { &["result"][..] },
            "differences": differences,
        }),
    };
    let layout = mechcore_document::canonical_yaml(mechcore_document::fight::project(&document))?;
    let simulated = match crate::convert::fought(|record| {
        mechcore_simulation::simulate_document(layout.as_bytes(), record, None)
    }) {
        Ok(simulated) => simulated.normalized(),
        Err(refused) => {
            return Ok(report(
                Some(format!(
                    "the simulator does not fight it: {}",
                    refused.reason()
                )),
                Vec::new(),
            ));
        }
    };
    let actual = mechcore_document::Fight {
        source: document.source,
        ticks: simulated.ticks.filter(|_| trajectory),
        hash: simulated.hash.clone().filter(|_| trajectory),
        ..simulated
    };
    let (error, differences) = compared(&document, &actual, "the simulator's fight")?;
    Ok(report(error, differences))
}

/// Where `actual` differs from what `document` states, and the error that
/// says so, naming who fought `actual`.
fn compared(
    document: &mechcore_document::Fight,
    actual: &mechcore_document::Fight,
    fought: &str,
) -> Result<(Option<String>, Vec<Difference>), String> {
    let differences = crate::diff::document_differences(document, actual)?
        .into_iter()
        .map(|difference| Difference {
            path: difference.path,
            expected: difference.left,
            actual: difference.right,
        })
        .collect::<Vec<_>>();
    if differences.is_empty() {
        return Ok((None, differences));
    }
    let paths = differences
        .iter()
        .map(|difference| difference.path.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let incomparable = document
        .hash
        .as_ref()
        .zip(actual.hash.as_ref())
        .filter(|(stated, computed)| stated.profile != computed.profile)
        .map(|(stated, computed)| {
            format!(
                "; the document's hash is under {} and this build computes {}, so the two \
                 hashes are not comparable",
                stated.profile, computed.profile
            )
        })
        .unwrap_or_default();
    Ok((
        Some(format!(
            "{fought} differs from the document in {paths}{incomparable}"
        )),
        differences,
    ))
}

/// Checks one file with the game fighting it.
async fn in_game(path: &Path, session: &Arc<Session>, update: bool) -> Report {
    match read(path) {
        Ok((Kind::Fight, bytes)) => fight_in_game(path, &bytes, session, update).await,
        Ok((Kind::Mcfr, _)) => recording_in_game(path, session, update).await,
        Ok(_) => Ok(check(path)),
        Err(error) => Err(error),
    }
    .unwrap_or_else(|error| Report::refused(path, error))
}

/// Records a fight document's projection with its seed in the game, and
/// compares the result it states with the one the game arrives at, as the
/// simulator's is compared. `--update` writes the game's fight over it,
/// keeping the comment at its top, and stating no build where it stated none.
async fn fight_in_game(
    path: &Path,
    bytes: &[u8],
    session: &Arc<Session>,
    update: bool,
) -> Result<Report, String> {
    let document = mechcore_document::fight::parse_yaml(bytes)?.normalized();
    let trajectory = document.source.has_trajectory();
    let staged = tempfile::Builder::new()
        .prefix("mechcore-verify-")
        .tempdir()
        .map_err(|error| format!("cannot stage the recording: {error}"))?;
    let layout = serde_json::to_value(mechcore_document::fight::project(&document))
        .map_err(|error| format!("cannot write the fight's layout: {error}"))?;
    let recorded = match record(layout, Vec::new(), staged.path(), session).await {
        Ok((recorded, _)) => recorded,
        Err(refused) => return Ok(game_report(path, "fight", Some(refused), &[], false)),
    };
    let actual = if update {
        recorded.clone()
    } else {
        mechcore_document::Fight {
            source: document.source,
            ticks: recorded.ticks.filter(|_| trajectory),
            hash: recorded.hash.clone().filter(|_| trajectory),
            ..recorded.clone()
        }
    };
    let (error, differences) = compared(&document, &actual, "the game's fight")?;
    if error.is_none() || !update {
        return Ok(game_report(path, "fight", error, &differences, false));
    }
    let text = String::from_utf8_lossy(bytes);
    let states_build = text.lines().any(|line| line.starts_with("game_build:"));
    let yaml = mechcore_document::fight::canonical_yaml(recorded.clone())?;
    let body: String = if states_build || recorded.game_build != document.game_build {
        yaml
    } else {
        yaml.lines()
            .filter(|line| !line.starts_with("game_build:"))
            .flat_map(|line| [line, "\n"])
            .collect()
    };
    let header: String = text
        .lines()
        .take_while(|line| line.starts_with('#') || line.trim().is_empty())
        .flat_map(|line| [line, "\n"])
        .collect();
    std::fs::write(path, header + &body)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    Ok(game_report(path, "fight", None, &differences, true))
}

/// Records a recording's own layout again in the game, with its seed and the
/// instrument channels it holds, and compares the two as fights. `--update`
/// replaces the file with the new recording, which is how a recording moves
/// to a new format: one that no longer reads as a fight is recorded again from
/// the layout it embeds all the same.
async fn recording_in_game(
    path: &Path,
    session: &Arc<Session>,
    update: bool,
) -> Result<Report, String> {
    let reader = crate::outcome::open(path).map_err(|failure| failure.reason().to_owned())?;
    let instrument = reader
        .instrument_channels()
        .map(|name| {
            serde_json::from_value(Value::String(name.to_owned()))
                .map_err(|_| format!("{name:?} is no instrument channel this build records"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let layout: Value = serde_yaml::from_str(reader.layout_yaml())
        .map_err(|error| format!("the recording's layout does not read: {error}"))?;
    let read = crate::outcome::read(&reader)
        .and_then(crate::outcome::Reading::fight)
        .map(mechcore_document::Fight::normalized)
        .map_err(|failure| failure.reason().to_owned());
    drop(reader);
    let staged = tempfile::Builder::new()
        .prefix(".mechcore-verify-")
        .tempdir_in(
            path.parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(Path::new(".")),
        )
        .map_err(|error| format!("cannot stage the recording: {error}"))?;
    let (recorded, written) = match record(layout, instrument, staged.path(), session).await {
        Ok(recorded) => recorded,
        Err(refused) => return Ok(game_report(path, "mcfr", Some(refused), &[], false)),
    };
    let (error, differences) = match read {
        Ok(found) => compared(&found, &recorded, "the game's fight")?,
        Err(unread) => (
            Some(format!("the recording does not read as a fight: {unread}")),
            Vec::new(),
        ),
    };
    if error.is_none() || !update {
        return Ok(game_report(path, "mcfr", error, &differences, false));
    }
    std::fs::rename(&written, path)
        .map_err(|error| format!("cannot replace {}: {error}", path.display()))?;
    Ok(game_report(path, "mcfr", None, &differences, true))
}

/// Records `layout`, which states its seed, in the game into `directory`, and
/// reads the recording back as a fight.
async fn record(
    layout: Value,
    instrument: Vec<mechcore_protocol::InstrumentChannel>,
    directory: &Path,
    session: &Arc<Session>,
) -> Result<(mechcore_document::Fight, PathBuf), String> {
    let output = directory.join("recording.mcfr");
    crate::game::Record::Layout {
        layout,
        seed: None,
        output: output.clone(),
        instrument,
    }
    .run(session, false)
    .await
    .map_err(|failure| format!("the game does not fight it: {}", failure.reason()))?;
    let recorded = crate::outcome::fight(&output)
        .map_err(|failure| failure.reason().to_owned())?
        .normalized();
    Ok((recorded, output))
}

fn game_report(
    path: &Path,
    kind: &'static str,
    error: Option<String>,
    differences: &[Difference],
    updated: bool,
) -> Report {
    Report {
        schema: SCHEMA,
        valid: error.is_none(),
        kind,
        path: path.display().to_string(),
        error,
        detail: serde_json::json!({
            "backend": "game",
            "updated": updated,
            "differences": differences,
        }),
    }
}

/// One field a fight document and the simulator's fight of it disagree on.
#[derive(Serialize)]
struct Difference {
    path: String,
    /// What the document states; absent when it states nothing there.
    #[serde(skip_serializing_if = "Option::is_none")]
    expected: Option<Value>,
    /// What the simulator arrived at; absent when it arrived at nothing there.
    #[serde(skip_serializing_if = "Option::is_none")]
    actual: Option<Value>,
}

/// Checks opening layouts and every reinforcement draw using seeded setup,
/// then measures how much of each next opening the transition predicts, and
/// fights the rounds in order, holding each result to the next opening.
/// Choices must name predicted offers; the source replay authenticates them.
///
/// A match verifies only when every leaf outside the fight is predicted and
/// agrees: a field no rule predicts yet fails it as surely as a wrong one.
fn verify_match(
    path: &Path,
    stated: &mechcore_document::opening::Stated,
) -> Result<Report, String> {
    let economy = mechcore_document::economy::Economy::embedded()?;
    let checked = mechcore_document::opening::verify(economy, stated).and_then(|opening| {
        mechcore_document::reinforcement::verify(economy, stated, &opening)
            .map(|reinforcements| (opening, reinforcements))
    });
    let coverage = mechcore_document::coverage::measure(
        economy,
        stated,
        match &checked {
            Ok((_, reinforcements)) => Ok(reinforcements),
            Err(error) => Err(error.as_str()),
        },
    );
    // Every round, projected from where it opens and from where it deploys,
    // has to be a layout the compiler takes: a match is what fights are run
    // from, so a round that cannot become one is not a match this build reads.
    let projected = checked
        .as_ref()
        .ok()
        .map(|(_, deal)| mechcore_document::project::every_round(economy, stated, deal));
    // Each round's fight, in order, until the first the simulator does not
    // fight or whose result is not the next round's opening.
    let fights = checked.as_ref().ok().map(|(_, deal)| {
        mechcore_document::coverage::fights(economy, stated, deal, |layout| {
            let yaml = mechcore_document::canonical_yaml(layout.clone())?;
            crate::convert::fought(|record| {
                mechcore_simulation::simulate_document(yaml.as_bytes(), record, None)
            })
            .map_err(|failure| failure.reason().to_owned())
        })
    });
    let error = match &checked {
        Err(error) => Some(error.clone()),
        Ok(_) if matches!(projected, Some(Err(_))) => projected.clone().and_then(Result::err),
        Ok(_) if !coverage.complete() => Some(format!(
            "transitions are not fully predicted: {} leaves unequal, {} unimplemented",
            coverage.total.unequal, coverage.total.unimplemented
        )),
        Ok(_) => fights.as_ref().and_then(|fights| {
            use mechcore_document::coverage::Stopped;
            fights.stopped.as_ref().map(|stopped| match stopped {
                Stopped::NotProjected { round, reason } => {
                    format!("round {round} is not fought: {reason}")
                }
                Stopped::Unsupported { round, reason } => {
                    format!("the simulator does not fight round {round}: {reason}")
                }
                Stopped::Differs { round, differences } => format!(
                    "round {round}'s fight is not what the match says it decided: {}",
                    differences
                        .iter()
                        .map(|difference| format!("{} {}", difference.side, difference.path))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            })
        }),
    };
    let found = checked.as_ref().ok();
    Ok(Report {
        schema: SCHEMA,
        valid: error.is_none(),
        kind: "match",
        path: path.display().to_string(),
        error,
        detail: serde_json::json!({
            "seed": stated.seed,
            "openings": 2,
            "map_id": stated.map_id,
            "prediction": found.map(|(opening, _)| opening),
            "reinforcement_rounds": found.map(|(_, checked)| checked.rounds.len()),
            "reinforcement_offers_checked": found.map(|(_, checked)| checked.offers_checked),
            "projected_layouts": projected.and_then(Result::ok),
            "reinforcements": found.map(|(_, checked)| &checked.rounds),
            "coverage": coverage,
            "fights": fights,
        }),
    })
}

const SCHEMA: &str = "mechcore.verify-result.v1";

/// What checking one file found.
#[derive(Serialize)]
pub(crate) struct Report {
    schema: &'static str,
    pub(crate) valid: bool,
    /// Which contract the file was checked against, or `unreadable` when it
    /// named none this build checks.
    kind: &'static str,
    pub(crate) path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
    #[serde(flatten)]
    detail: Value,
}

impl Report {
    fn refused(path: &Path, error: String) -> Self {
        Self {
            schema: SCHEMA,
            valid: false,
            kind: "unreadable",
            path: path.display().to_string(),
            error: Some(error),
            detail: Value::Null,
        }
    }
}
