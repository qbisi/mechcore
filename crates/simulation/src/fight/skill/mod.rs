mod check;
mod group;
mod perform;

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
    pub(in crate::fight) projectile_burst_finished: bool,
    pub(in crate::fight) projectile_burst_finished_same_tick_dead: bool,
    pub(in crate::fight) laser_attack_count: usize,
    pub(in crate::fight) retarget_after_own_direct_kill: bool,
}

impl Skill {
    /// The coarse phase: idle (a cooling reads idle), preparing, attacking.
    pub(in crate::fight) const fn phase(&self) -> FightSkillPhase {
        match self.state {
            SkillState::Idle { .. } | SkillState::Cooling { .. } => FightSkillPhase::Idle,
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

    pub(in crate::fight) fn quick_switch_active_target_outside_attack_area(
        &mut self,
        actor_id: u64,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        allow_phase_override: bool,
    ) -> Result<bool> {
        let actor = &self.actors[&actor_id];
        let Some(FightActorRef::Unit(target_id)) = actor.skill.attack_target() else {
            return Ok(false);
        };
        let target = FightActorRef::Unit(target_id);
        let in_attacking_phase = actor.skill.phase() == FightSkillPhase::Attack
            || (actor.skill.phase() == FightSkillPhase::Idle
                && actor.motion.state == MotionState::Attacking
                && !self.bodyless_target_in_attack_range(actor_id, target));
        if (!allow_phase_override && !in_attacking_phase)
            || !actor.rules.has_body
            || !actor.rules.attack.quick_switch_target
            || actor.rules.attack.weapons.mode != WeaponMode::Normal
            || !actor.skill.projectile_pending_releases.is_empty()
            || (!allow_phase_override
                && !self.fight_actor_is_alive(target)
                && actor.motion.state != MotionState::Attacking)
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
                if actor.skill.attack_target() != Some(target) {
                    actor.skill.laser_attack_count = 0;
                }
                actor.skill.lock_target = Some(target);
                false
            }
            Some(FightActorRef::Building(building_id)) => {
                actor.skill.lock_target = Some(FightActorRef::Building(building_id));
                actor.skill.lock_is_terminal_handoff = false;
                false
            }
            None => {
                actor.skill.drop_lock();
                actor.motion.state = MotionState::Idle;
                actor.skill.set_phase(FightSkillPhase::Idle);
                actor.skill.set_pending(None);
                actor.motion.next_target_x_q32 = actor.x_q32;
                actor.motion.next_target_z_q32 = actor.z_q32;
                actor.motion.next_speed_q32 = 0;
                actor.motion.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
                true
            }
        };
        actor.skill.search_target_time = SEARCH_TARGET_RESET_TICKS;
        actor.skill.retarget_after_own_direct_kill = false;
        // A quick switch asks the line of fire like any other search: the
        // Arclight whose splash killed its lock takes the next Crawler and,
        // the same tick, the block standing between them.
        if !entered_idle {
            self.search_attack_target(actor_id);
        }
        Ok(entered_idle)
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

    #[allow(clippy::too_many_lines)]
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
        if self.hold_through_cooling(actor_id, step, target_search_order)? {
            return Ok(());
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
            return Ok(());
        }
        // `SkillAttackState.Update` asks `CheckAttackable` between two blows,
        // and a failed check finishes the attack.
        if self.between_blows(actor_id, step) {
            if !self.attack_state_check_attackable(actor_id, target_search_order)? {
                self.finish_attack(actor_id, step);
                return Ok(());
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
        if matches!(
            self.actors[&actor_id].skill.lock_target,
            Some(FightActorRef::Building(_))
        ) && self.actors[&actor_id].skill.lock_is_terminal_handoff
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
            return Ok(());
        }
        self.update_group_skill_targets(actor_id, step, target_search_order)?;
        self.refresh_group_walls(actor_id);
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
            return Ok(());
        }
        let stale_attack_target = {
            let actor = &self.actors[&actor_id];
            if actor.motion.state == MotionState::Attacking
                && !actor.motion.attack_hold_fire
                && actor.skill.pending().is_none()
                && actor.skill.backswing_finish_step().is_none()
                && actor
                    .skill
                    .attack_target()
                    .is_some_and(|target| !self.fight_actor_is_alive(target))
            {
                actor.skill.attack_target()
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
            actor.motion.state = MotionState::Idle;
            actor.skill.drop_lock();
            actor.skill.set_phase(FightSkillPhase::Idle);
            actor.motion.next_target_x_q32 = actor.x_q32;
            actor.motion.next_target_z_q32 = actor.z_q32;
            actor.motion.next_speed_q32 = 0;
            actor.motion.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
            return Ok(());
        }
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
        let dead_backswing_just_finished = backswing_just_finished
            && self.actors[&actor_id]
                .skill
                .attack_target()
                .is_some_and(|target| !self.fight_actor_is_alive(target));
        let deferred_projectile_burst_finish = {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            std::mem::replace(&mut actor.skill.projectile_burst_finished, false)
        };
        let deferred_same_tick_dead_burst_finish = {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            std::mem::replace(
                &mut actor.skill.projectile_burst_finished_same_tick_dead,
                false,
            )
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
                .skill
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
                .skill
                .attack_target()
                .is_none_or(|target_id| !self.target_in_attack_area(actor_id, target_id));
        if force_burst_finish_target_search {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.skill.set_phase(FightSkillPhase::Idle);
            actor.skill.search_target_time = 0;
        }
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
                actor.skill.projectile_burst_finished = false;
                actor.skill.projectile_burst_finished_same_tick_dead = false;
                actor.motion.next_target_x_q32 = actor.x_q32;
                actor.motion.next_target_z_q32 = actor.z_q32;
                actor.motion.next_speed_q32 = 0;
                actor.motion.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
                return Ok(());
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
                if !due.is_empty() && actor.skill.projectile_pending_releases.is_empty() {
                    actor.skill.projectile_burst_finished = true;
                    actor.skill.projectile_burst_finished_same_tick_dead = true;
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
        // `SkillIdleState.TryPerform` reaches `SearchAttackTarget` on every
        // update the skill is idle with a lock.
        if self.actors[&actor_id].skill.phase() == FightSkillPhase::Idle {
            self.search_attack_target(actor_id);
        }
        self.update_fight_skill_target_search(actor_id, step, target_search_order)?;
        if quick_switch_backswing_due && !quick_switch_dead_backswing_due {
            self.actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .skill
                .set_backswing_finish_step(None);
        }
        let stale_replacement = if stale_attack_target.is_some()
            || quick_switch_dead_backswing_due
            || dead_backswing_just_finished
        {
            self.actors[&actor_id].skill.attack_target()
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
            let entered_idle = actor.motion.state != MotionState::Idle;
            actor.motion.state = MotionState::Idle;
            actor.skill.drop_lock();
            actor.skill.set_phase(FightSkillPhase::Idle);
            actor.skill.set_backswing_finish_step(None);
            actor.skill.retarget_after_own_direct_kill = false;
            if entered_idle {
                actor.motion.next_target_x_q32 = actor.x_q32;
                actor.motion.next_target_z_q32 = actor.z_q32;
            }
            actor.motion.next_speed_q32 = 0;
            actor.motion.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
            return Ok(());
        }
        if backswing_just_finished {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.skill.set_backswing_finish_step(None);
            actor
                .skill
                .set_phase(if actor.rules.attack.quick_switch_target {
                    FightSkillPhase::Attack
                } else {
                    FightSkillPhase::Idle
                });
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
            return Ok(());
        }
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
            return Ok(());
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
            let burst_finished =
                !due.is_empty() && actor.skill.projectile_pending_releases.is_empty();
            if burst_finished {
                actor.skill.projectile_burst_finished = true;
                actor.skill.projectile_burst_finished_same_tick_dead = false;
            }
            due
        };
        for pending in projectile_releases {
            self.release_pending_projectile(actor_id, pending, events)?;
        }
        let group_core_target = {
            let actor = &self.actors[&actor_id];
            (actor.rules.attack.weapons.mode == WeaponMode::Group
                && actor.motion.state == MotionState::Attacking
                && !actor.motion.attack_hold_fire
                && actor.skill.pending().is_none()
                && actor.skill.backswing_finish_step().is_none()
                && actor.skill.phase() == FightSkillPhase::Attack
                && actor
                    .skill
                    .group_skill_prepare_ready_steps
                    .first()
                    .is_none_or(|ready_step| *ready_step <= step)
                && step >= actor.skill.next_attack_step)
                .then(|| actor.skill.mechanical_attack_target())
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
            actor.skill.next_attack_step = next_attack_step;
            actor.skill.set_pending(Some(PendingRelease {
                step,
                target: target_id,
            }));
            let _attack_point_rejected = self.release(actor_id, events)?;
        }
        let group_releases = {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            let mut due = Vec::new();
            actor
                .skill
                .group_pending_releases
                .retain(|&(skill_index, pending)| {
                    if pending.step <= step {
                        due.push((skill_index, pending));
                        false
                    } else {
                        true
                    }
                });
            for skill_index in 1..actor.skill.group_skill_targets.len() {
                let next_attack_step = actor.skill.group_skill_next_attack_steps[skill_index];
                let prepare_ready_step = actor.skill.group_skill_prepare_ready_steps[skill_index];
                if next_attack_step > 0
                    && next_attack_step <= step
                    && prepare_ready_step <= step
                    && let Some(target) = actor.skill.group_attack_target(skill_index)
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
                        .skill
                        .group_skill_targets
                        .get(*skill_index)
                        .copied()
                        .flatten()
                    && let Some(target) = actor.skill.group_attack_target(*skill_index)
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
        let lock_target = self.actors[&actor_id].skill.mechanical_attack_target();
        if let Some(target) = lock_target {
            let target_alive = self.fight_actor_is_alive(target);
            if !target_alive
                && self.actors[&actor_id]
                    .skill
                    .backswing_finish_step()
                    .is_some()
            {
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
                    if self.actors[&actor_id].skill.in_the_way.is_some_and(|(wall, _)| wall == building));
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
                                .skill
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
                let entered_idle = actor.motion.state != MotionState::Idle && !holds_a_block;
                if !holds_a_block {
                    actor.motion.state = MotionState::Idle;
                }
                if entered_idle {
                    actor.motion.next_target_x_q32 = actor.x_q32;
                    actor.motion.next_target_z_q32 = actor.z_q32;
                }
                actor.motion.next_speed_q32 = 0;
                actor.motion.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
                return Ok(());
            }
            if !target_alive && backswing_just_finished {
                let actor = self
                    .actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable");
                let entered_idle = actor.motion.state != MotionState::Idle;
                actor.skill.drop_lock();
                actor.skill.retarget_after_own_direct_kill = false;
                actor.motion.state = MotionState::Idle;
                if entered_idle {
                    actor.motion.next_target_x_q32 = actor.x_q32;
                    actor.motion.next_target_z_q32 = actor.z_q32;
                }
                actor.motion.next_speed_q32 = 0;
                actor.motion.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
                return Ok(());
            }
            if !target_alive {
                self.actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable")
                    .skill
                    .drop_lock();
            }
        }
        let target = self.actors[&actor_id].skill.mechanical_attack_target();
        let Some(target) = target else {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.motion.state = MotionState::Idle;
            actor.motion.next_target_x_q32 = actor.x_q32;
            actor.motion.next_target_z_q32 = actor.z_q32;
            actor.motion.next_speed_q32 = 0;
            actor.motion.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
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
            .skill
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
            && actor.motion.state == MotionState::Attacking
            && actor
                .skill
                .group_skill_targets
                .first()
                .is_some_and(Option::is_none)
            && actor
                .skill
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
                let entered_attack = actor.motion.state != MotionState::Attacking;
                actor.motion.state = MotionState::Attacking;
                // RVOControllerFixed.StopMove refreshes the target point on
                // every MotionAttackState update. It submits zero desired
                // speed while retaining the unit's configured maximum speed,
                // so neighbouring agents can still push a stopped attacker.
                actor.motion.next_target_x_q32 = actor.x_q32;
                actor.motion.next_target_z_q32 = actor.z_q32;
                actor.motion.next_speed_q32 = 0;
                actor.motion.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
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
                    actor.motion.state = MotionState::Idle;
                    actor.skill.drop_lock();
                    actor.skill.set_phase(FightSkillPhase::Idle);
                    actor.motion.next_target_x_q32 = actor.x_q32;
                    actor.motion.next_target_z_q32 = actor.z_q32;
                    actor.motion.next_speed_q32 = 0;
                    actor.motion.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
                    return Ok(());
                }
                if entered_attack {
                    // A newly entered bodyless attack state cannot start its
                    // FightSkill while the root transform is outside the
                    // attack cone. MotionController clears this hold only
                    // after it has observed and corrected the facing.
                    actor.motion.attack_hold_fire = !actor.rules.has_body
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
                    && !actor.motion.attack_hold_fire
                    && !in_attack_angle
                    && actor.skill.pending().is_none()
                    && actor.skill.backswing_finish_step().is_none();
                if invalid_attack_angle_barrier {
                    // MotionAttackState returns to Idle when an active bodyless
                    // skill loses its root-transform attack angle. The new
                    // Idle state is entered synchronously but is not updated
                    // recursively, so target reacquisition waits one tick and
                    // this transition tick preserves the old body facing.
                    actor.motion.state = MotionState::Idle;
                    actor.skill.drop_lock();
                    actor.skill.set_phase(FightSkillPhase::Idle);
                    actor.motion.next_target_x_q32 = actor.x_q32;
                    actor.motion.next_target_z_q32 = actor.z_q32;
                    actor.motion.next_speed_q32 = 0;
                    actor.motion.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
                    return Ok(());
                }
                let clear_hold_after_motion = actor.motion.attack_hold_fire && in_attack_angle;
                let mut entered_skill_phase = false;
                // `SkillIdleState.TryStartAttack` enters the attack or prepare
                // state on the tick the unit comes into its attack area,
                // which is the tick its motion starts attacking; the state
                // is not updated until the tick after, where the first
                // blow's wait begins.
                if !actor.motion.attack_hold_fire
                    && in_attack_angle
                    && actor.skill.pending().is_none()
                    && actor.skill.backswing_finish_step().is_none()
                    && actor.skill.phase() == FightSkillPhase::Idle
                {
                    let prepare_steps =
                        native_time_units_to_steps(actor.rules.attack.prepare_time_units());
                    let from = if entered_attack { step + 1 } else { step };
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
                    && step >= actor.skill.next_attack_step
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
                    actor.skill.next_attack_step = step.saturating_add(sampled);
                    actor.skill.current_attack_interval = sampled;
                    let attack_point_steps =
                        native_time_units_to_steps(actor.rules.attack.attack_point_time_units());
                    actor.skill.set_pending(Some(PendingRelease {
                        step: step.saturating_add(attack_point_steps),
                        target,
                    }));
                }
                (
                    entered_attack,
                    actor
                        .skill
                        .pending()
                        .is_some_and(|pending| pending.step == step),
                    clear_hold_after_motion,
                )
            };
            if release_now {
                let _attack_point_rejected = self.release(actor_id, events)?;
            }
            if self.actors[&actor_id].motion.state != MotionState::Attacking {
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
                        .skill
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
                actor.motion.attack_hold_fire = false;
            }
            if actor.rules.has_body
                && (actor.motion.current_velocity_x_q32 != 0
                    || actor.motion.current_velocity_z_q32 != 0)
            {
                actor.rotate_body_towards(direction_degrees_q32_raw(
                    actor.motion.current_velocity_x_q32,
                    actor.motion.current_velocity_z_q32,
                ));
            }
            return Ok(());
        }
        if actor.rules.attack.weapons.mode == WeaponMode::Group
            && actor.motion.state == MotionState::Attacking
            && actor
                .skill
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
        if actor.motion.state == MotionState::Attacking
            && !actor.rules.has_body
            && !actor.motion.attack_hold_fire
            && actor.skill.pending().is_none()
            && actor.skill.backswing_finish_step().is_none()
            && (matches!(actor.rules.attack.path, AttackPath::Direct { melee: true })
                || actor.skill.phase() == FightSkillPhase::Attack)
        {
            // FightSkill updates before MotionController. An active bodyless
            // attack rejects an out-of-range retained target and enters
            // SkillIdleState before MotionAttackState can fall through to
            // movement. Both state machines expose one targetless Idle tick.
            actor.motion.state = MotionState::Idle;
            actor.skill.drop_lock();
            actor.skill.set_phase(FightSkillPhase::Idle);
            actor.motion.attack_hold_fire = false;
            actor.motion.next_target_x_q32 = actor.x_q32;
            actor.motion.next_target_z_q32 = actor.z_q32;
            actor.motion.next_speed_q32 = 0;
            actor.motion.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
            return Ok(());
        }
        if attack_point_rejected
            && matches!(actor.rules.attack.path, AttackPath::Direct { melee: true })
            && !actor.rules.has_body
            && actor.motion.state == MotionState::Attacking
            && actor.skill.pending().is_none()
            && actor.skill.backswing_finish_step().is_none()
        {
            // MotionAttackState leaves through Idle when its current attack
            // target is no longer in range. Idle target acquisition runs on
            // the following update rather than recursively entering Moving.
            actor.motion.state = MotionState::Idle;
            actor.skill.drop_lock();
            actor.skill.set_phase(FightSkillPhase::Idle);
            actor.motion.next_target_x_q32 = actor.x_q32;
            actor.motion.next_target_z_q32 = actor.z_q32;
            actor.motion.next_speed_q32 = 0;
            actor.motion.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
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
            actor.motion.state = MotionState::Idle;
            actor.skill.drop_lock();
            actor.skill.set_phase(FightSkillPhase::Idle);
            actor.motion.next_target_x_q32 = actor.x_q32;
            actor.motion.next_target_z_q32 = actor.z_q32;
            actor.motion.next_speed_q32 = 0;
            actor.motion.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
            return Ok(());
        }
        let entered_move_from_idle = actor.motion.state == MotionState::Idle;
        let entered_move = actor.motion.state != MotionState::Moving;
        let entered_move_below_min_range =
            entered_move && edge_distance_q32 < space_to_q32(actor.rules.attack.min_range());
        if !entered_move_from_idle && !entered_move_below_min_range {
            // FightSkill.Update tracks an existing target before MotionController updates movement.
            // A target acquired by MotionIdleState is not visible to FightSkill until the next tick.
            actor.rotate_weapons_towards(target_rotation_q32);
            if actor.rules.has_body {
                actor.aim_rotation = degrees_q32_to_mdeg(
                    actor
                        .skill
                        .weapon_rotations_q32
                        .first()
                        .copied()
                        .unwrap_or(actor.body_rotation_q32),
                );
            }
        }
        actor.motion.state = MotionState::Moving;
        actor.motion.attack_hold_fire = false;
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
        actor.motion.next_target_x_q32 = move_target_x_q32;
        actor.motion.next_target_z_q32 = move_target_z_q32;
        if actor.motion.current_velocity_x_q32 != 0 || actor.motion.current_velocity_z_q32 != 0 {
            // MotionMoveState.MoveUpdate runs NormalRotate before Move;
            // CalculateMoveSpeed therefore observes this tick's new facing.
            actor.rotate_body_towards(direction_degrees_q32_raw(
                actor.motion.current_velocity_x_q32,
                actor.motion.current_velocity_z_q32,
            ));
        }
        actor.motion.next_speed_q32 = turn_limited_move_speed_q32(
            space_to_q32(actor.stats.move_speed()),
            actor.rules.rotate_speed_mdeg_per_second(),
            actor.body_rotation_q32,
            actor.motion.current_velocity_x_q32,
            actor.motion.current_velocity_z_q32,
        );
        actor.motion.next_max_speed_q32 = actor.motion.next_speed_q32;
        if !actor.rules.has_body {
            actor.aim_rotation = actor.body_rotation;
        }
        Ok(())
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
