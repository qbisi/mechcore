use mechcore_mcfr::{
    Domain, DurableContext, Event, EventKind, Hashes, InstrumentationReader, InstrumentationRecord,
    InstrumentationSink, InstrumentationWriter, MCFR_SCHEMA_VERSION, McfrReader, McfrWriter,
    MotionState, ObjectKind, ObjectRef, ProjectileState, Rational, TransitionEvents, UnitState,
    Vec3, Visibility, WorldSnapshot,
};
use rust_hdf5::H5File;
use serde_json::json;

#[test]
fn writes_reads_and_verifies_state_and_event_tracks() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("battle.mcfr");
    let mut writer = McfrWriter::create(&path, context(), initial_state()).unwrap();
    writer
        .push_transition(release_events(), projectile_state())
        .unwrap();
    writer
        .push_transition(impact_events(), final_state())
        .unwrap();
    let written = writer.finish().unwrap();

    let reader = McfrReader::open_verified(&path).unwrap();
    assert_eq!(reader.state_count(), 3);
    assert_eq!(reader.transition_count(), 2);
    assert_eq!(reader.terminal_step(), 2);
    assert_eq!(reader.hashes(), &written);
    assert_eq!(reader.state(0).unwrap(), initial_state());
    assert_eq!(reader.state(1).unwrap(), projectile_state());
    assert_eq!(reader.events(0).unwrap(), release_events());
    assert_eq!(reader.events(1).unwrap(), impact_events());
}

#[test]
fn canonical_hashes_do_not_depend_on_input_collection_order() {
    let directory = tempfile::tempdir().unwrap();
    let left = directory.path().join("left.mcfr");
    let right = directory.path().join("right.mcfr");
    let expected = write_battle(&left, initial_state());
    let mut reversed = initial_state();
    reversed.units.reverse();
    let actual = write_battle(&right, reversed);
    assert_eq!(actual, expected);
}

#[test]
fn verification_rejects_a_tampered_hash() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tampered.mcfr");
    write_battle(&path, initial_state());
    let file = H5File::open_rw(&path).unwrap();
    file.set_attr_string("state_hash", &"0".repeat(64)).unwrap();
    file.close().unwrap();

    let reader = McfrReader::open(&path).unwrap();
    let error = reader.verify().unwrap_err();
    assert!(error.to_string().starts_with("state_hash mismatch:"));
}

#[test]
fn transition_requires_contiguous_events_and_lifecycle_evidence() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("invalid.mcfr");
    let mut writer = McfrWriter::create(&path, context(), initial_state()).unwrap();
    let mut events = release_events();
    events.events[0].event_seq = 1;
    assert!(writer.push_transition(events, projectile_state()).is_err());
    drop(writer);
    assert!(!path.exists());
}

#[test]
fn instrumentation_sidecar_supports_json_and_binary_channels() {
    let directory = tempfile::tempdir().unwrap();
    let mcfr = directory.path().join("battle.mcfr");
    let hashes = write_battle(&mcfr, initial_state());
    let sidecar = directory.path().join("battle.targeting.mcfr-i");
    let mut writer = InstrumentationWriter::create(
        &sidecar,
        &hashes.scenario_hash,
        "targeting-v1",
        "simulation",
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
    assert_eq!(reader.scenario_hash(), hashes.scenario_hash);
    assert_eq!(reader.profile(), "targeting-v1");
    assert_eq!(reader.producer(), "simulation");
    assert_eq!(reader.len(), 2);
    assert_eq!(reader.entry(0).unwrap().channel, "target_candidates");
    assert_eq!(reader.entry(1).unwrap().payload, [1, 2, 3, 4]);
}

fn write_battle(path: &std::path::Path, initial: WorldSnapshot) -> Hashes {
    let mut writer = McfrWriter::create(path, context(), initial).unwrap();
    writer
        .push_transition(release_events(), projectile_state())
        .unwrap();
    writer
        .push_transition(impact_events(), final_state())
        .unwrap();
    writer.finish().unwrap()
}

fn context() -> DurableContext {
    DurableContext {
        schema_version: MCFR_SCHEMA_VERSION,
        game_build: "test-build".into(),
        rules_fingerprint: "rules-v1".into(),
        logic_step: Rational {
            numerator: 1,
            denominator: 10,
        },
        numeric_convention: "world_q32_32".into(),
        rng_state: json!({"team_1": [1, 2, 3], "team_2": [4, 5, 6]}),
        identity_contract: "typed-session-u64-v1".into(),
        update_order_contract: "ascending-stable-id-v1".into(),
        durable_commands: vec![],
    }
}

fn initial_state() -> WorldSnapshot {
    WorldSnapshot {
        units: vec![unit(1, 1, 0, 100), unit(2, 2, 100, 100)],
        ..WorldSnapshot::default()
    }
}

fn projectile_state() -> WorldSnapshot {
    let mut state = initial_state();
    state.projectiles.push(ProjectileState {
        projectile_id: 10,
        team_id: 1,
        owner: Some(ObjectRef::new(ObjectKind::Unit, 1)),
        source: Some(ObjectRef::new(ObjectKind::Unit, 1)),
        projectile_type_id: 7,
        position: Vec3 { x: 10, y: 0, z: 0 },
        orientation: None,
        velocity: Some(Vec3 { x: 10, y: 0, z: 0 }),
        target: Some(ObjectRef::new(ObjectKind::Unit, 2)),
        cached_target_position: Vec3 { x: 100, y: 0, z: 0 },
        cached_target_radius: 5,
        active: true,
        life: None,
    });
    state
}

fn final_state() -> WorldSnapshot {
    let mut state = initial_state();
    state.units[1].life = 75;
    state
}

fn release_events() -> TransitionEvents {
    TransitionEvents {
        events: vec![Event {
            event_seq: 0,
            kind: EventKind::ProjectileReleased,
            subject: Some(ObjectRef::new(ObjectKind::Projectile, 10)),
            source: Some(ObjectRef::new(ObjectKind::Unit, 1)),
            target: Some(ObjectRef::new(ObjectKind::Unit, 2)),
            payload: json!({"projectile_type_id": 7}),
        }],
    }
}

fn impact_events() -> TransitionEvents {
    TransitionEvents {
        events: vec![
            Event {
                event_seq: 0,
                kind: EventKind::ProjectileImpacted,
                subject: Some(ObjectRef::new(ObjectKind::Projectile, 10)),
                source: Some(ObjectRef::new(ObjectKind::Unit, 1)),
                target: Some(ObjectRef::new(ObjectKind::Unit, 2)),
                payload: json!({"position": {"x": 100, "y": 0, "z": 0}}),
            },
            Event {
                event_seq: 1,
                kind: EventKind::Damage,
                subject: Some(ObjectRef::new(ObjectKind::Unit, 2)),
                source: Some(ObjectRef::new(ObjectKind::Projectile, 10)),
                target: Some(ObjectRef::new(ObjectKind::Unit, 2)),
                payload: json!({"applied": 25, "life_before": 100, "life_after": 75}),
            },
            Event {
                event_seq: 2,
                kind: EventKind::ProjectileRemoved,
                subject: Some(ObjectRef::new(ObjectKind::Projectile, 10)),
                source: None,
                target: None,
                payload: json!({"reason": "impact"}),
            },
        ],
    }
}

fn unit(id: u64, team: u32, x: i64, life: i64) -> UnitState {
    UnitState {
        unit_id: id,
        team_id: team,
        formation_id: id,
        unit_type_id: 1,
        parent_unit_id: None,
        domain: Domain::Ground,
        position: Vec3 { x, y: 0, z: 0 },
        body_rotation: 0,
        aim_pose: None,
        velocity: Vec3 { x: 0, y: 0, z: 0 },
        motion_state: MotionState::Idle,
        collision_radius: 5,
        life,
        max_life: 100,
        alive: life > 0,
        active: true,
        targetable: life > 0,
        visibility: Visibility::Visible,
        personal_shield: None,
    }
}
