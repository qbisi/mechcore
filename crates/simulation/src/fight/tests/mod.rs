mod damage;
mod deploy;
mod group;
mod math;
mod motion;
mod search;
mod skill;
mod walls;

use super::skill::PendingRelease;
use super::*;

pub(super) fn unit_target(id: u64) -> FightActorRef {
    FightActorRef::Unit(id)
}

pub(super) fn test_placement(
    team: u32,
    formation_index: i32,
    world_x: i64,
    world_z: i64,
) -> Placement {
    Placement {
        team,
        unit_id: 0,
        formation_id: 0,
        formation_index,
        type_name: "arclight".to_owned(),
        world_x,
        world_z,
        rotation: if team == 0 { 0 } else { 180_000 },
        rotated: false,
        level: 1,
        corrections: Vec::new(),
    }
}

pub(super) fn snapshot_velocity_q32(actor: &Actor) -> (i64, i64) {
    let velocity = actor.snapshot().velocity;
    (velocity.x, velocity.z)
}

pub(super) fn raw_test_simulation(
    layout: &CompiledLayout,
    config: &SimulationConfig,
    seed: i32,
) -> Simulation {
    let actors = initialize_actors(layout, &config.units, seed).unwrap();
    let InitialBuildings {
        states: buildings,
        unsearchable,
        colliders: construction_colliders,
        tower_losses,
        tower_buffed_constructions,
    } = initialize_buildings(&config.towers, &[], &BTreeMap::new()).unwrap();
    let target_quadtrees = initialize_target_quadtrees(&actors, &buildings);
    Simulation {
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
        constructions: BTreeMap::new(),
        towers: config.towers.clone(),
        tower_losses,
        tower_buffed_constructions,
    }
}

pub(super) fn set_actor_position(actor: &mut Actor, x: i64, z: i64) {
    set_actor_position_q32(actor, space_to_q32(x), space_to_q32(z));
}

pub(super) fn set_actor_position_q32(actor: &mut Actor, x_q32: i64, z_q32: i64) {
    actor.x_q32 = x_q32;
    actor.z_q32 = z_q32;
    actor.target_query_x_q32 = x_q32;
    actor.target_query_z_q32 = z_q32;
    actor.x = q32_to_space_rounded(x_q32);
    actor.z = q32_to_space_rounded(z_q32);
    actor.motion.next_target_x_q32 = actor.x_q32;
    actor.motion.next_target_z_q32 = actor.z_q32;
    actor.motion.solver_target_x_q32 = actor.x_q32;
    actor.motion.solver_target_z_q32 = actor.z_q32;
    actor.motion.published_target_x_q32 = actor.x_q32;
    actor.motion.published_target_z_q32 = actor.z_q32;
}

pub(super) fn micrometers_to_q32(value: i64) -> i64 {
    i64::try_from(i128::from(value) * i128::from(Q32_ONE) / 1_000_000).unwrap()
}
