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
    time::{Duration, Instant},
};

use mechcore_mcfr::{
    AttackPhase, BuildingState, ControlState, DamageStatistics, Domain, DurableContext,
    EnabledSkill, Event, EventPayload, FormationState, GaugeI32, Hashes, IdentityAllocator,
    LiveUnitState, McfrReader, McfrWriter, MemoryRecording, MotionState, ObjectKind, ObjectRef,
    PersonalShieldState, Producer, ProjectileState, QPlanar, QPose, QVec3, Rational, RecorderKind,
    Recording, SkillMachineState, SkillState, TickSlice, TransitionEvents, Visibility, WeaponState,
    WorldSnapshot,
};

use serde::Serialize;

use crate::{
    Error, Record, Result,
    layout::{
        CompiledLayout, ConstructionBuilding, InterceptorBuilding, MissileMine, MissileShot,
        Placement,
    },
    rules::{
        AttackConfig, AttackPath, AttackTargets, ExtraWeaponConfig, Magazine, MapBuilding,
        MapsConfig, RvoSize, SimulationConfig, TowersConfig, UnitConfig, UnitConfigs, UnitDomain,
        WeaponArc, WeaponMode, WeaponMount,
    },
};

mod attack_count;
mod attacker;
mod buff_cycle;
mod commander_skill;
mod construction;
mod control;
mod damage;
mod deploy;
mod diffusion;
mod experience;
mod explosion;
mod grid;
mod important_unit;
mod intercept;
mod kills;
mod math;
mod mech;
mod mech_group;
mod mine;
mod motion;
mod path_finding;
mod pilot;
mod projectile;
mod random;
mod reactive_armor;
mod recovery;
mod run;
mod rvo;
mod search;
mod shield;
mod siege;
mod skill;
mod statistics;
mod stealth;
mod super_deployment;
mod support_unit;
mod sweep;
mod terrain;
#[cfg(test)]
mod tests;
mod tower;
mod underground;

#[cfg(test)]
use attacker::Facing;
use construction::*;
use damage::*;
use deploy::*;
use intercept::*;
pub(crate) use math::*;
use mine::*;
use motion::*;
use projectile::*;
use random::GrRandom;
pub(crate) use run::*;
pub use run::{
    DivergentTick, Phases, SimulationComparison, SimulationProfile, SimulationResult, SlowestStep,
    TimelineSummary,
};
use rvo::{AgentInput as RvoAgentInput, AgentKey as RvoAgentKey, AgentSizeType, FixedVec2};
use search::*;
#[cfg(test)]
use skill::ATTACK_COUNT_RESET;
#[cfg(test)]
use skill::Performer;
use skill::{
    ExtraSkill, FightSkillPhase, GroupBehaviour, JoinedSlot, Launch, Skill, SkillKind,
    SkillManager, SkillRef, SkillSlot, SkillUpdate,
};
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
/// The RVO group of a map's neutral crystal, which no team owns. A crystal
/// is passable by no group, so which one it is changes nothing.
const NEUTRAL_RVO_GROUP: i32 = -1;

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
#[allow(
    clippy::struct_excessive_bools,
    reason = "a view mirrors the native actor's independent state flags"
)]
struct FightActorView {
    team: u32,
    x_q32: i64,
    z_q32: i64,
    query_x_q32: i64,
    query_z_q32: i64,
    radius: i64,
    alive: bool,
    query_alive: bool,
    /// Whether the tick-start snapshot the selectors score saw it visible:
    /// `ScoreRatingTargetSelector` passes over a unit that is not.
    query_visible: bool,
    /// Its visibility in that snapshot, which the selectors' own
    /// `AttackTargetFilter` asks.
    query_visibility: Visibility,
    targetable: bool,
    /// Whether it could be targeted as the tick opened, which the scores a
    /// search prepared then read: a tower that falls during the tick is
    /// still its winner.
    query_targetable: bool,
    /// `FightActor.IsVisible`, which the selectors and the range check ask
    /// and a lock already held does not: a Rhino keeps its lock on a
    /// Sandworm that burrows and walks on towards it.
    visible: bool,
    /// `FightActor.visibility`, which a shot that lands asks no more of than
    /// that it is not hidden (`IsValidTarget(Stealth)`).
    visibility: Visibility,
    domain: UnitDomain,
}

impl FightActorView {
    /// The visibility half of `AttackTargetFilter.Check`, which every
    /// selector's search asks: not in stealth, as the tick opened for a
    /// prepared search, or now.
    fn searchable(&self, prepared: bool) -> bool {
        let visibility = if prepared {
            self.query_visibility
        } else {
            self.visibility
        };
        visibility != Visibility::Stealth
    }
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
    target_query_visibility: Visibility,
    body_rotation: i64,
    body_rotation_q32: i64,
    aim_rotation: i64,
    /// The mech body's rotation, for a unit whose weapons each turn within
    /// an arc of their own (`RotationLimitFightTransform`): the motion turns
    /// it, and each weapon turns apart from it in its skill's update.
    turret_q32: Option<i64>,
    /// Where the mech body turns to on this update, for a batch of
    /// standalone weapons ([`Simulation::aim_standalone_turret`]), which
    /// need not be the unit's own lock the motion walks on.
    turret_aim_q32: Option<i64>,
    life: i64,
    last_damage_source: Option<(Option<ObjectRef>, u32)>,
    /// Whether a support skill summoned it: a unit with no `MechTeam`, which
    /// counts alone and never gains experience.
    summoned: bool,
    /// `FightMech.IsSuperDeployment`: still travelling to the field, which
    /// `FightCoreSystem.TeamUpdate` counts alive and does not update.
    travelling: bool,
    /// Whether its skill has searched an attack target: every unit at the
    /// fight's presearch, and a travelling one only at its first update.
    searched_attack: bool,
    /// `FightActor.lastLifeBeforeSuicide`: the life a unit had as its own
    /// blow took it, which its explosion deals.
    last_life_before_suicide: i64,
    /// `PilotAI`'s command: the path a Mobile Beacon walks it along, which
    /// its motion follows rather than its lock until it arrives.
    command: Option<pilot::MoveCommand>,
    /// The buffs running on it, `BuffManager`'s list.
    buffs: Vec<RunningBuff>,
    /// Its `EnergyShieldController`'s energy and maximum, when a shield
    /// source is in force on it: `EnergyShieldProvider.AddEffect` sets the
    /// controller's `lifeRate`, and its `Open` fills it to the whole part of
    /// the unit's maximum life times that rate.
    shield: Option<PersonalShield>,
    /// Its `BuffCycleController`s, one for each of its buff sources.
    buff_cycles: Vec<buff_cycle::BuffCycle>,
    /// `BuffManager.beHitDelayBuffInfos`: the buffs that disable technology
    /// a unit it hit queued on it, each with that unit, which
    /// `InvokeDelayAddBuff` adds as its `BuffManager.Update` ends.
    delayed_buffs: Vec<(u64, crate::modifier::BuffSource)>,
    /// `FightMech.mechCreateType` is `ParasiticalSummon`: a buff summoned it
    /// as a unit died, and it summons nothing as it dies.
    parasitic: bool,
    /// `MotionController.totalMoveDistanceWithoutDisableTech`, Q32.32 metres,
    /// and `prevPosition`, where its last `Move` before a solve found it.
    moved_q32: i64,
    move_mark_q32: (i64, i64),
    /// Its `AutoRecoveryController`'s clocks, when a repair source is in
    /// force on it.
    recovery: Option<recovery::RecoveryClock>,
    /// Its entry in `ReactiveArmorSystem`, when a technology gives it one.
    reactive_armor: Option<reactive_armor::ReactiveArmorState>,
    /// `RVOControllerFixed._maxSpeed`: the speed `Active` read when the unit
    /// took the field, which `StopMove` hands the agent as its maximum. A
    /// buff that changes the unit's speed later reaches a moving agent through
    /// `Move`, and never this.
    rvo_max_speed_q32: i64,
    /// `FightActor.visibility`: a hidden unit is no target.
    visibility: Visibility,
    /// `SkillManager.isActive`: a move ability stops every skill while it
    /// burrows or surfaces.
    skills_active: bool,
    /// Its move ability, for a unit whose description moves underground.
    underground: Option<underground::Underground>,
    /// The side it was deployed on, which a control beam may have turned it
    /// from.
    original_team: u32,
    /// The formation it was deployed in, which it leaves when it is turned
    /// and is counted in again when it dies.
    original_formation: u64,
    pub(in crate::fight) motion: Motion,
    pub(in crate::fight) skills: SkillManager,
}

/// A unit's own shield, `EnergyShieldController`.
#[derive(Debug, Clone, Copy)]
struct PersonalShield {
    energy: i64,
    maximum: i64,
}

/// The production lines the units run, one creator each, made as the fight
/// starts in the order the units were placed.
fn production_creators(actors: &BTreeMap<u64, Actor>) -> Vec<support_unit::Creator> {
    actors
        .values()
        .filter_map(|actor| {
            actor
                .placement
                .production
                .as_ref()
                .map(|production| support_unit::Creator::production(actor, production))
        })
        .collect()
}

/// The battlefield shields the units carry into the fight, each where its
/// owner stands as it is placed: `AdvancedEnergyShieldProvider` creates one
/// for a unit whose equipment is a Barrier.
fn carried_shields(actors: &BTreeMap<u64, Actor>) -> Vec<shield::CarriedShieldPlacement> {
    actors
        .iter()
        .filter_map(|(&id, actor)| {
            actor
                .placement
                .carried_shield
                .map(|carried| shield::CarriedShieldPlacement {
                    team: actor.placement.team,
                    x_q32: actor.x_q32,
                    z_q32: actor.z_q32,
                    radius_q32: carried.radius << 32,
                    energy: carried.energy,
                    owner: id,
                })
        })
        .collect()
}

/// `GameRiver.BuildingType.Special`.
const CONSTRUCTION_BUILDING_TYPE: u32 = 3;

/// The identities the fight hands out as it goes: to projectiles, and to the
/// units and formations that join it.
struct Identities {
    /// The projectiles', and every other object's the recording numbers.
    objects: IdentityAllocator,
    /// The identity the next unit to join takes, and its formation's.
    next_unit: u64,
    next_formation: u64,
}

/// `FightingState` as the fight ends: the step it stops on, and what its end
/// tore down and still owes the recording.
#[derive(Default)]
struct Ending {
    /// The step the fight stops on: a side had already won when it began,
    /// and no skill updates on it.
    stop_step: Option<u64>,
    terminal_drain_pending: bool,
    late_building_events_pending: bool,
    /// The buildings `FightCoreSystem.TryDstroyTower` tore down, whose
    /// `building_destroyed` the next tick records.
    torn_down_buildings: Vec<u64>,
}

struct Simulation {
    actors: BTreeMap<u64, Actor>,
    /// `TeamTranslationSystem.translatingDatas`, which a `SyncDictionary`
    /// keeps in the order of its keys: two units due on one tick are turned
    /// the lower identity first, whichever beam began first.
    translations: Vec<control::Translation>,
    /// The turned units that died this tick, in the order they died, whose
    /// `OnDead` hands them back to their side once every unit has updated.
    /// The units a death handed back to the side they were deployed on:
    /// `FightTeam.AddMech` put them in that side's trees after the dead had
    /// left them, and nothing takes them out again.
    returned_dead: BTreeSet<u64>,
    /// The splashes under way that diffuse, in the order they started: the
    /// `GRTimerManager` timers of their performers.
    diffusions: Vec<diffusion::Diffusion>,
    turned_fallen: Vec<u64>,
    /// The units a beam turned on this tick, whose formations of their own
    /// the recorder numbers as it records the tick.
    turned_unnamed: Vec<u64>,
    /// `DeadEffectSystem.deadEffectMeches` of `DeadExplosiveController`: the
    /// units with an explosion that died this tick, in the order they died,
    /// and whether each took its own life.
    dead_explosions: Vec<(u64, bool)>,
    /// `DeadEffectSystem.deadActors` of the units a hit killed this tick, in
    /// the order they died: each one's `FightMech.OnDead` leaves the fight
    /// (`ExitFight`) when that module updates, and until then its skill
    /// keeps what it fired at.
    dead_exits: Vec<u64>,
    /// The step being simulated, which what happens inside a hit reads.
    step_now: u64,
    /// The order the fight updates its deployed units in, which is not their
    /// identity order: see [`deploy::update_order`].
    unit_update_order: Vec<u64>,
    team_random: BTreeMap<u32, GrRandom>,
    projectiles: Vec<Projectile>,
    /// Each side's interceptors, `InterceptSystem`'s sources.
    interceptors: Vec<Interceptor>,
    /// Each side's missiles still standing, `MineSystem`'s, in side order.
    mines: Vec<Mine>,
    /// Each side's `SuperDeploymentController` that opened with a unit
    /// travelling.
    travels: BTreeMap<u32, super_deployment::Travel>,
    buildings: Vec<BuildingState>,
    target_quadtrees: BTreeMap<u32, TargetActorQuadtree>,
    /// The target trees as `FightCoreSystem.PreCalculate` found them, as the
    /// tick's search order was taken: what a prepared search asks.
    prepared_target_quadtrees: BTreeMap<u32, TargetActorQuadtree>,
    /// Each side's units alone, `FightTeam.mechQuadtree`, which the
    /// experience a kill shares out is looked for in.
    mech_quadtrees: BTreeMap<u32, TargetActorQuadtree>,
    /// Buildings a projectile destroyed this tick, held until every projectile
    /// has resolved so their events follow all of the tick's shots.
    fallen_buildings: Vec<Event>,
    /// The buildings standing when the tick's target queries were prepared,
    /// as `target_query_alive` is for a unit.
    buildings_query_alive: std::collections::BTreeSet<u64>,
    /// The buildings no unit searches for, which the target trees hold all
    /// the same.
    unsearchable_buildings: BTreeSet<u64>,
    /// Every construction's blocks, the buildings that are a
    /// `FightConstruction`: not a tower, nor an interceptor.
    construction_blocks: BTreeSet<u64>,
    /// The constructions whose skill fires, by building.
    constructions: BTreeMap<u64, Construction>,
    /// `BattleStatisticManager`'s counters.
    statistics: statistics::StatisticsSystem,
    /// `AdvancedEnergyShieldSystem`'s battlefield shields.
    shield: shield::ShieldSystem,
    /// `SupportUnitSystem`'s creators and the summons still appearing.
    support: support_unit::SupportUnitSystem,
    /// `CommanderSkillSystem`'s releases still to land.
    commander: commander_skill::CommanderSkillSystem,
    /// `ExpSystem`'s formations, attackers and loot.
    exp: experience::ExpSystem,
    /// `FightCoreSystem`'s attackers of each target, whom its death credits.
    kills: kills::KillCounts,
    /// `RangeItemSystem`'s terrains and the units standing in them.
    terrain: terrain::TerrainSystem,
    /// `StealthTechSystem`'s units.
    stealth: stealth::StealthSystem,
    /// `SiegeModeEffectSystem`'s units.
    siege: siege::SiegeModeSystem,
    /// `MechGrounpSystem`'s groups.
    mech_groups: mech_group::MechGroupSystem,
    /// The RVO simulator's state and the obstacles besides the units.
    rvo: RvoState,
    /// The buffs on constructions and the buff events a tick holds back.
    buffs: tower::BuffState,
    /// The towers of both sides and what losing one writes.
    towers: tower::TowerSystem,
    /// The identities the fight hands out.
    ids: Identities,
    /// How the fight ends, and what its end still owes the recording.
    ending: Ending,
}

impl Simulation {
    #[cfg(test)]
    fn new(
        layout: &CompiledLayout,
        configs: &UnitConfigs,
        towers: &TowersConfig,
        maps: &MapsConfig,
        seed: i32,
    ) -> Result<Self> {
        let mut simulation = Self::new_unprepared(layout, configs, towers, maps, seed)?;
        simulation.initialize_presearch_targets()?;
        Ok(simulation)
    }

    #[allow(clippy::too_many_lines)]
    fn new_unprepared(
        layout: &CompiledLayout,
        configs: &UnitConfigs,
        towers: &TowersConfig,
        maps: &MapsConfig,
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
        let mut actors = initialize_actors(layout, configs, seed)?;
        let travels = super_deployment::enter_travel(&mut actors, &layout.travel_time_rates)?;
        let unit_update_order = deploy::update_order(&actors);
        let InitialBuildings {
            states: buildings,
            unsearchable,
            colliders: construction_colliders,
            passable_constructions,
            tower_losses,
            tower_buffed_constructions,
            construction_groups,
            building_exp,
            interceptors,
        } = initialize_buildings(
            towers,
            &layout.constructions,
            &layout.interceptors,
            &layout.tower_levels,
        )?;
        let constructions = initialize_constructions(&buildings, &layout.constructions)?;
        let grounds = formation_grounds(layout, configs)?;
        let map_crystals = map_crystals(maps.buildings(layout.map_id)?, &grounds);
        let target_quadtrees =
            initialize_target_quadtrees(&actors, &buildings, &construction_colliders);
        let mech_quadtrees = initialize_mech_quadtrees(&actors);
        let buildings_query_alive = standing_buildings(&buildings);
        let carried = carried_shields(&actors);
        let productions = production_creators(&actors);
        let mut simulation = Self {
            actors,
            translations: Vec::new(),
            turned_fallen: Vec::new(),
            turned_unnamed: Vec::new(),
            returned_dead: BTreeSet::new(),
            diffusions: Vec::new(),
            dead_explosions: Vec::new(),
            dead_exits: Vec::new(),
            step_now: 0,
            unit_update_order,
            team_random: BTreeMap::new(),
            projectiles: Vec::new(),
            interceptors,
            mines: {
                let mut mines = layout.missiles.iter().map(Mine::new).collect::<Vec<_>>();
                mines.sort_by_key(Mine::team);
                mines
            },
            shield: shield::ShieldSystem::new(&layout.shields, &carried),
            commander: commander_skill::CommanderSkillSystem::new(layout),
            support: support_unit::SupportUnitSystem::new(productions, layout),
            travels,
            ids: Identities {
                objects: IdentityAllocator::new(),
                next_unit: 0,
                next_formation: 0,
            },
            ending: Ending::default(),
            buildings,
            prepared_target_quadtrees: target_quadtrees.clone(),
            target_quadtrees,
            mech_quadtrees,
            rvo: RvoState::new(construction_colliders, passable_constructions, map_crystals),
            fallen_buildings: Vec::new(),
            buildings_query_alive,
            unsearchable_buildings: unsearchable.clone(),
            construction_blocks: construction_groups.keys().copied().collect(),
            constructions,
            towers: tower::TowerSystem {
                config: towers.clone(),
                losses: tower_losses,
                buffed_constructions: tower_buffed_constructions,
                fallen: Vec::new(),
            },
            buffs: tower::BuffState::default(),
            statistics: statistics::StatisticsSystem::default(),
            exp: experience::ExpSystem::new(building_exp)?,
            kills: kills::KillCounts::default(),
            terrain: terrain::TerrainSystem::default(),
            stealth: stealth::StealthSystem::default(),
            siege: siege::SiegeModeSystem::default(),
            mech_groups: mech_group::MechGroupSystem::default(),
        };
        simulation.number_joiners();
        simulation.activate_interceptions();
        simulation.enter_stealth_fight();
        simulation.enter_reactive_armor_fight();
        simulation.start_groups();
        simulation.restore_standing_oil(&layout.standing_oil)?;
        // `CommanderSkillManager.OnFightStart`: a path is given out before
        // the first tick, and lands nothing.
        let releases = std::mem::take(&mut simulation.commander.releases);
        simulation.commander.releases = simulation.start_paths(releases);
        simulation.seed_statistics(&construction_groups);
        simulation.seed_experience()?;
        simulation.start_side_streams(layout.round);
        simulation.place_sub_effects()?;
        simulation.deploy_attack_intervals()?;
        simulation.face_constructions_at_fight_start(&layout.legacy_units, &layout.delivered);
        // `SiegeModeEffectSystem.OnEnterFight` digs its units in after each
        // skill drew its first interval as it was deployed, from the
        // interval the trench has not shortened yet.
        simulation.enter_siege_fight()?;
        Ok(simulation)
    }

    /// A unit joining the fight later takes the next number, and its
    /// formation the next formation's.
    fn number_joiners(&mut self) {
        self.ids.next_unit = self.actors.keys().max().map_or(1, |id| id + 1);
        self.ids.next_formation = self
            .actors
            .values()
            .map(|actor| actor.placement.formation_id)
            .max()
            .map_or(1, |id| id + 1);
    }

    /// What a recording holds of one unit.
    pub(in crate::fight) fn unit_snapshot(&self, id: u64) -> LiveUnitState {
        let mut state = self.actors[&id].snapshot(self.skill_states(id));
        state.control = self
            .translations
            .iter()
            .find(|entry| entry.target == id)
            .map(|entry| ControlState {
                progress: entry.progress,
                sources: entry
                    .sources
                    .iter()
                    .map(|source| ObjectRef::new(ObjectKind::Unit, source.owner))
                    .collect(),
            });
        state
    }

    /// Every skill a unit's `GetSkills()` holds, slot by slot: the main
    /// skill's group, then each extra skill's.
    fn skill_states(&self, id: u64) -> Vec<SkillState> {
        let actor = &self.actors[&id];
        let owner = FightActorRef::Unit(id);
        let slots = actor.skills.main_slots()
            + actor
                .skills
                .extras
                .iter()
                .map(|extra| extra.skill.group_size().max(1))
                .sum::<usize>();
        let mut weapons = actor.slot_weapons();
        weapons.sort_by_key(|(slot, weapon)| (*slot, weapon.weapon_index));
        (0..slots)
            .map(|slot| {
                let (held_by, offset) = actor.skills.at_slot(slot);
                let holder = actor.skills.get(held_by);
                let skill = holder.group_skill(offset);
                let skill_ref = SkillRef {
                    owner,
                    slot: held_by,
                };
                // A travelling unit's skills are not read until it arrives: it
                // does not update, and they do nothing, its extra weapons
                // whether a deployment action switched them off or not.
                let enabled = (!holder.disabled && !actor.travelling).then(|| EnabledSkill {
                    lock_target: skill.lock_target.map(FightActorRef::object_ref),
                    attack_target: actor
                        .slot_attack_target(slot)
                        .map(FightActorRef::object_ref),
                    state: recorded_machine_state(
                        &skill.state,
                        self.step_now,
                        math::native_time_units_to_steps(
                            self.skill_rules(skill_ref).cooling_time_units(),
                        ),
                    ),
                    attack_phase: recorded_attack_phase(skill, self.step_now),
                    attack_time: i32::try_from(
                        i64::try_from(self.step_now).unwrap_or(i64::MAX) - skill.attack_time_anchor,
                    )
                    .unwrap_or(i32::MAX),
                    current_attack_interval: i32::try_from(skill.current_attack_interval)
                        .unwrap_or(i32::MAX),
                    // `SkillAttackController.attackCount` counts a blow as it
                    // starts; this simulator counts it as its attack point
                    // lets it through.
                    attack_count: skill.attack_count
                        + i32::from(matches!(
                            skill.state,
                            skill::SkillState::Attack(skill::Blow::Before(_))
                        )),
                    perform_count: i32::try_from(skill.performed_count(self.step_now))
                        .unwrap_or(i32::MAX),
                    attack_range: self.slot_attack_range_q32(skill_ref, offset),
                    splash_range: math::space_to_q32(
                        self.skill_attacker(skill_ref)
                            .expect("skill owner identity is stable")
                            .splash_radius,
                    ),
                    attack_damage: self.slot_normal_damage(actor, skill_ref),
                    weapons: weapons
                        .iter()
                        .filter(|(held, _)| *held == slot)
                        .map(|(_, weapon)| weapon.clone())
                        .collect(),
                });
                SkillState {
                    skill_slot: u16::try_from(slot).expect("skill slot fits u16"),
                    enabled,
                }
            })
            .collect()
    }

    /// `FightSkill.GetAttackRange` of the skill at a place of its group, in
    /// Q32.32 metres: the main skill's for what it locks, a main slot's
    /// beyond it, and an extra skill's.
    pub(in crate::fight) fn slot_attack_range_q32(
        &self,
        skill_ref: SkillRef,
        offset: usize,
    ) -> i64 {
        let FightActorRef::Unit(id) = skill_ref.owner else {
            unreachable!("only a unit records skills")
        };
        match skill_ref.slot {
            SkillSlot::Main => {
                let main = self.main_attack_range_q32(id);
                let addend = self.slot_attack_range(skill_ref, Some(offset))
                    - self.slot_attack_range(skill_ref, Some(0));
                main.saturating_add(math::space_to_q32(addend))
            }
            // A skill that reaches past the main skill's range reads the
            // main skill's `GetAttackRange` in Q32.32 and adds its own: a
            // Secondary Armament under a buff that cuts the Sabertooth's
            // range by 30% reads the main gun's 77 metres and 88 raw, and 2
            // more.
            SkillSlot::Extra(index) => {
                let extra = &self.actors[&id].skills.extras[index];
                if extra.rules.use_main_skill_range || extra.skill.is_grouped() {
                    self.main_attack_range_q32(id)
                        .saturating_add(math::space_to_q32(extra.rules.attack.range()))
                } else {
                    math::space_to_q32(
                        self.skill_attacker(skill_ref)
                            .expect("skill owner identity is stable")
                            .attack_range,
                    )
                }
            }
        }
    }

    /// `FightSkill.GetNormalDamage(0)` of a skill: a beam's at its ramp's
    /// first step, whatever step it is on.
    fn slot_normal_damage(&self, actor: &Actor, skill_ref: SkillRef) -> i32 {
        if let Some(damage) = self.beam_snapshot_damage(skill_ref) {
            return damage;
        }
        let damage = match skill_ref.slot {
            SkillSlot::Main => {
                match &actor.rules.attack.path {
                    // The Steel Balls of `wall-laser.yaml` read 2, which is
                    // 55 at its first multiplier, on every tick of their
                    // fight.
                    crate::rules::AttackPath::Laser { .. } => {
                        actor.stats.laser_normal_damage(&actor.rules, 0)
                    }
                    _ => actor.stats.normal_damage(&actor.rules),
                }
            }
            SkillSlot::Extra(index) => {
                let rules = &actor.skills.extras[index].rules;
                match &rules.attack.path {
                    // An extra beam with a damage rate ramps from the unit's
                    // base damage at that rate: Energy Diffraction's read 1.
                    crate::rules::AttackPath::Laser { damage_multipliers }
                        if rules.damage_rate > 0.0 =>
                    {
                        actor.stats.ramped_laser_damage(
                            actor.rules.attack.base_damage,
                            damage_multipliers,
                            (rules.damage_rate, 0),
                            (0, UnitDomain::Ground),
                            true,
                        )
                    }
                    _ => {
                        self.skill_attacker(skill_ref)
                            .expect("skill owner identity is stable")
                            .attack_damage
                    }
                }
            }
        };
        i32::try_from(damage).unwrap_or(i32::MAX)
    }

    fn snapshot(&self) -> WorldSnapshot {
        WorldSnapshot {
            live_units: self
                .actors
                .values()
                .filter(|actor| actor.alive())
                .map(|actor| self.unit_snapshot(actor.placement.unit_id))
                .collect(),
            projectiles: self.projectiles.iter().map(Projectile::snapshot).collect(),
            buildings: self
                .buildings
                .iter()
                .filter(|building| building_alive(building))
                .cloned()
                .collect(),
            shields: self.shield_states(),
            terrains: self.terrain_states(),
            statistics: self.statistics.recorders.values().copied().collect(),
            formations: self.formation_states(),
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
    fn settle_intervals(&mut self, every_unit: bool, step: u64) {
        let alive_teams = self
            .actors
            .values()
            .filter(|actor| actor.alive())
            .map(|actor| actor.placement.team)
            .collect::<BTreeSet<_>>();
        let settled = self
            .actors
            .values()
            .filter(|actor| {
                actor.alive()
                    && alive_teams
                        .iter()
                        .all(|&other| other == actor.placement.team)
            })
            .map(|actor| {
                let id = actor.placement.unit_id;
                let extras = (0..actor.skills.extras.len())
                    .map(|index| {
                        self.skill_attacker(SkillRef {
                            owner: FightActorRef::Unit(id),
                            slot: SkillSlot::Extra(index),
                        })
                        .map_or(0, |attacker| {
                            seconds_q32_to_steps(attacker.attack_interval_q32)
                        })
                    })
                    .collect::<Vec<_>>();
                (
                    id,
                    seconds_q32_to_steps(actor.stats.attack_interval_q32()),
                    extras,
                )
            })
            .collect::<Vec<_>>();
        // `RefreshAttackInterval` with the clock refreshed: each skill, and
        // each slot of its group, that holds a lock, and on the fight's last
        // tick every one, reads its own composed interval.
        let refresh = |skill: &mut Skill, interval: u64| {
            let anchor = i64::try_from(step).unwrap_or(i64::MAX)
                - i64::try_from(interval).unwrap_or(i64::MAX);
            let held = skill.lock_target.is_some();
            if every_unit || held {
                skill.current_attack_interval = interval;
                skill.attack_time_anchor = anchor;
            }
            for sibling in skill.siblings_mut() {
                if every_unit || sibling.lock_target.is_some() {
                    sibling.current_attack_interval = interval;
                    sibling.attack_time_anchor = anchor;
                }
            }
        };
        for (id, main, extras) in settled {
            let actor = self.actors.get_mut(&id).expect("actor identity is stable");
            refresh(&mut actor.skills.main, main);
            for (extra, interval) in actor.skills.extras.iter_mut().zip(extras) {
                refresh(&mut extra.skill, interval);
            }
        }
    }

    /// The same for every unit, on the fight's last tick, before its snapshot
    /// is written.
    fn settle_intervals_if_finishing(&mut self) {
        if self.ready_to_finish() {
            self.settle_intervals(true, self.step_now);
        }
    }

    #[allow(clippy::too_many_lines)]
    fn step(&mut self, step: u64) -> Result<TransitionEvents> {
        if self.winner().is_some() {
            // A fight already won updates no skill: no `attackTime` counts.
            for actor in self.actors.values_mut() {
                actor.skills.hold_attack_clocks();
            }
        }
        self.settle_intervals(false, step);
        let publish_late_building_events = self.ending.late_building_events_pending;
        self.ending.late_building_events_pending = false;
        let drain_tick = self.ending.terminal_drain_pending;
        if self.ending.terminal_drain_pending {
            self.ending.terminal_drain_pending = false;
        }
        self.step_now = step;
        let fight_was_finished = self.naturally_finished();
        if drain_tick && fight_was_finished {
            // The fight ends on this tick, and every mech has left it
            // (`MotionController.ExitFight`) before any updates: a Sandworm
            // still underground is cleared and stands where it was.
            for actor in self.actors.values_mut() {
                actor.exit_fight_move_ability();
            }
        }
        let winner_was_decided = self.winner().is_some();
        self.ending.stop_step = winner_was_decided.then_some(step);
        self.refresh_target_query_snapshot();
        // `GRTimerManager.Update` runs before any module: the summons whose
        // second is up join the fight first. The tick's searches were
        // prepared without them, after the last tick's modules, and find them
        // from the next: the Crawlers around a Rhino that has just dropped go
        // on searching for what they were after. Their own searches were not
        // prepared, and find the summons that joined with them.
        let mut joined = Vec::new();
        self.join_summons(step, &mut joined)?;
        let mut events = Vec::new();
        let mut torn_down = Vec::new();
        if publish_late_building_events {
            // The towers the fight's end tore down, and then every buff a
            // unit still runs, cleared as the fight is left
            // (`BuffManager.Clear`): a buff the loss of a tower wrote and
            // that has not run out is written as cleared on every survivor.
            // `DeadEffectSystem` writes them after the tick's shots, below.
            for building_id in std::mem::take(&mut self.ending.torn_down_buildings) {
                let position = self
                    .buildings
                    .iter()
                    .find(|building| building.building_id == building_id)
                    .map(|building| building.position)
                    .ok_or_else(|| Error::new("a torn-down building is absent"))?;
                torn_down.push(event(
                    Some(ObjectRef::new(ObjectKind::Building, building_id)),
                    None,
                    None,
                    None,
                    EventPayload::BuildingDestroyed { position },
                ));
            }
        }
        events.extend(joined);
        // Then the timers of the splashes that diffuse, which started after
        // any summon now joining was due: a splash strikes where its
        // enemies stood as the tick opened.
        self.update_diffusions(&mut events)?;
        // And those of the units that left their trench.
        self.idle_after_trenches(step);
        // Native search jobs retain the actor-quadtree candidate order
        // prepared at the start of this FightCore update.
        let target_search_order = self.target_search_order();
        self.prepared_target_quadtrees
            .clone_from(&self.target_quadtrees);
        // `BuffSystem` updates before `CommanderSkillSystem`.
        self.step_buff_cycles(&target_search_order, &mut events)?;
        // `CommanderSkillSystem` and then `MineSystem` update before
        // `FightCoreSystem`: a skill lands, and a missile fires, on where
        // their enemies stood as the tick opened.
        // `TeamTranslationSystem` updates before `FightCoreSystem`: a unit a
        // beam's last hit turned acts on its new side on the next tick.
        self.update_translations(step, &mut events)?;
        self.step_battle_skills(step, &target_search_order, &mut events)?;
        self.step_mines(&target_search_order, &mut events)?;
        // `MechGrounpSystem` updates before `RangeItemSystem`.
        self.step_groups();
        // `RangeItemSystem` updates after `MineSystem` and before
        // `FightCoreSystem`.
        self.step_terrains(&mut events)?;
        // A side whose every unit a beam turned has none left, and is counted
        // all the same: its towers fall as a wiped-out side's do.
        let team_ids = self
            .actors
            .values()
            .flat_map(|actor| [actor.placement.team, actor.original_team])
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
                .units_in_update_order()
                .into_iter()
                .filter(|actor_id| self.actors[actor_id].placement.team == team_id)
                .collect::<Vec<_>>();
            for actor_id in actor_ids {
                if self.actors[&actor_id].travelling {
                    // A unit travelling in updates no skill.
                    self.actors
                        .get_mut(&actor_id)
                        .expect("actor identity is stable")
                        .skills
                        .hold_attack_clocks();
                    continue;
                }
                self.actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable")
                    .searched_attack = true;
                self.step_actor_with_target_order(
                    actor_id,
                    step,
                    &target_search_order,
                    &mut events,
                )?;
                self.step_actor_rvo_position(actor_id);
                self.follow_owner(actor_id);
                self.perform_command(actor_id);
            }
            // Every construction updates, a wall block as a turret: one that
            // fires runs its skill, and each runs its buffs last.
            let building_ids = self
                .buildings
                .iter()
                .filter(|building| {
                    building.team_id == team_id
                        && (self.constructions.contains_key(&building.building_id)
                            || self
                                .buffs
                                .building_buffs
                                .contains_key(&building.building_id))
                })
                .map(|building| building.building_id)
                .collect::<std::collections::BTreeSet<_>>();
            for building_id in building_ids {
                if self.constructions.contains_key(&building_id) {
                    self.step_construction(building_id, step, &target_search_order, &mut events)?;
                }
                // With the fight over, `FightConstruction.Update` returns
                // before `BuffManager.Update`, as a unit's does.
                if self.ending.stop_step.is_none() {
                    self.update_construction_buffs(building_id, &mut events)?;
                }
            }
        }
        let naturally_finished_before_projectiles = self.naturally_finished();
        self.step_projectiles(&mut events)?;
        self.step_interceptors(&mut events)?;
        // `SuperDeploymentSystem` updates after `InterceptSystem`.
        self.step_super_deployment()?;
        // `AutoRecoverySystem` updates after `SuperDeploymentSystem`.
        self.step_auto_recovery(&mut events)?;
        // `SupportUnitSystem` updates after `InterceptSystem`: a creator's
        // summons are made after every unit has moved and every shot landed.
        self.step_support_units(step, &mut events)?;
        // `DeadEffectSystem` updates after `FightCoreSystem` and
        // `ProjectileSystem` (`FightController.AddModules`), and calls `OnDead`
        // on what died this tick: a tower's loss reaches its side after every
        // unit has updated, and counts from the next tick whichever side felled
        // it.
        // Its dead effects first, the explosions among them, and then each
        // dead actor's `OnDead`.
        self.step_dead_explosions(&mut events)?;
        for unit_id in std::mem::take(&mut self.dead_exits) {
            self.actors
                .get_mut(&unit_id)
                .expect("actor identity is stable")
                .exit_fight_on_death();
            // `FightEffectSystem.DeactiveEffect` of the dead unit: its
            // group's `MechGrounpEffectProvider.DoDeactive`.
            self.remove_group_unit(unit_id);
            // Its `SiegeModeEffectProvider.DoDeactive`.
            self.remove_siege_unit(unit_id)?;
            // `SkillManager.OnOwnerDead` stops its skills, a control beam's
            // `ControllEffect` among them: the Rhino a Hacker was turning
            // holds no entry from the tick the Hacker dies.
            self.sync_beam(unit_id);
        }
        self.summon_from_the_dead()?;
        for building_id in std::mem::take(&mut self.towers.fallen) {
            self.lose_tower(building_id)?;
        }
        for unit_id in std::mem::take(&mut self.turned_fallen) {
            self.turned_unit_died(unit_id);
        }
        self.clear_dead_summons()?;
        self.drop_dead_owners_lines();
        self.deactivate_dead_interceptions();
        // Its `TryProcessDeadImportantUnit` too: a side whose last important
        // unit died this tick loses every unit it has left.
        self.lose_important_units(&events)?;
        // `SiegeModeEffectSystem` updates after `FightConstructionSystem`,
        // and `StealthTechSystem` after it, one of the last modules.
        self.step_siege()?;
        self.step_stealth();
        // A side whose last unit died this tick loses its towers even when
        // a shot landing on the same tick is what leaves the fight finished:
        // a Sandworm's blow that kills the last Overlord as a tower's shot
        // lands. Only a fight that waited on its shots alone, its last unit
        // gone earlier, drains without them.
        let unit_died_this_tick = events
            .iter()
            .any(|event| matches!(&event.payload, EventPayload::UnitDied { .. }));
        let projectile_finished_fight = !naturally_finished_before_projectiles
            && self.naturally_finished()
            && !unit_died_this_tick;
        // `FightingState.Update` runs `FightCoreSystem.TryDstroyTower` after
        // every module has updated: a side that has lost its last unit loses
        // its towers on that tick, whatever dealt the last blow, and their
        // `OnDead` lands on the next. Projectile drain reaches the same round
        // result without mutating buildings (observed in Fang mirror fights).
        // A tick that leaves both sides with no unit, a Missile Strike
        // landing among both sides' Crawlers, fells both sides' towers.
        let wiped_out = [0_u32, 1].into_iter().all(|team| {
            team_alive_counts.get(&team).is_none_or(|alive| *alive == 0) && !self.appearing_on(team)
        }) && !team_alive_counts.is_empty()
            && self.projectiles.is_empty();
        let towers_fall = !fight_was_finished
            && !winner_was_decided
            && !projectile_finished_fight
            && (self.winner().is_some() || wiped_out);
        let mut queued_late_building_events = false;
        let appearing_teams = [0_u32, 1]
            .into_iter()
            .filter(|&team| self.appearing_on(team))
            .collect::<BTreeSet<_>>();
        for building in self.buildings.iter_mut().filter(|building| {
            towers_fall
                && building_alive(building)
                && team_alive_counts
                    .get(&building.team_id)
                    .is_some_and(|alive_count| *alive_count == 0)
                && !appearing_teams.contains(&building.team_id)
        }) {
            building.life.current = 0;
            building.targetable = false;
            queued_late_building_events = true;
            self.ending.torn_down_buildings.push(building.building_id);
        }
        if queued_late_building_events {
            self.ending.terminal_drain_pending = true;
            self.ending.late_building_events_pending = true;
        }
        let ready_to_finish = self.ready_to_finish();
        let stop_fight = ready_to_finish || winner_was_decided;
        if stop_fight {
            for actor in self.actors.values_mut() {
                // Every motion loses its target: one without a command enters
                // `MotionIdleState`, whose `Enter` asks `StopMove`, so a unit
                // that took a tower on the last kill's tick publishes no speed
                // at the next solve. A command stays active and moves on
                // while a won fight runs on, and the fight's end idles every
                // motion.
                let entered_idle = actor.motion.state != MotionState::Idle;
                // A manager holding fire updates its main skill all the same
                // (`SkillManager.Update`, `isHoldFire`): a Sandworm below
                // keeps its lock and walks on until the fight ends.
                let holding_fire = !ready_to_finish
                    && actor
                        .underground
                        .as_ref()
                        .is_some_and(|underground| underground.below);
                if ready_to_finish {
                    actor.exit_fight_move_ability();
                    actor.stop_in_place(entered_idle);
                    // Leaving the fight drops the unit's own lock too.
                    if let Some(group) = &mut actor.skills.main.group {
                        group.mech_lock = None;
                    }
                } else if !holding_fire {
                    actor.lose_target_motion(entered_idle);
                }
                actor
                    .skills
                    .main
                    .clear_slots_cooling_before((!ready_to_finish).then_some(step));
                // `FightMech.OnFightEnd` hands the motion back to the main
                // skill (`SetMotionAttackerAfterSkill`).
                actor.motion.attacker = SkillSlot::Main;
                // Every skill of the unit lets its target go, its extra
                // skills' as its main one's. A won fight runs on without
                // `FightSkill.ExitFight` until it ends: a skill already
                // cooling goes on cooling at what it named, and only the end
                // of the fight ends it. A cooling this very tick would have
                // begun is not one the build's attack state has entered yet,
                // and goes idle with it.
                let skills = std::iter::once(&mut actor.skills.main)
                    .filter(|_| !holding_fire)
                    .chain(actor.skills.extras.iter_mut().map(|extra| &mut extra.skill));
                for skill in skills {
                    skill.drop_lock();
                    skill.attack_target_left = None;
                    let cooling_before = skill.cooling().is_some_and(|(started, _)| started < step);
                    if ready_to_finish || !cooling_before {
                        skill.set_cooling(None);
                        skill.set_phase(FightSkillPhase::Idle);
                    }
                }
                if ready_to_finish {
                    actor.motion.current_velocity_x_q32 = 0;
                    actor.motion.current_velocity_z_q32 = 0;
                }
            }
            for construction in self.constructions.values_mut() {
                construction.skills.main.drop_lock();
                construction.skills.main.set_phase(FightSkillPhase::Idle);
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
                .map_or((None, None), |(source, team_id)| (source, Some(team_id)));
        }
        // The deaths and falls the units' own hits caused come after the
        // rest of the tick, in the order they happened, as
        // `DeadEffectSystem.Update` takes its `deadActors`: a block one
        // Steel Ball's beam fells reads before a unit a later beam kills.
        // The towers the fight's end tore down on the tick before are first
        // in that list, after the shots this tick resolved: a Wasp's shot at
        // a tower torn down reads its removal before the tower falls.
        let (rest, ends): (Vec<_>, Vec<_>) =
            std::mem::take(&mut events).into_iter().partition(|event| {
                !matches!(
                    &event.payload,
                    EventPayload::UnitDied { .. } | EventPayload::BuildingDestroyed { .. }
                )
            });
        events.extend(rest);
        // The fight is left once nothing is in flight: on the towers' tick
        // when it drained already, and otherwise on the tick the last
        // projectile lands, the buffs standing still until then.
        let leaves_now = self.ready_to_finish();
        if publish_late_building_events {
            events.extend(torn_down);
            if leaves_now {
                self.clear_buffs_as_the_fight_ends(&mut events)?;
                self.clear_terrains_as_the_fight_ends()?;
                self.end_stealth_as_the_fight_ends();
                self.end_siege_as_the_fight_ends()?;
            }
        }
        // `BuffManager.Clear` takes a dying unit's buffs as it dies, whatever
        // killed it.
        for end in ends {
            let (precedes, follows) = self.around_an_end(&end);
            events.extend(precedes);
            events.push(end);
            events.extend(follows);
        }
        // What the tick's hits killed and felled comes last, in the order they
        // struck: a block a shot fells reads between the deaths its splash
        // caused, and after every removal the tick resolved. A fallen tower's
        // buff follows it, as a dead unit's cleared buffs follow its death.
        for fallen in std::mem::take(&mut self.fallen_buildings) {
            let (precedes, follows) = self.around_an_end(&fallen);
            events.extend(precedes);
            events.push(fallen);
            events.extend(follows);
        }
        self.buffs.dropped.clear();
        // A fight a projectile's drain finished leaves on this tick, with no
        // tower torn down to publish first: its buffs are cleared after
        // everything else the tick did.
        if !(publish_late_building_events && leaves_now) && self.ready_to_finish() {
            self.clear_buffs_as_the_fight_ends(&mut events)?;
            self.clear_terrains_as_the_fight_ends()?;
            self.end_stealth_as_the_fight_ends();
            self.end_siege_as_the_fight_ends()?;
        }
        if !self.buffs.tower_events.is_empty() {
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
            .filter(|(actor_id, actor)| !actor.alive() && !self.returned_dead.contains(actor_id))
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
        // So does a building that fell: a wall's block stays in the tree no
        // more than a dead unit does.
        let fallen = self
            .buildings
            .iter()
            .filter(|building| !building_alive(building))
            .map(|building| (building.team_id, building.building_id))
            .collect::<Vec<_>>();
        for (team, building_id) in fallen {
            if let Some(tree) = self.target_quadtrees.get_mut(&team) {
                tree.remove(FightActorRef::Building(building_id));
            }
        }
        events.append(&mut self.shield.created);
        events.append(&mut self.shield.destroyed);
        events.extend(self.take_terrain_events());
        Ok(TransitionEvents { events })
    }

    /// The events around a unit's death or a building's fall, before it and
    /// after it. A dead unit's buffs, cleared, follow its death; a fallen
    /// construction's precede its fall, and a fallen tower's buff on its side
    /// follows it.
    fn around_an_end(&mut self, end: &Event) -> (Vec<Event>, Vec<Event>) {
        match (end.subject, &end.payload) {
            // What a buff made a dying unit summon is made as it dies, before
            // its buffs are cleared; what its technology made it summon is
            // recorded before it dies.
            (Some(subject), EventPayload::UnitDied { .. }) if subject.kind == ObjectKind::Unit => {
                let (precedes, mut follows) = self
                    .support
                    .summoned_events
                    .remove(&subject.id)
                    .unwrap_or_default();
                follows.extend(self.buffs_cleared_by_death(subject.id));
                (precedes, follows)
            }
            (Some(subject), EventPayload::BuildingDestroyed { .. })
                if subject.kind == ObjectKind::Building =>
            {
                (
                    self.construction_buffs_cleared(subject.id),
                    self.buffs
                        .tower_events
                        .remove(&subject.id)
                        .unwrap_or_default(),
                )
            }
            _ => (Vec::new(), Vec::new()),
        }
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
                    query_visible: actor.target_query_visibility == Visibility::Normal,
                    query_visibility: actor.target_query_visibility,
                    targetable: actor.alive(),
                    query_targetable: actor.target_query_alive,
                    visible: actor.visibility == Visibility::Normal,
                    visibility: actor.visibility,
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
                    query_alive: self.buildings_query_alive.contains(&id),
                    query_visible: true,
                    query_visibility: Visibility::Normal,
                    visible: true,
                    visibility: Visibility::Normal,
                    targetable: building.targetable && building.available,
                    query_targetable: self.buildings_query_alive.contains(&id)
                        && building.available,
                    domain: UnitDomain::Ground,
                })
            }
        }
    }

    fn fight_actor_is_alive(&self, reference: FightActorRef) -> bool {
        self.fight_actor(reference)
            .is_some_and(|target| target.alive && target.targetable)
    }

    /// A side with a summon still appearing has not lost:
    /// `FightCoreSystem.TryDstroyTower` passes over a team that
    /// `HaveProcessingMech`.
    /// Whether a side has a unit left in the fight, or one still to appear.
    fn standing(&self, team: u32) -> bool {
        self.actors
            .values()
            .any(|actor| actor.placement.team == team && actor.alive())
            || self.appearing_on(team)
    }

    fn winner(&self) -> Option<u32> {
        let (blue, red) = (self.standing(0), self.standing(1));
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
        // `SupportUnitSystem.IsStepFinish` and `SummonSystem.IsStepFinish`:
        // a battle skill's creator still alive, or a summon still appearing,
        // holds the fight; a production line never does.
        living_teams.len() < 2
            && self.projectiles.is_empty()
            && self.support.creators.is_empty()
            && self.support.appearing.is_empty()
    }

    /// What happens between a tick's work and its snapshot: intervals settle
    /// as the fight finishes, and `BattleSystem.OnFightOver` prunes each
    /// formation's experience to a whole number and clears every skill's
    /// kills before the last state is read.
    fn close_tick(&mut self, out_of_time: bool) -> Result<()> {
        self.name_turned_formations();
        self.settle_intervals_if_finishing();
        if self.ready_to_finish() || out_of_time {
            self.prune_experience();
            self.clear_kills()?;
        }
        Ok(())
    }

    fn ready_to_finish(&self) -> bool {
        self.naturally_finished() && !self.ending.terminal_drain_pending
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

/// The buildings standing, by id.
pub(in crate::fight) fn standing_buildings(
    buildings: &[BuildingState],
) -> std::collections::BTreeSet<u64> {
    buildings
        .iter()
        .filter(|building| building_alive(building))
        .map(|building| building.building_id)
        .collect()
}

/// The `SkillStateController` state a recording names for a skill's: a
/// cooling has handed back to the idle state on its last step.
fn recorded_machine_state(
    state: &skill::SkillState,
    step: u64,
    cooling_steps: u64,
) -> SkillMachineState {
    match state {
        skill::SkillState::Cooling { started, .. }
            if step >= started.saturating_add(cooling_steps) =>
        {
            SkillMachineState::Idle
        }
        skill::SkillState::Idle { .. } => SkillMachineState::Idle,
        skill::SkillState::Prepare { .. } => SkillMachineState::Prepare,
        skill::SkillState::Attack(_) => SkillMachineState::Attack,
        skill::SkillState::Cooling { .. } => SkillMachineState::Cooling,
        skill::SkillState::Reloading { .. } => SkillMachineState::Reloading,
        skill::SkillState::Locked => SkillMachineState::Lock,
    }
}

/// The `SkillAttackController` phase a recording names for a skill: the wait
/// for its attack point, whose last update the attacking controller takes
/// over, the release of a burst or a sweep still under way, and its
/// backswing.
fn recorded_attack_phase(skill: &Skill, step: u64) -> Option<AttackPhase> {
    match skill.state {
        skill::SkillState::Attack(skill::Blow::Before(pending)) => {
            Some(if step.saturating_add(1) >= pending.step {
                AttackPhase::Attacking
            } else {
                AttackPhase::Before
            })
        }
        // The backswing's controller has handed back on its last step.
        skill::SkillState::Attack(skill::Blow::After { finish_step }) => {
            (step < finish_step).then_some(AttackPhase::After)
        }
        skill::SkillState::Attack(skill::Blow::Waiting) => match &skill.performer {
            skill::Performer::Projectile { pending, .. } if !pending.is_empty() => {
                Some(AttackPhase::Attacking)
            }
            // A sweep is released stretch by stretch until its last.
            skill::Performer::Sweep(sweep) if !sweep.over() => Some(AttackPhase::Attacking),
            _ => None,
        },
        _ => None,
    }
}
