use super::*;

#[test]
fn rvo_solves_a_collision_building_inside_the_influence_bound() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            Placement {
                type_name: "rhino".to_owned(),
                ..test_placement(0, 0, 0, 0)
            },
            test_placement(1, 0, 0, 100),
        ],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
    set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 100_000);
    let building = simulation.buildings.first_mut().unwrap();
    building.position = point(20_000, 0);
    let target_position = (simulation.actors[&2].x_q32, simulation.actors[&2].z_q32);
    let source = simulation.actors.get_mut(&1).unwrap();
    source.lock_target = Some(unit_target(2));
    source.motion = MotionState::Moving;
    source.next_target_x_q32 = target_position.0;
    source.next_target_z_q32 = target_position.1;
    source.next_speed_q32 = space_to_q32(source.stats.move_speed());
    source.next_max_speed_q32 = source.next_speed_q32;
    simulation.rvo_counter = 3;
    simulation.step_rvo();
    assert!(simulation.actors[&1].solver_speed_q32 > 0);
    assert_ne!(simulation.actors[&1].solver_target_z_q32, 0);
}

#[test]
fn rvo_q32_boundary_uses_raw_distance_not_snapshot_rounding() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            Placement {
                type_name: "rhino".to_owned(),
                ..test_placement(0, 0, 0, 0)
            },
            test_placement(1, 0, 0, 100),
            test_placement(1, 1, 1, 68),
        ],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    set_actor_position_q32(
        simulation.actors.get_mut(&1).unwrap(),
        micrometers_to_q32(499),
        micrometers_to_q32(499),
    );
    set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 100_000);
    set_actor_position_q32(
        simulation.actors.get_mut(&3).unwrap(),
        micrometers_to_q32(1_106_501),
        micrometers_to_q32(67_991_503),
    );
    assert_eq!(
        magnitude(
            simulation.actors[&3]
                .x
                .saturating_sub(simulation.actors[&1].x),
            simulation.actors[&3]
                .z
                .saturating_sub(simulation.actors[&1].z),
        ),
        68_001
    );
    assert!(q32_distance_within(
        simulation.actors[&1].x_q32,
        simulation.actors[&1].z_q32,
        simulation.actors[&3].x_q32,
        simulation.actors[&3].z_q32,
        68_000,
    ));
    let target_position = (simulation.actors[&2].x_q32, simulation.actors[&2].z_q32);
    let source = simulation.actors.get_mut(&1).unwrap();
    source.lock_target = Some(unit_target(2));
    source.motion = MotionState::Moving;
    source.next_target_x_q32 = target_position.0;
    source.next_target_z_q32 = target_position.1;
    source.next_speed_q32 = space_to_q32(source.stats.move_speed());
    source.next_max_speed_q32 = source.next_speed_q32;
    simulation.rvo_counter = 3;
    simulation.step_rvo();
    assert!(simulation.actors[&1].solver_speed_q32 > 0);
    assert_ne!(simulation.actors[&1].solver_target_z_q32, 0);
}

#[test]
fn rvo_allows_a_coarse_tree_hit_outside_candidate_relative_travel() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            Placement {
                type_name: "rhino".to_owned(),
                ..test_placement(0, 0, 0, 0)
            },
            test_placement(1, 0, 0, 100),
            test_placement(1, 1, 75, 0),
        ],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
    set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 100_000);
    set_actor_position(simulation.actors.get_mut(&3).unwrap(), 75_000, 0);
    let source = simulation.actors.get_mut(&1).unwrap();
    source.lock_target = Some(unit_target(2));
    source.motion = MotionState::Moving;
    simulation.rvo_counter = 3;
    simulation.step_rvo();
}

#[test]
fn zero_published_speed_still_snaps_a_tolerance_equal_rvo_delta() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 100)],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    let actor = simulation.actors.get_mut(&1).unwrap();
    actor.x_q32 = 1_717_060_204_994;
    actor.z_q32 = 1_696_431_970_300;
    actor.published_target_x_q32 = 1_717_060_248_001;
    actor.published_target_z_q32 = 1_696_431_622_651;
    actor.published_speed_q32 = 0;
    actor.lock_target = Some(unit_target(2));
    actor.backswing_finish_step = Some(10);
    simulation.actors.get_mut(&2).unwrap().life = 0;

    simulation.step_actor_rvo_position(1);

    let actor = &simulation.actors[&1];
    assert_eq!(actor.x_q32, actor.published_target_x_q32);
    assert_eq!(actor.z_q32, actor.published_target_z_q32);

    let actor = simulation.actors.get_mut(&1).unwrap();
    assert!(actor.rvo_stopped_snap_since_boundary);
    actor.motion = MotionState::Moving;
    actor.next_speed_q32 = space_to_q32(actor.stats.move_speed());
    actor.next_max_speed_q32 = actor.next_speed_q32;
    actor.solver_target_x_q32 = 1_717_060_204_994;
    actor.solver_target_z_q32 = 1_696_431_970_300;
    actor.solver_speed_q32 = 0;
    simulation.rvo_counter = 3;

    simulation.step_rvo();

    let actor = &simulation.actors[&1];
    assert_eq!(actor.published_target_x_q32, actor.x_q32);
    assert_eq!(actor.published_target_z_q32, actor.z_q32);
    assert!(!actor.rvo_stopped_snap_since_boundary);
}

#[test]
fn stopped_snap_reset_expires_on_a_moving_tick_before_the_rvo_boundary() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 100)],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    let actor = simulation.actors.get_mut(&1).unwrap();
    actor.motion = MotionState::Moving;
    actor.rvo_stopped_snap_since_boundary = true;
    actor.published_target_x_q32 = actor.x_q32;
    actor.published_target_z_q32 = actor.z_q32;
    actor.published_speed_q32 = 0;
    simulation.rvo_counter = 1;

    simulation.step_actor_rvo_position(1);

    assert!(!simulation.actors[&1].rvo_stopped_snap_since_boundary);
}

#[test]
fn snapshot_velocity_is_quantized_from_raw_agent_velocity() {
    let layout = CompiledLayout::of_units(
        1,
        vec![
            Placement {
                team: 0,
                unit_id: 1,
                formation_id: 1,
                formation_index: 0,
                type_name: "marksman".to_owned(),
                world_x: 0,
                world_z: -50,
                rotation: 0,
                rotated: false,
                corrections: Vec::new(),
            },
            Placement {
                team: 1,
                unit_id: 2,
                formation_id: 2,
                formation_index: 0,
                type_name: "arclight".to_owned(),
                world_x: 0,
                world_z: 100,
                rotation: 180_000,
                rotated: false,
                corrections: Vec::new(),
            },
        ],
    );
    let config = SimulationConfig::load().unwrap();
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

    assert_eq!(
        snapshot_velocity_q32(actor),
        (-198_556_428, -30_061_443_202)
    );
}

#[test]
fn rvo_pipeline_publishes_before_movement_consumes_velocity() {
    let layout = CompiledLayout::of_units(
        1,
        vec![
            Placement {
                team: 0,
                unit_id: 1,
                formation_id: 1,
                formation_index: 0,
                type_name: "marksman".to_owned(),
                world_x: 0,
                world_z: -50,
                rotation: 0,
                rotated: false,
                corrections: Vec::new(),
            },
            Placement {
                team: 1,
                unit_id: 2,
                formation_id: 2,
                formation_index: 0,
                type_name: "arclight".to_owned(),
                world_x: 0,
                world_z: 100,
                rotation: 180_000,
                rotated: false,
                corrections: Vec::new(),
            },
        ],
    );
    let config = SimulationConfig::load().unwrap();
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
            assert_eq!(snapshot_velocity_q32(arclight), (0, 0));
        } else if tick == 8 {
            assert_eq!((arclight.x, arclight.z), (initial.x, initial.z));
            assert_eq!(arclight.body_rotation, initial.body_rotation);
            assert_eq!(
                snapshot_velocity_q32(arclight),
                (-198_556_428, -30_061_443_202)
            );
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
fn first_split_rvo_tree_uses_the_zero_position_buffer() {
    let config = SimulationConfig::load().unwrap();
    let (_, layout) = crate::layout::compile_with_seed(
        include_bytes!("../../../../../tests/regression/rhino-vs-crawlers.yaml"),
        &config.units,
    )
    .unwrap();
    let mut simulation = Simulation::new(
        &layout,
        &config.units,
        &config.training_ground,
        1_787_748_319,
    )
    .unwrap();

    for step in 0..8 {
        simulation.step(step).unwrap();
    }

    assert_eq!(
        snapshot_velocity_q32(&simulation.actors[&11]),
        (10_146_579_184, -67_965_446_880)
    );
}

#[test]
fn rvo_boundary_recalculates_velocity_from_the_published_target_and_current_position() {
    let layout = CompiledLayout::of_units(
        1,
        vec![
            Placement {
                team: 0,
                unit_id: 1,
                formation_id: 1,
                formation_index: 0,
                type_name: "marksman".to_owned(),
                world_x: 0,
                world_z: -50,
                rotation: 0,
                rotated: false,
                corrections: Vec::new(),
            },
            Placement {
                team: 1,
                unit_id: 2,
                formation_id: 2,
                formation_index: 0,
                type_name: "arclight".to_owned(),
                world_x: 0,
                world_z: 100,
                rotation: 180_000,
                rotated: false,
                corrections: Vec::new(),
            },
        ],
    );
    let config = SimulationConfig::load().unwrap();
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
#[allow(clippy::too_many_lines)]
fn range_entry_stops_only_after_the_two_stage_rvo_delay() {
    let layout = CompiledLayout::of_units(
        1,
        vec![
            Placement {
                team: 0,
                unit_id: 1,
                formation_id: 1,
                formation_index: 0,
                type_name: "marksman".to_owned(),
                world_x: 0,
                world_z: -50,
                rotation: 0,
                rotated: false,
                corrections: Vec::new(),
            },
            Placement {
                team: 1,
                unit_id: 2,
                formation_id: 2,
                formation_index: 0,
                type_name: "arclight".to_owned(),
                world_x: 0,
                world_z: 100,
                rotation: 180_000,
                rotated: false,
                corrections: Vec::new(),
            },
        ],
    );
    let config = SimulationConfig::load().unwrap();
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
                assert_eq!(
                    snapshot_velocity_q32(arclight),
                    (
                        arclight.current_velocity_x_q32,
                        arclight.current_velocity_z_q32
                    )
                );
                tick_121_raw_position = Some((arclight.x_q32, arclight.z_q32));
                tick_121_body_rotation = Some(arclight.body_rotation);
            }
            122 => {
                assert_eq!(arclight.z, 60_800);
                assert_eq!(arclight.motion, MotionState::Attacking);
                assert_eq!(
                    snapshot_velocity_q32(arclight),
                    (
                        arclight.current_velocity_x_q32,
                        arclight.current_velocity_z_q32
                    )
                );
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
                assert_eq!(
                    snapshot_velocity_q32(arclight),
                    (
                        arclight.current_velocity_x_q32,
                        arclight.current_velocity_z_q32
                    )
                );
                if tick == 124 {
                    assert_eq!(arclight.published_speed_q32, space_to_q32(7_000));
                    assert_eq!(arclight.solver_speed_q32, 0);
                }
            }
            128 => {
                assert_eq!(arclight.z, 58_700);
                assert_eq!(arclight.motion, MotionState::Attacking);
                assert_eq!(snapshot_velocity_q32(arclight), (0, 0));
                assert_eq!(arclight.published_speed_q32, 0);
            }
            _ => {}
        }
    }
}

#[test]
fn stopped_attacker_rate_limits_aim_without_rotating_root_body() {
    let layout = CompiledLayout::of_units(
        1,
        vec![
            Placement {
                team: 0,
                unit_id: 1,
                formation_id: 1,
                formation_index: 0,
                type_name: "marksman".to_owned(),
                world_x: 0,
                world_z: -50,
                rotation: 0,
                rotated: false,
                corrections: Vec::new(),
            },
            Placement {
                team: 1,
                unit_id: 2,
                formation_id: 2,
                formation_index: 0,
                type_name: "arclight".to_owned(),
                world_x: 0,
                world_z: -100,
                rotation: 180_000,
                rotated: false,
                corrections: Vec::new(),
            },
        ],
    );
    let config = SimulationConfig::load().unwrap();
    let mut simulation = Simulation::new(
        &layout,
        &config.units,
        &config.training_ground,
        1_787_555_163,
    )
    .unwrap();
    let marksman = simulation.actors.get_mut(&1).unwrap();
    assert_eq!(marksman.rules.independent_aim, Some(false));
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
    let maximum = q32_mul(
        mdeg_to_degrees_q32(simulation.actors[&1].rules.rotate_speed_mdeg_per_second()),
        NATIVE_LOGIC_DELTA_Q32,
    );
    let expected_limited = degrees_q32_to_mdeg(rotate_towards_q32(
        mdeg_to_degrees_q32(initial_aim),
        mdeg_to_degrees_q32(expected_aim),
        maximum,
    ));
    simulation.step_actor(1, 1, &mut Vec::new()).unwrap();
    let marksman = &simulation.actors[&1];

    assert_eq!(marksman.motion, MotionState::Attacking);
    assert_eq!(marksman.body_rotation, root_body);
    assert_ne!(marksman.aim_rotation, initial_aim);
    assert_ne!(marksman.aim_rotation, expected_aim);
    assert_eq!(marksman.aim_rotation, expected_limited);
}
