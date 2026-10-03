//! `RangeItemSystem`: the terrains battle skills leave, and the units
//! standing in them.
//!
//! A terrain skill's sub-effect lands through `RangeItemEffectController`,
//! whose `RangeItemSystem.AddItem` hands a `RangeItem` to the controller of
//! its kind. Each tick, after `MineSystem` and before `FightCoreSystem`, each
//! controller holding items runs `RangeItemController.Update`: its items
//! age and the ones whose time is up go (`UpdateItemStatus`), then it works
//! out who stands in them (`UpdateAffectedActorChange`), then it runs the
//! periodic effects. `docs/rules/terrain.md` states the rule.

use super::*;
use crate::{
    data::{Channel, Correction, Entry, Index},
    layout::{TerrainEffect, TerrainKind, TerrainSpec},
};
use mechcore_mcfr::{
    TerrainApplicationState, TerrainLogicLifetime, TerrainRemovedReason, TerrainState, TerrainType,
};

/// What tags the entries a fog writes: the controller is the
/// `IDataModifier`, so a unit carries one fog's rate however many it stands in.
const FOG_SOURCE: &str = "FogController";

/// A `RangeItemController`'s quadtree holds 20 items to a node before it
/// splits; a split changes the order units are found in, which is not
/// read, so a controller is refused that many.
const ITEMS_BEFORE_A_SPLIT: usize = 19;

/// `RangeItemSystem`: every terrain, and a controller per kind in play.
#[derive(Default)]
pub(in crate::fight) struct TerrainSystem {
    terrains: BTreeMap<u64, Terrain>,
    controllers: Vec<TerrainController>,
    next_id: u64,
}

/// A `RangeItem`.
struct Terrain {
    team: u32,
    x_q32: i64,
    z_q32: i64,
    spec: TerrainSpec,
    /// `time`, counted by each update when the terrain has a life.
    elapsed: i32,
}

/// A `RangeItemController`: its items in the order they were added, and the
/// units it affects in the order they entered (`affectedUnits` and
/// `affectedUnitTimes`).
struct TerrainController {
    kind: TerrainKind,
    items: Vec<u64>,
    affected: Vec<Affected>,
    /// `effectTimeDuration` in ticks: how often a periodic effect repeats,
    /// none for a fog's.
    period: Option<i32>,
}

struct Affected {
    unit: u64,
    terrain: u64,
    time: i32,
}

const fn terrain_type(kind: TerrainKind) -> TerrainType {
    match kind {
        TerrainKind::Fire => TerrainType::Fire,
        TerrainKind::Oil => TerrainType::Oil,
        TerrainKind::Fog => TerrainType::Fog,
        TerrainKind::Acid => TerrainType::Acid,
    }
}

impl Simulation {
    /// `RangeItemSystem.AddItem` from a landing sub-effect: the terrain joins
    /// its kind's controller, which it creates the first time.
    pub(in crate::fight) fn add_terrain(
        &mut self,
        team: u32,
        name: &str,
        spec: TerrainSpec,
        (x_q32, z_q32): (i64, i64),
        events: &mut Vec<Event>,
    ) -> Result<()> {
        // A shield turns a terrain to a grid of cells, which is not read.
        if !self.shield.standing.is_empty() {
            return Err(Error::new(format!(
                "{name} leaves a terrain in a fight with a battlefield shield, which is not measured"
            )));
        }
        if self
            .terrain
            .controllers
            .iter()
            .any(|controller| controller.kind != spec.kind)
        {
            return Err(Error::new(format!(
                "{name} leaves a terrain beside one of another kind, and the order their \
                 controllers update in is not read"
            )));
        }
        if !self
            .terrain
            .controllers
            .iter()
            .any(|controller| controller.kind == spec.kind)
        {
            self.terrain.controllers.push(TerrainController {
                kind: spec.kind,
                items: Vec::new(),
                affected: Vec::new(),
                period: None,
            });
        }
        let index = self
            .terrain
            .controllers
            .iter()
            .position(|controller| controller.kind == spec.kind)
            .expect("the kind's controller was just made");
        if self.terrain.controllers[index].items.len() >= ITEMS_BEFORE_A_SPLIT {
            return Err(Error::new(format!(
                "{name} leaves its kind's twentieth terrain, which splits its controller's \
                 quadtree, and what that does to the order units are found in is not read"
            )));
        }
        self.terrain.next_id += 1;
        let id = self.terrain.next_id;
        self.terrain.terrains.insert(
            id,
            Terrain {
                team,
                x_q32,
                z_q32,
                spec,
                elapsed: 0,
            },
        );
        self.terrain.controllers[index].items.push(id);
        events.push(event(
            Some(ObjectRef::new(ObjectKind::Terrain, id)),
            None,
            Some(team),
            None,
            EventPayload::TerrainCreated {
                team_id: Some(team),
                terrain_type: terrain_type(spec.kind),
                position: QVec3 {
                    x: x_q32,
                    y: 0,
                    z: z_q32,
                },
                radius: spec.radius_q32,
            },
        ));
        Ok(())
    }

    /// `RangeItemSystem.Update`: each controller holding items, in the
    /// order the system made them.
    pub(in crate::fight) fn step_terrains(&mut self, events: &mut Vec<Event>) -> Result<()> {
        for index in 0..self.terrain.controllers.len() {
            if self.terrain.controllers[index].items.is_empty() {
                continue;
            }
            self.forget_the_dead(index);
            self.update_item_status(index, events);
            self.update_affected(index)?;
        }
        Ok(())
    }

    /// `OnMechDead`: a unit that died leaves the controller, keeping the
    /// order of the rest.
    fn forget_the_dead(&mut self, index: usize) {
        let actors = &self.actors;
        self.terrain.controllers[index]
            .affected
            .retain(|affected| actors[&affected.unit].alive());
    }

    /// `UpdateItemStatus`: each item, last first, ages, and goes once its
    /// time is up.
    fn update_item_status(&mut self, index: usize, events: &mut Vec<Event>) {
        let items = self.terrain.controllers[index].items.clone();
        for &id in items.iter().rev() {
            let terrain = self
                .terrain
                .terrains
                .get_mut(&id)
                .expect("an item's terrain exists");
            let Some(limit) = terrain.spec.life_ticks else {
                continue;
            };
            terrain.elapsed += 1;
            if terrain.elapsed >= limit {
                self.remove_terrain(index, id, TerrainRemovedReason::TimeExpired, events);
            }
        }
    }

    fn remove_terrain(
        &mut self,
        index: usize,
        id: u64,
        reason: TerrainRemovedReason,
        events: &mut Vec<Event>,
    ) {
        self.terrain.controllers[index]
            .items
            .retain(|&item| item != id);
        if let Some(terrain) = self.terrain.terrains.remove(&id) {
            events.push(event(
                Some(ObjectRef::new(ObjectKind::Terrain, id)),
                None,
                None,
                None,
                EventPayload::TerrainRemoved {
                    position: QVec3 {
                        x: terrain.x_q32,
                        y: 0,
                        z: terrain.z_q32,
                    },
                    reason,
                },
            ));
        }
    }

    /// `UpdateAffectedActorChange`. Side by side, each item against every
    /// unit of the side found by querying its units' tree with the item
    /// tree's node: a ground unit valid as a target whose edge the item's
    /// range reaches, in two dimensions. Every affected unit not found
    /// leaves, last first; every unit found that is not affected enters the
    /// first item it was found in, and one already affected keeps its own.
    fn update_affected(&mut self, index: usize) -> Result<()> {
        let mut found = Vec::<(u64, u64)>::new();
        let items = self.terrain.controllers[index].items.clone();
        for team in [0_u32, 1] {
            let Some(tree) = self.mech_quadtrees.get(&team) else {
                continue;
            };
            let units = tree.query_rect(TargetActorRect::map());
            for &item in &items {
                let terrain = &self.terrain.terrains[&item];
                for &unit in &units {
                    let FightActorRef::Unit(unit_id) = unit else {
                        continue;
                    };
                    let actor = &self.actors[&unit_id];
                    if !actor.alive()
                        || actor.visibility == Visibility::Hide
                        || actor.rules.domain == UnitDomain::Air
                    {
                        continue;
                    }
                    let edge = native_q32_magnitude(
                        actor.x_q32.saturating_sub(terrain.x_q32),
                        actor.z_q32.saturating_sub(terrain.z_q32),
                    )
                    .saturating_sub(space_to_q32(actor.rules.collision_radius()));
                    if fpoint_less_or_equal(edge, terrain.spec.radius_q32) {
                        found.push((unit_id, item));
                    }
                }
            }
        }
        let affected = self.terrain.controllers[index]
            .affected
            .iter()
            .map(|affected| affected.unit)
            .collect::<Vec<_>>();
        for &unit in affected.iter().rev() {
            if !found.iter().any(|&(candidate, _)| candidate == unit) {
                self.exit_terrain(index, unit)?;
            }
        }
        for (unit, item) in found {
            if self.terrain.controllers[index]
                .affected
                .iter()
                .any(|affected| affected.unit == unit)
            {
                continue;
            }
            self.terrain.controllers[index].affected.push(Affected {
                unit,
                terrain: item,
                time: 0,
            });
            self.perform_terrain_effect(item, unit)?;
        }
        Ok(())
    }

    /// The controller's `PerformItemEffect` on a unit standing in the item.
    fn perform_terrain_effect(&mut self, item: u64, unit: u64) -> Result<()> {
        match self.terrain.terrains[&item].spec.effect {
            // `FogController.PerformItemEffect`: the rate on every skill
            // that is not a melee attack, the controller its modifier.
            TerrainEffect::Fog { attack_range_rate } => {
                let actor = self
                    .actors
                    .get_mut(&unit)
                    .expect("actor identity is stable");
                if actor.rules.attack.melee {
                    return Ok(());
                }
                let skill = actor.stats.overlays.channel(Channel::Skill);
                skill.withdraw(FOG_SOURCE);
                skill.write(Entry {
                    index: Index::AttackRange,
                    source: FOG_SOURCE,
                    correction: if attack_range_rate < 0 {
                        Correction::Rate {
                            add: 0,
                            reduce: -attack_range_rate,
                        }
                    } else {
                        Correction::Rate {
                            add: attack_range_rate,
                            reduce: 0,
                        }
                    },
                });
                actor.stats.refresh(&actor.rules)
            }
        }
    }

    /// `OnActorExit`: the unit leaves the controller, and a fog takes its
    /// rate back.
    fn exit_terrain(&mut self, index: usize, unit: u64) -> Result<()> {
        let controller = &mut self.terrain.controllers[index];
        controller.affected.retain(|affected| affected.unit != unit);
        if controller.kind == TerrainKind::Fog {
            let actor = self
                .actors
                .get_mut(&unit)
                .expect("actor identity is stable");
            actor
                .stats
                .overlays
                .channel(Channel::Skill)
                .withdraw(FOG_SOURCE);
            actor.stats.refresh(&actor.rules)?;
        }
        Ok(())
    }

    /// As the fight is left every terrain goes, the round over for it, and
    /// takes back what it did to the units still in it.
    pub(in crate::fight) fn clear_terrains_as_the_fight_ends(
        &mut self,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        for index in 0..self.terrain.controllers.len() {
            self.forget_the_dead(index);
            let affected = self.terrain.controllers[index]
                .affected
                .iter()
                .map(|affected| affected.unit)
                .collect::<Vec<_>>();
            for unit in affected {
                self.exit_terrain(index, unit)?;
            }
            for id in self.terrain.controllers[index].items.clone() {
                self.remove_terrain(index, id, TerrainRemovedReason::RoundExpired, events);
            }
        }
        Ok(())
    }

    /// Every terrain as a snapshot holds it, with the living units it
    /// affects.
    pub(in crate::fight) fn terrain_states(&self) -> Vec<TerrainState> {
        let mut states = Vec::new();
        for controller in &self.terrain.controllers {
            for &id in &controller.items {
                let terrain = &self.terrain.terrains[&id];
                let mut applications = controller
                    .affected
                    .iter()
                    .filter(|affected| {
                        affected.terrain == id && self.actors[&affected.unit].alive()
                    })
                    .map(|affected| TerrainApplicationState {
                        unit_id: affected.unit,
                        periodic_clock: controller.period.map(|duration| {
                            mechcore_mcfr::TerrainEffectClock {
                                elapsed: affected.time,
                                duration,
                            }
                        }),
                    })
                    .collect::<Vec<_>>();
                applications.sort_by_key(|application| application.unit_id);
                states.push(TerrainState {
                    terrain_id: id,
                    team_id: Some(terrain.team),
                    terrain_type: terrain_type(terrain.spec.kind),
                    position: QVec3 {
                        x: terrain.x_q32,
                        y: 0,
                        z: terrain.z_q32,
                    },
                    radius: terrain.spec.radius_q32,
                    grid: None,
                    remaining_rounds: None,
                    logic_lifetime: terrain.spec.life_ticks.map(|limit| TerrainLogicLifetime {
                        elapsed: terrain.elapsed,
                        limit,
                    }),
                    applications,
                });
            }
        }
        states
    }
}
