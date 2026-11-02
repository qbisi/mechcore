//! Preview retains the input and initializes observations without simulation.
use serde_json::{Value, json};
use std::{fs, process::Command};

#[test]
fn previews_empty_states_and_layouts_with_explicit_seed_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("state.yaml");
    let side = mechcore_document::r#match::SideState::default();
    let input =
        json!({"kind":"state","round":1,"seed":17,"random_state":[1,2,3,4],"blue":side,"red":side});
    fs::write(&file, input.to_string()).unwrap();
    let before = fs::read(&file).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(["preview", file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(result["schema"], "mechcore.preview");
    assert_eq!(result["document"], input);
    assert_eq!(result["seed"], 17);
    assert_eq!(result["tick"], 0);
    assert_eq!(fs::read(&file).unwrap(), before);
    assert!(!file.with_extension("turn").exists());
    let overridden = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args([
            "preview",
            file.to_str().unwrap(),
            "--seed",
            "42",
            "--format",
            "yaml",
        ])
        .output()
        .unwrap();
    assert!(overridden.status.success());
    let value: Value = serde_yaml::from_slice(&overridden.stdout).unwrap();
    assert_eq!(value["seed"], 42);
    assert_eq!(value["seed_source"], "external");
    fs::write(&file, "kind: action\nround: 1\nblue: []\nred: []\n").unwrap();
    let refused = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(["preview", file.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(refused.status.code(), Some(3));
}
