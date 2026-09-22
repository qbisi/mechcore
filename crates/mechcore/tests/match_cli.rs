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

/// Takes the first opening a side was dealt, as the decision that takes it.
fn opening(view: &Value, side: &str) -> String {
    let offer = &view["sides"][side]["offers"][0];
    format!(
        "{{type: choose_advance_team, offer: 0, name: {}, specialist: {}}}",
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

fn verify(path: &str) -> Value {
    run(&["doc", "verify", path]).ok()
}

#[test]
fn a_match_is_dealt_once_joined_once_and_refuses_a_third() {
    let directory = tempfile::tempdir().unwrap();
    let path = dealt(directory.path());

    let third = run(&["match", "new", &path]).refused();
    assert!(third.contains("both sides"), "{third}");

    // A dealt match is a battle document from the first operation, so the
    // checker reads it before a single decision is taken.
    let report = verify(&path);
    assert_eq!(report["valid"], true, "{report}");
    assert_eq!(report["kind"], "battle");
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
        view["sides"]["blue"]["rounds"].as_array().unwrap().len(),
        0,
        "{view}"
    );
    assert_eq!(
        view["sides"]["red"]["rounds"].as_array().unwrap().len(),
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
        opened["sides"]["blue"]["rounds"][0]["decisions"][0]["type"],
        "choose_advance_team"
    );
    assert!(opened["remaining"].as_f64().unwrap() > 0.0);
    let report = verify(&path);
    assert_eq!(report["valid"], true, "{report}");
    assert_eq!(report["coverage"]["total"]["unequal"], 0);
    assert_eq!(report["coverage"]["total"]["unimplemented"], 0);
}

#[test]
fn a_side_sees_its_own_plans_and_the_other_side_only_on_the_board() {
    let directory = tempfile::tempdir().unwrap();
    let path = dealt(directory.path());
    open_the_match(&path);

    run(&[
        "match",
        "act",
        &path,
        "--side",
        "red",
        "{type: unlock_unit, name: marksman}",
    ])
    .ok();

    let view = run(&["match", "show", &path, "--side", "blue"]).ok();
    let (own, other) = (&view["sides"]["blue"], &view["sides"]["red"]);
    assert!(own["position"]["supply"].is_number(), "{own}");
    assert!(own["offers"].is_array());
    assert!(other["position"]["supply"].is_null(), "{other}");
    assert!(other["position"]["shop"].is_null(), "{other}");
    assert!(other["offers"].is_null(), "{other}");
    assert!(other["decisions"].is_null(), "{other}");
    // The seed deals both openings and every offer, so neither player is told
    // it.
    assert!(view["seed"].is_null(), "{view}");

    // A loadout is shown for what a side has fielded, which after the opening
    // is the two unit types its team arrived with.
    assert_eq!(other["tech_loadout"].as_object().unwrap().len(), 2);
    assert!(own["tech_loadout"].as_object().unwrap().len() > 2);

    let all = run(&["match", "show", &path, "--omniscient"]).ok();
    assert!(all["seed"].is_number(), "{all}");
    assert!(all["sides"]["red"]["position"]["supply"].is_number());
    assert!(all["sides"]["blue"]["position"]["supply"].is_number());
}

#[test]
fn a_refused_decision_keeps_nothing_and_a_dry_run_keeps_nothing_either() {
    let directory = tempfile::tempdir().unwrap();
    let path = dealt(directory.path());
    open_the_match(&path);

    // A formation stands on the board's grid, so its moves keep the column
    // it was placed in and this one only changes rows.
    let view = run(&["match", "show", &path, "--side", "blue"]).ok();
    let column = view["sides"]["blue"]["position"]["units"][0]["position"]["x"]
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
    assert_eq!(
        answered["sides"]["blue"]["position"]["units"][0]["position"]["y"],
        -280
    );
    let view = run(&["match", "show", &path, "--side", "blue"]).ok();
    assert_eq!(
        view["sides"]["blue"]["decisions"].as_array().unwrap().len(),
        0,
        "a dry run keeps nothing: {view}"
    );

    let kept = run(&["match", "act", &path, "--side", "blue", legal]).ok();
    assert_eq!(kept["events"].as_array().unwrap().len(), 0);
    let view = run(&["match", "show", &path, "--side", "blue"]).ok();
    assert_eq!(
        view["sides"]["blue"]["decisions"].as_array().unwrap().len(),
        1
    );
}

#[test]
fn a_purchase_answers_the_index_the_board_gave_it() {
    let directory = tempfile::tempdir().unwrap();
    let path = dealt(directory.path());
    open_the_match(&path);

    let view = run(&["match", "show", &path, "--side", "blue"]).ok();
    let held = view["sides"]["blue"]["position"]["units"]
        .as_array()
        .unwrap()
        .len();
    let bought = view["sides"]["blue"]["position"]["shop"]["unlocked_units"][0]
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
fn a_committed_round_is_written_and_the_fight_this_build_cannot_run_is_named() {
    let directory = tempfile::tempdir().unwrap();
    let path = dealt(directory.path());
    open_the_match(&path);

    run(&[
        "match",
        "act",
        &path,
        "--side",
        "blue",
        "{type: unlock_unit, name: marksman}",
    ])
    .ok();
    let after = run(&["match", "commit", &path, "--side", "blue"]).ok();
    assert_eq!(after["phase"], "deploy");

    // A commit is a write: what blue decided is in the document before red
    // has decided anything.
    let written = fs::read_to_string(&path).unwrap();
    assert!(
        written.ends_with("blue:\n- {type: unlock_unit, name: marksman}\nred: []\n"),
        "{written}"
    );

    let fought = run(&["match", "commit", &path, "--side", "red"]).ok();
    assert_eq!(fought["phase"], "fight");
    // The fight is run from the position the round ends in, so what stops it
    // is what the simulator says about that position. Every opening hands its
    // side an officer, a Defensive Wall and a turret. Each is applied alone,
    // and the turret fires, but whether the officer reaches the turret's skill
    // is not measured — so what a real match meets first is that question, and
    // the refusal names the construction it is about.
    let unresolved = fought["unresolved"].as_str().unwrap();
    assert!(
        unresolved.contains(
            "round 1 is not fought: side blue: \"rapid_fire_turret\" fires a skill, and \
             whether the side's officers and technologies reach it is not measured"
        ),
        "{unresolved}"
    );

    // Nothing is approximated: the round stands unfought and both commits
    // stand with it, so the next caller finds the same fight waiting.
    let view = run(&["match", "show", &path, "--side", "blue"]).ok();
    assert_eq!(view["phase"], "fight");
    assert_eq!(view["round"], 1);
    assert_eq!(verify(&path)["valid"], true);
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
