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
        let timeline = mechcore_player::timeline(&recording, None).unwrap();
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

/// Each side's three deployment regions, in the world's frame: blue's own
/// half below the centre line and its flanks beside red's, red's turned half
/// a turn; the field frames all six.
#[test]
fn the_timeline_carries_each_sides_deployment_regions() {
    let (_, timeline) = fought();
    let rects: Vec<_> = timeline
        .regions
        .iter()
        .map(|r| (r.team, r.flank, r.x0, r.z0, r.x1, r.z1))
        .collect();
    assert_eq!(
        rects,
        [
            (0, false, -30_000, -31_000, 30_000, -1_000),
            (0, true, -36_000, 1_000, -30_000, 31_000),
            (0, true, 30_000, 1_000, 36_000, 31_000),
            (1, false, -30_000, 1_000, 30_000, 31_000),
            (1, true, 30_000, -31_000, 36_000, -1_000),
            (1, true, -36_000, -31_000, -30_000, -1_000),
        ]
    );
    assert!(timeline.field.half_width >= 36_000);
    assert!(timeline.field.half_depth >= 31_000);
}

/// A recording's poses ride on the units they pose: each unit's base layer
/// clip, numbered in the clip table, and its normalized time in thousandths,
/// a tick with no pose holding -1.
#[test]
fn a_unit_carries_its_recorded_pose() {
    use mechcore_mcfr::{ObjectKind, ObjectRef, PoseClip, UnitPose};
    let (recording, _) = fought();
    let pose = |tick: u32, layer: u8, clip: &str, time: f32| {
        (
            tick,
            UnitPose {
                unit: ObjectRef::new(ObjectKind::Unit, 1),
                layer,
                layer_name: if layer == 0 {
                    "Base Layer"
                } else {
                    "AttackMove"
                }
                .into(),
                layer_weight: 1.0,
                state: 0,
                state_name: 0,
                normalized_time: time,
                state_length: 1.0,
                state_speed: 1.0,
                next_state: None,
                clips: vec![PoseClip {
                    name: clip.into(),
                    weight: 1.0,
                }],
                animator_speed: 1.0,
            },
        )
    };
    let rows = [
        pose(1, 0, "Longbow_BattleIdle", 0.0),
        pose(1, 1, "Longbow_Walk", 0.5),
        pose(2, 0, "Longbow_BattleIdle", 0.033),
        pose(3, 0, "Longbow_Walk", 1.25),
    ];
    let timeline = mechcore_player::timeline(recording, Some(&rows)).unwrap();
    assert_eq!(timeline.clips, ["Longbow_BattleIdle", "Longbow_Walk"]);
    let unit = timeline.units.iter().find(|unit| unit.id == 1).unwrap();
    let pose = unit.pose.as_ref().unwrap();
    assert_eq!(&read_back(&pose.clip)[..4], [0, 0, 1, -1]);
    assert_eq!(&read_back(&pose.time)[..4], [0, 33, 1250, 0]);
    // A unit no row poses still carries the track, at -1 throughout.
    let other = timeline.units.iter().find(|unit| unit.id == 2).unwrap();
    assert!(
        read_back(&other.pose.as_ref().unwrap().clip)
            .iter()
            .all(|&clip| clip == -1)
    );
}

/// Oil ignites while another patch is cut by a standing shield. Shapes must
/// survive timeline compression exactly, including their first/last tick.
#[test]
fn terrain_shapes_read_back_tick_for_tick() {
    let layout = br"
kind: layout
seed: 4242
round: 2
blue:
  units:
  - {name: marksman, index: 0, position: {x: -20, y: -150}}
  - {name: marksman, index: 1, position: {x: 20, y: -150}}
  battle_skills:
  - {name: shield_airdrop, standing: {position: {x: -80, y: 0}}}
  - {name: sticky_oil_bomb, positions: [{x: -60, y: -40}, {x: 60, y: -40}]}
  - {name: incendiary_bomb, positions: [{x: 60, y: -160}, {x: 60, y: -40}]}
red:
  units:
  - {name: sledgehammer, index: 0, position: {x: 5, y: -60}}
  - {name: crawler, index: 1, position: {x: 5, y: -110}}
";
    let recording = mechcore_simulation::simulate_document(layout, Record::Memory, None)
        .unwrap()
        .recording
        .unwrap();
    let timeline = mechcore_player::timeline(&recording, None).unwrap();
    let mut kinds = std::collections::BTreeSet::new();
    let mut saw_grid = false;
    for tick in 1..=recording.terminal_tick() {
        let expected = recording.state(tick).unwrap().terrains;
        let actual: BTreeMap<_, _> = timeline
            .terrains
            .iter()
            .filter_map(|t| {
                t.frames
                    .iter()
                    .find(|f| f.from <= tick && tick <= f.to)
                    .map(|f| (t.id, &f.shape))
            })
            .collect();
        assert_eq!(actual.len(), expected.len(), "terrain count at {tick}");
        for state in expected {
            let shape = actual[&state.terrain_id];
            assert_eq!(shape.kind, state.terrain_type);
            assert_eq!(shape.x, centimetres(state.position.x));
            assert_eq!(shape.z, centimetres(state.position.z));
            assert_eq!(shape.radius, centimetres(state.radius));
            assert_eq!(shape.grid.is_some(), state.grid.is_some());
            if let Some(grid) = state.grid {
                let drawn = shape.grid.as_ref().unwrap();
                assert_eq!(drawn.x, centimetres(grid.origin_x));
                assert_eq!(drawn.z, centimetres(grid.origin_y));
                assert_eq!((drawn.width, drawn.height), (grid.size_y, grid.size_x));
                for (x, column) in grid.rows.iter().enumerate() {
                    for (z, row) in drawn.rows.iter().enumerate() {
                        assert_eq!((row >> x) & 1, (column >> z) & 1);
                    }
                }
                saw_grid = true;
            }
            kinds.insert(serde_json::to_string(&shape.kind).unwrap());
        }
    }
    assert!(kinds.contains("\"oil\""));
    assert!(kinds.contains("\"fire\""));
    assert!(saw_grid, "the scene must exercise a shield-cut mask");
    assert!(
        timeline
            .terrains
            .iter()
            .any(|t| t.frames.iter().any(|f| f.to > f.from))
    );
}
