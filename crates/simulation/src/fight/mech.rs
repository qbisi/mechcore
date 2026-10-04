use super::*;

/// The bit of `status_mask` that holds `BuffManager.IsInvincible`.
const INVINCIBLE: u64 = 1;

/// The bit of `status_mask` that holds `FightMech.IsTechnologyDisabled`.
const TECHNOLOGY_DISABLED: u64 = 1 << 2;

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
        self.skills.main.kind = SkillKind::of(&rules.attack.path);
        self.skills.main.performer = Performer::of(self.skills.main.kind);
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
        let stats = crate::data::Stats::corrected(&rules, placement.level, &placement.corrections)
            .expect("the layout verified this loadout resolves");
        let max_life = stats.max_life();
        let magazine = rules.attack.magazine;
        let x = q32_to_space_rounded(x_q32);
        let z = q32_to_space_rounded(z_q32);
        let max_speed_q32 = stats.move_speed_q32();
        let weapon_rotations_q32 = vec![
            mdeg_to_degrees_q32(placement.rotation);
            usize::try_from(rules.attack.weapons.count())
                .expect("u32 weapon count fits the supported host")
        ];
        let group = group_shape(&rules);
        let kind = SkillKind::of(&rules.attack.path);
        // `AutoRecoveryEffectProvider.DoActive` hands a unit with a repair
        // source in force a controller, which its constructor resets.
        let recovery = placement
            .auto_recovery
            .map(|_| super::recovery::RecoveryClock::reset());
        let start_buffs_pending = !placement.start_buffs.is_empty();
        let shield = placement.energy_shield.map(|source| {
            let maximum = q32_mul(max_life << 32, source.life_rate_q32) >> 32;
            PersonalShield {
                energy: maximum,
                maximum,
            }
        });
        let underground = rules.underground.as_ref().map(underground::Underground::of);
        let original_team = placement.team;
        let original_formation = placement.formation_id;
        let mut actor = Self {
            x,
            z,
            x_q32,
            z_q32,
            target_query_x_q32: x_q32,
            target_query_z_q32: z_q32,
            target_query_source_rotation_q32: mdeg_to_degrees_q32(placement.rotation),
            target_query_alive: true,
            target_query_visible: true,
            body_rotation: placement.rotation,
            body_rotation_q32: mdeg_to_degrees_q32(placement.rotation),
            aim_rotation: placement.rotation,
            turret_q32: rules
                .attack
                .weapons
                .arcs
                .is_some()
                .then(|| mdeg_to_degrees_q32(placement.rotation)),
            turret_aim_q32: None,
            placement,
            rules,
            stats,
            life: max_life,
            last_damage_source: None,
            summoned: false,
            travelling: false,
            searched_attack: true,
            last_life_before_suicide: 0,
            command: None,
            buffs: Vec::new(),
            start_buffs_pending,
            shield,
            recovery,
            rvo_max_speed_q32: max_speed_q32,
            visibility: Visibility::Normal,
            skills_active: true,
            underground,
            beam: None,
            original_team,
            original_formation,
            motion: Motion {
                rvo_tree_x_q32: x_q32,
                rvo_tree_z_q32: z_q32,
                rvo_new_agent: false,
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
                state: MotionState::Idle,
                transition_to: None,
                attack_hold_fire: false,
                attacker: SkillSlot::Main,
            },
            skills: SkillManager::new(Skill::new(weapon_rotations_q32, group, magazine, kind)),
        };
        actor.skills.extras = extra_skills(&actor.placement);
        if actor.rules.mech_search
            && let Some(group) = &mut actor.skills.main.group
        {
            group.mech_search_time = Some(0);
        }
        actor
    }

    /// `MotionMoveState.MoveUpdate`'s `NormalRotate`: the facing turned to
    /// the velocity, before `Move` reads it for the speed.
    pub(in crate::fight) fn turn_to_move_direction(&mut self) {
        if self.motion.current_velocity_x_q32 != 0 || self.motion.current_velocity_z_q32 != 0 {
            self.rotate_body_towards(direction_degrees_q32_raw(
                self.motion.current_velocity_x_q32,
                self.motion.current_velocity_z_q32,
            ));
        }
    }

    /// `MotionController.Move`: the target point and the speed
    /// `CalculateMoveSpeed` takes from the facing the body has, handed to the
    /// agent. `Move` does this only on the update before the RVO solve, when
    /// `RVOSimulatorFixed`'s counter reads 3; on the other three it returns at
    /// once and the agent keeps what it was handed last.
    pub(in crate::fight) fn move_to(
        &mut self,
        target_x_q32: i64,
        target_z_q32: i64,
        solve_due: bool,
    ) {
        if !self.rules.has_body {
            self.aim_rotation = self.body_rotation;
        }
        if !solve_due {
            return;
        }
        self.motion.next_target_x_q32 = target_x_q32;
        self.motion.next_target_z_q32 = target_z_q32;
        self.motion.next_speed_q32 = turn_limited_move_speed_q32(
            self.stats.move_speed_q32(),
            self.rules.free_move,
            self.rules.rotate_speed_mdeg_per_second(),
            self.body_rotation_q32,
            self.motion.current_velocity_x_q32,
            self.motion.current_velocity_z_q32,
        );
        self.motion.next_max_speed_q32 = self.motion.next_speed_q32;
    }

    /// The skills of `FightMech.GetSkills()` whose `DataSet` holds the unit's
    /// skill corrections: the main skill's slots, and each extra skill with a
    /// damage rate, which takes what reaches the main skill
    /// (`SkillDataModifier.AvaliableCheck`, `IsMainSkillEffect`).
    pub(in crate::fight) fn corrected_skill_slots(&self) -> Vec<usize> {
        let main_slots = self.skills.main_slots();
        (0..main_slots)
            .chain(
                self.skills
                    .extras
                    .iter()
                    .enumerate()
                    .filter(|(_, extra)| extra.rules.damage_rate > 0.0)
                    .map(|(index, _)| main_slots + index),
            )
            .collect()
    }

    /// `FightMech.lockTarget`, which the unit's main searcher hands it: an
    /// active permanent preemptive skill's lock, or the main skill's.
    pub(in crate::fight) fn mech_lock(&self) -> Option<FightActorRef> {
        if self.skills.preemptive_active
            && let Some(extra) = self
                .skills
                .extras
                .iter()
                .find(|extra| extra.rules.preemptive.is_some())
        {
            return extra.skill.lock_target;
        }
        self.skills.main.unit_lock()
    }

    pub(in crate::fight) fn alive(&self) -> bool {
        self.life > 0
    }

    pub(in crate::fight) fn exit_fight_on_death(&mut self) {
        self.motion.state = MotionState::Idle;
        self.skills.main.set_pending(None);
        self.skills.main.lock_target = None;
        self.skills.main.search_target_time = SEARCH_TARGET_RESET_TICKS;
        self.skills.main.set_phase(FightSkillPhase::Idle);
        self.skills.main.clear_slots();
        self.skills.main.performer.stop();
        self.motion.attack_hold_fire = false;
        self.motion.current_velocity_x_q32 = 0;
        self.motion.current_velocity_z_q32 = 0;
        self.motion.next_target_x_q32 = self.x_q32;
        self.motion.next_target_z_q32 = self.z_q32;
        self.motion.next_speed_q32 = 0;
        self.motion.solver_target_x_q32 = self.x_q32;
        self.motion.solver_target_z_q32 = self.z_q32;
        self.motion.solver_speed_q32 = 0;
        self.motion.published_target_x_q32 = self.x_q32;
        self.motion.published_target_z_q32 = self.z_q32;
        self.motion.published_speed_q32 = 0;
    }

    /// The motion when the skill lets its target go. `AutoMoveBehaviour` is
    /// no longer active and the motion stops idle; a command stays active,
    /// and `MotionAttackState` changes to `MotionMoveState` instead.
    pub(in crate::fight) fn lose_target_motion(&mut self, publish_point: bool) {
        if self.command.is_some() {
            if self.motion.state == MotionState::Attacking {
                self.motion.state = MotionState::Moving;
            }
            return;
        }
        self.stop_in_place(publish_point);
    }

    /// `MotionIdleState` entered with a stop: the motion reads idle and
    /// publishes zero speed at its maximum, and, where the stop is published
    /// anew, the point it stands on as its target.
    pub(in crate::fight) fn stop_in_place(&mut self, publish_point: bool) {
        self.motion.state = MotionState::Idle;
        if publish_point {
            self.motion.next_target_x_q32 = self.x_q32;
            self.motion.next_target_z_q32 = self.z_q32;
        }
        self.motion.next_speed_q32 = 0;
        self.motion.next_max_speed_q32 = self.rvo_max_speed_q32;
    }

    pub(in crate::fight) fn object_ref(&self) -> ObjectRef {
        ObjectRef::new(ObjectKind::Unit, self.placement.unit_id)
    }

    pub(in crate::fight) fn set_weapon_rotation(&mut self, rotation_q32: i64) {
        self.skills.main.weapon_rotations_q32.fill(rotation_q32);
        if let Some(turret) = &mut self.turret_q32 {
            *turret = rotation_q32;
        }
    }

    /// Turns the whole unit, body, aim, weapons and the rotation its search
    /// scores from, to one facing, as it stands before the fight.
    pub(in crate::fight) fn face(&mut self, rotation_q32: i64) {
        self.set_body_rotation(rotation_q32);
        self.aim_rotation = self.body_rotation;
        self.set_weapon_rotation(rotation_q32);
        self.target_query_source_rotation_q32 = self.body_rotation_q32;
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

    /// `ISkillOwner.GetRotateSpeed`, as one update's turn.
    pub(in crate::fight) fn turn_q32(&self) -> i64 {
        q32_mul(
            mdeg_to_degrees_q32(self.rules.rotate_speed_mdeg_per_second()),
            NATIVE_LOGIC_DELTA_Q32,
        )
    }

    /// One update's turn of a weapon that turns within an arc of its own:
    /// `FightWeapon`'s constructor gives it the skill's weapon rotation
    /// speed where the skill has one, and the unit's otherwise.
    pub(in crate::fight) fn arc_weapon_turn_q32(&self) -> i64 {
        self.rules
            .attack
            .weapons
            .rotation_speed_mdeg_per_second()
            .map_or_else(
                || self.turn_q32(),
                |speed| q32_mul(mdeg_to_degrees_q32(speed), NATIVE_LOGIC_DELTA_Q32),
            )
    }

    pub(in crate::fight) fn rotate_weapons_towards(&mut self, target_q32: i64) {
        let turn_q32 = self.turn_q32();
        // The motion turns the mech body; weapons with arcs of their own
        // turn in their skills' update (`Simulation::turn_arc_weapons`).
        if let Some(turret) = &mut self.turret_q32 {
            let aim = if self.skills.main.standalone() {
                let Some(aim) = self.turret_aim_q32 else {
                    return;
                };
                aim
            } else {
                target_q32
            };
            *turret = rotate_towards_q32(*turret, aim, turn_q32);
            return;
        }
        self.skills.main.turn_weapons_towards(target_q32, turn_q32);
    }

    /// A unit with a body turns its turret, whose rotation every weapon of
    /// it shares; one without a body has none.
    pub(in crate::fight) fn turret_rotation(&self) -> Option<i64> {
        if self.turret_q32.is_some() {
            return self.turret_q32;
        }
        self.rules
            .has_body
            .then(|| self.skills.main.weapon_rotations_q32.first().copied())
            .flatten()
    }

    /// `FightSkill.GetMainTransform`'s rotation for a slot's search: a weapon
    /// with a transform of its own, which `CanRotate` answers for, is
    /// measured from; a slot without one is measured from the root.
    pub(in crate::fight) fn slot_main_rotation_q32(&self, slot: usize) -> i64 {
        if self.rules.attack.weapons.fixed_to_body && slot > 0 {
            self.skills.main.sibling_weapon_rotation_q32(slot)
        } else {
            self.body_rotation_q32
        }
    }

    /// What a standalone weapon's search is scored from when its skill
    /// searches from its default rotation (`useDefaultRotationSearchTarget`,
    /// `ScoreRatingTargetSelector.Selector.CalculateRotationData`): a weapon
    /// held to an arc is scored from its rest, the mech body's rotation plus
    /// its default angle, and passes over what lies outside its arc widened
    /// by the attack angle either side; a weapon that turns freely is scored
    /// from its rest too, and has no window: the Mountain's first gun, at 10
    /// degrees, takes the Crawler to the right of the one straight ahead. The
    /// window is how far it reaches left and right of the rest, which need
    /// not be the same: the War Factory's arcs are not.
    pub(in crate::fight) fn default_search_frame(
        &self,
        slot: usize,
    ) -> Option<(i64, Option<(i64, i64)>)> {
        if !self.rules.attack.default_rotation_search {
            return None;
        }
        let arc = self.rules.attack.weapons.arcs.as_ref()?.get(slot)?;
        let rest = self
            .arc_parent_q32()?
            .saturating_add(i64::from(arc.default) << 32)
            .rem_euclid(360_i64 << 32);
        let window = arc.left.zip(arc.right).map(|(left, right)| {
            let angle = mdeg_to_degrees_q32(self.rules.attack.attack_half_angle_mdeg());
            (
                (i64::from(left) << 32).saturating_add(angle),
                (i64::from(right) << 32).saturating_add(angle),
            )
        });
        Some((rest, window))
    }

    /// What a weapon that turns within an arc turns about
    /// (`RotationLimitFightTransform`'s parent): the mech body, the turret,
    /// for a skill mounted on it (`WeaponMountNode.MechBody`), and the unit's
    /// root otherwise. `None` for a unit whose weapons have no arcs.
    pub(in crate::fight) fn arc_parent_q32(&self) -> Option<i64> {
        let turret = self.turret_q32?;
        Some(
            if self.rules.attack.weapons.mount == WeaponMount::MechBody {
                turret
            } else {
                self.body_rotation_q32
            },
        )
    }

    /// A weapon fixed to the body stands where the unit stands; the core's
    /// takes the body's rotation whenever the body turns, a sibling's only
    /// when its own skill updates holding a lock. Any other weapon has no
    /// transform of its own.
    fn fixed_weapon_pose(&self, weapon_index: usize, position: QVec3) -> Option<QPose> {
        // A weapon that turns within an arc has a transform of its own too,
        // `RotationLimitFightTransform`, which turns on its own.
        if self.rules.attack.weapons.arcs.is_some() {
            return Some(QPose {
                position,
                rotation: self.skills.main.weapon_rotations_q32[weapon_index],
            });
        }
        self.rules.attack.weapons.fixed_to_body.then(|| QPose {
            position,
            rotation: match weapon_index {
                0 => self.body_rotation_q32,
                slot => self.skills.main.sibling_weapon_rotation_q32(slot),
            },
        })
    }

    pub(in crate::fight) fn snapshot(&self) -> LiveUnitState {
        let height = unit_height(self.rules.domain);
        let position = QVec3 {
            x: self.x_q32,
            y: space_to_q32(height),
            z: self.z_q32,
        };
        let weapon_aims = self.weapon_aims(position);
        LiveUnitState {
            unit_id: self.placement.unit_id,
            team_id: self.placement.team,
            original_team_id: self.original_team,
            formation_id: self.placement.formation_id,
            unit_type_id: self.rules.unit_type_id,
            domain: match self.rules.domain {
                UnitDomain::Ground => Domain::Ground,
                UnitDomain::Air => Domain::Air,
            },
            position,
            body_rotation: self.body_rotation_q32,
            turret_rotation: self.turret_rotation(),
            velocity: QPlanar {
                x: self.motion.current_velocity_x_q32,
                z: self.motion.current_velocity_z_q32,
            },
            motion_state: self.motion.state,
            mech_lock_target: self.mech_lock().map(FightActorRef::object_ref),
            collision_radius: space_to_q32(self.rules.collision_radius()),
            life: GaugeI32 {
                current: i32::try_from(self.life).expect("unit life fits i32"),
                maximum: i32::try_from(self.stats.max_life()).expect("unit max life fits i32"),
            },
            active: true,
            targetable: self.visibility == Visibility::Normal,
            visibility: self.visibility,
            status_mask: if self.invincible() { INVINCIBLE } else { 0 }
                | if self.technology_disabled() {
                    TECHNOLOGY_DISABLED
                } else {
                    0
                },
            modifiers: self
                .stats
                .modifiers(&self.corrected_skill_slots())
                .expect("the layout refused every correction a snapshot cannot record"),
            personal_shield: PersonalShieldState {
                active: self.shield.is_some(),
                enabled: true,
                energy: GaugeI32 {
                    current: self.shield.map_or(0, |shield| {
                        i32::try_from(shield.energy).expect("shield energy fits i32")
                    }),
                    maximum: self.shield.map_or(0, |shield| {
                        i32::try_from(shield.maximum).expect("shield energy fits i32")
                    }),
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
                move_speed: self.stats.move_speed_q32(),
                attack_range: self.stats.attack_range_q32(),
                // A beam's damage is its ramp's first step, whatever step it
                // is on: the Steel Balls of `wall-laser.yaml` read 2, which
                // is 55 at its first multiplier, on every tick of their fight.
                attack_damage: i32::try_from(match &self.rules.attack.path {
                    AttackPath::Laser { .. } => self.stats.laser_damage(&self.rules, 0),
                    _ => self.stats.attack_damage(),
                })
                .unwrap_or(i32::MAX),
                // The recording counts an interval in logic ticks, which is
                // the unit the build's own integer uses: the interval the
                // cycle in progress was scheduled with, stagger included, the
                // core's for a group, and the composed interval once no enemy
                // is left. `docs/rules/combat.md` says how each was read.
                current_attack_interval: i32::try_from(self.skills.main.current_attack_interval)
                    .unwrap_or(i32::MAX),
            },
        }
    }
}

impl Actor {
    /// Every weapon channel the unit records, the main skill's and then its
    /// extra skills'.
    fn weapon_aims(&self, position: QVec3) -> Vec<WeaponAimState> {
        (0..self.skills.main.weapon_rotations_q32.len())
            .map(|weapon_index| {
                let group_mode = self.rules.attack.weapons.mode != WeaponMode::Normal;
                let attack_target = if group_mode {
                    // A slot firing at a shield names no target the
                    // recording can.
                    self.skills
                        .main
                        .group_attack_target(weapon_index)
                        .filter(|_| {
                            weapon_index >= self.skills.main.group_size().max(1)
                                || self
                                    .skills
                                    .main
                                    .group_skill(weapon_index)
                                    .shield_target()
                                    .is_none()
                        })
                } else {
                    self.skills.main.named_attack_target()
                };
                // A unit that travelled in has run no update to search an
                // attack target with until its first: `SearchAttackTarget`
                // answers only in one.
                let attack_target = attack_target.filter(|_| self.searched_attack);
                WeaponAimState {
                    skill_slot: if group_mode {
                        u16::try_from(weapon_index).expect("weapon index fits u16")
                    } else {
                        0
                    },
                    weapon_index: self.rules.attack.weapons.index(weapon_index),
                    attack_target: attack_target.map(FightActorRef::object_ref),
                    pose: self.fixed_weapon_pose(weapon_index, position),
                }
            })
            .chain(self.extra_weapon_aims(position))
            .collect()
    }

    /// The weapon channel of each extra skill, after the main skill's: the
    /// weapon its row names, at its own rotation on a transform of its own
    /// where the unit stands, firing at what its skill does.
    fn extra_weapon_aims(&self, position: QVec3) -> impl Iterator<Item = WeaponAimState> + '_ {
        let main_slots = self.skills.main_slots();
        self.skills
            .extras
            .iter()
            .enumerate()
            .flat_map(move |(index, extra)| {
                let attack_target = extra
                    .skill
                    .named_attack_target()
                    .filter(|_| self.searched_attack)
                    .map(FightActorRef::object_ref);
                // Only a weapon that turns within an arc has a transform of
                // its own.
                let arcs = extra.rules.attack.weapons.arcs.is_some();
                extra.skill.weapon_rotations_q32.iter().enumerate().map(
                    move |(offset, &rotation)| WeaponAimState {
                        skill_slot: u16::try_from(main_slots + index).expect("skill slot fits u16"),
                        weapon_index: extra.rules.attack.weapons.index(extra.weapon + offset),
                        attack_target,
                        pose: arcs.then_some(QPose { position, rotation }),
                    },
                )
            })
    }
}

/// The skills a unit's extra weapon technologies add beside its main one, in
/// `SkillManager.extraSkills`' order: ascending skill ID
/// (`SkillManager.SortSkills`), and a row's skills in the order its weapons
/// come. A standalone row makes one skill for each weapon; each starts
/// pointing as the unit faces, as the main skill's weapons do.
fn extra_skills(placement: &Placement) -> Vec<ExtraSkill> {
    let mut weapons: Vec<&crate::layout::ExtraWeapon> = placement.extra_weapons.iter().collect();
    weapons.sort_by_key(|weapon| weapon.rules.skill);
    weapons
        .into_iter()
        .flat_map(|weapon| {
            let rules = &weapon.rules;
            let count = usize::try_from(rules.attack.weapons.count())
                .expect("u32 weapon count fits the supported host");
            // A standalone row makes a skill of each weapon; any other one
            // skill that fires them all.
            let (skills, per_skill) = if rules.attack.weapons.mode == WeaponMode::Standalone {
                (count, 1)
            } else {
                (1, count)
            };
            (0..skills).map(move |index| {
                let mut skill = Skill::new(
                    vec![mdeg_to_degrees_q32(placement.rotation); per_skill],
                    None,
                    rules.attack.magazine,
                    SkillKind::of(&rules.attack.path),
                );
                // `PreemptiveSkillController.MarkPermanentPreemptiveSkill`
                // locks a permanent preemptive skill until its condition holds.
                if rules.preemptive.is_some() {
                    skill.enter(super::skill::SkillState::Locked);
                }
                ExtraSkill {
                    skill,
                    rules: rules.clone(),
                    terrain: weapon.terrain,
                    buff: weapon.buff,
                    dead_fire: weapon.dead_fire,
                    weapon: index,
                }
            })
        })
        .collect()
}

/// How many skills a unit's weapons make and how they take turns, or `None`
/// for one skill alone.
fn group_shape(rules: &UnitConfig) -> Option<(usize, GroupBehaviour)> {
    let weapons = &rules.attack.weapons;
    (weapons.mode != WeaponMode::Normal).then(|| {
        (
            usize::try_from(weapons.count()).expect("u32 weapon count fits the supported host"),
            if weapons.mode == WeaponMode::Standalone {
                GroupBehaviour::Standalone
            } else if weapons.fusillade == Some(true) {
                GroupBehaviour::Fusillade
            } else {
                GroupBehaviour::Each
            },
        )
    })
}
