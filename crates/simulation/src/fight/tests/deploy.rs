use super::*;

#[test]
fn initial_identity_uses_seeded_snapshot_coordinates_not_layout_centers() {
    let layout = CompiledLayout::of_units(
        1,
        vec![
            test_placement(0, 0, -20, -100),
            test_placement(0, 1, 20, -100),
            test_placement(1, 0, 0, 100),
        ],
    );
    let config = SimulationConfig::load().unwrap();
    let actors = initialize_actors(&layout, &config.units, 1_787_591_883).unwrap();
    let first = &actors[&1];
    let second = &actors[&2];
    assert_eq!(first.placement.team, 0);
    assert_eq!(second.placement.team, 0);
    assert_eq!(first.placement.formation_index, 1);
    assert_eq!(second.placement.formation_index, 0);
    assert!((first.z, first.x) < (second.z, second.x));
    assert_eq!(actors[&3].placement.team, 1);
    assert_eq!(first.placement.formation_id, 1);
    assert_eq!(second.placement.formation_id, 2);
    assert_eq!(actors[&3].placement.formation_id, 3);
}

#[test]
#[allow(clippy::too_many_lines)]
fn multi_formation_initial_state_and_target_search_entry_match_build_2259() {
    let layout = CompiledLayout::of_units(
        1,
        vec![
            Placement {
                team: 0,
                unit_id: 0,
                formation_id: 0,
                formation_index: 0,
                type_name: "rhino".to_owned(),
                world_x: -285,
                world_z: -105,
                rotation: 0,
                rotated: false,
                level: 1,
                corrections: Vec::new(),
            },
            Placement {
                team: 1,
                unit_id: 0,
                formation_id: 0,
                formation_index: 0,
                type_name: "arclight".to_owned(),
                world_x: -290,
                world_z: 100,
                rotation: 180_000,
                rotated: false,
                level: 1,
                corrections: Vec::new(),
            },
            Placement {
                team: 1,
                unit_id: 0,
                formation_id: 0,
                formation_index: 1,
                type_name: "arclight".to_owned(),
                world_x: -190,
                world_z: 100,
                rotation: 180_000,
                rotated: false,
                level: 1,
                corrections: Vec::new(),
            },
        ],
    );
    let config = SimulationConfig::load().unwrap();
    let actors = initialize_actors(&layout, &config.units, 1_787_601_811).unwrap();
    let actual = actors
        .iter()
        .map(|(&unit_id, actor)| {
            (
                unit_id,
                actor.placement.team,
                actor.placement.formation_id,
                actor.placement.type_name.as_str(),
                actor.x,
                actor.z,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        [
            (1, 0, 1, "rhino", -284_400, -104_900),
            (2, 1, 2, "arclight", -189_500, 99_300),
            (3, 1, 3, "arclight", -290_600, 99_900),
        ]
    );
    let make_simulation = || {
        let actors = actors.clone();
        let towers = Towers::load().unwrap();
        let InitialBuildings {
            states: buildings,
            unsearchable,
            colliders: construction_colliders,
            tower_losses,
            tower_buffed_constructions,
        } = initialize_buildings(&config.training_ground, &[], &BTreeMap::new(), &towers).unwrap();
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
            towers,
            tower_losses,
            tower_buffed_constructions,
        }
    };
    let mut simulation = make_simulation();
    assert_eq!(simulation.select_normal_unit_target(1).unwrap(), Some(3));
    simulation.initialize_presearch_targets().unwrap();
    assert_eq!(
        simulation.actors[&1].skill.lock_target,
        Some(unit_target(3))
    );
    assert_eq!(simulation.actors[&1].body_rotation, 358_219);
    assert_eq!(simulation.actors[&1].skill.search_target_time, 0);
    assert_eq!(simulation.actors[&2].skill.search_target_time, 1);
    assert_eq!(simulation.actors[&3].skill.search_target_time, 2);

    let mut fight_skill = make_simulation();
    fight_skill.initialize_presearch_targets().unwrap();
    let current = fight_skill.actors.get_mut(&3).unwrap();
    current.x = -100_000;
    current.z = 300_000;
    current.x_q32 = space_to_q32(current.x);
    current.z_q32 = space_to_q32(current.z);
    let source = fight_skill.actors.get_mut(&1).unwrap();
    source.skill.search_target_time = 1;
    fight_skill.refresh_target_query_snapshot();
    let target_search_order = fight_skill.target_search_order();
    fight_skill
        .update_fight_skill_target_search(FightActorRef::Unit(1), 0, &target_search_order)
        .unwrap();
    assert_eq!(
        fight_skill.actors[&1].skill.lock_target,
        Some(unit_target(3))
    );
    assert_eq!(fight_skill.actors[&1].skill.search_target_time, 0);
    fight_skill
        .update_fight_skill_target_search(FightActorRef::Unit(1), 1, &target_search_order)
        .unwrap();
    assert_eq!(
        fight_skill.actors[&1].skill.lock_target,
        Some(unit_target(2))
    );
    assert_eq!(
        fight_skill.actors[&1].skill.search_target_time,
        SEARCH_TARGET_RESET_TICKS
    );

    let mut hold_fire = make_simulation();
    hold_fire.initialize_presearch_targets().unwrap();
    let current = hold_fire.actors.get_mut(&3).unwrap();
    current.x = -100_000;
    current.z = 300_000;
    current.x_q32 = space_to_q32(current.x);
    current.z_q32 = space_to_q32(current.z);
    let source = hold_fire.actors.get_mut(&1).unwrap();
    source.skill.search_target_time = 0;
    source.motion.attack_hold_fire = true;
    hold_fire.refresh_target_query_snapshot();
    let target_search_order = hold_fire.target_search_order();
    hold_fire
        .update_fight_skill_target_search(FightActorRef::Unit(1), 0, &target_search_order)
        .unwrap();
    assert_eq!(hold_fire.actors[&1].skill.lock_target, Some(unit_target(2)));
    assert_eq!(
        hold_fire.actors[&1].skill.search_target_time,
        SEARCH_TARGET_RESET_TICKS
    );

    let mut attack_state = make_simulation();
    attack_state.initialize_presearch_targets().unwrap();
    let current = attack_state.actors.get_mut(&3).unwrap();
    current.x = -100_000;
    current.z = 300_000;
    current.x_q32 = space_to_q32(current.x);
    current.z_q32 = space_to_q32(current.z);
    let source = attack_state.actors.get_mut(&1).unwrap();
    source.skill.search_target_time = 0;
    source.skill.set_phase(FightSkillPhase::Attack);
    attack_state.refresh_target_query_snapshot();
    let target_search_order = attack_state.target_search_order();
    attack_state
        .update_fight_skill_target_search(FightActorRef::Unit(1), 0, &target_search_order)
        .unwrap();
    assert_eq!(
        attack_state.actors[&1].skill.lock_target,
        Some(unit_target(3))
    );
    assert_eq!(attack_state.actors[&1].skill.search_target_time, 0);

    let mut prepare_state = make_simulation();
    prepare_state.initialize_presearch_targets().unwrap();
    let current = prepare_state.actors.get_mut(&3).unwrap();
    current.x = -100_000;
    current.z = 300_000;
    current.x_q32 = space_to_q32(current.x);
    current.z_q32 = space_to_q32(current.z);
    let source = prepare_state.actors.get_mut(&1).unwrap();
    source.skill.search_target_time = 0;
    source
        .skill
        .set_phase(FightSkillPhase::Prepare { finish_step: 20 });
    prepare_state.refresh_target_query_snapshot();
    let target_search_order = prepare_state.target_search_order();
    prepare_state
        .update_fight_skill_target_search(FightActorRef::Unit(1), 0, &target_search_order)
        .unwrap();
    assert_eq!(
        prepare_state.actors[&1].skill.lock_target,
        Some(unit_target(3))
    );
    assert_eq!(prepare_state.actors[&1].skill.search_target_time, 0);

    let mut dead_target = make_simulation();
    dead_target.initialize_presearch_targets().unwrap();
    dead_target
        .actors
        .get_mut(&1)
        .unwrap()
        .skill
        .search_target_time = 10;
    dead_target.actors.get_mut(&3).unwrap().life = 0;
    dead_target.step_actor(1, 0, &mut Vec::new()).unwrap();
    assert_eq!(
        dead_target.actors[&1].skill.lock_target,
        Some(unit_target(2))
    );
}

#[test]
fn crawler_member_grid_and_jitter_follow_native_creation_order() {
    let config = SimulationConfig::load().unwrap();
    let rules = config.units.get("crawler").unwrap();
    let placement = Placement {
        team: 0,
        unit_id: 0,
        formation_id: 0,
        formation_index: 0,
        type_name: "crawler".to_owned(),
        world_x: 0,
        world_z: 0,
        rotation: 0,
        rotated: false,
        level: 1,
        corrections: Vec::new(),
    };
    let seed = 1_787_601_811;
    let positions = generate_formation_positions(&placement, rules, seed).unwrap();
    assert_eq!(positions.len(), 24);

    let mut random = GrRandom::new(i64::from(seed).cast_unsigned());
    let mut base_positions = Vec::new();
    for (x_q32, z_q32) in &positions {
        let jitter_x =
            i64::from(random.next_in_range(FORMATION_JITTER_RANGE_TENTHS)).saturating_mul(C0_1_RAW);
        let jitter_z =
            i64::from(random.next_in_range(FORMATION_JITTER_RANGE_TENTHS)).saturating_mul(C0_1_RAW);
        base_positions.push(((x_q32 - jitter_x) >> 32, (z_q32 - jitter_z) >> 32));
    }
    let row = [
        (-21, 7),
        (-15, 7),
        (-9, 7),
        (-3, 7),
        (3, 7),
        (9, 7),
        (15, 7),
        (21, 7),
    ];
    let expected = row
        .into_iter()
        .chain(row.map(|(x, _)| (x, 1)))
        .chain(row.map(|(x, _)| (x, -5)))
        .collect::<Vec<_>>();
    assert_eq!(base_positions, expected);

    let mut red = placement;
    red.team = 1;
    red.rotation = 180_000;
    let red_positions = generate_formation_positions(&red, rules, seed).unwrap();
    assert!(
        positions
            .iter()
            .zip(red_positions)
            .all(|(&(blue_x, blue_z), (red_x, red_z))| { (red_x, red_z) == (-blue_x, -blue_z) })
    );

    red.rotated = true;
    red.world_z = 105;
    let seed = 1_787_832_792;
    let rotated_positions = generate_formation_positions(&red, rules, seed).unwrap();
    let mut random = GrRandom::new(i64::from(seed).cast_unsigned());
    let base_offsets = rotated_positions
        .into_iter()
        .map(|(x_q32, z_q32)| {
            let jitter_x = i64::from(random.next_in_range(FORMATION_JITTER_RANGE_TENTHS))
                .saturating_mul(C0_1_RAW);
            let jitter_z = i64::from(random.next_in_range(FORMATION_JITTER_RANGE_TENTHS))
                .saturating_mul(C0_1_RAW);
            (
                (x_q32 + jitter_x) >> 32,
                (z_q32 - 105 * Q32_ONE + jitter_z) >> 32,
            )
        })
        .collect::<Vec<_>>();
    let expected = [-22, -16, -10, -4, 2, 8, 14, 20]
        .into_iter()
        .flat_map(|z| [(6, z), (0, z), (-6, z)])
        .collect::<Vec<_>>();
    assert_eq!(base_offsets, expected);
}

#[test]
fn hound_partial_last_row_preserves_native_q32_centering() {
    let config = SimulationConfig::load().unwrap();
    let rules = config.units.get("hound").unwrap();
    let placement = Placement {
        team: 0,
        unit_id: 0,
        formation_id: 0,
        formation_index: 0,
        type_name: "hound".to_owned(),
        world_x: 0,
        world_z: 0,
        rotation: 0,
        rotated: false,
        level: 1,
        corrections: Vec::new(),
    };
    let seed = 1_787_601_811;
    let positions = generate_formation_positions(&placement, rules, seed).unwrap();
    let mut random = GrRandom::new(i64::from(seed).cast_unsigned());
    let base_positions_q32 = positions
        .into_iter()
        .map(|(x_q32, z_q32)| {
            let jitter_x = i64::from(random.next_in_range(FORMATION_JITTER_RANGE_TENTHS))
                .saturating_mul(C0_1_RAW);
            let jitter_z = i64::from(random.next_in_range(FORMATION_JITTER_RANGE_TENTHS))
                .saturating_mul(C0_1_RAW);
            (x_q32 - jitter_x, z_q32 - jitter_z)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        base_positions_q32,
        [
            (-13_i64 << 32, 5_i64 << 32),
            (0, 5_i64 << 32),
            (13_i64 << 32, 5_i64 << 32),
            (-(13_i64 << 31), -5_i64 << 32),
            (13_i64 << 31, -5_i64 << 32),
        ]
    );
}

#[test]
fn multi_member_identity_is_assigned_after_generation_and_shared_by_formation() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![Placement {
            team: 0,
            unit_id: 0,
            formation_id: 0,
            formation_index: 0,
            type_name: "crawler".to_owned(),
            world_x: 5,
            world_z: -50,
            rotation: 0,
            rotated: false,
            level: 1,
            corrections: Vec::new(),
        }],
    );
    let actors = initialize_actors(&layout, &config.units, 1_787_601_811).unwrap();
    assert_eq!(actors.len(), 24);
    assert!(
        actors
            .iter()
            .all(|(&unit_id, actor)| unit_id == actor.placement.unit_id
                && actor.placement.formation_id == 1
                && actor.placement.formation_index == 0)
    );
    assert!(actors.values().map(|actor| (actor.z, actor.x)).is_sorted());
}

#[test]
fn negative_formation_seed_is_sign_extended_like_the_native_constructor() {
    let config = SimulationConfig::load().unwrap();
    let rules = config.units.get("arclight").unwrap().clone();
    let actor = Actor::new(test_placement(0, 0, 0, 0), rules, -1);

    let mut native_random = GrRandom::new(i64::from(-1_i32).cast_unsigned());
    let expected_x = i64::from(native_random.next_in_range(FORMATION_JITTER_RANGE_TENTHS))
        .saturating_mul(C0_1_RAW);
    let expected_z = i64::from(native_random.next_in_range(FORMATION_JITTER_RANGE_TENTHS))
        .saturating_mul(C0_1_RAW);
    let mut zero_extended = GrRandom::new(u64::from((-1_i32).cast_unsigned()));
    let wrong_x = i64::from(zero_extended.next_in_range(FORMATION_JITTER_RANGE_TENTHS))
        .saturating_mul(C0_1_RAW);
    let wrong_z = i64::from(zero_extended.next_in_range(FORMATION_JITTER_RANGE_TENTHS))
        .saturating_mul(C0_1_RAW);

    assert_eq!((actor.x_q32, actor.z_q32), (expected_x, expected_z));
    assert_ne!((expected_x, expected_z), (wrong_x, wrong_z));
}

#[test]
fn formation_seed_addition_wraps_at_the_native_i32_boundary() {
    let config = SimulationConfig::load().unwrap();
    let rules = config.units.get("arclight").unwrap().clone();
    let actor = Actor::new(test_placement(0, 1, 0, 0), rules, i32::MAX);

    let mut native_random = GrRandom::new(i64::from(i32::MIN).cast_unsigned());
    let expected_x = i64::from(native_random.next_in_range(FORMATION_JITTER_RANGE_TENTHS))
        .saturating_mul(C0_1_RAW);
    let expected_z = i64::from(native_random.next_in_range(FORMATION_JITTER_RANGE_TENTHS))
        .saturating_mul(C0_1_RAW);
    assert_eq!((actor.x_q32, actor.z_q32), (expected_x, expected_z));
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
                level: 1,
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
                level: 1,
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
fn first_rvo_solve_avoids_same_formation_at_tick_eight() {
    let config = SimulationConfig::load().unwrap();
    let (_, layout) = crate::layout::compile_with_seed(
        include_bytes!("../../../../../tests/regression/steel-balls-vs-steel-balls.yaml"),
        &config.units,
    )
    .unwrap();
    let mut simulation = Simulation::new(
        &layout,
        &config.units,
        &config.training_ground,
        1_787_831_322,
    )
    .unwrap();

    for step in 0..8 {
        simulation.step(step).unwrap();
    }

    assert_eq!(
        snapshot_velocity_q32(&simulation.actors[&1]),
        (-3_142_838_517, 68_510_718_647)
    );
    assert_eq!(
        snapshot_velocity_q32(&simulation.actors[&2]),
        (-1_768_777_992, 61_456_524_119)
    );
    assert_eq!(
        snapshot_velocity_q32(&simulation.actors[&3]),
        (2_054_176_502, 68_688_002_951)
    );
}

#[test]
fn tick_fifteen_aim_uses_raw_q32_positions() {
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
                level: 1,
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
                level: 1,
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
