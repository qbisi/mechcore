//! `BuildingCollideController`: ground units crush lower-priority crystals.

use super::*;
use crate::data::Index;

const MAX_DEPTH: u8 = 7;

/// `BuildingFunctionController`'s tree and `BuildingCollideController`'s delays.
pub(in crate::fight) struct BuildingCollisions {
    tree: FightQuadtree<usize>,
    pub(super) damage_tree: FightQuadtree<usize>,
    delays: BTreeMap<usize, i32>,
    /// Neutral crystals have no recorded identity, but their `OnDead` still
    /// runs in the next `DeadEffectSystem` update and holds its finish gate.
    pub(in crate::fight) pending: Vec<usize>,
}

impl BuildingCollisions {
    pub(in crate::fight) fn new(crystals: &[MapCrystal]) -> Self {
        // BuildingSystem.Preprocess populates this tree during BattleSystem.Init,
        // before the teams create towers and constructions. OnFightStart syncs
        // that map tree; later BuildingSystem.AddBuilding only appends its list.
        let mut tree = FightQuadtree::with_max_depth(MAX_DEPTH);
        let mut damage_tree = FightQuadtree::with_max_depth(MAX_DEPTH);
        for (index, crystal) in crystals.iter().enumerate() {
            let range = TargetActorRect {
                min_x: crystal.x_q32 - crystal.radius_q32,
                max_x: crystal.x_q32 + crystal.radius_q32,
                min_z: crystal.z_q32 - crystal.radius_q32,
                max_z: crystal.z_q32 + crystal.radius_q32,
            };
            // BuildingFunctionController.IsBuildingInCheckRange excludes
            // out-of-bound map decorations before the tree is populated.
            if tree.root.rect.contains(range) {
                tree.insert_range(index, range);
                if crystal.strength > 0 {
                    damage_tree.insert_range(index, range);
                }
            }
        }
        Self {
            tree,
            damage_tree,
            delays: BTreeMap::new(),
            pending: Vec::new(),
        }
    }
}

impl Simulation {
    /// `BuildingCollideController.Update`, after `DeadEffectSystem`.
    pub(in crate::fight) fn step_building_collisions(&mut self) {
        for delay in self.building_collisions.delays.values_mut() {
            *delay = delay.saturating_sub(1);
        }
        let nodes = self
            .building_collisions
            .tree
            .all_interactable_nodes()
            .into_iter()
            .map(|node| (node.rect, node.depth, node.elements.clone()))
            .collect::<Vec<_>>();
        for (rect, depth, elements) in nodes {
            self.check_building_collisions_node(rect, depth, elements);
        }
    }

    /// `BuildingCollideController.Check` for one nonempty crystal node.
    fn check_building_collisions_node(
        &mut self,
        rect: TargetActorRect,
        depth: u8,
        elements: Vec<usize>,
    ) {
        for key in elements.into_iter().rev() {
            if self
                .building_collisions
                .delays
                .get(&key)
                .is_some_and(|&delay| delay > 0)
            {
                continue;
            }
            let crystal = &self.rvo.map_crystals[key];
            let (x, z, radius, priority, alive) = (
                crystal.x_q32,
                crystal.z_q32,
                crystal.radius_q32,
                crystal.collider_priority,
                crystal.alive,
            );
            if !alive {
                continue;
            }
            let width = (rect.max_x - rect.min_x) / 2;
            let height = (rect.max_z - rect.min_z) / 2;
            let range = TargetActorRect {
                min_x: x - width,
                max_x: x + width,
                min_z: z - height,
                max_z: z + height,
            };
            let candidates = [0, 1]
                .into_iter()
                .flat_map(|team| {
                    self.mech_quadtrees
                        .get(&team)
                        .into_iter()
                        .flat_map(|tree| tree.query_rect(range))
                })
                .collect::<Vec<_>>();
            let mut wait = if candidates.is_empty() {
                i32::from(MAX_DEPTH - depth) + 1
            } else {
                -1
            };
            for candidate in candidates {
                let FightActorRef::Unit(id) = candidate else {
                    continue;
                };
                if !self.is_mech_move_into_building(id, (x, z, radius, priority), &mut wait) {
                    continue;
                }
                self.rvo.map_crystals[key].alive = false;
                self.building_collisions.pending.push(key);
                wait = -1;
                break;
            }
            if wait > 0 {
                self.building_collisions.delays.insert(key, wait);
            }
        }
    }

    /// `BuildingCollideController.IsMechMoveIntoBuilding`, including its next
    /// check bound from the distance and the greater of nominal/current speed.
    fn is_mech_move_into_building(
        &self,
        id: u64,
        (x, z, radius, priority): (i64, i64, i64, i32),
        wait: &mut i32,
    ) -> bool {
        let actor = &self.actors[&id];
        if actor.domain == UnitDomain::Air
            || !actor.alive()
            || !matches!(actor.visibility, Visibility::Normal | Visibility::Disappear)
            || rvo_profile(&actor.rules).collider_priority <= priority
        {
            return false;
        }
        let distance = native_q32_magnitude(actor.x_q32 - x, actor.z_q32 - z)
            .saturating_sub(space_to_q32(actor.rules.collision_radius()))
            .saturating_sub(radius)
            .max(0);
        if fpoint_less_or_equal(distance, 0) {
            return true;
        }
        let mut speed = actor
            .rules
            .move_speed()
            .saturating_add(actor.stats.unit_data_value(Index::MoveSpeed))
            / 1000;
        if speed <= 0 {
            *wait = i32::MAX;
            return false;
        }
        let current = actor.stats.move_speed_q32();
        if rvo::fpoint_greater_than(current, speed << 32) {
            speed = (current >> 32) + 1;
        }
        let next =
            i32::try_from(q32_div(q32_div(distance, speed << 32), NATIVE_LOGIC_DELTA_Q32) >> 32)
                .unwrap_or(i32::MAX);
        *wait = if *wait >= 0 { (*wait).min(next) } else { next };
        false
    }

    /// `FightCrystal.OnDead` removes the neutral crystal from `BuildingSystem`
    /// and deactivates its RVO controller; it emits no recorded building event.
    pub(in crate::fight) fn finish_crystal_deaths(&mut self) {
        for index in std::mem::take(&mut self.building_collisions.pending) {
            self.rvo.map_crystals[index].destroyed = true;
            self.building_collisions.tree.remove(index);
            self.building_collisions.damage_tree.remove(index);
            self.building_collisions.delays.remove(&index);
        }
    }
}
