use std::{path::Path, path::PathBuf, process::Command};

use mechcore_mcfr::McfrReader;

fn recording(directory: &Path) -> PathBuf {
    let output = directory.join("fight.mcfr");
    let layout =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../layouts/marksman-vs-arclight.yaml");
    let command = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(["convert", "--to", "mcfr"])
        .arg(layout)
        .args(["--seed", "7"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        command.status.success(),
        "{}",
        String::from_utf8_lossy(&command.stderr)
    );
    output
}

fn query(recording: &Path, arguments: &[&str]) -> (i32, serde_json::Value) {
    let command = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .arg("query")
        .arg(recording)
        .args(arguments)
        .output()
        .unwrap();
    let code = command.status.code().unwrap();
    let stream = if code == 0 {
        &command.stdout
    } else {
        &command.stderr
    };
    (code, serde_json::from_slice(stream).unwrap())
}

fn single(recording: &Path, sql: &str) -> serde_json::Value {
    let (code, answer) = query(recording, &["--sql", sql]);
    assert_eq!(code, 0, "{answer}");
    answer["rows"][0][0].clone()
}

#[test]
fn every_row_and_element_of_a_recording_is_a_row_of_its_tables() {
    let directory = tempfile::tempdir().unwrap();
    let path = recording(directory.path());
    let reader = McfrReader::open(&path).unwrap();
    let (mut units, mut buffs, mut skills, mut events) = (0, 0, 0, 0);
    for tick in 1..=reader.tick_count() {
        let state = reader.state(tick).unwrap();
        units += state.live_units.len();
        buffs += state
            .live_units
            .iter()
            .map(|unit| unit.buffs.len())
            .sum::<usize>();
        skills += state
            .live_units
            .iter()
            .map(|unit| unit.skills.len())
            .sum::<usize>();
        events += reader.events(tick).unwrap().events.len();
    }
    assert_eq!(single(&path, "SELECT count(*) FROM units"), units);
    assert_eq!(single(&path, "SELECT count(*) FROM units__buffs"), buffs);
    assert_eq!(single(&path, "SELECT count(*) FROM units__skills"), skills);
    assert_eq!(single(&path, "SELECT count(*) FROM events"), events);
    assert_eq!(
        single(&path, "SELECT count(*) FROM ticks"),
        reader.tick_count()
    );
    // A table the recording leaves out has no rows.
    assert_eq!(single(&path, "SELECT count(*) FROM shields"), 0);
}

#[test]
fn a_recording_says_who_made_it_and_names_its_tags() {
    let directory = tempfile::tempdir().unwrap();
    let path = recording(directory.path());
    assert_eq!(single(&path, "SELECT producer FROM fight"), "simulator");
    assert_eq!(
        single(
            &path,
            "SELECT count(*) FROM events WHERE type NOT IN \
             (SELECT DISTINCT type FROM events WHERE type GLOB '[a-z]*')"
        ),
        0
    );
    assert_eq!(
        single(
            &path,
            "SELECT count(*) FROM units WHERE motion_state NOT GLOB '[a-z]*'"
        ),
        0
    );
}

#[test]
fn a_named_query_takes_its_parameters() {
    let directory = tempfile::tempdir().unwrap();
    let path = recording(directory.path());
    let (code, answer) = query(
        &path,
        &[
            "--query",
            "events-window",
            "--param",
            "from=1",
            "--param",
            "to=20",
        ],
    );
    assert_eq!(code, 0, "{answer}");
    assert_eq!(answer["schema"], "mechcore.query.v1");
    assert_eq!(answer["columns"][0], "tick");
    let (code, answer) = query(&path, &["--query", "events-window", "--param", "from=1"]);
    assert_eq!(code, 2, "{answer}");
    let (code, answer) = query(&path, &["--query", "kills-by-formation"]);
    assert_eq!(code, 0, "{answer}");
    assert!(!answer["rows"].as_array().unwrap().is_empty());
    let (code, answer) = query(&path, &["--query", "death-timeline"]);
    assert_eq!(code, 0, "{answer}");
}

#[test]
fn the_schema_names_every_table_and_query() {
    let directory = tempfile::tempdir().unwrap();
    let path = recording(directory.path());
    let (code, schema) = query(&path, &["--schema"]);
    assert_eq!(code, 0, "{schema}");
    let tables = schema["tables"].as_array().unwrap();
    let weapons = tables
        .iter()
        .find(|table| table["name"] == "units__skills__enabled__weapons")
        .unwrap();
    assert_eq!(
        weapons["key"],
        serde_json::json!(["tick", "unit_id", "skills_ordinal", "ordinal"])
    );
    assert!(
        tables
            .iter()
            .all(|table| table["origin"] == "hashed" || table["origin"] == "layout")
    );
    assert!(tables.iter().any(|table| table["name"] == "layout_units"));
    assert_eq!(schema["queries"].as_array().unwrap().len(), 5);
}

#[test]
fn every_placed_formation_is_named_by_its_placement() {
    let directory = tempfile::tempdir().unwrap();
    let path = recording(directory.path());
    let (code, answer) = query(
        &path,
        &[
            "--sql",
            "SELECT l.side, l.placement, l.name, count(DISTINCT u.unit_id) \
             FROM layout_units l JOIN units u ON u.formation_id = l.formation_id AND u.tick = 1 \
             GROUP BY l.side, l.placement ORDER BY l.side, l.placement",
        ],
    );
    assert_eq!(code, 0, "{answer}");
    let rows = answer["rows"].as_array().unwrap();
    assert_eq!(
        rows.len() as u64,
        single(&path, "SELECT count(*) FROM layout_units")
            .as_u64()
            .unwrap()
    );
    assert!(rows.iter().all(|row| row[3].as_u64().unwrap() > 0));
    assert_eq!(
        single(
            &path,
            "SELECT count(*) FROM layout_units WHERE formation_id IS NULL"
        ),
        0
    );
}

#[test]
fn travel_and_a_unit_at_a_tick_are_answered() {
    let directory = tempfile::tempdir().unwrap();
    let path = recording(directory.path());
    let (code, travel) = query(&path, &["--query", "travel-distance"]);
    assert_eq!(code, 0, "{travel}");
    assert_eq!(
        travel["rows"].as_array().unwrap().len() as u64,
        single(&path, "SELECT count(DISTINCT unit_id) FROM units")
            .as_u64()
            .unwrap()
    );
    let unit = single(&path, "SELECT min(unit_id) FROM units WHERE tick = 1");
    let (code, at) = query(
        &path,
        &[
            "--query",
            "unit-at",
            "--param",
            &format!("unit={unit}"),
            "--param",
            "tick=1",
        ],
    );
    assert_eq!(code, 0, "{at}");
    let rows = at["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    let skills: serde_json::Value = serde_json::from_str(
        rows[0]
            .as_array()
            .unwrap()
            .last()
            .unwrap()
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert!(!skills.as_array().unwrap().is_empty());
}
