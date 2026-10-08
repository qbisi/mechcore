//! The normal form's numbering of a first snapshot's units, for a producer
//! that numbered them otherwise.
//!
//! The normal form counts every unit the first snapshot holds as an initial
//! unit: they sort by `(team_id, position.z, position.x)` and take their IDs in
//! that order, and formations take theirs in the order those units first meet
//! them. A producer that numbers the units it places before the fight, and a
//! unit made during the first tick after them, holds a first snapshot out of
//! that order when such a unit joins at once: a production line whose makes
//! appear with no delay. [`UnitNumbering`] renames the producer's units and
//! formations into the normal form's, and leaves every one first numbered
//! later as it is.

use std::collections::BTreeMap;

use crate::model::{
    Event, EventPayload, ObjectKind, ObjectRef, RecorderKind, WorldSnapshot,
    compare_initial_unit_order,
};

/// A renaming of units and formations, each entry an ID that changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnitNumbering {
    units: BTreeMap<u64, u64>,
    formations: BTreeMap<u64, u64>,
}

impl UnitNumbering {
    /// The renaming that brings a first snapshot's units into the normal
    /// form, the producer's own IDs handed out again in its order.
    #[must_use]
    pub fn of_first_snapshot(snapshot: &WorldSnapshot) -> Self {
        let mut sorted = snapshot.live_units.iter().collect::<Vec<_>>();
        sorted.sort_by(|left, right| {
            compare_initial_unit_order(left.team_id, &left.position, right.team_id, &right.position)
        });
        let mut unit_ids = sorted.iter().map(|unit| unit.unit_id).collect::<Vec<_>>();
        unit_ids.sort_unstable();
        let mut met = Vec::new();
        for unit in &sorted {
            if !met.contains(&unit.formation_id) {
                met.push(unit.formation_id);
            }
        }
        let mut formation_ids = met.clone();
        formation_ids.sort_unstable();
        let changed = |pairs: Vec<(u64, u64)>| {
            pairs
                .into_iter()
                .filter(|(old, new)| old != new)
                .collect::<BTreeMap<_, _>>()
        };
        Self {
            units: changed(
                sorted
                    .iter()
                    .map(|unit| unit.unit_id)
                    .zip(unit_ids)
                    .collect(),
            ),
            formations: changed(met.into_iter().zip(formation_ids).collect()),
        }
    }

    /// Whether it renames nothing.
    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.units.is_empty() && self.formations.is_empty()
    }

    /// The snapshot and the tick's events with every unit and formation the
    /// renaming names under its new ID, the snapshot put back in order.
    pub fn apply(&self, snapshot: &mut WorldSnapshot, events: &mut [Event]) {
        if self.is_identity() {
            return;
        }
        let unit = |id: &mut u64| *id = self.units.get(id).copied().unwrap_or(*id);
        let formation = |id: &mut u64| *id = self.formations.get(id).copied().unwrap_or(*id);
        let reference = |reference: &mut ObjectRef| {
            if reference.kind == ObjectKind::Unit {
                unit(&mut reference.id);
            }
        };
        let optional = |value: &mut Option<ObjectRef>| {
            if let Some(value) = value {
                reference(value);
            }
        };
        for live in &mut snapshot.live_units {
            unit(&mut live.unit_id);
            formation(&mut live.formation_id);
            optional(&mut live.mech_lock_target);
            for buff in &mut live.buffs {
                optional(&mut buff.source);
            }
            for skill in &mut live.skills {
                if let Some(enabled) = &mut skill.enabled {
                    optional(&mut enabled.lock_target);
                    optional(&mut enabled.attack_target);
                }
            }
            if let Some(control) = &mut live.control {
                control.sources.iter_mut().for_each(reference);
            }
        }
        for projectile in &mut snapshot.projectiles {
            optional(&mut projectile.owner);
            optional(&mut projectile.target);
        }
        for shield in &mut snapshot.shields {
            optional(&mut shield.owner);
        }
        for terrain in &mut snapshot.terrains {
            for application in &mut terrain.applications {
                unit(&mut application.unit_id);
            }
        }
        for row in &mut snapshot.statistics {
            match row.recorder {
                RecorderKind::Formation => formation(&mut row.recorder_id),
                RecorderKind::Unit => unit(&mut row.recorder_id),
                RecorderKind::Construction => {}
            }
        }
        for row in &mut snapshot.formations {
            formation(&mut row.formation_id);
        }
        for event in events {
            optional(&mut event.subject);
            optional(&mut event.source);
            optional(&mut event.target);
            match &mut event.payload {
                EventPayload::ProjectileRemoved { absorbed_by, .. } => optional(absorbed_by),
                EventPayload::UnitCreated { formation_id, .. } => formation(formation_id),
                _ => {}
            }
        }
        snapshot.canonicalize();
    }
}
