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
    print(sys.argv[1] if sys.argv[1] != "--name" else "original")
    sys.exit(0)
wire = pathlib.Path("bot-wire.jsonl")
def record(direction, line):
    if wire.exists():
        try:
            message = json.loads(line)
        except ValueError:
            message = line
        with pathlib.Path(f"bot-wire-{side}.jsonl").open("a") as f:
            print(json.dumps(dict(side=side, direction=direction, phase="play", message=message)), file=f)
def read():
    line = input()
    record("arena_to_bot", line)
    return json.loads(line)
def send_line(line):
    record("bot_to_arena", line)
    print(line, flush=True)
def send(x):
    send_line(json.dumps(x))
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
    record("arena_to_bot", line)
    assert p["op"] == "problem"
    limits = pathlib.Path("limits.json")
    if limits.exists():
        assert [p["timeout"], p["parallel"]] == json.loads(limits.read_text()), p
    pathlib.Path(f"{side}-{n}.yaml").write_text(doc)
    if n > 0 and "resign" in sys.argv:
        own = [dict(type="concede")]
    elif n == 0:
        if wire.exists():
            send_line("{broken")
            assert read()["error"]["kind"] == "usage"
        seats = re.findall(r"^  - \{team: ([^,]+), specialist: ([^,]+), core_modify:", doc, re.M)
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
            death = {"type":"unit_died"}
            project = {"from":"events","where":death,"select":["tick","ordinal","object","side","formation","position","last_hit"]}
            collect = {"timeline":project,"total":{"count":death},"first":{"first":death},"second":{"nth":2,"where":death},
                       "last":{"last":death},"sides":{"count":death,"group_by":"side"},"cohort":"opening_deaths","settlement":"settlement"}
            send([request("simulate", 1, {}, collect=collect),
                  request("simulate", 2, {}, until={"count":{"where":death,"at_least":1}}, collect=collect),
                  request("simulate", 3, {}, until={"tick":0}, collect={"opening":"preview"}),
                  request("simulate", 4, {}, until={"bogus":5}),
                  request("act", 5, {"red":[{"type":"invalid"}]}),
                  request("act", 6, {})])
            replies = {r["id"]:r for r in (read() for _ in range(6))}
            if p["parallel"] == 1:
                assert list(replies) == list(range(1, 7)), list(replies)
            assert "error" in replies[4] and "error" in replies[5] and "state" in replies[6]["result"], replies
            full, limited, preview = (replies[i]["result"] for i in [1,2,3])
            assert preview["tick"] == 0 and not preview["settled"] and preview["collect"]["opening"]
            rows = full["collect"]["timeline"]
            assert full["settled"] and rows and full["collect"]["total"] == len(rows)
            assert [full["collect"][k] for k in ["first","second","last"]] == [rows[i]["tick"] for i in [0,1,-1]]
            assert sum(g["value"] for g in full["collect"]["sides"]) == len(rows)
            assert limited["collect"]["timeline"] == [r for r in rows if r["tick"] == rows[0]["tick"]]
            assert limited["tick"] == rows[0]["tick"] and "settlement" not in limited["collect"]
            death_ticks = {r["object"]["id"]:r["tick"] for r in reversed(rows)}
            cohort = full["collect"]["cohort"]
            assert len(cohort) >= 2 and all(c["first_death_tick"] == death_ticks.get(c["object"]["id"]) for c in cohort)
            # Deliberately cross the threshold inside a tick with several deaths.
            i = next(i for i in range(1,len(rows)) if rows[i]["tick"] == rows[i-1]["tick"])
            burst = ask(request("simulate", 7, {}, until={"count":{"where":death,"at_least":i}}, collect={"rows":project}))["result"]
            assert burst["tick"] == rows[i]["tick"] and len(burst["collect"]["rows"]) > i
            assert burst["collect"]["rows"] == [r for r in rows if r["tick"] <= burst["tick"]]
            predicate = {"all":[{"tick":5},{"metric":{"field":"alive_members","side":"blue","at_least":1}}]}
            small = ask(request("simulate", 8, {}, until={"any":[predicate,{"tick":10}]},
                                collect={"live":{"at_stop":"alive_members","group_by":"side"}}))["result"]
            assert small["tick"] == 5 and small["stop"] == predicate
            assert {g["key"] for g in small["collect"]["live"] if g["value"] > 0} == {"red","blue"}
            buy = {"type":"buy_unit","name":"crawler"}
            bought = ask(request("act", 9, {"blue":[buy]}))["result"]["state"]
            fresh = ask(request("act", 10, {}))["result"]["state"]
            assert len(bought["blue"]["units"]) == len(fresh["blue"]["units"]) + 1
            assert bought["red"] == fresh["red"] and fresh == replies[6]["result"]["state"]
            pathlib.Path("trial.json").write_text(json.dumps(full["collect"]))
    else:
        assert n == 2 and "reinforce_offers:" in doc.split("---\n")[-1]
        if "reinforce_offers:" in doc.split("---\n")[-1]:
            section = re.search(r"^reinforce_offers:\n((?:- .+\n)+)", doc.split("---\n")[-1], re.M)[1]
            offers = re.findall(r"name: ([^,}\s]+)", section)
            decline = dict(type="choose_reinforce_item",index=len(offers)-1,name=offers[-1])
            assert "error" in ask(request("act", 1, {}))
            assert "error" in ask(request("commit", 2, []))
            assert "error" in ask(request("simulate", 3, {side:[decline]}))
            both = {s:[decline] for s in ["blue","red"]}
            handled = ask(request("act", 4, both))["result"]["state"]
            assert all(handled[s]["reinforce_chosen"] for s in both)
            wrong = {"blue":[{"type":"buy_unit","name":"crawler"}, decline], "red":[decline]}
            err = ask(request("act", 5, wrong))["error"]
            assert err["details"] == {"side":"blue","action_index":0}
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

fn json_lines(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn play(dir: &Path, args: &[&str]) -> Output {
    let mut command = vec![
        "arena",
        "new",
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
    fs::write(dir.join("limits.json"), "[0, 1]").unwrap();
    fs::write(dir.join("bot-wire.jsonl"), "").unwrap();
    let bytes = success(play(
        dir,
        &[
            "--red",
            "exec python3 bot.py",
            "--full-tech",
            "--parallel",
            "1",
            "--timeout",
            "0",
            "--log",
            "arena.jsonl",
        ],
    ));
    let game = mechcore_document::r#match::read(&bytes).unwrap().unwrap();
    let log = json_lines(&dir.join("arena.jsonl"));
    for ([query, reply], side) in log[..4].as_chunks::<2>().0.iter().zip(["blue", "red"]) {
        assert_eq!(
            query,
            &json!({"side":side, "direction":"arena_to_bot", "phase":"name", "message":"exec python3 bot.py --name"})
        );
        assert_eq!(
            reply,
            &json!({"side":side, "direction":"bot_to_arena", "phase":"name", "message":"original\n"})
        );
    }
    assert_bot_wire_logs(dir, &log[4..]);
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
    let sql = success(cli(
        dir,
        &["query", "round.mcfr", "--query", "death-timeline"],
    ));
    let offline: Value = serde_json::from_slice(&sql).unwrap();
    let positions: Value = serde_json::from_slice(&success(cli(dir, &[
        "query", "round.mcfr", "--sql",
        "SELECT position__x, position__y, position__z FROM events WHERE type='unit_died' ORDER BY tick, ordinal"
    ]))).unwrap();
    let collected: Value =
        serde_json::from_slice(&fs::read(dir.join("trial.json")).unwrap()).unwrap();
    let rows = collected["timeline"].as_array().unwrap();
    let expected = offline["rows"].as_array().unwrap();
    assert_eq!(rows.len(), expected.len());
    let positions = positions["rows"].as_array().unwrap();
    assert_eq!(rows.len(), positions.len());
    for ((row, sql), position) in rows.iter().zip(expected).zip(positions) {
        assert_eq!(
            [
                &row["tick"],
                &row["ordinal"],
                &row["object"]["id"],
                &row["formation"]
            ],
            [&sql[0], &sql[1], &sql[2], &sql[5]]
        );
        assert_eq!(row["side"], if sql[4] == 0 { "blue" } else { "red" });
        assert_eq!(
            row["position"],
            json!({"x":position[0],"y":position[1],"z":position[2]})
        );
        assert_eq!(row["last_hit"]["source"]["kind"], sql[10]);
        assert_eq!(row["last_hit"]["source"]["id"], sql[11]);
        assert_eq!(row["last_hit"]["amount"], sql[12]);
    }
    let fight = success(cli(dir, &["convert", "round.mcfr", "--to", "fight"]));
    let fight: Value = serde_yaml::from_slice(&fight).unwrap();
    for side in ["blue", "red"] {
        assert_eq!(collected["settlement"][side], fight[side]);
    }
}

fn assert_bot_wire_logs(dir: &Path, play_log: &[Value]) {
    let mut total = 0;
    for side in ["blue", "red"] {
        let recorded: Vec<Value> = play_log
            .iter()
            .filter(|entry| entry["side"] == side)
            .cloned()
            .collect();
        let wire = json_lines(&dir.join(format!("bot-wire-{side}.jsonl")));
        assert_eq!(recorded, wire);
        total += wire.len();
    }
    assert_eq!(play_log.len(), total);
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
fn load_reopens_only_the_selected_round_and_uses_the_new_bot_names() {
    use mechcore_document::r#match::{Action, Winner, canonical_yaml, read};

    let original = fixture();
    fs::write(original.path().join("clock-test"), "").unwrap();
    let cpus = std::thread::available_parallelism().map_or(1, usize::from);
    fs::write(
        original.path().join("limits.json"),
        json!([30, cpus]).to_string(),
    )
    .unwrap();
    let bytes = success(play(
        original.path(),
        &["--red", "exec python3 bot.py", "--full-tech"],
    ));
    let mut recorded = read(&bytes).unwrap().unwrap();
    assert_eq!(recorded.result.as_ref().unwrap().winner, Winner::Blue);
    recorded.blue.elo_score = Some(1900);
    recorded.red.elo_score = Some(2100);
    let source = canonical_yaml(&recorded).unwrap();

    for round in [Some("2"), None] {
        let replay = fixture();
        let dir = replay.path();
        fs::write(dir.join("match.yaml"), &source).unwrap();
        fs::write(dir.join("limits.json"), "[30.75, 2]").unwrap();
        let mut args = vec![
            "arena",
            "load",
            "match.yaml",
            "--blue",
            "exec python3 bot.py Astra resign",
            "--red",
            "exec python3 bot.py Neon",
            "--timeout",
            "30.75",
            "--parallel",
            "2",
            "--log",
            "arena.jsonl",
        ];
        if let Some(round) = round {
            args.extend(["--round", round]);
        }
        let output = read(&success(cli(dir, &args))).unwrap().unwrap();
        let (last, history) = output.turns.split_last().unwrap();
        let (expected, history_before) = if round.is_some() {
            recorded.turns.split_last().unwrap()
        } else {
            (&recorded.turns[0], &[][..])
        };
        assert_eq!(last.round, expected.round);
        assert_eq!(last.actions.blue, [Action::Concede]);
        assert!(last.actions.red.is_empty());
        assert_eq!(output.result.unwrap().winner, Winner::Red);
        assert_eq!(history, history_before);
        assert_eq!(last.state, expected.state);
        assert_eq!(output.reinforce, recorded.reinforce);
        assert_eq!(output.max_round, recorded.max_round);
        assert_eq!(output.base_core, recorded.base_core);
        assert_eq!(output.seed, recorded.seed);
        assert_eq!(output.map_id, recorded.map_id);
        for (old, new, name) in [
            (&recorded.blue, &output.blue, "Astra"),
            (&recorded.red, &output.red, "Neon"),
        ] {
            let mut expected = old.clone();
            expected.name = name.into();
            expected.elo_score = None;
            assert_eq!(new, &expected);
        }
        let first = if round.is_some() { 2 } else { 0 };
        let log = json_lines(&dir.join("arena.jsonl"));
        assert_eq!(log[4]["message"]["round"], first);
        assert_eq!(log[4]["message"]["op"], "problem");
        assert_eq!(dir.join("blue-0.yaml").exists(), first == 0);
        let problem = fs::read_to_string(dir.join(format!("blue-{first}.yaml"))).unwrap();
        let initial =
            fs::read_to_string(original.path().join(format!("blue-{first}.yaml"))).unwrap();
        assert_eq!(
            problem,
            initial
                .replacen("name: original", "name: Astra", 1)
                .replace("name: original", "name: Neon")
        );
        assert_eq!(fs::read_to_string(dir.join("match.yaml")).unwrap(), source);
        assert!(!dir.join("match.turn").exists());
    }
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
fn log_retains_the_last_failed_exchange() {
    let dir = fixture();
    fs::write(dir.path().join("broken.py"), "import sys\nif '--name' in sys.argv: print('broken')\nelse: input(); sys.stdout.buffer.write(bytes([255, 10]))\n").unwrap();
    let args = [
        "arena",
        "new",
        "--blue",
        "exec python3 broken.py",
        "--red",
        "exec python3 bot.py",
        "--log",
        "session.jsonl",
    ];
    let output = cli(dir.path(), &args);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let path = dir.path().join("session.jsonl");
    let rows = json_lines(&path);
    assert_eq!(
        rows.last().unwrap(),
        &json!({"side":"blue","phase":"play","direction":"bot_to_arena","message":{"bytes":[255]}})
    );
    let retry = cli(dir.path(), &args);
    assert!(!retry.status.success());
    let error: Value = serde_json::from_slice(&retry.stderr).unwrap();
    assert!(
        error["reason"]
            .as_str()
            .unwrap()
            .contains("cannot create arena log")
    );
    assert_eq!(rows, json_lines(&path));
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

#[test]
fn arena_new_supports_max_round_and_base_core_overrides() {
    let dir = fixture();
    let stdout = success(cli(
        dir.path(),
        &[
            "arena",
            "new",
            "--seed",
            "7",
            "--map",
            "1001",
            "--max-round",
            "1",
            "--base-core",
            "6000",
            "--blue",
            "exec python3 bot.py",
            "--red",
            "exec python3 bot.py",
        ],
    ));
    let output = String::from_utf8(stdout).unwrap();
    assert!(output.contains("max_round: 1\n"), "{output}");
    assert!(output.contains("base_core: 6000\n"), "{output}");
    assert!(!output.contains("round: 2\n"), "{output}");
    assert!(output.contains("kind: result\n"), "{output}");
}
