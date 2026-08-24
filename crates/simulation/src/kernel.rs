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
    velocity_x: i64,
    velocity_z: i64,
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
        let jitter_unit = SPACE_UNITS_PER_METER / 10;
        let jitter_x =
            i64::from(layout_random.next_in_range(FORMATION_JITTER_RANGE_TENTHS)) * jitter_unit;
        let jitter_z =
            i64::from(layout_random.next_in_range(FORMATION_JITTER_RANGE_TENTHS)) * jitter_unit;
        let direction = if placement.team == 0 { 1 } else { -1 };
        let max_life = rules.max_life;
        Self {
            x: placement
                .world_x
                .saturating_mul(SPACE_UNITS_PER_METER)
                .saturating_add(jitter_x * direction),
            z: placement
                .world_z
                .saturating_mul(SPACE_UNITS_PER_METER)
                .saturating_add(jitter_z * direction),
            body_rotation: placement.rotation,
            aim_rotation: placement.rotation,
            placement,
            rules,
            velocity_x: 0,
            velocity_z: 0,
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
            velocity: point(self.velocity_x, self.velocity_z),
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
        }
        self.step_projectiles(&mut events)?;
        if self.naturally_finished() {
            for actor in self.actors.values_mut() {
                actor.motion = MotionState::Idle;
                actor.velocity_x = 0;
                actor.velocity_z = 0;
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
            actor.velocity_x = 0;
            actor.velocity_z = 0;
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
            actor.velocity_x = 0;
            actor.velocity_z = 0;
            return Ok(());
        };
        let target = &self.actors[&target_id];
        let target_x = target.x;
        let target_z = target.z;
        let target_radius = target.rules.collision_radius();
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let dx = target_x - actor.x;
        let dz = target_z - actor.z;
        let target_rotation = direction_mdeg(dx, dz);
        actor.aim_rotation = target_rotation;
        let center_distance = magnitude(dx, dz);
        let edge_distance = center_distance
            .saturating_sub(actor.rules.collision_radius())
            .saturating_sub(target_radius);
        if edge_distance <= actor.rules.attack.range() {
            let entered_attack = actor.motion != MotionState::Attacking;
            actor.motion = MotionState::Attacking;
            actor.velocity_x = 0;
            actor.velocity_z = 0;
            if !actor.rules.independent_aim {
                actor.body_rotation = rotate_towards(
                    actor.body_rotation,
                    target_rotation,
                    rotation_per_tick(actor),
                );
                actor.aim_rotation = actor.body_rotation;
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
        actor.motion = MotionState::Moving;
        let max_displacement = scale_per_tick(
            actor.rules.move_speed(),
            LOGIC_TICK_TIME_UNITS,
            TIME_UNITS_PER_SECOND,
        );
        let required = edge_distance.saturating_sub(actor.rules.attack.range());
        let displacement = max_displacement.min(required);
        let (move_x, move_z) = displacement_towards(dx, dz, displacement);
        actor.x = actor.x.saturating_add(move_x);
        actor.z = actor.z.saturating_add(move_z);
        actor.velocity_x = scale_per_second(move_x, LOGIC_TICK_TIME_UNITS, TIME_UNITS_PER_SECOND);
        actor.velocity_z = scale_per_second(move_z, LOGIC_TICK_TIME_UNITS, TIME_UNITS_PER_SECOND);
        actor.body_rotation = rotate_towards(
            actor.body_rotation,
            direction_mdeg(move_x, move_z),
            rotation_per_tick(actor),
        );
        Ok(())
    }

    fn release(&mut self, actor_id: u64, events: &mut Vec<Event>) -> Result<()> {
        let pending = self.actors[&actor_id]
            .pending
            .ok_or_else(|| Error::new("attack release has no pending action"))?;
        let target = &self.actors[&pending.target];
        let target_x = target.x;
        let target_z = target.z;
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
            x_q32: space_to_q32(owner.x),
            z_q32: space_to_q32(owner.z),
            cached_target_x: target_x,
            cached_target_z: target_z,
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
                projectile.cached_target_radius = target.rules.collision_radius();
            }
            let target_x_q32 = space_to_q32(projectile.cached_target_x);
            let target_z_q32 = space_to_q32(projectile.cached_target_z);
            let dx_q32 = target_x_q32.saturating_sub(projectile.x_q32);
            let dz_q32 = target_z_q32.saturating_sub(projectile.z_q32);
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
                target.velocity_x = 0;
                target.velocity_z = 0;
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

fn scale_per_tick(value_per_second: i64, tick_time_units: u64, time_units_per_second: u64) -> i64 {
    saturating_i128_to_i64(
        i128::from(value_per_second) * i128::from(tick_time_units)
            / i128::from(time_units_per_second),
    )
}

fn scale_per_second(value_per_tick: i64, tick_time_units: u64, time_units_per_second: u64) -> i64 {
    saturating_i128_to_i64(
        i128::from(value_per_tick) * i128::from(time_units_per_second)
            / i128::from(tick_time_units),
    )
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

fn displacement_towards(dx: i64, dz: i64, length: i64) -> (i64, i64) {
    let magnitude = magnitude(dx, dz);
    if magnitude == 0 || length == 0 {
        return (0, 0);
    }
    let x = i128::from(dx) * i128::from(length) / i128::from(magnitude);
    let z = i128::from(dz) * i128::from(length) / i128::from(magnitude);
    (saturating_i128_to_i64(x), saturating_i128_to_i64(z))
}

fn saturating_i128_to_i64(value: i128) -> i64 {
    match i64::try_from(value) {
        Ok(value) => value,
        Err(_) if value.is_negative() => i64::MIN,
        Err(_) => i64::MAX,
    }
}

fn direction_mdeg(dx: i64, dz: i64) -> i64 {
    if dx == 0 && dz == 0 {
        return 0;
    }
    let x = space_to_q32(dx);
    let z = space_to_q32(dz);
    let magnitude = native_q32_magnitude(x, z);
    if magnitude <= 0 {
        return 0;
    }
    let cosine = q32_div(z, magnitude).clamp(-Q32_ONE, Q32_ONE);
    let radians = fpcs_acos_fastest(cosine);
    let degrees = q32_mul(radians, 0x0039_4BB8_34C8);
    let degrees = if x < 0 {
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

    #[test]
    fn native_delta_distinguishes_1799_from_1800_time_units() {
        assert_eq!(native_time_units_to_steps(1_799), 17);
        assert_eq!(native_time_units_to_steps(1_800), 18);
    }

    #[test]
    fn native_fastest_angle_quantizes_small_jitter_to_forward() {
        assert_eq!(direction_mdeg(-600, 99_200), 0);
        assert_eq!(direction_mdeg(600, -99_200), 180_000);
    }
}
