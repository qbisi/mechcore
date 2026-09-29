use std::io::Read;

use bytes::Bytes;
use mechcore_mcfr::{
    BuildingState, CheckedSkill, DerivedStats, Domain, DurableContext, Event, EventPayload,
    GaugeI32, GroupSlot, HASH_PROFILE, Hashes, LiveUnitState, MCFR_FORMAT, McfrReader, McfrWriter,
    Modifier, ModifierChannel, ModifierPart, MotionState, ObjectKind, ObjectRef,
    PersonalShieldState, Producer, QPlanar, QVec3, Rational, RvoExit, RvoNeighbour,
    RvoNeighbourKind, RvoSolve, RvoVec, RvoVo, ShieldDestroyedReason, ShieldRoundPolicy,
    ShieldSourceKind, ShieldState, SkillAttackableCheck, TargetCandidate, TargetRefs, TargetSearch,
    TargetSearchPath, TerrainApplicationState, TerrainEffectClock, TerrainGridState,
    TerrainLogicLifetime, TerrainRemovedReason, TerrainState, TerrainType, TransitionEvents,
    Visibility, WeaponAimState, WorldSnapshot, sort_modifiers,
};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use serde_json::json;

const LAYOUT_YAML: &str = "kind: layout\nseed: 42\nround: 1\nblue:\n  units:\n  - {name: marksman, index: 0, position: {x: 0, y: -50}}\nred:\n  units:\n  - {name: arclight, index: 0, position: {x: 0, y: -50}}\n";

/// A layout as the writer embeds it: canonical, which states the version the
/// binary carries.
fn stated(layout: &str) -> String {
    layout.replacen(
        "kind: layout\n",
        &format!(
            "kind: layout\ngame_build: {}\n",
            mechcore_document::game_build()
        ),
        1,
    )
}

#[test]
#[allow(clippy::too_many_lines)]
fn writes_and_reads_every_table() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("fight.mcfr");
    let initial = state(100);
    let final_state = state(75);
    let events = damage_events();
    let hashes = write_fight(
        &path,
        "build-a",
        &context(),
        initial.clone(),
        final_state.clone(),
        &events,
    );

    let reader = McfrReader::open(&path).unwrap();
    assert_eq!(MCFR_FORMAT, "0.17.0");
    assert_eq!(reader.producer(), Producer::Game);
    assert_eq!(reader.tick_count(), 1);
    assert_eq!(reader.terminal_tick(), 1);
    assert_eq!(reader.game_build(), "build-a");
    assert_eq!(reader.context().match_seed, 42);
    assert_eq!(reader.layout_yaml(), stated(LAYOUT_YAML));
    assert_eq!(reader.hashes(), &hashes);
    assert_eq!(
        reader.file_size_bytes(),
        std::fs::metadata(&path).unwrap().len()
    );
    // The state has no projectile, so its table is left out.
    assert_eq!(
        reader.member_sizes_bytes().keys().collect::<Vec<_>>(),
        [
            "buildings.parquet",
            "events.parquet",
            "layout.yaml",
            "shields.parquet",
            "terrains.parquet",
            "ticks.parquet",
            "units.parquet",
        ]
    );
    assert!(reader.member_sizes_bytes().values().all(|size| *size > 0));
    assert!(reader.state(0).is_err());
    assert_eq!(reader.state(1).unwrap(), final_state);
    assert_eq!(reader.events(1).unwrap(), events);

    let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    assert_eq!(archive.len(), 7);
    assert!(archive.by_name("projectiles.parquet").is_err());
    {
        let mut entry = archive.by_name("layout.yaml").unwrap();
        assert_eq!(entry.compression(), zip::CompressionMethod::Stored);
        let mut layout = String::new();
        entry.read_to_string(&mut layout).unwrap();
        assert_eq!(layout, stated(LAYOUT_YAML));
    }
    for name in [
        "ticks.parquet",
        "units.parquet",
        "buildings.parquet",
        "shields.parquet",
        "terrains.parquet",
        "events.parquet",
    ] {
        let mut entry = archive.by_name(name).unwrap();
        assert_eq!(entry.compression(), zip::CompressionMethod::Stored);
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        assert_eq!(&bytes[..4], b"PAR1");
        assert_eq!(&bytes[bytes.len() - 4..], b"PAR1");
        let builder = ParquetRecordBatchReaderBuilder::try_new(Bytes::from(bytes)).unwrap();
        // No member embeds the Arrow schema; readers know each table's.
        assert!(
            builder
                .metadata()
                .file_metadata()
                .key_value_metadata()
                .is_none_or(|entries| entries.iter().all(|entry| entry.key != "ARROW:schema"))
        );
        if name == "ticks.parquet" {
            assert!(builder.schema().field_with_name("tick_hash").is_ok());
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
                metadata.get("hash_profile").map(String::as_str),
                Some(HASH_PROFILE)
            );
            assert_eq!(metadata.get("result_hash"), Some(&hashes.result_hash));
            assert!(!metadata.contains_key("scenario_hash"));
        } else if name == "buildings.parquet" {
            assert!(builder.schema().field_with_name("rotation").is_err());
        } else if name == "terrains.parquet" {
            assert!(builder.schema().field_with_name("active").is_err());
        } else if name == "events.parquet" {
            let batch = builder.build().unwrap().next().unwrap().unwrap();
            assert_eq!(batch.num_rows(), 1);
            let amount = batch
                .column_by_name("amount")
                .unwrap()
                .as_any()
                .downcast_ref::<arrow_array::Int32Array>()
                .unwrap();
            assert_eq!(amount.value(0), 25);
            // A damage carries no position.
            assert!(batch.column_by_name("position").unwrap().is_null(0));
        }
    }
}

#[test]
fn building_state_has_no_rotation_in_canonical_hash_input() {
    let mut value = serde_json::to_value(&state(100).buildings[0]).unwrap();
    assert!(value.get("rotation").is_none());
    value["rotation"] = json!(24_273_083_116_i64);
    assert!(serde_json::from_value::<BuildingState>(value).is_err());
}

#[test]
fn reader_open_and_comparison_trust_stored_tick_hashes() {
    let directory = tempfile::tempdir().unwrap();
    let original_path = directory.path().join("original.mcfr");
    write_fight(
        &original_path,
        "build-a",
        &context(),
        state(100),
        state(75),
        &damage_events(),
    );
    // The events of another fight, one damage point apart, swapped in.
    let other_path = directory.path().join("other.mcfr");
    let mut other_events = damage_events();
    other_events.events[0].payload = EventPayload::Damage {
        amount: 24,
        skill_slot: Some(0),
    };
    write_fight(
        &other_path,
        "build-a",
        &context(),
        state(100),
        state(75),
        &other_events,
    );
    let changed_path = directory.path().join("changed-events.mcfr");
    let mut original = zip::ZipArchive::new(std::fs::File::open(&original_path).unwrap()).unwrap();
    let mut other = zip::ZipArchive::new(std::fs::File::open(&other_path).unwrap()).unwrap();
    let mut changed = zip::ZipWriter::new(std::fs::File::create(&changed_path).unwrap());
    for index in 0..original.len() {
        let member = original.by_index(index).unwrap();
        if member.name() == "events.parquet" {
            drop(member);
            changed
                .raw_copy_file(other.by_name("events.parquet").unwrap())
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
        original.tick_hash(1).unwrap(),
        changed.tick_hash(1).unwrap()
    );
    assert_eq!(original.first_divergence(&changed).unwrap(), None);
    // Reading the tick rehashes what it holds, so it sees the swap the stored
    // hashes do not.
    assert_eq!(
        original.tick(1).unwrap().tick_hash,
        original.tick_hash(1).unwrap()
    );
    assert!(changed.tick(1).is_err());
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

    let path = directory.path().join("fight.mcfr");
    let published = write_fight(&path, "build-a", &context(), initial, final_state, &events);
    assert_eq!(hashes, published);
}

#[test]
fn game_build_metadata_does_not_change_result_hashes() {
    let directory = tempfile::tempdir().unwrap();
    let events = damage_events();
    let left = write_fight(
        &directory.path().join("left.mcfr"),
        "build-a",
        &context(),
        state(100),
        state(75),
        &events,
    );
    let right = write_fight(
        &directory.path().join("right.mcfr"),
        "build-b",
        &context(),
        state(100),
        state(75),
        &events,
    );
    assert_eq!(left, right);
}

/// The hash of this fixture has not moved since the definition was written:
/// a change here changes every pinned hash in the repository.
#[test]
fn the_result_hash_is_golden() {
    let hashes = hash_tick(&context(), state(75), &damage_events());
    assert_eq!(
        hashes.result_hash,
        "4f2928dc11779b421ccf2f3dd464564b8a5d6ca7905d77c30dcc10eed721fb8a"
    );
}

#[test]
fn hash_reads_every_field_of_the_state_and_events() {
    let baseline_state = state(75);
    let baseline_events = damage_events();
    let baseline = hash_tick(&context(), baseline_state.clone(), &baseline_events);

    for mutate in [
        |state: &mut WorldSnapshot| state.live_units[0].position.x += 1,
        |state: &mut WorldSnapshot| state.live_units[0].body_rotation += 1,
        |state: &mut WorldSnapshot| state.live_units[0].turret_rotation = Some(8 << 32),
        |state: &mut WorldSnapshot| state.live_units[0].velocity.z += 1,
        |state: &mut WorldSnapshot| state.live_units[0].life.current -= 1,
        |state: &mut WorldSnapshot| state.live_units[0].motion_state = MotionState::Attacking,
        |state: &mut WorldSnapshot| state.live_units[0].status_mask = 1,
        |state: &mut WorldSnapshot| state.live_units[0].modifiers[0].value += 1,
    ] {
        let mut changed = baseline_state.clone();
        mutate(&mut changed);
        assert_ne!(
            baseline.result_hash,
            hash_tick(&context(), changed, &baseline_events).result_hash
        );
    }

    let mut changed_events = baseline_events.clone();
    changed_events.events[0].payload = EventPayload::Damage {
        amount: 24,
        skill_slot: Some(0),
    };
    assert_ne!(
        baseline.result_hash,
        hash_tick(&context(), baseline_state.clone(), &changed_events).result_hash
    );
}

/// The durable context is metadata beside the timeline, not a hash input.
#[test]
fn hash_does_not_read_the_durable_context() {
    let baseline = hash_tick(&context(), state(75), &damage_events());
    let mut changed_context = context();
    changed_context.logic_step = Rational {
        numerator: 1,
        denominator: 20,
    };
    let changed = hash_tick(&changed_context, state(75), &damage_events());
    assert_eq!(baseline.result_hash, changed.result_hash);
}

#[test]
fn hash_preserves_native_event_order() {
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
    assert_ne!(baseline.result_hash, reversed.result_hash);
}

#[test]
fn embedded_layout_is_not_a_hash_input() {
    const OTHER_LAYOUT: &str = "kind: layout\nseed: 42\nround: 1\nblue:\n  units:\n  - {name: marksman, index: 0, position: {x: 20, y: -50}}\nred:\n  units:\n  - {name: arclight, index: 0, position: {x: 0, y: -50}}\n";
    let directory = tempfile::tempdir().unwrap();
    let events = damage_events();
    let left = write_fight(
        &directory.path().join("left.mcfr"),
        "build-a",
        &context(),
        state(100),
        state(75),
        &events,
    );
    let path = directory.path().join("right.mcfr");
    let mut writer =
        McfrWriter::create(&path, Producer::Game, "build-a", &context(), OTHER_LAYOUT).unwrap();
    writer.append_tick(state(75), &events).unwrap();
    let right = writer.finish().unwrap();
    assert_eq!(left, right);
    assert_eq!(
        McfrReader::open(path).unwrap().layout_yaml(),
        stated(OTHER_LAYOUT)
    );
}

#[test]
fn embedded_layout_preserves_adapter_state_outside_public_legality() {
    const PARTIAL_LAYOUT: &str = "kind: layout\nseed: 42\nround: 1\nblue:\n  units:\n  - {name: marksman, index: 0, position: {x: -310, y: 20}}\nred:\n  units:\n  - {name: arclight, index: 0, position: {x: -310, y: 20}}\n";
    assert!(mechcore_document::parse_yaml(PARTIAL_LAYOUT.as_bytes()).is_err());
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("partial-layout.mcfr");
    let context = context();
    let mut writer =
        McfrWriter::create(&path, Producer::Game, "build-a", &context, PARTIAL_LAYOUT).unwrap();
    writer
        .append_tick(state(75), &TransitionEvents { events: Vec::new() })
        .unwrap();
    writer.finish().unwrap();
    assert_eq!(
        McfrReader::open(path).unwrap().layout_yaml(),
        stated(PARTIAL_LAYOUT)
    );
}

#[test]
fn writer_rejects_initial_formation_ids_outside_zx_first_appearance_order() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("invalid-formations.mcfr");
    let mut invalid = state(100);
    invalid.live_units[0].formation_id = 2;
    invalid.live_units[1].formation_id = 1;
    let mut writer =
        McfrWriter::create(&path, Producer::Game, "build-a", &context(), LAYOUT_YAML).unwrap();
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
    let mut writer =
        McfrWriter::create(&path, Producer::Game, "build-a", &context(), LAYOUT_YAML).unwrap();
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
    let mut writer =
        McfrWriter::create(&path, Producer::Game, "build-a", &context(), LAYOUT_YAML).unwrap();
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
    assert!(writer.append_tick(state(100), &events).is_err());
    assert!(!path.exists());
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
    write_fight(&path, "build-a", &context(), state(100), state(75), &events);
    assert_eq!(McfrReader::open(&path).unwrap().events(1).unwrap(), events);
}

#[test]
fn writer_rejects_terrain_created_source() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("invalid-terrain-source.mcfr");
    let mut writer =
        McfrWriter::create(&path, Producer::Game, "build-a", &context(), LAYOUT_YAML).unwrap();
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
    assert!(writer.append_tick(state(75), &events).is_err());
    assert!(!path.exists());
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
    write_fight(&path, "build-a", &context(), state(100), state(75), &events);
    assert_eq!(McfrReader::open(&path).unwrap().events(1).unwrap(), events);

    let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let mut bytes = Vec::new();
    archive
        .by_name("events.parquet")
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let batch = ParquetRecordBatchReaderBuilder::try_new(Bytes::from(bytes))
        .unwrap()
        .build()
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    assert!(batch.column_by_name("source").unwrap().is_null(0));
}

#[test]
fn writer_rejects_inconsistent_shield_active_order() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("invalid-shield.mcfr");
    let mut invalid = state(100);
    invalid.shields[0].active_order = None;
    let mut writer =
        McfrWriter::create(&path, Producer::Game, "build-a", &context(), LAYOUT_YAML).unwrap();
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
        target: None,
        cached_target_position: QVec3 { x: 0, y: 0, z: 0 },
        cached_target_radius: 0,
        life: GaugeI32 {
            current: 1,
            maximum: 1,
        },
        spawn_containing_shields: vec![ObjectRef::new(ObjectKind::Building, 1)],
    });
    let mut writer =
        McfrWriter::create(&path, Producer::Game, "build-a", &context(), LAYOUT_YAML).unwrap();
    assert!(
        writer
            .append_tick(invalid, &TransitionEvents { events: Vec::new() })
            .is_err()
    );
    assert!(!path.exists());
}

#[test]
fn instrument_channels_ride_in_the_recording_outside_the_hash() {
    let directory = tempfile::tempdir().unwrap();
    let plain = write_fight(
        &directory.path().join("plain.mcfr"),
        "build-a",
        &context(),
        state(100),
        state(75),
        &damage_events(),
    );

    let path = directory.path().join("instrumented.mcfr");
    let mut writer =
        McfrWriter::create(&path, Producer::Game, "build-a", &context(), LAYOUT_YAML).unwrap();
    assert!(writer.append_instrument::<TargetRefs>(&[]).is_err());
    writer.append_tick(state(75), &damage_events()).unwrap();
    let refs = TargetRefs {
        unit: ObjectRef::new(ObjectKind::Unit, 1),
        mech_lock_target: Some(ObjectRef::new(ObjectKind::Unit, 2)),
        normal_skill_fields_available: true,
        skill_lock_target: None,
        skill_attack_target: Some(ObjectRef::new(ObjectKind::Building, 1)),
        skill_state: Some("SkillAttackState".into()),
        skill_attack_phase: Some("attacking".into()),
        skill_is_idle: Some(false),
    };
    writer
        .append_instrument(std::slice::from_ref(&refs))
        .unwrap();
    let check = SkillAttackableCheck {
        invocation_ordinal: 7,
        source_actor: ObjectRef::new(ObjectKind::Unit, 1),
        skill_slot: Some(2),
        is_attacking_check: true,
        before: CheckedSkill {
            lock_target: None,
            attack_target: None,
            skill_state: None,
            skill_attack_phase: None,
        },
        after: CheckedSkill {
            lock_target: Some(ObjectRef::new(ObjectKind::Unit, 2)),
            attack_target: None,
            skill_state: Some("SkillPrepareState".into()),
            skill_attack_phase: None,
        },
        check_return: true,
    };
    writer
        .append_instrument(std::slice::from_ref(&check))
        .unwrap();
    let slot = GroupSlot {
        unit: ObjectRef::new(ObjectKind::Unit, 1),
        skill_slot: 2,
        lock_target: Some(ObjectRef::new(ObjectKind::Unit, 2)),
        attack_target: Some(ObjectRef::new(ObjectKind::Unit, 2)),
        skill_state: Some("SkillPrepareState".into()),
        skill_attack_phase: None,
        skill_is_idle: Some(false),
    };
    writer
        .append_instrument(std::slice::from_ref(&slot))
        .unwrap();
    // Asked for and never filled: the channel is published empty.
    writer.append_instrument::<TargetSearch>(&[]).unwrap();
    assert_eq!(writer.finish().unwrap(), plain);

    let reader = McfrReader::open(&path).unwrap();
    assert_eq!(
        reader.instrument_channels().collect::<Vec<_>>(),
        [
            "group_slots",
            "skill_attackable_checker",
            "target_refs",
            "target_search"
        ]
    );
    assert_eq!(
        reader.instrument::<TargetRefs>().unwrap(),
        Some(vec![(1, refs)])
    );
    assert_eq!(
        reader.instrument::<SkillAttackableCheck>().unwrap(),
        Some(vec![(1, check)])
    );
    assert_eq!(
        reader.instrument::<GroupSlot>().unwrap(),
        Some(vec![(1, slot)])
    );
    assert_eq!(
        reader.instrument::<TargetSearch>().unwrap(),
        Some(Vec::new())
    );
    let plain = McfrReader::open(directory.path().join("plain.mcfr")).unwrap();
    assert_eq!(plain.instrument::<TargetRefs>().unwrap(), None);
}

#[test]
fn rvo_channels_round_trip_with_their_nulls_and_kinds() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("rvo.mcfr");
    let mut writer =
        McfrWriter::create(&path, Producer::Game, "build-a", &context(), LAYOUT_YAML).unwrap();
    writer.append_tick(state(75), &damage_events()).unwrap();
    let agent = ObjectRef::new(ObjectKind::Unit, 1);
    let at = |x, y| RvoVec { x, y };
    let solve = RvoSolve {
        agent,
        exit: RvoExit::Avoided,
        position: at(1 << 40, 2 << 40),
        elevation_raw: 1 << 32,
        height_raw: 0,
        current_velocity: at(0, 0),
        desired_velocity: at(-3, 1 << 34),
        desired_target: at(5, 6),
        desired_speed_raw: 1 << 34,
        max_speed_raw: 1 << 34,
        radius_outer_raw: 2 << 32,
        radius_inner_raw: 3 << 31,
        size: 0,
        priority_raw: 8_589_934,
        layer: 16,
        collides_with: 2_147_483_632,
        group: 0,
        ignore_same_group: false,
        team_id: 0,
        team_radius_raw: 0,
        max_neighbours: 20,
        neighbour_count: 2,
        biased_velocity: Some(at(-2, 1 << 34)),
        biased_target: Some(at(5, 7)),
        first_trace_point: Some(at(1, 2)),
        first_trace_score_raw: Some(-40),
        second_trace_point: Some(at(3, 4)),
        second_trace_score_raw: Some(900),
        output_target: Some(at(1 << 40, (2 << 40) + 2)),
        output_speed_raw: Some(1 << 33),
    };
    let neighbours = [
        RvoNeighbour {
            agent,
            slot: 0,
            neighbour: Some(ObjectRef::new(ObjectKind::Building, 3)),
            distance_sq_raw: 1 << 36,
            kind: RvoNeighbourKind::Opponent,
            vo: Some(0),
            radius_raw: Some(1 << 30),
            colliding: Some(false),
            penetration_raw: Some(0),
            weight_raw: Some(12),
        },
        // A map object the recording does not hold makes no VO here.
        RvoNeighbour {
            agent,
            slot: 1,
            neighbour: None,
            distance_sq_raw: 1 << 38,
            kind: RvoNeighbourKind::IgnoredSameGroup,
            vo: None,
            radius_raw: None,
            colliding: None,
            penetration_raw: None,
            weight_raw: None,
        },
    ];
    writer
        .append_instrument(std::slice::from_ref(&solve))
        .unwrap();
    writer.append_instrument(&neighbours).unwrap();
    writer.finish().unwrap();

    let reader = McfrReader::open(&path).unwrap();
    assert_eq!(
        reader.instrument::<RvoSolve>().unwrap(),
        Some(vec![(1, solve)])
    );
    assert_eq!(
        reader.instrument::<RvoNeighbour>().unwrap(),
        Some(neighbours.into_iter().map(|row| (1, row)).collect())
    );
    assert_eq!(reader.instrument::<RvoVo>().unwrap(), None);
}

#[test]
fn target_channels_round_trip_with_unseen_terms() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("targets.mcfr");
    let mut writer =
        McfrWriter::create(&path, Producer::Game, "build-a", &context(), LAYOUT_YAML).unwrap();
    writer.append_tick(state(75), &damage_events()).unwrap();
    let search = TargetSearch {
        search: 0,
        source: Some(ObjectRef::new(ObjectKind::Unit, 1)),
        skill_slot: Some(0),
        path: TargetSearchPath::SelectJob,
        candidates: 60,
        target: Some(ObjectRef::new(ObjectKind::Unit, 2)),
        nearest: None,
    };
    let candidate = TargetCandidate {
        search: 0,
        rank: 0,
        candidate: Some(ObjectRef::new(ObjectKind::Unit, 2)),
        score_raw: -7,
        distance_raw: None,
        distance_score_raw: None,
        angle_raw: None,
        angle_score_raw: None,
        is_left_side: None,
        max_attack_range_raw: None,
        source_rotation_raw: None,
        min_rotation_raw: None,
        max_rotation_raw: None,
    };
    writer
        .append_instrument(std::slice::from_ref(&search))
        .unwrap();
    writer
        .append_instrument(std::slice::from_ref(&candidate))
        .unwrap();
    writer.finish().unwrap();

    let reader = McfrReader::open(&path).unwrap();
    assert_eq!(
        reader.instrument::<TargetSearch>().unwrap(),
        Some(vec![(1, search)])
    );
    assert_eq!(
        reader.instrument::<TargetCandidate>().unwrap(),
        Some(vec![(1, candidate)])
    );
}

fn unit_modifiers(skill_count: u16) -> Vec<Modifier> {
    let mut modifiers = vec![
        modifier(
            ModifierChannel::MechFloat,
            None,
            "gf_range_value",
            ModifierPart::Value,
            1,
        ),
        modifier(
            ModifierChannel::MechFloatRate,
            None,
            "life_rate",
            ModifierPart::Add,
            4,
        ),
        modifier(
            ModifierChannel::MechFloatRate,
            None,
            "life_rate",
            ModifierPart::Reduce,
            5,
        ),
        modifier(
            ModifierChannel::MechInt,
            None,
            "move_speed_value",
            ModifierPart::Value,
            -10,
        ),
        modifier(
            ModifierChannel::Buff,
            None,
            "damage_rate",
            ModifierPart::Reduce,
            9,
        ),
    ];
    modifiers.extend((0..skill_count).flat_map(|skill_slot| {
        let base = i64::from(skill_slot) * 100;
        [
            modifier(
                ModifierChannel::SkillFloat,
                Some(skill_slot),
                "attack_range_value",
                ModifierPart::Value,
                base + 1,
            ),
            modifier(
                ModifierChannel::SkillInt,
                Some(skill_slot),
                "is_lock_target",
                ModifierPart::Value,
                base + 2,
            ),
        ]
    }));
    sort_modifiers(&mut modifiers);
    modifiers
}

fn modifier(
    channel: ModifierChannel,
    skill_slot: Option<u16>,
    field: &str,
    part: ModifierPart,
    value: i64,
) -> Modifier {
    Modifier {
        channel,
        skill_slot,
        field: field.to_owned(),
        part,
        value,
    }
}

fn write_fight(
    path: &std::path::Path,
    game_build: &str,
    context: &DurableContext,
    _initial: WorldSnapshot,
    final_state: WorldSnapshot,
    events: &TransitionEvents,
) -> Hashes {
    let mut writer =
        McfrWriter::create(path, Producer::Game, game_build, context, LAYOUT_YAML).unwrap();
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
        statistics: Vec::new(),
        formations: Vec::new(),
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
        turret_rotation: Some(7 << 32),
        velocity: QPlanar { x: 0, z: 0 },
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
        modifiers: unit_modifiers(skill_count),
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
        derived: DerivedStats {
            move_speed: 8 << 32,
            attack_range: 140 << 32,
            attack_damage: 2329,
            current_attack_interval: 62,
        },
    }
}

fn damage_events() -> TransitionEvents {
    TransitionEvents {
        events: vec![Event {
            subject: None,
            source: Some(ObjectRef::new(ObjectKind::Unit, 1)),
            source_team_id: Some(1),
            target: Some(ObjectRef::new(ObjectKind::Unit, 2)),
            payload: EventPayload::Damage {
                amount: 25,
                skill_slot: Some(0),
            },
        }],
    }
}
