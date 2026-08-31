use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs::File,
    io::{self, Read, Seek},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use arrow_array::{
    Array, ArrayRef, BooleanArray, FixedSizeBinaryArray, Int32Array, Int64Array, RecordBatch,
    StructArray, UInt8Array, UInt32Array, UInt64Array,
};
use arrow_buffer::NullBuffer;
use arrow_schema::{DataType, Field, Fields, Schema, SchemaRef};
use bytes::Bytes;
use parquet::{
    arrow::{ArrowWriter, arrow_reader::ParquetRecordBatchReaderBuilder},
    basic::{Compression, Encoding, ZstdLevel},
    file::{
        properties::WriterProperties,
        reader::{ChunkReader, Length},
    },
    schema::types::ColumnPath,
};
use tempfile::TempDir;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::{
    BuildingState, Domain, DurableContext, Error, Event, EventKind, EventPayload, Gauge, Hashes,
    MCFR_FORMAT, MotionState, ObjectKind, ObjectRef, PersonalShieldState, Pose, ProjectileState,
    Result, StatusState, TransitionEvents, UnitState, Vec3, Visibility, WorldSnapshot, canonical,
};

pub(crate) const MEMBER_NAMES: [&str; 6] = [
    "ticks.parquet",
    "units.parquet",
    "projectiles.parquet",
    "buildings.parquet",
    "statuses.parquet",
    "events.parquet",
];

const TICKS_PER_ROW_GROUP: u64 = 128;
const ROWS_PER_TICK_GROUP: usize = 128;

pub(crate) struct StorageWriter {
    directory: TempDir,
    units: Option<ArrowWriter<File>>,
    projectiles: Option<ArrowWriter<File>>,
    buildings: Option<ArrowWriter<File>>,
    statuses: Option<ArrowWriter<File>>,
    events: Option<ArrowWriter<File>>,
    unit_rows: Vec<(u64, UnitState)>,
    projectile_rows: Vec<(u64, ProjectileState)>,
    building_rows: Vec<(u64, BuildingState)>,
    status_rows: Vec<(u64, StatusState)>,
    event_rows: Vec<(u64, u32, Event)>,
    tick_hashes: Vec<[u8; canonical::HASH_BYTES]>,
}

impl StorageWriter {
    pub(crate) fn create(parent: &Path) -> Result<Self> {
        let directory = tempfile::Builder::new()
            .prefix(".mcfr-members-")
            .tempdir_in(parent)?;
        let units = create_member(
            directory.path().join("units.parquet"),
            unit_schema(),
            Track::Units,
        )?;
        let projectiles = create_member(
            directory.path().join("projectiles.parquet"),
            projectile_schema(),
            Track::Projectiles,
        )?;
        let buildings = create_member(
            directory.path().join("buildings.parquet"),
            building_schema(),
            Track::Buildings,
        )?;
        let statuses = create_member(
            directory.path().join("statuses.parquet"),
            status_schema(),
            Track::Statuses,
        )?;
        let events = create_member(
            directory.path().join("events.parquet"),
            event_schema(),
            Track::Events,
        )?;
        Ok(Self {
            directory,
            units: Some(units),
            projectiles: Some(projectiles),
            buildings: Some(buildings),
            statuses: Some(statuses),
            events: Some(events),
            unit_rows: Vec::new(),
            projectile_rows: Vec::new(),
            building_rows: Vec::new(),
            status_rows: Vec::new(),
            event_rows: Vec::new(),
            tick_hashes: Vec::new(),
        })
    }

    pub(crate) fn append_tick(
        &mut self,
        tick: u64,
        state: &WorldSnapshot,
        events: &TransitionEvents,
        tick_hash: [u8; canonical::HASH_BYTES],
    ) -> Result<()> {
        self.unit_rows
            .extend(state.units.iter().cloned().map(|row| (tick, row)));
        self.projectile_rows
            .extend(state.projectiles.iter().cloned().map(|row| (tick, row)));
        self.building_rows
            .extend(state.buildings.iter().cloned().map(|row| (tick, row)));
        self.status_rows
            .extend(state.statuses.iter().cloned().map(|row| (tick, row)));
        for (ordinal, event) in events.events.iter().cloned().enumerate() {
            self.event_rows.push((
                tick,
                u32::try_from(ordinal).map_err(|_| Error::invalid("event ordinal overflow"))?,
                event,
            ));
        }
        self.tick_hashes.push(tick_hash);
        if (tick + 1).is_multiple_of(TICKS_PER_ROW_GROUP) {
            self.flush()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        write_buffer(&mut self.units, unit_batch(&self.unit_rows)?)?;
        write_buffer(
            &mut self.projectiles,
            projectile_batch(&self.projectile_rows)?,
        )?;
        write_buffer(&mut self.buildings, building_batch(&self.building_rows)?)?;
        write_buffer(&mut self.statuses, status_batch(&self.status_rows)?)?;
        write_buffer(&mut self.events, event_batch(&self.event_rows)?)?;
        self.unit_rows.clear();
        self.projectile_rows.clear();
        self.building_rows.clear();
        self.status_rows.clear();
        self.event_rows.clear();
        Ok(())
    }

    pub(crate) fn finish(mut self, context_bytes: &[u8], hashes: &Hashes) -> Result<TempDir> {
        self.flush()?;
        close_writer(&mut self.units)?;
        close_writer(&mut self.projectiles)?;
        close_writer(&mut self.buildings)?;
        close_writer(&mut self.statuses)?;
        close_writer(&mut self.events)?;

        let tick_count = u64::try_from(self.tick_hashes.len())
            .map_err(|_| Error::invalid("tick count overflow"))?;
        let terminal_tick = tick_count
            .checked_sub(1)
            .ok_or_else(|| Error::invalid("an MCFR must contain tick zero"))?;
        let context_json = std::str::from_utf8(context_bytes)
            .map_err(|_| Error::invalid("canonical durable context is not UTF-8"))?;
        let metadata = HashMap::from([
            ("format".to_owned(), MCFR_FORMAT.to_owned()),
            ("durable_context".to_owned(), context_json.to_owned()),
            ("scenario_hash".to_owned(), hashes.scenario_hash.clone()),
            ("result_hash".to_owned(), hashes.result_hash.clone()),
            ("tick_count".to_owned(), tick_count.to_string()),
            ("terminal_tick".to_owned(), terminal_tick.to_string()),
        ]);
        let schema = Arc::new(Schema::new(tick_fields()).with_metadata(metadata));
        let mut ticks = create_member(
            self.directory.path().join("ticks.parquet"),
            schema,
            Track::Ticks,
        )?;
        for start in (0..self.tick_hashes.len()).step_by(ROWS_PER_TICK_GROUP) {
            let end = (start + ROWS_PER_TICK_GROUP).min(self.tick_hashes.len());
            let batch = tick_batch(start, &self.tick_hashes[start..end])?;
            ticks.write(&batch)?;
            ticks.flush()?;
        }
        ticks.close()?;
        Ok(self.directory)
    }
}

fn write_buffer(writer: &mut Option<ArrowWriter<File>>, batch: Option<RecordBatch>) -> Result<()> {
    if let Some(batch) = batch {
        let writer = writer
            .as_mut()
            .ok_or_else(|| Error::invalid("Parquet member writer is closed"))?;
        writer.write(&batch)?;
        writer.flush()?;
    }
    Ok(())
}

fn close_writer(writer: &mut Option<ArrowWriter<File>>) -> Result<()> {
    writer
        .take()
        .ok_or_else(|| Error::invalid("Parquet member writer is closed"))?
        .close()?;
    Ok(())
}

#[derive(Clone, Copy)]
enum Track {
    Ticks,
    Units,
    Projectiles,
    Buildings,
    Statuses,
    Events,
}

fn create_member(path: PathBuf, schema: SchemaRef, track: Track) -> Result<ArrowWriter<File>> {
    Ok(ArrowWriter::try_new(
        File::create(path)?,
        schema,
        Some(writer_properties(track)?),
    )?)
}

fn writer_properties(track: Track) -> Result<WriterProperties> {
    let mut builder = WriterProperties::builder()
        .set_compression(Compression::ZSTD(ZstdLevel::try_new(6)?))
        .set_dictionary_enabled(false)
        .set_max_row_group_row_count(Some(1_000_000));
    for path in dictionary_paths(track) {
        builder = builder.set_column_dictionary_enabled(ColumnPath::from(*path), true);
    }
    for path in delta_paths(track) {
        builder =
            builder.set_column_encoding(ColumnPath::from(*path), Encoding::DELTA_BINARY_PACKED);
    }
    Ok(builder.build())
}

fn dictionary_paths(track: Track) -> &'static [&'static str] {
    match track {
        Track::Ticks => &[],
        Track::Units => &[
            "team_id",
            "unit_type_id",
            "domain",
            "motion_state",
            "mech_lock_target.kind",
            "collision_radius",
            "max_life",
            "visibility",
            "personal_shield.energy",
            "personal_shield.max_energy",
        ],
        Track::Projectiles => &[
            "team_id",
            "owner.kind",
            "orientation",
            "target.kind",
            "cached_target_radius",
            "life.maximum",
        ],
        Track::Buildings => &[
            "team_id",
            "building_type_id",
            "rotation",
            "bounds_width",
            "bounds_height",
            "max_life",
        ],
        Track::Statuses => &["source.kind", "target.kind"],
        Track::Events => &["subject.kind", "source.kind", "target.kind", "payload.kind"],
    }
}

fn delta_paths(track: Track) -> &'static [&'static str] {
    match track {
        Track::Events => &["tick", "ordinal"],
        Track::Ticks | Track::Units | Track::Projectiles | Track::Buildings | Track::Statuses => {
            &["tick"]
        }
    }
}

fn tick_batch(start: usize, hashes: &[[u8; canonical::HASH_BYTES]]) -> Result<RecordBatch> {
    let ticks =
        UInt64Array::from_iter_values((start..start + hashes.len()).map(|value| value as u64));
    let hashes = FixedSizeBinaryArray::try_from_iter(
        hashes.iter().map(<[u8; canonical::HASH_BYTES]>::as_slice),
    )?;
    Ok(RecordBatch::try_new(
        Arc::new(Schema::new(tick_fields())),
        vec![Arc::new(ticks), Arc::new(hashes)],
    )?)
}

fn unit_batch(rows: &[(u64, UnitState)]) -> Result<Option<RecordBatch>> {
    if rows.is_empty() {
        return Ok(None);
    }
    let units = rows.iter().map(|(_, row)| row).collect::<Vec<_>>();
    Ok(Some(RecordBatch::try_new(
        unit_schema(),
        vec![
            u64_values(rows.iter().map(|(tick, _)| *tick)),
            u64_values(units.iter().map(|row| row.unit_id)),
            u32_values(units.iter().map(|row| row.team_id)),
            u64_values(units.iter().map(|row| row.formation_id)),
            u32_values(units.iter().map(|row| row.unit_type_id)),
            u8_values(units.iter().map(|row| encode_domain(row.domain))),
            vec3_values(units.iter().map(|row| row.position)),
            i64_values(units.iter().map(|row| row.body_rotation)),
            pose_values(units.iter().map(|row| row.aim_pose)),
            vec3_values(units.iter().map(|row| row.velocity)),
            u8_values(units.iter().map(|row| encode_motion(row.motion_state))),
            object_ref_values(units.iter().map(|row| row.mech_lock_target)),
            i64_values(units.iter().map(|row| row.collision_radius)),
            i64_values(units.iter().map(|row| row.life)),
            i64_values(units.iter().map(|row| row.max_life)),
            bool_values(units.iter().map(|row| row.alive)),
            bool_values(units.iter().map(|row| row.active)),
            bool_values(units.iter().map(|row| row.targetable)),
            u8_values(units.iter().map(|row| encode_visibility(row.visibility))),
            shield_values(units.iter().map(|row| row.personal_shield)),
        ],
    )?))
}

fn projectile_batch(rows: &[(u64, ProjectileState)]) -> Result<Option<RecordBatch>> {
    if rows.is_empty() {
        return Ok(None);
    }
    let values = rows.iter().map(|(_, row)| row).collect::<Vec<_>>();
    Ok(Some(RecordBatch::try_new(
        projectile_schema(),
        vec![
            u64_values(rows.iter().map(|(tick, _)| *tick)),
            u64_values(values.iter().map(|row| row.projectile_id)),
            u32_values(values.iter().map(|row| row.team_id)),
            object_ref_values(values.iter().map(|row| row.owner)),
            vec3_values(values.iter().map(|row| row.position)),
            i64_values(values.iter().map(|row| row.orientation)),
            object_ref_values(values.iter().map(|row| row.target)),
            vec3_values(values.iter().map(|row| row.cached_target_position)),
            i64_values(values.iter().map(|row| row.cached_target_radius)),
            bool_values(values.iter().map(|row| row.released)),
            gauge_values(values.iter().map(|row| row.life)),
        ],
    )?))
}

fn building_batch(rows: &[(u64, BuildingState)]) -> Result<Option<RecordBatch>> {
    if rows.is_empty() {
        return Ok(None);
    }
    let values = rows.iter().map(|(_, row)| row).collect::<Vec<_>>();
    Ok(Some(RecordBatch::try_new(
        building_schema(),
        vec![
            u64_values(rows.iter().map(|(tick, _)| *tick)),
            u64_values(values.iter().map(|row| row.building_id)),
            u32_values(values.iter().map(|row| row.team_id)),
            u32_values(values.iter().map(|row| row.building_type_id)),
            vec3_values(values.iter().map(|row| row.position)),
            i64_values(values.iter().map(|row| row.rotation)),
            i64_values(values.iter().map(|row| row.bounds_width)),
            i64_values(values.iter().map(|row| row.bounds_height)),
            i64_values(values.iter().map(|row| row.life)),
            i64_values(values.iter().map(|row| row.max_life)),
            bool_values(values.iter().map(|row| row.alive)),
            bool_values(values.iter().map(|row| row.destroyed)),
            bool_values(values.iter().map(|row| row.available)),
            bool_values(values.iter().map(|row| row.targetable)),
            bool_values(values.iter().map(|row| row.collision_enabled)),
        ],
    )?))
}

fn status_batch(rows: &[(u64, StatusState)]) -> Result<Option<RecordBatch>> {
    if rows.is_empty() {
        return Ok(None);
    }
    let values = rows.iter().map(|(_, row)| row).collect::<Vec<_>>();
    Ok(Some(RecordBatch::try_new(
        status_schema(),
        vec![
            u64_values(rows.iter().map(|(tick, _)| *tick)),
            u64_values(values.iter().map(|row| row.status_id)),
            u32_values(values.iter().map(|row| row.status_type_id)),
            object_ref_values(values.iter().map(|row| row.source)),
            required_ref_values(values.iter().map(|row| row.target)),
            i32_values(values.iter().map(|row| row.additive_stack)),
            i32_values(values.iter().map(|row| row.duration_time)),
            i32_values(values.iter().map(|row| row.max_duration_time)),
            i32_values(values.iter().map(|row| row.step_time)),
            i32_values(values.iter().map(|row| row.step_time_config)),
            bool_values(values.iter().map(|row| row.finished)),
            bool_values(values.iter().map(|row| row.frozen)),
        ],
    )?))
}

fn event_batch(rows: &[(u64, u32, Event)]) -> Result<Option<RecordBatch>> {
    if rows.is_empty() {
        return Ok(None);
    }
    let values = rows.iter().map(|(_, _, row)| row).collect::<Vec<_>>();
    Ok(Some(RecordBatch::try_new(
        event_schema(),
        vec![
            u64_values(rows.iter().map(|(tick, _, _)| *tick)),
            u32_values(rows.iter().map(|(_, ordinal, _)| *ordinal)),
            object_ref_values(values.iter().map(|row| row.subject)),
            object_ref_values(values.iter().map(|row| row.source)),
            object_ref_values(values.iter().map(|row| row.target)),
            event_payload_values(values.iter().map(|row| &row.payload)),
        ],
    )?))
}

fn u8_values(values: impl IntoIterator<Item = u8>) -> ArrayRef {
    Arc::new(UInt8Array::from_iter_values(values))
}

fn u32_values(values: impl IntoIterator<Item = u32>) -> ArrayRef {
    Arc::new(UInt32Array::from_iter_values(values))
}

fn u64_values(values: impl IntoIterator<Item = u64>) -> ArrayRef {
    Arc::new(UInt64Array::from_iter_values(values))
}

fn i32_values(values: impl IntoIterator<Item = i32>) -> ArrayRef {
    Arc::new(Int32Array::from_iter_values(values))
}

fn i64_values(values: impl IntoIterator<Item = i64>) -> ArrayRef {
    Arc::new(Int64Array::from_iter_values(values))
}

fn bool_values(values: impl IntoIterator<Item = bool>) -> ArrayRef {
    Arc::new(BooleanArray::from(values.into_iter().collect::<Vec<_>>()))
}

fn vec3_values(values: impl IntoIterator<Item = Vec3>) -> ArrayRef {
    let values = values.into_iter().collect::<Vec<_>>();
    Arc::new(StructArray::new(
        vec3_fields(),
        vec![
            i64_values(values.iter().map(|value| value.x)),
            i64_values(values.iter().map(|value| value.y)),
            i64_values(values.iter().map(|value| value.z)),
        ],
        None,
    ))
}

fn pose_values(values: impl IntoIterator<Item = Pose>) -> ArrayRef {
    let values = values.into_iter().collect::<Vec<_>>();
    Arc::new(StructArray::new(
        pose_fields(),
        vec![
            vec3_values(values.iter().map(|value| value.position)),
            i64_values(values.iter().map(|value| value.rotation)),
        ],
        None,
    ))
}

fn shield_values(values: impl IntoIterator<Item = PersonalShieldState>) -> ArrayRef {
    let values = values.into_iter().collect::<Vec<_>>();
    Arc::new(StructArray::new(
        shield_fields(),
        vec![
            bool_values(values.iter().map(|value| value.active)),
            bool_values(values.iter().map(|value| value.enabled)),
            i64_values(values.iter().map(|value| value.energy)),
            i64_values(values.iter().map(|value| value.max_energy)),
        ],
        None,
    ))
}

fn gauge_values(values: impl IntoIterator<Item = Gauge>) -> ArrayRef {
    let values = values.into_iter().collect::<Vec<_>>();
    Arc::new(StructArray::new(
        gauge_fields(),
        vec![
            i64_values(values.iter().map(|value| value.current)),
            i64_values(values.iter().map(|value| value.maximum)),
        ],
        None,
    ))
}

fn required_ref_values(values: impl IntoIterator<Item = ObjectRef>) -> ArrayRef {
    let values = values.into_iter().collect::<Vec<_>>();
    Arc::new(StructArray::new(
        object_ref_fields(),
        vec![
            u8_values(values.iter().map(|value| encode_kind(value.kind))),
            u64_values(values.iter().map(|value| value.id)),
        ],
        None,
    ))
}

fn object_ref_values(values: impl IntoIterator<Item = Option<ObjectRef>>) -> ArrayRef {
    let values = values.into_iter().collect::<Vec<_>>();
    Arc::new(StructArray::new(
        object_ref_fields(),
        vec![
            u8_values(
                values
                    .iter()
                    .map(|value| value.map_or(0, |value| encode_kind(value.kind))),
            ),
            u64_values(values.iter().map(|value| value.map_or(0, |value| value.id))),
        ],
        Some(values.iter().map(Option::is_some).collect::<NullBuffer>()),
    ))
}

fn event_payload_values<'a>(values: impl IntoIterator<Item = &'a EventPayload>) -> ArrayRef {
    let values = values.into_iter().collect::<Vec<_>>();
    let positions = values
        .iter()
        .map(|value| match value {
            EventPayload::ProjectileRemoved { position, .. } => Some(*position),
            EventPayload::ProjectileReleased | EventPayload::Damage { .. } => None,
        })
        .collect::<Vec<_>>();
    let position_values = positions
        .iter()
        .map(|value| value.unwrap_or(Vec3 { x: 0, y: 0, z: 0 }))
        .collect::<Vec<_>>();
    let position = StructArray::new(
        vec3_fields(),
        vec![
            i64_values(position_values.iter().map(|value| value.x)),
            i64_values(position_values.iter().map(|value| value.y)),
            i64_values(position_values.iter().map(|value| value.z)),
        ],
        Some(
            positions
                .iter()
                .map(Option::is_some)
                .collect::<NullBuffer>(),
        ),
    );
    let intercepted = BooleanArray::from(
        values
            .iter()
            .map(|value| match value {
                EventPayload::ProjectileRemoved { intercepted, .. } => Some(*intercepted),
                EventPayload::ProjectileReleased | EventPayload::Damage { .. } => None,
            })
            .collect::<Vec<_>>(),
    );
    let amount = Int64Array::from(
        values
            .iter()
            .map(|value| match value {
                EventPayload::Damage { amount } => Some(*amount),
                EventPayload::ProjectileReleased | EventPayload::ProjectileRemoved { .. } => None,
            })
            .collect::<Vec<_>>(),
    );
    Arc::new(StructArray::new(
        event_payload_fields(),
        vec![
            u8_values(values.iter().map(|value| encode_event(value.kind()))),
            Arc::new(position),
            Arc::new(intercepted),
            Arc::new(amount),
        ],
        None,
    ))
}

fn tick_fields() -> Vec<Field> {
    vec![
        Field::new("tick", DataType::UInt64, false),
        Field::new("tick_hash", DataType::FixedSizeBinary(32), false),
    ]
}

fn unit_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("tick", DataType::UInt64, false),
        Field::new("unit_id", DataType::UInt64, false),
        Field::new("team_id", DataType::UInt32, false),
        Field::new("formation_id", DataType::UInt64, false),
        Field::new("unit_type_id", DataType::UInt32, false),
        Field::new("domain", DataType::UInt8, false),
        struct_field("position", vec3_fields(), false),
        Field::new("body_rotation", DataType::Int64, false),
        struct_field("aim_pose", pose_fields(), false),
        struct_field("velocity", vec3_fields(), false),
        Field::new("motion_state", DataType::UInt8, false),
        struct_field("mech_lock_target", object_ref_fields(), true),
        Field::new("collision_radius", DataType::Int64, false),
        Field::new("life", DataType::Int64, false),
        Field::new("max_life", DataType::Int64, false),
        Field::new("alive", DataType::Boolean, false),
        Field::new("active", DataType::Boolean, false),
        Field::new("targetable", DataType::Boolean, false),
        Field::new("visibility", DataType::UInt8, false),
        struct_field("personal_shield", shield_fields(), false),
    ]))
}

fn projectile_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("tick", DataType::UInt64, false),
        Field::new("projectile_id", DataType::UInt64, false),
        Field::new("team_id", DataType::UInt32, false),
        struct_field("owner", object_ref_fields(), true),
        struct_field("position", vec3_fields(), false),
        Field::new("orientation", DataType::Int64, false),
        struct_field("target", object_ref_fields(), true),
        struct_field("cached_target_position", vec3_fields(), false),
        Field::new("cached_target_radius", DataType::Int64, false),
        Field::new("released", DataType::Boolean, false),
        struct_field("life", gauge_fields(), false),
    ]))
}

fn building_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("tick", DataType::UInt64, false),
        Field::new("building_id", DataType::UInt64, false),
        Field::new("team_id", DataType::UInt32, false),
        Field::new("building_type_id", DataType::UInt32, false),
        struct_field("position", vec3_fields(), false),
        Field::new("rotation", DataType::Int64, false),
        Field::new("bounds_width", DataType::Int64, false),
        Field::new("bounds_height", DataType::Int64, false),
        Field::new("life", DataType::Int64, false),
        Field::new("max_life", DataType::Int64, false),
        Field::new("alive", DataType::Boolean, false),
        Field::new("destroyed", DataType::Boolean, false),
        Field::new("available", DataType::Boolean, false),
        Field::new("targetable", DataType::Boolean, false),
        Field::new("collision_enabled", DataType::Boolean, false),
    ]))
}

fn status_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("tick", DataType::UInt64, false),
        Field::new("status_id", DataType::UInt64, false),
        Field::new("status_type_id", DataType::UInt32, false),
        struct_field("source", object_ref_fields(), true),
        struct_field("target", object_ref_fields(), false),
        Field::new("additive_stack", DataType::Int32, false),
        Field::new("duration_time", DataType::Int32, false),
        Field::new("max_duration_time", DataType::Int32, false),
        Field::new("step_time", DataType::Int32, false),
        Field::new("step_time_config", DataType::Int32, false),
        Field::new("finished", DataType::Boolean, false),
        Field::new("frozen", DataType::Boolean, false),
    ]))
}

fn event_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("tick", DataType::UInt64, false),
        Field::new("ordinal", DataType::UInt32, false),
        struct_field("subject", object_ref_fields(), true),
        struct_field("source", object_ref_fields(), true),
        struct_field("target", object_ref_fields(), true),
        struct_field("payload", event_payload_fields(), false),
    ]))
}

fn vec3_fields() -> Fields {
    vec![
        Field::new("x", DataType::Int64, false),
        Field::new("y", DataType::Int64, false),
        Field::new("z", DataType::Int64, false),
    ]
    .into()
}

fn pose_fields() -> Fields {
    vec![
        struct_field("position", vec3_fields(), false),
        Field::new("rotation", DataType::Int64, false),
    ]
    .into()
}

fn object_ref_fields() -> Fields {
    vec![
        Field::new("kind", DataType::UInt8, false),
        Field::new("id", DataType::UInt64, false),
    ]
    .into()
}

fn shield_fields() -> Fields {
    vec![
        Field::new("active", DataType::Boolean, false),
        Field::new("enabled", DataType::Boolean, false),
        Field::new("energy", DataType::Int64, false),
        Field::new("max_energy", DataType::Int64, false),
    ]
    .into()
}

fn gauge_fields() -> Fields {
    vec![
        Field::new("current", DataType::Int64, false),
        Field::new("maximum", DataType::Int64, false),
    ]
    .into()
}

fn event_payload_fields() -> Fields {
    vec![
        Field::new("kind", DataType::UInt8, false),
        struct_field("position", vec3_fields(), true),
        Field::new("intercepted", DataType::Boolean, true),
        Field::new("amount", DataType::Int64, true),
    ]
    .into()
}

fn struct_field(name: &str, fields: Fields, nullable: bool) -> Field {
    Field::new(name, DataType::Struct(fields), nullable)
}

pub(crate) fn package_members(directory: &Path, output: &Path) -> Result<()> {
    let file = File::create(output)?;
    let mut archive = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .large_file(true)
        .last_modified_time(zip::DateTime::default());
    for name in MEMBER_NAMES {
        archive.start_file(name, options)?;
        let mut member = File::open(directory.join(name))?;
        io::copy(&mut member, &mut archive)?;
    }
    archive.finish()?.sync_all()?;
    Ok(())
}

pub(crate) struct StoredMetadata {
    pub(crate) context: DurableContext,
    pub(crate) tick_count: u64,
    pub(crate) terminal_tick: u64,
    pub(crate) hashes: Hashes,
}

pub(crate) struct StorageReader {
    metadata: StoredMetadata,
    tick_hashes: Vec<[u8; canonical::HASH_BYTES]>,
    units: Vec<Vec<UnitState>>,
    projectiles: Vec<Vec<ProjectileState>>,
    buildings: Vec<Vec<BuildingState>>,
    statuses: Vec<Vec<StatusState>>,
    events: Vec<Vec<Event>>,
}

impl StorageReader {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let members = open_members(path)?;
        let ticks = members
            .get("ticks.parquet")
            .ok_or_else(|| Error::invalid("missing ticks.parquet"))?;
        let (metadata, tick_hashes) = read_ticks(ticks.clone())?;
        let tick_count = metadata.tick_count;
        let units = group_state_rows(
            read_units(member(&members, "units.parquet")?)?,
            tick_count,
            |row| row.unit_id,
            "unit",
        )?;
        let projectiles = group_state_rows(
            read_projectiles(member(&members, "projectiles.parquet")?)?,
            tick_count,
            |row| row.projectile_id,
            "projectile",
        )?;
        let buildings = group_state_rows(
            read_buildings(member(&members, "buildings.parquet")?)?,
            tick_count,
            |row| row.building_id,
            "building",
        )?;
        let statuses = group_state_rows(
            read_statuses(member(&members, "statuses.parquet")?)?,
            tick_count,
            |row| row.status_id,
            "status",
        )?;
        let events = group_event_rows(
            read_events(member(&members, "events.parquet")?)?,
            tick_count,
        )?;
        Ok(Self {
            metadata,
            tick_hashes,
            units,
            projectiles,
            buildings,
            statuses,
            events,
        })
    }

    pub(crate) const fn metadata(&self) -> &StoredMetadata {
        &self.metadata
    }

    pub(crate) fn tick_hash(&self, tick: u64) -> Result<[u8; canonical::HASH_BYTES]> {
        self.tick_hashes
            .get(tick_index(tick, self.metadata.tick_count)?)
            .copied()
            .ok_or_else(|| Error::invalid(format!("tick {tick} is out of range")))
    }

    pub(crate) fn tick_hashes(&self) -> &[[u8; canonical::HASH_BYTES]] {
        &self.tick_hashes
    }

    pub(crate) fn state(&self, tick: u64) -> Result<WorldSnapshot> {
        let index = tick_index(tick, self.metadata.tick_count)?;
        Ok(WorldSnapshot {
            units: self.units[index].clone(),
            projectiles: self.projectiles[index].clone(),
            buildings: self.buildings[index].clone(),
            statuses: self.statuses[index].clone(),
        })
    }

    pub(crate) fn events(&self, tick: u64) -> Result<TransitionEvents> {
        let index = tick_index(tick, self.metadata.tick_count)?;
        Ok(TransitionEvents {
            events: self.events[index].clone(),
        })
    }
}

fn member(members: &BTreeMap<String, MemberSlice>, name: &str) -> Result<MemberSlice> {
    members
        .get(name)
        .cloned()
        .ok_or_else(|| Error::invalid(format!("missing {name}")))
}

fn tick_index(tick: u64, tick_count: u64) -> Result<usize> {
    if tick >= tick_count {
        return Err(Error::invalid(format!("tick {tick} is out of range")));
    }
    usize::try_from(tick).map_err(|_| Error::invalid("tick index is too large"))
}

fn read_ticks(member: MemberSlice) -> Result<(StoredMetadata, Vec<[u8; canonical::HASH_BYTES]>)> {
    let builder = checked_builder(member, &Schema::new(tick_fields()), "ticks")?;
    let metadata = builder.schema().metadata();
    let format = required_metadata(metadata, "format")?;
    if format != MCFR_FORMAT {
        return Err(Error::invalid(format!(
            "unsupported MCFR format {format:?}"
        )));
    }
    let context_bytes = required_metadata(metadata, "durable_context")?.as_bytes();
    let context: DurableContext = canonical::decode(context_bytes, "durable context")?;
    context.validate()?;
    let hashes = Hashes {
        scenario_hash: required_metadata(metadata, "scenario_hash")?.to_owned(),
        result_hash: required_metadata(metadata, "result_hash")?.to_owned(),
    };
    hashes.validate_encoding()?;
    let tick_count = parse_metadata_u64(metadata, "tick_count")?;
    let terminal_tick = parse_metadata_u64(metadata, "terminal_tick")?;
    if tick_count == 0 || terminal_tick != tick_count - 1 {
        return Err(Error::invalid(
            "MCFR must contain tick zero and terminal_tick must be the final tick",
        ));
    }
    let mut hashes_out = Vec::new();
    let mut expected_tick = 0_u64;
    for batch in builder.build()? {
        let batch = batch?;
        let ticks = column::<UInt64Array>(&batch, "tick")?;
        let hashes = column::<FixedSizeBinaryArray>(&batch, "tick_hash")?;
        for index in 0..batch.num_rows() {
            if ticks.value(index) != expected_tick {
                return Err(Error::invalid(format!(
                    "ticks.parquet expected tick {expected_tick}, found {}",
                    ticks.value(index)
                )));
            }
            let value: [u8; canonical::HASH_BYTES] = hashes
                .value(index)
                .try_into()
                .map_err(|_| Error::invalid("tick_hash is not 32 bytes"))?;
            hashes_out.push(value);
            expected_tick += 1;
        }
    }
    if expected_tick != tick_count {
        return Err(Error::invalid(format!(
            "tick_count metadata is {tick_count}, found {expected_tick} tick rows"
        )));
    }
    Ok((
        StoredMetadata {
            context,
            tick_count,
            terminal_tick,
            hashes,
        },
        hashes_out,
    ))
}

fn required_metadata<'a>(metadata: &'a HashMap<String, String>, name: &str) -> Result<&'a str> {
    metadata
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| Error::invalid(format!("ticks.parquet metadata lacks {name}")))
}

fn parse_metadata_u64(metadata: &HashMap<String, String>, name: &str) -> Result<u64> {
    required_metadata(metadata, name)?
        .parse()
        .map_err(|_| Error::invalid(format!("ticks.parquet metadata {name} is not a u64")))
}

fn group_state_rows<T>(
    rows: Vec<(u64, T)>,
    tick_count: u64,
    identity: impl Fn(&T) -> u64,
    label: &str,
) -> Result<Vec<Vec<T>>> {
    let len = usize::try_from(tick_count).map_err(|_| Error::invalid("tick count is too large"))?;
    let mut grouped = (0..len).map(|_| Vec::new()).collect::<Vec<Vec<T>>>();
    let mut previous = None::<(u64, u64)>;
    for (tick, row) in rows {
        let id = identity(&row);
        let index = tick_index(tick, tick_count)?;
        if let Some((old_tick, old_id)) = previous
            && (tick < old_tick || (tick == old_tick && id <= old_id))
        {
            return Err(Error::invalid(format!(
                "{label} rows are not strictly ordered by (tick, id)"
            )));
        }
        previous = Some((tick, id));
        grouped[index].push(row);
    }
    Ok(grouped)
}

fn group_event_rows(rows: Vec<(u64, u32, Event)>, tick_count: u64) -> Result<Vec<Vec<Event>>> {
    let len = usize::try_from(tick_count).map_err(|_| Error::invalid("tick count is too large"))?;
    let mut grouped = (0..len).map(|_| Vec::new()).collect::<Vec<Vec<Event>>>();
    let mut previous_tick = None;
    let mut expected_ordinal = 0_u32;
    for (tick, ordinal, event) in rows {
        let index = tick_index(tick, tick_count)?;
        if previous_tick != Some(tick) {
            if previous_tick.is_some_and(|old| tick <= old) {
                return Err(Error::invalid("event rows are not ordered by tick"));
            }
            previous_tick = Some(tick);
            expected_ordinal = 0;
        }
        if ordinal != expected_ordinal {
            return Err(Error::invalid(format!(
                "event tick {tick} expected ordinal {expected_ordinal}, found {ordinal}"
            )));
        }
        expected_ordinal = expected_ordinal
            .checked_add(1)
            .ok_or_else(|| Error::invalid("event ordinal overflow"))?;
        grouped[index].push(event);
    }
    Ok(grouped)
}

fn read_units(member: MemberSlice) -> Result<Vec<(u64, UnitState)>> {
    let mut rows = Vec::new();
    for batch in checked_builder(member, unit_schema().as_ref(), "units")?.build()? {
        let batch = batch?;
        let tick = column::<UInt64Array>(&batch, "tick")?;
        let id = column::<UInt64Array>(&batch, "unit_id")?;
        let team = column::<UInt32Array>(&batch, "team_id")?;
        let formation = column::<UInt64Array>(&batch, "formation_id")?;
        let type_id = column::<UInt32Array>(&batch, "unit_type_id")?;
        let domain = column::<UInt8Array>(&batch, "domain")?;
        let position = struct_column(&batch, "position")?;
        let body_rotation = column::<Int64Array>(&batch, "body_rotation")?;
        let aim_pose = struct_column(&batch, "aim_pose")?;
        let velocity = struct_column(&batch, "velocity")?;
        let motion = column::<UInt8Array>(&batch, "motion_state")?;
        let target = struct_column(&batch, "mech_lock_target")?;
        let radius = column::<Int64Array>(&batch, "collision_radius")?;
        let life = column::<Int64Array>(&batch, "life")?;
        let max_life = column::<Int64Array>(&batch, "max_life")?;
        let alive = column::<BooleanArray>(&batch, "alive")?;
        let active = column::<BooleanArray>(&batch, "active")?;
        let targetable = column::<BooleanArray>(&batch, "targetable")?;
        let visibility = column::<UInt8Array>(&batch, "visibility")?;
        let shield = struct_column(&batch, "personal_shield")?;
        for index in 0..batch.num_rows() {
            rows.push((
                tick.value(index),
                UnitState {
                    unit_id: id.value(index),
                    team_id: team.value(index),
                    formation_id: formation.value(index),
                    unit_type_id: type_id.value(index),
                    domain: decode_domain(domain.value(index))?,
                    position: read_vec3(position, index)?,
                    body_rotation: body_rotation.value(index),
                    aim_pose: read_pose(aim_pose, index)?,
                    velocity: read_vec3(velocity, index)?,
                    motion_state: decode_motion(motion.value(index))?,
                    mech_lock_target: read_optional_ref(target, index)?,
                    collision_radius: radius.value(index),
                    life: life.value(index),
                    max_life: max_life.value(index),
                    alive: alive.value(index),
                    active: active.value(index),
                    targetable: targetable.value(index),
                    visibility: decode_visibility(visibility.value(index))?,
                    personal_shield: read_shield(shield, index)?,
                },
            ));
        }
    }
    Ok(rows)
}

fn read_projectiles(member: MemberSlice) -> Result<Vec<(u64, ProjectileState)>> {
    let mut rows = Vec::new();
    for batch in checked_builder(member, projectile_schema().as_ref(), "projectiles")?.build()? {
        let batch = batch?;
        let tick = column::<UInt64Array>(&batch, "tick")?;
        let id = column::<UInt64Array>(&batch, "projectile_id")?;
        let team = column::<UInt32Array>(&batch, "team_id")?;
        let owner = struct_column(&batch, "owner")?;
        let position = struct_column(&batch, "position")?;
        let orientation = column::<Int64Array>(&batch, "orientation")?;
        let target = struct_column(&batch, "target")?;
        let cached = struct_column(&batch, "cached_target_position")?;
        let radius = column::<Int64Array>(&batch, "cached_target_radius")?;
        let released = column::<BooleanArray>(&batch, "released")?;
        let life = struct_column(&batch, "life")?;
        for index in 0..batch.num_rows() {
            rows.push((
                tick.value(index),
                ProjectileState {
                    projectile_id: id.value(index),
                    team_id: team.value(index),
                    owner: read_optional_ref(owner, index)?,
                    position: read_vec3(position, index)?,
                    orientation: orientation.value(index),
                    target: read_optional_ref(target, index)?,
                    cached_target_position: read_vec3(cached, index)?,
                    cached_target_radius: radius.value(index),
                    released: released.value(index),
                    life: read_gauge(life, index)?,
                },
            ));
        }
    }
    Ok(rows)
}

fn read_buildings(member: MemberSlice) -> Result<Vec<(u64, BuildingState)>> {
    let mut rows = Vec::new();
    for batch in checked_builder(member, building_schema().as_ref(), "buildings")?.build()? {
        let batch = batch?;
        let tick = column::<UInt64Array>(&batch, "tick")?;
        let id = column::<UInt64Array>(&batch, "building_id")?;
        let team = column::<UInt32Array>(&batch, "team_id")?;
        let type_id = column::<UInt32Array>(&batch, "building_type_id")?;
        let position = struct_column(&batch, "position")?;
        let rotation = column::<Int64Array>(&batch, "rotation")?;
        let width = column::<Int64Array>(&batch, "bounds_width")?;
        let height = column::<Int64Array>(&batch, "bounds_height")?;
        let life = column::<Int64Array>(&batch, "life")?;
        let max_life = column::<Int64Array>(&batch, "max_life")?;
        let alive = column::<BooleanArray>(&batch, "alive")?;
        let destroyed = column::<BooleanArray>(&batch, "destroyed")?;
        let available = column::<BooleanArray>(&batch, "available")?;
        let targetable = column::<BooleanArray>(&batch, "targetable")?;
        let collision = column::<BooleanArray>(&batch, "collision_enabled")?;
        for index in 0..batch.num_rows() {
            rows.push((
                tick.value(index),
                BuildingState {
                    building_id: id.value(index),
                    team_id: team.value(index),
                    building_type_id: type_id.value(index),
                    position: read_vec3(position, index)?,
                    rotation: rotation.value(index),
                    bounds_width: width.value(index),
                    bounds_height: height.value(index),
                    life: life.value(index),
                    max_life: max_life.value(index),
                    alive: alive.value(index),
                    destroyed: destroyed.value(index),
                    available: available.value(index),
                    targetable: targetable.value(index),
                    collision_enabled: collision.value(index),
                },
            ));
        }
    }
    Ok(rows)
}

fn read_statuses(member: MemberSlice) -> Result<Vec<(u64, StatusState)>> {
    let mut rows = Vec::new();
    for batch in checked_builder(member, status_schema().as_ref(), "statuses")?.build()? {
        let batch = batch?;
        let tick = column::<UInt64Array>(&batch, "tick")?;
        let id = column::<UInt64Array>(&batch, "status_id")?;
        let type_id = column::<UInt32Array>(&batch, "status_type_id")?;
        let source = struct_column(&batch, "source")?;
        let target = struct_column(&batch, "target")?;
        let stack = column::<Int32Array>(&batch, "additive_stack")?;
        let duration = column::<Int32Array>(&batch, "duration_time")?;
        let max_duration = column::<Int32Array>(&batch, "max_duration_time")?;
        let step = column::<Int32Array>(&batch, "step_time")?;
        let step_config = column::<Int32Array>(&batch, "step_time_config")?;
        let finished = column::<BooleanArray>(&batch, "finished")?;
        let frozen = column::<BooleanArray>(&batch, "frozen")?;
        for index in 0..batch.num_rows() {
            rows.push((
                tick.value(index),
                StatusState {
                    status_id: id.value(index),
                    status_type_id: type_id.value(index),
                    source: read_optional_ref(source, index)?,
                    target: read_required_ref(target, index)?,
                    additive_stack: stack.value(index),
                    duration_time: duration.value(index),
                    max_duration_time: max_duration.value(index),
                    step_time: step.value(index),
                    step_time_config: step_config.value(index),
                    finished: finished.value(index),
                    frozen: frozen.value(index),
                },
            ));
        }
    }
    Ok(rows)
}

fn read_events(member: MemberSlice) -> Result<Vec<(u64, u32, Event)>> {
    let mut rows = Vec::new();
    for batch in checked_builder(member, event_schema().as_ref(), "events")?.build()? {
        let batch = batch?;
        let tick = column::<UInt64Array>(&batch, "tick")?;
        let ordinal = column::<UInt32Array>(&batch, "ordinal")?;
        let subject = struct_column(&batch, "subject")?;
        let source = struct_column(&batch, "source")?;
        let target = struct_column(&batch, "target")?;
        let payload = struct_column(&batch, "payload")?;
        for index in 0..batch.num_rows() {
            rows.push((
                tick.value(index),
                ordinal.value(index),
                Event {
                    subject: read_optional_ref(subject, index)?,
                    source: read_optional_ref(source, index)?,
                    target: read_optional_ref(target, index)?,
                    payload: read_event_payload(payload, index)?,
                },
            ));
        }
    }
    Ok(rows)
}

fn read_vec3(array: &StructArray, index: usize) -> Result<Vec3> {
    Ok(Vec3 {
        x: struct_child::<Int64Array>(array, "x")?.value(index),
        y: struct_child::<Int64Array>(array, "y")?.value(index),
        z: struct_child::<Int64Array>(array, "z")?.value(index),
    })
}

fn read_pose(array: &StructArray, index: usize) -> Result<Pose> {
    Ok(Pose {
        position: read_vec3(struct_child::<StructArray>(array, "position")?, index)?,
        rotation: struct_child::<Int64Array>(array, "rotation")?.value(index),
    })
}

fn read_shield(array: &StructArray, index: usize) -> Result<PersonalShieldState> {
    Ok(PersonalShieldState {
        active: struct_child::<BooleanArray>(array, "active")?.value(index),
        enabled: struct_child::<BooleanArray>(array, "enabled")?.value(index),
        energy: struct_child::<Int64Array>(array, "energy")?.value(index),
        max_energy: struct_child::<Int64Array>(array, "max_energy")?.value(index),
    })
}

fn read_gauge(array: &StructArray, index: usize) -> Result<Gauge> {
    Ok(Gauge {
        current: struct_child::<Int64Array>(array, "current")?.value(index),
        maximum: struct_child::<Int64Array>(array, "maximum")?.value(index),
    })
}

fn read_optional_ref(array: &StructArray, index: usize) -> Result<Option<ObjectRef>> {
    if array.is_null(index) {
        Ok(None)
    } else {
        Ok(Some(read_required_ref(array, index)?))
    }
}

fn read_required_ref(array: &StructArray, index: usize) -> Result<ObjectRef> {
    if array.is_null(index) {
        return Err(Error::invalid("required ObjectRef is null"));
    }
    let kind = struct_child::<UInt8Array>(array, "kind")?.value(index);
    let id = struct_child::<UInt64Array>(array, "id")?.value(index);
    Ok(ObjectRef::new(decode_kind(kind)?, id))
}

fn read_event_payload(array: &StructArray, index: usize) -> Result<EventPayload> {
    let kind = decode_event(struct_child::<UInt8Array>(array, "kind")?.value(index))?;
    let position = struct_child::<StructArray>(array, "position")?;
    let intercepted = struct_child::<BooleanArray>(array, "intercepted")?;
    let amount = struct_child::<Int64Array>(array, "amount")?;
    match kind {
        EventKind::ProjectileReleased => {
            require_null(position, index, "payload.position")?;
            require_null(intercepted, index, "payload.intercepted")?;
            require_null(amount, index, "payload.amount")?;
            Ok(EventPayload::ProjectileReleased)
        }
        EventKind::ProjectileRemoved => {
            if position.is_null(index) || intercepted.is_null(index) || !amount.is_null(index) {
                return Err(Error::invalid(
                    "ProjectileRemoved payload branch validity is invalid",
                ));
            }
            Ok(EventPayload::ProjectileRemoved {
                position: read_vec3(position, index)?,
                intercepted: intercepted.value(index),
            })
        }
        EventKind::Damage => {
            require_null(position, index, "payload.position")?;
            require_null(intercepted, index, "payload.intercepted")?;
            if amount.is_null(index) {
                return Err(Error::invalid("Damage payload amount is null"));
            }
            Ok(EventPayload::Damage {
                amount: amount.value(index),
            })
        }
    }
}

fn require_null(array: &dyn Array, index: usize, label: &str) -> Result<()> {
    if array.is_null(index) {
        Ok(())
    } else {
        Err(Error::invalid(format!(
            "{label} must be null for this event kind"
        )))
    }
}

fn column<'a, T: Array + 'static>(batch: &'a RecordBatch, name: &str) -> Result<&'a T> {
    batch
        .column_by_name(name)
        .ok_or_else(|| Error::invalid(format!("missing Parquet column {name}")))?
        .as_any()
        .downcast_ref::<T>()
        .ok_or_else(|| Error::invalid(format!("Parquet column {name} has the wrong type")))
}

fn struct_column<'a>(batch: &'a RecordBatch, name: &str) -> Result<&'a StructArray> {
    column(batch, name)
}

fn struct_child<'a, T: Array + 'static>(array: &'a StructArray, name: &str) -> Result<&'a T> {
    array
        .column_by_name(name)
        .ok_or_else(|| Error::invalid(format!("missing Parquet struct field {name}")))?
        .as_any()
        .downcast_ref::<T>()
        .ok_or_else(|| Error::invalid(format!("Parquet struct field {name} has the wrong type")))
}

fn checked_builder(
    member: MemberSlice,
    expected: &Schema,
    label: &str,
) -> Result<ParquetRecordBatchReaderBuilder<MemberSlice>> {
    let builder = ParquetRecordBatchReaderBuilder::try_new(member)?;
    if builder.schema().fields() != expected.fields() {
        return Err(Error::invalid(format!(
            "{label}.parquet schema does not match format {MCFR_FORMAT}"
        )));
    }
    for row_group in builder.metadata().row_groups() {
        for column in row_group.columns() {
            if !matches!(column.compression(), Compression::ZSTD(_)) {
                return Err(Error::invalid(format!(
                    "{label}.parquet column {} is not ZSTD compressed",
                    column.column_path().string()
                )));
            }
        }
    }
    Ok(builder)
}

fn open_members(path: &Path) -> Result<BTreeMap<String, MemberSlice>> {
    let archive_file = File::open(path)?;
    let mut archive = ZipArchive::new(archive_file)?;
    if archive.len() != MEMBER_NAMES.len() {
        return Err(Error::invalid(format!(
            "MCFR must contain exactly {} members",
            MEMBER_NAMES.len()
        )));
    }
    let expected = MEMBER_NAMES
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    let mut ranges = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let name = entry.name().to_owned();
        if !expected.contains(&name) || !seen.insert(name.clone()) {
            return Err(Error::invalid(format!(
                "unexpected or duplicate MCFR member {name:?}"
            )));
        }
        if entry.is_dir()
            || entry.compression() != CompressionMethod::Stored
            || entry.size() != entry.compressed_size()
        {
            return Err(Error::invalid(format!(
                "MCFR member {name:?} must be a STORE file"
            )));
        }
        let data_start = entry.data_start();
        let size = entry.size();
        io::copy(&mut entry, &mut io::sink())?;
        ranges.insert(name, (data_start, size));
    }
    if seen != expected {
        return Err(Error::invalid("MCFR member set is incomplete"));
    }

    let shared = Arc::new(Mutex::new(File::open(path)?));
    Ok(ranges
        .into_iter()
        .map(|(name, (offset, length))| {
            (
                name,
                MemberSlice {
                    file: Arc::clone(&shared),
                    offset,
                    length,
                },
            )
        })
        .collect())
}

#[derive(Clone)]
struct MemberSlice {
    file: Arc<Mutex<File>>,
    offset: u64,
    length: u64,
}

impl Length for MemberSlice {
    fn len(&self) -> u64 {
        self.length
    }
}

impl ChunkReader for MemberSlice {
    type T = SliceReader;

    fn get_read(&self, start: u64) -> parquet::errors::Result<Self::T> {
        if start > self.length {
            return Err(parquet::errors::ParquetError::EOF(format!(
                "member read offset {start} exceeds {}",
                self.length
            )));
        }
        Ok(SliceReader {
            file: Arc::clone(&self.file),
            offset: self.offset + start,
            remaining: self.length - start,
        })
    }

    fn get_bytes(&self, start: u64, length: usize) -> parquet::errors::Result<Bytes> {
        let requested = u64::try_from(length).map_err(|_| {
            parquet::errors::ParquetError::General("member byte range is too large".into())
        })?;
        if start
            .checked_add(requested)
            .is_none_or(|end| end > self.length)
        {
            return Err(parquet::errors::ParquetError::EOF(format!(
                "member byte range {start}+{length} exceeds {}",
                self.length
            )));
        }
        let mut buffer = vec![0; length];
        read_exact_at(&self.file, self.offset + start, &mut buffer)?;
        Ok(buffer.into())
    }
}

struct SliceReader {
    file: Arc<Mutex<File>>,
    offset: u64,
    remaining: u64,
}

impl Read for SliceReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Ok(0);
        }
        let limit =
            usize::try_from(self.remaining.min(buffer.len() as u64)).unwrap_or(buffer.len());
        let read = {
            let mut file = self
                .file
                .lock()
                .map_err(|_| io::Error::other("MCFR file lock is poisoned"))?;
            file.seek(io::SeekFrom::Start(self.offset))?;
            file.read(&mut buffer[..limit])?
        };
        self.offset += read as u64;
        self.remaining -= read as u64;
        Ok(read)
    }
}

fn read_exact_at(file: &Arc<Mutex<File>>, offset: u64, buffer: &mut [u8]) -> io::Result<()> {
    let mut file = file
        .lock()
        .map_err(|_| io::Error::other("MCFR file lock is poisoned"))?;
    file.seek(io::SeekFrom::Start(offset))?;
    file.read_exact(buffer)
}

const fn encode_kind(value: ObjectKind) -> u8 {
    match value {
        ObjectKind::Unit => 0,
        ObjectKind::Projectile => 1,
        ObjectKind::Building => 2,
        ObjectKind::Status => 3,
    }
}

fn decode_kind(value: u8) -> Result<ObjectKind> {
    match value {
        0 => Ok(ObjectKind::Unit),
        1 => Ok(ObjectKind::Projectile),
        2 => Ok(ObjectKind::Building),
        3 => Ok(ObjectKind::Status),
        _ => Err(Error::invalid(format!("invalid ObjectKind tag {value}"))),
    }
}

const fn encode_domain(value: Domain) -> u8 {
    match value {
        Domain::Ground => 0,
        Domain::Air => 1,
    }
}

fn decode_domain(value: u8) -> Result<Domain> {
    match value {
        0 => Ok(Domain::Ground),
        1 => Ok(Domain::Air),
        _ => Err(Error::invalid(format!("invalid Domain tag {value}"))),
    }
}

const fn encode_motion(value: MotionState) -> u8 {
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
        _ => Err(Error::invalid(format!("invalid MotionState tag {value}"))),
    }
}

const fn encode_visibility(value: Visibility) -> u8 {
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
        _ => Err(Error::invalid(format!("invalid Visibility tag {value}"))),
    }
}

const fn encode_event(value: EventKind) -> u8 {
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
        _ => Err(Error::invalid(format!("invalid EventKind tag {value}"))),
    }
}
