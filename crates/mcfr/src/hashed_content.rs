//! What the content hash reads, pinned: every field of `S(t)` and `E(t)`,
//! traced from the types the writer encodes, listed in `hashed-content.txt`.
//! A change to that file is a change to what the hash admits.

use std::collections::BTreeMap;

use schemars::{JsonSchema, schema_for};
use serde_json::Value;

use crate::{TransitionEvents, WorldSnapshot};

const PINNED: &str = include_str!("../hashed-content.txt");

#[test]
fn the_hash_reads_exactly_the_pinned_fields() {
    let traced = listing();
    let pinned: Vec<&str> = PINNED.lines().filter(|line| !line.is_empty()).collect();
    let added: Vec<&str> = traced
        .iter()
        .map(String::as_str)
        .filter(|line| !pinned.contains(line))
        .collect();
    let removed: Vec<&str> = pinned
        .iter()
        .copied()
        .filter(|line| !traced.iter().any(|traced| traced == line))
        .collect();
    assert!(
        added.is_empty() && removed.is_empty(),
        "what the hash reads changed; hashed-content.txt admits it only by the \
         admission rules in docs/spec/mcfr/mcfr.md\nadded:\n  {}\nremoved:\n  {}",
        added.join("\n  "),
        removed.join("\n  "),
    );
}

/// One line per field, variant or value the hash reads,
/// sorted.
fn listing() -> Vec<String> {
    let mut definitions = BTreeMap::new();
    collect::<WorldSnapshot>(&mut definitions);
    collect::<TransitionEvents>(&mut definitions);
    let mut lines = Vec::new();
    for (name, schema) in &definitions {
        describe_definition(name, schema, &mut lines);
    }
    lines.sort();
    lines
}

fn collect<T: JsonSchema>(definitions: &mut BTreeMap<String, Value>) {
    let mut root = schema_for!(T).to_value();
    let object = root.as_object_mut().expect("a root schema is an object");
    if let Some(Value::Object(defs)) = object.remove("$defs") {
        definitions.extend(defs);
    }
    definitions.insert(T::schema_name().into_owned(), root);
}

fn describe_definition(name: &str, schema: &Value, lines: &mut Vec<String>) {
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        for (field, field_schema) in properties {
            lines.push(format!("{name}.{field}: {}", describe(field_schema)));
        }
    } else if let Some(variants) = schema.get("oneOf").and_then(Value::as_array) {
        for variant in variants {
            describe_variant(name, variant, lines);
        }
    } else if schema.get("enum").is_some() {
        describe_variant(name, schema, lines);
    } else {
        lines.push(format!("{name}: {}", describe(schema)));
    }
}

/// An enum's variant: a unit value (`const`, or `enum` when undocumented), or
/// an internally tagged one whose fields follow its `kind`.
fn describe_variant(name: &str, variant: &Value, lines: &mut Vec<String>) {
    if let Some(value) = variant.get("const") {
        lines.push(format!("{name}::{}", literal(value)));
        return;
    }
    if let Some(values) = variant.get("enum").and_then(Value::as_array) {
        lines.extend(
            values
                .iter()
                .map(|value| format!("{name}::{}", literal(value))),
        );
        return;
    }
    let properties = variant
        .get("properties")
        .and_then(Value::as_object)
        .expect("a tagged variant has properties");
    let tag = properties
        .get("kind")
        .and_then(|kind| kind.get("const"))
        .map(literal)
        .expect("a tagged variant names its kind");
    lines.push(format!("{name}::{tag}"));
    for (field, field_schema) in properties.iter().filter(|(field, _)| *field != "kind") {
        lines.push(format!("{name}::{tag}.{field}: {}", describe(field_schema)));
    }
}

fn describe(schema: &Value) -> String {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        return reference.rsplit('/').next().unwrap_or(reference).to_owned();
    }
    for alternatives in ["anyOf", "oneOf"] {
        if let Some(options) = schema.get(alternatives).and_then(Value::as_array) {
            return options.iter().map(describe).collect::<Vec<_>>().join(" | ");
        }
    }
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        return values.iter().map(literal).collect::<Vec<_>>().join(" | ");
    }
    let types: Vec<&str> = match schema.get("type") {
        Some(Value::String(kind)) => vec![kind],
        Some(Value::Array(kinds)) => kinds.iter().filter_map(Value::as_str).collect(),
        _ => return "any".to_owned(),
    };
    types
        .iter()
        .map(|kind| match *kind {
            "array" => format!(
                "[{}]",
                schema.get("items").map_or("any".to_owned(), describe)
            ),
            "null" => "null".to_owned(),
            _ => match schema.get("format").and_then(Value::as_str) {
                Some(format) => format!("{kind}({format})"),
                None => (*kind).to_owned(),
            },
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

fn literal(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

/// Every column of a per-tick table is a field the hash reads, so a table the
/// hash reads is read whole: a column the writer adds outside the model fails
/// here. The columns that place a row rather than describe it stand outside
/// the model and are named: every table's `tick`, and the events table's
/// `ordinal` and `type`, which are the event's place and its payload's
/// variant. `ticks` holds the hashes themselves. A struct the table stores
/// flattened into its owner's columns is named in `FLATTENED`.
#[test]
fn every_column_of_a_hashed_table_is_a_hashed_field() {
    use arrow_schema::{DataType, Fields};

    use crate::McfrTables;

    /// A model field the table stores as its fields' own columns in the
    /// owner's row: a weapon's nullable `pose` is its `position` and
    /// `rotation`, both null together.
    const FLATTENED: &[(&str, &str, &str)] = &[("WeaponState", "pose", "QPose")];

    fn walk(owner: &str, fields: &Fields, listing: &[String], at: &str, missing: &mut Vec<String>) {
        for field in fields {
            let path = format!("{at}.{}", field.name());
            let owner = FLATTENED
                .iter()
                .find(|(flattened_owner, flattened, inner)| {
                    *flattened_owner == owner
                        && listing
                            .iter()
                            .any(|line| line.starts_with(&format!("{owner}.{flattened}: ")))
                        && listing
                            .iter()
                            .any(|line| line.starts_with(&format!("{inner}.{}: ", field.name())))
                })
                .map_or(owner, |(_, _, inner)| inner);
            let prefix = format!("{owner}.{}: ", field.name());
            let Some(line) = listing.iter().find(|line| line.starts_with(&prefix)) else {
                missing.push(path);
                continue;
            };
            let mut data_type = field.data_type();
            while let DataType::List(item) = data_type {
                data_type = item.data_type();
            }
            if let DataType::Struct(children) = data_type {
                let named = line[prefix.len()..]
                    .trim_start_matches('[')
                    .split([']', ' '])
                    .next()
                    .unwrap_or_default();
                walk(named, children, listing, &path, missing);
            }
        }
    }

    let listing = listing();
    let mut missing = Vec::new();
    for (table, owner) in [
        ("units", "LiveUnitState"),
        ("rebirths", "RebirthState"),
        ("projectiles", "ProjectileState"),
        ("buildings", "BuildingState"),
        ("shields", "ShieldState"),
        ("terrains", "TerrainState"),
        ("statistics", "DamageStatistics"),
        ("formations", "FormationState"),
    ] {
        let schema = McfrTables::schema_of(table).expect("a per-tick table");
        let fields = schema
            .fields()
            .iter()
            .filter(|field| field.name() != "tick")
            .cloned()
            .collect::<Fields>();
        walk(owner, &fields, &listing, table, &mut missing);
    }
    // An event's references are `Event`'s, its subject stored as `object`, and
    // every payload column is a field of some variant's payload.
    let events = McfrTables::schema_of("events").expect("the events table");
    for field in events.fields() {
        let name = field.name().as_str();
        let owner = match name {
            "tick" | "ordinal" | "type" => continue,
            "object" => "Event.subject",
            "source" | "target" | "source_team_id" => "Event",
            _ => "EventPayload",
        };
        let read = match owner {
            "Event.subject" => listing
                .iter()
                .any(|line| line.starts_with("Event.subject: ")),
            "Event" => listing
                .iter()
                .any(|line| line.starts_with(&format!("Event.{name}: "))),
            _ => listing.iter().any(|line| {
                line.starts_with("EventPayload::") && line.contains(&format!(".{name}: "))
            }),
        };
        if !read {
            missing.push(format!("events.{name}"));
        }
    }
    assert!(
        missing.is_empty(),
        "columns of a hashed table the hash does not read: {missing:?}"
    );
}
