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
//! (`TryBurrowUp`, `BuffSystem.RemoveBuff`). A unit is held from its
//! deployment (`AddEffect`, `AddMech`), `Deactive`, and is `Normal` once
//! `FightEffectSystem.ActiveEffect` activates its effects (`DoActive`,
//! `BurrowSystem.Active`): as the fight starts, or as it lands, joins or
//! rises again. Its technologies switched off, or its death, bring it up and
//! leave it `Deactive`, in its place (`DisableEffect`, `DoDeactive`,
//! `BurrowSystem.Deactive`), until they come back (`EnableEffect`). A source that takes its unit
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
    /// The units whose status is `Deactive`: their technologies are off.
    deactive: BTreeSet<u64>,
    /// The buffs it wrote and removed this tick.
    events: Vec<Event>,
}

impl Simulation {
    /// `TeamBurrowManager.AddMech` and `ActiveEffect` for every unit the
    /// fight starts with whose technologies hand it a source, each side's in
    /// `OnFightStart`'s order: by the second coordinate where each stands,
    /// then the first. A unit travelling in is held in its place,
    /// `Deactive` until it lands ([`Self::add_burrow_unit`]).
    pub(in crate::fight) fn enter_burrow_fight(&mut self) {
        let mut held = self
            .actors
            .iter()
            .filter(|(_, actor)| actor.placement.effects.single.burrow.is_some())
            .map(|(&id, actor)| ((actor.placement.team, actor.z_q32, actor.x_q32), id))
            .collect::<Vec<_>>();
        held.sort_unstable();
        self.burrow.order = held.iter().map(|(_, id)| *id).collect();
        self.burrow.deactive = held
            .into_iter()
            .map(|(_, id)| id)
            .filter(|id| self.actors[id].travelling)
            .collect();
    }

    /// `BurrowEffectProvider.DoActive` of a unit landing, joining or rising
    /// again: `Normal` in its place, and one made or summoned since the
    /// fight began held after the rest (`AddMech`).
    pub(in crate::fight) fn add_burrow_unit(&mut self, unit: u64) {
        if self.actors[&unit].placement.effects.single.burrow.is_none() {
            return;
        }
        if !self.burrow.order.contains(&unit) {
            self.burrow.order.push(unit);
        }
        self.burrow.deactive.remove(&unit);
    }

    /// `BurrowEffectProvider.DisableEffect` and `EnableEffect`: off, the unit
    /// comes up (`TryBurrowUp`, its buff removed) and its status is
    /// `Deactive`, which the manager's update passes over
    /// (`BurrowSystem.Deactive`); on, it is `Normal` again
    /// (`BurrowSystem.Active`).
    pub(in crate::fight) fn switch_burrow(
        &mut self,
        unit: u64,
        on: bool,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        if !self.burrow.order.contains(&unit) {
            return Ok(());
        }
        if on {
            self.burrow.deactive.remove(&unit);
            return Ok(());
        }
        self.burrow_up(unit, events)?;
        self.burrow.deactive.insert(unit);
        Ok(())
    }

    /// The units a production line made this tick renumbered as a recording
    /// numbers them.
    pub(in crate::fight) fn renumber_burrow_units(&mut self, renamed: &BTreeMap<u64, (u64, u64)>) {
        let rename = |id: &mut u64| {
            if let Some(&(new, _)) = renamed.get(id) {
                *id = new;
            }
        };
        self.burrow.order.iter_mut().for_each(rename);
        for set in [&mut self.burrow.burrowed, &mut self.burrow.deactive] {
            *set = set
                .iter()
                .map(|&id| renamed.get(&id).map_or(id, |&(new, _)| new))
                .collect();
        }
    }

    /// `TeamBurrowManager.Update`: each held unit comes up when its main
    /// skill's search measured no enemy nearest, or when that enemy or its
    /// attack target stands within its source's distance
    /// (`GetDistanceToTarget`, `GetDistanceToAttackTarget`,
    /// `FPoint.op_LessThan`), and burrows otherwise.
    pub(in crate::fight) fn step_burrows(&mut self) -> Result<()> {
        let mut events = std::mem::take(&mut self.burrow.events);
        for unit in self.burrow.order.clone() {
            if self.burrow.deactive.contains(&unit) {
                continue;
            }
            let actor = &self.actors[&unit];
            let source = actor
                .placement
                .effects
                .single
                .burrow
                .clone()
                .expect("a held unit has a source");
            let skill = &actor.skills.main;
            if skill.is_grouped() {
                return Err(Error::new(format!(
                    "unit {unit} burrows with a grouped main skill or a batch of standalone \
                     weapons, whose nearest enemy is not measured"
                )));
            }
            let relieve_q32 = level_value(&source.relieve_distance, actor.placement.level);
            let near = |target: Option<FightActorRef>| {
                target.is_some_and(|target| {
                    rvo::fpoint_less_than(self.burrow_distance(unit, target), relieve_q32)
                })
            };
            let nearest = skill.nearest_actor.get();
            let up = if nearest.is_none() {
                true
            } else {
                // The update measures only for a main skill holding an attack
                // target (`FightSkill.attackTarget`): a unit no update since
                // it landed has searched one for, and one whose attack ended,
                // are left as they stand.
                let Some(target) = actor.slot_attack_target(0) else {
                    continue;
                };
                near(nearest) || near(Some(target))
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

    /// `BurrowEffectProvider.DoDeactive`, as a unit dies
    /// (`BurrowSystem.Deactive`): it comes up (`TryBurrowUp`), its buff,
    /// which `BuffManager.Clear` cleared as it died, not there to remove, and
    /// is `Deactive` in its place until it rises again.
    pub(in crate::fight) fn remove_burrow_unit(&mut self, unit: u64) {
        if self.burrow.order.contains(&unit) {
            self.burrow.burrowed.remove(&unit);
            self.burrow.deactive.insert(unit);
        }
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
        // Its rate joins the unit's buffs' (`BuffManager.buffDatas`), which
        // `GetAmplifyDamageAddRate` and `GetAmplifyDamageReduceRate` read
        // composed, as any other buff's does.
        let team = actor.placement.team;
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
            .effects
            .single
            .burrow
            .as_ref()
            .expect("a held unit has a source")
            .technology;
        self.remove_technology_buff(unit, technology, events)
    }
}

/// `TechnologyData.GetLevelValue`: the entry at the unit's level, counting
/// from one, its last past it, and none of an empty list.
pub(in crate::fight) fn level_value(values: &[i64], level: i64) -> i64 {
    let index = usize::try_from(level - 1).unwrap_or_default();
    values
        .get(index)
        .or_else(|| values.last())
        .copied()
        .unwrap_or_default()
}
