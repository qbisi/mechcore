use std::io::{Read, Write};

use bytes::Bytes;
use mechcore_mcfr::{
    BuffModifierSet, BuildingState, CONTENT_HASH_PROFILE, Domain, DurableContext, Event,
    EventPayload, GaugeI32, Hashes, InstrumentationReader, InstrumentationRecord,
    InstrumentationSink, InstrumentationWriter, LiveUnitState, MCFR_FORMAT, McfrReader, McfrWriter,
    MotionState, ObjectKind, ObjectRef, PHYSICS_HASH_PROFILE, PersonalShieldState, QVec3,
    RateModifier, Rational, ShieldDestroyedReason, ShieldRoundPolicy, ShieldSourceKind,
    ShieldState, SkillDynamicModifierSet, SkillNumericModifierState, TerrainApplicationState,
    TerrainEffectClock, TerrainGridState, TerrainLogicLifetime, TerrainRemovedReason, TerrainState,
    TerrainType, TransitionEvents, UnitDynamicModifierSet, Visibility, WeaponAimState,
    WorldSnapshot,
};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use serde_json::json;

const LAYOUT_YAML: &str = "seed: 42\nround: 1\nsides:\n  blue:\n    formations:\n    - type: marksman\n      index: 0\n      position: {x: 0, y: -50}\n  red:\n    formations:\n    - type: arclight\n      index: 0\n      position: {x: 0, y: -50}\n";

#[test]
fn writes_and_reads_v6_tracks() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("battle.mcfr");
    let initial = state(100);
    let final_state = state(75);
    let events = damage_events();
    let hashes = write_battle(
        &path,
        "build-a",
        &context(),
        initial.clone(),
        final_state.clone(),
        &events,
    );

    let reader = McfrReader::open(&path).unwrap();
    assert_eq!(MCFR_FORMAT, "0.3.0");
    assert_eq!(reader.tick_count(), 1);
    assert_eq!(reader.terminal_tick(), 1);
    assert_eq!(reader.game_build(), "build-a");
    assert_eq!(reader.context().match_seed, 42);
    assert_eq!(reader.layout_yaml(), LAYOUT_YAML);
    assert_eq!(reader.hashes(), &hashes);
    assert_eq!(
        reader.file_size_bytes(),
        std::fs::metadata(&path).unwrap().len()
    );
    assert_eq!(reader.member_sizes_bytes().len(), 8);
    assert!(reader.member_sizes_bytes().values().all(|size| *size > 0));
    assert!(reader.state(0).is_err());
    assert_eq!(reader.state(1).unwrap(), final_state);
    assert_eq!(reader.events(1).unwrap(), events);

    let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    assert_eq!(archive.len(), 8);
    {
        let mut entry = archive.by_name("layout.yaml").unwrap();
        assert_eq!(entry.compression(), zip::CompressionMethod::Stored);
        let mut layout = String::new();
        entry.read_to_string(&mut layout).unwrap();
        assert_eq!(layout, LAYOUT_YAML);
    }
    for name in [
        "ticks.parquet",
        "units.parquet",
        "projectiles.parquet",
        "buildings.parquet",
        "shields.parquet",
        "terrains.parquet",
    ] {
        let mut entry = archive.by_name(name).unwrap();
        assert_eq!(entry.compression(), zip::CompressionMethod::Stored);
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        assert_eq!(&bytes[..4], b"PAR1");
        assert_eq!(&bytes[bytes.len() - 4..], b"PAR1");
        if name == "ticks.parquet" {
            let builder = ParquetRecordBatchReaderBuilder::try_new(Bytes::from(bytes)).unwrap();
            let metadata = builder.schema().metadata();
            assert_eq!(
                metadata.get("game_build").map(String::as_str),
                Some("build-a")
            );
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&metadata["durable_context"]).unwrap(),
                json!({
                    "combat_round": 1,
                    "logic_step": {"denominator": 10, "numerator": 1},
                    "time_units_per_second": 10
                })
            );
            assert_eq!(
                metadata.get("physics_hash_profile").map(String::as_str),
                Some(PHYSICS_HASH_PROFILE)
            );
            assert_eq!(
                metadata.get("content_hash_profile").map(String::as_str),
                Some(CONTENT_HASH_PROFILE)
            );
            assert_eq!(
                metadata.get("physics_result_hash"),
                Some(&hashes.physics_result_hash)
            );
            assert_eq!(
                metadata.get("content_result_hash"),
                Some(&hashes.content_result_hash)
            );
            assert!(!metadata.contains_key("result_hash"));
            assert!(!metadata.contains_key("scenario_hash"));
        } else if name == "buildings.parquet" {
            let builder = ParquetRecordBatchReaderBuilder::try_new(Bytes::from(bytes)).unwrap();
            assert!(builder.schema().field_with_name("rotation").is_err());
        } else if name == "terrains.parquet" {
            let builder = ParquetRecordBatchReaderBuilder::try_new(Bytes::from(bytes)).unwrap();
            assert!(builder.schema().field_with_name("active").is_err());
        }
    }
    let mut events_jsonl = String::new();
    archive
        .by_name("events.jsonl")
        .unwrap()
        .read_to_string(&mut events_jsonl)
        .unwrap();
    assert!(events_jsonl.ends_with('\n'));
    assert!(events_jsonl.contains("\"damage\""));
}

#[test]
fn building_state_has_no_rotation_in_canonical_hash_input() {
    let mut value = serde_json::to_value(&state(100).buildings[0]).unwrap();
    assert!(value.get("rotation").is_none());
    value["rotation"] = json!(24273083116_i64);
    assert!(serde_json::from_value::<BuildingState>(value).is_err());
}

#[test]
fn reader_open_and_comparison_trust_persisted_hashes() {
    let directory = tempfile::tempdir().unwrap();
    let original_path = directory.path().join("original.mcfr");
    write_battle(
        &original_path,
        "build-a",
        &context(),
        state(100),
        state(75),
        &damage_events(),
    );
    let changed_path = directory.path().join("changed-events.mcfr");
    let mut original = zip::ZipArchive::new(std::fs::File::open(&original_path).unwrap()).unwrap();
    let mut changed = zip::ZipWriter::new(std::fs::File::create(&changed_path).unwrap());
    for index in 0..original.len() {
        let mut member = original.by_index(index).unwrap();
        if member.name() == "events.jsonl" {
            let mut events = String::new();
            member.read_to_string(&mut events).unwrap();
            assert!(events.contains("\"amount\":25"));
            changed
                .start_file(
                    "events.jsonl",
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Stored)
                        .large_file(true),
                )
                .unwrap();
            changed
                .write_all(events.replace("\"amount\":25", "\"amount\":24").as_bytes())
                .unwrap();
        } else {
            changed.raw_copy_file(member).unwrap();
        }
    }
    changed.finish().unwrap();

    let original = McfrReader::open(original_path).unwrap();
    let changed = McfrReader::open(changed_path).unwrap();
    assert_ne!(original.events(1).unwrap(), changed.events(1).unwrap());
    assert_eq!(original.hashes(), changed.hashes());
    assert_eq!(
        original.physics_tick_hash(1).unwrap(),
        changed.physics_tick_hash(1).unwrap()
    );
    assert_eq!(
        original.content_tick_hash(1).unwrap(),
        changed.content_tick_hash(1).unwrap()
    );
    assert_eq!(original.first_divergence(&changed).unwrap(), None);
}

#[test]
fn hash_only_timeline_matches_published_mcfr_hashes_without_creating_storage() {
    let directory = tempfile::tempdir().unwrap();
    let initial = state(100);
    let final_state = state(75);
    let events = damage_events();
    let mut hash_only = McfrWriter::hash_only(&context()).unwrap();
    hash_only.append_tick(final_state.clone(), &events).unwrap();
    let hashes = hash_only.finish().unwrap();
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);

    let path = directory.path().join("battle.mcfr");
    let published = write_battle(&path, "build-a", &context(), initial, final_state, &events);
    assert_eq!(hashes, published);
}

#[test]
fn game_build_metadata_does_not_change_result_hashes() {
    let directory = tempfile::tempdir().unwrap();
    let events = damage_events();
    let left = write_battle(
        &directory.path().join("left.mcfr"),
        "build-a",
        &context(),
        state(100),
        state(75),
        &events,
    );
    let right = write_battle(
        &directory.path().join("right.mcfr"),
        "build-b",
        &context(),
        state(100),
        state(75),
        &events,
    );
    assert_eq!(left, right);
}

#[test]
fn physics_hash_ignores_nonphysical_details_while_content_hash_detects_them() {
    let events = damage_events();
    let baseline = hash_tick(&context(), state(75), &events);
    let mut changed = state(75);
    changed.live_units[0].motion_state = MotionState::Attacking;
    changed.live_units[0].mech_lock_target = Some(ObjectRef::new(ObjectKind::Unit, 2));
    changed.live_units[0].status_mask = 1;
    changed.live_units[0].unit_dynamic_modifiers.gf_range_value += 1;
    changed.live_units[0].weapon_aims[0].attack_target = Some(ObjectRef::new(ObjectKind::Unit, 2));
    let changed = hash_tick(&context(), changed, &events);
    assert_eq!(baseline.physics_result_hash, changed.physics_result_hash);
    assert_ne!(baseline.content_result_hash, changed.content_result_hash);
}

#[test]
fn battle_physics_v1_has_a_golden_result_hash() {
    let hashes = hash_tick(&context(), state(75), &damage_events());
    assert_eq!(
        hashes.physics_result_hash,
        "40efd7bd0c03b9430a9d2f3b868260e123ee67003c63c24cc6044fe837e348b4"
    );
}

#[test]
fn physics_hash_is_sensitive_to_time_motion_vitals_and_damage() {
    let baseline_state = state(75);
    let baseline_events = damage_events();
    let baseline = hash_tick(&context(), baseline_state.clone(), &baseline_events);

    for mutate in [
        |state: &mut WorldSnapshot| state.live_units[0].position.x += 1,
        |state: &mut WorldSnapshot| state.live_units[0].body_rotation += 1,
        |state: &mut WorldSnapshot| state.live_units[0].velocity.z += 1,
        |state: &mut WorldSnapshot| state.live_units[0].life.current -= 1,
    ] {
        let mut changed = baseline_state.clone();
        mutate(&mut changed);
        assert_ne!(
            baseline.physics_result_hash,
            hash_tick(&context(), changed, &baseline_events).physics_result_hash
        );
    }

    let mut changed_events = baseline_events.clone();
    changed_events.events[0].payload = EventPayload::Damage { amount: 24 };
    assert_ne!(
        baseline.physics_result_hash,
        hash_tick(&context(), baseline_state.clone(), &changed_events).physics_result_hash
    );

    let mut changed_context = context();
    changed_context.logic_step = Rational {
        numerator: 1,
        denominator: 20,
    };
    let changed = hash_tick(&changed_context, baseline_state, &baseline_events);
    assert_ne!(baseline.physics_result_hash, changed.physics_result_hash);
    assert_eq!(baseline.content_result_hash, changed.content_result_hash);
}

#[test]
fn physics_angles_are_normalized_without_weakening_content_hashes() {
    let baseline = hash_tick(&context(), state(75), &damage_events());
    let mut equivalent = state(75);
    equivalent.live_units[0].body_rotation = 360_i64 << 32;
    let equivalent = hash_tick(&context(), equivalent, &damage_events());
    assert_eq!(baseline.physics_result_hash, equivalent.physics_result_hash);
    assert_ne!(baseline.content_result_hash, equivalent.content_result_hash);
}

#[test]
fn physics_time_ratio_is_reduced_to_a_canonical_value() {
    let baseline = hash_tick(&context(), state(75), &damage_events());
    let mut equivalent_context = context();
    equivalent_context.logic_step = Rational {
        numerator: 2,
        denominator: 20,
    };
    let equivalent = hash_tick(&equivalent_context, state(75), &damage_events());
    assert_eq!(baseline.physics_result_hash, equivalent.physics_result_hash);
    assert_eq!(baseline.content_result_hash, equivalent.content_result_hash);
}

#[test]
fn physics_hash_preserves_native_event_order() {
    let mut ordered = damage_events();
    ordered.events.push(Event {
        subject: None,
        source: Some(ObjectRef::new(ObjectKind::Unit, 1)),
        source_team_id: Some(1),
        target: Some(ObjectRef::new(ObjectKind::Unit, 1)),
        payload: EventPayload::Healing { amount: 10 },
    });
    let baseline = hash_tick(&context(), state(75), &ordered);
    ordered.events.reverse();
    let reversed = hash_tick(&context(), state(75), &ordered);
    assert_ne!(baseline.physics_result_hash, reversed.physics_result_hash);
    assert_ne!(baseline.content_result_hash, reversed.content_result_hash);
}

#[test]
fn physics_hash_ignores_event_provenance_annotations() {
    let event = Event {
        subject: Some(ObjectRef::new(ObjectKind::Shield, 2)),
        source: None,
        source_team_id: None,
        target: None,
        payload: EventPayload::ShieldCreated {
            team_id: 2,
            source_kind: ShieldSourceKind::CommanderSkill,
            position: QVec3 { x: 7, y: 8, z: 9 },
        },
    };
    let baseline = hash_tick(
        &context(),
        state(75),
        &TransitionEvents {
            events: vec![event.clone()],
        },
    );
    let mut changed = event;
    changed.payload = EventPayload::ShieldCreated {
        team_id: 2,
        source_kind: ShieldSourceKind::SpawnedTemporary,
        position: QVec3 { x: 7, y: 8, z: 9 },
    };
    let changed = hash_tick(
        &context(),
        state(75),
        &TransitionEvents {
            events: vec![changed],
        },
    );
    assert_eq!(baseline.physics_result_hash, changed.physics_result_hash);
    assert_ne!(baseline.content_result_hash, changed.content_result_hash);
}

#[test]
fn embedded_layout_is_not_a_hash_input() {
    const OTHER_LAYOUT: &str = "seed: 42\nround: 1\nsides:\n  blue:\n    formations:\n    - type: marksman\n      index: 0\n      position: {x: 20, y: -50}\n  red:\n    formations:\n    - type: arclight\n      index: 0\n      position: {x: 0, y: -50}\n";
    let directory = tempfile::tempdir().unwrap();
    let events = damage_events();
    let left = write_battle(
        &directory.path().join("left.mcfr"),
        "build-a",
        &context(),
        state(100),
        state(75),
        &events,
    );
    let path = directory.path().join("right.mcfr");
    let mut writer = McfrWriter::create(&path, "build-a", &context(), OTHER_LAYOUT).unwrap();
    writer.append_tick(state(75), &events).unwrap();
    let right = writer.finish().unwrap();
    assert_eq!(left, right);
    assert_eq!(McfrReader::open(path).unwrap().layout_yaml(), OTHER_LAYOUT);
}

#[test]
fn embedded_layout_preserves_adapter_state_outside_public_legality() {
    const PARTIAL_LAYOUT: &str = "seed: 42\nround: 1\nsides:\n  blue:\n    formations:\n    - type: marksman\n      index: 0\n      position: {x: -310, y: 20}\n  red:\n    formations:\n    - type: arclight\n      index: 0\n      position: {x: -310, y: 20}\n";
    assert!(mechcore_layout::parse_yaml(PARTIAL_LAYOUT.as_bytes()).is_err());
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("partial-layout.mcfr");
    let context = context();
    let mut writer = McfrWriter::create(&path, "build-a", &context, PARTIAL_LAYOUT).unwrap();
    writer
        .append_tick(state(75), &TransitionEvents { events: Vec::new() })
        .unwrap();
    writer.finish().unwrap();
    assert_eq!(
        McfrReader::open(path).unwrap().layout_yaml(),
        PARTIAL_LAYOUT
    );
}

#[test]
fn writer_rejects_initial_formation_ids_outside_zx_first_appearance_order() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("invalid-formations.mcfr");
    let mut invalid = state(100);
    invalid.live_units[0].formation_id = 2;
    invalid.live_units[1].formation_id = 1;
    let mut writer = McfrWriter::create(&path, "build-a", &context(), LAYOUT_YAML).unwrap();
    let error = writer
        .append_tick(invalid, &TransitionEvents { events: Vec::new() })
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("first appearance in unit identity order")
    );
    assert!(!path.exists());
}

#[test]
fn writer_rejects_reserved_status_bits() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("invalid.mcfr");
    let mut invalid = state(100);
    invalid.live_units[0].status_mask = 1 << 4;
    let mut writer = McfrWriter::create(&path, "build-a", &context(), LAYOUT_YAML).unwrap();
    assert!(
        writer
            .append_tick(invalid, &TransitionEvents { events: Vec::new() })
            .is_err()
    );
    assert!(!path.exists());
}

#[test]
fn writer_rejects_partial_projectile_channel() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("invalid-event.mcfr");
    let mut writer = McfrWriter::create(&path, "build-a", &context(), LAYOUT_YAML).unwrap();
    let events = TransitionEvents {
        events: vec![Event {
            subject: Some(ObjectRef::new(ObjectKind::Projectile, 1)),
            source: Some(ObjectRef::new(ObjectKind::Unit, 1)),
            source_team_id: Some(1),
            target: Some(ObjectRef::new(ObjectKind::Unit, 2)),
            payload: EventPayload::ProjectileReleased {
                skill_slot: Some(0),
                weapon_index: None,
            },
        }],
    };
    writer.append_tick(state(100), &events).unwrap();
    assert!(writer.finish().is_err());
}

#[test]
fn shield_events_and_projectile_absorption_round_trip() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("shield-events.mcfr");
    let shield = ObjectRef::new(ObjectKind::Shield, 1);
    let projectile = ObjectRef::new(ObjectKind::Projectile, 1);
    let events = TransitionEvents {
        events: vec![
            Event {
                subject: Some(projectile),
                source: Some(ObjectRef::new(ObjectKind::Unit, 1)),
                source_team_id: Some(1),
                target: None,
                payload: EventPayload::ProjectileRemoved {
                    position: QVec3 { x: 4, y: 5, z: 6 },
                    intercepted: false,
                    absorbed_by: Some(shield),
                },
            },
            Event {
                subject: Some(ObjectRef::new(ObjectKind::Shield, 2)),
                source: None,
                source_team_id: None,
                target: None,
                payload: EventPayload::ShieldCreated {
                    team_id: 2,
                    source_kind: ShieldSourceKind::SpawnedTemporary,
                    position: QVec3 { x: 7, y: 8, z: 9 },
                },
            },
            Event {
                subject: Some(shield),
                source: None,
                source_team_id: None,
                target: None,
                payload: EventPayload::ShieldDestroyed {
                    position: QVec3 { x: 4, y: 5, z: 6 },
                    reason: ShieldDestroyedReason::Unknown,
                },
            },
        ],
    };
    write_battle(&path, "build-a", &context(), state(100), state(75), &events);
    assert_eq!(McfrReader::open(&path).unwrap().events(1).unwrap(), events);
}

#[test]
fn writer_rejects_terrain_created_source() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("invalid-terrain-source.mcfr");
    let mut writer = McfrWriter::create(&path, "build-a", &context(), LAYOUT_YAML).unwrap();
    let events = TransitionEvents {
        events: vec![Event {
            subject: Some(ObjectRef::new(ObjectKind::Terrain, 1)),
            source: Some(ObjectRef::new(ObjectKind::Projectile, 1)),
            source_team_id: Some(1),
            target: None,
            payload: EventPayload::TerrainCreated {
                team_id: Some(1),
                terrain_type: TerrainType::Fire,
                position: QVec3 { x: 8, y: 0, z: 12 },
                radius: 12_i64 << 32,
            },
        }],
    };
    writer.append_tick(state(75), &events).unwrap();
    assert!(writer.finish().is_err());
}

#[test]
fn terrain_events_round_trip() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("terrain-events.mcfr");
    let terrain = ObjectRef::new(ObjectKind::Terrain, 1);
    let events = TransitionEvents {
        events: vec![
            Event {
                subject: Some(terrain),
                source: None,
                source_team_id: Some(1),
                target: None,
                payload: EventPayload::TerrainCreated {
                    team_id: Some(1),
                    terrain_type: TerrainType::Oil,
                    position: QVec3 { x: 8, y: 0, z: 12 },
                    radius: 20_i64 << 32,
                },
            },
            Event {
                subject: Some(terrain),
                source: None,
                source_team_id: Some(1),
                target: None,
                payload: EventPayload::TerrainRemoved {
                    position: QVec3 { x: 8, y: 0, z: 12 },
                    reason: TerrainRemovedReason::RoundExpired,
                },
            },
            Event {
                subject: None,
                source: Some(terrain),
                source_team_id: Some(1),
                target: Some(ObjectRef::new(ObjectKind::Unit, 2)),
                payload: EventPayload::Healing { amount: 5 },
            },
        ],
    };
    write_battle(&path, "build-a", &context(), state(100), state(75), &events);
    assert_eq!(McfrReader::open(&path).unwrap().events(1).unwrap(), events);

    let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let mut events_jsonl = String::new();
    archive
        .by_name("events.jsonl")
        .unwrap()
        .read_to_string(&mut events_jsonl)
        .unwrap();
    let first =
        serde_json::from_str::<serde_json::Value>(events_jsonl.lines().next().unwrap()).unwrap();
    assert!(!first.as_object().unwrap().contains_key("source"));
}

#[test]
fn writer_rejects_inconsistent_shield_active_order() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("invalid-shield.mcfr");
    let mut invalid = state(100);
    invalid.shields[0].active_order = None;
    let mut writer = McfrWriter::create(&path, "build-a", &context(), LAYOUT_YAML).unwrap();
    assert!(
        writer
            .append_tick(invalid, &TransitionEvents { events: Vec::new() })
            .is_err()
    );
    assert!(!path.exists());
}

#[test]
fn writer_rejects_non_shield_projectile_containment_reference() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("invalid-projectile-shield-ref.mcfr");
    let mut invalid = state(100);
    invalid.projectiles.push(mechcore_mcfr::ProjectileState {
        projectile_id: 1,
        team_id: 1,
        owner: Some(ObjectRef::new(ObjectKind::Unit, 1)),
        position: QVec3 { x: 0, y: 0, z: 0 },
        orientation: 0,
        target: None,
        cached_target_position: QVec3 { x: 0, y: 0, z: 0 },
        cached_target_radius: 0,
        released: false,
        life: GaugeI32 {
            current: 1,
            maximum: 1,
        },
        spawn_containing_shields: vec![ObjectRef::new(ObjectKind::Building, 1)],
    });
    let mut writer = McfrWriter::create(&path, "build-a", &context(), LAYOUT_YAML).unwrap();
    assert!(
        writer
            .append_tick(invalid, &TransitionEvents { events: Vec::new() })
            .is_err()
    );
    assert!(!path.exists());
}

#[test]
fn instrumentation_sidecar_supports_json_and_binary_channels() {
    let directory = tempfile::tempdir().unwrap();
    let hashes = write_battle(
        &directory.path().join("battle.mcfr"),
        "build-a",
        &context(),
        state(100),
        state(75),
        &damage_events(),
    );
    let sidecar = directory.path().join("battle.targeting.mcfr-i");
    let mut writer = InstrumentationWriter::create(
        &sidecar,
        &hashes.physics_result_hash,
        "targeting-v1",
        "adapter",
    )
    .unwrap();
    writer
        .record_json(0, "target_candidates", &json!({"ids": [2, 1]}))
        .unwrap();
    writer
        .record(InstrumentationRecord {
            step: 1,
            channel: "rvo_solver",
            content_type: "application/octet-stream",
            payload: &[1, 2, 3, 4],
        })
        .unwrap();
    writer.finish().unwrap();

    let reader = InstrumentationReader::open(&sidecar).unwrap();
    assert_eq!(reader.physics_result_hash(), hashes.physics_result_hash);
    assert_eq!(reader.len(), 2);
    assert_eq!(reader.entry(1).unwrap().payload, [1, 2, 3, 4]);
}

fn write_battle(
    path: &std::path::Path,
    game_build: &str,
    context: &DurableContext,
    _initial: WorldSnapshot,
    final_state: WorldSnapshot,
    events: &TransitionEvents,
) -> Hashes {
    let mut writer = McfrWriter::create(path, game_build, context, LAYOUT_YAML).unwrap();
    writer.append_tick(final_state, events).unwrap();
    writer.finish().unwrap()
}

fn hash_tick(context: &DurableContext, state: WorldSnapshot, events: &TransitionEvents) -> Hashes {
    let mut writer = McfrWriter::hash_only(context).unwrap();
    writer.append_tick(state, events).unwrap();
    writer.finish().unwrap()
}

fn context() -> DurableContext {
    DurableContext {
        logic_step: Rational {
            numerator: 1,
            denominator: 10,
        },
        time_units_per_second: 10,
        combat_round: 1,
        match_seed: 42,
    }
}

fn state(enemy_life: i32) -> WorldSnapshot {
    WorldSnapshot {
        live_units: vec![unit(1, 1, 0, 100, true), unit(2, 2, 100, enemy_life, false)],
        projectiles: Vec::new(),
        buildings: vec![BuildingState {
            building_id: 1,
            team_id: 1,
            building_type_id: 7,
            position: QVec3 { x: -10, y: 4, z: 2 },
            bounds_width: 12,
            bounds_height: 8,
            life: GaugeI32 {
                current: 50,
                maximum: 60,
            },
            available: true,
            targetable: false,
            collision_enabled: true,
        }],
        shields: vec![ShieldState {
            shield_id: 1,
            team_id: 1,
            source_kind: ShieldSourceKind::Contraption,
            owner: None,
            position: QVec3 { x: 4, y: 5, z: 6 },
            radius: 70_i64 << 32,
            energy: GaugeI32 {
                current: 2_000,
                maximum: 2_000,
            },
            round_policy: ShieldRoundPolicy::ResetToMax,
            active: true,
            active_order: Some(0),
        }],
        terrains: vec![TerrainState {
            terrain_id: 1,
            team_id: Some(1),
            terrain_type: TerrainType::Oil,
            position: QVec3 { x: 8, y: 0, z: 12 },
            radius: 20_i64 << 32,
            grid: Some(TerrainGridState {
                origin_x: 1,
                origin_y: 2,
                size_x: 2,
                size_y: 2,
                rows: vec![0b01, 0b11],
            }),
            remaining_rounds: Some(1),
            logic_lifetime: Some(TerrainLogicLifetime {
                elapsed: 2,
                limit: 10,
            }),
            applications: vec![TerrainApplicationState {
                unit_id: 2,
                periodic_clock: Some(TerrainEffectClock {
                    elapsed: 1,
                    duration: 3,
                }),
            }],
        }],
    }
}

fn unit(id: u64, team: u32, x: i64, life: i32, with_secondary: bool) -> LiveUnitState {
    let skill_count: u16 = if with_secondary { 2 } else { 1 };
    LiveUnitState {
        unit_id: id,
        team_id: team,
        original_team_id: team,
        formation_id: id,
        unit_type_id: 1,
        domain: Domain::Ground,
        position: QVec3 { x, y: 0, z: 0 },
        body_rotation: 0,
        velocity: QVec3 { x: 0, y: 0, z: 0 },
        motion_state: MotionState::Idle,
        mech_lock_target: None,
        collision_radius: 5,
        life: GaugeI32 {
            current: life,
            maximum: 100,
        },
        active: true,
        targetable: true,
        visibility: Visibility::Normal,
        status_mask: 0,
        buff_modifiers: BuffModifierSet::default(),
        unit_dynamic_modifiers: UnitDynamicModifierSet {
            gf_range_value: 1,
            gf_life_time_value: 2,
            mech_group_distance: 3,
            life_rate: rate(4),
            life_rate_by_kill_count: rate(5),
            reduce_damage_from_remote: rate(6),
            move_ability_exit_time_change_rate: rate(7),
            move_speed_change_rate: rate(8),
            amplify_damage_rate: rate(9),
            move_speed_value: 10,
            reduce_damage_value: 11,
            child_inherit_technology_effect: 12,
        },
        skill_dynamic_modifiers: (0..skill_count)
            .map(|skill_slot| SkillNumericModifierState {
                skill_slot,
                modifiers: skill_modifiers(i64::from(skill_slot) * 100),
            })
            .collect(),
        personal_shield: PersonalShieldState {
            active: false,
            enabled: false,
            energy: GaugeI32 {
                current: 0,
                maximum: 0,
            },
        },
        weapon_aims: (0..skill_count)
            .map(|skill_slot| WeaponAimState {
                skill_slot,
                weapon_index: 0,
                attack_target: None,
                pose: None,
            })
            .collect(),
    }
}

fn rate(value: i64) -> RateModifier {
    RateModifier {
        add: value,
        reduce: value + 1,
    }
}

fn skill_modifiers(base: i64) -> SkillDynamicModifierSet {
    SkillDynamicModifierSet {
        min_attack_range_value: base + 1,
        attack_range_value: base + 2,
        attack_air_range_add_value: base + 3,
        attack_ground_range_add_value: base + 4,
        attack_interval_value: base + 5,
        damage_change_rate_ground: base + 6,
        damage_change_rate_air: base + 7,
        splash_range_value: base + 8,
        cb_life_recovery_rate: base + 9,
        projectile_speed_value: base + 10,
        attack_point_change_value: base + 11,
        projectile_duration_value: base + 12,
        projectile_random_range: base + 13,
        additional_damage_by_target_life: base + 14,
        damage_rate: rate(base + 15),
        damage_rate_by_kill_count: rate(base + 16),
        attack_range_rate: rate(base + 17),
        attack_interval_rate: rate(base + 18),
        damage_reduce_rate_base: rate(base + 19),
        projectile_life_rate: rate(base + 20),
        projectile_count_value: i32::try_from(base + 21).unwrap(),
        air_attack_value: i32::try_from(base + 22).unwrap(),
        ground_attack_value: i32::try_from(base + 23).unwrap(),
        attack_range_value_air: i32::try_from(base + 24).unwrap(),
        attack_range_value_ground: i32::try_from(base + 25).unwrap(),
        is_lock_target: i32::try_from(base + 26).unwrap(),
    }
}

fn damage_events() -> TransitionEvents {
    TransitionEvents {
        events: vec![Event {
            subject: None,
            source: Some(ObjectRef::new(ObjectKind::Unit, 1)),
            source_team_id: Some(1),
            target: Some(ObjectRef::new(ObjectKind::Unit, 2)),
            payload: EventPayload::Damage { amount: 25 },
        }],
    }
}
