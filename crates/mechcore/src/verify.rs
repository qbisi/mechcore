//! `verify`: checks each file against the contract its own kind defines.
//!
//! A layout is checked by the shared static compiler. A match is checked
//! against its seed and by predicting each round's next opening from the one
//! before. A recording is checked by simulating the layout it embeds again and
//! comparing the result with what it holds. A fight document is checked by
//! fighting its projection with its seed, as `convert --to fight` does, and
//! comparing the result it states with the one the simulator arrives at.

use std::{
    io::{IsTerminal, Read},
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::Value;

use crate::cli::{Args, Failure, Outcome};
use crate::kind::Kind;

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
/// # Errors
///
/// Returns an error when no input is named at all, or when the list of paths
/// cannot be read.
pub(crate) fn run(mut arguments: Args) -> Outcome {
    let paths = inputs(&mut arguments)?;
    arguments.finish()?;
    let mut valid = true;
    for path in paths {
        let report = check(&path);
        valid &= report.valid;
        println!(
            "{}",
            serde_json::to_string(&report)
                .map_err(|error| Failure::failed(format!("cannot write the report: {error}")))?
        );
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
    let simulated = match crate::convert::fought(|recording| {
        mechcore_simulation::simulate_document(layout.as_bytes(), Some(recording), None)
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
    let differences = crate::diff::document_differences(&document, &actual)?
        .into_iter()
        .map(|difference| Difference {
            path: difference.path,
            expected: difference.left,
            actual: difference.right,
        })
        .collect::<Vec<_>>();
    if differences.is_empty() {
        return Ok(report(None, differences));
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
    let error = format!("the simulator's fight differs from the document in {paths}{incomparable}");
    Ok(report(Some(error), differences))
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
/// then measures how much of each next opening the transition predicts.
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
    let error = match &checked {
        Err(error) => Some(error.clone()),
        Ok(_) if matches!(projected, Some(Err(_))) => projected.clone().and_then(Result::err),
        Ok(_) if !coverage.complete() => Some(format!(
            "transitions are not fully predicted: {} leaves unequal, {} unimplemented",
            coverage.total.unequal, coverage.total.unimplemented
        )),
        Ok(_) => None,
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
