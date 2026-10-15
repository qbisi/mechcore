//! The `match` namespace played from the command line.
//!
//! These drive the binary the way two players would: one process deals, the
//! other joins, and each names the side it was given. What they check is the
//! contract in `docs/spec/mechcore/cli.md` — a commit is a write, a refusal
//! keeps nothing, a side does not see the other's plans, and the clock ends a
//! match against whoever did not commit.

use std::{fs, path::Path, process::Command};

use serde_json::Value;

/// One command, answered as the exit code and whatever it wrote.
struct Answer {
    code: i32,
    out: Value,
    error: Value,
}

fn run(arguments: &[&str]) -> Answer {
    let output = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(arguments)
        .output()
        .unwrap();
    let parse = |bytes: &[u8]| serde_json::from_slice(bytes).unwrap_or(Value::Null);
    Answer {
        code: output.status.code().unwrap_or(-1),
        out: parse(&output.stdout),
        error: parse(&output.stderr),
    }
}

impl Answer {
    fn ok(self) -> Value {
        assert_eq!(self.code, 0, "{}", self.error);
        self.out
    }

    /// The reason a refusal gave, which is what a player reads.
    fn refused(self) -> String {
        assert_eq!(self.code, 3, "{} {}", self.out, self.error);
        assert_eq!(self.error["kind"], "refused");
        self.error["reason"].as_str().unwrap_or_default().to_owned()
    }
}

/// Deals a match and joins it, which is what two players do to start one.
fn dealt(directory: &Path) -> String {
    let path = directory.join("m.yaml").display().to_string();
    let blue = run(&["match", "new", &path, "--seed", "12345", "--map", "1011"]).ok();
    assert_eq!(blue["side"], "blue");
    let red = run(&["match", "new", &path]).ok();
    assert_eq!(red["side"], "red");
    path
}

/// The file's state and action segments, with no projected view schema.
fn state(view: &Value) -> &Value {
    view["document"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|segment| segment["kind"] == "state")
        .unwrap()
}

fn decisions<'a>(view: &'a Value, side: &str) -> &'a Value {
    &view["document"].as_array().unwrap().last().unwrap()[side]
}

fn disk_document(path: &str) -> Value {
    use serde::Deserialize as _;
    let yaml = fs::read_to_string(path).unwrap();
    Value::Array(
        serde_yaml::Deserializer::from_str(&yaml)
            .map(|document| {
                serde_json::to_value(serde_yaml::Value::deserialize(document).unwrap()).unwrap()
            })
            .collect(),
    )
}

/// Takes the first opening a side was dealt, as the decision that takes it.
fn opening(view: &Value, side: &str) -> String {
    let offer = &view["document"][0][side]["offers"][0];
    format!(
        "{{type: choose_advance_team, index: 0, name: {}, specialist: {}}}",
        offer["team"].as_str().unwrap(),
        offer["specialist"].as_str().unwrap()
    )
}

/// Plays round zero for both sides, which opens the first round.
fn open_the_match(path: &str) {
    for side in ["blue", "red"] {
        let view = run(&["match", "show", path, "--side", side]).ok();
        let decision = opening(&view, side);
        run(&["match", "act", path, "--side", side, &decision]).ok();
        run(&["match", "commit", path, "--side", side]).ok();
    }
}

/// Declines the round's reinforcement offer, the last the side is offered.
fn decline(path: &str, side: &str) {
    let view = run(&["match", "show", path, "--side", side]).ok();
    let last = state(&view)["reinforce_offers"].as_array().unwrap().len() - 1;
    let decision = format!("{{type: choose_reinforce_item, index: {last}, name: decline_offer}}");
    run(&["match", "act", path, "--side", side, &decision]).ok();
}

fn verify(path: &str) -> Value {
    run(&["verify", path]).ok()
}

#[test]
fn a_match_is_dealt_once_joined_once_and_refuses_a_third() {
    let directory = tempfile::tempdir().unwrap();
    let path = dealt(directory.path());
    let game = mechcore_document::r#match::read(&fs::read(&path).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(game.blue.name, "blue");
    assert_eq!(game.red.name, "red");

    let third = run(&["match", "new", &path]).refused();
    assert!(third.contains("both sides"), "{third}");

    // A dealt match is a match document from the first operation, so the
    // checker reads it before a single decision is taken.
    let report = verify(&path);
    assert_eq!(report["valid"], true, "{report}");
    assert_eq!(report["kind"], "match");
    assert_eq!(report["map_id"], 1011);
}

#[test]
fn a_join_that_names_another_seed_is_refused() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("m.yaml").display().to_string();
    run(&["match", "new", &path, "--seed", "12345", "--map", "1011"]).ok();
    let refused = run(&["match", "new", &path, "--seed", "999"]).refused();
    assert!(refused.contains("seed 12345"), "{refused}");
}

#[test]
fn both_openings_open_the_first_round() {
    let directory = tempfile::tempdir().unwrap();
    let path = dealt(directory.path());

    let view = run(&["match", "show", &path, "--side", "blue"]).ok();
    assert_eq!(view["phase"], "opening");
    assert_eq!(view["round"], 0);
    // Round zero has no clock: a match nobody has opened is a file, not a
    // stalled round, and the clock runs on the rounds that are fought.
    assert!(view["remaining"].is_null(), "{view}");

    let decision = opening(&view, "blue");
    run(&["match", "act", &path, "--side", "blue", &decision]).ok();
    let after = run(&["match", "commit", &path, "--side", "blue"]).ok();
    assert_eq!(after["phase"], "opening");
    assert_eq!(after["sides"]["blue"]["committed"], true);

    let again = run(&["match", "act", &path, "--side", "blue", &decision]).refused();
    assert!(again.contains("commits a round once"), "{again}");

    // Blue has committed its opening and red has not, so red is not shown
    // what blue took: an opening reaches the board as the first round opens,
    // and until then it is a round in progress like any other.
    let view = run(&["match", "show", &path, "--side", "red"]).ok();
    assert_eq!(
        decisions(&view, "blue").as_array().unwrap().len(),
        0,
        "{view}"
    );
    assert_eq!(
        decisions(&view, "red").as_array().unwrap().len(),
        0,
        "{view}"
    );

    let decision = opening(&view, "red");
    run(&["match", "act", &path, "--side", "red", &decision]).ok();
    let opened = run(&["match", "commit", &path, "--side", "red"]).ok();

    // The opening is not fought. What it chose arrives as the first round
    // opens, which is the transition's own answer.
    assert_eq!(opened["phase"], "deploy");
    assert_eq!(opened["round"], 1);
    // Once it has, both openings are what the board stands on.
    assert_eq!(
        opened["document"][1]["blue"][0]["type"],
        "choose_advance_team"
    );
    assert!(opened["remaining"].as_f64().unwrap() > 0.0);
    let report = verify(&path);
    assert_eq!(report["valid"], true, "{report}");
    assert_eq!(report["coverage"]["total"]["unequal"], 0);
    assert_eq!(report["coverage"]["total"]["unimplemented"], 0);
}

#[test]
fn a_view_exposes_the_document_except_the_opponents_current_actions() {
    let directory = tempfile::tempdir().unwrap();
    let path = dealt(directory.path());
    open_the_match(&path);
    let before = disk_document(&path);
    run(&[
        "match",
        "act",
        &path,
        "--side",
        "red",
        "{type: unlock_unit, name: marksman}",
    ])
    .ok();
    run(&[
        "match",
        "act",
        &path,
        "--side",
        "blue",
        "{type: unlock_unit, name: rhino}",
    ])
    .ok();

    let blue = run(&["match", "show", &path, "--side", "blue"]).ok();
    let red = run(&["match", "show", &path, "--side", "red"]).ok();
    for view in [&blue, &red] {
        assert_eq!(view["document"][0], before[0]);
        assert_eq!(state(view), &before[2]);
        assert!(view.get("position").is_none());
        assert!(view["document"][0]["seed"].is_number());
        for side in ["blue", "red"] {
            assert!(view["document"][0][side]["seed"].is_number());
            assert!(view["document"][0][side]["offers"].is_array());
            assert!(
                view["document"][0][side]["tech_loadout"]
                    .as_object()
                    .unwrap()
                    .len()
                    > 2
            );
            assert!(state(view)[side]["supply"].is_number());
            assert!(state(view)[side]["unlocked_units"].is_array());
            assert_eq!(
                state(view)[side]["random_state"].as_array().unwrap().len(),
                4
            );
        }
        assert_eq!(state(view)["random_state"].as_array().unwrap().len(), 4);
    }
    assert_eq!(decisions(&blue, "blue")[0]["name"], "rhino");
    assert_eq!(decisions(&red, "red")[0]["name"], "marksman");
    assert!(decisions(&blue, "red").as_array().unwrap().is_empty());
    assert!(decisions(&red, "blue").as_array().unwrap().is_empty());
    assert_eq!(
        disk_document(&path),
        before,
        "show must not write pending decisions"
    );

    run(&["match", "commit", &path, "--side", "red"]).ok();
    let after = run(&["match", "show", &path, "--side", "blue"]).ok();
    assert_eq!(after["sides"]["red"]["committed"], true);
    assert_eq!(
        after["document"], blue["document"],
        "an opponent's commit reveals no actions"
    );
    assert_eq!(disk_document(&path)[3]["red"][0]["name"], "marksman");
    let all = run(&["match", "show", &path, "--omniscient"]).ok();
    assert_eq!(decisions(&all, "red")[0]["name"], "marksman");
    assert_eq!(decisions(&all, "blue")[0]["name"], "rhino");

    run(&["match", "commit", &path, "--side", "blue"]).ok();
    let next = run(&["match", "show", &path, "--side", "blue"]).ok();
    assert_eq!(next["round"], 2);
    assert_eq!(next["document"], disk_document(&path));
    assert_eq!(next["document"][3]["red"][0]["type"], "unlock_unit");
    assert_eq!(next["document"][3]["red"][0]["name"], "marksman");
    assert_eq!(
        state(&next)["reinforce_pool"],
        disk_document(&path)[4]["reinforce_pool"]
    );
}

#[test]
fn a_refused_decision_keeps_nothing_and_a_dry_run_keeps_nothing_either() {
    let directory = tempfile::tempdir().unwrap();
    let path = dealt(directory.path());
    open_the_match(&path);

    // A formation stands on the board's grid, so its moves keep the column
    // it was placed in and this one only changes rows.
    let view = run(&["match", "show", &path, "--side", "blue"]).ok();
    let column = state(&view)["blue"]["units"][0]["position"]["x"]
        .as_i64()
        .unwrap();

    let buy = "{type: buy_unit, name: fortress, position: {x: 0, y: -100}}";
    let refused = run(&["match", "act", &path, "--side", "blue", buy]).refused();
    assert!(refused.contains("unlocked"), "{refused}");

    let off_grid = format!(
        "{{type: move_unit, index: 0, position: {{x: {}, y: -280}}}}",
        column + 1
    );
    let refused = run(&["match", "act", &path, "--side", "blue", &off_grid]).refused();
    assert!(refused.contains("grid"), "{refused}");

    let off_board = format!("{{type: move_unit, index: 0, position: {{x: {column}, y: -1000}}}}");
    let refused = run(&["match", "act", &path, "--side", "blue", &off_board]).refused();
    assert!(refused.contains("deployment boundary"), "{refused}");

    let legal = &format!("{{type: move_unit, index: 0, position: {{x: {column}, y: -280}}}}");
    let answered = run(&["match", "act", &path, "--side", "blue", "--dry-run", legal]).ok();
    assert_eq!(answered["position"]["units"][0]["position"]["y"], -280);
    let view = run(&["match", "show", &path, "--side", "blue"]).ok();
    assert_eq!(
        decisions(&view, "blue").as_array().unwrap().len(),
        0,
        "a dry run keeps nothing: {view}"
    );

    let kept = run(&["match", "act", &path, "--side", "blue", legal]).ok();
    assert_eq!(kept["events"].as_array().unwrap().len(), 0);
    let view = run(&["match", "show", &path, "--side", "blue"]).ok();
    assert_eq!(decisions(&view, "blue").as_array().unwrap().len(), 1);
}

#[test]
fn a_decision_sequence_uses_preceding_results_and_dry_run_keeps_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let path = dealt(directory.path());
    open_the_match(&path);
    let view = run(&["match", "show", &path, "--side", "blue"]).ok();
    let index = state(&view)["blue"]["units"].as_array().unwrap().len();
    let name = state(&view)["blue"]["unlocked_units"][0].clone();
    let buy = serde_json::json!({
        "type": "buy_unit", "name": name, "position": {"x": 40, "y": -280}
    });
    let movement = serde_json::json!({
        "type": "move_unit", "index": index, "position": {"x": 40, "y": -260}
    });
    // The second step can only work after the first has created its formation.
    let absent = run(&[
        "match",
        "act",
        &path,
        "--side",
        "blue",
        &movement.to_string(),
    ]);
    assert_eq!(absent.code, 3, "{}", absent.error);
    assert_eq!(absent.error["details"]["decision_index"], 0);
    assert_eq!(absent.error["details"]["decision"], movement);
    let sequence = serde_json::json!([buy, movement]);
    let document = fs::read(&path).unwrap();
    let turn = fs::read(Path::new(&path).with_extension("turn")).unwrap();

    // A YAML sequence on the CLI and a JSON array in the stream share a path.
    let yaml = serde_yaml::to_string(&sequence).unwrap();
    let trial = run(&["match", "act", &path, "--side", "blue", "--dry-run", &yaml]).ok();
    assert_eq!(trial["position"]["units"][index]["position"]["y"], -260);
    assert_eq!(decisions(&trial, "blue").as_array().unwrap().len(), 2);
    assert_eq!(trial["events"][0]["created"]["index"], index);
    assert_eq!(trial["events"][0]["created"]["position"]["y"], -280);
    assert_eq!(fs::read(&path).unwrap(), document);
    assert_eq!(
        fs::read(Path::new(&path).with_extension("turn")).unwrap(),
        turn
    );

    let answers = requests(
        &path,
        "blue",
        &[
            serde_json::json!({"op": "match.act", "decision": sequence}).to_string(),
            r#"{"op": "match.show"}"#.to_owned(),
        ],
    );
    assert_eq!(answers[0]["position"], trial["position"]);
    assert_eq!(answers[0]["document"], trial["document"]);
    assert_eq!(answers[0]["events"], trial["events"]);
    assert_eq!(decisions(&answers[1], "blue"), decisions(&trial, "blue"));
    assert_eq!(
        fs::read(&path).unwrap(),
        document,
        "act still requires commit"
    );
}

#[test]
fn a_failed_sequence_keeps_prior_decisions_and_reports_the_first_failed_step() {
    let directory = tempfile::tempdir().unwrap();
    let path = dealt(directory.path());
    open_the_match(&path);
    let view = run(&["match", "show", &path, "--side", "blue"]).ok();
    let unchanged = serde_json::json!({
        "type": "move_unit", "index": 0,
        "position": state(&view)["blue"]["units"][0]["position"]
    });
    run(&[
        "match",
        "act",
        &path,
        "--side",
        "blue",
        &unchanged.to_string(),
    ])
    .ok();
    let document = fs::read(&path).unwrap();
    let turn = fs::read(Path::new(&path).with_extension("turn")).unwrap();
    let buy = serde_json::json!({
        "type": "buy_unit", "name": state(&view)["blue"]["unlocked_units"][0],
        "position": {"x": 40, "y": -280}
    });
    let refused = serde_json::json!({
        "type": "move_unit", "index": 99999, "position": {"x": 40, "y": -260}
    });
    let malformed = serde_json::json!({"type": "not_an_action"});
    let answers = requests(&path, "blue", &[
        serde_json::json!({"op": "match.act", "decision": [buy, refused, malformed]}).to_string(),
        serde_json::json!({"op": "match.act", "decision": [buy, refused, malformed], "dry_run": true}).to_string(),
        serde_json::json!({"op": "match.act", "decision": [buy, malformed]}).to_string(),
        r#"{"op": "match.show"}"#.to_owned(),
    ]);
    for error in &answers[..2] {
        assert_eq!(error["kind"], "refused", "{error}");
        assert_eq!(error["operation"], "match.act");
        assert_eq!(error["details"]["decision_index"], 1);
        assert_eq!(error["details"]["decision"], refused);
        assert!(!error["reason"].as_str().unwrap().is_empty());
    }
    assert_eq!(answers[2]["kind"], "usage", "{}", answers[2]);
    assert_eq!(answers[2]["details"]["decision_index"], 1);
    assert_eq!(answers[2]["details"]["decision"], malformed);
    assert_eq!(decisions(&answers[3], "blue").as_array().unwrap().len(), 1);
    assert_eq!(fs::read(&path).unwrap(), document);
    assert_eq!(
        fs::read(Path::new(&path).with_extension("turn")).unwrap(),
        turn
    );
}

#[test]
fn an_empty_or_nested_sequence_is_not_a_decision() {
    let directory = tempfile::tempdir().unwrap();
    let path = dealt(directory.path());
    let turn = fs::read(Path::new(&path).with_extension("turn")).unwrap();
    for written in ["[]", "[[{type: concede}]]", "[null]", "[17]"] {
        let answer = run(&["match", "act", &path, "--side", "blue", written]);
        assert_eq!(answer.code, 2, "{}", answer.error);
        assert_eq!(answer.error["operation"], "match.act");
        if written != "[]" {
            assert_eq!(answer.error["details"]["decision_index"], 0);
        }
        assert_eq!(
            fs::read(Path::new(&path).with_extension("turn")).unwrap(),
            turn
        );
    }
}

#[test]
fn a_purchase_answers_the_index_the_board_gave_it() {
    let directory = tempfile::tempdir().unwrap();
    let path = dealt(directory.path());
    open_the_match(&path);

    let view = run(&["match", "show", &path, "--side", "blue"]).ok();
    let held = state(&view)["blue"]["units"].as_array().unwrap().len();
    let bought = state(&view)["blue"]["unlocked_units"][0]
        .as_str()
        .unwrap()
        .to_owned();
    let buy = format!("{{type: buy_unit, name: {bought}, position: {{x: 40, y: -280}}}}");
    let answered = run(&["match", "act", &path, "--side", "blue", &buy]).ok();
    let created = &answered["events"][0]["created"];
    assert_eq!(created["index"], i64::try_from(held).unwrap());
    assert_eq!(created["name"], bought.as_str());
    assert_eq!(created["position"]["x"], 40);
    assert_eq!(created["position"]["y"], -280);
}

#[test]
fn a_committed_round_is_written_and_fought() {
    let directory = tempfile::tempdir().unwrap();
    let path = dealt(directory.path());
    open_the_match(&path);

    run(&[
        "match",
        "act",
        &path,
        "--side",
        "blue",
        "{type: unlock_unit, name: rhino}",
    ])
    .ok();
    let after = run(&["match", "commit", &path, "--side", "blue"]).ok();
    assert_eq!(after["phase"], "deploy");

    // A commit is a write: what blue decided is in the document before red
    // has decided anything.
    let written = fs::read_to_string(&path).unwrap();
    assert!(
        written.ends_with("blue:\n- {type: unlock_unit, name: rhino}\nred: []\n"),
        "{written}"
    );

    // Every opening hands its side an officer, a Defensive Wall and a turret,
    // and neither the officer nor a technology reaches a construction, so the
    // first round is fought and the second opens.
    let fought = run(&["match", "commit", &path, "--side", "red"]).ok();
    assert_eq!(fought["phase"], "deploy");
    assert_eq!(fought["round"], 2);

    // The second round is fought once each side has answered its
    // reinforcement offer, here by declining it.
    for side in ["red", "blue"] {
        decline(&path, side);
        run(&["match", "commit", &path, "--side", side]).ok();
    }
    let third = run(&["match", "show", &path, "--side", "red"]).ok();
    assert_eq!(third["phase"], "deploy", "{third}");
    assert_eq!(third["round"], 3);

    // A Centurion's Melee Mode beside Dual Wield, whose side arm shares its
    // rounds, is fought from the position the round ends in, and the fourth
    // round opens.
    for side in ["red", "blue"] {
        decline(&path, side);
    }
    for decision in [
        "{type: unlock_unit, name: centurion}",
        "{type: buy_unit, name: centurion, position: {x: 45, y: -275}}",
        "{type: upgrade_technology, unit: centurion, tech: melee_mode}",
        "{type: upgrade_technology, unit: centurion, tech: dual_wield}",
    ] {
        run(&["match", "act", &path, "--side", "red", decision]).ok();
    }
    run(&["match", "commit", &path, "--side", "red"]).ok();
    let fourth = run(&["match", "commit", &path, "--side", "blue"]).ok();
    assert_eq!(fourth["phase"], "deploy", "{fourth}");
    assert_eq!(fourth["round"], 4);
    assert!(
        fourth.get("unresolved").is_none_or(Value::is_null),
        "{fourth}"
    );
    assert_eq!(verify(&path)["valid"], true);
    // Continuing the deal needs just the preceding opening and its actions.
    let bytes = fs::read(&path).unwrap();
    let mut stated = mechcore_document::opening::stated(&bytes).unwrap().unwrap();
    let tail = stated.turns.split_off(stated.turns.len() - 2);
    stated.turns = tail;
    let dealt = mechcore_document::reinforcement::deal_checkpoint(
        mechcore_document::economy::Economy::embedded().unwrap(),
        &stated,
    )
    .unwrap();
    let expected = &stated.turns.last().unwrap().state;
    assert_eq!(dealt.offers, expected.reinforce_offers);
    assert_eq!(dealt.checkpoint.random_state, expected.random_state);
    assert_eq!(dealt.checkpoint.reinforce_pool, expected.reinforce_pool);
}

#[test]
fn a_side_that_does_not_commit_in_time_loses_the_match() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("m.yaml").display().to_string();
    run(&[
        "match",
        "new",
        &path,
        "--seed",
        "12345",
        "--map",
        "1011",
        "--deploy-time",
        "1",
    ])
    .ok();
    run(&["match", "new", &path]).ok();
    open_the_match(&path);
    run(&["match", "commit", &path, "--side", "blue"]).ok();

    std::thread::sleep(std::time::Duration::from_millis(1500));
    let view = run(&["match", "show", &path, "--side", "blue"]).ok();
    assert_eq!(view["phase"], "over", "{view}");

    // The document says what happened: the side that did not commit gave the
    // match up, which is the one way a document states a round lost without a
    // fight.
    let written = fs::read_to_string(&path).unwrap();
    assert!(
        written.ends_with("blue: []\nred:\n- {type: concede}\n"),
        "{written}"
    );
    assert!(
        !directory.path().join("m.turn").exists(),
        "a match that is over takes its turn file with it"
    );
    assert_eq!(verify(&path)["valid"], true);

    let refused = run(&[
        "match",
        "act",
        &path,
        "--side",
        "red",
        "{type: unlock_unit, name: marksman}",
    ])
    .refused();
    assert!(refused.contains("over"), "{refused}");
    assert!(
        !directory.path().join("m.turn").exists(),
        "an operation on a finished match leaves no turn file behind either"
    );
}

#[test]
fn a_lost_turn_file_is_rebuilt_from_the_document() {
    let directory = tempfile::tempdir().unwrap();
    let path = dealt(directory.path());
    let turn = directory.path().join("m.turn");

    let view = run(&["match", "show", &path, "--side", "blue"]).ok();
    let decision = opening(&view, "blue");
    run(&["match", "act", &path, "--side", "blue", &decision]).ok();
    run(&["match", "commit", &path, "--side", "blue"]).ok();
    fs::remove_file(&turn).unwrap();

    let view = run(&["match", "show", &path, "--side", "blue"]).ok();
    assert_eq!(view["rebuilt"], true, "{view}");
    // What the file held is lost. What the document holds is not: blue
    // committed its opening, and a commit cannot be taken back by losing a
    // file beside it.
    assert_eq!(view["sides"]["blue"]["committed"], true);
    assert_eq!(view["sides"]["red"]["committed"], false);
    let refused = run(&["match", "act", &path, "--side", "blue", &decision]).refused();
    assert!(refused.contains("commits a round once"), "{refused}");

    // A player that had not joined cannot join a rebuilt match.
    let refused = run(&["match", "new", &path]).refused();
    assert!(refused.contains("both sides"), "{refused}");
}

#[test]
fn a_wait_answers_when_the_round_the_other_side_opened_arrives() {
    let directory = tempfile::tempdir().unwrap();
    let path = dealt(directory.path());
    for side in ["blue", "red"] {
        let view = run(&["match", "show", &path, "--side", side]).ok();
        let decision = opening(&view, side);
        run(&["match", "act", &path, "--side", side, &decision]).ok();
    }
    run(&["match", "commit", &path, "--side", "blue"]).ok();

    // Blue has committed, so the match is not waiting for it. The wait ends
    // when red's commit opens the next round.
    let waiting = std::thread::spawn({
        let path = path.clone();
        move || run(&["match", "show", &path, "--side", "blue", "--wait", "20"]).ok()
    });
    std::thread::sleep(std::time::Duration::from_millis(300));
    run(&["match", "commit", &path, "--side", "red"]).ok();
    let view = waiting.join().unwrap();
    assert_eq!(view["round"], 1, "{view}");
    assert_eq!(view["phase"], "deploy");
    assert_eq!(view["sides"]["blue"]["committed"], false);
}

#[test]
fn a_wait_that_reaches_its_own_bound_answers_the_phase_it_is_still_in() {
    let directory = tempfile::tempdir().unwrap();
    let path = dealt(directory.path());
    let view = run(&["match", "show", &path, "--side", "blue"]).ok();
    let decision = opening(&view, "blue");
    run(&["match", "act", &path, "--side", "blue", &decision]).ok();
    run(&["match", "commit", &path, "--side", "blue"]).ok();

    let waited = run(&["match", "show", &path, "--side", "blue", "--wait", "1"]).ok();
    assert_eq!(waited["phase"], "opening");
    assert_eq!(waited["sides"]["blue"]["committed"], true);
}

#[test]
fn a_command_without_a_side_says_so() {
    let directory = tempfile::tempdir().unwrap();
    let path = dealt(directory.path());
    let answer = run(&["match", "show", &path]);
    assert_eq!(answer.code, 2);
    assert_eq!(answer.error["kind"], "usage");
    assert_eq!(answer.error["operation"], "match.show");
}

/// Writes request lines to a shell opened on one side of a match, and answers
/// the result lines it wrote back, one per request.
fn requests(path: &str, side: &str, lines: &[String]) -> Vec<Value> {
    use std::io::Write;
    let mut child = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(["shell", path, "--side", side, "--json"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for line in lines {
        writeln!(input, "{line}").unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn a_request_stream_plays_the_side_its_shell_was_opened_on() {
    let directory = tempfile::tempdir().unwrap();
    let path = dealt(directory.path());
    let view = run(&["match", "show", &path, "--side", "red"]).ok();
    let offer = &view["document"][0]["red"]["offers"][0];
    let decision = serde_json::json!({
        "type": "choose_advance_team",
        "index": 0,
        "name": offer["team"],
        "specialist": offer["specialist"],
    });
    let answers = requests(
        &path,
        "red",
        &[
            r#"{"op": "match.show"}"#.to_owned(),
            serde_json::json!({"op": "match.act", "decision": decision, "dry_run": true})
                .to_string(),
            r#"{"op": "match.commit"}"#.to_owned(),
            serde_json::json!({"op": "match.act", "decision": decision}).to_string(),
            r#"{"op": "match.commit"}"#.to_owned(),
            r#"{"op": "match.show", "wait": 0.2}"#.to_owned(),
        ],
    );
    assert_eq!(answers.len(), 6);
    // A request names no side, and every answer is the side the shell plays.
    assert_eq!(answers[0]["side"], "red");
    assert_eq!(answers[0]["phase"], "opening");
    // A dry run keeps nothing, so the commit after it has nothing to write
    // and is answered with the refusal, on the line its result would be.
    assert_eq!(
        decisions(&answers[1], "red")[0]["type"],
        "choose_advance_team"
    );
    assert_eq!(answers[2]["schema"], "mechcore.error");
    assert_eq!(answers[2]["operation"], "match.commit");
    assert_eq!(answers[4]["sides"]["red"]["committed"], true);
    // Blue has not opened, so a bounded wait answers the phase it is still in.
    assert_eq!(answers[5]["phase"], "opening");
}

#[test]
fn a_request_stream_plays_one_match() {
    let answer = run(&["shell", "--json"]);
    assert_eq!(answer.code, 2);
    assert_eq!(answer.error["kind"], "usage");
}

#[test]
fn a_round_takes_one_reinforcement_answer() {
    let directory = tempfile::tempdir().unwrap();
    let path = dealt(directory.path());
    open_the_match(&path);
    for side in ["blue", "red"] {
        run(&["match", "commit", &path, "--side", side]).ok();
    }

    decline(&path, "blue");
    let view = run(&["match", "show", &path, "--side", "blue"]).ok();
    let last = state(&view)["reinforce_offers"].as_array().unwrap().len() - 1;
    let again = format!("{{type: choose_reinforce_item, index: {last}, name: decline_offer}}");
    let refused = run(&["match", "act", &path, "--side", "blue", &again]).refused();
    assert!(refused.contains("one answer"), "{refused}");
}

#[test]
fn a_technology_outside_the_loadout_is_refused() {
    let directory = tempfile::tempdir().unwrap();
    let loadout = directory.path().join("loadout.yaml");
    fs::write(&loadout, "arclight: [charged_shot]\n").unwrap();
    let path = directory.path().join("m.yaml").display().to_string();
    let loadout = loadout.display().to_string();
    run(&[
        "match",
        "new",
        &path,
        "--seed",
        "12345",
        "--map",
        "1011",
        "--loadout",
        &loadout,
    ])
    .ok();
    run(&["match", "new", &path]).ok();
    open_the_match(&path);

    let research = "{type: upgrade_technology, unit: arclight, tech: range_enhancement}";
    let refused = run(&[
        "match",
        "act",
        &path,
        "--side",
        "blue",
        "--dry-run",
        research,
    ])
    .refused();
    assert!(refused.contains("loadout"), "{refused}");
}

#[test]
fn a_side_answers_the_offer_before_it_commits() {
    let directory = tempfile::tempdir().unwrap();
    let path = dealt(directory.path());
    open_the_match(&path);
    for side in ["blue", "red"] {
        run(&["match", "commit", &path, "--side", side]).ok();
    }

    let refused = run(&["match", "commit", &path, "--side", "blue"]).refused();
    assert!(refused.contains("answers it"), "{refused}");
    decline(&path, "blue");
    run(&["match", "commit", &path, "--side", "blue"]).ok();
}

#[test]
fn a_round_opens_with_its_reinforcement_answer() {
    let directory = tempfile::tempdir().unwrap();
    let path = dealt(directory.path());
    open_the_match(&path);
    for side in ["blue", "red"] {
        run(&["match", "commit", &path, "--side", side]).ok();
    }

    let view = run(&["match", "show", &path, "--side", "blue"]).ok();
    let bought = state(&view)["blue"]["unlocked_units"][0]
        .as_str()
        .unwrap()
        .to_owned();
    let buy = format!("{{type: buy_unit, name: {bought}, position: {{x: 40, y: -280}}}}");
    let refused = run(&["match", "act", &path, "--side", "blue", "--dry-run", &buy]).refused();
    assert!(refused.contains("first decision"), "{refused}");
    decline(&path, "blue");
    run(&["match", "act", &path, "--side", "blue", "--dry-run", &buy]).ok();
}

#[test]
fn a_match_whose_deal_does_not_verify_is_not_written_as_a_replay() {
    let directory = tempfile::tempdir().unwrap();
    let path = dealt(directory.path());
    open_the_match(&path);
    for side in ["blue", "red"] {
        run(&["match", "commit", &path, "--side", side]).ok();
    }
    for side in ["blue", "red"] {
        decline(&path, side);
        run(&["match", "commit", &path, "--side", side]).ok();
    }

    // A second answer in one round is one `match act` refuses, so it is
    // written into the document by hand.
    let document = fs::read_to_string(&path).unwrap();
    let round = "kind: action\nround: 2\nblue:\n";
    let at = document.find(round).unwrap() + round.len();
    let answer = &document[at..=at + document[at..].find('\n').unwrap()];
    assert!(answer.contains("decline_offer"), "{answer}");
    fs::write(
        &path,
        format!("{}{answer}{}", &document[..at], &document[at..]),
    )
    .unwrap();

    let replay = directory.path().join("m.grbr").display().to_string();
    let refused = run(&["convert", &path, "--to", "grbr", &replay]).refused();
    assert!(refused.contains("deal does not verify"), "{refused}");
    assert!(!Path::new(&replay).exists());
}

#[test]
fn names_are_set_when_each_side_joins_and_survive_later_commands() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("named.yaml").display().to_string();
    let blue = run(&["match", "new", &path, "--name", "Blue Bot", "--seed", "7"]).ok();
    assert_eq!(blue["document"][0]["blue"]["name"], "Blue Bot");
    assert_eq!(blue["document"][0]["red"]["name"], "red");
    let red = run(&["match", "new", &path, "--name", "Red Bot"]).ok();
    assert_eq!(red["document"][0]["blue"]["name"], "Blue Bot");
    assert_eq!(red["document"][0]["red"]["name"], "Red Bot");
    let shown = run(&["match", "show", &path, "--side", "blue"]).ok();
    assert_eq!(shown["document"][0]["red"]["name"], "Red Bot");
    let game = mechcore_document::r#match::read(&fs::read(path).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(game.blue.name, "Blue Bot");
    assert_eq!(game.red.name, "Red Bot");
}
