use std::{fs, path::PathBuf};

use mechcore_mcfr::{EventKind, McfrReader, Recording};
use mechcore_simulation::{Record, simulate_layout};

fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture() -> PathBuf {
    repository().join("layouts/marksman-vs-arclight.yaml")
}

#[test]
fn marksman_vs_arclight_runs_to_a_readable_terminal_result() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("fight.mcfr");
    let result = simulate_layout(fixture(), Record::File(&output), Some(7)).unwrap();
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

/// A fight kept in memory reads as the recording written of the same fight:
/// the reader that takes either cannot tell them apart.
#[test]
fn a_fight_kept_in_memory_reads_as_its_recording() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("fight.mcfr");
    let written = simulate_layout(fixture(), Record::File(&output), Some(7)).unwrap();
    let kept = simulate_layout(fixture(), Record::Memory, Some(7)).unwrap();
    assert!(written.recording.is_none());
    assert_eq!(kept.output, None);
    assert_eq!(kept.hashes, written.hashes);
    let kept = kept.recording.unwrap();
    let reader = McfrReader::open(&output).unwrap();
    let written: &dyn Recording = &reader;
    assert_eq!(kept.producer(), written.producer());
    assert_eq!(kept.layout_yaml(), written.layout_yaml());
    assert_eq!(kept.hashes(), written.hashes());
    assert_eq!(kept.terminal_tick(), written.terminal_tick());
    for tick in 1..=written.terminal_tick() {
        assert_eq!(kept.state(tick).unwrap(), written.state(tick).unwrap());
        assert_eq!(kept.events(tick).unwrap(), written.events(tick).unwrap());
    }
    assert!(kept.state(0).is_err());
    assert!(kept.state(written.terminal_tick() + 1).is_err());
}

#[test]
fn the_same_layout_and_seed_have_identical_semantic_hashes() {
    let first = simulate_layout(fixture(), Record::Hash, Some(-19)).unwrap();
    let second = simulate_layout(fixture(), Record::Hash, Some(-19)).unwrap();
    assert_eq!(first.hashes, second.hashes);
    assert_eq!(first.winner, second.winner);
    assert_eq!(first.steps, second.steps);
}

#[test]
fn generated_seed_is_reported_and_replayable() {
    let generated = simulate_layout(fixture(), Record::Hash, None).unwrap();
    assert_eq!(generated.seed_source, "generated");
    assert_eq!(generated.output, None);
    let replayed = simulate_layout(fixture(), Record::Hash, Some(generated.seed)).unwrap();
    assert_eq!(generated.hashes, replayed.hashes);
}

#[test]
fn layout_seed_is_used_and_external_seed_overrides_it() {
    let directory = tempfile::tempdir().unwrap();
    let layout = directory.path().join("seeded.yaml");
    let source = fs::read_to_string(fixture()).unwrap();
    fs::write(&layout, format!("seed: -17\n{source}")).unwrap();

    let from_layout = simulate_layout(&layout, Record::Hash, None).unwrap();
    assert_eq!(from_layout.seed, -17);
    assert_eq!(from_layout.seed_source, "layout");

    let overridden = simulate_layout(&layout, Record::Hash, Some(23)).unwrap();
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
        simulate_layout(&layout, Record::Hash, None)
            .unwrap_err()
            .to_string()
            .contains("native system-random request")
    );

    assert!(
        simulate_layout(fixture(), Record::Hash, Some(0))
            .unwrap_err()
            .to_string()
            .contains("system-random request")
    );
}
