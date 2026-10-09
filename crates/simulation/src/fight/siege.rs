//! `SiegeModeEffectSystem`: a unit that a technology digs in as the fight
//! starts, until no enemy has stood in its main skill's range for a while.
//!
//! `SiegeModeEffectProvider.DoActive` hands the system each unit whose
//! technology is an `ISiegeModeEffectDataSource` (`AddSiegeModeOwner`). As
//! the fight starts every unit it holds is dug in (`OnEnterFight`,
//! `AddEffect`): its source's rates and values are written on its main skill
//! and its rate on its life, and its motion changes to `MotionStopState`,
//! which locks its agent where it stands (`RVOControllerFixed.Lock`) and turns
//! it to what it fires at without ever moving it. A unit handed over during
//! the fight is dug in at once.
//!
//! `SiegeModeEffectSystem.Update` counts, for each unit dug in, the time no
//! enemy unit has stood within its main skill's range of it
//! (`RangeTargetCalculator.CalculateRangeTargets`, no building, the unit's
//! own radius, only the fully visible, and no construction), and sets it back
//! to none on an update that finds one, as each blow of its main skill does
//! (`RegisterMainSkillPerformAttack`, `ResetTimer`). Once the time reaches
//! the source's duration the unit leaves its trench (`RemoveEffect`): what was
//! written is taken away, and its motion idles once the animation's delay is
//! over (`GRTimerManager`), the unit standing locked until then. It does not
//! dig in again in that fight. A disabling buff makes it leave on the next
//! update (`EndSiegeMode`), a unit that dies leaves as it dies
//! (`RemoveSiegeModeOwner`), and the fight's end takes every trench away
//! (`OnExitFight`). `docs/rules/technology_effects.md` states the rule.

use super::*;
use crate::data::{Channel, Entry};
use crate::modifier::SiegeMode;

/// What a trench writes its entries under, which is how it takes them away
/// again.
const SOURCE: &str = "SiegeModeEffectSystem";

/// `SiegeModeEffectSystem`'s units.
#[derive(Debug, Clone, Default)]
pub(in crate::fight) struct SiegeModeSystem {
    /// `siegeModeOwnerDatas` and `siegeModeOwnerDurations`: each unit's source
    /// and the time no enemy has stood in its range, Q32.32 seconds.
    owners: BTreeMap<u64, (SiegeMode, i64)>,
    /// `activeMeches`: the units dug in, in the order they were.
    active: Vec<u64>,
    /// The `GRTimerManager` timers of the units that left their trench: each
    /// one's motion idles on the step it names.
    idling: Vec<(u64, u64)>,
}

impl Simulation {
    /// `SiegeModeEffectSystem.AddSiegeModeOwner`, for every unit the fight
    /// starts with whose technologies hand it a source, then `OnEnterFight`:
    /// each held unit dug in, its time at zero. A unit travelling in is
    /// handed over as it arrives ([`Self::add_siege_unit`]).
    pub(in crate::fight) fn enter_siege_fight(&mut self) -> Result<()> {
        let held = self
            .actors
            .iter()
            .filter(|(_, actor)| !actor.travelling)
            .filter_map(|(&id, actor)| {
                actor
                    .placement
                    .effects
                    .single
                    .siege_mode
                    .clone()
                    .map(|source| (id, source))
            })
            .collect::<Vec<_>>();
        for (id, source) in held {
            self.siege.owners.entry(id).or_insert((source, 0));
        }
        self.siege.active = self.siege.owners.keys().copied().collect();
        for unit in self.siege.active.clone() {
            if let Some((_, time)) = self.siege.owners.get_mut(&unit) {
                *time = 0;
            }
            self.dig_in(unit)?;
        }
        Ok(())
    }

    /// `SiegeModeEffectSystem.AddSiegeModeOwner` during the fight: a unit it
    /// does not hold yet is dug in at once.
    pub(in crate::fight) fn add_siege_unit(&mut self, unit: u64) -> Result<()> {
        let Some(source) = self.actors[&unit]
            .placement
            .effects
            .single
            .siege_mode
            .clone()
        else {
            return Ok(());
        };
        if self.siege.owners.contains_key(&unit) {
            return Ok(());
        }
        self.siege.owners.insert(unit, (source, 0));
        self.siege.active.push(unit);
        self.dig_in(unit)
    }

    /// `SiegeModeEffectSystem.AddEffect`: the source's entries written, the
    /// life refreshed to the new maximum (`FightMech.AddData`), and the
    /// motion stopped (`MotionController.ChangeToStopState`).
    fn dig_in(&mut self, unit: u64) -> Result<()> {
        let source = self.siege.owners[&unit].0.clone();
        let actor = self
            .actors
            .get_mut(&unit)
            .expect("actor identity is stable");
        for &(channel, index, correction) in &source.written {
            actor.stats.overlays.channel(channel).write(Entry {
                index,
                source: SOURCE,
                correction,
            });
        }
        actor.refresh_life_data()?;
        actor.motion.state = MotionState::Stopped;
        Ok(())
    }

    /// `SiegeModeEffectSystem.RemoveEffect`: what the trench wrote taken
    /// away and the life refreshed, and, for a unit alive, its motion idled
    /// once the animation's delay is over while the fight goes on, and at
    /// once otherwise.
    fn leave_trench(&mut self, unit: u64, fighting: bool) -> Result<()> {
        let delay_q32 = self.siege.owners[&unit].0.animation_delay_q32;
        let actor = self
            .actors
            .get_mut(&unit)
            .expect("actor identity is stable");
        for channel in [Channel::Unit, Channel::Skill] {
            actor.stats.overlays.channel(channel).withdraw(SOURCE);
        }
        actor.refresh_life_data()?;
        if !actor.alive() {
            return Ok(());
        }
        if fighting && delay_q32 > 0 {
            let due = self.step_now + math::seconds_q32_to_steps(delay_q32);
            self.siege.idling.push((unit, due));
        } else {
            self.enter_motion_idle(unit);
        }
        Ok(())
    }

    /// The `GRTimerManager` timers due on this step: each unit that left its
    /// trench idles (`MotionController.ChangeToIdleState`), if it still
    /// lives.
    pub(in crate::fight) fn idle_after_trenches(&mut self, step: u64) {
        let (due, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.siege.idling)
            .into_iter()
            .partition(|&(_, at)| at <= step);
        self.siege.idling = waiting;
        for (unit, _) in due {
            if self.actors.get(&unit).is_some_and(Actor::alive) {
                self.enter_motion_idle(unit);
            }
        }
    }

    /// `SiegeModeEffectSystem.Update`, last unit first: a unit whose time
    /// has reached its duration (`FPoint.op_GreaterThanOrEqual`) leaves its
    /// trench; any other's time is set back to none when an enemy unit
    /// stands in its main skill's range, and counts the tick otherwise.
    pub(in crate::fight) fn step_siege(&mut self) -> Result<()> {
        for index in (0..self.siege.active.len()).rev() {
            let unit = self.siege.active[index];
            let Some((source, time)) = self.siege.owners.get(&unit) else {
                self.siege.active.remove(index);
                continue;
            };
            if rvo::fpoint_greater_or_equal(*time, source.duration_q32) {
                self.leave_trench(unit, true)?;
                self.siege.active.remove(index);
                continue;
            }
            let attacker = self
                .skill_attacker(SkillRef::main(FightActorRef::Unit(unit)))
                .expect("actor identity is stable");
            let enemies = self.range_targets(
                attacker.team,
                (attacker.x_q32, attacker.z_q32),
                space_to_q32(attacker.radius),
                space_to_q32(attacker.attack_range),
                attacker.targets,
            );
            let (_, time) = self
                .siege
                .owners
                .get_mut(&unit)
                .expect("the unit was held above");
            *time = if enemies.is_empty() {
                time.saturating_add(NATIVE_LOGIC_DELTA_Q32)
            } else {
                0
            };
        }
        Ok(())
    }

    /// `SiegeModeEffectSystem.ResetTimer`, which a blow of the unit's main
    /// skill raises (`FightMech.PerformMainSkillAttack`).
    pub(in crate::fight) fn reset_siege_time(&mut self, unit: u64) {
        if let Some((_, time)) = self.siege.owners.get_mut(&unit) {
            *time = 0;
        }
    }

    /// `SiegeModeEffectSystem.EndSiegeMode`, as a disabling buff switches
    /// the unit's technologies off: its time is its duration, so the next
    /// update takes it out of its trench.
    pub(in crate::fight) fn end_siege_mode(&mut self, unit: u64) {
        if let Some((source, time)) = self.siege.owners.get_mut(&unit) {
            *time = source.duration_q32;
        }
    }

    /// `SiegeModeEffectSystem.RemoveSiegeModeOwner`, as the unit dies
    /// (`SiegeModeEffectProvider.DoDeactive`): a unit dug in leaves its
    /// trench, and the system lets it go.
    pub(in crate::fight) fn remove_siege_unit(&mut self, unit: u64) -> Result<()> {
        if !self.siege.owners.contains_key(&unit) {
            return Ok(());
        }
        if let Some(index) = self.siege.active.iter().position(|&held| held == unit) {
            self.leave_trench(unit, true)?;
            self.siege.active.remove(index);
        }
        self.siege.owners.remove(&unit);
        Ok(())
    }

    /// `SiegeModeEffectSystem.OnExitFight`: every unit still dug in leaves
    /// its trench, and the list is emptied.
    pub(in crate::fight) fn end_siege_as_the_fight_ends(&mut self) -> Result<()> {
        for unit in std::mem::take(&mut self.siege.active) {
            self.leave_trench(unit, false)?;
        }
        self.siege.idling.clear();
        Ok(())
    }
}
