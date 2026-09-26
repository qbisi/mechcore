use std::{fs, path::Path};

use mechcore_document::{NativeFormation, SidePlan};

mod constructions;

use std::collections::BTreeMap;

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
    /// Each side's tower strengthen levels, in the order the side's towers
    /// stand in the map; a side that strengthened none has none.
    pub(crate) tower_levels: BTreeMap<u32, Vec<u8>>,
}

impl CompiledLayout {
    /// A layout of units and nothing else, which is what a kernel test builds.
    #[cfg(test)]
    pub(crate) fn of_units(round: u32, placements: Vec<Placement>) -> Self {
        Self {
            round,
            placements,
            constructions: Vec::new(),
            tower_levels: BTreeMap::new(),
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

/// Everything a layout is refused for, gathered rather than stopped at.
///
/// A caller wants to know how far a deployment is from being fought, not its
/// first step, so compiling goes on past a refusal and names every one. The
/// same refusal is named once: an officer the build cannot compose refuses
/// every formation it would reach with the same words.
#[derive(Default)]
struct Refusals(Vec<String>);

impl Refusals {
    fn push(&mut self, why: impl Into<String>) {
        let why = why.into();
        if !self.0.contains(&why) {
            self.0.push(why);
        }
    }

    /// The value, or nothing with its refusal kept.
    fn hold<T>(&mut self, result: Result<T>) -> Option<T> {
        result.map_err(|error| self.push(error.to_string())).ok()
    }

    /// Whether anything was refused, and if so every refusal as one error.
    fn settle(self) -> Result<()> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(Error::new(self.0.join("; ")))
        }
    }
}

pub(crate) fn compile_with_seed(
    bytes: &[u8],
    units: &UnitConfigs,
) -> Result<(Option<i32>, CompiledLayout)> {
    let layout = mechcore_document::parse_yaml(bytes).map_err(Error::new)?;
    let plan = mechcore_document::compile_layout(layout).map_err(Error::new)?;
    let loadouts = Loadouts {
        officers: OfficerEffects::load()?,
        technologies: TechnologyEffects::load()?,
        equipment: EquipmentEffects::load()?,
        round: plan.round,
    };
    let table = Constructions::load()?;

    // Both sides are asked everything before either is refused. The registry
    // speaks first, one clause a side naming every field it owes; what the
    // registry lets through is then refused member by member.
    let mut refused = Refusals::default();
    let sides = [("blue", 0, &plan.blue), ("red", 1, &plan.red)];
    for (name, _, side) in sides {
        let missing = crate::module::unsupported(side);
        if !missing.is_empty() {
            refused.push(crate::module::refusal(name, &missing));
        }
    }
    let mut placements = Vec::new();
    // Both sides' constructions are resolved here rather than in the kernel,
    // because this is the only place a refusal can still name the side and the
    // construction it is about.
    let mut constructions = Vec::new();
    let mut tower_levels = BTreeMap::new();
    for (name, team, side) in sides {
        for (index, formation) in side.units.iter().enumerate() {
            placements.extend(compile_formation(
                name,
                team,
                index,
                formation,
                units,
                side,
                &loadouts,
                &mut refused,
            ));
        }
        constructions.extend(compile_constructions(
            name,
            team,
            side,
            &table,
            &mut refused,
        ));
        let levels = side
            .tower_strengthen_levels
            .iter()
            .map(|level| {
                u8::try_from(*level)
                    .map_err(|_| Error::new(format!("a tower strengthen level of {level}")))
            })
            .collect::<Result<Vec<_>>>();
        if let Some(levels) = refused.hold(levels) {
            tower_levels.insert(team, levels);
        }
    }
    refused.settle()?;

    Ok((
        plan.seed,
        CompiledLayout {
            round: u32::try_from(plan.round).expect("validated layout round is positive"),
            placements,
            constructions,
            tower_levels,
        },
    ))
}

fn compile_constructions(
    name: &str,
    team: u32,
    side: &SidePlan,
    table: &Constructions,
    refused: &mut Refusals,
) -> Vec<ConstructionBuilding> {
    let mut built = Vec::new();
    for placement in &side.constructions {
        let Some(buildings) = refused.hold(
            table
                .buildings(team, placement)
                .map_err(|error| Error::new(format!("side {name}: {error}"))),
        ) else {
            continue;
        };
        // Whether an officer or a technology reaches a construction's skill is
        // not read: the fight would shoot with the row's numbers where the
        // game may not. A firing construction on a side that carries either is
        // refused rather than fought without them.
        if buildings.iter().any(|building| building.skill.is_some())
            && (!side.techs.officers.is_empty() || !side.techs.units.is_empty())
        {
            refused.push(format!(
                "side {name}: {:?} fires a skill, and whether the side's officers and \
                 technologies reach it is not measured",
                placement.type_name
            ));
            continue;
        }
        built.extend(buildings);
    }
    built
}

/// One formation as the fight places it, or nothing with every reason it
/// cannot be placed kept.
#[allow(clippy::too_many_arguments)]
fn compile_formation(
    side_name: &str,
    team: u32,
    index: usize,
    formation: &mechcore_document::Placement,
    units: &UnitConfigs,
    side: &SidePlan,
    loadouts: &Loadouts,
    refused: &mut Refusals,
) -> Option<Placement> {
    // Travelling is a claimed field and was refused by the module registry;
    // what is left is a placement that is not a unit at all.
    if !matches!(formation.native, NativeFormation::Unit(_)) {
        refused.push(format!(
            "side {side_name} holds a placement that is not a unit"
        ));
        return None;
    }
    let Some(rules) = units.get(&formation.type_name) else {
        refused.push(format!(
            "side {side_name} unit type {:?} has no unit configuration",
            formation.type_name
        ));
        return None;
    };
    let fired = refused.hold(
        rules
            .fired()
            .map_err(|error| Error::new(format!("side {side_name}: {error}"))),
    );
    let fits = refused.hold(validate_formation_footprint(side_name, formation, rules));
    let level = i64::from(formation.level.unwrap_or(1));
    let corrections = loadout(
        side_name,
        &formation.type_name,
        level,
        &formation.equipment,
        rules,
        side,
        loadouts,
        refused,
    );
    let formation_index = refused.hold(
        i32::try_from(index)
            .map_err(|_| Error::new("formation index exceeds the native integer range")),
    );
    let (Some(()), Some(()), Some(corrections), Some(formation_index)) =
        (fired, fits, corrections, formation_index)
    else {
        return None;
    };
    let rotated = formation.rotated;
    let local_x = i64::from(formation.position.x);
    let local_z = i64::from(formation.position.y);
    let (world_x, world_z, rotation) = if team == 0 {
        (local_x, local_z, 0)
    } else {
        (-local_x, -local_z, 180_000)
    };
    Some(Placement {
        team,
        unit_id: 0,
        formation_id: 0,
        formation_index,
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
/// where a refusal can still say whose side and which unit it is about. Each
/// officer, technology and equipment is asked on its own, so a refusal names
/// every one this build cannot apply. Once they are known to resolve, the
/// fight applies them without a decision to make.
#[allow(clippy::too_many_arguments)]
fn loadout(
    side_name: &str,
    type_name: &str,
    level: i64,
    equipment: &[i32],
    rules: &UnitConfig,
    side: &SidePlan,
    loadouts: &Loadouts,
    refused: &mut Refusals,
) -> Option<Vec<(Channel, Entry)>> {
    let on_side = |error: Error| Error::new(format!("side {side_name}: {error}"));
    let asked = side
        .techs
        .officers
        .iter()
        .map(|id| {
            loadouts
                .officers
                .corrections(std::slice::from_ref(id), rules)
                .map_err(on_side)
        })
        .chain(side.techs.units.iter().map(|id| {
            loadouts
                .technologies
                .corrections(std::slice::from_ref(id), type_name)
                .map_err(on_side)
        }))
        .chain(equipment.iter().map(|&id| {
            loadouts
                .equipment
                .corrections(id, rules, loadouts.round)
                .map_err(on_side)
        }))
        .collect::<Vec<_>>();
    let mut corrections = Vec::new();
    let mut resolved = true;
    for answer in asked {
        match refused.hold(answer) {
            Some(written) => corrections.extend(written),
            None => resolved = false,
        }
    }
    if !resolved {
        return None;
    }
    let refusal = |error: Error| {
        Error::new(format!(
            "side {side_name} unit type {type_name:?} carries a loadout this \
             build cannot resolve: {error}"
        ))
    };
    let stats = refused.hold(Stats::corrected(rules, level, &corrections).map_err(refusal))?;
    // A snapshot carries each `DataSet`'s aggregate; one this build cannot
    // record is refused here, where the side and the officer can be named.
    refused.hold(stats.unit_dynamic_modifiers().map_err(refusal))?;
    refused.hold(stats.skill_dynamic_modifiers(1).map_err(refusal))?;
    Some(corrections)
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

    /// A layout refused for several things is refused for all of them at
    /// once, each named once however many formations it reaches.
    #[test]
    fn a_refusal_names_everything_the_layout_is_refused_for() {
        let value = LAYOUT
            .replace(
                "blue:\n  units: [{name: marksman, index: 0, position: {x: 0, y: -50}}]",
                "blue:\n  officers: [berserk_rhino]\n  units:\n  - {name: marksman, index: 0, position: {x: 0, y: -50}}\n  - {name: marksman, index: 1, position: {x: 20, y: -50}}\n  terrains: [{name: oil, control_points: [{x: -60, y: 40}, {x: 60, y: 40}]}]",
            )
            .replace(
                "units: [{name: arclight, index: 0, position: {x: 0, y: -50}}]",
                "units:\n  - {name: sandworm, index: 0, position: {x: 0, y: -160}}\n  - {name: war_factory, index: 1, position: {x: 75, y: -165}}",
            );
        let refused = compile_default(&value).unwrap_err().to_string();
        let clauses: Vec<&str> = refused.split("; ").collect();
        assert_eq!(clauses.len(), 4, "{refused}");
        assert!(
            clauses[0].contains("terrains (RangeItemSystem)"),
            "{refused}"
        );
        assert!(clauses[1].contains("30502"), "{refused}");
        assert!(
            clauses[2].contains("\"sandworm\" has no unit configuration"),
            "{refused}"
        );
        assert!(
            clauses[3].contains("\"war_factory\" has no unit configuration"),
            "{refused}"
        );
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
