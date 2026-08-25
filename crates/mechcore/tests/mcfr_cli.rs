use std::{path::Path, process::Command};

use mechcore_mcfr::{
    DurableContext, Event, EventPayload, IdentityContract, MCFR_SCHEMA_VERSION, McfrWriter,
    NumericConvention, Rational, TransitionEvents, WorldSnapshot,
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
    assert_eq!(report["left"]["tick_count"], 2);
    assert_eq!(report["right"]["tick_count"], 3);
    assert!(report["divergent_ticks"]["left"].is_null());
    assert_eq!(report["divergent_ticks"]["right"]["tick"], 2);
}

#[test]
fn compare_rejects_different_scenarios() {
    let directory = tempfile::tempdir().unwrap();
    let left = directory.path().join("left.mcfr");
    let right = directory.path().join("right.mcfr");
    write_recording(&left, 42, &[1]);
    write_recording(&right, 43, &[1]);

    let output = compare(&left, &right);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("cannot compare first divergence for different durable contexts")
    );
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

fn write_recording(path: &Path, seed: i32, damages: &[i64]) {
    let context = DurableContext {
        schema_version: MCFR_SCHEMA_VERSION,
        game_build: "test-build".into(),
        logic_step: Rational {
            numerator: 1,
            denominator: 20,
        },
        numeric_convention: NumericConvention {
            distance_units_per_meter: 1_000,
            rotation_units_per_degree: 1_000,
            time_units_per_second: 2_000,
        },
        combat_round: 1,
        match_seed: seed,
        identity_contract: IdentityContract::TeamZxSequentialV1,
    };
    let mut writer = McfrWriter::create(path, &context).unwrap();
    writer
        .append_tick(
            WorldSnapshot::default(),
            &TransitionEvents { events: Vec::new() },
        )
        .unwrap();
    for &damage in damages {
        writer
            .append_tick(
                WorldSnapshot::default(),
                &TransitionEvents {
                    events: vec![Event {
                        subject: None,
                        source: None,
                        target: None,
                        payload: EventPayload::Damage { amount: damage },
                    }],
                },
            )
            .unwrap();
    }
    writer.finish().unwrap();
}
