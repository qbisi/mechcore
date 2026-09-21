use super::*;

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
        include_bytes!("../../../../../tests/construction/wall-line-of-fire.yaml"),
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
        include_bytes!("../../../../../tests/construction/wall-laser.yaml"),
        &config.units,
    )
    .unwrap();
    let mut simulation =
        Simulation::new(&layout, &config.units, &config.training_ground, 4242).unwrap();
    for step in 0..145 {
        simulation.step(step).unwrap();
    }
    assert!(matches!(
        simulation.actors[&4].skill.phase(),
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
        include_bytes!("../../../../../tests/construction/wall-block.yaml"),
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
        include_bytes!("../../../../../tests/construction/wall-line-of-fire.yaml"),
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
        marksman.skill.lock_target,
        Some(FightActorRef::Unit(behind_the_wall)),
        "the lock is never overwritten by the block"
    );
}
