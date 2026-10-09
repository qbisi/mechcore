use super::attacker::ScoreOffsets;
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) struct TargetActorRect {
    pub(in crate::fight) min_x: i64,
    pub(in crate::fight) min_z: i64,
    pub(in crate::fight) max_x: i64,
    pub(in crate::fight) max_z: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::fight) struct FightQuadtreeNode<E> {
    pub(in crate::fight) rect: TargetActorRect,
    pub(in crate::fight) depth: u8,
    pub(in crate::fight) elements: Vec<E>,
    pub(in crate::fight) children: Option<Box<[FightQuadtreeNode<E>; 4]>>,
}

/// A `FightQuadtree`: a side's units for the target trees, or a terrain
/// controller's items, each with the range it was put in by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::fight) struct FightQuadtree<E: Ord> {
    pub(in crate::fight) root: FightQuadtreeNode<E>,
    pub(in crate::fight) ranges: BTreeMap<E, TargetActorRect>,
    /// `MaxDepth`: a node this deep never splits.
    max_depth: u8,
}

/// The trees units are found in.
pub(in crate::fight) type TargetActorQuadtree = FightQuadtree<FightActorRef>;

/// `RectRange.Overlaps` of two rects: on each axis, twice the distance
/// between their centres is less than the sum of their widths.
fn overlaps(one: TargetActorRect, other: TargetActorRect) -> bool {
    let axis = |min: i64, max: i64, other_min: i64, other_max: i64| {
        let twice_delta =
            (i128::from(min) + i128::from(max) - i128::from(other_min) - i128::from(other_max))
                .abs();
        twice_delta
            < i128::from(max) - i128::from(min) + i128::from(other_max) - i128::from(other_min)
    };
    axis(one.min_x, one.max_x, other.min_x, other.max_x)
        && axis(one.min_z, one.max_z, other.min_z, other.max_z)
}

impl TargetActorRect {
    /// The battlefield, which every fight quadtree's root covers.
    pub(in crate::fight) const fn map() -> Self {
        Self {
            min_x: -TARGET_QUADTREE_HALF_WIDTH_Q32,
            min_z: -TARGET_QUADTREE_HALF_HEIGHT_Q32,
            max_x: TARGET_QUADTREE_HALF_WIDTH_Q32,
            max_z: TARGET_QUADTREE_HALF_HEIGHT_Q32,
        }
    }

    pub(in crate::fight) fn around(x_q32: i64, z_q32: i64, radius: i64) -> Self {
        let radius_q32 = space_to_q32(radius.max(0));
        Self {
            min_x: x_q32.saturating_sub(radius_q32),
            min_z: z_q32.saturating_sub(radius_q32),
            max_x: x_q32.saturating_add(radius_q32),
            max_z: z_q32.saturating_add(radius_q32),
        }
    }

    pub(in crate::fight) fn contains(self, other: Self) -> bool {
        self.min_x <= other.min_x
            && self.min_z <= other.min_z
            && self.max_x >= other.max_x
            && self.max_z >= other.max_z
    }

    pub(in crate::fight) fn children(self) -> [Self; 4] {
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

impl<E: Copy + Ord> FightQuadtreeNode<E> {
    pub(in crate::fight) fn new(rect: TargetActorRect, depth: u8) -> Self {
        Self {
            rect,
            depth,
            elements: Vec::new(),
            children: None,
        }
    }

    pub(in crate::fight) fn child_containing(&self, range: TargetActorRect) -> Option<usize> {
        self.children
            .as_ref()?
            .iter()
            .position(|child| child.rect.contains(range))
    }

    pub(in crate::fight) fn insert(
        &mut self,
        candidate: E,
        ranges: &BTreeMap<E, TargetActorRect>,
        max_depth: u8,
    ) {
        let range = ranges[&candidate];
        if let Some(child_index) = self.child_containing(range) {
            self.children.as_mut().expect("observed child exists")[child_index]
                .insert(candidate, ranges, max_depth);
            return;
        }

        if self.children.is_none()
            && self.depth < max_depth
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
                        .insert(old, ranges, max_depth);
                }
            }
            if let Some(child_index) = self.child_containing(range) {
                self.children.as_mut().expect("split children exist")[child_index]
                    .insert(candidate, ranges, max_depth);
                return;
            }
        }
        self.elements.push(candidate);
    }

    pub(in crate::fight) fn find_path(&self, candidate: E, path: &mut Vec<usize>) -> bool {
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

    pub(in crate::fight) fn node_at_path(&self, path: &[usize]) -> &Self {
        let mut node = self;
        for &index in path {
            node = &node.children.as_ref().expect("quadtree child exists")[index];
        }
        node
    }

    pub(in crate::fight) fn node_at_path_mut(&mut self, path: &[usize]) -> &mut Self {
        let mut node = self;
        for &index in path {
            node = &mut node.children.as_mut().expect("quadtree child exists")[index];
        }
        node
    }

    /// `FightQuadtreeNode.Query`: a node the range overlaps answers its
    /// elements, then each child's, children in order.
    fn query(&self, overlaps: &impl Fn(TargetActorRect) -> bool, found: &mut Vec<E>) -> bool {
        if !overlaps(self.rect) {
            return false;
        }
        found.extend(self.elements.iter().copied());
        for child in self.children.iter().flat_map(|children| children.iter()) {
            child.query(overlaps, found);
        }
        true
    }

    pub(in crate::fight) fn append_query_order(&self, output: &mut Vec<E>) {
        output.extend(self.elements.iter().copied());
        if let Some(children) = &self.children {
            for child in children {
                child.append_query_order(output);
            }
        }
    }
}

impl<E: Copy + Ord> FightQuadtree<E> {
    /// A side's units' tree: `MaxDepth` 6.
    pub(in crate::fight) fn new() -> Self {
        Self::with_max_depth(TARGET_QUADTREE_MAX_DEPTH)
    }

    /// An empty tree over the battlefield, splitting no node `max_depth`
    /// deep.
    pub(in crate::fight) fn with_max_depth(max_depth: u8) -> Self {
        Self {
            root: FightQuadtreeNode::new(TargetActorRect::map(), 0),
            ranges: BTreeMap::new(),
            max_depth,
        }
    }

    pub(in crate::fight) fn insert(&mut self, candidate: E, x_q32: i64, z_q32: i64, radius: i64) {
        self.insert_range(candidate, TargetActorRect::around(x_q32, z_q32, radius));
    }

    /// `FightQuadtree.Insert` of an element by its bounds: into the deepest
    /// node holding them, the root when they leave the battlefield.
    pub(in crate::fight) fn insert_range(&mut self, candidate: E, range: TargetActorRect) {
        self.ranges.insert(candidate, range);
        if self.root.rect.contains(range) {
            self.root.insert(candidate, &self.ranges, self.max_depth);
        } else {
            self.root.elements.push(candidate);
        }
    }

    /// Takes an element out, keeping the order of the rest.
    pub(in crate::fight) fn remove(&mut self, candidate: E) {
        let mut path = Vec::new();
        if !self.root.find_path(candidate, &mut path) {
            return;
        }
        let node = self.root.node_at_path_mut(&path);
        if let Some(index) = node
            .elements
            .iter()
            .position(|element| *element == candidate)
        {
            node.elements.remove(index);
        }
        self.ranges.remove(&candidate);
    }

    pub(in crate::fight) fn position_changed(
        &mut self,
        candidate: E,
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
            node.children.as_mut().expect("located child exists")[child_index].insert(
                candidate,
                &self.ranges,
                self.max_depth,
            );
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
            .insert(candidate, &self.ranges, self.max_depth);
    }

    /// `FightQuadtree.Query` for a square of full width `size` around a
    /// point, all Q32.32: the root's elements always, then every element of
    /// each node whose rect overlaps the square, strictly on both axes, with
    /// no test of the elements themselves; a node's before its children's,
    /// the children in order.
    pub(in crate::fight) fn query_square(
        &self,
        center_x_q32: i64,
        center_z_q32: i64,
        size_q32: i64,
    ) -> Vec<E> {
        let overlaps = |rect: TargetActorRect| {
            let axis = |min: i64, max: i64, center: i64| {
                let twice_delta =
                    (i128::from(min) + i128::from(max) - 2 * i128::from(center)).abs();
                twice_delta < i128::from(max) - i128::from(min) + i128::from(size_q32)
            };
            axis(rect.min_x, rect.max_x, center_x_q32) && axis(rect.min_z, rect.max_z, center_z_q32)
        };
        let mut found = Vec::new();
        if !self.root.query(&overlaps, &mut found) {
            found.extend(self.root.elements.iter().copied());
        }
        found
    }

    /// `FightQuadtree.Query` for a rect: the root's elements always, then,
    /// as [`TargetActorQuadtree::query_square`] walks them, every element of
    /// each node whose rect overlaps it strictly on both axes.
    pub(in crate::fight) fn query_rect(&self, range: TargetActorRect) -> Vec<E> {
        let overlaps = |rect: TargetActorRect| overlaps(rect, range);
        let mut found = Vec::new();
        if !self.root.query(&overlaps, &mut found) {
            found.extend(self.root.elements.iter().copied());
        }
        found
    }

    /// `FightQuadtree.Query` for a circle (`RangeItemController.GetItems`):
    /// the root's elements always, then every element of each node the
    /// circle overlaps, a node's before its children's, the children in
    /// order. A circle overlaps a rect (`FightRange.Overlaps`) when the point
    /// of the rect nearest its centre, each coordinate clamped, lies strictly
    /// within its radius, the squares `FPoint` products.
    pub(in crate::fight) fn query_circle(&self, x_q32: i64, z_q32: i64, radius_q32: i64) -> Vec<E> {
        let overlaps = |rect: TargetActorRect| {
            let dx = x_q32.min(rect.max_x).max(rect.min_x) - x_q32;
            let dz = z_q32.min(rect.max_z).max(rect.min_z) - z_q32;
            q32_mul(dz, dz).saturating_add(q32_mul(dx, dx)) < q32_mul(radius_q32, radius_q32)
        };
        let mut found = Vec::new();
        if !self.root.query(&overlaps, &mut found) {
            found.extend(self.root.elements.iter().copied());
        }
        found
    }

    /// `FightQuadtree.IsInteractableRange`: whether a node holding elements
    /// overlaps the rect, strictly on both axes (`RectRange.Overlaps`).
    pub(in crate::fight) fn is_interactable(&self, range: TargetActorRect) -> bool {
        fn walk<E>(node: &FightQuadtreeNode<E>, range: TargetActorRect) -> bool {
            (!node.elements.is_empty() && overlaps(node.rect, range))
                || node
                    .children
                    .iter()
                    .flat_map(|children| children.iter())
                    .any(|child| walk(child, range))
        }
        walk(&self.root, range)
    }

    /// `FightQuadtree.GetInteractableNodes` against another tree: every node
    /// holding elements whose rect the other tree is interactable with
    /// (`IsInteractableRange`), a node before its children, the children in
    /// order, each child visited whether its parent was taken or not.
    pub(in crate::fight) fn interactable_nodes<F: Copy + Ord>(
        &self,
        target: &FightQuadtree<F>,
    ) -> Vec<&FightQuadtreeNode<E>> {
        fn walk<'a, E, F: Copy + Ord>(
            node: &'a FightQuadtreeNode<E>,
            target: &FightQuadtree<F>,
            found: &mut Vec<&'a FightQuadtreeNode<E>>,
        ) {
            if !node.elements.is_empty() && target.is_interactable(node.rect) {
                found.push(node);
            }
            for child in node.children.iter().flat_map(|children| children.iter()) {
                walk(child, target, found);
            }
        }
        let mut found = Vec::new();
        walk(&self.root, target, &mut found);
        found
    }

    pub(in crate::fight) fn query_order(&self) -> Vec<E> {
        let mut output = Vec::with_capacity(self.ranges.len());
        self.root.append_query_order(&mut output);
        output
    }
}

/// The trees a unit looks for a target in.
///
/// A building nobody searches for is left out of them rather than scored and
/// rejected: a Defensive Wall answers `IsEnableSearchTarget` with false, and
/// the game's Crawlers lock onto the unit behind one at tick one.
/// Each side's `FightTeam.mechQuadtree`: its units alone, in the order the
/// target trees take them.
pub(in crate::fight) fn initialize_mech_quadtrees(
    actors: &BTreeMap<u64, Actor>,
) -> BTreeMap<u32, TargetActorQuadtree> {
    let mut trees = BTreeMap::<u32, TargetActorQuadtree>::new();
    for (&actor_id, actor) in actors {
        trees
            .entry(actor.placement.team)
            .or_insert_with(TargetActorQuadtree::new)
            .insert(
                FightActorRef::Unit(actor_id),
                actor.x_q32,
                actor.z_q32,
                actor.rules.collision_radius(),
            );
    }
    trees
}

pub(in crate::fight) fn initialize_target_quadtrees(
    actors: &BTreeMap<u64, Actor>,
    buildings: &[BuildingState],
    constructions: &BTreeMap<u64, i32>,
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
        let (team_constructions, towers): (Vec<_>, Vec<_>) = team_buildings
            .into_iter()
            .partition(|building| constructions.contains_key(&building.building_id));
        let insert_building = |tree: &mut TargetActorQuadtree, building: &BuildingState| {
            tree.insert(
                FightActorRef::Building(building.building_id),
                building.position.x,
                building.position.z,
                building_radius(building),
            );
        };
        // `FightTeam.CreateQuadtree` takes `activeActors` in their order: the
        // towers, then the units, then the constructions. A construction no
        // unit searches for is still in the tree: the selector passes it
        // over, and a splash takes it where it stands in the tree's order —
        // an Arclight's shot at a block of a wall reads its damage on the
        // block between the Crawlers around it.
        for building in towers {
            insert_building(&mut tree, building);
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
        for building in team_constructions {
            insert_building(&mut tree, building);
        }
        trees.insert(team, tree);
    }
    trees
}

/// `ScoreRatingTargetSelector.invisibleActorDistanceScoreOffset`, negated,
/// millimetres: a candidate that is not visible is scored as if it stood 40
/// metres further off, by `DistanceScoreCalculator.Calculate`. It is offered
/// all the same: a Rhino searching while the Sandworm it locked is burrowed
/// keeps it.
pub(in crate::fight) const INVISIBLE_DISTANCE_SCORE_OFFSET: i64 = 40_000;

/// What a selector's `IDistanceCalculator` measures from a source to a
/// candidate: centre to centre, less both radii, never below zero.
pub(in crate::fight) fn target_edge_distance_q32(
    source_x_q32: i64,
    source_z_q32: i64,
    source_radius: i64,
    target_x_q32: i64,
    target_z_q32: i64,
    target_radius: i64,
) -> i64 {
    native_q32_magnitude(
        target_x_q32.saturating_sub(source_x_q32),
        target_z_q32.saturating_sub(source_z_q32),
    )
    .saturating_sub(space_to_q32(source_radius))
    .saturating_sub(space_to_q32(target_radius))
    .max(0)
}

#[allow(clippy::too_many_arguments)]
pub(in crate::fight) fn full_rotation_target_score_q32(
    source_x_q32: i64,
    source_z_q32: i64,
    source_radius: i64,
    source_rotation_q32: i64,
    target_x_q32: i64,
    target_z_q32: i64,
    target_radius: i64,
    distance_score_offset: i64,
    min_range: i64,
    max_range: i64,
    rotation_window_q32: Option<(i64, i64)>,
) -> Option<i64> {
    let distance_q32 = target_edge_distance_q32(
        source_x_q32,
        source_z_q32,
        source_radius,
        target_x_q32,
        target_z_q32,
        target_radius,
    );
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
    // `CalculateScore` checks the side of the source the candidate is on,
    // `sourceRotation` less the angle on the left and plus it on the right,
    // which is the bearing itself.
    let outside_rotation_window = rotation_window_q32
        .and_then(|(left_q32, right_q32)| rotation_window(source_rotation_q32, left_q32, right_q32))
        .is_some_and(|(min_q32, max_q32)| !is_in_range_rotation(bearing_q32, min_q32, max_q32));
    full_rotation_score_from_distance_and_angle_q32(
        distance_q32,
        space_to_q32(distance_score_offset),
        angle_q32,
        space_to_q32(min_range),
        space_to_q32(max_range),
        outside_rotation_window,
    )
}

/// The window `CalculateScore` is handed for a source whose window is its
/// rotation widened by `half_width_q32` either side, if it checks it: it
/// checks a window only when it starts above `Angle0` and ends below
/// `Angle360`.
fn rotation_window(rotation_q32: i64, left_q32: i64, right_q32: i64) -> Option<(i64, i64)> {
    let less = rvo::fpoint_less_than;
    let full_rotation = 360_i64 << 32;
    let min_q32 = clamp_rotation(rotation_q32.saturating_sub(left_q32));
    let max_q32 = clamp_rotation(rotation_q32.saturating_add(right_q32));
    (less(0, min_q32) && less(max_q32, full_rotation)).then_some((min_q32, max_q32))
}

/// `FightUtility.ClampRotation`: one turn added below `Angle0`, one taken off
/// from `Angle360`, with `FPoint`'s tolerant comparisons.
fn clamp_rotation(rotation_q32: i64) -> i64 {
    let full_rotation = 360_i64 << 32;
    if rvo::fpoint_less_than(rotation_q32, 0) {
        rotation_q32.saturating_add(full_rotation)
    } else if !rvo::fpoint_less_than(rotation_q32, full_rotation) {
        rotation_q32.saturating_sub(full_rotation)
    } else {
        rotation_q32
    }
}

/// `FightUtility.IsInRangeRotation`: whether a rotation lies between two
/// others, clockwise from the first, the window wrapping through 0 when the
/// first is not below the second; every comparison is `FPoint`'s tolerant one.
pub(in crate::fight) fn is_in_range_rotation(
    rotation_q32: i64,
    min_q32: i64,
    max_q32: i64,
) -> bool {
    let less = rvo::fpoint_less_than;
    if less(min_q32, max_q32) {
        !less(rotation_q32, min_q32) && !less(max_q32, rotation_q32)
    } else {
        !(less(max_q32, rotation_q32) && less(rotation_q32, min_q32))
    }
}

pub(in crate::fight) fn full_rotation_score_from_distance_and_angle_q32(
    distance_q32: i64,
    distance_score_offset_q32: i64,
    angle_q32: i64,
    min_range_q32: i64,
    max_range_q32: i64,
    outside_rotation_window: bool,
) -> Option<i64> {
    if distance_q32 < min_range_q32 {
        return None;
    }
    // `DistanceScoreCalculator.Calculate`: the offset counted off.
    let angle_score_q32 = q32_mul(
        angle_q32.min(TARGET_SCORE_ANGLE_LIMIT_Q32),
        TARGET_SCORE_ANGLE_FACTOR_Q32,
    );
    let distance_score_q32 = distance_q32
        .saturating_sub(distance_score_offset_q32)
        .max(TARGET_SCORE_MIN_DISTANCE_Q32);
    let mut score_q32 = q32_mul(
        distance_score_q32,
        TARGET_SCORE_BASE_Q32.saturating_add(angle_score_q32),
    );
    // One penalty for a candidate out of range, or in range but outside the
    // rotation window.
    if distance_q32 > max_range_q32 || outside_rotation_window {
        score_q32 = score_q32.saturating_add(TARGET_SCORE_OUT_OF_RANGE_PENALTY_Q32);
    }
    Some(score_q32.saturating_add(distance_q32))
}

/// A skill's search looks at least this far around its searcher, in space
/// units, whatever its reach: the 400 of `max(range, 400)`. Recorded in
/// replay 134266831 round 3: a Crawler's first search scores the 13
/// candidates, 11 units and 2 towers, that a square 800 m wide around it
/// holds, and not the enemy straight ahead 538 m off.
const SEARCH_MIN_RADIUS: i64 = 400_000;

/// What `ScoreRatingTargetSelector.Selector.CheckResultTarget` keeps of the
/// candidates it scores: the lowest score, and the lowest-scored visible one
/// other than it (`nextTarget`), which a lower score that is visible hands
/// its place to.
#[derive(Debug, Default)]
pub(in crate::fight) struct Scoring {
    best: Option<(FightActorRef, i64, bool)>,
    next: Option<(FightActorRef, i64)>,
}

impl Scoring {
    pub(in crate::fight) fn consider(
        &mut self,
        candidate: FightActorRef,
        score: i64,
        visible: bool,
    ) {
        match self.best {
            Some((_, best_score, _)) if score >= best_score => {
                if visible && self.next.is_none_or(|(_, next_score)| score < next_score) {
                    self.next = Some((candidate, score));
                }
            }
            previous => {
                if let Some((previous, previous_score, true)) = previous {
                    self.next = Some((previous, previous_score));
                }
                self.best = Some((candidate, score, visible));
            }
        }
    }

    /// `ScoreRatingTargetSelector.Select` and `TrySelect`: the lowest score,
    /// unless it is not visible and the visible one after it is in the
    /// attacker's range (`IsActorInAttackRange`). A Marksman whose Sandworm
    /// burrows takes a visible unit as soon as one is in its reach, and the
    /// burrowed one while none is.
    pub(in crate::fight) fn chosen(
        self,
        reaches: impl Fn(FightActorRef) -> bool,
    ) -> Option<FightActorRef> {
        let (best, _, visible) = self.best?;
        match self.next {
            Some((next, _)) if !visible && reaches(next) => Some(next),
            _ => Some(best),
        }
    }
}

impl Simulation {
    /// `MainSkillSearchTargetController.PrepareSearch` reads its skill's
    /// ranges as it prepares, after the last tick's modules: a technology an
    /// Electromagnetic Impact switches off on this tick reaches the next
    /// tick's searches, not this one's.
    pub(in crate::fight) fn snapshot_search_ranges(&mut self) {
        let ranges = self
            .actors
            .keys()
            .map(|&id| (self.main_attack_range(id), self.main_min_range(id)))
            .collect::<Vec<_>>();
        for (actor, ranges) in self.actors.values_mut().zip(ranges) {
            actor.target_query_ranges = ranges;
        }
    }

    pub(in crate::fight) fn refresh_target_query_snapshot(&mut self) {
        // The build prepares selector inputs before FightCore updates actors
        // sequentially. Red actors must therefore score the tick-start pose,
        // not positions already advanced by blue actors in the same tick.
        // FightSkill::GetMainTransform returns its first valid owned weapon transform;
        // bodyless weapons without one fall back to the mech's root transform.
        self.snapshot_search_ranges();
        for actor in self.actors.values_mut() {
            actor.target_query_x_q32 = actor.x_q32;
            actor.target_query_z_q32 = actor.z_q32;
            actor.target_query_source_rotation_q32 =
                // A bodyless unit scores from its root, whether or not its
                // weapons turn at a speed of their own: a Wraith's weapons,
                // at 90° a second against its body's 120°, have no transform
                // for `GetMainTransform` to return, and its search reads the
                // root's facing, as a Vortex's does in `m6-formations`.
                if actor.rules.has_body {
                    actor
                        .skills.main
                        .weapon_rotations_q32
                        .first()
                        .copied()
                        .unwrap_or(actor.body_rotation_q32)
                } else {
                    actor.body_rotation_q32
                };
            actor.target_query_alive = actor.alive();
            actor.target_query_visibility = actor.visibility;
            actor.skills.main.searched_this_tick = false;
            // `MainSkillSearchTargetController.PrepareSearch` prepares no
            // skill a `SkillGroup` holds: a Wraith's or a Raiden's search is
            // a `Select` whenever it searches. A unit with one grouped skill,
            // the Vortex, and a batch of standalone weapons have none.
            let skill = &actor.skills.main;
            // Nor one whose selector is no `ScoreRatingTargetSelector`.
            actor.skills.main.search_prepared = actor.alive()
                && !actor.travelling
                && !actor.life_priority
                && (skill.group_size() <= 1 || skill.standalone());
        }
        self.buildings_query_alive = super::standing_buildings(&self.buildings);
    }

    /// Whether this skill's search is one `FightCoreSystem.PreCalculate`
    /// prepared on the tick's query snapshot. It prepares the skills of the
    /// mechs in the fight as the tick opens, alive and not travelling, and no
    /// construction's: a turret's skill searches with `Select` whenever it
    /// searches, scoring candidates where they stand by then, after every
    /// unit of the sides that update before its own has moved. Every one of
    /// 565 recorded searches of a construction took that path, and the
    /// Anti-Armor Turret of replay 268477093 round 2 locks a Hound that the
    /// tick-start positions put 0.33 metres out of its reach. Only a mech's
    /// main skill is prepared (`MainSkillSearchTargetController.PrepareSearch`),
    /// and only one no `SkillGroup` holds: an extra skill's `PrepareSearch`
    /// does nothing, nor does a Wraith's, and their search is a `Select`. A summon that joins after the preparation is not among the
    /// attackers `TrySelect` answers for, and its first search is a `Select`
    /// too, which finds the summons that joined with it.
    pub(in crate::fight) fn search_prepared(&self, skill_ref: SkillRef) -> bool {
        self.skill(skill_ref).search_prepared
    }

    /// Whether the skill's state prepared its search as the tick opened, on
    /// a skill [`Self::search_prepared`] lets prepare.
    /// `SkillIdleState.PreCalculate` prepares it when `CanStartSearchTarget`
    /// held then: its search timer due (`SearchTargetController.
    /// CanStartSearch`), or its lock absent or dead; `SkillAttackState.
    /// PreCalculate` only when its lock is absent or dead. A Crawler walking
    /// on a tower whose timer was due searches with `TrySelect` when the
    /// tower falls that tick; one whose timer was not, with `Select`.
    pub(in crate::fight) fn prepared_at_tick_start(&self, skill_ref: SkillRef) -> bool {
        let skill = self.skill(skill_ref);
        let lock_gone = skill
            .lock_target
            .and_then(|lock| self.fight_actor(lock))
            .is_none_or(|lock| !lock.query_alive);
        self.search_prepared(skill_ref)
            && match skill.phase() {
                FightSkillPhase::Idle => lock_gone || skill.search_target_time < 1,
                FightSkillPhase::Attack => lock_gone,
                FightSkillPhase::Prepare { .. } => false,
            }
    }

    pub(in crate::fight) fn target_search_order(&self) -> BTreeMap<u32, Vec<FightActorRef>> {
        self.target_quadtrees
            .iter()
            .map(|(&team, tree)| (team, tree.query_order()))
            .collect()
    }

    /// `RangeTargetCalculator.CalculateRangeActors` of units alone, about a
    /// point: every unit of either side, side by side in the order each
    /// side's objects are searched, that `FightCalculator.IsValidTarget`
    /// passes, alive, of a domain `domains` accepts and not underground
    /// (`ActorVisibility.Stealth`), and whose distance from the point, less
    /// its radius when `target_radius`, is within the range by
    /// `FPoint.op_LessThanOrEqual` (`IsInRange2D`).
    pub(in crate::fight) fn units_in_range(
        &self,
        (x_q32, z_q32): (i64, i64),
        range_q32: i64,
        (domains, target_radius): (crate::rules::AttackTargets, bool),
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Vec<u64> {
        target_search_order
            .values()
            .flatten()
            .filter_map(|&candidate| {
                let FightActorRef::Unit(id) = candidate else {
                    return None;
                };
                let actor = &self.actors[&id];
                if !actor.alive()
                    || actor.visibility == Visibility::Hide
                    || !domains.accepts(actor.domain)
                {
                    return None;
                }
                let radius = if target_radius {
                    space_to_q32(actor.rules.collision_radius())
                } else {
                    0
                };
                let distance = native_q32_magnitude(
                    actor.x_q32.saturating_sub(x_q32),
                    actor.z_q32.saturating_sub(z_q32),
                )
                .saturating_sub(radius);
                fpoint_less_or_equal(distance, range_q32).then_some(id)
            })
            .collect()
    }

    /// The selector, restricted to units. A test asks for one; the fight
    /// itself takes whatever stands nearest, buildings included.
    #[cfg(test)]
    pub(in crate::fight) fn select_normal_unit_target(&self, actor_id: u64) -> Result<Option<u64>> {
        let target_search_order = self.target_search_order();
        self.select_normal_unit_target_with_order(actor_id, &target_search_order, false)
    }

    #[cfg(test)]
    pub(in crate::fight) fn select_normal_unit_target_with_order(
        &self,
        actor_id: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        use_live_candidate_positions: bool,
    ) -> Result<Option<u64>> {
        match self.select_normal_target_with_order(
            SkillRef::main(FightActorRef::Unit(actor_id)),
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

    /// The candidates a skill's search scores: what the other sides' target
    /// trees answer for a square around the searcher as wide as twice its
    /// reach, and never narrower than 800 metres. A search
    /// `FightCoreSystem.PreCalculate` prepared asks the trees as they stood
    /// when it was prepared, about where the searcher stood then. A unit
    /// that updates before the searcher may have moved to another node by
    /// its turn: the tree prepared for a Crawler of replay 67260372's round
    /// 4 answers a Centurion 417 metres off for its square, and the tree by
    /// the Crawler's turn on tick 42 does not. One performed when it runs
    /// asks them as they stand then, about where the searcher stands.
    pub(in crate::fight) fn search_candidates(
        &self,
        owner: FightActorRef,
        prepared: bool,
        widening: i64,
    ) -> Option<BTreeSet<FightActorRef>> {
        let source = self.attacker(owner)?;
        let radius_q32 = space_to_q32(
            source
                .attack_range
                .max(SEARCH_MIN_RADIUS)
                .saturating_add(widening),
        );
        let (trees, x_q32, z_q32) = if prepared {
            (
                &self.prepared_target_quadtrees,
                source.query_x_q32,
                source.query_z_q32,
            )
        } else {
            (&self.target_quadtrees, source.x_q32, source.z_q32)
        };
        Some(
            trees
                .iter()
                .filter(|(team, _)| **team != source.team)
                .flat_map(|(_, tree)| tree.query_square(x_q32, z_q32, radius_q32.saturating_mul(2)))
                .collect(),
        )
    }

    /// `MainSkillSearchTargetController`: the other side's candidates around
    /// the owner ([`Simulation::search_candidates`]) in the order the target
    /// trees hold them, scored from where the owner stood and
    /// pointed at the tick's start, the lowest score taken. A unit's skill and
    /// a construction's ask the same selector.
    pub(in crate::fight) fn select_normal_target_with_order(
        &self,
        skill_ref: SkillRef,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        use_live_candidate_positions: bool,
    ) -> Result<Option<FightActorRef>> {
        // A side arm's `SideArmSearchTargetController` overrides
        // `PerformNormalSkillSearch`.
        if self.is_side_arm(skill_ref) {
            return Ok(self.select_side_arm_target(skill_ref, target_search_order));
        }
        let source = self
            .skill_attacker(skill_ref)
            .ok_or_else(|| Error::new("target selector source is absent"))?;
        Ok(self.perform_normal_skill_search(
            &source,
            target_search_order,
            !use_live_candidate_positions,
        ))
    }

    /// `SkillSearchTargetController.PerformNormalSkillSearch`.
    ///
    /// A search prepared at the tick's start is `TrySelect`'s: its winner,
    /// while it lives and is still of another side. A winner that has died
    /// since, or no winner, sends it to `Select` over every enemy the skill
    /// attacks (`OpponentController.GetActors`), not to the square: a Crawler
    /// of replay 201477923's round 5 whose tower fell that tick locks a Fang
    /// no square about it holds. A winner a beam has turned to the
    /// searcher's side sends it to `PerformSearch`, as does a search not
    /// prepared: `Select` over the square of `max(range, 400)`, then one 200
    /// metres wider, then one 300 wider again, each where everything stands
    /// by then, and `Select` over every enemy when all three answer nothing.
    fn perform_normal_skill_search(
        &self,
        source: &super::attacker::Attacker<'_>,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        prepared: bool,
    ) -> Option<FightActorRef> {
        if !source.searches {
            return None;
        }
        if prepared {
            // The prepared job scores with the ranges it was prepared with.
            let prepared_source = match source.owner {
                FightActorRef::Unit(id) => {
                    let (attack_range, min_range) = self.actors[&id].target_query_ranges;
                    super::attacker::Attacker {
                        min_range,
                        attack_range,
                        ..*source
                    }
                }
                FightActorRef::Building(_) => *source,
            };
            match self.select_in_square(&prepared_source, target_search_order, false, Some(0)) {
                Some(chosen)
                    if self
                        .fight_actor(chosen)
                        .is_some_and(|target| target.team == source.team) => {}
                Some(chosen) if self.fight_actor(chosen).is_some_and(|target| target.alive) => {
                    return Some(chosen);
                }
                _ => return self.select_in_square(source, target_search_order, true, None),
            }
        }
        [0, 200, 500]
            .into_iter()
            .find_map(|widening| {
                self.select_in_square(source, target_search_order, true, Some(widening))
            })
            .or_else(|| self.select_in_square(source, target_search_order, true, None))
    }

    /// [`Simulation::select_normal_target_with_order`] for a given source.
    pub(in crate::fight) fn select_normal_target_from(
        &self,
        source: &super::attacker::Attacker<'_>,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        use_live_candidate_positions: bool,
    ) -> Option<FightActorRef> {
        if !source.searches {
            return None;
        }
        self.select_in_square(
            source,
            target_search_order,
            use_live_candidate_positions,
            Some(0),
        )
    }

    /// `ScoreRatingTargetSelector.Select`, or `TrySelect` for a search
    /// prepared at the tick's start, over the candidates the target trees
    /// answer for the search's square widened by `widening` metres, or over
    /// every enemy for none.
    fn select_in_square(
        &self,
        source: &super::attacker::Attacker<'_>,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        use_live_candidate_positions: bool,
        widening: Option<i64>,
    ) -> Option<FightActorRef> {
        let owner = source.owner;
        let nearby = widening.and_then(|widening| {
            self.search_candidates(owner, !use_live_candidate_positions, widening * 1000)
        });
        let tower_attackable = self.tower_attackable(source.skill);
        let mut scoring = Scoring::default();
        // `Selector.Calculate`: the first candidate strictly nearer than any
        // before it, under the skill's minimum range or not.
        let mut nearest: Option<(FightActorRef, i64)> = None;

        let mut admitted = Vec::new();
        for (&team, candidates) in target_search_order {
            // The prepared trees hold each unit on the side it stood on as the
            // tick opened; a search made where everything stands now asks the
            // side it stands on, which a beam may have turned since.
            if team == source.team && !use_live_candidate_positions {
                continue;
            }
            for &candidate in candidates {
                let Some(target) = self.fight_actor(candidate) else {
                    continue;
                };
                // A unit turned since the tick opened has left the tree the
                // job's candidates came from; the job still holds it.
                let turned_since = !use_live_candidate_positions && target.team != team;
                if !turned_since
                    && nearby
                        .as_ref()
                        .is_some_and(|nearby| !nearby.contains(&candidate))
                {
                    continue;
                }
                if use_live_candidate_positions && target.team == source.team {
                    continue;
                }
                let candidate_alive = if use_live_candidate_positions {
                    target.alive
                } else {
                    target.query_alive
                };
                let candidate_targetable = if use_live_candidate_positions {
                    target.targetable
                } else {
                    target.query_targetable
                };
                // The prepared job scores each unit as the tick opened, on
                // the side it stood on then, a unit a beam has turned since
                // among them. `AttackTargetFilter` passes over a unit in
                // stealth, though not one underground.
                if !candidate_alive
                    || !candidate_targetable
                    || !target.searchable(!use_live_candidate_positions)
                    || matches!(candidate, FightActorRef::Building(id)
                        if self.unsearchable_buildings.contains(&id))
                    || (!tower_attackable && self.is_tower(candidate))
                    || !source.targets.accepts(target.domain)
                {
                    continue;
                }
                admitted.push((candidate, target));
            }
        }
        if source.life_priority {
            admitted = self.life_priority(source, admitted);
        }
        for (candidate, target) in admitted {
            let (candidate_x_q32, candidate_z_q32) = if use_live_candidate_positions {
                (target.x_q32, target.z_q32)
            } else {
                (target.query_x_q32, target.query_z_q32)
            };
            let visible = if use_live_candidate_positions {
                target.visible
            } else {
                target.query_visible
            };
            let distance_q32 = target_edge_distance_q32(
                source.query_x_q32,
                source.query_z_q32,
                source.radius,
                candidate_x_q32,
                candidate_z_q32,
                target.radius,
            );
            if nearest.is_none_or(|(_, least)| distance_q32 < least) {
                nearest = Some((candidate, distance_q32));
            }
            if let Some(score) = full_rotation_target_score_q32(
                source.query_x_q32,
                source.query_z_q32,
                source.radius,
                source.query_rotation_q32,
                candidate_x_q32,
                candidate_z_q32,
                target.radius,
                source.score_offsets.for_candidate(target.domain, visible),
                source.min_range,
                source.attack_range,
                source.rotation_window_q32,
            ) {
                scoring.consider(candidate, score, visible);
            }
        }

        self.skill(source.skill)
            .nearest_actor
            .set(nearest.map(|(candidate, _)| candidate));
        scoring.chosen(|next| self.target_in_attack_range(source.skill, next))
    }

    /// `LifePriorityTargetSelector.Select` of `CurrentLifeHighestFirst`
    /// before it hands its choice to the `ScoreRatingTargetSelector` it
    /// holds: of the candidates its filters pass, those whose edge stands
    /// beyond the skill's minimum range and within its range of the
    /// searcher's (`FightTransform.Distance2D`, less both radii), and of them
    /// the ones of the
    /// most life, every one tied at it (`SelectBySearchTargetType`); when no
    /// candidate stands in that ring, every one it passed, in order.
    fn life_priority(
        &self,
        source: &super::attacker::Attacker<'_>,
        admitted: Vec<(FightActorRef, FightActorView)>,
    ) -> Vec<(FightActorRef, FightActorView)> {
        let min_q32 = space_to_q32(source.min_range);
        let range_q32 = space_to_q32(source.attack_range);
        let mut most = 0;
        let mut in_range = Vec::new();
        let mut others = Vec::new();
        for (candidate, target) in admitted {
            let distance_q32 = target_edge_distance_q32(
                source.query_x_q32,
                source.query_z_q32,
                source.radius,
                target.x_q32,
                target.z_q32,
                target.radius,
            );
            if distance_q32 > min_q32 && distance_q32 <= range_q32 {
                let life = self.actor_life(candidate);
                if life < most {
                    continue;
                }
                if life > most {
                    in_range.clear();
                }
                most = life;
                in_range.push((candidate, target));
            } else {
                others.push((candidate, target));
            }
        }
        if in_range.is_empty() {
            others
        } else {
            in_range
        }
    }

    /// `FightActor.GetLife`: a unit's whole life, or a building's.
    fn actor_life(&self, actor: FightActorRef) -> i64 {
        match actor {
            FightActorRef::Unit(id) => self.actors.get(&id).map_or(0, |actor| actor.life),
            FightActorRef::Building(id) => self
                .buildings
                .iter()
                .find(|building| building.building_id == id)
                .map_or(0, |building| i64::from(building.life.current)),
        }
    }

    /// `MechSearchTargetController.Update`, before the unit's skills: a unit
    /// that searches for itself keeps its lock while it lives and the search
    /// is not due, counting down, and otherwise searches
    /// (`MechSearchTargetController.SearchLockTarget`) and waits ten updates.
    /// Its search scores from where the unit stands and its root points as
    /// it updates (`FightMech`'s `GetMainTransform`), over the whole turn,
    /// and falls back on any live enemy.
    pub(in crate::fight) fn update_mech_search(
        &mut self,
        actor_id: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<()> {
        let skill = &self.actors[&actor_id].skills.main;
        let Some(group) = skill.group.as_ref() else {
            return Ok(());
        };
        let Some(time) = group.mech_search_time else {
            return Ok(());
        };
        let lock_alive = group
            .mech_lock
            .is_some_and(|lock| self.fight_actor_is_alive(lock));
        let found = if lock_alive && time >= 1 {
            None
        } else {
            let source = self
                .mech_attacker(actor_id)
                .ok_or_else(|| Error::new("mech search source is absent"))?;
            Some(
                match self.select_normal_target_from(&source, target_search_order, true) {
                    Some(found) => Some(found),
                    None => self.select_alive_target(
                        SkillRef::main(FightActorRef::Unit(actor_id)),
                        None,
                        target_search_order,
                    )?,
                },
            )
        };
        let group = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .skills
            .main
            .group
            .as_mut()
            .expect("checked above");
        match found {
            None => group.mech_search_time = Some(time - 1),
            Some(found) => {
                group.mech_lock = found;
                group.mech_search_time = Some(10);
            }
        }
        Ok(())
    }

    /// `FightSkillBase.IsMainSearcher`: the main skill's slots but a joined
    /// row's, or, while it is active, the permanent preemptive skill.
    fn searches_for_owner(&self, skill_ref: SkillRef, slot: Option<usize>) -> bool {
        match skill_ref.slot {
            SkillSlot::Main => slot.is_none_or(|slot| self.skill(skill_ref).joined(slot).is_none()),
            SkillSlot::Extra(index) => {
                let skills = self.skills(skill_ref.owner);
                skills.preemptive_active && skills.extras[index].rules.preemptive.is_some()
            }
        }
    }

    /// `SkillSearchTargetController.TrySearchAliveTarget`, which
    /// `SearchLockTarget` falls back on when the skill's own search answers
    /// nothing, leaving the skill idle.
    ///
    /// The skill that searches for its owner (`FightSkillBase.IsMainSearcher`,
    /// every slot of the main skill) is offered every actor of the other side
    /// in either domain (`OpponentController.GetActors(Both)`) through
    /// `aliveTargetSelector`, a selector like its own that filters on nothing
    /// but `IsAlive`, scoring candidates where they stand when it runs. The
    /// other side's blocking constructions are offered only when nothing
    /// else answers. So a Vortex, which cannot fire at aircraft, that fells
    /// the last tower with only aircraft left locks onto one and walks on it.
    pub(in crate::fight) fn select_alive_target(
        &self,
        skill_ref: SkillRef,
        slot: Option<usize>,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<Option<FightActorRef>> {
        // It answers nothing for a skill that is no main searcher: an extra
        // skill's, unless it is the active permanent preemptive one, and a
        // slot of a row that joined the main skill's group.
        if !self.searches_for_owner(skill_ref, slot) {
            return Ok(None);
        }
        let source = self
            .skill_attacker(skill_ref)
            .ok_or_else(|| Error::new("target selector source is absent"))?;
        let (rotation_q32, attack_range, rotation_window_q32) = match (skill_ref.owner, slot) {
            (FightActorRef::Unit(actor_id), Some(slot)) if slot > 0 => {
                let actor = &self.actors[&actor_id];
                let (rotation, window) =
                    actor.default_search_frame(source.attack, slot).unwrap_or((
                        actor.slot_main_rotation_q32(source.attack, self.skill(skill_ref), slot),
                        None,
                    ));
                (
                    rotation,
                    self.slot_attack_range(skill_ref, Some(slot)),
                    window,
                )
            }
            _ => (
                source.query_rotation_q32,
                source.attack_range,
                source.rotation_window_q32,
            ),
        };
        let select = |blocking: bool| {
            let mut scoring = Scoring::default();
            for (&team, candidates) in target_search_order {
                if team == source.team {
                    continue;
                }
                for &candidate in candidates {
                    let Some(target) = self.fight_actor(candidate) else {
                        continue;
                    };
                    let blocks = matches!(candidate, FightActorRef::Building(id)
                        if self.rvo.passable_constructions.contains(&id));
                    if target.team != team || !target.alive || blocks != blocking {
                        continue;
                    }
                    let Some(score) = full_rotation_target_score_q32(
                        source.query_x_q32,
                        source.query_z_q32,
                        source.radius,
                        rotation_q32,
                        target.x_q32,
                        target.z_q32,
                        target.radius,
                        // `aliveTargetSelector` is the one the controller was
                        // made with, which no change of search type touches.
                        ScoreOffsets::default().for_candidate(target.domain, target.visible),
                        source.min_range,
                        attack_range,
                        rotation_window_q32,
                    ) else {
                        continue;
                    };
                    scoring.consider(candidate, score, target.visible);
                }
            }
            scoring.chosen(|next| self.target_in_attack_range(skill_ref, next))
        };
        Ok(select(false).or_else(|| select(true)))
    }
}

impl Simulation {
    /// `PerformGroupedSkillSearch` for a fusillade's core that fell back on
    /// what its siblings hold: every skill of the group whose lock is what
    /// the core found drops it (`ChangeLockTarget(null)`), before the core
    /// takes it.
    pub(in crate::fight) fn take_from_siblings(
        &mut self,
        skill_ref: SkillRef,
        found: Option<FightActorRef>,
    ) {
        let skill = self.skill_mut(skill_ref);
        if !skill.fusillade() {
            return;
        }
        let Some(found) = found else {
            return;
        };
        let group = skill.group.as_mut().expect("a fusillade is a group");
        for sibling in &mut group.siblings {
            if sibling.lock_target == Some(found) {
                // `ChangeAttackTarget(null, shield)` left the skill no attack
                // target while it fired at a shield.
                sibling.attack_target_left = sibling
                    .attack_target()
                    .filter(|_| sibling.shield_target().is_none());
                sibling.lock_target = None;
                group.mech_lock = None;
            }
        }
    }

    /// `PerformGroupedSkillSearch` excludes the other slots' locks before
    /// scoring opponents. If sharing is allowed and that answer cannot be
    /// attacked, the same selector is asked about the held targets instead.
    pub(in crate::fight) fn select_group_lock_replacement(
        &self,
        skill_ref: SkillRef,
        slot: usize,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<Option<FightActorRef>> {
        let actor_id = skill_ref
            .owner
            .unit_id()
            .expect("only a unit's skill is grouped");
        let source = &self.actors[&actor_id];
        let skill = self.skill(skill_ref);
        let attack = self.skill_rules(skill_ref);
        // A standalone weapon's skill searches as a skill of its own: no
        // group keeps it off what the others hold.
        if skill.standalone() && slot == 0 {
            return self.select_lock_replacement(skill_ref, target_search_order);
        }
        let held = skill
            .slot_locks()
            .into_iter()
            .enumerate()
            .filter_map(|(index, target)| (index != slot).then_some(target).flatten())
            .collect::<Vec<_>>();
        // A grouped slot whose siblings hold nothing searches as the core
        // does; a standalone weapon searches from its own weapon whatever
        // the others hold.
        if held.is_empty() && !skill.standalone() {
            if slot == 0 {
                return self.select_lock_replacement(skill_ref, target_search_order);
            }
            // `PerformGroupedSkillSearch` hands the slot's own search
            // controller to `PerformNormalSkillSearch`: the slot's skill is
            // the attacker, scoring from its own weapon and reaching its own
            // range. A Raiden's second gun whose Crawler died with its core's
            // takes the Crawler in its own reach that the core's would not.
            let mut source = self
                .skill_attacker(skill_ref)
                .ok_or_else(|| Error::new("a grouped skill's owner is absent"))?;
            let (rotation, window) = self.actors[&actor_id]
                .default_search_frame(attack, slot)
                .unwrap_or((
                    self.actors[&actor_id].slot_main_rotation_q32(attack, skill, slot),
                    None,
                ));
            source.query_rotation_q32 = rotation;
            source.rotation_window_q32 = window;
            source.attack_range = self.slot_attack_range(skill_ref, Some(slot));
            return Ok(self.perform_normal_skill_search(&source, target_search_order, false));
        }
        // The group's skill searches with its own selector, which a
        // technology may have turned to `DistanceIntensify`.
        let (score_offsets, targets) = self
            .skill_attacker(skill_ref)
            .map(|attacker| (attacker.score_offsets, attacker.targets))
            .ok_or_else(|| Error::new("a grouped skill's owner is absent"))?;
        let select = |shared: bool| {
            let mut scoring = Scoring::default();
            for (&team, candidates) in target_search_order {
                if team == source.placement.team {
                    continue;
                }
                for &candidate in candidates {
                    // An enemy tower is an opponent too: a slot with no unit
                    // left to it is allocated one.
                    let Some(target) = self.fight_actor(candidate) else {
                        continue;
                    };
                    // A grouped slot searches as it updates, where everything
                    // stands now: a unit a beam turned since the tick opened
                    // stands on its own side, though the prepared trees
                    // still hold it on the other.
                    if (held.contains(&candidate) != shared && !skill.standalone())
                        || (!skill.standalone() && target.team != team)
                        || !target.alive
                        || !target.targetable
                        || !target.searchable(skill.standalone())
                        || matches!(candidate, FightActorRef::Building(id)
                            if self.unsearchable_buildings.contains(&id))
                        || !targets.accepts(target.domain)
                    {
                        continue;
                    }
                    let (rotation, window) = source
                        .default_search_frame(attack, slot)
                        .unwrap_or((source.slot_main_rotation_q32(attack, skill, slot), None));
                    // A standalone weapon's skill is a main skill: its search
                    // is the one `FightCoreSystem.PreCalculate` prepared, on
                    // where everything stood as the tick opened.
                    let (target_x_q32, target_z_q32, visible) = if skill.standalone() {
                        (target.query_x_q32, target.query_z_q32, target.query_visible)
                    } else {
                        (target.x_q32, target.z_q32, target.visible)
                    };
                    let Some(score) = full_rotation_target_score_q32(
                        source.target_query_x_q32,
                        source.target_query_z_q32,
                        source.rules.collision_radius(),
                        rotation,
                        target_x_q32,
                        target_z_q32,
                        target.radius,
                        score_offsets.for_candidate(target.domain, visible),
                        attack.min_range(),
                        self.slot_attack_range(skill_ref, Some(slot)),
                        window,
                    ) else {
                        continue;
                    };
                    scoring.consider(candidate, score, visible);
                }
            }
            scoring.chosen(|next| self.target_in_attack_range(skill_ref, next))
        };
        // A fusillade's core falls back on what its siblings hold as a
        // group that shares does, and takes what it finds from them
        // (`take_from_siblings`).
        let fusillade_core = slot == 0 && skill.fusillade();
        let selected = select(false);
        if (attack.weapons.allow_same_target == Some(true) || fusillade_core)
            && selected.is_none_or(|target| {
                !self.slot_target_in_attack_range(skill_ref, Some(slot), target)
            })
        {
            return Ok(select(true).or(selected));
        }
        Ok(selected)
    }
}
