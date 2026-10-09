//! `StealthTechSystem`: a unit that a technology puts in stealth once its life
//! first falls to a share of its maximum, for a while.
//!
//! `StealthTechEffectProvider.DoActive` hands the system each unit whose
//! technology is an `IStealthTechDataSource` (`AddMech`), which listens to the
//! unit's `OnLifeChange`. As the fight starts every unit it holds is pending
//! (`OnEnterFight`). A hit that takes life from a pending unit, alive and left
//! with no more than the share of its maximum life, puts it in stealth
//! (`ActiveStealth`): its visibility becomes `Stealth`, and it is triggered for
//! the rest of the fight. `StealthTechSystem.Update` counts each triggered
//! unit's time in stealth, and once it is past the duration shows the unit
//! again (`EndStealth`) on every update from then on. Nothing sets the time
//! back during a fight, so a unit goes into stealth once.
//!
//! What stealth does is the visibility's: no selector takes the unit and no
//! skill reaches it, but a shot, a splash or a sweep already on its way
//! strikes it (`IsValidTarget(Stealth)`), and `PerformHitTargetEffect` and
//! `ReduceLife` take nothing from it. `docs/rules/technology_effects.md`
//! states the rule.

use super::*;
use crate::modifier::Stealth;

/// `StealthTechSystem`'s units.
#[derive(Debug, Clone, Default)]
pub(in crate::fight) struct StealthSystem {
    /// `stealthOwnerDatas` and `stealthOwnerDurations`: each unit's source and
    /// its time in stealth, Q32.32 seconds.
    owners: BTreeMap<u64, (Stealth, i64)>,
    /// `pendingMeches`: the units that have not gone into stealth.
    pending: Vec<u64>,
    /// `triggeredMeches`: the units that have, or that a disabling buff
    /// took out of it before they did. A list, as the build's is: a unit
    /// disabled twice is in it twice.
    triggered: Vec<u64>,
}

/// `List.Remove`: the first occurrence alone.
fn remove_first(list: &mut Vec<u64>, unit: u64) -> bool {
    list.iter()
        .position(|&held| held == unit)
        .map(|index| list.remove(index))
        .is_some()
}

impl Simulation {
    /// `StealthTechSystem.AddMech`, for every unit the fight starts with
    /// whose technologies hand it a source, then `OnEnterFight`: each held
    /// unit pending and its time at zero. A unit travelling in is handed
    /// over as it arrives ([`Self::add_stealth_unit`]).
    pub(in crate::fight) fn enter_stealth_fight(&mut self) {
        let held = self
            .actors
            .iter()
            .filter(|(_, actor)| !actor.travelling)
            .filter_map(|(&id, actor)| actor.placement.stealth.map(|source| (id, source)))
            .collect::<Vec<_>>();
        let system = &mut self.stealth;
        for (id, source) in held {
            system.owners.entry(id).or_insert((source, 0));
        }
        system.triggered.clear();
        system.pending = system.owners.keys().copied().collect();
        for (_, time) in system.owners.values_mut() {
            *time = 0;
        }
    }

    /// `StealthTechSystem.AddMech` during the fight: a unit it does not hold
    /// yet is pending, and goes into stealth at once if its life is already
    /// low enough.
    pub(in crate::fight) fn add_stealth_unit(&mut self, unit: u64) {
        let Some(source) = self.actors[&unit].placement.stealth else {
            return;
        };
        let system = &mut self.stealth;
        if system.owners.contains_key(&unit) {
            return;
        }
        system.owners.insert(unit, (source, 0));
        if system.triggered.contains(&unit) || system.pending.contains(&unit) {
            return;
        }
        system.pending.push(unit);
        if self.stealth_due(unit) {
            self.go_into_stealth(unit);
        }
    }

    /// `StealthTechSystem.OnLifeChange`: a hit took life from a unit.
    pub(in crate::fight) fn stealth_on_life_change(&mut self, unit: u64) {
        if self.stealth.owners.contains_key(&unit) && self.stealth_due(unit) {
            self.go_into_stealth(unit);
        }
    }

    /// `StealthTechSystem.CheckActiveStealth`: whether the unit's life over
    /// its maximum is no more than its source's share, by
    /// `FPoint.op_LessThanOrEqual`.
    fn stealth_due(&self, unit: u64) -> bool {
        let (source, _) = self.stealth.owners[&unit];
        let actor = &self.actors[&unit];
        let share = math::q32_div(actor.life << 32, actor.stats.max_life() << 32);
        math::fpoint_less_or_equal(share, source.trigger_life_rate_q32)
    }

    /// `StealthTechSystem.ActiveStealth`: a pending unit still alive goes
    /// into stealth, unless it is hidden further already.
    fn go_into_stealth(&mut self, unit: u64) {
        let system = &mut self.stealth;
        let actor = self
            .actors
            .get_mut(&unit)
            .expect("actor identity is stable");
        if !actor.alive() || system.triggered.contains(&unit) || !system.pending.contains(&unit) {
            return;
        }
        if matches!(actor.visibility, Visibility::Normal | Visibility::Disappear) {
            actor.visibility = Visibility::Stealth;
        }
        system.triggered.push(unit);
        remove_first(&mut system.pending, unit);
    }

    /// `StealthTechSystem.EndStealth`: a triggered unit is shown.
    fn end_stealth(&mut self, unit: u64) {
        if !self.stealth.triggered.contains(&unit) {
            return;
        }
        let actor = self
            .actors
            .get_mut(&unit)
            .expect("actor identity is stable");
        if actor.visibility != Visibility::Normal {
            actor.visibility = Visibility::Normal;
        }
    }

    /// `StealthTechSystem.Update`: each triggered unit no longer pending is
    /// shown once its time is past its duration (`FPoint.op_GreaterThan`),
    /// and counts the tick otherwise.
    pub(in crate::fight) fn step_stealth(&mut self) {
        for unit in self.stealth.triggered.clone() {
            let Some(&(source, time)) = self.stealth.owners.get(&unit) else {
                continue;
            };
            if self.stealth.pending.contains(&unit) {
                continue;
            }
            if rvo::fpoint_greater_than(time, source.duration_q32) {
                self.end_stealth(unit);
            } else if let Some((_, time)) = self.stealth.owners.get_mut(&unit) {
                *time = time.saturating_add(NATIVE_LOGIC_DELTA_Q32);
            }
        }
    }

    /// `StealthTechSystem.DisableStealthTech` and `EnableStealthTech`, as a
    /// disabling buff switches the unit's technologies off and on. Off, the
    /// unit counts as triggered and is shown. On, a unit still pending is no
    /// longer triggered, and goes into stealth at once if its life is low
    /// enough.
    pub(in crate::fight) fn switch_stealth(&mut self, unit: u64, on: bool) {
        if !self.stealth.owners[&unit].0.can_disable {
            return;
        }
        if on {
            if self.stealth.pending.contains(&unit) {
                remove_first(&mut self.stealth.triggered, unit);
            }
            if self.stealth_due(unit) {
                self.go_into_stealth(unit);
            }
        } else {
            self.stealth.triggered.push(unit);
            self.end_stealth(unit);
        }
    }

    /// Whether `StealthTechSystem` holds a unit.
    pub(in crate::fight) fn holds_stealth(&self, unit: u64) -> bool {
        self.stealth.owners.contains_key(&unit)
    }

    /// `StealthTechSystem.OnExitFight`: every triggered unit is shown, and
    /// the lists are emptied.
    pub(in crate::fight) fn end_stealth_as_the_fight_ends(&mut self) {
        for unit in self.stealth.triggered.clone() {
            self.end_stealth(unit);
        }
        let system = &mut self.stealth;
        system.pending.clear();
        system.triggered.clear();
        for (_, time) in system.owners.values_mut() {
            *time = 0;
        }
    }
}
