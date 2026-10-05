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
//! `AirAttackTech` its skills turned onto or off aircraft; any other is
//! refused by name rather than applied for its numbers alone.
//!
//! A technology belongs to one unit type, which is how a side's flat list of
//! technologies reaches the units it corrects: a technology the side holds
//! writes onto the units its table row names and onto nothing else.
//!
//! **An effect is a list indexed by the unit's rank, and this reads rank one.**
//! A technology whose list holds more than one entry is refused rather than
//! read at index zero, because which index a fight reads for a given rank is
//! `docs/rules/technology_effects.md`'s unresolved question and a layout's
//! units above rank one are refused by the module registry anyway.
//! An armour technology's reduction is not one of these effects: the build
//! reads it at the unit's level itself.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::{
    Error, Result,
    data::{Channel, Correction, Entry, Index},
    rules::UnitDomain,
};

use super::{
    effects::{self, Fields, PROJECTILE, VALUE_ELSEWHERE},
    sources::{AutoRecovery, EnergyShield, LifeSteal, SweepIntensify},
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

/// The lists whose rows this build applies, each with its mechanism.
const IMPLEMENTED: [&str; 9] = [
    PLAIN,
    LIFESTEAL,
    AUTO_RECOVERY,
    ENERGY_SHIELD,
    SWEEP,
    ARMOR,
    SEARCH_TARGET_SPECIFIC,
    AIR_ATTACK,
    DAMAGE_INTENSIFY,
];

/// The list whose `DamageIntensifyTech` writes its damage against one domain.
const DAMAGE_INTENSIFY: &str = "damageIntensifyTechnologies";

/// The list whose `AirAttackTech` is an `IAirAttackDataSource`.
const AIR_ATTACK: &str = "airAttackTechnologyDatas";

/// The list whose `ExtraWeaponTech` adds a skill beside its unit's main one.
const EXTRA_WEAPON: &str = "extraWeaponTechnologies";

/// The extra weapon technologies the fight runs. Each member's skill and
/// what it does beyond a projectile differ, so each joins once a recording
/// of it agrees: Secondary Armament and Anti-Air Missile, the Sabertooth's,
/// Incendiary Bomb, Scorching Charge, Homing Missile, Sticky Oil Bomb, the
/// Phantom Ray's and the Vulcan's, Whirlwind, the Rhino's, Energy
/// Diffraction, the Melting Point's, Spider Mine, the Tarantula's, Matrix
/// Bombardment, the Wraith's, Anti-Air Barrage, the Fortress's, and Air
/// Defense Mark, the Typhoon's.
pub(crate) const FOUGHT_EXTRA_WEAPONS: [i32; 13] = [
    1_105, 1_107, 1_109, 11_010, 11_020, 11_024, 11_025, 11_028, 110_181, 110_211, 110_212,
    110_322, 1_102_022,
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

#[derive(Debug, Clone)]
struct Technology {
    /// The unit type whose numbers it corrects.
    unit: String,
    /// What it writes, or why this build will not apply it.
    effect: std::result::Result<Vec<(Channel, Index, Correction)>, String>,
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
}

/// What an `AirAttackEffectProvider` does with one technology: it turns the
/// main skill onto aircraft if it attacks none and off them if it does,
/// adding 1 or -1 to its `AirAttackValue`, and the same to each extra
/// skill's where the row's `extraSkillEffect` says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AirAttack {
    pub(crate) extra_skills: bool,
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
}

/// One row of the table. Every effect is a list because a technology's effect
/// can grow with the unit's rank.
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
    /// metres, and a rate by the unit's rank, which a
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
            // the unit's rank, which `corrections_of` refuses past one entry;
            // no lifesteal row sets `ignoreElectricEffect`, so each answers
            // `CanDisable` true.
            let lifesteal = (row.kind == LIFESTEAL).then(|| LifeSteal {
                multiplier_q32: row.lifesteal_multiplier.first().copied().unwrap_or(0),
                priority: PRIORITY,
                can_disable: true,
            });
            // `AutoRecoveryTech` reads its two lists at the unit's rank, as
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
            let technology = Technology {
                unit: row.unit.clone(),
                effect: corrections_of(&row),
                lifesteal,
                auto_recovery,
                energy_shield,
                sweep,
                reduce_damage,
                distance_intensify: row.kind == SEARCH_TARGET_SPECIFIC,
                air_attack: (row.kind == AIR_ATTACK).then_some(AirAttack {
                    extra_skills: row.extra_skill_effect,
                }),
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
    ) -> Result<Vec<(Channel, Entry)>> {
        let mut written = Vec::new();
        for id in held {
            let Some(technology) = self.technologies.get(id) else {
                return Err(Error::new(format!(
                    "technology {id} is not in the technology effect table"
                )));
            };
            if technology.unit != unit_type {
                continue;
            }
            let corrections = technology
                .effect
                .as_ref()
                .map_err(|why| Error::new(why.clone()))?;
            for (channel, index, correction) in corrections {
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

    /// What this side's technologies answer `ILifeSteal` with on one unit
    /// type, each that is one.
    ///
    /// # Errors
    ///
    /// Returns the error [`Self::corrections`] does.
    pub(crate) fn lifesteal(&self, held: &[i32], unit_type: &str) -> Result<Vec<LifeSteal>> {
        self.corrections(held, unit_type)?;
        Ok(held
            .iter()
            .filter_map(|id| self.technologies.get(id))
            .filter(|technology| technology.unit == unit_type)
            .filter_map(|technology| technology.lifesteal)
            .collect())
    }

    /// What this side's technologies answer `IEnergyShieldSource` with on
    /// one unit type, each that is one.
    ///
    /// # Errors
    ///
    /// Returns the error [`Self::corrections`] does.
    pub(crate) fn energy_shield(&self, held: &[i32], unit_type: &str) -> Result<Vec<EnergyShield>> {
        self.corrections(held, unit_type)?;
        Ok(held
            .iter()
            .filter_map(|id| self.technologies.get(id))
            .filter(|technology| technology.unit == unit_type)
            .filter_map(|technology| technology.energy_shield)
            .collect())
    }

    /// What this side's technologies change about one unit type's main skill
    /// beyond its numbers.
    ///
    /// # Errors
    ///
    /// Returns the error [`Self::corrections`] does.
    pub(crate) fn main_skill(&self, held: &[i32], unit_type: &str) -> Result<MainSkill> {
        self.corrections(held, unit_type)?;
        let own = || {
            held.iter()
                .filter_map(|id| self.technologies.get(id))
                .filter(|technology| technology.unit == unit_type)
        };
        Ok(MainSkill {
            sweep: own().find_map(|technology| technology.sweep),
            distance_intensify: own().any(|technology| technology.distance_intensify),
            air_attack: own().find_map(|technology| technology.air_attack),
        })
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
        self.corrections(held, unit_type)?;
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

    /// What this side's technologies answer `IAutoRecovery` with on one unit
    /// type, each that is one.
    ///
    /// # Errors
    ///
    /// Returns the error [`Self::corrections`] does.
    pub(crate) fn auto_recovery(&self, held: &[i32], unit_type: &str) -> Result<Vec<AutoRecovery>> {
        self.corrections(held, unit_type)?;
        Ok(held
            .iter()
            .filter_map(|id| self.technologies.get(id))
            .filter(|technology| technology.unit == unit_type)
            .filter_map(|technology| technology.auto_recovery)
            .collect())
    }
}

/// What a row writes at rank one, or why this build will not apply it.
fn corrections_of(row: &Row) -> std::result::Result<Vec<(Channel, Index, Correction)>, String> {
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
    if row.kind == AUTO_RECOVERY && row.auto_recovery_state_type != NORMAL {
        return Err(format!(
            "technology {} ({}) repairs only in autoRecoveryStateType {}, underground or \
             cloaked, which no mechanism here reads",
            row.id, row.name, row.auto_recovery_state_type
        ));
    }
    let every = [
        ("life_rate", &row.life_rate),
        ("damage_rate", &row.damage_rate),
        ("speed_value", &row.speed_value),
        ("min_attack_range_value", &row.min_attack_range_value),
        ("attack_range_value", &row.attack_range_value),
        ("attack_range_rate", &row.attack_range_rate),
        ("attack_interval_value", &row.attack_interval_value),
        ("attack_interval_rate", &row.attack_interval_rate),
        ("splash_range_value", &row.splash_range_value),
        ("projectile_speed_value", &row.projectile_speed_value),
        ("projectile_life_rate", &row.projectile_life_rate),
        ("lifesteal_multiplier", &row.lifesteal_multiplier),
        ("recovery_duration", &row.recovery_duration),
        ("recovery_life_rate", &row.recovery_life_rate),
        ("air_damage_change_rate", &row.air_damage_change_rate),
        ("ground_damage_change_rate", &row.ground_damage_change_rate),
    ];
    for (field, values) in every {
        if values.len() > 1 {
            return Err(format!(
                "technology {} ({}) grows with the unit's rank, and which entry of \
                 {field} a fight reads for a given rank is not established: see the \
                 unresolved questions in docs/rules/technology_effects.md",
                row.id, row.name
            ));
        }
    }

    let unsupported = [
        (
            &row.min_attack_range_value,
            "min_attack_range_value",
            VALUE_ELSEWHERE,
        ),
        (
            &row.projectile_life_rate,
            "projectile_life_rate",
            PROJECTILE,
        ),
    ];
    for (values, field, why) in unsupported {
        if values.iter().any(|value| *value != 0) {
            return Err(format!(
                "technology {} ({}) writes {field}, and {why}",
                row.id, row.name
            ));
        }
    }

    let at_rank_one = |values: &Vec<i64>| values.first().copied().filter(|value| *value != 0);
    let mut written = against_domains(row, at_rank_one);
    written.extend(effects::corrections(Fields {
        life_rate: at_rank_one(&row.life_rate),
        damage_rate: at_rank_one(&row.damage_rate),
        // The table has no such column.
        damage_rate_by_kill_count: None,
        attack_range_rate: at_rank_one(&row.attack_range_rate),
        attack_interval_rate: at_rank_one(&row.attack_interval_rate),
        attack_range_value: at_rank_one(&row.attack_range_value),
        attack_interval_value: at_rank_one(&row.attack_interval_value),
        splash_range_value: at_rank_one(&row.splash_range_value),
        speed_value: at_rank_one(&row.speed_value),
        damage_reduce_rate_base: Some(row.all_weapon_reduce_damage_rate),
        projectile_speed_value: at_rank_one(&row.projectile_speed_value),
    }));
    Ok(written)
}

/// What a technology writes onto its unit's skill against one domain: for
/// each domain a `SearchTargetSpecificTech` reaches further at, the metres
/// both into the range its `AttackRangeAirProperty` or
/// `AttackRangeGroundProperty` adds and into what its search counts off a
/// candidate of that domain (`SearchTargetSpecificProvider.DoEnable`), and
/// the rate its damage on that domain gains, which
/// `SearchTargetSpecificTech.AddData` and `DamageIntensifyTech.AddData` write
/// alike: Ground Specialization's 2 triples a Wasp's damage on the ground.
fn against_domains(
    row: &Row,
    at_rank_one: impl Fn(&Vec<i64>) -> Option<i64>,
) -> Vec<(Channel, Index, Correction)> {
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
        if let Some(rate) = at_rank_one(damage_rate) {
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
    /// Elite Marksman for the Fortress, whose effect grows with rank.
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

    #[test]
    fn a_technology_writes_onto_the_unit_whose_table_row_names_it() {
        let table = TechnologyEffects::load().unwrap();
        let written = table.corrections(&[RANGE_ENHANCEMENT], "marksman").unwrap();
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].0, Channel::Skill);
        assert_eq!(written[0].1.index, Index::AttackRange);
        assert_eq!(written[0].1.correction, Correction::Value(40_000));

        assert!(
            table
                .corrections(&[RANGE_ENHANCEMENT], "arclight")
                .unwrap()
                .is_empty(),
            "another unit's Range Enhancement is another row"
        );
    }

    /// A technology whose effect grows is refused rather than read at rank
    /// one, because which entry a rank reads is not established.
    #[test]
    fn a_technology_that_grows_with_rank_is_refused() {
        let table = TechnologyEffects::load().unwrap();
        let refused = table
            .corrections(&[ELITE_MARKSMAN], "fortress")
            .unwrap_err()
            .to_string();
        assert!(refused.contains("10801"), "{refused}");
        assert!(refused.contains("rank"), "{refused}");
    }

    /// Assault Mode's splash value lands in the skill's channel, as the
    /// skill's `SplashRangeValue`.
    #[test]
    fn a_technology_correcting_a_splash_writes_the_skill() {
        let table = TechnologyEffects::load().unwrap();
        let written = table.corrections(&[ASSAULT_MODE], "marksman").unwrap();
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
            (
                MISSILE_INTERCEPTION,
                "mustang",
                "interceptMissileTechnologyDatas",
            ),
            (1201, "fortress", "supportUnitTechnologies"),
        ] {
            let refused = table.corrections(&[id], unit).unwrap_err().to_string();
            assert!(refused.contains(&id.to_string()), "{refused}");
            assert!(refused.contains(kind), "{refused}");
        }
        let refused = table
            .corrections(&[MACHINE_LEARNING], "vortex")
            .unwrap_err()
            .to_string();
        assert!(refused.contains("expChangeRate"), "{refused}");
    }

    /// An armour technology writes its reduction at the unit's level, the
    /// last entry beyond its list, beside its life rate.
    #[test]
    fn an_armour_technology_reduces_by_the_units_level() {
        let table = TechnologyEffects::load().unwrap();
        let written = table.corrections(&[ARMOR_ENHANCEMENT], "rhino").unwrap();
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
            .corrections(&[AERIAL_SPECIALIZATION], "marksman")
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
            .corrections(&[GRENADE_LAUNCHER], "fang")
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
            .corrections(&[GROUND_SPECIALIZATION], "wasp")
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
}
