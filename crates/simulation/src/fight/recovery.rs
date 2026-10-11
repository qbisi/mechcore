//! `AutoRecoverySystem`: a unit's repair while it is hurt, and the life a
//! repair or a lifesteal adds.
//!
//! A unit with an `IAutoRecovery` in force has an `AutoRecoveryController` in
//! its side's `TeamAutoRecoveryManager`. `TeamAutoRecoveryManager.Update`
//! goes through them every tick, after `SuperDeploymentSystem`: one whose
//! unit is alive and short of its maximum life adds the tick to its start
//! clock, and once that clock reaches the source's start time, to its repair
//! clock; when the repair clock reaches the source's interval it is set back
//! to zero and `RecoveryMech` repairs the unit by the whole part of its
//! maximum life times the source's rate. The start clock begins at −1 second
//! (`AutoRecoveryController.Reset`) and is never set back during a fight.
//! A source of `AutoRecoveryStateType.Underground` runs only while its unit
//! is below: its controller's condition is set as the burrow ends and
//! cleared as the surfacing begins, and its clocks stand still without it.
//! `FPoint`'s comparisons count 43 raw as equal, so twenty ticks reach a
//! second and two reach 0.1 second, though each falls a few raw short.
//! `docs/rules/combat.md` states the rule.

use super::*;
use crate::modifier::{AutoRecovery, RecoveryState};

/// `TeamAutoRecoveryManager.AutoRecoveryController`'s two clocks, Q32.32
/// seconds.
#[derive(Debug, Clone, Copy)]
pub(in crate::fight) struct RecoveryClock {
    start_q32: i64,
    recovery_q32: i64,
    /// `isCondition`: whether the unit is in the source's state.
    pub(in crate::fight) condition: bool,
    /// `isEnable`: cleared while a technology of the unit that is an
    /// `IAutoRecovery` and `CanDisable` is switched off.
    enabled: bool,
}

impl RecoveryClock {
    /// `AutoRecoveryController.Reset`: the start clock at −1 second, the
    /// repair clock at zero, enabled, and in condition only for a source of
    /// `AutoRecoveryStateType.Normal`.
    pub(in crate::fight) fn reset(source: &AutoRecovery) -> Self {
        Self {
            start_q32: -Q32_ONE,
            recovery_q32: 0,
            condition: source.state == RecoveryState::Normal,
            enabled: true,
        }
    }
}

impl Simulation {
    /// `AutoRecoveryEffectProvider.DoActive` as a unit's effects are
    /// activated: `AutoRecoverySystem.AddMech` gives a unit with a repair and
    /// no controller a new one, reset. A unit arriving from a flank starts its
    /// repair clocks then.
    pub(in crate::fight) fn add_auto_recovery(&mut self, unit: u64) {
        let Some(actor) = self.actors.get_mut(&unit) else {
            return;
        };
        if actor.recovery.is_none()
            && let Some(source) = &actor.placement.effects.auto_recovery
        {
            actor.recovery = Some(RecoveryClock::reset(source));
        }
    }

    /// `AutoRecoverySystem.Update`: each side's `TeamAutoRecoveryManager.
    /// Update`, blue's first, its controllers in the order their units joined.
    /// A controller disabled or out of its condition is passed over, its
    /// clocks standing still (`AutoRecoveryController.IsPass`).
    pub(in crate::fight) fn step_auto_recovery(&mut self, events: &mut Vec<Event>) -> Result<()> {
        let mut repaired = self
            .actors
            .iter()
            .filter(|(_, actor)| actor.recovery.is_some())
            .map(|(&id, actor)| (actor.placement.team, id))
            .collect::<Vec<_>>();
        repaired.sort_unstable();
        for (_, id) in repaired {
            let actor = self.actors.get_mut(&id).expect("actor identity is stable");
            let Some(source) = actor.placement.effects.auto_recovery else {
                continue;
            };
            let max_life = actor.stats.max_life();
            if !actor.alive() || actor.life >= max_life {
                continue;
            }
            let Some(clock) = actor
                .recovery
                .as_mut()
                .filter(|clock| clock.enabled && clock.condition)
            else {
                continue;
            };
            clock.start_q32 = clock.start_q32.saturating_add(NATIVE_LOGIC_DELTA_Q32);
            // `FPoint.op_GreaterThanOrEqual`, as tolerant as its `<=`.
            if !fpoint_less_or_equal(source.start_time_q32, clock.start_q32) {
                continue;
            }
            clock.recovery_q32 = clock.recovery_q32.saturating_add(NATIVE_LOGIC_DELTA_Q32);
            if !fpoint_less_or_equal(source.duration_q32, clock.recovery_q32) {
                continue;
            }
            clock.recovery_q32 = 0;
            // `RecoveryMech` hands `FightMech.RecoveryLife` the whole part.
            let repair = q32_mul(max_life << 32, source.life_rate_q32) >> 32;
            self.add_life(id, repair, events)?;
        }
        Ok(())
    }

    /// `AutoRecoveryEffectProvider.DisableEffect` and `EnableEffect`
    /// (`AutoRecoverySystem.DisableMech`, `EnableMech`): the unit's
    /// controller disabled or enabled, whichever of its sources is in force.
    pub(in crate::fight) fn switch_auto_recovery(&mut self, unit: u64, on: bool) {
        let actor = self
            .actors
            .get_mut(&unit)
            .expect("actor identity is stable");
        if let Some(clock) = actor.recovery.as_mut() {
            clock.enabled = on;
        }
    }

    /// `FightMech.ForceRecoveryLife`: `FightActor.AddLife` of the unit's
    /// maximum whatever holds its recovery off, with no life bar shown, so a
    /// recording holds no `healing` for it.
    pub(in crate::fight) fn force_add_life(&mut self, id: u64, value: i64) {
        let actor = self.actors.get_mut(&id).expect("actor identity is stable");
        actor.life = (actor.life + value).min(actor.stats.max_life());
    }

    /// `FightMech.RecoveryLife` and `FightMech.StealLife`: `FightActor.
    /// AddLife`, capped at the unit's maximum, recorded as a `healing` event
    /// on the unit for the life it gained. Both give nothing while a buff
    /// disables recovery (`BuffManager.IsRecoverDisabled`).
    pub(in crate::fight) fn add_life(
        &mut self,
        id: u64,
        value: i64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let actor = self.actors.get_mut(&id).expect("actor identity is stable");
        if actor.recover_disabled() {
            return Ok(());
        }
        let before = actor.life;
        actor.life = (actor.life + value).min(actor.stats.max_life());
        if actor.life > before {
            events.push(event(
                None,
                None,
                None,
                Some(actor.object_ref()),
                EventPayload::Healing {
                    amount: i32::try_from(actor.life - before)
                        .map_err(|_| Error::new("healing exceeds i32"))?,
                },
            ));
        }
        Ok(())
    }
}
