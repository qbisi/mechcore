use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) struct TargetActorRect {
    pub(in crate::fight) min_x: i64,
    pub(in crate::fight) min_z: i64,
    pub(in crate::fight) max_x: i64,
    pub(in crate::fight) max_z: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::fight) struct TargetActorQuadtreeNode {
    pub(in crate::fight) rect: TargetActorRect,
    pub(in crate::fight) depth: u8,
    pub(in crate::fight) elements: Vec<FightActorRef>,
    pub(in crate::fight) children: Option<Box<[TargetActorQuadtreeNode; 4]>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::fight) struct TargetActorQuadtree {
    pub(in crate::fight) root: TargetActorQuadtreeNode,
    pub(in crate::fight) ranges: BTreeMap<FightActorRef, TargetActorRect>,
}

impl TargetActorRect {
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

impl TargetActorQuadtreeNode {
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

    pub(in crate::fight) fn find_path(
        &self,
        candidate: FightActorRef,
        path: &mut Vec<usize>,
    ) -> bool {
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

    pub(in crate::fight) fn append_query_order(&self, output: &mut Vec<FightActorRef>) {
        output.extend(self.elements.iter().copied());
        if let Some(children) = &self.children {
            for child in children {
                child.append_query_order(output);
            }
        }
    }
}

impl TargetActorQuadtree {
    pub(in crate::fight) fn new() -> Self {
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

    pub(in crate::fight) fn insert(
        &mut self,
        candidate: FightActorRef,
        x_q32: i64,
        z_q32: i64,
        radius: i64,
    ) {
        let range = TargetActorRect::around(x_q32, z_q32, radius);
        self.ranges.insert(candidate, range);
        if self.root.rect.contains(range) {
            self.root.insert(candidate, &self.ranges);
        } else {
            self.root.elements.push(candidate);
        }
    }

    /// Takes an element out, keeping the order of the rest.
    pub(in crate::fight) fn remove(&mut self, candidate: FightActorRef) {
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
        candidate: FightActorRef,
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

    pub(in crate::fight) fn query_order(&self) -> Vec<FightActorRef> {
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
pub(in crate::fight) fn initialize_target_quadtrees(
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
        // A construction no unit searches for is still in the tree: the
        // selector passes it over, and a splash takes it where it stands in
        // the tree's order — an Arclight's shot at a block of a wall reads its
        // damage on the block between the Crawlers around it.
        for building in team_buildings {
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

#[allow(clippy::too_many_arguments)]
pub(in crate::fight) fn normal_visible_full_rotation_target_score_q32(
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

pub(in crate::fight) fn normal_visible_full_rotation_score_from_distance_and_angle_q32(
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

impl Simulation {
    pub(in crate::fight) fn refresh_target_query_snapshot(&mut self) {
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
                        .skill
                        .weapon_rotations_q32
                        .first()
                        .copied()
                        .unwrap_or(actor.body_rotation_q32)
                } else {
                    actor.body_rotation_q32
                };
            actor.target_query_alive = actor.alive();
            actor.skill.searched_this_tick = false;
        }
    }

    pub(in crate::fight) fn target_search_order(&self) -> BTreeMap<u32, Vec<FightActorRef>> {
        self.target_quadtrees
            .iter()
            .map(|(&team, tree)| (team, tree.query_order()))
            .collect()
    }

    pub(in crate::fight) fn select_normal_building_target(
        &self,
        actor_id: u64,
        team_id: u32,
    ) -> Option<u64> {
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

    pub(in crate::fight) fn select_normal_target_with_order(
        &self,
        actor_id: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        use_live_candidate_positions: bool,
    ) -> Result<Option<FightActorRef>> {
        let source = self
            .actors
            .get(&actor_id)
            .ok_or_else(|| Error::new("target selector source actor is absent"))?;
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
                    || matches!(candidate, FightActorRef::Building(id)
                        if self.unsearchable_buildings.contains(&id))
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
}
