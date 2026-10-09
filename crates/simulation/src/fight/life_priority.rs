//! `SkillSearchTargetProvider` of a technology: a unit whose main skill
//! searches for the enemy of the most life in its reach.
//!
//! A `SearchTargetModifyTech` answers `ISkillSearchTargetProviderDataSource`
//! with its row's `SkillSearchTargetType`, `CurrentLifeHighestFirst` for
//! Fortified Target Lock. Its provider, a `SingleEffectProvider`, turns its
//! unit's main skill's selector to it as the unit's effects are activated
//! (`DoActive`, `FightSkill.ChangeSearchTargetType`,
//! `SearchTargetController.Change`, which makes a
//! `LifePriorityTargetSelector`), and back to `Normal` as they are
//! deactivated or switched off (`DoDeactive`, `DoDisable`); `DoEnable` turns
//! it again. A selector that is no `ScoreRatingTargetSelector` is never
//! prepared at the tick's start (`MainSkillSearchTargetController.
//! PrepareSearch`), so the skill searches with `Select` whenever it searches
//! ([`Simulation::life_priority`]). `docs/rules/technology_effects.md` states
//! the rule.

use super::*;

impl Simulation {
    /// `SkillSearchTargetProvider.DoActive` of every unit the fight starts
    /// with on the ground; a travelling unit's selector turns as it lands.
    pub(in crate::fight) fn enter_life_priority_fight(&mut self) {
        for actor in self.actors.values_mut() {
            if !actor.travelling && actor.placement.effects.single.life_priority {
                actor.life_priority = true;
            }
        }
    }

    /// `SkillSearchTargetProvider.DoActive` and `DoEnable` (`on`), and
    /// `DoDeactive` and `DoDisable`: the unit's main skill's selector turned
    /// to `CurrentLifeHighestFirst` or back to `Normal`.
    pub(in crate::fight) fn switch_life_priority(&mut self, unit: u64, on: bool) {
        let actor = self
            .actors
            .get_mut(&unit)
            .expect("actor identity is stable");
        if actor.placement.effects.single.life_priority {
            actor.life_priority = on;
        }
    }
}
