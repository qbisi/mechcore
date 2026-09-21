//! What a recording's standing objects are: the towers a map gives a side,
//! and the objects each of its constructions became.
//!
//! A construction is not one object. `FightConstructionSystem.Create` answers
//! an `IReadOnlyList<FightConstruction>`, and a recording holds each of them as
//! its own building row, so a Defensive Wall arrives as several rows and a
//! turret as one. MCFR records a building's `BuildingType` and not the
//! construction it came from, which is why this matches the rows against the
//! layout the recording embeds rather than reading a type out of them.
//!
//! This is neither of the other two readers' job. [`crate::outcome`] answers
//! what a fight decided and a building is not among `battle.md`'s five fields;
//! [`crate::stats`] answers a unit's numbers and a building is not a unit.
//!
//! Positions, bounds and every other length are the recording's own fixed
//! point, `1 << 32` to the metre, the same units [`crate::stats`] reports a
//! derived number in.

use std::path::Path;

use mechcore_document::StaticPlacement;
use mechcore_mcfr::{BuildingState, McfrReader};
use serde::Serialize;

use crate::{
    cli::Failure,
    scene::{self, FIRST_TICK},
    turn::Side,
};

pub(crate) const SCHEMA: &str = "mechcore.fight-buildings.v1";

/// Every standing object one tick of a recording holds.
#[derive(Serialize)]
pub(crate) struct Standing {
    schema: &'static str,
    recording: String,
    /// The tick this was read at, and the last one the recording holds.
    tick: u32,
    ticks: u32,
    sides: Sides,
}

#[derive(Serialize)]
struct Sides {
    blue: SideBuildings,
    red: SideBuildings,
}

/// One side's towers, and what each of its constructions became.
#[derive(Serialize)]
struct SideBuildings {
    /// The map's own buildings for this side, which no layout places.
    towers: Vec<Building>,
    /// Every construction the layout declares, in index order, each with the
    /// buildings still standing that belong to it. A construction whose
    /// buildings are all destroyed answers an empty list rather than
    /// disappearing, because that is a reading too.
    constructions: Vec<Construction>,
}

/// One construction of the layout, and the buildings it owns at this tick.
#[derive(Serialize)]
struct Construction {
    index: i32,
    name: String,
    /// How many of this construction's objects are standing at this tick,
    /// which is the whole of what the description says until something
    /// destroys one. It is `parts`' length, stated so an assertion can hold
    /// the number itself rather than one entry of a list.
    standing: usize,
    parts: Vec<Building>,
}

/// One standing object, as the recording holds it.
#[derive(Serialize)]
struct Building {
    /// The recording's own identity for it, normalized at the capture
    /// boundary and stable for the length of the recording.
    id: u64,
    /// What the build's `BuildingType` calls it.
    kind: &'static str,
    position: Position,
    /// The object's bounding box, which is twice the radius its description
    /// carries.
    bounds: Bounds,
    life: Life,
    available: bool,
    targetable: bool,
    collision_enabled: bool,
}

#[derive(Serialize)]
struct Position {
    x: i64,
    z: i64,
}

#[derive(Serialize)]
struct Bounds {
    width: i64,
    height: i64,
}

#[derive(Serialize)]
struct Life {
    current: i32,
    maximum: i32,
}

impl Building {
    fn of(state: &BuildingState) -> Result<Self, Failure> {
        Ok(Self {
            id: state.building_id,
            kind: kind(state.building_type_id)?,
            position: Position {
                x: state.position.x,
                z: state.position.z,
            },
            bounds: Bounds {
                width: state.bounds_width,
                height: state.bounds_height,
            },
            life: Life {
                current: state.life.current,
                maximum: state.life.maximum,
            },
            available: state.available,
            targetable: state.targetable,
            collision_enabled: state.collision_enabled,
        })
    }
}

/// The build's `GameRiver.BuildingType`, which is what MCFR records.
///
/// It says what kind of object this is and not which construction released it:
/// a wall block and a turret are both `Special`, and telling them apart is
/// what the layout is matched for.
fn kind(building_type: u32) -> Result<&'static str, Failure> {
    match building_type {
        0 => Ok("normal"),
        1 => Ok("energy_tower"),
        2 => Ok("research_center"),
        3 => Ok("special"),
        other => Err(Failure::refused(format!(
            "recording holds building type {other}, which this build does not name"
        ))),
    }
}

/// Whether a building is one of the map's own rather than a construction's.
const fn is_tower(building_type: u32) -> bool {
    matches!(building_type, 1 | 2)
}

/// Reads one tick of a recording for the objects standing in it.
///
/// # Errors
///
/// Returns a failure when the file is not a recording this build reads, when
/// it holds no such tick, or when a building belongs to no single construction
/// of the layout the recording embeds.
pub(crate) fn read(path: &Path, tick: Option<u32>) -> Result<Standing, Failure> {
    let reader = McfrReader::open(path)
        .map_err(|error| Failure::refused(format!("cannot read {}: {error}", path.display())))?;
    let layout = scene::layout(&reader)?;
    let tick = tick.unwrap_or(FIRST_TICK);
    let state = scene::snapshot(&reader, tick)?;

    let mut answered = Vec::new();
    for side in Side::BOTH {
        let placements = &scene::side_of(&layout, side).constructions;
        let mut towers = Vec::new();
        let mut parts: Vec<Vec<Building>> =
            placements.iter().map(|_| Vec::new()).collect::<Vec<_>>();
        for building in &state.buildings {
            if u64::from(building.team_id) != side.seat() as u64 {
                continue;
            }
            if is_tower(building.building_type_id) {
                towers.push(Building::of(building)?);
                continue;
            }
            let owner = owner(placements, side, building)?;
            parts[owner].push(Building::of(building)?);
        }
        answered.push(SideBuildings {
            towers,
            constructions: placements
                .iter()
                .zip(parts)
                .map(|(placement, parts)| Construction {
                    index: placement.index,
                    name: placement.type_name.clone(),
                    standing: parts.len(),
                    parts,
                })
                .collect(),
        });
    }
    let red = answered.pop().expect("both sides were read");
    let blue = answered.pop().expect("both sides were read");
    Ok(Standing {
        schema: SCHEMA,
        recording: path.display().to_string(),
        tick,
        ticks: reader.terminal_tick(),
        sides: Sides { blue, red },
    })
}

/// Which construction of the layout this building belongs to.
///
/// The two are matched the only way a recording allows: a construction's
/// objects stand around the position it was placed at, so the nearest
/// placement on the same side owns it. A tie is refused rather than broken,
/// and so is a side that placed nothing while its fight holds a building no
/// map gave it.
fn owner(
    placements: &[StaticPlacement],
    side: Side,
    building: &BuildingState,
) -> Result<usize, Failure> {
    let centre = (building.position.x >> 32, building.position.z >> 32);
    let mut nearest: Option<(i64, usize)> = None;
    let mut tied = false;
    for (offset, placement) in placements.iter().enumerate() {
        let distance = scene::distance(scene::world(placement.position, side), centre);
        match nearest {
            Some((best, _)) if distance > best => {}
            Some((best, _)) if distance == best => tied = true,
            _ => {
                nearest = Some((distance, offset));
                tied = false;
            }
        }
    }
    nearest
        .filter(|_| !tied)
        .map(|(_, offset)| offset)
        .ok_or_else(|| {
            Failure::refused(format!(
                "building {} on {} belongs to no single construction of the layout \
                 the recording embeds",
                building.building_id,
                side.name()
            ))
        })
}
