//! The build's `IMechLevelData`, supplied before the dynamic data changes.
//! `attributeUpgradeDatas` is copied into `config/unit_levels.yaml`; the
//! level selects a row rather than generating a multiplier arithmetically.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::{
    Error, Result,
    data::{Channel, Correction, Entry, Index},
};

const TABLE: &str = include_str!("../../../../config/unit_levels.yaml");

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    level: i32,
    life_rating: i64,
    damage_rating: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    schema: String,
    game_build: String,
    levels: Vec<Row>,
}

pub(crate) struct LevelEffects {
    levels: BTreeMap<i32, Row>,
}

impl LevelEffects {
    pub(crate) fn load() -> Result<Self> {
        Self::parse(TABLE)
    }

    fn parse(text: &str) -> Result<Self> {
        let table: Table = serde_yaml::from_str(text)
            .map_err(|error| Error::new(format!("cannot read the unit level table: {error}")))?;
        if table.schema != "mechcore.unit_levels" || table.game_build != "1.11.1.3.2259" {
            return Err(Error::new(
                "unit level table has an unsupported schema or build",
            ));
        }
        let mut levels = BTreeMap::new();
        for row in table.levels {
            if row.life_rating <= 0 || row.damage_rating <= 0 {
                return Err(Error::new("unit level ratings must be positive"));
            }
            if levels.insert(row.level, row).is_some() {
                return Err(Error::new("unit level table repeats a level"));
            }
        }
        if levels.keys().copied().collect::<Vec<_>>() != (1..=9).collect::<Vec<_>>() {
            return Err(Error::new(
                "unit level table must contain levels 1 through 9",
            ));
        }
        Ok(Self { levels })
    }

    pub(crate) fn corrections(&self, level: i32) -> Result<Vec<(Channel, Entry)>> {
        let row = self.levels.get(&level).ok_or_else(|| {
            Error::new(format!(
                "unit level {level} is not supported: the build's table covers levels 1 through 9"
            ))
        })?;
        Ok([
            (Index::MaxLife, row.life_rating),
            (Index::AttackDamage, row.damage_rating),
        ]
        .into_iter()
        // Identity ratings need no entry, just as a zero officer rate does.
        .filter(|(_, rating)| *rating != 1 << 32)
        .map(|(index, raw)| {
            (
                Channel::Base,
                Entry {
                    index,
                    source: "Modifier.Level",
                    correction: Correction::Rating(raw),
                },
            )
        })
        .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{data::Stats, modifier::OfficerEffects, rules::SimulationConfig};

    #[test]
    fn native_levels_and_officer_compose_on_the_base() {
        let rules = SimulationConfig::load()
            .unwrap()
            .units
            .get("marksman")
            .unwrap()
            .clone();
        let table = LevelEffects::load().unwrap();
        let bare = Stats::of(&rules).unwrap();
        for (level, life, damage) in [
            (1, 1622, 2329),
            (2, 3244, 4658),
            (3, 4866, 6987),
            (9, 14598, 20961),
        ] {
            let written = table.corrections(level).unwrap();
            let stats = Stats::corrected(&rules, &written).unwrap();
            assert_eq!((stats.max_life(), stats.attack_damage()), (life, damage));
            assert!(stats.skill_damage_modifiers(1).is_empty());
            assert_eq!(
                (
                    stats.move_speed(),
                    stats.attack_range(),
                    stats.attack_interval()
                ),
                (
                    bare.move_speed(),
                    bare.attack_range(),
                    bare.attack_interval()
                )
            );
        }
        let mut written = table.corrections(2).unwrap();
        written.extend(
            OfficerEffects::load()
                .unwrap()
                .corrections(&[20002], "marksman")
                .unwrap(),
        );
        let stats = Stats::corrected(&rules, &written).unwrap();
        assert_eq!(stats.attack_damage(), 6055, "4658 x 1.3, not 2329 x 2.3");
        assert_eq!(stats.max_life(), 3244);
        let overlays = stats.skill_damage_modifiers(1);
        assert_eq!(overlays.len(), 1);
        assert_eq!(overlays[0].skill_slot, 0);
        assert_eq!(overlays[0].modifiers.damage_rate.add, 1_288_490_188);
        assert_eq!(overlays[0].modifiers.damage_rate.reduce, 0);
    }

    #[test]
    fn ratings_come_from_the_table_and_zero_stays_zero() {
        let table = LevelEffects::parse(
            &TABLE.replace("life_rating: 8589934592", "life_rating: 10737418240"),
        )
        .unwrap();
        let mut rules = SimulationConfig::load()
            .unwrap()
            .units
            .get("marksman")
            .unwrap()
            .clone();
        assert_eq!(
            Stats::corrected(&rules, &table.corrections(2).unwrap())
                .unwrap()
                .max_life(),
            4055
        );
        rules.max_life = 0;
        rules.attack.base_damage = 0;
        let stats = Stats::corrected(&rules, &table.corrections(9).unwrap()).unwrap();
        assert_eq!((stats.max_life(), stats.attack_damage()), (0, 0));
    }

    #[test]
    fn laser_ramp_uses_the_level_base_before_dynamic_rates() {
        let rules = SimulationConfig::load()
            .unwrap()
            .units
            .get("steel_ball")
            .unwrap()
            .clone();
        let levels = LevelEffects::load().unwrap();
        let mut written = levels.corrections(2).unwrap();
        let stats = Stats::corrected(&rules, &written).unwrap();
        assert_eq!(
            stats.laser_damage(&rules, 4),
            63,
            "110 x the fifth native multiplier, truncated after scaling"
        );
        written.extend(
            OfficerEffects::load()
                .unwrap()
                .corrections(&[20002], "steel_ball")
                .unwrap(),
        );
        let stats = Stats::corrected(&rules, &written).unwrap();
        assert_eq!(
            stats.laser_damage(&rules, 4),
            81,
            "the +0.3 rate corrects the integer ramp damage"
        );
    }

    #[test]
    fn levels_outside_the_extracted_table_are_refused() {
        let table = LevelEffects::load().unwrap();
        for level in [0, 10] {
            let error = table.corrections(level).unwrap_err().to_string();
            assert!(error.contains(&format!("unit level {level}")), "{error}");
        }
    }
}
