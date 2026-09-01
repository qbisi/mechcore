use std::{collections::BTreeSet, fs, path::PathBuf};

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
        "round": plan.round,
        "formation_count": plan.formation_count(),
        "construction_count": plan.construction_count(),
        "contraption_count": plan.contraption_count(),
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
        schema: "mechcore.layout-diff-result.v1",
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
}
