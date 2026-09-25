//! What an equipment writes onto the unit that wears it.
//!
//! `config/equipment_effects.yaml` is the build's table of ordinary
//! `EquipmentData` rows, extracted by `scripts/extract-equipment-effects.py`,
//! and `docs/rules/equipment_effects.md` states what each field means.
//! `Equipment.AddData` writes through `MechDataModifer.TryAddCommonData` and
//! `SkillDataModifier.AddData`, the writers an officer's correction goes
//! through, so a row becomes the corrections [`super::effects`] makes of an
//! officer's, in the same channels, and sums with an officer's there.
//!
//! An equipment this build cannot apply is refused by name rather than partly
//! applied: an item of another `EquipmentData` class, a lifetime this build
//! has not measured, a field no mechanism here reads, and a targeting this
//! build does not resolve.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::{
    Error, Result,
    data::{Channel, Correction, Entry, Index},
    rules::UnitConfig,
};

use super::{
    effects::{self, Fields, PROJECTILE, SPLASH, VALUE_ELSEWHERE},
    targets::Targets,
};

const DEFAULT_EQUIPMENT_EFFECTS: &str = include_str!("../../../../config/equipment_effects.yaml");

/// The module that tags every entry an equipment writes.
pub(crate) const SOURCE: &str = "Modifier";

/// Every ordinary equipment's combat effect, by the id a layout compiles to.
#[derive(Debug, Clone)]
pub(crate) struct EquipmentEffects {
    equipment: BTreeMap<i32, Equipment>,
}

#[derive(Debug, Clone)]
struct Equipment {
    /// Which units the row reaches.
    targets: Targets,
    /// What it writes, or why this build will not apply it.
    effect: std::result::Result<Vec<(Channel, Index, Correction)>, String>,
}

/// One row of the table, with every field the extraction writes.
///
/// A field is written only when it is set, so every one but the id, the name
/// and the targeting defaults to zero or false.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "EquipmentData's serialized flags are independent fields"
)]
struct Row {
    id: i32,
    name: String,
    mech_type: Vec<i32>,
    #[serde(default)]
    units: Vec<i32>,
    #[serde(default)]
    main_skill_effect: bool,
    #[serde(default)]
    #[allow(
        dead_code,
        reason = "names skills the simulator does not give a unit; see corrections_of"
    )]
    extra_skill_effect: bool,
    #[serde(default)]
    permanent_effect: bool,
    #[serde(default)]
    important_unit: bool,
    #[serde(default)]
    round_duration: i32,
    #[serde(default)]
    life_rate: Option<i64>,
    #[serde(default)]
    damage_rate: Option<i64>,
    #[serde(default)]
    attack_range_rate: Option<i64>,
    #[serde(default)]
    attack_interval_rate: Option<i64>,
    #[serde(default)]
    projectile_life_rate: Option<i64>,
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
    exp_rate: Option<i64>,
    #[serde(default)]
    grade_upper_limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    schema: String,
    equipment: Vec<Row>,
}

impl EquipmentEffects {
    /// Reads the tracked table.
    ///
    /// # Errors
    ///
    /// Returns an error when the table is not the one this build reads.
    pub(crate) fn load() -> Result<Self> {
        let table: Table = serde_yaml::from_str(DEFAULT_EQUIPMENT_EFFECTS).map_err(|error| {
            Error::new(format!("cannot read the equipment effect table: {error}"))
        })?;
        if table.schema != "mechcore.equipment_effects" {
            return Err(Error::new(format!(
                "equipment effect table declares schema {:?}",
                table.schema
            )));
        }
        let mut equipment = BTreeMap::new();
        for row in table.equipment {
            let id = row.id;
            if equipment.insert(id, Equipment::of(&row)).is_some() {
                return Err(Error::new(format!(
                    "equipment effect table holds equipment {id} twice"
                )));
            }
        }
        Ok(Self { equipment })
    }

    /// What one equipment writes onto the unit wearing it, in `round`.
    ///
    /// # Errors
    ///
    /// Returns an error naming the equipment when this build cannot apply it,
    /// rather than applying the part of it that it understands.
    pub(crate) fn corrections(
        &self,
        id: i32,
        unit: &UnitConfig,
        round: i32,
    ) -> Result<Vec<(Channel, Entry)>> {
        let Some(equipment) = self.equipment.get(&id) else {
            let name = mechcore_document::names::equipment_name(id).unwrap_or("unnamed");
            return Err(Error::new(format!(
                "equipment {id} ({name}) is not an ordinary EquipmentData row: \
                 its effect belongs to another equipment class, which no \
                 mechanism here reads"
            )));
        };
        // `Equipment.durability` is what an item carries from one round to
        // the next, and no fixture here has worn one past the first.
        if round != 1 {
            return Err(Error::new(format!(
                "equipment {id} is worn in round {round}, and what its durability \
                 does after round 1 is not established"
            )));
        }
        let corrections = equipment
            .effect
            .as_ref()
            .map_err(|why| Error::new(why.clone()))?;
        if !equipment.targets.reaches(unit)? {
            return Ok(Vec::new());
        }
        Ok(corrections
            .iter()
            .map(|(channel, index, correction)| {
                (
                    *channel,
                    Entry {
                        index: *index,
                        source: SOURCE,
                        correction: *correction,
                    },
                )
            })
            .collect())
    }
}

impl Equipment {
    fn of(row: &Row) -> Self {
        let who = format!("equipment {} ({})", row.id, row.name);
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
            effect: corrections_of(row, &who),
        }
    }
}

/// What a row writes, or why this build will not apply it.
///
/// The simulator gives a unit one skill, its main one, so a row reaches it
/// only through `mainSkillEffect`; `extraSkillEffect` names skills this
/// simulator does not give a unit, and is read neither way.
fn corrections_of(
    row: &Row,
    who: &str,
) -> std::result::Result<Vec<(Channel, Index, Correction)>, String> {
    if !row.main_skill_effect {
        return Err(format!(
            "{who} leaves the main skill out, and no other skill is simulated"
        ));
    }
    let lifetimes = [
        (row.round_duration != 0, "round_duration"),
        (row.permanent_effect, "permanent_effect"),
        (row.important_unit, "important_unit"),
    ];
    for (set, field) in lifetimes {
        if set {
            return Err(format!(
                "{who} sets {field}, whose effect is not established"
            ));
        }
    }
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
        (row.exp_rate, "exp_rate", VALUE_ELSEWHERE),
        (row.grade_upper_limit, "grade_upper_limit", VALUE_ELSEWHERE),
    ];
    for (value, field, why) in unsupported {
        if value.is_some_and(|value| value != 0) {
            return Err(format!("{who} writes {field}, and {why}"));
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

#[cfg(test)]
mod tests {
    use super::EquipmentEffects;
    use crate::{
        data::{Channel, Stats},
        modifier::OfficerEffects,
        rules::{UnitConfig, UnitConfigs},
    };

    const LASER_SIGHTS: i32 = 13_030_001;
    const HEAVY_ARMOR: i32 = 13_030_002;
    const IMPROVED_FIREPOWER: i32 = 13_030_003;
    const DOMINION_CORE: i32 = 13_030_010;
    const RAPID_LOADER: i32 = 13_030_011;
    const BARRIER: i32 = 1_307_001;
    const ADVANCED_DEFENSIVE_TACTICS: i32 = 20001;
    const ADVANCED_OFFENSIVE_TACTICS: i32 = 20002;

    fn unit(name: &str) -> UnitConfig {
        UnitConfigs::load().unwrap().get(name).unwrap().clone()
    }

    /// An equipment's rate sums with an officer's in one channel:
    /// `tests/equipment/` recorded 2838 and 3325 of life, 3842 and 4541 of
    /// damage, and 160 m of range.
    #[test]
    fn an_equipment_sums_with_an_officer_in_one_channel() {
        let marksman = unit("marksman");
        let equipment = EquipmentEffects::load().unwrap();
        let officers = OfficerEffects::load().unwrap();
        let resolve = |corrections: &[_]| Stats::corrected(&marksman, 1, corrections).unwrap();

        let mut life = equipment.corrections(HEAVY_ARMOR, &marksman, 1).unwrap();
        assert_eq!(life[0].0, Channel::Unit);
        assert_eq!(resolve(&life).max_life(), 2838);
        life.extend(
            officers
                .corrections(&[ADVANCED_DEFENSIVE_TACTICS], &marksman)
                .unwrap(),
        );
        assert_eq!(resolve(&life).max_life(), 3325);

        let mut damage = equipment
            .corrections(IMPROVED_FIREPOWER, &marksman, 1)
            .unwrap();
        assert_eq!(damage[0].0, Channel::Skill);
        assert_eq!(resolve(&damage).attack_damage(), 3842);
        damage.extend(
            officers
                .corrections(&[ADVANCED_OFFENSIVE_TACTICS], &marksman)
                .unwrap(),
        );
        assert_eq!(resolve(&damage).attack_damage(), 4541);

        let range = equipment.corrections(LASER_SIGHTS, &marksman, 1).unwrap();
        assert_eq!(resolve(&range).attack_range(), 160_000);
    }

    /// Laser Sights is a Ranged row, and reaches a melee unit as nothing.
    #[test]
    fn a_ranged_equipment_writes_nothing_onto_a_melee_unit() {
        let equipment = EquipmentEffects::load().unwrap();
        assert!(
            equipment
                .corrections(LASER_SIGHTS, &unit("rhino"), 1)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            equipment
                .corrections(LASER_SIGHTS, &unit("steel_ball"), 1)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn an_equipment_this_build_cannot_apply_is_refused_by_name() {
        let equipment = EquipmentEffects::load().unwrap();
        let marksman = unit("marksman");
        for (id, round, said) in [
            (BARRIER, 1, "barrier"),
            (RAPID_LOADER, 1, "round_duration"),
            (DOMINION_CORE, 1, "important_unit"),
            (HEAVY_ARMOR, 2, "durability"),
        ] {
            let refused = equipment
                .corrections(id, &marksman, round)
                .unwrap_err()
                .to_string();
            assert!(refused.contains(&id.to_string()), "{refused}");
            assert!(refused.contains(said), "{refused}");
        }
    }
}
