use std::{fs, path::PathBuf};

use mechcore_mcfr::{EventKind, McfrReader};
use mechcore_simulation::simulate_layout;
use serde::Deserialize;

/// Deserialized strictly, so a manifest field added without a reader fails here
/// rather than being silently ignored. `smoke`, `format` and the hash are what
/// the mcscript readers check, and have no consumer in this file.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeRegression {
    name: String,
    #[allow(dead_code)]
    smoke: bool,
    layout: PathBuf,
    #[allow(dead_code)]
    format: String,
    seed: i32,
    tick_count: u32,
    #[allow(dead_code)]
    physics_result_hash: String,
}

fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn native_regressions() -> Vec<NativeRegression> {
    let manifest = repository().join("tests/regression/mcfr-regressions.yaml");
    serde_yaml::from_slice(&fs::read(manifest).unwrap()).unwrap()
}

fn native_regression(name: &str) -> NativeRegression {
    native_regressions()
        .into_iter()
        .find(|regression| regression.name == name)
        .unwrap_or_else(|| panic!("missing native MCFR regression {name}"))
}

fn regression_layout(regression: &NativeRegression) -> PathBuf {
    repository().join(&regression.layout)
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/regression/marksman-vs-arclight.yaml")
}

#[test]
fn marksman_vs_arclight_runs_to_a_readable_terminal_result() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("battle.mcfr");
    let result = simulate_layout(fixture(), Some(&output), Some(7)).unwrap();
    assert_eq!(result.game_build, mechcore_document::game_build());
    assert_eq!(result.seed, 7);
    assert_eq!(result.seed_source, "external");
    assert_eq!(result.end_reason, "natural_module_drain");
    assert_eq!(result.output.as_deref(), output.to_str());
    assert!(result.profiling.generation_duration_milliseconds > 0.0);
    assert!(result.profiling.simulation_to_real_time_rate > 0.0);
    assert!(result.winner.is_some());
    assert!(!result.draw);

    let reader = McfrReader::open(&output).unwrap();
    assert_eq!(reader.game_build(), mechcore_document::game_build());
    assert_eq!(reader.hashes(), &result.hashes);
    assert_eq!(
        Some(reader.file_size_bytes()),
        result.profiling.file_size_bytes
    );
    assert_eq!(
        Some(reader.member_sizes_bytes()),
        result.profiling.member_sizes_bytes.as_ref()
    );
    assert_eq!(u64::from(reader.terminal_tick()), result.steps);
    let final_state = reader.state(reader.terminal_tick()).unwrap();
    assert_eq!(final_state.live_units.len(), 1);
    let event_kinds = (1..=reader.tick_count())
        .flat_map(|tick| reader.events(tick).unwrap().events)
        .map(|event| event.kind())
        .collect::<Vec<_>>();
    assert!(event_kinds.contains(&EventKind::ProjectileReleased));
    assert!(event_kinds.contains(&EventKind::Damage));
    assert!(event_kinds.contains(&EventKind::ProjectileRemoved));
}

/// Runs a native regression case through the simulator and opens what it
/// wrote.
///
/// Its physics hash is not checked here: `tests/regression/simulate.mcscript`
/// holds every smoke case to it, and the hash already covers positions, life
/// and every event, damage included. What these tests check is the content
/// layer the hash leaves out — a unit's lock and its motion state.
fn recorded(name: &str) -> (tempfile::TempDir, McfrReader) {
    let regression = native_regression(name);
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("battle.mcfr");
    simulate_layout(
        regression_layout(&regression),
        Some(&output),
        Some(regression.seed),
    )
    .unwrap();
    let reader = McfrReader::open(output).unwrap();
    assert_eq!(reader.tick_count(), regression.tick_count);
    (directory, reader)
}

#[test]
fn marksman_vs_arclight_ends_with_no_lock() {
    let (_directory, reader) = recorded("marksman-vs-arclight");
    let terminal = reader.state(reader.terminal_tick()).unwrap();
    assert!(
        terminal
            .live_units
            .iter()
            .all(|unit| unit.mech_lock_target.is_none())
    );
}

#[test]
fn rhino_vs_arclight_ends_with_no_lock() {
    let (_directory, reader) = recorded("rhino-vs-arclight");
    let terminal = reader.state(reader.terminal_tick()).unwrap();
    assert!(
        terminal
            .live_units
            .iter()
            .all(|unit| unit.mech_lock_target.is_none())
    );
}

#[test]
fn rhino_retarget_waits_idle_then_moves_and_attacks() {
    let (_directory, reader) = recorded("rhino-retarget");
    let rhino_motion = |tick| {
        reader
            .state(tick)
            .unwrap()
            .live_units
            .into_iter()
            .find(|unit| unit.unit_id == 1)
            .unwrap()
            .motion_state
    };
    assert_eq!(rhino_motion(222), mechcore_mcfr::MotionState::Idle);
    assert_eq!(rhino_motion(232), mechcore_mcfr::MotionState::Idle);
    assert_eq!(rhino_motion(233), mechcore_mcfr::MotionState::Moving);
    assert_eq!(rhino_motion(308), mechcore_mcfr::MotionState::Attacking);
}

#[test]
fn the_same_layout_and_seed_have_identical_semantic_hashes() {
    let first = simulate_layout(fixture(), None, Some(-19)).unwrap();
    let second = simulate_layout(fixture(), None, Some(-19)).unwrap();
    assert_eq!(first.hashes, second.hashes);
    assert_eq!(first.winner, second.winner);
    assert_eq!(first.steps, second.steps);
}

#[test]
fn generated_seed_is_reported_and_replayable() {
    let generated = simulate_layout(fixture(), None, None).unwrap();
    assert_eq!(generated.seed_source, "generated");
    assert_eq!(generated.output, None);
    let replayed = simulate_layout(fixture(), None, Some(generated.seed)).unwrap();
    assert_eq!(generated.hashes, replayed.hashes);
}

#[test]
fn layout_seed_is_used_and_external_seed_overrides_it() {
    let directory = tempfile::tempdir().unwrap();
    let layout = directory.path().join("seeded.yaml");
    let source = fs::read_to_string(fixture()).unwrap();
    fs::write(&layout, format!("seed: -17\n{source}")).unwrap();

    let from_layout = simulate_layout(&layout, None, None).unwrap();
    assert_eq!(from_layout.seed, -17);
    assert_eq!(from_layout.seed_source, "layout");

    let overridden = simulate_layout(&layout, None, Some(23)).unwrap();
    assert_eq!(overridden.seed, 23);
    assert_eq!(overridden.seed_source, "external");
}

#[test]
fn the_zero_seed_sentinel_is_refused_from_either_source() {
    let directory = tempfile::tempdir().unwrap();
    let layout = directory.path().join("zero.yaml");
    let source = fs::read_to_string(fixture()).unwrap();
    fs::write(&layout, format!("seed: 0\n{source}")).unwrap();
    assert!(
        simulate_layout(&layout, None, None)
            .unwrap_err()
            .to_string()
            .contains("native system-random request")
    );

    assert!(
        simulate_layout(fixture(), None, Some(0))
            .unwrap_err()
            .to_string()
            .contains("system-random request")
    );
}
