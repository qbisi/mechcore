use std::{fs, path::PathBuf, process::Command};

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
blue:
  units: [{name: marksman, index: 0, position: {x: 0, y: -50}}]
  constructions: [{name: defensive_wall, index: 0, position: {x: 140, y: -105}}]
  contraptions: [{name: interceptor, index: 0, position: {x: 35, y: -85}}]
  battle_skills:
  - {name: sticky_oil_bomb, standing: {control_points: [{x: -60, y: 40}, {x: 60, y: 40}]}}
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
    assert_eq!(report["standing_shield_count"], 0);
    assert_eq!(report["standing_oil_count"], 1);
}

#[test]
fn verify_rejects_the_zero_seed_sentinel() {
    let directory = tempfile::tempdir().unwrap();
    let layout = directory.path().join("layout.yaml");
    fs::write(
        &layout,
        "kind: layout\nseed: 0\nround: 1\nblue:\n  units: [{name: marksman, index: 0, position: {x: 0, y: -50}}]\nred:\n  units: [{name: arclight, index: 0, position: {x: 0, y: -50}}]\n",
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
blue:
  units: [{name: marksman, index: 0, position: {x: 0, y: -50}, level: 1, rotated: false}]
  battle_skills:
  - {name: sticky_oil_bomb, standing: {control_points: [{x: -60, y: 40}, {x: 60, y: 40}]}}
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
        "battle_skills:\n  - {name: sticky_oil_bomb, standing: {control_points: [{x: -60, y: 40}, {x: 60, y: 40}]}}\n"
    ));
    assert!(
        canonical.contains("  - {name: marksman, index: 0, position: {x: 0, y: -50}}\n"),
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
        "kind: layout\nround: 1\nblue:\n  units: [{name: marksman, index: 0, position: {x: 0, y: -50}, level: 1}]\nred:\n  units: [{name: arclight, index: 0, position: {x: 0, y: -50}}]\n",
    )
    .unwrap();
    fs::write(
        &right,
        "kind: layout\nround: 1\nblue:\n  units: [{name: marksman, index: 0, position: {x: 0, y: -50}}]\nred:\n  units: [{name: arclight, index: 0, position: {x: 0, y: -50}}]\n",
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
        "kind: layout\nround: 1\nblue:\n  units: [{name: marksman, index: 0, position: {x: 20, y: -50}}]\nred:\n  units: [{name: arclight, index: 0, position: {x: 0, y: -50}}]\n",
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
        "/blue/units/index=0/position/x"
    );
}

fn mechcore(arguments: &[&std::ffi::OsStr]) -> (Option<i32>, serde_json::Value, serde_json::Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(arguments)
        .output()
        .unwrap();
    let parse = |bytes: &[u8]| serde_json::from_slice(bytes).unwrap_or(serde_json::Value::Null);
    (
        output.status.code(),
        parse(&output.stdout),
        parse(&output.stderr),
    )
}

/// A file's kind is what it holds, not what it is called: a replay written
/// under a `.yaml` name is still a replay, and a verb or a conversion its kind
/// does not take is refused by naming the kind.
#[test]
fn the_kind_is_read_from_the_content_and_an_unsupported_pair_names_it() {
    let directory = tempfile::tempdir().unwrap();
    let layout =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../layouts/marksman-vs-arclight.yaml");
    let replay = directory.path().join("replay.yaml");
    let os = |text: &'static str| std::ffi::OsStr::new(text);

    // A rewrite to a binary kind writes a file, so it needs one.
    let (code, _, error) = mechcore(&[os("convert"), layout.as_os_str(), os("--to"), os("grbr")]);
    assert_eq!(code, Some(2), "{error}");

    let (code, written, error) = mechcore(&[
        os("convert"),
        layout.as_os_str(),
        os("--to"),
        os("grbr"),
        replay.as_os_str(),
        os("--seed"),
        os("7"),
    ]);
    assert_eq!(code, Some(0), "{error}");
    assert_eq!(
        written["schema"],
        "mechcore.replay-convert-layout-result.v1"
    );
    assert_eq!(written["seed"], 7);

    // An existing destination is replaced only when asked to.
    let again = [
        os("convert"),
        layout.as_os_str(),
        os("--to"),
        os("grbr"),
        replay.as_os_str(),
        os("--seed"),
        os("7"),
    ];
    let (code, _, error) = mechcore(&again);
    assert_eq!(code, Some(3), "{error}");
    assert!(
        error["reason"].as_str().unwrap().contains("--force"),
        "{error}"
    );
    let (code, _, error) = mechcore(&[&again[..], &[os("--force")]].concat());
    assert_eq!(code, Some(0), "{error}");

    let (code, _, error) = mechcore(&[os("convert"), replay.as_os_str(), os("--to"), os("layout")]);
    assert_eq!(code, Some(3), "{error}");
    let reason = error["reason"].as_str().unwrap();
    assert!(
        reason.contains("a grbr file does not convert to layout; it converts to match"),
        "{error}"
    );

    let (code, _, error) = mechcore(&[os("diff"), layout.as_os_str(), replay.as_os_str()]);
    assert_eq!(code, Some(3), "{error}");
    assert!(
        error["reason"].as_str().unwrap().contains("grbr"),
        "{error}"
    );

    let (code, _, error) = mechcore(&[os("show"), layout.as_os_str(), os("--view"), os("outcome")]);
    assert_eq!(code, Some(3), "{error}");
    assert!(
        error["reason"]
            .as_str()
            .unwrap()
            .contains("show does not take a layout file"),
        "{error}"
    );
    assert_eq!(error["operation"], "show");

    let state = directory.path().join("state.yaml");
    fs::write(&state, "kind: state\nround: 1\n").unwrap();
    let (code, report, _) = mechcore(&[os("verify"), state.as_os_str()]);
    assert_eq!(code, Some(1));
    assert!(
        report["error"]
            .as_str()
            .unwrap()
            .contains("verify does not take a state file"),
        "{report}"
    );
}
