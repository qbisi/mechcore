use std::{path::Path, process::Command};

use mechcore_mcfr::{
    DurableContext, Event, EventPayload, McfrWriter, ObjectKind, ObjectRef, Rational,
    TransitionEvents, WorldSnapshot,
};

#[test]
fn compare_reports_equal_result_hashes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("battle.mcfr");
    write_recording(&path, 42, &[1]);

    let output = compare(&path, &path);
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "mechcore.mcfr-compare-result.v1");
    assert_eq!(report["equal"], true);
    assert!(report["first_divergence"].is_null());
    assert!(report["divergent_ticks"].is_null());
    assert_eq!(
        report["left"]["result_hash"],
        report["right"]["result_hash"]
    );
}

#[test]
fn compare_reports_the_first_divergent_tick_and_fails() {
    let directory = tempfile::tempdir().unwrap();
    let left = directory.path().join("left.mcfr");
    let right = directory.path().join("right.mcfr");
    write_recording(&left, 42, &[1]);
    write_recording(&right, 42, &[2]);

    let output = compare(&left, &right);
    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["equal"], false);
    assert_eq!(report["first_divergence"], 1);
    assert_eq!(report["divergent_ticks"]["left"]["tick"], 1);
    assert_eq!(report["divergent_ticks"]["right"]["tick"], 1);
    assert_ne!(
        report["divergent_ticks"]["left"]["tick_hash"],
        report["divergent_ticks"]["right"]["tick_hash"]
    );
    assert_ne!(
        report["divergent_ticks"]["left"]["events"],
        report["divergent_ticks"]["right"]["events"]
    );
    assert_ne!(
        report["left"]["result_hash"],
        report["right"]["result_hash"]
    );
}

#[test]
fn compare_reports_the_first_missing_tick() {
    let directory = tempfile::tempdir().unwrap();
    let left = directory.path().join("left.mcfr");
    let right = directory.path().join("right.mcfr");
    write_recording(&left, 42, &[1]);
    write_recording(&right, 42, &[1, 2]);

    let output = compare(&left, &right);
    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["first_divergence"], 2);
    assert_eq!(report["left"]["tick_count"], 1);
    assert_eq!(report["right"]["tick_count"], 2);
    assert!(report["divergent_ticks"]["left"].is_null());
    assert_eq!(report["divergent_ticks"]["right"]["tick"], 2);
}

#[test]
fn compare_ignores_context_when_ticks_are_equal() {
    let directory = tempfile::tempdir().unwrap();
    let left = directory.path().join("left.mcfr");
    let right = directory.path().join("right.mcfr");
    write_recording(&left, 42, &[1]);
    write_recording(&right, 43, &[1]);

    let output = compare(&left, &right);
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["equal"], true);
    assert!(report.get("scenario_hash").is_none());
}

fn compare(left: &Path, right: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .arg("mcfr")
        .arg("compare")
        .arg(left)
        .arg(right)
        .output()
        .unwrap()
}

fn write_recording(path: &Path, seed: i32, damages: &[i32]) {
    let context = DurableContext {
        logic_step: Rational {
            numerator: 1,
            denominator: 20,
        },
        time_units_per_second: 2_000,
        combat_round: 1,
        match_seed: seed,
    };
    let layout = format!(
        "seed: {seed}\nround: 1\nsides:\n  blue:\n    formations:\n    - type: marksman\n      x: 0\n      y: -50\n  red:\n    formations:\n    - type: arclight\n      x: 0\n      y: -50\n"
    );
    let mut writer = McfrWriter::create(path, "test-build", &context, &layout).unwrap();
    for &damage in damages {
        writer
            .append_tick(
                WorldSnapshot::default(),
                &TransitionEvents {
                    events: vec![Event {
                        subject: None,
                        source: None,
                        source_team_id: None,
                        target: Some(ObjectRef::new(ObjectKind::Unit, 1)),
                        payload: EventPayload::Damage { amount: damage },
                    }],
                },
            )
            .unwrap();
    }
    writer.finish().unwrap();
}
