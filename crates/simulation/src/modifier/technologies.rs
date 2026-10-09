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
//! `BuffTech` the buff it adds its unit as the fight starts, and a
//! `SupportUnitTech` the production line a production item's
//! `SupportUnitEquipment` would, and a `MultiAttackTech` the projectiles it
//! adds its unit's bursts, and a `StealthTech` its stealth once it is hurt,
//! and a `DeadLineTech` the life under which its main skill destroys what it
//! hits, and a `SiegeModeTech` its unit dug in as the fight starts, and a
//! `WreckageRecoveryTech` life as an enemy it struck dies; any other
//! is refused by name rather than applied for its numbers alone.
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
    data::{Channel, Correction, Entry, ExperienceRate, Index},
    layout::{InterceptNumbers, Interception, TerrainSpec},
    rules::UnitDomain,
};

use super::{
    buffs::{self, BuffBlock, CycleBlock},
    effects::{self, Fields},
    providers::EffectProvider,
    sources::{
        AutoRecovery, BuffSource, Burrow, CarriedShield, ClearRangeItem, EnergyShield,
        KillExplosion, LifeSteal, MoveAbilityAttack, MoveAbilityRangeItem, ProductionLine,
        ReactiveArmor, Rebirth, RebirthFollow, RecoveryState, Repair, RvoRadiusChange, SiegeMode,
        Stealth, SweepIntensify, WreckageRecovery,
    },
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

/// The list whose `SupportUnitTech` runs a production line.
const SUPPORT: &str = "supportUnitTechnologies";

/// The list whose `DeadSummonTech` summons where its unit dies.
const DEAD_SUMMON: &str = "deadSummonTechnologies";

/// The list whose `MoveAbilitySummonTech` makes units as its unit's move
/// ability reaches a time.
const MOVE_SUMMON: &str = "moveAbilitySummonTechDatas";

/// The list whose `MultiAttackTech` adds to its unit's skill's projectile
/// count, the time between two of them and how far each may land from its
/// target, beside its numbers.
const MULTI_ATTACK: &str = "multiAttackTechnologies";

/// The list whose `StealthTech` is an `IStealthTechDataSource`.
const STEALTH: &str = "stealthTechData";

/// The list whose `DeadLineTech` is an `IDeadLineDataSource`.
const DEAD_LINE: &str = "deadLineTechDatas";

/// The list whose `MoveAbilityAttackIntensifyTech` is an
/// `IMoveAbilityAttackIntensify`.
const MOVE_ABILITY_ATTACK: &str = "moveAbilityAttackIntensifyTechDatas";

/// The list whose `MoveAbilityRangeItemTech` is an `IMoveAbilityRangeItem`.
const MOVE_ABILITY_RANGE_ITEM: &str = "moveAbilityRangeItemTechDatas";

/// `MoveAbilityTimeType.OnExitMoveEnd`: as its unit ends a surfacing.
const ON_EXIT_MOVE_END: i32 = 3;

/// `MechMoveType.Underground`.
const UNDERGROUND_MOVE: i32 = 1;
/// The list whose `DamageShareTech` is an `IMechGroupSource`.
const DAMAGE_SHARE: &str = "damageShareTechnologies";
/// The list whose `AdvancedEnergyShieldTech` is an
/// `IAdvancedEnergyShieldSource`, which hands its unit a [`CarriedShield`].
const BARRIER: &str = "advancedEnergyShieldTechnologies";
/// The list whose `ReactiveArmorTech` is an `IReactiveArmorTechDataSource`.
const REACTIVE_ARMOR: &str = "reactiveArmorTechDatas";
/// The list whose `FireIntensifyTech` is an `IFireIntensify`.
const FIRE_INTENSIFY: &str = "fireIntensifyTechnologies";
/// The list whose `SiegeModeTech` is an `ISiegeModeEffectDataSource`.
const SIEGE: &str = "siegeModeTechDatas";
/// The list whose `WreckageRecoveryTech` is an `IWreckageRecovery`.
const WRECKAGE: &str = "wreckageRecoveryTechnologies";
/// The list whose `RebirthTech` is an `IDeadEffect` and an `IRebirthData`.
const REBIRTH: &str = "rebirthEffectTechologyDatas";
/// The list whose `DeadAcidRangeItemTech` is an `IDeadAcidRangeItem`.
const DEAD_ACID: &str = "deadAcidRangeItemTechnologyDatas";
/// The list whose `BurrowTech` is an `IBurrow`.
const BURROW: &str = "burrowTechnologies";
/// The list whose `RVORadiusChangeTechnology` is an `IRVORadiusChangeSource`.
const RVO_RADIUS_CHANGE: &str = "rVORadiusChangeTechnologyTechDatas";
/// The list whose `ClearRangeItemTech` is an `IClearRangeItem`.
const CLEAR_RANGE_ITEM: &str = "clearRangeItemTechDatas";
/// The list whose `RecoveryTech` is an `IRecoveryTechEffectDataSource`.
const REPAIR: &str = "recoveryTechDatas";
/// The list whose `KillExplosionTech` is an `IKillExplosionDataSource`.
const KILL_EXPLOSION: &str = "killExplosionTechDatas";

/// The lists whose rows this build applies, each with its mechanism.
const IMPLEMENTED: [&str; 35] = [
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
    SUPPORT,
    SPLASH,
    MOBILITY,
    DEAD_SUMMON,
    MOVE_SUMMON,
    MULTI_ATTACK,
    STEALTH,
    DEAD_LINE,
    MOVE_ABILITY_ATTACK,
    MOVE_ABILITY_RANGE_ITEM,
    DAMAGE_SHARE,
    BARRIER,
    REACTIVE_ARMOR,
    SIEGE,
    FIRE_INTENSIFY,
    WRECKAGE,
    REBIRTH,
    RVO_RADIUS_CHANGE,
    CLEAR_RANGE_ITEM,
    BURROW,
    REPAIR,
    DEAD_ACID,
    KILL_EXPLOSION,
];

/// The list whose `SplashTech` adds its row's `range` to its unit's skill's
/// `SplashRangeValue` beside its numbers.
const SPLASH: &str = "splashTechnologies";

/// The list whose `MobilityIntensifyTech` overrides nothing of `Technology`:
/// in a fight it is a plain technology, and what it frees, its formation
/// during deployment, a fight does not read.
const MOBILITY: &str = "mobilityIntensifyTechnologies";

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

/// `AutoRecoveryStateType.Normal` and `Underground`; `Cloak`, 2, is refused.
const NORMAL: i64 = 0;
const UNDERGROUND: i64 = 1;

/// `Technology`'s `IEffectProviderDataSource.GetPriority`, which an
/// equipment's 1 overrides.
const PRIORITY: i32 = 0;

/// The module that tags every entry a technology writes.
pub(crate) const SOURCE: &str = "Modifier";

/// What a production line writes onto its makes is its
/// `SupportUnitData.modifyData`'s, which no technology switch takes away.
const MAKE_SOURCE: &str = "SupportUnitData";

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
    /// Its `expChangeRate`, one entry per level.
    exp_rate: Vec<i64>,
    /// What it answers `ILifeSteal` with, if its class is one.
    lifesteal: Option<LifeSteal>,
    /// What it answers `IAutoRecovery` with, if its class is one.
    auto_recovery: Option<AutoRecovery>,
    /// What it answers `IEnergyShieldSource` with, if its class is one.
    energy_shield: Option<EnergyShield>,
    /// What it answers `IStealthTechDataSource` with, if its class is one.
    stealth: Option<Stealth>,
    /// What it answers `IDeadLineDataSource` with, if its class is one: the
    /// life by its unit's level, and whether a shield keeps it off.
    dead_line: Option<(Vec<i64>, bool)>,
    /// What it answers `IMechGroupSource` with, if its class is one: the
    /// share distance, `FPoint` metres by its unit's level, and the rest.
    mech_group: Option<(Vec<i64>, MechGroup)>,
    /// What it answers `IAdvancedEnergyShieldSource` with, if its class is
    /// one: the energy and the whole metres of radius by its unit's level.
    carried_shield: Option<(Vec<i64>, Vec<i64>)>,
    /// What it answers `IReactiveArmorTechDataSource` with, if its class is
    /// one.
    reactive_armor: Option<ReactiveArmor>,
    /// What it answers `IFireIntensify` with, if its class is one: its
    /// fire's `FPoint` metres and seconds by its unit's level.
    fire_intensify: Option<(Vec<i64>, Vec<i64>)>,
    /// What it answers `ISiegeModeEffectDataSource` with, if its class is
    /// one.
    siege_mode: Option<SiegeMode>,
    /// What it answers `IWreckageRecovery` with, if its class is one.
    wreckage: Option<WreckageRecovery>,
    /// What it answers `IRebirthData` with, if its class is one.
    rebirth: Option<Rebirth>,
    /// What it answers `IRVORadiusChangeSource` with, if its class is one.
    rvo_radius_change: Option<RvoRadiusChange>,
    /// What it answers `IClearRangeItem` with, if its class is one.
    clear_range_item: Option<ClearRangeItem>,
    /// What it answers `IRecoveryTechEffectDataSource` with, if its class
    /// is one.
    repair: Option<Repair>,
    /// What it answers `IKillExplosionDataSource` with, if its class is one.
    kill_explosion: Option<KillExplosion>,
    /// What it answers `IBurrow` with, if its class is one.
    burrow: Option<Burrow>,
    /// The acid it leaves where its unit dies, if its class is an
    /// `IDeadAcidRangeItem`, or why it is refused.
    dead_acid: Option<std::result::Result<TerrainSpec, String>>,
    /// What it answers `IMoveAbilityAttackIntensify` with, if its class is
    /// one.
    move_ability_attack: Option<MoveAbilityAttack>,
    /// What it answers `IMoveAbilityRangeItem` with, if its class is one.
    move_ability_range_item: Option<MoveAbilityRangeItem>,
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
    /// The production line it hands its unit, if its class is an
    /// `ISupportEffectDataSource`.
    production: Option<ProductionLine>,
    /// What its unit summons where it dies, if its class is an
    /// `IDeadSummon`.
    dead_summon: Option<UnitDeadSummon>,
    /// The line its unit runs once each time it begins to surface, if its
    /// class is an `IMoveAbilitySummon`.
    surfacing_line: Option<ProductionLine>,
    /// The provider beside the numbers' its class reaches, if any.
    provider: Option<EffectProvider>,
    /// `Technology.CanDisable`: its row's `ignoreElectricEffect` unset.
    can_disable: bool,
}

/// What `DeadLineEffectProvider` hands its unit's main skill as a pre-hit
/// effect (`PerformPreHitEffect`): a unit it hits at or under `life` is
/// destroyed, unless its own shield has energy left and the row does not
/// ignore shields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DeadLine {
    /// `IDeadLineDataSource.GetDeadLineValue` at the unit's level.
    pub(crate) life: i64,
    /// `IsIgnoreEnergyShield`.
    pub(crate) ignores_shield: bool,
}

/// What a grouping technology answers `IMechGroupSource` with at its unit's
/// level, which `MechGrounpEffectProvider` hands its side's manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MechGroup {
    /// `GetShareDistance`: `FPoint` metres, edge to edge.
    pub(crate) distance_q32: i64,
    /// `GetGroupPurpose`.
    pub(crate) purpose: GroupPurpose,
    /// `GetFloatRateValue`: what each member's main skill's damage rate
    /// rises by for every other member, `FPoint`.
    pub(crate) damage_rate_q32: i64,
    /// `GetMaxCount`: the most members that count, or every one at zero.
    pub(crate) max_count: i32,
}

/// `MechGroupPurpose`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GroupPurpose {
    /// `DamageShare`: a hit on a member is shared among the group.
    DamageShare,
    /// `DamageChange`: each member's skill damage rises with the group's
    /// count.
    DamageChange,
}

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
    /// The sources of which the unit holds one, the first.
    pub(crate) single: SingleSources,
    pub(crate) buff_sources: Vec<BuffSource>,
    pub(crate) interception: Vec<UnitInterception>,
    pub(crate) dead_summon: Option<UnitDeadSummon>,
    /// The line it runs once each time it begins to surface
    /// (`MoveAbilitySummonTech`).
    pub(crate) surfacing_line: Option<ProductionLine>,
}

/// The sources of the interfaces a unit holds one source of, the first its
/// side's technologies hand it: the one a `SingleEffectProvider` enables, or
/// the one a system or controller holds. A unit made, summoned or summoned
/// as another dies carries them whole.
#[derive(Debug, Clone, Default)]
pub(crate) struct SingleSources {
    /// The first that puts its unit in stealth: `StealthTechSystem.AddMech`
    /// passes over a unit it already holds.
    pub(crate) stealth: Option<Stealth>,
    /// The first that digs its unit in: `SiegeModeEffectSystem.
    /// AddSiegeModeOwner` passes over a unit it already holds.
    pub(crate) siege_mode: Option<SiegeMode>,
    /// The first that heals its unit as an enemy it struck dies: the
    /// provider enables one source (`SingleEffectProvider`).
    pub(crate) wreckage: Option<WreckageRecovery>,
    /// What brings its unit back after it dies.
    pub(crate) rebirth: Option<Rebirth>,
    /// The first that widens its unit's agent: the provider enables one
    /// source (`SingleEffectProvider`).
    pub(crate) rvo_radius_change: Option<RvoRadiusChange>,
    /// The first that clears terrain about its unit: the provider enables
    /// one source (`SingleEffectProvider`).
    pub(crate) clear_range_item: Option<ClearRangeItem>,
    /// The first that repairs the units about its unit: the provider enables
    /// one source (`SingleEffectProvider`).
    pub(crate) repair: Option<Repair>,
    /// The first that sets off what its unit's skill kills: the provider
    /// enables one source (`SingleEffectProvider`).
    pub(crate) kill_explosion: Option<KillExplosion>,
    /// The first that burrows its unit: the provider enables one source
    /// (`SingleEffectProvider`).
    pub(crate) burrow: Option<Burrow>,
    /// The acid the first that leaves one where its unit dies leaves
    /// (`DeadAcidRangeItemController`).
    pub(crate) dead_acid: Option<TerrainSpec>,
}

/// What a move ability summon row answers `IMoveAbilitySummon` and
/// `ISupportDataSource` with.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveSummonBlock {
    /// `MoveAbilityTimeType`.
    summon_time: i32,
    support_unit_id: u32,
    /// `DynamicMechLevel`.
    unit_level: i32,
    max_batch: u32,
    max_alive: u32,
    create_count_per_time: u32,
    /// `MoveAbilitySummonTech.GetStartTime`; what it delays is not read.
    #[allow(dead_code, reason = "every row's is 0")]
    start_time: i64,
    /// `SupportUnitAppearType`.
    appear_type: i32,
    product_time: i64,
    create_duration: i64,
    positions: Vec<SupportOffset>,
}

/// `MoveAbilityTimeType.OnExitMoveBegin`: as its unit begins to surface.
const ON_EXIT_MOVE_BEGIN: i32 = 2;

/// The line a move ability summon row runs as its unit begins to surface,
/// or why this build will not. `MoveAbilitySummonSystem` hands the unit's
/// side a creator of a `SpecialSupportUnitData` of it then
/// (`TeamSupportUnitManager.AddTemporaryCreator`), whose offsets turn with
/// the unit's root (`GetSupportUnitPositionSpace` answers `Parent`) and
/// which `SupportUnitCreator.IsFinished` ends after its first update, its
/// `GetLifeTime` being -1.
fn surfacing_line_of(row: &Row, who: &str) -> std::result::Result<Option<ProductionLine>, String> {
    if row.kind != MOVE_SUMMON {
        return Ok(None);
    }
    let block = row
        .move_summon
        .as_ref()
        .ok_or_else(|| format!("{who} carries no move ability summon"))?;
    let unread = [
        (
            block.summon_time != ON_EXIT_MOVE_BEGIN,
            "makes its units at a moveAbilityTimeType other than 2",
        ),
        (
            ![
                IMMEDIATE,
                IMMEDIATE_WITH_EFFECT,
                UNDERGROUND_STRIKE,
                UNDERGROUND_STRIKE_ANIMATOR,
                TRANSITION,
            ]
            .contains(&block.appear_type)
                && !PRODUCED.contains(&block.appear_type),
            "makes its units appear by an appearType other than 0, 1, 3, 4, 5, 6 or 8",
        ),
        (
            block.positions.is_empty(),
            "makes its units at no positions",
        ),
        (
            ![LEVEL_ONE, PARENT_LEVEL].contains(&block.unit_level),
            "makes its units at a unitLevel of their own",
        ),
    ];
    if let Some((_, what)) = unread.iter().find(|(set, _)| *set) {
        return Err(format!("{who} {what}, which is not read"));
    }
    Ok(Some(ProductionLine {
        unit_type_id: block.support_unit_id,
        max_batch: block.max_batch,
        max_alive: block.max_alive,
        per_time: block.create_count_per_time,
        interval_q32: block.create_duration,
        offsets: block
            .positions
            .iter()
            .map(|offset| (offset.x, offset.z))
            .collect(),
        appear_q32: appear_q32(block.appear_type, block.product_time),
        parent_level: block.unit_level == PARENT_LEVEL,
        body_frame: false,
        arrival: super::sources::Arrival::InPlace,
        make_corrections: Vec::new(),
        gated: false,
    }))
}

/// What a `DeadSummonTech` answers `IDeadSummon` with: the unit type its unit
/// summons where it dies, at the first level, and how many by its unit's
/// level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnitDeadSummon {
    unit_type_id: u32,
    counts: Vec<u32>,
    /// Whether they take its unit's level (`DynamicMechLevel.Parent`).
    parent_level: bool,
}

impl UnitDeadSummon {
    /// What it summons from a unit of `level`: as many as
    /// `DeadSummonTechnologyData.GetUnitCount` answers, the entry of the
    /// level, the last past it, at the first level or the unit's
    /// (`DeadSummonController.PerformDeadEffect`).
    pub(crate) fn at(&self, level: i64) -> crate::layout::DeadSummonOnDeath {
        let index = usize::try_from(level - 1).unwrap_or(0);
        crate::layout::DeadSummonOnDeath {
            unit_type_id: self.unit_type_id,
            count: self
                .counts
                .get(index)
                .or(self.counts.last())
                .copied()
                .unwrap_or(0),
            level: if self.parent_level { level } else { 1 },
        }
    }
}

/// What a dead summon row answers `IDeadSummon` with.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeadSummonBlock {
    #[serde(rename = "unit_id")]
    type_id: u32,
    #[serde(rename = "unit_count")]
    counts: Vec<u32>,
    /// `DynamicMechLevel`.
    #[serde(rename = "unit_level")]
    level: i32,
}

/// What a dead summon row makes its unit summon, or why this build will not.
fn dead_summon_of(row: &Row, who: &str) -> std::result::Result<Option<UnitDeadSummon>, String> {
    if row.kind != DEAD_SUMMON {
        return Ok(None);
    }
    let block = row
        .dead_summon
        .as_ref()
        .ok_or_else(|| format!("{who} carries no dead summon"))?;
    // `DeadSummonController.PerformDeadEffect` takes its unit's level for
    // `DynamicMechLevel.Parent`.
    if ![LEVEL_ONE, PARENT_LEVEL].contains(&block.level) {
        return Err(format!(
            "{who} summons where its unit dies at a unitLevel of {}, which is not read",
            block.level
        ));
    }
    Ok(Some(UnitDeadSummon {
        unit_type_id: block.type_id,
        counts: block.counts.clone(),
        parent_level: block.level == PARENT_LEVEL,
    }))
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
    /// `expChangeRate`, which `GetExpChangeRate` answers the unit's
    /// `MechDataModifer.TryAddCommonData` with.
    #[serde(default)]
    exp_rate: Vec<i64>,
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
    /// `TechnologyData.extraSkillEffect`, on an air-attack, grouping or
    /// kill-explosion row.
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
    /// `SplashTechnologyData.range`, on a splash row: `FPoint` metres by the
    /// unit's level.
    #[serde(default)]
    splash_range: Vec<i64>,
    /// `TechnologyData.isInverseIsLockTarget`: whether it turns its unit's
    /// skills' locking of their target over.
    #[serde(default)]
    inverse_lock_target: bool,
    /// `TechnologyData.ignoreElectricEffect`: whether a buff that disables
    /// technology leaves it on (`Technology.CanDisable` answers false).
    #[serde(default)]
    ignore_electric_effect: bool,
    /// `MultiAttackTechnologyData`'s `countIncrease`, `durationChangeValue`
    /// and `randomRangeChangeValue`, on a row of its list: whole projectiles,
    /// `FPoint` seconds and `FPoint` metres by the unit's level.
    #[serde(default)]
    projectile_count_value: Vec<i64>,
    #[serde(default)]
    projectile_duration_value: Vec<i64>,
    #[serde(default)]
    projectile_random_range_value: Vec<i64>,
    /// `InterceptMissileTechnologyData`'s fields, on a row of its list.
    #[serde(default)]
    intercept: Option<InterceptBlock>,
    /// `StealthTechData`'s fields, on a row of its list.
    #[serde(default)]
    stealth: Option<StealthBlock>,
    /// `SiegeModeTechData`'s fields, on a row of its list.
    #[serde(default)]
    siege_mode: Option<SiegeModeBlock>,
    /// `WreckageRecoveryTechnologyData`'s fields, on a row of its list.
    #[serde(default)]
    wreckage: Option<WreckageBlock>,
    /// `RebirthTechnologyData`'s fields, on a row of its list.
    #[serde(default)]
    rebirth: Option<RebirthBlock>,
    /// `RVORadiusChangeTechnologyTechData.moveRadius` and
    /// `nearTargetThreshold`, `FPoint` metres, on a row of its list.
    #[serde(default)]
    rvo_move_radius: i64,
    #[serde(default)]
    rvo_near_target_threshold: i64,
    /// `ClearRangeItemTechData.radius`, whole metres, and `rangeItemTypes`,
    /// on a row of its list.
    #[serde(default)]
    clear_radius: i32,
    #[serde(default)]
    clear_range_item_types: Vec<i32>,
    /// `RecoveryTechData`'s `life`, whole life by level, `maxLifeRate`,
    /// `interval` and `range`, `FPoint`, and `canRecoverEnemy`,
    /// `isOnlyRecoverMech` and `canRecoverAir`, on a row of its list.
    #[serde(default)]
    repair_life: Vec<i64>,
    #[serde(default)]
    repair_max_life_rate: i64,
    #[serde(default)]
    repair_interval: i64,
    #[serde(default)]
    repair_range: i64,
    #[serde(default)]
    repair_enemy: bool,
    #[serde(default)]
    repair_only_mech: bool,
    #[serde(default)]
    repair_air: bool,
    /// `KillExplosionTechData`'s `explosionDamage`, whole damage by level,
    /// `explosionRange`, `FPoint`, and `canExplosionTriggerExplosion`,
    /// `canHitAlly` and `canBeAffectedByBuff`, on a row of its list;
    /// `main_skill_effect` and `extra_skill_effect` are read beside them.
    #[serde(default)]
    kill_explosion_damage: Vec<i64>,
    #[serde(default)]
    kill_explosion_range: i64,
    #[serde(default)]
    kill_explosion_chains: bool,
    #[serde(default)]
    kill_explosion_hits_allies: bool,
    #[serde(default)]
    kill_explosion_buffed: bool,
    /// `BurrowData.amplifyDamageRate`, `relieveDistance` and
    /// `isEnterUnderGround`, on a row of its list.
    #[serde(default)]
    burrow_amplify_damage_rate: Vec<i64>,
    #[serde(default)]
    burrow_relieve_distance: Vec<i64>,
    #[serde(default)]
    burrow_enters_underground: bool,
    /// `DeadAcidRangeItemTechnologyData`'s fields, on a row of its list.
    #[serde(default)]
    dead_acid: Option<DeadAcidBlock>,
    /// `DeadLineTechData`'s `deadLineValue`, whole life by the unit's level,
    /// and `ignoreEnergyShield`, on a row of its list.
    #[serde(default)]
    dead_line_value: Vec<i64>,
    #[serde(default)]
    dead_line_ignores_shield: bool,
    /// `DamageShareTechnologyData.distance`, on a row of its list: `FPoint`
    /// metres by the unit's level.
    #[serde(default)]
    share_distance: Vec<i64>,
    /// `DamageShareTechnologyData`'s `mechGroupPurpose`, `floatRateValue`
    /// and `maxCount`, and `TechnologyData.mainSkillEffect`, on a row of
    /// its list; `extra_skill_effect` is read beside them.
    #[serde(default)]
    group_purpose: i32,
    #[serde(default)]
    group_damage_rate: i64,
    #[serde(default)]
    group_max_count: i32,
    #[serde(default)]
    main_skill_effect: bool,
    /// `AdvancedEnergyShieldTechData.shieldValues` and `radius`, on a row of
    /// its list: energy and whole metres by the unit's level.
    #[serde(default)]
    barrier_energy: Vec<i64>,
    #[serde(default)]
    barrier_radius: Vec<i64>,
    /// `ReactiveArmorTechData.damageReduceRate`, an `FPoint` raw rate, and
    /// `damageReduceCount`, on a row of its list.
    #[serde(default)]
    reactive_armor_rate: i64,
    #[serde(default)]
    reactive_armor_count: i32,
    /// `FireIntensifyTechnologyData.range` and `lifeTime`, on a row of its
    /// list: `FPoint` metres and seconds by the unit's level.
    #[serde(default)]
    fire_range: Vec<i64>,
    #[serde(default)]
    fire_life_time: Vec<i64>,
    /// `MoveAbilityAttackIntensifyTechData`'s fields and the
    /// `exitTimeChangeRate` its tech answers, on a row of its list.
    #[serde(default)]
    exit_time_rate: i64,
    #[serde(default)]
    strike_trigger_count: i32,
    #[serde(default)]
    strike_damage_rate: i64,
    #[serde(default)]
    strike_splash_range: i64,
    #[serde(default)]
    strike_attack_point: i64,
    /// `MoveAbilityRangeItemTechData`'s fields, on a row of its list.
    #[serde(default)]
    range_item_time: i32,
    #[serde(default)]
    range_item_move_type: i32,
    #[serde(default)]
    range_item_range: i64,
    #[serde(default)]
    range_item_life_time: i64,
    #[serde(default)]
    fog_attack_range_rate: i64,
    #[serde(default)]
    reduce_damage_from_remote: i64,
    /// `SupportUnitTechnologyData`'s fields, on a row of its list.
    #[serde(default)]
    production: Option<SupportBlock>,
    /// `DeadSummonTechnologyData`'s fields, on a row of its list.
    #[serde(default)]
    dead_summon: Option<DeadSummonBlock>,
    /// `MoveAbilitySummonTechData`'s fields, on a row of its list.
    #[serde(default)]
    move_summon: Option<MoveSummonBlock>,
}

/// What a production row answers `ISupportDataSource` with.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SupportBlock {
    support_unit_id: u32,
    /// `DynamicMechLevel`.
    unit_level: i32,
    max_batch: u32,
    max_alive: u32,
    create_count_per_time: u32,
    /// What it delays is not read: a production item's line makes its first
    /// batch on the fight's first tick whatever its `startTime`.
    #[allow(dead_code, reason = "the first batch comes on the first tick")]
    start_time: i64,
    /// `SupportUnitAppearType`.
    appear_type: i32,
    /// How long a make of type 6 or 8 takes to appear.
    product_time: i64,
    max_create_count: u32,
    create_duration: i64,
    /// `SupportUnitPositionSpace`.
    position_space: i32,
    unit_life_rate: i64,
    unit_damage_rate: i64,
    unit_attack_range_value: i32,
    intensify_mode: bool,
    inherit_technology: bool,
    positions: Vec<SupportOffset>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct SupportOffset {
    x: i64,
    z: i64,
}

/// `DynamicMechLevel.Level1` and `Parent`.
const LEVEL_ONE: i32 = 0;
const PARENT_LEVEL: i32 = 3;

/// `SupportUnitAppearType.Immediate`, `ImmediateWithEffect` and
/// `Transition`. The first two differ only in what the client shows: in
/// `SupportUnitCreator.CreateMech` neither takes time to appear, and only the
/// event a view reads carries an effect's duration.
const IMMEDIATE: i32 = 0;
const IMMEDIATE_WITH_EFFECT: i32 = 1;
const TRANSITION: i32 = 5;
/// `SupportUnitAppearType.UndergroundStrike` and `UndergroundStrikeAnimator`,
/// which only the client tells apart from a transition.
const UNDERGROUND_STRIKE: i32 = 3;
const UNDERGROUND_STRIKE_ANIMATOR: i32 = 4;
/// The two types `SupportUnitCreator.CreateMech` gives the row's
/// `productTime` to appear, through `SupportUnitData.GetProductMoveTime`.
const PRODUCED: [i32; 2] = [6, 8];
/// `appearType` 7, which comes out of its unit: as long to appear, the
/// row's `productTime`, and made by `SummonSystem.CreateMechDelaySetPos`.
const COMES_OUT: i32 = 7;

/// `SupportUnitPositionSpace.None`, `Parent` and `ParentBody`.
/// `SpecialSupportUnitData.GetRotation` turns an offset by the unit's body for
/// `ParentBody` and by its root otherwise.
const NO_SPACE: i32 = 0;
const PARENT_SPACE: i32 = 1;
const PARENT_BODY_SPACE: i32 = 2;

/// `SupportUnitCreator.CreateMech`: a make appears at once for types 0 and
/// 1, in the row's `productTime` for 6, 7 and 8, and in `APPEAR_DURATION`, a
/// second, otherwise.
fn appear_q32(appear_type: i32, product_time: i64) -> i64 {
    if [IMMEDIATE, IMMEDIATE_WITH_EFFECT].contains(&appear_type) {
        0
    } else if PRODUCED.contains(&appear_type) || appear_type == COMES_OUT {
        product_time
    } else {
        1 << 32
    }
}

impl SupportBlock {
    /// How long its makes take to appear, [`appear_q32`] of its row.
    fn appear_q32(&self) -> i64 {
        appear_q32(self.appear_type, self.product_time)
    }

    fn takes_time_to_appear(&self) -> bool {
        self.appear_q32() > 0
    }

    /// The line it runs, or why this build will not run it: one whose makes
    /// appear any way but at once or in their time at their offsets, at a
    /// level of their own, whose attack range the row corrects, capped in
    /// all, made in its intensify mode or without its side's technologies.
    fn line(&self, who: &str) -> std::result::Result<ProductionLine, String> {
        let unread = [
            (
                ![IMMEDIATE, IMMEDIATE_WITH_EFFECT, TRANSITION, COMES_OUT]
                    .contains(&self.appear_type)
                    && !PRODUCED.contains(&self.appear_type),
                "an appearType other than 0, 1, 5, 6, 7 or 8",
            ),
            (
                self.takes_time_to_appear() && self.positions.is_empty(),
                "an appearance that takes time and no positions",
            ),
            (
                ![LEVEL_ONE, PARENT_LEVEL].contains(&self.unit_level),
                "a unitLevel of its own",
            ),
            (
                ![NO_SPACE, PARENT_SPACE, PARENT_BODY_SPACE].contains(&self.position_space),
                "a positionSpace other than 0, 1 or 2",
            ),
            (self.max_create_count != 0, "a maxCreateCount"),
            (
                self.unit_attack_range_value != 0,
                "a correction of its makes' attack range",
            ),
            (self.intensify_mode, "its intensifyMode"),
            (
                !self.inherit_technology,
                "makes without its side's technologies",
            ),
        ];
        if let Some((_, what)) = unread.iter().find(|(set, _)| *set) {
            return Err(format!(
                "{who} runs a production line with {what}, which is not read"
            ));
        }
        Ok(ProductionLine {
            unit_type_id: self.support_unit_id,
            max_batch: self.max_batch,
            max_alive: self.max_alive,
            per_time: self.create_count_per_time,
            interval_q32: self.create_duration,
            offsets: self
                .positions
                .iter()
                .map(|offset| (offset.x, offset.z))
                .collect(),
            appear_q32: self.appear_q32(),
            parent_level: self.unit_level == PARENT_LEVEL,
            body_frame: self.position_space == PARENT_BODY_SPACE,
            arrival: if self.appear_type == COMES_OUT {
                super::sources::Arrival::ComesOut
            } else {
                super::sources::Arrival::InPlace
            },
            make_corrections: effects::corrections(&Fields {
                life_rate: Some(self.unit_life_rate),
                damage_rate: Some(self.unit_damage_rate),
                ..Fields::default()
            })
            .into_iter()
            .map(|(channel, index, correction)| {
                (
                    channel,
                    Entry {
                        index,
                        source: MAKE_SOURCE,
                        correction,
                    },
                )
            })
            .collect(),
            gated: false,
        })
    }
}

/// What a stealth row answers `IStealthTechDataSource` with: `FPoint` raw
/// integers.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct StealthBlock {
    trigger_life_rate: i64,
    duration: i64,
}

impl StealthBlock {
    const fn source(self, can_disable: bool) -> Stealth {
        Stealth {
            trigger_life_rate_q32: self.trigger_life_rate,
            duration_q32: self.duration,
            can_disable,
        }
    }
}

/// What a wreckage-recovery row answers `IWreckageRecovery` with: `FPoint`
/// seconds, and whole metres by the dying unit's level. No row sets
/// `ignoreElectricEffect`, so each answers `CanDisable` true.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct WreckageBlock {
    time: i64,
    distance: Vec<i64>,
}

/// What a dead-acid row answers `IRangeItemProvider` with: whole metres of
/// range (`GetRangeItemRange`, `subEffectRange`) and the rounds it stands
/// (`GetRoundDuration`), and the `buffDatas` row of its `buffID`, which
/// `PreProcess` makes its `BuffData`. `GetLifeTime` answers none.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeadAcidBlock {
    range: i64,
    rounds: i32,
    buff: BuffBlock,
}

/// What a rebirth row answers `IRebirthData` with: whole seconds, counts and
/// the unit it rises as, then what a pilot that follows an ally flies by,
/// `FPoint` raw.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(
    dead_code,
    reason = "its rotation's rate turns a pilot no recording reads"
)]
struct RebirthBlock {
    cost_time: i64,
    count: i64,
    rebirthing_time: f64,
    unit_id: u32,
    unit_count: i64,
    follow_others: bool,
    interval_x: i64,
    interval_z: i64,
    interval_from_mech_center_z: i64,
    random_offset_range: [i64; 3],
    follow_start_offset_range: [i64; 3],
    transfer_distance_min: i64,
    speed_in_transfer: i64,
    per_r: i64,
    follow_rate: i64,
    follow_rotate_rate: i64,
}

/// The terrain a fire-extinguisher row clears, by `RangeItemType`'s
/// numbers, or why this build will not.
fn clear_range_item_of(
    row: &Row,
    who: &str,
) -> std::result::Result<Option<ClearRangeItem>, String> {
    use crate::layout::TerrainKind;
    if row.kind != CLEAR_RANGE_ITEM {
        return Ok(None);
    }
    let kinds = row
        .clear_range_item_types
        .iter()
        .map(|&kind| match kind {
            0 => Ok(TerrainKind::Fire),
            1 => Ok(TerrainKind::Oil),
            2 => Ok(TerrainKind::Fog),
            3 => Ok(TerrainKind::Acid),
            5 => Ok(TerrainKind::FogSand),
            _ => Err(format!(
                "{who} clears a terrain of kind {kind}, which is not simulated"
            )),
        })
        .collect::<std::result::Result<_, _>>()?;
    Ok(Some(ClearRangeItem {
        radius: row.clear_radius,
        kinds,
    }))
}

/// What a kill-explosion row sets off, or why this build will not.
///
/// `SkillManager.AddHitEffect` hands the effect to the skills
/// `SkillDataModifier.AvaliableCheck` admits, which for a row of the main
/// skill and no extra skill are those that take the main skill's
/// corrections; another pair is not read. `DamagePerformer` strikes a
/// splash of whole space units.
fn kill_explosion_of(
    row: &Row,
    who: &str,
    can_disable: bool,
) -> std::result::Result<Option<KillExplosion>, String> {
    if row.kind != KILL_EXPLOSION {
        return Ok(None);
    }
    if !row.main_skill_effect || row.extra_skill_effect {
        return Err(format!(
            "{who} sets off what skills beside the main skill's kill, which is not read"
        ));
    }
    let scaled = i128::from(row.kill_explosion_range)
        * i128::from(crate::rules::SPACE_UNITS_PER_METER_SCALE);
    if scaled & 0xffff_ffff != 0 {
        return Err(format!(
            "{who} explodes over a range of a fraction of a space unit, which is not read"
        ));
    }
    Ok(Some(KillExplosion {
        damage: row.kill_explosion_damage.clone(),
        range: i64::try_from(scaled >> 32).map_err(|_| format!("{who} explodes too far"))?,
        chains: row.kill_explosion_chains,
        hits_allies: row.kill_explosion_hits_allies,
        buffed: row.kill_explosion_buffed,
        can_disable,
    }))
}

/// What a rebirth row brings its unit back as, or why this build will not.
fn rebirth_of(row: &Row, who: &str) -> std::result::Result<Option<Rebirth>, String> {
    if row.kind != REBIRTH {
        return Ok(None);
    }
    let block = row
        .rebirth
        .as_ref()
        .ok_or_else(|| format!("{who} carries no rebirth"))?;
    let own = mechcore_document::unit_type_from_id(i32::try_from(block.unit_id).unwrap_or(-1))
        .is_some_and(|(name, _)| name == row.unit);
    if !own || block.unit_count != 1 {
        return Err(format!(
            "{who} brings its unit back as {} of unit type {}, which is not its own unit",
            block.unit_count, block.unit_id
        ));
    }
    // `RebirthTechnologyData.rebirthingTime` is a `float`, a whole number
    // of quarter seconds on every row, which `FPoint` takes exactly.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the row's float is a few seconds"
    )]
    let rebirthing_q32 = (block.rebirthing_time * 4.0).round() as i64 * (1 << 30);
    Ok(Some(Rebirth {
        cost_seconds: block.cost_time,
        count: block.count,
        rebirthing_q32,
        follow: block.follow_others.then_some(RebirthFollow {
            interval_x_q32: block.interval_x,
            interval_z_q32: block.interval_z,
            interval_from_center_z_q32: block.interval_from_mech_center_z,
            random_offset_q32: block.random_offset_range,
            start_offset_q32: block.follow_start_offset_range,
            transfer_distance_q32: block.transfer_distance_min,
            transfer_speed_q32: block.speed_in_transfer,
            per_r_q32: block.per_r,
            follow_rate_q32: block.follow_rate,
        }),
    }))
}

/// What a siege-mode row answers `ISiegeModeEffectDataSource` with: `FPoint`
/// raw integers.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_field_names,
    reason = "the fields are the row's own names"
)]
struct SiegeModeBlock {
    life_rate: i64,
    attack_interval_rate: i64,
    attack_interval_value: i64,
    damage_rate: i64,
    attack_range_value: i64,
    attack_range_rate: i64,
    splash_range_value: i64,
    projectile_speed_value: i64,
    duration: i64,
    animation_delay: i64,
}

impl SiegeModeBlock {
    /// Its numbers as a technology's own fields would write them:
    /// `SiegeModeEffectSystem.AddEffect` writes them on the unit's main skill
    /// (`FightSkill.AddData`) and the unit (`FightMech.AddData`) as
    /// `SkillDataModifier.AddData` writes a technology's.
    fn source(self) -> SiegeMode {
        let fields = Fields {
            life_rate: Some(self.life_rate),
            damage_rate: Some(self.damage_rate),
            attack_range_rate: Some(self.attack_range_rate),
            attack_interval_rate: Some(self.attack_interval_rate),
            attack_range_value: Some(self.attack_range_value),
            attack_interval_value: Some(self.attack_interval_value),
            splash_range_value: Some(self.splash_range_value),
            projectile_speed_value: Some(self.projectile_speed_value),
            ..Fields::default()
        };
        SiegeMode {
            written: effects::corrections(&fields),
            duration_q32: self.duration,
            animation_delay_q32: self.animation_delay,
        }
    }
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

    #[allow(clippy::too_many_lines)]
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
            let can_disable = !row.ignore_electric_effect;
            // `LifestealTech.GetLifestealMuliplier` reads its row's list at
            // the unit's level, which `corrections_of` refuses past one entry.
            let lifesteal = (row.kind == LIFESTEAL).then(|| LifeSteal {
                multiplier_q32: row.lifesteal_multiplier.first().copied().unwrap_or(0),
                priority: PRIORITY,
                can_disable,
            });
            // `AutoRecoveryTech` reads its two lists at the unit's level, as
            // `LifestealTech` does.
            let auto_recovery = (row.kind == AUTO_RECOVERY).then(|| AutoRecovery {
                start_time_q32: row.start_time,
                duration_q32: row.recovery_duration.first().copied().unwrap_or(0),
                life_rate_q32: row.recovery_life_rate.first().copied().unwrap_or(0),
                state: if row.auto_recovery_state_type == UNDERGROUND {
                    RecoveryState::Underground
                } else {
                    RecoveryState::Normal
                },
                priority: PRIORITY,
                can_disable,
            });
            let energy_shield = (row.kind == ENERGY_SHIELD).then_some(EnergyShield {
                life_rate_q32: SHIELD_LIFE_RATE,
                priority: PRIORITY,
                can_disable,
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
                    can_disable,
                    &row.buff_special,
                    (row.buff.as_ref(), row.buff_range_item.as_ref()),
                )
            });
            let (buff_source, effect) = match buff_source {
                Some(Err(why)) => (None, Err(why)),
                Some(Ok(buff)) => (Some(buff), corrections_of(&row)),
                None => (None, corrections_of(&row)),
            };
            let ((interception, production), effect) = match subclass_of(&row, &who) {
                Ok(subclass) => (subclass, effect),
                Err(why) => ((None, None), Err(why)),
            };
            let summons = dead_summon_of(&row, &who)
                .and_then(|dead| surfacing_line_of(&row, &who).map(|line| (dead, line)));
            let ((dead_summon, surfacing_line), effect) = match summons {
                Ok(summons) => (summons, effect),
                Err(why) => ((None, None), Err(why)),
            };
            let (mech_group, effect) = match mech_group_of(&row, &who) {
                Ok(group) => (group, effect),
                Err(why) => (None, Err(why)),
            };
            let (rebirth, effect) = match rebirth_of(&row, &who) {
                Ok(rebirth) => (rebirth, effect),
                Err(why) => (None, Err(why)),
            };
            // `isOnlyRecoverMech` keeps a `FightMech`, which is every target
            // `CalculateRangeActors` finds without buildings; without it, a
            // target that is not one is not read.
            let effect = if row.kind == REPAIR && !row.repair_only_mech {
                Err(format!(
                    "{who} repairs targets beside units, which is not read"
                ))
            } else {
                effect
            };
            let (kill_explosion, effect) = match kill_explosion_of(&row, &who, can_disable) {
                Ok(explosion) => (explosion, effect),
                Err(why) => (None, Err(why)),
            };
            let (clear_range_item, effect) = match clear_range_item_of(&row, &who) {
                Ok(clear) => (clear, effect),
                Err(why) => (None, Err(why)),
            };
            let self_buff = buff_source.as_ref().is_some_and(adds_its_unit_a_buff);
            let technology = Technology {
                unit: row.unit.clone(),
                exp_rate: row.exp_rate.clone(),
                effect,
                lifesteal,
                auto_recovery,
                energy_shield,
                stealth: row.stealth.map(|block| block.source(can_disable)),
                siege_mode: row.siege_mode.map(SiegeModeBlock::source),
                wreckage: row.wreckage.clone().map(|block| WreckageRecovery {
                    time_q32: block.time,
                    distance: block.distance,
                    can_disable: true,
                }),
                rebirth,
                dead_acid: row.dead_acid.as_ref().map(|block| {
                    super::buffs::acid_terrain(
                        &format!("technology {} ({})", row.id, row.name),
                        (block.range, 0, block.rounds),
                        &block.buff,
                    )
                }),
                burrow: (row.kind == BURROW).then(|| Burrow {
                    technology: u32::try_from(row.id).unwrap_or(u32::MAX),
                    amplify_damage_rate: row.burrow_amplify_damage_rate.clone(),
                    relieve_distance: row.burrow_relieve_distance.clone(),
                }),
                rvo_radius_change: (row.kind == RVO_RADIUS_CHANGE).then_some(RvoRadiusChange {
                    move_radius_q32: row.rvo_move_radius,
                    near_target_threshold_q32: row.rvo_near_target_threshold,
                }),
                clear_range_item,
                // `isOnlyRecoverMech` keeps a `FightMech`, which every target
                // `CalculateRangeActors` finds without buildings is.
                repair: (row.kind == REPAIR).then(|| Repair {
                    life: row.repair_life.clone(),
                    max_life_rate_q32: row.repair_max_life_rate,
                    interval_q32: row.repair_interval,
                    range_q32: row.repair_range,
                    enemies: row.repair_enemy,
                    air: row.repair_air,
                }),
                kill_explosion,
                dead_line: (row.kind == DEAD_LINE)
                    .then(|| (row.dead_line_value.clone(), row.dead_line_ignores_shield)),
                mech_group,
                carried_shield: (row.kind == BARRIER)
                    .then(|| (row.barrier_energy.clone(), row.barrier_radius.clone())),
                fire_intensify: (row.kind == FIRE_INTENSIFY)
                    .then(|| (row.fire_range.clone(), row.fire_life_time.clone())),
                reactive_armor: (row.kind == REACTIVE_ARMOR).then_some(ReactiveArmor {
                    rate_q32: row.reactive_armor_rate,
                    count: row.reactive_armor_count,
                }),
                move_ability_attack: (row.kind == MOVE_ABILITY_ATTACK).then_some(
                    MoveAbilityAttack {
                        exit_time_rate_q32: row.exit_time_rate,
                        trigger_count: row.strike_trigger_count,
                        damage_rate_q32: row.strike_damage_rate,
                        splash_range_q32: row.strike_splash_range,
                    },
                ),
                move_ability_range_item: (row.kind == MOVE_ABILITY_RANGE_ITEM).then_some(
                    MoveAbilityRangeItem {
                        range: row.range_item_range,
                        life_time: row.range_item_life_time,
                        attack_range_rate: row.fog_attack_range_rate,
                        remote_damage_rate: row.reduce_damage_from_remote,
                    },
                ),
                sweep,
                reduce_damage,
                distance_intensify: row.kind == SEARCH_TARGET_SPECIFIC,
                air_attack: (row.kind == AIR_ATTACK).then_some(AirAttack {
                    extra_skills: row.extra_skill_effect,
                }),
                buff_source,
                interception,
                production,
                dead_summon,
                surfacing_line,
                secondary_damage: (row.kind == SECONDARY_DAMAGE).then_some(SecondaryDamage {
                    damage: row.secondary_damage,
                    splash_radius: effects::fixed_to(row.secondary_splash_range, effects::METERS),
                    hits_main_target: row.secondary_hits_main_target,
                    buffed: row.secondary_buffed,
                }),
                provider: provider_of(&row.kind, self_buff),
                can_disable,
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
            sources.single.stealth = sources.single.stealth.or(technology.stealth);
            if sources.single.siege_mode.is_none() {
                sources.single.siege_mode.clone_from(&technology.siege_mode);
            }
            if sources.single.wreckage.is_none() {
                sources.single.wreckage.clone_from(&technology.wreckage);
            }
            if let Some(rebirth) = &technology.rebirth {
                if sources.single.rebirth.is_some() {
                    return Err(Error::new(format!(
                        "unit type {unit_type:?} is brought back by two technologies, \
                         which is not measured"
                    )));
                }
                sources.single.rebirth = Some(rebirth.clone());
            }
            sources.single.rvo_radius_change = sources
                .single
                .rvo_radius_change
                .or(technology.rvo_radius_change);
            if sources.single.clear_range_item.is_none() {
                sources
                    .single
                    .clear_range_item
                    .clone_from(&technology.clear_range_item);
            }
            if sources.single.repair.is_none() {
                sources.single.repair.clone_from(&technology.repair);
            }
            if sources.single.kill_explosion.is_none() {
                sources
                    .single
                    .kill_explosion
                    .clone_from(&technology.kill_explosion);
            }
            if sources.single.burrow.is_none() {
                sources.single.burrow.clone_from(&technology.burrow);
            }
            if sources.single.dead_acid.is_none()
                && let Some(acid) = &technology.dead_acid
            {
                sources.single.dead_acid = Some(acid.clone().map_err(Error::new)?);
            }
            sources.buff_sources.extend(technology.buff_source);
            sources.interception.extend(technology.interception);
            if let Some(dead_summon) = &technology.dead_summon {
                if sources.dead_summon.is_some() {
                    return Err(Error::new(format!(
                        "unit type {unit_type:?} summons where it dies by two technologies, \
                         which is not measured"
                    )));
                }
                sources.dead_summon = Some(dead_summon.clone());
            }
            if let Some(line) = &technology.surfacing_line {
                if sources.surfacing_line.is_some() {
                    return Err(Error::new(format!(
                        "unit type {unit_type:?} makes units as it surfaces by two \
                         technologies, which is not measured"
                    )));
                }
                sources.surfacing_line = Some(line.clone());
            }
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

    /// The production lines this side's technologies hand one unit type,
    /// as `SupportUnitEffectProvider` hands a production item's.
    ///
    /// # Errors
    ///
    /// Returns the error [`Self::corrections`] does.
    pub(crate) fn production(&self, held: &[i32], unit_type: &str) -> Result<Vec<ProductionLine>> {
        self.effects(held, unit_type)?;
        Ok(held
            .iter()
            .filter_map(|id| self.technologies.get(id))
            .filter(|technology| technology.unit == unit_type)
            .filter_map(|technology| technology.production.clone())
            .collect())
    }

    /// What the first of this side's technologies on one unit type that is
    /// an `IMoveAbilityRangeItem` answers.
    ///
    /// # Errors
    ///
    /// Returns the error [`Self::corrections`] does.
    pub(crate) fn move_ability_range_item(
        &self,
        held: &[i32],
        unit_type: &str,
    ) -> Result<Option<MoveAbilityRangeItem>> {
        self.effects(held, unit_type)?;
        Ok(held
            .iter()
            .filter_map(|id| self.technologies.get(id))
            .filter(|technology| technology.unit == unit_type)
            .find_map(|technology| technology.move_ability_range_item))
    }

    /// What the first of this side's technologies on one unit type that is
    /// an `IMoveAbilityAttackIntensify` answers.
    ///
    /// # Errors
    ///
    /// Returns the error [`Self::corrections`] does.
    pub(crate) fn move_ability_attack(
        &self,
        held: &[i32],
        unit_type: &str,
    ) -> Result<Option<MoveAbilityAttack>> {
        self.effects(held, unit_type)?;
        Ok(held
            .iter()
            .filter_map(|id| self.technologies.get(id))
            .filter(|technology| technology.unit == unit_type)
            .find_map(|technology| technology.move_ability_attack))
    }

    /// What the first of this side's technologies on one unit type that is
    /// an `IDeadLineDataSource` answers at the unit's level:
    /// `DeadLineTech.GetDeadLineValue` reads entry `CardLevel`, counting from
    /// zero, and its last past it.
    ///
    /// # Errors
    ///
    /// Returns the error [`Self::corrections`] does.
    pub(crate) fn dead_line(
        &self,
        held: &[i32],
        unit_type: &str,
        level: i64,
    ) -> Result<Option<DeadLine>> {
        self.effects(held, unit_type)?;
        Ok(held
            .iter()
            .filter_map(|id| self.technologies.get(id))
            .filter(|technology| technology.unit == unit_type)
            .find_map(|technology| technology.dead_line.as_ref())
            .and_then(|(values, ignores_shield)| {
                let index = usize::try_from(level - 1).unwrap_or_default();
                values
                    .get(index)
                    .or_else(|| values.last())
                    .map(|&life| DeadLine {
                        life,
                        ignores_shield: *ignores_shield,
                    })
            }))
    }

    /// What the first of this side's technologies on one unit type that is
    /// an `IMechGroupSource` answers it with at the unit's level: its share
    /// distance is `TechnologyData.GetLevelValue`'s, entry `GetLevel()`,
    /// counting from zero, and its last past it.
    ///
    /// # Errors
    ///
    /// Returns the error [`Self::corrections`] does.
    pub(crate) fn mech_group(
        &self,
        held: &[i32],
        unit_type: &str,
        level: i64,
    ) -> Result<Option<MechGroup>> {
        self.effects(held, unit_type)?;
        Ok(held
            .iter()
            .filter_map(|id| self.technologies.get(id))
            .filter(|technology| technology.unit == unit_type)
            .find_map(|technology| technology.mech_group.as_ref())
            .and_then(|(values, group)| {
                let index = usize::try_from(level - 1).unwrap_or_default();
                values
                    .get(index)
                    .or_else(|| values.last())
                    .map(|&distance_q32| MechGroup {
                        distance_q32,
                        ..*group
                    })
            }))
    }

    /// The `FPoint` metres and seconds of the fire the first of this side's
    /// technologies on one unit type that is an `IFireIntensify` answers at
    /// the unit's level: `FireIntensifyTechnologyData.GetRange` and
    /// `GetLIfeTime` read entry `GetLevel()` of their lists, their last past
    /// it, and zero for an empty list.
    ///
    /// # Errors
    ///
    /// Returns the error [`Self::corrections`] does.
    pub(crate) fn fire_intensify(
        &self,
        held: &[i32],
        unit_type: &str,
        level: i64,
    ) -> Result<Option<[i64; 2]>> {
        self.effects(held, unit_type)?;
        let index = usize::try_from(level - 1).unwrap_or_default();
        let at_level = |values: &Vec<i64>| values.get(index).or_else(|| values.last()).copied();
        Ok(held
            .iter()
            .filter_map(|id| self.technologies.get(id))
            .filter(|technology| technology.unit == unit_type)
            .find_map(|technology| technology.fire_intensify.as_ref())
            .map(|(range, life)| {
                [
                    at_level(range).unwrap_or_default(),
                    at_level(life).unwrap_or_default(),
                ]
            }))
    }

    /// What the first of this side's technologies on one unit type that is
    /// an `IReactiveArmorTechDataSource` answers. Its `GetDamageReduceValue`
    /// reads a list no row sets, which `special` would name.
    ///
    /// # Errors
    ///
    /// Returns the error [`Self::corrections`] does.
    pub(crate) fn reactive_armor(
        &self,
        held: &[i32],
        unit_type: &str,
    ) -> Result<Option<ReactiveArmor>> {
        self.effects(held, unit_type)?;
        Ok(held
            .iter()
            .filter_map(|id| self.technologies.get(id))
            .filter(|technology| technology.unit == unit_type)
            .find_map(|technology| technology.reactive_armor))
    }

    /// The battlefield shield the first of this side's technologies on one
    /// unit type that is an `IAdvancedEnergyShieldSource` makes the unit
    /// carry at its level: `AdvancedEnergyShieldTech.GetShieldValue` and
    /// `GetRadius` read their lists with `TechnologyData.GetLevelValue`,
    /// entry `GetLevel()` counting from zero and its last past it, and zero
    /// for an empty list.
    ///
    /// # Errors
    ///
    /// Returns the error [`Self::corrections`] does.
    pub(crate) fn carried_shield(
        &self,
        held: &[i32],
        unit_type: &str,
        level: i64,
    ) -> Result<Option<CarriedShield>> {
        self.effects(held, unit_type)?;
        let index = usize::try_from(level - 1).unwrap_or_default();
        let at_level = |values: &Vec<i64>| values.get(index).or_else(|| values.last()).copied();
        Ok(held
            .iter()
            .filter_map(|id| self.technologies.get(id))
            .filter(|technology| technology.unit == unit_type)
            .find_map(|technology| technology.carried_shield.as_ref())
            .map(|(energy, radius)| CarriedShield {
                radius: at_level(radius).unwrap_or_default(),
                energy: at_level(energy).unwrap_or_default(),
            }))
    }

    /// The technologies this side holds for one unit type that a buff that
    /// disables technology switches (`Technology.CanDisable`), in the order
    /// the side holds them.
    pub(crate) fn switched(&self, held: &[i32], unit_type: &str) -> Vec<i32> {
        held.iter()
            .copied()
            .filter(|id| {
                self.technologies.get(id).is_some_and(|technology| {
                    technology.unit == unit_type && technology.can_disable
                })
            })
            .collect()
    }

    /// The providers beside the numbers' this side's technologies on one
    /// unit type reach, each once, with the technologies that reach it.
    pub(crate) fn providers(
        &self,
        held: &[i32],
        unit_type: &str,
    ) -> BTreeMap<EffectProvider, Vec<i32>> {
        let mut providers = BTreeMap::<EffectProvider, Vec<i32>>::new();
        for &id in held {
            if let Some(provider) = self
                .technologies
                .get(&id)
                .filter(|technology| technology.unit == unit_type)
                .and_then(|technology| technology.provider)
            {
                providers.entry(provider).or_default().push(id);
            }
        }
        providers
    }

    /// The rate this side's technologies put on what one unit of this type
    /// gains. `TechnologyData.GetExpChangeRate` answers the unit's own
    /// `TryAddCommonData` with its entry at the unit's level, so the rate is
    /// the unit's `MechDataChangeFloatRate.ExpChangeRate` and not its card's,
    /// whose `IUnitDataChangeDataSource` a technology is not.
    ///
    /// # Errors
    ///
    /// Returns the error [`Self::corrections`] does.
    pub(crate) fn experience_rate(
        &self,
        held: &[i32],
        unit_type: &str,
        level: i64,
    ) -> Result<ExperienceRate> {
        self.effects(held, unit_type)?;
        Ok(held
            .iter()
            .filter_map(|id| self.technologies.get(id))
            .filter(|technology| technology.unit == unit_type)
            .filter_map(|technology| {
                let last = technology.exp_rate.last()?;
                Some(
                    *usize::try_from(level - 1)
                        .ok()
                        .and_then(|index| technology.exp_rate.get(index))
                        .unwrap_or(last),
                )
            })
            .fold(ExperienceRate::default(), ExperienceRate::with))
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

/// What an interception row makes its unit, and the line a production row
/// hands it, or why this build will not.
fn subclass_of(
    row: &Row,
    who: &str,
) -> std::result::Result<(Option<UnitInterception>, Option<ProductionLine>), String> {
    let interception = if row.kind == INTERCEPT {
        let block = row
            .intercept
            .ok_or_else(|| format!("{who} carries no interception"))?;
        Some(block.interception(who)?)
    } else {
        None
    };
    let production = if row.kind == SUPPORT {
        let block = row
            .production
            .as_ref()
            .ok_or_else(|| format!("{who} carries no production line"))?;
        Some(block.line(who)?)
    } else {
        None
    };
    Ok((interception, production))
}

/// What a grouping row answers `IMechGroupSource` with. A group that raises
/// its members' damage names their main skill alone
/// (`MechGroupInternal.RefreshMechData` writes each skill the row's
/// `ISkillDataChangeDataSource` names); one on their extra skills is
/// refused.
fn mech_group_of(
    row: &Row,
    who: &str,
) -> std::result::Result<Option<(Vec<i64>, MechGroup)>, String> {
    if row.kind != DAMAGE_SHARE {
        return Ok(None);
    }
    let purpose = match row.group_purpose {
        0 => GroupPurpose::DamageShare,
        1 if row.extra_skill_effect => {
            return Err(format!("{who} raises its units' extra skills' damage"));
        }
        1 => GroupPurpose::DamageChange,
        other => return Err(format!("{who} groups its units for purpose {other}")),
    };
    Ok(Some((
        row.share_distance.clone(),
        MechGroup {
            distance_q32: 0,
            purpose,
            damage_rate_q32: if row.main_skill_effect {
                row.group_damage_rate
            } else {
                0
            },
            max_count: row.group_max_count,
        },
    )))
}

/// Whether a buff technology's buff goes on its own unit: one of a range
/// cycle that reaches only it, or one its unit's hit or being hit adds.
fn adds_its_unit_a_buff(buff: &BuffSource) -> bool {
    matches!(
        buff.trigger,
        crate::modifier::BuffTrigger::All(crate::modifier::AllCycle { reach: None, .. })
            | crate::modifier::BuffTrigger::Hit
            | crate::modifier::BuffTrigger::BeHit
            | crate::modifier::BuffTrigger::Damaged
    )
}

/// What a row writes at each level, or why this build will not apply it.
/// The provider beside the numbers' a row's class reaches: the interface it
/// answers beside `IDataModifier`. A plain, mobility, damage intensify,
/// splash or multi-attack technology answers none, its class a
/// `Technology` or a `DataModifyTech` alone.
fn provider_of(kind: &str, self_buff: bool) -> Option<EffectProvider> {
    Some(match kind {
        LIFESTEAL => EffectProvider::LifeSteal,
        AUTO_RECOVERY => EffectProvider::AutoRecovery,
        ENERGY_SHIELD => EffectProvider::EnergyShield,
        SWEEP => EffectProvider::SweepSkillIntensify,
        ARMOR => EffectProvider::ArmorStrengthen,
        SEARCH_TARGET_SPECIFIC => EffectProvider::SearchTargetSpecific,
        AIR_ATTACK => EffectProvider::AirAttack,
        SECONDARY_DAMAGE => EffectProvider::SecondaryDamageIntensify,
        BUFF => EffectProvider::Buff { cycles: !self_buff },
        INTERCEPT => EffectProvider::InterceptMissile,
        SUPPORT => EffectProvider::SupportUnit,
        DEAD_SUMMON | REBIRTH | DEAD_ACID => EffectProvider::DeadEffect,
        MOVE_SUMMON => EffectProvider::MoveAbilitySummon,
        EXTRA_WEAPON => EffectProvider::ExtraSkill,
        STEALTH => EffectProvider::StealthTech,
        DEAD_LINE => EffectProvider::DeadLine,
        MOVE_ABILITY_ATTACK => EffectProvider::MoveAbilityAttackIntensify,
        MOVE_ABILITY_RANGE_ITEM => EffectProvider::MoveAbilityRangeItem,
        DAMAGE_SHARE => EffectProvider::MechGroup,
        BARRIER => EffectProvider::AdvancedEnergyShield,
        REACTIVE_ARMOR => EffectProvider::ReactiveArmor,
        FIRE_INTENSIFY => EffectProvider::FireIntensify,
        SIEGE => EffectProvider::SiegeMode,
        WRECKAGE => EffectProvider::WreckageRecovery,
        RVO_RADIUS_CHANGE => EffectProvider::RvoRadiusChange,
        CLEAR_RANGE_ITEM => EffectProvider::ClearRangeItem,
        REPAIR => EffectProvider::Repair,
        KILL_EXPLOSION => EffectProvider::KillExplosion,
        BURROW => EffectProvider::Burrow,
        _ => return None,
    })
}

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
    if row.kind == MOVE_ABILITY_RANGE_ITEM
        && (row.range_item_time != ON_EXIT_MOVE_END || row.range_item_move_type != UNDERGROUND_MOVE)
    {
        return Err(format!(
            "technology {} ({}) leaves its terrain at MoveAbilityTimeType {} of MechMoveType \
             {}, and only as an underground unit ends a surfacing is read",
            row.id, row.name, row.range_item_time, row.range_item_move_type
        ));
    }
    if row.burrow_enters_underground {
        return Err(format!(
            "technology {} ({}) takes its unit underground as it burrows, which is not \
             measured",
            row.id, row.name
        ));
    }
    if row.strike_attack_point != 0 {
        return Err(format!(
            "technology {} ({}) changes the attack point of its unit's first attacks after \
             surfacing, which is not measured",
            row.id, row.name
        ));
    }
    if row.secondary_disables_technology || row.secondary_buff_id != 0 {
        return Err(format!(
            "technology {} ({}) disables the technologies of the units its second damage \
             strikes, or writes a buff on them, which is not measured",
            row.id, row.name
        ));
    }
    if row.kind == AUTO_RECOVERY && ![NORMAL, UNDERGROUND].contains(&row.auto_recovery_state_type) {
        return Err(format!(
            "technology {} ({}) repairs only in autoRecoveryStateType {}, cloaked, which no \
             mechanism here reads",
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

    let levels = [
        &row.life_rate,
        &row.damage_rate,
        &row.speed_value,
        &row.min_attack_range_value,
        &row.attack_range_value,
        &row.attack_range_rate,
        &row.attack_interval_value,
        &row.attack_interval_rate,
        &row.splash_range_value,
        &row.splash_range,
        &row.projectile_speed_value,
        &row.projectile_life_rate,
        &row.air_damage_change_rate,
        &row.ground_damage_change_rate,
        &row.projectile_count_value,
        &row.projectile_duration_value,
        &row.projectile_random_range_value,
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
    written.extend(effects::corrections(&Fields {
        life_rate: at_level(&row.life_rate),
        damage_rate: at_level(&row.damage_rate),
        // The table has no such column.
        damage_rate_by_kill_count: None,
        attack_range_rate: at_level(&row.attack_range_rate),
        attack_interval_rate: at_level(&row.attack_interval_rate),
        attack_range_value: at_level(&row.attack_range_value),
        attack_interval_value: at_level(&row.attack_interval_value),
        // `SplashTech.AddData` adds its range to what the row's numbers add,
        // through `SkillDataModifier.AddData` as they go.
        splash_range_value: [
            at_level(&row.splash_range_value),
            at_level(&row.splash_range),
        ]
        .into_iter()
        .flatten()
        .reduce(i64::saturating_add),
        speed_value: at_level(&row.speed_value),
        damage_reduce_rate_base: Some(row.all_weapon_reduce_damage_rate),
        projectile_speed_value: at_level(&row.projectile_speed_value),
        projectile_life_rate: at_level(&row.projectile_life_rate),
        // `MultiAttackTech.AddData` writes them through
        // `DataModifyTech.AddSkillData` and `SkillDataModifier.AddData`.
        projectile_count_value: at_level(&row.projectile_count_value),
        projectile_duration_value: at_level(&row.projectile_duration_value),
        projectile_random_range_value: at_level(&row.projectile_random_range_value),
        inverse_lock_target: row.inverse_lock_target,
        min_attack_range_value: at_level(&row.min_attack_range_value),
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

    /// High-Explosive Ammo's range lands in the skill's splash beside its
    /// damage rate, as a splash value does: 7 metres on the Wasp.
    #[test]
    fn a_splash_technology_adds_its_range_to_the_splash() {
        let table = TechnologyEffects::load().unwrap();
        let written = table.corrections(&[406], "wasp", 1).unwrap();
        let splash = written
            .iter()
            .find(|(channel, entry)| {
                *channel == Channel::Skill && entry.index == Index::SplashRange
            })
            .map(|(_, entry)| entry.correction);
        assert_eq!(splash, Some(Correction::Value(7_000)), "{written:?}");
        assert!(
            written
                .iter()
                .any(|(_, entry)| entry.index == Index::AttackDamage),
            "{written:?}"
        );
    }

    /// A multi-attack technology writes its count, duration and random
    /// range into the skill's `DataSet` beside its numbers.
    #[test]
    fn a_multi_attack_technology_adds_to_the_burst() {
        let table = TechnologyEffects::parse(
            "schema: mechcore.technology_effects\n\
             technologies:\n\
             - {id: 9, name: probe, unit: farseer, kind: multiAttackTechnologies, \
             attack_interval_rate: [4294967296], projectile_count_value: [10], \
             projectile_duration_value: [-429496729], \
             projectile_random_range_value: [55834574848]}\n",
        )
        .unwrap();
        let written = table.corrections(&[9], "farseer", 1).unwrap();
        let value = |index| {
            written
                .iter()
                .find(|(channel, entry)| *channel == Channel::Skill && entry.index == index)
                .map(|(_, entry)| entry.correction)
        };
        assert_eq!(value(Index::ProjectileCount), Some(Correction::Value(10)));
        assert_eq!(
            value(Index::ProjectileDuration),
            Some(Correction::Value(-429_496_729))
        );
        assert_eq!(
            value(Index::ProjectileRandomRange),
            Some(Correction::Value(13_000))
        );
        assert!(value(Index::AttackInterval).is_some(), "{written:?}");
    }

    /// A technology that turns its unit's locking over keeps one inversion,
    /// and its minimum range lands in the skill's `DataSet` in millimetres;
    /// the inversion turns a locking skill off and a free one on.
    #[test]
    fn an_inverting_technology_turns_the_lock_over() {
        let table = TechnologyEffects::parse(
            "schema: mechcore.technology_effects\n\
             technologies:\n\
             - {id: 9, name: probe, unit: scorpion, kind: technologyDatas, \
             inverse_lock_target: true, min_attack_range_value: [75]}\n",
        )
        .unwrap();
        let written = table.corrections(&[9], "scorpion", 1).unwrap();
        let mut skill = crate::data::Overlay::default();
        for (channel, entry) in &written {
            assert_eq!(*channel, Channel::Skill, "{written:?}");
            skill.write(*entry);
        }
        assert_eq!(skill.value(Index::MinAttackRange), 75_000);
        assert!(!crate::data::lock_target(&skill, true, true));
        assert!(crate::data::lock_target(&skill, false, false));
        // A skill that locks under a main skill that does not is written 1.
        assert!(!crate::data::lock_target(&skill, false, true));
    }

    /// A stealth technology hands its unit the share of its life and the
    /// duration `StealthTechSystem` reads, and nothing else.
    #[test]
    fn a_stealth_technology_hands_its_unit_a_source() {
        let table = TechnologyEffects::parse(
            "schema: mechcore.technology_effects\n\
             technologies:\n\
             - {id: 9, name: probe, unit: vortex, kind: stealthTechData, \
             stealth: {trigger_life_rate: 2147483648, duration: 17179869184}}\n",
        )
        .unwrap();
        assert!(table.corrections(&[9], "vortex", 1).unwrap().is_empty());
        let sources = table.sources(&[9], "vortex").unwrap();
        assert_eq!(
            sources.single.stealth,
            Some(super::Stealth {
                trigger_life_rate_q32: 1 << 31,
                duration_q32: 4 << 32,
                can_disable: true,
            })
        );
        assert_eq!(
            table
                .providers(&[9], "vortex")
                .into_keys()
                .collect::<Vec<_>>(),
            [super::EffectProvider::StealthTech]
        );
    }

    /// A siege-mode technology writes nothing as the fight starts: it hands
    /// its unit what `SiegeModeEffectSystem` writes while the unit is dug
    /// in, its life rate on the unit and the rest on its skill, and the two
    /// times.
    #[test]
    fn a_siege_mode_technology_hands_its_unit_a_trench() {
        let table = TechnologyEffects::parse(
            "schema: mechcore.technology_effects\n\
             technologies:\n\
             - {id: 9, name: probe, unit: sabertooth, kind: siegeModeTechDatas, \
             siege_mode: {life_rate: 2147483648, attack_interval_rate: -858993459, \
             attack_interval_value: 0, damage_rate: 0, attack_range_value: 85899345920, \
             attack_range_rate: 0, splash_range_value: 0, projectile_speed_value: 0, \
             duration: 30064771072, animation_delay: 1288490188}}\n",
        )
        .unwrap();
        assert!(table.corrections(&[9], "sabertooth", 1).unwrap().is_empty());
        let trench = table
            .sources(&[9], "sabertooth")
            .unwrap()
            .single
            .siege_mode
            .unwrap();
        assert_eq!(
            trench.written,
            [
                (
                    Channel::Skill,
                    Index::AttackInterval,
                    Correction::Rate {
                        add: 0,
                        reduce: 858_993_459,
                    },
                ),
                (
                    Channel::Unit,
                    Index::MaxLife,
                    Correction::Rate {
                        add: 1 << 31,
                        reduce: 0,
                    },
                ),
                (
                    Channel::Skill,
                    Index::AttackRange,
                    Correction::Value(20_000),
                ),
            ]
        );
        assert_eq!(
            (trench.duration_q32, trench.animation_delay_q32),
            (7 << 32, 1_288_490_188)
        );
        assert_eq!(
            table
                .providers(&[9], "sabertooth")
                .into_keys()
                .collect::<Vec<_>>(),
            [super::EffectProvider::SiegeMode]
        );
    }

    /// A wreckage-recovery technology writes its numbers and hands its unit
    /// the time a strike counts and the distance by level.
    #[test]
    fn a_wreckage_recovery_technology_hands_its_unit_a_source() {
        let table = TechnologyEffects::parse(
            "schema: mechcore.technology_effects\n\
             technologies:\n\
             - {id: 9, name: probe, unit: rhino, kind: wreckageRecoveryTechnologies, \
             wreckage: {time: 8589934592, distance: [15]}, damage_rate: [2576980377]}\n",
        )
        .unwrap();
        assert_eq!(table.corrections(&[9], "rhino", 1).unwrap().len(), 1);
        assert_eq!(
            table.sources(&[9], "rhino").unwrap().single.wreckage,
            Some(super::WreckageRecovery {
                time_q32: 2 << 32,
                distance: vec![15],
                can_disable: true,
            })
        );
        assert_eq!(
            table
                .providers(&[9], "rhino")
                .into_keys()
                .collect::<Vec<_>>(),
            [super::EffectProvider::WreckageRecovery]
        );
    }

    /// A rebirth technology hands its unit what brings it back, through
    /// `DeadEffectProvider`, and one whose pilot follows an ally how it
    /// flies.
    #[test]
    fn a_rebirth_technology_hands_its_unit_a_source() {
        let table = TechnologyEffects::parse(
            "schema: mechcore.technology_effects\n\
             technologies:\n\
             - {id: 9, name: probe, unit: typhoon, kind: rebirthEffectTechologyDatas, \
             rebirth: {cost_time: 5, count: 1, rebirthing_time: 1.5, unit_id: 22, unit_count: 1, follow_others: false, interval_x: 0, interval_z: 0, interval_from_mech_center_z: 0, random_offset_range: [0, 0, 0], follow_start_offset_range: [0, 0, 0], transfer_distance_min: 0, speed_in_transfer: 0, per_r: 0, follow_rate: 0, follow_rotate_rate: 0}}\n\
             - {id: 10, name: pilot, unit: phoenix, kind: rebirthEffectTechologyDatas, \
             rebirth: {cost_time: 5, count: 1, rebirthing_time: 1.5, unit_id: 16, unit_count: 1, follow_others: true, interval_x: 0, interval_z: 0, interval_from_mech_center_z: 0, random_offset_range: [0, 0, 0], follow_start_offset_range: [0, 0, 0], transfer_distance_min: 0, speed_in_transfer: 0, per_r: 0, follow_rate: 0, follow_rotate_rate: 0}}\n",
        )
        .unwrap();
        assert_eq!(
            table.sources(&[9], "typhoon").unwrap().single.rebirth,
            Some(super::Rebirth {
                cost_seconds: 5,
                count: 1,
                rebirthing_q32: 3 << 31,
                follow: None,
            })
        );
        assert_eq!(
            table
                .providers(&[9], "typhoon")
                .into_keys()
                .collect::<Vec<_>>(),
            [super::EffectProvider::DeadEffect]
        );
        assert!(
            table
                .sources(&[10], "phoenix")
                .unwrap()
                .single
                .rebirth
                .is_some_and(|rebirth| rebirth.follow.is_some())
        );
    }

    /// An acid technology hands its unit the acid it leaves where it dies,
    /// through `DeadEffectProvider`.
    #[test]
    fn an_acid_technology_hands_its_unit_an_acid() {
        let table = TechnologyEffects::parse(
            "schema: mechcore.technology_effects\n\
             technologies:\n\
             - {id: 9, name: probe, unit: crawler, kind: deadAcidRangeItemTechnologyDatas, \
             dead_acid: {range: 9, rounds: 1, buff: {id: 500001, name: acid, duration: 4294967296, \
             divide: 0, additive: false, debuff: true, invincible: false, disable_technology: false, \
             amplify_damage_rate: 6442450944, damage_rate: 0, speed_rate: 0, attack_range_value: 0, \
             attack_range_rate: 0, max_life_rate: 0, step_time: 2147483648, additive_effect: false, \
             additive_condition: 0, additive_condition_param: 0, max_additive_stack: 0, \
             clear_when_technologies_disabled: true, summon_unit: 0, summon_level_inherit: false, \
             life_change_rate: -64424509, disable_recover: false, additive_reset_condition: 0}}}\n",
        )
        .unwrap();
        let acid = table
            .sources(&[9], "crawler")
            .unwrap()
            .single
            .dead_acid
            .unwrap();
        assert_eq!(acid.kind, crate::layout::TerrainKind::Acid);
        assert_eq!(acid.radius_q32, 9 << 32);
        assert_eq!(acid.rounds, 1);
        assert_eq!(
            table
                .providers(&[9], "crawler")
                .into_keys()
                .collect::<Vec<_>>(),
            [super::EffectProvider::DeadEffect]
        );
    }

    /// A burrowing technology writes its numbers and hands its unit a
    /// burrow; one that takes its unit underground is refused by name.
    #[test]
    fn a_burrowing_technology_hands_its_unit_a_burrow() {
        let table = TechnologyEffects::parse(
            "schema: mechcore.technology_effects\n\
             technologies:\n\
             - {id: 9, name: probe, unit: crawler, kind: burrowTechnologies, \
             burrow_enters_underground: false, burrow_amplify_damage_rate: [-1717986918], \
             burrow_relieve_distance: [214748364800], speed_value: [3]}\n\
             - {id: 10, name: below, unit: crawler, kind: burrowTechnologies, \
             burrow_enters_underground: true, burrow_amplify_damage_rate: [-1717986918], \
             burrow_relieve_distance: [214748364800]}\n",
        )
        .unwrap();
        assert_eq!(table.corrections(&[9], "crawler", 1).unwrap().len(), 1);
        assert_eq!(
            table.sources(&[9], "crawler").unwrap().single.burrow,
            Some(super::Burrow {
                technology: 9,
                amplify_damage_rate: vec![-1_717_986_918],
                relieve_distance: vec![50 << 32],
            })
        );
        assert!(
            table
                .corrections(&[10], "crawler", 1)
                .unwrap_err()
                .to_string()
                .contains("underground")
        );
    }

    /// A loose-formation technology writes its numbers and hands its unit's
    /// motion a radius change.
    #[test]
    fn a_loose_formation_technology_hands_its_unit_a_radius_change() {
        let table = TechnologyEffects::parse(
            "schema: mechcore.technology_effects\n\
             technologies:\n\
             - {id: 9, name: probe, unit: crawler, kind: rVORadiusChangeTechnologyTechDatas, \
             rvo_move_radius: 14602888806, rvo_near_target_threshold: 107374182400, \
             life_rate: [-1717986918]}\n",
        )
        .unwrap();
        assert_eq!(table.corrections(&[9], "crawler", 1).unwrap().len(), 1);
        assert_eq!(
            table
                .sources(&[9], "crawler")
                .unwrap()
                .single
                .rvo_radius_change,
            Some(super::RvoRadiusChange {
                move_radius_q32: 14_602_888_806,
                near_target_threshold_q32: 25 << 32,
            })
        );
        assert_eq!(
            table
                .providers(&[9], "crawler")
                .into_keys()
                .collect::<Vec<_>>(),
            [super::EffectProvider::RvoRadiusChange]
        );
    }

    /// A repair technology hands its unit what it repairs with, and one that
    /// repairs targets beside units is refused.
    #[test]
    fn a_repair_technology_hands_its_unit_a_repair() {
        let table = TechnologyEffects::parse(
            "schema: mechcore.technology_effects\n\
             technologies:\n\
             - {id: 9, name: probe, unit: typhoon, kind: recoveryTechDatas, \
             repair_life: [500, 1000], repair_max_life_rate: 0, \
             repair_interval: 12884901888, repair_range: 429496729600, \
             repair_enemy: false, repair_only_mech: true, repair_air: true}\n\
             - {id: 10, name: towers, unit: typhoon, kind: recoveryTechDatas, \
             repair_life: [500], repair_interval: 12884901888, \
             repair_range: 429496729600, repair_only_mech: false}\n",
        )
        .unwrap();
        assert_eq!(
            table.sources(&[9], "typhoon").unwrap().single.repair,
            Some(super::Repair {
                life: vec![500, 1000],
                max_life_rate_q32: 0,
                interval_q32: 3 << 32,
                range_q32: 100 << 32,
                enemies: false,
                air: true,
            })
        );
        assert_eq!(
            table
                .providers(&[9], "typhoon")
                .into_keys()
                .collect::<Vec<_>>(),
            [super::EffectProvider::Repair]
        );
        assert!(table.corrections(&[10], "typhoon", 1).is_err());
    }

    /// A kill-explosion technology hands its unit what it sets off, and
    /// refuses an explosion of its extra skills or of a fraction of a space
    /// unit.
    #[test]
    fn a_kill_explosion_technology_hands_its_unit_an_explosion() {
        let table = TechnologyEffects::parse(
            "schema: mechcore.technology_effects\n\
             technologies:\n\
             - {id: 9, name: probe, unit: typhoon, kind: killExplosionTechDatas, \
             kill_explosion_range: 51539607552, kill_explosion_chains: false, \
             kill_explosion_hits_allies: true, kill_explosion_buffed: true, \
             main_skill_effect: true, extra_skill_effect: false, \
             kill_explosion_damage: [115]}\n\
             - {id: 10, name: extras, unit: typhoon, kind: killExplosionTechDatas, \
             kill_explosion_range: 51539607552, main_skill_effect: true, \
             extra_skill_effect: true, kill_explosion_damage: [115]}\n\
             - {id: 11, name: fraction, unit: typhoon, kind: killExplosionTechDatas, \
             kill_explosion_range: 1, main_skill_effect: true, \
             kill_explosion_damage: [115]}\n",
        )
        .unwrap();
        assert_eq!(
            table
                .sources(&[9], "typhoon")
                .unwrap()
                .single
                .kill_explosion,
            Some(super::KillExplosion {
                damage: vec![115],
                range: 12_000,
                chains: false,
                hits_allies: true,
                buffed: true,
                can_disable: true,
            })
        );
        assert_eq!(
            table
                .providers(&[9], "typhoon")
                .into_keys()
                .collect::<Vec<_>>(),
            [super::EffectProvider::KillExplosion]
        );
        assert!(table.corrections(&[10], "typhoon", 1).is_err());
        assert!(table.corrections(&[11], "typhoon", 1).is_err());
    }

    /// A fire-extinguisher technology hands its unit the terrain it clears.
    #[test]
    fn a_fire_extinguisher_technology_hands_its_unit_what_it_clears() {
        let table = TechnologyEffects::parse(
            "schema: mechcore.technology_effects\n\
             technologies:\n\
             - {id: 9, name: probe, unit: hound, kind: clearRangeItemTechDatas, \
             clear_radius: 40, clear_range_item_types: [0, 3, 2]}\n",
        )
        .unwrap();
        assert_eq!(
            table
                .sources(&[9], "hound")
                .unwrap()
                .single
                .clear_range_item,
            Some(super::ClearRangeItem {
                radius: 40,
                kinds: vec![
                    crate::layout::TerrainKind::Fire,
                    crate::layout::TerrainKind::Acid,
                    crate::layout::TerrainKind::Fog,
                ],
            })
        );
        assert_eq!(
            table
                .providers(&[9], "hound")
                .into_keys()
                .collect::<Vec<_>>(),
            [super::EffectProvider::ClearRangeItem]
        );
    }

    /// A dead-line technology answers the line at its unit's level, the
    /// first at level one and the last past its list.
    #[test]
    fn a_dead_line_technology_reads_its_line_by_level() {
        let table = TechnologyEffects::parse(
            "schema: mechcore.technology_effects\n\
             technologies:\n\
             - {id: 9, name: probe, unit: mustang, kind: deadLineTechDatas, \
             dead_line_ignores_shield: true, damage_rate: [-1503238553], \
             dead_line_value: [320, 520, 720]}\n",
        )
        .unwrap();
        let line = |level| table.dead_line(&[9], "mustang", level).unwrap();
        assert_eq!(
            line(1),
            Some(super::DeadLine {
                life: 320,
                ignores_shield: true,
            })
        );
        assert_eq!(line(3).map(|line| line.life), Some(720));
        assert_eq!(line(9).map(|line| line.life), Some(720));
        assert_eq!(table.dead_line(&[9], "rhino", 1).unwrap(), None);
        assert!(!table.corrections(&[9], "mustang", 1).unwrap().is_empty());
    }

    /// A technology of a list whose mechanism is not here is refused by name
    /// and kind, numbers and all.
    #[test]
    fn a_technology_that_does_more_than_numbers_is_refused() {
        let table = TechnologyEffects::parse(
            "schema: mechcore.technology_effects\n\
             technologies:\n\
             - {id: 9, name: probe, unit: wasp, kind: unreadTechnologies, damage_rate: [1]}\n",
        )
        .unwrap();
        let refused = table.corrections(&[9], "wasp", 1).unwrap_err().to_string();
        assert!(refused.contains("technology 9 (probe)"), "{refused}");
        assert!(refused.contains("unreadTechnologies"), "{refused}");
    }

    /// Best Partner hands the Vulcan a line of one Marksman at its level, and
    /// a line whose makes appear in a way not read is refused by name.
    #[test]
    fn a_production_technology_hands_its_unit_a_line() {
        let table = TechnologyEffects::load().unwrap();
        let lines = table.production(&[1203], "vulcan").unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].unit_type_id, 2);
        assert!(lines[0].parent_level);
        assert_eq!(lines[0].offsets, vec![(25 << 32, -30 << 32)]);
        let probe = TechnologyEffects::parse(
            "schema: mechcore.technology_effects\n\
             technologies:\n\
             - id: 9\n  name: probe\n  unit: vulcan\n  kind: supportUnitTechnologies\n  \
             production: {support_unit_id: 2, unit_level: 3, max_batch: 1, max_alive: 1, \
             create_count_per_time: 1, start_time: 0, appear_type: 3, product_time: 0, \
             max_create_count: 0, create_duration: 0, position_space: 1, unit_life_rate: 0, \
             unit_damage_rate: 0, unit_attack_range_value: 0, intensify_mode: false, \
             inherit_technology: true, positions: [{x: 0, z: 0}]}\n",
        )
        .unwrap();
        let refused = probe.production(&[9], "vulcan").unwrap_err().to_string();
        assert!(refused.contains("appearType"), "{refused}");
    }

    /// Machine Learning writes no correction, and doubles what its unit gains
    /// as the unit's own rate.
    #[test]
    fn machine_learning_rates_its_units_experience() {
        let table = TechnologyEffects::load().unwrap();
        assert!(
            table
                .corrections(&[MACHINE_LEARNING], "vortex", 1)
                .unwrap()
                .is_empty()
        );
        let rate = table
            .experience_rate(&[MACHINE_LEARNING], "vortex", 1)
            .unwrap();
        assert_eq!(rate.add, 1 << 32);
        assert_eq!(rate.remaining, 1 << 32);
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
