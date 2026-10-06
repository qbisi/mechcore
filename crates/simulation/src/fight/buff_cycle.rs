//! `BuffSystem`: the buffs a unit's technologies and equipment add on a
//! trigger.
//!
//! A `BuffTech` or a `BuffEquipment` hands its unit's `BuffEffectProvider` a
//! source, and
//! `BuffEffectProvider.RegisterEffectEvent` a `BuffCycleController` in its
//! side's `TeamBuffCycleManager`. The one trigger read is the fight's start:
//! `BuffCycleController.OnEnterFight` starts the controller of a unit not
//! travelling, and `BuffSystem` updates it on every tick, first in the tick
//! and before `CommanderSkillSystem`, while its unit lives and its
//! technologies are not disabled (`isAvailable`). Under
//! `BuffTargetUpdateModel.All` its first `Update` adds the buff to the unit
//! itself through `BuffSystem.AddBuffByCheck`, once. Under `Each` its
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
    modifier::{BuffReach, BuffSource},
};

/// What tags the entries a technology's or an equipment's buff writes.
const SOURCE: &str = "BuffEffectProvider";

/// `RangeUnitCycle.selectRangeInterval`: the updates its `currentFrame`
/// counts before it first selects, which it never sets back.
const SELECT_RANGE_INTERVAL: u32 = 8;

/// A `BuffCycleController` as it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::fight) enum BuffCycle {
    /// Started by `TriggerCycleStart` (`BuffCycleState.Delaying`), before its
    /// first update.
    Starting,
    /// Under `All`, triggered and done.
    Done,
    /// Under `Each`, its `RangeUnitCycle` past its delay: `currentFrame`, and
    /// `fightMeches`, the units it holds in the order it took them.
    Running { frame: u32, members: Vec<u64> },
}

impl Simulation {
    /// `TeamBuffCycleManager.Update`: every controller of a live unit whose
    /// technologies are not disabled, blue's units first, each unit's in the
    /// order its sources came, every buff recorded as written by that unit.
    pub(in crate::fight) fn step_buff_cycles(
        &mut self,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let mut owners = self
            .actors
            .iter()
            .filter(|(_, actor)| {
                !actor.buff_cycles.is_empty() && actor.alive() && !actor.technology_disabled()
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
        let source = actor.placement.buff_sources[index];
        let owner = actor.object_ref();
        let cycle = actor.buff_cycles[index].clone();
        let (next, reached) = match (source.reach, cycle) {
            (_, BuffCycle::Done) => return Ok(()),
            (None, BuffCycle::Starting) => (BuffCycle::Done, vec![id]),
            // `RangeUnitCycle.Update` in `Delay`: a delay never set is over
            // on the first update, which selects nothing.
            (Some(_), BuffCycle::Starting) => (
                BuffCycle::Running {
                    frame: 0,
                    members: Vec::new(),
                },
                Vec::new(),
            ),
            (Some(reach), BuffCycle::Running { frame, members }) => {
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
            (None, BuffCycle::Running { .. }) => {
                unreachable!("a buff source with no reach never runs a range cycle")
            }
        };
        self.actors
            .get_mut(&id)
            .expect("actor identity is stable")
            .buff_cycles[index] = next;
        let row = buff_row(&source)?;
        for target in reached {
            if self.buff_reaches(target, &row) {
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
        let owner = &self.actors[&id];
        let team = owner.placement.team;
        let found = self
            .units_in_range(
                (owner.x_q32, owner.z_q32),
                reach.range_q32,
                (reach.domains, reach.target_radius),
                target_search_order,
            )
            .into_iter()
            .filter(|&target| {
                let same_side = self.actors[&target].placement.team == team;
                let targets = reach.targets;
                // `AvailableCheck`: `MechUnit` the unit itself,
                // `OtherSelfUnits` its side's others, `FriendUnits` its
                // group's, a group being one side here, and `OpponentUnits`
                // the other side's.
                (targets.itself && target == id)
                    || (targets.own_others && same_side && target != id)
                    || (targets.friends && same_side)
                    || (targets.opponents && !same_side)
            })
            .collect::<Vec<_>>();
        members.retain(|member| found.contains(member));
        for target in found {
            if !members.contains(&target) {
                members.push(target);
            }
        }
        members
    }
}

/// The `buffDatas` row a source adds, as `BuffManager.AddBuff` adds it.
fn buff_row(buff: &BuffSource) -> Result<BuffRow> {
    let step_ticks = u32::try_from(seconds_q32_to_steps(buff.step_q32))
        .map_err(|_| Error::new("a buff's step outlasts a fight"))?;
    Ok(BuffRow {
        buff_id: buff.buff_id,
        clears_when_technologies_disabled: buff.clears_when_technologies_disabled,
        max_life_rate: buff.max_life_rate,
        stacking: buff.stacking.map(|stacking| StackRule {
            step_ticks,
            max: stacking.max,
            condition: stacking.condition,
        }),
        divide: buff.divide,
        additive: buff.additive,
        ticks: u32::try_from(seconds_q32_to_steps(buff.duration_q32))
            .map_err(|_| Error::new("a source's buff outlasts a fight"))?,
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
        // `attackRangeChangeValue`: whole metres on the main skill's range.
        .chain((buff.attack_range_value != 0).then(|| Entry {
            index: Index::AttackRange,
            source: SOURCE,
            correction: crate::data::Correction::Value(
                buff.attack_range_value * crate::rules::SPACE_UNITS_PER_METER_SCALE,
            ),
        }))
        .collect(),
        disables_technology: false,
        debuff: buff.debuff,
        invincible: buff.invincible,
        life_change: None,
        current_life_rate: 0,
    })
}
