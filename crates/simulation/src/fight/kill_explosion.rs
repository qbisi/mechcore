//! `KillExplosionEffectProvider`: the units a hit of a kill-explosion
//! technology's unit's skill kills explode (a Typhoon's Wreckage
//! Detonation).
//!
//! `KillExplosionEffectProvider.EnableEffect` hands the skills
//! `SkillDataModifier.AvaliableCheck` admits a hit effect
//! (`SkillManager.AddHitEffect`). After a hit of one of them
//! (`PerformHitEffect`), unless the unit's technologies are disabled and the
//! source `CanDisable`, the units the hit struck, in the order it struck
//! them and passing over what is not a unit, explode while each is dead and
//! its death the skill's unit's (`FightActor.deadSourceSkillOwner`): the
//! first alive, or another's to have killed, ends the walk, and those after
//! it do not explode however they died. Each explodes in `DamagePerformer.Perform`
//! with a `KillExplosionDamageProvider` aimed at it, owned by the skill's
//! unit and of its side as it now stands (`GetTeamController`), dealing the
//! source's damage at the unit's level, raised by its tower buffs where the
//! source says so (`GetDamage`), to everything of the domain the dead unit
//! was of (`GetTargetType`, `IsFly`) within the source's range of where it
//! fell (`CalculateDamagePosition`, `GetSplashRange`), the unit's own side
//! too where the source hits allies (`GetEffectTargetType`). A unit an
//! explosion kills that the hit struck later explodes in its turn, its death
//! the skill's unit's. Where the source chains
//! (`CanExplosionTriggerExplosion`), the units an explosion struck explode
//! in turn, walked as the hit's are while each died of that explosion
//! (`deadSourceDamageProvider`,
//! `KillExplosionDamageProvider.DispatchHitDamageEvent`).
//! `docs/rules/technology_effects.md` states the rule.

use super::*;
use crate::{data::Index, modifier::KillExplosion};

impl Simulation {
    /// `KillExplosionEffectProvider.PerformHitEffect` after a hit of the
    /// unit's skill in `slot`: the deaths and falls its explosions dealt,
    /// which follow the hit's own.
    pub(in crate::fight) fn explode_kills(
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
        let Some(source) = actor.placement.effects.single.kill_explosion.clone() else {
            return Ok(ends);
        };
        if (source.can_disable && actor.technology_disabled())
            || !actor.corrected_skill_slots().contains(&usize::from(slot))
        {
            return Ok(ends);
        }
        let killer = Some(actor.object_ref());
        for &target in targets {
            let FightActorRef::Unit(unit) = target else {
                continue;
            };
            let dead = &self.actors[&unit];
            if dead.alive() || dead.last_damage_source.and_then(|(source, _)| source) != killer {
                break;
            }
            self.explode_kill(owner, &source, unit, &mut ends, events)?;
        }
        Ok(ends)
    }

    /// `DamagePerformer.Perform` of one `KillExplosionDamageProvider`, and
    /// the explosions it sets off in turn where the source chains.
    fn explode_kill(
        &mut self,
        owner: u64,
        source: &KillExplosion,
        unit: u64,
        ends: &mut Struck,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let actor = &self.actors[&owner];
        // `KillExplosionTech.GetExplosionDamage`: the level's entry, the last
        // past the list.
        let damage = usize::try_from(actor.placement.level - 1)
            .ok()
            .and_then(|index| source.damage.get(index))
            .or_else(|| source.damage.last())
            .copied()
            .unwrap_or_default();
        let amount = if source.buffed {
            actor.stats.overlays.scaled_by_buffs_of(
                super::tower::SOURCE,
                Index::AttackDamage,
                damage,
            )
        } else {
            damage
        };
        let team = actor.placement.team;
        let dead = &self.actors[&unit];
        let hit = DamageHit {
            source: Some(actor.object_ref()),
            source_team: team,
            team,
            effect: if source.hits_allies {
                EffectTarget::Both
            } else {
                EffectTarget::Opponent
            },
            amount,
            provider: Provider::Other,
            projectile: None,
            skill_slot: None,
            aimed: Some(FightActorRef::Unit(unit)),
            hits_aimed: false,
            center_q32: (dead.x_q32, dead.z_q32),
            center_y_q32: space_to_q32(unit_height(dead.domain)),
            shield: None,
            crosses_shields: false,
            strikes_buildings: true,
            splash_radius: source.range,
            fire: false,
            shield_damage: None,
            reach: Reach::Domain(dead.domain),
        };
        let struck = self.perform_damage(hit, events)?;
        let targets = struck.targets.clone();
        let deaths = struck
            .deaths
            .iter()
            .map(|&(dead, _)| dead)
            .collect::<Vec<_>>();
        ends.absorb_ends(struck);
        if !source.chains {
            return Ok(());
        }
        for target in targets {
            let FightActorRef::Unit(unit) = target else {
                continue;
            };
            if self.actors[&unit].alive() || !deaths.contains(&unit) {
                break;
            }
            self.explode_kill(owner, source, unit, ends, events)?;
        }
        Ok(())
    }
}
