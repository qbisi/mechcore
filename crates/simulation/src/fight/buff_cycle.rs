//! `BuffSystem`: the buffs a unit's technologies and equipment add on a
//! trigger.
//!
//! A `BuffTech` or a `BuffEquipment` hands its unit's `BuffEffectProvider` a
//! source, and
//! `BuffEffectProvider.RegisterEffectEvent` a `BuffCycleController` in its
//! side's `TeamBuffCycleManager`. A controller whose listener is a hit or the
//! unit's losing life never cycles; one whose listener is the fight's start
//! does: `BuffCycleController.OnEnterFight` starts the controller of a unit not
//! travelling, and `BuffSystem` updates it on every tick, first in the tick
//! and before `CommanderSkillSystem`, while its unit lives and its
//! technologies are not disabled (`isAvailable`). Under
//! `BuffTargetUpdateModel.All` its `UpdateModel1` counts the updates: once
//! they reach its delay it triggers, and when it cycles, again every interval
//! after; each trigger adds the buff through `BuffSystem.AddBuffByCheck` to
//! the unit itself, or to the units in reach. Under `Each` its
//! `RangeUnitCycle` leaves its delay on its first update and, from its eighth
//! update on, takes every update the units in reach and adds the buff to each
//! of them again. `docs/rules/equipment_effects.md` and
//! `docs/rules/technology_effects.md` state the rule.

use super::{
    tower::{BuffRow, StackRule},
    *,
};
use crate::{
    data::{Entry, Index},
    modifier::{AllCycle, BuffReach, BuffSource, BuffTrigger},
};

/// What tags the entries a technology's or an equipment's buff writes.
const SOURCE: &str = "BuffEffectProvider";

/// `RangeUnitCycle.selectRangeInterval`: the updates its `currentFrame`
/// counts before it first selects, which it never sets back.
const SELECT_RANGE_INTERVAL: u32 = 8;

/// `FPoint`'s equality tolerance, in raw units: `get_IsCycle` takes an
/// interval within it of zero for none.
const FPOINT_EQUALITY_RAW: i64 = 43;

/// A `BuffCycleController` as it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::fight) enum BuffCycle {
    /// Under `Each`, started by `TriggerCycleStart`
    /// (`BuffCycleState.Delaying`), before its first update.
    Starting,
    /// Under `All`, counting: `timeSum`, and whether it is past its delay
    /// (`BuffCycleState.Cycleing`).
    Counting { cycling: bool, time_sum: u32 },
    /// Under `All`, triggered and not cycling; or never started, its
    /// listener not being the fight's start.
    Done,
    /// Under `Each`, its `RangeUnitCycle` past its delay: `currentFrame`, and
    /// `fightMeches`, the units it holds in the order it took them.
    Running { frame: u32, members: Vec<u64> },
}

impl BuffCycle {
    /// A unit's controllers as the fight starts: `OnEnterFight` starts each
    /// whose listener is the fight's start, and no other.
    pub(in crate::fight) fn of(sources: &[BuffSource]) -> Vec<Self> {
        sources
            .iter()
            .map(|source| match source.trigger {
                BuffTrigger::Hit | BuffTrigger::BeHit | BuffTrigger::Damaged => Self::Done,
                BuffTrigger::All(_) => Self::Counting {
                    cycling: false,
                    time_sum: 0,
                },
                BuffTrigger::Around(_) => Self::Starting,
            })
            .collect()
    }
}

impl Simulation {
    /// `BuffEffectProvider.DoDisableCycle` and `DoEnableCycle`, which its
    /// `DisableEffect` and `EnableEffect` run for every source it holds:
    /// each controller of the unit stops, its listener taken off
    /// (`RemoveListener`), and starts again with its listener and its
    /// `timeSum` at zero; a `RangeUnitCycle` keeps where it stood.
    pub(in crate::fight) fn switch_buff_cycles(&mut self, unit: u64, on: bool) {
        let actor = self
            .actors
            .get_mut(&unit)
            .expect("actor identity is stable");
        if actor.buff_cycles_available == on {
            return;
        }
        actor.buff_cycles_available = on;
        if on {
            for cycle in &mut actor.buff_cycles {
                if let BuffCycle::Counting { time_sum, .. } = cycle {
                    *time_sum = 0;
                }
            }
        }
    }

    /// `TeamBuffCycleManager.Update`: every available controller of a live
    /// unit, blue's units first, each unit's in the order its sources came,
    /// every buff recorded as written by that unit.
    pub(in crate::fight) fn step_buff_cycles(
        &mut self,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let mut owners = self
            .actors
            .iter()
            .filter(|(_, actor)| {
                !actor.buff_cycles.is_empty() && actor.alive() && actor.buff_cycles_available
            })
            .map(|(&id, actor)| (actor.placement.team, id))
            .collect::<Vec<_>>();
        owners.sort_unstable();
        for (team, id) in owners {
            for index in 0..self.actors[&id].buff_cycles.len() {
                self.update_buff_cycle(id, team, index, target_search_order, events)?;
            }
        }
        Ok(())
    }

    /// `BuffCycleController.Update` of the `index`th source of unit `id`.
    fn update_buff_cycle(
        &mut self,
        id: u64,
        team: u32,
        index: usize,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let actor = &self.actors[&id];
        let source = actor.placement.effects.buff_sources[index];
        let owner = actor.object_ref();
        let cycle = actor.buff_cycles[index].clone();
        let (next, reached) = match (source.trigger, cycle) {
            (_, BuffCycle::Done) => return Ok(()),
            (BuffTrigger::All(all), BuffCycle::Counting { cycling, time_sum }) => {
                match count_all(&all, cycling, time_sum) {
                    (next, false) => (next, Vec::new()),
                    (next, true) => {
                        let reached = match all.reach {
                            None => vec![id],
                            Some(reach) => self.units_reached(id, &reach, target_search_order),
                        };
                        (next, reached)
                    }
                }
            }
            // `RangeUnitCycle.Update` in `Delay`: a delay never set is over
            // on the first update, which selects nothing.
            (BuffTrigger::Around(_), BuffCycle::Starting) => (
                BuffCycle::Running {
                    frame: 0,
                    members: Vec::new(),
                },
                Vec::new(),
            ),
            (BuffTrigger::Around(reach), BuffCycle::Running { frame, members }) => {
                let frame = frame.saturating_add(1);
                if frame < SELECT_RANGE_INTERVAL {
                    (BuffCycle::Running { frame, members }, Vec::new())
                } else {
                    let members = self.select_in_reach(id, &reach, members, target_search_order);
                    (
                        BuffCycle::Running {
                            frame,
                            members: members.clone(),
                        },
                        members,
                    )
                }
            }
            (_, BuffCycle::Running { .. } | BuffCycle::Starting | BuffCycle::Counting { .. }) => {
                unreachable!("a cycle runs as its source's update model does")
            }
        };
        self.actors
            .get_mut(&id)
            .expect("actor identity is stable")
            .buff_cycles[index] = next;
        let row = buff_row(&source)?;
        for target in reached {
            if self.buff_reaches(target, &row)? {
                self.write_buff(target, Some(owner), team, &row, events)?;
            }
        }
        Ok(())
    }

    /// `RangeUnitCycle.UpdateSelector`: the units `CalculateRangeActors`
    /// finds within the source's reach of its unit that `AvailableCheck`
    /// passes. A unit it held that is no longer among them is let go, the
    /// rest keep their places, and a new one comes after them. Its
    /// `intervalTimeConfig` is never set, so every unit it holds triggers
    /// (`OnActorTrigger`) on every update: a Vortex with Mobile Power Station
    /// adds its buff to itself and a Marksman beside it on every tick from
    /// the fight's ninth.
    fn select_in_reach(
        &self,
        id: u64,
        reach: &BuffReach,
        mut members: Vec<u64>,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Vec<u64> {
        let found = self.units_reached(id, reach, target_search_order);
        members.retain(|member| found.contains(member));
        for target in found {
            if !members.contains(&target) {
                members.push(target);
            }
        }
        members
    }

    /// `RangeTargetCalculator.CalculateRangeActors` around unit `id` within
    /// the source's reach, in the order it finds them, of those
    /// `BuffCycleController.AvailableCheck` passes.
    fn units_reached(
        &self,
        id: u64,
        reach: &BuffReach,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Vec<u64> {
        let owner = &self.actors[&id];
        let team = owner.placement.team;
        self.units_in_range(
            (owner.x_q32, owner.z_q32),
            reach.range_q32,
            (reach.domains, reach.target_radius),
            target_search_order,
        )
        .into_iter()
        .filter(|&target| {
            let same_side = self.actors[&target].placement.team == team;
            let targets = reach.targets;
            // `AvailableCheck`: a source of a distance type passes only a
            // unit whose main skill's `IsMeleeAttack` answers for it, and
            // then `MechUnit` the unit itself, `OtherSelfUnits` its team's
            // others, and `OpponentUnits` the other side's.
            reach
                .melee
                .is_none_or(|melee| self.actors[&target].rules.attack.melee == melee)
                && ((targets.itself && target == id)
                    || (targets.own_others && same_side && target != id)
                    || (targets.opponents && !same_side))
        })
        .collect()
    }
}

/// `BuffCycleController.UpdateModel1`'s count: `timeSum` a tick on, and the
/// controller's next state with whether it triggers. Before its delay is
/// over (`delayTimeConfig`, the delay in whole ticks) it waits; then it
/// triggers, keeps what the delay left over, and cycles if it has an
/// interval (`IsCycle`, `FPoint`'s tolerant inequality to zero) or is done.
/// Cycling, it triggers each time `timeSum` reaches `intervalTimeConfig`
/// and keeps the rest.
fn count_all(all: &AllCycle, cycling: bool, time_sum: u32) -> (BuffCycle, bool) {
    let ticks = |seconds_q32| u32::try_from(seconds_q32_to_steps(seconds_q32)).unwrap_or(u32::MAX);
    let time_sum = time_sum.saturating_add(1);
    let limit = ticks(if cycling {
        all.interval_q32
    } else {
        all.delay_q32
    });
    if time_sum < limit {
        return (BuffCycle::Counting { cycling, time_sum }, false);
    }
    let time_sum = time_sum - limit;
    let next = if cycling || all.interval_q32.abs() > FPOINT_EQUALITY_RAW {
        BuffCycle::Counting {
            cycling: true,
            time_sum,
        }
    } else {
        BuffCycle::Done
    };
    (next, true)
}

impl Simulation {
    /// `BuffCycleController.PerformHitEffect` of each source of the skill's
    /// owner whose listener is `Hit`, after a hit of its skill in `slot`:
    /// `RegisterMechEvent` made it a hit effect of each skill
    /// `SkillDataModifier.AvaliableCheck` passes, and a source that
    /// `CanDisable` does nothing while the owner's technologies are disabled.
    /// `BuffSystem.AddBuff` then adds the buff, under the owner's side and
    /// as written by it, to every unit the hit struck, in the order it
    /// struck them, a dead one only when the buff summons; a construction
    /// takes none, the buff not reaching one. A
    /// Void Eye with Suppression Shots cuts a struck Fortress's range from
    /// 100 to 70, and a struck Rhino's melee reach not at all.
    pub(in crate::fight) fn add_hit_buffs(
        &mut self,
        owner_id: u64,
        slot: u16,
        (targets, center): (&[FightActorRef], (i64, i64, i64)),
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let Some(owner) = self.actors.get(&owner_id) else {
            return Ok(());
        };
        let sources = owner
            .placement
            .effects
            .buff_sources
            .iter()
            .filter(|source| source.trigger == BuffTrigger::Hit)
            .filter(|source| !(source.can_disable && owner.technology_disabled()))
            .copied()
            .collect::<Vec<_>>();
        if sources.is_empty() || !owner.corrected_skill_slots().contains(&usize::from(slot)) {
            return Ok(());
        }
        if owner.placement.effects.lifesteal.is_some() {
            return Err(Error::new(format!(
                "unit {owner_id} steals life and adds a buff on a hit, and in which order its \
                 skill's hit effects run is not read"
            )));
        }
        let (source, team) = (owner.object_ref(), owner.placement.team);
        for buff in sources {
            let row = buff_row(&buff)?;
            for target in targets {
                let FightActorRef::Unit(id) = *target else {
                    continue;
                };
                // `BuffSystem.IsAvaliableWhenActorDead`: a buff that summons
                // or disables technology reaches a unit the hit killed.
                let reaches_the_dead = row.summons.is_some() || row.disables_technology;
                if (self.actors[&id].alive() || reaches_the_dead) && self.buff_reaches(id, &row)? {
                    self.write_buff(id, Some(source), team, &row, events)?;
                }
            }
            // `GetBuffRangeItem`: what the hit leaves where it lands, under
            // the owner's side, when what the skill fires at
            // (`attackTarget`, or its lock) stands on the ground.
            if let Some(terrain) = buff.range_item {
                let skill = self
                    .skill(self.skill_at_slot(FightActorRef::Unit(owner_id), usize::from(slot)));
                let aimed = skill.attack_target().or(skill.lock_target);
                if aimed.is_some_and(|aimed| self.domain_of(aimed) == UnitDomain::Ground) {
                    self.add_terrain(team, &format!("unit {owner_id}"), terrain, center)?;
                }
            }
        }
        Ok(())
    }
}

impl Simulation {
    /// `BuffCycleController.OnBeHit` of each source of unit `id` whose
    /// listener is `BeHit`, as `FightMech.OnHitted` raises `OnMechBeHit`
    /// after a hit of `attacker`'s took what it took: a source that
    /// `CanDisable` does nothing while the unit's technologies are disabled,
    /// and one aimed at `OpponentUnits` reaches the attacker when it is a
    /// live unit. A buff that disables technology waits on the attacker's
    /// `BuffManager.AddBeHitDelayBuffInfo` until its next update; any other
    /// `BuffSystem.AddBuffByCheck` adds at once, as written by the unit hit.
    pub(in crate::fight) fn on_mech_be_hit(
        &mut self,
        id: u64,
        attacker: ObjectRef,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let Some(owner) = self.actors.get(&id) else {
            return Ok(());
        };
        let sources = owner
            .placement
            .effects
            .buff_sources
            .iter()
            .filter(|source| source.trigger == BuffTrigger::BeHit)
            // `RemoveListener` took it off while its controllers are not
            // available.
            .filter(|_| owner.buff_cycles_available)
            .copied()
            .collect::<Vec<_>>();
        if sources.is_empty()
            || attacker.kind != ObjectKind::Unit
            || !self.actors.get(&attacker.id).is_some_and(Actor::alive)
        {
            return Ok(());
        }
        let (source, team) = (owner.object_ref(), owner.placement.team);
        for buff in sources {
            if buff.disables_technology {
                self.actors
                    .get_mut(&attacker.id)
                    .expect("actor identity is stable")
                    .delayed_buffs
                    .push((id, buff));
                continue;
            }
            let row = buff_row(&buff)?;
            if self.buff_reaches(attacker.id, &row)? {
                self.write_buff(attacker.id, Some(source), team, &row, events)?;
            }
        }
        Ok(())
    }

    /// `BuffManager.InvokeDelayAddBuff` as unit `id`'s `BuffManager.Update`
    /// ends: each buff queued on it, in the order it was queued, through
    /// `BuffSystem.DoAddBuff`, as written by the unit that queued it and
    /// under that unit's side, while it lives or when the buff reaches the
    /// dead; then the queue is emptied.
    pub(in crate::fight) fn invoke_delayed_buffs(
        &mut self,
        id: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let delayed = std::mem::take(
            &mut self
                .actors
                .get_mut(&id)
                .expect("actor identity is stable")
                .delayed_buffs,
        );
        for (writer, buff) in delayed {
            let row = buff_row(&buff)?;
            let reaches_the_dead = row.summons.is_some() || row.disables_technology;
            if !(self.actors[&id].alive() || reaches_the_dead) {
                continue;
            }
            let writer = &self.actors[&writer];
            let (source, team) = (writer.object_ref(), writer.placement.team);
            if self.buff_reaches(id, &row)? {
                self.write_buff(id, Some(source), team, &row, events)?;
            }
        }
        Ok(())
    }

    /// `BuffCycleController.OnGetDamage` of each source of unit `id` whose
    /// listener is `GetDamage`, after `FightActor.ReduceLife` took life from
    /// it and invoked its `OnLifeChange`: a source that `CanDisable` does
    /// nothing while the unit's technologies are disabled, and
    /// `BuffSystem.AddBuffByCheck` adds the buff to the unit itself, as
    /// written by it, while it lives or when the buff reaches the dead. A
    /// Fire Badger with Counter-Fire hit by anything reaches 70 m further
    /// for the next 20 seconds.
    pub(in crate::fight) fn add_damaged_buffs(
        &mut self,
        id: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let owner = &self.actors[&id];
        let sources = owner
            .placement
            .effects
            .buff_sources
            .iter()
            .filter(|source| source.trigger == BuffTrigger::Damaged)
            // `RemoveListener` took it off while its controllers are not
            // available.
            .filter(|_| owner.buff_cycles_available)
            .copied()
            .collect::<Vec<_>>();
        let (source, team) = (owner.object_ref(), owner.placement.team);
        for buff in sources {
            let row = buff_row(&buff)?;
            // `BuffSystem.IsAvaliableWhenActorDead`.
            let reaches_the_dead = row.summons.is_some() || row.disables_technology;
            if (self.actors[&id].alive() || reaches_the_dead) && self.buff_reaches(id, &row)? {
                self.write_buff(id, Some(source), team, &row, events)?;
            }
        }
        Ok(())
    }
}

/// The `buffDatas` row a source adds, as `BuffManager.AddBuff` adds it.
fn buff_row(buff: &BuffSource) -> Result<BuffRow> {
    let step_ticks = u32::try_from(seconds_q32_to_steps(buff.step_q32))
        .map_err(|_| Error::new("a buff's step outlasts a fight"))?;
    Ok(BuffRow {
        buff_id: buff.buff_id,
        technology: false,
        clears_when_technologies_disabled: buff.clears_when_technologies_disabled,
        max_life_rate: buff.max_life_rate,
        summons: buff.summons,
        stacking: buff.stacking.map(|stacking| StackRule {
            max: stacking.max,
            condition: stacking.condition,
            resets_on_main_hit: stacking.resets_on_main_hit,
        }),
        divide: buff.divide,
        additive: buff.additive,
        ticks: u32::try_from(seconds_q32_to_steps(buff.duration_q32))
            .map_err(|_| Error::new("a source's buff outlasts a fight"))?,
        step_ticks,
        source: SOURCE,
        entries: [
            (Index::MoveSpeed, buff.speed_rate),
            (Index::AttackDamage, buff.damage_rate),
            (Index::AmplifyDamage, buff.amplify_damage_rate),
        ]
        .into_iter()
        .filter(|&(_, rate)| rate != 0)
        .map(|(index, rate)| Entry {
            index,
            source: SOURCE,
            correction: super::tower::rate(rate),
        })
        // `attackRangeChangeRate`: a rate on the main skill's range.
        .chain((buff.attack_range_rate != 0).then(|| Entry {
            index: Index::AttackRange,
            source: SOURCE,
            correction: super::tower::rate(buff.attack_range_rate),
        }))
        // `attackRangeChangeValue`: whole metres on the main skill's range.
        .chain((buff.attack_range_value != 0).then(|| Entry {
            index: Index::AttackRange,
            source: SOURCE,
            correction: crate::data::Correction::Value(
                buff.attack_range_value * crate::rules::SPACE_UNITS_PER_METER_SCALE,
            ),
        }))
        .collect(),
        disables_technology: buff.disables_technology,
        debuff: buff.debuff,
        probability: buff.probability,
        invincible: buff.invincible,
        disables_recover: buff.disables_recover,
        life_change_rate: buff.life_change_rate,
        current_life_rate: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::{AllCycle, BuffCycle, count_all};

    /// The updates on which a controller under `All` triggers, of the first
    /// `updates`.
    fn triggers(all: &AllCycle, updates: u32) -> Vec<u32> {
        let mut state = BuffCycle::Counting {
            cycling: false,
            time_sum: 0,
        };
        let mut fired = Vec::new();
        for update in 1..=updates {
            let BuffCycle::Counting { cycling, time_sum } = state else {
                break;
            };
            let (next, triggered) = count_all(all, cycling, time_sum);
            if triggered {
                fired.push(update);
            }
            state = next;
        }
        fired
    }

    #[test]
    fn a_delay_triggers_once_and_an_interval_again_and_again() {
        let second = 1_i64 << 32;
        // Photon Emission: 0.6 s, 12 ticks, and no interval.
        let emission = AllCycle {
            delay_q32: 2_576_980_377,
            interval_q32: 0,
            reach: None,
        };
        assert_eq!(triggers(&emission, 100), [12]);
        // Photon Loop: no delay, so the first update, and every 30 s after,
        // the count keeping the tick the delay left over.
        let looped = AllCycle {
            delay_q32: 0,
            interval_q32: 30 * second,
            reach: None,
        };
        assert_eq!(triggers(&looped, 1300), [1, 600, 1200]);
    }
}
