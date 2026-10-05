//! What an officer writes onto a unit, and onto which units.
//!
//! `config/officer_effects.yaml` is the build's own table, extracted by
//! `scripts/extract/extract-officer-effects.py`, and `docs/rules/officer_effects.md`
//! states what each field means and how it is encoded. This turns a row of it
//! into the corrections [`crate::data`] resolves, for the units the row
//! reaches.
//!
//! The table holds only the officers a standard 1v1 side can hold, and each
//! of their fields is applied. An officer whose targeting category the
//! build's data does not enumerate is refused by name rather than partly
//! applied: a fight is not fought with half an officer on it.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::{
    Error, Result,
    data::{Channel, Correction, Entry, ExperienceRate, Index},
    rules::UnitConfig,
};

use super::{
    effects::{self, Fields},
    targets::Targets,
};

const DEFAULT_OFFICER_EFFECTS: &str = include_str!("../../../../config/officer_effects.yaml");

/// The module that tags every entry an officer writes, so that taking the
/// officer away takes its corrections with it.
pub(crate) const SOURCE: &str = "Modifier";

/// Every officer's combat effect, by the id a layout compiles to.
#[derive(Debug, Clone)]
pub(crate) struct OfficerEffects {
    officers: BTreeMap<i32, Officer>,
}

/// The rates a side's officers add onto its contraptions, Q32.32:
/// `EnergyShieldContraption.energyRate` and `LandMineContraption.damageRate`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ContraptionRates {
    pub(crate) shield_energy: i64,
    pub(crate) missile_damage: i64,
}

#[derive(Debug, Clone)]
struct Officer {
    /// Which units the row reaches, as the build stores it.
    targets: Targets,
    /// `superDeploymentTimeChangeRate`, Q32.32: a side's number, not a
    /// unit's.
    super_deployment_time_rate: i64,
    /// `expChangeRate`, Q32.32: what the officer answers as an
    /// `IUnitDataChangeDataSource`, onto the card of a unit it reaches. As an
    /// `ICommonMechDataChangeDataSource` `OfficerData.GetExpChangeRate`
    /// answers zero, so it writes nothing onto the unit itself.
    exp_rate: i64,
    /// `energyShieldChangeRate` and `landMineChangeRate`, Q32.32: what
    /// `SystemOfficerController` adds onto its side's shield and missile.
    contraption_rates: ContraptionRates,
    /// What it writes onto a unit it reaches.
    effect: Vec<(Channel, Index, Correction)>,
}

/// One row of the table, with every field the extraction writes.
///
/// Unknown fields are refused rather than ignored: a field this build has
/// never seen is a decision about what it corrects, not a value to skip. That
/// holds for a field the build's officers answer but no standard officer
/// carries, such as a tower's life or a life per kill.
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
    attack_interval_rate: Option<i64>,
    #[serde(default)]
    attack_range_rate: Option<i64>,
    #[serde(default)]
    energy_shield_rate: Option<i64>,
    #[serde(default)]
    land_mine_rate: Option<i64>,
    #[serde(default)]
    super_deployment_time_rate: Option<i64>,
    #[serde(default)]
    attack_range_value: Option<i64>,
    #[serde(default)]
    attack_interval_value: Option<i64>,
    #[serde(default)]
    splash_range_value: Option<i64>,
    #[serde(default)]
    speed_value: Option<i64>,
    #[serde(default)]
    exp_rate: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    schema: String,
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
        let mut officers = BTreeMap::new();
        for row in table.officers {
            let id = row.id;
            if officers.insert(id, Officer::of(&row)).is_some() {
                return Err(Error::new(format!(
                    "officer effect table holds officer {id} twice"
                )));
            }
        }
        // Which of two officers that set the travel time rate holds is not
        // measured, and no standard side can hold two.
        let setters = officers
            .values()
            .filter(|officer| officer.super_deployment_time_rate != 0)
            .count();
        if setters > 1 {
            return Err(Error::new(format!(
                "officer effect table holds {setters} officers that set the travel time rate"
            )));
        }
        Ok(Self { officers })
    }

    /// `FightTeam.superDeploymentTimeChangeRate`: the rate a side's officers
    /// set on its travel time, `SystemOfficerFightController` through
    /// `FightTeam.SetSuperDeploymentTimeChangeRate`.
    ///
    /// The setter sets rather than adds, so it is the rate of the one officer
    /// that sets it: the table holds one, and a layout never holds that
    /// officer twice, since the pool deals it once.
    pub(crate) fn super_deployment_time_rate(&self, held: &[i32]) -> i64 {
        held.iter()
            .filter_map(|id| self.officers.get(id))
            .map(|officer| officer.super_deployment_time_rate)
            .find(|rate| *rate != 0)
            .unwrap_or(0)
    }

    /// What this side's officers add onto its contraptions:
    /// `SystemOfficerController.ChangeConstraptionEnergyShield` and
    /// `ChangeConstraptionLandMine` hand each officer's rate to its side's
    /// `ContraptionManager`, which adds it onto the one shield and the one
    /// missile contraption every placement of that kind reads.
    pub(crate) fn contraption_rates(&self, held: &[i32]) -> ContraptionRates {
        held.iter().filter_map(|id| self.officers.get(id)).fold(
            ContraptionRates::default(),
            |sum, officer| ContraptionRates {
                shield_energy: sum
                    .shield_energy
                    .saturating_add(officer.contraption_rates.shield_energy),
                missile_damage: sum
                    .missile_damage
                    .saturating_add(officer.contraption_rates.missile_damage),
            },
        )
    }

    /// Every correction this side's officers write onto one unit.
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
        unit: &UnitConfig,
    ) -> Result<Vec<(Channel, Entry)>> {
        let mut written = Vec::new();
        for id in held {
            let Some(officer) = self.officers.get(id) else {
                continue;
            };
            if officer.effect.is_empty() || !officer.reaches(unit)? {
                continue;
            }
            for (channel, index, correction) in &officer.effect {
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
    /// The rate this side's officers put on what a formation of this unit
    /// gains: each one that reaches the unit writes its `expChangeRate` onto
    /// the unit's card, which hands the aggregate to the formation.
    ///
    /// # Errors
    ///
    /// Returns the refusal of a targeting category this build does not
    /// resolve, for an officer that carries a rate.
    pub(crate) fn experience_rate(
        &self,
        held: &[i32],
        unit: &UnitConfig,
    ) -> Result<ExperienceRate> {
        let mut rate = ExperienceRate::default();
        for id in held {
            let Some(officer) = self.officers.get(id) else {
                continue;
            };
            if officer.exp_rate != 0 && officer.reaches(unit)? {
                rate = rate.with(officer.exp_rate);
            }
        }
        Ok(rate)
    }
}

impl Officer {
    fn of(row: &Row) -> Self {
        let who = format!("officer {} ({})", row.id, row.name);
        let targets = Targets::of(row.mech_type, &row.units, &who);
        Self {
            targets,
            super_deployment_time_rate: row.super_deployment_time_rate.unwrap_or(0),
            exp_rate: row.exp_rate.unwrap_or(0),
            contraption_rates: ContraptionRates {
                shield_energy: row.energy_shield_rate.unwrap_or(0),
                missile_damage: row.land_mine_rate.unwrap_or(0),
            },
            effect: corrections_of(row),
        }
    }

    /// Whether this officer writes onto the given unit.
    ///
    /// A refused targeting category is only an error for an officer that
    /// writes something: a device officer, type 11, rates its side's
    /// contraptions and reaches no unit either way, and refusing the fight
    /// over it would refuse a side for carrying an officer whose effect is
    /// not a unit's at all.
    fn reaches(&self, unit: &UnitConfig) -> Result<bool> {
        self.targets.reaches(unit)
    }
}

/// What a row writes: the fields an officer shares with every other source
/// of corrections, [`super::effects`]'s.
fn corrections_of(row: &Row) -> Vec<(Channel, Index, Correction)> {
    effects::corrections(Fields {
        life_rate: row.life_rate,
        damage_rate: row.damage_rate,
        damage_rate_by_kill_count: row.damage_rate_by_kill_count,
        attack_range_rate: row.attack_range_rate,
        attack_interval_rate: row.attack_interval_rate,
        attack_range_value: row.attack_range_value,
        attack_interval_value: row.attack_interval_value,
        splash_range_value: row.splash_range_value,
        speed_value: row.speed_value,
        damage_reduce_rate_base: None,
    })
}

#[cfg(test)]
mod tests {
    use super::{Channel, Index, OfficerEffects};
    use crate::{
        data::{Correction, ExperienceRate},
        rules::{UnitConfig, UnitConfigs},
    };

    fn unit(name: &str) -> UnitConfig {
        UnitConfigs::load().unwrap().get(name).unwrap().clone()
    }

    /// Advanced Offensive Tactics, the officer the capture measured.
    const ADVANCED_OFFENSIVE_TACTICS: i32 = 20002;
    /// Advanced Defensive Tactics, the same shape on life instead.
    const ADVANCED_DEFENSIVE_TACTICS: i32 = 20001;
    /// Advanced Targeting System, whose `+10` of range reaches ranged units.
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
            .corrections(&[ADVANCED_POWER_SYSTEM], &unit("marksman"))
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
            .corrections(&[ADVANCED_OFFENSIVE_TACTICS], &unit("marksman"))
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
            .corrections(&[ADVANCED_DEFENSIVE_TACTICS], &unit("rhino"))
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
        assert_eq!(
            table.corrections(&held, &unit("marksman")).unwrap().len(),
            2
        );
    }

    #[test]
    fn an_officer_reaches_only_the_units_its_row_lists() {
        let table = OfficerEffects::load().unwrap();
        assert_eq!(
            table
                .corrections(&[AERIAL_SPECIALIST], &unit("wasp"))
                .unwrap()
                .len(),
            2,
            "an air unit takes both of its rates"
        );
        assert!(
            table
                .corrections(&[AERIAL_SPECIALIST], &unit("marksman"))
                .unwrap()
                .is_empty(),
            "a ground unit takes neither"
        );
    }

    /// A Ranged row reaches every unit whose main skill is not a melee
    /// attack, whatever its attack path, and no melee unit:
    /// `tests/modifier/fights/targeting-ranged.yaml` pins Advanced Targeting System
    /// on all six of these.
    #[test]
    fn a_ranged_officer_reaches_every_unit_that_is_not_melee() {
        let table = OfficerEffects::load().unwrap();
        for ranged in ["marksman", "arclight", "fang", "steel_ball"] {
            let written = table
                .corrections(&[ADVANCED_TARGETING_SYSTEM], &unit(ranged))
                .unwrap();
            assert_eq!(written.len(), 1, "{ranged}");
            assert_eq!(written[0].1.index, Index::AttackRange);
            assert_eq!(written[0].1.correction, Correction::Value(10_000));
        }
        for melee in ["rhino", "crawler"] {
            assert!(
                table
                    .corrections(&[ADVANCED_TARGETING_SYSTEM], &unit(melee))
                    .unwrap()
                    .is_empty(),
                "{melee}"
            );
        }
    }

    /// Smart Marksman's `exp_rate` is its formation's, not a correction on
    /// the unit, and reaches only the unit its row lists:
    /// `tests/modifier/fights/officer-exp-rate-marksman.yaml`.
    #[test]
    fn an_experience_rate_is_the_formations_and_reaches_its_listed_unit() {
        const SMART_MARKSMAN: i32 = 30202;
        let table = OfficerEffects::load().unwrap();
        let marksman = unit("marksman");
        assert!(
            table
                .corrections(&[SMART_MARKSMAN], &marksman)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            table.experience_rate(&[SMART_MARKSMAN], &marksman).unwrap(),
            ExperienceRate {
                add: 3_221_225_472,
                remaining: 1 << 32,
            }
        );
        assert_eq!(
            table
                .experience_rate(&[SMART_MARKSMAN], &unit("arclight"))
                .unwrap(),
            ExperienceRate::default()
        );
    }

    /// Advanced Shield Device and Advanced Missile Device write nothing onto
    /// a unit and add their rates onto the side's contraptions, which sum:
    /// `tests/shield/fights/advanced-shield-device.yaml`,
    /// `tests/missile/fights/advanced-missile-device.yaml`.
    #[test]
    fn a_device_officer_rates_its_sides_contraptions() {
        use super::ContraptionRates;
        const SHIELD_DEVICE: i32 = 10007;
        const MISSILE_DEVICE: i32 = 10008;
        let table = OfficerEffects::load().unwrap();
        for id in [SHIELD_DEVICE, MISSILE_DEVICE] {
            assert!(
                table
                    .corrections(&[id], &unit("marksman"))
                    .unwrap()
                    .is_empty()
            );
        }
        assert_eq!(
            table.contraption_rates(&[SHIELD_DEVICE, MISSILE_DEVICE, MISSILE_DEVICE]),
            ContraptionRates {
                shield_energy: 1_717_986_918,
                missile_damage: 2 * (2_i64 << 32),
            }
        );
    }

    /// An officer that only touches a ledger is not in this table, and writes
    /// nothing onto a unit rather than refusing the fight.
    #[test]
    fn an_officer_with_no_combat_effect_writes_nothing() {
        let table = OfficerEffects::load().unwrap();
        assert!(
            table
                .corrections(&[10001], &unit("marksman"))
                .unwrap()
                .is_empty()
        );
    }
}
