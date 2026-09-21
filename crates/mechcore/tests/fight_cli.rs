use std::{fs, path::PathBuf, process::Command};

use mechcore_mcfr::{McfrReader, McfrWriter};

#[test]
fn sim_command_writes_mcfr_and_prints_the_result() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("battle.mcfr");
    let layout = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/layouts/marksman-vs-arclight.yaml");
    let command = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .arg("fight")
        .arg("run")
        .arg(layout)
        .arg("--seed")
        .arg("7")
        .arg("--output")
        .arg(&output)
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
    let command = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .arg("fight")
        .arg("run")
        .arg(&layout)
        .arg("--seed")
        .arg("7")
        .output()
        .unwrap();
    assert!(
        command.status.success(),
        "{}",
        String::from_utf8_lossy(&command.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&command.stdout).unwrap();
    assert_eq!(report["schema"], "mechcore.simulation-result.v3");
    assert!(report.get("output").is_none());
    assert!(report["hashes"].get("scenario_hash").is_none());
    assert!(report["hashes"]["physics_result_hash"].is_string());
    assert!(report["hashes"]["content_result_hash"].is_string());
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
    let generated = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .arg("fight")
        .arg("run")
        .arg(&layout)
        .arg("--seed")
        .arg("7")
        .arg("--output")
        .arg(&recording_path)
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
    for tick in 1..=recording.tick_count() {
        let mut slice = recording.tick(tick).unwrap();
        if tick == 1 {
            slice.state.live_units[0].life.current -= 1;
        }
        writer.append_tick(slice.state, &slice.events).unwrap();
    }
    let divergent_hashes = writer.finish().unwrap();
    assert_ne!(
        divergent_hashes.physics_result_hash,
        recording.hashes().physics_result_hash
    );

    drop(recording);

    let compared = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .arg("fight")
        .arg("verify")
        .arg(&recording_path)
        .arg(&divergent_path)
        .output()
        .unwrap();
    assert!(!compared.status.success());
    assert!(compared.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&compared.stdout).unwrap();
    assert_eq!(report["schema"], "mechcore.fight-verify-result.v1");
    assert_eq!(report["equal"], false);
    assert_eq!(
        report["comparisons"][0]["schema"],
        "mechcore.sim-compare-result.v2"
    );
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

/// What a fight decided, read back out of the recording of it.
///
/// The recording numbers its formations by where their members stand, not by
/// the order the layout declares them, so this fixture gives three formations
/// of one type gapped document indices: a reader that lost the mapping would
/// answer 0, 1, 2.
#[test]
fn outcome_reads_survivors_under_the_indices_the_document_uses() {
    let directory = tempfile::tempdir().unwrap();
    let layout = directory.path().join("deployment.yaml");
    let recording = directory.path().join("fight.mcfr");
    fs::write(
        &layout,
        r"
kind: layout
round: 1
seed: 4242
blue:
  units:
    - {name: marksman, index: 0, position: {x: -60, y: -100}}
    - {name: marksman, index: 3, position: {x: 0, y: -100}}
    - {name: marksman, index: 7, position: {x: 60, y: -100}}
red:
  units:
    - {name: arclight, index: 2, position: {x: 0, y: -60}}
",
    )
    .unwrap();
    let run = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(["fight", "run"])
        .arg(&layout)
        .arg("--output")
        .arg(&recording)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );

    let read = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(["fight", "outcome"])
        .arg(&recording)
        .output()
        .unwrap();
    // The answer is no, because two of the five fields have no rule: the fight
    // was read and it does not settle a round.
    assert_eq!(read.status.code(), Some(1));
    let outcome: serde_json::Value = serde_json::from_slice(&read.stdout).unwrap();
    assert_eq!(outcome["schema"], "mechcore.fight-outcome.v1");
    assert!(outcome["ticks"].as_u64().unwrap() > 0);

    let blue = &outcome["sides"]["blue"]["survivors"];
    let indices: Vec<i64> = blue
        .as_array()
        .unwrap()
        .iter()
        .map(|survivor| survivor["index"].as_i64().unwrap())
        .collect();
    assert_eq!(indices, vec![0, 3, 7], "{outcome}");
    assert_eq!(blue[0]["name"], "marksman");
    assert_eq!(blue[0]["members"], 1);
    assert_eq!(blue[0]["alive"], 1);
    assert!(blue[0]["life"].as_i64().unwrap() > 0);
    assert!(
        outcome["sides"]["red"]["survivors"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    // A round that carried no contraption, terrain or shield into the fight
    // has none left, and that is the only one of the three this reader closes.
    for side in ["blue", "red"] {
        for field in ["contraptions", "terrains", "airdrop_shields"] {
            assert_eq!(
                outcome["sides"][side][field].as_array().unwrap().len(),
                0,
                "{side} {field}"
            );
        }
    }
    let unresolved = outcome["unresolved"].as_array().unwrap();
    assert_eq!(unresolved.len(), 2, "{outcome}");
    assert!(unresolved[0].as_str().unwrap().starts_with("reactor_core:"));
    assert!(unresolved[1].as_str().unwrap().starts_with("units.exp:"));
}

/// What was written onto a fight's units, which is not what the fight decided.
///
/// Both halves of a unit's numbers, read out of a recording.
///
/// This layout carries no officer, so nothing is written onto either unit and
/// the corrections are neutral — and every formation still answers, with the
/// numbers its description alone gives. That is the case a control is read
/// for, and it is also what says the derived numbers are recorded rather than
/// inferred. The corrected cases are measured against the game by the scripts
/// under `tests/layouts/modifier/`.
#[test]
fn stats_read_a_tick_and_answer_both_halves() {
    let directory = tempfile::tempdir().unwrap();
    let recording = directory.path().join("fight.mcfr");
    let layout = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/layouts/marksman-vs-arclight.yaml");
    let run = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(["fight", "run"])
        .arg(&layout)
        .arg("--output")
        .arg(&recording)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );

    let read = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(["fight", "stats"])
        .arg(&recording)
        .output()
        .unwrap();
    assert!(read.status.success());
    let written: serde_json::Value = serde_json::from_slice(&read.stdout).unwrap();
    assert_eq!(written["schema"], "mechcore.fight-stats.v1");
    assert_eq!(written["tick"], 1, "the first tick is the default");
    assert!(written["ticks"].as_u64().unwrap() > 1);
    // Every formation answers. A Marksman's description gives 8 m/s, 140 m
    // and 2329 of damage, and nothing here corrects any of them.
    let blue = &written["sides"]["blue"][0];
    assert_eq!(blue["name"], "marksman", "{written}");
    assert_eq!(blue["derived"]["move_speed"], 8_i64 << 32);
    assert_eq!(blue["derived"]["attack_range"], 140_i64 << 32);
    assert_eq!(blue["derived"]["attack_damage"], 2329);
    assert!(
        blue.get("unit").is_none() && blue.get("skill").is_none(),
        "a neutral channel is left out: {written}"
    );
    assert_eq!(written["sides"]["red"][0]["name"], "arclight", "{written}");

    // A tick the recording does not hold is refused rather than answered from
    // the nearest one it does.
    let beyond = written["ticks"].as_u64().unwrap() + 1_000;
    let refused = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(["fight", "stats"])
        .arg(&recording)
        .arg("--tick")
        .arg(beyond.to_string())
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stdout).contains("has no tick")
            || String::from_utf8_lossy(&refused.stderr).contains("has no tick"),
        "stdout {} stderr {}",
        String::from_utf8_lossy(&refused.stdout),
        String::from_utf8_lossy(&refused.stderr)
    );
}

/// What is standing in a fight, which is neither what it decided nor a unit's
/// numbers.
///
/// A simulated layout carries no construction yet — `FightConstructionSystem`
/// is unimplemented and the closure refuses one — so what this reads is the
/// other half of the answer: the two towers a map gives each side, named by
/// the build's own `BuildingType`, and an empty construction list beside them.
/// What a construction becomes is measured against the game by
/// `tests/layouts/construction/shape.mcscript`.
#[test]
fn buildings_read_the_towers_a_map_gives_each_side() {
    let directory = tempfile::tempdir().unwrap();
    let recording = directory.path().join("fight.mcfr");
    let layout = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/layouts/marksman-vs-arclight.yaml");
    let run = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(["fight", "run"])
        .arg(&layout)
        .arg("--output")
        .arg(&recording)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );

    let read = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(["fight", "buildings"])
        .arg(&recording)
        .output()
        .unwrap();
    assert!(read.status.success());
    let standing: serde_json::Value = serde_json::from_slice(&read.stdout).unwrap();
    assert_eq!(standing["schema"], "mechcore.fight-buildings.v1");
    assert_eq!(standing["tick"], 1, "the first tick is the default");
    for side in ["blue", "red"] {
        let towers = standing["sides"][side]["towers"].as_array().unwrap();
        assert_eq!(towers.len(), 2, "{side}: {standing}");
        let kinds: Vec<&str> = towers
            .iter()
            .map(|tower| tower["kind"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, ["energy_tower", "research_center"], "{side}");
        // 3400 of life and a 20 m box, which `config/training_ground.yaml`
        // states and the capture confirmed.
        assert_eq!(towers[0]["life"]["maximum"], 3400, "{side}");
        assert_eq!(towers[0]["bounds"]["width"], 20_i64 << 32, "{side}");
        assert_eq!(
            standing["sides"][side]["constructions"]
                .as_array()
                .unwrap()
                .len(),
            0,
            "{side} places none: {standing}"
        );
    }
}
