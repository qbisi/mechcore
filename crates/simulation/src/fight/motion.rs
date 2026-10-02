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

/// The RVO simulator's own state, and the obstacles it holds besides the
/// units: `RVOSimulatorFixed` and the colliders the buildings activate.
pub(in crate::fight) struct RvoState {
    /// The ticks since the last quadtree boundary.
    pub(in crate::fight) counter: u8,
    // Native Agent::.ctor stores its initial position in the public backing
    // buffer. The internal position read by the first BuildQuadtree remains
    // zero until the subsequent BufferSwitch.
    pub(in crate::fight) first_tree_pending: bool,
    pub(in crate::fight) quadtree_capacity: rvo::QuadtreeCapacity,
    /// The RVO collider layer of every construction, by building.
    ///
    /// A construction is an obstacle only to the other side: the wall's own
    /// description says it sinks into the ground for a friendly unit, and a
    /// Crawler of the side that placed it walks through a block. To the other
    /// side it is an immovable agent on its `pathfinding_collider_priority`
    /// layer with its own box for a radius — a Steel Ball of `wall-laser.yaml`
    /// overlapping block 3 is pushed off it as that agent pushes it, tick for
    /// tick, and every other wall fight is unchanged by it.
    pub(in crate::fight) construction_colliders: BTreeMap<u64, i32>,
    /// The constructions their own side passes through
    /// (`FightConstruction.IsEnableBlock`); a turret is not one.
    pub(in crate::fight) passable_constructions: BTreeSet<u64>,
    /// The map's neutral crystals that take part in movement, in the order
    /// the map lists them, which is the order they enter the RVO tree after
    /// the towers.
    pub(in crate::fight) map_crystals: Vec<MapCrystal>,
}

impl RvoState {
    pub(in crate::fight) fn new(
        construction_colliders: BTreeMap<u64, i32>,
        passable_constructions: BTreeSet<u64>,
        map_crystals: Vec<MapCrystal>,
    ) -> Self {
        Self {
            counter: 0,
            first_tree_pending: true,
            quadtree_capacity: rvo::QuadtreeCapacity::default(),
            construction_colliders,
            passable_constructions,
            map_crystals,
        }
    }
}

/// `MotionController`: what the body does — its state, where it has been
/// asked to go and how fast, and what the RVO solver made of that.
#[derive(Debug, Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the motion mirrors the native agent's independent state flags"
)]
pub(in crate::fight) struct Motion {
    pub(in crate::fight) rvo_tree_x_q32: i64,
    pub(in crate::fight) rvo_tree_z_q32: i64,
    /// Whether its agent was made afresh since the last solve.
    pub(in crate::fight) rvo_fresh: bool,
    /// Whether its agent was made since the last tree was built: a new
    /// agent's first tree reads its position as zero
    /// (`docs/spec/simulation/rvo.md`).
    pub(in crate::fight) rvo_new_agent: bool,
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
            RvoSize::Xl => AgentSizeType::Xl,
            RvoSize::Xxl => AgentSizeType::Xxl,
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
    free_move: bool,
    rotate_speed_mdeg_per_second: i64,
    body_rotation_q32: i64,
    velocity_x_q32: i64,
    velocity_z_q32: i64,
) -> i64 {
    const RIGHT_ANGLE_Q32: i64 = 90_i64 << 32;
    const HALF_ROTATION_Q32: i64 = 180_i64 << 32;
    const MIN_SPEED_FACTOR_Q32: i64 = 0x028f_5c28;

    // `MotionController.CalculateMoveSpeed` answers a free-moving unit's
    // full speed before it looks at the turn.
    if free_move || base_speed_q32 <= 0 || rotate_speed_mdeg_per_second >= 180_000 {
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
        let rvo_boundary_due = self.rvo.counter == 3;
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
            if let Some(tree) = self.mech_quadtrees.get_mut(&changed.1) {
                tree.position_changed(
                    FightActorRef::Unit(actor_id),
                    changed.2,
                    changed.3,
                    changed.4,
                );
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    pub(in crate::fight) fn step_rvo(&mut self) {
        self.rvo.counter += 1;
        if self.rvo.counter < 4 {
            return;
        }
        self.rvo.counter = 0;
        let first_tree = self.rvo.first_tree_pending;
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
        let immovable = |key, layer, collides_with, group, x, z, radius, passable| RvoAgentInput {
            key,
            main_layer: 1,
            layer,
            collides_with,
            passable_by_own_group: passable,
            group,
            locked: true,
            tree_position: if first_tree {
                FixedVec2::ZERO
            } else {
                rvo_position(x, z)
            },
            position: rvo_position(x, z),
            current_velocity: FixedVec2::ZERO,
            desired_velocity: FixedVec2::ZERO,
            desired_target_delta: FixedVec2::ZERO,
            desired_speed: 0,
            max_speed: 0,
            published_calculated_speed: 0,
            radius_outer: radius,
            radius_inner: radius,
            size: AgentSizeType::M,
            priority: Q32_ONE,
        };
        let building_agent = |building: &BuildingState| {
            let construction = self
                .rvo
                .construction_colliders
                .get(&building.building_id)
                .copied();
            if !building_alive(building) || !(rvo_collides(building) || construction.is_some()) {
                return None;
            }
            let (layer, collides_with) = construction.map_or(
                (tower_layer, tower_collides_with),
                immovable_rvo_collision_masks,
            );
            Some(immovable(
                RvoAgentKey::Building(building.building_id),
                layer,
                collides_with,
                i32::try_from(building.team_id).unwrap_or(i32::MAX),
                building.position.x,
                building.position.z,
                building.bounds_width / 2,
                // `RVOControllerFixed.RefreshGroup` lets a construction's own
                // side through when its row answers `IsEnableBlock`: a wall's
                // block, not a turret. An interceptor is no construction.
                self.rvo
                    .passable_constructions
                    .contains(&building.building_id),
            ))
        };
        // Each side's buildings, its towers and then its constructions, side
        // by side, then the map's crystals in the order the map lists them:
        // the order their RVO controllers activate in when the fight starts.
        let mut teams = self
            .buildings
            .iter()
            .map(|building| building.team_id)
            .collect::<Vec<_>>();
        teams.sort_unstable();
        teams.dedup();
        for team in teams {
            let (constructions, towers): (Vec<_>, Vec<_>) = self
                .buildings
                .iter()
                .filter(|building| building.team_id == team)
                .partition(|building| {
                    self.rvo
                        .construction_colliders
                        .contains_key(&building.building_id)
                });
            agents.extend(towers.into_iter().filter_map(building_agent));
            agents.extend(constructions.into_iter().filter_map(building_agent));
        }
        for (index, crystal) in self.rvo.map_crystals.iter().enumerate() {
            let (layer, collides_with) = immovable_rvo_collision_masks(crystal.collider_priority);
            agents.push(immovable(
                RvoAgentKey::MapBuilding(index),
                layer,
                collides_with,
                NEUTRAL_RVO_GROUP,
                crystal.x_q32,
                crystal.z_q32,
                crystal.radius_q32,
                false,
            ));
        }
        // A summon still appearing has its agent already, where it was made,
        // locked: `CreateMechDelay` locks its movement. Crawlers still
        // surfacing turn the Crawlers already up aside.
        let units = self
            .actors
            .iter()
            .filter(|(_, actor)| actor.alive() && !actor.travelling)
            .map(|(&actor_id, actor)| (actor_id, actor, false))
            .chain(
                self.appearing_actors()
                    .map(|actor| (actor.placement.unit_id, actor, true)),
            );
        for (actor_id, actor, appearing) in units {
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
                locked: appearing,
                tree_position: if first_tree || actor.motion.rvo_new_agent {
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
        let solutions =
            super::rvo::solve_agents(&agents, inverse_delta_time, &mut self.rvo.quadtree_capacity);
        self.rvo.first_tree_pending = false;
        for (&actor_id, actor) in self
            .actors
            .iter_mut()
            .filter(|(_, actor)| actor.alive() && !actor.travelling)
        {
            actor.motion.rvo_new_agent = false;
            if actor.motion.rvo_fresh {
                actor.motion.rvo_tree_x_q32 = actor.x_q32;
                actor.motion.rvo_tree_z_q32 = actor.z_q32;
                continue;
            }
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
        update: SkillUpdate,
    ) -> Result<()> {
        let SkillUpdate {
            backswing_just_finished,
            prepare_finished,
            ..
        } = update;
        if let Flow::Done = self.hold_dead_target_moving(actor_id, backswing_just_finished) {
            return Ok(());
        }
        let target = self.actors[&actor_id].skill.attack_target();
        if target.is_none() && self.actors[&actor_id].command.is_some() {
            // A command is active without a target: every motion state
            // goes to or stays in `MotionMoveState`, which walks the path.
            self.follow_command(actor_id);
            return Ok(());
        }
        let Some(target) = target else {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            // `MotionIdleState.Enter` stops the move, and its `Update` does
            // not: an idle unit an RVO solve nudged keeps the target point it
            // stopped at, and the next solve steers it back there.
            if actor.motion.state != MotionState::Idle {
                actor.motion.next_target_x_q32 = actor.x_q32;
                actor.motion.next_target_z_q32 = actor.z_q32;
                actor.motion.next_speed_q32 = 0;
                actor.motion.next_max_speed_q32 = actor.rvo_max_speed_q32;
            }
            actor.motion.state = MotionState::Idle;
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
            .lock_target
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
            && actor.skill.lock_target.is_none()
            && actor
                .skill
                .siblings()
                .iter()
                .any(|slot| slot.lock_target.is_some())
        {
            actor.rotate_body_towards(mdeg_to_degrees_q32(actor.placement.rotation));
            actor.aim_rotation = actor.body_rotation;
            return Ok(());
        }
        let in_reach = if actor.skill.shield_target().is_some() {
            // A skill firing at a shield stops its unit once the shield's
            // surface is in reach, however far the lock stands behind it.
            self.target_in_attack_range(FightActorRef::Unit(actor_id), target)
        } else {
            edge_distance_q32 >= space_to_q32(actor.rules.attack.min_range())
                && edge_distance_q32 <= space_to_q32(actor.stats.attack_range())
        };
        if in_reach {
            let was_attacking = self.actors[&actor_id].motion.state == MotionState::Attacking;
            self.attack_in_range(
                actor_id,
                step,
                events,
                target,
                target_rotation_q32,
                backswing_just_finished,
                prepare_finished,
            )?;
            self.attack_move(actor_id, was_attacking);
            return Ok(());
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
            update,
        );
        Ok(())
    }

    /// `MotionAttackState.Update` under a command, whose `IsIdle` and
    /// `IsActive` do not ask whether the lock lives: the motion leaves the
    /// attack only once its target is out of range, so a dead target still in
    /// range keeps the unit attacking, turning to it (`AttackRotate`) and
    /// walking on or stopping as `AttackMove` decides, until its skill takes
    /// another.
    fn attack_dead_lock_under_command(&mut self, actor_id: u64) -> Flow {
        let actor = &self.actors[&actor_id];
        if actor.command.is_none() || actor.motion.state != MotionState::Attacking {
            return Flow::Next;
        }
        let Some(target) = actor.skill.attack_target() else {
            return Flow::Next;
        };
        if self.fight_actor_is_alive(target) {
            return Flow::Next;
        }
        let Some(view) = self.fight_actor(target) else {
            return Flow::Next;
        };
        let target_rotation_q32 = direction_degrees_q32_raw(
            view.x_q32.saturating_sub(actor.x_q32),
            view.z_q32.saturating_sub(actor.z_q32),
        );
        let edge_distance_q32 = native_q32_magnitude(
            view.x_q32.saturating_sub(actor.x_q32),
            view.z_q32.saturating_sub(actor.z_q32),
        )
        .saturating_sub(space_to_q32(actor.rules.collision_radius()))
        .saturating_sub(space_to_q32(view.radius))
        .max(0);
        let in_reach = edge_distance_q32 >= space_to_q32(actor.rules.attack.min_range())
            && edge_distance_q32 <= space_to_q32(actor.stats.attack_range());
        if !in_reach {
            return Flow::Next;
        }
        if !self.command_attack_moves(actor_id) {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.motion.next_target_x_q32 = actor.x_q32;
            actor.motion.next_target_z_q32 = actor.z_q32;
            actor.motion.next_speed_q32 = 0;
            actor.motion.next_max_speed_q32 = actor.rvo_max_speed_q32;
        }
        self.track_target_in_range(actor_id, target_rotation_q32, false);
        self.attack_move(actor_id, true);
        Flow::Done
    }

    /// `hold_dead_target`, and under a command, which stays active with a
    /// dead target: in range it keeps attacking
    /// ([`Self::attack_dead_lock_under_command`]); out of range, where
    /// `AutoMoveBehaviour` goes idle, the unit changes to `MotionMoveState`,
    /// or walks on in it.
    fn hold_dead_target_moving(&mut self, actor_id: u64, backswing_just_finished: bool) -> Flow {
        if let Flow::Done = self.attack_dead_lock_under_command(actor_id) {
            return Flow::Done;
        }
        let was_moving = self.actors[&actor_id].motion.state == MotionState::Moving;
        let Flow::Done = self.hold_dead_target(actor_id, backswing_just_finished) else {
            return Flow::Next;
        };
        if self.actors[&actor_id].command.is_some()
            && self.actors[&actor_id].motion.state == MotionState::Idle
        {
            if was_moving {
                self.move_to_command_point(actor_id);
            }
            self.actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .motion
                .state = MotionState::Moving;
        }
        Flow::Done
    }

    /// A target that died while the unit still swings at it: a unit is left
    /// idle with it through its backswing, a felled block keeps it attacking
    /// and turning to it, and a unit whose backswing just ended drops it.
    fn hold_dead_target(&mut self, actor_id: u64, backswing_just_finished: bool) -> Flow {
        let lock_target = self.actors[&actor_id].skill.attack_target();
        if let Some(target) = lock_target {
            let target_alive = self.fight_actor_is_alive(target);
            // `MotionAttackState.Update` asks the lock, not what the weapons
            // fire at: a block that falls in the way of a live lock leaves the
            // unit attacking, still on the block, until the skill's next
            // check finishes the attack: a Vortex that fells a block with a
            // single blow reads attacking on it that tick, with no backswing
            // to wait out.
            let lock = self.actors[&actor_id].skill.lock_target;
            let block_before_live_lock = !target_alive
                && matches!(target, FightActorRef::Building(_))
                && lock != Some(target)
                && lock.is_some_and(|lock| self.fight_actor_is_alive(lock));
            // With its lock alive, `MotionAttackState.Update` still asks
            // whether the fallen block is in reach, where it stood: a
            // Crawler pushed out of reach of the block an ally felled
            // during its backswing walks on towards its lock the next
            // update, while one still in reach goes on attacking.
            if block_before_live_lock && !self.reaches_where_it_stood(actor_id, target) {
                return Flow::Next;
            }
            if !target_alive
                && (block_before_live_lock
                    || self.actors[&actor_id]
                        .skill
                        .backswing_finish_step()
                        .is_some())
            {
                // The build enters idle but retains the dead target through
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
                    && lock != Some(target)
                    && lock.is_some_and(|lock| self.fight_actor_is_alive(lock));
                // And keeps turning through its swing, as it did while the
                // block stood. A tower the match's end tears down is not
                // turned to.
                self.turn_past_fallen_wall(actor_id, target);
                let actor = self
                    .actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable");
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
                actor.motion.state = MotionState::Idle;
                if entered_idle {
                    actor.motion.next_target_x_q32 = actor.x_q32;
                    actor.motion.next_target_z_q32 = actor.z_q32;
                }
                actor.motion.next_speed_q32 = 0;
                actor.motion.next_max_speed_q32 = actor.rvo_max_speed_q32;
                return Flow::Done;
            }
            let died_this_tick = self
                .fight_actor(target)
                .is_some_and(|view| view.query_alive && !view.alive);
            // A target that died this tick is held through it even with no
            // backswing to wait out: `MotionAttackState.Update` goes idle once
            // its lock is dead, and the skill, which updated before the
            // motion, sees the death only the update after. A Vortex reads
            // idle on its kill, still on the dead unit, and attacks the next
            // one the tick after, as a Rhino does once its backswing is over.
            if !target_alive && died_this_tick {
                let actor = self
                    .actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable");
                let entered_idle = actor.motion.state != MotionState::Idle;
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
            // And the other way round: a lock that died behind a block that
            // stands leaves the unit idle, still on both, until the skill
            // searches again.
            if target_alive
                && lock.is_some_and(|lock| lock != target && !self.fight_actor_is_alive(lock))
            {
                let actor = self
                    .actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable");
                let entered_idle = actor.motion.state != MotionState::Idle;
                actor.motion.state = MotionState::Idle;
                if entered_idle {
                    actor.motion.next_target_x_q32 = actor.x_q32;
                    actor.motion.next_target_z_q32 = actor.z_q32;
                }
                actor.motion.next_speed_q32 = 0;
                actor.motion.next_max_speed_q32 = actor.rvo_max_speed_q32;
                return Flow::Done;
            }
        }
        Flow::Next
    }

    /// Where a unit turns once the wall block in its way has fallen.
    ///
    /// `MotionController.CalculateTargetDirection` faces the attack target
    /// only while `FightSkill.TryGetValidAttackTarget` finds it alive, and the
    /// lock otherwise, so a felled block leaves the unit turning to the unit
    /// behind it. `wall-block.yaml` and
    /// `wall-rhino.yaml` measure it for a bodyless root; a unit with a body
    /// turns its weapons, and whether they follow the lock is not measured,
    /// so they stay on the block.
    /// Whether a unit's attack reaches a target where it stands, dead or
    /// alive: the range half of `IsAttackTargetInAttackRange`.
    fn reaches_where_it_stood(&self, actor_id: u64, target: FightActorRef) -> bool {
        let (Some(attacker), Some(view)) = (
            self.attacker(FightActorRef::Unit(actor_id)),
            self.fight_actor(target),
        ) else {
            return false;
        };
        attacker.reaches(view.x_q32, view.z_q32, view.radius)
    }

    fn turn_past_fallen_wall(&mut self, actor_id: u64, target: FightActorRef) {
        let skill = &self.actors[&actor_id].skill;
        let FightActorRef::Building(building) = target else {
            return;
        };
        if skill.in_the_way.is_none_or(|(wall, _)| wall != building) {
            return;
        }
        let has_body = self.actors[&actor_id].rules.has_body;
        let facing = if has_body {
            Some(target)
        } else {
            skill
                .lock_target
                .filter(|lock| self.fight_actor_is_alive(*lock))
        };
        let Some(rotation) = facing
            .and_then(|facing| self.fight_actor(facing))
            .map(|view| {
                let actor = &self.actors[&actor_id];
                direction_degrees_q32_raw(
                    view.x_q32.saturating_sub(actor.x_q32),
                    view.z_q32.saturating_sub(actor.z_q32),
                )
            })
        else {
            return;
        };
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        actor.rotate_weapons_towards(rotation);
        if has_body {
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

    /// Where a bodyless unit's lock stands from it, if the lock is alive: what
    /// `CalculateTargetDirection` faces once the attack target is not valid.
    /// A unit with a body turns its weapons, and whether they follow the lock
    /// is not recorded, so it answers `None`.
    fn lock_rotation_for_bodyless(&self, actor_id: u64) -> Option<i64> {
        let actor = &self.actors[&actor_id];
        if actor.rules.has_body {
            return None;
        }
        let lock = actor
            .skill
            .lock_target
            .filter(|lock| self.fight_actor_is_alive(*lock))?;
        let view = self.fight_actor(lock)?;
        Some(direction_degrees_q32_raw(
            view.x_q32.saturating_sub(actor.x_q32),
            view.z_q32.saturating_sub(actor.z_q32),
        ))
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
        // `AttackMove` walks a command on with `Move` instead of stopping.
        let walks_on = self.actors[&actor_id].motion.state == MotionState::Attacking
            && self.command_attack_moves(actor_id);
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let (entered_attack, release_now, clear_hold_after_motion) = {
            let entered_attack = actor.motion.state != MotionState::Attacking;
            actor.motion.state = MotionState::Attacking;
            // RVOControllerFixed.StopMove refreshes the target point on
            // every MotionAttackState update that does not walk on. It
            // submits zero desired speed while retaining the unit's
            // configured maximum speed, so neighbouring agents can still
            // push a stopped attacker.
            if !walks_on {
                actor.motion.next_target_x_q32 = actor.x_q32;
                actor.motion.next_target_z_q32 = actor.z_q32;
                actor.motion.next_speed_q32 = 0;
                actor.motion.next_max_speed_q32 = actor.rvo_max_speed_q32;
            }
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
                actor.motion.attack_hold_fire = !actor.rules.has_body && !in_attack_angle;
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
        // `MotionAttackState.AttackRotate` turns after the release, towards
        // `CalculateTargetDirection`: the lock, once the blow just released
        // has felled what it fired at. The Steel Ball of `wall-laser.yaml`
        // turns onto the Marksman on the tick its beam fells block 4.
        let target_rotation_q32 = if release_now && !self.fight_actor_is_alive(target) {
            self.lock_rotation_for_bodyless(actor_id)
                .unwrap_or(target_rotation_q32)
        } else {
            target_rotation_q32
        };
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
            // `IsFreeFireMove`: a command walking a unit that fires all
            // round turns its root to where it moves, not to the target.
            let free_fire_move =
                actor.command.is_some() && actor.rules.attack.attack_half_angle_mdeg() >= 360_000;
            if !free_fire_move {
                actor.rotate_body_towards(target_rotation_q32);
            } else if actor.motion.current_velocity_x_q32 != 0
                || actor.motion.current_velocity_z_q32 != 0
            {
                actor.rotate_body_towards(direction_degrees_q32_raw(
                    actor.motion.current_velocity_x_q32,
                    actor.motion.current_velocity_z_q32,
                ));
            }
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
    fn leave_or_approach(&mut self, actor_id: u64, approach: Approach, update: SkillUpdate) {
        if let Flow::Done = self.leave_attack_range(actor_id, update) {
            // A skill that let its target go leaves `MotionAttackState` for
            // `MotionMoveState` under a command, which stays active without
            // one.
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            if actor.command.is_some() && actor.motion.state == MotionState::Idle {
                actor.motion.state = MotionState::Moving;
            }
            return;
        }
        self.approach(actor_id, approach);
    }

    /// `MotionAttackState.AttackMove`: a command may walk on while the unit
    /// fires, where `AutoMoveBehaviour` stops. A state entered this update
    /// is not updated on it. `MotionController.Move` does not turn the body;
    /// only the move state's `NormalRotate` turns it to where it moves.
    fn attack_move(&mut self, actor_id: u64, was_attacking: bool) {
        if was_attacking
            && self.actors[&actor_id].motion.state == MotionState::Attacking
            && self.command_attack_moves(actor_id)
        {
            let Some((move_target_x_q32, move_target_z_q32)) = self.command_move_point(actor_id)
            else {
                return;
            };
            let solve_due = self.rvo_solve_due();
            self.actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .move_to(move_target_x_q32, move_target_z_q32, solve_due);
        }
    }

    /// The motion states under a command with nothing to fire at:
    /// `MotionIdleState` and `MotionAttackState` change to
    /// `MotionMoveState`, which is not updated the tick it is entered, and
    /// `MotionMoveState` walks towards the command's point.
    pub(in crate::fight) fn follow_command(&mut self, actor_id: u64) {
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        if actor.motion.state != MotionState::Moving {
            actor.motion.state = MotionState::Moving;
            actor.motion.attack_hold_fire = false;
            return;
        }
        self.move_to_command_point(actor_id);
        // `NormalRotate` with nothing to face turns the weapons to where the
        // body faces.
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let facing = actor.body_rotation_q32;
        actor.rotate_weapons_towards(facing);
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

    /// `MotionMoveState.MoveUpdate` under a command: `NormalRotate`, then
    /// `MotionController.Move` towards the command's point.
    fn move_to_command_point(&mut self, actor_id: u64) {
        let Some((move_target_x_q32, move_target_z_q32)) = self.command_move_point(actor_id) else {
            return;
        };
        let solve_due = self.rvo_solve_due();
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        actor.turn_to_move_direction();
        actor.move_to(move_target_x_q32, move_target_z_q32, solve_due);
    }

    /// Whether this update is the one before the RVO solve, the only one on
    /// which `MotionController.Move` hands the agent anything.
    fn rvo_solve_due(&self) -> bool {
        self.rvo.counter == 3
    }

    /// The ways a unit leaves its attack motion when what it fires at is out
    /// of range: a grouped root turning back while its slots still fire, and
    /// the bodyless exits through idle.
    fn leave_attack_range(&mut self, actor_id: u64, update: SkillUpdate) -> Flow {
        let SkillUpdate {
            backswing_just_finished,
            attack_point_rejected,
            burst_releasing,
            ..
        } = update;
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        if actor.rules.attack.weapons.mode == WeaponMode::Group
            && actor.motion.state == MotionState::Attacking
            && actor
                .skill
                .siblings()
                .iter()
                .any(|slot| slot.lock_target.is_some())
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
            && !burst_releasing
            && actor.skill.performer.pending().is_empty()
            && actor.skill.backswing_finish_step().is_none()
            && (actor.rules.attack.melee || actor.skill.phase() == FightSkillPhase::Attack)
        {
            // FightSkill updates before MotionController. An active bodyless
            // attack rejects an out-of-range retained target and enters
            // SkillIdleState before MotionAttackState can fall through to
            // movement. Both state machines expose one targetless Idle tick.
            // A burst still releasing is not checked between its shots, so
            // the unit moves after its target and fires the rest: an
            // Overlord whose Crawler walks out of reach after its third shot
            // follows it and fires the fourth. Nor on the update its last
            // shot leaves: a Phantom Ray whose Rhino walks out of reach
            // moves after it that update and goes idle on the next.
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
        let solve_due = self.rvo_solve_due();
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let entered_move_from_idle = actor.motion.state == MotionState::Idle;
        let entered_move = actor.motion.state != MotionState::Moving;
        let entered_move_below_min_range =
            entered_move && edge_distance_q32 < space_to_q32(actor.rules.attack.min_range());
        // A target the skill's own search answered this tick, because the one
        // it attacked died during it, was not the target its update tracked:
        // the Melting Point whose Crawler an ally kills keeps its turret still
        // on the tick it sets off for the next one.
        let retargeted_this_tick = entered_move && actor.skill.searched_this_tick;
        if !entered_move_from_idle && !entered_move_below_min_range && !retargeted_this_tick {
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
        if actor.command.is_some() {
            // A command's `GetTargetPosition` is its point, not the lock.
            self.move_to_command_point(actor_id);
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
        actor.turn_to_move_direction();
        actor.move_to(move_target_x_q32, move_target_z_q32, solve_due);
    }
}
