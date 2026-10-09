//! `ReactiveArmorSystem`: the rate a reactive armor technology puts on the
//! damage its unit takes, for as many hits as its count.
//!
//! `ReactiveArmorTechEffectProvider.DoActive` lists the unit and its count
//! (`AddReactiveArmorOwner`), and `OnEnterFight` writes on every unit
//! listed the technology's rate as its `MechDataChangeFloatRate.AmplifyDamageRate`
//! (`AddEffect`), with its whole count. `FightCalculator.PerformHitTargetEffect`
//! multiplies a hit on the unit by it as by a buff's amplify rates.
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
    /// `reactiveArmorRemainCountDict`'s count.
    remaining: i32,
    /// Whether it is in `activeMeches`, its rate written.
    active: bool,
}

impl ReactiveArmorState {
    /// A unit's entry before the fight starts: listed with its count.
    pub(in crate::fight) fn of(placement: &crate::layout::Placement) -> Option<Self> {
        placement.reactive_armor.map(|source| Self {
            source,
            remaining: source.count,
            active: false,
        })
    }
}

impl Simulation {
    /// `ReactiveArmorSystem.OnEnterFight`: every unit listed, which is every
    /// unit the fight starts with whose technology gives it one (a unit
    /// travelling in is refused).
    pub(in crate::fight) fn enter_reactive_armor_fight(&mut self) {
        for actor in self.actors.values_mut() {
            actor.enter_reactive_armor();
        }
    }
}

impl Actor {
    /// `ReactiveArmorSystem.OnEnterFight` for this unit: its rate written and
    /// its count whole.
    fn enter_reactive_armor(&mut self) {
        let Some(state) = self.reactive_armor.as_mut() else {
            return;
        };
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
    /// switched off, a unit in force has its rate taken away and keeps its
    /// count; switched on, one out of force with a count left has it
    /// written again.
    pub(in crate::fight) fn switch_reactive_armor(&mut self, on: bool) {
        let Some(state) = self.reactive_armor.as_mut() else {
            return;
        };
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
