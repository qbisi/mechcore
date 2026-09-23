use super::*;

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
        owner: FightActorRef::Unit(1),
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
        owner: FightActorRef::Unit(1),
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
        owner: FightActorRef::Unit(1),
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
        owner: FightActorRef::Unit(1),
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
    let mut simulation =
        Simulation::new(&layout, &config.units, &config.towers, 1_787_555_163).unwrap();
    let mut millimeter_path = None;
    let mut raw_x = Vec::new();
    let mut millimeter_x = Vec::new();

    for tick in 1..=18 {
        simulation.step(tick - 1).unwrap();
        if tick < 14 {
            continue;
        }

        let projectile = &simulation.projectiles[0];
        let owner = &simulation.actors[&projectile.owner.id()];
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
