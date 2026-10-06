//! A permanent preemptive skill (`PreemptiveSkillController`).
//!
//! `MarkPermanentPreemptiveSkill` locks the skill as the fight begins
//! (`SkillLockState`). After the unit's skills have updated,
//! `SkillManager.Update` runs the controller: once its condition holds, the
//! skill takes the main skill's place. The main skill locks, the skill writes
//! its buffs and takes the motion (`OnPermanentPreemptiveSkillActive`), and,
//! with no transition to wait out, leaves its lock for its idle state while the
//! motion stops and idles. From then on it is the unit's main searcher
//! (`FightSkillBase.IsMainSearcher`): its lock is the unit's.

use super::*;
use crate::data::{Correction, Entry, Index};

/// What tags a preemptive skill's buff's entries.
const PREEMPTIVE_SOURCE: &str = "preemptive";

/// `FPoint`'s equality tolerance, in raw units: `op_LessThanOrEqual` holds
/// within it.
const FPOINT_EPSILON: i64 = 43;

impl Simulation {
    /// The unit's permanent preemptive skill, its index among its extra
    /// skills.
    pub(in crate::fight) fn permanent_preemptive(&self, actor_id: u64) -> Option<usize> {
        self.actors[&actor_id]
            .skills
            .extras
            .iter()
            .position(|extra| extra.rules.preemptive.is_some())
    }

    /// `PreemptiveSkillController.Update`, after the unit's skills: a
    /// permanent preemptive skill not yet active activates once its
    /// condition holds.
    pub(in crate::fight) fn update_preemptive(
        &mut self,
        actor_id: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let actor = &self.actors[&actor_id];
        if actor.skills.preemptive_active || !actor.alive() {
            return Ok(());
        }
        let Some(index) = self.permanent_preemptive(actor_id) else {
            return Ok(());
        };
        let preemptive = actor.skills.extras[index]
            .rules
            .preemptive
            .clone()
            .expect("a permanent preemptive skill has its condition");
        // `PermanentPreemptiveActiveConditionLifeController.CheckCanActive`:
        // the life over the maximum, as `FPoint`s, no more than the
        // condition's share.
        let max_life = actor.stats.max_life().max(1);
        let share_q32 = i64::try_from((i128::from(actor.life) << 32) / i128::from(max_life))
            .map_err(|_| Error::new("a unit's share of its life is outside i64"))?;
        let condition_q32 = crate::rules::metres_q32(preemptive.life_below);
        if share_q32 > condition_q32 && share_q32 - condition_q32 > FPOINT_EPSILON {
            return Ok(());
        }
        let team = actor.placement.team;
        let object = actor.object_ref();
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        // `SkillManager.ActivePermanentPreemptiveSkill`: the skill is set and
        // the main skill locks (`SkillLockState.Enter` clears its targets).
        actor.skills.preemptive_active = true;
        lock(&mut actor.skills.main);
        // `FightExplosionSkill.OnPermanentPreemptiveSkillActive` gives the
        // unit an `AutoMoveBehaviour` that asks this skill.
        actor.motion.attacker = SkillSlot::Extra(index);
        // `ChangeToLockState`, then, with no transition to wait out,
        // `ChangeToIdleState`: `SkillLockState.Exit` restarts its search
        // timer.
        let skill = &mut actor.skills.extras[index].skill;
        lock(skill);
        skill.enter(SkillState::Idle { ready_step: None });
        skill.search_target_time = SEARCH_TARGET_RESET_TICKS;
        // `MotionController.ChangeToStopState`, then its idle state.
        self.enter_motion_idle(actor_id);
        // `FightSkill.OnPermanentPreemptiveSkillActive`: its buff, on the
        // unit, from the unit.
        let buff = &preemptive.buff;
        let row = super::super::tower::BuffRow {
            clears_when_technologies_disabled: false,
            buff_id: buff.id,
            max_life_rate: 0,
            stacking: None,
            divide: buff.divide,
            additive: buff.additive,
            ticks: u32::try_from(seconds_q32_to_steps(crate::rules::metres_q32(
                buff.duration,
            )))
            .unwrap_or(u32::MAX),
            source: PREEMPTIVE_SOURCE,
            entries: [
                Correction::Value(
                    buff.move_speed_value * crate::rules::SPACE_UNITS_PER_METER_SCALE,
                ),
                super::super::tower::rate(buff.move_speed_rate),
            ]
            .into_iter()
            .filter(|correction| !correction.neutral())
            .map(|correction| Entry {
                index: Index::MoveSpeed,
                source: PREEMPTIVE_SOURCE,
                correction,
            })
            .collect(),
            disables_technology: false,
            debuff: buff.debuff,
            invincible: buff.invincible,
            life_change: None,
            current_life_rate: buff.current_life_rate,
        };
        self.write_buff(actor_id, Some(object), team, &row, events)?;
        Ok(())
    }
}

/// `SkillLockState.Enter`: the skill lets its lock and what it fires at go,
/// and anything under way with them.
pub(super) fn lock(skill: &mut Skill) {
    skill.drop_lock();
    skill.attack_target_left = None;
    skill.performer.stop();
    skill.idle = false;
    skill.enter(SkillState::Locked);
}
