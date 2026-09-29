//! What strengthening a tower does to it, and what losing one writes on its
//! side.
//!
//! `config/towers.yaml`, read as [`TowersConfig`], holds the map's towers and
//! the build's strengthen and buff rows, and `docs/rules/towers.md` states the
//! rule.
//! A tower's strengthen level adds life and chooses the buff its loss writes:
//! `buffDatas` 1 to 5, one buff that differs only in how long it lasts.
//!
//! The loss is `FightTeamController.OnTowerDestoryed`: when a tower falls,
//! `BuffSystem.TryAddSpecialBuffForTeam` hands its buff to `BuffSystem.AddBuff`
//! for every live object of the side, `FightTeam.activeActors`, and each
//! reaches `BuffManager.AddBuff`. A buff already running in the same
//! `buffDivide` is not added again: `Buff.Reset` lengthens it by the new row's
//! duration in additive mode, and restarts it otherwise. `BuffManager.Update`
//! runs last in `FightMech.Update` and `FightConstruction.Update`, after the
//! skill and the motion, and a buff ends on the update its elapsed ticks reach
//! its duration. The side's objects are its units and then its constructions
//! whose row lets a tower's buff reach them.
//!
//! Each of these is an event: `buff_applied` for every object the loss
//! reaches, after the tower's `building_destroyed`, naming the running buff's
//! row and the ticks left on it; `buff_removed` when its time is up, and,
//! `cleared`, right after the `unit_died` of a unit that dies with it or right
//! before the `building_destroyed` of a construction that falls with it.

use crate::{
    Error, Result,
    data::{Channel, Correction, Entry, Index, Overlays},
    rules::{TowerLevel, TowersConfig},
};

use super::{LOGIC_TICK_TIME_UNITS, Simulation, TIME_UNITS_PER_SECOND, event};
use mechcore_mcfr::{BuffRemovedReason, Event, EventPayload, ObjectKind, ObjectRef};

/// What tags a tower's loss writes, so that its end takes it away.
pub(in crate::fight) const SOURCE: &str = "BuffSystem";

/// A buff running on a unit: `Buff.durationTime` against `maxDurationtime`,
/// both in ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) struct RunningBuff {
    /// The `buffDatas` row it was added with, which a later one it merges
    /// into does not change.
    buff_id: u32,
    divide: i32,
    additive: bool,
    elapsed: u32,
    duration: u32,
    /// What tags the entries it wrote, so that its end takes them away
    /// and leaves every other buff's.
    source: &'static str,
}

/// One `buffDatas` row as `BuffManager.AddBuff` adds it: which it is, how it
/// merges with one already running, how long it lasts, and what it writes.
#[derive(Debug, Clone)]
pub(in crate::fight) struct BuffRow {
    pub(in crate::fight) buff_id: u32,
    pub(in crate::fight) divide: i32,
    pub(in crate::fight) additive: bool,
    pub(in crate::fight) ticks: u32,
    pub(in crate::fight) source: &'static str,
    pub(in crate::fight) entries: Vec<Entry>,
}

/// What one tower's fall writes: on whom, and for how many ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) struct TowerLoss {
    pub(in crate::fight) team: u32,
    pub(in crate::fight) buff_id: u32,
    pub(in crate::fight) ticks: u32,
}

impl TowersConfig {
    fn level(&self, level: u8) -> Result<&TowerLevel> {
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

    /// The `buffDatas` row the loss of a tower of this level writes.
    pub(in crate::fight) fn loss_buff(&self, level: u8) -> Result<u32> {
        Ok(self.level(level)?.buff)
    }

    /// Whether the buff reaches a construction whose row lets a tower's buff
    /// reach it.
    pub(in crate::fight) const fn reaches_constructions(&self) -> bool {
        self.destroyed_buff.can_affect_construction
    }

    /// What the buff writes, in the buff channel.
    fn entries(&self) -> Vec<Entry> {
        [
            (Index::MoveSpeed, self.destroyed_buff.move_speed_rate),
            (Index::AttackDamage, self.destroyed_buff.damage_rate),
            (
                Index::AmplifyDamage,
                self.destroyed_buff.amplify_damage_rate,
            ),
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
    /// every live object of its side, `FightTeam.activeActors`: its units,
    /// then its constructions whose row lets a tower's buff reach them.
    pub(in crate::fight) fn lose_tower(&mut self, building_id: u64) -> Result<()> {
        let Some(loss) = self.tower_losses.get(&building_id).copied() else {
            return Ok(());
        };
        if let Some(interceptor) = self.standing_interceptor(loss.team) {
            return Err(Error::new(format!(
                "team {} loses a tower while its interceptor {interceptor} stands, and what a \
                 tower's loss writes on an interceptor is not measured",
                loss.team
            )));
        }
        let row = BuffRow {
            buff_id: loss.buff_id,
            divide: self.towers.destroyed_buff.buff_divide,
            additive: self.towers.destroyed_buff.additive,
            ticks: loss.ticks,
            source: SOURCE,
            entries: self.towers.entries(),
        };
        let mut applied = Vec::new();
        let actor_ids = self
            .actors
            .iter()
            .filter_map(|(&id, actor)| {
                (actor.placement.team == loss.team && actor.alive()).then_some(id)
            })
            .collect::<Vec<_>>();
        for actor_id in actor_ids {
            applied.push(self.write_buff(actor_id, loss.team, &row)?);
        }
        let reached = if self.towers.reaches_constructions() {
            self.buildings
                .iter()
                .filter(|building| {
                    building.team_id == loss.team
                        && building.life.current > 0
                        && self
                            .tower_buffed_constructions
                            .contains(&building.building_id)
                })
                .map(|building| building.building_id)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        for construction_id in reached {
            let buffed = self.building_buffs.entry(construction_id).or_default();
            let (running, added) = add_buff(&mut buffed.buffs, &row);
            applied.push(buff_applied(
                ObjectRef::new(ObjectKind::Building, construction_id),
                loss.team,
                &running,
            ));
            if added {
                for entry in &row.entries {
                    buffed.overlays.channel(Channel::Buff).write(entry.clone());
                }
                self.refresh_construction(construction_id)?;
            }
        }
        self.tower_buff_events.insert(building_id, applied);
        Ok(())
    }

    /// `BuffManager.AddBuff` on a unit: the row added, or merged into the one
    /// of its divide already running, and what it writes on the unit's buff
    /// channel. The `buff_applied` it answers names the running buff.
    pub(in crate::fight) fn write_buff(
        &mut self,
        actor_id: u64,
        team: u32,
        row: &BuffRow,
    ) -> Result<Event> {
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let (running, added) = add_buff(&mut actor.buffs, row);
        if added {
            for entry in &row.entries {
                actor
                    .stats
                    .overlays
                    .channel(Channel::Buff)
                    .write(entry.clone());
            }
            actor.stats.refresh(&actor.rules)?;
        }
        Ok(buff_applied(
            ObjectRef::new(ObjectKind::Unit, actor_id),
            team,
            &running,
        ))
    }

    /// A firing construction's damage, as its buffs leave it.
    fn refresh_construction(&mut self, building_id: u64) -> Result<()> {
        let Some(construction) = self.constructions.get_mut(&building_id) else {
            return Ok(());
        };
        let base = construction.attack.base_damage;
        construction.attack_damage = match self.building_buffs.get(&building_id) {
            Some(buffed) => buffed.overlays.resolve(Index::AttackDamage, base)?,
            None => base,
        };
        Ok(())
    }

    /// What one hit of `amount` takes off a construction, and what it counts
    /// as taken: the hit scaled by its buffs' rate on damage taken, as a
    /// unit's is.
    pub(in crate::fight) fn construction_damage_taken(
        &self,
        building_id: u64,
        amount: i64,
    ) -> Result<(i64, i64)> {
        match self.building_buffs.get(&building_id) {
            Some(buffed) => Ok((
                buffed.overlays.resolve(Index::AmplifyDamage, amount)?,
                buffed
                    .overlays
                    .resolve_raised(Index::AmplifyDamage, amount)?,
            )),
            None => Ok((amount, amount)),
        }
    }

    /// `BuffManager.Update`, last in `FightConstruction.Update`: a
    /// construction's buffs one tick older, and those whose time is up taken
    /// away.
    pub(in crate::fight) fn update_construction_buffs(
        &mut self,
        building_id: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let Some(buffed) = self.building_buffs.get_mut(&building_id) else {
            return Ok(());
        };
        let subject = ObjectRef::new(ObjectKind::Building, building_id);
        let ended = tick_buffs(&mut buffed.buffs, subject, events);
        if !ended.is_empty() {
            for source in ended {
                buffed.overlays.channel(Channel::Buff).withdraw(source);
            }
            if buffed.buffs.is_empty() {
                self.building_buffs.remove(&building_id);
            }
            self.refresh_construction(building_id)?;
        }
        Ok(())
    }

    /// The buffs a construction had when it fell, `cleared`, to follow its
    /// `building_destroyed`.
    pub(in crate::fight) fn construction_buffs_cleared(&mut self, building_id: u64) -> Vec<Event> {
        let subject = ObjectRef::new(ObjectKind::Building, building_id);
        self.building_buffs
            .remove(&building_id)
            .map(|buffed| {
                buffed
                    .buffs
                    .iter()
                    .map(|buff| buff_removed(subject, buff.buff_id, BuffRemovedReason::Cleared))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The buffs a unit that died this tick had, `cleared`, to follow its
    /// `unit_died`: the ones it still runs, or the ones its update already
    /// dropped.
    pub(in crate::fight) fn buffs_cleared_by_death(&mut self, actor_id: u64) -> Vec<Event> {
        let running = self
            .actors
            .get(&actor_id)
            .map(|actor| {
                actor
                    .buffs
                    .iter()
                    .map(|buff| buff.buff_id)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let dropped = self.dropped_buffs.remove(&actor_id).unwrap_or_default();
        let ids = if running.is_empty() { dropped } else { running };
        ids.into_iter()
            .map(|buff_id| {
                buff_removed(
                    ObjectRef::new(ObjectKind::Unit, actor_id),
                    buff_id,
                    BuffRemovedReason::Cleared,
                )
            })
            .collect()
    }

    /// `BuffManager.Update` on a unit that is no longer alive: its buffs go.
    ///
    /// Not on the tick it dies but on its next update. A Fang of the
    /// two-tower fight killed by a Steel Ball's beam lands its projectile the
    /// same tick for the debuffed 6; one that died two ticks before its
    /// projectile landed lands it for the full 63.
    /// Every buff a live unit still runs, taken off and written as cleared
    /// in unit order, as the fight is left.
    pub(in crate::fight) fn clear_buffs_as_the_fight_ends(
        &mut self,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        for (&actor_id, actor) in &mut self.actors {
            if !actor.alive() || actor.buffs.is_empty() {
                continue;
            }
            events.extend(actor.buffs.iter().map(|buff| {
                buff_removed(
                    ObjectRef::new(ObjectKind::Unit, actor_id),
                    buff.buff_id,
                    BuffRemovedReason::Cleared,
                )
            }));
            for buff in std::mem::take(&mut actor.buffs) {
                actor
                    .stats
                    .overlays
                    .channel(Channel::Buff)
                    .withdraw(buff.source);
            }
            actor.stats.refresh(&actor.rules)?;
        }
        Ok(())
    }

    pub(in crate::fight) fn drop_buffs_of_the_dead(&mut self, actor_id: u64) -> Result<()> {
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        if actor.buffs.is_empty() {
            return Ok(());
        }
        self.dropped_buffs.insert(
            actor_id,
            actor.buffs.iter().map(|buff| buff.buff_id).collect(),
        );
        for buff in std::mem::take(&mut actor.buffs) {
            actor
                .stats
                .overlays
                .channel(Channel::Buff)
                .withdraw(buff.source);
        }
        actor.stats.refresh(&actor.rules)
    }

    /// `BuffManager.Update`: every running buff one tick older, and those
    /// whose time is up taken away.
    pub(in crate::fight) fn update_buffs(
        &mut self,
        actor_id: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let subject = ObjectRef::new(ObjectKind::Unit, actor_id);
        let ended = tick_buffs(&mut actor.buffs, subject, events);
        if !ended.is_empty() {
            // A buff's end takes what it wrote, and every other buff's
            // entries stay.
            for source in ended {
                actor.stats.overlays.channel(Channel::Buff).withdraw(source);
            }
            actor.stats.refresh(&actor.rules)?;
        }
        Ok(())
    }
}

/// A buff's rate as the correction it writes: an increase adds, a decrease
/// impairs.
pub(in crate::fight) const fn rate(raw: i64) -> Correction {
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
}

/// A construction's running buffs and what they write: the `BuffManager`
/// a `FightConstruction` carries, which a unit's stats hold for it.
#[derive(Debug, Clone, Default)]
pub(in crate::fight) struct BuildingBuffs {
    buffs: Vec<RunningBuff>,
    overlays: Overlays,
}

/// `BuffManager.AddBuff`: a buff already running in the row's divide is
/// `Buff.Reset`, lengthened by the new row's duration when additive and
/// started over otherwise; any other is added. The running buff is returned,
/// with whether it is new.
fn add_buff(buffs: &mut Vec<RunningBuff>, row: &BuffRow) -> (RunningBuff, bool) {
    if let Some(running) = buffs
        .iter_mut()
        .find(|running| running.divide == row.divide)
    {
        if running.additive {
            running.duration = running.duration.saturating_add(row.ticks);
        } else {
            running.elapsed = 0;
        }
        return (*running, false);
    }
    let running = RunningBuff {
        buff_id: row.buff_id,
        divide: row.divide,
        additive: row.additive,
        elapsed: 0,
        duration: row.ticks,
        source: row.source,
    };
    buffs.push(running);
    (running, true)
}

/// Every running buff one tick older, and those whose time is up removed
/// with an `expired` event; what the ended ones wrote their entries under.
fn tick_buffs(
    buffs: &mut Vec<RunningBuff>,
    subject: ObjectRef,
    events: &mut Vec<Event>,
) -> Vec<&'static str> {
    if buffs.is_empty() {
        return Vec::new();
    }
    for running in buffs.iter_mut() {
        running.elapsed = running.elapsed.saturating_add(1);
    }
    let before = buffs.len();
    let mut ended = Vec::new();
    for running in buffs.iter() {
        if running.elapsed >= running.duration {
            events.push(buff_removed(
                subject,
                running.buff_id,
                BuffRemovedReason::Expired,
            ));
            ended.push(running.source);
        }
    }
    buffs.retain(|running| running.elapsed < running.duration);
    debug_assert_eq!(buffs.len() + ended.len(), before);
    ended
}

fn buff_applied(subject: ObjectRef, team: u32, running: &RunningBuff) -> Event {
    event(
        None,
        None,
        Some(team),
        Some(subject),
        EventPayload::BuffApplied {
            buff_id: running.buff_id,
            duration: i32::try_from(running.duration.saturating_sub(running.elapsed))
                .unwrap_or(i32::MAX),
        },
    )
}

fn buff_removed(subject: ObjectRef, buff_id: u32, reason: BuffRemovedReason) -> Event {
    event(
        None,
        None,
        None,
        Some(subject),
        EventPayload::BuffRemoved { buff_id, reason },
    )
}

#[cfg(test)]
mod tests {

    /// Rows 1 to 5 last 9, 7, 5, 3 and 1 seconds, and the tower-loss fights
    /// read 180, 140, 100, 60 and 20 ticks; a level's life adds up its row and
    /// every row below it on top of the map's 3400.
    #[test]
    fn a_level_chooses_the_loss_and_adds_the_life() {
        let towers = crate::rules::SimulationConfig::load().unwrap().towers;
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
