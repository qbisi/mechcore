//! `GRTimerManager`: callbacks share registration order across their owners.

use super::*;

/// The callbacks whose timers the simulator implements.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::fight) enum Callback {
    Summon,
    Diffusion,
    Chain,
    Trench,
    Fly(u64),
    Melee(u64),
}

/// `activeTimers`, ordered as `AddTimer` appends them. Each owner retains its
/// timer's counters; the manager decides which callback observes the world first.
#[derive(Default)]
pub(in crate::fight) struct Timers {
    next: u64,
    active: BTreeMap<u64, Callback>,
}

impl Timers {
    /// `GRTimerManager.AddTimer`.
    pub(in crate::fight) fn start(&mut self, callback: Callback) -> u64 {
        self.next += 1;
        self.active.insert(self.next, callback);
        self.next
    }

    /// `TryStopByObject` for callbacks whose owner has only one active timer.
    pub(in crate::fight) fn stop(&mut self, callback: Callback) {
        self.active.retain(|_, running| *running != callback);
    }
}

impl Simulation {
    /// `GRTimerManager.Update`: a timer appended by a callback is updated on
    /// this tick too; a stopped timer is absent when its turn would arrive.
    pub(in crate::fight) fn update_timers(
        &mut self,
        step: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let mut previous = 0;
        while let Some((&timer, &callback)) = self.timers.active.range((previous + 1)..).next() {
            self.timers.active.remove(&timer);
            let running = match callback {
                Callback::Summon => self.join_summon(timer, step, events)?,
                Callback::Diffusion => self.update_diffusion(timer, events)?,
                Callback::Chain => self.update_chain(timer, events)?,
                Callback::Trench => self.idle_after_trench(timer, step),
                Callback::Fly(unit) => self.finish_fly_transition(unit, step),
                Callback::Melee(unit) => self.finish_melee_transition(unit, step)?,
            };
            if running {
                self.timers.active.insert(timer, callback);
            }
            previous = timer;
        }
        Ok(())
    }
}
