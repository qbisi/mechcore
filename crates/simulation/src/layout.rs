use std::{fs, path::Path};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    Error, Result,
    rules::{UnitConfig, UnitConfigs},
};

const DEPLOYMENT_MIN_X: i64 = -300;
const DEPLOYMENT_MAX_X: i64 = 300;
const DEPLOYMENT_MIN_Z: i64 = -310;
const DEPLOYMENT_MAX_Z: i64 = -10;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Layout {
    round: u32,
    sides: Sides,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Sides {
    blue: Side,
    red: Side,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Techs {
    officers: Vec<i32>,
    units: Vec<i32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[allow(clippy::struct_field_names)] // Names are fixed by the public layout schema.
struct ResearchCenter {
    strength_level: i32,
    attack_level: i32,
    defense_level: i32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct EnergyTower {
    strength_level: i32,
    range_enhancement: bool,
    movement_enhancement: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Side {
    #[serde(default)]
    techs: Techs,
    #[serde(default)]
    research_center: ResearchCenter,
    #[serde(default)]
    energy_tower: EnergyTower,
    formations: Vec<Formation>,
    #[serde(default)]
    battle_skills: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Formation {
    #[serde(rename = "type")]
    type_name: String,
    x: i32,
    y: i32,
    #[serde(default)]
    level: Option<i32>,
    #[serde(default)]
    rotated: Option<bool>,
    #[serde(default)]
    equipment: Option<i32>,
    #[serde(default)]
    travelling: Option<bool>,
}

#[derive(Debug, Clone)]
pub(crate) struct Placement {
    pub(crate) team: u32,
    pub(crate) unit_id: u64,
    pub(crate) formation_id: u64,
    // Native side-local UnitIndex for the unit-only, cleared baseline deployment.
    pub(crate) formation_index: i32,
    pub(crate) type_name: String,
    pub(crate) world_x: i64,
    pub(crate) world_z: i64,
    pub(crate) rotation: i64,
    pub(crate) rotated: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct CompiledLayout {
    pub(crate) round: u32,
    pub(crate) placements: Vec<Placement>,
}

pub(crate) fn load(path: &Path, units: &UnitConfigs) -> Result<CompiledLayout> {
    let bytes = fs::read(path)
        .map_err(|error| Error::new(format!("failed to read {}: {error}", path.display())))?;
    compile(&bytes, units).map_err(|error| {
        Error::new(format!(
            "cannot simulate layout {}: {error}",
            path.display()
        ))
    })
}

fn compile(bytes: &[u8], units: &UnitConfigs) -> Result<CompiledLayout> {
    let layout: Layout = serde_yaml::from_slice(bytes)
        .map_err(|error| Error::new(format!("invalid layout YAML: {error}")))?;
    if !(1..=15).contains(&layout.round) {
        return Err(Error::new("layout round must be within 1..=15"));
    }
    let mut placements = compile_side("blue", 0, &layout.sides.blue, units)?;
    placements.extend(compile_side("red", 1, &layout.sides.red, units)?);
    validate_collisions(&placements, units)?;

    Ok(CompiledLayout {
        round: layout.round,
        placements,
    })
}

fn compile_side(name: &str, team: u32, side: &Side, units: &UnitConfigs) -> Result<Vec<Placement>> {
    if !side.techs.officers.is_empty() || !side.techs.units.is_empty() {
        return Err(Error::new(format!(
            "side {name} technologies are outside the current baseline simulator slice"
        )));
    }
    if side.research_center.strength_level != 0
        || side.research_center.attack_level != 0
        || side.research_center.defense_level != 0
        || side.energy_tower.strength_level != 0
        || side.energy_tower.range_enhancement
        || side.energy_tower.movement_enhancement
    {
        return Err(Error::new(format!(
            "side {name} tower modifiers are outside the current baseline simulator slice"
        )));
    }
    if !side.battle_skills.is_empty() {
        return Err(Error::new(format!(
            "side {name} battle skills are outside the current baseline simulator slice"
        )));
    }
    if side.formations.is_empty() {
        return Err(Error::new(format!(
            "side {name} must contain at least one formation"
        )));
    }

    side.formations
        .iter()
        .enumerate()
        .map(|(index, formation)| compile_formation(name, team, index, formation, units))
        .collect()
}

fn compile_formation(
    side_name: &str,
    team: u32,
    index: usize,
    formation: &Formation,
    units: &UnitConfigs,
) -> Result<Placement> {
    if formation.type_name.trim().is_empty() {
        return Err(Error::new(format!(
            "side {side_name} formation type must not be empty"
        )));
    }
    if formation.level.unwrap_or(1) != 1
        || formation.equipment.is_some()
        || formation.travelling.unwrap_or(false)
    {
        return Err(Error::new(format!(
            "side {side_name} requires level-one, unequipped, non-travelling formations"
        )));
    }
    let rotated = formation.rotated.unwrap_or(false);
    let rules = units.get(&formation.type_name).ok_or_else(|| {
        Error::new(format!(
            "side {side_name} formation type {:?} has no unit configuration",
            formation.type_name
        ))
    })?;
    validate_deployment_position(side_name, formation, rules)?;
    let local_x = i64::from(formation.x);
    let local_z = i64::from(formation.y);
    let (world_x, world_z, rotation) = if team == 0 {
        (local_x, local_z, 0)
    } else {
        (-local_x, -local_z, 180_000)
    };
    Ok(Placement {
        team,
        unit_id: 0,
        formation_id: 0,
        formation_index: i32::try_from(index)
            .map_err(|_| Error::new("formation index exceeds the native integer range"))?,
        type_name: formation.type_name.clone(),
        world_x,
        world_z,
        rotation,
        rotated,
    })
}

fn validate_deployment_position(
    side_name: &str,
    formation: &Formation,
    rules: &UnitConfig,
) -> Result<()> {
    let (width, depth) = rules.formation_footprint_meters()?;
    let (width, depth) = if formation.rotated.unwrap_or(false) {
        (depth, width)
    } else {
        (width, depth)
    };
    let x = i64::from(formation.x);
    let z = i64::from(formation.y);
    let required_x = grid_center_remainder(width).ok_or_else(|| {
        Error::new(format!(
            "side {side_name} formation type {:?} has unsupported footprint width {width}",
            formation.type_name
        ))
    })?;
    let required_z = grid_center_remainder(depth).ok_or_else(|| {
        Error::new(format!(
            "side {side_name} formation type {:?} has unsupported footprint depth {depth}",
            formation.type_name
        ))
    })?;
    if x.rem_euclid(10) != required_x || z.rem_euclid(10) != required_z {
        return Err(Error::new(format!(
            "side {side_name} formation type {:?} at ({}, {}) is off the native 10x10 grid",
            formation.type_name, formation.x, formation.y
        )));
    }
    let min_x = x - width / 2;
    let max_x = x + width / 2;
    let min_z = z - depth / 2;
    let max_z = z + depth / 2;
    if min_x < DEPLOYMENT_MIN_X
        || max_x > DEPLOYMENT_MAX_X
        || min_z < DEPLOYMENT_MIN_Z
        || max_z > DEPLOYMENT_MAX_Z
    {
        return Err(Error::new(format!(
            "side {side_name} formation type {:?} at ({}, {}) exceeds the main deployment boundary",
            formation.type_name, formation.x, formation.y
        )));
    }
    Ok(())
}

fn validate_collisions(placements: &[Placement], units: &UnitConfigs) -> Result<()> {
    for (index, left) in placements.iter().enumerate() {
        let left_rules = units
            .get(&left.type_name)
            .expect("compiled placement owns unit rules");
        let (left_width, left_depth) = left_rules.formation_footprint_meters()?;
        let (left_width, left_depth) = if left.rotated {
            (left_depth, left_width)
        } else {
            (left_width, left_depth)
        };
        for right in &placements[index + 1..] {
            let right_rules = units
                .get(&right.type_name)
                .expect("compiled placement owns unit rules");
            let (right_width, right_depth) = right_rules.formation_footprint_meters()?;
            let (right_width, right_depth) = if right.rotated {
                (right_depth, right_width)
            } else {
                (right_width, right_depth)
            };
            let overlaps_x =
                (left.world_x - right.world_x).abs() * 2 < left_width.saturating_add(right_width);
            let overlaps_z =
                (left.world_z - right.world_z).abs() * 2 < left_depth.saturating_add(right_depth);
            if overlaps_x && overlaps_z {
                return Err(Error::new(format!(
                    "formations collide: team {} type {:?} and team {} type {:?}",
                    left.team, left.type_name, right.team, right.type_name
                )));
            }
        }
    }
    Ok(())
}

fn grid_center_remainder(extent: i64) -> Option<i64> {
    match extent.rem_euclid(20) {
        0 => Some(0),
        10 => Some(5),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::SimulationConfig;

    const LAYOUT: &str = r"
round: 1
sides:
  blue:
    formations: [{type: marksman, x: 0, y: -50}]
  red:
    formations: [{type: arclight, x: 0, y: -50}]
";

    fn compile_default(value: &str) -> Result<CompiledLayout> {
        let config = SimulationConfig::load(None)?;
        compile(value.as_bytes(), &config.units)
    }

    #[test]
    fn compiles_side_local_positions_into_one_world() {
        let layout = compile_default(LAYOUT).unwrap();
        assert_eq!(layout.placements[0].world_z, -50);
        assert_eq!(layout.placements[1].world_z, 50);
        assert_eq!(layout.placements[1].rotation, 180_000);
        assert_eq!(layout.placements[0].unit_id, 0);
        assert_eq!(layout.placements[1].unit_id, 0);
        assert_eq!(layout.placements[0].formation_index, 0);
    }

    #[test]
    fn formation_index_preserves_native_declaration_order_before_seeded_generation() {
        let value = LAYOUT.replace(
            "formations: [{type: marksman, x: 0, y: -50}]",
            "formations:\n      - {type: arclight, x: 20, y: -100}\n      - {type: rhino, x: -15, y: -105}",
        );
        let layout = compile_default(&value).unwrap();
        assert_eq!(layout.placements.len(), 3);
        assert_eq!(layout.placements[0].type_name, "arclight");
        assert_eq!(layout.placements[0].formation_index, 0);
        assert_eq!(layout.placements[1].type_name, "rhino");
        assert_eq!(layout.placements[1].formation_index, 1);
        assert_eq!(layout.placements[2].formation_index, 0);
        assert!(
            layout
                .placements
                .iter()
                .all(|placement| placement.unit_id == 0 && placement.formation_id == 0)
        );
    }

    #[test]
    fn rejects_features_not_owned_by_this_slice() {
        let value = LAYOUT.replace(
            "formations: [{type: marksman, x: 0, y: -50}]",
            "techs: {units: [10202]}\n    formations: [{type: marksman, x: 0, y: -50}]",
        );
        assert!(compile_default(&value).is_err());
    }

    #[test]
    fn compiles_multi_member_formations_as_one_deployment_placement() {
        let value = LAYOUT.replace(
            "{type: marksman, x: 0, y: -50}",
            "{type: crawler, x: 5, y: -50}",
        );
        let layout = compile_default(&value).unwrap();
        assert_eq!(layout.placements.len(), 2);
        assert_eq!(layout.placements[0].type_name, "crawler");
    }

    #[test]
    fn rotated_formation_swaps_config_footprint_without_rotating_unit_facing() {
        let value = LAYOUT.replace(
            "{type: arclight, x: 0, y: -50}",
            "{type: crawler, rotated: true, x: 0, y: -105}",
        );
        let layout = compile_default(&value).unwrap();
        let red = &layout.placements[1];

        assert!(red.rotated);
        assert_eq!((red.world_x, red.world_z), (0, 105));
        assert_eq!(red.rotation, 180_000);
    }
}
