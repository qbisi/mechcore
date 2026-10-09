//! `SimplePathFindingController`: the way a huge ground unit
//! (`IsEnableAvoidanceAssist`) walks around its own side's towers. Every
//! `MotionController.Move` that hands the agent a point first asks it
//! (`CalculateNextPoint`): a tower standing between the unit and what it
//! walks to turns the point aside, two of the unit's radii from where it
//! stands, and the point holds for up to ten moves while the target stays
//! where it was. `docs/rules/towers.md` states the rule.

use super::math::fpoint_less_or_equal;
use super::shield::{Vector, normalized, scale};
use super::support_unit::turn_about_vertical;
use super::*;
use crate::rules::UnitSize;

/// `IsAvoidancePointAvaliable`: a point is reused while the count of moves
/// since it was found is at most this.
const LAST_REUSED_CHECK: i32 = 9;

/// `CalculateNextPoint` with no lock sets its count here.
const NO_LOCK_CHECK: i32 = 5;

/// The height of `CalculateAvoidancePoint`'s second plane point above the
/// unit's ground, `FPoint` 10.
const PLANE_HEIGHT: i64 = 10 << 32;

/// The most `CalculateAvoidancePoint` turns the point towards the tower, in
/// degrees.
const MAX_TURN: i64 = 60;

/// One unit's `SimplePathFindingController`.
#[derive(Debug, Clone, Default)]
pub(in crate::fight) struct PathFinding {
    /// `target`: the lock the last point was found for.
    target: Option<FightActorRef>,
    /// `targetRangeRecord`: where that lock stood, and twice its radius.
    target_range: (i64, i64, i64),
    /// `avoidanceActor`: the tower the last point turns around.
    avoidance_actor: Option<FightActorRef>,
    /// `avoidancePoint`.
    avoidance_point: (i64, i64),
    /// `checkCount`: the moves since the point was found.
    check_count: i32,
}

impl PathFinding {
    /// `MotionController`'s constructor makes one for a unit whose
    /// `IsEnableAvoidanceAssist` answers, `MechData`'s: a huge unit that
    /// does not fly.
    pub(in crate::fight) fn of(rules: &UnitConfig) -> Option<Self> {
        (rules.size == UnitSize::Huge && rules.domain == UnitDomain::Ground).then(Self::default)
    }

    /// `IsAvoidancePointAvaliable`: a point found, not too many moves ago,
    /// for the lock the unit still holds unless the target stands still,
    /// which still stands within twice its radius of where it stood.
    fn avoidance_point_available(
        &self,
        lock: FightActorRef,
        target_position: (i64, i64),
        static_target: bool,
    ) -> bool {
        if self.avoidance_actor.is_none() || self.check_count > LAST_REUSED_CHECK {
            return false;
        }
        if !static_target && self.target != Some(lock) {
            return false;
        }
        // `CircleRange.Contains`.
        let (x, z, radius) = self.target_range;
        let distance = native_q32_magnitude(
            target_position.0.saturating_sub(x),
            target_position.1.saturating_sub(z),
        );
        fpoint_less_or_equal(distance, radius)
    }
}

impl Simulation {
    /// `MotionController.Move`'s point, through the unit's path finding when
    /// it has one (`CalculateNextPoint`). A command's point stands still
    /// (`MoveAttackCommand.IsStaticTarget`); a lock's moves
    /// (`AutoMoveBehaviour`).
    pub(in crate::fight) fn next_move_point(
        &mut self,
        actor_id: u64,
        point: (i64, i64),
        static_target: bool,
    ) -> (i64, i64) {
        let actor = &self.actors[&actor_id];
        let Some(mut path) = actor.motion.path_finding.clone() else {
            return point;
        };
        let next = self.calculate_next_point(actor_id, &mut path, point, static_target);
        self.actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .motion
            .path_finding = Some(path);
        next
    }

    /// `SimplePathFindingController.CalculateNextPoint`.
    fn calculate_next_point(
        &self,
        actor_id: u64,
        path: &mut PathFinding,
        point: (i64, i64),
        static_target: bool,
    ) -> (i64, i64) {
        if path.check_count < 0 {
            return point;
        }
        path.check_count += 1;
        let Some(lock) = self.actors[&actor_id].mech_lock() else {
            path.check_count = NO_LOCK_CHECK;
            return point;
        };
        // A moving target is asked where the lock the point was found for
        // stands now, not where the move would go.
        let target_position = path
            .target
            .filter(|_| !static_target)
            .and_then(|target| self.fight_actor(target))
            .map_or(point, |view| (view.x_q32, view.z_q32));
        if path.avoidance_point_available(lock, target_position, static_target) {
            return path.avoidance_point;
        }
        path.check_count = 0;
        path.target = Some(lock);
        let lock_radius = self
            .fight_actor(lock)
            .map_or(0, |view| space_to_q32(view.radius));
        // The record's centre goes through `FVector3`'s conversion to an
        // `FVector2`, its `x` and `y`, then `ToVector3XZ` and the conversion
        // again: the target's `x` and nought. A target away from the map's
        // middle line is never inside it, and the point is found anew on
        // every move.
        path.target_range = (target_position.0, 0, lock_radius.saturating_mul(2));
        path.avoidance_actor =
            self.find_avoidance_actor(actor_id, path, target_position, lock_radius);
        let Some(tower) = path.avoidance_actor else {
            return point;
        };
        path.avoidance_point = self.calculate_avoidance_point(actor_id, tower, target_position);
        path.avoidance_point
    }

    /// `FindAvoidanceActor`: the first of the unit's side's towers within
    /// three of its radii (`GetNeighbours`) that stands in the strip from it
    /// to the target, as wide as the larger of the two radii. A unit with no
    /// tower that near stops asking for good (`checkCount` -1).
    fn find_avoidance_actor(
        &self,
        actor_id: u64,
        path: &mut PathFinding,
        target_position: (i64, i64),
        lock_radius: i64,
    ) -> Option<FightActorRef> {
        let actor = &self.actors[&actor_id];
        let radius = space_to_q32(actor.rules.collision_radius());
        let range = radius.saturating_mul(3);
        let start = (actor.x_q32, actor.z_q32);
        let width = if rvo::fpoint_less_than(lock_radius, radius) {
            radius
        } else {
            lock_radius
        };
        let neighbours: Vec<_> = self
            .buildings
            .iter()
            .filter(|building| building.team_id == actor.placement.team)
            .map(|building| FightActorRef::Building(building.building_id))
            .filter(|&tower| self.is_tower(tower))
            .filter_map(|tower| self.fight_actor(tower).map(|view| (tower, view)))
            .filter(|(_, view)| view.alive)
            .filter(|(_, view)| fpoint_less_or_equal(distance_2d(start, radius, view), range))
            .collect();
        if neighbours.is_empty() {
            path.check_count = -1;
            return None;
        }
        neighbours
            .into_iter()
            .find(|(_, view)| {
                line_range_contains(start, target_position, width, (view.x_q32, view.z_q32))
            })
            .map(|(tower, _)| tower)
    }

    /// `CalculateAvoidancePoint`: two of the unit's radii from where it
    /// stands, square to the tower on the target's side of the vertical
    /// plane through the unit and the tower, turned towards the tower by up
    /// to 60 degrees as the tower's edge is further than those two radii.
    fn calculate_avoidance_point(
        &self,
        actor_id: u64,
        tower: FightActorRef,
        target_position: (i64, i64),
    ) -> (i64, i64) {
        let actor = &self.actors[&actor_id];
        let view = self.fight_actor(tower).expect("an avoided tower stands");
        let own: Vector = (actor.x_q32, 0, actor.z_q32);
        // `FPlane(a, b, c)`: the normal of (b - a) × (c - a), normalized,
        // and its distance -normal·a.
        let up: Vector = (0, PLANE_HEIGHT, 0);
        let to_tower: Vector = (
            view.x_q32.wrapping_sub(own.0),
            0,
            view.z_q32.wrapping_sub(own.2),
        );
        let normal = normalized(cross(up, to_tower));
        let distance = dot(normal, own).wrapping_neg();
        let target: Vector = (target_position.0, 0, target_position.1);
        // `FPlane.GetSide`.
        let side = rvo::fpoint_greater_than(dot(normal, target).saturating_add(distance), 0);
        let mut direction = if side {
            normal
        } else {
            (
                normal.0.wrapping_neg(),
                normal.1.wrapping_neg(),
                normal.2.wrapping_neg(),
            )
        };
        let radius = space_to_q32(actor.rules.collision_radius());
        let edge = distance_2d((actor.x_q32, actor.z_q32), radius, &view);
        let edge = if rvo::fpoint_less_than(edge, 0) {
            0
        } else {
            edge
        };
        let step = radius.saturating_mul(2);
        if rvo::fpoint_greater_than(edge, 0) {
            let share = math::fpoint_min(Q32_ONE, q32_div(edge, step));
            let angle = share.saturating_mul(MAX_TURN);
            // About `FVector3.down` on the plane's side, `up` off it.
            let axis = if side { -Q32_ONE } else { Q32_ONE };
            let (x, z) = turn_about_vertical(angle, axis, direction.0, direction.2);
            direction = (x, direction.1, z);
        }
        let offset = scale(normalized(direction), step);
        (own.0.wrapping_add(offset.0), own.2.wrapping_add(offset.2))
    }
}

/// `FightActor.Distance2D`: edge to edge, never below zero.
pub(in crate::fight) fn distance_2d(from: (i64, i64), radius: i64, to: &FightActorView) -> i64 {
    native_q32_magnitude(
        to.x_q32.saturating_sub(from.0),
        to.z_q32.saturating_sub(from.1),
    )
    .saturating_sub(radius)
    .saturating_sub(space_to_q32(to.radius))
    .max(0)
}

/// `LineRange.Contains(FVector2)`: an end itself, or a point no further
/// across the segment than its width and neither end's angle obtuse. The
/// build measures the angles and the sine in its fixed point; this asks the
/// same of the exact products.
fn line_range_contains(start: (i64, i64), end: (i64, i64), width: i64, point: (i64, i64)) -> bool {
    if point == start || point == end {
        return true;
    }
    let (dx, dz) = (
        i128::from(end.0) - i128::from(start.0),
        i128::from(end.1) - i128::from(start.1),
    );
    let (px, pz) = (
        i128::from(point.0) - i128::from(start.0),
        i128::from(point.1) - i128::from(start.1),
    );
    let (qx, qz) = (
        i128::from(point.0) - i128::from(end.0),
        i128::from(point.1) - i128::from(end.1),
    );
    if px * dx + pz * dz < 0 || -(qx * dx + qz * dz) < 0 {
        return false;
    }
    // The distance across, |p × d| / |d|, against the width.
    let across = (px * dz - pz * dx).abs();
    let length = math::magnitude(
        i64::try_from(dx).unwrap_or(i64::MAX),
        i64::try_from(dz).unwrap_or(i64::MAX),
    );
    across <= i128::from(width) * i128::from(length)
}

/// `FVector3.Cross`.
fn cross(left: Vector, right: Vector) -> Vector {
    (
        q32_mul(left.1, right.2).wrapping_sub(q32_mul(left.2, right.1)),
        q32_mul(left.2, right.0).wrapping_sub(q32_mul(left.0, right.2)),
        q32_mul(left.0, right.1).wrapping_sub(q32_mul(left.1, right.0)),
    )
}

/// `FVector3.Dot`.
fn dot(left: Vector, right: Vector) -> i64 {
    q32_mul(left.0, right.0)
        .wrapping_add(q32_mul(left.1, right.1))
        .wrapping_add(q32_mul(left.2, right.2))
}
