//! `DeadEffectSystem`'s important units: active mechs die with their last.
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
use crate::modifier::IMPORTANT_SOURCE;

impl Actor {
    /// `FightMech.RemoveImportantUnitEffect`, retaining the modifier identity.
    fn remove_important_unit_effect(&mut self, step: u64) -> Result<()> {
        self.important_effect_enabled = false;
        for &(channel, entry) in &self.placement.effects.corrections {
            if entry.source == IMPORTANT_SOURCE {
                self.stats.overlays.channel(channel).remove(entry);
            }
        }
        for extra in &mut self.skills.extras {
            extra
                .skill_corrections
                .retain(|entry| entry.source != IMPORTANT_SOURCE);
        }
        self.refresh_life_data()?;
        self.refresh_common_skill_data(step)
    }

    /// `FightMech.AddImportantUnitEffect`, called on a dead translated unit.
    fn add_important_unit_effect(&mut self, step: u64) -> Result<()> {
        self.important_effect_enabled = true;
        for &(channel, entry) in &self.placement.effects.corrections {
            if entry.source == IMPORTANT_SOURCE {
                self.stats.overlays.channel(channel).write(entry);
            }
        }
        for extra in &mut self.skills.extras {
            if let Some(source) = self
                .placement
                .effects
                .extra_weapons
                .iter()
                .find(|source| source.rules.technology == extra.rules.technology)
            {
                extra.skill_corrections.extend(
                    source
                        .skill_corrections
                        .iter()
                        .copied()
                        .filter(|entry| entry.source == IMPORTANT_SOURCE),
                );
            }
        }
        self.refresh_life_data()?;
        self.refresh_common_skill_data(step)
    }
}

impl Simulation {
    /// `DeadEffectSystem.OnMechChangeTeam`: unregister a living original-side
    /// important unit; a translated unit dying restores its modifier only.
    pub(in crate::fight) fn important_unit_change_team(
        &mut self,
        unit_id: u64,
        old_team: u32,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let actor = &self.actors[&unit_id];
        if !actor.placement.effects.important {
            return Ok(());
        }
        if actor.alive() {
            if actor.original_team == old_team && actor.important_effect_enabled {
                let actor = self
                    .actors
                    .get_mut(&unit_id)
                    .expect("actor identity is stable");
                if actor.important_registered {
                    actor.important_registered = false;
                    actor.remove_important_unit_effect(self.step_now)?;
                }
                self.check_team_important_unit(old_team, unit_id, events)?;
            }
        } else if !actor.important_effect_enabled {
            self.actors
                .get_mut(&unit_id)
                .expect("actor identity is stable")
                .add_important_unit_effect(self.step_now)?;
        }
        Ok(())
    }

    /// `DeadEffectSystem.TryProcessDeadImportantUnit` after a unit's `OnDead`.
    /// What it destroys dies after every other death of the tick, as
    /// `OnActorDead` adds it to the end of `deadActors`.
    pub(in crate::fight) fn try_process_dead_important_unit(&mut self, dead_id: u64) -> Result<()> {
        let actor = self
            .actors
            .get_mut(&dead_id)
            .expect("actor identity is stable");
        if actor.summoned || !actor.important_registered || actor.rebirthing {
            return Ok(());
        }
        actor.important_registered = false;
        let team = actor.placement.team;
        let mut recorded = Vec::new();
        self.check_team_important_unit(team, dead_id, &mut recorded)?;
        self.fallen_buildings.extend(recorded);
        Ok(())
    }

    /// `DeadEffectSystem.CheckTeamImportantUnit` for ordinary important units.
    /// Registered dead units stay in the list until their death is handled.
    fn check_team_important_unit(
        &mut self,
        team: u32,
        trigger: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        if self
            .actors
            .values()
            .any(|actor| actor.placement.team == team && actor.important_registered)
        {
            return Ok(());
        }
        // `CheckTeamImportantUnit` destroys its side's `activeMeches`:
        // a pending summon joins only through SummonSystem.AddMech, so
        // it is not destroyed and can appear after the important unit
        // dies. Pending summons are separate from self.actors here.
        // Membership of a travelling unit is not measured.
        if self
            .actors
            .values()
            .any(|actor| actor.placement.team == team && actor.alive() && actor.travelling)
        {
            return Err(Error::new(format!(
                "unit {trigger}, its side's last important unit, leaves while a unit of its \
                    side is still travelling, which is not measured"
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
            // no shield takes it, and its death names the last unit that
            // struck it, as any death does: a Sledgehammer a Phoenix
            // struck six ticks before reads it as its killer, a Fang
            // nothing ever struck reads none.
            unit.life = 0;
            let position = QVec3 {
                x: unit.x_q32,
                y: space_to_q32(unit_height(unit.domain)),
                z: unit.z_q32,
            };
            // DestroySelf reaches the same OnActorDead registration as a
            // hit, including deferred dead effects. A translated unit's
            // OnMechDead listener also returns it to its original side.
            if unit.placement.team != unit.original_team {
                self.turned_fallen.push(unit_id);
            }
            self.on_actor_dead(unit_id);
            deaths.push((unit_id, position));
        }
        self.record_deaths(deaths, events);
        Ok(())
    }
}
