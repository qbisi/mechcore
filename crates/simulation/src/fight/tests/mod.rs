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

/// The layout a pinned fight starts from: a fight document under `tests/`,
/// projected onto the layout the game recorded it from.
pub(super) fn pinned_layout(fight: &[u8], units: &crate::rules::UnitConfigs) -> CompiledLayout {
    let fight = mechcore_document::fight::parse_yaml(fight).unwrap();
    let layout =
        mechcore_document::canonical_yaml(mechcore_document::fight::project(&fight)).unwrap();
    crate::layout::compile_with_seed(layout.as_bytes(), units)
        .unwrap()
        .1
}

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
        exp: 0,
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
        construction_groups: _,
        building_exp,
    } = initialize_buildings(&config.towers, &[], &BTreeMap::new()).unwrap();
    let map_crystals = map_crystals(config.maps.buildings(1021).unwrap());
    let target_quadtrees = initialize_target_quadtrees(&actors, &buildings);
    let buildings_query_alive = standing_buildings(&buildings);
    Simulation {
        actors,
        team_random: BTreeMap::new(),
        projectiles: Vec::new(),
        buildings,
        target_quadtrees,
        mech_quadtrees: BTreeMap::new(),
        identities: IdentityAllocator::new(),
        rvo_counter: 0,
        rvo_first_tree_pending: true,
        rvo_quadtree_capacity: crate::fight::rvo::QuadtreeCapacity::default(),
        terminal_drain_pending: false,
        stop_step: None,
        late_building_events_pending: false,
        torn_down_buildings: Vec::new(),
        fallen_buildings: Vec::new(),
        tower_buff_events: BTreeMap::new(),
        fallen_towers: Vec::new(),
        building_buffs: BTreeMap::new(),
        buildings_query_alive,
        dropped_buffs: BTreeMap::new(),
        construction_colliders: construction_colliders.clone(),
        map_crystals,
        unsearchable_buildings: unsearchable.clone(),
        constructions: BTreeMap::new(),
        towers: config.towers.clone(),
        tower_losses,
        tower_buffed_constructions,
        statistics: BTreeMap::new(),
        construction_recorders: BTreeMap::new(),
        formations: BTreeMap::new(),
        attackers: BTreeMap::new(),
        building_exp,
        experience: super::experience::ExperienceTable::load().unwrap(),
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
