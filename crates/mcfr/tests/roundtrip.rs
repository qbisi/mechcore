use mechcore_mcfr::{
    Domain, DurableContext, Event, EventPayload, Gauge, Hashes, IdentityContract,
    InstrumentationReader, InstrumentationRecord, InstrumentationSink, InstrumentationWriter,
    MCFR_SCHEMA_VERSION, McfrReader, McfrWriter, MotionState, NumericConvention, ObjectKind,
    ObjectRef, PersonalShieldState, Pose, ProjectileState, Rational, TransitionEvents, UnitState,
    Vec3, Visibility, WorldSnapshot,
};
use rust_hdf5::H5File;
use serde_json::json;

#[test]
fn writes_reads_and_verifies_state_and_event_tracks() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("battle.mcfr");
    let mut writer = McfrWriter::create(&path, &context(), initial_state()).unwrap();
    writer
        .push_transition(&release_events(), projectile_state())
        .unwrap();
    writer
        .push_transition(&impact_events(), final_state())
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
fn writer_does_not_validate_gameplay_transition_legality() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("invalid.mcfr");
    let mut writer = McfrWriter::create(&path, &context(), initial_state()).unwrap();
    let events = TransitionEvents {
        events: vec![Event {
            subject: None,
            source: None,
            target: Some(ObjectRef::new(ObjectKind::Unit, 999)),
            payload: EventPayload::Damage { amount: -1 },
        }],
    };
    writer.push_transition(&events, projectile_state()).unwrap();
    writer.finish().unwrap();
    assert!(McfrReader::open_verified(path).is_ok());
}

#[test]
fn writer_rejects_noncanonical_initial_identities() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("invalid-identities.mcfr");
    let mut state = initial_state();
    state.units[1].unit_id = 3;
    state.units[1].formation_id = 3;
    assert!(McfrWriter::create(&path, &context(), state).is_err());
    assert!(!path.exists());
}

#[test]
fn writer_rejects_initial_unit_ids_outside_team_yx_order() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("invalid-unit-order.mcfr");
    let mut state = initial_state();
    state.units[1].team_id = 1;
    state.units[0].position.x = 100;
    state.units[1].position.x = 0;
    assert!(McfrWriter::create(&path, &context(), state).is_err());
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
    let mut writer = McfrWriter::create(path, &context(), initial).unwrap();
    writer
        .push_transition(&release_events(), projectile_state())
        .unwrap();
    writer
        .push_transition(&impact_events(), final_state())
        .unwrap();
    writer.finish().unwrap()
}

fn context() -> DurableContext {
    DurableContext {
        schema_version: MCFR_SCHEMA_VERSION,
        game_build: "test-build".into(),
        logic_step: Rational {
            numerator: 1,
            denominator: 10,
        },
        numeric_convention: NumericConvention {
            distance_units_per_meter: 1_000,
            rotation_units_per_degree: 1_000,
            time_units_per_second: 10,
        },
        combat_round: 1,
        match_seed: 42,
        identity_contract: IdentityContract::TeamYxSequentialV1,
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
        projectile_id: 1,
        team_id: 1,
        owner: Some(ObjectRef::new(ObjectKind::Unit, 1)),
        position: Vec3 { x: 10, y: 0, z: 0 },
        orientation: 0,
        target: Some(ObjectRef::new(ObjectKind::Unit, 2)),
        cached_target_position: Vec3 { x: 100, y: 0, z: 0 },
        cached_target_radius: 5,
        released: true,
        life: Gauge {
            current: 0,
            maximum: 0,
        },
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
            subject: Some(ObjectRef::new(ObjectKind::Projectile, 1)),
            source: Some(ObjectRef::new(ObjectKind::Unit, 1)),
            target: Some(ObjectRef::new(ObjectKind::Unit, 2)),
            payload: EventPayload::ProjectileReleased,
        }],
    }
}

fn impact_events() -> TransitionEvents {
    TransitionEvents {
        events: vec![
            Event {
                subject: None,
                source: Some(ObjectRef::new(ObjectKind::Projectile, 1)),
                target: Some(ObjectRef::new(ObjectKind::Unit, 2)),
                payload: EventPayload::Damage { amount: 25 },
            },
            Event {
                subject: Some(ObjectRef::new(ObjectKind::Projectile, 1)),
                source: Some(ObjectRef::new(ObjectKind::Unit, 1)),
                target: Some(ObjectRef::new(ObjectKind::Unit, 2)),
                payload: EventPayload::ProjectileRemoved {
                    position: Vec3 { x: 100, y: 0, z: 0 },
                    intercepted: false,
                },
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
        domain: Domain::Ground,
        position: Vec3 { x, y: 0, z: 0 },
        body_rotation: 0,
        aim_pose: Pose {
            position: Vec3 { x, y: 0, z: 0 },
            rotation: 0,
        },
        velocity: Vec3 { x: 0, y: 0, z: 0 },
        motion_state: MotionState::Idle,
        collision_radius: 5,
        life,
        max_life: 100,
        alive: life > 0,
        active: true,
        targetable: life > 0,
        visibility: Visibility::Normal,
        personal_shield: PersonalShieldState {
            active: false,
            enabled: false,
            energy: 0,
            max_energy: 0,
        },
    }
}
