use super::*;

#[test]
fn grouped_bodyless_selector_scores_the_root_rotation() {
    // A Wraith's weapons turn on their own but carry no transform, so its
    // search scores from the root: facing 2.68°, the unit at 3 m to the
    // right is nearer the line than the one 8 m to the left, which its
    // first weapon, turned to 358.9°, would have chosen.
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            placement(0, "wraith", 0, 0, 0),
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

    assert_eq!(simulation.select_normal_unit_target(1).unwrap(), Some(3));
}

#[test]
fn grouped_skills_prime_one_attack_interval_sample_per_child() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            placement(0, "wraith", 0, 0, 0),
            placement(1, "wraith", 0, 0, 100),
        ],
    );
    let mut simulation =
        Simulation::new(&layout, &config.units, &config.towers, &config.maps, 7).unwrap();

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
            include_bytes!("../../../../../tests/wraith/fights/two-targets.yaml").as_slice(),
            10_u64,
        ),
        (
            include_bytes!("../../../../../tests/regression/fights/wraith-group-attack-01.yaml")
                .as_slice(),
            206_u64,
        ),
    ] {
        let layout = pinned_layout(yaml, &config.units);
        let mut sim = Simulation::new(
            &layout,
            &config.units,
            &config.towers,
            &config.maps,
            1_787_857_041,
        )
        .unwrap();
        for step in 0..ticks {
            sim.step(step).unwrap();
            if ticks == 10 && step == 9 {
                assert_eq!(
                    sim.actors[&1].skills.main.slot_locks(),
                    [3, 2, 3, 3].map(|unit| Some(unit_target(unit)))
                );
            }
            if ticks == 206 && step == 130 {
                assert_eq!(
                    sim.actors[&30].skills.main.slot_locks(),
                    [23, 19, 25, 21].map(|unit| Some(unit_target(unit)))
                );
            }
            if ticks == 206 && step == 205 {
                assert_eq!(
                    sim.actors[&29].skills.main.slot_locks(),
                    [41, 30, 48, 55].map(|unit| Some(unit_target(unit)))
                );
                // The core keeps its own lock; the unit's is the latest a
                // slot took, the fourth's.
                assert_eq!(
                    sim.actors[&29].skills.main.lock_target,
                    Some(unit_target(41))
                );
                assert_eq!(
                    sim.actors[&29].skills.main.unit_lock(),
                    Some(unit_target(55))
                );
                assert!(!sim.slot_target_in_attack_range(
                    SkillRef::main(FightActorRef::Unit(29)),
                    Some(0),
                    unit_target(55)
                ));
                assert!(sim.slot_target_in_attack_range(
                    SkillRef::main(FightActorRef::Unit(29)),
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
        vec![placement(0, "wraith", 0, 0, 0), test_placement(1, 0, 0, 80)],
    );
    let mut sim = raw_test_simulation(&layout, &config, 7);
    set_actor_position(sim.actors.get_mut(&1).unwrap(), 0, 0);
    let radii = sim.actors[&1].rules.collision_radius() + sim.actors[&2].rules.collision_radius();
    set_actor_position(sim.actors.get_mut(&2).unwrap(), 0, radii + 70_000);
    assert!(!sim.slot_target_in_attack_range(
        SkillRef::main(FightActorRef::Unit(1)),
        Some(0),
        unit_target(2)
    ));
    assert!(sim.slot_target_in_attack_range(
        SkillRef::main(FightActorRef::Unit(1)),
        Some(1),
        unit_target(2)
    ));
    set_actor_position(sim.actors.get_mut(&2).unwrap(), 0, radii + 70_100);
    assert!(!sim.slot_target_in_attack_range(
        SkillRef::main(FightActorRef::Unit(1)),
        Some(1),
        unit_target(2)
    ));
}

/// A Wraith's sibling slot, to set up and read in place.
fn slot(sim: &mut Simulation, slot: usize) -> &mut Skill {
    sim.actors
        .get_mut(&1)
        .unwrap()
        .skills
        .main
        .sibling_mut(slot)
}

/// An attacking sibling gives up a unit it shares when every other sharer but
/// the core has struck more blows, its search timer is up, and a search
/// finds a unit no slot holds; a unit it holds alone, a group with a slot
/// holding nothing, a sharer with as few blows, and a search that finds
/// only held units all keep it.
#[test]
fn an_attacking_sibling_gives_up_a_shared_unit_by_its_blows() {
    let config = SimulationConfig::load().unwrap();
    let layout = CompiledLayout::of_units(
        1,
        vec![
            placement(0, "wraith", 0, 0, 0),
            test_placement(1, 0, 0, 30),
            test_placement(1, 1, 0, 40),
            test_placement(1, 2, 0, 50),
            test_placement(1, 3, 10, 45),
        ],
    );
    let mut sim = raw_test_simulation(&layout, &config, 7);
    let skill = &mut sim.actors.get_mut(&1).unwrap().skills.main;
    skill.lock_target = Some(unit_target(2));
    for (slot, unit) in [(1, 2), (2, 3), (3, 4)] {
        skill.sibling_mut(slot).lock_target = Some(unit_target(unit));
    }
    skill.sibling_mut(1).search_target_time = 1;
    sim.refresh_target_query_snapshot();
    let order = sim.target_search_order();

    assert!(!sim.sibling_yields(1, 1, &order).unwrap(), "timer not up");
    slot(&mut sim, 1).search_target_time = 0;
    assert!(
        sim.sibling_yields(1, 1, &order).unwrap(),
        "shared with the core"
    );
    assert_eq!(slot(&mut sim, 1).search_target_time, 10);

    slot(&mut sim, 1).search_target_time = 0;
    assert!(!sim.sibling_yields(1, 2, &order).unwrap(), "held alone");

    slot(&mut sim, 1).attack_count = 0;
    slot(&mut sim, 2).lock_target = Some(unit_target(2));
    assert!(
        !sim.sibling_yields(1, 1, &order).unwrap(),
        "a sharer with fewer blows"
    );
    slot(&mut sim, 2).attack_count = 0;
    assert!(
        !sim.sibling_yields(1, 1, &order).unwrap(),
        "a sharer with as many"
    );
    slot(&mut sim, 2).attack_count = 1;
    assert!(
        sim.sibling_yields(1, 1, &order).unwrap(),
        "every sharer has more"
    );

    slot(&mut sim, 1).search_target_time = 0;
    for unit in [3, 5] {
        sim.actors.get_mut(&unit).unwrap().life = 0;
    }
    assert!(
        !sim.sibling_yields(1, 1, &order).unwrap(),
        "nothing unheld to find"
    );
    slot(&mut sim, 3).lock_target = None;
    assert!(
        !sim.sibling_yields(1, 1, &order).unwrap(),
        "a slot holds nothing"
    );
}

/// Every slot of a grouped skill takes the construction in its way, and
/// every slot is dropped with the lock.
///
/// The Wraith of `tests/construction/fights/wall-weapon-group.yaml` was
/// recorded doing all of it: its core engages block 3 at tick 32 and the
/// other three slots follow eight ticks later, while the lock stays on the
/// Marksman; block 3 falls at tick 59 and all four slots read empty at
/// tick 60; the core engages block 4 at tick 74 and the children are
/// allocated again eight ticks after that, not at once.
#[test]
fn grouped_slots_take_the_wall_and_are_dropped_with_the_lock() {
    let config = SimulationConfig::load().unwrap();
    let layout = pinned_layout(
        include_bytes!("../../../../../tests/construction/fights/wall-weapon-group.yaml"),
        &config.units,
    );
    let mut simulation =
        Simulation::new(&layout, &config.units, &config.towers, &config.maps, 4242).unwrap();
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
    use std::{cell::RefCell, path::Path};

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
                let saved = self.actors[&actor_id].skills.main.clone();
                // Each call names its slot. The captures visit slots in their
                // group order, and each before snapshot restores only that
                // slot, preserving the previous shadow call's changes to its
                // siblings.
                let slot_of = |call: &Value| {
                    usize::try_from(
                        call["skill_slot"]
                            .as_u64()
                            .expect("a grouped call names its slot"),
                    )
                    .unwrap()
                };
                for call in &calls {
                    let slot = slot_of(call);
                    let lock = target(&call["before"]["lock_target"]);
                    let skill = &mut self.actors.get_mut(&actor_id).unwrap().skills.main;
                    if slot == 0 {
                        skill.lock_target = lock;
                    } else {
                        skill.sibling_mut(slot).lock_target = lock;
                    }
                }
                let order = self.target_search_order();
                for call in &calls {
                    let slot = slot_of(call);
                    let before = &call["before"];
                    let lock = target(&before["lock_target"]);
                    let attack = target(&before["attack_target"]);
                    let skill = &mut self.actors.get_mut(&actor_id).unwrap().skills.main;
                    let in_the_way = match (attack, lock) {
                        (Some(FightActorRef::Building(b)), Some(lock @ FightActorRef::Unit(_))) => {
                            Some((b, lock))
                        }
                        _ => None,
                    };
                    if slot == 0 {
                        skill.lock_target = lock;
                        skill.in_the_way = in_the_way;
                    } else {
                        let sibling = skill.sibling_mut(slot);
                        sibling.lock_target = lock;
                        sibling.in_the_way = in_the_way;
                    }
                    let result = self
                        .check_attackable_slot(
                            SkillRef::main(FightActorRef::Unit(actor_id)),
                            Some(slot),
                            call["is_attacking_check"].as_bool().unwrap(),
                            &order,
                        )
                        .unwrap();
                    let actual = (
                        result,
                        self.actors[&actor_id].skills.main.slot_lock(slot),
                        self.actors[&actor_id].skills.main.group_attack_target(slot),
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
                self.actors.get_mut(&actor_id).unwrap().skills.main = saved;
            }
            REPLAY.with(|cell| *cell.borrow_mut() = Some(replay));
        }
    }

    #[test]
    #[ignore = "requires the recordings tests/wraith/README.md records where the game runs"]
    fn grouped_checker_matches_every_captured_call() {
        let config = SimulationConfig::load().unwrap();
        for (name, expected_count) in [("wraith-group-attack-01", 4188), ("two-targets", 344)] {
            let root = Path::new("/tmp/mechcore/wraith/slots");
            let recording = McfrReader::open(root.join(format!("{name}.mcfr"))).unwrap();
            let calls = recording
                .instrument::<mechcore_mcfr::SkillAttackableCheck>()
                .unwrap()
                .expect("the recording carries the skill_attackable_checker channel");
            let (_, layout) =
                crate::layout::compile_with_seed(recording.layout_yaml().as_bytes(), &config.units)
                    .unwrap();
            let mut sim = Simulation::new(
                &layout,
                &config.units,
                &config.towers,
                &config.maps,
                1_787_857_041,
            )
            .unwrap();
            let mut replay = Replay::default();
            for (tick, call) in calls {
                let actor = call.source_actor.id;
                if sim.actors[&actor].rules.attack.weapons.mode == WeaponMode::Group {
                    replay
                        .calls
                        .entry((u64::from(tick), actor))
                        .or_default()
                        .push(serde_json::to_value(call).unwrap());
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

/// Every grouped unit's slots, tick by tick, against a recording's
/// `group_slots` channel: the lock, the attack target and the state of each
/// slot, with the unit's lock and motion beside them.
mod slots {
    use super::*;
    use crate::fight::skill::SkillState;
    use mechcore_mcfr::{GroupSlot, McfrReader, ObjectRef};
    use std::path::Path;

    fn object(target: Option<FightActorRef>) -> Option<ObjectRef> {
        target.map(FightActorRef::object_ref)
    }

    fn state_name(state: SkillState) -> &'static str {
        match state {
            SkillState::Idle { .. } => "SkillIdleState",
            SkillState::Prepare { .. } => "SkillPrepareState",
            SkillState::Attack(_) => "SkillAttackState",
            SkillState::Cooling { .. } => "SkillCoolingState",
            SkillState::Reloading { .. } => "SkillReloadingState",
        }
    }

    /// The first tick a recording's slots and the simulator's part, and how.
    fn first_difference(path: &Path) -> Option<String> {
        let config = SimulationConfig::load().unwrap();
        let recording = McfrReader::open(path).unwrap();
        let rows = recording
            .instrument::<GroupSlot>()
            .unwrap()
            .expect("the recording carries the group_slots channel");
        let (_, layout) =
            crate::layout::compile_with_seed(recording.layout_yaml().as_bytes(), &config.units)
                .unwrap();
        let mut sim = Simulation::new(
            &layout,
            &config.units,
            &config.towers,
            &config.maps,
            recording.context().match_seed,
        )
        .unwrap();
        let mut by_tick: BTreeMap<u32, Vec<GroupSlot>> = BTreeMap::new();
        for (tick, row) in rows {
            by_tick.entry(tick).or_default().push(row);
        }
        for tick in 1..=recording.tick_count() {
            if let Err(error) = sim.step(u64::from(tick) - 1) {
                return Some(format!("tick {tick}: the simulator refused: {error}"));
            }
            let game = recording.state(tick).unwrap();
            let mut differences = Vec::new();
            for row in by_tick.get(&tick).into_iter().flatten() {
                let Some(actor) = sim.actors.get(&row.unit.id).filter(|actor| actor.alive()) else {
                    differences.push(format!("u{} is gone", row.unit.id));
                    continue;
                };
                let slot = usize::from(row.skill_slot);
                if slot >= actor.skills.main.group_size() {
                    differences.push(format!("u{} has no slot {slot}", row.unit.id));
                    continue;
                }
                // The core is the unit's own skill.
                let (lock, attack, state) = if slot == 0 {
                    (
                        object(actor.skills.main.lock_target),
                        object(actor.skills.main.group_attack_target(0)),
                        // A cooling's last step reads idle, its weapon
                        // cleared, as a sibling's does.
                        match actor.skills.main.state {
                            SkillState::Cooling { started, .. }
                                if u64::from(tick)
                                    > started.saturating_add(native_time_units_to_steps(
                                        actor.rules.attack.cooling_time_units(),
                                    )) =>
                            {
                                "SkillIdleState"
                            }
                            state => state_name(state),
                        },
                    )
                } else {
                    let held = actor.skills.main.sibling(slot);
                    (
                        object(held.lock_target),
                        object(held.attack_target()),
                        state_name(held.state),
                    )
                };
                let recorded_state = row.skill_state.as_deref().unwrap_or("-");
                if lock != row.lock_target || attack != row.attack_target || state != recorded_state
                {
                    differences.push(format!(
                        "u{} s{slot}: game {:?}/{:?} {recorded_state}, sim {lock:?}/{attack:?} {state}",
                        row.unit.id, row.lock_target, row.attack_target
                    ));
                }
            }
            let ours = sim.snapshot();
            for unit in &game.live_units {
                if !by_tick
                    .get(&tick)
                    .is_some_and(|rows| rows.iter().any(|row| row.unit.id == unit.unit_id))
                {
                    continue;
                }
                let Some(mine) = ours.live_units.iter().find(|u| u.unit_id == unit.unit_id) else {
                    continue;
                };
                if mine.mech_lock_target != unit.mech_lock_target
                    || mine.motion_state != unit.motion_state
                {
                    differences.push(format!(
                        "u{} unit: game lock {:?} {:?}, sim lock {:?} {:?}",
                        unit.unit_id,
                        unit.mech_lock_target,
                        unit.motion_state,
                        mine.mech_lock_target,
                        mine.motion_state
                    ));
                }
            }
            if !differences.is_empty() {
                return Some(format!("tick {tick}: {}", differences.join("; ")));
            }
        }
        None
    }

    #[test]
    #[ignore = "requires the recordings tests/wraith/README.md and tests/raiden/README.md record where the game runs"]
    fn grouped_slots_match_every_recorded_tick() {
        let mut parted = Vec::new();
        let mut paths = ["/tmp/mechcore/wraith/slots", "/tmp/mechcore/raiden/slots"]
            .into_iter()
            .flat_map(|root| std::fs::read_dir(root).unwrap())
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "mcfr")
            })
            .collect::<Vec<_>>();
        paths.sort();
        if let Ok(only) = std::env::var("SLOT_ONLY") {
            paths.retain(|path| {
                path.file_stem()
                    .is_some_and(|stem| stem.to_string_lossy() == only)
            });
        }
        for path in paths {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            match first_difference(&path) {
                None => eprintln!("{name}: every tick agrees"),
                Some(difference) => {
                    eprintln!("{name}: {difference}");
                    parted.push(name);
                }
            }
        }
        assert!(parted.is_empty(), "parted: {parted:?}");
    }
}
