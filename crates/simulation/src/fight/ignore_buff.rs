//! `IgnoreBuffEffectProvider` of a technology: a unit that ignores one kind
//! of buff effect.
//!
//! An `IgnoreBuffEffectTech` whose `IsIgnoreBuffEffect` holds makes its unit
//! ignore its row's `BuffEffectType`: `IgnoreBuffEffectSystem.ApplyIgnoreBuff`
//! raises the unit's `BuffManager.stateDatas` of that kind
//! (`AddIgnoredBuffEffectData`), and while it is above zero a buff writes
//! nothing of it (`Buff.AddEffect`, `Buff.Reset` and `Buff.RefreshEffect`
//! pass over a `BuffDataFloatRate` of that index). What a buff wrote before
//! stays until it ends. The only kind a technology ignores is
//! `SpeedChangeRate`, Power Armor's.
//!
//! `IgnoreBuffEffectSystem.Active` holds a unit until the fight begins and
//! `OnEnterFight` applies it; one activated in the fight is applied at once.
//! `Deactive` takes it away, and so does `Disable` as the unit's
//! technologies are switched off, the row's duration being none; `Enable`
//! applies it again. `docs/rules/technology_effects.md` states the rule.

use super::*;

impl Simulation {
    /// `IgnoreBuffEffectSystem.OnEnterFight`: every unit the fight starts
    /// with on the ground, held since its effects were activated, applied. A
    /// travelling unit is applied as it lands ([`Self::add_ignore_buff_unit`]).
    pub(in crate::fight) fn enter_ignore_buff_fight(&mut self) {
        for actor in self.actors.values_mut() {
            if !actor.travelling && actor.placement.effects.ignores_speed_rate {
                actor.ignores_speed_rate = true;
            }
        }
    }

    /// `IgnoreBuffEffectSystem.Active` in the fight, `ApplyIgnoreBuff` at
    /// once.
    pub(in crate::fight) fn add_ignore_buff_unit(&mut self, unit: u64) {
        let actor = self
            .actors
            .get_mut(&unit)
            .expect("actor identity is stable");
        if actor.placement.effects.ignores_speed_rate {
            actor.ignores_speed_rate = true;
        }
    }

    /// `IgnoreBuffEffectSystem.Deactive` (`RemoveIgnoreBuff`), as the unit
    /// dies, and `Disable` and `Enable` as its technologies are switched off
    /// and on.
    pub(in crate::fight) fn switch_ignore_buff(&mut self, unit: u64, on: bool) {
        let actor = self
            .actors
            .get_mut(&unit)
            .expect("actor identity is stable");
        if actor.placement.effects.ignores_speed_rate {
            actor.ignores_speed_rate = on;
        }
    }
}
