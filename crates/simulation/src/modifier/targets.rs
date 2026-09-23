//! Which units a row reaches: `UnitUtility.IsEffectTarget`.
//!
//! Officers, equipment, energy-tower skills and unit reinforcements all ask
//! that one method, which switches on the row's `UnitEffectTargetType`. So a
//! category means the same thing whichever table names it, and it is resolved
//! here rather than by each table.

use crate::{Error, Result, rules::UnitConfig};

/// A row's `UnitEffectTargetType`, for the categories this build resolves.
#[derive(Debug, Clone)]
pub(crate) enum Targets {
    /// `All` (0): every unit.
    Every,
    /// `mech_type` 1 and 10: the units the row lists.
    Listed(Vec<String>),
    /// `Melee` (3): a unit whose main skill's `isMeleeAttack` is set.
    Melee,
    /// `Ranged` (4): a unit whose main skill's `isMeleeAttack` is clear.
    Ranged,
    /// A category this build does not resolve, carrying what to say about it.
    Refused(String),
}

impl Targets {
    /// The category `mech_type` names, for a row `who` describes.
    pub(crate) fn of(mech_type: i32, units: &[String], who: &str) -> Self {
        match mech_type {
            0 => Self::Every,
            1 | 10 => Self::Listed(units.to_vec()),
            3 => Self::Melee,
            4 => Self::Ranged,
            // Type 11 corrects a tower, a shield or a mine. Its rows carry no
            // unit number at all, so a refusal names the field first; this is
            // here so a future type-11 row with one is not silently applied
            // to every unit.
            11 => Self::Refused(format!(
                "{who} corrects a tower, a shield or a mine rather than a unit"
            )),
            other => Self::Refused(format!(
                "{who} targets mech_type {other}, which this build does not read"
            )),
        }
    }

    /// Whether the row writes onto this unit.
    ///
    /// `IsEffectTarget` reads Melee and Ranged from the unit's main
    /// `SkillData.isMeleeAttack`, which is [`crate::rules::AttackConfig`]'s
    /// `melee`.
    ///
    /// # Errors
    ///
    /// Returns the refusal of a category this build does not resolve.
    pub(crate) fn reaches(&self, unit: &UnitConfig) -> Result<bool> {
        match self {
            Self::Every => Ok(true),
            Self::Listed(units) => Ok(units.contains(&unit.type_name)),
            Self::Melee => Ok(unit.attack.melee),
            Self::Ranged => Ok(!unit.attack.melee),
            Self::Refused(reason) => Err(Error::new(reason.clone())),
        }
    }
}
