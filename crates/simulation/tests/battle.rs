use std::{fs, path::PathBuf};

use mechcore_mcfr::{EventKind, EventPayload, McfrReader};
use mechcore_simulation::{simulate_layout, simulate_layout_with_config};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/layouts/marksman-vs-arclight.yaml")
}

fn rhino_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/layouts/rhino-vs-arclight.yaml")
}

fn rhino_retarget_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/layouts/rhino-retarget-vs-arclights.yaml")
}

#[test]
fn public_simulation_rejects_unclosed_multi_member_behavior_before_creating_an_mcfr() {
    let directory = tempfile::tempdir().unwrap();
    let layout = directory.path().join("crawler.yaml");
    let output = directory.path().join("battle.mcfr");
    fs::write(
        &layout,
        r"
round: 1
sides:
  blue:
    formations: [{type: crawler, x: 5, y: -50}]
  red:
    formations: [{type: arclight, x: 0, y: -50}]
",
    )
    .unwrap();

    let error = simulate_layout(&layout, &output, Some(1_787_601_811))
        .unwrap_err()
        .to_string();
    assert!(error.contains("not in the current kernel's supported behavior set"));
    assert!(!output.exists());
}

#[test]
fn marksman_vs_arclight_runs_to_a_readable_terminal_result() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("battle.mcfr");
    let result = simulate_layout(fixture(), &output, Some(7)).unwrap();
    assert_eq!(result.game_build, "1.11.1.3.2259");
    assert_eq!(result.seed, 7);
    assert_eq!(result.seed_source, "external");
    assert_eq!(result.end_reason, "natural_module_drain");
    assert!(result.winner.is_some());
    assert!(!result.draw);

    let reader = McfrReader::open(&output).unwrap();
    assert_eq!(reader.context().game_build, "1.11.1.3.2259");
    assert_eq!(reader.hashes(), &result.hashes);
    assert_eq!(reader.terminal_tick(), result.steps);
    let final_state = reader.state(reader.terminal_tick()).unwrap();
    assert_eq!(
        final_state.units.iter().filter(|unit| unit.alive).count(),
        1
    );
    let event_kinds = (0..reader.tick_count())
        .flat_map(|tick| reader.events(tick).unwrap().events)
        .map(|event| event.kind())
        .collect::<Vec<_>>();
    assert!(event_kinds.contains(&EventKind::ProjectileReleased));
    assert!(event_kinds.contains(&EventKind::Damage));
    assert!(event_kinds.contains(&EventKind::ProjectileRemoved));
}

#[test]
fn marksman_vs_arclight_matches_the_build_2259_native_recording() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("battle.mcfr");
    let result = simulate_layout(fixture(), &output, Some(1_787_551_408)).unwrap();
    assert_eq!(
        result.hashes.scenario_hash,
        "af4c7c7410377edb84ec3106ad2fe2427864e313d42092351e217462c025efee"
    );
    assert_eq!(
        result.hashes.result_hash,
        "6dc45a4ef0bd5b469f727790555ff6bd47c38cadb1e0d0aebf97b260c5059fc5"
    );
    let reader = McfrReader::open(output).unwrap();
    assert_eq!(reader.tick_count(), 92);
    assert!(
        reader
            .state(reader.terminal_tick())
            .unwrap()
            .buildings
            .iter()
            .all(|building| building.life == 3_400 && building.alive && building.targetable)
    );
}

#[test]
fn rhino_vs_arclight_matches_the_build_2259_native_recording() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("battle.mcfr");
    let result = simulate_layout(rhino_fixture(), &output, Some(1_787_591_883)).unwrap();
    assert_eq!(result.winner, Some("blue"));
    assert_eq!(result.steps, 235);
    assert_eq!(
        result.hashes.scenario_hash,
        "c16a57a90f13f9ac4791bd25c3a35d2926b943643f83fc87ca38fc73b5fd08c2"
    );
    assert_eq!(
        result.hashes.result_hash,
        "0709395104ff9c229874e5d4216f04da5062694f780d49f07f7be6be60d68a4e"
    );

    let reader = McfrReader::open(output).unwrap();
    assert_eq!(reader.tick_count(), 236);
    let direct_damage = (0..reader.tick_count())
        .flat_map(|tick| reader.events(tick).unwrap().events)
        .filter_map(|event| match event.payload {
            EventPayload::Damage { amount } if event.source.is_none() => {
                Some((event.target.unwrap().id, amount))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(direct_damage, [(2, 3_560), (2, 1_253)]);

    let terminal = reader.state(reader.terminal_tick()).unwrap();
    assert!(
        terminal
            .buildings
            .iter()
            .filter(|building| building.team_id == 1)
            .all(|building| building.life == 0 && !building.alive && !building.targetable)
    );
}

#[test]
fn rhino_retarget_matches_the_build_2259_native_recording() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("battle.mcfr");
    let result = simulate_layout(rhino_retarget_fixture(), &output, Some(1_787_601_811)).unwrap();
    assert_eq!(result.winner, Some("blue"));
    assert_eq!(result.steps, 337);
    assert_eq!(
        result.hashes.scenario_hash,
        "9e7c8f5e5e7a3c3cbaaa8352f03dba1d6fc62493b2da1ff1356c27f771943fcf"
    );
    assert_eq!(
        result.hashes.result_hash,
        "02302cb6f2d780fc507e45747e3669d11b30947159592602a69072c2e22303d4"
    );

    let reader = McfrReader::open(output).unwrap();
    assert_eq!(reader.tick_count(), 338);
    let initial = reader.state(0).unwrap();
    assert_eq!(
        initial
            .units
            .iter()
            .map(|unit| (
                unit.unit_id,
                unit.team_id,
                unit.position.x,
                unit.position.z,
                unit.body_rotation,
            ))
            .collect::<Vec<_>>(),
        [
            (1, 0, -284_400, -104_900, 358_219),
            (2, 1, -189_500, 99_300, 204_939),
            (3, 1, -290_600, 99_900, 178_219),
        ]
    );

    let rhino_motion = |tick| {
        reader
            .state(tick)
            .unwrap()
            .units
            .into_iter()
            .find(|unit| unit.unit_id == 1)
            .unwrap()
            .motion_state
    };
    assert_eq!(rhino_motion(223), mechcore_mcfr::MotionState::Idle);
    assert_eq!(rhino_motion(233), mechcore_mcfr::MotionState::Idle);
    assert_eq!(rhino_motion(234), mechcore_mcfr::MotionState::Moving);
    assert_eq!(rhino_motion(309), mechcore_mcfr::MotionState::Attacking);

    let direct_damage = (0..reader.tick_count())
        .flat_map(|tick| {
            reader
                .events(tick)
                .unwrap()
                .events
                .into_iter()
                .filter_map(move |event| match event.payload {
                    EventPayload::Damage { amount } if event.source.is_none() => {
                        Some((tick, event.target.unwrap().id, amount))
                    }
                    _ => None,
                })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        direct_damage,
        [
            (205, 3, 3_560),
            (223, 3, 1_253),
            (318, 2, 3_560),
            (336, 2, 1_253)
        ]
    );
}

#[test]
fn the_same_layout_and_seed_have_identical_semantic_hashes() {
    let directory = tempfile::tempdir().unwrap();
    let first = simulate_layout(fixture(), directory.path().join("first.mcfr"), Some(-19)).unwrap();
    let second =
        simulate_layout(fixture(), directory.path().join("second.mcfr"), Some(-19)).unwrap();
    assert_eq!(first.hashes, second.hashes);
    assert_eq!(first.winner, second.winner);
    assert_eq!(first.steps, second.steps);
}

#[test]
fn generated_seed_is_reported_and_replayable() {
    let directory = tempfile::tempdir().unwrap();
    let generated =
        simulate_layout(fixture(), directory.path().join("generated.mcfr"), None).unwrap();
    assert_eq!(generated.seed_source, "generated");
    let replayed = simulate_layout(
        fixture(),
        directory.path().join("replayed.mcfr"),
        Some(generated.seed),
    )
    .unwrap();
    assert_eq!(generated.hashes, replayed.hashes);
}

#[test]
fn unit_names_are_config_data_not_kernel_branches() {
    let directory = tempfile::tempdir().unwrap();
    let baseline =
        simulate_layout(fixture(), directory.path().join("baseline.mcfr"), Some(7)).unwrap();

    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source_units = repository.join("config/units");
    let config_root = directory.path().join("config");
    let units = config_root.join("units");
    fs::create_dir_all(&units).unwrap();
    fs::copy(
        repository.join("config/config.yaml"),
        config_root.join("config.yaml"),
    )
    .unwrap();
    fs::copy(
        repository.join("config/training_ground.yaml"),
        config_root.join("training_ground.yaml"),
    )
    .unwrap();
    for (source, renamed) in [("marksman", "unit_a"), ("arclight", "unit_b")] {
        let mut config: serde_yaml::Value =
            serde_yaml::from_slice(&fs::read(source_units.join(format!("{source}.yaml"))).unwrap())
                .unwrap();
        config["type_name"] = serde_yaml::Value::String(renamed.to_owned());
        fs::write(
            units.join(format!("{renamed}.yaml")),
            serde_yaml::to_string(&config).unwrap(),
        )
        .unwrap();
    }
    let layout = directory.path().join("layout.yaml");
    fs::write(
        &layout,
        fs::read_to_string(fixture())
            .unwrap()
            .replace("marksman", "unit_a")
            .replace("arclight", "unit_b"),
    )
    .unwrap();

    let renamed = simulate_layout_with_config(
        layout,
        directory.path().join("renamed.mcfr"),
        Some(7),
        Some(&config_root),
    )
    .unwrap();
    assert_eq!(renamed.winner, baseline.winner);
    assert_eq!(renamed.steps, baseline.steps);
    assert_eq!(renamed.hashes, baseline.hashes);
}
