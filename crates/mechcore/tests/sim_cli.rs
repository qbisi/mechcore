use std::{fs, path::PathBuf, process::Command};

use mechcore_mcfr::{McfrReader, McfrWriter};

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
        8
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

#[test]
fn sim_compare_reports_the_first_divergent_tick_without_an_output_recording() {
    let directory = tempfile::tempdir().unwrap();
    let recording_path = directory.path().join("equal.mcfr");
    let divergent_path = directory.path().join("divergent.mcfr");
    let layout = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/layouts/marksman-vs-arclight.yaml");
    let config = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config");
    let generated = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .arg("sim")
        .arg(&layout)
        .arg("--seed")
        .arg("7")
        .arg("--output")
        .arg(&recording_path)
        .arg("--config")
        .arg(&config)
        .output()
        .unwrap();
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );

    let recording = McfrReader::open(&recording_path).unwrap();
    let mut writer = McfrWriter::create(
        &divergent_path,
        recording.game_build(),
        recording.context(),
        recording.layout_yaml(),
    )
    .unwrap();
    writer
        .set_initial_state(recording.state(0).unwrap())
        .unwrap();
    for tick in 1..=recording.tick_count() {
        let mut slice = recording.tick(tick).unwrap();
        if tick == 1 {
            slice.state.live_units[0].life.current -= 1;
        }
        writer.append_tick(slice.state, &slice.events).unwrap();
    }
    let divergent_hashes = writer.finish().unwrap();
    assert_eq!(
        divergent_hashes.scenario_hash,
        recording.hashes().scenario_hash
    );
    assert_ne!(divergent_hashes.result_hash, recording.hashes().result_hash);

    drop(recording);

    let compared = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .arg("sim")
        .arg("compare")
        .arg(&recording_path)
        .arg(&divergent_path)
        .arg("--config")
        .arg(&config)
        .output()
        .unwrap();
    assert!(!compared.status.success());
    assert!(compared.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&compared.stdout).unwrap();
    assert_eq!(report["schema"], "mechcore.sim-compare-batch-result.v1");
    assert_eq!(report["equal"], false);
    assert_eq!(report["comparisons"][0]["equal"], true);
    assert!(report["comparisons"][0]["first_divergence"].is_null());
    assert_eq!(report["comparisons"][1]["equal"], false);
    assert_eq!(report["comparisons"][1]["first_divergence"], 1);
    assert_eq!(
        report["comparisons"][1]["divergent_tick"]["recording"]["tick"],
        1
    );
    assert_eq!(
        report["comparisons"][1]["divergent_tick"]["simulation"]["tick"],
        1
    );
}
