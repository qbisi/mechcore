//! The document commands: verify, format and diff.
//!
//! They are named for what they do to a document rather than for one kind.
//! `format` and `diff` accept a layout, and a state, turn or battle document is
//! refused by the parser until those two verbs learn the other kinds. `verify`
//! also checks deployment recordings through the transition and battle opening
//! offers against the seeded random stream.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{IsTerminal, Read},
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::Value;

/// Checks each named file against the contract its own kind defines.
///
/// A file says which kind it is, so nothing is inferred from an extension. A
/// layout is checked by the shared static compiler. A deployment recording is
/// checked by replaying every decision it holds through the transition, which
/// is a stronger statement than parsing it: the recording already parsed when
/// the Adapter published it, and what is open is whether this build reproduces
/// what the game did.
///
/// Paths come from the arguments, or from standard input one per line when
/// there are none, so a batch is a pipe rather than a flag:
///
/// ```text
/// mechcore verify layout.yaml
/// find work/replay-corpus/observations -name '*.jsonl' | mechcore verify
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
pub(crate) fn verify(arguments: impl Iterator<Item = String>) -> Result<bool, String> {
    let mut valid = true;
    for path in inputs(arguments)? {
        let report = match verify_one(&path) {
            Ok(report) => report,
            Err(error) => VerifyReport::refused(&path, error),
        };
        valid &= report.valid;
        println!(
            "{}",
            serde_json::to_string(&report)
                .map_err(|error| format!("cannot serialize verification report: {error}"))?
        );
    }
    Ok(valid)
}

/// The paths to check: the arguments, or standard input one per line.
///
/// An empty argument list with a terminal on standard input is a mistake rather
/// than an empty batch, so it is refused instead of succeeding over nothing.
fn inputs(arguments: impl Iterator<Item = String>) -> Result<Vec<PathBuf>, String> {
    let named: Vec<PathBuf> = arguments.map(PathBuf::from).collect();
    if !named.is_empty() {
        return Ok(named);
    }
    if std::io::stdin().is_terminal() {
        return Err("expected <document>... after `verify`, or paths on standard input".into());
    }
    let mut piped = String::new();
    std::io::stdin()
        .read_to_string(&mut piped)
        .map_err(|error| format!("cannot read paths from standard input: {error}"))?;
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
    if let Ok(text) = std::str::from_utf8(&bytes)
        && mechcore_document::observe::is_observation(text)?
    {
        return verify_observation(path, text);
    }
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
            "formation_count": plan.formation_count(),
            "construction_count": plan.construction_count(),
            "contraption_count": plan.contraption_count(),
            "airdrop_shield_count": plan.airdrop_shield_count(),
            "terrain_count": plan.terrain_count(),
        }),
    })
}

/// Checks that a battle's two openings are the ones its own seed deals.
///
/// The opening is the one decision a replay does not record the alternatives
/// for, so a converted battle states four combinations per side that nothing in
/// the file it came from carries. They are not invented: they are drawn from
/// the match's reinforcement stream, and that stream starts at the seed. So the
/// check is to deal them again from the seed and compare, which is what makes
/// the field evidence rather than decoration.
///
/// The pool draws from the same stream before the opening does, and by how much
/// is not yet settled, so the starting position is searched within a window
/// rather than computed. A match establishes consistency inside that window,
/// not the unique stream position or the player's recorded choice.
fn verify_battle(
    path: &Path,
    stated: &mechcore_document::opening::Stated,
) -> Result<VerifyReport, String> {
    let economy = mechcore_document::economy::Economy::embedded()?;
    let checked = mechcore_document::opening::verify(&economy, stated);
    let detail = |found: Option<&mechcore_document::opening::Verified>| {
        serde_json::json!({
            "seed": stated.seed,
            "openings": 2,
            "opening_offset": found.map(|found| found.offset),
            "opening_offsets": found.map(|found| found.matches.clone()),
            "opening_offers": found.map(|found| {
                serde_json::json!({
                    "blue": found.deal.blue.iter().copied().map(pair).collect::<Vec<_>>(),
                    "red": found.deal.red.iter().copied().map(pair).collect::<Vec<_>>(),
                })
            }),
        })
    };
    match checked {
        Ok(found) => Ok(VerifyReport {
            schema: VERIFY_SCHEMA,
            valid: true,
            kind: "battle",
            path: path.display().to_string(),
            error: None,
            detail: detail(Some(&found)),
        }),
        Err(error) => Ok(VerifyReport {
            schema: VERIFY_SCHEMA,
            valid: false,
            kind: "battle",
            path: path.display().to_string(),
            error: Some(error),
            detail: detail(None),
        }),
    }
}

fn pair(offer: mechcore_document::battle::OpeningOffer) -> Value {
    serde_json::json!({ "team": offer.team, "specialist": offer.specialist })
}

/// Replays a recording's decisions through the deployment transition.
///
/// Two checks, and the second is not implied by the first. Each decision is
/// applied to the position it was taken from and every field of the result
/// compared; then each round's collapsed sequence is applied to the position
/// the round opened with and compared against the one it closed with. A
/// decision this build's tables cannot settle is counted apart under the reason
/// it gave, and neither counted as reproduced nor as failed.
fn verify_observation(path: &Path, text: &str) -> Result<VerifyReport, String> {
    let economy = mechcore_document::economy::Economy::embedded()?;
    let records = mechcore_document::observe::read(text)?;
    let steps = mechcore_document::oracle::step_check(&economy, &records)?;
    let deployments = mechcore_document::oracle::round_check(&economy, &records)?;
    let unsettled: serde_json::Map<String, Value> = steps
        .unsettled
        .iter()
        .map(|(reason, count)| (reason.clone(), Value::from(*count)))
        .collect();
    Ok(VerifyReport {
        schema: VERIFY_SCHEMA,
        valid: steps.failed == 0 && deployments.failed == 0,
        kind: "observation",
        path: path.display().to_string(),
        error: None,
        detail: serde_json::json!({
            "decisions": steps.checked(),
            "decisions_closed": steps.closed,
            "decisions_unsettled": unsettled,
            "retractions": steps.retractions,
            "deployments": deployments.checked(),
            "deployments_closed": deployments.closed,
            "deployments_unsettled": deployments.unsettled,
            "failures": steps
                .failures
                .iter()
                .map(|failure| {
                    serde_json::json!({
                        "sequence": failure.sequence,
                        "round": failure.round,
                        "side": failure.side,
                        "action": failure.native_type,
                        "field": failure.field,
                        "produced": failure.expected,
                        "reached": failure.actual,
                    })
                })
                .chain(deployments.failures.iter().map(|failure| {
                    serde_json::json!({
                        "round": failure.round,
                        "side": failure.side,
                        "action": "deployment",
                        "field": failure.field,
                        "produced": failure.expected,
                        "reached": failure.actual,
                    })
                }))
                .collect::<Vec<_>>(),
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

pub(crate) fn format(mut arguments: impl Iterator<Item = String>) -> Result<(), String> {
    let path = required_path(&mut arguments, "expected layout.yaml after `format`")?;
    let write = match arguments.next().as_deref() {
        None => false,
        Some("--write") => true,
        Some(extra) => return Err(format!("unexpected argument {extra:?}")),
    };
    reject_extra(&mut arguments)?;
    let canonical = mechcore_document::canonical_yaml(read_layout(&path)?)?;
    if write {
        fs::write(&path, canonical)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    } else {
        print!("{canonical}");
    }
    Ok(())
}

pub(crate) fn diff(mut arguments: impl Iterator<Item = String>) -> Result<bool, String> {
    let left_path = required_path(&mut arguments, "expected left.yaml after `diff`")?;
    let right_path = required_path(&mut arguments, "expected right.yaml after left.yaml")?;
    reject_extra(&mut arguments)?;
    let left = read_layout(&left_path)?.normalized();
    let right = read_layout(&right_path)?.normalized();
    let left_value = serde_json::to_value(left)
        .map_err(|error| format!("cannot normalize {}: {error}", left_path.display()))?;
    let right_value = serde_json::to_value(right)
        .map_err(|error| format!("cannot normalize {}: {error}", right_path.display()))?;
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
    println!(
        "{}",
        serde_json::to_string_pretty(&report)
            .map_err(|error| format!("cannot serialize layout diff: {error}"))?
    );
    Ok(equal)
}

fn read_layout(path: &PathBuf) -> Result<mechcore_document::Layout, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    mechcore_document::parse_yaml(&bytes)
}

fn required_path(
    arguments: &mut impl Iterator<Item = String>,
    message: &str,
) -> Result<PathBuf, String> {
    arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| message.into())
}

fn reject_extra(arguments: &mut impl Iterator<Item = String>) -> Result<(), String> {
    if let Some(extra) = arguments.next() {
        Err(format!("unexpected argument {extra:?}"))
    } else {
        Ok(())
    }
}

/// Collections whose entries carry a cross-round deployment identity.
///
/// Aligning these by array position would report an object inserted in the
/// middle as a change to every later entry plus one removal at the end. Keying
/// by `index` instead reports what actually happened, which is what makes a
/// difference correspond to a decision rather than to a shift in the list.
const IDENTITY_KEYED_COLLECTIONS: [&str; 3] = ["formations", "constructions", "contraptions"];

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
        let left = serde_json::json!({"sides": {"blue": {"units": [1, 2]}}});
        let right = serde_json::json!({"sides": {"blue": {"units": [1, 3, 4]}}});
        let mut differences = Vec::new();
        collect_differences("", Some(&left), Some(&right), &mut differences);
        assert_eq!(differences.len(), 2);
        assert_eq!(differences[0].path, "/sides/blue/units/1");
        assert_eq!(differences[1].path, "/sides/blue/units/2");
        assert!(differences[1].left.is_none());
    }

    #[test]
    fn placement_diff_aligns_by_deployment_index() {
        let entry = |index: i32, x: i32| serde_json::json!({"index": index, "position": {"x": x}});
        let left = serde_json::json!({"formations": [entry(0, 0), entry(3, 40), entry(7, 80)]});
        let right = serde_json::json!({"formations": [entry(0, 0), entry(7, 85), entry(9, 120)]});
        let mut differences = Vec::new();
        collect_differences("", Some(&left), Some(&right), &mut differences);

        // One removal, one field change, one addition: no entry is reported as
        // changed merely because a neighbour moved along the list.
        assert_eq!(differences.len(), 3);
        assert_eq!(differences[0].path, "/formations/index=3");
        assert!(differences[0].right.is_none());
        assert_eq!(differences[1].path, "/formations/index=7/position/x");
        assert_eq!(differences[2].path, "/formations/index=9");
        assert!(differences[2].left.is_none());
    }

    #[test]
    fn placement_diff_falls_back_to_position_without_usable_indices() {
        let repeated = serde_json::json!({"index": 0, "position": {"x": 0}});
        let left = serde_json::json!({"formations": [repeated, repeated]});
        let right =
            serde_json::json!({"formations": [repeated, {"index": 0, "position": {"x": 5}}]});
        let mut differences = Vec::new();
        collect_differences("", Some(&left), Some(&right), &mut differences);
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].path, "/formations/1/position/x");
    }
}
