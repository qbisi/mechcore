//! What a layout's battle skills do once released.
//!
//! [`config/commander_skill_effects.yaml`](../../../../config/commander_skill_effects.yaml),
//! which `scripts/extract/extract-commander-skill-effects.py` writes, holds the
//! skills the fight releases, the buff skills and the support skills;
//! `docs/rules/battle_skill.md` states what they do. Every other battle skill
//! is refused by name.

use mechcore_document::BattleSkill;
use serde::Deserialize;

use crate::{
    Error, Result,
    rules::{UnitConfig, UnitConfigs},
};

const DEFAULT_EFFECTS: &str = include_str!("../../../../config/commander_skill_effects.yaml");
const SPACE: i64 = 1_000;
/// `FightUtility.LogicDeltaTime`, the seconds of one tick as the build's
/// `FPoint` holds them, which a row's times are divided by.
const LOGIC_DELTA_RAW: i64 = 0x0CCC_CCCC;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    schema: String,
    buff_skills: Vec<BuffSkillRow>,
    support_skills: Vec<SupportSkillRow>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SupportSkillRow {
    id: i32,
    name: String,
    scope: i32,
    effect_range_type: i32,
    effect_type: i32,
    unit_type_id: u32,
    max_count: u32,
    create_count_per_time: u32,
    max_batch: u32,
    appear_type: i32,
    start_time: i64,
    effect_range: i64,
    sub_effect_move_speed: i64,
    sub_effect_move_time: i64,
    create_interval: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct BuffSkillRow {
    id: i32,
    name: String,
    scope: i32,
    effect_range_type: i32,
    effect_type: i32,
    sub_effect_damage: i32,
    start_time: i64,
    effect_range: i64,
    sub_effect_move_speed: i64,
    sub_effect_move_time: i64,
    buff: BuffRow,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the buff row's flags are independent fields"
)]
struct BuffRow {
    id: u32,
    name: String,
    divide: i32,
    duration: i64,
    additive: bool,
    disable_technology: bool,
    can_affect_construction: bool,
    can_affect_tower: bool,
    move_speed_rate: i64,
}

/// One battle skill a side releases, as `CommanderSkillReleaseState` carries
/// it to the tick its sub-effect lands.
#[derive(Debug, Clone)]
pub(crate) struct SkillRelease {
    pub(crate) team: u32,
    /// The skill's name, for a refusal the fight makes.
    pub(crate) name: String,
    /// Where it lands, in space units, a thousand to the metre.
    pub(crate) x: i64,
    pub(crate) z: i64,
    /// The tick its sub-effect lands on, the fight's first update being 1.
    pub(crate) lands_on: u64,
    /// What its sub-effect does where it lands.
    pub(crate) effect: SkillEffect,
}

/// What a sub-effect does where it lands: the controller its skill's kind
/// creates.
#[derive(Debug, Clone)]
pub(crate) enum SkillEffect {
    /// `CommanderSkillSubEffectController`: a buff on every unit in range.
    Buff {
        /// A unit's edge this near, `FPoint` raw metres, is reached.
        range_q32: i64,
        /// What it writes on every unit it reaches.
        buff: SkillBuff,
    },
    /// `SupportUnitEffectController`: a creator of summons.
    Summon(Box<Summon>),
}

/// What a support skill's `SupportUnitCreator` makes, read off its row.
#[derive(Debug, Clone)]
pub(crate) struct Summon {
    /// The unit it summons, `unitID`, as `IFightSetting.GetMechData` reads it.
    pub(crate) rules: UnitConfig,
    /// `maxCount`: how many it summons in all.
    pub(crate) count: u32,
    /// `createCountPerTime`: how many one creation makes.
    pub(crate) per_time: u32,
    /// `createInterval` in whole ticks: how long between two creations.
    pub(crate) interval_ticks: u32,
    /// How far from the landing point a summon may stand, `FPoint` raw
    /// metres: the skill's range when it summons two or more, and none
    /// otherwise.
    pub(crate) random_range_q32: i64,
    /// Whether a summon deals its life around it as it lands:
    /// `appearType` 2, an air drop.
    pub(crate) drop_damage: bool,
    /// How many updates the creator lives, the last of them included.
    pub(crate) updates: u64,
}

/// The buff a released skill writes: the Electromagnetic Impact's slow.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SkillBuff {
    pub(crate) id: u32,
    pub(crate) divide: i32,
    pub(crate) additive: bool,
    pub(crate) ticks: u32,
    /// `speedChangeRate`, an `FPoint` raw rate.
    pub(crate) move_speed_rate: i64,
    /// `disableTechnology`: it switches off the technologies of what it is
    /// written on.
    pub(crate) disable_technology: bool,
}

/// Every battle skill the fight releases, by its commander skill id.
#[derive(Debug, Clone)]
pub(crate) struct CommanderSkillEffects {
    buff_skills: Vec<BuffSkillRow>,
    support_skills: Vec<SupportSkillRow>,
}

impl CommanderSkillEffects {
    /// Reads the tracked table.
    ///
    /// # Errors
    ///
    /// Returns an error when the table is not the one this build reads.
    pub(crate) fn load() -> Result<Self> {
        let table: Table = serde_yaml::from_str(DEFAULT_EFFECTS).map_err(|error| {
            Error::new(format!("cannot read the commander skill table: {error}"))
        })?;
        if table.schema != "mechcore.commander_skill_effects" {
            return Err(Error::new(format!(
                "commander skill table declares schema {:?}",
                table.schema
            )));
        }
        Ok(Self {
            buff_skills: table.buff_skills,
            support_skills: table.support_skills,
        })
    }

    /// A released battle skill, as `CSRC_Common` releases it: its one
    /// sub-effect falling on the release's position, on its side's half.
    ///
    /// The table's rows are circles, `effectRangeType` 0, which
    /// `CommanderSkillData.PreProcess` makes one sub-effect reaching
    /// `effectRange` with no interval, so the row's own `subEffectCount`,
    /// `subEffectRange` and `subEffectIntervalTime` are never read.
    ///
    /// # Errors
    ///
    /// Returns an error naming the skill when the table does not hold it, or
    /// when its row asks for what no recording has measured.
    pub(crate) fn release(
        &self,
        team: u32,
        skill: &BattleSkill,
        units: &UnitConfigs,
    ) -> Result<SkillRelease> {
        let named = format!(
            "battle skill {} ({})",
            skill.type_name, skill.commander_skill_id
        );
        let id = skill.commander_skill_id;
        let (row_name, common, effect) =
            if let Some(row) = self.buff_skills.iter().find(|row| row.id == id) {
                let named = format!("{named}, {}", row.name);
                (
                    row.name.as_str(),
                    Common::of_buff(row),
                    buff_effect(&named, row)?,
                )
            } else if let Some(row) = self.support_skills.iter().find(|row| row.id == id) {
                let named = format!("{named}, {}", row.name);
                (
                    row.name.as_str(),
                    Common::of_support(row),
                    SkillEffect::Summon(Box::new(summon(&named, row, units)?)),
                )
            } else {
                return Err(Error::new(format!("{named} is not released by this build")));
            };
        let named = format!("{named}, {row_name}");
        if common.scope != 1 || (common.effect_type, common.effect_range_type) != (0, 0) {
            return Err(Error::new(format!(
                "{named} reaches scope {} with effect type {} over range type {}, which this \
                 build does not read",
                common.scope, common.effect_type, common.effect_range_type
            )));
        }
        let [position] = skill.positions.as_slice() else {
            return Err(Error::new(format!(
                "{named} is released at {} positions, not one",
                skill.positions.len()
            )));
        };
        let (local_x, local_z) = (i64::from(position.x), i64::from(position.y));
        let (x, z) = if team == 0 {
            (local_x, local_z)
        } else {
            (-local_x, -local_z)
        };
        Ok(SkillRelease {
            team,
            name: skill.type_name.clone(),
            x: x * SPACE,
            z: z * SPACE,
            lands_on: lands_on(common.start_time, common.move_time, common.move_speed)?,
            effect,
        })
    }
}

/// What every kind of skill row holds that places and times its release.
struct Common {
    scope: i32,
    effect_type: i32,
    effect_range_type: i32,
    start_time: i64,
    move_time: i64,
    move_speed: i64,
}

impl Common {
    const fn of_buff(row: &BuffSkillRow) -> Self {
        Self {
            scope: row.scope,
            effect_type: row.effect_type,
            effect_range_type: row.effect_range_type,
            start_time: row.start_time,
            move_time: row.sub_effect_move_time,
            move_speed: row.sub_effect_move_speed,
        }
    }

    const fn of_support(row: &SupportSkillRow) -> Self {
        Self {
            scope: row.scope,
            effect_type: row.effect_type,
            effect_range_type: row.effect_range_type,
            start_time: row.start_time,
            move_time: row.sub_effect_move_time,
            move_speed: row.sub_effect_move_speed,
        }
    }
}

/// A buff skill's sub-effect: its buff on every unit within its range, which
/// `CommanderSkillData.PreProcess` made the circle's one sub-effect's.
fn buff_effect(named: &str, row: &BuffSkillRow) -> Result<SkillEffect> {
    if row.sub_effect_damage != 0 {
        return Err(Error::new(format!(
            "{named} deals damage this build does not read"
        )));
    }
    let buff = &row.buff;
    if buff.can_affect_construction || buff.can_affect_tower {
        return Err(Error::new(format!(
            "{named}'s buff {} ({}) reaches a building, which is not measured",
            buff.id, buff.name
        )));
    }
    Ok(SkillEffect::Buff {
        range_q32: row.effect_range,
        buff: SkillBuff {
            id: buff.id,
            divide: buff.divide,
            additive: buff.additive,
            ticks: u32::try_from(ticks(buff.duration)?)
                .map_err(|_| Error::new(format!("{named}'s buff outlasts a fight")))?,
            move_speed_rate: buff.move_speed_rate,
            disable_technology: buff.disable_technology,
        },
    })
}

/// A support skill's creator, as `SupportUnitCreator` is built over its row.
///
/// `CSD_SupportUnit.PreProcess` gives the creator a life of the creations it
/// needs times their interval, after the skill's `startTime`, and
/// `CS_SupportUnit.CreateSubEffectController` hands it the skill's range to
/// scatter in only when it summons two or more.
fn summon(named: &str, row: &SupportSkillRow, units: &UnitConfigs) -> Result<Summon> {
    if row.max_batch != 0 {
        return Err(Error::new(format!(
            "{named} summons in {} batches, which this build does not read",
            row.max_batch
        )));
    }
    if row.max_count == 0 || row.create_count_per_time == 0 {
        return Err(Error::new(format!("{named} summons nothing")));
    }
    let config = units.by_type_id(row.unit_type_id).ok_or_else(|| {
        Error::new(format!(
            "{named} summons unit {}, which has no unit configuration",
            row.unit_type_id
        ))
    })?;
    let creations = i64::from(row.max_count.div_ceil(row.create_count_per_time));
    let life = row
        .start_time
        .saturating_add(creations.saturating_mul(row.create_interval));
    Ok(Summon {
        rules: config.clone(),
        count: row.max_count,
        per_time: row.create_count_per_time,
        interval_ticks: u32::try_from(ticks(row.create_interval)?)
            .map_err(|_| Error::new(format!("{named} creates too seldom")))?,
        random_range_q32: if row.max_count >= 2 {
            row.effect_range
        } else {
            0
        },
        drop_damage: row.appear_type == 2,
        // `SupportUnitCreator.IsFinished`: its updates times a tick have
        // reached that life.
        updates: u64::try_from(
            (i128::from(life) + i128::from(LOGIC_DELTA_RAW) - 1) / i128::from(LOGIC_DELTA_RAW),
        )
        .map_err(|_| Error::new(format!("{named} creates for no time")))?,
    })
}

/// The tick a released skill's first sub-effect lands on.
///
/// `CSRS_Prepare` counts ticks from the release's first update and hands over
/// once it has counted `startTime - subEffectMoveTime`, never before its first
/// tick. `CSRS_Perform` starts its count there and activates the sub-effect on
/// its own first update, after that tick's agents have moved, so the
/// sub-effect falls from the next. It falls from `speed x min(startTime,
/// moveTime)` above where it lands by `speed x LogicDeltaTime` a tick, and a
/// tick's step is a hair under a twentieth of a second's, so a fall of a whole
/// number of twentieths takes one step more.
fn lands_on(start_raw: i64, move_time_raw: i64, speed_raw: i64) -> Result<u64> {
    let prepare = (ticks(start_raw)? - ticks(move_time_raw)?).max(1);
    let falls = if speed_raw == 0 {
        1
    } else {
        let height = multiply(speed_raw, start_raw.min(move_time_raw));
        let step = multiply(speed_raw, LOGIC_DELTA_RAW);
        (height + step - 1) / step
    };
    u64::try_from(prepare + 1 + falls)
        .map_err(|_| Error::new("a battle skill lands before the fight begins"))
}

/// `FPoint` multiplication.
fn multiply(left: i64, right: i64) -> i64 {
    i64::try_from((i128::from(left) * i128::from(right)) >> 32).unwrap_or(i64::MAX)
}

/// A time in seconds as whole ticks, `(int)(time / LogicDeltaTime)` in `FPoint`
/// division.
fn ticks(seconds_raw: i64) -> Result<i64> {
    let quotient = (i128::from(seconds_raw) << 32) / i128::from(LOGIC_DELTA_RAW);
    i64::try_from(quotient >> 32).map_err(|_| {
        Error::new(format!(
            "a battle skill time of {seconds_raw} is not a tick count"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn seconds(tenths: i64) -> i64 {
        (tenths << 32) / 10
    }

    /// The three landings the game recorded: the Electromagnetic Impact on 57,
    /// a Missile Strike on 63 and an Orbital Javelin on 62. The speed cancels;
    /// what separates them is whether the fall is shorter than the wait.
    #[test]
    fn a_sub_effect_lands_as_the_recordings_have_it() {
        let emp = CommanderSkillEffects::load().unwrap().buff_skills[0].clone();
        let rhino = CommanderSkillEffects::load().unwrap().support_skills[1].clone();
        assert_eq!(rhino.id, 1_200_002);
        // A Rhino Assault lands, and its creator makes the Rhino, on 28.
        assert_eq!(
            lands_on(
                rhino.start_time,
                rhino.sub_effect_move_time,
                rhino.sub_effect_move_speed
            )
            .unwrap(),
            28
        );
        assert_eq!(emp.id, 200_001);
        assert_eq!(
            lands_on(
                emp.start_time,
                emp.sub_effect_move_time,
                emp.sub_effect_move_speed
            )
            .unwrap(),
            57
        );
        // Missile Strike: 3 s against a 4.5 s fall at 650 m/s.
        assert_eq!(lands_on(seconds(30), seconds(45), 650 << 32).unwrap(), 63);
        // Orbital Javelin: 3 s against a 2 s fall at 1000 m/s.
        assert_eq!(lands_on(seconds(30), seconds(20), 1000 << 32).unwrap(), 62);
    }
}
