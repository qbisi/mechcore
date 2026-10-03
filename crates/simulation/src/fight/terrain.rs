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
//!
//! A recording finds a terrain made or gone by comparing two snapshots'
//! terrains, so the events come last in their tick: the made ones, then the
//! gone ones, each in identity order. It names a terrain the first time a
//! snapshot holds it, controller by controller in the system's order and
//! item by item, so a terrain made and gone within one tick is never named.

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
    /// Every terrain made, by key, the gone ones too: a unit a controller
    /// still holds may name one its controller no longer does.
    terrains: BTreeMap<u64, Terrain>,
    /// In the order `RangeItemSystem.Init` makes them.
    controllers: Vec<TerrainController>,
    next_key: u64,
    /// The identity a recording gave each terrain it saw, by key.
    ids: BTreeMap<u64, u64>,
    next_id: u64,
    /// The terrains the last snapshot held, by key.
    seen: Vec<u64>,
    /// Why a terrain went, when a recording can tell; any other goes
    /// `unknown`.
    reasons: BTreeMap<u64, TerrainRemovedReason>,
}

/// A `RangeItem`.
struct Terrain {
    /// The skill that left it, for a refusal, and with `provider_team` its
    /// `IRangeItemProvider`: a fire an oil turns to keeps the oil's.
    name: String,
    provider_team: u32,
    team: u32,
    x_q32: i64,
    z_q32: i64,
    spec: TerrainSpec,
    /// `time`, counted by each update when the terrain has a life.
    elapsed: i32,
    /// `Round`: the rounds it has stood, counted as each fight ends.
    round: i32,
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

/// A controller's place in `RangeItemSystem.Init`, which `Update` keeps:
/// fire, oil, fog, sand fog, acid, recovery zone.
const fn controller_rank(kind: TerrainKind) -> u8 {
    match kind {
        TerrainKind::Fire => 0,
        TerrainKind::Oil => 1,
        TerrainKind::Fog => 2,
        TerrainKind::Acid => 4,
    }
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
    /// `RangeItemSystem.AddItem` from a landing sub-effect: the terrain
    /// joins its kind's controller. `interactiveInfos` holds one interaction,
    /// a fire with oil: an oil a fire reaches as it lands turns at once to a
    /// fire of the oil's, under the oil's side, which takes every oil it
    /// reaches in turn.
    pub(in crate::fight) fn add_terrain(
        &mut self,
        team: u32,
        name: &str,
        spec: TerrainSpec,
        position: (i64, i64),
    ) -> Result<()> {
        // A shield turns a terrain to a grid of cells, which is not read.
        if !self.shield.standing.is_empty() {
            return Err(Error::new(format!(
                "{name} leaves a terrain in a fight with a battlefield shield, which is not measured"
            )));
        }
        self.add_item((team, name), team, spec, position)?;
        if let Some(fire) = spec.burning()
            && self.fire_reaches(position, spec.radius_q32)
        {
            self.add_item((team, name), team, fire, position)?;
        }
        Ok(())
    }

    /// `RangeItemSystem.DoAddItem`. A fire of a provider that already has
    /// one standing where it lands is not made again: the standing one burns
    /// from the start (`FightGroundFire.Reset`). Any other terrain joins its
    /// kind's controller, which is made the first time, and a fire then takes
    /// the oils it reaches (`CheckInteractableItems`).
    fn add_item(
        &mut self,
        (provider_team, name): (u32, &str),
        team: u32,
        spec: TerrainSpec,
        (x_q32, z_q32): (i64, i64),
    ) -> Result<()> {
        if spec.kind == TerrainKind::Fire
            && let Some(repeat) = self.controller_of(TerrainKind::Fire).and_then(|index| {
                self.terrain.controllers[index]
                    .items
                    .iter()
                    .copied()
                    .find(|key| {
                        let terrain = &self.terrain.terrains[key];
                        terrain.provider_team == provider_team
                            && terrain.name == name
                            && (terrain.x_q32, terrain.z_q32) == (x_q32, z_q32)
                    })
            })
        {
            self.terrain
                .terrains
                .get_mut(&repeat)
                .expect("an item's terrain exists")
                .elapsed = 0;
            return Ok(());
        }
        let index = self.controller_for(spec);
        if self.terrain.controllers[index].items.len() >= ITEMS_BEFORE_A_SPLIT {
            return Err(Error::new(format!(
                "{name} leaves its kind's twentieth terrain, which splits its controller's \
                 quadtree, and what that does to the order units are found in is not read"
            )));
        }
        self.terrain.next_key += 1;
        let key = self.terrain.next_key;
        self.terrain.terrains.insert(
            key,
            Terrain {
                name: name.to_owned(),
                provider_team,
                team,
                x_q32,
                z_q32,
                spec,
                elapsed: 0,
                round: 0,
            },
        );
        self.terrain.controllers[index].items.push(key);
        if spec.kind == TerrainKind::Fire {
            self.ignite_oils(key)?;
        }
        Ok(())
    }

    /// `CheckInteractableItems` for a new fire: every oil it reaches, in its
    /// controller's order, goes, and each turns to a fire of its own
    /// provider where it stood, under the new fire's side.
    fn ignite_oils(&mut self, fire: u64) -> Result<()> {
        let Some(index) = self.controller_of(TerrainKind::Oil) else {
            return Ok(());
        };
        let (x_q32, z_q32, radius_q32, team) = {
            let fire = &self.terrain.terrains[&fire];
            (fire.x_q32, fire.z_q32, fire.spec.radius_q32, fire.team)
        };
        let reached = self.terrain.controllers[index]
            .items
            .iter()
            .copied()
            .filter(|key| self.circles_overlap((x_q32, z_q32, radius_q32), *key))
            .collect::<Vec<_>>();
        self.terrain.controllers[index]
            .items
            .retain(|key| !reached.contains(key));
        for oil in reached {
            let oil = &self.terrain.terrains[&oil];
            let (provider, spec, position) = (
                (oil.provider_team, oil.name.clone()),
                oil.spec.burning().expect("an oil burns"),
                (oil.x_q32, oil.z_q32),
            );
            self.add_item((provider.0, &provider.1), team, spec, position)?;
        }
        Ok(())
    }

    /// `RangeItemController.IsInteractable` of the fire controller: whether
    /// any fire reaches a circle.
    fn fire_reaches(&self, (x_q32, z_q32): (i64, i64), radius_q32: i64) -> bool {
        self.controller_of(TerrainKind::Fire).is_some_and(|index| {
            self.terrain.controllers[index]
                .items
                .iter()
                .any(|&key| self.circles_overlap((x_q32, z_q32, radius_q32), key))
        })
    }

    /// `CircleRange.Overlaps`: the centres no further apart than the radii
    /// together, by `FPoint`'s tolerant comparison.
    fn circles_overlap(&self, (x_q32, z_q32, radius_q32): (i64, i64, i64), key: u64) -> bool {
        let terrain = &self.terrain.terrains[&key];
        let distance = native_q32_magnitude(
            terrain.x_q32.saturating_sub(x_q32),
            terrain.z_q32.saturating_sub(z_q32),
        );
        fpoint_less_or_equal(distance, radius_q32.saturating_add(terrain.spec.radius_q32))
    }

    /// The controller of a terrain's kind, made in its place the first time.
    fn controller_for(&mut self, spec: TerrainSpec) -> usize {
        if let Some(index) = self.controller_of(spec.kind) {
            return index;
        }
        let index = self
            .terrain
            .controllers
            .iter()
            .position(|controller| controller_rank(controller.kind) > controller_rank(spec.kind))
            .unwrap_or(self.terrain.controllers.len());
        self.terrain.controllers.insert(
            index,
            TerrainController {
                kind: spec.kind,
                items: Vec::new(),
                affected: Vec::new(),
                period: match spec.effect {
                    TerrainEffect::Fog { .. } => None,
                    TerrainEffect::Fire { period_ticks, .. }
                    | TerrainEffect::Buff { period_ticks, .. } => Some(period_ticks),
                },
            },
        );
        index
    }

    fn controller_of(&self, kind: TerrainKind) -> Option<usize> {
        self.terrain
            .controllers
            .iter()
            .position(|controller| controller.kind == kind)
    }

    /// The tick's terrain events, as a recording finds them at its end: the
    /// terrains its snapshot holds are named, the new ones in the system's
    /// order, and set against the last snapshot's, the made and then the
    /// gone.
    pub(in crate::fight) fn take_terrain_events(&mut self) -> Vec<Event> {
        let holding = self
            .terrain
            .controllers
            .iter()
            .flat_map(|controller| controller.items.iter().copied())
            .collect::<Vec<_>>();
        let mut created = Vec::new();
        for &key in &holding {
            if self.terrain.ids.contains_key(&key) {
                continue;
            }
            self.terrain.next_id += 1;
            let id = self.terrain.next_id;
            self.terrain.ids.insert(key, id);
            let terrain = &self.terrain.terrains[&key];
            created.push(event(
                Some(ObjectRef::new(ObjectKind::Terrain, id)),
                None,
                Some(terrain.team),
                None,
                EventPayload::TerrainCreated {
                    team_id: Some(terrain.team),
                    terrain_type: terrain_type(terrain.spec.kind),
                    position: QVec3 {
                        x: terrain.x_q32,
                        y: 0,
                        z: terrain.z_q32,
                    },
                    radius: terrain.spec.radius_q32,
                },
            ));
        }
        let mut removed = std::mem::take(&mut self.terrain.seen)
            .into_iter()
            .filter(|key| !holding.contains(key))
            .map(|key| (self.terrain.ids[&key], key))
            .collect::<Vec<_>>();
        removed.sort_unstable();
        let removed = removed.into_iter().map(|(id, key)| {
            let terrain = &self.terrain.terrains[&key];
            event(
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
                    reason: self
                        .terrain
                        .reasons
                        .get(&key)
                        .copied()
                        .unwrap_or(TerrainRemovedReason::Unknown),
                },
            )
        });
        let events = created.into_iter().chain(removed).collect();
        self.terrain.seen = holding;
        events
    }

    /// `RangeItemSystem.Update`: each controller holding items, in the
    /// order the system made them.
    pub(in crate::fight) fn step_terrains(&mut self, events: &mut Vec<Event>) -> Result<()> {
        for index in 0..self.terrain.controllers.len() {
            if self.terrain.controllers[index].items.is_empty() {
                continue;
            }
            self.forget_the_dead(index);
            self.update_item_status(index);
            self.update_affected(index, events)?;
            self.update_periodic(index, events)?;
        }
        Ok(())
    }

    /// The periodic effects, `effectTimeDuration` apart: every affected unit,
    /// last first, counts one, and one whose count reaches the period counts
    /// it back off and takes the effect again. A unit the effect kills leaves.
    fn update_periodic(&mut self, index: usize, events: &mut Vec<Event>) -> Result<()> {
        let Some(period) = self.terrain.controllers[index]
            .period
            .filter(|&period| period > 0)
        else {
            return Ok(());
        };
        let mut position = self.terrain.controllers[index].affected.len();
        while position > 0 {
            position -= 1;
            let Some(affected) = self.terrain.controllers[index].affected.get_mut(position) else {
                continue;
            };
            affected.time += 1;
            if affected.time < period {
                continue;
            }
            affected.time -= period;
            let (unit, item) = (affected.unit, affected.terrain);
            self.perform_terrain_effect(item, unit, events)?;
            self.forget_the_dead(index);
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
    fn update_item_status(&mut self, index: usize) {
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
                self.remove_terrain(index, id, TerrainRemovedReason::TimeExpired);
            }
        }
    }

    /// `RangeItemController.Remove`: the terrain leaves its controller,
    /// which still holds any unit that stood in it until it next updates.
    fn remove_terrain(&mut self, index: usize, key: u64, reason: TerrainRemovedReason) {
        self.terrain.controllers[index]
            .items
            .retain(|&item| item != key);
        self.terrain.reasons.insert(key, reason);
    }

    /// `UpdateAffectedActorChange`. Side by side, each item against every
    /// unit of the side found by querying its units' tree with the item
    /// tree's node: a ground unit valid as a target whose edge the item's
    /// range reaches, in two dimensions. Every affected unit not found
    /// leaves, last first; every unit found that is not affected enters the
    /// first item it was found in, and one already affected keeps its own.
    fn update_affected(&mut self, index: usize, events: &mut Vec<Event>) -> Result<()> {
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
            self.perform_terrain_effect(item, unit, events)?;
            self.forget_the_dead(index);
        }
        Ok(())
    }

    /// The controller's `PerformItemEffect` on a unit standing in the item.
    fn perform_terrain_effect(
        &mut self,
        item: u64,
        unit: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let team = self.terrain.terrains[&item].team;
        match self.terrain.terrains[&item].spec.effect {
            // `BuffItemController.PerformItemEffect`: the buff, written by no
            // object under the terrain's side, `BuffSystem.AddBuff`.
            TerrainEffect::Buff { buff, .. } => {
                let name = self.terrain.terrains[&item].name.clone();
                self.write_skill_buff((&name, team), &buff, &[unit], events)
            }
            // `GroundFireController.PerformItemEffect`: a hit of the fire's
            // damage through `PerformHitTargetEffect`, with no owner, under
            // the fire's side.
            TerrainEffect::Fire { damage, .. } => {
                self.hit_with_no_object(FightActorRef::Unit(unit), team, (damage, true), events)
            }
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

    /// `RangeItemController.OnExitFight`: every unit leaves, taking back
    /// what the terrain did to it, and every terrain counts a round and goes,
    /// `round_expired`, once it has stood its rounds, or at once for a fire,
    /// whose controller ignores them (`IsIgnoreRoundDuration`).
    pub(in crate::fight) fn clear_terrains_as_the_fight_ends(&mut self) -> Result<()> {
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
                let terrain = self
                    .terrain
                    .terrains
                    .get_mut(&id)
                    .expect("an item's terrain exists");
                terrain.round += 1;
                if terrain.spec.kind == TerrainKind::Fire || terrain.round >= terrain.spec.rounds {
                    self.remove_terrain(index, id, TerrainRemovedReason::RoundExpired);
                }
            }
        }
        Ok(())
    }

    /// Every terrain as a snapshot holds it, with the living units it
    /// affects.
    pub(in crate::fight) fn terrain_states(&self) -> Vec<TerrainState> {
        let mut states = Vec::new();
        for controller in &self.terrain.controllers {
            for &key in &controller.items {
                let terrain = &self.terrain.terrains[&key];
                let mut applications = controller
                    .affected
                    .iter()
                    .filter(|affected| {
                        affected.terrain == key && self.actors[&affected.unit].alive()
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
                    terrain_id: self.terrain.ids.get(&key).copied().unwrap_or_default(),
                    team_id: Some(terrain.team),
                    terrain_type: terrain_type(terrain.spec.kind),
                    position: QVec3 {
                        x: terrain.x_q32,
                        y: 0,
                        z: terrain.z_q32,
                    },
                    radius: terrain.spec.radius_q32,
                    grid: None,
                    // `GetDuration` less `Round`, read only of a terrain that
                    // stands more than one round.
                    remaining_rounds: (terrain.spec.rounds > 1)
                        .then(|| u32::try_from(terrain.spec.rounds - terrain.round).unwrap_or(0)),
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
