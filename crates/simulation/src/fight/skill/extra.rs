//! The skills an extra weapon technology adds beside a unit's main one
//! (`ExtraWeaponTech`, `ExtraSkillSystem.AddMech`).
//!
//! `SkillManager.Update` runs every skill of the unit, the main one among
//! them, in ascending skill ID, each through the one skill machine. What sets
//! an extra skill apart is what its `FightSkill.Init` makes of
//! `isMainSkill` false and what its owner leaves it: it searches as it
//! updates (`SkillSearchTargetController.PrepareSearch` does nothing), hands
//! its owner no lock (`FightSkillBase.IsMainSearcher`), so the motion follows
//! the main skill alone, and starts and performs its attack from its own
//! idle and attack states, as a construction's skill does.

use super::{group::held_within_arc, *};

impl Simulation {
    /// The extra skills of a unit that update before or after its main one:
    /// `SkillManager.SortSkills` orders a unit's skills by ID.
    pub(in crate::fight) fn extra_skills_around_main(
        &self,
        actor_id: u64,
    ) -> (Vec<usize>, Vec<usize>) {
        let actor = &self.actors[&actor_id];
        let main = actor.rules.main_skill;
        (0..actor.skills.extras.len())
            .partition(|&index| actor.skills.extras[index].rules.skill < main)
    }

    /// Some of a unit's extra skills' updates, in their order.
    pub(in crate::fight) fn step_extra_skills(
        &mut self,
        actor_id: u64,
        indices: &[usize],
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        for &index in indices {
            self.step_extra_skill(actor_id, index, step, target_search_order, events)?;
        }
        Ok(())
    }

    /// One extra skill's update: `FightSkill.Update`, the state machine, the
    /// attack it starts and performs itself, and its weapon turning towards
    /// its lock after the state has updated.
    pub(in crate::fight) fn step_extra_skill(
        &mut self,
        actor_id: u64,
        index: usize,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let skill_ref = SkillRef {
            owner: FightActorRef::Unit(actor_id),
            slot: SkillSlot::Extra(index),
        };
        // `SkillLockState` does not update.
        if self.skill(skill_ref).state == SkillState::Locked {
            return Ok(());
        }
        let parent = self.actors[&actor_id].body_rotation_q32;
        self.skill_mut(skill_ref).lock_written = false;
        let was_idle = matches!(self.skill(skill_ref).state, SkillState::Idle { .. });
        let was_attacking = self.skill(skill_ref).phase() == FightSkillPhase::Attack;
        let attack_state_before = matches!(self.skill(skill_ref).state, SkillState::Attack(_));
        if let Some(update) = self.update_skill(skill_ref, step, target_search_order, events)? {
            self.attack_in_reach(skill_ref, step, update, events)?;
        }
        // `SkillAttackState.Update` counts the search timer down after the
        // check and the blow, which a skill of a group reads to give its
        // lock up, as its siblings' do; a state is not updated on the update
        // it is entered.
        if self.skill(skill_ref).is_grouped()
            && attack_state_before
            && matches!(self.skill(skill_ref).state, SkillState::Attack(_))
        {
            self.skill_mut(skill_ref).search_target_time -= 1;
        }
        // The skills of its group update after it, each its own
        // `FightSkill`, in the order of their IDs.
        if self.skill(skill_ref).is_grouped() && self.actors[&actor_id].alive() {
            let core_entered_attack =
                !was_attacking && self.skill(skill_ref).phase() == FightSkillPhase::Attack;
            self.update_group_slots(
                skill_ref,
                step,
                core_entered_attack,
                parent,
                target_search_order,
                events,
            )?;
        }
        // A preemptive skill's state behaviours: leaving its idle state it
        // takes the main skill's place, and entering it hands it back.
        if matches!(
            self.skill(skill_ref).kind,
            SkillKind::Around | SkillKind::Support
        ) {
            let is_idle = matches!(self.skill(skill_ref).state, SkillState::Idle { .. });
            if was_idle && !is_idle {
                self.preemptive_leaves_idle(skill_ref, target_search_order)?;
            } else if !was_idle && is_idle {
                self.preemptive_enters_idle(actor_id);
            }
        }
        if !self.actors[&actor_id].alive() {
            return Ok(());
        }
        self.turn_extra_weapon(actor_id, index, parent);
        Ok(())
    }

    /// `RotationLimitFightTransform.RotateTo`: an extra skill's weapon turns
    /// towards its lock at the skill's weapon rotation speed, held within its
    /// arc about what it is mounted on, as that pointed before the motion
    /// turned it. A skill without a lock leaves it where it points, and a
    /// weapon without an arc has no transform of its own to turn.
    fn turn_extra_weapon(&mut self, actor_id: u64, index: usize, chassis: i64) {
        if self.ending.stop_step.is_some()
            || !self.actors[&actor_id].skills.extras[index].own_transform()
        {
            return;
        }
        let actor = &self.actors[&actor_id];
        let extra = &actor.skills.extras[index];
        let Some(view) = extra
            .skill
            .lock_target
            .and_then(|target| self.fight_actor(target))
        else {
            return;
        };
        let weapons = &extra.rules.attack.weapons;
        let turn = weapons.rotation_speed_mdeg_per_second().map_or_else(
            || actor.turn_q32(),
            |speed| q32_mul(mdeg_to_degrees_q32(speed), NATIVE_LOGIC_DELTA_Q32),
        );
        let parent = match (weapons.mount, actor.turret_rotation()) {
            (WeaponMount::MechBody, Some(turret)) => turret,
            _ => chassis,
        };
        let arcs = (0..extra.skill.weapon_rotations_q32.len())
            .map(|offset| extra.arc(offset).copied())
            .collect::<Vec<_>>();
        let bearing = direction_degrees_q32_raw(
            view.x_q32.saturating_sub(actor.x_q32),
            view.z_q32.saturating_sub(actor.z_q32),
        );
        let skill = &mut self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .skills
            .extras[index]
            .skill;
        // `FightSkill.RotateWeaponTo` turns each of the skill's weapons.
        for (rotation, arc) in skill.weapon_rotations_q32.iter_mut().zip(arcs) {
            let turned = rotate_towards_q32(*rotation, bearing, turn);
            *rotation = arc.map_or(turned, |arc| held_within_arc(turned, parent, &arc));
        }
    }
}
