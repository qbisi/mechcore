//! `UndergroundMoveAbility`: a unit that burrows to move and surfaces to
//! attack, and the `TransitionState` its motion holds while it does.
//!
//! `MotionController` changes to `MotionMoveState`, and from it, through the
//! `TransitionState` of its `MotionFSM` whenever the unit has a move ability
//! (`ChangeToMoveState` unless already underground, `ChangeToAttackState`
//! always, `MotionMoveState.ChangeToIdle` always). The state's `Enter` hands
//! the ability the state left and the one to come (`OnTransitionBegin`): into
//! the move state it burrows (`EnterMoveBegin`), out of it it surfaces
//! (`ExitMoveBegin`), and otherwise it does nothing. The state's `Update`
//! asks the ability's `Update` whether its time is up, and its `Exit` ends
//! the burrow or the surfacing (`OnTransitionEnd`) before the next state is
//! entered, which is not updated on that tick.
//!
//! Both ends stop the unit's skills (`SkillManager.Deactive`) and lock its
//! agent, and a burrowed unit is hidden, which makes it no target. Neither is
//! updated by the motion while it lasts. `docs/rules/mobility.md` says what
//! the recordings showed.

use super::skill::Flow;
use super::*;

/// `MoveAbility.MoveState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) enum AbilityState {
    None,
    /// Burrowing: `EnterMoveBegin` to `EnterMoveEnd`.
    Enter,
    /// Underground.
    Moving,
    /// Surfacing: `ExitMoveBegin` to `ExitMoveEnd`.
    Exit,
}

/// `RVOControllerFixed.Lock(false, false)`: the agent stands still on collider
/// priority 11 at full priority until the move ability lets it go.
const LOCKED_COLLIDER_PRIORITY: i32 = 11;

/// One unit's `UndergroundMoveAbility`.
#[derive(Debug, Clone)]
pub(in crate::fight) struct Underground {
    enter_q32: i64,
    exit_q32: i64,
    exit_keep_q32: i64,
    /// `undergroundExitRange`, space units: how near its lock a unit moving
    /// below surfaces.
    exit_range: i64,
    pub(in crate::fight) state: AbilityState,
    /// `time`, Q32.32 seconds since the burrow or the surfacing began.
    time_q32: i64,
    /// `translationTime`: how long the burrow or the surfacing lasts.
    translation_q32: i64,
    /// `NormalExitMoveBehviour.exitKeepTime` while one is surfacing: the unit
    /// shows itself once `time` reaches it.
    showing_q32: Option<i64>,
    /// `RVOControllerFixed`'s collider record: the agent is locked.
    pub(in crate::fight) agent_locked: bool,
    /// `RVOAgentFixed.mainLayer` is `Underground`.
    pub(in crate::fight) below: bool,
}

impl Underground {
    /// `UndergroundMoveAbility.IsLockTargetInRange` for a unit moving below:
    /// whether its lock, `edge_distance_q32` off, is within the exit range.
    pub(in crate::fight) fn lock_in_exit_range(&self, edge_distance_q32: i64) -> Option<bool> {
        (self.state == AbilityState::Moving)
            .then(|| !rvo::fpoint_greater_than(edge_distance_q32, space_to_q32(self.exit_range)))
    }

    pub(in crate::fight) fn of(config: &crate::rules::UndergroundConfig) -> Self {
        // Time units, two thousand a second, as Q32.32 seconds.
        let seconds = |units: u64| {
            i64::try_from(
                i128::from(units) * i128::from(Q32_ONE)
                    / i128::from(crate::rules::TIME_UNITS_PER_SECOND_SCALE),
            )
            .expect("a configured time fits Q32.32")
        };
        Self {
            enter_q32: seconds(config.enter_time_units()),
            exit_q32: seconds(config.exit_time_units()),
            exit_keep_q32: seconds(config.exit_keep_time_units()),
            exit_range: config.attack_range(),
            state: AbilityState::None,
            time_q32: 0,
            translation_q32: 0,
            showing_q32: None,
            agent_locked: false,
            below: false,
        }
    }
}

/// What the agent of a unit with a move ability reads instead of its own
/// profile while the ability holds it.
#[derive(Debug, Clone, Copy)]
pub(in crate::fight) struct AgentOverride {
    pub(in crate::fight) main_layer: Option<i32>,
    pub(in crate::fight) locked: Option<(i32, i64)>,
}

impl Actor {
    /// The agent overrides the move ability holds, if any.
    pub(in crate::fight) fn agent_override(&self) -> AgentOverride {
        let Some(underground) = &self.underground else {
            return AgentOverride {
                main_layer: None,
                locked: None,
            };
        };
        AgentOverride {
            main_layer: underground.below.then_some(0),
            locked: underground
                .agent_locked
                .then_some((LOCKED_COLLIDER_PRIORITY, Q32_ONE)),
        }
    }

    /// Whether the unit's agent is locked: it stands where it is.
    pub(in crate::fight) fn agent_locked(&self) -> bool {
        self.underground
            .as_ref()
            .is_some_and(|underground| underground.agent_locked)
    }

    /// `SkillManager.Deactive`: every skill stops its attack and is left idle
    /// with no lock, and none updates until the manager is active again.
    /// `FightSkill.StopAttack` clears the lock and not the attack target, so
    /// the weapon still names what it fired at.
    fn deactivate_skills(&mut self) {
        self.skills_active = false;
        self.skills.main.attack_target_left = self.skills.main.attack_target();
        self.skills.main.set_pending(None);
        self.skills.main.drop_lock();
        self.skills.main.set_phase(FightSkillPhase::Idle);
        self.skills.main.clear_slots();
        self.skills.main.performer.stop();
    }
}

impl Actor {
    /// `MotionController.ExitFight`'s part for a move ability: the motion
    /// changes to `MotionIdleState` without a transition, and
    /// `UndergroundMoveAbility.Clear` ends a burrow there and then
    /// (`DoExitMoveBegin(false)` and `DoExitMoveEnd(false)`): the unit shows
    /// itself and its agent is back on the ground. `DoExitMoveBegin` locked the agent, and `DoExitMoveEnd`
    /// lets it go only on a normal exit, so it stays locked: a Sandworm that
    /// was burrowing on when the fight ended stands still on its last tick,
    /// while every other unit moves. A transition under way is ended as its
    /// state's `Exit` ends it.
    pub(in crate::fight) fn exit_fight_move_ability(&mut self) {
        if self.underground.is_none() {
            return;
        }
        if self.motion.state == MotionState::Transitioning {
            self.end_transition_state();
        }
        let underground = self.underground.as_mut().expect("checked above");
        if underground.state == AbilityState::Moving {
            self.visibility = Visibility::Normal;
            underground.agent_locked = true;
            underground.below = false;
            underground.showing_q32 = None;
        }
        underground.state = AbilityState::None;
        underground.time_q32 = 0;
        underground.translation_q32 = 0;
    }
}

impl Simulation {
    /// Whether changing the motion from `from` to `to` passes through the
    /// `TransitionState`: `MotionController.ChangeToMoveState` unless the unit
    /// is already underground, `ChangeToAttackState` always, and
    /// `MotionMoveState.ChangeToIdle`.
    pub(in crate::fight) fn transits(
        &self,
        actor_id: u64,
        from: MotionState,
        to: MotionState,
    ) -> bool {
        let Some(underground) = &self.actors[&actor_id].underground else {
            return false;
        };
        match to {
            MotionState::Moving => underground.state != AbilityState::Moving,
            MotionState::Attacking => true,
            MotionState::Idle => from == MotionState::Moving,
            MotionState::Stopped | MotionState::Transitioning => false,
        }
    }

    /// `MotionFSM`'s change into its `TransitionState`, whose `Enter` hands
    /// the ability the state left and the one to come.
    pub(in crate::fight) fn begin_transition(
        &mut self,
        actor_id: u64,
        from: MotionState,
        to: MotionState,
    ) {
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        actor.motion.state = MotionState::Transitioning;
        actor.motion.transition_to = Some(to);
        actor.motion.attack_hold_fire = false;
        let underground = actor
            .underground
            .as_mut()
            .expect("only a unit with a move ability transits");
        if to == MotionState::Moving {
            // `UndergroundMoveAbility.EnterMoveBegin`.
            underground.state = AbilityState::Enter;
            underground.translation_q32 = underground.enter_q32;
            underground.time_q32 = 0;
            underground.agent_locked = true;
            actor.deactivate_skills();
        } else if from == MotionState::Moving {
            // `MoveAbility.ExitMoveBegin` and `DoExitMoveBegin(true)`.
            underground.state = AbilityState::Exit;
            underground.time_q32 = 0;
            underground.translation_q32 = underground.exit_q32;
            underground.agent_locked = true;
            underground.showing_q32 = Some(underground.exit_keep_q32);
            actor.deactivate_skills();
        }
    }

    /// `TransitionState.Update`: the ability's `Update`, and once its time is
    /// up the change to the next state. `Done` while the unit is in the
    /// state, which is the whole of its motion's update.
    pub(in crate::fight) fn update_transition(&mut self, actor_id: u64) -> Flow {
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        if actor.motion.state != MotionState::Transitioning {
            return Flow::Next;
        }
        let underground = actor
            .underground
            .as_mut()
            .expect("only a unit with a move ability transits");
        // `UndergroundMoveAbility.Update`.
        underground.time_q32 = underground.time_q32.saturating_add(NATIVE_LOGIC_DELTA_Q32);
        if let Some(showing_q32) = underground.showing_q32
            && rvo::fpoint_greater_or_equal(underground.time_q32, showing_q32)
        {
            underground.showing_q32 = None;
            actor.visibility = Visibility::Normal;
        }
        if !rvo::fpoint_greater_or_equal(underground.time_q32, underground.translation_q32) {
            return Flow::Done;
        }
        self.end_transition(actor_id);
        Flow::Done
    }

    /// `TransitionState.Exit`, the ability's `OnTransitionEnd`, and the next
    /// state entered.
    fn end_transition(&mut self, actor_id: u64) {
        self.actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .end_transition_state();
    }
}

impl Actor {
    /// `TransitionState.Exit`, the ability's `OnTransitionEnd`, and the next
    /// state entered, `range` what its main skill's `GetAttackRange` answers.
    fn end_transition_state(&mut self) {
        let alive = self.alive();
        let underground = self
            .underground
            .as_mut()
            .expect("only a unit with a move ability transits");
        match underground.state {
            AbilityState::Exit => {
                // `ExitMoveEnd` and `DoExitMoveEnd(true)`: the agent let go
                // and back on the ground.
                underground.state = AbilityState::None;
                underground.agent_locked = false;
                underground.below = false;
                self.skills_active = true;
            }
            AbilityState::Enter => {
                // `UndergroundMoveAbility.EnterMoveEnd(IsAlive)`.
                self.skills_active = true;
                underground.agent_locked = false;
                if alive {
                    underground.state = AbilityState::Moving;
                    underground.below = true;
                    self.visibility = Visibility::Hide;
                }
            }
            AbilityState::None | AbilityState::Moving => {}
        }
        let next = self
            .motion
            .transition_to
            .take()
            .expect("a transition knows the state it leads to");
        self.motion.state = next;
        // `MotionIdleState.Enter` and `MotionAttackState.Enter` stop the
        // agent where it stands.
        if matches!(next, MotionState::Idle | MotionState::Attacking) {
            self.motion.next_target_x_q32 = self.x_q32;
            self.motion.next_target_z_q32 = self.z_q32;
            self.motion.next_speed_q32 = 0;
            self.motion.next_max_speed_q32 = self.rvo_max_speed_q32;
        }
    }
}
