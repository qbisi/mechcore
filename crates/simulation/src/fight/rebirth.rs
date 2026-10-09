//! `DeadRebirthController`: a unit that a technology brings back a while
//! after it dies.
//!
//! `DeadEffectProvider.DoActive` hands `DeadEffectSystem` each unit whose
//! technology is an `IRebirthData`. As one dies, `DeadEffectSystem.Update`
//! hands it to the rebirth controller (`PerformDeadEffect`) before its
//! `OnDead`: the first time, its count is the source's
//! (`CostRebirthCount`, `GetRebirthCount`), and while any is left one is
//! spent and a task starts (`RebirthTask.StartTask`) where the unit fell. A
//! unit that does not follow an ally is marked rising
//! (`FightMech.isRebirthing`), which holds its side standing
//! (`FightCoreSystem.TryDstroyTower`, `IsStepFinish`) while it waits; one
//! that does sends its pilot (`RebirthSurvival`) after the nearest ally of
//! its type (`TryGetNearestTeamMech`, `SetFollowTarget`).
//!
//! Each update after, the same tick first, every task counts one
//! (`RebirthTask.Update`; a task that ends leaves the one after it uncounted
//! on that update). A pilot whose ally died turns to the nearest other, and
//! one left with none fails, its unit gone (`UpdateReadyRebirth`). Once a
//! task has counted the source's whole seconds over a tick the unit stands
//! again (`RebirthTask.RebirthMech`) where it fell, or where its pilot flies,
//! facing as the ally it rises behind: its life refilled
//! (`FightMech.ForceRecoveryLife`), back among its side's active units
//! (`FightTeam.ActiveMech`), its effects active again
//! (`FightEffectSystem.ActiveEffect`), entering the fight
//! (`FightMech.EnterFight`), and counted reborn (`AddRebirthCount`).
//!
//! After the tasks, each ally followed moves its pilots
//! (`FollowPointManager.Update`): its points behind it, rows of five, the
//! nearest first, and each pilot not yet rising flies to its point
//! (`RebirthSurvival.DoMoveSurvival`). The fight's end drops every task
//! (`OnFightExit`). `docs/rules/technology_effects.md` states the rule.

use super::*;
use crate::modifier::RebirthFollow;
use mechcore_mcfr::RebirthState;

/// `FollowPointManager.colMax`: the points in a row.
const COLUMNS: i64 = 5;
/// `FollowPointManager.survivalX` and `survivalZ`, Q32.32 metres: a pilot's
/// room in a row, and between rows.
const ROOM_X_Q32: i64 = 12 << 32;
const ROOM_Z_Q32: i64 = 5 << 32;
/// The points `CreateNearestPoints` makes at a time.
const POINTS: i64 = 50;
/// `RebirthSurvival`'s constructor: `timeMax`, `transferTimeMax` and the
/// first `R_Sum`, Q32.32, and `FOLLOW_RANGE_CHECK`.
const TIME_MAX_Q32: i64 = 3 << 32;
const TRANSFER_TIME_Q32: i64 = 0xC000_0000;
const FIRST_R_SUM_Q32: i64 = 0x6_4CCC_CCCC;
const FOLLOW_RANGE_Q32: i64 = 0x8000_0000;
/// `FPoint.PiTimes2`.
const PI_TIMES_2_Q32: i64 = 0x6_487E_D511;

/// `RebirthTask.ActionState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    None,
    Following,
    Standing,
    Rebirthing,
}

/// `RebirthSurvival.MoveState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flight {
    None,
    FollowByLerp,
    EnterTransfer,
    Transfering,
    FollowBySpeed,
}

/// A `RebirthSurvival`'s flight behind the ally it follows.
#[derive(Debug, Clone)]
struct Pilot {
    source: RebirthFollow,
    /// `followTarget`.
    target: Option<u64>,
    /// `lastTarget`.
    last_target: Option<u64>,
    flight: Flight,
    time_sum_q32: i64,
    transfer_sum_q32: i64,
    end: [i64; 3],
    r_sum_q32: i64,
    offset: [i64; 3],
}

/// A `RebirthTask`.
#[derive(Debug, Clone)]
struct Task {
    unit: u64,
    /// `taskTime`: the updates it has counted.
    time: i64,
    /// `moveTimeSum`.
    move_time: i64,
    /// `rebirthCostTime`: the source's whole seconds over a tick.
    cost: i64,
    /// `rebirthingTime`: the last of them, in ticks, in which the unit rises.
    rebirthing: i64,
    action: Action,
    /// `RebirthSurvival.worldPos`: where the unit will stand.
    position: [i64; 3],
    pilot: Option<Pilot>,
}

/// A `FollowPoint`.
#[derive(Debug, Clone, Copy)]
struct Point {
    offset_x_q32: i64,
    offset_z_q32: i64,
    world: [i64; 3],
}

/// A `FollowPointManager`: an ally followed, its points and its pilots.
#[derive(Debug, Clone)]
struct Manager {
    target: u64,
    points: Vec<Point>,
    /// `followTasks`, their units, in the order they began following.
    tasks: Vec<u64>,
}

/// `DeadRebirthController`'s tasks, counts and followed allies.
#[derive(Debug, Clone, Default)]
pub(in crate::fight) struct RebirthSystem {
    /// `rebirthCount`: the rebirths each unit has left, from its first death.
    left: BTreeMap<u64, i64>,
    /// `rebirthTasks`, in the order they started.
    tasks: Vec<Task>,
    /// `followDicts`, in the order each ally was first followed.
    managers: Vec<Manager>,
}

impl Simulation {
    /// `DeadRebirthController.PerformDeadEffect` for each unit that died
    /// this tick and is brought back, in the order they died, and then its
    /// `Update`.
    pub(in crate::fight) fn step_rebirths(&mut self, dead: &[u64]) -> Result<()> {
        for &unit in dead {
            self.start_rebirth(unit)?;
        }
        // The tasks by index, the count read again each time: a task that
        // ends is taken out under the index, and the one after it waits for
        // the next update.
        let mut index = 0;
        while index < self.rebirth.tasks.len() {
            let ready = self.ready_rebirth(index);
            let task = &mut self.rebirth.tasks[index];
            if ready {
                update_move_state(task);
                task.time += 1;
                if task.time < task.cost {
                    index += 1;
                    continue;
                }
            }
            let task = self.rebirth.tasks.remove(index);
            if ready {
                match &task.pilot {
                    None => self.rebirth_mech(&task, None)?,
                    Some(_) => {
                        if let Some(follow) = self.nearest_ally(&task) {
                            self.rebirth_mech(&task, Some(follow))?;
                        }
                    }
                }
            }
            // `DoTaskEndProcess`.
            if let Some(target) = task.pilot.as_ref().and_then(|pilot| pilot.target) {
                self.leave_follow(target, task.unit);
            }
            index += 1;
        }
        for index in 0..self.rebirth.managers.len() {
            self.update_follow_points(index)?;
        }
        Ok(())
    }

    /// `CostRebirthCount`, then `GetRebirthTask` and `StartTask`.
    fn start_rebirth(&mut self, unit: u64) -> Result<()> {
        let actor = &self.actors[&unit];
        let Some(source) = actor.placement.rebirth.clone() else {
            return Ok(());
        };
        if actor.placement.team != actor.original_team {
            return Err(Error::new(
                "a unit another side turned dies and is brought back, which is not measured",
            ));
        }
        let left = self.rebirth.left.entry(unit).or_insert(source.count);
        if *left < 1 {
            return Ok(());
        }
        *left -= 1;
        let fell = actor.recorded_position();
        let ticks = |seconds_q32: i64| math::q32_div(seconds_q32, NATIVE_LOGIC_DELTA_Q32) >> 32;
        let follows = source.follow.is_some();
        let mut task = Task {
            unit,
            time: 0,
            move_time: 0,
            cost: ticks(source.cost_seconds << 32),
            rebirthing: ticks(source.rebirthing_q32),
            action: if follows {
                Action::Following
            } else {
                Action::Standing
            },
            position: [fell.x, fell.y, fell.z],
            pilot: source.follow.map(|source| Pilot {
                source,
                target: None,
                last_target: None,
                flight: Flight::None,
                time_sum_q32: 0,
                transfer_sum_q32: 0,
                end: [0; 3],
                r_sum_q32: FIRST_R_SUM_Q32,
                offset: [0; 3],
            }),
        };
        if follows {
            if let Some(ally) = self.nearest_ally(&task) {
                self.set_follow_target(&mut task, ally);
            }
        } else {
            self.actors
                .get_mut(&unit)
                .expect("actor identity is stable")
                .rebirthing = true;
        }
        self.rebirth.tasks.push(task);
        Ok(())
    }

    /// `RebirthTask.UpdateReadyRebirth`: a unit that rises where it fell is
    /// always ready; a pilot keeps an ally alive on its side, turns to the
    /// nearest other, or, with none left, lets go and fails.
    fn ready_rebirth(&mut self, index: usize) -> bool {
        let mut task = self.rebirth.tasks[index].clone();
        let Some(pilot) = &task.pilot else {
            return true;
        };
        let team = self.actors[&task.unit].placement.team;
        let type_id = self.actors[&task.unit].rules.unit_type_id;
        let any_ally = self.actors.values().any(|actor| {
            actor.alive() && actor.placement.team == team && actor.rules.unit_type_id == type_id
        });
        let target = pilot.target;
        let mut ready = false;
        if any_ally {
            if target.is_some_and(|target| {
                let ally = &self.actors[&target];
                ally.alive() && ally.placement.team == team
            }) {
                return true;
            }
            if let Some(ally) = self.nearest_ally(&task) {
                self.set_follow_target(&mut task, ally);
                ready = true;
            }
        }
        if !ready {
            if let Some(target) = target {
                self.leave_follow(target, task.unit);
            }
            task.pilot
                .as_mut()
                .expect("a pilot was matched above")
                .target = None;
        }
        self.rebirth.tasks[index] = task;
        ready
    }

    /// `RebirthTask.TryGetNearestTeamMech`: the units of the side alive, last
    /// first, of the unit's own type and not itself, by their distance from
    /// the ally it follows or else from where it fell, the nearest first
    /// (`List.Sort`, whose comparison never answers equal).
    fn nearest_ally(&self, task: &Task) -> Option<u64> {
        let dead = &self.actors[&task.unit];
        let team = dead.placement.team;
        let type_id = dead.rules.unit_type_id;
        let origin = task
            .pilot
            .as_ref()
            .and_then(|pilot| pilot.target)
            .map_or_else(
                || dead.recorded_position(),
                |target| self.actors[&target].recorded_position(),
            );
        let mut candidates = self
            .units_in_update_order()
            .into_iter()
            .filter(|id| {
                let actor = &self.actors[id];
                actor.alive() && actor.placement.team == team
            })
            .collect::<Vec<_>>();
        candidates.reverse();
        let mut found = candidates
            .into_iter()
            .filter(|&id| id != task.unit && self.actors[&id].rules.unit_type_id == type_id)
            .map(|id| {
                let at = self.actors[&id].recorded_position();
                (
                    id,
                    distance([origin.x, origin.y, origin.z], [at.x, at.y, at.z]),
                )
            })
            .collect::<Vec<_>>();
        list_sort(&mut found, |left, right| {
            if rvo::fpoint_greater_than(left.1.saturating_sub(right.1), 0) {
                1
            } else {
                -1
            }
        });
        found.first().map(|&(id, _)| id)
    }

    /// `RebirthTask.SetFollowTarget`: the pilot leaves the ally it followed
    /// for `ally` (`FollowPointManager.RemoveFollow`, `AddFollow`).
    fn set_follow_target(&mut self, task: &mut Task, ally: u64) {
        let pilot = task.pilot.as_mut().expect("only a pilot follows");
        if pilot.target == Some(ally) {
            return;
        }
        let (old, source) = (pilot.target, pilot.source);
        if let Some(old) = old {
            self.leave_follow(old, task.unit);
        }
        let manager = self.manager(ally, &source);
        if !manager.tasks.contains(&task.unit) {
            manager.tasks.push(task.unit);
        }
        task.pilot.as_mut().expect("only a pilot follows").target = Some(ally);
    }

    /// `DeadRebirthController.GetFollowMgr`: the ally's manager, made with
    /// its first points (`CreateNearestPoints`) the first time it is asked
    /// for.
    fn manager(&mut self, target: u64, source: &RebirthFollow) -> &mut Manager {
        if let Some(index) = self
            .rebirth
            .managers
            .iter()
            .position(|manager| manager.target == target)
        {
            return &mut self.rebirth.managers[index];
        }
        self.rebirth.managers.push(Manager {
            target,
            points: nearest_points(source),
            tasks: Vec::new(),
        });
        self.rebirth
            .managers
            .last_mut()
            .expect("a manager was just added")
    }

    /// `FollowPointManager.RemoveFollow` of the ally's manager.
    fn leave_follow(&mut self, target: u64, unit: u64) {
        if let Some(manager) = self
            .rebirth
            .managers
            .iter_mut()
            .find(|manager| manager.target == target)
        {
            manager.tasks.retain(|&held| held != unit);
        }
    }

    /// `FollowPointManager.Update`: each point stands behind the ally
    /// (`FollowPoint.UpdatePoint`), and each pilot not yet rising flies to
    /// its own, the first to the nearest (`Follow`).
    fn update_follow_points(&mut self, index: usize) -> Result<()> {
        let target = self.rebirth.managers[index].target;
        let ally = &self.actors[&target];
        let at = ally.recorded_position();
        let rotation = ally.body_rotation_q32;
        let one = 1_i64 << 32;
        let forward = support_unit::turn_about_vertical(rotation, one, 0, one);
        let right = support_unit::turn_about_vertical(rotation, one, one, 0);
        for point in &mut self.rebirth.managers[index].points {
            let offset_x = math::q32_mul(forward.0, point.offset_z_q32)
                + math::q32_mul(right.0, point.offset_x_q32);
            let offset_z = math::q32_mul(forward.1, point.offset_z_q32)
                + math::q32_mul(right.1, point.offset_x_q32);
            point.world = [at.x + offset_x, at.y, at.z + offset_z];
        }
        let units = self.rebirth.managers[index].tasks.clone();
        for (slot, unit) in units.into_iter().enumerate() {
            let point = self.rebirth.managers[index].points[slot];
            let Some(task_index) = self.rebirth.tasks.iter().position(|task| task.unit == unit)
            else {
                continue;
            };
            if self.rebirth.tasks[task_index].action == Action::Rebirthing {
                continue;
            }
            self.do_move_survival(task_index, point.world, target)?;
        }
        Ok(())
    }

    /// `RebirthSurvival.DoMoveSurvival`, its pilot flying to `end`.
    fn do_move_survival(&mut self, index: usize, end: [i64; 3], target: u64) -> Result<()> {
        let team = self.actors[&self.rebirth.tasks[index].unit].original_team;
        let mut task = self.rebirth.tasks[index].clone();
        let distance_to_end = distance(end, task.position);
        let pilot = task.pilot.as_mut().expect("only a pilot follows");
        let source = pilot.source;
        if pilot.last_target == Some(target) {
            if matches!(pilot.flight, Flight::FollowBySpeed | Flight::FollowByLerp) {
                make_random_offset(pilot, self.side_random(team)?);
                pilot.time_sum_q32 = pilot.time_sum_q32.saturating_add(NATIVE_LOGIC_DELTA_Q32);
                let goal = add(end, pilot.offset);
                task.position = if pilot.flight == Flight::FollowBySpeed {
                    move_towards(
                        task.position,
                        goal,
                        math::q32_mul(source.transfer_speed_q32, NATIVE_LOGIC_DELTA_Q32),
                    )
                } else {
                    lerp(task.position, goal, source.follow_rate_q32)
                };
                if pilot.flight == Flight::FollowBySpeed
                    && math::fpoint_less_or_equal(distance(goal, task.position), FOLLOW_RANGE_Q32)
                {
                    pilot.flight = Flight::FollowByLerp;
                }
                if rvo::fpoint_greater_or_equal(pilot.time_sum_q32, TIME_MAX_Q32) {
                    pilot.time_sum_q32 = 0;
                }
            }
        } else {
            // A flight at speed to an ally near, or `DoTransfer` to one far,
            // where the pilot waits and then stands; either way its landing
            // is drawn, the point moved by the start's range times -1 or 0
            // on each axis (`GRRandom.Next(-1, 1)`, whose upper bound it
            // never draws).
            pilot.flight =
                if rvo::fpoint_greater_or_equal(distance_to_end, source.transfer_distance_q32) {
                    Flight::EnterTransfer
                } else {
                    Flight::FollowBySpeed
                };
            let random = self.side_random(team)?;
            let draws = [
                random.next_between_inclusive(-1, 0),
                random.next_between_inclusive(-1, 0),
                random.next_between_inclusive(-1, 0),
            ];
            for axis in 0..3 {
                pilot.end[axis] = end[axis].saturating_add(math::q32_mul(
                    source.start_offset_q32[axis],
                    i64::from(draws[axis]) << 32,
                ));
            }
        }
        match pilot.flight {
            Flight::EnterTransfer => {
                pilot.transfer_sum_q32 = 0;
                pilot.flight = Flight::Transfering;
            }
            Flight::Transfering => {
                pilot.transfer_sum_q32 = pilot
                    .transfer_sum_q32
                    .saturating_add(NATIVE_LOGIC_DELTA_Q32);
                if rvo::fpoint_greater_or_equal(pilot.transfer_sum_q32, TRANSFER_TIME_Q32) {
                    pilot.flight = Flight::FollowByLerp;
                    task.position = pilot.end;
                }
            }
            Flight::None | Flight::FollowByLerp | Flight::FollowBySpeed => {}
        }
        pilot.last_target = Some(target);
        self.rebirth.tasks[index] = task;
        Ok(())
    }

    /// `RebirthTask.RebirthMech`: the unit stands where its task says, facing
    /// as the ally it rises behind, if it follows one.
    fn rebirth_mech(&mut self, task: &Task, follow: Option<u64>) -> Result<()> {
        let facing = follow.map(|ally| self.actors[&ally].body_rotation_q32);
        let actor = self
            .actors
            .get_mut(&task.unit)
            .expect("actor identity is stable");
        if let Some(facing) = facing {
            actor.set_position(task.position[0], task.position[2]);
            actor.face(facing);
        }
        actor.life = actor.stats.max_life();
        actor.rebirthing = false;
        actor.rebirth_count += 1;
        // `FightMech.EnterFight`: each skill draws its interval again, and
        // its clock stands at it.
        self.draw_owner_first_intervals(FightActorRef::Unit(task.unit))?;
        self.actors
            .get_mut(&task.unit)
            .expect("actor identity is stable")
            .skills
            .ready_attack_clocks(self.step_now);
        // `FightTeam.DeactiveMech` and `ActiveMech`: it updates after every
        // unit of its side, as one joining it does.
        self.joins_side_last(task.unit);
        self.plant(task.unit);
        self.active_effect(task.unit)?;
        Ok(())
    }

    /// Whether a unit of `team` is rising: a side waiting on one stands.
    pub(in crate::fight) fn rebirthing_on(&self, team: u32) -> bool {
        self.rebirth.tasks.iter().any(|task| {
            let actor = &self.actors[&task.unit];
            actor.rebirthing && actor.placement.team == team
        })
    }

    /// `FightEffectSystem.ActiveEffect`: each provider of the unit's
    /// technologies takes it, as it lands or rises again.
    pub(in crate::fight) fn active_effect(&mut self, unit: u64) -> Result<()> {
        self.activate_interception(unit);
        self.add_stealth_unit(unit);
        self.add_group_unit(unit);
        self.add_siege_unit(unit)?;
        self.activate_reactive_armor(unit);
        self.add_wreckage_unit(unit);
        Ok(())
    }

    /// Every unit waiting to be reborn, as a recording holds it.
    pub(in crate::fight) fn rebirth_states(&self) -> Vec<RebirthState> {
        let mut states = self
            .rebirth
            .tasks
            .iter()
            .map(|task| RebirthState {
                unit_id: task.unit,
                position: QVec3 {
                    x: task.position[0],
                    y: task.position[1],
                    z: task.position[2],
                },
            })
            .collect::<Vec<_>>();
        states.sort_by_key(|state| state.unit_id);
        states
    }

    /// `DeadRebirthController.OnFightExit`: every task and follow goes.
    pub(in crate::fight) fn end_rebirths_as_the_fight_ends(&mut self) {
        self.rebirth.tasks.clear();
        self.rebirth.managers.clear();
    }
}

/// `RebirthTask.UpdateMoveState`: the last of the wait is spent rising.
fn update_move_state(task: &mut Task) {
    if task.action == Action::None {
        return;
    }
    task.move_time += 1;
    task.action = if task.move_time >= task.cost {
        Action::None
    } else if task.rebirthing < task.cost - task.move_time {
        if task.pilot.is_some() {
            Action::Following
        } else {
            Action::Standing
        }
    } else {
        Action::Rebirthing
    };
}

/// `FollowPointManager.CreateNearestPoints`: fifty points
/// (`CreateFollowPoint`), rows of five behind the ally, a row's middle
/// point the source's distance behind it, then sorted by their distance
/// from it, the nearest first.
fn nearest_points(source: &RebirthFollow) -> Vec<Point> {
    let step_x = ROOM_X_Q32 + source.interval_x_q32;
    let step_z = ROOM_Z_Q32 + source.interval_z_q32;
    // `intervalFormMechCenterX`: the middle of a row of five.
    let center_x = 2 * ROOM_X_Q32 + 2 * source.interval_x_q32;
    let mut points = (0..POINTS)
        .map(|index| {
            let (row, column) = (index / COLUMNS, index % COLUMNS);
            Point {
                offset_x_q32: column * step_x - center_x,
                offset_z_q32: -row * step_z - source.interval_from_center_z_q32,
                world: [0; 3],
            }
        })
        .collect::<Vec<_>>();
    let length = |point: &Point| distance([point.offset_x_q32, 0, point.offset_z_q32], [0; 3]);
    list_sort(&mut points, |left, right| {
        if rvo::fpoint_greater_than(length(left).saturating_sub(length(right)), 0) {
            1
        } else {
            -1
        }
    });
    points
}

/// `RebirthSurvival.MakeRandomOffest`: the swing's angle moves on, and each
/// time it comes round the offset is drawn again, each axis its range at
/// the swing times -1 or 0.
fn make_random_offset(pilot: &mut Pilot, random: &mut random::GrRandom) {
    pilot.r_sum_q32 = pilot.r_sum_q32.saturating_add(pilot.source.per_r_q32);
    let swing = math::fpcs_cos_fastest(pilot.r_sum_q32);
    let range = pilot
        .source
        .random_offset_q32
        .map(|axis| math::q32_mul(swing, axis));
    if rvo::fpoint_greater_or_equal(pilot.r_sum_q32, PI_TIMES_2_Q32) {
        pilot.r_sum_q32 = pilot.r_sum_q32.saturating_sub(PI_TIMES_2_Q32);
        for axis in 0..3 {
            let draw = random.next_between_inclusive(-1, 0);
            pilot.offset[axis] = math::q32_mul(range[axis], i64::from(draw) << 32);
        }
    }
}

fn add(left: [i64; 3], right: [i64; 3]) -> [i64; 3] {
    [0, 1, 2].map(|axis| left[axis].saturating_add(right[axis]))
}

/// `FVector3.Distance`.
fn distance(left: [i64; 3], right: [i64; 3]) -> i64 {
    math::native_q32_magnitude_3d(
        left[0].saturating_sub(right[0]),
        left[1].saturating_sub(right[1]),
        left[2].saturating_sub(right[2]),
    )
}

/// `FVector3.Lerp`: the amount held to 0 and 1, then each axis the start's
/// share and the end's.
fn lerp(start: [i64; 3], end: [i64; 3], amount_q32: i64) -> [i64; 3] {
    let amount = amount_q32.clamp(0, 1 << 32);
    let rest = (1_i64 << 32) - amount;
    [0, 1, 2].map(|axis| {
        math::q32_mul(start[axis], rest).saturating_add(math::q32_mul(end[axis], amount))
    })
}

/// `FVector3.MoveTowards`.
fn move_towards(current: [i64; 3], target: [i64; 3], maximum_q32: i64) -> [i64; 3] {
    let delta = [0, 1, 2].map(|axis| target[axis].saturating_sub(current[axis]));
    let square = delta.iter().fold(0_i64, |sum, &axis| {
        sum.saturating_add(math::q32_mul(axis, axis))
    });
    if square.abs() <= 43
        || (maximum_q32 >= 0
            && math::fpoint_less_or_equal(square, math::q32_mul(maximum_q32, maximum_q32)))
    {
        return target;
    }
    let scale = math::q32_div(maximum_q32, math::fpcs_sqrt_fastest(square));
    [0, 1, 2].map(|axis| current[axis].saturating_add(math::q32_mul(delta[axis], scale)))
}

/// `List.Sort` with a comparison (`ArraySortHelper.IntrospectiveSort`): a
/// run of sixteen or fewer by insertion, a longer one partitioned about the
/// median of its first, middle and last. It is not stable, which a
/// comparison that never answers equal makes visible.
fn list_sort<T: Copy>(items: &mut [T], compare: impl Fn(&T, &T) -> i32) {
    if items.len() < 2 {
        return;
    }
    let depth = 2 * floor_log2(items.len());
    intro_sort(items, 0, items.len() - 1, depth, &compare);
}

fn floor_log2(mut value: usize) -> usize {
    let mut result = 0;
    while value >= 1 {
        result += 1;
        value /= 2;
    }
    result
}

fn intro_sort<T: Copy>(
    items: &mut [T],
    low: usize,
    mut high: usize,
    mut depth: usize,
    compare: &impl Fn(&T, &T) -> i32,
) {
    while high > low {
        let size = high - low + 1;
        if size <= 16 {
            match size {
                1 => {}
                2 => swap_if_greater(items, compare, low, high),
                3 => {
                    swap_if_greater(items, compare, low, high - 1);
                    swap_if_greater(items, compare, low, high);
                    swap_if_greater(items, compare, high - 1, high);
                }
                _ => insertion_sort(items, low, high, compare),
            }
            return;
        }
        if depth == 0 {
            heap_sort(items, low, high, compare);
            return;
        }
        depth -= 1;
        let pivot = pick_pivot_and_partition(items, low, high, compare);
        intro_sort(items, pivot + 1, high, depth, compare);
        if pivot == 0 {
            return;
        }
        high = pivot - 1;
    }
}

fn swap_if_greater<T: Copy>(
    items: &mut [T],
    compare: &impl Fn(&T, &T) -> i32,
    left: usize,
    right: usize,
) {
    if left != right && compare(&items[left], &items[right]) > 0 {
        items.swap(left, right);
    }
}

fn pick_pivot_and_partition<T: Copy>(
    items: &mut [T],
    low: usize,
    high: usize,
    compare: &impl Fn(&T, &T) -> i32,
) -> usize {
    let middle = low + (high - low) / 2;
    swap_if_greater(items, compare, low, middle);
    swap_if_greater(items, compare, low, high);
    swap_if_greater(items, compare, middle, high);
    let pivot = items[middle];
    items.swap(middle, high - 1);
    let (mut left, mut right) = (low, high - 1);
    while left < right {
        left += 1;
        while compare(&items[left], &pivot) < 0 {
            left += 1;
        }
        right -= 1;
        while compare(&pivot, &items[right]) < 0 {
            right -= 1;
        }
        if left >= right {
            break;
        }
        items.swap(left, right);
    }
    if left != high - 1 {
        items.swap(left, high - 1);
    }
    left
}

fn insertion_sort<T: Copy>(
    items: &mut [T],
    low: usize,
    high: usize,
    compare: &impl Fn(&T, &T) -> i32,
) {
    for index in low..high {
        let mut at = index;
        let item = items[index + 1];
        loop {
            if compare(&item, &items[at]) >= 0 {
                items[at + 1] = item;
                break;
            }
            items[at + 1] = items[at];
            if at == low {
                items[at] = item;
                break;
            }
            at -= 1;
        }
    }
}

fn heap_sort<T: Copy>(items: &mut [T], low: usize, high: usize, compare: &impl Fn(&T, &T) -> i32) {
    let count = high - low + 1;
    for index in (1..=count / 2).rev() {
        down_heap(items, index, count, low, compare);
    }
    for index in (2..=count).rev() {
        items.swap(low, low + index - 1);
        down_heap(items, 1, index - 1, low, compare);
    }
}

fn down_heap<T: Copy>(
    items: &mut [T],
    mut index: usize,
    count: usize,
    low: usize,
    compare: &impl Fn(&T, &T) -> i32,
) {
    let item = items[low + index - 1];
    while index <= count / 2 {
        let mut child = 2 * index;
        if child < count && compare(&items[low + child - 1], &items[low + child]) < 0 {
            child += 1;
        }
        if compare(&item, &items[low + child - 1]) >= 0 {
            break;
        }
        items[low + index - 1] = items[low + child - 1];
        index = child;
    }
    items[low + index - 1] = item;
}
