//! `CommanderSkillSystem`: a released battle skill lands and writes its buff.
//!
//! A released skill is `CommanderSkillReleaseState` under `CSRC_Common`: it
//! waits, its one sub-effect falls, and on the tick the sub-effect lands
//! `CommanderSkillSubEffectController.PerformNegativeEffect` reaches every
//! unit whose edge is within the skill's range of where it landed, of either
//! domain and of either side, the releasing side's own included, and writes
//! the skill's buff on each. `CommanderSkillSystem`
//! updates after `BuffSystem` and before `MineSystem`, so what it writes runs
//! from the next tick. The layout compiled the tick it lands on;
//! `docs/rules/battle_skill.md` states the rule.

use super::rvo::fpoint_less_or_equal;
use super::*;
use crate::{
    data::{Entry, Index},
    layout::{SkillBuff, SkillEffect, SkillRelease},
};

/// What tags a battle skill's buff, so that its end takes it away.
const SKILL_SOURCE: &str = "BuffSystem.CommanderSkill";

impl Simulation {
    /// `CommanderSkillSystem`'s update: every release whose sub-effect lands
    /// on this tick, in the order the sides released them.
    pub(in crate::fight) fn step_battle_skills(
        &mut self,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let tick = step + 1;
        let landing = self
            .battle_skills
            .iter()
            .filter(|release| release.lands_on == tick)
            .cloned()
            .collect::<Vec<_>>();
        for release in landing {
            match &release.effect {
                SkillEffect::Buff { range_q32, buff } => {
                    let reached = self.skill_reach(&release, *range_q32, target_search_order);
                    self.write_skill_buff(&release, buff, &reached, events)?;
                }
                // `SupportUnitEffectController.PerformEffect`: a creator for
                // the side's `TeamSupportUnitManager`, which updates later in
                // this very tick.
                SkillEffect::Summon(summon) => {
                    self.creators.push(super::support_unit::Creator::new(
                        release.team,
                        release.x,
                        release.z,
                        (**summon).clone(),
                    ));
                }
            }
        }
        Ok(())
    }

    /// `RangeTargetCalculator.CalculateRangeActors` as the sub-effect asks it,
    /// and what `PerformNegativeEffect` keeps of it: every live unit of either
    /// side, ground or air, whose edge is within the range of where the skill
    /// landed by `FPoint.op_LessThanOrEqual`, side by side in the order each
    /// side's objects are searched. The calculator is given no team, and
    /// `IsBuffTarget` keeps units alone.
    fn skill_reach(
        &self,
        release: &SkillRelease,
        range_q32: i64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Vec<u64> {
        let (x_q32, z_q32) = (space_to_q32(release.x), space_to_q32(release.z));
        target_search_order
            .values()
            .flatten()
            .filter_map(|&candidate| {
                let FightActorRef::Unit(id) = candidate else {
                    return None;
                };
                if !self.actors[&id].alive() {
                    return None;
                }
                let view = self.fight_actor(candidate)?;
                let distance = native_q32_magnitude(
                    view.x_q32.saturating_sub(x_q32),
                    view.z_q32.saturating_sub(z_q32),
                )
                .saturating_sub(space_to_q32(view.radius));
                fpoint_less_or_equal(distance, range_q32).then_some(id)
            })
            .collect()
    }

    /// `BuffSystem.AddBuff` of the skill's row on every unit it reached.
    fn write_skill_buff(
        &mut self,
        release: &SkillRelease,
        buff: &SkillBuff,
        reached: &[u64],
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let row = super::tower::BuffRow {
            buff_id: buff.id,
            divide: buff.divide,
            additive: buff.additive,
            ticks: buff.ticks,
            source: SKILL_SOURCE,
            entries: vec![Entry {
                index: Index::MoveSpeed,
                source: SKILL_SOURCE,
                correction: super::tower::rate(buff.move_speed_rate),
            }],
            disables_technology: buff.disable_technology,
        };
        for &id in reached {
            let actor = &self.actors[&id];
            if buff.disable_technology && self.researched.contains(&actor.placement.team) {
                return Err(Error::new(format!(
                    "{} reaches unit {id}, whose side researched a technology, and disabling \
                     a technology mid-fight is not measured",
                    release.name
                )));
            }
            if let Some(running) = actor.other_buff(buff.id) {
                return Err(Error::new(format!(
                    "{} reaches unit {id}, which runs buff {running}, and a skill's buff over \
                     another is not measured",
                    release.name
                )));
            }
            events.push(self.write_buff(id, release.team, &row)?);
        }
        Ok(())
    }
}
