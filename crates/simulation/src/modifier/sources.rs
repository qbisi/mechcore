//! What an equipment or a technology hands its unit beyond its numbers.
//!
//! A subclass's `Equipment` or `Technology` is an `IEffectProviderDataSource`
//! of its own interface as well as a correction: `LifestealEquipment` and
//! `LifestealTech` are both `ILifeSteal`s, `AutoRecoveryEquipment` and
//! `AutoRecoveryTech` both `IAutoRecovery`s. The unit's `FightEffectMananger`
//! holds one `SingleEffectProvider` per interface, and it enables one of the
//! sources it is handed, its `Current`: `AddDataSource` sorts them by
//! `EffectProvider.IsOverrideEffect`, the higher `GetPriority` first, and
//! `Equipment.GetPriority` is 1 where `Technology`'s is 0. What the fight
//! reads of a source is what its interface answers, so an equipment and a
//! technology of one interface are one type here, and the fight never asks
//! which of the two it came from.

/// What an `ILifeSteal` answers, and what its provider asks of it as an
/// `IEffectProviderDataSource`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LifeSteal {
    /// `GetLifestealMuliplier`, Q32.32: the share of a hit's damage the unit
    /// takes back as life.
    pub(crate) multiplier_q32: i64,
    /// `GetPriority`: 1 for an equipment, 0 for a technology.
    pub(crate) priority: i32,
    /// `CanDisable`: whether a unit whose technologies are disabled loses
    /// it. `Equipment` answers false, and `Technology` true unless its row
    /// sets `ignoreElectricEffect`.
    pub(crate) can_disable: bool,
}

/// `AutoRecoveryStateType`: when an `IAutoRecovery` repairs. `Cloak`, which
/// no unit's move ability makes, is refused where a row is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryState {
    /// `Normal`: whenever its unit is hurt.
    Normal,
    /// `Underground`: only while its unit is below, from the end of its
    /// burrow (`OnEnterMoveEnd`) to the start of its surfacing
    /// (`OnExitMoveBegin`).
    Underground,
}

/// What an `IAutoRecovery` answers: it repairs while its unit is hurt and in
/// its state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AutoRecovery {
    /// `GetStartTime`, Q32.32 seconds.
    pub(crate) start_time_q32: i64,
    /// `GetRecoveryDuration`, Q32.32 seconds between two repairs.
    pub(crate) duration_q32: i64,
    /// `GetRecoveryLIfeRate`, Q32.32: the share of the unit's maximum life
    /// one repair restores.
    pub(crate) life_rate_q32: i64,
    /// `GetAutoRecoveryStateType`.
    pub(crate) state: RecoveryState,
    /// `GetPriority`: 1 for an equipment, 0 for a technology.
    pub(crate) priority: i32,
    /// `CanDisable`, as [`LifeSteal::can_disable`].
    pub(crate) can_disable: bool,
}

/// What an `IMoveAbilityAttackIntensify` answers, which
/// `MoveAbilityAttackIntensifyProvider` hands its unit: a rate on its
/// surfacing time, and an `AttackCountEffectLinker` on its main skill that
/// strengthens its first attacks after each surfacing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MoveAbilityAttack {
    /// `GetExitTimeChangeRate`, Q32.32: the rate `DoActive` writes into the
    /// unit's `MechDataChangeFloatRate.MoveAbilityExitTimeChangeRate`.
    pub(crate) exit_time_rate_q32: i64,
    /// `GetTriggerCount`: how many attacks after surfacing it strengthens.
    pub(crate) trigger_count: i32,
    /// `GetDamageChangeRateInCondition`, Q32.32: a rate on their damage.
    pub(crate) damage_rate_q32: i64,
    /// `GetSplashRangeChange`, Q32.32 metres added to their splash.
    pub(crate) splash_range_q32: i64,
}

/// What an `IMoveAbilityRangeItem` answers, which `MoveAbilityRangeItemSystem`
/// leaves as a sand fog (`RangeItemType.FogSand`) where its unit ends a
/// surfacing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MoveAbilityRangeItem {
    /// `GetRangeItemRange`, Q32.32 metres.
    pub(crate) range: i64,
    /// `GetLifeTime`, Q32.32 seconds.
    pub(crate) life_time: i64,
    /// `IFogSandProvider.GetAttackRangeChangeRate`, Q32.32.
    pub(crate) attack_range_rate: i64,
    /// `IFogSandProvider.GetReduceDamageFromRemote`, Q32.32.
    pub(crate) remote_damage_rate: i64,
}

/// What an `IStealthTechDataSource` answers: `StealthTechSystem` puts its
/// unit in stealth once its life first falls to a share of its maximum, for a
/// while.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Stealth {
    /// `GetTriggerConditionValue`, Q32.32: the share of its maximum life its
    /// life falls to.
    pub(crate) trigger_life_rate_q32: i64,
    /// `GetDuration`, Q32.32 seconds.
    pub(crate) duration_q32: i64,
    /// `CanDisable`, as [`LifeSteal::can_disable`].
    pub(crate) can_disable: bool,
}

/// A `BuffEquipment` or a `BuffTech` as `BuffEffectProvider` reads it: one
/// whose `BuffTechListener` is `FightStart` and which always triggers, the
/// targets its `BuffCycleController` gives the buff, and the `buffDatas` row
/// it adds. `BuffEffectProvider` holds every such source, not one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the buff row's and its source's flags are independent fields"
)]
pub(crate) struct BuffSource {
    pub(crate) buff_id: u32,
    pub(crate) divide: i32,
    pub(crate) additive: bool,
    /// `duration`, Q32.32 seconds.
    pub(crate) duration_q32: i64,
    /// `IsDebuff`.
    pub(crate) debuff: bool,
    /// `IsInvincible`: while it runs, no debuff reaches the unit.
    pub(crate) invincible: bool,
    /// `disableTechnology`: while it runs, the unit's technologies are off.
    pub(crate) disables_technology: bool,
    /// `amplifyDamageRate`, Q32.32: the rate on the damage the unit takes.
    pub(crate) amplify_damage_rate: i64,
    /// `damageChangeRate`, Q32.32: the rate on the damage the unit deals.
    pub(crate) damage_rate: i64,
    /// `speedChangeRate`, Q32.32: the rate on the unit's speed.
    pub(crate) speed_rate: i64,
    /// `attackRangeChangeValue`: whole metres on the main skill's range.
    pub(crate) attack_range_value: i64,
    /// `attackRangeChangeRate`, Q32.32: the rate on the main skill's range.
    pub(crate) attack_range_rate: i64,
    /// `maxLifeChangeRate`, Q32.32: what `IBEC_ChangeMaxLife` adds to the
    /// unit's own life rate.
    pub(crate) max_life_rate: i64,
    /// `stepTime`, Q32.32 seconds: how often its controllers update.
    pub(crate) step_q32: i64,
    /// `lifeChangeRate`, Q32.32: the rate of the unit's maximum life
    /// `IBEC_ChangeLIfe` changes it by every step.
    pub(crate) life_change_rate: i64,
    /// `disableRecover`: while it runs, the unit's repair and lifesteal add
    /// nothing.
    pub(crate) disables_recover: bool,
    /// How it stacks, if it does (`IsAdditiveEffect`).
    pub(crate) stacking: Option<Stacking>,
    /// `isClearSelfBuffWhenDisableTech`: as its unit's technologies are
    /// disabled, the buff its unit added itself is cleared.
    pub(crate) clears_when_technologies_disabled: bool,
    /// `summonUnitID`, through `Buff.GetSummonMechID`: what the unit the
    /// buff is on summons as it dies, if anything (`IBEC_DeadSummon`).
    pub(crate) summons: Option<DeadSummon>,
    /// `GetProbablity`: the chance, in thousandths, that `BuffSystem.
    /// DoAddBuff` adds it to a unit it reaches.
    pub(crate) probability: i32,
    /// What a hit leaves where it lands (`GetBuffRangeItem`), if anything.
    pub(crate) range_item: Option<crate::layout::TerrainSpec>,
    /// When and to whom the controller gives it.
    pub(crate) trigger: BuffTrigger,
    /// `CanDisable`, as [`LifeSteal::can_disable`]: whether a hit of a unit
    /// whose technologies are disabled adds no buff.
    pub(crate) can_disable: bool,
}

/// The unit a buff makes its unit summon as it dies: `Buff.GetSummonMechID`
/// answers the row's `summonUnitID`, or below 1 the type of the unit that
/// added the buff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeadSummon {
    /// The type of the unit that added the buff.
    SourceType,
    /// A unit type, by its id.
    Unit(i32),
}

/// When a buff source's `BuffCycleController` adds its buff, and to whom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuffTrigger {
    /// `FightStart` under `BuffTargetUpdateModel.All`: after its delay, and
    /// then every interval if it has one, the unit itself or the units in
    /// reach.
    All(AllCycle),
    /// `FightStart` under `Each`: every unit in reach, on every update.
    Around(BuffReach),
    /// `Hit`: what each hit of its unit's skills strikes, the controller
    /// being one of the skills' hit effects.
    Hit,
    /// `BeHit` onto `OpponentUnits`: the unit that hit it, each time a hit
    /// of a live unit's reaches it, the controller listening to its
    /// `OnMechBeHit`.
    BeHit,
    /// `GetDamage` onto `MechUnit`: the unit itself, each time it loses life,
    /// the controller listening to its `OnLifeChange`.
    Damaged,
}

/// A `BuffCycleController` under `BuffTargetUpdateModel.All`, which
/// `UpdateModel1` counts through itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AllCycle {
    /// `GetDelayTime` and `GetIntervalTime`, Q32.32 seconds.
    pub(crate) delay_q32: i64,
    pub(crate) interval_q32: i64,
    /// Whom it gives the buff: the unit itself when its targets are
    /// `MechUnit` alone, and otherwise the units in reach.
    pub(crate) reach: Option<BuffReach>,
}

/// The units a `RangeUnitCycle` keeps a buff on: those
/// `RangeTargetCalculator.CalculateRangeActors` finds around the source's
/// unit that `BuffCycleController.AvailableCheck` passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BuffReach {
    /// `GetMax`, Q32.32 metres.
    pub(crate) range_q32: i64,
    /// `GetAttackTargetFlyType`: the domains it reaches.
    pub(crate) domains: crate::rules::AttackTargets,
    /// `IsDistanceCalculateTargetRadius`: the distance is to a unit's edge.
    pub(crate) target_radius: bool,
    /// `GetEffectTargetTypes`: which units of those it passes.
    pub(crate) targets: BuffTargets,
    /// `GetTargetDamageDistanceType`: whether it passes only the units whose
    /// main skill is melee (`Some(true)`), only those whose is not
    /// (`Some(false)`), or either.
    pub(crate) melee: Option<bool>,
}

/// The `TargetType`s a source names among the units that reach any:
/// `MechUnit` (its own unit), `OtherSelfUnits` (its team's others) and
/// `OpponentUnits`. `FriendUnits` takes a unit of its group but not of its
/// team, and in a fight of one team a side reaches none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each target type is named or not on its own"
)]
pub(crate) struct BuffTargets {
    pub(crate) itself: bool,
    pub(crate) own_others: bool,
    pub(crate) opponents: bool,
}

/// A buff that stacks, `IBEC_AdditiveEffectBuff`, which asks its condition
/// for the stack at each step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Stacking {
    /// `maxAdditiveStack`: the most it stacks, none for no bound.
    pub(crate) max: u32,
    pub(crate) condition: StackCondition,
    /// `buffEffectAdditiveResetCondition` `Hitted`: its stack goes back to
    /// none each time its unit's main skill hits.
    pub(crate) resets_on_main_hit: bool,
}

/// `BuffEffectAdditiveCondition`: what a step's stack counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StackCondition {
    /// `BuffAdditiveStackConditionTimeController`: a stack more each step.
    Time,
    /// `BuffAdditiveStackConditionDistanceController`: a stack for every
    /// `buffEffectAdditiveConditionParam` (Q32.32 metres) the unit has moved
    /// while its technologies were not disabled.
    Distance { metres_q32: i64 },
}

/// What a `SweepSkillIntensifyTech` hands its unit's sweep
/// (`SweepSkillIntensifyEffectProvider`): metres onto the strip's width and
/// length, and how it lies and runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SweepIntensify {
    pub(crate) width_value: i32,
    pub(crate) length_value: i32,
    /// `isAttackDirectionPerpendicular`: whether the strip lies across the
    /// line to the target.
    pub(crate) perpendicular: bool,
    /// `isReverse`: whether it runs the other way.
    pub(crate) reverse: bool,
    /// `isDiableDirectionChange`: whether it keeps one direction attack after
    /// attack.
    pub(crate) fixed_direction: bool,
}

/// What an `IEnergyShieldSource` answers: the share of its unit's maximum
/// life its shield holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EnergyShield {
    /// `GetLifeRate`, Q32.32.
    pub(crate) life_rate_q32: i64,
    /// `GetPriority`: 1 for an equipment, 0 for a technology.
    pub(crate) priority: i32,
    /// `CanDisable`, as [`LifeSteal::can_disable`].
    pub(crate) can_disable: bool,
}

/// What an `IAdvancedEnergyShieldSource` answers: the battlefield shield its
/// unit carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CarriedShield {
    /// `GetRadius`, whole metres.
    pub(crate) radius: i64,
    /// `GetShieldValue`: its energy, full.
    pub(crate) energy: i64,
}

/// What an `IReactiveArmorTechDataSource` answers: the rate on the damage
/// its unit takes (`GetDamageReduceRate`) and how many hits dealing it damage
/// the rate lasts (`GetDamageReduceCount`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReactiveArmor {
    /// An `FPoint` raw rate, -0.8 on every row.
    pub(crate) rate_q32: i64,
    pub(crate) count: i32,
}

/// What a `SupportUnitEquipment` or a `SupportUnitTech` answers
/// `ISupportDataSource` with: a production line its wearer runs, whose makes
/// stand at set offsets from it or about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProductionLine {
    /// `GetUnitID`: the unit type it makes.
    pub(crate) unit_type_id: u32,
    /// `GetBatchMaxCount`: how many batches it makes in all.
    pub(crate) max_batch: u32,
    /// `GetMaxCount`: how many of its makes may live at once.
    pub(crate) max_alive: u32,
    /// `GetCreateCountPerTime`.
    pub(crate) per_time: u32,
    /// `GetCreateInterval`, Q32.32 seconds.
    pub(crate) interval_q32: i64,
    /// `GetPositionDatas`: each make's offset from its wearer, Q32.32
    /// metres, right and forward of its facing.
    pub(crate) offsets: Vec<(i64, i64)>,
    /// How long each make takes to appear (`SupportUnitCreator.CreateMech`),
    /// Q32.32 seconds: `APPEAR_DURATION`, a second, or the row's
    /// `productTime`.
    pub(crate) appear_q32: i64,
    /// Whether each make takes its wearer's level (`DynamicMechLevel.Parent`)
    /// rather than the first.
    pub(crate) parent_level: bool,
    /// Whether an offset turns with the wearer's body rather than its root
    /// (`SupportUnitPositionSpace.ParentBody`).
    pub(crate) body_frame: bool,
    /// Where each make appears.
    pub(crate) arrival: Arrival,
    /// What the row writes onto each make, `SupportUnitData.modifyData`'s:
    /// `SupportUnitCreator.CreateMech` adds its `GetUnitLifeChangeRate` to
    /// the make's `MechDataChangeFloatRate.LifeRate` and its
    /// `GetUnitDamageChangeRate` to its main skill's
    /// `SkillDataChangeFloatRate.DamageRate`.
    pub(crate) make_corrections: Vec<(crate::data::Channel, crate::data::Entry)>,
    /// Whether a support skill of its wearer's lets each batch out
    /// (`SupportSkillStartAttackChecker`), locking the line while it may not
    /// start.
    pub(crate) gated: bool,
}

/// Where a production line's make appears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Arrival {
    /// At its offset from its wearer as it is made.
    InPlace,
    /// Out of its wearer, `appearType` 7 (`SummonSystem.CreateMechDelaySetPos`):
    /// made where the wearer stands, it takes its place at its offset from
    /// where the wearer stands as it joins.
    ComesOut,
}

/// An `IEffectProviderDataSource` a `SingleEffectProvider` sorts.
pub(crate) trait Source: Copy + PartialEq {
    /// The interface's name, which a refusal says.
    const INTERFACE: &'static str;

    fn priority(&self) -> i32;
}

impl Source for LifeSteal {
    const INTERFACE: &'static str = "ILifeSteal";

    fn priority(&self) -> i32 {
        self.priority
    }
}

impl Source for EnergyShield {
    const INTERFACE: &'static str = "IEnergyShieldSource";

    fn priority(&self) -> i32 {
        self.priority
    }
}

impl Source for AutoRecovery {
    const INTERFACE: &'static str = "IAutoRecovery";

    fn priority(&self) -> i32 {
        self.priority
    }
}

/// `SingleEffectProvider<T>.Current`: of the sources of one interface a
/// unit carries, the one of the highest priority.
///
/// # Errors
///
/// Refuses two sources of the highest priority that answer differently: the
/// comparison `AddDataSource` sorts by orders neither before the other, and
/// which one the sort leaves first is not established.
pub(crate) fn current<T: Source>(sources: &[T]) -> Result<Option<T>, String> {
    let Some(highest) = sources.iter().map(Source::priority).max() else {
        return Ok(None);
    };
    let mut first = sources.iter().filter(|source| source.priority() == highest);
    let current = *first.next().expect("the highest priority is some source's");
    if first.any(|other| *other != current) {
        return Err(format!(
            "two {} sources of priority {highest} answer differently, and which one \
             SingleEffectProvider enables is not established",
            T::INTERFACE
        ));
    }
    Ok(Some(current))
}

#[cfg(test)]
mod tests {
    use super::{LifeSteal, current};

    const ABSORPTION_MODULE: LifeSteal = LifeSteal {
        multiplier_q32: 3_865_470_566,
        priority: 1,
        can_disable: false,
    };
    const ENERGY_DRAIN: LifeSteal = LifeSteal {
        multiplier_q32: 3_435_973_836,
        priority: 0,
        can_disable: true,
    };

    /// An equipment's priority is the higher, so it is the one enabled.
    #[test]
    fn the_equipment_overrides_the_technology() {
        assert_eq!(
            current(&[ENERGY_DRAIN, ABSORPTION_MODULE]),
            Ok(Some(ABSORPTION_MODULE))
        );
        assert_eq!(current::<LifeSteal>(&[]), Ok(None));
        assert_eq!(
            current(&[ABSORPTION_MODULE, ABSORPTION_MODULE]),
            Ok(Some(ABSORPTION_MODULE))
        );
        let other = LifeSteal {
            multiplier_q32: 1,
            ..ABSORPTION_MODULE
        };
        assert!(current(&[ABSORPTION_MODULE, other]).is_err());
    }
}
