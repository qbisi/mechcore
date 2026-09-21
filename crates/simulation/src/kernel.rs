// Paired fixed-point components are named `x_q32` / `z_q32` throughout, which
// `similar_names` flags on every coordinate pair.
#![allow(clippy::similar_names)]

use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Instant,
};

use mechcore_mcfr::{
    BuffModifierSet, BuildingState, DerivedStats, Domain, DurableContext, Event, EventPayload,
    GaugeI32, Hashes, IdentityAllocator, LiveUnitState, McfrReader, McfrWriter, MotionState,
    ObjectKind, ObjectRef, PersonalShieldState, ProjectileState, QVec3, Rational, TickSlice,
    TransitionEvents, UnitDynamicModifierSet, Visibility, WeaponAimState, WorldSnapshot,
};
use serde::Serialize;

use crate::{
    Error, Result,
    constructions::ConstructionBuilding,
    layout::{CompiledLayout, Placement},
    random::GrRandom,
    rules::{
        AttackPath, AttackTargets, RvoSize, SimulationConfig, TrainingGroundConfig, UnitConfig,
        UnitConfigs, UnitDomain, WeaponMode,
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
const C0_01_RAW: i64 = 0x028F_5C28;
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
/// How far off the line of fire an enemy construction may stand and still take
/// the shot, in space units.
///
/// `docs/rules/constructions.md` carries the measurement: 94 decisions over
/// six unit types leave it in `[10.8, 11.8]` metres, and a Steel Ball of
/// `wall-laser.yaml`, closing on block 3 a few centimetres a tick, narrows it
/// to `[11.447, 11.507)` — it passes the block by at 11.507 for nine ticks and
/// takes it the tick after 11.447. 11.5 is the value inside that. It is not
/// the attacker's: a Fang of radius 2, a Marksman of 8, a Steel Ball of 6 and
/// a Wraith of 11 use the same one.
const WALL_IN_THE_WAY_WIDTH: i64 = 11_500;

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
    let layer_index = (collider_priority * 2 - 2).cast_unsigned();
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
    target: FightActorRef,
}

#[derive(Debug, Clone, Copy)]
struct PendingProjectileRelease {
    step: u64,
    target_kind: ObjectKind,
    target: u64,
    target_x_q32: i64,
    target_z_q32: i64,
    weapon_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FightSkillPhase {
    Idle,
    Prepare { finish_step: u64 },
    Attack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum FightActorRef {
    Unit(u64),
    Building(u64),
}

impl FightActorRef {
    const fn id(self) -> u64 {
        match self {
            Self::Unit(id) | Self::Building(id) => id,
        }
    }

    const fn kind(self) -> ObjectKind {
        match self {
            Self::Unit(_) => ObjectKind::Unit,
            Self::Building(_) => ObjectKind::Building,
        }
    }

    const fn object_ref(self) -> ObjectRef {
        ObjectRef::new(self.kind(), self.id())
    }

    const fn unit_id(self) -> Option<u64> {
        match self {
            Self::Unit(id) => Some(id),
            Self::Building(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct FightActorView {
    team: u32,
    x_q32: i64,
    z_q32: i64,
    query_x_q32: i64,
    query_z_q32: i64,
    radius: i64,
    alive: bool,
    query_alive: bool,
    targetable: bool,
    domain: UnitDomain,
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
    elements: Vec<FightActorRef>,
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
        candidate: FightActorRef,
        ranges: &BTreeMap<FightActorRef, TargetActorRect>,
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

    fn find_path(&self, candidate: FightActorRef, path: &mut Vec<usize>) -> bool {
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

    fn append_query_order(&self, output: &mut Vec<FightActorRef>) {
        output.extend(self.elements.iter().copied());
        if let Some(children) = &self.children {
            for child in children {
                child.append_query_order(output);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TargetActorQuadtree {
    root: TargetActorQuadtreeNode,
    ranges: BTreeMap<FightActorRef, TargetActorRect>,
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

    fn insert(&mut self, candidate: FightActorRef, x_q32: i64, z_q32: i64, radius: i64) {
        let range = TargetActorRect::around(x_q32, z_q32, radius);
        self.ranges.insert(candidate, range);
        if self.root.rect.contains(range) {
            self.root.insert(candidate, &self.ranges);
        } else {
            self.root.elements.push(candidate);
        }
    }

    fn position_changed(&mut self, candidate: FightActorRef, x_q32: i64, z_q32: i64, radius: i64) {
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

    fn query_order(&self) -> Vec<FightActorRef> {
        let mut output = Vec::with_capacity(self.ranges.len());
        self.root.append_query_order(&mut output);
        output
    }
}

#[derive(Debug, Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the actor mirrors the native fight actor's independent state flags"
)]
struct Actor {
    placement: Placement,
    rules: UnitConfig,
    /// The numbers the fight reads, which are the description corrected by the
    /// overlays. `crate::data` is the layer; nothing here reads `rules` for a
    /// number this carries.
    stats: crate::data::Stats,
    x: i64,
    z: i64,
    x_q32: i64,
    z_q32: i64,
    target_query_x_q32: i64,
    target_query_z_q32: i64,
    target_query_source_rotation_q32: i64,
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
    last_damage_source: Option<(ObjectRef, u32)>,
    motion: MotionState,
    next_attack_step: u64,
    /// The interval this cycle was scheduled with, in logic ticks: the
    /// description plus the stagger drawn for it. The build keeps the same
    /// thing in `FightSkill.attackInterval` and answers it from
    /// `GetCurrentAttackInterval`, and a recording carries it so the two can
    /// be compared. Before a unit's first attack it is the description with
    /// the draw its deployment took.
    current_attack_interval: u64,
    motion_attack_hold_fire: bool,
    /// What the mech's body is directed at: the target its search found, which
    /// it moves toward and which a unit with a body keeps facing while it
    /// attacks. A recording carries it as `mech_lock_target`.
    ///
    /// It is not always what the weapons fire at: [`Actor::attack_target`] is,
    /// and the two part company when an enemy construction stands in the line
    /// of fire.
    lock_target: Option<FightActorRef>,
    /// The enemy construction in the line of fire, and the lock it was found
    /// for.
    ///
    /// `FightSkill.SearchAttackTarget` asks `WallConstructionTargetChecker`
    /// every tick the skill is idle and hands the block to the weapons while
    /// the mech keeps its lock. The pairing is what keeps this honest: once
    /// `lock_target` is anything but the lock it was found for, the block no
    /// longer answers, without anyone having to clear it.
    in_the_way: Option<(u64, FightActorRef)>,
    /// The construction whose fall ended this actor's attack, which its
    /// weapon still names until a new target is taken.
    ///
    /// The game reads a unit whose block has just fallen as idle, with no
    /// lock, and with its weapon still aimed at the block: for one tick where
    /// the next target is in reach at once, for as long as it takes otherwise.
    /// Only the snapshot reads this; nothing aims or fires at a fallen block.
    fallen_attack_target: Option<u64>,
    /// The construction this actor has launched an attack at since it last
    /// took a lock.
    ///
    /// Only a unit that attacked a block has an attack on it to end when it
    /// falls. Two Crawlers of `wall-block.yaml` closing on block 4 without yet
    /// striking it go straight on to the Marksman behind the wall when a third
    /// fells it, with no idle tick, where the one that felled it idles for one.
    attacked_wall: Option<u64>,
    /// The last step of the cooling that follows a shot, and the target the
    /// skill quick-switched to if its own died during it.
    ///
    /// A Marksman whose shot kills its target reads idle with no lock until
    /// its cooling is over, its weapon already on the unit the selector
    /// answers the tick after the kill; one tick after the cooling ends the
    /// weapon clears, and the lock is searched the tick after that. Both
    /// Marksmen that were recorded killing with enemies left — in
    /// `crawlers-vs-marksman.yaml` and `wall-passage.yaml` — do exactly this.
    cooling_until_step: Option<u64>,
    cooling_candidate: Option<FightActorRef>,
    /// Whether this actor is being held through its cooling, which only a
    /// target dying during it starts.
    cooling_hold: Option<u64>,
    lock_is_terminal_handoff: bool,
    fight_skill_search_target_time: i32,
    fight_skill_searched_this_tick: bool,
    fight_skill_phase: FightSkillPhase,
    group_skill_targets: Vec<Option<u64>>,
    /// The enemy construction in each grouped slot's line of fire, with the
    /// unit that slot was allocated.
    ///
    /// The same pairing as [`Actor::in_the_way`], slot by slot: a Wraith's four
    /// slots each take the block standing between it and the unit they were
    /// given, and a slot given another unit no longer answers with it.
    group_in_the_way: Vec<Option<(u64, u64)>>,
    group_skill_next_attack_steps: Vec<u64>,
    group_skill_prepare_ready_steps: Vec<u64>,
    group_pending_releases: Vec<(usize, PendingRelease)>,
    projectile_pending_releases: Vec<PendingProjectileRelease>,
    projectile_burst_finished: bool,
    projectile_burst_finished_same_tick_dead: bool,
    laser_attack_count: usize,
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

    /// Gives this actor another description, and recomputes what the fight
    /// reads from it.
    ///
    /// The build never swaps a description; a test does, and a derived number
    /// that kept the old one would be a cache telling a lie.
    #[cfg(test)]
    fn describe(&mut self, rules: UnitConfig) {
        self.stats = crate::data::Stats::of(&rules).expect("an uncorrected description resolves");
        self.rules = rules;
    }

    fn at_generated_position(
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
            fallen_attack_target: None,
            attacked_wall: None,
            cooling_until_step: None,
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

    fn alive(&self) -> bool {
        self.life > 0
    }

    /// What this actor's weapons fire at: the construction in the way if one
    /// stands there for the current lock, and the lock itself otherwise.
    ///
    /// Range, attack angle, release and the question of whether the target
    /// is still alive are all asked of this. Where to move and where a body
    /// faces are asked of `lock_target`.
    fn attack_target(&self) -> Option<FightActorRef> {
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
    fn drop_lock(&mut self) {
        self.lock_target = None;
        self.fallen_attack_target = None;
        self.attacked_wall = None;
        self.group_skill_targets.fill(None);
        self.group_in_the_way.fill(None);
        self.group_skill_next_attack_steps.fill(0);
        self.group_skill_prepare_ready_steps.fill(0);
        self.group_pending_releases.clear();
    }

    /// What one grouped slot fires at: the construction in its way if one
    /// stands there for the unit it was allocated, and that unit otherwise.
    fn group_attack_target(&self, slot: usize) -> Option<FightActorRef> {
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
    fn mechanical_attack_target(&self) -> Option<FightActorRef> {
        self.group_attack_target(0)
            .or_else(|| {
                (0..self.group_skill_targets.len())
                    .rev()
                    .find_map(|slot| self.group_attack_target(slot))
            })
            .or(self.attack_target())
    }

    fn mechanical_lock_target(&self) -> Option<FightActorRef> {
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

    fn exit_fight_on_death(&mut self) {
        self.motion = MotionState::Idle;
        self.pending = None;
        self.lock_target = None;
        self.fallen_attack_target = None;
        self.attacked_wall = None;
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
        let unwrapped = rotate_towards_q32_unwrapped(self.body_rotation_q32, target_q32, maximum);
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

    fn snapshot(&self) -> LiveUnitState {
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
                        self.fallen_attack_target
                            .filter(|_| self.lock_target.is_none())
                            .map(FightActorRef::Building)
                            .or(self
                                .cooling_candidate
                                .filter(|_| self.lock_target.is_none()))
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

fn generate_formation_positions(
    placement: &Placement,
    rules: &UnitConfig,
    seed: i32,
) -> Result<Vec<(i64, i64)>> {
    let members = i64::from(rules.formation.members);
    let (width, depth) = rules.formation_footprint_meters()?;
    let (width, depth) = if placement.rotated {
        (depth, width)
    } else {
        (width, depth)
    };
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
        let formation_id = if let Some(id) = formation_ids.get(&formation_key) {
            *id
        } else {
            let id = identities.allocate_formation()?;
            formation_ids.insert(formation_key, id);
            id
        };
        actor.placement.unit_id = unit_id;
        actor.placement.formation_id = formation_id;
        actors.insert(unit_id, actor);
    }
    Ok(actors)
}

/// Every building the fight starts with: the map's own, and the ones this
/// layout's constructions place.
///
/// Identity is the capture's: a building's id is its place in `(team, type,
/// x, z)` order over both sources together, one-based, which is how a
/// recording numbers them and therefore the only numbering the two backends
/// can be compared under.
fn initialize_buildings(
    training_ground: &TrainingGroundConfig,
    constructions: &[ConstructionBuilding],
) -> Result<InitialBuildings> {
    let mut raw = training_ground
        .buildings
        .iter()
        .map(|building| RawBuilding {
            team_id: building.team_id,
            building_type_id: building.building_type_id,
            x: building.x(),
            z: building.z(),
            radius: building.radius(),
            life: building.life,
            collision_enabled: building.collision_enabled,
            searchable: true,
            collider_priority: None,
        })
        .collect::<Vec<_>>();
    raw.extend(constructions.iter().map(|building| RawBuilding {
        team_id: building.team,
        building_type_id: building.building_type_id,
        x: building.x,
        z: building.z,
        radius: building.radius,
        life: i64::from(building.life),
        // `BuildingData.EnableCollision` as the capture reads it, which is
        // true for a construction as it is for a tower. Whether the object is
        // an obstacle is a different question, and [`rvo_collides`] answers
        // it.
        collision_enabled: true,
        searchable: building.searchable,
        collider_priority: Some(building.collider_priority),
    }));

    let building_key = |building: &RawBuilding| {
        (
            building.team_id,
            building.building_type_id,
            building.x,
            building.z,
        )
    };
    let mut ordered = raw.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|building| building_key(building));
    let mut normalized_ids = BTreeMap::new();
    for (index, building) in ordered.into_iter().enumerate() {
        let id = u64::try_from(index)
            .map_err(|_| Error::new("building index overflow"))?
            .saturating_add(1);
        if normalized_ids.insert(building_key(building), id).is_some() {
            return Err(Error::new("two buildings stand in the same place"));
        }
    }
    let unsearchable = raw
        .iter()
        .filter(|building| !building.searchable)
        .map(|building| normalized_ids[&building_key(building)])
        .collect::<BTreeSet<_>>();
    let colliders = raw
        .iter()
        .filter_map(|building| {
            building
                .collider_priority
                .map(|priority| (normalized_ids[&building_key(building)], priority))
        })
        .collect::<BTreeMap<_, _>>();
    let states = raw
        .iter()
        .map(|building| {
            let building_id = normalized_ids[&building_key(building)];
            Ok(BuildingState {
                building_id,
                team_id: building.team_id,
                building_type_id: building.building_type_id,
                position: point(building.x, building.z),
                bounds_width: space_to_q32(building.radius.saturating_mul(2)),
                bounds_height: space_to_q32(building.radius.saturating_mul(2)),
                life: GaugeI32 {
                    current: i32::try_from(building.life)
                        .map_err(|_| Error::new("building life exceeds i32"))?,
                    maximum: i32::try_from(building.life)
                        .map_err(|_| Error::new("building life exceeds i32"))?,
                },
                available: true,
                targetable: building.life > 0,
                collision_enabled: building.collision_enabled,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(InitialBuildings {
        states,
        unsearchable,
        colliders,
    })
}

/// Every building a fight starts with, and what a unit may do about each.
struct InitialBuildings {
    states: Vec<BuildingState>,
    /// The ones a unit looking for a target may not find.
    unsearchable: BTreeSet<u64>,
    /// Each construction's RVO collider priority.
    colliders: BTreeMap<u64, i32>,
}

/// One building before it is given an identity, from either source.
#[derive(Debug, Clone, Copy)]
struct RawBuilding {
    team_id: u32,
    building_type_id: u32,
    x: i64,
    z: i64,
    radius: i64,
    life: i64,
    collision_enabled: bool,
    searchable: bool,
    /// A construction's `pathfinding_collider_priority`; none for a tower.
    collider_priority: Option<i32>,
}

/// Whether a building takes part in RVO as a tower, which is not the same as
/// whether its data says collision is enabled.
///
/// The map's towers push every unit around. A construction takes part too,
/// but on its own collider layer and only for the other side, which
/// [`Simulation::construction_colliders`] carries: every construction is
/// `BuildingType.Special` and only the map's own two towers are anything
/// else, so the type is what separates them here.
const fn rvo_collides(building: &BuildingState) -> bool {
    building.collision_enabled && building.building_type_id != CONSTRUCTION_BUILDING_TYPE
}

/// `GameRiver.BuildingType.Special`.
const CONSTRUCTION_BUILDING_TYPE: u32 = 3;

/// The trees a unit looks for a target in.
///
/// A building nobody searches for is left out of them rather than scored and
/// rejected: a Defensive Wall answers `IsEnableSearchTarget` with false, and
/// the game's Crawlers lock onto the unit behind one at tick one.
fn initialize_target_quadtrees(
    actors: &BTreeMap<u64, Actor>,
    buildings: &[BuildingState],
    unsearchable: &BTreeSet<u64>,
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
            if unsearchable.contains(&building.building_id) {
                continue;
            }
            tree.insert(
                FightActorRef::Building(building.building_id),
                building.position.x,
                building.position.z,
                building_radius(building),
            );
        }

        for (&actor_id, actor) in actors
            .iter()
            .filter(|(_, actor)| actor.placement.team == team)
        {
            tree.insert(
                FightActorRef::Unit(actor_id),
                actor.x_q32,
                actor.z_q32,
                actor.rules.collision_radius(),
            );
        }
        trees.insert(team, tree);
    }
    trees
}

/// One hit, as the fight's damage pipeline reads it.
///
/// This is the build's `IDamageProvider` reduced to what this simulator uses:
/// who dealt it, how much, what it was aimed at, where its splash is measured
/// from and how far it reaches, and which domains it can touch. A direct
/// strike, a projectile arriving and a laser are all described as one, and
/// [`Simulation::damage_targets`] and [`Simulation::strike`] resolve every one
/// of them, which is the build's arrangement: `DamagePerformer` resolves the
/// damage of any provider — skills, projectiles, commander skills, mines,
/// explosions — against `FightActor`s, and a unit and a building are both.
#[derive(Debug, Clone, Copy)]
struct DamageHit {
    source: ObjectRef,
    /// The team the hit is recorded under.
    source_team: u32,
    /// The team whose enemies it strikes: the attacker's own at the moment of
    /// impact, which a projectile reads from its owner rather than from the
    /// team it was released under.
    team: u32,
    amount: i64,
    /// What the attack was aimed at.
    aimed: FightActorRef,
    /// Whether the aimed-at object is struck wherever it stands, rather than
    /// only if the splash reaches it. A direct strike always is; a projectile
    /// is when it locks its target.
    hits_aimed: bool,
    /// Where the splash is measured from, in space units.
    center: (i64, i64),
    splash_radius: i64,
    reach: Reach,
}

/// Which units a hit can touch.
#[derive(Debug, Clone, Copy)]
enum Reach {
    /// The attacker's own `targets`: ground, air or both.
    Targets(AttackTargets),
    /// One domain only. `FightProjectile.Init` narrows a dual-domain skill to
    /// the actual target's domain, and `IDamageProvider.GetTargetType`
    /// preserves that choice for range damage.
    Domain(UnitDomain),
}

impl Reach {
    const fn touches(self, domain: UnitDomain) -> bool {
        match (self, domain) {
            (Self::Targets(targets), UnitDomain::Ground) => targets.ground,
            (Self::Targets(targets), UnitDomain::Air) => targets.air,
            (Self::Domain(UnitDomain::Ground), UnitDomain::Ground)
            | (Self::Domain(UnitDomain::Air), UnitDomain::Air) => true,
            (Self::Domain(_), _) => false,
        }
    }
}

/// What one target took from a hit.
#[derive(Debug, Clone, Copy)]
struct Stroke {
    /// The life it actually lost, which is what a `damage` event records.
    actual: i64,
    /// Where a unit died, when this stroke killed it.
    death: Option<QVec3>,
    /// Where a building fell, when this stroke destroyed it.
    fallen: Option<QVec3>,
}

/// What a performed hit left for its caller to record.
///
/// Deaths and fallen buildings are handed back rather than recorded here
/// because each way of dealing damage records them in its own place in the
/// tick's events: a projectile records its own removal first.
#[derive(Debug, Default)]
struct Struck {
    deaths: Vec<(u64, QVec3)>,
    fallen: Vec<(u64, QVec3)>,
}

#[derive(Debug, Clone)]
struct Projectile {
    id: u64,
    team: u32,
    owner: u64,
    target_kind: ObjectKind,
    target: u64,
    x: i64,
    y: i64,
    z: i64,
    x_q32: i64,
    y_q32: i64,
    z_q32: i64,
    cached_target_x: i64,
    cached_target_y: i64,
    cached_target_z: i64,
    cached_target_x_q32: i64,
    cached_target_y_q32: i64,
    cached_target_z_q32: i64,
    cached_target_radius: i64,
    speed: i64,
    damage: i64,
    life: i64,
    lock_target: bool,
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
            position: QVec3 {
                x: self.x_q32,
                y: self.y_q32,
                z: self.z_q32,
            },
            orientation: 0,
            target: Some(ObjectRef::new(self.target_kind, self.target)),
            cached_target_position: QVec3 {
                x: self.cached_target_x_q32,
                y: self.cached_target_y_q32,
                z: self.cached_target_z_q32,
            },
            cached_target_radius: space_to_q32(self.cached_target_radius),
            released: false,
            life: GaugeI32 {
                current: i32::try_from(self.life).expect("projectile life fits i32"),
                maximum: i32::try_from(self.life).expect("projectile life fits i32"),
            },
            spawn_containing_shields: Vec::new(),
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    pub end_reason: &'static str,
    pub steps: u64,
    pub simulated_duration_milliseconds: u64,
    pub winner: Option<&'static str>,
    pub draw: bool,
    pub teams: Vec<TeamResult>,
    pub hashes: Hashes,
    pub profiling: SimulationProfile,
}

#[derive(Debug, Clone, Serialize)]
pub struct SimulationProfile {
    pub generation_duration_milliseconds: f64,
    pub simulation_to_real_time_rate: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_sizes_bytes: Option<BTreeMap<String, u64>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SimulationComparison {
    pub schema: &'static str,
    pub game_build: String,
    pub seed: i32,
    pub equal: bool,
    pub content_equal: bool,
    pub recording: TimelineSummary,
    pub simulation: TimelineSummary,
    pub first_divergence: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub divergent_tick: Option<DivergentTick>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TimelineSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub physics_result_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_result_hash: Option<String>,
    pub tick_count: u32,
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DivergentTick {
    pub recording: Option<TickSlice>,
    pub simulation: Option<TickSlice>,
}

struct Simulation {
    actors: BTreeMap<u64, Actor>,
    team_random: BTreeMap<u32, GrRandom>,
    projectiles: Vec<Projectile>,
    buildings: Vec<BuildingState>,
    target_quadtrees: BTreeMap<u32, TargetActorQuadtree>,
    identities: IdentityAllocator,
    rvo_counter: u8,
    // Native Agent::.ctor stores its initial position in the public backing
    // buffer. The internal position read by the first BuildQuadtree remains
    // zero until the subsequent BufferSwitch.
    rvo_first_tree_pending: bool,
    terminal_drain_pending: bool,
    late_building_events_pending: bool,
    /// Buildings a projectile destroyed this tick, held until every projectile
    /// has resolved so their events follow all of the tick's shots.
    fallen_buildings: Vec<Event>,
    /// The RVO collider layer of every construction, by building.
    ///
    /// A construction is an obstacle only to the other side: the wall's own
    /// description says it sinks into the ground for a friendly unit, and a
    /// Crawler of the side that placed it walks through a block. To the other
    /// side it is an immovable agent on its `pathfinding_collider_priority`
    /// layer with its own box for a radius — a Steel Ball of `wall-laser.yaml`
    /// overlapping block 3 is pushed off it as that agent pushes it, tick for
    /// tick, and every other wall fight is unchanged by it.
    construction_colliders: BTreeMap<u64, i32>,
}

impl Simulation {
    #[cfg(test)]
    fn new(
        layout: &CompiledLayout,
        configs: &UnitConfigs,
        training_ground: &TrainingGroundConfig,
        seed: i32,
    ) -> Result<Self> {
        let mut simulation = Self::new_unprepared(layout, configs, training_ground, seed)?;
        simulation.initialize_presearch_targets()?;
        Ok(simulation)
    }

    fn new_unprepared(
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
        let mut actors = initialize_actors(layout, configs, seed)?;
        let mut team_random = BTreeMap::new();
        // Deployment draws one stagger per member, in identity order, and the
        // build keeps it as that member's first interval. Nothing schedules an
        // attack yet, so the draw is kept rather than consumed and discarded.
        for actor in actors.values_mut() {
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
                let skill_count = if actor.rules.attack.weapons.mode == WeaponMode::Group {
                    actor.rules.attack.weapons.count
                } else {
                    1
                };
                for index in 0..skill_count {
                    let sample =
                        random.next_in_range(i32::try_from(offset_steps).unwrap_or(i32::MAX));
                    if index == 0 {
                        actor.current_attack_interval = i64::try_from(native_time_units_to_steps(
                            actor.stats.attack_interval(),
                        ))
                        .unwrap_or(i64::MAX)
                        .saturating_add(i64::from(sample))
                        .max(1)
                        .cast_unsigned();
                    }
                }
            } else {
                actor.current_attack_interval =
                    native_time_units_to_steps(actor.stats.attack_interval()).max(1);
            }
        }
        let InitialBuildings {
            states: buildings,
            unsearchable,
            colliders: construction_colliders,
        } = initialize_buildings(training_ground, &layout.constructions)?;
        let target_quadtrees = initialize_target_quadtrees(&actors, &buildings, &unsearchable);
        Ok(Self {
            actors,
            team_random,
            projectiles: Vec::new(),
            buildings,
            target_quadtrees,
            identities: IdentityAllocator::new(),
            rvo_counter: 0,
            rvo_first_tree_pending: true,
            terminal_drain_pending: false,
            late_building_events_pending: false,
            fallen_buildings: Vec::new(),
            construction_colliders: construction_colliders.clone(),
        })
    }

    fn initialize_presearch_targets(&mut self) -> Result<()> {
        let actor_ids = self.actors.keys().copied().collect::<Vec<_>>();
        // Build 2259 PresearchTargetController::CalculateCountPerTime returns
        // ceil(mech_count / 10). SearchTarget assigns the zero-based batch
        // ordinal to the main FightSkill search controller before selecting
        // its initial target.
        let count_per_time = actor_ids.len().div_ceil(10).max(1);
        // The selector answers whatever stands nearest, and a building is an
        // answer: a Defensive Wall in front of a deployment is what the other
        // side presearches, which is what it does in the game.
        let target_search_order = self.target_search_order();
        let selections = actor_ids
            .iter()
            .map(|&actor_id| {
                Ok((
                    actor_id,
                    self.select_normal_target_with_order(actor_id, &target_search_order, false)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        for (ordinal, (actor_id, target)) in selections.into_iter().enumerate() {
            self.actors
                .get_mut(&actor_id)
                .expect("initial actor identity is stable")
                .fight_skill_search_target_time = i32::try_from(ordinal / count_per_time)
                .expect("presearch batch ordinal is at most nine");
            let Some(target) = target else {
                continue;
            };
            let view = self
                .fight_actor(target)
                .ok_or_else(|| Error::new("presearch chose a target that is not on the board"))?;
            let target_rotation_q32 = direction_degrees_q32_raw(
                view.x_q32.saturating_sub(self.actors[&actor_id].x_q32),
                view.z_q32.saturating_sub(self.actors[&actor_id].z_q32),
            );
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("initial actor identity is stable");
            actor.lock_target = Some(target);
            actor.set_body_rotation(target_rotation_q32);
            actor.aim_rotation = actor.body_rotation;
            actor.set_weapon_rotation(target_rotation_q32);
            self.engage_wall_in_the_way(actor_id);
        }
        Ok(())
    }

    fn snapshot(&self) -> WorldSnapshot {
        WorldSnapshot {
            live_units: self
                .actors
                .values()
                .filter(|actor| actor.alive())
                .map(Actor::snapshot)
                .collect(),
            projectiles: self.projectiles.iter().map(Projectile::snapshot).collect(),
            buildings: self
                .buildings
                .iter()
                .filter(|building| building_alive(building))
                .cloned()
                .collect(),
            ..WorldSnapshot::default()
        }
    }

    fn refresh_target_query_snapshot(&mut self) {
        // Build 2259 prepares selector inputs before FightCore updates actors
        // sequentially. Red actors must therefore score the tick-start pose,
        // not positions already advanced by blue actors in the same tick.
        // FightSkill::GetMainTransform returns its first valid owned weapon transform;
        // bodyless weapons without one fall back to the mech's root transform.
        for actor in self.actors.values_mut() {
            actor.target_query_x_q32 = actor.x_q32;
            actor.target_query_z_q32 = actor.z_q32;
            actor.target_query_source_rotation_q32 =
                if actor.rules.has_body || actor.rules.attack.weapons.mode == WeaponMode::Group {
                    actor
                        .weapon_rotations_q32
                        .first()
                        .copied()
                        .unwrap_or(actor.body_rotation_q32)
                } else {
                    actor.body_rotation_q32
                };
            actor.target_query_alive = actor.alive();
            actor.fight_skill_searched_this_tick = false;
        }
    }

    /// Reads every unit with no enemy left at its interval as composed, with
    /// no stagger.
    ///
    /// The stagger rides on the cycle in progress, and once a unit's last
    /// enemy is dead there is none: from the tick after, and on the fight's
    /// final tick if that is the one, the game reads a Marksman at 62, an
    /// Arclight at 18 and a Wraith at 32 — their descriptions — whatever
    /// cycle they were on.
    fn settle_intervals(&mut self) {
        let alive_teams = self
            .actors
            .values()
            .filter(|actor| actor.alive())
            .map(|actor| actor.placement.team)
            .collect::<BTreeSet<_>>();
        for actor in self.actors.values_mut().filter(|actor| actor.alive()) {
            let team = actor.placement.team;
            if alive_teams.iter().all(|&other| other == team) {
                actor.current_attack_interval =
                    native_time_units_to_steps(actor.stats.attack_interval());
            }
        }
    }

    /// The same, on the fight's last tick, before its snapshot is written.
    fn settle_intervals_if_finishing(&mut self) {
        if self.ready_to_finish() {
            self.settle_intervals();
        }
    }

    #[allow(clippy::too_many_lines)]
    fn step(&mut self, step: u64) -> Result<TransitionEvents> {
        self.settle_intervals();
        let publish_late_building_events = self.late_building_events_pending;
        self.late_building_events_pending = false;
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
        let teams_with_building_target_at_start = self
            .actors
            .values()
            .filter_map(|actor| {
                matches!(actor.attack_target(), Some(FightActorRef::Building(_)))
                    .then_some(actor.placement.team)
            })
            .collect::<std::collections::BTreeSet<_>>();
        self.refresh_target_query_snapshot();
        let mut events = Vec::new();
        if publish_late_building_events && let Some(winning_team) = self.winner() {
            for building in self
                .buildings
                .iter()
                .filter(|building| building.team_id != winning_team && !building_alive(building))
            {
                events.push(event(
                    Some(ObjectRef::new(ObjectKind::Building, building.building_id)),
                    None,
                    None,
                    None,
                    EventPayload::BuildingDestroyed {
                        position: building.position,
                    },
                ));
            }
        }
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
        let naturally_finished_before_projectiles = self.naturally_finished();
        let actors_alive_before_projectiles = self
            .actors
            .iter()
            .filter_map(|(&actor_id, actor)| actor.alive().then_some(actor_id))
            .collect::<Vec<_>>();
        self.step_projectiles(&mut events)?;
        if let Some(winning_team) = self.winner() {
            let winning_actors_killed_by_projectiles = actors_alive_before_projectiles
                .into_iter()
                .filter(|actor_id| {
                    let actor = &self.actors[actor_id];
                    !actor.alive() && actor.placement.team == winning_team
                })
                .collect::<Vec<_>>();
            for actor_id in winning_actors_killed_by_projectiles {
                let actor = &self.actors[&actor_id];
                if actor_motion_at_start.get(&actor_id) != Some(&MotionState::Moving)
                    || !actor.rules.has_body
                    || !matches!(actor.rules.attack.path, AttackPath::Projectile { .. })
                {
                    continue;
                }
                let Some(enemy_team) = self
                    .buildings
                    .iter()
                    .find(|building| {
                        building.team_id != actor.placement.team && building_alive(building)
                    })
                    .map(|building| building.team_id)
                else {
                    continue;
                };
                let Some(building_id) = self.select_normal_building_target(actor_id, enemy_team)
                else {
                    continue;
                };
                let building = self
                    .buildings
                    .iter()
                    .find(|building| building.building_id == building_id)
                    .expect("selected building exists");
                let target_x_q32 = building.position.x;
                let target_z_q32 = building.position.z;
                let actor = self
                    .actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable");
                actor.rotate_weapons_towards(direction_degrees_q32_raw(
                    target_x_q32.saturating_sub(actor.x_q32),
                    target_z_q32.saturating_sub(actor.z_q32),
                ));
                actor.aim_rotation = degrees_q32_to_mdeg(
                    actor
                        .weapon_rotations_q32
                        .first()
                        .copied()
                        .unwrap_or(actor.body_rotation_q32),
                );
            }
        }
        let projectile_finished_fight =
            !naturally_finished_before_projectiles && self.naturally_finished();
        let natural_finish_handoff = !fight_was_finished && self.naturally_finished();
        let early_projectile_handoff = !winner_was_decided && !self.naturally_finished();
        if (natural_finish_handoff || early_projectile_handoff)
            && let Some(winning_team) = self.winner()
            && let Some(losing_team) = self
                .buildings
                .iter()
                .find(|building| building.team_id != winning_team && building_alive(building))
                .map(|building| building.team_id)
            && !teams_with_building_target_at_start.contains(&losing_team)
        {
            let mut queued_direct_own_kill_handoff = false;
            let actor_ids = self
                .actors
                .iter()
                .filter_map(|(&actor_id, actor)| actor.alive().then_some(actor_id))
                .collect::<Vec<_>>();
            for actor_id in actor_ids {
                let Some(&motion_at_start) = actor_motion_at_start.get(&actor_id) else {
                    continue;
                };
                let actor = &self.actors[&actor_id];
                let team_has_bodyful_projectile = self.actors.values().any(|candidate| {
                    candidate.placement.team == actor.placement.team
                        && candidate.alive()
                        && candidate.rules.has_body
                        && matches!(candidate.rules.attack.path, AttackPath::Projectile { .. })
                });
                let moving_direct = motion_at_start == MotionState::Moving
                    && matches!(actor.rules.attack.path, AttackPath::Direct { .. })
                    && team_has_bodyful_projectile
                    && (actor.current_velocity_x_q32 != 0 || actor.current_velocity_z_q32 != 0)
                    && actor.pending.is_none()
                    && actor.backswing_finish_step.is_none()
                    && actor.fight_skill_phase == FightSkillPhase::Idle
                    && !actor.motion_attack_hold_fire
                    && actor.fight_skill_searched_this_tick
                    && actor.attack_target().is_none();
                let moving_bodyful_projectile = motion_at_start == MotionState::Moving
                    && actor.rules.has_body
                    && matches!(actor.rules.attack.path, AttackPath::Projectile { .. })
                    && actor.fight_skill_searched_this_tick
                    && actor.attack_target().is_none();
                let ineligible = if natural_finish_handoff {
                    !moving_direct
                        && (actor.attack_target().is_some()
                            || actor.retarget_after_own_direct_kill
                            || !actor.fight_skill_searched_this_tick)
                } else {
                    (!moving_direct && !moving_bodyful_projectile)
                        || actor
                            .attack_target()
                            .is_some_and(|target| self.fight_actor_is_alive(target))
                };
                if ineligible {
                    continue;
                }
                let Some(building_id) = self.select_normal_building_target(actor_id, losing_team)
                else {
                    continue;
                };
                let building = self
                    .buildings
                    .iter()
                    .find(|building| building.building_id == building_id)
                    .expect("selected building exists");
                let target_x_q32 = building.position.x;
                let target_z_q32 = building.position.z;
                let target_radius = building_radius(building);
                let actor = self
                    .actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable");
                if natural_finish_handoff {
                    if motion_at_start == MotionState::Moving
                        && (actor.current_velocity_x_q32 != 0 || actor.current_velocity_z_q32 != 0)
                    {
                        actor.rotate_body_towards(direction_degrees_q32_raw(
                            actor.current_velocity_x_q32,
                            actor.current_velocity_z_q32,
                        ));
                        actor.aim_rotation = actor.body_rotation;
                    }
                } else {
                    if actor.current_velocity_x_q32 != 0 || actor.current_velocity_z_q32 != 0 {
                        actor.rotate_body_towards(direction_degrees_q32_raw(
                            actor.current_velocity_x_q32,
                            actor.current_velocity_z_q32,
                        ));
                    }
                    if moving_direct {
                        actor.aim_rotation = actor.body_rotation;
                    } else {
                        actor.rotate_weapons_towards(direction_degrees_q32_raw(
                            target_x_q32.saturating_sub(actor.x_q32),
                            target_z_q32.saturating_sub(actor.z_q32),
                        ));
                        actor.aim_rotation = degrees_q32_to_mdeg(
                            actor
                                .weapon_rotations_q32
                                .first()
                                .copied()
                                .unwrap_or(actor.body_rotation_q32),
                        );
                    }
                }
                actor.lock_target = Some(FightActorRef::Building(building_id));
                actor.lock_is_terminal_handoff = true;
                actor.motion = MotionState::Moving;
                if natural_finish_handoff {
                    actor.next_target_x_q32 = target_x_q32;
                    actor.next_target_z_q32 = target_z_q32;
                    actor.next_speed_q32 = space_to_q32(actor.stats.move_speed());
                } else {
                    let (move_target_x_q32, move_target_z_q32) = native_auto_move_target_point(
                        actor.x_q32,
                        actor.z_q32,
                        actor.rules.collision_radius(),
                        target_x_q32,
                        target_z_q32,
                        target_radius,
                        actor.stats.attack_range(),
                    );
                    actor.next_target_x_q32 = move_target_x_q32;
                    actor.next_target_z_q32 = move_target_z_q32;
                    actor.next_speed_q32 = turn_limited_move_speed_q32(
                        space_to_q32(actor.stats.move_speed()),
                        actor.rules.rotate_speed_mdeg_per_second(),
                        actor.body_rotation_q32,
                        actor.current_velocity_x_q32,
                        actor.current_velocity_z_q32,
                    );
                }
                actor.next_max_speed_q32 = actor.next_speed_q32;
                queued_direct_own_kill_handoff |= natural_finish_handoff && moving_direct;
            }
            if queued_direct_own_kill_handoff {
                self.terminal_drain_pending = true;
            }
        }
        // Native build 2259 tears down the defeated core buildings through the
        // direct-attack finish path. Projectile drain reaches the same round
        // result without mutating buildings (observed in Fang mirror battles).
        let direct_attack_winner = !fight_was_finished
            && !winner_was_decided
            && !projectile_finished_fight
            && self.winner().is_some_and(|winning_team| {
                self.actors.values().any(|actor| {
                    actor.placement.team == winning_team
                        && actor.alive()
                        && matches!(actor.rules.attack.path, AttackPath::Direct { .. })
                })
            });
        let mut queued_late_building_events = false;
        for building in self.buildings.iter_mut().filter(|building| {
            direct_attack_winner
                && building_alive(building)
                && team_alive_counts
                    .get(&building.team_id)
                    .is_some_and(|alive_count| *alive_count == 0)
        }) {
            building.life.current = 0;
            building.targetable = false;
            queued_late_building_events = true;
        }
        if queued_late_building_events {
            self.terminal_drain_pending = true;
            self.late_building_events_pending = true;
        }
        let laser_finish_observed_at_defeated_team_entry = !fight_was_finished
            && self.naturally_finished()
            && self.winner().is_some_and(|winning_team| {
                self.actors.values().any(|actor| {
                    actor.placement.team == winning_team
                        && actor.alive()
                        && matches!(actor.rules.attack.path, AttackPath::Laser { .. })
                })
            })
            && team_alive_counts
                .iter()
                .any(|(&team_id, &alive_count)| alive_count == 0 && Some(team_id) != self.winner());
        if laser_finish_observed_at_defeated_team_entry {
            // FightCore updates blue before red. If an earlier team eliminates
            // a later team, the native finish callback is queued only after
            // that defeated team's module observes its empty actor set. Its
            // attacker therefore exposes the dead laser target for one tick
            // while the callback tears down that team's core buildings.
            for building in self.buildings.iter_mut().filter(|building| {
                building_alive(building)
                    && team_alive_counts
                        .get(&building.team_id)
                        .is_some_and(|alive_count| *alive_count == 0)
            }) {
                events.push(event(
                    Some(ObjectRef::new(ObjectKind::Building, building.building_id)),
                    None,
                    None,
                    None,
                    EventPayload::BuildingDestroyed {
                        position: building.position,
                    },
                ));
                building.life.current = 0;
                building.targetable = false;
            }
            self.terminal_drain_pending = true;
        }
        let ready_to_finish = self.ready_to_finish();
        let stop_fight = ready_to_finish || winner_was_decided;
        if stop_fight {
            for actor in self.actors.values_mut() {
                actor.motion = MotionState::Idle;
                actor.drop_lock();
                actor.lock_is_terminal_handoff = false;
                actor.fight_skill_phase = FightSkillPhase::Idle;
                if ready_to_finish {
                    actor.current_velocity_x_q32 = 0;
                    actor.current_velocity_z_q32 = 0;
                }
            }
        }
        if !ready_to_finish {
            self.step_rvo();
        }
        for death in events
            .iter_mut()
            .filter(|event| matches!(&event.payload, EventPayload::UnitDied { .. }))
        {
            let Some(dead_id) = death
                .subject
                .filter(|subject| subject.kind == ObjectKind::Unit)
                .map(|subject| subject.id)
            else {
                continue;
            };
            (death.source, death.source_team_id) = self.actors[&dead_id]
                .last_damage_source
                .map_or((None, None), |(source, team_id)| {
                    (Some(source), Some(team_id))
                });
        }
        let (mut events, deaths): (Vec<_>, Vec<_>) = events
            .into_iter()
            .partition(|event| !matches!(&event.payload, EventPayload::UnitDied { .. }));
        events.extend(deaths);
        Ok(TransitionEvents { events })
    }

    fn target_search_order(&self) -> BTreeMap<u32, Vec<FightActorRef>> {
        self.target_quadtrees
            .iter()
            .map(|(&team, tree)| (team, tree.query_order()))
            .collect()
    }

    fn fight_actor(&self, reference: FightActorRef) -> Option<FightActorView> {
        match reference {
            FightActorRef::Unit(id) => {
                let actor = self.actors.get(&id)?;
                Some(FightActorView {
                    team: actor.placement.team,
                    x_q32: actor.x_q32,
                    z_q32: actor.z_q32,
                    query_x_q32: actor.target_query_x_q32,
                    query_z_q32: actor.target_query_z_q32,
                    radius: actor.rules.collision_radius(),
                    alive: actor.alive(),
                    query_alive: actor.target_query_alive,
                    targetable: actor.alive(),
                    domain: actor.rules.domain,
                })
            }
            FightActorRef::Building(id) => {
                let building = self
                    .buildings
                    .iter()
                    .find(|building| building.building_id == id)?;
                let x_q32 = building.position.x;
                let z_q32 = building.position.z;
                Some(FightActorView {
                    team: building.team_id,
                    x_q32,
                    z_q32,
                    query_x_q32: x_q32,
                    query_z_q32: z_q32,
                    radius: building_radius(building),
                    alive: building_alive(building),
                    query_alive: building_alive(building),
                    targetable: building.targetable && building.available,
                    domain: UnitDomain::Ground,
                })
            }
        }
    }

    fn fight_actor_is_alive(&self, reference: FightActorRef) -> bool {
        self.fight_actor(reference)
            .is_some_and(|target| target.alive && target.targetable)
    }

    fn select_normal_building_target(&self, actor_id: u64, team_id: u32) -> Option<u64> {
        let source = self.actors.get(&actor_id)?;
        if !source.rules.attack.targets.ground {
            return None;
        }
        let mut best: Option<(u64, i64)> = None;
        for building in self.buildings.iter().filter(|building| {
            building.team_id == team_id && building_alive(building) && building.targetable
        }) {
            let Some(score) = normal_visible_full_rotation_target_score_q32(
                source.target_query_x_q32,
                source.target_query_z_q32,
                source.rules.collision_radius(),
                source.target_query_source_rotation_q32,
                building.position.x,
                building.position.z,
                building_radius(building),
                source.rules.attack.min_range(),
                source.stats.attack_range(),
            ) else {
                continue;
            };
            if best.is_none_or(|(_, best_score)| score < best_score) {
                best = Some((building.building_id, score));
            }
        }
        best.map(|(building_id, _)| building_id)
    }

    /// Asks again which construction, if any, stands in this actor's line of
    /// fire, and hands it to the weapons.
    ///
    /// The lock is left alone. `FightSkill.SearchAttackTarget` asks
    /// `WallConstructionTargetChecker` wherever a search settles and every
    /// tick the skill is idle, and a block that is no longer in the way stops
    /// being the attack target the next time it is asked. Nothing changes in a
    /// fight that places no enemy construction.
    fn engage_wall_in_the_way(&mut self, actor_id: u64) {
        let found = match self.actors[&actor_id].lock_target {
            // A search that already chose a building is not redirected: the
            // measurement is a wall taking the place of a unit.
            Some(target @ FightActorRef::Unit(_)) => self
                .wall_in_the_way(actor_id, target)
                .map(|building| (building, target)),
            _ => None,
        };
        self.actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .in_the_way = found;
        self.refresh_group_walls(actor_id);
    }

    /// Asks each grouped slot which construction stands between the actor and
    /// the unit that slot was allocated.
    ///
    /// Every slot of a Wraith takes the block its core took, eight ticks later,
    /// when the group allocates its children. With one enemy unit the slots'
    /// lines are the core's, so whether several blocks in reach would be
    /// shared out among the slots is not something any recording has shown;
    /// the build's `CheckWallConstructionForGroupedSkill` keeps a list of walls
    /// already checked, which suggests it might, and nothing here assumes so.
    fn refresh_group_walls(&mut self, actor_id: u64) {
        let slots = self.actors[&actor_id].group_skill_targets.clone();
        if slots.is_empty() {
            return;
        }
        let found = slots
            .iter()
            .map(|slot| {
                slot.and_then(|unit| {
                    self.wall_in_the_way(actor_id, FightActorRef::Unit(unit))
                        .map(|building| (building, unit))
                })
            })
            .collect::<Vec<_>>();
        self.actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .group_in_the_way = found;
    }

    /// Which enemy construction stands between this actor and its target.
    ///
    /// `docs/rules/constructions.md` states the rule and the readings behind
    /// it: of the enemy's constructions, the ones within reach edge to edge and
    /// within the width of the line of fire, the **nearest to the attacker** —
    /// not the nearest construction and not the one nearest the line.
    fn wall_in_the_way(&self, actor_id: u64, target: FightActorRef) -> Option<u64> {
        let actor = self.actors.get(&actor_id)?;
        let aimed = self.fight_actor(target)?;
        // A wall is considered when it is within reach edge to edge: the
        // attacker's range plus its own radius and the block's. A constant
        // allowance fits a Crawler and not a Wraith, which attacks a block 73.8
        // metres off with a reach of 60.
        let reach = space_to_q32(
            actor
                .stats
                .attack_range()
                .saturating_add(actor.rules.collision_radius()),
        );
        let width = space_to_q32(WALL_IN_THE_WAY_WIDTH);
        let mut nearest: Option<(i64, u64)> = None;
        for building in &self.buildings {
            if building.building_type_id != CONSTRUCTION_BUILDING_TYPE
                || building.team_id == actor.placement.team
                || !building_alive(building)
                || !building.targetable
            {
                continue;
            }
            let distance = native_q32_magnitude(
                building.position.x.saturating_sub(actor.x_q32),
                building.position.z.saturating_sub(actor.z_q32),
            );
            if distance > reach.saturating_add(building.bounds_width / 2) {
                continue;
            }
            if distance_to_segment_q32(
                (actor.x_q32, actor.z_q32),
                (aimed.x_q32, aimed.z_q32),
                (building.position.x, building.position.z),
            ) > width
            {
                continue;
            }
            if nearest.is_none_or(|(best, _)| distance < best) {
                nearest = Some((distance, building.building_id));
            }
        }
        nearest.map(|(_, building_id)| building_id)
    }

    /// The selector, restricted to units. A test asks for one; the fight
    /// itself takes whatever stands nearest, buildings included.
    #[cfg(test)]
    fn select_normal_unit_target(&self, actor_id: u64) -> Result<Option<u64>> {
        let target_search_order = self.target_search_order();
        self.select_normal_unit_target_with_order(actor_id, &target_search_order, false)
    }

    fn select_normal_unit_target_with_order(
        &self,
        actor_id: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        use_live_candidate_positions: bool,
    ) -> Result<Option<u64>> {
        match self.select_normal_target_with_order(
            actor_id,
            target_search_order,
            use_live_candidate_positions,
        )? {
            Some(FightActorRef::Unit(unit_id)) => Ok(Some(unit_id)),
            Some(FightActorRef::Building(building_id)) => Err(Error::new(format!(
                "Normal selector chose building {building_id}, but this caller requires a unit"
            ))),
            None => Ok(None),
        }
    }

    fn select_normal_target_with_order(
        &self,
        actor_id: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        use_live_candidate_positions: bool,
    ) -> Result<Option<FightActorRef>> {
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
        let mut best: Option<(FightActorRef, i64)> = None;
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
                let Some(target) = self.fight_actor(candidate) else {
                    continue;
                };
                let candidate_alive = if use_live_candidate_positions {
                    target.alive
                } else {
                    target.query_alive
                };
                let candidate_targetable = if use_live_candidate_positions {
                    target.targetable
                } else {
                    match candidate {
                        FightActorRef::Unit(_) => target.query_alive,
                        FightActorRef::Building(_) => target.targetable,
                    }
                };
                if target.team != team
                    || !candidate_alive
                    || !candidate_targetable
                    || !source.rules.attack.accepts(target.domain)
                {
                    continue;
                }
                let (candidate_x_q32, candidate_z_q32) = if use_live_candidate_positions {
                    (target.x_q32, target.z_q32)
                } else {
                    (target.query_x_q32, target.query_z_q32)
                };
                if let Some(score) = normal_visible_full_rotation_target_score_q32(
                    source.target_query_x_q32,
                    source.target_query_z_q32,
                    source.rules.collision_radius(),
                    source.target_query_source_rotation_q32,
                    candidate_x_q32,
                    candidate_z_q32,
                    target.radius,
                    source.rules.attack.min_range(),
                    source.stats.attack_range(),
                ) {
                    consider(candidate, score);
                }
            }
        }

        Ok(best.map(|(candidate, _)| candidate))
    }

    fn rank_group_unit_targets_with_order(
        &self,
        actor_id: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        use_live_candidate_positions: bool,
    ) -> Result<Vec<u64>> {
        let source = self
            .actors
            .get(&actor_id)
            .ok_or_else(|| Error::new("target selector source actor is absent"))?;
        let mut scored = Vec::new();
        let mut ordinal = 0_usize;
        for (&team, candidates) in target_search_order {
            if team == source.placement.team {
                continue;
            }
            for &candidate in candidates {
                let FightActorRef::Unit(candidate_id) = candidate else {
                    continue;
                };
                let Some(candidate_actor) = self.actors.get(&candidate_id) else {
                    continue;
                };
                let candidate_alive = if use_live_candidate_positions {
                    candidate_actor.alive()
                } else {
                    candidate_actor.target_query_alive
                };
                if !candidate_alive || !source.rules.attack.accepts(candidate_actor.rules.domain) {
                    continue;
                }
                let (candidate_x_q32, candidate_z_q32) = if use_live_candidate_positions {
                    (candidate_actor.x_q32, candidate_actor.z_q32)
                } else {
                    (
                        candidate_actor.target_query_x_q32,
                        candidate_actor.target_query_z_q32,
                    )
                };
                let Some(score) = normal_visible_full_rotation_target_score_q32(
                    source.target_query_x_q32,
                    source.target_query_z_q32,
                    source.rules.collision_radius(),
                    source.body_rotation_q32,
                    candidate_x_q32,
                    candidate_z_q32,
                    candidate_actor.rules.collision_radius(),
                    source.rules.attack.min_range(),
                    source.stats.attack_range(),
                ) else {
                    continue;
                };
                // PerformGroupedSkillSearch walks OpponentController.GetActors
                // and lets the selector score every alive, type-valid actor.
                // Range is checked later by the individual FightSkill state.
                scored.push((score, ordinal, candidate_id));
                ordinal = ordinal.saturating_add(1);
            }
        }
        scored.sort_by_key(|&(score, order, _)| (score, order));
        Ok(scored
            .into_iter()
            .map(|(_, _, candidate_id)| candidate_id)
            .collect())
    }

    #[allow(clippy::too_many_lines)]
    fn update_group_skill_targets(
        &mut self,
        actor_id: u64,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<()> {
        let actor = &self.actors[&actor_id];
        if actor.rules.attack.weapons.mode != WeaponMode::Group
            || actor.motion != MotionState::Attacking
        {
            return Ok(());
        }
        let needs_initial_targets = actor.group_skill_targets.iter().any(Option::is_none);
        let all_targets_empty = actor.group_skill_targets.iter().all(Option::is_none);
        let core_was_empty = actor
            .group_skill_targets
            .first()
            .is_some_and(Option::is_none);
        let has_dead_target = actor
            .group_skill_targets
            .iter()
            .flatten()
            .any(|target_id| !self.actors.get(target_id).is_some_and(Actor::alive));
        let core_has_entered_attack = actor.fight_skill_phase == FightSkillPhase::Attack
            || matches!(
                actor.fight_skill_phase,
                FightSkillPhase::Prepare { finish_step }
                    if finish_step <= step.saturating_add(1)
            );
        if needs_initial_targets && !core_has_entered_attack {
            return Ok(());
        }
        if !needs_initial_targets && !has_dead_target {
            return Ok(());
        }
        let ranked = self
            .rank_group_unit_targets_with_order(actor_id, target_search_order, has_dead_target)
            .map_err(|error| Error::new(format!("logic step {step} actor {actor_id}: {error}")))?;
        let core_ranked = ranked
            .iter()
            .copied()
            .filter(|target_id| {
                let target = &self.actors[target_id];
                let (target_x_q32, target_z_q32) = if has_dead_target {
                    (target.x_q32, target.z_q32)
                } else {
                    (target.target_query_x_q32, target.target_query_z_q32)
                };
                native_q32_magnitude(
                    target_x_q32.saturating_sub(actor.target_query_x_q32),
                    target_z_q32.saturating_sub(actor.target_query_z_q32),
                )
                .saturating_sub(space_to_q32(actor.rules.collision_radius()))
                .saturating_sub(space_to_q32(target.rules.collision_radius()))
                    <= space_to_q32(actor.stats.attack_range())
            })
            .collect::<Vec<_>>();
        // Slots hold the units they were allocated; a construction in a slot's
        // way is asked for afterwards. So the core is seeded with the unit the
        // mech is locked on, never with the block its weapons are firing at.
        let current_target = actor
            .mechanical_lock_target()
            .and_then(FightActorRef::unit_id);
        let prepare_steps = native_time_units_to_steps(actor.rules.attack.prepare_time_units());
        let allow_same_target = actor
            .rules
            .attack
            .weapons
            .allow_same_target
            .unwrap_or(false);
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        if all_targets_empty
            && actor
                .group_skill_targets
                .first()
                .is_some_and(Option::is_none)
        {
            actor.group_skill_targets[0] = current_target;
        }
        let previous_targets = actor.group_skill_targets.clone();
        let core_needs_replacement = previous_targets
            .first()
            .copied()
            .flatten()
            .is_none_or(|target_id| !core_ranked.contains(&target_id));
        let mut used = actor
            .group_skill_targets
            .iter()
            .flatten()
            .copied()
            .filter(|target_id| ranked.contains(target_id))
            .collect::<std::collections::BTreeSet<_>>();
        let no_unused_global_target = ranked.iter().all(|target_id| used.contains(target_id));
        let mut formal_target_changes = BTreeMap::new();
        let mut redistributed_indices = std::collections::BTreeSet::new();
        if core_was_empty
            && !all_targets_empty
            && let Some(global_replacement) = ranked
                .iter()
                .copied()
                .find(|target_id| !used.contains(target_id))
            && !core_ranked.contains(&global_replacement)
            && let Some(donor_target) = core_ranked.iter().copied().find(|target_id| {
                actor
                    .group_skill_targets
                    .iter()
                    .skip(1)
                    .flatten()
                    .any(|current| current == target_id)
            })
            && let Some(donor_index) = actor
                .group_skill_targets
                .iter()
                .position(|target| *target == Some(donor_target))
        {
            actor.group_skill_targets[0] = Some(donor_target);
            actor.group_skill_targets[donor_index] = Some(global_replacement);
            let prepare_ready_step = step.saturating_add(prepare_steps).saturating_add(1);
            actor.group_skill_prepare_ready_steps[0] = prepare_ready_step;
            actor.group_skill_prepare_ready_steps[donor_index] = prepare_ready_step;
            used.insert(global_replacement);
            formal_target_changes.insert(0, Some(donor_target));
            formal_target_changes.insert(donor_index, Some(global_replacement));
            redistributed_indices.insert(0);
            redistributed_indices.insert(donor_index);
        }
        let mut replacement_indices = (0..actor.group_skill_targets.len()).collect::<Vec<_>>();
        if !all_targets_empty && !core_needs_replacement {
            replacement_indices.sort_by_key(|&index| {
                let target_is_invalid =
                    previous_targets[index].is_none_or(|target_id| !ranked.contains(&target_id));
                let follows_intervening_attack = target_is_invalid
                    && actor.group_skill_next_attack_steps[index] == step.saturating_add(1)
                    && (1..index).any(|earlier_index| {
                        let earlier_is_invalid = previous_targets[earlier_index]
                            .is_none_or(|target_id| !ranked.contains(&target_id));
                        earlier_is_invalid
                            && ((earlier_index + 1)..index).any(|middle_index| {
                                previous_targets[middle_index]
                                    .is_some_and(|target_id| ranked.contains(&target_id))
                                    && actor.group_skill_next_attack_steps[middle_index] > 0
                                    && actor.group_skill_next_attack_steps[middle_index] <= step
                            })
                    });
                (!follows_intervening_attack, index)
            });
        }
        for index in replacement_indices {
            if redistributed_indices.contains(&index) {
                continue;
            }
            let candidates = if index == 0 && !core_was_empty {
                &core_ranked
            } else {
                &ranked
            };
            let retained =
                actor.group_skill_targets[index].filter(|target_id| candidates.contains(target_id));
            if retained.is_some() {
                continue;
            }
            let replacement = candidates
                .iter()
                .copied()
                .find(|target_id| !used.contains(target_id))
                .or_else(|| {
                    (allow_same_target && (index != 0 || no_unused_global_target))
                        .then(|| candidates.first().copied())
                        .flatten()
                });
            actor.group_skill_targets[index] = replacement;
            formal_target_changes.insert(index, replacement);
            if let Some(target_id) = replacement {
                used.insert(target_id);
                if index != 0 && previous_targets[index].is_none() {
                    actor.group_pending_releases.push((
                        index,
                        PendingRelease {
                            step: step.saturating_add(prepare_steps).saturating_add(1),
                            target: FightActorRef::Unit(target_id),
                        },
                    ));
                }
            }
        }
        if let Some((_, target_id)) = formal_target_changes.last_key_value() {
            actor.lock_target = target_id.map(FightActorRef::Unit);
        }
        Ok(())
    }

    /// Holds a unit whose shot's target has died idle and without a lock
    /// until its cooling is over, with its weapon quick-switched to the
    /// selector's answer. Answers whether it did.
    ///
    /// The weapon holds the candidate for the cooling's length, counted from
    /// the step after the target died; it clears the step after that, and the
    /// lock is searched the step after that. A Marksman's cooling is 0.2
    /// seconds, four steps: idle five, locked on the sixth.
    fn hold_through_cooling(
        &mut self,
        actor_id: u64,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<bool> {
        let actor = &self.actors[&actor_id];
        let started = if let Some(started) = actor.cooling_hold {
            started
        } else {
            {
                // Only a target that dies while the attack that fired at it is
                // still running starts a hold: one that dies later, at the
                // end of a longer flight, is replaced at once, as quick
                // switching does.
                let Some(until) = actor.cooling_until_step else {
                    return Ok(false);
                };
                let target_dead = actor
                    .mechanical_attack_target()
                    .and_then(|target| self.fight_actor(target))
                    .is_some_and(|target| !(target.alive && target.targetable));
                // Seen on the step after the death.
                if !target_dead || step > until.saturating_add(1) {
                    return Ok(false);
                }
                // A replacement the quick switch can attack at once is
                // taken at once; only one it cannot starts the hold.
                let candidate =
                    self.select_normal_target_with_order(actor_id, target_search_order, true)?;
                if candidate.is_none_or(|candidate| self.target_in_attack_area(actor_id, candidate))
                {
                    return Ok(false);
                }
                step
            }
        };
        let cooling_steps =
            native_time_units_to_steps(self.actors[&actor_id].rules.attack.cooling_time_units());
        if step > started.saturating_add(cooling_steps) {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.cooling_hold = None;
            actor.cooling_until_step = None;
            actor.cooling_candidate = None;
            return Ok(false);
        }
        let candidate = if step < started.saturating_add(cooling_steps) {
            match self.actors[&actor_id].cooling_candidate {
                Some(candidate) => Some(candidate),
                None => {
                    self.select_normal_target_with_order(actor_id, target_search_order, true)?
                }
            }
        } else {
            None
        };
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        actor.lock_target = None;
        actor.cooling_candidate = candidate;
        actor.cooling_hold = Some(started);
        actor.motion = MotionState::Idle;
        actor.fight_skill_phase = FightSkillPhase::Idle;
        actor.fight_skill_search_target_time = 0;
        actor.next_target_x_q32 = actor.x_q32;
        actor.next_target_z_q32 = actor.z_q32;
        actor.next_speed_q32 = 0;
        actor.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
        Ok(true)
    }

    fn update_fight_skill_target_search(
        &mut self,
        actor_id: u64,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<()> {
        // Target-build MechData disables MechSearchTargetController for every
        // supported non-supergiant unit, so live periodic selection belongs to
        // the main FightSkill. Prepare and Attack retain this private counter;
        // Attack only enters the selector when its private attack target is no
        // longer alive.
        let actor = &self.actors[&actor_id];
        let target = actor
            .mechanical_attack_target()
            .and_then(|target| self.fight_actor(target));
        let target_alive = target.is_some_and(|target| target.alive && target.targetable);
        let target_died_during_tick =
            target.is_some_and(|target| target.query_alive && !target.alive);
        if !target_alive
            && actor
                .backswing_finish_step
                .is_some_and(|finish_step| finish_step >= step)
        {
            let quick_switch_interval_due =
                actor.rules.attack.quick_switch_target && step > actor.next_attack_step;
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

        let mut selected_candidate = self
            .select_normal_target_with_order(actor_id, target_search_order, target_died_during_tick)
            .map_err(|error| Error::new(format!("logic step {step} actor {actor_id}: {error}")))?;
        if !target_died_during_tick
            && selected_candidate
                .and_then(|candidate| self.fight_actor(candidate))
                .is_some_and(|target| target.query_alive && !target.alive)
        {
            selected_candidate = self
                .select_normal_target_with_order(actor_id, target_search_order, true)
                .map_err(|error| {
                    Error::new(format!("logic step {step} actor {actor_id}: {error}"))
                })?;
        }
        if let Some(FightActorRef::Building(building_id)) = selected_candidate {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.lock_target = Some(FightActorRef::Building(building_id));
            actor.lock_is_terminal_handoff = false;
            actor.fight_skill_search_target_time = SEARCH_TARGET_RESET_TICKS;
            actor.fight_skill_phase = FightSkillPhase::Idle;
            actor.retarget_after_own_direct_kill = false;
            actor.laser_attack_count = 0;
            return Ok(());
        }
        let selected = selected_candidate;
        let actor = &self.actors[&actor_id];
        let quick_idle_retains_attackable_target = actor.rules.attack.quick_switch_target
            && actor.fight_skill_phase == FightSkillPhase::Idle
            && step >= actor.next_attack_step
            && actor
                .attack_target()
                .is_some_and(|target_id| self.target_in_attack_area(actor_id, target_id));
        let selected = if target_alive
            && (!actor.rules.attack.quick_switch_target || quick_idle_retains_attackable_target)
            && actor.motion == MotionState::Attacking
            && !actor.motion_attack_hold_fire
            && actor.pending.is_none()
            && actor.backswing_finish_step.is_none()
            && selected != actor.lock_target
        {
            // An attacking unit keeps the lock it has. What its weapons fire
            // at is asked again below, so a construction still in the way is
            // handed back to them rather than written into the lock.
            actor.lock_target
        } else {
            selected
        };
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        actor.lock_is_terminal_handoff = false;
        if actor.attack_target() != selected {
            actor.laser_attack_count = 0;
        }
        actor.lock_target = selected;
        actor.fight_skill_search_target_time = SEARCH_TARGET_RESET_TICKS;
        actor.retarget_after_own_direct_kill = false;
        self.engage_wall_in_the_way(actor_id);
        Ok(())
    }

    fn quick_switch_active_target_outside_attack_area(
        &mut self,
        actor_id: u64,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        allow_phase_override: bool,
    ) -> Result<bool> {
        let actor = &self.actors[&actor_id];
        let Some(FightActorRef::Unit(target_id)) = actor.attack_target() else {
            return Ok(false);
        };
        let target = FightActorRef::Unit(target_id);
        let in_attacking_phase = actor.fight_skill_phase == FightSkillPhase::Attack
            || (actor.fight_skill_phase == FightSkillPhase::Idle
                && actor.motion == MotionState::Attacking
                && !self.bodyless_target_in_attack_range(actor_id, target));
        if (!allow_phase_override && !in_attacking_phase)
            || !actor.rules.has_body
            || !actor.rules.attack.quick_switch_target
            || actor.rules.attack.weapons.mode != WeaponMode::Normal
            || !actor.projectile_pending_releases.is_empty()
            || (!allow_phase_override
                && !self.fight_actor_is_alive(target)
                && actor.motion != MotionState::Attacking)
            || self.target_in_attack_area(actor_id, target)
        {
            return Ok(false);
        }
        let use_live_candidate_positions = {
            let target = self
                .fight_actor(target)
                .expect("lock target identity is stable");
            target.query_alive && !target.alive
        };
        let mut selected = self
            .select_normal_target_with_order(
                actor_id,
                target_search_order,
                use_live_candidate_positions,
            )
            .map_err(|error| Error::new(format!("logic step {step} actor {actor_id}: {error}")))?;
        if !use_live_candidate_positions
            && selected
                .and_then(|candidate| self.fight_actor(candidate))
                .is_some_and(|target| target.query_alive && !target.alive)
        {
            selected = self
                .select_normal_target_with_order(actor_id, target_search_order, true)
                .map_err(|error| {
                    Error::new(format!("logic step {step} actor {actor_id}: {error}"))
                })?;
        }
        let selected = selected.filter(|candidate| match candidate {
            FightActorRef::Unit(_) => self.target_in_attack_area(actor_id, *candidate),
            FightActorRef::Building(_) => false,
        });
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let entered_idle = match selected {
            Some(FightActorRef::Unit(target_id)) => {
                let target = FightActorRef::Unit(target_id);
                if actor.attack_target() != Some(target) {
                    actor.laser_attack_count = 0;
                }
                actor.lock_target = Some(target);
                false
            }
            Some(FightActorRef::Building(building_id)) => {
                actor.lock_target = Some(FightActorRef::Building(building_id));
                actor.lock_is_terminal_handoff = false;
                false
            }
            None => {
                actor.drop_lock();
                actor.motion = MotionState::Idle;
                actor.fight_skill_phase = FightSkillPhase::Idle;
                actor.pending = None;
                actor.next_target_x_q32 = actor.x_q32;
                actor.next_target_z_q32 = actor.z_q32;
                actor.next_speed_q32 = 0;
                actor.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
                true
            }
        };
        actor.fight_skill_search_target_time = SEARCH_TARGET_RESET_TICKS;
        actor.retarget_after_own_direct_kill = false;
        Ok(entered_idle)
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
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
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
        if self.hold_through_cooling(actor_id, step, target_search_order)? {
            return Ok(());
        }
        // A block that falls ends the attack on it, and the lock with it. The
        // tick after, the game reads the unit idle and without a lock, its
        // weapon still naming the block, and it looks for a target only from
        // there — so the next block is not engaged on the tick the last one
        // fell. A group drops its slots instead, which `drop_lock` does and
        // the Wraith was recorded doing, with no weapon left naming anything.
        let fallen = {
            let actor = &self.actors[&actor_id];
            match actor.attack_target() {
                Some(FightActorRef::Building(building))
                    if actor.group_skill_targets.is_empty()
                        && actor.attacked_wall == Some(building)
                        && actor.pending.is_none()
                        && actor
                            .backswing_finish_step
                            .is_none_or(|finish| finish < step)
                        && actor.in_the_way.is_some_and(|(wall, _)| wall == building)
                        && !self.fight_actor_is_alive(FightActorRef::Building(building)) =>
                {
                    Some(building)
                }
                _ => None,
            }
        };
        if let Some(building) = fallen {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.motion = MotionState::Idle;
            actor.drop_lock();
            // Only a unit with a body is read still aiming at the block: the
            // Marksman is, the Rhino and the Steel Balls read no weapon
            // target at all on the same tick.
            actor.fallen_attack_target = actor.rules.has_body.then_some(building);
            actor.fight_skill_phase = FightSkillPhase::Idle;
            // And it looks again on the very next tick, whatever its search
            // timer says: the Rhino takes the next block, or the unit behind a
            // wall it has broken through, one tick after going idle each time.
            actor.fight_skill_search_target_time = 0;
            actor.backswing_finish_step = None;
            actor.next_target_x_q32 = actor.x_q32;
            actor.next_target_z_q32 = actor.z_q32;
            actor.next_speed_q32 = 0;
            actor.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
            return Ok(());
        }
        // A block that comes into the way while an attack on a unit is being
        // prepared ends that attack. The Steel Ball of `wall-laser.yaml`
        // prepares a beam on the Marksman behind the wall for six ticks; on the
        // tick block 3 comes within the line it reads idle, with no lock and no
        // weapon target, and the tick after it is on the block. So the check
        // is asked while preparing too, and a block it finds is not fired at
        // until the unit has looked again.
        //
        // The same holds between two blows on a block, once the swing is over
        // and before the next begins: the skill is idle for that moment and
        // asks again, and a different block now nearer in the line ends the
        // attack on the old one. A Crawler of `wall-block.yaml`, pushed along
        // the wall while it strikes block 4, reads idle with no lock on the
        // tick its swing ends and is on block 3 the tick after.
        let interrupted = {
            let actor = &self.actors[&actor_id];
            let between_blows = actor.fight_skill_phase != FightSkillPhase::Idle
                && actor
                    .backswing_finish_step
                    .is_none_or(|finish| finish < step);
            match actor.lock_target {
                Some(target @ FightActorRef::Unit(_)) if actor.group_skill_targets.is_empty() => {
                    let held = actor
                        .in_the_way
                        .filter(|(_, found_for)| *found_for == target)
                        .map(|(wall, _)| wall);
                    if matches!(actor.fight_skill_phase, FightSkillPhase::Prepare { .. })
                        && held.is_none()
                    {
                        self.wall_in_the_way(actor_id, target).is_some()
                    } else if between_blows && let Some(held) = held {
                        self.wall_in_the_way(actor_id, target)
                            .is_some_and(|wall| wall != held)
                    } else {
                        false
                    }
                }
                _ => false,
            }
        };
        if interrupted {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.motion = MotionState::Idle;
            actor.drop_lock();
            actor.fight_skill_phase = FightSkillPhase::Idle;
            actor.fight_skill_search_target_time = 0;
            actor.backswing_finish_step = None;
            actor.pending = None;
            actor.next_target_x_q32 = actor.x_q32;
            actor.next_target_z_q32 = actor.z_q32;
            actor.next_speed_q32 = 0;
            actor.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
            return Ok(());
        }
        // A skill looks for its attack target every tick it is idle —
        // `SkillIdleState.TryPerform` reaches `SearchAttackTarget`, which asks
        // `WallConstructionTargetChecker` — while the mech's lock is searched
        // on its own ten-tick timer. So a wall that comes into reach between
        // two lock searches is engaged the tick it does.
        if self.actors[&actor_id].fight_skill_phase == FightSkillPhase::Idle {
            self.engage_wall_in_the_way(actor_id);
        }
        if matches!(
            self.actors[&actor_id].lock_target,
            Some(FightActorRef::Building(_))
        ) && self.actors[&actor_id].lock_is_terminal_handoff
        {
            // The native terminal handoff exposes the defeated team's first
            // core building for one tick. The following update consumes the
            // already published displacement, then the tower teardown clears
            // the transient lock before the terminal snapshot is written.
            let clear_velocity = self.terminal_drain_pending;
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.motion = MotionState::Idle;
            actor.drop_lock();
            actor.lock_is_terminal_handoff = false;
            actor.fight_skill_phase = FightSkillPhase::Idle;
            if clear_velocity {
                actor.current_velocity_x_q32 = 0;
                actor.current_velocity_z_q32 = 0;
            }
            actor.next_target_x_q32 = actor.x_q32;
            actor.next_target_z_q32 = actor.z_q32;
            actor.next_speed_q32 = 0;
            actor.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
            return Ok(());
        }
        self.update_group_skill_targets(actor_id, step, target_search_order)?;
        self.refresh_group_walls(actor_id);
        let completed_laser_kill = {
            let actor = &self.actors[&actor_id];
            actor.retarget_after_own_direct_kill
                && matches!(actor.rules.attack.path, AttackPath::Laser { .. })
                && actor
                    .attack_target()
                    .is_some_and(|target| !self.fight_actor_is_alive(target))
        };
        if completed_laser_kill {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.drop_lock();
            actor.fight_skill_phase = FightSkillPhase::Idle;
            actor.laser_attack_count = 0;
            actor.retarget_after_own_direct_kill = false;
            return Ok(());
        }
        let stale_attack_target = {
            let actor = &self.actors[&actor_id];
            if actor.motion == MotionState::Attacking
                && !actor.motion_attack_hold_fire
                && actor.pending.is_none()
                && actor.backswing_finish_step.is_none()
                && actor
                    .attack_target()
                    .is_some_and(|target| !self.fight_actor_is_alive(target))
            {
                actor.attack_target()
            } else {
                None
            }
        };
        if stale_attack_target.is_some() && !self.actors[&actor_id].rules.attack.quick_switch_target
        {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.motion = MotionState::Idle;
            actor.drop_lock();
            actor.fight_skill_phase = FightSkillPhase::Idle;
            actor.next_target_x_q32 = actor.x_q32;
            actor.next_target_z_q32 = actor.z_q32;
            actor.next_speed_q32 = 0;
            actor.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
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
                && actor.attack_target().is_some_and(|target_id| {
                    self.bodyless_target_in_attack_area(actor_id, target_id)
                })
        };
        if bodyless_skill_starts_before_idle_search {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            let prepare_steps = native_time_units_to_steps(actor.rules.attack.prepare_time_units());
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
                .attack_target()
                .is_some_and(|target| !self.fight_actor_is_alive(target));
        let dead_backswing_just_finished = backswing_just_finished
            && self.actors[&actor_id]
                .attack_target()
                .is_some_and(|target| !self.fight_actor_is_alive(target));
        let deferred_projectile_burst_finish = {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            std::mem::replace(&mut actor.projectile_burst_finished, false)
        };
        let deferred_same_tick_dead_burst_finish = {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            std::mem::replace(&mut actor.projectile_burst_finished_same_tick_dead, false)
        };
        if deferred_same_tick_dead_burst_finish
            && self.quick_switch_active_target_outside_attack_area(
                actor_id,
                step,
                target_search_order,
                true,
            )?
        {
            return Ok(());
        }
        let deferred_target_outside_attack_area = deferred_projectile_burst_finish
            && self.actors[&actor_id]
                .attack_target()
                .is_none_or(|target_id| !self.target_in_attack_area(actor_id, target_id));
        if deferred_target_outside_attack_area
            && self.quick_switch_active_target_outside_attack_area(
                actor_id,
                step,
                target_search_order,
                true,
            )?
        {
            return Ok(());
        }
        let force_burst_finish_target_search = deferred_projectile_burst_finish
            && self.actors[&actor_id]
                .attack_target()
                .is_none_or(|target_id| !self.target_in_attack_area(actor_id, target_id));
        if force_burst_finish_target_search {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.fight_skill_phase = FightSkillPhase::Idle;
            actor.fight_skill_search_target_time = 0;
        }
        let active_projectile_burst_lost_target = {
            let actor = &self.actors[&actor_id];
            !actor.projectile_pending_releases.is_empty()
                && actor
                    .attack_target()
                    .is_some_and(|target| !self.fight_actor_is_alive(target))
        };
        if active_projectile_burst_lost_target {
            let owner_team = self.actors[&actor_id].placement.team;
            let has_alive_enemy = self
                .actors
                .values()
                .any(|actor| actor.placement.team != owner_team && actor.alive());
            if !has_alive_enemy {
                let actor = self
                    .actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable");
                actor.motion = MotionState::Idle;
                actor.projectile_pending_releases.clear();
                actor.projectile_burst_finished = false;
                actor.projectile_burst_finished_same_tick_dead = false;
                actor.next_target_x_q32 = actor.x_q32;
                actor.next_target_z_q32 = actor.z_q32;
                actor.next_speed_q32 = 0;
                actor.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
                return Ok(());
            }
            let due = {
                let actor = self
                    .actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable");
                actor.motion = MotionState::Idle;
                actor.next_target_x_q32 = actor.x_q32;
                actor.next_target_z_q32 = actor.z_q32;
                actor.next_speed_q32 = 0;
                actor.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
                let mut due = Vec::new();
                actor.projectile_pending_releases.retain(|pending| {
                    if pending.step <= step {
                        due.push(*pending);
                        false
                    } else {
                        true
                    }
                });
                if !due.is_empty() && actor.projectile_pending_releases.is_empty() {
                    actor.projectile_burst_finished = true;
                    actor.projectile_burst_finished_same_tick_dead = true;
                }
                due
            };
            for pending in due {
                self.release_pending_projectile(actor_id, pending, events)?;
            }
            return Ok(());
        }
        if self.quick_switch_active_target_outside_attack_area(
            actor_id,
            step,
            target_search_order,
            false,
        )? {
            return Ok(());
        }
        self.update_fight_skill_target_search(actor_id, step, target_search_order)?;
        if quick_switch_backswing_due && !quick_switch_dead_backswing_due {
            self.actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .backswing_finish_step = None;
        }
        let stale_replacement = if stale_attack_target.is_some()
            || quick_switch_dead_backswing_due
            || dead_backswing_just_finished
        {
            self.actors[&actor_id].attack_target()
        } else {
            None
        };
        let stale_replacement_outside_attack_area = stale_replacement
            .is_some_and(|target_id| !self.target_in_attack_area(actor_id, target_id));
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
            actor.drop_lock();
            actor.fight_skill_phase = FightSkillPhase::Idle;
            actor.backswing_finish_step = None;
            actor.retarget_after_own_direct_kill = false;
            if entered_idle {
                actor.next_target_x_q32 = actor.x_q32;
                actor.next_target_z_q32 = actor.z_q32;
            }
            actor.next_speed_q32 = 0;
            actor.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
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
        let bodyful_quick_switch_target = {
            let actor = &self.actors[&actor_id];
            (actor.rules.has_body
                && actor.rules.attack.quick_switch_target
                && actor.pending.is_some())
            .then_some(actor.attack_target())
            .flatten()
            .filter(|&target_id| self.target_in_attack_area(actor_id, target_id))
        };
        if let Some(target_id) = bodyful_quick_switch_target {
            self.actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .pending
                .as_mut()
                .expect("pending attack identity is stable")
                .target = target_id;
        }
        let active_attack_rejected = self.actors[&actor_id]
            .pending
            .is_some_and(|pending| self.bodyless_attackable_invalid(actor_id, pending.target));
        if active_attack_rejected {
            // Build 2259 SkillPrepareState and SkillAttackState both run
            // CheckAttackable before advancing their current attack phase.
            // A failed check enters SkillIdleState in the same update.
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.motion = MotionState::Idle;
            actor.drop_lock();
            actor.pending = None;
            actor.fight_skill_phase = FightSkillPhase::Idle;
            actor.next_target_x_q32 = actor.x_q32;
            actor.next_target_z_q32 = actor.z_q32;
            actor.next_speed_q32 = 0;
            actor.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
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
        if released_this_step && self.actors[&actor_id].motion != MotionState::Attacking {
            return Ok(());
        }
        let projectile_releases = {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            let mut due = Vec::new();
            actor.projectile_pending_releases.retain(|pending| {
                if pending.step <= step {
                    due.push(*pending);
                    false
                } else {
                    true
                }
            });
            let burst_finished = !due.is_empty() && actor.projectile_pending_releases.is_empty();
            if burst_finished {
                actor.projectile_burst_finished = true;
                actor.projectile_burst_finished_same_tick_dead = false;
            }
            due
        };
        for pending in projectile_releases {
            self.release_pending_projectile(actor_id, pending, events)?;
        }
        let group_core_target = {
            let actor = &self.actors[&actor_id];
            (actor.rules.attack.weapons.mode == WeaponMode::Group
                && actor.motion == MotionState::Attacking
                && !actor.motion_attack_hold_fire
                && actor.pending.is_none()
                && actor.backswing_finish_step.is_none()
                && actor.fight_skill_phase == FightSkillPhase::Attack
                && actor
                    .group_skill_prepare_ready_steps
                    .first()
                    .is_none_or(|ready_step| *ready_step <= step)
                && step >= actor.next_attack_step)
                .then(|| actor.mechanical_attack_target())
                .flatten()
        };
        if let Some(target_id) = group_core_target
            && self.target_in_attack_area(actor_id, target_id)
        {
            let next_attack_step = self.sample_actor_attack_interval(actor_id, step)?;
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.next_attack_step = next_attack_step;
            actor.pending = Some(PendingRelease {
                step,
                target: target_id,
            });
            if let FightActorRef::Building(building) = target_id {
                actor.attacked_wall = Some(building);
            }
            let _attack_point_rejected = self.release(actor_id, events)?;
        }
        let group_releases = {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            let mut due = Vec::new();
            actor
                .group_pending_releases
                .retain(|&(skill_index, pending)| {
                    if pending.step <= step {
                        due.push((skill_index, pending));
                        false
                    } else {
                        true
                    }
                });
            for skill_index in 1..actor.group_skill_targets.len() {
                let next_attack_step = actor.group_skill_next_attack_steps[skill_index];
                let prepare_ready_step = actor.group_skill_prepare_ready_steps[skill_index];
                if next_attack_step > 0
                    && next_attack_step <= step
                    && prepare_ready_step <= step
                    && let Some(target) = actor.group_attack_target(skill_index)
                {
                    due.push((skill_index, PendingRelease { step, target }));
                }
            }
            // A release queued when its slot was allocated names the unit it
            // was allocated. It fires at what that slot fires at now, which is
            // a construction in its way if one has been found since: the slot
            // still holds the same unit, and only what it shoots has changed.
            for (skill_index, pending) in &mut due {
                if pending.target.unit_id()
                    == actor
                        .group_skill_targets
                        .get(*skill_index)
                        .copied()
                        .flatten()
                    && let Some(target) = actor.group_attack_target(*skill_index)
                {
                    pending.target = target;
                }
            }
            due.sort_by_key(|&(skill_index, _)| skill_index);
            due
        };
        for (skill_index, pending) in group_releases {
            match pending.target {
                FightActorRef::Unit(target_id)
                    if self.actors.get(&target_id).is_some_and(Actor::alive) =>
                {
                    self.refresh_group_skill_attack_interval(actor_id, skill_index, step)?;
                    self.release_projectile(actor_id, target_id, skill_index, skill_index, events)?;
                }
                // A slot whose line of fire a construction stands in fires at
                // the construction, as the core does.
                FightActorRef::Building(building_id) => {
                    let Some((x_q32, z_q32, radius)) = self
                        .buildings
                        .iter()
                        .find(|building| {
                            building.building_id == building_id && building_alive(building)
                        })
                        .map(|building| {
                            (
                                building.position.x,
                                building.position.z,
                                building_radius(building),
                            )
                        })
                    else {
                        continue;
                    };
                    self.refresh_group_skill_attack_interval(actor_id, skill_index, step)?;
                    self.release_projectile_to(
                        actor_id,
                        ObjectKind::Building,
                        building_id,
                        q32_to_space_rounded(x_q32),
                        0,
                        q32_to_space_rounded(z_q32),
                        x_q32,
                        z_q32,
                        radius,
                        skill_index,
                        skill_index,
                        events,
                    )?;
                }
                FightActorRef::Unit(_) => {}
            }
        }
        let lock_target = self.actors[&actor_id].mechanical_attack_target();
        if let Some(target) = lock_target {
            let target_alive = self.fight_actor_is_alive(target);
            if !target_alive && self.actors[&actor_id].backswing_finish_step.is_some() {
                // Build 2259 enters idle but retains the dead target through
                // the remaining backswing even when an ally dealt the kill.
                // MotionIdleState.Enter publishes StopMove once; its Update
                // does not refresh that target on every remaining backswing
                // tick.
                // A felled block is held differently: the Rhino of
                // `wall-rhino.yaml` reads attacking, still on the block, until
                // its swing is over, and only then goes idle.
                let holds_a_block = matches!(target, FightActorRef::Building(_));
                // And keeps turning to it: the Crawlers of `wall-block.yaml`
                // that fell block 5 face it a little more each tick of their
                // swing, as they did while it stood.
                // A tower the match's end tears down is not turned to.
                let holds_a_wall = matches!(target, FightActorRef::Building(building)
                    if self.actors[&actor_id].in_the_way.is_some_and(|(wall, _)| wall == building));
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
                let entered_idle = actor.motion != MotionState::Idle && !holds_a_block;
                if !holds_a_block {
                    actor.motion = MotionState::Idle;
                }
                if entered_idle {
                    actor.next_target_x_q32 = actor.x_q32;
                    actor.next_target_z_q32 = actor.z_q32;
                }
                actor.next_speed_q32 = 0;
                actor.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
                return Ok(());
            }
            if !target_alive && backswing_just_finished {
                let actor = self
                    .actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable");
                let entered_idle = actor.motion != MotionState::Idle;
                actor.drop_lock();
                actor.retarget_after_own_direct_kill = false;
                actor.motion = MotionState::Idle;
                if entered_idle {
                    actor.next_target_x_q32 = actor.x_q32;
                    actor.next_target_z_q32 = actor.z_q32;
                }
                actor.next_speed_q32 = 0;
                actor.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
                return Ok(());
            }
            if !target_alive {
                self.actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable")
                    .drop_lock();
            }
        }
        let target = self.actors[&actor_id].mechanical_attack_target();
        let Some(target) = target else {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.motion = MotionState::Idle;
            actor.next_target_x_q32 = actor.x_q32;
            actor.next_target_z_q32 = actor.z_q32;
            actor.next_speed_q32 = 0;
            actor.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
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
            && actor.motion == MotionState::Attacking
            && actor
                .group_skill_targets
                .first()
                .is_some_and(Option::is_none)
            && actor
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
                actor.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
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
                    actor.drop_lock();
                    actor.fight_skill_phase = FightSkillPhase::Idle;
                    actor.next_target_x_q32 = actor.x_q32;
                    actor.next_target_z_q32 = actor.z_q32;
                    actor.next_speed_q32 = 0;
                    actor.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
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
                            AttackPath::Projectile { .. }
                                | AttackPath::Direct { melee: true }
                                | AttackPath::Laser { .. }
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
                    actor.drop_lock();
                    actor.fight_skill_phase = FightSkillPhase::Idle;
                    actor.next_target_x_q32 = actor.x_q32;
                    actor.next_target_z_q32 = actor.z_q32;
                    actor.next_speed_q32 = 0;
                    actor.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
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
                    let interval_steps = native_time_units_to_steps(actor.stats.attack_interval());
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
                    actor.current_attack_interval = sampled;
                    let attack_point_steps =
                        native_time_units_to_steps(actor.rules.attack.attack_point_time_units());
                    actor.pending = Some(PendingRelease {
                        step: step.saturating_add(attack_point_steps),
                        target,
                    });
                    if let FightActorRef::Building(building) = target {
                        actor.attacked_wall = Some(building);
                    }
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
            if self.actors[&actor_id].motion != MotionState::Attacking {
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
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            // FightSkill.Update rotates every free weapon after its state controller.
            actor.rotate_weapons_towards(target_rotation_q32);
            if actor.rules.has_body {
                actor.aim_rotation = degrees_q32_to_mdeg(
                    actor
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
        if actor.rules.attack.weapons.mode == WeaponMode::Group
            && actor.motion == MotionState::Attacking
            && actor
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
            actor.drop_lock();
            actor.fight_skill_phase = FightSkillPhase::Idle;
            actor.motion_attack_hold_fire = false;
            actor.next_target_x_q32 = actor.x_q32;
            actor.next_target_z_q32 = actor.z_q32;
            actor.next_speed_q32 = 0;
            actor.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
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
            actor.drop_lock();
            actor.fight_skill_phase = FightSkillPhase::Idle;
            actor.next_target_x_q32 = actor.x_q32;
            actor.next_target_z_q32 = actor.z_q32;
            actor.next_speed_q32 = 0;
            actor.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
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
            actor.drop_lock();
            actor.fight_skill_phase = FightSkillPhase::Idle;
            actor.next_target_x_q32 = actor.x_q32;
            actor.next_target_z_q32 = actor.z_q32;
            actor.next_speed_q32 = 0;
            actor.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
            return Ok(());
        }
        let entered_move_from_idle = actor.motion == MotionState::Idle;
        let entered_move = actor.motion != MotionState::Moving;
        let entered_move_below_min_range =
            entered_move && edge_distance_q32 < space_to_q32(actor.rules.attack.min_range());
        if !entered_move_from_idle && !entered_move_below_min_range {
            // FightSkill.Update tracks an existing target before MotionController updates movement.
            // A target acquired by MotionIdleState is not visible to FightSkill until the next tick.
            actor.rotate_weapons_towards(target_rotation_q32);
            if actor.rules.has_body {
                actor.aim_rotation = degrees_q32_to_mdeg(
                    actor
                        .weapon_rotations_q32
                        .first()
                        .copied()
                        .unwrap_or(actor.body_rotation_q32),
                );
            }
        }
        actor.motion = MotionState::Moving;
        actor.motion_attack_hold_fire = false;
        if entered_move {
            return Ok(());
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
            space_to_q32(actor.stats.move_speed()),
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
                    FightActorRef::Unit(actor_id),
                    changed.2,
                    changed.3,
                    changed.4,
                );
        }
    }

    #[allow(clippy::too_many_lines)]
    fn step_rvo(&mut self) {
        self.rvo_counter += 1;
        if self.rvo_counter < 4 {
            return;
        }
        self.rvo_counter = 0;
        let first_tree = self.rvo_first_tree_pending;
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
                passable_by_own_group: false,
                locked: false,
                tree_position: if first_tree {
                    FixedVec2::ZERO
                } else {
                    rvo_position(actor.rvo_tree_x_q32, actor.rvo_tree_z_q32)
                },
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
        self.rvo_first_tree_pending = false;
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
    }

    fn quick_switch_bodyless_pending_target(
        &mut self,
        actor_id: u64,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<()> {
        let Some(dead_target_id) = self.actors[&actor_id]
            .pending
            .and_then(|pending| pending.target.unit_id())
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
            .map_err(|error| Error::new(format!("logic step {step} actor {actor_id}: {error}")))?;
        if !target_died_during_tick
            && selected
                .and_then(|target_id| self.actors.get(&target_id))
                .is_some_and(|target| target.target_query_alive && !target.alive())
        {
            selected = self
                .select_normal_unit_target_with_order(actor_id, target_search_order, true)
                .map_err(|error| {
                    Error::new(format!("logic step {step} actor {actor_id}: {error}"))
                })?;
        }
        let Some(selected) = selected.filter(|&target_id| {
            self.bodyless_target_in_attack_area(actor_id, FightActorRef::Unit(target_id))
        }) else {
            return Ok(());
        };
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        actor.lock_target = Some(FightActorRef::Unit(selected));
        actor
            .pending
            .as_mut()
            .expect("pending attack identity is stable")
            .target = FightActorRef::Unit(selected);
        Ok(())
    }

    fn bodyless_target_in_attack_area(&self, actor_id: u64, target: FightActorRef) -> bool {
        self.bodyless_target_in_attack_range(actor_id, target)
            && self.bodyless_target_in_attack_angle(actor_id, target)
    }

    fn target_in_attack_area(&self, actor_id: u64, target: FightActorRef) -> bool {
        if !self.bodyless_target_in_attack_range(actor_id, target) {
            return false;
        }
        let actor = &self.actors[&actor_id];
        if !actor.rules.has_body {
            return self.bodyless_target_in_attack_angle(actor_id, target);
        }
        let target = self.fight_actor(target).expect("target identity is stable");
        actor.weapons_in_attack_angle(direction_degrees_q32_raw(
            target.x_q32.saturating_sub(actor.x_q32),
            target.z_q32.saturating_sub(actor.z_q32),
        ))
    }

    fn bodyless_target_in_attack_range(&self, actor_id: u64, target: FightActorRef) -> bool {
        let actor = &self.actors[&actor_id];
        let Some(target) = self.fight_actor(target) else {
            return false;
        };
        if !target.alive || !target.targetable {
            return false;
        }
        let center_distance_q32 = native_q32_magnitude(
            target.x_q32.saturating_sub(actor.x_q32),
            target.z_q32.saturating_sub(actor.z_q32),
        );
        let edge_distance_q32 = center_distance_q32
            .saturating_sub(space_to_q32(actor.rules.collision_radius()))
            .saturating_sub(space_to_q32(target.radius))
            .max(0);
        edge_distance_q32 >= space_to_q32(actor.rules.attack.min_range())
            && edge_distance_q32 <= space_to_q32(actor.stats.attack_range())
    }

    fn bodyless_target_in_attack_angle(&self, actor_id: u64, target: FightActorRef) -> bool {
        let actor = &self.actors[&actor_id];
        let Some(target) = self.fight_actor(target) else {
            return false;
        };
        target.alive
            && rotation_distance_q32(
                actor.body_rotation_q32,
                direction_degrees_q32_raw(
                    target.x_q32.saturating_sub(actor.x_q32),
                    target.z_q32.saturating_sub(actor.z_q32),
                ),
            ) <= mdeg_to_degrees_q32(actor.rules.attack.attack_half_angle_mdeg())
    }

    fn bodyless_attackable_invalid(&self, actor_id: u64, target: FightActorRef) -> bool {
        let actor = &self.actors[&actor_id];
        if actor.rules.has_body {
            return false;
        }
        !self.bodyless_target_in_attack_area(actor_id, target)
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

    fn start_projectile_burst(
        &mut self,
        actor_id: u64,
        target: FightActorRef,
        step: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            // A cooling of nothing holds nothing: the Stormcaller's is 0,
            // and its regressions, the game's, never hold.
            if actor.rules.has_body
                && actor.rules.attack.quick_switch_target
                && actor.rules.attack.weapons.mode == WeaponMode::Normal
                && actor.rules.attack.cooling_time_units() > 0
            {
                // The attack is still running for its attack point after the
                // release; a target that dies in that time is held for.
                let attack_point_steps =
                    native_time_units_to_steps(actor.rules.attack.attack_point_time_units());
                actor.cooling_until_step =
                    Some(step.saturating_add(attack_point_steps.saturating_sub(1)));
                actor.cooling_hold = None;
                actor.cooling_candidate = None;
            }
        }
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

    fn projectile_target_offsets(
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

    fn release_projectile(
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

    fn release_pending_projectile(
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
    fn release_projectile_at(
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
    fn release_projectile_to(
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

    fn refresh_group_skill_attack_interval(
        &mut self,
        actor_id: u64,
        skill_index: usize,
        step: u64,
    ) -> Result<()> {
        // What a recording reads as the unit's current interval is its core
        // skill's, so a child slot's draw does not replace it.
        let core_interval = self.actors[&actor_id].current_attack_interval;
        let sampled_step = self.sample_actor_attack_interval(actor_id, step)?;
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("group skill owner identity is stable");
        if skill_index != 0 {
            actor.current_attack_interval = core_interval;
        }
        let next_attack_step = actor
            .group_skill_next_attack_steps
            .get_mut(skill_index)
            .ok_or_else(|| Error::new("group skill index is absent"))?;
        *next_attack_step = sampled_step;
        Ok(())
    }

    /// Schedules the next attack and remembers the interval it used.
    fn sample_actor_attack_interval(&mut self, actor_id: u64, step: u64) -> Result<u64> {
        let actor = self
            .actors
            .get(&actor_id)
            .ok_or_else(|| Error::new("attack interval owner is absent"))?;
        let interval_steps = native_time_units_to_steps(actor.stats.attack_interval());
        let offset_steps =
            native_time_units_to_steps(actor.rules.attack.interval_offset_time_units());
        let team = actor.placement.team;
        let sample = if offset_steps == 0 {
            0
        } else {
            i64::from(
                self.team_random
                    .get_mut(&team)
                    .ok_or_else(|| Error::new("group skill team random stream is absent"))?
                    .next_in_range(i32::try_from(offset_steps).unwrap_or(i32::MAX)),
            )
        };
        let sampled = i64::try_from(interval_steps)
            .unwrap_or(i64::MAX)
            .saturating_add(sample)
            .max(1)
            .cast_unsigned();
        if let Some(actor) = self.actors.get_mut(&actor_id) {
            actor.current_attack_interval = sampled;
        }
        Ok(step.saturating_add(sampled))
    }

    /// Every object one hit strikes, in the order it strikes them.
    ///
    /// The build's `DamagePerformer.PrepareRangeTargets`: what the hit was
    /// aimed at, if it strikes that wherever it stands, and every enemy its
    /// splash reaches. It is the one place this simulator decides who a hit
    /// lands on, which is why what it does not yet decide is refused here and
    /// nowhere else.
    ///
    /// # Errors
    ///
    /// Refuses a splash that has not been measured: one reaching a building
    /// from a hit aimed at a unit, and one reaching a unit from a hit aimed at
    /// a building.
    fn damage_targets(&self, hit: &DamageHit) -> Result<Vec<FightActorRef>> {
        let (center_x, center_z) = hit.center;
        if let FightActorRef::Building(building_id) = hit.aimed {
            let building = self
                .buildings
                .iter()
                .find(|building| building.building_id == building_id)
                .ok_or_else(|| Error::new("damage target building is absent"))?;
            let reached = magnitude(
                building_x(building).saturating_sub(center_x),
                building_z(building).saturating_sub(center_z),
            ) <= building_radius(building).saturating_add(hit.splash_radius);
            let mut struck = Vec::new();
            if reached {
                struck.push(hit.aimed);
            }
            if hit.splash_radius == 0 || !hit.reach.touches(UnitDomain::Ground) {
                return Ok(struck);
            }
            // A shot at a building splashes the enemy buildings around it, for
            // its full damage and after the building it was aimed at: a
            // Wraith's shot at one block of a wall reads 381 on that block,
            // then 381 on the next one, whose edge is exactly its 8 metres of
            // splash away.
            struck.extend(
                self.buildings
                    .iter()
                    .filter(|other| {
                        other.building_id != building_id
                            && building_alive(other)
                            && other.targetable
                            && other.team_id != hit.team
                            && magnitude(
                                building_x(other).saturating_sub(center_x),
                                building_z(other).saturating_sub(center_z),
                            )
                            .saturating_sub(building_radius(other))
                                <= hit.splash_radius
                    })
                    .map(|other| FightActorRef::Building(other.building_id)),
            );
            // Whether it takes a unit standing by the building too has not been
            // recorded.
            let unit_in_reach = self.actors.values().any(|unit| {
                unit.alive()
                    && unit.placement.team != hit.team
                    && hit.reach.touches(unit.rules.domain)
                    && magnitude(
                        unit.x.saturating_sub(center_x),
                        unit.z.saturating_sub(center_z),
                    )
                    .saturating_sub(unit.rules.collision_radius())
                        <= hit.splash_radius
            });
            if unit_in_reach {
                return Err(Error::new(
                    "splash from a shot at a building onto a unit is not closed",
                ));
            }
            return Ok(struck);
        }
        let units = self
            .target_search_order()
            .into_values()
            .flatten()
            .filter(|candidate_ref| {
                let FightActorRef::Unit(candidate_id) = *candidate_ref else {
                    return false;
                };
                let candidate = &self.actors[&candidate_id];
                candidate.alive()
                    && candidate.placement.team != hit.team
                    && hit.reach.touches(candidate.rules.domain)
                    && ((hit.hits_aimed && *candidate_ref == hit.aimed)
                        || (hit.splash_radius > 0
                            && magnitude(
                                candidate.x.saturating_sub(center_x),
                                candidate.z.saturating_sub(center_z),
                            )
                            .saturating_sub(candidate.rules.collision_radius())
                                <= hit.splash_radius))
            })
            .collect::<Vec<_>>();
        if hit.splash_radius > 0
            && hit.reach.touches(UnitDomain::Ground)
            && self.buildings.iter().any(|building| {
                building_alive(building)
                    && building.targetable
                    && building.team_id != hit.team
                    && magnitude(
                        building_x(building).saturating_sub(center_x),
                        building_z(building).saturating_sub(center_z),
                    )
                    .saturating_sub(building_radius(building))
                        <= hit.splash_radius
            })
        {
            return Err(Error::new("splash against a building is not closed"));
        }
        Ok(units)
    }

    /// Takes one hit's damage off one target, unit or building alike.
    ///
    /// The one place this simulator takes life away. A unit remembers who hurt
    /// it and leaves the fight when it dies; a building stops being a target
    /// when it falls.
    fn strike(
        &mut self,
        target: FightActorRef,
        source: ObjectRef,
        source_team: u32,
        amount: i64,
    ) -> Result<Stroke> {
        match target {
            FightActorRef::Unit(unit_id) => {
                let unit = self
                    .actors
                    .get_mut(&unit_id)
                    .ok_or_else(|| Error::new("damage target unit is absent"))?;
                let previous_life = unit.life;
                unit.life = unit.life.saturating_sub(amount).max(0);
                let actual = previous_life - unit.life;
                if actual > 0 {
                    unit.last_damage_source = Some((source, source_team));
                }
                let death = (unit.life == 0).then(|| {
                    let position = QVec3 {
                        x: unit.x_q32,
                        y: space_to_q32(unit_height(unit.rules.domain)),
                        z: unit.z_q32,
                    };
                    unit.exit_fight_on_death();
                    position
                });
                Ok(Stroke {
                    actual,
                    death,
                    fallen: None,
                })
            }
            FightActorRef::Building(building_id) => {
                let building = self
                    .buildings
                    .iter_mut()
                    .find(|building| building.building_id == building_id)
                    .ok_or_else(|| Error::new("damage target building is absent"))?;
                let damage =
                    i32::try_from(amount).map_err(|_| Error::new("building damage exceeds i32"))?;
                let previous_life = building.life.current;
                building.life.current = building.life.current.saturating_sub(damage).max(0);
                let destroyed = previous_life > 0 && building.life.current == 0;
                if destroyed {
                    building.targetable = false;
                }
                Ok(Stroke {
                    actual: i64::from(previous_life - building.life.current),
                    death: None,
                    fallen: destroyed.then_some(building.position),
                })
            }
        }
    }

    /// Resolves one hit against everything it strikes.
    ///
    /// `damage` is recorded here, once per object that lost life, in the order
    /// the objects were struck. Deaths and fallen buildings are handed back:
    /// each way of dealing damage records them where it records them.
    fn perform_damage(&mut self, hit: DamageHit, events: &mut Vec<Event>) -> Result<Struck> {
        let mut struck = Struck::default();
        for target in self.damage_targets(&hit)? {
            let stroke = self.strike(target, hit.source, hit.source_team, hit.amount)?;
            if stroke.actual > 0 {
                events.push(event(
                    None,
                    Some(hit.source),
                    Some(hit.source_team),
                    Some(target.object_ref()),
                    EventPayload::Damage {
                        amount: i32::try_from(stroke.actual)
                            .map_err(|_| Error::new("damage exceeds i32"))?,
                    },
                ));
            }
            let id = match target {
                FightActorRef::Unit(id) | FightActorRef::Building(id) => id,
            };
            if let Some(position) = stroke.death {
                struck.deaths.push((id, position));
            }
            if let Some(position) = stroke.fallen {
                struck.fallen.push((id, position));
            }
        }
        Ok(struck)
    }

    /// Records the units a hit killed, each credited to whoever last hurt it.
    fn record_deaths(&self, deaths: Vec<(u64, QVec3)>, events: &mut Vec<Event>) {
        for (dead_id, position) in deaths {
            let (source, source_team_id) = self.actors[&dead_id]
                .last_damage_source
                .map_or((None, None), |(source, team_id)| {
                    (Some(source), Some(team_id))
                });
            events.push(event(
                Some(ObjectRef::new(ObjectKind::Unit, dead_id)),
                source,
                source_team_id,
                None,
                EventPayload::UnitDied { position },
            ));
        }
    }

    fn direct_effect(
        &mut self,
        actor_id: u64,
        target: FightActorRef,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let attacker = &self.actors[&actor_id];
        let center = match target {
            FightActorRef::Unit(target_id) => {
                let aimed = &self.actors[&target_id];
                (aimed.x, aimed.z)
            }
            FightActorRef::Building(building_id) => self
                .buildings
                .iter()
                .find(|building| building.building_id == building_id)
                .map(|building| (building_x(building), building_z(building)))
                .ok_or_else(|| Error::new("direct attack target is absent"))?,
        };
        let hit = DamageHit {
            source: attacker.object_ref(),
            source_team: attacker.placement.team,
            team: attacker.placement.team,
            amount: attacker.stats.attack_damage(),
            aimed: target,
            hits_aimed: true,
            center,
            splash_radius: attacker.rules.attack.splash_radius(),
            reach: Reach::Targets(attacker.rules.attack.targets),
        };
        let struck = self.perform_damage(hit, events)?;
        self.record_deaths(struck.deaths, events);
        // A block a blow fells falls after every hit the tick resolves, as a
        // shot's does: the Crawlers of `wall-block.yaml` read three more blows
        // between the one that fells block 5 and `building_destroyed`.
        for (building_id, position) in struck.fallen {
            self.fallen_buildings.push(event(
                Some(ObjectRef::new(ObjectKind::Building, building_id)),
                None,
                None,
                None,
                EventPayload::BuildingDestroyed { position },
            ));
        }
        Ok(())
    }

    fn laser_effect(
        &mut self,
        actor_id: u64,
        target: FightActorRef,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let (damage, attacker_ref, attacker_team) = {
            let attacker = &self.actors[&actor_id];
            (
                attacker
                    .rules
                    .attack
                    .laser_damage(attacker.laser_attack_count),
                attacker.object_ref(),
                attacker.placement.team,
            )
        };
        self.actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .laser_attack_count += 1;
        // A laser strikes one target and has no splash, so it takes the
        // stroke without the range step. A unit it kills is recorded dead
        // before the damage, and a block it fells falls after it: the Steel
        // Balls of `wall-laser.yaml` read `damage` and then
        // `building_destroyed`. Damage is recorded even when none was dealt.
        let stroke = self.strike(target, attacker_ref, attacker_team, damage)?;
        if let Some(position) = stroke.death {
            events.push(event(
                Some(target.object_ref()),
                Some(attacker_ref),
                Some(attacker_team),
                None,
                EventPayload::UnitDied { position },
            ));
        }
        events.push(event(
            None,
            Some(attacker_ref),
            Some(attacker_team),
            Some(target.object_ref()),
            EventPayload::Damage {
                amount: i32::try_from(stroke.actual)
                    .map_err(|_| Error::new("laser damage exceeds i32"))?,
            },
        ));
        if let Some(position) = stroke.fallen {
            events.push(event(
                Some(target.object_ref()),
                None,
                None,
                None,
                EventPayload::BuildingDestroyed { position },
            ));
        }
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
            if projectile.target_kind == ObjectKind::Unit
                && projectile.lock_target
                && let Some(target) = self
                    .actors
                    .get(&projectile.target)
                    .filter(|actor| actor.alive())
            {
                projectile.cached_target_x = target.x;
                projectile.cached_target_y = unit_height(target.rules.domain);
                projectile.cached_target_z = target.z;
                projectile.cached_target_x_q32 = target.x_q32;
                projectile.cached_target_y_q32 = space_to_q32(projectile.cached_target_y);
                projectile.cached_target_z_q32 = target.z_q32;
                projectile.cached_target_radius = target.rules.collision_radius();
            }
            let dx_q32 = projectile
                .cached_target_x_q32
                .saturating_sub(projectile.x_q32);
            let dz_q32 = projectile
                .cached_target_z_q32
                .saturating_sub(projectile.z_q32);
            let dy_q32 = projectile
                .cached_target_y_q32
                .saturating_sub(projectile.y_q32);
            let distance_q32 = native_q32_magnitude_3d(dx_q32, dy_q32, dz_q32);
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
                    projectile.y_q32 = projectile
                        .y_q32
                        .saturating_add(q32_mul(q32_mul(dy_q32, reciprocal), move_q32));
                    projectile.z_q32 = projectile
                        .z_q32
                        .saturating_add(q32_mul(q32_mul(dz_q32, reciprocal), move_q32));
                }
                projectile.x = q32_to_space_rounded(projectile.x_q32);
                projectile.y = q32_to_space_rounded(projectile.y_q32);
                projectile.z = q32_to_space_rounded(projectile.z_q32);
                retained.push(projectile);
            }
        }
        retained.reverse();
        self.projectiles = retained;
        events.append(&mut self.fallen_buildings);
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn impact(&mut self, projectile: &Projectile, events: &mut Vec<Event>) -> Result<()> {
        let owner = self
            .actors
            .get(&projectile.owner)
            .ok_or_else(|| Error::new("projectile owner is absent"))?;
        let (aimed, reach) = if projectile.target_kind == ObjectKind::Building {
            // A building stands on the ground, and a projectile narrows a
            // dual-domain skill to its target's domain.
            (
                FightActorRef::Building(projectile.target),
                Reach::Domain(UnitDomain::Ground),
            )
        } else {
            let target_domain = self
                .actors
                .get(&projectile.target)
                .ok_or_else(|| Error::new("projectile target is absent"))?
                .rules
                .domain;
            (
                FightActorRef::Unit(projectile.target),
                Reach::Domain(target_domain),
            )
        };
        let hit = DamageHit {
            source: ObjectRef::new(ObjectKind::Unit, projectile.owner),
            source_team: projectile.team,
            team: owner.placement.team,
            amount: projectile.damage,
            aimed,
            hits_aimed: projectile.lock_target,
            center: (projectile.x, projectile.z),
            splash_radius: owner.rules.attack.splash_radius(),
            reach,
        };
        let struck = self.perform_damage(hit, events)?;
        events.push(event(
            Some(projectile.object_ref()),
            Some(hit.source),
            Some(projectile.team),
            Some(aimed.object_ref()),
            EventPayload::ProjectileRemoved {
                position: QVec3 {
                    x: projectile.x_q32,
                    y: projectile.y_q32,
                    z: projectile.z_q32,
                },
                intercepted: false,
                absorbed_by: None,
            },
        ));
        self.record_deaths(struck.deaths, events);
        // A building falls after every shot the tick resolves has been
        // recorded, not after its own: a tick that lands two reads `damage`,
        // `projectile_removed`, `projectile_removed`, `building_destroyed`.
        // `step_projectiles` emits these last.
        for (building_id, position) in struck.fallen {
            self.fallen_buildings.push(event(
                Some(ObjectRef::new(ObjectKind::Building, building_id)),
                None,
                None,
                None,
                EventPayload::BuildingDestroyed { position },
            ));
        }
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
    output: Option<&Path>,
    replay_layout: &str,
) -> Result<SimulationResult> {
    let generation_started = Instant::now();
    let execution = execute(layout, config, seed, output, None, Some(replay_layout))?;
    let Execution {
        simulation,
        writer,
        steps,
        end_reason,
        first_divergence,
        ..
    } = execution;
    debug_assert!(first_divergence.is_none());
    let hashes = writer.finish()?;
    let (file_size_bytes, member_sizes_bytes) = if let Some(path) = output {
        let published = mechcore_mcfr::McfrReader::open(path)?;
        if published.hashes() != &hashes {
            return Err(Error::new("published MCFR hashes changed after reopening"));
        }
        (
            Some(published.file_size_bytes()),
            Some(published.member_sizes_bytes().clone()),
        )
    } else {
        (None, None)
    };
    let generation_duration = generation_started.elapsed();
    let simulated_duration_milliseconds = steps
        .saturating_mul(LOGIC_TICK_TIME_UNITS)
        .saturating_mul(1_000)
        / TIME_UNITS_PER_SECOND;
    #[allow(
        clippy::cast_precision_loss,
        reason = "a millisecond count stays far below f64's exact integer range"
    )]
    let simulation_to_real_time_rate =
        simulated_duration_milliseconds as f64 / generation_duration.as_secs_f64() / 1_000.0;
    let winner = simulation.winner().map(team_name);
    Ok(SimulationResult {
        schema: "mechcore.simulation-result.v3",
        game_build: config.game_build.clone(),
        seed,
        seed_source,
        output: output.map(|path| path.display().to_string()),
        end_reason,
        steps,
        simulated_duration_milliseconds,
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
                max_life: actor.stats.max_life(),
            })
            .collect(),
        hashes,
        profiling: SimulationProfile {
            generation_duration_milliseconds: generation_duration.as_secs_f64() * 1_000.0,
            simulation_to_real_time_rate,
            file_size_bytes,
            member_sizes_bytes,
        },
    })
}

pub(crate) fn compare(
    layout: &CompiledLayout,
    config: &SimulationConfig,
    seed: i32,
    recording: &McfrReader,
) -> Result<SimulationComparison> {
    if recording.game_build() != config.game_build {
        return Err(Error::new(format!(
            "game_build mismatch: recording={}, simulation={}",
            recording.game_build(),
            config.game_build
        )));
    }
    let execution = execute(layout, config, seed, None, Some(recording), None)?;
    let Execution {
        writer,
        steps,
        first_divergence,
        divergent_tick,
        ..
    } = execution;
    let simulation_hashes = if first_divergence.is_none() {
        Some(writer.finish()?)
    } else {
        None
    };
    if let Some(hashes) = &simulation_hashes
        && hashes.physics_result_hash != recording.hashes().physics_result_hash
    {
        return Err(Error::new(
            "physics result hashes differ although every compared physics tick hash matches",
        ));
    }
    let content_equal = simulation_hashes
        .as_ref()
        .is_some_and(|hashes| hashes.content_result_hash == recording.hashes().content_result_hash);
    Ok(SimulationComparison {
        schema: "mechcore.sim-compare-result.v2",
        game_build: config.game_build.clone(),
        seed,
        equal: first_divergence.is_none(),
        content_equal,
        recording: TimelineSummary {
            physics_result_hash: Some(recording.hashes().physics_result_hash.clone()),
            content_result_hash: Some(recording.hashes().content_result_hash.clone()),
            tick_count: recording.tick_count(),
            complete: true,
        },
        simulation: TimelineSummary {
            physics_result_hash: simulation_hashes
                .as_ref()
                .map(|hashes| hashes.physics_result_hash.clone()),
            content_result_hash: simulation_hashes.map(|hashes| hashes.content_result_hash),
            tick_count: u32::try_from(steps)
                .map_err(|_| Error::new("simulation tick count exceeds u32"))?,
            complete: first_divergence.is_none(),
        },
        first_divergence,
        divergent_tick,
    })
}

struct Execution {
    simulation: Simulation,
    writer: McfrWriter,
    steps: u64,
    end_reason: &'static str,
    first_divergence: Option<u32>,
    divergent_tick: Option<DivergentTick>,
}

fn execute(
    layout: &CompiledLayout,
    config: &SimulationConfig,
    seed: i32,
    output: Option<&Path>,
    recording: Option<&McfrReader>,
    replay_layout: Option<&str>,
) -> Result<Execution> {
    let divisor = gcd(LOGIC_TICK_TIME_UNITS, TIME_UNITS_PER_SECOND);
    let context = DurableContext {
        logic_step: Rational {
            numerator: u32::try_from(LOGIC_TICK_TIME_UNITS / divisor)
                .map_err(|_| Error::new("logic-step numerator exceeds u32"))?,
            denominator: u32::try_from(TIME_UNITS_PER_SECOND / divisor)
                .map_err(|_| Error::new("logic-step denominator exceeds u32"))?,
        },
        time_units_per_second: u32::try_from(TIME_UNITS_PER_SECOND)
            .map_err(|_| Error::new("time units per second exceeds u32"))?,
        combat_round: layout.round,
        match_seed: seed,
    };
    let mut simulation =
        Simulation::new_unprepared(layout, &config.units, &config.training_ground, seed)?;
    let mut writer = match output {
        Some(path) => {
            let mut replay_layout = mechcore_document::parse_yaml(
                replay_layout
                    .ok_or_else(|| Error::new("output MCFR requires a replay layout"))?
                    .as_bytes(),
            )
            .map_err(Error::new)?;
            replay_layout.seed = Some(seed);
            let replay_layout =
                mechcore_document::canonical_yaml(replay_layout).map_err(Error::new)?;
            McfrWriter::create(path, &config.game_build, &context, &replay_layout)?
        }
        None => McfrWriter::hash_only(&context)?,
    };
    simulation.initialize_presearch_targets()?;
    let mut steps = 0;
    let mut first_divergence = None;
    let mut divergent_tick = None;
    let max_steps = FIGHT_TIME_SECONDS
        .saturating_mul(TIME_UNITS_PER_SECOND)
        .div_ceil(LOGIC_TICK_TIME_UNITS);
    let mut end_reason = loop {
        if steps >= max_steps {
            break "forced_time_limit";
        }
        let events = simulation.step(steps)?;
        steps += 1;
        let tick = u32::try_from(steps).map_err(|_| Error::new("tick index exceeds u32"))?;
        simulation.settle_intervals_if_finishing();
        let mut state = simulation.snapshot();
        state.canonicalize();
        let tick_hashes = writer.append_tick(state.clone(), &events)?;
        if let Some(recording) = recording {
            let expected_hash = if tick <= recording.tick_count() {
                Some(recording.physics_tick_hash(tick)?)
            } else {
                None
            };
            if expected_hash.as_deref() != Some(&tick_hashes.physics_tick_hash) {
                first_divergence = Some(tick);
                divergent_tick = Some(DivergentTick {
                    recording: if expected_hash.is_some() {
                        Some(recording.tick(tick)?)
                    } else {
                        None
                    },
                    simulation: Some(TickSlice {
                        tick,
                        state,
                        events,
                        physics_tick_hash: tick_hashes.physics_tick_hash,
                        content_tick_hash: tick_hashes.content_tick_hash,
                    }),
                });
                break "first_divergence";
            }
        }
        if simulation.ready_to_finish() {
            break "natural_module_drain";
        }
    };
    if first_divergence.is_none()
        && let Some(recording) = recording
        && steps < u64::from(recording.tick_count())
    {
        let tick = u32::try_from(steps)
            .map_err(|_| Error::new("tick index exceeds u32"))?
            .checked_add(1)
            .ok_or_else(|| Error::new("tick index overflow"))?;
        first_divergence = Some(tick);
        divergent_tick = Some(DivergentTick {
            recording: Some(recording.tick(tick)?),
            simulation: None,
        });
        end_reason = "first_divergence";
    }
    Ok(Execution {
        simulation,
        writer,
        steps,
        end_reason,
        first_divergence,
        divergent_tick,
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
    source_team_id: Option<u32>,
    target: Option<ObjectRef>,
    payload: EventPayload,
) -> Event {
    Event {
        subject,
        source,
        source_team_id,
        target,
        payload,
    }
}

fn point(x: i64, z: i64) -> QVec3 {
    QVec3 {
        x: space_to_q32(x),
        y: 0,
        z: space_to_q32(z),
    }
}

const fn building_alive(building: &BuildingState) -> bool {
    building.life.current > 0
}

fn building_radius(building: &BuildingState) -> i64 {
    q32_to_space_rounded(building.bounds_width / 2)
}

fn building_x(building: &BuildingState) -> i64 {
    q32_to_space_rounded(building.position.x)
}

fn building_z(building: &BuildingState) -> i64 {
    q32_to_space_rounded(building.position.z)
}

const fn unit_height(domain: UnitDomain) -> i64 {
    match domain {
        UnitDomain::Ground => 0,
        UnitDomain::Air => AIR_UNIT_HEIGHT,
    }
}

/// How far a point lies from a segment, in the fixed point the fight holds.
///
/// The build asks `LineRange.Overlaps(CircleRange)`, a rectangle from the
/// attacker to its target against a circle on the construction. This is the
/// same test written as a distance, which is what makes it one comparison
/// against a measured width.
fn distance_to_segment_q32(from: (i64, i64), to: (i64, i64), point: (i64, i64)) -> i64 {
    let (dx, dz) = (
        i128::from(to.0) - i128::from(from.0),
        i128::from(to.1) - i128::from(from.1),
    );
    let (px, pz) = (
        i128::from(point.0) - i128::from(from.0),
        i128::from(point.1) - i128::from(from.1),
    );
    let length = dx * dx + dz * dz;
    if length == 0 {
        return magnitude(
            i64::try_from(px).unwrap_or(i64::MAX),
            i64::try_from(pz).unwrap_or(i64::MAX),
        );
    }
    let along = (px * dx + pz * dz).clamp(0, length);
    let (nearest_x, nearest_z) = (dx * along / length, dz * along / length);
    magnitude(
        i64::try_from(px - nearest_x).unwrap_or(i64::MAX),
        i64::try_from(pz - nearest_z).unwrap_or(i64::MAX),
    )
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

fn native_q32_magnitude_3d(x: i64, y: i64, z: i64) -> i64 {
    fpcs_sqrt_fastest(
        q32_mul(x, x)
            .saturating_add(q32_mul(y, y))
            .saturating_add(q32_mul(z, z)),
    )
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

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "the 32-bit truncation and sign reinterpretation reproduce build-2227 Q32.32 arithmetic"
)]
pub(crate) fn fpcs_sin_fastest(value: i64) -> i64 {
    let turn = q32_mul(value, 0x28BE_60DC) as i32;
    let doubled = turn.wrapping_mul(2);
    let folded = 0x8000_0000_u32.wrapping_sub(turn as u32) as i32;
    let coordinate = i64::from(if doubled ^ turn >= 0 { turn } else { folded });
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

    fn unit_target(id: u64) -> FightActorRef {
        FightActorRef::Unit(id)
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
            let (x, z) = positions[(usize::try_from(id).unwrap() - 1) % 4];
            tree.insert(unit_target(id), x, z, 0);
        }
        assert!(tree.root.children.is_none());
        tree.insert(unit_target(20), positions[3].0, positions[3].1, 0);

        let children = tree.root.children.as_ref().unwrap();
        assert_eq!(
            children[0].elements,
            [17, 13, 9, 5, 1].map(unit_target).to_vec()
        );
        assert_eq!(
            children[1].elements,
            [18, 14, 10, 6, 2].map(unit_target).to_vec()
        );
        assert_eq!(
            children[2].elements,
            [19, 15, 11, 7, 3].map(unit_target).to_vec()
        );
        assert_eq!(
            children[3].elements,
            [16, 12, 8, 4, 20].map(unit_target).to_vec()
        );
    }

    #[test]
    fn target_quadtree_queries_parent_straddlers_before_children() {
        let mut tree = TargetActorQuadtree::new();
        tree.insert(unit_target(1), 0, 0, 1_000);
        for id in 2..=20 {
            tree.insert(unit_target(id), -100 * Q32_ONE, -100 * Q32_ONE, 0);
        }

        assert_eq!(tree.query_order().first(), Some(&unit_target(1)));
    }

    #[test]
    fn target_quadtree_reinserts_a_moved_child_element_in_native_order() {
        let mut tree = TargetActorQuadtree::new();
        for id in 1..=20 {
            tree.insert(unit_target(id), -100 * Q32_ONE, -100 * Q32_ONE, 0);
        }
        tree.position_changed(unit_target(7), 100 * Q32_ONE, -100 * Q32_ONE, 0);

        let children = tree.root.children.as_ref().unwrap();
        assert!(!children[0].elements.contains(&unit_target(7)));
        assert_eq!(children[1].elements.last(), Some(&unit_target(7)));
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
            rotated: false,
            corrections: Vec::new(),
        }
    }

    fn snapshot_velocity_q32(actor: &Actor) -> (i64, i64) {
        let velocity = actor.snapshot().velocity;
        (velocity.x, velocity.z)
    }

    fn raw_test_simulation(
        layout: &CompiledLayout,
        config: &SimulationConfig,
        seed: i32,
    ) -> Simulation {
        let actors = initialize_actors(layout, &config.units, seed).unwrap();
        let InitialBuildings {
            states: buildings,
            unsearchable,
            colliders: construction_colliders,
        } = initialize_buildings(&config.training_ground, &[]).unwrap();
        let target_quadtrees = initialize_target_quadtrees(&actors, &buildings, &unsearchable);
        Simulation {
            actors,
            team_random: BTreeMap::new(),
            projectiles: Vec::new(),
            buildings,
            target_quadtrees,
            identities: IdentityAllocator::new(),
            rvo_counter: 0,
            rvo_first_tree_pending: true,
            terminal_drain_pending: false,
            late_building_events_pending: false,
            fallen_buildings: Vec::new(),
            construction_colliders: construction_colliders.clone(),
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
        let config = SimulationConfig::load().unwrap();
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
        let layout = CompiledLayout::of_units(
            1,
            vec![
                test_placement(0, 0, -20, -100),
                test_placement(0, 1, 20, -100),
                test_placement(1, 0, 0, 100),
            ],
        );
        let config = SimulationConfig::load().unwrap();
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
    #[allow(clippy::too_many_lines)]
    fn multi_formation_initial_state_and_target_search_entry_match_build_2259() {
        let layout = CompiledLayout::of_units(
            1,
            vec![
                Placement {
                    team: 0,
                    unit_id: 0,
                    formation_id: 0,
                    formation_index: 0,
                    type_name: "rhino".to_owned(),
                    world_x: -285,
                    world_z: -105,
                    rotation: 0,
                    rotated: false,
                    corrections: Vec::new(),
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
                    rotated: false,
                    corrections: Vec::new(),
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
                    rotated: false,
                    corrections: Vec::new(),
                },
            ],
        );
        let config = SimulationConfig::load().unwrap();
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
            let InitialBuildings {
                states: buildings,
                unsearchable,
                colliders: construction_colliders,
            } = initialize_buildings(&config.training_ground, &[]).unwrap();
            let target_quadtrees = initialize_target_quadtrees(&actors, &buildings, &unsearchable);
            Simulation {
                actors,
                team_random: BTreeMap::new(),
                projectiles: Vec::new(),
                buildings,
                target_quadtrees,
                identities: IdentityAllocator::new(),
                rvo_counter: 0,
                rvo_first_tree_pending: true,
                terminal_drain_pending: false,
                late_building_events_pending: false,
                fallen_buildings: Vec::new(),
                construction_colliders: construction_colliders.clone(),
            }
        };
        let mut simulation = make_simulation();
        assert_eq!(simulation.select_normal_unit_target(1).unwrap(), Some(3));
        simulation.initialize_presearch_targets().unwrap();
        assert_eq!(simulation.actors[&1].lock_target, Some(unit_target(3)));
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
        assert_eq!(fight_skill.actors[&1].lock_target, Some(unit_target(3)));
        assert_eq!(fight_skill.actors[&1].fight_skill_search_target_time, 0);
        fight_skill
            .update_fight_skill_target_search(1, 1, &target_search_order)
            .unwrap();
        assert_eq!(fight_skill.actors[&1].lock_target, Some(unit_target(2)));
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
        assert_eq!(hold_fire.actors[&1].lock_target, Some(unit_target(2)));
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
        assert_eq!(attack_state.actors[&1].lock_target, Some(unit_target(3)));
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
            target: unit_target(3),
        });
        prepare_state.refresh_target_query_snapshot();
        let target_search_order = prepare_state.target_search_order();
        prepare_state
            .update_fight_skill_target_search(1, 0, &target_search_order)
            .unwrap();
        assert_eq!(prepare_state.actors[&1].lock_target, Some(unit_target(3)));
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
        assert_eq!(dead_target.actors[&1].lock_target, Some(unit_target(2)));
    }

    #[test]
    fn normal_selector_split_query_remains_order_independent_for_a_unique_best() {
        let config = SimulationConfig::load().unwrap();
        let mut placements = vec![Placement {
            team: 0,
            unit_id: 0,
            formation_id: 0,
            formation_index: 0,
            type_name: "rhino".to_owned(),
            world_x: 0,
            world_z: -100,
            rotation: 0,
            rotated: false,
            corrections: Vec::new(),
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
            rotated: false,
            corrections: Vec::new(),
        }));
        let layout = CompiledLayout::of_units(1, placements);
        let actors = initialize_actors(&layout, &config.units, 7).unwrap();
        let InitialBuildings {
            states: buildings,
            unsearchable,
            colliders: construction_colliders,
        } = initialize_buildings(&config.training_ground, &[]).unwrap();
        let target_quadtrees = initialize_target_quadtrees(&actors, &buildings, &unsearchable);
        let simulation = Simulation {
            actors,
            team_random: BTreeMap::new(),
            projectiles: Vec::new(),
            buildings,
            target_quadtrees,
            identities: IdentityAllocator::new(),
            rvo_counter: 0,
            rvo_first_tree_pending: true,
            terminal_drain_pending: false,
            late_building_events_pending: false,
            fallen_buildings: Vec::new(),
            construction_colliders: construction_colliders.clone(),
        };
        assert_eq!(simulation.select_normal_unit_target(1).unwrap(), Some(10));
    }

    #[test]
    fn normal_selector_refuses_a_building_best_candidate() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 100)],
        );
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
    fn fight_skill_adopts_a_selected_building_and_enters_moving() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                Placement {
                    type_name: "steel_ball".to_owned(),
                    ..test_placement(0, 0, 0, 0)
                },
                test_placement(1, 0, 0, 200),
            ],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 200_000);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.lock_target = None;
        source.fight_skill_search_target_time = 0;
        source.motion = MotionState::Idle;
        let building = simulation
            .buildings
            .iter_mut()
            .find(|building| building.team_id == 1)
            .unwrap();
        building.position = point(0, 100_000);
        let building_id = building.building_id;

        simulation.step_actor(1, 0, &mut Vec::new()).unwrap();

        let source = &simulation.actors[&1];
        assert_eq!(source.motion, MotionState::Moving);
        assert_eq!(
            source.lock_target,
            Some(FightActorRef::Building(building_id))
        );
        assert!(!source.lock_is_terminal_handoff);
        assert_eq!(
            source.snapshot().mech_lock_target,
            Some(ObjectRef::new(ObjectKind::Building, building_id))
        );

        let other_building_id = simulation
            .buildings
            .iter()
            .find(|building| building.team_id == 1 && building.building_id != building_id)
            .unwrap()
            .building_id;
        for building in simulation
            .buildings
            .iter_mut()
            .filter(|building| building.team_id == 1)
        {
            building.position = if building.building_id == building_id {
                point(0, 300_000)
            } else {
                point(0, 100_000)
            };
        }

        for step in 1..=10 {
            simulation.step_actor(1, step, &mut Vec::new()).unwrap();
            assert_eq!(
                simulation.actors[&1].lock_target,
                Some(FightActorRef::Building(building_id))
            );
        }
        simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

        assert_eq!(
            simulation.actors[&1].lock_target,
            Some(FightActorRef::Building(other_building_id))
        );
    }

    #[test]
    fn normal_selector_keeps_the_first_native_quadtree_candidate_on_equal_score() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                test_placement(0, 0, 0, 0),
                test_placement(1, 0, -20, 100),
                test_placement(1, 1, 20, 100),
            ],
        );
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
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                test_placement(0, 0, 0, 0),
                test_placement(1, 0, 0, 100),
                test_placement(1, 1, 20, 100),
            ],
        );
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
    fn fight_skill_selector_scores_the_weapon_rotation_instead_of_the_root_body() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                test_placement(0, 0, 0, 0),
                test_placement(1, 0, -8, 94),
                test_placement(1, 1, 3, 96),
            ],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&1).unwrap(), 298, -64_312);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), -8_193, 29_690);
        set_actor_position(simulation.actors.get_mut(&3).unwrap(), 3_679, 31_889);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.set_body_rotation(mdeg_to_degrees_q32(2_680));
        source.set_weapon_rotation(mdeg_to_degrees_q32(358_902));
        simulation.refresh_target_query_snapshot();

        assert_eq!(simulation.select_normal_unit_target(1).unwrap(), Some(2));

        simulation
            .actors
            .get_mut(&1)
            .unwrap()
            .target_query_source_rotation_q32 = mdeg_to_degrees_q32(2_680);
        assert_eq!(simulation.select_normal_unit_target(1).unwrap(), Some(3));

        simulation.actors.get_mut(&1).unwrap().rules.has_body = false;
        simulation.refresh_target_query_snapshot();
        assert_eq!(simulation.select_normal_unit_target(1).unwrap(), Some(3));
    }

    #[test]
    fn grouped_bodyless_selector_scores_the_first_weapon_rotation() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                Placement {
                    type_name: "wraith".to_owned(),
                    ..test_placement(0, 0, 0, 0)
                },
                test_placement(1, 0, -8, 94),
                test_placement(1, 1, 3, 96),
            ],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&1).unwrap(), 298, -64_312);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), -8_193, 29_690);
        set_actor_position(simulation.actors.get_mut(&3).unwrap(), 3_679, 31_889);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.set_body_rotation(mdeg_to_degrees_q32(2_680));
        source.set_weapon_rotation(mdeg_to_degrees_q32(358_902));

        simulation.refresh_target_query_snapshot();

        assert_eq!(simulation.select_normal_unit_target(1).unwrap(), Some(2));
    }

    #[test]
    fn grouped_child_search_scores_the_owner_root_rotation() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                Placement {
                    type_name: "wraith".to_owned(),
                    ..test_placement(0, 0, 0, 0)
                },
                test_placement(1, 0, -8, 94),
                test_placement(1, 1, 3, 96),
            ],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&1).unwrap(), 298, -64_312);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), -8_193, 29_690);
        set_actor_position(simulation.actors.get_mut(&3).unwrap(), 3_679, 31_889);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.set_body_rotation(mdeg_to_degrees_q32(2_680));
        source.set_weapon_rotation(mdeg_to_degrees_q32(358_902));
        simulation.refresh_target_query_snapshot();
        let order = simulation.target_search_order();

        let ranked = simulation
            .rank_group_unit_targets_with_order(1, &order, false)
            .unwrap();

        assert_eq!(ranked.first(), Some(&3));
    }

    #[test]
    fn has_body_attack_replacement_outside_range_exits_through_one_idle_tick() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                test_placement(0, 0, 0, 0),
                test_placement(1, 0, 0, 20),
                test_placement(1, 1, 0, 200),
            ],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        simulation.actors.get_mut(&2).unwrap().life = 0;
        let source = simulation.actors.get_mut(&1).unwrap();
        source.motion = MotionState::Attacking;
        source.lock_target = Some(unit_target(2));
        source.fight_skill_phase = FightSkillPhase::Attack;
        source.set_weapon_rotation(mdeg_to_degrees_q32(4_924));
        source.aim_rotation = 4_924;

        simulation.step_actor(1, 1, &mut Vec::new()).unwrap();
        assert_eq!(simulation.actors[&1].motion, MotionState::Idle);
        assert_eq!(simulation.actors[&1].lock_target, None);

        simulation.step_actor(1, 2, &mut Vec::new()).unwrap();
        assert_eq!(simulation.actors[&1].motion, MotionState::Moving);
        assert_eq!(simulation.actors[&1].lock_target, Some(unit_target(3)));
        assert_eq!(simulation.actors[&1].aim_rotation, 4_924);
    }

    #[test]
    fn normal_quick_switch_only_adopts_an_immediately_attackable_target() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                Placement {
                    type_name: "stormcaller".to_owned(),
                    ..test_placement(0, 0, 0, 0)
                },
                test_placement(1, 0, 0, 60),
                test_placement(1, 1, 0, 100),
            ],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        for building in &mut simulation.buildings {
            building.targetable = false;
        }
        for actor in simulation
            .actors
            .values_mut()
            .filter(|actor| actor.placement.team == 1)
        {
            set_actor_position(actor, 0, 250_000);
        }
        set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
        set_actor_position(simulation.actors.get_mut(&5).unwrap(), 0, 60_000);
        set_actor_position(simulation.actors.get_mut(&6).unwrap(), 0, 100_000);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.motion = MotionState::Attacking;
        source.lock_target = Some(unit_target(5));
        source.fight_skill_phase = FightSkillPhase::Attack;
        simulation.refresh_target_query_snapshot();
        let target_search_order = simulation.target_search_order();

        assert!(
            !simulation
                .quick_switch_active_target_outside_attack_area(1, 1, &target_search_order, false)
                .unwrap()
        );
        assert_eq!(simulation.actors[&1].lock_target, Some(unit_target(6)));

        let source = simulation.actors.get_mut(&1).unwrap();
        source.motion = MotionState::Attacking;
        source.lock_target = Some(unit_target(5));
        source.fight_skill_phase = FightSkillPhase::Attack;
        simulation.refresh_target_query_snapshot();
        simulation.actors.get_mut(&5).unwrap().life = 0;
        let target_search_order = simulation.target_search_order();

        assert!(
            !simulation
                .quick_switch_active_target_outside_attack_area(1, 2, &target_search_order, false)
                .unwrap()
        );
        assert_eq!(simulation.actors[&1].lock_target, Some(unit_target(6)));

        set_actor_position(simulation.actors.get_mut(&6).unwrap(), 0, 250_000);
        simulation.actors.get_mut(&5).unwrap().life = 263;
        let source = simulation.actors.get_mut(&1).unwrap();
        source.motion = MotionState::Attacking;
        source.lock_target = Some(unit_target(5));
        source.fight_skill_phase = FightSkillPhase::Attack;
        source.pending = Some(PendingRelease {
            step: 3,
            target: unit_target(5),
        });
        simulation.refresh_target_query_snapshot();
        let target_search_order = simulation.target_search_order();

        assert!(
            simulation
                .quick_switch_active_target_outside_attack_area(1, 3, &target_search_order, false)
                .unwrap()
        );
        assert_eq!(simulation.actors[&1].lock_target, None);
        assert_eq!(simulation.actors[&1].motion, MotionState::Idle);
        assert!(simulation.actors[&1].pending.is_none());

        let source = simulation.actors.get_mut(&1).unwrap();
        source.motion = MotionState::Idle;
        source.lock_target = Some(unit_target(5));
        source.fight_skill_phase = FightSkillPhase::Idle;
        simulation.actors.get_mut(&5).unwrap().life = 0;

        assert!(
            simulation
                .quick_switch_active_target_outside_attack_area(1, 4, &target_search_order, true)
                .unwrap()
        );
        assert_eq!(simulation.actors[&1].lock_target, None);
        assert_eq!(simulation.actors[&1].motion, MotionState::Idle);
    }

    #[test]
    fn native_hundredth_constant_is_not_rationally_rounded() {
        assert_eq!(C0_01_RAW, 42_949_672);
        assert_eq!(q32_mul(30_i64 << 32, C0_01_RAW), 1_288_490_160);
        assert_ne!(C0_01_RAW, q32_div(Q32_ONE, 100_i64 << 32));
    }

    #[test]
    fn has_body_attack_replacement_outside_weapon_angle_exits_through_idle() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                test_placement(0, 0, 0, 0),
                test_placement(1, 0, 0, 20),
                test_placement(1, 1, 0, 50),
            ],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        simulation.actors.get_mut(&2).unwrap().life = 0;
        let source = simulation.actors.get_mut(&1).unwrap();
        source.motion = MotionState::Attacking;
        source.lock_target = Some(unit_target(2));
        source.fight_skill_phase = FightSkillPhase::Attack;
        source.set_weapon_rotation(mdeg_to_degrees_q32(90_000));

        simulation.step_actor(1, 1, &mut Vec::new()).unwrap();

        assert_eq!(simulation.actors[&1].motion, MotionState::Idle);
        assert_eq!(simulation.actors[&1].lock_target, None);
    }

    #[test]
    fn entering_attack_defers_weapon_tracking_until_the_next_tick() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 50, 0)],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        let initial_rotation = mdeg_to_degrees_q32(168_143);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.lock_target = Some(unit_target(2));
        source.set_weapon_rotation(initial_rotation);
        source.aim_rotation = 168_143;

        simulation.step_actor(1, 1, &mut Vec::new()).unwrap();
        assert_eq!(simulation.actors[&1].motion, MotionState::Attacking);
        assert_eq!(
            simulation.actors[&1].weapon_rotations_q32[0],
            initial_rotation
        );
        assert_eq!(simulation.actors[&1].aim_rotation, 168_143);

        simulation.step_actor(1, 2, &mut Vec::new()).unwrap();
        assert_ne!(
            simulation.actors[&1].weapon_rotations_q32[0],
            initial_rotation
        );
        assert_ne!(simulation.actors[&1].aim_rotation, 168_143);
    }

    #[test]
    fn same_tick_target_death_scores_live_candidate_positions() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                test_placement(0, 0, 0, 0),
                test_placement(1, 0, 0, 20),
                test_placement(1, 1, 0, 40),
                test_placement(1, 2, 0, 60),
            ],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 20_000);
        set_actor_position(simulation.actors.get_mut(&3).unwrap(), 0, 40_000);
        set_actor_position(simulation.actors.get_mut(&4).unwrap(), 0, 60_000);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.lock_target = Some(unit_target(2));
        source.fight_skill_search_target_time = 10;
        simulation.refresh_target_query_snapshot();
        let target_search_order = simulation.target_search_order();

        simulation.actors.get_mut(&2).unwrap().life = 0;
        simulation.actors.get_mut(&3).unwrap().z_q32 = space_to_q32(100_000);
        simulation.actors.get_mut(&4).unwrap().z_q32 = space_to_q32(10_000);
        simulation
            .update_fight_skill_target_search(1, 1, &target_search_order)
            .unwrap();

        assert_eq!(simulation.actors[&1].lock_target, Some(unit_target(4)));

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

        assert_eq!(simulation.actors[&1].lock_target, Some(unit_target(4)));
    }

    #[test]
    fn completed_bodyless_melee_attack_uses_one_idle_tick_before_reapproach() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 100)],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 100_000);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.describe(config.units.get("crawler").unwrap().clone());
        source.lock_target = Some(unit_target(2));
        source.motion = MotionState::Moving;
        source.backswing_finish_step = Some(10);
        source.set_body_rotation(mdeg_to_degrees_q32(123_000));

        simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

        let source = &simulation.actors[&1];
        assert_eq!(source.motion, MotionState::Idle);
        assert_eq!(source.lock_target, None);
        assert_eq!(source.body_rotation, 123_000);
        assert_eq!((source.next_target_x_q32, source.next_target_z_q32), (0, 0));
    }

    #[test]
    fn completed_bodyless_melee_attack_uses_idle_before_an_out_of_angle_reentry() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 5)],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 5_000);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.describe(config.units.get("crawler").unwrap().clone());
        source.lock_target = Some(unit_target(2));
        source.motion = MotionState::Moving;
        source.fight_skill_phase = FightSkillPhase::Attack;
        source.backswing_finish_step = Some(10);
        source.set_body_rotation(mdeg_to_degrees_q32(90_000));

        simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

        let source = &simulation.actors[&1];
        assert_eq!(source.motion, MotionState::Idle);
        assert_eq!(source.lock_target, None);
        assert_eq!(source.body_rotation, 90_000);
        assert_eq!((source.next_target_x_q32, source.next_target_z_q32), (0, 0));
    }

    #[test]
    fn allied_kill_during_backswing_retains_the_dead_target_until_finish() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                test_placement(0, 0, 0, 0),
                test_placement(0, 1, 0, 10),
                test_placement(1, 0, 0, 100),
                test_placement(1, 1, 0, 110),
            ],
        );
        for type_name in ["crawler", "wasp"] {
            let mut simulation = raw_test_simulation(&layout, &config, 7);
            let source = simulation.actors.get_mut(&1).unwrap();
            source.describe(config.units.get(type_name).unwrap().clone());
            source.lock_target = Some(unit_target(3));
            source.motion = MotionState::Attacking;
            source.fight_skill_phase = FightSkillPhase::Attack;
            source.backswing_finish_step = Some(10);
            source.next_attack_step = 20;
            simulation.actors.get_mut(&3).unwrap().life = 0;

            simulation.step_actor(1, 8, &mut Vec::new()).unwrap();
            let stop_target = {
                let source = &simulation.actors[&1];
                assert_eq!(source.motion, MotionState::Idle, "{type_name}");
                assert_eq!(source.lock_target, Some(unit_target(3)), "{type_name}");
                assert_eq!(source.backswing_finish_step, Some(10), "{type_name}");
                (source.next_target_x_q32, source.next_target_z_q32)
            };
            simulation.actors.get_mut(&1).unwrap().x_q32 += Q32_ONE;

            for step in 9..=10 {
                simulation.step_actor(1, step, &mut Vec::new()).unwrap();
                let source = &simulation.actors[&1];
                assert_eq!(source.motion, MotionState::Idle, "{type_name}");
                assert_eq!(source.lock_target, Some(unit_target(3)), "{type_name}");
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
            assert_eq!(source.lock_target, None, "{type_name}");
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
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 20)],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.describe(config.units.get("crawler").unwrap().clone());
        source.lock_target = Some(unit_target(2));
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
        assert_eq!(source.lock_target, Some(unit_target(2)));
        assert_eq!(source.backswing_finish_step, Some(20));
    }

    #[test]
    fn later_final_enemy_death_does_not_clear_an_own_kill_backswing_target() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                test_placement(0, 0, 0, 0),
                test_placement(1, 0, 0, 20),
                test_placement(1, 1, 0, 100),
            ],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.describe(config.units.get("crawler").unwrap().clone());
        source.lock_target = Some(unit_target(2));
        source.motion = MotionState::Idle;
        source.fight_skill_phase = FightSkillPhase::Attack;
        source.retarget_after_own_direct_kill = true;
        source.backswing_finish_step = Some(20);
        simulation.actors.get_mut(&2).unwrap().life = 0;

        simulation.step_actor(1, 10, &mut Vec::new()).unwrap();
        assert_eq!(simulation.actors[&1].lock_target, Some(unit_target(2)));

        simulation.actors.get_mut(&3).unwrap().life = 0;
        simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

        let source = &simulation.actors[&1];
        assert_eq!(source.motion, MotionState::Idle);
        assert_eq!(source.lock_target, Some(unit_target(2)));
        assert_eq!(source.backswing_finish_step, Some(20));
    }

    #[test]
    fn bodyless_attack_defers_a_dead_target_replacement_outside_attack_area() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                test_placement(0, 0, 0, 0),
                test_placement(1, 0, 0, 20),
                test_placement(1, 1, 0, 100),
            ],
        );
        for type_name in ["crawler", "fang"] {
            let mut simulation = raw_test_simulation(&layout, &config, 7);
            let source = simulation.actors.get_mut(&1).unwrap();
            source.describe(config.units.get(type_name).unwrap().clone());
            source.lock_target = Some(unit_target(2));
            source.motion = MotionState::Attacking;
            source.fight_skill_phase = FightSkillPhase::Idle;
            source.motion_attack_hold_fire = false;
            source.next_attack_step = 20;
            simulation.actors.get_mut(&2).unwrap().life = 0;

            simulation.step_actor(1, 10, &mut Vec::new()).unwrap();
            let source = &simulation.actors[&1];
            assert_eq!(source.motion, MotionState::Idle, "{type_name}");
            assert_eq!(source.lock_target, None, "{type_name}");
            assert_eq!(source.next_attack_step, 20, "{type_name}");

            simulation.step_actor(1, 11, &mut Vec::new()).unwrap();
            assert_eq!(
                simulation.actors[&1].lock_target,
                Some(unit_target(3)),
                "{type_name}"
            );
        }
    }

    #[test]
    fn bodyless_in_range_turn_barrier_preserves_attack_timing() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                test_placement(0, 0, 0, 0),
                test_placement(1, 0, 0, -50),
                test_placement(1, 1, 0, -40),
            ],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.describe(config.units.get("fang").unwrap().clone());
        source.lock_target = Some(unit_target(2));
        source.motion = MotionState::Attacking;
        source.fight_skill_phase = FightSkillPhase::Idle;
        source.next_attack_step = 20;
        simulation.actors.get_mut(&2).unwrap().life = 0;
        assert!(simulation.bodyless_target_in_attack_range(1, unit_target(3)));
        assert!(!simulation.bodyless_target_in_attack_angle(1, unit_target(3)));

        simulation.step_actor(1, 10, &mut Vec::new()).unwrap();

        let source = &simulation.actors[&1];
        assert_eq!(source.lock_target, None);
        assert_eq!(source.motion, MotionState::Idle);
        assert_eq!(source.next_attack_step, 20);
    }

    #[test]
    fn bodyless_projectile_idle_entry_holds_for_turning() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![test_placement(0, 0, 0, 0), test_placement(1, 0, -20, 50)],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.describe(config.units.get("fang").unwrap().clone());
        source.lock_target = Some(unit_target(2));
        source.motion = MotionState::Idle;

        simulation.step_actor(1, 10, &mut Vec::new()).unwrap();

        let source = &simulation.actors[&1];
        assert_eq!(source.motion, MotionState::Attacking);
        assert!(source.motion_attack_hold_fire);
        assert_eq!(source.fight_skill_phase, FightSkillPhase::Idle);
    }

    #[test]
    fn bodyless_quick_switch_replaces_a_dead_target_immediately_in_range() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                test_placement(0, 0, 0, 0),
                test_placement(1, 0, 0, 20),
                test_placement(1, 1, 0, 40),
            ],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        simulation.team_random.insert(0, GrRandom::new(7));
        let source = simulation.actors.get_mut(&1).unwrap();
        source.describe(config.units.get("wasp").unwrap().clone());
        source.lock_target = Some(unit_target(2));
        source.motion = MotionState::Attacking;
        source.fight_skill_phase = FightSkillPhase::Attack;
        source.next_attack_step = 10;
        source.backswing_finish_step = Some(20);
        simulation.actors.get_mut(&2).unwrap().life = 0;

        simulation.step_actor(1, 10, &mut Vec::new()).unwrap();

        assert_eq!(simulation.actors[&1].lock_target, Some(unit_target(2)));
        assert_eq!(simulation.actors[&1].backswing_finish_step, Some(20));

        let mut events = Vec::new();
        simulation.step_actor(1, 11, &mut events).unwrap();

        assert_eq!(simulation.actors[&1].lock_target, Some(unit_target(3)));
        assert_eq!(simulation.actors[&1].backswing_finish_step, Some(41));
        assert!(
            events
                .iter()
                .any(|event| { matches!(event.payload, EventPayload::ProjectileReleased { .. }) })
        );
    }

    #[test]
    fn bodyless_non_quick_switch_defers_an_in_range_dead_target_replacement() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                test_placement(0, 0, 0, 0),
                test_placement(1, 0, 0, 2),
                test_placement(1, 1, 0, 4),
            ],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.describe(config.units.get("crawler").unwrap().clone());
        source.lock_target = Some(unit_target(2));
        source.motion = MotionState::Attacking;
        source.fight_skill_phase = FightSkillPhase::Idle;
        simulation.actors.get_mut(&2).unwrap().life = 0;

        simulation.step_actor(1, 10, &mut Vec::new()).unwrap();
        assert_eq!(simulation.actors[&1].lock_target, None);

        simulation.step_actor(1, 11, &mut Vec::new()).unwrap();
        assert_eq!(simulation.actors[&1].lock_target, Some(unit_target(3)));
    }

    #[test]
    fn bodyless_melee_attack_motion_exits_through_idle_when_target_leaves_range() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 100)],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 100_000);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.describe(config.units.get("crawler").unwrap().clone());
        source.lock_target = Some(unit_target(2));
        source.motion = MotionState::Attacking;
        source.set_body_rotation(mdeg_to_degrees_q32(123_000));

        simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

        let source = &simulation.actors[&1];
        assert_eq!(source.motion, MotionState::Idle);
        assert_eq!(source.lock_target, None);
        assert_eq!(source.body_rotation, 123_000);
        assert_eq!((source.next_target_x_q32, source.next_target_z_q32), (0, 0));
    }

    #[test]
    fn bodyless_projectile_attack_motion_exits_through_idle_when_target_leaves_range() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 100)],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 100_000);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.describe(config.units.get("fang").unwrap().clone());
        source.lock_target = Some(unit_target(2));
        source.motion = MotionState::Attacking;
        source.fight_skill_phase = FightSkillPhase::Attack;
        source.set_body_rotation(mdeg_to_degrees_q32(123_000));

        simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

        let source = &simulation.actors[&1];
        assert_eq!(source.motion, MotionState::Idle);
        assert_eq!(source.lock_target, None);
        assert_eq!(source.fight_skill_phase, FightSkillPhase::Idle);
        assert_eq!(source.body_rotation, 123_000);
        assert_eq!((source.next_target_x_q32, source.next_target_z_q32), (0, 0));
    }

    #[test]
    fn bodyless_melee_retains_target_while_motion_attack_is_held() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 100)],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 100_000);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.describe(config.units.get("crawler").unwrap().clone());
        source.lock_target = Some(unit_target(2));
        source.motion = MotionState::Attacking;
        source.motion_attack_hold_fire = true;
        source.set_body_rotation(mdeg_to_degrees_q32(123_000));

        simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

        let source = &simulation.actors[&1];
        assert_eq!(source.motion, MotionState::Moving);
        assert_eq!(source.lock_target, Some(unit_target(2)));
        assert_eq!(source.body_rotation, 123_000);
    }

    #[test]
    fn rejected_bodyless_melee_active_attack_reopens_idle_search_before_release() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 100)],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 100_000);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.describe(config.units.get("crawler").unwrap().clone());
        source.lock_target = Some(unit_target(2));
        source.motion = MotionState::Attacking;
        source.pending = Some(PendingRelease {
            step: 12,
            target: unit_target(2),
        });
        source.fight_skill_phase = FightSkillPhase::Attack;

        simulation.step_actor(1, 11, &mut Vec::new()).unwrap();
        let source = &simulation.actors[&1];
        assert_eq!(source.motion, MotionState::Idle);
        assert_eq!(source.lock_target, None);
        assert_eq!(source.fight_skill_phase, FightSkillPhase::Idle);

        simulation.step_actor(1, 12, &mut Vec::new()).unwrap();
        let source = &simulation.actors[&1];
        assert_eq!(source.motion, MotionState::Moving);
        assert_eq!(source.lock_target, Some(unit_target(2)));
    }

    #[test]
    fn bodyless_attack_cancels_a_pending_attack_when_an_ally_kills_its_target() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                test_placement(0, 0, 0, 0),
                test_placement(1, 0, 0, 100),
                test_placement(1, 1, 20, 100),
            ],
        );
        for type_name in ["crawler", "fang"] {
            let mut simulation = raw_test_simulation(&layout, &config, 7);
            let source = simulation.actors.get_mut(&1).unwrap();
            source.describe(config.units.get(type_name).unwrap().clone());
            source.lock_target = Some(unit_target(2));
            source.motion = MotionState::Attacking;
            source.pending = Some(PendingRelease {
                step: 12,
                target: unit_target(2),
            });
            source.fight_skill_phase = FightSkillPhase::Attack;
            simulation.actors.get_mut(&2).unwrap().life = 0;

            let mut events = Vec::new();
            simulation.step_actor(1, 11, &mut events).unwrap();
            let source = &simulation.actors[&1];
            assert_eq!(source.motion, MotionState::Idle, "{type_name}");
            assert_eq!(source.lock_target, None, "{type_name}");
            assert_eq!(
                source.fight_skill_phase,
                FightSkillPhase::Idle,
                "{type_name}"
            );
            assert!(source.pending.is_none(), "{type_name}");
            assert!(events.is_empty(), "{type_name}");

            simulation.step_actor(1, 12, &mut events).unwrap();
            assert_eq!(
                simulation.actors[&1].lock_target,
                Some(unit_target(3)),
                "{type_name}"
            );
            assert!(events.is_empty(), "{type_name}");
        }
    }

    #[test]
    fn laser_own_kill_retains_then_clears_the_dead_target() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 20)],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.describe(config.units.get("steel_ball").unwrap().clone());
        source.lock_target = Some(unit_target(2));
        source.motion = MotionState::Attacking;
        source.fight_skill_phase = FightSkillPhase::Attack;
        source.pending = Some(PendingRelease {
            step: 10,
            target: unit_target(2),
        });
        simulation.actors.get_mut(&2).unwrap().life = 1;
        let mut events = Vec::new();

        simulation.release(1, &mut events).unwrap();

        let source = &simulation.actors[&1];
        assert_eq!(source.motion, MotionState::Idle);
        assert_eq!(source.lock_target, Some(unit_target(2)));
        assert!(source.retarget_after_own_direct_kill);
        assert_eq!(source.laser_attack_count, 1);
        assert!(matches!(events[0].payload, EventPayload::UnitDied { .. }));
        assert!(matches!(
            events[1].payload,
            EventPayload::Damage { amount: 1 }
        ));

        simulation.step_actor(1, 11, &mut Vec::new()).unwrap();
        let source = &simulation.actors[&1];
        assert_eq!(source.motion, MotionState::Idle);
        assert_eq!(source.lock_target, None);
        assert!(!source.retarget_after_own_direct_kill);
        assert_eq!(source.laser_attack_count, 0);
    }

    #[test]
    fn laser_own_kill_skips_the_same_tick_bodyless_rotation() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 20)],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.describe(config.units.get("steel_ball").unwrap().clone());
        source.lock_target = Some(unit_target(2));
        source.motion = MotionState::Attacking;
        source.fight_skill_phase = FightSkillPhase::Attack;
        source.pending = Some(PendingRelease {
            step: 10,
            target: unit_target(2),
        });
        source.set_body_rotation(0);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 3_000, 20_000);
        simulation.actors.get_mut(&2).unwrap().life = 1;

        simulation.step_actor(1, 10, &mut Vec::new()).unwrap();

        let source = &simulation.actors[&1];
        assert_eq!(source.motion, MotionState::Idle);
        assert_eq!(source.body_rotation, 0);
        assert_eq!(source.lock_target, Some(unit_target(2)));
    }

    #[test]
    fn bodyless_quick_switch_retargets_a_pending_attack_in_attack_area() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                test_placement(0, 0, 0, 0),
                test_placement(1, 0, 0, 20),
                test_placement(1, 1, 0, 40),
            ],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.describe(config.units.get("fang").unwrap().clone());
        source.lock_target = Some(unit_target(2));
        source.motion = MotionState::Attacking;
        source.pending = Some(PendingRelease {
            step: 12,
            target: unit_target(2),
        });
        source.fight_skill_phase = FightSkillPhase::Attack;
        simulation.actors.get_mut(&2).unwrap().life = 0;

        simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

        let source = &simulation.actors[&1];
        assert_eq!(source.motion, MotionState::Attacking);
        assert_eq!(source.lock_target, Some(unit_target(3)));
        assert_eq!(source.pending.unwrap().target, unit_target(3));
    }

    #[test]
    fn direct_splash_emits_one_damage_event_per_actual_target() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                Placement {
                    type_name: "rhino".to_owned(),
                    ..test_placement(0, 0, 0, 0)
                },
                test_placement(1, 0, 0, 20),
                test_placement(1, 1, 1, 20),
            ],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 20_000);
        set_actor_position(simulation.actors.get_mut(&3).unwrap(), 1_000, 20_000);
        let mut events = Vec::new();
        simulation
            .direct_effect(1, FightActorRef::Unit(2), &mut events)
            .unwrap();
        assert_eq!(
            [simulation.actors[&2].life, simulation.actors[&3].life],
            [1_253, 1_253]
        );
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].source, Some(ObjectRef::new(ObjectKind::Unit, 1)));
        assert_eq!(events[0].source_team_id, Some(0));
        assert_eq!(events[0].target, Some(ObjectRef::new(ObjectKind::Unit, 2)));
        assert_eq!(events[0].payload, EventPayload::Damage { amount: 3_560 });
        assert_eq!(events[1].target, Some(ObjectRef::new(ObjectKind::Unit, 3)));
        assert_eq!(events[1].payload, EventPayload::Damage { amount: 3_560 });
        assert_eq!(
            [
                simulation.actors[&2].last_damage_source,
                simulation.actors[&3].last_damage_source,
            ],
            [
                Some((ObjectRef::new(ObjectKind::Unit, 1), 0)),
                Some((ObjectRef::new(ObjectKind::Unit, 1), 0)),
            ]
        );
    }

    #[test]
    fn direct_kill_emits_damage_before_death_with_raw_target_position() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                Placement {
                    type_name: "rhino".to_owned(),
                    ..test_placement(0, 0, 0, 0)
                },
                test_placement(1, 0, 0, 20),
            ],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        let target = simulation.actors.get_mut(&2).unwrap();
        target.life = 1;
        target.x_q32 += 7;
        target.z_q32 -= 9;
        let expected_position = QVec3 {
            x: target.x_q32,
            y: 0,
            z: target.z_q32,
        };
        let mut events = Vec::new();

        simulation
            .direct_effect(1, FightActorRef::Unit(2), &mut events)
            .unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].payload, EventPayload::Damage { amount: 1 });
        assert_eq!(
            events[1].payload,
            EventPayload::UnitDied {
                position: expected_position
            }
        );
    }

    #[test]
    fn zero_radius_direct_attack_does_not_damage_an_overlapping_secondary_target() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                test_placement(0, 0, 0, 0),
                test_placement(1, 0, 0, 20),
                test_placement(1, 1, 1, 20),
            ],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        simulation
            .actors
            .get_mut(&1)
            .unwrap()
            .describe(config.units.get("crawler").unwrap().clone());
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 20_000);
        set_actor_position(simulation.actors.get_mut(&3).unwrap(), 1_000, 20_000);
        let primary_life = simulation.actors[&2].life;
        let secondary_life = simulation.actors[&3].life;

        let mut events = Vec::new();
        simulation
            .direct_effect(1, FightActorRef::Unit(2), &mut events)
            .unwrap();

        assert_eq!(simulation.actors[&2].life, primary_life - 79);
        assert_eq!(simulation.actors[&3].life, secondary_life);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].target, Some(ObjectRef::new(ObjectKind::Unit, 2)));
        assert_eq!(events[0].payload, EventPayload::Damage { amount: 79 });
    }

    #[test]
    fn direct_splash_refuses_a_building_before_unit_damage() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                Placement {
                    type_name: "rhino".to_owned(),
                    ..test_placement(0, 0, 0, 0)
                },
                test_placement(1, 0, 0, 20),
            ],
        );
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
            .direct_effect(1, FightActorRef::Unit(2), &mut Vec::new())
            .unwrap_err()
            .to_string();
        assert!(error.contains("splash against a building is not closed"));
        assert_eq!(simulation.actors[&2].life, previous_life);
    }

    #[test]
    fn projectile_splash_emits_one_damage_event_per_actual_target() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
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
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 20_000);
        set_actor_position(simulation.actors.get_mut(&3).unwrap(), 1_000, 20_000);
        let projectile = Projectile {
            id: 1,
            team: 0,
            owner: 1,
            target_kind: ObjectKind::Unit,
            target: 2,
            x: 0,
            y: 0,
            z: 20_000,
            x_q32: 0,
            y_q32: 0,
            z_q32: space_to_q32(20_000),
            cached_target_x: 0,
            cached_target_y: 0,
            cached_target_z: 20_000,
            cached_target_x_q32: 0,
            cached_target_y_q32: 0,
            cached_target_z_q32: space_to_q32(20_000),
            cached_target_radius: simulation.actors[&2].rules.collision_radius(),
            speed: simulation.actors[&1].rules.attack.projectile_speed(),
            damage: simulation.actors[&1].stats.attack_damage(),
            life: 1,
            lock_target: true,
        };
        let previous_life = [simulation.actors[&2].life, simulation.actors[&3].life];
        let mut events = Vec::new();
        simulation.impact(&projectile, &mut events).unwrap();
        assert_eq!(
            [simulation.actors[&2].life, simulation.actors[&3].life],
            previous_life.map(|life| life - 365)
        );
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].source, Some(ObjectRef::new(ObjectKind::Unit, 1)));
        assert_eq!(events[0].target, Some(ObjectRef::new(ObjectKind::Unit, 2)));
        assert_eq!(events[0].payload, EventPayload::Damage { amount: 365 });
        assert_eq!(events[1].target, Some(ObjectRef::new(ObjectKind::Unit, 3)));
        assert_eq!(events[1].payload, EventPayload::Damage { amount: 365 });
        assert!(matches!(
            events[2].payload,
            EventPayload::ProjectileRemoved { .. }
        ));
        assert_eq!(
            [
                simulation.actors[&2].last_damage_source,
                simulation.actors[&3].last_damage_source,
            ],
            [
                Some((ObjectRef::new(ObjectKind::Unit, 1), 0)),
                Some((ObjectRef::new(ObjectKind::Unit, 1), 0)),
            ]
        );
    }

    #[test]
    fn dual_domain_projectile_splash_uses_the_main_targets_domain() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                Placement {
                    type_name: "wraith".to_owned(),
                    ..test_placement(0, 0, 0, 0)
                },
                Placement {
                    type_name: "marksman".to_owned(),
                    ..test_placement(1, 0, 0, 20)
                },
                Placement {
                    type_name: "wraith".to_owned(),
                    ..test_placement(1, 1, 0, 20)
                },
            ],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 20_000);
        set_actor_position(simulation.actors.get_mut(&3).unwrap(), 0, 20_000);
        let projectile = Projectile {
            id: 1,
            team: 0,
            owner: 1,
            target_kind: ObjectKind::Unit,
            target: 2,
            x: 0,
            y: 0,
            z: 20_000,
            x_q32: 0,
            y_q32: 0,
            z_q32: space_to_q32(20_000),
            cached_target_x: 0,
            cached_target_y: 0,
            cached_target_z: 20_000,
            cached_target_x_q32: 0,
            cached_target_y_q32: 0,
            cached_target_z_q32: space_to_q32(20_000),
            cached_target_radius: simulation.actors[&2].rules.collision_radius(),
            speed: simulation.actors[&1].rules.attack.projectile_speed(),
            damage: simulation.actors[&1].stats.attack_damage(),
            life: 1,
            lock_target: true,
        };
        let ground_life = simulation.actors[&2].life;
        let air_life = simulation.actors[&3].life;

        simulation.impact(&projectile, &mut Vec::new()).unwrap();

        assert_eq!(simulation.actors[&2].life, ground_life - 381);
        assert_eq!(simulation.actors[&3].life, air_life);
    }

    #[test]
    fn projectile_drain_does_not_late_teardown_the_defeated_teams_buildings() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 100)],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        simulation.actors.get_mut(&1).unwrap().life = 0;
        simulation.projectiles.push(Projectile {
            id: 1,
            team: 0,
            owner: 1,
            target_kind: ObjectKind::Unit,
            target: 2,
            x: 0,
            y: 0,
            z: 0,
            x_q32: 0,
            y_q32: 0,
            z_q32: 0,
            cached_target_x: 0,
            cached_target_y: 0,
            cached_target_z: 1_000_000,
            cached_target_x_q32: 0,
            cached_target_y_q32: 0,
            cached_target_z_q32: space_to_q32(1_000_000),
            cached_target_radius: simulation.actors[&2].rules.collision_radius(),
            speed: 1,
            damage: 1,
            life: 1,
            lock_target: false,
        });

        simulation.step(1).unwrap();

        assert!(
            simulation
                .buildings
                .iter()
                .filter(|building| building.team_id == 0)
                .all(|building| building_alive(building) && building.life.current == 3_400)
        );
    }

    #[test]
    fn projectile_splash_refuses_a_secondary_building_before_damage() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                test_placement(0, 0, 0, 0),
                Placement {
                    type_name: "rhino".to_owned(),
                    ..test_placement(1, 0, 0, 20)
                },
            ],
        );
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
            target_kind: ObjectKind::Unit,
            target: 2,
            x: 0,
            y: 0,
            z: 20_000,
            x_q32: 0,
            y_q32: 0,
            z_q32: space_to_q32(20_000),
            cached_target_x: 0,
            cached_target_y: 0,
            cached_target_z: 20_000,
            cached_target_x_q32: 0,
            cached_target_y_q32: 0,
            cached_target_z_q32: space_to_q32(20_000),
            cached_target_radius: simulation.actors[&2].rules.collision_radius(),
            speed: simulation.actors[&1].rules.attack.projectile_speed(),
            damage: simulation.actors[&1].stats.attack_damage(),
            life: 1,
            lock_target: true,
        };
        let previous_life = simulation.actors[&2].life;
        let error = simulation
            .impact(&projectile, &mut Vec::new())
            .unwrap_err()
            .to_string();
        assert!(error.contains("splash against a building is not closed"));
        assert_eq!(simulation.actors[&2].life, previous_life);
    }

    #[test]
    fn rvo_solves_a_collision_building_inside_the_influence_bound() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                Placement {
                    type_name: "rhino".to_owned(),
                    ..test_placement(0, 0, 0, 0)
                },
                test_placement(1, 0, 0, 100),
            ],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 100_000);
        let building = simulation.buildings.first_mut().unwrap();
        building.position = point(20_000, 0);
        let target_position = (simulation.actors[&2].x_q32, simulation.actors[&2].z_q32);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.lock_target = Some(unit_target(2));
        source.motion = MotionState::Moving;
        source.next_target_x_q32 = target_position.0;
        source.next_target_z_q32 = target_position.1;
        source.next_speed_q32 = space_to_q32(source.stats.move_speed());
        source.next_max_speed_q32 = source.next_speed_q32;
        simulation.rvo_counter = 3;
        simulation.step_rvo();
        assert!(simulation.actors[&1].solver_speed_q32 > 0);
        assert_ne!(simulation.actors[&1].solver_target_z_q32, 0);
    }

    #[test]
    fn rvo_q32_boundary_uses_raw_distance_not_snapshot_rounding() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                Placement {
                    type_name: "rhino".to_owned(),
                    ..test_placement(0, 0, 0, 0)
                },
                test_placement(1, 0, 0, 100),
                test_placement(1, 1, 1, 68),
            ],
        );
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
        source.lock_target = Some(unit_target(2));
        source.motion = MotionState::Moving;
        source.next_target_x_q32 = target_position.0;
        source.next_target_z_q32 = target_position.1;
        source.next_speed_q32 = space_to_q32(source.stats.move_speed());
        source.next_max_speed_q32 = source.next_speed_q32;
        simulation.rvo_counter = 3;
        simulation.step_rvo();
        assert!(simulation.actors[&1].solver_speed_q32 > 0);
        assert_ne!(simulation.actors[&1].solver_target_z_q32, 0);
    }

    #[test]
    fn rvo_allows_a_coarse_tree_hit_outside_candidate_relative_travel() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                Placement {
                    type_name: "rhino".to_owned(),
                    ..test_placement(0, 0, 0, 0)
                },
                test_placement(1, 0, 0, 100),
                test_placement(1, 1, 75, 0),
            ],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
        set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 100_000);
        set_actor_position(simulation.actors.get_mut(&3).unwrap(), 75_000, 0);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.lock_target = Some(unit_target(2));
        source.motion = MotionState::Moving;
        simulation.rvo_counter = 3;
        simulation.step_rvo();
    }

    #[test]
    fn reviewed_direct_kill_keeps_then_clears_the_mech_lock_target_state() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                Placement {
                    team: 0,
                    unit_id: 0,
                    formation_id: 0,
                    formation_index: 0,
                    type_name: "rhino".to_owned(),
                    world_x: -285,
                    world_z: -105,
                    rotation: 0,
                    rotated: false,
                    corrections: Vec::new(),
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
                    rotated: false,
                    corrections: Vec::new(),
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
                    rotated: false,
                    corrections: Vec::new(),
                },
            ],
        );
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
                    assert_eq!(simulation.actors[&1].lock_target, Some(unit_target(3)));
                    assert!(simulation.actors[&1].retarget_after_own_direct_kill);
                    assert_eq!(simulation.actors[&1].motion, MotionState::Idle);
                }
                224..=232 => {
                    assert_eq!(simulation.actors[&1].lock_target, Some(unit_target(3)));
                    assert!(simulation.actors[&1].retarget_after_own_direct_kill);
                }
                233 => {
                    assert_eq!(simulation.actors[&1].lock_target, None);
                    assert!(!simulation.actors[&1].retarget_after_own_direct_kill);
                    assert_eq!(simulation.actors[&1].motion, MotionState::Idle);
                }
                234 => {
                    // The exact native assignment point is not observable. This only
                    // locks the simulator-private state needed to reproduce S/E tick 234.
                    assert_eq!(simulation.actors[&1].lock_target, Some(unit_target(2)));
                    assert_eq!(simulation.actors[&1].motion, MotionState::Moving);
                }
                _ => {}
            }
        }
    }

    #[test]
    fn crawler_member_grid_and_jitter_follow_native_creation_order() {
        let config = SimulationConfig::load().unwrap();
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
            rotated: false,
            corrections: Vec::new(),
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

        red.rotated = true;
        red.world_z = 105;
        let seed = 1_787_832_792;
        let rotated_positions = generate_formation_positions(&red, rules, seed).unwrap();
        let mut random = GrRandom::new(i64::from(seed).cast_unsigned());
        let base_offsets = rotated_positions
            .into_iter()
            .map(|(x_q32, z_q32)| {
                let jitter_x = i64::from(random.next_in_range(FORMATION_JITTER_RANGE_TENTHS))
                    .saturating_mul(C0_1_RAW);
                let jitter_z = i64::from(random.next_in_range(FORMATION_JITTER_RANGE_TENTHS))
                    .saturating_mul(C0_1_RAW);
                (
                    (x_q32 + jitter_x) >> 32,
                    (z_q32 - 105 * Q32_ONE + jitter_z) >> 32,
                )
            })
            .collect::<Vec<_>>();
        let expected = [-22, -16, -10, -4, 2, 8, 14, 20]
            .into_iter()
            .flat_map(|z| [(6, z), (0, z), (-6, z)])
            .collect::<Vec<_>>();
        assert_eq!(base_offsets, expected);
    }

    #[test]
    fn hound_partial_last_row_preserves_native_q32_centering() {
        let config = SimulationConfig::load().unwrap();
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
            rotated: false,
            corrections: Vec::new(),
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
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![Placement {
                team: 0,
                unit_id: 0,
                formation_id: 0,
                formation_index: 0,
                type_name: "crawler".to_owned(),
                world_x: 5,
                world_z: -50,
                rotation: 0,
                rotated: false,
                corrections: Vec::new(),
            }],
        );
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
        let config = SimulationConfig::load().unwrap();
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
    fn grouped_skills_prime_one_attack_interval_sample_per_child() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                Placement {
                    type_name: "wraith".to_owned(),
                    ..test_placement(0, 0, 0, 0)
                },
                Placement {
                    type_name: "wraith".to_owned(),
                    ..test_placement(1, 0, 0, 100)
                },
            ],
        );
        let mut simulation =
            Simulation::new(&layout, &config.units, &config.training_ground, 7).unwrap();

        let blue = simulation.team_random.get_mut(&0).unwrap();
        assert_eq!([blue.next_in_range(4), blue.next_in_range(4)], [-1, -2]);
        let red = simulation.team_random.get_mut(&1).unwrap();
        assert_eq!([red.next_in_range(4), red.next_in_range(4)], [-2, -3]);
    }

    #[test]
    fn grouped_core_replacement_swaps_or_shares_existing_child_targets() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![
                Placement {
                    type_name: "wraith".to_owned(),
                    ..test_placement(0, 0, 0, 0)
                },
                test_placement(1, 0, 0, 30),
                test_placement(1, 1, 0, 40),
                test_placement(1, 2, 0, 50),
                test_placement(1, 3, 0, 80),
            ],
        );
        let mut swap = raw_test_simulation(&layout, &config, 7);
        let source = swap.actors.get_mut(&1).unwrap();
        source.motion = MotionState::Attacking;
        source.fight_skill_phase = FightSkillPhase::Attack;
        source.lock_target = None;
        source.group_skill_targets = vec![None, Some(2), Some(3), Some(4)];
        swap.refresh_target_query_snapshot();
        let order = swap.target_search_order();

        swap.update_group_skill_targets(1, 10, &order).unwrap();

        let source = &swap.actors[&1];
        assert_eq!(
            source.group_skill_targets,
            [Some(2), Some(5), Some(3), Some(4)]
        );
        assert_eq!(source.lock_target, Some(unit_target(5)));
        assert_eq!(source.group_skill_prepare_ready_steps, [19, 19, 0, 0]);

        let mut shared = raw_test_simulation(&layout, &config, 7);
        let source = shared.actors.get_mut(&1).unwrap();
        source.motion = MotionState::Attacking;
        source.fight_skill_phase = FightSkillPhase::Attack;
        source.lock_target = Some(unit_target(5));
        source.group_skill_targets = vec![Some(5), Some(2), Some(3), Some(4)];
        source.group_skill_next_attack_steps = vec![0, 0, 0, 11];
        shared.actors.get_mut(&5).unwrap().life = 0;
        shared.refresh_target_query_snapshot();
        let order = shared.target_search_order();

        shared.update_group_skill_targets(1, 10, &order).unwrap();

        let source = &shared.actors[&1];
        assert_eq!(
            source.group_skill_targets,
            [Some(2), Some(2), Some(3), Some(4)]
        );
        assert_eq!(source.lock_target, Some(unit_target(2)));
        assert_eq!(source.group_skill_prepare_ready_steps, [0, 0, 0, 0]);
    }

    #[test]
    fn grouped_core_search_assigns_the_core_before_children() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            std::iter::once(Placement {
                type_name: "wraith".to_owned(),
                ..test_placement(0, 0, 0, 0)
            })
            .chain((0..6).map(|index| test_placement(1, index, 0, 40 + i64::from(index))))
            .collect(),
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.motion = MotionState::Attacking;
        source.fight_skill_phase = FightSkillPhase::Attack;
        source.lock_target = Some(unit_target(5));
        source.group_skill_targets = vec![Some(5), Some(2), Some(3), Some(4)];
        simulation.actors.get_mut(&4).unwrap().life = 0;
        simulation.actors.get_mut(&5).unwrap().life = 0;
        simulation.refresh_target_query_snapshot();
        let order = simulation.target_search_order();

        simulation
            .update_group_skill_targets(1, 10, &order)
            .unwrap();

        let source = &simulation.actors[&1];
        assert!(source.group_skill_targets[0].is_some());
        assert!(source.group_skill_targets[3].is_some());
        assert_ne!(source.group_skill_targets[0], source.group_skill_targets[3]);
        assert_eq!(
            source.lock_target,
            source.group_skill_targets[3].map(FightActorRef::Unit)
        );
    }

    #[test]
    fn grouped_child_replacements_follow_skill_order() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            std::iter::once(Placement {
                type_name: "wraith".to_owned(),
                ..test_placement(0, 0, 0, 0)
            })
            .chain((0..6).map(|index| test_placement(1, index, 0, 40 + i64::from(index))))
            .collect(),
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.motion = MotionState::Attacking;
        source.fight_skill_phase = FightSkillPhase::Attack;
        source.lock_target = Some(unit_target(5));
        source.group_skill_targets = vec![Some(2), Some(3), Some(4), Some(5)];
        source.group_skill_next_attack_steps = vec![0, 0, 10, 0];
        simulation.actors.get_mut(&4).unwrap().life = 0;
        simulation.actors.get_mut(&5).unwrap().life = 0;
        simulation.refresh_target_query_snapshot();
        let order = simulation.target_search_order();
        let replacements = simulation
            .rank_group_unit_targets_with_order(1, &order, true)
            .unwrap()
            .into_iter()
            .filter(|target_id| ![2, 3].contains(target_id))
            .take(2)
            .collect::<Vec<_>>();

        simulation
            .update_group_skill_targets(1, 10, &order)
            .unwrap();

        let source = &simulation.actors[&1];
        assert_eq!(source.group_skill_targets[2], Some(replacements[0]));
        assert_eq!(source.group_skill_targets[3], Some(replacements[1]));
        assert_eq!(source.lock_target, Some(unit_target(replacements[1])));
    }

    #[test]
    fn grouped_intervening_attack_rebalances_the_later_missing_child() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            std::iter::once(Placement {
                type_name: "wraith".to_owned(),
                ..test_placement(0, 0, 0, 0)
            })
            .chain((0..6).map(|index| test_placement(1, index, 0, 40 + i64::from(index))))
            .collect(),
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.motion = MotionState::Attacking;
        source.fight_skill_phase = FightSkillPhase::Attack;
        source.lock_target = Some(unit_target(5));
        source.group_skill_targets = vec![Some(2), Some(3), Some(4), Some(5)];
        source.group_skill_next_attack_steps = vec![0, 248, 228, 229];
        simulation.actors.get_mut(&3).unwrap().life = 0;
        simulation.actors.get_mut(&5).unwrap().life = 0;
        simulation.refresh_target_query_snapshot();
        let order = simulation.target_search_order();
        let replacements = simulation
            .rank_group_unit_targets_with_order(1, &order, true)
            .unwrap()
            .into_iter()
            .filter(|target_id| ![2, 4].contains(target_id))
            .take(2)
            .collect::<Vec<_>>();

        simulation
            .update_group_skill_targets(1, 228, &order)
            .unwrap();

        let source = &simulation.actors[&1];
        assert_eq!(source.group_skill_targets[3], Some(replacements[0]));
        assert_eq!(source.group_skill_targets[1], Some(replacements[1]));
        assert_eq!(source.lock_target, Some(unit_target(replacements[0])));
    }

    #[test]
    fn formation_seed_addition_wraps_at_the_native_i32_boundary() {
        let config = SimulationConfig::load().unwrap();
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
        let config = SimulationConfig::load().unwrap();
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
                rotated: false,
                corrections: Vec::new(),
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
        let layout = CompiledLayout::of_units(
            1,
            vec![
                Placement {
                    team: 0,
                    unit_id: 1,
                    formation_id: 1,
                    formation_index: 0,
                    type_name: "rhino".to_owned(),
                    world_x: -35,
                    world_z: -105,
                    rotation: 0,
                    rotated: false,
                    corrections: Vec::new(),
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
                    rotated: false,
                    corrections: Vec::new(),
                },
            ],
        );
        let config = SimulationConfig::load().unwrap();
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
                216 | 225 => assert_eq!(rhino.backswing_finish_step, Some(224)),
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
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 100)],
        );
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        let actor = simulation.actors.get_mut(&1).unwrap();
        actor.x_q32 = 1_717_060_204_994;
        actor.z_q32 = 1_696_431_970_300;
        actor.published_target_x_q32 = 1_717_060_248_001;
        actor.published_target_z_q32 = 1_696_431_622_651;
        actor.published_speed_q32 = 0;
        actor.lock_target = Some(unit_target(2));
        actor.backswing_finish_step = Some(10);
        simulation.actors.get_mut(&2).unwrap().life = 0;

        simulation.step_actor_rvo_position(1);

        let actor = &simulation.actors[&1];
        assert_eq!(actor.x_q32, actor.published_target_x_q32);
        assert_eq!(actor.z_q32, actor.published_target_z_q32);

        let actor = simulation.actors.get_mut(&1).unwrap();
        assert!(actor.rvo_stopped_snap_since_boundary);
        actor.motion = MotionState::Moving;
        actor.next_speed_q32 = space_to_q32(actor.stats.move_speed());
        actor.next_max_speed_q32 = actor.next_speed_q32;
        actor.solver_target_x_q32 = 1_717_060_204_994;
        actor.solver_target_z_q32 = 1_696_431_970_300;
        actor.solver_speed_q32 = 0;
        simulation.rvo_counter = 3;

        simulation.step_rvo();

        let actor = &simulation.actors[&1];
        assert_eq!(actor.published_target_x_q32, actor.x_q32);
        assert_eq!(actor.published_target_z_q32, actor.z_q32);
        assert!(!actor.rvo_stopped_snap_since_boundary);
    }

    #[test]
    fn stopped_snap_reset_expires_on_a_moving_tick_before_the_rvo_boundary() {
        let config = SimulationConfig::load().unwrap();
        let layout = CompiledLayout::of_units(
            1,
            vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 100)],
        );
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
        let layout = CompiledLayout::of_units(
            1,
            vec![
                Placement {
                    team: 0,
                    unit_id: 1,
                    formation_id: 1,
                    formation_index: 0,
                    type_name: "marksman".to_owned(),
                    world_x: 0,
                    world_z: -50,
                    rotation: 0,
                    rotated: false,
                    corrections: Vec::new(),
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
                    rotated: false,
                    corrections: Vec::new(),
                },
            ],
        );
        let config = SimulationConfig::load().unwrap();
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

        assert_eq!(
            snapshot_velocity_q32(actor),
            (-198_556_428, -30_061_443_202)
        );
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
        let layout = CompiledLayout::of_units(
            1,
            vec![
                Placement {
                    team: 0,
                    unit_id: 1,
                    formation_id: 1,
                    formation_index: 0,
                    type_name: "marksman".to_owned(),
                    world_x: 0,
                    world_z: -50,
                    rotation: 0,
                    rotated: false,
                    corrections: Vec::new(),
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
                    rotated: false,
                    corrections: Vec::new(),
                },
            ],
        );
        let config = SimulationConfig::load().unwrap();
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
        let layout = CompiledLayout::of_units(
            1,
            vec![
                Placement {
                    team: 0,
                    unit_id: 1,
                    formation_id: 1,
                    formation_index: 0,
                    type_name: "marksman".to_owned(),
                    world_x: 0,
                    world_z: -50,
                    rotation: 0,
                    rotated: false,
                    corrections: Vec::new(),
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
                    rotated: false,
                    corrections: Vec::new(),
                },
            ],
        );
        let config = SimulationConfig::load().unwrap();
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
                assert_eq!(snapshot_velocity_q32(arclight), (0, 0));
            } else if tick == 8 {
                assert_eq!((arclight.x, arclight.z), (initial.x, initial.z));
                assert_eq!(arclight.body_rotation, initial.body_rotation);
                assert_eq!(
                    snapshot_velocity_q32(arclight),
                    (-198_556_428, -30_061_443_202)
                );
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

    /// Every slot of a grouped skill takes the construction in its way, and
    /// every slot is dropped with the lock.
    ///
    /// The Wraith of `tests/construction/wall-weapon-group.yaml` was
    /// recorded doing all of it: its core engages block 3 at tick 32 and the
    /// other three slots follow eight ticks later, while the lock stays on the
    /// Marksman; block 3 falls at tick 59 and all four slots read empty at
    /// tick 60; the core engages block 4 at tick 74 and the children are
    /// allocated again eight ticks after that, not at once.
    #[test]
    fn grouped_slots_take_the_wall_and_are_dropped_with_the_lock() {
        let config = SimulationConfig::load().unwrap();
        let (_, layout) = crate::layout::compile_with_seed(
            include_bytes!("../../../tests/construction/wall-weapon-group.yaml"),
            &config.units,
        )
        .unwrap();
        let mut simulation =
            Simulation::new(&layout, &config.units, &config.training_ground, 4242).unwrap();
        let slots_at = |simulation: &mut Simulation, tick: u64, done: &mut u64| {
            while *done < tick {
                simulation.step(*done).unwrap();
                *done += 1;
            }
            let wraith = simulation
                .actors
                .values()
                .find(|actor| actor.placement.team == 1)
                .unwrap()
                .snapshot();
            (
                wraith.mech_lock_target,
                wraith
                    .weapon_aims
                    .iter()
                    .map(|aim| aim.attack_target)
                    .collect::<Vec<_>>(),
            )
        };
        let building = |id| Some(ObjectRef::new(ObjectKind::Building, id));
        let marksman = Some(ObjectRef::new(ObjectKind::Unit, 1));
        let mut done = 0;

        let (lock, slots) = slots_at(&mut simulation, 41, &mut done);
        assert_eq!(lock, marksman, "the lock stays on the unit behind the wall");
        assert_eq!(slots, vec![building(3); 4], "all four slots on block 3");

        let (lock, slots) = slots_at(&mut simulation, 60, &mut done);
        assert_eq!(lock, None, "block 3 has fallen and the lock is dropped");
        assert_eq!(slots, vec![None; 4], "and every slot with it");

        let (_, slots) = slots_at(&mut simulation, 78, &mut done);
        assert_eq!(
            slots,
            vec![building(4), None, None, None],
            "the core has block 4; the children wait to be allocated again"
        );
    }

    /// A block that falls ends the attack on it and the lock with it, and the
    /// weapon keeps naming the block until a new target is taken.
    ///
    /// Red's Marksman of `wall-line-of-fire.yaml` fells block 6 on tick 18.
    /// The game reads it on tick 19 as idle, with no lock, and with its weapon
    /// still on block 6 — not on block 7, the next one in its line, and not on
    /// the Marksman behind the wall. `tests/construction/line-of-fire.mcscript`
    /// recorded it; the physics hash cannot see any of the three fields.
    #[test]
    fn a_fallen_block_leaves_its_attacker_idle_and_still_aimed_at_it() {
        let config = SimulationConfig::load().unwrap();
        let (_, layout) = crate::layout::compile_with_seed(
            include_bytes!("../../../tests/construction/wall-line-of-fire.yaml"),
            &config.units,
        )
        .unwrap();
        let mut simulation =
            Simulation::new(&layout, &config.units, &config.training_ground, 4242).unwrap();
        let state = |simulation: &Simulation| {
            simulation
                .actors
                .values()
                .find(|actor| actor.placement.team == 1)
                .unwrap()
                .snapshot()
        };
        for step in 0..18 {
            simulation.step(step).unwrap();
        }
        let shooting = state(&simulation);
        assert_eq!(shooting.motion_state, MotionState::Attacking);
        assert_eq!(
            shooting.weapon_aims[0].attack_target,
            Some(ObjectRef::new(ObjectKind::Building, 6))
        );

        simulation.step(18).unwrap();
        let fallen = state(&simulation);
        assert_eq!(fallen.motion_state, MotionState::Idle);
        assert_eq!(
            fallen.mech_lock_target, None,
            "the lock falls with the block"
        );
        assert_eq!(
            fallen.weapon_aims[0].attack_target,
            Some(ObjectRef::new(ObjectKind::Building, 6)),
            "the weapon still names the block it felled"
        );
    }

    /// A block that comes into the way while an attack on a unit is being
    /// prepared ends that attack, and the unit looks again before firing.
    ///
    /// Steel Ball 4 of `wall-laser.yaml` prepares a beam on the Marksman behind
    /// the wall from tick 140. Block 3 stands 11.507 metres off its line until
    /// tick 144 and 11.447 at 145, inside the 11.5 the line is wide: on tick
    /// 146 the game reads it idle, with no lock and no weapon target, and on
    /// 147 on block 3 with its lock back on the Marksman.
    /// `tests/construction/attacks.mcscript` recorded it.
    #[test]
    fn a_block_that_comes_into_the_way_ends_a_prepared_attack() {
        let config = SimulationConfig::load().unwrap();
        let (_, layout) = crate::layout::compile_with_seed(
            include_bytes!("../../../tests/construction/wall-laser.yaml"),
            &config.units,
        )
        .unwrap();
        let mut simulation =
            Simulation::new(&layout, &config.units, &config.training_ground, 4242).unwrap();
        for step in 0..145 {
            simulation.step(step).unwrap();
        }
        assert!(matches!(
            simulation.actors[&4].fight_skill_phase,
            FightSkillPhase::Prepare { .. }
        ));
        simulation.step(145).unwrap();
        let interrupted = simulation.actors[&4].snapshot();
        assert_eq!(interrupted.motion_state, MotionState::Idle);
        assert_eq!(interrupted.mech_lock_target, None);
        assert_eq!(interrupted.weapon_aims[0].attack_target, None);

        simulation.step(146).unwrap();
        let turned = simulation.actors[&4].snapshot();
        assert_eq!(turned.motion_state, MotionState::Attacking);
        assert_eq!(
            turned.mech_lock_target,
            Some(ObjectRef::new(ObjectKind::Unit, 1))
        );
        assert_eq!(
            turned.weapon_aims[0].attack_target,
            Some(ObjectRef::new(ObjectKind::Building, 3))
        );
    }

    /// Crawlers against a wall change blocks between blows, and only one that
    /// struck a block idles when it falls.
    ///
    /// In `wall-block.yaml`, Crawler 2 is pushed along the wall while it strikes
    /// block 4: when its swing is over on tick 96, block 3 is the nearer one in
    /// its line, and it reads idle with no lock before turning on block 3 at
    /// 97. Crawlers 7 and 23 are closing on block 4 without having struck it
    /// when another fells it at 118, and they go straight on to the Marksman
    /// at 119. `tests/construction/wall.mcscript` recorded the fight.
    #[test]
    fn crawlers_change_blocks_between_blows_and_only_a_striker_idles() {
        let config = SimulationConfig::load().unwrap();
        let (_, layout) = crate::layout::compile_with_seed(
            include_bytes!("../../../tests/construction/wall-block.yaml"),
            &config.units,
        )
        .unwrap();
        let mut simulation =
            Simulation::new(&layout, &config.units, &config.training_ground, 4242).unwrap();
        let read = |simulation: &Simulation, id: u64| simulation.actors[&id].snapshot();
        for step in 0..96 {
            simulation.step(step).unwrap();
        }
        let switching = read(&simulation, 2);
        assert_eq!(switching.motion_state, MotionState::Idle);
        assert_eq!(switching.mech_lock_target, None);
        simulation.step(96).unwrap();
        assert_eq!(
            read(&simulation, 2).weapon_aims[0].attack_target,
            Some(ObjectRef::new(ObjectKind::Building, 3))
        );
        for step in 97..119 {
            simulation.step(step).unwrap();
        }
        for id in [7, 23] {
            let going_on = read(&simulation, id);
            assert_eq!(going_on.motion_state, MotionState::Moving, "Crawler {id}");
            assert_eq!(
                going_on.mech_lock_target,
                Some(ObjectRef::new(ObjectKind::Unit, 1)),
                "Crawler {id}"
            );
        }
    }

    /// A Marksman whose shot kills its target before the attack is over holds
    /// through its cooling when the replacement cannot be attacked at once.
    ///
    /// In `crawlers-vs-marksman.yaml` the Marksman releases at tick 135 and
    /// kills Crawler 8 at 136. The Crawler the selector answers, 4, is out of
    /// its attack angle, so for ticks 137 to 140 it reads idle with no lock and
    /// its weapon on Crawler 4; at 141 the weapon clears; at 142 it locks
    /// Crawler 7, whom the Crawlers' approach has made the selector's answer.
    #[test]
    fn a_marksman_holds_through_its_cooling_after_a_kill_it_cannot_follow() {
        let config = SimulationConfig::load().unwrap();
        let (_, layout) = crate::layout::compile_with_seed(
            include_bytes!("../../../tests/regression/crawlers-vs-marksman.yaml"),
            &config.units,
        )
        .unwrap();
        let mut simulation =
            Simulation::new(&layout, &config.units, &config.training_ground, 4242).unwrap();
        let mut states = BTreeMap::new();
        for step in 0..142u64 {
            simulation.step(step).unwrap();
            if step + 1 >= 137 {
                states.insert(step + 1, simulation.actors[&1].snapshot());
            }
        }
        for tick in 137..=140 {
            let state = &states[&tick];
            assert_eq!(state.motion_state, MotionState::Idle, "tick {tick}");
            assert_eq!(state.mech_lock_target, None, "tick {tick}");
            assert_eq!(
                state.weapon_aims[0].attack_target,
                Some(ObjectRef::new(ObjectKind::Unit, 4)),
                "tick {tick}"
            );
        }
        assert_eq!(states[&141].weapon_aims[0].attack_target, None);
        assert_eq!(
            states[&142].mech_lock_target,
            Some(ObjectRef::new(ObjectKind::Unit, 7))
        );
    }

    /// The body and the weapons have separate targets, and a recording reports
    /// both.
    ///
    /// Red's Marksman locks onto the Marksman behind blue's wall and shoots
    /// block 6, which stands in its line of fire: `docs/rules/combat.md`
    /// measured the lock staying on the unit while the weapon holds the block,
    /// and `tests/construction/line-of-fire.mcscript` recorded this
    /// exact fight. The physics hash cannot see either field, which is why
    /// this pins them here.
    #[test]
    fn a_wall_in_the_way_takes_the_weapon_and_leaves_the_lock() {
        let config = SimulationConfig::load().unwrap();
        let (_, layout) = crate::layout::compile_with_seed(
            include_bytes!("../../../tests/construction/wall-line-of-fire.yaml"),
            &config.units,
        )
        .unwrap();
        let mut simulation =
            Simulation::new(&layout, &config.units, &config.training_ground, 4242).unwrap();
        for step in 0..2 {
            simulation.step(step).unwrap();
        }
        let marksman = simulation
            .actors
            .values()
            .find(|actor| actor.placement.team == 1)
            .unwrap();
        let behind_the_wall = simulation
            .actors
            .values()
            .find(|actor| actor.placement.team == 0)
            .unwrap()
            .placement
            .unit_id;
        let state = marksman.snapshot();

        assert_eq!(
            state.mech_lock_target,
            Some(ObjectRef::new(ObjectKind::Unit, behind_the_wall)),
            "the body keeps the unit it searched for"
        );
        assert_eq!(
            state.weapon_aims[0].attack_target,
            Some(ObjectRef::new(ObjectKind::Building, 6)),
            "the weapon holds the block in the way"
        );
        assert_eq!(state.motion_state, MotionState::Attacking);
        assert_eq!(
            marksman.lock_target,
            Some(FightActorRef::Unit(behind_the_wall)),
            "the lock is never overwritten by the block"
        );
    }

    #[test]
    fn first_rvo_solve_avoids_same_formation_at_tick_eight() {
        let config = SimulationConfig::load().unwrap();
        let (_, layout) = crate::layout::compile_with_seed(
            include_bytes!("../../../tests/regression/steel-balls-vs-steel-balls.yaml"),
            &config.units,
        )
        .unwrap();
        let mut simulation = Simulation::new(
            &layout,
            &config.units,
            &config.training_ground,
            1_787_831_322,
        )
        .unwrap();

        for step in 0..8 {
            simulation.step(step).unwrap();
        }

        assert_eq!(
            snapshot_velocity_q32(&simulation.actors[&1]),
            (-3_142_838_517, 68_510_718_647)
        );
        assert_eq!(
            snapshot_velocity_q32(&simulation.actors[&2]),
            (-1_768_777_992, 61_456_524_119)
        );
        assert_eq!(
            snapshot_velocity_q32(&simulation.actors[&3]),
            (2_054_176_502, 68_688_002_951)
        );
    }

    #[test]
    fn first_split_rvo_tree_uses_the_zero_position_buffer() {
        let config = SimulationConfig::load().unwrap();
        let (_, layout) = crate::layout::compile_with_seed(
            include_bytes!("../../../tests/regression/rhino-vs-crawlers.yaml"),
            &config.units,
        )
        .unwrap();
        let mut simulation = Simulation::new(
            &layout,
            &config.units,
            &config.training_ground,
            1_787_748_319,
        )
        .unwrap();

        for step in 0..8 {
            simulation.step(step).unwrap();
        }

        assert_eq!(
            snapshot_velocity_q32(&simulation.actors[&11]),
            (10_146_579_184, -67_965_446_880)
        );
    }

    #[test]
    fn rvo_boundary_recalculates_velocity_from_the_published_target_and_current_position() {
        let layout = CompiledLayout::of_units(
            1,
            vec![
                Placement {
                    team: 0,
                    unit_id: 1,
                    formation_id: 1,
                    formation_index: 0,
                    type_name: "marksman".to_owned(),
                    world_x: 0,
                    world_z: -50,
                    rotation: 0,
                    rotated: false,
                    corrections: Vec::new(),
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
                    rotated: false,
                    corrections: Vec::new(),
                },
            ],
        );
        let config = SimulationConfig::load().unwrap();
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
    #[allow(clippy::too_many_lines)]
    fn range_entry_stops_only_after_the_two_stage_rvo_delay() {
        let layout = CompiledLayout::of_units(
            1,
            vec![
                Placement {
                    team: 0,
                    unit_id: 1,
                    formation_id: 1,
                    formation_index: 0,
                    type_name: "marksman".to_owned(),
                    world_x: 0,
                    world_z: -50,
                    rotation: 0,
                    rotated: false,
                    corrections: Vec::new(),
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
                    rotated: false,
                    corrections: Vec::new(),
                },
            ],
        );
        let config = SimulationConfig::load().unwrap();
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
                    assert_eq!(
                        snapshot_velocity_q32(arclight),
                        (
                            arclight.current_velocity_x_q32,
                            arclight.current_velocity_z_q32
                        )
                    );
                    tick_121_raw_position = Some((arclight.x_q32, arclight.z_q32));
                    tick_121_body_rotation = Some(arclight.body_rotation);
                }
                122 => {
                    assert_eq!(arclight.z, 60_800);
                    assert_eq!(arclight.motion, MotionState::Attacking);
                    assert_eq!(
                        snapshot_velocity_q32(arclight),
                        (
                            arclight.current_velocity_x_q32,
                            arclight.current_velocity_z_q32
                        )
                    );
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
                    assert_eq!(
                        snapshot_velocity_q32(arclight),
                        (
                            arclight.current_velocity_x_q32,
                            arclight.current_velocity_z_q32
                        )
                    );
                    if tick == 124 {
                        assert_eq!(arclight.published_speed_q32, space_to_q32(7_000));
                        assert_eq!(arclight.solver_speed_q32, 0);
                    }
                }
                128 => {
                    assert_eq!(arclight.z, 58_700);
                    assert_eq!(arclight.motion, MotionState::Attacking);
                    assert_eq!(snapshot_velocity_q32(arclight), (0, 0));
                    assert_eq!(arclight.published_speed_q32, 0);
                }
                _ => {}
            }
        }
    }

    #[test]
    fn tick_fifteen_aim_uses_raw_q32_positions() {
        let layout = CompiledLayout::of_units(
            1,
            vec![
                Placement {
                    team: 0,
                    unit_id: 1,
                    formation_id: 1,
                    formation_index: 0,
                    type_name: "marksman".to_owned(),
                    world_x: 0,
                    world_z: -50,
                    rotation: 0,
                    rotated: false,
                    corrections: Vec::new(),
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
                    rotated: false,
                    corrections: Vec::new(),
                },
            ],
        );
        let config = SimulationConfig::load().unwrap();
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
        let layout = CompiledLayout::of_units(
            1,
            vec![
                Placement {
                    team: 0,
                    unit_id: 1,
                    formation_id: 1,
                    formation_index: 0,
                    type_name: "marksman".to_owned(),
                    world_x: 0,
                    world_z: -50,
                    rotation: 0,
                    rotated: false,
                    corrections: Vec::new(),
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
                    rotated: false,
                    corrections: Vec::new(),
                },
            ],
        );
        let config = SimulationConfig::load().unwrap();
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
    fn stopped_attacker_rate_limits_aim_without_rotating_root_body() {
        let layout = CompiledLayout::of_units(
            1,
            vec![
                Placement {
                    team: 0,
                    unit_id: 1,
                    formation_id: 1,
                    formation_index: 0,
                    type_name: "marksman".to_owned(),
                    world_x: 0,
                    world_z: -50,
                    rotation: 0,
                    rotated: false,
                    corrections: Vec::new(),
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
                    rotated: false,
                    corrections: Vec::new(),
                },
            ],
        );
        let config = SimulationConfig::load().unwrap();
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
        let maximum = q32_mul(
            mdeg_to_degrees_q32(simulation.actors[&1].rules.rotate_speed_mdeg_per_second()),
            NATIVE_LOGIC_DELTA_Q32,
        );
        let expected_limited = degrees_q32_to_mdeg(rotate_towards_q32(
            mdeg_to_degrees_q32(initial_aim),
            mdeg_to_degrees_q32(expected_aim),
            maximum,
        ));
        simulation.step_actor(1, 1, &mut Vec::new()).unwrap();
        let marksman = &simulation.actors[&1];

        assert_eq!(marksman.motion, MotionState::Attacking);
        assert_eq!(marksman.body_rotation, root_body);
        assert_ne!(marksman.aim_rotation, initial_aim);
        assert_ne!(marksman.aim_rotation, expected_aim);
        assert_eq!(marksman.aim_rotation, expected_limited);
    }
}
