use std::{fs, process::Command};

#[test]
fn layout_verify_reports_shared_compiler_summary() {
    let directory = tempfile::tempdir().unwrap();
    let layout = directory.path().join("layout.yaml");
    fs::write(
        &layout,
        r#"
seed: -17
round: 1
sides:
  blue:
    formations: [{type: marksman, x: 0, y: -50}]
    contraptions: [{type: interceptor, x: 35, y: -85}]
  red:
    formations: [{type: arclight, x: 0, y: -50}]
"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .arg("layout")
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
    assert_eq!(report["contraption_count"], 1);
}

#[test]
fn layout_verify_rejects_contraptions_in_formations() {
    let directory = tempfile::tempdir().unwrap();
    let layout = directory.path().join("layout.yaml");
    fs::write(
        &layout,
        r#"
round: 1
sides:
  blue:
    formations:
      - {type: marksman, x: 0, y: -50}
      - {type: shield, x: 0, y: -100}
  red:
    formations: [{type: arclight, x: 0, y: -50}]
"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .arg("layout")
        .arg("verify")
        .arg(&layout)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("belongs in contraptions"));
}
