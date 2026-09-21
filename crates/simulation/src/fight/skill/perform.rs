use super::*;

impl Simulation {
    pub(in crate::fight) fn release(
        &mut self,
        actor_id: u64,
        events: &mut Vec<Event>,
    ) -> Result<bool> {
        let pending = self.actors[&actor_id]
            .pending
            .ok_or_else(|| Error::new("attack release has no pending action"))?;
        let release_attackable_invalid = self.bodyless_attackable_invalid(actor_id, pending.target);
        if release_attackable_invalid {
            // SkillAttackState rechecks CheckAttackable and target angle at
            // the attack point. A failed check skips PerformAttack; the skill
            // phase can then finish and MotionAttackState returns to Idle in
            // the same logic update.
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.pending = None;
            actor.fight_skill_phase = FightSkillPhase::Idle;
            return Ok(true);
        }
        let backswing_steps =
            native_time_units_to_steps(self.actors[&actor_id].rules.attack.backswing_time_units());
        let owner = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        owner.pending = None;
        owner.backswing_finish_step =
            (backswing_steps > 0).then(|| pending.step.saturating_add(backswing_steps));
        owner.fight_skill_phase = if owner.backswing_finish_step.is_none()
            && !owner.rules.attack.quick_switch_target
            && !matches!(owner.rules.attack.path, AttackPath::Laser { .. })
        {
            FightSkillPhase::Idle
        } else {
            FightSkillPhase::Attack
        };
        if matches!(
            self.actors[&actor_id].rules.attack.path,
            AttackPath::Direct { .. }
        ) {
            let target = pending.target;
            let target_was_alive = self.fight_actor_is_alive(target);
            self.direct_effect(actor_id, target, events)?;
            // A block felled by a blow is left to the backswing and then to
            // the fallen-block rule: the Rhino of `wall-rhino.yaml` stays on
            // the block it felled until its swing is over, where a unit it
            // kills hands it straight to the retarget.
            if target_was_alive
                && !self.fight_actor_is_alive(target)
                && matches!(target, FightActorRef::Unit(_))
            {
                self.actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable")
                    .retarget_after_own_direct_kill = true;
            }
            return Ok(false);
        }
        if matches!(
            self.actors[&actor_id].rules.attack.path,
            AttackPath::Laser { .. }
        ) {
            let target = pending.target;
            let target_was_alive = self.fight_actor_is_alive(target);
            self.laser_effect(actor_id, target, events)?;
            // A block the beam fells is left to the fallen-block rule on the
            // next tick, as a blow's is: the Steel Ball of `wall-laser.yaml`
            // that fells block 4 reads attacking on that tick, idle on the
            // next.
            if target_was_alive
                && !self.fight_actor_is_alive(target)
                && matches!(target, FightActorRef::Unit(_))
            {
                let actor = self
                    .actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable");
                actor.motion = MotionState::Idle;
                actor.retarget_after_own_direct_kill = true;
            }
            return Ok(false);
        }
        self.start_projectile_burst(actor_id, pending.target, pending.step, events)?;
        Ok(false)
    }

    pub(in crate::fight) fn start_projectile_burst(
        &mut self,
        actor_id: u64,
        target: FightActorRef,
        step: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let target_view = self
            .fight_actor(target)
            .ok_or_else(|| Error::new("projectile target is absent"))?;
        let target_x_q32 = target_view.x_q32;
        let target_z_q32 = target_view.z_q32;
        let owner = &self.actors[&actor_id];
        let count = usize::try_from(owner.rules.attack.projectile_count())
            .expect("u32 projectile count fits the supported host");
        let weapon_count = usize::try_from(owner.rules.attack.weapons.count)
            .expect("u32 weapon count fits the supported host");
        let interval =
            native_time_units_to_steps(owner.rules.attack.projectile_release_interval_time_units());
        let radius = owner.rules.attack.projectile_target_offset_radius();
        let offsets =
            self.projectile_target_offsets(actor_id, target_x_q32, target_z_q32, count, radius)?;
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
                    weapon_index: index % weapon_count,
                });
        let first = releases
            .next()
            .ok_or_else(|| Error::new("projectile burst contains no release"))?;
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        actor.projectile_burst_finished_same_tick_dead = false;
        actor.projectile_pending_releases.extend(releases);
        self.release_pending_projectile(actor_id, first, events)
    }

    pub(in crate::fight) fn projectile_target_offsets(
        &mut self,
        actor_id: u64,
        target_x_q32: i64,
        target_z_q32: i64,
        count: usize,
        radius: i64,
    ) -> Result<Vec<(i64, i64)>> {
        if radius == 0 {
            return Ok(vec![(0, 0); count]);
        }
        let owner = &self.actors[&actor_id];
        let team = owner.placement.team;
        let source_x_q32 = owner.x_q32;
        let source_z_q32 = owner.z_q32;
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
        if self.actors[&actor_id].rules.attack.weapons.count == 2 && offsets.len() >= 2 {
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
                    // Build 2259 passes Cross(up, targetDirection) first and the
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
                    let plane_side = q32_mul(source_x_q32, absolute_value_x)
                        .saturating_add(q32_mul(source_z_q32, absolute_value_z))
                        .saturating_sub(plane_distance);
                    let value_x = i64::try_from(value_x)
                        .expect("projectile offset direction remains in Q32 range");
                    let value_z = i64::try_from(value_z)
                        .expect("projectile offset direction remains in Q32 range");
                    let direction_squared = q32_mul(direction_x, direction_x)
                        .saturating_add(q32_mul(direction_z, direction_z));
                    let value_squared =
                        q32_mul(value_x, value_x).saturating_add(q32_mul(value_z, value_z));
                    let magnitude_product =
                        if direction_squared.saturating_add(value_squared) < 0x1_6A09_0000_0001 {
                            fpcs_sqrt_fastest(q32_mul(direction_squared, value_squared))
                        } else {
                            q32_mul(
                                fpcs_sqrt_fastest(direction_squared),
                                fpcs_sqrt_fastest(value_squared),
                            )
                        };
                    let dot =
                        q32_mul(direction_x, value_x).saturating_add(q32_mul(direction_z, value_z));
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
        }
        Ok(offsets)
    }

    pub(in crate::fight) fn release_projectile(
        &mut self,
        actor_id: u64,
        target_id: u64,
        skill_slot: usize,
        weapon_index: usize,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let target = &self.actors[&target_id];
        let target_x_q32 = target.x_q32;
        let target_z_q32 = target.z_q32;
        self.release_projectile_at(
            actor_id,
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
        actor_id: u64,
        pending: PendingProjectileRelease,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        match pending.target_kind {
            ObjectKind::Unit => self.release_projectile_at(
                actor_id,
                pending.target,
                pending.target_x_q32,
                pending.target_z_q32,
                0,
                pending.weapon_index,
                events,
            ),
            ObjectKind::Building => {
                let building = self
                    .buildings
                    .iter()
                    .find(|building| building.building_id == pending.target)
                    .ok_or_else(|| Error::new("projectile building target is absent"))?;
                self.release_projectile_to(
                    actor_id,
                    ObjectKind::Building,
                    pending.target,
                    q32_to_space_rounded(pending.target_x_q32),
                    0,
                    q32_to_space_rounded(pending.target_z_q32),
                    pending.target_x_q32,
                    pending.target_z_q32,
                    building_radius(building),
                    0,
                    pending.weapon_index,
                    events,
                )
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
        actor_id: u64,
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
            actor_id,
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
        actor_id: u64,
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
        let projectile_id = self.identities.allocate_object(ObjectKind::Projectile)?.id;
        let owner = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let projectile = Projectile {
            id: projectile_id,
            team: owner.placement.team,
            owner: actor_id,
            target_kind,
            target: target_id,
            x: owner.x,
            y: unit_height(owner.rules.domain),
            z: owner.z,
            x_q32: owner.x_q32,
            y_q32: space_to_q32(unit_height(owner.rules.domain)),
            z_q32: owner.z_q32,
            cached_target_x: target_x,
            cached_target_y: target_y,
            cached_target_z: target_z,
            cached_target_x_q32: target_x_q32,
            cached_target_y_q32: space_to_q32(target_y),
            cached_target_z_q32: target_z_q32,
            cached_target_radius: target_radius,
            speed: owner.rules.attack.projectile_speed(),
            damage: owner.stats.attack_damage(),
            life: owner.rules.attack.projectile_life(),
            lock_target: owner.rules.attack.lock_target,
        };
        let projectile_ref = projectile.object_ref();
        events.push(event(
            Some(projectile_ref),
            Some(owner.object_ref()),
            Some(owner.placement.team),
            Some(ObjectRef::new(target_kind, target_id)),
            EventPayload::ProjectileReleased {
                skill_slot: Some(u16::try_from(skill_slot).expect("skill slot fits u16")),
                weapon_index: Some(i32::try_from(weapon_index).expect("weapon index fits i32")),
            },
        ));
        self.projectiles.push(projectile);
        Ok(())
    }
}
