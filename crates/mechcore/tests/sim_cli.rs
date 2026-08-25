use std::{path::PathBuf, process::Command};

use mechcore_mcfr::McfrReader;

#[test]
fn sim_command_writes_mcfr_and_prints_the_result() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("battle.mcfr");
    let layout = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/layouts/marksman-vs-arclight.yaml");
    let config = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config");
    let command = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .arg("sim")
        .arg(layout)
        .arg("--seed")
        .arg("7")
        .arg("--output")
        .arg(&output)
        .arg("--config")
        .arg(config)
        .output()
        .unwrap();
    assert!(
        command.status.success(),
        "{}",
        String::from_utf8_lossy(&command.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&command.stdout).unwrap();
    assert_eq!(report["seed"], 7);
    assert!(report["winner"].is_string());
    McfrReader::open(output).unwrap();
}
