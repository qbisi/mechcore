//! `WreckageRecoverySystem`: a unit that a technology heals as an enemy it
//! struck dies.
//!
//! `WreckageRecoveryEffectProvider.DoActive` hands each unit whose technology
//! is an `IWreckageRecovery` to its side's `TeamWreckageRecoveryManager`
//! (`Add`), which hands the skills the source names a hit effect
//! (`SkillManager.AddHitEffect`). Each unit a hit of one of them strikes is
//! recorded against the unit, its time at none again when it is already
//! (`PerformHitEffect`), unless the unit's technologies are disabled. The
//! manager's `Update`, after `DeadEffectSystem`, counts every record's
//! updates and drops it once they reach the source's time over a tick. As a
//! recorded unit dies (`FightMech.OnDead`, `OnMechDead`), each holder alive
//! and short of its maximum life drops its last record of it, and those
//! within the source's distance of it share its maximum life, each taking
//! the whole part of it over their count and at least 1
//! (`PerformTargetDeadEffect`, `PerformRecoveryEffect`).
//! `docs/rules/technology_effects.md` states the rule.

use super::*;
use crate::modifier::WreckageRecovery;

/// One holder's `RecoveryInfo`.
#[derive(Debug, Clone)]
struct Holder {
    source: WreckageRecovery,
    /// `recoverTime`: the whole part of the source's time over a tick
    /// (`FPoint.op_Division`).
    recover_updates: i64,
    /// `attackInfos`: each unit struck and the updates since, in the order
    /// they were first struck.
    struck: Vec<(u64, i64)>,
}

/// The sides' `TeamWreckageRecoveryManager`s.
#[derive(Debug, Clone, Default)]
pub(in crate::fight) struct WreckageSystem {
    holders: BTreeMap<u64, Holder>,
    /// `recoveryInfos`' order: the units the fight starts with by where they
    /// stand (`OnFightStart` sorts them by `FightUtility.SkillOwnerComparer`),
    /// then those that arrive later.
    order: Vec<u64>,
    /// The healing each unit that died this tick gave, which the record
    /// reads after its death.
    healing: BTreeMap<u64, Vec<Event>>,
}

impl Simulation {
    /// `TeamWreckageRecoveryManager.Add` for every unit the fight starts
    /// with whose technologies hand it a source, then `OnFightStart`'s sort
    /// (`FightUtility.SkillOwnerComparer`): by the second coordinate where
    /// each stands, then the first. A unit travelling in is handed over as
    /// it arrives ([`Self::add_wreckage_unit`]).
    pub(in crate::fight) fn enter_wreckage_fight(&mut self) {
        let mut held = self
            .actors
            .iter()
            .filter(|(_, actor)| !actor.travelling)
            .filter_map(|(&id, actor)| {
                let source = actor.placement.wreckage.clone()?;
                Some(((actor.z_q32, actor.x_q32), id, source))
            })
            .collect::<Vec<_>>();
        held.sort_by_key(|(place, _, _)| *place);
        for (_, id, source) in held {
            self.hold_wreckage(id, source);
        }
    }

    /// `TeamWreckageRecoveryManager.Add` during the fight.
    pub(in crate::fight) fn add_wreckage_unit(&mut self, unit: u64) {
        if let Some(source) = self.actors[&unit].placement.wreckage.clone() {
            self.hold_wreckage(unit, source);
        }
    }

    fn hold_wreckage(&mut self, unit: u64, source: WreckageRecovery) {
        if self.wreckage.holders.contains_key(&unit) {
            return;
        }
        let recover_updates = math::q32_div(source.time_q32, NATIVE_LOGIC_DELTA_Q32) >> 32;
        self.wreckage.holders.insert(
            unit,
            Holder {
                source,
                recover_updates,
                struck: Vec::new(),
            },
        );
        self.wreckage.order.push(unit);
    }

    /// `TeamWreckageRecoveryManager.PerformHitEffect`: each unit a hit of
    /// the holder's skill struck, recorded or its time set back to none. A
    /// skill the source names is one a technology's numbers reach, as a buff
    /// source's hit effect's is; the technologies disabled, nothing is
    /// recorded (`WreckageRecoveryEffectProvider.DisableEffect` takes the
    /// effect off its skills).
    pub(in crate::fight) fn record_wreckage_hit(
        &mut self,
        owner: u64,
        slot: u16,
        targets: &[FightActorRef],
    ) {
        let Some(holder) = self.wreckage.holders.get(&owner) else {
            return;
        };
        let actor = &self.actors[&owner];
        if (holder.source.can_disable && actor.technology_disabled())
            || !actor.corrected_skill_slots().contains(&usize::from(slot))
        {
            return;
        }
        let struck = targets
            .iter()
            .filter_map(|target| match target {
                FightActorRef::Unit(id) => Some(*id),
                FightActorRef::Building(_) => None,
            })
            .collect::<Vec<_>>();
        let holder = self
            .wreckage
            .holders
            .get_mut(&owner)
            .expect("the holder was found above");
        for unit in struck {
            match holder.struck.iter_mut().find(|(held, _)| *held == unit) {
                Some((_, updates)) => *updates = 0,
                None => holder.struck.push((unit, 0)),
            }
        }
    }

    /// `TeamWreckageRecoveryManager.Update`: every record counts the update,
    /// last first, and goes once its count reaches the holder's time.
    pub(in crate::fight) fn step_wreckage(&mut self) {
        for unit in self.wreckage.order.clone() {
            let Some(holder) = self.wreckage.holders.get_mut(&unit) else {
                continue;
            };
            let recover_updates = holder.recover_updates;
            for index in (0..holder.struck.len()).rev() {
                holder.struck[index].1 += 1;
                if recover_updates <= holder.struck[index].1 {
                    holder.struck.remove(index);
                }
            }
        }
    }

    /// `TeamWreckageRecoveryManager.PerformTargetDeadEffect`, as a unit dies:
    /// each holder alive and short of its maximum life drops its last record
    /// of it and, within its source's distance of it at the dead unit's
    /// level (`FightActor.Distance2D`, `FPoint.op_LessThanOrEqual`), takes
    /// its share of its maximum life (`PerformRecoveryEffect`,
    /// `FightMech.RecoveryLife`).
    pub(in crate::fight) fn wreckage_on_dead(&mut self, dead: u64) -> Result<()> {
        let level = self.actors[&dead].placement.level;
        let mut healed = Vec::new();
        for unit in self.wreckage.order.clone() {
            let actor = &self.actors[&unit];
            if !actor.alive() || actor.life >= actor.stats.max_life() {
                continue;
            }
            let holder = self
                .wreckage
                .holders
                .get_mut(&unit)
                .expect("an ordered unit is held");
            let Some(index) = holder.struck.iter().rposition(|(held, _)| *held == dead) else {
                continue;
            };
            holder.struck.remove(index);
            let distance = &holder.source.distance;
            let at_level = usize::try_from(level - 1)
                .unwrap_or_default()
                .min(distance.len().saturating_sub(1));
            let reach_q32 = distance.get(at_level).copied().unwrap_or_default() << 32;
            if math::fpoint_less_or_equal(self.distance_2d(unit, dead), reach_q32) {
                healed.push(unit);
            }
        }
        if healed.is_empty() {
            return Ok(());
        }
        let count = i64::try_from(healed.len()).expect("the units are few");
        let share = (self.actors[&dead].stats.max_life() / count).max(1);
        let mut events = Vec::new();
        for unit in healed {
            self.add_life(unit, share, &mut events)?;
        }
        self.wreckage.healing.insert(dead, events);
        Ok(())
    }

    /// The healing a unit's death gave, which follows its death.
    pub(in crate::fight) fn healing_from_wreckage(&mut self, dead: u64) -> Vec<Event> {
        self.wreckage.healing.remove(&dead).unwrap_or_default()
    }

    /// `TeamWreckageRecoveryManager.Remove`, as a holder dies
    /// (`WreckageRecoveryEffectProvider.DoDeactive`).
    pub(in crate::fight) fn remove_wreckage_unit(&mut self, unit: u64) {
        if self.wreckage.holders.remove(&unit).is_some() {
            self.wreckage.order.retain(|&held| held != unit);
        }
    }

    /// `TeamWreckageRecoveryManager.OnFightEnd`: every record goes.
    pub(in crate::fight) fn end_wreckage_as_the_fight_ends(&mut self) {
        for holder in self.wreckage.holders.values_mut() {
            holder.struck.clear();
        }
    }
}
