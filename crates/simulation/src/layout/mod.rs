use std::{fs, path::Path};

use mechcore_document::{NativeFormation, SidePlan};

mod commander_skills;
mod constructions;
mod contraptions;

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    Error, Result,
    data::{Channel, Correction, Entry, ExperienceRate, Index, Stats},
    modifier::{
        AutoRecovery, BuffSource, CarriedShield, DeadSummon, EnergyShield, EnergyTowerSkillEffects,
        EquipmentEffects, LifeSteal, MainSkill, OfficerEffects, ProductionLine, SecondaryDamage,
        SweepIntensify, TECHNOLOGY_SOURCE, TechnologyEffects, UnitInterception, current_source,
    },
    rules::{ExtraWeaponConfig, UnitConfig, UnitConfigs, UnitDomain},
};
use commander_skills::{CommanderSkillEffects, technology_buff};
pub(crate) use commander_skills::{
    ItemBuff, Scatter, SkillBuff, SkillEffect, SkillRelease, StandingOil, SubEffect, Summon,
    TerrainEffect, TerrainKind, TerrainSpec, buff_item_terrain,
};
pub(crate) use constructions::ConstructionBuilding;
use constructions::Constructions;
use contraptions::Contraptions;
pub(crate) use contraptions::ShieldKind;
pub(crate) use contraptions::{
    InterceptNumbers, Interception, InterceptorBuilding, MissileMine, MissileShot, ShieldPlacement,
};

#[derive(Debug, Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each flag is an independent fact the unit's equipment or deployment gives it"
)]
pub(crate) struct Placement {
    pub(crate) team: u32,
    pub(crate) unit_id: u64,
    pub(crate) formation_id: u64,
    /// The native side-local `UnitIndex`, which seeds the formation's layout
    /// stream: the document's `index`, which a sold unit leaves a gap in,
    /// and the declaration order only for a document that states none.
    pub(crate) formation_index: i32,
    pub(crate) type_name: String,
    pub(crate) world_x: i64,
    pub(crate) world_z: i64,
    pub(crate) rotation: i64,
    pub(crate) rotated: bool,
    /// The formation's level, 1 to 9: its `IMechLevelData` rating.
    pub(crate) level: i64,
    /// The experience the formation brings into the fight, whole: the
    /// layout's `exp` within its level, 0 when it has none.
    pub(crate) exp: i64,
    /// The rate its side's officers put on what its formation gains.
    pub(crate) experience_rate: ExperienceRate,
    /// The rate its technologies put on what it gains: the unit's own
    /// `MechDataChangeFloatRate.ExpChangeRate`.
    pub(crate) unit_experience_rate: ExperienceRate,
    /// What the side's loadout wrote onto this formation, in the channel each
    /// correction belongs to. The entries are verified to resolve while the
    /// layout is compiled, which is the only place that can name the side and
    /// the officer in a refusal.
    pub(crate) corrections: Vec<(Channel, Entry)>,
    /// The `ILifeSteal` its `LifeStealEffectProvider` enables, if its
    /// technologies or equipment hand it one.
    pub(crate) lifesteal: Option<LifeSteal>,
    /// The `IAutoRecovery` its `AutoRecoveryEffectProvider` enables, if its
    /// technologies or equipment hand it one.
    pub(crate) auto_recovery: Option<AutoRecovery>,
    /// The `IEnergyShieldSource` its `EnergyShieldProvider` enables, if its
    /// technologies or equipment hand it one.
    pub(crate) energy_shield: Option<EnergyShield>,
    /// What its technologies hand its sweep (`SweepSkillIntensifyTech`).
    pub(crate) sweep: Option<SweepIntensify>,
    /// Whether its technologies turn its main skill's search to
    /// `DistanceIntensify` (`SearchTargetSpecificTech`).
    pub(crate) distance_intensify: bool,
    /// The second damage its technologies make its main skill deal around
    /// each hit (`SecondaryDamageIntensifyTech`).
    pub(crate) secondary_damage: Option<SecondaryDamage>,
    /// The interceptors its technologies make it (`InterceptMissileTech`).
    pub(crate) interception: Option<UnitInterception>,
    /// The battlefield shield its equipment makes it carry.
    pub(crate) carried_shield: Option<CarriedShield>,
    /// The production line its equipment makes it run.
    pub(crate) production: Option<Production>,
    /// The buffs its equipment adds to it as the fight starts.
    pub(crate) buff_sources: Vec<BuffSource>,
    /// The `buffDatas` rows its equipment makes it ignore,
    /// `BuffManager.ignoredBuffs`.
    pub(crate) ignored_buffs: Vec<u32>,
    /// Whether its equipment makes it an important unit
    /// (`FightMech.IsImportant`): its side does not outlive it.
    pub(crate) important: bool,
    /// Whether its equipment keeps every control beam from turning it,
    /// `TeamTranslationSystem.IsIgnoredMech`.
    pub(crate) ignores_control_beam: bool,
    /// Whether it opens the fight travelling: a unit deployed into an ambush
    /// zone, which `SuperDeploymentSystem` holds until its side arrives.
    pub(crate) travelling: bool,
    /// The extra weapons its technologies add beside its main skill.
    pub(crate) extra_weapons: Vec<ExtraWeapon>,
    /// What a buff that disables technology switches off on it.
    pub(crate) technology_disable: TechnologyDisable,
}

/// An extra weapon a unit's technology adds, and the fire its hit leaves.
#[derive(Debug, Clone)]
pub(crate) struct ExtraWeapon {
    pub(crate) rules: ExtraWeaponConfig,
    /// The terrain its hit leaves: a fire or an oil.
    pub(crate) terrain: Option<TerrainSpec>,
    /// The buff its hit writes on what it struck.
    pub(crate) buff: Option<SkillBuff>,
    /// The fire an explosion skill's unit's death leaves.
    pub(crate) dead_fire: Option<TerrainSpec>,
    /// Whether its skills join the main skill's group: a grouped row on a
    /// unit whose main skill is grouped (`FightSkillFactory.PrepareGroupedSkill`).
    pub(crate) joins_main_group: bool,
    /// What its own `DataSet` holds, for a skill without a damage rate: the
    /// skill corrections of the equipment and Energy Tower skills that reach
    /// it. A skill with a damage rate holds the main skill's.
    pub(crate) skill_corrections: Vec<Entry>,
}

/// A production line a unit runs, with what it makes resolved: the unit's
/// description and what its side's officers, technologies and Energy Tower
/// skills write onto that type, as `FightEffectSystem` registers them for a
/// unit of it deployed with no equipment.
#[derive(Debug, Clone)]
pub(crate) struct Production {
    pub(crate) line: ProductionLine,
    pub(crate) rules: UnitConfig,
    pub(crate) corrections: Vec<(Channel, Entry)>,
    pub(crate) technology_disable: TechnologyDisable,
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
    /// The interceptors both sides release, each side's in the order its
    /// layout lists them.
    pub(crate) interceptors: Vec<InterceptorBuilding>,
    /// The missiles both sides release, each side's in the order its layout
    /// lists them.
    pub(crate) missiles: Vec<MissileMine>,
    /// The shields both sides release, each side's in the order its layout
    /// lists them.
    pub(crate) shields: Vec<ShieldPlacement>,
    /// The battle skills both sides release, each side's in the order its
    /// layout lists them.
    pub(crate) battle_skills: Vec<SkillRelease>,
    /// The oil earlier rounds left, blue's and then red's, each side's in the
    /// order its layout lists them.
    pub(crate) standing_oil: Vec<StandingOil>,
    /// Each side's `legacy_index`: a formation of a lower index is one the
    /// side carried into the round.
    pub(crate) legacy_units: BTreeMap<u32, i32>,
    /// The formations an officer delivered as the round opened, by side and
    /// layout index: the last of each side's legacy formations.
    pub(crate) delivered: BTreeSet<(u32, i32)>,
    /// Each side's `superDeploymentTimeChangeRate`, Q32.32, where an officer
    /// sets one.
    pub(crate) travel_time_rates: BTreeMap<u32, i64>,
    /// Each side's tower strengthen levels, in the order the side's towers
    /// stand in the map; a side that strengthened none has none.
    pub(crate) tower_levels: BTreeMap<u32, Vec<u8>>,
    /// The map the fight is on: the layout's, or the Training Ground's when
    /// it names none, as a layout replay loads it.
    pub(crate) map_id: i32,
    /// What each side's buffs make a unit summon as it dies, by side and the
    /// summoned unit's type id ([`compile_death_summons`]).
    pub(crate) death_summons: BTreeMap<(u32, u32), DeathSummon>,
}

/// A unit a side's buff makes a dying unit summon: its description, and its
/// placement but for where it stands and who it is.
#[derive(Debug, Clone)]
pub(crate) struct DeathSummon {
    pub(crate) rules: UnitConfig,
    pub(crate) placement: Placement,
}

impl CompiledLayout {
    /// A layout of units and nothing else, which is what a kernel test builds.
    #[cfg(test)]
    pub(crate) fn of_units(round: u32, placements: Vec<Placement>) -> Self {
        Self {
            round,
            placements,
            constructions: Vec::new(),
            interceptors: Vec::new(),
            missiles: Vec::new(),
            shields: Vec::new(),
            battle_skills: Vec::new(),
            standing_oil: Vec::new(),
            legacy_units: BTreeMap::new(),
            delivered: BTreeSet::new(),
            travel_time_rates: BTreeMap::new(),
            tower_levels: BTreeMap::new(),
            map_id: mechcore_document::layout_replay::DEFAULT_MAP_ID,
            death_summons: BTreeMap::new(),
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
/// technologies, equipment and the energy tower's skills; and the battle
/// skills', whose `Config` fire an extra weapon's fire deals.
struct Loadouts {
    officers: OfficerEffects,
    technologies: TechnologyEffects,
    equipment: EquipmentEffects,
    energy_tower: EnergyTowerSkillEffects,
    skill_effects: CommanderSkillEffects,
}

impl Loadouts {
    fn load() -> Result<Self> {
        Ok(Self {
            officers: OfficerEffects::load()?,
            technologies: TechnologyEffects::load()?,
            equipment: EquipmentEffects::load()?,
            energy_tower: EnergyTowerSkillEffects::load()?,
            skill_effects: CommanderSkillEffects::load()?,
        })
    }
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

/// The registry's refusals: one clause a side naming every field it owes.
fn registry_refusals(sides: &[(&str, u32, &SidePlan); 2]) -> Refusals {
    let mut refused = Refusals::default();
    for (name, _, side) in sides {
        let missing = crate::module::unsupported(side);
        if !missing.is_empty() {
            refused.push(crate::module::refusal(name, &missing));
        }
    }
    refused
}

/// The formations a side's officers delivered as the round opened, by side
/// and layout index, or none when the side's units cannot be those squads.
fn delivered_formations(
    (name, team, side): (&str, u32, &SidePlan),
    round: i32,
    refused: &mut Refusals,
) -> Vec<(u32, i32)> {
    refused
        .hold(
            mechcore_document::layout_replay::delivered_units(side, round)
                .map_err(|reason| Error::new(format!("side {name}: {reason}"))),
        )
        .unwrap_or_default()
        .into_iter()
        .filter_map(|at| side.units[at].index.map(|index| (team, index)))
        .collect()
}

pub(crate) fn compile_with_seed(
    bytes: &[u8],
    units: &UnitConfigs,
) -> Result<(Option<i32>, CompiledLayout)> {
    let layout = mechcore_document::parse_yaml(bytes).map_err(Error::new)?;
    let plan = mechcore_document::compile_layout(layout).map_err(Error::new)?;
    let loadouts = Loadouts::load()?;
    let table = Constructions::load()?;
    let contraptions = Contraptions::load()?;
    let skill_effects = &loadouts.skill_effects;

    // Both sides are asked everything before either is refused. The registry
    // speaks first, one clause a side naming every field it owes; what the
    // registry lets through is then refused member by member.
    let sides = [("blue", 0, &plan.blue), ("red", 1, &plan.red)];
    let mut refused = registry_refusals(&sides);
    let mut placements = Vec::new();
    // Both sides' constructions are resolved here rather than in the kernel,
    // because this is the only place a refusal can still name the side and the
    // construction it is about.
    let mut constructions = Vec::new();
    let mut interceptors = Vec::new();
    let mut missiles = Vec::new();
    let mut shields = Vec::new();
    let mut battle_skills = Vec::new();
    let mut standing_oil = Vec::new();
    let mut tower_levels = BTreeMap::new();
    let mut delivered = BTreeSet::new();
    let mut death_summons = BTreeMap::new();
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
        death_summons.extend(compile_death_summons(
            (name, team, side),
            &placements,
            units,
            &loadouts,
            &mut refused,
        ));
        constructions.extend(compile_constructions(
            name,
            team,
            side,
            &table,
            &mut refused,
        ));
        compile_contraptions(
            name,
            team,
            side,
            (&contraptions, &loadouts.officers),
            &mut refused,
            (&mut interceptors, &mut missiles, &mut shields),
        );
        let standing = compile_standing(name, team, side, skill_effects, &mut refused);
        shields.extend(standing.0);
        standing_oil.extend(standing.1);
        battle_skills.extend(compile_battle_skills(
            name,
            team,
            side,
            skill_effects,
            units,
            &loadouts,
            &mut refused,
        ));
        if let Some(levels) = refused.hold(tower_strengthen_levels(side)) {
            tower_levels.insert(team, levels);
        }
        delivered.extend(delivered_formations(
            (name, team, side),
            plan.round,
            &mut refused,
        ));
    }
    refused.settle()?;

    Ok((
        plan.seed,
        CompiledLayout {
            round: u32::try_from(plan.round).expect("validated layout round is positive"),
            placements,
            constructions,
            interceptors,
            missiles,
            shields,
            battle_skills,
            standing_oil,
            legacy_units: sides
                .iter()
                .map(|(_, team, side)| (*team, side.legacy_unit))
                .collect(),
            delivered,
            // The rate a side's officers set on its travel time, where one does.
            travel_time_rates: travel_time_rates(&sides, &loadouts.officers),
            tower_levels,
            map_id: plan
                .map_id
                .unwrap_or(mechcore_document::layout_replay::DEFAULT_MAP_ID),
            death_summons,
        },
    ))
}

/// The units a side's buffs make a unit summon as it dies
/// (`IBEC_DeadSummon`), by side and type id: each type a buff of one of the
/// side's placements summons, and each a buff of one of these summons in
/// turn. `SummonSystem.DoCreateMech` makes it at `CardLevel.Level1` with no
/// equipment, and `FightController.CreateMech` gives it its side's
/// technologies when its parent `IsChildInheritTechnologyEffect`, which
/// `MechData` answers for every unit but types 4001 and 5203.
fn compile_death_summons(
    (name, team, side): (&str, u32, &SidePlan),
    placements: &[Placement],
    units: &UnitConfigs,
    loadouts: &Loadouts,
    refused: &mut Refusals,
) -> BTreeMap<(u32, u32), DeathSummon> {
    let summoned = |placement: &Placement| -> Vec<u32> {
        let own = units
            .get(&placement.type_name)
            .map(|rules| rules.unit_type_id);
        placement
            .buff_sources
            .iter()
            .filter_map(|source| match source.summons? {
                DeadSummon::SourceType => own,
                DeadSummon::Unit(id) => u32::try_from(id).ok(),
            })
            .collect()
    };
    let mut pending = placements
        .iter()
        .filter(|placement| placement.team == team)
        .flat_map(summoned)
        .collect::<Vec<_>>();
    let mut templates = BTreeMap::new();
    while let Some(type_id) = pending.pop() {
        if templates.contains_key(&(team, type_id)) {
            continue;
        }
        let Some(rules) = units.by_type_id(type_id) else {
            refused.push(format!(
                "side {name} summons unit {type_id} as a buffed unit dies, which has no unit \
                 configuration"
            ));
            continue;
        };
        if matches!(rules.unit_type_id, 4001 | 5203) {
            refused.push(format!(
                "side {name} summons a {} as a buffed unit dies, whose parent lends it no \
                 technologies, which is not read",
                rules.type_name
            ));
            continue;
        }
        let Some(worn) = loadout(
            name,
            &rules.type_name,
            1,
            &[],
            rules,
            side,
            loadouts,
            refused,
        ) else {
            continue;
        };
        if worn.interception.is_some() {
            refused.push(format!(
                "side {name} summons a {} as a buffed unit dies, which its technologies make \
                 an interceptor, and when a summon's interceptors start is not measured",
                rules.type_name
            ));
            continue;
        }
        let template = Placement {
            team,
            unit_id: 0,
            formation_id: 0,
            formation_index: -1,
            type_name: rules.type_name.clone(),
            world_x: 0,
            world_z: 0,
            rotation: if team == 0 { 0 } else { 180_000 },
            rotated: false,
            level: 1,
            exp: 0,
            experience_rate: worn.experience_rates.0,
            unit_experience_rate: worn.experience_rates.1,
            corrections: worn.corrections,
            lifesteal: worn.lifesteal,
            auto_recovery: worn.auto_recovery,
            energy_shield: worn.energy_shield,
            sweep: worn.sweep,
            distance_intensify: worn.distance_intensify,
            secondary_damage: worn.secondary_damage,
            interception: worn.interception,
            carried_shield: worn.carried_shield,
            production: None,
            buff_sources: worn.buff_sources,
            ignored_buffs: worn.ignored_buffs,
            important: worn.important,
            ignores_control_beam: worn.ignores_control_beam,
            travelling: false,
            extra_weapons: worn.extra_weapons,
            technology_disable: worn.technology_disable,
        };
        pending.extend(summoned(&template));
        templates.insert(
            (team, type_id),
            DeathSummon {
                rules: rules.clone(),
                placement: template,
            },
        );
    }
    templates
}

/// The rate each side's officers set on its travel time, where one does.
fn travel_time_rates(
    sides: &[(&str, u32, &SidePlan); 2],
    officers: &OfficerEffects,
) -> BTreeMap<u32, i64> {
    sides
        .iter()
        .map(|(_, team, side)| {
            (
                *team,
                officers.super_deployment_time_rate(&side.techs.officers),
            )
        })
        .filter(|(_, rate)| *rate != 0)
        .collect()
}

/// A side's contraptions: an interceptor released as `CRC_Interceptor`
/// releases it, a missile as `CRC_Mine` does, and a shield as
/// `CRC_EnergyShield`.
fn compile_contraptions(
    name: &str,
    team: u32,
    side: &SidePlan,
    (contraptions, officers): (&Contraptions, &OfficerEffects),
    refused: &mut Refusals,
    (interceptors, missiles, shields): (
        &mut Vec<InterceptorBuilding>,
        &mut Vec<MissileMine>,
        &mut Vec<ShieldPlacement>,
    ),
) {
    // The side's officers rate its shield and missile kinds alike.
    let rates = officers.contraption_rates(&side.techs.officers);
    for placement in &side.contraptions {
        let located = |error: Error| Error::new(format!("side {name}: {error}"));
        match placement.type_name.as_str() {
            "interceptor" => interceptors
                .extend(refused.hold(contraptions.interceptor(team, placement).map_err(located))),
            "missile" => missiles.extend(
                refused.hold(
                    contraptions
                        .missile(team, placement, rates)
                        .map_err(located),
                ),
            ),
            "shield" => {
                shields.extend(
                    refused.hold(contraptions.shield(team, placement, rates).map_err(located)),
                );
            }
            _ => {}
        }
    }
}

/// A side's tower strengthen levels, in the order the side's towers stand in
/// the map.
fn tower_strengthen_levels(side: &SidePlan) -> Result<Vec<u8>> {
    side.tower_strengthen_levels
        .iter()
        .map(|level| {
            u8::try_from(*level)
                .map_err(|_| Error::new(format!("a tower strengthen level of {level}")))
        })
        .collect()
}

/// What earlier rounds left standing on a side: its Shield Airdrops,
/// installed before any release as shields of the side, and its oil,
/// restored before the fight.
fn compile_standing(
    name: &str,
    team: u32,
    side: &SidePlan,
    skill_effects: &CommanderSkillEffects,
    refused: &mut Refusals,
) -> (Vec<ShieldPlacement>, Vec<StandingOil>) {
    let named = |error: Error| Error::new(format!("side {name}: {error}"));
    let shields = side
        .standing_shields
        .iter()
        .filter_map(|&position| {
            refused.hold(skill_effects.standing_shield(team, position).map_err(named))
        })
        .collect();
    let oil = side
        .standing_oil
        .iter()
        .filter_map(|area| refused.hold(skill_effects.standing_oil(team, area).map_err(named)))
        .collect();
    (shields, oil)
}

/// A side's released battle skills, or nothing for each one refused with its
/// refusal kept.
#[allow(clippy::too_many_arguments)]
fn compile_battle_skills(
    name: &str,
    team: u32,
    side: &SidePlan,
    skill_effects: &CommanderSkillEffects,
    units: &UnitConfigs,
    loadouts: &Loadouts,
    refused: &mut Refusals,
) -> Vec<SkillRelease> {
    let mut battle_skills = Vec::new();
    for skill in &side.battle_skills {
        let Some(mut release) = refused.hold(
            skill_effects
                .release(team, skill, units)
                .map_err(|error| Error::new(format!("side {name}: {error}"))),
        ) else {
            continue;
        };
        if let SkillEffect::Summon(summon) = &mut release.effect {
            // `SummonSystem` makes a summon at level 1, with no equipment.
            let Some(worn) = loadout(
                name,
                &summon.rules.type_name,
                1,
                &[],
                &summon.rules,
                side,
                loadouts,
                refused,
            ) else {
                continue;
            };
            if worn.lifesteal.is_some()
                || worn.auto_recovery.is_some()
                || worn.energy_shield.is_some()
            {
                refused.push(format!(
                    "side {name} summons a {} that its technologies give lifesteal, repair \
                     or a shield, and what a summon's effect providers carry is not measured",
                    summon.rules.type_name
                ));
                continue;
            }
            summon.corrections = worn.corrections;
            summon.technology_disable = worn.technology_disable;
        }
        battle_skills.push(release);
    }
    battle_skills
}

fn compile_constructions(
    name: &str,
    team: u32,
    side: &SidePlan,
    table: &Constructions,
    refused: &mut Refusals,
) -> Vec<ConstructionBuilding> {
    let mut built = Vec::new();
    for (group, placement) in side.constructions.iter().enumerate() {
        let Some(mut buildings) = refused.hold(
            table
                .buildings(team, placement)
                .map_err(|error| Error::new(format!("side {name}: {error}"))),
        ) else {
            continue;
        };
        for building in &mut buildings {
            building.group = group;
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
    // What is left to refuse here is a placement that is not a unit at all.
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
    let worn = loadout(
        side_name,
        &formation.type_name,
        level,
        &formation.equipment,
        rules,
        side,
        loadouts,
        refused,
    );
    let production = production_of(
        side_name,
        (&formation.equipment, level),
        rules,
        units,
        side,
        loadouts,
        refused,
    );
    let formation_index = refused.hold(formation.index.map_or_else(
        || {
            i32::try_from(index)
                .map_err(|_| Error::new("formation index exceeds the native integer range"))
        },
        Ok,
    ));
    let (Some(()), Some(()), Some(worn), Some(production), Some(formation_index)) =
        (fired, fits, worn, production, formation_index)
    else {
        return None;
    };
    // `BuffCycleController.OnEnterFight` starts no controller on a unit still
    // travelling, and when one that arrives starts it is not read.
    if formation.travelling && !worn.buff_sources.is_empty() {
        refused.push(format!(
            "side {side_name} unit type {:?} travels in with a buff its equipment adds as \
             the fight starts, and when a travelling unit's starts is not measured",
            formation.type_name
        ));
        return None;
    }
    let local_x = i64::from(formation.position.x);
    let local_z = i64::from(formation.position.y);
    let (world_x, world_z) = if team == 0 {
        (local_x, local_z)
    } else {
        (-local_x, -local_z)
    };
    let rotation = formation_rotation(formation.position, team, world_x);
    Some(Placement {
        team,
        unit_id: 0,
        formation_id: 0,
        formation_index,
        type_name: formation.type_name.clone(),
        world_x,
        world_z,
        rotation,
        rotated: formation.rotated,
        level,
        exp: i64::from(formation.exp.unwrap_or(0)),
        experience_rate: worn.experience_rates.0,
        unit_experience_rate: worn.experience_rates.1,
        corrections: worn.corrections,
        lifesteal: worn.lifesteal,
        auto_recovery: worn.auto_recovery,
        energy_shield: worn.energy_shield,
        sweep: worn.sweep,
        distance_intensify: worn.distance_intensify,
        secondary_damage: worn.secondary_damage,
        interception: worn.interception,
        carried_shield: worn.carried_shield,
        production,
        buff_sources: worn.buff_sources,
        ignored_buffs: worn.ignored_buffs,
        important: worn.important,
        ignores_control_beam: worn.ignores_control_beam,
        travelling: formation.travelling,
        extra_weapons: worn.extra_weapons,
        technology_disable: worn.technology_disable,
    })
}

/// A formation faces the enemy from its side's half, and the middle from a
/// flank: the world's left flank faces +x and its right flank -x, whichever
/// side stands there.
fn formation_rotation(position: mechcore_document::Position, team: u32, world_x: i64) -> i64 {
    match mechcore_document::Region::of(position) {
        mechcore_document::Region::Main if team == 0 => 0,
        mechcore_document::Region::Main => 180_000,
        _ if world_x < 0 => 90_000,
        _ => 270_000,
    }
}

/// The production line a formation's equipment runs, resolved, or `None`
/// with its refusals kept: `Some(None)` for a formation that runs none.
#[allow(clippy::option_option, reason = "a refusal is kept apart from no line")]
fn production_of(
    side_name: &str,
    (equipment, level): (&[i32], i64),
    rules: &UnitConfig,
    units: &UnitConfigs,
    side: &SidePlan,
    loadouts: &Loadouts,
    refused: &mut Refusals,
) -> Option<Option<Production>> {
    let mut lines = Vec::new();
    for &id in equipment {
        lines.extend(
            refused.hold(
                loadouts
                    .equipment
                    .production(id, rules)
                    .map_err(|error| Error::new(format!("side {side_name}: {error}"))),
            )?,
        );
    }
    // A researched technology's line runs beside them, as its
    // `SupportUnitTech` or its support skill hands it.
    lines.extend(
        refused.hold(
            loadouts
                .technologies
                .production(&side.techs.units, &rules.type_name)
                .map_err(|error| Error::new(format!("side {side_name}: {error}"))),
        )?,
    );
    lines.extend(
        rules
            .extra_weapons
            .iter()
            .filter(|weapon| side.techs.units.contains(&weapon.technology))
            .filter_map(|weapon| weapon.production.as_ref())
            .map(technology_line),
    );
    let line = match lines.as_slice() {
        [] => return Some(None),
        [line] => line.clone(),
        _ => {
            refused.push(format!(
                "side {side_name} unit type {:?} runs two production lines, which is not \
                 measured",
                rules.type_name
            ));
            return None;
        }
    };
    let Some(made) = units.by_type_id(line.unit_type_id) else {
        refused.push(format!(
            "side {side_name}: a production line makes unit {}, which has no unit configuration",
            line.unit_type_id
        ));
        return None;
    };
    let made = made.clone();
    // A make takes its owner's level, or the first.
    let worn = loadout(
        side_name,
        &made.type_name,
        if line.parent_level { level } else { 1 },
        &[],
        &made,
        side,
        loadouts,
        refused,
    )?;
    if worn.lifesteal.is_some()
        || worn.auto_recovery.is_some()
        || worn.energy_shield.is_some()
        || worn.distance_intensify
        || worn.secondary_damage.is_some()
        || worn.interception.is_some()
    {
        refused.push(format!(
            "side {side_name} makes a {} that its technologies give lifesteal, repair, a \
             shield, a search by distance, a second damage or interceptors, and what a made \
             unit's effect providers carry is not measured",
            made.type_name
        ));
        return None;
    }
    Some(Some(Production {
        line,
        rules: made,
        corrections: worn.corrections,
        technology_disable: worn.technology_disable,
    }))
}

/// A technology's support skill as the production line it runs.
fn technology_line(production: &crate::rules::TechnologyProduction) -> ProductionLine {
    let metres = crate::rules::metres_q32;
    ProductionLine {
        unit_type_id: production.unit_type_id,
        max_batch: production.max_batch,
        max_alive: production.max_alive,
        per_time: production.per_time,
        interval_q32: metres(production.interval),
        offsets: production
            .offsets
            .iter()
            .map(|offset| (metres(offset.x), metres(offset.y)))
            .collect(),
        appear_q32: metres(production.appear),
        parent_level: production.level == crate::rules::ProductionLevel::Parent,
        body_frame: production.frame == crate::rules::ProductionFrame::ParentBody,
        arrival: crate::modifier::Arrival::InPlace,
        gated: true,
    }
}

/// What a buff that disables technology takes from a unit while it runs
/// (`CBEC_DisableTechnology`, `FightEffectSystem.DisableEffect`): of its
/// sources, only its technologies, whose `Technology.CanDisable` answers
/// `ignoreElectricEffect` false, and none of its officers', equipment's or
/// Energy Tower skills', which answer false.
#[derive(Debug, Clone, Default)]
pub(crate) struct TechnologyDisable {
    /// What its technologies wrote onto its numbers, which their providers
    /// take away and write again (`IEffectProviderDataSource.RemoveData`,
    /// `AddData`).
    pub(crate) corrections: Vec<(Channel, Entry)>,
    /// What it carries whose switching off is not measured, by name: a
    /// technology whose provider does more than take its numbers away.
    pub(crate) unmeasured: Vec<String>,
}

/// The interceptors a unit's technologies make it, from the one that makes
/// any: a second is refused, which is not measured.
fn one_interception(
    held: &[UnitInterception],
    side_name: &str,
    type_name: &str,
) -> Result<Option<UnitInterception>> {
    match held {
        [] => Ok(None),
        [one] => Ok(Some(*one)),
        _ => Err(Error::new(format!(
            "side {side_name} unit type {type_name:?} holds two interception technologies, \
             which is not measured"
        ))),
    }
}

/// What this side's loadout and a formation's equipment hand one unit.
struct Worn {
    corrections: Vec<(Channel, Entry)>,
    /// The card's rate and the unit's own.
    experience_rates: (ExperienceRate, ExperienceRate),
    lifesteal: Option<LifeSteal>,
    auto_recovery: Option<AutoRecovery>,
    energy_shield: Option<EnergyShield>,
    sweep: Option<SweepIntensify>,
    distance_intensify: bool,
    secondary_damage: Option<SecondaryDamage>,
    interception: Option<UnitInterception>,
    carried_shield: Option<CarriedShield>,
    buff_sources: Vec<BuffSource>,
    ignored_buffs: Vec<u32>,
    important: bool,
    ignores_control_beam: bool,
    extra_weapons: Vec<ExtraWeapon>,
    technology_disable: TechnologyDisable,
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
) -> Option<Worn> {
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
            let held = std::slice::from_ref(id);
            let technologies = &loadouts.technologies;
            technologies
                .corrections(held, type_name, level)
                .and_then(|mut written| {
                    written.extend(technologies.armor(held, type_name, level)?);
                    Ok(written)
                })
                .map_err(on_side)
        }))
        .chain(
            equipment
                .iter()
                .map(|&id| loadouts.equipment.corrections(id, rules).map_err(on_side)),
        )
        .chain(side.energy_tower_skills.iter().map(|id| {
            loadouts
                .energy_tower
                .corrections(std::slice::from_ref(id), rules)
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
    let experience_rate = refused.hold(
        loadouts
            .officers
            .experience_rate(&side.techs.officers, rules)
            .map_err(on_side),
    )?;
    let unit_experience_rate = refused.hold(
        loadouts
            .technologies
            .experience_rate(&side.techs.units, type_name, level)
            .map_err(on_side),
    )?;
    let mut worn = worn(
        side_name,
        type_name,
        level,
        equipment,
        rules,
        side,
        loadouts,
        corrections,
        refused,
    )?;
    worn.technology_disable = technology_disable(
        type_name,
        level,
        side,
        loadouts,
        &worn.extra_weapons,
        &on_side,
        refused,
    )?;
    let stats = refused.hold(Stats::corrected(rules, level, &worn.corrections).map_err(refusal))?;
    // A correction the build has no field for is refused here, where the
    // side and the officer can be named.
    refused.hold(stats.refuse_fieldless_corrections().map_err(refusal))?;
    worn.experience_rates = (experience_rate, unit_experience_rate);
    Some(worn)
}

/// What a buff that disables technology takes from one unit: what its
/// technologies wrote, which resolved with the rest of its loadout, and what
/// of them a disabling buff does more to than take its numbers away.
fn technology_disable(
    type_name: &str,
    level: i64,
    side: &SidePlan,
    loadouts: &Loadouts,
    extra_weapons: &[ExtraWeapon],
    on_side: &dyn Fn(Error) -> Error,
    refused: &mut Refusals,
) -> Option<TechnologyDisable> {
    let technologies = &loadouts.technologies;
    let held = &side.techs.units;
    Some(TechnologyDisable {
        corrections: refused.hold(
            technologies
                .corrections(held, type_name, level)
                .and_then(|mut written| {
                    written.extend(technologies.armor(held, type_name, level)?);
                    Ok(written)
                })
                .map_err(on_side),
        )?,
        unmeasured: technologies
            .disabled_unmeasured(held, type_name)
            .into_iter()
            .map(|id| format!("technology {id}"))
            // An extra skill switched off neither searches nor starts from
            // idle and ends its attack between blows. A permanent preemptive
            // explosion, Scorching Charge's, does not activate and its death
            // does not explode while its unit's technologies are off; a
            // production line's, any other explosion's or preemptive skill's
            // and a group's own paths are not measured switched off.
            .chain(
                extra_weapons
                    .iter()
                    .filter(|weapon| {
                        let rules = &weapon.rules;
                        let preemptive_explosion =
                            rules.explosion.is_some() && rules.preemptive.is_some();
                        let read_path = matches!(
                            rules.attack.path,
                            crate::rules::AttackPath::Direct
                                | crate::rules::AttackPath::Projectile { .. }
                                | crate::rules::AttackPath::Laser { .. }
                        );
                        rules.production.is_some()
                            || rules.attack.weapons.makes_group()
                            || weapon.joins_main_group
                            || !preemptive_explosion
                                && (rules.explosion.is_some()
                                    || rules.preemptive.is_some()
                                    || !read_path)
                    })
                    .map(|weapon| format!("technology {}", weapon.rules.technology)),
            )
            .collect(),
    })
}

/// What a unit's technologies and equipment hand it beyond its numbers, the
/// sources of each interface its one provider of that interface enables.
#[allow(clippy::too_many_arguments)]
fn worn(
    side_name: &str,
    type_name: &str,
    level: i64,
    equipment: &[i32],
    rules: &UnitConfig,
    side: &SidePlan,
    loadouts: &Loadouts,
    corrections: Vec<(Channel, Entry)>,
    refused: &mut Refusals,
) -> Option<Worn> {
    let on_side = |error: Error| Error::new(format!("side {side_name}: {error}"));
    let refusal = |error: Error| {
        Error::new(format!(
            "side {side_name} unit type {type_name:?} carries a loadout this \
             build cannot resolve: {error}"
        ))
    };
    // Every source of an interface reaches the unit's one provider of it,
    // which enables the one of the highest priority whatever order they came
    // in.
    let sources = refused.hold(
        loadouts
            .technologies
            .sources(&side.techs.units, type_name)
            .map_err(on_side),
    )?;
    let mut lifesteal = sources.lifesteal;
    let mut auto_recovery = sources.auto_recovery;
    let mut energy_shield = sources.energy_shield;
    let main_skill = refused.hold(
        loadouts
            .technologies
            .main_skill(&side.techs.units, type_name)
            .map_err(on_side),
    )?;
    let mut carried_shields = Vec::new();
    // The buffs its technologies add as the fight starts, then its items'.
    let mut buff_sources = sources.buff_sources;
    let mut ignored_buffs = Vec::new();
    let mut important = false;
    let mut ignores_control_beam = false;
    for &id in equipment {
        important |= refused.hold(loadouts.equipment.important(id, rules).map_err(on_side))?;
        ignores_control_beam |= refused.hold(
            loadouts
                .equipment
                .ignores_control_beam(id, rules)
                .map_err(on_side),
        )?;
        ignored_buffs
            .extend(refused.hold(loadouts.equipment.ignored_buffs(id, rules).map_err(on_side))?);
        buff_sources
            .extend(refused.hold(loadouts.equipment.buff_source(id, rules).map_err(on_side))?);
        energy_shield
            .extend(refused.hold(loadouts.equipment.energy_shield(id, rules).map_err(on_side))?);
        carried_shields.extend(
            refused.hold(
                loadouts
                    .equipment
                    .carried_shield(id, rules)
                    .map_err(on_side),
            )?,
        );
        lifesteal.extend(refused.hold(loadouts.equipment.lifesteal(id, rules).map_err(on_side))?);
        auto_recovery
            .extend(refused.hold(loadouts.equipment.auto_recovery(id, rules).map_err(on_side))?);
    }
    let mut extra_weapons = extra_weapons(
        side_name,
        (type_name, level),
        equipment,
        rules,
        side,
        loadouts,
        refused,
    )?;
    let mut corrections = corrections;
    for weapon in &extra_weapons {
        corrections.extend(extra_weapon_corrections(&weapon.rules));
    }
    switch_air_attack(&main_skill, rules, &mut corrections, &mut extra_weapons);
    let in_force = |error: String| refusal(Error::new(error));
    Some(Worn {
        corrections,
        experience_rates: Default::default(),
        lifesteal: refused.hold(current_source(&lifesteal).map_err(in_force))?,
        auto_recovery: refused.hold(current_source(&auto_recovery).map_err(in_force))?,
        energy_shield: refused.hold(current_source(&energy_shield).map_err(in_force))?,
        sweep: main_skill.sweep,
        distance_intensify: main_skill.distance_intensify,
        secondary_damage: main_skill.secondary_damage,
        interception: refused.hold(one_interception(
            &sources.interception,
            side_name,
            type_name,
        ))?,
        carried_shield: match carried_shields.as_slice() {
            [] => None,
            [one] => Some(*one),
            _ => {
                refused.push(format!(
                    "side {side_name} unit type {type_name:?} carries two barriers, which is \
                     not measured"
                ));
                return None;
            }
        },
        buff_sources,
        ignored_buffs,
        important,
        ignores_control_beam,
        extra_weapons,
        technology_disable: TechnologyDisable::default(),
    })
}

/// Whether a grouped row's skills are the main skill's slots in all but
/// their range, their attack angle and their weapons: the main skill's base
/// damage at a rate of one, nothing left behind or written, and no weapon
/// turning within an arc.
fn joins_as_main_slots(weapon: &ExtraWeaponConfig, rules: &UnitConfig) -> bool {
    let mut attack = weapon.attack.clone();
    attack.base_damage = rules.attack.base_damage;
    attack.range = rules.attack.range;
    attack.attack_half_angle = rules.attack.attack_half_angle;
    attack
        .weapons
        .indices
        .clone_from(&rules.attack.weapons.indices);
    attack == rules.attack
        && rules.attack.weapons.arcs.is_none()
        && (weapon.damage_rate - 1.0).abs() < f64::EPSILON
        && weapon.damage_by_level.is_empty()
        && weapon.fire.is_none()
        && weapon.oil.is_none()
        && weapon.fog.is_none()
        && weapon.buff.is_none()
        && weapon.production.is_none()
        && weapon.preemptive.is_none()
        && weapon.explosion.is_none()
}

/// The fire an extra weapon's hit leaves: its skill's splash wide
/// (`ExtraSkillProvider.AddEffect` writes `GetSplashRange` onto the unit as
/// the fire's range) and burning its row's first `fireLifeTime`
/// (`ExtraWeaponTechnologyData.GetFireLifeTime`).
fn extra_weapon_fire(weapon: &ExtraWeaponConfig, loadouts: &Loadouts) -> Result<TerrainSpec> {
    let [range, life] = ground_fire(weapon);
    loadouts.skill_effects.unit_fire(range, life)
}

/// The terrain an extra weapon's hit leaves: the unit's fire, the
/// technology's oil, which writes the row's buff, or the technology's fog,
/// which rates the attack range of what stands in it, as wide as the skill
/// splashes and standing one round (`ExtraWeaponTech`'s `IRangeItemProvider`
/// and `IFogProvider`).
fn extra_weapon_terrain(
    weapon: &ExtraWeaponConfig,
    named: &str,
    buff: Option<SkillBuff>,
    loadouts: &Loadouts,
) -> Result<Option<TerrainSpec>> {
    match (&weapon.fire, &weapon.oil, buff) {
        (None, None, _) => Ok(weapon.fog.as_ref().map(|fog| TerrainSpec {
            kind: TerrainKind::Fog,
            radius_q32: crate::rules::metres_q32(weapon.attack.splash_radius),
            life_ticks: None,
            rounds: 1,
            effect: TerrainEffect::Fog {
                attack_range_rate: fog.attack_range_rate_q32(),
            },
            burns: None,
        })),
        (Some(_), None, _) if weapon.fog.is_none() => extra_weapon_fire(weapon, loadouts).map(Some),
        (None, Some(_), Some(buff)) if weapon.fog.is_none() => {
            let [range, life] = ground_fire(weapon);
            loadouts
                .skill_effects
                .unit_oil(named, range, buff, life)
                .map(Some)
        }
        _ => Err(Error::new(
            "a hit that leaves an oil with no buff, or two terrains, is not read",
        )),
    }
}

/// The fire's range and life time, Q32.32 metres and seconds, as
/// `ExtraSkillProvider.AddEffect` writes them onto the unit: the skill's
/// splash, and the row's first `fireLifeTime`, a fire's or an oil's.
fn ground_fire(weapon: &ExtraWeaponConfig) -> [i64; 2] {
    let life = weapon
        .fire
        .as_ref()
        .map(|fire| &fire.life_time)
        .or_else(|| weapon.oil.as_ref().map(|oil| &oil.fire_life_time))
        .and_then(|life| life.first().copied())
        .unwrap_or(0.0);
    [
        crate::rules::metres_q32(weapon.attack.splash_radius),
        crate::rules::metres_q32(life),
    ]
}

/// What an extra weapon writes onto its unit's `DataSet`: where its row burns
/// (`GetFireLifeTime` above zero), `ExtraSkillProvider.AddEffect` adds the
/// fire's range and life time through `MechDataModifer.AddData`.
fn extra_weapon_corrections(weapon: &ExtraWeaponConfig) -> Vec<(Channel, Entry)> {
    let [range, life] = ground_fire(weapon);
    if life <= 0 {
        return Vec::new();
    }
    [
        (Index::GroundFireRange, range),
        (Index::GroundFireLifeTime, life),
    ]
    .into_iter()
    .map(|(index, value)| {
        (
            Channel::Unit,
            Entry {
                index,
                source: "extra weapon",
                correction: Correction::Value(value),
            },
        )
    })
    .collect()
}

/// The extra weapons a unit's technologies add beside its main skill
/// (`ExtraWeaponTech`), or `None` with a refusal kept.
///
/// What a source writes onto a skill reaches an extra skill only where
/// `SkillDataModifier.AvaliableCheck` lets it ([`reaching_extra_skills`]):
/// an officer and a technology of the units these weapons serve only a skill
/// with a damage rate, and an equipment through its `extraSkillEffect` and
/// an Energy Tower skill always. How such a correction composes on an extra
/// skill is not measured, so a unit it reaches is refused.
fn extra_weapons(
    side_name: &str,
    (type_name, level): (&str, i64),
    equipment: &[i32],
    rules: &UnitConfig,
    side: &SidePlan,
    loadouts: &Loadouts,
    refused: &mut Refusals,
) -> Option<Vec<ExtraWeapon>> {
    let mut weapons = Vec::new();
    for weapon in rules
        .extra_weapons
        .iter()
        .filter(|weapon| side.techs.units.contains(&weapon.technology))
    {
        let named = format!("technology {}", weapon.technology);
        let on_unit = |error: Error| {
            Error::new(format!(
                "side {side_name} unit type {type_name:?} technology {}: {error}",
                weapon.technology
            ))
        };
        let buff = match &weapon.buff {
            None => None,
            Some(buff) => Some(refused.hold(technology_buff(&named, buff).map_err(on_unit))?),
        };
        let terrain =
            refused.hold(extra_weapon_terrain(weapon, &named, buff, loadouts).map_err(on_unit))?;
        let dead_fire = match weapon
            .explosion
            .as_ref()
            .and_then(|explosion| explosion.dead_fire)
        {
            None => None,
            Some(fire) => Some(
                refused.hold(
                    loadouts
                        .skill_effects
                        .unit_fire(
                            crate::rules::metres_q32(fire.radius),
                            crate::rules::metres_q32(fire.life_time),
                        )
                        .map_err(on_unit),
                )?,
            ),
        };
        let (skill_corrections, reaching) =
            reaching_extra_skill(weapon, (type_name, level), equipment, rules, side, loadouts);
        if !reaching.is_empty() {
            refused.push(format!(
                "side {side_name} unit type {type_name:?} carries an extra weapon that {} \
                 reaches with a correction other than damage, and how one composes on an \
                 extra skill is not measured",
                reaching.join(" and ")
            ));
            return None;
        }
        // A grouped row on a unit whose main skill holds a group joins that
        // group (`FightSkillFactory.PrepareGroupedSkill`): its skills are the
        // group's next slots, which run on the main skill's numbers here, so
        // a row joins only where its numbers are those but for its range, its
        // attack angle and its weapons.
        let joins_main_group = weapon.attack.weapons.mode == crate::rules::WeaponMode::Group
            && rules.attack.weapons.mode == crate::rules::WeaponMode::Group;
        if joins_main_group && !joins_as_main_slots(weapon, rules) {
            refused.push(format!(
                "side {side_name} unit type {type_name:?} technology {}: its skills join the \
                 main skill's group with numbers of their own beyond their range, attack \
                 angle and weapons, which is not measured",
                weapon.technology
            ));
            return None;
        }
        weapons.push(ExtraWeapon {
            rules: weapon.clone(),
            terrain,
            buff,
            dead_fire,
            joins_main_group,
            skill_corrections,
        });
    }
    Some(weapons)
}

/// `AirAttackEffectProvider.SwitchMechAirAttackEnabled`: an air-attack
/// technology adds -1 to the main skill's `AirAttackValue` if its row attacks
/// aircraft and 1 otherwise, so a Fang with Grenade Launcher attacks none and
/// an Arclight with Anti-Aircraft Ammunition attacks them. Where the row's
/// `extraSkillEffect` says so, each extra skill holding its own `DataSet`
/// gains the same, as a Tarantula's Spider Mine does; one with a damage rate
/// holds the main skill's.
fn switch_air_attack(
    main_skill: &MainSkill,
    rules: &UnitConfig,
    corrections: &mut Vec<(Channel, Entry)>,
    extra_weapons: &mut [ExtraWeapon],
) {
    let Some(air_attack) = main_skill.air_attack else {
        return;
    };
    let entry = Entry {
        index: Index::AttackValueFor(UnitDomain::Air),
        source: TECHNOLOGY_SOURCE,
        correction: Correction::Value(if rules.attack.targets.air { -1 } else { 1 }),
    };
    corrections.push((Channel::Skill, entry));
    if air_attack.extra_skills {
        for weapon in extra_weapons
            .iter_mut()
            .filter(|weapon| weapon.rules.damage_rate <= 0.0)
        {
            weapon.skill_corrections.push(entry);
        }
    }
}

/// What writes onto one extra skill's numbers (`SkillDataModifier.AvaliableCheck`):
/// an equipment through its `extraSkillEffect`, unless the skill ignores
/// equipment, and an Energy Tower skill always, and, onto a skill with a damage rate, what reaches the main skill
/// too (`IsMainSkillEffect`), which every officer, technology of a unit and
/// equipment answers: the skill corrections that reach a skill without a
/// damage rate, and the sources that write it a number other than damage and
/// range, which no skill here reads. A skill whose range is the main skill's
/// with its own added (`useMainSkillRange`, or a grouped row's `ParentSkill`)
/// never reads its own range property, and a melee skill's reads no
/// correction, so a range reaching either changes nothing; one of its own
/// range without a damage rate composes it, and one with a rate is refused.
fn reaching_extra_skill(
    weapon: &ExtraWeaponConfig,
    (type_name, level): (&str, i64),
    equipment: &[i32],
    rules: &UnitConfig,
    side: &SidePlan,
    loadouts: &Loadouts,
) -> (Vec<Entry>, Vec<String>) {
    let rated = weapon.damage_rate > 0.0;
    let parent_range = weapon.use_main_skill_range || weapon.attack.weapons.makes_group();
    // A range reaches a skill of its own range without a damage rate, which
    // holds what reaches it alone; a melee skill's range reads no correction.
    let range_read = parent_range || weapon.attack.melee || !rated;
    let read = |index: Index| {
        matches!(index, Index::AttackDamage | Index::DamageReduceRateBase)
            || (index == Index::AttackRange && range_read)
    };
    let mut sources = Vec::new();
    for &id in equipment {
        if !weapon.ignore_equipment && (rated || loadouts.equipment.reaches_extra_skills(id)) {
            sources.push((
                format!("equipment {id}"),
                loadouts.equipment.corrections(id, rules),
            ));
        }
    }
    for &id in &side.energy_tower_skills {
        sources.push((
            format!("energy tower skill {id}"),
            loadouts
                .energy_tower
                .corrections(std::slice::from_ref(&id), rules),
        ));
    }
    if rated {
        for &id in &side.techs.officers {
            sources.push((
                format!("officer {id}"),
                loadouts
                    .officers
                    .corrections(std::slice::from_ref(&id), rules),
            ));
        }
        for &id in &side.techs.units {
            sources.push((
                format!("technology {id}"),
                loadouts
                    .technologies
                    .corrections(std::slice::from_ref(&id), type_name, level),
            ));
        }
    }
    let mut corrections = Vec::new();
    let mut reaching = Vec::new();
    for (named, written) in sources {
        // A source this build cannot apply is refused where it is asked for.
        let Ok(written) = written else {
            continue;
        };
        let on_skill = written
            .into_iter()
            .filter(|(channel, _)| *channel == Channel::Skill)
            .map(|(_, entry)| entry)
            .collect::<Vec<_>>();
        if on_skill.iter().any(|entry| !read(entry.index)) {
            reaching.push(named);
        } else if !rated {
            corrections.extend(on_skill);
        }
    }
    (corrections, reaching)
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
    fn formation_index_is_the_stated_unit_index_across_a_gap() {
        let value = LAYOUT.replace(
            "units: [{name: marksman, index: 0, position: {x: 0, y: -50}}]",
            "units:\n      - {name: arclight, index: 0, position: {x: 20, y: -100}}\n      - {name: rhino, index: 5, position: {x: -15, y: -105}}",
        );
        let layout = compile_default(&value).unwrap();
        assert_eq!(layout.placements[1].type_name, "rhino");
        assert_eq!(layout.placements[1].formation_index, 5);
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
    /// The game agrees to the tick: the same officer on the same Marksman,
    /// shooting a Rhino, is `tests/modifier/fights/officer-composition-once.yaml`,
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

    /// A turret beside an officer and a technology is placed as it is alone:
    /// neither reaches a construction, so its skill keeps the row's numbers.
    #[test]
    fn a_turret_beside_an_officer_is_placed() {
        let turret = LAYOUT.replace(
            "units: [{name: marksman, index: 0, position: {x: 0, y: -50}}]",
            "units: [{name: marksman, index: 0, position: {x: 0, y: -50}}]\n  constructions: [{name: rapid_fire_turret, index: 1, position: {x: 140, y: -100}}]",
        );
        let alone = compile_default(&turret).unwrap();
        assert!(alone.constructions[0].skill.is_some(), "the turret fires");
        let with_officer = turret.replace(
            "blue:\n",
            "blue:\n  officers: [advanced_offensive_tactics]\n  techs:\n    marksman: [range_enhancement]\n",
        );
        assert_ne!(with_officer, turret, "the fixture carries an officer");
        let beside = compile_default(&with_officer).unwrap();
        assert_eq!(
            format!("{:?}", beside.constructions),
            format!("{:?}", alone.constructions)
        );
    }

    #[test]
    fn restores_standing_oil_with_the_cells_a_point_holds() {
        let standing = |grid: &str| {
            LAYOUT.replace(
                "units: [{name: marksman, index: 0, position: {x: 0, y: -50}}]",
                &format!(
                    "units: [{{name: marksman, index: 0, position: {{x: 0, y: -50}}}}]\n  \
                     battle_skills: [{{name: sticky_oil_bomb, standing: {{control_points: \
                     [{{x: -60, y: 40}}, {{x: 60, y: 40}}]{grid}}}}}]"
                ),
            )
        };
        let layout = compile_default(&standing(", grid_rows: {0: [], 6: []}")).unwrap();
        assert_eq!(layout.standing_oil.len(), 1);
        assert_eq!(layout.standing_oil[0].points, [(0, None), (6, None)]);
        let mut rows = vec![4095; 12];
        rows[0] = 1;
        let layout = compile_default(&standing(&format!(
            ", grid_rows: {{0: [{}]}}",
            rows.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )))
        .unwrap();
        assert_eq!(layout.standing_oil[0].points, [(0, Some(rows))]);
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
