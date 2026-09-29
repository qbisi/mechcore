//! The build's own damage and kill counters, `BattleStatisticManager`'s
//! current round, kept as the build keeps them.
//!
//! `FightController.OnActorHitted` credits every hit to the attacker's
//! recorder and charges it to the target's. A recorder is a formation, whose
//! units count together, or a construction's blocks, which count together as
//! its `FightConstructionCombination`; a tower is neither. Every formation and
//! every construction has its entry, at zero, from deployment.

use super::*;

/// A statistics entry's place: its side, kind and recorder.
pub(in crate::fight) type RecorderKey = (u32, RecorderKind, u64);

impl Simulation {
    /// Every formation's and construction's entry, at zero, and each
    /// construction block's recorder.
    pub(in crate::fight) fn seed_statistics(&mut self, groups: &BTreeMap<u64, (u32, usize)>) {
        let formations = self
            .actors
            .values()
            .map(|actor| {
                (
                    actor.placement.team,
                    RecorderKind::Formation,
                    actor.placement.formation_id,
                )
            })
            .collect::<Vec<_>>();
        // A construction's blocks are named by the lowest of their buildings.
        let mut lowest = BTreeMap::<(u32, usize), u64>::new();
        for (building, group) in groups {
            let entry = lowest.entry(*group).or_insert(*building);
            *entry = (*entry).min(*building);
        }
        self.construction_recorders = groups
            .iter()
            .map(|(building, group)| {
                (
                    *building,
                    (group.0, RecorderKind::Construction, lowest[group]),
                )
            })
            .collect();
        let constructions = self
            .construction_recorders
            .values()
            .copied()
            .collect::<Vec<_>>();
        for key in formations.into_iter().chain(constructions) {
            self.statistics.entry(key).or_insert(DamageStatistics {
                team_id: key.0,
                recorder: key.1,
                recorder_id: key.2,
                damage: 0,
                damage_real: 0,
                kills: 0,
                damage_taken: 0,
            });
        }
    }

    fn recorder(&self, object: ObjectRef) -> Option<RecorderKey> {
        match object.kind {
            ObjectKind::Unit => self.actors.get(&object.id).map(|actor| {
                (
                    actor.placement.team,
                    RecorderKind::Formation,
                    actor.placement.formation_id,
                )
            }),
            ObjectKind::Building => self.construction_recorders.get(&object.id).copied(),
            _ => None,
        }
    }

    /// One hit's credit to its attacker and charge to its target.
    pub(in crate::fight) fn count_hit(
        &mut self,
        source: ObjectRef,
        source_team: u32,
        target: FightActorRef,
        stroke: &Stroke,
    ) -> Result<()> {
        if !stroke.reached_alive {
            return Ok(());
        }
        self.count_experience(source, source_team, target, stroke.killed)?;
        let narrow =
            |value: i64| i32::try_from(value).map_err(|_| Error::new("a counted hit exceeds i32"));
        if let Some(key) = self.recorder(source) {
            let (dealt, actual) = (narrow(stroke.dealt)?, narrow(stroke.actual)?);
            if let Some(row) = self.statistics.get_mut(&key) {
                row.damage = row.damage.wrapping_add(dealt);
                row.damage_real = row.damage_real.wrapping_add(actual);
                row.kills = row.kills.wrapping_add(i32::from(stroke.killed));
            }
        }
        if let Some(key) = self.recorder(target.object_ref()) {
            let taken = narrow(stroke.taken)?;
            if let Some(row) = self.statistics.get_mut(&key) {
                row.damage_taken = row.damage_taken.wrapping_add(taken);
            }
        }
        Ok(())
    }
}
