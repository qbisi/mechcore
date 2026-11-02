//! Geometry queries admit empty deployment documents and refuse other kinds.
use serde_json::{Value, json};
use std::{
    fs,
    process::{Command, Output},
};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(args)
        .output()
        .unwrap()
}
fn answer(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn layout_and_state_answer_the_same_geometry_without_spending_supply() {
    let dir = tempfile::tempdir().unwrap();
    let layout = dir.path().join("input.yaml");
    fs::write(
        &layout,
        "kind: layout\nround: 1\nblue: {units: []}\nred: {units: []}\n",
    )
    .unwrap();
    let mask = answer(&run(&[
        "mask",
        layout.to_str().unwrap(),
        "--side",
        "blue",
        "--unit",
        "rhino",
    ]));
    assert_eq!(mask["schema"], "mechcore.mask");
    assert!(
        mask["grids"]
            .as_array()
            .unwrap()
            .iter()
            .all(|g| g["region"] == "main")
    );
    let side = mechcore_document::r#match::SideState::default();
    let state = dir.path().join("state.json");
    fs::write(
        &state,
        json!({"kind":"state","map_id":1001,"seed":17,"round":1,"random_state":[1,2,3,4],"blue":side,"red":side})
            .to_string(),
    )
    .unwrap();
    assert_eq!(
        mask,
        answer(&run(&[
            "mask",
            state.to_str().unwrap(),
            "--side",
            "blue",
            "--unit",
            "rhino"
        ]))
    );
    let yaml = run(&[
        "mask",
        layout.to_str().unwrap(),
        "--side",
        "red",
        "--unit",
        "rhino",
        "--format",
        "yaml",
    ]);
    assert!(yaml.status.success());
    assert_eq!(
        serde_yaml::from_slice::<Value>(&yaml.stdout).unwrap()["side"],
        "red"
    );
    assert_eq!(
        run(&["mask", layout.to_str().unwrap(), "--unit", "rhino"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        run(&[
            "mask",
            layout.to_str().unwrap(),
            "--side",
            "blue",
            "--unit",
            "missing"
        ])
        .status
        .code(),
        Some(3)
    );
    fs::write(&state, "kind: action\nround: 1\nblue: []\nred: []\n").unwrap();
    assert_eq!(
        run(&[
            "mask",
            state.to_str().unwrap(),
            "--side",
            "blue",
            "--unit",
            "rhino"
        ])
        .status
        .code(),
        Some(3)
    );
}

#[test]
fn standalone_state_reads_unit_reinforcements_in_its_own_round() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state.yaml");
    let side = mechcore_document::r#match::SideState::default();
    let input = json!({"kind":"state","round":2,"random_state":[1,2,3,4],
        "reinforce_offers":["rhino_1x_lv1","refund_200"],"blue":side,"red":side});
    fs::write(&state, input.to_string()).unwrap();
    let result = answer(&run(&[
        "mask",
        state.to_str().unwrap(),
        "--side",
        "red",
        "--unit",
        "steel_ball",
    ]));
    assert!(
        result["grids"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["region"] == "left_flank")
    );
}
