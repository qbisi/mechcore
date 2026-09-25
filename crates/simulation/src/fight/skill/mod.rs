mod check;
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
    /// Where the burst aimed this projectile when it began: the target's
    /// position then, plus the offset.
    pub(in crate::fight) target_x_q32: i64,
    pub(in crate::fight) target_z_q32: i64,
    /// The offset alone. A projectile that follows its target lands it from
    /// where the target stands when the projectile is released: the second
    /// of a Phantom Ray's two projectiles, from where a charging Rhino stands
    /// 0.3 seconds on.
    pub(in crate::fight) offset_x_q32: i64,
    pub(in crate::fight) offset_z_q32: i64,
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
    /// The rounds left in a skill that fires from a magazine
    /// (`SkillData.isLoadingType`), and none for one that does not.
    pub(in crate::fight) rounds: Option<u32>,
}

impl Skill {
    /// A skill entering the fight: idle, no lock, nothing scheduled.
    pub(in crate::fight) fn new(
        weapon_rotations_q32: Vec<i64>,
        group_skill_count: usize,
        magazine: Option<Magazine>,
    ) -> Self {
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
            rounds: magazine.map(|magazine| magazine.capacity),
        }
    }

    /// A blow performed takes a round from the magazine, if the skill has one.
    pub(in crate::fight) fn fire_round(&mut self) {
        if let Some(rounds) = &mut self.rounds {
            *rounds = rounds.saturating_sub(1);
        }
    }

    /// `SkillAttackState.TryPerformAttack` once the interval is up: schedules
    /// the next attack after the interval drawn for it, and winds up a blow
    /// that lands after the attack point.
    pub(in crate::fight) fn schedule_blow(
        &mut self,
        step: u64,
        interval: u64,
        attack_point_steps: u64,
        target: FightActorRef,
    ) {
        self.next_attack_step = step.saturating_add(interval);
        self.current_attack_interval = interval;
        self.set_pending(Some(PendingRelease {
            step: step.saturating_add(attack_point_steps),
            target,
        }));
    }

    /// `SkillManager.UpdateWeaponRotateion`: every weapon turns towards a
    /// bearing, by at most one update's turn.
    pub(in crate::fight) fn turn_weapons_towards(&mut self, bearing_q32: i64, turn_q32: i64) {
        for rotation in &mut self.weapon_rotations_q32 {
            *rotation = rotate_towards_q32(*rotation, bearing_q32, turn_q32);
        }
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

/// What a skill's update hands on to the rest of its owner's update.
#[derive(Debug, Clone, Copy)]
pub(in crate::fight) struct SkillUpdate {
    /// A backswing ended before this update.
    pub(in crate::fight) backswing_just_finished: bool,
    /// A prepare ended on this update.
    pub(in crate::fight) prepare_finished: bool,
    /// The blow due on this update was rejected at its attack point.
    pub(in crate::fight) attack_point_rejected: bool,
}

impl Simulation {
    /// `SkillCoolingState`: holds a skill whose attack has finished idle and
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
        owner: FightActorRef,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<bool> {
        let Some((started, held)) = self.skill(owner).cooling() else {
            return Ok(false);
        };
        let cooling_steps = native_time_units_to_steps(
            self.attacker(owner)
                .expect("skill owner identity is stable")
                .attack
                .cooling_time_units(),
        );
        if step > started.saturating_add(cooling_steps) {
            self.skill_mut(owner).set_cooling(None);
            return Ok(false);
        }
        let candidate = if step < started.saturating_add(cooling_steps) {
            match held {
                Some(candidate) => Some(candidate),
                None => self.select_normal_target_with_order(owner, target_search_order, true)?,
            }
        } else {
            None
        };
        let skill = self.skill_mut(owner);
        skill.lock_target = None;
        skill.set_cooling(Some((started, candidate)));
        skill.search_target_time = 0;
        if let Some(actor) = self.moving_mut(owner) {
            actor.stop_in_place(true);
        }
        Ok(true)
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the search timer's update, to be split with the attack state"
    )]
    pub(in crate::fight) fn update_fight_skill_target_search(
        &mut self,
        owner: FightActorRef,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<()> {
        // Target-build MechData disables MechSearchTargetController for every
        // supported non-supergiant unit, so live periodic selection belongs to
        // the main FightSkill. Prepare and Attack retain this private counter;
        // Attack only enters the selector when its private attack target is no
        // longer alive.
        let quick_switch_target = self
            .attacker(owner)
            .expect("skill owner identity is stable")
            .attack
            .quick_switch_target;
        let skill = self.skill(owner);
        let target = skill
            .mechanical_attack_target()
            .and_then(|target| self.fight_actor(target));
        let target_alive = target.is_some_and(|target| target.alive && target.targetable);
        let target_died_during_tick =
            target.is_some_and(|target| target.query_alive && !target.alive);
        if !target_alive
            && skill
                .backswing_finish_step()
                .is_some_and(|finish_step| finish_step >= step)
        {
            let quick_switch_interval_due = quick_switch_target && step > skill.next_attack_step;
            if !quick_switch_interval_due {
                return Ok(());
            }
            self.skill_mut(owner).set_backswing_finish_step(None);
        }
        let skill = self.skill(owner);
        if (matches!(skill.phase(), FightSkillPhase::Prepare { .. })
            || skill.phase() == FightSkillPhase::Attack)
            && (!quick_switch_target || target_alive)
        {
            return Ok(());
        }
        if target_alive && skill.search_target_time > 0 {
            self.skill_mut(owner).search_target_time -= 1;
            return Ok(());
        }

        self.skill_mut(owner).searched_this_tick = true;

        // With no enemy unit left, the idle search is the match's end: the
        // selector would answer the defeated team's `FightCrystal`, and
        // `Simulation::step` stands in for that tick (the terminal handoff).
        let source_team = self
            .attacker(owner)
            .expect("skill owner identity is stable")
            .team;
        let match_over = !self
            .actors
            .values()
            .any(|candidate| candidate.placement.team != source_team && candidate.alive());
        let located =
            |error: Error| Error::new(format!("logic step {step} actor {}: {error}", owner.id()));
        let mut selected_candidate = if match_over {
            None
        } else {
            self.select_normal_target_with_order(
                owner,
                target_search_order,
                target_died_during_tick,
            )
            .map_err(located)?
        };
        if !match_over
            && !target_died_during_tick
            && selected_candidate
                .and_then(|candidate| self.fight_actor(candidate))
                .is_some_and(|target| target.query_alive && !target.alive)
        {
            selected_candidate = self
                .select_normal_target_with_order(owner, target_search_order, true)
                .map_err(located)?;
        }
        if let Some(FightActorRef::Building(building_id)) = selected_candidate {
            let skill = self.skill_mut(owner);
            skill.lock_target = Some(FightActorRef::Building(building_id));
            skill.lock_is_terminal_handoff = false;
            skill.search_target_time = SEARCH_TARGET_RESET_TICKS;
            skill.set_phase(FightSkillPhase::Idle);
            skill.retarget_after_own_direct_kill = false;
            skill.laser_attack_count = 0;
            return Ok(());
        }
        let selected = selected_candidate;
        let skill = self.skill(owner);
        let quick_idle_retains_attackable_target = quick_switch_target
            && skill.phase() == FightSkillPhase::Idle
            && step >= skill.next_attack_step
            && skill
                .attack_target()
                .is_some_and(|target_id| self.target_in_attack_area(owner, target_id));
        let selected = if target_alive
            && (!quick_switch_target || quick_idle_retains_attackable_target)
            && self.motion_state(owner) == MotionState::Attacking
            && !self.attack_hold_fire(owner)
            && skill.pending().is_none()
            && skill.backswing_finish_step().is_none()
            && selected != skill.lock_target
        {
            // An attacking unit keeps the lock it has. What its weapons fire
            // at is asked again below, so a construction still in the way is
            // handed back to them rather than written into the lock.
            skill.lock_target
        } else {
            selected
        };
        let skill = self.skill_mut(owner);
        skill.lock_is_terminal_handoff = false;
        if skill.attack_target() != selected {
            skill.laser_attack_count = 0;
        }
        skill.lock_target = selected;
        skill.search_target_time = SEARCH_TARGET_RESET_TICKS;
        skill.retarget_after_own_direct_kill = false;
        self.search_attack_target(owner);
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

    /// One unit's update, in the order `FightMech.Update` runs it: its skill,
    /// then its motion, then its buffs.
    ///
    /// The motion's part, `update_motion`, is where a target in range starts
    /// the skill (`SkillIdleState.TryStartAttack`) and one out of range is
    /// left or walked towards. `BuffManager.Update` runs whichever way the two
    /// before it ended.
    pub(in crate::fight) fn step_actor_with_target_order(
        &mut self,
        actor_id: u64,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        if !self.actors[&actor_id].alive() {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.exit_fight_on_death();
            return self.drop_buffs_of_the_dead(actor_id);
        }
        self.step_actor_skill_and_motion(actor_id, step, target_search_order, events)?;
        self.update_buffs(actor_id)
    }

    fn step_actor_skill_and_motion(
        &mut self,
        actor_id: u64,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let Some(update) = self.update_skill(
            FightActorRef::Unit(actor_id),
            step,
            target_search_order,
            events,
        )?
        else {
            return Ok(());
        };
        self.update_motion(
            actor_id,
            step,
            events,
            update.backswing_just_finished,
            update.prepare_finished,
            update.attack_point_rejected,
        )
    }

    /// `SkillManager.Update`: one skill's update, for whoever owns it.
    ///
    /// A magazine emptied or refilling (`SkillReloadingState`), the checks the
    /// skill's state runs (`SkillCoolingState`, `SkillPrepareState` and
    /// `SkillAttackState` asking `CheckAttackable`), a grouped skill's slots,
    /// the ends of a burst, the idle search and its timer, the state moving
    /// on as its time is up, and what is due to be performed. Each part says
    /// whether the update goes on; `None` is an update that ended here.
    pub(in crate::fight) fn update_skill(
        &mut self,
        owner: FightActorRef,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        events: &mut Vec<Event>,
    ) -> Result<Option<SkillUpdate>> {
        let backswing_just_finished = self
            .skill(owner)
            .backswing_finish_step()
            .is_some_and(|finish_step| finish_step < step);
        if let Flow::Done = self.update_reload(owner, step) {
            return Ok(None);
        }
        if let Flow::Done = self.update_skill_checks(owner, step, target_search_order)? {
            return Ok(None);
        }
        if let Flow::Done = self.release_terminal_handoff(owner) {
            return Ok(None);
        }
        let grouped = self
            .attacker(owner)
            .expect("skill owner identity is stable")
            .attack
            .weapons
            .mode
            == WeaponMode::Group;
        if grouped {
            let actor_id = owner.unit_id().expect("only a unit's skill is grouped");
            self.update_group_skill_targets(actor_id, step, target_search_order)?;
            self.refresh_group_walls(actor_id);
        }
        if let Flow::Done = self.finish_laser_own_kill(owner) {
            return Ok(None);
        }
        self.start_bodyless_skill(owner, step);
        let quick_switch_target = self
            .attacker(owner)
            .expect("skill owner identity is stable")
            .attack
            .quick_switch_target;
        let quick_switch_backswing_due = {
            let skill = self.skill(owner);
            quick_switch_target
                && skill
                    .backswing_finish_step()
                    .is_some_and(|finish_step| finish_step >= step)
                && step > skill.next_attack_step
        };
        let quick_switch_dead_backswing_due = quick_switch_backswing_due
            && self
                .skill(owner)
                .attack_target()
                .is_some_and(|target| !self.fight_actor_is_alive(target));
        if let Flow::Done = self.projectile_burst_lost_target(owner, step, events)? {
            return Ok(None);
        }
        // `SkillIdleState.TryPerform` reaches `SearchAttackTarget` on every
        // update the skill is idle with a lock.
        if self.skill(owner).phase() == FightSkillPhase::Idle {
            self.search_attack_target(owner);
        }
        self.update_fight_skill_target_search(owner, step, target_search_order)?;
        let prepare_finished = self.advance_skill_state(
            owner,
            step,
            backswing_just_finished,
            quick_switch_backswing_due,
            quick_switch_dead_backswing_due,
        );
        if let Flow::Done = self.retarget_pending_blow(owner) {
            return Ok(None);
        }
        let (flow, attack_point_rejected) = self.perform_due_blows(owner, step, events)?;
        if let Flow::Done = flow {
            return Ok(None);
        }
        Ok(Some(SkillUpdate {
            backswing_just_finished,
            prepare_finished,
            attack_point_rejected,
        }))
    }

    /// `SkillReloadingState`, for a skill that fires from a magazine: an
    /// empty magazine takes the skill into the reload on its next update,
    /// whatever state it is in; the reload keeps the lock, lasts its time, and
    /// hands the skill back to idle, full, its search timer reset
    /// (`SkillReloadingState.Exit`). The state's update is the skill's whole
    /// update. A skill without a magazine never reloads.
    fn update_reload(&mut self, owner: FightActorRef, step: u64) -> Flow {
        let Some(magazine) = self
            .attacker(owner)
            .expect("skill owner identity is stable")
            .attack
            .magazine
        else {
            return Flow::Next;
        };
        let skill = self.skill_mut(owner);
        match skill.state {
            SkillState::Reloading { finish_step } => {
                if step >= finish_step {
                    // The Rapid-Fire Turret of `rapid-fire-head-on.yaml` fires
                    // its 11th shot at the Crawler it fired its 10th at, where
                    // a search from its weapon would have taken another: the
                    // idle the reload hands the lock to keeps it.
                    skill.rounds = Some(magazine.capacity);
                    skill.state = SkillState::Idle { ready_step: None };
                    skill.search_target_time = SEARCH_TARGET_RESET_TICKS;
                }
                Flow::Done
            }
            _ if skill.rounds == Some(0) => {
                skill.state = SkillState::Reloading {
                    finish_step: step
                        .saturating_add(native_time_units_to_steps(magazine.reload_time_units())),
                };
                Flow::Done
            }
            _ => Flow::Next,
        }
    }

    /// `SkillIdleState.TryStartAttack` and `SkillAttackState.TryPerformAttack`
    /// for a skill whose target is in reach: an idle skill enters its prepare
    /// or attack state, and an attacking one begins the next blow's wait once
    /// its interval is up.
    ///
    /// `entered_attack` is whether the owner comes into its attack on this
    /// update: a state is not updated on the tick it is entered, so a blow
    /// waits for the tick after.
    pub(in crate::fight) fn try_start_attack(
        &mut self,
        owner: FightActorRef,
        step: u64,
        target: FightActorRef,
        entered_attack: bool,
        in_attack_angle: bool,
        prepare_finished: bool,
    ) {
        let attack = self
            .attacker(owner)
            .expect("skill owner identity is stable")
            .attack;
        let prepare_steps = native_time_units_to_steps(attack.prepare_time_units());
        let attack_point_steps = native_time_units_to_steps(attack.attack_point_time_units());
        let attack_hold_fire = self.attack_hold_fire(owner);
        let skill = self.skill_mut(owner);
        let mut entered_skill_phase = false;
        // `SkillIdleState.TryStartAttack` enters the attack or prepare
        // state on the tick the unit comes into its attack area,
        // which is the tick its motion starts attacking; the state
        // is not updated until the tick after, where the first
        // blow's wait begins.
        // A bodyless unit whose facing its motion is still correcting
        // enters it all the same; only the blow waits for the facing.
        if in_attack_angle
            && skill.pending().is_none()
            && skill.backswing_finish_step().is_none()
            && skill.phase() == FightSkillPhase::Idle
        {
            // A grouped skill's core is a `GroupedSkillAttackBehaviour`,
            // not a `FightSkill`, and prepares from the tick after its
            // motion starts attacking; nothing has captured its states
            // yet to say why.
            let from = if entered_attack && !skill.group_skill_targets.is_empty() {
                step + 1
            } else {
                step
            };
            skill.set_phase(if prepare_steps == 0 {
                FightSkillPhase::Attack
            } else {
                FightSkillPhase::Prepare {
                    finish_step: from.saturating_add(prepare_steps),
                }
            });
            entered_skill_phase = prepare_steps > 0 || entered_attack;
        }
        // A skill already in its attack state starts its next blow's wait on
        // the tick its interval is up, whatever its motion did: a Crawler
        // pushed out of reach during its backswing and back in on the tick
        // after starts its next blow on the tick it returns, as the game's
        // skill state reads in the Rhino's formation fight.
        if !attack_hold_fire
            && in_attack_angle
            && skill.pending().is_none()
            && skill.backswing_finish_step().is_none()
            && skill.phase() == FightSkillPhase::Attack
            && !entered_skill_phase
            // A state is not updated on the tick it is entered: the
            // first blow waits for the tick after the prepare ends.
            && !prepare_finished
            && step >= skill.next_attack_step
        {
            let interval = self
                .draw_attack_interval(owner)
                .expect("every skill owner's team owns one attack random stream");
            self.skill_mut(owner)
                .schedule_blow(step, interval, attack_point_steps, target);
        }
    }

    /// The checks the skill's state runs at the start of its update:
    /// `SkillCoolingState` holding, `SkillPrepareState` and `SkillAttackState`
    /// asking `CheckAttackable`.
    fn update_skill_checks(
        &mut self,
        owner: FightActorRef,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<Flow> {
        if self.hold_through_cooling(owner, step, target_search_order)? {
            return Ok(Flow::Done);
        }
        // `SkillPrepareState.Update` asks `SkillAttackableChecker.Check` on
        // every update; a failed check leaves the skill idle with its targets
        // cleared. The check decides the attack target again on the way,
        // which is where a block coming into the line of fire is met.
        if matches!(self.skill(owner).phase(), FightSkillPhase::Prepare { .. })
            && !self.check_attackable(owner, target_search_order)?
        {
            self.enter_idle_clearing_targets(owner);
            return Ok(Flow::Done);
        }
        // `SkillAttackState.Update` asks `CheckAttackable` between two blows,
        // and a failed check finishes the attack.
        if self.between_blows(owner, step) {
            if !self.attack_state_check_attackable(owner, target_search_order)? {
                self.finish_attack(owner, step);
                return Ok(Flow::Done);
            }
            // The blow being wound up is performed on the skill's attack
            // target, which the check may just have changed.
            let skill = self.skill_mut(owner);
            if skill.group_skill_targets.is_empty()
                && let Some(target) = skill.attack_target()
                && let Some(pending) = skill.pending_mut()
            {
                pending.target = target;
            }
        }
        Ok(Flow::Next)
    }

    /// The match's end hands a unit one of the defeated team's towers
    /// (`FightCrystal.IsTower`: its `EnergyTower` or `ResearchCenter`) for one
    /// tick, and takes it back the tick after.
    fn release_terminal_handoff(&mut self, owner: FightActorRef) -> Flow {
        let skill = self.skill(owner);
        if matches!(skill.lock_target, Some(FightActorRef::Building(_)))
            && skill.lock_is_terminal_handoff
        {
            // The native terminal handoff exposes one of the defeated team's
            // towers for one tick. The following update consumes the already
            // published displacement, then `FightCoreSystem.TryDstroyTower` clears
            // the transient lock before the terminal snapshot is written.
            let clear_velocity = self.terminal_drain_pending;
            let skill = self.skill_mut(owner);
            skill.drop_lock();
            skill.lock_is_terminal_handoff = false;
            skill.set_phase(FightSkillPhase::Idle);
            if let Some(actor) = self.moving_mut(owner) {
                if clear_velocity {
                    actor.motion.current_velocity_x_q32 = 0;
                    actor.motion.current_velocity_z_q32 = 0;
                }
                actor.stop_in_place(true);
            }
            return Flow::Done;
        }
        Flow::Next
    }

    /// A laser whose own beam killed its target ends its attack the update
    /// after.
    fn finish_laser_own_kill(&mut self, owner: FightActorRef) -> Flow {
        let beams = matches!(
            self.attacker(owner)
                .expect("skill owner identity is stable")
                .attack
                .path,
            AttackPath::Laser { .. }
        );
        let skill = self.skill(owner);
        let completed_laser_kill = skill.retarget_after_own_direct_kill
            && beams
            && skill
                .attack_target()
                .is_some_and(|target| !self.fight_actor_is_alive(target));
        if completed_laser_kill {
            let skill = self.skill_mut(owner);
            skill.drop_lock();
            skill.set_phase(FightSkillPhase::Idle);
            skill.laser_attack_count = 0;
            skill.retarget_after_own_direct_kill = false;
            return Flow::Done;
        }
        Flow::Next
    }

    /// A bodyless skill attacking by its motion starts its state before the
    /// idle search, when what it fires at is already in its attack area.
    fn start_bodyless_skill(&mut self, owner: FightActorRef, step: u64) {
        let attacker = self
            .attacker(owner)
            .expect("skill owner identity is stable");
        let has_body = attacker.has_body;
        let prepare_steps = native_time_units_to_steps(attacker.attack.prepare_time_units());
        let skill = self.skill(owner);
        let bodyless_skill_starts_before_idle_search = self.motion_state(owner)
            == MotionState::Attacking
            && skill.phase() == FightSkillPhase::Idle
            && !has_body
            && !self.attack_hold_fire(owner)
            && skill.pending().is_none()
            && skill.backswing_finish_step().is_none()
            && skill
                .attack_target()
                .is_some_and(|target_id| self.target_in_attack_area(owner, target_id));
        if bodyless_skill_starts_before_idle_search {
            self.skill_mut(owner).set_phase(if prepare_steps == 0 {
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
        owner: FightActorRef,
        step: u64,
        events: &mut Vec<Event>,
    ) -> Result<Flow> {
        let skill = self.skill(owner);
        let active_projectile_burst_lost_target = !skill.projectile_pending_releases.is_empty()
            && skill
                .attack_target()
                .is_some_and(|target| !self.fight_actor_is_alive(target));
        if active_projectile_burst_lost_target {
            let owner_team = self
                .attacker(owner)
                .expect("skill owner identity is stable")
                .team;
            let has_alive_enemy = self
                .actors
                .values()
                .any(|actor| actor.placement.team != owner_team && actor.alive());
            if let Some(actor) = self.moving_mut(owner) {
                actor.stop_in_place(true);
            }
            if !has_alive_enemy {
                self.skill_mut(owner).projectile_pending_releases.clear();
                return Ok(Flow::Done);
            }
            let mut due = Vec::new();
            self.skill_mut(owner)
                .projectile_pending_releases
                .retain(|pending| {
                    if pending.step <= step {
                        due.push(*pending);
                        false
                    } else {
                        true
                    }
                });
            for pending in due {
                self.release_pending_projectile(owner, pending, events)?;
            }
            return Ok(Flow::Done);
        }
        Ok(Flow::Next)
    }

    /// Moves the skill's state on where its time is up: a backswing that has
    /// ended, a prepare that has ended. Answers whether the prepare ended.
    fn advance_skill_state(
        &mut self,
        owner: FightActorRef,
        step: u64,
        backswing_just_finished: bool,
        quick_switch_backswing_due: bool,
        quick_switch_dead_backswing_due: bool,
    ) -> bool {
        let skill = self.skill_mut(owner);
        if quick_switch_backswing_due && !quick_switch_dead_backswing_due {
            skill.set_backswing_finish_step(None);
        }
        if backswing_just_finished {
            skill.set_backswing_finish_step(None);
            // The attack state outlives the backswing: `SkillAttackController`
            // has no phase running until the next blow's wait begins, and the
            // checker is asked in that gap.
            skill.set_phase(FightSkillPhase::Attack);
        }
        let prepare_finished = matches!(
            skill.phase(),
            FightSkillPhase::Prepare { finish_step } if finish_step <= step
        );
        if prepare_finished {
            skill.set_phase(FightSkillPhase::Attack);
        }
        prepare_finished
    }

    /// The blow being wound up follows what the skill fires at, and a bodyless
    /// one whose target left its attack area is dropped.
    fn retarget_pending_blow(&mut self, owner: FightActorRef) -> Flow {
        let attacker = self
            .attacker(owner)
            .expect("skill owner identity is stable");
        let follows = attacker.has_body && attacker.attack.quick_switch_target;
        let skill = self.skill(owner);
        let bodyful_quick_switch_target = (follows && skill.pending().is_some())
            .then_some(skill.attack_target())
            .flatten()
            .filter(|&target_id| self.target_in_attack_area(owner, target_id));
        if let Some(target_id) = bodyful_quick_switch_target {
            self.skill_mut(owner)
                .pending_mut()
                .expect("pending attack identity is stable")
                .target = target_id;
        }
        let active_attack_rejected = self
            .skill(owner)
            .pending()
            .is_some_and(|pending| self.bodyless_attackable_invalid(owner, pending.target));
        if active_attack_rejected {
            // The build's SkillPrepareState and SkillAttackState both run
            // CheckAttackable before advancing their current attack phase.
            // A failed check enters SkillIdleState in the same update.
            let skill = self.skill_mut(owner);
            skill.drop_lock();
            skill.set_pending(None);
            skill.set_phase(FightSkillPhase::Idle);
            if let Some(actor) = self.moving_mut(owner) {
                actor.stop_in_place(true);
            }
            return Flow::Done;
        }
        Flow::Next
    }

    /// Performs what is due this update: the blow wound up, the shots of a
    /// burst, a grouped skill's core and its slots. Answers whether the blow
    /// was rejected at its attack point.
    fn perform_due_blows(
        &mut self,
        owner: FightActorRef,
        step: u64,
        events: &mut Vec<Event>,
    ) -> Result<(Flow, bool)> {
        let released_this_step = self
            .skill(owner)
            .pending()
            .is_some_and(|pending| pending.step <= step);
        let attack_point_rejected = if released_this_step {
            self.release(owner, events)?
        } else {
            false
        };
        if released_this_step && self.motion_state(owner) != MotionState::Attacking {
            return Ok((Flow::Done, attack_point_rejected));
        }
        let mut due = Vec::new();
        self.skill_mut(owner)
            .projectile_pending_releases
            .retain(|pending| {
                if pending.step <= step {
                    due.push(*pending);
                    false
                } else {
                    true
                }
            });
        for pending in due {
            self.release_pending_projectile(owner, pending, events)?;
        }
        let grouped = self
            .attacker(owner)
            .expect("skill owner identity is stable")
            .attack
            .weapons
            .mode
            == WeaponMode::Group;
        if grouped {
            let actor_id = owner.unit_id().expect("only a unit's skill is grouped");
            self.perform_group_blows(actor_id, step, events)?;
        }
        Ok((Flow::Next, attack_point_rejected))
    }

    /// Whether a target is within the attack angle of the unit's own
    /// rotation, whatever its weapons point at: the angle a unit without a
    /// body is measured by, and a grouped skill's other slots.
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

    /// Whether a blow wound up by an owner without a body has lost what it
    /// was wound up for: at the attack point `SkillAttackState` checks the
    /// attack area again, measured from the root, where an owner whose weapons
    /// turn on their own goes on.
    pub(in crate::fight) fn bodyless_attackable_invalid(
        &self,
        owner: FightActorRef,
        target: FightActorRef,
    ) -> bool {
        let has_body = self
            .attacker(owner)
            .is_some_and(|attacker| attacker.has_body);
        !has_body && !self.target_in_attack_area(owner, target)
    }

    /// Schedules the next attack and remembers the interval it used.
    pub(in crate::fight) fn sample_actor_attack_interval(
        &mut self,
        actor_id: u64,
        step: u64,
    ) -> Result<u64> {
        let owner = FightActorRef::Unit(actor_id);
        let sampled = self.draw_attack_interval(owner)?;
        self.skill_mut(owner).current_attack_interval = sampled;
        Ok(step.saturating_add(sampled))
    }
}
