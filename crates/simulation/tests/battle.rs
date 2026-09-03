use std::{collections::BTreeMap, fs, path::PathBuf};

use mechcore_mcfr::{EventKind, EventPayload, McfrReader};
use mechcore_simulation::simulate_layout;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeRegression {
    name: String,
    smoke: bool,
    layout: PathBuf,
    game_build: String,
    format: String,
    seed: i32,
    tick_count: u32,
    physics_result_hash: String,
}

fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn native_regressions() -> Vec<NativeRegression> {
    let manifest = repository().join("tests/mcfr-regressions.yaml");
    serde_yaml::from_slice(&fs::read(manifest).unwrap()).unwrap()
}

fn native_regression(name: &str) -> NativeRegression {
    native_regressions()
        .into_iter()
        .find(|regression| regression.name == name)
        .unwrap_or_else(|| panic!("missing native MCFR regression {name}"))
}

fn current_native_regressions() -> Vec<NativeRegression> {
    native_regressions()
        .into_iter()
        .filter(|regression| regression.format == mechcore_mcfr::MCFR_FORMAT)
        .collect()
}

fn smoke_native_regressions() -> Vec<NativeRegression> {
    let regressions = current_native_regressions();
    let mut smoke_counts = BTreeMap::<PathBuf, usize>::new();
    for regression in &regressions {
        smoke_counts.entry(regression.layout.clone()).or_default();
        if regression.smoke {
            *smoke_counts.entry(regression.layout.clone()).or_default() += 1;
        }
    }
    for (layout, count) in smoke_counts {
        assert_eq!(
            count,
            1,
            "{} must have exactly one smoke case",
            layout.display()
        );
    }
    regressions
        .into_iter()
        .filter(|regression| regression.smoke)
        .collect()
}

fn regression_layout(regression: &NativeRegression) -> PathBuf {
    repository().join(&regression.layout)
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/layouts/marksman-vs-arclight.yaml")
}

fn rhino_two_arclights_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/layouts/rhino-vs-two-arclights.yaml")
}

#[test]
fn rhino_vs_two_arclights_preserves_the_reviewed_timeline() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("battle.mcfr");
    let result = simulate_layout(
        rhino_two_arclights_fixture(),
        Some(&output),
        Some(1_787_634_176),
    )
    .unwrap();
    assert_eq!(result.winner, Some("blue"));
    assert_eq!(result.steps, 321);
    let reader = McfrReader::open(output).unwrap();
    assert_eq!(reader.tick_count(), 321);
    assert_eq!(
        reader
            .events(151)
            .unwrap()
            .events
            .into_iter()
            .map(|event| (
                event.kind(),
                event.subject.map(|value| value.id),
                event.source.map(|value| value.id),
            ))
            .collect::<Vec<_>>(),
        [
            (EventKind::Damage, None, Some(2)),
            (EventKind::ProjectileRemoved, Some(7), Some(2)),
            (EventKind::Damage, None, Some(3)),
            (EventKind::ProjectileRemoved, Some(6), Some(3)),
        ]
    );
}

#[test]
fn marksman_vs_arclight_runs_to_a_readable_terminal_result() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("battle.mcfr");
    let result = simulate_layout(fixture(), Some(&output), Some(7)).unwrap();
    assert_eq!(result.game_build, "1.11.1.3.2259");
    assert_eq!(result.seed, 7);
    assert_eq!(result.seed_source, "external");
    assert_eq!(result.end_reason, "natural_module_drain");
    assert_eq!(result.output.as_deref(), output.to_str());
    assert!(result.profiling.generation_duration_milliseconds > 0.0);
    assert!(result.profiling.simulation_to_real_time_rate > 0.0);
    assert!(result.winner.is_some());
    assert!(!result.draw);

    let reader = McfrReader::open(&output).unwrap();
    assert_eq!(reader.game_build(), "1.11.1.3.2259");
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

#[test]
fn marksman_vs_arclight_preserves_the_reviewed_behavior() {
    let regression = native_regression("marksman-vs-arclight");
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("battle.mcfr");
    let result = simulate_layout(
        regression_layout(&regression),
        Some(&output),
        Some(regression.seed),
    )
    .unwrap();
    assert_eq!(result.game_build, regression.game_build);
    let reader = McfrReader::open(output).unwrap();
    assert_eq!(reader.tick_count(), regression.tick_count);
    let terminal = reader.state(reader.terminal_tick()).unwrap();
    assert!(
        terminal
            .live_units
            .iter()
            .all(|unit| unit.mech_lock_target.is_none())
    );
    assert!(
        terminal
            .buildings
            .iter()
            .all(|building| building.life.current == 3_400 && building.targetable)
    );
}

#[test]
fn rhino_vs_arclight_preserves_the_reviewed_behavior() {
    let regression = native_regression("rhino-vs-arclight");
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("battle.mcfr");
    let result = simulate_layout(
        regression_layout(&regression),
        Some(&output),
        Some(regression.seed),
    )
    .unwrap();
    assert_eq!(result.game_build, regression.game_build);
    assert_eq!(result.winner, Some("blue"));
    assert_eq!(result.steps, u64::from(regression.tick_count));
    let reader = McfrReader::open(output).unwrap();
    assert_eq!(reader.tick_count(), regression.tick_count);
    let direct_damage = (1..=reader.tick_count())
        .flat_map(|tick| reader.events(tick).unwrap().events)
        .filter_map(|event| match event.payload {
            EventPayload::Damage { amount }
                if event.source
                    == Some(mechcore_mcfr::ObjectRef::new(
                        mechcore_mcfr::ObjectKind::Unit,
                        1,
                    )) =>
            {
                Some((event.target.unwrap().id, amount))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(direct_damage, [(2, 3_560), (2, 1_253)]);

    let terminal = reader.state(reader.terminal_tick()).unwrap();
    assert!(
        terminal
            .live_units
            .iter()
            .all(|unit| unit.mech_lock_target.is_none())
    );
    assert!(
        terminal
            .buildings
            .iter()
            .all(|building| building.team_id != 1)
    );
}

#[test]
fn rhino_retarget_preserves_the_reviewed_behavior() {
    let regression = native_regression("rhino-retarget");
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("battle.mcfr");
    let result = simulate_layout(
        regression_layout(&regression),
        Some(&output),
        Some(regression.seed),
    )
    .unwrap();
    assert_eq!(result.game_build, regression.game_build);
    assert_eq!(result.winner, Some("blue"));
    assert_eq!(result.steps, 336);
    let reader = McfrReader::open(output).unwrap();
    assert_eq!(reader.tick_count(), regression.tick_count);
    let initial = reader.state(1).unwrap();
    assert_eq!(
        initial
            .live_units
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
            (
                1,
                0,
                -1_223_206_685_902,
                -448_824_082_435,
                1_539_518_232_354
            ),
            (2, 1, -1_246_399_509_298, 427_349_245_955, 766_424_119_044),
            (3, 1, -817_332_276_427, 427_778_742_684, 879_856_166_708),
        ]
    );

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

    let direct_damage = (1..=reader.tick_count())
        .flat_map(|tick| {
            reader
                .events(tick)
                .unwrap()
                .events
                .into_iter()
                .filter_map(move |event| match event.payload {
                    EventPayload::Damage { amount }
                        if event.source
                            == Some(mechcore_mcfr::ObjectRef::new(
                                mechcore_mcfr::ObjectKind::Unit,
                                1,
                            )) =>
                    {
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

fn assert_native_regression_hashes(regressions: impl IntoIterator<Item = NativeRegression>) {
    for regression in regressions {
        let name = regression.name.as_str();
        let result =
            simulate_layout(regression_layout(&regression), None, Some(regression.seed)).unwrap();
        assert_eq!(
            result.hashes.physics_result_hash, regression.physics_result_hash,
            "{name}"
        );
    }
}

#[test]
fn native_regression_smoke_hashes_match() {
    assert_native_regression_hashes(smoke_native_regressions());
}

#[test]
#[ignore = "optional full native regression gate"]
fn native_regression_full_hashes_match() {
    assert_native_regression_hashes(current_native_regressions());
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
