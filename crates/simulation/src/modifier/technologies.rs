//! What a technology writes onto the unit that researched it.
//!
//! `config/technology_effects.yaml` is the build's own table, extracted by
//! `scripts/extract-technology-effects.py`, and
//! `docs/rules/technology_effects.md` states what each field means. The fields
//! are [`super::effects`]'s, the same ones an officer writes, because
//! `TechnologyData` and `OfficerData` answer the same interface.
//!
//! A technology belongs to one unit type, which is how a side's flat list of
//! technologies reaches the units it corrects: a technology the side holds
//! writes onto the units its table row names and onto nothing else.
//!
//! **An effect is a list indexed by the unit's rank, and this reads rank one.**
//! A technology whose list holds more than one entry is refused rather than
//! read at index zero, because which index a fight reads for a given rank is
//! `docs/rules/technology_effects.md`'s unresolved question and a layout's
//! units above rank one are refused by the module registry anyway.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::{
    Error, Result,
    data::{Channel, Correction, Entry, Index},
};

use super::effects::{self, Fields, PROJECTILE, SPLASH, VALUE_ELSEWHERE};

const DEFAULT_TECHNOLOGY_EFFECTS: &str = include_str!("../../../../config/technology_effects.yaml");

/// The module that tags every entry a technology writes.
pub(crate) const SOURCE: &str = "Modifier";

/// Every technology's combat effect, by the id a layout compiles to.
#[derive(Debug, Clone)]
pub(crate) struct TechnologyEffects {
    technologies: BTreeMap<i32, Technology>,
}

#[derive(Debug, Clone)]
struct Technology {
    /// The unit type whose numbers it corrects.
    unit: String,
    /// What it writes, or why this build will not apply it.
    effect: std::result::Result<Vec<(Channel, Index, Correction)>, String>,
}

/// One row of the table. Every effect is a list because a technology's effect
/// can grow with the unit's rank.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    id: i32,
    name: String,
    unit: String,
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
            let technology = Technology {
                unit: row.unit.clone(),
                effect: corrections_of(&row),
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
    /// An id the table does not hold writes nothing onto a unit: the table
    /// carries the technologies that correct a unit's numbers, and the other
    /// 96 summon something, change a skill or debuff the enemy. The ids
    /// reaching here come from a compiled layout, whose technologies are
    /// validated against the catalogue.
    ///
    /// # Errors
    ///
    /// Returns an error naming the technology when the side holds one this
    /// build cannot apply, rather than applying the part it understands.
    pub(crate) fn corrections(
        &self,
        held: &[i32],
        unit_type: &str,
    ) -> Result<Vec<(Channel, Entry)>> {
        let mut written = Vec::new();
        for id in held {
            let Some(technology) = self.technologies.get(id) else {
                continue;
            };
            if technology.unit != unit_type {
                continue;
            }
            let corrections = technology
                .effect
                .as_ref()
                .map_err(|why| Error::new(why.clone()))?;
            for (channel, index, correction) in corrections {
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
}

/// What a row writes at rank one, or why this build will not apply it.
fn corrections_of(row: &Row) -> std::result::Result<Vec<(Channel, Index, Correction)>, String> {
    let every = [
        ("life_rate", &row.life_rate),
        ("damage_rate", &row.damage_rate),
        ("speed_value", &row.speed_value),
        ("min_attack_range_value", &row.min_attack_range_value),
        ("attack_range_value", &row.attack_range_value),
        ("attack_range_rate", &row.attack_range_rate),
        ("attack_interval_value", &row.attack_interval_value),
        ("attack_interval_rate", &row.attack_interval_rate),
        ("splash_range_value", &row.splash_range_value),
        ("projectile_speed_value", &row.projectile_speed_value),
        ("projectile_life_rate", &row.projectile_life_rate),
    ];
    for (field, values) in every {
        if values.len() > 1 {
            return Err(format!(
                "technology {} ({}) grows with the unit's rank, and which entry of \
                 {field} a fight reads for a given rank is not established: see the \
                 unresolved questions in docs/rules/technology_effects.md",
                row.id, row.name
            ));
        }
    }

    let unsupported = [
        (
            &row.min_attack_range_value,
            "min_attack_range_value",
            VALUE_ELSEWHERE,
        ),
        (&row.splash_range_value, "splash_range_value", SPLASH),
        (
            &row.projectile_speed_value,
            "projectile_speed_value",
            PROJECTILE,
        ),
        (
            &row.projectile_life_rate,
            "projectile_life_rate",
            PROJECTILE,
        ),
    ];
    for (values, field, why) in unsupported {
        if values.iter().any(|value| *value != 0) {
            return Err(format!(
                "technology {} ({}) writes {field}, and {why}",
                row.id, row.name
            ));
        }
    }

    let at_rank_one = |values: &Vec<i64>| values.first().copied().filter(|value| *value != 0);
    Ok(effects::corrections(Fields {
        life_rate: at_rank_one(&row.life_rate),
        damage_rate: at_rank_one(&row.damage_rate),
        attack_range_rate: at_rank_one(&row.attack_range_rate),
        attack_interval_rate: at_rank_one(&row.attack_interval_rate),
        attack_range_value: at_rank_one(&row.attack_range_value),
        attack_interval_value: at_rank_one(&row.attack_interval_value),
        speed_value: at_rank_one(&row.speed_value),
    }))
}

#[cfg(test)]
mod tests {
    use super::TechnologyEffects;
    use crate::data::{Channel, Correction, Index};

    /// Range Enhancement for the Marksman: `+40` metres and nothing else.
    const RANGE_ENHANCEMENT: i32 = 10202;
    /// Elite Marksman for the Fortress, whose effect grows with rank.
    const ELITE_MARKSMAN: i32 = 10801;
    /// Grenade Launcher for the Fang, which corrects a splash radius.
    const GRENADE_LAUNCHER: i32 = 3109;

    #[test]
    fn a_technology_writes_onto_the_unit_whose_table_row_names_it() {
        let table = TechnologyEffects::load().unwrap();
        let written = table.corrections(&[RANGE_ENHANCEMENT], "marksman").unwrap();
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].0, Channel::Skill);
        assert_eq!(written[0].1.index, Index::AttackRange);
        assert_eq!(written[0].1.correction, Correction::Value(40_000));

        assert!(
            table
                .corrections(&[RANGE_ENHANCEMENT], "arclight")
                .unwrap()
                .is_empty(),
            "another unit's Range Enhancement is another row"
        );
    }

    /// A technology whose effect grows is refused rather than read at rank
    /// one, because which entry a rank reads is not established.
    #[test]
    fn a_technology_that_grows_with_rank_is_refused() {
        let table = TechnologyEffects::load().unwrap();
        let refused = table
            .corrections(&[ELITE_MARKSMAN], "fortress")
            .unwrap_err()
            .to_string();
        assert!(refused.contains("10801"), "{refused}");
        assert!(refused.contains("rank"), "{refused}");
    }

    #[test]
    fn a_technology_correcting_a_number_this_build_lacks_is_refused() {
        let table = TechnologyEffects::load().unwrap();
        let refused = table
            .corrections(&[GRENADE_LAUNCHER], "fang")
            .unwrap_err()
            .to_string();
        assert!(refused.contains("splash_range_value"), "{refused}");
    }

    /// A technology that summons or debuffs is not in this table, and writes
    /// nothing rather than refusing the fight.
    #[test]
    fn a_technology_with_no_correction_writes_nothing() {
        let table = TechnologyEffects::load().unwrap();
        assert!(table.corrections(&[1201], "fortress").unwrap().is_empty());
    }
}
