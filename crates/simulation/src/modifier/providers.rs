//! The effect providers a technology hands its sources to, and which of them
//! the fight switches off.
//!
//! `FightMech.DisableTechnology` reaches `FightEffectMananger.DisableEffect`,
//! which hands every provider of the unit `DisableEffect`; `EnableEffect`
//! gives back. Each provider takes away what its sources that `CanDisable`
//! gave, as its own class does. Every technology is an `IDataModifier`, whose
//! numbers its provider removes and writes again
//! (`IEffectProviderDataSource.RemoveData`, `AddData`); a class that answers
//! an interface beside it, `ILifeSteal` or `IStealthTechDataSource`, is that
//! interface's provider's source too. One path switches them all, in the
//! fight (`Simulation::switch_technologies`); what differs is each
//! provider's own `DisableEffect`, which `Simulation::switch_provider`
//! mirrors.

/// A provider beside the numbers' that a technology's class reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum EffectProvider {
    /// `LifeStealEffectProvider`, for an `ILifeSteal`.
    LifeSteal,
    /// `AutoRecoveryEffectProvider`, for an `IAutoRecovery`.
    AutoRecovery,
    /// `EnergyShieldProvider`, for an `IEnergyShieldSource`.
    EnergyShield,
    /// `SweepSkillIntensifyEffectProvider`.
    SweepSkillIntensify,
    /// `ArmorStrengthenEffectProvider`, for an `IArmorStrengthen`.
    ArmorStrengthen,
    /// `SearchTargetSpecificProvider`, for an `ISearchTargetSpecific`.
    SearchTargetSpecific,
    /// `AirAttackEffectProvider`, for an `IAirAttackDataSource`.
    AirAttack,
    /// `SecondaryDamageIntensifyEffectProvider`.
    SecondaryDamageIntensify,
    /// `BuffEffectProvider`, for an `IEffectBuffDataSource`.
    Buff,
    /// `InterceptMissileEffectProvider`, for an
    /// `IInterceptEffectProviderDataSource`.
    InterceptMissile,
    /// `SupportUnitProvider`, for an `ISupportEffectDataSource`.
    SupportUnit,
    /// `DeadEffectProvider`, for an `IDeadEffect`.
    DeadEffect,
    /// `MoveAbilitySummonProvider`, for an `IMoveAbilitySummon`.
    MoveAbilitySummon,
    /// `ExtraSkillProvider`, for an `IExtraSkill`.
    ExtraSkill,
    /// `StealthTechEffectProvider`, for an `IStealthTechDataSource`.
    StealthTech,
    /// `DeadLineEffectProvider`, for an `IDeadLineDataSource`.
    DeadLine,
    /// `MoveAbilityAttackIntensifyProvider`, for an
    /// `IMoveAbilityAttackIntensify`.
    MoveAbilityAttackIntensify,
    /// `MoveAbilityRangeItemProvider`, for an `IMoveAbilityRangeItem`.
    MoveAbilityRangeItem,
    /// `MechGrounpEffectProvider`, for an `IMechGroupSource`.
    MechGroup,
    /// `AdvancedEnergyShieldProvider`, for an `IAdvancedEnergyShieldSource`.
    AdvancedEnergyShield,
    /// `ReactiveArmorTechEffectProvider`, for an
    /// `IReactiveArmorTechDataSource`.
    ReactiveArmor,
    /// `SiegeModeEffectProvider`, for an `ISiegeModeEffectDataSource`.
    SiegeMode,
    /// `FireIntensifyEffectProvider`, for an `IFireIntensify`.
    FireIntensify,
    /// `WreckageRecoveryEffectProvider`, for an `IWreckageRecovery`.
    WreckageRecovery,
    /// `RVORadiusChangeProvider`, for an `IRVORadiusChangeSource`.
    RvoRadiusChange,
    /// `ClearRangeItemEffectProvider`, for an `IClearRangeItem`.
    ClearRangeItem,
    /// `KillExplosionEffectProvider`, for an `IKillExplosionDataSource`.
    KillExplosion,
    /// `RecoveryEffectProvider`, for an `IRecoveryTechEffectDataSource`.
    Repair,
    /// `BurrowEffectProvider`, for an `IBurrow`.
    Burrow,
    /// `FlyTechEffectProvider`, for an `IFlyTechDataSource`.
    FlyTech,
    /// `IgnoreBuffEffectProvider`, for an `IIgnoreBuffDataSouce`.
    IgnoreBuff,
}

impl EffectProvider {
    /// Every provider, in the order the fight hands a unit to each as
    /// `FightEffectSystem` activates, deactivates or switches its effects.
    pub(crate) const ALL: [Self; 31] = [
        Self::InterceptMissile,
        Self::StealthTech,
        Self::MechGroup,
        Self::SiegeMode,
        Self::ReactiveArmor,
        Self::WreckageRecovery,
        Self::Burrow,
        Self::Repair,
        Self::LifeSteal,
        Self::AutoRecovery,
        Self::EnergyShield,
        Self::SweepSkillIntensify,
        Self::ArmorStrengthen,
        Self::SearchTargetSpecific,
        Self::AirAttack,
        Self::SecondaryDamageIntensify,
        Self::Buff,
        Self::SupportUnit,
        Self::DeadEffect,
        Self::MoveAbilitySummon,
        Self::ExtraSkill,
        Self::DeadLine,
        Self::MoveAbilityAttackIntensify,
        Self::MoveAbilityRangeItem,
        Self::AdvancedEnergyShield,
        Self::FireIntensify,
        Self::RvoRadiusChange,
        Self::ClearRangeItem,
        Self::KillExplosion,
        Self::FlyTech,
        Self::IgnoreBuff,
    ];
}
