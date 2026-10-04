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
        // The backswing is cut short by the next blow: a blow fitted into its
        // interval by `SkillAttackController.PerformAttack` gives its
        // backswing what is left of the interval, and a Wasp's 1.5-second
        // backswing reads 27 ticks, its interval, in the game's own states.
        let next_attack_step = skill.next_attack_step;
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
            SkillKind::Strike => {
                let actor_id = skill_ref.owner.unit_id().ok_or_else(|| {
                    Error::new("a construction's skill that strikes is not supported")
                })?;
                self.direct_effect(actor_id, pending.target, 0, events)?;
            }
            SkillKind::Laser => {
                let actor_id = skill_ref
                    .owner
                    .unit_id()
                    .ok_or_else(|| Error::new("a construction's laser is not supported"))?;
                let target = pending.target;
                let target_was_alive = self.fight_actor_is_alive(target);
                let target_was_lock = self.skill(skill_ref).lock_target == Some(target);
                self.laser_effect(actor_id, target, events)?;
                // A lock the beam kills stops the motion on that tick, whatever
                // it is: the Steel Ball that fells a tower in the tower-loss
                // fights, or the turret of `laser-fells-turret.yaml`, reads idle
                // on that tick. A block that only stood in the way is left to
                // the fallen-block rule on the next tick, as a blow's is: the
                // Steel Ball of `wall-laser.yaml` that fells block 4 reads
                // attacking on that tick, idle on the next.
                if target_was_alive && !self.fight_actor_is_alive(target) && target_was_lock {
                    let actor = self
                        .actors
                        .get_mut(&actor_id)
                        .expect("actor identity is stable");
                    actor.motion.state = MotionState::Idle;
                }
            }
            SkillKind::Projectile => {
                self.start_projectile_burst(skill_ref, pending.target, pending.step, events)?;
            }
            SkillKind::ControlBeam => {
                let actor_id = skill_ref
                    .owner
                    .unit_id()
                    .ok_or_else(|| Error::new("a construction's control beam is not supported"))?;
                self.control_effect(actor_id, pending.target, events)?;
            }
        }
        Ok(false)
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
        let attack = self
            .skill_attacker(skill_ref)
            .ok_or_else(|| Error::new("projectile owner is absent"))?
            .attack;
        let count = usize::try_from(attack.projectile_count())
            .expect("u32 projectile count fits the supported host");
        let weapon_count = usize::try_from(attack.weapons.count())
            .expect("u32 weapon count fits the supported host");
        let interval = native_time_units_to_steps(attack.projectile_release_interval_time_units());
        let radius = attack.projectile_target_offset_radius();
        let climb_target = self.climb_target(target)?;
        // An extra skill fires the one weapon of its row it was made for.
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
        let target_y = match target {
            FightActorRef::Unit(id) => unit_height(self.actors[&id].rules.domain),
            FightActorRef::Building(_) => 0,
        };
        let offsets = self.projectile_target_offsets(
            skill_ref.owner,
            target_x_q32,
            target_z_q32,
            (space_to_q32(source_y), space_to_q32(target_y)),
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
                    target_x_q32: target_x_q32.saturating_add(x),
                    target_z_q32: target_z_q32.saturating_add(z),
                    offset_x_q32: x,
                    offset_z_q32: z,
                    climb_target,
                    weapon_index: (first_weapon + index) % weapon_count,
                    skill_slot,
                });
        let first = releases
            .next()
            .ok_or_else(|| Error::new("projectile burst contains no release"))?;
        let Performer::Projectile { pending } = &mut self.skill_mut(skill_ref).performer else {
            return Err(Error::new("a burst needs a projectile performer"));
        };
        pending.extend(releases);
        self.release_pending_projectile(skill_ref, first, events)
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
    fn projectile_climb_q32(
        &self,
        skill_ref: SkillRef,
        (target_x_q32, target_z_q32, target_y): (i64, i64, i64),
    ) -> Result<Option<i64>> {
        let source = self
            .skill_attacker(skill_ref)
            .ok_or_else(|| Error::new("projectile owner is absent"))?
            .launch();
        if source.climb <= 0 && skill_ref.slot == SkillSlot::Main {
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
        owner: FightActorRef,
        target_x_q32: i64,
        target_z_q32: i64,
        (source_y_q32, target_y_q32): (i64, i64),
        count: usize,
        radius: i64,
    ) -> Result<Vec<(i64, i64)>> {
        if radius == 0 {
            return Ok(vec![(0, 0); count]);
        }
        let source = self
            .attacker(owner)
            .ok_or_else(|| Error::new("projectile owner is absent"))?;
        let team = source.team;
        let source_x_q32 = source.x_q32;
        let source_z_q32 = source.z_q32;
        let weapon_count = source.attack.weapons.count();
        let radius_centimeters = i32::try_from(radius / 10)
            .map_err(|_| Error::new("projectile target offset radius exceeds native range"))?;
        let random = self
            .team_random
            .get_mut(&team)
            .ok_or_else(|| Error::new("projectile owner team random stream is absent"))?;
        let mut offsets = Vec::with_capacity(count);
        for _ in 0..count {
            let x_centimeters =
                random.next_between_inclusive(-radius_centimeters, radius_centimeters);
            let z_centimeters =
                random.next_between_inclusive(-radius_centimeters, radius_centimeters);
            let clamp_centimeters = random.next_between_inclusive(0, radius_centimeters - 1);
            let x_q32 = q32_mul(i64::from(x_centimeters) << 32, C0_01_RAW);
            let z_q32 = q32_mul(i64::from(z_centimeters) << 32, C0_01_RAW);
            let clamp_q32 = q32_mul(i64::from(clamp_centimeters) << 32, C0_01_RAW);
            let (x_q32, z_q32) = clamp_magnitude_q32_raw(x_q32, z_q32, clamp_q32);
            offsets.push((x_q32, z_q32));
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
        let view = self
            .fight_actor(target)
            .ok_or_else(|| Error::new("projectile target is absent"))?;
        let (target_x_q32, target_z_q32) = (view.x_q32, view.z_q32);
        let attacker = self
            .attacker(owner)
            .ok_or_else(|| Error::new("projectile owner is absent"))?;
        let radius = attacker.attack.projectile_target_offset_radius();
        let source_y = attacker.y;
        let target_y = match target {
            FightActorRef::Unit(id) => unit_height(self.actors[&id].rules.domain),
            FightActorRef::Building(_) => 0,
        };
        let climb_target = self.climb_target(target)?;
        let (x, z) = self
            .projectile_target_offsets(
                owner,
                target_x_q32,
                target_z_q32,
                (space_to_q32(source_y), space_to_q32(target_y)),
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
                target_z_q32: target_z_q32.saturating_add(z),
                offset_x_q32: x,
                offset_z_q32: z,
                climb_target,
                weapon_index: slot,
                skill_slot: slot,
            },
            events,
        )
    }

    pub(in crate::fight) fn release_projectile(
        &mut self,
        skill_ref: SkillRef,
        target_id: u64,
        skill_slot: usize,
        weapon_index: usize,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let target = &self.actors[&target_id];
        let target_x_q32 = target.x_q32;
        let target_z_q32 = target.z_q32;
        self.release_projectile_at(
            skill_ref,
            target_id,
            target_x_q32,
            target_z_q32,
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
        match pending.target_kind {
            ObjectKind::Unit => {
                // A projectile leaves for where its burst aimed it, the
                // target's position when the burst began plus its offset,
                // and one that follows its target takes the target's position
                // up again from its first update: a Farseer's second shot,
                // which climbs first, still names the point the burst aimed
                // at when it levels off.
                let (target_x_q32, target_z_q32) = (pending.target_x_q32, pending.target_z_q32);
                let climb_q32 = self.projectile_climb_q32(skill_ref, pending.climb_target)?;
                self.release_projectile_at(
                    skill_ref,
                    pending.target,
                    target_x_q32,
                    target_z_q32,
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
                let (target_x_q32, target_z_q32) = (pending.target_x_q32, pending.target_z_q32);
                let radius = building_radius(building);
                let climb_q32 = self.projectile_climb_q32(skill_ref, pending.climb_target)?;
                self.release_projectile_to(
                    skill_ref,
                    ObjectKind::Building,
                    pending.target,
                    q32_to_space_rounded(target_x_q32),
                    0,
                    q32_to_space_rounded(target_z_q32),
                    target_x_q32,
                    target_z_q32,
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
        target_x_q32: i64,
        target_z_q32: i64,
        skill_slot: usize,
        weapon_index: usize,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let target = &self.actors[&target_id];
        let target_y = unit_height(target.rules.domain);
        let target_radius = target.rules.collision_radius();
        let target_x = q32_to_space_rounded(target_x_q32);
        let target_z = q32_to_space_rounded(target_z_q32);
        self.release_projectile_to(
            skill_ref,
            ObjectKind::Unit,
            target_id,
            target_x,
            target_y,
            target_z,
            target_x_q32,
            target_z_q32,
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
        target_x: i64,
        target_y: i64,
        target_z: i64,
        target_x_q32: i64,
        target_z_q32: i64,
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
            (target_x, target_y, target_z),
            (target_x_q32, target_z_q32),
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
        (target_x, target_y, target_z): (i64, i64, i64),
        (target_x_q32, target_z_q32): (i64, i64),
        target_radius: i64,
        skill_slot: usize,
        weapon: i32,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let projectile_id = self.ids.objects.allocate_object(ObjectKind::Projectile)?.id;
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
            cached_target_x: target_x,
            cached_target_y: target_y,
            cached_target_z: target_z,
            cached_target_x_q32: target_x_q32,
            cached_target_y_q32: space_to_q32(target_y),
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
            move_range_q32: Some(
                space_to_q32(source.range).saturating_add(space_to_q32(target_radius)),
            ),
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
