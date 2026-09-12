use std::{fs, process::Command};

#[test]
fn verify_reports_shared_compiler_summary() {
    let directory = tempfile::tempdir().unwrap();
    let layout = directory.path().join("layout.yaml");
    fs::write(
        &layout,
        r"
kind: layout
seed: -17
round: 1
sides:
  blue:
    formations: [{type: marksman, index: 0, position: {x: 0, y: -50}}]
    constructions: [{type: defensive_wall, index: 0, position: {x: 140, y: -105}}]
    contraptions: [{type: interceptor, index: 0, position: {x: 35, y: -85}}]
    terrains: [{type: oil, control_points: [{x: -60, y: 40}, {x: 60, y: 40}]}]
  red:
    formations: [{type: arclight, index: 0, position: {x: 0, y: -50}}]
",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .arg("verify")
        .arg(&layout)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["valid"], true);
    assert_eq!(report["seed"], -17);
    assert_eq!(report["round"], 1);
    assert_eq!(report["formation_count"], 2);
    assert_eq!(report["construction_count"], 1);
    assert_eq!(report["contraption_count"], 1);
    assert_eq!(report["terrain_count"], 1);
}

#[test]
fn verify_rejects_the_zero_seed_sentinel() {
    let directory = tempfile::tempdir().unwrap();
    let layout = directory.path().join("layout.yaml");
    fs::write(
        &layout,
        "kind: layout\nseed: 0\nround: 1\nsides:\n  blue:\n    formations: [{type: marksman, index: 0, position: {x: 0, y: -50}}]\n  red:\n    formations: [{type: arclight, index: 0, position: {x: 0, y: -50}}]\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .arg("verify")
        .arg(&layout)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("native system-random request"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn verify_rejects_contraptions_in_formations() {
    let directory = tempfile::tempdir().unwrap();
    let layout = directory.path().join("layout.yaml");
    fs::write(
        &layout,
        r"
kind: layout
round: 1
sides:
  blue:
    formations:
      - {type: marksman, index: 0, position: {x: 0, y: -50}}
      - {type: shield, index: 1, position: {x: 0, y: -100}}
  red:
    formations: [{type: arclight, index: 0, position: {x: 0, y: -50}}]
",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .arg("verify")
        .arg(&layout)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("belongs in contraptions"));
}

#[test]
fn format_emits_canonical_defaults_and_supports_in_place_write() {
    let directory = tempfile::tempdir().unwrap();
    let layout = directory.path().join("layout.yaml");
    fs::write(
        &layout,
        r"
kind: layout
round: 1
sides:
  blue:
    formations: [{type: marksman, index: 0, position: {x: 0, y: -50}, level: 1, rotated: false}]
    terrains: [{type: oil, control_points: [{x: -60, y: 40}, {x: 60, y: 40}]}]
  red:
    formations: [{type: arclight, index: 0, position: {x: 0, y: -50}, travelling: false}]
",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .arg("format")
        .arg(&layout)
        .output()
        .unwrap();
    assert!(output.status.success());
    let canonical = String::from_utf8(output.stdout).unwrap();
    assert!(canonical.starts_with("kind: layout\nround: 1\n"));
    assert!(!canonical.contains("seed:"));
    assert!(!canonical.contains("level:"));
    assert!(!canonical.contains("rotated:"));
    assert!(!canonical.contains("travelling:"));
    assert!(canonical.contains(
        "terrains:\n    - type: oil\n      control_points:\n      - {x: -60, y: 40}\n      - {x: 60, y: 40}"
    ));
    assert!(
        canonical.contains("- type: marksman\n      index: 0\n      position: {x: 0, y: -50}\n"),
        "a placement position stays on one line: {canonical}"
    );
    assert!(!canonical.contains("grid_rows:"));

    let output = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .arg("format")
        .arg(&layout)
        .arg("--write")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(fs::read_to_string(layout).unwrap(), canonical);
}

#[test]
fn diff_compares_normalized_fields() {
    let directory = tempfile::tempdir().unwrap();
    let left = directory.path().join("left.yaml");
    let right = directory.path().join("right.yaml");
    fs::write(
        &left,
        "kind: layout\nround: 1\nsides:\n  blue:\n    formations: [{type: marksman, index: 0, position: {x: 0, y: -50}, level: 1}]\n  red:\n    formations: [{type: arclight, index: 0, position: {x: 0, y: -50}}]\n",
    )
    .unwrap();
    fs::write(
        &right,
        "kind: layout\nround: 1\nsides:\n  blue:\n    formations: [{type: marksman, index: 0, position: {x: 0, y: -50}}]\n  red:\n    formations: [{type: arclight, index: 0, position: {x: 0, y: -50}}]\n",
    )
    .unwrap();

    let equal = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .arg("diff")
        .arg(&left)
        .arg(&right)
        .output()
        .unwrap();
    assert!(equal.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&equal.stdout).unwrap()["equal"],
        true
    );

    fs::write(
        &right,
        "kind: layout\nround: 1\nsides:\n  blue:\n    formations: [{type: marksman, index: 0, position: {x: 20, y: -50}}]\n  red:\n    formations: [{type: arclight, index: 0, position: {x: 0, y: -50}}]\n",
    )
    .unwrap();
    let different = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .arg("diff")
        .arg(&left)
        .arg(&right)
        .output()
        .unwrap();
    assert!(!different.status.success());
    let report: serde_json::Value = serde_json::from_slice(&different.stdout).unwrap();
    assert_eq!(
        report["differences"][0]["path"],
        "/sides/blue/formations/index=0/position/x"
    );
}
