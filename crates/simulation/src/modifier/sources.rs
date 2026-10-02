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

/// The `buffDatas` row a `BuffEquipment` adds to its unit as the fight
/// starts: one whose `BuffTechListener` is `FightStart`, whose target is the
/// unit itself, and which always triggers. `BuffEffectProvider` holds every
/// such source, not one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StartBuff {
    pub(crate) buff_id: u32,
    pub(crate) divide: i32,
    pub(crate) additive: bool,
    /// `duration`, Q32.32 seconds.
    pub(crate) duration_q32: i64,
    /// `IsDebuff`.
    pub(crate) debuff: bool,
    /// `IsInvincible`: while it runs, no debuff reaches the unit.
    pub(crate) invincible: bool,
    /// `amplifyDamageRate`, Q32.32: the rate on the damage the unit takes.
    pub(crate) amplify_damage_rate: i64,
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
