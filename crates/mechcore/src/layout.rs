use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
};

use serde::Serialize;
use serde_json::Value;

pub(crate) fn run(mut arguments: impl Iterator<Item = String>) -> Result<bool, String> {
    match arguments.next().as_deref() {
        Some("verify") => verify(arguments).map(|()| true),
        Some("format") => format(arguments).map(|()| true),
        Some("diff") => diff(arguments),
        _ => Err("expected `verify <layout.yaml>`, `format <layout.yaml> [--write]`, or `diff <left.yaml> <right.yaml>`".into()),
    }
}

fn verify(mut arguments: impl Iterator<Item = String>) -> Result<(), String> {
    let path = required_path(&mut arguments, "expected layout.yaml after `verify`")?;
    reject_extra(&mut arguments)?;
    let layout = read_layout(&path)?;
    let plan = mechcore_layout::compile_layout(layout)?;
    let report = serde_json::json!({
        "valid": true,
        "layout": path,
        "seed": plan.seed,
        "map_id": plan.map_id,
        "round": plan.round,
        "formation_count": plan.formation_count(),
        "construction_count": plan.construction_count(),
        "contraption_count": plan.contraption_count(),
        "airdrop_shield_count": plan.airdrop_shield_count(),
        "terrain_count": plan.terrain_count(),
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&report)
            .map_err(|error| format!("cannot serialize verification report: {error}"))?
    );
    Ok(())
}

fn format(mut arguments: impl Iterator<Item = String>) -> Result<(), String> {
    let path = required_path(&mut arguments, "expected layout.yaml after `format`")?;
    let write = match arguments.next().as_deref() {
        None => false,
        Some("--write") => true,
        Some(extra) => return Err(format!("unexpected argument {extra:?}")),
    };
    reject_extra(&mut arguments)?;
    let canonical = mechcore_layout::canonical_yaml(read_layout(&path)?)?;
    if write {
        fs::write(&path, canonical)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    } else {
        print!("{canonical}");
    }
    Ok(())
}

fn diff(mut arguments: impl Iterator<Item = String>) -> Result<bool, String> {
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

fn read_layout(path: &PathBuf) -> Result<mechcore_layout::Layout, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    mechcore_layout::parse_yaml(&bytes)
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
