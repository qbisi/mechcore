use std::collections::BTreeMap;

use mechcore_mcfr::{
    Domain, DurableContext, Event, EventPayload, Gauge, Hashes, IdentityAllocator,
    IdentityContract, MCFR_SCHEMA_VERSION, McfrWriter, MotionState, NumericConvention, ObjectKind,
    ObjectRef, PersonalShieldState, Pose, ProjectileState, Rational, TransitionEvents, UnitState,
    Vec3, Visibility, WorldSnapshot,
};
use serde::Serialize;

use crate::{
    Error, Result,
    layout::{CompiledLayout, Placement},
    random::GrRandom,
    rules::{TargetDomain, UnitConfig, UnitConfigs, UnitDomain},
};

const SPACE_UNITS_PER_METER: i64 = 1_000;
const TIME_UNITS_PER_SECOND: u64 = 2_000;
const LOGIC_TICK_TIME_UNITS: u64 = 100;
const FIGHT_TIME_SECONDS: u64 = 120;
const FORMATION_JITTER_RANGE_TENTHS: i32 = 8;
const REFERENCE_GAME_BUILD: &str = "1.11.1.2.2227";

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
    y: i64,
    velocity_x: i64,
    velocity_y: i64,
    body_rotation: i64,
    aim_rotation: i64,
    life: i64,
    motion: MotionState,
    next_attack_step: u64,
    pending: Option<PendingRelease>,
    attack_random: GrRandom,
}

impl Actor {
    fn new(placement: Placement, rules: UnitConfig, round: u32, seed: i32) -> Self {
        let mut layout_random = GrRandom::new(u64::from(seed.cast_unsigned()));
        let jitter_unit = SPACE_UNITS_PER_METER / 10;
        let jitter_x =
            i64::from(layout_random.next_in_range(FORMATION_JITTER_RANGE_TENTHS)) * jitter_unit;
        let jitter_y =
            i64::from(layout_random.next_in_range(FORMATION_JITTER_RANGE_TENTHS)) * jitter_unit;
        let direction = if placement.team == 0 { 1 } else { -1 };
        let mut attack_random = GrRandom::new(u64::from(
            round
                .cast_signed()
                .wrapping_add(placement.team.cast_signed())
                .wrapping_mul(4_444)
                .cast_unsigned(),
        ));
        // `FightTeam.RefreshRandomData` consumes one initial interval sample
        // for every member with a nonzero interval offset.
        let interval_offset_steps = positive_time_units_to_steps(
            rules.attack.interval_offset_time_units(),
            LOGIC_TICK_TIME_UNITS,
        );
        if interval_offset_steps > 0 {
            let _initial_sample = attack_random
                .next_in_range(i32::try_from(interval_offset_steps).unwrap_or(i32::MAX));
        }
        let max_life = rules.max_life;
        Self {
            x: placement
                .world_x
                .saturating_mul(SPACE_UNITS_PER_METER)
                .saturating_add(jitter_x * direction),
            y: placement
                .world_y
                .saturating_mul(SPACE_UNITS_PER_METER)
                .saturating_add(jitter_y * direction),
            body_rotation: placement.rotation,
            aim_rotation: placement.rotation,
            placement,
            rules,
            velocity_x: 0,
            velocity_y: 0,
            life: max_life,
            motion: MotionState::Idle,
            next_attack_step: 0,
            pending: None,
            attack_random,
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
            position: point(self.x, self.y),
            body_rotation: self.body_rotation,
            aim_pose: Pose {
                position: point(self.x, self.y),
                rotation: self.aim_rotation,
            },
            velocity: point(self.velocity_x, self.velocity_y),
            motion_state: if self.alive() {
                self.motion
            } else {
                MotionState::Stopped
            },
            collision_radius: self.rules.collision_radius(),
            life: self.life,
            max_life: self.rules.max_life,
            alive: self.alive(),
            active: self.alive(),
            targetable: self.alive(),
            visibility: Visibility::Normal,
            personal_shield: PersonalShieldState {
                active: false,
                enabled: false,
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
    y: i64,
    cached_target_x: i64,
    cached_target_y: i64,
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
            position: point(self.x, self.y),
            orientation: direction_mdeg(
                self.cached_target_x - self.x,
                self.cached_target_y - self.y,
            ),
            target: Some(ObjectRef::new(ObjectKind::Unit, self.target)),
            cached_target_position: point(self.cached_target_x, self.cached_target_y),
            cached_target_radius: self.cached_target_radius,
            released: true,
            life: Gauge {
                current: 0,
                maximum: 0,
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
    projectiles: Vec<Projectile>,
    identities: IdentityAllocator,
}

impl Simulation {
    fn new(layout: &CompiledLayout, configs: &UnitConfigs, seed: i32) -> Result<Self> {
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
                Actor::new(placement.clone(), rules.clone(), layout.round, seed),
            );
        }
        Ok(Self {
            actors,
            projectiles: Vec::new(),
            identities: IdentityAllocator::new(),
        })
    }

    fn snapshot(&self) -> WorldSnapshot {
        WorldSnapshot {
            units: self.actors.values().map(Actor::snapshot).collect(),
            projectiles: self.projectiles.iter().map(Projectile::snapshot).collect(),
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
            actor.velocity_y = 0;
            actor.motion = MotionState::Stopped;
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
            actor.velocity_y = 0;
            return Ok(());
        };
        let target = &self.actors[&target_id];
        let target_x = target.x;
        let target_y = target.y;
        let target_radius = target.rules.collision_radius();
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let dx = target_x - actor.x;
        let dy = target_y - actor.y;
        let target_rotation = direction_mdeg(dx, dy);
        actor.aim_rotation = target_rotation;
        let center_distance = magnitude(dx, dy);
        let edge_distance = center_distance
            .saturating_sub(actor.rules.collision_radius())
            .saturating_sub(target_radius);
        if edge_distance <= actor.rules.attack.range() {
            actor.motion = MotionState::Attacking;
            actor.velocity_x = 0;
            actor.velocity_y = 0;
            if !actor.rules.independent_aim {
                actor.body_rotation = rotate_towards(
                    actor.body_rotation,
                    target_rotation,
                    rotation_per_tick(actor),
                );
                actor.aim_rotation = actor.body_rotation;
            }
            if actor.pending.is_none() && step >= actor.next_attack_step {
                let interval_steps = positive_time_units_to_steps(
                    actor.rules.attack.interval_time_units(),
                    LOGIC_TICK_TIME_UNITS,
                );
                let offset_steps = positive_time_units_to_steps(
                    actor.rules.attack.interval_offset_time_units(),
                    LOGIC_TICK_TIME_UNITS,
                );
                let sample = if offset_steps == 0 {
                    0
                } else {
                    i64::from(
                        actor
                            .attack_random
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
                    step: step.saturating_add(positive_time_units_to_steps(
                        actor.rules.attack.release_delay_time_units(),
                        LOGIC_TICK_TIME_UNITS,
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
        let (move_x, move_y) = displacement_towards(dx, dy, displacement);
        actor.x = actor.x.saturating_add(move_x);
        actor.y = actor.y.saturating_add(move_y);
        actor.velocity_x = scale_per_second(move_x, LOGIC_TICK_TIME_UNITS, TIME_UNITS_PER_SECOND);
        actor.velocity_y = scale_per_second(move_y, LOGIC_TICK_TIME_UNITS, TIME_UNITS_PER_SECOND);
        actor.body_rotation = rotate_towards(
            actor.body_rotation,
            direction_mdeg(move_x, move_y),
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
        let target_y = target.y;
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
            y: owner.y,
            cached_target_x: target_x,
            cached_target_y: target_y,
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

    fn step_projectiles(&mut self, events: &mut Vec<Event>) -> Result<()> {
        let mut retained = Vec::with_capacity(self.projectiles.len());
        for mut projectile in std::mem::take(&mut self.projectiles) {
            if let Some(target) = self
                .actors
                .get(&projectile.target)
                .filter(|actor| actor.alive())
            {
                projectile.cached_target_x = target.x;
                projectile.cached_target_y = target.y;
                projectile.cached_target_radius = target.rules.collision_radius();
            }
            let dx = projectile.cached_target_x - projectile.x;
            let dy = projectile.cached_target_y - projectile.y;
            let distance = magnitude(dx, dy);
            let displacement = scale_per_tick(
                projectile.speed,
                LOGIC_TICK_TIME_UNITS,
                TIME_UNITS_PER_SECOND,
            );
            if distance.saturating_sub(projectile.cached_target_radius) <= displacement {
                let travel = distance
                    .saturating_sub(projectile.cached_target_radius)
                    .max(0);
                let (move_x, move_y) = displacement_towards(dx, dy, travel);
                projectile.x = projectile.x.saturating_add(move_x);
                projectile.y = projectile.y.saturating_add(move_y);
                self.impact(&projectile, events)?;
            } else {
                let (move_x, move_y) = displacement_towards(dx, dy, displacement);
                projectile.x = projectile.x.saturating_add(move_x);
                projectile.y = projectile.y.saturating_add(move_y);
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
            target.life = target.life.saturating_sub(projectile.damage).max(0);
            events.push(event(
                None,
                Some(projectile_ref),
                Some(target_ref),
                EventPayload::Damage {
                    amount: projectile.damage,
                },
            ));
            if target.life == 0 {
                target.motion = MotionState::Stopped;
                target.velocity_x = 0;
                target.velocity_y = 0;
                target.pending = None;
            }
        }
        events.push(event(
            Some(projectile_ref),
            Some(owner_ref),
            Some(target_ref),
            EventPayload::ProjectileRemoved {
                position: point(projectile.x, projectile.y),
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
    configs: &UnitConfigs,
    seed: i32,
    seed_source: &'static str,
    output: &std::path::Path,
) -> Result<SimulationResult> {
    let divisor = gcd(LOGIC_TICK_TIME_UNITS, TIME_UNITS_PER_SECOND);
    let context = DurableContext {
        schema_version: MCFR_SCHEMA_VERSION,
        game_build: REFERENCE_GAME_BUILD.to_owned(),
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
        identity_contract: IdentityContract::TeamYxSequentialV1,
    };
    let mut simulation = Simulation::new(layout, configs, seed)?;
    let mut writer = McfrWriter::create(output, &context, simulation.snapshot())?;
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
        writer.push_transition(&events, simulation.snapshot())?;
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
        game_build: REFERENCE_GAME_BUILD.to_owned(),
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

fn positive_time_units_to_steps(time_units: u64, tick_time_units: u64) -> u64 {
    if time_units == 0 {
        0
    } else {
        time_units.saturating_add(tick_time_units / 2) / tick_time_units
    }
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

const fn point(x: i64, y: i64) -> Vec3 {
    Vec3 { x, y, z: 0 }
}

fn magnitude(x: i64, y: i64) -> i64 {
    integer_sqrt(i128::from(x) * i128::from(x) + i128::from(y) * i128::from(y))
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

fn displacement_towards(dx: i64, dy: i64, length: i64) -> (i64, i64) {
    let magnitude = magnitude(dx, dy);
    if magnitude == 0 || length == 0 {
        return (0, 0);
    }
    let x = i128::from(dx) * i128::from(length) / i128::from(magnitude);
    let y = i128::from(dy) * i128::from(length) / i128::from(magnitude);
    (saturating_i128_to_i64(x), saturating_i128_to_i64(y))
}

fn saturating_i128_to_i64(value: i128) -> i64 {
    match i64::try_from(value) {
        Ok(value) => value,
        Err(_) if value.is_negative() => i64::MIN,
        Err(_) => i64::MAX,
    }
}

fn direction_mdeg(dx: i64, dy: i64) -> i64 {
    if dx == 0 && dy == 0 {
        return 0;
    }
    let x = dx.unsigned_abs();
    let y = dy.unsigned_abs();
    let quadrant = if y >= x {
        i64::try_from(x.saturating_mul(45_000) / y.max(1)).unwrap_or(45_000)
    } else {
        90_000 - i64::try_from(y.saturating_mul(45_000) / x.max(1)).unwrap_or(45_000)
    };
    match (dx >= 0, dy >= 0) {
        (true, true) => quadrant,
        (true, false) => 180_000 - quadrant,
        (false, false) => 180_000 + quadrant,
        (false, true) => 360_000 - quadrant,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn q32_rendered_and_si_arclight_intervals_reach_the_same_logic_step() {
        assert_eq!(
            positive_time_units_to_steps(1_799, LOGIC_TICK_TIME_UNITS),
            18
        );
        assert_eq!(
            positive_time_units_to_steps(1_800, LOGIC_TICK_TIME_UNITS),
            18
        );
    }
}
