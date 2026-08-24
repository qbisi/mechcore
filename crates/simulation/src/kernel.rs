use std::{cmp::Ordering, collections::BTreeMap};

use mechcore_mcfr::{
    BuildingState, Domain, DurableContext, Event, EventPayload, Gauge, Hashes, IdentityAllocator,
    IdentityContract, MCFR_SCHEMA_VERSION, McfrWriter, MotionState, NumericConvention, ObjectKind,
    ObjectRef, PersonalShieldState, Pose, ProjectileState, Rational, TransitionEvents, UnitState,
    Vec3, Visibility, WorldSnapshot,
};
use serde::Serialize;

use crate::{
    Error, Result,
    layout::{CompiledLayout, Placement},
    random::GrRandom,
    rules::{
        SimulationConfig, TargetDomain, TrainingGroundConfig, UnitConfig, UnitConfigs, UnitDomain,
    },
};

const SPACE_UNITS_PER_METER: i64 = 1_000;
const TIME_UNITS_PER_SECOND: u64 = 2_000;
const LOGIC_TICK_TIME_UNITS: u64 = 100;
const FIGHT_TIME_SECONDS: u64 = 120;
const FORMATION_JITTER_RANGE_TENTHS: i32 = 8;
const Q32_ONE: i64 = 1_i64 << 32;
const C0_1_RAW: i64 = 0x1999_9999;
const NATIVE_LOGIC_DELTA_Q32: i64 = 0x0CCC_CCCC;

#[derive(Debug, Clone, Copy)]
struct PendingRelease {
    step: u64,
    target: u64,
}

#[derive(Debug, Clone)]
struct Actor {
    placement: Placement,
    rules: UnitConfig,
    x: i64,
    z: i64,
    x_q32: i64,
    z_q32: i64,
    current_velocity_x_q32: i64,
    current_velocity_z_q32: i64,
    next_target_x_q32: i64,
    next_target_z_q32: i64,
    next_speed_q32: i64,
    solver_target_x_q32: i64,
    solver_target_z_q32: i64,
    solver_speed_q32: i64,
    published_target_x_q32: i64,
    published_target_z_q32: i64,
    published_speed_q32: i64,
    body_rotation: i64,
    aim_rotation: i64,
    life: i64,
    motion: MotionState,
    next_attack_step: u64,
    pending: Option<PendingRelease>,
}

impl Actor {
    fn new(placement: Placement, rules: UnitConfig, seed: i32) -> Self {
        let mut layout_random = GrRandom::new(u64::from(seed.cast_unsigned()));
        let jitter_x_tenths = i64::from(layout_random.next_in_range(FORMATION_JITTER_RANGE_TENTHS));
        let jitter_z_tenths = i64::from(layout_random.next_in_range(FORMATION_JITTER_RANGE_TENTHS));
        let direction = if placement.team == 0 { 1 } else { -1 };
        let max_life = rules.max_life;
        let x_q32 = placement
            .world_x
            .saturating_mul(Q32_ONE)
            .saturating_add(jitter_x_tenths.saturating_mul(C0_1_RAW) * direction);
        let z_q32 = placement
            .world_z
            .saturating_mul(Q32_ONE)
            .saturating_add(jitter_z_tenths.saturating_mul(C0_1_RAW) * direction);
        let x = q32_to_space_rounded(x_q32);
        let z = q32_to_space_rounded(z_q32);
        Self {
            x,
            z,
            x_q32,
            z_q32,
            body_rotation: placement.rotation,
            aim_rotation: placement.rotation,
            placement,
            rules,
            current_velocity_x_q32: 0,
            current_velocity_z_q32: 0,
            next_target_x_q32: x_q32,
            next_target_z_q32: z_q32,
            next_speed_q32: 0,
            solver_target_x_q32: x_q32,
            solver_target_z_q32: z_q32,
            solver_speed_q32: 0,
            published_target_x_q32: x_q32,
            published_target_z_q32: z_q32,
            published_speed_q32: 0,
            life: max_life,
            motion: MotionState::Idle,
            next_attack_step: 0,
            pending: None,
        }
    }

    fn alive(&self) -> bool {
        self.life > 0
    }

    fn object_ref(&self) -> ObjectRef {
        ObjectRef::new(ObjectKind::Unit, self.placement.unit_id)
    }

    fn snapshot(&self) -> UnitState {
        UnitState {
            unit_id: self.placement.unit_id,
            team_id: self.placement.team,
            formation_id: self.placement.formation_id,
            unit_type_id: self.rules.unit_type_id,
            domain: match self.rules.domain {
                UnitDomain::Ground => Domain::Ground,
                UnitDomain::Air => Domain::Air,
            },
            position: point(self.x, self.z),
            body_rotation: self.body_rotation,
            aim_pose: Pose {
                position: point(self.x, self.z),
                rotation: self.aim_rotation,
            },
            velocity: point(
                q32_to_space_rounded(self.current_velocity_x_q32),
                q32_to_space_rounded(self.current_velocity_z_q32),
            ),
            motion_state: self.motion,
            collision_radius: self.rules.collision_radius(),
            life: self.life,
            max_life: self.rules.max_life,
            alive: self.alive(),
            active: true,
            targetable: self.alive(),
            visibility: Visibility::Normal,
            personal_shield: PersonalShieldState {
                active: false,
                enabled: true,
                energy: 0,
                max_energy: 0,
            },
        }
    }
}

#[derive(Debug, Clone)]
struct Projectile {
    id: u64,
    team: u32,
    owner: u64,
    target: u64,
    x: i64,
    z: i64,
    x_q32: i64,
    z_q32: i64,
    cached_target_x: i64,
    cached_target_z: i64,
    cached_target_x_q32: i64,
    cached_target_z_q32: i64,
    cached_target_radius: i64,
    speed: i64,
    damage: i64,
}

impl Projectile {
    fn object_ref(&self) -> ObjectRef {
        ObjectRef::new(ObjectKind::Projectile, self.id)
    }

    fn snapshot(&self) -> ProjectileState {
        ProjectileState {
            projectile_id: self.id,
            team_id: self.team,
            owner: Some(ObjectRef::new(ObjectKind::Unit, self.owner)),
            position: point(self.x, self.z),
            orientation: 0,
            target: Some(ObjectRef::new(ObjectKind::Unit, self.target)),
            cached_target_position: point(self.cached_target_x, self.cached_target_z),
            cached_target_radius: self.cached_target_radius,
            released: false,
            life: Gauge {
                current: 1,
                maximum: 1,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TeamResult {
    pub team: &'static str,
    pub unit: String,
    pub alive: bool,
    pub remaining_life: i64,
    pub max_life: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SimulationResult {
    pub schema: &'static str,
    pub game_build: String,
    pub seed: i32,
    pub seed_source: &'static str,
    pub output: String,
    pub end_reason: &'static str,
    pub steps: u64,
    pub elapsed_milliseconds: u64,
    pub winner: Option<&'static str>,
    pub draw: bool,
    pub teams: Vec<TeamResult>,
    pub hashes: Hashes,
}

struct Simulation {
    actors: BTreeMap<u64, Actor>,
    team_random: BTreeMap<u32, GrRandom>,
    projectiles: Vec<Projectile>,
    buildings: Vec<BuildingState>,
    identities: IdentityAllocator,
    rvo_counter: u8,
}

impl Simulation {
    fn new(
        layout: &CompiledLayout,
        configs: &UnitConfigs,
        training_ground: &TrainingGroundConfig,
        seed: i32,
    ) -> Result<Self> {
        let mut actors = BTreeMap::new();
        for placement in &layout.placements {
            let rules = configs.get(placement.type_name.as_str()).ok_or_else(|| {
                Error::new(format!(
                    "unit type {:?} has no configuration",
                    placement.type_name
                ))
            })?;
            actors.insert(
                placement.unit_id,
                Actor::new(placement.clone(), rules.clone(), seed),
            );
        }
        if actors.len() == 2 {
            let mut pair = actors.iter();
            let (&first_id, first) = pair.next().expect("two actors contain a first actor");
            let (&second_id, second) = pair.next().expect("two actors contain a second actor");
            if first.placement.team != second.placement.team
                && accepts_target(first.rules.attack.target_domain, second.rules.domain)
                && accepts_target(second.rules.attack.target_domain, first.rules.domain)
            {
                let first_rotation = direction_mdeg(second.x - first.x, second.z - first.z);
                let second_rotation = direction_mdeg(first.x - second.x, first.z - second.z);
                for (actor_id, rotation) in
                    [(first_id, first_rotation), (second_id, second_rotation)]
                {
                    let actor = actors
                        .get_mut(&actor_id)
                        .expect("initial actor identity is stable");
                    actor.body_rotation = rotation;
                    actor.aim_rotation = rotation;
                }
            }
        }
        let mut team_random = BTreeMap::new();
        for actor in actors.values() {
            let random = team_random.entry(actor.placement.team).or_insert_with(|| {
                GrRandom::new(u64::from(
                    layout
                        .round
                        .cast_signed()
                        .wrapping_add(actor.placement.team.cast_signed())
                        .wrapping_mul(4_444)
                        .cast_unsigned(),
                ))
            });
            let offset_steps =
                native_time_units_to_steps(actor.rules.attack.interval_offset_time_units());
            if offset_steps > 0 {
                let _initial_sample =
                    random.next_in_range(i32::try_from(offset_steps).unwrap_or(i32::MAX));
            }
        }
        let buildings = training_ground
            .buildings
            .iter()
            .enumerate()
            .map(|(index, building)| {
                let building_id = u64::try_from(index)
                    .map_err(|_| Error::new("training-ground building index overflow"))?
                    .saturating_add(1);
                let radius = building.radius();
                Ok(BuildingState {
                    building_id,
                    team_id: building.team_id,
                    building_type_id: building.building_type_id,
                    position: point(building.x(), building.z()),
                    rotation: 0,
                    bounds_width: radius.saturating_mul(2),
                    bounds_height: radius.saturating_mul(2),
                    life: building.life,
                    max_life: building.life,
                    alive: building.life > 0,
                    destroyed: false,
                    available: true,
                    targetable: building.life > 0,
                    collision_enabled: building.collision_enabled,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            actors,
            team_random,
            projectiles: Vec::new(),
            buildings,
            identities: IdentityAllocator::new(),
            rvo_counter: 0,
        })
    }

    fn snapshot(&self) -> WorldSnapshot {
        WorldSnapshot {
            units: self.actors.values().map(Actor::snapshot).collect(),
            projectiles: self.projectiles.iter().map(Projectile::snapshot).collect(),
            buildings: self.buildings.clone(),
            ..WorldSnapshot::default()
        }
    }

    fn step(&mut self, step: u64) -> Result<TransitionEvents> {
        let mut events = Vec::new();
        let actor_ids = self.actors.keys().copied().collect::<Vec<_>>();
        for actor_id in actor_ids {
            self.step_actor(actor_id, step, &mut events)?;
            self.step_actor_rvo_position(actor_id);
        }
        self.step_projectiles(&mut events)?;
        self.step_rvo();
        if self.naturally_finished() {
            for actor in self.actors.values_mut() {
                actor.motion = MotionState::Idle;
            }
        }
        Ok(TransitionEvents { events })
    }

    #[allow(clippy::too_many_lines)]
    fn step_actor(&mut self, actor_id: u64, step: u64, events: &mut Vec<Event>) -> Result<()> {
        if !self.actors[&actor_id].alive() {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.pending = None;
            actor.motion = MotionState::Idle;
            return Ok(());
        }
        if self.actors[&actor_id]
            .pending
            .is_some_and(|pending| pending.step <= step)
        {
            self.release(actor_id, events)?;
        }
        let target_id = self.actors.iter().find_map(|(&id, actor)| {
            (id != actor_id
                && actor.alive()
                && accepts_target(
                    self.actors[&actor_id].rules.attack.target_domain,
                    actor.rules.domain,
                ))
            .then_some(id)
        });
        let Some(target_id) = target_id else {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            actor.motion = MotionState::Idle;
            return Ok(());
        };
        let target = &self.actors[&target_id];
        let target_x = target.x;
        let target_z = target.z;
        let target_x_q32 = target.x_q32;
        let target_z_q32 = target.z_q32;
        let target_radius = target.rules.collision_radius();
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let dx = target_x - actor.x;
        let dz = target_z - actor.z;
        let target_rotation = direction_mdeg_q32_raw(
            target_x_q32.saturating_sub(actor.x_q32),
            target_z_q32.saturating_sub(actor.z_q32),
        );
        let center_distance = magnitude(dx, dz);
        let edge_distance = center_distance
            .saturating_sub(actor.rules.collision_radius())
            .saturating_sub(target_radius);
        if edge_distance <= actor.rules.attack.range() {
            let previous_motion = actor.motion;
            let entered_attack = actor.motion != MotionState::Attacking;
            actor.motion = MotionState::Attacking;
            if previous_motion == MotionState::Moving {
                actor.next_target_x_q32 = actor.x_q32;
                actor.next_target_z_q32 = actor.z_q32;
                actor.next_speed_q32 = 0;
            }
            if entered_attack {
                return Ok(());
            }
            actor.aim_rotation = target_rotation;
            if actor.current_velocity_x_q32 != 0 || actor.current_velocity_z_q32 != 0 {
                actor.body_rotation = rotate_towards(
                    actor.body_rotation,
                    direction_mdeg_q32_raw(
                        actor.current_velocity_x_q32,
                        actor.current_velocity_z_q32,
                    ),
                    rotation_per_tick(actor),
                );
            }
            if !entered_attack && actor.pending.is_none() && step >= actor.next_attack_step {
                let interval_steps =
                    native_time_units_to_steps(actor.rules.attack.interval_time_units());
                let offset_steps =
                    native_time_units_to_steps(actor.rules.attack.interval_offset_time_units());
                let sample = if offset_steps == 0 {
                    0
                } else {
                    i64::from(
                        self.team_random
                            .get_mut(&actor.placement.team)
                            .expect("every actor team owns one attack random stream")
                            .next_in_range(i32::try_from(offset_steps).unwrap_or(i32::MAX)),
                    )
                };
                let sampled = i64::try_from(interval_steps)
                    .unwrap_or(i64::MAX)
                    .saturating_add(sample)
                    .max(1)
                    .cast_unsigned();
                actor.next_attack_step = step.saturating_add(sampled);
                actor.pending = Some(PendingRelease {
                    step: step.saturating_add(native_time_units_to_steps(
                        actor.rules.attack.release_delay_time_units(),
                    )),
                    target: target_id,
                });
                if actor.pending.is_some_and(|pending| pending.step == step) {
                    self.release(actor_id, events)?;
                }
            }
            return Ok(());
        }
        actor.aim_rotation = target_rotation;
        let entered_move = actor.motion == MotionState::Idle;
        actor.motion = MotionState::Moving;
        if entered_move {
            return Ok(());
        }
        actor.next_target_x_q32 = target_x_q32;
        actor.next_target_z_q32 = target_z_q32;
        actor.next_speed_q32 = space_to_q32(actor.rules.move_speed());
        if actor.current_velocity_x_q32 != 0 || actor.current_velocity_z_q32 != 0 {
            actor.body_rotation = rotate_towards(
                actor.body_rotation,
                direction_mdeg_q32_raw(actor.current_velocity_x_q32, actor.current_velocity_z_q32),
                rotation_per_tick(actor),
            );
        }
        Ok(())
    }

    fn step_actor_rvo_position(&mut self, actor_id: u64) {
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        if !actor.alive() || actor.motion == MotionState::Idle || actor.published_speed_q32 == 0 {
            return;
        }
        let maximum_delta_q32 = q32_mul(actor.published_speed_q32, NATIVE_LOGIC_DELTA_Q32);
        let (movement_x_q32, movement_z_q32) = clamp_magnitude_q32_raw(
            actor.published_target_x_q32.saturating_sub(actor.x_q32),
            actor.published_target_z_q32.saturating_sub(actor.z_q32),
            maximum_delta_q32,
        );
        actor.x_q32 = actor.x_q32.saturating_add(movement_x_q32);
        actor.z_q32 = actor.z_q32.saturating_add(movement_z_q32);
        actor.x = q32_to_space_rounded(actor.x_q32);
        actor.z = q32_to_space_rounded(actor.z_q32);
    }

    fn step_rvo(&mut self) {
        self.rvo_counter += 1;
        if self.rvo_counter < 4 {
            return;
        }
        self.rvo_counter = 0;
        for actor in self
            .actors
            .values_mut()
            .filter(|actor| actor.alive() && actor.motion != MotionState::Idle)
        {
            actor.published_target_x_q32 = actor.solver_target_x_q32;
            actor.published_target_z_q32 = actor.solver_target_z_q32;
            actor.published_speed_q32 = actor.solver_speed_q32;
            (actor.current_velocity_x_q32, actor.current_velocity_z_q32) =
                normalized_velocity_q32_raw(
                    actor.published_target_x_q32.saturating_sub(actor.x_q32),
                    actor.published_target_z_q32.saturating_sub(actor.z_q32),
                    actor.published_speed_q32,
                );
            actor.solver_target_x_q32 = actor.next_target_x_q32;
            actor.solver_target_z_q32 = actor.next_target_z_q32;
            actor.solver_speed_q32 = actor.next_speed_q32;
        }
    }

    fn release(&mut self, actor_id: u64, events: &mut Vec<Event>) -> Result<()> {
        let pending = self.actors[&actor_id]
            .pending
            .ok_or_else(|| Error::new("attack release has no pending action"))?;
        let target = &self.actors[&pending.target];
        let target_x = target.x;
        let target_z = target.z;
        let target_x_q32 = target.x_q32;
        let target_z_q32 = target.z_q32;
        let target_radius = target.rules.collision_radius();
        let projectile_id = self.identities.allocate_object(ObjectKind::Projectile)?.id;
        let owner = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        owner.pending = None;
        let projectile = Projectile {
            id: projectile_id,
            team: owner.placement.team,
            owner: actor_id,
            target: pending.target,
            x: owner.x,
            z: owner.z,
            x_q32: owner.x_q32,
            z_q32: owner.z_q32,
            cached_target_x: target_x,
            cached_target_z: target_z,
            cached_target_x_q32: target_x_q32,
            cached_target_z_q32: target_z_q32,
            cached_target_radius: target_radius,
            speed: owner.rules.attack.projectile_speed(),
            damage: owner.rules.attack.damage,
        };
        let projectile_ref = projectile.object_ref();
        events.push(event(
            Some(projectile_ref),
            Some(owner.object_ref()),
            Some(ObjectRef::new(ObjectKind::Unit, pending.target)),
            EventPayload::ProjectileReleased,
        ));
        self.projectiles.push(projectile);
        Ok(())
    }

    #[allow(clippy::similar_names)] // Paired fixed-point x/z components are intentionally parallel.
    fn step_projectiles(&mut self, events: &mut Vec<Event>) -> Result<()> {
        let mut retained = Vec::with_capacity(self.projectiles.len());
        for mut projectile in std::mem::take(&mut self.projectiles) {
            if let Some(target) = self
                .actors
                .get(&projectile.target)
                .filter(|actor| actor.alive())
            {
                projectile.cached_target_x = target.x;
                projectile.cached_target_z = target.z;
                projectile.cached_target_x_q32 = target.x_q32;
                projectile.cached_target_z_q32 = target.z_q32;
                projectile.cached_target_radius = target.rules.collision_radius();
            }
            let dx_q32 = projectile
                .cached_target_x_q32
                .saturating_sub(projectile.x_q32);
            let dz_q32 = projectile
                .cached_target_z_q32
                .saturating_sub(projectile.z_q32);
            let distance_q32 = native_q32_magnitude(dx_q32, dz_q32);
            if distance_q32 < space_to_q32(projectile.cached_target_radius) {
                self.impact(&projectile, events)?;
            } else {
                let step_q32 = q32_mul(
                    space_to_q32(projectile.speed),
                    Q32_ONE.saturating_mul(LOGIC_TICK_TIME_UNITS.cast_signed())
                        / TIME_UNITS_PER_SECOND.cast_signed(),
                );
                if distance_q32 > 0 {
                    let move_q32 = step_q32.min(distance_q32);
                    let reciprocal = q32_div(Q32_ONE, distance_q32);
                    projectile.x_q32 = projectile
                        .x_q32
                        .saturating_add(q32_mul(q32_mul(dx_q32, reciprocal), move_q32));
                    projectile.z_q32 = projectile
                        .z_q32
                        .saturating_add(q32_mul(q32_mul(dz_q32, reciprocal), move_q32));
                }
                projectile.x = q32_to_space_rounded(projectile.x_q32);
                projectile.z = q32_to_space_rounded(projectile.z_q32);
                retained.push(projectile);
            }
        }
        self.projectiles = retained;
        Ok(())
    }

    fn impact(&mut self, projectile: &Projectile, events: &mut Vec<Event>) -> Result<()> {
        let projectile_ref = projectile.object_ref();
        let owner_ref = ObjectRef::new(ObjectKind::Unit, projectile.owner);
        let target_ref = ObjectRef::new(ObjectKind::Unit, projectile.target);
        let target = self
            .actors
            .get_mut(&projectile.target)
            .ok_or_else(|| Error::new("projectile target is absent"))?;
        if target.alive() {
            let previous_life = target.life;
            target.life = target.life.saturating_sub(projectile.damage).max(0);
            events.push(event(
                None,
                Some(projectile_ref),
                Some(target_ref),
                EventPayload::Damage {
                    amount: previous_life - target.life,
                },
            ));
            if target.life == 0 {
                target.motion = MotionState::Idle;
                target.pending = None;
            }
        }
        events.push(event(
            Some(projectile_ref),
            Some(owner_ref),
            Some(target_ref),
            EventPayload::ProjectileRemoved {
                position: point(projectile.x, projectile.z),
                intercepted: false,
            },
        ));
        Ok(())
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
}

pub(crate) fn run(
    layout: &CompiledLayout,
    config: &SimulationConfig,
    seed: i32,
    seed_source: &'static str,
    output: &std::path::Path,
) -> Result<SimulationResult> {
    let divisor = gcd(LOGIC_TICK_TIME_UNITS, TIME_UNITS_PER_SECOND);
    let context = DurableContext {
        schema_version: MCFR_SCHEMA_VERSION,
        game_build: config.game_build.clone(),
        logic_step: Rational {
            numerator: LOGIC_TICK_TIME_UNITS / divisor,
            denominator: TIME_UNITS_PER_SECOND / divisor,
        },
        numeric_convention: NumericConvention {
            distance_units_per_meter: SPACE_UNITS_PER_METER.cast_unsigned(),
            rotation_units_per_degree: 1_000,
            time_units_per_second: TIME_UNITS_PER_SECOND,
        },
        combat_round: layout.round,
        match_seed: seed,
        identity_contract: IdentityContract::TeamZxSequentialV1,
    };
    let mut simulation = Simulation::new(layout, &config.units, &config.training_ground, seed)?;
    let mut writer = McfrWriter::create(output, &context)?;
    writer.append_tick(
        simulation.snapshot(),
        &TransitionEvents { events: Vec::new() },
    )?;
    let mut steps = 0;
    let max_steps = FIGHT_TIME_SECONDS
        .saturating_mul(TIME_UNITS_PER_SECOND)
        .div_ceil(LOGIC_TICK_TIME_UNITS);
    let end_reason = loop {
        if steps >= max_steps {
            break "forced_time_limit";
        }
        let events = simulation.step(steps)?;
        steps += 1;
        writer.append_tick(simulation.snapshot(), &events)?;
        if simulation.naturally_finished() {
            break "natural_module_drain";
        }
    };
    let hashes = writer.finish()?;
    let verified = mechcore_mcfr::McfrReader::open_verified(output)?;
    if verified.hashes() != &hashes {
        return Err(Error::new(
            "published MCFR hashes changed during verification",
        ));
    }
    let winner = simulation.winner().map(team_name);
    Ok(SimulationResult {
        schema: "mechcore.simulation-result.v1",
        game_build: config.game_build.clone(),
        seed,
        seed_source,
        output: output.display().to_string(),
        end_reason,
        steps,
        elapsed_milliseconds: steps
            .saturating_mul(LOGIC_TICK_TIME_UNITS)
            .saturating_mul(1_000)
            / TIME_UNITS_PER_SECOND,
        winner,
        draw: winner.is_none(),
        teams: simulation
            .actors
            .values()
            .map(|actor| TeamResult {
                team: team_name(actor.placement.team),
                unit: actor.placement.type_name.clone(),
                alive: actor.alive(),
                remaining_life: actor.life,
                max_life: actor.rules.max_life,
            })
            .collect(),
        hashes,
    })
}

const fn gcd(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

fn native_time_units_to_steps(time_units: u64) -> u64 {
    let raw_time =
        i64::try_from((u128::from(time_units) << 32) / u128::from(TIME_UNITS_PER_SECOND))
            .unwrap_or(i64::MAX);
    q32_div(raw_time, NATIVE_LOGIC_DELTA_Q32)
        .max(0)
        .cast_unsigned()
        >> 32
}

fn team_name(team: u32) -> &'static str {
    if team == 0 { "blue" } else { "red" }
}

const fn accepts_target(target_domain: TargetDomain, unit_domain: UnitDomain) -> bool {
    matches!(
        (target_domain, unit_domain),
        (TargetDomain::Both, _)
            | (TargetDomain::Ground, UnitDomain::Ground)
            | (TargetDomain::Air, UnitDomain::Air)
    )
}

fn rotation_per_tick(actor: &Actor) -> i64 {
    actor
        .rules
        .rotate_speed_mdeg_per_second()
        .saturating_mul(i64::try_from(LOGIC_TICK_TIME_UNITS).unwrap_or(i64::MAX))
        / i64::try_from(TIME_UNITS_PER_SECOND).unwrap_or(i64::MAX)
}

fn rotate_towards(current: i64, target: i64, maximum: i64) -> i64 {
    let current = current.rem_euclid(360_000);
    let target = target.rem_euclid(360_000);
    let delta = (target - current + 540_000).rem_euclid(360_000) - 180_000;
    (current + delta.clamp(-maximum, maximum)).rem_euclid(360_000)
}

fn event(
    subject: Option<ObjectRef>,
    source: Option<ObjectRef>,
    target: Option<ObjectRef>,
    payload: EventPayload,
) -> Event {
    Event {
        subject,
        source,
        target,
        payload,
    }
}

const fn point(x: i64, z: i64) -> Vec3 {
    Vec3 { x, y: 0, z }
}

fn magnitude(x: i64, z: i64) -> i64 {
    integer_sqrt(i128::from(x) * i128::from(x) + i128::from(z) * i128::from(z))
}

fn space_to_q32(value: i64) -> i64 {
    i64::try_from(i128::from(value) * i128::from(Q32_ONE) / i128::from(SPACE_UNITS_PER_METER))
        .unwrap_or(if value < 0 { i64::MIN } else { i64::MAX })
}

fn q32_to_space_rounded(value: i64) -> i64 {
    let scaled = i128::from(value) * i128::from(SPACE_UNITS_PER_METER);
    let rounded = if scaled >= 0 {
        (scaled + i128::from(Q32_ONE / 2)) >> 32
    } else {
        -((-scaled + i128::from(Q32_ONE / 2)) >> 32)
    };
    i64::try_from(rounded).unwrap_or(if rounded < 0 { i64::MIN } else { i64::MAX })
}

fn q32_mul(left: i64, right: i64) -> i64 {
    i64::try_from((i128::from(left) * i128::from(right)) >> 32).unwrap_or({
        if (left < 0) == (right < 0) {
            i64::MAX
        } else {
            i64::MIN
        }
    })
}

fn q32_div(numerator: i64, denominator: i64) -> i64 {
    if denominator == 0 {
        return if numerator < 0 { i64::MIN } else { i64::MAX };
    }
    let scaled = u128::from(numerator.unsigned_abs()) << 32;
    let divisor = u128::from(denominator.unsigned_abs());
    let quotient = scaled / divisor;
    let remainder = scaled % divisor;
    let rounded = quotient.saturating_add(u128::from(remainder.saturating_mul(2) >= divisor));
    if (numerator < 0) == (denominator < 0) {
        i64::try_from(rounded).unwrap_or(i64::MAX)
    } else {
        i64::try_from(rounded)
            .ok()
            .and_then(i64::checked_neg)
            .unwrap_or(i64::MIN)
    }
}

fn native_q32_magnitude(x: i64, z: i64) -> i64 {
    fpcs_sqrt_fastest(q32_mul(x, x).saturating_add(q32_mul(z, z)))
}

fn normalized_velocity_q32_raw(dx: i64, dz: i64, speed: i64) -> (i64, i64) {
    let magnitude = native_q32_magnitude(dx, dz);
    if magnitude <= 0 {
        return (0, 0);
    }

    let inverse_magnitude = q32_div(Q32_ONE, magnitude);
    (
        q32_mul(q32_mul(dx, inverse_magnitude), speed),
        q32_mul(q32_mul(dz, inverse_magnitude), speed),
    )
}

fn clamp_magnitude_q32_raw(dx: i64, dz: i64, maximum: i64) -> (i64, i64) {
    let magnitude = native_q32_magnitude(dx, dz);
    if magnitude <= maximum {
        return (dx, dz);
    }
    if magnitude <= 0 || maximum <= 0 {
        return (0, 0);
    }

    let inverse_magnitude = q32_div(Q32_ONE, magnitude);
    (
        q32_mul(q32_mul(dx, inverse_magnitude), maximum),
        q32_mul(q32_mul(dz, inverse_magnitude), maximum),
    )
}

fn fpcs_sqrt_fastest(value: i64) -> i64 {
    if value <= 0 {
        return 0;
    }
    let exponent = 31 - i32::try_from(value.leading_zeros()).unwrap_or(64);
    let normalized = if exponent >= 0 {
        value >> exponent
    } else {
        value.wrapping_shl(exponent.unsigned_abs())
    };
    let mut variable = (i64::from_ne_bytes(0xC000_0000_0000_0000_u64.to_ne_bytes())
        .wrapping_add(normalized.wrapping_shl(30)))
        >> 32;
    let coefficient = variable;
    variable = variable.wrapping_mul(0x0664_5730);
    variable =
        (i64::from_ne_bytes(0xF90F_54C4_0000_0000_u64.to_ne_bytes()).wrapping_add(variable)) >> 32;
    let coefficient = coefficient.wrapping_shl(2);
    variable = variable.wrapping_mul(coefficient);
    variable = (0x1FDA_0F0B_0000_0000_i64.wrapping_add(variable)) >> 32;
    variable = coefficient.wrapping_mul(variable);
    variable = (0x4000_0000_0000_0000_i64.wrapping_add(variable)) >> 32;
    let odd_factor = if exponent & 1 == 0 {
        Q32_ONE
    } else {
        0x0001_6A09_E664
    };
    let mut result = odd_factor.wrapping_mul(variable) >> 30;
    result &= !3;
    let half_exponent = exponent >> 1;
    if half_exponent >= 0 {
        result.wrapping_shl(half_exponent.cast_unsigned())
    } else {
        result >> half_exponent.unsigned_abs()
    }
}

fn q32_exponent(value: i64) -> i32 {
    debug_assert!(value > 0);
    31 - i32::try_from(value.leading_zeros()).unwrap_or(64)
}

fn normalize_q32(value: i64, exponent: i32) -> i64 {
    if exponent >= 0 {
        value >> exponent
    } else {
        value.wrapping_shl(exponent.unsigned_abs())
    }
}

fn fpcs_atan2_div_fastest(y: i64, x: i64) -> i32 {
    debug_assert!(y >= 0 && x > 0 && y <= x);
    let exponent = q32_exponent(x);
    let normalized_y = normalize_q32(y, exponent);
    let normalized_x = normalize_q32(x, exponent);
    let mut variable = (i64::from_ne_bytes(0xC000_0000_0000_0000_u64.to_ne_bytes())
        .wrapping_add(normalized_x.wrapping_shl(30)))
        >> 32;
    variable = variable.wrapping_mul(0x279B_5BB0);
    let coefficient = ((i64::from_ne_bytes(0xDD58_0FC7_0000_0000_u64.to_ne_bytes())
        .wrapping_add(variable))
        >> 32)
        .wrapping_mul(
            ((i64::from_ne_bytes(0xC000_0000_0000_0000_u64.to_ne_bytes())
                .wrapping_add(normalized_x.wrapping_shl(30)))
                >> 32)
                .wrapping_shl(2),
        );
    let polynomial = (0x37FD_4590_0000_0000_i64.wrapping_add(coefficient)) >> 32;
    let argument = ((i64::from_ne_bytes(0xC000_0000_0000_0000_u64.to_ne_bytes())
        .wrapping_add(normalized_x.wrapping_shl(30)))
        >> 32)
        .wrapping_shl(2);
    let polynomial = polynomial.wrapping_mul(argument);
    let polynomial = (i64::from_ne_bytes(0xC0C3_D3BF_0000_0000_u64.to_ne_bytes())
        .wrapping_add(polynomial))
        >> 32;
    let polynomial = polynomial.wrapping_mul(argument);
    let polynomial = (0x4000_0000_0000_0000_i64.wrapping_add(polynomial)) >> 32;
    let y_quarter = normalized_y >> 2;
    i32::try_from(polynomial.wrapping_mul(y_quarter) >> 30).unwrap_or(i32::MAX)
}

fn fpcs_atan_polynomial(divided: i32, sign_mask: i64) -> i64 {
    let variable = i64::from(divided);
    let mut polynomial = variable.wrapping_mul(0x2651_FC38);
    polynomial = (i64::from_ne_bytes(0xE8C5_3128_0000_0000_u64.to_ne_bytes())
        .wrapping_add(polynomial))
        >> 32;
    let argument = variable.wrapping_shl(2);
    polynomial = polynomial.wrapping_mul(argument);
    polynomial = (i64::from_ne_bytes(0xFFE4_A871_0000_0000_u64.to_ne_bytes())
        .wrapping_add(polynomial))
        >> 32;
    polynomial = polynomial.wrapping_mul(argument);
    polynomial = (0x4005_9E04_0000_0000_i64.wrapping_add(polynomial)) >> 32;
    polynomial = polynomial.wrapping_mul(argument) >> 30;
    (polynomial & !3) ^ sign_mask
}

fn fpcs_atan2_fastest(y: i64, x: i64) -> i64 {
    const PI_OVER_TWO: i64 = 0x1921_FB544;
    const PI: i64 = 0x3243_F6A89;
    if x == 0 {
        return match y.cmp(&0) {
            Ordering::Greater => PI_OVER_TWO,
            Ordering::Less => -PI_OVER_TWO,
            Ordering::Equal => 0,
        };
    }
    let absolute_x = x.saturating_abs();
    let absolute_y = y.saturating_abs();
    let sign_mask = (x ^ y) >> 63;
    if absolute_x < absolute_y {
        let divided = fpcs_atan2_div_fastest(absolute_x, absolute_y);
        let approximate = fpcs_atan_polynomial(divided, sign_mask);
        if y > 0 {
            PI_OVER_TWO.wrapping_sub(approximate)
        } else {
            (-PI_OVER_TWO).wrapping_sub(approximate)
        }
    } else {
        let divided = fpcs_atan2_div_fastest(absolute_y, absolute_x);
        let approximate = fpcs_atan_polynomial(divided, sign_mask);
        if x > 0 {
            approximate
        } else if y >= 0 {
            approximate.wrapping_add(PI)
        } else {
            approximate.wrapping_sub(PI)
        }
    }
}

fn fpcs_acos_fastest(value: i64) -> i64 {
    let complement = q32_mul(Q32_ONE.saturating_sub(value), Q32_ONE.saturating_add(value));
    fpcs_atan2_fastest(fpcs_sqrt_fastest(complement), value)
}

fn integer_sqrt(value: i128) -> i64 {
    if value <= 0 {
        return 0;
    }
    let value = u128::try_from(value).unwrap_or(u128::MAX);
    let mut low = 0_u128;
    let mut high = value.min(u128::from(u64::MAX));
    while low < high {
        let middle = (low + high).div_ceil(2);
        if middle <= value / middle {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    i64::try_from(low).unwrap_or(i64::MAX)
}

fn direction_mdeg(dx: i64, dz: i64) -> i64 {
    direction_mdeg_q32_raw(space_to_q32(dx), space_to_q32(dz))
}

fn direction_mdeg_q32_raw(dx: i64, dz: i64) -> i64 {
    if dx == 0 && dz == 0 {
        return 0;
    }
    let magnitude = native_q32_magnitude(dx, dz);
    if magnitude <= 0 {
        return 0;
    }
    let cosine = q32_div(dz, magnitude).clamp(-Q32_ONE, Q32_ONE);
    let radians = fpcs_acos_fastest(cosine);
    let degrees = q32_mul(radians, 0x0039_4BB8_34C8);
    let degrees = if dx < 0 {
        (360_i64 << 32).saturating_sub(degrees)
    } else {
        degrees
    };
    let scaled = i128::from(degrees) * 1_000;
    let rounded = if scaled >= 0 {
        (scaled + i128::from(Q32_ONE / 2)) >> 32
    } else {
        -((-scaled + i128::from(Q32_ONE / 2)) >> 32)
    };
    i64::try_from(rounded).unwrap_or(if rounded < 0 { i64::MIN } else { i64::MAX }) % 360_000
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visible_velocity(actor: &Actor) -> (i64, i64) {
        let velocity = actor.snapshot().velocity;
        (velocity.x, velocity.z)
    }

    #[test]
    fn native_delta_distinguishes_1799_from_1800_time_units() {
        assert_eq!(native_time_units_to_steps(1_799), 17);
        assert_eq!(native_time_units_to_steps(1_800), 18);
    }

    #[test]
    fn native_fastest_angle_quantizes_small_jitter_to_forward() {
        assert_eq!(direction_mdeg(-600, 99_200), 0);
        assert_eq!(direction_mdeg(600, -99_200), 180_000);
        assert_eq!(direction_mdeg(1_000, 151_400), 853);
    }

    #[test]
    fn raw_q32_velocity_preserves_arclight_target_angle_precision() {
        assert_eq!(
            direction_mdeg_q32_raw(-198_556_428, -30_061_443_202),
            180_812
        );
        assert_eq!(direction_mdeg(-46, -6_999), 180_811);
    }

    #[test]
    fn q32_normalized_velocity_matches_frozen_arclight_delta() {
        let speed = space_to_q32(7_000);
        let reconstructed =
            normalized_velocity_q32_raw(space_to_q32(-1_000), space_to_q32(-151_400), speed);
        assert_eq!(reconstructed, (-198_556_428, -30_061_443_202));
        assert_eq!(
            (
                q32_to_space_rounded(reconstructed.0),
                q32_to_space_rounded(reconstructed.1),
            ),
            (-46, -6_999)
        );

        let c0_1 = 0x1999_9999;
        let native_raw = normalized_velocity_q32_raw(-10 * c0_1, -150 * Q32_ONE - 14 * c0_1, speed);
        assert_eq!(native_raw, reconstructed);
    }

    #[test]
    fn q32_clamp_magnitude_stops_at_a_near_target_point() {
        let dx = space_to_q32(100);
        let dz = space_to_q32(-50);
        let speed = space_to_q32(7_123);
        let maximum = q32_mul(speed, NATIVE_LOGIC_DELTA_Q32);

        assert!(native_q32_magnitude(dx, dz) < maximum);
        assert_eq!(clamp_magnitude_q32_raw(dx, dz, maximum), (dx, dz));

        let far_dx = space_to_q32(1_000);
        let far_dz = space_to_q32(-10_000);
        let magnitude = native_q32_magnitude(far_dx, far_dz);
        let reciprocal = q32_div(Q32_ONE, magnitude);
        let native_order = (
            q32_mul(q32_mul(far_dx, reciprocal), maximum),
            q32_mul(q32_mul(far_dz, reciprocal), maximum),
        );
        let old_grouping = (
            q32_mul(
                q32_mul(q32_mul(far_dx, reciprocal), speed),
                NATIVE_LOGIC_DELTA_Q32,
            ),
            q32_mul(
                q32_mul(q32_mul(far_dz, reciprocal), speed),
                NATIVE_LOGIC_DELTA_Q32,
            ),
        );
        assert_ne!(native_order, old_grouping);
        assert_eq!(
            clamp_magnitude_q32_raw(far_dx, far_dz, maximum),
            native_order
        );
    }

    #[test]
    fn snapshot_velocity_is_quantized_from_raw_agent_velocity() {
        let layout = CompiledLayout {
            round: 1,
            placements: [
                Placement {
                    team: 0,
                    unit_id: 1,
                    formation_id: 1,
                    type_name: "marksman".to_owned(),
                    world_x: 0,
                    world_z: -50,
                    rotation: 0,
                },
                Placement {
                    team: 1,
                    unit_id: 2,
                    formation_id: 2,
                    type_name: "arclight".to_owned(),
                    world_x: 0,
                    world_z: 100,
                    rotation: 180_000,
                },
            ],
        };
        let config = SimulationConfig::load(None).unwrap();
        let mut simulation = Simulation::new(
            &layout,
            &config.units,
            &config.training_ground,
            1_787_555_163,
        )
        .unwrap();
        let actor = simulation.actors.get_mut(&1).unwrap();
        actor.current_velocity_x_q32 = -198_556_428;
        actor.current_velocity_z_q32 = -30_061_443_202;

        assert_eq!(visible_velocity(actor), (-46, -6_999));
    }

    #[test]
    fn deployment_raw_alone_still_rounds_tick_twenty_two_up() {
        let initial_z_q32 = 100 * Q32_ONE + 7 * C0_1_RAW;
        let fixed_delta_z_q32 = q32_mul(-30_061_443_202, NATIVE_LOGIC_DELTA_Q32);
        let deploy_only_z_q32 = initial_z_q32 + 14 * fixed_delta_z_q32;

        assert_eq!(initial_z_q32, 432_503_206_703);
        assert_eq!(deploy_only_z_q32, 411_460_196_533);
        assert_eq!(q32_to_space_rounded(deploy_only_z_q32), 95_801);
    }

    #[test]
    fn deployment_raw_and_per_tick_target_direction_round_tick_twenty_two_down() {
        let layout = CompiledLayout {
            round: 1,
            placements: [
                Placement {
                    team: 0,
                    unit_id: 1,
                    formation_id: 1,
                    type_name: "marksman".to_owned(),
                    world_x: 0,
                    world_z: -50,
                    rotation: 0,
                },
                Placement {
                    team: 1,
                    unit_id: 2,
                    formation_id: 2,
                    type_name: "arclight".to_owned(),
                    world_x: 0,
                    world_z: 100,
                    rotation: 180_000,
                },
            ],
        };
        let config = SimulationConfig::load(None).unwrap();
        let mut simulation = Simulation::new(
            &layout,
            &config.units,
            &config.training_ground,
            1_787_555_163,
        )
        .unwrap();

        assert_eq!(
            (
                simulation.actors[&1].x_q32,
                simulation.actors[&1].z_q32,
                simulation.actors[&2].x_q32,
                simulation.actors[&2].z_q32,
            ),
            (
                -2_147_483_645,
                -217_754_841_903,
                2_147_483_645,
                432_503_206_703,
            )
        );

        for step in 0..22 {
            simulation.step(step).unwrap();
        }

        let arclight = &simulation.actors[&2];
        assert_eq!(arclight.z_q32, 411_459_840_709);
        assert_eq!(arclight.z, 95_800);
    }

    #[test]
    fn rvo_pipeline_publishes_before_movement_consumes_velocity() {
        let layout = CompiledLayout {
            round: 1,
            placements: [
                Placement {
                    team: 0,
                    unit_id: 1,
                    formation_id: 1,
                    type_name: "marksman".to_owned(),
                    world_x: 0,
                    world_z: -50,
                    rotation: 0,
                },
                Placement {
                    team: 1,
                    unit_id: 2,
                    formation_id: 2,
                    type_name: "arclight".to_owned(),
                    world_x: 0,
                    world_z: 100,
                    rotation: 180_000,
                },
            ],
        };
        let config = SimulationConfig::load(None).unwrap();
        let mut simulation = Simulation::new(
            &layout,
            &config.units,
            &config.training_ground,
            1_787_555_163,
        )
        .unwrap();
        let initial = simulation.actors[&2].clone();

        for tick in 1..=9 {
            simulation.step(tick - 1).unwrap();
            let arclight = &simulation.actors[&2];
            if tick <= 7 {
                assert_eq!((arclight.x, arclight.z), (initial.x, initial.z));
                assert_eq!(arclight.body_rotation, initial.body_rotation);
                assert_eq!(visible_velocity(arclight), (0, 0));
            } else if tick == 8 {
                assert_eq!((arclight.x, arclight.z), (initial.x, initial.z));
                assert_eq!(arclight.body_rotation, initial.body_rotation);
                assert_eq!(visible_velocity(arclight), (-46, -6_999));
                assert_eq!(
                    (
                        arclight.current_velocity_x_q32,
                        arclight.current_velocity_z_q32,
                    ),
                    (-198_556_428, -30_061_443_202)
                );
            } else {
                assert_eq!((arclight.x, arclight.z), (498, 100_350));
                assert_eq!(
                    (arclight.x_q32, arclight.z_q32),
                    (2_137_555_823, 431_000_134_548)
                );
                assert_ne!(arclight.body_rotation, initial.body_rotation);
            }
        }
    }

    #[test]
    fn rvo_boundary_recalculates_velocity_from_the_published_target_and_current_position() {
        let layout = CompiledLayout {
            round: 1,
            placements: [
                Placement {
                    team: 0,
                    unit_id: 1,
                    formation_id: 1,
                    type_name: "marksman".to_owned(),
                    world_x: 0,
                    world_z: -50,
                    rotation: 0,
                },
                Placement {
                    team: 1,
                    unit_id: 2,
                    formation_id: 2,
                    type_name: "arclight".to_owned(),
                    world_x: 0,
                    world_z: 100,
                    rotation: 180_000,
                },
            ],
        };
        let config = SimulationConfig::load(None).unwrap();
        let mut simulation = Simulation::new(
            &layout,
            &config.units,
            &config.training_ground,
            1_787_555_163,
        )
        .unwrap();

        for step in 0..4 {
            simulation.step(step).unwrap();
        }
        let arclight = &simulation.actors[&2];
        let previous_boundary_candidate = normalized_velocity_q32_raw(
            arclight.solver_target_x_q32.saturating_sub(arclight.x_q32),
            arclight.solver_target_z_q32.saturating_sub(arclight.z_q32),
            arclight.solver_speed_q32,
        );
        let arclight = simulation.actors.get_mut(&2).unwrap();
        arclight.x_q32 = arclight.x_q32.saturating_add(10 * Q32_ONE);
        arclight.x = q32_to_space_rounded(arclight.x_q32);

        for step in 4..8 {
            simulation.step(step).unwrap();
        }
        let arclight = &simulation.actors[&2];
        let recalculated = normalized_velocity_q32_raw(
            arclight
                .published_target_x_q32
                .saturating_sub(arclight.x_q32),
            arclight
                .published_target_z_q32
                .saturating_sub(arclight.z_q32),
            arclight.published_speed_q32,
        );

        assert_eq!(
            (
                arclight.current_velocity_x_q32,
                arclight.current_velocity_z_q32,
            ),
            recalculated
        );
        assert_ne!(previous_boundary_candidate, recalculated);
    }

    #[test]
    fn range_entry_stops_only_after_the_two_stage_rvo_delay() {
        let layout = CompiledLayout {
            round: 1,
            placements: [
                Placement {
                    team: 0,
                    unit_id: 1,
                    formation_id: 1,
                    type_name: "marksman".to_owned(),
                    world_x: 0,
                    world_z: -50,
                    rotation: 0,
                },
                Placement {
                    team: 1,
                    unit_id: 2,
                    formation_id: 2,
                    type_name: "arclight".to_owned(),
                    world_x: 0,
                    world_z: 100,
                    rotation: 180_000,
                },
            ],
        };
        let config = SimulationConfig::load(None).unwrap();
        let mut simulation = Simulation::new(
            &layout,
            &config.units,
            &config.training_ground,
            1_787_555_163,
        )
        .unwrap();
        let mut tick_121_raw_position = None;
        let mut tick_121_body_rotation = None;

        for tick in 1..=128 {
            simulation.step(tick - 1).unwrap();
            let arclight = &simulation.actors[&2];
            match tick {
                120 => {
                    assert_eq!(arclight.z, 61_500);
                    assert_eq!(arclight.motion, MotionState::Moving);
                }
                121 => {
                    assert_eq!(arclight.z, 61_150);
                    assert_eq!(arclight.motion, MotionState::Moving);
                    assert_eq!(visible_velocity(arclight), (-46, -6_999));
                    tick_121_raw_position = Some((arclight.x_q32, arclight.z_q32));
                    tick_121_body_rotation = Some(arclight.body_rotation);
                }
                122 => {
                    assert_eq!(arclight.z, 60_800);
                    assert_eq!(arclight.motion, MotionState::Attacking);
                    assert_eq!(visible_velocity(arclight), (-46, -6_999));
                    assert_eq!(
                        (arclight.next_target_x_q32, arclight.next_target_z_q32),
                        tick_121_raw_position.unwrap()
                    );
                    assert_eq!(arclight.next_speed_q32, 0);
                    assert_eq!(arclight.body_rotation, tick_121_body_rotation.unwrap());
                    assert!(arclight.pending.is_none());
                }
                123..=127 => {
                    let expected_z = 60_800 - i64::try_from(tick - 122).unwrap() * 350;
                    assert_eq!(arclight.z, expected_z);
                    assert_eq!(arclight.motion, MotionState::Attacking);
                    assert_eq!(visible_velocity(arclight), (-46, -6_999));
                    if tick == 124 {
                        assert_eq!(arclight.published_speed_q32, space_to_q32(7_000));
                        assert_eq!(arclight.solver_speed_q32, 0);
                    }
                }
                128 => {
                    assert_eq!(arclight.z, 58_700);
                    assert_eq!(arclight.motion, MotionState::Attacking);
                    assert_eq!(visible_velocity(arclight), (0, 0));
                    assert_eq!(arclight.published_speed_q32, 0);
                }
                _ => {}
            }
        }
    }

    #[test]
    fn tick_fifteen_aim_uses_raw_q32_positions() {
        let layout = CompiledLayout {
            round: 1,
            placements: [
                Placement {
                    team: 0,
                    unit_id: 1,
                    formation_id: 1,
                    type_name: "marksman".to_owned(),
                    world_x: 0,
                    world_z: -50,
                    rotation: 0,
                },
                Placement {
                    team: 1,
                    unit_id: 2,
                    formation_id: 2,
                    type_name: "arclight".to_owned(),
                    world_x: 0,
                    world_z: 100,
                    rotation: 180_000,
                },
            ],
        };
        let config = SimulationConfig::load(None).unwrap();
        let mut simulation = Simulation::new(
            &layout,
            &config.units,
            &config.training_ground,
            1_787_555_163,
        )
        .unwrap();

        for step in 0..14 {
            simulation.step(step).unwrap();
        }

        let marksman = &simulation.actors[&1];
        let arclight = &simulation.actors[&2];
        let raw_dx = arclight.x_q32.saturating_sub(marksman.x_q32);
        let raw_dz = arclight.z_q32.saturating_sub(marksman.z_q32);
        assert_eq!((raw_dx, raw_dz), (4_235_400_048, 641_239_568_913));
        assert_eq!(direction_mdeg_q32_raw(raw_dx, raw_dz), 798);
        assert_eq!(direction_mdeg_q32_raw(-raw_dx, -raw_dz), 180_798);

        let mm_dx = arclight.x - marksman.x;
        let mm_dz = arclight.z - marksman.z;
        assert_eq!((mm_dx, mm_dz), (986, 149_300));
        assert_eq!(direction_mdeg(mm_dx, mm_dz), 797);
        assert_eq!(direction_mdeg(-mm_dx, -mm_dz), 180_797);

        simulation.step(14).unwrap();
        assert_eq!(simulation.actors[&1].aim_rotation, 798);
        assert_eq!(simulation.actors[&2].aim_rotation, 180_798);
    }

    #[test]
    fn projectile_raw_target_cache_preserves_rounding_sequence() {
        let layout = CompiledLayout {
            round: 1,
            placements: [
                Placement {
                    team: 0,
                    unit_id: 1,
                    formation_id: 1,
                    type_name: "marksman".to_owned(),
                    world_x: 0,
                    world_z: -50,
                    rotation: 0,
                },
                Placement {
                    team: 1,
                    unit_id: 2,
                    formation_id: 2,
                    type_name: "arclight".to_owned(),
                    world_x: 0,
                    world_z: 100,
                    rotation: 180_000,
                },
            ],
        };
        let config = SimulationConfig::load(None).unwrap();
        let mut simulation = Simulation::new(
            &layout,
            &config.units,
            &config.training_ground,
            1_787_555_163,
        )
        .unwrap();
        let mut millimeter_path = None;
        let mut raw_x = Vec::new();
        let mut millimeter_x = Vec::new();

        for tick in 1..=18 {
            simulation.step(tick - 1).unwrap();
            if tick < 14 {
                continue;
            }

            let projectile = &simulation.projectiles[0];
            let owner = &simulation.actors[&projectile.owner];
            let target = &simulation.actors[&projectile.target];
            let (x_q32, z_q32) = millimeter_path
                .get_or_insert_with(|| (space_to_q32(owner.x), space_to_q32(owner.z)));
            let dx_q32 = space_to_q32(target.x).saturating_sub(*x_q32);
            let dz_q32 = space_to_q32(target.z).saturating_sub(*z_q32);
            let distance_q32 = native_q32_magnitude(dx_q32, dz_q32);
            let step_q32 = q32_mul(space_to_q32(projectile.speed), NATIVE_LOGIC_DELTA_Q32);
            let move_q32 = step_q32.min(distance_q32);
            let reciprocal = q32_div(Q32_ONE, distance_q32);
            *x_q32 = x_q32.saturating_add(q32_mul(q32_mul(dx_q32, reciprocal), move_q32));
            *z_q32 = z_q32.saturating_add(q32_mul(q32_mul(dz_q32, reciprocal), move_q32));

            raw_x.push(projectile.x);
            millimeter_x.push(q32_to_space_rounded(*x_q32));
        }

        assert_eq!(raw_x, [-335, -170, -5, 160, 326]);
        assert_eq!(millimeter_x, [-335, -170, -4, 161, 326]);
    }

    #[test]
    fn stopped_attacker_tracks_target_with_aim_without_rotating_root_body() {
        let layout = CompiledLayout {
            round: 1,
            placements: [
                Placement {
                    team: 0,
                    unit_id: 1,
                    formation_id: 1,
                    type_name: "marksman".to_owned(),
                    world_x: 0,
                    world_z: -50,
                    rotation: 0,
                },
                Placement {
                    team: 1,
                    unit_id: 2,
                    formation_id: 2,
                    type_name: "arclight".to_owned(),
                    world_x: 0,
                    world_z: -100,
                    rotation: 180_000,
                },
            ],
        };
        let config = SimulationConfig::load(None).unwrap();
        let mut simulation = Simulation::new(
            &layout,
            &config.units,
            &config.training_ground,
            1_787_555_163,
        )
        .unwrap();
        let marksman = simulation.actors.get_mut(&1).unwrap();
        assert!(!marksman.rules.independent_aim);
        marksman.motion = MotionState::Attacking;
        marksman.next_attack_step = u64::MAX;

        simulation.step_actor(1, 0, &mut Vec::new()).unwrap();
        let root_body = simulation.actors[&1].body_rotation;
        let initial_aim = simulation.actors[&1].aim_rotation;

        let target = simulation.actors.get_mut(&2).unwrap();
        target.x += 10_000;
        target.x_q32 = space_to_q32(target.x);
        let expected_aim = direction_mdeg_q32_raw(
            simulation.actors[&2]
                .x_q32
                .saturating_sub(simulation.actors[&1].x_q32),
            simulation.actors[&2]
                .z_q32
                .saturating_sub(simulation.actors[&1].z_q32),
        );
        simulation.step_actor(1, 1, &mut Vec::new()).unwrap();
        let marksman = &simulation.actors[&1];

        assert_eq!(marksman.motion, MotionState::Attacking);
        assert_eq!(marksman.body_rotation, root_body);
        assert_ne!(marksman.aim_rotation, initial_aim);
        assert_eq!(marksman.aim_rotation, expected_aim);
    }
}
