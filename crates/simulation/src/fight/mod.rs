// Paired fixed-point components are named `x_q32` / `z_q32` throughout, which
// `similar_names` flags on every coordinate pair.
#![allow(clippy::similar_names)]
// The files under `fight/` are one module's code split by what it mirrors in
// the build — search, motion, damage, the skill — and share its names the way
// one file would, so each opens with `use super::*`.
#![allow(clippy::wildcard_imports)]

use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Instant,
};

use mechcore_mcfr::{
    BuffModifierSet, BuildingState, DerivedStats, Domain, DurableContext, Event, EventPayload,
    GaugeI32, Hashes, IdentityAllocator, LiveUnitState, McfrReader, McfrWriter, MotionState,
    ObjectKind, ObjectRef, PersonalShieldState, ProjectileState, QVec3, Rational, TickSlice,
    TransitionEvents, UnitDynamicModifierSet, Visibility, WeaponAimState, WorldSnapshot,
};

use serde::Serialize;

use crate::{
    Error, Result,
    layout::{CompiledLayout, ConstructionBuilding, Placement},
    rules::{
        AttackConfig, AttackPath, AttackTargets, RvoSize, SimulationConfig, TrainingGroundConfig,
        UnitConfig, UnitConfigs, UnitDomain, WeaponMode,
    },
};

mod attacker;
mod construction;
mod damage;
mod deploy;
mod math;
mod mech;
mod motion;
mod projectile;
mod random;
mod run;
mod rvo;
mod search;
mod skill;
#[cfg(test)]
mod tests;

#[cfg(test)]
use attacker::Facing;
use construction::*;
use damage::*;
use deploy::*;
pub(crate) use math::*;
use motion::*;
use projectile::*;
use random::GrRandom;
pub(crate) use run::*;
pub use run::{DivergentTick, SimulationComparison, SimulationResult, TimelineSummary};
use rvo::{AgentInput as RvoAgentInput, AgentKey as RvoAgentKey, AgentSizeType, FixedVec2};
use search::*;
use skill::{FightSkillPhase, Launch, Skill};

const SPACE_UNITS_PER_METER: i64 = 1_000;

const AIR_UNIT_HEIGHT: i64 = 70_000;

const TIME_UNITS_PER_SECOND: u64 = 2_000;

const LOGIC_TICK_TIME_UNITS: u64 = 100;

const FIGHT_TIME_SECONDS: u64 = 120;

const FORMATION_JITTER_RANGE_TENTHS: i32 = 8;

pub(crate) const Q32_ONE: i64 = 1_i64 << 32;

const C0_1_RAW: i64 = 0x1999_9999;

const C0_01_RAW: i64 = 0x028F_5C28;

const NATIVE_LOGIC_DELTA_Q32: i64 = 0x0CCC_CCCC;

const TARGET_SCORE_MIN_DISTANCE_Q32: i64 = 3_i64 << 32;

const TARGET_SCORE_ANGLE_LIMIT_Q32: i64 = 100_i64 << 32;

const TARGET_SCORE_ANGLE_FACTOR_Q32: i64 = 0x9999_9999;

const TARGET_SCORE_BASE_Q32: i64 = 100_i64 << 32;

const TARGET_SCORE_OUT_OF_RANGE_PENALTY_Q32: i64 = 200_000_i64 << 32;

const TARGET_QUADTREE_MAX_DEPTH: u8 = 6;

const TARGET_QUADTREE_MAX_ELEMENTS: usize = 20;

const TARGET_QUADTREE_HALF_WIDTH_Q32: i64 = 400 * Q32_ONE;

const TARGET_QUADTREE_HALF_HEIGHT_Q32: i64 = 350 * Q32_ONE;

const RVO_SIMULATOR_ORIGIN_OFFSET_Q32: i64 = 400 * Q32_ONE;

const TOWER_RVO_COLLIDER_PRIORITY: i32 = 10;

const SEARCH_TARGET_RESET_TICKS: i32 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum FightActorRef {
    Unit(u64),
    Building(u64),
}

impl FightActorRef {
    const fn id(self) -> u64 {
        match self {
            Self::Unit(id) | Self::Building(id) => id,
        }
    }

    const fn kind(self) -> ObjectKind {
        match self {
            Self::Unit(_) => ObjectKind::Unit,
            Self::Building(_) => ObjectKind::Building,
        }
    }

    const fn object_ref(self) -> ObjectRef {
        ObjectRef::new(self.kind(), self.id())
    }

    const fn unit_id(self) -> Option<u64> {
        match self {
            Self::Unit(id) => Some(id),
            Self::Building(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct FightActorView {
    team: u32,
    x_q32: i64,
    z_q32: i64,
    query_x_q32: i64,
    query_z_q32: i64,
    radius: i64,
    alive: bool,
    query_alive: bool,
    targetable: bool,
    domain: UnitDomain,
}

#[derive(Debug, Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the actor mirrors the native fight actor's independent state flags"
)]
struct Actor {
    placement: Placement,
    rules: UnitConfig,
    /// The numbers the fight reads, which are the description corrected by the
    /// overlays. `crate::data` is the layer; nothing here reads `rules` for a
    /// number this carries.
    stats: crate::data::Stats,
    x: i64,
    z: i64,
    x_q32: i64,
    z_q32: i64,
    target_query_x_q32: i64,
    target_query_z_q32: i64,
    target_query_source_rotation_q32: i64,
    target_query_alive: bool,
    body_rotation: i64,
    body_rotation_q32: i64,
    aim_rotation: i64,
    life: i64,
    last_damage_source: Option<(ObjectRef, u32)>,
    pub(in crate::fight) motion: Motion,
    pub(in crate::fight) skill: Skill,
}

/// `GameRiver.BuildingType.Special`.
const CONSTRUCTION_BUILDING_TYPE: u32 = 3;

struct Simulation {
    actors: BTreeMap<u64, Actor>,
    team_random: BTreeMap<u32, GrRandom>,
    projectiles: Vec<Projectile>,
    buildings: Vec<BuildingState>,
    target_quadtrees: BTreeMap<u32, TargetActorQuadtree>,
    identities: IdentityAllocator,
    rvo_counter: u8,
    // Native Agent::.ctor stores its initial position in the public backing
    // buffer. The internal position read by the first BuildQuadtree remains
    // zero until the subsequent BufferSwitch.
    rvo_first_tree_pending: bool,
    terminal_drain_pending: bool,
    late_building_events_pending: bool,
    /// Buildings a projectile destroyed this tick, held until every projectile
    /// has resolved so their events follow all of the tick's shots.
    fallen_buildings: Vec<Event>,
    /// The RVO collider layer of every construction, by building.
    ///
    /// A construction is an obstacle only to the other side: the wall's own
    /// description says it sinks into the ground for a friendly unit, and a
    /// Crawler of the side that placed it walks through a block. To the other
    /// side it is an immovable agent on its `pathfinding_collider_priority`
    /// layer with its own box for a radius — a Steel Ball of `wall-laser.yaml`
    /// overlapping block 3 is pushed off it as that agent pushes it, tick for
    /// tick, and every other wall fight is unchanged by it.
    construction_colliders: BTreeMap<u64, i32>,
    /// The buildings no unit searches for, which the target trees hold all
    /// the same.
    unsearchable_buildings: BTreeSet<u64>,
    /// The constructions whose skill fires, by building.
    constructions: BTreeMap<u64, Construction>,
}

impl Simulation {
    #[cfg(test)]
    fn new(
        layout: &CompiledLayout,
        configs: &UnitConfigs,
        training_ground: &TrainingGroundConfig,
        seed: i32,
    ) -> Result<Self> {
        let mut simulation = Self::new_unprepared(layout, configs, training_ground, seed)?;
        simulation.initialize_presearch_targets()?;
        Ok(simulation)
    }

    fn new_unprepared(
        layout: &CompiledLayout,
        configs: &UnitConfigs,
        training_ground: &TrainingGroundConfig,
        seed: i32,
    ) -> Result<Self> {
        for placement in &layout.placements {
            configs
                .get(&placement.type_name)
                .ok_or_else(|| {
                    Error::new(format!(
                        "unit type {:?} has no configuration",
                        placement.type_name
                    ))
                })?
                .ensure_current_kernel_support()?;
        }
        let actors = initialize_actors(layout, configs, seed)?;
        let InitialBuildings {
            states: buildings,
            unsearchable,
            colliders: construction_colliders,
        } = initialize_buildings(training_ground, &layout.constructions)?;
        let constructions = initialize_constructions(&buildings, &layout.constructions)?;
        let target_quadtrees = initialize_target_quadtrees(&actors, &buildings);
        let mut simulation = Self {
            actors,
            team_random: BTreeMap::new(),
            projectiles: Vec::new(),
            buildings,
            target_quadtrees,
            identities: IdentityAllocator::new(),
            rvo_counter: 0,
            rvo_first_tree_pending: true,
            terminal_drain_pending: false,
            late_building_events_pending: false,
            fallen_buildings: Vec::new(),
            construction_colliders: construction_colliders.clone(),
            unsearchable_buildings: unsearchable.clone(),
            constructions,
        };
        simulation.deploy_attack_intervals(layout.round)?;
        Ok(simulation)
    }

    fn snapshot(&self) -> WorldSnapshot {
        WorldSnapshot {
            live_units: self
                .actors
                .values()
                .filter(|actor| actor.alive())
                .map(Actor::snapshot)
                .collect(),
            projectiles: self.projectiles.iter().map(Projectile::snapshot).collect(),
            buildings: self
                .buildings
                .iter()
                .filter(|building| building_alive(building))
                .cloned()
                .collect(),
            ..WorldSnapshot::default()
        }
    }

    /// Reads every unit with no enemy left at its interval as composed, with
    /// no stagger.
    ///
    /// The stagger rides on the cycle in progress, and once a unit's last
    /// enemy is dead there is none: from the tick after, and on the fight's
    /// final tick if that is the one, the game reads a Marksman at 62, an
    /// Arclight at 18 and a Wraith at 32 — their descriptions — whatever
    /// cycle they were on.
    fn settle_intervals(&mut self) {
        let alive_teams = self
            .actors
            .values()
            .filter(|actor| actor.alive())
            .map(|actor| actor.placement.team)
            .collect::<BTreeSet<_>>();
        for actor in self.actors.values_mut().filter(|actor| actor.alive()) {
            let team = actor.placement.team;
            if alive_teams.iter().all(|&other| other == team) {
                actor.skill.current_attack_interval =
                    native_time_units_to_steps(actor.stats.attack_interval());
            }
        }
    }

    /// The same, on the fight's last tick, before its snapshot is written.
    fn settle_intervals_if_finishing(&mut self) {
        if self.ready_to_finish() {
            self.settle_intervals();
        }
    }

    #[allow(clippy::too_many_lines)]
    fn step(&mut self, step: u64) -> Result<TransitionEvents> {
        self.settle_intervals();
        let publish_late_building_events = self.late_building_events_pending;
        self.late_building_events_pending = false;
        if self.terminal_drain_pending {
            self.terminal_drain_pending = false;
        }
        let fight_was_finished = self.naturally_finished();
        let winner_was_decided = self.winner().is_some();
        let actor_motion_at_start = self
            .actors
            .iter()
            .map(|(&actor_id, actor)| (actor_id, actor.motion.state))
            .collect::<BTreeMap<_, _>>();
        let teams_with_building_target_at_start = self
            .actors
            .values()
            .filter_map(|actor| {
                matches!(
                    actor.skill.attack_target(),
                    Some(FightActorRef::Building(_))
                )
                .then_some(actor.placement.team)
            })
            .collect::<std::collections::BTreeSet<_>>();
        self.refresh_target_query_snapshot();
        let mut events = Vec::new();
        if publish_late_building_events && let Some(winning_team) = self.winner() {
            for building in self
                .buildings
                .iter()
                .filter(|building| building.team_id != winning_team && !building_alive(building))
            {
                events.push(event(
                    Some(ObjectRef::new(ObjectKind::Building, building.building_id)),
                    None,
                    None,
                    None,
                    EventPayload::BuildingDestroyed {
                        position: building.position,
                    },
                ));
            }
        }
        // Native search jobs retain the actor-quadtree candidate order
        // prepared at the start of this FightCore update.
        let target_search_order = self.target_search_order();
        let team_ids = self
            .actors
            .values()
            .map(|actor| actor.placement.team)
            .collect::<std::collections::BTreeSet<_>>();
        let mut team_alive_counts = BTreeMap::new();
        for team_id in team_ids {
            team_alive_counts.insert(
                team_id,
                self.actors
                    .values()
                    .filter(|actor| actor.placement.team == team_id && actor.alive())
                    .count(),
            );
            let actor_ids = self
                .actors
                .iter()
                .filter_map(|(&actor_id, actor)| {
                    (actor.placement.team == team_id).then_some(actor_id)
                })
                .collect::<Vec<_>>();
            for actor_id in actor_ids {
                self.step_actor_with_target_order(
                    actor_id,
                    step,
                    &target_search_order,
                    &mut events,
                )?;
                self.step_actor_rvo_position(actor_id);
            }
            let building_ids = self
                .constructions
                .iter()
                .filter_map(|(&building_id, construction)| {
                    (construction.team == team_id).then_some(building_id)
                })
                .collect::<Vec<_>>();
            for building_id in building_ids {
                self.step_construction(building_id, step, &target_search_order, &mut events)?;
            }
        }
        let naturally_finished_before_projectiles = self.naturally_finished();
        let actors_alive_before_projectiles = self
            .actors
            .iter()
            .filter_map(|(&actor_id, actor)| actor.alive().then_some(actor_id))
            .collect::<Vec<_>>();
        self.step_projectiles(&mut events)?;
        if let Some(winning_team) = self.winner() {
            let winning_actors_killed_by_projectiles = actors_alive_before_projectiles
                .into_iter()
                .filter(|actor_id| {
                    let actor = &self.actors[actor_id];
                    !actor.alive() && actor.placement.team == winning_team
                })
                .collect::<Vec<_>>();
            for actor_id in winning_actors_killed_by_projectiles {
                let actor = &self.actors[&actor_id];
                if actor_motion_at_start.get(&actor_id) != Some(&MotionState::Moving)
                    || !actor.rules.has_body
                    || !matches!(actor.rules.attack.path, AttackPath::Projectile { .. })
                {
                    continue;
                }
                let Some(enemy_team) = self
                    .buildings
                    .iter()
                    .find(|building| {
                        building.team_id != actor.placement.team && building_alive(building)
                    })
                    .map(|building| building.team_id)
                else {
                    continue;
                };
                let Some(building_id) = self.select_normal_building_target(actor_id, enemy_team)
                else {
                    continue;
                };
                let building = self
                    .buildings
                    .iter()
                    .find(|building| building.building_id == building_id)
                    .expect("selected building exists");
                let target_x_q32 = building.position.x;
                let target_z_q32 = building.position.z;
                let actor = self
                    .actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable");
                actor.rotate_weapons_towards(direction_degrees_q32_raw(
                    target_x_q32.saturating_sub(actor.x_q32),
                    target_z_q32.saturating_sub(actor.z_q32),
                ));
                actor.aim_rotation = degrees_q32_to_mdeg(
                    actor
                        .skill
                        .weapon_rotations_q32
                        .first()
                        .copied()
                        .unwrap_or(actor.body_rotation_q32),
                );
            }
        }
        let projectile_finished_fight =
            !naturally_finished_before_projectiles && self.naturally_finished();
        let natural_finish_handoff = !fight_was_finished && self.naturally_finished();
        let early_projectile_handoff = !winner_was_decided && !self.naturally_finished();
        if (natural_finish_handoff || early_projectile_handoff)
            && let Some(winning_team) = self.winner()
            && let Some(losing_team) = self
                .buildings
                .iter()
                .find(|building| building.team_id != winning_team && building_alive(building))
                .map(|building| building.team_id)
            && !teams_with_building_target_at_start.contains(&losing_team)
        {
            let mut queued_direct_own_kill_handoff = false;
            let actor_ids = self
                .actors
                .iter()
                .filter_map(|(&actor_id, actor)| actor.alive().then_some(actor_id))
                .collect::<Vec<_>>();
            for actor_id in actor_ids {
                let Some(&motion_at_start) = actor_motion_at_start.get(&actor_id) else {
                    continue;
                };
                let actor = &self.actors[&actor_id];
                let team_has_bodyful_projectile = self.actors.values().any(|candidate| {
                    candidate.placement.team == actor.placement.team
                        && candidate.alive()
                        && candidate.rules.has_body
                        && matches!(candidate.rules.attack.path, AttackPath::Projectile { .. })
                });
                let moving_direct = motion_at_start == MotionState::Moving
                    && matches!(actor.rules.attack.path, AttackPath::Direct { .. })
                    && team_has_bodyful_projectile
                    && (actor.motion.current_velocity_x_q32 != 0
                        || actor.motion.current_velocity_z_q32 != 0)
                    && actor.skill.pending().is_none()
                    && actor.skill.backswing_finish_step().is_none()
                    && actor.skill.phase() == FightSkillPhase::Idle
                    && !actor.motion.attack_hold_fire
                    && actor.skill.searched_this_tick
                    && actor.skill.attack_target().is_none();
                let moving_bodyful_projectile = motion_at_start == MotionState::Moving
                    && actor.rules.has_body
                    && matches!(actor.rules.attack.path, AttackPath::Projectile { .. })
                    && actor.skill.searched_this_tick
                    && actor.skill.attack_target().is_none();
                let ineligible = if natural_finish_handoff {
                    !moving_direct
                        && (actor.skill.attack_target().is_some()
                            || actor.skill.retarget_after_own_direct_kill
                            || !actor.skill.searched_this_tick)
                } else {
                    (!moving_direct && !moving_bodyful_projectile)
                        || actor
                            .skill
                            .attack_target()
                            .is_some_and(|target| self.fight_actor_is_alive(target))
                };
                if ineligible {
                    continue;
                }
                let Some(building_id) = self.select_normal_building_target(actor_id, losing_team)
                else {
                    continue;
                };
                let building = self
                    .buildings
                    .iter()
                    .find(|building| building.building_id == building_id)
                    .expect("selected building exists");
                let target_x_q32 = building.position.x;
                let target_z_q32 = building.position.z;
                let target_radius = building_radius(building);
                let actor = self
                    .actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable");
                if natural_finish_handoff {
                    if motion_at_start == MotionState::Moving
                        && (actor.motion.current_velocity_x_q32 != 0
                            || actor.motion.current_velocity_z_q32 != 0)
                    {
                        actor.rotate_body_towards(direction_degrees_q32_raw(
                            actor.motion.current_velocity_x_q32,
                            actor.motion.current_velocity_z_q32,
                        ));
                        actor.aim_rotation = actor.body_rotation;
                    }
                } else {
                    if actor.motion.current_velocity_x_q32 != 0
                        || actor.motion.current_velocity_z_q32 != 0
                    {
                        actor.rotate_body_towards(direction_degrees_q32_raw(
                            actor.motion.current_velocity_x_q32,
                            actor.motion.current_velocity_z_q32,
                        ));
                    }
                    if moving_direct {
                        actor.aim_rotation = actor.body_rotation;
                    } else {
                        actor.rotate_weapons_towards(direction_degrees_q32_raw(
                            target_x_q32.saturating_sub(actor.x_q32),
                            target_z_q32.saturating_sub(actor.z_q32),
                        ));
                        actor.aim_rotation = degrees_q32_to_mdeg(
                            actor
                                .skill
                                .weapon_rotations_q32
                                .first()
                                .copied()
                                .unwrap_or(actor.body_rotation_q32),
                        );
                    }
                }
                actor.skill.lock_target = Some(FightActorRef::Building(building_id));
                actor.skill.lock_is_terminal_handoff = true;
                actor.motion.state = MotionState::Moving;
                if natural_finish_handoff {
                    actor.motion.next_target_x_q32 = target_x_q32;
                    actor.motion.next_target_z_q32 = target_z_q32;
                    actor.motion.next_speed_q32 = space_to_q32(actor.stats.move_speed());
                } else {
                    let (move_target_x_q32, move_target_z_q32) = native_auto_move_target_point(
                        actor.x_q32,
                        actor.z_q32,
                        actor.rules.collision_radius(),
                        target_x_q32,
                        target_z_q32,
                        target_radius,
                        actor.stats.attack_range(),
                    );
                    actor.motion.next_target_x_q32 = move_target_x_q32;
                    actor.motion.next_target_z_q32 = move_target_z_q32;
                    actor.motion.next_speed_q32 = turn_limited_move_speed_q32(
                        space_to_q32(actor.stats.move_speed()),
                        actor.rules.rotate_speed_mdeg_per_second(),
                        actor.body_rotation_q32,
                        actor.motion.current_velocity_x_q32,
                        actor.motion.current_velocity_z_q32,
                    );
                }
                actor.motion.next_max_speed_q32 = actor.motion.next_speed_q32;
                queued_direct_own_kill_handoff |= natural_finish_handoff && moving_direct;
            }
            if queued_direct_own_kill_handoff {
                self.terminal_drain_pending = true;
            }
        }
        // Native build 2259 tears down the defeated towers through the
        // direct-attack finish path. Projectile drain reaches the same round
        // result without mutating buildings (observed in Fang mirror battles).
        let direct_attack_winner = !fight_was_finished
            && !winner_was_decided
            && !projectile_finished_fight
            && self.winner().is_some_and(|winning_team| {
                self.actors.values().any(|actor| {
                    actor.placement.team == winning_team
                        && actor.alive()
                        && matches!(actor.rules.attack.path, AttackPath::Direct { .. })
                })
            });
        let mut queued_late_building_events = false;
        for building in self.buildings.iter_mut().filter(|building| {
            direct_attack_winner
                && building_alive(building)
                && team_alive_counts
                    .get(&building.team_id)
                    .is_some_and(|alive_count| *alive_count == 0)
        }) {
            building.life.current = 0;
            building.targetable = false;
            queued_late_building_events = true;
        }
        if queued_late_building_events {
            self.terminal_drain_pending = true;
            self.late_building_events_pending = true;
        }
        let laser_finish_observed_at_defeated_team_entry = !fight_was_finished
            && self.naturally_finished()
            && self.winner().is_some_and(|winning_team| {
                self.actors.values().any(|actor| {
                    actor.placement.team == winning_team
                        && actor.alive()
                        && matches!(actor.rules.attack.path, AttackPath::Laser { .. })
                })
            })
            && team_alive_counts
                .iter()
                .any(|(&team_id, &alive_count)| alive_count == 0 && Some(team_id) != self.winner());
        if laser_finish_observed_at_defeated_team_entry {
            // FightCore updates blue before red. If an earlier team eliminates
            // a later team, the native finish callback is queued only after
            // that defeated team's module observes its empty actor set. Its
            // attacker therefore exposes the dead laser target for one tick
            // while the callback tears down that team's towers.
            for building in self.buildings.iter_mut().filter(|building| {
                building_alive(building)
                    && team_alive_counts
                        .get(&building.team_id)
                        .is_some_and(|alive_count| *alive_count == 0)
            }) {
                events.push(event(
                    Some(ObjectRef::new(ObjectKind::Building, building.building_id)),
                    None,
                    None,
                    None,
                    EventPayload::BuildingDestroyed {
                        position: building.position,
                    },
                ));
                building.life.current = 0;
                building.targetable = false;
            }
            self.terminal_drain_pending = true;
        }
        let ready_to_finish = self.ready_to_finish();
        let stop_fight = ready_to_finish || winner_was_decided;
        if stop_fight {
            for actor in self.actors.values_mut() {
                actor.motion.state = MotionState::Idle;
                actor.skill.drop_lock();
                // `FightSkill.ExitFight` ends a cooling as well.
                actor.skill.set_cooling(None);
                actor.skill.lock_is_terminal_handoff = false;
                actor.skill.set_phase(FightSkillPhase::Idle);
                if ready_to_finish {
                    actor.motion.current_velocity_x_q32 = 0;
                    actor.motion.current_velocity_z_q32 = 0;
                }
            }
            for construction in self.constructions.values_mut() {
                construction.skill.drop_lock();
                construction.skill.set_phase(FightSkillPhase::Idle);
            }
        }
        if !ready_to_finish {
            self.step_rvo();
        }
        for death in events
            .iter_mut()
            .filter(|event| matches!(&event.payload, EventPayload::UnitDied { .. }))
        {
            let Some(dead_id) = death
                .subject
                .filter(|subject| subject.kind == ObjectKind::Unit)
                .map(|subject| subject.id)
            else {
                continue;
            };
            (death.source, death.source_team_id) = self.actors[&dead_id]
                .last_damage_source
                .map_or((None, None), |(source, team_id)| {
                    (Some(source), Some(team_id))
                });
        }
        let (mut events, deaths): (Vec<_>, Vec<_>) = events
            .into_iter()
            .partition(|event| !matches!(&event.payload, EventPayload::UnitDied { .. }));
        events.extend(deaths);
        // What the tick's hits killed and felled comes last, in the order they
        // struck: a block a shot fells reads between the deaths its splash
        // caused, and after every removal the tick resolved.
        events.append(&mut self.fallen_buildings);
        // A unit that died leaves its team's target tree, which keeps the
        // rest in their order but changes when a node next splits: a splash
        // that kills a crowd reads the next crowd in the order the game does
        // only with the dead taken out.
        let dead = self
            .actors
            .iter()
            .filter(|(_, actor)| !actor.alive())
            .map(|(&actor_id, actor)| (actor.placement.team, actor_id))
            .collect::<Vec<_>>();
        for (team, actor_id) in dead {
            if let Some(tree) = self.target_quadtrees.get_mut(&team) {
                tree.remove(FightActorRef::Unit(actor_id));
            }
        }
        Ok(TransitionEvents { events })
    }

    fn fight_actor(&self, reference: FightActorRef) -> Option<FightActorView> {
        match reference {
            FightActorRef::Unit(id) => {
                let actor = self.actors.get(&id)?;
                Some(FightActorView {
                    team: actor.placement.team,
                    x_q32: actor.x_q32,
                    z_q32: actor.z_q32,
                    query_x_q32: actor.target_query_x_q32,
                    query_z_q32: actor.target_query_z_q32,
                    radius: actor.rules.collision_radius(),
                    alive: actor.alive(),
                    query_alive: actor.target_query_alive,
                    targetable: actor.alive(),
                    domain: actor.rules.domain,
                })
            }
            FightActorRef::Building(id) => {
                let building = self
                    .buildings
                    .iter()
                    .find(|building| building.building_id == id)?;
                let x_q32 = building.position.x;
                let z_q32 = building.position.z;
                Some(FightActorView {
                    team: building.team_id,
                    x_q32,
                    z_q32,
                    query_x_q32: x_q32,
                    query_z_q32: z_q32,
                    radius: building_radius(building),
                    alive: building_alive(building),
                    query_alive: building_alive(building),
                    targetable: building.targetable && building.available,
                    domain: UnitDomain::Ground,
                })
            }
        }
    }

    fn fight_actor_is_alive(&self, reference: FightActorRef) -> bool {
        self.fight_actor(reference)
            .is_some_and(|target| target.alive && target.targetable)
    }

    fn winner(&self) -> Option<u32> {
        let blue = self
            .actors
            .values()
            .any(|actor| actor.placement.team == 0 && actor.alive());
        let red = self
            .actors
            .values()
            .any(|actor| actor.placement.team == 1 && actor.alive());
        match (blue, red) {
            (true, false) => Some(0),
            (false, true) => Some(1),
            _ => None,
        }
    }

    fn naturally_finished(&self) -> bool {
        let living_teams = self
            .actors
            .values()
            .filter(|actor| actor.alive())
            .map(|actor| actor.placement.team)
            .collect::<std::collections::BTreeSet<_>>();
        living_teams.len() < 2 && self.projectiles.is_empty()
    }

    fn ready_to_finish(&self) -> bool {
        self.naturally_finished() && !self.terminal_drain_pending
    }
}

fn team_name(team: u32) -> &'static str {
    if team == 0 { "blue" } else { "red" }
}

fn event(
    subject: Option<ObjectRef>,
    source: Option<ObjectRef>,
    source_team_id: Option<u32>,
    target: Option<ObjectRef>,
    payload: EventPayload,
) -> Event {
    Event {
        subject,
        source,
        source_team_id,
        target,
        payload,
    }
}

fn point(x: i64, z: i64) -> QVec3 {
    QVec3 {
        x: space_to_q32(x),
        y: 0,
        z: space_to_q32(z),
    }
}

const fn building_alive(building: &BuildingState) -> bool {
    building.life.current > 0
}

fn building_radius(building: &BuildingState) -> i64 {
    q32_to_space_rounded(building.bounds_width / 2)
}

fn building_x(building: &BuildingState) -> i64 {
    q32_to_space_rounded(building.position.x)
}

fn building_z(building: &BuildingState) -> i64 {
    q32_to_space_rounded(building.position.z)
}

const fn unit_height(domain: UnitDomain) -> i64 {
    match domain {
        UnitDomain::Ground => 0,
        UnitDomain::Air => AIR_UNIT_HEIGHT,
    }
}
