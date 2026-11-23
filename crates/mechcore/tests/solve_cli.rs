//! End-to-end tests for `mechcore solve` and its composable plugin runtime.

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

fn r1_blind_profile() -> String {
    repo_root()
        .join("profiles/r1_blind.yaml")
        .display()
        .to_string()
}

fn verify_legal_blue_solutions(match_path: &str, solutions: &[Value]) {
    let bytes = fs::read(match_path).unwrap();
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

    for solution in solutions {
        assert_eq!(solution["normalized"], true);
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

fn opening_decision(view: &Value, side: &str) -> String {
    let offer = &view["header"][side]["offers"][0];
    format!(
        "{{type: choose_advance_team, index: 0, name: {}, specialist: {}}}",
        offer["team"].as_str().unwrap(),
        offer["specialist"].as_str().unwrap()
    )
}

fn open_round_one(directory: &Path) -> String {
    let path = directory.join("match.yaml").display().to_string();
    let blue = run(&["match", "new", &path, "--seed", "12345", "--map", "1011"]).ok();
    let red = run(&["match", "new", &path]).ok();

    let blue_open = opening_decision(&blue, "blue");
    let red_open = opening_decision(&red, "red");
    run(&["match", "act", &path, "--side", "blue", &blue_open]).ok();
    run(&["match", "act", &path, "--side", "red", &red_open]).ok();
    run(&["match", "commit", &path, "--side", "blue"]).ok();
    run(&["match", "commit", &path, "--side", "red"]).ok();

    path
}

/// Adds one recorded purchase per side and commits the round.
fn played_round_one(directory: &Path) -> String {
    let path = open_round_one(directory);
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
fn solve_default_profile_runs_coordinate_sweep_loop_and_verifies_actions() {
    let dir = tempfile::tempdir().unwrap();
    let match_path = played_round_one(dir.path());
    let profile = default_profile();

    // Default --round selects round 1 (last round with actions), default --side is blue.
    let result = run(&["solve", &match_path, "--profile", &profile]).ok();
    assert_eq!(result["schema"], "mechcore.solve");
    assert_eq!(result["round"], 1);
    assert_eq!(result["side"], "blue");
    assert_eq!(result["profile"], "default");

    assert!(result.get("baseline").is_none(), "{result}");
    let solutions = result["solutions"].as_array().unwrap();
    assert_eq!(solutions.len(), 3, "default.yaml asks for top_k=3");

    let scores: Vec<f64> = solutions
        .iter()
        .map(|sol| sol["score"].as_f64().unwrap())
        .collect();
    assert!(
        scores.windows(2).all(|pair| pair[0] >= pair[1]),
        "{scores:?}"
    );
    for sol in solutions {
        assert!(sol.get("delta_vs_baseline").is_none(), "{sol}");
    }

    // Verify repeat loop, named operator calls, and trace emission.
    assert!(result["trace"]["sweeps"].as_u64().unwrap() >= 1);
    let stages = result["trace"]["stages"].as_array().unwrap();
    assert!(
        stages
            .iter()
            .any(|s| s["operator"] == "placement.coordinate_sweep")
    );
    assert!(stages.iter().any(|s| s["operator"] == "combinator.repeat"));
    assert!(stages.iter().any(|s| s["operator"] == "call:keep_top_3"));

    // Independently replay every returned solution's actions and verify legality.
    for (idx, solution) in solutions.iter().enumerate() {
        assert_eq!(solution["rank"], idx + 1);
        let lineage = solution["lineage"].as_array().unwrap();
        assert_eq!(lineage[0]["operator"], "init");
        assert!(lineage.len() >= 3);
    }
    verify_legal_blue_solutions(&match_path, solutions);
}

#[test]
fn solve_supports_branch_scope_guards_and_multi_proposer_composition() {
    let dir = tempfile::tempdir().unwrap();
    let match_path = played_round_one(dir.path());

    // Combine stay + right-shift proposers, deduplicate via filter.dedup_poses,
    // constrain one branch with scope bounds, and cap simulations with bail guard.
    let composed_profile = dir.path().join("composed.yaml");
    fs::write(
        &composed_profile,
        "\
schema: mechcore.profile/v1
name: composed-scope-guard
services:
  opponent:
    use: opponent.replay_truth
  space:
    use: space.strict_grid
  evaluator:
    use: eval.sim_exact
proposers:
  - use: proposer.stay
  - use: proposer.shift_stub
    with: { offsets: [10, 20, 30, 40, 50, 60, 70, 80] }
guards:
  - use: guard.max_simulations
    with: { limit: 2 }
filters:
  - use: filter.dedup_poses
  - use: filter.scope_bounds
observers:
  - use: observer.trace
operators:
  prune_two:
    use: eval.top_k
    with: { top_k: 2 }
pipeline:
  - use: warm_start.own_actions
    with: { unpin_all_movable: true }
  - branch:
      - scope:
          constraints: { min_x: -500, max_x: -500 }
          body:
            - use: placement.predict_unpinned
              with: { max_branches: 4 }
      - repeat:
          max_iters: 3
          stop_on_bail: true
          body:
            - use: placement.coordinate_sweep
              with: { keep_per_unit: 1, beam_width: 2 }
  - use: flow.dedup
  - call: prune_two
",
    )
    .unwrap();

    let answer = run(&[
        "solve",
        &match_path,
        "--round",
        "1",
        "--side",
        "red",
        "--profile",
        composed_profile.to_str().unwrap(),
        "--format",
        "yaml",
    ]);
    assert_eq!(answer.code, 0, "{}", answer.error);
    let parsed: Value = serde_yaml::from_str(&answer.stdout).unwrap();
    assert_eq!(parsed["side"], "red");
    assert_eq!(parsed["profile"], "composed-scope-guard");
    assert_eq!(parsed["solutions"].as_array().unwrap().len(), 2);
    assert_eq!(parsed["trace"]["simulations"], 2);
    assert_eq!(
        parsed["trace"]["stop_reason"],
        "simulation budget reached (2/2)"
    );
}

#[test]
fn solve_returns_cached_candidates_after_budget_runs_out() {
    let dir = tempfile::tempdir().unwrap();
    let match_path = played_round_one(dir.path());
    let profile = dir.path().join("budget-cache.yaml");
    fs::write(
        &profile,
        "\
schema: mechcore.profile/v1
name: budget-cache
services:
  opponent: { use: opponent.replay_truth }
guards:
  - { use: guard.max_simulations, with: { limit: 1 } }
pipeline:
  - { use: eval.score }
  - branch:
      - { use: warm_start.own_actions }
      - { use: flow.dedup }
",
    )
    .unwrap();
    // `eval.score` spends the entire 1-fight budget on the opening state.
    // The first arm's deployed state is an uncached miss and is dropped, while
    // the second arm (`flow.dedup`) retains the cached opening state.
    let result = run(&["solve", &match_path, "--profile", profile.to_str().unwrap()]).ok();
    assert_eq!(result["trace"]["simulations"], 1);
    assert_eq!(
        result["trace"]["stop_reason"],
        "simulation budget reached (1/1)"
    );
    let solutions = result["solutions"].as_array().unwrap();
    assert_eq!(solutions.len(), 1);
    assert!(solutions[0]["score"].is_number());
    assert_eq!(solutions[0]["lineage"].as_array().unwrap().len(), 1);
    assert_eq!(solutions[0]["lineage"][0]["operator"], "init");
    let stages = result["trace"]["stages"].as_array().unwrap();
    assert_eq!(stages[0]["operator"], "eval.score");
    assert_eq!(stages[0]["simulations"], 1);
}

#[test]
fn solve_refuses_bad_profiles_cycles_and_non_match_inputs() {
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

    // Cyclic operator call -> refused statically before running.
    let cycle_profile = dir.path().join("cycle.yaml");
    fs::write(
        &cycle_profile,
        "\
schema: mechcore.profile/v1
name: cycle
services:
  opponent:
    use: opponent.replay_truth
operators:
  a: { call: b }
  b: { call: a }
pipeline:
  - call: a
",
    )
    .unwrap();
    let reason = run(&[
        "solve",
        &match_path,
        "--profile",
        cycle_profile.to_str().unwrap(),
    ])
    .refused();
    assert!(reason.contains("cyclic"), "{reason}");

    // Undefined operator call -> refused statically.
    let missing_call = dir.path().join("missing_call.yaml");
    fs::write(
        &missing_call,
        "\
schema: mechcore.profile/v1
name: missing-call
services:
  opponent:
    use: opponent.replay_truth
pipeline:
  - call: does_not_exist
",
    )
    .unwrap();
    let reason = run(&[
        "solve",
        &match_path,
        "--profile",
        missing_call.to_str().unwrap(),
    ])
    .refused();
    assert!(reason.contains("undefined operator"), "{reason}");

    // Non-grid offset -> refused.
    let bad_offset = dir.path().join("bad_offset.yaml");
    fs::write(
        &bad_offset,
        "\
schema: mechcore.profile/v1
name: bad-offset
services:
  opponent:
    use: opponent.replay_truth
proposers:
  - use: proposer.shift_stub
    with: { offsets: [5] }
pipeline:
  - use: warm_start.own_actions
",
    )
    .unwrap();
    let reason = run(&[
        "solve",
        &match_path,
        "--profile",
        bad_offset.to_str().unwrap(),
    ])
    .refused();
    assert!(reason.contains("10m"), "{reason}");
}

#[test]
fn solve_r1_blind_profile_generates_legal_macro_solutions_against_six_beliefs() {
    let dir = tempfile::tempdir().unwrap();
    let match_path = played_round_one(dir.path());
    let profile = r1_blind_profile();

    let result = run(&["solve", &match_path, "--profile", &profile]).ok();
    assert_eq!(result["schema"], "mechcore.solve");
    assert_eq!(result["round"], 1);
    assert_eq!(result["side"], "blue");
    assert_eq!(result["profile"], "r1-blind");

    let solutions = result["solutions"].as_array().unwrap();
    assert_eq!(solutions.len(), 3);

    let scores: Vec<f64> = solutions
        .iter()
        .map(|sol| sol["score"].as_f64().unwrap())
        .collect();
    assert!(
        scores.windows(2).all(|pair| pair[0] >= pair[1]),
        "{scores:?}"
    );

    let stages = result["trace"]["stages"].as_array().unwrap();
    assert!(
        stages
            .iter()
            .any(|s| s["operator"] == "seed.macro_templates")
    );

    let sims = result["trace"]["simulations"].as_u64().unwrap();
    assert!(sims >= 6 && sims.is_multiple_of(6), "sims={sims}");
    assert!(result.get("baseline").is_none(), "{result}");
    for sol in solutions {
        assert!(sol.get("delta_vs_baseline").is_none(), "{sol}");
    }

    verify_legal_blue_solutions(&match_path, solutions);
}

#[test]
fn solve_refuses_template_plugins_without_templates_service() {
    let dir = tempfile::tempdir().unwrap();
    let match_path = played_round_one(dir.path());

    let missing_for_opponent = dir.path().join("missing-templates-opp.yaml");
    fs::write(
        &missing_for_opponent,
        "\
schema: mechcore.profile/v1
name: missing-templates-opp
services:
  opponent:
    use: opponent.replay_unpinned_templates
pipeline:
  - use: warm_start.own_actions
",
    )
    .unwrap();
    let reason = run(&[
        "solve",
        &match_path,
        "--profile",
        missing_for_opponent.to_str().unwrap(),
    ])
    .refused();
    assert!(reason.contains("services.templates"), "{reason}");

    let missing_for_seed = dir.path().join("missing-templates-seed.yaml");
    fs::write(
        &missing_for_seed,
        "\
schema: mechcore.profile/v1
name: missing-templates-seed
services:
  opponent:
    use: opponent.replay_truth
pipeline:
  - use: warm_start.own_actions
  - use: seed.macro_templates
",
    )
    .unwrap();
    let reason = run(&[
        "solve",
        &match_path,
        "--profile",
        missing_for_seed.to_str().unwrap(),
    ])
    .refused();
    assert!(reason.contains("services.templates"), "{reason}");
}

#[test]
fn solve_r1_blind_succeeds_when_opponent_actions_are_absent() {
    let dir = tempfile::tempdir().unwrap();
    let path = open_round_one(dir.path());
    let profile = r1_blind_profile();
    let result = run(&[
        "solve",
        &path,
        "--round",
        "1",
        "--side",
        "blue",
        "--profile",
        &profile,
    ])
    .ok();
    assert!(result.get("baseline").is_none(), "{result}");
    let solutions = result["solutions"].as_array().unwrap();
    assert_eq!(solutions.len(), 3);
    for sol in solutions {
        assert!(sol.get("delta_vs_baseline").is_none(), "{sol}");
    }
}

fn extract_timeline_json(html: &str) -> Value {
    let marker = r#"<script id="timeline" type="application/json">"#;
    let start = html.find(marker).expect("timeline script tag") + marker.len();
    let end = html[start..]
        .find("</script>")
        .expect("closing timeline script tag");
    serde_json::from_str(&html[start..start + end]).unwrap()
}

#[test]
fn solve_html_writes_offline_comparison_page_with_baseline_and_solutions() {
    let dir = tempfile::tempdir().unwrap();
    let match_path = played_round_one(dir.path());
    let profile = r1_blind_profile();
    let out_html = dir.path().join("compare.html");
    let out_str = out_html.to_str().unwrap();

    let res = run(&[
        "solve",
        &match_path,
        "--profile",
        &profile,
        "--round",
        "1",
        "--side",
        "red",
        "--html",
        out_str,
    ])
    .ok();
    assert_eq!(res["html"], out_str);
    assert_eq!(res["solutions"].as_array().unwrap().len(), 3);

    let html = fs::read_to_string(&out_html).unwrap();
    let timelines = extract_timeline_json(&html);
    let arr = timelines.as_array().unwrap();
    assert_eq!(arr.len(), 4, "expected baseline + 3 solve candidates");
    assert!(arr[0]["label"].as_str().unwrap().starts_with("Baseline ("));
    assert!(arr[1]["label"].as_str().unwrap().starts_with("Solve #1 ("));
    for (index, entry) in arr.iter().enumerate() {
        let cores = entry["cores"].as_array().unwrap();
        let red_delta = cores[0]["damage"].as_i64().unwrap() - cores[1]["damage"].as_i64().unwrap();
        let name = if index == 0 {
            "Baseline".to_owned()
        } else {
            format!("Solve #{index}")
        };
        assert_eq!(entry["label"], format!("{name} ({red_delta:+})"));
        assert!(entry["ticks"].as_u64().unwrap() > 0, "{entry}");
        assert!(!entry["lead_in"].is_null(), "{entry}");
    }

    let refused = run(&[
        "solve",
        &match_path,
        "--profile",
        &profile,
        "--round",
        "1",
        "--side",
        "red",
        "--html",
        out_str,
    ])
    .refused();
    assert!(refused.contains("--force"), "{refused}");

    let forced = run(&[
        "solve",
        &match_path,
        "--profile",
        &profile,
        "--round",
        "1",
        "--side",
        "red",
        "--html",
        out_str,
        "--force",
    ])
    .ok();
    assert_eq!(forced["html"], out_str);
}

#[test]
fn solve_html_uses_top_opponent_belief_when_actions_are_absent() {
    let dir = tempfile::tempdir().unwrap();
    let profile = r1_blind_profile();
    let open_match = open_round_one(dir.path());
    let open_html_path = dir.path().join("open_compare.html");
    let open_html_str = open_html_path.to_str().unwrap();
    let open_res = run(&[
        "solve",
        &open_match,
        "--profile",
        &profile,
        "--round",
        "1",
        "--side",
        "blue",
        "--html",
        open_html_str,
    ])
    .ok();
    assert_eq!(open_res["html"], open_html_str);

    let open_html = fs::read_to_string(&open_html_path).unwrap();
    let open_timelines = extract_timeline_json(&open_html);
    let open_arr = open_timelines.as_array().unwrap();
    assert_eq!(open_arr.len(), 3);
    assert!(
        open_arr[0]["label"]
            .as_str()
            .unwrap()
            .starts_with("Solve #1 (")
    );
    assert!(open_html.contains("vs belief:"), "{open_html_str}");
    for entry in open_arr {
        assert!(entry["ticks"].as_u64().unwrap() > 0, "{entry}");
        assert!(!entry["lead_in"].is_null(), "{entry}");
    }
}
