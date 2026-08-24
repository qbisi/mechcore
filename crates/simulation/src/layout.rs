use std::{fs, path::Path};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use mechcore_mcfr::{IdentityAllocator, ObjectKind};

use crate::{Error, Result};

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
    pub(crate) type_name: String,
    pub(crate) world_x: i64,
    pub(crate) world_z: i64,
    pub(crate) rotation: i64,
}

#[derive(Debug, Clone)]
pub(crate) struct CompiledLayout {
    pub(crate) round: u32,
    pub(crate) placements: [Placement; 2],
}

pub(crate) fn load(path: &Path) -> Result<CompiledLayout> {
    let bytes = fs::read(path)
        .map_err(|error| Error::new(format!("failed to read {}: {error}", path.display())))?;
    compile(&bytes).map_err(|error| {
        Error::new(format!(
            "cannot simulate layout {}: {error}",
            path.display()
        ))
    })
}

fn compile(bytes: &[u8]) -> Result<CompiledLayout> {
    let layout: Layout = serde_yaml::from_slice(bytes)
        .map_err(|error| Error::new(format!("invalid layout YAML: {error}")))?;
    if !(1..=15).contains(&layout.round) {
        return Err(Error::new("layout round must be within 1..=15"));
    }
    let mut identities = IdentityAllocator::new();
    let blue = compile_side("blue", 0, &layout.sides.blue, &mut identities)?;
    let red = compile_side("red", 1, &layout.sides.red, &mut identities)?;
    Ok(CompiledLayout {
        round: layout.round,
        placements: [blue, red],
    })
}

fn compile_side(
    name: &str,
    team: u32,
    side: &Side,
    identities: &mut IdentityAllocator,
) -> Result<Placement> {
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
    let [formation] = side.formations.as_slice() else {
        return Err(Error::new(format!(
            "side {name} must contain exactly one formation for this simulation slice"
        )));
    };
    if formation.type_name.trim().is_empty() {
        return Err(Error::new(format!(
            "side {name} formation type must not be empty"
        )));
    }
    if formation.level.unwrap_or(1) != 1
        || formation.equipment.is_some()
        || formation.travelling.unwrap_or(false)
    {
        return Err(Error::new(format!(
            "side {name} requires a level-one, unequipped, non-travelling formation"
        )));
    }
    if formation.rotated.unwrap_or(false) {
        return Err(Error::new(format!(
            "side {name} rotated formations are outside the current baseline simulator slice"
        )));
    }
    let local_x = i64::from(formation.x);
    let local_z = i64::from(formation.y);
    let (world_x, world_z, rotation) = if team == 0 {
        (local_x, local_z, 0)
    } else {
        (-local_x, -local_z, 180_000)
    };
    let formation_id = identities.allocate_formation()?;
    let unit_id = identities.allocate_object(ObjectKind::Unit)?.id;
    Ok(Placement {
        team,
        unit_id,
        formation_id,
        type_name: formation.type_name.clone(),
        world_x,
        world_z,
        rotation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAYOUT: &str = r"
round: 1
sides:
  blue:
    formations: [{type: marksman, x: 0, y: -50}]
  red:
    formations: [{type: arclight, x: 0, y: -50}]
";

    #[test]
    fn compiles_side_local_positions_into_one_world() {
        let layout = compile(LAYOUT.as_bytes()).unwrap();
        assert_eq!(layout.placements[0].world_z, -50);
        assert_eq!(layout.placements[1].world_z, 50);
        assert_eq!(layout.placements[1].rotation, 180_000);
        assert_eq!(layout.placements[0].unit_id, 1);
        assert_eq!(layout.placements[1].unit_id, 2);
    }

    #[test]
    fn rejects_features_not_owned_by_this_slice() {
        let value = LAYOUT.replace(
            "formations: [{type: marksman, x: 0, y: -50}]",
            "techs: {units: [10202]}\n    formations: [{type: marksman, x: 0, y: -50}]",
        );
        assert!(compile(value.as_bytes()).is_err());
    }
}
