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
        if crate::rvo::fpoint_less_than(q32_mul(target_distance, target_distance), squared_distance)
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
    if !crate::rvo::fpoint_less_than(squared_maximum, squared_magnitude) {
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
            immovable_rvo_collision_masks(CORE_TOWER_RVO_COLLIDER_PRIORITY);
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
        let solutions = crate::rvo::solve_agents(&agents, inverse_delta_time);
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
