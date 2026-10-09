use std::{path::Path, path::PathBuf, process::Command};

use mechcore_mcfr::McfrReader;

fn recording(directory: &Path) -> PathBuf {
    recording_with_seed(directory, 7)
}

fn recording_with_seed(directory: &Path, seed: u32) -> PathBuf {
    let output = directory.join(format!("fight-{seed}.mcfr"));
    let layout =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../layouts/marksman-vs-arclight.yaml");
    let command = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .args(["convert", "--to", "mcfr"])
        .arg(layout)
        .args(["--seed", &seed.to_string()])
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
    queries(&[recording.as_os_str()], arguments)
}

fn queries(recordings: &[&std::ffi::OsStr], arguments: &[&str]) -> (i32, serde_json::Value) {
    let cache = tempfile::tempdir().unwrap();
    queries_in(cache.path(), recordings, arguments)
}

fn queries_in(
    cache: &Path,
    recordings: &[&std::ffi::OsStr],
    arguments: &[&str],
) -> (i32, serde_json::Value) {
    let command = Command::new(env!("CARGO_BIN_EXE_mechcore"))
        .env("MECHCORE_QUERY_CACHE", cache)
        .arg("query")
        .args(recordings)
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
            .all(|table| ["hashed", "layout", "view"].contains(&table["origin"].as_str().unwrap()))
    );
    assert!(tables.iter().any(|table| table["name"] == "layout_units"));
    assert_eq!(schema["queries"].as_array().unwrap().len(), 7);
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

#[test]
fn two_recordings_are_left_and_right() {
    let directory = tempfile::tempdir().unwrap();
    let seven = recording_with_seed(directory.path(), 7);
    let eight = recording_with_seed(directory.path(), 8);
    let (code, same) = queries(
        &[seven.as_os_str(), seven.as_os_str()],
        &["--query", "divergence"],
    );
    assert_eq!(code, 0, "{same}");
    assert!(same["rows"].as_array().unwrap().is_empty());
    let (code, differ) = queries(
        &[seven.as_os_str(), eight.as_os_str()],
        &["--query", "divergence"],
    );
    assert_eq!(code, 0, "{differ}");
    let rows = differ["rows"].as_array().unwrap();
    let first = rows
        .iter()
        .find(|row| row[0] == "ticks")
        .expect("seeds 7 and 8 fight differently");
    assert_eq!(
        first[1],
        single_of(
            &[seven.as_os_str(), eight.as_os_str()],
            "SELECT min(l.tick) FROM left.ticks l JOIN right.ticks r USING (tick) \
             WHERE l.tick_hash <> r.tick_hash"
        )
    );
    let (code, units) = queries(
        &[seven.as_os_str(), eight.as_os_str()],
        &[
            "--query",
            "units-diff",
            "--param",
            &format!("tick={}", first[1]),
        ],
    );
    assert_eq!(code, 0, "{units}");
}

#[test]
fn recordings_take_the_names_they_are_given() {
    let directory = tempfile::tempdir().unwrap();
    let path = recording(directory.path());
    let a = format!("a={}", path.display());
    let b = format!("b={}", path.display());
    let c = format!("c={}", path.display());
    assert_eq!(
        single_of(
            &[a.as_ref(), b.as_ref(), c.as_ref()],
            "SELECT (SELECT count(*) FROM a.units) = (SELECT count(*) FROM c.units)"
        ),
        1
    );
    let (code, schema) = queries(&[a.as_ref(), b.as_ref()], &["--schema"]);
    assert_eq!(code, 0, "{schema}");
    assert!(
        schema["tables"]
            .as_array()
            .unwrap()
            .iter()
            .any(|table| table["database"] == "b" && table["name"] == "units")
    );
    let (code, _) = queries(&[a.as_ref(), a.as_ref()], &["--schema"]);
    assert_eq!(code, 2);
}

fn single_of(recordings: &[&std::ffi::OsStr], sql: &str) -> serde_json::Value {
    let (code, answer) = queries(recordings, &["--sql", sql]);
    assert_eq!(code, 0, "{answer}");
    answer["rows"][0][0].clone()
}

#[test]
fn every_event_is_a_row_of_its_kinds_view() {
    let directory = tempfile::tempdir().unwrap();
    let path = recording(directory.path());
    let (code, schema) = query(&path, &["--schema"]);
    assert_eq!(code, 0, "{schema}");
    let views = schema["tables"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|table| table["origin"] == "view")
        .map(|table| table["name"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(views.len(), 16);
    let total = views
        .iter()
        .map(|view| format!("(SELECT count(*) FROM {view})"))
        .collect::<Vec<_>>()
        .join(" + ");
    assert_eq!(
        single(&path, &format!("SELECT {total}")),
        single(&path, "SELECT count(*) FROM events")
    );
    let damage = schema["tables"]
        .as_array()
        .unwrap()
        .iter()
        .find(|table| table["name"] == "ev_damage")
        .unwrap()["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|column| column["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(damage.contains(&"amount") && !damage.contains(&"buff_id"));
}

#[test]
fn a_statement_reads_from_a_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = recording(directory.path());
    let sql = directory.path().join("count.sql");
    std::fs::write(&sql, "-- units\nSELECT count(*) FROM units\n").unwrap();
    let (code, answer) = query(&path, &["--sql-file", sql.to_str().unwrap()]);
    assert_eq!(code, 0, "{answer}");
    assert_eq!(
        answer["rows"][0][0],
        single(&path, "SELECT count(*) FROM units")
    );
    let (code, _) = query(
        &path,
        &["--sql-file", sql.to_str().unwrap(), "--sql", "SELECT 1"],
    );
    assert_eq!(code, 2);
}

#[test]
fn a_later_query_reads_what_an_earlier_one_cached() {
    let directory = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let path = recording(directory.path());
    let moved = directory.path().join("moved.mcfr");
    std::fs::copy(&path, &moved).unwrap();
    let count = "SELECT count(*) FROM units__skills";
    let (code, first) = queries_in(cache.path(), &[path.as_os_str()], &["--sql", count]);
    assert_eq!(code, 0, "{first}");
    let files = std::fs::read_dir(cache.path()).unwrap().count();
    assert!(files >= 1);
    // The same content elsewhere opens the same file, which holds the table
    // filled, and records where it now lies.
    let (code, filled) = queries_in(
        cache.path(),
        &[moved.as_os_str()],
        &["--sql", "SELECT name FROM mechcore_filled ORDER BY name"],
    );
    assert_eq!(code, 0, "{filled}");
    assert!(
        filled["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row[0] == "units__skills")
    );
    let (code, second) = queries_in(cache.path(), &[moved.as_os_str()], &["--sql", count]);
    assert_eq!(code, 0, "{second}");
    assert_eq!(first["rows"], second["rows"]);
    let (code, at) = queries_in(
        cache.path(),
        &[moved.as_os_str()],
        &["--sql", "SELECT value FROM meta WHERE key = 'path'"],
    );
    assert_eq!(code, 0, "{at}");
    assert_eq!(at["rows"][0][0], moved.to_str().unwrap());
    // The same recording read twice at once fills each table once.
    let (code, both) = queries_in(
        cache.path(),
        &[path.as_os_str(), moved.as_os_str()],
        &[
            "--sql",
            "SELECT (SELECT count(*) FROM left.units) = (SELECT count(*) FROM right.units)",
        ],
    );
    assert_eq!(code, 0, "{both}");
    assert_eq!(both["rows"][0][0], 1);
    let (code, memory) = queries_in(
        cache.path(),
        &[path.as_os_str()],
        &["--sql", count, "--no-cache"],
    );
    assert_eq!(code, 0, "{memory}");
    assert_eq!(memory["rows"], first["rows"]);
}
