//! `PilotAI` with a `MoveAttackCommand`: the path a Mobile Beacon gives the
//! units it selects.
//!
//! `CSRC_WayPoint.OnFightStart` selects its side's units whose bounds meet a
//! circle of `subEffectRange` around the release's first position, and hands
//! each a command that walks the path, keeping the unit's offset from that
//! position. The command is the unit's `MotionController.moveBehaviour` until
//! it arrives at the path's end: a unit with a command moves towards the
//! command's point rather than towards its lock, and may walk on while it
//! fires. `PilotAI.Update` runs after the unit's motion and moves the command
//! on once the unit is near its point. `docs/rules/battle_skill.md` is the
//! rule.

use super::{
    FightActorRef, Q32_ONE, Simulation,
    math::{direction_degrees_q32_raw, native_q32_magnitude, space_to_q32},
    motion::{clamp_magnitude_q32_raw, normalized_velocity_q32_raw},
    rvo::{fpoint_less_or_equal, fpoint_less_than, q32_div},
};
use crate::layout::{SkillEffect, SkillRelease};

/// `MoveAttackCommand`'s reach, `FPoint` metres: its point stands this far
/// past a segment's end, and a unit this near it has arrived.
const REACH_Q32: i64 = 20 * Q32_ONE;
/// `FVector3.get_normalized`'s epsilon.
const NORMALIZE_EPSILON: i64 = 0xA7C5;
/// `MotionController.MIN_MOVE_DISTANCE`.
const MIN_MOVE_DISTANCE_Q32: i64 = 0x028f_5c28;
/// The farthest `IsEnableAttackMove` looks for an enemy near the path.
const ATTACK_MOVE_REACH: i64 = 140_000;

/// One unit's `MoveAttackCommand`.
#[derive(Debug, Clone)]
pub(in crate::fight) struct MoveCommand {
    /// Each segment's start and end, `FPoint` raw metres.
    segments: Vec<((i64, i64), (i64, i64))>,
    /// `LineRange.width`.
    width_q32: i64,
    /// Where the unit stood from the first position when it was selected.
    offset: (i64, i64),
    /// `currentIndex`.
    index: usize,
    /// `RefreshCurrentTargetInfo`'s point for the current segment.
    point: (i64, i64),
}

impl MoveCommand {
    fn new(segments: Vec<((i64, i64), (i64, i64))>, width_q32: i64, offset: (i64, i64)) -> Self {
        let mut command = Self {
            segments,
            width_q32,
            offset,
            index: 0,
            point: (0, 0),
        };
        command.refresh_point();
        command
    }

    /// `RefreshCurrentTargetInfo`: from the segment's start, moved by the
    /// unit's offset, along the segment for its length and `REACH` more.
    fn refresh_point(&mut self) {
        let ((start_x, start_z), (end_x, end_z)) = self.segments[self.index];
        let (from_x, from_z) = (
            start_x.saturating_add(self.offset.0),
            start_z.saturating_add(self.offset.1),
        );
        let (along_x, along_z) = (
            end_x.saturating_add(self.offset.0).saturating_sub(from_x),
            end_z.saturating_add(self.offset.1).saturating_sub(from_z),
        );
        // `FVector3.get_normalized` and `get_magnitude`.
        let length = native_q32_magnitude(along_x, along_z);
        let (unit_x, unit_z) = if length < NORMALIZE_EPSILON {
            (0, 0)
        } else {
            let inverse = q32_div(Q32_ONE, length);
            (q32(along_x, inverse), q32(along_z, inverse))
        };
        let reach = length.saturating_add(REACH_Q32);
        self.point = (
            from_x.saturating_add(q32(unit_x, reach)),
            from_z.saturating_add(q32(unit_z, reach)),
        );
    }
}

impl Simulation {
    /// `CSRC_WayPoint.OnFightStart` for every path released: the releases
    /// left for the fight to land are the others.
    pub(in crate::fight) fn start_paths(
        &mut self,
        releases: Vec<SkillRelease>,
    ) -> Vec<SkillRelease> {
        let mut landing = Vec::new();
        for release in releases {
            let SkillEffect::Path { points, width_q32 } = &release.effect else {
                landing.push(release);
                continue;
            };
            let points = points
                .iter()
                .map(|&(x, z)| (space_to_q32(x), space_to_q32(z)))
                .collect::<Vec<_>>();
            let segments = points
                .windows(2)
                .map(|pair| (pair[0], pair[1]))
                .collect::<Vec<_>>();
            let (first_x, first_z) = points[0];
            for actor in self.actors.values_mut() {
                // A unit already walking a path of its own side keeps it.
                if actor.placement.team != release.team || !actor.alive() || actor.command.is_some()
                {
                    continue;
                }
                let (offset_x, offset_z) = (
                    actor.x_q32.saturating_sub(first_x),
                    actor.z_q32.saturating_sub(first_z),
                );
                let reach = width_q32.saturating_add(space_to_q32(actor.rules.collision_radius()));
                if !fpoint_less_or_equal(native_q32_magnitude(offset_x, offset_z), reach) {
                    continue;
                }
                let command = MoveCommand::new(segments.clone(), *width_q32, (offset_x, offset_z));
                // The unit enters the fight facing along the first segment,
                // and the presearch scores from there.
                let ((start_x, start_z), (end_x, end_z)) = segments[0];
                let facing = direction_degrees_q32_raw(
                    end_x.saturating_sub(start_x),
                    end_z.saturating_sub(start_z),
                );
                actor.set_body_rotation(facing);
                actor.aim_rotation = actor.body_rotation;
                actor.set_weapon_rotation(facing);
                actor.target_query_source_rotation_q32 = facing;
                actor.command = Some(command);
            }
        }
        landing
    }

    /// Where `MotionController.Move` sends a unit with a command: towards
    /// the command's point, stopping `REACH` short of it.
    pub(in crate::fight) fn command_move_point(&self, actor_id: u64) -> Option<(i64, i64)> {
        let actor = &self.actors[&actor_id];
        let command = actor.command.as_ref()?;
        let delta_x = command.point.0.saturating_sub(actor.x_q32);
        let delta_z = command.point.1.saturating_sub(actor.z_q32);
        let distance = native_q32_magnitude(delta_x, delta_z).saturating_sub(REACH_Q32);
        let (step_x, step_z) = if distance > MIN_MOVE_DISTANCE_Q32 {
            clamp_magnitude_q32_raw(delta_x, delta_z, distance)
        } else {
            normalized_velocity_q32_raw(delta_x, delta_z, MIN_MOVE_DISTANCE_Q32)
        };
        Some((
            actor.x_q32.saturating_add(step_x),
            actor.z_q32.saturating_add(step_z),
        ))
    }

    /// `MoveAttackCommand.IsEnableAttackMove`: whether a unit attacking
    /// walks on. A melee unit stops; a ranged one stops only for an enemy
    /// near what is left of its segment and within its reach.
    pub(in crate::fight) fn command_attack_moves(&self, actor_id: u64) -> bool {
        let actor = &self.actors[&actor_id];
        let Some(command) = actor.command.as_ref() else {
            return false;
        };
        if actor.rules.attack.melee {
            return false;
        }
        let reach = space_to_q32(self.main_attack_range(actor_id).min(ATTACK_MOVE_REACH));
        let (start, end) = command.remaining_segment(actor.x_q32, actor.z_q32);
        !self
            .opponents_of(actor.placement.team)
            .into_iter()
            .filter_map(|opponent| self.fight_actor(opponent))
            .filter(|view| view.alive)
            .any(|view| {
                let radius = space_to_q32(view.radius);
                // `LineRange.Overlaps` measures from the segment half its
                // width out; the point past its end is the whole width out.
                let near_line = (start != end
                    && fpoint_less_or_equal(
                        segment_distance(start, end, (view.x_q32, view.z_q32)),
                        (command.width_q32 / 2).saturating_add(radius),
                    ))
                    || fpoint_less_than(
                        native_q32_magnitude(
                            view.x_q32.saturating_sub(end.0),
                            view.z_q32.saturating_sub(end.1),
                        ),
                        radius.saturating_add(command.width_q32),
                    );
                // `FightActor.Distance2D`: edge to edge, never below zero.
                let distance = native_q32_magnitude(
                    view.x_q32.saturating_sub(actor.x_q32),
                    view.z_q32.saturating_sub(actor.z_q32),
                )
                .saturating_sub(space_to_q32(actor.rules.collision_radius()))
                .saturating_sub(radius)
                .max(0);
                near_line && fpoint_less_than(distance, reach)
            })
    }

    /// `PilotAI.Update`, after the unit's motion: `MoveAttackCommand.Perform`
    /// moves the command on once the unit is near its point, and a finished
    /// command leaves the unit to `AutoMoveBehaviour`.
    pub(in crate::fight) fn perform_command(&mut self, actor_id: u64) {
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        if !actor.alive() {
            return;
        }
        let (x, z, radius) = (
            actor.x_q32,
            actor.z_q32,
            space_to_q32(actor.rules.collision_radius()),
        );
        let Some(command) = actor.command.as_mut() else {
            return;
        };
        // `FightCalculator.IsInRange2D`: the distance less the unit's radius.
        let distance = native_q32_magnitude(
            command.point.0.saturating_sub(x),
            command.point.1.saturating_sub(z),
        )
        .saturating_sub(radius);
        if !fpoint_less_or_equal(distance, REACH_Q32) {
            return;
        }
        command.index += 1;
        if command.index < command.segments.len() {
            command.refresh_point();
        } else {
            actor.command = None;
        }
    }

    fn opponents_of(&self, team: u32) -> Vec<FightActorRef> {
        self.actors
            .iter()
            .filter(|(_, actor)| actor.placement.team != team)
            .map(|(&id, _)| FightActorRef::Unit(id))
            .chain(
                self.buildings
                    .iter()
                    .filter(|building| building.team_id != team)
                    .map(|building| FightActorRef::Building(building.building_id)),
            )
            .collect()
    }
}

impl MoveCommand {
    /// `CalculateMoveLineRange`: the current segment from where the unit's
    /// foot falls on it to its end, or the end alone once the unit is past.
    fn remaining_segment(&self, x: i64, z: i64) -> ((i64, i64), (i64, i64)) {
        let (start, end) = self.segments[self.index];
        let along = (start.0.saturating_sub(end.0), start.1.saturating_sub(end.1));
        let from_end = (x.saturating_sub(end.0), z.saturating_sub(end.1));
        let dot = q32(along.0, from_end.0).saturating_add(q32(along.1, from_end.1));
        if dot <= 0 {
            return (end, end);
        }
        let length_squared = q32(along.0, along.0).saturating_add(q32(along.1, along.1));
        if length_squared <= 0 {
            return (end, end);
        }
        let share = q32_div(dot, length_squared);
        (
            (
                end.0.saturating_add(q32(along.0, share)),
                end.1.saturating_add(q32(along.1, share)),
            ),
            end,
        )
    }
}

fn q32(left: i64, right: i64) -> i64 {
    super::rvo::q32_mul(left, right)
}

/// The distance from a point to a segment.
fn segment_distance(start: (i64, i64), end: (i64, i64), point: (i64, i64)) -> i64 {
    let along = (end.0.saturating_sub(start.0), end.1.saturating_sub(start.1));
    let from_start = (
        point.0.saturating_sub(start.0),
        point.1.saturating_sub(start.1),
    );
    let length_squared = q32(along.0, along.0).saturating_add(q32(along.1, along.1));
    let share = if length_squared <= 0 {
        0
    } else {
        q32_div(
            q32(along.0, from_start.0).saturating_add(q32(along.1, from_start.1)),
            length_squared,
        )
        .clamp(0, Q32_ONE)
    };
    let foot = (
        start.0.saturating_add(q32(along.0, share)),
        start.1.saturating_add(q32(along.1, share)),
    );
    native_q32_magnitude(
        point.0.saturating_sub(foot.0),
        point.1.saturating_sub(foot.1),
    )
}
