mod check;
mod construction;
mod group;
mod perform;

pub(in crate::fight) use perform::Launch;

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) struct PendingRelease {
    pub(in crate::fight) step: u64,
    pub(in crate::fight) target: FightActorRef,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::fight) struct PendingProjectileRelease {
    pub(in crate::fight) step: u64,
    pub(in crate::fight) target_kind: ObjectKind,
    pub(in crate::fight) target: u64,
    pub(in crate::fight) target_x_q32: i64,
    pub(in crate::fight) target_z_q32: i64,
    pub(in crate::fight) weapon_index: usize,
}

/// The skill's state, as `SkillStateController` holds it.
///
/// Every combination of phase, wind-up, backswing and cooling the kernel
/// used to carry apart is one of these, which the regression fights were
/// counted against before they were folded: a wind-up and a backswing never
/// run together, and neither does anything else with a cooling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) enum SkillState {
    /// `SkillIdleState`, with the step a finished attack's recovery runs
    /// until, if one still runs: the idle state waits it out before it
    /// starts another.
    Idle { ready_step: Option<u64> },
    /// `SkillPrepareState`, until its prepare time is over.
    Prepare { finish_step: u64 },
    /// `SkillAttackState`, and where in its blow it is.
    Attack(Blow),
    /// `SkillCoolingState`: when it began, and what the weapons still name.
    Cooling {
        started: u64,
        candidate: Option<FightActorRef>,
    },
    /// `SkillReloadingState`: a skill that fires from a magazine and has
    /// emptied it, until the step its reload is over.
    Reloading { finish_step: u64 },
}

/// Where `SkillAttackController` is in a blow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) enum Blow {
    /// No phase runs: the blow has not begun, or the last one is over.
    Waiting,
    /// The wait before the blow lands, and what it will land on.
    Before(PendingRelease),
    /// The backswing, until its last step.
    After { finish_step: u64 },
}

/// Whether an actor's update goes on to its next part or ends here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) enum Flow {
    Next,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) enum FightSkillPhase {
    Idle,
    Prepare { finish_step: u64 },
    Attack,
}

/// `FightSkill`: the lock and what the weapons fire at, the state the skill
/// is in, and the attack it is making.

#[derive(Debug, Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the flags are pieces of the skill's state the kernel still carries apart; the attack state's own enum replaces them"
)]
pub(in crate::fight) struct Skill {
    pub(in crate::fight) weapon_rotations_q32: Vec<i64>,
    pub(in crate::fight) next_attack_step: u64,
    /// The interval this cycle was scheduled with, in logic ticks: the
    /// description plus the stagger drawn for it. The build keeps the same
    /// thing in `FightSkill.attackInterval` and answers it from
    /// `GetCurrentAttackInterval`, and a recording carries it so the two can
    /// be compared. Before a unit's first attack it is the description with
    /// the draw its deployment took.
    pub(in crate::fight) current_attack_interval: u64,
    /// What the mech's body is directed at: the target its search found, which
    /// it moves toward and which a unit with a body keeps facing while it
    /// attacks. A recording carries it as `mech_lock_target`.
    ///
    /// It is not always what the weapons fire at: [`Actor::attack_target`] is,
    /// and the two part company when an enemy construction stands in the line
    /// of fire.
    pub(in crate::fight) lock_target: Option<FightActorRef>,
    /// The enemy construction in the line of fire, and the lock it was found
    /// for.
    ///
    /// `FightSkill.SearchAttackTarget` asks `WallConstructionTargetChecker`
    /// wherever the skill asks what to fire at, and hands the block to the
    /// weapons while the mech keeps its lock. The pairing is what keeps this honest: once
    /// `lock_target` is anything but the lock it was found for, the block no
    /// longer answers, without anyone having to clear it.
    pub(in crate::fight) in_the_way: Option<(u64, FightActorRef)>,
    pub(in crate::fight) lock_is_terminal_handoff: bool,
    pub(in crate::fight) search_target_time: i32,
    pub(in crate::fight) searched_this_tick: bool,
    /// Which `SkillStateController` state the skill is in, with what that
    /// state carries.
    pub(in crate::fight) state: SkillState,
    pub(in crate::fight) group_skill_targets: Vec<Option<u64>>,
    /// The enemy construction in each grouped slot's line of fire, with the
    /// unit that slot was allocated.
    ///
    /// The same pairing as [`Actor::in_the_way`], slot by slot: a Wraith's four
    /// slots each take the block standing between it and the unit they were
    /// given, and a slot given another unit no longer answers with it.
    pub(in crate::fight) group_in_the_way: Vec<Option<(u64, u64)>>,
    pub(in crate::fight) group_skill_next_attack_steps: Vec<u64>,
    pub(in crate::fight) group_skill_prepare_ready_steps: Vec<u64>,
    pub(in crate::fight) group_pending_releases: Vec<(usize, PendingRelease)>,
    pub(in crate::fight) projectile_pending_releases: Vec<PendingProjectileRelease>,
    pub(in crate::fight) laser_attack_count: usize,
    pub(in crate::fight) retarget_after_own_direct_kill: bool,
}

impl Skill {
    /// A skill entering the fight: idle, no lock, nothing scheduled.
    pub(in crate::fight) fn new(weapon_rotations_q32: Vec<i64>, group_skill_count: usize) -> Self {
        Self {
            weapon_rotations_q32,
            next_attack_step: 0,
            current_attack_interval: 0,
            lock_target: None,
            in_the_way: None,
            lock_is_terminal_handoff: false,
            // FightSkill owns a second SearchTargetController. FightPrepareState
            // replaces this constructor value with the presearch batch ordinal.
            search_target_time: SEARCH_TARGET_RESET_TICKS,
            searched_this_tick: false,
            state: SkillState::Idle { ready_step: None },
            group_skill_targets: vec![None; group_skill_count],
            group_in_the_way: vec![None; group_skill_count],
            group_skill_next_attack_steps: vec![0; group_skill_count],
            group_skill_prepare_ready_steps: vec![0; group_skill_count],
            group_pending_releases: Vec::new(),
            projectile_pending_releases: Vec::new(),
            laser_attack_count: 0,
            retarget_after_own_direct_kill: false,
        }
    }

    /// `SkillAttackState.TryPerformAttack` once the interval is up: draws the
    /// next interval from the team's stream, schedules the next attack after
    /// it, and winds up a blow that lands after the attack point.
    ///
    /// The draw is taken only when the skill has a random offset, so a skill
    /// without one leaves the stream to the next owner.
    pub(in crate::fight) fn schedule_blow(
        &mut self,
        random: &mut GrRandom,
        step: u64,
        interval_steps: u64,
        offset_steps: u64,
        attack_point_steps: u64,
        target: FightActorRef,
    ) {
        let sample = if offset_steps == 0 {
            0
        } else {
            i64::from(random.next_in_range(i32::try_from(offset_steps).unwrap_or(i32::MAX)))
        };
        let sampled = i64::try_from(interval_steps)
            .unwrap_or(i64::MAX)
            .saturating_add(sample)
            .max(1)
            .cast_unsigned();
        self.next_attack_step = step.saturating_add(sampled);
        self.current_attack_interval = sampled;
        self.set_pending(Some(PendingRelease {
            step: step.saturating_add(attack_point_steps),
            target,
        }));
    }

    /// The coarse phase: idle (a cooling reads idle), preparing, attacking.
    pub(in crate::fight) const fn phase(&self) -> FightSkillPhase {
        match self.state {
            SkillState::Idle { .. } | SkillState::Cooling { .. } | SkillState::Reloading { .. } => {
                FightSkillPhase::Idle
            }
            SkillState::Prepare { finish_step } => FightSkillPhase::Prepare { finish_step },
            SkillState::Attack(_) => FightSkillPhase::Attack,
        }
    }

    /// Moves to a coarse phase, carrying a running backswing across, as the
    /// fields this replaces did.
    pub(in crate::fight) fn set_phase(&mut self, phase: FightSkillPhase) {
        self.state = match (phase, self.state) {
            (FightSkillPhase::Idle, SkillState::Attack(Blow::After { finish_step })) => {
                SkillState::Idle {
                    ready_step: Some(finish_step),
                }
            }
            (
                FightSkillPhase::Idle,
                state @ (SkillState::Idle { .. } | SkillState::Cooling { .. }),
            )
            | (FightSkillPhase::Attack, state @ SkillState::Attack(_)) => state,
            (FightSkillPhase::Idle, _) => SkillState::Idle { ready_step: None },
            (FightSkillPhase::Prepare { finish_step }, state) => {
                debug_assert!(
                    !matches!(
                        state,
                        SkillState::Idle {
                            ready_step: Some(_)
                        }
                    ),
                    "a skill prepares while recovering: {state:?}"
                );
                SkillState::Prepare { finish_step }
            }
            (
                FightSkillPhase::Attack,
                SkillState::Idle {
                    ready_step: Some(finish_step),
                },
            ) => SkillState::Attack(Blow::After { finish_step }),
            (FightSkillPhase::Attack, state) => {
                debug_assert!(
                    !matches!(state, SkillState::Cooling { .. }),
                    "a cooling skill attacks: {state:?}"
                );
                SkillState::Attack(Blow::Waiting)
            }
        };
    }

    /// The blow being wound up, if one is.
    pub(in crate::fight) const fn pending(&self) -> Option<PendingRelease> {
        match self.state {
            SkillState::Attack(Blow::Before(pending)) => Some(pending),
            _ => None,
        }
    }

    pub(in crate::fight) const fn pending_mut(&mut self) -> Option<&mut PendingRelease> {
        match &mut self.state {
            SkillState::Attack(Blow::Before(pending)) => Some(pending),
            _ => None,
        }
    }

    pub(in crate::fight) fn set_pending(&mut self, pending: Option<PendingRelease>) {
        match (pending, self.state) {
            (Some(pending), SkillState::Attack(_)) => {
                self.state = SkillState::Attack(Blow::Before(pending));
            }
            (Some(_), state) => panic!("a blow is wound up outside an attack: {state:?}"),
            (None, SkillState::Attack(Blow::Before(_))) => {
                self.state = SkillState::Attack(Blow::Waiting);
            }
            (None, _) => {}
        }
    }

    /// The last step of a running backswing, in an attack or waited out in
    /// idle.
    pub(in crate::fight) const fn backswing_finish_step(&self) -> Option<u64> {
        match self.state {
            SkillState::Attack(Blow::After { finish_step })
            | SkillState::Idle {
                ready_step: Some(finish_step),
            } => Some(finish_step),
            _ => None,
        }
    }

    pub(in crate::fight) fn set_backswing_finish_step(&mut self, finish_step: Option<u64>) {
        self.state = match (finish_step, self.state) {
            (Some(finish_step), SkillState::Attack(_)) => {
                SkillState::Attack(Blow::After { finish_step })
            }
            (Some(finish_step), SkillState::Idle { .. }) => SkillState::Idle {
                ready_step: Some(finish_step),
            },
            (Some(_), state) => panic!("a backswing runs outside an attack or idle: {state:?}"),
            (None, SkillState::Attack(Blow::After { .. })) => SkillState::Attack(Blow::Waiting),
            (None, SkillState::Idle { .. }) => SkillState::Idle { ready_step: None },
            (None, state) => state,
        };
    }

    /// When the cooling began, and what the weapons name through it.
    pub(in crate::fight) const fn cooling(&self) -> Option<(u64, Option<FightActorRef>)> {
        match self.state {
            SkillState::Cooling { started, candidate } => Some((started, candidate)),
            _ => None,
        }
    }

    /// Starts, updates or ends a cooling; ending it leaves the skill idle.
    pub(in crate::fight) fn set_cooling(&mut self, cooling: Option<(u64, Option<FightActorRef>)>) {
        self.state = match (cooling, self.state) {
            (Some((started, candidate)), _) => SkillState::Cooling { started, candidate },
            (None, SkillState::Cooling { .. }) => SkillState::Idle { ready_step: None },
            (None, state) => state,
        };
    }
}

impl Skill {
    /// What this actor's weapons fire at: the construction in the way if one
    /// stands there for the current lock, and the lock itself otherwise.
    ///
    /// Range, attack angle, release and the question of whether the target
    /// is still alive are all asked of this. Where to move and where a body
    /// faces are asked of `lock_target`.
    pub(in crate::fight) fn attack_target(&self) -> Option<FightActorRef> {
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
    pub(in crate::fight) fn drop_lock(&mut self) {
        self.lock_target = None;
        self.group_skill_targets.fill(None);
        self.group_in_the_way.fill(None);
        self.group_skill_next_attack_steps.fill(0);
        self.group_skill_prepare_ready_steps.fill(0);
        self.group_pending_releases.clear();
    }

    /// What one grouped slot fires at: the construction in its way if one
    /// stands there for the unit it was allocated, and that unit otherwise.
    pub(in crate::fight) fn group_attack_target(&self, slot: usize) -> Option<FightActorRef> {
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
    pub(in crate::fight) fn mechanical_attack_target(&self) -> Option<FightActorRef> {
        self.group_attack_target(0)
            .or_else(|| {
                (0..self.group_skill_targets.len())
                    .rev()
                    .find_map(|slot| self.group_attack_target(slot))
            })
            .or(self.attack_target())
    }

    pub(in crate::fight) fn mechanical_lock_target(&self) -> Option<FightActorRef> {
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
}

impl Simulation {
    /// `SkillCoolingState`: holds a unit whose attack has finished idle and
    /// without a lock for its cooling time, its weapon on what it last fired
    /// at, then clears the weapon and lets the idle skill search. Answers
    /// whether it held. Only `finish_attack` starts a cooling.
    ///
    /// A Marksman's cooling is 0.2 seconds, four steps: its skill reads
    /// cooling for four ticks and idle, emptied, for one more, and prepares
    /// on the next — after a kill whose replacement is out of reach
    /// (`crawlers-vs-marksman.yaml`) as after a fallen block
    /// (`wall-line-width.yaml`).
    pub(in crate::fight) fn hold_through_cooling(
        &mut self,
        actor_id: u64,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<bool> {
        let Some((started, held)) = self.actors[&actor_id].skill.cooling() else {
            return Ok(false);
        };
        let cooling_steps =
            native_time_units_to_steps(self.actors[&actor_id].rules.attack.cooling_time_units());
        if step > started.saturating_add(cooling_steps) {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.skill.set_cooling(None);
            return Ok(false);
        }
        let candidate = if step < started.saturating_add(cooling_steps) {
            match held {
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
        actor.skill.lock_target = None;
        actor.skill.set_cooling(Some((started, candidate)));
        actor.motion.state = MotionState::Idle;
        actor.skill.search_target_time = 0;
        actor.motion.next_target_x_q32 = actor.x_q32;
        actor.motion.next_target_z_q32 = actor.z_q32;
        actor.motion.next_speed_q32 = 0;
        actor.motion.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
        Ok(true)
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the search timer's update, to be split with the attack state"
    )]
    pub(in crate::fight) fn update_fight_skill_target_search(
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
            .skill
            .mechanical_attack_target()
            .and_then(|target| self.fight_actor(target));
        let target_alive = target.is_some_and(|target| target.alive && target.targetable);
        let target_died_during_tick =
            target.is_some_and(|target| target.query_alive && !target.alive);
        if !target_alive
            && actor
                .skill
                .backswing_finish_step()
                .is_some_and(|finish_step| finish_step >= step)
        {
            let quick_switch_interval_due =
                actor.rules.attack.quick_switch_target && step > actor.skill.next_attack_step;
            if !quick_switch_interval_due {
                return Ok(());
            }
            self.actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .skill
                .set_backswing_finish_step(None);
        }
        let actor = &self.actors[&actor_id];
        if (matches!(actor.skill.phase(), FightSkillPhase::Prepare { .. })
            || actor.skill.phase() == FightSkillPhase::Attack)
            && (!actor.rules.attack.quick_switch_target || target_alive)
        {
            return Ok(());
        }
        if target_alive && self.actors[&actor_id].skill.search_target_time > 0 {
            self.actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .skill
                .search_target_time -= 1;
            return Ok(());
        }

        self.actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .skill
            .searched_this_tick = true;

        // With no enemy unit left, the idle search is the match's end: the
        // selector would answer the defeated team's `FightCrystal`, and
        // `Simulation::step` stands in for that tick (the terminal handoff).
        let source_team = self.actors[&actor_id].placement.team;
        let match_over = !self
            .actors
            .values()
            .any(|candidate| candidate.placement.team != source_team && candidate.alive());
        let mut selected_candidate = if match_over {
            None
        } else {
            self.select_normal_target_with_order(
                actor_id,
                target_search_order,
                target_died_during_tick,
            )
            .map_err(|error| Error::new(format!("logic step {step} actor {actor_id}: {error}")))?
        };
        if !match_over
            && !target_died_during_tick
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
            actor.skill.lock_target = Some(FightActorRef::Building(building_id));
            actor.skill.lock_is_terminal_handoff = false;
            actor.skill.search_target_time = SEARCH_TARGET_RESET_TICKS;
            actor.skill.set_phase(FightSkillPhase::Idle);
            actor.skill.retarget_after_own_direct_kill = false;
            actor.skill.laser_attack_count = 0;
            return Ok(());
        }
        let selected = selected_candidate;
        let actor = &self.actors[&actor_id];
        let quick_idle_retains_attackable_target = actor.rules.attack.quick_switch_target
            && actor.skill.phase() == FightSkillPhase::Idle
            && step >= actor.skill.next_attack_step
            && actor
                .skill
                .attack_target()
                .is_some_and(|target_id| self.target_in_attack_area(actor_id, target_id));
        let selected = if target_alive
            && (!actor.rules.attack.quick_switch_target || quick_idle_retains_attackable_target)
            && actor.motion.state == MotionState::Attacking
            && !actor.motion.attack_hold_fire
            && actor.skill.pending().is_none()
            && actor.skill.backswing_finish_step().is_none()
            && selected != actor.skill.lock_target
        {
            // An attacking unit keeps the lock it has. What its weapons fire
            // at is asked again below, so a construction still in the way is
            // handed back to them rather than written into the lock.
            actor.skill.lock_target
        } else {
            selected
        };
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        actor.skill.lock_is_terminal_handoff = false;
        if actor.skill.attack_target() != selected {
            actor.skill.laser_attack_count = 0;
        }
        actor.skill.lock_target = selected;
        actor.skill.search_target_time = SEARCH_TARGET_RESET_TICKS;
        actor.skill.retarget_after_own_direct_kill = false;
        self.search_attack_target(actor_id);
        Ok(())
    }

    #[cfg(test)]
    pub(in crate::fight) fn step_actor(
        &mut self,
        actor_id: u64,
        step: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        self.refresh_target_query_snapshot();
        let target_search_order = self.target_search_order();
        self.step_actor_with_target_order(actor_id, step, &target_search_order, events)
    }

    /// One unit's update, in the order `FightMech` runs it: its skill, then
    /// its motion.
    ///
    /// The skill's part: the checks its state runs (`SkillCoolingState`,
    /// `SkillPrepareState` and `SkillAttackState` asking `CheckAttackable`), a
    /// grouped skill's slots, the ends of a burst, the idle search and its
    /// timer, the state moving on as its time is up, and what is due to be
    /// performed. The motion's part, `update_motion`, is where a target in range
    /// starts the skill (`SkillIdleState.TryStartAttack`) and one out of range
    /// is left or walked towards. Each part says whether the update goes on.
    pub(in crate::fight) fn step_actor_with_target_order(
        &mut self,
        actor_id: u64,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let backswing_just_finished = self.actors[&actor_id]
            .skill
            .backswing_finish_step()
            .is_some_and(|finish_step| finish_step < step);
        if !self.actors[&actor_id].alive() {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.exit_fight_on_death();
            return Ok(());
        }
        if let Flow::Done = self.update_skill_checks(actor_id, step, target_search_order)? {
            return Ok(());
        }
        if let Flow::Done = self.release_terminal_handoff(actor_id) {
            return Ok(());
        }
        self.update_group_skill_targets(actor_id, step, target_search_order)?;
        self.refresh_group_walls(actor_id);
        if let Flow::Done = self.finish_laser_own_kill(actor_id) {
            return Ok(());
        }
        self.start_bodyless_skill(actor_id, step);
        let quick_switch_backswing_due = {
            let actor = &self.actors[&actor_id];
            actor.rules.attack.quick_switch_target
                && actor
                    .skill
                    .backswing_finish_step()
                    .is_some_and(|finish_step| finish_step >= step)
                && step > actor.skill.next_attack_step
        };
        let quick_switch_dead_backswing_due = quick_switch_backswing_due
            && self.actors[&actor_id]
                .skill
                .attack_target()
                .is_some_and(|target| !self.fight_actor_is_alive(target));
        if let Flow::Done = self.projectile_burst_lost_target(actor_id, step, events)? {
            return Ok(());
        }
        // `SkillIdleState.TryPerform` reaches `SearchAttackTarget` on every
        // update the skill is idle with a lock.
        if self.actors[&actor_id].skill.phase() == FightSkillPhase::Idle {
            self.search_attack_target(actor_id);
        }
        self.update_fight_skill_target_search(actor_id, step, target_search_order)?;
        let prepare_finished = self.advance_skill_state(
            actor_id,
            step,
            backswing_just_finished,
            quick_switch_backswing_due,
            quick_switch_dead_backswing_due,
        );
        if let Flow::Done = self.retarget_pending_blow(actor_id) {
            return Ok(());
        }
        let (flow, attack_point_rejected) = self.perform_due_blows(actor_id, step, events)?;
        if let Flow::Done = flow {
            return Ok(());
        }
        self.update_motion(
            actor_id,
            step,
            events,
            backswing_just_finished,
            prepare_finished,
            attack_point_rejected,
        )
    }

    /// `SkillIdleState.TryStartAttack` and `SkillAttackState.TryPerformAttack`
    /// for a unit whose motion is attacking: an idle skill enters its prepare
    /// or attack state, and an attacking one begins the next blow's wait once
    /// its interval is up.
    pub(in crate::fight) fn try_start_attack(
        &mut self,
        actor_id: u64,
        step: u64,
        target: FightActorRef,
        entered_attack: bool,
        in_attack_angle: bool,
        prepare_finished: bool,
    ) {
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let mut entered_skill_phase = false;
        // `SkillIdleState.TryStartAttack` enters the attack or prepare
        // state on the tick the unit comes into its attack area,
        // which is the tick its motion starts attacking; the state
        // is not updated until the tick after, where the first
        // blow's wait begins.
        // A bodyless unit whose facing its motion is still correcting
        // enters it all the same; only the blow waits for the facing.
        if in_attack_angle
            && actor.skill.pending().is_none()
            && actor.skill.backswing_finish_step().is_none()
            && actor.skill.phase() == FightSkillPhase::Idle
        {
            let prepare_steps = native_time_units_to_steps(actor.rules.attack.prepare_time_units());
            // A grouped skill's core is a `GroupedSkillAttackBehaviour`,
            // not a `FightSkill`, and prepares from the tick after its
            // motion starts attacking; nothing has captured its states
            // yet to say why.
            let from = if entered_attack && !actor.skill.group_skill_targets.is_empty() {
                step + 1
            } else {
                step
            };
            actor.skill.set_phase(if prepare_steps == 0 {
                FightSkillPhase::Attack
            } else {
                FightSkillPhase::Prepare {
                    finish_step: from.saturating_add(prepare_steps),
                }
            });
            entered_skill_phase = prepare_steps > 0 || entered_attack;
        }
        if (!entered_attack || actor.rules.attack.quick_switch_target)
            && !actor.motion.attack_hold_fire
            && in_attack_angle
            && actor.skill.pending().is_none()
            && actor.skill.backswing_finish_step().is_none()
            && actor.skill.phase() == FightSkillPhase::Attack
            && !entered_skill_phase
            // A state is not updated on the tick it is entered: the
            // first blow waits for the tick after the prepare ends.
            && !prepare_finished
            && step >= actor.skill.next_attack_step
        {
            let interval_steps = native_time_units_to_steps(actor.stats.attack_interval());
            let offset_steps =
                native_time_units_to_steps(actor.rules.attack.interval_offset_time_units());
            let attack_point_steps =
                native_time_units_to_steps(actor.rules.attack.attack_point_time_units());
            let random = self
                .team_random
                .get_mut(&actor.placement.team)
                .expect("every actor team owns one attack random stream");
            actor.skill.schedule_blow(
                random,
                step,
                interval_steps,
                offset_steps,
                attack_point_steps,
                target,
            );
        }
    }

    /// The checks the skill's state runs at the start of its update:
    /// `SkillCoolingState` holding, `SkillPrepareState` and `SkillAttackState`
    /// asking `CheckAttackable`.
    fn update_skill_checks(
        &mut self,
        actor_id: u64,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<Flow> {
        if self.hold_through_cooling(actor_id, step, target_search_order)? {
            return Ok(Flow::Done);
        }
        // `SkillPrepareState.Update` asks `SkillAttackableChecker.Check` on
        // every update; a failed check leaves the skill idle with its targets
        // cleared. The check decides the attack target again on the way,
        // which is where a block coming into the line of fire is met.
        if matches!(
            self.actors[&actor_id].skill.phase(),
            FightSkillPhase::Prepare { .. }
        ) && !self.check_attackable(actor_id, target_search_order)?
        {
            self.enter_idle_clearing_targets(actor_id);
            return Ok(Flow::Done);
        }
        // `SkillAttackState.Update` asks `CheckAttackable` between two blows,
        // and a failed check finishes the attack.
        if self.between_blows(actor_id, step) {
            if !self.attack_state_check_attackable(actor_id, target_search_order)? {
                self.finish_attack(actor_id, step);
                return Ok(Flow::Done);
            }
            // The blow being wound up is performed on the skill's attack
            // target, which the check may just have changed.
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            if actor.skill.group_skill_targets.is_empty()
                && let Some(target) = actor.skill.attack_target()
                && let Some(pending) = actor.skill.pending_mut()
            {
                pending.target = target;
            }
        }
        Ok(Flow::Next)
    }

    /// The match's end hands a unit one of the defeated team's towers
    /// (`FightCrystal.IsTower`: its `EnergyTower` or `ResearchCenter`) for one
    /// tick, and takes it back the tick after.
    fn release_terminal_handoff(&mut self, actor_id: u64) -> Flow {
        if matches!(
            self.actors[&actor_id].skill.lock_target,
            Some(FightActorRef::Building(_))
        ) && self.actors[&actor_id].skill.lock_is_terminal_handoff
        {
            // The native terminal handoff exposes one of the defeated team's
            // towers for one tick. The following update consumes the already
            // published displacement, then `FightCoreSystem.TryDstroyTower` clears
            // the transient lock before the terminal snapshot is written.
            let clear_velocity = self.terminal_drain_pending;
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.motion.state = MotionState::Idle;
            actor.skill.drop_lock();
            actor.skill.lock_is_terminal_handoff = false;
            actor.skill.set_phase(FightSkillPhase::Idle);
            if clear_velocity {
                actor.motion.current_velocity_x_q32 = 0;
                actor.motion.current_velocity_z_q32 = 0;
            }
            actor.motion.next_target_x_q32 = actor.x_q32;
            actor.motion.next_target_z_q32 = actor.z_q32;
            actor.motion.next_speed_q32 = 0;
            actor.motion.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
            return Flow::Done;
        }
        Flow::Next
    }

    /// A laser whose own beam killed its target ends its attack the update
    /// after.
    fn finish_laser_own_kill(&mut self, actor_id: u64) -> Flow {
        let completed_laser_kill = {
            let actor = &self.actors[&actor_id];
            actor.skill.retarget_after_own_direct_kill
                && matches!(actor.rules.attack.path, AttackPath::Laser { .. })
                && actor
                    .skill
                    .attack_target()
                    .is_some_and(|target| !self.fight_actor_is_alive(target))
        };
        if completed_laser_kill {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.skill.drop_lock();
            actor.skill.set_phase(FightSkillPhase::Idle);
            actor.skill.laser_attack_count = 0;
            actor.skill.retarget_after_own_direct_kill = false;
            return Flow::Done;
        }
        Flow::Next
    }

    /// A bodyless skill attacking by its motion starts its state before the
    /// idle search, when what it fires at is already in its attack area.
    fn start_bodyless_skill(&mut self, actor_id: u64, step: u64) {
        let bodyless_skill_starts_before_idle_search = {
            let actor = &self.actors[&actor_id];
            actor.motion.state == MotionState::Attacking
                && actor.skill.phase() == FightSkillPhase::Idle
                && !actor.rules.has_body
                && !actor.motion.attack_hold_fire
                && actor.skill.pending().is_none()
                && actor.skill.backswing_finish_step().is_none()
                && actor.skill.attack_target().is_some_and(|target_id| {
                    self.bodyless_target_in_attack_area(actor_id, target_id)
                })
        };
        if bodyless_skill_starts_before_idle_search {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            let prepare_steps = native_time_units_to_steps(actor.rules.attack.prepare_time_units());
            actor.skill.set_phase(if prepare_steps == 0 {
                FightSkillPhase::Attack
            } else {
                FightSkillPhase::Prepare {
                    finish_step: step.saturating_add(prepare_steps),
                }
            });
        }
    }

    /// A burst whose target died while it was still firing: with no enemy
    /// left it stops, and otherwise the rest of it is fired where it was aimed
    /// while the unit stands.
    fn projectile_burst_lost_target(
        &mut self,
        actor_id: u64,
        step: u64,
        events: &mut Vec<Event>,
    ) -> Result<Flow> {
        let active_projectile_burst_lost_target = {
            let actor = &self.actors[&actor_id];
            !actor.skill.projectile_pending_releases.is_empty()
                && actor
                    .skill
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
                actor.motion.state = MotionState::Idle;
                actor.skill.projectile_pending_releases.clear();
                actor.motion.next_target_x_q32 = actor.x_q32;
                actor.motion.next_target_z_q32 = actor.z_q32;
                actor.motion.next_speed_q32 = 0;
                actor.motion.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
                return Ok(Flow::Done);
            }
            let due = {
                let actor = self
                    .actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable");
                actor.motion.state = MotionState::Idle;
                actor.motion.next_target_x_q32 = actor.x_q32;
                actor.motion.next_target_z_q32 = actor.z_q32;
                actor.motion.next_speed_q32 = 0;
                actor.motion.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
                let mut due = Vec::new();
                actor.skill.projectile_pending_releases.retain(|pending| {
                    if pending.step <= step {
                        due.push(*pending);
                        false
                    } else {
                        true
                    }
                });
                due
            };
            for pending in due {
                self.release_pending_projectile(actor_id, pending, events)?;
            }
            return Ok(Flow::Done);
        }
        Ok(Flow::Next)
    }

    /// Moves the skill's state on where its time is up: a backswing that has
    /// ended, a prepare that has ended. Answers whether the prepare ended.
    fn advance_skill_state(
        &mut self,
        actor_id: u64,
        step: u64,
        backswing_just_finished: bool,
        quick_switch_backswing_due: bool,
        quick_switch_dead_backswing_due: bool,
    ) -> bool {
        if quick_switch_backswing_due && !quick_switch_dead_backswing_due {
            self.actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .skill
                .set_backswing_finish_step(None);
        }
        if backswing_just_finished {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.skill.set_backswing_finish_step(None);
            // The attack state outlives the backswing: `SkillAttackController`
            // has no phase running until the next blow's wait begins, and the
            // checker is asked in that gap.
            actor.skill.set_phase(FightSkillPhase::Attack);
        }
        let prepare_finished = matches!(
            self.actors[&actor_id].skill.phase(),
            FightSkillPhase::Prepare { finish_step } if finish_step <= step
        );
        if prepare_finished {
            self.actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .skill
                .set_phase(FightSkillPhase::Attack);
        }
        prepare_finished
    }

    /// The blow being wound up follows what the skill fires at, and a bodyless
    /// one whose target left its attack area is dropped.
    fn retarget_pending_blow(&mut self, actor_id: u64) -> Flow {
        let bodyful_quick_switch_target = {
            let actor = &self.actors[&actor_id];
            (actor.rules.has_body
                && actor.rules.attack.quick_switch_target
                && actor.skill.pending().is_some())
            .then_some(actor.skill.attack_target())
            .flatten()
            .filter(|&target_id| self.target_in_attack_area(actor_id, target_id))
        };
        if let Some(target_id) = bodyful_quick_switch_target {
            self.actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .skill
                .pending_mut()
                .expect("pending attack identity is stable")
                .target = target_id;
        }
        let active_attack_rejected = self.actors[&actor_id]
            .skill
            .pending()
            .is_some_and(|pending| self.bodyless_attackable_invalid(actor_id, pending.target));
        if active_attack_rejected {
            // Build 2259 SkillPrepareState and SkillAttackState both run
            // CheckAttackable before advancing their current attack phase.
            // A failed check enters SkillIdleState in the same update.
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.motion.state = MotionState::Idle;
            actor.skill.drop_lock();
            actor.skill.set_pending(None);
            actor.skill.set_phase(FightSkillPhase::Idle);
            actor.motion.next_target_x_q32 = actor.x_q32;
            actor.motion.next_target_z_q32 = actor.z_q32;
            actor.motion.next_speed_q32 = 0;
            actor.motion.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
            return Flow::Done;
        }
        Flow::Next
    }

    /// Performs what is due this update: the blow wound up, the shots of a
    /// burst, a grouped skill's core and its slots. Answers whether the blow
    /// was rejected at its attack point.
    fn perform_due_blows(
        &mut self,
        actor_id: u64,
        step: u64,
        events: &mut Vec<Event>,
    ) -> Result<(Flow, bool)> {
        let released_this_step = self.actors[&actor_id]
            .skill
            .pending()
            .is_some_and(|pending| pending.step <= step);
        let attack_point_rejected = if released_this_step {
            self.release(actor_id, events)?
        } else {
            false
        };
        if released_this_step && self.actors[&actor_id].motion.state != MotionState::Attacking {
            return Ok((Flow::Done, attack_point_rejected));
        }
        let projectile_releases = {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            let mut due = Vec::new();
            actor.skill.projectile_pending_releases.retain(|pending| {
                if pending.step <= step {
                    due.push(*pending);
                    false
                } else {
                    true
                }
            });
            due
        };
        for pending in projectile_releases {
            self.release_pending_projectile(actor_id, pending, events)?;
        }
        self.perform_group_blows(actor_id, step, events)?;
        Ok((Flow::Next, attack_point_rejected))
    }

    pub(in crate::fight) fn bodyless_target_in_attack_area(
        &self,
        actor_id: u64,
        target: FightActorRef,
    ) -> bool {
        self.bodyless_target_in_attack_range(actor_id, target)
            && self.bodyless_target_in_attack_angle(actor_id, target)
    }

    pub(in crate::fight) fn target_in_attack_area(
        &self,
        actor_id: u64,
        target: FightActorRef,
    ) -> bool {
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

    pub(in crate::fight) fn bodyless_target_in_attack_range(
        &self,
        actor_id: u64,
        target: FightActorRef,
    ) -> bool {
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

    pub(in crate::fight) fn bodyless_target_in_attack_angle(
        &self,
        actor_id: u64,
        target: FightActorRef,
    ) -> bool {
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

    pub(in crate::fight) fn bodyless_attackable_invalid(
        &self,
        actor_id: u64,
        target: FightActorRef,
    ) -> bool {
        let actor = &self.actors[&actor_id];
        if actor.rules.has_body {
            return false;
        }
        !self.bodyless_target_in_attack_area(actor_id, target)
    }

    /// Schedules the next attack and remembers the interval it used.
    pub(in crate::fight) fn sample_actor_attack_interval(
        &mut self,
        actor_id: u64,
        step: u64,
    ) -> Result<u64> {
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
            actor.skill.current_attack_interval = sampled;
        }
        Ok(step.saturating_add(sampled))
    }
}
