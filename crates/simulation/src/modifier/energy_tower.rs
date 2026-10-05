//! What an Energy Tower skill writes onto the units of the side that activated
//! it.
//!
//! `config/energy_tower_skill_effects.yaml` is the build's table, extracted by
//! `scripts/extract/extract-energy-tower-skill-effects.py`, and
//! `docs/rules/energy_tower_skills.md` states what each field means.
//! `EnergyTowerSkill` answers the interfaces a technology answers and writes
//! through the same writers, `MechDataModifer.TryAddCommonData` and
//! `SkillDataModifier.AddData`, so a row becomes the corrections
//! [`super::effects`] makes of an officer's, in the same channels.
//!
//! A skill reaches a fight only when `EnergyTowerSkill.IsCommonEffect` holds,
//! which reads its speed value and its attack range value and nothing else, so
//! the table holds those skills and those two fields alone. A skill with no row
//! writes nothing onto a unit.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::{
    Error, Result,
    data::{Channel, Correction, Entry, Index},
    rules::UnitConfig,
};

use super::{
    effects::{self, Fields},
    targets::Targets,
};

const DEFAULT_ENERGY_TOWER_SKILL_EFFECTS: &str =
    include_str!("../../../../config/energy_tower_skill_effects.yaml");

/// The module that tags every entry a skill writes.
pub(crate) const SOURCE: &str = "Modifier";

/// Every Energy Tower skill's combat effect, by the id a layout compiles to.
#[derive(Debug, Clone)]
pub(crate) struct EnergyTowerSkillEffects {
    skills: BTreeMap<i32, Skill>,
}

#[derive(Debug, Clone)]
struct Skill {
    /// Which units the row reaches.
    targets: Targets,
    /// What it writes.
    effect: Vec<(Channel, Index, Correction)>,
}

/// One row of the table, with every field the extraction writes.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    id: i32,
    name: String,
    mech_type: Vec<i32>,
    #[serde(default)]
    units: Vec<i32>,
    #[serde(default)]
    attack_range_value: Option<i64>,
    #[serde(default)]
    speed_value: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    schema: String,
    skills: Vec<Row>,
}

impl EnergyTowerSkillEffects {
    /// Reads the tracked table.
    ///
    /// # Errors
    ///
    /// Returns an error when the table is not the one this build reads.
    pub(crate) fn load() -> Result<Self> {
        let table: Table =
            serde_yaml::from_str(DEFAULT_ENERGY_TOWER_SKILL_EFFECTS).map_err(|error| {
                Error::new(format!(
                    "cannot read the energy tower skill effect table: {error}"
                ))
            })?;
        if table.schema != "mechcore.energy_tower_skill_effects" {
            return Err(Error::new(format!(
                "energy tower skill effect table declares schema {:?}",
                table.schema
            )));
        }
        let mut skills = BTreeMap::new();
        for row in table.skills {
            let id = row.id;
            if skills.insert(id, Skill::of(&row)).is_some() {
                return Err(Error::new(format!(
                    "energy tower skill effect table holds skill {id} twice"
                )));
            }
        }
        Ok(Self { skills })
    }

    /// What the skills a side activated write onto one of its units.
    ///
    /// # Errors
    ///
    /// Returns an error naming the skill when its targeting is one this build
    /// does not resolve.
    pub(crate) fn corrections(
        &self,
        activated: &[i32],
        unit: &UnitConfig,
    ) -> Result<Vec<(Channel, Entry)>> {
        let mut written = Vec::new();
        for id in activated {
            let Some(skill) = self.skills.get(id) else {
                continue;
            };
            if !skill.targets.reaches(unit)? {
                continue;
            }
            written.extend(skill.effect.iter().map(|(channel, index, correction)| {
                (
                    *channel,
                    Entry {
                        index: *index,
                        source: SOURCE,
                        correction: *correction,
                    },
                )
            }));
        }
        Ok(written)
    }
}

impl Skill {
    fn of(row: &Row) -> Self {
        let who = format!("energy tower skill {} ({})", row.id, row.name);
        let targets = match (row.mech_type.as_slice(), row.units.is_empty()) {
            (&[mech_type], true) => Targets::of(mech_type, &[], &who),
            _ => Targets::Refused(format!(
                "{who} targets mech_type {:?} and units {:?}, which this build \
                 does not read",
                row.mech_type, row.units
            )),
        };
        Self {
            targets,
            effect: effects::corrections(Fields {
                attack_range_value: row.attack_range_value,
                speed_value: row.speed_value,
                ..Fields::default()
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::EnergyTowerSkillEffects;
    use crate::{
        data::Stats,
        rules::{UnitConfig, UnitConfigs, UnitDomain},
    };

    const ENHANCED_RANGE: i32 = 5;
    const HIGH_MOBILITY: i32 = 6;

    fn unit(name: &str) -> UnitConfig {
        UnitConfigs::load().unwrap().get(name).unwrap().clone()
    }

    /// `tests/modifier/fights/` recorded a Marksman at 155 of range and 11 of
    /// speed under both skills, and a Rhino at its own range and 11 of speed.
    #[test]
    fn range_reaches_a_ranged_unit_and_speed_every_unit() {
        let skills = EnergyTowerSkillEffects::load().unwrap();
        let both = [ENHANCED_RANGE, HIGH_MOBILITY];
        let marksman = unit("marksman");
        let corrected =
            Stats::corrected(&marksman, 1, &skills.corrections(&both, &marksman).unwrap()).unwrap();
        assert_eq!(corrected.attack_range_against(UnitDomain::Ground), 155_000);
        assert_eq!(corrected.move_speed_q32(), 11 << 32);
        let rhino = unit("rhino");
        let corrected =
            Stats::corrected(&rhino, 1, &skills.corrections(&both, &rhino).unwrap()).unwrap();
        let plain = Stats::corrected(&rhino, 1, &[]).unwrap();
        assert_eq!(
            corrected.attack_range_against(UnitDomain::Ground),
            plain.attack_range_against(UnitDomain::Ground)
        );
        assert_eq!(
            corrected.move_speed_q32(),
            plain.move_speed_q32() + (3 << 32)
        );
    }
}
