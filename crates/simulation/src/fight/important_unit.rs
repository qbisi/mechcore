//! `DeadEffectSystem`'s important units: a side does not outlive its last.
//!
//! An equipment row that sets `importantUnit`, Dominion Core's, marks the unit
//! wearing it as it writes its numbers (`MechDataModifer.TryAddCommonData`),
//! and `DeadEffectSystem` records each side's as the fight starts. As it takes
//! the tick's deaths (`DeadEffectSystem.Update`), the death of an important
//! unit that leaves its side none (`TryProcessDeadImportantUnit`,
//! `CheckTeamImportantUnit`) makes every other unit of the side still standing
//! destroy itself (`FightActor.DestroySelf`): each loses its whole life to no
//! one and dies after it, in its side's order.
//! `docs/rules/equipment_effects.md` states the rule.

use super::*;

impl Simulation {
    /// `DeadEffectSystem.TryProcessDeadImportantUnit` for each important
    /// unit that died this tick, in the order they died. What it destroys
    /// dies after every other death of the tick, as `OnActorDead` adds it to
    /// the end of `deadActors`.
    pub(in crate::fight) fn lose_important_units(&mut self, events: &[Event]) -> Result<()> {
        // The tick's deaths: those its shots caused wait, after the rest, in
        // the order they struck.
        let fallen = events
            .iter()
            .chain(&self.fallen_buildings)
            .filter(|event| matches!(event.payload, EventPayload::UnitDied { .. }))
            .filter_map(|event| event.subject)
            .filter(|subject| subject.kind == ObjectKind::Unit)
            .map(|subject| subject.id)
            .filter(|id| self.actors[id].placement.effects.important)
            .collect::<Vec<_>>();
        for dead_id in fallen {
            let team = self.actors[&dead_id].placement.team;
            if self.actors.values().any(|actor| {
                actor.placement.team == team && actor.placement.effects.important && actor.alive()
            }) {
                continue;
            }
            // `CheckTeamImportantUnit` destroys its side's `activeMeches`:
            // whether a summon still appearing or a unit still travelling is
            // among them is not measured.
            if self.appearing_on(team)
                || self
                    .actors
                    .values()
                    .any(|actor| actor.placement.team == team && actor.alive() && actor.travelling)
            {
                return Err(Error::new(format!(
                    "unit {dead_id}, its side's last important unit, dies while a unit of its \
                     side is still appearing or travelling, which is not measured"
                )));
            }
            let side = self
                .units_in_update_order()
                .into_iter()
                .filter(|id| {
                    let actor = &self.actors[id];
                    actor.placement.team == team && actor.alive()
                })
                .collect::<Vec<_>>();
            let mut deaths = Vec::with_capacity(side.len());
            for unit_id in side {
                let unit = self
                    .actors
                    .get_mut(&unit_id)
                    .expect("actor identity is stable");
                // `DestroySelf` is a suicide's `ReduceLife` of its whole life:
                // no shield takes it, and no one dealt it.
                unit.life = 0;
                unit.last_damage_source = None;
                let position = QVec3 {
                    x: unit.x_q32,
                    y: space_to_q32(unit_height(unit.domain)),
                    z: unit.z_q32,
                };
                unit.exit_fight_on_death();
                deaths.push((unit_id, position));
            }
            let mut recorded = Vec::new();
            self.record_deaths(deaths, &mut recorded);
            self.fallen_buildings.extend(recorded);
        }
        Ok(())
    }
}
