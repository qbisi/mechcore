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
//! (`FightCoreSystem.TryDstroyTower`, `IsStepFinish`) while it waits. Each
//! update after, the same tick first, the task counts one
//! (`RebirthTask.Update`; a task that ends leaves the one after it uncounted
//! on that update), and once it has counted the source's whole seconds
//! over a tick the unit stands again where it fell (`RebirthTask.RebirthMech`):
//! its life refilled (`FightMech.ForceRecoveryLife`), back among its side's
//! active units (`FightTeam.ActiveMech`), its effects active again
//! (`FightEffectSystem.ActiveEffect`), entering the fight
//! (`FightMech.EnterFight`), and counted reborn (`AddRebirthCount`). The
//! fight's end drops every task (`OnFightExit`).
//! `docs/rules/technology_effects.md` states the rule.

use super::*;
use mechcore_mcfr::RebirthState;

/// A `RebirthTask`.
#[derive(Debug, Clone)]
struct Task {
    unit: u64,
    /// `taskTime`: the updates it has counted.
    time: i64,
    /// `rebirthCostTime`: the source's whole seconds over a tick.
    cost: i64,
    /// `RebirthSurvival.worldPos`: where the unit fell, as a recording
    /// stands it.
    position: QVec3,
}

/// `DeadRebirthController`'s tasks and counts.
#[derive(Debug, Clone, Default)]
pub(in crate::fight) struct RebirthSystem {
    /// `rebirthCount`: the rebirths each unit has left, from its first death.
    left: BTreeMap<u64, i64>,
    /// `rebirthTasks`, in the order they started.
    tasks: Vec<Task>,
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
            let task = &mut self.rebirth.tasks[index];
            task.time += 1;
            if task.time >= task.cost {
                let unit = task.unit;
                self.rebirth.tasks.remove(index);
                self.rebirth_mech(unit)?;
            }
            index += 1;
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
        let position = actor.recorded_position();
        self.actors
            .get_mut(&unit)
            .expect("actor identity is stable")
            .rebirthing = true;
        let cost = math::q32_div(source.cost_seconds << 32, NATIVE_LOGIC_DELTA_Q32) >> 32;
        self.rebirth.tasks.push(Task {
            unit,
            time: 0,
            cost,
            position,
        });
        Ok(())
    }

    /// `RebirthTask.RebirthMech` for a unit that rises where it fell.
    fn rebirth_mech(&mut self, unit: u64) -> Result<()> {
        let actor = self
            .actors
            .get_mut(&unit)
            .expect("actor identity is stable");
        actor.life = actor.stats.max_life();
        actor.rebirthing = false;
        actor.rebirth_count += 1;
        actor.skills.ready_attack_clocks(self.step_now);
        self.plant(unit);
        self.active_effect(unit)?;
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
                position: task.position,
            })
            .collect::<Vec<_>>();
        states.sort_by_key(|state| state.unit_id);
        states
    }

    /// `DeadRebirthController.OnFightExit`: every task goes.
    pub(in crate::fight) fn end_rebirths_as_the_fight_ends(&mut self) {
        self.rebirth.tasks.clear();
    }
}
