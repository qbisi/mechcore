use rust_hdf5::{H5File, H5Group, H5Type};

use crate::{
    BuildingState, Domain, Error, Event, EventKind, EventPayload, Gauge, MotionState, ObjectKind,
    ObjectRef, PersonalShieldState, Pose, ProjectileState, Result, StatusState, TransitionEvents,
    UnitState, Vec3, Visibility, WorldSnapshot,
};

const ROW_CHUNK: usize = 4096;
const TICK_CHUNK: usize = 1024;

const UNIT_COLUMNS: &[(&str, usize)] = &[
    ("unit_id", 1),
    ("team_id", 1),
    ("formation_id", 1),
    ("unit_type_id", 1),
    ("domain", 1),
    ("position", 3),
    ("body_rotation", 1),
    ("aim_position", 3),
    ("aim_rotation", 1),
    ("velocity", 3),
    ("motion_state", 1),
    ("collision_radius", 1),
    ("life", 1),
    ("max_life", 1),
    ("flags", 1),
    ("visibility", 1),
    ("shield_energy", 1),
    ("shield_max_energy", 1),
];
const PROJECTILE_COLUMNS: &[(&str, usize)] = &[
    ("projectile_id", 1),
    ("team_id", 1),
    ("owner_valid", 1),
    ("owner_kind", 1),
    ("owner_id", 1),
    ("position", 3),
    ("orientation", 1),
    ("target_valid", 1),
    ("target_kind", 1),
    ("target_id", 1),
    ("cached_target_position", 3),
    ("cached_target_radius", 1),
    ("released", 1),
    ("life_current", 1),
    ("life_maximum", 1),
];
const BUILDING_COLUMNS: &[(&str, usize)] = &[
    ("building_id", 1),
    ("team_id", 1),
    ("building_type_id", 1),
    ("position", 3),
    ("rotation", 1),
    ("bounds_width", 1),
    ("bounds_height", 1),
    ("life", 1),
    ("max_life", 1),
    ("flags", 1),
];
const STATUS_COLUMNS: &[(&str, usize)] = &[
    ("status_id", 1),
    ("status_type_id", 1),
    ("source_valid", 1),
    ("source_kind", 1),
    ("source_id", 1),
    ("target_kind", 1),
    ("target_id", 1),
    ("additive_stack", 1),
    ("duration_time", 1),
    ("max_duration_time", 1),
    ("step_time", 1),
    ("step_time_config", 1),
    ("flags", 1),
];
const EVENT_COLUMNS: &[(&str, usize)] = &[
    ("kind", 1),
    ("subject_valid", 1),
    ("subject_kind", 1),
    ("subject_id", 1),
    ("source_valid", 1),
    ("source_kind", 1),
    ("source_id", 1),
    ("target_valid", 1),
    ("target_kind", 1),
    ("target_id", 1),
    ("position", 3),
    ("intercepted", 1),
    ("amount", 1),
];

pub(crate) struct StorageReader {
    unit_offsets: Vec<u64>,
    projectile_offsets: Vec<u64>,
    building_offsets: Vec<u64>,
    status_offsets: Vec<u64>,
    event_offsets: Vec<u64>,
    tick_hashes: Vec<[u8; 32]>,
}

pub(crate) fn create(file: &H5File) -> Result<()> {
    let ticks = file.create_group("ticks")?;
    let states = file.create_group("states")?;
    let units = states.create_group("units")?;
    let projectiles = states.create_group("projectiles")?;
    let buildings = states.create_group("buildings")?;
    let statuses = states.create_group("statuses")?;
    let events = file.create_group("events")?;

    create_offsets(&ticks, "unit_offsets")?;
    create_offsets(&ticks, "projectile_offsets")?;
    create_offsets(&ticks, "building_offsets")?;
    create_offsets(&ticks, "status_offsets")?;
    create_offsets(&ticks, "event_offsets")?;
    ticks
        .new_dataset::<u8>()
        .shape([0, 32])
        .chunk(&[TICK_CHUNK, 32])
        .max_shape(&[None, Some(32)])
        .create("hash")?;

    create_unit_columns(&units)?;
    create_projectile_columns(&projectiles)?;
    create_building_columns(&buildings)?;
    create_status_columns(&statuses)?;
    create_event_columns(&events)?;
    Ok(())
}

pub(crate) fn append_tick(
    file: &H5File,
    state: &WorldSnapshot,
    events: &TransitionEvents,
    tick_hash: &[u8; 32],
) -> Result<()> {
    append_units(file, &state.units)?;
    append_projectiles(file, &state.projectiles)?;
    append_buildings(file, &state.buildings)?;
    append_statuses(file, &state.statuses)?;
    append_events(file, &events.events)?;

    append_offset(file, "ticks/unit_offsets", "states/units/unit_id")?;
    append_offset(
        file,
        "ticks/projectile_offsets",
        "states/projectiles/projectile_id",
    )?;
    append_offset(
        file,
        "ticks/building_offsets",
        "states/buildings/building_id",
    )?;
    append_offset(file, "ticks/status_offsets", "states/statuses/status_id")?;
    append_offset(file, "ticks/event_offsets", "events/kind")?;
    file.dataset_writer("ticks/hash")?.append(tick_hash)?;
    Ok(())
}

impl StorageReader {
    pub(crate) fn open(file: &H5File, tick_count: u64) -> Result<Self> {
        let unit_offsets = read_offsets(file, "ticks/unit_offsets", tick_count, "unit")?;
        let projectile_offsets =
            read_offsets(file, "ticks/projectile_offsets", tick_count, "projectile")?;
        let building_offsets =
            read_offsets(file, "ticks/building_offsets", tick_count, "building")?;
        let status_offsets = read_offsets(file, "ticks/status_offsets", tick_count, "status")?;
        let event_offsets = read_offsets(file, "ticks/event_offsets", tick_count, "event")?;
        validate_columns(
            file,
            "states/units",
            UNIT_COLUMNS,
            *unit_offsets.last().unwrap(),
        )?;
        validate_columns(
            file,
            "states/projectiles",
            PROJECTILE_COLUMNS,
            *projectile_offsets.last().unwrap(),
        )?;
        validate_columns(
            file,
            "states/buildings",
            BUILDING_COLUMNS,
            *building_offsets.last().unwrap(),
        )?;
        validate_columns(
            file,
            "states/statuses",
            STATUS_COLUMNS,
            *status_offsets.last().unwrap(),
        )?;
        validate_columns(
            file,
            "events",
            EVENT_COLUMNS,
            *event_offsets.last().unwrap(),
        )?;
        let hashes = file.dataset("ticks/hash")?;
        let tick_count_usize =
            usize::try_from(tick_count).map_err(|_| Error::invalid("tick count is too large"))?;
        if hashes.shape() != [tick_count_usize, 32] {
            return Err(Error::invalid("tick hash dataset has an invalid shape"));
        }
        let raw = hashes.read_raw::<u8>()?;
        let tick_hashes = raw
            .chunks_exact(32)
            .map(|bytes| bytes.try_into().expect("exact hash chunk"))
            .collect();
        Ok(Self {
            unit_offsets,
            projectile_offsets,
            building_offsets,
            status_offsets,
            event_offsets,
            tick_hashes,
        })
    }

    pub(crate) fn tick_hash(&self, tick: u64) -> Result<[u8; 32]> {
        self.tick_hashes
            .get(index(tick, "tick")?)
            .copied()
            .ok_or_else(|| Error::invalid(format!("tick index {tick} is out of range")))
    }

    pub(crate) fn tick_hashes(&self) -> &[[u8; 32]] {
        &self.tick_hashes
    }

    pub(crate) fn state(&self, file: &H5File, tick: u64) -> Result<WorldSnapshot> {
        Ok(WorldSnapshot {
            units: read_units(file, range(&self.unit_offsets, tick, "unit")?)?,
            projectiles: read_projectiles(
                file,
                range(&self.projectile_offsets, tick, "projectile")?,
            )?,
            buildings: read_buildings(file, range(&self.building_offsets, tick, "building")?)?,
            statuses: read_statuses(file, range(&self.status_offsets, tick, "status")?)?,
        })
    }

    pub(crate) fn events(&self, file: &H5File, tick: u64) -> Result<TransitionEvents> {
        Ok(TransitionEvents {
            events: read_events(file, range(&self.event_offsets, tick, "event")?)?,
        })
    }
}

fn create_offsets(group: &H5Group, name: &str) -> Result<()> {
    let dataset = group
        .new_dataset::<u64>()
        .shape([0])
        .chunk(&[TICK_CHUNK])
        .max_shape(&[None])
        .shuffle_deflate(1)
        .create(name)?;
    dataset.append(&[0_u64])?;
    Ok(())
}

fn create_scalar<T: H5Type>(group: &H5Group, name: &str) -> Result<()> {
    group
        .new_dataset::<T>()
        .shape([0])
        .chunk(&[ROW_CHUNK])
        .max_shape(&[None])
        .shuffle_deflate(1)
        .create(name)?;
    Ok(())
}

fn create_vec3(group: &H5Group, name: &str) -> Result<()> {
    group
        .new_dataset::<i64>()
        .shape([0, 3])
        .chunk(&[ROW_CHUNK, 3])
        .max_shape(&[None, Some(3)])
        .shuffle_deflate(1)
        .create(name)?;
    Ok(())
}

fn create_unit_columns(group: &H5Group) -> Result<()> {
    create_scalar::<u64>(group, "unit_id")?;
    create_scalar::<u32>(group, "team_id")?;
    create_scalar::<u64>(group, "formation_id")?;
    create_scalar::<u32>(group, "unit_type_id")?;
    create_scalar::<u8>(group, "domain")?;
    create_vec3(group, "position")?;
    create_scalar::<i64>(group, "body_rotation")?;
    create_vec3(group, "aim_position")?;
    create_scalar::<i64>(group, "aim_rotation")?;
    create_vec3(group, "velocity")?;
    create_scalar::<u8>(group, "motion_state")?;
    for name in ["collision_radius", "life", "max_life"] {
        create_scalar::<i64>(group, name)?;
    }
    create_scalar::<u8>(group, "flags")?;
    create_scalar::<u8>(group, "visibility")?;
    create_scalar::<i64>(group, "shield_energy")?;
    create_scalar::<i64>(group, "shield_max_energy")?;
    Ok(())
}

fn create_projectile_columns(group: &H5Group) -> Result<()> {
    create_scalar::<u64>(group, "projectile_id")?;
    create_scalar::<u32>(group, "team_id")?;
    for name in ["owner_valid", "owner_kind"] {
        create_scalar::<u8>(group, name)?;
    }
    create_scalar::<u64>(group, "owner_id")?;
    create_vec3(group, "position")?;
    create_scalar::<i64>(group, "orientation")?;
    for name in ["target_valid", "target_kind"] {
        create_scalar::<u8>(group, name)?;
    }
    create_scalar::<u64>(group, "target_id")?;
    create_vec3(group, "cached_target_position")?;
    for name in ["cached_target_radius", "life_current", "life_maximum"] {
        create_scalar::<i64>(group, name)?;
    }
    create_scalar::<u8>(group, "released")?;
    Ok(())
}

fn create_building_columns(group: &H5Group) -> Result<()> {
    create_scalar::<u64>(group, "building_id")?;
    for name in ["team_id", "building_type_id"] {
        create_scalar::<u32>(group, name)?;
    }
    create_vec3(group, "position")?;
    for name in [
        "rotation",
        "bounds_width",
        "bounds_height",
        "life",
        "max_life",
    ] {
        create_scalar::<i64>(group, name)?;
    }
    create_scalar::<u8>(group, "flags")?;
    Ok(())
}

fn create_status_columns(group: &H5Group) -> Result<()> {
    create_scalar::<u64>(group, "status_id")?;
    create_scalar::<u32>(group, "status_type_id")?;
    for name in ["source_valid", "source_kind"] {
        create_scalar::<u8>(group, name)?;
    }
    create_scalar::<u64>(group, "source_id")?;
    create_scalar::<u8>(group, "target_kind")?;
    create_scalar::<u64>(group, "target_id")?;
    for name in [
        "additive_stack",
        "duration_time",
        "max_duration_time",
        "step_time",
        "step_time_config",
    ] {
        create_scalar::<i32>(group, name)?;
    }
    create_scalar::<u8>(group, "flags")?;
    Ok(())
}

fn create_event_columns(group: &H5Group) -> Result<()> {
    for name in ["kind", "subject_valid", "subject_kind"] {
        create_scalar::<u8>(group, name)?;
    }
    create_scalar::<u64>(group, "subject_id")?;
    for name in ["source_valid", "source_kind"] {
        create_scalar::<u8>(group, name)?;
    }
    create_scalar::<u64>(group, "source_id")?;
    for name in ["target_valid", "target_kind"] {
        create_scalar::<u8>(group, name)?;
    }
    create_scalar::<u64>(group, "target_id")?;
    create_vec3(group, "position")?;
    create_scalar::<u8>(group, "intercepted")?;
    create_scalar::<i64>(group, "amount")?;
    Ok(())
}

fn append_offset(file: &H5File, path: &str, row_path: &str) -> Result<()> {
    let rows = file.dataset_writer(row_path)?.shape()[0];
    let end = u64::try_from(rows).map_err(|_| Error::invalid("row offset overflow"))?;
    file.dataset_writer(path)?.append(&[end])?;
    Ok(())
}

fn append<T: H5Type>(file: &H5File, path: &str, values: &[T]) -> Result<()> {
    if !values.is_empty() {
        let dataset = file.dataset_writer(path)?;
        if dataset.element_size() != T::element_size() {
            return Err(Error::invalid(format!(
                "{path} has element size {}, append uses {}",
                dataset.element_size(),
                T::element_size()
            )));
        }
        dataset.append(values)?;
    }
    Ok(())
}

fn append_vec3(file: &H5File, path: &str, values: impl Iterator<Item = Vec3>) -> Result<()> {
    let flat = values.flat_map(|v| [v.x, v.y, v.z]).collect::<Vec<_>>();
    append(file, path, &flat)
}

#[allow(clippy::too_many_lines)] // A direct field-to-column projection is clearer kept together.
fn append_units(file: &H5File, rows: &[UnitState]) -> Result<()> {
    append(
        file,
        "states/units/unit_id",
        &rows.iter().map(|v| v.unit_id).collect::<Vec<_>>(),
    )?;
    append(
        file,
        "states/units/team_id",
        &rows.iter().map(|v| v.team_id).collect::<Vec<_>>(),
    )?;
    append(
        file,
        "states/units/formation_id",
        &rows.iter().map(|v| v.formation_id).collect::<Vec<_>>(),
    )?;
    append(
        file,
        "states/units/unit_type_id",
        &rows.iter().map(|v| v.unit_type_id).collect::<Vec<_>>(),
    )?;
    append(
        file,
        "states/units/domain",
        &rows
            .iter()
            .map(|v| encode_domain(v.domain))
            .collect::<Vec<_>>(),
    )?;
    append_vec3(
        file,
        "states/units/position",
        rows.iter().map(|v| v.position),
    )?;
    append(
        file,
        "states/units/body_rotation",
        &rows.iter().map(|v| v.body_rotation).collect::<Vec<_>>(),
    )?;
    append_vec3(
        file,
        "states/units/aim_position",
        rows.iter().map(|v| v.aim_pose.position),
    )?;
    append(
        file,
        "states/units/aim_rotation",
        &rows.iter().map(|v| v.aim_pose.rotation).collect::<Vec<_>>(),
    )?;
    append_vec3(
        file,
        "states/units/velocity",
        rows.iter().map(|v| v.velocity),
    )?;
    append(
        file,
        "states/units/motion_state",
        &rows
            .iter()
            .map(|v| encode_motion(v.motion_state))
            .collect::<Vec<_>>(),
    )?;
    append(
        file,
        "states/units/collision_radius",
        &rows.iter().map(|v| v.collision_radius).collect::<Vec<_>>(),
    )?;
    append(
        file,
        "states/units/life",
        &rows.iter().map(|v| v.life).collect::<Vec<_>>(),
    )?;
    append(
        file,
        "states/units/max_life",
        &rows.iter().map(|v| v.max_life).collect::<Vec<_>>(),
    )?;
    append(
        file,
        "states/units/flags",
        &rows.iter().map(unit_flags).collect::<Vec<_>>(),
    )?;
    append(
        file,
        "states/units/visibility",
        &rows
            .iter()
            .map(|v| encode_visibility(v.visibility))
            .collect::<Vec<_>>(),
    )?;
    append(
        file,
        "states/units/shield_energy",
        &rows
            .iter()
            .map(|v| v.personal_shield.energy)
            .collect::<Vec<_>>(),
    )?;
    append(
        file,
        "states/units/shield_max_energy",
        &rows
            .iter()
            .map(|v| v.personal_shield.max_energy)
            .collect::<Vec<_>>(),
    )
}

fn append_projectiles(file: &H5File, rows: &[ProjectileState]) -> Result<()> {
    let owners = rows.iter().map(|v| encode_ref(v.owner)).collect::<Vec<_>>();
    let targets = rows
        .iter()
        .map(|v| encode_ref(v.target))
        .collect::<Vec<_>>();
    append(
        file,
        "states/projectiles/projectile_id",
        &rows.iter().map(|v| v.projectile_id).collect::<Vec<_>>(),
    )?;
    append(
        file,
        "states/projectiles/team_id",
        &rows.iter().map(|v| v.team_id).collect::<Vec<_>>(),
    )?;
    append_refs(file, "states/projectiles/owner", &owners)?;
    append_vec3(
        file,
        "states/projectiles/position",
        rows.iter().map(|v| v.position),
    )?;
    append(
        file,
        "states/projectiles/orientation",
        &rows.iter().map(|v| v.orientation).collect::<Vec<_>>(),
    )?;
    append_refs(file, "states/projectiles/target", &targets)?;
    append_vec3(
        file,
        "states/projectiles/cached_target_position",
        rows.iter().map(|v| v.cached_target_position),
    )?;
    append(
        file,
        "states/projectiles/cached_target_radius",
        &rows
            .iter()
            .map(|v| v.cached_target_radius)
            .collect::<Vec<_>>(),
    )?;
    append(
        file,
        "states/projectiles/released",
        &rows
            .iter()
            .map(|v| u8::from(v.released))
            .collect::<Vec<_>>(),
    )?;
    append(
        file,
        "states/projectiles/life_current",
        &rows.iter().map(|v| v.life.current).collect::<Vec<_>>(),
    )?;
    append(
        file,
        "states/projectiles/life_maximum",
        &rows.iter().map(|v| v.life.maximum).collect::<Vec<_>>(),
    )
}

fn append_buildings(file: &H5File, rows: &[BuildingState]) -> Result<()> {
    append(
        file,
        "states/buildings/building_id",
        &rows.iter().map(|v| v.building_id).collect::<Vec<_>>(),
    )?;
    append(
        file,
        "states/buildings/team_id",
        &rows.iter().map(|v| v.team_id).collect::<Vec<_>>(),
    )?;
    append(
        file,
        "states/buildings/building_type_id",
        &rows.iter().map(|v| v.building_type_id).collect::<Vec<_>>(),
    )?;
    append_vec3(
        file,
        "states/buildings/position",
        rows.iter().map(|v| v.position),
    )?;
    for (name, values) in [
        (
            "rotation",
            rows.iter().map(|v| v.rotation).collect::<Vec<_>>(),
        ),
        (
            "bounds_width",
            rows.iter().map(|v| v.bounds_width).collect::<Vec<_>>(),
        ),
        (
            "bounds_height",
            rows.iter().map(|v| v.bounds_height).collect::<Vec<_>>(),
        ),
        ("life", rows.iter().map(|v| v.life).collect::<Vec<_>>()),
        (
            "max_life",
            rows.iter().map(|v| v.max_life).collect::<Vec<_>>(),
        ),
    ] {
        append(file, &format!("states/buildings/{name}"), &values)?;
    }
    append(
        file,
        "states/buildings/flags",
        &rows.iter().map(building_flags).collect::<Vec<_>>(),
    )
}

fn append_statuses(file: &H5File, rows: &[StatusState]) -> Result<()> {
    let sources = rows
        .iter()
        .map(|v| encode_ref(v.source))
        .collect::<Vec<_>>();
    append(
        file,
        "states/statuses/status_id",
        &rows.iter().map(|v| v.status_id).collect::<Vec<_>>(),
    )?;
    append(
        file,
        "states/statuses/status_type_id",
        &rows.iter().map(|v| v.status_type_id).collect::<Vec<_>>(),
    )?;
    append_refs(file, "states/statuses/source", &sources)?;
    append(
        file,
        "states/statuses/target_kind",
        &rows
            .iter()
            .map(|v| encode_kind(v.target.kind))
            .collect::<Vec<_>>(),
    )?;
    append(
        file,
        "states/statuses/target_id",
        &rows.iter().map(|v| v.target.id).collect::<Vec<_>>(),
    )?;
    for (name, values) in [
        (
            "additive_stack",
            rows.iter().map(|v| v.additive_stack).collect::<Vec<_>>(),
        ),
        (
            "duration_time",
            rows.iter().map(|v| v.duration_time).collect::<Vec<_>>(),
        ),
        (
            "max_duration_time",
            rows.iter().map(|v| v.max_duration_time).collect::<Vec<_>>(),
        ),
        (
            "step_time",
            rows.iter().map(|v| v.step_time).collect::<Vec<_>>(),
        ),
        (
            "step_time_config",
            rows.iter().map(|v| v.step_time_config).collect::<Vec<_>>(),
        ),
    ] {
        append(file, &format!("states/statuses/{name}"), &values)?;
    }
    append(
        file,
        "states/statuses/flags",
        &rows
            .iter()
            .map(|v| u8::from(v.finished) | (u8::from(v.frozen) << 1))
            .collect::<Vec<_>>(),
    )
}

fn append_events(file: &H5File, rows: &[Event]) -> Result<()> {
    let subjects = rows
        .iter()
        .map(|v| encode_ref(v.subject))
        .collect::<Vec<_>>();
    let sources = rows
        .iter()
        .map(|v| encode_ref(v.source))
        .collect::<Vec<_>>();
    let targets = rows
        .iter()
        .map(|v| encode_ref(v.target))
        .collect::<Vec<_>>();
    append(
        file,
        "events/kind",
        &rows
            .iter()
            .map(|v| encode_event(v.kind()))
            .collect::<Vec<_>>(),
    )?;
    append_refs(file, "events/subject", &subjects)?;
    append_refs(file, "events/source", &sources)?;
    append_refs(file, "events/target", &targets)?;
    append_vec3(
        file,
        "events/position",
        rows.iter().map(|v| match v.payload {
            EventPayload::ProjectileRemoved { position, .. } => position,
            _ => Vec3 { x: 0, y: 0, z: 0 },
        }),
    )?;
    append(
        file,
        "events/intercepted",
        &rows
            .iter()
            .map(|v| match v.payload {
                EventPayload::ProjectileRemoved { intercepted, .. } => u8::from(intercepted),
                _ => 0,
            })
            .collect::<Vec<_>>(),
    )?;
    append(
        file,
        "events/amount",
        &rows
            .iter()
            .map(|v| match v.payload {
                EventPayload::Damage { amount } => amount,
                _ => 0,
            })
            .collect::<Vec<_>>(),
    )
}

fn append_refs(file: &H5File, prefix: &str, refs: &[(u8, u8, u64)]) -> Result<()> {
    append(
        file,
        &format!("{prefix}_valid"),
        &refs.iter().map(|v| v.0).collect::<Vec<_>>(),
    )?;
    append(
        file,
        &format!("{prefix}_kind"),
        &refs.iter().map(|v| v.1).collect::<Vec<_>>(),
    )?;
    append(
        file,
        &format!("{prefix}_id"),
        &refs.iter().map(|v| v.2).collect::<Vec<_>>(),
    )
}

fn read_units(file: &H5File, (start, len): (usize, usize)) -> Result<Vec<UnitState>> {
    let id = read::<u64>(file, "states/units/unit_id", start, len)?;
    let team = read::<u32>(file, "states/units/team_id", start, len)?;
    let formation = read::<u64>(file, "states/units/formation_id", start, len)?;
    let type_id = read::<u32>(file, "states/units/unit_type_id", start, len)?;
    let domain = read::<u8>(file, "states/units/domain", start, len)?;
    let position = read_vec3(file, "states/units/position", start, len)?;
    let body_rotation = read::<i64>(file, "states/units/body_rotation", start, len)?;
    let aim_position = read_vec3(file, "states/units/aim_position", start, len)?;
    let aim_rotation = read::<i64>(file, "states/units/aim_rotation", start, len)?;
    let velocity = read_vec3(file, "states/units/velocity", start, len)?;
    let motion = read::<u8>(file, "states/units/motion_state", start, len)?;
    let radius = read::<i64>(file, "states/units/collision_radius", start, len)?;
    let life = read::<i64>(file, "states/units/life", start, len)?;
    let max_life = read::<i64>(file, "states/units/max_life", start, len)?;
    let flags = read::<u8>(file, "states/units/flags", start, len)?;
    let visibility = read::<u8>(file, "states/units/visibility", start, len)?;
    let shield_energy = read::<i64>(file, "states/units/shield_energy", start, len)?;
    let shield_max = read::<i64>(file, "states/units/shield_max_energy", start, len)?;
    (0..len)
        .map(|i| {
            Ok(UnitState {
                unit_id: id[i],
                team_id: team[i],
                formation_id: formation[i],
                unit_type_id: type_id[i],
                domain: decode_domain(domain[i])?,
                position: position[i],
                body_rotation: body_rotation[i],
                aim_pose: Pose {
                    position: aim_position[i],
                    rotation: aim_rotation[i],
                },
                velocity: velocity[i],
                motion_state: decode_motion(motion[i])?,
                collision_radius: radius[i],
                life: life[i],
                max_life: max_life[i],
                alive: flag(flags[i], 0),
                active: flag(flags[i], 1),
                targetable: flag(flags[i], 2),
                visibility: decode_visibility(visibility[i])?,
                personal_shield: PersonalShieldState {
                    active: flag(flags[i], 3),
                    enabled: flag(flags[i], 4),
                    energy: shield_energy[i],
                    max_energy: shield_max[i],
                },
            })
        })
        .collect()
}

fn read_projectiles(file: &H5File, (start, len): (usize, usize)) -> Result<Vec<ProjectileState>> {
    let id = read::<u64>(file, "states/projectiles/projectile_id", start, len)?;
    let team = read::<u32>(file, "states/projectiles/team_id", start, len)?;
    let owner = read_refs(file, "states/projectiles/owner", start, len)?;
    let position = read_vec3(file, "states/projectiles/position", start, len)?;
    let orientation = read::<i64>(file, "states/projectiles/orientation", start, len)?;
    let target = read_refs(file, "states/projectiles/target", start, len)?;
    let cached = read_vec3(
        file,
        "states/projectiles/cached_target_position",
        start,
        len,
    )?;
    let radius = read::<i64>(file, "states/projectiles/cached_target_radius", start, len)?;
    let released = read::<u8>(file, "states/projectiles/released", start, len)?;
    let current = read::<i64>(file, "states/projectiles/life_current", start, len)?;
    let maximum = read::<i64>(file, "states/projectiles/life_maximum", start, len)?;
    Ok((0..len)
        .map(|i| ProjectileState {
            projectile_id: id[i],
            team_id: team[i],
            owner: owner[i],
            position: position[i],
            orientation: orientation[i],
            target: target[i],
            cached_target_position: cached[i],
            cached_target_radius: radius[i],
            released: released[i] != 0,
            life: Gauge {
                current: current[i],
                maximum: maximum[i],
            },
        })
        .collect())
}

fn read_buildings(file: &H5File, (start, len): (usize, usize)) -> Result<Vec<BuildingState>> {
    let id = read::<u64>(file, "states/buildings/building_id", start, len)?;
    let team = read::<u32>(file, "states/buildings/team_id", start, len)?;
    let type_id = read::<u32>(file, "states/buildings/building_type_id", start, len)?;
    let position = read_vec3(file, "states/buildings/position", start, len)?;
    let rotation = read::<i64>(file, "states/buildings/rotation", start, len)?;
    let width = read::<i64>(file, "states/buildings/bounds_width", start, len)?;
    let height = read::<i64>(file, "states/buildings/bounds_height", start, len)?;
    let life = read::<i64>(file, "states/buildings/life", start, len)?;
    let max_life = read::<i64>(file, "states/buildings/max_life", start, len)?;
    let flags = read::<u8>(file, "states/buildings/flags", start, len)?;
    Ok((0..len)
        .map(|i| BuildingState {
            building_id: id[i],
            team_id: team[i],
            building_type_id: type_id[i],
            position: position[i],
            rotation: rotation[i],
            bounds_width: width[i],
            bounds_height: height[i],
            life: life[i],
            max_life: max_life[i],
            alive: flag(flags[i], 0),
            destroyed: flag(flags[i], 1),
            available: flag(flags[i], 2),
            targetable: flag(flags[i], 3),
            collision_enabled: flag(flags[i], 4),
        })
        .collect())
}

fn read_statuses(file: &H5File, (start, len): (usize, usize)) -> Result<Vec<StatusState>> {
    let id = read::<u64>(file, "states/statuses/status_id", start, len)?;
    let type_id = read::<u32>(file, "states/statuses/status_type_id", start, len)?;
    let source = read_refs(file, "states/statuses/source", start, len)?;
    let target_kind = read::<u8>(file, "states/statuses/target_kind", start, len)?;
    let target_id = read::<u64>(file, "states/statuses/target_id", start, len)?;
    let stack = read::<i32>(file, "states/statuses/additive_stack", start, len)?;
    let duration = read::<i32>(file, "states/statuses/duration_time", start, len)?;
    let max_duration = read::<i32>(file, "states/statuses/max_duration_time", start, len)?;
    let step = read::<i32>(file, "states/statuses/step_time", start, len)?;
    let step_config = read::<i32>(file, "states/statuses/step_time_config", start, len)?;
    let flags = read::<u8>(file, "states/statuses/flags", start, len)?;
    (0..len)
        .map(|i| {
            Ok(StatusState {
                status_id: id[i],
                status_type_id: type_id[i],
                source: source[i],
                target: ObjectRef::new(decode_kind(target_kind[i])?, target_id[i]),
                additive_stack: stack[i],
                duration_time: duration[i],
                max_duration_time: max_duration[i],
                step_time: step[i],
                step_time_config: step_config[i],
                finished: flag(flags[i], 0),
                frozen: flag(flags[i], 1),
            })
        })
        .collect()
}

fn read_events(file: &H5File, (start, len): (usize, usize)) -> Result<Vec<Event>> {
    let kind = read::<u8>(file, "events/kind", start, len)?;
    let subject = read_refs(file, "events/subject", start, len)?;
    let source = read_refs(file, "events/source", start, len)?;
    let target = read_refs(file, "events/target", start, len)?;
    let position = read_vec3(file, "events/position", start, len)?;
    let intercepted = read::<u8>(file, "events/intercepted", start, len)?;
    let amount = read::<i64>(file, "events/amount", start, len)?;
    (0..len)
        .map(|i| {
            Ok(Event {
                subject: subject[i],
                source: source[i],
                target: target[i],
                payload: match decode_event(kind[i])? {
                    EventKind::ProjectileReleased => EventPayload::ProjectileReleased,
                    EventKind::ProjectileRemoved => EventPayload::ProjectileRemoved {
                        position: position[i],
                        intercepted: intercepted[i] != 0,
                    },
                    EventKind::Damage => EventPayload::Damage { amount: amount[i] },
                },
            })
        })
        .collect()
}

fn read<T: H5Type>(file: &H5File, path: &str, start: usize, len: usize) -> Result<Vec<T>> {
    if len == 0 {
        return Ok(Vec::new());
    }
    Ok(file.dataset(path)?.read_slice::<T>(&[start], &[len])?)
}

fn read_vec3(file: &H5File, path: &str, start: usize, len: usize) -> Result<Vec<Vec3>> {
    if len == 0 {
        return Ok(Vec::new());
    }
    let flat = file
        .dataset(path)?
        .read_slice::<i64>(&[start, 0], &[len, 3])?;
    Ok(flat
        .chunks_exact(3)
        .map(|v| Vec3 {
            x: v[0],
            y: v[1],
            z: v[2],
        })
        .collect())
}

fn read_refs(
    file: &H5File,
    prefix: &str,
    start: usize,
    len: usize,
) -> Result<Vec<Option<ObjectRef>>> {
    let valid = read::<u8>(file, &format!("{prefix}_valid"), start, len)?;
    let kind = read::<u8>(file, &format!("{prefix}_kind"), start, len)?;
    let id = read::<u64>(file, &format!("{prefix}_id"), start, len)?;
    (0..len)
        .map(|i| {
            if valid[i] == 0 {
                Ok(None)
            } else {
                Ok(Some(ObjectRef::new(decode_kind(kind[i])?, id[i])))
            }
        })
        .collect()
}

fn read_offsets(file: &H5File, path: &str, ticks: u64, label: &str) -> Result<Vec<u64>> {
    let offsets = file.dataset(path)?.read_raw::<u64>()?;
    let expected = usize::try_from(ticks)
        .unwrap_or(usize::MAX)
        .saturating_add(1);
    if offsets.len() != expected
        || offsets.first() != Some(&0)
        || offsets.windows(2).any(|v| v[0] > v[1])
    {
        return Err(Error::invalid(format!(
            "{label} offsets have an invalid shape or order"
        )));
    }
    Ok(offsets)
}

fn validate_columns(
    file: &H5File,
    group: &str,
    columns: &[(&str, usize)],
    rows: u64,
) -> Result<()> {
    let rows =
        usize::try_from(rows).map_err(|_| Error::invalid(format!("{group} row count overflow")))?;
    for &(name, width) in columns {
        let expected = if width == 1 {
            vec![rows]
        } else {
            vec![rows, width]
        };
        if file.dataset(&format!("{group}/{name}"))?.shape() != expected {
            return Err(Error::invalid(format!(
                "{group}/{name} has an invalid shape"
            )));
        }
    }
    Ok(())
}

fn range(offsets: &[u64], tick: u64, label: &str) -> Result<(usize, usize)> {
    let i = index(tick, label)?;
    let (&start, &end) = offsets
        .get(i)
        .zip(offsets.get(i + 1))
        .ok_or_else(|| Error::invalid(format!("tick {tick} is out of range")))?;
    Ok((
        usize::try_from(start).map_err(|_| Error::invalid("row offset overflow"))?,
        usize::try_from(end - start).map_err(|_| Error::invalid("row length overflow"))?,
    ))
}

fn index(value: u64, label: &str) -> Result<usize> {
    usize::try_from(value).map_err(|_| Error::invalid(format!("{label} index overflow")))
}

fn encode_ref(value: Option<ObjectRef>) -> (u8, u8, u64) {
    value.map_or((0, 0, 0), |v| (1, encode_kind(v.kind), v.id))
}
fn encode_kind(value: ObjectKind) -> u8 {
    match value {
        ObjectKind::Unit => 1,
        ObjectKind::Projectile => 2,
        ObjectKind::Building => 3,
        ObjectKind::Status => 4,
    }
}
fn decode_kind(value: u8) -> Result<ObjectKind> {
    match value {
        1 => Ok(ObjectKind::Unit),
        2 => Ok(ObjectKind::Projectile),
        3 => Ok(ObjectKind::Building),
        4 => Ok(ObjectKind::Status),
        _ => Err(Error::invalid(format!("invalid object kind {value}"))),
    }
}
fn encode_domain(value: Domain) -> u8 {
    match value {
        Domain::Ground => 0,
        Domain::Air => 1,
    }
}
fn decode_domain(value: u8) -> Result<Domain> {
    match value {
        0 => Ok(Domain::Ground),
        1 => Ok(Domain::Air),
        _ => Err(Error::invalid(format!("invalid domain {value}"))),
    }
}
fn encode_motion(value: MotionState) -> u8 {
    match value {
        MotionState::Idle => 0,
        MotionState::Moving => 1,
        MotionState::Attacking => 2,
        MotionState::Stopped => 3,
    }
}
fn decode_motion(value: u8) -> Result<MotionState> {
    match value {
        0 => Ok(MotionState::Idle),
        1 => Ok(MotionState::Moving),
        2 => Ok(MotionState::Attacking),
        3 => Ok(MotionState::Stopped),
        _ => Err(Error::invalid(format!("invalid motion state {value}"))),
    }
}
fn encode_visibility(value: Visibility) -> u8 {
    match value {
        Visibility::Normal => 0,
        Visibility::Disappear => 1,
        Visibility::Stealth => 2,
        Visibility::Hide => 3,
    }
}
fn decode_visibility(value: u8) -> Result<Visibility> {
    match value {
        0 => Ok(Visibility::Normal),
        1 => Ok(Visibility::Disappear),
        2 => Ok(Visibility::Stealth),
        3 => Ok(Visibility::Hide),
        _ => Err(Error::invalid(format!("invalid visibility {value}"))),
    }
}
fn encode_event(value: EventKind) -> u8 {
    match value {
        EventKind::ProjectileReleased => 0,
        EventKind::ProjectileRemoved => 1,
        EventKind::Damage => 2,
    }
}
fn decode_event(value: u8) -> Result<EventKind> {
    match value {
        0 => Ok(EventKind::ProjectileReleased),
        1 => Ok(EventKind::ProjectileRemoved),
        2 => Ok(EventKind::Damage),
        _ => Err(Error::invalid(format!("invalid event kind {value}"))),
    }
}
fn flag(value: u8, bit: u8) -> bool {
    value & (1 << bit) != 0
}
fn unit_flags(value: &UnitState) -> u8 {
    u8::from(value.alive)
        | (u8::from(value.active) << 1)
        | (u8::from(value.targetable) << 2)
        | (u8::from(value.personal_shield.active) << 3)
        | (u8::from(value.personal_shield.enabled) << 4)
}
fn building_flags(value: &BuildingState) -> u8 {
    u8::from(value.alive)
        | (u8::from(value.destroyed) << 1)
        | (u8::from(value.available) << 2)
        | (u8::from(value.targetable) << 3)
        | (u8::from(value.collision_enabled) << 4)
}
