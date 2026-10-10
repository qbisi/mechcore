use std::{fs, path::PathBuf, process::Command};

use mechcore_mcfr::{McfrReader, McfrWriter};

#[test]
fn converting_a_layout_to_mcfr_writes_it_and_prints_the_result() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("fight.mcfr");
    let layout =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../layouts/marksman-vs-arclight.yaml");
    let command = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(["convert", "--to", "mcfr"])
        .arg(layout)
        .arg("--seed")
        .arg("7")
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
    // No shield or terrain appears in this fight, so their tables are left out.
    assert_eq!(
        report["profiling"]["member_sizes_bytes"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "buildings.parquet",
            "events.parquet",
            "formations.parquet",
            "layout.yaml",
            "projectiles.parquet",
            "statistics.parquet",
            "ticks.parquet",
            "units.parquet",
        ]
    );
    McfrReader::open(output).unwrap();
}

#[test]
fn converting_without_an_output_answers_the_result_and_writes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let layout = directory.path().join("layout.yaml");
    fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../layouts/marksman-vs-arclight.yaml"),
        &layout,
    )
    .unwrap();
    let command = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(["convert", "--to", "mcfr"])
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
    assert_eq!(report["schema"], "mechcore.simulation-result");
    assert!(report.get("output").is_none());
    assert!(report["hashes"].get("scenario_hash").is_none());
    assert!(report["hashes"]["result_hash"].is_string());
    assert!(report["teams"].is_array());
    assert!(report["profiling"]["generation_duration_milliseconds"].is_number());
    assert!(report["profiling"]["simulation_to_real_time_rate"].is_number());
    let profiling = &report["profiling"];
    let phases = &profiling["phases_milliseconds"];
    let phase_sum: f64 = ["prepare", "step", "snapshot", "record", "finish"]
        .iter()
        .map(|phase| phases[phase].as_f64().unwrap())
        .sum();
    assert!(
        phase_sum
            <= profiling["generation_duration_milliseconds"]
                .as_f64()
                .unwrap()
    );
    assert!(
        profiling["unit_ticks"].as_u64().unwrap() >= profiling["peak_live_units"].as_u64().unwrap()
    );
    assert!(profiling["peak_live_units"].as_u64().unwrap() >= 2);
    assert!(
        profiling["step_microseconds_per_unit_tick"]
            .as_f64()
            .unwrap()
            > 0.0
    );
    assert!(profiling["slowest_step"]["tick"].as_u64().unwrap() >= 1);
    assert!(report["profiling"].get("file_size_bytes").is_none());
    assert!(report["profiling"].get("member_sizes_bytes").is_none());
    assert!(!layout.with_extension("mcfr").exists());
}

#[test]
fn verify_reports_the_first_divergent_tick_of_each_recording() {
    let directory = tempfile::tempdir().unwrap();
    let recording_path = directory.path().join("equal.mcfr");
    let divergent_path = directory.path().join("divergent.mcfr");
    let layout =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../layouts/marksman-vs-arclight.yaml");
    let generated = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(["convert", "--to", "mcfr"])
        .arg(&layout)
        .arg("--seed")
        .arg("7")
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
        recording.producer(),
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
    assert_ne!(divergent_hashes.result_hash, recording.hashes().result_hash);

    drop(recording);

    let compared = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .arg("verify")
        .arg(&recording_path)
        .arg(&divergent_path)
        .output()
        .unwrap();
    assert!(!compared.status.success());
    assert!(compared.stderr.is_empty());
    // One report per recording, in the order they were named.
    let reports: Vec<serde_json::Value> = String::from_utf8(compared.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(reports.len(), 2);
    let [equal, divergent] = &reports[..] else {
        unreachable!()
    };
    assert_eq!(equal["schema"], "mechcore.verify-result");
    assert_eq!(equal["kind"], "mcfr");
    assert_eq!(equal["valid"], true);
    assert!(equal.get("error").is_none());
    assert_eq!(equal["comparison"]["schema"], "mechcore.sim-compare-result");
    assert!(equal["comparison"]["first_divergence"].is_null());
    assert_eq!(divergent["valid"], false);
    assert!(
        divergent["error"].as_str().unwrap().contains("tick 1"),
        "{divergent}"
    );
    assert_eq!(divergent["comparison"]["equal"], false);
    assert_eq!(divergent["comparison"]["first_divergence"], 1);
    assert_eq!(
        divergent["comparison"]["divergent_tick"]["recording"]["tick"],
        1
    );
    assert_eq!(
        divergent["comparison"]["divergent_tick"]["simulation"]["tick"],
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
        .args(["convert", "--to", "mcfr"])
        .arg(&layout)
        .arg(&recording)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );

    let read = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(["show", "--view", "outcome"])
        .arg(&recording)
        .output()
        .unwrap();
    // The answer is yes: the recording answers everything its fight decided.
    assert_eq!(read.status.code(), Some(0));
    let outcome: serde_json::Value = serde_json::from_slice(&read.stdout).unwrap();
    assert_eq!(outcome["schema"], "mechcore.fight-outcome");
    assert!(outcome["ticks"].as_u64().unwrap() > 0);
    assert_eq!(outcome["unresolved"], serde_json::json!([]), "{outcome}");

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
}

/// A recording converts to the fight document it records, and a layout to
/// the one the simulator fights it into: the same document, since the
/// simulator wrote the recording.
#[test]
fn a_recording_and_its_layout_convert_to_one_fight_document() {
    let directory = tempfile::tempdir().unwrap();
    let layout = directory.path().join("deployment.yaml");
    let recording = directory.path().join("fight.mcfr");
    let written = directory.path().join("fight.yaml");
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
    let mechcore = |arguments: &[&std::ffi::OsStr]| {
        Command::new(env!("CARGO_BIN_EXE_mechcore"))
            .args(arguments)
            .output()
            .unwrap()
    };
    let recorded = mechcore(&[
        "convert".as_ref(),
        layout.as_os_str(),
        "--to".as_ref(),
        "mcfr".as_ref(),
        recording.as_os_str(),
    ]);
    assert!(recorded.status.success());

    let from_recording = mechcore(&[
        "convert".as_ref(),
        recording.as_os_str(),
        "--to".as_ref(),
        "fight".as_ref(),
    ]);
    assert!(
        from_recording.status.success(),
        "{}",
        String::from_utf8_lossy(&from_recording.stderr)
    );
    let document = String::from_utf8(from_recording.stdout).unwrap();
    // The simulator wrote the recording, so the document says so.
    assert!(document.contains("\nsource: simulator\n"), "{document}");
    assert!(document.contains("\nticks: "), "{document}");
    assert!(document.contains("\nhash: 23:"));
    // Blue alone stands, so red's core takes the three deployed level 1
    // Marksmen's scores and blue's takes nothing.
    assert!(
        document.contains("\nred:\n  core_damage: 300\n"),
        "{document}"
    );
    assert!(!document.contains("blue:\n  core_damage"), "{document}");
    // The Arclight fell to the Marksmen, and its experience went to them.
    assert!(document.contains("exp: 0/"), "{document}");

    let from_layout = mechcore(&[
        "convert".as_ref(),
        layout.as_os_str(),
        "--to".as_ref(),
        "fight".as_ref(),
        written.as_os_str(),
    ]);
    assert!(from_layout.status.success());
    let report: serde_json::Value = serde_json::from_slice(&from_layout.stdout).unwrap();
    assert_eq!(report["schema"], "mechcore.convert-fight-result");
    assert_eq!(report["source"], "simulator");
    assert_eq!(fs::read_to_string(&written).unwrap(), document);

    // The document is in normal form, equal to itself, and has a shape.
    let formatted = mechcore(&["format".as_ref(), written.as_os_str()]);
    assert_eq!(String::from_utf8(formatted.stdout).unwrap(), document);
    let compared = mechcore(&["diff".as_ref(), written.as_os_str(), written.as_os_str()]);
    assert_eq!(compared.status.code(), Some(0));
    let report: serde_json::Value = serde_json::from_slice(&compared.stdout).unwrap();
    assert_eq!(report["schema"], "mechcore.fight-diff-result");
    let changed = directory.path().join("changed.yaml");
    fs::write(
        &changed,
        document.replace("core_damage: 300", "core_damage: 299"),
    )
    .unwrap();
    let compared = mechcore(&["diff".as_ref(), written.as_os_str(), changed.as_os_str()]);
    assert_eq!(compared.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&compared.stdout).unwrap();
    assert_eq!(report["differences"][0]["path"], "/red/core_damage");
    let schema = mechcore(&["schema".as_ref(), "fight".as_ref()]);
    assert!(schema.status.success());
}

#[test]
fn a_formation_opens_the_simulated_fight_with_the_experience_its_layout_brings() {
    let directory = tempfile::tempdir().unwrap();
    let layout = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../layouts/experience.yaml");
    let recording = directory.path().join("fight.mcfr");
    let recorded = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(["convert".as_ref(), layout.as_os_str()])
        .args(["--to", "mcfr"])
        .arg(&recording)
        .args(["--seed", "4242"])
        .output()
        .unwrap();
    assert!(
        recorded.status.success(),
        "{}",
        String::from_utf8_lossy(&recorded.stderr)
    );
    let read = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .arg("convert")
        .arg(&recording)
        .args(["--to", "fight"])
        .output()
        .unwrap();
    let document = String::from_utf8(read.stdout).unwrap();
    assert!(
        read.status.success(),
        "{document}{}",
        String::from_utf8_lossy(&read.stderr)
    );
    // Each formation opens holding what the layout says, as the game's
    // recording of this layout does: the Marksmen gain the Arclight's
    // experience, and the Arclight ends with what it opened with.
    assert!(
        document.contains("level: 2, exp: 700/900/1465}"),
        "{document}"
    );
    assert!(document.contains("exp: 300/300/750}"), "{document}");
}

/// The fight document the simulator fights a small layout into, written by
/// `convert --to fight`.
fn simulated_fight(directory: &std::path::Path) -> String {
    let layout = directory.join("deployment.yaml");
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
red:
  units:
    - {name: arclight, index: 2, position: {x: 0, y: -60}}
",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args([
            "convert".as_ref(),
            layout.as_os_str(),
            "--to".as_ref(),
            "fight".as_ref(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap()
}

/// Writes `text` as `name` and verifies it: the exit code and the report.
fn verify_text(
    directory: &std::path::Path,
    name: &str,
    text: &str,
) -> (Option<i32>, serde_json::Value) {
    let path = directory.join(name);
    fs::write(&path, text).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .arg("verify")
        .arg(&path)
        .output()
        .unwrap();
    (
        output.status.code(),
        serde_json::from_slice(&output.stdout).unwrap(),
    )
}

/// A fight document is checked by fighting its projection with its seed and
/// comparing the result it states with the one the simulator arrives at.
#[test]
fn verify_fights_a_fight_document_again_and_names_what_differs() {
    let directory = tempfile::tempdir().unwrap();
    let document = simulated_fight(directory.path());
    let verify = |name: &str, text: &str| verify_text(directory.path(), name, text);

    // What the simulator wrote, it arrives at again.
    let (code, report) = verify("fight.yaml", &document);
    assert_eq!(code, Some(0), "{report}");
    assert_eq!(report["schema"], "mechcore.verify-result");
    assert_eq!(report["kind"], "fight");
    assert_eq!(report["valid"], true);
    assert_eq!(
        report["compared"],
        serde_json::json!(["result", "trajectory"])
    );
    assert_eq!(report["differences"], serde_json::json!([]));

    // One unit's experience moved: that path, both values.
    let exp = document.find("exp: 0/").unwrap() + "exp: 0/".len();
    let after = &document[exp..document[exp..].find('/').unwrap() + exp];
    let moved = format!(
        "{}{}{}",
        &document[..exp],
        after.parse::<u32>().unwrap() + 1,
        &document[exp + after.len()..]
    );
    let (code, report) = verify("exp.yaml", &moved);
    assert_eq!(code, Some(1), "{report}");
    assert_eq!(report["valid"], false);
    let differences = report["differences"].as_array().unwrap();
    assert_eq!(differences.len(), 1, "{report}");
    let path = differences[0]["path"].as_str().unwrap();
    assert!(path.starts_with("/blue/units/index="), "{path}");
    assert!(path.ends_with("/exp"), "{path}");
    assert_ne!(differences[0]["expected"], differences[0]["actual"]);
    assert!(report["error"].as_str().unwrap().contains(path), "{report}");

    // The trajectory's hash moved: the hash, and nothing else.
    let hash = document.find("hash: 23:").unwrap() + "hash: 23:".len();
    let flipped = if &document[hash..=hash] == "0" {
        "1"
    } else {
        "0"
    };
    let rehashed = format!("{}{flipped}{}", &document[..hash], &document[hash + 1..]);
    let (code, report) = verify("hash.yaml", &rehashed);
    assert_eq!(code, Some(1), "{report}");
    assert_eq!(report["differences"][0]["path"], "/hash");
    assert_eq!(report["differences"].as_array().unwrap().len(), 1);
}

/// A game's fight is compared on its result and its trajectory alike.
#[test]
fn verify_compares_a_game_fight() {
    let directory = tempfile::tempdir().unwrap();
    let replay = simulated_fight(directory.path()).replace("source: simulator", "source: game");
    let verify = |name: &str, text: &str| verify_text(directory.path(), name, text);

    let (code, report) = verify("game.yaml", &replay);
    assert_eq!(code, Some(0), "{report}");
    assert_eq!(report["source"], "game");
    assert_eq!(
        report["compared"],
        serde_json::json!(["result", "trajectory"])
    );
    let (code, report) = verify(
        "game-damage.yaml",
        &replay.replace("core_damage: ", "core_damage: 1"),
    );
    assert_eq!(code, Some(1), "{report}");
    assert!(
        report["differences"][0]["path"]
            .as_str()
            .unwrap()
            .ends_with("/core_damage"),
        "{report}"
    );
}

/// What was written onto a fight's units, which is not what the fight decided.
///
/// Both halves of a unit's numbers, read out of a recording.
///
/// This layout carries no officer and no buff reaches either unit at the first
/// tick — and every formation still answers, with the numbers its description
/// alone gives. That is the case a control is read
/// for, and it is also what says the derived numbers are recorded rather than
/// inferred. The corrected cases are measured against the game by the scripts
/// under `tests/modifier/`.
#[test]
fn stats_read_a_tick_and_answer_every_formation() {
    let directory = tempfile::tempdir().unwrap();
    let recording = directory.path().join("fight.mcfr");
    let layout =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../layouts/marksman-vs-arclight.yaml");
    let run = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(["convert", "--to", "mcfr"])
        .arg(&layout)
        .arg(&recording)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );

    let read = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(["show", "--view", "stats"])
        .arg(&recording)
        .output()
        .unwrap();
    assert!(read.status.success());
    let written: serde_json::Value = serde_json::from_slice(&read.stdout).unwrap();
    assert_eq!(written["schema"], "mechcore.fight-stats");
    assert_eq!(written["tick"], 1, "the first tick is the default");
    assert!(written["ticks"].as_u64().unwrap() > 1);
    // Every formation answers. A Marksman's description gives 8 m/s, 140 m
    // and 2329 of damage, and nothing here corrects any of them.
    assert_eq!(written["sides"]["blue"][0]["name"], "marksman", "{written}");
    // No buff reaches one member alone, so the formation reads as one.
    let readings = written["sides"]["blue"][0]["readings"].as_array().unwrap();
    assert_eq!(readings.len(), 1, "{written}");
    let blue = &readings[0];
    assert!(!blue["units"].as_array().unwrap().is_empty(), "{written}");
    assert_eq!(blue["move_speed"], 8_i64 << 32);
    assert_eq!(blue["skills"][0]["attack_range"], 140_i64 << 32);
    assert_eq!(blue["skills"][0]["attack_damage"], 2329);
    assert!(
        blue.get("buffs").is_none(),
        "a unit holding no buff leaves them out: {written}"
    );
    assert_eq!(written["sides"]["red"][0]["name"], "arclight", "{written}");

    // A tick the recording does not hold is refused rather than answered from
    // the nearest one it does.
    let beyond = written["ticks"].as_u64().unwrap() + 1_000;
    let refused = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(["show", "--view", "stats"])
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
/// What a construction becomes was measured against the game from
/// `layouts/construction-shape.yaml`.
#[test]
fn buildings_read_the_towers_a_map_gives_each_side() {
    let directory = tempfile::tempdir().unwrap();
    let recording = directory.path().join("fight.mcfr");
    let layout =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../layouts/marksman-vs-arclight.yaml");
    let run = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(["convert", "--to", "mcfr"])
        .arg(&layout)
        .arg(&recording)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );

    let read = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(["show", "--view", "buildings"])
        .arg(&recording)
        .output()
        .unwrap();
    assert!(read.status.success());
    let standing: serde_json::Value = serde_json::from_slice(&read.stdout).unwrap();
    assert_eq!(standing["schema"], "mechcore.fight-buildings");
    assert_eq!(standing["tick"], 1, "the first tick is the default");
    for side in ["blue", "red"] {
        let towers = standing["sides"][side]["towers"].as_array().unwrap();
        assert_eq!(towers.len(), 2, "{side}: {standing}");
        let kinds: Vec<&str> = towers
            .iter()
            .map(|tower| tower["kind"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, ["energy_tower", "research_center"], "{side}");
        // 3400 of life and a 20 m box, which `config/towers.yaml`
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

/// A fight document converts to the recording of the fight it states: its
/// projection, fought with its own seed, lands on the hash it pins.
#[test]
fn a_fight_converts_to_the_recording_it_pins() {
    let fight =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/marksman/vs-arclight.yaml");
    let command = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(["convert", "--to", "mcfr"])
        .arg(&fight)
        .output()
        .unwrap();
    assert!(
        command.status.success(),
        "{}",
        String::from_utf8_lossy(&command.stdout)
    );
    let report: serde_json::Value = serde_json::from_slice(&command.stdout).unwrap();
    let document: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(&fight).unwrap()).unwrap();
    assert_eq!(
        report["hashes"]["result_hash"].as_str(),
        document["hash"]
            .as_str()
            .and_then(|hash| hash.strip_prefix("23:"))
    );
    let seeded = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(["convert", "--to", "mcfr", "--seed", "7"])
        .arg(&fight)
        .output()
        .unwrap();
    assert!(!seeded.status.success(), "a fight states its own seed");
}

/// What the game fights is a recording to write, and what the simulator does
/// not open is refused rather than attempted; none of these reaches a game.
#[test]
fn the_game_backend_fights_into_a_recording_only() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let layout = root.join("layouts/marksman-vs-arclight.yaml");
    let refused = |arguments: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_mechcore"))
            .args(arguments)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{arguments:?}");
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    };
    let layout = layout.to_str().unwrap();
    assert!(
        refused(&[
            "convert",
            layout,
            "--to",
            "grbr",
            "--backend",
            "game",
            "/tmp/x.grbr"
        ])
        .contains("--to mcfr")
    );
    assert!(refused(&["convert", layout, "--to", "mcfr", "--backend", "game"]).contains("name it"));
    assert!(
        refused(&[
            "convert",
            layout,
            "--to",
            "mcfr",
            "--instrument",
            "target_refs"
        ])
        .contains("--backend game")
    );
    assert!(refused(&["game", "record", layout, "/tmp/x.mcfr"]).contains("--backend game"));
    // The simulator never writes a pin, so only the game's fight updates one.
    assert!(refused(&["verify", "--update", layout]).contains("--backend game"));
}
