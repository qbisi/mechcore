//! `SpawnAdvancedShieldEffectProvider`: a technology whose unit's hits
//! spawn battlefield shields where it stands.
//!
//! A `SpawnAdvancedShieldTech` answers `ISpawnAdvancedShieldDataSource`,
//! Accumulator Shield's: a shield of 40 metres and 2000 energy a level,
//! after 5 hits, 5 more for each shield after. Its provider, a
//! `SingleEffectProvider`, makes the unit a `SpawnAdvancedShieldController`,
//! a hit effect of the main skill (`SkillManager.AddHitEffect`). Each first
//! hit, not a second damage's, of a source not switched off counts once,
//! whatever it struck, until the unit has spawned its most; once the count
//! reaches the attacks plus the increment for each shield spawned, it starts
//! over and a shield is spawned (`AdvancedEnergyShieldSystem.Create`) where
//! the unit stands, of its side, with no owner, active and full: it stands
//! until a hit empties it, and the round's end destroys it
//! (`IsShortLifeTime`). The counts outlast the unit's technologies switched
//! off, and the shields stand whatever befalls the unit.
//! `docs/rules/technology_effects.md` states the rule.

use super::*;

impl Simulation {
    /// `SpawnAdvancedShieldController.PerformHitEffect` of a first hit of
    /// the unit's skill at `slot`.
    pub(in crate::fight) fn count_shield_hit(&mut self, owner: u64, slot: u16) {
        let Some(actor) = self.actors.get_mut(&owner) else {
            return;
        };
        let Some(source) = actor.placement.effects.single.spawn_shield.clone() else {
            return;
        };
        if (source.can_disable && actor.technology_disabled())
            || usize::from(slot) >= actor.skills.main_slots()
            || actor.spawned_shields >= source.max
        {
            return;
        }
        actor.shield_hits += 1;
        let threshold = source.attacks + source.increment * actor.spawned_shields;
        if actor.shield_hits < threshold {
            return;
        }
        actor.shield_hits -= threshold;
        actor.spawned_shields += 1;
        let level = actor.placement.level;
        let (team, x_q32, z_q32) = (actor.placement.team, actor.x_q32, actor.z_q32);
        let radius_q32 = super::burrow::level_value(&source.radius, level);
        let energy = super::burrow::level_value(&source.energy, level);
        self.create_shield(
            (team, mechcore_mcfr::ShieldSourceKind::SpawnedTemporary),
            x_q32,
            z_q32,
            radius_q32,
            energy,
        );
    }
}
