use std::{fs, path::PathBuf};

use mechcore_mcfr::{EventKind, McfrReader};
use mechcore_simulation::{simulate_layout, simulate_layout_with_config};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/layouts/marksman-vs-arclight.yaml")
}

#[test]
fn marksman_vs_arclight_runs_to_a_verified_terminal_result() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("battle.mcfr");
    let result = simulate_layout(fixture(), &output, Some(7)).unwrap();
    assert_eq!(result.game_build, "1.11.1.3.2259");
    assert_eq!(result.seed, 7);
    assert_eq!(result.seed_source, "external");
    assert_eq!(result.end_reason, "natural_module_drain");
    assert!(result.winner.is_some());
    assert!(!result.draw);

    let reader = McfrReader::open_verified(&output).unwrap();
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
