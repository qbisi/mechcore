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
    units: [{name: marksman, index: 0, position: {x: 0, y: -50}}]
    constructions: [{name: defensive_wall, index: 0, position: {x: 140, y: -105}}]
    contraptions: [{name: interceptor, index: 0, position: {x: 35, y: -85}}]
    terrains: [{name: oil, control_points: [{x: -60, y: 40}, {x: 60, y: 40}]}]
  red:
    units: [{name: arclight, index: 0, position: {x: 0, y: -50}}]
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
    assert_eq!(report["kind"], "layout");
    assert_eq!(report["seed"], -17);
    assert_eq!(report["round"], 1);
    assert_eq!(report["unit_count"], 2);
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
        "kind: layout\nseed: 0\nround: 1\nsides:\n  blue:\n    units: [{name: marksman, index: 0, position: {x: 0, y: -50}}]\n  red:\n    units: [{name: arclight, index: 0, position: {x: 0, y: -50}}]\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .arg("verify")
        .arg(&layout)
        .output()
        .unwrap();
    // A refusal is a report like any other, so a batch that meets one keeps
    // going and the exit code is what says something was wrong.
    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["valid"], false);
    assert_eq!(report["kind"], "unreadable");
    assert!(
        report["error"]
            .as_str()
            .unwrap()
            .contains("native system-random request"),
        "{report}"
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
    units:
      - {name: marksman, index: 0, position: {x: 0, y: -50}}
      - {name: shield, index: 1, position: {x: 0, y: -100}}
  red:
    units: [{name: arclight, index: 0, position: {x: 0, y: -50}}]
",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .arg("verify")
        .arg(&layout)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        report["error"]
            .as_str()
            .unwrap()
            .contains("belongs in contraptions"),
        "{report}"
    );
}

/// A batch is a pipe: paths on standard input, one report per line, and one
/// bad input does not stop the others.
#[test]
fn verify_reads_a_batch_of_paths_from_standard_input() {
    use std::io::Write;
    use std::process::Stdio;

    let directory = tempfile::tempdir().unwrap();
    let good = directory.path().join("good.yaml");
    fs::write(
        &good,
        "kind: layout
round: 1
sides:
  blue:
    units: [{name: marksman, index: 0, position: {x: 0, y: -50}}]
  red:
    units: [{name: arclight, index: 0, position: {x: 0, y: -50}}]
",
    )
    .unwrap();
    let missing = directory.path().join("absent.yaml");

    let mut child = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .arg("verify")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        child.stdin.as_mut().unwrap(),
        "{}\n\n{}",
        good.display(),
        missing.display()
    )
    .unwrap();
    let output = child.wait_with_output().unwrap();

    // Two paths and one blank line between them, so three lines in and two
    // reports out.
    let reports: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0]["valid"], true);
    assert_eq!(reports[1]["valid"], false);
    assert!(!output.status.success());
}

/// A directory is refused by naming what to type instead, because expanding one
/// is the shell's job rather than this command's.
#[test]
fn verify_refuses_a_directory_by_saying_what_to_name() {
    let directory = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .arg("verify")
        .arg(directory.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        report["error"].as_str().unwrap().contains("shell glob"),
        "{report}"
    );
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
    units: [{name: marksman, index: 0, position: {x: 0, y: -50}, level: 1, rotated: false}]
    terrains: [{name: oil, control_points: [{x: -60, y: 40}, {x: 60, y: 40}]}]
  red:
    units: [{name: arclight, index: 0, position: {x: 0, y: -50}, travelling: false}]
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
        "terrains:\n    - {name: oil, control_points: [{x: -60, y: 40}, {x: 60, y: 40}]}\n"
    ));
    assert!(
        canonical.contains("    - {name: marksman, index: 0, position: {x: 0, y: -50}}\n"),
        "a unit stays on one line: {canonical}"
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
        "kind: layout\nround: 1\nsides:\n  blue:\n    units: [{name: marksman, index: 0, position: {x: 0, y: -50}, level: 1}]\n  red:\n    units: [{name: arclight, index: 0, position: {x: 0, y: -50}}]\n",
    )
    .unwrap();
    fs::write(
        &right,
        "kind: layout\nround: 1\nsides:\n  blue:\n    units: [{name: marksman, index: 0, position: {x: 0, y: -50}}]\n  red:\n    units: [{name: arclight, index: 0, position: {x: 0, y: -50}}]\n",
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
        "kind: layout\nround: 1\nsides:\n  blue:\n    units: [{name: marksman, index: 0, position: {x: 20, y: -50}}]\n  red:\n    units: [{name: arclight, index: 0, position: {x: 0, y: -50}}]\n",
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
        "/sides/blue/units/index=0/position/x"
    );
}

/// A tracked battle as its segments, and a way to verify a rewrite of them.
/// Only the YAML is named: neither a GRBR path nor an observation is input.
struct BattleFixture {
    original: Vec<serde_yaml::Value>,
    _directory: tempfile::TempDir,
    path: std::path::PathBuf,
}

impl BattleFixture {
    fn new() -> Self {
        use serde::Deserialize;
        let source = include_str!(
            "../../../tests/battle/2259_20260901--201562374_[crower]VS[[TUFF]MARLFAUX].yaml"
        );
        let original = serde_yaml::Deserializer::from_str(source)
            .map(|document| serde_yaml::Value::deserialize(document).unwrap())
            .collect();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("battle.yaml");
        Self {
            original,
            _directory: directory,
            path,
        }
    }

    fn verify(&self, documents: &[serde_yaml::Value]) -> std::process::Output {
        let yaml = documents
            .iter()
            .map(|document| serde_yaml::to_string(document).unwrap())
            .collect::<Vec<_>>()
            .join("---\n");
        fs::write(&self.path, yaml).unwrap();
        Command::new(env!("CARGO_BIN_EXE_mechcore"))
            .arg("verify")
            .arg(&self.path)
            .output()
            .unwrap()
    }
}

#[test]
fn battle_verification_reports_transition_coverage() {
    use serde_yaml::Value;

    let fixture = BattleFixture::new();
    let original = &fixture.original;
    let run = |documents: &[Value]| fixture.verify(documents);
    // The recorded battle is predicted in every leaf outside the fight.
    let recorded = run(original);
    let report: serde_json::Value = serde_json::from_slice(&recorded.stdout).unwrap();
    assert!(recorded.status.success(), "{report}");
    assert_eq!(report["valid"], true);
    let coverage = &report["coverage"];
    assert_eq!(coverage["total"]["unequal"], 0);
    assert_eq!(coverage["total"]["unimplemented"], 0);
    assert!(coverage["total"]["equal"].as_u64().unwrap() > 0);
    assert_eq!(coverage["fields"]["supply"]["equal"], 16);
    assert!(report["reinforcement_offers_checked"].as_u64().unwrap() > 0);

    // A well-formed wrong value in a predicted field is found where it is, and
    // fails the battle.
    let mut documents = original.clone();
    let red = &mut documents[4]["sides"]["red"];
    let level = red["tower_strengthen_levels"][0].as_i64().unwrap();
    red["tower_strengthen_levels"][0] = Value::Number((level + 1).into());
    let supply = red["supply"].as_i64().unwrap();
    red["supply"] = Value::Number((supply + 50).into());
    let broken = run(&documents);
    assert!(!broken.status.success());
    let report: serde_json::Value = serde_json::from_slice(&broken.stdout).unwrap();
    let paths: Vec<_> = report["coverage"]["unequal"]
        .as_array()
        .unwrap()
        .iter()
        .map(|difference| {
            (
                difference["round"].as_i64().unwrap(),
                difference["side"].as_str().unwrap().to_owned(),
                difference["path"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        paths,
        [
            (1, "red".to_owned(), "supply".to_owned()),
            (1, "red".to_owned(), "tower_strengthen_levels".to_owned()),
            (2, "red".to_owned(), "supply".to_owned()),
            (2, "red".to_owned(), "tower_strengthen_levels".to_owned()),
        ]
    );
}

#[test]
fn battle_verification_reads_fields_outside_the_deal() {
    use serde_yaml::Value;

    let fixture = BattleFixture::new();
    let original = &fixture.original;
    for case in [
        "supply",
        "cooldown",
        "equipment",
        "position",
        "unknown_state_field",
        "missing_operand",
        "unknown_action",
    ] {
        let mut documents = original.clone();
        let blue = &mut documents[2]["sides"]["blue"];
        match case {
            "supply" => blue["supply"] = Value::String("broken".into()),
            "cooldown" => {
                blue["battle_skills"] =
                    serde_yaml::from_str("[{index: 0, id: 1000001, cooldown: broken}]").unwrap();
            }
            "equipment" => blue["equipment"] = serde_yaml::from_str("[{id: broken}]").unwrap(),
            "position" => blue["units"][0]["position"]["x"] = Value::String("broken".into()),
            "unknown_state_field" => blue["supply_typo"] = Value::Number(1.into()),
            "missing_operand" => {
                documents[3]["blue"] = serde_yaml::from_str("[{type: upgrade_unit}]").unwrap();
            }
            "unknown_action" => {
                documents[3]["blue"] = serde_yaml::from_str("[{type: unknown_action}]").unwrap();
            }
            _ => unreachable!(),
        }
        let output = fixture.verify(&documents);
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(!output.status.success(), "{case}: {report}");
        assert_eq!(report["valid"], false, "{case}");
        assert!(
            report["error"].as_str().unwrap().contains("round 1"),
            "{case}: {report}"
        );
    }
}
