//! Integration tests for `mechcore rpc` stateless JSON-RPC 2.0 server.

use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
};

use serde_json::{Value, json};
use tempfile::TempDir;

fn cli(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn spawn_rpc(args: &[&str]) -> Child {
    Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(["rpc"])
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

fn roundtrip(lines: &[&str], args: &[&str]) -> (Vec<Value>, String, std::process::ExitStatus) {
    let mut child = spawn_rpc(args);
    {
        let mut stdin = child.stdin.take().unwrap();
        for line in lines {
            writeln!(stdin, "{line}").unwrap();
        }
    }
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let replies = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .collect();
    (replies, stderr, output.status)
}

fn sample_match_yaml(dir: &Path) -> String {
    let bot = r#"import json, sys
for line in sys.stdin:
    if not line.strip():
        continue
    msg = json.loads(line)
    if msg["method"] == "initialize":
        print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": {"name": "fixture-bot"}}), flush=True)
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
    fs::write(dir.join("bot.py"), bot).unwrap();
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
            "exec python3 bot.py",
            "--red",
            "exec python3 bot.py",
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn rpc_cli_rejects_invalid_arguments_as_usage() {
    let dir = TempDir::new().unwrap();
    for args in [
        &["rpc", "--parallel", "0"][..],
        &["rpc", "--parallel", "-2"][..],
        &["rpc", "extra"][..],
    ] {
        let out = cli(dir.path(), args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(out.stdout.is_empty());
        let err: Value = serde_json::from_slice(&out.stderr).unwrap();
        assert_eq!(err["kind"], "usage");
    }
}

#[test]
fn rpc_initialize_framing_batching_and_cancel() {
    let minimal_state = json!({
        "kind": "state",
        "round": 1,
        "seed": 7,
        "blue": {
            "reactor_core": 4500,
            "supply": 200,
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

    let init_req = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"client": {"name": "test"}}});
    let bad_batch = "[]";
    let bad_json = "{broken";
    let null_id = json!({"jsonrpc": "2.0", "id": null, "method": "act", "params": {}});
    let unknown_method = json!({"jsonrpc": "2.0", "id": "u1", "method": "unknown_op"});
    let ctx_both = json!({"jsonrpc": "2.0", "id": "c1", "method": "act", "params": {"context": {"state": minimal_state, "ref": "r"}}});
    let ctx_yaml_str = json!({"jsonrpc": "2.0", "id": "c2", "method": "act", "params": {"context": {"state": "kind: state"}}});
    let unknown_ref = json!({"jsonrpc": "2.0", "id": "c3", "method": "act", "params": {"context": {"ref": "missing"}}});
    let zero_seed = json!({"jsonrpc": "2.0", "id": "s0", "method": "simulate", "params": {"context": {"state": minimal_state}, "seed": 0}});
    let preview_sim = json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "simulate",
        "params": {
            "context": {"state": minimal_state},
            "until": {"tick": 0},
            "collect": {"board": "preview"}
        }
    });
    let batch_with_error = json!([
        {"jsonrpc": "2.0", "id": "b1", "method": "simulate", "params": {"context": {"state": minimal_state}, "until": {"bogus": 1}}},
        {"jsonrpc": "2.0", "id": "b2", "method": "simulate", "params": {"context": {"state": minimal_state}, "until": {"tick": 0}, "collect": {"board": "preview"}}}
    ]);

    let (replies, _stderr, status) = roundtrip(
        &[
            "",
            &init_req.to_string(),
            bad_batch,
            bad_json,
            &null_id.to_string(),
            &unknown_method.to_string(),
            &ctx_both.to_string(),
            &ctx_yaml_str.to_string(),
            &unknown_ref.to_string(),
            &zero_seed.to_string(),
            &preview_sim.to_string(),
            &batch_with_error.to_string(),
        ],
        &["--parallel", "1"],
    );
    assert!(status.success());

    let by_id = |id: Value| replies.iter().find(|r| r["id"] == id).unwrap().clone();
    let init_output = &by_id(json!(1))["result"];
    assert_eq!(init_output["protocol"], "mechcore-rpc");
    assert_eq!(init_output["parallel"], 1);
    assert_eq!(
        init_output["capabilities"]["methods"],
        json!(["convert", "act", "simulate"])
    );

    let null_errors: Vec<_> = replies.iter().filter(|r| r["id"].is_null()).collect();
    assert_eq!(null_errors.len(), 3);
    assert!(null_errors.iter().any(|r| r["error"]["code"] == -32700));
    assert_eq!(
        null_errors
            .iter()
            .filter(|r| r["error"]["code"] == -32600)
            .count(),
        2
    );

    assert_eq!(by_id(json!("u1"))["error"]["code"], -32601);
    assert_eq!(by_id(json!("c1"))["error"]["code"], -32602);
    assert_eq!(by_id(json!("c2"))["error"]["code"], -32602);
    assert_eq!(by_id(json!("c3"))["error"]["code"], -32001);
    assert_eq!(by_id(json!("s0"))["error"]["code"], -32602);

    let p = &by_id(json!(10))["result"];
    assert_eq!(p["tick"], 0);
    assert_eq!(p["settled"], false);
    assert!(
        !p["collect"]["board"]["objects"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    assert_eq!(by_id(json!("b1"))["error"]["code"], -32602);
    assert_eq!(by_id(json!("b2"))["result"]["tick"], 0);
}

#[test]
fn rpc_pipelining_cancel_and_id_reuse() {
    let minimal_state = json!({
        "kind": "state",
        "round": 1,
        "seed": 7,
        "blue": {
            "reactor_core": 4500,
            "supply": 200,
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

    let mut child = spawn_rpc(&["--parallel", "1"]);
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());

    let heavy = |id: &str| {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "simulate",
            "params": {"context": {"state": minimal_state}}
        })
    };
    let batch = Value::Array(vec![
        heavy("slot1"),
        heavy("slot2"),
        heavy("slot2"),
        json!({"jsonrpc": "2.0", "method": "$/cancel", "params": {"id": "slot2"}}),
    ]);
    writeln!(stdin, "{batch}").unwrap();
    stdin.flush().unwrap();

    let mut seen = Vec::new();
    for _ in 0..3 {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        seen.push(serde_json::from_str::<Value>(&line).unwrap());
    }
    assert!(
        seen.iter()
            .any(|r| r["id"] == "slot2" && r["error"]["code"] == -32602),
        "{seen:?}"
    );
    assert!(
        seen.iter().any(|r| r["id"] == "slot2"
            && r["error"]["code"] == -32800
            && r["error"]["data"].get("details").is_none()),
        "{seen:?}"
    );
    assert!(
        seen.iter()
            .any(|r| r["id"] == "slot1" && r["result"]["settled"] == true),
        "{seen:?}"
    );

    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc": "2.0", "id": "slot1", "method": "act", "params": {"context": {"state": minimal_state}, "actions": {"blue": []}}})
    )
    .unwrap();
    drop(stdin);

    let mut last_line = String::new();
    reader.read_line(&mut last_line).unwrap();
    let reused: Value = serde_json::from_str(&last_line).unwrap();
    assert_eq!(reused["id"], "slot1");
    assert!(reused["result"]["state"]["blue"]["shop"].is_object());
    assert!(child.wait().unwrap().success());
}

#[test]
fn rpc_convert_variants() {
    let dir = TempDir::new().unwrap();
    let match_yaml = sample_match_yaml(dir.path());

    let reqs = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "convert", "params": {"input": match_yaml, "to": "state", "round": 0}}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "convert", "params": {"input": match_yaml, "to": "state", "round": 0, "expanded": false}}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "convert", "params": {"input": match_yaml, "to": "state"}}),
        json!({"jsonrpc": "2.0", "id": 4, "method": "convert", "params": {"input": match_yaml, "to": "state", "round": 1, "apply_actions": true}}),
        json!({"jsonrpc": "2.0", "id": 5, "method": "convert", "params": {"input": "kind: layout\nround: 1\n", "to": "state"}}),
        json!({"jsonrpc": "2.0", "id": 6, "method": "convert", "params": {"input": "kind: state\nround: 1\nblue: {random_state: [1,2,3,4], reactor_core: 4500, supply: 200, unlocked_units: [], next_index: {unit: 0, contraption: 0}, units: []}\nred: {random_state: [1,2,3,4], reactor_core: 4500, supply: 200, unlocked_units: [], next_index: {unit: 0, contraption: 0}, units: []}\n", "to": "state", "round": 1}}),
        json!({"jsonrpc": "2.0", "id": 7, "method": "convert", "params": {"input": "kind: state\nround: 1\nblue: {random_state: [1,2,3,4], reactor_core: 4500, supply: 200, unlocked_units: [], next_index: {unit: 0, contraption: 0}, units: []}\nred: {random_state: [1,2,3,4], reactor_core: 4500, supply: 200, unlocked_units: [], next_index: {unit: 0, contraption: 0}, units: []}\n", "to": "state", "apply_actions": false}}),
    ];
    let lines: Vec<String> = reqs.iter().map(Value::to_string).collect();
    let payloads: Vec<&str> = lines.iter().map(String::as_str).collect();
    let (replies, _, status) = roundtrip(&payloads, &[]);
    assert!(status.success());
    let by_id = |id: i64| replies.iter().find(|r| r["id"] == id).unwrap().clone();

    let s0_exp = &by_id(1)["result"]["state"];
    assert_eq!(by_id(1)["result"]["source_kind"], "match");
    assert_eq!(s0_exp["round"], 0);
    assert!(s0_exp["reinforce_pool"].is_array());
    assert!(s0_exp["blue"]["tech_loadout"].is_object());
    assert!(s0_exp["blue"].get("name").is_none());

    let s0_cmp = &by_id(2)["result"]["state"];
    assert!(s0_cmp.get("reinforce_pool").is_none());
    assert!(s0_cmp["blue"]["tech_loadout"].is_string());

    let s_last = &by_id(3)["result"]["state"];
    assert_eq!(s_last["round"], 2);
    assert!(s_last["reinforce_offers"].is_array());
    assert!(s_last["blue"].get("tower_strengthen_levels").is_none());
    assert_eq!(s_last["blue"]["towers"].as_array().unwrap().len(), 2);
    assert!(s_last["blue"]["units"][0]["size"]["width"].is_i64());

    let s1_applied = &by_id(4)["result"]["state"];
    assert_eq!(s1_applied["round"], 1);
    assert!(s1_applied["blue"]["shop"].is_object());
    assert!(!s1_applied["blue"]["units"].as_array().unwrap().is_empty());
    assert!(s1_applied["blue"]["units"][0]["size"]["depth"].is_i64());

    assert_eq!(by_id(5)["error"]["code"], -32001);
    assert_eq!(by_id(6)["error"]["code"], -32602);
    assert_eq!(by_id(7)["error"]["code"], -32602);
}

#[test]
fn rpc_act_and_chained_simulate() {
    let dir = TempDir::new().unwrap();
    let match_yaml = sample_match_yaml(dir.path());
    let conv_line = json!({"jsonrpc": "2.0", "id": 1, "method": "convert", "params": {"input": match_yaml, "to": "state"}}).to_string();
    let (conv_replies, _, st) = roundtrip(&[&conv_line], &[]);
    assert!(st.success());
    let s_last = &conv_replies[0]["result"]["state"];

    let act_partial = json!({
        "jsonrpc": "2.0",
        "id": 20,
        "method": "act",
        "params": {
            "context": {"state": s_last},
            "actions": {"blue": [{"type": "choose_reinforce_item", "index": s_last["reinforce_offers"].as_array().unwrap().len() - 1, "name": s_last["reinforce_offers"].as_array().unwrap().last().unwrap()["name"]}]}
        }
    });
    let act_explicit_empty = json!({
        "jsonrpc": "2.0",
        "id": 21,
        "method": "act",
        "params": {
            "context": {"state": s_last},
            "actions": {"blue": [], "red": []}
        }
    });
    let act_bad_syntax = json!({
        "jsonrpc": "2.0",
        "id": 22,
        "method": "act",
        "params": {
            "context": {"state": s_last},
            "actions": {"red": [{"type": "bogus"}]}
        }
    });
    let act_one_concede = json!({
        "jsonrpc": "2.0",
        "id": 23,
        "method": "simulate",
        "params": {
            "context": {"state": s_last},
            "actions": {"blue": [{"type": "concede"}]}
        }
    });
    let (r2, _, st2) = roundtrip(
        &[
            &act_partial.to_string(),
            &act_explicit_empty.to_string(),
            &act_bad_syntax.to_string(),
            &act_one_concede.to_string(),
        ],
        &[],
    );
    assert!(st2.success());
    let find2 = |id: i64| r2.iter().find(|r| r["id"] == id).unwrap().clone();
    assert_eq!(
        find2(20)["result"]["state"]["blue"]["reinforce_chosen"],
        true
    );
    assert_eq!(
        find2(20)["result"]["state"]["red"]["reinforce_chosen"],
        false
    );
    assert_eq!(find2(21)["error"]["code"], -32001);
    assert_eq!(find2(22)["error"]["code"], -32602);
    assert_eq!(
        find2(22)["error"]["data"]["details"],
        json!({"side": "red", "action_index": 0})
    );
    assert_eq!(find2(23)["result"]["stop"], "no_fight");

    let chained_start = find2(20)["result"]["state"].clone();
    let chain_sim = json!({
        "jsonrpc": "2.0",
        "id": 30,
        "method": "act",
        "params": {
            "context": {"state": chained_start},
            "actions": {"red": [{"type": "concede"}]}
        }
    });
    let (r3, _, st3) = roundtrip(&[&chain_sim.to_string()], &[]);
    assert!(st3.success());
    let find3 = |id: i64| r3.iter().find(|r| r["id"] == id).unwrap().clone();
    assert_eq!(
        find3(30)["result"]["state"]["blue"]["reinforce_chosen"],
        true
    );
}

#[test]
fn rpc_simulate_emits_projection_warnings() {
    let cloak_state = json!({
        "kind": "state",
        "round": 5,
        "seed": 7,
        "blue": {
            "tech_loadout": {"phantom_ray": ["stealth_cloak"]},
            "reactor_core": 4500,
            "supply": 10000,
            "unlocked_units": ["phantom_ray"],
            "next_index": {"unit": 0, "contraption": 0},
            "units": []
        },
        "red": {
            "reactor_core": 4500,
            "supply": 200,
            "unlocked_units": ["marksman"],
            "next_index": {"unit": 1, "contraption": 0},
            "units": [{"name": "marksman", "index": 0, "value": 100, "position": {"x": 0, "y": -100}}]
        }
    });
    let warn_sim = json!({
        "jsonrpc": "2.0",
        "id": 31,
        "method": "simulate",
        "params": {
            "context": {"state": cloak_state},
            "actions": {
                "blue": [
                    {"type": "buy_unit", "name": "phantom_ray", "position": {"x": -105, "y": -160}},
                    {"type": "move_unit", "index": 0, "position": {"x": 320, "y": 55}},
                    {"type": "upgrade_technology", "unit": "phantom_ray", "tech": "stealth_cloak"}
                ]
            }
        }
    });
    let (replies, _, st) = roundtrip(&[&warn_sim.to_string()], &[]);
    assert!(st.success());
    assert!(replies[0]["result"]["settled"].as_bool().unwrap());
    assert!(
        !replies[0]["result"]["warnings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn rpc_rejects_second_opening_choice_terminal_state_and_null_action_list() {
    let dir = TempDir::new().unwrap();
    let match_yaml = sample_match_yaml(dir.path());
    let conv0 = json!({"jsonrpc": "2.0", "id": 1, "method": "convert", "params": {"input": match_yaml, "to": "state", "round": 0}}).to_string();
    let (conv_replies, _, st) = roundtrip(&[&conv0], &[]);
    assert!(st.success());
    let r0 = &conv_replies[0]["result"]["state"];
    let offer0 = &r0["blue"]["offers"][0];
    let blue_pick = json!({
        "type": "choose_advance_team",
        "index": 0,
        "name": offer0["team"],
        "specialist": offer0["specialist"],
    });

    // Step 1: blue picks its opening team
    let act1 = json!({"jsonrpc": "2.0", "id": 2, "method": "act", "params": {"context": {"state": r0}, "actions": {"blue": [blue_pick]}}});
    let (r1, _, st1) = roundtrip(&[&act1.to_string()], &[]);
    assert!(st1.success());
    let r0_after_blue = &r1[0]["result"]["state"];

    // Step 2: chained blue double-pick -> refused; red first pick -> ok; null blue -> -32602
    let red_off = &r0_after_blue["red"]["offers"][0];
    let red_pick = json!({
        "type": "choose_advance_team",
        "index": 0,
        "name": red_off["team"],
        "specialist": red_off["specialist"],
    });
    let dbl_pick = json!({"jsonrpc": "2.0", "id": 3, "method": "act", "params": {"context": {"state": r0_after_blue}, "actions": {"blue": [blue_pick]}}});
    let red_ok = json!({"jsonrpc": "2.0", "id": 4, "method": "act", "params": {"context": {"state": r0_after_blue}, "actions": {"red": [red_pick]}}});
    let null_side = json!({"jsonrpc": "2.0", "id": 5, "method": "act", "params": {"context": {"state": r0_after_blue}, "actions": {"blue": null}}});

    let term_state = json!({
        "kind": "state",
        "round": 1,
        "blue": {"reactor_core": 0, "supply": 200, "next_index": {"unit": 0, "contraption": 0}, "units": []},
        "red": {"reactor_core": 4500, "supply": 200, "next_index": {"unit": 0, "contraption": 0}, "units": []}
    });
    let term_act = json!({"jsonrpc": "2.0", "id": 6, "method": "act", "params": {"context": {"state": term_state}, "actions": {"blue": []}}});
    let term_sim = json!({"jsonrpc": "2.0", "id": 7, "method": "simulate", "params": {"context": {"state": term_state}}});

    let lines: Vec<String> = [dbl_pick, red_ok, null_side, term_act, term_sim]
        .iter()
        .map(Value::to_string)
        .collect();
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let (r2, _, st2) = roundtrip(&refs, &[]);
    assert!(st2.success());
    let by_id = |id: i64| r2.iter().find(|r| r["id"] == id).unwrap().clone();
    assert_eq!(by_id(3)["error"]["code"], -32001);
    assert!(
        by_id(4)["result"]["state"]["red"].get("shop").is_some(),
        "{}",
        by_id(4)
    );
    assert_eq!(by_id(5)["error"]["code"], -32602);
    assert_eq!(by_id(6)["error"]["code"], -32001);
    assert_eq!(by_id(7)["error"]["code"], -32001);
}

#[test]
fn rpc_act_strengthen_tower_matches_when_tower_levels_omitted_or_expanded() {
    let state = json!({
        "kind": "state",
        "round": 1,
        "seed": 7,
        "blue": {
            "reactor_core": 4500,
            "supply": 400,
            "next_index": {"unit": 0, "contraption": 0},
            "units": []
        },
        "red": {
            "reactor_core": 4500,
            "supply": 400,
            "next_index": {"unit": 0, "contraption": 0},
            "units": []
        }
    });
    let state_yaml = format!(
        "kind: state\nround: 1\nseed: 7\nblue: {}\nred: {}\n",
        state["blue"], state["red"]
    );

    let conv = json!({"jsonrpc": "2.0", "id": 1, "method": "convert", "params": {"input": state_yaml, "to": "state"}});
    let act_compact = json!({"jsonrpc": "2.0", "id": 2, "method": "act", "params": {"context": {"state": state}, "actions": {"blue": [{"type": "strengthen_tower", "tower": 0}]}}});
    let (r1, _, st1) = roundtrip(&[&conv.to_string(), &act_compact.to_string()], &[]);
    assert!(st1.success());
    let find1 = |id: i64| r1.iter().find(|r| r["id"] == id).unwrap().clone();
    let expanded_state = find1(1)["result"]["state"].clone();
    assert!(expanded_state.is_object(), "convert failed: {}", find1(1));
    let act_expanded = json!({"jsonrpc": "2.0", "id": 3, "method": "act", "params": {"context": {"state": expanded_state}, "actions": {"blue": [{"type": "strengthen_tower", "tower": 0}]}}});
    let (r2, _, st2) = roundtrip(&[&act_expanded.to_string()], &[]);
    assert!(st2.success());
    let resp_compact = find1(2);
    let resp_expanded = r2.iter().find(|r| r["id"] == 3).unwrap().clone();
    assert!(
        resp_compact.get("result").is_some(),
        "compact act failed: {resp_compact}"
    );
    assert!(
        resp_expanded.get("result").is_some(),
        "expanded act failed: {resp_expanded} from {expanded_state}"
    );

    assert_eq!(
        resp_compact["result"]["state"]["blue"]["towers"],
        resp_expanded["result"]["state"]["blue"]["towers"]
    );
    assert_eq!(
        resp_compact["result"]["state"]["blue"]["towers"][0]["level"],
        1
    );
    assert_eq!(
        resp_compact["result"]["state"]["blue"]["towers"][1]["level"],
        0
    );
}
