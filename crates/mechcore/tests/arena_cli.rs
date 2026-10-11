//! The `arena` namespace: matches played between programs.
//!
//! These run real players, as the commands an arena starts, and check the
//! contract in `docs/spec/mechcore/cli.md`: a player speaks requests and is
//! answered on the side it was given, a player that leaves or stops asking
//! is not answered for, and a batch deals each match from its own seed.

use std::{path::Path, process::Command};

use serde_json::Value;

/// A player that opens with its first offer, then concedes, which ends a
/// match in one round without a fight.
const CONCEDES: &str = r#"python3 -c '
import json, sys
def ask(request):
    print(json.dumps(request), flush=True)
    return json.loads(sys.stdin.readline())
view = ask({"op": "match.show"})
offer = view["sides"][view["side"]]["offers"][0]
ask({"op": "match.act", "decision": {"type": "choose_advance_team", "index": 0,
     "name": offer["team"], "specialist": offer["specialist"]}})
ask({"op": "match.commit"})
ask({"op": "match.show", "wait": True})
ask({"op": "match.act", "decision": {"type": "concede"}})
ask({"op": "match.commit"})
'"#;

/// A player that opens, then commits every round as it opens without taking
/// a decision, until the match is over.
const COMMITS: &str = r#"python3 -c '
import json, sys
def ask(request):
    print(json.dumps(request), flush=True)
    return json.loads(sys.stdin.readline())
view = ask({"op": "match.show"})
offer = view["sides"][view["side"]]["offers"][0]
ask({"op": "match.act", "decision": {"type": "choose_advance_team", "index": 0,
     "name": offer["team"], "specialist": offer["specialist"]}})
ask({"op": "match.commit"})
while True:
    if ask({"op": "match.show", "wait": True})["phase"] == "over":
        break
    ask({"op": "match.commit"})
'"#;

fn arena(arguments: &[&str]) -> (i32, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .arg("arena")
        .arg("run")
        .args(arguments)
        .output()
        .unwrap();
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8(output.stdout).unwrap() + &String::from_utf8(output.stderr).unwrap(),
    )
}

fn played(arguments: &[&str]) -> Value {
    let (code, out) = arena(arguments);
    assert_eq!(code, 0, "{out}");
    serde_json::from_str(&out).unwrap()
}

fn document(directory: &Path, name: &str) -> String {
    directory.join(name).display().to_string()
}

#[test]
fn a_side_that_concedes_loses_and_the_match_is_written() {
    let directory = tempfile::tempdir().unwrap();
    let path = document(directory.path(), "m.yaml");
    let outcome = played(&[&path, "--seed", "7", "--blue", COMMITS, "--red", CONCEDES]);
    assert_eq!(outcome["schema"], "mechcore.arena");
    assert_eq!(outcome["phase"], "over");
    assert_eq!(outcome["round"], 1);
    assert_eq!(outcome["winner"], "blue");
    assert_eq!(outcome["seed"], 7);
    assert_eq!(outcome["players"]["blue"]["ended"], "over");
    assert_eq!(outcome["players"]["red"]["code"], 0);
    // The match is a document like any other, and nothing of the round in
    // progress is left beside it.
    let report = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(["verify", &path])
        .output()
        .unwrap();
    assert!(report.status.success());
    assert!(!directory.path().join("m.turn").exists());
    assert!(directory.path().join("m.red.log").exists());
}

#[test]
fn a_player_that_leaves_in_the_opening_stops_the_match_there() {
    let directory = tempfile::tempdir().unwrap();
    let path = document(directory.path(), "m.yaml");
    let outcome = played(&[&path, "--blue", "exit 3", "--red", COMMITS]);
    assert_eq!(outcome["phase"], "opening");
    assert_eq!(outcome["winner"], Value::Null);
    assert_eq!(outcome["players"]["blue"]["ended"], "exited");
    assert_eq!(outcome["players"]["blue"]["code"], 3);
    assert_eq!(outcome["players"]["red"]["ended"], "stopped");
}

#[test]
fn a_player_that_stops_asking_is_killed_and_the_clock_ends_its_match() {
    let directory = tempfile::tempdir().unwrap();
    let path = document(directory.path(), "m.yaml");
    let outcome = played(&[
        &path,
        "--deploy-time",
        "2",
        "--request-timeout",
        "1",
        "--blue",
        COMMITS,
        "--red",
        // Opens like the waiting player, then sleeps on the first round.
        &COMMITS.replace("while True:", "import time\ntime.sleep(60)\nwhile True:"),
    ]);
    assert_eq!(outcome["phase"], "over");
    assert_eq!(outcome["winner"], "blue");
    assert_eq!(outcome["players"]["red"]["ended"], "timed_out");
}

#[test]
fn a_batch_deals_each_match_from_its_own_seed() {
    let directory = tempfile::tempdir().unwrap();
    let batch = document(directory.path(), "batch");
    let (code, out) = arena(&[
        &batch,
        "--matches",
        "2",
        "--seed",
        "40",
        "--blue",
        CONCEDES,
        "--red",
        COMMITS,
    ]);
    assert_eq!(code, 0, "{out}");
    let lines: Vec<Value> = out
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["seed"], 40);
    assert_eq!(lines[1]["seed"], 41);
    assert_eq!(lines[2]["schema"], "mechcore.arena-summary");
    assert_eq!(lines[2]["matches"], 2);
    assert_eq!(lines[2]["red"], 2);
}

#[test]
fn an_arena_deals_its_match_rather_than_joining_one() {
    let directory = tempfile::tempdir().unwrap();
    let path = document(directory.path(), "m.yaml");
    std::fs::write(&path, "").unwrap();
    let (code, out) = arena(&[&path, "--blue", COMMITS, "--red", COMMITS]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("an arena deals the match it plays"), "{out}");
}
