//! `ClearRangeItemSystem`: the terrain a unit's technology clears about it.
//!
//! A unit with an `IClearRangeItem` in force (a Hound's Fire Extinguisher)
//! is in its side's `TeamClearRangeItemManager`, active while it lives
//! (`ClearRangeItemEffectProvider.DoDeactive` as it dies) and its
//! technologies are not disabled (`DisableEffect`). Each manager counts its
//! updates and clears on every second (`m_updateInterval`), after
//! `WreckageRecoverySystem` and before `SiegeModeEffectSystem`: for each
//! active unit and each kind its source names, the terrain of that kind
//! about the unit loses its cells within a circle of the unit's whole radius
//! plus the source's radius, a terrain of either side alike, and one left
//! with none goes. Which unit clears first changes nothing that is left.
//! `docs/rules/technology_effects.md` states the rule.

use super::*;

/// `TeamClearRangeItemManager.m_updateInterval`.
const CLEAR_INTERVAL: i32 = 2;

impl Simulation {
    /// `ClearRangeItemSystem.Update`: each side's
    /// `TeamClearRangeItemManager.Update`. Both count the same updates, so
    /// one count stands for both (`m_deltaTime`).
    pub(in crate::fight) fn step_clear_range_items(&mut self) -> Result<()> {
        self.clear_range_item_time += 1;
        if self.clear_range_item_time < CLEAR_INTERVAL {
            return Ok(());
        }
        self.clear_range_item_time = 0;
        let clearing = self
            .actors
            .iter()
            .filter(|(_, actor)| {
                actor.placement.clear_range_item.is_some()
                    && actor.alive()
                    && !actor.technology_disabled()
            })
            .map(|(&id, _)| id)
            .collect::<Vec<_>>();
        for id in clearing {
            let actor = &self.actors[&id];
            let source = actor
                .placement
                .clear_range_item
                .clone()
                .expect("a clearing unit has a source");
            // `RemoveRangeItemGrids`: the circle about `FightTransform.
            // position2D`, its radius the source's whole metres and the
            // whole part of the unit's radius.
            let radius =
                i64::from(source.radius) + (space_to_q32(actor.rules.collision_radius()) >> 32);
            let circle = (actor.x_q32, actor.z_q32, radius << 32);
            for kind in source.kinds {
                self.clear_terrain(kind, circle)?;
            }
        }
        Ok(())
    }
}
