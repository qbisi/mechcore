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
use crate::data::{Channel, Correction, Entry, Index};

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
        // `AmmoEmptyController.CheckCanActive`: the unit's rounds are gone.
        // Its source, a melee mode's, answers `CanDisable` false.
        if let Some(ammo_empty) = &preemptive.ammo_empty {
            if actor.ammo.is_none_or(|rounds| rounds > 0) {
                return Ok(());
            }
            return self.activate_melee(actor_id, index, ammo_empty, events);
        }
        // `PermanentPreemptiveActiveConditionLifeController.CheckCanActive`
        // holds nothing while the unit's technologies are disabled, the skill
        // being a technology's (`canDisable`).
        if actor.technology_disabled() {
            return Ok(());
        }
        let (Some(life_below), Some(buff)) = (preemptive.life_below, preemptive.buff.clone())
        else {
            return Err(Error::new(format!(
                "unit {actor_id}'s permanent preemptive skill names no condition"
            )));
        };
        // `PermanentPreemptiveActiveConditionLifeController.CheckCanActive`:
        // the life over the maximum, as `FPoint`s, no more than the
        // condition's share.
        let max_life = actor.stats.max_life().max(1);
        let share_q32 = i64::try_from((i128::from(actor.life) << 32) / i128::from(max_life))
            .map_err(|_| Error::new("a unit's share of its life is outside i64"))?;
        let condition_q32 = crate::rules::metres_q32(life_below);
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
        unlock(skill);
        // `MotionController.ChangeToStopState`, then its idle state.
        self.enter_motion_idle(actor_id);
        // `FightSkill.OnPermanentPreemptiveSkillActive`: its buff, on the
        // unit, from the unit.
        let buff = &buff;
        let row = super::super::tower::BuffRow {
            clears_when_technologies_disabled: false,
            technology: false,
            buff_id: buff.id,
            max_life_rate: 0,
            stacking: None,
            summons: None,
            divide: buff.divide,
            additive: buff.additive,
            ticks: u32::try_from(seconds_q32_to_steps(crate::rules::metres_q32(
                buff.duration,
            )))
            .unwrap_or(u32::MAX),
            step_ticks: u32::try_from(seconds_q32_to_steps(crate::rules::metres_q32(
                buff.step_time,
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
            probability: crate::fight::tower::CERTAIN,
            invincible: buff.invincible,
            disables_recover: false,
            life_change_rate: 0,
            current_life_rate: buff.current_life_rate,
        };
        self.write_buff(actor_id, Some(object), team, &row, events)?;
        Ok(())
    }
}

impl Simulation {
    /// `SkillManager.ActivePermanentPreemptiveSkill` of a melee mode's skill
    /// as its unit's rounds run out: the skill is set, the main skill and the
    /// extra skills the melee skill names incompatible lock, the unit takes
    /// its whole life back as `OnMeleeSkillActive` restores it, and the
    /// skill waits out its transition locked, the motion stopped
    /// (`StartActiveTransition`, a `GRTimer` of the condition's seconds).
    fn activate_melee(
        &mut self,
        actor_id: u64,
        index: usize,
        ammo_empty: &crate::rules::AmmoEmpty,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let step_now = self.step_now;
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        actor.skills.preemptive_active = true;
        lock(&mut actor.skills.main);
        actor.motion.attacker = SkillSlot::Extra(index);
        lock(&mut actor.skills.extras[index].skill);
        for extra in &mut actor.skills.extras {
            if ammo_empty.incompatible.contains(&extra.rules.skill) {
                lock(&mut extra.skill);
            }
        }
        actor.melee_transition_at =
            Some(step_now + seconds_q32_to_steps(crate::rules::metres_q32(ammo_empty.transition)));
        // `MotionController.ChangeToStopState` for the transition. The main
        // skill's weapon, locked, turns to where the unit points and stays
        // there.
        actor.motion.state = MotionState::Stopped;
        let body = actor.body_rotation_q32;
        if let Some(weapon) = actor.skills.main.weapon_rotations_q32.first_mut() {
            *weapon = body;
        }
        let melee = actor.placement.effects.single.melee;
        if let Some(melee) = melee.filter(|melee| melee.recovers_life) {
            let actor = &self.actors[&actor_id];
            let max_life = actor.stats.max_life();
            if melee.recovery_ignores_disable {
                // `FightMech.ForceRecoveryLife`: `AddLife` of its maximum,
                // whatever holds its recovery off.
                self.force_add_life(actor_id, max_life, events)?;
            } else {
                self.add_life(actor_id, max_life, events)?;
            }
        }
        Ok(())
    }

    /// The `GRTimerManager` timers due on this step: each melee skill whose
    /// transition is over takes the main skill's place
    /// (`FinishActiveTransition`), its unit alive and the skill still set,
    /// and the melee mode writes its numbers
    /// (`OnMeleeModeTransitionComplete`): its rate on life, which keeps the
    /// unit's share of it, its speed, and its rate on damage, which reaches
    /// the main skill and every extra skill dealing a share of it.
    pub(in crate::fight) fn finish_melee_transitions(&mut self, step: u64) -> Result<()> {
        let due = self
            .actors
            .iter()
            .filter(|(_, actor)| actor.melee_transition_at.is_some_and(|at| at <= step))
            .map(|(&id, _)| id)
            .collect::<Vec<_>>();
        for actor_id in due {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.melee_transition_at = None;
            if !actor.alive() || !actor.skills.preemptive_active {
                continue;
            }
            let Some(index) = actor
                .skills
                .extras
                .iter()
                .position(|extra| extra.rules.preemptive.is_some())
            else {
                continue;
            };
            unlock(&mut actor.skills.extras[index].skill);
            if let Some(melee) = actor.placement.effects.single.melee {
                for (channel, entry) in melee_entries(melee) {
                    actor.stats.overlays.channel(channel).write(entry);
                }
                actor.melee_written = true;
                actor.refresh_life_data()?;
            }
            self.enter_motion_idle(actor_id);
        }
        Ok(())
    }
}

/// What tags a melee mode's entries.
const MELEE_SOURCE: &str = "melee mode";

/// What `OnMeleeModeTransitionComplete` writes: the rate on the unit's life
/// (`MechDataChangeFloatRate.LifeRate`), its speed
/// (`MechDataChangeInt.MoveSpeedValue`), and the rate on the main skill's
/// damage (`SkillDataModifier.AddData`, `DamageRate`).
fn melee_entries(melee: crate::modifier::MeleeMode) -> [(Channel, Entry); 3] {
    [
        (
            Channel::Unit,
            Entry {
                index: Index::MaxLife,
                source: MELEE_SOURCE,
                correction: super::super::tower::rate(melee.life_rate_q32),
            },
        ),
        (
            Channel::Unit,
            Entry {
                index: Index::MoveSpeed,
                source: MELEE_SOURCE,
                correction: Correction::Value(
                    melee.speed_value * crate::rules::SPACE_UNITS_PER_METER_SCALE,
                ),
            },
        ),
        (
            Channel::Skill,
            Entry {
                index: Index::AttackDamage,
                source: MELEE_SOURCE,
                correction: super::super::tower::rate(melee.damage_rate_q32),
            },
        ),
    ]
}

impl Actor {
    /// `MeleeModeEffectSystem.RemoveMeleeModeDataModifier` as the unit leaves
    /// the fight (`OnPermanentPreemptiveSkillDeactive`): what the melee mode
    /// wrote taken away, the life keeping its share.
    pub(in crate::fight) fn exit_melee_mode(&mut self) -> Result<()> {
        let Some(melee) = self.placement.effects.single.melee else {
            return Ok(());
        };
        if !std::mem::take(&mut self.melee_written) {
            return Ok(());
        }
        for (channel, entry) in melee_entries(melee) {
            self.stats.overlays.channel(channel).remove(entry);
        }
        self.refresh_life_data()
    }
}

/// `SkillLockState.Enter`: the skill lets its lock and what it fires at go,
/// and anything under way with them.
pub(in crate::fight) fn lock(skill: &mut Skill) {
    skill.drop_lock();
    skill.attack_target_left = None;
    skill.performer.stop();
    skill.idle = false;
    skill.enter(SkillState::Locked);
}

/// `ChangeToIdleState` out of `SkillLockState`: the skill idles, and
/// `SkillLockState.Exit` restarts its search timer.
pub(in crate::fight) fn unlock(skill: &mut Skill) {
    skill.enter(SkillState::Idle { ready_step: None });
    skill.search_target_time = SEARCH_TARGET_RESET_TICKS;
}
