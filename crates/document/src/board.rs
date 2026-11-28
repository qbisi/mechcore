//! What stands on the board before anyone deploys.
//!
//! Each side's Energy Tower and Research Center are elements of its main
//! deployment region, and `MapRegion.IsAvailible` refuses a new element that
//! overlaps one exactly as it refuses one that overlaps a unit: a placement on
//! a tower is `PlayerActionCheckResult.RegionLimit` (16). Their deployment
//! footprint is a 20 m square on the tower's centre. The build keeps it in the
//! map asset, which this repository does not read, so it was measured against
//! the game: a 20 x 20 Marksman ten metres off a tower's centre, on either
//! axis, is refused, and one twenty metres off is placed; a Crawler whose edge
//! meets the square is placed.

use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};
use serde_yaml::{Mapping as YamlMap, Value as YamlValue};

use crate::catalog::{resolve_construction_type, resolve_contraption_type};
use crate::compile::unit_footprint;
use crate::layout::{MAX_TOWER_STRENGTHEN_LEVEL, Position, StaticPlacement, TOWER_COUNT};
use crate::r#match::SideState;

/// The side of a tower's deployment footprint, in metres.
pub(crate) const TOWER_FOOTPRINT: i64 = 20;

/// The four towers, as world centres with their names: blue's at `y = -170`,
/// red's at `y = 170`, each side's Energy Tower on its own left.
pub(crate) const TOWERS: [((i64, i64), &str); 4] = [
    ((-140, -170), "blue Energy Tower"),
    ((140, -170), "blue Research Center"),
    ((140, 170), "red Energy Tower"),
    ((-140, 170), "red Research Center"),
];

/// A side's own two towers, as local centres: the same for either side,
/// because red's frame is the world turned half a turn.
pub(crate) const OWN_TOWERS_LOCAL: [(i64, i64); 2] = [(-140, -170), (140, -170)];

const TOWER_NAMES: [&str; 2] = ["energy_tower", "research_center"];

/// Occupied axis-aligned width (`width`, delta x) and depth (`depth`, delta y)
/// in the owning side's local coordinate system, in metres.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Size {
    pub width: i64,
    pub depth: i64,
}

/// Expanded infrastructure entity representing one fixed side tower with its
/// local centre, spatial footprint, and strengthen level.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TowerEntity {
    pub name: String,
    pub index: i32,
    pub position: Position,
    pub size: Size,
    pub level: i32,
}

pub(crate) fn towers_from_levels(levels: &[i32]) -> Result<Vec<TowerEntity>, String> {
    let resolved = if levels.is_empty() {
        [0, 0]
    } else if levels.len() == TOWER_COUNT {
        [levels[0], levels[1]]
    } else {
        return Err(format!(
            "tower_strengthen_levels must hold {TOWER_COUNT} levels, not {}",
            levels.len()
        ));
    };
    let mut out = Vec::with_capacity(TOWER_COUNT);
    for (idx, level) in resolved.into_iter().enumerate() {
        if !(0..=MAX_TOWER_STRENGTHEN_LEVEL).contains(&level) {
            return Err(format!(
                "tower_strengthen_levels[{idx}] must be 0..={MAX_TOWER_STRENGTHEN_LEVEL}"
            ));
        }
        let (x, y) = OWN_TOWERS_LOCAL[idx];
        out.push(TowerEntity {
            name: TOWER_NAMES[idx].to_owned(),
            index: i32::try_from(idx).map_err(|_| "tower index overflow")?,
            position: Position {
                x: i32::try_from(x).map_err(|_| "tower x overflow")?,
                y: i32::try_from(y).map_err(|_| "tower y overflow")?,
            },
            size: Size {
                width: TOWER_FOOTPRINT,
                depth: TOWER_FOOTPRINT,
            },
            level,
        });
    }
    Ok(out)
}

pub(crate) fn unit_size(
    type_name: &str,
    position: Position,
    rotated: bool,
) -> Result<Size, String> {
    let (width, depth) = unit_footprint(type_name, position, rotated)
        .ok_or_else(|| format!("unit {type_name:?} has no footprint"))?;
    Ok(Size { width, depth })
}

pub(crate) fn construction_size(type_name: &str) -> Result<Size, String> {
    let (width, depth) = resolve_construction_type(type_name)
        .and_then(|spec| spec.footprint)
        .ok_or_else(|| format!("construction {type_name:?} has no footprint"))?;
    Ok(Size { width, depth })
}

pub(crate) fn contraption_size(type_name: &str) -> Result<Option<Size>, String> {
    let spec = resolve_contraption_type(type_name)
        .ok_or_else(|| format!("unknown contraption {type_name:?}"))?;
    Ok(spec.footprint.map(|(width, depth)| Size { width, depth }))
}

#[derive(Serialize)]
struct ExpandedStatic<'a> {
    name: &'a str,
    index: i32,
    position: Position,
    size: Size,
}

pub(crate) fn expanded_constructions_json(
    items: &[StaticPlacement],
) -> Result<Vec<JsonValue>, String> {
    items
        .iter()
        .map(|item| {
            let size = construction_size(&item.type_name)?;
            serde_json::to_value(ExpandedStatic {
                name: &item.type_name,
                index: item.index,
                position: item.position,
                size,
            })
            .map_err(|e| e.to_string())
        })
        .collect()
}

pub(crate) fn serialize_expanded_constructions<S: serde::Serializer>(
    items: &[StaticPlacement],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let expanded: Vec<ExpandedStatic<'_>> = items
        .iter()
        .map(|item| {
            let size = construction_size(&item.type_name).map_err(serde::ser::Error::custom)?;
            Ok(ExpandedStatic {
                name: &item.type_name,
                index: item.index,
                position: item.position,
                size,
            })
        })
        .collect::<Result<_, _>>()?;
    expanded.serialize(serializer)
}

pub(crate) fn expand_round_side_json(
    ordered: &mut JsonMap<String, JsonValue>,
    side: &SideState,
) -> Result<(), String> {
    ordered.remove("tower_strengthen_levels");
    let towers = towers_from_levels(&side.tower_strengthen_levels)?;
    ordered.insert(
        "towers".into(),
        serde_json::to_value(&towers).map_err(|e| e.to_string())?,
    );
    if let Some(JsonValue::Array(units)) = ordered.get_mut("units") {
        for (item, unit) in units.iter_mut().zip(&side.units) {
            if unit.recovered {
                continue;
            }
            let pos = unit
                .position
                .ok_or_else(|| format!("standing unit {} misses position", unit.index))?;
            let dims = unit_size(&unit.type_name, pos, unit.rotated.unwrap_or(false))?;
            if let Some(obj) = item.as_object_mut() {
                obj.insert(
                    "size".into(),
                    serde_json::to_value(dims).map_err(|e| e.to_string())?,
                );
            }
        }
    }
    if let Some(JsonValue::Array(constructions)) = ordered.get_mut("constructions") {
        for (item, c) in constructions.iter_mut().zip(&side.constructions) {
            let dims = construction_size(&c.type_name)?;
            if let Some(obj) = item.as_object_mut() {
                obj.insert(
                    "size".into(),
                    serde_json::to_value(dims).map_err(|e| e.to_string())?,
                );
            }
        }
    }
    if let Some(JsonValue::Array(contraptions)) = ordered.get_mut("contraptions") {
        for (item, c) in contraptions.iter_mut().zip(&side.contraptions) {
            if let Some(dims) = contraption_size(&c.type_name)?
                && let Some(obj) = item.as_object_mut()
            {
                obj.insert(
                    "size".into(),
                    serde_json::to_value(dims).map_err(|e| e.to_string())?,
                );
            }
        }
    }
    Ok(())
}

pub(crate) fn serialize_expanded_round_side<S: serde::Serializer>(
    side: &SideState,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let val = expand_round_side_yaml(side).map_err(serde::ser::Error::custom)?;
    val.serialize(serializer)
}

fn expand_round_side_yaml(side: &SideState) -> Result<YamlValue, String> {
    let YamlValue::Mapping(fields) = serde_yaml::to_value(side).map_err(|e| e.to_string())? else {
        return Err("serialized SideState must be a mapping".into());
    };
    let towers_val = serde_yaml::to_value(towers_from_levels(&side.tower_strengthen_levels)?)
        .map_err(|e| e.to_string())?;
    let key_levels = YamlValue::String("tower_strengthen_levels".into());
    let key_towers = YamlValue::String("towers".into());
    let mut rebuilt = YamlMap::with_capacity(fields.len());
    let mut inserted_towers = false;
    for (k, mut v) in fields {
        if k == key_levels {
            rebuilt.insert(key_towers.clone(), towers_val.clone());
            inserted_towers = true;
            continue;
        }
        match k.as_str() {
            Some("units") => attach_unit_sizes_yaml(&mut v, &side.units)?,
            Some("constructions") => attach_construction_sizes_yaml(&mut v, &side.constructions)?,
            Some("contraptions") => attach_contraption_sizes_yaml(&mut v, &side.contraptions)?,
            _ => {}
        }
        rebuilt.insert(k, v);
    }
    if !inserted_towers {
        rebuilt.insert(key_towers, towers_val);
    }
    Ok(YamlValue::Mapping(rebuilt))
}

fn attach_unit_sizes_yaml(
    val: &mut YamlValue,
    units: &[crate::r#match::StateUnit],
) -> Result<(), String> {
    let Some(seq) = val.as_sequence_mut() else {
        return Ok(());
    };
    for (item, unit) in seq.iter_mut().zip(units) {
        if unit.recovered {
            continue;
        }
        let pos = unit
            .position
            .ok_or_else(|| format!("standing unit {} misses position", unit.index))?;
        let size = unit_size(&unit.type_name, pos, unit.rotated.unwrap_or(false))?;
        if let Some(map) = item.as_mapping_mut() {
            map.insert(
                YamlValue::String("size".into()),
                serde_yaml::to_value(size).map_err(|e| e.to_string())?,
            );
        }
    }
    Ok(())
}

fn attach_construction_sizes_yaml(
    val: &mut YamlValue,
    constructions: &[StaticPlacement],
) -> Result<(), String> {
    let Some(seq) = val.as_sequence_mut() else {
        return Ok(());
    };
    for (item, c) in seq.iter_mut().zip(constructions) {
        let size = construction_size(&c.type_name)?;
        if let Some(map) = item.as_mapping_mut() {
            map.insert(
                YamlValue::String("size".into()),
                serde_yaml::to_value(size).map_err(|e| e.to_string())?,
            );
        }
    }
    Ok(())
}

fn attach_contraption_sizes_yaml(
    val: &mut YamlValue,
    contraptions: &[crate::layout::ContraptionPlacement],
) -> Result<(), String> {
    let Some(seq) = val.as_sequence_mut() else {
        return Ok(());
    };
    for (item, c) in seq.iter_mut().zip(contraptions) {
        if let Some(size) = contraption_size(&c.type_name)?
            && let Some(map) = item.as_mapping_mut()
        {
            map.insert(
                YamlValue::String("size".into()),
                serde_yaml::to_value(size).map_err(|e| e.to_string())?,
            );
        }
    }
    Ok(())
}

/// Validates and strips expanded-format `size` annotations from a round-0 opening side mapping.
pub(crate) fn normalize_expanded_opening_side(
    side_map: &mut YamlMap,
    side_name: &str,
) -> Result<(), String> {
    strip_construction_sizes(side_map, side_name)
}

/// Validates and translates expanded-format `towers` and `size` fields on a round >= 1
/// side mapping back into internal compact fields (`tower_strengthen_levels`, unsized items).
pub(crate) fn normalize_expanded_round_side(
    side_map: &mut YamlMap,
    side_name: &str,
) -> Result<(), String> {
    let key_towers = YamlValue::String("towers".into());
    let key_levels = YamlValue::String("tower_strengthen_levels".into());
    if side_map.contains_key(&key_towers) && side_map.contains_key(&key_levels) {
        return Err(format!(
            "{side_name} cannot state both towers and tower_strengthen_levels"
        ));
    }
    if let Some(towers_val) = side_map.remove(&key_towers) {
        let towers: Vec<TowerEntity> =
            serde_yaml::from_value(towers_val).map_err(|e| format!("{side_name} towers: {e}"))?;
        let levels = parse_and_validate_towers(&towers, side_name)?;
        side_map.insert(
            key_levels,
            serde_yaml::to_value(levels).map_err(|e| e.to_string())?,
        );
    }
    strip_unit_sizes(side_map, side_name)?;
    strip_construction_sizes(side_map, side_name)?;
    strip_contraption_sizes(side_map, side_name)?;
    Ok(())
}

fn parse_and_validate_towers(towers: &[TowerEntity], side_name: &str) -> Result<Vec<i32>, String> {
    if towers.len() != TOWER_COUNT {
        return Err(format!(
            "{side_name} towers must hold {TOWER_COUNT} entries, not {}",
            towers.len()
        ));
    }
    let mut levels = Vec::with_capacity(TOWER_COUNT);
    for (idx, tower) in towers.iter().enumerate() {
        let expected_idx = i32::try_from(idx).map_err(|_| "tower index overflow")?;
        let (exp_x, exp_y) = OWN_TOWERS_LOCAL[idx];
        let expected_pos = Position {
            x: i32::try_from(exp_x).map_err(|_| "tower x overflow")?,
            y: i32::try_from(exp_y).map_err(|_| "tower y overflow")?,
        };
        let expected_size = Size {
            width: TOWER_FOOTPRINT,
            depth: TOWER_FOOTPRINT,
        };
        if tower.name != TOWER_NAMES[idx] || tower.index != expected_idx {
            return Err(format!(
                "{side_name} towers[{idx}] must be {:?} with index {expected_idx}",
                TOWER_NAMES[idx]
            ));
        }
        if tower.position != expected_pos {
            return Err(format!(
                "{side_name} towers[{idx}] position must be ({}, {}), not ({}, {})",
                expected_pos.x, expected_pos.y, tower.position.x, tower.position.y
            ));
        }
        if tower.size != expected_size {
            return Err(format!(
                "{side_name} towers[{idx}] size must be {}x{}, not {}x{}",
                expected_size.width, expected_size.depth, tower.size.width, tower.size.depth
            ));
        }
        if !(0..=MAX_TOWER_STRENGTHEN_LEVEL).contains(&tower.level) {
            return Err(format!(
                "{side_name} towers[{idx}] level must be 0..={MAX_TOWER_STRENGTHEN_LEVEL}"
            ));
        }
        levels.push(tower.level);
    }
    Ok(levels)
}

fn strip_unit_sizes(side_map: &mut YamlMap, side_name: &str) -> Result<(), String> {
    let Some(seq) = side_map
        .get_mut(YamlValue::String("units".into()))
        .and_then(YamlValue::as_sequence_mut)
    else {
        return Ok(());
    };
    let size_key = YamlValue::String("size".into());
    for (idx, item) in seq.iter_mut().enumerate() {
        let Some(map) = item.as_mapping_mut() else {
            continue;
        };
        let Some(size_val) = map.remove(&size_key) else {
            continue;
        };
        if map
            .get(YamlValue::String("recovered".into()))
            .and_then(YamlValue::as_bool)
            .unwrap_or(false)
        {
            return Err(format!(
                "{side_name} units[{idx}] recovered tombstone must not carry size"
            ));
        }
        let actual: Size = serde_yaml::from_value(size_val)
            .map_err(|e| format!("{side_name} units[{idx}] size: {e}"))?;
        let name = map
            .get(YamlValue::String("name".into()))
            .and_then(YamlValue::as_str)
            .ok_or_else(|| format!("{side_name} units[{idx}] misses name"))?;
        let pos_val = map
            .get(YamlValue::String("position".into()))
            .ok_or_else(|| format!("{side_name} units[{idx}] misses position"))?;
        let pos: Position = serde_yaml::from_value(pos_val.clone())
            .map_err(|e| format!("{side_name} units[{idx}] position: {e}"))?;
        let rotated = map
            .get(YamlValue::String("rotated".into()))
            .and_then(YamlValue::as_bool)
            .unwrap_or(false);
        let expected = unit_size(name, pos, rotated)?;
        if actual != expected {
            return Err(format!(
                "{side_name} units[{idx}] ({name}) size must be {}x{}, not {}x{}",
                expected.width, expected.depth, actual.width, actual.depth
            ));
        }
    }
    Ok(())
}

fn strip_construction_sizes(side_map: &mut YamlMap, side_name: &str) -> Result<(), String> {
    let Some(seq) = side_map
        .get_mut(YamlValue::String("constructions".into()))
        .and_then(YamlValue::as_sequence_mut)
    else {
        return Ok(());
    };
    let size_key = YamlValue::String("size".into());
    for (idx, item) in seq.iter_mut().enumerate() {
        let Some(map) = item.as_mapping_mut() else {
            continue;
        };
        let Some(size_val) = map.remove(&size_key) else {
            continue;
        };
        let actual: Size = serde_yaml::from_value(size_val)
            .map_err(|e| format!("{side_name} constructions[{idx}] size: {e}"))?;
        let name = map
            .get(YamlValue::String("name".into()))
            .and_then(YamlValue::as_str)
            .ok_or_else(|| format!("{side_name} constructions[{idx}] misses name"))?;
        let expected = construction_size(name)?;
        if actual != expected {
            return Err(format!(
                "{side_name} constructions[{idx}] ({name}) size must be {}x{}, not {}x{}",
                expected.width, expected.depth, actual.width, actual.depth
            ));
        }
    }
    Ok(())
}

fn strip_contraption_sizes(side_map: &mut YamlMap, side_name: &str) -> Result<(), String> {
    let Some(seq) = side_map
        .get_mut(YamlValue::String("contraptions".into()))
        .and_then(YamlValue::as_sequence_mut)
    else {
        return Ok(());
    };
    let size_key = YamlValue::String("size".into());
    for (idx, item) in seq.iter_mut().enumerate() {
        let Some(map) = item.as_mapping_mut() else {
            continue;
        };
        let Some(size_val) = map.remove(&size_key) else {
            continue;
        };
        let actual: Size = serde_yaml::from_value(size_val)
            .map_err(|e| format!("{side_name} contraptions[{idx}] size: {e}"))?;
        let name = map
            .get(YamlValue::String("name".into()))
            .and_then(YamlValue::as_str)
            .ok_or_else(|| format!("{side_name} contraptions[{idx}] misses name"))?;
        let Some(expected) = contraption_size(name)? else {
            return Err(format!(
                "{side_name} contraptions[{idx}] ({name}) has no collision size"
            ));
        };
        if actual != expected {
            return Err(format!(
                "{side_name} contraptions[{idx}] ({name}) size must be {}x{}, not {}x{}",
                expected.width, expected.depth, actual.width, actual.depth
            ));
        }
    }
    Ok(())
}
