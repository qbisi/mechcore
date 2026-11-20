//! End-to-end tests for `mechcore solve`.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use mechcore_document::{economy::Economy, r#match::Action, opening, project, transition};
use serde_json::Value;

struct Answer {
    code: i32,
    out: Value,
    error: Value,
    stdout: String,
}

fn run(arguments: &[&str]) -> Answer {
    let output = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(arguments)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let parse = |bytes: &[u8]| serde_json::from_slice(bytes).unwrap_or(Value::Null);
    Answer {
        code: output.status.code().unwrap_or(-1),
        out: parse(&output.stdout),
        error: parse(&output.stderr),
        stdout,
    }
}

impl Answer {
    fn ok(self) -> Value {
        assert_eq!(self.code, 0, "{} {}", self.error, self.out);
        self.out
    }

    fn refused(self) -> String {
        assert_eq!(self.code, 3, "{} {}", self.out, self.error);
        assert_eq!(self.error["kind"], "refused");
        self.error["reason"].as_str().unwrap_or_default().to_owned()
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf()
}

fn default_profile() -> String {
    repo_root()
        .join("profiles/default.yaml")
        .display()
        .to_string()
}

fn opening_decision(view: &Value, side: &str) -> String {
    let offer = &view["header"][side]["offers"][0];
    format!(
        "{{type: choose_advance_team, index: 0, name: {}, specialist: {}}}",
        offer["team"].as_str().unwrap(),
        offer["specialist"].as_str().unwrap()
    )
}

/// Plays through round 0 (openings) and commits round 1 so the match holds one
/// completed deployment round (round 1) plus round 2's opening state.
fn played_round_one(directory: &Path) -> String {
    let path = directory.join("match.yaml").display().to_string();
    let blue = run(&["match", "new", &path, "--seed", "12345", "--map", "1011"]).ok();
    let red = run(&["match", "new", &path]).ok();

    let blue_open = opening_decision(&blue, "blue");
    let red_open = opening_decision(&red, "red");
    run(&["match", "act", &path, "--side", "blue", &blue_open]).ok();
    run(&["match", "act", &path, "--side", "red", &red_open]).ok();
    run(&["match", "commit", &path, "--side", "blue"]).ok();
    run(&["match", "commit", &path, "--side", "red"]).ok();

    // Round 1: buy one already-unlocked unit for each side, then commit both sides.
    for side in ["blue", "red"] {
        let view = run(&["match", "show", &path, "--side", side]).ok();
        let unit = view["state"][side]["unlocked_units"][0].as_str().unwrap();
        let buy = format!(r#"{{"type":"buy_unit","name":"{unit}"}}"#);
        run(&["match", "act", &path, "--side", side, &buy]).ok();
        run(&["match", "commit", &path, "--side", side]).ok();
    }
    path
}

#[test]
fn solve_default_profile_returns_top_k_verified_actions_and_trace() {
    let dir = tempfile::tempdir().unwrap();
    let match_path = played_round_one(dir.path());
    let profile = default_profile();

    // Default --round selects round 1 (last round with actions), default --side is blue.
    let result = run(&["solve", &match_path, "--profile", &profile]).ok();
    assert_eq!(result["schema"], "mechcore.solve");
    assert_eq!(result["round"], 1);
    assert_eq!(result["side"], "blue");
    assert_eq!(result["profile"], "default");

    let baseline_score = result["baseline"]["core_delta"].as_i64().unwrap();
    let solutions = result["solutions"].as_array().unwrap();
    assert_eq!(solutions.len(), 3, "default.yaml asks for top_k=3");

    // Scores must be sorted in descending order and beat or tie the stay baseline.
    let scores: Vec<i64> = solutions
        .iter()
        .map(|sol| sol["score"].as_i64().unwrap())
        .collect();
    assert!(
        scores.windows(2).all(|pair| pair[0] >= pair[1]),
        "{scores:?}"
    );
    assert!(
        scores[0] >= baseline_score,
        "best score {} must be >= baseline {baseline_score}",
        scores[0]
    );

    // Verify stages trace and causal lineage.
    let stages = result["trace"]["stages"].as_array().unwrap();
    assert_eq!(stages.len(), 4);
    assert_eq!(stages[0]["operator"], "warm_start.own_actions");
    assert_eq!(stages[1]["operator"], "placement.predict_unpinned");
    assert_eq!(stages[2]["operator"], "flow.dedup");
    assert_eq!(stages[3]["operator"], "eval.top_k");
    assert!(result["trace"]["simulations"].as_u64().unwrap() >= 3);

    // Independently replay every returned solution's actions and verify legality.
    let bytes = fs::read(&match_path).unwrap();
    let stated = opening::stated(&bytes).unwrap().unwrap();
    let turn = stated.turns.iter().find(|t| t.round == 1).unwrap();
    let economy = Economy::embedded().unwrap();
    let red_deployed = transition::deployed(
        economy,
        &turn.state.red,
        &turn.actions.red,
        true,
        turn.state.reinforce_offers.as_ref(),
    )
    .unwrap();

    for (idx, solution) in solutions.iter().enumerate() {
        assert_eq!(solution["rank"], idx + 1);
        assert_eq!(solution["normalized"], true);
        let lineage = solution["lineage"].as_array().unwrap();
        assert_eq!(lineage[0]["operator"], "init");
        assert!(lineage.len() >= 3);

        let actions: Vec<Action> = serde_json::from_value(solution["actions"].clone()).unwrap();
        let blue_deployed = transition::deployed(
            economy,
            &turn.state.blue,
            &actions,
            false,
            turn.state.reinforce_offers.as_ref(),
        )
        .unwrap();
        let mut combined = turn.state.clone();
        combined.blue = blue_deployed;
        combined.red = red_deployed.clone();
        let layout =
            project::project(&turn.state, &combined, 1, stated.map_id, stated.seed).unwrap();
        let yaml = mechcore_document::canonical_yaml(layout).unwrap();
        let sim = mechcore_simulation::simulate_document(
            yaml.as_bytes(),
            mechcore_simulation::Record::Memory,
            None,
        )
        .unwrap();
        assert!(sim.recording.is_some());
    }
}

#[test]
fn solve_supports_red_side_explicit_round_stay_and_right_shift_stubs() {
    let dir = tempfile::tempdir().unwrap();
    let match_path = played_round_one(dir.path());

    let stay_profile = dir.path().join("stay.yaml");
    fs::write(
        &stay_profile,
        "\
schema: mechcore.profile/v1
name: stay-only
opponent:
  use: opponent.replay_truth
pipeline:
  - use: warm_start.own_actions
    with: { unpin_all_movable: true }
  - use: placement.predict_unpinned
    with: { predictor: stay }
  - use: eval.top_k
    with: { top_k: 2, objective: core_delta }
",
    )
    .unwrap();

    let stay = run(&[
        "solve",
        &match_path,
        "--round",
        "1",
        "--side",
        "red",
        "--profile",
        stay_profile.to_str().unwrap(),
    ])
    .ok();
    assert_eq!(stay["side"], "red");
    assert_eq!(stay["round"], 1);
    assert_eq!(stay["solutions"].as_array().unwrap().len(), 1);
    assert_eq!(stay["solutions"][0]["delta_vs_baseline"], 0);

    // Single right-shift stub (with YAML output format).
    let right_profile = dir.path().join("right.yaml");
    fs::write(
        &right_profile,
        "\
schema: mechcore.profile/v1
name: shift-right-only
opponent:
  use: opponent.replay_truth
pipeline:
  - use: warm_start.own_actions
  - use: placement.predict_unpinned
    with:
      predictor: shift_stub
      offsets: [10]
  - use: eval.top_k
    with:
      top_k: 1
",
    )
    .unwrap();
    let right_yaml = run(&[
        "solve",
        &match_path,
        "--round",
        "1",
        "--side",
        "blue",
        "--profile",
        right_profile.to_str().unwrap(),
        "--format",
        "yaml",
    ]);
    assert_eq!(right_yaml.code, 0, "{}", right_yaml.error);
    let parsed: Value = serde_yaml::from_str(&right_yaml.stdout).unwrap();
    assert_eq!(parsed["profile"], "shift-right-only");
    assert_eq!(parsed["solutions"].as_array().unwrap().len(), 1);
}

#[test]
fn solve_refuses_bad_profile_or_non_match_input() {
    let dir = tempfile::tempdir().unwrap();
    let match_path = played_round_one(dir.path());
    let profile = default_profile();

    // Missing --profile -> usage error (code 2).
    assert_eq!(run(&["solve", &match_path]).code, 2);

    // Non-match document -> refused (code 3).
    let layout = dir.path().join("layout.yaml");
    fs::write(
        &layout,
        "kind: layout\nround: 1\nblue: {units: []}\nred: {units: []}\n",
    )
    .unwrap();
    let reason = run(&["solve", layout.to_str().unwrap(), "--profile", &profile]).refused();
    assert!(reason.contains("layout"), "{reason}");

    // Non-grid offset -> refused.
    let bad_profile = dir.path().join("bad.yaml");
    fs::write(
        &bad_profile,
        "\
schema: mechcore.profile/v1
name: bad
opponent:
  use: opponent.replay_truth
pipeline:
  - use: placement.predict_unpinned
    with: { predictor: shift_stub, offsets: [5] }
",
    )
    .unwrap();
    let reason = run(&[
        "solve",
        &match_path,
        "--profile",
        bad_profile.to_str().unwrap(),
    ])
    .refused();
    assert!(reason.contains("10m"), "{reason}");
}
