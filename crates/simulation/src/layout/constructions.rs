//! What a layout's `constructions` become when the fight is built.
//!
//! `config/constructions.yaml` is the build's own table, extracted by
//! `scripts/extract-constructions.py`, and `docs/rules/constructions.md` states
//! what each field means and which of them was measured against the game.
//!
//! **A construction is several objects.** One placement becomes `count`
//! buildings, each with its own life and its own box, which is why this
//! answers a list. Where those objects stand is a measurement rather than a
//! reading, so a construction whose geometry nobody has measured is refused by
//! name instead of being laid out by a formula fitted to the one that was.
//!
//! Nothing here fights. A turret carries a skill and its damage and no
//! mechanism in this build fires one, so a turret is refused for that and not
//! for its shape.

use std::collections::BTreeMap;

use mechcore_document::{NativeFormation, Placement};
use serde::Deserialize;

use crate::{Error, Result};

const DEFAULT_CONSTRUCTIONS: &str = include_str!("../../../../config/constructions.yaml");

/// `GameRiver.BuildingType.Special`, which every construction a recording
/// holds is. The map's own two towers are `EnergyTower` and `ResearchCenter`.
const SPECIAL: u32 = 3;

/// The spacing between the objects of one construction, in metres, for the
/// constructions whose geometry has been measured.
///
/// It is not derived from the table, and `docs/rules/constructions.md` says
/// why: `block_width` and `space` span 83 metres across a footprint 60 wide,
/// and the one row that would settle a general rule — the Magnetic Barrier,
/// whose `real_row_count` is 2 — cannot be placed by this build.
/// `tests/construction/shape.mcscript` is the measurement.
const MEASURED_SPACING: &[(i32, i64)] = &[(1, 12)];

/// One building a construction places.
///
/// Every length is in the space units the kernel holds a building's in, a
/// thousand to the metre, so that a construction and the map's own towers
/// reach `BuildingState` the same way.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ConstructionBuilding {
    pub(crate) team: u32,
    pub(crate) building_type_id: u32,
    pub(crate) x: i64,
    pub(crate) z: i64,
    /// Half the box a recording reports, which is the row's own `radius`.
    pub(crate) radius: i64,
    pub(crate) life: i32,
    /// Whether a unit looking for something to shoot may find this one.
    ///
    /// A Defensive Wall may not: its row answers `IsEnableSearchTarget` with
    /// false, and the game's own recording of a Crawler deployment opposite
    /// one shows every Crawler locked onto the unit behind the wall at tick
    /// one rather than onto the wall.
    pub(crate) searchable: bool,
    /// The row's `pathfinding_collider_priority`: the RVO layer the other
    /// side avoids it on.
    pub(crate) collider_priority: i32,
}

/// Space units to the metre, as `crates/simulation/src/rules.rs` quantizes a
/// description with.
const SPACE: i64 = 1_000;
const FIXED_ONE: i128 = 1 << 32;

/// An `FPoint` in space units. Every radius in the table is a whole number of
/// metres, so this is exact.
fn fixed_to_space(raw: i64) -> i64 {
    let scaled = i128::from(raw) * i128::from(SPACE) / FIXED_ONE;
    i64::try_from(scaled).unwrap_or(i64::MAX)
}

/// Every construction the build holds, by the id a layout compiles to.
#[derive(Debug, Clone)]
pub(crate) struct Constructions {
    rows: BTreeMap<i32, Row>,
}

/// One row of the table, whole.
///
/// Every field is named because the table is read with `deny_unknown_fields`:
/// the shape is the contract, so a re-extraction that adds, drops or renames
/// one is refused here rather than read past. Most of them are what a
/// construction *does*, which nothing in this build reads yet.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(
    dead_code,
    clippy::struct_excessive_bools,
    reason = "the table's whole shape is what is checked"
)]
struct Row {
    id: i32,
    name: String,
    #[serde(default)]
    layout_name: Option<String>,
    count: i32,
    max_life: i32,
    damage: i32,
    durability: i32,
    block_width: i32,
    space: i32,
    grid_column_count: i32,
    grid_row_count: i32,
    real_row_count: i32,
    rotate_speed: i32,
    skill_id: i32,
    exp: i32,
    sell_supply: i32,
    pathfinding_collider_priority: i32,
    default_actor_visibility: i32,
    radius: i64,
    path_radius: i64,
    attack_angle: i64,
    special_damage_reduce_rate: i64,
    has_durability: bool,
    can_be_bought: bool,
    can_be_sold: bool,
    can_be_effected_by_tower_buff: bool,
    enable_search_target: bool,
    quick_search_target: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    schema: String,
    game_build: String,
    constructions: Vec<Row>,
}

impl Constructions {
    /// Reads the tracked table.
    ///
    /// # Errors
    ///
    /// Returns an error when the table is not the one this build reads.
    pub(crate) fn load() -> Result<Self> {
        Self::parse(DEFAULT_CONSTRUCTIONS)
    }

    fn parse(text: &str) -> Result<Self> {
        let table: Table = serde_yaml::from_str(text)
            .map_err(|error| Error::new(format!("cannot read the construction table: {error}")))?;
        if table.schema != "mechcore.constructions" {
            return Err(Error::new(format!(
                "construction table declares schema {:?}",
                table.schema
            )));
        }
        if table.game_build.trim().is_empty() {
            return Err(Error::new("construction table declares no game build"));
        }
        let mut rows = BTreeMap::new();
        for row in table.constructions {
            let id = row.id;
            if rows.insert(id, row).is_some() {
                return Err(Error::new(format!(
                    "construction table holds construction {id} twice"
                )));
            }
        }
        Ok(Self { rows })
    }

    /// The buildings one placement puts on the board.
    ///
    /// # Errors
    ///
    /// Returns an error naming the construction when this build will not place
    /// it: because it attacks, or because where its objects stand has not been
    /// measured.
    pub(crate) fn buildings(
        &self,
        team: u32,
        placement: &Placement,
    ) -> Result<Vec<ConstructionBuilding>> {
        let NativeFormation::Construction(id) = placement.native else {
            return Err(Error::new(format!(
                "placement {:?} is not a construction",
                placement.type_name
            )));
        };
        let row = self.rows.get(&id).ok_or_else(|| {
            Error::new(format!(
                "construction {id} ({:?}) is not in the construction table",
                placement.type_name
            ))
        })?;
        let named = format!("construction {id} ({})", row.name);
        if row.damage != 0 {
            return Err(Error::new(format!(
                "{named} attacks for {}, and no mechanism here fires a construction's skill",
                row.damage
            )));
        }
        let offsets = row.offsets().ok_or_else(|| {
            Error::new(format!(
                "{named} places {} objects over {} rows, and where they stand is not measured",
                row.count, row.real_row_count
            ))
        })?;
        let (local_x, local_z) = (
            i64::from(placement.position.x),
            i64::from(placement.position.y),
        );
        let (centre_x, centre_z) = if team == 0 {
            (local_x, local_z)
        } else {
            (-local_x, -local_z)
        };
        Ok(offsets
            .into_iter()
            .map(|offset| ConstructionBuilding {
                team,
                building_type_id: SPECIAL,
                x: (centre_x + offset) * SPACE,
                z: centre_z * SPACE,
                radius: fixed_to_space(row.radius),
                life: row.max_life,
                searchable: row.enable_search_target,
                collider_priority: row.pathfinding_collider_priority,
            })
            .collect())
    }
}

impl Row {
    /// Where this construction's objects stand, relative to the placement, in
    /// whole metres along x.
    ///
    /// One object stands on the placement itself, whatever the row. More than
    /// one is answered only for a row whose spacing was measured and which
    /// places them in a single row: `real_row_count` above one is a second
    /// dimension nobody has read back, and an even count has no object on the
    /// placement to measure the others from.
    fn offsets(&self) -> Option<Vec<i64>> {
        if self.count == 1 {
            return Some(vec![0]);
        }
        let spacing = MEASURED_SPACING
            .iter()
            .find_map(|(id, spacing)| (*id == self.id).then_some(*spacing))?;
        if self.real_row_count != 1 || self.count % 2 == 0 || self.count < 1 {
            return None;
        }
        let middle = i64::from((self.count - 1) / 2);
        Some(
            (0..i64::from(self.count))
                .map(|index| (index - middle) * spacing)
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Constructions, MEASURED_SPACING};
    use mechcore_document::{NativeFormation, Placement, Position};

    fn placement(name: &str, id: i32, x: i32, y: i32) -> Placement {
        Placement {
            type_name: name.to_owned(),
            native: NativeFormation::Construction(id),
            footprint: None,
            position: Position { x, y },
            index: Some(0),
            level: None,
            exp: None,
            rotated: false,
            equipment: None,
            travelling: false,
        }
    }

    /// The five blocks `tests/construction/shape.mcscript` read back,
    /// which is the whole of what the geometry rests on.
    #[test]
    fn a_wall_is_five_blocks_twelve_metres_apart() {
        let table = Constructions::load().unwrap();
        let built = table
            .buildings(0, &placement("defensive_wall", 1, -140, -55))
            .unwrap();
        assert_eq!(
            built
                .iter()
                .map(|building| (building.x / 1_000, building.z / 1_000))
                .collect::<Vec<_>>(),
            [
                (-164, -55),
                (-152, -55),
                (-140, -55),
                (-128, -55),
                (-116, -55)
            ]
        );
        for building in &built {
            assert_eq!(building.life, 1112);
            assert!(
                !building.searchable,
                "a wall is not something a unit searches for"
            );
            assert_eq!(building.radius, 4_000, "4 metres, half of the 8 m box");
            assert_eq!(building.building_type_id, 3, "BuildingType.Special");
        }
    }

    /// Red states its board in its own frame, and the fight turns it around,
    /// which is the transform a unit placement gets.
    #[test]
    fn reds_wall_is_turned_around_with_the_rest_of_its_board() {
        let table = Constructions::load().unwrap();
        let built = table
            .buildings(1, &placement("defensive_wall", 1, 140, -55))
            .unwrap();
        assert_eq!(
            built
                .iter()
                .map(|building| (building.x / 1_000, building.z / 1_000))
                .collect::<Vec<_>>(),
            [(-164, 55), (-152, 55), (-140, 55), (-128, 55), (-116, 55)]
        );
    }

    /// A turret is one object and is still refused: it attacks, and nothing
    /// here fires a construction's skill. The refusal names it so a layout
    /// carrying one says which of its constructions it was.
    #[test]
    fn a_turret_is_refused_for_what_it_does_rather_than_for_its_shape() {
        let table = Constructions::load().unwrap();
        let refusal = table
            .buildings(0, &placement("anti_armor_turret", 2, -140, -100))
            .unwrap_err()
            .to_string();
        assert!(refusal.contains("attacks for 2748"), "{refusal}");
        assert!(refusal.contains("反装甲炮"), "{refusal}");
    }

    /// The one row that would settle how a multi-row construction is laid out
    /// is refused rather than guessed, which is the same thing
    /// `docs/rules/constructions.md` says is open.
    #[test]
    fn a_construction_with_two_rows_is_refused() {
        let table = Constructions::load().unwrap();
        let refusal = table
            .buildings(0, &placement("magnetic_barrier", 4, -145, -55))
            .unwrap_err()
            .to_string();
        assert!(refusal.contains("10 objects over 2 rows"), "{refusal}");
        assert_eq!(
            MEASURED_SPACING.len(),
            1,
            "one measurement, and adding a second means measuring it"
        );
    }
}
