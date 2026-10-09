//! `BurrowSystem`: a unit that a technology burrows while no enemy is near.
//!
//! `BurrowEffectProvider.DoActive` hands each unit whose technology is an
//! `IBurrow` to its side's `TeamBurrowManager` (`AddMech`, `ActiveEffect`),
//! which `OnFightStart` sorts by where each stands
//! (`FightUtility.SkillOwnerComparer`). Each update, after
//! `WreckageRecoverySystem`, the manager brings up a unit whose main skill's
//! search measured no enemy nearest, or whose nearest enemy or attack target
//! stands within the source's distance, and burrows any other
//! (`TeamBurrowManager.Update`). Burrowing writes `BurrowTech` on the unit as
//! its own buff, its rate on the damage the unit takes, from no source
//! (`TryBurrowDown`, `BuffSystem.DoAddBuff`); coming up removes it
//! (`TryBurrowUp`, `BuffSystem.RemoveBuff`). A source that takes its unit
//! underground (`IsEnterUnderGround`) is refused by the technology table.
//! `docs/rules/technology_effects.md` states the rule.

use super::tower::{BuffRow, CERTAIN, SOURCE, rate};
use super::*;
use crate::data::{Entry, Index};
use crate::modifier::Burrow;

/// The sides' `TeamBurrowManager`s.
#[derive(Debug, Clone, Default)]
pub(in crate::fight) struct BurrowSystem {
    /// `burrowDatas`' order: each side's units by where they stand
    /// (`OnFightStart` sorts them by `FightUtility.SkillOwnerComparer`).
    order: Vec<u64>,
    /// The units whose `BurrowInfo.burrowStatus` is `Underground`; every
    /// other held unit's is `Normal`.
    burrowed: BTreeSet<u64>,
    /// The buffs it wrote and removed this tick.
    events: Vec<Event>,
}

impl Simulation {
    /// `TeamBurrowManager.AddMech` and `ActiveEffect` for every unit the
    /// fight starts with whose technologies hand it a source, each side's in
    /// `OnFightStart`'s order: by the second coordinate where each stands,
    /// then the first. The layout refuses a unit travelling in with one.
    pub(in crate::fight) fn enter_burrow_fight(&mut self) {
        let mut held = self
            .actors
            .iter()
            .filter(|(_, actor)| actor.placement.burrow.is_some())
            .map(|(&id, actor)| ((actor.placement.team, actor.z_q32, actor.x_q32), id))
            .collect::<Vec<_>>();
        held.sort_unstable();
        self.burrow.order = held.into_iter().map(|(_, id)| id).collect();
    }

    /// `TeamBurrowManager.Update`: each held unit comes up when its main
    /// skill's search measured no enemy nearest, or when that enemy or its
    /// attack target stands within its source's distance
    /// (`GetDistanceToTarget`, `GetDistanceToAttackTarget`,
    /// `FPoint.op_LessThan`), and burrows otherwise.
    pub(in crate::fight) fn step_burrows(&mut self) -> Result<()> {
        let mut events = std::mem::take(&mut self.burrow.events);
        for unit in self.burrow.order.clone() {
            let actor = &self.actors[&unit];
            let source = actor
                .placement
                .burrow
                .clone()
                .expect("a held unit has a source");
            let skill = &actor.skills.main;
            if skill.group.is_some() {
                return Err(Error::new(format!(
                    "unit {unit} burrows with a grouped main skill, whose search's nearest \
                     enemy is not measured"
                )));
            }
            let relieve_q32 = level_value(&source.relieve_distance, actor.placement.level);
            let near = |target: Option<FightActorRef>| {
                target.is_some_and(|target| {
                    rvo::fpoint_less_than(self.burrow_distance(unit, target), relieve_q32)
                })
            };
            let nearest = skill.nearest_actor.get();
            let up = match nearest {
                None => true,
                Some(_) if near(nearest) => true,
                // `GetDistanceToAttackTarget` measures to the attack target,
                // else the unit's lock, and reads a null one's transform: the
                // exception leaves the unit as it stands, and the manager goes
                // on to the next.
                Some(_) => match skill.attack_target().or_else(|| actor.mech_lock()) {
                    None => continue,
                    target => near(target),
                },
            };
            if up {
                self.burrow_up(unit, &mut events)?;
            } else {
                self.burrow_down(unit, &source, &mut events)?;
            }
        }
        self.burrow.events = events;
        Ok(())
    }

    /// `TeamBurrowManager.RemoveMech`, as a unit dies
    /// (`BurrowEffectProvider.DoDeactive`): a burrowed one comes up
    /// (`TryBurrowUp`), and its buff, which `BuffManager.Clear` cleared as
    /// it died, is not there to remove.
    pub(in crate::fight) fn remove_burrow_unit(&mut self, unit: u64) {
        self.burrow.burrowed.remove(&unit);
        self.burrow.order.retain(|held| *held != unit);
    }

    /// What the manager wrote this tick, which follows the tick's deaths:
    /// `BurrowSystem` updates after `DeadEffectSystem`.
    pub(in crate::fight) fn take_burrow_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.burrow.events)
    }

    /// The distance the manager measures from a unit to an enemy:
    /// `FVector3.Distance` of their positions, less both radii
    /// (`IBuffTarget.GetRadius`), not held at zero.
    fn burrow_distance(&self, unit: u64, target: FightActorRef) -> i64 {
        let actor = &self.actors[&unit];
        let Some(view) = self.fight_actor(target) else {
            return i64::MAX;
        };
        math::native_q32_magnitude_3d(
            view.x_q32.saturating_sub(actor.x_q32),
            0,
            view.z_q32.saturating_sub(actor.z_q32),
        )
        .saturating_sub(space_to_q32(actor.rules.collision_radius()))
        .saturating_sub(space_to_q32(view.radius))
    }

    /// `TeamBurrowManager.TryBurrowDown`: a unit not yet burrowed burrows,
    /// and writes its technology on itself as its own buff.
    fn burrow_down(&mut self, unit: u64, source: &Burrow, events: &mut Vec<Event>) -> Result<()> {
        if self.burrow.burrowed.contains(&unit) {
            return Ok(());
        }
        self.burrow.burrowed.insert(unit);
        let actor = &self.actors[&unit];
        let rate_q32 = level_value(&source.amplify_damage_rate, actor.placement.level);
        let row = BuffRow {
            buff_id: source.technology,
            technology: true,
            // `BurrowTech` answers `IsClearSelfBuffWhenDisableTech` with
            // true, `GetBuffDivide` with none, `IsAdditiveTime` and
            // `IsAdditiveEffect` with false.
            clears_when_technologies_disabled: true,
            divide: 0,
            additive: false,
            // `GetDuration`: `FightUtility.MaxTime`, which `Buff.Init` holds
            // at 2^30 ticks.
            ticks: 1 << 30,
            step_ticks: 0,
            source: SOURCE,
            // `IBuffDataFloatRate.GetData` answers its rate for
            // `AmplifyDamageRate` alone.
            entries: (rate_q32 != 0)
                .then_some(Entry {
                    index: Index::AmplifyDamage,
                    source: SOURCE,
                    correction: rate(rate_q32),
                })
                .into_iter()
                .collect(),
            disables_technology: false,
            debuff: false,
            probability: CERTAIN,
            invincible: false,
            disables_recover: false,
            life_change_rate: 0,
            current_life_rate: 0,
            max_life_rate: 0,
            stacking: None,
            summons: None,
        };
        let team = actor.placement.team;
        if let Some(beside) = actor.buff_not_beside(&row) {
            return Err(Error::new(format!(
                "unit {unit} burrows beside buff {beside}, and how their rates compose is \
                 not measured"
            )));
        }
        if !self.buff_reaches(unit, &row)? {
            return Ok(());
        }
        // `TryBurrowDown` hands `DoAddBuff` no source.
        self.write_buff(unit, None, team, &row, events)
    }

    /// `TeamBurrowManager.TryBurrowUp`: a burrowed unit comes up, and its
    /// buff is removed.
    fn burrow_up(&mut self, unit: u64, events: &mut Vec<Event>) -> Result<()> {
        if !self.burrow.burrowed.remove(&unit) {
            return Ok(());
        }
        let technology = self.actors[&unit]
            .placement
            .burrow
            .as_ref()
            .expect("a held unit has a source")
            .technology;
        self.remove_technology_buff(unit, technology, events)
    }
}

/// `TechnologyData.GetLevelValue`: the entry at the unit's level, counting
/// from one, its last past it, and none of an empty list.
fn level_value(values: &[i64], level: i64) -> i64 {
    let index = usize::try_from(level - 1).unwrap_or_default();
    values
        .get(index)
        .or_else(|| values.last())
        .copied()
        .unwrap_or_default()
}
