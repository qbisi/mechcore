//! `FightCoreSystem`'s kill count: whom a target's death credits.
//!
//! `FightCoreSystem.OnActorHitted` hears every hit, as `ExpSystem`'s does. It
//! keeps each target's list of the skill owners that hit it, each once, and
//! when a hit leaves the target dead it calls `ISkillOwner.AddKillCount` on
//! every one of them still alive, the killer or not, then forgets the list.
//! A unit's `FightMech.AddKillCount` reaches its skills' `DamageCalculator`,
//! whose count a kill-count damage rate multiplies; a building's count is
//! read by nothing here.

use super::*;

/// `FightCoreSystem.attackData`: each target's attackers, first hit first.
#[derive(Default)]
pub(in crate::fight) struct KillCounts {
    attackers: BTreeMap<ObjectRef, Vec<ObjectRef>>,
}

impl Simulation {
    /// `FightCoreSystem.OnActorHitted`: the hit's source joins the target's
    /// list, and a death credits everyone on it who is alive.
    pub(in crate::fight) fn count_kill(
        &mut self,
        source: Option<ObjectRef>,
        target: FightActorRef,
        killed: bool,
    ) -> Result<()> {
        let key = target.object_ref();
        let attackers = self.kills.attackers.entry(key).or_default();
        if let Some(source) = source
            && !attackers.contains(&source)
        {
            attackers.push(source);
        }
        if !killed {
            return Ok(());
        }
        for attacker in self.kills.attackers.remove(&key).unwrap_or_default() {
            if attacker.kind != ObjectKind::Unit {
                continue;
            }
            if let Some(actor) = self.actors.get_mut(&attacker.id)
                && actor.alive()
            {
                actor.stats.add_kill(&actor.rules)?;
            }
        }
        Ok(())
    }

    /// `DamageCalculator.Clear` on every unit's skills as the fight ends: the
    /// last state reads each unit's damage without its kills.
    pub(in crate::fight) fn clear_kills(&mut self) -> Result<()> {
        for actor in self.actors.values_mut() {
            actor.stats.clear_kills(&actor.rules)?;
        }
        Ok(())
    }
}
