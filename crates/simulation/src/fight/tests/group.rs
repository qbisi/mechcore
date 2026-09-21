use super::*;

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
    source.motion.state = MotionState::Attacking;
    source.skill.set_phase(FightSkillPhase::Attack);
    source.skill.lock_target = None;
    source.skill.group_skill_targets = vec![None, Some(2), Some(3), Some(4)];
    swap.refresh_target_query_snapshot();
    let order = swap.target_search_order();

    swap.update_group_skill_targets(1, 10, &order).unwrap();

    let source = &swap.actors[&1];
    assert_eq!(
        source.skill.group_skill_targets,
        [Some(2), Some(5), Some(3), Some(4)]
    );
    assert_eq!(source.skill.lock_target, Some(unit_target(5)));
    assert_eq!(source.skill.group_skill_prepare_ready_steps, [19, 19, 0, 0]);

    let mut shared = raw_test_simulation(&layout, &config, 7);
    let source = shared.actors.get_mut(&1).unwrap();
    source.motion.state = MotionState::Attacking;
    source.skill.set_phase(FightSkillPhase::Attack);
    source.skill.lock_target = Some(unit_target(5));
    source.skill.group_skill_targets = vec![Some(5), Some(2), Some(3), Some(4)];
    source.skill.group_skill_next_attack_steps = vec![0, 0, 0, 11];
    shared.actors.get_mut(&5).unwrap().life = 0;
    shared.refresh_target_query_snapshot();
    let order = shared.target_search_order();

    shared.update_group_skill_targets(1, 10, &order).unwrap();

    let source = &shared.actors[&1];
    assert_eq!(
        source.skill.group_skill_targets,
        [Some(2), Some(2), Some(3), Some(4)]
    );
    assert_eq!(source.skill.lock_target, Some(unit_target(2)));
    assert_eq!(source.skill.group_skill_prepare_ready_steps, [0, 0, 0, 0]);
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
    source.motion.state = MotionState::Attacking;
    source.skill.set_phase(FightSkillPhase::Attack);
    source.skill.lock_target = Some(unit_target(5));
    source.skill.group_skill_targets = vec![Some(5), Some(2), Some(3), Some(4)];
    simulation.actors.get_mut(&4).unwrap().life = 0;
    simulation.actors.get_mut(&5).unwrap().life = 0;
    simulation.refresh_target_query_snapshot();
    let order = simulation.target_search_order();

    simulation
        .update_group_skill_targets(1, 10, &order)
        .unwrap();

    let source = &simulation.actors[&1];
    assert!(source.skill.group_skill_targets[0].is_some());
    assert!(source.skill.group_skill_targets[3].is_some());
    assert_ne!(
        source.skill.group_skill_targets[0],
        source.skill.group_skill_targets[3]
    );
    assert_eq!(
        source.skill.lock_target,
        source.skill.group_skill_targets[3].map(FightActorRef::Unit)
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
    source.motion.state = MotionState::Attacking;
    source.skill.set_phase(FightSkillPhase::Attack);
    source.skill.lock_target = Some(unit_target(5));
    source.skill.group_skill_targets = vec![Some(2), Some(3), Some(4), Some(5)];
    source.skill.group_skill_next_attack_steps = vec![0, 0, 10, 0];
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
    assert_eq!(source.skill.group_skill_targets[2], Some(replacements[0]));
    assert_eq!(source.skill.group_skill_targets[3], Some(replacements[1]));
    assert_eq!(source.skill.lock_target, Some(unit_target(replacements[1])));
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
    source.motion.state = MotionState::Attacking;
    source.skill.set_phase(FightSkillPhase::Attack);
    source.skill.lock_target = Some(unit_target(5));
    source.skill.group_skill_targets = vec![Some(2), Some(3), Some(4), Some(5)];
    source.skill.group_skill_next_attack_steps = vec![0, 248, 228, 229];
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
    assert_eq!(source.skill.group_skill_targets[3], Some(replacements[0]));
    assert_eq!(source.skill.group_skill_targets[1], Some(replacements[1]));
    assert_eq!(source.skill.lock_target, Some(unit_target(replacements[0])));
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
        include_bytes!("../../../../../tests/construction/wall-weapon-group.yaml"),
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
