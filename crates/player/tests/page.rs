//! The demo scene, fought by the simulator and laid out for the page: what
//! the page reads back must be what the recording holds.

use std::{collections::BTreeMap, path::PathBuf, sync::OnceLock};

use mechcore_mcfr::{MemoryRecording, Recording};
use mechcore_player::{Cue, Step, Timeline, Track};
use mechcore_simulation::{Record, simulate_layout};

const SIX: [&str; 6] = [
    "marksman",
    "arclight",
    "rhino",
    "crawler",
    "sledgehammer",
    "wasp",
];

fn fought() -> &'static (MemoryRecording, Timeline) {
    static FOUGHT: OnceLock<(MemoryRecording, Timeline)> = OnceLock::new();
    FOUGHT.get_or_init(|| {
        let scene = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenes/six-units.yaml");
        let recording = simulate_layout(scene, Record::Memory, Some(7))
            .unwrap()
            .recording
            .unwrap();
        let timeline = mechcore_player::timeline(&recording).unwrap();
        (recording, timeline)
    })
}

/// A track as the page reads it: through its JSON.
fn read_back(track: &Track) -> Vec<i64> {
    let written = serde_json::to_value(track).unwrap();
    let steps: Vec<Step> = written
        .as_array()
        .unwrap()
        .iter()
        .map(|step| match step {
            serde_json::Value::String(run) => Step::Hold(run.parse().unwrap()),
            number => Step::Change(number.as_i64().unwrap()),
        })
        .collect();
    Track::from_steps(&steps).values().to_vec()
}

fn centimetres(raw: i64) -> i64 {
    i64::try_from((i128::from(raw) * 100 + (1 << 31)) >> 32).unwrap()
}

#[test]
fn every_unit_reads_back_tick_for_tick() {
    let (recording, timeline) = fought();
    let units: BTreeMap<u64, _> = timeline.units.iter().map(|unit| (unit.id, unit)).collect();
    let tracks: BTreeMap<u64, _> = units
        .iter()
        .map(|(&id, unit)| {
            (
                id,
                (
                    read_back(&unit.x),
                    read_back(&unit.z),
                    read_back(&unit.life),
                ),
            )
        })
        .collect();
    for unit in &timeline.units {
        let ticks = usize::try_from(unit.to - unit.from + 1).unwrap();
        assert_eq!(tracks[&unit.id].0.len(), ticks, "unit {}", unit.id);
    }
    for tick in 1..=recording.terminal_tick() {
        for state in recording.state(tick).unwrap().live_units {
            let unit = units[&state.unit_id];
            let at = usize::try_from(tick - unit.from).unwrap();
            let (x, z, life) = &tracks[&state.unit_id];
            assert_eq!(
                x[at],
                centimetres(state.position.x),
                "unit {} x at {tick}",
                unit.id
            );
            assert_eq!(
                z[at],
                centimetres(state.position.z),
                "unit {} z at {tick}",
                unit.id
            );
            assert_eq!(life[at], i64::from(state.life.current));
        }
    }
}

#[test]
fn the_scene_holds_six_kinds_a_side_and_the_map_and_constructions() {
    let (_, timeline) = fought();
    for team in [0, 1] {
        for kind in SIX {
            assert!(
                timeline
                    .units
                    .iter()
                    .any(|unit| unit.team == team && unit.kind == kind),
                "team {team} has no {kind}"
            );
        }
        let mut buildings: BTreeMap<&str, usize> = BTreeMap::new();
        for building in timeline.buildings.iter().filter(|b| b.team == team) {
            *buildings.entry(building.kind.as_str()).or_default() += 1;
        }
        assert_eq!(
            buildings,
            BTreeMap::from([
                ("anti_armor_turret", 1),
                ("defensive_wall", 5),
                ("energy_tower", 1),
                ("rapid_fire_turret", 1),
                ("research_center", 1),
            ]),
            "team {team}"
        );
        assert!(
            timeline
                .shields
                .iter()
                .any(|shield| shield.team == team && shield.source == "contraption")
        );
    }
    assert!(
        timeline
            .units
            .iter()
            .filter(|unit| unit.kind == "wasp")
            .all(|unit| unit.air)
    );
}

#[test]
fn every_shot_and_blow_names_who_dealt_it() {
    let (_, timeline) = fought();
    let kind_of: BTreeMap<String, &str> = timeline
        .units
        .iter()
        .map(|unit| (format!("u{}", unit.id), unit.kind.as_str()))
        .chain(
            timeline
                .buildings
                .iter()
                .map(|building| (format!("b{}", building.id), building.kind.as_str())),
        )
        .collect();
    let name = |reference: &mechcore_player::Ref| serde_json::to_value(reference).unwrap();
    let mut fired = BTreeMap::new();
    let mut struck = BTreeMap::new();
    for cue in &timeline.cues {
        match cue {
            Cue::Fire { p, by, .. } => {
                let by = name(by.as_ref().expect("a shot has a source"));
                fired.insert(*p, kind_of[by.as_str().unwrap()]);
            }
            Cue::Hit {
                by: Some(by),
                p: None,
                ..
            } => {
                if let Some(&kind) = kind_of.get(name(by).as_str().unwrap()) {
                    *struck.entry(kind).or_insert(0) += 1;
                }
            }
            _ => {}
        }
    }
    for projectile in &timeline.projectiles {
        assert!(
            fired.contains_key(&projectile.id),
            "projectile {} has no shot",
            projectile.id
        );
    }
    for kind in ["marksman", "arclight", "sledgehammer", "wasp"] {
        assert!(fired.values().any(|&by| by == kind), "no {kind} fired");
    }
    for kind in ["rhino", "crawler"] {
        assert!(struck.contains_key(kind), "no {kind} struck");
    }
}

#[test]
fn the_page_carries_the_timeline_whole() {
    let (_, timeline) = fought();
    let page = mechcore_player::page(timeline, "six <units>").unwrap();
    assert!(page.starts_with("<!doctype html>"));
    assert!(page.contains("<title>six &lt;units&gt; · mechcore player</title>"));
    for placeholder in ["{{", "/*{{"] {
        assert!(
            !page.contains(placeholder),
            "{placeholder} left in the page"
        );
    }
    let open = r#"<script id="timeline" type="application/json">"#;
    let start = page.find(open).unwrap() + open.len();
    let end = start + page[start..].find("</script>").unwrap();
    let embedded: serde_json::Value = serde_json::from_str(&page[start..end]).unwrap();
    assert_eq!(embedded, serde_json::to_value(timeline).unwrap());
    assert_eq!(embedded["schema"], mechcore_player::SCHEMA);
}
