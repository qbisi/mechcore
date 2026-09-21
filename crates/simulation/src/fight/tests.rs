use super::*;

fn unit_target(id: u64) -> FightActorRef {
    FightActorRef::Unit(id)
}

#[test]
fn target_quadtree_twentieth_insert_reverses_old_child_ordinals() {
    let positions = [
        (-100 * Q32_ONE, -100 * Q32_ONE),
        (100 * Q32_ONE, -100 * Q32_ONE),
        (-100 * Q32_ONE, 100 * Q32_ONE),
        (100 * Q32_ONE, 100 * Q32_ONE),
    ];
    let mut tree = TargetActorQuadtree::new();
    for id in 1..=19 {
        let (x, z) = positions[(usize::try_from(id).unwrap() - 1) % 4];
        tree.insert(unit_target(id), x, z, 0);
    }
    assert!(tree.root.children.is_none());
    tree.insert(unit_target(20), positions[3].0, positions[3].1, 0);

    let children = tree.root.children.as_ref().unwrap();
    assert_eq!(
        children[0].elements,
        [17, 13, 9, 5, 1].map(unit_target).to_vec()
    );
    assert_eq!(
        children[1].elements,
        [18, 14, 10, 6, 2].map(unit_target).to_vec()
    );
    assert_eq!(
        children[2].elements,
        [19, 15, 11, 7, 3].map(unit_target).to_vec()
    );
    assert_eq!(
        children[3].elements,
        [16, 12, 8, 4, 20].map(unit_target).to_vec()
    );
}

#[test]
fn target_quadtree_queries_parent_straddlers_before_children() {
    let mut tree = TargetActorQuadtree::new();
    tree.insert(unit_target(1), 0, 0, 1_000);
    for id in 2..=20 {
        tree.insert(unit_target(id), -100 * Q32_ONE, -100 * Q32_ONE, 0);
    }

    assert_eq!(tree.query_order().first(), Some(&unit_target(1)));
}

#[test]
fn target_quadtree_reinserts_a_moved_child_element_in_native_order() {
    let mut tree = TargetActorQuadtree::new();
    for id in 1..=20 {
        tree.insert(unit_target(id), -100 * Q32_ONE, -100 * Q32_ONE, 0);
    }
    tree.position_changed(unit_target(7), 100 * Q32_ONE, -100 * Q32_ONE, 0);

    let children = tree.root.children.as_ref().unwrap();
    assert!(!children[0].elements.contains(&unit_target(7)));
    assert_eq!(children[1].elements.last(), Some(&unit_target(7)));
}

fn test_placement(team: u32, formation_index: i32, world_x: i64, world_z: i64) -> Placement {
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
        corrections: Vec::new(),
    }
}

fn snapshot_velocity_q32(actor: &Actor) -> (i64, i64) {
    let velocity = actor.snapshot().velocity;
    (velocity.x, velocity.z)
}

fn raw_test_simulation(
    layout: &CompiledLayout,
    config: &SimulationConfig,
    seed: i32,
) -> Simulation {
    let actors = initialize_actors(layout, &config.units, seed).unwrap();
    let InitialBuildings {
        states: buildings,
        unsearchable,
        colliders: construction_colliders,
    } = initialize_buildings(&config.training_ground, &[]).unwrap();
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
    }
}

fn set_actor_position(actor: &mut Actor, x: i64, z: i64) {
    set_actor_position_q32(actor, space_to_q32(x), space_to_q32(z));
}

fn set_actor_position_q32(actor: &mut Actor, x_q32: i64, z_q32: i64) {
    actor.x_q32 = x_q32;
    actor.z_q32 = z_q32;
    actor.target_query_x_q32 = x_q32;
    actor.target_query_z_q32 = z_q32;
    actor.x = q32_to_space_rounded(x_q32);
    actor.z = q32_to_space_rounded(z_q32);
    actor.next_target_x_q32 = actor.x_q32;
    actor.next_target_z_q32 = actor.z_q32;
    actor.solver_target_x_q32 = actor.x_q32;
    actor.solver_target_z_q32 = actor.z_q32;
    actor.published_target_x_q32 = actor.x_q32;
    actor.published_target_z_q32 = actor.z_q32;
}

fn micrometers_to_q32(value: i64) -> i64 {
    i64::try_from(i128::from(value) * i128::from(Q32_ONE) / 1_000_000).unwrap()
}

#[test]
fn native_delta_distinguishes_1799_from_1800_time_units() {
    assert_eq!(native_time_units_to_steps(1_799), 17);
    assert_eq!(native_time_units_to_steps(1_800), 18);
}

#[test]
fn rotation_distance_uses_the_shortest_wrapped_arc() {
    assert_eq!(rotation_distance(359_000, 1_000), 2_000);
    assert_eq!(rotation_distance(1_000, 359_000), 2_000);
    assert_eq!(rotation_distance(20_000, 60_000), 40_000);
}

#[test]
fn exact_half_turn_uses_the_native_positive_direction() {
    let maximum = 6_i64 << 32;
    assert_eq!(
        rotate_towards_q32_unwrapped(0, 180_i64 << 32, maximum),
        maximum
    );
    assert_eq!(
        rotate_towards_q32_unwrapped(0, (180_i64 << 32) + 43, maximum),
        maximum
    );
    assert_eq!(
        rotate_towards_q32_unwrapped(0, (180_i64 << 32) + 44, maximum),
        -maximum
    );
    assert_eq!(
        rotate_towards_q32_unwrapped(180_i64 << 32, 0, maximum),
        186_i64 << 32
    );
}

#[test]
fn positive_body_rotation_exposes_the_exact_full_turn_for_one_tick() {
    let config = SimulationConfig::load().unwrap();
    let mut actor = Actor::new(
        test_placement(0, 0, 0, 0),
        config.units.get("crawler").unwrap().clone(),
        7,
    );
    actor.set_body_rotation(mdeg_to_degrees_q32(354_000));

    actor.rotate_body_towards(mdeg_to_degrees_q32(30_000));
    assert_eq!(degrees_q32_to_mdeg(actor.body_rotation_q32), 0);
    assert_eq!(actor.body_rotation, 360_000);

    actor.rotate_body_towards(mdeg_to_degrees_q32(30_000));
    assert_eq!(actor.body_rotation, 6_000);
}

#[test]
fn normal_target_score_uses_strict_minimum_and_maximum_range_edges() {
    let distance_q32 = 20_i64 << 32;
    let score = |min_range_q32, max_range_q32| {
        normal_visible_full_rotation_score_from_distance_and_angle_q32(
            distance_q32,
            0,
            min_range_q32,
            max_range_q32,
        )
    };
    let at_both_edges = score(distance_q32, distance_q32).unwrap();
    assert!(score(distance_q32 + 1, distance_q32).is_none());
    assert_eq!(
        score(0, distance_q32 - 1).unwrap(),
        at_both_edges.saturating_add(TARGET_SCORE_OUT_OF_RANGE_PENALTY_Q32)
    );
}

#[test]
fn normal_target_score_adds_raw_distance_after_weighted_term() {
    let distance_q32 = 20_i64 << 32;
    let angle_q32 = 50_i64 << 32;
    let angle_score_q32 = q32_mul(angle_q32, TARGET_SCORE_ANGLE_FACTOR_Q32);
    let expected_q32 = q32_mul(
        distance_q32,
        TARGET_SCORE_BASE_Q32.saturating_add(angle_score_q32),
    )
    .saturating_add(distance_q32);

    assert_eq!(
        normal_visible_full_rotation_score_from_distance_and_angle_q32(
            distance_q32,
            angle_q32,
            0,
            100_i64 << 32,
        ),
        Some(expected_q32)
    );
}

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
        let InitialBuildings {
            states: buildings,
            unsearchable,
            colliders: construction_colliders,
        } = initialize_buildings(&config.training_ground, &[]).unwrap();
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
        }
    };
    let mut simulation = make_simulation();
    assert_eq!(simulation.select_normal_unit_target(1).unwrap(), Some(3));
    simulation.initialize_presearch_targets().unwrap();
    assert_eq!(simulation.actors[&1].lock_target, Some(unit_target(3)));
    assert_eq!(simulation.actors[&1].body_rotation, 358_219);
    assert_eq!(simulation.actors[&1].fight_skill_search_target_time, 0);
    assert_eq!(simulation.actors[&2].fight_skill_search_target_time, 1);
    assert_eq!(simulation.actors[&3].fight_skill_search_target_time, 2);

    let mut fight_skill = make_simulation();
    fight_skill.initialize_presearch_targets().unwrap();
    let current = fight_skill.actors.get_mut(&3).unwrap();
    current.x = -100_000;
    current.z = 300_000;
    current.x_q32 = space_to_q32(current.x);
    current.z_q32 = space_to_q32(current.z);
    let source = fight_skill.actors.get_mut(&1).unwrap();
    source.fight_skill_search_target_time = 1;
    fight_skill.refresh_target_query_snapshot();
    let target_search_order = fight_skill.target_search_order();
    fight_skill
        .update_fight_skill_target_search(1, 0, &target_search_order)
        .unwrap();
    assert_eq!(fight_skill.actors[&1].lock_target, Some(unit_target(3)));
    assert_eq!(fight_skill.actors[&1].fight_skill_search_target_time, 0);
    fight_skill
        .update_fight_skill_target_search(1, 1, &target_search_order)
        .unwrap();
    assert_eq!(fight_skill.actors[&1].lock_target, Some(unit_target(2)));
    assert_eq!(
        fight_skill.actors[&1].fight_skill_search_target_time,
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
    source.fight_skill_search_target_time = 0;
    source.motion_attack_hold_fire = true;
    hold_fire.refresh_target_query_snapshot();
    let target_search_order = hold_fire.target_search_order();
    hold_fire
        .update_fight_skill_target_search(1, 0, &target_search_order)
        .unwrap();
    assert_eq!(hold_fire.actors[&1].lock_target, Some(unit_target(2)));
    assert_eq!(
        hold_fire.actors[&1].fight_skill_search_target_time,
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
    source.fight_skill_search_target_time = 0;
    source.fight_skill_phase = FightSkillPhase::Attack;
    attack_state.refresh_target_query_snapshot();
    let target_search_order = attack_state.target_search_order();
    attack_state
        .update_fight_skill_target_search(1, 0, &target_search_order)
        .unwrap();
    assert_eq!(attack_state.actors[&1].lock_target, Some(unit_target(3)));
    assert_eq!(attack_state.actors[&1].fight_skill_search_target_time, 0);

    let mut prepare_state = make_simulation();
    prepare_state.initialize_presearch_targets().unwrap();
    let current = prepare_state.actors.get_mut(&3).unwrap();
    current.x = -100_000;
    current.z = 300_000;
    current.x_q32 = space_to_q32(current.x);
    current.z_q32 = space_to_q32(current.z);
    let source = prepare_state.actors.get_mut(&1).unwrap();
    source.fight_skill_search_target_time = 0;
    source.fight_skill_phase = FightSkillPhase::Prepare { finish_step: 20 };
    source.pending = Some(PendingRelease {
        step: 20,
        target: unit_target(3),
    });
    prepare_state.refresh_target_query_snapshot();
    let target_search_order = prepare_state.target_search_order();
    prepare_state
        .update_fight_skill_target_search(1, 0, &target_search_order)
        .unwrap();
    assert_eq!(prepare_state.actors[&1].lock_target, Some(unit_target(3)));
    assert_eq!(prepare_state.actors[&1].fight_skill_search_target_time, 0);

    let mut dead_target = make_simulation();
    dead_target.initialize_presearch_targets().unwrap();
    dead_target
        .actors
        .get_mut(&1)
        .unwrap()
        .fight_skill_search_target_time = 10;
    dead_target.actors.get_mut(&3).unwrap().life = 0;
    dead_target.step_actor(1, 0, &mut Vec::new()).unwrap();
    assert_eq!(dead_target.actors[&1].lock_target, Some(unit_target(2)));
}

#[test]
fn normal_selector_split_query_remains_order_independent_for_a_unique_best() {
    let config = SimulationConfig::load().unwrap();
    let mut placements = vec![Placement {
        team: 0,
        unit_id: 0,
        formation_id: 0,
        formation_index: 0,
        type_name: "rhino".to_owned(),
        world_x: 0,
        world_z: -100,
        rotation: 0,
        rotated: false,
        corrections: Vec::new(),
    }];
    placements.extend((0_i32..18).map(|index| Placement {
        team: 1,
        unit_id: 0,
        formation_id: 0,
        formation_index: index,
        type_name: "arclight".to_owned(),
        world_x: i64::from(index) * 20 - 170,
        world_z: 100,
        rotation: 180_000,
        rotated: false,
        corrections: Vec::new(),
    }));
    let layout = CompiledLayout::of_units(1, placements);
    let actors = initialize_actors(&layout, &config.units, 7).unwrap();
    let InitialBuildings {
        states: buildings,
        unsearchable,
        colliders: construction_colliders,
    } = initialize_buildings(&config.training_ground, &[]).unwrap();
    let target_quadtrees = initialize_target_quadtrees(&actors, &buildings);
    let simulation = Simulation {
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
    };
    assert_eq!(simulation.select_normal_unit_target(1).unwrap(), Some(10));
}

#[test]
fn normal_selector_refuses_a_building_best_candidate() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 100)],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    let source = simulation.actors.get_mut(&1).unwrap();
    set_actor_position(source, 0, 0);
    source.set_body_rotation(0);
    set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 100_000);
    let building = simulation
        .buildings
        .iter_mut()
        .find(|building| building.team_id == 1)
        .unwrap();
    building.position = point(0, 20_000);
    let error = simulation
        .select_normal_unit_target(1)
        .unwrap_err()
        .to_string();
    assert!(error.contains("selector chose building"));
}

#[test]
fn fight_skill_adopts_a_selected_building_and_enters_moving() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            Placement {
                type_name: "steel_ball".to_owned(),
                ..test_placement(0, 0, 0, 0)
            },
            test_placement(1, 0, 0, 200),
        ],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
    set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 200_000);
    let source = simulation.actors.get_mut(&1).unwrap();
    source.lock_target = None;
    source.fight_skill_search_target_time = 0;
    source.motion = MotionState::Idle;
    let building = simulation
        .buildings
        .iter_mut()
        .find(|building| building.team_id == 1)
        .unwrap();
    building.position = point(0, 100_000);
    let building_id = building.building_id;

    simulation.step_actor(1, 0, &mut Vec::new()).unwrap();

    let source = &simulation.actors[&1];
    assert_eq!(source.motion, MotionState::Moving);
    assert_eq!(
        source.lock_target,
        Some(FightActorRef::Building(building_id))
    );
    assert!(!source.lock_is_terminal_handoff);
    assert_eq!(
        source.snapshot().mech_lock_target,
        Some(ObjectRef::new(ObjectKind::Building, building_id))
    );

    let other_building_id = simulation
        .buildings
        .iter()
        .find(|building| building.team_id == 1 && building.building_id != building_id)
        .unwrap()
        .building_id;
    for building in simulation
        .buildings
        .iter_mut()
        .filter(|building| building.team_id == 1)
    {
        building.position = if building.building_id == building_id {
            point(0, 300_000)
        } else {
            point(0, 100_000)
        };
    }

    for step in 1..=10 {
        simulation.step_actor(1, step, &mut Vec::new()).unwrap();
        assert_eq!(
            simulation.actors[&1].lock_target,
            Some(FightActorRef::Building(building_id))
        );
    }
    simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

    assert_eq!(
        simulation.actors[&1].lock_target,
        Some(FightActorRef::Building(other_building_id))
    );
}

#[test]
fn normal_selector_keeps_the_first_native_quadtree_candidate_on_equal_score() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            test_placement(0, 0, 0, 0),
            test_placement(1, 0, -20, 100),
            test_placement(1, 1, 20, 100),
        ],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    let source = simulation.actors.get_mut(&1).unwrap();
    set_actor_position(source, 0, 0);
    source.set_body_rotation(0);
    set_actor_position(simulation.actors.get_mut(&2).unwrap(), -20_000, 100_000);
    set_actor_position(simulation.actors.get_mut(&3).unwrap(), 20_000, 100_000);
    assert_eq!(simulation.select_normal_unit_target(1).unwrap(), Some(2));
}

#[test]
fn normal_selector_scores_the_start_of_tick_snapshot() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            test_placement(0, 0, 0, 0),
            test_placement(1, 0, 0, 100),
            test_placement(1, 1, 20, 100),
        ],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
    set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 20_000);
    set_actor_position(simulation.actors.get_mut(&3).unwrap(), 0, 40_000);
    assert_eq!(simulation.select_normal_unit_target(1).unwrap(), Some(2));

    let current = simulation.actors.get_mut(&2).unwrap();
    current.x_q32 = 0;
    current.z_q32 = space_to_q32(100_000);
    let current = simulation.actors.get_mut(&3).unwrap();
    current.x_q32 = 0;
    current.z_q32 = space_to_q32(10_000);

    assert_eq!(simulation.select_normal_unit_target(1).unwrap(), Some(2));
}

#[test]
fn fight_skill_selector_scores_the_weapon_rotation_instead_of_the_root_body() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            test_placement(0, 0, 0, 0),
            test_placement(1, 0, -8, 94),
            test_placement(1, 1, 3, 96),
        ],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    set_actor_position(simulation.actors.get_mut(&1).unwrap(), 298, -64_312);
    set_actor_position(simulation.actors.get_mut(&2).unwrap(), -8_193, 29_690);
    set_actor_position(simulation.actors.get_mut(&3).unwrap(), 3_679, 31_889);
    let source = simulation.actors.get_mut(&1).unwrap();
    source.set_body_rotation(mdeg_to_degrees_q32(2_680));
    source.set_weapon_rotation(mdeg_to_degrees_q32(358_902));
    simulation.refresh_target_query_snapshot();

    assert_eq!(simulation.select_normal_unit_target(1).unwrap(), Some(2));

    simulation
        .actors
        .get_mut(&1)
        .unwrap()
        .target_query_source_rotation_q32 = mdeg_to_degrees_q32(2_680);
    assert_eq!(simulation.select_normal_unit_target(1).unwrap(), Some(3));

    simulation.actors.get_mut(&1).unwrap().rules.has_body = false;
    simulation.refresh_target_query_snapshot();
    assert_eq!(simulation.select_normal_unit_target(1).unwrap(), Some(3));
}

#[test]
fn grouped_bodyless_selector_scores_the_first_weapon_rotation() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            Placement {
                type_name: "wraith".to_owned(),
                ..test_placement(0, 0, 0, 0)
            },
            test_placement(1, 0, -8, 94),
            test_placement(1, 1, 3, 96),
        ],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    set_actor_position(simulation.actors.get_mut(&1).unwrap(), 298, -64_312);
    set_actor_position(simulation.actors.get_mut(&2).unwrap(), -8_193, 29_690);
    set_actor_position(simulation.actors.get_mut(&3).unwrap(), 3_679, 31_889);
    let source = simulation.actors.get_mut(&1).unwrap();
    source.set_body_rotation(mdeg_to_degrees_q32(2_680));
    source.set_weapon_rotation(mdeg_to_degrees_q32(358_902));

    simulation.refresh_target_query_snapshot();

    assert_eq!(simulation.select_normal_unit_target(1).unwrap(), Some(2));
}

#[test]
fn grouped_child_search_scores_the_owner_root_rotation() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            Placement {
                type_name: "wraith".to_owned(),
                ..test_placement(0, 0, 0, 0)
            },
            test_placement(1, 0, -8, 94),
            test_placement(1, 1, 3, 96),
        ],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    set_actor_position(simulation.actors.get_mut(&1).unwrap(), 298, -64_312);
    set_actor_position(simulation.actors.get_mut(&2).unwrap(), -8_193, 29_690);
    set_actor_position(simulation.actors.get_mut(&3).unwrap(), 3_679, 31_889);
    let source = simulation.actors.get_mut(&1).unwrap();
    source.set_body_rotation(mdeg_to_degrees_q32(2_680));
    source.set_weapon_rotation(mdeg_to_degrees_q32(358_902));
    simulation.refresh_target_query_snapshot();
    let order = simulation.target_search_order();

    let ranked = simulation
        .rank_group_unit_targets_with_order(1, &order, false)
        .unwrap();

    assert_eq!(ranked.first(), Some(&3));
}

#[test]
fn has_body_attack_replacement_outside_range_exits_through_one_idle_tick() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            test_placement(0, 0, 0, 0),
            test_placement(1, 0, 0, 20),
            test_placement(1, 1, 0, 200),
        ],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    simulation.actors.get_mut(&2).unwrap().life = 0;
    let source = simulation.actors.get_mut(&1).unwrap();
    source.motion = MotionState::Attacking;
    source.lock_target = Some(unit_target(2));
    source.fight_skill_phase = FightSkillPhase::Attack;
    source.set_weapon_rotation(mdeg_to_degrees_q32(4_924));
    source.aim_rotation = 4_924;

    simulation.step_actor(1, 1, &mut Vec::new()).unwrap();
    assert_eq!(simulation.actors[&1].motion, MotionState::Idle);
    assert_eq!(simulation.actors[&1].lock_target, None);

    simulation.step_actor(1, 2, &mut Vec::new()).unwrap();
    assert_eq!(simulation.actors[&1].motion, MotionState::Moving);
    assert_eq!(simulation.actors[&1].lock_target, Some(unit_target(3)));
    assert_eq!(simulation.actors[&1].aim_rotation, 4_924);
}

#[test]
fn normal_quick_switch_only_adopts_an_immediately_attackable_target() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            Placement {
                type_name: "stormcaller".to_owned(),
                ..test_placement(0, 0, 0, 0)
            },
            test_placement(1, 0, 0, 60),
            test_placement(1, 1, 0, 100),
        ],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    for building in &mut simulation.buildings {
        building.targetable = false;
    }
    for actor in simulation
        .actors
        .values_mut()
        .filter(|actor| actor.placement.team == 1)
    {
        set_actor_position(actor, 0, 250_000);
    }
    set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
    set_actor_position(simulation.actors.get_mut(&5).unwrap(), 0, 60_000);
    set_actor_position(simulation.actors.get_mut(&6).unwrap(), 0, 100_000);
    let source = simulation.actors.get_mut(&1).unwrap();
    source.motion = MotionState::Attacking;
    source.lock_target = Some(unit_target(5));
    source.fight_skill_phase = FightSkillPhase::Attack;
    simulation.refresh_target_query_snapshot();
    let target_search_order = simulation.target_search_order();

    assert!(
        !simulation
            .quick_switch_active_target_outside_attack_area(1, 1, &target_search_order, false)
            .unwrap()
    );
    assert_eq!(simulation.actors[&1].lock_target, Some(unit_target(6)));

    let source = simulation.actors.get_mut(&1).unwrap();
    source.motion = MotionState::Attacking;
    source.lock_target = Some(unit_target(5));
    source.fight_skill_phase = FightSkillPhase::Attack;
    simulation.refresh_target_query_snapshot();
    simulation.actors.get_mut(&5).unwrap().life = 0;
    let target_search_order = simulation.target_search_order();

    assert!(
        !simulation
            .quick_switch_active_target_outside_attack_area(1, 2, &target_search_order, false)
            .unwrap()
    );
    assert_eq!(simulation.actors[&1].lock_target, Some(unit_target(6)));

    set_actor_position(simulation.actors.get_mut(&6).unwrap(), 0, 250_000);
    simulation.actors.get_mut(&5).unwrap().life = 263;
    let source = simulation.actors.get_mut(&1).unwrap();
    source.motion = MotionState::Attacking;
    source.lock_target = Some(unit_target(5));
    source.fight_skill_phase = FightSkillPhase::Attack;
    source.pending = Some(PendingRelease {
        step: 3,
        target: unit_target(5),
    });
    simulation.refresh_target_query_snapshot();
    let target_search_order = simulation.target_search_order();

    assert!(
        simulation
            .quick_switch_active_target_outside_attack_area(1, 3, &target_search_order, false)
            .unwrap()
    );
    assert_eq!(simulation.actors[&1].lock_target, None);
    assert_eq!(simulation.actors[&1].motion, MotionState::Idle);
    assert!(simulation.actors[&1].pending.is_none());

    let source = simulation.actors.get_mut(&1).unwrap();
    source.motion = MotionState::Idle;
    source.lock_target = Some(unit_target(5));
    source.fight_skill_phase = FightSkillPhase::Idle;
    simulation.actors.get_mut(&5).unwrap().life = 0;

    assert!(
        simulation
            .quick_switch_active_target_outside_attack_area(1, 4, &target_search_order, true)
            .unwrap()
    );
    assert_eq!(simulation.actors[&1].lock_target, None);
    assert_eq!(simulation.actors[&1].motion, MotionState::Idle);
}

#[test]
fn native_hundredth_constant_is_not_rationally_rounded() {
    assert_eq!(C0_01_RAW, 42_949_672);
    assert_eq!(q32_mul(30_i64 << 32, C0_01_RAW), 1_288_490_160);
    assert_ne!(C0_01_RAW, q32_div(Q32_ONE, 100_i64 << 32));
}

#[test]
fn has_body_attack_replacement_outside_weapon_angle_exits_through_idle() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            test_placement(0, 0, 0, 0),
            test_placement(1, 0, 0, 20),
            test_placement(1, 1, 0, 50),
        ],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    simulation.actors.get_mut(&2).unwrap().life = 0;
    let source = simulation.actors.get_mut(&1).unwrap();
    source.motion = MotionState::Attacking;
    source.lock_target = Some(unit_target(2));
    source.fight_skill_phase = FightSkillPhase::Attack;
    source.set_weapon_rotation(mdeg_to_degrees_q32(90_000));

    simulation.step_actor(1, 1, &mut Vec::new()).unwrap();

    assert_eq!(simulation.actors[&1].motion, MotionState::Idle);
    assert_eq!(simulation.actors[&1].lock_target, None);
}

#[test]
fn entering_attack_defers_weapon_tracking_until_the_next_tick() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 50, 0)],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    let initial_rotation = mdeg_to_degrees_q32(168_143);
    let source = simulation.actors.get_mut(&1).unwrap();
    source.lock_target = Some(unit_target(2));
    source.set_weapon_rotation(initial_rotation);
    source.aim_rotation = 168_143;

    simulation.step_actor(1, 1, &mut Vec::new()).unwrap();
    assert_eq!(simulation.actors[&1].motion, MotionState::Attacking);
    assert_eq!(
        simulation.actors[&1].weapon_rotations_q32[0],
        initial_rotation
    );
    assert_eq!(simulation.actors[&1].aim_rotation, 168_143);

    simulation.step_actor(1, 2, &mut Vec::new()).unwrap();
    assert_ne!(
        simulation.actors[&1].weapon_rotations_q32[0],
        initial_rotation
    );
    assert_ne!(simulation.actors[&1].aim_rotation, 168_143);
}

#[test]
fn same_tick_target_death_scores_live_candidate_positions() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            test_placement(0, 0, 0, 0),
            test_placement(1, 0, 0, 20),
            test_placement(1, 1, 0, 40),
            test_placement(1, 2, 0, 60),
        ],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
    set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 20_000);
    set_actor_position(simulation.actors.get_mut(&3).unwrap(), 0, 40_000);
    set_actor_position(simulation.actors.get_mut(&4).unwrap(), 0, 60_000);
    let source = simulation.actors.get_mut(&1).unwrap();
    source.lock_target = Some(unit_target(2));
    source.fight_skill_search_target_time = 10;
    simulation.refresh_target_query_snapshot();
    let target_search_order = simulation.target_search_order();

    simulation.actors.get_mut(&2).unwrap().life = 0;
    simulation.actors.get_mut(&3).unwrap().z_q32 = space_to_q32(100_000);
    simulation.actors.get_mut(&4).unwrap().z_q32 = space_to_q32(10_000);
    simulation
        .update_fight_skill_target_search(1, 1, &target_search_order)
        .unwrap();

    assert_eq!(simulation.actors[&1].lock_target, Some(unit_target(4)));

    let mut simulation = raw_test_simulation(&layout, &config, 7);
    set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
    set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 20_000);
    set_actor_position(simulation.actors.get_mut(&3).unwrap(), 0, 40_000);
    set_actor_position(simulation.actors.get_mut(&4).unwrap(), 0, 60_000);
    simulation.refresh_target_query_snapshot();
    let target_search_order = simulation.target_search_order();
    simulation.actors.get_mut(&2).unwrap().life = 0;
    simulation.actors.get_mut(&3).unwrap().z_q32 = space_to_q32(100_000);
    simulation.actors.get_mut(&4).unwrap().z_q32 = space_to_q32(10_000);
    simulation
        .update_fight_skill_target_search(1, 1, &target_search_order)
        .unwrap();

    assert_eq!(simulation.actors[&1].lock_target, Some(unit_target(4)));
}

#[test]
fn completed_bodyless_melee_attack_uses_one_idle_tick_before_reapproach() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 100)],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
    set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 100_000);
    let source = simulation.actors.get_mut(&1).unwrap();
    source.describe(config.units.get("crawler").unwrap().clone());
    source.lock_target = Some(unit_target(2));
    source.motion = MotionState::Moving;
    source.backswing_finish_step = Some(10);
    source.set_body_rotation(mdeg_to_degrees_q32(123_000));

    simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

    let source = &simulation.actors[&1];
    assert_eq!(source.motion, MotionState::Idle);
    assert_eq!(source.lock_target, None);
    assert_eq!(source.body_rotation, 123_000);
    assert_eq!((source.next_target_x_q32, source.next_target_z_q32), (0, 0));
}

#[test]
fn completed_bodyless_melee_attack_uses_idle_before_an_out_of_angle_reentry() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 5)],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
    set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 5_000);
    let source = simulation.actors.get_mut(&1).unwrap();
    source.describe(config.units.get("crawler").unwrap().clone());
    source.lock_target = Some(unit_target(2));
    source.motion = MotionState::Moving;
    source.fight_skill_phase = FightSkillPhase::Attack;
    source.backswing_finish_step = Some(10);
    source.set_body_rotation(mdeg_to_degrees_q32(90_000));

    simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

    let source = &simulation.actors[&1];
    assert_eq!(source.motion, MotionState::Idle);
    assert_eq!(source.lock_target, None);
    assert_eq!(source.body_rotation, 90_000);
    assert_eq!((source.next_target_x_q32, source.next_target_z_q32), (0, 0));
}

#[test]
fn allied_kill_during_backswing_retains_the_dead_target_until_finish() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            test_placement(0, 0, 0, 0),
            test_placement(0, 1, 0, 10),
            test_placement(1, 0, 0, 100),
            test_placement(1, 1, 0, 110),
        ],
    );
    for type_name in ["crawler", "wasp"] {
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.describe(config.units.get(type_name).unwrap().clone());
        source.lock_target = Some(unit_target(3));
        source.motion = MotionState::Attacking;
        source.fight_skill_phase = FightSkillPhase::Attack;
        source.backswing_finish_step = Some(10);
        source.next_attack_step = 20;
        simulation.actors.get_mut(&3).unwrap().life = 0;

        simulation.step_actor(1, 8, &mut Vec::new()).unwrap();
        let stop_target = {
            let source = &simulation.actors[&1];
            assert_eq!(source.motion, MotionState::Idle, "{type_name}");
            assert_eq!(source.lock_target, Some(unit_target(3)), "{type_name}");
            assert_eq!(source.backswing_finish_step, Some(10), "{type_name}");
            (source.next_target_x_q32, source.next_target_z_q32)
        };
        simulation.actors.get_mut(&1).unwrap().x_q32 += Q32_ONE;

        for step in 9..=10 {
            simulation.step_actor(1, step, &mut Vec::new()).unwrap();
            let source = &simulation.actors[&1];
            assert_eq!(source.motion, MotionState::Idle, "{type_name}");
            assert_eq!(source.lock_target, Some(unit_target(3)), "{type_name}");
            assert_eq!(source.backswing_finish_step, Some(10), "{type_name}");
            assert_eq!(
                (source.next_target_x_q32, source.next_target_z_q32),
                stop_target,
                "{type_name}"
            );
        }

        simulation.step_actor(1, 11, &mut Vec::new()).unwrap();
        let source = &simulation.actors[&1];
        assert_eq!(source.motion, MotionState::Idle, "{type_name}");
        assert_eq!(source.lock_target, None, "{type_name}");
        assert_eq!(source.backswing_finish_step, None, "{type_name}");
        assert_eq!(
            (source.next_target_x_q32, source.next_target_z_q32),
            stop_target,
            "{type_name}"
        );
    }
}

#[test]
fn final_same_tick_allied_kill_retains_the_dead_backswing_target() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 20)],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    let source = simulation.actors.get_mut(&1).unwrap();
    source.describe(config.units.get("crawler").unwrap().clone());
    source.lock_target = Some(unit_target(2));
    source.motion = MotionState::Attacking;
    source.fight_skill_phase = FightSkillPhase::Attack;
    source.backswing_finish_step = Some(20);
    simulation.refresh_target_query_snapshot();
    let target_search_order = simulation.target_search_order();
    simulation.actors.get_mut(&2).unwrap().life = 0;

    simulation
        .step_actor_with_target_order(1, 10, &target_search_order, &mut Vec::new())
        .unwrap();

    let source = &simulation.actors[&1];
    assert_eq!(source.motion, MotionState::Idle);
    assert_eq!(source.lock_target, Some(unit_target(2)));
    assert_eq!(source.backswing_finish_step, Some(20));
}

#[test]
fn later_final_enemy_death_does_not_clear_an_own_kill_backswing_target() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            test_placement(0, 0, 0, 0),
            test_placement(1, 0, 0, 20),
            test_placement(1, 1, 0, 100),
        ],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    let source = simulation.actors.get_mut(&1).unwrap();
    source.describe(config.units.get("crawler").unwrap().clone());
    source.lock_target = Some(unit_target(2));
    source.motion = MotionState::Idle;
    source.fight_skill_phase = FightSkillPhase::Attack;
    source.retarget_after_own_direct_kill = true;
    source.backswing_finish_step = Some(20);
    simulation.actors.get_mut(&2).unwrap().life = 0;

    simulation.step_actor(1, 10, &mut Vec::new()).unwrap();
    assert_eq!(simulation.actors[&1].lock_target, Some(unit_target(2)));

    simulation.actors.get_mut(&3).unwrap().life = 0;
    simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

    let source = &simulation.actors[&1];
    assert_eq!(source.motion, MotionState::Idle);
    assert_eq!(source.lock_target, Some(unit_target(2)));
    assert_eq!(source.backswing_finish_step, Some(20));
}

#[test]
fn bodyless_attack_defers_a_dead_target_replacement_outside_attack_area() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            test_placement(0, 0, 0, 0),
            test_placement(1, 0, 0, 20),
            test_placement(1, 1, 0, 100),
        ],
    );
    for type_name in ["crawler", "fang"] {
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.describe(config.units.get(type_name).unwrap().clone());
        source.lock_target = Some(unit_target(2));
        source.motion = MotionState::Attacking;
        source.fight_skill_phase = FightSkillPhase::Idle;
        source.motion_attack_hold_fire = false;
        source.next_attack_step = 20;
        simulation.actors.get_mut(&2).unwrap().life = 0;

        simulation.step_actor(1, 10, &mut Vec::new()).unwrap();
        let source = &simulation.actors[&1];
        assert_eq!(source.motion, MotionState::Idle, "{type_name}");
        assert_eq!(source.lock_target, None, "{type_name}");
        assert_eq!(source.next_attack_step, 20, "{type_name}");

        simulation.step_actor(1, 11, &mut Vec::new()).unwrap();
        assert_eq!(
            simulation.actors[&1].lock_target,
            Some(unit_target(3)),
            "{type_name}"
        );
    }
}

#[test]
fn bodyless_in_range_turn_barrier_preserves_attack_timing() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            test_placement(0, 0, 0, 0),
            test_placement(1, 0, 0, -50),
            test_placement(1, 1, 0, -40),
        ],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    let source = simulation.actors.get_mut(&1).unwrap();
    source.describe(config.units.get("fang").unwrap().clone());
    source.lock_target = Some(unit_target(2));
    source.motion = MotionState::Attacking;
    source.fight_skill_phase = FightSkillPhase::Idle;
    source.next_attack_step = 20;
    simulation.actors.get_mut(&2).unwrap().life = 0;
    assert!(simulation.bodyless_target_in_attack_range(1, unit_target(3)));
    assert!(!simulation.bodyless_target_in_attack_angle(1, unit_target(3)));

    simulation.step_actor(1, 10, &mut Vec::new()).unwrap();

    let source = &simulation.actors[&1];
    assert_eq!(source.lock_target, None);
    assert_eq!(source.motion, MotionState::Idle);
    assert_eq!(source.next_attack_step, 20);
}

#[test]
fn bodyless_projectile_idle_entry_holds_for_turning() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![test_placement(0, 0, 0, 0), test_placement(1, 0, -20, 50)],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    let source = simulation.actors.get_mut(&1).unwrap();
    source.describe(config.units.get("fang").unwrap().clone());
    source.lock_target = Some(unit_target(2));
    source.motion = MotionState::Idle;

    simulation.step_actor(1, 10, &mut Vec::new()).unwrap();

    let source = &simulation.actors[&1];
    assert_eq!(source.motion, MotionState::Attacking);
    assert!(source.motion_attack_hold_fire);
    assert_eq!(source.fight_skill_phase, FightSkillPhase::Idle);
}

#[test]
fn bodyless_quick_switch_replaces_a_dead_target_immediately_in_range() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            test_placement(0, 0, 0, 0),
            test_placement(1, 0, 0, 20),
            test_placement(1, 1, 0, 40),
        ],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    simulation.team_random.insert(0, GrRandom::new(7));
    let source = simulation.actors.get_mut(&1).unwrap();
    source.describe(config.units.get("wasp").unwrap().clone());
    source.lock_target = Some(unit_target(2));
    source.motion = MotionState::Attacking;
    source.fight_skill_phase = FightSkillPhase::Attack;
    source.next_attack_step = 10;
    source.backswing_finish_step = Some(20);
    simulation.actors.get_mut(&2).unwrap().life = 0;

    simulation.step_actor(1, 10, &mut Vec::new()).unwrap();

    assert_eq!(simulation.actors[&1].lock_target, Some(unit_target(2)));
    assert_eq!(simulation.actors[&1].backswing_finish_step, Some(20));

    let mut events = Vec::new();
    simulation.step_actor(1, 11, &mut events).unwrap();

    assert_eq!(simulation.actors[&1].lock_target, Some(unit_target(3)));
    assert_eq!(simulation.actors[&1].backswing_finish_step, Some(41));
    assert!(
        events
            .iter()
            .any(|event| { matches!(event.payload, EventPayload::ProjectileReleased { .. }) })
    );
}

#[test]
fn bodyless_non_quick_switch_defers_an_in_range_dead_target_replacement() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            test_placement(0, 0, 0, 0),
            test_placement(1, 0, 0, 2),
            test_placement(1, 1, 0, 4),
        ],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    let source = simulation.actors.get_mut(&1).unwrap();
    source.describe(config.units.get("crawler").unwrap().clone());
    source.lock_target = Some(unit_target(2));
    source.motion = MotionState::Attacking;
    source.fight_skill_phase = FightSkillPhase::Idle;
    simulation.actors.get_mut(&2).unwrap().life = 0;

    simulation.step_actor(1, 10, &mut Vec::new()).unwrap();
    assert_eq!(simulation.actors[&1].lock_target, None);

    simulation.step_actor(1, 11, &mut Vec::new()).unwrap();
    assert_eq!(simulation.actors[&1].lock_target, Some(unit_target(3)));
}

#[test]
fn bodyless_melee_attack_motion_exits_through_idle_when_target_leaves_range() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 100)],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
    set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 100_000);
    let source = simulation.actors.get_mut(&1).unwrap();
    source.describe(config.units.get("crawler").unwrap().clone());
    source.lock_target = Some(unit_target(2));
    source.motion = MotionState::Attacking;
    source.set_body_rotation(mdeg_to_degrees_q32(123_000));

    simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

    let source = &simulation.actors[&1];
    assert_eq!(source.motion, MotionState::Idle);
    assert_eq!(source.lock_target, None);
    assert_eq!(source.body_rotation, 123_000);
    assert_eq!((source.next_target_x_q32, source.next_target_z_q32), (0, 0));
}

#[test]
fn bodyless_projectile_attack_motion_exits_through_idle_when_target_leaves_range() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 100)],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
    set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 100_000);
    let source = simulation.actors.get_mut(&1).unwrap();
    source.describe(config.units.get("fang").unwrap().clone());
    source.lock_target = Some(unit_target(2));
    source.motion = MotionState::Attacking;
    source.fight_skill_phase = FightSkillPhase::Attack;
    source.set_body_rotation(mdeg_to_degrees_q32(123_000));

    simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

    let source = &simulation.actors[&1];
    assert_eq!(source.motion, MotionState::Idle);
    assert_eq!(source.lock_target, None);
    assert_eq!(source.fight_skill_phase, FightSkillPhase::Idle);
    assert_eq!(source.body_rotation, 123_000);
    assert_eq!((source.next_target_x_q32, source.next_target_z_q32), (0, 0));
}

#[test]
fn bodyless_melee_retains_target_while_motion_attack_is_held() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 100)],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
    set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 100_000);
    let source = simulation.actors.get_mut(&1).unwrap();
    source.describe(config.units.get("crawler").unwrap().clone());
    source.lock_target = Some(unit_target(2));
    source.motion = MotionState::Attacking;
    source.motion_attack_hold_fire = true;
    source.set_body_rotation(mdeg_to_degrees_q32(123_000));

    simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

    let source = &simulation.actors[&1];
    assert_eq!(source.motion, MotionState::Moving);
    assert_eq!(source.lock_target, Some(unit_target(2)));
    assert_eq!(source.body_rotation, 123_000);
}

#[test]
fn rejected_bodyless_melee_active_attack_reopens_idle_search_before_release() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 100)],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    set_actor_position(simulation.actors.get_mut(&1).unwrap(), 0, 0);
    set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 100_000);
    let source = simulation.actors.get_mut(&1).unwrap();
    source.describe(config.units.get("crawler").unwrap().clone());
    source.lock_target = Some(unit_target(2));
    source.motion = MotionState::Attacking;
    source.pending = Some(PendingRelease {
        step: 12,
        target: unit_target(2),
    });
    source.fight_skill_phase = FightSkillPhase::Attack;

    simulation.step_actor(1, 11, &mut Vec::new()).unwrap();
    let source = &simulation.actors[&1];
    assert_eq!(source.motion, MotionState::Idle);
    assert_eq!(source.lock_target, None);
    assert_eq!(source.fight_skill_phase, FightSkillPhase::Idle);

    simulation.step_actor(1, 12, &mut Vec::new()).unwrap();
    let source = &simulation.actors[&1];
    assert_eq!(source.motion, MotionState::Moving);
    assert_eq!(source.lock_target, Some(unit_target(2)));
}

#[test]
fn bodyless_attack_cancels_a_pending_attack_when_an_ally_kills_its_target() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            test_placement(0, 0, 0, 0),
            test_placement(1, 0, 0, 100),
            test_placement(1, 1, 20, 100),
        ],
    );
    for type_name in ["crawler", "fang"] {
        let mut simulation = raw_test_simulation(&layout, &config, 7);
        let source = simulation.actors.get_mut(&1).unwrap();
        source.describe(config.units.get(type_name).unwrap().clone());
        source.lock_target = Some(unit_target(2));
        source.motion = MotionState::Attacking;
        source.pending = Some(PendingRelease {
            step: 12,
            target: unit_target(2),
        });
        source.fight_skill_phase = FightSkillPhase::Attack;
        simulation.actors.get_mut(&2).unwrap().life = 0;

        let mut events = Vec::new();
        simulation.step_actor(1, 11, &mut events).unwrap();
        let source = &simulation.actors[&1];
        assert_eq!(source.motion, MotionState::Idle, "{type_name}");
        assert_eq!(source.lock_target, None, "{type_name}");
        assert_eq!(
            source.fight_skill_phase,
            FightSkillPhase::Idle,
            "{type_name}"
        );
        assert!(source.pending.is_none(), "{type_name}");
        assert!(events.is_empty(), "{type_name}");

        simulation.step_actor(1, 12, &mut events).unwrap();
        assert_eq!(
            simulation.actors[&1].lock_target,
            Some(unit_target(3)),
            "{type_name}"
        );
        assert!(events.is_empty(), "{type_name}");
    }
}

#[test]
fn laser_own_kill_retains_then_clears_the_dead_target() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 20)],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    let source = simulation.actors.get_mut(&1).unwrap();
    source.describe(config.units.get("steel_ball").unwrap().clone());
    source.lock_target = Some(unit_target(2));
    source.motion = MotionState::Attacking;
    source.fight_skill_phase = FightSkillPhase::Attack;
    source.pending = Some(PendingRelease {
        step: 10,
        target: unit_target(2),
    });
    simulation.actors.get_mut(&2).unwrap().life = 1;
    let mut events = Vec::new();

    simulation.release(1, &mut events).unwrap();

    let source = &simulation.actors[&1];
    assert_eq!(source.motion, MotionState::Idle);
    assert_eq!(source.lock_target, Some(unit_target(2)));
    assert!(source.retarget_after_own_direct_kill);
    assert_eq!(source.laser_attack_count, 1);
    assert!(matches!(events[0].payload, EventPayload::UnitDied { .. }));
    assert!(matches!(
        events[1].payload,
        EventPayload::Damage { amount: 1 }
    ));

    simulation.step_actor(1, 11, &mut Vec::new()).unwrap();
    let source = &simulation.actors[&1];
    assert_eq!(source.motion, MotionState::Idle);
    assert_eq!(source.lock_target, None);
    assert!(!source.retarget_after_own_direct_kill);
    assert_eq!(source.laser_attack_count, 0);
}

#[test]
fn laser_own_kill_skips_the_same_tick_bodyless_rotation() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 20)],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    let source = simulation.actors.get_mut(&1).unwrap();
    source.describe(config.units.get("steel_ball").unwrap().clone());
    source.lock_target = Some(unit_target(2));
    source.motion = MotionState::Attacking;
    source.fight_skill_phase = FightSkillPhase::Attack;
    source.pending = Some(PendingRelease {
        step: 10,
        target: unit_target(2),
    });
    source.set_body_rotation(0);
    set_actor_position(simulation.actors.get_mut(&2).unwrap(), 3_000, 20_000);
    simulation.actors.get_mut(&2).unwrap().life = 1;

    simulation.step_actor(1, 10, &mut Vec::new()).unwrap();

    let source = &simulation.actors[&1];
    assert_eq!(source.motion, MotionState::Idle);
    assert_eq!(source.body_rotation, 0);
    assert_eq!(source.lock_target, Some(unit_target(2)));
}

#[test]
fn a_quick_switch_before_a_blow_takes_the_next_unit_in_its_attack_area() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            test_placement(0, 0, 0, 0),
            test_placement(1, 0, 0, 20),
            test_placement(1, 1, 0, 40),
        ],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    let source = simulation.actors.get_mut(&1).unwrap();
    source.describe(config.units.get("fang").unwrap().clone());
    source.lock_target = Some(unit_target(2));
    source.motion = MotionState::Attacking;
    source.pending = Some(PendingRelease {
        step: 12,
        target: unit_target(2),
    });
    source.fight_skill_phase = FightSkillPhase::Attack;
    simulation.actors.get_mut(&2).unwrap().life = 0;

    simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

    let source = &simulation.actors[&1];
    assert_eq!(source.motion, MotionState::Attacking);
    assert_eq!(source.lock_target, Some(unit_target(3)));
    assert_eq!(source.pending.unwrap().target, unit_target(3));
}

#[test]
fn direct_splash_emits_one_damage_event_per_actual_target() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            Placement {
                type_name: "rhino".to_owned(),
                ..test_placement(0, 0, 0, 0)
            },
            test_placement(1, 0, 0, 20),
            test_placement(1, 1, 1, 20),
        ],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 20_000);
    set_actor_position(simulation.actors.get_mut(&3).unwrap(), 1_000, 20_000);
    let mut events = Vec::new();
    simulation
        .direct_effect(1, FightActorRef::Unit(2), &mut events)
        .unwrap();
    assert_eq!(
        [simulation.actors[&2].life, simulation.actors[&3].life],
        [1_253, 1_253]
    );
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].source, Some(ObjectRef::new(ObjectKind::Unit, 1)));
    assert_eq!(events[0].source_team_id, Some(0));
    assert_eq!(events[0].target, Some(ObjectRef::new(ObjectKind::Unit, 2)));
    assert_eq!(events[0].payload, EventPayload::Damage { amount: 3_560 });
    assert_eq!(events[1].target, Some(ObjectRef::new(ObjectKind::Unit, 3)));
    assert_eq!(events[1].payload, EventPayload::Damage { amount: 3_560 });
    assert_eq!(
        [
            simulation.actors[&2].last_damage_source,
            simulation.actors[&3].last_damage_source,
        ],
        [
            Some((ObjectRef::new(ObjectKind::Unit, 1), 0)),
            Some((ObjectRef::new(ObjectKind::Unit, 1), 0)),
        ]
    );
}

#[test]
fn direct_kill_emits_damage_before_death_with_raw_target_position() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            Placement {
                type_name: "rhino".to_owned(),
                ..test_placement(0, 0, 0, 0)
            },
            test_placement(1, 0, 0, 20),
        ],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    let target = simulation.actors.get_mut(&2).unwrap();
    target.life = 1;
    target.x_q32 += 7;
    target.z_q32 -= 9;
    let expected_position = QVec3 {
        x: target.x_q32,
        y: 0,
        z: target.z_q32,
    };
    let mut events = Vec::new();

    simulation
        .direct_effect(1, FightActorRef::Unit(2), &mut events)
        .unwrap();

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].payload, EventPayload::Damage { amount: 1 });
    assert_eq!(
        events[1].payload,
        EventPayload::UnitDied {
            position: expected_position
        }
    );
}

#[test]
fn zero_radius_direct_attack_does_not_damage_an_overlapping_secondary_target() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            test_placement(0, 0, 0, 0),
            test_placement(1, 0, 0, 20),
            test_placement(1, 1, 1, 20),
        ],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    simulation
        .actors
        .get_mut(&1)
        .unwrap()
        .describe(config.units.get("crawler").unwrap().clone());
    set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 20_000);
    set_actor_position(simulation.actors.get_mut(&3).unwrap(), 1_000, 20_000);
    let primary_life = simulation.actors[&2].life;
    let secondary_life = simulation.actors[&3].life;

    let mut events = Vec::new();
    simulation
        .direct_effect(1, FightActorRef::Unit(2), &mut events)
        .unwrap();

    assert_eq!(simulation.actors[&2].life, primary_life - 79);
    assert_eq!(simulation.actors[&3].life, secondary_life);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].target, Some(ObjectRef::new(ObjectKind::Unit, 2)));
    assert_eq!(events[0].payload, EventPayload::Damage { amount: 79 });
}

#[test]
fn direct_splash_takes_a_building_beside_the_unit() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            Placement {
                type_name: "rhino".to_owned(),
                ..test_placement(0, 0, 0, 0)
            },
            test_placement(1, 0, 0, 20),
        ],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 20_000);
    let building = simulation
        .buildings
        .iter_mut()
        .find(|building| building.team_id == 1)
        .unwrap();
    building.position = point(1_000, 20_000);
    let building_id = building.building_id;
    let building_life = building.life.current;
    let previous_life = simulation.actors[&2].life;
    simulation
        .direct_effect(1, FightActorRef::Unit(2), &mut Vec::new())
        .unwrap();
    let dealt = previous_life - simulation.actors[&2].life;
    assert!(dealt > 0);
    let building = simulation
        .buildings
        .iter()
        .find(|building| building.building_id == building_id)
        .unwrap();
    // A Rhino's blow is more than the tower's life, which caps it.
    assert_eq!(
        i64::from(building_life - building.life.current),
        dealt.min(i64::from(building_life))
    );
}

#[test]
fn projectile_splash_emits_one_damage_event_per_actual_target() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            test_placement(0, 0, 0, 0),
            Placement {
                type_name: "rhino".to_owned(),
                ..test_placement(1, 0, 0, 20)
            },
            Placement {
                type_name: "rhino".to_owned(),
                ..test_placement(1, 1, 1, 20)
            },
        ],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 20_000);
    set_actor_position(simulation.actors.get_mut(&3).unwrap(), 1_000, 20_000);
    let projectile = Projectile {
        id: 1,
        team: 0,
        owner: 1,
        target_kind: ObjectKind::Unit,
        target: 2,
        x: 0,
        y: 0,
        z: 20_000,
        x_q32: 0,
        y_q32: 0,
        z_q32: space_to_q32(20_000),
        cached_target_x: 0,
        cached_target_y: 0,
        cached_target_z: 20_000,
        cached_target_x_q32: 0,
        cached_target_y_q32: 0,
        cached_target_z_q32: space_to_q32(20_000),
        cached_target_radius: simulation.actors[&2].rules.collision_radius(),
        speed: simulation.actors[&1].rules.attack.projectile_speed(),
        damage: simulation.actors[&1].stats.attack_damage(),
        life: 1,
        lock_target: true,
    };
    let previous_life = [simulation.actors[&2].life, simulation.actors[&3].life];
    let mut events = Vec::new();
    simulation.impact(&projectile, &mut events).unwrap();
    assert_eq!(
        [simulation.actors[&2].life, simulation.actors[&3].life],
        previous_life.map(|life| life - 365)
    );
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].source, Some(ObjectRef::new(ObjectKind::Unit, 1)));
    assert_eq!(events[0].target, Some(ObjectRef::new(ObjectKind::Unit, 2)));
    assert_eq!(events[0].payload, EventPayload::Damage { amount: 365 });
    assert_eq!(events[1].target, Some(ObjectRef::new(ObjectKind::Unit, 3)));
    assert_eq!(events[1].payload, EventPayload::Damage { amount: 365 });
    assert!(matches!(
        events[2].payload,
        EventPayload::ProjectileRemoved { .. }
    ));
    assert_eq!(
        [
            simulation.actors[&2].last_damage_source,
            simulation.actors[&3].last_damage_source,
        ],
        [
            Some((ObjectRef::new(ObjectKind::Unit, 1), 0)),
            Some((ObjectRef::new(ObjectKind::Unit, 1), 0)),
        ]
    );
}

#[test]
fn dual_domain_projectile_splash_uses_the_main_targets_domain() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            Placement {
                type_name: "wraith".to_owned(),
                ..test_placement(0, 0, 0, 0)
            },
            Placement {
                type_name: "marksman".to_owned(),
                ..test_placement(1, 0, 0, 20)
            },
            Placement {
                type_name: "wraith".to_owned(),
                ..test_placement(1, 1, 0, 20)
            },
        ],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 20_000);
    set_actor_position(simulation.actors.get_mut(&3).unwrap(), 0, 20_000);
    let projectile = Projectile {
        id: 1,
        team: 0,
        owner: 1,
        target_kind: ObjectKind::Unit,
        target: 2,
        x: 0,
        y: 0,
        z: 20_000,
        x_q32: 0,
        y_q32: 0,
        z_q32: space_to_q32(20_000),
        cached_target_x: 0,
        cached_target_y: 0,
        cached_target_z: 20_000,
        cached_target_x_q32: 0,
        cached_target_y_q32: 0,
        cached_target_z_q32: space_to_q32(20_000),
        cached_target_radius: simulation.actors[&2].rules.collision_radius(),
        speed: simulation.actors[&1].rules.attack.projectile_speed(),
        damage: simulation.actors[&1].stats.attack_damage(),
        life: 1,
        lock_target: true,
    };
    let ground_life = simulation.actors[&2].life;
    let air_life = simulation.actors[&3].life;

    simulation.impact(&projectile, &mut Vec::new()).unwrap();

    assert_eq!(simulation.actors[&2].life, ground_life - 381);
    assert_eq!(simulation.actors[&3].life, air_life);
}

#[test]
fn projectile_drain_does_not_late_teardown_the_defeated_teams_buildings() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 100)],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    simulation.actors.get_mut(&1).unwrap().life = 0;
    simulation.projectiles.push(Projectile {
        id: 1,
        team: 0,
        owner: 1,
        target_kind: ObjectKind::Unit,
        target: 2,
        x: 0,
        y: 0,
        z: 0,
        x_q32: 0,
        y_q32: 0,
        z_q32: 0,
        cached_target_x: 0,
        cached_target_y: 0,
        cached_target_z: 1_000_000,
        cached_target_x_q32: 0,
        cached_target_y_q32: 0,
        cached_target_z_q32: space_to_q32(1_000_000),
        cached_target_radius: simulation.actors[&2].rules.collision_radius(),
        speed: 1,
        damage: 1,
        life: 1,
        lock_target: false,
    });

    simulation.step(1).unwrap();

    assert!(
        simulation
            .buildings
            .iter()
            .filter(|building| building.team_id == 0)
            .all(|building| building_alive(building) && building.life.current == 3_400)
    );
}

#[test]
fn projectile_splash_takes_a_building_beside_its_target() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            test_placement(0, 0, 0, 0),
            Placement {
                type_name: "rhino".to_owned(),
                ..test_placement(1, 0, 0, 20)
            },
        ],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    set_actor_position(simulation.actors.get_mut(&2).unwrap(), 0, 20_000);
    let building = simulation
        .buildings
        .iter_mut()
        .find(|building| building.team_id == 1)
        .unwrap();
    building.position = point(1_000, 20_000);
    let building_id = building.building_id;
    let building_life = building.life.current;
    let projectile = Projectile {
        id: 1,
        team: 0,
        owner: 1,
        target_kind: ObjectKind::Unit,
        target: 2,
        x: 0,
        y: 0,
        z: 20_000,
        x_q32: 0,
        y_q32: 0,
        z_q32: space_to_q32(20_000),
        cached_target_x: 0,
        cached_target_y: 0,
        cached_target_z: 20_000,
        cached_target_x_q32: 0,
        cached_target_y_q32: 0,
        cached_target_z_q32: space_to_q32(20_000),
        cached_target_radius: simulation.actors[&2].rules.collision_radius(),
        speed: simulation.actors[&1].rules.attack.projectile_speed(),
        damage: simulation.actors[&1].stats.attack_damage(),
        life: 1,
        lock_target: true,
    };
    let previous_life = simulation.actors[&2].life;
    simulation.impact(&projectile, &mut Vec::new()).unwrap();
    let dealt = previous_life - simulation.actors[&2].life;
    assert!(dealt > 0);
    let building = simulation
        .buildings
        .iter()
        .find(|building| building.building_id == building_id)
        .unwrap();
    assert_eq!(i64::from(building_life - building.life.current), dealt);
}

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
fn reviewed_direct_kill_keeps_then_clears_the_mech_lock_target_state() {
    let config = SimulationConfig::load().unwrap();
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
                corrections: Vec::new(),
            },
        ],
    );
    let mut simulation = Simulation::new(
        &layout,
        &config.units,
        &config.training_ground,
        1_787_601_811,
    )
    .unwrap();
    for output_tick in 1..=234 {
        simulation.step(output_tick - 1).unwrap();
        match output_tick {
            223 => {
                assert_eq!(simulation.actors[&1].lock_target, Some(unit_target(3)));
                assert!(simulation.actors[&1].retarget_after_own_direct_kill);
                assert_eq!(simulation.actors[&1].motion, MotionState::Idle);
            }
            224..=232 => {
                assert_eq!(simulation.actors[&1].lock_target, Some(unit_target(3)));
                assert!(simulation.actors[&1].retarget_after_own_direct_kill);
            }
            233 => {
                assert_eq!(simulation.actors[&1].lock_target, None);
                assert!(!simulation.actors[&1].retarget_after_own_direct_kill);
                assert_eq!(simulation.actors[&1].motion, MotionState::Idle);
            }
            234 => {
                // The exact native assignment point is not observable. This only
                // locks the simulator-private state needed to reproduce S/E tick 234.
                assert_eq!(simulation.actors[&1].lock_target, Some(unit_target(2)));
                assert_eq!(simulation.actors[&1].motion, MotionState::Moving);
            }
            _ => {}
        }
    }
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
fn grouped_skills_prime_one_attack_interval_sample_per_child() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            Placement {
                type_name: "wraith".to_owned(),
                ..test_placement(0, 0, 0, 0)
            },
            Placement {
                type_name: "wraith".to_owned(),
                ..test_placement(1, 0, 0, 100)
            },
        ],
    );
    let mut simulation =
        Simulation::new(&layout, &config.units, &config.training_ground, 7).unwrap();

    let blue = simulation.team_random.get_mut(&0).unwrap();
    assert_eq!([blue.next_in_range(4), blue.next_in_range(4)], [-1, -2]);
    let red = simulation.team_random.get_mut(&1).unwrap();
    assert_eq!([red.next_in_range(4), red.next_in_range(4)], [-2, -3]);
}

#[test]
fn grouped_core_replacement_swaps_or_shares_existing_child_targets() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            Placement {
                type_name: "wraith".to_owned(),
                ..test_placement(0, 0, 0, 0)
            },
            test_placement(1, 0, 0, 30),
            test_placement(1, 1, 0, 40),
            test_placement(1, 2, 0, 50),
            test_placement(1, 3, 0, 80),
        ],
    );
    let mut swap = raw_test_simulation(&layout, &config, 7);
    let source = swap.actors.get_mut(&1).unwrap();
    source.motion = MotionState::Attacking;
    source.fight_skill_phase = FightSkillPhase::Attack;
    source.lock_target = None;
    source.group_skill_targets = vec![None, Some(2), Some(3), Some(4)];
    swap.refresh_target_query_snapshot();
    let order = swap.target_search_order();

    swap.update_group_skill_targets(1, 10, &order).unwrap();

    let source = &swap.actors[&1];
    assert_eq!(
        source.group_skill_targets,
        [Some(2), Some(5), Some(3), Some(4)]
    );
    assert_eq!(source.lock_target, Some(unit_target(5)));
    assert_eq!(source.group_skill_prepare_ready_steps, [19, 19, 0, 0]);

    let mut shared = raw_test_simulation(&layout, &config, 7);
    let source = shared.actors.get_mut(&1).unwrap();
    source.motion = MotionState::Attacking;
    source.fight_skill_phase = FightSkillPhase::Attack;
    source.lock_target = Some(unit_target(5));
    source.group_skill_targets = vec![Some(5), Some(2), Some(3), Some(4)];
    source.group_skill_next_attack_steps = vec![0, 0, 0, 11];
    shared.actors.get_mut(&5).unwrap().life = 0;
    shared.refresh_target_query_snapshot();
    let order = shared.target_search_order();

    shared.update_group_skill_targets(1, 10, &order).unwrap();

    let source = &shared.actors[&1];
    assert_eq!(
        source.group_skill_targets,
        [Some(2), Some(2), Some(3), Some(4)]
    );
    assert_eq!(source.lock_target, Some(unit_target(2)));
    assert_eq!(source.group_skill_prepare_ready_steps, [0, 0, 0, 0]);
}

#[test]
fn grouped_core_search_assigns_the_core_before_children() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        std::iter::once(Placement {
            type_name: "wraith".to_owned(),
            ..test_placement(0, 0, 0, 0)
        })
        .chain((0..6).map(|index| test_placement(1, index, 0, 40 + i64::from(index))))
        .collect(),
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    let source = simulation.actors.get_mut(&1).unwrap();
    source.motion = MotionState::Attacking;
    source.fight_skill_phase = FightSkillPhase::Attack;
    source.lock_target = Some(unit_target(5));
    source.group_skill_targets = vec![Some(5), Some(2), Some(3), Some(4)];
    simulation.actors.get_mut(&4).unwrap().life = 0;
    simulation.actors.get_mut(&5).unwrap().life = 0;
    simulation.refresh_target_query_snapshot();
    let order = simulation.target_search_order();

    simulation
        .update_group_skill_targets(1, 10, &order)
        .unwrap();

    let source = &simulation.actors[&1];
    assert!(source.group_skill_targets[0].is_some());
    assert!(source.group_skill_targets[3].is_some());
    assert_ne!(source.group_skill_targets[0], source.group_skill_targets[3]);
    assert_eq!(
        source.lock_target,
        source.group_skill_targets[3].map(FightActorRef::Unit)
    );
}

#[test]
fn grouped_child_replacements_follow_skill_order() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        std::iter::once(Placement {
            type_name: "wraith".to_owned(),
            ..test_placement(0, 0, 0, 0)
        })
        .chain((0..6).map(|index| test_placement(1, index, 0, 40 + i64::from(index))))
        .collect(),
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    let source = simulation.actors.get_mut(&1).unwrap();
    source.motion = MotionState::Attacking;
    source.fight_skill_phase = FightSkillPhase::Attack;
    source.lock_target = Some(unit_target(5));
    source.group_skill_targets = vec![Some(2), Some(3), Some(4), Some(5)];
    source.group_skill_next_attack_steps = vec![0, 0, 10, 0];
    simulation.actors.get_mut(&4).unwrap().life = 0;
    simulation.actors.get_mut(&5).unwrap().life = 0;
    simulation.refresh_target_query_snapshot();
    let order = simulation.target_search_order();
    let replacements = simulation
        .rank_group_unit_targets_with_order(1, &order, true)
        .unwrap()
        .into_iter()
        .filter(|target_id| ![2, 3].contains(target_id))
        .take(2)
        .collect::<Vec<_>>();

    simulation
        .update_group_skill_targets(1, 10, &order)
        .unwrap();

    let source = &simulation.actors[&1];
    assert_eq!(source.group_skill_targets[2], Some(replacements[0]));
    assert_eq!(source.group_skill_targets[3], Some(replacements[1]));
    assert_eq!(source.lock_target, Some(unit_target(replacements[1])));
}

#[test]
fn grouped_intervening_attack_rebalances_the_later_missing_child() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        std::iter::once(Placement {
            type_name: "wraith".to_owned(),
            ..test_placement(0, 0, 0, 0)
        })
        .chain((0..6).map(|index| test_placement(1, index, 0, 40 + i64::from(index))))
        .collect(),
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    let source = simulation.actors.get_mut(&1).unwrap();
    source.motion = MotionState::Attacking;
    source.fight_skill_phase = FightSkillPhase::Attack;
    source.lock_target = Some(unit_target(5));
    source.group_skill_targets = vec![Some(2), Some(3), Some(4), Some(5)];
    source.group_skill_next_attack_steps = vec![0, 248, 228, 229];
    simulation.actors.get_mut(&3).unwrap().life = 0;
    simulation.actors.get_mut(&5).unwrap().life = 0;
    simulation.refresh_target_query_snapshot();
    let order = simulation.target_search_order();
    let replacements = simulation
        .rank_group_unit_targets_with_order(1, &order, true)
        .unwrap()
        .into_iter()
        .filter(|target_id| ![2, 4].contains(target_id))
        .take(2)
        .collect::<Vec<_>>();

    simulation
        .update_group_skill_targets(1, 228, &order)
        .unwrap();

    let source = &simulation.actors[&1];
    assert_eq!(source.group_skill_targets[3], Some(replacements[0]));
    assert_eq!(source.group_skill_targets[1], Some(replacements[1]));
    assert_eq!(source.lock_target, Some(unit_target(replacements[0])));
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
fn rhino_attack_angle_requires_every_weapon_and_accepts_the_boundary() {
    let config = SimulationConfig::load().unwrap();
    let rules = config.units.get("rhino").unwrap().clone();
    let mut actor = Actor::new(
        Placement {
            team: 0,
            unit_id: 1,
            formation_id: 1,
            formation_index: 0,
            type_name: "rhino".to_owned(),
            world_x: 0,
            world_z: 0,
            rotation: 0,
            rotated: false,
            corrections: Vec::new(),
        },
        rules,
        0,
    );
    let target = 0;
    actor.weapon_rotations_q32 = vec![0, 41_i64 << 32];
    assert!(!actor.weapons_in_attack_angle(target));
    actor.weapon_rotations_q32[1] = 40_i64 << 32;
    assert!(actor.weapons_in_attack_angle(target));
}

#[test]
fn rhino_backswing_remains_active_through_its_ninth_wait_update() {
    let layout = CompiledLayout::of_units(
        1,
        vec![
            Placement {
                team: 0,
                unit_id: 1,
                formation_id: 1,
                formation_index: 0,
                type_name: "rhino".to_owned(),
                world_x: -35,
                world_z: -105,
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
                world_x: 40,
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
        1_787_591_883,
    )
    .unwrap();

    for step in 0..226 {
        simulation.step(step).unwrap();
        let output_tick = step + 1;
        let rhino = &simulation.actors[&1];
        match output_tick {
            216 | 225 => assert_eq!(rhino.backswing_finish_step, Some(224)),
            226 => {
                assert_eq!(rhino.backswing_finish_step, None);
                assert_eq!(rhino.pending.unwrap().step, 233);
            }
            _ => {}
        }
    }
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
fn q32_clamp_magnitude_preserves_a_tolerance_equal_zero_speed_delta() {
    // FPoint's comparison treats this squared magnitude (28 raw) as
    // equal to zero, so FVector2.ClampMagnitude returns the input delta.
    assert_eq!(
        clamp_magnitude_q32_raw(43_007, -347_649, 0),
        (43_007, -347_649)
    );
    assert_eq!(clamp_magnitude_q32_raw(0, Q32_ONE, 0), (0, 0));
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

/// Every slot of a grouped skill takes the construction in its way, and
/// every slot is dropped with the lock.
///
/// The Wraith of `tests/construction/wall-weapon-group.yaml` was
/// recorded doing all of it: its core engages block 3 at tick 32 and the
/// other three slots follow eight ticks later, while the lock stays on the
/// Marksman; block 3 falls at tick 59 and all four slots read empty at
/// tick 60; the core engages block 4 at tick 74 and the children are
/// allocated again eight ticks after that, not at once.
#[test]
fn grouped_slots_take_the_wall_and_are_dropped_with_the_lock() {
    let config = SimulationConfig::load().unwrap();
    let (_, layout) = crate::layout::compile_with_seed(
        include_bytes!("../../../../tests/construction/wall-weapon-group.yaml"),
        &config.units,
    )
    .unwrap();
    let mut simulation =
        Simulation::new(&layout, &config.units, &config.training_ground, 4242).unwrap();
    let slots_at = |simulation: &mut Simulation, tick: u64, done: &mut u64| {
        while *done < tick {
            simulation.step(*done).unwrap();
            *done += 1;
        }
        let wraith = simulation
            .actors
            .values()
            .find(|actor| actor.placement.team == 1)
            .unwrap()
            .snapshot();
        (
            wraith.mech_lock_target,
            wraith
                .weapon_aims
                .iter()
                .map(|aim| aim.attack_target)
                .collect::<Vec<_>>(),
        )
    };
    let building = |id| Some(ObjectRef::new(ObjectKind::Building, id));
    let marksman = Some(ObjectRef::new(ObjectKind::Unit, 1));
    let mut done = 0;

    let (lock, slots) = slots_at(&mut simulation, 41, &mut done);
    assert_eq!(lock, marksman, "the lock stays on the unit behind the wall");
    assert_eq!(slots, vec![building(3); 4], "all four slots on block 3");

    let (lock, slots) = slots_at(&mut simulation, 60, &mut done);
    assert_eq!(lock, None, "block 3 has fallen and the lock is dropped");
    assert_eq!(slots, vec![None; 4], "and every slot with it");

    let (_, slots) = slots_at(&mut simulation, 78, &mut done);
    assert_eq!(
        slots,
        vec![building(4), None, None, None],
        "the core has block 4; the children wait to be allocated again"
    );
}

/// A block that falls ends the attack on it and the lock with it, and the
/// weapon keeps naming the block until a new target is taken.
///
/// Red's Marksman of `wall-line-of-fire.yaml` fells block 6 on tick 18.
/// The game reads it on tick 19 as idle, with no lock, and with its weapon
/// still on block 6 — not on block 7, the next one in its line, and not on
/// the Marksman behind the wall. `tests/construction/line-of-fire.mcscript`
/// recorded it; the physics hash cannot see any of the three fields.
#[test]
fn a_fallen_block_leaves_its_attacker_idle_and_still_aimed_at_it() {
    let config = SimulationConfig::load().unwrap();
    let (_, layout) = crate::layout::compile_with_seed(
        include_bytes!("../../../../tests/construction/wall-line-of-fire.yaml"),
        &config.units,
    )
    .unwrap();
    let mut simulation =
        Simulation::new(&layout, &config.units, &config.training_ground, 4242).unwrap();
    let state = |simulation: &Simulation| {
        simulation
            .actors
            .values()
            .find(|actor| actor.placement.team == 1)
            .unwrap()
            .snapshot()
    };
    for step in 0..18 {
        simulation.step(step).unwrap();
    }
    let shooting = state(&simulation);
    assert_eq!(shooting.motion_state, MotionState::Attacking);
    assert_eq!(
        shooting.weapon_aims[0].attack_target,
        Some(ObjectRef::new(ObjectKind::Building, 6))
    );

    simulation.step(18).unwrap();
    let fallen = state(&simulation);
    assert_eq!(fallen.motion_state, MotionState::Idle);
    assert_eq!(
        fallen.mech_lock_target, None,
        "the lock falls with the block"
    );
    assert_eq!(
        fallen.weapon_aims[0].attack_target,
        Some(ObjectRef::new(ObjectKind::Building, 6)),
        "the weapon still names the block it felled"
    );
}

/// A block that comes into the way while an attack on a unit is being
/// prepared ends that attack, and the unit looks again before firing.
///
/// Steel Ball 4 of `wall-laser.yaml` prepares a beam on the Marksman behind
/// the wall from tick 140. Block 3 stands 11.507 metres off its line until
/// tick 144 and 11.447 at 145, inside the 11.5 the line is wide: on tick
/// 146 the game reads it idle, with no lock and no weapon target, and on
/// 147 on block 3 with its lock back on the Marksman.
/// `tests/construction/attacks.mcscript` recorded it.
#[test]
fn a_block_that_comes_into_the_way_ends_a_prepared_attack() {
    let config = SimulationConfig::load().unwrap();
    let (_, layout) = crate::layout::compile_with_seed(
        include_bytes!("../../../../tests/construction/wall-laser.yaml"),
        &config.units,
    )
    .unwrap();
    let mut simulation =
        Simulation::new(&layout, &config.units, &config.training_ground, 4242).unwrap();
    for step in 0..145 {
        simulation.step(step).unwrap();
    }
    assert!(matches!(
        simulation.actors[&4].fight_skill_phase,
        FightSkillPhase::Prepare { .. }
    ));
    simulation.step(145).unwrap();
    let interrupted = simulation.actors[&4].snapshot();
    assert_eq!(interrupted.motion_state, MotionState::Idle);
    assert_eq!(interrupted.mech_lock_target, None);
    assert_eq!(interrupted.weapon_aims[0].attack_target, None);

    simulation.step(146).unwrap();
    let turned = simulation.actors[&4].snapshot();
    assert_eq!(turned.motion_state, MotionState::Attacking);
    assert_eq!(
        turned.mech_lock_target,
        Some(ObjectRef::new(ObjectKind::Unit, 1))
    );
    assert_eq!(
        turned.weapon_aims[0].attack_target,
        Some(ObjectRef::new(ObjectKind::Building, 3))
    );
}

/// Crawlers against a wall change blocks between blows, and only one that
/// struck a block idles when it falls.
///
/// In `wall-block.yaml`, Crawler 2 is pushed along the wall while it strikes
/// block 4: when its swing is over on tick 96, block 3 is the nearer one in
/// its line, and it reads idle with no lock before turning on block 3 at
/// 97. Crawlers 7 and 23 are closing on block 4 without having struck it
/// when another fells it at 118, and they go straight on to the Marksman
/// at 119. `tests/construction/wall.mcscript` recorded the fight.
#[test]
fn crawlers_change_blocks_between_blows_and_only_a_striker_idles() {
    let config = SimulationConfig::load().unwrap();
    let (_, layout) = crate::layout::compile_with_seed(
        include_bytes!("../../../../tests/construction/wall-block.yaml"),
        &config.units,
    )
    .unwrap();
    let mut simulation =
        Simulation::new(&layout, &config.units, &config.training_ground, 4242).unwrap();
    let read = |simulation: &Simulation, id: u64| simulation.actors[&id].snapshot();
    for step in 0..96 {
        simulation.step(step).unwrap();
    }
    let switching = read(&simulation, 2);
    assert_eq!(switching.motion_state, MotionState::Idle);
    assert_eq!(switching.mech_lock_target, None);
    simulation.step(96).unwrap();
    assert_eq!(
        read(&simulation, 2).weapon_aims[0].attack_target,
        Some(ObjectRef::new(ObjectKind::Building, 3))
    );
    for step in 97..119 {
        simulation.step(step).unwrap();
    }
    for id in [7, 23] {
        let going_on = read(&simulation, id);
        assert_eq!(going_on.motion_state, MotionState::Moving, "Crawler {id}");
        assert_eq!(
            going_on.mech_lock_target,
            Some(ObjectRef::new(ObjectKind::Unit, 1)),
            "Crawler {id}"
        );
    }
}

/// A Marksman whose shot kills its target before the attack is over holds
/// through its cooling when the replacement cannot be attacked at once.
///
/// In `crawlers-vs-marksman.yaml` the Marksman releases at tick 135 and
/// kills Crawler 8 at 136. The Crawler the selector answers, 4, is out of
/// its attack angle, so for ticks 137 to 140 it reads idle with no lock and
/// its weapon on Crawler 4; at 141 the weapon clears; at 142 it locks
/// Crawler 7, whom the Crawlers' approach has made the selector's answer.
#[test]
fn a_marksman_holds_through_its_cooling_after_a_kill_it_cannot_follow() {
    let config = SimulationConfig::load().unwrap();
    let (_, layout) = crate::layout::compile_with_seed(
        include_bytes!("../../../../tests/regression/crawlers-vs-marksman.yaml"),
        &config.units,
    )
    .unwrap();
    let mut simulation =
        Simulation::new(&layout, &config.units, &config.training_ground, 4242).unwrap();
    let mut states = BTreeMap::new();
    for step in 0..142u64 {
        simulation.step(step).unwrap();
        if step + 1 >= 137 {
            states.insert(step + 1, simulation.actors[&1].snapshot());
        }
    }
    for tick in 137..=140 {
        let state = &states[&tick];
        assert_eq!(state.motion_state, MotionState::Idle, "tick {tick}");
        assert_eq!(state.mech_lock_target, None, "tick {tick}");
        assert_eq!(
            state.weapon_aims[0].attack_target,
            Some(ObjectRef::new(ObjectKind::Unit, 4)),
            "tick {tick}"
        );
    }
    assert_eq!(states[&141].weapon_aims[0].attack_target, None);
    assert_eq!(
        states[&142].mech_lock_target,
        Some(ObjectRef::new(ObjectKind::Unit, 7))
    );
}

/// The body and the weapons have separate targets, and a recording reports
/// both.
///
/// Red's Marksman locks onto the Marksman behind blue's wall and shoots
/// block 6, which stands in its line of fire: `docs/rules/combat.md`
/// measured the lock staying on the unit while the weapon holds the block,
/// and `tests/construction/line-of-fire.mcscript` recorded this
/// exact fight. The physics hash cannot see either field, which is why
/// this pins them here.
#[test]
fn a_wall_in_the_way_takes_the_weapon_and_leaves_the_lock() {
    let config = SimulationConfig::load().unwrap();
    let (_, layout) = crate::layout::compile_with_seed(
        include_bytes!("../../../../tests/construction/wall-line-of-fire.yaml"),
        &config.units,
    )
    .unwrap();
    let mut simulation =
        Simulation::new(&layout, &config.units, &config.training_ground, 4242).unwrap();
    for step in 0..2 {
        simulation.step(step).unwrap();
    }
    let marksman = simulation
        .actors
        .values()
        .find(|actor| actor.placement.team == 1)
        .unwrap();
    let behind_the_wall = simulation
        .actors
        .values()
        .find(|actor| actor.placement.team == 0)
        .unwrap()
        .placement
        .unit_id;
    let state = marksman.snapshot();

    assert_eq!(
        state.mech_lock_target,
        Some(ObjectRef::new(ObjectKind::Unit, behind_the_wall)),
        "the body keeps the unit it searched for"
    );
    assert_eq!(
        state.weapon_aims[0].attack_target,
        Some(ObjectRef::new(ObjectKind::Building, 6)),
        "the weapon holds the block in the way"
    );
    assert_eq!(state.motion_state, MotionState::Attacking);
    assert_eq!(
        marksman.lock_target,
        Some(FightActorRef::Unit(behind_the_wall)),
        "the lock is never overwritten by the block"
    );
}

#[test]
fn first_rvo_solve_avoids_same_formation_at_tick_eight() {
    let config = SimulationConfig::load().unwrap();
    let (_, layout) = crate::layout::compile_with_seed(
        include_bytes!("../../../../tests/regression/steel-balls-vs-steel-balls.yaml"),
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
fn first_split_rvo_tree_uses_the_zero_position_buffer() {
    let config = SimulationConfig::load().unwrap();
    let (_, layout) = crate::layout::compile_with_seed(
        include_bytes!("../../../../tests/regression/rhino-vs-crawlers.yaml"),
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
        let (x_q32, z_q32) =
            millimeter_path.get_or_insert_with(|| (space_to_q32(owner.x), space_to_q32(owner.z)));
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
