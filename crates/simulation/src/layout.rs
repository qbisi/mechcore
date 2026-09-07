use std::{fs, path::Path};

use mechcore_layout::{NativeFormation, SidePlan};

use crate::{
    Error, Result,
    rules::{UnitConfig, UnitConfigs},
};

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

pub(crate) fn load(
    path: &Path,
    units: &UnitConfigs,
) -> Result<(Option<i32>, CompiledLayout, String)> {
    let bytes = fs::read(path)
        .map_err(|error| Error::new(format!("failed to read {}: {error}", path.display())))?;
    let (seed, layout) = compile_with_seed(&bytes, units).map_err(|error| {
        Error::new(format!(
            "cannot simulate layout {}: {error}",
            path.display()
        ))
    })?;
    let parsed = mechcore_layout::parse_yaml(&bytes).map_err(Error::new)?;
    let canonical = mechcore_layout::canonical_yaml(parsed).map_err(Error::new)?;
    Ok((seed, layout, canonical))
}

#[cfg(test)]
fn compile(bytes: &[u8], units: &UnitConfigs) -> Result<CompiledLayout> {
    compile_with_seed(bytes, units).map(|(_, layout)| layout)
}

pub(crate) fn compile_with_seed(
    bytes: &[u8],
    units: &UnitConfigs,
) -> Result<(Option<i32>, CompiledLayout)> {
    let layout = mechcore_layout::parse_yaml(bytes).map_err(Error::new)?;
    let plan = mechcore_layout::compile_layout(layout).map_err(Error::new)?;
    let mut placements = compile_side("blue", 0, &plan.blue, units)?;
    placements.extend(compile_side("red", 1, &plan.red, units)?);

    Ok((
        plan.seed,
        CompiledLayout {
            round: u32::try_from(plan.round).expect("validated layout round is positive"),
            placements,
        },
    ))
}

fn compile_side(
    name: &str,
    team: u32,
    side: &SidePlan,
    units: &UnitConfigs,
) -> Result<Vec<Placement>> {
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
    if !side.terrains.is_empty() {
        return Err(Error::new(format!(
            "side {name} terrains are outside the current simulator closure; persistent terrain simulation requires GRBR-derived MCFR comparison"
        )));
    }
    if !side.battle_skills.is_empty() {
        return Err(Error::new(format!(
            "side {name} battle skills are outside the current baseline simulator slice"
        )));
    }
    if !side.constructions.is_empty() {
        return Err(Error::new(format!(
            "side {name} constructions are outside the current baseline simulator slice"
        )));
    }
    if !side.contraptions.is_empty() {
        return Err(Error::new(format!(
            "side {name} contraptions are outside the current baseline simulator slice"
        )));
    }
    if !side.airdrop_shields.is_empty() {
        return Err(Error::new(format!(
            "side {name} airdrop shields are outside the current baseline simulator slice"
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
    formation: &mechcore_layout::Placement,
    units: &UnitConfigs,
) -> Result<Placement> {
    if !matches!(formation.native, NativeFormation::Unit(_))
        || formation.level != Some(1)
        || formation.equipment.is_some()
        || formation.travelling
    {
        return Err(Error::new(format!(
            "side {side_name} requires level-one, unequipped, non-travelling formations"
        )));
    }
    let rotated = formation.rotated;
    let rules = units.get(&formation.type_name).ok_or_else(|| {
        Error::new(format!(
            "side {side_name} formation type {:?} has no unit configuration",
            formation.type_name
        ))
    })?;
    validate_formation_footprint(side_name, formation, rules)?;
    let local_x = i64::from(formation.position.x);
    let local_z = i64::from(formation.position.y);
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

fn validate_formation_footprint(
    side_name: &str,
    formation: &mechcore_layout::Placement,
    rules: &UnitConfig,
) -> Result<()> {
    let configured = rules.formation_footprint_meters()?;
    if formation.footprint == Some(configured) {
        Ok(())
    } else {
        Err(Error::new(format!(
            "side {side_name} formation type {:?} layout footprint {:?} does not match simulator configuration {:?}",
            formation.type_name, formation.footprint, configured
        )))
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
    formations: [{type: marksman, index: 0, position: {x: 0, y: -50}}]
  red:
    formations: [{type: arclight, index: 0, position: {x: 0, y: -50}}]
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
            "formations: [{type: marksman, index: 0, position: {x: 0, y: -50}}]",
            "formations:\n      - {type: arclight, index: 0, position: {x: 20, y: -100}}\n      - {type: rhino, index: 1, position: {x: -15, y: -105}}",
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
            "formations: [{type: marksman, index: 0, position: {x: 0, y: -50}}]",
            "techs: {units: [10202]}\n    formations: [{type: marksman, index: 0, position: {x: 0, y: -50}}]",
        );
        assert!(compile_default(&value).is_err());
    }

    #[test]
    fn rejects_constructions_outside_the_baseline_slice() {
        let value = LAYOUT.replace(
            "formations: [{type: marksman, index: 0, position: {x: 0, y: -50}}]",
            "formations: [{type: marksman, index: 0, position: {x: 0, y: -50}}]\n    constructions: [{type: defensive_wall, index: 0, position: {x: 140, y: -105}}]",
        );
        assert_eq!(
            compile_default(&value).unwrap_err().to_string(),
            "side blue constructions are outside the current baseline simulator slice"
        );
    }

    #[test]
    fn rejects_persistent_terrains_outside_simulator_closure() {
        let value = LAYOUT.replace(
            "formations: [{type: marksman, index: 0, position: {x: 0, y: -50}}]",
            "formations: [{type: marksman, index: 0, position: {x: 0, y: -50}}]\n    terrains: [{type: oil, control_points: [{x: -60, y: 40}, {x: 60, y: 40}]}]",
        );
        assert_eq!(
            compile_default(&value).unwrap_err().to_string(),
            "side blue terrains are outside the current simulator closure; persistent terrain simulation requires GRBR-derived MCFR comparison"
        );
    }

    #[test]
    fn compiles_multi_member_formations_as_one_deployment_placement() {
        let value = LAYOUT.replace(
            "{type: marksman, index: 0, position: {x: 0, y: -50}}",
            "{type: crawler, index: 0, position: {x: 5, y: -50}}",
        );
        let layout = compile_default(&value).unwrap();
        assert_eq!(layout.placements.len(), 2);
        assert_eq!(layout.placements[0].type_name, "crawler");
    }

    #[test]
    fn rotated_formation_swaps_config_footprint_without_rotating_unit_facing() {
        let value = LAYOUT.replace(
            "{type: arclight, index: 0, position: {x: 0, y: -50}}",
            "{type: crawler, index: 0, rotated: true, position: {x: 0, y: -105}}",
        );
        let layout = compile_default(&value).unwrap();
        let red = &layout.placements[1];

        assert!(red.rotated);
        assert_eq!((red.world_x, red.world_z), (0, 105));
        assert_eq!(red.rotation, 180_000);
    }
}
