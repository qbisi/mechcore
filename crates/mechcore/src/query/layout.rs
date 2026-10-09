//! Which recorded formation is which placement of the layout a recording
//! embeds.
//!
//! A recording numbers its formations by where their members stand, so a
//! question that names a formation by its side, placement and unit type needs
//! the layout beside it. `layout_units.formation_id` is [`scene::formations`]'s
//! matching, the one every reader of a recording names formations by. The
//! rest of the layout is read as it is written, from `meta`'s `layout.yaml`;
//! these tables hold no more of it than naming a formation takes.

use mechcore_mcfr::{McfrTables, WorldSnapshot};
use rusqlite::{Transaction, params};

use crate::{scene, turn::Side};

/// The tables this module fills, which a statement reading either fills both
/// of.
pub(super) const TABLES: &[&str] = &["layout", "layout_units"];

/// The columns each layout table is keyed by.
pub(super) fn key(table: &str) -> &'static [&'static str] {
    match table {
        "layout_units" => &["side", "placement"],
        _ => &[],
    }
}

pub(super) const CREATE: &str = "
CREATE TABLE layout (game_build TEXT NOT NULL, map_id INTEGER, seed INTEGER, round INTEGER NOT NULL);
CREATE TABLE layout_units (side TEXT NOT NULL, team_id INTEGER NOT NULL, placement INTEGER NOT NULL,
  name TEXT NOT NULL, formation_id INTEGER);
";

/// Fills both tables. A placement the matching names no formation for, or
/// every placement where the matching refuses, has a null `formation_id`; the
/// refusal is kept in `meta` as `layout_formations_refused`.
pub(super) fn fill(transaction: &Transaction<'_>, recording: &McfrTables) -> Result<(), String> {
    let sql = |error: rusqlite::Error| error.to_string();
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
                .map_err(sql)?;
            std::collections::BTreeMap::new()
        }
    };
    transaction
        .execute(
            "INSERT INTO layout VALUES (?1, ?2, ?3, ?4)",
            params![layout.game_build, layout.map_id, layout.seed, layout.round],
        )
        .map_err(sql)?;
    for side in Side::BOTH {
        for unit in scene::units_of(&layout, side) {
            let formation = formations
                .iter()
                .find(|(_, named)| **named == (side, unit.index))
                .map(|(formation, _)| i64::try_from(*formation))
                .transpose()
                .map_err(|_| "formation id")?;
            transaction
                .execute(
                    "INSERT INTO layout_units VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        side.name(),
                        team(side),
                        unit.index,
                        unit.type_name,
                        formation
                    ],
                )
                .map_err(sql)?;
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
