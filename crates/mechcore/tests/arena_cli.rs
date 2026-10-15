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
if "--name" in sys.argv[1:]:
    print("concedes")
    sys.exit(0)
def ask(request):
    print(json.dumps(request), flush=True)
    return json.loads(sys.stdin.readline())
view = ask({"op": "match.show"})
offer = view["document"][0][view["side"]]["offers"][0]
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
if "--name" in sys.argv[1:]:
    print("commits")
    sys.exit(0)
def ask(request):
    print(json.dumps(request), flush=True)
    return json.loads(sys.stdin.readline())
view = ask({"op": "match.show"})
offer = view["document"][0][view["side"]]["offers"][0]
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
    let named = COMMITS.replace("print(\"commits\")", "print(\"commits & <v1>\")");
    let outcome = played(&[&path, "--seed", "7", "--blue", &named, "--red", CONCEDES]);
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
    let game = mechcore_document::r#match::read(&std::fs::read(&path).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(game.blue.name, "commits & <v1>");
    assert_eq!(game.red.name, "concedes");

    let replay = document(directory.path(), "m.grbr");
    let output = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(["convert", &path, "--to", "grbr", &replay])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let restored =
        mechcore_document::convert::match_from_grbr(&std::fs::read(&replay).unwrap()).unwrap();
    assert_eq!(restored.blue.name, "commits & <v1>");
    assert_eq!(restored.red.name, "concedes");
}

#[test]
fn a_player_that_leaves_in_the_opening_stops_the_match_there() {
    let directory = tempfile::tempdir().unwrap();
    let path = document(directory.path(), "m.yaml");
    let outcome = played(&[
        &path,
        "--blue",
        r#"python3 -c 'import sys; print("exits") if "--name" in sys.argv else sys.exit(3)'"#,
        "--red",
        COMMITS,
    ]);
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
    let queries = directory.path().join("queries");
    let concedes = CONCEDES.replace(
        "print(\"concedes\")",
        &format!(
            "open({:?}, \"a\").write(\"name\\n\"); print(\"concedes\")",
            queries.display().to_string()
        ),
    );
    let (code, out) = arena(&[
        &batch,
        "--matches",
        "2",
        "--seed",
        "40",
        "--blue",
        &concedes,
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
    assert_eq!(std::fs::read_to_string(queries).unwrap(), "name\n");
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

#[test]
fn invalid_name_queries_fail_before_a_match_is_dealt() {
    let directory = tempfile::tempdir().unwrap();
    let path = document(directory.path(), "m.yaml");
    for (command, reason) in [
        ("true", "one nonempty name line"),
        (
            "python3 -c 'print(\"one\\ntwo\")'",
            "one nonempty name line",
        ),
        ("exit 3", "exited with"),
        (
            "python3 -c 'import time; time.sleep(2)'",
            "name query timed out",
        ),
    ] {
        let timeout = if reason == "name query timed out" {
            "0.1"
        } else {
            "5"
        };
        let (code, out) = arena(&[
            &path,
            "--blue",
            command,
            "--red",
            COMMITS,
            "--request-timeout",
            timeout,
        ]);
        assert_eq!(code, 5, "{out}");
        assert!(out.contains(reason), "{out}");
        assert!(!Path::new(&path).exists());
        assert!(!directory.path().join("m.turn").exists());
    }
}

#[test]
fn reference_bots_report_names_without_reading_a_match() {
    for (script, name) in [
        ("random-player.py", "random"),
        ("game-ai-player.py", "game-ai"),
    ] {
        let output = Command::new("python3")
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../scripts/arena")
                    .join(script),
            )
            .args(["--seed", "7", "--name"])
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!("{name}\n")
        );
    }
}
