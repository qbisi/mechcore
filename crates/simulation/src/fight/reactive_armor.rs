//! `ReactiveArmorSystem`: the rate a reactive armor technology puts on the
//! damage its unit takes, for as many hits as its count.
//!
//! `ReactiveArmorTechEffectProvider.DoActive` lists the unit with its count
//! (`reactiveArmorDataInfoDict`, `reactiveArmorRemainCountDict`); a unit
//! listed already is left as it is. Before the fight, that is all it does,
//! and `OnEnterFight` writes on every unit listed the technology's rate as
//! its `MechDataChangeFloatRate.AmplifyDamageRate` (`AddEffect`), its count
//! whole. During the fight (`FightController.IsFighting`), `DoActive`
//! writes it at once on a unit not in force: a unit arriving from its
//! travel, or a summon or a make as `FightEffectSystem` activates its
//! effects. `FightCalculator.PerformHitTargetEffect` multiplies a hit on the
//! unit by it as by a buff's amplify rates.
//!
//! `FightController.OnActorHitted` hands every hit to
//! `OnReactiveArmorOwnerDamaged`, which takes one off the count of a unit in
//! force for a hit that took life (`damageReal` above zero), and takes the
//! rate away (`RemoveEffect`) once the count reaches none: the hit that does
//! has had the rate.

use super::*;
use crate::data::{Channel, Entry, Index};
use crate::modifier::ReactiveArmor;

/// What the system writes the rate under, which is how it takes it away.
const SOURCE: &str = "ReactiveArmorSystem";

/// One unit's entry in `ReactiveArmorSystem`.
#[derive(Debug, Clone, Copy)]
pub(in crate::fight) struct ReactiveArmorState {
    source: ReactiveArmor,
    /// Whether `DoActive` has listed it (`reactiveArmorDataInfoDict`).
    listed: bool,
    /// `reactiveArmorRemainCountDict`'s count.
    remaining: i32,
    /// Whether it is in `activeMeches`, its rate written.
    active: bool,
}

impl ReactiveArmorState {
    /// The entry a unit's technology will give it, not listed yet.
    pub(in crate::fight) fn of(placement: &crate::layout::Placement) -> Option<Self> {
        placement.effects.reactive_armor.map(|source| Self {
            source,
            listed: false,
            remaining: source.count,
            active: false,
        })
    }
}

impl Simulation {
    /// `DoActive` of every unit the fight starts with and not travelling,
    /// as `FightEffectSystem` activates their effects, then
    /// `ReactiveArmorSystem.OnEnterFight`: every unit listed has its rate
    /// written and its count whole.
    pub(in crate::fight) fn enter_reactive_armor_fight(&mut self) {
        for actor in self.actors.values_mut() {
            if actor.travelling {
                continue;
            }
            actor.list_reactive_armor();
            actor.enter_reactive_armor();
        }
    }

    /// `ReactiveArmorTechEffectProvider.DoActive` during the fight, for a
    /// unit whose effects are activated after it started: listed with its
    /// count, and its rate written if it is not in force.
    pub(in crate::fight) fn activate_reactive_armor(&mut self, unit: u64) {
        let Some(actor) = self.actors.get_mut(&unit) else {
            return;
        };
        if !actor.list_reactive_armor() {
            return;
        }
        let Some(state) = actor.reactive_armor.as_mut() else {
            return;
        };
        if state.active {
            return;
        }
        state.active = true;
        actor.add_reactive_armor_effect();
    }
}

impl Actor {
    /// The listing `DoActive` begins with: a unit with a source and not
    /// listed yet is listed with its whole count. Answers whether it was.
    fn list_reactive_armor(&mut self) -> bool {
        let Some(state) = self.reactive_armor.as_mut() else {
            return false;
        };
        if state.listed {
            return false;
        }
        state.listed = true;
        state.remaining = state.source.count;
        true
    }

    /// `ReactiveArmorSystem.OnEnterFight` for this unit: a listed one has its
    /// rate written and its count whole.
    fn enter_reactive_armor(&mut self) {
        let Some(state) = self.reactive_armor.as_mut() else {
            return;
        };
        if !state.listed {
            return;
        }
        state.remaining = state.source.count;
        state.active = true;
        self.add_reactive_armor_effect();
    }

    /// `ReactiveArmorSystem.OnReactiveArmorOwnerDamaged` for a hit on this
    /// unit that took `actual` of its life.
    pub(in crate::fight) fn reactive_armor_hit(&mut self, actual: i64) {
        let Some(state) = self.reactive_armor.as_mut() else {
            return;
        };
        if actual <= 0 || !state.active || state.remaining == 0 {
            return;
        }
        state.remaining -= 1;
        if state.remaining == 0 {
            state.active = false;
            self.remove_reactive_armor_effect();
        }
    }

    /// `ReactiveArmorSystem.DisableReactiveArmor` and `EnableReactiveArmor`:
    /// switched off, a listed unit in force has its rate taken away and keeps
    /// its count; switched on, a listed one out of force with a count left
    /// has it written again.
    pub(in crate::fight) fn switch_reactive_armor(&mut self, on: bool) {
        let Some(state) = self.reactive_armor.as_mut() else {
            return;
        };
        if !state.listed {
            return;
        }
        if on {
            if state.remaining < 1 || state.active {
                return;
            }
            state.active = true;
            self.add_reactive_armor_effect();
        } else if state.active {
            state.active = false;
            self.remove_reactive_armor_effect();
        }
    }

    /// `ReactiveArmorSystem.AddEffect`: the rate on the unit's
    /// `AmplifyDamageRate`. Its `ReduceDamageValue`, `GetDamageReduceValue`,
    /// reads a list no row sets.
    fn add_reactive_armor_effect(&mut self) {
        let Some(state) = self.reactive_armor else {
            return;
        };
        self.stats.overlays.channel(Channel::Unit).write(Entry {
            index: Index::AmplifyDamage,
            source: SOURCE,
            correction: super::tower::rate(state.source.rate_q32),
        });
        self.stats
            .refresh(&self.rules)
            .expect("a rate on damage taken resolves");
    }

    /// `ReactiveArmorSystem.RemoveEffect`.
    fn remove_reactive_armor_effect(&mut self) {
        self.stats.overlays.channel(Channel::Unit).withdraw(SOURCE);
        self.stats
            .refresh(&self.rules)
            .expect("the numbers resolved before the rate was written");
    }
}
