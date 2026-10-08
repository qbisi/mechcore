use super::*;

/// Where a projectile leaves from and what it carries: the attacker's side of
/// `ProjectileSystem.Create`.
#[derive(Debug, Clone, Copy)]
pub(in crate::fight) struct Launch {
    pub(in crate::fight) owner: FightActorRef,
    pub(in crate::fight) team: u32,
    pub(in crate::fight) x: i64,
    pub(in crate::fight) z: i64,
    pub(in crate::fight) x_q32: i64,
    pub(in crate::fight) y: i64,
    pub(in crate::fight) z_q32: i64,
    pub(in crate::fight) speed: i64,
    pub(in crate::fight) life: i64,
    /// Whether an interceptor may take it out of the air: the skill's
    /// `canBeIntercept`.
    pub(in crate::fight) interceptible: bool,
    pub(in crate::fight) lock_target: bool,
    /// How high the projectile climbs before it flies, in space units.
    pub(in crate::fight) climb: i64,
    /// `IAttacker.GetAttackRange`, which a climb is measured against.
    pub(in crate::fight) range: i64,
}

impl Simulation {
    /// `SkillAttackState.PerformAttack` at the attack point: the blow wound up
    /// lands, by the skill's path. Answers whether the attack point rejected
    /// it.
    pub(in crate::fight) fn release(
        &mut self,
        skill_ref: SkillRef,
        events: &mut Vec<Event>,
    ) -> Result<bool> {
        let pending = self
            .skill(skill_ref)
            .pending()
            .ok_or_else(|| Error::new("attack release has no pending action"))?;
        let release_attackable_invalid =
            self.bodyless_attackable_invalid(skill_ref, pending.target);
        if release_attackable_invalid {
            // SkillAttackState rechecks CheckAttackable and target angle at
            // the attack point. A failed check skips PerformAttack; the skill
            // phase can then finish and MotionAttackState returns to Idle in
            // the same logic update.
            let skill = self.skill_mut(skill_ref);
            skill.set_pending(None);
            skill.set_phase(FightSkillPhase::Idle);
            return Ok(true);
        }
        let attack = self
            .skill_attacker(skill_ref)
            .expect("skill owner identity is stable")
            .attack;
        let backswing_steps = native_time_units_to_steps(attack.backswing_time_units());
        let skill = self.skill_mut(skill_ref);
        let kind = skill.kind;
        skill.set_pending(None);
        skill.fire_round();
        // `SkillAttackController.PerformAttack` counts the blow as it starts.
        skill.attack_count += 1;
        skill.total_attack_count += 1;
        self.count_punch(skill_ref);
        let skill = self.skill_mut(skill_ref);
        // The backswing is cut short by the next blow: a blow fitted into its
        // interval by `SkillAttackController.PerformAttack` gives its
        // backswing what is left of the interval, and a Wasp's 1.5-second
        // backswing reads 27 ticks, its interval, in the game's own states.
        let next_attack_step = skill.next_attack_step;
        // A blow with no backswing runs its cycle out as its attacking phase
        // ends (`SkillAttackController.ChangeToIdle`), where `performCount`
        // counts it: a Farseer's two-projectile burst released on tick 2
        // reads 0 until its last projectile goes out, and 1 from tick 6.
        skill.attacking_unfinished = backswing_steps == 0;
        skill.set_backswing_finish_step((backswing_steps > 0).then(|| {
            pending
                .step
                .saturating_add(backswing_steps)
                .min(next_attack_step)
        }));
        // The skill stays in `SkillAttackState` after the blow, until a check
        // fails or the attack finishes.
        skill.set_phase(FightSkillPhase::Attack);
        match kind {
            SkillKind::Sweep => {
                let actor_id = skill_ref
                    .owner
                    .unit_id()
                    .ok_or_else(|| Error::new("a construction's sweep is not supported"))?;
                let actor = &self.actors[&actor_id];
                let aimed = actor
                    .skills
                    .main
                    .attack_target()
                    .or(actor.skills.main.lock_target);
                let sweep = super::super::sweep::Sweep::starting(
                    &actor.rules.attack,
                    actor.placement.sweep,
                    aimed,
                    actor.skills.main.total_attack_count,
                );
                if let Some(sweep) = sweep {
                    self.actors
                        .get_mut(&actor_id)
                        .expect("actor identity is stable")
                        .skills
                        .main
                        .performer = Performer::Sweep(Box::new(sweep));
                    // `SkillAttackController.ChangeToNextPhase` starts the
                    // attacking phase and updates it on the same update.
                    self.update_sweep(actor_id, events)?;
                }
            }
            SkillKind::Suicide => self.suicide(skill_ref, events)?,
            SkillKind::Around => self.around_effect(skill_ref, pending.target, events)?,
            // `FightSupportSkill.GetAttackEffect`: its blow does nothing; its
            // production line makes its units.
            SkillKind::Support => {}
            SkillKind::Strike => {
                let actor_id = skill_ref.owner.unit_id().ok_or_else(|| {
                    Error::new("a construction's skill that strikes is not supported")
                })?;
                match skill_ref.slot {
                    SkillSlot::Main => self.direct_effect(actor_id, pending.target, 0, events)?,
                    SkillSlot::Extra(_) => {
                        self.extra_direct_effect(skill_ref, pending.target, events)?;
                    }
                }
            }
            SkillKind::Laser => self.release_beam(skill_ref, pending.target, events)?,
            SkillKind::Projectile => {
                self.start_projectile_burst(skill_ref, pending.target, pending.step, events)?;
            }
            SkillKind::ControlBeam => {
                self.control_effect(skill_ref, 0, pending.target, events)?;
            }
        }
        self.finish_attacking(skill_ref);
        Ok(false)
    }

    /// `FightRocketPunchSkill.OnAttack`: a rocket punch counts its punch as
    /// the blow starts.
    fn count_punch(&mut self, skill_ref: SkillRef) {
        if let (SkillSlot::Extra(index), FightActorRef::Unit(actor_id)) =
            (skill_ref.slot, skill_ref.owner)
        {
            let extra = &mut self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .skills
                .extras[index];
            if extra.rules.rocket_punch.is_some() {
                extra.punches += 1;
            }
        }
    }

    /// `SkillAttackController.ChangeToIdle` after an attacking phase with no
    /// backswing: once the performer's work is done, the blow has run its
    /// cycle out and `performCount` counts it.
    pub(in crate::fight) fn finish_attacking(&mut self, skill_ref: SkillRef) {
        let skill = self.skill_mut(skill_ref);
        if skill.attacking_unfinished && skill.performer.done() {
            skill.attacking_unfinished = false;
            skill.perform_count += 1;
        }
    }

    /// A laser skill's blow, the skill's own: the `attack_count`th of its
    /// attack, at what it fires at.
    fn release_beam(
        &mut self,
        skill_ref: SkillRef,
        target: FightActorRef,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let actor_id = skill_ref
            .owner
            .unit_id()
            .ok_or_else(|| Error::new("a construction's laser is not supported"))?;
        let target_was_alive = self.fight_actor_is_alive(target);
        let target_was_lock = self.skill(skill_ref).lock_target == Some(target);
        let blow = usize::try_from(self.skill(skill_ref).attack_count).unwrap_or(0);
        self.laser_effect(skill_ref, 0, blow, target, events)?;
        // A building lock the beam fells stops the motion on that tick,
        // whatever building it is: the Steel Ball that fells a tower in the
        // tower-loss fights, or the turret of `laser-fells-turret.yaml`,
        // reads idle on that tick. A block that only stood in the way is
        // left to the fallen-block rule on the next tick, as a blow's is:
        // the Steel Ball of `wall-laser.yaml` that fells block 4 reads
        // attacking on that tick, idle on the next. So is a unit lock the
        // beam kills, as a blow's dead target is: the Steel Ball of replay
        // 201370830 round 6 reads attacking on the tick its beam kills its
        // Sledgehammer. Only the skill the motion follows stops it.
        if target_was_alive
            && !self.fight_actor_is_alive(target)
            && target_was_lock
            && matches!(target, FightActorRef::Building(_))
            && self.actors[&actor_id].motion.attacker == skill_ref.slot
        {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.motion.state = MotionState::Idle;
        }
        Ok(())
    }

    pub(in crate::fight) fn start_projectile_burst(
        &mut self,
        skill_ref: SkillRef,
        target: FightActorRef,
        step: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let target_view = self
            .fight_actor(target)
            .ok_or_else(|| Error::new("projectile target is absent"))?;
        let target_x_q32 = target_view.x_q32;
        let target_z_q32 = target_view.z_q32;
        let attacker = self
            .skill_attacker(skill_ref)
            .ok_or_else(|| Error::new("projectile owner is absent"))?;
        let attack = attacker.attack;
        if let AttackPath::Projectile {
            evenly_allocate_targets: true,
            extra_search_range,
            ..
        } = attack.path
        {
            return self.start_evenly_allocated_burst(
                skill_ref,
                target,
                (target_x_q32, target_z_q32),
                crate::rules::metres_q32(extra_search_range),
                step,
                events,
            );
        }
        let count = attacker.projectile_count();
        let interval = attacker.projectile_interval_steps();
        let radius = attacker.projectile_target_offset_radius();
        // A standalone skill fires its own weapons, `weaponCountPerSkill` of
        // its row's; any other all of them. Its projectiles take turns
        // between two only through a `MultiAttackTargetPositionController`,
        // which the performer makes for a burst that lands about its target
        // (`randomTargetRange` above zero) and which splits its offsets
        // between two weapons alone (`GetAndDeletePositionOffsets`); any
        // other burst fires from the first: a Sabertooth's Doubleshot, both
        // from weapon 0.
        let weapon_count = usize::try_from(if attack.weapons.mode == WeaponMode::Standalone {
            attack.weapons.per_skill
        } else {
            attack.weapons.count()
        })
        .expect("u32 weapon count fits the supported host");
        let turns = if radius > 0 && weapon_count == 2 {
            2
        } else {
            1
        };
        let climb_target = self.climb_target(target)?;
        // An extra skill fires from the first of its row's weapons it was
        // made for.
        let (first_weapon, skill_slot) = (
            match skill_ref.slot {
                SkillSlot::Main => 0,
                SkillSlot::Extra(index) => self.skills(skill_ref.owner).extras[index].weapon,
            },
            self.skill_slot(skill_ref),
        );
        let source_y = self
            .skill_attacker(skill_ref)
            .ok_or_else(|| Error::new("projectile owner is absent"))?
            .y;
        // `OnStartFirstPerform` prepares the burst's points about where the
        // skill aims at its target (`CalculateAttackPosition`).
        let (aimed_x_q32, aimed_y_q32, aimed_z_q32) =
            self.attack_position(skill_ref.owner, skill_slot, target, 0, true)?;
        let offsets = self.projectile_target_offsets(
            skill_ref,
            aimed_x_q32,
            aimed_z_q32,
            (space_to_q32(source_y), aimed_y_q32),
            count,
            radius,
        )?;
        let mut releases =
            offsets
                .into_iter()
                .enumerate()
                .map(|(index, (x, z))| PendingProjectileRelease {
                    step: step.saturating_add(interval.saturating_mul(index as u64)),
                    target_kind: target.kind(),
                    target: target.id(),
                    target_x_q32: aimed_x_q32.saturating_add(x),
                    target_y_q32: aimed_y_q32,
                    target_z_q32: aimed_z_q32.saturating_add(z),
                    offset_x_q32: x,
                    offset_z_q32: z,
                    climb_target,
                    aims_at_release: radius == 0,
                    weapon_index: first_weapon + index % turns,
                    skill_slot,
                });
        let first = releases
            .next()
            .ok_or_else(|| Error::new("projectile burst contains no release"))?;
        let Performer::Projectile { pending, .. } = &mut self.skill_mut(skill_ref).performer else {
            return Err(Error::new("a burst needs a projectile performer"));
        };
        pending.extend(releases);
        self.release_pending_projectile(skill_ref, first, events)
    }

    /// A burst whose row allocates its projectiles evenly
    /// (`EvenlyAllocatedAttackTargetPositionController.Prepare`, on the first
    /// perform): the other sides' units within the skill's range and its
    /// extra search range of the unit, each measured less both radii
    /// (`RangeTargetCalculator.CalculateRangeTargets`), in the order their
    /// trees answer, shuffled by the unit's side's stream (`ShuffleSync`),
    /// or the attack target alone when none is. Each takes the burst's count
    /// over theirs, and the rest go one each to units drawn from them
    /// (`GetRandomTargetsByRemain`). The offsets are drawn then, round by
    /// round over the units and then for the units drawn, each about where
    /// the attack target stands (`RandomInsideSphere`). The projectiles
    /// leave on the burst's schedule, and each takes its unit as it leaves
    /// ([`Simulation::allocate_evenly`]).
    fn start_evenly_allocated_burst(
        &mut self,
        skill_ref: SkillRef,
        target: FightActorRef,
        target_q32: (i64, i64),
        extra_range_q32: i64,
        step: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let source = self
            .skill_attacker(skill_ref)
            .ok_or_else(|| Error::new("projectile owner is absent"))?;
        let (team, x_q32, z_q32) = (source.team, source.x_q32, source.z_q32);
        let self_radius_q32 = space_to_q32(source.radius);
        let range_q32 = space_to_q32(source.attack_range).saturating_add(extra_range_q32);
        let targets_accepted = source.targets;
        let count = source.projectile_count();
        let interval = source.projectile_interval_steps();
        let radius_centimeters = i32::try_from(source.projectile_target_offset_radius() / 10)
            .map_err(|_| Error::new("projectile target offset radius exceeds native range"))?;
        let mut targets = self.range_targets(
            team,
            (x_q32, z_q32),
            self_radius_q32,
            range_q32,
            targets_accepted,
        );
        let random = self
            .team_random
            .get_mut(&team)
            .ok_or_else(|| Error::new("projectile owner team random stream is absent"))?;
        // `IListExtensions.ShuffleSync`: each place swaps with one drawn
        // from the whole list.
        let len = i32::try_from(targets.len()).map_err(|_| Error::new("too many targets"))?;
        for index in 0..targets.len() {
            let drawn = usize::try_from(random.next_between_inclusive(0, len - 1))
                .expect("a draw within the list is an index");
            targets.swap(index, drawn);
        }
        if targets.is_empty()
            && let FightActorRef::Unit(id) = target
        {
            targets.push(id);
        }
        let each = count.checked_div(targets.len()).unwrap_or(0);
        let left = count - each * targets.len();
        // `GetRandomTargetsByRemain`: each drawn from those not drawn yet
        // (`RandomElementSync`).
        let mut undrawn = targets.clone();
        let mut drawn = Vec::with_capacity(left);
        for _ in 0..left.min(undrawn.len()) {
            let len = i32::try_from(undrawn.len()).map_err(|_| Error::new("too many targets"))?;
            let index = usize::try_from(random.next_between_inclusive(0, len - 1))
                .expect("a draw within the list is an index");
            drawn.push(undrawn.remove(index));
        }
        let mut offsets = BTreeMap::<u64, Vec<(i64, i64)>>::new();
        for _ in 0..each {
            for &unit in &targets {
                offsets
                    .entry(unit)
                    .or_default()
                    .push(random_inside_sphere(random, radius_centimeters));
            }
        }
        for &unit in &drawn {
            offsets
                .entry(unit)
                .or_default()
                .push(random_inside_sphere(random, radius_centimeters));
        }
        let skill_slot = self.skill_slot(skill_ref);
        let first_weapon = match skill_ref.slot {
            SkillSlot::Main => 0,
            SkillSlot::Extra(index) => self.skills(skill_ref.owner).extras[index].weapon,
        };
        let Performer::Projectile { pending, evenly } = &mut self.skill_mut(skill_ref).performer
        else {
            return Err(Error::new("a burst needs a projectile performer"));
        };
        *evenly = Some(Box::new(EvenlyAllocated {
            targets,
            offsets,
            weapon: 0,
            last_attack: target_q32,
        }));
        // What each projectile fires at is taken as it leaves.
        let placeholder = |index: usize| PendingProjectileRelease {
            step: step.saturating_add(interval.saturating_mul(index as u64)),
            target_kind: target.kind(),
            target: target.id(),
            target_x_q32: target_q32.0,
            target_y_q32: 0,
            target_z_q32: target_q32.1,
            offset_x_q32: 0,
            offset_z_q32: 0,
            climb_target: (target_q32.0, target_q32.1, 0),
            aims_at_release: false,
            weapon_index: first_weapon,
            skill_slot,
        };
        pending.extend((1..count).map(placeholder));
        self.release_pending_projectile(skill_ref, placeholder(0), events)
    }

    /// `RangeTargetCalculator.CalculateRangeTargets` of units alone, fully
    /// visible: the other sides' units, side by side in the order their trees
    /// answer a square twice the range wide, each whose distance from the
    /// point less its radius and the source's is within the range
    /// (`FightCalculator.IsInRange2D`).
    fn range_targets(
        &self,
        team: u32,
        (x_q32, z_q32): (i64, i64),
        self_radius_q32: i64,
        range_q32: i64,
        accepted: AttackTargets,
    ) -> Vec<u64> {
        self.target_quadtrees
            .iter()
            .filter(|(side, _)| **side != team)
            .flat_map(|(_, tree)| tree.query_square(x_q32, z_q32, range_q32.saturating_mul(2)))
            .filter_map(|candidate| {
                let FightActorRef::Unit(id) = candidate else {
                    return None;
                };
                let actor = self.actors.get(&id)?;
                let distance = native_q32_magnitude(
                    actor.x_q32.saturating_sub(x_q32),
                    actor.z_q32.saturating_sub(z_q32),
                )
                .saturating_sub(space_to_q32(actor.rules.collision_radius()))
                .saturating_sub(self_radius_q32);
                (actor.alive()
                    && actor.visibility == Visibility::Normal
                    && accepted.accepts(actor.rules.domain)
                    && fpoint_less_or_equal(distance, range_q32))
                .then_some(id)
            })
            .collect()
    }

    /// `EvenlyAllocatedAttackTargetPositionController` as a projectile
    /// leaves: `UpdateCurrentTarget` takes the first unit of its list that
    /// lives, dropping the dead, and puts it last; `GetWeaponIndex` hands out
    /// the two weapons in turn; `GetTargetPosition` aims at where the unit
    /// stands now; and `GetAndDeletePositionOffsets` takes the unit's next
    /// offset, none when its own have run out. A burst whose every unit is
    /// gone is not measured. A burst allocated otherwise leaves as it was
    /// scheduled.
    fn allocate_evenly(
        &mut self,
        skill_ref: SkillRef,
        pending: PendingProjectileRelease,
    ) -> Result<PendingProjectileRelease> {
        let alive =
            |simulation: &Self, id: u64| simulation.actors.get(&id).is_some_and(Actor::alive);
        let Performer::Projectile {
            evenly: Some(evenly),
            ..
        } = &self.skill(skill_ref).performer
        else {
            return Ok(pending);
        };
        let mut targets = evenly.targets.clone();
        let current = loop {
            if targets.is_empty() {
                break None;
            }
            let first = targets.remove(0);
            if alive(self, first) {
                targets.push(first);
                break Some(first);
            }
        };
        let Some(current) = current else {
            return Err(Error::new(
                "an evenly allocated burst whose every unit is gone is not measured",
            ));
        };
        let actor = &self.actors[&current];
        let (x_q32, z_q32) = (actor.x_q32, actor.z_q32);
        let height = unit_height(actor.rules.domain);
        // `GetTargetPosition`: where the skill aims at the unit
        // (`CalculateAttackPosition`, reaching its extra search range too, on
        // the ground), its offset added.
        let extra_search_range_q32 = match self
            .skill_attacker(skill_ref)
            .ok_or_else(|| Error::new("projectile owner is absent"))?
            .attack
            .path
        {
            AttackPath::Projectile {
                extra_search_range, ..
            } => crate::rules::metres_q32(extra_search_range),
            _ => 0,
        };
        let (aimed_x_q32, aimed_y_q32, aimed_z_q32) = self.attack_position(
            skill_ref.owner,
            pending.skill_slot,
            FightActorRef::Unit(current),
            extra_search_range_q32,
            false,
        )?;
        let Performer::Projectile {
            evenly: Some(evenly),
            ..
        } = &mut self.skill_mut(skill_ref).performer
        else {
            unreachable!("checked above");
        };
        evenly.targets = targets;
        let weapon = evenly.weapon;
        evenly.weapon = usize::from(weapon == 0);
        evenly.last_attack = (x_q32, z_q32);
        let (offset_x_q32, offset_z_q32) = match evenly.offsets.get_mut(&current) {
            Some(offsets) if !offsets.is_empty() => {
                let offset = offsets.remove(0);
                if offsets.is_empty() {
                    evenly.offsets.remove(&current);
                }
                offset
            }
            _ => (0, 0),
        };
        Ok(PendingProjectileRelease {
            target_kind: ObjectKind::Unit,
            target: current,
            target_x_q32: aimed_x_q32.saturating_add(offset_x_q32),
            target_y_q32: aimed_y_q32,
            target_z_q32: aimed_z_q32.saturating_add(offset_z_q32),
            offset_x_q32,
            offset_z_q32,
            climb_target: (x_q32, z_q32, height),
            weapon_index: pending.weapon_index + weapon,
            ..pending
        })
    }

    /// Where a burst's target stands as the burst begins, and its height:
    /// what every projectile of the burst measures its climb to. A unit of
    /// the side that updates first has already moved this tick: a Farseer's
    /// burst at a Rhino closing on it climbs to the height the Rhino's new
    /// position gives, both its projectiles alike.
    fn climb_target(&self, target: FightActorRef) -> Result<(i64, i64, i64)> {
        let view = self
            .fight_actor(target)
            .ok_or_else(|| Error::new("projectile target is absent"))?;
        let height = match target {
            FightActorRef::Unit(id) => unit_height(self.actors[&id].rules.domain),
            FightActorRef::Building(_) => 0,
        };
        Ok((view.x_q32, view.z_q32, height))
    }

    /// How high a projectile climbs before it flies, above where it leaves:
    /// `ProjectileSystem.Create` scales the pre-flight height by the distance
    /// from where the projectile leaves to the burst's target over the attack
    /// range, to the whole height at the range and beyond. It is measured
    /// for each projectile as it is created, from where its owner stands
    /// then: an Overlord pushed aside between two releases of one burst
    /// climbs its later projectiles to another height.
    ///
    /// `Create` hands every projectile of a skill that is not the main one
    /// such a flight, whatever its height, and the projectile's first update
    /// is its climb (`FightProjectile.IsFlying`): a Sabertooth's Secondary
    /// Armament shot stands where it left on the tick it is released.
    /// A projectile a grouped slot has just released, climbing first where
    /// the slot is a row's that joined the main skill's group, as an extra
    /// skill's projectile does: it leaves on the tick after its release.
    pub(in crate::fight) fn climb_joined_slot_projectile(
        &mut self,
        skill_ref: SkillRef,
        slot: usize,
        target: FightActorRef,
    ) -> Result<()> {
        if self.skill(skill_ref).joined(slot).is_none() {
            return Ok(());
        }
        let climb_target = self.climb_target(target)?;
        let climb_q32 = self.projectile_climb_q32(skill_ref, slot, climb_target)?;
        let projectile = self
            .projectiles
            .last_mut()
            .expect("a projectile was just released");
        projectile.climb_to_q32 =
            climb_q32.map(|climb_q32| projectile.y_q32.saturating_add(climb_q32));
        Ok(())
    }

    fn projectile_climb_q32(
        &self,
        skill_ref: SkillRef,
        slot: usize,
        (target_x_q32, target_z_q32, target_y): (i64, i64, i64),
    ) -> Result<Option<i64>> {
        let source = self
            .skill_attacker(skill_ref)
            .ok_or_else(|| Error::new("projectile owner is absent"))?
            .launch();
        // A slot of a row that joined the main skill's group is its row's
        // skill, and climbs as an extra skill's projectile does.
        if source.climb <= 0
            && skill_ref.slot == SkillSlot::Main
            && self.skill(skill_ref).joined(slot).is_none()
        {
            return Ok(None);
        }
        let distance_q32 = native_q32_magnitude_3d(
            target_x_q32.saturating_sub(source.x_q32),
            space_to_q32(target_y).saturating_sub(space_to_q32(source.y)),
            target_z_q32.saturating_sub(source.z_q32),
        );
        let climb_q32 = space_to_q32(source.climb);
        let ratio_q32 = q32_div(distance_q32, space_to_q32(source.range));
        Ok(Some(q32_mul(ratio_q32, climb_q32).min(climb_q32)))
    }

    pub(in crate::fight) fn projectile_target_offsets(
        &mut self,
        skill_ref: SkillRef,
        target_x_q32: i64,
        target_z_q32: i64,
        (source_y_q32, target_y_q32): (i64, i64),
        count: usize,
        radius: i64,
    ) -> Result<Vec<(i64, i64)>> {
        if radius == 0 {
            return Ok(vec![(0, 0); count]);
        }
        // The skill's own weapons split its burst: Electromagnetic Barrage's
        // two launchers, whatever the Melting Point's main beam has.
        let source = self
            .skill_attacker(skill_ref)
            .ok_or_else(|| Error::new("projectile owner is absent"))?;
        let team = source.team;
        let source_x_q32 = source.x_q32;
        let source_z_q32 = source.z_q32;
        let weapons = &source.attack.weapons;
        let weapon_count = if weapons.mode == WeaponMode::Standalone {
            weapons.per_skill
        } else {
            weapons.count()
        };
        let radius_centimeters = i32::try_from(radius / 10)
            .map_err(|_| Error::new("projectile target offset radius exceeds native range"))?;
        let random = self
            .team_random
            .get_mut(&team)
            .ok_or_else(|| Error::new("projectile owner team random stream is absent"))?;
        let mut offsets = Vec::with_capacity(count);
        for _ in 0..count {
            offsets.push(random_inside_sphere(random, radius_centimeters));
        }
        if weapon_count == 2 && offsets.len() >= 2 {
            offsets = split_between_two_weapons(
                offsets,
                (source_x_q32, source_y_q32, source_z_q32),
                (target_x_q32, target_y_q32, target_z_q32),
                count,
            )?;
        } else {
            // One weapon takes the offsets last drawn first: a Phantom Ray's
            // first projectile lands the second offset its burst drew.
            offsets.reverse();
        }
        Ok(offsets)
    }

    /// A standalone weapon's skill releasing its blow, as the unit's main
    /// skill does: one projectile at a point its offset draw gives.
    pub(in crate::fight) fn release_standalone_projectile(
        &mut self,
        actor_id: u64,
        slot: usize,
        target: FightActorRef,
        step: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let owner = FightActorRef::Unit(actor_id);
        let attacker = self
            .attacker(owner)
            .ok_or_else(|| Error::new("projectile owner is absent"))?;
        let radius = attacker.projectile_target_offset_radius();
        let source_y = attacker.y;
        let (target_x_q32, target_y_q32, target_z_q32) =
            self.attack_position(owner, slot, target, 0, true)?;
        let climb_target = self.climb_target(target)?;
        let (x, z) = self
            .projectile_target_offsets(
                SkillRef::main(owner),
                target_x_q32,
                target_z_q32,
                (space_to_q32(source_y), target_y_q32),
                1,
                radius,
            )?
            .into_iter()
            .next()
            .ok_or_else(|| Error::new("a standalone blow draws one offset"))?;
        self.release_pending_projectile(
            SkillRef::main(owner),
            PendingProjectileRelease {
                step,
                target_kind: target.kind(),
                target: target.id(),
                target_x_q32: target_x_q32.saturating_add(x),
                target_y_q32,
                target_z_q32: target_z_q32.saturating_add(z),
                offset_x_q32: x,
                offset_z_q32: z,
                climb_target,
                aims_at_release: false,
                weapon_index: slot,
                skill_slot: slot,
            },
            events,
        )
    }

    /// `ProjectileAttackPerformer.CalculateAttackPosition`: where a
    /// projectile skill aims at a target. The way from the owner to the
    /// target is held to the skill's reach (`FVector3.ClampMagnitude`): its
    /// range, both radii and `extra_range`, and where only one of the two
    /// flies, the hypotenuse of that and the air height. With `using_3d` the
    /// way runs between their positions in space; without it from the
    /// target's ground point, to the owner's ground point when neither flies
    /// and to its point in space otherwise. A skill that attacks only the
    /// air aims at the air height. A target within reach is aimed at where
    /// it stands; an Overlord firing at the shield of a Fortress beyond its
    /// reach aims at the point of its reach on the way. `skill_slot` is the
    /// build's index of the skill that fires, whose own range is its reach.
    pub(in crate::fight) fn attack_position(
        &self,
        owner: FightActorRef,
        skill_slot: usize,
        target: FightActorRef,
        extra_range_q32: i64,
        using_3d: bool,
    ) -> Result<(i64, i64, i64)> {
        let (skill_ref, range_q32) = match owner {
            FightActorRef::Unit(id) => {
                let (held_by, offset) = self.actors[&id].skills.at_slot(skill_slot);
                let skill_ref = SkillRef {
                    owner,
                    slot: held_by,
                };
                (skill_ref, self.slot_attack_range_q32(skill_ref, offset))
            }
            FightActorRef::Building(_) => {
                let skill_ref = SkillRef::main(owner);
                let attacker = self
                    .skill_attacker(skill_ref)
                    .ok_or_else(|| Error::new("projectile owner is absent"))?;
                (skill_ref, space_to_q32(attacker.attack_range))
            }
        };
        let attacker = self
            .skill_attacker(skill_ref)
            .ok_or_else(|| Error::new("projectile owner is absent"))?;
        let view = self
            .fight_actor(target)
            .ok_or_else(|| Error::new("projectile target is absent"))?;
        let owner_domain = match owner {
            FightActorRef::Unit(id) => self.actors[&id].rules.domain,
            FightActorRef::Building(_) => UnitDomain::Ground,
        };
        let owner_flies = owner_domain == UnitDomain::Air;
        let target_flies = view.domain == UnitDomain::Air;
        let mut reach_q32 = range_q32
            .saturating_add(space_to_q32(attacker.radius))
            .saturating_add(space_to_q32(view.radius))
            .saturating_add(extra_range_q32);
        if owner_flies != target_flies {
            let square = 2 << 32;
            reach_q32 = fpcs_sqrt_fastest(
                fpcs_pow_fastest(reach_q32, square)
                    .saturating_add(fpcs_pow_fastest(space_to_q32(AIR_UNIT_HEIGHT), square)),
            );
        }
        let from = (
            attacker.x_q32,
            if using_3d || owner_flies || target_flies {
                space_to_q32(unit_height(owner_domain))
            } else {
                0
            },
            attacker.z_q32,
        );
        let to = (
            view.x_q32,
            if using_3d {
                space_to_q32(unit_height(view.domain))
            } else {
                0
            },
            view.z_q32,
        );
        let way = clamp_magnitude_3d(
            (
                to.0.saturating_sub(from.0),
                to.1.saturating_sub(from.1),
                to.2.saturating_sub(from.2),
            ),
            reach_q32,
        );
        let y_q32 = if attacker.targets.air && !attacker.targets.ground {
            space_to_q32(AIR_UNIT_HEIGHT)
        } else {
            from.1.saturating_add(way.1)
        };
        Ok((
            from.0.saturating_add(way.0),
            y_q32,
            from.2.saturating_add(way.2),
        ))
    }

    pub(in crate::fight) fn release_projectile(
        &mut self,
        skill_ref: SkillRef,
        target_id: u64,
        skill_slot: usize,
        weapon_index: usize,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let aimed = self.attack_position(
            skill_ref.owner,
            skill_slot,
            FightActorRef::Unit(target_id),
            0,
            true,
        )?;
        self.release_projectile_at(
            skill_ref,
            target_id,
            aimed,
            skill_slot,
            weapon_index,
            events,
        )
    }

    pub(in crate::fight) fn release_pending_projectile(
        &mut self,
        skill_ref: SkillRef,
        pending: PendingProjectileRelease,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let pending = self.allocate_evenly(skill_ref, pending)?;
        match pending.target_kind {
            ObjectKind::Unit => {
                // A projectile leaves for where its burst aimed it, the
                // target's position when the burst began plus its offset,
                // and one that follows its target takes the target's position
                // up again from its first update: a Farseer's second shot,
                // which climbs first, still names the point the burst aimed
                // at when it levels off.
                let aimed = if pending.aims_at_release && self.actors.contains_key(&pending.target)
                {
                    self.attack_position(
                        skill_ref.owner,
                        pending.skill_slot,
                        FightActorRef::Unit(pending.target),
                        0,
                        true,
                    )?
                } else {
                    (
                        pending.target_x_q32,
                        pending.target_y_q32,
                        pending.target_z_q32,
                    )
                };
                let climb_q32 =
                    self.projectile_climb_q32(skill_ref, pending.skill_slot, pending.climb_target)?;
                self.release_projectile_at(
                    skill_ref,
                    pending.target,
                    aimed,
                    pending.skill_slot,
                    pending.weapon_index,
                    events,
                )?;
                let projectile = self
                    .projectiles
                    .last_mut()
                    .expect("a projectile was just released");
                if projectile.lock_target {
                    projectile.offset_x_q32 = pending.offset_x_q32;
                    projectile.offset_z_q32 = pending.offset_z_q32;
                }
                projectile.climb_to_q32 =
                    climb_q32.map(|climb_q32| projectile.y_q32.saturating_add(climb_q32));
                Ok(())
            }
            ObjectKind::Building => {
                let building = self
                    .buildings
                    .iter()
                    .find(|building| building.building_id == pending.target)
                    .ok_or_else(|| Error::new("projectile building target is absent"))?;
                let radius = building_radius(building);
                let climb_q32 =
                    self.projectile_climb_q32(skill_ref, pending.skill_slot, pending.climb_target)?;
                self.release_projectile_to(
                    skill_ref,
                    ObjectKind::Building,
                    pending.target,
                    (
                        pending.target_x_q32,
                        pending.target_y_q32,
                        pending.target_z_q32,
                    ),
                    radius,
                    pending.skill_slot,
                    pending.weapon_index,
                    events,
                )?;
                // A projectile that climbs first climbs at a block as at a
                // unit: an extra skill's always does.
                let projectile = self
                    .projectiles
                    .last_mut()
                    .expect("a projectile was just released");
                if projectile.lock_target {
                    projectile.offset_x_q32 = pending.offset_x_q32;
                    projectile.offset_z_q32 = pending.offset_z_q32;
                }
                projectile.climb_to_q32 =
                    climb_q32.map(|climb_q32| projectile.y_q32.saturating_add(climb_q32));
                Ok(())
            }
            ObjectKind::Projectile | ObjectKind::Shield | ObjectKind::Terrain => {
                Err(Error::new("projectile target kind is unsupported"))
            }
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the parameters mirror the native projectile release call"
    )]
    pub(in crate::fight) fn release_projectile_at(
        &mut self,
        skill_ref: SkillRef,
        target_id: u64,
        aimed: (i64, i64, i64),
        skill_slot: usize,
        weapon_index: usize,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let target_radius = self.actors[&target_id].rules.collision_radius();
        self.release_projectile_to(
            skill_ref,
            ObjectKind::Unit,
            target_id,
            aimed,
            target_radius,
            skill_slot,
            weapon_index,
            events,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::fight) fn release_projectile_to(
        &mut self,
        skill_ref: SkillRef,
        target_kind: ObjectKind,
        target_id: u64,
        aimed: (i64, i64, i64),
        target_radius: i64,
        skill_slot: usize,
        weapon_index: usize,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let attacker = self
            .skill_attacker(skill_ref)
            .ok_or_else(|| Error::new("projectile owner is absent"))?;
        // The weapon is fired by its position and named by the build's index.
        let weapon = attacker.attack.weapons.index(weapon_index);
        let source = attacker.launch();
        self.launch_projectile(
            source,
            target_kind,
            target_id,
            aimed,
            target_radius,
            skill_slot,
            weapon,
            events,
        )
    }

    /// Puts a projectile in flight from its source at a target, and records
    /// its release: `ProjectileSystem.Create` for any `IAttacker`, a unit's
    /// skill or a construction's.
    #[allow(
        clippy::too_many_arguments,
        reason = "the parameters mirror the native projectile release call"
    )]
    pub(in crate::fight) fn launch_projectile(
        &mut self,
        source: Launch,
        target_kind: ObjectKind,
        target_id: u64,
        (target_x_q32, target_y_q32, target_z_q32): (i64, i64, i64),
        target_radius: i64,
        skill_slot: usize,
        weapon: i32,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let projectile_id = self.ids.objects.allocate_object(ObjectKind::Projectile)?.id;
        // `FightProjectile.Init`: its data source's `GetAttackRange()`, the
        // releasing slot's own: a Wraith's slot reaches 85 metres where its
        // main skill reaches 75.
        let data_source_range_q32 = match source.owner {
            FightActorRef::Unit(id) => {
                let (held_by, offset) = self.actors[&id].skills.at_slot(skill_slot);
                self.slot_attack_range_q32(
                    SkillRef {
                        owner: source.owner,
                        slot: held_by,
                    },
                    offset,
                )
            }
            FightActorRef::Building(_) => space_to_q32(source.range),
        };
        let skill_slot =
            u16::try_from(skill_slot).map_err(|_| Error::new("skill slot exceeds u16"))?;
        let projectile = Projectile {
            id: projectile_id,
            team: source.team,
            shooter: Shooter::Actor(source.owner),
            skill_slot,
            target_kind,
            target: target_id,
            x: source.x,
            y: source.y,
            z: source.z,
            x_q32: source.x_q32,
            y_q32: space_to_q32(source.y),
            z_q32: source.z_q32,
            cached_target_x: q32_to_space_rounded(target_x_q32),
            cached_target_y: q32_to_space_rounded(target_y_q32),
            cached_target_z: q32_to_space_rounded(target_z_q32),
            cached_target_x_q32: target_x_q32,
            cached_target_y_q32: target_y_q32,
            cached_target_z_q32: target_z_q32,
            cached_target_radius: target_radius,
            speed: source.speed,
            life: source.life,
            max_life: source.life,
            interceptible: source.interceptible,
            sources: Vec::new(),
            locked_by: Vec::new(),
            lock_target: source.lock_target,
            offset_x_q32: 0,
            offset_z_q32: 0,
            climb_to_q32: None,
            spawn_shields: Vec::new(),
            absorbed_by: None,
            move_direction: (0, 0, 0),
            move_range_q32: data_source_range_q32.saturating_add(space_to_q32(target_radius)),
        };
        let mut projectile = projectile;
        // `ProjectileController.Init`: the enemy shields that already hold it.
        projectile.spawn_shields = self.enemy_shields_at(
            source.team,
            projectile.x_q32,
            projectile.y_q32,
            projectile.z_q32,
        );
        let projectile_ref = projectile.object_ref();
        events.push(event(
            Some(projectile_ref),
            Some(source.owner.object_ref()),
            Some(source.team),
            Some(ObjectRef::new(target_kind, target_id)),
            EventPayload::ProjectileReleased {
                skill_slot: Some(skill_slot),
                weapon_index: Some(weapon),
            },
        ));
        self.projectiles.push(projectile);
        Ok(())
    }
}

/// How two weapons share a burst's offsets: each weapon takes the half on its
/// side of the line from the weapon to the target, ordered by the angle it
/// sees, and they take turns from the ends of their halves.
/// `FightUtility.RandomInsideSphere` about a point, less the point: an
/// offset within the radius, in centimetres, its `x` and `z` drawn first and
/// then the length it is held to.
fn random_inside_sphere(
    random: &mut super::super::random::GrRandom,
    radius_centimeters: i32,
) -> (i64, i64) {
    let x_centimeters = random.next_between_inclusive(-radius_centimeters, radius_centimeters);
    let z_centimeters = random.next_between_inclusive(-radius_centimeters, radius_centimeters);
    let clamp_centimeters = random.next_between_inclusive(0, radius_centimeters - 1);
    let x_q32 = q32_mul(i64::from(x_centimeters) << 32, C0_01_RAW);
    let z_q32 = q32_mul(i64::from(z_centimeters) << 32, C0_01_RAW);
    let clamp_q32 = q32_mul(i64::from(clamp_centimeters) << 32, C0_01_RAW);
    clamp_magnitude_q32_raw(x_q32, z_q32, clamp_q32)
}

fn split_between_two_weapons(
    mut offsets: Vec<(i64, i64)>,
    (source_x_q32, source_y_q32, source_z_q32): (i64, i64, i64),
    (target_x_q32, target_y_q32, target_z_q32): (i64, i64, i64),
    count: usize,
) -> Result<Vec<(i64, i64)>> {
    let height_q32 = target_y_q32.saturating_sub(source_y_q32);
    let direction_x = i128::from(target_x_q32.saturating_sub(source_x_q32));
    let direction_z = i128::from(target_z_q32.saturating_sub(source_z_q32));
    offsets.sort_by(|&(left_x, left_z), &(right_x, right_z)| {
        let angle_parts = |offset_x: i64, offset_z: i64| {
            let value_x = i128::from(
                target_x_q32
                    .saturating_add(offset_x)
                    .saturating_sub(source_x_q32),
            );
            let value_z = i128::from(
                target_z_q32
                    .saturating_add(offset_z)
                    .saturating_sub(source_z_q32),
            );
            // The build passes Cross(up, targetDirection) first and the
            // main-weapon world position second to FPlane(position, normal).
            // The resulting plane normal is therefore the absolute source
            // position, not the lateral target-direction normal.
            let direction_x = i64::try_from(direction_x)
                .expect("projectile target direction remains in Q32 range");
            let direction_z = i64::try_from(direction_z)
                .expect("projectile target direction remains in Q32 range");
            let plane_position_x = direction_z;
            let plane_position_z = direction_x.saturating_neg();
            let absolute_value_x = target_x_q32.saturating_add(offset_x);
            let absolute_value_z = target_z_q32.saturating_add(offset_z);
            let plane_distance = q32_mul(plane_position_x, source_x_q32)
                .saturating_add(q32_mul(plane_position_z, source_z_q32));
            // In three dimensions, which is where a flier's weapon and a
            // flying target meet: two Overlords shooting at each other
            // split their offsets by it.
            let plane_side = q32_mul(source_x_q32, absolute_value_x)
                .saturating_add(q32_mul(source_y_q32, target_y_q32))
                .saturating_add(q32_mul(source_z_q32, absolute_value_z))
                .saturating_sub(plane_distance);
            let value_x =
                i64::try_from(value_x).expect("projectile offset direction remains in Q32 range");
            let value_z =
                i64::try_from(value_z).expect("projectile offset direction remains in Q32 range");
            // The directions are three-dimensional, from the weapon to a
            // point on the target's height: an Overlord shooting down
            // at a Crawler orders its offsets by the angles it sees.
            let height_squared = q32_mul(height_q32, height_q32);
            let direction_squared = q32_mul(direction_x, direction_x)
                .saturating_add(height_squared)
                .saturating_add(q32_mul(direction_z, direction_z));
            let value_squared = q32_mul(value_x, value_x)
                .saturating_add(height_squared)
                .saturating_add(q32_mul(value_z, value_z));
            let magnitude_product =
                if direction_squared.saturating_add(value_squared) < 0x1_6A09_0000_0001 {
                    fpcs_sqrt_fastest(q32_mul(direction_squared, value_squared))
                } else {
                    q32_mul(
                        fpcs_sqrt_fastest(direction_squared),
                        fpcs_sqrt_fastest(value_squared),
                    )
                };
            let dot = q32_mul(direction_x, value_x)
                .saturating_add(height_squared)
                .saturating_add(q32_mul(direction_z, value_z));
            let cosine_q32 = q32_div(dot, magnitude_product).clamp(-Q32_ONE, Q32_ONE);
            let angle = fpcs_acos_fastest(cosine_q32);
            if plane_side > 0 { angle } else { -angle }
        };
        angle_parts(left_x, left_z).cmp(&angle_parts(right_x, right_z))
    });
    let half = offsets.len() / 2;
    let mut weapons = [offsets[half..].to_vec(), offsets[..half].to_vec()];
    let mut weapon_index = 0;
    offsets.clear();
    while offsets.len() < count {
        offsets.push(
            weapons[weapon_index]
                .pop()
                .ok_or_else(|| Error::new("projectile weapon offset list is empty"))?,
        );
        weapon_index = usize::from(weapon_index == 0);
    }
    Ok(offsets)
}

/// `FVector3.ClampMagnitude`: a vector longer than the length, its square
/// magnitude over the length's square (`FPoint.op_GreaterThan`), shortened
/// to it along its direction.
fn clamp_magnitude_3d((x, y, z): (i64, i64, i64), length: i64) -> (i64, i64, i64) {
    let squared = q32_mul(x, x)
        .saturating_add(q32_mul(y, y))
        .saturating_add(q32_mul(z, z));
    if fpoint_less_or_equal(squared, q32_mul(length, length)) {
        return (x, y, z);
    }
    let inverse = q32_div(Q32_ONE, fpcs_sqrt_fastest(squared));
    (
        q32_mul(q32_mul(x, inverse), length),
        q32_mul(q32_mul(y, inverse), length),
        q32_mul(q32_mul(z, inverse), length),
    )
}
