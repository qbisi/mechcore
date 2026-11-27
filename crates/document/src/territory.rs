//! The board a side deploys on: its three regions and the map's bound, as the
//! layout every standard 1v1 map shares lays them out.
//!
//! `config/maps.yaml` holds each map data's `MapLayout`: one `PlayerTerritory`
//! per seat, a main region and two flanks. The territory here is blue's, in
//! world metres, which is blue's own frame; red's is the same turned half a
//! turn, which reading the layout checks. The map's bound is `Map.Bound`, the
//! least rectangle holding every territory's regions (`Map.RefreshBound`), and
//! what a battle skill's map rule measures against
//! (`PlayerController.GetRegionForCommanderSkill`). `docs/rules/map.md` states
//! both.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::Deserialize;

use crate::layout::{MAX_TOWER_STRENGTHEN_LEVEL, TOWER_COUNT};

/// A rectangle in metres: its least corner and its greatest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub min_x: i64,
    pub min_y: i64,
    pub max_x: i64,
    pub max_y: i64,
}

impl Rect {
    /// The same rectangle in a frame turned half a turn.
    const fn turned(self) -> Self {
        Self {
            min_x: -self.max_x,
            min_y: -self.max_y,
            max_x: -self.min_x,
            max_y: -self.min_y,
        }
    }

    const fn union(self, other: Self) -> Self {
        Self {
            min_x: if self.min_x < other.min_x {
                self.min_x
            } else {
                other.min_x
            },
            min_y: if self.min_y < other.min_y {
                self.min_y
            } else {
                other.min_y
            },
            max_x: if self.max_x > other.max_x {
                self.max_x
            } else {
                other.max_x
            },
            max_y: if self.max_y > other.max_y {
                self.max_y
            } else {
                other.max_y
            },
        }
    }
}

/// A side's regions in its own frame, and the map's bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Territory {
    /// The main deployment region.
    pub main: Rect,
    /// The flank whose `x` is negative.
    pub left_flank: Rect,
    /// The flank whose `x` is positive.
    pub right_flank: Rect,
    /// `Map.Bound`.
    pub bound: Rect,
}

struct MapDefaults {
    territory: Territory,
    /// `tower_order[seat][doc_index] = native_index`, where `doc_index` 0 is the
    /// Energy Tower (`building_type_id: 1`) and 1 is the Research Center (`2`).
    tower_order: [[usize; TOWER_COUNT]; 2],
}

static TERRITORY: LazyLock<MapDefaults> = LazyLock::new(|| {
    read(include_str!("../../../config/maps.yaml"))
        .unwrap_or_else(|error| panic!("config/maps.yaml lays out no territory: {error}"))
});

/// The territory of every standard 1v1 map.
#[must_use]
pub fn territory() -> &'static Territory {
    &TERRITORY.territory
}

/// Translates a document tower index (`0 = energy_tower`, `1 = research_center`)
/// into the seat's `BuildingManager.buildings` index.
///
/// # Errors
///
/// Returns an error when `doc_index` is outside `0..TOWER_COUNT`.
pub fn native_tower_index(red: bool, doc_index: i32) -> Result<i32, String> {
    let slot = usize::try_from(doc_index)
        .ok()
        .filter(|slot| *slot < TOWER_COUNT)
        .ok_or_else(|| format!("tower index {doc_index} is out of range 0..{TOWER_COUNT}"))?;
    let native = TERRITORY.tower_order[usize::from(red)][slot];
    i32::try_from(native).map_err(|error| error.to_string())
}

/// Translates a seat's `BuildingManager.buildings` index into the document
/// tower index (`0 = energy_tower`, `1 = research_center`).
///
/// # Errors
///
/// Returns an error when `native_index` is outside `0..TOWER_COUNT`.
pub fn doc_tower_index(red: bool, native_index: i32) -> Result<i32, String> {
    let native = usize::try_from(native_index)
        .ok()
        .filter(|index| *index < TOWER_COUNT)
        .ok_or_else(|| {
            format!("native tower index {native_index} is out of range 0..{TOWER_COUNT}")
        })?;
    let slot = TERRITORY.tower_order[usize::from(red)]
        .iter()
        .position(|&entry| entry == native)
        .ok_or_else(|| format!("native tower index {native_index} has no document slot"))?;
    i32::try_from(slot).map_err(|error| error.to_string())
}

/// Translates document `tower_strengthen_levels` (`[energy_tower, research_center]`)
/// into native `BuildingManager.buildings` order.
///
/// # Errors
///
/// Returns an error when non-empty `doc_levels` does not hold `TOWER_COUNT`
/// entries or any level lies outside `0..=MAX_TOWER_STRENGTHEN_LEVEL`.
pub fn native_tower_levels(red: bool, doc_levels: &[i32]) -> Result<Vec<i32>, String> {
    if doc_levels.is_empty() {
        return Ok(Vec::new());
    }
    if doc_levels.len() != TOWER_COUNT {
        return Err(format!(
            "tower_strengthen_levels holds {} levels, expected {TOWER_COUNT}",
            doc_levels.len()
        ));
    }
    let order = TERRITORY.tower_order[usize::from(red)];
    let mut native = vec![0; TOWER_COUNT];
    for (doc_index, &level) in doc_levels.iter().enumerate() {
        if !(0..=MAX_TOWER_STRENGTHEN_LEVEL).contains(&level) {
            return Err(format!(
                "tower_strengthen_levels[{doc_index}] must be 0..={MAX_TOWER_STRENGTHEN_LEVEL}"
            ));
        }
        native[order[doc_index]] = level;
    }
    Ok(native)
}

/// Translates native `BuildingManager.buildings` tower strengthening levels into
/// document order (`[energy_tower, research_center]`).
///
/// # Errors
///
/// Returns an error when non-empty `native_levels` does not hold `TOWER_COUNT`
/// entries or any level lies outside `0..=MAX_TOWER_STRENGTHEN_LEVEL`.
pub fn doc_tower_levels(red: bool, native_levels: &[i32]) -> Result<Vec<i32>, String> {
    if native_levels.is_empty() {
        return Ok(vec![0; TOWER_COUNT]);
    }
    if native_levels.len() != TOWER_COUNT {
        return Err(format!(
            "towerStrengthenLevels holds {} levels, expected {TOWER_COUNT}",
            native_levels.len()
        ));
    }
    for (native_index, &level) in native_levels.iter().enumerate() {
        if !(0..=MAX_TOWER_STRENGTHEN_LEVEL).contains(&level) {
            return Err(format!(
                "native tower {native_index} level {level} must be 0..={MAX_TOWER_STRENGTHEN_LEVEL}"
            ));
        }
    }
    let order = TERRITORY.tower_order[usize::from(red)];
    Ok(order.iter().map(|&native| native_levels[native]).collect())
}

#[derive(Deserialize)]
struct MapsFile {
    maps: BTreeMap<i32, String>,
    layouts: BTreeMap<String, String>,
    map_layouts: BTreeMap<String, Vec<PlayerTerritory>>,
    map_data: BTreeMap<String, Vec<MapDataObject>>,
}

#[derive(Deserialize)]
struct MapDataObject {
    kind: String,
    team_id: Option<i32>,
    building_type_id: Option<i32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerTerritory {
    main_region: i32,
    regions: Vec<MapRegion>,
}

/// A region's id and rectangle; its facing and opening round are the
/// layout's too, and nothing here reads them.
#[derive(Deserialize)]
struct MapRegion {
    id: i32,
    x_min: i64,
    y_min: i64,
    width: i64,
    height: i64,
}

impl MapRegion {
    const fn rect(&self) -> Rect {
        Rect {
            min_x: self.x_min,
            min_y: self.y_min,
            max_x: self.x_min + self.width,
            max_y: self.y_min + self.height,
        }
    }
}

fn read_tower_order(
    data_name: &str,
    objects: &[MapDataObject],
) -> Result<[[usize; TOWER_COUNT]; 2], String> {
    let mut sides: [[Option<usize>; TOWER_COUNT]; 2] = [[None; TOWER_COUNT]; 2];
    let mut seen = [0usize; 2];
    for object in objects {
        if object.kind != "tower" {
            continue;
        }
        let team = object
            .team_id
            .ok_or_else(|| format!("map data {data_name} tower misses team_id"))?;
        let seat = usize::try_from(team)
            .ok()
            .filter(|seat| *seat < 2)
            .ok_or_else(|| format!("map data {data_name} tower has invalid team_id {team}"))?;
        let doc_index = match object.building_type_id {
            Some(1) => 0,
            Some(2) => 1,
            other => {
                return Err(format!(
                    "map data {data_name} tower has unsupported building_type_id {other:?}"
                ));
            }
        };
        let native_index = seen[seat];
        seen[seat] += 1;
        if sides[seat][doc_index].replace(native_index).is_some() {
            return Err(format!(
                "map data {data_name} seat {seat} duplicates fixed tower slot {doc_index}"
            ));
        }
    }
    let mut order = [[0usize; TOWER_COUNT]; 2];
    for (seat, (slots, count)) in sides.into_iter().zip(seen).enumerate() {
        if count != TOWER_COUNT {
            return Err(format!(
                "map data {data_name} seat {seat} holds {count} towers, expected {TOWER_COUNT}"
            ));
        }
        for (doc_index, slot) in slots.into_iter().enumerate() {
            order[seat][doc_index] = slot.ok_or_else(|| {
                format!("map data {data_name} seat {seat} misses fixed tower slot {doc_index}")
            })?;
        }
    }
    Ok(order)
}

fn read(yaml: &str) -> Result<MapDefaults, String> {
    let maps: MapsFile = serde_yaml::from_str(yaml).map_err(|error| error.to_string())?;
    let mut data_names = maps.maps.values();
    let first_data = data_names.next().ok_or("no map")?;
    let first_objects = maps
        .map_data
        .get(first_data)
        .ok_or_else(|| format!("map data {first_data} is not in the config"))?;
    let tower_order = read_tower_order(first_data, first_objects)?;
    for data in data_names {
        let objects = maps
            .map_data
            .get(data)
            .ok_or_else(|| format!("map data {data} is not in the config"))?;
        if read_tower_order(data, objects)? != tower_order {
            return Err("the standard maps place fixed towers in different order".to_owned());
        }
    }
    let mut names = maps.maps.values().map(|data| {
        maps.layouts
            .get(data)
            .ok_or_else(|| format!("map data {data} names no layout"))
    });
    let name = names.next().ok_or("no map")??;
    if names.any(|other| other != Ok(name)) {
        return Err("the standard maps lay out different territories".to_owned());
    }
    let seats = maps
        .map_layouts
        .get(name)
        .ok_or_else(|| format!("layout {name} is not in the config"))?;
    let [blue, red] = seats.as_slice() else {
        return Err(format!(
            "layout {name} holds {} territories, not 2",
            seats.len()
        ));
    };
    let regions = |seat: &PlayerTerritory| -> Result<(Rect, Vec<Rect>), String> {
        let main = seat
            .regions
            .iter()
            .find(|region| region.id == seat.main_region)
            .ok_or_else(|| format!("layout {name} has no region {}", seat.main_region))?
            .rect();
        let mut flanks = seat
            .regions
            .iter()
            .filter(|region| region.id != seat.main_region)
            .map(MapRegion::rect)
            .collect::<Vec<_>>();
        flanks.sort_by_key(|rect| rect.min_x);
        Ok((main, flanks))
    };
    let (main, flanks) = regions(blue)?;
    let [left_flank, right_flank] = flanks[..] else {
        return Err(format!(
            "layout {name} gives a side {} flanks, not 2",
            flanks.len()
        ));
    };
    let (red_main, red_flanks) = regions(red)?;
    let mut turned = flanks.iter().map(|rect| rect.turned()).collect::<Vec<_>>();
    turned.sort_by_key(|rect| rect.min_x);
    if red_main != main.turned() || red_flanks != turned {
        return Err(format!(
            "layout {name} does not give red blue's territory turned half a turn"
        ));
    }
    let bound = [main, left_flank, right_flank, red_main]
        .into_iter()
        .chain(red_flanks)
        .reduce(Rect::union)
        .expect("a territory has regions");
    Ok(MapDefaults {
        territory: Territory {
            main,
            left_flank,
            right_flank,
            bound,
        },
        tower_order,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The standard maps' board: a 600 by 300 metre main region and two 60 by
    /// 300 metre flanks beyond the middle, inside a 720 by 620 metre map.
    #[test]
    fn the_standard_board() {
        let territory = territory();
        assert_eq!(
            territory.main,
            Rect {
                min_x: -300,
                min_y: -310,
                max_x: 300,
                max_y: -10
            }
        );
        assert_eq!(
            territory.left_flank,
            Rect {
                min_x: -360,
                min_y: 10,
                max_x: -300,
                max_y: 310
            }
        );
        assert_eq!(territory.right_flank.min_x, 300);
        assert_eq!(
            territory.bound,
            Rect {
                min_x: -360,
                min_y: -310,
                max_x: 360,
                max_y: 310
            }
        );
    }

    #[test]
    fn tower_mapping_keeps_energy_tower_first_in_documents() {
        assert_eq!(native_tower_index(false, 0).unwrap(), 0);
        assert_eq!(native_tower_index(false, 1).unwrap(), 1);
        assert_eq!(native_tower_index(true, 0).unwrap(), 1);
        assert_eq!(native_tower_index(true, 1).unwrap(), 0);
        assert_eq!(doc_tower_index(true, 0).unwrap(), 1);
        assert_eq!(doc_tower_index(true, 1).unwrap(), 0);

        assert_eq!(native_tower_levels(false, &[1, 3]).unwrap(), [1, 3]);
        assert_eq!(native_tower_levels(true, &[1, 3]).unwrap(), [3, 1]);
        assert_eq!(doc_tower_levels(true, &[3, 1]).unwrap(), [1, 3]);
        assert_eq!(doc_tower_levels(true, &[]).unwrap(), [0, 0]);
        assert!(native_tower_levels(false, &[]).unwrap().is_empty());
    }
}
