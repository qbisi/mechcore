//! `play` writes the page that plays a fight, from each kind it takes.

use std::{path::PathBuf, process::Command};

fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn mechcore(arguments: &[&std::ffi::OsStr]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(arguments)
        .output()
        .unwrap()
}

fn played(arguments: &[&std::ffi::OsStr]) -> serde_json::Value {
    let output = mechcore(arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// The page holds the timeline the player lays out, and nothing left unfilled.
fn assert_a_page(page: &std::path::Path, ticks: u64) {
    let html = std::fs::read_to_string(page).unwrap();
    assert!(html.starts_with("<!doctype html>"));
    assert!(html.contains(r#""schema":"mechcore.player.v1""#));
    assert!(html.contains(&format!(r#""ticks":{ticks},"#)));
    assert!(!html.contains("{{"));
}

/// The timeline a page carries.
fn timeline(page: &std::path::Path) -> serde_json::Value {
    let html = std::fs::read_to_string(page).unwrap();
    let open = r#"<script id="timeline" type="application/json">"#;
    let start = html.find(open).unwrap() + open.len();
    let end = start + html[start..].find("</script>").unwrap();
    serde_json::from_str(&html[start..end]).unwrap()
}

#[test]
fn a_layout_is_fought_in_memory_and_played() {
    let directory = tempfile::tempdir().unwrap();
    let page = directory.path().join("six.html");
    let layout = repository().join("layouts/marksman-vs-arclight.yaml");
    let report = played(&[
        "play".as_ref(),
        layout.as_os_str(),
        page.as_os_str(),
        "--seed".as_ref(),
        "7".as_ref(),
        "--no-open".as_ref(),
    ]);
    assert_eq!(report["schema"], "mechcore.play-result.v2");
    assert_eq!(report["opened"], false);
    assert_eq!(report["kind"], "layout");
    assert_eq!(report["producer"], "simulator");
    assert_eq!(report["seed"], 7);
    assert_eq!(report["seed_source"], "external");
    assert_eq!(report["page"], page.to_str().unwrap());
    assert_a_page(&page, report["ticks"].as_u64().unwrap());
    // Nothing but the page is written: the fight was kept in memory.
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn a_fight_and_its_recording_play_the_same_fight() {
    let directory = tempfile::tempdir().unwrap();
    let fight = repository().join("tests/regression/fights/marksman-vs-arclight.yaml");
    let from_fight = directory.path().join("fight.html");
    let report = played(&[
        "play".as_ref(),
        fight.as_os_str(),
        from_fight.as_os_str(),
        "--no-open".as_ref(),
    ]);
    assert_eq!(report["kind"], "fight");
    assert_eq!(report["seed_source"], "layout");
    let ticks = report["ticks"].as_u64().unwrap();
    assert_a_page(&from_fight, ticks);

    let recording = directory.path().join("fight.mcfr");
    let converted = mechcore(&[
        "convert".as_ref(),
        fight.as_os_str(),
        "--to".as_ref(),
        "mcfr".as_ref(),
        recording.as_os_str(),
    ]);
    assert!(converted.status.success());
    // Without a page named, the page is written beside the recording.
    let report = played(&["play".as_ref(), recording.as_os_str(), "--no-open".as_ref()]);
    assert_eq!(report["kind"], "mcfr");
    assert_eq!(report["ticks"].as_u64().unwrap(), ticks);
    assert!(report.get("seed").is_none());
    let beside = recording.with_extension("html");
    assert_eq!(report["page"], beside.to_str().unwrap());
    assert_eq!(
        timeline(&beside),
        timeline(&from_fight),
        "a fight and the recording of it lay out the same timeline",
    );

    let refused = mechcore(&[
        "play".as_ref(),
        recording.as_os_str(),
        "--seed".as_ref(),
        "3".as_ref(),
    ]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("--seed belongs to a layout"));
}

#[test]
fn play_refuses_a_kind_with_no_fight() {
    let directory = tempfile::tempdir().unwrap();
    let state = directory.path().join("state.yaml");
    std::fs::write(&state, "kind: state\n").unwrap();
    let output = mechcore(&["play".as_ref(), state.as_os_str()]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("play does not take a state file"));
}
