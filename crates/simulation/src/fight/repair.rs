//! `RecoveryEffectSystem`: the units a repair technology's unit repairs
//! about it every so often (a Typhoon's Maintenance Array).
//!
//! `RecoveryEffectProvider.DoActive` adds the unit to the system's
//! `recoveryOwnerInfos` (`AddRecoveryOwner`), whose clock counts once the
//! fight is on: `OnEnterFight` sets every clock at zero and enables it, and
//! one added during the fight, a unit arriving from its travel, rising again
//! or made, starts at zero and enabled. `DoDeactive`, as the unit dies,
//! takes it away. Each update while fighting, one module after
//! `StealthTechSystem`, each clock in the dictionary's order adds
//! `FightUtility.DeltaTime`, and on reaching the source's interval
//! (`FPoint.op_GreaterThanOrEqual`) goes back to zero and repairs
//! (`RecoveryDataInfo.Update`, `DoRecover`): every unit
//! `RangeTargetCalculator.CalculateRangeActors` finds within the source's
//! range of where the unit stands, its own radius counted and no building,
//! of its own side unless the source repairs enemies, and not aerial unless
//! it repairs those, gains the source's life at the repairing unit's level
//! and the source's rate of its own maximum life, the whole part of the sum
//! (`FightActor.RecoveryLife`). A disabled clock (`DisableEffect`) stays at
//! zero until it is enabled again. `docs/rules/technology_effects.md`
//! states the rule.

use super::*;
use crate::modifier::Repair;

/// `RecoveryEffectSystem.RecoveryDataInfo`.
#[derive(Debug, Clone, Copy)]
struct Info {
    owner: u64,
    /// `intervalTimeCounter`, Q32.32 seconds.
    time_q32: i64,
    /// `isEnabled`.
    enabled: bool,
}

/// `recoveryOwnerInfos`, a `Dictionary` whose entries enumerate in their
/// slots' order: a removed entry's slot is the next one an addition takes,
/// the last freed first.
#[derive(Debug, Default)]
pub(in crate::fight) struct RepairSystem {
    slots: Vec<Option<Info>>,
    free: Vec<usize>,
    /// The repairs this tick, which follow its deaths: `DeadEffectSystem`
    /// updates before it.
    events: Vec<Event>,
}

impl RepairSystem {
    fn position(&self, owner: u64) -> Option<usize> {
        self.slots
            .iter()
            .position(|slot| slot.is_some_and(|info| info.owner == owner))
    }

    /// `AddRecoveryOwner`: an owner it holds already is left as it is.
    fn add(&mut self, owner: u64, enabled: bool) {
        if self.position(owner).is_some() {
            return;
        }
        let info = Some(Info {
            owner,
            time_q32: 0,
            enabled,
        });
        match self.free.pop() {
            Some(slot) => self.slots[slot] = info,
            None => self.slots.push(info),
        }
    }
}

impl Simulation {
    /// `DoActive` of every unit the fight starts with and not travelling,
    /// then `RecoveryEffectSystem.OnEnterFight`: each clock at zero and
    /// enabled.
    pub(in crate::fight) fn enter_repair_fight(&mut self) {
        let owners = self
            .actors
            .iter()
            .filter(|(_, actor)| actor.placement.repair.is_some() && !actor.travelling)
            .map(|(&id, _)| id)
            .collect::<Vec<_>>();
        for owner in owners {
            self.repair.add(owner, true);
        }
    }

    /// `RecoveryEffectProvider.DoActive` during the fight: the unit's clock
    /// at zero and enabled.
    pub(in crate::fight) fn add_repair_unit(&mut self, unit: u64) {
        if self.actors[&unit].placement.repair.is_some() {
            self.repair.add(unit, true);
        }
    }

    /// `RecoveryEffectProvider.DoDeactive` (`RemoveRecoveryOwner`).
    pub(in crate::fight) fn remove_repair_unit(&mut self, unit: u64) {
        if let Some(slot) = self.repair.position(unit) {
            self.repair.slots[slot] = None;
            self.repair.free.push(slot);
        }
    }

    /// `RecoveryEffectProvider.DisableEffect` and `EnableEffect`
    /// (`DisableRecovery`, `EnableRecovery`).
    pub(in crate::fight) fn switch_repair(&mut self, unit: u64, on: bool) {
        if let Some(slot) = self.repair.position(unit)
            && let Some(info) = &mut self.repair.slots[slot]
        {
            info.enabled = on;
        }
    }

    /// `RecoveryEffectSystem.Update`.
    pub(in crate::fight) fn step_repair(&mut self) -> Result<()> {
        let mut events = Vec::new();
        for slot in 0..self.repair.slots.len() {
            let Some(info) = &mut self.repair.slots[slot] else {
                continue;
            };
            let owner = info.owner;
            let source = self.actors[&owner]
                .placement
                .repair
                .clone()
                .expect("a repairing unit has a source");
            if !info.enabled {
                info.time_q32 = 0;
                continue;
            }
            info.time_q32 = info.time_q32.saturating_add(NATIVE_LOGIC_DELTA_Q32);
            // `FPoint.op_GreaterThanOrEqual`, as tolerant as its `<=`.
            if !fpoint_less_or_equal(source.interval_q32, info.time_q32) {
                continue;
            }
            info.time_q32 = 0;
            self.repair_about(owner, &source, &mut events)?;
        }
        self.repair.events = events;
        Ok(())
    }

    /// The tick's repairs, as [`Self::step_repair`] made them.
    pub(in crate::fight) fn take_repair_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.repair.events)
    }

    /// `RecoveryEffectSystem.DoRecover`.
    fn repair_about(&mut self, owner: u64, source: &Repair, events: &mut Vec<Event>) -> Result<()> {
        let actor = &self.actors[&owner];
        let (position, team, level) = (
            (actor.x_q32, actor.z_q32),
            actor.placement.team,
            actor.placement.level,
        );
        let order = self.target_search_order();
        let targets = self
            .units_in_range(
                position,
                source.range_q32,
                (
                    crate::rules::AttackTargets {
                        ground: true,
                        air: true,
                    },
                    true,
                ),
                &order,
            )
            .into_iter()
            .filter(|target| {
                let target = &self.actors[target];
                (source.enemies || target.placement.team == team)
                    && (source.air || target.rules.domain != crate::rules::UnitDomain::Air)
            })
            .collect::<Vec<_>>();
        // `RecoveryTech.GetLife`: the level's entry, the last past the list.
        let life = usize::try_from(level - 1)
            .ok()
            .and_then(|index| source.life.get(index))
            .or_else(|| source.life.last())
            .copied()
            .unwrap_or_default();
        for target in targets {
            let max_life = self.actors[&target].stats.max_life();
            let repair_q32 =
                q32_mul(max_life << 32, source.max_life_rate_q32).saturating_add(life << 32);
            self.add_life(target, repair_q32 >> 32, events)?;
        }
        Ok(())
    }
}
