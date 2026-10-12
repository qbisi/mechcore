//! How the documents are spelled.
//!
//! A layout and a match segment are written by the same three rules, stated in
//! `docs/spec/document/layout.md` and `docs/spec/document/match.md`. They
//! decide spelling by the shape of a value and never by its field, so one
//! document has one byte sequence and a new field needs no rule of its own.
//!
//! `serde_yaml` has no per-field style, so the documents are written here from
//! the value `serde_yaml` would have written. A list item is what a document
//! holds by the dozen or the thousand, whether an action, a formation or a
//! terrain, and one line each keeps a document on a screen and makes a diff
//! name the item that changed. A coordinate pair and an allocator are scalar
//! mappings, so they fold too; a side, a shop or a technology list mixes
//! shapes and stays a block.

use serde_yaml::Value;

/// Writes one document in the normal form's spelling.
///
/// Three rules cover every value, and none names a field:
///
/// - a sequence item is written on one line, in flow style;
/// - a mapping or sequence whose members are all scalars is written in flow
///   style on its key's line;
/// - every other value is written in block style.
///
/// # Errors
///
/// Returns an error when the document is not a mapping, or a scalar cannot be
/// quoted.
pub(crate) fn document(root: &Value) -> Result<String, String> {
    let Value::Mapping(fields) = root else {
        return Err("a document is a mapping".into());
    };
    let mut out = String::new();
    block_mapping(fields, 0, &mut out)?;
    Ok(out)
}

/// Fight results use the layout's spelling; assertions keep SQL literal
/// and each answer row on its own line, preserving the SQL string exactly.
pub(crate) fn fight(root: &Value) -> Result<String, String> {
    let Value::Mapping(fields) = root else {
        return Err("a document is a mapping".into());
    };
    let mut out = String::new();
    for (key, value) in fields {
        if key.as_str() != Some("asserts") {
            let mut one = serde_yaml::Mapping::new();
            one.insert(key.clone(), value.clone());
            block_mapping(&one, 0, &mut out)?;
            continue;
        }
        let items = value.as_sequence().ok_or("asserts is not a sequence")?;
        out.push_str("asserts:\n");
        for item in items {
            let fields = item.as_mapping().ok_or("an assert is not a mapping")?;
            let sql = fields
                .get("sql")
                .and_then(Value::as_str)
                .ok_or("an assert has no SQL")?;
            if sql
                .chars()
                .any(|character| character.is_control() && !matches!(character, '\n' | '\t'))
            {
                // YAML literal blocks normalize carriage returns; quote these
                // strings so formatting cannot change the SQL value.
                out.push_str("- sql: ");
                flow(&Value::String(sql.to_owned()), &mut out)?;
                out.push('\n');
            } else {
                let ending =
                    if sql.ends_with("\n\n") || (sql.trim().is_empty() && sql.ends_with('\n')) {
                        "+"
                    } else if sql.ends_with('\n') {
                        ""
                    } else {
                        "-"
                    };
                let indentation = if sql
                    .lines()
                    .find(|line| !line.is_empty())
                    .is_none_or(|line| line.starts_with([' ', '\t']))
                {
                    "2"
                } else {
                    ""
                };
                out.push_str("- sql: |");
                out.push_str(indentation);
                out.push_str(ending);
                out.push('\n');
                for line in sql.split_terminator('\n') {
                    out.push_str("    ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
            if let Some(rows) = fields.get("rows") {
                let rows = rows.as_sequence().ok_or("assert rows is not a sequence")?;
                if rows.is_empty() {
                    out.push_str("  rows: []\n");
                } else {
                    out.push_str("  rows:\n");
                    for row in rows {
                        out.push_str("  - ");
                        flow(row, &mut out)?;
                        out.push('\n');
                    }
                }
            }
        }
    }
    Ok(out)
}

fn block_mapping(
    fields: &serde_yaml::Mapping,
    indent: usize,
    out: &mut String,
) -> Result<(), String> {
    let pad = " ".repeat(indent);
    for (key, value) in fields {
        out.push_str(&pad);
        flow(key, out)?;
        out.push(':');
        match value {
            Value::Mapping(inner) if !is_flat(value) => {
                out.push('\n');
                block_mapping(inner, indent + 2, out)?;
            }
            Value::Sequence(items) if !is_flat(value) => {
                out.push('\n');
                for item in items {
                    out.push_str(&pad);
                    out.push_str("- ");
                    flow(item, out)?;
                    out.push('\n');
                }
            }
            _ => {
                out.push(' ');
                flow(value, out)?;
                out.push('\n');
            }
        }
    }
    Ok(())
}

/// Whether a value is a scalar, or a collection holding only scalars.
fn is_flat(value: &Value) -> bool {
    let scalar = |value: &Value| {
        matches!(
            value,
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
        )
    };
    match value {
        Value::Sequence(items) => items.iter().all(scalar),
        Value::Mapping(fields) => fields.values().all(scalar),
        Value::Tagged(_) => false,
        _ => true,
    }
}

/// Writes a YAML value in flow style.
pub(crate) fn flow(value: &Value, out: &mut String) -> Result<(), String> {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        Value::Number(value) => out.push_str(&value.to_string()),
        Value::String(value) => {
            // An identifier, a gauge such as `124/450`, a team such as
            // `vortex-fire_badger`, a version such as `0.0.0.0.1` or a hash
            // such as `23:e4ed…` is written bare, unless the reader would take it for something
            // other than this string, as it would `true`, `null`, `12` or
            // `1.5`.
            let plain = value
                .chars()
                .next()
                .is_some_and(|first| first.is_ascii_alphanumeric())
                && value.chars().all(|character| {
                    character.is_ascii_alphanumeric()
                        || matches!(character, '_' | '/' | '-' | '.' | ':')
                })
                && serde_yaml::from_str::<Value>(value).is_ok_and(|read| read == *value.as_str());
            if plain {
                out.push_str(value);
            } else {
                out.push_str(
                    &serde_json::to_string(value)
                        .map_err(|error| format!("cannot quote {value:?}: {error}"))?,
                );
            }
        }
        Value::Sequence(items) => {
            out.push('[');
            for (at, item) in items.iter().enumerate() {
                if at > 0 {
                    out.push_str(", ");
                }
                flow(item, out)?;
            }
            out.push(']');
        }
        Value::Mapping(fields) => {
            out.push('{');
            for (at, (key, field)) in fields.iter().enumerate() {
                if at > 0 {
                    out.push_str(", ");
                }
                flow(key, out)?;
                out.push_str(": ");
                flow(field, out)?;
            }
            out.push('}');
        }
        Value::Tagged(tagged) => {
            out.push_str(&tagged.tag.to_string());
            out.push(' ');
            flow(&tagged.value, out)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_yaml::Value;

    fn written(value: &str) -> String {
        let mut out = String::new();
        super::flow(&Value::String(value.to_owned()), &mut out).unwrap();
        out
    }

    /// A string is written bare when it reads back as itself, which is what
    /// keeps a name, a gauge and a build readable and a number quoted.
    #[test]
    fn a_string_is_bare_only_when_it_reads_back_as_itself() {
        for bare in [
            "marksman",
            "vortex-fire_badger",
            "124/450",
            "0.0.0.0.1",
            "1a",
            "23:e4ed",
        ] {
            assert_eq!(written(bare), bare);
        }
        for quoted in ["12", "1.5", "true", "null", "", "a b", "a: b", "a:"] {
            assert_eq!(
                written(quoted),
                format!("{quoted:?}"),
                "{quoted} must be quoted"
            );
        }
    }
}
