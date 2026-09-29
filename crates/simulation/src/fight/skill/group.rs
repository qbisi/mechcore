use super::*;

/// Whether an idle sibling may start: a fusillade's
/// `CanStartAttackCheck` asks whether the core prepares or attacks, another
/// group's is `SkillGroup.IsAttacking`, any of its skills attacking.
fn may_start_group_slot(fusillade: bool, skill: &Skill) -> bool {
    if fusillade {
        matches!(
            skill.phase(),
            FightSkillPhase::Prepare { .. } | FightSkillPhase::Attack
        )
    } else {
        skill.phase() == FightSkillPhase::Attack
            || skill
                .siblings()
                .iter()
                .any(|slot| matches!(slot.state, SkillState::Attack(_)))
    }
}

impl Simulation {
    /// Asks each grouped slot which construction stands between the actor and
    /// the unit that slot was allocated.
    ///
    /// Every slot of a Wraith takes the block its core took, eight ticks later,
    /// when the group allocates its children. With one enemy unit the slots'
    /// lines are the core's, so whether several blocks in reach would be
    /// shared out among the slots is not something any recording has shown;
    /// the build's `CheckWallConstructionForGroupedSkill` keeps a list of walls
    /// already checked, which suggests it might, and nothing here assumes so.
    ///
    /// A slot's shield is its own `SearchAttackTarget`'s, which only that
    /// slot asks: `searching` names the slot, if one, whose search this is,
    /// and every other slot keeps the shield it last found, even one that
    /// has since broken.
    pub(in crate::fight) fn refresh_group_walls(
        &mut self,
        actor_id: u64,
        searching: Option<usize>,
    ) {
        let siblings = self.actors[&actor_id]
            .skill
            .siblings()
            .iter()
            .map(|slot| slot.lock_target)
            .collect::<Vec<_>>();
        let owner = FightActorRef::Unit(actor_id);
        let found = siblings
            .iter()
            .enumerate()
            .map(|(index, lock)| {
                let wall = lock.and_then(|lock| {
                    self.wall_in_the_way(owner, lock)
                        .map(|building| (building, lock))
                });
                // Each slot's `SearchTargetShield`, as the core's.
                if searching != Some(index + 1) {
                    return (wall, None, false);
                }
                let shield = if wall.is_none() {
                    lock.and_then(|lock| {
                        self.search_target_shield_in(
                            owner,
                            lock,
                            self.slot_attack_range(actor_id, Some(index + 1)),
                        )
                        .map(|shield| (shield, lock))
                    })
                } else {
                    None
                };
                (wall, shield, true)
            })
            .collect::<Vec<_>>();
        for (slot, (in_the_way, shield, searched)) in self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .skill
            .siblings_mut()
            .iter_mut()
            .zip(found)
        {
            slot.in_the_way = in_the_way;
            if searched {
                slot.target_shield = shield;
            }
        }
    }

    /// `SkillAttackableChecker.TrySearchGroupSkillLockTarget`, which an
    /// attacking sibling's check asks while its lock lives: whether the slot
    /// gives up a unit another skill of its own group holds, so that the
    /// check fails and the slot, idle, searches afresh on the next update.
    ///
    /// Every skill of the group must hold a lock and two must share one, and
    /// the slot must not hold its unit alone. Of the skills sharing a lock,
    /// less the slot and the core, the first that has struck fewer blows in
    /// its attack than the slot gives up instead, and failing one, the last
    /// that has struck as many; only when every other has struck more is it
    /// this slot, and only when its search timer is up. It then searches,
    /// resets the timer, and gives the unit up when the search finds one no
    /// skill of the group holds.
    pub(in crate::fight) fn sibling_yields(
        &mut self,
        actor_id: u64,
        slot: usize,
        order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<bool> {
        let skill = &self.actors[&actor_id].skill;
        let Some(locks) = skill.slot_locks().into_iter().collect::<Option<Vec<_>>>() else {
            return Ok(false);
        };
        // `attackInfos`: each lock with the skills holding it, in the order
        // the skills first name it.
        let mut holders: Vec<(FightActorRef, Vec<usize>)> = Vec::new();
        for (index, &lock) in locks.iter().enumerate() {
            match holders.iter_mut().find(|(held, _)| *held == lock) {
                Some((_, skills)) => skills.push(index),
                None => holders.push((lock, vec![index])),
            }
        }
        if holders.len() == locks.len()
            || holders
                .iter()
                .any(|(held, skills)| *held == locks[slot] && skills.len() == 1)
        {
            return Ok(false);
        }
        let others = holders
            .iter()
            .filter(|(_, skills)| skills.len() >= 2)
            .flat_map(|(_, skills)| skills.iter().copied())
            .filter(|&other| other != slot && other != 0)
            .collect::<Vec<_>>();
        let own = skill.sibling(slot).attack_count;
        let blows = |other: usize| skill.sibling(other).attack_count;
        if others.iter().any(|&other| blows(other) <= own)
            || skill.sibling(slot).search_target_time > 0
        {
            return Ok(false);
        }
        let found = self.select_group_lock_replacement(actor_id, slot, order)?;
        self.actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .skill
            .sibling_mut(slot)
            .search_target_time = SEARCH_TARGET_RESET_TICKS;
        Ok(found.is_some_and(|found| holders.iter().all(|(held, _)| *held != found)))
    }

    /// `SkillManager.Update` for a grouped unit's core's siblings, after the
    /// core: each slot is a `FightSkill` with its own state machine.
    ///
    /// An idle slot starts only while the group attacks
    /// (`GroupedSkillAttackBehaviour.CanStartAttackCheck` is
    /// `SkillGroup.IsAttacking`, any of its skills in `SkillAttackState`): it
    /// searches around what its siblings hold and prepares for the unit it
    /// finds, if that is in its attack area. A preparing or attacking slot
    /// asks `SkillAttackableChecker.Check` on every update and goes idle,
    /// dropping its lock, when the check fails, whatever its siblings do; the
    /// core leaving its attack leaves the others attacking. A slot enters
    /// its attack when its prepare is over and fires on the update after,
    /// then at its interval.
    ///
    /// A fusillade (`GroupedSkillFusilladeBehaviour`) holds its siblings to
    /// its core: a sibling starts only while the core prepares or attacks
    /// (`CanStartAttackCheck`), the core entering its attack hands every
    /// sibling its own schedule (`FusilladeStart`, `RefreshAttackData` with
    /// `isSync`), a sibling fires only on an update the core started a blow
    /// on (`CanPerformAttack`), and after such an update every sibling is
    /// due again when the core is (`FusilladeEnd`).
    pub(in crate::fight) fn update_group_slots(
        &mut self,
        actor_id: u64,
        step: u64,
        core_entered_attack: bool,
        body_rotation_q32: i64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        #[cfg(test)]
        self.replay_group_checker_calls(actor_id);
        let prepare_steps =
            native_time_units_to_steps(self.actors[&actor_id].rules.attack.prepare_time_units());
        let fusillade = self.actors[&actor_id].skill.fusillade();
        let cooling_steps =
            native_time_units_to_steps(self.actors[&actor_id].rules.attack.cooling_time_units());
        let core_blew = self.actors[&actor_id]
            .skill
            .group
            .as_ref()
            .is_some_and(|group| group.core_blow_step == Some(step));
        let skill = &mut self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .skill;
        if fusillade && core_entered_attack {
            skill.align_slots_to_core();
        }
        for slot in 1..self.actors[&actor_id].skill.group_size() {
            let before = self.actors[&actor_id].skill.sibling(slot).lock_target;
            match self.actors[&actor_id].skill.sibling(slot).state {
                SkillState::Idle { .. } => {
                    if may_start_group_slot(fusillade, &self.actors[&actor_id].skill) {
                        self.start_group_slot(
                            actor_id,
                            slot,
                            step,
                            prepare_steps,
                            target_search_order,
                        )?;
                    }
                }
                SkillState::Prepare { finish_step } => {
                    if !self.check_attackable_slot(
                        FightActorRef::Unit(actor_id),
                        Some(slot),
                        false,
                        target_search_order,
                    )? {
                        self.idle_group_slot(actor_id, slot);
                    } else if step >= finish_step {
                        let sibling = self
                            .actors
                            .get_mut(&actor_id)
                            .expect("actor identity is stable")
                            .skill
                            .sibling_mut(slot);
                        sibling.enter(SkillState::Attack(Blow::Waiting));
                        sibling.next_attack_step =
                            sibling.next_attack_step.max(step.saturating_add(1));
                    }
                }
                SkillState::Attack(_) => {
                    if !self.check_attackable_slot(
                        FightActorRef::Unit(actor_id),
                        Some(slot),
                        true,
                        target_search_order,
                    )? {
                        self.finish_group_slot(actor_id, slot, step, cooling_steps);
                    } else if (!fusillade || core_blew)
                        && step >= self.actors[&actor_id].skill.sibling(slot).next_attack_step
                        && let Some(target) = self.actors[&actor_id].skill.group_attack_target(slot)
                    {
                        self.release_group_slot(actor_id, slot, target, step, events)?;
                    }
                    // `SkillAttackState.Update` counts the search timer down
                    // after the check and the blow.
                    if let SkillState::Attack(_) = self.actors[&actor_id].skill.sibling(slot).state
                    {
                        self.actors
                            .get_mut(&actor_id)
                            .expect("actor identity is stable")
                            .skill
                            .sibling_mut(slot)
                            .search_target_time -= 1;
                    }
                }
                // `SkillCoolingState`: the weapon names what it last turned
                // to until the cooling's last step, which reads idle with the
                // weapon cleared.
                SkillState::Cooling { started, .. } => {
                    if step >= started.saturating_add(cooling_steps) {
                        self.actors
                            .get_mut(&actor_id)
                            .expect("actor identity is stable")
                            .skill
                            .sibling_mut(slot)
                            .enter(SkillState::Idle { ready_step: None });
                    }
                }
                SkillState::Reloading { .. } => {}
            }
            self.settle_group_slot(actor_id, slot, before, step, body_rotation_q32);
        }
        if fusillade && core_blew {
            self.actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .skill
                .align_slots_to_core();
        }
        Ok(())
    }

    /// What a sibling's update hands on: a lock it changed reaches the
    /// owner, and a weapon fixed to the body that holds a lock turns.
    fn settle_group_slot(
        &mut self,
        actor_id: u64,
        slot: usize,
        before: Option<FightActorRef>,
        step: u64,
        body_rotation_q32: i64,
    ) {
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let after = actor.skill.sibling(slot).lock_target;
        if after != before {
            actor.skill.set_mech_lock(after);
        }
        // `FightSkill.Update` turns a skill's weapons toward its lock
        // after its state has updated, and a weapon fixed to the body
        // takes the body's rotation instead: the rotation the body had
        // before this update turned it, as the body turns after its
        // skills.
        if actor.rules.attack.weapons.fixed_to_body
            && after.is_some()
            && self.stop_step != Some(step)
            && let Some(group) = &mut actor.skill.group
        {
            group.sibling_weapon_rotations_q32[slot - 1] = body_rotation_q32;
        }
    }

    /// A sibling whose attack check failed: `SkillAttackState.Finish` and
    /// `StopAttack`, its lock dropped, then its cooling, its weapon naming
    /// what the check turned it to. With no cooling it is idle at once.
    fn finish_group_slot(&mut self, actor_id: u64, slot: usize, step: u64, cooling_steps: u64) {
        // A slot firing at a shield has no attack target to go on naming:
        // `ChangeAttackTarget(null, shield)` cleared it.
        let skill = &self.actors[&actor_id].skill;
        let candidate = skill
            .group_attack_target(slot)
            .filter(|_| skill.group_skill(slot).shield_target().is_none());
        self.idle_group_slot(actor_id, slot);
        // `StopAttack` hands the owner the dropped lock.
        self.actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .skill
            .set_mech_lock(None);
        if cooling_steps > 0 {
            self.actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .skill
                .sibling_mut(slot)
                .enter(SkillState::Cooling {
                    started: step,
                    candidate,
                });
        }
    }

    /// `SkillIdleState.TryStartAttack` for a sibling: `PerformGroupedSkillSearch`
    /// around what the others hold, and the prepare, if the unit it finds is
    /// in its attack area.
    fn start_group_slot(
        &mut self,
        actor_id: u64,
        slot: usize,
        step: u64,
        prepare_steps: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<()> {
        // `SkillIdleState.TrySearchLockTarget` searches when the skill's
        // search timer is up or what it holds is gone, and holds the lock in
        // between.
        let held_alive = self.actors[&actor_id]
            .skill
            .sibling(slot)
            .lock_target
            .is_some_and(|lock| self.fight_actor_is_alive(lock));
        let sibling = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .skill
            .sibling_mut(slot);
        if held_alive && sibling.search_target_time > 0 {
            sibling.search_target_time -= 1;
        } else {
            let selected =
                self.select_group_lock_replacement(actor_id, slot, target_search_order)?;
            let sibling = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .skill
                .sibling_mut(slot);
            sibling.lock_target = selected;
            sibling.attack_target_left = None;
            sibling.search_target_time = SEARCH_TARGET_RESET_TICKS;
            self.refresh_group_walls(actor_id, Some(slot));
        }
        if let Some(target) = self.actors[&actor_id].skill.group_attack_target(slot)
            && self.slot_target_in_attack_area(FightActorRef::Unit(actor_id), Some(slot), target)
        {
            let sibling = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .skill
                .sibling_mut(slot);
            // `SkillIdleState.Exit` resets the search timer. A skill with no
            // prepare is attacking on the update it starts.
            sibling.search_target_time = SEARCH_TARGET_RESET_TICKS;
            let state = if prepare_steps == 0 {
                sibling.next_attack_step = sibling.next_attack_step.max(step.saturating_add(1));
                SkillState::Attack(Blow::Waiting)
            } else {
                SkillState::Prepare {
                    finish_step: step.saturating_add(prepare_steps),
                }
            };
            sibling.enter(state);
        }
        Ok(())
    }

    /// A sibling whose check failed: `SkillAttackState.Finish` and
    /// `StopAttack`, its lock dropped. The time its next blow is due is the
    /// skill's own and stays: a Wraith's slot that leaves its attack and
    /// comes back to it fires when its interval is up, not on its return.
    fn idle_group_slot(&mut self, actor_id: u64, slot: usize) {
        let sibling = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .skill
            .sibling_mut(slot);
        *sibling = Skill {
            next_attack_step: sibling.next_attack_step,
            ..Skill::sibling_entering(sibling.kind)
        };
    }

    pub(in crate::fight) fn refresh_group_skill_attack_interval(
        &mut self,
        actor_id: u64,
        skill_index: usize,
        step: u64,
    ) -> Result<()> {
        // What a recording reads as the unit's current interval is its core
        // skill's, so a sibling's draw does not replace it.
        let core_interval = self.actors[&actor_id].skill.current_attack_interval;
        let sampled_step = self.sample_actor_attack_interval(actor_id, step)?;
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("group skill owner identity is stable");
        actor.skill.current_attack_interval = core_interval;
        actor.skill.sibling_mut(skill_index).next_attack_step = sampled_step;
        Ok(())
    }

    /// A grouped skill's core's blow, once its interval is up; never on the
    /// update its attack state was entered.
    pub(in crate::fight) fn perform_group_blows(
        &mut self,
        actor_id: u64,
        step: u64,
        entered_attack: bool,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let group_core_target = {
            let actor = &self.actors[&actor_id];
            (actor.skill.is_grouped()
                && !entered_attack
                && actor.motion.state == MotionState::Attacking
                && !actor.motion.attack_hold_fire
                && actor.skill.pending().is_none()
                && actor.skill.backswing_finish_step().is_none()
                && actor.skill.phase() == FightSkillPhase::Attack
                && step >= actor.skill.next_attack_step)
                .then(|| actor.skill.attack_target())
                .flatten()
        };
        if let Some(target_id) = group_core_target
            && self.target_in_attack_area(FightActorRef::Unit(actor_id), target_id)
        {
            let next_attack_step = self.sample_actor_attack_interval(actor_id, step)?;
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.skill.next_attack_step = next_attack_step;
            if let Some(group) = &mut actor.skill.group {
                group.core_blow_step = Some(step);
            }
            actor.skill.set_pending(Some(PendingRelease {
                step,
                target: target_id,
            }));
            let _attack_point_rejected = self.release(FightActorRef::Unit(actor_id), events)?;
        }
        Ok(())
    }

    /// A sibling's blow at what it fires at now: the unit it holds, or a
    /// construction in its way.
    fn release_group_slot(
        &mut self,
        actor_id: u64,
        skill_index: usize,
        target: FightActorRef,
        step: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        match target {
            FightActorRef::Unit(target_id)
                if self.actors.get(&target_id).is_some_and(Actor::alive) =>
            {
                self.refresh_group_skill_attack_interval(actor_id, skill_index, step)?;
                if self.actors[&actor_id].skill.kind == SkillKind::Strike {
                    self.direct_effect(actor_id, target, skill_index, events)?;
                } else {
                    self.release_projectile(
                        FightActorRef::Unit(actor_id),
                        target_id,
                        skill_index,
                        skill_index,
                        events,
                    )?;
                }
            }
            // A slot whose line of fire a construction stands in fires at
            // the construction, as the core does.
            FightActorRef::Building(building_id) => {
                let Some((x_q32, z_q32, radius)) = self
                    .buildings
                    .iter()
                    .find(|building| {
                        building.building_id == building_id && building_alive(building)
                    })
                    .map(|building| {
                        (
                            building.position.x,
                            building.position.z,
                            building_radius(building),
                        )
                    })
                else {
                    return Ok(());
                };
                self.refresh_group_skill_attack_interval(actor_id, skill_index, step)?;
                self.release_projectile_to(
                    FightActorRef::Unit(actor_id),
                    ObjectKind::Building,
                    building_id,
                    q32_to_space_rounded(x_q32),
                    0,
                    q32_to_space_rounded(z_q32),
                    x_q32,
                    z_q32,
                    radius,
                    skill_index,
                    skill_index,
                    events,
                )?;
            }
            FightActorRef::Unit(_) => return Ok(()),
        }
        // `SkillAttackController.PerformAttack` counts the blow.
        self.actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .skill
            .sibling_mut(skill_index)
            .attack_count += 1;
        Ok(())
    }
}
