//! What strengthening a tower does to it, and what losing one writes on its
//! side.
//!
//! `config/towers.yaml` is the build's own table, extracted by
//! `scripts/extract-towers.py`, and `docs/rules/towers.md` states the rule.
//! A tower's strengthen level adds life and chooses the buff its loss writes:
//! `buffDatas` 1 to 5, one buff that differs only in how long it lasts.
//!
//! The loss is `FightTeamController.OnTowerDestoryed`: when a tower falls,
//! `BuffSystem.TryAddSpecialBuffForTeam` hands its buff to `BuffSystem.AddBuff`
//! for every live object of the side, `FightTeam.activeActors`, and each
//! reaches `BuffManager.AddBuff`. A buff already running in the same
//! `buffDivide` is not added again: `Buff.Reset` lengthens it by the new row's
//! duration in additive mode, and restarts it otherwise. `BuffManager.Update`
//! runs last in `FightMech.Update`, after the skill and the motion, and a buff
//! ends on the update its elapsed ticks reach its duration.

use serde::Deserialize;

use crate::{
    Error, Result,
    data::{Channel, Correction, Entry, Index},
};

use super::{LOGIC_TICK_TIME_UNITS, Simulation, TIME_UNITS_PER_SECOND};

const TOWERS: &str = include_str!("../../../../config/towers.yaml");

/// The module that tags what a buff writes, so that its end takes it away.
pub(in crate::fight) const SOURCE: &str = "BuffSystem";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    schema: String,
    game_build: String,
    destroyed_buff: DestroyedBuff,
    levels: Vec<Row>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the buff row's flags are independent fields"
)]
struct DestroyedBuff {
    name: String,
    buff_divide: i32,
    additive: bool,
    max_additive_stack: i32,
    can_affect_construction: bool,
    /// `isClearSelfBuffWhenDisableTech`. Nothing this simulator places
    /// disables a unit's technologies, so nothing reads it.
    #[allow(dead_code, reason = "no mechanism here disables technologies")]
    clear_when_technologies_disabled: bool,
    move_speed_rate: i64,
    damage_rate: i64,
    amplify_damage_rate: i64,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    level: u8,
    life: i64,
    #[allow(
        dead_code,
        reason = "which row it is; the duration is what the fight reads"
    )]
    buff: i32,
    duration: u32,
}

/// The tower table, as the fight reads it.
#[derive(Debug, Clone)]
pub(in crate::fight) struct Towers {
    buff: DestroyedBuff,
    levels: Vec<Row>,
}

/// A buff running on a unit: `Buff.durationTime` against `maxDurationtime`,
/// both in ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) struct RunningBuff {
    divide: i32,
    additive: bool,
    elapsed: u32,
    duration: u32,
}

/// What one tower's fall writes: on whom, and for how many ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) struct TowerLoss {
    pub(in crate::fight) team: u32,
    pub(in crate::fight) ticks: u32,
}

impl Towers {
    pub(in crate::fight) fn load() -> Result<Self> {
        let table: Table = serde_yaml::from_str(TOWERS)
            .map_err(|error| Error::new(format!("cannot read the tower table: {error}")))?;
        if table.schema != "mechcore.towers" || table.game_build.trim().is_empty() {
            return Err(Error::new(
                "the tower table is not the one this build reads",
            ));
        }
        if !table
            .levels
            .iter()
            .enumerate()
            .all(|(index, level)| usize::from(level.level) == index)
        {
            return Err(Error::new(
                "the tower table's levels are not 0 onwards in order",
            ));
        }
        // `maxAdditiveStack` bounds how many times an additive buff is
        // lengthened. Zero is the tower's row, and no bound is read for it.
        if table.destroyed_buff.max_additive_stack != 0 {
            return Err(Error::new(format!(
                "{} bounds its additive stack, which is not read",
                table.destroyed_buff.name
            )));
        }
        Ok(Self {
            buff: table.destroyed_buff,
            levels: table.levels,
        })
    }

    fn level(&self, level: u8) -> Result<&Row> {
        self.levels
            .get(usize::from(level))
            .ok_or_else(|| Error::new(format!("a tower has no strengthen level {level}")))
    }

    /// The life a tower of this level has beyond the map's own: every level
    /// up to it adds its row's.
    pub(in crate::fight) fn life_added(&self, level: u8) -> Result<i64> {
        self.level(level)?;
        Ok(self.levels[..=usize::from(level)]
            .iter()
            .map(|row| row.life)
            .sum())
    }

    /// How many ticks the loss of a tower of this level writes its buff for.
    pub(in crate::fight) fn loss_ticks(&self, level: u8) -> Result<u32> {
        let seconds = u64::from(self.level(level)?.duration);
        u32::try_from(seconds * TIME_UNITS_PER_SECOND / LOGIC_TICK_TIME_UNITS)
            .map_err(|_| Error::new("a tower's loss lasts longer than a fight holds"))
    }

    /// Whether the buff reaches a construction whose row lets a tower's buff
    /// reach it.
    pub(in crate::fight) const fn reaches_constructions(&self) -> bool {
        self.buff.can_affect_construction
    }

    /// What the buff writes, in the buff channel.
    fn entries(&self) -> Vec<Entry> {
        let rate = |raw: i64| {
            if raw >= 0 {
                Correction::Rate {
                    add: raw,
                    reduce: 0,
                }
            } else {
                Correction::Rate {
                    add: 0,
                    reduce: -raw,
                }
            }
        };
        [
            (Index::MoveSpeed, self.buff.move_speed_rate),
            (Index::AttackDamage, self.buff.damage_rate),
            (Index::AmplifyDamage, self.buff.amplify_damage_rate),
        ]
        .into_iter()
        .filter(|(_, raw)| *raw != 0)
        .map(|(index, raw)| Entry {
            index,
            source: SOURCE,
            correction: rate(raw),
        })
        .collect()
    }
}

impl Simulation {
    /// Whether this target is one of the map's towers (`FightCrystal.IsTower`),
    /// which is an actor of its own rather than one block of a construction.
    pub(in crate::fight) fn is_tower(&self, target: super::FightActorRef) -> bool {
        matches!(target, super::FightActorRef::Building(id) if self.tower_losses.contains_key(&id))
    }

    /// `FightTeamController.OnTowerDestoryed`: the fallen building's buff, on
    /// every live object of its side.
    ///
    /// # Errors
    ///
    /// Returns an error when the side has a live construction the buff would
    /// reach, which no recording has shown, rather than leaving it out.
    pub(in crate::fight) fn lose_tower(&mut self, building_id: u64) -> Result<()> {
        let Some(loss) = self.tower_losses.get(&building_id).copied() else {
            return Ok(());
        };
        if self.towers.reaches_constructions() {
            let reached = self.buildings.iter().find(|building| {
                building.team_id == loss.team
                    && building.life.current > 0
                    && self
                        .tower_buffed_constructions
                        .contains(&building.building_id)
            });
            if let Some(building) = reached {
                return Err(Error::new(format!(
                    "team {} loses a tower while its construction {} stands, and \
                     what a tower's loss writes on a construction is not measured",
                    loss.team, building.building_id
                )));
            }
        }
        let entries = self.towers.entries();
        let divide = self.towers.buff.buff_divide;
        let additive = self.towers.buff.additive;
        let actor_ids = self
            .actors
            .iter()
            .filter_map(|(&id, actor)| {
                (actor.placement.team == loss.team && actor.alive()).then_some(id)
            })
            .collect::<Vec<_>>();
        for actor_id in actor_ids {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            if let Some(running) = actor
                .buffs
                .iter_mut()
                .find(|running| running.divide == divide)
            {
                // `Buff.Reset`: an additive buff lasts the new row's duration
                // longer; any other starts over.
                if running.additive {
                    running.duration = running.duration.saturating_add(loss.ticks);
                } else {
                    running.elapsed = 0;
                }
                continue;
            }
            actor.buffs.push(RunningBuff {
                divide,
                additive,
                elapsed: 0,
                duration: loss.ticks,
            });
            for entry in &entries {
                actor
                    .stats
                    .overlays
                    .channel(Channel::Buff)
                    .write(entry.clone());
            }
            actor.stats.refresh(&actor.rules)?;
        }
        Ok(())
    }

    /// `BuffManager.Update` on a unit that is no longer alive: its buffs go.
    ///
    /// Not on the tick it dies but on its next update. A Fang of the
    /// two-tower fight killed by a Steel Ball's beam lands its projectile the
    /// same tick for the debuffed 6; one that died two ticks before its
    /// projectile landed lands it for the full 63.
    pub(in crate::fight) fn drop_buffs_of_the_dead(&mut self, actor_id: u64) -> Result<()> {
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        if actor.buffs.is_empty() {
            return Ok(());
        }
        actor.buffs.clear();
        actor.stats.overlays.channel(Channel::Buff).withdraw(SOURCE);
        actor.stats.refresh(&actor.rules)
    }

    /// `BuffManager.Update`: every running buff one tick older, and those
    /// whose time is up taken away.
    pub(in crate::fight) fn update_buffs(&mut self, actor_id: u64) -> Result<()> {
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        if actor.buffs.is_empty() {
            return Ok(());
        }
        for running in &mut actor.buffs {
            running.elapsed = running.elapsed.saturating_add(1);
        }
        let before = actor.buffs.len();
        actor
            .buffs
            .retain(|running| running.elapsed < running.duration);
        if actor.buffs.len() != before {
            // One buff writes here today, so its end takes the channel's
            // entries with it.
            actor.stats.overlays.channel(Channel::Buff).withdraw(SOURCE);
            actor.stats.refresh(&actor.rules)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Towers;

    /// Rows 1 to 5 last 9, 7, 5, 3 and 1 seconds, and the tower-loss fights
    /// read 180, 140, 100, 60 and 20 ticks; a level's life adds up its row and
    /// every row below it on top of the map's 3400.
    #[test]
    fn a_level_chooses_the_loss_and_adds_the_life() {
        let towers = Towers::load().unwrap();
        let ticks = (0..=4)
            .map(|level| towers.loss_ticks(level).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(ticks, [180, 140, 100, 60, 20]);
        let life = (0..=4)
            .map(|level| 3400 + towers.life_added(level).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(life, [3_400, 23_400, 59_400, 131_400, 235_400]);
        assert!(towers.loss_ticks(5).is_err());
    }
}
