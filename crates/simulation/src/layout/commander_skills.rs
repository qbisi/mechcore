//! What a layout's battle skills do once released.
//!
//! [`config/commander_skill_effects.yaml`](../../../../config/commander_skill_effects.yaml),
//! which `scripts/extract/extract-commander-skill-effects.py` writes, holds the
//! skills the fight releases; `docs/rules/battle_skill.md` states what they
//! do. Every other battle skill is refused by name.

use mechcore_document::BattleSkill;
use serde::Deserialize;

use crate::{Error, Result};

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
    /// A unit's edge this near, `FPoint` raw metres, is reached.
    pub(crate) range_q32: i64,
    /// What it writes on every unit it reaches.
    pub(crate) buff: SkillBuff,
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
    pub(crate) fn release(&self, team: u32, skill: &BattleSkill) -> Result<SkillRelease> {
        let named = format!(
            "battle skill {} ({})",
            skill.type_name, skill.commander_skill_id
        );
        let row = self
            .buff_skills
            .iter()
            .find(|row| row.id == skill.commander_skill_id)
            .ok_or_else(|| Error::new(format!("{named} is not released by this build")))?;
        let named = format!("{named}, {}", row.name);
        if row.scope != 1 || (row.effect_type, row.effect_range_type) != (0, 0) {
            return Err(Error::new(format!(
                "{named} reaches scope {} with effect type {} over range type {}, which this \
                 build does not read",
                row.scope, row.effect_type, row.effect_range_type
            )));
        }
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
            lands_on: lands_on(
                row.start_time,
                row.sub_effect_move_time,
                row.sub_effect_move_speed,
            )?,
            // `CommanderSkillData.PreProcess`: a circle is one sub-effect,
            // whose range is the skill's, whatever the row's own say.
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
