use std::{fs, path::Path};

use mechcore_document::{NativeFormation, SidePlan};

mod constructions;

use crate::{
    Error, Result,
    data::{Channel, Entry, Stats},
    modifier::{EquipmentEffects, OfficerEffects, TechnologyEffects},
    rules::{UnitConfig, UnitConfigs},
};
pub(crate) use constructions::ConstructionBuilding;
use constructions::Constructions;

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
    /// The formation's level, 1 to 9: its `IMechLevelData` rating.
    pub(crate) level: i64,
    /// What the side's loadout wrote onto this formation, in the channel each
    /// correction belongs to. The entries are verified to resolve while the
    /// layout is compiled, which is the only place that can name the side and
    /// the officer in a refusal.
    pub(crate) corrections: Vec<(Channel, Entry)>,
}

#[derive(Debug, Clone)]
pub(crate) struct CompiledLayout {
    pub(crate) round: u32,
    pub(crate) placements: Vec<Placement>,
    /// The buildings this layout's constructions put on the board, both sides
    /// together. One `constructions` entry answers several of them, and the
    /// fight sees buildings rather than constructions, so the placement they
    /// came from is not carried past here.
    pub(crate) constructions: Vec<ConstructionBuilding>,
}

impl CompiledLayout {
    /// A layout of units and nothing else, which is what a kernel test builds.
    #[cfg(test)]
    pub(crate) fn of_units(round: u32, placements: Vec<Placement>) -> Self {
        Self {
            round,
            placements,
            constructions: Vec::new(),
        }
    }
}

pub(crate) fn load(
    path: &Path,
    units: &UnitConfigs,
) -> Result<(Option<i32>, CompiledLayout, String)> {
    let bytes = fs::read(path)
        .map_err(|error| Error::new(format!("failed to read {}: {error}", path.display())))?;
    read(&bytes, units).map_err(|error| {
        Error::new(format!(
            "cannot simulate layout {}: {error}",
            path.display()
        ))
    })
}

/// A layout held in memory, compiled and kept in its normal form.
///
/// A caller that has the document rather than a file on disk says so, and its
/// errors then name what is wrong with the layout rather than where it was
/// read from.
pub(crate) fn read(
    bytes: &[u8],
    units: &UnitConfigs,
) -> Result<(Option<i32>, CompiledLayout, String)> {
    let (seed, layout) = compile_with_seed(bytes, units)?;
    let parsed = mechcore_document::parse_yaml(bytes).map_err(Error::new)?;
    let canonical = mechcore_document::canonical_yaml(parsed).map_err(Error::new)?;
    Ok((seed, layout, canonical))
}

#[cfg(test)]
fn compile(bytes: &[u8], units: &UnitConfigs) -> Result<CompiledLayout> {
    compile_with_seed(bytes, units).map(|(_, layout)| layout)
}

/// The tables a side's corrections are read from.
///
/// One per source of an `ICommonMechDataChangeDataSource`: officers,
/// technologies and equipment today, the energy tower's skills when their
/// table is extracted. The round is what an equipment's lifetime is read in.
struct Loadouts {
    officers: OfficerEffects,
    technologies: TechnologyEffects,
    equipment: EquipmentEffects,
    round: i32,
}

pub(crate) fn compile_with_seed(
    bytes: &[u8],
    units: &UnitConfigs,
) -> Result<(Option<i32>, CompiledLayout)> {
    let layout = mechcore_document::parse_yaml(bytes).map_err(Error::new)?;
    let plan = mechcore_document::compile_layout(layout).map_err(Error::new)?;
    // Both sides are asked before either is refused, so a caller sees the whole
    // distance between this deployment and a fight rather than its first step.
    let missing: Vec<String> = [("blue", &plan.blue), ("red", &plan.red)]
        .into_iter()
        .filter_map(|(name, side)| {
            let missing = crate::module::unsupported(side);
            (!missing.is_empty()).then(|| crate::module::refusal(name, &missing))
        })
        .collect();
    if !missing.is_empty() {
        return Err(Error::new(missing.join("; ")));
    }
    let loadouts = Loadouts {
        officers: OfficerEffects::load()?,
        technologies: TechnologyEffects::load()?,
        equipment: EquipmentEffects::load()?,
        round: plan.round,
    };
    let mut placements = compile_side("blue", 0, &plan.blue, units, &loadouts)?;
    placements.extend(compile_side("red", 1, &plan.red, units, &loadouts)?);

    // Both sides' constructions are resolved here rather than in the kernel,
    // because this is the only place a refusal can still name the side and the
    // construction it is about.
    let table = Constructions::load()?;
    let mut constructions = compile_constructions("blue", 0, &plan.blue, &table)?;
    constructions.extend(compile_constructions("red", 1, &plan.red, &table)?);

    Ok((
        plan.seed,
        CompiledLayout {
            round: u32::try_from(plan.round).expect("validated layout round is positive"),
            placements,
            constructions,
        },
    ))
}

fn compile_constructions(
    name: &str,
    team: u32,
    side: &SidePlan,
    table: &Constructions,
) -> Result<Vec<ConstructionBuilding>> {
    let mut built = Vec::new();
    for placement in &side.constructions {
        let buildings = table
            .buildings(team, placement)
            .map_err(|error| Error::new(format!("side {name}: {error}")))?;
        // Whether an officer or a technology reaches a construction's skill is
        // not read: the fight would shoot with the row's numbers where the
        // game may not. A firing construction on a side that carries either is
        // refused rather than fought without them.
        if buildings.iter().any(|building| building.skill.is_some())
            && (!side.techs.officers.is_empty() || !side.techs.units.is_empty())
        {
            return Err(Error::new(format!(
                "side {name}: {:?} fires a skill, and whether the side's officers and \
                 technologies reach it is not measured",
                placement.type_name
            )));
        }
        built.extend(buildings);
    }
    Ok(built)
}

fn compile_side(
    name: &str,
    team: u32,
    side: &SidePlan,
    units: &UnitConfigs,
    loadouts: &Loadouts,
) -> Result<Vec<Placement>> {
    side.units
        .iter()
        .enumerate()
        .map(|(index, formation)| {
            compile_formation(name, team, index, formation, units, side, loadouts)
        })
        .collect()
}

fn compile_formation(
    side_name: &str,
    team: u32,
    index: usize,
    formation: &mechcore_document::Placement,
    units: &UnitConfigs,
    side: &SidePlan,
    loadouts: &Loadouts,
) -> Result<Placement> {
    // Travelling is a claimed field and was refused by the module registry
    // before this ran; what is left is a placement that is
    // not a unit at all.
    if !matches!(formation.native, NativeFormation::Unit(_)) {
        return Err(Error::new(format!(
            "side {side_name} holds a placement that is not a unit"
        )));
    }
    let rotated = formation.rotated;
    let rules = units.get(&formation.type_name).ok_or_else(|| {
        Error::new(format!(
            "side {side_name} unit type {:?} has no unit configuration",
            formation.type_name
        ))
    })?;
    validate_formation_footprint(side_name, formation, rules)?;
    let level = i64::from(formation.level.unwrap_or(1));
    let corrections = loadout(
        side_name,
        &formation.type_name,
        level,
        formation.equipment,
        rules,
        side,
        loadouts,
    )?;
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
        level,
        corrections,
    })
}

/// What this side's loadout and a formation's equipment write onto it.
///
/// The corrections are resolved here as well as gathered, because this is
/// where a refusal can still say whose side and which unit it is about. Once
/// they are known to resolve, the fight applies them without a decision to
/// make.
fn loadout(
    side_name: &str,
    type_name: &str,
    level: i64,
    equipment: Option<i32>,
    rules: &UnitConfig,
    side: &SidePlan,
    loadouts: &Loadouts,
) -> Result<Vec<(Channel, Entry)>> {
    let mut corrections = loadouts
        .officers
        .corrections(&side.techs.officers, rules)
        .map_err(|error| Error::new(format!("side {side_name}: {error}")))?;
    corrections.extend(
        loadouts
            .technologies
            .corrections(&side.techs.units, type_name)
            .map_err(|error| Error::new(format!("side {side_name}: {error}")))?,
    );
    if let Some(id) = equipment {
        corrections.extend(
            loadouts
                .equipment
                .corrections(id, rules, loadouts.round)
                .map_err(|error| Error::new(format!("side {side_name}: {error}")))?,
        );
    }
    let refused = |error: Error| {
        Error::new(format!(
            "side {side_name} unit type {type_name:?} carries a loadout this \
             build cannot resolve: {error}"
        ))
    };
    let stats = Stats::corrected(rules, level, &corrections).map_err(refused)?;
    // A snapshot carries each `DataSet`'s aggregate; one this build cannot
    // record is refused here, where the side and the officer can be named.
    stats.unit_dynamic_modifiers().map_err(refused)?;
    stats.skill_dynamic_modifiers(1).map_err(refused)?;
    Ok(corrections)
}

fn validate_formation_footprint(
    side_name: &str,
    formation: &mechcore_document::Placement,
    rules: &UnitConfig,
) -> Result<()> {
    let configured = rules.formation_footprint_meters()?;
    if formation.footprint == Some(configured) {
        Ok(())
    } else {
        Err(Error::new(format!(
            "side {side_name} unit type {:?} layout footprint {:?} does not match simulator configuration {:?}",
            formation.type_name, formation.footprint, configured
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::SimulationConfig;

    const LAYOUT: &str = r"
kind: layout
round: 1
blue:
  units: [{name: marksman, index: 0, position: {x: 0, y: -50}}]
red:
  units: [{name: arclight, index: 0, position: {x: 0, y: -50}}]
";

    fn compile_default(value: &str) -> Result<CompiledLayout> {
        let config = SimulationConfig::load()?;
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
            "units: [{name: marksman, index: 0, position: {x: 0, y: -50}}]",
            "units:\n      - {name: arclight, index: 0, position: {x: 20, y: -100}}\n      - {name: rhino, index: 1, position: {x: -15, y: -105}}",
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

    /// An officer reaches the fight as corrections on the units it targets.
    ///
    /// The game agrees with this one to the tick: the same layout is
    /// `marksman-vs-rhino-officer-damage` in `tests/regression/mcfr-regressions.yaml`,
    /// recorded natively.
    #[test]
    fn an_officer_writes_onto_the_units_it_reaches() {
        let value = LAYOUT.replace(
            "blue:\n  units:",
            "blue:\n  officers: [advanced_offensive_tactics]\n  units:",
        );
        let layout = compile_default(&value).unwrap();
        let blue = &layout.placements[0];
        assert_eq!(blue.type_name, "marksman");
        assert_eq!(blue.corrections.len(), 1, "one officer, one rate");
        assert_eq!(blue.corrections[0].0, Channel::Skill);
        assert!(
            layout.placements[1].corrections.is_empty(),
            "the other side holds no officer"
        );
    }

    /// A side that carries an officer this build cannot apply is refused, and
    /// the refusal names the side, the officer and what is missing.
    #[test]
    fn an_officer_this_build_cannot_apply_refuses_the_side_that_holds_it() {
        let value = LAYOUT.replace(
            "blue:\n  units:",
            "blue:\n  officers: [berserk_rhino]\n  units:",
        );
        let refused = compile_default(&value).unwrap_err().to_string();
        assert!(refused.contains("side blue"), "{refused}");
        assert!(refused.contains("30502"), "{refused}");
        assert!(refused.contains("kills"), "{refused}");
    }

    /// An officer that only touches a ledger reaches the fight as nothing,
    /// rather than as a refusal.
    #[test]
    fn an_officer_with_no_combat_effect_compiles_to_no_correction() {
        let value = LAYOUT.replace(
            "blue:\n  units:",
            "blue:\n  officers: [supply_specialist]\n  units:",
        );
        let layout = compile_default(&value).unwrap();
        assert!(layout.placements[0].corrections.is_empty());
    }

    /// A technology reaches the fight as corrections on the units its table
    /// row names, and on nothing else.
    #[test]
    fn a_technology_writes_onto_the_unit_that_researched_it() {
        let value = LAYOUT.replace(
            "units: [{name: marksman, index: 0, position: {x: 0, y: -50}}]",
            "techs: {marksman: [range_enhancement]}\n  units: [{name: marksman, index: 0, position: {x: 0, y: -50}}]",
        );
        let layout = compile_default(&value).unwrap();
        let blue = &layout.placements[0];
        assert_eq!(blue.type_name, "marksman");
        assert_eq!(blue.corrections.len(), 1, "forty metres of range");
        assert_eq!(blue.corrections[0].0, Channel::Skill);
        assert!(layout.placements[1].corrections.is_empty());
    }

    /// A Defensive Wall reaches the fight as the five buildings it is, and
    /// the layout that carries it compiles.
    #[test]
    fn a_wall_reaches_the_fight_as_five_buildings() {
        let value = LAYOUT.replace(
            "units: [{name: marksman, index: 0, position: {x: 0, y: -50}}]",
            "units: [{name: marksman, index: 0, position: {x: 0, y: -50}}]\n  constructions: [{name: defensive_wall, index: 0, position: {x: 140, y: -105}}]",
        );
        let layout = compile_default(&value).unwrap();
        assert_eq!(layout.constructions.len(), 5);
        assert_eq!(
            layout
                .constructions
                .iter()
                .map(|building| building.x / 1_000)
                .collect::<Vec<_>>(),
            [116, 128, 140, 152, 164]
        );
    }

    /// A construction this build will not place refuses the side that carries
    /// it, and the refusal names the construction rather than the field: the
    /// field is understood and this one member of it is not.
    #[test]
    fn a_magnetic_barrier_refuses_the_side_that_placed_it() {
        let value = LAYOUT.replace(
            "units: [{name: marksman, index: 0, position: {x: 0, y: -50}}]",
            "units: [{name: marksman, index: 0, position: {x: 0, y: -50}}]\n  constructions: [{name: magnetic_barrier, index: 0, position: {x: -145, y: -55}}]",
        );
        let refused = compile_default(&value).unwrap_err().to_string();
        assert!(refused.contains("side blue"), "{refused}");
        assert!(refused.contains("construction 4"), "{refused}");
        assert!(refused.contains("10 objects over 2 rows"), "{refused}");
    }

    /// A turret is placed; beside an officer it is refused, because whether
    /// the officer reaches its skill is not measured.
    #[test]
    fn a_turret_beside_an_officer_is_refused() {
        let turret = LAYOUT.replace(
            "units: [{name: marksman, index: 0, position: {x: 0, y: -50}}]",
            "units: [{name: marksman, index: 0, position: {x: 0, y: -50}}]\n  constructions: [{name: rapid_fire_turret, index: 1, position: {x: 140, y: -100}}]",
        );
        let compiled = compile_default(&turret).unwrap();
        assert!(
            compiled.constructions[0].skill.is_some(),
            "the turret fires"
        );
        let with_officer = turret.replace("blue:\n", "blue:\n  officers: [improved_wasp]\n");
        assert_ne!(with_officer, turret, "the fixture carries an officer");
        let refused = compile_default(&with_officer).unwrap_err().to_string();
        assert!(refused.contains("side blue"), "{refused}");
        assert!(refused.contains("rapid_fire_turret"), "{refused}");
        assert!(refused.contains("officers and technologies"), "{refused}");
    }

    #[test]
    fn rejects_persistent_terrains_outside_simulator_closure() {
        let value = LAYOUT.replace(
            "units: [{name: marksman, index: 0, position: {x: 0, y: -50}}]",
            "units: [{name: marksman, index: 0, position: {x: 0, y: -50}}]\n  terrains: [{name: oil, control_points: [{x: -60, y: 40}, {x: 60, y: 40}]}]",
        );
        assert_eq!(
            compile_default(&value).unwrap_err().to_string(),
            "side blue needs modules this build has not implemented: \
             terrains (RangeItemSystem)"
        );
    }

    #[test]
    fn compiles_multi_member_formations_as_one_deployment_placement() {
        let value = LAYOUT.replace(
            "{name: marksman, index: 0, position: {x: 0, y: -50}}",
            "{name: crawler, index: 0, position: {x: 5, y: -50}}",
        );
        let layout = compile_default(&value).unwrap();
        assert_eq!(layout.placements.len(), 2);
        assert_eq!(layout.placements[0].type_name, "crawler");
    }

    #[test]
    fn rotated_formation_swaps_config_footprint_without_rotating_unit_facing() {
        let value = LAYOUT.replace(
            "{name: arclight, index: 0, position: {x: 0, y: -50}}",
            "{name: crawler, index: 0, rotated: true, position: {x: 0, y: -105}}",
        );
        let layout = compile_default(&value).unwrap();
        let red = &layout.placements[1];

        assert!(red.rotated);
        assert_eq!((red.world_x, red.world_z), (0, 105));
        assert_eq!(red.rotation, 180_000);
    }
}
