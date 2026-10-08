//! What a technology writes onto the unit that researched it.
//!
//! `config/technology_effects.yaml` is the build's own table, extracted by
//! `scripts/extract/extract-technology-effects.py`, and
//! `docs/rules/technology_effects.md` states what each field means. The fields
//! are [`super::effects`]'s, the same ones an officer writes, because
//! `TechnologyData` and `OfficerData` answer the same interface.
//!
//! **A plain technology is applied, and a subclass whose mechanism is here.**
//! Every technology a unit may research has a row, whose `kind` is the list of
//! `TechnologyGroupData` it comes from. One of `technologyDatas` does nothing
//! but correct its unit's numbers, and is applied unless its row names a field
//! in `special`. One of any other list is a subclass that does more (a buff, a
//! splash, a second weapon, a summon). A `LifestealTech` is applied for its
//! numbers and hands its unit a [`LifeSteal`], an `AutoRecoveryTech` that
//! repairs in any state an [`AutoRecovery`], an `ArmorStrengthenTech` a
//! reduction of every hit on it, and a `SearchTargetSpecificTech` its numbers
//! against aerial and ground targets and a search by distance, a
//! `DamageIntensifyTech` its damage against them, and an
//! `AirAttackTech` its skills turned onto or off aircraft, and a
//! `SecondaryDamageIntensifyTech` a second damage around its hits, and a
//! `BuffTech` the buff it adds its unit as the fight starts; any other is
//! refused by name rather than applied for its numbers alone.
//!
//! A technology belongs to one unit type, which is how a side's flat list of
//! technologies reaches the units it corrects: a technology the side holds
//! writes onto the units its table row names and onto nothing else.
//!
//! **An effect is a list indexed by the unit's level.** `TechnologyData`'s
//! getters read entry `GetLevel()` of a list, `CardLevel` counting from zero,
//! and its last entry for a level beyond it (`GetLevelValue`), as an armour
//! technology's reduction is read. A lifesteal or repair source still reads
//! its first entry, and a row whose list for one grows is refused.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::{
    Error, Result,
    data::{Channel, Correction, Entry, Index},
    layout::{InterceptNumbers, Interception},
    rules::UnitDomain,
};

use super::{
    buffs::{self, BuffBlock, CycleBlock},
    effects::{self, Fields, VALUE_ELSEWHERE},
    sources::{AutoRecovery, BuffSource, EnergyShield, LifeSteal, SweepIntensify},
};

const DEFAULT_TECHNOLOGY_EFFECTS: &str = include_str!("../../../../config/technology_effects.yaml");

/// The list of `TechnologyGroupData` a plain technology comes from.
const PLAIN: &str = "technologyDatas";

/// The list whose `LifestealTech` is an `ILifeSteal` as well.
const LIFESTEAL: &str = "lifestealTechnologies";

/// The list whose `AutoRecoveryTech` is an `IAutoRecovery` as well.
const AUTO_RECOVERY: &str = "autoRecoveryTechnologies";

/// The list whose `EnergyShieldTech` is an `IEnergyShieldSource` as well.
const ENERGY_SHIELD: &str = "energyShieldTechnologies";

/// The list a sweep technology comes from.
const SWEEP: &str = "sweepSkillIntensifyTechDatas";

/// The list whose `ArmorStrengthenTech` is an `IArmorStrengthen` as well.
const ARMOR: &str = "armorStrengthenTechnologyDatas";

/// The list whose `SearchTargetSpecificTech` is an `ISearchTargetSpecific`.
const SEARCH_TARGET_SPECIFIC: &str = "searchTargetSpecificDatas";

/// The list whose `InterceptMissileTech` is an `IInterceptData`.
const INTERCEPT: &str = "interceptMissileTechnologyDatas";

/// The lists whose rows this build applies, each with its mechanism.
const IMPLEMENTED: [&str; 12] = [
    PLAIN,
    LIFESTEAL,
    AUTO_RECOVERY,
    ENERGY_SHIELD,
    SWEEP,
    ARMOR,
    SEARCH_TARGET_SPECIFIC,
    AIR_ATTACK,
    DAMAGE_INTENSIFY,
    SECONDARY_DAMAGE,
    BUFF,
    INTERCEPT,
];

/// The list whose `DamageIntensifyTech` writes its damage against one domain.
const DAMAGE_INTENSIFY: &str = "damageIntensifyTechnologies";

/// The list whose `SecondaryDamageIntensifyTech` is an
/// `ISecondaryDamageIntensifyEffectDataSource`.
const SECONDARY_DAMAGE: &str = "secondaryDamageIntensifyTechDatas";

/// The list whose `BuffTech` is an `IEffectBuffDataSource`.
const BUFF: &str = "buffTechnologies";

/// The list whose `AirAttackTech` is an `IAirAttackDataSource`.
const AIR_ATTACK: &str = "airAttackTechnologyDatas";

/// The list whose `ExtraWeaponTech` adds a skill beside its unit's main one.
const EXTRA_WEAPON: &str = "extraWeaponTechnologies";

/// The extra weapon technologies the fight runs. Each member's skill and
/// what it does beyond a projectile differ, so each joins once a recording
/// of it agrees: Secondary Armament and Anti-Air Missile, the Sabertooth's,
/// Incendiary Bomb, the Hound's and the Vulcan's, Scorching Charge, Homing
/// Missile, Sticky Oil Bomb, the Phantom Ray's and the Vulcan's, Whirlwind,
/// the Rhino's, Energy Diffraction, the Melting Point's, Spider Mine, the
/// Tarantula's, Matrix Bombardment, the Wraith's, Anti-Air Barrage, the
/// Fortress's, Air Defense Mark, the Typhoon's, Disintegration, the Abyss's,
/// Naval Gun, the Overlord's, Gun-launched Missile, the Mountain's,
/// Electromagnetic Barrage, the Melting Point's, Dual Wield, the
/// Centurion's, Fork, the Raiden's, Smoke Bomb, the Mountain's, Swarm
/// Missiles, the Abyss's, Rocket Punch, the Fortress's, and Multi Control, the
/// Hacker's.
pub(crate) const FOUGHT_EXTRA_WEAPONS: [i32; 24] = [
    1_103, 1_105, 1_106, 1_107, 1_108, 1_109, 11_010, 11_014, 11_020, 11_024, 11_025, 11_028,
    11_029, 110_181, 110_201, 110_211, 110_212, 110_271, 110_291, 110_321, 110_322, 1_102_022,
    11_020_021, 11_020_022,
];

/// `EnergyShieldTech.GetLifeRate`: `FPoint.One`, whatever its row, so the
/// shield holds the unit's whole maximum life.
const SHIELD_LIFE_RATE: i64 = 1 << 32;

/// `AutoRecoveryStateType.Normal`: a repair that runs whenever its unit is
/// hurt, not only underground or cloaked.
const NORMAL: i64 = 0;

/// `Technology`'s `IEffectProviderDataSource.GetPriority`, which an
/// equipment's 1 overrides.
const PRIORITY: i32 = 0;

/// The module that tags every entry a technology writes.
pub(crate) const SOURCE: &str = "Modifier";

/// What an armour technology writes is its effect provider's,
/// `ArmorStrengthenEffectProvider`, which `FightEffectSystem.ActiveEffect`
/// enables: a unit that travels in holds it once it arrives.
pub(crate) const ARMOR_SOURCE: &str = "ArmorStrengthenEffectProvider";

/// Every technology's combat effect, by the id a layout compiles to.
#[derive(Debug, Clone)]
pub(crate) struct TechnologyEffects {
    technologies: BTreeMap<i32, Technology>,
}

/// What a technology writes at one level.
type Written = Vec<(Channel, Index, Correction)>;

#[derive(Debug, Clone)]
struct Technology {
    /// The unit type whose numbers it corrects.
    unit: String,
    /// What it writes at each entry of its lists, the first level's first,
    /// or why this build will not apply it.
    effect: std::result::Result<Vec<Written>, String>,
    /// What it answers `ILifeSteal` with, if its class is one.
    lifesteal: Option<LifeSteal>,
    /// What it answers `IAutoRecovery` with, if its class is one.
    auto_recovery: Option<AutoRecovery>,
    /// What it answers `IEnergyShieldSource` with, if its class is one.
    energy_shield: Option<EnergyShield>,
    /// What it hands its unit's sweep, if its class is a sweep's.
    sweep: Option<SweepIntensify>,
    /// What it answers `IArmorStrengthen.GetReduceDamageValue` with, by its
    /// unit's level, if its class is one.
    reduce_damage: Option<Vec<i64>>,
    /// Whether it turns its unit's main skill's search to
    /// `DistanceIntensify`: an `ISearchTargetSpecific`, whose
    /// `GetSearchTargetType` answers that whatever its row.
    distance_intensify: bool,
    /// Whether it turns its unit's skill onto or off aircraft, and its
    /// extra skills too: an `IAirAttackDataSource`.
    air_attack: Option<AirAttack>,
    /// The second damage its unit's main skill deals around each hit, if its
    /// class is an `ISecondaryDamageIntensifyEffectDataSource`.
    secondary_damage: Option<SecondaryDamage>,
    /// The buff it adds its unit as the fight starts, if its class is an
    /// `IEffectBuffDataSource`.
    buff_source: Option<BuffSource>,
    /// What its unit intercepts with, if its class is an `IInterceptData`.
    interception: Option<UnitInterception>,
    /// Whether what switching it off does is read and fought: its numbers
    /// taken away, as [`DISABLED_AS_NUMBERS`] lists, an extra weapon's skills
    /// disabled, or the buff a fight-start buff technology adds its own unit
    /// cleared (`BuffManager.ClearSelfResourceBuffByDisableTech`). A source
    /// that keeps its buff on the units around its unit stops its cycle
    /// (`BuffEffectProvider.DoDisableCycle`), and how it starts again is not
    /// measured.
    switch_off_read: bool,
}

/// The lists whose technologies a disabling buff switches off by taking their
/// numbers away and gives back by writing them again, which the fight does:
/// `EffectProvider.DisableEffect` removes a source's data
/// (`IEffectProviderDataSource.RemoveData`) and `EnableEffect` adds it, and
/// an armour's `ArmorStrengthenEffectProvider` its reduction. A lifesteal's
/// and a second damage's providers take their hit effect away, which the
/// fight asks of the unit at each hit. A search technology's
/// `SearchTargetSpecificProvider.DoDisable` takes its ranges and score
/// offsets away and turns its unit's selector back to `Normal`, which is the
/// selector it turned to with no offsets, and `DoEnable` writes them and
/// turns it to `DistanceIntensify` again, which reads them as they now stand.
/// An extra weapon's provider takes its numbers and disables its skills,
/// which the layout refuses for the shapes it does not fight switched off.
/// Every other list's provider does more, which is not measured.
const DISABLED_AS_NUMBERS: [&str; 6] = [
    PLAIN,
    ARMOR,
    DAMAGE_INTENSIFY,
    LIFESTEAL,
    SECONDARY_DAMAGE,
    SEARCH_TARGET_SPECIFIC,
];

/// What `SecondaryDamageIntensifyEffectProvider` hands its unit's main skill
/// (`FightSkill.SetSecondaryDamageInfo`), and `DamagePerformer.PerformSecondaryEffect`
/// deals after each of the skill's hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SecondaryDamage {
    /// `SecondaryDamageInfo.Damage`: what it deals each unit it reaches.
    pub(crate) damage: i64,
    /// `SecondaryDamageInfo.SplashRange`, millimetres: how far from where the
    /// hit landed it reaches.
    pub(crate) splash_radius: i64,
    /// `SecondaryDamageInfo.CanMainTargetBeHit`: whether it strikes what the
    /// hit itself struck.
    pub(crate) hits_main_target: bool,
    /// `SecondaryDamageInfo.CanBeAffectedByBuff`: whether the attacker's
    /// tower buffs and the struck unit's damage taken scale it.
    pub(crate) buffed: bool,
}

/// What `InterceptMissileEffectProvider` makes of one technology: the
/// `InterceptCtrGroup_Mech` it adds its unit's side, `weapons` interceptors
/// that each intercept with the same numbers from where the unit stands, and
/// whether each locks the unit's main skill while it intercepts
/// (`InterceptEffect_FightMech_Preemptive`) or leaves it be
/// (`InterceptEffect_FightMech_NoPreemptive`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UnitInterception {
    pub(crate) interception: Interception,
    pub(crate) weapons: u32,
    pub(crate) preemptive: bool,
}

/// What an `AirAttackEffectProvider` does with one technology: it turns the
/// main skill onto aircraft if it attacks none and off them if it does,
/// adding 1 or -1 to its `AirAttackValue`, and the same to each extra
/// skill's where the row's `extraSkillEffect` says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AirAttack {
    pub(crate) extra_skills: bool,
}

/// The sources a side's technologies hand one unit type's effect providers.
#[derive(Debug, Clone, Default)]
pub(crate) struct UnitSources {
    pub(crate) lifesteal: Vec<LifeSteal>,
    pub(crate) auto_recovery: Vec<AutoRecovery>,
    pub(crate) energy_shield: Vec<EnergyShield>,
    pub(crate) buff_sources: Vec<BuffSource>,
    pub(crate) interception: Vec<UnitInterception>,
}

/// What a side's technologies change about one unit type's main skill
/// beyond its numbers.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct MainSkill {
    /// What the first that hands its sweep anything hands it.
    pub(crate) sweep: Option<SweepIntensify>,
    /// Whether one turns its search to `DistanceIntensify`
    /// (`SearchTargetSpecificProvider.DoEnable`,
    /// `FightSkill.ChangeSearchTargetType`).
    pub(crate) distance_intensify: bool,
    /// The first that turns it onto or off aircraft.
    pub(crate) air_attack: Option<AirAttack>,
    /// The second damage the first that hands it one hands it.
    pub(crate) secondary_damage: Option<SecondaryDamage>,
}

/// One row of the table. Every effect is a list because a technology's effect
/// can grow with the unit's level.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is a column of the build's row"
)]
struct Row {
    id: i32,
    name: String,
    unit: String,
    /// The list of `TechnologyGroupData` the row comes from.
    kind: String,
    /// The fields a plain, a lifesteal or a repair row sets beyond what this
    /// table carries.
    #[serde(default)]
    special: Vec<String>,
    #[serde(default)]
    life_rate: Vec<i64>,
    #[serde(default)]
    damage_rate: Vec<i64>,
    #[serde(default)]
    speed_value: Vec<i64>,
    #[serde(default)]
    min_attack_range_value: Vec<i64>,
    #[serde(default)]
    attack_range_value: Vec<i64>,
    #[serde(default)]
    attack_range_rate: Vec<i64>,
    #[serde(default)]
    attack_interval_value: Vec<i64>,
    #[serde(default)]
    attack_interval_rate: Vec<i64>,
    #[serde(default)]
    splash_range_value: Vec<i64>,
    #[serde(default)]
    projectile_speed_value: Vec<i64>,
    #[serde(default)]
    projectile_life_rate: Vec<i64>,
    /// `LifestealTechnologyData.lifestealMultiplier`, on a lifesteal row.
    #[serde(default)]
    lifesteal_multiplier: Vec<i64>,
    /// `AutoRecoveryTechnologyData`'s fields, on a repair row.
    #[serde(default)]
    start_time: i64,
    #[serde(default)]
    auto_recovery_state_type: i64,
    #[serde(default)]
    recovery_duration: Vec<i64>,
    #[serde(default)]
    recovery_life_rate: Vec<i64>,
    /// `ArmorStrengthenTechnologyData.reduceDamageValue`, on an armour row:
    /// one entry per unit level.
    #[serde(default)]
    reduce_damage_value: Vec<i64>,
    /// `SearchTargetSpecificData`'s fields, on a row of its list: whole
    /// metres, and a rate by the unit's level, which a
    /// `DamageIntensifyTechnologyData` row carries too.
    #[serde(default)]
    air_target_score_offset: i64,
    #[serde(default)]
    ground_target_score_offset: i64,
    #[serde(default)]
    air_damage_change_rate: Vec<i64>,
    #[serde(default)]
    ground_damage_change_rate: Vec<i64>,
    /// `TechnologyData.extraSkillEffect`, on an air-attack row.
    #[serde(default)]
    extra_skill_effect: bool,
    /// `SecondaryDamageIntensifyTechData`'s fields, on a row of its list:
    /// whole damage, `FPoint` metres, and its flags and buff.
    #[serde(default)]
    secondary_damage: i64,
    #[serde(default)]
    secondary_splash_range: i64,
    #[serde(default)]
    secondary_hits_main_target: bool,
    #[serde(default)]
    secondary_buffed: bool,
    #[serde(default)]
    secondary_disables_technology: bool,
    #[serde(default)]
    secondary_buff_id: i64,
    /// `BuffTechnologyData`'s source fields and the `buffDatas` row it adds,
    /// on a row of its list.
    #[serde(default)]
    buff_trigger: Option<i32>,
    #[serde(default)]
    buff_targets: Vec<i32>,
    #[serde(default)]
    probability: Option<i64>,
    #[serde(default)]
    buff_cycle: CycleBlock,
    #[serde(default)]
    buff_special: Vec<String>,
    #[serde(default)]
    buff: Option<BuffBlock>,
    #[serde(default)]
    buff_range_item: Option<buffs::RangeItemBlock>,
    /// `ExtraWeaponTechnologyData.allWeaponReduceDamageRate`, on an extra
    /// weapon row that sets it.
    #[serde(default)]
    all_weapon_reduce_damage_rate: i64,
    #[serde(default)]
    sweep_skill_id: i64,
    #[serde(default)]
    sweep_width_value: i32,
    #[serde(default)]
    sweep_length_value: i32,
    #[serde(default)]
    sweep_perpendicular: bool,
    #[serde(default)]
    sweep_reverse: bool,
    #[serde(default)]
    sweep_fixed_direction: bool,
    /// `InterceptMissileTechnologyData`'s fields, on a row of its list.
    #[serde(default)]
    intercept: Option<InterceptBlock>,
}

/// What an interception row answers `IInterceptData` with, named as
/// `config/contraptions.yaml`'s interceptor names them.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct InterceptBlock {
    attack: i32,
    range_max: i64,
    range_min: i64,
    prepare_time: i64,
    interval: i64,
    cooling_time: i64,
    rise_interval: i64,
    decline: i64,
    lower_limit: i64,
    rise: i64,
    judgment_probability: i64,
    weapon_count: u32,
    preemptive: bool,
}

impl InterceptBlock {
    fn interception(&self, named: &str) -> std::result::Result<UnitInterception, String> {
        let interception = InterceptNumbers {
            attack: self.attack,
            range_max: self.range_max,
            range_min: self.range_min,
            prepare_time: self.prepare_time,
            interval: self.interval,
            cooling_time: self.cooling_time,
            rise_interval: self.rise_interval,
            decline: self.decline,
            lower_limit: self.lower_limit,
            rise: self.rise,
            judgment_probability: self.judgment_probability,
        }
        .interception(named)
        .map_err(|error| error.to_string())?;
        Ok(UnitInterception {
            interception,
            weapons: self.weapon_count,
            preemptive: self.preemptive,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    schema: String,
    technologies: Vec<Row>,
}

impl TechnologyEffects {
    /// Reads the tracked table.
    ///
    /// # Errors
    ///
    /// Returns an error when the table is not the one this build reads.
    pub(crate) fn load() -> Result<Self> {
        Self::parse(DEFAULT_TECHNOLOGY_EFFECTS)
    }

    fn parse(text: &str) -> Result<Self> {
        let table: Table = serde_yaml::from_str(text).map_err(|error| {
            Error::new(format!("cannot read the technology effect table: {error}"))
        })?;
        if table.schema != "mechcore.technology_effects" {
            return Err(Error::new(format!(
                "technology effect table declares schema {:?}",
                table.schema
            )));
        }
        let mut technologies = BTreeMap::new();
        for row in table.technologies {
            let id = row.id;
            // `LifestealTech.GetLifestealMuliplier` reads its row's list at
            // the unit's level, which `corrections_of` refuses past one entry;
            // no lifesteal row sets `ignoreElectricEffect`, so each answers
            // `CanDisable` true.
            let lifesteal = (row.kind == LIFESTEAL).then(|| LifeSteal {
                multiplier_q32: row.lifesteal_multiplier.first().copied().unwrap_or(0),
                priority: PRIORITY,
                can_disable: true,
            });
            // `AutoRecoveryTech` reads its two lists at the unit's level, as
            // `LifestealTech` does.
            let auto_recovery = (row.kind == AUTO_RECOVERY).then(|| AutoRecovery {
                start_time_q32: row.start_time,
                duration_q32: row.recovery_duration.first().copied().unwrap_or(0),
                life_rate_q32: row.recovery_life_rate.first().copied().unwrap_or(0),
                priority: PRIORITY,
                can_disable: true,
            });
            let energy_shield = (row.kind == ENERGY_SHIELD).then_some(EnergyShield {
                life_rate_q32: SHIELD_LIFE_RATE,
                priority: PRIORITY,
                can_disable: true,
            });
            // `SweepSkillIntensifyEffectProvider` hands the sweep its row's
            // changes; the row names the unit's main skill.
            let sweep = (row.kind == SWEEP).then_some(SweepIntensify {
                width_value: row.sweep_width_value,
                length_value: row.sweep_length_value,
                perpendicular: row.sweep_perpendicular,
                reverse: row.sweep_reverse,
                fixed_direction: row.sweep_fixed_direction,
            });
            let _ = row.sweep_skill_id;
            let reduce_damage = (row.kind == ARMOR).then(|| row.reduce_damage_value.clone());
            let who = format!("technology {} ({})", row.id, row.name);
            let buff_source = (row.kind == BUFF).then(|| {
                buffs::buff_source(
                    &who,
                    (row.buff_trigger, &row.buff_targets, row.probability),
                    &row.buff_cycle,
                    true,
                    &row.buff_special,
                    (row.buff.as_ref(), row.buff_range_item.as_ref()),
                )
            });
            let (buff_source, effect) = match buff_source {
                Some(Err(why)) => (None, Err(why)),
                Some(Ok(buff)) => (Some(buff), corrections_of(&row)),
                None => (None, corrections_of(&row)),
            };
            let (interception, effect) = match interception_of(&row, &who) {
                Ok(interception) => (interception, effect),
                Err(why) => (None, Err(why)),
            };
            let self_buff = buff_source.as_ref().is_some_and(|buff: &BuffSource| {
                matches!(
                    buff.trigger,
                    crate::modifier::BuffTrigger::All(crate::modifier::AllCycle {
                        reach: None,
                        ..
                    }) | crate::modifier::BuffTrigger::Hit
                        | crate::modifier::BuffTrigger::BeHit
                        | crate::modifier::BuffTrigger::Damaged
                )
            });
            let technology = Technology {
                unit: row.unit.clone(),
                effect,
                lifesteal,
                auto_recovery,
                energy_shield,
                sweep,
                reduce_damage,
                distance_intensify: row.kind == SEARCH_TARGET_SPECIFIC,
                air_attack: (row.kind == AIR_ATTACK).then_some(AirAttack {
                    extra_skills: row.extra_skill_effect,
                }),
                buff_source,
                interception,
                secondary_damage: (row.kind == SECONDARY_DAMAGE).then_some(SecondaryDamage {
                    damage: row.secondary_damage,
                    splash_radius: effects::fixed_to(row.secondary_splash_range, effects::METERS),
                    hits_main_target: row.secondary_hits_main_target,
                    buffed: row.secondary_buffed,
                }),
                switch_off_read: DISABLED_AS_NUMBERS.contains(&row.kind.as_str())
                    || row.kind == EXTRA_WEAPON
                    || self_buff,
            };
            if technologies.insert(id, technology).is_some() {
                return Err(Error::new(format!(
                    "technology effect table holds technology {id} twice"
                )));
            }
        }
        Ok(Self { technologies })
    }

    /// Every correction this side's technologies write onto one unit type.
    ///
    /// Every technology a unit may research is in the table, so an id it
    /// does not hold is refused too.
    ///
    /// # Errors
    ///
    /// Returns an error naming the technology when the side holds one this
    /// build cannot apply, rather than applying the part it understands.
    pub(crate) fn corrections(
        &self,
        held: &[i32],
        unit_type: &str,
        level: i64,
    ) -> Result<Vec<(Channel, Entry)>> {
        let mut written = Vec::new();
        for by_level in self.effects(held, unit_type)? {
            let last = by_level.len().saturating_sub(1);
            let index = usize::try_from(level - 1).unwrap_or_default().min(last);
            for (channel, index, correction) in &by_level[index] {
                written.push((
                    *channel,
                    Entry {
                        index: *index,
                        source: SOURCE,
                        correction: *correction,
                    },
                ));
            }
        }
        Ok(written)
    }

    /// What each technology this side holds for one unit type writes at each
    /// level, or the refusal of the first this build cannot apply.
    fn effects(&self, held: &[i32], unit_type: &str) -> Result<Vec<&Vec<Written>>> {
        let mut effects = Vec::new();
        for id in held {
            let Some(technology) = self.technologies.get(id) else {
                return Err(Error::new(format!(
                    "technology {id} is not in the technology effect table"
                )));
            };
            if technology.unit == unit_type {
                effects.push(
                    technology
                        .effect
                        .as_ref()
                        .map_err(|why| Error::new(why.clone()))?,
                );
            }
        }
        Ok(effects)
    }

    /// The sources this side's technologies hand one unit type's effect
    /// providers, each that is one: what they answer `ILifeSteal`,
    /// `IAutoRecovery` and `IEnergyShieldSource` with, and the buffs they add
    /// as the fight starts, in the order the side holds them.
    ///
    /// # Errors
    ///
    /// Returns the error [`Self::corrections`] does.
    pub(crate) fn sources(&self, held: &[i32], unit_type: &str) -> Result<UnitSources> {
        self.effects(held, unit_type)?;
        let own = held
            .iter()
            .filter_map(|id| self.technologies.get(id))
            .filter(|technology| technology.unit == unit_type);
        let mut sources = UnitSources::default();
        for technology in own {
            sources.lifesteal.extend(technology.lifesteal);
            sources.auto_recovery.extend(technology.auto_recovery);
            sources.energy_shield.extend(technology.energy_shield);
            sources.buff_sources.extend(technology.buff_source);
            sources.interception.extend(technology.interception);
        }
        Ok(sources)
    }

    /// What this side's technologies change about one unit type's main skill
    /// beyond its numbers.
    ///
    /// # Errors
    ///
    /// Returns the error [`Self::corrections`] does.
    pub(crate) fn main_skill(&self, held: &[i32], unit_type: &str) -> Result<MainSkill> {
        self.effects(held, unit_type)?;
        let own = || {
            held.iter()
                .filter_map(|id| self.technologies.get(id))
                .filter(|technology| technology.unit == unit_type)
        };
        Ok(MainSkill {
            sweep: own().find_map(|technology| technology.sweep),
            distance_intensify: own().any(|technology| technology.distance_intensify),
            air_attack: own().find_map(|technology| technology.air_attack),
            secondary_damage: own().find_map(|technology| technology.secondary_damage),
        })
    }

    /// This side's technologies on one unit type whose switching off by a
    /// disabling buff is not measured, by id.
    pub(crate) fn disabled_unmeasured(&self, held: &[i32], unit_type: &str) -> Vec<i32> {
        held.iter()
            .copied()
            .filter(|id| {
                self.technologies.get(id).is_some_and(|technology| {
                    technology.unit == unit_type && !technology.switch_off_read
                })
            })
            .collect()
    }

    /// What this side's armour technologies write onto one unit type at one
    /// level: `ArmorStrengthenEffectProvider.EnableEffect` adds each one's
    /// `GetReduceDamageValue` to the unit's `ReduceDamageValue`, which
    /// `ArmorStrengthenTechnologyData` reads at the unit's level, as
    /// `SkillData.GetDamage` reads a skill's damage, its last entry for a
    /// level beyond the list.
    ///
    /// # Errors
    ///
    /// Returns the error [`Self::corrections`] does.
    pub(crate) fn armor(
        &self,
        held: &[i32],
        unit_type: &str,
        level: i64,
    ) -> Result<Vec<(Channel, Entry)>> {
        self.effects(held, unit_type)?;
        Ok(held
            .iter()
            .filter_map(|id| self.technologies.get(id))
            .filter(|technology| technology.unit == unit_type)
            .filter_map(|technology| technology.reduce_damage.as_deref())
            .filter_map(|values| {
                let last = values.last()?;
                let at_level = usize::try_from(level - 1)
                    .ok()
                    .and_then(|index| values.get(index))
                    .unwrap_or(last);
                (*at_level != 0).then_some((
                    Channel::Unit,
                    Entry {
                        index: Index::ReduceDamage,
                        source: ARMOR_SOURCE,
                        correction: Correction::Value(*at_level),
                    },
                ))
            })
            .collect())
    }
}

/// What an interception row makes its unit, or why this build will not.
fn interception_of(row: &Row, who: &str) -> std::result::Result<Option<UnitInterception>, String> {
    if row.kind != INTERCEPT {
        return Ok(None);
    }
    row.intercept
        .ok_or_else(|| format!("{who} carries no interception"))?
        .interception(who)
        .map(Some)
}

/// What a row writes at each level, or why this build will not apply it.
fn corrections_of(row: &Row) -> std::result::Result<Vec<Written>, String> {
    let fought_extra_weapon = row.kind == EXTRA_WEAPON && FOUGHT_EXTRA_WEAPONS.contains(&row.id);
    if !IMPLEMENTED.contains(&row.kind.as_str()) && !fought_extra_weapon {
        return Err(format!(
            "technology {} ({}) comes from TechnologyGroupData's {} list, and what \
             it does beyond its unit's numbers is not implemented",
            row.id, row.name, row.kind
        ));
    }
    if !row.special.is_empty() {
        return Err(format!(
            "technology {} ({}) sets {}, which no mechanism here reads",
            row.id,
            row.name,
            row.special.join(", ")
        ));
    }
    if row.secondary_disables_technology || row.secondary_buff_id != 0 {
        return Err(format!(
            "technology {} ({}) disables the technologies of the units its second damage \
             strikes, or writes a buff on them, which is not measured",
            row.id, row.name
        ));
    }
    if row.kind == AUTO_RECOVERY && row.auto_recovery_state_type != NORMAL {
        return Err(format!(
            "technology {} ({}) repairs only in autoRecoveryStateType {}, underground or \
             cloaked, which no mechanism here reads",
            row.id, row.name, row.auto_recovery_state_type
        ));
    }
    // `LifestealTech` and `AutoRecoveryTech` hand their mechanism their first
    // entry here.
    let read_first = [
        ("lifesteal_multiplier", &row.lifesteal_multiplier),
        ("recovery_duration", &row.recovery_duration),
        ("recovery_life_rate", &row.recovery_life_rate),
    ];
    for (field, values) in read_first {
        if values.len() > 1 {
            return Err(format!(
                "technology {} ({}) grows {field} with the unit's level, and its \
                 mechanism here reads the first entry",
                row.id, row.name
            ));
        }
    }

    let unsupported = [(
        &row.min_attack_range_value,
        "min_attack_range_value",
        VALUE_ELSEWHERE,
    )];
    for (values, field, why) in unsupported {
        if values.iter().any(|value| *value != 0) {
            return Err(format!(
                "technology {} ({}) writes {field}, and {why}",
                row.id, row.name
            ));
        }
    }

    let levels = [
        &row.life_rate,
        &row.damage_rate,
        &row.speed_value,
        &row.attack_range_value,
        &row.attack_range_rate,
        &row.attack_interval_value,
        &row.attack_interval_rate,
        &row.splash_range_value,
        &row.projectile_speed_value,
        &row.projectile_life_rate,
        &row.air_damage_change_rate,
        &row.ground_damage_change_rate,
    ]
    .iter()
    .map(|values| values.len())
    .max()
    .unwrap_or_default()
    .max(1);
    Ok((0..levels).map(|level| at_level(row, level)).collect())
}

/// What a row writes at one level, counted from zero: each list's entry at
/// it, its last past it (`TechnologyData.GetLevelValue`).
fn at_level(row: &Row, level: usize) -> Written {
    let at_level = |values: &Vec<i64>| {
        values
            .get(level.min(values.len().saturating_sub(1)))
            .copied()
            .filter(|value| *value != 0)
    };
    let mut written = against_domains(row, at_level);
    written.extend(effects::corrections(Fields {
        life_rate: at_level(&row.life_rate),
        damage_rate: at_level(&row.damage_rate),
        // The table has no such column.
        damage_rate_by_kill_count: None,
        attack_range_rate: at_level(&row.attack_range_rate),
        attack_interval_rate: at_level(&row.attack_interval_rate),
        attack_range_value: at_level(&row.attack_range_value),
        attack_interval_value: at_level(&row.attack_interval_value),
        splash_range_value: at_level(&row.splash_range_value),
        speed_value: at_level(&row.speed_value),
        damage_reduce_rate_base: Some(row.all_weapon_reduce_damage_rate),
        projectile_speed_value: at_level(&row.projectile_speed_value),
        projectile_life_rate: at_level(&row.projectile_life_rate),
    }));
    written
}

/// What a technology writes onto its unit's skill against one domain: for
/// each domain a `SearchTargetSpecificTech` reaches further at, the metres
/// both into the range its `AttackRangeAirProperty` or
/// `AttackRangeGroundProperty` adds and into what its search counts off a
/// candidate of that domain (`SearchTargetSpecificProvider.DoEnable`), and
/// the rate its damage on that domain gains, which
/// `SearchTargetSpecificTech.AddData` and `DamageIntensifyTech.AddData` write
/// alike: Ground Specialization's 2 triples a Wasp's damage on the ground.
fn against_domains(row: &Row, at_level: impl Fn(&Vec<i64>) -> Option<i64>) -> Written {
    let mut written = Vec::new();
    for (domain, offset, damage_rate) in [
        (
            UnitDomain::Air,
            row.air_target_score_offset,
            &row.air_damage_change_rate,
        ),
        (
            UnitDomain::Ground,
            row.ground_target_score_offset,
            &row.ground_damage_change_rate,
        ),
    ] {
        if offset > 0 {
            let metres = Correction::Value(offset.saturating_mul(effects::METERS));
            written.push((Channel::Skill, Index::ScoreOffsetFor(domain), metres));
            written.push((Channel::Skill, Index::RangeAgainst(domain), metres));
        }
        if let Some(rate) = at_level(damage_rate) {
            written.push((
                Channel::Skill,
                Index::DamageRateAgainst(domain),
                Correction::Value(rate),
            ));
        }
    }
    written
}

#[cfg(test)]
mod tests {
    use super::TechnologyEffects;
    use crate::data::{Channel, Correction, Index};

    /// Range Enhancement for the Marksman: `+40` metres and nothing else.
    const RANGE_ENHANCEMENT: i32 = 10202;
    /// Elite Marksman for the Fortress, whose effect grows with level.
    const ELITE_MARKSMAN: i32 = 10801;
    /// Assault Mode for the Marksman, a plain technology that corrects a
    /// splash radius.
    const ASSAULT_MODE: i32 = 10102;
    /// Grenade Launcher for the Fang, an `airAttackTechnologyDatas` row that
    /// also writes a range, a splash and a projectile speed.
    const GRENADE_LAUNCHER: i32 = 3109;
    /// Anti-Aircraft Ammunition for the Arclight, an
    /// `airAttackTechnologyDatas` row that reaches no extra skill.
    const ANTI_AIRCRAFT_AMMUNITION: i32 = 3115;
    /// Missile Interception for the Mustang, an
    /// `interceptMissileTechnologyDatas` row.
    const MISSILE_INTERCEPTION: i32 = 3307;
    /// Machine Learning for the Vortex, a plain technology that corrects the
    /// experience its unit gains.
    const MACHINE_LEARNING: i32 = 10131;
    /// Armor Enhancement for the Rhino, an `armorStrengthenTechnologyDatas`
    /// row: +0.5 of life and 60 off each hit a level.
    const ARMOR_ENHANCEMENT: i32 = 3005;
    /// Aerial Specialization for the Marksman, a `searchTargetSpecificDatas`
    /// row: 30 metres and 0.9 of damage against aircraft.
    const AERIAL_SPECIALIZATION: i32 = 3202;
    /// Ground Specialization for the Wasp, a `damageIntensifyTechnologies`
    /// row: 2 of damage against the ground.
    const GROUND_SPECIALIZATION: i32 = 506;
    /// Shockwave for the Arclight, a `secondaryDamageIntensifyTechDatas` row:
    /// 75 within 30 metres, and 5 metres off its range.
    const SHOCKWAVE: i32 = 4515;
    /// Combat Evolvement for the Rhino, a `buffTechnologies` row whose buff
    /// stacks every second.
    const COMBAT_EVOLVEMENT: i32 = 180_805;
    /// Kinetic Charge for the Steel Ball, whose buff stacks on distance.
    const KINETIC_CHARGE: i32 = 180_808;
    /// Mobile Power Station and Degeneration Beam.
    const MOBILE_POWER_STATION: i32 = 180_931;
    const DEGENERATION_BEAM: i32 = 180_418;
    /// Suppression Shots for the Void Eye, whose buff comes with a hit.
    const SUPPRESSION_SHOTS: i32 = 180_430;
    /// Replicate for the Crawler, whose hit's buff summons as its unit dies.
    const REPLICATE: i32 = 180_110;
    /// Electromagnetic Cloud for the Vortex, whose second damage disables
    /// technologies and writes a buff.
    const ELECTROMAGNETIC_CLOUD: i32 = 4531;

    #[test]
    fn a_technology_writes_onto_the_unit_whose_table_row_names_it() {
        let table = TechnologyEffects::load().unwrap();
        let written = table
            .corrections(&[RANGE_ENHANCEMENT], "marksman", 1)
            .unwrap();
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].0, Channel::Skill);
        assert_eq!(written[0].1.index, Index::AttackRange);
        assert_eq!(written[0].1.correction, Correction::Value(40_000));

        assert!(
            table
                .corrections(&[RANGE_ENHANCEMENT], "arclight", 1)
                .unwrap()
                .is_empty(),
            "another unit's Range Enhancement is another row"
        );
    }

    /// A technology whose effect grows writes its entry for the unit's level,
    /// and its last past the list: Elite Marksman's range is 5 metres a level.
    #[test]
    fn a_technology_that_grows_writes_the_entry_for_the_level() {
        let table = TechnologyEffects::load().unwrap();
        let range = |level| {
            table
                .corrections(&[ELITE_MARKSMAN], "fortress", level)
                .unwrap()
                .into_iter()
                .find(|(_, entry)| entry.index == Index::AttackRange)
                .map(|(_, entry)| entry.correction)
        };
        assert_eq!(range(1), Some(Correction::Value(5_000)));
        assert_eq!(range(3), Some(Correction::Value(15_000)));
        assert_eq!(range(12), Some(Correction::Value(45_000)));
    }

    /// Assault Mode's splash value lands in the skill's channel, as the
    /// skill's `SplashRangeValue`.
    #[test]
    fn a_technology_correcting_a_splash_writes_the_skill() {
        let table = TechnologyEffects::load().unwrap();
        let written = table.corrections(&[ASSAULT_MODE], "marksman", 1).unwrap();
        assert!(
            written
                .iter()
                .any(|(channel, entry)| *channel == Channel::Skill
                    && entry.index == Index::SplashRange),
            "{written:?}"
        );
    }

    /// A technology of a subclass is refused by name and kind, numbers and
    /// all: Grenade Launcher's splash, and Fang Production's summons.
    #[test]
    fn a_technology_that_does_more_than_numbers_is_refused() {
        let table = TechnologyEffects::load().unwrap();
        for (id, unit, kind) in [
            (1201, "fortress", "supportUnitTechnologies"),
            (812, "stormcaller", "fireIntensifyTechnologies"),
        ] {
            let refused = table.corrections(&[id], unit, 1).unwrap_err().to_string();
            assert!(refused.contains(&id.to_string()), "{refused}");
            assert!(refused.contains(kind), "{refused}");
        }
        let refused = table
            .corrections(&[MACHINE_LEARNING], "vortex", 1)
            .unwrap_err()
            .to_string();
        assert!(refused.contains("expChangeRate"), "{refused}");
    }

    /// An armour technology writes its reduction at the unit's level, the
    /// last entry beyond its list, beside its life rate.
    #[test]
    fn an_armour_technology_reduces_by_the_units_level() {
        let table = TechnologyEffects::load().unwrap();
        let written = table.corrections(&[ARMOR_ENHANCEMENT], "rhino", 1).unwrap();
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].1.index, Index::MaxLife);
        for (level, reduction) in [(1, 60), (3, 180), (12, 540)] {
            let armour = table.armor(&[ARMOR_ENHANCEMENT], "rhino", level).unwrap();
            assert_eq!(armour.len(), 1);
            assert_eq!(armour[0].0, Channel::Unit);
            assert_eq!(armour[0].1.index, Index::ReduceDamage);
            assert_eq!(armour[0].1.correction, Correction::Value(reduction));
        }
        assert!(
            table
                .armor(&[ARMOR_ENHANCEMENT], "marksman", 1)
                .unwrap()
                .is_empty()
        );
    }

    /// A search-target technology writes its metres into the skill's air
    /// range and air search offset and its rate into the air damage rate, and
    /// turns the unit's search to `DistanceIntensify`.
    #[test]
    fn aerial_specialization_writes_its_numbers_against_aircraft() {
        use crate::rules::UnitDomain::Air;
        let table = TechnologyEffects::load().unwrap();
        let written = table
            .corrections(&[AERIAL_SPECIALIZATION], "marksman", 1)
            .unwrap()
            .into_iter()
            .map(|(channel, entry)| (channel, entry.index, entry.correction))
            .collect::<Vec<_>>();
        assert_eq!(
            written,
            [
                (
                    Channel::Skill,
                    Index::ScoreOffsetFor(Air),
                    Correction::Value(30_000)
                ),
                (
                    Channel::Skill,
                    Index::RangeAgainst(Air),
                    Correction::Value(30_000)
                ),
                (
                    Channel::Skill,
                    Index::DamageRateAgainst(Air),
                    Correction::Value(3_865_470_566)
                ),
            ]
        );
        assert!(
            table
                .main_skill(&[AERIAL_SPECIALIZATION], "marksman")
                .unwrap()
                .distance_intensify
        );
        assert!(
            !table
                .main_skill(&[AERIAL_SPECIALIZATION], "wasp")
                .unwrap()
                .distance_intensify
        );
    }

    /// An air-attack technology turns its unit's skill onto or off aircraft,
    /// its extra skills where its row says so, and writes its numbers.
    #[test]
    fn an_air_attack_technology_switches_and_writes_its_numbers() {
        let table = TechnologyEffects::load().unwrap();
        let fang = table.main_skill(&[GRENADE_LAUNCHER], "fang").unwrap();
        assert_eq!(
            fang.air_attack,
            Some(super::AirAttack { extra_skills: true })
        );
        let indices = table
            .corrections(&[GRENADE_LAUNCHER], "fang", 1)
            .unwrap()
            .into_iter()
            .map(|(_, entry)| entry.index)
            .collect::<Vec<_>>();
        assert_eq!(
            indices,
            [
                Index::AttackRange,
                Index::SplashRange,
                Index::ProjectileSpeed
            ]
        );
        let arclight = table
            .main_skill(&[ANTI_AIRCRAFT_AMMUNITION], "arclight")
            .unwrap();
        assert_eq!(
            arclight.air_attack,
            Some(super::AirAttack {
                extra_skills: false
            })
        );
    }

    /// A damage-intensify technology writes its rate against one domain and
    /// nothing else: Ground Specialization's 2 on a Wasp's ground damage.
    #[test]
    fn ground_specialization_writes_its_ground_rate() {
        use crate::rules::UnitDomain::Ground;
        let table = TechnologyEffects::load().unwrap();
        let written = table
            .corrections(&[GROUND_SPECIALIZATION], "wasp", 1)
            .unwrap()
            .into_iter()
            .map(|(channel, entry)| (channel, entry.index, entry.correction))
            .collect::<Vec<_>>();
        assert_eq!(
            written,
            [(
                Channel::Skill,
                Index::DamageRateAgainst(Ground),
                Correction::Value(2 << 32)
            )]
        );
    }

    /// A secondary-damage technology hands its unit's main skill its second
    /// damage and writes its numbers; one whose second damage disables
    /// technologies is refused by name.
    #[test]
    fn shockwave_hands_a_second_damage() {
        let table = TechnologyEffects::load().unwrap();
        let shockwave = table.main_skill(&[SHOCKWAVE], "arclight").unwrap();
        assert_eq!(
            shockwave.secondary_damage,
            Some(super::SecondaryDamage {
                damage: 75,
                splash_radius: 30_000,
                hits_main_target: false,
                buffed: true,
            })
        );
        let written = table.corrections(&[SHOCKWAVE], "arclight", 1).unwrap();
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].1.correction, Correction::Value(-5_000));
        let refused = table
            .corrections(&[ELECTROMAGNETIC_CLOUD], "vortex", 1)
            .unwrap_err()
            .to_string();
        assert!(refused.contains("4531"), "{refused}");
    }

    /// An interception technology hands its unit the interceptors it adds:
    /// the Mustang's one locks its main skill, and the War Factory's four
    /// leave it be.
    #[test]
    fn an_interception_technology_hands_its_unit_interceptors() {
        let table = TechnologyEffects::load().unwrap();
        let mustang = table
            .sources(&[MISSILE_INTERCEPTION], "mustang")
            .unwrap()
            .interception;
        assert_eq!(mustang.len(), 1);
        assert_eq!((mustang[0].weapons, mustang[0].preemptive), (1, true));
        let interception = mustang[0].interception;
        assert_eq!(interception.attack, 21067);
        assert_eq!(
            (interception.prepare_ticks, interception.interval_ticks),
            (4, 8)
        );
        // `attackNum × 0.08` and `× 0.4`, each rate a hair under its value.
        assert_eq!((interception.decline, interception.lower), (1685, 8426));
        let war_factory = table.sources(&[3317], "war_factory").unwrap().interception;
        assert_eq!(
            (war_factory[0].weapons, war_factory[0].preemptive),
            (4, false)
        );
    }

    /// A buff technology hands its unit the buff it adds as the fight starts:
    /// Combat Evolvement's stacks every second, and Kinetic Charge's a metre
    /// of range for every 7 rolled, to 100.
    #[test]
    fn a_buff_technology_adds_a_stacking_buff() {
        let table = TechnologyEffects::load().unwrap();
        let buffs = table
            .sources(&[COMBAT_EVOLVEMENT], "rhino")
            .unwrap()
            .buff_sources;
        assert_eq!(buffs.len(), 1);
        assert_eq!(buffs[0].buff_id, 8005);
        assert_eq!(buffs[0].damage_rate, 193_273_528);
        assert_eq!(buffs[0].max_life_rate, 107_374_182);
        assert_eq!(buffs[0].step_q32, 1 << 32);
        assert!(buffs[0].stacking.is_some());
        assert!(
            table
                .sources(&[COMBAT_EVOLVEMENT], "marksman")
                .unwrap()
                .buff_sources
                .is_empty()
        );
        let rolled = table
            .sources(&[KINETIC_CHARGE], "steel_ball")
            .unwrap()
            .buff_sources;
        let stacking = rolled[0].stacking.unwrap();
        assert_eq!(rolled[0].attack_range_value, 1);
        assert_eq!(stacking.max, 100);
        assert_eq!(
            stacking.condition,
            crate::modifier::StackCondition::Distance {
                metres_q32: 7 << 32
            }
        );
    }

    /// Suppression Shots adds its buff on a hit, cutting the struck unit's
    /// range by 30%.
    #[test]
    fn a_buff_added_on_a_hit() {
        let table = TechnologyEffects::load().unwrap();
        let shots = table
            .sources(&[SUPPRESSION_SHOTS], "void_eye")
            .unwrap()
            .buff_sources;
        assert_eq!(shots[0].trigger, crate::modifier::BuffTrigger::Hit);
        assert_eq!(shots[0].buff_id, 10301);
        assert_eq!(shots[0].attack_range_rate, -1_288_490_188);
        assert!(shots[0].can_disable);
        let replicate = table.sources(&[REPLICATE], "crawler").unwrap().buff_sources;
        assert_eq!(replicate[0].trigger, crate::modifier::BuffTrigger::Hit);
        assert_eq!(
            replicate[0].summons,
            Some(crate::modifier::DeadSummon::SourceType)
        );
    }

    /// A buff technology of the update model `Each` keeps its buff on the
    /// units in reach: Mobile Power Station on its side's ground units within
    /// 100 m, Degeneration Beam on the enemies of either domain within 120 m,
    /// each to the unit's edge.
    #[test]
    fn a_buff_kept_on_the_units_around() {
        let table = TechnologyEffects::load().unwrap();
        let station = table
            .sources(&[MOBILE_POWER_STATION], "vortex")
            .unwrap()
            .buff_sources;
        let crate::modifier::BuffTrigger::Around(reach) = station[0].trigger else {
            panic!("Mobile Power Station keeps its buff on the units around");
        };
        assert_eq!(station[0].buff_id, 10001);
        assert_eq!(reach.range_q32, 100 << 32);
        assert!(reach.domains.ground && !reach.domains.air);
        assert!(reach.target_radius);
        let targets = reach.targets;
        assert!(targets.itself && targets.own_others && !targets.opponents);
        let beam = table
            .sources(&[DEGENERATION_BEAM], "wraith")
            .unwrap()
            .buff_sources;
        let crate::modifier::BuffTrigger::Around(reach) = beam[0].trigger else {
            panic!("Degeneration Beam keeps its buff on the units around");
        };
        assert_eq!(beam[0].speed_rate, -1_717_986_918);
        assert_eq!(reach.range_q32, 120 << 32);
        assert!(reach.domains.ground && reach.domains.air);
        assert!(reach.targets.opponents && !reach.targets.itself);
    }
}
