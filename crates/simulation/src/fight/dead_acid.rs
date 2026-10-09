//! `DeadAcidRangeItemController`: an acid a technology leaves where its unit
//! dies.
//!
//! `DeadEffectProvider` hands a unit whose technology is an
//! `IDeadAcidRangeItem` to `DeadEffectSystem`'s acid controller, the second
//! of its controllers (`DeadEffectSystem.Init`: buff, acid, explosive,
//! summon, rebirth). As the module updates, each unit that died this tick
//! and that the controller holds leaves the technology's acid where it fell,
//! for the side it stands on (`PerformDeadEffect`,
//! `RangeItemSystem.AddItem`): a battle skill's acid, of the technology's
//! range and rounds, writing its buff on the units in it.
//! `docs/rules/technology_effects.md` states the rule.

use super::*;

impl Simulation {
    /// `DeadEffectController.IsAvaliable` of a unit a technology's dead
    /// effect is on: the controller holds it once its effects are active,
    /// not while it travels in, and not while a buff has its technologies
    /// off (`DeadEffectProvider.DisableEffect` takes the effect away,
    /// `EnableEffect` hands it back).
    pub(in crate::fight) fn technology_dead_effect_held(&self, unit: u64) -> bool {
        let actor = &self.actors[&unit];
        !actor.travelling && !actor.technology_disabled()
    }

    /// `DeadAcidRangeItemController.PerformDeadEffect` of every unit that
    /// died this tick, in the order they died.
    pub(in crate::fight) fn step_dead_acids(&mut self) -> Result<()> {
        for unit in self.dead_exits.clone() {
            let actor = &self.actors[&unit];
            let Some(spec) = actor.placement.effects.single.dead_acid else {
                continue;
            };
            if !self.technology_dead_effect_held(unit) {
                continue;
            }
            let position = (
                actor.x_q32,
                space_to_q32(unit_height(actor.rules.domain)),
                actor.z_q32,
            );
            let team = actor.placement.team;
            self.add_terrain(team, &format!("unit {unit}"), spec, position)?;
        }
        Ok(())
    }
}
