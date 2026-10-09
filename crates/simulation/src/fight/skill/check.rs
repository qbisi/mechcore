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
        // `SearchTargetShield` first: a lock its side's shield covers makes
        // the shield what the skill fires at.
        let lock = self.skill(skill_ref).lock_target;
        let range = self
            .skill_attacker(skill_ref)
            .map_or(0, |attacker| attacker.shield_range());
        let shield =
            lock.and_then(|target| self.search_target_shield_in(skill_ref.owner, target, range));
        // Then `CheckWallConstruction`, whatever the lock is, a unit or a
        // building: toward the lock, or, when the skill fires at a shield,
        // toward the shield's centre, passing over a block inside it. A Fire
        // Badger whose lock stands in a shield beyond a wall off the line to
        // the shield keeps firing at the shield.
        let found = lock.and_then(|target| {
            match shield {
                Some(shield_id) => {
                    let (x_q32, z_q32) = self.shield_centre(shield_id)?;
                    self.wall_in_the_way_at(skill_ref, None, (x_q32, z_q32))
                        .filter(|&wall| {
                            !self.shield_holds(shield_id, FightActorRef::Building(wall))
                        })
                }
                None => self.wall_in_the_way(skill_ref, None, target),
            }
            .map(|building| (building, target))
        });
        let shield = if found.is_none() {
            shield.zip(lock)
        } else {
            None
        };
        self.skill_mut(skill_ref).in_the_way = found;
        self.skill_mut(skill_ref).target_shield = shield;
        self.skill_mut(skill_ref).kept_attack_target = None;
        // The siblings are asked for their blocks only once the core has
        // found one; a core with a clear line leaves each naming its own.
        if found.is_some() && !self.skill(skill_ref).siblings().is_empty() {
            self.refresh_group_walls(skill_ref, None);
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
        // `SkillAttackableChecker.Check` of a `FightSupportSkill`: its lock
        // lives.
        if self.skill(skill_ref).kind == SkillKind::Support {
            return Ok(lock.is_some_and(|lock| self.fight_actor_is_alive(lock)));
        }
        let before = self.slot_attack_target(skill_ref, slot);
        if lock.is_some_and(|lock| self.fight_actor_is_alive(lock)) {
            // Every skill of a group checks whether it gives its lock up but
            // the main skill's own first, which no `ParentSkill` parents: an
            // extra skill's group's first does.
            if let Some(slot) = slot.filter(|&slot| slot > 0 || skill_ref.slot != SkillSlot::Main)
                && attacking_check
                && !self.skill(skill_ref).standalone()
                && self.sibling_yields(skill_ref, slot, target_search_order)?
            {
                return Ok(false);
            }
            self.search_slot_attack_target(skill_ref, slot);
        } else {
            if !self.search_lock_target(skill_ref, slot, target_search_order)? {
                return Ok(false);
            }
            if !self.quick_switch_target(skill_ref)
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
            // `IsActorInAttackAngle`'s `isMissing`: a weapon held at the
            // edge of its arc cannot turn onto a target beyond it, and the
            // check searches again once the search timer allows it
            // (`SearchTargetController.CanStartSearch`). A Sabertooth's
            // Secondary Armament gun whose Crawler walks out of its arc takes
            // the next one in it.
            if !(self.weapon_at_arc_edge(skill_ref) && self.skill(skill_ref).search_target_time < 1)
            {
                return Ok(false);
            }
        } else if !self.target_inside_min_range(skill_ref, target) {
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
        let refresh = skill.performed_count(self.step_now) == 0
            && !(matches!(blow, Blow::Before(_)) && attackable);
        let started = skill
            .next_attack_step
            .saturating_sub(skill.current_attack_interval);
        let interval = self.draw_attack_interval(skill_ref)?;
        let now = i64::try_from(self.step_now).unwrap_or(i64::MAX);
        let skill = self.skill_mut(skill_ref);
        skill.current_attack_interval = interval;
        // `RefreshAttackInterval(true, refresh)`: a refreshed clock reads the
        // new interval, ready to attack.
        if refresh {
            skill.attack_time_anchor = now - i64::try_from(interval).unwrap_or(i64::MAX);
        }
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
    /// every standalone weapon's skill, which no `SkillGroup` parents. Every
    /// skill of an extra skill's group is parented by the main skill and
    /// reads its own range past the main skill's, as its first does.
    pub(in crate::fight) fn slot_attack_range(
        &self,
        skill_ref: SkillRef,
        slot: Option<usize>,
    ) -> i64 {
        let range = self
            .skill_attacker(skill_ref)
            .expect("skill owner identity is stable")
            .attack_range;
        // A main skill's slot reaches beyond its parent's range by what its
        // row adds (`FightSkill.GetAttackRange`); an extra row's own group
        // is checked against its own range already.
        range.saturating_add(match (slot, skill_ref.slot) {
            (Some(slot), SkillSlot::Main) if slot > 0 && !self.skill(skill_ref).standalone() => {
                self.skill(skill_ref)
                    .joined(slot)
                    .map_or(super::MAIN_SLOT_RANGE_ADDEND, |joined| joined.range)
            }
            _ => 0,
        })
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
        let min_range = self.skill_rules(skill_ref).min_range();
        // A slot firing at a shield reaches it once the shield's surface on
        // its way to the lock is in its range, as the core does; any other
        // target is measured to itself: a Wraith's gun whose lock stood in a
        // shield reaches the Crawler beside it its search offers.
        let sibling = self.skill(skill_ref).sibling(slot.unwrap_or(0));
        if let Some(shield) = sibling.shield_target()
            && Some(target) == sibling.lock_target
        {
            return self.fight_actor(target).is_some_and(|view| view.alive)
                && self
                    .shield_attack_point(shield, skill_ref, target)
                    .is_some_and(|(x_q32, z_q32)| {
                        let distance =
                            native_q32_magnitude(x_q32 - source.x_q32, z_q32 - source.z_q32)
                                .saturating_sub(space_to_q32(source.rules.collision_radius()))
                                .max(0);
                        distance >= space_to_q32(min_range)
                            && distance
                                <= space_to_q32(
                                    self.slot_attack_range(skill_ref, slot).saturating_add(
                                        self.skill_rules(skill_ref).extra_shield_range(),
                                    ),
                                )
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
            && distance >= space_to_q32(min_range)
            && distance <= space_to_q32(self.slot_attack_range(skill_ref, slot))
    }

    /// `IsInAttackRange`'s `isMissing`: the target stands nearer than the
    /// skill's minimum range, the only miss of distance
    /// `CheckWhenLoseTarget` searches again for.
    fn target_inside_min_range(&self, skill_ref: SkillRef, target: FightActorRef) -> bool {
        let (Some(source), Some(target)) =
            (self.skill_attacker(skill_ref), self.fight_actor(target))
        else {
            return false;
        };
        let distance =
            native_q32_magnitude(target.x_q32 - source.x_q32, target.z_q32 - source.z_q32)
                .saturating_sub(space_to_q32(source.radius))
                .saturating_sub(space_to_q32(target.radius))
                .max(0);
        distance < space_to_q32(source.min_range)
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
        let skill = self.skill(skill_ref);
        let attack = self.skill_rules(skill_ref);
        let slot = slot.unwrap_or(0);
        // A joined row's slot measures the angle its own row gives it
        // (`FightSkill.Init`).
        let half_angle = skill
            .joined(slot)
            .map_or(attack.attack_half_angle_mdeg(), |joined| {
                joined.half_angle_mdeg
            });
        // A standalone weapon's skill measures its angle from its own weapon.
        let rotation = if skill.standalone() {
            skill.weapon_rotations_q32[slot]
        } else {
            actor.slot_main_rotation_q32(attack, skill, slot)
        };
        self.slot_target_in_attack_range(skill_ref, Some(slot), target)
            && self.fight_actor(target).is_some_and(|view| {
                view.alive
                    && calculate_angle_q32(
                        rotation,
                        view.x_q32.saturating_sub(actor.x_q32),
                        view.z_q32.saturating_sub(actor.z_q32),
                    ) <= mdeg_to_degrees_q32(half_angle)
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
            self.refresh_group_walls(skill_ref, slot);
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
        let selected = self.select_group_lock_replacement(skill_ref, slot, target_search_order)?;
        if slot == 0 {
            self.take_from_siblings(skill_ref, selected);
        }
        let idle = selected.is_none();
        let selected = if idle {
            self.select_alive_target(skill_ref, Some(slot), target_search_order)?
        } else {
            selected
        };
        if slot == 0 {
            let skill = self.skill_mut(skill_ref);
            skill.idle = idle;
            skill.write_lock(selected);
            self.search_attack_target(skill_ref);
            self.hand_motion_after_lock_search(skill_ref);
        } else {
            let sibling = self.skill_mut(skill_ref).sibling_mut(slot);
            sibling.idle = idle;
            sibling.lock_target = selected;
            sibling.attack_target_left = None;
            self.refresh_group_walls(skill_ref, Some(slot));
            self.hand_standalone_motion(skill_ref, slot);
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
        let winding_up = skill.pending().is_some_and(|pending| step <= pending.step)
            && !self.self_splash_winding_up(skill_ref, step);
        skill.phase() == FightSkillPhase::Attack && (waiting || winding_up)
    }

    /// Whether a skill that splashes about itself is in the wait before its
    /// blow, which checks nothing: `SkillAttackController` makes that wait
    /// (`AttackWaitController`) with `isEnableCheckTarget` set to
    /// `!IsSelfSplash`. A Whirlwind winding up goes on naming a lock another
    /// unit kills, and its motion idles on the dead lock until it strikes.
    pub(in crate::fight) fn self_splash_winding_up(&self, skill_ref: SkillRef, step: u64) -> bool {
        self.skill_attacker(skill_ref)
            .is_some_and(|attacker| attacker.attack.self_splash)
            && self
                .skill(skill_ref)
                .pending()
                .is_some_and(|pending| step <= pending.step)
    }

    /// `SkillAttackState.CheckAttackable`: an attack on a construction that
    /// has fallen is over; otherwise `FightSkill.CheckAttackable(true)`.
    ///
    /// The construction test is the build's own: the state tests the attack
    /// target for `FightConstruction`, the class `FightTeamController.RemoveActor`
    /// hands to `RemoveConstruction`, and rejects a dead one before asking the
    /// checker. A dead unit goes on to the checker and may be switched from,
    /// and so do a tower, a `FightTower`, and an interceptor, a plain
    /// `FightCrystal`: the Marksman of `crawlers-vs-marksman.yaml` stays
    /// attacking onto the next Crawler, the one of `anti-armor-head-on.yaml`
    /// onto a Crawler the tick after it fells red's tower, the Fortress of
    /// `intercepted-control.yaml` cools naming red's tower after it fells red's
    /// interceptor, and the Marksman of `wall-line-of-fire.yaml` finishes when
    /// its block falls.
    pub(in crate::fight) fn attack_state_check_attackable(
        &mut self,
        skill_ref: SkillRef,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<bool> {
        // A skill that is not enabled fails between its blows: the blow under
        // way runs out, and its attack ends (`SkillAttackState.CheckAttackable`
        // asks `FightSkill.isEnable` while its controller is idle).
        if self.skill(skill_ref).disabled {
            return Ok(false);
        }
        // A preemptive skill attacks once: its check fails once it has
        // performed (`SkillAttackController.IsPerformedOnce`).
        if self.skill_is_preemptive(skill_ref) && self.skill(skill_ref).performed() {
            return Ok(false);
        }
        if self.attacks_fallen_construction(skill_ref, None) {
            return Ok(false);
        }
        self.check_attackable(skill_ref, target_search_order)
    }

    /// `SkillAttackState.CheckAttackable`'s own test, before it asks the
    /// checker: an attack on a construction that has fallen is over. Every
    /// skill of a group is in a state of its own and asks it of what it
    /// fires at: a Wraith's gun whose block falls stops with its lock
    /// dropped, though the unit it was found for lives.
    pub(in crate::fight) fn attacks_fallen_construction(
        &self,
        skill_ref: SkillRef,
        slot: Option<usize>,
    ) -> bool {
        let target = match slot {
            Some(slot) => self.slot_attack_target(skill_ref, Some(slot)),
            None => self.skill(skill_ref).attack_target(),
        };
        target.is_some_and(|target| {
            self.is_construction(target) && !self.fight_actor_is_alive(target)
        })
    }

    /// Whether an extra skill's weapon stands at an edge of its arc
    /// (`SkillAttackAngleChecker.IsWeaponInAttackAngle` with
    /// `isCheckMissing`): `CalculateRotationRange` gives a range narrower than
    /// the full turn and the weapon's rotation about what it is mounted on is
    /// one of its ends.
    fn weapon_at_arc_edge(&self, skill_ref: SkillRef) -> bool {
        let (FightActorRef::Unit(actor_id), SkillSlot::Extra(index)) =
            (skill_ref.owner, skill_ref.slot)
        else {
            return false;
        };
        let actor = &self.actors[&actor_id];
        let extra = &actor.skills.extras[index];
        let Some(arc) = extra.arc(0) else {
            return false;
        };
        let (Some(left), Some(right)) = (arc.left, arc.right) else {
            return false;
        };
        let Some(&rotation) = extra.skill.weapon_rotations_q32.first() else {
            return false;
        };
        let parent = match (extra.rules.attack.weapons.mount, actor.turret_rotation()) {
            (crate::rules::WeaponMount::MechBody, Some(turret)) => turret,
            _ => actor.body_rotation_q32,
        };
        let rest = parent.saturating_add(i64::from(arc.default) << 32);
        let (full, half) = (360_i64 << 32, 180_i64 << 32);
        let delta = (rotation - rest + half).rem_euclid(full) - half;
        delta == -(i64::from(left) << 32) || delta == i64::from(right) << 32
    }

    /// `SkillAttackState.Finish`: `StopAttack` drops the lock, the weapons
    /// keeping what they fired at; the skill then cools for its cooling time
    /// and enters `SkillIdleState` with its targets cleared.
    pub(in crate::fight) fn finish_attack(&mut self, skill_ref: SkillRef, step: u64) {
        // A side arm whose turn it is, with no blow under way, gives the turn
        // back.
        let skill = self.skill(skill_ref);
        if skill.pending().is_none()
            && skill
                .backswing_finish_step()
                .is_none_or(|finish| finish < step)
            && self.can_fire_by_take_turns(skill_ref)
        {
            self.force_side_arm_end_fire_turn(skill_ref);
        }
        let cooling_steps = native_time_units_to_steps(
            self.skill_attacker(skill_ref)
                .expect("skill owner identity is stable")
                .attack
                .cooling_time_units(),
        );
        let skill = self.skill_mut(skill_ref);
        // A skill firing at a shield has no attack target to go on naming.
        let fired_at = skill
            .checked_attack_target()
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
        self.cool_motion(skill_ref);
    }

    /// The motion as the skill cools without a lock. `AutoMoveBehaviour`
    /// is no longer active and the motion stops idle, publishing the stop
    /// once as `MotionIdleState.Enter` does: a unit whose motion is idle
    /// already keeps the point it stopped at. A command stays active
    /// (`MoveAttackCommand.IsActive` answers true): an attack motion goes on
    /// in its own update while the cooling names a target
    /// ([`Self::attack_under_command`]), and otherwise changes to
    /// `MotionMoveState`.
    pub(in crate::fight) fn cool_motion(&mut self, skill_ref: SkillRef) {
        let names_target = self
            .skill(skill_ref)
            .cooling()
            .is_some_and(|(_, named)| named.is_some());
        if let Some(actor) = self.moving_mut(skill_ref) {
            if names_target
                && actor.command.is_some()
                && actor.motion.state == MotionState::Attacking
            {
                return;
            }
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
    /// that has itself died since sends the search to every enemy
    /// ([`Simulation::select_normal_target_with_order`]).
    pub(in crate::fight) fn select_lock_replacement(
        &self,
        skill_ref: SkillRef,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<Option<FightActorRef>> {
        let prepared = self.skill(skill_ref).phase() == FightSkillPhase::Attack
            && self.prepared_at_tick_start(skill_ref);
        self.select_normal_target_with_order(skill_ref, target_search_order, !prepared)
    }

    /// `SearchLockTarget` followed by `SearchAttackTarget`, as the checker
    /// runs them; no lock found clears the targets.
    pub(in crate::fight) fn search_normal_lock_target(
        &mut self,
        skill_ref: SkillRef,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<bool> {
        if !self.search_skill_lock_target(skill_ref, target_search_order)? {
            return Ok(false);
        }
        self.search_attack_target(skill_ref);
        Ok(true)
    }

    /// `FightSkill.SearchLockTarget`: the lock changes to what the search
    /// finds and the owner's motion is handed on; the attack target is left
    /// as it was unless nothing is found. A preemptive skill leaving its idle
    /// state runs this alone, and goes on firing at the attack target its
    /// idle state took: a Tarantula's Spider Mine skill that locks a new
    /// enemy keeps its turret on the one it held.
    pub(in crate::fight) fn search_skill_lock_target(
        &mut self,
        skill_ref: SkillRef,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<bool> {
        let selected = match self.side_arm_lock_for_main(skill_ref) {
            Some(side_arm_lock) => Some(side_arm_lock),
            None => self.select_lock_replacement(skill_ref, target_search_order)?,
        };
        let idle = selected.is_none();
        let selected = if idle {
            self.select_alive_target(skill_ref, None, target_search_order)?
        } else {
            selected
        };
        let firing_at = self.skill(skill_ref).attack_target();
        let skill = self.skill_mut(skill_ref);
        skill.idle = idle;
        let Some(selected) = selected else {
            skill.drop_lock();
            self.hand_motion_after_lock_search(skill_ref);
            return Ok(false);
        };
        skill.write_lock(Some(selected));
        // `ChangeLockTarget` alone: the attack target stays, and only a skill
        // left idle has it cleared.
        skill.kept_attack_target = firing_at
            .filter(|&target| !idle && target != selected)
            .map(|target| (target, selected));
        self.hand_motion_after_lock_search(skill_ref);
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
        if let Some(actor) = self.moving_mut(skill_ref) {
            actor.lose_target_motion(true);
        }
    }

    /// Which enemy construction stands between this actor and its target.
    ///
    /// `docs/rules/constructions.md` states the rule and the readings behind
    /// it: of the enemy's constructions, the ones within reach edge to edge and
    /// within the width of the line of fire, the **nearest to the attacker** —
    /// not the nearest construction and not the one nearest the line.
    /// `WallConstructionTargetChecker.CheckWallConstruction` then asks the
    /// skill's `IsTowerAttackable` of the block it found, and an extra skill
    /// that deals nothing, a Sticky Oil Bomb's, fires past it.
    ///
    /// `slot` names a grouped slot, which asks with its own range
    /// (`FightSkill.GetAttackRange`): a Wraith's sibling gun meets a block
    /// ten metres further off than its core does.
    pub(in crate::fight) fn wall_in_the_way(
        &self,
        skill_ref: SkillRef,
        slot: Option<usize>,
        target: FightActorRef,
    ) -> Option<u64> {
        let aimed = self.fight_actor(target)?;
        self.wall_in_the_way_at(skill_ref, slot, (aimed.x_q32, aimed.z_q32))
    }

    /// [`Self::wall_in_the_way`] of the line to a point.
    pub(in crate::fight) fn wall_in_the_way_at(
        &self,
        skill_ref: SkillRef,
        slot: Option<usize>,
        (aimed_x_q32, aimed_z_q32): (i64, i64),
    ) -> Option<u64> {
        // The skill's own reach: a Centurion's Homing Missile meets a block
        // its main gun is too short for.
        let actor = self.skill_attacker(skill_ref)?;
        let range = self.slot_attack_range(skill_ref, slot);
        // A wall is considered when it is within reach edge to edge: the
        // attacker's range plus its own radius and the block's. A constant
        // allowance fits a Crawler and not a Wraith, which attacks a block 73.8
        // metres off with a reach of 60.
        let reach = space_to_q32(range.saturating_add(actor.radius));
        let width = space_to_q32(WALL_IN_THE_WAY_WIDTH);
        let mut nearest: Option<(i64, u64)> = None;
        for building in &self.buildings {
            // `PrepareWalls` takes a construction that answers
            // `IsEnableBlock`: a turret is not in the way.
            if building.building_type_id != CONSTRUCTION_BUILDING_TYPE
                || !self
                    .rvo
                    .passable_constructions
                    .contains(&building.building_id)
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
                (aimed_x_q32, aimed_z_q32),
                (building.position.x, building.position.z),
            ) > width
            {
                continue;
            }
            if nearest.is_none_or(|(best, _)| distance < best) {
                nearest = Some((distance, building.building_id));
            }
        }
        nearest
            .map(|(_, building_id)| building_id)
            .filter(|_| self.tower_attackable(skill_ref))
    }
}
