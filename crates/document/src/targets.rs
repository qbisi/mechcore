//! Which units a row reaches: `UnitUtility.IsEffectTarget`.
//!
//! Officers, equipment, energy-tower skills and unit reinforcements all ask
//! that one method, which switches on the row's `UnitEffectTargetType`. So a
//! category means the same thing whichever table names it, and it is resolved
//! here rather than by each table, for the simulator and the layout alike.

use serde::{Deserialize, Serialize};

/// `UnitType`: what a row targeting small, medium or huge units reads.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UnitSize {
    Small,
    Medium,
    Huge,
}

/// What `IsEffectTarget` reads of a unit.
#[derive(Debug, Clone, Copy)]
pub struct Category<'a> {
    pub type_name: &'a str,
    /// The main skill's `SkillData.isMeleeAttack`.
    pub melee: bool,
    /// `IUnitStateEffect.GetUnitStateType`: whether it is a ground unit.
    pub ground: bool,
    pub size: UnitSize,
}

/// A row's `UnitEffectTargetType`, for the categories this build resolves.
#[derive(Debug, Clone)]
pub enum Targets {
    /// `All` (0): every unit.
    Every,
    /// `mech_type` 1 and 10: the units the row lists.
    Listed(Vec<String>),
    /// `Melee` (3): a unit whose main skill's `isMeleeAttack` is set.
    Melee,
    /// `Ranged` (4): a unit whose main skill's `isMeleeAttack` is clear.
    Ranged,
    /// `Ground` (2): a unit that does not fly.
    Ground,
    /// `Small`, `Medium` and `Huge` (5 to 7): a unit of that `UnitType`.
    Size(UnitSize),
    /// A row naming several categories: `IsEffectTarget` walks them and
    /// reaches the unit only if every one does.
    AllOf(Vec<Targets>),
    /// A category this build does not resolve, carrying what to say about it.
    Refused(String),
}

impl Targets {
    /// The category `mech_type` names, for a row `who` describes.
    #[must_use]
    pub fn of(mech_type: i32, units: &[String], who: &str) -> Self {
        match mech_type {
            0 => Self::Every,
            1 | 10 => Self::Listed(units.to_vec()),
            2 => Self::Ground,
            3 => Self::Melee,
            4 => Self::Ranged,
            5 => Self::Size(UnitSize::Small),
            6 => Self::Size(UnitSize::Medium),
            7 => Self::Size(UnitSize::Huge),
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

    /// The categories a row's `mech_type` list names, every one of which a
    /// unit has to be in.
    #[must_use]
    pub fn of_list(mech_types: &[i32], units: &[String], who: &str) -> Self {
        match mech_types {
            [mech_type] => Self::of(*mech_type, units, who),
            _ => Self::AllOf(
                mech_types
                    .iter()
                    .map(|&mech_type| Self::of(mech_type, units, who))
                    .collect(),
            ),
        }
    }

    /// Whether the category names a unit by whether it flies
    /// (`IUnitStateEffect.GetUnitStateType`), which a technology that turns
    /// its unit's domain changes in the fight.
    #[must_use]
    pub fn by_domain(&self) -> bool {
        match self {
            Self::Ground => true,
            Self::AllOf(every) => every.iter().any(Self::by_domain),
            Self::Every
            | Self::Listed(_)
            | Self::Melee
            | Self::Ranged
            | Self::Size(_)
            | Self::Refused(_) => false,
        }
    }

    /// Whether the row writes onto this unit.
    ///
    /// `IsEffectTarget` reads Melee and Ranged from the unit's main
    /// `SkillData.isMeleeAttack`.
    ///
    /// # Errors
    ///
    /// Returns the refusal of a category this build does not resolve.
    pub fn reaches(&self, unit: &Category<'_>) -> Result<bool, String> {
        match self {
            Self::Every => Ok(true),
            Self::Listed(units) => Ok(units.iter().any(|name| name == unit.type_name)),
            Self::Melee => Ok(unit.melee),
            Self::Ranged => Ok(!unit.melee),
            Self::Ground => Ok(unit.ground),
            Self::Size(size) => Ok(unit.size == *size),
            Self::AllOf(every) => {
                for targets in every {
                    if !targets.reaches(unit)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            Self::Refused(reason) => Err(reason.clone()),
        }
    }
}

#[derive(Deserialize)]
struct UnitRow {
    type_name: String,
    domain: Domain,
    size: UnitSize,
    attack: AttackRow,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Domain {
    Ground,
    Air,
}

#[derive(Deserialize)]
struct AttackRow {
    #[serde(default)]
    melee: bool,
}

/// What `IsEffectTarget` reads of a unit type as its card deploys it, from
/// its `config/units` file, or `None` for a type no file names.
///
/// # Panics
///
/// Panics if an embedded unit file does not parse, which the simulator's
/// own load refuses first.
#[must_use]
pub fn category(type_name: &str) -> Option<Category<'static>> {
    static UNITS: std::sync::OnceLock<Vec<UnitRow>> = std::sync::OnceLock::new();
    UNITS
        .get_or_init(|| {
            crate::catalog::UNIT_CONFIGS
                .iter()
                .map(|text| serde_yaml::from_str(text).expect("an embedded unit file parses"))
                .collect()
        })
        .iter()
        .find(|row| row.type_name == type_name)
        .map(|row| Category {
            type_name: &row.type_name,
            melee: row.attack.melee,
            ground: row.domain == Domain::Ground,
            size: row.size,
        })
}
