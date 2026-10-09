//! `AdditionalDamageProvider`: a technology whose unit's hits take a share
//! of each target's life besides.
//!
//! An `AdditionalDamageTech` answers `IAdditionalDamage` with
//! `GetReduceLifeRate`, Ionization's 0.5. Its provider, a
//! `SingleEffectProvider`, is a hit effect of the skills its row reaches
//! (`IHitEffectPerformer.PerformHitEffect`): after a first hit, not a second
//! damage's, each target it struck that still lives is handed, through
//! `FightActor.OnHitted`, a hit of that rate times the life it has left, the
//! `FPoint` product's whole part, from no object and under its unit's side.
//! `OnHitted` neither raises it by the target's rate on damage taken nor
//! takes its reduction off it: a unit's own shield takes it first, and the
//! rest comes off its life. A source that `CanDisable` takes nothing while
//! its unit's technologies are disabled. `docs/rules/technology_effects.md`
//! states the rule.

use super::*;

impl Simulation {
    /// `AdditionalDamageProvider.PerformHitEffect` of the hit the unit's
    /// skill at `slot` dealt to `targets`. Answers the deaths and falls it
    /// dealt.
    pub(in crate::fight) fn take_life_share(
        &mut self,
        owner: u64,
        slot: u16,
        targets: &[FightActorRef],
        events: &mut Vec<Event>,
    ) -> Result<Struck> {
        let mut ends = Struck::default();
        let Some(actor) = self.actors.get(&owner) else {
            return Ok(ends);
        };
        let Some(source) = actor.placement.effects.single.additional_damage else {
            return Ok(ends);
        };
        if (source.can_disable && actor.technology_disabled())
            || !actor
                .technology_skill_slots(source.extra_skills)
                .contains(&usize::from(slot))
            || source.rate_q32 <= 0
        {
            return Ok(ends);
        }
        let team = actor.placement.team;
        for &target in targets {
            let life = match target {
                FightActorRef::Unit(id) => match self.actors.get(&id) {
                    Some(unit) if unit.alive() => unit.life,
                    _ => continue,
                },
                FightActorRef::Building(id) => match self
                    .buildings
                    .iter()
                    .find(|building| building.building_id == id)
                {
                    Some(building) if building_alive(building) => i64::from(building.life.current),
                    _ => continue,
                },
            };
            let amount = q32_mul(life << 32, source.rate_q32) >> 32;
            let stroke = match target {
                FightActorRef::Unit(id) => self.land(id, (None, team), (amount, amount), events)?,
                FightActorRef::Building(_) => self.strike_target(
                    target,
                    None,
                    team,
                    (amount, false),
                    Provider::Other,
                    events,
                )?,
            };
            // `OnHitted` raises no hit event: nothing counts or records the
            // damage beyond the life it took. Half a unit's life left never
            // empties it.
            let id = match target {
                FightActorRef::Unit(id) | FightActorRef::Building(id) => id,
            };
            if let Some(position) = stroke.death {
                ends.deaths.push((id, position));
                ends.ends.push((target, position));
            }
            if let Some(position) = stroke.fallen {
                ends.fallen.push((id, position));
                ends.ends.push((target, position));
            }
        }
        Ok(ends)
    }
}
