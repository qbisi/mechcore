use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs::File,
    io::{self, Read, Seek, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use arrow_array::{
    Array, ArrayRef, BooleanArray, FixedSizeBinaryArray, Int32Array, Int64Array, ListArray,
    RecordBatch, StructArray, UInt8Array, UInt16Array, UInt32Array, UInt64Array,
};
use arrow_buffer::{NullBuffer, OffsetBuffer, ScalarBuffer};
use arrow_schema::{DataType, Field, Fields, Schema, SchemaRef};
use bytes::Bytes;
use parquet::{
    arrow::{
        ArrowWriter, arrow_reader::ParquetRecordBatchReaderBuilder,
        arrow_writer::ArrowWriterOptions,
    },
    basic::{Compression, Encoding, ZstdLevel},
    file::{
        metadata::KeyValue,
        properties::{EnabledStatistics, WriterProperties},
        reader::{ChunkReader, Length},
    },
    schema::types::ColumnPath,
};
use serde::{Deserialize, Serialize};
use tempfile::TempDir;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::{
    AttackPhase, BuffDataKind, BuffDataRef, BuffState, BuildingState, ControlState,
    DamageStatistics, Domain, DurableContext, EnabledSkill, Error, Event, EventPayload,
    FormationState, GaugeI32, HASH_PROFILE, Hashes, LiveUnitState, MCFR_FORMAT, MotionState,
    ObjectKind, ObjectRef, PersonalShieldState, Producer, ProjectileState, QPlanar, QPose, QVec3,
    RecorderKind, Result, ShieldRoundPolicy, ShieldSourceKind, ShieldState, SkillMachineState,
    SkillState, TerrainApplicationState, TerrainEffectClock, TerrainGridState,
    TerrainLogicLifetime, TerrainState, TerrainType, TransitionEvents, Visibility, WeaponState,
    WorldSnapshot, canonical,
    event_table::{batch_events, event_batch, event_schema},
    instrument::{self, ChannelSchema, InstrumentRow},
};

/// The members every recording holds.
const REQUIRED_MEMBERS: [&str; 2] = ["layout.yaml", "ticks.parquet"];

/// The per-tick tables, in container order. A table no row was written to is
/// left out, and reads as having none.
const TABLE_MEMBERS: [&str; 8] = [
    "units.parquet",
    "projectiles.parquet",
    "buildings.parquet",
    "shields.parquet",
    "terrains.parquet",
    "statistics.parquet",
    "formations.parquet",
    "events.parquet",
];

/// Where a recording's instrument channels sit inside the container.
const INSTRUMENT_DIRECTORY: &str = "instrument";

const TICKS_PER_ROW_GROUP: u64 = 1024;
const ROWS_PER_TICK_GROUP: usize = 1024;
const MAX_LAYOUT_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredDurableContext {
    combat_round: u32,
    logic_step: crate::Rational,
    time_units_per_second: u32,
}

impl StoredDurableContext {
    fn with_match_seed(self, match_seed: i32) -> DurableContext {
        DurableContext {
            logic_step: self.logic_step,
            time_units_per_second: self.time_units_per_second,
            combat_round: self.combat_round,
            match_seed,
        }
    }
}

pub(crate) fn encode_durable_context(context: &DurableContext) -> Result<Vec<u8>> {
    canonical::encode(&StoredDurableContext {
        logic_step: context.logic_step,
        time_units_per_second: context.time_units_per_second,
        combat_round: context.combat_round,
    })
}

pub(crate) struct StorageWriter {
    directory: TempDir,
    units: Option<ArrowWriter<File>>,
    projectiles: Option<ArrowWriter<File>>,
    buildings: Option<ArrowWriter<File>>,
    shields: Option<ArrowWriter<File>>,
    statistics: Option<ArrowWriter<File>>,
    formations: Option<ArrowWriter<File>>,
    terrains: Option<ArrowWriter<File>>,
    unit_rows: Vec<(u32, LiveUnitState)>,
    projectile_rows: Vec<(u32, ProjectileState)>,
    building_rows: Vec<(u32, BuildingState)>,
    shield_rows: Vec<(u32, ShieldState)>,
    statistic_rows: Vec<(u32, DamageStatistics)>,
    formation_rows: Vec<(u32, FormationState)>,
    terrain_rows: Vec<(u32, TerrainState)>,
    event_rows: Vec<(u32, u32, Event)>,
    events: Option<ArrowWriter<File>>,
    channels: BTreeMap<&'static str, ChannelWriter>,
    tick_hashes: Vec<[u8; canonical::HASH_BYTES]>,
}

/// One instrument channel's member, open while the fight is recorded.
struct ChannelWriter {
    schema: ChannelSchema,
    writer: Option<ArrowWriter<File>>,
}

impl StorageWriter {
    pub(crate) fn create(parent: &Path) -> Result<Self> {
        let directory = tempfile::Builder::new()
            .prefix(".mcfr-members-")
            .tempdir_in(parent)?;
        Ok(Self {
            directory,
            units: None,
            projectiles: None,
            buildings: None,
            shields: None,
            statistics: None,
            formations: None,
            terrains: None,
            unit_rows: Vec::new(),
            projectile_rows: Vec::new(),
            building_rows: Vec::new(),
            shield_rows: Vec::new(),
            statistic_rows: Vec::new(),
            formation_rows: Vec::new(),
            terrain_rows: Vec::new(),
            event_rows: Vec::new(),
            events: None,
            channels: BTreeMap::new(),
            tick_hashes: Vec::new(),
        })
    }

    /// Appends one tick's rows of a channel, opening its member on first use.
    ///
    /// A channel opened with no rows is still published, empty: a study that
    /// asked for it and saw nothing is told so, not left to guess.
    pub(crate) fn append_instrument<R: InstrumentRow>(
        &mut self,
        tick: u32,
        rows: &[R],
    ) -> Result<()> {
        if !self.channels.contains_key(R::CHANNEL) {
            let schema = ChannelSchema::of::<R>()?;
            let directory = self.directory.path().join(INSTRUMENT_DIRECTORY);
            std::fs::create_dir_all(&directory)?;
            let writer = create_member(
                directory.join(format!("{}.parquet", R::CHANNEL)),
                Arc::clone(&schema.schema),
                Track::Instrument,
            )?;
            self.channels.insert(
                R::CHANNEL,
                ChannelWriter {
                    schema,
                    writer: Some(writer),
                },
            );
        }
        if rows.is_empty() {
            return Ok(());
        }
        let channel = self
            .channels
            .get_mut(R::CHANNEL)
            .ok_or_else(|| Error::invalid("instrument channel was not opened"))?;
        let batch = channel.schema.batch(tick, rows)?;
        channel
            .writer
            .as_mut()
            .ok_or_else(|| Error::invalid("instrument channel writer is closed"))?
            .write(&batch)?;
        Ok(())
    }

    fn append_state(&mut self, tick: u32, state: &WorldSnapshot) {
        self.unit_rows
            .extend(state.live_units.iter().cloned().map(|row| (tick, row)));
        self.projectile_rows
            .extend(state.projectiles.iter().cloned().map(|row| (tick, row)));
        self.building_rows
            .extend(state.buildings.iter().cloned().map(|row| (tick, row)));
        self.shield_rows
            .extend(state.shields.iter().cloned().map(|row| (tick, row)));
        self.terrain_rows
            .extend(state.terrains.iter().cloned().map(|row| (tick, row)));
        self.statistic_rows
            .extend(state.statistics.iter().copied().map(|row| (tick, row)));
        self.formation_rows
            .extend(state.formations.iter().copied().map(|row| (tick, row)));
    }

    pub(crate) fn append_tick(
        &mut self,
        tick: u32,
        state: &WorldSnapshot,
        events: &TransitionEvents,
        tick_hash: [u8; canonical::HASH_BYTES],
    ) -> Result<()> {
        self.append_state(tick, state);
        for event in &events.events {
            validate_event_refs(event)?;
        }
        for (ordinal, event) in events.events.iter().cloned().enumerate() {
            self.event_rows.push((
                tick,
                u32::try_from(ordinal).map_err(|_| Error::invalid("event ordinal overflow"))?,
                event,
            ));
        }
        self.tick_hashes.push(tick_hash);
        if u64::from(tick).is_multiple_of(TICKS_PER_ROW_GROUP) {
            self.flush()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        let directory = self.directory.path();
        write_buffer(
            directory,
            &mut self.units,
            Track::Units,
            unit_batch(&self.unit_rows)?,
        )?;
        write_buffer(
            directory,
            &mut self.projectiles,
            Track::Projectiles,
            projectile_batch(&self.projectile_rows)?,
        )?;
        write_buffer(
            directory,
            &mut self.buildings,
            Track::Buildings,
            building_batch(&self.building_rows)?,
        )?;
        write_buffer(
            directory,
            &mut self.shields,
            Track::Shields,
            shield_batch(&self.shield_rows)?,
        )?;
        write_buffer(
            directory,
            &mut self.terrains,
            Track::Terrains,
            terrain_batch(&self.terrain_rows)?,
        )?;
        write_buffer(
            directory,
            &mut self.statistics,
            Track::Statistics,
            statistic_batch(&self.statistic_rows)?,
        )?;
        write_buffer(
            directory,
            &mut self.formations,
            Track::Formations,
            formation_batch(&self.formation_rows)?,
        )?;
        write_buffer(
            directory,
            &mut self.events,
            Track::Events,
            event_batch(&self.event_rows)?,
        )?;
        for channel in self.channels.values_mut() {
            channel
                .writer
                .as_mut()
                .ok_or_else(|| Error::invalid("instrument channel writer is closed"))?
                .flush()?;
        }
        self.unit_rows.clear();
        self.projectile_rows.clear();
        self.building_rows.clear();
        self.shield_rows.clear();
        self.statistic_rows.clear();
        self.formation_rows.clear();
        self.terrain_rows.clear();
        self.event_rows.clear();
        Ok(())
    }

    pub(crate) fn finish(
        mut self,
        producer: Producer,
        game_build: &str,
        context_bytes: &[u8],
        layout_yaml: &str,
        hashes: &Hashes,
    ) -> Result<TempDir> {
        self.flush()?;
        // A table nothing was written to is left out of the container.
        for table in [
            &mut self.units,
            &mut self.projectiles,
            &mut self.buildings,
            &mut self.shields,
            &mut self.terrains,
            &mut self.statistics,
            &mut self.formations,
            &mut self.events,
        ] {
            if let Some(writer) = table.take() {
                writer.close()?;
            }
        }
        for channel in self.channels.values_mut() {
            close_writer(&mut channel.writer)?;
        }
        let mut layout = File::create(self.directory.path().join("layout.yaml"))?;
        layout.write_all(layout_yaml.as_bytes())?;
        layout.sync_all()?;

        let tick_count = u32::try_from(self.tick_hashes.len())
            .map_err(|_| Error::invalid("tick count overflow"))?;
        if tick_count == 0 {
            return Err(Error::invalid("an MCFR must contain at least T(1)"));
        }
        let terminal_tick = tick_count;
        let context_json = std::str::from_utf8(context_bytes)
            .map_err(|_| Error::invalid("canonical durable context is not UTF-8"))?;
        let metadata = HashMap::from([
            ("format".to_owned(), MCFR_FORMAT.to_owned()),
            ("producer".to_owned(), producer.as_str().to_owned()),
            ("game_build".to_owned(), game_build.to_owned()),
            ("durable_context".to_owned(), context_json.to_owned()),
            ("hash_profile".to_owned(), HASH_PROFILE.to_owned()),
            ("result_hash".to_owned(), hashes.result_hash.clone()),
            ("tick_count".to_owned(), tick_count.to_string()),
            ("terminal_tick".to_owned(), terminal_tick.to_string()),
        ]);
        let mut ticks = create_member_with_metadata(
            self.directory.path().join("ticks.parquet"),
            Arc::new(Schema::new(tick_fields())),
            Track::Ticks,
            metadata,
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

/// Writes one row group of a table, opening its member on the first rows.
fn write_buffer(
    directory: &Path,
    writer: &mut Option<ArrowWriter<File>>,
    track: Track,
    batch: Option<RecordBatch>,
) -> Result<()> {
    let Some(batch) = batch else {
        return Ok(());
    };
    if writer.is_none() {
        let (name, schema) = track
            .table()
            .ok_or_else(|| Error::invalid("a row group was written to a non-table member"))?;
        *writer = Some(create_member(directory.join(name), schema, track)?);
    }
    let writer = writer
        .as_mut()
        .ok_or_else(|| Error::invalid("Parquet member writer is closed"))?;
    writer.write(&batch)?;
    writer.flush()?;
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
    Shields,
    Terrains,
    Statistics,
    Formations,
    Events,
    Instrument,
}

impl Track {
    /// The member a table is stored in and its schema; `None` for the members
    /// that are not a per-tick table.
    fn table(self) -> Option<(&'static str, SchemaRef)> {
        match self {
            Self::Units => Some(("units.parquet", unit_schema())),
            Self::Projectiles => Some(("projectiles.parquet", projectile_schema())),
            Self::Buildings => Some(("buildings.parquet", building_schema())),
            Self::Shields => Some(("shields.parquet", shield_schema())),
            Self::Terrains => Some(("terrains.parquet", terrain_schema())),
            Self::Statistics => Some(("statistics.parquet", statistic_schema())),
            Self::Formations => Some(("formations.parquet", formation_schema())),
            Self::Events => Some(("events.parquet", event_schema())),
            Self::Ticks | Self::Instrument => None,
        }
    }
}

fn create_member(path: PathBuf, schema: SchemaRef, track: Track) -> Result<ArrowWriter<File>> {
    create_member_with_metadata(path, schema, track, HashMap::new())
}

/// Opens a member. The Arrow schema is not embedded: every reader knows each
/// table's schema, and in a short recording the embedded copy outweighed the
/// data. File metadata, which `ticks.parquet` carries, is Parquet key/value
/// metadata instead.
fn create_member_with_metadata(
    path: PathBuf,
    schema: SchemaRef,
    track: Track,
    metadata: HashMap<String, String>,
) -> Result<ArrowWriter<File>> {
    let mut metadata = metadata
        .into_iter()
        .map(|(key, value)| KeyValue::new(key, value))
        .collect::<Vec<_>>();
    metadata.sort_by(|left, right| left.key.cmp(&right.key));
    let properties = writer_properties(track, metadata)?;
    Ok(ArrowWriter::try_new_with_options(
        File::create(path)?,
        schema,
        ArrowWriterOptions::new()
            .with_properties(properties)
            .with_skip_arrow_metadata(true),
    )?)
}

fn writer_properties(track: Track, metadata: Vec<KeyValue>) -> Result<WriterProperties> {
    let mut builder = WriterProperties::builder()
        .set_compression(Compression::ZSTD(ZstdLevel::try_new(6)?))
        .set_dictionary_enabled(false)
        .set_max_row_group_row_count(Some(1_000_000))
        // Statistics per column chunk, not per page: no reader seeks by page,
        // and the page index repeated in every row group of a short recording.
        .set_statistics_enabled(EnabledStatistics::Chunk)
        .set_offset_index_disabled(true)
        .set_key_value_metadata((!metadata.is_empty()).then_some(metadata));
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
        Track::Ticks | Track::Instrument => &[],
        Track::Events => &["type", "object.kind", "source.kind", "target.kind"],
        Track::Units => &[
            "team_id",
            "original_team_id",
            "unit_type_id",
            "domain",
            "motion_state",
            "mech_lock_target.kind",
            "collision_radius",
            "visibility",
            "life.maximum",
            "personal_shield.energy.maximum",
        ],
        Track::Projectiles => &[
            "team_id",
            "owner.kind",
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
            "life.maximum",
        ],
        Track::Shields => &["team_id", "source_kind", "energy.maximum", "round_policy"],
        Track::Terrains => &["team_id", "terrain_type", "remaining_rounds"],
        Track::Statistics => &["team_id", "recorder"],
        Track::Formations => &["team_id", "max_experience"],
    }
}

fn delta_paths(track: Track) -> &'static [&'static str] {
    match track {
        Track::Ticks
        | Track::Units
        | Track::Projectiles
        | Track::Buildings
        | Track::Shields
        | Track::Terrains
        | Track::Statistics
        | Track::Formations
        | Track::Events
        | Track::Instrument => &["tick"],
    }
}

fn tick_batch(start: usize, tick_hashes: &[[u8; canonical::HASH_BYTES]]) -> Result<RecordBatch> {
    let start_tick = u32::try_from(start)
        .map_err(|_| Error::invalid("tick row offset exceeds u32"))?
        .checked_add(1)
        .ok_or_else(|| Error::invalid("tick row offset exceeds u32"))?;
    let tick_count = u32::try_from(tick_hashes.len())
        .map_err(|_| Error::invalid("tick batch length exceeds u32"))?;
    let end_tick = start_tick
        .checked_add(tick_count)
        .ok_or_else(|| Error::invalid("tick batch range exceeds u32"))?;
    let ticks = UInt32Array::from_iter_values(start_tick..end_tick);
    let tick_hashes = FixedSizeBinaryArray::try_from_iter(
        tick_hashes
            .iter()
            .map(<[u8; canonical::HASH_BYTES]>::as_slice),
    )?;
    Ok(RecordBatch::try_new(
        Arc::new(Schema::new(tick_fields())),
        vec![Arc::new(ticks), Arc::new(tick_hashes)],
    )?)
}

fn unit_batch(rows: &[(u32, LiveUnitState)]) -> Result<Option<RecordBatch>> {
    if rows.is_empty() {
        return Ok(None);
    }
    let units = rows.iter().map(|(_, row)| row).collect::<Vec<_>>();
    Ok(Some(RecordBatch::try_new(
        unit_schema(),
        vec![
            u32_values(rows.iter().map(|(tick, _)| *tick)),
            u64_values(units.iter().map(|row| row.unit_id)),
            u32_values(units.iter().map(|row| row.team_id)),
            u32_values(units.iter().map(|row| row.original_team_id)),
            u64_values(units.iter().map(|row| row.formation_id)),
            u32_values(units.iter().map(|row| row.unit_type_id)),
            u8_values(units.iter().map(|row| encode_domain(row.domain))),
            vec3_values(units.iter().map(|row| row.position)),
            i64_values(units.iter().map(|row| row.body_rotation)),
            optional_i64_values(units.iter().map(|row| row.turret_rotation)),
            planar_values(units.iter().map(|row| row.velocity)),
            u8_values(units.iter().map(|row| encode_motion(row.motion_state))),
            object_ref_values(units.iter().map(|row| row.mech_lock_target)),
            i64_values(units.iter().map(|row| row.collision_radius)),
            gauge_values(units.iter().map(|row| row.life)),
            bool_values(units.iter().map(|row| row.active)),
            bool_values(units.iter().map(|row| row.targetable)),
            u8_values(units.iter().map(|row| encode_visibility(row.visibility))),
            buff_list_values(units.iter().map(|row| &row.buffs))?,
            shield_values(units.iter().map(|row| row.personal_shield)),
            i64_values(units.iter().map(|row| row.move_speed)),
            skill_list_values(units.iter().map(|row| &row.skills))?,
            control_values(units.iter().map(|row| row.control.as_ref()))?,
        ],
    )?))
}

fn projectile_batch(rows: &[(u32, ProjectileState)]) -> Result<Option<RecordBatch>> {
    if rows.is_empty() {
        return Ok(None);
    }
    let values = rows.iter().map(|(_, row)| row).collect::<Vec<_>>();
    Ok(Some(RecordBatch::try_new(
        projectile_schema(),
        vec![
            u32_values(rows.iter().map(|(tick, _)| *tick)),
            u64_values(values.iter().map(|row| row.projectile_id)),
            u32_values(values.iter().map(|row| row.team_id)),
            object_ref_values(values.iter().map(|row| row.owner)),
            vec3_values(values.iter().map(|row| row.position)),
            object_ref_values(values.iter().map(|row| row.target)),
            vec3_values(values.iter().map(|row| row.cached_target_position)),
            i64_values(values.iter().map(|row| row.cached_target_radius)),
            i64_values(values.iter().map(|row| row.move_range)),
            gauge_values(values.iter().map(|row| row.life)),
            object_ref_list_values(values.iter().map(|row| &row.spawn_containing_shields))?,
        ],
    )?))
}

fn statistic_batch(rows: &[(u32, DamageStatistics)]) -> Result<Option<RecordBatch>> {
    if rows.is_empty() {
        return Ok(None);
    }
    let values = rows.iter().map(|(_, row)| row).collect::<Vec<_>>();
    Ok(Some(RecordBatch::try_new(
        statistic_schema(),
        vec![
            u32_values(rows.iter().map(|(tick, _)| *tick)),
            u32_values(values.iter().map(|row| row.team_id)),
            u8_values(values.iter().map(|row| encode_recorder(row.recorder))),
            u64_values(values.iter().map(|row| row.recorder_id)),
            i32_values(values.iter().map(|row| row.damage)),
            i32_values(values.iter().map(|row| row.damage_real)),
            i32_values(values.iter().map(|row| row.kills)),
            i32_values(values.iter().map(|row| row.damage_taken)),
        ],
    )?))
}

const RECORDER_KINDS: [RecorderKind; 3] = [
    RecorderKind::Formation,
    RecorderKind::Construction,
    RecorderKind::Unit,
];

fn encode_recorder(kind: RecorderKind) -> u8 {
    RECORDER_KINDS
        .iter()
        .position(|candidate| *candidate == kind)
        .and_then(|index| u8::try_from(index).ok())
        .expect("every recorder kind has a tag")
}

/// A statistics row's identity within its tick, for the ordering check.
fn statistic_identity(row: &DamageStatistics) -> u64 {
    (u64::from(row.team_id) << 56)
        | (u64::from(encode_recorder(row.recorder)) << 48)
        | row.recorder_id
}

fn formation_batch(rows: &[(u32, FormationState)]) -> Result<Option<RecordBatch>> {
    if rows.is_empty() {
        return Ok(None);
    }
    let values = rows.iter().map(|(_, row)| row).collect::<Vec<_>>();
    Ok(Some(RecordBatch::try_new(
        formation_schema(),
        vec![
            u32_values(rows.iter().map(|(tick, _)| *tick)),
            u64_values(values.iter().map(|row| row.formation_id)),
            u32_values(values.iter().map(|row| row.team_id)),
            i64_values(values.iter().map(|row| row.experience)),
            i64_values(values.iter().map(|row| row.max_experience)),
        ],
    )?))
}

fn shield_batch(rows: &[(u32, ShieldState)]) -> Result<Option<RecordBatch>> {
    if rows.is_empty() {
        return Ok(None);
    }
    let values = rows.iter().map(|(_, row)| row).collect::<Vec<_>>();
    Ok(Some(RecordBatch::try_new(
        shield_schema(),
        vec![
            u32_values(rows.iter().map(|(tick, _)| *tick)),
            u64_values(values.iter().map(|row| row.shield_id)),
            u32_values(values.iter().map(|row| row.team_id)),
            u8_values(
                values
                    .iter()
                    .map(|row| encode_shield_source(row.source_kind)),
            ),
            object_ref_values(values.iter().map(|row| row.owner)),
            vec3_values(values.iter().map(|row| row.position)),
            i64_values(values.iter().map(|row| row.radius)),
            gauge_values(values.iter().map(|row| row.energy)),
            u8_values(
                values
                    .iter()
                    .map(|row| encode_shield_round_policy(row.round_policy)),
            ),
            bool_values(values.iter().map(|row| row.active)),
            optional_u32_values(values.iter().map(|row| row.active_order)),
        ],
    )?))
}

fn terrain_batch(rows: &[(u32, TerrainState)]) -> Result<Option<RecordBatch>> {
    if rows.is_empty() {
        return Ok(None);
    }
    let values = rows.iter().map(|(_, row)| row).collect::<Vec<_>>();
    Ok(Some(RecordBatch::try_new(
        terrain_schema(),
        vec![
            u32_values(rows.iter().map(|(tick, _)| *tick)),
            u64_values(values.iter().map(|row| row.terrain_id)),
            optional_u32_values(values.iter().map(|row| row.team_id)),
            u8_values(
                values
                    .iter()
                    .map(|row| encode_terrain_type(row.terrain_type)),
            ),
            vec3_values(values.iter().map(|row| row.position)),
            i64_values(values.iter().map(|row| row.radius)),
            terrain_grid_values(values.iter().map(|row| row.grid.as_ref()))?,
            optional_u32_values(values.iter().map(|row| row.remaining_rounds)),
            terrain_lifetime_values(values.iter().map(|row| row.logic_lifetime)),
            terrain_application_list_values(values.iter().map(|row| &row.applications))?,
        ],
    )?))
}

fn building_batch(rows: &[(u32, BuildingState)]) -> Result<Option<RecordBatch>> {
    if rows.is_empty() {
        return Ok(None);
    }
    let values = rows.iter().map(|(_, row)| row).collect::<Vec<_>>();
    Ok(Some(RecordBatch::try_new(
        building_schema(),
        vec![
            u32_values(rows.iter().map(|(tick, _)| *tick)),
            u64_values(values.iter().map(|row| row.building_id)),
            u32_values(values.iter().map(|row| row.team_id)),
            u32_values(values.iter().map(|row| row.building_type_id)),
            vec3_values(values.iter().map(|row| row.position)),
            i64_values(values.iter().map(|row| row.bounds_width)),
            i64_values(values.iter().map(|row| row.bounds_height)),
            gauge_values(values.iter().map(|row| row.life)),
            bool_values(values.iter().map(|row| row.available)),
            bool_values(values.iter().map(|row| row.targetable)),
            bool_values(values.iter().map(|row| row.collision_enabled)),
        ],
    )?))
}

fn u8_values(values: impl IntoIterator<Item = u8>) -> ArrayRef {
    Arc::new(UInt8Array::from_iter_values(values))
}

fn u16_values(values: impl IntoIterator<Item = u16>) -> ArrayRef {
    Arc::new(UInt16Array::from_iter_values(values))
}

fn u32_values(values: impl IntoIterator<Item = u32>) -> ArrayRef {
    Arc::new(UInt32Array::from_iter_values(values))
}

fn optional_u32_values(values: impl IntoIterator<Item = Option<u32>>) -> ArrayRef {
    Arc::new(UInt32Array::from_iter(values))
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

fn optional_i64_values(values: impl IntoIterator<Item = Option<i64>>) -> ArrayRef {
    Arc::new(Int64Array::from_iter(values))
}

fn bool_values(values: impl IntoIterator<Item = bool>) -> ArrayRef {
    Arc::new(BooleanArray::from(values.into_iter().collect::<Vec<_>>()))
}

fn vec3_values(values: impl IntoIterator<Item = QVec3>) -> ArrayRef {
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

fn planar_values(values: impl IntoIterator<Item = QPlanar>) -> ArrayRef {
    let values = values.into_iter().collect::<Vec<_>>();
    Arc::new(StructArray::new(
        planar_fields(),
        vec![
            i64_values(values.iter().map(|value| value.x)),
            i64_values(values.iter().map(|value| value.z)),
        ],
        None,
    ))
}

fn optional_vec3_values(values: impl IntoIterator<Item = Option<QVec3>>) -> ArrayRef {
    let values = values.into_iter().collect::<Vec<_>>();
    Arc::new(StructArray::new(
        vec3_fields(),
        vec![
            i64_values(values.iter().map(|value| value.map_or(0, |value| value.x))),
            i64_values(values.iter().map(|value| value.map_or(0, |value| value.y))),
            i64_values(values.iter().map(|value| value.map_or(0, |value| value.z))),
        ],
        Some(values.iter().map(Option::is_some).collect::<NullBuffer>()),
    ))
}

fn shield_values(values: impl IntoIterator<Item = PersonalShieldState>) -> ArrayRef {
    let values = values.into_iter().collect::<Vec<_>>();
    Arc::new(StructArray::new(
        shield_fields(),
        vec![
            bool_values(values.iter().map(|value| value.active)),
            bool_values(values.iter().map(|value| value.enabled)),
            gauge_values(values.iter().map(|value| value.energy)),
        ],
        None,
    ))
}

fn gauge_values(values: impl IntoIterator<Item = GaugeI32>) -> ArrayRef {
    let values = values.into_iter().collect::<Vec<_>>();
    Arc::new(StructArray::new(
        gauge_fields(),
        vec![
            i32_values(values.iter().map(|value| value.current)),
            i32_values(values.iter().map(|value| value.maximum)),
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

fn object_ref_list_values<'a>(
    values: impl IntoIterator<Item = &'a Vec<ObjectRef>>,
) -> Result<ArrayRef> {
    let values = values.into_iter().collect::<Vec<_>>();
    let flat = values
        .iter()
        .flat_map(|value| value.iter().copied())
        .collect::<Vec<_>>();
    let items = StructArray::new(
        object_ref_fields(),
        vec![
            u8_values(flat.iter().map(|value| encode_kind(value.kind))),
            u64_values(flat.iter().map(|value| value.id)),
        ],
        None,
    );
    Ok(Arc::new(ListArray::new(
        Arc::new(Field::new(
            "item",
            DataType::Struct(object_ref_fields()),
            false,
        )),
        list_offsets(values.iter().map(|value| value.len()))?,
        Arc::new(items),
        None,
    )))
}

fn u32_list_values<'a>(values: impl IntoIterator<Item = &'a Vec<u32>>) -> Result<ArrayRef> {
    let values = values.into_iter().collect::<Vec<_>>();
    let flat = values
        .iter()
        .flat_map(|value| value.iter().copied())
        .collect::<Vec<_>>();
    Ok(Arc::new(ListArray::new(
        Arc::new(Field::new("item", DataType::UInt32, false)),
        list_offsets(values.iter().map(|value| value.len()))?,
        Arc::new(UInt32Array::from_iter_values(flat)),
        None,
    )))
}

fn terrain_grid_values<'a>(
    values: impl IntoIterator<Item = Option<&'a TerrainGridState>>,
) -> Result<ArrayRef> {
    let values = values.into_iter().collect::<Vec<_>>();
    let empty = Vec::new();
    let rows = values
        .iter()
        .map(|value| value.map_or(&empty, |value| &value.rows))
        .collect::<Vec<_>>();
    Ok(Arc::new(StructArray::new(
        terrain_grid_fields(),
        vec![
            i64_values(
                values
                    .iter()
                    .map(|value| value.map_or(0, |value| value.origin_x)),
            ),
            i64_values(
                values
                    .iter()
                    .map(|value| value.map_or(0, |value| value.origin_y)),
            ),
            u32_values(
                values
                    .iter()
                    .map(|value| value.map_or(0, |value| value.size_x)),
            ),
            u32_values(
                values
                    .iter()
                    .map(|value| value.map_or(0, |value| value.size_y)),
            ),
            u32_list_values(rows)?,
        ],
        Some(values.iter().map(Option::is_some).collect::<NullBuffer>()),
    )))
}

fn terrain_lifetime_values(
    values: impl IntoIterator<Item = Option<TerrainLogicLifetime>>,
) -> ArrayRef {
    let values = values.into_iter().collect::<Vec<_>>();
    Arc::new(StructArray::new(
        terrain_lifetime_fields(),
        vec![
            i32_values(
                values
                    .iter()
                    .map(|value| value.map_or(0, |value| value.elapsed)),
            ),
            i32_values(
                values
                    .iter()
                    .map(|value| value.map_or(0, |value| value.limit)),
            ),
        ],
        Some(values.iter().map(Option::is_some).collect::<NullBuffer>()),
    ))
}

fn terrain_application_list_values<'a>(
    values: impl IntoIterator<Item = &'a Vec<TerrainApplicationState>>,
) -> Result<ArrayRef> {
    let values = values.into_iter().collect::<Vec<_>>();
    let flat = values
        .iter()
        .flat_map(|value| value.iter().copied())
        .collect::<Vec<_>>();
    let items = StructArray::new(
        terrain_application_fields(),
        vec![
            u64_values(flat.iter().map(|value| value.unit_id)),
            terrain_effect_clock_values(flat.iter().map(|value| value.periodic_clock)),
        ],
        None,
    );
    Ok(Arc::new(ListArray::new(
        Arc::new(Field::new(
            "item",
            DataType::Struct(terrain_application_fields()),
            false,
        )),
        list_offsets(values.iter().map(|value| value.len()))?,
        Arc::new(items),
        None,
    )))
}

fn terrain_effect_clock_values(
    values: impl IntoIterator<Item = Option<TerrainEffectClock>>,
) -> ArrayRef {
    let values = values.into_iter().collect::<Vec<_>>();
    Arc::new(StructArray::new(
        terrain_effect_clock_fields(),
        vec![
            i32_values(
                values
                    .iter()
                    .map(|value| value.map_or(0, |value| value.elapsed)),
            ),
            i32_values(
                values
                    .iter()
                    .map(|value| value.map_or(0, |value| value.duration)),
            ),
        ],
        Some(values.iter().map(Option::is_some).collect::<NullBuffer>()),
    ))
}

fn list_offsets(lengths: impl IntoIterator<Item = usize>) -> Result<OffsetBuffer<i32>> {
    let mut offsets = vec![0_i32];
    for length in lengths {
        let next = offsets
            .last()
            .copied()
            .unwrap_or(0)
            .checked_add(i32::try_from(length).map_err(|_| Error::invalid("list is too long"))?)
            .ok_or_else(|| Error::invalid("list offset overflow"))?;
        offsets.push(next);
    }
    Ok(OffsetBuffer::new(ScalarBuffer::from(offsets)))
}

fn buff_list_values<'a>(values: impl IntoIterator<Item = &'a Vec<BuffState>>) -> Result<ArrayRef> {
    let values = values.into_iter().collect::<Vec<_>>();
    let flat = values
        .iter()
        .flat_map(|value| value.iter())
        .collect::<Vec<_>>();
    let data = StructArray::new(
        buff_data_fields(),
        vec![
            u8_values(
                flat.iter()
                    .map(|value| encode_buff_data_kind(value.data.kind)),
            ),
            Arc::new(UInt32Array::from(
                flat.iter().map(|value| value.data.id).collect::<Vec<_>>(),
            )),
        ],
        None,
    );
    let items = StructArray::new(
        buff_fields(),
        vec![
            Arc::new(data),
            object_ref_values(flat.iter().map(|value| value.source)),
            Arc::new(UInt32Array::from(
                flat.iter()
                    .map(|value| value.source_team)
                    .collect::<Vec<_>>(),
            )),
            i32_values(flat.iter().map(|value| value.elapsed)),
            i32_values(flat.iter().map(|value| value.duration)),
            i32_values(flat.iter().map(|value| value.step)),
            i32_values(flat.iter().map(|value| value.stacks)),
        ],
        None,
    );
    Ok(Arc::new(ListArray::new(
        Arc::new(Field::new("item", DataType::Struct(buff_fields()), false)),
        list_offsets(values.iter().map(|value| value.len()))?,
        Arc::new(items),
        None,
    )))
}

const BUFF_DATA_KINDS: [BuffDataKind; 2] = [BuffDataKind::Buff, BuffDataKind::Technology];

fn encode_buff_data_kind(kind: BuffDataKind) -> u8 {
    BUFF_DATA_KINDS
        .iter()
        .position(|candidate| *candidate == kind)
        .and_then(|index| u8::try_from(index).ok())
        .expect("every buff data kind has a tag")
}

fn skill_list_values<'a>(
    values: impl IntoIterator<Item = &'a Vec<SkillState>>,
) -> Result<ArrayRef> {
    let values = values.into_iter().collect::<Vec<_>>();
    let flat = values
        .iter()
        .flat_map(|value| value.iter())
        .collect::<Vec<_>>();
    let enabled = flat
        .iter()
        .map(|skill| skill.enabled.as_ref())
        .collect::<Vec<_>>();
    let enabled_items = enabled_skill_values(&enabled)?;
    let items = StructArray::new(
        skill_fields(),
        vec![
            u16_values(flat.iter().map(|skill| skill.skill_slot)),
            Arc::new(enabled_items),
        ],
        None,
    );
    Ok(Arc::new(ListArray::new(
        Arc::new(Field::new("item", DataType::Struct(skill_fields()), false)),
        list_offsets(values.iter().map(|value| value.len()))?,
        Arc::new(items),
        None,
    )))
}

/// The `enabled` struct of each skill, null for a switched-off one.
fn enabled_skill_values(enabled: &[Option<&EnabledSkill>]) -> Result<StructArray> {
    let weapons = enabled
        .iter()
        .flat_map(|skill| skill.map_or(&[][..], |skill| skill.weapons.as_slice()))
        .collect::<Vec<_>>();
    let weapon_items = StructArray::new(
        weapon_fields(),
        vec![
            i32_values(weapons.iter().map(|weapon| weapon.weapon_index)),
            optional_vec3_values(
                weapons
                    .iter()
                    .map(|weapon| weapon.pose.map(|pose| pose.position)),
            ),
            optional_i64_values(
                weapons
                    .iter()
                    .map(|weapon| weapon.pose.map(|pose| pose.rotation)),
            ),
        ],
        None,
    );
    let weapon_lists = ListArray::new(
        Arc::new(Field::new("item", DataType::Struct(weapon_fields()), false)),
        list_offsets(
            enabled
                .iter()
                .map(|skill| skill.map_or(0, |skill| skill.weapons.len())),
        )?,
        Arc::new(weapon_items),
        None,
    );
    let field = |read: fn(&EnabledSkill) -> i64| {
        enabled
            .iter()
            .map(move |skill| skill.map_or(0, read))
            .collect::<Vec<_>>()
    };
    let enabled_items = StructArray::new(
        enabled_skill_fields(),
        vec![
            object_ref_values(
                enabled
                    .iter()
                    .map(|skill| skill.and_then(|skill| skill.lock_target)),
            ),
            object_ref_values(
                enabled
                    .iter()
                    .map(|skill| skill.and_then(|skill| skill.attack_target)),
            ),
            u8_values(
                enabled
                    .iter()
                    .map(|skill| skill.map_or(0, |skill| encode_skill_machine_state(skill.state))),
            ),
            Arc::new(UInt8Array::from(
                enabled
                    .iter()
                    .map(|skill| {
                        skill
                            .and_then(|skill| skill.attack_phase)
                            .map(encode_attack_phase)
                    })
                    .collect::<Vec<_>>(),
            )),
            i32_values(
                enabled
                    .iter()
                    .map(|skill| skill.map_or(0, |skill| skill.attack_time)),
            ),
            i32_values(
                enabled
                    .iter()
                    .map(|skill| skill.map_or(0, |skill| skill.current_attack_interval)),
            ),
            i32_values(
                enabled
                    .iter()
                    .map(|skill| skill.map_or(0, |skill| skill.attack_count)),
            ),
            i32_values(
                enabled
                    .iter()
                    .map(|skill| skill.map_or(0, |skill| skill.perform_count)),
            ),
            i64_values(field(|skill| skill.attack_range)),
            i64_values(field(|skill| skill.splash_range)),
            i32_values(
                enabled
                    .iter()
                    .map(|skill| skill.map_or(0, |skill| skill.attack_damage)),
            ),
            Arc::new(weapon_lists),
        ],
        Some(enabled.iter().map(Option::is_some).collect::<NullBuffer>()),
    );
    Ok(enabled_items)
}

const SKILL_MACHINE_STATES: [SkillMachineState; 6] = [
    SkillMachineState::Idle,
    SkillMachineState::Prepare,
    SkillMachineState::Attack,
    SkillMachineState::Cooling,
    SkillMachineState::Reloading,
    SkillMachineState::Lock,
];

const ATTACK_PHASES: [AttackPhase; 3] = [
    AttackPhase::Before,
    AttackPhase::Attacking,
    AttackPhase::After,
];

fn encode_skill_machine_state(state: SkillMachineState) -> u8 {
    SKILL_MACHINE_STATES
        .iter()
        .position(|candidate| *candidate == state)
        .and_then(|index| u8::try_from(index).ok())
        .expect("every skill state has a tag")
}

fn encode_attack_phase(phase: AttackPhase) -> u8 {
    ATTACK_PHASES
        .iter()
        .position(|candidate| *candidate == phase)
        .and_then(|index| u8::try_from(index).ok())
        .expect("every attack phase has a tag")
}

fn tick_fields() -> Vec<Field> {
    vec![
        Field::new("tick", DataType::UInt32, false),
        Field::new("tick_hash", DataType::FixedSizeBinary(32), false),
    ]
}

fn unit_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("tick", DataType::UInt32, false),
        Field::new("unit_id", DataType::UInt64, false),
        Field::new("team_id", DataType::UInt32, false),
        Field::new("original_team_id", DataType::UInt32, false),
        Field::new("formation_id", DataType::UInt64, false),
        Field::new("unit_type_id", DataType::UInt32, false),
        Field::new("domain", DataType::UInt8, false),
        struct_field("position", vec3_fields(), false),
        Field::new("body_rotation", DataType::Int64, false),
        Field::new("turret_rotation", DataType::Int64, true),
        struct_field("velocity", planar_fields(), false),
        Field::new("motion_state", DataType::UInt8, false),
        struct_field("mech_lock_target", object_ref_fields(), true),
        Field::new("collision_radius", DataType::Int64, false),
        struct_field("life", gauge_fields(), false),
        Field::new("active", DataType::Boolean, false),
        Field::new("targetable", DataType::Boolean, false),
        Field::new("visibility", DataType::UInt8, false),
        list_field("buffs", buff_fields()),
        struct_field("personal_shield", shield_fields(), false),
        Field::new("move_speed", DataType::Int64, false),
        list_field("skills", skill_fields()),
        struct_field("control", control_fields(), true),
    ]))
}

fn statistic_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("tick", DataType::UInt32, false),
        Field::new("team_id", DataType::UInt32, false),
        Field::new("recorder", DataType::UInt8, false),
        Field::new("recorder_id", DataType::UInt64, false),
        Field::new("damage", DataType::Int32, false),
        Field::new("damage_real", DataType::Int32, false),
        Field::new("kills", DataType::Int32, false),
        Field::new("damage_taken", DataType::Int32, false),
    ]))
}

fn formation_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("tick", DataType::UInt32, false),
        Field::new("formation_id", DataType::UInt64, false),
        Field::new("team_id", DataType::UInt32, false),
        Field::new("experience", DataType::Int64, false),
        Field::new("max_experience", DataType::Int64, false),
    ]))
}

fn shield_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("tick", DataType::UInt32, false),
        Field::new("shield_id", DataType::UInt64, false),
        Field::new("team_id", DataType::UInt32, false),
        Field::new("source_kind", DataType::UInt8, false),
        struct_field("owner", object_ref_fields(), true),
        struct_field("position", vec3_fields(), false),
        Field::new("radius", DataType::Int64, false),
        struct_field("energy", gauge_fields(), false),
        Field::new("round_policy", DataType::UInt8, false),
        Field::new("active", DataType::Boolean, false),
        Field::new("active_order", DataType::UInt32, true),
    ]))
}

fn terrain_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("tick", DataType::UInt32, false),
        Field::new("terrain_id", DataType::UInt64, false),
        Field::new("team_id", DataType::UInt32, true),
        Field::new("terrain_type", DataType::UInt8, false),
        struct_field("position", vec3_fields(), false),
        Field::new("radius", DataType::Int64, false),
        struct_field("grid", terrain_grid_fields(), true),
        Field::new("remaining_rounds", DataType::UInt32, true),
        struct_field("logic_lifetime", terrain_lifetime_fields(), true),
        list_field("applications", terrain_application_fields()),
    ]))
}

fn projectile_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("tick", DataType::UInt32, false),
        Field::new("projectile_id", DataType::UInt64, false),
        Field::new("team_id", DataType::UInt32, false),
        struct_field("owner", object_ref_fields(), true),
        struct_field("position", vec3_fields(), false),
        struct_field("target", object_ref_fields(), true),
        struct_field("cached_target_position", vec3_fields(), false),
        Field::new("cached_target_radius", DataType::Int64, false),
        Field::new("move_range", DataType::Int64, false),
        struct_field("life", gauge_fields(), false),
        list_field("spawn_containing_shields", object_ref_fields()),
    ]))
}

fn building_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("tick", DataType::UInt32, false),
        Field::new("building_id", DataType::UInt64, false),
        Field::new("team_id", DataType::UInt32, false),
        Field::new("building_type_id", DataType::UInt32, false),
        struct_field("position", vec3_fields(), false),
        Field::new("bounds_width", DataType::Int64, false),
        Field::new("bounds_height", DataType::Int64, false),
        struct_field("life", gauge_fields(), false),
        Field::new("available", DataType::Boolean, false),
        Field::new("targetable", DataType::Boolean, false),
        Field::new("collision_enabled", DataType::Boolean, false),
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

fn planar_fields() -> Fields {
    vec![
        Field::new("x", DataType::Int64, false),
        Field::new("z", DataType::Int64, false),
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
        struct_field("energy", gauge_fields(), false),
    ]
    .into()
}

fn gauge_fields() -> Fields {
    vec![
        Field::new("current", DataType::Int32, false),
        Field::new("maximum", DataType::Int32, false),
    ]
    .into()
}

fn terrain_grid_fields() -> Fields {
    vec![
        Field::new("origin_x", DataType::Int64, false),
        Field::new("origin_y", DataType::Int64, false),
        Field::new("size_x", DataType::UInt32, false),
        Field::new("size_y", DataType::UInt32, false),
        Field::new(
            "rows",
            DataType::List(Arc::new(Field::new("item", DataType::UInt32, false))),
            false,
        ),
    ]
    .into()
}

fn terrain_lifetime_fields() -> Fields {
    vec![
        Field::new("elapsed", DataType::Int32, false),
        Field::new("limit", DataType::Int32, false),
    ]
    .into()
}

fn terrain_application_fields() -> Fields {
    vec![
        Field::new("unit_id", DataType::UInt64, false),
        struct_field("periodic_clock", terrain_effect_clock_fields(), true),
    ]
    .into()
}

fn terrain_effect_clock_fields() -> Fields {
    vec![
        Field::new("elapsed", DataType::Int32, false),
        Field::new("duration", DataType::Int32, false),
    ]
    .into()
}

fn buff_data_fields() -> Fields {
    vec![
        Field::new("kind", DataType::UInt8, false),
        Field::new("id", DataType::UInt32, false),
    ]
    .into()
}

fn buff_fields() -> Fields {
    vec![
        struct_field("data", buff_data_fields(), false),
        struct_field("source", object_ref_fields(), true),
        Field::new("source_team", DataType::UInt32, false),
        Field::new("elapsed", DataType::Int32, false),
        Field::new("duration", DataType::Int32, false),
        Field::new("step", DataType::Int32, false),
        Field::new("stacks", DataType::Int32, false),
    ]
    .into()
}

fn skill_fields() -> Fields {
    vec![
        Field::new("skill_slot", DataType::UInt16, false),
        struct_field("enabled", enabled_skill_fields(), true),
    ]
    .into()
}

fn enabled_skill_fields() -> Fields {
    vec![
        struct_field("lock_target", object_ref_fields(), true),
        struct_field("attack_target", object_ref_fields(), true),
        Field::new("state", DataType::UInt8, false),
        Field::new("attack_phase", DataType::UInt8, true),
        Field::new("attack_time", DataType::Int32, false),
        Field::new("current_attack_interval", DataType::Int32, false),
        Field::new("attack_count", DataType::Int32, false),
        Field::new("perform_count", DataType::Int32, false),
        Field::new("attack_range", DataType::Int64, false),
        Field::new("splash_range", DataType::Int64, false),
        Field::new("attack_damage", DataType::Int32, false),
        list_field("weapons", weapon_fields()),
    ]
    .into()
}

fn weapon_fields() -> Fields {
    vec![
        Field::new("weapon_index", DataType::Int32, false),
        struct_field("position", vec3_fields(), true),
        Field::new("rotation", DataType::Int64, true),
    ]
    .into()
}

fn control_fields() -> Fields {
    vec![
        Field::new("progress", DataType::Int32, false),
        list_field("sources", object_ref_fields()),
    ]
    .into()
}

fn control_values<'a>(
    values: impl IntoIterator<Item = Option<&'a ControlState>>,
) -> Result<ArrayRef> {
    let values = values.into_iter().collect::<Vec<_>>();
    let empty = Vec::new();
    Ok(Arc::new(StructArray::new(
        control_fields(),
        vec![
            i32_values(
                values
                    .iter()
                    .map(|value| value.map_or(0, |value| value.progress)),
            ),
            object_ref_list_values(
                values
                    .iter()
                    .map(|value| value.map_or(&empty, |value| &value.sources)),
            )?,
        ],
        Some(values.iter().map(Option::is_some).collect::<NullBuffer>()),
    )))
}

fn read_control(array: &StructArray, index: usize) -> Result<Option<ControlState>> {
    let sources = struct_child::<ListArray>(array, "sources")?;
    if array.is_null(index) {
        if sources.value_length(index) != 0 {
            return Err(Error::invalid(
                "a unit no beam turns carries control sources",
            ));
        }
        return Ok(None);
    }
    Ok(Some(ControlState {
        progress: struct_child::<Int32Array>(array, "progress")?.value(index),
        sources: read_object_ref_list(sources, index, "control sources")?,
    }))
}

fn struct_field(name: &str, fields: Fields, nullable: bool) -> Field {
    Field::new(name, DataType::Struct(fields), nullable)
}

fn list_field(name: &str, fields: Fields) -> Field {
    Field::new(
        name,
        DataType::List(Arc::new(Field::new(
            "item",
            DataType::Struct(fields),
            false,
        ))),
        false,
    )
}

fn read_layout_yaml(member: &MemberSlice, combat_round: u32) -> Result<(String, i32)> {
    if member.len() == 0 || member.len() > MAX_LAYOUT_BYTES {
        return Err(Error::invalid(format!(
            "layout.yaml size must be within 1..={MAX_LAYOUT_BYTES} bytes"
        )));
    }
    let mut bytes = Vec::new();
    member.get_read(0)?.read_to_end(&mut bytes)?;
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) || bytes.contains(&b'\r') {
        return Err(Error::invalid(
            "layout.yaml must be UTF-8 without BOM and use LF line endings",
        ));
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| Error::invalid("layout.yaml is not valid UTF-8"))?;
    let layout = mechcore_document::parse_embedded_yaml(&bytes).map_err(Error::invalid)?;
    if u32::try_from(layout.round).ok() != Some(combat_round) {
        return Err(Error::invalid(format!(
            "layout.yaml round {} differs from durable context combat_round {}",
            layout.round, combat_round
        )));
    }
    let match_seed = layout.seed.ok_or_else(|| {
        Error::invalid("layout.yaml has no seed; a recording embeds the resolved match seed")
    })?;
    let canonical = mechcore_document::canonical_embedded_yaml(layout).map_err(Error::invalid)?;
    if text != canonical {
        return Err(Error::invalid("layout.yaml is not canonical"));
    }
    Ok((canonical, match_seed))
}

fn read_events(member: MemberSlice) -> Result<Vec<(u32, u32, Event)>> {
    let mut rows = Vec::new();
    for batch in checked_builder(member, event_schema().as_ref(), "events")?.build()? {
        for row in batch_events(&batch?)? {
            validate_event_refs(&row.2)?;
            rows.push(row);
        }
    }
    Ok(rows)
}

#[allow(
    clippy::match_same_arms,
    reason = "the payload arms stay in variant order so each event's JSON shape reads in one place"
)]
fn validate_event_refs(event: &Event) -> Result<()> {
    if matches!(event.payload, EventPayload::TerrainCreated { .. }) && event.source.is_some() {
        return Err(Error::invalid("terrain_created does not record source"));
    }
    if let EventPayload::ProjectileReleased {
        skill_slot,
        weapon_index,
    } = event.payload
        && skill_slot.is_some() != weapon_index.is_some()
    {
        return Err(Error::invalid(
            "projectile_released skill_slot and weapon_index must both be present or null",
        ));
    }
    if let EventPayload::ProjectileRemoved {
        intercepted,
        absorbed_by,
        ..
    } = event.payload
    {
        if intercepted && absorbed_by.is_some() {
            return Err(Error::invalid(
                "projectile_removed cannot be intercepted and absorbed_by a shield",
            ));
        }
        if absorbed_by.is_some_and(|reference| reference.kind != ObjectKind::Shield) {
            return Err(Error::invalid(
                "projectile_removed absorbed_by must reference a shield",
            ));
        }
    }
    let required_subject = matches!(
        event.payload,
        EventPayload::ProjectileReleased { .. }
            | EventPayload::ProjectileRemoved { .. }
            | EventPayload::UnitCreated { .. }
            | EventPayload::UnitDied { .. }
            | EventPayload::BuildingDestroyed { .. }
            | EventPayload::UnitTeamChanged { .. }
            | EventPayload::ShieldCreated { .. }
            | EventPayload::ShieldDestroyed { .. }
            | EventPayload::TerrainCreated { .. }
            | EventPayload::TerrainRemoved { .. }
            | EventPayload::TerrainConverted { .. }
    );
    if required_subject && event.subject.is_none() {
        return Err(Error::invalid(format!(
            "{} event requires object",
            event_type_name(event.payload.kind())
        )));
    }
    let required_target = matches!(
        event.payload,
        EventPayload::Damage { .. }
            | EventPayload::Healing { .. }
            | EventPayload::TerrainConverted { .. }
            | EventPayload::BuffApplied { .. }
            | EventPayload::BuffRemoved { .. }
    );
    if required_target && event.target.is_none() {
        return Err(Error::invalid(format!(
            "{} event requires target",
            event_type_name(event.payload.kind())
        )));
    }
    // A damage event's subject is the projectile that carried it, if one did.
    if matches!(event.payload, EventPayload::Damage { .. })
        && event
            .subject
            .is_some_and(|subject| subject.kind != ObjectKind::Projectile)
    {
        return Err(Error::invalid("damage object must be a projectile"));
    }
    if matches!(event.payload, EventPayload::Healing { amount } if amount <= 0) {
        return Err(Error::invalid("healing amount must be positive"));
    }
    Ok(())
}

fn event_type_name(kind: crate::EventKind) -> &'static str {
    match kind {
        crate::EventKind::ProjectileReleased => "projectile_released",
        crate::EventKind::ProjectileRemoved => "projectile_removed",
        crate::EventKind::Damage => "damage",
        crate::EventKind::UnitCreated => "unit_created",
        crate::EventKind::UnitDied => "unit_died",
        crate::EventKind::BuildingDestroyed => "building_destroyed",
        crate::EventKind::UnitTeamChanged => "unit_team_changed",
        crate::EventKind::ShieldCreated => "shield_created",
        crate::EventKind::ShieldDestroyed => "shield_destroyed",
        crate::EventKind::TerrainCreated => "terrain_created",
        crate::EventKind::TerrainRemoved => "terrain_removed",
        crate::EventKind::TerrainConverted => "terrain_converted",
        crate::EventKind::Healing => "healing",
        crate::EventKind::BuffApplied => "buff_applied",
        crate::EventKind::BuffRemoved => "buff_removed",
    }
}

#[allow(
    clippy::match_same_arms,
    reason = "the event-type table stays in schema order so each event's columns read in one place"
)]
pub(crate) fn package_members(directory: &Path, output: &Path) -> Result<()> {
    let file = File::create(output)?;
    let mut archive = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .large_file(true)
        .last_modified_time(zip::DateTime::default());
    let mut names = REQUIRED_MEMBERS.map(str::to_owned).to_vec();
    names.extend(
        TABLE_MEMBERS
            .iter()
            .filter(|name| directory.join(name).exists())
            .map(|name| (*name).to_owned()),
    );
    names.extend(instrument_member_names(directory)?);
    for name in names {
        archive.start_file(name.as_str(), options)?;
        let mut member = File::open(directory.join(&name))?;
        io::copy(&mut member, &mut archive)?;
    }
    archive.finish()?.sync_all()?;
    Ok(())
}

/// The channel members a finished recording directory holds, in name order.
fn instrument_member_names(directory: &Path) -> Result<Vec<String>> {
    let channels = directory.join(INSTRUMENT_DIRECTORY);
    if !channels.exists() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in std::fs::read_dir(channels)? {
        let name = entry?.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| Error::invalid("instrument member name is not UTF-8"))?;
        names.push(format!("{INSTRUMENT_DIRECTORY}/{name}"));
    }
    names.sort();
    Ok(names)
}

/// The channel an `instrument/<channel>.parquet` member holds.
fn instrument_channel(member: &str) -> Option<&str> {
    member
        .strip_prefix(INSTRUMENT_DIRECTORY)?
        .strip_prefix('/')?
        .strip_suffix(".parquet")
        .filter(|channel| instrument::valid_channel_name(channel))
}

pub(crate) struct StoredMetadata {
    pub(crate) producer: Producer,
    pub(crate) game_build: String,
    pub(crate) context: DurableContext,
    pub(crate) tick_count: u32,
    pub(crate) terminal_tick: u32,
    pub(crate) hashes: Hashes,
}

struct TickMetadata {
    producer: Producer,
    game_build: String,
    context: StoredDurableContext,
    tick_count: u32,
    terminal_tick: u32,
    hashes: Hashes,
}

pub(crate) struct StorageReader {
    metadata: StoredMetadata,
    layout_yaml: String,
    member_sizes: BTreeMap<String, u64>,
    tick_hashes: Vec<[u8; canonical::HASH_BYTES]>,
    units: Vec<Vec<LiveUnitState>>,
    projectiles: Vec<Vec<ProjectileState>>,
    buildings: Vec<Vec<BuildingState>>,
    shields: Vec<Vec<ShieldState>>,
    terrains: Vec<Vec<TerrainState>>,
    statistics: Vec<Vec<DamageStatistics>>,
    formations: Vec<Vec<FormationState>>,
    events: Vec<Vec<Event>>,
    instrument: BTreeMap<String, MemberSlice>,
}

/// A published container's hashes, read from its `ticks.parquet` alone with
/// the result hash held to its tick hash column, and its members' sizes.
pub(crate) fn published(path: &Path) -> Result<(Hashes, BTreeMap<String, u64>)> {
    let members = open_members(path)?;
    let member_sizes = members
        .iter()
        .map(|(name, member)| (name.clone(), member.len()))
        .collect();
    let ticks = members
        .get("ticks.parquet")
        .ok_or_else(|| Error::invalid("missing ticks.parquet"))?;
    let (metadata, _) = read_ticks(ticks.clone())?;
    Ok((metadata.hashes, member_sizes))
}

impl StorageReader {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let members = open_members(path)?;
        let member_sizes = members
            .iter()
            .map(|(name, member)| (name.clone(), member.len()))
            .collect();
        let ticks = members
            .get("ticks.parquet")
            .ok_or_else(|| Error::invalid("missing ticks.parquet"))?;
        let (tick_metadata, tick_hashes) = read_ticks(ticks.clone())?;
        let (layout_yaml, match_seed) = read_layout_yaml(
            &member(&members, "layout.yaml")?,
            tick_metadata.context.combat_round,
        )?;
        let tick_count = tick_metadata.tick_count;
        let metadata = StoredMetadata {
            producer: tick_metadata.producer,
            game_build: tick_metadata.game_build,
            context: tick_metadata.context.with_match_seed(match_seed),
            tick_count,
            terminal_tick: tick_metadata.terminal_tick,
            hashes: tick_metadata.hashes,
        };
        let units = group_state_rows(
            table(&members, "units.parquet", read_units)?,
            tick_count,
            |row| row.unit_id,
            "unit",
        )?;
        let projectiles = group_state_rows(
            table(&members, "projectiles.parquet", read_projectiles)?,
            tick_count,
            |row| row.projectile_id,
            "projectile",
        )?;
        let buildings = group_state_rows(
            table(&members, "buildings.parquet", read_buildings)?,
            tick_count,
            |row| row.building_id,
            "building",
        )?;
        let shields = group_state_rows(
            table(&members, "shields.parquet", read_shields)?,
            tick_count,
            |row| row.shield_id,
            "shield",
        )?;
        let terrains = group_state_rows(
            table(&members, "terrains.parquet", read_terrains)?,
            tick_count,
            |row| row.terrain_id,
            "terrain",
        )?;
        let statistics = group_state_rows(
            table(&members, "statistics.parquet", read_statistics)?,
            tick_count,
            statistic_identity,
            "statistics",
        )?;
        let formations = group_state_rows(
            table(&members, "formations.parquet", read_formations)?,
            tick_count,
            |row| row.formation_id,
            "formation",
        )?;
        let events = group_event_rows(table(&members, "events.parquet", read_events)?, tick_count)?;
        let instrument = members
            .iter()
            .filter_map(|(name, slice)| {
                instrument_channel(name).map(|channel| (channel.to_owned(), slice.clone()))
            })
            .collect();
        Ok(Self {
            metadata,
            layout_yaml,
            member_sizes,
            tick_hashes,
            units,
            projectiles,
            buildings,
            shields,
            terrains,
            statistics,
            formations,
            events,
            instrument,
        })
    }

    /// The instrument channels the recording holds, in name order.
    pub(crate) fn instrument_channels(&self) -> impl Iterator<Item = &str> {
        self.instrument.keys().map(String::as_str)
    }

    /// Every row of one channel with its tick, or `None` if it was not recorded.
    pub(crate) fn instrument<R: InstrumentRow>(&self) -> Result<Option<Vec<(u32, R)>>> {
        let Some(slice) = self.instrument.get(R::CHANNEL) else {
            return Ok(None);
        };
        let tick_count = self.metadata.tick_count;
        let mut rows = Vec::new();
        for batch in ParquetRecordBatchReaderBuilder::try_new(slice.clone())?.build()? {
            for (tick, row) in instrument::batch_rows::<R>(&batch?)? {
                if tick == 0 || tick > tick_count {
                    return Err(Error::invalid(format!(
                        "channel {} has a row at tick {tick}, outside 1..={tick_count}",
                        R::CHANNEL
                    )));
                }
                rows.push((tick, row));
            }
        }
        Ok(Some(rows))
    }

    pub(crate) const fn metadata(&self) -> &StoredMetadata {
        &self.metadata
    }

    pub(crate) fn layout_yaml(&self) -> &str {
        &self.layout_yaml
    }

    pub(crate) const fn member_sizes(&self) -> &BTreeMap<String, u64> {
        &self.member_sizes
    }

    pub(crate) fn tick_hash(&self, tick: u32) -> Result<[u8; canonical::HASH_BYTES]> {
        if tick == 0 || tick > self.metadata.tick_count {
            return Err(Error::invalid(format!("tick {tick} is out of range")));
        }
        self.tick_hashes
            .get(usize::try_from(tick - 1).map_err(|_| Error::invalid("tick is too large"))?)
            .copied()
            .ok_or_else(|| Error::invalid(format!("tick {tick} is out of range")))
    }

    pub(crate) fn tick_hashes(&self) -> &[[u8; canonical::HASH_BYTES]] {
        &self.tick_hashes
    }

    pub(crate) fn state(&self, tick: u32) -> Result<WorldSnapshot> {
        let index = state_tick_index(tick, self.metadata.tick_count)?;
        Ok(WorldSnapshot {
            live_units: self.units[index].clone(),
            projectiles: self.projectiles[index].clone(),
            buildings: self.buildings[index].clone(),
            shields: self.shields[index].clone(),
            terrains: self.terrains[index].clone(),
            statistics: self.statistics[index].clone(),
            formations: self.formations[index].clone(),
        })
    }

    pub(crate) fn events(&self, tick: u32) -> Result<TransitionEvents> {
        if tick == 0 {
            return Err(Error::invalid("E(0) does not exist in format 0.3.0"));
        }
        let index = state_tick_index(tick, self.metadata.tick_count)?;
        Ok(TransitionEvents {
            events: self.events[index].clone(),
        })
    }
}

/// The rows of a per-tick table, none when the recording left it out.
fn table<T>(
    members: &BTreeMap<String, MemberSlice>,
    name: &str,
    read: impl FnOnce(MemberSlice) -> Result<Vec<T>>,
) -> Result<Vec<T>> {
    members
        .get(name)
        .cloned()
        .map_or_else(|| Ok(Vec::new()), read)
}

fn member(members: &BTreeMap<String, MemberSlice>, name: &str) -> Result<MemberSlice> {
    members
        .get(name)
        .cloned()
        .ok_or_else(|| Error::invalid(format!("missing {name}")))
}

fn state_tick_index(tick: u32, tick_count: u32) -> Result<usize> {
    if tick == 0 || tick > tick_count {
        return Err(Error::invalid(format!("tick {tick} is out of range")));
    }
    usize::try_from(tick).map_err(|_| Error::invalid("tick index is too large"))
}

/// Tick metadata paired with the per-tick state and trace hash columns.
type TickColumns = (TickMetadata, Vec<[u8; canonical::HASH_BYTES]>);

fn read_ticks(member: MemberSlice) -> Result<TickColumns> {
    let builder = checked_builder(member, &Schema::new(tick_fields()), "ticks")?;
    let metadata = builder.schema().metadata();
    let format = required_metadata(metadata, "format")?;
    if format != MCFR_FORMAT {
        return Err(Error::invalid(format!(
            "unsupported MCFR format {format:?}"
        )));
    }
    if metadata.contains_key("scenario_hash") {
        return Err(Error::invalid(
            "format 0.3.0 ticks.parquet must not contain scenario_hash",
        ));
    }
    let context_bytes = required_metadata(metadata, "durable_context")?.as_bytes();
    let context: StoredDurableContext = canonical::decode(context_bytes, "durable context")?;
    context.clone().with_match_seed(0).validate()?;
    let producer = Producer::parse(required_metadata(metadata, "producer")?)?;
    let game_build = required_metadata(metadata, "game_build")?.to_owned();
    if game_build.trim().is_empty() {
        return Err(Error::invalid(
            "ticks.parquet metadata game_build must not be empty",
        ));
    }
    if required_metadata(metadata, "hash_profile")? != HASH_PROFILE {
        return Err(Error::invalid("unsupported hash profile"));
    }
    let hashes = Hashes {
        result_hash: required_metadata(metadata, "result_hash")?.to_owned(),
    };
    hashes.validate_encoding()?;
    let tick_count = parse_metadata_u32(metadata, "tick_count")?;
    let terminal_tick = parse_metadata_u32(metadata, "terminal_tick")?;
    if tick_count == 0 || terminal_tick != tick_count {
        return Err(Error::invalid(
            "MCFR must contain T(1) and terminal_tick must equal tick_count",
        ));
    }
    let mut tick_hashes_out = Vec::new();
    let mut expected_tick = 1_u32;
    for batch in builder.build()? {
        let batch = batch?;
        let ticks = column::<UInt32Array>(&batch, "tick")?;
        let tick_hashes = column::<FixedSizeBinaryArray>(&batch, "tick_hash")?;
        for index in 0..batch.num_rows() {
            if ticks.value(index) != expected_tick {
                return Err(Error::invalid(format!(
                    "ticks.parquet expected tick {expected_tick}, found {}",
                    ticks.value(index)
                )));
            }
            let value: [u8; canonical::HASH_BYTES] = tick_hashes
                .value(index)
                .try_into()
                .map_err(|_| Error::invalid("tick_hash is not 32 bytes"))?;
            tick_hashes_out.push(value);
            expected_tick += 1;
        }
    }
    if expected_tick != tick_count.saturating_add(1) {
        return Err(Error::invalid(format!(
            "tick_count metadata is {tick_count}, found {} tick rows",
            expected_tick.saturating_sub(1)
        )));
    }
    if canonical::hex(&canonical::result_hash(&tick_hashes_out)) != hashes.result_hash {
        return Err(Error::invalid(
            "result_hash metadata is not the hash of the tick_hash column",
        ));
    }
    Ok((
        TickMetadata {
            producer,
            game_build,
            context,
            tick_count,
            terminal_tick,
            hashes,
        },
        tick_hashes_out,
    ))
}

fn required_metadata<'a>(metadata: &'a HashMap<String, String>, name: &str) -> Result<&'a str> {
    metadata
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| Error::invalid(format!("ticks.parquet metadata lacks {name}")))
}

fn parse_metadata_u32(metadata: &HashMap<String, String>, name: &str) -> Result<u32> {
    required_metadata(metadata, name)?
        .parse()
        .map_err(|_| Error::invalid(format!("ticks.parquet metadata {name} is not a u32")))
}

fn group_state_rows<T>(
    rows: Vec<(u32, T)>,
    tick_count: u32,
    identity: impl Fn(&T) -> u64,
    label: &str,
) -> Result<Vec<Vec<T>>> {
    let len = usize::try_from(tick_count)
        .map_err(|_| Error::invalid("tick count is too large"))?
        .checked_add(1)
        .ok_or_else(|| Error::invalid("tick count is too large"))?;
    let mut grouped = (0..len).map(|_| Vec::new()).collect::<Vec<Vec<T>>>();
    let mut previous = None::<(u32, u64)>;
    for (tick, row) in rows {
        let id = identity(&row);
        let index = state_tick_index(tick, tick_count)?;
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

fn group_event_rows(rows: Vec<(u32, u32, Event)>, tick_count: u32) -> Result<Vec<Vec<Event>>> {
    let len = usize::try_from(tick_count)
        .map_err(|_| Error::invalid("tick count is too large"))?
        .checked_add(1)
        .ok_or_else(|| Error::invalid("tick count is too large"))?;
    let mut grouped = (0..len).map(|_| Vec::new()).collect::<Vec<Vec<Event>>>();
    let mut previous_tick = None;
    let mut expected_ordinal = 0_u32;
    for (tick, ordinal, event) in rows {
        if tick == 0 {
            return Err(Error::invalid("events.jsonl contains E(0)"));
        }
        let index = state_tick_index(tick, tick_count)?;
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

fn read_units(member: MemberSlice) -> Result<Vec<(u32, LiveUnitState)>> {
    let mut rows = Vec::new();
    for batch in checked_builder(member, unit_schema().as_ref(), "units")?.build()? {
        let batch = batch?;
        let tick = column::<UInt32Array>(&batch, "tick")?;
        let id = column::<UInt64Array>(&batch, "unit_id")?;
        let team = column::<UInt32Array>(&batch, "team_id")?;
        let original_team = column::<UInt32Array>(&batch, "original_team_id")?;
        let formation = column::<UInt64Array>(&batch, "formation_id")?;
        let type_id = column::<UInt32Array>(&batch, "unit_type_id")?;
        let domain = column::<UInt8Array>(&batch, "domain")?;
        let position = struct_column(&batch, "position")?;
        let body_rotation = column::<Int64Array>(&batch, "body_rotation")?;
        let turret_rotation = column::<Int64Array>(&batch, "turret_rotation")?;
        let velocity = struct_column(&batch, "velocity")?;
        let motion = column::<UInt8Array>(&batch, "motion_state")?;
        let target = struct_column(&batch, "mech_lock_target")?;
        let radius = column::<Int64Array>(&batch, "collision_radius")?;
        let life = struct_column(&batch, "life")?;
        let active = column::<BooleanArray>(&batch, "active")?;
        let targetable = column::<BooleanArray>(&batch, "targetable")?;
        let visibility = column::<UInt8Array>(&batch, "visibility")?;
        let buffs = column::<ListArray>(&batch, "buffs")?;
        let shield = struct_column(&batch, "personal_shield")?;
        let move_speed = column::<Int64Array>(&batch, "move_speed")?;
        let skills = column::<ListArray>(&batch, "skills")?;
        let control = struct_column(&batch, "control")?;
        for index in 0..batch.num_rows() {
            rows.push((
                tick.value(index),
                LiveUnitState {
                    unit_id: id.value(index),
                    team_id: team.value(index),
                    original_team_id: original_team.value(index),
                    formation_id: formation.value(index),
                    unit_type_id: type_id.value(index),
                    domain: decode_domain(domain.value(index))?,
                    position: read_vec3(position, index)?,
                    body_rotation: body_rotation.value(index),
                    turret_rotation: turret_rotation
                        .is_valid(index)
                        .then(|| turret_rotation.value(index)),
                    velocity: read_planar(velocity, index)?,
                    motion_state: decode_motion(motion.value(index))?,
                    mech_lock_target: read_optional_ref(target, index)?,
                    collision_radius: radius.value(index),
                    life: read_gauge(life, index)?,
                    active: active.value(index),
                    targetable: targetable.value(index),
                    visibility: decode_visibility(visibility.value(index))?,
                    buffs: read_buff_list(buffs, index)?,
                    personal_shield: read_shield(shield, index)?,
                    move_speed: move_speed.value(index),
                    skills: read_skill_list(skills, index)?,
                    control: read_control(control, index)?,
                },
            ));
        }
    }
    Ok(rows)
}

fn read_statistics(member: MemberSlice) -> Result<Vec<(u32, DamageStatistics)>> {
    let mut rows = Vec::new();
    for batch in checked_builder(member, statistic_schema().as_ref(), "statistics")?.build()? {
        let batch = batch?;
        let tick = column::<UInt32Array>(&batch, "tick")?;
        let team = column::<UInt32Array>(&batch, "team_id")?;
        let recorder = column::<UInt8Array>(&batch, "recorder")?;
        let recorder_id = column::<UInt64Array>(&batch, "recorder_id")?;
        let damage = column::<Int32Array>(&batch, "damage")?;
        let damage_real = column::<Int32Array>(&batch, "damage_real")?;
        let kills = column::<Int32Array>(&batch, "kills")?;
        let damage_taken = column::<Int32Array>(&batch, "damage_taken")?;
        for index in 0..batch.num_rows() {
            rows.push((
                tick.value(index),
                DamageStatistics {
                    team_id: team.value(index),
                    recorder: *RECORDER_KINDS
                        .get(usize::from(recorder.value(index)))
                        .ok_or_else(|| {
                            Error::invalid(format!("recorder tag {}", recorder.value(index)))
                        })?,
                    recorder_id: recorder_id.value(index),
                    damage: damage.value(index),
                    damage_real: damage_real.value(index),
                    kills: kills.value(index),
                    damage_taken: damage_taken.value(index),
                },
            ));
        }
    }
    Ok(rows)
}

fn read_formations(member: MemberSlice) -> Result<Vec<(u32, FormationState)>> {
    let mut rows = Vec::new();
    for batch in checked_builder(member, formation_schema().as_ref(), "formations")?.build()? {
        let batch = batch?;
        let tick = column::<UInt32Array>(&batch, "tick")?;
        let id = column::<UInt64Array>(&batch, "formation_id")?;
        let team = column::<UInt32Array>(&batch, "team_id")?;
        let experience = column::<Int64Array>(&batch, "experience")?;
        let max_experience = column::<Int64Array>(&batch, "max_experience")?;
        for index in 0..batch.num_rows() {
            rows.push((
                tick.value(index),
                FormationState {
                    formation_id: id.value(index),
                    team_id: team.value(index),
                    experience: experience.value(index),
                    max_experience: max_experience.value(index),
                },
            ));
        }
    }
    Ok(rows)
}

fn read_shields(member: MemberSlice) -> Result<Vec<(u32, ShieldState)>> {
    let mut rows = Vec::new();
    for batch in checked_builder(member, shield_schema().as_ref(), "shields")?.build()? {
        let batch = batch?;
        let tick = column::<UInt32Array>(&batch, "tick")?;
        let id = column::<UInt64Array>(&batch, "shield_id")?;
        let team = column::<UInt32Array>(&batch, "team_id")?;
        let source_kind = column::<UInt8Array>(&batch, "source_kind")?;
        let owner = struct_column(&batch, "owner")?;
        let position = struct_column(&batch, "position")?;
        let radius = column::<Int64Array>(&batch, "radius")?;
        let energy = struct_column(&batch, "energy")?;
        let round_policy = column::<UInt8Array>(&batch, "round_policy")?;
        let active = column::<BooleanArray>(&batch, "active")?;
        let active_order = column::<UInt32Array>(&batch, "active_order")?;
        for index in 0..batch.num_rows() {
            rows.push((
                tick.value(index),
                ShieldState {
                    shield_id: id.value(index),
                    team_id: team.value(index),
                    source_kind: decode_shield_source(source_kind.value(index))?,
                    owner: read_optional_ref(owner, index)?,
                    position: read_vec3(position, index)?,
                    radius: radius.value(index),
                    energy: read_gauge(energy, index)?,
                    round_policy: decode_shield_round_policy(round_policy.value(index))?,
                    active: active.value(index),
                    active_order: (!active_order.is_null(index)).then(|| active_order.value(index)),
                },
            ));
        }
    }
    Ok(rows)
}

fn read_terrains(member: MemberSlice) -> Result<Vec<(u32, TerrainState)>> {
    let mut rows = Vec::new();
    for batch in checked_builder(member, terrain_schema().as_ref(), "terrains")?.build()? {
        let batch = batch?;
        let tick = column::<UInt32Array>(&batch, "tick")?;
        let id = column::<UInt64Array>(&batch, "terrain_id")?;
        let team = column::<UInt32Array>(&batch, "team_id")?;
        let terrain_type = column::<UInt8Array>(&batch, "terrain_type")?;
        let position = struct_column(&batch, "position")?;
        let radius = column::<Int64Array>(&batch, "radius")?;
        let grid = struct_column(&batch, "grid")?;
        let remaining_rounds = column::<UInt32Array>(&batch, "remaining_rounds")?;
        let logic_lifetime = struct_column(&batch, "logic_lifetime")?;
        let applications = column::<ListArray>(&batch, "applications")?;
        for index in 0..batch.num_rows() {
            rows.push((
                tick.value(index),
                TerrainState {
                    terrain_id: id.value(index),
                    team_id: (!team.is_null(index)).then(|| team.value(index)),
                    terrain_type: decode_terrain_type(terrain_type.value(index))?,
                    position: read_vec3(position, index)?,
                    radius: radius.value(index),
                    grid: read_terrain_grid(grid, index)?,
                    remaining_rounds: (!remaining_rounds.is_null(index))
                        .then(|| remaining_rounds.value(index)),
                    logic_lifetime: read_terrain_lifetime(logic_lifetime, index)?,
                    applications: read_terrain_applications(applications, index)?,
                },
            ));
        }
    }
    Ok(rows)
}

fn read_terrain_grid(array: &StructArray, index: usize) -> Result<Option<TerrainGridState>> {
    if array.is_null(index) {
        return Ok(None);
    }
    let origin_x = struct_child::<Int64Array>(array, "origin_x")?.value(index);
    let origin_y = struct_child::<Int64Array>(array, "origin_y")?.value(index);
    let size_x = struct_child::<UInt32Array>(array, "size_x")?.value(index);
    let size_y = struct_child::<UInt32Array>(array, "size_y")?.value(index);
    let rows = struct_child::<ListArray>(array, "rows")?;
    Ok(Some(TerrainGridState {
        origin_x,
        origin_y,
        size_x,
        size_y,
        rows: read_u32_list(rows, index, "terrain grid rows")?,
    }))
}

fn read_terrain_lifetime(
    array: &StructArray,
    index: usize,
) -> Result<Option<TerrainLogicLifetime>> {
    if array.is_null(index) {
        return Ok(None);
    }
    Ok(Some(TerrainLogicLifetime {
        elapsed: struct_child::<Int32Array>(array, "elapsed")?.value(index),
        limit: struct_child::<Int32Array>(array, "limit")?.value(index),
    }))
}

fn read_terrain_applications(
    array: &ListArray,
    index: usize,
) -> Result<Vec<TerrainApplicationState>> {
    let items = list_struct_items(array, index, "terrain applications")?;
    let unit_id = struct_child::<UInt64Array>(&items, "unit_id")?;
    let periodic_clock = struct_child::<StructArray>(&items, "periodic_clock")?;
    (0..items.len())
        .map(|item| {
            Ok(TerrainApplicationState {
                unit_id: unit_id.value(item),
                periodic_clock: if periodic_clock.is_null(item) {
                    None
                } else {
                    Some(TerrainEffectClock {
                        elapsed: struct_child::<Int32Array>(periodic_clock, "elapsed")?.value(item),
                        duration: struct_child::<Int32Array>(periodic_clock, "duration")?
                            .value(item),
                    })
                },
            })
        })
        .collect()
}

fn read_u32_list(array: &ListArray, index: usize, label: &str) -> Result<Vec<u32>> {
    if array.is_null(index) {
        return Err(Error::invalid(format!("{label} list is null")));
    }
    let values = array.value(index);
    let values = values
        .as_any()
        .downcast_ref::<UInt32Array>()
        .ok_or_else(|| Error::invalid(format!("{label} items have the wrong type")))?;
    Ok(values.values().to_vec())
}

fn read_projectiles(member: MemberSlice) -> Result<Vec<(u32, ProjectileState)>> {
    let mut rows = Vec::new();
    for batch in checked_builder(member, projectile_schema().as_ref(), "projectiles")?.build()? {
        let batch = batch?;
        let tick = column::<UInt32Array>(&batch, "tick")?;
        let id = column::<UInt64Array>(&batch, "projectile_id")?;
        let team = column::<UInt32Array>(&batch, "team_id")?;
        let owner = struct_column(&batch, "owner")?;
        let position = struct_column(&batch, "position")?;
        let target = struct_column(&batch, "target")?;
        let cached = struct_column(&batch, "cached_target_position")?;
        let radius = column::<Int64Array>(&batch, "cached_target_radius")?;
        let move_range = column::<Int64Array>(&batch, "move_range")?;
        let life = struct_column(&batch, "life")?;
        let spawn_containing_shields = column::<ListArray>(&batch, "spawn_containing_shields")?;
        for index in 0..batch.num_rows() {
            rows.push((
                tick.value(index),
                ProjectileState {
                    projectile_id: id.value(index),
                    team_id: team.value(index),
                    owner: read_optional_ref(owner, index)?,
                    position: read_vec3(position, index)?,
                    target: read_optional_ref(target, index)?,
                    cached_target_position: read_vec3(cached, index)?,
                    cached_target_radius: radius.value(index),
                    move_range: move_range.value(index),
                    life: read_gauge(life, index)?,
                    spawn_containing_shields: read_object_ref_list(
                        spawn_containing_shields,
                        index,
                        "spawn_containing_shields",
                    )?,
                },
            ));
        }
    }
    Ok(rows)
}

fn read_buildings(member: MemberSlice) -> Result<Vec<(u32, BuildingState)>> {
    let mut rows = Vec::new();
    for batch in checked_builder(member, building_schema().as_ref(), "buildings")?.build()? {
        let batch = batch?;
        let tick = column::<UInt32Array>(&batch, "tick")?;
        let id = column::<UInt64Array>(&batch, "building_id")?;
        let team = column::<UInt32Array>(&batch, "team_id")?;
        let type_id = column::<UInt32Array>(&batch, "building_type_id")?;
        let position = struct_column(&batch, "position")?;
        let width = column::<Int64Array>(&batch, "bounds_width")?;
        let height = column::<Int64Array>(&batch, "bounds_height")?;
        let life = struct_column(&batch, "life")?;
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
                    bounds_width: width.value(index),
                    bounds_height: height.value(index),
                    life: read_gauge(life, index)?,
                    available: available.value(index),
                    targetable: targetable.value(index),
                    collision_enabled: collision.value(index),
                },
            ));
        }
    }
    Ok(rows)
}

fn read_vec3(array: &StructArray, index: usize) -> Result<QVec3> {
    Ok(QVec3 {
        x: struct_child::<Int64Array>(array, "x")?.value(index),
        y: struct_child::<Int64Array>(array, "y")?.value(index),
        z: struct_child::<Int64Array>(array, "z")?.value(index),
    })
}

fn read_planar(array: &StructArray, index: usize) -> Result<QPlanar> {
    Ok(QPlanar {
        x: struct_child::<Int64Array>(array, "x")?.value(index),
        z: struct_child::<Int64Array>(array, "z")?.value(index),
    })
}

fn read_shield(array: &StructArray, index: usize) -> Result<PersonalShieldState> {
    Ok(PersonalShieldState {
        active: struct_child::<BooleanArray>(array, "active")?.value(index),
        enabled: struct_child::<BooleanArray>(array, "enabled")?.value(index),
        energy: read_gauge(struct_child::<StructArray>(array, "energy")?, index)?,
    })
}

fn read_gauge(array: &StructArray, index: usize) -> Result<GaugeI32> {
    Ok(GaugeI32 {
        current: struct_child::<Int32Array>(array, "current")?.value(index),
        maximum: struct_child::<Int32Array>(array, "maximum")?.value(index),
    })
}

fn read_buff_list(array: &ListArray, index: usize) -> Result<Vec<BuffState>> {
    let items = list_struct_items(array, index, "buffs")?;
    let data = struct_child::<StructArray>(&items, "data")?;
    let kinds = struct_child::<UInt8Array>(data, "kind")?;
    let ids = struct_child::<UInt32Array>(data, "id")?;
    let sources = struct_child::<StructArray>(&items, "source")?;
    let teams = struct_child::<UInt32Array>(&items, "source_team")?;
    let elapsed = struct_child::<Int32Array>(&items, "elapsed")?;
    let durations = struct_child::<Int32Array>(&items, "duration")?;
    let steps = struct_child::<Int32Array>(&items, "step")?;
    let stacks = struct_child::<Int32Array>(&items, "stacks")?;
    (0..items.len())
        .map(|item| {
            Ok(BuffState {
                data: BuffDataRef {
                    kind: *BUFF_DATA_KINDS
                        .get(usize::from(kinds.value(item)))
                        .ok_or_else(|| {
                            Error::invalid(format!("buff data kind tag {}", kinds.value(item)))
                        })?,
                    id: ids.value(item),
                },
                source: read_optional_ref(sources, item)?,
                source_team: teams.value(item),
                elapsed: elapsed.value(item),
                duration: durations.value(item),
                step: steps.value(item),
                stacks: stacks.value(item),
            })
        })
        .collect()
}

fn read_skill_list(array: &ListArray, index: usize) -> Result<Vec<SkillState>> {
    let items = list_struct_items(array, index, "skills")?;
    let slots = struct_child::<UInt16Array>(&items, "skill_slot")?;
    let enabled = struct_child::<StructArray>(&items, "enabled")?;
    let locks = struct_child::<StructArray>(enabled, "lock_target")?;
    let targets = struct_child::<StructArray>(enabled, "attack_target")?;
    let states = struct_child::<UInt8Array>(enabled, "state")?;
    let phases = struct_child::<UInt8Array>(enabled, "attack_phase")?;
    let times = struct_child::<Int32Array>(enabled, "attack_time")?;
    let intervals = struct_child::<Int32Array>(enabled, "current_attack_interval")?;
    let counts = struct_child::<Int32Array>(enabled, "attack_count")?;
    let performed = struct_child::<Int32Array>(enabled, "perform_count")?;
    let ranges = struct_child::<Int64Array>(enabled, "attack_range")?;
    let splashes = struct_child::<Int64Array>(enabled, "splash_range")?;
    let damages = struct_child::<Int32Array>(enabled, "attack_damage")?;
    let weapons = struct_child::<ListArray>(enabled, "weapons")?;
    (0..items.len())
        .map(|item| {
            let tag = |tags: &UInt8Array, what: &str| {
                Error::invalid(format!("skill {what} tag {}", tags.value(item)))
            };
            let read = if enabled.is_null(item) {
                if weapons.value_length(item) != 0 {
                    return Err(Error::invalid("a switched-off skill carries weapons"));
                }
                None
            } else {
                Some(EnabledSkill {
                    lock_target: read_optional_ref(locks, item)?,
                    attack_target: read_optional_ref(targets, item)?,
                    state: *SKILL_MACHINE_STATES
                        .get(usize::from(states.value(item)))
                        .ok_or_else(|| tag(states, "state"))?,
                    attack_phase: if phases.is_null(item) {
                        None
                    } else {
                        Some(
                            *ATTACK_PHASES
                                .get(usize::from(phases.value(item)))
                                .ok_or_else(|| tag(phases, "attack phase"))?,
                        )
                    },
                    attack_time: times.value(item),
                    current_attack_interval: intervals.value(item),
                    attack_count: counts.value(item),
                    perform_count: performed.value(item),
                    attack_range: ranges.value(item),
                    splash_range: splashes.value(item),
                    attack_damage: damages.value(item),
                    weapons: read_weapon_list(weapons, item)?,
                })
            };
            Ok(SkillState {
                skill_slot: slots.value(item),
                enabled: read,
            })
        })
        .collect()
}

fn read_weapon_list(array: &ListArray, index: usize) -> Result<Vec<WeaponState>> {
    let items = list_struct_items(array, index, "weapons")?;
    let weapon_indexes = struct_child::<Int32Array>(&items, "weapon_index")?;
    let positions = struct_child::<StructArray>(&items, "position")?;
    let rotations = struct_child::<Int64Array>(&items, "rotation")?;
    (0..items.len())
        .map(|item| {
            let position_is_null = positions.is_null(item);
            if position_is_null != rotations.is_null(item) {
                return Err(Error::invalid(
                    "weapon position and rotation nullability differ",
                ));
            }
            Ok(WeaponState {
                weapon_index: weapon_indexes.value(item),
                pose: if position_is_null {
                    None
                } else {
                    Some(QPose {
                        position: read_vec3(positions, item)?,
                        rotation: rotations.value(item),
                    })
                },
            })
        })
        .collect()
}

fn read_object_ref_list(array: &ListArray, index: usize, label: &str) -> Result<Vec<ObjectRef>> {
    let items = list_struct_items(array, index, label)?;
    (0..items.len())
        .map(|item| read_required_ref(&items, item))
        .collect()
}

fn list_struct_items(array: &ListArray, index: usize, label: &str) -> Result<StructArray> {
    if array.is_null(index) {
        return Err(Error::invalid(format!("{label} list is null")));
    }
    array
        .value(index)
        .as_any()
        .downcast_ref::<StructArray>()
        .cloned()
        .ok_or_else(|| Error::invalid(format!("{label} items have the wrong type")))
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
    let expected = REQUIRED_MEMBERS
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    let mut ranges = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let name = entry.name().to_owned();
        let known = expected.contains(&name)
            || TABLE_MEMBERS.contains(&name.as_str())
            || instrument_channel(&name).is_some();
        if !known || !seen.insert(name.clone()) {
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
    if !expected.is_subset(&seen) {
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

pub(crate) const fn encode_kind(value: ObjectKind) -> u8 {
    match value {
        ObjectKind::Unit => 0,
        ObjectKind::Projectile => 1,
        ObjectKind::Building => 2,
        ObjectKind::Shield => 3,
        ObjectKind::Terrain => 4,
    }
}

pub(crate) fn decode_kind(value: u8) -> Result<ObjectKind> {
    match value {
        0 => Ok(ObjectKind::Unit),
        1 => Ok(ObjectKind::Projectile),
        2 => Ok(ObjectKind::Building),
        3 => Ok(ObjectKind::Shield),
        4 => Ok(ObjectKind::Terrain),
        _ => Err(Error::invalid(format!("invalid ObjectKind tag {value}"))),
    }
}

pub(crate) const fn encode_terrain_type(value: TerrainType) -> u8 {
    match value {
        TerrainType::Fire => 0,
        TerrainType::Oil => 1,
        TerrainType::Fog => 2,
        TerrainType::Acid => 3,
        TerrainType::RecoveryZone => 4,
        TerrainType::FogSand => 5,
    }
}

pub(crate) fn decode_terrain_type(value: u8) -> Result<TerrainType> {
    match value {
        0 => Ok(TerrainType::Fire),
        1 => Ok(TerrainType::Oil),
        2 => Ok(TerrainType::Fog),
        3 => Ok(TerrainType::Acid),
        4 => Ok(TerrainType::RecoveryZone),
        5 => Ok(TerrainType::FogSand),
        _ => Err(Error::invalid(format!("invalid TerrainType tag {value}"))),
    }
}

pub(crate) const fn encode_shield_source(value: ShieldSourceKind) -> u8 {
    match value {
        ShieldSourceKind::Contraption => 0,
        ShieldSourceKind::CommanderSkill => 1,
        ShieldSourceKind::OwnerAdvanced => 2,
        ShieldSourceKind::SpawnedTemporary => 3,
    }
}

pub(crate) fn decode_shield_source(value: u8) -> Result<ShieldSourceKind> {
    match value {
        0 => Ok(ShieldSourceKind::Contraption),
        1 => Ok(ShieldSourceKind::CommanderSkill),
        2 => Ok(ShieldSourceKind::OwnerAdvanced),
        3 => Ok(ShieldSourceKind::SpawnedTemporary),
        _ => Err(Error::invalid(format!(
            "invalid ShieldSourceKind tag {value}"
        ))),
    }
}

const fn encode_shield_round_policy(value: ShieldRoundPolicy) -> u8 {
    match value {
        ShieldRoundPolicy::DestroyAtRoundEnd => 0,
        ShieldRoundPolicy::ResetToMax => 1,
        ShieldRoundPolicy::RetainState => 2,
    }
}

fn decode_shield_round_policy(value: u8) -> Result<ShieldRoundPolicy> {
    match value {
        0 => Ok(ShieldRoundPolicy::DestroyAtRoundEnd),
        1 => Ok(ShieldRoundPolicy::ResetToMax),
        2 => Ok(ShieldRoundPolicy::RetainState),
        _ => Err(Error::invalid(format!(
            "invalid ShieldRoundPolicy tag {value}"
        ))),
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
        MotionState::Transitioning => 4,
    }
}

fn decode_motion(value: u8) -> Result<MotionState> {
    match value {
        0 => Ok(MotionState::Idle),
        1 => Ok(MotionState::Moving),
        2 => Ok(MotionState::Attacking),
        3 => Ok(MotionState::Stopped),
        4 => Ok(MotionState::Transitioning),
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
