//! End-to-end tests of the arena wire protocol with real subprocesses.
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

fn cli(directory: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .current_dir(directory)
        .args(args)
        .output()
        .unwrap()
}

fn success(output: Output) -> Vec<u8> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

// The bot only needs the two opening names from the canonical header.
// Keep this protocol test runnable with a standard-library Python.
const BOT: &str = r#"import json, pathlib, re, sys, time
if "--name" in sys.argv:
    print("test bot")
    sys.exit(0)
def read():
    return json.loads(input())
def send(x):
    print(json.dumps(x), flush=True)
def ask(x):
    send(x)
    reply = read()
    assert reply["id"] == x["id"] and reply["round"] == p["round"], reply
    return reply
def request(op, i, actions, **opts):
    return dict(op=op, id=i, actions=actions, **opts)
for line in sys.stdin:
    p = json.loads(line)
    side, n, doc = p["side"], p["round"], p["match_yaml"]
    assert p["op"] == "problem"
    pathlib.Path(f"{side}-{n}.yaml").write_text(doc)
    if n == 0:
        seats = re.findall(r"^  - \{team: ([^,]+), specialist: ([^}]+)\}", doc, re.M)
        opening = {s:[dict(type="choose_advance_team", index=i, name=seats[start+i][0],
                           specialist=seats[start+i][1])] for s,start,i in [("blue",0,2),("red",4,1)]}
        assert "kind: action" not in doc
        assert "error" in ask(request("commit", 0, opening))
        assert "error" in ask(request("act", 1, {}))
        assert ask(request("simulate", 2, opening))["result"]["stop"] == "no_fight"
        assert "state" in ask(request("act", 3, opening))["result"]
        own = opening[side]
    elif n == 1:
        if "stall" in sys.argv and side == "red":
            time.sleep(5)
            break
        own = []
        if side == "blue" and not pathlib.Path("clock-test").exists():
            send([request("simulate", 1, {}), request("act", 2, {"red":[{"type":"invalid"}]}), request("act", 3, {})])
            replies = {r["id"]: r for r in (read() for _ in range(3))}
            assert "error" in replies[2] and "state" in replies[3]["result"]
            full = replies[1]["result"]
            assert full["settled"] and full["tick"] > 0
            pathlib.Path("trial.json").write_text(json.dumps(full["collect"]))
    else:
        assert n == 2 and "reinforce_offers:" in doc.split("---\n")[-1]
        if "reinforce_offers:" in doc.split("---\n")[-1]:
            offers = re.search(r"^reinforce_offers: \[([^\]]+)\]", doc.split("---\n")[-1], re.M)[1].split(", ")
            decline = dict(type="choose_reinforce_item",index=len(offers)-1,name=offers[-1])
            assert "error" in ask(request("act", 1, {}))
            assert "error" in ask(request("commit", 2, []))
            assert "error" in ask(request("simulate", 3, {side:[decline]}))
            both = {s:[decline] for s in ["blue","red"]}
            assert "state" in ask(request("act", 4, both))["result"]
            own = [decline]
        else:
            own = []
        if side == "red":
            own = [{"type":"concede"}]
    assert ask(request("commit", 99, own))["result"]["committed"]
"#;

fn fixture() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("bot.py"), BOT).unwrap();
    dir
}

fn play(dir: &Path, args: &[&str]) -> Output {
    let mut command = vec![
        "arena",
        "--seed",
        "7",
        "--map",
        "1011",
        "--blue",
        "exec python3 bot.py",
    ];
    command.extend(args);
    cli(dir, &command)
}

#[test]
fn parallel_trials_match_the_recording_and_leave_both_openings_unchanged() {
    let dir = fixture();
    let dir = dir.path();
    let bytes = success(play(
        dir,
        &[
            "--red",
            "exec python3 bot.py",
            "--full-tech",
            "--timeout",
            "0",
        ],
    ));
    let game = mechcore_document::r#match::read(&bytes).unwrap().unwrap();
    assert_eq!(
        game.result.unwrap().winner,
        mechcore_document::r#match::Winner::Blue
    );
    for round in 0..=2 {
        assert_eq!(
            fs::read(dir.join(format!("blue-{round}.yaml"))).unwrap(),
            fs::read(dir.join(format!("red-{round}.yaml"))).unwrap()
        );
    }
    fs::write(dir.join("match.yaml"), bytes).unwrap();
    success(cli(
        dir,
        &[
            "convert",
            "match.yaml",
            "--round",
            "1",
            "--to",
            "mcfr",
            "round.mcfr",
            "--backend",
            "simulator",
        ],
    ));
    let collected: Value =
        serde_json::from_slice(&fs::read(dir.join("trial.json")).unwrap()).unwrap();
    let fight = success(cli(dir, &["convert", "round.mcfr", "--to", "fight"]));
    let fight: Value = serde_yaml::from_slice(&fight).unwrap();
    for side in ["blue", "red"] {
        assert_eq!(collected["settlement"][side], fight[side]);
    }
}

#[test]
fn deadline_concedes_the_active_seat() {
    let dir = fixture();
    fs::write(dir.path().join("clock-test"), "").unwrap();
    let bytes = success(play(
        dir.path(),
        &[
            "--red",
            "exec python3 bot.py stall",
            "--no-reinforce",
            "--timeout",
            "2",
        ],
    ));
    let game = mechcore_document::r#match::read(&bytes).unwrap().unwrap();
    assert_eq!(json!(game.result.unwrap().winner), "blue");
}

#[test]
fn bot_exit_fails_with_empty_match_output() {
    let dir = fixture();
    let output = play(
        dir.path(),
        &[
            "--red",
            "python3 -c 'import sys; print(1) if \"--name\" in sys.argv else sys.exit(3)'",
        ],
    );
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["kind"], "failed");
    assert_eq!(error["operation"], "arena");
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
