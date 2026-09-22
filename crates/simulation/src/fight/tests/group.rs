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

/// The two native checker captures distinguish sharing from leaving slots
/// empty, sibling exclusion from an ordinary re-search, and child range from
/// the core's range. Tick numbers are MCFR numbers (kernel step + 1).
#[test]
fn grouped_slots_follow_the_native_exclusion_and_fallback() {
    let config = SimulationConfig::load().unwrap();
    for (yaml, ticks) in [
        (
            include_bytes!("../../../../../tests/wraith/two-targets.yaml").as_slice(),
            10_u64,
        ),
        (
            include_bytes!("../../../../../tests/regression/wraith-group-attack.yaml").as_slice(),
            206_u64,
        ),
    ] {
        let (_, layout) = crate::layout::compile_with_seed(yaml, &config.units).unwrap();
        let mut sim = Simulation::new(
            &layout,
            &config.units,
            &config.training_ground,
            1_787_857_041,
        )
        .unwrap();
        for step in 0..ticks {
            sim.step(step).unwrap();
            if ticks == 10 && step == 9 {
                assert_eq!(
                    sim.actors[&1].skill.group_skill_targets,
                    [Some(3), Some(2), Some(3), Some(3)]
                );
            }
            if ticks == 206 && step == 130 {
                assert_eq!(
                    sim.actors[&30].skill.group_skill_targets,
                    [Some(23), Some(19), Some(25), Some(21)]
                );
            }
            if ticks == 206 && step == 205 {
                assert_eq!(
                    sim.actors[&29].skill.group_skill_targets,
                    [Some(41), Some(30), Some(48), Some(55)]
                );
                assert_eq!(sim.actors[&29].skill.lock_target, Some(unit_target(55)));
                assert!(!sim.slot_target_in_attack_range(
                    FightActorRef::Unit(29),
                    Some(0),
                    unit_target(55)
                ));
                assert!(sim.slot_target_in_attack_range(
                    FightActorRef::Unit(29),
                    Some(3),
                    unit_target(55)
                ));
            }
        }
    }
}

#[test]
fn grouped_child_range_is_parent_range_plus_ten_metres() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            Placement {
                type_name: "wraith".to_owned(),
                ..test_placement(0, 0, 0, 0)
            },
            test_placement(1, 0, 0, 80),
        ],
    );
    let mut sim = raw_test_simulation(&layout, &config, 7);
    set_actor_position(sim.actors.get_mut(&1).unwrap(), 0, 0);
    let radii = sim.actors[&1].rules.collision_radius() + sim.actors[&2].rules.collision_radius();
    set_actor_position(sim.actors.get_mut(&2).unwrap(), 0, radii + 70_000);
    assert!(!sim.slot_target_in_attack_range(FightActorRef::Unit(1), Some(0), unit_target(2)));
    assert!(sim.slot_target_in_attack_range(FightActorRef::Unit(1), Some(1), unit_target(2)));
    set_actor_position(sim.actors.get_mut(&2).unwrap(), 0, radii + 70_100);
    assert!(!sim.slot_target_in_attack_range(FightActorRef::Unit(1), Some(1), unit_target(2)));
}

#[test]
fn live_shared_lock_redistribution_is_refused_when_a_new_target_is_available() {
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
        ],
    );
    let mut sim = raw_test_simulation(&layout, &config, 7);
    sim.actors.get_mut(&1).unwrap().skill.group_skill_targets = vec![Some(2); 4];
    sim.refresh_target_query_snapshot();
    let order = sim.target_search_order();
    assert!(sim.check_group_redistribution_scope(1, 1, &order).is_err());
    sim.actors.get_mut(&3).unwrap().life = 0;
    assert!(sim.check_group_redistribution_scope(1, 1, &order).is_ok());
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

mod oracle {
    use super::*;
    use serde_json::Value;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Replay {
        tick: u64,
        calls: BTreeMap<(u64, u64), Vec<Value>>,
        checked: usize,
        differences: Vec<String>,
    }

    thread_local! {
        static REPLAY: RefCell<Option<Replay>> = const { RefCell::new(None) };
    }

    fn target(value: &Value) -> Option<FightActorRef> {
        let id = value["id"].as_u64()?;
        match value["kind"].as_str()? {
            "unit" => Some(FightActorRef::Unit(id)),
            "building" => Some(FightActorRef::Building(id)),
            other => panic!("unexpected target kind {other}"),
        }
    }

    impl Simulation {
        /// Replay on a shadow skill at the actual checker call site; never
        /// feed observed targets back into the simulation that verifies hashes.
        pub(in crate::fight) fn replay_group_checker_calls(&mut self, actor_id: u64) {
            let Some(mut replay) = REPLAY.with(|cell| cell.borrow_mut().take()) else {
                return;
            };
            if let Some(calls) = replay.calls.remove(&(replay.tick, actor_id)) {
                assert!(
                    matches!(calls.len(), 1 | 4),
                    "capture does not identify slots"
                );
                let saved = self.actors[&actor_id].skill.clone();
                // The captures visit slots in their group order. Each before
                // snapshot restores only that slot, preserving the previous
                // shadow call's changes to its siblings.
                for (slot, call) in calls.iter().enumerate() {
                    self.actors
                        .get_mut(&actor_id)
                        .unwrap()
                        .skill
                        .group_skill_targets[slot] =
                        target(&call["before"]["lock_target"]).and_then(FightActorRef::unit_id);
                }
                let order = self.target_search_order();
                for (slot, call) in calls.iter().enumerate() {
                    let before = &call["before"];
                    let lock = target(&before["lock_target"]);
                    let attack = target(&before["attack_target"]);
                    let skill = &mut self.actors.get_mut(&actor_id).unwrap().skill;
                    skill.group_skill_targets[slot] = lock.and_then(FightActorRef::unit_id);
                    skill.group_in_the_way[slot] = match (attack, lock) {
                        (Some(FightActorRef::Building(b)), Some(FightActorRef::Unit(u))) => {
                            Some((b, u))
                        }
                        _ => None,
                    };
                    let result = self
                        .check_attackable_slot(FightActorRef::Unit(actor_id), Some(slot), &order)
                        .unwrap();
                    let actual = (
                        result,
                        self.actors[&actor_id].skill.group_skill_targets[slot]
                            .map(FightActorRef::Unit),
                        self.actors[&actor_id].skill.group_attack_target(slot),
                    );
                    let expected = (
                        call["check_return"].as_bool().unwrap(),
                        target(&call["after"]["lock_target"]),
                        target(&call["after"]["attack_target"]),
                    );
                    if actual != expected {
                        replay.differences.push(format!(
                            "tick {} actor {actor_id} slot {slot}: {actual:?} != {expected:?}",
                            replay.tick
                        ));
                    }
                    replay.checked += 1;
                }
                self.actors.get_mut(&actor_id).unwrap().skill = saved;
            }
            REPLAY.with(|cell| *cell.borrow_mut() = Some(replay));
        }
    }

    #[test]
    #[ignore = "requires scripts/oracle.py fetch 81"]
    fn grouped_checker_matches_every_captured_call() {
        let config = SimulationConfig::load().unwrap();
        for (name, expected_count) in [("group-attack", 4188), ("two-targets", 344)] {
            let root = Path::new("/tmp/mechcore/wraith/slots");
            let recording = McfrReader::open(root.join(format!("{name}-checker.mcfr"))).unwrap();
            let capture =
                mechcore_mcfr::InstrumentationReader::open(root.join(format!("{name}-checker.h5")))
                    .unwrap();
            assert_eq!(capture.profile(), "skill_attackable_checker_v1");
            assert_eq!(
                capture.physics_result_hash(),
                recording.hashes().physics_result_hash
            );
            let (_, layout) =
                crate::layout::compile_with_seed(recording.layout_yaml().as_bytes(), &config.units)
                    .unwrap();
            let mut sim = Simulation::new(
                &layout,
                &config.units,
                &config.training_ground,
                1_787_857_041,
            )
            .unwrap();
            let mut replay = Replay::default();
            for index in 0..capture.len() {
                let entry = capture.entry(index).unwrap();
                let payload: Value = serde_json::from_slice(&entry.payload).unwrap();
                for call in payload["checker_calls"].as_array().unwrap() {
                    let actor = call["source_actor"]["id"].as_u64().unwrap();
                    if sim.actors[&actor].rules.attack.weapons.mode == WeaponMode::Group {
                        replay
                            .calls
                            .entry((entry.step, actor))
                            .or_default()
                            .push(call.clone());
                    }
                }
            }
            REPLAY.with(|cell| *cell.borrow_mut() = Some(replay));
            for step in 0..u64::from(recording.tick_count()) {
                REPLAY.with(|cell| cell.borrow_mut().as_mut().unwrap().tick = step + 1);
                sim.step(step).unwrap();
            }
            let replay = REPLAY.with(|cell| cell.borrow_mut().take().unwrap());
            assert!(
                replay.calls.is_empty(),
                "unreplayed calls: {:?}",
                replay.calls.keys().collect::<Vec<_>>()
            );
            assert_eq!(replay.checked, expected_count);
            eprintln!(
                "{name}: {}/{} calls match return, lock, and attack target",
                replay.checked - replay.differences.len(),
                replay.checked
            );
            assert!(
                replay.differences.is_empty(),
                "{} differences: {:?}",
                replay.differences.len(),
                &replay.differences[..replay.differences.len().min(20)]
            );
        }
    }
}
