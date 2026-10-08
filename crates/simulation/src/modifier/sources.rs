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

/// What an `IAutoRecovery` whose `GetAutoRecoveryStateType` is `Normal`
/// answers: it repairs whenever its unit is hurt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AutoRecovery {
    /// `GetStartTime`, Q32.32 seconds.
    pub(crate) start_time_q32: i64,
    /// `GetRecoveryDuration`, Q32.32 seconds between two repairs.
    pub(crate) duration_q32: i64,
    /// `GetRecoveryLIfeRate`, Q32.32: the share of the unit's maximum life
    /// one repair restores.
    pub(crate) life_rate_q32: i64,
    /// `GetPriority`: 1 for an equipment, 0 for a technology.
    pub(crate) priority: i32,
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

/// What a `SupportUnitEquipment` answers `ISupportDataSource` with: a
/// production line its wearer runs, `appearType` 5, whose makes stand at
/// set offsets from it.
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
    /// Whether a support skill of its wearer's lets each batch out
    /// (`SupportSkillStartAttackChecker`), locking the line while it may not
    /// start.
    pub(crate) gated: bool,
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
