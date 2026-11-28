//! Integration tests for `mechcore solve`.

use std::{fs, path::Path, process::Command};

use serde_json::{Value, json};
use tempfile::TempDir;

fn cli(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn make_open_match(dir: &Path) -> String {
    let bot = r#"import json, sys
for line in sys.stdin:
    if not line.strip():
        continue
    msg = json.loads(line)
    if msg["method"] == "initialize":
        print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": {"name": "prep"}}), flush=True)
    elif msg["method"] == "solve":
        p = msg["params"]["problem"]
        side = msg["params"]["side"]
        st = p["state"]
        r = st["round"]
        if r == 0:
            off = st[side]["offers"][0]
            acts = [{"type": "choose_advance_team", "index": 0, "name": off["team"], "specialist": off["specialist"]}]
        elif r == 1:
            unlocked = set(st[side]["unlocked_units"])
            pick = next(u for u in ("crawler", "fang", "marksman", "arclight", "mustang") if u not in unlocked)
            acts = [{"type": "unlock_unit", "name": pick}]
        else:
            offers = st.get("reinforce_offers") or []
            if offers:
                acts = [{"type": "choose_reinforce_item", "index": len(offers) - 1, "name": offers[-1]["name"]}]
            else:
                acts = []
            if side == "red" and r == 2:
                acts.append({"type": "concede"})
        print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": {"actions": acts}}), flush=True)
"#;
    fs::write(dir.join("prep.py"), bot).unwrap();
    let out = cli(
        dir,
        &[
            "arena",
            "new",
            "--seed",
            "7",
            "--map",
            "1011",
            "--blue",
            "exec python3 prep.py",
            "--red",
            "exec python3 prep.py",
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

const REVERSE_RPC_SOLVER: &str = r#"import json, sys
for line in sys.stdin:
    if not line.strip():
        continue
    msg = json.loads(line)
    if msg["method"] == "initialize":
        print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": {"name": "smart-bot"}}), flush=True)
        continue
    assert msg["method"] == "solve"
    p = msg["params"]["problem"]
    side = msg["params"]["side"]
    st = p["state"]
    r = st["round"]
    if r == 0:
        assert "history" not in p
        assert isinstance(st["reinforce_pool"], list)
        off = st[side]["offers"][0]
        acts = [{"type": "choose_advance_team", "index": 0, "name": off["team"], "specialist": off["specialist"]}]
        meta = {}
    else:
        assert isinstance(st[side]["tech_loadout"], dict)
        other = "red" if side == "blue" else "blue"
        offers = st.get("reinforce_offers") or []
        acts = []
        opp_acts = []
        if offers:
            pick = {"type": "choose_reinforce_item", "index": len(offers) - 1, "name": offers[-1]["name"]}
            acts.append(pick)
            opp_acts.append(pick)
        print(json.dumps({"jsonrpc": "2.0", "id": 1, "method": "act", "params": {"context": {"ref": p["ref"]}, "actions": {side: acts}}}), flush=True)
        rep1 = json.loads(input())
        print(json.dumps({"jsonrpc": "2.0", "id": 2, "method": "act", "params": {"context": {"state": st}, "actions": {side: acts}}}), flush=True)
        rep2 = json.loads(input())
        assert rep1["result"]["state"] == rep2["result"]["state"]
        print(json.dumps({"jsonrpc": "2.0", "id": 3, "method": "simulate", "params": {"context": {"ref": p["ref"]}, "actions": {side: acts, other: opp_acts}, "until": {"tick": 0}, "collect": {"b": "preview"}}}), flush=True)
        sim1 = json.loads(input())
        assert sim1["result"]["tick"] == 0
        meta = {"value": 0.75}
    print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": {"actions": acts, "meta": meta}}), flush=True)
"#;

#[test]
fn solve_round_zero_omits_empty_meta() {
    let dir = TempDir::new().unwrap();
    let match_yaml = make_open_match(dir.path());
    fs::write(dir.path().join("m.yaml"), &match_yaml).unwrap();
    fs::write(dir.path().join("solver.py"), REVERSE_RPC_SOLVER).unwrap();

    let r0 = cli(
        dir.path(),
        &[
            "solve",
            "m.yaml",
            "--side",
            "blue",
            "--round",
            "0",
            "--bot",
            "exec python3 solver.py",
        ],
    );
    assert!(
        r0.status.success(),
        "{}",
        String::from_utf8_lossy(&r0.stderr)
    );
    let r0_json: Value = serde_json::from_slice(&r0.stdout).unwrap();
    assert_eq!(r0_json["schema"], "mechcore.solve-result");
    assert_eq!(r0_json["bot"], "smart-bot");
    assert_eq!(r0_json["round"], 0);
    assert!(r0_json.get("meta").is_none());
}

#[test]
fn solve_open_match_with_reverse_rpc_and_wire_log() {
    let dir = TempDir::new().unwrap();
    let match_yaml = make_open_match(dir.path());
    let parts: Vec<&str> = match_yaml.split("---\n").collect();
    let open_match = parts[..parts.len() - 2].join("---\n");
    fs::write(dir.path().join("m_open.yaml"), &open_match).unwrap();
    fs::write(dir.path().join("solver.py"), REVERSE_RPC_SOLVER).unwrap();

    let open_out = cli(
        dir.path(),
        &[
            "solve",
            "m_open.yaml",
            "--side",
            "red",
            "--bot",
            "exec python3 solver.py",
            "--log",
            "solve.jsonl",
        ],
    );
    assert!(
        open_out.status.success(),
        "{}",
        String::from_utf8_lossy(&open_out.stderr)
    );
    let open_json: Value = serde_json::from_slice(&open_out.stdout).unwrap();
    assert_eq!(open_json["round"], 2);
    assert_eq!(open_json["meta"], json!({"value": 0.75}));

    let log_rows: Vec<Value> = fs::read_to_string(dir.path().join("solve.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(log_rows[0]["phase"], "initialize");
    assert_eq!(log_rows[0]["direction"], "host_to_bot");
    assert_eq!(log_rows[1]["phase"], "initialize");
    assert_eq!(log_rows[1]["direction"], "bot_to_host");
    assert_eq!(log_rows[2]["phase"], "solve");

    let dup_log = cli(
        dir.path(),
        &[
            "solve",
            "m_open.yaml",
            "--side",
            "red",
            "--bot",
            "exec python3 solver.py",
            "--log",
            "solve.jsonl",
        ],
    );
    assert_eq!(dup_log.status.code(), Some(5));
}

#[test]
fn solve_standalone_state_supports_yaml_output() {
    let dir = TempDir::new().unwrap();
    let solver_bot = r#"import json, sys
for line in sys.stdin:
    if not line.strip():
        continue
    msg = json.loads(line)
    if msg["method"] == "initialize":
        print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": {"name": "smart-bot"}}), flush=True)
    else:
        print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": {"actions": [], "meta": {"ok": True}}}), flush=True)
"#;
    fs::write(dir.path().join("solver.py"), solver_bot).unwrap();
    let state_yaml = json!({
        "kind": "state",
        "round": 1,
        "seed": 7,
        "blue": {
            "reactor_core": 4500,
            "supply": 200,
            "unlocked_units": ["crawler"],
            "next_index": {"unit": 1, "contraption": 0},
            "units": [{"name": "marksman", "index": 0, "value": 100, "position": {"x": 0, "y": -50}}]
        },
        "red": {
            "reactor_core": 4500,
            "supply": 200,
            "unlocked_units": ["arclight"],
            "next_index": {"unit": 1, "contraption": 0},
            "units": [{"name": "arclight", "index": 0, "value": 100, "position": {"x": 0, "y": -100}}]
        }
    });
    fs::write(dir.path().join("state.yaml"), state_yaml.to_string()).unwrap();

    let state_res = cli(
        dir.path(),
        &[
            "solve",
            "state.yaml",
            "--side",
            "blue",
            "--bot",
            "exec python3 solver.py",
            "--format",
            "yaml",
        ],
    );
    assert!(
        state_res.status.success(),
        "{}",
        String::from_utf8_lossy(&state_res.stderr)
    );
    let yaml_res: Value = serde_yaml::from_slice(&state_res.stdout).unwrap();
    assert_eq!(yaml_res["round"], 1);
}

#[test]
fn solve_enforces_error_taxonomy_for_usage_and_refused() {
    let dir = TempDir::new().unwrap();
    let mut state = json!({
        "kind": "state",
        "round": 1,
        "seed": 7,
        "blue": {
            "reactor_core": 4500,
            "supply": 200,
            "unlocked_units": ["crawler"],
            "next_index": {"unit": 1, "contraption": 0},
            "units": [{"name": "marksman", "index": 0, "value": 100, "position": {"x": 0, "y": -50}}]
        },
        "red": {
            "reactor_core": 4500,
            "supply": 200,
            "next_index": {"unit": 1, "contraption": 0},
            "units": [{"name": "arclight", "index": 0, "value": 100, "position": {"x": 0, "y": -100}}]
        }
    });
    fs::write(dir.path().join("open.yaml"), state.to_string()).unwrap();

    let r_usage = cli(
        dir.path(),
        &[
            "solve",
            "open.yaml",
            "--side",
            "blue",
            "--round",
            "1",
            "--bot",
            "true",
        ],
    );
    assert_eq!(r_usage.status.code(), Some(2));

    state["blue"]["energy_tower_skills"] = json!(["rapid_resupply"]);
    fs::write(dir.path().join("mid.yaml"), state.to_string()).unwrap();
    let r_mid = cli(
        dir.path(),
        &["solve", "mid.yaml", "--side", "blue", "--bot", "true"],
    );
    assert_eq!(r_mid.status.code(), Some(3));

    state["blue"]["energy_tower_skills"] = json!([]);
    state["blue"]["reactor_core"] = json!(0);
    fs::write(dir.path().join("term.yaml"), state.to_string()).unwrap();
    let r_term = cli(
        dir.path(),
        &["solve", "term.yaml", "--side", "blue", "--bot", "true"],
    );
    assert_eq!(r_term.status.code(), Some(3));

    let bad_actions_bot = r#"import json, sys
for line in sys.stdin:
    msg = json.loads(line)
    if msg["method"] == "initialize":
        print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": {"name": "bad"}}), flush=True)
    else:
        print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": {"actions": [{"type": "buy_unit", "name": "vulcan"}]}}), flush=True)
"#;
    fs::write(dir.path().join("bad_act.py"), bad_actions_bot).unwrap();
    let r_illegal = cli(
        dir.path(),
        &[
            "solve",
            "open.yaml",
            "--side",
            "blue",
            "--bot",
            "exec python3 bad_act.py",
        ],
    );
    assert_eq!(r_illegal.status.code(), Some(3));
    let err: Value = serde_json::from_slice(&r_illegal.stderr).unwrap();
    assert_eq!(err["kind"], "refused");
    assert_eq!(err["details"], json!({"side": "blue", "action_index": 0}));
}

#[test]
fn solve_enforces_error_taxonomy_for_failed_timeout_and_protocol() {
    let dir = TempDir::new().unwrap();
    let state = json!({
        "kind": "state",
        "round": 1,
        "seed": 7,
        "blue": {
            "reactor_core": 4500,
            "supply": 200,
            "unlocked_units": ["crawler"],
            "next_index": {"unit": 1, "contraption": 0},
            "units": [{"name": "marksman", "index": 0, "value": 100, "position": {"x": 0, "y": -50}}]
        },
        "red": {
            "reactor_core": 4500,
            "supply": 200,
            "next_index": {"unit": 1, "contraption": 0},
            "units": [{"name": "arclight", "index": 0, "value": 100, "position": {"x": 0, "y": -100}}]
        }
    });
    fs::write(dir.path().join("open.yaml"), state.to_string()).unwrap();

    let slow_bot = r#"import json, sys, time
for line in sys.stdin:
    msg = json.loads(line)
    if msg["method"] == "initialize":
        print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": {"name": "slow"}}), flush=True)
    else:
        time.sleep(5)
"#;
    fs::write(dir.path().join("slow.py"), slow_bot).unwrap();
    let r_timeout = cli(
        dir.path(),
        &[
            "solve",
            "open.yaml",
            "--side",
            "blue",
            "--timeout",
            "0.2",
            "--bot",
            "exec python3 slow.py",
        ],
    );
    assert_eq!(r_timeout.status.code(), Some(5));

    let bad_meta_bot = r#"import json, sys
for line in sys.stdin:
    msg = json.loads(line)
    if msg["method"] == "initialize":
        print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": {"name": "bad-meta"}}), flush=True)
    else:
        print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": {"actions": [], "meta": 123}}), flush=True)
"#;
    fs::write(dir.path().join("bad_meta.py"), bad_meta_bot).unwrap();
    let r_meta = cli(
        dir.path(),
        &[
            "solve",
            "open.yaml",
            "--side",
            "blue",
            "--bot",
            "exec python3 bad_meta.py",
        ],
    );
    assert_eq!(r_meta.status.code(), Some(5));
}

#[test]
fn solve_closes_stdin_promptly_and_times_out_if_stdin_blocks() {
    let dir = TempDir::new().unwrap();
    let state = json!({
        "kind": "state",
        "round": 1,
        "seed": 7,
        "blue": {
            "reactor_core": 4500,
            "supply": 200,
            "unlocked_units": ["crawler"],
            "next_index": {"unit": 1, "contraption": 0},
            "units": []
        },
        "red": {
            "reactor_core": 4500,
            "supply": 200,
            "next_index": {"unit": 1, "contraption": 0},
            "units": []
        }
    });
    fs::write(dir.path().join("open.yaml"), state.to_string()).unwrap();

    // Bot that exits only on stdin EOF; finish() must close the write fd immediately.
    let eof_bot = r#"import json, sys
for line in sys.stdin:
    msg = json.loads(line)
    if msg["method"] == "initialize":
        print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": {"name": "eof-bot"}}), flush=True)
    elif msg["method"] == "solve":
        print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": {"actions": []}}), flush=True)
"#;
    fs::write(dir.path().join("eof_bot.py"), eof_bot).unwrap();
    let start = std::time::Instant::now();
    let eof_res = cli(
        dir.path(),
        &[
            "solve",
            "open.yaml",
            "--side",
            "blue",
            "--bot",
            "exec python3 eof_bot.py",
        ],
    );
    assert!(
        eof_res.status.success(),
        "{}",
        String::from_utf8_lossy(&eof_res.stderr)
    );
    assert!(
        start.elapsed() < std::time::Duration::from_millis(1500),
        "finish() waited too long for EOF: {:?}",
        start.elapsed()
    );

    // Bot answers initialize then never reads stdin while problem exceeds pipe buffer.
    let deaf_bot = r#"import json, sys, time
line = sys.stdin.readline()
msg = json.loads(line)
print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": {"name": "deaf"}}), flush=True)
time.sleep(5)
"#;
    fs::write(dir.path().join("deaf.py"), deaf_bot).unwrap();
    let deaf_res = cli(
        dir.path(),
        &[
            "solve",
            "open.yaml",
            "--side",
            "blue",
            "--timeout",
            "0.2",
            "--bot",
            "exec python3 deaf.py",
        ],
    );
    assert_eq!(deaf_res.status.code(), Some(5));
}
