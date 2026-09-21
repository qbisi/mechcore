//! Fixed-point sampled-RVO used by build 2259 units.
//!
//! The implementation follows the target-build
//! `GRPF.RVO.Sampled.Agent`/`RVOAgentFixed` path. Address-normalized ISIL for
//! the sampled Agent, fixed Agent, Simulator, Quadtree and controller is
//! byte-identical between builds 2227 and 2259, so this module is ported from
//! the already tested 2227 implementation and must still pass build-2259
//! native tick comparison before review. It consumes only ordinary simulation
//! state; private native RVO buffers are neither replay inputs nor capture
//! requirements.

use std::collections::BTreeMap;

use super::{
    Q32_ONE, fpcs_acos_fastest, fpcs_atan2_fastest, fpcs_cos_fastest, fpcs_sin_fastest,
    fpcs_sqrt_fastest,
};

// Build 2259's `RVOControllerFixed.Init` overwrites the base controller's
// constructor default of 10 with 20 before it creates either a movable member
// or an immovable building agent. Native Crawler diagnostics independently
// confirm 20 retained neighbours and 20 generated velocity obstacles.
const MAX_NEIGHBOURS: usize = 20;
const TRACE_ITERATIONS: i64 = 50;
const DESIRED_VELOCITY_WEIGHT: i64 = Q32_ONE / 10;
const MAX_SPEED_WEIGHT: i64 = Q32_ONE * 3;
// Native `Agent.Trace` reads `FPoint.C2` directly. Build 2259 initializes it
// to raw 0x33333333; reconstructing it from two separately truncated C1em1
// values makes the Crawler's first trace step 16 raw units too small.
const TRACE_SPEED_WEIGHT: i64 = 0x3333_3333;
// `RVOAgentFixed.EvaluateGradient`, however, constructs the speed penalty as
// C1em1 + C1em1. Preserve the independently truncated operands here: this is
// one raw unit smaller than C2 and is visible in the native gradient.
const GRADIENT_SPEED_WEIGHT: i64 = (Q32_ONE / 10) * 2;
const VO_SCALE: i64 = Q32_ONE * 2;
const COLLISION_RESPONSE: i64 = Q32_ONE * 3 / 10;
// `RVOAgentFixed.GenerateOpponentVOs` uses `FPoint.Hundred` as the inverse
// horizon for agents in a different RVO group. This group is an avoidance
// purpose, not the owning fight team.
const DIFFERENT_GROUP_INVERSE_HORIZON: i64 = Q32_ONE * 100;
const EPSILON: i64 = Q32_ONE / 1_000;
const NORMALIZE_EPSILON: i64 = Q32_ONE / 10_000;
const VECTOR_NORMALIZE_EPSILON: i64 = Q32_ONE / 100_000;
const DEFAULT_SYMMETRY_BIAS: i64 = Q32_ONE / 10;
// `RVOController` constructs this field as 2 seconds, but every serialized
// build-2259 Rhino and Crawler prefabs override it
// with the Q32.32 value 12. `UpdateAgentProperties` then copies that field to
// the sampled RVO agent on every refresh.
const DEFAULT_AGENT_TIME_HORIZON: i64 = Q32_ONE * 12;
const QUADTREE_LEAF_SIZE: u8 = 15;
const QUADTREE_MAX_DEPTH: u8 = 11;

/// Native `GRPF.RVO.AgentSizeType` ordering used by build 2259.
///
/// The ordering is behavioural: for two members in the same RVO group, the
/// smaller observer uses both inner radii while an equal-or-larger observer
/// uses both outer radii. Fight-team ownership is not the group selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum AgentSizeType {
    Xs,
    S,
    M,
    L,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum AgentKey {
    Unit(u64),
    Building(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct FixedVec2 {
    pub(crate) x: i64,
    pub(crate) y: i64,
}

impl FixedVec2 {
    pub(crate) const ZERO: Self = Self { x: 0, y: 0 };

    fn add(self, other: Self) -> Self {
        Self {
            x: self.x.saturating_add(other.x),
            y: self.y.saturating_add(other.y),
        }
    }

    fn sub(self, other: Self) -> Self {
        Self {
            x: self.x.saturating_sub(other.x),
            y: self.y.saturating_sub(other.y),
        }
    }

    fn mul(self, scalar: i64) -> Self {
        Self {
            x: q32_mul(self.x, scalar),
            y: q32_mul(self.y, scalar),
        }
    }

    fn div(self, scalar: i64) -> Self {
        // Native `FVector2.op_Division` computes one shared reciprocal with
        // `FPoint.RawDiv(One, scalar)` and then multiplies both components by
        // it.  Direct component-wise division has a different Q32.32 rounding
        // path and is observably wrong inside RVO normalization/gradients.
        self.mul(q32_div(Q32_ONE, scalar))
    }

    fn dot(self, other: Self) -> i64 {
        q32_mul(self.x, other.x).saturating_add(q32_mul(self.y, other.y))
    }

    fn sqr_magnitude(self) -> i64 {
        self.dot(self)
    }

    fn magnitude(self) -> i64 {
        fpcs_sqrt_fastest(q32_mul(self.x, self.x).saturating_add(q32_mul(self.y, self.y)))
    }

    fn normalized(self) -> Self {
        let magnitude = self.magnitude();
        // Native `FVector2.Normalize()` returns zero only while the magnitude
        // is strictly below `FPoint.C1em5`; equality takes the division path.
        if normalization_divides(magnitude) {
            self.div(magnitude)
        } else {
            Self::ZERO
        }
    }

    fn clockwise_tangent(self) -> Self {
        Self {
            x: self.y,
            y: self.x.saturating_neg(),
        }
    }

    fn counter_clockwise_tangent(self) -> Self {
        Self {
            x: self.y.saturating_neg(),
            y: self.x,
        }
    }
}

fn normalization_divides(magnitude: i64) -> bool {
    magnitude >= VECTOR_NORMALIZE_EPSILON
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AgentInput {
    pub(crate) key: AgentKey,
    /// Synchronized `RVOMainLayer` (`Ground = 1`, `Air = 2`). Native
    /// `RVOAgentFixed.InsertAgentNeighbour` rejects a candidate before the
    /// distance insertion when this value differs.
    pub(crate) main_layer: i32,
    /// Native sampled-RVO collision bit assigned by
    /// `RVOControllerFixed.RefreshCollideInfo`.
    pub(crate) layer: u32,
    /// Native collision mask. Ordinary movable agents collide with their own
    /// collider-priority layer and all higher-priority layers.
    pub(crate) collides_with: u32,
    pub(crate) group: i32,
    /// Whether an agent of the same group passes through this one rather than
    /// avoiding it. A construction sinks for its own side: it is still among
    /// that side's neighbours, and yields them no velocity obstacle.
    pub(crate) passable_by_own_group: bool,
    /// Native sampled-agent lock. Immovable towers set this bit, which
    /// makes a movable neighbour take the full avoidance responsibility.
    pub(crate) locked: bool,
    /// Position published before native's current `BufferSwitch`. The
    /// double-buffered simulator builds its quadtree from this position, then
    /// switches every agent to `position` before querying the already-built
    /// tree.
    pub(crate) tree_position: FixedVec2,
    pub(crate) position: FixedVec2,
    pub(crate) current_velocity: FixedVec2,
    pub(crate) desired_velocity: FixedVec2,
    pub(crate) desired_target_delta: FixedVec2,
    pub(crate) desired_speed: i64,
    pub(crate) max_speed: i64,
    /// Last published `Agent.CalculatedSpeed` (`Agent + 0x110`). Native
    /// quadtree nodes aggregate this value for branch reachability; the query
    /// agent's current `maxSpeed` remains the separate query-speed operand.
    pub(crate) published_calculated_speed: i64,
    pub(crate) radius_outer: i64,
    pub(crate) radius_inner: i64,
    pub(crate) size: AgentSizeType,
    pub(crate) priority: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AgentSolution {
    /// Offset from the agent position at solve time to the target point stored
    /// by native `Agent.CalculateVelocity`.
    pub(crate) target_delta: FixedVec2,
    /// Native `CalculatedSpeed`, independent from the target-point distance.
    pub(crate) speed: i64,
}

#[derive(Debug, Clone, Copy)]
struct QuadtreeRect {
    min: FixedVec2,
    max: FixedVec2,
}

impl QuadtreeRect {
    fn center(self) -> FixedVec2 {
        // `FRect.get_center` evaluates x/y plus half the stored width/height.
        // Keeping that order matters when the raw width is odd.
        FixedVec2 {
            x: self
                .min
                .x
                .saturating_add(q32_mul(self.max.x.saturating_sub(self.min.x), Q32_ONE / 2)),
            y: self
                .min
                .y
                .saturating_add(q32_mul(self.max.y.saturating_sub(self.min.y), Q32_ONE / 2)),
        }
    }

    fn child(self, quadrant: usize) -> Self {
        let center = self.center();
        match quadrant {
            0 => Self {
                min: self.min,
                max: center,
            },
            1 => Self {
                min: FixedVec2 {
                    x: self.min.x,
                    y: center.y,
                },
                max: FixedVec2 {
                    x: center.x,
                    y: self.max.y,
                },
            },
            2 => Self {
                min: FixedVec2 {
                    x: center.x,
                    y: self.min.y,
                },
                max: FixedVec2 {
                    x: self.max.x,
                    y: center.y,
                },
            },
            3 => Self {
                min: center,
                max: self.max,
            },
            _ => unreachable!("quadtree quadrant is in 0..4"),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct QuadtreeNode {
    child00: usize,
    head: Option<usize>,
    count: u8,
    max_speed: i64,
}

struct NativeQuadtree<'a> {
    inputs: &'a [AgentInput],
    nodes: Vec<QuadtreeNode>,
    next: Vec<Option<usize>>,
    bounds: QuadtreeRect,
}

impl<'a> NativeQuadtree<'a> {
    fn build(inputs: &'a [AgentInput]) -> Option<Self> {
        let first = inputs.first()?;
        let mut bounds = QuadtreeRect {
            min: first.tree_position,
            max: first.tree_position,
        };
        for input in &inputs[1..] {
            bounds.min.x = fpoint_min(bounds.min.x, input.tree_position.x);
            bounds.min.y = fpoint_min(bounds.min.y, input.tree_position.y);
            bounds.max.x = fpoint_max(bounds.max.x, input.tree_position.x);
            bounds.max.y = fpoint_max(bounds.max.y, input.tree_position.y);
        }

        let mut tree = Self {
            inputs,
            nodes: vec![QuadtreeNode::default()],
            next: vec![None; inputs.len()],
            bounds,
        };
        for index in 0..inputs.len() {
            tree.insert(index);
        }
        tree.calculate_max_speed(0);
        Some(tree)
    }

    fn insert(&mut self, agent_index: usize) {
        let mut node_index = 0usize;
        let mut rect = self.bounds;
        let mut depth = 1u8;
        loop {
            if self.nodes[node_index].child00 == node_index {
                if self.nodes[node_index].count >= QUADTREE_LEAF_SIZE && depth < QUADTREE_MAX_DEPTH
                {
                    let child00 = self.nodes.len();
                    self.nodes.extend((0..4).map(|quadrant| QuadtreeNode {
                        child00: child00 + quadrant,
                        ..QuadtreeNode::default()
                    }));
                    self.nodes[node_index].child00 = child00;
                    self.distribute(node_index, rect);
                } else {
                    self.prepend(node_index, agent_index);
                    return;
                }
            }

            let quadrant = Self::quadrant(self.inputs[agent_index].tree_position, rect);
            node_index = self.nodes[node_index].child00 + quadrant;
            rect = rect.child(quadrant);
            depth = depth.saturating_add(1);
        }
    }

    fn quadrant(position: FixedVec2, rect: QuadtreeRect) -> usize {
        let center = rect.center();
        usize::from(fpoint_less_than(center.x, position.x)) * 2
            + usize::from(fpoint_less_than(center.y, position.y))
    }

    fn prepend(&mut self, node_index: usize, agent_index: usize) {
        self.next[agent_index] = self.nodes[node_index].head;
        self.nodes[node_index].head = Some(agent_index);
        self.nodes[node_index].count = self.nodes[node_index].count.saturating_add(1);
    }

    fn distribute(&mut self, node_index: usize, rect: QuadtreeRect) {
        let child00 = self.nodes[node_index].child00;
        let mut current = self.nodes[node_index].head;
        self.nodes[node_index].head = None;
        while let Some(agent_index) = current {
            let old_next = self.next[agent_index];
            let quadrant = Self::quadrant(self.inputs[agent_index].tree_position, rect);
            let child_index = child00 + quadrant;
            self.next[agent_index] = self.nodes[child_index].head;
            self.nodes[child_index].head = Some(agent_index);
            current = old_next;
        }
        self.nodes[node_index].count = 0;
    }

    fn calculate_max_speed(&mut self, node_index: usize) -> i64 {
        let child00 = self.nodes[node_index].child00;
        let max_speed = if child00 == node_index {
            let mut current = self.nodes[node_index].head;
            let mut max_speed = 0;
            while let Some(agent_index) = current {
                max_speed = fpoint_max(
                    max_speed,
                    self.inputs[agent_index].published_calculated_speed,
                );
                current = self.next[agent_index];
            }
            max_speed
        } else {
            let child0 = self.calculate_max_speed(child00);
            (1..4).fold(child0, |current, quadrant| {
                fpoint_max(current, self.calculate_max_speed(child00 + quadrant))
            })
        };
        self.nodes[node_index].max_speed = max_speed;
        max_speed
    }

    fn query(&self, agent: &AgentInput) -> Vec<&'a AgentInput> {
        let mut state = QuadtreeQueryState {
            agent,
            neighbours: Vec::new(),
            max_radius: None,
        };
        self.query_rec(0, self.bounds, &mut state);
        state
            .neighbours
            .into_iter()
            .map(|(index, _)| &self.inputs[index])
            .collect()
    }

    fn query_rec(&self, node_index: usize, rect: QuadtreeRect, state: &mut QuadtreeQueryState<'_>) {
        let node = self.nodes[node_index];
        let reachable = fpoint_max(
            q32_mul(
                node.max_speed.saturating_add(state.agent.max_speed),
                DEFAULT_AGENT_TIME_HORIZON,
            ),
            state.agent.radius_outer,
        )
        .saturating_add(state.agent.radius_outer);
        let mut distance = state
            .max_radius
            .map_or(reachable, |max_radius| fpoint_min(reachable, max_radius));
        if node.child00 == node_index {
            let leaf_range_sq = q32_mul(distance, distance);
            let mut current = node.head;
            while let Some(candidate_index) = current {
                let returned_range_sq = insert_neighbour(
                    state.agent,
                    self.inputs,
                    candidate_index,
                    leaf_range_sq,
                    &mut state.neighbours,
                );
                let narrows_range = state.max_radius.is_none_or(|max_radius| {
                    fpoint_less_than(returned_range_sq, q32_mul(max_radius, max_radius))
                });
                if narrows_range {
                    state.max_radius = Some(fpcs_sqrt_fastest(returned_range_sq));
                }
                current = self.next[candidate_index];
            }
            return;
        }

        let center = rect.center();
        if fpoint_less_than(state.agent.position.x.saturating_sub(distance), center.x) {
            if fpoint_less_than(state.agent.position.y.saturating_sub(distance), center.y) {
                distance = self.visit_child(node, rect, 0, distance, state);
            }
            if fpoint_less_than(center.y, state.agent.position.y.saturating_add(distance)) {
                distance = self.visit_child(node, rect, 1, distance, state);
            }
        }
        if fpoint_less_than(center.x, state.agent.position.x.saturating_add(distance)) {
            if fpoint_less_than(state.agent.position.y.saturating_sub(distance), center.y) {
                distance = self.visit_child(node, rect, 2, distance, state);
            }
            if fpoint_less_than(center.y, state.agent.position.y.saturating_add(distance)) {
                self.visit_child(node, rect, 3, distance, state);
            }
        }
    }

    fn visit_child(
        &self,
        node: QuadtreeNode,
        rect: QuadtreeRect,
        quadrant: usize,
        distance: i64,
        state: &mut QuadtreeQueryState<'_>,
    ) -> i64 {
        self.query_rec(node.child00 + quadrant, rect.child(quadrant), state);
        state
            .max_radius
            .map_or(distance, |max_radius| fpoint_min(distance, max_radius))
    }
}

struct QuadtreeQueryState<'a> {
    agent: &'a AgentInput,
    neighbours: Vec<(usize, i64)>,
    max_radius: Option<i64>,
}

fn insert_neighbour(
    agent: &AgentInput,
    inputs: &[AgentInput],
    candidate_index: usize,
    range_sq: i64,
    neighbours: &mut Vec<(usize, i64)>,
) -> i64 {
    let candidate = &inputs[candidate_index];
    if candidate.key == agent.key
        || candidate.main_layer != agent.main_layer
        || agent.collides_with & candidate.layer == 0
    {
        return range_sq;
    }
    let distance = candidate.position.sub(agent.position).sqr_magnitude();
    if !fpoint_less_than(distance, range_sq) {
        return range_sq;
    }

    let insertion = neighbours
        .iter()
        .position(|(_, current)| fpoint_less_than(distance, *current))
        .unwrap_or(neighbours.len());
    neighbours.insert(insertion, (candidate_index, distance));
    neighbours.truncate(MAX_NEIGHBOURS);
    if neighbours.len() == MAX_NEIGHBOURS {
        neighbours[MAX_NEIGHBOURS - 1].1
    } else {
        range_sq
    }
}

#[derive(Debug, Clone, Copy)]
struct VelocityObstacle {
    line1: FixedVec2,
    line2: FixedVec2,
    dir1: FixedVec2,
    dir2: FixedVec2,
    cutoff_line: FixedVec2,
    cutoff_dir: FixedVec2,
    circle_center: FixedVec2,
    colliding: bool,
    radius: i64,
    weight_factor: i64,
}

impl VelocityObstacle {
    fn new(
        center: FixedVec2,
        offset: FixedVec2,
        radius: i64,
        inverse_horizon: i64,
        inverse_delta_time: i64,
    ) -> Self {
        let circle_center = center.mul(inverse_horizon).add(offset);
        let center_magnitude = center.magnitude();
        let center_sqr_magnitude = center.sqr_magnitude();
        let radius_sq = q32_mul(radius, radius);
        let ratio = if radius_sq > 0 {
            // Native `Agent.VO..ctor` reads `center.sqrMagnitude`, divides it
            // by the squared radius and then squares that quotient for the
            // exponent. Keep both squaring stages: using the vector magnitude
            // here overweights close same-size neighbours and changes the
            // selected avoidance branch.
            q32_div(center_sqr_magnitude, radius_sq)
        } else {
            i64::MAX
        };
        let exponent = q32_mul(ratio, ratio).saturating_neg();
        let weight_factor = Q32_ONE
            .saturating_add(q32_mul(Q32_ONE * 4, q32_exp(exponent)))
            .max(Q32_ONE);
        // Build 2227 calls `FPoint.op_LessThan(centerMagnitude, radius)` and
        // enters the collision branch only when that strict comparison holds.
        // Exact contact therefore takes the non-colliding tangent path.
        if center_magnitude < radius {
            let line1 = center
                .normalized()
                .mul(
                    center_magnitude
                        .saturating_sub(radius)
                        .saturating_sub(EPSILON),
                )
                .mul(COLLISION_RESPONSE)
                .mul(inverse_delta_time)
                .add(offset);
            return Self {
                line1,
                line2: FixedVec2::ZERO,
                dir1: line1.sub(offset).clockwise_tangent().normalized(),
                dir2: FixedVec2::ZERO,
                cutoff_line: FixedVec2::ZERO,
                cutoff_dir: FixedVec2::ZERO,
                circle_center,
                colliding: true,
                radius: 0,
                weight_factor,
            };
        }

        let scaled_center = center.mul(inverse_horizon);
        let scaled_radius = q32_mul(radius, inverse_horizon);
        let scaled_magnitude = scaled_center.magnitude();
        let global_center = scaled_center.add(offset);
        let cutoff_distance = scaled_magnitude
            .saturating_sub(scaled_radius)
            .saturating_add(EPSILON);
        let cutoff_relative = scaled_center.normalized().mul(cutoff_distance);
        let cutoff_line = cutoff_relative.add(offset);
        let cutoff_dir = cutoff_relative.counter_clockwise_tangent().normalized();

        // `FPoint.Atan2` and `FPoint.Acos` tail-call the build-2227
        // `FPCSMath.*Fastest` entry points. The separately named
        // `Atan2Precise` wrapper is not used by this constructor.
        let center_angle = fpcs_atan2_fastest(center.y.saturating_neg(), center.x.saturating_neg());
        let tangent_delta = fpcs_acos_fastest(q32_div(radius, center_magnitude)).abs();
        let first_angle = center_angle.saturating_add(tangent_delta);
        let second_angle = center_angle.saturating_sub(tangent_delta);
        let circle_dir1 = FixedVec2 {
            x: fpcs_cos_fastest(first_angle),
            y: fpcs_sin_fastest(first_angle),
        };
        let circle_dir2 = FixedVec2 {
            x: fpcs_cos_fastest(second_angle),
            y: fpcs_sin_fastest(second_angle),
        };
        let line1 = circle_dir1.mul(scaled_radius).add(global_center);
        let line2 = circle_dir2.mul(scaled_radius).add(global_center);
        Self {
            line1,
            line2,
            dir1: circle_dir1.clockwise_tangent(),
            dir2: circle_dir2.clockwise_tangent(),
            cutoff_line,
            cutoff_dir,
            circle_center,
            colliding: false,
            radius: scaled_radius,
            weight_factor,
        }
    }

    fn signed_distance(line: FixedVec2, direction: FixedVec2, point: FixedVec2) -> i64 {
        let relative = point.sub(line);
        q32_mul(relative.x, direction.y).saturating_sub(q32_mul(direction.x, relative.y))
    }

    fn gradient(self, point: FixedVec2) -> (FixedVec2, i64) {
        if self.colliding {
            let distance = Self::signed_distance(self.line1, self.dir1, point);
            return if distance >= 0 {
                (self.dir1.counter_clockwise_tangent(), distance)
            } else {
                (FixedVec2::ZERO, 0)
            };
        }
        let cutoff_distance = Self::signed_distance(self.cutoff_line, self.cutoff_dir, point);
        if cutoff_distance <= 0 {
            return (FixedVec2::ZERO, 0);
        }
        let distance1 = Self::signed_distance(self.line1, self.dir1, point);
        let distance2 = Self::signed_distance(self.line2, self.dir2, point);
        if distance1 < 0 || distance2 < 0 {
            return (FixedVec2::ZERO, 0);
        }
        if point.sub(self.line1).dot(self.dir1) > 0 && point.sub(self.line2).dot(self.dir2) < 0 {
            let from_center = point.sub(self.circle_center);
            let distance = from_center.magnitude();
            return (
                from_center.normalized(),
                self.radius.saturating_sub(distance),
            );
        }
        if distance1 < distance2 {
            (self.dir1.counter_clockwise_tangent(), distance1)
        } else {
            (self.dir2.counter_clockwise_tangent(), distance2)
        }
    }

    fn scaled_gradient(self, point: FixedVec2) -> (FixedVec2, i64) {
        let (mut gradient, mut weight) = self.gradient(point);
        if weight > 0 {
            let scale = q32_mul(VO_SCALE, self.weight_factor);
            gradient = gradient.mul(scale);
            weight = q32_mul(weight, scale).saturating_add(Q32_ONE);
        }
        (gradient, weight)
    }
}

pub(crate) fn solve_agents(
    inputs: &[AgentInput],
    inverse_delta_time: i64,
) -> BTreeMap<AgentKey, AgentSolution> {
    inputs
        .iter()
        .map(|agent| {
            let neighbours = nearest_neighbours(agent, inputs);
            // A construction of the agent's own group is still one of its
            // neighbours — it takes one of the twenty places, as the native
            // sidecar shows for a Crawler crossing its own wall — and yields
            // no velocity obstacle: twenty neighbours, seventeen obstacles.
            let obstacles = neighbours
                .into_iter()
                .filter(|other| !(other.passable_by_own_group && other.group == agent.group))
                .map(|other| neighbour_obstacle(agent, other, inverse_delta_time))
                .collect::<Vec<_>>();
            (agent.key, solve_agent(agent, &obstacles))
        })
        .collect()
}

fn nearest_neighbours<'a>(agent: &AgentInput, inputs: &'a [AgentInput]) -> Vec<&'a AgentInput> {
    NativeQuadtree::build(inputs)
        .map(|tree| tree.query(agent))
        .unwrap_or_default()
}

fn neighbour_obstacle(
    agent: &AgentInput,
    other: &AgentInput,
    inverse_delta_time: i64,
) -> VelocityObstacle {
    let center = other.position.sub(agent.position);
    if agent.group != other.group {
        // `RVOAgentFixed.GenerateOpponentVOs`: a different RVO group uses the
        // observer's outer radius plus the target's inner radius and a fixed
        // 0.01-second horizon. Fight teams do not select this branch.
        return VelocityObstacle::new(
            center,
            FixedVec2::ZERO,
            agent.radius_outer.saturating_add(other.radius_inner),
            DIFFERENT_GROUP_INVERSE_HORIZON,
            inverse_delta_time,
        );
    }
    let avoidance_strength = if other.locked {
        Q32_ONE
    } else {
        let priority_sum = agent.priority.saturating_add(other.priority);
        if priority_sum > 0 {
            q32_div(other.priority, priority_sum)
        } else {
            Q32_ONE / 2
        }
    };
    let other_optimal = lerp(
        other.current_velocity,
        other.desired_velocity,
        avoidance_strength.saturating_mul(2).saturating_sub(Q32_ONE),
    );
    let center_velocity = lerp(agent.current_velocity, other_optimal, avoidance_strength);
    let radius = if agent.size < other.size {
        agent.radius_inner.saturating_add(other.radius_inner)
    } else {
        agent.radius_outer.saturating_add(other.radius_outer)
    };
    VelocityObstacle::new(
        center,
        center_velocity,
        radius,
        q32_div(Q32_ONE, DEFAULT_AGENT_TIME_HORIZON),
        inverse_delta_time,
    )
}

fn solve_agent(agent: &AgentInput, obstacles: &[VelocityObstacle]) -> AgentSolution {
    let mut desired = agent.desired_velocity;
    let mut target = agent.desired_target_delta;
    let inside = bias_desired_velocity(obstacles, &mut desired, &mut target);
    if inside {
        let calculated =
            gradient_descent(agent, obstacles, agent.current_velocity, desired, desired);
        AgentSolution {
            target_delta: calculated,
            speed: calculated.magnitude().min(agent.max_speed),
        }
    } else {
        // The unobstructed branch preserves the submitted target point rather
        // than replacing it with a one-second desired-velocity endpoint.
        AgentSolution {
            target_delta: target,
            speed: agent.desired_speed,
        }
    }
}

fn bias_desired_velocity(
    obstacles: &[VelocityObstacle],
    desired: &mut FixedVec2,
    target: &mut FixedVec2,
) -> bool {
    let desired_magnitude = desired.magnitude();
    // Native initializes the accumulator from FPoint.Zero and applies
    // FPoint.Max for every VO. A negative radial weight therefore clamps to
    // zero; it must not become a negative symmetry-bias angle while the
    // method returns `false`.
    let max_value = obstacles.iter().fold(0, |current, obstacle| {
        fpoint_max(current, obstacle.gradient(*desired).1)
    });
    let inside = fpoint_less_than(0, max_value);
    if desired_magnitude < EPSILON {
        return inside;
    }
    let angle = fpoint_min(DEFAULT_SYMMETRY_BIAS, q32_div(max_value, desired_magnitude));
    *desired = desired.add(desired.clockwise_tangent().mul(angle));
    *target = target.add(target.clockwise_tangent().mul(angle));
    inside
}

fn gradient_descent(
    agent: &AgentInput,
    obstacles: &[VelocityObstacle],
    first: FixedVec2,
    second: FixedVec2,
    desired_velocity: FixedVec2,
) -> FixedVec2 {
    let (first_point, first_score) = trace(agent, obstacles, first, desired_velocity);
    let (second_point, second_score) = trace(agent, obstacles, second, desired_velocity);
    if fpoint_less_than(first_score, second_score) {
        first_point
    } else {
        second_point
    }
}

fn trace(
    agent: &AgentInput,
    obstacles: &[VelocityObstacle],
    mut point: FixedVec2,
    desired_velocity: FixedVec2,
) -> (FixedVec2, i64) {
    let step_size = agent
        .radius_outer
        .max(q32_mul(TRACE_SPEED_WEIGHT, agent.desired_speed));
    // Native Trace establishes its incumbent from the first evaluated point.
    // Starting from the raw integer maximum and routing that sentinel through
    // FPoint.op_LessThan is not equivalent: a negative first score wraps the
    // internal subtraction and can be rejected even though it is the only
    // minimum (observed for a build-2227 Fang avoidance solve).
    let mut best_score = 0;
    let mut best_point = point;
    for step_index in 0..TRACE_ITERATIONS {
        // Native `Agent.Trace` divides the Q32.32 iteration counter by 50 and
        // then subtracts that rounded quotient from one. Do not rewrite this
        // as `(50 - i) / 50`: native fixed-point rounding can make the two
        // schedules differ by raw units.
        let elapsed = q32_div(
            step_index.saturating_mul(Q32_ONE),
            TRACE_ITERATIONS.saturating_mul(Q32_ONE),
        );
        let remaining = Q32_ONE.saturating_sub(elapsed);
        let step = q32_mul(q32_mul(remaining, remaining), step_size);
        let (gradient, score) = evaluate_gradient(agent, obstacles, point, desired_velocity);
        if trace_score_replaces_incumbent(step_index, score, best_score) {
            best_score = score;
            best_point = point;
        }
        point = point.add(gradient.normalized().mul(step));
    }
    (best_point, best_score)
}

fn trace_score_replaces_incumbent(step_index: i64, score: i64, incumbent: i64) -> bool {
    step_index == 0 || fpoint_less_than(score, incumbent)
}

/// Build-2227 `FPoint.op_LessThan` is not a raw signed comparison. It treats
/// differences of at most 43 raw Q32.32 units as equal and rejects the fixed
/// sentinel. This matters in the late Trace iterations: a numerically lower
/// score only replaces the incumbent when it is lower by at least 44 raw
/// units.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "the 32-bit truncation and sign reinterpretation reproduce build-2227 Q32.32 arithmetic"
)]
pub(crate) fn fpoint_less_than(left: i64, right: i64) -> bool {
    const SENTINEL: i64 = i64::MIN + 1;
    if left == SENTINEL || right == SENTINEL {
        return false;
    }
    let difference = left.wrapping_sub(right);
    difference < 0 && (difference.wrapping_add(43) as u64) >= 87
}

/// Native `FPoint.Min` and `FPoint.Max` delegate to the tolerant comparison
/// above and return their second argument when the operands compare equal.
/// Preserve that argument-order tie behaviour: quadtree bounds and branch
/// radii feed their result back into later center and reach calculations.
fn fpoint_min(first: i64, second: i64) -> i64 {
    if fpoint_less_than(first, second) {
        first
    } else {
        second
    }
}

fn fpoint_max(first: i64, second: i64) -> i64 {
    if fpoint_less_than(second, first) {
        first
    } else {
        second
    }
}

fn evaluate_gradient(
    agent: &AgentInput,
    obstacles: &[VelocityObstacle],
    point: FixedVec2,
    desired_velocity: FixedVec2,
) -> (FixedVec2, i64) {
    let mut gradient = FixedVec2::ZERO;
    let mut value = 0;
    for obstacle in obstacles {
        let (candidate, weight) = obstacle.scaled_gradient(point);
        if weight > value {
            value = weight;
            gradient = candidate;
        }
    }
    // `RVOAgentFixed.CalculateVelocity` passes `sync_desiredVelocity` (+0x1E0)
    // by reference to `BiasDesiredVelocity`, and its `EvaluateGradient`
    // override reads that same field. Both traces therefore evaluate against
    // the biased synchronized velocity.
    let to_desired = desired_velocity.sub(point);
    let desired_distance = to_desired.magnitude();
    if fpoint_less_than(NORMALIZE_EPSILON, desired_distance) {
        gradient = gradient.add(to_desired.mul(q32_div(DESIRED_VELOCITY_WEIGHT, desired_distance)));
        value = value.saturating_add(q32_mul(desired_distance, DESIRED_VELOCITY_WEIGHT));
    }
    let speed_sq = point.sqr_magnitude();
    let desired_speed_sq = q32_mul(agent.desired_speed, agent.desired_speed);
    if fpoint_less_than(desired_speed_sq, speed_sq) {
        // `FPoint.Sqrt` delegates to `FPCSMath.SqrtFastest` in build 2227.
        let speed = fpcs_sqrt_fastest(speed_sq);
        if speed > agent.max_speed {
            value = value.saturating_add(q32_mul(
                MAX_SPEED_WEIGHT,
                speed.saturating_sub(agent.max_speed),
            ));
            // Native `RVOAgentFixed.EvaluateGradient` deliberately divides
            // the vector first and only then applies the scalar weight.  The
            // two Q32.32 truncations make this observably different from
            // `point * (weight / speed)`.
            gradient = gradient.sub(point.div(speed).mul(MAX_SPEED_WEIGHT));
        }
        value = value.saturating_add(q32_mul(
            GRADIENT_SPEED_WEIGHT,
            speed.saturating_sub(agent.desired_speed),
        ));
        gradient = gradient.sub(point.div(speed).mul(GRADIENT_SPEED_WEIGHT));
    }
    (gradient, value)
}

fn lerp(left: FixedVec2, right: FixedVec2, amount: i64) -> FixedVec2 {
    // `FVector2.Lerp(in start, in end, ...)` multiplies the two endpoints
    // independently before adding them.  Keep that operation order: Q32.32
    // truncation makes it differ from `start + (end - start) * t`.
    let amount = amount.clamp(0, Q32_ONE);
    left.mul(Q32_ONE.saturating_sub(amount))
        .add(right.mul(amount))
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "the 32-bit truncation and sign reinterpretation reproduce build-2227 Q32.32 arithmetic"
)]
fn q32_mul(left: i64, right: i64) -> i64 {
    let low = ((u64::from(left as u32) * u64::from(right as u32)) >> 32) as i64;
    (left >> 32)
        .wrapping_mul(right)
        .wrapping_add((right >> 32).wrapping_mul(i64::from(left as u32)))
        .wrapping_add(low)
}

fn q32_div(numerator: i64, denominator: i64) -> i64 {
    if denominator == 0 {
        return if numerator < 0 { i64::MIN } else { i64::MAX };
    }
    // Build 2227 `FPoint.RawDiv` computes one extra quotient bit and ends in
    // `(quotient + 1) >> 1`, so finite values round to nearest magnitude
    // rather than truncating toward zero. Restore the sign only after that
    // rounding step, matching the native routine.
    let scaled = u128::from(numerator.unsigned_abs()) << 32;
    let divisor = u128::from(denominator.unsigned_abs());
    let quotient = scaled / divisor;
    let remainder = scaled % divisor;
    let rounded = quotient.saturating_add(u128::from(remainder.saturating_mul(2) >= divisor));
    let negative = (numerator < 0) != (denominator < 0);
    if negative {
        i64::try_from(rounded)
            .ok()
            .and_then(i64::checked_neg)
            .unwrap_or(i64::MIN)
    } else {
        i64::try_from(rounded).unwrap_or(i64::MAX)
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "the 32-bit truncation and sign reinterpretation reproduce build-2227 Q32.32 arithmetic"
)]
fn q32_exp(value: i64) -> i64 {
    let low = u64::from(value as u32);
    let high = value >> 32;
    // `FPoint.Exp` uses the inlined build-2227 approximation constant ending
    // in `...7652`, not the adjacent private `RAW_RCP_LN2` declaration.
    let correction = ((u128::from(low) * u128::from(0x7154_7652_u64)) >> 32) as i64;
    let exp2_input = high
        .wrapping_mul(0x1_7154_7652)
        .wrapping_add(low as i64)
        .wrapping_add(correction);
    q32_exp2_fastest(exp2_input)
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "the 32-bit truncation and sign reinterpretation reproduce build-2227 Q32.32 arithmetic"
)]
fn q32_exp2_fastest(value: i64) -> i64 {
    const LIMIT: i64 = 0x1f_ffff_ffff;
    if value > LIMIT {
        return i64::MAX - 1;
    }
    if value < -LIMIT {
        return 0;
    }
    let fraction = ((value as u64) >> 2) & 0x3fff_ffff;
    // FPoint.Exp tail-calls FPCSMath.Exp2Fastest in build 2227. Preserve its
    // two-stage polynomial and the intermediate wrapping operation exactly.
    let first = 0x0E7B_D338_0000_0000_u64.wrapping_add(fraction.wrapping_mul(0x1409_5EA4)) >> 32;
    let second =
        0x2C81_D51F_0000_0000_u64.wrapping_add(first.wrapping_mul(fraction).wrapping_mul(4)) >> 32;
    let polynomial = second.wrapping_mul(fraction) >> 28;
    let mantissa = (Q32_ONE as u64).wrapping_add(polynomial) & 0x3_ffff_fffc;
    let exponent = value >> 32;
    if exponent >= 0 {
        mantissa.checked_shl(exponent as u32).unwrap_or(u64::MAX) as i64
    } else {
        (mantissa >> exponent.unsigned_abs().min(63)) as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(
        clippy::too_many_arguments,
        reason = "the builder mirrors every AgentInput field the tests vary"
    )]
    fn observed_agent(
        key: AgentKey,
        position: FixedVec2,
        radius_inner: i64,
        radius_outer: i64,
        max_speed: i64,
        layer: u32,
        collides_with: u32,
        group: i32,
        locked: bool,
        size: AgentSizeType,
    ) -> AgentInput {
        AgentInput {
            key,
            main_layer: 1,
            layer,
            collides_with,
            group,
            passable_by_own_group: false,
            locked,
            tree_position: position,
            position,
            current_velocity: FixedVec2::ZERO,
            desired_velocity: FixedVec2::ZERO,
            desired_target_delta: FixedVec2::ZERO,
            desired_speed: max_speed,
            max_speed,
            published_calculated_speed: max_speed,
            radius_outer,
            radius_inner,
            size,
            priority: Q32_ONE,
        }
    }

    #[test]
    fn quadtree_builds_from_the_previous_buffer_but_queries_current_positions() {
        let mut source = observed_agent(
            AgentKey::Unit(1),
            FixedVec2 {
                x: 100 * Q32_ONE,
                y: 0,
            },
            Q32_ONE,
            Q32_ONE,
            0,
            1,
            1,
            0,
            false,
            AgentSizeType::S,
        );
        source.tree_position = FixedVec2::ZERO;
        let mut candidate = observed_agent(
            AgentKey::Unit(2),
            FixedVec2 {
                x: 101 * Q32_ONE,
                y: 0,
            },
            Q32_ONE,
            Q32_ONE,
            0,
            1,
            1,
            1,
            false,
            AgentSizeType::S,
        );
        candidate.tree_position = FixedVec2 {
            x: 200 * Q32_ONE,
            y: 0,
        };
        let inputs = [source, candidate];

        let tree = NativeQuadtree::build(&inputs).unwrap();
        assert_eq!(tree.bounds.min, source.tree_position);
        assert_eq!(tree.bounds.max, candidate.tree_position);
        assert_eq!(
            tree.query(&source)
                .into_iter()
                .map(|agent| agent.key)
                .collect::<Vec<_>>(),
            [candidate.key]
        );
    }

    #[test]
    fn build_2259_same_group_pair_matches_native_v7_solution() {
        let arclight = AgentInput {
            key: AgentKey::Unit(2),
            main_layer: 1,
            layer: 1_024,
            collides_with: 2_147_482_624,
            group: 1,
            passable_by_own_group: false,
            locked: false,
            tree_position: FixedVec2 {
                x: 670_862_199_496,
                y: 1_877_415_695_252,
            },
            position: FixedVec2 {
                x: 670_862_199_496,
                y: 1_877_415_695_252,
            },
            current_velocity: FixedVec2 {
                x: -7_307_757_926,
                y: -29_164_898_147,
            },
            desired_velocity: FixedVec2 {
                x: -7_307_989_276,
                y: -29_165_821_440,
            },
            desired_target_delta: FixedVec2 {
                x: -141_818_089_969,
                y: -565_988_938_214,
            },
            desired_speed: 30_064_771_072,
            max_speed: 30_064_771_072,
            published_calculated_speed: 30_064_771_072,
            radius_outer: 34_359_738_368,
            radius_inner: 17_179_869_184,
            size: AgentSizeType::L,
            priority: 2_147_483_648,
        };
        let rhino = AgentInput {
            key: AgentKey::Unit(3),
            main_layer: 1,
            layer: 16_384,
            collides_with: 2_147_467_264,
            group: 1,
            passable_by_own_group: false,
            locked: false,
            tree_position: FixedVec2 {
                x: 913_100_625_018,
                y: 2_174_047_205_703,
            },
            position: FixedVec2 {
                x: 913_100_625_018,
                y: 2_174_047_205_703,
            },
            current_velocity: FixedVec2 {
                x: -27_792_516_144,
                y: -62_842_303_808,
            },
            desired_velocity: FixedVec2 {
                x: -27_947_699_456,
                y: -62_772_680_752,
            },
            desired_target_delta: FixedVec2 {
                x: -384_056_515_491,
                y: -862_620_448_665,
            },
            desired_speed: 68_719_476_736,
            max_speed: 68_719_476_736,
            published_calculated_speed: 68_719_476_736,
            radius_outer: 42_949_672_960,
            radius_inner: 25_769_803_776,
            size: AgentSizeType::L,
            priority: Q32_ONE,
        };

        let solutions = solve_agents(&[arclight, rhino], Q32_ONE * 5);
        assert_eq!(
            solutions[&AgentKey::Unit(2)],
            AgentSolution {
                target_delta: FixedVec2 {
                    x: -141_818_089_969,
                    y: -565_988_938_214,
                },
                speed: 30_064_771_072,
            }
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn build_2259_same_group_vo_matches_native_v8_construction() {
        let arclight = AgentInput {
            key: AgentKey::Unit(2),
            main_layer: 1,
            layer: 1_024,
            collides_with: 2_147_482_624,
            group: 1,
            passable_by_own_group: false,
            locked: false,
            tree_position: FixedVec2 {
                x: 673_358_686_856,
                y: 1_880_735_461_297,
            },
            position: FixedVec2 {
                x: 673_358_686_856,
                y: 1_880_735_461_297,
            },
            current_velocity: FixedVec2 {
                x: -7_448_199_731,
                y: -29_127_781_088,
            },
            desired_velocity: FixedVec2 {
                x: -7_448_538_909,
                y: -29_129_107_504,
            },
            desired_target_delta: FixedVec2 {
                x: -148_364_322_183,
                y: -580_210_474_218,
            },
            desired_speed: 30_064_771_072,
            max_speed: 30_064_771_072,
            published_calculated_speed: 30_064_771_072,
            radius_outer: 34_359_738_368,
            radius_inner: 17_179_869_184,
            size: AgentSizeType::L,
            priority: 2_147_483_648,
        };
        let rhino = AgentInput {
            key: AgentKey::Unit(3),
            main_layer: 1,
            layer: 16_384,
            collides_with: 2_147_467_264,
            group: 1,
            passable_by_own_group: false,
            locked: false,
            tree_position: FixedVec2 {
                x: 917_439_655_179,
                y: 2_181_187_784_687,
            },
            position: FixedVec2 {
                x: 917_439_655_179,
                y: 2_181_187_784_687,
            },
            current_velocity: FixedVec2 {
                x: -27_821_608_160,
                y: -62_831_627_408,
            },
            desired_velocity: FixedVec2 {
                x: -27_969_365_408,
                y: -62_764_365_344,
            },
            desired_target_delta: FixedVec2 {
                x: -392_445_290_506,
                y: -880_662_797_608,
            },
            desired_speed: 68_719_476_736,
            max_speed: 68_719_476_736,
            published_calculated_speed: 68_719_476_736,
            radius_outer: 42_949_672_960,
            radius_inner: 25_769_803_776,
            size: AgentSizeType::L,
            priority: Q32_ONE,
        };

        let obstacle = neighbour_obstacle(&arclight, &rhino, Q32_ONE * 5);
        assert_eq!(
            (
                obstacle.line1,
                obstacle.line2,
                obstacle.dir1,
                obstacle.dir2,
                obstacle.cutoff_line,
                obstacle.cutoff_dir,
                obstacle.circle_center,
                obstacle.colliding,
                obstacle.radius,
                obstacle.weight_factor,
            ),
            (
                FixedVec2 {
                    x: 3_365_848_657,
                    y: -31_524_083_401,
                },
                FixedVec2 {
                    x: -6_433_823_876,
                    y: -23_561_220_050,
                },
                FixedVec2 {
                    x: -3_319_808_100,
                    y: -2_726_049_968,
                },
                FixedVec2 {
                    x: 1_988_767_472,
                    y: 3_807_065_060,
                },
                FixedVec2 {
                    x: -4_782_976_738,
                    y: -31_541_735_291,
                },
                FixedVec2 {
                    x: -3_333_876_454,
                    y: 2_708_369_114,
                },
                FixedVec2 {
                    x: -723_226_291,
                    y: -26_544_371_255,
                },
                false,
                6_442_450_938,
                Q32_ONE,
            )
        );

        assert_eq!(obstacle.gradient(arclight.desired_velocity).1, -763_110_658);
        assert_eq!(
            solve_agent(&arclight, &[obstacle]),
            AgentSolution {
                target_delta: FixedVec2 {
                    x: -148_364_322_183,
                    y: -580_210_474_218,
                },
                speed: 30_064_771_072,
            }
        );
    }

    /// A construction is a neighbour to both sides and an obstacle only to the
    /// other: a Defensive Wall sinks for its own side, which still counts it
    /// among a unit's twenty neighbours and builds no velocity obstacle from it.
    #[test]
    fn a_construction_is_an_obstacle_to_the_other_group_only() {
        let wall = |group| AgentInput {
            passable_by_own_group: true,
            ..observed_agent(
                AgentKey::Building(3),
                FixedVec2 {
                    x: 5 * Q32_ONE,
                    y: 0,
                },
                4 * Q32_ONE,
                4 * Q32_ONE,
                0,
                1 << 9,
                0,
                group,
                true,
                AgentSizeType::M,
            )
        };
        let unit = |group| AgentInput {
            desired_velocity: FixedVec2 {
                x: 4 * Q32_ONE,
                y: 0,
            },
            desired_target_delta: FixedVec2 {
                x: 20 * Q32_ONE,
                y: 0,
            },
            ..observed_agent(
                AgentKey::Unit(1),
                FixedVec2::ZERO,
                3 * Q32_ONE,
                6 * Q32_ONE,
                4 * Q32_ONE,
                1 << 8,
                0x7fff_ff00,
                group,
                false,
                AgentSizeType::M,
            )
        };
        let found = |agent: &AgentInput, candidate: AgentInput| {
            let mut neighbours = Vec::new();
            insert_neighbour(agent, &[candidate], 0, i64::MAX, &mut neighbours);
            neighbours.len()
        };
        assert_eq!(found(&unit(1), wall(0)), 1, "the other side counts it");
        assert_eq!(found(&unit(0), wall(0)), 1, "and so does its own");

        let solved = |inputs: &[AgentInput]| solve_agents(inputs, Q32_ONE * 5)[&AgentKey::Unit(1)];
        let alone = solved(&[unit(0)]);
        assert_eq!(
            solved(&[unit(0), wall(0)]),
            alone,
            "its own side walks through it"
        );
        assert_ne!(
            solved(&[unit(1), wall(0)]),
            alone,
            "the other side avoids it"
        );
    }
}
