//! The conversion matrix: projecting input, encoding it, and fighting it are
//! separate operations, with one layout shared by their execution paths.

use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn run(input: &Path, to: &str, output: Option<&Path>, options: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mechcore"));
    command.arg("convert").arg(input).args(["--to", to]);
    if let Some(output) = output {
        command.arg(output);
    }
    command.args(options).output().unwrap()
}

fn converted(input: &Path, to: &str, output: Option<&Path>, options: &[&str]) -> Value {
    let answer = run(input, to, output, options);
    assert!(
        answer.status.success(),
        "{}",
        String::from_utf8_lossy(&answer.stderr)
    );
    serde_json::from_slice(&answer.stdout).unwrap()
}

fn layout(at: &Path) {
    let input =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/marksman-vs-arclight.yaml");
    let mut layout = mechcore_document::parse_yaml(&fs::read(input).unwrap()).unwrap();
    layout.seed = Some(7);
    layout.map_id = Some(1021);
    layout.round = 3;
    fs::write(at, mechcore_document::canonical_yaml(layout).unwrap()).unwrap();
}

#[test]
fn synthetic_replay_and_recording_share_the_same_layout_and_simulation() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.yaml");
    let grbr = dir.path().join("input.grbr");
    let mcfr = dir.path().join("input.mcfr");
    let fight = dir.path().join("fight.yaml");
    let projected = dir.path().join("projected.yaml");
    let encoded = dir.path().join("encoded.grbr");
    layout(&input);
    converted(&input, "grbr", Some(&grbr), &[]);
    let report = converted(&input, "mcfr", Some(&mcfr), &[]);
    converted(&mcfr, "fight", Some(&fight), &[]);
    for source in [&grbr, &fight, &mcfr] {
        converted(source, "layout", Some(&projected), &["--force"]);
        assert_eq!(fs::read(&input).unwrap(), fs::read(&projected).unwrap());
    }
    for source in [&fight, &mcfr] {
        converted(source, "grbr", Some(&encoded), &["--force"]);
        assert_eq!(fs::read(&grbr).unwrap(), fs::read(&encoded).unwrap());
    }
    for source in [&grbr, &fight] {
        let result = converted(source, "mcfr", None, &[]);
        assert_eq!(
            result["hashes"]["result_hash"],
            report["hashes"]["result_hash"]
        );
    }
    let from_grbr = dir.path().join("grbr-fight.yaml");
    converted(&grbr, "fight", Some(&from_grbr), &["--round", "3"]);
    assert_eq!(fs::read(&fight).unwrap(), fs::read(&from_grbr).unwrap());
    let answer = run(&mcfr, "layout", None, &[]);
    assert!(answer.status.success());
    assert_eq!(answer.stdout, fs::read(&input).unwrap());
}

#[test]
fn conversion_options_are_refused_before_acquiring_a_game() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.yaml");
    let grbr = dir.path().join("input.grbr");
    let mcfr = dir.path().join("input.mcfr");
    let fight = dir.path().join("fight.yaml");
    let output = dir.path().join("output");
    layout(&input);
    converted(&input, "grbr", Some(&grbr), &[]);
    converted(&input, "mcfr", Some(&mcfr), &[]);
    converted(&mcfr, "fight", Some(&fight), &[]);
    for (source, to) in [
        (&input, "grbr"),
        (&grbr, "layout"),
        (&fight, "layout"),
        (&mcfr, "fight"),
        (&mcfr, "layout"),
        (&mcfr, "grbr"),
    ] {
        for backend in ["simulator", "game"] {
            let answer = run(source, to, Some(&output), &["--backend", backend]);
            assert_eq!(answer.status.code(), Some(2));
            assert!(
                String::from_utf8_lossy(&answer.stderr)
                    .contains("--backend belongs to a computation")
            );
            assert!(!output.exists());
        }
    }
    for (source, to, flag, value) in [
        (&fight, "mcfr", "--round", "3"),
        (&mcfr, "layout", "--round", "3"),
        (&input, "grbr", "--round", "3"),
        (&grbr, "match", "--round", "3"),
        (&grbr, "mcfr", "--seed", "7"),
        (&fight, "mcfr", "--seed", "7"),
        (&mcfr, "grbr", "--seed", "7"),
    ] {
        let answer = run(source, to, Some(&output), &[flag, value]);
        assert_eq!(
            answer.status.code(),
            Some(2),
            "{}",
            String::from_utf8_lossy(&answer.stderr)
        );
        assert!(!output.exists());
    }
    for source in [&fight, &mcfr] {
        assert_eq!(
            run(source, "match", Some(&output), &[]).status.code(),
            Some(3)
        );
    }
}

#[test]
fn extracting_mcfr_layout_does_not_depend_on_mapping_the_fight_outcome() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.yaml");
    let mcfr = dir.path().join("input.mcfr");
    let unmapped = dir.path().join("unmapped.mcfr");
    layout(&input);
    converted(&input, "mcfr", Some(&mcfr), &[]);
    let recording = mechcore_mcfr::McfrReader::open(&mcfr).unwrap();
    let mut writer = mechcore_mcfr::McfrWriter::create(
        &unmapped,
        recording.producer(),
        recording.game_build(),
        recording.context(),
        recording.layout_yaml(),
    )
    .unwrap();
    for tick in 1..=recording.tick_count() {
        let mut slice = recording.tick(tick).unwrap();
        for unit in &mut slice.state.live_units {
            unit.unit_type_id = 999_999;
        }
        writer.append_tick(slice.state, &slice.events).unwrap();
    }
    writer.finish(recording.settlement()).unwrap();
    assert_eq!(run(&unmapped, "fight", None, &[]).status.code(), Some(3));
    let answer = run(&unmapped, "layout", None, &[]);
    assert!(
        answer.status.success(),
        "{}",
        String::from_utf8_lossy(&answer.stderr)
    );
    assert_eq!(answer.stdout, fs::read(input).unwrap());
}
