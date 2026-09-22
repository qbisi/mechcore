//! Equipment.AddData uses the same common correction fields as an officer.
//! Only ordinary `EquipmentData` at first deployment is read here. A subtype,
//! lifetime, targeting or skill selection this table cannot resolve is refused.

use super::effects::{self, Fields};
use crate::{
    Error, Result,
    data::{Channel, Entry},
    rules::{AttackPath, UnitConfig},
};
use serde::Deserialize;
use std::collections::BTreeMap;

const TABLE: &str = include_str!("../../../../config/equipment_effects.yaml");

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "serialized EquipmentData flags are independent native fields"
)]
struct Row {
    id: i32,
    name: String,
    #[serde(rename = "level")]
    _level: i64,
    #[serde(rename = "scope")]
    _scope: i64,
    #[serde(rename = "supply")]
    _supply: i64,
    #[serde(rename = "reactor_core")]
    _reactor_core: i64,
    #[serde(rename = "limited_scene")]
    _limited_scene: Vec<i32>,
    #[serde(rename = "earliest_round")]
    _earliest_round: i64,
    #[serde(rename = "latest_round")]
    _latest_round: i64,
    #[serde(rename = "can_repeated")]
    _can_repeated: bool,
    permanent_effect: bool,
    main_skill_effect: bool,
    extra_skill_effect: bool,
    round_duration: i64,
    #[serde(rename = "round_supply")]
    _round_supply: i64,
    life_rate: i64,
    damage_rate: i64,
    speed_value: i64,
    min_attack_range_value: i64,
    attack_range_value: i64,
    attack_range_rate: i64,
    attack_interval_value: i64,
    attack_interval_rate: i64,
    splash_range_value: i64,
    projectile_speed_value: i64,
    projectile_life_rate: i64,
    important_unit: bool,
    mech_type: Vec<i32>,
    units: Vec<i32>,
    exp_rate: i64,
    #[serde(rename = "upgrade_supply_rate")]
    _upgrade_supply_rate: i64,
    #[serde(rename = "upgrade_supply_value")]
    _upgrade_supply_value: i64,
    #[serde(rename = "supply_value")]
    _supply_value: i64,
    #[serde(rename = "destroy_huge_mech_earnings_value")]
    _destroy_huge_mech_earnings_value: i64,
    grade_upper_limit: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    schema: String,
    game_build: String,
    equipment: Vec<Row>,
}

#[derive(Deserialize)]
struct Names {
    equipment: BTreeMap<i32, String>,
}

pub(crate) struct EquipmentEffects {
    rows: BTreeMap<i32, Row>,
    names: BTreeMap<i32, String>,
}

impl EquipmentEffects {
    pub(crate) fn load() -> Result<Self> {
        let table: Table = serde_yaml::from_str(TABLE)
            .map_err(|e| Error::new(format!("cannot read equipment effects: {e}")))?;
        if table.schema != "mechcore.equipment_effects" || table.game_build != "1.11.1.3.2259" {
            return Err(Error::new(
                "equipment effect table is not build 1.11.1.3.2259",
            ));
        }
        let mut rows = BTreeMap::new();
        for row in table.equipment {
            let id = row.id;
            if rows.insert(id, row).is_some() {
                return Err(Error::new(format!("equipment effect table repeats {id}")));
            }
        }
        let names: Names = serde_yaml::from_str(include_str!("../../../../config/names.yaml"))
            .map_err(|e| Error::new(format!("cannot read equipment names: {e}")))?;
        Ok(Self {
            rows,
            names: names.equipment,
        })
    }

    pub(crate) fn corrections(
        &self,
        id: i32,
        rules: &UnitConfig,
        round: i32,
        level: i32,
    ) -> Result<Vec<(Channel, Entry)>> {
        let name = self
            .names
            .get(&id)
            .map_or("unnamed EquipmentData", String::as_str);
        let refuse = |why: &str| Error::new(format!("equipment {name} ({id}): {why}"));
        let row = self.rows.get(&id).ok_or_else(|| refuse(
            "a separate EquipmentData subclass owns this effect; ordinary corrections do not describe it"
        ))?;
        if round != 1 {
            return Err(refuse(
                "roundDuration and durability after round 1 are not established",
            ));
        }
        if level != 1 {
            return Err(refuse(
                "IsLocked(CardLevel) outside level 1 is not established",
            ));
        }
        if row.round_duration != 0 {
            return Err(refuse("roundDuration is not established"));
        }
        if row.permanent_effect {
            return Err(refuse("permanentEffect is not established"));
        }
        if row.important_unit {
            return Err(refuse("importantUnit effect is not established"));
        }
        if !row.main_skill_effect {
            return Err(refuse(
                "mainSkillEffect=false needs separate skill selection",
            ));
        }
        // MechType=4 is not enumerated by the table. The native first-deployment
        // recordings establish the Marksman case; no other unit is guessed.
        if row.mech_type != [0] && !(row.mech_type == [4] && rules.type_name == "marksman") {
            return Err(refuse(
                "mechType targeting outside the recorded Marksman is not established",
            ));
        }
        if !row.units.is_empty() {
            return Err(refuse("unitID targeting is not established"));
        }
        if !row.extra_skill_effect && rules.type_name != "marksman" {
            return Err(refuse(
                "extraSkillEffect=false needs separate skill selection",
            ));
        }
        for (field, value) in [
            ("minAttackRangeChangeValue", row.min_attack_range_value),
            ("splashRangeChangeValue", row.splash_range_value),
            ("projectileSpeedChangeValue", row.projectile_speed_value),
            ("projectileLifeChangeRate", row.projectile_life_rate),
            ("expChangeRate", row.exp_rate),
            ("changeGradeUpperLimit", row.grade_upper_limit),
        ] {
            if value != 0 {
                return Err(refuse(&format!("{} writes unsupported {field}", row.name)));
            }
        }
        // The laser currently reads its raw description directly. Do not let a
        // damage correction appear supported until its Stats read is integrated.
        if row.damage_rate != 0 && matches!(rules.attack.path, AttackPath::Laser { .. }) {
            return Err(refuse("laser damage does not read Stats corrections"));
        }
        Ok(effects::corrections(Fields {
            life_rate: Some(row.life_rate),
            damage_rate: Some(row.damage_rate),
            attack_range_rate: Some(row.attack_range_rate),
            attack_interval_rate: Some(row.attack_interval_rate),
            attack_range_value: Some(row.attack_range_value),
            attack_interval_value: Some(row.attack_interval_value),
            speed_value: Some(row.speed_value),
        })
        .into_iter()
        .map(|(channel, index, correction)| {
            (
                channel,
                Entry {
                    index,
                    correction,
                    source: "Modifier.Equipment",
                },
            )
        })
        .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::EquipmentEffects;
    use crate::{data::Stats, modifier::OfficerEffects, rules::UnitConfigs};

    #[test]
    fn native_equipment_and_officer_numbers_share_the_same_channel() {
        let units = UnitConfigs::load().unwrap();
        let marksman = units.get("marksman").unwrap();
        let equipment = EquipmentEffects::load().unwrap();
        let officers = OfficerEffects::load().unwrap();
        let mut armor = equipment.corrections(13_030_002, marksman, 1, 1).unwrap();
        let base = Stats::corrected(marksman, &armor).unwrap();
        assert_eq!(base.max_life(), 2838);
        armor.extend(officers.corrections(&[20001], "marksman").unwrap());
        assert_eq!(Stats::corrected(marksman, &armor).unwrap().max_life(), 3325);
        let mut damage = equipment.corrections(13_030_003, marksman, 1, 1).unwrap();
        assert_eq!(
            Stats::corrected(marksman, &damage).unwrap().attack_damage(),
            3842
        );
        damage.extend(officers.corrections(&[20002], "marksman").unwrap());
        assert_eq!(
            Stats::corrected(marksman, &damage).unwrap().attack_damage(),
            4541
        );
        let range = equipment.corrections(13_030_001, marksman, 1, 1).unwrap();
        assert_eq!(
            Stats::corrected(marksman, &range).unwrap().attack_range(),
            160_000
        );
    }

    #[test]
    fn no_equipment_is_partly_applied_across_an_unknown_boundary() {
        let units = UnitConfigs::load().unwrap();
        let marksman = units.get("marksman").unwrap();
        let table = EquipmentEffects::load().unwrap();
        for (&id, name) in &table.names {
            if !table.rows.contains_key(&id) {
                let error = table
                    .corrections(id, marksman, 1, 1)
                    .unwrap_err()
                    .to_string();
                assert!(error.contains(name), "{error}");
                assert!(error.contains("subclass"), "{error}");
            }
        }
        for (id, round, level, field) in [
            (13_030_002, 2, 1, "durability"),
            (13_030_002, 1, 2, "IsLocked"),
            (13_030_011, 1, 1, "roundDuration"),
            (13_030_010, 1, 1, "importantUnit"),
        ] {
            let error = table
                .corrections(id, marksman, round, level)
                .unwrap_err()
                .to_string();
            assert!(error.contains(field), "{error}");
        }
        let rhino = units.get("rhino").unwrap();
        assert!(
            table
                .corrections(13_030_001, rhino, 1, 1)
                .unwrap_err()
                .to_string()
                .contains("mechType")
        );
    }
}
