use std::{fs, path::PathBuf};

use mechcore_mcfr::{EventKind, EventPayload, McfrReader};
use mechcore_simulation::{simulate_layout, simulate_layout_with_config};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeRegression {
    name: String,
    layout: PathBuf,
    game_build: String,
    schema_version: u32,
    seed: i32,
    tick_count: u64,
    scenario_hash: String,
    result_hash: String,
}

fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn native_regression(name: &str) -> NativeRegression {
    let manifest = repository().join("tests/mcfr-regressions.yaml");
    let regressions: Vec<NativeRegression> =
        serde_yaml::from_slice(&fs::read(manifest).unwrap()).unwrap();
    regressions
        .into_iter()
        .find(|regression| regression.name == name)
        .unwrap_or_else(|| panic!("missing native MCFR regression {name}"))
}

fn regression_layout(regression: &NativeRegression) -> PathBuf {
    repository().join(&regression.layout)
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/layouts/marksman-vs-arclight.yaml")
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
fn marksman_vs_arclight_matches_the_schema_v3_build_2259_native_recording() {
    let regression = native_regression("marksman-vs-arclight");
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("battle.mcfr");
    let result = simulate_layout(
        regression_layout(&regression),
        &output,
        Some(regression.seed),
    )
    .unwrap();
    assert_eq!(result.game_build, regression.game_build);
    assert_eq!(result.hashes.scenario_hash, regression.scenario_hash);
    assert_eq!(result.hashes.result_hash, regression.result_hash);
    let reader = McfrReader::open(output).unwrap();
    assert_eq!(reader.context().schema_version, regression.schema_version);
    assert_eq!(reader.tick_count(), regression.tick_count);
    let terminal = reader.state(reader.terminal_tick()).unwrap();
    assert!(
        terminal
            .units
            .iter()
            .all(|unit| unit.mech_lock_target.is_none())
    );
    assert!(
        terminal
            .buildings
            .iter()
            .all(|building| building.life == 3_400 && building.alive && building.targetable)
    );
}

#[test]
fn rhino_vs_arclight_matches_the_schema_v3_build_2259_native_recording() {
    let regression = native_regression("rhino-vs-arclight");
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("battle.mcfr");
    let result = simulate_layout(
        regression_layout(&regression),
        &output,
        Some(regression.seed),
    )
    .unwrap();
    assert_eq!(result.game_build, regression.game_build);
    assert_eq!(result.winner, Some("blue"));
    assert_eq!(result.steps, 235);
    assert_eq!(result.hashes.scenario_hash, regression.scenario_hash);
    assert_eq!(result.hashes.result_hash, regression.result_hash);

    let reader = McfrReader::open(output).unwrap();
    assert_eq!(reader.context().schema_version, regression.schema_version);
    assert_eq!(reader.tick_count(), regression.tick_count);
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
            .units
            .iter()
            .all(|unit| unit.mech_lock_target.is_none())
    );
    assert!(
        terminal
            .buildings
            .iter()
            .filter(|building| building.team_id == 1)
            .all(|building| building.life == 0 && !building.alive && !building.targetable)
    );
}

#[test]
fn rhino_retarget_matches_the_schema_v3_build_2259_native_recording() {
    let regression = native_regression("rhino-retarget");
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("battle.mcfr");
    let result = simulate_layout(
        regression_layout(&regression),
        &output,
        Some(regression.seed),
    )
    .unwrap();
    assert_eq!(result.game_build, regression.game_build);
    assert_eq!(result.winner, Some("blue"));
    assert_eq!(result.steps, 336);
    assert_eq!(result.hashes.scenario_hash, regression.scenario_hash);
    assert_eq!(result.hashes.result_hash, regression.result_hash);

    let reader = McfrReader::open(output).unwrap();
    assert_eq!(reader.context().schema_version, regression.schema_version);
    assert_eq!(reader.tick_count(), regression.tick_count);
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
            (1, 0, -284_800, -104_500, 358_447),
            (2, 1, -290_200, 99_500, 178_447),
            (3, 1, -190_300, 99_600, 204_857),
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
    assert_eq!(rhino_motion(222), mechcore_mcfr::MotionState::Idle);
    assert_eq!(rhino_motion(232), mechcore_mcfr::MotionState::Idle);
    assert_eq!(rhino_motion(233), mechcore_mcfr::MotionState::Moving);
    assert_eq!(rhino_motion(308), mechcore_mcfr::MotionState::Attacking);

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
            (204, 2, 3_560),
            (222, 2, 1_253),
            (317, 3, 3_560),
            (335, 3, 1_253)
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
