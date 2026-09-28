//! `diff`: what two files of one kind differ in.
//!
//! Two documents are normalized and compared field by field. Two recordings
//! are compared by their stored tick hashes, which cover every field, and then
//! field group by field group on every tick both hold.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use mechcore_mcfr::McfrReader;
use serde::Serialize;
use serde_json::Value;

use crate::cli::{Args, Failure, Format, Outcome};
use crate::difference::{self, Selection};
use crate::kind::Kind;

const COMPARE_SCHEMA: &str = "mechcore.fight-compare-result.v3";

/// Reads `diff <left> <right>` off a command line.
///
/// # Errors
///
/// Returns a usage failure for a command the contract does not define, and
/// a refusal for two files of different kinds or a kind `diff` does not take.
pub(crate) fn run(mut arguments: Args) -> Outcome {
    let format = arguments.format()?;
    let fields = arguments
        .value("--fields")?
        .map(|names| names.split(',').map(str::to_owned).collect::<Vec<_>>());
    let tick = arguments.parsed::<u32>("--tick", "a tick")?;
    let left = arguments.path("the file on the left")?;
    let right = arguments.path("the file on the right")?;
    arguments.finish()?;
    let (verdict, report) = diff(&left, &right, fields, tick)?;
    if format == Format::Text {
        if report["schema"] != COMPARE_SCHEMA {
            return Err(Failure::usage("a document diff has no text rendering"));
        }
        print_comparison(&report);
    } else {
        crate::cli::emit(&report, format)?;
    }
    Ok(verdict.into())
}

/// Compares two files of one kind, answering the verdict and the report.
///
/// `fields` and `tick` select and explain the groups of a recording, and a
/// document has neither.
///
/// # Errors
///
/// Returns a refusal for two kinds, a kind `diff` does not take, or options
/// the kind does not take, and whatever reading either file fails with.
pub(crate) fn diff(
    left: &Path,
    right: &Path,
    fields: Option<Vec<String>>,
    tick: Option<u32>,
) -> Result<(bool, Value), Failure> {
    let (kind, left_bytes) = Kind::read(left)?;
    let (other, right_bytes) = Kind::read(right)?;
    if kind != other {
        return Err(Failure::refused(format!(
            "{} is a {} file and {} a {} file; diff compares two of one kind",
            left.display(),
            kind.name(),
            right.display(),
            other.name()
        )));
    }
    kind.require("diff")?;
    if kind == Kind::Mcfr {
        let selection = Selection::of(fields.unwrap_or_default());
        return compare(left, right, &selection, tick).map_err(Failure::refused);
    }
    if fields.is_some() || tick.is_some() {
        return Err(Failure::usage(
            "--fields and --tick select the groups of a recording; a document has none",
        ));
    }
    let (schema, differences) = if kind == Kind::Fight {
        let parse = |bytes: &[u8]| {
            mechcore_document::fight::parse_yaml(bytes)
                .map(mechcore_document::Fight::normalized)
                .map_err(Failure::refused)
        };
        (
            "mechcore.fight-diff-result.v1",
            document_differences(&parse(&left_bytes)?, &parse(&right_bytes)?)
                .map_err(Failure::failed)?,
        )
    } else {
        let parse = |bytes: &[u8]| mechcore_document::parse_yaml(bytes).map_err(Failure::refused);
        (
            "mechcore.layout-diff-result.v2",
            layout_differences(parse(&left_bytes)?, parse(&right_bytes)?)
                .map_err(Failure::failed)?,
        )
    };
    let equal = differences.is_empty();
    let report = DiffReport {
        schema,
        equal,
        left: left.display().to_string(),
        right: right.display().to_string(),
        differences,
    };
    let report = serde_json::to_value(&report)
        .map_err(|error| Failure::failed(format!("cannot write the result: {error}")))?;
    Ok((equal, report))
}

/// Every field two layouts differ in, once both are in normal form.
///
/// # Errors
///
/// Returns an error when a layout cannot be serialized.
pub(crate) fn layout_differences(
    left: mechcore_document::Layout,
    right: mechcore_document::Layout,
) -> Result<Vec<FieldDifference>, String> {
    document_differences(&left.normalized(), &right.normalized())
}

/// Every field two documents in normal form differ in.
///
/// # Errors
///
/// Returns an error when a document cannot be serialized.
fn document_differences<T: Serialize>(left: &T, right: &T) -> Result<Vec<FieldDifference>, String> {
    let left = serde_json::to_value(left)
        .map_err(|error| format!("cannot normalize a document: {error}"))?;
    let right = serde_json::to_value(right)
        .map_err(|error| format!("cannot normalize a document: {error}"))?;
    let mut differences = Vec::new();
    collect_differences("", Some(&left), Some(&right), &mut differences);
    Ok(differences)
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
pub(crate) struct FieldDifference {
    pub(crate) path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) left: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) right: Option<Value>,
}

/// Compare two recordings, returning the verdict and the structured report.
///
/// Shared with `mechcore run`, whose `diff` step asserts on the same fields
/// `diff` prints. The verdict comes from the stored tick
/// hashes; `fields` then says, group by group, where
/// the two recordings differ, and `at` explains one tick of it: the first
/// divergence of the selected groups, or the tick asked for.
///
/// The verdict is the hashes' unless groups are selected, in which
/// case it is whether those groups agree on every tick both recordings hold.
pub(crate) fn compare(
    left_path: &Path,
    right_path: &Path,
    selection: &Selection,
    tick: Option<u32>,
) -> Result<(bool, serde_json::Value), String> {
    let left = McfrReader::open(left_path).map_err(|error| error.to_string())?;
    let right = McfrReader::open(right_path).map_err(|error| error.to_string())?;
    let first_divergence = left
        .first_divergence(&right)
        .map_err(|error| error.to_string())?;
    if first_divergence.is_none() && left.hashes().result_hash != right.hashes().result_hash {
        return Err("result hashes differ although every stored tick hash matches".into());
    }
    let fields = difference::fields(&left, &right, selection)?;
    let at = tick
        .or_else(|| fields.first_divergence())
        .map(|tick| difference::detail(&left, &right, tick, selection))
        .transpose()?;
    let equal = first_divergence.is_none();
    let verdict = if selection.is_everything() {
        equal
    } else {
        fields.equal()
    };
    let report = CompareReport {
        schema: COMPARE_SCHEMA,
        equal,
        left: RecordingSummary {
            result_hash: &left.hashes().result_hash,
            tick_count: left.tick_count(),
        },
        right: RecordingSummary {
            result_hash: &right.hashes().result_hash,
            tick_count: right.tick_count(),
        },
        first_divergence,
        compared_ticks: fields.compared_ticks,
        fields_equal: fields.equal(),
        fields: fields.nested(),
        at,
    };
    let report = serde_json::to_value(&report)
        .map_err(|error| format!("cannot serialize comparison: {error}"))?;
    Ok((verdict, report))
}

/// The same report, as a person reads it: the verdicts, each differing group
/// with the ticks it differs on, and the explained tick.
fn print_comparison(report: &serde_json::Value) {
    let agreed = |value: &serde_json::Value| {
        if value.as_bool() == Some(true) {
            "equal"
        } else {
            "different"
        }
    };
    println!(
        "hashes {}{}, {} ticks compared ({} left, {} right)",
        agreed(&report["equal"]),
        report["first_divergence"]
            .as_u64()
            .map_or(String::new(), |tick| format!(" from t{tick}")),
        report["compared_ticks"],
        report["left"]["tick_count"],
        report["right"]["tick_count"],
    );
    let mut groups = Vec::new();
    collect_groups("", &report["fields"], &mut groups);
    groups.sort_by_key(|(group, first, _, _)| (*first, group.clone()));
    if groups.is_empty() {
        println!("every selected field agrees on every tick");
    }
    for (group, first, last, count) in groups {
        let span = if first == last {
            format!("t{first}")
        } else {
            format!("t{first}..t{last}")
        };
        println!("  {group:<48} {span:<12} {count} tick(s)");
    }
    let at = &report["at"];
    if at.is_null() {
        return;
    }
    println!("\nat t{}:", at["tick"]);
    for shown in at["differences"].as_array().into_iter().flatten() {
        println!(
            "  {} {}: {} | {}",
            shown["object"].as_str().unwrap_or_default(),
            shown["field"].as_str().unwrap_or_default(),
            shown["left"].as_str().unwrap_or_default(),
            shown["right"].as_str().unwrap_or_default(),
        );
    }
    if let Some(further) = at["further"].as_u64() {
        println!("  and {further} more");
    }
    if let Some(references) = at["references"]
        .as_object()
        .filter(|found| !found.is_empty())
    {
        println!("named:");
        for (name, sides) in references {
            println!(
                "  {name}: {} | {}",
                sides["left"].as_str().unwrap_or_default(),
                sides["right"].as_str().unwrap_or_default()
            );
        }
    }
    for side in ["left", "right"] {
        println!("{side} events:");
        for line in at["events"][side].as_array().into_iter().flatten() {
            println!("  {}", line.as_str().unwrap_or_default());
        }
    }
}

/// Flattens the nested `fields` map back into `(group, first, last, ticks)`.
fn collect_groups(path: &str, node: &serde_json::Value, out: &mut Vec<(String, u64, u64, u64)>) {
    let Some(fields) = node.as_object() else {
        return;
    };
    if let (Some(first), Some(last), Some(count)) = (
        fields
            .get("first_divergence")
            .and_then(serde_json::Value::as_u64),
        fields
            .get("last_divergence")
            .and_then(serde_json::Value::as_u64),
        fields
            .get("divergent_ticks")
            .and_then(serde_json::Value::as_u64),
    ) {
        out.push((path.to_owned(), first, last, count));
    }
    for (name, inner) in fields {
        if inner.is_object() {
            let at = if path.is_empty() {
                name.clone()
            } else {
                format!("{path}.{name}")
            };
            collect_groups(&at, inner, out);
        }
    }
}

#[derive(Serialize)]
struct CompareReport<'a> {
    schema: &'static str,
    equal: bool,
    left: RecordingSummary<'a>,
    right: RecordingSummary<'a>,
    first_divergence: Option<u32>,
    compared_ticks: u32,
    fields_equal: bool,
    fields: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    at: Option<difference::Detail>,
}

#[derive(Serialize)]
struct RecordingSummary<'a> {
    result_hash: &'a str,
    tick_count: u32,
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
