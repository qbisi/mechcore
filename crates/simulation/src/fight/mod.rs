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
    BuildingState, DamageStatistics, DerivedStats, Domain, DurableContext, Event, EventPayload,
    FormationState, GaugeI32, Hashes, IdentityAllocator, LiveUnitState, McfrReader, McfrWriter,
    MotionState, ObjectKind, ObjectRef, PersonalShieldState, ProjectileState, QPlanar, QPose,
    QVec3, Rational, RecorderKind, TickSlice, TransitionEvents, Visibility, WeaponAimState,
    WorldSnapshot,
};

use serde::Serialize;

use crate::{
    Error, Result,
    layout::{CompiledLayout, ConstructionBuilding, Placement},
    rules::{
        AttackConfig, AttackPath, AttackTargets, Magazine, RvoSize, SimulationConfig, TowersConfig,
        UnitConfig, UnitConfigs, UnitDomain, WeaponMode,
    },
};

mod attacker;
mod construction;
mod damage;
mod deploy;
mod experience;
mod math;
mod mech;
mod motion;
mod projectile;
mod random;
mod run;
mod rvo;
mod search;
mod skill;
mod statistics;
#[cfg(test)]
mod tests;
mod tower;

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
#[cfg(test)]
use skill::ATTACK_COUNT_RESET;
use skill::{FightSkillPhase, Launch, Performer, Skill, SkillUpdate};
use tower::{RunningBuff, TowerLoss};

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
    /// The buffs running on it, `BuffManager`'s list.
    buffs: Vec<RunningBuff>,
    /// `RVOControllerFixed._maxSpeed`: the speed `Active` read when the unit
    /// took the field, which `StopMove` hands the agent as its maximum. A
    /// buff that changes the unit's speed later reaches a moving agent through
    /// `Move`, and never this.
    rvo_max_speed_q32: i64,
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
    /// Each side's units alone, `FightTeam.mechQuadtree`, which the
    /// experience a kill shares out is looked for in.
    mech_quadtrees: BTreeMap<u32, TargetActorQuadtree>,
    identities: IdentityAllocator,
    rvo_counter: u8,
    // Native Agent::.ctor stores its initial position in the public backing
    // buffer. The internal position read by the first BuildQuadtree remains
    // zero until the subsequent BufferSwitch.
    rvo_first_tree_pending: bool,
    terminal_drain_pending: bool,
    /// The step the fight stops on: a side had already won when it began,
    /// and no skill updates on it.
    stop_step: Option<u64>,
    late_building_events_pending: bool,
    /// Buildings a projectile destroyed this tick, held until every projectile
    /// has resolved so their events follow all of the tick's shots.
    fallen_buildings: Vec<Event>,
    /// The `buff_applied` events a tower's loss wrote this tick, by tower, to
    /// follow its `building_destroyed`.
    tower_buff_events: BTreeMap<u64, Vec<Event>>,
    /// The buffs `BuffManager.Update` dropped from a unit dead this tick, to
    /// name in the `cleared` that follows its `unit_died`.
    dropped_buffs: BTreeMap<u64, Vec<u32>>,
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
    /// The tower table: what a strengthen level adds, what a loss writes.
    towers: TowersConfig,
    /// What each tower's fall writes, by building.
    tower_losses: BTreeMap<u64, TowerLoss>,
    /// The constructions a tower's loss would reach, by building.
    tower_buffed_constructions: BTreeSet<u64>,
    /// The build's damage and kill counters, by recorder.
    statistics: BTreeMap<statistics::RecorderKey, DamageStatistics>,
    /// Each construction block's recorder, by building.
    construction_recorders: BTreeMap<u64, statistics::RecorderKey>,
    /// Each formation's experience, by formation.
    formations: BTreeMap<u64, experience::FormationExperience>,
    /// Who has hit each target, first hit first.
    attackers: BTreeMap<ObjectRef, Vec<ObjectRef>>,
    /// What each building's destruction hands out, by building.
    building_exp: BTreeMap<u64, i64>,
    /// The bars and loot of every unit type.
    experience: experience::ExperienceTable,
}

impl Simulation {
    #[cfg(test)]
    fn new(
        layout: &CompiledLayout,
        configs: &UnitConfigs,
        towers: &TowersConfig,
        seed: i32,
    ) -> Result<Self> {
        let mut simulation = Self::new_unprepared(layout, configs, towers, seed)?;
        simulation.initialize_presearch_targets()?;
        Ok(simulation)
    }

    fn new_unprepared(
        layout: &CompiledLayout,
        configs: &UnitConfigs,
        towers: &TowersConfig,
        seed: i32,
    ) -> Result<Self> {
        for placement in &layout.placements {
            configs.get(&placement.type_name).ok_or_else(|| {
                Error::new(format!(
                    "unit type {:?} has no configuration",
                    placement.type_name
                ))
            })?;
        }
        let actors = initialize_actors(layout, configs, seed)?;
        let InitialBuildings {
            states: buildings,
            unsearchable,
            colliders: construction_colliders,
            tower_losses,
            tower_buffed_constructions,
            construction_groups,
            building_exp,
        } = initialize_buildings(towers, &layout.constructions, &layout.tower_levels)?;
        let constructions = initialize_constructions(&buildings, &layout.constructions)?;
        let target_quadtrees = initialize_target_quadtrees(&actors, &buildings);
        let mech_quadtrees = initialize_mech_quadtrees(&actors);
        let mut simulation = Self {
            actors,
            team_random: BTreeMap::new(),
            projectiles: Vec::new(),
            buildings,
            target_quadtrees,
            mech_quadtrees,
            identities: IdentityAllocator::new(),
            rvo_counter: 0,
            rvo_first_tree_pending: true,
            terminal_drain_pending: false,
            stop_step: None,
            late_building_events_pending: false,
            fallen_buildings: Vec::new(),
            tower_buff_events: BTreeMap::new(),
            dropped_buffs: BTreeMap::new(),
            construction_colliders: construction_colliders.clone(),
            unsearchable_buildings: unsearchable.clone(),
            constructions,
            towers: towers.clone(),
            tower_losses,
            tower_buffed_constructions,
            statistics: BTreeMap::new(),
            construction_recorders: BTreeMap::new(),
            formations: BTreeMap::new(),
            attackers: BTreeMap::new(),
            building_exp,
            experience: experience::ExperienceTable::load()?,
        };
        simulation.seed_statistics(&construction_groups);
        simulation.seed_experience()?;
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
            statistics: self.statistics.values().copied().collect(),
            formations: self.formation_states(),
            ..WorldSnapshot::default()
        }
    }

    /// Reads a unit with no enemy left at its interval as composed, with no
    /// stagger.
    ///
    /// The stagger rides on the cycle in progress, and a unit's last enemy
    /// dying ends it: from the tick after, the game reads a Marksman at 62, an
    /// Arclight at 18 and a Wraith at 32 — their descriptions — whatever cycle
    /// they were on. That reset is the lock being lost, so it reaches only a
    /// unit that still held one: the Mustang of `m3-crawler.yaml` (seed
    /// 1787720817) whose own target died the tick before the last enemy did
    /// stands idle and lockless, and reads its drawn 6 until the fight's final
    /// tick, where every unit reads its composed interval.
    fn settle_intervals(&mut self, every_unit: bool) {
        let alive_teams = self
            .actors
            .values()
            .filter(|actor| actor.alive())
            .map(|actor| actor.placement.team)
            .collect::<BTreeSet<_>>();
        for actor in self.actors.values_mut().filter(|actor| actor.alive()) {
            let team = actor.placement.team;
            if alive_teams.iter().all(|&other| other == team)
                && (every_unit || actor.skill.lock_target.is_some())
            {
                actor.skill.current_attack_interval =
                    native_time_units_to_steps(actor.stats.attack_interval());
            }
        }
    }

    /// The same for every unit, on the fight's last tick, before its snapshot
    /// is written.
    fn settle_intervals_if_finishing(&mut self) {
        if self.ready_to_finish() {
            self.settle_intervals(true);
        }
    }

    #[allow(clippy::too_many_lines)]
    fn step(&mut self, step: u64) -> Result<TransitionEvents> {
        self.settle_intervals(false);
        let publish_late_building_events = self.late_building_events_pending;
        self.late_building_events_pending = false;
        if self.terminal_drain_pending {
            self.terminal_drain_pending = false;
        }
        let fight_was_finished = self.naturally_finished();
        let winner_was_decided = self.winner().is_some();
        self.stop_step = winner_was_decided.then_some(step);
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
        self.step_projectiles(&mut events)?;
        let projectile_finished_fight =
            !naturally_finished_before_projectiles && self.naturally_finished();
        // `FightingState.Update` runs `FightCoreSystem.TryDstroyTower` after
        // every module has updated: a side that has lost its last unit loses
        // its towers on that tick, whatever dealt the last blow, and their
        // `OnDead` lands on the next. Projectile drain reaches the same round
        // result without mutating buildings (observed in Fang mirror battles).
        let towers_fall = !fight_was_finished
            && !winner_was_decided
            && !projectile_finished_fight
            && self.winner().is_some();
        let mut queued_late_building_events = false;
        for building in self.buildings.iter_mut().filter(|building| {
            towers_fall
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
        let ready_to_finish = self.ready_to_finish();
        let stop_fight = ready_to_finish || winner_was_decided;
        if stop_fight {
            for actor in self.actors.values_mut() {
                // Every motion enters `MotionIdleState`, whose `Enter` asks
                // `StopMove`: a unit that took a tower on the last kill's tick
                // publishes no speed at the next solve.
                let entered_idle = actor.motion.state != MotionState::Idle;
                actor.stop_in_place(entered_idle);
                actor.skill.drop_lock();
                actor.skill.clear_slots();
                // A won fight runs on without `FightSkill.ExitFight` until it
                // ends: a skill already cooling goes on cooling at what it
                // named, and only the end of the fight ends it. A cooling
                // this very tick would have begun is not one the build's
                // attack state has entered yet, and goes idle with it.
                let cooling_before = actor
                    .skill
                    .cooling()
                    .is_some_and(|(started, _)| started < step);
                if ready_to_finish || !cooling_before {
                    actor.skill.set_cooling(None);
                    actor.skill.set_phase(FightSkillPhase::Idle);
                }
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
        // `BuffManager.Clear` takes a dying unit's buffs as it dies.
        for death in deaths {
            let dead_id = death
                .subject
                .filter(|subject| subject.kind == ObjectKind::Unit)
                .map(|subject| subject.id);
            events.push(death);
            if let Some(dead_id) = dead_id {
                events.extend(self.buffs_cleared_by_death(dead_id));
            }
        }
        self.dropped_buffs.clear();
        // What the tick's hits killed and felled comes last, in the order they
        // struck: a block a shot fells reads between the deaths its splash
        // caused, and after every removal the tick resolved. A fallen tower's
        // buff follows it.
        for fallen in std::mem::take(&mut self.fallen_buildings) {
            let building_id = fallen
                .subject
                .filter(|subject| subject.kind == ObjectKind::Building)
                .map(|subject| subject.id);
            events.push(fallen);
            if let Some(applied) = building_id.and_then(|id| self.tower_buff_events.remove(&id)) {
                events.extend(applied);
            }
        }
        if !self.tower_buff_events.is_empty() {
            return Err(Error::new(
                "a tower's loss wrote its buff on a tick that records no building_destroyed for it",
            ));
        }
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
            if let Some(tree) = self.mech_quadtrees.get_mut(&team) {
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

    /// What happens between a tick's work and its snapshot: intervals settle
    /// as the fight finishes, and `BattleSystem.OnFightOver` prunes each
    /// formation's experience to a whole number before the last state is read.
    fn close_tick(&mut self, out_of_time: bool) {
        self.settle_intervals_if_finishing();
        if self.ready_to_finish() || out_of_time {
            self.prune_experience();
        }
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
