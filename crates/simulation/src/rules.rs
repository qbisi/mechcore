use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

const DEFAULT_UNITS: [&str; 34] = [
    include_str!("../../../config/units/marksman.yaml"),
    include_str!("../../../config/units/rhino.yaml"),
    include_str!("../../../config/units/wasp.yaml"),
    include_str!("../../../config/units/mustang.yaml"),
    include_str!("../../../config/units/steel_ball.yaml"),
    include_str!("../../../config/units/fang.yaml"),
    include_str!("../../../config/units/crawler.yaml"),
    include_str!("../../../config/units/stormcaller.yaml"),
    include_str!("../../../config/units/sledgehammer.yaml"),
    include_str!("../../../config/units/hacker.yaml"),
    include_str!("../../../config/units/arclight.yaml"),
    include_str!("../../../config/units/phoenix.yaml"),
    include_str!("../../../config/units/wraith.yaml"),
    include_str!("../../../config/units/scorpion.yaml"),
    include_str!("../../../config/units/fire_badger.yaml"),
    include_str!("../../../config/units/sabertooth.yaml"),
    include_str!("../../../config/units/typhoon.yaml"),
    include_str!("../../../config/units/tarantula.yaml"),
    include_str!("../../../config/units/phantom_ray.yaml"),
    include_str!("../../../config/units/farseer.yaml"),
    include_str!("../../../config/units/hound.yaml"),
    include_str!("../../../config/units/void_eye.yaml"),
    include_str!("../../../config/units/vortex.yaml"),
    include_str!("../../../config/units/fortress.yaml"),
    include_str!("../../../config/units/vulcan.yaml"),
    include_str!("../../../config/units/melting_point.yaml"),
    include_str!("../../../config/units/overlord.yaml"),
    include_str!("../../../config/units/raiden.yaml"),
    include_str!("../../../config/units/centurion.yaml"),
    include_str!("../../../config/units/sandworm.yaml"),
    include_str!("../../../config/units/mountain.yaml"),
    include_str!("../../../config/units/war_factory.yaml"),
    include_str!("../../../config/units/abyss.yaml"),
    include_str!("../../../config/units/spider_mine.yaml"),
];
const DEFAULT_TOWERS: &str = include_str!("../../../config/towers.yaml");
const DEFAULT_MAPS: &str = include_str!("../../../config/maps.yaml");

/// The integer form of [`SPACE_UNITS_PER_METER`], for whole-meter checks on
/// values that have already been quantized.
pub(crate) const SPACE_UNITS_PER_METER_SCALE: i64 = 1_000;
#[allow(
    clippy::cast_precision_loss,
    reason = "the scale is a small power of ten and is exact in f64"
)]
const SPACE_UNITS_PER_METER: f64 = SPACE_UNITS_PER_METER_SCALE as f64;
const Q32_UNITS_PER_ONE: f64 = 4_294_967_296.0;
/// The integer form of [`TIME_UNITS_PER_SECOND`].
pub(crate) const TIME_UNITS_PER_SECOND_SCALE: i64 = 2_000;
#[allow(clippy::cast_precision_loss, reason = "2000 is exact in f64")]
const TIME_UNITS_PER_SECOND: f64 = TIME_UNITS_PER_SECOND_SCALE as f64;
const MILLIDEGREES_PER_DEGREE: f64 = 1_000.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UnitConfig {
    schema: String,
    pub(crate) type_name: String,
    pub(crate) unit_type_id: u32,
    /// `MechData.mainSkillID`: its main skill's ID, which orders it among the
    /// unit's skills (`SkillManager.SortSkills`).
    pub(crate) main_skill: i32,
    pub(crate) formation: FormationConfig,
    pub(crate) domain: UnitDomain,
    /// `MechData.mechType`, a `UnitType`.
    pub(crate) size: UnitSize,
    pub(crate) max_life: i64,
    pub(crate) collision_radius: f64,
    pub(crate) move_speed: f64,
    pub(crate) rotate_speed: f64,
    pub(crate) has_body: bool,
    /// `FightMech.isFreeMove`, which `MechData.PreProcess` sets for a few
    /// ids: such a unit moves at its full speed whichever way it faces.
    #[serde(default)]
    pub(crate) free_move: bool,
    pub(crate) independent_aim: Option<bool>,
    /// `MechData.isEnableMechSearchTarget`: the unit runs a
    /// `MechSearchTargetController` of its own, whose lock its motion follows
    /// and no skill hands it.
    #[serde(default)]
    pub(crate) mech_search: bool,
    pub(crate) rvo: RvoConfig,
    /// `UndergroundMoveAbility`, which `MoveAbility.Create` makes for a unit
    /// whose `moveType` is `Underground`: it burrows to move and surfaces to
    /// attack.
    #[serde(default)]
    pub(crate) underground: Option<UndergroundConfig>,
    pub(crate) attack: AttackConfig,
    /// The skill each extra weapon technology the unit may research adds
    /// beside its main skill (`ExtraWeaponTech`, `ExtraSkillSystem.AddMech`),
    /// where its shape is one this file can state.
    #[serde(default)]
    pub(crate) extra_weapons: Vec<ExtraWeaponConfig>,
    /// A main skill that is an explosion (`FightExplosionSkill`): the unit's
    /// blow is its own death, and its death strikes about it.
    #[serde(default)]
    pub(crate) explosion: Option<ExplosionConfig>,
}

/// A support skill's production line (`SupportUnitCreator`): the unit it
/// makes at its owner's level (`DynamicMechLevel.Parent`), how many batches
/// and how many alive at once (none for no bound), how many a batch makes and
/// the seconds between two, the seconds each takes to appear (`productTime`),
/// and the offsets, metres right and forward of what `frame` names, each make
/// stands at.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TechnologyProduction {
    pub(crate) unit_type_id: u32,
    pub(crate) level: ProductionLevel,
    pub(crate) max_batch: u32,
    pub(crate) max_alive: u32,
    pub(crate) per_time: u32,
    pub(crate) interval: f64,
    pub(crate) appear: f64,
    pub(crate) frame: ProductionFrame,
    pub(crate) offsets: Vec<ProductionOffset>,
}

/// `DynamicMechLevel`: the level a make takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProductionLevel {
    Parent,
}

/// `SupportUnitPositionSpace`: what a make's offset turns with, its owner's
/// root or its owner's body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProductionFrame {
    Parent,
    ParentBody,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProductionOffset {
    pub(crate) x: f64,
    pub(crate) y: f64,
}

/// An `ExtraWeaponTechnologyData` row and the skill its `skillID` names.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExtraWeaponConfig {
    pub(crate) technology: i32,
    pub(crate) skill: i32,
    /// `ExtraWeaponTechnologyData.useMainSkillRange`: the skill reaches as far
    /// as the unit's main skill does (`FightSkill.GetAttackRange`).
    pub(crate) use_main_skill_range: bool,
    /// `SkillData.damage`: a skill with no damage rate deals its own, one
    /// entry for each unit level.
    pub(crate) damage_by_level: Vec<i64>,
    /// `SkillData.damageRate`: a skill with one deals that share of its
    /// unit's base damage at its level.
    #[serde(default)]
    pub(crate) damage_rate: f64,
    /// `SkillData.ignoreEquipmentEffect`: no equipment writes onto the
    /// skill (`SkillDataModifier.AvaliableCheck`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) ignore_equipment: bool,
    /// The fire its hit leaves, for a row whose `rangeItemType` is a fire.
    #[serde(default)]
    pub(crate) fire: Option<ExtraWeaponFire>,
    /// The oil its hit leaves, for a row whose `rangeItemType` is an oil.
    #[serde(default)]
    pub(crate) oil: Option<ExtraWeaponOil>,
    /// The `buffDatas` row its `buffID` names: the buff its hit writes on
    /// what it struck, and its oil on what stands in it.
    #[serde(default)]
    pub(crate) buff: Option<BuffConfig>,
    /// A support skill's production line: the skill starts as a batch is
    /// due, and lets the line make it.
    #[serde(default)]
    pub(crate) production: Option<TechnologyProduction>,
    /// A permanent preemptive skill: locked until its condition holds, then
    /// in the main skill's place.
    #[serde(default)]
    pub(crate) preemptive: Option<PermanentPreemptive>,
    /// An explosion skill: the effect its unit's death has.
    #[serde(default)]
    pub(crate) explosion: Option<ExplosionConfig>,
    pub(crate) attack: AttackConfig,
}

/// `SkillData.isPreemptivePermanent`, of `permanentPreemptiveActiveConditionType`
/// life: the skill activates once its unit's life is no more than
/// `life_below` of its maximum, and writes its buff on the unit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PermanentPreemptive {
    pub(crate) life_below: f64,
    pub(crate) buff: BuffConfig,
}

/// A `buffDatas` row a technology's skill writes: a permanent preemptive
/// skill's `permanentPreemptiveActiveBuffID`, an extra weapon's `buffID`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the buff row's flags are independent fields"
)]
pub(crate) struct BuffConfig {
    pub(crate) id: u32,
    /// Seconds.
    pub(crate) duration: f64,
    pub(crate) divide: i32,
    pub(crate) additive: bool,
    pub(crate) debuff: bool,
    pub(crate) invincible: bool,
    /// `speedChangeValue`, whole metres a second.
    pub(crate) move_speed_value: i64,
    /// `speedChangeRate`, an `FPoint` raw rate.
    pub(crate) move_speed_rate: i64,
    /// `amplifyDamageRate`, an `FPoint` raw rate on the damage the unit
    /// takes.
    pub(crate) amplify_damage_rate: i64,
    /// `attackRangeChangeValue`, whole metres on the main skill's range.
    pub(crate) attack_range_value: i64,
    /// `currentLifeDisposableChangeRate`, an `FPoint` raw rate of the unit's
    /// life, taken once as the buff is written.
    pub(crate) current_life_rate: i64,
}

/// An `ExplosionSkillData` as `IDeadExplosive`: its unit's death deals what
/// `damage` names times `damage_multiplier`, within the skill's splash, and
/// may leave a fire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExplosionConfig {
    pub(crate) damage: ExplosionDamage,
    pub(crate) damage_multiplier: f64,
    /// `enableFriendlyFire`: the explosion strikes its own side too.
    pub(crate) friendly_fire: bool,
    #[serde(default)]
    pub(crate) dead_fire: Option<DeadFire>,
}

/// `ExplosiveDamageCondition`: the skill's attack damage, or the life its
/// unit had before it took its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExplosionDamage {
    Attack,
    CurrentLife,
}

/// `deadRangeItemLifeTime` in seconds and `deadRangeItemRange` in metres.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeadFire {
    pub(crate) life_time: f64,
    pub(crate) radius: f64,
}

/// `ExtraWeaponTechnologyData.fireLifeTime`: how long the fire its hit leaves
/// burns, in seconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExtraWeaponFire {
    pub(crate) life_time: Vec<f64>,
}

/// What `ExtraWeaponTech` answers `IRangeItemProvider` and `IFireProvider`
/// with for an oil: `fireLifeTime`, how long the oil burns once a fire
/// reaches it, in seconds. It stands as wide as its skill splashes, for no
/// set time, and one round.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExtraWeaponOil {
    pub(crate) fire_life_time: Vec<f64>,
}

/// The four numbers `UndergroundMoveAbility`'s constructor reads from the
/// unit's `MechData`, in seconds and metres.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UndergroundConfig {
    /// `moveAbilityEnterTime`: how long burrowing takes.
    pub(crate) enter: f64,
    /// `moveAbilityExitTime`: how long surfacing takes.
    pub(crate) exit: f64,
    /// `moveAbilityExitKeepEffectTime`: how long into surfacing it stays hidden.
    pub(crate) exit_keep: f64,
    /// `underGroundExitRange`: the attack range it is given underground.
    pub(crate) attack_range: f64,
}

impl UndergroundConfig {
    pub(crate) fn enter_time_units(&self) -> u64 {
        quantize_u64(self.enter, TIME_UNITS_PER_SECOND)
    }

    pub(crate) fn exit_time_units(&self) -> u64 {
        quantize_u64(self.exit, TIME_UNITS_PER_SECOND)
    }

    pub(crate) fn exit_keep_time_units(&self) -> u64 {
        quantize_u64(self.exit_keep, TIME_UNITS_PER_SECOND)
    }

    pub(crate) fn attack_range(&self) -> i64 {
        quantize_i64(self.attack_range, SPACE_UNITS_PER_METER)
    }

    fn validate(&self) -> Result<()> {
        validate_scaled(
            self.enter,
            TIME_UNITS_PER_SECOND,
            "underground.enter",
            false,
        )?;
        validate_scaled(self.exit, TIME_UNITS_PER_SECOND, "underground.exit", false)?;
        validate_scaled(
            self.exit_keep,
            TIME_UNITS_PER_SECOND,
            "underground.exit_keep",
            true,
        )?;
        validate_scaled(
            self.attack_range,
            SPACE_UNITS_PER_METER,
            "underground.attack_range",
            false,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RvoConfig {
    pub(crate) outer_radius: f64,
    pub(crate) inner_radius: f64,
    pub(crate) size: RvoSize,
    pub(crate) collider_priority: i32,
    pub(crate) priority: f64,
}

impl RvoConfig {
    pub(crate) fn outer_radius(&self) -> i64 {
        quantize_i64(self.outer_radius, SPACE_UNITS_PER_METER)
    }

    pub(crate) fn inner_radius(&self) -> i64 {
        quantize_i64(self.inner_radius, SPACE_UNITS_PER_METER)
    }

    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn priority_q32(&self) -> i64 {
        (self.priority * Q32_UNITS_PER_ONE).trunc() as i64
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RvoSize {
    Xs,
    S,
    M,
    L,
    Xl,
    Xxl,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FormationConfig {
    pub(crate) members: u32,
    pub(crate) slot_size: f64,
    pub(crate) footprint: FormationFootprint,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FormationFootprint {
    pub(crate) width: f64,
    pub(crate) depth: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UnitDomain {
    Ground,
    Air,
}

/// `UnitType`: what a row targeting small, medium or huge units reads.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UnitSize {
    Small,
    Medium,
    Huge,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the skill row's flags are independent fields"
)]
pub(crate) struct AttackConfig {
    pub(crate) base_damage: i64,
    pub(crate) min_range: f64,
    pub(crate) range: f64,
    pub(crate) attack_half_angle: f64,
    pub(crate) targets: AttackTargets,
    pub(crate) lock_target: bool,
    pub(crate) quick_switch_target: bool,
    pub(crate) timing: AttackTiming,
    pub(crate) splash_radius: f64,
    pub(crate) weapons: WeaponTopology,
    /// `SkillData.isMeleeAttack` of the main skill. The fight's melee
    /// branches read it, and `UnitUtility.IsEffectTarget` answers the Melee
    /// and Ranged targeting categories from it.
    pub(crate) melee: bool,
    /// `SkillData.canCrossAdvancedShield` of the main skill: its hits pass
    /// every battlefield shield, as a Crawler's and a Rhino's do.
    pub(crate) crosses_shields: bool,
    /// `SkillData.useDefaultRotationSearchTarget` of the main skill: its
    /// searches are scored from each weapon's default rotation.
    #[serde(default)]
    pub(crate) default_rotation_search: bool,
    /// `SkillData.useSelfSplash`: the splash is measured from the skill's
    /// own unit rather than from what it struck.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) self_splash: bool,
    /// `SkillData.isDiffusion`: the splash grows from where it lands a step
    /// at a time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) diffusion: Option<Diffusion>,
    pub(crate) path: AttackPath,
    /// A skill that fires from a magazine: `SkillData.isLoadingType`, which a
    /// turret's is and no unit this build places reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) magazine: Option<Magazine>,
}

/// `SkillData.diffusionInteval` and `diffusionSpeed`: every `interval`
/// seconds the splash reaches `speed` metres further, until it reaches the
/// skill's whole splash.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Diffusion {
    pub(crate) interval: f64,
    pub(crate) speed: f64,
}

impl Diffusion {
    /// How much further the splash reaches each step, in space units.
    pub(crate) fn step_radius(&self) -> i64 {
        quantize_i64(self.speed, SPACE_UNITS_PER_METER)
    }
}

/// `SkillData.loadingCapacity` and `reloadingTime`: the rounds a skill fires
/// before `SkillReloadingState` takes it, and how long that state lasts.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Magazine {
    pub(crate) capacity: u32,
    pub(crate) reload: f64,
}

impl Magazine {
    pub(crate) fn reload_time_units(&self) -> u64 {
        quantize_u64(self.reload, TIME_UNITS_PER_SECOND)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AttackTargets {
    pub(crate) ground: bool,
    pub(crate) air: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AttackTiming {
    pub(crate) interval: f64,
    pub(crate) interval_offset: f64,
    pub(crate) initial_cooldown: f64,
    pub(crate) prepare: f64,
    pub(crate) attack_point: f64,
    pub(crate) backswing: f64,
    pub(crate) cooling: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WeaponMode {
    Normal,
    Group,
    Standalone,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WeaponTopology {
    pub(crate) mode: WeaponMode,
    /// Each weapon's own index in the build, in the order the skill lists
    /// them. A weapon is fired by its position in this list and named by
    /// its index: a Hound's one weapon is index 2.
    pub(crate) indices: Vec<i32>,
    pub(crate) per_skill: u32,
    pub(crate) fusillade: Option<bool>,
    pub(crate) allow_same_target: Option<bool>,
    pub(crate) rotation_speed: Option<f64>,
    /// `FightWeapon`'s constructor gives each weapon of the unit whose data
    /// is 27 a transform of its own, `RotateType.Fixed` and parented to the
    /// unit's: it stands where the unit stands and turns only when told to
    /// take the unit's rotation.
    #[serde(default)]
    pub(crate) fixed_to_body: bool,
    /// Each weapon's `WeaponData.defaultAngle` and how far it may turn from
    /// it, `rotateAngleLeft` and `rotateAngleRight`, in the order of
    /// `indices`; absent for a unit whose every weapon turns freely from the
    /// unit's own rotation.
    #[serde(default)]
    pub(crate) arcs: Option<Vec<WeaponArc>>,
    /// The skill's `weaponMountNode`: a weapon with an arc turns about the
    /// mech body, the turret, where it is `MechBody`, and about the unit's
    /// root otherwise.
    #[serde(default)]
    pub(crate) mount: WeaponMount,
}

/// `WeaponMountNode`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WeaponMount {
    #[default]
    Default,
    Mech,
    MechBody,
}

/// One weapon's rotation in degrees: where it rests from the unit's rotation,
/// and how far it may turn to either side, a side that is absent turning
/// freely.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WeaponArc {
    pub(crate) default: i32,
    pub(crate) left: Option<i32>,
    pub(crate) right: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum AttackPath {
    Projectile {
        count: u32,
        release_interval: f64,
        speed: f64,
        target_offset_radius: f64,
        evenly_allocate_targets: bool,
        extra_search_range: f64,
        pre_flight_height: f64,
        simulated_motion: bool,
        interceptible: bool,
        max_life: i64,
    },
    Direct,
    /// `SuicideEffect`: the blow takes its own unit's life.
    Suicide,
    /// `FightSupportSkill`: a preemptive skill whose blow does nothing; its
    /// production line makes its units.
    Support,
    /// `FightAroundSkill`, a preemptive skill that starts only where
    /// `target_count` enemies stand within `select_radius` metres of its unit
    /// (`IAroundAttackSkillData`), and strikes about the unit.
    Around {
        select_radius: f64,
        target_count: u32,
    },
    Laser {
        damage_multipliers: Vec<f64>,
    },
    ControlBeam {
        warmup_attack_count: u32,
        warmup_damage_multiplier: f64,
    },
    /// `FightSweepSkill`: a strip of `length` by `width` metres swept across
    /// the target, `damage_times` strikes `damage_interval` apart after
    /// `damage_delay`; `hit_caps` is `unitRadiusList` beside
    /// `maxDamageTimesList`.
    Sweep {
        perpendicular: bool,
        length: u32,
        width: u32,
        sweeps: u32,
        damage_times: u32,
        damage_interval: f64,
        damage_delay: f64,
        hit_caps: Vec<SweepHitCap>,
    },
}

/// One entry of a sweep's `unitRadiusList` with its `maxDamageTimesList`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SweepHitCap {
    pub(crate) radius: f64,
    pub(crate) hits: u32,
}

pub(crate) struct UnitConfigs {
    units: BTreeMap<String, UnitConfig>,
}

pub(crate) struct SimulationConfig {
    pub(crate) game_build: String,
    pub(crate) units: UnitConfigs,
    pub(crate) towers: TowersConfig,
    pub(crate) maps: MapsConfig,
}

/// `config/maps.yaml`: what each standard 1v1 map puts on the board.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MapsConfig {
    schema: String,
    /// The map data each map id plays on; several share one.
    maps: BTreeMap<i32, String>,
    /// Each map data's buildings, in the order `FightBuildingLoader.Load`
    /// creates them.
    map_data: BTreeMap<String, Vec<MapBuilding>>,
}

/// One building a map places, as the fight's movement sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum MapBuilding {
    /// A tower, `config/towers.yaml`'s: only its place in the order is the
    /// map's, and the map is held to stand it where that file does.
    Tower {
        team_id: u32,
        building_type_id: u32,
        x: i64,
        z: i64,
    },
    /// A neutral `FightCrystal` that runs an `RVOControllerFixed`: its centre
    /// and radius, `FPoint` raw, and its collider priority.
    Crystal {
        x: i64,
        z: i64,
        radius: i64,
        collider_priority: i32,
    },
}

/// `config/towers.yaml`: the map's towers, what strengthening one adds and
/// what losing one writes on its side.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TowersConfig {
    schema: String,
    /// The Training Ground's four towers, measured on a capture.
    pub(crate) buildings: Vec<BuildingConfig>,
    /// `buffDatas` 1 to 5, one buff that differs only in duration.
    pub(crate) destroyed_buff: DestroyedBuff,
    /// `towerStrengthenDatas`, with level 0 before them.
    pub(crate) levels: Vec<TowerLevel>,
}

/// The buff a tower's loss writes on its side.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the buff row's flags are independent fields"
)]
pub(crate) struct DestroyedBuff {
    pub(crate) name: String,
    pub(crate) buff_divide: i32,
    pub(crate) additive: bool,
    /// `debuff`: a unit a buff makes invincible does not take it.
    pub(crate) debuff: bool,
    pub(crate) max_additive_stack: i32,
    pub(crate) can_affect_construction: bool,
    /// `isClearSelfBuffWhenDisableTech`. Nothing this simulator places
    /// disables a unit's technologies, so nothing reads it.
    #[allow(dead_code, reason = "no mechanism here disables technologies")]
    pub(crate) clear_when_technologies_disabled: bool,
    pub(crate) move_speed_rate: i64,
    pub(crate) damage_rate: i64,
    pub(crate) amplify_damage_rate: i64,
    /// `canAffectTower`, which build 2.0 added when a tower became a buff
    /// target. This simulator puts no buff on a tower, so nothing reads it.
    #[allow(dead_code, reason = "no mechanism here puts a buff on a tower")]
    #[serde(default)]
    pub(crate) can_affect_tower: bool,
}

/// One strengthen level: the life it adds and the buff its loss writes.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TowerLevel {
    pub(crate) level: u8,
    pub(crate) life: i64,
    /// The `buffDatas` row its loss writes, which the buff's events name.
    pub(crate) buff: u32,
    /// Seconds.
    pub(crate) duration: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BuildingConfig {
    pub(crate) team_id: u32,
    pub(crate) building_type_id: u32,
    pub(crate) position: BuildingPosition,
    pub(crate) life: i64,
    /// The experience its destruction hands out, `towerDefaultDatas.exp`.
    pub(crate) exp: i64,
    radius: f64,
    pub(crate) collision_enabled: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BuildingPosition {
    x: f64,
    z: f64,
}

impl SimulationConfig {
    pub(crate) fn load() -> Result<Self> {
        let towers = parse_towers(DEFAULT_TOWERS.as_bytes(), "embedded tower config")?;
        towers.validate()?;
        let maps: MapsConfig = serde_yaml::from_slice(DEFAULT_MAPS.as_bytes())
            .map_err(|error| Error::new(format!("invalid embedded map config: {error}")))?;
        maps.validate(&towers)?;
        Ok(Self {
            game_build: mechcore_document::game_build().to_owned(),
            units: UnitConfigs::load()?,
            towers,
            maps,
        })
    }
}

impl TowersConfig {
    fn validate(&self) -> Result<()> {
        if self.schema != "mechcore.towers" {
            return Err(Error::new("unsupported tower config type"));
        }
        if self.buildings.is_empty() {
            return Err(Error::new("tower config must contain native building rows"));
        }
        if !self
            .levels
            .iter()
            .enumerate()
            .all(|(index, level)| usize::from(level.level) == index)
        {
            return Err(Error::new("the tower levels are not 0 onwards in order"));
        }
        // `maxAdditiveStack` bounds how many times an additive buff is
        // lengthened. Zero is the tower's row, and no bound is read for it.
        if self.destroyed_buff.max_additive_stack != 0 {
            return Err(Error::new(format!(
                "{} bounds its additive stack, which is not read",
                self.destroyed_buff.name
            )));
        }
        for building in &self.buildings {
            if building.team_id > 1 || building.building_type_id == 0 || building.life <= 0 {
                return Err(Error::new(
                    "a tower building contains invalid identities or life",
                ));
            }
            validate_signed_scaled(building.position.x, SPACE_UNITS_PER_METER, "position.x")?;
            validate_signed_scaled(building.position.z, SPACE_UNITS_PER_METER, "position.z")?;
            validate_scaled(building.radius, SPACE_UNITS_PER_METER, "radius", false)?;
        }
        Ok(())
    }
}

impl MapsConfig {
    /// The buildings map `map_id` places, in the order the game creates them.
    ///
    /// # Errors
    ///
    /// Returns an error naming a map id that is not a standard 1v1 map.
    pub(crate) fn buildings(&self, map_id: i32) -> Result<&[MapBuilding]> {
        let name = self.maps.get(&map_id).ok_or_else(|| {
            Error::new(format!(
                "map_id {map_id} is not a standard 1v1 map; the maps are {}",
                self.maps
                    .keys()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })?;
        Ok(&self.map_data[name])
    }

    /// Every map data a map names exists, stands the towers where
    /// `config/towers.yaml` does, in its order, and gives each crystal a
    /// collider priority an RVO layer can hold.
    fn validate(&self, towers: &TowersConfig) -> Result<()> {
        if self.schema != "mechcore.maps" {
            return Err(Error::new("unsupported map config type"));
        }
        let expected = towers
            .buildings
            .iter()
            .map(|tower| {
                (
                    tower.team_id,
                    tower.building_type_id,
                    tower.x() / SPACE_UNITS_PER_METER_SCALE,
                    tower.z() / SPACE_UNITS_PER_METER_SCALE,
                )
            })
            .collect::<Vec<_>>();
        for (map_id, name) in &self.maps {
            let buildings = self.map_data.get(name).ok_or_else(|| {
                Error::new(format!(
                    "map {map_id} names map data {name}, which is not in the config"
                ))
            })?;
            let placed = buildings
                .iter()
                .filter_map(|building| match *building {
                    MapBuilding::Tower {
                        team_id,
                        building_type_id,
                        x,
                        z,
                    } => Some((team_id, building_type_id, x, z)),
                    MapBuilding::Crystal { .. } => None,
                })
                .collect::<Vec<_>>();
            if placed != expected {
                return Err(Error::new(format!(
                    "map data {name} does not stand config/towers.yaml's towers"
                )));
            }
            if buildings.iter().any(|building| {
                matches!(building, MapBuilding::Crystal { collider_priority, .. }
                    if !(1..=16).contains(collider_priority))
            }) {
                return Err(Error::new(format!(
                    "map data {name} has a crystal whose collider priority no RVO layer holds"
                )));
            }
        }
        Ok(())
    }
}

impl BuildingConfig {
    pub(crate) fn x(&self) -> i64 {
        quantize_i64(self.position.x, SPACE_UNITS_PER_METER)
    }

    pub(crate) fn z(&self) -> i64 {
        quantize_i64(self.position.z, SPACE_UNITS_PER_METER)
    }

    pub(crate) fn radius(&self) -> i64 {
        quantize_i64(self.radius, SPACE_UNITS_PER_METER)
    }
}

impl UnitConfigs {
    pub(crate) fn load() -> Result<Self> {
        let configs = DEFAULT_UNITS
            .iter()
            .map(|text| parse(text.as_bytes(), "embedded unit config"))
            .collect::<Result<Vec<_>>>()?;
        Self::from_configs(configs)
    }

    pub(crate) fn get(&self, type_name: &str) -> Option<&UnitConfig> {
        self.units.get(type_name)
    }

    /// The unit a build's unit id names, `IFightSetting.GetMechData`.
    pub(crate) fn by_type_id(&self, unit_type_id: u32) -> Option<&UnitConfig> {
        self.units
            .values()
            .find(|config| config.unit_type_id == unit_type_id)
    }

    fn from_configs(configs: Vec<UnitConfig>) -> Result<Self> {
        if configs.is_empty() {
            return Err(Error::new("no unit configuration is embedded"));
        }
        let mut ids = BTreeSet::new();
        let mut units = BTreeMap::new();
        for config in configs {
            config.validate()?;
            if !ids.insert(config.unit_type_id)
                || units.insert(config.type_name.clone(), config).is_some()
            {
                return Err(Error::new("unit configs contain duplicate identities"));
            }
        }
        Ok(Self { units })
    }
}

impl UnitConfig {
    fn validate(&self) -> Result<()> {
        if self.schema != "mechcore.unit" {
            return Err(Error::new("unsupported unit config type"));
        }
        if self.type_name.trim().is_empty()
            || self.unit_type_id == 0
            || self.max_life <= 0
            || self.formation.members == 0
            || self.attack.base_damage <= 0
        {
            return Err(Error::new(format!(
                "unit config for {:?} contains invalid values",
                self.type_name
            )));
        }
        validate_scaled(
            self.collision_radius,
            SPACE_UNITS_PER_METER,
            "collision_radius",
            true,
        )?;
        validate_scaled(self.move_speed, SPACE_UNITS_PER_METER, "move_speed", true)?;
        validate_scaled(
            self.rotate_speed,
            MILLIDEGREES_PER_DEGREE,
            "rotate_speed",
            true,
        )?;
        validate_scaled(
            self.formation.slot_size,
            SPACE_UNITS_PER_METER,
            "formation.slot_size",
            false,
        )?;
        let _ = self.formation_slot_size_meters()?;
        validate_scaled(
            self.formation.footprint.width,
            SPACE_UNITS_PER_METER,
            "formation.footprint.width",
            false,
        )?;
        validate_scaled(
            self.formation.footprint.depth,
            SPACE_UNITS_PER_METER,
            "formation.footprint.depth",
            false,
        )?;
        if !self.has_body && self.independent_aim.is_some() {
            return Err(Error::new(
                "independent_aim is not applicable when the unit has no mech body",
            ));
        }
        validate_scaled(
            self.rvo.outer_radius,
            SPACE_UNITS_PER_METER,
            "rvo.outer_radius",
            false,
        )?;
        validate_scaled(
            self.rvo.inner_radius,
            SPACE_UNITS_PER_METER,
            "rvo.inner_radius",
            false,
        )?;
        if self.rvo.inner_radius > self.rvo.outer_radius {
            return Err(Error::new(
                "rvo.inner_radius cannot exceed rvo.outer_radius",
            ));
        }
        if !(1..=10).contains(&self.rvo.collider_priority)
            || !(0.0..=1.0).contains(&self.rvo.priority)
        {
            return Err(Error::new(
                "rvo collider_priority or priority is outside its native range",
            ));
        }
        if let Some(underground) = &self.underground {
            underground.validate()?;
        }
        self.attack.validate()
    }

    /// Whether the kernel has a way to fire this unit's main skill.
    ///
    /// A configuration states every shape the build's main skills take; the
    /// kernel fires some of them. A shape it has no code for is refused here,
    /// naming the unit, rather than reaching a path written for another shape.
    pub(crate) fn fired(&self) -> Result<()> {
        let weapons = &self.attack.weapons;
        let why = match (&self.attack.path, weapons.mode) {
            // A group of one weapon fires one blow whatever its topology
            // says: a Vortex's is a single direct weapon.
            (_, WeaponMode::Group) if weapons.count() == 1 => return Ok(()),
            // The Raiden's blows are the one fusillade recorded, and a
            // Wraith's projectiles the one group that shares out targets.
            (AttackPath::Direct, WeaponMode::Group) if weapons.fusillade == Some(true) => {
                return Ok(());
            }
            (AttackPath::Projectile { .. }, WeaponMode::Group)
                if weapons.fusillade == Some(false) =>
            {
                return Ok(());
            }
            // The Mountain's four guns are each a skill of its own.
            (_, WeaponMode::Normal) | (AttackPath::Projectile { .. }, WeaponMode::Standalone) => {
                return Ok(());
            }
            (_, WeaponMode::Group) if weapons.fusillade == Some(true) => {
                "fires a fusillade of weapons that do not strike"
            }
            (_, WeaponMode::Group) => "groups weapons that fire no projectile",
            (_, WeaponMode::Standalone) => "fires standalone weapons",
        };
        Err(Error::new(format!(
            "unit {:?} {why}, which the kernel does not",
            self.type_name
        )))
    }

    pub(crate) fn collision_radius(&self) -> i64 {
        quantize_i64(self.collision_radius, SPACE_UNITS_PER_METER)
    }

    pub(crate) fn formation_footprint_meters(&self) -> Result<(i64, i64)> {
        fn whole_meters(value: f64, field: &str) -> Result<i64> {
            let raw = quantize_i64(value, SPACE_UNITS_PER_METER);
            let scale = SPACE_UNITS_PER_METER_SCALE;
            if raw.rem_euclid(scale) != 0 {
                return Err(Error::new(format!(
                    "unit config field {field} must use whole meters for layout placement"
                )));
            }
            Ok(raw / scale)
        }

        Ok((
            whole_meters(self.formation.footprint.width, "formation.footprint.width")?,
            whole_meters(self.formation.footprint.depth, "formation.footprint.depth")?,
        ))
    }

    pub(crate) fn formation_slot_size_meters(&self) -> Result<i64> {
        let raw = quantize_i64(self.formation.slot_size, SPACE_UNITS_PER_METER);
        let scale = SPACE_UNITS_PER_METER_SCALE;
        if raw.rem_euclid(scale) != 0 {
            return Err(Error::new(
                "unit config field formation.slot_size must use whole meters for member generation",
            ));
        }
        Ok(raw / scale)
    }

    pub(crate) fn move_speed(&self) -> i64 {
        quantize_i64(self.move_speed, SPACE_UNITS_PER_METER)
    }

    pub(crate) fn rotate_speed_mdeg_per_second(&self) -> i64 {
        quantize_i64(self.rotate_speed, MILLIDEGREES_PER_DEGREE)
    }
}

impl WeaponTopology {
    /// The skill's own weapon rotation speed, where it has one.
    pub(crate) fn rotation_speed_mdeg_per_second(&self) -> Option<i64> {
        self.rotation_speed
            .map(|speed| quantize_i64(speed, MILLIDEGREES_PER_DEGREE))
    }
}

impl AttackConfig {
    #[allow(clippy::too_many_lines)]
    fn validate(&self) -> Result<()> {
        // A support skill sets neither flag, and searches the ground.
        if !self.targets.ground && !self.targets.air && !matches!(self.path, AttackPath::Support) {
            return Err(Error::new("attack must target ground, air, or both"));
        }
        validate_scaled(self.min_range, SPACE_UNITS_PER_METER, "min_range", true)?;
        validate_scaled(self.range, SPACE_UNITS_PER_METER, "range", true)?;
        if self.min_range > self.range {
            return Err(Error::new("attack min_range must not exceed range"));
        }
        validate_scaled(
            self.attack_half_angle,
            MILLIDEGREES_PER_DEGREE,
            "attack_half_angle",
            true,
        )?;
        if self.attack_half_angle > 360.0 {
            return Err(Error::new("attack_half_angle must not exceed 360 degrees"));
        }
        // A suicide blows once, and its row may set no interval.
        validate_scaled(
            self.timing.interval,
            TIME_UNITS_PER_SECOND,
            "timing.interval",
            matches!(self.path, AttackPath::Suicide | AttackPath::Support),
        )?;
        validate_scaled(
            self.timing.interval_offset,
            TIME_UNITS_PER_SECOND,
            "timing.interval_offset",
            true,
        )?;
        for (value, field) in [
            (self.timing.initial_cooldown, "timing.initial_cooldown"),
            (self.timing.prepare, "timing.prepare"),
            (self.timing.attack_point, "timing.attack_point"),
            (self.timing.backswing, "timing.backswing"),
            (self.timing.cooling, "timing.cooling"),
        ] {
            validate_scaled(value, TIME_UNITS_PER_SECOND, field, true)?;
        }
        validate_scaled(
            self.splash_radius,
            SPACE_UNITS_PER_METER,
            "splash_radius",
            true,
        )?;
        self.weapons.validate()?;
        match &self.path {
            AttackPath::Projectile {
                count,
                release_interval,
                speed,
                target_offset_radius,
                extra_search_range,
                pre_flight_height,
                interceptible,
                max_life,
                ..
            } => {
                if *count == 0 || *max_life < 0 || (*interceptible && *max_life == 0) {
                    return Err(Error::new(
                        "projectile path requires a count and valid projectile life",
                    ));
                }
                validate_scaled(
                    *release_interval,
                    TIME_UNITS_PER_SECOND,
                    "path.release_interval",
                    true,
                )?;
                validate_scaled(*speed, SPACE_UNITS_PER_METER, "path.speed", false)?;
                validate_scaled(
                    *target_offset_radius,
                    SPACE_UNITS_PER_METER,
                    "path.target_offset_radius",
                    true,
                )?;
                let target_offset_space =
                    quantize_i64(*target_offset_radius, SPACE_UNITS_PER_METER);
                if target_offset_space != 0 && target_offset_space < 10 {
                    return Err(Error::new(
                        "path.target_offset_radius must be zero or at least one centimeter",
                    ));
                }
                validate_scaled(
                    *extra_search_range,
                    SPACE_UNITS_PER_METER,
                    "path.extra_search_range",
                    true,
                )?;
                validate_scaled(
                    *pre_flight_height,
                    SPACE_UNITS_PER_METER,
                    "path.pre_flight_height",
                    true,
                )
            }
            AttackPath::Direct | AttackPath::Suicide | AttackPath::Support => Ok(()),
            AttackPath::Around {
                select_radius,
                target_count,
            } => {
                if *target_count == 0 {
                    return Err(Error::new("around path requires a target count"));
                }
                validate_scaled(
                    *select_radius,
                    SPACE_UNITS_PER_METER,
                    "path.select_radius",
                    false,
                )
            }
            AttackPath::Laser { damage_multipliers } => {
                if damage_multipliers.is_empty()
                    || damage_multipliers
                        .iter()
                        .any(|value| !value.is_finite() || *value <= 0.0)
                {
                    return Err(Error::new(
                        "laser path requires positive finite damage multipliers",
                    ));
                }
                Ok(())
            }
            AttackPath::Sweep {
                damage_times,
                damage_interval,
                ..
            } => {
                if *damage_times == 0 || !damage_interval.is_finite() || *damage_interval <= 0.0 {
                    return Err(Error::new("sweep path requires positive strikes"));
                }
                Ok(())
            }
            AttackPath::ControlBeam {
                warmup_attack_count,
                warmup_damage_multiplier,
            } => {
                if *warmup_attack_count == 0
                    || !warmup_damage_multiplier.is_finite()
                    || *warmup_damage_multiplier <= 0.0
                {
                    return Err(Error::new(
                        "control beam path requires a positive warmup definition",
                    ));
                }
                Ok(())
            }
        }
    }

    pub(crate) fn min_range(&self) -> i64 {
        quantize_i64(self.min_range, SPACE_UNITS_PER_METER)
    }

    pub(crate) fn range(&self) -> i64 {
        quantize_i64(self.range, SPACE_UNITS_PER_METER)
    }

    pub(crate) fn attack_half_angle_mdeg(&self) -> i64 {
        quantize_i64(self.attack_half_angle, MILLIDEGREES_PER_DEGREE)
    }

    pub(crate) fn splash_radius(&self) -> i64 {
        quantize_i64(self.splash_radius, SPACE_UNITS_PER_METER)
    }

    pub(crate) fn interval_time_units(&self) -> u64 {
        quantize_u64(self.timing.interval, TIME_UNITS_PER_SECOND)
    }

    pub(crate) fn interval_offset_time_units(&self) -> u64 {
        quantize_u64(self.timing.interval_offset, TIME_UNITS_PER_SECOND)
    }

    pub(crate) fn prepare_time_units(&self) -> u64 {
        quantize_u64(self.timing.prepare, TIME_UNITS_PER_SECOND)
    }

    pub(crate) fn attack_point_time_units(&self) -> u64 {
        quantize_u64(self.timing.attack_point, TIME_UNITS_PER_SECOND)
    }

    pub(crate) fn cooling_time_units(&self) -> u64 {
        quantize_u64(self.timing.cooling, TIME_UNITS_PER_SECOND)
    }

    pub(crate) fn backswing_time_units(&self) -> u64 {
        quantize_u64(self.timing.backswing, TIME_UNITS_PER_SECOND)
    }

    pub(crate) fn projectile_speed(&self) -> i64 {
        let AttackPath::Projectile { speed, .. } = self.path else {
            unreachable!("the current kernel validates the projectile path")
        };
        quantize_i64(speed, SPACE_UNITS_PER_METER)
    }

    /// How high a projectile climbs straight up before it flies at its
    /// target, in space units; zero for one that flies at once.
    pub(crate) fn projectile_pre_flight_height(&self) -> i64 {
        let AttackPath::Projectile {
            pre_flight_height, ..
        } = self.path
        else {
            unreachable!("pre-flight height requires the projectile attack path")
        };
        quantize_i64(pre_flight_height, SPACE_UNITS_PER_METER)
    }

    /// Whether an interceptor may take this projectile out of the air.
    pub(crate) fn projectile_interceptible(&self) -> bool {
        let AttackPath::Projectile { interceptible, .. } = self.path else {
            unreachable!("interception requires the projectile attack path")
        };
        interceptible
    }

    pub(crate) fn projectile_life(&self) -> i64 {
        let AttackPath::Projectile { max_life, .. } = self.path else {
            unreachable!("projectile life requires the projectile attack path")
        };
        max_life.max(1)
    }

    pub(crate) fn projectile_count(&self) -> u32 {
        let AttackPath::Projectile { count, .. } = self.path else {
            unreachable!("projectile count requires the projectile attack path")
        };
        count
    }

    pub(crate) fn projectile_release_interval_time_units(&self) -> u64 {
        let AttackPath::Projectile {
            release_interval, ..
        } = self.path
        else {
            unreachable!("projectile interval requires the projectile attack path")
        };
        quantize_u64(release_interval, TIME_UNITS_PER_SECOND)
    }

    pub(crate) fn projectile_target_offset_radius(&self) -> i64 {
        let AttackPath::Projectile {
            target_offset_radius,
            ..
        } = self.path
        else {
            unreachable!("projectile target offset requires the projectile attack path")
        };
        quantize_i64(target_offset_radius, SPACE_UNITS_PER_METER)
    }
}

impl AttackTargets {
    /// `FightCalculator.IsValidTarget` of `FightSkillBase.GetAttackTargetType`:
    /// a skill that attacks no air attacks the ground, whatever its ground
    /// flag says, and one that attacks the air attacks the ground too only
    /// where its ground flag says so. A Spider Mine's support skill, with
    /// neither flag, locks a Rhino.
    pub(crate) const fn accepts(self, domain: UnitDomain) -> bool {
        match domain {
            UnitDomain::Ground => self.ground || !self.air,
            UnitDomain::Air => self.air,
        }
    }
}

impl WeaponTopology {
    /// How many weapons the skill fires.
    pub(crate) fn count(&self) -> u32 {
        u32::try_from(self.indices.len()).expect("a weapon list fits u32")
    }

    /// Whether a grouped row's skills make a group: more than one of them,
    /// `weaponCountPerSkill` weapons each (`FightSkillFactory.PrepareGroupedSkill`
    /// makes none of one skill where the main skill holds none).
    pub(crate) fn makes_group(&self) -> bool {
        self.mode == WeaponMode::Group && self.count() / self.per_skill > 1
    }

    /// The build's index of the weapon at this position.
    pub(crate) fn index(&self, position: usize) -> i32 {
        self.indices[position]
    }

    fn validate(&self) -> Result<()> {
        let count = self.count();
        if count == 0 || self.per_skill == 0 || self.per_skill > count {
            return Err(Error::new("weapon topology contains invalid counts"));
        }
        let mut distinct = self.indices.clone();
        distinct.sort_unstable();
        distinct.dedup();
        if distinct.len() != self.indices.len() || distinct[0] < 0 {
            return Err(Error::new(
                "weapon indices must be distinct and non-negative",
            ));
        }
        let group_fields_are_complete =
            self.fusillade.is_some() && self.allow_same_target.is_some();
        let has_any_group_field = self.fusillade.is_some() || self.allow_same_target.is_some();
        if (self.mode == WeaponMode::Group && !group_fields_are_complete)
            || (self.mode != WeaponMode::Group && has_any_group_field)
        {
            return Err(Error::new(
                "group weapon topology requires fusillade and allow_same_target only for group mode",
            ));
        }
        if let Some(rotation_speed) = self.rotation_speed {
            validate_scaled(
                rotation_speed,
                MILLIDEGREES_PER_DEGREE,
                "weapons.rotation_speed",
                false,
            )?;
        }
        Ok(())
    }
}

fn validate_scaled(value: f64, scale: f64, field: &str, allow_zero: bool) -> Result<()> {
    let scaled = value * scale;
    if !value.is_finite()
        || value < 0.0
        || (!allow_zero && value == 0.0)
        || scaled > 9_000_000_000_000_000.0
        || (scaled - scaled.round()).abs() > 1.0e-9
    {
        return Err(Error::new(format!(
            "unit config field {field} cannot be represented exactly by the kernel"
        )));
    }
    Ok(())
}

fn validate_signed_scaled(value: f64, scale: f64, field: &str) -> Result<()> {
    let scaled = value * scale;
    if !value.is_finite()
        || scaled.abs() > 9_000_000_000_000_000.0
        || (scaled - scaled.round()).abs() > 1.0e-9
    {
        return Err(Error::new(format!(
            "training-ground config field {field} cannot be represented exactly by the kernel"
        )));
    }
    Ok(())
}

#[allow(clippy::cast_possible_truncation)]
fn quantize_i64(value: f64, scale: f64) -> i64 {
    (value * scale).round() as i64
}

/// Metres, or seconds, as an `FPoint`'s Q32.32 raw value: exact for any
/// value a table writes, which `config/` states in whole or few decimals.
pub(crate) fn metres_q32(value: f64) -> i64 {
    quantize_i64(value, 4_294_967_296.0)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn quantize_u64(value: f64, scale: f64) -> u64 {
    (value * scale).round() as u64
}

fn parse(bytes: &[u8], source: &str) -> Result<UnitConfig> {
    serde_yaml::from_slice(bytes)
        .map_err(|error| Error::new(format!("invalid unit config {source}: {error}")))
}

fn parse_towers(bytes: &[u8], source: &str) -> Result<TowersConfig> {
    serde_yaml::from_slice(bytes)
        .map_err(|error| Error::new(format!("invalid tower config {source}: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn si_values_quantize_to_the_internal_integer_grid() {
        let config = SimulationConfig::load().unwrap();
        assert_eq!(config.game_build, mechcore_document::game_build());
        assert_eq!(config.units.units.len(), 34);
        let arclight = config.units.get("arclight").unwrap();
        assert_eq!(arclight.collision_radius(), 9_000);
        assert_eq!(arclight.move_speed(), 7_000);
        assert_eq!(arclight.attack.interval_time_units(), 1_800);
        assert_eq!(arclight.attack.interval_offset_time_units(), 600);
        assert_eq!(arclight.independent_aim, Some(false));
        assert_eq!(
            config.units.get("marksman").unwrap().independent_aim,
            Some(false)
        );
        assert_eq!(config.towers.buildings.len(), 4);
        assert_eq!(config.towers.buildings[0].x(), -140_000);
        assert_eq!(config.towers.buildings[0].radius(), 10_000);
    }

    #[test]
    fn formation_slot_size_is_required_whole_meter_data() {
        let source = include_str!("../../../config/units/marksman.yaml");
        let missing = source.replace("  slot_size: 20\n", "");
        assert!(parse(missing.as_bytes(), "missing slot size").is_err());

        let fractional = source.replace("  slot_size: 20\n", "  slot_size: 20.5\n");
        let config = parse(fractional.as_bytes(), "fractional slot size").unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn rvo_profile_is_required_and_inner_radius_cannot_exceed_outer_radius() {
        let source = include_str!("../../../config/units/marksman.yaml");
        let missing = source.replace(
            "rvo: {outer_radius: 8, inner_radius: 4, size: l, collider_priority: 6, priority: 0.006}\n",
            "",
        );
        assert!(parse(missing.as_bytes(), "missing RVO profile").is_err());

        let mut invalid = parse(source.as_bytes(), "invalid RVO radii").unwrap();
        invalid.rvo.inner_radius = invalid.rvo.outer_radius + 0.5;
        assert!(invalid.validate().is_err());

        let mut invalid = parse(source.as_bytes(), "invalid RVO priority").unwrap();
        invalid.rvo.collider_priority = 11;
        assert!(invalid.validate().is_err());
        invalid.rvo.collider_priority = 10;
        invalid.rvo.priority = 1.000_001;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn readable_rvo_priorities_preserve_native_q32_values() {
        let config = SimulationConfig::load().unwrap();
        let expected = [
            ("marksman", 25_769_803),
            ("rhino", 4_294_967_296),
            ("wasp", 17_179_869),
            ("mustang", 8_589_934),
            ("steel_ball", 21_474_836),
            ("fang", 4_294_967),
            ("crawler", 8_589_934),
            ("stormcaller", 17_179_869),
            ("sledgehammer", 858_993_459),
            ("hacker", 30_064_771),
            ("arclight", 2_147_483_648),
            ("phoenix", 25_769_803),
            ("wraith", 3_435_973),
            ("scorpion", 34_359_738),
            ("fire_badger", 1_717_986_918),
            ("sabertooth", 1_288_490_188),
            ("typhoon", 429_496_729),
            ("tarantula", 2_147_483_648),
            ("phantom_ray", 25_769_803),
            ("farseer", 858_993_459),
            ("hound", 21_474_836),
            ("void_eye", 21_474_836),
            ("vortex", 2_147_483_648),
        ];
        for (unit, priority_q32) in expected {
            assert_eq!(
                config.units.get(unit).unwrap().rvo.priority_q32(),
                priority_q32
            );
        }
    }

    #[test]
    fn p0_configs_preserve_path_and_topology_discriminants() {
        let config = SimulationConfig::load().unwrap();
        let rhino = config.units.get("rhino").unwrap();
        assert_eq!(rhino.formation.members, 1);
        assert_eq!(rhino.formation_slot_size_meters().unwrap(), 30);
        assert!(!rhino.has_body);
        assert_eq!(rhino.independent_aim, None);
        assert!(matches!(rhino.attack.path, AttackPath::Direct));
        assert!(rhino.attack.melee);

        let wraith = config.units.get("wraith").unwrap();
        assert_eq!(wraith.attack.weapons.mode, WeaponMode::Group);
        assert_eq!(wraith.attack.weapons.indices, [0, 1, 2, 3]);
        assert_eq!(wraith.attack.weapons.fusillade, Some(false));
        assert_eq!(wraith.attack.weapons.allow_same_target, Some(true));
        assert_eq!(wraith.attack.weapons.rotation_speed, Some(90.0));
        assert!(matches!(
            wraith.attack.path,
            AttackPath::Projectile { count: 1, .. }
        ));

        let vortex = config.units.get("vortex").unwrap();
        assert_eq!(vortex.attack.weapons.mode, WeaponMode::Group);
        assert_eq!(vortex.attack.weapons.fusillade, Some(true));
        assert_eq!(vortex.attack.weapons.allow_same_target, Some(false));
        assert!(matches!(vortex.attack.path, AttackPath::Direct));
        assert!(!vortex.attack.melee);

        let crawler = config.units.get("crawler").unwrap();
        assert_eq!(crawler.formation.members, 24);
        assert_eq!(crawler.formation_slot_size_meters().unwrap(), 6);
    }

    #[test]
    fn projectile_target_offset_radius_is_zero_or_at_least_one_centimeter() {
        let config = SimulationConfig::load().unwrap();
        let mut stormcaller = config.units.get("stormcaller").unwrap().clone();
        for (radius, valid) in [(0.009, false), (0.01, true), (0.0, true)] {
            let AttackPath::Projectile {
                target_offset_radius,
                ..
            } = &mut stormcaller.attack.path
            else {
                panic!("stormcaller uses the projectile path");
            };
            *target_offset_radius = radius;
            assert_eq!(stormcaller.validate().is_ok(), valid, "radius {radius}");
        }
    }

    #[test]
    fn steel_ball_laser_damage_truncates_and_caps_the_native_multiplier_sequence() {
        let config = SimulationConfig::load().unwrap();
        let rules = config.units.get("steel_ball").unwrap();
        let stats = crate::data::Stats::of(rules).unwrap();
        let damage = (0..7)
            .map(|attack_count| stats.laser_damage(rules, attack_count, UnitDomain::Ground))
            .collect::<Vec<_>>();

        assert_eq!(damage, [2, 3, 8, 17, 31, 51, 77]);
        assert_eq!(
            stats.laser_damage(rules, usize::MAX, UnitDomain::Ground),
            2_604
        );
    }

    /// The kernel fires projectiles, blows and lasers, groups several
    /// projectile weapons that share out targets and several striking
    /// weapons that fire as a fusillade; a control beam is refused by the
    /// unit that fires it.
    #[test]
    fn a_main_skill_the_kernel_cannot_fire_is_refused_by_unit() {
        let config = SimulationConfig::load().unwrap();
        for fired in [
            "marksman",
            "rhino",
            "steel_ball",
            "wraith",
            "melting_point",
            "vortex",
            "raiden",
            "hacker",
        ] {
            assert!(config.units.get(fired).unwrap().fired().is_ok(), "{fired}");
        }
    }

    #[test]
    fn group_fields_are_all_or_nothing_and_group_only() {
        let config = SimulationConfig::load().unwrap();
        let mut partial_group = config.units.get("wraith").unwrap().clone();
        partial_group.attack.weapons.allow_same_target = None;
        assert!(partial_group.validate().is_err());

        let mut group_field_on_normal = config.units.get("marksman").unwrap().clone();
        group_field_on_normal.attack.weapons.fusillade = Some(false);
        assert!(group_field_on_normal.validate().is_err());
    }
}
