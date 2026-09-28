use super::*;

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
    pub(in crate::fight) fn refresh_group_walls(&mut self, actor_id: u64) {
        let siblings = self.actors[&actor_id]
            .skill
            .slots
            .iter()
            .map(|slot| slot.lock)
            .collect::<Vec<_>>();
        let found = siblings
            .iter()
            .map(|lock| {
                lock.and_then(|unit| {
                    self.wall_in_the_way(FightActorRef::Unit(actor_id), FightActorRef::Unit(unit))
                        .map(|building| (building, unit))
                })
            })
            .collect::<Vec<_>>();
        for (slot, in_the_way) in self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .skill
            .slots
            .iter_mut()
            .zip(found)
        {
            slot.in_the_way = in_the_way;
        }
    }

    /// `SkillAttackableChecker.TrySearchGroupSkillLockTarget`, which an
    /// attacking sibling's check asks on every update while its lock lives:
    /// whether the slot gives up a unit another skill of its group also
    /// holds, so that the check fails and the slot, idle, searches afresh on
    /// the next update. It does when the core took that very unit earlier in
    /// the same update, no other sibling holds it, and a unit no slot holds
    /// stands in the sibling's reach. A unit shared from before is kept, and
    /// so is one more siblings hold, and one with nothing else in reach. A
    /// sibling that took the unit earlier in the update has
    /// not been recorded doing so, and is refused by name; the build weighs
    /// the holders by attack counts, which is not read.
    pub(in crate::fight) fn sibling_yields(
        &self,
        actor_id: u64,
        slot: usize,
        order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<bool> {
        let skill = &self.actors[&actor_id].skill;
        let slots = skill.slot_locks();
        let held = slots[slot];
        // The holders that took the unit on this update: the core, which
        // updates first, and siblings before this one.
        let taken_now = (0..slots.len())
            .filter(|&other| other != slot && slots[other] == held)
            .filter(|&other| {
                if other == 0 {
                    skill.lock_written
                } else {
                    skill.sibling(other).lock_written
                }
            })
            .collect::<Vec<_>>();
        if taken_now.is_empty() {
            return Ok(false);
        }
        // A unit more of the group holds than the core and this slot is
        // kept: the Wraith's M3 with seed 1787720817 at tick 178, its core
        // taking the Crawler three of its siblings hold, two of them still
        // preparing, with two free Crawlers in reach.
        let holders = (0..slots.len())
            .filter(|&other| other != slot && slots[other] == held)
            .count();
        if taken_now == [0] && holders > 1 {
            return Ok(false);
        }
        let Some(FightActorRef::Unit(candidate)) =
            self.select_group_lock_replacement(actor_id, slot, order)?
        else {
            return Ok(false);
        };
        if slots.contains(&Some(candidate))
            || !self.slot_target_in_attack_range(
                FightActorRef::Unit(actor_id),
                Some(slot),
                FightActorRef::Unit(candidate),
            )
        {
            return Ok(false);
        }
        if taken_now != [0] {
            return Err(Error::new(
                "a sibling giving up a unit another sibling took on the same update is not supported",
            ));
        }
        Ok(true)
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
    pub(in crate::fight) fn update_group_slots(
        &mut self,
        actor_id: u64,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        #[cfg(test)]
        self.replay_group_checker_calls(actor_id);
        let prepare_steps =
            native_time_units_to_steps(self.actors[&actor_id].rules.attack.prepare_time_units());
        for sibling in &mut self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .skill
            .slots
        {
            sibling.lock_written = false;
        }
        for slot in 1..self.actors[&actor_id].skill.group_size {
            let before = self.actors[&actor_id].skill.sibling(slot).lock;
            match self.actors[&actor_id].skill.sibling(slot).state {
                SkillState::Idle { .. } => {
                    if self.group_attacking(actor_id) {
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
                        sibling.state = SkillState::Attack(Blow::Waiting);
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
                        self.idle_group_slot(actor_id, slot);
                    } else if step >= self.actors[&actor_id].skill.sibling(slot).next_attack_step
                        && let Some(target) = self.actors[&actor_id].skill.group_attack_target(slot)
                    {
                        self.release_group_slot(actor_id, slot, target, step, events)?;
                    }
                }
                SkillState::Cooling { .. } | SkillState::Reloading { .. } => {}
            }
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            let after = actor.skill.sibling(slot).lock;
            if after != before {
                actor.skill.mech_lock = after.map(FightActorRef::Unit);
            }
        }
        Ok(())
    }

    /// `SkillGroup.IsAttacking`: any skill of the group in its attack state.
    fn group_attacking(&self, actor_id: u64) -> bool {
        let skill = &self.actors[&actor_id].skill;
        skill.phase() == FightSkillPhase::Attack
            || skill
                .slots
                .iter()
                .any(|slot| matches!(slot.state, SkillState::Attack(_)))
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
        let selected = self.select_group_lock_replacement(actor_id, slot, target_search_order)?;
        let sibling = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .skill
            .sibling_mut(slot);
        sibling.lock = selected.and_then(FightActorRef::unit_id);
        sibling.lock_written = true;
        self.refresh_group_walls(actor_id);
        if let Some(target) = self.actors[&actor_id].skill.group_attack_target(slot)
            && self.slot_target_in_attack_range(FightActorRef::Unit(actor_id), Some(slot), target)
        {
            self.actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .skill
                .sibling_mut(slot)
                .state = SkillState::Prepare {
                finish_step: step.saturating_add(prepare_steps),
            };
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
        *sibling = SlotSkill {
            next_attack_step: sibling.next_attack_step,
            ..SlotSkill::default()
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
            (actor.rules.attack.weapons.mode == WeaponMode::Group
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
                self.release_projectile(
                    FightActorRef::Unit(actor_id),
                    target_id,
                    skill_index,
                    skill_index,
                    events,
                )?;
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
            FightActorRef::Unit(_) => {}
        }
        Ok(())
    }
}
