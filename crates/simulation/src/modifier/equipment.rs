//! What an equipment writes onto the unit that wears it.
//!
//! `config/equipment_effects.yaml` is the build's table of ordinary
//! `EquipmentData` rows, extracted by `scripts/extract/extract-equipment-effects.py`,
//! and `docs/rules/equipment_effects.md` states what each field means.
//! `Equipment.AddData` writes through `MechDataModifer.TryAddCommonData` and
//! `SkillDataModifier.AddData`, the writers an officer's correction goes
//! through, so a row becomes the corrections [`super::effects`] makes of an
//! officer's, in the same channels, and sums with an officer's there.
//!
//! **Every class writes its numbers; a class's own mechanism is its kind's.**
//! Each row's `kind` is the list of `EquipmentGroupData` it comes from, and
//! `Equipment.AddData` writes the plain correction of a row of any class. A
//! kind whose subclass does nothing more in a fight is applied for its
//! numbers; one whose subclass does more (a buff, lifesteal, a shield) is
//! refused by name until its mechanism lands, rather than applied for its
//! numbers alone.
//!
//! An equipment this build cannot apply is refused by name rather than partly
//! applied: a kind whose mechanism is not here, a lifetime this build has not
//! measured, a field no mechanism here reads, and a targeting this build does
//! not resolve.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::{
    Error, Result,
    data::{Channel, Correction, Entry, Index},
    rules::UnitConfig,
};

use super::{
    effects::{self, Fields, PROJECTILE, VALUE_ELSEWHERE},
    sources::{AutoRecovery, CarriedShield, EnergyShield, LifeSteal, ProductionLine, StartBuff},
    targets::Targets,
};

const DEFAULT_EQUIPMENT_EFFECTS: &str = include_str!("../../../../config/equipment_effects.yaml");

/// The kinds applied, each the list of `EquipmentGroupData` it comes from:
/// `equipmentDatas`, the plain item; `mobilityIntensifyEquipmentDatas`, whose
/// `MobilityIntensifyEquipment` overrides nothing of `Equipment` and frees its
/// formation during deployment, which a fight does not read; [`LIFESTEAL`];
/// [`AUTO_RECOVERY`]; [`SPLASH`]; [`BUFF`]; [`IGNORE_BUFF`];
/// [`ENERGY_SHIELD`]; [`BARRIER`]; and [`PRODUCTION`].
const APPLIED: [&str; 10] = [
    "equipmentDatas",
    "mobilityIntensifyEquipmentDatas",
    LIFESTEAL,
    AUTO_RECOVERY,
    SPLASH,
    BUFF,
    IGNORE_BUFF,
    ENERGY_SHIELD,
    BARRIER,
    PRODUCTION,
];

/// The list whose `SupportUnitEquipment` is an `ISupportDataSource`, which
/// hands its unit a [`ProductionLine`].
const PRODUCTION: &str = "supportUnitEquipmentDatas";

/// `SupportUnitAppearType` 5: makes stand at the row's offsets from the
/// wearer.
const AT_OFFSETS: i32 = 5;

/// The list whose `AdvancedEnergyShieldEquipment` is an
/// `IAdvancedEnergyShieldSource`, which hands its unit a [`CarriedShield`]:
/// its `GetRecoverTime` and `GetEnergyChangeValue` answer zero, so the shield
/// it carries neither recovers nor refills.
const BARRIER: &str = "advancedEnergyShieldEquipmentDatas";

/// The list whose `EnergyShieldEquipment` is an `IEnergyShieldSource`,
/// which hands its unit an [`EnergyShield`] of its row's `lifeRate`.
const ENERGY_SHIELD: &str = "energyShieldEquipmentDatas";

/// The list whose `IgnoreBuffEquipment` is an `IIgnoreBuffDataSouce`:
/// `IgnoreBuffEffectSystem.ApplyIgnoreBuff` adds every buff of its group to
/// its unit's `BuffManager.AddIgnoredBuff` as it enters the fight, for good,
/// since its `GetDuration` is zero. Its row is a permanent effect, which
/// `EffectProvider.ActiveCheck` activates before the fight, during
/// deployment; `IgnoreBuffEffectSystem.Active` then holds the buffs until
/// `OnEnterFight`, so in the fight they are ignored from its start.
const IGNORE_BUFF: &str = "ignoreBuffEquipmentDatas";

/// The list whose `LifestealEquipment` is an `ILifeSteal` as well, which
/// hands its unit a [`LifeSteal`].
const LIFESTEAL: &str = "lifestealEquipmentDatas";

/// The list whose `AutoRecoveryEquipment` is an `IAutoRecovery` as well,
/// whose `GetAutoRecoveryStateType` is `Normal`, and which hands its unit an
/// [`AutoRecovery`].
const AUTO_RECOVERY: &str = "autoRecoveryEquipmentDatas";

/// The list whose `BuffEquipment` is an `IEffectBuffDataSource`: a buff it
/// adds on a trigger. The one trigger read is the fight's start, onto the
/// unit itself, which hands its unit a [`StartBuff`].
const BUFF: &str = "buffEquipmentDatas";

/// `BuffTechListener.FightStart`.
const FIGHT_START: i32 = 1;

/// `TargetType.MechUnit`: the unit the buff's source is on.
const MECH_UNIT: i32 = 1;

/// One, Q32.32.
const ONE: i64 = 1 << 32;

/// The list whose `SplashEquipment.AddData` writes its row's correction and
/// then its `range` into the skill's `SkillDataChangeFloat.SplashRangeValue`.
const SPLASH: &str = "splashEquipmentDatas";

/// `Equipment.GetPriority`, which overrides a technology's 0.
const PRIORITY: i32 = 1;

/// The module that tags every entry an equipment writes.
pub(crate) const SOURCE: &str = "Modifier";

/// Every ordinary equipment's combat effect, by the id a layout compiles to.
#[derive(Debug, Clone)]
pub(crate) struct EquipmentEffects {
    equipment: BTreeMap<i32, Equipment>,
}

#[derive(Debug, Clone)]
struct Equipment {
    /// Which units the row reaches.
    targets: Targets,
    /// What it writes, or why this build will not apply it.
    effect: std::result::Result<Vec<(Channel, Index, Correction)>, String>,
    /// What it answers `ILifeSteal` with, if its class is one.
    lifesteal: Option<LifeSteal>,
    /// What it answers `IAutoRecovery` with, if its class is one.
    auto_recovery: Option<AutoRecovery>,
    /// The buff it adds as the fight starts, if its class is a buff item's.
    start_buff: Option<StartBuff>,
    /// The `buffDatas` rows its unit ignores, if its class is an
    /// anti-interference item's.
    ignored_buffs: Vec<u32>,
    /// What it answers `IEnergyShieldSource` with, if its class is one.
    energy_shield: Option<EnergyShield>,
    /// What it answers `IAdvancedEnergyShieldSource` with, if its class is
    /// one.
    carried_shield: Option<CarriedShield>,
    /// The production line it runs, if its class is one.
    production: Option<ProductionLine>,
    /// `ICommonMechDataChangeDataSource.IsImportantUnit`: whether it makes its
    /// unit one its side cannot outlive.
    important: bool,
    /// `IIgnoreBuffDataSouce.IgnoreControllerBeam`, which every
    /// `IgnoreBuffEquipment` answers true: no control beam turns its unit.
    ignores_control_beam: bool,
    /// `ISkillDataChangeDataSource.IsExtraSkillEffect`: whether what it
    /// writes onto a skill reaches its unit's extra skills too
    /// (`SkillDataModifier.AvaliableCheck`).
    reaches_extra_skills: bool,
}

/// One row of the table, with every field the extraction writes.
///
/// A field is written only when it is set, so every one but the id, the name
/// and the targeting defaults to zero or false.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "EquipmentData's serialized flags are independent fields"
)]
struct Row {
    id: i32,
    name: String,
    /// The list of `EquipmentGroupData` the row comes from.
    kind: String,
    mech_type: Vec<i32>,
    #[serde(default)]
    units: Vec<i32>,
    #[serde(default)]
    main_skill_effect: bool,
    #[serde(default)]
    extra_skill_effect: bool,
    #[serde(default)]
    permanent_effect: bool,
    #[serde(default)]
    important_unit: bool,
    #[serde(default)]
    round_duration: i32,
    #[serde(default)]
    life_rate: Option<i64>,
    #[serde(default)]
    damage_rate: Option<i64>,
    #[serde(default)]
    attack_range_rate: Option<i64>,
    #[serde(default)]
    attack_interval_rate: Option<i64>,
    #[serde(default)]
    projectile_life_rate: Option<i64>,
    #[serde(default)]
    attack_range_value: Option<i64>,
    #[serde(default)]
    min_attack_range_value: Option<i64>,
    #[serde(default)]
    attack_interval_value: Option<i64>,
    #[serde(default)]
    splash_range_value: Option<i64>,
    #[serde(default)]
    projectile_speed_value: Option<i64>,
    #[serde(default)]
    speed_value: Option<i64>,
    #[serde(default)]
    exp_rate: Option<i64>,
    #[serde(default)]
    grade_upper_limit: Option<i64>,
    /// `LifestealEquipmentData.lifestealMultiplier`, on a lifesteal row.
    #[serde(default)]
    lifesteal_multiplier: Option<i64>,
    /// `AutoRecoveryEquipmentData`'s three fields, on a repair row.
    #[serde(default)]
    start_time: Option<i64>,
    #[serde(default)]
    recovery_duration: Option<i64>,
    #[serde(default)]
    recovery_life_rate: Option<i64>,
    /// `SplashEquipmentData.range`, on a splash row.
    #[serde(default)]
    splash_range: Option<i64>,
    /// `BuffEquipmentData`'s trigger, on a buff row.
    #[serde(default)]
    buff_trigger: Option<i32>,
    #[serde(default)]
    buff_targets: Vec<i32>,
    #[serde(default)]
    probability: Option<i64>,
    /// The other `BuffEquipmentData` fields a buff row sets.
    #[serde(default)]
    buff_special: Vec<String>,
    /// The `buffDatas` row a buff row adds.
    #[serde(default)]
    buff: Option<BuffBlock>,
    /// The buffs of an anti-interference row's group.
    #[serde(default)]
    ignored_buffs: Vec<u32>,
    /// `EnergyShieldEquipmentData.lifeRate`, on a shield row.
    #[serde(default)]
    shield_life_rate: Option<i64>,
    /// `AdvancedEnergyShieldEquipmentData.radius` and `shieldValue`, on a
    /// barrier row.
    #[serde(default)]
    barrier_radius: Option<i64>,
    #[serde(default)]
    barrier_energy: Option<i64>,
    /// `SupportUnitEquipmentData`'s fields, on a production row.
    #[serde(default)]
    production: Option<ProductionRow>,
}

/// A production row's fields.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProductionRow {
    support_unit_id: u32,
    unit_level: i32,
    max_batch: u32,
    max_alive: u32,
    create_count_per_time: u32,
    /// `startTime`. A production line makes its first batch on the fight's
    /// first tick whatever it is; what it gates is not read.
    #[allow(dead_code, reason = "the first batch comes on the first tick")]
    start_time: i32,
    appear_type: i32,
    max_create_count: u32,
    create_duration: i64,
    unit_life_rate: i64,
    positions: Vec<Offset>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Offset {
    x: i64,
    z: i64,
}

/// A `buffDatas` row a buff item adds, with the fields the simulator reads.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the buff row's flags are independent fields"
)]
struct BuffBlock {
    id: u32,
    name: String,
    duration: i64,
    divide: i32,
    additive: bool,
    debuff: bool,
    invincible: bool,
    disable_technology: bool,
    amplify_damage_rate: i64,
    /// `isClearSelfBuffWhenDisableTech`. A buff is cleared by it only when
    /// its unit's technologies are disabled, which no buff here can do to an
    /// invincible unit; nothing reads it.
    #[allow(
        dead_code,
        reason = "no read buff runs on a unit whose technologies go off"
    )]
    clear_when_technologies_disabled: bool,
    /// The other fields it sets.
    #[serde(default)]
    special: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    schema: String,
    equipment: Vec<Row>,
}

impl EquipmentEffects {
    /// Reads the tracked table.
    ///
    /// # Errors
    ///
    /// Returns an error when the table is not the one this build reads.
    pub(crate) fn load() -> Result<Self> {
        let table: Table = serde_yaml::from_str(DEFAULT_EQUIPMENT_EFFECTS).map_err(|error| {
            Error::new(format!("cannot read the equipment effect table: {error}"))
        })?;
        if table.schema != "mechcore.equipment_effects" {
            return Err(Error::new(format!(
                "equipment effect table declares schema {:?}",
                table.schema
            )));
        }
        let mut equipment = BTreeMap::new();
        for row in table.equipment {
            let id = row.id;
            if equipment.insert(id, Equipment::of(&row)).is_some() {
                return Err(Error::new(format!(
                    "equipment effect table holds equipment {id} twice"
                )));
            }
        }
        Ok(Self { equipment })
    }

    /// What one equipment writes onto the unit wearing it.
    ///
    /// # Errors
    ///
    /// Returns an error naming the equipment when this build cannot apply it,
    /// rather than applying the part of it that it understands.
    pub(crate) fn corrections(&self, id: i32, unit: &UnitConfig) -> Result<Vec<(Channel, Entry)>> {
        let Some(equipment) = self.worn(id, unit)? else {
            return Ok(Vec::new());
        };
        Ok(equipment
            .effect
            .iter()
            .flatten()
            .map(|(channel, index, correction)| {
                (
                    *channel,
                    Entry {
                        index: *index,
                        source: SOURCE,
                        correction: *correction,
                    },
                )
            })
            .collect())
    }

    /// What one equipment answers `ILifeSteal` with on the unit wearing it,
    /// if its class is one.
    ///
    /// # Errors
    ///
    /// Returns the error [`Self::corrections`] does.
    pub(crate) fn lifesteal(&self, id: i32, unit: &UnitConfig) -> Result<Option<LifeSteal>> {
        Ok(self
            .worn(id, unit)?
            .and_then(|equipment| equipment.lifesteal))
    }

    /// What one equipment answers `IAutoRecovery` with on the unit wearing
    /// it, if its class is one.
    ///
    /// # Errors
    ///
    /// Returns the error [`Self::corrections`] does.
    pub(crate) fn auto_recovery(&self, id: i32, unit: &UnitConfig) -> Result<Option<AutoRecovery>> {
        Ok(self
            .worn(id, unit)?
            .and_then(|equipment| equipment.auto_recovery))
    }

    /// The buff one equipment adds to the unit wearing it as the fight
    /// starts, if its class is a buff item's.
    ///
    /// # Errors
    ///
    /// Returns the error [`Self::corrections`] does.
    pub(crate) fn start_buff(&self, id: i32, unit: &UnitConfig) -> Result<Option<StartBuff>> {
        Ok(self
            .worn(id, unit)?
            .and_then(|equipment| equipment.start_buff))
    }

    /// What one equipment answers `IEnergyShieldSource` with on the unit
    /// wearing it, if its class is one.
    ///
    /// # Errors
    ///
    /// Returns the error [`Self::corrections`] does.
    pub(crate) fn energy_shield(&self, id: i32, unit: &UnitConfig) -> Result<Option<EnergyShield>> {
        Ok(self
            .worn(id, unit)?
            .and_then(|equipment| equipment.energy_shield))
    }

    /// The battlefield shield one equipment makes the unit wearing it carry,
    /// if its class is a barrier's.
    ///
    /// # Errors
    ///
    /// Returns the error [`Self::corrections`] does.
    pub(crate) fn carried_shield(
        &self,
        id: i32,
        unit: &UnitConfig,
    ) -> Result<Option<CarriedShield>> {
        Ok(self
            .worn(id, unit)?
            .and_then(|equipment| equipment.carried_shield))
    }

    /// The production line one equipment makes the unit wearing it run, if
    /// its class is one.
    ///
    /// # Errors
    ///
    /// Returns the error [`Self::corrections`] does.
    pub(crate) fn production(&self, id: i32, unit: &UnitConfig) -> Result<Option<ProductionLine>> {
        Ok(self
            .worn(id, unit)?
            .and_then(|equipment| equipment.production.clone()))
    }

    /// Whether one equipment makes the unit wearing it an important unit,
    /// which `MechDataModifer.TryAddCommonData` marks as it writes the row.
    ///
    /// # Errors
    ///
    /// Returns the error [`Self::corrections`] does.
    pub(crate) fn important(&self, id: i32, unit: &UnitConfig) -> Result<bool> {
        Ok(self
            .worn(id, unit)?
            .is_some_and(|equipment| equipment.important))
    }

    /// The buffs one equipment makes the unit wearing it ignore.
    ///
    /// # Errors
    ///
    /// Returns the error [`Self::corrections`] does.
    pub(crate) fn ignored_buffs(&self, id: i32, unit: &UnitConfig) -> Result<Vec<u32>> {
        Ok(self
            .worn(id, unit)?
            .map(|equipment| equipment.ignored_buffs.clone())
            .unwrap_or_default())
    }

    /// Whether the item keeps every control beam from turning its unit.
    ///
    /// # Errors
    ///
    /// See [`Self::ignored_buffs`].
    pub(crate) fn ignores_control_beam(&self, id: i32, unit: &UnitConfig) -> Result<bool> {
        Ok(self
            .worn(id, unit)?
            .is_some_and(|equipment| equipment.ignores_control_beam))
    }

    /// Whether what one equipment writes onto its unit's skills reaches the
    /// unit's extra skills too.
    pub(crate) fn reaches_extra_skills(&self, id: i32) -> bool {
        self.equipment
            .get(&id)
            .is_some_and(|equipment| equipment.reaches_extra_skills)
    }

    /// One equipment's row, once its effect is known to apply, or nothing
    /// when its targeting does not reach the unit.
    fn worn(&self, id: i32, unit: &UnitConfig) -> Result<Option<&Equipment>> {
        let Some(equipment) = self.equipment.get(&id) else {
            return Err(Error::new(format!(
                "equipment {id} is not in the equipment table"
            )));
        };
        equipment
            .effect
            .as_ref()
            .map_err(|why| Error::new(why.clone()))?;
        if !equipment.targets.reaches(unit)? {
            return Ok(None);
        }
        Ok(Some(equipment))
    }
}

impl Equipment {
    fn of(row: &Row) -> Self {
        let who = format!("equipment {} ({})", row.id, row.name);
        let targets = if row.units.is_empty() && !row.mech_type.is_empty() {
            Targets::of_list(&row.mech_type, &[], &who)
        } else {
            Targets::Refused(format!(
                "{who} targets mech_type {:?} and units {:?}, which this build \
                 does not read",
                row.mech_type, row.units
            ))
        };
        // `LifestealEquipment.GetLifestealMuliplier` answers its row's
        // multiplier, and `Equipment.CanDisable` false.
        let lifesteal = (row.kind == LIFESTEAL).then(|| LifeSteal {
            multiplier_q32: row.lifesteal_multiplier.unwrap_or(0),
            priority: PRIORITY,
            can_disable: false,
        });
        // `AutoRecoveryEquipment` answers its row's three fields, and
        // `Normal` for its state.
        let auto_recovery = (row.kind == AUTO_RECOVERY).then(|| AutoRecovery {
            start_time_q32: row.start_time.unwrap_or(0),
            duration_q32: row.recovery_duration.unwrap_or(0),
            life_rate_q32: row.recovery_life_rate.unwrap_or(0),
            priority: PRIORITY,
            can_disable: false,
        });
        let (effect, start_buff, production) = match (
            corrections_of(row, &who),
            start_buff_of(row, &who),
            production_of(row, &who),
        ) {
            (Ok(corrections), Ok(start_buff), Ok(production)) => {
                (Ok(corrections), start_buff, production)
            }
            (Err(why), _, _) | (_, Err(why), _) | (_, _, Err(why)) => (Err(why), None, None),
        };
        Self {
            targets,
            effect,
            lifesteal,
            auto_recovery,
            start_buff,
            ignored_buffs: row.ignored_buffs.clone(),
            // `EnergyShieldEquipment.GetLifeRate` answers its row's.
            energy_shield: (row.kind == ENERGY_SHIELD).then(|| EnergyShield {
                life_rate_q32: row.shield_life_rate.unwrap_or(0),
                priority: PRIORITY,
                can_disable: false,
            }),
            // `AdvancedEnergyShieldEquipment.GetRadius` and `GetShieldValue`
            // answer its row's.
            production,
            carried_shield: (row.kind == BARRIER).then(|| CarriedShield {
                radius: row.barrier_radius.unwrap_or(0),
                energy: row.barrier_energy.unwrap_or(0),
            }),
            important: row.important_unit,
            ignores_control_beam: row.kind == IGNORE_BUFF,
            reaches_extra_skills: row.extra_skill_effect,
        }
    }
}

/// The buff a buff row adds as the fight starts, or why this build will not
/// apply the row. `BuffCycleController.OnEnterFight` runs a controller whose
/// listener is `FightStart`, and its first `Update` triggers once, since a
/// row with no delay and no interval does not cycle.
fn start_buff_of(row: &Row, who: &str) -> std::result::Result<Option<StartBuff>, String> {
    if row.kind != BUFF {
        return Ok(None);
    }
    let Some(buff) = &row.buff else {
        return Err(format!("{who} names no buff"));
    };
    if row.buff_trigger != Some(FIGHT_START) || row.buff_targets != [MECH_UNIT] {
        return Err(format!(
            "{who} adds its buff on BuffTechListener {:?} to TargetTypes {:?}, and only the \
             fight's start onto the unit itself is read",
            row.buff_trigger, row.buff_targets
        ));
    }
    if row.probability != Some(ONE) {
        return Err(format!(
            "{who} adds its buff with probability {:?}, and only a certain one is read",
            row.probability
        ));
    }
    if !row.buff_special.is_empty() {
        return Err(format!(
            "{who} sets {}, which no mechanism here reads",
            row.buff_special.join(", ")
        ));
    }
    if !buff.special.is_empty() || buff.disable_technology {
        return Err(format!(
            "{who} adds buff {} ({}), which sets {}, and no mechanism here reads it on a \
             unit's own buff",
            buff.id,
            buff.name,
            if buff.disable_technology {
                "disableTechnology".to_owned()
            } else {
                buff.special.join(", ")
            }
        ));
    }
    Ok(Some(StartBuff {
        buff_id: buff.id,
        divide: buff.divide,
        additive: buff.additive,
        duration_q32: buff.duration,
        debuff: buff.debuff,
        invincible: buff.invincible,
        amplify_damage_rate: buff.amplify_damage_rate,
    }))
}

/// The production line a production row runs, or why this build will not
/// apply the row: one that makes its units anywhere but at its offsets, at a
/// level of its own, with a life rate or with a cap on all it makes.
fn production_of(row: &Row, who: &str) -> std::result::Result<Option<ProductionLine>, String> {
    if row.kind != PRODUCTION {
        return Ok(None);
    }
    let Some(line) = &row.production else {
        return Err(format!("{who} names no production"));
    };
    let unread = [
        (line.appear_type != AT_OFFSETS, "an appearType other than 5"),
        (line.unit_level != 0, "a unitLevel"),
        (line.max_create_count != 0, "a maxCreateCount"),
        (line.unit_life_rate != 0, "a unitLifeChangerate"),
        (line.positions.is_empty(), "no positions"),
    ];
    if let Some((_, what)) = unread.iter().find(|(set, _)| *set) {
        return Err(format!(
            "{who} runs a production line with {what}, which is not read"
        ));
    }
    Ok(Some(ProductionLine {
        unit_type_id: line.support_unit_id,
        max_batch: line.max_batch,
        max_alive: line.max_alive,
        per_time: line.create_count_per_time,
        interval_q32: line.create_duration,
        offsets: line
            .positions
            .iter()
            .map(|offset| (offset.x, offset.z))
            .collect(),
        // `SupportUnitCreator.APPEAR_DURATION` of `appearType` 5.
        appear_q32: 1 << 32,
        parent_level: false,
        body_frame: false,
        gated: false,
    }))
}

/// What a row writes, or why this build will not apply it.
///
/// A row reaches a unit's main skill only through `mainSkillEffect`; what
/// it writes reaches an extra skill through `extraSkillEffect`, which
/// [`EquipmentEffects::reaches_extra_skills`] answers.
fn corrections_of(
    row: &Row,
    who: &str,
) -> std::result::Result<Vec<(Channel, Index, Correction)>, String> {
    if !APPLIED.contains(&row.kind.as_str()) {
        return Err(format!(
            "{who} comes from EquipmentGroupData's {} list, which no mechanism here reads",
            row.kind
        ));
    }
    if !row.main_skill_effect {
        return Err(format!(
            "{who} leaves the main skill out, and no other skill is simulated"
        ));
    }
    let lifetimes = [
        (row.round_duration != 0, "round_duration"),
        // A permanent effect is activated during deployment, before the
        // fight: what an anti-interference item does in it does not change.
        (
            row.permanent_effect && row.kind != IGNORE_BUFF,
            "permanent_effect",
        ),
    ];
    for (set, field) in lifetimes {
        if set {
            return Err(format!(
                "{who} sets {field}, whose effect is not established"
            ));
        }
    }
    let unsupported = [
        (
            row.min_attack_range_value,
            "min_attack_range_value",
            VALUE_ELSEWHERE,
        ),
        (
            row.projectile_speed_value,
            "projectile_speed_value",
            PROJECTILE,
        ),
        (row.projectile_life_rate, "projectile_life_rate", PROJECTILE),
        (row.exp_rate, "exp_rate", VALUE_ELSEWHERE),
        (row.grade_upper_limit, "grade_upper_limit", VALUE_ELSEWHERE),
    ];
    for (value, field, why) in unsupported {
        if value.is_some_and(|value| value != 0) {
            return Err(format!("{who} writes {field}, and {why}"));
        }
    }
    let mut corrections = effects::corrections(Fields {
        life_rate: row.life_rate,
        damage_rate: row.damage_rate,
        // The table has no such column.
        damage_rate_by_kill_count: None,
        attack_range_rate: row.attack_range_rate,
        attack_interval_rate: row.attack_interval_rate,
        attack_range_value: row.attack_range_value,
        attack_interval_value: row.attack_interval_value,
        splash_range_value: row.splash_range_value,
        speed_value: row.speed_value,
        damage_reduce_rate_base: None,
        projectile_speed_value: None,
    });
    // `SplashEquipment.AddData`'s second write, `AddSkillData` of its range,
    // which lands where a splash value does.
    if row.kind == SPLASH {
        corrections.extend(effects::corrections(Fields {
            splash_range_value: row.splash_range,
            ..Fields::default()
        }));
    }
    Ok(corrections)
}

#[cfg(test)]
mod tests {
    use super::EquipmentEffects;
    use crate::{
        data::{Channel, Stats},
        modifier::OfficerEffects,
        rules::{UnitConfig, UnitConfigs, UnitDomain},
    };

    const LASER_SIGHTS: i32 = 13_030_001;
    const HEAVY_ARMOR: i32 = 13_030_002;
    const IMPROVED_FIREPOWER: i32 = 13_030_003;
    const DOMINION_CORE: i32 = 13_030_010;
    const RAPID_LOADER: i32 = 13_030_011;
    const DEPLOYMENT_MODULE: i32 = 13_040_001;
    const ABSORPTION_MODULE: i32 = 1_309_001;
    const NANO_REPAIR_KIT: i32 = 13_020_001;
    const PHOTON_COATING: i32 = 1_305_003;
    const CHARGED_AMMO: i32 = 1_305_001;
    const ANTI_INTERFERENCE_MODULE: i32 = 1_308_001;
    const PORTABLE_SHIELD: i32 = 13_010_001;
    const BARRIER_ITEM: i32 = 1_307_001;
    const TANK_PRODUCTION_LINE: i32 = 1_306_001;
    const ADVANCED_DEFENSIVE_TACTICS: i32 = 20001;
    const ADVANCED_OFFENSIVE_TACTICS: i32 = 20002;

    fn unit(name: &str) -> UnitConfig {
        UnitConfigs::load().unwrap().get(name).unwrap().clone()
    }

    /// An equipment's rate sums with an officer's in one channel:
    /// `tests/equipment/` recorded 2838 and 3325 of life, 3842 and 4541 of
    /// damage, and 160 m of range.
    #[test]
    fn an_equipment_sums_with_an_officer_in_one_channel() {
        let marksman = unit("marksman");
        let equipment = EquipmentEffects::load().unwrap();
        let officers = OfficerEffects::load().unwrap();
        let resolve = |corrections: &[_]| Stats::corrected(&marksman, 1, corrections).unwrap();

        let mut life = equipment.corrections(HEAVY_ARMOR, &marksman).unwrap();
        assert_eq!(life[0].0, Channel::Unit);
        assert_eq!(resolve(&life).max_life(), 2838);
        life.extend(
            officers
                .corrections(&[ADVANCED_DEFENSIVE_TACTICS], &marksman)
                .unwrap(),
        );
        assert_eq!(resolve(&life).max_life(), 3325);

        let mut damage = equipment
            .corrections(IMPROVED_FIREPOWER, &marksman)
            .unwrap();
        assert_eq!(damage[0].0, Channel::Skill);
        assert_eq!(
            resolve(&damage).attack_damage_against(UnitDomain::Ground),
            3842
        );
        damage.extend(
            officers
                .corrections(&[ADVANCED_OFFENSIVE_TACTICS], &marksman)
                .unwrap(),
        );
        assert_eq!(
            resolve(&damage).attack_damage_against(UnitDomain::Ground),
            4541
        );

        let range = equipment.corrections(LASER_SIGHTS, &marksman).unwrap();
        assert_eq!(
            resolve(&range).attack_range_against(UnitDomain::Ground),
            160_000
        );
    }

    /// Laser Sights is a Ranged row, and reaches a melee unit as nothing.
    #[test]
    fn a_ranged_equipment_writes_nothing_onto_a_melee_unit() {
        let equipment = EquipmentEffects::load().unwrap();
        assert!(
            equipment
                .corrections(LASER_SIGHTS, &unit("rhino"))
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            equipment
                .corrections(LASER_SIGHTS, &unit("steel_ball"))
                .unwrap()
                .len(),
            1
        );
    }

    /// The Deployment Module's class does nothing in a fight, and its row
    /// writes no number: it is worn and changes nothing.
    #[test]
    fn a_kind_that_does_nothing_more_is_applied_for_its_numbers() {
        let equipment = EquipmentEffects::load().unwrap();
        assert!(
            equipment
                .corrections(DEPLOYMENT_MODULE, &unit("marksman"))
                .unwrap()
                .is_empty()
        );
    }

    /// Absorption Module writes its row's life rate, as any class does, and
    /// hands its unit an `ILifeSteal` of 0.9 at an equipment's priority.
    #[test]
    fn a_lifesteal_item_writes_its_numbers_and_hands_a_source() {
        let equipment = EquipmentEffects::load().unwrap();
        let marksman = unit("marksman");
        let written = equipment.corrections(ABSORPTION_MODULE, &marksman).unwrap();
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].0, Channel::Unit);
        let lifesteal = equipment
            .lifesteal(ABSORPTION_MODULE, &marksman)
            .unwrap()
            .unwrap();
        assert_eq!(lifesteal.multiplier_q32, 3_865_470_566);
        assert_eq!(lifesteal.priority, 1);
        assert!(!lifesteal.can_disable);
        assert_eq!(equipment.lifesteal(HEAVY_ARMOR, &marksman).unwrap(), None);
    }

    /// Nano Repair Kit hands its unit an `IAutoRecovery` of its row's
    /// numbers at an equipment's priority, and writes no number.
    #[test]
    fn a_repair_item_hands_a_source() {
        let equipment = EquipmentEffects::load().unwrap();
        let marksman = unit("marksman");
        assert!(
            equipment
                .corrections(NANO_REPAIR_KIT, &marksman)
                .unwrap()
                .is_empty()
        );
        let repair = equipment
            .auto_recovery(NANO_REPAIR_KIT, &marksman)
            .unwrap()
            .unwrap();
        assert_eq!(
            (
                repair.start_time_q32,
                repair.duration_q32,
                repair.life_rate_q32
            ),
            (0, 429_496_729, 19_327_352)
        );
        assert_eq!(repair.priority, 1);
        assert_eq!(
            equipment
                .auto_recovery(ABSORPTION_MODULE, &marksman)
                .unwrap(),
            None
        );
    }

    /// Photon Coating adds buff 4000 to its unit as the fight starts; a buff
    /// item that adds its buff on a hit, Charged Ammo, is refused by name.
    #[test]
    fn a_buff_item_adds_its_buff_as_the_fight_starts() {
        let equipment = EquipmentEffects::load().unwrap();
        let marksman = unit("marksman");
        let buff = equipment
            .start_buff(PHOTON_COATING, &marksman)
            .unwrap()
            .unwrap();
        assert_eq!(buff.buff_id, 4000);
        assert!(buff.invincible && !buff.debuff);
        assert_eq!(buff.amplify_damage_rate, -1_288_490_188);
        let refused = equipment
            .corrections(CHARGED_AMMO, &marksman)
            .unwrap_err()
            .to_string();
        assert!(refused.contains("BuffTechListener"), "{refused}");
    }

    /// Anti-Interference Module, a permanent effect, makes its unit ignore
    /// its group's buffs: every tower loss's and the Electromagnetic
    /// Impact's among them.
    #[test]
    fn an_anti_interference_item_names_the_buffs_its_unit_ignores() {
        let equipment = EquipmentEffects::load().unwrap();
        let rhino = unit("rhino");
        assert!(
            equipment
                .corrections(ANTI_INTERFERENCE_MODULE, &rhino)
                .unwrap()
                .is_empty()
        );
        let ignored = equipment
            .ignored_buffs(ANTI_INTERFERENCE_MODULE, &rhino)
            .unwrap();
        for buff in [1, 2, 3, 4, 5, 200_001] {
            assert!(ignored.contains(&buff), "{ignored:?}");
        }
    }

    /// Portable Shield hands its unit a shield of its row's rate, 1, at an
    /// equipment's priority.
    #[test]
    fn a_shield_item_hands_a_source() {
        let equipment = EquipmentEffects::load().unwrap();
        let marksman = unit("marksman");
        let shield = equipment
            .energy_shield(PORTABLE_SHIELD, &marksman)
            .unwrap()
            .unwrap();
        assert_eq!(shield.life_rate_q32, 1 << 32);
        assert_eq!(shield.priority, 1);
    }

    /// Barrier reaches huge ground units alone, `[7, 2]` both holding: a
    /// Fortress carries its shield, a Raiden, huge but flying, and a
    /// Marksman, on the ground but medium, carry none.
    #[test]
    fn a_barrier_reaches_huge_ground_units() {
        let equipment = EquipmentEffects::load().unwrap();
        let carried = equipment
            .carried_shield(BARRIER_ITEM, &unit("fortress"))
            .unwrap()
            .unwrap();
        assert_eq!((carried.radius, carried.energy), (65, 60_000));
        for other in ["raiden", "marksman"] {
            assert_eq!(
                equipment
                    .carried_shield(BARRIER_ITEM, &unit(other))
                    .unwrap(),
                None
            );
        }
    }

    /// Tank Production Line hands the Fortress a line of two Tanks every 13 s,
    /// seven batches at most and twenty alive, at its two offsets.
    #[test]
    fn a_production_line_hands_its_unit_a_line() {
        let equipment = EquipmentEffects::load().unwrap();
        let fortress = unit("fortress");
        let line = equipment
            .production(TANK_PRODUCTION_LINE, &fortress)
            .unwrap()
            .unwrap();
        assert_eq!(line.unit_type_id, 13);
        assert_eq!((line.max_batch, line.max_alive, line.per_time), (7, 20, 2));
        assert_eq!(line.interval_q32, 13 << 32);
        assert_eq!(line.offsets, [(24 << 32, 15 << 32), (-24 << 32, 15 << 32)]);
        assert_eq!(equipment.production(HEAVY_ARMOR, &fortress).unwrap(), None);
    }

    /// Dominion Core makes its unit an important one and writes its life and
    /// damage rates as an ordinary row does.
    #[test]
    fn an_important_item_marks_its_unit_and_writes_its_numbers() {
        let equipment = EquipmentEffects::load().unwrap();
        let marksman = unit("marksman");
        assert!(equipment.important(DOMINION_CORE, &marksman).unwrap());
        assert_eq!(
            equipment
                .corrections(DOMINION_CORE, &marksman)
                .unwrap()
                .len(),
            2
        );
        assert!(!equipment.important(HEAVY_ARMOR, &marksman).unwrap());
    }

    #[test]
    fn an_equipment_this_build_cannot_apply_is_refused_by_name() {
        let equipment = EquipmentEffects::load().unwrap();
        let marksman = unit("marksman");
        let refused = equipment
            .corrections(RAPID_LOADER, &marksman)
            .unwrap_err()
            .to_string();
        assert!(refused.contains(&RAPID_LOADER.to_string()), "{refused}");
        assert!(refused.contains("round_duration"), "{refused}");
    }
}
