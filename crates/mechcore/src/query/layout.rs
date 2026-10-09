//! The layout a recording embeds, as tables beside the recording's own.
//!
//! A layout is a document, not a timeline, so it is laid out by its own shape
//! rather than by [`super::flatten`]'s rule. What it adds that the recording
//! does not hold is which recorded formation is which placement:
//! `layout_units.formation_id` is [`scene::formations`]'s matching, the one
//! every reader of a recording names formations by, so a question about a
//! formation can name it by its side, its placement and its unit type.

use mechcore_document::Position;
use mechcore_mcfr::{McfrTables, WorldSnapshot};
use rusqlite::{Transaction, params};
use serde_json::Value;

use crate::{scene, turn::Side};

/// The tables this module fills, which a statement reading any of them fills
/// all of.
pub(super) const TABLES: &[&str] = &[
    "layout",
    "layout_sides",
    "layout_units",
    "layout_units__equipment",
    "layout_constructions",
    "layout_contraptions",
    "layout_choices",
];

/// The columns each layout table is keyed by.
pub(super) fn key(table: &str) -> &'static [&'static str] {
    match table {
        "layout_sides" => &["side"],
        "layout_units" | "layout_constructions" | "layout_contraptions" => &["side", "placement"],
        "layout_units__equipment" => &["side", "placement", "ordinal"],
        "layout_choices" => &["side", "list", "unit", "ordinal"],
        _ => &[],
    }
}

pub(super) const CREATE: &str = "
CREATE TABLE layout (game_build TEXT NOT NULL, map_id INTEGER, seed INTEGER, round INTEGER NOT NULL);
CREATE TABLE layout_sides (side TEXT NOT NULL, team_id INTEGER NOT NULL, legacy_index INTEGER NOT NULL);
CREATE TABLE layout_units (side TEXT NOT NULL, team_id INTEGER NOT NULL, placement INTEGER NOT NULL,
  name TEXT NOT NULL, x INTEGER NOT NULL, y INTEGER NOT NULL, level INTEGER,
  exp__current INTEGER, exp__maximum INTEGER, rotated INTEGER, travelling INTEGER,
  formation_id INTEGER);
CREATE TABLE layout_units__equipment (side TEXT NOT NULL, placement INTEGER NOT NULL,
  ordinal INTEGER NOT NULL, value TEXT NOT NULL);
CREATE TABLE layout_constructions (side TEXT NOT NULL, team_id INTEGER NOT NULL,
  placement INTEGER NOT NULL, name TEXT NOT NULL, x INTEGER NOT NULL, y INTEGER NOT NULL);
CREATE TABLE layout_contraptions (side TEXT NOT NULL, team_id INTEGER NOT NULL,
  placement INTEGER NOT NULL, name TEXT NOT NULL, x INTEGER NOT NULL, y INTEGER NOT NULL);
CREATE TABLE layout_choices (side TEXT NOT NULL, team_id INTEGER NOT NULL, list TEXT NOT NULL,
  unit TEXT, ordinal INTEGER NOT NULL, value NOT NULL);
";

/// Fills every layout table. A formation the matching names no placement for,
/// or every formation where the matching refuses, has a null `formation_id`;
/// the refusal is kept in `meta` as `layout_formations_refused`.
pub(super) fn fill(transaction: &Transaction<'_>, recording: &McfrTables) -> Result<(), String> {
    let layout = mechcore_document::parse_yaml(recording.layout_yaml().as_bytes())
        .map_err(|error| format!("the embedded layout does not read: {error}"))?;
    let opened = WorldSnapshot {
        live_units: recording
            .live_units(scene::FIRST_TICK)
            .map_err(|error| error.to_string())?,
        ..WorldSnapshot::default()
    };
    let formations = match scene::formations(&layout, &opened) {
        Ok(formations) => formations,
        Err(failure) => {
            transaction
                .execute(
                    "INSERT INTO meta (key, value) VALUES ('layout_formations_refused', ?1)",
                    [failure.reason()],
                )
                .map_err(|error| error.to_string())?;
            std::collections::BTreeMap::new()
        }
    };
    let sql = |error: rusqlite::Error| error.to_string();
    transaction
        .execute(
            "INSERT INTO layout VALUES (?1, ?2, ?3, ?4)",
            params![layout.game_build, layout.map_id, layout.seed, layout.round],
        )
        .map_err(sql)?;
    for side in Side::BOTH {
        let placed = scene::side_of(&layout, side);
        transaction
            .execute(
                "INSERT INTO layout_sides VALUES (?1, ?2, ?3)",
                params![side.name(), team(side), placed.legacy_index],
            )
            .map_err(sql)?;
        units(transaction, side, placed, &formations)?;
        statics(transaction, side, placed)?;
        choices(transaction, side, placed)?;
    }
    Ok(())
}

/// One side's formation placements and what each wears.
fn units(
    transaction: &Transaction<'_>,
    side: Side,
    placed: &mechcore_document::Side,
    formations: &std::collections::BTreeMap<u64, (Side, i32)>,
) -> Result<(), String> {
    let sql = |error: rusqlite::Error| error.to_string();
    let team = team(side);
    {
        for unit in &placed.units {
            let formation = formations
                .iter()
                .find(|(_, named)| **named == (side, unit.index))
                .map(|(formation, _)| *formation);
            transaction
                .execute(
                    "INSERT INTO layout_units VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    params![
                        side.name(),
                        team,
                        unit.index,
                        unit.type_name,
                        unit.position.x,
                        unit.position.y,
                        unit.level,
                        unit.exp.map(|exp| exp.current),
                        unit.exp.map(|exp| exp.maximum),
                        unit.rotated,
                        unit.travelling,
                        formation.map(i64::try_from).transpose().map_err(|_| "formation id")?,
                    ],
                )
                .map_err(sql)?;
        }
        let worn = serde_json::to_value(&placed.units).map_err(|error| error.to_string())?;
        for (unit, worn) in placed
            .units
            .iter()
            .zip(worn.as_array().into_iter().flatten())
        {
            for (ordinal, value) in names(worn.get("equipment")).into_iter().enumerate() {
                transaction
                    .execute(
                        "INSERT INTO layout_units__equipment VALUES (?1, ?2, ?3, ?4)",
                        params![
                            side.name(),
                            unit.index,
                            i64::try_from(ordinal).map_err(|_| "ordinal")?,
                            value
                        ],
                    )
                    .map_err(sql)?;
            }
        }
    }
    Ok(())
}

/// One side's construction and contraption placements.
fn statics(
    transaction: &Transaction<'_>,
    side: Side,
    placed: &mechcore_document::Side,
) -> Result<(), String> {
    let team = team(side);
    {
        let statics = placed
            .constructions
            .iter()
            .map(|placement| {
                (
                    "layout_constructions",
                    &placement.type_name,
                    placement.index,
                    placement.position,
                )
            })
            .chain(placed.contraptions.iter().map(|placement| {
                (
                    "layout_contraptions",
                    &placement.type_name,
                    placement.index,
                    placement.position,
                )
            }));
        for (table, name, index, Position { x, y }) in statics {
            transaction
                .execute(
                    &format!("INSERT INTO {table} VALUES (?1, ?2, ?3, ?4, ?5, ?6)"),
                    params![side.name(), team, index, name, x, y],
                )
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

const fn team(side: Side) -> u32 {
    match side {
        Side::Blue => 0,
        Side::Red => 1,
    }
}

/// Every list of names or numbers a side states, as the document writes it:
/// its officers, blueprints and the like, and its technologies by the unit
/// type they belong to. What a side places is in the other tables.
fn choices(
    transaction: &Transaction<'_>,
    side: Side,
    placed: &mechcore_document::Side,
) -> Result<(), String> {
    let written = serde_json::to_value(placed).map_err(|error| error.to_string())?;
    let Some(fields) = written.as_object() else {
        return Ok(());
    };
    let insert = |list: &str, unit: Option<&str>, ordinal: usize, value: &Value| {
        let value = match value {
            Value::String(text) => rusqlite::types::Value::Text(text.clone()),
            Value::Number(number) => number.as_i64().map_or_else(
                || rusqlite::types::Value::Real(number.as_f64().unwrap_or_default()),
                rusqlite::types::Value::Integer,
            ),
            _ => return Ok(()),
        };
        transaction
            .execute(
                "INSERT INTO layout_choices VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    side.name(),
                    team(side),
                    list,
                    unit,
                    i64::try_from(ordinal).map_err(|_| "ordinal".to_owned())?,
                    value
                ],
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    };
    for (list, value) in fields {
        match value {
            Value::Array(items) if items.iter().all(scalar) => {
                for (ordinal, item) in items.iter().enumerate() {
                    insert(list, None, ordinal, item)?;
                }
            }
            Value::Object(groups)
                if groups.values().all(|group| {
                    group
                        .as_array()
                        .is_some_and(|items| items.iter().all(scalar))
                }) =>
            {
                for (unit, group) in groups {
                    for (ordinal, item) in group.as_array().into_iter().flatten().enumerate() {
                        insert(list, Some(unit), ordinal, item)?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

const fn scalar(value: &Value) -> bool {
    matches!(value, Value::String(_) | Value::Number(_))
}

fn names(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str().map(str::to_owned))
        .collect()
}
