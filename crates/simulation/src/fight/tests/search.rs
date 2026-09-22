use super::*;

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
        constructions: BTreeMap::new(),
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
    source.skill.lock_target = None;
    source.skill.search_target_time = 0;
    source.motion.state = MotionState::Idle;
    let building = simulation
        .buildings
        .iter_mut()
        .find(|building| building.team_id == 1)
        .unwrap();
    building.position = point(0, 100_000);
    let building_id = building.building_id;

    simulation.step_actor(1, 0, &mut Vec::new()).unwrap();

    let source = &simulation.actors[&1];
    assert_eq!(source.motion.state, MotionState::Moving);
    assert_eq!(
        source.skill.lock_target,
        Some(FightActorRef::Building(building_id))
    );
    assert!(!source.skill.lock_is_terminal_handoff);
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
            simulation.actors[&1].skill.lock_target,
            Some(FightActorRef::Building(building_id))
        );
    }
    simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

    assert_eq!(
        simulation.actors[&1].skill.lock_target,
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
    source.skill.lock_target = Some(unit_target(2));
    source.skill.search_target_time = 10;
    simulation.refresh_target_query_snapshot();
    let target_search_order = simulation.target_search_order();

    simulation.actors.get_mut(&2).unwrap().life = 0;
    simulation.actors.get_mut(&3).unwrap().z_q32 = space_to_q32(100_000);
    simulation.actors.get_mut(&4).unwrap().z_q32 = space_to_q32(10_000);
    simulation
        .update_fight_skill_target_search(1, 1, &target_search_order)
        .unwrap();

    assert_eq!(
        simulation.actors[&1].skill.lock_target,
        Some(unit_target(4))
    );

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

    assert_eq!(
        simulation.actors[&1].skill.lock_target,
        Some(unit_target(4))
    );
}

#[test]
fn the_selector_answers_a_tower_once_no_enemy_unit_is_left() {
    // `SkillAttackableChecker.Check` on the tick the last enemy unit falls
    // searches again and is answered with one of that team's towers.
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![test_placement(0, 0, 0, 0), test_placement(1, 0, 0, 60)],
    );
    let mut simulation = raw_test_simulation(&layout, &config, 7);
    simulation.actors.get_mut(&2).unwrap().life = 0;
    simulation.refresh_target_query_snapshot();
    let target_search_order = simulation.target_search_order();
    let selected = simulation
        .select_normal_target_with_order(1, &target_search_order, true)
        .unwrap();
    let Some(FightActorRef::Building(building_id)) = selected else {
        panic!("expected a tower, got {selected:?}");
    };
    let building = simulation
        .buildings
        .iter()
        .find(|building| building.building_id == building_id)
        .unwrap();
    assert_eq!(building.team_id, 1);
    assert!(
        matches!(building.building_type_id, 1 | 2),
        "EnergyTower or ResearchCenter"
    );
}
