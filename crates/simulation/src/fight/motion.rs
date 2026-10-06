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
    /// `TransitionState.nextState`: the state a transition leads to.
    pub(in crate::fight) transition_to: Option<MotionState>,
    pub(in crate::fight) attack_hold_fire: bool,
    /// `MotionController.attacker`: the skill the motion asks. The main skill
    /// is, except while one of the unit's extra skills has taken it
    /// ([`Simulation::hand_motion_after_lock_search`]).
    pub(in crate::fight) attacker: SkillSlot,
    /// `MotionController.pathFindingController`, a
    /// `SimplePathFindingController` for a unit that has one.
    pub(in crate::fight) path_finding: Option<path_finding::PathFinding>,
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
        let suicided = self.suicided_this_tick(actor_id);
        let changed = {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            // A locked agent stands where it is: `DoCalculateNextPosition`
            // moves it nowhere.
            if (!actor.alive() && !suicided) || actor.agent_locked() {
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
            // `Agent.BufferSwitch` zeroes a locked agent's velocity.
            if actor.agent_locked() {
                actor.motion.current_velocity_x_q32 = 0;
                actor.motion.current_velocity_z_q32 = 0;
            }
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
            let mut profile = rvo_profile(&actor.rules);
            // A move ability's `Lock` takes the agent to its own collider
            // priority and full priority until it lets go.
            let agent_override = actor.agent_override();
            if let Some((collider_priority, priority_q32)) = agent_override.locked {
                profile.collider_priority = collider_priority;
                profile.priority_q32 = priority_q32;
            }
            let (layer, collides_with) = movable_rvo_collision_masks(profile.collider_priority);
            let target_delta = FixedVec2 {
                x: actor.motion.next_target_x_q32.saturating_sub(actor.x_q32),
                y: actor.motion.next_target_z_q32.saturating_sub(actor.z_q32),
            };
            // `Agent.BufferSwitch` gives a locked agent no desired velocity.
            let (desired_x, desired_z) = if agent_override.locked.is_some() {
                (0, 0)
            } else {
                normalized_velocity_q32_raw(
                    target_delta.x,
                    target_delta.y,
                    actor.motion.next_speed_q32,
                )
            };
            agents.push(RvoAgentInput {
                key: RvoAgentKey::Unit(actor_id),
                main_layer: agent_override
                    .main_layer
                    .unwrap_or(match actor.rules.domain {
                        UnitDomain::Ground => 1,
                        UnitDomain::Air => 2,
                    }),
                layer,
                collides_with,
                group: i32::try_from(actor.placement.team).unwrap_or(i32::MAX),
                passable_by_own_group: false,
                locked: appearing || agent_override.locked.is_some(),
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
        self.appearing_agents_built();
    }
}

/// Where a unit out of range of what it fires at stands against it, and where
/// its body goes if it moves.
#[derive(Debug, Clone, Copy)]
struct Approach {
    target_rotation_q32: i64,
    body_x_q32: i64,
    body_z_q32: i64,
    body_radius: i64,
}

impl Simulation {
    /// `MotionController`'s update: holding a target that died, attacking one
    /// in range, and leaving one or moving towards it. A grouped or standalone
    /// main skill is still started here (`SkillIdleState.TryStartAttack`);
    /// any other starts in its own update (`try_perform_main`).
    pub(in crate::fight) fn update_motion(
        &mut self,
        actor_id: u64,
        step: u64,
        events: &mut Vec<Event>,
        update: SkillUpdate,
    ) -> Result<()> {
        let before = self.actors[&actor_id].motion.state;
        self.update_motion_states(actor_id, step, events, update)?;
        // A change of state goes through the transition with a move ability
        // (`ChangeToMoveState`, `ChangeToAttackState`,
        // `MotionMoveState.ChangeToIdle`). What the skill did on the way
        // stands: `SkillIdleState.TryStartAttack` starts the attack of a
        // unit coming into range whatever its motion does, and only the
        // surfacing's `SkillManager.Deactive` stops it again.
        let after = self.actors[&actor_id].motion.state;
        if before != after && self.transits(actor_id, before, after) {
            self.begin_transition(actor_id, before, after);
        }
        Ok(())
    }

    /// What the motion goes after. A batch of standalone weapons answers its
    /// first weapon that holds a lock (`FightSkillBatch.GetLockTarget`), but
    /// only while the mech holds a lock, the latest any weapon took or
    /// dropped, or while it attacks with a weapon in its attack.
    fn motion_target(&self, actor_id: u64) -> Option<FightActorRef> {
        let actor = &self.actors[&actor_id];
        let skill = &actor.skills.main;
        // A unit that searches for itself is its motion's attacker
        // (`FightMech.SetMotionAttackerAfterSkill`): the motion goes after
        // the unit's own lock.
        if skill.mech_searches() {
            return skill.unit_lock();
        }
        if skill.standalone() {
            let holder = skill.group.as_ref().map_or(0, |group| group.motion_slot);
            return skill
                .slot_lock(holder)
                .and_then(|_| skill.group_attack_target(holder));
        }
        skill.batch_attack_target()
    }

    /// A batch of standalone weapons already attacking stays in its attack
    /// while any weapon's target is in range
    /// (`FightSkillBatch.IsAttackTargetInAttackRange`); `None` for any other
    /// unit.
    fn batch_in_reach(&self, actor_id: u64) -> Option<bool> {
        let actor = &self.actors[&actor_id];
        let skill = &actor.skills.main;
        (skill.standalone()
            && !skill.mech_searches()
            && actor.motion.state == MotionState::Attacking)
            .then(|| {
                let holder = skill.group.as_ref().map_or(0, |group| group.motion_slot);
                skill.group_attack_target(holder).is_some_and(|aimed| {
                    self.slot_target_in_attack_range(
                        SkillRef::main(FightActorRef::Unit(actor_id)),
                        Some(holder),
                        aimed,
                    )
                })
            })
    }

    /// What the body walks on: the skill's lock, or the unit's own for a
    /// unit that searches for itself.
    fn walked_on(&self, actor_id: u64, target: FightActorRef) -> Option<FightActorRef> {
        if self.actors[&actor_id].skills.main.mech_searches() {
            Some(target)
        } else {
            self.actors[&actor_id].skills.main.lock_target
        }
    }

    /// `MotionAttackState.Update` for a unit that searches for itself, its
    /// own lock out of range: it changes to `MotionMoveState`, and the state
    /// entered is not updated on the tick it is entered, so the body neither
    /// turns nor walks until the next.
    fn mech_leaves_attack(&mut self, actor_id: u64) -> Flow {
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        if actor.skills.main.mech_searches() && actor.motion.state == MotionState::Attacking {
            actor.motion.state = MotionState::Moving;
            return Flow::Done;
        }
        Flow::Next
    }

    /// Whether the motion's target is in reach: a batch's weapon's, a unit's
    /// own lock, a shield's surface, or its target where it stands.
    fn motion_in_reach(
        &self,
        actor_id: u64,
        target: FightActorRef,
        sees_target: bool,
        edge_distance_q32: i64,
    ) -> bool {
        let actor = &self.actors[&actor_id];
        if let Some(batch_in_reach) = self.batch_in_reach(actor_id) {
            batch_in_reach
        } else if actor.skills.main.mech_searches() {
            // `FightMech.IsActorInAttackRange`: the unit's own lock is in
            // reach nearer than its main skill's range less a metre.
            sees_target
                && rvo::fpoint_less_than(
                    edge_distance_q32,
                    space_to_q32(self.main_attack_range(actor_id)).saturating_sub(1_i64 << 32),
                )
        } else if actor.skills.main.shield_target().is_some() {
            // A skill firing at a shield stops its unit once the shield's
            // surface is in reach, however far the lock stands behind it.
            self.target_in_attack_range(SkillRef::main(FightActorRef::Unit(actor_id)), target)
        } else {
            sees_target
                && edge_distance_q32 >= space_to_q32(actor.rules.attack.min_range())
                && edge_distance_q32 <= space_to_q32(self.main_attack_range(actor_id))
        }
    }

    fn update_motion_states(
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
        if let SkillSlot::Extra(index) = self.actors[&actor_id].motion.attacker
            && self.actors[&actor_id].command.is_none()
        {
            self.follow_extra_attacker(actor_id, index);
            return Ok(());
        }
        if let Flow::Done = self.hold_dead_target_moving(actor_id, backswing_just_finished) {
            return Ok(());
        }
        // A batch of standalone weapons answers its first weapon that holds
        // a lock (`FightSkillBatch.GetLockTarget`).
        let target = self.motion_target(actor_id);
        if target.is_none() && self.actors[&actor_id].command.is_some() {
            // A command is active without a target: every motion state
            // goes to or stays in `MotionMoveState`, which walks the path.
            self.follow_command(actor_id);
            return Ok(());
        }
        let Some(target) = target else {
            if let Flow::Next = self.walk_on_idle_lock(actor_id, update) {
                self.enter_motion_idle(actor_id);
            }
            return Ok(());
        };
        let target_view = self.fight_actor(target).expect("target identity is stable");
        let target_x_q32 = target_view.x_q32;
        let target_z_q32 = target_view.z_q32;
        let target_radius = target_view.radius;
        // `IsAttackTargetInAttackRange` asks whether the target is visible.
        let sees_target = self.reaches_hidden(FightActorRef::Unit(actor_id), target_view.visible);
        // Where the body goes when it moves is the lock's, not the weapons':
        // a unit held by a construction in its line of fire still advances on
        // the unit behind it, and only stops because the construction is in
        // reach. The two coincide in every fight without one.
        let (body_x_q32, body_z_q32, body_radius) = self
            .walked_on(actor_id, target)
            .and_then(|lock| self.fight_actor(lock))
            .map_or((target_x_q32, target_z_q32, target_radius), |view| {
                (view.x_q32, view.z_q32, view.radius)
            });
        let actor = &self.actors[&actor_id];
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
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        if actor.rules.attack.weapons.mode == WeaponMode::Group
            && actor.motion.state == MotionState::Attacking
            && actor.skills.main.lock_target.is_none()
            && actor
                .skills
                .main
                .siblings()
                .iter()
                .any(|slot| slot.lock_target.is_some())
        {
            actor.rotate_body_towards(mdeg_to_degrees_q32(actor.placement.rotation));
            actor.aim_rotation = actor.body_rotation;
            return Ok(());
        }
        let in_reach = self.motion_in_reach(actor_id, target, sees_target, edge_distance_q32);
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
                update.blow_released,
            )?;
            self.attack_move(actor_id, was_attacking);
            return Ok(());
        }
        if let Flow::Done = self.mech_leaves_attack(actor_id) {
            return Ok(());
        }
        self.leave_or_approach(
            actor_id,
            Approach {
                target_rotation_q32,
                body_x_q32,
                body_z_q32,
                body_radius,
            },
            update,
        );
        Ok(())
    }

    /// `MotionIdleState`: its `Enter` stops the move, and its `Update` does
    /// not, so an idle unit an RVO solve nudged keeps the target point it
    /// stopped at, and the next solve steers it back there.
    pub(in crate::fight) fn enter_motion_idle(&mut self, actor_id: u64) {
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        if actor.motion.state != MotionState::Idle {
            actor.motion.next_target_x_q32 = actor.x_q32;
            actor.motion.next_target_z_q32 = actor.z_q32;
            actor.motion.next_speed_q32 = 0;
            actor.motion.next_max_speed_q32 = actor.rvo_max_speed_q32;
        }
        actor.motion.state = MotionState::Idle;
    }

    /// `AutoMoveBehaviour` for a skill its search left idle ([`Skill::idle`]):
    /// it is active while the lock lives, and with nothing to fire at the
    /// motion asks only whether the lock is in touch
    /// (`IsLockTargetInTouchRange`): the whole metres of `Distance2D`, edge to
    /// edge, no more than twice the unit's radius. A moving unit walks on the
    /// lock until it is in touch and idles there; an idle one sets off again
    /// once it is not.
    fn walk_on_idle_lock(&mut self, actor_id: u64, update: SkillUpdate) -> Flow {
        let actor = &self.actors[&actor_id];
        if !actor.skills.main.idle {
            return Flow::Next;
        }
        let Some(view) = actor
            .skills
            .main
            .lock_target
            .filter(|&lock| self.fight_actor_is_alive(lock))
            .and_then(|lock| self.fight_actor(lock))
        else {
            return Flow::Next;
        };
        // `MotionAttackState.Update` asks `IsIdle` first and goes idle; the
        // idle state sets off on the update after.
        if actor.motion.state == MotionState::Attacking {
            self.enter_motion_idle(actor_id);
            return Flow::Done;
        }
        let radius_q32 = space_to_q32(actor.rules.collision_radius());
        let edge_distance_q32 = native_q32_magnitude(
            view.x_q32.saturating_sub(actor.x_q32),
            view.z_q32.saturating_sub(actor.z_q32),
        )
        .saturating_sub(radius_q32)
        .saturating_sub(space_to_q32(view.radius))
        .max(0);
        let whole_metres_q32 = edge_distance_q32 & !(Q32_ONE - 1);
        if whole_metres_q32 <= radius_q32.saturating_mul(2) {
            self.enter_motion_idle(actor_id);
            return Flow::Done;
        }
        let target_rotation_q32 = direction_degrees_q32_raw(
            view.x_q32.saturating_sub(actor.x_q32),
            view.z_q32.saturating_sub(actor.z_q32),
        );
        self.leave_or_approach(
            actor_id,
            Approach {
                target_rotation_q32,
                body_x_q32: view.x_q32,
                body_z_q32: view.z_q32,
                body_radius: view.radius,
            },
            update,
        );
        Flow::Done
    }

    /// The end of `FightSkill.SearchLockTarget`, which hands the motion its
    /// attacker (`ISkillOwner.SetAttacker`). A unit that searches for itself
    /// is its own. The main skill, the unit's main searcher, takes the motion
    /// back on each search, since an extra skill is no main target provider
    /// (`IsMainTargetProvider` answers `isMainSkill`). An extra skill takes it
    /// only while the main searcher holds no lock
    /// (`GetMainSearcherSkill().GetLockTarget()`), and hands the unit no lock
    /// with it; a skill of a `SkillGroup` that is no main searcher never
    /// takes it: Energy Diffraction's beams leave a Melting Point idle while
    /// its main beam holds no lock.
    pub(in crate::fight) fn hand_motion_after_lock_search(&mut self, skill_ref: SkillRef) {
        let FightActorRef::Unit(actor_id) = skill_ref.owner else {
            return;
        };
        let Some(actor) = self.actors.get_mut(&actor_id) else {
            return;
        };
        if actor.skills.main.mech_searches() {
            return;
        }
        // An active permanent preemptive skill is the main searcher, and
        // the motion's whoever else searches.
        if actor.skills.preemptive_active {
            if let SkillSlot::Extra(index) = skill_ref.slot
                && actor.skills.extras[index].rules.preemptive.is_some()
            {
                actor.motion.attacker = skill_ref.slot;
            }
            return;
        }
        match skill_ref.slot {
            SkillSlot::Main => {
                actor.motion.attacker = SkillSlot::Main;
                self.hand_standalone_motion(SkillRef::main(FightActorRef::Unit(actor_id)), 0);
            }
            SkillSlot::Extra(index) => {
                if actor.skills.main.slot_lock(0).is_none()
                    && !actor.skills.extras[index].skill.is_grouped()
                {
                    actor.motion.attacker = skill_ref.slot;
                }
            }
        }
    }

    /// The end of `FightSkill.SearchLockTarget` for one of a batch's
    /// standalone weapons, each of them a main searcher: it takes the motion
    /// (`ISkillOwner.SetAttacker`) unless another weapon holds it and holds a
    /// lock (`IAttacker.IsMainTargetProvider`, `GetLockTarget`). A weapon
    /// taking a fresh lock does not draw the turret off the one that has it.
    pub(in crate::fight) fn hand_standalone_motion(&mut self, skill_ref: SkillRef, slot: usize) {
        let (FightActorRef::Unit(actor_id), SkillSlot::Main) = (skill_ref.owner, skill_ref.slot)
        else {
            return;
        };
        let Some(actor) = self.actors.get_mut(&actor_id) else {
            return;
        };
        let skill = &actor.skills.main;
        if !skill.standalone() || skill.mech_searches() {
            return;
        }
        let holder = skill.group.as_ref().map_or(0, |group| group.motion_slot);
        if holder != slot && skill.slot_lock(holder).is_some() {
            return;
        }
        if let Some(group) = actor.skills.main.group.as_mut() {
            group.motion_slot = slot;
        }
    }

    /// `MotionController` while an extra skill is its attacker.
    /// `AutoMoveBehaviour` asks the extra skill: whether it is idle
    /// (`IsIdle`), whether its lock lives (`IsActive`), and whether what it
    /// fires at is in its own range (`IsAttackTargetInAttackRange`). A state
    /// entered is not updated on the update it is entered.
    fn follow_extra_attacker(&mut self, actor_id: u64, index: usize) {
        let skill_ref = SkillRef {
            owner: FightActorRef::Unit(actor_id),
            slot: SkillSlot::Extra(index),
        };
        let skill = self.skill(skill_ref);
        let idle = skill.idle;
        let lock = skill
            .lock_target
            .filter(|&lock| self.fight_actor_is_alive(lock))
            .and_then(|lock| self.fight_actor(lock));
        let in_range = skill
            .attack_target()
            .is_some_and(|target| self.target_in_attack_range(skill_ref, target));
        // `CalculateTargetDirection` turns to what the skill fires at: a block
        // in the way before the lock behind it.
        let aimed = skill
            .attack_target()
            .and_then(|target| self.fight_actor(target));
        let Some(lock) = lock else {
            // `IsActive` fails: the attack and the move states go idle, and
            // the idle state stays.
            self.enter_motion_idle(actor_id);
            return;
        };
        let actor = &self.actors[&actor_id];
        let state = actor.motion.state;
        let (dx, dz) = (
            lock.x_q32.saturating_sub(actor.x_q32),
            lock.z_q32.saturating_sub(actor.z_q32),
        );
        let edge_distance_q32 = native_q32_magnitude(dx, dz)
            .saturating_sub(space_to_q32(actor.rules.collision_radius()))
            .saturating_sub(space_to_q32(lock.radius))
            .max(0);
        // `IsLockTargetInTouchRange`, as [`Self::walk_on_idle_lock`] asks it.
        let in_touch = edge_distance_q32 & !(Q32_ONE - 1)
            <= space_to_q32(actor.rules.collision_radius()).saturating_mul(2);
        let solve_due = self.rvo_solve_due();
        let attack_range = self
            .skill_attacker(skill_ref)
            .expect("skill owner identity is stable")
            .attack_range;
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        match state {
            MotionState::Attacking if idle => self.enter_motion_idle(actor_id),
            MotionState::Attacking if !in_range => actor.motion.state = MotionState::Moving,
            MotionState::Attacking => {
                // `RVOControllerFixed.StopMove`, and `AttackRotate` turning
                // to what the skill fires at (`CalculateTargetDirection`): a
                // unit with a body its body (`FightMech.RotateBodyTo`), one
                // without its root (`ISkillOwner.RotateTo`).
                actor.motion.next_target_x_q32 = actor.x_q32;
                actor.motion.next_target_z_q32 = actor.z_q32;
                actor.motion.next_speed_q32 = 0;
                actor.motion.next_max_speed_q32 = actor.rvo_max_speed_q32;
                let bearing_q32 = aimed.map_or_else(
                    || direction_degrees_q32_raw(dx, dz),
                    |aimed| {
                        direction_degrees_q32_raw(
                            aimed.x_q32.saturating_sub(actor.x_q32),
                            aimed.z_q32.saturating_sub(actor.z_q32),
                        )
                    },
                );
                if actor.rules.has_body {
                    actor.rotate_weapons_towards(bearing_q32);
                    // The body still turns to where the unit moves, as the
                    // main skill's attack turns it: a Centurion walking on
                    // as its missile skill holds the motion.
                    actor.turn_to_move_direction();
                } else {
                    actor.rotate_body_towards(bearing_q32);
                    actor.aim_rotation = actor.body_rotation;
                }
            }
            MotionState::Idle if idle && in_touch => {}
            // `MotionAttackState.Enter` calls `RVOControllerFixed.StopMove`:
            // a unit walking on its lock submits no speed on the update it
            // comes into range, and stands at the next boundary.
            MotionState::Idle | MotionState::Moving if !idle && in_range => {
                actor.motion.state = MotionState::Attacking;
                actor.motion.next_target_x_q32 = actor.x_q32;
                actor.motion.next_target_z_q32 = actor.z_q32;
                actor.motion.next_speed_q32 = 0;
                actor.motion.next_max_speed_q32 = actor.rvo_max_speed_q32;
            }
            MotionState::Idle => actor.motion.state = MotionState::Moving,
            MotionState::Moving if idle && in_touch => self.enter_motion_idle(actor_id),
            MotionState::Moving => {
                // `NormalRotate` and the move towards the lock, to the
                // extra skill's range.
                actor.turn_extra_weapons_to(aimed.map(|aimed| (aimed.x_q32, aimed.z_q32)));
                let (x_q32, z_q32) = native_auto_move_target_point(
                    actor.x_q32,
                    actor.z_q32,
                    actor.rules.collision_radius(),
                    lock.x_q32,
                    lock.z_q32,
                    lock.radius,
                    attack_range,
                );
                actor.turn_to_move_direction();
                self.move_body_to(actor_id, (x_q32, z_q32), false, solve_due);
            }
            MotionState::Stopped | MotionState::Transitioning => {}
        }
    }

    /// `SkillAttackState.TryPerformAttack` of a main skill already attacking,
    /// in the skill's own update: the next blow's interval is drawn, the
    /// blow scheduled, and one due at once released, before the skills that
    /// update after it. It asks what the motion asks before it: the motion's
    /// target, alive and in reach, and the attack angle. A skill entering
    /// its attack, a grouped skill, a batch and a unit that searches for
    /// itself are left to the motion.
    pub(in crate::fight) fn perform_main_blow(
        &mut self,
        actor_id: u64,
        step: u64,
        events: &mut Vec<Event>,
        mut update: SkillUpdate,
    ) -> Result<SkillUpdate> {
        let actor = &self.actors[&actor_id];
        let skill = &actor.skills.main;
        if actor.motion.state != MotionState::Attacking
            || actor.motion.attacker != SkillSlot::Main
            || skill.phase() != FightSkillPhase::Attack
            || skill.is_grouped()
            || skill.standalone()
            || skill.mech_searches()
            || actor.rules.attack.weapons.mode == WeaponMode::Group
        {
            return Ok(update);
        }
        let Some(target) = self.motion_target(actor_id) else {
            return Ok(update);
        };
        let Some(view) = self
            .fight_actor(target)
            .filter(|_| self.fight_actor_is_alive(target))
        else {
            return Ok(update);
        };
        let sees_target = self.reaches_hidden(FightActorRef::Unit(actor_id), view.visible);
        let (dx, dz) = (
            view.x_q32.saturating_sub(actor.x_q32),
            view.z_q32.saturating_sub(actor.z_q32),
        );
        let edge_distance_q32 = native_q32_magnitude(dx, dz)
            .saturating_sub(space_to_q32(actor.rules.collision_radius()))
            .saturating_sub(space_to_q32(view.radius))
            .max(0);
        if !self.motion_in_reach(actor_id, target, sees_target, edge_distance_q32) {
            return Ok(update);
        }
        let in_attack_angle = self
            .attacker(FightActorRef::Unit(actor_id))
            .expect("actor identity is stable")
            .faces(direction_degrees_q32_raw(dx, dz));
        let skill_ref = SkillRef::main(FightActorRef::Unit(actor_id));
        self.try_start_attack(
            skill_ref,
            step,
            target,
            false,
            in_attack_angle,
            update.prepare_finished,
        );
        if self
            .skill(skill_ref)
            .pending()
            .is_some_and(|pending| pending.step == step)
        {
            let _attack_point_rejected = self.release(skill_ref, events)?;
            update.blow_released = true;
        }
        Ok(update)
    }

    /// `MotionAttackState.Update` under a command, whose `IsIdle` and
    /// `IsActive` do not ask whether the lock lives: the motion leaves the
    /// attack only once its target is out of range, so a dead target still in
    /// range keeps the unit attacking, turning to it (`AttackRotate`) and
    /// walking on or stopping as `AttackMove` decides, until its skill takes
    /// another. A skill cooling without a lock goes on naming what its
    /// check found (`IsAttackTargetInAttackRange` asks the skill's attack
    /// target), and the motion attacks it the same way, except that with no
    /// lock `CalculateTargetDirection` answers the velocity: a unit under a
    /// Mobile Beacon whose lock another unit killed holds its attack through
    /// the cooling, turning only to where it walks, and changes to
    /// `MotionMoveState` as the cooling ends and names nothing.
    fn attack_under_command(&mut self, actor_id: u64) -> Flow {
        let actor = &self.actors[&actor_id];
        if actor.command.is_none() || actor.motion.state != MotionState::Attacking {
            return Flow::Next;
        }
        let skill = &actor.skills.main;
        let cooling = skill.cooling();
        let target = match cooling {
            Some((_, named)) => named,
            None => skill
                .attack_target()
                .filter(|&target| !self.fight_actor_is_alive(target)),
        };
        let Some(target) = target else {
            return Flow::Next;
        };
        let Some(target_rotation_q32) = self.command_target_in_reach(actor_id, target) else {
            return Flow::Next;
        };
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
        if cooling.is_none() {
            self.track_target_in_range(actor_id, target_rotation_q32, false);
        } else {
            self.attack_rotate_to_velocity(actor_id);
        }
        self.attack_move(actor_id, true);
        Flow::Done
    }

    /// `IsAttackTargetInAttackRange` as a command's motion asks it of the
    /// skill's attack target, edge to edge against the main skill's range,
    /// whether the target lives or not: the bearing to it when it is in
    /// range.
    fn command_target_in_reach(&self, actor_id: u64, target: FightActorRef) -> Option<i64> {
        let actor = &self.actors[&actor_id];
        let view = self.fight_actor(target)?;
        let edge_distance_q32 = native_q32_magnitude(
            view.x_q32.saturating_sub(actor.x_q32),
            view.z_q32.saturating_sub(actor.z_q32),
        )
        .saturating_sub(space_to_q32(actor.rules.collision_radius()))
        .saturating_sub(space_to_q32(view.radius))
        .max(0);
        let in_reach = edge_distance_q32 >= space_to_q32(actor.rules.attack.min_range())
            && edge_distance_q32 <= space_to_q32(self.main_attack_range(actor_id));
        in_reach.then(|| {
            direction_degrees_q32_raw(
                view.x_q32.saturating_sub(actor.x_q32),
                view.z_q32.saturating_sub(actor.z_q32),
            )
        })
    }

    /// `MotionMoveState.Update` under a command while the skill cools without
    /// a lock, naming what its check found: `IsAttackTargetInAttackRange`
    /// asks that target, and one in range changes the motion to
    /// `MotionAttackState`, which is not updated on the update it is entered.
    /// A Phantom Ray on a Mobile Beacon, cooling on the Tarantula its last
    /// check found, stops walking and turning as the Tarantula comes into
    /// range, and then attacks it as [`Self::attack_under_command`] does.
    pub(in crate::fight) fn move_into_cooled_target(&mut self, actor_id: u64) -> Flow {
        let actor = &self.actors[&actor_id];
        let Some((_, Some(target))) = actor.skills.main.cooling() else {
            return Flow::Next;
        };
        if actor.motion.state != MotionState::Moving
            || self.command_target_in_reach(actor_id, target).is_none()
        {
            return Flow::Next;
        }
        if self.transits(actor_id, MotionState::Moving, MotionState::Attacking) {
            self.begin_transition(actor_id, MotionState::Moving, MotionState::Attacking);
            return Flow::Done;
        }
        self.actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .motion
            .state = MotionState::Attacking;
        Flow::Done
    }

    /// `MotionAttackState.AttackRotate` without a lock: it turns to the
    /// velocity (`CalculateTargetDirection`), a unit with a body its body
    /// and weapons, one without its root, and a unit standing still turns
    /// nothing.
    fn attack_rotate_to_velocity(&mut self, actor_id: u64) {
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let (dx, dz) = (
            actor.motion.current_velocity_x_q32,
            actor.motion.current_velocity_z_q32,
        );
        if dx == 0 && dz == 0 {
            return;
        }
        let bearing_q32 = direction_degrees_q32_raw(dx, dz);
        actor.rotate_body_towards(bearing_q32);
        if actor.rules.has_body {
            actor.rotate_weapons_towards(bearing_q32);
        } else {
            actor.aim_rotation = actor.body_rotation;
        }
    }

    /// `hold_dead_target`, and under a command, which stays active with a
    /// dead target: in range it keeps attacking
    /// ([`Self::attack_under_command`]); out of range, where
    /// `AutoMoveBehaviour` goes idle, the unit changes to `MotionMoveState`,
    /// or walks on in it.
    fn hold_dead_target_moving(&mut self, actor_id: u64, backswing_just_finished: bool) -> Flow {
        if let Flow::Done = self.attack_under_command(actor_id) {
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
    /// A sweep under way keeps naming its dead target until it is over
    /// (`SweepAttackPerformer.IsInterruptedByInvalidTarget`), and the
    /// motion, its lock dead, stays idle.
    fn idle_through_sweep(&mut self, actor_id: u64) -> Flow {
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        if !actor.skills.main.performer.sweeping() {
            return Flow::Next;
        }
        actor.motion.state = MotionState::Idle;
        actor.motion.next_speed_q32 = 0;
        actor.motion.next_max_speed_q32 = actor.rvo_max_speed_q32;
        Flow::Done
    }

    fn hold_dead_target(&mut self, actor_id: u64, backswing_just_finished: bool) -> Flow {
        let lock_target = self.actors[&actor_id].skills.main.attack_target();
        if let Some(target) = lock_target {
            let target_alive = self.fight_actor_is_alive(target);
            // `MotionAttackState.Update` asks the lock, not what the weapons
            // fire at: a block that falls in the way of a live lock leaves the
            // unit attacking, still on the block, until the skill's next
            // check finishes the attack: a Vortex that fells a block with a
            // single blow reads attacking on it that tick, with no backswing
            // to wait out.
            let lock = self.actors[&actor_id].skills.main.lock_target;
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
                        .skills
                        .main
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
                actor.skills.main.drop_lock();
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
                actor.stop_in_place(entered_idle);
                return Flow::Done;
            }
            if !target_alive && let Flow::Done = self.idle_through_sweep(actor_id) {
                return Flow::Done;
            }
            if !target_alive {
                self.actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable")
                    .skills
                    .main
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
                actor.stop_in_place(entered_idle);
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
        let skill = &self.actors[&actor_id].skills.main;
        let FightActorRef::Building(building) = target else {
            return;
        };
        if skill.in_the_way.is_none_or(|(wall, _)| wall != building) {
            return;
        }
        let has_body = self.actors[&actor_id].rules.has_body;
        let facing = skill
            .lock_target
            .filter(|lock| self.fight_actor_is_alive(*lock));
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
                    .skills
                    .main
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
            .skills
            .main
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
    /// The first weapon's skill of a batch attacks its own target, whatever
    /// the unit walks on, once it is in its weapon's attack area.
    pub(in crate::fight) fn start_standalone_core(
        &mut self,
        actor_id: u64,
        step: u64,
        entered_attack: bool,
        prepare_finished: bool,
    ) {
        let owner = FightActorRef::Unit(actor_id);
        let Some(own) = self.actors[&actor_id].skills.main.attack_target() else {
            return;
        };
        if !self.slot_target_in_attack_range(SkillRef::main(owner), Some(0), own) {
            return;
        }
        let actor = &self.actors[&actor_id];
        let in_own_angle = self.fight_actor(own).is_some_and(|view| {
            rotation_distance_q32(
                actor.skills.main.weapon_rotations_q32[0],
                direction_degrees_q32_raw(
                    view.x_q32.saturating_sub(actor.x_q32),
                    view.z_q32.saturating_sub(actor.z_q32),
                ),
            ) <= mdeg_to_degrees_q32(actor.rules.attack.attack_half_angle_mdeg())
        });
        self.try_start_attack(
            SkillRef::main(owner),
            step,
            own,
            entered_attack,
            in_own_angle,
            prepare_finished,
        );
    }

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
        blow_released: bool,
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
                actor.skills.main.drop_lock();
                actor.skills.main.set_phase(FightSkillPhase::Idle);
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
                && actor.skills.main.pending().is_none()
                && actor.skills.main.backswing_finish_step().is_none()
                && actor.skills.main.phase() == FightSkillPhase::Attack;
            if invalid_attack_angle_barrier {
                // MotionAttackState returns to Idle when an active bodyless
                // skill loses its root-transform attack angle. The new
                // Idle state is entered synchronously but is not updated
                // recursively, so target reacquisition waits one tick and
                // this transition tick preserves the old body facing. An idle
                // skill is not active, and `MotionAttackState.AttackRotate`
                // turns the unit to its target: a Crawler whose target a beam
                // turned takes the next and turns to it.
                actor.motion.state = MotionState::Idle;
                actor.skills.main.drop_lock();
                actor.skills.main.set_phase(FightSkillPhase::Idle);
                actor.motion.next_target_x_q32 = actor.x_q32;
                actor.motion.next_target_z_q32 = actor.z_q32;
                actor.motion.next_speed_q32 = 0;
                actor.motion.next_max_speed_q32 = actor.rvo_max_speed_q32;
                return Ok(());
            }
            let clear_hold_after_motion = actor.motion.attack_hold_fire && in_attack_angle;
            if self.actors[&actor_id].skills.main.mech_searches() {
                // Its first weapon's skill starts in its own update.
            } else if self.actors[&actor_id].skills.main.standalone() {
                self.start_standalone_core(actor_id, step, entered_attack, prepare_finished);
            } else {
                self.try_start_attack(
                    SkillRef::main(FightActorRef::Unit(actor_id)),
                    step,
                    target,
                    entered_attack,
                    in_attack_angle,
                    prepare_finished,
                );
            }
            self.start_standalone_slots(actor_id, step);
            (
                entered_attack,
                self.actors[&actor_id]
                    .skills
                    .main
                    .pending()
                    .is_some_and(|pending| pending.step == step),
                clear_hold_after_motion,
            )
        };
        let released = release_now || blow_released;
        if release_now {
            let _attack_point_rejected =
                self.release(SkillRef::main(FightActorRef::Unit(actor_id)), events)?;
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
        let target_rotation_q32 = if released && !self.fight_actor_is_alive(target) {
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
                    .skills
                    .main
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
            self.move_body_to(
                actor_id,
                (move_target_x_q32, move_target_z_q32),
                true,
                solve_due,
            );
        }
    }

    /// The motion states under a command with nothing to fire at:
    /// `MotionIdleState` and `MotionAttackState` change to
    /// `MotionMoveState`, which is not updated the tick it is entered, and
    /// `MotionMoveState` walks towards the command's point.
    pub(in crate::fight) fn follow_command(&mut self, actor_id: u64) {
        let state = self.actors[&actor_id].motion.state;
        if state != MotionState::Moving && self.transits(actor_id, state, MotionState::Moving) {
            self.begin_transition(actor_id, state, MotionState::Moving);
            return;
        }
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
        // `NormalRotate` with no lock turns the weapons to the velocity
        // (`CalculateTargetDirection`), and a unit standing still turns
        // them nothing: a Centurion cooling under a Mobile Beacon keeps its
        // turret where its last shot left it.
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let (vx, vz) = (
            actor.motion.current_velocity_x_q32,
            actor.motion.current_velocity_z_q32,
        );
        if vx != 0 || vz != 0 {
            actor.rotate_weapons_towards(direction_degrees_q32_raw(vx, vz));
        }
        if actor.rules.has_body {
            actor.aim_rotation = degrees_q32_to_mdeg(
                actor
                    .skills
                    .main
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
        self.move_body_to(
            actor_id,
            (move_target_x_q32, move_target_z_q32),
            true,
            solve_due,
        );
    }

    /// `MotionController.Move` handing the agent its point: through the
    /// unit's path finding, which only a move that hands the agent anything
    /// asks.
    fn move_body_to(
        &mut self,
        actor_id: u64,
        point: (i64, i64),
        static_target: bool,
        solve_due: bool,
    ) {
        let (x_q32, z_q32) = if solve_due {
            self.next_move_point(actor_id, point, static_target)
        } else {
            point
        };
        self.actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .move_to(x_q32, z_q32, solve_due);
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
                .skills
                .main
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
            && actor.skills.main.pending().is_none()
            && !burst_releasing
            && actor.skills.main.performer.pending().is_empty()
            && !actor.skills.main.performer.sweeping()
            && actor.skills.main.backswing_finish_step().is_none()
            && actor.skills.main.phase() == FightSkillPhase::Attack
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
            // moves after it that update and goes idle on the next. An idle
            // skill rejects nothing, and `MotionAttackState` changes to
            // `MotionMoveState`: a Crawler whose target a beam turned walks
            // on the next one it finds.
            actor.motion.state = MotionState::Idle;
            actor.skills.main.drop_lock();
            actor.skills.main.set_phase(FightSkillPhase::Idle);
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
            && actor.skills.main.pending().is_none()
            && actor.skills.main.backswing_finish_step().is_none()
        {
            // MotionAttackState leaves through Idle when its current attack
            // target is no longer in range. Idle target acquisition runs on
            // the following update rather than recursively entering Moving.
            actor.motion.state = MotionState::Idle;
            actor.skills.main.drop_lock();
            actor.skills.main.set_phase(FightSkillPhase::Idle);
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
            actor.skills.main.drop_lock();
            actor.skills.main.set_phase(FightSkillPhase::Idle);
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
            target_rotation_q32,
            body_x_q32,
            body_z_q32,
            body_radius,
        } = approach;
        let solve_due = self.rvo_solve_due();
        let attack_range = self.main_attack_range(actor_id);
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let entered_move = actor.motion.state != MotionState::Moving;
        // `MotionMoveState.NormalRotate` turns the weapons to the lock
        // (`CalculateTargetDirection`). A state entered is not updated on
        // the update it is entered, and `FightSkill.Update` turns only a
        // weapon with a transform of its own (`FightWeapon.CanRotate`): a
        // unit setting off turns nothing, whichever state it leaves.
        if !entered_move {
            actor.rotate_weapons_towards(target_rotation_q32);
            if actor.rules.has_body {
                actor.aim_rotation = degrees_q32_to_mdeg(
                    actor
                        .skills
                        .main
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
            attack_range,
        );
        actor.turn_to_move_direction();
        self.move_body_to(
            actor_id,
            (move_target_x_q32, move_target_z_q32),
            false,
            solve_due,
        );
    }
}
