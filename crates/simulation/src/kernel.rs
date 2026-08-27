use std::{cmp::Ordering, collections::BTreeMap};

use mechcore_mcfr::{
    BuildingState, Domain, DurableContext, Event, EventPayload, Gauge, Hashes, IdentityAllocator,
    IdentityContract, MCFR_SCHEMA_VERSION, McfrWriter, MotionState, NumericConvention, ObjectKind,
    ObjectRef, PersonalShieldState, Pose, ProjectileState, Rational, TransitionEvents, UnitState,
    Vec3, Visibility, WorldSnapshot,
};
use serde::Serialize;

use crate::{
    Error, Result,
    layout::{CompiledLayout, Placement},
    random::GrRandom,
    rules::{
        AttackPath, RvoSize, SimulationConfig, TrainingGroundConfig, UnitConfig, UnitConfigs,
        UnitDomain,
    },
    rvo::{AgentInput as RvoAgentInput, AgentKey as RvoAgentKey, AgentSizeType, FixedVec2},
};

const SPACE_UNITS_PER_METER: i64 = 1_000;
const AIR_UNIT_HEIGHT: i64 = 70_000;
const TIME_UNITS_PER_SECOND: u64 = 2_000;
const LOGIC_TICK_TIME_UNITS: u64 = 100;
const FIGHT_TIME_SECONDS: u64 = 120;
const FORMATION_JITTER_RANGE_TENTHS: i32 = 8;
pub(crate) const Q32_ONE: i64 = 1_i64 << 32;
const C0_1_RAW: i64 = 0x1999_9999;
const NATIVE_LOGIC_DELTA_Q32: i64 = 0x0CCC_CCCC;
const TARGET_SCORE_MIN_DISTANCE_Q32: i64 = 3_i64 << 32;
const TARGET_SCORE_ANGLE_LIMIT_Q32: i64 = 100_i64 << 32;
const TARGET_SCORE_ANGLE_FACTOR_Q32: i64 = 0x9999_9999;
const TARGET_SCORE_BASE_Q32: i64 = 100_i64 << 32;
const TARGET_SCORE_OUT_OF_RANGE_PENALTY_Q32: i64 = 200_000_i64 << 32;
const TARGET_QUADTREE_MAX_DEPTH: u8 = 6;
const TARGET_QUADTREE_MAX_ELEMENTS: usize = 20;
const TARGET_QUADTREE_HALF_WIDTH_Q32: i64 = 400 * Q32_ONE;
const TARGET_QUADTREE_HALF_HEIGHT_Q32: i64 = 350 * Q32_ONE;
const RVO_SIMULATOR_ORIGIN_OFFSET_Q32: i64 = 400 * Q32_ONE;
const CORE_TOWER_RVO_COLLIDER_PRIORITY: i32 = 10;
const SEARCH_TARGET_RESET_TICKS: i32 = 10;

#[derive(Debug, Clone, Copy)]
struct RvoProfile {
    outer_radius_q32: i64,
    inner_radius_q32: i64,
    size: AgentSizeType,
    collider_priority: i32,
    priority_q32: i64,
}

fn rvo_profile(rules: &UnitConfig) -> RvoProfile {
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

fn movable_rvo_collision_masks(collider_priority: i32) -> (u32, u32) {
    debug_assert!((1..=16).contains(&collider_priority));
    let layer_index = (collider_priority * 2 - 2) as u32;
    let layer = 1_u32 << layer_index;
    let higher = if layer_index < 30 {
        0x7fff_ffff_u32 & (!0_u32 << (layer_index + 1))
    } else {
        0
    };
    (layer, layer | higher)
}

fn immovable_rvo_collision_masks(collider_priority: i32) -> (u32, u32) {
    debug_assert!((1..=16).contains(&collider_priority));
    (1_u32 << (collider_priority * 2 - 1), 0)
}

fn rvo_position(x_q32: i64, z_q32: i64) -> FixedVec2 {
    FixedVec2 {
        x: x_q32.saturating_add(RVO_SIMULATOR_ORIGIN_OFFSET_Q32),
        y: z_q32.saturating_add(RVO_SIMULATOR_ORIGIN_OFFSET_Q32),
    }
}

#[derive(Debug, Clone, Copy)]
struct PendingRelease {
    step: u64,
    target: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FightSkillPhase {
    Idle,
    Prepare { finish_step: u64 },
    Attack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum NormalTargetCandidate {
    Unit(u64),
    Building(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TargetActorRect {
    min_x: i64,
    min_z: i64,
    max_x: i64,
    max_z: i64,
}

impl TargetActorRect {
    fn around(x_q32: i64, z_q32: i64, radius: i64) -> Self {
        let radius_q32 = space_to_q32(radius.max(0));
        Self {
            min_x: x_q32.saturating_sub(radius_q32),
            min_z: z_q32.saturating_sub(radius_q32),
            max_x: x_q32.saturating_add(radius_q32),
            max_z: z_q32.saturating_add(radius_q32),
        }
    }

    fn contains(self, other: Self) -> bool {
        self.min_x <= other.min_x
            && self.min_z <= other.min_z
            && self.max_x >= other.max_x
            && self.max_z >= other.max_z
    }

    fn children(self) -> [Self; 4] {
        let center_x = self.min_x.saturating_add(self.max_x) / 2;
        let center_z = self.min_z.saturating_add(self.max_z) / 2;
        [
            Self {
                min_x: self.min_x,
                min_z: self.min_z,
                max_x: center_x,
                max_z: center_z,
            },
            Self {
                min_x: center_x,
                min_z: self.min_z,
                max_x: self.max_x,
                max_z: center_z,
            },
            Self {
                min_x: self.min_x,
                min_z: center_z,
                max_x: center_x,
                max_z: self.max_z,
            },
            Self {
                min_x: center_x,
                min_z: center_z,
                max_x: self.max_x,
                max_z: self.max_z,
            },
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TargetActorQuadtreeNode {
    rect: TargetActorRect,
    depth: u8,
    elements: Vec<NormalTargetCandidate>,
    children: Option<Box<[TargetActorQuadtreeNode; 4]>>,
}

impl TargetActorQuadtreeNode {
    fn new(rect: TargetActorRect, depth: u8) -> Self {
        Self {
            rect,
            depth,
            elements: Vec::new(),
            children: None,
        }
    }

    fn child_containing(&self, range: TargetActorRect) -> Option<usize> {
        self.children
            .as_ref()?
            .iter()
            .position(|child| child.rect.contains(range))
    }

    fn insert(
        &mut self,
        candidate: NormalTargetCandidate,
        ranges: &BTreeMap<NormalTargetCandidate, TargetActorRect>,
    ) {
        let range = ranges[&candidate];
        if let Some(child_index) = self.child_containing(range) {
            self.children.as_mut().expect("observed child exists")[child_index]
                .insert(candidate, ranges);
            return;
        }

        if self.children.is_none()
            && self.depth < TARGET_QUADTREE_MAX_DEPTH
            && self.elements.len().saturating_add(1) >= TARGET_QUADTREE_MAX_ELEMENTS
        {
            self.children = Some(Box::new(
                self.rect
                    .children()
                    .map(|rect| Self::new(rect, self.depth.saturating_add(1))),
            ));
            // FightQuadtreeNode.Distribute scans the old list backwards.
            for index in (0..self.elements.len()).rev() {
                let old = self.elements[index];
                if let Some(child_index) = self.child_containing(ranges[&old]) {
                    self.elements.remove(index);
                    self.children.as_mut().expect("split children exist")[child_index]
                        .insert(old, ranges);
                }
            }
            if let Some(child_index) = self.child_containing(range) {
                self.children.as_mut().expect("split children exist")[child_index]
                    .insert(candidate, ranges);
                return;
            }
        }
        self.elements.push(candidate);
    }

    fn find_path(&self, candidate: NormalTargetCandidate, path: &mut Vec<usize>) -> bool {
        if self.elements.contains(&candidate) {
            return true;
        }
        let Some(children) = self.children.as_ref() else {
            return false;
        };
        for (index, child) in children.iter().enumerate() {
            path.push(index);
            if child.find_path(candidate, path) {
                return true;
            }
            path.pop();
        }
        false
    }

    fn node_at_path(&self, path: &[usize]) -> &Self {
        let mut node = self;
        for &index in path {
            node = &node.children.as_ref().expect("quadtree child exists")[index];
        }
        node
    }

    fn node_at_path_mut(&mut self, path: &[usize]) -> &mut Self {
        let mut node = self;
        for &index in path {
            node = &mut node.children.as_mut().expect("quadtree child exists")[index];
        }
        node
    }

    fn append_query_order(&self, output: &mut Vec<NormalTargetCandidate>) {
        output.extend(self.elements.iter().copied());
        if let Some(children) = &self.children {
            for child in children.iter() {
                child.append_query_order(output);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TargetActorQuadtree {
    root: TargetActorQuadtreeNode,
    ranges: BTreeMap<NormalTargetCandidate, TargetActorRect>,
}

impl TargetActorQuadtree {
    fn new() -> Self {
        Self {
            root: TargetActorQuadtreeNode::new(
                TargetActorRect {
                    min_x: -TARGET_QUADTREE_HALF_WIDTH_Q32,
                    min_z: -TARGET_QUADTREE_HALF_HEIGHT_Q32,
                    max_x: TARGET_QUADTREE_HALF_WIDTH_Q32,
                    max_z: TARGET_QUADTREE_HALF_HEIGHT_Q32,
                },
                0,
            ),
            ranges: BTreeMap::new(),
        }
    }

    fn insert(&mut self, candidate: NormalTargetCandidate, x_q32: i64, z_q32: i64, radius: i64) {
        let range = TargetActorRect::around(x_q32, z_q32, radius);
        self.ranges.insert(candidate, range);
        if self.root.rect.contains(range) {
            self.root.insert(candidate, &self.ranges);
        } else {
            self.root.elements.push(candidate);
        }
    }

    fn position_changed(
        &mut self,
        candidate: NormalTargetCandidate,
        x_q32: i64,
        z_q32: i64,
        radius: i64,
    ) {
        if !self.ranges.contains_key(&candidate) {
            self.insert(candidate, x_q32, z_q32, radius);
            return;
        }
        let mut path = Vec::new();
        if !self.root.find_path(candidate, &mut path) {
            self.insert(candidate, x_q32, z_q32, radius);
            return;
        }

        let new_range = TargetActorRect::around(x_q32, z_q32, radius);
        self.ranges.insert(candidate, new_range);
        if self.root.node_at_path(&path).rect.contains(new_range) {
            let Some(child_index) = self.root.node_at_path(&path).child_containing(new_range)
            else {
                return;
            };
            let node = self.root.node_at_path_mut(&path);
            let index = node
                .elements
                .iter()
                .position(|element| *element == candidate)
                .expect("located quadtree element exists");
            node.elements.remove(index);
            node.children.as_mut().expect("located child exists")[child_index]
                .insert(candidate, &self.ranges);
            return;
        }

        // Root-external actors retain their root-list ordinal because the
        // native callback has no parent from which to retry insertion.
        if path.is_empty() {
            return;
        }
        let node = self.root.node_at_path_mut(&path);
        let index = node
            .elements
            .iter()
            .position(|element| *element == candidate)
            .expect("located quadtree element exists");
        node.elements.remove(index);
        while !path.is_empty() && !self.root.node_at_path(&path).rect.contains(new_range) {
            path.pop();
        }
        self.root
            .node_at_path_mut(&path)
            .insert(candidate, &self.ranges);
    }

    fn query_order(&self) -> Vec<NormalTargetCandidate> {
        let mut output = Vec::with_capacity(self.ranges.len());
        self.root.append_query_order(&mut output);
        output
    }
}

#[derive(Debug, Clone)]
struct Actor {
    placement: Placement,
    rules: UnitConfig,
    x: i64,
    z: i64,
    x_q32: i64,
    z_q32: i64,
    target_query_x_q32: i64,
    target_query_z_q32: i64,
    target_query_body_rotation_q32: i64,
    target_query_alive: bool,
    rvo_tree_x_q32: i64,
    rvo_tree_z_q32: i64,
    current_velocity_x_q32: i64,
    current_velocity_z_q32: i64,
    next_target_x_q32: i64,
    next_target_z_q32: i64,
    next_speed_q32: i64,
    next_max_speed_q32: i64,
    solver_target_x_q32: i64,
    solver_target_z_q32: i64,
    solver_speed_q32: i64,
    published_target_x_q32: i64,
    published_target_z_q32: i64,
    published_speed_q32: i64,
    rvo_stopped_snap_since_boundary: bool,
    body_rotation: i64,
    body_rotation_q32: i64,
    aim_rotation: i64,
    weapon_rotations_q32: Vec<i64>,
    life: i64,
    motion: MotionState,
    next_attack_step: u64,
    motion_attack_hold_fire: bool,
    mech_lock_target: Option<u64>,
    terminal_building_target: Option<u64>,
    fight_skill_search_target_time: i32,
    fight_skill_searched_this_tick: bool,
    fight_skill_phase: FightSkillPhase,
    retarget_after_own_direct_kill: bool,
    pending: Option<PendingRelease>,
    backswing_finish_step: Option<u64>,
}

impl Actor {
    #[cfg(test)]
    fn new(placement: Placement, rules: UnitConfig, seed: i32) -> Self {
        let (x_q32, z_q32) = generate_formation_positions(&placement, &rules, seed)
            .expect("embedded test config has a valid formation definition")[0];
        Self::at_generated_position(placement, rules, x_q32, z_q32)
    }

    fn at_generated_position(
        placement: Placement,
        rules: UnitConfig,
        x_q32: i64,
        z_q32: i64,
    ) -> Self {
        let max_life = rules.max_life;
        let x = q32_to_space_rounded(x_q32);
        let z = q32_to_space_rounded(z_q32);
        let max_speed_q32 = space_to_q32(rules.move_speed());
        let weapon_rotations_q32 = vec![
            mdeg_to_degrees_q32(placement.rotation);
            usize::try_from(rules.attack.weapons.count)
                .expect("u32 weapon count fits the supported host")
        ];
        Self {
            x,
            z,
            x_q32,
            z_q32,
            target_query_x_q32: x_q32,
            target_query_z_q32: z_q32,
            target_query_body_rotation_q32: mdeg_to_degrees_q32(placement.rotation),
            target_query_alive: true,
            rvo_tree_x_q32: x_q32,
            rvo_tree_z_q32: z_q32,
            body_rotation: placement.rotation,
            body_rotation_q32: mdeg_to_degrees_q32(placement.rotation),
            aim_rotation: placement.rotation,
            weapon_rotations_q32,
            placement,
            rules,
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
            motion: MotionState::Idle,
            next_attack_step: 0,
            motion_attack_hold_fire: false,
            mech_lock_target: None,
            terminal_building_target: None,
            // FightSkill owns a second SearchTargetController. FightPrepareState
            // replaces this constructor value with the presearch batch ordinal.
            fight_skill_search_target_time: SEARCH_TARGET_RESET_TICKS,
            fight_skill_searched_this_tick: false,
            fight_skill_phase: FightSkillPhase::Idle,
            retarget_after_own_direct_kill: false,
            pending: None,
            backswing_finish_step: None,
        }
    }

    fn alive(&self) -> bool {
        self.life > 0
    }

    fn exit_fight_on_death(&mut self) {
        self.motion = MotionState::Idle;
        self.pending = None;
        self.mech_lock_target = None;
        self.terminal_building_target = None;
        self.fight_skill_search_target_time = SEARCH_TARGET_RESET_TICKS;
        self.fight_skill_phase = FightSkillPhase::Idle;
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

    fn object_ref(&self) -> ObjectRef {
        ObjectRef::new(ObjectKind::Unit, self.placement.unit_id)
    }

    fn set_weapon_rotation(&mut self, rotation_q32: i64) {
        self.weapon_rotations_q32.fill(rotation_q32);
    }

    fn set_body_rotation(&mut self, rotation_q32: i64) {
        self.body_rotation_q32 = rotation_q32.rem_euclid(360_i64 << 32);
        self.body_rotation = degrees_q32_to_mdeg(self.body_rotation_q32);
    }

    fn rotate_body_towards(&mut self, target_q32: i64) {
        let maximum = q32_mul(
            mdeg_to_degrees_q32(self.rules.rotate_speed_mdeg_per_second()),
            NATIVE_LOGIC_DELTA_Q32,
        );
        let rate_limited = rotation_distance_q32(self.body_rotation_q32, target_q32) > maximum;
        let unwrapped = rotate_towards_q32_unwrapped(
            self.body_rotation_q32,
            target_q32,
            maximum,
        );
        self.set_body_rotation(unwrapped);
        if rate_limited
            && unwrapped < 360_i64 << 32
            && degrees_q32_to_unwrapped_mdeg(unwrapped) == 360_000
        {
            self.body_rotation = 360_000;
        }
    }

    fn rotate_weapons_towards(&mut self, target_q32: i64) {
        let maximum = q32_mul(
            mdeg_to_degrees_q32(self.rules.rotate_speed_mdeg_per_second()),
            NATIVE_LOGIC_DELTA_Q32,
        );
        for rotation in &mut self.weapon_rotations_q32 {
            *rotation = rotate_towards_q32(*rotation, target_q32, maximum);
        }
    }

    fn weapons_in_attack_angle(&self, target_q32: i64) -> bool {
        let half_angle_q32 = mdeg_to_degrees_q32(self.rules.attack.attack_half_angle_mdeg());
        !self.weapon_rotations_q32.is_empty()
            && self
                .weapon_rotations_q32
                .iter()
                .all(|rotation| rotation_distance_q32(*rotation, target_q32) <= half_angle_q32)
    }

    fn snapshot(&self) -> UnitState {
        let height = unit_height(self.rules.domain);
        UnitState {
            unit_id: self.placement.unit_id,
            team_id: self.placement.team,
            formation_id: self.placement.formation_id,
            unit_type_id: self.rules.unit_type_id,
            domain: match self.rules.domain {
                UnitDomain::Ground => Domain::Ground,
                UnitDomain::Air => Domain::Air,
            },
            position: point_at_height(self.x, height, self.z),
            body_rotation: self.body_rotation,
            aim_pose: Pose {
                position: point_at_height(self.x, height, self.z),
                rotation: self.aim_rotation,
            },
            velocity: point(
                q32_to_space_rounded(self.current_velocity_x_q32),
                q32_to_space_rounded(self.current_velocity_z_q32),
            ),
            motion_state: self.motion,
            mech_lock_target: self
                .terminal_building_target
                .map(|target| ObjectRef::new(ObjectKind::Building, target))
                .or_else(|| {
                    self.mech_lock_target
                        .map(|target| ObjectRef::new(ObjectKind::Unit, target))
                }),
            collision_radius: self.rules.collision_radius(),
            life: self.life,
            max_life: self.rules.max_life,
            alive: self.alive(),
            active: true,
            targetable: self.alive(),
            visibility: Visibility::Normal,
            personal_shield: PersonalShieldState {
                active: false,
                enabled: true,
                energy: 0,
                max_energy: 0,
            },
        }
    }
}

fn generate_formation_positions(
    placement: &Placement,
    rules: &UnitConfig,
    seed: i32,
) -> Result<Vec<(i64, i64)>> {
    let members = i64::from(rules.formation.members);
    let (width, depth) = rules.formation_footprint_meters()?;
    let slot_size = rules.formation_slot_size_meters()?;
    let max_columns = width / slot_size;
    if max_columns <= 0 {
        return Err(Error::new(format!(
            "unit {:?} formation slot size exceeds its footprint width",
            rules.type_name
        )));
    }
    let rows = members.saturating_add(max_columns - 1) / max_columns;
    if rows <= 0 {
        return Err(Error::new(format!(
            "unit {:?} formation has no member rows",
            rules.type_name
        )));
    }
    let row_step = depth / rows;
    let column_step = width / max_columns;
    if row_step <= 0 || column_step <= 0 {
        return Err(Error::new(format!(
            "unit {:?} formation grid has a nonpositive native step",
            rules.type_name
        )));
    }

    let formation_seed = seed.wrapping_add(placement.formation_index);
    let mut layout_random = GrRandom::new(i64::from(formation_seed).cast_unsigned());
    let direction = if placement.team == 0 { 1_i64 } else { -1_i64 };
    let center_x_q32 = placement.world_x.saturating_mul(Q32_ONE);
    let center_z_q32 = placement.world_z.saturating_mul(Q32_ONE);
    let mut positions = Vec::with_capacity(usize::try_from(members).unwrap_or(usize::MAX));
    for row in 0..rows {
        let remaining = members.saturating_sub(i64::try_from(positions.len()).unwrap_or(i64::MAX));
        let columns = max_columns.min(remaining);
        let local_z_q32 = depth.saturating_mul(Q32_ONE) / 2
            - (row.saturating_mul(row_step).saturating_mul(Q32_ONE)
                + row_step.saturating_mul(Q32_ONE) / 2);
        let occupied_width = columns.saturating_mul(column_step);
        for column in 0..columns {
            let local_x_q32 = column.saturating_mul(column_step).saturating_mul(Q32_ONE)
                + column_step.saturating_mul(Q32_ONE) / 2
                - occupied_width.saturating_mul(Q32_ONE) / 2;
            let jitter_x = i64::from(layout_random.next_in_range(FORMATION_JITTER_RANGE_TENTHS))
                .saturating_mul(C0_1_RAW);
            let jitter_z = i64::from(layout_random.next_in_range(FORMATION_JITTER_RANGE_TENTHS))
                .saturating_mul(C0_1_RAW);
            positions.push((
                center_x_q32.saturating_add(
                    local_x_q32
                        .saturating_add(jitter_x)
                        .saturating_mul(direction),
                ),
                center_z_q32.saturating_add(
                    local_z_q32
                        .saturating_add(jitter_z)
                        .saturating_mul(direction),
                ),
            ));
        }
    }
    if positions.len() != usize::try_from(rules.formation.members).unwrap_or(usize::MAX) {
        return Err(Error::new(format!(
            "unit {:?} formation generation produced the wrong member count",
            rules.type_name
        )));
    }
    Ok(positions)
}

fn initialize_actors(
    layout: &CompiledLayout,
    configs: &UnitConfigs,
    seed: i32,
) -> Result<BTreeMap<u64, Actor>> {
    let mut initial = Vec::new();
    for placement in &layout.placements {
        let rules = configs.get(placement.type_name.as_str()).ok_or_else(|| {
            Error::new(format!(
                "unit type {:?} has no configuration",
                placement.type_name
            ))
        })?;
        for (x_q32, z_q32) in generate_formation_positions(placement, rules, seed)? {
            initial.push(Actor::at_generated_position(
                placement.clone(),
                rules.clone(),
                x_q32,
                z_q32,
            ));
        }
    }
    initial.sort_by_key(|actor| (actor.placement.team, actor.z, actor.x));
    for pair in initial.windows(2) {
        if pair[0].placement.team == pair[1].placement.team
            && pair[0].x == pair[1].x
            && pair[0].z == pair[1].z
        {
            return Err(Error::new(
                "two initial same-team units have equal world coordinates",
            ));
        }
    }

    let mut identities = IdentityAllocator::new();
    let mut formation_ids = BTreeMap::new();
    let mut actors = BTreeMap::new();
    for mut actor in initial {
        let unit_id = identities.allocate_object(ObjectKind::Unit)?.id;
        let formation_key = (actor.placement.team, actor.placement.formation_index);
        let formation_id = match formation_ids.get(&formation_key) {
            Some(id) => *id,
            None => {
                let id = identities.allocate_formation()?;
                formation_ids.insert(formation_key, id);
                id
            }
        };
        actor.placement.unit_id = unit_id;
        actor.placement.formation_id = formation_id;
        actors.insert(unit_id, actor);
    }
    Ok(actors)
}

fn initialize_buildings(training_ground: &TrainingGroundConfig) -> Result<Vec<BuildingState>> {
    training_ground
        .buildings
        .iter()
        .enumerate()
        .map(|(index, building)| {
            let building_id = u64::try_from(index)
                .map_err(|_| Error::new("training-ground building index overflow"))?
                .saturating_add(1);
            let radius = building.radius();
            Ok(BuildingState {
                building_id,
                team_id: building.team_id,
                building_type_id: building.building_type_id,
                position: point(building.x(), building.z()),
                rotation: 0,
                bounds_width: radius.saturating_mul(2),
                bounds_height: radius.saturating_mul(2),
                life: building.life,
                max_life: building.life,
                alive: building.life > 0,
                destroyed: false,
                available: true,
                targetable: building.life > 0,
                collision_enabled: building.collision_enabled,
            })
        })
        .collect()
}

fn initialize_target_quadtrees(
    actors: &BTreeMap<u64, Actor>,
    buildings: &[BuildingState],
) -> BTreeMap<u32, TargetActorQuadtree> {
    let teams = actors
        .values()
        .map(|actor| actor.placement.team)
        .chain(buildings.iter().map(|building| building.team_id))
        .collect::<std::collections::BTreeSet<_>>();
    let mut trees = BTreeMap::new();
    for team in teams {
        let mut tree = TargetActorQuadtree::new();

        let mut team_buildings = buildings
            .iter()
            .filter(|building| building.team_id == team)
            .collect::<Vec<_>>();
        team_buildings.sort_by_key(|building| building.building_id);
        for building in team_buildings {
            tree.insert(
                NormalTargetCandidate::Building(building.building_id),
                space_to_q32(building.position.x),
                space_to_q32(building.position.z),
                building.bounds_width / 2,
            );
        }

        for (&actor_id, actor) in actors
            .iter()
            .filter(|(_, actor)| actor.placement.team == team)
        {
            tree.insert(
                NormalTargetCandidate::Unit(actor_id),
                actor.x_q32,
                actor.z_q32,
                actor.rules.collision_radius(),
            );
        }
        trees.insert(team, tree);
    }
    trees
}

#[derive(Debug, Clone)]
struct Projectile {
    id: u64,
    team: u32,
    owner: u64,
    target: u64,
    x: i64,
    y: i64,
    z: i64,
    x_q32: i64,
    z_q32: i64,
    cached_target_x: i64,
    cached_target_y: i64,
    cached_target_z: i64,
    cached_target_x_q32: i64,
    cached_target_z_q32: i64,
    cached_target_radius: i64,
    speed: i64,
    damage: i64,
}

impl Projectile {
    fn object_ref(&self) -> ObjectRef {
        ObjectRef::new(ObjectKind::Projectile, self.id)
    }

    fn snapshot(&self) -> ProjectileState {
        ProjectileState {
            projectile_id: self.id,
            team_id: self.team,
            owner: Some(ObjectRef::new(ObjectKind::Unit, self.owner)),
            position: point_at_height(self.x, self.y, self.z),
            orientation: 0,
            target: Some(ObjectRef::new(ObjectKind::Unit, self.target)),
            cached_target_position: point_at_height(
                self.cached_target_x,
                self.cached_target_y,
                self.cached_target_z,
            ),
            cached_target_radius: self.cached_target_radius,
            released: false,
            life: Gauge {
                current: 1,
                maximum: 1,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TeamResult {
    pub team: &'static str,
    pub unit: String,
    pub alive: bool,
    pub remaining_life: i64,
    pub max_life: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SimulationResult {
    pub schema: &'static str,
    pub game_build: String,
    pub seed: i32,
    pub seed_source: &'static str,
    pub output: String,
    pub end_reason: &'static str,
    pub steps: u64,
    pub elapsed_milliseconds: u64,
    pub winner: Option<&'static str>,
    pub draw: bool,
    pub teams: Vec<TeamResult>,
    pub hashes: Hashes,
}

struct Simulation {
    actors: BTreeMap<u64, Actor>,
    team_random: BTreeMap<u32, GrRandom>,
    projectiles: Vec<Projectile>,
    buildings: Vec<BuildingState>,
    target_quadtrees: BTreeMap<u32, TargetActorQuadtree>,
    identities: IdentityAllocator,
    rvo_counter: u8,
    rvo_solver_started: bool,
    terminal_drain_pending: bool,
}

impl Simulation {
    fn new(
        layout: &CompiledLayout,
        configs: &UnitConfigs,
        training_ground: &TrainingGroundConfig,
        seed: i32,
    ) -> Result<Self> {
        for placement in &layout.placements {
            configs
                .get(&placement.type_name)
                .ok_or_else(|| {
                    Error::new(format!(
                        "unit type {:?} has no configuration",
                        placement.type_name
                    ))
                })?
                .ensure_current_kernel_support()?;
        }
        let actors = initialize_actors(layout, configs, seed)?;
        let mut team_random = BTreeMap::new();
        for actor in actors.values() {
            let random = team_random.entry(actor.placement.team).or_insert_with(|| {
                GrRandom::new(u64::from(
                    layout
                        .round
                        .cast_signed()
                        .wrapping_add(actor.placement.team.cast_signed())
                        .wrapping_mul(4_444)
                        .cast_unsigned(),
                ))
            });
            let offset_steps =
                native_time_units_to_steps(actor.rules.attack.interval_offset_time_units());
            if offset_steps > 0 {
                let _initial_sample =
                    random.next_in_range(i32::try_from(offset_steps).unwrap_or(i32::MAX));
            }
        }
        let buildings = initialize_buildings(training_ground)?;
        let target_quadtrees = initialize_target_quadtrees(&actors, &buildings);
        let mut simulation = Self {
            actors,
            team_random,
            projectiles: Vec::new(),
            buildings,
            target_quadtrees,
            identities: IdentityAllocator::new(),
            rvo_counter: 0,
            rvo_solver_started: false,
            terminal_drain_pending: false,
        };
        simulation.initialize_presearch_targets()?;
        Ok(simulation)
    }

    fn initialize_presearch_targets(&mut self) -> Result<()> {
        let actor_ids = self.actors.keys().copied().collect::<Vec<_>>();
        // Build 2259 PresearchTargetController::CalculateCountPerTime returns
        // ceil(mech_count / 10). SearchTarget assigns the zero-based batch
        // ordinal to the main FightSkill search controller before selecting
        // its initial target.
        let count_per_time = actor_ids.len().div_ceil(10).max(1);
        let selections = actor_ids
            .iter()
            .map(|&actor_id| Ok((actor_id, self.select_normal_unit_target(actor_id)?)))
            .collect::<Result<Vec<_>>>()?;
        for (ordinal, (actor_id, target_id)) in selections.into_iter().enumerate() {
            self.actors
                .get_mut(&actor_id)
                .expect("initial actor identity is stable")
                .fight_skill_search_target_time = i32::try_from(ordinal / count_per_time)
                .expect("presearch batch ordinal is at most nine");
            let Some(target_id) = target_id else {
                continue;
            };
            let target = &self.actors[&target_id];
            let target_rotation_q32 = direction_degrees_q32_raw(
                target.x_q32.saturating_sub(self.actors[&actor_id].x_q32),
                target.z_q32.saturating_sub(self.actors[&actor_id].z_q32),
            );
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("initial actor identity is stable");
            actor.mech_lock_target = Some(target_id);
            actor.set_body_rotation(target_rotation_q32);
            actor.aim_rotation = actor.body_rotation;
            actor.set_weapon_rotation(target_rotation_q32);
        }
        Ok(())
    }

    fn snapshot(&self) -> WorldSnapshot {
        WorldSnapshot {
            units: self.actors.values().map(Actor::snapshot).collect(),
            projectiles: self.projectiles.iter().map(Projectile::snapshot).collect(),
            buildings: self.buildings.clone(),
            ..WorldSnapshot::default()
        }
    }

    fn refresh_target_query_snapshot(&mut self) {
        // Build 2259 prepares selector inputs before FightCore updates actors
        // sequentially. Red actors must therefore score the tick-start pose,
        // not positions already advanced by blue actors in the same tick.
        for actor in self.actors.values_mut() {
            actor.target_query_x_q32 = actor.x_q32;
            actor.target_query_z_q32 = actor.z_q32;
            actor.target_query_body_rotation_q32 = actor.body_rotation_q32;
            actor.target_query_alive = actor.alive();
            actor.fight_skill_searched_this_tick = false;
        }
    }

    fn step(&mut self, step: u64) -> Result<TransitionEvents> {
        if self.terminal_drain_pending {
            self.terminal_drain_pending = false;
        }
        let fight_was_finished = self.naturally_finished();
        let winner_was_decided = self.winner().is_some();
        let actor_motion_at_start = self
            .actors
            .iter()
            .map(|(&actor_id, actor)| (actor_id, actor.motion))
            .collect::<BTreeMap<_, _>>();
        self.refresh_target_query_snapshot();
        let mut events = Vec::new();
        // Native search jobs retain the actor-quadtree candidate order
        // prepared at the start of this FightCore update.
        let target_search_order = self.target_search_order();
        let team_ids = self
            .actors
            .values()
            .map(|actor| actor.placement.team)
            .collect::<std::collections::BTreeSet<_>>();
        let mut team_alive_counts = BTreeMap::new();
        for team_id in team_ids {
            team_alive_counts.insert(
                team_id,
                self.actors
                    .values()
                    .filter(|actor| actor.placement.team == team_id && actor.alive())
                    .count(),
            );
            let actor_ids = self
                .actors
                .iter()
                .filter_map(|(&actor_id, actor)| {
                    (actor.placement.team == team_id).then_some(actor_id)
                })
                .collect::<Vec<_>>();
            for actor_id in actor_ids {
                self.step_actor_with_target_order(
                    actor_id,
                    step,
                    &target_search_order,
                    &mut events,
                )?;
                self.step_actor_rvo_position(actor_id);
            }
        }
        self.step_projectiles(&mut events)?;
        if !fight_was_finished
            && self.naturally_finished()
            && let Some(winning_team) = self.winner()
            && let Some(losing_team) = self
                .buildings
                .iter()
                .find(|building| building.team_id != winning_team && building.alive)
                .map(|building| building.team_id)
        {
            let actor_ids = self
                .actors
                .iter()
                .filter_map(|(&actor_id, actor)| actor.alive().then_some(actor_id))
                .collect::<Vec<_>>();
            for actor_id in actor_ids {
                let Some(&motion_at_start) = actor_motion_at_start.get(&actor_id)
                else {
                    continue;
                };
                let actor = &self.actors[&actor_id];
                if actor.mech_lock_target.is_some()
                    || actor.retarget_after_own_direct_kill
                    || !actor.fight_skill_searched_this_tick
                {
                    continue;
                }
                let Some(building_id) =
                    self.select_normal_building_target(actor_id, losing_team)
                else {
                    continue;
                };
                let building = self
                    .buildings
                    .iter()
                    .find(|building| building.building_id == building_id)
                    .expect("selected building exists");
                let target_x_q32 = space_to_q32(building.position.x);
                let target_z_q32 = space_to_q32(building.position.z);
                let actor = self
                    .actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable");
                if motion_at_start == MotionState::Moving
                    && (actor.current_velocity_x_q32 != 0 || actor.current_velocity_z_q32 != 0)
                {
                    actor.rotate_body_towards(direction_degrees_q32_raw(
                        actor.current_velocity_x_q32,
                        actor.current_velocity_z_q32,
                    ));
                    actor.aim_rotation = actor.body_rotation;
                }
                actor.mech_lock_target = None;
                actor.terminal_building_target = Some(building_id);
                actor.motion = MotionState::Moving;
                actor.next_target_x_q32 = target_x_q32;
                actor.next_target_z_q32 = target_z_q32;
                actor.next_speed_q32 = space_to_q32(actor.rules.move_speed());
                actor.next_max_speed_q32 = actor.next_speed_q32;
            }
        }
        // Native build 2259 tears down the defeated core buildings through the
        // direct-attack finish path. Projectile drain reaches the same round
        // result without mutating buildings (observed in Fang mirror battles).
        let direct_attack_winner = self.winner().is_some_and(|winning_team| {
            self.actors.values().any(|actor| {
                actor.placement.team == winning_team
                    && actor.alive()
                    && matches!(actor.rules.attack.path, AttackPath::Direct { .. })
            })
        });
        let mut queued_late_building_death = false;
        for building in self.buildings.iter_mut().filter(|building| {
            direct_attack_winner
                && building.alive
                && team_alive_counts
                    .get(&building.team_id)
                    .is_some_and(|alive_count| *alive_count == 0)
        }) {
            building.life = 0;
            building.alive = false;
            building.targetable = false;
            queued_late_building_death = true;
        }
        if queued_late_building_death {
            self.terminal_drain_pending = true;
        }
        let ready_to_finish = self.ready_to_finish();
        let stop_fight = ready_to_finish || winner_was_decided;
        if stop_fight {
            for actor in self.actors.values_mut() {
                actor.motion = MotionState::Idle;
                actor.mech_lock_target = None;
                actor.terminal_building_target = None;
                actor.fight_skill_phase = FightSkillPhase::Idle;
                if ready_to_finish {
                    actor.current_velocity_x_q32 = 0;
                    actor.current_velocity_z_q32 = 0;
                }
            }
        }
        if !ready_to_finish {
            self.step_rvo()?;
        }
        Ok(TransitionEvents { events })
    }

    fn target_search_order(&self) -> BTreeMap<u32, Vec<NormalTargetCandidate>> {
        self.target_quadtrees
            .iter()
            .map(|(&team, tree)| (team, tree.query_order()))
            .collect()
    }

    fn select_normal_building_target(&self, actor_id: u64, team_id: u32) -> Option<u64> {
        let source = self.actors.get(&actor_id)?;
        if !source.rules.attack.targets.ground {
            return None;
        }
        let mut best: Option<(u64, i64)> = None;
        for building in self.buildings.iter().filter(|building| {
            building.team_id == team_id && building.alive && building.targetable
        }) {
            let Some(score) = normal_visible_full_rotation_target_score_q32(
                source.target_query_x_q32,
                source.target_query_z_q32,
                source.rules.collision_radius(),
                source.target_query_body_rotation_q32,
                space_to_q32(building.position.x),
                space_to_q32(building.position.z),
                building.bounds_width / 2,
                source.rules.attack.min_range(),
                source.rules.attack.range(),
            ) else {
                continue;
            };
            if best.is_none_or(|(_, best_score)| score < best_score) {
                best = Some((building.building_id, score));
            }
        }
        best.map(|(building_id, _)| building_id)
    }

    fn select_normal_unit_target(&self, actor_id: u64) -> Result<Option<u64>> {
        let target_search_order = self.target_search_order();
        self.select_normal_unit_target_with_order(actor_id, &target_search_order, false)
    }

    fn select_normal_unit_target_with_order(
        &self,
        actor_id: u64,
        target_search_order: &BTreeMap<u32, Vec<NormalTargetCandidate>>,
        use_live_candidate_positions: bool,
    ) -> Result<Option<u64>> {
        let source = self
            .actors
            .get(&actor_id)
            .ok_or_else(|| Error::new("target selector source actor is absent"))?;
        let alive_enemy_count = self
            .actors
            .values()
            .filter(|candidate| {
                candidate.placement.team != source.placement.team && candidate.alive()
            })
            .count();
        if alive_enemy_count == 0 {
            return Ok(None);
        }
        let mut best: Option<(NormalTargetCandidate, i64)> = None;
        let mut consider = |candidate, score| match best {
            None => {
                best = Some((candidate, score));
            }
            Some((_, best_score)) if score < best_score => {
                best = Some((candidate, score));
            }
            Some(_) => {}
        };

        for (&team, candidates) in target_search_order {
            if team == source.placement.team {
                continue;
            }
            for &candidate in candidates {
                match candidate {
                    NormalTargetCandidate::Unit(candidate_id) => {
                        let Some(candidate_actor) = self.actors.get(&candidate_id) else {
                            continue;
                        };
                        let candidate_alive = if use_live_candidate_positions {
                            candidate_actor.alive()
                        } else {
                            candidate_actor.target_query_alive
                        };
                        if !candidate_alive
                            || !source.rules.attack.accepts(candidate_actor.rules.domain)
                        {
                            continue;
                        }
                        let (candidate_x_q32, candidate_z_q32) =
                            if use_live_candidate_positions {
                                (candidate_actor.x_q32, candidate_actor.z_q32)
                            } else {
                                (
                                    candidate_actor.target_query_x_q32,
                                    candidate_actor.target_query_z_q32,
                                )
                            };
                        if let Some(score) = normal_visible_full_rotation_target_score_q32(
                            source.target_query_x_q32,
                            source.target_query_z_q32,
                            source.rules.collision_radius(),
                            source.target_query_body_rotation_q32,
                            candidate_x_q32,
                            candidate_z_q32,
                            candidate_actor.rules.collision_radius(),
                            source.rules.attack.min_range(),
                            source.rules.attack.range(),
                        ) {
                            consider(candidate, score);
                        }
                    }
                    NormalTargetCandidate::Building(building_id)
                        if source.rules.attack.targets.ground =>
                    {
                        let Some(building) = self.buildings.iter().find(|building| {
                            building.building_id == building_id
                                && building.team_id == team
                                && building.alive
                                && building.targetable
                        }) else {
                            continue;
                        };
                        if let Some(score) = normal_visible_full_rotation_target_score_q32(
                            source.target_query_x_q32,
                            source.target_query_z_q32,
                            source.rules.collision_radius(),
                            source.target_query_body_rotation_q32,
                            space_to_q32(building.position.x),
                            space_to_q32(building.position.z),
                            building.bounds_width / 2,
                            source.rules.attack.min_range(),
                            source.rules.attack.range(),
                        ) {
                            consider(candidate, score);
                        }
                    }
                    NormalTargetCandidate::Building(_) => {}
                }
            }
        }

        match best.map(|(candidate, _)| candidate) {
            Some(NormalTargetCandidate::Unit(unit_id)) => Ok(Some(unit_id)),
            Some(NormalTargetCandidate::Building(building_id)) => Err(Error::new(format!(
                "Normal selector chose building {building_id}, but building attacks are not closed"
            ))),
            None => Ok(None),
        }
    }

    fn update_fight_skill_target_search(
        &mut self,
        actor_id: u64,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<NormalTargetCandidate>>,
    ) -> Result<()> {
        // Target-build MechData disables MechSearchTargetController for every
        // supported non-supergiant unit, so live periodic selection belongs to
        // the main FightSkill. Prepare and Attack retain this private counter;
        // Attack only enters the selector when its private attack target is no
        // longer alive.
        let actor = &self.actors[&actor_id];
        let target = self.actors[&actor_id]
            .mech_lock_target
            .and_then(|target_id| self.actors.get(&target_id));
        let target_alive = target.is_some_and(Actor::alive);
        let target_died_during_tick =
            target.is_some_and(|target| target.target_query_alive && !target.alive());
        if !target_alive
            && actor
                .backswing_finish_step
                .is_some_and(|finish_step| finish_step >= step)
        {
            let quick_switch_interval_due = actor.rules.attack.quick_switch_target
                && step > actor.next_attack_step;
            if !quick_switch_interval_due {
                return Ok(());
            }
            self.actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .backswing_finish_step = None;
        }
        let actor = &self.actors[&actor_id];
        if (matches!(actor.fight_skill_phase, FightSkillPhase::Prepare { .. })
            || actor.fight_skill_phase == FightSkillPhase::Attack)
            && (!actor.rules.attack.quick_switch_target || target_alive)
        {
            return Ok(());
        }
        if target_alive && self.actors[&actor_id].fight_skill_search_target_time > 0 {
            self.actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .fight_skill_search_target_time -= 1;
            return Ok(());
        }

        self.actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .fight_skill_searched_this_tick = true;

        let mut selected = self
            .select_normal_unit_target_with_order(
                actor_id,
                target_search_order,
                target_died_during_tick,
            )
            .map_err(|error| Error::new(format!("logic step {step}: {error}")))?;
        if !target_died_during_tick
            && selected
                .and_then(|target_id| self.actors.get(&target_id))
                .is_some_and(|target| target.target_query_alive && !target.alive())
        {
            selected = self
                .select_normal_unit_target_with_order(actor_id, target_search_order, true)
                .map_err(|error| Error::new(format!("logic step {step}: {error}")))?;
        }
        let actor = &self.actors[&actor_id];
        let selected = if target_alive
            && !actor.rules.attack.quick_switch_target
            && actor.motion == MotionState::Attacking
            && !actor.motion_attack_hold_fire
            && actor.pending.is_none()
            && actor.backswing_finish_step.is_none()
            && selected != actor.mech_lock_target
        {
            actor.mech_lock_target
        } else {
            selected
        };
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        actor.mech_lock_target = selected;
        actor.fight_skill_search_target_time = SEARCH_TARGET_RESET_TICKS;
        actor.retarget_after_own_direct_kill = false;
        Ok(())
    }

    #[cfg(test)]
    fn step_actor(&mut self, actor_id: u64, step: u64, events: &mut Vec<Event>) -> Result<()> {
        self.refresh_target_query_snapshot();
        let target_search_order = self.target_search_order();
        self.step_actor_with_target_order(actor_id, step, &target_search_order, events)
    }

    #[allow(clippy::too_many_lines)]
    fn step_actor_with_target_order(
        &mut self,
        actor_id: u64,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<NormalTargetCandidate>>,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let backswing_just_finished = self.actors[&actor_id]
            .backswing_finish_step
            .is_some_and(|finish_step| finish_step < step);
        if !self.actors[&actor_id].alive() {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.exit_fight_on_death();
            return Ok(());
        }
        if self.actors[&actor_id].terminal_building_target.is_some() {
            // The native terminal handoff exposes the defeated team's first
            // core building for one tick. The following update consumes the
            // already published displacement, then the tower teardown clears
            // the transient lock before the terminal snapshot is written.
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.motion = MotionState::Idle;
            actor.terminal_building_target = None;
            actor.fight_skill_phase = FightSkillPhase::Idle;
            actor.current_velocity_x_q32 = 0;
            actor.current_velocity_z_q32 = 0;
            actor.next_target_x_q32 = actor.x_q32;
            actor.next_target_z_q32 = actor.z_q32;
            actor.next_speed_q32 = 0;
            actor.next_max_speed_q32 = space_to_q32(actor.rules.move_speed());
            return Ok(());
        }
        let stale_bodyless_attack_target = {
            let actor = &self.actors[&actor_id];
            if actor.motion == MotionState::Attacking
                && !actor.rules.has_body
                && !actor.motion_attack_hold_fire
                && actor.pending.is_none()
                && actor.backswing_finish_step.is_none()
                && actor
                    .mech_lock_target
                    .is_some_and(|target_id| !self.actors[&target_id].alive())
            {
                actor.mech_lock_target
            } else {
                None
            }
        };
        if stale_bodyless_attack_target.is_some()
            && !self.actors[&actor_id].rules.attack.quick_switch_target
        {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.motion = MotionState::Idle;
            actor.mech_lock_target = None;
            actor.fight_skill_phase = FightSkillPhase::Idle;
            actor.next_target_x_q32 = actor.x_q32;
            actor.next_target_z_q32 = actor.z_q32;
            actor.next_speed_q32 = 0;
            actor.next_max_speed_q32 = space_to_q32(actor.rules.move_speed());
            return Ok(());
        }
        let bodyless_skill_starts_before_idle_search = {
            let actor = &self.actors[&actor_id];
            actor.motion == MotionState::Attacking
                && actor.fight_skill_phase == FightSkillPhase::Idle
                && !actor.rules.has_body
                && !actor.motion_attack_hold_fire
                && actor.pending.is_none()
                && actor.backswing_finish_step.is_none()
                && actor
                    .mech_lock_target
                    .is_some_and(|target_id| self.bodyless_target_in_attack_area(actor_id, target_id))
        };
        if bodyless_skill_starts_before_idle_search {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            let prepare_steps =
                native_time_units_to_steps(actor.rules.attack.prepare_time_units());
            actor.fight_skill_phase = if prepare_steps == 0 {
                FightSkillPhase::Attack
            } else {
                FightSkillPhase::Prepare {
                    finish_step: step.saturating_add(prepare_steps),
                }
            };
        }
        let quick_switch_backswing_due = {
            let actor = &self.actors[&actor_id];
            actor.rules.attack.quick_switch_target
                && actor
                    .backswing_finish_step
                    .is_some_and(|finish_step| finish_step >= step)
                && step > actor.next_attack_step
        };
        let quick_switch_dead_backswing_due = quick_switch_backswing_due
            && self.actors[&actor_id]
                .mech_lock_target
                .is_some_and(|target_id| !self.actors[&target_id].alive());
        let dead_backswing_just_finished = backswing_just_finished
            && self.actors[&actor_id]
                .mech_lock_target
                .is_some_and(|target_id| !self.actors[&target_id].alive());
        self.update_fight_skill_target_search(actor_id, step, target_search_order)?;
        if quick_switch_backswing_due && !quick_switch_dead_backswing_due {
            self.actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .backswing_finish_step = None;
        }
        let stale_replacement = if stale_bodyless_attack_target.is_some()
            || quick_switch_dead_backswing_due
            || dead_backswing_just_finished
        {
            self.actors[&actor_id].mech_lock_target
        } else {
            None
        };
        let stale_replacement_outside_attack_area = stale_replacement.is_some_and(|target_id| {
            !self.bodyless_target_in_attack_area(actor_id, target_id)
        });
        if stale_replacement_outside_attack_area {
            // SkillAttackableChecker can adopt an immediately attackable
            // replacement. A replacement outside its range or root-transform
            // attack angle first exits through SkillIdleState; SimpleFSM does
            // not recursively update the newly entered state in this tick.
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            let entered_idle = actor.motion != MotionState::Idle;
            actor.motion = MotionState::Idle;
            actor.mech_lock_target = None;
            actor.fight_skill_phase = FightSkillPhase::Idle;
            actor.backswing_finish_step = None;
            actor.retarget_after_own_direct_kill = false;
            if entered_idle {
                actor.next_target_x_q32 = actor.x_q32;
                actor.next_target_z_q32 = actor.z_q32;
            }
            actor.next_speed_q32 = 0;
            actor.next_max_speed_q32 = space_to_q32(actor.rules.move_speed());
            return Ok(());
        }
        if backswing_just_finished {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.backswing_finish_step = None;
            actor.fight_skill_phase = if actor.rules.attack.quick_switch_target {
                FightSkillPhase::Attack
            } else {
                FightSkillPhase::Idle
            };
        }
        let prepare_finished = matches!(
            self.actors[&actor_id].fight_skill_phase,
            FightSkillPhase::Prepare { finish_step } if finish_step <= step
        );
        if prepare_finished {
            self.actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .fight_skill_phase = FightSkillPhase::Attack;
        }
        self.quick_switch_bodyless_pending_target(actor_id, step, target_search_order)?;
        let active_attack_rejected = self.actors[&actor_id].pending.is_some_and(|pending| {
            self.bodyless_attackable_invalid(actor_id, pending.target)
        });
        if active_attack_rejected {
            // Build 2259 SkillPrepareState and SkillAttackState both run
            // CheckAttackable before advancing their current attack phase.
            // A failed check enters SkillIdleState in the same update.
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.motion = MotionState::Idle;
            actor.mech_lock_target = None;
            actor.pending = None;
            actor.fight_skill_phase = FightSkillPhase::Idle;
            actor.next_target_x_q32 = actor.x_q32;
            actor.next_target_z_q32 = actor.z_q32;
            actor.next_speed_q32 = 0;
            actor.next_max_speed_q32 = space_to_q32(actor.rules.move_speed());
            return Ok(());
        }
        let released_this_step = self.actors[&actor_id]
            .pending
            .is_some_and(|pending| pending.step <= step);
        let attack_point_rejected = if released_this_step {
            self.release(actor_id, events)?
        } else {
            false
        };
        let mech_lock_target = self.actors[&actor_id].mech_lock_target;
        if let Some(target_id) = mech_lock_target {
            let target_alive = self.actors.get(&target_id).is_some_and(Actor::alive);
            if !target_alive && self.actors[&actor_id].backswing_finish_step.is_some() {
                // Build 2259 enters idle but retains the dead target through
                // the remaining backswing even when an ally dealt the kill.
                // MotionIdleState.Enter publishes StopMove once; its Update
                // does not refresh that target on every remaining backswing
                // tick.
                let actor = self
                    .actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable");
                let entered_idle = actor.motion != MotionState::Idle;
                actor.motion = MotionState::Idle;
                if entered_idle {
                    actor.next_target_x_q32 = actor.x_q32;
                    actor.next_target_z_q32 = actor.z_q32;
                }
                actor.next_speed_q32 = 0;
                actor.next_max_speed_q32 = space_to_q32(actor.rules.move_speed());
                return Ok(());
            }
            if !target_alive && backswing_just_finished {
                let actor = self
                    .actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable");
                let entered_idle = actor.motion != MotionState::Idle;
                actor.mech_lock_target = None;
                actor.retarget_after_own_direct_kill = false;
                actor.motion = MotionState::Idle;
                if entered_idle {
                    actor.next_target_x_q32 = actor.x_q32;
                    actor.next_target_z_q32 = actor.z_q32;
                }
                actor.next_speed_q32 = 0;
                actor.next_max_speed_q32 = space_to_q32(actor.rules.move_speed());
                return Ok(());
            }
            if !target_alive {
                self.actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable")
                    .mech_lock_target = None;
            }
        }
        let target_id = self.actors[&actor_id].mech_lock_target;
        let Some(target_id) = target_id else {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.motion = MotionState::Idle;
            actor.next_target_x_q32 = actor.x_q32;
            actor.next_target_z_q32 = actor.z_q32;
            actor.next_speed_q32 = 0;
            actor.next_max_speed_q32 = space_to_q32(actor.rules.move_speed());
            return Ok(());
        };
        let target = &self.actors[&target_id];
        let target_x_q32 = target.x_q32;
        let target_z_q32 = target.z_q32;
        let target_radius = target.rules.collision_radius();
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let target_rotation_q32 = direction_degrees_q32_raw(
            target_x_q32.saturating_sub(actor.x_q32),
            target_z_q32.saturating_sub(actor.z_q32),
        );
        let target_rotation = degrees_q32_to_mdeg(target_rotation_q32);
        let center_distance_q32 = native_q32_magnitude(
            target_x_q32.saturating_sub(actor.x_q32),
            target_z_q32.saturating_sub(actor.z_q32),
        );
        let edge_distance_q32 = center_distance_q32
            .saturating_sub(space_to_q32(actor.rules.collision_radius()))
            .saturating_sub(space_to_q32(target_radius));
        if edge_distance_q32 <= space_to_q32(actor.rules.attack.range()) {
            let (entered_attack, release_now, clear_hold_after_motion) = {
                let entered_attack = actor.motion != MotionState::Attacking;
                actor.motion = MotionState::Attacking;
                // RVOControllerFixed.StopMove refreshes the target point on
                // every MotionAttackState update. It submits zero desired
                // speed while retaining the unit's configured maximum speed,
                // so neighbouring agents can still push a stopped attacker.
                actor.next_target_x_q32 = actor.x_q32;
                actor.next_target_z_q32 = actor.z_q32;
                actor.next_speed_q32 = 0;
                actor.next_max_speed_q32 = space_to_q32(actor.rules.move_speed());
                let in_attack_angle = if actor.rules.has_body {
                    actor.weapons_in_attack_angle(target_rotation_q32)
                } else {
                    // SkillAttackAngleChecker falls back to the FightMech
                    // transform when a bodyless unit's weapon has no own
                    // transform. Its root rotation is therefore the attack
                    // gate even though FightSkill also updates weapon state.
                    rotation_distance_q32(actor.body_rotation_q32, target_rotation_q32)
                        <= mdeg_to_degrees_q32(actor.rules.attack.attack_half_angle_mdeg())
                };
                let completed_attack_reentry_rejected = entered_attack
                    && backswing_just_finished
                    && matches!(actor.rules.attack.path, AttackPath::Direct { melee: true })
                    && !actor.rules.has_body
                    && !in_attack_angle;
                if completed_attack_reentry_rejected {
                    actor.motion = MotionState::Idle;
                    actor.mech_lock_target = None;
                    actor.fight_skill_phase = FightSkillPhase::Idle;
                    actor.next_target_x_q32 = actor.x_q32;
                    actor.next_target_z_q32 = actor.z_q32;
                    actor.next_speed_q32 = 0;
                    actor.next_max_speed_q32 = space_to_q32(actor.rules.move_speed());
                    return Ok(());
                }
                if entered_attack {
                    // A newly entered bodyless attack state cannot start its
                    // FightSkill while the root transform is outside the
                    // attack cone. MotionController clears this hold only
                    // after it has observed and corrected the facing.
                    actor.motion_attack_hold_fire = !actor.rules.has_body
                        && !in_attack_angle
                        && matches!(
                            actor.rules.attack.path,
                            AttackPath::Projectile { .. } | AttackPath::Direct { melee: true }
                        );
                }
                let invalid_attack_angle_barrier = !actor.rules.has_body
                        && !entered_attack
                        && !actor.motion_attack_hold_fire
                        && !in_attack_angle
                        && actor.pending.is_none()
                        && actor.backswing_finish_step.is_none();
                if invalid_attack_angle_barrier {
                    // MotionAttackState returns to Idle when an active bodyless
                    // skill loses its root-transform attack angle. The new
                    // Idle state is entered synchronously but is not updated
                    // recursively, so target reacquisition waits one tick and
                    // this transition tick preserves the old body facing.
                    actor.motion = MotionState::Idle;
                    actor.mech_lock_target = None;
                    actor.fight_skill_phase = FightSkillPhase::Idle;
                    actor.next_target_x_q32 = actor.x_q32;
                    actor.next_target_z_q32 = actor.z_q32;
                    actor.next_speed_q32 = 0;
                    actor.next_max_speed_q32 = space_to_q32(actor.rules.move_speed());
                    return Ok(());
                }
                let clear_hold_after_motion = actor.motion_attack_hold_fire && in_attack_angle;
                let mut entered_skill_phase = false;
                if !entered_attack
                    && !actor.motion_attack_hold_fire
                    && in_attack_angle
                    && actor.pending.is_none()
                    && actor.backswing_finish_step.is_none()
                    && actor.fight_skill_phase == FightSkillPhase::Idle
                {
                    let prepare_steps =
                        native_time_units_to_steps(actor.rules.attack.prepare_time_units());
                    actor.fight_skill_phase = if prepare_steps == 0 {
                        FightSkillPhase::Attack
                    } else {
                        FightSkillPhase::Prepare {
                            finish_step: step.saturating_add(prepare_steps),
                        }
                    };
                    entered_skill_phase = prepare_steps > 0;
                }
                if (!entered_attack || actor.rules.attack.quick_switch_target)
                    && !actor.motion_attack_hold_fire
                    && in_attack_angle
                    && actor.pending.is_none()
                    && actor.backswing_finish_step.is_none()
                    && actor.fight_skill_phase == FightSkillPhase::Attack
                    && !entered_skill_phase
                    && step >= actor.next_attack_step
                {
                    let interval_steps =
                        native_time_units_to_steps(actor.rules.attack.interval_time_units());
                    let offset_steps =
                        native_time_units_to_steps(actor.rules.attack.interval_offset_time_units());
                    let sample = if offset_steps == 0 {
                        0
                    } else {
                        i64::from(
                            self.team_random
                                .get_mut(&actor.placement.team)
                                .expect("every actor team owns one attack random stream")
                                .next_in_range(i32::try_from(offset_steps).unwrap_or(i32::MAX)),
                        )
                    };
                    let sampled = i64::try_from(interval_steps)
                        .unwrap_or(i64::MAX)
                        .saturating_add(sample)
                        .max(1)
                        .cast_unsigned();
                    actor.next_attack_step = step.saturating_add(sampled);
                    let attack_point_steps =
                        native_time_units_to_steps(actor.rules.attack.attack_point_time_units());
                    actor.pending = Some(PendingRelease {
                        step: step.saturating_add(attack_point_steps),
                        target: target_id,
                    });
                }
                (
                    entered_attack,
                    actor.pending.is_some_and(|pending| pending.step == step),
                    clear_hold_after_motion,
                )
            };
            if release_now {
                let _attack_point_rejected = self.release(actor_id, events)?;
            }
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            // FightSkill.Update rotates every free weapon after its state controller.
            actor.rotate_weapons_towards(target_rotation_q32);
            if entered_attack {
                return Ok(());
            }
            if actor.rules.has_body {
                actor.aim_rotation = target_rotation;
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
                actor.motion_attack_hold_fire = false;
            }
            if actor.rules.has_body
                && (actor.current_velocity_x_q32 != 0 || actor.current_velocity_z_q32 != 0)
            {
                actor.rotate_body_towards(direction_degrees_q32_raw(
                    actor.current_velocity_x_q32,
                    actor.current_velocity_z_q32,
                ));
            }
            return Ok(());
        }
        if actor.motion == MotionState::Attacking
            && !actor.rules.has_body
            && !actor.motion_attack_hold_fire
            && actor.pending.is_none()
            && actor.backswing_finish_step.is_none()
            && (matches!(actor.rules.attack.path, AttackPath::Direct { melee: true })
                || actor.fight_skill_phase == FightSkillPhase::Attack)
        {
            // FightSkill updates before MotionController. An active bodyless
            // attack rejects an out-of-range retained target and enters
            // SkillIdleState before MotionAttackState can fall through to
            // movement. Both state machines expose one targetless Idle tick.
            actor.motion = MotionState::Idle;
            actor.mech_lock_target = None;
            actor.fight_skill_phase = FightSkillPhase::Idle;
            actor.motion_attack_hold_fire = false;
            actor.next_target_x_q32 = actor.x_q32;
            actor.next_target_z_q32 = actor.z_q32;
            actor.next_speed_q32 = 0;
            actor.next_max_speed_q32 = space_to_q32(actor.rules.move_speed());
            return Ok(());
        }
        if attack_point_rejected
            && matches!(actor.rules.attack.path, AttackPath::Direct { melee: true })
            && !actor.rules.has_body
            && actor.motion == MotionState::Attacking
            && actor.pending.is_none()
            && actor.backswing_finish_step.is_none()
        {
            // MotionAttackState leaves through Idle when its current attack
            // target is no longer in range. Idle target acquisition runs on
            // the following update rather than recursively entering Moving.
            actor.motion = MotionState::Idle;
            actor.mech_lock_target = None;
            actor.fight_skill_phase = FightSkillPhase::Idle;
            actor.next_target_x_q32 = actor.x_q32;
            actor.next_target_z_q32 = actor.z_q32;
            actor.next_speed_q32 = 0;
            actor.next_max_speed_q32 = space_to_q32(actor.rules.move_speed());
            return Ok(());
        }
        if backswing_just_finished
            && matches!(actor.rules.attack.path, AttackPath::Direct { melee: true })
            && !actor.rules.has_body
        {
            // SkillAttackState rechecks its retained target after the attack
            // controller finishes. If that target has left the legal attack
            // area, Finish synchronously enters SkillIdleState; SimpleFSM does
            // not update the new state recursively, so MotionController sees
            // one targetless Idle tick before reacquisition on the next tick.
            actor.motion = MotionState::Idle;
            actor.mech_lock_target = None;
            actor.fight_skill_phase = FightSkillPhase::Idle;
            actor.next_target_x_q32 = actor.x_q32;
            actor.next_target_z_q32 = actor.z_q32;
            actor.next_speed_q32 = 0;
            actor.next_max_speed_q32 = space_to_q32(actor.rules.move_speed());
            return Ok(());
        }
        // FightSkill.Update tracks its current target before MotionController updates movement.
        actor.rotate_weapons_towards(target_rotation_q32);
        if actor.rules.has_body {
            actor.aim_rotation = target_rotation;
        }
        let entered_move = actor.motion != MotionState::Moving;
        actor.motion = MotionState::Moving;
        actor.motion_attack_hold_fire = false;
        if entered_move {
            return Ok(());
        }
        let (move_target_x_q32, move_target_z_q32) = native_auto_move_target_point(
            actor.x_q32,
            actor.z_q32,
            actor.rules.collision_radius(),
            target_x_q32,
            target_z_q32,
            target_radius,
            actor.rules.attack.range(),
        );
        actor.next_target_x_q32 = move_target_x_q32;
        actor.next_target_z_q32 = move_target_z_q32;
        if actor.current_velocity_x_q32 != 0 || actor.current_velocity_z_q32 != 0 {
            // MotionMoveState.MoveUpdate runs NormalRotate before Move;
            // CalculateMoveSpeed therefore observes this tick's new facing.
            actor.rotate_body_towards(direction_degrees_q32_raw(
                actor.current_velocity_x_q32,
                actor.current_velocity_z_q32,
            ));
        }
        actor.next_speed_q32 = turn_limited_move_speed_q32(
            space_to_q32(actor.rules.move_speed()),
            actor.rules.rotate_speed_mdeg_per_second(),
            actor.body_rotation_q32,
            actor.current_velocity_x_q32,
            actor.current_velocity_z_q32,
        );
        actor.next_max_speed_q32 = actor.next_speed_q32;
        if !actor.rules.has_body {
            actor.aim_rotation = actor.body_rotation;
        }
        Ok(())
    }

    fn step_actor_rvo_position(&mut self, actor_id: u64) {
        let rvo_boundary_due = self.rvo_counter == 3;
        let changed = {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            if !actor.alive() {
                return;
            }
            let maximum_delta_q32 = q32_mul(actor.published_speed_q32, NATIVE_LOGIC_DELTA_Q32);
            let (movement_x_q32, movement_z_q32) = clamp_magnitude_q32_raw(
                actor.published_target_x_q32.saturating_sub(actor.x_q32),
                actor.published_target_z_q32.saturating_sub(actor.z_q32),
                maximum_delta_q32,
            );
            actor.x_q32 = actor.x_q32.saturating_add(movement_x_q32);
            actor.z_q32 = actor.z_q32.saturating_add(movement_z_q32);
            if actor.motion != MotionState::Moving
                && actor.published_speed_q32 == 0
                && (movement_x_q32 != 0 || movement_z_q32 != 0)
            {
                actor.rvo_stopped_snap_since_boundary = true;
            }
            if actor.motion == MotionState::Moving && !rvo_boundary_due {
                actor.rvo_stopped_snap_since_boundary = false;
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
                    NormalTargetCandidate::Unit(actor_id),
                    changed.2,
                    changed.3,
                    changed.4,
                );
        }
    }

    fn step_rvo(&mut self) -> Result<()> {
        self.rvo_counter += 1;
        if self.rvo_counter < 4 {
            return Ok(());
        }
        self.rvo_counter = 0;
        for actor in self.actors.values_mut().filter(|actor| actor.alive()) {
            if actor.rvo_stopped_snap_since_boundary
                && actor.motion == MotionState::Moving
                && actor.next_speed_q32 > 0
                && actor.solver_speed_q32 == 0
            {
                actor.solver_target_x_q32 = actor.x_q32;
                actor.solver_target_z_q32 = actor.z_q32;
            }
            actor.published_target_x_q32 = actor.solver_target_x_q32;
            actor.published_target_z_q32 = actor.solver_target_z_q32;
            actor.published_speed_q32 = actor.solver_speed_q32;
            (actor.current_velocity_x_q32, actor.current_velocity_z_q32) =
                normalized_velocity_q32_raw(
                    actor.published_target_x_q32.saturating_sub(actor.x_q32),
                    actor.published_target_z_q32.saturating_sub(actor.z_q32),
                    actor.published_speed_q32,
                );
        }

        // Native's first four-tick boundary only primes the double buffer.
        // The first sampled-RVO solve occurs after that straight desired
        // publication, and its result becomes visible four ticks later.
        if !self.rvo_solver_started {
            for actor in self.actors.values_mut().filter(|actor| actor.alive()) {
                actor.solver_target_x_q32 = actor.next_target_x_q32;
                actor.solver_target_z_q32 = actor.next_target_z_q32;
                actor.solver_speed_q32 = actor.next_speed_q32;
            }
            self.rvo_solver_started = true;
            return Ok(());
        }

        let mut agents = Vec::new();
        let mut building_indices = (0..self.buildings.len()).collect::<Vec<_>>();
        building_indices.sort_by_key(|index| {
            let building = &self.buildings[*index];
            (building.team_id, building.building_id)
        });
        let (tower_layer, tower_collides_with) =
            immovable_rvo_collision_masks(CORE_TOWER_RVO_COLLIDER_PRIORITY);
        for index in building_indices {
            let building = &self.buildings[index];
            if !building.alive || !building.collision_enabled {
                continue;
            }
            let radius_q32 = space_to_q32(building.bounds_width / 2);
            agents.push(RvoAgentInput {
                key: RvoAgentKey::Building(building.building_id),
                main_layer: 1,
                layer: tower_layer,
                collides_with: tower_collides_with,
                group: i32::try_from(building.team_id).unwrap_or(i32::MAX),
                locked: true,
                tree_position: rvo_position(
                    space_to_q32(building.position.x),
                    space_to_q32(building.position.z),
                ),
                position: rvo_position(
                    space_to_q32(building.position.x),
                    space_to_q32(building.position.z),
                ),
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
        agents.extend(crate::rvo::training_ground_static_agents());
        for (&actor_id, actor) in self.actors.iter().filter(|(_, actor)| actor.alive()) {
            let profile = rvo_profile(&actor.rules);
            let (layer, collides_with) = movable_rvo_collision_masks(profile.collider_priority);
            let target_delta = FixedVec2 {
                x: actor.next_target_x_q32.saturating_sub(actor.x_q32),
                y: actor.next_target_z_q32.saturating_sub(actor.z_q32),
            };
            let (desired_x, desired_z) =
                normalized_velocity_q32_raw(target_delta.x, target_delta.y, actor.next_speed_q32);
            agents.push(RvoAgentInput {
                key: RvoAgentKey::Unit(actor_id),
                main_layer: match actor.rules.domain {
                    UnitDomain::Ground => 1,
                    UnitDomain::Air => 2,
                },
                layer,
                collides_with,
                group: i32::try_from(actor.placement.team).unwrap_or(i32::MAX),
                locked: false,
                tree_position: rvo_position(actor.rvo_tree_x_q32, actor.rvo_tree_z_q32),
                position: rvo_position(actor.x_q32, actor.z_q32),
                current_velocity: FixedVec2 {
                    x: actor.current_velocity_x_q32,
                    y: actor.current_velocity_z_q32,
                },
                desired_velocity: FixedVec2 {
                    x: desired_x,
                    y: desired_z,
                },
                desired_target_delta: target_delta,
                desired_speed: actor.next_speed_q32,
                max_speed: actor.next_max_speed_q32,
                published_calculated_speed: actor.published_speed_q32,
                radius_outer: profile.outer_radius_q32,
                radius_inner: profile.inner_radius_q32,
                size: profile.size,
                priority: profile.priority_q32,
            });
        }

        let inverse_delta_time = q32_div(Q32_ONE, NATIVE_LOGIC_DELTA_Q32.saturating_mul(4));
        let solutions = crate::rvo::solve_agents(&agents, inverse_delta_time);
        for (&actor_id, actor) in self.actors.iter_mut().filter(|(_, actor)| actor.alive()) {
            let solution = solutions
                .get(&RvoAgentKey::Unit(actor_id))
                .expect("every live actor has an RVO solution");
            actor.solver_target_x_q32 = actor.x_q32.saturating_add(solution.target_delta.x);
            actor.solver_target_z_q32 = actor.z_q32.saturating_add(solution.target_delta.y);
            actor.solver_speed_q32 = solution.speed;
            actor.rvo_tree_x_q32 = actor.x_q32;
            actor.rvo_tree_z_q32 = actor.z_q32;
            actor.rvo_stopped_snap_since_boundary = false;
        }
        Ok(())
    }

    fn quick_switch_bodyless_pending_target(
        &mut self,
        actor_id: u64,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<NormalTargetCandidate>>,
    ) -> Result<()> {
        let Some(dead_target_id) = self.actors[&actor_id]
            .pending
            .map(|pending| pending.target)
            .filter(|&target_id| !self.actors[&target_id].alive())
        else {
            return Ok(());
        };
        let actor = &self.actors[&actor_id];
        if actor.rules.has_body || !actor.rules.attack.quick_switch_target {
            return Ok(());
        }
        let target_died_during_tick = self.actors[&dead_target_id].target_query_alive;
        let mut selected = self
            .select_normal_unit_target_with_order(
                actor_id,
                target_search_order,
                target_died_during_tick,
            )
            .map_err(|error| Error::new(format!("logic step {step}: {error}")))?;
        if !target_died_during_tick
            && selected
                .and_then(|target_id| self.actors.get(&target_id))
                .is_some_and(|target| target.target_query_alive && !target.alive())
        {
            selected = self
                .select_normal_unit_target_with_order(actor_id, target_search_order, true)
                .map_err(|error| Error::new(format!("logic step {step}: {error}")))?;
        }
        let Some(selected) = selected.filter(|&target_id| {
            self.bodyless_target_in_attack_area(actor_id, target_id)
        }) else {
            return Ok(());
        };
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        actor.mech_lock_target = Some(selected);
        actor.pending
            .as_mut()
            .expect("pending attack identity is stable")
            .target = selected;
        Ok(())
    }

    fn bodyless_target_in_attack_area(&self, actor_id: u64, target_id: u64) -> bool {
        self.bodyless_target_in_attack_range(actor_id, target_id)
            && self.bodyless_target_in_attack_angle(actor_id, target_id)
    }

    fn bodyless_target_in_attack_range(&self, actor_id: u64, target_id: u64) -> bool {
        let actor = &self.actors[&actor_id];
        let target = &self.actors[&target_id];
        if !target.alive() {
            return false;
        }
        let center_distance_q32 = native_q32_magnitude(
            target.x_q32.saturating_sub(actor.x_q32),
            target.z_q32.saturating_sub(actor.z_q32),
        );
        let edge_distance_q32 = center_distance_q32
            .saturating_sub(space_to_q32(actor.rules.collision_radius()))
            .saturating_sub(space_to_q32(target.rules.collision_radius()));
        edge_distance_q32 <= space_to_q32(actor.rules.attack.range())
    }

    fn bodyless_target_in_attack_angle(&self, actor_id: u64, target_id: u64) -> bool {
        let actor = &self.actors[&actor_id];
        let target = &self.actors[&target_id];
        target.alive()
            && rotation_distance_q32(
            actor.body_rotation_q32,
            direction_degrees_q32_raw(
                target.x_q32.saturating_sub(actor.x_q32),
                target.z_q32.saturating_sub(actor.z_q32),
            ),
        ) <= mdeg_to_degrees_q32(actor.rules.attack.attack_half_angle_mdeg())
    }

    fn bodyless_attackable_invalid(&self, actor_id: u64, target_id: u64) -> bool {
        let actor = &self.actors[&actor_id];
        if actor.rules.has_body {
            return false;
        }
        !self.bodyless_target_in_attack_area(actor_id, target_id)
    }

    fn release(&mut self, actor_id: u64, events: &mut Vec<Event>) -> Result<bool> {
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
        {
            FightSkillPhase::Idle
        } else {
            FightSkillPhase::Attack
        };
        if matches!(
            self.actors[&actor_id].rules.attack.path,
            AttackPath::Direct { .. }
        ) {
            let target_was_alive = self.actors[&pending.target].alive();
            self.direct_effect(actor_id, pending.target, events)?;
            if target_was_alive && !self.actors[&pending.target].alive() {
                self.actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable")
                    .retarget_after_own_direct_kill = true;
            }
            return Ok(false);
        }
        let target = &self.actors[&pending.target];
        let target_x = target.x;
        let target_y = unit_height(target.rules.domain);
        let target_z = target.z;
        let target_x_q32 = target.x_q32;
        let target_z_q32 = target.z_q32;
        let target_radius = target.rules.collision_radius();
        let projectile_id = self.identities.allocate_object(ObjectKind::Projectile)?.id;
        let owner = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let projectile = Projectile {
            id: projectile_id,
            team: owner.placement.team,
            owner: actor_id,
            target: pending.target,
            x: owner.x,
            y: unit_height(owner.rules.domain),
            z: owner.z,
            x_q32: owner.x_q32,
            z_q32: owner.z_q32,
            cached_target_x: target_x,
            cached_target_y: target_y,
            cached_target_z: target_z,
            cached_target_x_q32: target_x_q32,
            cached_target_z_q32: target_z_q32,
            cached_target_radius: target_radius,
            speed: owner.rules.attack.projectile_speed(),
            damage: owner.rules.attack.base_damage,
        };
        let projectile_ref = projectile.object_ref();
        events.push(event(
            Some(projectile_ref),
            Some(owner.object_ref()),
            Some(ObjectRef::new(ObjectKind::Unit, pending.target)),
            EventPayload::ProjectileReleased,
        ));
        self.projectiles.push(projectile);
        Ok(false)
    }

    fn direct_effect(
        &mut self,
        actor_id: u64,
        target_id: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let attacker = &self.actors[&actor_id];
        let damage = attacker.rules.attack.base_damage;
        let attacker_team = attacker.placement.team;
        let splash_radius = attacker.rules.attack.splash_radius();
        let target_domain = attacker.rules.attack.targets;
        let center_x = self.actors[&target_id].x;
        let center_z = self.actors[&target_id].z;
        let target_ids = self
            .actors
            .iter()
            .filter_map(|(&candidate_id, candidate)| {
                let dx = candidate.x.saturating_sub(center_x);
                let dz = candidate.z.saturating_sub(center_z);
                let edge_distance =
                    magnitude(dx, dz).saturating_sub(candidate.rules.collision_radius());
                (candidate.alive()
                    && candidate.placement.team != attacker_team
                    && match candidate.rules.domain {
                        UnitDomain::Ground => target_domain.ground,
                        UnitDomain::Air => target_domain.air,
                    }
                    && (candidate_id == target_id
                        || (splash_radius > 0 && edge_distance <= splash_radius)))
                    .then_some(candidate_id)
            })
            .collect::<Vec<_>>();
        if splash_radius > 0
            && self.buildings.iter().any(|building| {
                building.alive
                    && building.targetable
                    && building.team_id != attacker_team
                    && target_domain.ground
                    && magnitude(
                        building.position.x.saturating_sub(center_x),
                        building.position.z.saturating_sub(center_z),
                    )
                    .saturating_sub(building.bounds_width / 2)
                        <= splash_radius
            })
        {
            return Err(Error::new("direct splash against a building is not closed"));
        }
        let mut aggregate_damage = 0;
        for affected_id in target_ids {
            let target = self
                .actors
                .get_mut(&affected_id)
                .ok_or_else(|| Error::new("direct attack target is absent"))?;
            let previous_life = target.life;
            target.life = target.life.saturating_sub(damage).max(0);
            aggregate_damage += previous_life - target.life;
            if target.life == 0 {
                target.exit_fight_on_death();
            }
        }
        events.push(event(
            None,
            None,
            Some(ObjectRef::new(ObjectKind::Unit, target_id)),
            EventPayload::Damage {
                amount: aggregate_damage,
            },
        ));
        Ok(())
    }

    #[allow(clippy::similar_names)] // Paired fixed-point x/z components are intentionally parallel.
    fn step_projectiles(&mut self, events: &mut Vec<Event>) -> Result<()> {
        let mut retained = Vec::with_capacity(self.projectiles.len());
        // Build 2259's ProjectileSystem keeps registration order in its List,
        // but Update walks that list from Count - 1 down to zero. Preserve the
        // list order after this reverse update pass so later ticks use the
        // same stable registration sequence.
        for mut projectile in std::mem::take(&mut self.projectiles).into_iter().rev() {
            if let Some(target) = self
                .actors
                .get(&projectile.target)
                .filter(|actor| actor.alive())
            {
                projectile.cached_target_x = target.x;
                projectile.cached_target_z = target.z;
                projectile.cached_target_x_q32 = target.x_q32;
                projectile.cached_target_z_q32 = target.z_q32;
                projectile.cached_target_radius = target.rules.collision_radius();
            }
            let dx_q32 = projectile
                .cached_target_x_q32
                .saturating_sub(projectile.x_q32);
            let dz_q32 = projectile
                .cached_target_z_q32
                .saturating_sub(projectile.z_q32);
            let distance_q32 = native_q32_magnitude(dx_q32, dz_q32);
            if distance_q32 < space_to_q32(projectile.cached_target_radius) {
                self.impact(&projectile, events)?;
            } else {
                let step_q32 = q32_mul(
                    space_to_q32(projectile.speed),
                    Q32_ONE.saturating_mul(LOGIC_TICK_TIME_UNITS.cast_signed())
                        / TIME_UNITS_PER_SECOND.cast_signed(),
                );
                if distance_q32 > 0 {
                    let move_q32 = step_q32.min(distance_q32);
                    let reciprocal = q32_div(Q32_ONE, distance_q32);
                    projectile.x_q32 = projectile
                        .x_q32
                        .saturating_add(q32_mul(q32_mul(dx_q32, reciprocal), move_q32));
                    projectile.z_q32 = projectile
                        .z_q32
                        .saturating_add(q32_mul(q32_mul(dz_q32, reciprocal), move_q32));
                }
                projectile.x = q32_to_space_rounded(projectile.x_q32);
                projectile.z = q32_to_space_rounded(projectile.z_q32);
                retained.push(projectile);
            }
        }
        retained.reverse();
        self.projectiles = retained;
        Ok(())
    }

    fn impact(&mut self, projectile: &Projectile, events: &mut Vec<Event>) -> Result<()> {
        let owner = self
            .actors
            .get(&projectile.owner)
            .ok_or_else(|| Error::new("projectile owner is absent"))?;
        let splash_radius = owner.rules.attack.splash_radius();
        if splash_radius > 0 {
            let owner_team = owner.placement.team;
            let targets = owner.rules.attack.targets;
            let secondary_unit = self.actors.iter().any(|(&candidate_id, candidate)| {
                candidate_id != projectile.target
                    && candidate.alive()
                    && candidate.placement.team != owner_team
                    && match candidate.rules.domain {
                        UnitDomain::Ground => targets.ground,
                        UnitDomain::Air => targets.air,
                    }
                    && [
                        (projectile.x, projectile.z),
                        (projectile.cached_target_x, projectile.cached_target_z),
                    ]
                    .into_iter()
                    .any(|(center_x, center_z)| {
                        magnitude(
                            candidate.x.saturating_sub(center_x),
                            candidate.z.saturating_sub(center_z),
                        )
                        .saturating_sub(candidate.rules.collision_radius())
                            <= splash_radius
                    })
            });
            let secondary_building = targets.ground
                && self.buildings.iter().any(|building| {
                    building.alive
                        && building.targetable
                        && building.team_id != owner_team
                        && [
                            (projectile.x, projectile.z),
                            (projectile.cached_target_x, projectile.cached_target_z),
                        ]
                        .into_iter()
                        .any(|(center_x, center_z)| {
                            magnitude(
                                building.position.x.saturating_sub(center_x),
                                building.position.z.saturating_sub(center_z),
                            )
                            .saturating_sub(building.bounds_width / 2)
                                <= splash_radius
                        })
                });
            if secondary_unit || secondary_building {
                return Err(Error::new(
                    "projectile splash with a secondary target is not closed",
                ));
            }
        }
        let projectile_ref = projectile.object_ref();
        let owner_ref = ObjectRef::new(ObjectKind::Unit, projectile.owner);
        let target_ref = ObjectRef::new(ObjectKind::Unit, projectile.target);
        let target = self
            .actors
            .get_mut(&projectile.target)
            .ok_or_else(|| Error::new("projectile target is absent"))?;
        if target.alive() {
            let previous_life = target.life;
            target.life = target.life.saturating_sub(projectile.damage).max(0);
            events.push(event(
                None,
                Some(projectile_ref),
                Some(target_ref),
                EventPayload::Damage {
                    amount: previous_life - target.life,
                },
            ));
            if target.life == 0 {
                target.exit_fight_on_death();
            }
        }
        events.push(event(
            Some(projectile_ref),
            Some(owner_ref),
            Some(target_ref),
            EventPayload::ProjectileRemoved {
                position: point_at_height(projectile.x, projectile.y, projectile.z),
                intercepted: false,
            },
        ));
        Ok(())
    }

    fn winner(&self) -> Option<u32> {
        let blue = self
            .actors
            .values()
            .any(|actor| actor.placement.team == 0 && actor.alive());
        let red = self
            .actors
            .values()
            .any(|actor| actor.placement.team == 1 && actor.alive());
        match (blue, red) {
            (true, false) => Some(0),
            (false, true) => Some(1),
            _ => None,
        }
    }

    fn naturally_finished(&self) -> bool {
        let living_teams = self
            .actors
            .values()
            .filter(|actor| actor.alive())
            .map(|actor| actor.placement.team)
            .collect::<std::collections::BTreeSet<_>>();
        living_teams.len() < 2 && self.projectiles.is_empty()
    }

    fn ready_to_finish(&self) -> bool {
        self.naturally_finished() && !self.terminal_drain_pending
    }
}

pub(crate) fn run(
    layout: &CompiledLayout,
    config: &SimulationConfig,
    seed: i32,
    seed_source: &'static str,
    output: &std::path::Path,
) -> Result<SimulationResult> {
    let divisor = gcd(LOGIC_TICK_TIME_UNITS, TIME_UNITS_PER_SECOND);
    let context = DurableContext {
        schema_version: MCFR_SCHEMA_VERSION,
        game_build: config.game_build.clone(),
        logic_step: Rational {
            numerator: LOGIC_TICK_TIME_UNITS / divisor,
            denominator: TIME_UNITS_PER_SECOND / divisor,
        },
        numeric_convention: NumericConvention {
            distance_units_per_meter: SPACE_UNITS_PER_METER.cast_unsigned(),
            rotation_units_per_degree: 1_000,
            time_units_per_second: TIME_UNITS_PER_SECOND,
        },
        combat_round: layout.round,
        match_seed: seed,
        identity_contract: IdentityContract::TeamZxSequentialV1,
    };
    let mut simulation = Simulation::new(layout, &config.units, &config.training_ground, seed)?;
    let mut writer = McfrWriter::create(output, &context)?;
    writer.append_tick(
        simulation.snapshot(),
        &TransitionEvents { events: Vec::new() },
    )?;
    let mut steps = 0;
    let max_steps = FIGHT_TIME_SECONDS
        .saturating_mul(TIME_UNITS_PER_SECOND)
        .div_ceil(LOGIC_TICK_TIME_UNITS);
    let end_reason = loop {
        if steps >= max_steps {
            break "forced_time_limit";
        }
        let events = simulation.step(steps)?;
        steps += 1;
        writer.append_tick(simulation.snapshot(), &events)?;
        if simulation.ready_to_finish() {
            break "natural_module_drain";
        }
    };
    let hashes = writer.finish()?;
    let published = mechcore_mcfr::McfrReader::open(output)?;
    if published.hashes() != &hashes {
        return Err(Error::new("published MCFR hashes changed after reopening"));
    }
    let winner = simulation.winner().map(team_name);
    Ok(SimulationResult {
        schema: "mechcore.simulation-result.v1",
        game_build: config.game_build.clone(),
        seed,
        seed_source,
        output: output.display().to_string(),
        end_reason,
        steps,
        elapsed_milliseconds: steps
            .saturating_mul(LOGIC_TICK_TIME_UNITS)
            .saturating_mul(1_000)
            / TIME_UNITS_PER_SECOND,
        winner,
        draw: winner.is_none(),
        teams: simulation
            .actors
            .values()
            .map(|actor| TeamResult {
                team: team_name(actor.placement.team),
                unit: actor.placement.type_name.clone(),
                alive: actor.alive(),
                remaining_life: actor.life,
                max_life: actor.rules.max_life,
            })
            .collect(),
        hashes,
    })
}

const fn gcd(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

fn native_time_units_to_steps(time_units: u64) -> u64 {
    let raw_time =
        i64::try_from((u128::from(time_units) << 32) / u128::from(TIME_UNITS_PER_SECOND))
            .unwrap_or(i64::MAX);
    q32_div(raw_time, NATIVE_LOGIC_DELTA_Q32)
        .max(0)
        .cast_unsigned()
        >> 32
}

fn team_name(team: u32) -> &'static str {
    if team == 0 { "blue" } else { "red" }
}

#[cfg(test)]
fn rotation_distance(left: i64, right: i64) -> i64 {
    ((right.rem_euclid(360_000) - left.rem_euclid(360_000) + 540_000).rem_euclid(360_000) - 180_000)
        .abs()
}

fn mdeg_to_degrees_q32(value: i64) -> i64 {
    i64::try_from(i128::from(value) * i128::from(Q32_ONE) / 1_000).unwrap_or(if value < 0 {
        i64::MIN
    } else {
        i64::MAX
    })
}

fn degrees_q32_to_mdeg(value: i64) -> i64 {
    degrees_q32_to_unwrapped_mdeg(value) % 360_000
}

fn degrees_q32_to_unwrapped_mdeg(value: i64) -> i64 {
    let scaled = i128::from(value) * 1_000;
    let rounded = if scaled >= 0 {
        (scaled + i128::from(Q32_ONE / 2)) >> 32
    } else {
        -((-scaled + i128::from(Q32_ONE / 2)) >> 32)
    };
    i64::try_from(rounded).unwrap_or(if rounded < 0 { i64::MIN } else { i64::MAX })
}

fn rotate_towards_q32(current: i64, target: i64, maximum: i64) -> i64 {
    rotate_towards_q32_unwrapped(current, target, maximum).rem_euclid(360_i64 << 32)
}

fn rotate_towards_q32_unwrapped(current: i64, target: i64, maximum: i64) -> i64 {
    let full = 360_i64 << 32;
    let half = 180_i64 << 32;
    let current = current.rem_euclid(full);
    let target = target.rem_euclid(full);
    let mut delta = (target - current).rem_euclid(full);
    if crate::rvo::fpoint_less_than(half, delta) {
        delta -= full;
    }
    current + delta.clamp(-maximum, maximum)
}

fn rotation_distance_q32(left: i64, right: i64) -> i64 {
    let full = 360_i64 << 32;
    let half = 180_i64 << 32;
    ((right.rem_euclid(full) - left.rem_euclid(full) + full + half).rem_euclid(full) - half).abs()
}

fn turn_limited_move_speed_q32(
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

fn event(
    subject: Option<ObjectRef>,
    source: Option<ObjectRef>,
    target: Option<ObjectRef>,
    payload: EventPayload,
) -> Event {
    Event {
        subject,
        source,
        target,
        payload,
    }
}

const fn point(x: i64, z: i64) -> Vec3 {
    Vec3 { x, y: 0, z }
}

const fn point_at_height(x: i64, y: i64, z: i64) -> Vec3 {
    Vec3 { x, y, z }
}

const fn unit_height(domain: UnitDomain) -> i64 {
    match domain {
        UnitDomain::Ground => 0,
        UnitDomain::Air => AIR_UNIT_HEIGHT,
    }
}

fn magnitude(x: i64, z: i64) -> i64 {
    integer_sqrt(i128::from(x) * i128::from(x) + i128::from(z) * i128::from(z))
}

#[cfg(test)]
fn q32_distance_within(
    source_x_q32: i64,
    source_z_q32: i64,
    target_x_q32: i64,
    target_z_q32: i64,
    bound_space: i64,
) -> bool {
    let dx = i128::from(target_x_q32) - i128::from(source_x_q32);
    let dz = i128::from(target_z_q32) - i128::from(source_z_q32);
    let bound = i128::from(space_to_q32(bound_space).max(0));
    dx.saturating_mul(dx).saturating_add(dz.saturating_mul(dz)) <= bound.saturating_mul(bound)
}

fn space_to_q32(value: i64) -> i64 {
    i64::try_from(i128::from(value) * i128::from(Q32_ONE) / i128::from(SPACE_UNITS_PER_METER))
        .unwrap_or(if value < 0 { i64::MIN } else { i64::MAX })
}

fn q32_to_space_rounded(value: i64) -> i64 {
    let scaled = i128::from(value) * i128::from(SPACE_UNITS_PER_METER);
    let rounded = if scaled >= 0 {
        (scaled + i128::from(Q32_ONE / 2)) >> 32
    } else {
        -((-scaled + i128::from(Q32_ONE / 2)) >> 32)
    };
    i64::try_from(rounded).unwrap_or(if rounded < 0 { i64::MIN } else { i64::MAX })
}

fn q32_mul(left: i64, right: i64) -> i64 {
    i64::try_from((i128::from(left) * i128::from(right)) >> 32).unwrap_or({
        if (left < 0) == (right < 0) {
            i64::MAX
        } else {
            i64::MIN
        }
    })
}

fn q32_div(numerator: i64, denominator: i64) -> i64 {
    if denominator == 0 {
        return if numerator < 0 { i64::MIN } else { i64::MAX };
    }
    let scaled = u128::from(numerator.unsigned_abs()) << 32;
    let divisor = u128::from(denominator.unsigned_abs());
    let quotient = scaled / divisor;
    let remainder = scaled % divisor;
    let rounded = quotient.saturating_add(u128::from(remainder.saturating_mul(2) >= divisor));
    if (numerator < 0) == (denominator < 0) {
        i64::try_from(rounded).unwrap_or(i64::MAX)
    } else {
        i64::try_from(rounded)
            .ok()
            .and_then(i64::checked_neg)
            .unwrap_or(i64::MIN)
    }
}

fn native_q32_magnitude(x: i64, z: i64) -> i64 {
    fpcs_sqrt_fastest(q32_mul(x, x).saturating_add(q32_mul(z, z)))
}

#[allow(clippy::too_many_arguments)]
fn normal_visible_full_rotation_target_score_q32(
    source_x_q32: i64,
    source_z_q32: i64,
    source_radius: i64,
    source_rotation_q32: i64,
    target_x_q32: i64,
    target_z_q32: i64,
    target_radius: i64,
    min_range: i64,
    max_range: i64,
) -> Option<i64> {
    let distance_q32 = native_q32_magnitude(
        target_x_q32.saturating_sub(source_x_q32),
        target_z_q32.saturating_sub(source_z_q32),
    )
    .saturating_sub(space_to_q32(source_radius))
    .saturating_sub(space_to_q32(target_radius))
    .max(0);
    let bearing_q32 = direction_degrees_q32_raw(
        target_x_q32.saturating_sub(source_x_q32),
        target_z_q32.saturating_sub(source_z_q32),
    );
    let full_rotation = 360_i64 << 32;
    let delta_q32 = bearing_q32
        .saturating_sub(source_rotation_q32)
        .rem_euclid(full_rotation);
    let angle_q32 = if delta_q32 > 180_i64 << 32 {
        full_rotation.saturating_sub(delta_q32)
    } else {
        delta_q32
    };
    normal_visible_full_rotation_score_from_distance_and_angle_q32(
        distance_q32,
        angle_q32,
        space_to_q32(min_range),
        space_to_q32(max_range),
    )
}

fn normal_visible_full_rotation_score_from_distance_and_angle_q32(
    distance_q32: i64,
    angle_q32: i64,
    min_range_q32: i64,
    max_range_q32: i64,
) -> Option<i64> {
    if distance_q32 < min_range_q32 {
        return None;
    }
    let angle_score_q32 = q32_mul(
        angle_q32.min(TARGET_SCORE_ANGLE_LIMIT_Q32),
        TARGET_SCORE_ANGLE_FACTOR_Q32,
    );
    let distance_score_q32 = distance_q32.max(TARGET_SCORE_MIN_DISTANCE_Q32);
    let mut score_q32 = q32_mul(
        distance_score_q32,
        TARGET_SCORE_BASE_Q32.saturating_add(angle_score_q32),
    );
    if distance_q32 > max_range_q32 {
        score_q32 = score_q32.saturating_add(TARGET_SCORE_OUT_OF_RANGE_PENALTY_Q32);
    }
    Some(score_q32.saturating_add(distance_q32))
}

fn normalized_velocity_q32_raw(dx: i64, dz: i64, speed: i64) -> (i64, i64) {
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
fn native_auto_move_target_point(
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
        if center_distance > target_distance {
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

fn clamp_magnitude_q32_raw(dx: i64, dz: i64, maximum: i64) -> (i64, i64) {
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

pub(crate) fn fpcs_sqrt_fastest(value: i64) -> i64 {
    if value <= 0 {
        return 0;
    }
    let exponent = 31 - i32::try_from(value.leading_zeros()).unwrap_or(64);
    let normalized = if exponent >= 0 {
        value >> exponent
    } else {
        value.wrapping_shl(exponent.unsigned_abs())
    };
    let mut variable = (i64::from_ne_bytes(0xC000_0000_0000_0000_u64.to_ne_bytes())
        .wrapping_add(normalized.wrapping_shl(30)))
        >> 32;
    let coefficient = variable;
    variable = variable.wrapping_mul(0x0664_5730);
    variable =
        (i64::from_ne_bytes(0xF90F_54C4_0000_0000_u64.to_ne_bytes()).wrapping_add(variable)) >> 32;
    let coefficient = coefficient.wrapping_shl(2);
    variable = variable.wrapping_mul(coefficient);
    variable = (0x1FDA_0F0B_0000_0000_i64.wrapping_add(variable)) >> 32;
    variable = coefficient.wrapping_mul(variable);
    variable = (0x4000_0000_0000_0000_i64.wrapping_add(variable)) >> 32;
    let odd_factor = if exponent & 1 == 0 {
        Q32_ONE
    } else {
        0x0001_6A09_E664
    };
    let mut result = odd_factor.wrapping_mul(variable) >> 30;
    result &= !3;
    let half_exponent = exponent >> 1;
    if half_exponent >= 0 {
        result.wrapping_shl(half_exponent.cast_unsigned())
    } else {
        result >> half_exponent.unsigned_abs()
    }
}

fn q32_exponent(value: i64) -> i32 {
    debug_assert!(value > 0);
    31 - i32::try_from(value.leading_zeros()).unwrap_or(64)
}

fn normalize_q32(value: i64, exponent: i32) -> i64 {
    if exponent >= 0 {
        value >> exponent
    } else {
        value.wrapping_shl(exponent.unsigned_abs())
    }
}

fn fpcs_atan2_div_fastest(y: i64, x: i64) -> i32 {
    debug_assert!(y >= 0 && x > 0 && y <= x);
    let exponent = q32_exponent(x);
    let normalized_y = normalize_q32(y, exponent);
    let normalized_x = normalize_q32(x, exponent);
    let mut variable = (i64::from_ne_bytes(0xC000_0000_0000_0000_u64.to_ne_bytes())
        .wrapping_add(normalized_x.wrapping_shl(30)))
        >> 32;
    variable = variable.wrapping_mul(0x279B_5BB0);
    let coefficient = ((i64::from_ne_bytes(0xDD58_0FC7_0000_0000_u64.to_ne_bytes())
        .wrapping_add(variable))
        >> 32)
        .wrapping_mul(
            ((i64::from_ne_bytes(0xC000_0000_0000_0000_u64.to_ne_bytes())
                .wrapping_add(normalized_x.wrapping_shl(30)))
                >> 32)
                .wrapping_shl(2),
        );
    let polynomial = (0x37FD_4590_0000_0000_i64.wrapping_add(coefficient)) >> 32;
    let argument = ((i64::from_ne_bytes(0xC000_0000_0000_0000_u64.to_ne_bytes())
        .wrapping_add(normalized_x.wrapping_shl(30)))
        >> 32)
        .wrapping_shl(2);
    let polynomial = polynomial.wrapping_mul(argument);
    let polynomial = (i64::from_ne_bytes(0xC0C3_D3BF_0000_0000_u64.to_ne_bytes())
        .wrapping_add(polynomial))
        >> 32;
    let polynomial = polynomial.wrapping_mul(argument);
    let polynomial = (0x4000_0000_0000_0000_i64.wrapping_add(polynomial)) >> 32;
    let y_quarter = normalized_y >> 2;
    i32::try_from(polynomial.wrapping_mul(y_quarter) >> 30).unwrap_or(i32::MAX)
}

fn fpcs_atan_polynomial(divided: i32, sign_mask: i64) -> i64 {
    let variable = i64::from(divided);
    let mut polynomial = variable.wrapping_mul(0x2651_FC38);
    polynomial = (i64::from_ne_bytes(0xE8C5_3128_0000_0000_u64.to_ne_bytes())
        .wrapping_add(polynomial))
        >> 32;
    let argument = variable.wrapping_shl(2);
    polynomial = polynomial.wrapping_mul(argument);
    polynomial = (i64::from_ne_bytes(0xFFE4_A871_0000_0000_u64.to_ne_bytes())
        .wrapping_add(polynomial))
        >> 32;
    polynomial = polynomial.wrapping_mul(argument);
    polynomial = (0x4005_9E04_0000_0000_i64.wrapping_add(polynomial)) >> 32;
    polynomial = polynomial.wrapping_mul(argument) >> 30;
    (polynomial & !3) ^ sign_mask
}

pub(crate) fn fpcs_atan2_fastest(y: i64, x: i64) -> i64 {
    const PI_OVER_TWO: i64 = 0x1921_FB544;
    const PI: i64 = 0x3243_F6A89;
    if x == 0 {
        return match y.cmp(&0) {
            Ordering::Greater => PI_OVER_TWO,
            Ordering::Less => -PI_OVER_TWO,
            Ordering::Equal => 0,
        };
    }
    let absolute_x = x.saturating_abs();
    let absolute_y = y.saturating_abs();
    let sign_mask = (x ^ y) >> 63;
    if absolute_x < absolute_y {
        let divided = fpcs_atan2_div_fastest(absolute_x, absolute_y);
        let approximate = fpcs_atan_polynomial(divided, sign_mask);
        if y > 0 {
            PI_OVER_TWO.wrapping_sub(approximate)
        } else {
            (-PI_OVER_TWO).wrapping_sub(approximate)
        }
    } else {
        let divided = fpcs_atan2_div_fastest(absolute_y, absolute_x);
        let approximate = fpcs_atan_polynomial(divided, sign_mask);
        if x > 0 {
            approximate
        } else if y >= 0 {
            approximate.wrapping_add(PI)
        } else {
            approximate.wrapping_sub(PI)
        }
    }
}

pub(crate) fn fpcs_acos_fastest(value: i64) -> i64 {
    let complement = q32_mul(Q32_ONE.saturating_sub(value), Q32_ONE.saturating_add(value));
    fpcs_atan2_fastest(fpcs_sqrt_fastest(complement), value)
}

pub(crate) fn fpcs_sin_fastest(value: i64) -> i64 {
    let turn = q32_mul(value, 0x28BE_60DC) as i32;
    let doubled = turn.wrapping_mul(2);
    let folded = 0x8000_0000_u32.wrapping_sub(turn as u32) as i32;
    let coordinate = if doubled ^ turn >= 0 { turn } else { folded } as i64;
    let scaled_coordinate = coordinate.wrapping_mul(4);
    let squared = coordinate.wrapping_mul(scaled_coordinate) >> 32;
    let coefficient = i64::from_ne_bytes(0xD6CF_6F97_0000_0000_u64.to_ne_bytes())
        .wrapping_add(squared.wrapping_mul(0x12A2_8C60))
        >> 32;
    let polynomial = i64::from_ne_bytes(0x6487_ED51_0000_0000_u64.to_ne_bytes())
        .wrapping_add(coefficient.wrapping_mul(squared).wrapping_mul(4))
        >> 32;
    polynomial.wrapping_mul(scaled_coordinate) >> 30 & !3
}

pub(crate) fn fpcs_cos_fastest(value: i64) -> i64 {
    fpcs_sin_fastest(value.saturating_add(0x1_921F_B544))
}

fn integer_sqrt(value: i128) -> i64 {
    if value <= 0 {
        return 0;
    }
    let value = u128::try_from(value).unwrap_or(u128::MAX);
    let mut low = 0_u128;
    let mut high = value.min(u128::from(u64::MAX));
    while low < high {
        let middle = (low + high).div_ceil(2);
        if middle <= value / middle {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    i64::try_from(low).unwrap_or(i64::MAX)
}

#[cfg(test)]
fn direction_mdeg(dx: i64, dz: i64) -> i64 {
    direction_mdeg_q32_raw(space_to_q32(dx), space_to_q32(dz))
}

#[cfg(test)]
fn direction_mdeg_q32_raw(dx: i64, dz: i64) -> i64 {
    degrees_q32_to_mdeg(direction_degrees_q32_raw(dx, dz))
}

fn direction_degrees_q32_raw(dx: i64, dz: i64) -> i64 {
    if dx == 0 && dz == 0 {
        return 0;
    }
    let magnitude = native_q32_magnitude(dx, dz);
    if magnitude <= 0 {
        return 0;
    }
    let cosine = q32_div(dz, magnitude).clamp(-Q32_ONE, Q32_ONE);
    let radians = fpcs_acos_fastest(cosine);
    let degrees = q32_mul(radians, 0x0039_4BB8_34C8);
    let degrees = if dx < 0 {
        (360_i64 << 32).saturating_sub(degrees)
    } else {
        degrees
    };
    degrees.rem_euclid(360_i64 << 32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target_tree_unit(id: u64) -> NormalTargetCandidate {
        NormalTargetCandidate::Unit(id)
    }

    #[test]
    fn target_quadtree_twentieth_insert_reverses_old_child_ordinals() {
        let positions = [
            (-100 * Q32_ONE, -100 * Q32_ONE),
            (100 * Q32_ONE, -100 * Q32_ONE),
            (-100 * Q32_ONE, 100 * Q32_ONE),
            (100 * Q32_ONE, 100 * Q32_ONE),
        ];
        let mut tree = TargetActorQuadtree::new();
        for id in 1..=19 {
            let (x, z) = positions[(id as usize - 1) % 4];
            tree.insert(target_tree_unit(id), x, z, 0);
        }
        assert!(tree.root.children.is_none());
        tree.insert(target_tree_unit(20), positions[3].0, positions[3].1, 0);

        let children = tree.root.children.as_ref().unwrap();
        assert_eq!(
            children[0].elements,
            [17, 13, 9, 5, 1].map(target_tree_unit).to_vec()
        );
        assert_eq!(
            children[1].elements,
            [18, 14, 10, 6, 2].map(target_tree_unit).to_vec()
        );
        assert_eq!(
            children[2].elements,
            [19, 15, 11, 7, 3].map(target_tree_unit).to_vec()
        );
        assert_eq!(
            children[3].elements,
            [16, 12, 8, 4, 20].map(target_tree_unit).to_vec()
        );
    }

    #[test]
    fn target_quadtree_queries_parent_straddlers_before_children() {
        let mut tree = TargetActorQuadtree::new();
        tree.insert(target_tree_unit(1), 0, 0, 1_000);
        for id in 2..=20 {
            tree.insert(target_tree_unit(id), -100 * Q32_ONE, -100 * Q32_ONE, 0);
        }

        assert_eq!(tree.query_order().first(), Some(&target_tree_unit(1)));
    }

    #[test]
    fn target_quadtree_reinserts_a_moved_child_element_in_native_order() {
        let mut tree = TargetActorQuadtree::new();
        for id in 1..=20 {
            tree.insert(target_tree_unit(id), -100 * Q32_ONE, -100 * Q32_ONE, 0);
        }
        tree.position_changed(target_tree_unit(7), 100 * Q32_ONE, -100 * Q32_ONE, 0);

        let children = tree.root.children.as_ref().unwrap();
        assert!(!children[0].elements.contains(&target_tree_unit(7)));
        assert_eq!(children[1].elements.last(), Some(&target_tree_unit(7)));
    }

    fn test_placement(team: u32, formation_index: i32, world_x: i64, world_z: i64) -> Placement {
        Placement {
            team,
            unit_id: 0,
            formation_id: 0,
            formation_index,
            type_name: "arclight".to_owned(),
            world_x,
            world_z,
            rotation: if team == 0 { 0 } else { 180_000 },
        }
    }

    fn visible_velocity(actor: &Actor) -> (i64, i64) {
        let velocity = actor.snapshot().velocity;
        (velocity.x, velocity.z)
    }

    fn raw_test_simulation(
        layout: &CompiledLayout,
        config: &SimulationConfig,
        seed: i32,
    ) -> Simulation {
        let actors = initialize_actors(layout, &config.units, seed).unwrap();
        let buildings = initialize_buildings(&config.training_ground).unwrap();
        let target_quadtrees = initialize_target_quadtrees(&actors, &buildings);
        Simulation {
            actors,
            team_random: BTreeMap::new(),
            projectiles: Vec::new(),
            buildings,
            target_quadtrees,
            identities: IdentityAllocator::new(),
            rvo_counter: 0,
            rvo_solver_started: false,
            terminal_drain_pending: false,
        }
    }

    fn set_actor_position(actor: &mut Actor, x: i64, z: i64) {
        set_actor_position_q32(actor, space_to_q32(x), space_to_q32(z));
    }

    fn set_actor_position_q32(actor: &mut Actor, x_q32: i64, z_q32: i64) {
        actor.x_q32 = x_q32;
        actor.z_q32 = z_q32;
        actor.target_query_x_q32 = x_q32;
        actor.target_query_z_q32 = z_q32;
        actor.x = q32_to_space_rounded(x_q32);
        actor.z = q32_to_space_rounded(z_q32);
        actor.next_target_x_q32 = actor.x_q32;
        actor.next_target_z_q32 = actor.z_q32;
        actor.solver_target_x_q32 = actor.x_q32;
        actor.solver_target_z_q32 = actor.z_q32;
        actor.published_target_x_q32 = actor.x_q32;
        actor.published_target_z_q32 = actor.z_q32;
    }

    fn micrometers_to_q32(value: i64) -> i64 {
        i64::try_from(i128::from(value) * i128::from(Q32_ONE) / 1_000_000).unwrap()
    }

    #[test]
    fn native_delta_distinguishes_1799_from_1800_time_units() {
        assert_eq!(native_time_units_to_steps(1_799), 17);
        assert_eq!(native_time_units_to_steps(1_800), 18);
    }

    #[test]
    fn rotation_distance_uses_the_shortest_wrapped_arc() {
        assert_eq!(rotation_distance(359_000, 1_000), 2_000);
        assert_eq!(rotation_distance(1_000, 359_000), 2_000);
        assert_eq!(rotation_distance(20_000, 60_000), 40_000);
    }

    #[test]
    fn exact_half_turn_uses_the_native_positive_direction() {
        let maximum = 6_i64 << 32;
        assert_eq!(
            rotate_towards_q32_unwrapped(0, 180_i64 << 32, maximum),
            maximum
        );
        assert_eq!(
            rotate_towards_q32_unwrapped(0, (180_i64 << 32) + 43, maximum),
            maximum
        );
        assert_eq!(
            rotate_towards_q32_unwrapped(0, (180_i64 << 32) + 44, maximum),
            -maximum
        );
        assert_eq!(
            rotate_towards_q32_unwrapped(180_i64 << 32, 0, maximum),
            186_i64 << 32
        );
    }

    #[test]
    fn positive_body_rotation_exposes_the_exact_full_turn_for_one_tick() {
        let config = SimulationConfig::load(None).unwrap();
        let mut actor = Actor::new(
            test_placement(0, 0, 0, 0),
            config.units.get("crawler").unwrap().clone(),
            7,
        );
        actor.set_body_rotation(mdeg_to_degrees_q32(354_000));

        actor.rotate_body_towards(mdeg_to_degrees_q32(30_000));
        assert_eq!(degrees_q32_to_mdeg(actor.body_rotation_q32), 0);
        assert_eq!(actor.body_rotation, 360_000);

        actor.rotate_body_towards(mdeg_to_degrees_q32(30_000));
        assert_eq!(actor.body_rotation, 6_000);
    }

    #[test]
    fn normal_target_score_uses_strict_minimum_and_maximum_range_edges() {
        let distance_q32 = 20_i64 << 32;
        let score = |min_range_q32, max_range_q32| {
            normal_visible_full_rotation_score_from_distance_and_angle_q32(
                distance_q32,
                0,
                min_range_q32,
                max_range_q32,
            )
        };
        let at_both_edges = score(distance_q32, distance_q32).unwrap();
        assert!(score(distance_q32 + 1, distance_q32).is_none());
        assert_eq!(
            score(0, distance_q32 - 1).unwrap(),
            at_both_edges.saturating_add(TARGET_SCORE_OUT_OF_RANGE_PENALTY_Q32)
        );
    }

    #[test]
    fn normal_target_score_adds_raw_distance_after_weighted_term() {
        let distance_q32 = 20_i64 << 32;
        let angle_q32 = 50_i64 << 32;
        let angle_score_q32 = q32_mul(angle_q32, TARGET_SCORE_ANGLE_FACTOR_Q32);
        let expected_q32 = q32_mul(
            distance_q32,
            TARGET_SCORE_BASE_Q32.saturating_add(angle_score_q32),
        )
        .saturating_add(distance_q32);

        assert_eq!(
            normal_visible_full_rotation_score_from_distance_and_angle_q32(
                distance_q32,
                angle_q32,
                0,
                100_i64 << 32,
            ),
            Some(expected_q32)
        );
    }

    #[test]
    fn initial_identity_uses_seeded_snapshot_coordinates_not_layout_centers() {
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                test_placement(0, 0, -20, -100),
                test_placement(0, 1, 20, -100),
                test_placement(1, 0, 0, 100),
            ],
        };
        let config = SimulationConfig::load(None).unwrap();
        let actors = initialize_actors(&layout, &config.units, 1_787_591_883).unwrap();
        let first = &actors[&1];
        let second = &actors[&2];
        assert_eq!(first.placement.team, 0);
        assert_eq!(second.placement.team, 0);
        assert_eq!(first.placement.formation_index, 1);
        assert_eq!(second.placement.formation_index, 0);
        assert!((first.z, first.x) < (second.z, second.x));
        assert_eq!(actors[&3].placement.team, 1);
        assert_eq!(first.placement.formation_id, 1);
        assert_eq!(second.placement.formation_id, 2);
        assert_eq!(actors[&3].placement.formation_id, 3);
    }

    #[test]
    fn multi_formation_initial_state_and_target_search_entry_match_build_2259() {
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                Placement {
                    team: 0,
                    unit_id: 0,
                    formation_id: 0,
                    formation_index: 0,
                    type_name: "rhino".to_owned(),
                    world_x: -285,
                    world_z: -105,
                    rotation: 0,
                },
                Placement {
                    team: 1,
                    unit_id: 0,
                    formation_id: 0,
                    formation_index: 0,
                    type_name: "arclight".to_owned(),
                    world_x: -290,
                    world_z: 100,
                    rotation: 180_000,
                },
                Placement {
                    team: 1,
                    unit_id: 0,
                    formation_id: 0,
                    formation_index: 1,
                    type_name: "arclight".to_owned(),
                    world_x: -190,
                    world_z: 100,
                    rotation: 180_000,
                },
            ],
        };
        let config = SimulationConfig::load(None).unwrap();
        let actors = initialize_actors(&layout, &config.units, 1_787_601_811).unwrap();
        let actual = actors
            .iter()
            .map(|(&unit_id, actor)| {
                (
                    unit_id,
                    actor.placement.team,
                    actor.placement.formation_id,
                    actor.placement.type_name.as_str(),
                    actor.x,
                    actor.z,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            [
                (1, 0, 1, "rhino", -284_400, -104_900),
                (2, 1, 2, "arclight", -189_500, 99_300),
                (3, 1, 3, "arclight", -290_600, 99_900),
            ]
        );
        let make_simulation = || {
            let actors = actors.clone();
            let buildings = initialize_buildings(&config.training_ground).unwrap();
            let target_quadtrees = initialize_target_quadtrees(&actors, &buildings);
            Simulation {
                actors,
                team_random: BTreeMap::new(),
                projectiles: Vec::new(),
                buildings,
                target_quadtrees,
                identities: IdentityAllocator::new(),
                rvo_counter: 0,
                rvo_solver_started: false,
                terminal_drain_pending: false,
            }
        };
        let mut simulation = make_simulation();
        assert_eq!(simulation.select_normal_unit_target(1).unwrap(), Some(3));
        simulation.initialize_presearch_targets().unwrap();
        assert_eq!(simulation.actors[&1].mech_lock_target, Some(3));
        assert_eq!(simulation.actors[&1].body_rotation, 358_219);
        assert_eq!(simulation.actors[&1].fight_skill_search_target_time, 0);
        assert_eq!(simulation.actors[&2].fight_skill_search_target_time, 1);
        assert_eq!(simulation.actors[&3].fight_skill_search_target_time, 2);

        let mut fight_skill = make_simulation();
        fight_skill.initialize_presearch_targets().unwrap();
        let current = fight_skill.actors.get_mut(&3).unwrap();
        current.x = -100_000;
        current.z = 300_000;
        current.x_q32 = space_to_q32(current.x);
        current.z_q32 = space_to_q32(current.z);
        let source = fight_skill.actors.get_mut(&1).unwrap();
        source.fight_skill_search_target_time = 1;
        fight_skill.refresh_target_query_snapshot();
        let target_search_order = fight_skill.target_search_order();
        fight_skill
            .update_fight_skill_target_search(1, 0, &target_search_order)
            .unwrap();
        assert_eq!(fight_skill.actors[&1].mech_lock_target, Some(3));
        assert_eq!(fight_skill.actors[&1].fight_skill_search_target_time, 0);
        fight_skill
            .update_fight_skill_target_search(1, 1, &target_search_order)
            .unwrap();
        assert_eq!(fight_skill.actors[&1].mech_lock_target, Some(2));
        assert_eq!(
            fight_skill.actors[&1].fight_skill_search_target_time,
            SEARCH_TARGET_RESET_TICKS
        );

        let mut hold_fire = make_simulation();
        hold_fire.initialize_presearch_targets().unwrap();
        let current = hold_fire.actors.get_mut(&3).unwrap();
        current.x = -100_000;
        current.z = 300_000;
        current.x_q32 = space_to_q32(current.x);
        current.z_q32 = space_to_q32(current.z);
        let source = hold_fire.actors.get_mut(&1).unwrap();
        source.fight_skill_search_target_time = 0;
        source.motion_attack_hold_fire = true;
        hold_fire.refresh_target_query_snapshot();
        let target_search_order = hold_fire.target_search_order();
        hold_fire
            .update_fight_skill_target_search(1, 0, &target_search_order)
            .unwrap();
        assert_eq!(hold_fire.actors[&1].mech_lock_target, Some(2));
        assert_eq!(
            hold_fire.actors[&1].fight_skill_search_target_time,
            SEARCH_TARGET_RESET_TICKS
        );

        let mut attack_state = make_simulation();
        attack_state.initialize_presearch_targets().unwrap();
        let current = attack_state.actors.get_mut(&3).unwrap();
        current.x = -100_000;
        current.z = 300_000;
        current.x_q32 = space_to_q32(current.x);
        current.z_q32 = space_to_q32(current.z);
        let source = attack_state.actors.get_mut(&1).unwrap();
        source.fight_skill_search_target_time = 0;
        source.fight_skill_phase = FightSkillPhase::Attack;
        attack_state.refresh_target_query_snapshot();
        let target_search_order = attack_state.target_search_order();
        attack_state
            .update_fight_skill_target_search(1, 0, &target_search_order)
            .unwrap();
        assert_eq!(attack_state.actors[&1].mech_lock_target, Some(3));
        assert_eq!(attack_state.actors[&1].fight_skill_search_target_time, 0);

        let mut prepare_state = make_simulation();
        prepare_state.initialize_presearch_targets().unwrap();
        let current = prepare_state.actors.get_mut(&3).unwrap();
        current.x = -100_000;
        current.z = 300_000;
        current.x_q32 = space_to_q32(current.x);
        current.z_q32 = space_to_q32(current.z);
        let source = prepare_state.actors.get_mut(&1).unwrap();
        source.fight_skill_search_target_time = 0;
        source.fight_skill_phase = FightSkillPhase::Prepare { finish_step: 20 };
        source.pending = Some(PendingRelease {
            step: 20,
            target: 3,
        });
        prepare_state.refresh_target_query_snapshot();
        let target_search_order = prepare_state.target_search_order();
        prepare_state
            .update_fight_skill_target_search(1, 0, &target_search_order)
            .unwrap();
        assert_eq!(prepare_state.actors[&1].mech_lock_target, Some(3));
        assert_eq!(prepare_state.actors[&1].fight_skill_search_target_time, 0);

        let mut dead_target = make_simulation();
        dead_target.initialize_presearch_targets().unwrap();
        dead_target
            .actors
            .get_mut(&1)
            .unwrap()
            .fight_skill_search_target_time = 10;
        dead_target.actors.get_mut(&3).unwrap().life = 0;
        dead_target.step_actor(1, 0, &mut Vec::new()).unwrap();
        assert_eq!(dead_target.actors[&1].mech_lock_target, Some(2));
    }

    #[test]
    fn normal_selector_split_query_remains_order_independent_for_a_unique_best() {
        let config = SimulationConfig::load(None).unwrap();
        let mut placements = vec![Placement {
            team: 0,
            unit_id: 0,
            formation_id: 0,
            formation_index: 0,
            type_name: "rhino".to_owned(),
            world_x: 0,
            world_z: -100,
            rotation: 0,
        }];
        placements.extend((0_i32..18).map(|index| Placement {
            team: 1,
            unit_id: 0,
            formation_id: 0,
            formation_index: index,
            type_name: "arclight".to_owned(),
            world_x: i64::from(index) * 20 - 170,
            world_z: 100,
            rotation: 180_000,
        }));
        let layout = CompiledLayout {
            round: 1,
            placements,
        };
        let actors = initialize_actors(&layout, &config.units, 7).unwrap();
        let buildings = initialize_buildings(&config.training_ground).unwrap();
        let target_quadtrees = initialize_target_quadtrees(&actors, &buildings);
        let simulation = Simulation {
            actors,
            team_random: BTreeMap::new(),
            projectiles: Vec::new(),
            buildings,
            target_quadtrees,
            identities: IdentityAllocator::new(),
            rvo_counter: 0,
            rvo_solver_started: false,
            terminal_drain_pending: false,
        };
        assert_eq!(simulation.select_normal_unit_target(1).unwrap(), Some(10));
    }

    #[test]
    fn normal_selector_refuses_a_building_best_candidate() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 100)],
        };
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        let source = simulation.actors.get_mut(&1).unwrap();
        set_actor_position(source, 0, 0);
        source.set_body_rotation(0);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 100_000);
        let building = simulation
            .buildings
            .iter_mut()
            .find(|building| building.team_id == 1)
            .unwrap();
        building.position = point(0, 20_000);
        let error = simulation
            .select_normal_unit_target(1)
            .unwrap_err()
            .to_string();
        assert!(error.contains("selector chose building"));
    }

    #[test]
    fn normal_selector_keeps_the_first_native_quadtree_candidate_on_equal_score() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                test_placement(0, 0, 0, 0),
                test_placement(1, 0, -20, 100),
                test_placement(1, 1, 20, 100),
            ],
        };
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        let source = simulation.actors.get_mut(&1).unwrap();
        set_actor_position(source, 0, 0);
        source.set_body_rotation(0);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), -20_000, 100_000);
        set_actor_position(simulation.actors.get_mut(&3).unwrap(), 20_000, 100_000);
        assert_eq!(simulation.select_normal_unit_target(1).unwrap(), Some(2));
    }

    #[test]
    fn normal_selector_scores_the_start_of_tick_snapshot() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                test_placement(0, 0, 0, 0),
                test_placement(1, 0, 0, 100),
                test_placement(1, 1, 20, 100),
            ],
        };
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 20_000);
        set_actor_position(simulation.actors.get_mut(&3).unwrap(), 0, 40_000);
        assert_eq!(simulation.select_normal_unit_target(1).unwrap(), Some(2));

        let current = simulation.actors.get_mut(&2).unwrap();
        current.x_q32 = 0;
        current.z_q32 = space_to_q32(100_000);
        let current = simulation.actors.get_mut(&3).unwrap();
        current.x_q32 = 0;
        current.z_q32 = space_to_q32(10_000);

        assert_eq!(simulation.select_normal_unit_target(1).unwrap(), Some(2));
    }

    #[test]
    fn same_tick_target_death_scores_live_candidate_positions() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                test_placement(0, 0, 0, 0),
                test_placement(1, 0, 0, 20),
                test_placement(1, 1, 0, 40),
                test_placement(1, 2, 0, 60),
            ],
        };
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 20_000);
        set_actor_position(simulation.actors.get_mut(&3).unwrap(), 0, 40_000);
        set_actor_position(simulation.actors.get_mut(&4).unwrap(), 0, 60_000);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.mech_lock_target = Some(2);
        source.fight_skill_search_target_time = 10;
        simulation.refresh_target_query_snapshot();
        let target_search_order = simulation.target_search_order();

        simulation.actors.get_mut(&2).unwrap().life = 0;
        simulation.actors.get_mut(&3).unwrap().z_q32 = space_to_q32(100_000);
        simulation.actors.get_mut(&4).unwrap().z_q32 = space_to_q32(10_000);
        simulation
            .update_fight_skill_target_search(1, 1, &target_search_order)
            .unwrap();

        assert_eq!(simulation.actors[&1].mech_lock_target, Some(4));

        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 20_000);
        set_actor_position(simulation.actors.get_mut(&3).unwrap(), 0, 40_000);
        set_actor_position(simulation.actors.get_mut(&4).unwrap(), 0, 60_000);
        simulation.refresh_target_query_snapshot();
        let target_search_order = simulation.target_search_order();
        simulation.actors.get_mut(&2).unwrap().life = 0;
        simulation.actors.get_mut(&3).unwrap().z_q32 = space_to_q32(100_000);
        simulation.actors.get_mut(&4).unwrap().z_q32 = space_to_q32(10_000);
        simulation
            .update_fight_skill_target_search(1, 1, &target_search_order)
            .unwrap();

        assert_eq!(simulation.actors[&1].mech_lock_target, Some(4));
    }

    #[test]
    fn completed_bodyless_melee_attack_uses_one_idle_tick_before_reapproach() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 100)],
        };
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 100_000);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.rules = config.units.get("crawler").unwrap().clone();
        source.mech_lock_target = Some(2);
        source.motion = MotionState::Moving;
        source.backswing_finish_step = Some(10);
        source.set_body_rotation(mdeg_to_degrees_q32(123_000));

        simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

        let source = &simulation.actors[&1];
        assert_eq!(source.motion, MotionState::Idle);
        assert_eq!(source.mech_lock_target, None);
        assert_eq!(source.body_rotation, 123_000);
        assert_eq!((source.next_target_x_q32, source.next_target_z_q32), (0, 0));
    }

    #[test]
    fn completed_bodyless_melee_attack_uses_idle_before_an_out_of_angle_reentry() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 5)],
        };
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 5_000);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.rules = config.units.get("crawler").unwrap().clone();
        source.mech_lock_target = Some(2);
        source.motion = MotionState::Moving;
        source.fight_skill_phase = FightSkillPhase::Attack;
        source.backswing_finish_step = Some(10);
        source.set_body_rotation(mdeg_to_degrees_q32(90_000));

        simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

        let source = &simulation.actors[&1];
        assert_eq!(source.motion, MotionState::Idle);
        assert_eq!(source.mech_lock_target, None);
        assert_eq!(source.body_rotation, 90_000);
        assert_eq!((source.next_target_x_q32, source.next_target_z_q32), (0, 0));
    }

    #[test]
    fn allied_kill_during_backswing_retains_the_dead_target_until_finish() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                test_placement(0, 0, 0, 0),
                test_placement(0, 1, 0, 10),
                test_placement(1, 0, 0, 100),
                test_placement(1, 1, 0, 110),
            ],
        };
        for type_name in ["crawler", "wasp"] {
            let mut simulation = raw_test_simulation(&layout, &config, 7);
            let source = simulation.actors.get_mut(&1).unwrap();
            source.rules = config.units.get(type_name).unwrap().clone();
            source.mech_lock_target = Some(3);
            source.motion = MotionState::Attacking;
            source.fight_skill_phase = FightSkillPhase::Attack;
            source.backswing_finish_step = Some(10);
            source.next_attack_step = 20;
            simulation.actors.get_mut(&3).unwrap().life = 0;

            simulation.step_actor(1, 8, &mut Vec::new()).unwrap();
            let stop_target = {
                let source = &simulation.actors[&1];
                assert_eq!(source.motion, MotionState::Idle, "{type_name}");
                assert_eq!(source.mech_lock_target, Some(3), "{type_name}");
                assert_eq!(source.backswing_finish_step, Some(10), "{type_name}");
                (source.next_target_x_q32, source.next_target_z_q32)
            };
            simulation.actors.get_mut(&1).unwrap().x_q32 += Q32_ONE;

            for step in 9..=10 {
                simulation.step_actor(1, step, &mut Vec::new()).unwrap();
                let source = &simulation.actors[&1];
                assert_eq!(source.motion, MotionState::Idle, "{type_name}");
                assert_eq!(source.mech_lock_target, Some(3), "{type_name}");
                assert_eq!(source.backswing_finish_step, Some(10), "{type_name}");
                assert_eq!(
                    (source.next_target_x_q32, source.next_target_z_q32),
                    stop_target,
                    "{type_name}"
                );
            }

            simulation.step_actor(1, 11, &mut Vec::new()).unwrap();
            let source = &simulation.actors[&1];
            assert_eq!(source.motion, MotionState::Idle, "{type_name}");
            assert_eq!(source.mech_lock_target, None, "{type_name}");
            assert_eq!(source.backswing_finish_step, None, "{type_name}");
            assert_eq!(
                (source.next_target_x_q32, source.next_target_z_q32),
                stop_target,
                "{type_name}"
            );
        }
    }

    #[test]
    fn final_same_tick_allied_kill_retains_the_dead_backswing_target() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 20)],
        };
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.rules = config.units.get("crawler").unwrap().clone();
        source.mech_lock_target = Some(2);
        source.motion = MotionState::Attacking;
        source.fight_skill_phase = FightSkillPhase::Attack;
        source.backswing_finish_step = Some(20);
        simulation.refresh_target_query_snapshot();
        let target_search_order = simulation.target_search_order();
        simulation.actors.get_mut(&2).unwrap().life = 0;

        simulation
            .step_actor_with_target_order(1, 10, &target_search_order, &mut Vec::new())
            .unwrap();

        let source = &simulation.actors[&1];
        assert_eq!(source.motion, MotionState::Idle);
        assert_eq!(source.mech_lock_target, Some(2));
        assert_eq!(source.backswing_finish_step, Some(20));
    }

    #[test]
    fn later_final_enemy_death_does_not_clear_an_own_kill_backswing_target() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                test_placement(0, 0, 0, 0),
                test_placement(1, 0, 0, 20),
                test_placement(1, 1, 0, 100),
            ],
        };
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.rules = config.units.get("crawler").unwrap().clone();
        source.mech_lock_target = Some(2);
        source.motion = MotionState::Idle;
        source.fight_skill_phase = FightSkillPhase::Attack;
        source.retarget_after_own_direct_kill = true;
        source.backswing_finish_step = Some(20);
        simulation.actors.get_mut(&2).unwrap().life = 0;

        simulation.step_actor(1, 10, &mut Vec::new()).unwrap();
        assert_eq!(simulation.actors[&1].mech_lock_target, Some(2));

        simulation.actors.get_mut(&3).unwrap().life = 0;
        simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

        let source = &simulation.actors[&1];
        assert_eq!(source.motion, MotionState::Idle);
        assert_eq!(source.mech_lock_target, Some(2));
        assert_eq!(source.backswing_finish_step, Some(20));
    }

    #[test]
    fn bodyless_attack_defers_a_dead_target_replacement_outside_attack_area() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                test_placement(0, 0, 0, 0),
                test_placement(1, 0, 0, 20),
                test_placement(1, 1, 0, 100),
            ],
        };
        for type_name in ["crawler", "fang"] {
            let mut simulation = raw_test_simulation(&layout, &config, 7);
            let source = simulation.actors.get_mut(&1).unwrap();
            source.rules = config.units.get(type_name).unwrap().clone();
            source.mech_lock_target = Some(2);
            source.motion = MotionState::Attacking;
            source.fight_skill_phase = FightSkillPhase::Idle;
            source.motion_attack_hold_fire = false;
            source.next_attack_step = 20;
            simulation.actors.get_mut(&2).unwrap().life = 0;

            simulation.step_actor(1, 10, &mut Vec::new()).unwrap();
            let source = &simulation.actors[&1];
            assert_eq!(source.motion, MotionState::Idle, "{type_name}");
            assert_eq!(source.mech_lock_target, None, "{type_name}");
            assert_eq!(source.next_attack_step, 20, "{type_name}");

            simulation.step_actor(1, 11, &mut Vec::new()).unwrap();
            assert_eq!(simulation.actors[&1].mech_lock_target, Some(3), "{type_name}");
        }
    }

    #[test]
    fn bodyless_in_range_turn_barrier_preserves_attack_timing() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                test_placement(0, 0, 0, 0),
                test_placement(1, 0, 0, -50),
                test_placement(1, 1, 0, -40),
            ],
        };
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.rules = config.units.get("fang").unwrap().clone();
        source.mech_lock_target = Some(2);
        source.motion = MotionState::Attacking;
        source.fight_skill_phase = FightSkillPhase::Idle;
        source.next_attack_step = 20;
        simulation.actors.get_mut(&2).unwrap().life = 0;
        assert!(simulation.bodyless_target_in_attack_range(1, 3));
        assert!(!simulation.bodyless_target_in_attack_angle(1, 3));

        simulation.step_actor(1, 10, &mut Vec::new()).unwrap();

        let source = &simulation.actors[&1];
        assert_eq!(source.mech_lock_target, None);
        assert_eq!(source.motion, MotionState::Idle);
        assert_eq!(source.next_attack_step, 20);
    }

    #[test]
    fn bodyless_projectile_idle_entry_holds_for_turning() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![test_placement(0, 0, 0, 0), test_placement(1, 0, -20, 50)],
        };
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.rules = config.units.get("fang").unwrap().clone();
        source.mech_lock_target = Some(2);
        source.motion = MotionState::Idle;

        simulation.step_actor(1, 10, &mut Vec::new()).unwrap();

        let source = &simulation.actors[&1];
        assert_eq!(source.motion, MotionState::Attacking);
        assert!(source.motion_attack_hold_fire);
        assert_eq!(source.fight_skill_phase, FightSkillPhase::Idle);
    }

    #[test]
    fn bodyless_quick_switch_replaces_a_dead_target_immediately_in_range() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                test_placement(0, 0, 0, 0),
                test_placement(1, 0, 0, 20),
                test_placement(1, 1, 0, 40),
            ],
        };
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        simulation.team_random.insert(0, GrRandom::new(7));
        let source = simulation.actors.get_mut(&1).unwrap();
        source.rules = config.units.get("wasp").unwrap().clone();
        source.mech_lock_target = Some(2);
        source.motion = MotionState::Attacking;
        source.fight_skill_phase = FightSkillPhase::Attack;
        source.next_attack_step = 10;
        source.backswing_finish_step = Some(20);
        simulation.actors.get_mut(&2).unwrap().life = 0;

        simulation.step_actor(1, 10, &mut Vec::new()).unwrap();

        assert_eq!(simulation.actors[&1].mech_lock_target, Some(2));
        assert_eq!(simulation.actors[&1].backswing_finish_step, Some(20));

        let mut events = Vec::new();
        simulation.step_actor(1, 11, &mut events).unwrap();

        assert_eq!(simulation.actors[&1].mech_lock_target, Some(3));
        assert_eq!(simulation.actors[&1].backswing_finish_step, Some(41));
        assert!(events.iter().any(|event| {
            matches!(event.payload, EventPayload::ProjectileReleased)
        }));
    }

    #[test]
    fn bodyless_non_quick_switch_defers_an_in_range_dead_target_replacement() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                test_placement(0, 0, 0, 0),
                test_placement(1, 0, 0, 2),
                test_placement(1, 1, 0, 4),
            ],
        };
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.rules = config.units.get("crawler").unwrap().clone();
        source.mech_lock_target = Some(2);
        source.motion = MotionState::Attacking;
        source.fight_skill_phase = FightSkillPhase::Idle;
        simulation.actors.get_mut(&2).unwrap().life = 0;

        simulation.step_actor(1, 10, &mut Vec::new()).unwrap();
        assert_eq!(simulation.actors[&1].mech_lock_target, None);

        simulation.step_actor(1, 11, &mut Vec::new()).unwrap();
        assert_eq!(simulation.actors[&1].mech_lock_target, Some(3));
    }

    #[test]
    fn bodyless_melee_attack_motion_exits_through_idle_when_target_leaves_range() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 100)],
        };
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 100_000);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.rules = config.units.get("crawler").unwrap().clone();
        source.mech_lock_target = Some(2);
        source.motion = MotionState::Attacking;
        source.set_body_rotation(mdeg_to_degrees_q32(123_000));

        simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

        let source = &simulation.actors[&1];
        assert_eq!(source.motion, MotionState::Idle);
        assert_eq!(source.mech_lock_target, None);
        assert_eq!(source.body_rotation, 123_000);
        assert_eq!((source.next_target_x_q32, source.next_target_z_q32), (0, 0));
    }

    #[test]
    fn bodyless_projectile_attack_motion_exits_through_idle_when_target_leaves_range() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 100)],
        };
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 100_000);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.rules = config.units.get("fang").unwrap().clone();
        source.mech_lock_target = Some(2);
        source.motion = MotionState::Attacking;
        source.fight_skill_phase = FightSkillPhase::Attack;
        source.set_body_rotation(mdeg_to_degrees_q32(123_000));

        simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

        let source = &simulation.actors[&1];
        assert_eq!(source.motion, MotionState::Idle);
        assert_eq!(source.mech_lock_target, None);
        assert_eq!(source.fight_skill_phase, FightSkillPhase::Idle);
        assert_eq!(source.body_rotation, 123_000);
        assert_eq!((source.next_target_x_q32, source.next_target_z_q32), (0, 0));
    }

    #[test]
    fn bodyless_melee_retains_target_while_motion_attack_is_held() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 100)],
        };
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 100_000);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.rules = config.units.get("crawler").unwrap().clone();
        source.mech_lock_target = Some(2);
        source.motion = MotionState::Attacking;
        source.motion_attack_hold_fire = true;
        source.set_body_rotation(mdeg_to_degrees_q32(123_000));

        simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

        let source = &simulation.actors[&1];
        assert_eq!(source.motion, MotionState::Moving);
        assert_eq!(source.mech_lock_target, Some(2));
        assert_eq!(source.body_rotation, 123_000);
    }

    #[test]
    fn rejected_bodyless_melee_active_attack_reopens_idle_search_before_release() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 100)],
        };
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 100_000);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.rules = config.units.get("crawler").unwrap().clone();
        source.mech_lock_target = Some(2);
        source.motion = MotionState::Attacking;
        source.pending = Some(PendingRelease {
            step: 12,
            target: 2,
        });
        source.fight_skill_phase = FightSkillPhase::Attack;

        simulation.step_actor(1, 11, &mut Vec::new()).unwrap();
        let source = &simulation.actors[&1];
        assert_eq!(source.motion, MotionState::Idle);
        assert_eq!(source.mech_lock_target, None);
        assert_eq!(source.fight_skill_phase, FightSkillPhase::Idle);

        simulation.step_actor(1, 12, &mut Vec::new()).unwrap();
        let source = &simulation.actors[&1];
        assert_eq!(source.motion, MotionState::Moving);
        assert_eq!(source.mech_lock_target, Some(2));
    }

    #[test]
    fn bodyless_attack_cancels_a_pending_attack_when_an_ally_kills_its_target() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                test_placement(0, 0, 0, 0),
                test_placement(1, 0, 0, 100),
                test_placement(1, 1, 20, 100),
            ],
        };
        for type_name in ["crawler", "fang"] {
            let mut simulation = raw_test_simulation(&layout, &config, 7);
            let source = simulation.actors.get_mut(&1).unwrap();
            source.rules = config.units.get(type_name).unwrap().clone();
            source.mech_lock_target = Some(2);
            source.motion = MotionState::Attacking;
            source.pending = Some(PendingRelease {
                step: 12,
                target: 2,
            });
            source.fight_skill_phase = FightSkillPhase::Attack;
            simulation.actors.get_mut(&2).unwrap().life = 0;

            let mut events = Vec::new();
            simulation.step_actor(1, 11, &mut events).unwrap();
            let source = &simulation.actors[&1];
            assert_eq!(source.motion, MotionState::Idle, "{type_name}");
            assert_eq!(source.mech_lock_target, None, "{type_name}");
            assert_eq!(source.fight_skill_phase, FightSkillPhase::Idle, "{type_name}");
            assert!(source.pending.is_none(), "{type_name}");
            assert!(events.is_empty(), "{type_name}");

            simulation.step_actor(1, 12, &mut events).unwrap();
            assert_eq!(simulation.actors[&1].mech_lock_target, Some(3), "{type_name}");
            assert!(events.is_empty(), "{type_name}");
        }
    }

    #[test]
    fn bodyless_quick_switch_retargets_a_pending_attack_in_attack_area() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                test_placement(0, 0, 0, 0),
                test_placement(1, 0, 0, 20),
                test_placement(1, 1, 0, 40),
            ],
        };
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.rules = config.units.get("fang").unwrap().clone();
        source.mech_lock_target = Some(2);
        source.motion = MotionState::Attacking;
        source.pending = Some(PendingRelease {
            step: 12,
            target: 2,
        });
        source.fight_skill_phase = FightSkillPhase::Attack;
        simulation.actors.get_mut(&2).unwrap().life = 0;

        simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

        let source = &simulation.actors[&1];
        assert_eq!(source.motion, MotionState::Attacking);
        assert_eq!(source.mech_lock_target, Some(3));
        assert_eq!(source.pending.unwrap().target, 3);
    }

    #[test]
    fn direct_splash_applies_one_aggregate_event_to_secondary_targets() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                Placement {
                    type_name: "rhino".to_owned(),
                    ..test_placement(0, 0, 0, 0)
                },
                test_placement(1, 0, 0, 20),
                test_placement(1, 1, 1, 20),
            ],
        };
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 20_000);
        set_actor_position(simulation.actors.get_mut(&3).unwrap(), 1_000, 20_000);
        let mut events = Vec::new();
        simulation.direct_effect(1, 2, &mut events).unwrap();
        assert_eq!(
            [simulation.actors[&2].life, simulation.actors[&3].life],
            [1_253, 1_253]
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].target, Some(ObjectRef::new(ObjectKind::Unit, 2)));
        assert_eq!(events[0].payload, EventPayload::Damage { amount: 7_120 });
    }

    #[test]
    fn zero_radius_direct_attack_does_not_damage_an_overlapping_secondary_target() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                test_placement(0, 0, 0, 0),
                test_placement(1, 0, 0, 20),
                test_placement(1, 1, 1, 20),
            ],
        };
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        simulation.actors.get_mut(&1).unwrap().rules = config.units.get("crawler").unwrap().clone();
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 20_000);
        set_actor_position(simulation.actors.get_mut(&3).unwrap(), 1_000, 20_000);
        let primary_life = simulation.actors[&2].life;
        let secondary_life = simulation.actors[&3].life;

        let mut events = Vec::new();
        simulation.direct_effect(1, 2, &mut events).unwrap();

        assert_eq!(simulation.actors[&2].life, primary_life - 79);
        assert_eq!(simulation.actors[&3].life, secondary_life);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].target, Some(ObjectRef::new(ObjectKind::Unit, 2)));
        assert_eq!(events[0].payload, EventPayload::Damage { amount: 79 });
    }

    #[test]
    fn direct_splash_refuses_a_building_before_unit_damage() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                Placement {
                    type_name: "rhino".to_owned(),
                    ..test_placement(0, 0, 0, 0)
                },
                test_placement(1, 0, 0, 20),
            ],
        };
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 20_000);
        let building = simulation
            .buildings
            .iter_mut()
            .find(|building| building.team_id == 1)
            .unwrap();
        building.position = point(1_000, 20_000);
        let previous_life = simulation.actors[&2].life;
        let error = simulation
            .direct_effect(1, 2, &mut Vec::new())
            .unwrap_err()
            .to_string();
        assert!(error.contains("direct splash against a building"));
        assert_eq!(simulation.actors[&2].life, previous_life);
    }

    #[test]
    fn projectile_splash_refuses_secondary_targets_before_damage() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                test_placement(0, 0, 0, 0),
                Placement {
                    type_name: "rhino".to_owned(),
                    ..test_placement(1, 0, 0, 20)
                },
                Placement {
                    type_name: "rhino".to_owned(),
                    ..test_placement(1, 1, 1, 20)
                },
            ],
        };
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 20_000);
        set_actor_position(simulation.actors.get_mut(&3).unwrap(), 1_000, 20_000);
        let projectile = Projectile {
            id: 1,
            team: 0,
            owner: 1,
            target: 2,
            x: 0,
            y: 0,
            z: 20_000,
            x_q32: 0,
            z_q32: space_to_q32(20_000),
            cached_target_x: 0,
            cached_target_y: 0,
            cached_target_z: 20_000,
            cached_target_x_q32: 0,
            cached_target_z_q32: space_to_q32(20_000),
            cached_target_radius: simulation.actors[&2].rules.collision_radius(),
            speed: simulation.actors[&1].rules.attack.projectile_speed(),
            damage: simulation.actors[&1].rules.attack.base_damage,
        };
        let previous_life = [simulation.actors[&2].life, simulation.actors[&3].life];
        let error = simulation
            .impact(&projectile, &mut Vec::new())
            .unwrap_err()
            .to_string();
        assert!(error.contains("projectile splash with a secondary target"));
        assert_eq!(
            [simulation.actors[&2].life, simulation.actors[&3].life],
            previous_life
        );
    }

    #[test]
    fn projectile_splash_refuses_a_secondary_building_before_damage() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                test_placement(0, 0, 0, 0),
                Placement {
                    type_name: "rhino".to_owned(),
                    ..test_placement(1, 0, 0, 20)
                },
            ],
        };
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 20_000);
        let building = simulation
            .buildings
            .iter_mut()
            .find(|building| building.team_id == 1)
            .unwrap();
        building.position = point(1_000, 20_000);
        let projectile = Projectile {
            id: 1,
            team: 0,
            owner: 1,
            target: 2,
            x: 0,
            y: 0,
            z: 20_000,
            x_q32: 0,
            z_q32: space_to_q32(20_000),
            cached_target_x: 0,
            cached_target_y: 0,
            cached_target_z: 20_000,
            cached_target_x_q32: 0,
            cached_target_z_q32: space_to_q32(20_000),
            cached_target_radius: simulation.actors[&2].rules.collision_radius(),
            speed: simulation.actors[&1].rules.attack.projectile_speed(),
            damage: simulation.actors[&1].rules.attack.base_damage,
        };
        let previous_life = simulation.actors[&2].life;
        let error = simulation
            .impact(&projectile, &mut Vec::new())
            .unwrap_err()
            .to_string();
        assert!(error.contains("projectile splash with a secondary target"));
        assert_eq!(simulation.actors[&2].life, previous_life);
    }

    #[test]
    fn rvo_solves_a_collision_building_inside_the_influence_bound() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                Placement {
                    type_name: "rhino".to_owned(),
                    ..test_placement(0, 0, 0, 0)
                },
                test_placement(1, 0, 0, 100),
            ],
        };
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 100_000);
        let building = simulation.buildings.first_mut().unwrap();
        building.position = point(20_000, 0);
        let target_position = (simulation.actors[&2].x_q32, simulation.actors[&2].z_q32);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.mech_lock_target = Some(2);
        source.motion = MotionState::Moving;
        source.next_target_x_q32 = target_position.0;
        source.next_target_z_q32 = target_position.1;
        source.next_speed_q32 = space_to_q32(source.rules.move_speed());
        source.next_max_speed_q32 = source.next_speed_q32;
        simulation.rvo_solver_started = true;
        simulation.rvo_counter = 3;
        simulation.step_rvo().unwrap();
        assert!(simulation.actors[&1].solver_speed_q32 > 0);
        assert_ne!(simulation.actors[&1].solver_target_z_q32, 0);
    }

    #[test]
    fn rvo_q32_boundary_uses_raw_distance_not_snapshot_rounding() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                Placement {
                    type_name: "rhino".to_owned(),
                    ..test_placement(0, 0, 0, 0)
                },
                test_placement(1, 0, 0, 100),
                test_placement(1, 1, 1, 68),
            ],
        };
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position_q32(
            simulation.actors.get_mut(&1).unwrap(),
            micrometers_to_q32(499),
            micrometers_to_q32(499),
        );
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 100_000);
        set_actor_position_q32(
            simulation.actors.get_mut(&3).unwrap(),
            micrometers_to_q32(1_106_501),
            micrometers_to_q32(67_991_503),
        );
        assert_eq!(
            magnitude(
                simulation.actors[&3]
                    .x
                    .saturating_sub(simulation.actors[&1].x),
                simulation.actors[&3]
                    .z
                    .saturating_sub(simulation.actors[&1].z),
            ),
            68_001
        );
        assert!(q32_distance_within(
            simulation.actors[&1].x_q32,
            simulation.actors[&1].z_q32,
            simulation.actors[&3].x_q32,
            simulation.actors[&3].z_q32,
            68_000,
        ));
        let target_position = (simulation.actors[&2].x_q32, simulation.actors[&2].z_q32);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.mech_lock_target = Some(2);
        source.motion = MotionState::Moving;
        source.next_target_x_q32 = target_position.0;
        source.next_target_z_q32 = target_position.1;
        source.next_speed_q32 = space_to_q32(source.rules.move_speed());
        source.next_max_speed_q32 = source.next_speed_q32;
        simulation.rvo_solver_started = true;
        simulation.rvo_counter = 3;
        simulation.step_rvo().unwrap();
        assert!(simulation.actors[&1].solver_speed_q32 > 0);
        assert_ne!(simulation.actors[&1].solver_target_z_q32, 0);
    }

    #[test]
    fn rvo_allows_a_coarse_tree_hit_outside_candidate_relative_travel() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                Placement {
                    type_name: "rhino".to_owned(),
                    ..test_placement(0, 0, 0, 0)
                },
                test_placement(1, 0, 0, 100),
                test_placement(1, 1, 75, 0),
            ],
        };
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 100_000);
        set_actor_position(simulation.actors.get_mut(&3).unwrap(), 75_000, 0);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.mech_lock_target = Some(2);
        source.motion = MotionState::Moving;
        simulation.rvo_counter = 3;
        simulation.step_rvo().unwrap();
    }

    #[test]
    fn reviewed_direct_kill_keeps_then_clears_the_mech_lock_target_state() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                Placement {
                    team: 0,
                    unit_id: 0,
                    formation_id: 0,
                    formation_index: 0,
                    type_name: "rhino".to_owned(),
                    world_x: -285,
                    world_z: -105,
                    rotation: 0,
                },
                Placement {
                    team: 1,
                    unit_id: 0,
                    formation_id: 0,
                    formation_index: 0,
                    type_name: "arclight".to_owned(),
                    world_x: -290,
                    world_z: 100,
                    rotation: 180_000,
                },
                Placement {
                    team: 1,
                    unit_id: 0,
                    formation_id: 0,
                    formation_index: 1,
                    type_name: "arclight".to_owned(),
                    world_x: -190,
                    world_z: 100,
                    rotation: 180_000,
                },
            ],
        };
        let mut simulation = Simulation::new(
            &layout,
            &config.units,
            &config.training_ground,
            1_787_601_811,
        )
        .unwrap();
        for output_tick in 1..=234 {
            simulation.step(output_tick - 1).unwrap();
            match output_tick {
                223 => {
                    assert_eq!(simulation.actors[&1].mech_lock_target, Some(3));
                    assert!(simulation.actors[&1].retarget_after_own_direct_kill);
                    assert_eq!(simulation.actors[&1].motion, MotionState::Idle);
                }
                224..=232 => {
                    assert_eq!(simulation.actors[&1].mech_lock_target, Some(3));
                    assert!(simulation.actors[&1].retarget_after_own_direct_kill);
                }
                233 => {
                    assert_eq!(simulation.actors[&1].mech_lock_target, None);
                    assert!(!simulation.actors[&1].retarget_after_own_direct_kill);
                    assert_eq!(simulation.actors[&1].motion, MotionState::Idle);
                }
                234 => {
                    // The exact native assignment point is not observable. This only
                    // locks the simulator-private state needed to reproduce S/E tick 234.
                    assert_eq!(simulation.actors[&1].mech_lock_target, Some(2));
                    assert_eq!(simulation.actors[&1].motion, MotionState::Moving);
                }
                _ => {}
            }
        }
    }

    #[test]
    fn crawler_member_grid_and_jitter_follow_native_creation_order() {
        let config = SimulationConfig::load(None).unwrap();
        let rules = config.units.get("crawler").unwrap();
        let placement = Placement {
            team: 0,
            unit_id: 0,
            formation_id: 0,
            formation_index: 0,
            type_name: "crawler".to_owned(),
            world_x: 0,
            world_z: 0,
            rotation: 0,
        };
        let seed = 1_787_601_811;
        let positions = generate_formation_positions(&placement, rules, seed).unwrap();
        assert_eq!(positions.len(), 24);

        let mut random = GrRandom::new(i64::from(seed).cast_unsigned());
        let mut base_positions = Vec::new();
        for (x_q32, z_q32) in &positions {
            let jitter_x = i64::from(random.next_in_range(FORMATION_JITTER_RANGE_TENTHS))
                .saturating_mul(C0_1_RAW);
            let jitter_z = i64::from(random.next_in_range(FORMATION_JITTER_RANGE_TENTHS))
                .saturating_mul(C0_1_RAW);
            base_positions.push(((x_q32 - jitter_x) >> 32, (z_q32 - jitter_z) >> 32));
        }
        let row = [
            (-21, 7),
            (-15, 7),
            (-9, 7),
            (-3, 7),
            (3, 7),
            (9, 7),
            (15, 7),
            (21, 7),
        ];
        let expected = row
            .into_iter()
            .chain(row.map(|(x, _)| (x, 1)))
            .chain(row.map(|(x, _)| (x, -5)))
            .collect::<Vec<_>>();
        assert_eq!(base_positions, expected);

        let mut red = placement;
        red.team = 1;
        red.rotation = 180_000;
        let red_positions = generate_formation_positions(&red, rules, seed).unwrap();
        assert!(
            positions
                .iter()
                .zip(red_positions)
                .all(|(&(blue_x, blue_z), (red_x, red_z))| {
                    (red_x, red_z) == (-blue_x, -blue_z)
                })
        );
    }

    #[test]
    fn hound_partial_last_row_preserves_native_q32_centering() {
        let config = SimulationConfig::load(None).unwrap();
        let rules = config.units.get("hound").unwrap();
        let placement = Placement {
            team: 0,
            unit_id: 0,
            formation_id: 0,
            formation_index: 0,
            type_name: "hound".to_owned(),
            world_x: 0,
            world_z: 0,
            rotation: 0,
        };
        let seed = 1_787_601_811;
        let positions = generate_formation_positions(&placement, rules, seed).unwrap();
        let mut random = GrRandom::new(i64::from(seed).cast_unsigned());
        let base_positions_q32 = positions
            .into_iter()
            .map(|(x_q32, z_q32)| {
                let jitter_x = i64::from(random.next_in_range(FORMATION_JITTER_RANGE_TENTHS))
                    .saturating_mul(C0_1_RAW);
                let jitter_z = i64::from(random.next_in_range(FORMATION_JITTER_RANGE_TENTHS))
                    .saturating_mul(C0_1_RAW);
                (x_q32 - jitter_x, z_q32 - jitter_z)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            base_positions_q32,
            [
                (-13_i64 << 32, 5_i64 << 32),
                (0, 5_i64 << 32),
                (13_i64 << 32, 5_i64 << 32),
                (-(13_i64 << 31), -5_i64 << 32),
                (13_i64 << 31, -5_i64 << 32),
            ]
        );
    }

    #[test]
    fn multi_member_identity_is_assigned_after_generation_and_shared_by_formation() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![Placement {
                team: 0,
                unit_id: 0,
                formation_id: 0,
                formation_index: 0,
                type_name: "crawler".to_owned(),
                world_x: 5,
                world_z: -50,
                rotation: 0,
            }],
        };
        let actors = initialize_actors(&layout, &config.units, 1_787_601_811).unwrap();
        assert_eq!(actors.len(), 24);
        assert!(
            actors
                .iter()
                .all(|(&unit_id, actor)| unit_id == actor.placement.unit_id
                    && actor.placement.formation_id == 1
                    && actor.placement.formation_index == 0)
        );
        assert!(actors.values().map(|actor| (actor.z, actor.x)).is_sorted());
    }

    #[test]
    fn negative_formation_seed_is_sign_extended_like_the_native_constructor() {
        let config = SimulationConfig::load(None).unwrap();
        let rules = config.units.get("arclight").unwrap().clone();
        let actor = Actor::new(test_placement(0, 0, 0, 0), rules, -1);

        let mut native_random = GrRandom::new(i64::from(-1_i32).cast_unsigned());
        let expected_x = i64::from(native_random.next_in_range(FORMATION_JITTER_RANGE_TENTHS))
            .saturating_mul(C0_1_RAW);
        let expected_z = i64::from(native_random.next_in_range(FORMATION_JITTER_RANGE_TENTHS))
            .saturating_mul(C0_1_RAW);
        let mut zero_extended = GrRandom::new(u64::from((-1_i32).cast_unsigned()));
        let wrong_x = i64::from(zero_extended.next_in_range(FORMATION_JITTER_RANGE_TENTHS))
            .saturating_mul(C0_1_RAW);
        let wrong_z = i64::from(zero_extended.next_in_range(FORMATION_JITTER_RANGE_TENTHS))
            .saturating_mul(C0_1_RAW);

        assert_eq!((actor.x_q32, actor.z_q32), (expected_x, expected_z));
        assert_ne!((expected_x, expected_z), (wrong_x, wrong_z));
    }

    #[test]
    fn formation_seed_addition_wraps_at_the_native_i32_boundary() {
        let config = SimulationConfig::load(None).unwrap();
        let rules = config.units.get("arclight").unwrap().clone();
        let actor = Actor::new(test_placement(0, 1, 0, 0), rules, i32::MAX);

        let mut native_random = GrRandom::new(i64::from(i32::MIN).cast_unsigned());
        let expected_x = i64::from(native_random.next_in_range(FORMATION_JITTER_RANGE_TENTHS))
            .saturating_mul(C0_1_RAW);
        let expected_z = i64::from(native_random.next_in_range(FORMATION_JITTER_RANGE_TENTHS))
            .saturating_mul(C0_1_RAW);
        assert_eq!((actor.x_q32, actor.z_q32), (expected_x, expected_z));
    }

    #[test]
    fn rhino_attack_angle_requires_every_weapon_and_accepts_the_boundary() {
        let config = SimulationConfig::load(None).unwrap();
        let rules = config.units.get("rhino").unwrap().clone();
        let mut actor = Actor::new(
            Placement {
                team: 0,
                unit_id: 1,
                formation_id: 1,
                formation_index: 0,
                type_name: "rhino".to_owned(),
                world_x: 0,
                world_z: 0,
                rotation: 0,
            },
            rules,
            0,
        );
        let target = 0;
        actor.weapon_rotations_q32 = vec![0, 41_i64 << 32];
        assert!(!actor.weapons_in_attack_angle(target));
        actor.weapon_rotations_q32[1] = 40_i64 << 32;
        assert!(actor.weapons_in_attack_angle(target));
    }

    #[test]
    fn rhino_backswing_remains_active_through_its_ninth_wait_update() {
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                Placement {
                    team: 0,
                    unit_id: 1,
                    formation_id: 1,
                    formation_index: 0,
                    type_name: "rhino".to_owned(),
                    world_x: -35,
                    world_z: -105,
                    rotation: 0,
                },
                Placement {
                    team: 1,
                    unit_id: 2,
                    formation_id: 2,
                    formation_index: 0,
                    type_name: "arclight".to_owned(),
                    world_x: 40,
                    world_z: 100,
                    rotation: 180_000,
                },
            ],
        };
        let config = SimulationConfig::load(None).unwrap();
        let mut simulation = Simulation::new(
            &layout,
            &config.units,
            &config.training_ground,
            1_787_591_883,
        )
        .unwrap();

        for step in 0..226 {
            simulation.step(step).unwrap();
            let output_tick = step + 1;
            let rhino = &simulation.actors[&1];
            match output_tick {
                216 => assert_eq!(rhino.backswing_finish_step, Some(224)),
                225 => assert_eq!(rhino.backswing_finish_step, Some(224)),
                226 => {
                    assert_eq!(rhino.backswing_finish_step, None);
                    assert_eq!(rhino.pending.unwrap().step, 233);
                }
                _ => {}
            }
        }
    }

    #[test]
    fn native_fastest_angle_quantizes_small_jitter_to_forward() {
        assert_eq!(direction_mdeg(-600, 99_200), 0);
        assert_eq!(direction_mdeg(600, -99_200), 180_000);
        assert_eq!(direction_mdeg(1_000, 151_400), 853);
    }

    #[test]
    fn raw_q32_velocity_preserves_arclight_target_angle_precision() {
        assert_eq!(
            direction_mdeg_q32_raw(-198_556_428, -30_061_443_202),
            180_812
        );
        assert_eq!(direction_mdeg(-46, -6_999), 180_811);
    }

    #[test]
    fn q32_normalized_velocity_matches_frozen_arclight_delta() {
        let speed = space_to_q32(7_000);
        let reconstructed =
            normalized_velocity_q32_raw(space_to_q32(-1_000), space_to_q32(-151_400), speed);
        assert_eq!(reconstructed, (-198_556_428, -30_061_443_202));
        assert_eq!(
            (
                q32_to_space_rounded(reconstructed.0),
                q32_to_space_rounded(reconstructed.1),
            ),
            (-46, -6_999)
        );

        let c0_1 = 0x1999_9999;
        let native_raw = normalized_velocity_q32_raw(-10 * c0_1, -150 * Q32_ONE - 14 * c0_1, speed);
        assert_eq!(native_raw, reconstructed);
    }

    #[test]
    fn q32_clamp_magnitude_stops_at_a_near_target_point() {
        let dx = space_to_q32(100);
        let dz = space_to_q32(-50);
        let speed = space_to_q32(7_123);
        let maximum = q32_mul(speed, NATIVE_LOGIC_DELTA_Q32);

        assert!(native_q32_magnitude(dx, dz) < maximum);
        assert_eq!(clamp_magnitude_q32_raw(dx, dz, maximum), (dx, dz));

        let far_dx = space_to_q32(1_000);
        let far_dz = space_to_q32(-10_000);
        let magnitude = native_q32_magnitude(far_dx, far_dz);
        let reciprocal = q32_div(Q32_ONE, magnitude);
        let native_order = (
            q32_mul(q32_mul(far_dx, reciprocal), maximum),
            q32_mul(q32_mul(far_dz, reciprocal), maximum),
        );
        let old_grouping = (
            q32_mul(
                q32_mul(q32_mul(far_dx, reciprocal), speed),
                NATIVE_LOGIC_DELTA_Q32,
            ),
            q32_mul(
                q32_mul(q32_mul(far_dz, reciprocal), speed),
                NATIVE_LOGIC_DELTA_Q32,
            ),
        );
        assert_ne!(native_order, old_grouping);
        assert_eq!(
            clamp_magnitude_q32_raw(far_dx, far_dz, maximum),
            native_order
        );
    }

    #[test]
    fn q32_clamp_magnitude_preserves_a_tolerance_equal_zero_speed_delta() {
        // FPoint's comparison treats this squared magnitude (28 raw) as
        // equal to zero, so FVector2.ClampMagnitude returns the input delta.
        assert_eq!(
            clamp_magnitude_q32_raw(43_007, -347_649, 0),
            (43_007, -347_649)
        );
        assert_eq!(clamp_magnitude_q32_raw(0, Q32_ONE, 0), (0, 0));
    }

    #[test]
    fn zero_published_speed_still_snaps_a_tolerance_equal_rvo_delta() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                test_placement(0, 0, 0, 0),
                test_placement(1, 0, 0, 100),
            ],
        };
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        let actor = simulation.actors.get_mut(&1).unwrap();
        actor.x_q32 = 1_717_060_204_994;
        actor.z_q32 = 1_696_431_970_300;
        actor.published_target_x_q32 = 1_717_060_248_001;
        actor.published_target_z_q32 = 1_696_431_622_651;
        actor.published_speed_q32 = 0;
        actor.mech_lock_target = Some(2);
        actor.backswing_finish_step = Some(10);
        simulation.actors.get_mut(&2).unwrap().life = 0;

        simulation.step_actor_rvo_position(1);

        let actor = &simulation.actors[&1];
        assert_eq!(actor.x_q32, actor.published_target_x_q32);
        assert_eq!(actor.z_q32, actor.published_target_z_q32);

        let actor = simulation.actors.get_mut(&1).unwrap();
        assert!(actor.rvo_stopped_snap_since_boundary);
        actor.motion = MotionState::Moving;
        actor.next_speed_q32 = space_to_q32(actor.rules.move_speed());
        actor.next_max_speed_q32 = actor.next_speed_q32;
        actor.solver_target_x_q32 = 1_717_060_204_994;
        actor.solver_target_z_q32 = 1_696_431_970_300;
        actor.solver_speed_q32 = 0;
        simulation.rvo_solver_started = true;
        simulation.rvo_counter = 3;

        simulation.step_rvo().unwrap();

        let actor = &simulation.actors[&1];
        assert_eq!(actor.published_target_x_q32, actor.x_q32);
        assert_eq!(actor.published_target_z_q32, actor.z_q32);
        assert!(!actor.rvo_stopped_snap_since_boundary);
    }

    #[test]
    fn stopped_snap_reset_expires_on_a_moving_tick_before_the_rvo_boundary() {
        let config = SimulationConfig::load(None).unwrap();
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                test_placement(0, 0, 0, 0),
                test_placement(1, 0, 0, 100),
            ],
        };
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        let actor = simulation.actors.get_mut(&1).unwrap();
        actor.motion = MotionState::Moving;
        actor.rvo_stopped_snap_since_boundary = true;
        actor.published_target_x_q32 = actor.x_q32;
        actor.published_target_z_q32 = actor.z_q32;
        actor.published_speed_q32 = 0;
        simulation.rvo_counter = 1;

        simulation.step_actor_rvo_position(1);

        assert!(!simulation.actors[&1].rvo_stopped_snap_since_boundary);
    }

    #[test]
    fn snapshot_velocity_is_quantized_from_raw_agent_velocity() {
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                Placement {
                    team: 0,
                    unit_id: 1,
                    formation_id: 1,
                    formation_index: 0,
                    type_name: "marksman".to_owned(),
                    world_x: 0,
                    world_z: -50,
                    rotation: 0,
                },
                Placement {
                    team: 1,
                    unit_id: 2,
                    formation_id: 2,
                    formation_index: 0,
                    type_name: "arclight".to_owned(),
                    world_x: 0,
                    world_z: 100,
                    rotation: 180_000,
                },
            ],
        };
        let config = SimulationConfig::load(None).unwrap();
        let mut simulation = Simulation::new(
            &layout,
            &config.units,
            &config.training_ground,
            1_787_555_163,
        )
        .unwrap();
        let actor = simulation.actors.get_mut(&1).unwrap();
        actor.current_velocity_x_q32 = -198_556_428;
        actor.current_velocity_z_q32 = -30_061_443_202;

        assert_eq!(visible_velocity(actor), (-46, -6_999));
    }

    #[test]
    fn deployment_raw_alone_still_rounds_tick_twenty_two_up() {
        let initial_z_q32 = 100 * Q32_ONE + 7 * C0_1_RAW;
        let fixed_delta_z_q32 = q32_mul(-30_061_443_202, NATIVE_LOGIC_DELTA_Q32);
        let deploy_only_z_q32 = initial_z_q32 + 14 * fixed_delta_z_q32;

        assert_eq!(initial_z_q32, 432_503_206_703);
        assert_eq!(deploy_only_z_q32, 411_460_196_533);
        assert_eq!(q32_to_space_rounded(deploy_only_z_q32), 95_801);
    }

    #[test]
    fn deployment_raw_and_per_tick_target_direction_round_tick_twenty_two_down() {
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                Placement {
                    team: 0,
                    unit_id: 1,
                    formation_id: 1,
                    formation_index: 0,
                    type_name: "marksman".to_owned(),
                    world_x: 0,
                    world_z: -50,
                    rotation: 0,
                },
                Placement {
                    team: 1,
                    unit_id: 2,
                    formation_id: 2,
                    formation_index: 0,
                    type_name: "arclight".to_owned(),
                    world_x: 0,
                    world_z: 100,
                    rotation: 180_000,
                },
            ],
        };
        let config = SimulationConfig::load(None).unwrap();
        let mut simulation = Simulation::new(
            &layout,
            &config.units,
            &config.training_ground,
            1_787_555_163,
        )
        .unwrap();

        assert_eq!(
            (
                simulation.actors[&1].x_q32,
                simulation.actors[&1].z_q32,
                simulation.actors[&2].x_q32,
                simulation.actors[&2].z_q32,
            ),
            (
                -2_147_483_645,
                -217_754_841_903,
                2_147_483_645,
                432_503_206_703,
            )
        );

        for step in 0..22 {
            simulation.step(step).unwrap();
        }

        let arclight = &simulation.actors[&2];
        assert_eq!(arclight.z_q32, 411_459_840_709);
        assert_eq!(arclight.z, 95_800);
    }

    #[test]
    fn rvo_pipeline_publishes_before_movement_consumes_velocity() {
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                Placement {
                    team: 0,
                    unit_id: 1,
                    formation_id: 1,
                    formation_index: 0,
                    type_name: "marksman".to_owned(),
                    world_x: 0,
                    world_z: -50,
                    rotation: 0,
                },
                Placement {
                    team: 1,
                    unit_id: 2,
                    formation_id: 2,
                    formation_index: 0,
                    type_name: "arclight".to_owned(),
                    world_x: 0,
                    world_z: 100,
                    rotation: 180_000,
                },
            ],
        };
        let config = SimulationConfig::load(None).unwrap();
        let mut simulation = Simulation::new(
            &layout,
            &config.units,
            &config.training_ground,
            1_787_555_163,
        )
        .unwrap();
        let initial = simulation.actors[&2].clone();

        for tick in 1..=9 {
            simulation.step(tick - 1).unwrap();
            let arclight = &simulation.actors[&2];
            if tick <= 7 {
                assert_eq!((arclight.x, arclight.z), (initial.x, initial.z));
                assert_eq!(arclight.body_rotation, initial.body_rotation);
                assert_eq!(visible_velocity(arclight), (0, 0));
            } else if tick == 8 {
                assert_eq!((arclight.x, arclight.z), (initial.x, initial.z));
                assert_eq!(arclight.body_rotation, initial.body_rotation);
                assert_eq!(visible_velocity(arclight), (-46, -6_999));
                assert_eq!(
                    (
                        arclight.current_velocity_x_q32,
                        arclight.current_velocity_z_q32,
                    ),
                    (-198_556_428, -30_061_443_202)
                );
            } else {
                assert_eq!((arclight.x, arclight.z), (498, 100_350));
                assert_eq!(
                    (arclight.x_q32, arclight.z_q32),
                    (2_137_555_823, 431_000_134_548)
                );
                assert_ne!(arclight.body_rotation, initial.body_rotation);
            }
        }
    }

    #[test]
    fn rvo_boundary_recalculates_velocity_from_the_published_target_and_current_position() {
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                Placement {
                    team: 0,
                    unit_id: 1,
                    formation_id: 1,
                    formation_index: 0,
                    type_name: "marksman".to_owned(),
                    world_x: 0,
                    world_z: -50,
                    rotation: 0,
                },
                Placement {
                    team: 1,
                    unit_id: 2,
                    formation_id: 2,
                    formation_index: 0,
                    type_name: "arclight".to_owned(),
                    world_x: 0,
                    world_z: 100,
                    rotation: 180_000,
                },
            ],
        };
        let config = SimulationConfig::load(None).unwrap();
        let mut simulation = Simulation::new(
            &layout,
            &config.units,
            &config.training_ground,
            1_787_555_163,
        )
        .unwrap();

        for step in 0..4 {
            simulation.step(step).unwrap();
        }
        let arclight = &simulation.actors[&2];
        let previous_boundary_candidate = normalized_velocity_q32_raw(
            arclight.solver_target_x_q32.saturating_sub(arclight.x_q32),
            arclight.solver_target_z_q32.saturating_sub(arclight.z_q32),
            arclight.solver_speed_q32,
        );
        let arclight = simulation.actors.get_mut(&2).unwrap();
        arclight.x_q32 = arclight.x_q32.saturating_add(10 * Q32_ONE);
        arclight.x = q32_to_space_rounded(arclight.x_q32);

        for step in 4..8 {
            simulation.step(step).unwrap();
        }
        let arclight = &simulation.actors[&2];
        let recalculated = normalized_velocity_q32_raw(
            arclight
                .published_target_x_q32
                .saturating_sub(arclight.x_q32),
            arclight
                .published_target_z_q32
                .saturating_sub(arclight.z_q32),
            arclight.published_speed_q32,
        );

        assert_eq!(
            (
                arclight.current_velocity_x_q32,
                arclight.current_velocity_z_q32,
            ),
            recalculated
        );
        assert_ne!(previous_boundary_candidate, recalculated);
    }

    #[test]
    fn range_entry_stops_only_after_the_two_stage_rvo_delay() {
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                Placement {
                    team: 0,
                    unit_id: 1,
                    formation_id: 1,
                    formation_index: 0,
                    type_name: "marksman".to_owned(),
                    world_x: 0,
                    world_z: -50,
                    rotation: 0,
                },
                Placement {
                    team: 1,
                    unit_id: 2,
                    formation_id: 2,
                    formation_index: 0,
                    type_name: "arclight".to_owned(),
                    world_x: 0,
                    world_z: 100,
                    rotation: 180_000,
                },
            ],
        };
        let config = SimulationConfig::load(None).unwrap();
        let mut simulation = Simulation::new(
            &layout,
            &config.units,
            &config.training_ground,
            1_787_555_163,
        )
        .unwrap();
        let mut tick_121_raw_position = None;
        let mut tick_121_body_rotation = None;

        for tick in 1..=128 {
            simulation.step(tick - 1).unwrap();
            let arclight = &simulation.actors[&2];
            match tick {
                120 => {
                    assert_eq!(arclight.z, 61_500);
                    assert_eq!(arclight.motion, MotionState::Moving);
                }
                121 => {
                    assert_eq!(arclight.z, 61_150);
                    assert_eq!(arclight.motion, MotionState::Moving);
                    assert_eq!(visible_velocity(arclight), (-46, -6_999));
                    tick_121_raw_position = Some((arclight.x_q32, arclight.z_q32));
                    tick_121_body_rotation = Some(arclight.body_rotation);
                }
                122 => {
                    assert_eq!(arclight.z, 60_800);
                    assert_eq!(arclight.motion, MotionState::Attacking);
                    assert_eq!(visible_velocity(arclight), (-46, -6_999));
                    assert_eq!(
                        (arclight.next_target_x_q32, arclight.next_target_z_q32),
                        tick_121_raw_position.unwrap()
                    );
                    assert_eq!(arclight.next_speed_q32, 0);
                    assert_eq!(arclight.body_rotation, tick_121_body_rotation.unwrap());
                    assert!(arclight.pending.is_none());
                }
                123..=127 => {
                    let expected_z = 60_800 - i64::try_from(tick - 122).unwrap() * 350;
                    assert_eq!(arclight.z, expected_z);
                    assert_eq!(arclight.motion, MotionState::Attacking);
                    assert_eq!(visible_velocity(arclight), (-46, -6_999));
                    if tick == 124 {
                        assert_eq!(arclight.published_speed_q32, space_to_q32(7_000));
                        assert_eq!(arclight.solver_speed_q32, 0);
                    }
                }
                128 => {
                    assert_eq!(arclight.z, 58_700);
                    assert_eq!(arclight.motion, MotionState::Attacking);
                    assert_eq!(visible_velocity(arclight), (0, 0));
                    assert_eq!(arclight.published_speed_q32, 0);
                }
                _ => {}
            }
        }
    }

    #[test]
    fn tick_fifteen_aim_uses_raw_q32_positions() {
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                Placement {
                    team: 0,
                    unit_id: 1,
                    formation_id: 1,
                    formation_index: 0,
                    type_name: "marksman".to_owned(),
                    world_x: 0,
                    world_z: -50,
                    rotation: 0,
                },
                Placement {
                    team: 1,
                    unit_id: 2,
                    formation_id: 2,
                    formation_index: 0,
                    type_name: "arclight".to_owned(),
                    world_x: 0,
                    world_z: 100,
                    rotation: 180_000,
                },
            ],
        };
        let config = SimulationConfig::load(None).unwrap();
        let mut simulation = Simulation::new(
            &layout,
            &config.units,
            &config.training_ground,
            1_787_555_163,
        )
        .unwrap();

        for step in 0..14 {
            simulation.step(step).unwrap();
        }

        let marksman = &simulation.actors[&1];
        let arclight = &simulation.actors[&2];
        let raw_dx = arclight.x_q32.saturating_sub(marksman.x_q32);
        let raw_dz = arclight.z_q32.saturating_sub(marksman.z_q32);
        assert_eq!((raw_dx, raw_dz), (4_235_400_048, 641_239_568_913));
        assert_eq!(direction_mdeg_q32_raw(raw_dx, raw_dz), 798);
        assert_eq!(direction_mdeg_q32_raw(-raw_dx, -raw_dz), 180_798);

        let mm_dx = arclight.x - marksman.x;
        let mm_dz = arclight.z - marksman.z;
        assert_eq!((mm_dx, mm_dz), (986, 149_300));
        assert_eq!(direction_mdeg(mm_dx, mm_dz), 797);
        assert_eq!(direction_mdeg(-mm_dx, -mm_dz), 180_797);

        simulation.step(14).unwrap();
        assert_eq!(simulation.actors[&1].aim_rotation, 798);
        assert_eq!(simulation.actors[&2].aim_rotation, 180_798);
    }

    #[test]
    fn projectile_raw_target_cache_preserves_rounding_sequence() {
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                Placement {
                    team: 0,
                    unit_id: 1,
                    formation_id: 1,
                    formation_index: 0,
                    type_name: "marksman".to_owned(),
                    world_x: 0,
                    world_z: -50,
                    rotation: 0,
                },
                Placement {
                    team: 1,
                    unit_id: 2,
                    formation_id: 2,
                    formation_index: 0,
                    type_name: "arclight".to_owned(),
                    world_x: 0,
                    world_z: 100,
                    rotation: 180_000,
                },
            ],
        };
        let config = SimulationConfig::load(None).unwrap();
        let mut simulation = Simulation::new(
            &layout,
            &config.units,
            &config.training_ground,
            1_787_555_163,
        )
        .unwrap();
        let mut millimeter_path = None;
        let mut raw_x = Vec::new();
        let mut millimeter_x = Vec::new();

        for tick in 1..=18 {
            simulation.step(tick - 1).unwrap();
            if tick < 14 {
                continue;
            }

            let projectile = &simulation.projectiles[0];
            let owner = &simulation.actors[&projectile.owner];
            let target = &simulation.actors[&projectile.target];
            let (x_q32, z_q32) = millimeter_path
                .get_or_insert_with(|| (space_to_q32(owner.x), space_to_q32(owner.z)));
            let dx_q32 = space_to_q32(target.x).saturating_sub(*x_q32);
            let dz_q32 = space_to_q32(target.z).saturating_sub(*z_q32);
            let distance_q32 = native_q32_magnitude(dx_q32, dz_q32);
            let step_q32 = q32_mul(space_to_q32(projectile.speed), NATIVE_LOGIC_DELTA_Q32);
            let move_q32 = step_q32.min(distance_q32);
            let reciprocal = q32_div(Q32_ONE, distance_q32);
            *x_q32 = x_q32.saturating_add(q32_mul(q32_mul(dx_q32, reciprocal), move_q32));
            *z_q32 = z_q32.saturating_add(q32_mul(q32_mul(dz_q32, reciprocal), move_q32));

            raw_x.push(projectile.x);
            millimeter_x.push(q32_to_space_rounded(*x_q32));
        }

        assert_eq!(raw_x, [-335, -170, -5, 160, 326]);
        assert_eq!(millimeter_x, [-335, -170, -4, 161, 326]);
    }

    #[test]
    fn stopped_attacker_tracks_target_with_aim_without_rotating_root_body() {
        let layout = CompiledLayout {
            round: 1,
            placements: vec![
                Placement {
                    team: 0,
                    unit_id: 1,
                    formation_id: 1,
                    formation_index: 0,
                    type_name: "marksman".to_owned(),
                    world_x: 0,
                    world_z: -50,
                    rotation: 0,
                },
                Placement {
                    team: 1,
                    unit_id: 2,
                    formation_id: 2,
                    formation_index: 0,
                    type_name: "arclight".to_owned(),
                    world_x: 0,
                    world_z: -100,
                    rotation: 180_000,
                },
            ],
        };
        let config = SimulationConfig::load(None).unwrap();
        let mut simulation = Simulation::new(
            &layout,
            &config.units,
            &config.training_ground,
            1_787_555_163,
        )
        .unwrap();
        let marksman = simulation.actors.get_mut(&1).unwrap();
        assert_eq!(marksman.rules.independent_aim, Some(false));
        marksman.motion = MotionState::Attacking;
        marksman.next_attack_step = u64::MAX;

        simulation.step_actor(1, 0, &mut Vec::new()).unwrap();
        let root_body = simulation.actors[&1].body_rotation;
        let initial_aim = simulation.actors[&1].aim_rotation;

        let target = simulation.actors.get_mut(&2).unwrap();
        target.x += 10_000;
        target.x_q32 = space_to_q32(target.x);
        let expected_aim = direction_mdeg_q32_raw(
            simulation.actors[&2]
                .x_q32
                .saturating_sub(simulation.actors[&1].x_q32),
            simulation.actors[&2]
                .z_q32
                .saturating_sub(simulation.actors[&1].z_q32),
        );
        simulation.step_actor(1, 1, &mut Vec::new()).unwrap();
        let marksman = &simulation.actors[&1];

        assert_eq!(marksman.motion, MotionState::Attacking);
        assert_eq!(marksman.body_rotation, root_body);
        assert_ne!(marksman.aim_rotation, initial_aim);
        assert_eq!(marksman.aim_rotation, expected_aim);
    }
}
