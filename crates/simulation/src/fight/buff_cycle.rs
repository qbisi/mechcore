//! `BuffSystem`: the buffs a unit's equipment adds on a trigger.
//!
//! A `BuffEquipment` hands its unit's `BuffEffectProvider` a source, and
//! `BuffEffectProvider.RegisterEffectEvent` a `BuffCycleController` in its
//! side's `TeamBuffCycleManager`. The one trigger read is the fight's start:
//! `BuffCycleController.OnEnterFight` starts the controller of a unit not
//! travelling, and its first `Update`, as `BuffSystem` updates on the fight's
//! first tick before `CommanderSkillSystem`, adds the buff to the unit
//! itself through `BuffSystem.AddBuffByCheck`, once. `docs/rules/equipment_effects.md`
//! states the rule.

use super::{tower::BuffRow, *};
use crate::data::{Entry, Index};

/// What tags the entries an equipment's buff writes.
const SOURCE: &str = "EquipmentBuff";

impl Simulation {
    /// `TeamBuffCycleManager.Update` on the fight's first tick: every unit
    /// whose equipment adds a buff as the fight starts takes it, blue's units
    /// first, each recorded as written by the unit itself.
    pub(in crate::fight) fn step_buff_cycles(&mut self, events: &mut Vec<Event>) -> Result<()> {
        let mut started = self
            .actors
            .iter()
            .filter(|(_, actor)| actor.start_buffs_pending)
            .map(|(&id, actor)| (actor.placement.team, id))
            .collect::<Vec<_>>();
        started.sort_unstable();
        for (team, id) in started {
            let actor = self.actors.get_mut(&id).expect("actor identity is stable");
            actor.start_buffs_pending = false;
            let source = actor.object_ref();
            for buff in actor.placement.start_buffs.clone() {
                let row = BuffRow {
                    buff_id: buff.buff_id,
                    divide: buff.divide,
                    additive: buff.additive,
                    ticks: u32::try_from(seconds_q32_to_steps(buff.duration_q32))
                        .map_err(|_| Error::new("an equipment's buff outlasts a fight"))?,
                    source: SOURCE,
                    entries: (buff.amplify_damage_rate != 0)
                        .then(|| Entry {
                            index: Index::AmplifyDamage,
                            source: SOURCE,
                            correction: super::tower::rate(buff.amplify_damage_rate),
                        })
                        .into_iter()
                        .collect(),
                    disables_technology: false,
                    debuff: buff.debuff,
                    invincible: buff.invincible,
                    life_change: None,
                    current_life_rate: 0,
                };
                if self.buff_reaches(id, &row) {
                    self.write_buff(id, Some(source), team, &row, events)?;
                }
            }
        }
        Ok(())
    }
}
