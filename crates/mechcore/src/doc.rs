//! The `doc` namespace: what a command does to a document on disk.
//!
//! Its verbs are named for what they do rather than for one kind of document.
//! `format` and `diff` accept a layout, and a battle stream is refused by the
//! parser until those two verbs learn it. `verify` also checks deployment
//! recordings through the transition and battle opening and reinforcement
//! offers against the seeded random stream.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{IsTerminal, Read},
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::Value;

use crate::cli::{Args, Failure, Outcome, Verdict};

/// Dispatches one of the namespace's verbs.
///
/// # Errors
///
/// Returns a usage failure for a verb this namespace does not hold, and
/// whatever the verb returns otherwise.
pub(crate) fn run(mut arguments: Args) -> Outcome {
    match arguments
        .operand("a verb: verify, format or diff")?
        .as_str()
    {
        "verify" => verify(arguments),
        "format" => format(arguments),
        "diff" => diff(arguments),
        other => Err(Failure::usage(format!(
            "doc has no verb {other:?}; it has verify, format and diff"
        ))),
    }
}

/// Checks each named file against the contract its own kind defines.
///
/// A file says which kind it is, so nothing is inferred from an extension. A
/// layout is checked by the shared static compiler. A battle is checked against
/// its seed and by predicting each round's next opening from the one before.
///
/// Paths come from the arguments, or from standard input one per line when
/// there are none, so a batch is a pipe rather than a flag:
///
/// ```text
/// mechcore doc verify layout.yaml
/// ls tests/battle/*.yaml | mechcore doc verify
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
fn verify(mut arguments: Args) -> Outcome {
    let paths = inputs(&mut arguments)?;
    arguments.finish()?;
    let mut valid = true;
    for path in paths {
        let report = match verify_one(&path) {
            Ok(report) => report,
            Err(error) => VerifyReport::refused(&path, error),
        };
        valid &= report.valid;
        println!(
            "{}",
            serde_json::to_string(&report)
                .map_err(|error| Failure::failed(format!("cannot write the report: {error}")))?
        );
    }
    Ok(valid.into())
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
            "expected <document>... after `doc verify`, or paths on standard input",
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

fn verify_one(path: &Path) -> Result<VerifyReport, String> {
    // A directory names no document, and expanding one is the shell's job:
    // saying so beats an operating system error about a read that could not
    // have worked.
    if path.is_dir() {
        return Err(format!(
            "{} is a directory; name the files themselves, as a shell glob \
             or on standard input",
            path.display()
        ));
    }
    let bytes =
        fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if let Some(stated) = mechcore_document::opening::stated(&bytes)? {
        return verify_battle(path, &stated);
    }
    let plan = mechcore_document::compile_layout(mechcore_document::parse_yaml(&bytes)?)?;
    Ok(VerifyReport {
        schema: VERIFY_SCHEMA,
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
            "airdrop_shield_count": plan.airdrop_shield_count(),
            "terrain_count": plan.terrain_count(),
        }),
    })
}

/// Checks opening layouts and every reinforcement draw using seeded setup,
/// then measures how much of each next opening the transition predicts.
/// Choices must name predicted offers; the source replay authenticates them.
///
/// A battle verifies only when every leaf outside the fight is predicted and
/// agrees: a field no rule predicts yet fails it as surely as a wrong one.
fn verify_battle(
    path: &Path,
    stated: &mechcore_document::opening::Stated,
) -> Result<VerifyReport, String> {
    let economy = mechcore_document::economy::Economy::embedded()?;
    let checked = mechcore_document::opening::verify(&economy, stated).and_then(|opening| {
        mechcore_document::reinforcement::verify(&economy, stated, &opening)
            .map(|reinforcements| (opening, reinforcements))
    });
    let coverage = mechcore_document::coverage::measure(
        &economy,
        stated,
        match &checked {
            Ok((_, reinforcements)) => Ok(reinforcements),
            Err(error) => Err(error.as_str()),
        },
    );
    let error = match &checked {
        Err(error) => Some(error.clone()),
        Ok(_) if !coverage.complete() => Some(format!(
            "transitions are not fully predicted: {} leaves unequal, {} unimplemented",
            coverage.total.unequal, coverage.total.unimplemented
        )),
        Ok(_) => None,
    };
    let found = checked.as_ref().ok();
    Ok(VerifyReport {
        schema: VERIFY_SCHEMA,
        valid: error.is_none(),
        kind: "battle",
        path: path.display().to_string(),
        error,
        detail: serde_json::json!({
            "seed": stated.seed,
            "openings": 2,
            "map_id": stated.map_id,
            "prediction": found.map(|(opening, _)| opening),
            "reinforcement_rounds": found.map(|(_, checked)| checked.rounds.len()),
            "reinforcement_offers_checked": found.map(|(_, checked)| checked.offers_checked),
            "reinforcements": found.map(|(_, checked)| &checked.rounds),
            "coverage": coverage,
        }),
    })
}

const VERIFY_SCHEMA: &str = "mechcore.verify-result.v1";

/// What checking one file found.
#[derive(Serialize)]
struct VerifyReport {
    schema: &'static str,
    valid: bool,
    /// Which contract the file was checked against, or `unreadable` when it
    /// named none this build knows.
    kind: &'static str,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(flatten)]
    detail: Value,
}

impl VerifyReport {
    fn refused(path: &Path, error: String) -> Self {
        Self {
            schema: VERIFY_SCHEMA,
            valid: false,
            kind: "unreadable",
            path: path.display().to_string(),
            error: Some(error),
            detail: Value::Null,
        }
    }
}

fn format(mut arguments: Args) -> Outcome {
    let write = arguments.flag("--write")?;
    let path = arguments.path("a document to format")?;
    arguments.finish()?;
    let canonical =
        mechcore_document::canonical_yaml(read_layout(&path)?).map_err(Failure::refused)?;
    if write {
        fs::write(&path, canonical).map_err(|error| {
            Failure::failed(format!("cannot write {}: {error}", path.display()))
        })?;
    } else {
        print!("{canonical}");
    }
    Ok(Verdict::Yes)
}

fn diff(mut arguments: Args) -> Outcome {
    let format = arguments.format()?;
    let left_path = arguments.path("the document on the left")?;
    let right_path = arguments.path("the document on the right")?;
    arguments.finish()?;
    let left = read_layout(&left_path)?.normalized();
    let right = read_layout(&right_path)?.normalized();
    let left_value = serde_json::to_value(left).map_err(|error| {
        Failure::failed(format!("cannot normalize {}: {error}", left_path.display()))
    })?;
    let right_value = serde_json::to_value(right).map_err(|error| {
        Failure::failed(format!(
            "cannot normalize {}: {error}",
            right_path.display()
        ))
    })?;
    let mut differences = Vec::new();
    collect_differences("", Some(&left_value), Some(&right_value), &mut differences);
    let equal = differences.is_empty();
    let report = DiffReport {
        schema: "mechcore.layout-diff-result.v2",
        equal,
        left: left_path.display().to_string(),
        right: right_path.display().to_string(),
        differences,
    };
    crate::cli::emit(&report, format)?;
    Ok(equal.into())
}

fn read_layout(path: &PathBuf) -> Result<mechcore_document::Layout, Failure> {
    let bytes = fs::read(path)
        .map_err(|error| Failure::failed(format!("cannot read {}: {error}", path.display())))?;
    mechcore_document::parse_yaml(&bytes).map_err(Failure::refused)
}

/// Collections whose entries carry a cross-round deployment identity.
///
/// Aligning these by array position would report an object inserted in the
/// middle as a change to every later entry plus one removal at the end. Keying
/// by `index` instead reports what actually happened, which is what makes a
/// difference correspond to a decision rather than to a shift in the list.
const IDENTITY_KEYED_COLLECTIONS: [&str; 3] = ["units", "constructions", "contraptions"];

fn collect_differences(
    path: &str,
    left: Option<&Value>,
    right: Option<&Value>,
    output: &mut Vec<FieldDifference>,
) {
    match (left, right) {
        (Some(Value::Object(left)), Some(Value::Object(right))) => {
            let keys = left
                .keys()
                .chain(right.keys())
                .map(String::as_str)
                .collect::<BTreeSet<_>>();
            for key in keys {
                let child = format!("{path}/{}", escape_pointer(key));
                if IDENTITY_KEYED_COLLECTIONS.contains(&key)
                    && let (Some(Value::Array(left)), Some(Value::Array(right))) =
                        (left.get(key), right.get(key))
                    && let (Some(left), Some(right)) =
                        (indexed_entries(left), indexed_entries(right))
                {
                    for identity in left
                        .keys()
                        .chain(right.keys())
                        .copied()
                        .collect::<BTreeSet<_>>()
                    {
                        collect_differences(
                            &format!("{child}/index={identity}"),
                            left.get(&identity).copied(),
                            right.get(&identity).copied(),
                            output,
                        );
                    }
                    continue;
                }
                collect_differences(&child, left.get(key), right.get(key), output);
            }
        }
        (Some(Value::Array(left)), Some(Value::Array(right))) => {
            for index in 0..left.len().max(right.len()) {
                collect_differences(
                    &format!("{path}/{index}"),
                    left.get(index),
                    right.get(index),
                    output,
                );
            }
        }
        (Some(left), Some(right)) if left == right => {}
        (left, right) => output.push(FieldDifference {
            path: if path.is_empty() { "/" } else { path }.to_owned(),
            left: left.cloned(),
            right: right.cloned(),
        }),
    }
}

/// Keys one collection by its entries' `index`, or gives up so the caller falls
/// back to position. A compiled layout cannot repeat an index, so a duplicate
/// here means the value did not come through the shared compiler.
fn indexed_entries(entries: &[Value]) -> Option<BTreeMap<i64, &Value>> {
    let mut keyed = BTreeMap::new();
    for entry in entries {
        let index = entry.get("index")?.as_i64()?;
        if keyed.insert(index, entry).is_some() {
            return None;
        }
    }
    Some(keyed)
}

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

#[derive(Serialize)]
struct DiffReport {
    schema: &'static str,
    equal: bool,
    left: String,
    right: String,
    differences: Vec<FieldDifference>,
}

#[derive(Serialize)]
struct FieldDifference {
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    left: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    right: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_diff_uses_json_pointers_and_marks_missing_values() {
        let left = serde_json::json!({"blue": {"units": [1, 2]}});
        let right = serde_json::json!({"blue": {"units": [1, 3, 4]}});
        let mut differences = Vec::new();
        collect_differences("", Some(&left), Some(&right), &mut differences);
        assert_eq!(differences.len(), 2);
        assert_eq!(differences[0].path, "/blue/units/1");
        assert_eq!(differences[1].path, "/blue/units/2");
        assert!(differences[1].left.is_none());
    }

    #[test]
    fn placement_diff_aligns_by_deployment_index() {
        let entry = |index: i32, x: i32| serde_json::json!({"index": index, "position": {"x": x}});
        let left = serde_json::json!({"units": [entry(0, 0), entry(3, 40), entry(7, 80)]});
        let right = serde_json::json!({"units": [entry(0, 0), entry(7, 85), entry(9, 120)]});
        let mut differences = Vec::new();
        collect_differences("", Some(&left), Some(&right), &mut differences);

        // One removal, one field change, one addition: no entry is reported as
        // changed merely because a neighbour moved along the list.
        assert_eq!(differences.len(), 3);
        assert_eq!(differences[0].path, "/units/index=3");
        assert!(differences[0].right.is_none());
        assert_eq!(differences[1].path, "/units/index=7/position/x");
        assert_eq!(differences[2].path, "/units/index=9");
        assert!(differences[2].left.is_none());
    }

    #[test]
    fn placement_diff_falls_back_to_position_without_usable_indices() {
        let repeated = serde_json::json!({"index": 0, "position": {"x": 0}});
        let left = serde_json::json!({"units": [repeated, repeated]});
        let right = serde_json::json!({"units": [repeated, {"index": 0, "position": {"x": 5}}]});
        let mut differences = Vec::new();
        collect_differences("", Some(&left), Some(&right), &mut differences);
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].path, "/units/1/position/x");
    }
}
