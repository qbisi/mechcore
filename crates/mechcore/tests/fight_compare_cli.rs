use std::{path::Path, process::Command};

use mechcore_mcfr::{
    DurableContext, Event, EventPayload, McfrWriter, ObjectKind, ObjectRef, QVec3, Rational,
    ShieldSourceKind, TransitionEvents, WorldSnapshot,
};

#[test]
fn compare_reports_equal_physics_result_hashes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("battle.mcfr");
    write_recording(&path, 42, &[1]);

    let output = compare(&path, &path);
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "mechcore.fight-compare-result.v2");
    assert_eq!(report["equal"], true);
    assert!(report["first_divergence"].is_null());
    assert_eq!(report["compared_ticks"], 1);
    assert_eq!(report["fields_equal"], true);
    assert_eq!(report["fields"], serde_json::json!({}));
    assert!(report.get("at").is_none());
    assert_eq!(
        report["left"]["physics_result_hash"],
        report["right"]["physics_result_hash"]
    );
    assert_eq!(report["content_equal"], true);
}

#[test]
fn compare_reports_content_differences_outside_the_physics_projection() {
    let directory = tempfile::tempdir().unwrap();
    let left = directory.path().join("left.mcfr");
    let right = directory.path().join("right.mcfr");
    write_shield_recording(&left, ShieldSourceKind::CommanderSkill);
    write_shield_recording(&right, ShieldSourceKind::SpawnedTemporary);

    let output = compare(&left, &right);
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["equal"], true);
    assert_eq!(report["content_equal"], false);
    assert!(report["first_divergence"].is_null());
    assert_eq!(
        report["left"]["physics_result_hash"],
        report["right"]["physics_result_hash"]
    );
    assert_ne!(
        report["left"]["content_result_hash"],
        report["right"]["content_result_hash"]
    );
    // The content difference is named: the shield's creation event, at the
    // one tick it happens.
    assert_eq!(report["fields"]["events"]["first_divergence"], 1);
    assert_eq!(report["fields"]["events"]["divergent_ticks"], 1);
    assert_eq!(report["at"]["tick"], 1);
    assert_eq!(report["at"]["differences"][0]["field"], "events");
    assert!(
        report["at"]["differences"][0]["left"]
            .as_str()
            .unwrap()
            .contains("source_kind=commander_skill")
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
    assert_eq!(report["fields_equal"], false);
    assert_eq!(report["at"]["tick"], 1);
    assert_eq!(
        report["at"]["events"]["left"],
        serde_json::json!(["t1 damage at unit 1 amount=1"])
    );
    assert_eq!(
        report["at"]["events"]["right"],
        serde_json::json!(["t1 damage at unit 1 amount=2"])
    );
    assert_eq!(report["at"]["references"]["unit 1"]["left"], "absent");
    assert_ne!(
        report["left"]["physics_result_hash"],
        report["right"]["physics_result_hash"]
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
    // Fields are compared over the ticks both hold, and those agree.
    assert_eq!(report["compared_ticks"], 1);
    assert_eq!(report["fields_equal"], true);
}

/// Naming field groups makes them the verdict: two recordings whose physics
/// differs in an event agree on every unit field.
#[test]
fn a_selection_of_fields_is_the_verdict() {
    let directory = tempfile::tempdir().unwrap();
    let left = directory.path().join("left.mcfr");
    let right = directory.path().join("right.mcfr");
    write_recording(&left, 42, &[1, 1]);
    write_recording(&right, 42, &[1, 2]);

    let output = compare_with(&left, &right, &["--fields", "units.motion_state"]);
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["equal"], false);
    assert_eq!(report["fields_equal"], true);

    let output = compare_with(&left, &right, &["--fields", "events"]);
    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["fields"]["events"]["first_divergence"], 2);
    assert_eq!(report["at"]["tick"], 2);
}

/// A tick can be explained whether or not it differs, and one neither side
/// holds is refused.
#[test]
fn a_named_tick_is_explained_and_one_outside_is_refused() {
    let directory = tempfile::tempdir().unwrap();
    let left = directory.path().join("left.mcfr");
    let right = directory.path().join("right.mcfr");
    write_recording(&left, 42, &[1, 1]);
    write_recording(&right, 42, &[1, 2]);

    let output = compare_with(&left, &right, &["--tick", "1"]);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["at"]["tick"], 1);
    assert_eq!(report["at"]["differences"], serde_json::json!([]));

    let output = compare_with(&left, &right, &["--tick", "3"]);
    assert_eq!(output.status.code(), Some(3), "{output:?}");
}

#[test]
fn the_text_rendering_names_the_verdict_and_the_groups() {
    let directory = tempfile::tempdir().unwrap();
    let left = directory.path().join("left.mcfr");
    let right = directory.path().join("right.mcfr");
    write_recording(&left, 42, &[1]);
    write_recording(&right, 42, &[2]);

    let output = compare_with(&left, &right, &["--format", "text"]);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.starts_with("physics different from t1"), "{text}");
    assert!(text.contains("events"), "{text}");
    assert!(text.contains("at t1:"), "{text}");
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
    compare_with(left, right, &[])
}

fn compare_with(left: &Path, right: &Path, options: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .arg("fight")
        .arg("compare")
        .arg(left)
        .arg(right)
        .args(options)
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
        "kind: layout\nseed: {seed}\nround: 1\nblue:\n  units:\n  - name: marksman\n    index: 0\n    position: {{x: 0, y: -50}}\nred:\n  units:\n  - name: arclight\n    index: 0\n    position: {{x: 0, y: -50}}\n"
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

fn write_shield_recording(path: &Path, source_kind: ShieldSourceKind) {
    let context = DurableContext {
        logic_step: Rational {
            numerator: 1,
            denominator: 20,
        },
        time_units_per_second: 2_000,
        combat_round: 1,
        match_seed: 42,
    };
    let layout = "kind: layout\nseed: 42\nround: 1\nblue:\n  units:\n  - {name: marksman, index: 0, position: {x: 0, y: -50}}\nred:\n  units:\n  - {name: arclight, index: 0, position: {x: 0, y: -50}}\n";
    let mut writer = McfrWriter::create(path, "build-test", &context, layout).unwrap();
    writer
        .append_tick(
            WorldSnapshot::default(),
            &TransitionEvents {
                events: vec![Event {
                    subject: Some(ObjectRef::new(ObjectKind::Shield, 1)),
                    source: None,
                    source_team_id: None,
                    target: None,
                    payload: EventPayload::ShieldCreated {
                        team_id: 1,
                        source_kind,
                        position: QVec3 { x: 1, y: 2, z: 3 },
                    },
                }],
            },
        )
        .unwrap();
    writer.finish().unwrap();
}
