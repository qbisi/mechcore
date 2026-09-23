use super::skill::Flow;
use super::*;

#[derive(Debug, Clone, Copy)]
pub(in crate::fight) struct RvoProfile {
    pub(in crate::fight) outer_radius_q32: i64,
    pub(in crate::fight) inner_radius_q32: i64,
    pub(in crate::fight) size: AgentSizeType,
    pub(in crate::fight) collider_priority: i32,
    pub(in crate::fight) priority_q32: i64,
}

/// `MotionController`: what the body does — its state, where it has been
/// asked to go and how fast, and what the RVO solver made of that.

#[derive(Debug, Clone)]
pub(in crate::fight) struct Motion {
    pub(in crate::fight) rvo_tree_x_q32: i64,
    pub(in crate::fight) rvo_tree_z_q32: i64,
    pub(in crate::fight) current_velocity_x_q32: i64,
    pub(in crate::fight) current_velocity_z_q32: i64,
    pub(in crate::fight) next_target_x_q32: i64,
    pub(in crate::fight) next_target_z_q32: i64,
    pub(in crate::fight) next_speed_q32: i64,
    pub(in crate::fight) next_max_speed_q32: i64,
    pub(in crate::fight) solver_target_x_q32: i64,
    pub(in crate::fight) solver_target_z_q32: i64,
    pub(in crate::fight) solver_speed_q32: i64,
    pub(in crate::fight) published_target_x_q32: i64,
    pub(in crate::fight) published_target_z_q32: i64,
    pub(in crate::fight) published_speed_q32: i64,
    pub(in crate::fight) rvo_stopped_snap_since_boundary: bool,
    pub(in crate::fight) state: MotionState,
    pub(in crate::fight) attack_hold_fire: bool,
}

pub(in crate::fight) fn rvo_profile(rules: &UnitConfig) -> RvoProfile {
    let profile = rules.rvo;
    RvoProfile {
        outer_radius_q32: space_to_q32(profile.outer_radius()),
        inner_radius_q32: space_to_q32(profile.inner_radius()),
        size: match profile.size {
            RvoSize::Xs => AgentSizeType::Xs,
            RvoSize::S => AgentSizeType::S,
            RvoSize::M => AgentSizeType::M,
            RvoSize::L => AgentSizeType::L,
        },
        collider_priority: profile.collider_priority,
        priority_q32: profile.priority_q32(),
    }
}

pub(in crate::fight) fn movable_rvo_collision_masks(collider_priority: i32) -> (u32, u32) {
    debug_assert!((1..=16).contains(&collider_priority));
    let layer_index = (collider_priority * 2 - 2).cast_unsigned();
    let layer = 1_u32 << layer_index;
    let higher = if layer_index < 30 {
        0x7fff_ffff_u32 & (!0_u32 << (layer_index + 1))
    } else {
        0
    };
    (layer, layer | higher)
}

pub(in crate::fight) fn immovable_rvo_collision_masks(collider_priority: i32) -> (u32, u32) {
    debug_assert!((1..=16).contains(&collider_priority));
    (1_u32 << (collider_priority * 2 - 1), 0)
}

pub(in crate::fight) fn rvo_position(x_q32: i64, z_q32: i64) -> FixedVec2 {
    FixedVec2 {
        x: x_q32.saturating_add(RVO_SIMULATOR_ORIGIN_OFFSET_Q32),
        y: z_q32.saturating_add(RVO_SIMULATOR_ORIGIN_OFFSET_Q32),
    }
}

pub(in crate::fight) fn turn_limited_move_speed_q32(
    base_speed_q32: i64,
    rotate_speed_mdeg_per_second: i64,
    body_rotation_q32: i64,
    velocity_x_q32: i64,
    velocity_z_q32: i64,
) -> i64 {
    const RIGHT_ANGLE_Q32: i64 = 90_i64 << 32;
    const HALF_ROTATION_Q32: i64 = 180_i64 << 32;
    const MIN_SPEED_FACTOR_Q32: i64 = 0x028f_5c28;

    if base_speed_q32 <= 0 || rotate_speed_mdeg_per_second >= 180_000 {
        return base_speed_q32.max(0);
    }
    if velocity_x_q32 == 0 && velocity_z_q32 == 0 {
        return base_speed_q32;
    }
    let velocity_rotation_q32 = direction_degrees_q32_raw(velocity_x_q32, velocity_z_q32);
    let angle_q32 = rotation_distance_q32(body_rotation_q32, velocity_rotation_q32);
    if angle_q32 <= RIGHT_ANGLE_Q32 {
        return base_speed_q32;
    }
    if angle_q32 == HALF_ROTATION_Q32 {
        return q32_mul(base_speed_q32, MIN_SPEED_FACTOR_Q32);
    }
    let factor_q32 = q32_div(HALF_ROTATION_Q32.saturating_sub(angle_q32), RIGHT_ANGLE_Q32);
    q32_mul(base_speed_q32, factor_q32)
}

pub(in crate::fight) fn normalized_velocity_q32_raw(dx: i64, dz: i64, speed: i64) -> (i64, i64) {
    let magnitude = native_q32_magnitude(dx, dz);
    if magnitude <= 0 {
        return (0, 0);
    }

    let inverse_magnitude = q32_div(Q32_ONE, magnitude);
    (
        q32_mul(q32_mul(dx, inverse_magnitude), speed),
        q32_mul(q32_mul(dz, inverse_magnitude), speed),
    )
}

#[allow(clippy::too_many_arguments)]
pub(in crate::fight) fn native_auto_move_target_point(
    source_x_q32: i64,
    source_z_q32: i64,
    source_radius: i64,
    target_x_q32: i64,
    target_z_q32: i64,
    target_radius: i64,
    attack_range: i64,
) -> (i64, i64) {
    const MIN_AUTO_MOVE_DISTANCE_Q32: i64 = 0x028f_5c28;
    let delta_x = target_x_q32.saturating_sub(source_x_q32);
    let delta_z = target_z_q32.saturating_sub(source_z_q32);
    let center_distance = native_q32_magnitude(delta_x, delta_z);
    let target_distance = center_distance
        .saturating_sub(space_to_q32(target_radius))
        .saturating_add(space_to_q32(attack_range))
        .saturating_add(space_to_q32(source_radius));
    let requested_distance = if target_distance > MIN_AUTO_MOVE_DISTANCE_Q32 {
        // Whether the unit is farther than it needs to be is asked of the
        // squared distance, which is exact, against the squared stopping
        // distance, which comes from the fast square root. The two disagree
        // in the last bits where a unit's reach exactly cancels the target's
        // radius, as a Crawler's 6 + 2 does a Marksman's 8: of 24 Crawlers
        // charging one, the game sends the ones whose exact distance is
        // longer to the computed point and the rest to the Marksman itself.
        let squared_distance = q32_mul(delta_x, delta_x).saturating_add(q32_mul(delta_z, delta_z));
        if super::rvo::fpoint_less_than(q32_mul(target_distance, target_distance), squared_distance)
        {
            target_distance
        } else {
            return (target_x_q32, target_z_q32);
        }
    } else {
        MIN_AUTO_MOVE_DISTANCE_Q32
    };
    let (target_delta_x, target_delta_z) =
        normalized_velocity_q32_raw(delta_x, delta_z, requested_distance);
    (
        source_x_q32.saturating_add(target_delta_x),
        source_z_q32.saturating_add(target_delta_z),
    )
}

pub(in crate::fight) fn clamp_magnitude_q32_raw(dx: i64, dz: i64, maximum: i64) -> (i64, i64) {
    let squared_magnitude = q32_mul(dx, dx).saturating_add(q32_mul(dz, dz));
    let squared_maximum = q32_mul(maximum, maximum);
    if !super::rvo::fpoint_less_than(squared_maximum, squared_magnitude) {
        return (dx, dz);
    }
    let magnitude = fpcs_sqrt_fastest(squared_magnitude);
    if magnitude <= 0 || maximum <= 0 {
        return (0, 0);
    }

    let inverse_magnitude = q32_div(Q32_ONE, magnitude);
    (
        q32_mul(q32_mul(dx, inverse_magnitude), maximum),
        q32_mul(q32_mul(dz, inverse_magnitude), maximum),
    )
}

impl Simulation {
    pub(in crate::fight) fn step_actor_rvo_position(&mut self, actor_id: u64) {
        let rvo_boundary_due = self.rvo_counter == 3;
        let changed = {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            if !actor.alive() {
                return;
            }
            let maximum_delta_q32 =
                q32_mul(actor.motion.published_speed_q32, NATIVE_LOGIC_DELTA_Q32);
            let (movement_x_q32, movement_z_q32) = clamp_magnitude_q32_raw(
                actor
                    .motion
                    .published_target_x_q32
                    .saturating_sub(actor.x_q32),
                actor
                    .motion
                    .published_target_z_q32
                    .saturating_sub(actor.z_q32),
                maximum_delta_q32,
            );
            actor.x_q32 = actor.x_q32.saturating_add(movement_x_q32);
            actor.z_q32 = actor.z_q32.saturating_add(movement_z_q32);
            if actor.motion.state != MotionState::Moving
                && actor.motion.published_speed_q32 == 0
                && (movement_x_q32 != 0 || movement_z_q32 != 0)
            {
                actor.motion.rvo_stopped_snap_since_boundary = true;
            }
            if actor.motion.state == MotionState::Moving && !rvo_boundary_due {
                actor.motion.rvo_stopped_snap_since_boundary = false;
            }
            actor.x = q32_to_space_rounded(actor.x_q32);
            actor.z = q32_to_space_rounded(actor.z_q32);
            (
                movement_x_q32 != 0 || movement_z_q32 != 0,
                actor.placement.team,
                actor.x_q32,
                actor.z_q32,
                actor.rules.collision_radius(),
            )
        };
        if changed.0 {
            self.target_quadtrees
                .get_mut(&changed.1)
                .expect("actor team has a target quadtree")
                .position_changed(
                    FightActorRef::Unit(actor_id),
                    changed.2,
                    changed.3,
                    changed.4,
                );
        }
    }

    #[allow(clippy::too_many_lines)]
    pub(in crate::fight) fn step_rvo(&mut self) {
        self.rvo_counter += 1;
        if self.rvo_counter < 4 {
            return;
        }
        self.rvo_counter = 0;
        let first_tree = self.rvo_first_tree_pending;
        for actor in self.actors.values_mut().filter(|actor| actor.alive()) {
            if actor.motion.rvo_stopped_snap_since_boundary
                && actor.motion.state == MotionState::Moving
                && actor.motion.next_speed_q32 > 0
                && actor.motion.solver_speed_q32 == 0
            {
                actor.motion.solver_target_x_q32 = actor.x_q32;
                actor.motion.solver_target_z_q32 = actor.z_q32;
            }
            actor.motion.published_target_x_q32 = actor.motion.solver_target_x_q32;
            actor.motion.published_target_z_q32 = actor.motion.solver_target_z_q32;
            actor.motion.published_speed_q32 = actor.motion.solver_speed_q32;
            (
                actor.motion.current_velocity_x_q32,
                actor.motion.current_velocity_z_q32,
            ) = normalized_velocity_q32_raw(
                actor
                    .motion
                    .published_target_x_q32
                    .saturating_sub(actor.x_q32),
                actor
                    .motion
                    .published_target_z_q32
                    .saturating_sub(actor.z_q32),
                actor.motion.published_speed_q32,
            );
        }

        let mut agents = Vec::new();
        let (tower_layer, tower_collides_with) =
            immovable_rvo_collision_masks(TOWER_RVO_COLLIDER_PRIORITY);
        for building in &self.buildings {
            let construction = self
                .construction_colliders
                .get(&building.building_id)
                .copied();
            if !building_alive(building) || !(rvo_collides(building) || construction.is_some()) {
                continue;
            }
            let (layer, collides_with) = construction.map_or(
                (tower_layer, tower_collides_with),
                immovable_rvo_collision_masks,
            );
            let radius_q32 = building.bounds_width / 2;
            agents.push(RvoAgentInput {
                key: RvoAgentKey::Building(building.building_id),
                main_layer: 1,
                layer,
                collides_with,
                passable_by_own_group: construction.is_some(),
                group: i32::try_from(building.team_id).unwrap_or(i32::MAX),
                locked: true,
                tree_position: if first_tree {
                    FixedVec2::ZERO
                } else {
                    rvo_position(building.position.x, building.position.z)
                },
                position: rvo_position(building.position.x, building.position.z),
                current_velocity: FixedVec2::ZERO,
                desired_velocity: FixedVec2::ZERO,
                desired_target_delta: FixedVec2::ZERO,
                desired_speed: 0,
                max_speed: 0,
                published_calculated_speed: 0,
                radius_outer: radius_q32,
                radius_inner: radius_q32,
                size: AgentSizeType::M,
                priority: Q32_ONE,
            });
        }
        for (&actor_id, actor) in self.actors.iter().filter(|(_, actor)| actor.alive()) {
            let profile = rvo_profile(&actor.rules);
            let (layer, collides_with) = movable_rvo_collision_masks(profile.collider_priority);
            let target_delta = FixedVec2 {
                x: actor.motion.next_target_x_q32.saturating_sub(actor.x_q32),
                y: actor.motion.next_target_z_q32.saturating_sub(actor.z_q32),
            };
            let (desired_x, desired_z) = normalized_velocity_q32_raw(
                target_delta.x,
                target_delta.y,
                actor.motion.next_speed_q32,
            );
            agents.push(RvoAgentInput {
                key: RvoAgentKey::Unit(actor_id),
                main_layer: match actor.rules.domain {
                    UnitDomain::Ground => 1,
                    UnitDomain::Air => 2,
                },
                layer,
                collides_with,
                group: i32::try_from(actor.placement.team).unwrap_or(i32::MAX),
                passable_by_own_group: false,
                locked: false,
                tree_position: if first_tree {
                    FixedVec2::ZERO
                } else {
                    rvo_position(actor.motion.rvo_tree_x_q32, actor.motion.rvo_tree_z_q32)
                },
                position: rvo_position(actor.x_q32, actor.z_q32),
                current_velocity: FixedVec2 {
                    x: actor.motion.current_velocity_x_q32,
                    y: actor.motion.current_velocity_z_q32,
                },
                desired_velocity: FixedVec2 {
                    x: desired_x,
                    y: desired_z,
                },
                desired_target_delta: target_delta,
                desired_speed: actor.motion.next_speed_q32,
                max_speed: actor.motion.next_max_speed_q32,
                published_calculated_speed: actor.motion.published_speed_q32,
                radius_outer: profile.outer_radius_q32,
                radius_inner: profile.inner_radius_q32,
                size: profile.size,
                priority: profile.priority_q32,
            });
        }

        let inverse_delta_time = q32_div(Q32_ONE, NATIVE_LOGIC_DELTA_Q32.saturating_mul(4));
        let solutions = super::rvo::solve_agents(&agents, inverse_delta_time);
        self.rvo_first_tree_pending = false;
        for (&actor_id, actor) in self.actors.iter_mut().filter(|(_, actor)| actor.alive()) {
            let solution = solutions
                .get(&RvoAgentKey::Unit(actor_id))
                .expect("every live actor has an RVO solution");
            actor.motion.solver_target_x_q32 = actor.x_q32.saturating_add(solution.target_delta.x);
            actor.motion.solver_target_z_q32 = actor.z_q32.saturating_add(solution.target_delta.y);
            actor.motion.solver_speed_q32 = solution.speed;
            actor.motion.rvo_tree_x_q32 = actor.x_q32;
            actor.motion.rvo_tree_z_q32 = actor.z_q32;
            actor.motion.rvo_stopped_snap_since_boundary = false;
        }
    }
}

/// Where a unit out of range of what it fires at stands against it, and where
/// its body goes if it moves.
#[derive(Debug, Clone, Copy)]
struct Approach {
    edge_distance_q32: i64,
    target_rotation_q32: i64,
    body_x_q32: i64,
    body_z_q32: i64,
    body_radius: i64,
}

impl Simulation {
    /// `MotionController`'s update, with the `SkillIdleState.TryStartAttack`
    /// it runs into: holding a target that died, attacking one in range, and
    /// leaving one or moving towards it.
    pub(in crate::fight) fn update_motion(
        &mut self,
        actor_id: u64,
        step: u64,
        events: &mut Vec<Event>,
        backswing_just_finished: bool,
        prepare_finished: bool,
        attack_point_rejected: bool,
    ) -> Result<()> {
        if let Flow::Done = self.hold_dead_target(actor_id, backswing_just_finished) {
            return Ok(());
        }
        let target = self.actors[&actor_id].skill.mechanical_attack_target();
        let Some(target) = target else {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.motion.state = MotionState::Idle;
            actor.motion.next_target_x_q32 = actor.x_q32;
            actor.motion.next_target_z_q32 = actor.z_q32;
            actor.motion.next_speed_q32 = 0;
            actor.motion.next_max_speed_q32 = actor.rvo_max_speed_q32;
            return Ok(());
        };
        let target_view = self.fight_actor(target).expect("target identity is stable");
        let target_x_q32 = target_view.x_q32;
        let target_z_q32 = target_view.z_q32;
        let target_radius = target_view.radius;
        // Where the body goes when it moves is the lock's, not the weapons':
        // a unit held by a construction in its line of fire still advances on
        // the unit behind it, and only stops because the construction is in
        // reach. The two coincide in every fight without one.
        let (body_x_q32, body_z_q32, body_radius) = self.actors[&actor_id]
            .skill
            .mechanical_lock_target()
            .and_then(|lock| self.fight_actor(lock))
            .map_or((target_x_q32, target_z_q32, target_radius), |view| {
                (view.x_q32, view.z_q32, view.radius)
            });
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let target_rotation_q32 = direction_degrees_q32_raw(
            target_x_q32.saturating_sub(actor.x_q32),
            target_z_q32.saturating_sub(actor.z_q32),
        );
        let center_distance_q32 = native_q32_magnitude(
            target_x_q32.saturating_sub(actor.x_q32),
            target_z_q32.saturating_sub(actor.z_q32),
        );
        let edge_distance_q32 = center_distance_q32
            .saturating_sub(space_to_q32(actor.rules.collision_radius()))
            .saturating_sub(space_to_q32(target_radius))
            .max(0);
        if actor.rules.attack.weapons.mode == WeaponMode::Group
            && actor.motion.state == MotionState::Attacking
            && actor
                .skill
                .group_skill_targets
                .first()
                .is_some_and(Option::is_none)
            && actor
                .skill
                .group_skill_targets
                .iter()
                .skip(1)
                .any(Option::is_some)
        {
            actor.rotate_body_towards(mdeg_to_degrees_q32(actor.placement.rotation));
            actor.aim_rotation = actor.body_rotation;
            return Ok(());
        }
        if edge_distance_q32 >= space_to_q32(actor.rules.attack.min_range())
            && edge_distance_q32 <= space_to_q32(actor.stats.attack_range())
        {
            return self.attack_in_range(
                actor_id,
                step,
                events,
                target,
                target_rotation_q32,
                backswing_just_finished,
                prepare_finished,
            );
        }
        self.leave_or_approach(
            actor_id,
            Approach {
                edge_distance_q32,
                target_rotation_q32,
                body_x_q32,
                body_z_q32,
                body_radius,
            },
            backswing_just_finished,
            attack_point_rejected,
        );
        Ok(())
    }

    /// A target that died while the unit still swings at it: a unit is left
    /// idle with it through its backswing, a felled block keeps it attacking
    /// and turning to it, and a unit whose backswing just ended drops it.
    fn hold_dead_target(&mut self, actor_id: u64, backswing_just_finished: bool) -> Flow {
        let lock_target = self.actors[&actor_id].skill.mechanical_attack_target();
        if let Some(target) = lock_target {
            let target_alive = self.fight_actor_is_alive(target);
            if !target_alive
                && self.actors[&actor_id]
                    .skill
                    .backswing_finish_step()
                    .is_some()
            {
                // Build 2259 enters idle but retains the dead target through
                // the remaining backswing even when an ally dealt the kill.
                // MotionIdleState.Enter publishes StopMove once; its Update
                // does not refresh that target on every remaining backswing
                // tick.
                // A felled block is held differently: the Rhino of
                // `wall-rhino.yaml` reads attacking, still on the block, until
                // its swing is over, and only then goes idle. That is a block
                // in the way of another lock; a building that was the lock
                // itself is held as a unit is — the Crawlers whose swing the
                // Anti-Armor Turret of `anti-armor-head-on.yaml` fell in read
                // idle on the tick it fell, still on it, until their swing is
                // over.
                let holds_a_block = matches!(target, FightActorRef::Building(_))
                    && (self.actors[&actor_id].skill.lock_target != Some(target)
                        || self.actors[&actor_id].skill.lock_is_terminal_handoff);
                // And keeps turning to it: the Crawlers of `wall-block.yaml`
                // that fell block 5 face it a little more each tick of their
                // swing, as they did while it stood.
                // A tower the match's end tears down is not turned to.
                let holds_a_wall = matches!(target, FightActorRef::Building(building)
                    if self.actors[&actor_id].skill.in_the_way.is_some_and(|(wall, _)| wall == building));
                let held_rotation = self
                    .fight_actor(target)
                    .filter(|_| holds_a_wall)
                    .map(|view| {
                        let actor = &self.actors[&actor_id];
                        direction_degrees_q32_raw(
                            view.x_q32.saturating_sub(actor.x_q32),
                            view.z_q32.saturating_sub(actor.z_q32),
                        )
                    });
                let actor = self
                    .actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable");
                if let Some(rotation) = held_rotation {
                    actor.rotate_weapons_towards(rotation);
                    if actor.rules.has_body {
                        actor.aim_rotation = degrees_q32_to_mdeg(
                            actor
                                .skill
                                .weapon_rotations_q32
                                .first()
                                .copied()
                                .unwrap_or(actor.body_rotation_q32),
                        );
                    } else {
                        actor.rotate_body_towards(rotation);
                        actor.aim_rotation = actor.body_rotation;
                        actor.rotate_weapons_towards(rotation);
                    }
                }
                let entered_idle = actor.motion.state != MotionState::Idle && !holds_a_block;
                if !holds_a_block {
                    actor.motion.state = MotionState::Idle;
                }
                if entered_idle {
                    actor.motion.next_target_x_q32 = actor.x_q32;
                    actor.motion.next_target_z_q32 = actor.z_q32;
                }
                actor.motion.next_speed_q32 = 0;
                actor.motion.next_max_speed_q32 = actor.rvo_max_speed_q32;
                return Flow::Done;
            }
            if !target_alive && backswing_just_finished {
                let actor = self
                    .actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable");
                let entered_idle = actor.motion.state != MotionState::Idle;
                actor.skill.drop_lock();
                actor.skill.retarget_after_own_direct_kill = false;
                actor.motion.state = MotionState::Idle;
                if entered_idle {
                    actor.motion.next_target_x_q32 = actor.x_q32;
                    actor.motion.next_target_z_q32 = actor.z_q32;
                }
                actor.motion.next_speed_q32 = 0;
                actor.motion.next_max_speed_q32 = actor.rvo_max_speed_q32;
                return Flow::Done;
            }
            if !target_alive {
                self.actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable")
                    .skill
                    .drop_lock();
            }
        }
        Flow::Next
    }

    /// `MotionAttackState` with a target in range, and the skill it starts:
    /// `TryStartAttack` from idle, and the next blow's wait once the interval
    /// is up.
    #[allow(
        clippy::too_many_arguments,
        reason = "the motion's reading of its target, handed on from update_motion"
    )]
    fn attack_in_range(
        &mut self,
        actor_id: u64,
        step: u64,
        events: &mut Vec<Event>,
        target: FightActorRef,
        target_rotation_q32: i64,
        backswing_just_finished: bool,
        prepare_finished: bool,
    ) -> Result<()> {
        // `SkillAttackAngleChecker`, against the weapons or, for a unit
        // without a body, its root: the motion does not turn anything before
        // the skill asks.
        let in_attack_angle = self
            .attacker(FightActorRef::Unit(actor_id))
            .expect("actor identity is stable")
            .faces(target_rotation_q32);
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let (entered_attack, release_now, clear_hold_after_motion) = {
            let entered_attack = actor.motion.state != MotionState::Attacking;
            actor.motion.state = MotionState::Attacking;
            // RVOControllerFixed.StopMove refreshes the target point on
            // every MotionAttackState update. It submits zero desired
            // speed while retaining the unit's configured maximum speed,
            // so neighbouring agents can still push a stopped attacker.
            actor.motion.next_target_x_q32 = actor.x_q32;
            actor.motion.next_target_z_q32 = actor.z_q32;
            actor.motion.next_speed_q32 = 0;
            actor.motion.next_max_speed_q32 = actor.rvo_max_speed_q32;
            let completed_attack_reentry_rejected = entered_attack
                && backswing_just_finished
                && actor.rules.attack.melee
                && !actor.rules.has_body
                && !in_attack_angle;
            if completed_attack_reentry_rejected {
                actor.motion.state = MotionState::Idle;
                actor.skill.drop_lock();
                actor.skill.set_phase(FightSkillPhase::Idle);
                actor.motion.next_target_x_q32 = actor.x_q32;
                actor.motion.next_target_z_q32 = actor.z_q32;
                actor.motion.next_speed_q32 = 0;
                actor.motion.next_max_speed_q32 = actor.rvo_max_speed_q32;
                return Ok(());
            }
            if entered_attack {
                // A newly entered bodyless attack state cannot start its
                // FightSkill while the root transform is outside the
                // attack cone. MotionController clears this hold only
                // after it has observed and corrected the facing.
                actor.motion.attack_hold_fire = !actor.rules.has_body
                    && !in_attack_angle
                    && (actor.rules.attack.melee
                        || matches!(
                            actor.rules.attack.path,
                            AttackPath::Projectile { .. } | AttackPath::Laser { .. }
                        ));
            }
            let invalid_attack_angle_barrier = !actor.rules.has_body
                && !entered_attack
                && !actor.motion.attack_hold_fire
                && !in_attack_angle
                && actor.skill.pending().is_none()
                && actor.skill.backswing_finish_step().is_none();
            if invalid_attack_angle_barrier {
                // MotionAttackState returns to Idle when an active bodyless
                // skill loses its root-transform attack angle. The new
                // Idle state is entered synchronously but is not updated
                // recursively, so target reacquisition waits one tick and
                // this transition tick preserves the old body facing.
                actor.motion.state = MotionState::Idle;
                actor.skill.drop_lock();
                actor.skill.set_phase(FightSkillPhase::Idle);
                actor.motion.next_target_x_q32 = actor.x_q32;
                actor.motion.next_target_z_q32 = actor.z_q32;
                actor.motion.next_speed_q32 = 0;
                actor.motion.next_max_speed_q32 = actor.rvo_max_speed_q32;
                return Ok(());
            }
            let clear_hold_after_motion = actor.motion.attack_hold_fire && in_attack_angle;
            self.try_start_attack(
                FightActorRef::Unit(actor_id),
                step,
                target,
                entered_attack,
                in_attack_angle,
                prepare_finished,
            );
            (
                entered_attack,
                self.actors[&actor_id]
                    .skill
                    .pending()
                    .is_some_and(|pending| pending.step == step),
                clear_hold_after_motion,
            )
        };
        if release_now {
            let _attack_point_rejected = self.release(FightActorRef::Unit(actor_id), events)?;
        }
        if self.actors[&actor_id].motion.state != MotionState::Attacking {
            // FightSkill runs before MotionController. A laser own-kill
            // exits MotionAttackState during the skill update, so the
            // killed target is retained for the snapshot but cannot drive
            // another root rotation in the same tick.
            return Ok(());
        }
        if entered_attack {
            // SimpleFSM enters MotionAttackState synchronously but does not
            // update the newly entered state in the same tick. FightSkill
            // therefore starts tracking the target on the next tick.
            return Ok(());
        }
        self.track_target_in_range(actor_id, target_rotation_q32, clear_hold_after_motion);
        Ok(())
    }

    /// `FightSkill.Update` and `MotionAttackState` turning the weapons, and a
    /// bodyless root, to a target in range, and letting a held attack go once
    /// the facing is right.
    fn track_target_in_range(
        &mut self,
        actor_id: u64,
        target_rotation_q32: i64,
        clear_hold_after_motion: bool,
    ) {
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        // FightSkill.Update rotates every free weapon after its state controller.
        actor.rotate_weapons_towards(target_rotation_q32);
        if actor.rules.has_body {
            actor.aim_rotation = degrees_q32_to_mdeg(
                actor
                    .skill
                    .weapon_rotations_q32
                    .first()
                    .copied()
                    .unwrap_or(actor.body_rotation_q32),
            );
        } else {
            actor.rotate_body_towards(target_rotation_q32);
            actor.aim_rotation = actor.body_rotation;
            // MotionAttackState subsequently asks the active FightSkill to rotate its weapons.
            actor.rotate_weapons_towards(target_rotation_q32);
        }
        if clear_hold_after_motion {
            // FightMech runs SkillManager before MotionController. The
            // current skill update therefore remains held; clearing here
            // makes the attack eligible on the following logic tick.
            actor.motion.attack_hold_fire = false;
        }
        if actor.rules.has_body
            && (actor.motion.current_velocity_x_q32 != 0
                || actor.motion.current_velocity_z_q32 != 0)
        {
            actor.rotate_body_towards(direction_degrees_q32_raw(
                actor.motion.current_velocity_x_q32,
                actor.motion.current_velocity_z_q32,
            ));
        }
    }

    /// A target out of range: the attack motion is left, through idle where
    /// the build does, or the unit moves towards where its lock stands.
    fn leave_or_approach(
        &mut self,
        actor_id: u64,
        approach: Approach,
        backswing_just_finished: bool,
        attack_point_rejected: bool,
    ) {
        if let Flow::Done =
            self.leave_attack_range(actor_id, backswing_just_finished, attack_point_rejected)
        {
            return;
        }
        self.approach(actor_id, approach);
    }

    /// The ways a unit leaves its attack motion when what it fires at is out
    /// of range: a grouped root turning back while its slots still fire, and
    /// the bodyless exits through idle.
    fn leave_attack_range(
        &mut self,
        actor_id: u64,
        backswing_just_finished: bool,
        attack_point_rejected: bool,
    ) -> Flow {
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        if actor.rules.attack.weapons.mode == WeaponMode::Group
            && actor.motion.state == MotionState::Attacking
            && actor
                .skill
                .group_skill_targets
                .iter()
                .skip(1)
                .any(Option::is_some)
        {
            // GroupedSkillAttackBehaviour keeps the group attacking while any
            // child FightSkill remains in SkillAttackState. When the core
            // target is outside its own range, the bodyless root returns to
            // the deployment facing while child weapons keep their locks.
            actor.rotate_body_towards(mdeg_to_degrees_q32(actor.placement.rotation));
            actor.aim_rotation = actor.body_rotation;
            return Flow::Done;
        }
        if actor.motion.state == MotionState::Attacking
            && !actor.rules.has_body
            && !actor.motion.attack_hold_fire
            && actor.skill.pending().is_none()
            && actor.skill.backswing_finish_step().is_none()
            && (actor.rules.attack.melee || actor.skill.phase() == FightSkillPhase::Attack)
        {
            // FightSkill updates before MotionController. An active bodyless
            // attack rejects an out-of-range retained target and enters
            // SkillIdleState before MotionAttackState can fall through to
            // movement. Both state machines expose one targetless Idle tick.
            actor.motion.state = MotionState::Idle;
            actor.skill.drop_lock();
            actor.skill.set_phase(FightSkillPhase::Idle);
            actor.motion.attack_hold_fire = false;
            actor.motion.next_target_x_q32 = actor.x_q32;
            actor.motion.next_target_z_q32 = actor.z_q32;
            actor.motion.next_speed_q32 = 0;
            actor.motion.next_max_speed_q32 = actor.rvo_max_speed_q32;
            return Flow::Done;
        }
        if attack_point_rejected
            && actor.rules.attack.melee
            && !actor.rules.has_body
            && actor.motion.state == MotionState::Attacking
            && actor.skill.pending().is_none()
            && actor.skill.backswing_finish_step().is_none()
        {
            // MotionAttackState leaves through Idle when its current attack
            // target is no longer in range. Idle target acquisition runs on
            // the following update rather than recursively entering Moving.
            actor.motion.state = MotionState::Idle;
            actor.skill.drop_lock();
            actor.skill.set_phase(FightSkillPhase::Idle);
            actor.motion.next_target_x_q32 = actor.x_q32;
            actor.motion.next_target_z_q32 = actor.z_q32;
            actor.motion.next_speed_q32 = 0;
            actor.motion.next_max_speed_q32 = actor.rvo_max_speed_q32;
            return Flow::Done;
        }
        if backswing_just_finished && actor.rules.attack.melee && !actor.rules.has_body {
            // SkillAttackState rechecks its retained target after the attack
            // controller finishes. If that target has left the legal attack
            // area, Finish synchronously enters SkillIdleState; SimpleFSM does
            // not update the new state recursively, so MotionController sees
            // one targetless Idle tick before reacquisition on the next tick.
            actor.motion.state = MotionState::Idle;
            actor.skill.drop_lock();
            actor.skill.set_phase(FightSkillPhase::Idle);
            actor.motion.next_target_x_q32 = actor.x_q32;
            actor.motion.next_target_z_q32 = actor.z_q32;
            actor.motion.next_speed_q32 = 0;
            actor.motion.next_max_speed_q32 = actor.rvo_max_speed_q32;
            return Flow::Done;
        }
        Flow::Next
    }

    /// `MotionMoveState`: moves towards where the lock stands, turning first.
    fn approach(&mut self, actor_id: u64, approach: Approach) {
        let Approach {
            edge_distance_q32,
            target_rotation_q32,
            body_x_q32,
            body_z_q32,
            body_radius,
        } = approach;
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let entered_move_from_idle = actor.motion.state == MotionState::Idle;
        let entered_move = actor.motion.state != MotionState::Moving;
        let entered_move_below_min_range =
            entered_move && edge_distance_q32 < space_to_q32(actor.rules.attack.min_range());
        if !entered_move_from_idle && !entered_move_below_min_range {
            // FightSkill.Update tracks an existing target before MotionController updates movement.
            // A target acquired by MotionIdleState is not visible to FightSkill until the next tick.
            actor.rotate_weapons_towards(target_rotation_q32);
            if actor.rules.has_body {
                actor.aim_rotation = degrees_q32_to_mdeg(
                    actor
                        .skill
                        .weapon_rotations_q32
                        .first()
                        .copied()
                        .unwrap_or(actor.body_rotation_q32),
                );
            }
        }
        actor.motion.state = MotionState::Moving;
        actor.motion.attack_hold_fire = false;
        if entered_move {
            return;
        }
        let (move_target_x_q32, move_target_z_q32) = native_auto_move_target_point(
            actor.x_q32,
            actor.z_q32,
            actor.rules.collision_radius(),
            body_x_q32,
            body_z_q32,
            body_radius,
            actor.stats.attack_range(),
        );
        actor.motion.next_target_x_q32 = move_target_x_q32;
        actor.motion.next_target_z_q32 = move_target_z_q32;
        if actor.motion.current_velocity_x_q32 != 0 || actor.motion.current_velocity_z_q32 != 0 {
            // MotionMoveState.MoveUpdate runs NormalRotate before Move;
            // CalculateMoveSpeed therefore observes this tick's new facing.
            actor.rotate_body_towards(direction_degrees_q32_raw(
                actor.motion.current_velocity_x_q32,
                actor.motion.current_velocity_z_q32,
            ));
        }
        actor.motion.next_speed_q32 = turn_limited_move_speed_q32(
            actor.stats.move_speed_q32(),
            actor.rules.rotate_speed_mdeg_per_second(),
            actor.body_rotation_q32,
            actor.motion.current_velocity_x_q32,
            actor.motion.current_velocity_z_q32,
        );
        actor.motion.next_max_speed_q32 = actor.motion.next_speed_q32;
        if !actor.rules.has_body {
            actor.aim_rotation = actor.body_rotation;
        }
    }
}
