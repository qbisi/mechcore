//! `BurrowSystem`: a unit that a technology burrows while no enemy is near.
//!
//! `BurrowEffectProvider.DoActive` hands each unit whose technology is an
//! `IBurrow` to its side's `TeamBurrowManager` (`AddMech`, `ActiveEffect`),
//! which `OnFightStart` sorts by where each stands
//! (`FightUtility.SkillOwnerComparer`). Each update, after
//! `WreckageRecoverySystem`, the manager brings up a unit whose main skill's
//! search measured no enemy nearest, or whose nearest enemy or attack target
//! stands within the source's distance, and burrows any other
//! (`TeamBurrowManager.Update`). Burrowing writes the source's rate directly
//! through `FightMech.AddData(AmplifyDamageRate)`; coming up withdraws that
//! source with `FightMech.RemoveData`. A unit is held from its
//! deployment (`AddEffect`, `AddMech`), `Deactive`, and is `Normal` once
//! `FightEffectSystem.ActiveEffect` activates its effects (`DoActive`,
//! `BurrowSystem.Active`): as the fight starts, or as it lands, joins or
//! rises again. Its technologies switched off, or its death, bring it up and
//! leave it `Deactive`, in its place (`DisableEffect`, `DoDeactive`,
//! `BurrowSystem.Deactive`), until they come back (`EnableEffect`). A source that takes its unit
//! underground (`IsEnterUnderGround`) is refused by the technology table.
//! `docs/rules/technology_effects.md` states the rule.

use super::tower::rate;
use super::*;
use crate::data::{Channel, Entry, Index};
use crate::modifier::Burrow;

const SOURCE: &str = "BurrowTech";

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
    /// comes up (`TryBurrowUp`, its rate withdrawn) and its status is
    /// `Deactive`, which the manager's update passes over
    /// (`BurrowSystem.Deactive`); on, it is `Normal` again
    /// (`BurrowSystem.Active`).
    pub(in crate::fight) fn switch_burrow(&mut self, unit: u64, on: bool) -> Result<()> {
        if !self.burrow.order.contains(&unit) {
            return Ok(());
        }
        if on {
            self.burrow.deactive.remove(&unit);
            return Ok(());
        }
        self.burrow_up(unit)?;
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
                self.burrow_up(unit)?;
            } else {
                self.burrow_down(unit, &source)?;
            }
        }
        Ok(())
    }

    /// `BurrowEffectProvider.DoDeactive`: withdraw its rate and leave it deactive.
    pub(in crate::fight) fn remove_burrow_unit(&mut self, unit: u64) -> Result<()> {
        if self.burrow.order.contains(&unit) {
            self.burrow_up(unit)?;
            self.burrow.deactive.insert(unit);
        }
        Ok(())
    }

    /// `TeamBurrowManager.OnFightEnd` brings every held unit up.
    pub(in crate::fight) fn end_burrows_as_the_fight_ends(&mut self) -> Result<()> {
        for unit in self.burrow.order.clone() {
            self.burrow_up(unit)?;
        }
        Ok(())
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

    /// `TeamBurrowManager.TryBurrowDown` writes the source's rate directly.
    fn burrow_down(&mut self, unit: u64, source: &Burrow) -> Result<()> {
        if !self.burrow.burrowed.insert(unit) {
            return Ok(());
        }
        let actor = self.actors.get_mut(&unit).expect("a held unit exists");
        let rate_q32 = level_value(&source.amplify_damage_rate, actor.placement.level);
        actor.stats.overlays.channel(Channel::Unit).write(Entry {
            index: Index::AmplifyDamage,
            source: SOURCE,
            correction: rate(rate_q32),
        });
        actor.stats.refresh(&actor.rules)
    }

    /// `TeamBurrowManager.TryBurrowUp` withdraws the same source's rate.
    fn burrow_up(&mut self, unit: u64) -> Result<()> {
        if !self.burrow.burrowed.remove(&unit) {
            return Ok(());
        }
        let actor = self.actors.get_mut(&unit).expect("a held unit exists");
        actor.stats.overlays.channel(Channel::Unit).withdraw(SOURCE);
        actor.stats.refresh(&actor.rules)
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
