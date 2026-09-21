//! What an officer writes onto a unit, and onto which units.
//!
//! `config/officer_effects.yaml` is the build's own table, extracted by
//! `scripts/extract-officer-effects.py`, and `docs/rules/officer_effects.md`
//! states what each field means and how it is encoded. This turns a row of it
//! into the corrections [`crate::data`] resolves, for the units the row
//! reaches.
//!
//! An officer this build cannot apply is refused by name rather than partly
//! applied. Three things make one: a field that corrects a number no mechanism
//! here reads, a field whose composition nobody has measured, and a targeting
//! category the build's data does not enumerate. A fight is not fought with
//! half an officer on it.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::{
    Error, Result,
    data::{Channel, Correction, Entry, Index},
};

use super::effects::{self, Fields, KILLS, PROJECTILE, SPLASH, VALUE_ELSEWHERE};

const DEFAULT_OFFICER_EFFECTS: &str = include_str!("../../../../config/officer_effects.yaml");

/// The module that tags every entry an officer writes, so that taking the
/// officer away takes its corrections with it.
pub(crate) const SOURCE: &str = "Modifier";

/// Every officer's combat effect, by the id a layout compiles to.
#[derive(Debug, Clone)]
pub(crate) struct OfficerEffects {
    officers: BTreeMap<i32, Officer>,
}

#[derive(Debug, Clone)]
struct Officer {
    /// Which units the row reaches, as the build stores it.
    targets: Targets,
    /// What it writes, or why this build will not apply it. The table loads
    /// whole either way: an officer nobody holds refuses nothing, and a fight
    /// is only refused for what its sides actually carry.
    effect: std::result::Result<Vec<(Channel, Index, Correction)>, String>,
}

#[derive(Debug, Clone)]
enum Targets {
    /// `mech_type` 0: every unit.
    Every,
    /// `mech_type` 1 and 10: the units the row lists.
    Listed(Vec<String>),
    /// A category this build does not resolve, carrying what to say about it.
    Refused(String),
}

/// One row of the table, with every field the extraction writes.
///
/// Unknown fields are refused rather than ignored: a field this build has
/// never seen is a decision about what it corrects, not a value to skip.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    id: i32,
    name: String,
    mech_type: i32,
    #[serde(default)]
    units: Vec<String>,
    #[serde(default)]
    damage_rate: Option<i64>,
    #[serde(default)]
    damage_rate_by_kill_count: Option<i64>,
    #[serde(default)]
    life_rate: Option<i64>,
    #[serde(default)]
    life_rate_by_kill_count: Option<i64>,
    #[serde(default)]
    attack_interval_rate: Option<i64>,
    #[serde(default)]
    attack_range_rate: Option<i64>,
    #[serde(default)]
    projectile_life_rate: Option<i64>,
    #[serde(default)]
    tower_life_rate: Option<i64>,
    #[serde(default)]
    energy_shield_rate: Option<i64>,
    #[serde(default)]
    land_mine_rate: Option<i64>,
    #[serde(default)]
    super_deployment_time_rate: Option<i64>,
    #[serde(default)]
    attack_range_value: Option<i64>,
    #[serde(default)]
    min_attack_range_value: Option<i64>,
    #[serde(default)]
    attack_interval_value: Option<i64>,
    #[serde(default)]
    splash_range_value: Option<i64>,
    #[serde(default)]
    projectile_speed_value: Option<i64>,
    #[serde(default)]
    speed_value: Option<i64>,
    #[serde(default)]
    extra_life: Option<i64>,
    #[serde(default)]
    exp_rate: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    schema: String,
    game_build: String,
    officers: Vec<Row>,
}

impl OfficerEffects {
    /// Reads the tracked table.
    ///
    /// # Errors
    ///
    /// Returns an error when the table is not the one this build reads.
    pub(crate) fn load() -> Result<Self> {
        Self::parse(DEFAULT_OFFICER_EFFECTS)
    }

    fn parse(text: &str) -> Result<Self> {
        let table: Table = serde_yaml::from_str(text).map_err(|error| {
            Error::new(format!("cannot read the officer effect table: {error}"))
        })?;
        if table.schema != "mechcore.officer_effects" {
            return Err(Error::new(format!(
                "officer effect table declares schema {:?}",
                table.schema
            )));
        }
        if table.game_build.trim().is_empty() {
            return Err(Error::new("officer effect table declares no game build"));
        }
        let mut officers = BTreeMap::new();
        for row in table.officers {
            let id = row.id;
            if officers.insert(id, Officer::of(&row)).is_some() {
                return Err(Error::new(format!(
                    "officer effect table holds officer {id} twice"
                )));
            }
        }
        Ok(Self { officers })
    }

    /// Every correction this side's officers write onto one unit type.
    ///
    /// An id the table does not hold writes nothing: the table carries the
    /// officers that correct a unit's numbers, and an officer that only
    /// discounts a price or hands out a squad is `config/officers.yaml`'s.
    /// The ids reaching here come from a compiled layout, whose officer names
    /// are validated against the catalogue, so an id is a real officer.
    ///
    /// # Errors
    ///
    /// Returns an error naming the officer when the side holds one this build
    /// cannot apply, rather than applying the part of it that it understands.
    pub(crate) fn corrections(
        &self,
        held: &[i32],
        unit_type: &str,
    ) -> Result<Vec<(Channel, Entry)>> {
        let mut written = Vec::new();
        for id in held {
            let Some(officer) = self.officers.get(id) else {
                continue;
            };
            let corrections = officer
                .effect
                .as_ref()
                .map_err(|why| Error::new(why.clone()))?;
            if corrections.is_empty() || !officer.reaches(unit_type)? {
                continue;
            }
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

impl Officer {
    fn of(row: &Row) -> Self {
        let name = row.name.clone();
        let targets = match row.mech_type {
            0 => Targets::Every,
            1 | 10 => Targets::Listed(row.units.clone()),
            // Type 11 corrects a tower, a shield or a mine. Its rows carry no
            // unit number at all, so the refusal below names the field first;
            // this is here so a future type-11 row with one is not silently
            // applied to every unit.
            11 => Targets::Refused(format!(
                "officer {} ({}) corrects a tower, a shield or a mine rather \
                 than a unit",
                row.id, name
            )),
            4 => Targets::Refused(format!(
                "officer {} ({}) targets ranged units, which neither the \
                 build's data nor docs/rules/officer_effects.md enumerates",
                row.id, name
            )),
            other => Targets::Refused(format!(
                "officer {} ({}) targets mech_type {other}, which this build \
                 does not read",
                row.id, name
            )),
        };
        Self {
            targets,
            effect: corrections_of(row),
        }
    }

    /// Whether this officer writes onto the given unit type.
    ///
    /// A refused targeting category is only an error for an officer that
    /// writes something: a row that corrects a tower reaches no unit either
    /// way, and refusing the fight over it would refuse a side for carrying an
    /// officer whose effect is not a unit's at all.
    fn reaches(&self, unit_type: &str) -> Result<bool> {
        match &self.targets {
            Targets::Every => Ok(true),
            Targets::Listed(units) => Ok(units.iter().any(|listed| listed == unit_type)),
            Targets::Refused(reason) => Err(Error::new(reason.clone())),
        }
    }
}

/// What a row writes, or why this build will not apply it.
///
/// The fields an officer shares with every other source of corrections are
/// [`super::effects`]'s; the ones only an officer carries are refused here,
/// each with what it would take to support it.
fn corrections_of(row: &Row) -> std::result::Result<Vec<(Channel, Index, Correction)>, String> {
    let unsupported = [
        (
            row.min_attack_range_value,
            "min_attack_range_value",
            VALUE_ELSEWHERE,
        ),
        (row.splash_range_value, "splash_range_value", SPLASH),
        (
            row.projectile_speed_value,
            "projectile_speed_value",
            PROJECTILE,
        ),
        (row.projectile_life_rate, "projectile_life_rate", PROJECTILE),
        (
            row.damage_rate_by_kill_count,
            "damage_rate_by_kill_count",
            KILLS,
        ),
        (
            row.life_rate_by_kill_count,
            "life_rate_by_kill_count",
            KILLS,
        ),
        (row.tower_life_rate, "tower_life_rate", ELSEWHERE),
        (row.energy_shield_rate, "energy_shield_rate", ELSEWHERE),
        (row.land_mine_rate, "land_mine_rate", ELSEWHERE),
        (
            row.super_deployment_time_rate,
            "super_deployment_time_rate",
            ELSEWHERE,
        ),
        (row.extra_life, "extra_life", ELSEWHERE),
        (row.exp_rate, "exp_rate", ELSEWHERE),
    ];
    for (value, field, why) in unsupported {
        if value.is_some_and(|value| value != 0) {
            return Err(format!(
                "officer {} ({}) writes {field}, and {why}",
                row.id, row.name
            ));
        }
    }
    Ok(effects::corrections(Fields {
        life_rate: row.life_rate,
        damage_rate: row.damage_rate,
        attack_range_rate: row.attack_range_rate,
        attack_interval_rate: row.attack_interval_rate,
        attack_range_value: row.attack_range_value,
        attack_interval_value: row.attack_interval_value,
        speed_value: row.speed_value,
    }))
}

const ELSEWHERE: &str = "it corrects a tower, a shield, a mine, a deployment \
                         clock or a side's experience rather than a unit's own \
                         number, and no mechanism here reads one";

#[cfg(test)]
mod tests {
    use super::{Channel, Index, OfficerEffects};
    use crate::data::Correction;

    /// Advanced Offensive Tactics, the officer the capture measured.
    const ADVANCED_OFFENSIVE_TACTICS: i32 = 20002;
    /// Advanced Defensive Tactics, the same shape on life instead.
    const ADVANCED_DEFENSIVE_TACTICS: i32 = 20001;
    /// Advanced Targeting System, whose `+10` of range reaches a category
    /// nothing enumerates.
    const ADVANCED_TARGETING_SYSTEM: i32 = 20006;
    /// Advanced Power System, whose `+3` of movement is a plain integer.
    const ADVANCED_POWER_SYSTEM: i32 = 20004;
    /// Aerial Specialist, which lists the units it reaches.
    const AERIAL_SPECIALIST: i32 = 20021;
    const THIRTY_PERCENT: i64 = 1_288_490_188;

    /// A plain integer is a value in the unit's own channel, in the units the
    /// description is quantized with: `+3` of movement is three metres a
    /// second.
    #[test]
    fn a_plain_integer_is_a_value_in_the_unit_channel() {
        let table = OfficerEffects::load().unwrap();
        let speed = table
            .corrections(&[ADVANCED_POWER_SYSTEM], "marksman")
            .unwrap();
        assert_eq!(speed.len(), 1);
        assert_eq!(speed[0].0, Channel::Unit);
        assert_eq!(speed[0].1.index, Index::MoveSpeed);
        assert_eq!(speed[0].1.correction, Correction::Value(3_000));
    }

    #[test]
    fn a_rate_lands_in_the_channel_its_recording_keeps_it_in() {
        let table = OfficerEffects::load().unwrap();
        let damage = table
            .corrections(&[ADVANCED_OFFENSIVE_TACTICS], "marksman")
            .unwrap();
        assert_eq!(damage.len(), 1);
        assert_eq!(damage[0].0, Channel::Skill);
        assert_eq!(damage[0].1.index, Index::AttackDamage);
        assert_eq!(
            damage[0].1.correction,
            Correction::Rate {
                add: THIRTY_PERCENT,
                reduce: 0
            }
        );

        let life = table
            .corrections(&[ADVANCED_DEFENSIVE_TACTICS], "rhino")
            .unwrap();
        assert_eq!(life.len(), 1);
        assert_eq!(life[0].0, Channel::Unit, "life is a unit's number");
        assert_eq!(life[0].1.index, Index::MaxLife);
    }

    /// Two of one officer are two entries, which the data layer then sums.
    #[test]
    fn holding_one_officer_twice_writes_it_twice() {
        let table = OfficerEffects::load().unwrap();
        let held = [ADVANCED_OFFENSIVE_TACTICS, ADVANCED_OFFENSIVE_TACTICS];
        assert_eq!(table.corrections(&held, "marksman").unwrap().len(), 2);
    }

    #[test]
    fn an_officer_reaches_only_the_units_its_row_lists() {
        let table = OfficerEffects::load().unwrap();
        assert_eq!(
            table
                .corrections(&[AERIAL_SPECIALIST], "wasp")
                .unwrap()
                .len(),
            2,
            "an air unit takes both of its rates"
        );
        assert!(
            table
                .corrections(&[AERIAL_SPECIALIST], "marksman")
                .unwrap()
                .is_empty(),
            "a ground unit takes neither"
        );
    }

    /// The table loads whole, and an officer this build cannot apply refuses
    /// the fight that carries it rather than the table that lists it.
    #[test]
    fn an_officer_this_build_cannot_apply_is_refused_by_name() {
        let table = OfficerEffects::load().unwrap();
        let refused = table
            .corrections(&[ADVANCED_TARGETING_SYSTEM], "marksman")
            .unwrap_err()
            .to_string();
        assert!(refused.contains("20006"), "{refused}");
        assert!(refused.contains("ranged units"), "{refused}");

        // Its `+10` of range is applied now, so what refuses it is which
        // units it reaches rather than what it writes.
    }

    /// An officer that only touches a ledger is not in this table, and writes
    /// nothing onto a unit rather than refusing the fight.
    #[test]
    fn an_officer_with_no_combat_effect_writes_nothing() {
        let table = OfficerEffects::load().unwrap();
        assert!(table.corrections(&[10001], "marksman").unwrap().is_empty());
    }
}
