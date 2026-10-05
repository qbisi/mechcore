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

use std::collections::{BTreeMap, BTreeSet};

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
    /// `Buff.source`: the actor that first added it, which `Buff.Reset`
    /// keeps unless the buff summons.
    source_actor: Option<ObjectRef>,
    /// `IsDisableTechnology`: while it runs, `BuffManager` holds the unit's
    /// `DisableTechnology` count above zero.
    disables_technology: bool,
    /// `IsInvincible`: while it runs, `BuffManager` holds the unit's
    /// `Invincible` count above zero.
    invincible: bool,
    /// `IBEC_ChangeLIfe`, the controller `Buff.Init` gives a buff of a
    /// nonzero `lifeChangeRate`, and its step.
    life_change: Option<LifeChangeStep>,
}

/// A buff's `lifeChangeRate` as it runs: `Buff.stepTime` counting to
/// `stepTimeConfig`, and the side whose `sourceTeamController` the hit is
/// dealt under, which `Buff.Reset` does not change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LifeChangeStep {
    life_change: LifeChange,
    team: u32,
    elapsed: u32,
}

/// A buff row's `lifeChangeRate` and `stepTime`, the latter in ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) struct LifeChange {
    pub(in crate::fight) rate: i64,
    pub(in crate::fight) step_ticks: u32,
}

/// One `buffDatas` row as `BuffManager.AddBuff` adds it: which it is, how it
/// merges with one already running, how long it lasts, and what it writes.
#[derive(Debug, Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the buff row's flags are independent fields"
)]
pub(in crate::fight) struct BuffRow {
    pub(in crate::fight) buff_id: u32,
    pub(in crate::fight) divide: i32,
    pub(in crate::fight) additive: bool,
    pub(in crate::fight) ticks: u32,
    pub(in crate::fight) source: &'static str,
    pub(in crate::fight) entries: Vec<Entry>,
    pub(in crate::fight) disables_technology: bool,
    /// `IsDebuff`: a unit a buff makes invincible does not take it.
    pub(in crate::fight) debuff: bool,
    /// `IsInvincible`.
    pub(in crate::fight) invincible: bool,
    /// `lifeChangeRate` and `stepTime`, when the rate is not zero.
    pub(in crate::fight) life_change: Option<LifeChange>,
    /// `currentLifeDisposableChangeRate`, an `FPoint` raw rate.
    pub(in crate::fight) current_life_rate: i64,
}

/// The towers of both sides: what their table says, what each one's loss
/// writes, and the towers a hit emptied this tick.
pub(in crate::fight) struct TowerSystem {
    /// The tower table: what a strengthen level adds, what a loss writes.
    pub(in crate::fight) config: TowersConfig,
    /// What each tower's fall writes, by building.
    pub(in crate::fight) losses: BTreeMap<u64, TowerLoss>,
    /// The constructions a tower's loss would reach, by building.
    pub(in crate::fight) buffed_constructions: BTreeSet<u64>,
    /// The towers a hit emptied this tick, in the order they fell: the towers
    /// among `DeadEffectSystem.deadActors`, whose `OnDead` waits for that
    /// module's update.
    pub(in crate::fight) fallen: Vec<u64>,
}

/// `BuffManager`'s state the fight keeps outside the units: the buffs on
/// constructions, and the buff events a tick holds back.
#[derive(Default)]
pub(in crate::fight) struct BuffState {
    /// The buffs on constructions, by building.
    pub(in crate::fight) building_buffs: BTreeMap<u64, BuildingBuffs>,
    /// The buffs `BuffManager.Update` dropped from a unit dead this tick, to
    /// name in the `cleared` that follows its `unit_died`.
    pub(in crate::fight) dropped: BTreeMap<u64, Vec<u32>>,
    /// The `buff_applied` events a tower's loss wrote this tick, by tower, to
    /// follow its `building_destroyed`.
    pub(in crate::fight) tower_events: BTreeMap<u64, Vec<Event>>,
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

impl super::Actor {
    /// The row of a buff running on this unit beside which `row` is not
    /// measured, if one runs: one whose entries share `row`'s tag, which
    /// would take them away together, or that corrects a number `row` does
    /// other than the speed. Two buffs run side by side in `BuffManager`, and
    /// their speeds compose in `MoveSpeedProperty.Refresh`; how their other
    /// rates do, a tower's kept apart in `towerBuffDatas`, is not recorded.
    pub(in crate::fight) fn buff_not_beside(&self, row: &BuffRow) -> Option<u32> {
        let buffs = &self.stats.overlays;
        self.buffs
            .iter()
            .filter(|running| running.buff_id != row.buff_id)
            .find(|running| {
                running.source == row.source
                    || row.entries.iter().any(|entry| {
                        entry.index != Index::MoveSpeed
                            && buffs.buff_writes(running.source, entry.index)
                    })
            })
            .map(|running| running.buff_id)
    }

    /// `BuffManager.IsInvincible`: whether a running buff makes the unit
    /// invincible.
    pub(in crate::fight) fn invincible(&self) -> bool {
        self.buffs.iter().any(|running| running.invincible)
    }

    /// `FightMech.IsTechnologyDisabled`: whether a running buff holds the
    /// unit's technologies off.
    pub(in crate::fight) fn technology_disabled(&self) -> bool {
        self.buffs.iter().any(|running| running.disables_technology)
    }
}

impl Simulation {
    /// Whether this target is one of the map's towers (`FightCrystal.IsTower`),
    /// which is an actor of its own rather than one block of a construction.
    pub(in crate::fight) fn is_tower(&self, target: super::FightActorRef) -> bool {
        matches!(target, super::FightActorRef::Building(id) if self.towers.losses.contains_key(&id))
    }

    /// `FightTeamController.OnTowerDestoryed`: the fallen building's buff, on
    /// every live object of its side, `FightTeam.activeActors`: its units,
    /// then its constructions whose row lets a tower's buff reach them.
    pub(in crate::fight) fn lose_tower(&mut self, building_id: u64) -> Result<()> {
        let Some(loss) = self.towers.losses.get(&building_id).copied() else {
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
            divide: self.towers.config.destroyed_buff.buff_divide,
            additive: self.towers.config.destroyed_buff.additive,
            ticks: loss.ticks,
            source: SOURCE,
            entries: self.towers.config.entries(),
            disables_technology: false,
            debuff: self.towers.config.destroyed_buff.debuff,
            invincible: false,
            life_change: None,
            current_life_rate: 0,
        };
        let mut applied = Vec::new();
        // `activeActors` holds a side's units in the order they joined it: a
        // unit a beam turned onto it comes after every unit already there.
        let actor_ids = self
            .units_in_update_order()
            .into_iter()
            .filter(|id| {
                let actor = &self.actors[id];
                actor.placement.team == loss.team && actor.alive()
            })
            .collect::<Vec<_>>();
        for actor_id in actor_ids {
            if self.buff_reaches(actor_id, &row) {
                self.write_buff(actor_id, None, loss.team, &row, &mut applied)?;
            }
        }
        let reached = if self.towers.config.reaches_constructions() {
            self.buildings
                .iter()
                .filter(|building| {
                    building.team_id == loss.team
                        && building.life.current > 0
                        && self
                            .towers
                            .buffed_constructions
                            .contains(&building.building_id)
                })
                .map(|building| building.building_id)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        for construction_id in reached {
            let buffed = self
                .buffs
                .building_buffs
                .entry(construction_id)
                .or_default();
            let (running, added) = add_buff(&mut buffed.buffs, &row, None, loss.team);
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
        self.buffs.tower_events.insert(building_id, applied);
        Ok(())
    }

    /// `BuffManager.AddBuff` on a unit: the row added, or merged into the one
    /// of its divide already running, and what it writes on the unit's buff
    /// channel. The `buff_applied` it answers names the running buff.
    /// `BuffSystem.DoAddBuff` before it hands a buff to the unit's
    /// `BuffManager.AddBuff`, and the head of that: whether the buff reaches
    /// the unit at all. A buff the unit ignores (`FightMech.IsIgnoredBuff`)
    /// does not, nor does a debuff a running buff makes it invincible to, and
    /// nothing is recorded.
    pub(in crate::fight) fn buff_reaches(&self, actor_id: u64, row: &BuffRow) -> bool {
        let actor = &self.actors[&actor_id];
        if actor.placement.ignored_buffs.contains(&row.buff_id) {
            return false;
        }
        !row.debuff || !actor.invincible()
    }

    /// `BuffManager.AddBuff` of a buff that reaches the unit, recorded with
    /// the object that wrote it when one did, after what the buff did as it
    /// was written: a new buff's `Enter` and a running one's
    /// `ReEnableDisposableEffect` both perform its once-only effects.
    pub(in crate::fight) fn write_buff(
        &mut self,
        actor_id: u64,
        source: Option<ObjectRef>,
        team: u32,
        row: &BuffRow,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let (running, added) = add_buff(&mut actor.buffs, row, source, team);
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
        if row.current_life_rate != 0 {
            self.change_current_life(actor_id, team, row.current_life_rate, events)?;
        }
        events.push(buff_applied(
            ObjectRef::new(ObjectKind::Unit, actor_id),
            team,
            &running,
        ));
        Ok(())
    }

    /// `IBEC_ChangeCurrentLife.Perform`: the unit's life now times the rate,
    /// its whole part. A loss is a hit of no object under the buff's side,
    /// which the rate on damage taken raises (`PerformHitTargetEffect` with
    /// `isAmplifyDamageAffected`): a Rhino of 19297 loses 3860 to
    /// Disintegration's −0.2.
    fn change_current_life(
        &mut self,
        actor_id: u64,
        team: u32,
        rate: i64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let change = self.actors[&actor_id].life.saturating_mul(rate) >> 32;
        match change.cmp(&0) {
            std::cmp::Ordering::Less => {
                let target = super::FightActorRef::Unit(actor_id);
                self.hit_with_no_object(target, team, (-change, true), events)
            }
            std::cmp::Ordering::Equal => Ok(()),
            std::cmp::Ordering::Greater => Err(Error::new(format!(
                "a buff heals unit {actor_id}, and a buff's healing is not measured"
            ))),
        }
    }

    /// A firing construction's damage, as its buffs leave it.
    fn refresh_construction(&mut self, building_id: u64) -> Result<()> {
        let Some(construction) = self.constructions.get_mut(&building_id) else {
            return Ok(());
        };
        let base = construction.attack.base_damage;
        construction.attack_damage = match self.buffs.building_buffs.get(&building_id) {
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
        match self.buffs.building_buffs.get(&building_id) {
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
        let Some(buffed) = self.buffs.building_buffs.get_mut(&building_id) else {
            return Ok(());
        };
        let subject = ObjectRef::new(ObjectKind::Building, building_id);
        let ended = tick_buffs(&mut buffed.buffs, 0, subject, events);
        if !ended.is_empty() {
            for source in ended {
                buffed.overlays.channel(Channel::Buff).withdraw(source);
            }
            if buffed.buffs.is_empty() {
                self.buffs.building_buffs.remove(&building_id);
            }
            self.refresh_construction(building_id)?;
        }
        Ok(())
    }

    /// The buffs a construction had when it fell, `cleared`, to follow its
    /// `building_destroyed`: last first, as `BuffManager.Clear` removes them.
    pub(in crate::fight) fn construction_buffs_cleared(&mut self, building_id: u64) -> Vec<Event> {
        let subject = ObjectRef::new(ObjectKind::Building, building_id);
        self.buffs
            .building_buffs
            .remove(&building_id)
            .map(|buffed| {
                buffed
                    .buffs
                    .iter()
                    .rev()
                    .map(|buff| buff_removed(subject, buff.buff_id, BuffRemovedReason::Cleared))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The buffs a unit that died this tick had, `cleared`, to follow its
    /// `unit_died`: the ones it still runs, or the ones its update already
    /// dropped, last first, as `BuffManager.Clear` removes them.
    pub(in crate::fight) fn buffs_cleared_by_death(&mut self, actor_id: u64) -> Vec<Event> {
        let running = self
            .actors
            .get(&actor_id)
            .map(|actor| {
                actor
                    .buffs
                    .iter()
                    .rev()
                    .map(|buff| buff.buff_id)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let dropped = self.buffs.dropped.remove(&actor_id).unwrap_or_default();
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

    /// Every buff a live unit still runs, taken off and written as cleared
    /// in unit order, as the fight is left, and then every buff a standing
    /// construction runs, in building order: each actor's `BuffManager.Clear`
    /// as the fight's objects are let go, which removes an actor's buffs last
    /// first.
    pub(in crate::fight) fn clear_buffs_as_the_fight_ends(
        &mut self,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        for (&actor_id, actor) in &mut self.actors {
            if !actor.alive() || actor.buffs.is_empty() {
                continue;
            }
            events.extend(actor.buffs.iter().rev().map(|buff| {
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
        for (building_id, buffed) in std::mem::take(&mut self.buffs.building_buffs) {
            let subject = ObjectRef::new(ObjectKind::Building, building_id);
            events.extend(
                buffed
                    .buffs
                    .iter()
                    .rev()
                    .map(|buff| buff_removed(subject, buff.buff_id, BuffRemovedReason::Cleared)),
            );
            self.refresh_construction(building_id)?;
        }
        Ok(())
    }

    /// `BuffManager.Update` on a unit that is no longer alive: its buffs go.
    ///
    /// Not on the tick it dies but on its first update of a later tick, even
    /// one it reaches dead on the tick it died. A Fang of the two-tower fight
    /// killed by a Steel Ball's beam lands its projectile the same tick for
    /// the debuffed 6; one that died two ticks before its projectile landed
    /// lands it for the full 63; and a Vulcan of 67158166 r4 killed before its
    /// own update lands its shell that tick for the debuffed 7, not 74.
    pub(in crate::fight) fn drop_buffs_of_the_dead(&mut self, actor_id: u64) -> Result<()> {
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        if actor.buffs.is_empty() {
            return Ok(());
        }
        self.buffs.dropped.insert(
            actor_id,
            actor.buffs.iter().rev().map(|buff| buff.buff_id).collect(),
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

    /// `Buff.Update`'s step of every buff on a unit, last first as
    /// `BuffManager.Update` runs them: `stepTime` a tick on, and at
    /// `stepTimeConfig` back to zero and each of the buff's controllers
    /// updated, of which only `IBEC_ChangeLIfe` acts here. `Buff.Reset` leaves
    /// the step where it was. A step that kills the unit ends the update, and
    /// the buffs before it in the list do not run; the answer is the first
    /// that did.
    fn step_buffs(&mut self, actor_id: u64, events: &mut Vec<Event>) -> Result<usize> {
        let count = self.actors[&actor_id].buffs.len();
        for index in (0..count).rev() {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            let max_life = actor.stats.max_life();
            let Some(step) = actor.buffs[index].life_change.as_mut() else {
                continue;
            };
            step.elapsed += 1;
            if step.elapsed < step.life_change.step_ticks {
                continue;
            }
            step.elapsed = 0;
            // `IBEC_ChangeLIfe.Update`: the unit's maximum life times the
            // rate, its whole part. A loss is a hit of no object under the
            // buff's side that the rate on damage taken does not affect.
            let change = max_life.saturating_mul(step.life_change.rate) >> 32;
            let team = step.team;
            match change.cmp(&0) {
                std::cmp::Ordering::Less => {
                    let target = super::FightActorRef::Unit(actor_id);
                    self.hit_with_no_object(target, team, (-change, false), events)?;
                }
                std::cmp::Ordering::Equal => {}
                std::cmp::Ordering::Greater => {
                    return Err(Error::new(format!(
                        "a buff heals unit {actor_id}, and a buff's healing is not measured"
                    )));
                }
            }
            if !self.actors[&actor_id].alive() {
                return Ok(index);
            }
        }
        Ok(0)
    }

    /// `BuffManager.Update`: every running buff one tick older, and those
    /// whose time is up taken away.
    pub(in crate::fight) fn update_buffs(
        &mut self,
        actor_id: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let from = self.step_buffs(actor_id, events)?;
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let subject = ObjectRef::new(ObjectKind::Unit, actor_id);
        let ended = tick_buffs(&mut actor.buffs, from, subject, events);
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

/// `BuffManager.AddBuff`: a buff already running that `IsSameBuff` matches,
/// the same row or one of the same nonzero divide, is `Buff.Reset`,
/// lengthened by the new row's duration when additive and started over
/// otherwise; any other is added. The running buff is returned, with whether
/// it is new.
fn add_buff(
    buffs: &mut Vec<RunningBuff>,
    row: &BuffRow,
    source_actor: Option<ObjectRef>,
    team: u32,
) -> (RunningBuff, bool) {
    if let Some(running) = buffs.iter_mut().find(|running| {
        running.buff_id == row.buff_id || (row.divide != 0 && running.divide == row.divide)
    }) {
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
        source_actor,
        disables_technology: row.disables_technology,
        invincible: row.invincible,
        life_change: row.life_change.map(|life_change| LifeChangeStep {
            life_change,
            team,
            elapsed: 0,
        }),
    };
    buffs.push(running);
    (running, true)
}

/// Every running buff one tick older, and those whose time is up removed
/// with an `expired` event; what the ended ones wrote their entries under.
///
/// Only the buffs from `from` on run: `BuffManager.Update` runs them last
/// first and stops at one whose step killed the unit.
fn tick_buffs(
    buffs: &mut Vec<RunningBuff>,
    from: usize,
    subject: ObjectRef,
    events: &mut Vec<Event>,
) -> Vec<&'static str> {
    if buffs.is_empty() {
        return Vec::new();
    }
    for running in &mut buffs[from..] {
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
        running.source_actor,
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
