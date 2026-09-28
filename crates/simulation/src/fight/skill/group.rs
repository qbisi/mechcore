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
        let slots = self.actors[&actor_id].skill.slot_locks();
        if slots.is_empty() {
            return;
        }
        let found = slots
            .iter()
            .map(|slot| {
                slot.and_then(|unit| {
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

    /// The ordinary grouped search is closed here; the separate checker
    /// branch that redistributes live shared locks by attack count is not.
    /// Refuse when that branch could acquire an unheld in-range target,
    /// rather than silently keep the shared lock. A saturated group (the
    /// two-target fixture) has no such alternative and keeps its holdings.
    pub(in crate::fight) fn check_group_redistribution_scope(
        &self,
        actor_id: u64,
        slot: usize,
        order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<()> {
        let slots = &self.actors[&actor_id].skill.slot_locks();
        if slots.iter().any(Option::is_none)
            || slots
                .iter()
                .filter(|target| **target == slots[slot])
                .count()
                < 2
        {
            return Ok(());
        }
        if let Some(FightActorRef::Unit(candidate)) =
            self.select_group_lock_replacement(actor_id, slot, order)?
            && !slots.contains(&Some(candidate))
            && self.slot_target_in_attack_range(
                FightActorRef::Unit(actor_id),
                Some(slot),
                FightActorRef::Unit(candidate),
            )
        {
            return Err(Error::new(
                "grouped live-lock redistribution by attack count is not supported",
            ));
        }
        Ok(())
    }

    pub(in crate::fight) fn update_group_skill_targets(
        &mut self,
        actor_id: u64,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<()> {
        #[cfg(test)]
        self.replay_group_checker_calls(actor_id);
        let actor = &self.actors[&actor_id];
        if actor.rules.attack.weapons.mode != WeaponMode::Group
            || actor.motion.state != MotionState::Attacking
        {
            return Ok(());
        }
        let ready = actor.skill.phase() == FightSkillPhase::Attack
            || matches!(actor.skill.phase(), FightSkillPhase::Prepare { finish_step }
                if finish_step <= step.saturating_add(1));
        if !ready {
            return Ok(());
        }
        let count = actor.skill.slots.len();
        let prepare_steps = native_time_units_to_steps(actor.rules.attack.prepare_time_units());
        if actor.skill.slots.iter().all(|slot| slot.lock.is_none()) {
            let core = actor.skill.lock_target.and_then(FightActorRef::unit_id);
            self.actors
                .get_mut(&actor_id)
                .expect("actor exists")
                .skill
                .slots[0]
                .lock = core;
        }
        for slot in 0..count {
            let before = self.actors[&actor_id].skill.slots[slot].lock;
            if self.check_attackable_slot(
                FightActorRef::Unit(actor_id),
                Some(slot),
                target_search_order,
            )? {
                let actor = self.actors.get_mut(&actor_id).expect("actor exists");
                if slot != 0 && before.is_none() {
                    let target = actor
                        .skill
                        .group_attack_target(slot)
                        .expect("checked target");
                    actor.skill.slots[slot]
                        .pending_releases
                        .push(PendingRelease {
                            step: step.saturating_add(prepare_steps).saturating_add(1),
                            target,
                        });
                }
            } else {
                return Err(Error::new(
                    "grouped slot leaving its attack area is not supported",
                ));
            }
        }
        Ok(())
    }

    pub(in crate::fight) fn refresh_group_skill_attack_interval(
        &mut self,
        actor_id: u64,
        skill_index: usize,
        step: u64,
    ) -> Result<()> {
        // What a recording reads as the unit's current interval is its core
        // skill's, so a child slot's draw does not replace it.
        let core_interval = self.actors[&actor_id].skill.current_attack_interval;
        let sampled_step = self.sample_actor_attack_interval(actor_id, step)?;
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("group skill owner identity is stable");
        if skill_index != 0 {
            actor.skill.current_attack_interval = core_interval;
        }
        actor
            .skill
            .slots
            .get_mut(skill_index)
            .ok_or_else(|| Error::new("group skill index is absent"))?
            .next_attack_step = sampled_step;
        Ok(())
    }

    /// A grouped skill's blows: the core's, once its interval and prepare are
    /// up, and each slot's release that is due.
    pub(in crate::fight) fn perform_group_blows(
        &mut self,
        actor_id: u64,
        step: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let group_core_target = {
            let actor = &self.actors[&actor_id];
            (actor.rules.attack.weapons.mode == WeaponMode::Group
                && actor.motion.state == MotionState::Attacking
                && !actor.motion.attack_hold_fire
                && actor.skill.pending().is_none()
                && actor.skill.backswing_finish_step().is_none()
                && actor.skill.phase() == FightSkillPhase::Attack
                && step >= actor.skill.next_attack_step)
                .then(|| actor.skill.mechanical_attack_target())
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
        self.release_group_slots(actor_id, step, events)
    }

    /// Each slot's release that is due, fired at what the slot fires at now.
    fn release_group_slots(
        &mut self,
        actor_id: u64,
        step: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let group_releases = {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            // Each slot's queued releases that are due, in the order queued,
            // then the blow its interval has brought round.
            let mut due = Vec::new();
            for (skill_index, slot) in actor.skill.slots.iter_mut().enumerate() {
                slot.pending_releases.retain(|&pending| {
                    if pending.step <= step {
                        due.push((skill_index, pending));
                        false
                    } else {
                        true
                    }
                });
                if skill_index > 0
                    && slot.next_attack_step > 0
                    && slot.next_attack_step <= step
                    && let Some(target) = slot.attack_target()
                {
                    due.push((skill_index, PendingRelease { step, target }));
                }
            }
            // A release queued when its slot was allocated names the unit it
            // was allocated. It fires at what that slot fires at now, which is
            // a construction in its way if one has been found since: the slot
            // still holds the same unit, and only what it shoots has changed.
            for (skill_index, pending) in &mut due {
                let slot = &actor.skill.slots[*skill_index];
                if pending.target.unit_id() == slot.lock
                    && let Some(target) = slot.attack_target()
                {
                    pending.target = target;
                }
            }
            due
        };
        for (skill_index, pending) in group_releases {
            match pending.target {
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
                        continue;
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
        }
        Ok(())
    }
}
