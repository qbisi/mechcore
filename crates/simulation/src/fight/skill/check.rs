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
    pub(in crate::fight) fn search_attack_target(&mut self, skill_ref: SkillRef) {
        // Whatever the lock is, a unit or a building: `SearchAttackTarget`
        // asks `CheckWallConstruction` before it looks at the lock at all.
        let found = self.skill(skill_ref).lock_target.and_then(|target| {
            self.wall_in_the_way(skill_ref.owner, target)
                .map(|building| (building, target))
        });
        // `SearchTargetShield`: with no construction in the way, a lock its
        // side's shield covers makes the shield what the skill fires at.
        let shield = if found.is_none() {
            self.skill(skill_ref).lock_target.and_then(|target| {
                self.search_target_shield(skill_ref.owner, target)
                    .map(|shield| (shield, target))
            })
        } else {
            None
        };
        self.skill_mut(skill_ref).in_the_way = found;
        self.skill_mut(skill_ref).target_shield = shield;
        if !self.skill(skill_ref).siblings().is_empty() {
            self.refresh_group_walls(
                skill_ref
                    .owner
                    .unit_id()
                    .expect("only a unit's skill is grouped"),
                None,
            );
        }
    }

    /// `SkillAttackableChecker.Check`: whether the skill can go on with its
    /// attack, deciding its lock and what it fires at again on the way.
    ///
    /// A live lock is kept and `SearchAttackTarget` asked again; a dead one
    /// is searched for, and a skill that cannot switch quickly fails if that
    /// changes what it fires at. What it fires at must then be in the attack
    /// area.
    ///
    /// A target out of the area is searched for again only when
    /// `IsAttackTargetInAttackArea` reports `isMissingDistance`, and that is
    /// `FightSkill.IsInAttackRange`'s `isMissing`: the target stands nearer
    /// than the skill's minimum range (vtable 1088), not beyond its range
    /// (1104). Then `CheckWhenLoseTarget` searches (`SearchLockTarget`) and the
    /// check passes if the answer is in the area. A target beyond the range
    /// fails the check, for every skill, whether it switches quickly or not.
    /// The Stormcaller, the one unit with a minimum range (70 m), is the one
    /// whose lock walking inside it takes the next target in reach; a Crawler
    /// or a Marksman whose live lock walks out of reach ends its attack
    /// (`tests/turret/fights/anti-armor-head-on.yaml`, tick 1027).
    ///
    /// The build reads the quick-switch flag (`ISkillData` slot 24) in one
    /// place, the research branch above. An earlier stand-in let a
    /// quick-switching skill search again beyond its range; every pinned fight
    /// plays back the same without it, and the Anti-Armor Crawler fight only
    /// without it. Grouped skills are checked against all 4,188 captured slot
    /// calls plus 344 in the two-target fallback capture, on return value,
    /// lock and attack target (`grouped_checker_matches_every_captured_call`).
    pub(in crate::fight) fn check_attackable(
        &mut self,
        skill_ref: SkillRef,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<bool> {
        #[cfg(test)]
        if let Some(actor_id) = skill_ref.owner.unit_id() {
            self.replay_group_checker_calls(actor_id);
        }
        // A grouped core searches as its group's slot 0, around what its
        // siblings hold.
        let slot = self.skill(skill_ref).is_grouped().then_some(0);
        self.check_attackable_slot(skill_ref, slot, true, target_search_order)
    }

    pub(in crate::fight) fn check_attackable_slot(
        &mut self,
        skill_ref: SkillRef,
        slot: Option<usize>,
        attacking_check: bool,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<bool> {
        let lock = self.slot_lock_target(skill_ref, slot);
        let before = self.slot_attack_target(skill_ref, slot);
        if lock.is_some_and(|lock| self.fight_actor_is_alive(lock)) {
            if let Some(slot) = slot.filter(|slot| *slot > 0)
                && attacking_check
                && !self.skill(skill_ref).standalone()
                && self.sibling_yields(
                    skill_ref
                        .owner
                        .unit_id()
                        .expect("only a unit's skill is grouped"),
                    slot,
                    target_search_order,
                )?
            {
                return Ok(false);
            }
            self.search_slot_attack_target(skill_ref, slot);
        } else {
            if !self.search_lock_target(skill_ref, slot, target_search_order)? {
                return Ok(false);
            }
            if !self.quick_switch_target(skill_ref.owner)
                && self.slot_attack_target(skill_ref, slot) != before
            {
                return Ok(false);
            }
        }
        let Some(target) = self.slot_attack_target(skill_ref, slot) else {
            return Ok(false);
        };
        if self.slot_target_in_attack_area(skill_ref, slot, target) {
            return Ok(true);
        }
        if self.slot_target_in_attack_range(skill_ref, slot, target) {
            return Ok(false);
        }
        if !self.target_inside_min_range(skill_ref.owner, target) {
            return Ok(false);
        }
        let attackable = self.search_lock_target(skill_ref, slot, target_search_order)?
            && self
                .slot_attack_target(skill_ref, slot)
                .is_some_and(|target| self.slot_target_in_attack_area(skill_ref, slot, target));
        if attacking_check && slot.is_none_or(|slot| slot == 0) {
            self.reset_attack_data_on_losing_target(skill_ref, attackable)?;
        }
        Ok(attackable)
    }

    /// The `ResetAttackData` of `CheckWhenLoseTarget`, which an attacking
    /// check of the main searcher calls while a blow is under way
    /// (`SkillAttackController.currentController` set): it draws the interval
    /// again (`RefreshAttackInterval` with `addOffsetTime`), and gives the
    /// wait back whole (`refreshAttackTime`) when no blow of this attack state
    /// has run its cycle out, unless the blow is still winding up on a target
    /// it can attack. Recorded: a Stormcaller whose target walked inside its
    /// minimum range two ticks into the wind-up read `attackTime` 3 before
    /// the check and 132, its interval, after it.
    fn reset_attack_data_on_losing_target(
        &mut self,
        skill_ref: SkillRef,
        attackable: bool,
    ) -> Result<()> {
        let skill = self.skill(skill_ref);
        let SkillState::Attack(blow) = skill.state else {
            return Ok(());
        };
        if blow == Blow::Waiting {
            return Ok(());
        }
        let refresh = skill.perform_count == 0 && !(matches!(blow, Blow::Before(_)) && attackable);
        let started = skill
            .next_attack_step
            .saturating_sub(skill.current_attack_interval);
        let interval = self.draw_attack_interval(skill_ref.owner)?;
        let skill = self.skill_mut(skill_ref);
        skill.current_attack_interval = interval;
        skill.next_attack_step = if refresh {
            0
        } else {
            started.saturating_add(interval)
        };
        Ok(())
    }

    /// Main child skills read their parent's range plus Q32 `0xA00000000`
    /// (10 metres) in the build's `FightSkill.GetAttackRange`. The first
    /// grouped skill has no parent and keeps the ordinary range, as does
    /// every standalone weapon's skill, which no `SkillGroup` parents.
    pub(in crate::fight) fn slot_attack_range(&self, actor_id: u64, slot: Option<usize>) -> i64 {
        let actor = &self.actors[&actor_id];
        actor.stats.attack_range().saturating_add(
            if slot.is_some_and(|slot| slot > 0) && !actor.skills.main.standalone() {
                10_000
            } else {
                0
            },
        )
    }

    pub(in crate::fight) fn slot_target_in_attack_range(
        &self,
        skill_ref: SkillRef,
        slot: Option<usize>,
        target: FightActorRef,
    ) -> bool {
        if slot.is_none_or(|slot| slot == 0) {
            return self.target_in_attack_range(skill_ref, target);
        }
        let actor_id = skill_ref
            .owner
            .unit_id()
            .expect("only a unit's skill is grouped");
        let source = &self.actors[&actor_id];
        // A slot firing at a shield reaches it once the shield's surface on
        // its way to the lock is in its range, as the core does.
        if let Some(shield) = source
            .skills
            .main
            .sibling(slot.unwrap_or(0))
            .shield_target()
        {
            return self.fight_actor(target).is_some_and(|view| view.alive)
                && self
                    .shield_attack_point(shield, skill_ref, target)
                    .is_some_and(|(x_q32, z_q32)| {
                        let distance =
                            native_q32_magnitude(x_q32 - source.x_q32, z_q32 - source.z_q32)
                                .saturating_sub(space_to_q32(source.rules.collision_radius()))
                                .max(0);
                        distance >= space_to_q32(source.rules.attack.min_range())
                            && distance <= space_to_q32(self.slot_attack_range(actor_id, slot))
                    });
        }
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

    /// `IsInAttackRange`'s `isMissing`: the target stands nearer than the
    /// skill's minimum range, the only miss of distance
    /// `CheckWhenLoseTarget` searches again for.
    fn target_inside_min_range(&self, owner: FightActorRef, target: FightActorRef) -> bool {
        let (Some(source), Some(target)) = (self.attacker(owner), self.fight_actor(target)) else {
            return false;
        };
        let distance =
            native_q32_magnitude(target.x_q32 - source.x_q32, target.z_q32 - source.z_q32)
                .saturating_sub(space_to_q32(source.radius))
                .saturating_sub(space_to_q32(target.radius))
                .max(0);
        distance < space_to_q32(source.attack.min_range())
    }

    pub(in crate::fight) fn slot_target_in_attack_area(
        &self,
        skill_ref: SkillRef,
        slot: Option<usize>,
        target: FightActorRef,
    ) -> bool {
        if slot.is_none_or(|slot| slot == 0) {
            return self.target_in_attack_area(skill_ref, target);
        }
        let actor_id = skill_ref
            .owner
            .unit_id()
            .expect("only a unit's skill is grouped");
        let actor = &self.actors[&actor_id];
        let slot = slot.unwrap_or(0);
        // A standalone weapon's skill measures its angle from its own weapon.
        let rotation = if actor.skills.main.standalone() {
            actor.skills.main.weapon_rotations_q32[slot]
        } else {
            actor.slot_main_rotation_q32(slot)
        };
        self.slot_target_in_attack_range(skill_ref, Some(slot), target)
            && self.fight_actor(target).is_some_and(|view| {
                view.alive
                    && rotation_distance_q32(
                        rotation,
                        direction_degrees_q32_raw(
                            view.x_q32.saturating_sub(actor.x_q32),
                            view.z_q32.saturating_sub(actor.z_q32),
                        ),
                    ) <= mdeg_to_degrees_q32(actor.rules.attack.attack_half_angle_mdeg())
            })
    }

    fn slot_lock_target(&self, skill_ref: SkillRef, slot: Option<usize>) -> Option<FightActorRef> {
        self.skill(skill_ref).slot_lock(slot.unwrap_or(0))
    }

    fn slot_attack_target(
        &self,
        skill_ref: SkillRef,
        slot: Option<usize>,
    ) -> Option<FightActorRef> {
        self.skill(skill_ref).group_attack_target(slot.unwrap_or(0))
    }

    fn search_slot_attack_target(&mut self, skill_ref: SkillRef, slot: Option<usize>) {
        if slot.is_some_and(|slot| slot > 0) {
            self.refresh_group_walls(
                skill_ref
                    .owner
                    .unit_id()
                    .expect("only a unit's skill is grouped"),
                slot,
            );
        } else {
            self.search_attack_target(skill_ref);
        }
    }

    fn search_lock_target(
        &mut self,
        skill_ref: SkillRef,
        slot: Option<usize>,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<bool> {
        let Some(slot) = slot else {
            return self.search_normal_lock_target(skill_ref, target_search_order);
        };
        let actor_id = skill_ref
            .owner
            .unit_id()
            .expect("only a unit's skill is grouped");
        let selected = self.select_group_lock_replacement(actor_id, slot, target_search_order)?;
        if slot == 0 {
            self.take_from_siblings(actor_id, selected);
        }
        let idle = selected.is_none();
        let selected = if idle {
            self.select_alive_target(skill_ref, Some(slot), target_search_order)?
        } else {
            selected
        };
        let actor = self.actors.get_mut(&actor_id).expect("actor exists");
        if slot == 0 {
            actor.skills.main.idle = idle;
            actor.skills.main.write_lock(selected);
            self.search_attack_target(SkillRef::main(FightActorRef::Unit(actor_id)));
        } else {
            let sibling = actor.skills.main.sibling_mut(slot);
            sibling.idle = idle;
            sibling.lock_target = selected;
            sibling.attack_target_left = None;
            self.refresh_group_walls(actor_id, Some(slot));
        }
        Ok(selected.is_some())
    }

    /// Whether `SkillAttackState` asks `CheckAttackable` on this update.
    ///
    /// It asks while no attack phase runs between two blows, through the wait
    /// before a blow, and on the update the blow lands, before it is
    /// performed; not through a burst after its first shot, nor during the
    /// backswing. Every `Check` call the game made across the 82 fights of
    /// `tests/regression/fights/` falls on one of these updates.
    pub(in crate::fight) fn between_blows(&self, skill_ref: SkillRef, step: u64) -> bool {
        let skill = self.skill(skill_ref);
        let waiting = skill.pending().is_none()
            && skill.performer.pending().is_empty()
            && !skill.performer.sweeping()
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
        skill_ref: SkillRef,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<bool> {
        if let Some(target @ FightActorRef::Building(_)) = self.skill(skill_ref).attack_target()
            && !self.is_tower(target)
            && !self.fight_actor_is_alive(target)
        {
            return Ok(false);
        }
        self.check_attackable(skill_ref, target_search_order)
    }

    /// `SkillAttackState.Finish`: `StopAttack` drops the lock, the weapons
    /// keeping what they fired at; the skill then cools for its cooling time
    /// and enters `SkillIdleState` with its targets cleared.
    pub(in crate::fight) fn finish_attack(&mut self, skill_ref: SkillRef, step: u64) {
        let cooling_steps = native_time_units_to_steps(
            self.attacker(skill_ref.owner)
                .expect("skill owner identity is stable")
                .attack
                .cooling_time_units(),
        );
        let skill = self.skill_mut(skill_ref);
        // A skill firing at a shield has no attack target to go on naming.
        let fired_at = skill
            .attack_target()
            .filter(|_| skill.shield_target().is_none());
        skill.drop_lock();
        skill.set_phase(FightSkillPhase::Idle);
        skill.set_backswing_finish_step(None);
        skill.set_pending(None);
        if cooling_steps > 0 {
            skill.set_cooling(Some((step, fired_at)));
        }
        // A standalone weapon's skill leaves the motion to the batch.
        if skill.standalone() {
            return;
        }
        // `MotionIdleState.Enter` publishes the stop once; a unit whose
        // motion is idle already keeps the point it stopped at.
        if let Some(actor) = self.moving_mut(skill_ref.owner) {
            let entered_idle = actor.motion.state != MotionState::Idle;
            actor.lose_target_motion(entered_idle);
        }
    }

    /// What `SearchLockTarget` answers in the middle of an update.
    ///
    /// `ScoreRatingTargetSelector.TrySelect` answers from the scores
    /// `FightCoreSystem.PreCalculate` worked out on the tick's query snapshot,
    /// but only for a skill whose state asked for them there; any other
    /// skill's search is `PerformSearch`, which scores every candidate where
    /// it stands now. `SkillAttackState.PreCalculate` asks only while the
    /// lock is absent or dead, and `SkillPrepareState` never does. So a
    /// Stormcaller whose live lock walks inside its minimum range searches
    /// past it: the lock was alive when the tick began. A prepared answer
    /// that has itself died since is searched past with live positions.
    pub(in crate::fight) fn select_lock_replacement(
        &self,
        skill_ref: SkillRef,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<Option<FightActorRef>> {
        let skill = self.skill(skill_ref);
        let prepared = search_prepared(skill_ref.owner)
            && skill.phase() == FightSkillPhase::Attack
            && skill
                .lock_target
                .and_then(|lock| self.fight_actor(lock))
                .is_none_or(|lock| !lock.query_alive);
        let selected =
            self.select_normal_target_with_order(skill_ref.owner, target_search_order, !prepared)?;
        if prepared
            && selected
                .and_then(|candidate| self.fight_actor(candidate))
                .is_some_and(|target| target.query_alive && !target.alive)
        {
            return self.select_normal_target_with_order(
                skill_ref.owner,
                target_search_order,
                true,
            );
        }
        Ok(selected)
    }

    /// `SearchLockTarget` followed by `SearchAttackTarget`, as the checker
    /// runs them; no lock found clears the targets.
    fn search_normal_lock_target(
        &mut self,
        skill_ref: SkillRef,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<bool> {
        let selected = self.select_lock_replacement(skill_ref, target_search_order)?;
        let idle = selected.is_none();
        let selected = if idle {
            self.select_alive_target(skill_ref, None, target_search_order)?
        } else {
            selected
        };
        let skill = self.skill_mut(skill_ref);
        skill.idle = idle;
        let Some(selected) = selected else {
            skill.drop_lock();
            return Ok(false);
        };
        skill.write_lock(Some(selected));
        self.search_attack_target(skill_ref);
        Ok(true)
    }

    /// Enters `SkillIdleState` with `needClearTarget`, as a failed check does:
    /// the lock and the attack target are cleared, and the next update
    /// searches.
    pub(in crate::fight) fn enter_idle_clearing_targets(&mut self, skill_ref: SkillRef) {
        let skill = self.skill_mut(skill_ref);
        skill.drop_lock();
        skill.set_phase(FightSkillPhase::Idle);
        skill.search_target_time = 0;
        skill.set_backswing_finish_step(None);
        skill.set_pending(None);
        if skill.standalone() {
            return;
        }
        if let Some(actor) = self.moving_mut(skill_ref.owner) {
            actor.lose_target_motion(true);
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
