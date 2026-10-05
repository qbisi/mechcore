use super::*;

/// Whether an idle sibling may start: a fusillade's
/// `CanStartAttackCheck` asks whether the core prepares or attacks, another
/// group's is `SkillGroup.IsAttacking`, any of its skills attacking.
fn may_start_group_slot(fusillade: bool, skill: &Skill) -> bool {
    if skill.standalone() {
        return true;
    }
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
    /// A slot's wall and shield are its own `SearchAttackTarget`'s, which
    /// only that slot asks: `searching` names the slot, if one, whose search
    /// this is, and every other slot keeps the wall and the shield it last
    /// found, even ones that have since fallen or broken. A Wraith's gun
    /// whose block another gun fells goes on naming the block until its own
    /// check ends its attack.
    pub(in crate::fight) fn refresh_group_walls(
        &mut self,
        skill_ref: SkillRef,
        searching: Option<usize>,
    ) {
        let siblings = self
            .skill(skill_ref)
            .siblings()
            .iter()
            .map(|slot| (slot.lock_target, slot.in_the_way))
            .collect::<Vec<_>>();
        let owner = skill_ref.owner;
        let found = siblings
            .iter()
            .enumerate()
            .map(|(index, &(lock, kept))| {
                if searching.is_some_and(|slot| slot != index + 1) {
                    return (kept, None, false);
                }
                let wall = lock.and_then(|lock| {
                    self.wall_in_the_way(skill_ref, Some(index + 1), lock)
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
                            self.slot_attack_range(skill_ref, Some(index + 1)),
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
            .skill_mut(skill_ref)
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
    /// less the slot and the main skill's first (`ISkillOwner.GetMainSkill`,
    /// `GetSkills`), which only the main skill's own group holds as its
    /// core, the first that has struck fewer blows in
    /// its attack than the slot gives up instead, and failing one, the last
    /// that has struck as many; only when every other has struck more is it
    /// this slot, and only when its search timer is up. It then searches,
    /// resets the timer, and gives the unit up when the search finds one no
    /// skill of the group holds.
    pub(in crate::fight) fn sibling_yields(
        &mut self,
        skill_ref: SkillRef,
        slot: usize,
        order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<bool> {
        let skill = self.skill(skill_ref);
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
            .filter(|&other| other != slot && (other != 0 || skill_ref.slot != SkillSlot::Main))
            .collect::<Vec<_>>();
        let own = skill.group_skill(slot).attack_count;
        let blows = |other: usize| skill.group_skill(other).attack_count;
        if others.iter().any(|&other| blows(other) <= own)
            || skill.group_skill(slot).search_target_time > 0
        {
            return Ok(false);
        }
        let found = self.select_group_lock_replacement(skill_ref, slot, order)?;
        self.skill_mut(skill_ref)
            .group_skill_mut(slot)
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
        skill_ref: SkillRef,
        step: u64,
        core_entered_attack: bool,
        body_rotation_q32: i64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        #[cfg(test)]
        if let Some(actor_id) = skill_ref.owner.unit_id() {
            self.replay_group_checker_calls(actor_id);
        }
        let main = skill_ref;
        let prepare_steps = native_time_units_to_steps(self.skill_rules(main).prepare_time_units());
        let fusillade = self.skill(main).fusillade();
        let cooling_steps = native_time_units_to_steps(self.skill_rules(main).cooling_time_units());
        let core_blew = self
            .skill(main)
            .group
            .as_ref()
            .is_some_and(|group| group.core_blow_step == Some(step));
        if fusillade && core_entered_attack {
            self.skill_mut(main).align_slots_to_core();
        }
        for slot in 1..self.skill(main).group_size() {
            let before = self.skill(main).sibling(slot).lock_target;
            match self.skill(main).sibling(slot).state {
                SkillState::Idle { .. } => {
                    if may_start_group_slot(fusillade, self.skill(main)) {
                        self.start_group_slot(
                            main,
                            slot,
                            step,
                            prepare_steps,
                            target_search_order,
                        )?;
                    }
                }
                SkillState::Prepare { finish_step } => {
                    if !self.check_attackable_slot(main, Some(slot), false, target_search_order)? {
                        self.idle_group_slot(main, slot);
                    } else if step >= finish_step {
                        let sibling = self.skill_mut(main).sibling_mut(slot);
                        sibling.enter(SkillState::Attack(Blow::Waiting));
                        sibling.next_attack_step =
                            sibling.next_attack_step.max(step.saturating_add(1));
                    }
                }
                SkillState::Attack(_) => {
                    if self.attacks_fallen_construction(main, Some(slot))
                        || !self.check_attackable_slot(
                            main,
                            Some(slot),
                            true,
                            target_search_order,
                        )?
                    {
                        self.finish_group_slot(main, slot, step, cooling_steps);
                    } else if (!fusillade || core_blew)
                        && step >= self.skill(main).sibling(slot).next_attack_step
                        && let Some(target) = self.skill(main).group_attack_target(slot)
                    {
                        self.release_group_slot(main, slot, target, step, events)?;
                    }
                    // `SkillAttackState.Update` counts the search timer down
                    // after the check and the blow.
                    if let SkillState::Attack(_) = self.skill(main).sibling(slot).state {
                        self.skill_mut(main).sibling_mut(slot).search_target_time -= 1;
                    }
                }
                // `SkillCoolingState`: the weapon names what it last turned
                // to until the cooling's last step, which reads idle with the
                // weapon cleared.
                SkillState::Cooling { started, .. } => {
                    if step >= started.saturating_add(cooling_steps) {
                        self.skill_mut(main)
                            .sibling_mut(slot)
                            .enter(SkillState::Idle { ready_step: None });
                    }
                }
                SkillState::Reloading { .. } | SkillState::Locked => {}
            }
            self.settle_group_slot(main, slot, before, step, body_rotation_q32);
        }
        if fusillade && core_blew {
            self.skill_mut(main).align_slots_to_core();
        }
        Ok(())
    }

    /// What a sibling's update hands on: a lock it changed reaches the
    /// owner, a main searcher's (`FightSkillBase.IsMainSearcher`), and a
    /// weapon fixed to the body that holds a lock turns.
    fn settle_group_slot(
        &mut self,
        skill_ref: SkillRef,
        slot: usize,
        before: Option<FightActorRef>,
        step: u64,
        body_rotation_q32: i64,
    ) {
        let fixed_to_body = self.skill_rules(skill_ref).weapons.fixed_to_body;
        let stopping = self.ending.stop_step == Some(step);
        // A joined row's slot is its row's skill, which is no main searcher
        // (`FightSkillBase.IsMainSearcher` asks `isMainSkill`): its lock
        // does not reach the owner.
        let main_searcher =
            skill_ref.slot == SkillSlot::Main && self.skill(skill_ref).joined_range(slot).is_none();
        let skill = self.skill_mut(skill_ref);
        let after = skill.sibling(slot).lock_target;
        if after != before && main_searcher {
            skill.set_mech_lock(after);
        }
        // `FightSkill.Update` turns a skill's weapons toward its lock
        // after its state has updated, and a weapon fixed to the body
        // takes the body's rotation instead: the rotation the body had
        // before this update turned it, as the body turns after its
        // skills.
        if fixed_to_body
            && after.is_some()
            && !stopping
            && let Some(group) = &mut skill.group
        {
            group.sibling_weapon_rotations_q32[slot - 1] = body_rotation_q32;
        }
    }

    /// A sibling whose attack check failed: `SkillAttackState.Finish` and
    /// `StopAttack`, its lock dropped, then its cooling, its weapon naming
    /// what the check turned it to. With no cooling it is idle at once.
    fn finish_group_slot(
        &mut self,
        skill_ref: SkillRef,
        slot: usize,
        step: u64,
        cooling_steps: u64,
    ) {
        // A slot firing at a shield has no attack target to go on naming:
        // `ChangeAttackTarget(null, shield)` cleared it.
        let skill = self.skill(skill_ref);
        let candidate = skill
            .group_skill(slot)
            .checked_attack_target()
            .filter(|_| skill.group_skill(slot).shield_target().is_none());
        self.idle_group_slot(skill_ref, slot);
        // `StopAttack` hands a main searcher's owner the dropped lock, which
        // a joined row's slot is not.
        if skill_ref.slot == SkillSlot::Main && self.skill(skill_ref).joined_range(slot).is_none() {
            self.skill_mut(skill_ref).set_mech_lock(None);
        }
        if cooling_steps > 0 {
            self.skill_mut(skill_ref)
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
        skill_ref: SkillRef,
        slot: usize,
        step: u64,
        prepare_steps: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<()> {
        // `SkillIdleState.TrySearchLockTarget` searches when the skill's
        // search timer is up or what it holds is gone, and holds the lock in
        // between.
        let held_alive = self
            .skill(skill_ref)
            .sibling(slot)
            .lock_target
            .is_some_and(|lock| self.fight_actor_is_alive(lock));
        let sibling = self.skill_mut(skill_ref).sibling_mut(slot);
        if held_alive && sibling.search_target_time > 0 {
            sibling.search_target_time -= 1;
        } else {
            let selected =
                self.select_group_lock_replacement(skill_ref, slot, target_search_order)?;
            let idle = selected.is_none();
            let selected = if idle {
                self.select_alive_target(skill_ref, Some(slot), target_search_order)?
            } else {
                selected
            };
            let sibling = self.skill_mut(skill_ref).sibling_mut(slot);
            sibling.idle = idle;
            sibling.lock_target = selected;
            sibling.attack_target_left = None;
            sibling.search_target_time = SEARCH_TARGET_RESET_TICKS;
            self.refresh_group_walls(skill_ref, Some(slot));
            self.hand_standalone_motion(skill_ref, slot);
        }
        self.try_start_group_slot(skill_ref, slot, step, prepare_steps);
        Ok(())
    }

    /// The start itself: a slot whose target is in its attack area prepares,
    /// or with no prepare is attacking and fires on the next update.
    fn try_start_group_slot(
        &mut self,
        skill_ref: SkillRef,
        slot: usize,
        step: u64,
        prepare_steps: u64,
    ) {
        if let Some(target) = self.skill(skill_ref).group_attack_target(slot)
            && self.slot_target_in_attack_area(skill_ref, Some(slot), target)
        {
            let sibling = self.skill_mut(skill_ref).sibling_mut(slot);
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
    }

    /// The standalone weapons' skills starting with the unit's main skill, as
    /// its motion comes into its attack: each is a main skill of its own,
    /// and starts in the same update.
    pub(in crate::fight) fn start_standalone_slots(&mut self, actor_id: u64, step: u64) {
        if !self.actors[&actor_id].skills.main.standalone() {
            return;
        }
        let prepare_steps =
            native_time_units_to_steps(self.actors[&actor_id].rules.attack.prepare_time_units());
        for slot in 1..self.actors[&actor_id].skills.main.group_size() {
            if matches!(
                self.actors[&actor_id].skills.main.sibling(slot).state,
                SkillState::Idle { .. }
            ) {
                self.try_start_group_slot(
                    SkillRef::main(FightActorRef::Unit(actor_id)),
                    slot,
                    step,
                    prepare_steps,
                );
            }
        }
    }

    /// A sibling whose check failed: `SkillAttackState.Finish` and
    /// `StopAttack`, its lock dropped. The time its next blow is due is the
    /// skill's own and stays: a Wraith's slot that leaves its attack and
    /// comes back to it fires when its interval is up, not on its return.
    fn idle_group_slot(&mut self, skill_ref: SkillRef, slot: usize) {
        let sibling = self.skill_mut(skill_ref).sibling_mut(slot);
        *sibling = Skill {
            next_attack_step: sibling.next_attack_step,
            ..Skill::sibling_entering(sibling.kind)
        };
    }

    pub(in crate::fight) fn refresh_group_skill_attack_interval(
        &mut self,
        skill_ref: SkillRef,
        skill_index: usize,
        step: u64,
    ) -> Result<()> {
        // What a recording reads as the unit's current interval is its core
        // skill's, so a sibling's draw does not replace it.
        let core_interval = self.skill(skill_ref).current_attack_interval;
        let sampled_step = self.sample_attack_interval(skill_ref, step)?;
        let skill = self.skill_mut(skill_ref);
        skill.current_attack_interval = core_interval;
        skill.sibling_mut(skill_index).next_attack_step = sampled_step;
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
            (actor.skills.main.is_grouped()
                && !entered_attack
                && actor.motion.state == MotionState::Attacking
                && !actor.motion.attack_hold_fire
                && actor.skills.main.pending().is_none()
                && actor.skills.main.backswing_finish_step().is_none()
                && actor.skills.main.phase() == FightSkillPhase::Attack
                && step >= actor.skills.main.next_attack_step)
                .then(|| actor.skills.main.attack_target())
                .flatten()
        };
        if let Some(target_id) = group_core_target
            && self.target_in_attack_area(SkillRef::main(FightActorRef::Unit(actor_id)), target_id)
        {
            let next_attack_step =
                self.sample_attack_interval(SkillRef::main(FightActorRef::Unit(actor_id)), step)?;
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.skills.main.next_attack_step = next_attack_step;
            if let Some(group) = &mut actor.skills.main.group {
                group.core_blow_step = Some(step);
            }
            actor.skills.main.set_pending(Some(PendingRelease {
                step,
                target: target_id,
            }));
            let _attack_point_rejected =
                self.release(SkillRef::main(FightActorRef::Unit(actor_id)), events)?;
        }
        Ok(())
    }

    /// A sibling's blow at what it fires at now: the unit it holds, or a
    /// construction in its way.
    fn release_group_slot(
        &mut self,
        skill_ref: SkillRef,
        skill_index: usize,
        target: FightActorRef,
        step: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let actor_id = skill_ref
            .owner
            .unit_id()
            .expect("only a unit's skill is grouped");
        // A beam strikes what it fires at, a unit or a construction, as the
        // core's does, and counts as its blow starts
        // (`SkillAttackController.PerformAttack`): its ramp's multiplier is
        // the one of its own count.
        if self.skill(skill_ref).kind == SkillKind::Laser {
            if !self.fight_actor_is_alive(target) {
                return Ok(());
            }
            self.refresh_group_skill_attack_interval(skill_ref, skill_index, step)?;
            let sibling = self.skill_mut(skill_ref).sibling_mut(skill_index);
            sibling.attack_count += 1;
            let blow = usize::try_from(sibling.attack_count).unwrap_or(0);
            return self.laser_effect(skill_ref, skill_index, blow, target, events);
        }
        match target {
            FightActorRef::Unit(target_id)
                if self.actors.get(&target_id).is_some_and(Actor::alive) =>
            {
                self.refresh_group_skill_attack_interval(skill_ref, skill_index, step)?;
                if self.skill(skill_ref).kind == SkillKind::Strike {
                    self.direct_effect(actor_id, target, skill_index, events)?;
                } else if self.skill(skill_ref).standalone() {
                    self.release_standalone_projectile(
                        actor_id,
                        skill_index,
                        target,
                        step,
                        events,
                    )?;
                } else {
                    self.release_projectile(
                        skill_ref,
                        target_id,
                        skill_index,
                        skill_index,
                        events,
                    )?;
                    self.climb_joined_slot_projectile(skill_ref, skill_index, target)?;
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
                // A striking sibling at a construction: the Raiden's slots
                // recorded against a Defensive Wall strike no block where
                // this path would, so which construction a striking slot
                // takes is not established.
                if self.skill(skill_ref).kind == SkillKind::Strike {
                    return Err(Error::new(format!(
                        "unit {actor_id}'s grouped slot {skill_index} strikes construction \
                         {building_id}, and a striking slot at a construction is not measured"
                    )));
                }
                self.refresh_group_skill_attack_interval(skill_ref, skill_index, step)?;
                self.release_projectile_to(
                    skill_ref,
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
                self.climb_joined_slot_projectile(
                    skill_ref,
                    skill_index,
                    FightActorRef::Building(building_id),
                )?;
            }
            FightActorRef::Unit(_) => return Ok(()),
        }
        // `SkillAttackController.PerformAttack` counts the blow.
        self.skill_mut(skill_ref)
            .sibling_mut(skill_index)
            .attack_count += 1;
        Ok(())
    }

    /// `FightSkill.Update` turning a weapon with an arc of its own,
    /// `RotationLimitFightTransform.RotateTo`: each slot's weapon turns
    /// towards its skill's lock by the unit's rotate speed, then is
    /// held within its arc, measured from the mech body's rotation plus the
    /// weapon's default angle (`CalculateDefaultRotation`), as the body
    /// pointed before the motion turned it.
    pub(in crate::fight) fn turn_arc_weapons(&mut self, actor_id: u64, turret: Option<i64>) {
        let actor = &self.actors[&actor_id];
        let (Some(arcs), Some(turret)) = (actor.rules.attack.weapons.arcs.clone(), turret) else {
            return;
        };
        // No skill updates once the fight is decided.
        if self.ending.stop_step.is_some() {
            return;
        }
        let turn = actor.arc_weapon_turn_q32();
        let (x, z) = (actor.x_q32, actor.z_q32);
        for (slot, arc) in arcs.iter().enumerate() {
            // `FightSkill.Update` turns the weapon to the skill's lock, and a
            // skill without one, idle or cooling, leaves it where it points.
            let Some(target) = self.actors[&actor_id].skills.main.slot_lock(slot) else {
                continue;
            };
            let Some(view) = self.fight_actor(target) else {
                continue;
            };
            let bearing = direction_degrees_q32_raw(
                view.x_q32.saturating_sub(x),
                view.z_q32.saturating_sub(z),
            );
            let skill = &mut self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .skills
                .main;
            let turned = rotate_towards_q32(skill.weapon_rotations_q32[slot], bearing, turn);
            skill.weapon_rotations_q32[slot] = held_within_arc(turned, turret, arc);
        }
    }

    /// Where a batch of standalone weapons turns its mech body on this
    /// update (`MotionAttackState.AttackRotate`, `FightMech.RotateBodyTo`),
    /// from where the unit stands as its motion updates: the attack target
    /// of its first weapon that holds a lock, or, while the motion attacks,
    /// of its first weapon in its attack that holds one. A weapon idle on a
    /// fresh lock does not draw the body off a weapon firing.
    pub(in crate::fight) fn aim_standalone_turret(&mut self, actor_id: u64) {
        let actor = &self.actors[&actor_id];
        if !actor.skills.main.standalone() || actor.turret_q32.is_none() {
            return;
        }
        let skill = &actor.skills.main;
        if skill.mech_searches() {
            // `CalculateTargetDirection` with the unit as the motion's
            // attacker: towards its own lock.
            let aim = skill
                .unit_lock()
                .and_then(|lock| self.fight_actor(lock))
                .map(|view| {
                    direction_degrees_q32_raw(
                        view.x_q32.saturating_sub(actor.x_q32),
                        view.z_q32.saturating_sub(actor.z_q32),
                    )
                });
            self.actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .turret_aim_q32 = aim;
            return;
        }
        // `CalculateTargetDirection` asks the weapon holding the motion for
        // its lock (`GetLockTarget`), idle or not.
        let holder = skill.group.as_ref().map_or(0, |group| group.motion_slot);
        let aim = skill
            .slot_lock(holder)
            .and_then(|target| self.fight_actor(target))
            .map(|view| {
                direction_degrees_q32_raw(
                    view.x_q32.saturating_sub(actor.x_q32),
                    view.z_q32.saturating_sub(actor.z_q32),
                )
            });
        self.actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .turret_aim_q32 = aim;
    }
}

/// A weapon's rotation held within its arc: `RotationLimitFightTransform`
/// keeps it within its left and right limits of its rest, its parent's
/// rotation plus its default angle (`CalculateDefaultRotation`); a weapon
/// without limits turns freely.
pub(in crate::fight) fn held_within_arc(turned: i64, parent: i64, arc: &WeaponArc) -> i64 {
    match (arc.left, arc.right) {
        (Some(left), Some(right)) => {
            let rest = parent.saturating_add(i64::from(arc.default) << 32);
            let full = 360_i64 << 32;
            let half = 180_i64 << 32;
            let delta = (turned - rest + half).rem_euclid(full) - half;
            let delta = delta.clamp(-(i64::from(left) << 32), i64::from(right) << 32);
            (rest + delta).rem_euclid(full)
        }
        _ => turned,
    }
}
