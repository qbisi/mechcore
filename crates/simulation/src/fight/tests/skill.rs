use super::*;

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
    source.motion.state = MotionState::Attacking;
    source.skill.lock_target = Some(unit_target(2));
    source.skill.set_phase(FightSkillPhase::Attack);
    source.set_weapon_rotation(mdeg_to_degrees_q32(4_924));
    source.aim_rotation = 4_924;

    simulation.step_actor(1, 1, &mut Vec::new()).unwrap();
    assert_eq!(simulation.actors[&1].motion.state, MotionState::Idle);
    assert_eq!(simulation.actors[&1].skill.lock_target, None);

    simulation.step_actor(1, 2, &mut Vec::new()).unwrap();
    assert_eq!(simulation.actors[&1].motion.state, MotionState::Moving);
    assert_eq!(
        simulation.actors[&1].skill.lock_target,
        Some(unit_target(3))
    );
    assert_eq!(simulation.actors[&1].aim_rotation, 4_924);
}

#[test]
fn a_quick_switch_check_takes_the_unit_in_its_attack_area() {
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
    source.motion.state = MotionState::Attacking;
    source.skill.lock_target = Some(unit_target(5));
    source.skill.set_phase(FightSkillPhase::Attack);
    simulation.refresh_target_query_snapshot();
    let target_search_order = simulation.target_search_order();

    // A lock inside the minimum range is out of the attack area; a skill
    // that switches quickly searches again and takes the unit in its area.
    assert!(
        simulation
            .check_attackable(1, &target_search_order)
            .unwrap()
    );
    assert_eq!(
        simulation.actors[&1].skill.lock_target,
        Some(unit_target(6))
    );

    // A dead lock is searched for the same way.
    simulation.actors.get_mut(&1).unwrap().skill.lock_target = Some(unit_target(5));
    simulation.actors.get_mut(&5).unwrap().life = 0;
    simulation.refresh_target_query_snapshot();
    let target_search_order = simulation.target_search_order();
    assert!(
        simulation
            .check_attackable(1, &target_search_order)
            .unwrap()
    );
    assert_eq!(
        simulation.actors[&1].skill.lock_target,
        Some(unit_target(6))
    );

    // With nothing in its area the search answers a unit it cannot attack,
    // and the check fails.
    set_actor_position(simulation.actors.get_mut(&6).unwrap(), 0, 250_000);
    simulation.actors.get_mut(&5).unwrap().life = 263;
    simulation.actors.get_mut(&1).unwrap().skill.lock_target = Some(unit_target(5));
    simulation.refresh_target_query_snapshot();
    let target_search_order = simulation.target_search_order();
    assert!(
        !simulation
            .check_attackable(1, &target_search_order)
            .unwrap()
    );
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
    source.motion.state = MotionState::Attacking;
    source.skill.lock_target = Some(unit_target(2));
    source.skill.set_phase(FightSkillPhase::Attack);
    source.set_weapon_rotation(mdeg_to_degrees_q32(90_000));

    simulation.step_actor(1, 1, &mut Vec::new()).unwrap();

    assert_eq!(simulation.actors[&1].motion.state, MotionState::Idle);
    assert_eq!(simulation.actors[&1].skill.lock_target, None);
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
    source.skill.lock_target = Some(unit_target(2));
    source.set_weapon_rotation(initial_rotation);
    source.aim_rotation = 168_143;

    simulation.step_actor(1, 1, &mut Vec::new()).unwrap();
    assert_eq!(simulation.actors[&1].motion.state, MotionState::Attacking);
    assert_eq!(
        simulation.actors[&1].skill.weapon_rotations_q32[0],
        initial_rotation
    );
    assert_eq!(simulation.actors[&1].aim_rotation, 168_143);

    simulation.step_actor(1, 2, &mut Vec::new()).unwrap();
    assert_ne!(
        simulation.actors[&1].skill.weapon_rotations_q32[0],
        initial_rotation
    );
    assert_ne!(simulation.actors[&1].aim_rotation, 168_143);
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
    source.skill.lock_target = Some(unit_target(2));
    source.motion.state = MotionState::Moving;
    source.skill.set_backswing_finish_step(Some(10));
    source.set_body_rotation(mdeg_to_degrees_q32(123_000));

    simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

    let source = &simulation.actors[&1];
    assert_eq!(source.motion.state, MotionState::Idle);
    assert_eq!(source.skill.lock_target, None);
    assert_eq!(source.body_rotation, 123_000);
    assert_eq!(
        (
            source.motion.next_target_x_q32,
            source.motion.next_target_z_q32
        ),
        (0, 0)
    );
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
    source.skill.lock_target = Some(unit_target(2));
    source.motion.state = MotionState::Moving;
    source.skill.set_phase(FightSkillPhase::Attack);
    source.skill.set_backswing_finish_step(Some(10));
    source.set_body_rotation(mdeg_to_degrees_q32(90_000));

    simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

    let source = &simulation.actors[&1];
    assert_eq!(source.motion.state, MotionState::Idle);
    assert_eq!(source.skill.lock_target, None);
    assert_eq!(source.body_rotation, 90_000);
    assert_eq!(
        (
            source.motion.next_target_x_q32,
            source.motion.next_target_z_q32
        ),
        (0, 0)
    );
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
        source.skill.lock_target = Some(unit_target(3));
        source.motion.state = MotionState::Attacking;
        source.skill.set_phase(FightSkillPhase::Attack);
        source.skill.set_backswing_finish_step(Some(10));
        source.skill.next_attack_step = 20;
        simulation.actors.get_mut(&3).unwrap().life = 0;

        simulation.step_actor(1, 8, &mut Vec::new()).unwrap();
        let stop_target = {
            let source = &simulation.actors[&1];
            assert_eq!(source.motion.state, MotionState::Idle, "{type_name}");
            assert_eq!(
                source.skill.lock_target,
                Some(unit_target(3)),
                "{type_name}"
            );
            assert_eq!(
                source.skill.backswing_finish_step(),
                Some(10),
                "{type_name}"
            );
            (
                source.motion.next_target_x_q32,
                source.motion.next_target_z_q32,
            )
        };
        simulation.actors.get_mut(&1).unwrap().x_q32 += Q32_ONE;

        for step in 9..=10 {
            simulation.step_actor(1, step, &mut Vec::new()).unwrap();
            let source = &simulation.actors[&1];
            assert_eq!(source.motion.state, MotionState::Idle, "{type_name}");
            assert_eq!(
                source.skill.lock_target,
                Some(unit_target(3)),
                "{type_name}"
            );
            assert_eq!(
                source.skill.backswing_finish_step(),
                Some(10),
                "{type_name}"
            );
            assert_eq!(
                (
                    source.motion.next_target_x_q32,
                    source.motion.next_target_z_q32
                ),
                stop_target,
                "{type_name}"
            );
        }

        simulation.step_actor(1, 11, &mut Vec::new()).unwrap();
        let source = &simulation.actors[&1];
        assert_eq!(source.motion.state, MotionState::Idle, "{type_name}");
        assert_eq!(source.skill.lock_target, None, "{type_name}");
        assert_eq!(source.skill.backswing_finish_step(), None, "{type_name}");
        assert_eq!(
            (
                source.motion.next_target_x_q32,
                source.motion.next_target_z_q32
            ),
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
    source.skill.lock_target = Some(unit_target(2));
    source.motion.state = MotionState::Attacking;
    source.skill.set_phase(FightSkillPhase::Attack);
    source.skill.set_backswing_finish_step(Some(20));
    simulation.refresh_target_query_snapshot();
    let target_search_order = simulation.target_search_order();
    simulation.actors.get_mut(&2).unwrap().life = 0;

    simulation
        .step_actor_with_target_order(1, 10, &target_search_order, &mut Vec::new())
        .unwrap();

    let source = &simulation.actors[&1];
    assert_eq!(source.motion.state, MotionState::Idle);
    assert_eq!(source.skill.lock_target, Some(unit_target(2)));
    assert_eq!(source.skill.backswing_finish_step(), Some(20));
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
    source.skill.lock_target = Some(unit_target(2));
    source.motion.state = MotionState::Idle;
    source.skill.set_phase(FightSkillPhase::Attack);
    source.skill.retarget_after_own_direct_kill = true;
    source.skill.set_backswing_finish_step(Some(20));
    simulation.actors.get_mut(&2).unwrap().life = 0;

    simulation.step_actor(1, 10, &mut Vec::new()).unwrap();
    assert_eq!(
        simulation.actors[&1].skill.lock_target,
        Some(unit_target(2))
    );

    simulation.actors.get_mut(&3).unwrap().life = 0;
    simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

    let source = &simulation.actors[&1];
    assert_eq!(source.motion.state, MotionState::Idle);
    assert_eq!(source.skill.lock_target, Some(unit_target(2)));
    assert_eq!(source.skill.backswing_finish_step(), Some(20));
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
        source.skill.lock_target = Some(unit_target(2));
        source.motion.state = MotionState::Attacking;
        source.skill.set_phase(FightSkillPhase::Attack);
        source.motion.attack_hold_fire = false;
        source.skill.next_attack_step = 20;
        simulation.actors.get_mut(&2).unwrap().life = 0;

        simulation.step_actor(1, 10, &mut Vec::new()).unwrap();
        let source = &simulation.actors[&1];
        assert_eq!(source.motion.state, MotionState::Idle, "{type_name}");
        assert_eq!(source.skill.lock_target, None, "{type_name}");
        assert_eq!(source.skill.next_attack_step, 20, "{type_name}");

        simulation.step_actor(1, 11, &mut Vec::new()).unwrap();
        assert_eq!(
            simulation.actors[&1].skill.lock_target,
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
    source.skill.lock_target = Some(unit_target(2));
    source.motion.state = MotionState::Attacking;
    source.skill.set_phase(FightSkillPhase::Idle);
    source.skill.next_attack_step = 20;
    simulation.actors.get_mut(&2).unwrap().life = 0;
    assert!(simulation.target_in_attack_range(FightActorRef::Unit(1), unit_target(3)));
    assert!(!simulation.bodyless_target_in_attack_angle(1, unit_target(3)));

    simulation.step_actor(1, 10, &mut Vec::new()).unwrap();

    let source = &simulation.actors[&1];
    assert_eq!(source.skill.lock_target, None);
    assert_eq!(source.motion.state, MotionState::Idle);
    assert_eq!(source.skill.next_attack_step, 20);
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
    source.skill.lock_target = Some(unit_target(2));
    source.motion.state = MotionState::Idle;

    simulation.step_actor(1, 10, &mut Vec::new()).unwrap();

    let source = &simulation.actors[&1];
    assert_eq!(source.motion.state, MotionState::Attacking);
    assert!(source.motion.attack_hold_fire);
    assert_eq!(source.skill.phase(), FightSkillPhase::Idle);
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
    source.skill.lock_target = Some(unit_target(2));
    source.motion.state = MotionState::Attacking;
    source.skill.set_phase(FightSkillPhase::Attack);
    source.skill.next_attack_step = 10;
    // A backswing ends by the next blow.
    source.skill.set_backswing_finish_step(Some(10));
    simulation.actors.get_mut(&2).unwrap().life = 0;

    simulation.step_actor(1, 10, &mut Vec::new()).unwrap();

    assert_eq!(
        simulation.actors[&1].skill.lock_target,
        Some(unit_target(2))
    );
    assert_eq!(
        simulation.actors[&1].skill.backswing_finish_step(),
        Some(10)
    );

    let mut events = Vec::new();
    simulation.step_actor(1, 11, &mut events).unwrap();

    assert_eq!(
        simulation.actors[&1].skill.lock_target,
        Some(unit_target(3))
    );
    assert_eq!(
        simulation.actors[&1].skill.backswing_finish_step(),
        Some(simulation.actors[&1].skill.next_attack_step)
    );
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
    source.skill.lock_target = Some(unit_target(2));
    source.motion.state = MotionState::Attacking;
    // Between two blows the skill is still attacking, and the checker finds
    // the dead lock there.
    source.skill.set_phase(FightSkillPhase::Attack);
    simulation.actors.get_mut(&2).unwrap().life = 0;

    simulation.step_actor(1, 10, &mut Vec::new()).unwrap();
    assert_eq!(simulation.actors[&1].skill.lock_target, None);

    simulation.step_actor(1, 11, &mut Vec::new()).unwrap();
    assert_eq!(
        simulation.actors[&1].skill.lock_target,
        Some(unit_target(3))
    );
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
    source.skill.lock_target = Some(unit_target(2));
    source.motion.state = MotionState::Attacking;
    source.set_body_rotation(mdeg_to_degrees_q32(123_000));

    simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

    let source = &simulation.actors[&1];
    assert_eq!(source.motion.state, MotionState::Idle);
    assert_eq!(source.skill.lock_target, None);
    assert_eq!(source.body_rotation, 123_000);
    assert_eq!(
        (
            source.motion.next_target_x_q32,
            source.motion.next_target_z_q32
        ),
        (0, 0)
    );
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
    source.skill.lock_target = Some(unit_target(2));
    source.motion.state = MotionState::Attacking;
    source.skill.set_phase(FightSkillPhase::Attack);
    source.set_body_rotation(mdeg_to_degrees_q32(123_000));

    simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

    let source = &simulation.actors[&1];
    assert_eq!(source.motion.state, MotionState::Idle);
    assert_eq!(source.skill.lock_target, None);
    assert_eq!(source.skill.phase(), FightSkillPhase::Idle);
    assert_eq!(source.body_rotation, 123_000);
    assert_eq!(
        (
            source.motion.next_target_x_q32,
            source.motion.next_target_z_q32
        ),
        (0, 0)
    );
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
    source.skill.lock_target = Some(unit_target(2));
    source.motion.state = MotionState::Attacking;
    source.motion.attack_hold_fire = true;
    source.set_body_rotation(mdeg_to_degrees_q32(123_000));

    simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

    let source = &simulation.actors[&1];
    assert_eq!(source.motion.state, MotionState::Moving);
    assert_eq!(source.skill.lock_target, Some(unit_target(2)));
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
    source.skill.lock_target = Some(unit_target(2));
    source.motion.state = MotionState::Attacking;
    source.skill.set_phase(FightSkillPhase::Attack);
    source.skill.set_pending(Some(PendingRelease {
        step: 12,
        target: unit_target(2),
    }));

    simulation.step_actor(1, 11, &mut Vec::new()).unwrap();
    let source = &simulation.actors[&1];
    assert_eq!(source.motion.state, MotionState::Idle);
    assert_eq!(source.skill.lock_target, None);
    assert_eq!(source.skill.phase(), FightSkillPhase::Idle);

    simulation.step_actor(1, 12, &mut Vec::new()).unwrap();
    let source = &simulation.actors[&1];
    assert_eq!(source.motion.state, MotionState::Moving);
    assert_eq!(source.skill.lock_target, Some(unit_target(2)));
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
        source.skill.lock_target = Some(unit_target(2));
        source.motion.state = MotionState::Attacking;
        source.skill.set_phase(FightSkillPhase::Attack);
        source.skill.set_pending(Some(PendingRelease {
            step: 12,
            target: unit_target(2),
        }));
        simulation.actors.get_mut(&2).unwrap().life = 0;

        let mut events = Vec::new();
        simulation.step_actor(1, 11, &mut events).unwrap();
        let source = &simulation.actors[&1];
        assert_eq!(source.motion.state, MotionState::Idle, "{type_name}");
        assert_eq!(source.skill.lock_target, None, "{type_name}");
        assert_eq!(source.skill.phase(), FightSkillPhase::Idle, "{type_name}");
        assert!(source.skill.pending().is_none(), "{type_name}");
        assert!(events.is_empty(), "{type_name}");

        simulation.step_actor(1, 12, &mut events).unwrap();
        assert_eq!(
            simulation.actors[&1].skill.lock_target,
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
    source.skill.lock_target = Some(unit_target(2));
    source.motion.state = MotionState::Attacking;
    source.skill.set_phase(FightSkillPhase::Attack);
    source.skill.set_phase(FightSkillPhase::Attack);
    source.skill.set_pending(Some(PendingRelease {
        step: 10,
        target: unit_target(2),
    }));
    simulation.actors.get_mut(&2).unwrap().life = 1;
    let mut events = Vec::new();

    simulation.release(1, &mut events).unwrap();

    let source = &simulation.actors[&1];
    assert_eq!(source.motion.state, MotionState::Idle);
    assert_eq!(source.skill.lock_target, Some(unit_target(2)));
    assert!(source.skill.retarget_after_own_direct_kill);
    assert_eq!(source.skill.laser_attack_count, 1);
    assert!(matches!(events[0].payload, EventPayload::UnitDied { .. }));
    assert!(matches!(
        events[1].payload,
        EventPayload::Damage { amount: 1 }
    ));

    simulation.step_actor(1, 11, &mut Vec::new()).unwrap();
    let source = &simulation.actors[&1];
    assert_eq!(source.motion.state, MotionState::Idle);
    assert_eq!(source.skill.lock_target, None);
    assert!(!source.skill.retarget_after_own_direct_kill);
    assert_eq!(source.skill.laser_attack_count, 0);
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
    source.skill.lock_target = Some(unit_target(2));
    source.motion.state = MotionState::Attacking;
    source.skill.set_phase(FightSkillPhase::Attack);
    source.skill.set_pending(Some(PendingRelease {
        step: 10,
        target: unit_target(2),
    }));
    source.set_body_rotation(0);
    set_actor_position(simulation.actors.get_mut(&2).unwrap(), 3_000, 20_000);
    simulation.actors.get_mut(&2).unwrap().life = 1;

    simulation.step_actor(1, 10, &mut Vec::new()).unwrap();

    let source = &simulation.actors[&1];
    assert_eq!(source.motion.state, MotionState::Idle);
    assert_eq!(source.body_rotation, 0);
    assert_eq!(source.skill.lock_target, Some(unit_target(2)));
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
    source.skill.lock_target = Some(unit_target(2));
    source.motion.state = MotionState::Attacking;
    source.skill.set_phase(FightSkillPhase::Attack);
    source.skill.set_pending(Some(PendingRelease {
        step: 12,
        target: unit_target(2),
    }));
    simulation.actors.get_mut(&2).unwrap().life = 0;

    simulation.step_actor(1, 11, &mut Vec::new()).unwrap();

    let source = &simulation.actors[&1];
    assert_eq!(source.motion.state, MotionState::Attacking);
    assert_eq!(source.skill.lock_target, Some(unit_target(3)));
    assert_eq!(source.skill.pending().unwrap().target, unit_target(3));
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
                assert_eq!(
                    simulation.actors[&1].skill.lock_target,
                    Some(unit_target(3))
                );
                assert!(simulation.actors[&1].skill.retarget_after_own_direct_kill);
                assert_eq!(simulation.actors[&1].motion.state, MotionState::Idle);
            }
            224..=232 => {
                assert_eq!(
                    simulation.actors[&1].skill.lock_target,
                    Some(unit_target(3))
                );
                assert!(simulation.actors[&1].skill.retarget_after_own_direct_kill);
            }
            233 => {
                assert_eq!(simulation.actors[&1].skill.lock_target, None);
                assert!(!simulation.actors[&1].skill.retarget_after_own_direct_kill);
                assert_eq!(simulation.actors[&1].motion.state, MotionState::Idle);
            }
            234 => {
                // The exact native assignment point is not observable. This only
                // locks the simulator-private state needed to reproduce S/E tick 234.
                assert_eq!(
                    simulation.actors[&1].skill.lock_target,
                    Some(unit_target(2))
                );
                assert_eq!(simulation.actors[&1].motion.state, MotionState::Moving);
            }
            _ => {}
        }
    }
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
    actor.skill.weapon_rotations_q32 = vec![0, 41_i64 << 32];
    assert!(
        !Facing::Weapons(&actor.skill.weapon_rotations_q32)
            .faces(target, actor.rules.attack.attack_half_angle_mdeg())
    );
    actor.skill.weapon_rotations_q32[1] = 40_i64 << 32;
    assert!(
        Facing::Weapons(&actor.skill.weapon_rotations_q32)
            .faces(target, actor.rules.attack.attack_half_angle_mdeg())
    );
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
            216 | 225 => assert_eq!(rhino.skill.backswing_finish_step(), Some(224)),
            226 => {
                assert_eq!(rhino.skill.backswing_finish_step(), None);
                assert_eq!(rhino.skill.pending().unwrap().step, 233);
            }
            _ => {}
        }
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
        include_bytes!("../../../../../tests/regression/crawlers-vs-marksman.yaml"),
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
