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

/// A level-one formation of `type_name` at a world position, facing the other
/// side as a deployment does; a test states only what it changes from it.
pub(super) fn placement(
    team: u32,
    type_name: &str,
    formation_index: i32,
    world_x: i64,
    world_z: i64,
) -> Placement {
    Placement {
        team,
        unit_id: 0,
        formation_id: 0,
        formation_index,
        type_name: type_name.to_owned(),
        world_x,
        world_z,
        rotation: if team == 0 { 0 } else { 180_000 },
        rotated: false,
        level: 1,
        exp: 0,
        experience_rate: crate::data::ExperienceRate::default(),
        unit_experience_rate: crate::data::ExperienceRate::default(),
        corrections: Vec::new(),
        lifesteal: None,
        auto_recovery: None,
        energy_shield: None,
        stealth: None,
        sweep: None,
        distance_intensify: false,
        secondary_damage: None,
        dead_line: None,
        share_distance: None,
        move_ability_attack: None,
        move_ability_range_item: None,
        interception: None,
        carried_shield: None,
        production: None,
        buff_sources: Vec::new(),
        ignored_buffs: Vec::new(),
        important: false,
        ignores_control_beam: false,
        travelling: false,
        extra_weapons: Vec::new(),
        technology_disable: crate::layout::TechnologyDisable::default(),
        dead_summon: None,
        surfacing: None,
    }
}

/// An Arclight's formation, `placement`'s commonest case.
pub(super) fn test_placement(
    team: u32,
    formation_index: i32,
    world_x: i64,
    world_z: i64,
) -> Placement {
    placement(team, "arclight", formation_index, world_x, world_z)
}

/// A unit's recorded velocity, which reads nothing its range changes.
pub(super) fn snapshot_velocity_q32(actor: &Actor) -> (i64, i64) {
    let velocity = actor.snapshot(Vec::new()).velocity;
    (velocity.x, velocity.z)
}

/// A simulation of `layout` built as a fight builds one, without the
/// presearch that picks every unit's first target.
pub(super) fn raw_test_simulation(
    layout: &CompiledLayout,
    config: &SimulationConfig,
    seed: i32,
) -> Simulation {
    Simulation::new_unprepared(layout, &config.units, &config.towers, &config.maps, seed).unwrap()
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
