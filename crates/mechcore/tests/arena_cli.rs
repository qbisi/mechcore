//! End-to-end tests of the arena JSON-RPC 2.0 `solve` dispatch and adjudication protocol.
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

const BOT: &str = r#"import json, pathlib, sys, time

bot_name = sys.argv[1] if len(sys.argv) > 1 and sys.argv[1] not in ("resign", "stall", "illegal") else "original"
flags = set(sys.argv[1:])
wire = pathlib.Path("bot-wire.jsonl")
side_for_log = None
req_seq = 0

def record(direction, phase, line):
    if wire.exists() and side_for_log is not None:
        try:
            message = json.loads(line)
        except ValueError:
            message = line
        with pathlib.Path(f"bot-wire-{side_for_log}.jsonl").open("a") as f:
            print(json.dumps(dict(side=side_for_log, direction=direction, phase=phase, message=message)), file=f)

def send(obj, phase="solve"):
    line = json.dumps(obj)
    record("bot_to_host", phase, line)
    print(line, flush=True)

def read_msg(phase="solve"):
    while True:
        line = input()
        if not line.strip():
            continue
        record("host_to_bot", phase, line)
        return json.loads(line)

def rpc_req(method, params):
    global req_seq
    req_seq += 1
    return {"jsonrpc": "2.0", "id": req_seq, "method": method, "params": params}

def ask(method, params):
    req = rpc_req(method, params)
    send(req)
    reply = read_msg()
    assert reply["id"] == req["id"], reply
    return reply

for raw in sys.stdin:
    if not raw.strip():
        continue
    msg = json.loads(raw)
    method = msg["method"]
    msg_id = msg["id"]
    if method == "initialize":
        caps = msg["params"]["capabilities"]
        assert caps["methods"] == ["convert", "act", "simulate"]
        reply = {"jsonrpc": "2.0", "id": msg_id, "result": {"name": bot_name}}
        # Delay wire recording until first solve tells us our side, or record on flush once side is known.
        init_host_msg = raw.strip()
        init_bot_msg = json.dumps(reply)
        print(init_bot_msg, flush=True)
        continue

    assert method == "solve", msg
    p = msg["params"]
    side = p["side"]
    if side_for_log is None:
        side_for_log = side
        record("host_to_bot", "initialize", init_host_msg)
        record("bot_to_host", "initialize", init_bot_msg)
    record("host_to_bot", "solve", raw.strip())

    prob = p["problem"]
    st = prob["state"]
    n = st["round"]
    ref = prob["ref"]
    limits = p["limits"]

    limits_file = pathlib.Path("limits.json")
    if limits_file.exists():
        assert [limits["timeout"], limits["parallel"]] == json.loads(limits_file.read_text()), limits

    # Problem invariants: expanded tech_loadout & reinforce_pool, pure JSON, neutral across sides.
    assert isinstance(st["blue"]["tech_loadout"], dict) and isinstance(st["red"]["tech_loadout"], dict)
    if st.get("reinforce", True):
        assert isinstance(st["reinforce_pool"], list)
    else:
        assert "reinforce_pool" not in st
    assert "name" not in st["blue"] and "elo_score" not in st["blue"]
    pathlib.Path(f"{side}-{n}.json").write_text(json.dumps(prob, sort_keys=True))

    if n > 0 and "resign" in flags:
        own = [{"type": "concede"}]
    elif n == 0:
        assert "history" not in prob
        offers = st[side]["offers"]
        other = "red" if side == "blue" else "blue"
        other_offers = st[other]["offers"]
        idx = 2 if side == "blue" else 1
        own_offer = offers[idx]
        other_offer = other_offers[1 if side == "blue" else 2]
        own = [{"type": "choose_advance_team", "index": idx, "name": own_offer["team"], "specialist": own_offer["specialist"]}]
        other_acts = [{"type": "choose_advance_team", "index": 1 if side == "blue" else 2, "name": other_offer["team"], "specialist": other_offer["specialist"]}]

        # Refused act & no_fight simulate & valid act on ref
        assert "error" in ask("act", {"context": {"ref": ref}, "actions": {"blue": []}})
        assert ask("simulate", {"context": {"ref": ref}, "actions": {side: own, other: other_acts}})["result"]["stop"] == "no_fight"
        assert "state" in ask("act", {"context": {"ref": ref}, "actions": {side: own}})["result"]
    elif n == 1:
        assert len(prob["history"]) == 1 and prob["history"][0]["round"] == 0
        if "stall" in flags and side == "red":
            time.sleep(5)
            break
        if "illegal" in flags and side == "red":
            send({"jsonrpc": "2.0", "id": msg_id, "result": {"actions": [{"type": "buy_unit", "name": "vulcan"}]}})
            continue
        own = []
        if side == "blue" and not pathlib.Path("clock-test").exists():
            death = {"type": "unit_died"}
            project = {"from": "events", "where": death, "select": ["tick", "ordinal", "object", "side", "formation", "position", "last_hit"]}
            collect = {
                "timeline": project, "total": {"count": death}, "first": {"first": death},
                "second": {"nth": 2, "where": death}, "last": {"last": death},
                "sides": {"count": death, "group_by": "side"}, "cohort": "opening_deaths",
                "settlement": "settlement",
            }
            batch = [
                rpc_req("simulate", {"context": {"ref": ref}, "collect": collect}),
                rpc_req("simulate", {"context": {"ref": ref}, "until": {"count": {"where": death, "at_least": 1}}, "collect": collect}),
                rpc_req("simulate", {"context": {"ref": ref}, "until": {"tick": 0}, "collect": {"opening": "preview"}}),
                rpc_req("simulate", {"context": {"ref": ref}, "until": {"bogus": 5}}),
                rpc_req("act", {"context": {"ref": ref}, "actions": {"red": [{"type": "invalid"}]}}),
                rpc_req("act", {"context": {"ref": ref}, "actions": {}}),
            ]
            send(batch)
            replies = {r["id"]: r for r in (read_msg() for _ in range(len(batch)))}
            assert "error" in replies[batch[3]["id"]] and "error" in replies[batch[4]["id"]] and "state" in replies[batch[5]["id"]]["result"]
            full, limited, preview = (replies[batch[i]["id"]]["result"] for i in [0, 1, 2])
            assert preview["tick"] == 0 and not preview["settled"] and preview["collect"]["opening"]
            rows = full["collect"]["timeline"]
            assert full["settled"] and rows and full["collect"]["total"] == len(rows)
            assert [full["collect"][k] for k in ["first", "second", "last"]] == [rows[i]["tick"] for i in [0, 1, -1]]
            assert sum(g["value"] for g in full["collect"]["sides"]) == len(rows)
            assert limited["collect"]["timeline"] == [r for r in rows if r["tick"] == rows[0]["tick"]]
            assert limited["tick"] == rows[0]["tick"] and "settlement" not in limited["collect"]
            death_ticks = {r["object"]["id"]: r["tick"] for r in reversed(rows)}
            cohort = full["collect"]["cohort"]
            assert len(cohort) >= 2 and all(c["first_death_tick"] == death_ticks.get(c["object"]["id"]) for c in cohort)

            buy = {"type": "buy_unit", "name": "crawler"}
            bought = ask("act", {"context": {"ref": ref}, "actions": {"blue": [buy]}})["result"]["state"]
            fresh = ask("act", {"context": {"ref": ref}, "actions": {}})["result"]["state"]
            assert len(bought["blue"]["units"]) == len(fresh["blue"]["units"]) + 1
            assert bought["red"] == fresh["red"]
            pathlib.Path("trial.json").write_text(json.dumps(full["collect"]))
    else:
        assert n == 2 and "reinforce_offers" in st
        offers = st["reinforce_offers"]
        decline = {"type": "choose_reinforce_item", "index": len(offers) - 1, "name": offers[-1]["name"]}
        assert "error" in ask("act", {"context": {"ref": ref}, "actions": {"blue": []}})
        assert "error" in ask("simulate", {"context": {"ref": ref}, "actions": {side: [decline]}})
        both = {s: [decline] for s in ["blue", "red"]}
        handled = ask("act", {"context": {"ref": ref}, "actions": both})["result"]["state"]
        assert all(handled[s]["reinforce_chosen"] for s in both)
        wrong = {"blue": [{"type": "buy_unit", "name": "crawler"}, decline], "red": [decline]}
        err = ask("act", {"context": {"ref": ref}, "actions": wrong})["error"]
        assert err["data"]["details"] == {"side": "blue", "action_index": 0}
        own = [decline]
        if side == "red":
            own = [{"type": "concede"}]

    send({"jsonrpc": "2.0", "id": msg_id, "result": {"actions": own}})
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
    fs::write(dir.join("limits.json"), "[null, 1]").unwrap();
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
        assert_eq!(query["side"], side);
        assert_eq!(query["direction"], "host_to_bot");
        assert_eq!(query["phase"], "initialize");
        assert_eq!(query["message"]["method"], "initialize");
        assert_eq!(reply["side"], side);
        assert_eq!(reply["direction"], "bot_to_host");
        assert_eq!(reply["phase"], "initialize");
        assert_eq!(reply["message"]["result"]["name"], "original");
    }
    assert_bot_wire_logs(dir, &log);
    assert_eq!(
        game.result.unwrap().winner,
        mechcore_document::r#match::Winner::Blue
    );
    for round in 0..=2 {
        assert_eq!(
            fs::read(dir.join(format!("blue-{round}.json"))).unwrap(),
            fs::read(dir.join(format!("red-{round}.json"))).unwrap()
        );
    }
    assert_trial_matches_mcfr(dir, &bytes);
}

fn assert_trial_matches_mcfr(dir: &Path, match_bytes: &[u8]) {
    fs::write(dir.join("match.yaml"), match_bytes).unwrap();
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
    let positions: Value = serde_json::from_slice(&success(cli(
        dir,
        &[
            "query",
            "round.mcfr",
            "--sql",
            "SELECT position__x, position__y, position__z FROM events WHERE type='unit_died' ORDER BY tick, ordinal",
        ],
    )))
    .unwrap();
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
                &row["formation"],
            ],
            [&sql[0], &sql[1], &sql[2], &sql[5]]
        );
        assert_eq!(row["side"], if sql[4] == 0 { "blue" } else { "red" });
        assert_eq!(
            row["position"],
            json!({"x": position[0], "y": position[1], "z": position[2]})
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

fn assert_bot_wire_logs(dir: &Path, full_log: &[Value]) {
    let mut total = 0;
    for side in ["blue", "red"] {
        let recorded: Vec<Value> = full_log
            .iter()
            .filter(|entry| entry["side"] == side)
            .cloned()
            .collect();
        let wire = json_lines(&dir.join(format!("bot-wire-{side}.jsonl")));
        assert_eq!(recorded, wire);
        total += wire.len();
    }
    assert_eq!(full_log.len(), total);
}

#[test]
fn deadline_and_illegal_deploy_adjudicate_as_concede() {
    for (flag, expect_illegal_in_log) in [("stall", false), ("illegal", true)] {
        let dir = fixture();
        fs::write(dir.path().join("clock-test"), "").unwrap();
        let args = [
            "--red",
            &format!("exec python3 bot.py {flag}"),
            "--no-reinforce",
            "--timeout",
            "1",
            "--log",
            "adj.jsonl",
        ];
        let bytes = success(play(dir.path(), &args));
        let game = mechcore_document::r#match::read(&bytes).unwrap().unwrap();
        assert_eq!(json!(game.result.unwrap().winner), "blue");
        assert_eq!(
            game.turns.last().unwrap().actions.red,
            [mechcore_document::r#match::Action::Concede]
        );
        if expect_illegal_in_log {
            let log = json_lines(&dir.path().join("adj.jsonl"));
            assert!(log.iter().any(|entry| {
                entry["side"] == "red"
                    && entry["direction"] == "bot_to_host"
                    && entry["message"]["result"]["actions"][0]["name"] == "vulcan"
            }));
        }
    }
}

#[test]
fn opening_illegal_or_timeout_fails_with_exit_5() {
    let dir = fixture();
    let bad_open = r#"import json, sys
for line in sys.stdin:
    msg = json.loads(line)
    if msg["method"] == "initialize":
        print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": {"name": "bad-open"}}), flush=True)
    else:
        print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": {"actions": []}}), flush=True)
"#;
    fs::write(dir.path().join("bad_open.py"), bad_open).unwrap();
    let out = play(dir.path(), &["--red", "exec python3 bad_open.py"]);
    assert_eq!(out.status.code(), Some(5));
    assert!(out.stdout.is_empty());
    let err: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(err["kind"], "failed");
}

#[test]
fn load_reopens_only_the_selected_round_and_uses_the_new_bot_names() {
    use mechcore_document::r#match::{Action, Winner, canonical_yaml, read};

    let original = fixture();
    fs::write(original.path().join("clock-test"), "").unwrap();
    let cpus = std::thread::available_parallelism().map_or(1, usize::from);
    fs::write(
        original.path().join("limits.json"),
        json!([30.0, cpus]).to_string(),
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
        assert_eq!(log[4]["message"]["method"], "solve");
        assert_eq!(
            log[4]["message"]["params"]["problem"]["state"]["round"],
            first
        );
        assert_eq!(dir.join("blue-0.json").exists(), first == 0);
        let problem: Value = serde_json::from_str(
            &fs::read_to_string(dir.join(format!("blue-{first}.json"))).unwrap(),
        )
        .unwrap();
        let initial: Value = serde_json::from_str(
            &fs::read_to_string(original.path().join(format!("blue-{first}.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(problem, initial);
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
            "python3 -c 'import json, sys; msg = json.loads(input()); sys.exit(3)'",
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
    fs::write(
        dir.path().join("broken.py"),
        "import json, sys\nmsg = json.loads(input())\nprint(json.dumps({'jsonrpc':'2.0','id':msg['id'],'result':{'name':'broken'}}), flush=True)\ninput()\nsys.stdout.buffer.write(bytes([255, 10]))\n",
    )
    .unwrap();
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
        &json!({"side":"blue","direction":"bot_to_host","phase":"solve","message":{"bytes":[255]}})
    );
    let retry = cli(dir.path(), &args);
    assert!(!retry.status.success());
    let error: Value = serde_json::from_slice(&retry.stderr).unwrap();
    assert!(
        error["reason"]
            .as_str()
            .unwrap()
            .contains("cannot create wire log")
    );
    assert_eq!(rows, json_lines(&path));
}

#[test]
fn reference_bots_complete_verified_match_without_pyyaml() {
    let dir = fixture();
    let scripts = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/arena");
    let blue_cmd = format!(
        "python3 {} --seed 1",
        scripts.join("game-ai-player.py").display()
    );
    let red_cmd = format!(
        "python3 {} --seed 2",
        scripts.join("random-player.py").display()
    );
    let match_bytes = success(cli(
        dir.path(),
        &[
            "arena",
            "new",
            "--seed",
            "7",
            "--map",
            "1001",
            "--max-round",
            "2",
            "--blue",
            &blue_cmd,
            "--red",
            &red_cmd,
        ],
    ));
    let match_path = dir.path().join("ref_match.yaml");
    fs::write(&match_path, &match_bytes).unwrap();
    let report: Value = serde_json::from_slice(&success(cli(
        dir.path(),
        &["verify", match_path.to_str().unwrap()],
    )))
    .unwrap();
    assert_eq!(report["valid"], true, "{report}");
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
    assert!(output.contains("reactor_core: 6000\n"), "{output}");
    assert!(!output.contains("base_core:"), "{output}");
    assert!(!output.contains("round: 2\n"), "{output}");
    assert!(output.contains("kind: result\n"), "{output}");
}
