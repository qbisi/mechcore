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
//! `FPoint`'s comparisons count 43 raw as equal, so twenty ticks reach a
//! second and two reach 0.1 second, though each falls a few raw short.
//! `docs/rules/combat.md` states the rule.

use super::*;

/// `TeamAutoRecoveryManager.AutoRecoveryController`'s two clocks, Q32.32
/// seconds.
#[derive(Debug, Clone, Copy)]
pub(in crate::fight) struct RecoveryClock {
    start_q32: i64,
    recovery_q32: i64,
}

impl RecoveryClock {
    /// `AutoRecoveryController.Reset`: the start clock at −1 second, the
    /// repair clock at zero.
    pub(in crate::fight) const fn reset() -> Self {
        Self {
            start_q32: -Q32_ONE,
            recovery_q32: 0,
        }
    }
}

impl Simulation {
    /// `AutoRecoverySystem.Update`: each side's `TeamAutoRecoveryManager.
    /// Update`, blue's first, its controllers in the order their units joined.
    ///
    /// A source that `CanDisable` is disabled while its unit's technologies
    /// are (`AutoRecoveryEffectProvider.DisableEffect`), and its clocks stand
    /// still.
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
            let Some(source) = actor.placement.auto_recovery else {
                continue;
            };
            if source.can_disable && actor.technology_disabled() {
                continue;
            }
            let max_life = actor.stats.max_life();
            if !actor.alive() || actor.life >= max_life {
                continue;
            }
            let Some(clock) = actor.recovery.as_mut() else {
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
