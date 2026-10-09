//! What strengthening a tower does to it, and what losing one writes on its
//! side.
//!
//! `config/towers.yaml`, read as [`TowersConfig`], holds the map's towers and
//! the build's strengthen and buff rows, and `docs/rules/towers.md` states the
//! rule.
//! A tower's strengthen level adds life and chooses the buff its loss writes:
//! `buffDatas` 1 to 5, one buff that differs only in how long it lasts.
//!
//! The loss is `FightTeamController.OnTowerDestoryed`: when a tower falls,
//! `BuffSystem.TryAddSpecialBuffForTeam` hands its buff to `BuffSystem.AddBuff`
//! for every live object of the side, `FightTeam.activeActors`, and each
//! reaches `BuffManager.AddBuff`. A buff already running in the same
//! `buffDivide` is not added again: `Buff.Reset` lengthens it by the new row's
//! duration in additive mode, and restarts it otherwise. `BuffManager.Update`
//! runs last in `FightMech.Update` and `FightConstruction.Update`, after the
//! skill and the motion, and a buff ends on the update its elapsed ticks reach
//! its duration. The side's objects are its units and then its constructions
//! whose row lets a tower's buff reach them.
//!
//! Each of these is an event: `buff_applied` for every object the loss
//! reaches, after the tower's `building_destroyed`, naming the running buff's
//! row and the ticks left on it; `buff_removed` when its time is up, and,
//! `cleared`, right after the `unit_died` of a unit that dies with it or right
//! before the `building_destroyed` of a construction that falls with it.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    Error, Result,
    data::{Channel, Correction, Entry, Index, Overlays},
    rules::{TowerLevel, TowersConfig},
};

use super::{LOGIC_TICK_TIME_UNITS, Simulation, TIME_UNITS_PER_SECOND, event, math::q32_div};
use crate::modifier::{DeadSummon, StackCondition};
use mechcore_mcfr::{BuffRemovedReason, Event, EventPayload, ObjectKind, ObjectRef};

/// What tags a tower's loss writes, so that its end takes it away.
pub(in crate::fight) const SOURCE: &str = "BuffSystem";

/// A buff running on a unit: `Buff.durationTime` against `maxDurationtime`,
/// both in ticks.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the buff row's flags are independent fields"
)]
pub(in crate::fight) struct RunningBuff {
    /// The `buffDatas` row it was added with, which a later one it merges
    /// into does not change, or the technology that is its own data.
    buff_id: u32,
    /// Whether `buff_id` names a technology that serves as its own buff data
    /// (`BurrowTech`) rather than a `buffDatas` row.
    technology: bool,
    divide: i32,
    additive: bool,
    elapsed: u32,
    duration: u32,
    /// `Buff.stepTime`, counting each update to `stepTimeConfig`, the row's
    /// `stepTime` in ticks, at which it goes back to none and the buff's
    /// controllers update.
    step: u32,
    step_ticks: u32,
    /// The side of `Buff.sourceTeamController`, which `Buff.Reset` does not
    /// change: what a sourceless buff is from, and the side its life change
    /// is dealt under.
    team: u32,
    /// What tags the entries it writes.
    source: &'static str,
    /// What it writes among the buffs': its row's entries, or one stack's
    /// when it stacks. Its end takes away these and leaves every other
    /// buff's, whatever module wrote them.
    entries: Vec<Entry>,
    /// Of its row's entries, those it left unwritten because its unit
    /// ignored their kind of effect as it was written (`Buff.AddEffect`
    /// passes over a `BuffDataFloatRate` whose `BuffManager.stateDatas` is
    /// above zero, its `avaliableBuffEffectRecord` left false).
    /// `Buff.Reset` writes each once the unit no longer ignores it.
    held: Vec<Entry>,
    /// `Buff.source`: the actor that first added it, which `Buff.Reset`
    /// keeps unless the buff summons.
    source_actor: Option<ObjectRef>,
    /// `IsDisableTechnology`: while it runs, `BuffManager` holds the unit's
    /// `DisableTechnology` count above zero.
    disables_technology: bool,
    /// `IsInvincible`: while it runs, `BuffManager` holds the unit's
    /// `Invincible` count above zero.
    invincible: bool,
    /// `IsDisableRecover`: while it runs, `BuffManager` holds the unit's
    /// `DisableRecover` count above zero.
    disables_recover: bool,
    /// `lifeChangeRate`, an `FPoint` raw rate: a nonzero one gives the buff
    /// `IBEC_ChangeLIfe`.
    life_change_rate: i64,
    /// `IBEC_ChangeMaxLife`'s rate, which it writes into the unit's own life
    /// rate as it enters.
    max_life_rate: i64,
    /// `IBEC_AdditiveEffectBuff`, the controller of a buff that stacks.
    stack: Option<StackStep>,
    /// `IBEC_DeadSummon`, the controller of a buff that summons as its unit
    /// dies, and what it summons.
    summons: Option<DeadSummon>,
    /// `IsClearSelfBuffWhenDisableTech`: a buff its own unit added is
    /// cleared as that unit's technologies are disabled.
    clears_when_technologies_disabled: bool,
}

impl RunningBuff {
    /// The buff as a unit's state records it.
    pub(in crate::fight) fn state(&self) -> mechcore_mcfr::BuffState {
        let ticks = |value: u32| i32::try_from(value).unwrap_or(i32::MAX);
        mechcore_mcfr::BuffState {
            data: mechcore_mcfr::BuffDataRef {
                kind: if self.technology {
                    mechcore_mcfr::BuffDataKind::Technology
                } else {
                    mechcore_mcfr::BuffDataKind::Buff
                },
                id: self.buff_id,
            },
            source: self.source_actor,
            source_team: self.team,
            elapsed: ticks(self.elapsed),
            duration: ticks(self.duration),
            step: ticks(self.step),
            stacks: ticks(self.stack.map_or(0, |stack| stack.count)),
        }
    }

    /// Its `IBEC_DeadSummon`'s summon and the buff's source.
    pub(in crate::fight) const fn dead_summon(&self) -> Option<(DeadSummon, Option<ObjectRef>)> {
        match self.summons {
            Some(summon) => Some((summon, self.source_actor)),
            None => None,
        }
    }

    /// The stacks it has written among the buffs': one for a buff that does
    /// not stack, and none before a stacking one's first step.
    fn stacks(&self) -> u32 {
        self.stack.map_or(1, |stack| stack.written)
    }

    /// What `IBEC_ChangeMaxLife` holds in the unit's own life rate: its rate
    /// as it entered, and the rate times the stack written at its last step,
    /// none at a stack of none.
    fn life_entry(&self) -> Option<Entry> {
        self.life_entry_at(
            self.stack
                .map_or(1, |stack| if stack.life_held { 0 } else { stack.life }),
        )
    }

    /// Its life rate times `stacks`, none at none.
    fn life_entry_at(&self, stacks: u32) -> Option<Entry> {
        (self.max_life_rate != 0 && stacks > 0).then(|| Entry {
            index: Index::MaxLife,
            source: self.source,
            correction: rate(self.max_life_rate.saturating_mul(i64::from(stacks))),
        })
    }

    /// Whether `ClearSelfResourceBuffByDisableTech` clears it from `unit`:
    /// `unit` added it, and its row says so.
    fn cleared_from(&self, unit: ObjectRef) -> bool {
        self.clears_when_technologies_disabled && self.source_actor == Some(unit)
    }

    /// What it wrote taken out of `overlays`: each of its entries among the
    /// buffs' once a stack, and its life rate.
    fn withdraw_from(&self, overlays: &mut Overlays) {
        for _ in 0..self.stacks() {
            for entry in &self.entries {
                overlays.channel(Channel::Buff).remove(*entry);
            }
        }
        if let Some(entry) = self.life_entry() {
            overlays.channel(Channel::Unit).remove(entry);
        }
    }
}

/// A stacking buff as it runs: the stack it has reached (`additiveStack`), the stack its rates are
/// written at (`additiveStackRecord`), which a disable clears while the
/// stack stays, the stack `IBEC_ChangeMaxLife.maxLifeChangeRate` holds its
/// rate at, one as it enters, and whether a disable has taken that rate out
/// (`isDisableTech`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StackStep {
    rule: StackRule,
    count: u32,
    written: u32,
    life: u32,
    life_held: bool,
}

/// How a buff row stacks: what a step's stack counts, and
/// `maxAdditiveStack`, none for no bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) struct StackRule {
    pub(in crate::fight) max: u32,
    pub(in crate::fight) condition: StackCondition,
    /// Whether its unit's main skill's hit sets its stack back to none.
    pub(in crate::fight) resets_on_main_hit: bool,
}

/// A buff source's chance that is never drawn: `BuffSystem.DoAddBuff` adds
/// the buff outright when `GetProbablity` is above 999.
pub(in crate::fight) const CERTAIN: i32 = 1_000;

/// One `buffDatas` row as `BuffManager.AddBuff` adds it: which it is, how it
/// merges with one already running, how long it lasts, and what it writes.
#[derive(Debug, Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the buff row's flags are independent fields"
)]
pub(in crate::fight) struct BuffRow {
    pub(in crate::fight) buff_id: u32,
    /// Whether `buff_id` names a technology that is its own buff data.
    pub(in crate::fight) technology: bool,
    /// `IsClearSelfBuffWhenDisableTech`.
    pub(in crate::fight) clears_when_technologies_disabled: bool,
    pub(in crate::fight) divide: i32,
    pub(in crate::fight) additive: bool,
    pub(in crate::fight) ticks: u32,
    /// `stepTime` in ticks, `Buff.Init`'s `stepTimeConfig`.
    pub(in crate::fight) step_ticks: u32,
    pub(in crate::fight) source: &'static str,
    pub(in crate::fight) entries: Vec<Entry>,
    pub(in crate::fight) disables_technology: bool,
    /// `IsDebuff`: a unit a buff makes invincible does not take it.
    pub(in crate::fight) debuff: bool,
    /// Its source's `GetProbablity`, in thousandths: [`CERTAIN`] or more is
    /// never drawn.
    pub(in crate::fight) probability: i32,
    /// `IsInvincible`.
    pub(in crate::fight) invincible: bool,
    /// `IsDisableRecover`.
    pub(in crate::fight) disables_recover: bool,
    /// `lifeChangeRate`, an `FPoint` raw rate of the unit's maximum life it
    /// changes by every step.
    pub(in crate::fight) life_change_rate: i64,
    /// `currentLifeDisposableChangeRate`, an `FPoint` raw rate.
    pub(in crate::fight) current_life_rate: i64,
    /// `maxLifeChangeRate`, Q32.32, which `IBEC_ChangeMaxLife` writes into
    /// the unit's own life rate rather than among the buffs'.
    pub(in crate::fight) max_life_rate: i64,
    /// How it stacks, when `IsAdditiveEffect`: its `entries` are then one
    /// stack's, of which `IBEC_AdditiveEffectBuff.GetData` answers the stack
    /// times as many, none before the first step.
    pub(in crate::fight) stacking: Option<StackRule>,
    /// What its unit summons as it dies, when `IsSummoning`.
    pub(in crate::fight) summons: Option<DeadSummon>,
}

/// The towers of both sides: what their table says, what each one's loss
/// writes, and the towers a hit emptied this tick.
pub(in crate::fight) struct TowerSystem {
    /// The tower table: what a strengthen level adds, what a loss writes.
    pub(in crate::fight) config: TowersConfig,
    /// What each tower's fall writes, by building.
    pub(in crate::fight) losses: BTreeMap<u64, TowerLoss>,
    /// The constructions a tower's loss would reach, by building.
    pub(in crate::fight) buffed_constructions: BTreeSet<u64>,
    /// The towers a hit emptied this tick, in the order they fell: the towers
    /// among `DeadEffectSystem.deadActors`, whose `OnDead` waits for that
    /// module's update.
    pub(in crate::fight) fallen: Vec<u64>,
}

/// `BuffManager`'s state the fight keeps outside the units: the buffs on
/// constructions, and the buff events a tick holds back.
#[derive(Default)]
pub(in crate::fight) struct BuffState {
    /// The buffs on constructions, by building.
    pub(in crate::fight) building_buffs: BTreeMap<u64, BuildingBuffs>,
    /// The buffs `BuffManager.Update` dropped from a unit dead this tick, to
    /// name in the `cleared` that follows its `unit_died`.
    pub(in crate::fight) dropped: BTreeMap<u64, Vec<u32>>,
    /// The `buff_applied` events a tower's loss wrote this tick, by tower, to
    /// follow its `building_destroyed`.
    pub(in crate::fight) tower_events: BTreeMap<u64, Vec<Event>>,
}

/// What one tower's fall writes: on whom, and for how many ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) struct TowerLoss {
    pub(in crate::fight) team: u32,
    pub(in crate::fight) buff_id: u32,
    pub(in crate::fight) ticks: u32,
}

impl TowersConfig {
    fn level(&self, level: u8) -> Result<&TowerLevel> {
        self.levels
            .get(usize::from(level))
            .ok_or_else(|| Error::new(format!("a tower has no strengthen level {level}")))
    }

    /// The life a tower of this level has beyond the map's own: every level
    /// up to it adds its row's.
    pub(in crate::fight) fn life_added(&self, level: u8) -> Result<i64> {
        self.level(level)?;
        Ok(self.levels[..=usize::from(level)]
            .iter()
            .map(|row| row.life)
            .sum())
    }

    /// How many ticks the loss of a tower of this level writes its buff for.
    pub(in crate::fight) fn loss_ticks(&self, level: u8) -> Result<u32> {
        let seconds = u64::from(self.level(level)?.duration);
        u32::try_from(seconds * TIME_UNITS_PER_SECOND / LOGIC_TICK_TIME_UNITS)
            .map_err(|_| Error::new("a tower's loss lasts longer than a fight holds"))
    }

    /// The `buffDatas` row the loss of a tower of this level writes.
    pub(in crate::fight) fn loss_buff(&self, level: u8) -> Result<u32> {
        Ok(self.level(level)?.buff)
    }

    /// Whether the buff reaches a construction whose row lets a tower's buff
    /// reach it.
    pub(in crate::fight) const fn reaches_constructions(&self) -> bool {
        self.destroyed_buff.can_affect_construction
    }

    /// What the buff writes, in the buff channel.
    fn entries(&self) -> Vec<Entry> {
        [
            (Index::MoveSpeed, self.destroyed_buff.move_speed_rate),
            (Index::AttackDamage, self.destroyed_buff.damage_rate),
            (
                Index::AmplifyDamage,
                self.destroyed_buff.amplify_damage_rate,
            ),
        ]
        .into_iter()
        .filter(|(_, raw)| *raw != 0)
        .map(|(index, raw)| Entry {
            index,
            source: SOURCE,
            correction: rate(raw),
        })
        .collect()
    }
}

impl super::Actor {
    /// The row of a buff running on this unit beside which `row` is not
    /// measured, if one runs: one that corrects a number `row` does other
    /// than the speed. Two buffs run side by side in `BuffManager`, and
    /// their speeds compose in `MoveSpeedProperty.Refresh`; how their other
    /// rates do, a tower's kept apart in `towerBuffDatas`, is not recorded.
    pub(in crate::fight) fn buff_not_beside(&self, row: &BuffRow) -> Option<u32> {
        let buffs = &self.stats.overlays;
        self.buffs
            .iter()
            .filter(|running| !same_buff(running, row))
            .find(|running| {
                row.entries.iter().any(|entry| {
                    entry.index != Index::MoveSpeed
                        && buffs.buff_writes(running.source, entry.index)
                })
            })
            .map(|running| running.buff_id)
    }

    /// `BuffManager.IsInvincible`: whether a running buff makes the unit
    /// invincible.
    pub(in crate::fight) fn invincible(&self) -> bool {
        self.buffs.iter().any(|running| running.invincible)
    }

    /// `FightMech.IsTechnologyDisabled`: whether a running buff holds the
    /// unit's technologies off.
    pub(in crate::fight) fn technology_disabled(&self) -> bool {
        self.buffs.iter().any(|running| running.disables_technology)
    }

    /// `IsRecoverDisabled`: whether a running buff holds the unit's
    /// recovery off.
    pub(in crate::fight) fn recover_disabled(&self) -> bool {
        self.buffs.iter().any(|running| running.disables_recover)
    }
}

impl super::Actor {
    /// `IBEC_AdditiveEffectBuff.Update` on the step of the `index`th running
    /// buff, below its bound: the stack its condition's `TryAddStack` answers,
    /// and, when that moved, its rates written at the stack
    /// (`Buff.RefreshEffect`), since a stack's enhancements sum. A buff its
    /// unit added that a disable clears adds no stack while the unit's
    /// technologies are disabled (`AddAdditiveStack`). Its life rate then
    /// follows ([`Self::step_max_life`]). A Rhino with Combat Evolvement
    /// deals 4.5% more a second and, from its second second, has 2.5% more
    /// life a second; a Steel Ball with Kinetic Charge reaches a metre further
    /// for every 7 it has rolled.
    fn step_stack(&mut self, index: usize) -> Result<()> {
        let moved_q32 = self.moved_q32;
        let held = self.technology_disabled() && self.buffs[index].cleared_from(self.object_ref());
        let running = &mut self.buffs[index];
        let Some(stack) = running.stack.as_mut() else {
            return Ok(());
        };
        if !held {
            if stack.rule.max > 0 && stack.count >= stack.rule.max {
                return Ok(());
            }
            let count = match stack.rule.condition {
                StackCondition::Time => stack.count + 1,
                // `BuffAdditiveStackConditionDistanceController.TryAddStack`:
                // the whole part of the distance over the condition's metres,
                // at most the bound.
                StackCondition::Distance { metres_q32 } => {
                    u32::try_from(q32_div(moved_q32, metres_q32) >> 32)
                        .unwrap_or(0)
                        .min(stack.rule.max)
                }
            };
            if count == stack.count {
                return Ok(());
            }
            stack.count = count;
            self.write_stacks(index, count);
        }
        if self.buffs[index].max_life_rate == 0 {
            return self.stats.refresh(&self.rules);
        }
        self.step_max_life(index, held)
    }

    /// `IBEC_ChangeMaxLife.DoAdditiveEffect`: the rate it holds taken out of
    /// the unit's own life rate and put back times the stack written, each
    /// `MechDataModifer` call refreshing the life, so the share is taken
    /// through the maximum without the buff. On the first step its unit's
    /// technologies hold it, it takes the rate out alone and refreshes the
    /// life once more at that maximum (`FightMech.RefreshLifeData`), which
    /// takes the share again; while they hold it, a step does nothing; on the
    /// first after, it puts back the rate it took out before it steps.
    fn step_max_life(&mut self, index: usize, held: bool) -> Result<()> {
        let Some(stack) = self.buffs[index].stack else {
            return Ok(());
        };
        if stack.life_held && held {
            return self.stats.refresh(&self.rules);
        }
        let held_entry = self.buffs[index].life_entry_at(stack.life);
        if let Some(step) = self.buffs[index].stack.as_mut() {
            step.life_held = held;
        }
        if stack.life_held {
            if let Some(entry) = held_entry {
                self.stats.overlays.channel(Channel::Unit).write(entry);
            }
            self.refresh_life_data()?;
        }
        if let Some(entry) = held_entry {
            self.stats.overlays.channel(Channel::Unit).remove(entry);
        }
        self.refresh_life_data()?;
        if held {
            return self.share_life(self.stats.max_life(), self.stats.max_life());
        }
        let written = stack.written;
        if let Some(step) = self.buffs[index].stack.as_mut() {
            step.life = written;
        }
        if let Some(entry) = self.buffs[index].life_entry_at(written) {
            self.stats.overlays.channel(Channel::Unit).write(entry);
            self.refresh_life_data()?;
        }
        Ok(())
    }

    /// `IBEC_AdditiveEffectBuff.RefreshAdditiveEffect`: the `index`th running
    /// buff's rates written at `stacks`, among the buffs'.
    fn write_stacks(&mut self, index: usize, stacks: u32) {
        let running = &mut self.buffs[index];
        let Some(stack) = running.stack.as_mut() else {
            return;
        };
        let written = std::mem::replace(&mut stack.written, stacks);
        let buffs = self.stats.overlays.channel(Channel::Buff);
        for _ in stacks..written {
            for entry in &running.entries {
                buffs.remove(*entry);
            }
        }
        for _ in written..stacks {
            for entry in &running.entries {
                buffs.write(*entry);
            }
        }
    }

    /// `FightMech.PerformMainSkillHitted` after a projectile of the unit's
    /// main skill hits: each stacking buff whose `BuffAdditiveStackResetHittedController`
    /// registered with it runs `IBEC_AdditiveEffectBuff.ResetAdditiveStackNormal`,
    /// its stack and the stack it is written at back to none and its rates
    /// written at none (`Buff.RefreshEffect`). Its step runs on.
    pub(in crate::fight) fn reset_stacks_on_main_hit(&mut self) -> Result<()> {
        let mut reset = false;
        for index in 0..self.buffs.len() {
            let Some(stack) = self.buffs[index].stack.as_mut() else {
                continue;
            };
            if !stack.rule.resets_on_main_hit {
                continue;
            }
            stack.count = 0;
            self.write_stacks(index, 0);
            reset = true;
        }
        if reset {
            self.stats.refresh(&self.rules)?;
        }
        Ok(())
    }

    /// `BuffManager.ClearSelfResourceBuffByDisableTech`, which
    /// `FightMech.DisableTechnology` raises after the unit's effects are
    /// switched off: each buff the unit added itself whose row clears it
    /// (`IsClearSelfBuffWhenDisableTech`), last first. One that stacks
    /// (`IsAdditiveEffect`) is written at no stack
    /// (`ResetAdditiveEffectStackByDisableTech`), which leaves its stack and
    /// its life rate until its next step; any other is removed
    /// (`BuffManager.RemoveBuff`), what it wrote taken away and the life
    /// refreshed (`Buff.Exit`), as a buff that runs out is.
    pub(in crate::fight) fn clear_self_buffs(&mut self, events: &mut Vec<Event>) -> Result<()> {
        let unit = self.object_ref();
        for index in (0..self.buffs.len()).rev() {
            if !self.buffs[index].cleared_from(unit) {
                continue;
            }
            if self.buffs[index].stack.is_some() {
                self.write_stacks(index, 0);
                continue;
            }
            let buff = self.buffs.remove(index);
            events.push(buff_removed(unit, buff.buff_id, BuffRemovedReason::Removed));
            self.withdraw_buff(&buff);
            self.refresh_life_data()?;
        }
        self.stats.refresh(&self.rules)
    }
    /// What a running buff wrote taken away: its entries among the buffs',
    /// and its rate in the unit's own life rate.
    fn withdraw_buff(&mut self, buff: &RunningBuff) {
        buff.withdraw_from(&mut self.stats.overlays);
    }

    /// The numbers again, and `FightMech.RefreshLifeData` when the maximum
    /// life moved: a unit at its whole life keeps its whole life, and any
    /// other keeps its share of it, an `FPoint` quotient times the new
    /// maximum, at least 1 while it lives.
    pub(in crate::fight) fn refresh_life_data(&mut self) -> Result<()> {
        let before = self.stats.max_life();
        self.stats.refresh(&self.rules)?;
        self.life_to_new_maximum(before)
    }

    /// `RefreshLifeData` from a maximum of `before` to the one the numbers
    /// hold now: the share is an `FPoint` quotient, rounded, and the life its
    /// product with the maximum, truncated.
    fn life_to_new_maximum(&mut self, before: i64) -> Result<()> {
        let after = self.stats.max_life();
        if after == before {
            return Ok(());
        }
        self.share_life(before, after)?;
        self.refresh_shield();
        Ok(())
    }

    /// `EnergyShieldController.Refresh`, which `RefreshLifeData` runs when
    /// asked to (`needRefreshShield`, which every caller but a stacking
    /// buff's step passes, `IBEC_ChangeMaxLife.DoAdditiveEffect`): an active
    /// shield with a maximum takes its new one from the unit's maximum life
    /// times its life rate (`RefreshMaxEnergy`), full where it was full, and
    /// otherwise its share of the old, an `FPoint` quotient, rounded, times
    /// the new, truncated. The shield is active once its provider activated
    /// it and until its unit dies (`EnergyShieldBehaviour.Active`,
    /// `Deactive`), whether enabled or not.
    fn refresh_shield(&mut self) {
        let active = self.alive() && !self.travelling;
        let max_life = self.stats.max_life();
        let Some(rate_q32) = self
            .placement
            .effects
            .energy_shield
            .map(|source| source.life_rate_q32)
        else {
            return;
        };
        let Some(shield) = self.shield.as_mut().filter(|_| active) else {
            return;
        };
        if shield.maximum < 1 {
            return;
        }
        let maximum = super::q32_mul(max_life << 32, rate_q32) >> 32;
        shield.energy = if shield.energy == shield.maximum {
            maximum
        } else {
            let old = i128::from(shield.maximum);
            let share = ((i128::from(shield.energy) << 32) + old / 2) / old;
            i64::try_from((share * i128::from(maximum)) >> 32).expect("a share of the maximum fits")
        };
        shield.maximum = maximum;
    }

    /// The life `FightMech.RefreshLifeData` leaves as the maximum moves from
    /// `before` to `after`: the whole of `after` for a unit at its whole
    /// life, and its share of it for any other, which a maximum that did not
    /// move can take one off.
    fn share_life(&mut self, before: i64, after: i64) -> Result<()> {
        self.life = if self.life >= before {
            after
        } else {
            let share =
                ((i128::from(self.life) << 32) + i128::from(before) / 2) / i128::from(before);
            let life = i64::try_from((share * i128::from(after)) >> 32)
                .map_err(|_| Error::new("a unit's life is outside i64"))?;
            if self.life > 0 { life.max(1) } else { life }
        };
        Ok(())
    }
}

impl Simulation {
    /// Whether this target is one of the map's towers (`FightCrystal.IsTower`),
    /// which is an actor of its own rather than one block of a construction.
    pub(in crate::fight) fn is_tower(&self, target: super::FightActorRef) -> bool {
        matches!(target, super::FightActorRef::Building(id) if self.towers.losses.contains_key(&id))
    }

    /// `FightTeamController.OnTowerDestoryed`: the fallen building's buff, on
    /// every live object of its side, `FightTeam.activeActors`: its units,
    /// then its constructions whose row lets a tower's buff reach them.
    pub(in crate::fight) fn lose_tower(&mut self, building_id: u64) -> Result<()> {
        let Some(loss) = self.towers.losses.get(&building_id).copied() else {
            return Ok(());
        };
        if let Some(interceptor) = self.standing_interceptor(loss.team) {
            return Err(Error::new(format!(
                "team {} loses a tower while its interceptor {interceptor} stands, and what a \
                 tower's loss writes on an interceptor is not measured",
                loss.team
            )));
        }
        let row = BuffRow {
            buff_id: loss.buff_id,
            technology: false,
            clears_when_technologies_disabled: self
                .towers
                .config
                .destroyed_buff
                .clear_when_technologies_disabled,
            max_life_rate: 0,
            stacking: None,
            summons: None,
            divide: self.towers.config.destroyed_buff.buff_divide,
            additive: self.towers.config.destroyed_buff.additive,
            ticks: loss.ticks,
            // `extract-towers.py` refuses a tower buff that steps.
            step_ticks: 0,
            source: SOURCE,
            entries: self.towers.config.entries(),
            disables_technology: false,
            debuff: self.towers.config.destroyed_buff.debuff,
            probability: CERTAIN,
            invincible: false,
            disables_recover: false,
            life_change_rate: 0,
            current_life_rate: 0,
        };
        let mut applied = Vec::new();
        // `activeActors` holds a side's units in the order they joined it: a
        // unit a beam turned onto it comes after every unit already there.
        let actor_ids = self
            .units_in_update_order()
            .into_iter()
            .filter(|id| {
                let actor = &self.actors[id];
                actor.placement.team == loss.team && actor.alive()
            })
            .collect::<Vec<_>>();
        for actor_id in actor_ids {
            if self.buff_reaches(actor_id, &row)? {
                self.write_buff(actor_id, None, loss.team, &row, &mut applied)?;
            }
        }
        let reached = if self.towers.config.reaches_constructions() {
            self.buildings
                .iter()
                .filter(|building| {
                    building.team_id == loss.team
                        && building.life.current > 0
                        && self
                            .towers
                            .buffed_constructions
                            .contains(&building.building_id)
                })
                .map(|building| building.building_id)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        for construction_id in reached {
            let buffed = self
                .buffs
                .building_buffs
                .entry(construction_id)
                .or_default();
            let (running, added, written) =
                add_buff(&mut buffed.buffs, &row, (None, false), loss.team, false);
            applied.push(buff_applied(
                ObjectRef::new(ObjectKind::Building, construction_id),
                loss.team,
                &running,
            ));
            if added {
                for entry in &written {
                    buffed.overlays.channel(Channel::Buff).write(*entry);
                }
                self.refresh_construction(construction_id)?;
            }
        }
        self.buffs.tower_events.insert(building_id, applied);
        Ok(())
    }

    /// `BuffManager.AddBuff` on a unit: the row added, or merged into the one
    /// of its divide already running, and what it writes on the unit's buff
    /// channel. The `buff_applied` it answers names the running buff.
    /// `BuffSystem.DoAddBuff` before it hands a buff to the unit's
    /// `BuffManager.AddBuff`, and the head of that: whether the buff reaches
    /// the unit at all. A buff the unit ignores (`FightMech.IsIgnoredBuff`)
    /// does not, nor does a debuff a running buff makes it invincible to, and
    /// nothing is recorded. Of one that does, a source's chance of none
    /// adds nothing, a certain one adds it, and any other is drawn from the
    /// unit's side's stream: `GRRandom.IsProbabilityFail` fails when
    /// `Next(1000)` is not below the chance.
    pub(in crate::fight) fn buff_reaches(&mut self, actor_id: u64, row: &BuffRow) -> Result<bool> {
        let actor = &self.actors[&actor_id];
        if (!row.technology && actor.placement.effects.ignored_buffs.contains(&row.buff_id))
            || (row.debuff && actor.invincible())
            || row.probability <= 0
        {
            return Ok(false);
        }
        if row.probability >= CERTAIN {
            return Ok(true);
        }
        let team = actor.placement.team;
        let draw = self
            .side_random(team)?
            .next_between_inclusive(0, CERTAIN - 1);
        Ok(draw < row.probability)
    }

    /// `BuffManager.AddBuff` of a buff that reaches the unit, recorded with
    /// the object that wrote it when one did, after what the buff did as it
    /// was written: a new buff's `Enter` and a running one's
    /// `ReEnableDisposableEffect` both perform its once-only effects.
    pub(in crate::fight) fn write_buff(
        &mut self,
        actor_id: u64,
        source: Option<ObjectRef>,
        team: u32,
        row: &BuffRow,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        // `Buff.Reset` of a buff that summons: the unit that adds it again
        // becomes its source when its level is above the source's.
        let level = |unit: Option<ObjectRef>| {
            unit.filter(|unit| unit.kind == ObjectKind::Unit)
                .and_then(|unit| self.actors.get(&unit.id))
                .map(|actor| actor.placement.level)
        };
        let takes_source = row.summons.is_some()
            && self.actors[&actor_id]
                .buffs
                .iter()
                .find(|running| same_buff(running, row))
                .is_some_and(|running| {
                    matches!((level(running.source_actor), level(source)), (Some(was), Some(now)) if was < now)
                });
        let unmeasured = &self.actors[&actor_id]
            .placement
            .effects
            .technology_disable
            .unmeasured;
        if row.disables_technology && !unmeasured.is_empty() {
            return Err(Error::new(format!(
                "buff {} disables the technologies of unit {actor_id}, which carries {}, and \
                 switching that off mid-fight is not measured",
                row.buff_id,
                unmeasured.join(" and ")
            )));
        }
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        if row.stacking.is_some()
            && actor.ignores_speed_rate
            && row.entries.iter().any(is_speed_rate)
        {
            return Err(Error::new(format!(
                "buff {} stacks a speed rate on unit {actor_id}, which ignores speed rates, \
                 and `Buff.RefreshEffect` passing over it is not measured",
                row.buff_id
            )));
        }
        let was_disabled = actor.technology_disabled();
        // `BuffManager.AddBuff` raises the unit's `DisableTechnology` count,
        // and with it switches its technologies off
        // (`TryAddCommonEffectController`), before `Buff.AddEffect`: a buff
        // that disables an ignoring technology writes what it ignored.
        let ignores_speed_rate = actor.ignores_speed_rate
            && !(row.disables_technology
                && !was_disabled
                && actor
                    .placement
                    .effects
                    .technology_disable
                    .providers
                    .contains(&crate::modifier::EffectProvider::IgnoreBuff));
        let (running, added, written) = add_buff(
            &mut actor.buffs,
            row,
            (source, takes_source),
            team,
            ignores_speed_rate,
        );
        if row.stacking.is_none() {
            for entry in &written {
                actor.stats.overlays.channel(Channel::Buff).write(*entry);
            }
            if !added && !written.is_empty() {
                actor.stats.refresh(&actor.rules)?;
            }
        }
        if added {
            // `IBEC_ChangeMaxLife.Enter`: the rate once, into the unit's own
            // `DataSet`, whether the buff stacks or not.
            if let Some(entry) = running.life_entry() {
                actor.stats.overlays.channel(Channel::Unit).write(entry);
            }
            actor.refresh_life_data()?;
        }
        // The first buff that disables technology switches the unit's
        // technologies off as it enters (`CBEC_DisableTechnology.Enter`).
        if !was_disabled && self.actors[&actor_id].technology_disabled() {
            self.switch_technologies(actor_id, false, events)?;
            self.actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .clear_self_buffs(events)?;
        }
        if row.current_life_rate != 0 {
            self.change_current_life(actor_id, team, row.current_life_rate, events)?;
        }
        events.push(buff_applied(
            ObjectRef::new(ObjectKind::Unit, actor_id),
            team,
            &running,
        ));
        Ok(())
    }

    /// `IBEC_ChangeCurrentLife.Perform`: the unit's life now times the rate,
    /// its whole part. A loss is a hit of no object under the buff's side,
    /// which the rate on damage taken raises (`PerformHitTargetEffect` with
    /// `isAmplifyDamageAffected`): a Rhino of 19297 loses 3860 to
    /// Disintegration's −0.2.
    fn change_current_life(
        &mut self,
        actor_id: u64,
        team: u32,
        rate: i64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let change = self.actors[&actor_id].life.saturating_mul(rate) >> 32;
        match change.cmp(&0) {
            std::cmp::Ordering::Less => {
                let target = super::FightActorRef::Unit(actor_id);
                self.hit_with_no_object(target, team, (-change, true), events)
            }
            std::cmp::Ordering::Equal => Ok(()),
            std::cmp::Ordering::Greater => Err(Error::new(format!(
                "a buff heals unit {actor_id}, and a buff's healing is not measured"
            ))),
        }
    }

    /// A firing construction's damage, as its buffs leave it.
    fn refresh_construction(&mut self, building_id: u64) -> Result<()> {
        let Some(construction) = self.constructions.get_mut(&building_id) else {
            return Ok(());
        };
        let base = construction.attack.base_damage;
        construction.attack_damage = match self.buffs.building_buffs.get(&building_id) {
            Some(buffed) => buffed.overlays.resolve(Index::AttackDamage, base)?,
            None => base,
        };
        Ok(())
    }

    /// What one hit of `amount` takes off a construction, and what it counts
    /// as taken: the hit scaled by its buffs' rate on damage taken, as a
    /// unit's is.
    pub(in crate::fight) fn construction_damage_taken(
        &self,
        building_id: u64,
        amount: i64,
    ) -> Result<(i64, i64)> {
        match self.buffs.building_buffs.get(&building_id) {
            Some(buffed) => Ok((
                buffed.overlays.resolve(Index::AmplifyDamage, amount)?,
                buffed
                    .overlays
                    .resolve_raised(Index::AmplifyDamage, amount)?,
            )),
            None => Ok((amount, amount)),
        }
    }

    /// `BuffManager.Update`, last in `FightConstruction.Update`: a
    /// construction's buffs one tick older, and those whose time is up taken
    /// away.
    pub(in crate::fight) fn update_construction_buffs(
        &mut self,
        building_id: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let Some(buffed) = self.buffs.building_buffs.get_mut(&building_id) else {
            return Ok(());
        };
        let subject = ObjectRef::new(ObjectKind::Building, building_id);
        let ended = tick_buffs(&mut buffed.buffs, 0, subject, events);
        if !ended.is_empty() {
            for buff in &ended {
                buff.withdraw_from(&mut buffed.overlays);
            }
            if buffed.buffs.is_empty() {
                self.buffs.building_buffs.remove(&building_id);
            }
            self.refresh_construction(building_id)?;
        }
        Ok(())
    }

    /// The buffs a construction had when it fell, `cleared`, to follow its
    /// `building_destroyed`: last first, as `BuffManager.Clear` removes them.
    pub(in crate::fight) fn construction_buffs_cleared(&mut self, building_id: u64) -> Vec<Event> {
        let subject = ObjectRef::new(ObjectKind::Building, building_id);
        self.buffs
            .building_buffs
            .remove(&building_id)
            .map(|buffed| {
                buffed
                    .buffs
                    .iter()
                    .rev()
                    .map(|buff| buff_removed(subject, buff.buff_id, BuffRemovedReason::Cleared))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `BuffManager.RemoveBuffEffect` as a beam turns the unit: every running
    /// buff, last first, but those `kept` names, is removed (`RemoveBuff`)
    /// and recorded `team_changed`. What each wrote goes, the life is
    /// refreshed, and the last that disabled technology switches them on
    /// again as it leaves (`CBEC_DisableTechnology.Exit`). The build keeps a
    /// buff `IsSameBuff` matches with a kept one; none of the kept rows
    /// shares a divide here, so their ids are what is matched.
    pub(in crate::fight) fn remove_buffs_on_turn(
        &mut self,
        actor_id: u64,
        kept: &[u32],
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let was_disabled = actor.technology_disabled();
        let subject = ObjectRef::new(ObjectKind::Unit, actor_id);
        let mut removed = false;
        for index in (0..actor.buffs.len()).rev() {
            if !actor.buffs[index].technology && kept.contains(&actor.buffs[index].buff_id) {
                continue;
            }
            let buff = actor.buffs.remove(index);
            events.push(buff_removed(
                subject,
                buff.buff_id,
                BuffRemovedReason::TeamChanged,
            ));
            actor.withdraw_buff(&buff);
            removed = true;
        }
        if !removed {
            return Ok(());
        }
        actor.refresh_life_data()?;
        if was_disabled && !actor.technology_disabled() {
            self.switch_technologies(actor_id, true, events)?;
        }
        Ok(())
    }

    /// `BuffSystem.RemoveBuff` of the buff a technology is its own data of
    /// (`BurrowTech`): taken off the unit with what it wrote, `removed`.
    pub(in crate::fight) fn remove_technology_buff(
        &mut self,
        actor_id: u64,
        technology: u32,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let Some(index) = actor
            .buffs
            .iter()
            .position(|running| running.technology && running.buff_id == technology)
        else {
            return Ok(());
        };
        let buff = actor.buffs.remove(index);
        events.push(buff_removed(
            ObjectRef::new(ObjectKind::Unit, actor_id),
            buff.buff_id,
            BuffRemovedReason::Removed,
        ));
        actor.withdraw_buff(&buff);
        actor.refresh_life_data()
    }

    /// The buffs a unit that died this tick had, `cleared`, to follow its
    /// `unit_died`: the ones it still runs, or the ones its update already
    /// dropped, last first, as `BuffManager.Clear` removes them.
    pub(in crate::fight) fn buffs_cleared_by_death(&mut self, actor_id: u64) -> Vec<Event> {
        let running = self
            .actors
            .get(&actor_id)
            .map(|actor| {
                actor
                    .buffs
                    .iter()
                    .rev()
                    .map(|buff| buff.buff_id)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let dropped = self.buffs.dropped.remove(&actor_id).unwrap_or_default();
        let ids = if running.is_empty() { dropped } else { running };
        ids.into_iter()
            .map(|buff_id| {
                buff_removed(
                    ObjectRef::new(ObjectKind::Unit, actor_id),
                    buff_id,
                    BuffRemovedReason::Cleared,
                )
            })
            .collect()
    }

    /// Every buff a live unit still runs, taken off and written as cleared
    /// in unit order, as the fight is left, and then every buff a standing
    /// construction runs, in building order: each actor's `BuffManager.Clear`
    /// as the fight's objects are let go, which removes an actor's buffs last
    /// first.
    pub(in crate::fight) fn clear_buffs_as_the_fight_ends(
        &mut self,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let mut disabled = Vec::new();
        // Each side's units in the order they joined it (`activeActors`): the
        // Crawlers a Steel Ball's death summoned, in the order they joined,
        // whatever their identities.
        for actor_id in self.units_in_update_order() {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            if !actor.alive() || actor.buffs.is_empty() {
                continue;
            }
            if actor.technology_disabled() {
                disabled.push(actor_id);
            }
            events.extend(actor.buffs.iter().rev().map(|buff| {
                buff_removed(
                    ObjectRef::new(ObjectKind::Unit, actor_id),
                    buff.buff_id,
                    BuffRemovedReason::Cleared,
                )
            }));
            for buff in std::mem::take(&mut actor.buffs) {
                actor.withdraw_buff(&buff);
            }
            // `IBEC_ChangeMaxLife.Exit` takes its rate out, and the life is
            // refreshed: a Rhino with Combat Evolvement ends the fight at its
            // share of its bare maximum.
            actor.refresh_life_data()?;
        }
        // And a unit whose technologies a buff had switched off has them
        // again: a Rhino's Mechanical Rage reads in its corrections on the
        // fight's last tick.
        for actor_id in disabled {
            self.switch_technologies(actor_id, true, events)?;
        }
        for (building_id, buffed) in std::mem::take(&mut self.buffs.building_buffs) {
            let subject = ObjectRef::new(ObjectKind::Building, building_id);
            events.extend(
                buffed
                    .buffs
                    .iter()
                    .rev()
                    .map(|buff| buff_removed(subject, buff.buff_id, BuffRemovedReason::Cleared)),
            );
            self.refresh_construction(building_id)?;
        }
        Ok(())
    }

    /// `BuffManager.Update` on a unit that is no longer alive: its buffs go.
    ///
    /// Not on the tick it dies but on its first update of a later tick, even
    /// one it reaches dead on the tick it died. A Fang of the two-tower fight
    /// killed by a Steel Ball's beam lands its projectile the same tick for
    /// the debuffed 6; one that died two ticks before its projectile landed
    /// lands it for the full 63; and a Vulcan of 67158166 r4 killed before its
    /// own update lands its shell that tick for the debuffed 7, not 74. The
    /// last buff that disabled technology switches them on again as it goes
    /// (`CBEC_DisableTechnology.Exit`): a Vulcan with Scorching Fire that an
    /// Electromagnetic Shot struck deals 97 while they are off, and the shot
    /// it left in the air as it died lands for its 161.
    pub(in crate::fight) fn drop_buffs_of_the_dead(&mut self, actor_id: u64) -> Result<()> {
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        if actor.buffs.is_empty() {
            return Ok(());
        }
        self.buffs.dropped.insert(
            actor_id,
            actor.buffs.iter().rev().map(|buff| buff.buff_id).collect(),
        );
        let was_disabled = actor.technology_disabled();
        for buff in std::mem::take(&mut actor.buffs) {
            actor.withdraw_buff(&buff);
        }
        actor.refresh_life_data()?;
        if was_disabled {
            self.switch_technologies(actor_id, true, &mut Vec::new())?;
        }
        Ok(())
    }

    /// `Buff.Update`'s step of every buff on a unit, last first as
    /// `BuffManager.Update` runs them: `stepTime` a tick on, and at
    /// `stepTimeConfig` back to zero and each of the buff's controllers
    /// updated, of which only `IBEC_ChangeLIfe` acts here. `Buff.Reset` leaves
    /// the step where it was. A step that kills the unit ends the update, and
    /// the buffs before it in the list do not run; the answer is the first
    /// that did.
    fn step_buffs(&mut self, actor_id: u64, events: &mut Vec<Event>) -> Result<usize> {
        let count = self.actors[&actor_id].buffs.len();
        for index in (0..count).rev() {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            // `Buff.Update`: its step one tick on, and its controllers'
            // update as the step comes round.
            let running = &mut actor.buffs[index];
            running.step += 1;
            if running.step < running.step_ticks {
                continue;
            }
            running.step = 0;
            actor.step_stack(index)?;
            let running = &actor.buffs[index];
            let (rate, team) = (running.life_change_rate, running.team);
            if rate == 0 {
                continue;
            }
            // `IBEC_ChangeLIfe.Update`: the unit's maximum life times the
            // rate, its whole part. A loss is a hit of no object under the
            // buff's side that the rate on damage taken does not affect.
            let change = actor.stats.max_life().saturating_mul(rate) >> 32;
            match change.cmp(&0) {
                std::cmp::Ordering::Less => {
                    let target = super::FightActorRef::Unit(actor_id);
                    self.hit_with_no_object(target, team, (-change, false), events)?;
                }
                std::cmp::Ordering::Equal => {}
                std::cmp::Ordering::Greater => {
                    return Err(Error::new(format!(
                        "a buff heals unit {actor_id}, and a buff's healing is not measured"
                    )));
                }
            }
            if !self.actors[&actor_id].alive() {
                return Ok(index);
            }
        }
        Ok(0)
    }

    /// `BuffManager.Update`: every running buff one tick older, and those
    /// whose time is up taken away.
    pub(in crate::fight) fn update_buffs(
        &mut self,
        actor_id: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let from = self.step_buffs(actor_id, events)?;
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let subject = ObjectRef::new(ObjectKind::Unit, actor_id);
        let was_disabled = actor.technology_disabled();
        let ended = tick_buffs(&mut actor.buffs, from, subject, events);
        if !ended.is_empty() {
            // A buff's end takes what it wrote, among the buffs' and in the
            // unit's own life rate, and every other buff's entries stay.
            for buff in &ended {
                actor.withdraw_buff(buff);
            }
            actor.refresh_life_data()?;
            // The last buff that disables technology switches them on again
            // as it leaves (`CBEC_DisableTechnology.Exit`).
            if was_disabled && !actor.technology_disabled() {
                self.switch_technologies(actor_id, true, events)?;
            }
        }
        Ok(())
    }

    /// `FightEffectSystem.DisableEffect` and `EnableEffect` of a unit's
    /// technologies: what they wrote taken away or written again
    /// (`IEffectProviderDataSource.RemoveData`, `AddData`), its life
    /// refreshed to the maximum that leaves it (`FightMech.RefreshLifeData`),
    /// and its skill's current interval made again without its stagger
    /// (`FightSkill.RefreshAttackInterval`): a Rhino with Mechanical Rage
    /// waits 18 ticks between blows where it waited 12, and a Marksman with
    /// Assault Mode's drawn 69 becomes its plain 62.
    pub(in crate::fight) fn switch_technologies(
        &mut self,
        actor_id: u64,
        on: bool,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        // `PreemptiveSkillController.Update` gives an active permanent
        // preemptive skill back up while the technologies are off
        // (`OnPermanentPreemptiveSkillDeactive`), which is not measured; its
        // buff makes the unit invincible, so no debuff reaches it.
        if !on && actor.skills.preemptive_active {
            return Err(Error::new(format!(
                "unit {actor_id}'s technologies are disabled while its permanent preemptive \
                 skill is active, and giving it up is not measured"
            )));
        }
        // `FightEffectMananger.DisableEffect` and `EnableEffect`: every
        // provider beside the numbers' its technologies reach.
        for provider in actor.placement.effects.technology_disable.providers.clone() {
            self.switch_provider(actor_id, provider, on, events)?;
        }
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let corrections = actor
            .placement
            .effects
            .technology_disable
            .corrections
            .clone();
        // A unit with no technology has nothing to refresh: the Wasps an
        // Electromagnetic Impact reaches keep the intervals they drew. One
        // whose technologies wrote nothing still refreshes: a Fortress with
        // Barrier's drawn 35 becomes its plain 36.
        if actor
            .placement
            .effects
            .technology_disable
            .technologies
            .is_empty()
        {
            return Ok(());
        }
        for (channel, entry) in corrections {
            let channel = actor.stats.overlays.channel(channel);
            if on {
                channel.write(entry);
            } else {
                channel.remove(entry);
            }
        }
        actor.refresh_life_data()?;
        let skill_ref = super::skill::SkillRef::main(super::FightActorRef::Unit(actor_id));
        let interval_q32 = self
            .skill_attacker(skill_ref)
            .ok_or_else(|| Error::new("a unit whose technologies switch is absent"))?
            .attack_interval_q32;
        let interval = super::math::seconds_q32_to_steps(interval_q32).max(1);
        let skill = self.skill_mut(skill_ref);
        let started = skill
            .next_attack_step
            .saturating_sub(skill.current_attack_interval);
        skill.current_attack_interval = interval;
        if skill.next_attack_step > 0 {
            skill.next_attack_step = started.saturating_add(interval);
        }
        Ok(())
    }
}

/// A buff's rate as the correction it writes: an increase adds, a decrease
/// impairs.
pub(in crate::fight) const fn rate(raw: i64) -> Correction {
    if raw >= 0 {
        Correction::Rate {
            add: raw,
            reduce: 0,
        }
    } else {
        Correction::Rate {
            add: 0,
            reduce: -raw,
        }
    }
}

/// A construction's running buffs and what they write: the `BuffManager`
/// a `FightConstruction` carries, which a unit's stats hold for it.
#[derive(Debug, Clone, Default)]
pub(in crate::fight) struct BuildingBuffs {
    buffs: Vec<RunningBuff>,
    overlays: Overlays,
}

/// `BuffManager.AddBuff`: a buff already running that `IsSameBuff` matches,
/// the same row or one of the same nonzero divide, is `Buff.Reset`,
/// lengthened by the new row's duration when additive and started over
/// otherwise; any other is added. The running buff is returned, with whether
/// it is new and the entries to write now: a new buff's that its unit does
/// not ignore (`Buff.AddEffect`), and those a reset one held that its unit
/// no longer ignores (`Buff.Reset`).
fn add_buff(
    buffs: &mut Vec<RunningBuff>,
    row: &BuffRow,
    (source_actor, takes_source): (Option<ObjectRef>, bool),
    team: u32,
    ignores_speed_rate: bool,
) -> (RunningBuff, bool, Vec<Entry>) {
    if let Some(running) = buffs.iter_mut().find(|running| same_buff(running, row)) {
        if takes_source {
            running.source_actor = source_actor;
        }
        if running.additive {
            running.duration = running.duration.saturating_add(row.ticks);
        } else {
            running.elapsed = 0;
        }
        let (held, written) = std::mem::take(&mut running.held)
            .into_iter()
            .partition::<Vec<_>, _>(|entry| ignores_speed_rate && is_speed_rate(entry));
        running.held = held;
        running.entries.extend(written.iter().copied());
        return (running.clone(), false, written);
    }
    let (held, entries) = row
        .entries
        .iter()
        .partition::<Vec<Entry>, _>(|entry| ignores_speed_rate && is_speed_rate(entry));
    let running = RunningBuff {
        buff_id: row.buff_id,
        technology: row.technology,
        divide: row.divide,
        additive: row.additive,
        elapsed: 0,
        duration: row.ticks,
        step: 0,
        step_ticks: row.step_ticks,
        team,
        source: row.source,
        entries: entries.clone(),
        held,
        source_actor,
        disables_technology: row.disables_technology,
        invincible: row.invincible,
        disables_recover: row.disables_recover,
        life_change_rate: row.life_change_rate,
        max_life_rate: row.max_life_rate,
        summons: row.summons,
        stack: row.stacking.map(|rule| StackStep {
            rule,
            count: 0,
            written: 0,
            life: 1,
            life_held: false,
        }),
        clears_when_technologies_disabled: row.clears_when_technologies_disabled,
    };
    buffs.push(running.clone());
    (running, true, entries)
}

/// Whether an entry is a buff's `BuffDataFloatRate.MoveSpeedChangeRate`,
/// which a unit that ignores `BuffEffectType.SpeedChangeRate` leaves
/// unwritten.
fn is_speed_rate(entry: &Entry) -> bool {
    entry.index == Index::MoveSpeed && matches!(entry.correction, Correction::Rate { .. })
}

/// `Buff.IsSameBuff`: the same row, or one of the same nonzero divide.
fn same_buff(running: &RunningBuff, row: &BuffRow) -> bool {
    (running.technology == row.technology && running.buff_id == row.buff_id)
        || (row.divide != 0 && running.divide == row.divide)
}

/// Every running buff one tick older, and those whose time is up removed
/// with an `expired` event and handed back.
///
/// Only the buffs from `from` on run: `BuffManager.Update` runs them last
/// first and stops at one whose step killed the unit.
fn tick_buffs(
    buffs: &mut Vec<RunningBuff>,
    from: usize,
    subject: ObjectRef,
    events: &mut Vec<Event>,
) -> Vec<RunningBuff> {
    if buffs.is_empty() {
        return Vec::new();
    }
    for running in &mut buffs[from..] {
        running.elapsed = running.elapsed.saturating_add(1);
    }
    let (ended, kept) = std::mem::take(buffs)
        .into_iter()
        .partition::<Vec<_>, _>(|running| running.elapsed >= running.duration);
    *buffs = kept;
    for running in &ended {
        events.push(buff_removed(
            subject,
            running.buff_id,
            BuffRemovedReason::Expired,
        ));
    }
    ended
}

fn buff_applied(subject: ObjectRef, team: u32, running: &RunningBuff) -> Event {
    event(
        None,
        running.source_actor,
        Some(team),
        Some(subject),
        EventPayload::BuffApplied {
            buff_id: running.buff_id,
            duration: i32::try_from(running.duration.saturating_sub(running.elapsed))
                .unwrap_or(i32::MAX),
        },
    )
}

fn buff_removed(subject: ObjectRef, buff_id: u32, reason: BuffRemovedReason) -> Event {
    event(
        None,
        None,
        None,
        Some(subject),
        EventPayload::BuffRemoved { buff_id, reason },
    )
}

#[cfg(test)]
mod tests {

    /// Rows 1 to 5 last 9, 7, 5, 3 and 1 seconds, and the tower-loss fights
    /// read 180, 140, 100, 60 and 20 ticks; a level's life adds up its row and
    /// every row below it on top of the map's 3400.
    #[test]
    fn a_level_chooses_the_loss_and_adds_the_life() {
        let towers = crate::rules::SimulationConfig::load().unwrap().towers;
        let ticks = (0..=4)
            .map(|level| towers.loss_ticks(level).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(ticks, [180, 140, 100, 60, 20]);
        let life = (0..=4)
            .map(|level| 3400 + towers.life_added(level).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(life, [3_400, 23_400, 59_400, 131_400, 235_400]);
        assert!(towers.loss_ticks(5).is_err());
    }
}
