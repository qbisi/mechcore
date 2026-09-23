use super::*;

/// How far off the line of fire an enemy construction may stand and still take
/// the shot, in space units.
///
/// `docs/rules/constructions.md` carries the measurement: 94 decisions over
/// six unit types leave it in `[10.8, 11.8]` metres, and a Steel Ball of
/// `wall-laser.yaml`, closing on block 3 a few centimetres a tick, narrows it
/// to `[11.447, 11.507)` — it passes the block by at 11.507 for nine ticks and
/// takes it the tick after 11.447. 11.5 is the value inside that. It is not
/// the attacker's: a Fang of radius 2, a Marksman of 8, a Steel Ball of 6 and
/// a Wraith of 11 use the same one.
pub(in crate::fight) const WALL_IN_THE_WAY_WIDTH: i64 = 11_500;

impl Simulation {
    /// Asks again which construction, if any, stands in this actor's line of
    /// fire, and hands it to the weapons.
    ///
    /// The lock is left alone. `FightSkill.SearchAttackTarget` asks
    /// `WallConstructionTargetChecker` wherever a search settles and every
    /// tick the skill is idle, and a block that is no longer in the way stops
    /// being the attack target the next time it is asked. Nothing changes in a
    /// fight that places no enemy construction.
    pub(in crate::fight) fn search_attack_target(&mut self, owner: FightActorRef) {
        let found = match self.skill(owner).lock_target {
            // A search that already chose a building is not redirected: the
            // measurement is a wall taking the place of a unit.
            Some(target @ FightActorRef::Unit(_)) => self
                .wall_in_the_way(owner, target)
                .map(|building| (building, target)),
            _ => None,
        };
        self.skill_mut(owner).in_the_way = found;
        if !self.skill(owner).group_skill_targets.is_empty() {
            self.refresh_group_walls(owner.unit_id().expect("only a unit's skill is grouped"));
        }
    }

    /// `SkillAttackableChecker.Check`: whether the skill can go on with its
    /// attack, deciding its lock and what it fires at again on the way.
    ///
    /// A live lock is kept and `SearchAttackTarget` asked again; a dead one
    /// is searched for, and a skill that cannot switch quickly fails if that
    /// changes what it fires at. What it fires at must then be in the attack
    /// area. One out of range is searched for again (`CheckWhenLoseTarget`),
    /// and the check passes if the answer is in the area.
    ///
    /// What that second search can answer depends on the skill. A skill that
    /// switches quickly takes the selector's answer; one that does not gets
    /// back the lock it has while that lock lives, and fails. Checked against
    /// every `Check` call of the 82 manifest fights, 149,695 of 149,829 before
    /// the grouped search was included. The grouped oracle now also agrees on
    /// all 4,188 slot calls, plus 344 in the two-target fallback capture, on
    /// return value, lock and attack target (`grouped_checker_matches_every_captured_call`). A
    /// Crawler whose lock walks out of reach keeps it and ends its attack; a
    /// Stormcaller whose lock does takes the next target in reach.
    ///
    /// It is not a quick-switch branch. The build reads that flag
    /// (`ISkillData` slot 24) in one place, the research branch above. The
    /// second search is `SkillSearchTargetController.PerformNormalSkillSearch`,
    /// which finds no prepared job for a live lock, because only a null or
    /// dead lock prepares one. So it runs the synchronous search: the selector
    /// over the enemies within max(reach, 400 m), then 200 m and 300 m wider,
    /// then all of them. That search answers the kept lock. Why it does is
    /// not read: our selector, fed the snapshot or live positions in place of
    /// the rule, keeps 73 or 71 of the 106 pinned fights.
    pub(in crate::fight) fn check_attackable(
        &mut self,
        owner: FightActorRef,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<bool> {
        #[cfg(test)]
        if let Some(actor_id) = owner.unit_id() {
            self.replay_group_checker_calls(actor_id);
        }
        let skill = self.skill(owner);
        let slot = skill
            .group_skill_targets
            .iter()
            .any(Option::is_some)
            .then_some(0);
        self.check_attackable_slot(owner, slot, target_search_order)
    }

    pub(in crate::fight) fn check_attackable_slot(
        &mut self,
        owner: FightActorRef,
        slot: Option<usize>,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<bool> {
        let lock = self.slot_lock_target(owner, slot);
        let before = self.slot_attack_target(owner, slot);
        if lock.is_some_and(|lock| self.fight_actor_is_alive(lock)) {
            if let Some(slot) = slot.filter(|slot| *slot > 0) {
                self.check_group_redistribution_scope(
                    owner.unit_id().expect("only a unit's skill is grouped"),
                    slot,
                    target_search_order,
                )?;
            }
            self.search_slot_attack_target(owner, slot);
        } else {
            if !self.search_lock_target(owner, slot, target_search_order)? {
                return Ok(false);
            }
            if !self.quick_switch_target(owner) && self.slot_attack_target(owner, slot) != before {
                return Ok(false);
            }
        }
        let Some(target) = self.slot_attack_target(owner, slot) else {
            return Ok(false);
        };
        if self.slot_target_in_attack_area(owner, slot, target) {
            return Ok(true);
        }
        if self.slot_target_in_attack_range(owner, slot, target) {
            return Ok(false);
        }
        if !self.quick_switch_target(owner)
            && self
                .slot_lock_target(owner, slot)
                .is_some_and(|lock| self.fight_actor_is_alive(lock))
        {
            return Ok(false);
        }
        if !self.search_lock_target(owner, slot, target_search_order)? {
            return Ok(false);
        }
        Ok(self
            .slot_attack_target(owner, slot)
            .is_some_and(|target| self.slot_target_in_attack_area(owner, slot, target)))
    }

    /// Main child skills read their parent's range plus Q32 `0xA00000000`
    /// (10 metres) in build 2259's `FightSkill.GetAttackRange`. The first
    /// grouped skill has no parent and keeps the ordinary range.
    pub(in crate::fight) fn slot_attack_range(&self, actor_id: u64, slot: Option<usize>) -> i64 {
        self.actors[&actor_id].stats.attack_range().saturating_add(
            if slot.is_some_and(|slot| slot > 0) {
                10_000
            } else {
                0
            },
        )
    }

    pub(in crate::fight) fn slot_target_in_attack_range(
        &self,
        owner: FightActorRef,
        slot: Option<usize>,
        target: FightActorRef,
    ) -> bool {
        if slot.is_none_or(|slot| slot == 0) {
            return self.target_in_attack_range(owner, target);
        }
        let actor_id = owner.unit_id().expect("only a unit's skill is grouped");
        let source = &self.actors[&actor_id];
        let Some(target) = self.fight_actor(target) else {
            return false;
        };
        let distance =
            native_q32_magnitude(target.x_q32 - source.x_q32, target.z_q32 - source.z_q32)
                .saturating_sub(space_to_q32(source.rules.collision_radius()))
                .saturating_sub(space_to_q32(target.radius))
                .max(0);
        target.alive
            && target.targetable
            && distance >= space_to_q32(source.rules.attack.min_range())
            && distance <= space_to_q32(self.slot_attack_range(actor_id, slot))
    }

    fn slot_target_in_attack_area(
        &self,
        owner: FightActorRef,
        slot: Option<usize>,
        target: FightActorRef,
    ) -> bool {
        if slot.is_none_or(|slot| slot == 0) {
            return self.target_in_attack_area(owner, target);
        }
        self.slot_target_in_attack_range(owner, slot, target)
            && self.bodyless_target_in_attack_angle(
                owner.unit_id().expect("only a unit's skill is grouped"),
                target,
            )
    }

    fn slot_lock_target(&self, owner: FightActorRef, slot: Option<usize>) -> Option<FightActorRef> {
        let skill = self.skill(owner);
        slot.map_or(skill.lock_target, |slot| {
            skill.group_skill_targets[slot].map(FightActorRef::Unit)
        })
    }

    fn slot_attack_target(
        &self,
        owner: FightActorRef,
        slot: Option<usize>,
    ) -> Option<FightActorRef> {
        let skill = self.skill(owner);
        slot.map_or_else(
            || skill.attack_target(),
            |slot| skill.group_attack_target(slot),
        )
    }

    fn search_slot_attack_target(&mut self, owner: FightActorRef, slot: Option<usize>) {
        if slot.is_some() {
            self.refresh_group_walls(owner.unit_id().expect("only a unit's skill is grouped"));
        } else {
            self.search_attack_target(owner);
        }
    }

    fn search_lock_target(
        &mut self,
        owner: FightActorRef,
        slot: Option<usize>,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<bool> {
        let Some(slot) = slot else {
            return self.search_normal_lock_target(owner, target_search_order);
        };
        let actor_id = owner.unit_id().expect("only a unit's skill is grouped");
        let selected = self.select_group_lock_replacement(actor_id, slot, target_search_order)?;
        let actor = self.actors.get_mut(&actor_id).expect("actor exists");
        actor.skill.group_skill_targets[slot] = selected.and_then(FightActorRef::unit_id);
        actor.skill.lock_target = selected;
        self.refresh_group_walls(actor_id);
        Ok(selected.is_some())
    }

    /// Whether `SkillAttackState` asks `CheckAttackable` on this update.
    ///
    /// It asks while no attack phase runs between two blows, through the wait
    /// before a blow, and on the update the blow lands, before it is
    /// performed; not through a burst after its first shot, nor during the
    /// backswing. Every `Check` call the game made across the 82 manifest
    /// fights falls on one of these updates.
    pub(in crate::fight) fn between_blows(&self, owner: FightActorRef, step: u64) -> bool {
        let skill = self.skill(owner);
        let waiting = skill.pending().is_none()
            && skill.projectile_pending_releases.is_empty()
            && skill
                .backswing_finish_step()
                .is_none_or(|finish| finish < step);
        let winding_up = skill.pending().is_some_and(|pending| step <= pending.step);
        skill.phase() == FightSkillPhase::Attack && (waiting || winding_up)
    }

    /// `SkillAttackState.CheckAttackable`: an attack on a construction that
    /// has fallen is over; otherwise `FightSkill.CheckAttackable(true)`.
    ///
    /// The construction test is the build's own: the state tests the attack
    /// target for `FightConstruction`, the class `FightTeamController.RemoveActor`
    /// hands to `RemoveConstruction`, and rejects a dead one before asking the
    /// checker. A dead unit goes on to the checker and may be switched from,
    /// and so does a tower, which is a `FightCrystal`: the Marksman of
    /// `crawlers-vs-marksman.yaml` stays attacking onto the next Crawler, the
    /// one of `anti-armor-head-on.yaml` onto a Crawler the tick after it fells
    /// red's tower, and the one of `wall-line-of-fire.yaml` finishes when its
    /// block falls.
    pub(in crate::fight) fn attack_state_check_attackable(
        &mut self,
        owner: FightActorRef,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<bool> {
        if let Some(target @ FightActorRef::Building(_)) = self.skill(owner).attack_target()
            && !self.is_tower(target)
            && !self.fight_actor_is_alive(target)
        {
            return Ok(false);
        }
        self.check_attackable(owner, target_search_order)
    }

    /// `SkillAttackState.Finish`: `StopAttack` drops the lock, the weapons
    /// keeping what they fired at; the skill then cools for its cooling time
    /// and enters `SkillIdleState` with its targets cleared.
    pub(in crate::fight) fn finish_attack(&mut self, owner: FightActorRef, step: u64) {
        let cooling_steps = native_time_units_to_steps(
            self.attacker(owner)
                .expect("skill owner identity is stable")
                .attack
                .cooling_time_units(),
        );
        let skill = self.skill_mut(owner);
        let fired_at = skill.attack_target();
        skill.drop_lock();
        skill.laser_attack_count = 0;
        skill.retarget_after_own_direct_kill = false;
        skill.set_phase(FightSkillPhase::Idle);
        skill.set_backswing_finish_step(None);
        skill.set_pending(None);
        if cooling_steps > 0 {
            skill.set_cooling(Some((step, fired_at)));
        }
        // `MotionIdleState.Enter` publishes the stop once; a unit whose
        // motion is idle already keeps the point it stopped at.
        if let Some(actor) = self.moving_mut(owner) {
            let entered_idle = actor.motion.state != MotionState::Idle;
            actor.stop_in_place(entered_idle);
        }
    }

    /// What `SearchLockTarget` answers in the middle of an update.
    ///
    /// The selector reads the positions the tick's query snapshot holds,
    /// unless the lock it replaces died during this very tick, when it reads
    /// where every candidate stands now; and a candidate that has itself
    /// just died is searched past with live positions.
    pub(in crate::fight) fn select_lock_replacement(
        &self,
        owner: FightActorRef,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<Option<FightActorRef>> {
        let died_this_tick = self
            .skill(owner)
            .mechanical_attack_target()
            .and_then(|target| self.fight_actor(target))
            .is_some_and(|target| target.query_alive && !target.alive);
        let selected =
            self.select_normal_target_with_order(owner, target_search_order, died_this_tick)?;
        if !died_this_tick
            && selected
                .and_then(|candidate| self.fight_actor(candidate))
                .is_some_and(|target| target.query_alive && !target.alive)
        {
            return self.select_normal_target_with_order(owner, target_search_order, true);
        }
        Ok(selected)
    }

    /// `SearchLockTarget` followed by `SearchAttackTarget`, as the checker
    /// runs them; no lock found clears the targets.
    fn search_normal_lock_target(
        &mut self,
        owner: FightActorRef,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<bool> {
        let selected = self.select_lock_replacement(owner, target_search_order)?;
        let skill = self.skill_mut(owner);
        let Some(selected) = selected else {
            skill.drop_lock();
            return Ok(false);
        };
        skill.lock_target = Some(selected);
        self.search_attack_target(owner);
        Ok(true)
    }

    /// Enters `SkillIdleState` with `needClearTarget`, as a failed check does:
    /// the lock and the attack target are cleared, and the next update
    /// searches.
    pub(in crate::fight) fn enter_idle_clearing_targets(&mut self, owner: FightActorRef) {
        let skill = self.skill_mut(owner);
        skill.drop_lock();
        skill.set_phase(FightSkillPhase::Idle);
        skill.search_target_time = 0;
        skill.set_backswing_finish_step(None);
        skill.set_pending(None);
        if let Some(actor) = self.moving_mut(owner) {
            actor.stop_in_place(true);
        }
    }

    /// Which enemy construction stands between this actor and its target.
    ///
    /// `docs/rules/constructions.md` states the rule and the readings behind
    /// it: of the enemy's constructions, the ones within reach edge to edge and
    /// within the width of the line of fire, the **nearest to the attacker** —
    /// not the nearest construction and not the one nearest the line.
    pub(in crate::fight) fn wall_in_the_way(
        &self,
        owner: FightActorRef,
        target: FightActorRef,
    ) -> Option<u64> {
        let actor = self.attacker(owner)?;
        let aimed = self.fight_actor(target)?;
        // A wall is considered when it is within reach edge to edge: the
        // attacker's range plus its own radius and the block's. A constant
        // allowance fits a Crawler and not a Wraith, which attacks a block 73.8
        // metres off with a reach of 60.
        let reach = space_to_q32(actor.attack_range.saturating_add(actor.radius));
        let width = space_to_q32(WALL_IN_THE_WAY_WIDTH);
        let mut nearest: Option<(i64, u64)> = None;
        for building in &self.buildings {
            if building.building_type_id != CONSTRUCTION_BUILDING_TYPE
                || building.team_id == actor.team
                || !building_alive(building)
                || !building.targetable
            {
                continue;
            }
            let distance = native_q32_magnitude(
                building.position.x.saturating_sub(actor.x_q32),
                building.position.z.saturating_sub(actor.z_q32),
            );
            if distance > reach.saturating_add(building.bounds_width / 2) {
                continue;
            }
            if distance_to_segment_q32(
                (actor.x_q32, actor.z_q32),
                (aimed.x_q32, aimed.z_q32),
                (building.position.x, building.position.z),
            ) > width
            {
                continue;
            }
            if nearest.is_none_or(|(best, _)| distance < best) {
                nearest = Some((distance, building.building_id));
            }
        }
        nearest.map(|(_, building_id)| building_id)
    }
}
