use std::{fs, path::PathBuf, process::Command};

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
    assert_eq!(report["output"], output.to_str().unwrap());
    assert!(report["winner"].is_string());
    assert!(report["profiling"]["file_size_bytes"].is_number());
    assert_eq!(
        report["profiling"]["member_sizes_bytes"]
            .as_object()
            .unwrap()
            .len(),
        7
    );
    McfrReader::open(output).unwrap();
}

#[test]
fn sim_command_defaults_to_a_structured_result_without_persisting_mcfr() {
    let directory = tempfile::tempdir().unwrap();
    let layout = directory.path().join("battle.yaml");
    fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/layouts/marksman-vs-arclight.yaml"),
        &layout,
    )
    .unwrap();
    let config = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config");
    let command = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .arg("sim")
        .arg(&layout)
        .arg("--seed")
        .arg("7")
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
    assert_eq!(report["schema"], "mechcore.simulation-result.v2");
    assert!(report.get("output").is_none());
    assert!(report["hashes"]["scenario_hash"].is_string());
    assert!(report["hashes"]["result_hash"].is_string());
    assert!(report["teams"].is_array());
    assert!(report["profiling"]["generation_duration_milliseconds"].is_number());
    assert!(report["profiling"]["simulation_to_real_time_rate"].is_number());
    assert!(report["profiling"].get("file_size_bytes").is_none());
    assert!(report["profiling"].get("member_sizes_bytes").is_none());
    assert!(!layout.with_extension("mcfr").exists());
}
