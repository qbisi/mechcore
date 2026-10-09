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
//! provider's own `DisableEffect`, and a provider whose `DisableEffect` the
//! fight does not mirror is refused by name as a disabling buff reaches its
//! unit.

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
    /// `BuffEffectProvider`, for an `IEffectBuffDataSource`; `cycles` when
    /// its buff runs on the units around its unit rather than once on the
    /// unit itself.
    Buff { cycles: bool },
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
}

impl EffectProvider {
    /// The build's name for it, which a refusal gives.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::LifeSteal => "LifeStealEffectProvider",
            Self::AutoRecovery => "AutoRecoveryEffectProvider",
            Self::EnergyShield => "EnergyShieldProvider",
            Self::SweepSkillIntensify => "SweepSkillIntensifyEffectProvider",
            Self::ArmorStrengthen => "ArmorStrengthenEffectProvider",
            Self::SearchTargetSpecific => "SearchTargetSpecificProvider",
            Self::AirAttack => "AirAttackEffectProvider",
            Self::SecondaryDamageIntensify => "SecondaryDamageIntensifyEffectProvider",
            Self::Buff { .. } => "BuffEffectProvider",
            Self::InterceptMissile => "InterceptMissileEffectProvider",
            Self::SupportUnit => "SupportUnitProvider",
            Self::DeadEffect => "DeadEffectProvider",
            Self::MoveAbilitySummon => "MoveAbilitySummonProvider",
            Self::ExtraSkill => "ExtraSkillProvider",
            Self::StealthTech => "StealthTechEffectProvider",
            Self::DeadLine => "DeadLineEffectProvider",
            Self::MoveAbilityAttackIntensify => "MoveAbilityAttackIntensifyProvider",
            Self::MoveAbilityRangeItem => "MoveAbilityRangeItemProvider",
            Self::MechGroup => "MechGrounpEffectProvider",
            Self::AdvancedEnergyShield => "AdvancedEnergyShieldProvider",
            Self::ReactiveArmor => "ReactiveArmorTechEffectProvider",
            Self::SiegeMode => "SiegeModeEffectProvider",
            Self::FireIntensify => "FireIntensifyEffectProvider",
        }
    }

    /// Whether the fight mirrors its `DisableEffect` and `EnableEffect`.
    ///
    /// - An armour's provider takes its reduction away, which is the unit's
    ///   numbers.
    /// - A lifesteal's and a second damage's providers take their hit effect
    ///   away, which the fight asks of the unit at each hit.
    /// - A search technology's `SearchTargetSpecificProvider.DoDisable` takes
    ///   its ranges and score offsets away and turns its unit's selector back
    ///   to `Normal`, which is the selector it turned to with no offsets, and
    ///   `DoEnable` writes them and turns it to `DistanceIntensify` again,
    ///   which reads them as they now stand.
    /// - An extra weapon's provider disables its skills
    ///   (`ExtraSkillProvider.DisableSkill`), which the layout refuses for the
    ///   shapes it does not fight switched off.
    /// - A buff added once onto its own unit is cleared
    ///   (`BuffManager.ClearSelfResourceBuffByDisableTech`); one that cycles
    ///   over the units around stops (`BuffEffectProvider.DoDisableCycle`),
    ///   and how it starts again is not measured.
    /// - A stealth technology's unit is shown and counts as triggered
    ///   (`StealthTechSystem.DisableStealthTech`).
    /// - A dead-line technology's pre-hit effect is taken off its unit's
    ///   skills (`DeadLineEffectProvider.DisableEffect`), and does nothing
    ///   while the technologies are off (`PerformPreHitEffect`).
    /// - A grouping technology's unit leaves its side's groups
    ///   (`MechGrounpEffectProvider.DisableEffect`,
    ///   `TeamMechGroupManager.RemoveMech`) and is handed back (`AddMech`).
    /// - A barrier technology's shield is disabled and deactivated, its
    ///   energy recorded, and given back that energy and activated again
    ///   (`AdvancedEnergyShieldProvider.DisableEffect`, `EnableEffect`).
    /// - A reactive armor's rate leaves its unit, its count kept, and comes
    ///   back while the count lasts (`ReactiveArmorSystem.DisableReactiveArmor`,
    ///   `EnableReactiveArmor`).
    /// - A siege-mode technology's unit leaves its trench on the system's
    ///   next update, as if no enemy had stood in range for its whole
    ///   duration (`SiegeModeEffectProvider.DisableEffect`,
    ///   `SiegeModeEffectSystem.EndSiegeMode`), and does not dig in again
    ///   (`EnableEffect` is `SingleEffectProvider`'s alone).
    /// - A fire technology's hit leaves no fire while the technologies are
    ///   off (`FireIntensifyEffectProvider.PerformHitEffect` returns on
    ///   `isTechnologyDisabled` for a source that `CanDisable`).
    ///
    /// Every other provider does more, which is not measured.
    pub(crate) const fn disable_read(self) -> bool {
        match self {
            Self::LifeSteal
            | Self::ArmorStrengthen
            | Self::SearchTargetSpecific
            | Self::SecondaryDamageIntensify
            | Self::Buff { cycles: false }
            | Self::ExtraSkill
            | Self::StealthTech
            | Self::DeadLine
            | Self::MechGroup
            | Self::AdvancedEnergyShield
            | Self::ReactiveArmor
            | Self::SiegeMode
            | Self::FireIntensify => true,
            Self::AutoRecovery
            | Self::EnergyShield
            | Self::SweepSkillIntensify
            | Self::AirAttack
            | Self::Buff { cycles: true }
            | Self::InterceptMissile
            | Self::SupportUnit
            | Self::DeadEffect
            | Self::MoveAbilitySummon
            | Self::MoveAbilityAttackIntensify
            | Self::MoveAbilityRangeItem => false,
        }
    }
}
