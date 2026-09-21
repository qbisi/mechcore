use super::*;

impl Actor {
    #[cfg(test)]
    pub(in crate::fight) fn new(placement: Placement, rules: UnitConfig, seed: i32) -> Self {
        let (x_q32, z_q32) = generate_formation_positions(&placement, &rules, seed)
            .expect("embedded test config has a valid formation definition")[0];
        Self::at_generated_position(placement, rules, x_q32, z_q32)
    }

    /// Gives this actor another description, and recomputes what the fight
    /// reads from it.
    ///
    /// The build never swaps a description; a test does, and a derived number
    /// that kept the old one would be a cache telling a lie.
    #[cfg(test)]
    pub(in crate::fight) fn describe(&mut self, rules: UnitConfig) {
        self.stats = crate::data::Stats::of(&rules).expect("an uncorrected description resolves");
        self.rules = rules;
    }

    pub(in crate::fight) fn at_generated_position(
        placement: Placement,
        rules: UnitConfig,
        x_q32: i64,
        z_q32: i64,
    ) -> Self {
        // The layout resolved these when it compiled the placement, which is
        // where a refusal can name the side and the officer; reaching here
        // means they resolve.
        let stats = crate::data::Stats::corrected(&rules, &placement.corrections)
            .expect("the layout verified this loadout resolves");
        let max_life = stats.max_life();
        let x = q32_to_space_rounded(x_q32);
        let z = q32_to_space_rounded(z_q32);
        let max_speed_q32 = space_to_q32(stats.move_speed());
        let weapon_rotations_q32 = vec![
            mdeg_to_degrees_q32(placement.rotation);
            usize::try_from(rules.attack.weapons.count)
                .expect("u32 weapon count fits the supported host")
        ];
        let group_skill_count = if rules.attack.weapons.mode == WeaponMode::Group {
            usize::try_from(rules.attack.weapons.count)
                .expect("u32 weapon count fits the supported host")
        } else {
            0
        };
        Self {
            x,
            z,
            x_q32,
            z_q32,
            target_query_x_q32: x_q32,
            target_query_z_q32: z_q32,
            target_query_source_rotation_q32: mdeg_to_degrees_q32(placement.rotation),
            target_query_alive: true,
            rvo_tree_x_q32: x_q32,
            rvo_tree_z_q32: z_q32,
            body_rotation: placement.rotation,
            body_rotation_q32: mdeg_to_degrees_q32(placement.rotation),
            aim_rotation: placement.rotation,
            weapon_rotations_q32,
            placement,
            rules,
            stats,
            current_velocity_x_q32: 0,
            current_velocity_z_q32: 0,
            next_target_x_q32: x_q32,
            next_target_z_q32: z_q32,
            next_speed_q32: 0,
            next_max_speed_q32: max_speed_q32,
            solver_target_x_q32: x_q32,
            solver_target_z_q32: z_q32,
            solver_speed_q32: 0,
            published_target_x_q32: x_q32,
            published_target_z_q32: z_q32,
            published_speed_q32: 0,
            rvo_stopped_snap_since_boundary: false,
            life: max_life,
            last_damage_source: None,
            motion: MotionState::Idle,
            next_attack_step: 0,
            current_attack_interval: 0,
            motion_attack_hold_fire: false,
            lock_target: None,
            in_the_way: None,
            cooling_candidate: None,
            cooling_hold: None,
            lock_is_terminal_handoff: false,
            // FightSkill owns a second SearchTargetController. FightPrepareState
            // replaces this constructor value with the presearch batch ordinal.
            fight_skill_search_target_time: SEARCH_TARGET_RESET_TICKS,
            fight_skill_searched_this_tick: false,
            fight_skill_phase: FightSkillPhase::Idle,
            group_skill_targets: vec![None; group_skill_count],
            group_in_the_way: vec![None; group_skill_count],
            group_skill_next_attack_steps: vec![0; group_skill_count],
            group_skill_prepare_ready_steps: vec![0; group_skill_count],
            group_pending_releases: Vec::new(),
            projectile_pending_releases: Vec::new(),
            projectile_burst_finished: false,
            projectile_burst_finished_same_tick_dead: false,
            laser_attack_count: 0,
            retarget_after_own_direct_kill: false,
            pending: None,
            backswing_finish_step: None,
        }
    }

    pub(in crate::fight) fn alive(&self) -> bool {
        self.life > 0
    }

    /// What this actor's weapons fire at: the construction in the way if one
    /// stands there for the current lock, and the lock itself otherwise.
    ///
    /// Range, attack angle, release and the question of whether the target
    /// is still alive are all asked of this. Where to move and where a body
    /// faces are asked of `lock_target`.
    pub(in crate::fight) fn attack_target(&self) -> Option<FightActorRef> {
        match self.in_the_way {
            Some((building, found_for)) if self.lock_target == Some(found_for) => {
                Some(FightActorRef::Building(building))
            }
            _ => self.lock_target,
        }
    }

    /// Drops the mech's target, and every grouped slot with it.
    ///
    /// A group whose mech holds no target holds no slots: every time a Wraith
    /// was recorded losing its lock — to a block it was shooting falling, and
    /// to the last enemy dying — all four slots read empty the same tick, and
    /// the children were allocated again only once the core was attacking,
    /// the usual eight ticks later. Nothing changes for a unit without a
    /// group, whose slot lists are empty.
    pub(in crate::fight) fn drop_lock(&mut self) {
        self.lock_target = None;
        self.group_skill_targets.fill(None);
        self.group_in_the_way.fill(None);
        self.group_skill_next_attack_steps.fill(0);
        self.group_skill_prepare_ready_steps.fill(0);
        self.group_pending_releases.clear();
    }

    /// What one grouped slot fires at: the construction in its way if one
    /// stands there for the unit it was allocated, and that unit otherwise.
    pub(in crate::fight) fn group_attack_target(&self, slot: usize) -> Option<FightActorRef> {
        let unit = self.group_skill_targets.get(slot).copied().flatten()?;
        match self.group_in_the_way.get(slot).copied().flatten() {
            Some((building, found_for)) if found_for == unit => {
                Some(FightActorRef::Building(building))
            }
            _ => Some(FightActorRef::Unit(unit)),
        }
    }

    /// What a grouped skill's core fires at, or the weapons' target when the
    /// group has none.
    pub(in crate::fight) fn mechanical_attack_target(&self) -> Option<FightActorRef> {
        self.group_attack_target(0)
            .or_else(|| {
                (0..self.group_skill_targets.len())
                    .rev()
                    .find_map(|slot| self.group_attack_target(slot))
            })
            .or(self.attack_target())
    }

    pub(in crate::fight) fn mechanical_lock_target(&self) -> Option<FightActorRef> {
        self.group_skill_targets
            .first()
            .copied()
            .flatten()
            .or_else(|| {
                self.group_skill_targets
                    .iter()
                    .rev()
                    .flatten()
                    .copied()
                    .next()
            })
            .map(FightActorRef::Unit)
            .or(self.lock_target)
    }

    pub(in crate::fight) fn exit_fight_on_death(&mut self) {
        self.motion = MotionState::Idle;
        self.pending = None;
        self.lock_target = None;
        self.lock_is_terminal_handoff = false;
        self.fight_skill_search_target_time = SEARCH_TARGET_RESET_TICKS;
        self.fight_skill_phase = FightSkillPhase::Idle;
        self.group_skill_targets.fill(None);
        self.group_in_the_way.fill(None);
        self.group_skill_next_attack_steps.fill(0);
        self.group_skill_prepare_ready_steps.fill(0);
        self.group_pending_releases.clear();
        self.projectile_pending_releases.clear();
        self.projectile_burst_finished = false;
        self.projectile_burst_finished_same_tick_dead = false;
        self.laser_attack_count = 0;
        self.retarget_after_own_direct_kill = false;
        self.motion_attack_hold_fire = false;
        self.current_velocity_x_q32 = 0;
        self.current_velocity_z_q32 = 0;
        self.next_target_x_q32 = self.x_q32;
        self.next_target_z_q32 = self.z_q32;
        self.next_speed_q32 = 0;
        self.solver_target_x_q32 = self.x_q32;
        self.solver_target_z_q32 = self.z_q32;
        self.solver_speed_q32 = 0;
        self.published_target_x_q32 = self.x_q32;
        self.published_target_z_q32 = self.z_q32;
        self.published_speed_q32 = 0;
    }

    pub(in crate::fight) fn object_ref(&self) -> ObjectRef {
        ObjectRef::new(ObjectKind::Unit, self.placement.unit_id)
    }

    pub(in crate::fight) fn set_weapon_rotation(&mut self, rotation_q32: i64) {
        self.weapon_rotations_q32.fill(rotation_q32);
    }

    pub(in crate::fight) fn set_body_rotation(&mut self, rotation_q32: i64) {
        self.body_rotation_q32 = rotation_q32.rem_euclid(360_i64 << 32);
        self.body_rotation = degrees_q32_to_mdeg(self.body_rotation_q32);
    }

    pub(in crate::fight) fn rotate_body_towards(&mut self, target_q32: i64) {
        let maximum = q32_mul(
            mdeg_to_degrees_q32(self.rules.rotate_speed_mdeg_per_second()),
            NATIVE_LOGIC_DELTA_Q32,
        );
        let rate_limited = rotation_distance_q32(self.body_rotation_q32, target_q32) > maximum;
        let unwrapped = rotate_towards_q32_unwrapped(self.body_rotation_q32, target_q32, maximum);
        self.set_body_rotation(unwrapped);
        if rate_limited
            && unwrapped < 360_i64 << 32
            && degrees_q32_to_unwrapped_mdeg(unwrapped) == 360_000
        {
            self.body_rotation = 360_000;
        }
    }

    pub(in crate::fight) fn rotate_weapons_towards(&mut self, target_q32: i64) {
        let maximum = q32_mul(
            mdeg_to_degrees_q32(self.rules.rotate_speed_mdeg_per_second()),
            NATIVE_LOGIC_DELTA_Q32,
        );
        for rotation in &mut self.weapon_rotations_q32 {
            *rotation = rotate_towards_q32(*rotation, target_q32, maximum);
        }
    }

    pub(in crate::fight) fn weapons_in_attack_angle(&self, target_q32: i64) -> bool {
        let half_angle_q32 = mdeg_to_degrees_q32(self.rules.attack.attack_half_angle_mdeg());
        !self.weapon_rotations_q32.is_empty()
            && self
                .weapon_rotations_q32
                .iter()
                .all(|rotation| rotation_distance_q32(*rotation, target_q32) <= half_angle_q32)
    }

    pub(in crate::fight) fn snapshot(&self) -> LiveUnitState {
        let height = unit_height(self.rules.domain);
        let position = QVec3 {
            x: self.x_q32,
            y: space_to_q32(height),
            z: self.z_q32,
        };
        let weapon_aims = (0..self.weapon_rotations_q32.len())
            .map(|weapon_index| {
                let group_mode = self.rules.attack.weapons.mode == WeaponMode::Group;
                let attack_target = if group_mode {
                    self.group_attack_target(weapon_index).or_else(|| {
                        (weapon_index == 0)
                            .then_some(self.attack_target())
                            .flatten()
                    })
                } else {
                    self.attack_target().or_else(|| {
                        self.cooling_candidate
                            .filter(|_| self.lock_target.is_none())
                    })
                };
                WeaponAimState {
                    skill_slot: if group_mode {
                        u16::try_from(weapon_index).expect("weapon index fits u16")
                    } else {
                        0
                    },
                    weapon_index: i32::try_from(weapon_index).expect("weapon index fits i32"),
                    attack_target: attack_target.map(FightActorRef::object_ref),
                    pose: None,
                }
            })
            .collect();
        LiveUnitState {
            unit_id: self.placement.unit_id,
            team_id: self.placement.team,
            original_team_id: self.placement.team,
            formation_id: self.placement.formation_id,
            unit_type_id: self.rules.unit_type_id,
            domain: match self.rules.domain {
                UnitDomain::Ground => Domain::Ground,
                UnitDomain::Air => Domain::Air,
            },
            position,
            body_rotation: self.body_rotation_q32,
            velocity: QVec3 {
                x: self.current_velocity_x_q32,
                y: 0,
                z: self.current_velocity_z_q32,
            },
            motion_state: self.motion,
            mech_lock_target: self.lock_target.map(FightActorRef::object_ref),
            collision_radius: space_to_q32(self.rules.collision_radius()),
            life: GaugeI32 {
                current: i32::try_from(self.life).expect("unit life fits i32"),
                maximum: i32::try_from(self.stats.max_life()).expect("unit max life fits i32"),
            },
            active: true,
            targetable: true,
            visibility: Visibility::Normal,
            status_mask: 0,
            buff_modifiers: BuffModifierSet::default(),
            unit_dynamic_modifiers: UnitDynamicModifierSet::default(),
            skill_dynamic_modifiers: Vec::new(),
            personal_shield: PersonalShieldState {
                active: false,
                enabled: true,
                energy: GaugeI32 {
                    current: 0,
                    maximum: 0,
                },
            },
            weapon_aims,
            // What this fight reads, in the units the recording keeps them
            // in: a distance is `FPoint`, and damage is the integer the
            // build's own `DamageProperty` answers. Writing them here is what
            // lets a capture compare the number the game computed with the
            // number this simulator computed, one tick at a time, instead of
            // arranging a fight whose outcome happens to tell them apart.
            derived: DerivedStats {
                move_speed: space_to_q32(self.stats.move_speed()),
                attack_range: space_to_q32(self.stats.attack_range()),
                // A beam's damage is its ramp's first step, whatever step it
                // is on: the Steel Balls of `wall-laser.yaml` read 2, which
                // is 55 at its first multiplier, on every tick of their fight.
                attack_damage: i32::try_from(match &self.rules.attack.path {
                    AttackPath::Laser { .. } => self.rules.attack.laser_damage(0),
                    _ => self.stats.attack_damage(),
                })
                .unwrap_or(i32::MAX),
                // The recording counts an interval in logic ticks, which is
                // the unit the build's own integer uses: the interval the
                // cycle in progress was scheduled with, stagger included, the
                // core's for a group, and the composed interval once no enemy
                // is left. `docs/rules/combat.md` says how each was read.
                current_attack_interval: i32::try_from(self.current_attack_interval)
                    .unwrap_or(i32::MAX),
            },
        }
    }
}
