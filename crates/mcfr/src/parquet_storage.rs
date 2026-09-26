use std::fmt::Write as _;
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
    arrow::{ArrowWriter, arrow_reader::ParquetRecordBatchReaderBuilder},
    basic::{Compression, Encoding, ZstdLevel},
    file::{
        properties::WriterProperties,
        reader::{ChunkReader, Length},
    },
    schema::types::ColumnPath,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tempfile::TempDir;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::{
    BuffModifierSet, BuildingState, CONTENT_HASH_PROFILE, DerivedStats, Domain, DurableContext,
    Error, Event, EventPayload, GaugeI32, Hashes, LiveUnitState, MCFR_FORMAT, MotionState,
    ObjectKind, ObjectRef, PHYSICS_HASH_PROFILE, PersonalShieldState, ProjectileState, QPose,
    QVec3, RateModifier, Result, ShieldDestroyedReason, ShieldRoundPolicy, ShieldSourceKind,
    ShieldState, SkillDynamicModifierSet, SkillNumericModifierState, TerrainApplicationState,
    TerrainEffectClock, TerrainGridState, TerrainLogicLifetime, TerrainRemovedReason, TerrainState,
    TerrainType, TransitionEvents, UnitDynamicModifierSet, ValueModifier, Visibility,
    WeaponAimState, WorldSnapshot, canonical,
};

pub(crate) const MEMBER_NAMES: [&str; 8] = [
    "layout.yaml",
    "ticks.parquet",
    "units.parquet",
    "projectiles.parquet",
    "buildings.parquet",
    "shields.parquet",
    "terrains.parquet",
    "events.jsonl",
];

const TICKS_PER_ROW_GROUP: u64 = 128;
const ROWS_PER_TICK_GROUP: usize = 128;
const MAX_LAYOUT_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredDurableContext {
    logic_step: crate::Rational,
    time_units_per_second: u32,
    combat_round: u32,
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
    terrains: Option<ArrowWriter<File>>,
    unit_rows: Vec<(u32, LiveUnitState)>,
    projectile_rows: Vec<(u32, ProjectileState)>,
    building_rows: Vec<(u32, BuildingState)>,
    shield_rows: Vec<(u32, ShieldState)>,
    terrain_rows: Vec<(u32, TerrainState)>,
    event_rows: Vec<(u32, u32, Event)>,
    physics_tick_hashes: Vec<[u8; canonical::HASH_BYTES]>,
    content_tick_hashes: Vec<[u8; canonical::HASH_BYTES]>,
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
        let shields = create_member(
            directory.path().join("shields.parquet"),
            shield_schema(),
            Track::Shields,
        )?;
        let terrains = create_member(
            directory.path().join("terrains.parquet"),
            terrain_schema(),
            Track::Terrains,
        )?;
        Ok(Self {
            directory,
            units: Some(units),
            projectiles: Some(projectiles),
            buildings: Some(buildings),
            shields: Some(shields),
            terrains: Some(terrains),
            unit_rows: Vec::new(),
            projectile_rows: Vec::new(),
            building_rows: Vec::new(),
            shield_rows: Vec::new(),
            terrain_rows: Vec::new(),
            event_rows: Vec::new(),
            physics_tick_hashes: Vec::new(),
            content_tick_hashes: Vec::new(),
        })
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
    }

    pub(crate) fn append_tick(
        &mut self,
        tick: u32,
        state: &WorldSnapshot,
        events: &TransitionEvents,
        physics_tick_hash: [u8; canonical::HASH_BYTES],
        content_tick_hash: [u8; canonical::HASH_BYTES],
    ) -> Result<()> {
        self.append_state(tick, state);
        for (ordinal, event) in events.events.iter().cloned().enumerate() {
            self.event_rows.push((
                tick,
                u32::try_from(ordinal).map_err(|_| Error::invalid("event ordinal overflow"))?,
                event,
            ));
        }
        self.physics_tick_hashes.push(physics_tick_hash);
        self.content_tick_hashes.push(content_tick_hash);
        if u64::from(tick).is_multiple_of(TICKS_PER_ROW_GROUP) {
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
        write_buffer(&mut self.shields, shield_batch(&self.shield_rows)?)?;
        write_buffer(&mut self.terrains, terrain_batch(&self.terrain_rows)?)?;
        self.unit_rows.clear();
        self.projectile_rows.clear();
        self.building_rows.clear();
        self.shield_rows.clear();
        self.terrain_rows.clear();
        Ok(())
    }

    pub(crate) fn finish(
        mut self,
        game_build: &str,
        context_bytes: &[u8],
        layout_yaml: &str,
        hashes: &Hashes,
    ) -> Result<TempDir> {
        self.flush()?;
        close_writer(&mut self.units)?;
        close_writer(&mut self.projectiles)?;
        close_writer(&mut self.buildings)?;
        close_writer(&mut self.shields)?;
        close_writer(&mut self.terrains)?;
        write_events_jsonl(
            &self.directory.path().join("events.jsonl"),
            &self.event_rows,
        )?;
        let mut layout = File::create(self.directory.path().join("layout.yaml"))?;
        layout.write_all(layout_yaml.as_bytes())?;
        layout.sync_all()?;

        debug_assert_eq!(
            self.physics_tick_hashes.len(),
            self.content_tick_hashes.len()
        );
        let tick_count = u32::try_from(self.physics_tick_hashes.len())
            .map_err(|_| Error::invalid("tick count overflow"))?;
        if tick_count == 0 {
            return Err(Error::invalid("an MCFR must contain at least T(1)"));
        }
        let terminal_tick = tick_count;
        let context_json = std::str::from_utf8(context_bytes)
            .map_err(|_| Error::invalid("canonical durable context is not UTF-8"))?;
        let metadata = HashMap::from([
            ("format".to_owned(), MCFR_FORMAT.to_owned()),
            ("game_build".to_owned(), game_build.to_owned()),
            ("durable_context".to_owned(), context_json.to_owned()),
            (
                "physics_hash_profile".to_owned(),
                PHYSICS_HASH_PROFILE.to_owned(),
            ),
            (
                "physics_result_hash".to_owned(),
                hashes.physics_result_hash.clone(),
            ),
            (
                "content_hash_profile".to_owned(),
                CONTENT_HASH_PROFILE.to_owned(),
            ),
            (
                "content_result_hash".to_owned(),
                hashes.content_result_hash.clone(),
            ),
            ("tick_count".to_owned(), tick_count.to_string()),
            ("terminal_tick".to_owned(), terminal_tick.to_string()),
        ]);
        let schema = Arc::new(Schema::new(tick_fields()).with_metadata(metadata));
        let mut ticks = create_member(
            self.directory.path().join("ticks.parquet"),
            schema,
            Track::Ticks,
        )?;
        for start in (0..self.physics_tick_hashes.len()).step_by(ROWS_PER_TICK_GROUP) {
            let end = (start + ROWS_PER_TICK_GROUP).min(self.physics_tick_hashes.len());
            let batch = tick_batch(
                start,
                &self.physics_tick_hashes[start..end],
                &self.content_tick_hashes[start..end],
            )?;
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
    Shields,
    Terrains,
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
            "life.maximum",
        ],
        Track::Shields => &["team_id", "source_kind", "energy.maximum", "round_policy"],
        Track::Terrains => &["team_id", "terrain_type", "remaining_rounds"],
    }
}

fn delta_paths(track: Track) -> &'static [&'static str] {
    match track {
        Track::Ticks
        | Track::Units
        | Track::Projectiles
        | Track::Buildings
        | Track::Shields
        | Track::Terrains => &["tick"],
    }
}

fn tick_batch(
    start: usize,
    physics_hashes: &[[u8; canonical::HASH_BYTES]],
    content_hashes: &[[u8; canonical::HASH_BYTES]],
) -> Result<RecordBatch> {
    debug_assert_eq!(physics_hashes.len(), content_hashes.len());
    let start_tick = u32::try_from(start)
        .map_err(|_| Error::invalid("tick row offset exceeds u32"))?
        .checked_add(1)
        .ok_or_else(|| Error::invalid("tick row offset exceeds u32"))?;
    let tick_count = u32::try_from(physics_hashes.len())
        .map_err(|_| Error::invalid("tick batch length exceeds u32"))?;
    let end_tick = start_tick
        .checked_add(tick_count)
        .ok_or_else(|| Error::invalid("tick batch range exceeds u32"))?;
    let ticks = UInt32Array::from_iter_values(start_tick..end_tick);
    let physics_hashes = FixedSizeBinaryArray::try_from_iter(
        physics_hashes
            .iter()
            .map(<[u8; canonical::HASH_BYTES]>::as_slice),
    )?;
    let content_hashes = FixedSizeBinaryArray::try_from_iter(
        content_hashes
            .iter()
            .map(<[u8; canonical::HASH_BYTES]>::as_slice),
    )?;
    Ok(RecordBatch::try_new(
        Arc::new(Schema::new(tick_fields())),
        vec![
            Arc::new(ticks),
            Arc::new(physics_hashes),
            Arc::new(content_hashes),
        ],
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
            vec3_values(units.iter().map(|row| row.velocity)),
            u8_values(units.iter().map(|row| encode_motion(row.motion_state))),
            object_ref_values(units.iter().map(|row| row.mech_lock_target)),
            i64_values(units.iter().map(|row| row.collision_radius)),
            gauge_values(units.iter().map(|row| row.life)),
            bool_values(units.iter().map(|row| row.active)),
            bool_values(units.iter().map(|row| row.targetable)),
            u8_values(units.iter().map(|row| encode_visibility(row.visibility))),
            u64_values(units.iter().map(|row| row.status_mask)),
            modifier_set_values(units.iter().map(|row| row.buff_modifiers)),
            unit_modifier_values(units.iter().map(|row| row.unit_dynamic_modifiers)),
            skill_modifier_list_values(units.iter().map(|row| &row.skill_dynamic_modifiers))?,
            shield_values(units.iter().map(|row| row.personal_shield)),
            weapon_aim_list_values(units.iter().map(|row| &row.weapon_aims))?,
            derived_values(units.iter().map(|row| row.derived)),
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
            i64_values(values.iter().map(|row| row.orientation)),
            object_ref_values(values.iter().map(|row| row.target)),
            vec3_values(values.iter().map(|row| row.cached_target_position)),
            i64_values(values.iter().map(|row| row.cached_target_radius)),
            bool_values(values.iter().map(|row| row.released)),
            gauge_values(values.iter().map(|row| row.life)),
            object_ref_list_values(values.iter().map(|row| &row.spawn_containing_shields))?,
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

fn optional_i32_values(values: impl IntoIterator<Item = Option<i32>>) -> ArrayRef {
    Arc::new(Int32Array::from_iter(values))
}

fn sparse_i64_values(values: impl IntoIterator<Item = i64>) -> ArrayRef {
    optional_i64_values(
        values
            .into_iter()
            .map(|value| (value != 0).then_some(value)),
    )
}

fn sparse_i32_values(values: impl IntoIterator<Item = i32>) -> ArrayRef {
    optional_i32_values(
        values
            .into_iter()
            .map(|value| (value != 0).then_some(value)),
    )
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

fn derived_values(values: impl IntoIterator<Item = DerivedStats>) -> ArrayRef {
    let values = values.into_iter().collect::<Vec<_>>();
    Arc::new(StructArray::new(
        derived_fields(),
        vec![
            i64_values(values.iter().map(|value| value.move_speed)),
            i64_values(values.iter().map(|value| value.attack_range)),
            i32_values(values.iter().map(|value| value.attack_damage)),
            i32_values(values.iter().map(|value| value.current_attack_interval)),
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

fn rate_modifier_values(values: impl IntoIterator<Item = RateModifier>) -> ArrayRef {
    let values = values.into_iter().collect::<Vec<_>>();
    Arc::new(StructArray::new(
        rate_modifier_fields(),
        vec![
            optional_i64_values(
                values
                    .iter()
                    .map(|value| (value.add != 0).then_some(value.add)),
            ),
            optional_i64_values(
                values
                    .iter()
                    .map(|value| (value.reduce != 0).then_some(value.reduce)),
            ),
        ],
        Some(
            values
                .iter()
                .map(|value| !value.is_zero())
                .collect::<NullBuffer>(),
        ),
    ))
}

fn value_modifier_values(values: impl IntoIterator<Item = ValueModifier>) -> ArrayRef {
    let values = values.into_iter().collect::<Vec<_>>();
    Arc::new(StructArray::new(
        value_modifier_fields(),
        vec![
            optional_i32_values(
                values
                    .iter()
                    .map(|value| (value.add != 0).then_some(value.add)),
            ),
            optional_i32_values(
                values
                    .iter()
                    .map(|value| (value.reduce != 0).then_some(value.reduce)),
            ),
        ],
        Some(
            values
                .iter()
                .map(|value| !value.is_zero())
                .collect::<NullBuffer>(),
        ),
    ))
}

fn modifier_set_values(values: impl IntoIterator<Item = BuffModifierSet>) -> ArrayRef {
    let values = values.into_iter().collect::<Vec<_>>();
    Arc::new(StructArray::new(
        modifier_set_fields(),
        vec![
            rate_modifier_values(values.iter().map(|value| value.move_speed_rate)),
            value_modifier_values(values.iter().map(|value| value.move_speed_value)),
            rate_modifier_values(values.iter().map(|value| value.damage_rate)),
            rate_modifier_values(values.iter().map(|value| value.attack_interval_rate)),
            rate_modifier_values(values.iter().map(|value| value.extra_attack_interval_rate)),
            rate_modifier_values(values.iter().map(|value| value.amplify_damage_rate)),
            value_modifier_values(values.iter().map(|value| value.attack_range_value)),
            value_modifier_values(values.iter().map(|value| value.extra_attack_range_value)),
            rate_modifier_values(values.iter().map(|value| value.attack_range_rate)),
            rate_modifier_values(values.iter().map(|value| value.extra_attack_range_rate)),
        ],
        Some(
            values
                .iter()
                .map(|value| !value.is_zero())
                .collect::<NullBuffer>(),
        ),
    ))
}

fn unit_modifier_values(values: impl IntoIterator<Item = UnitDynamicModifierSet>) -> ArrayRef {
    let values = values.into_iter().collect::<Vec<_>>();
    Arc::new(StructArray::new(
        unit_modifier_fields(),
        vec![
            optional_i64_values(
                values
                    .iter()
                    .map(|value| (value.gf_range_value != 0).then_some(value.gf_range_value)),
            ),
            optional_i64_values(
                values.iter().map(|value| {
                    (value.gf_life_time_value != 0).then_some(value.gf_life_time_value)
                }),
            ),
            optional_i64_values(values.iter().map(|value| {
                (value.mech_group_distance != 0).then_some(value.mech_group_distance)
            })),
            rate_modifier_values(values.iter().map(|value| value.life_rate)),
            rate_modifier_values(values.iter().map(|value| value.life_rate_by_kill_count)),
            rate_modifier_values(values.iter().map(|value| value.reduce_damage_from_remote)),
            rate_modifier_values(
                values
                    .iter()
                    .map(|value| value.move_ability_exit_time_change_rate),
            ),
            rate_modifier_values(values.iter().map(|value| value.move_speed_change_rate)),
            rate_modifier_values(values.iter().map(|value| value.amplify_damage_rate)),
            optional_i32_values(
                values
                    .iter()
                    .map(|value| (value.move_speed_value != 0).then_some(value.move_speed_value)),
            ),
            optional_i32_values(values.iter().map(|value| {
                (value.reduce_damage_value != 0).then_some(value.reduce_damage_value)
            })),
            optional_i32_values(values.iter().map(|value| {
                (value.child_inherit_technology_effect != 0)
                    .then_some(value.child_inherit_technology_effect)
            })),
        ],
        Some(
            values
                .iter()
                .map(|value| !value.is_zero())
                .collect::<NullBuffer>(),
        ),
    ))
}

fn skill_modifier_values(values: impl IntoIterator<Item = SkillDynamicModifierSet>) -> ArrayRef {
    let values = values.into_iter().collect::<Vec<_>>();
    Arc::new(StructArray::new(
        skill_dynamic_modifier_fields(),
        vec![
            sparse_i64_values(values.iter().map(|value| value.min_attack_range_value)),
            sparse_i64_values(values.iter().map(|value| value.attack_range_value)),
            sparse_i64_values(values.iter().map(|value| value.attack_air_range_add_value)),
            sparse_i64_values(
                values
                    .iter()
                    .map(|value| value.attack_ground_range_add_value),
            ),
            sparse_i64_values(values.iter().map(|value| value.attack_interval_value)),
            sparse_i64_values(values.iter().map(|value| value.damage_change_rate_ground)),
            sparse_i64_values(values.iter().map(|value| value.damage_change_rate_air)),
            sparse_i64_values(values.iter().map(|value| value.splash_range_value)),
            sparse_i64_values(values.iter().map(|value| value.cb_life_recovery_rate)),
            sparse_i64_values(values.iter().map(|value| value.projectile_speed_value)),
            sparse_i64_values(values.iter().map(|value| value.attack_point_change_value)),
            sparse_i64_values(values.iter().map(|value| value.projectile_duration_value)),
            sparse_i64_values(values.iter().map(|value| value.projectile_random_range)),
            sparse_i64_values(
                values
                    .iter()
                    .map(|value| value.additional_damage_by_target_life),
            ),
            rate_modifier_values(values.iter().map(|value| value.damage_rate)),
            rate_modifier_values(values.iter().map(|value| value.damage_rate_by_kill_count)),
            rate_modifier_values(values.iter().map(|value| value.attack_range_rate)),
            rate_modifier_values(values.iter().map(|value| value.attack_interval_rate)),
            rate_modifier_values(values.iter().map(|value| value.damage_reduce_rate_base)),
            rate_modifier_values(values.iter().map(|value| value.projectile_life_rate)),
            sparse_i32_values(values.iter().map(|value| value.projectile_count_value)),
            sparse_i32_values(values.iter().map(|value| value.air_attack_value)),
            sparse_i32_values(values.iter().map(|value| value.ground_attack_value)),
            sparse_i32_values(values.iter().map(|value| value.attack_range_value_air)),
            sparse_i32_values(values.iter().map(|value| value.attack_range_value_ground)),
            sparse_i32_values(values.iter().map(|value| value.is_lock_target)),
        ],
        None,
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

fn skill_modifier_list_values<'a>(
    values: impl IntoIterator<Item = &'a Vec<SkillNumericModifierState>>,
) -> Result<ArrayRef> {
    let values = values.into_iter().collect::<Vec<_>>();
    let flat = values
        .iter()
        .flat_map(|value| value.iter())
        .collect::<Vec<_>>();
    let items = StructArray::new(
        skill_modifier_fields(),
        vec![
            u16_values(flat.iter().map(|value| value.skill_slot)),
            skill_modifier_values(flat.iter().map(|value| value.modifiers)),
        ],
        None,
    );
    Ok(Arc::new(ListArray::new(
        Arc::new(Field::new(
            "item",
            DataType::Struct(skill_modifier_fields()),
            false,
        )),
        list_offsets(values.iter().map(|value| value.len()))?,
        Arc::new(items),
        None,
    )))
}

fn weapon_aim_list_values<'a>(
    values: impl IntoIterator<Item = &'a Vec<WeaponAimState>>,
) -> Result<ArrayRef> {
    let values = values.into_iter().collect::<Vec<_>>();
    let flat = values
        .iter()
        .flat_map(|value| value.iter())
        .collect::<Vec<_>>();
    let items = StructArray::new(
        weapon_aim_fields(),
        vec![
            u16_values(flat.iter().map(|value| value.skill_slot)),
            i32_values(flat.iter().map(|value| value.weapon_index)),
            object_ref_values(flat.iter().map(|value| value.attack_target)),
            optional_vec3_values(
                flat.iter()
                    .map(|value| value.pose.map(|pose| pose.position)),
            ),
            optional_i64_values(
                flat.iter()
                    .map(|value| value.pose.map(|pose| pose.rotation)),
            ),
        ],
        None,
    );
    Ok(Arc::new(ListArray::new(
        Arc::new(Field::new(
            "item",
            DataType::Struct(weapon_aim_fields()),
            false,
        )),
        list_offsets(values.iter().map(|value| value.len()))?,
        Arc::new(items),
        None,
    )))
}

fn tick_fields() -> Vec<Field> {
    vec![
        Field::new("tick", DataType::UInt32, false),
        Field::new("physics_tick_hash", DataType::FixedSizeBinary(32), false),
        Field::new("content_tick_hash", DataType::FixedSizeBinary(32), false),
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
        struct_field("velocity", vec3_fields(), false),
        Field::new("motion_state", DataType::UInt8, false),
        struct_field("mech_lock_target", object_ref_fields(), true),
        Field::new("collision_radius", DataType::Int64, false),
        struct_field("life", gauge_fields(), false),
        Field::new("active", DataType::Boolean, false),
        Field::new("targetable", DataType::Boolean, false),
        Field::new("visibility", DataType::UInt8, false),
        Field::new("status_mask", DataType::UInt64, false),
        struct_field("buff_modifiers", modifier_set_fields(), true),
        struct_field("unit_dynamic_modifiers", unit_modifier_fields(), true),
        list_field("skill_dynamic_modifiers", skill_modifier_fields()),
        struct_field("personal_shield", shield_fields(), false),
        list_field("weapon_aims", weapon_aim_fields()),
        struct_field("derived", derived_fields(), false),
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
        Field::new("orientation", DataType::Int64, false),
        struct_field("target", object_ref_fields(), true),
        struct_field("cached_target_position", vec3_fields(), false),
        Field::new("cached_target_radius", DataType::Int64, false),
        Field::new("released", DataType::Boolean, false),
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

fn derived_fields() -> Fields {
    vec![
        Field::new("move_speed", DataType::Int64, false),
        Field::new("attack_range", DataType::Int64, false),
        Field::new("attack_damage", DataType::Int32, false),
        Field::new("current_attack_interval", DataType::Int32, false),
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

fn rate_modifier_fields() -> Fields {
    vec![
        Field::new("add", DataType::Int64, true),
        Field::new("reduce", DataType::Int64, true),
    ]
    .into()
}

fn value_modifier_fields() -> Fields {
    vec![
        Field::new("add", DataType::Int32, true),
        Field::new("reduce", DataType::Int32, true),
    ]
    .into()
}

fn modifier_set_fields() -> Fields {
    vec![
        struct_field("move_speed_rate", rate_modifier_fields(), true),
        struct_field("move_speed_value", value_modifier_fields(), true),
        struct_field("damage_rate", rate_modifier_fields(), true),
        struct_field("attack_interval_rate", rate_modifier_fields(), true),
        struct_field("extra_attack_interval_rate", rate_modifier_fields(), true),
        struct_field("amplify_damage_rate", rate_modifier_fields(), true),
        struct_field("attack_range_value", value_modifier_fields(), true),
        struct_field("extra_attack_range_value", value_modifier_fields(), true),
        struct_field("attack_range_rate", rate_modifier_fields(), true),
        struct_field("extra_attack_range_rate", rate_modifier_fields(), true),
    ]
    .into()
}

fn unit_modifier_fields() -> Fields {
    vec![
        Field::new("gf_range_value", DataType::Int64, true),
        Field::new("gf_life_time_value", DataType::Int64, true),
        Field::new("mech_group_distance", DataType::Int64, true),
        struct_field("life_rate", rate_modifier_fields(), true),
        struct_field("life_rate_by_kill_count", rate_modifier_fields(), true),
        struct_field("reduce_damage_from_remote", rate_modifier_fields(), true),
        struct_field(
            "move_ability_exit_time_change_rate",
            rate_modifier_fields(),
            true,
        ),
        struct_field("move_speed_change_rate", rate_modifier_fields(), true),
        struct_field("amplify_damage_rate", rate_modifier_fields(), true),
        Field::new("move_speed_value", DataType::Int32, true),
        Field::new("reduce_damage_value", DataType::Int32, true),
        Field::new("child_inherit_technology_effect", DataType::Int32, true),
    ]
    .into()
}

fn skill_dynamic_modifier_fields() -> Fields {
    vec![
        Field::new("min_attack_range_value", DataType::Int64, true),
        Field::new("attack_range_value", DataType::Int64, true),
        Field::new("attack_air_range_add_value", DataType::Int64, true),
        Field::new("attack_ground_range_add_value", DataType::Int64, true),
        Field::new("attack_interval_value", DataType::Int64, true),
        Field::new("damage_change_rate_ground", DataType::Int64, true),
        Field::new("damage_change_rate_air", DataType::Int64, true),
        Field::new("splash_range_value", DataType::Int64, true),
        Field::new("cb_life_recovery_rate", DataType::Int64, true),
        Field::new("projectile_speed_value", DataType::Int64, true),
        Field::new("attack_point_change_value", DataType::Int64, true),
        Field::new("projectile_duration_value", DataType::Int64, true),
        Field::new("projectile_random_range", DataType::Int64, true),
        Field::new("additional_damage_by_target_life", DataType::Int64, true),
        struct_field("damage_rate", rate_modifier_fields(), true),
        struct_field("damage_rate_by_kill_count", rate_modifier_fields(), true),
        struct_field("attack_range_rate", rate_modifier_fields(), true),
        struct_field("attack_interval_rate", rate_modifier_fields(), true),
        struct_field("damage_reduce_rate_base", rate_modifier_fields(), true),
        struct_field("projectile_life_rate", rate_modifier_fields(), true),
        Field::new("projectile_count_value", DataType::Int32, true),
        Field::new("air_attack_value", DataType::Int32, true),
        Field::new("ground_attack_value", DataType::Int32, true),
        Field::new("attack_range_value_air", DataType::Int32, true),
        Field::new("attack_range_value_ground", DataType::Int32, true),
        Field::new("is_lock_target", DataType::Int32, true),
    ]
    .into()
}

fn skill_modifier_fields() -> Fields {
    vec![
        Field::new("skill_slot", DataType::UInt16, false),
        struct_field("modifiers", skill_dynamic_modifier_fields(), false),
    ]
    .into()
}

fn weapon_aim_fields() -> Fields {
    vec![
        Field::new("skill_slot", DataType::UInt16, false),
        Field::new("weapon_index", DataType::Int32, false),
        struct_field("attack_target", object_ref_fields(), true),
        struct_field("position", vec3_fields(), true),
        Field::new("rotation", DataType::Int64, true),
    ]
    .into()
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

fn write_events_jsonl(path: &Path, rows: &[(u32, u32, Event)]) -> Result<()> {
    let mut file = File::create(path)?;
    for (tick, ordinal, event) in rows {
        if *tick == 0 {
            return Err(Error::invalid("events.jsonl cannot contain E(0)"));
        }
        file.write_all(event_json_line(*tick, *ordinal, event)?.as_bytes())?;
        file.write_all(b"\n")?;
    }
    file.sync_all()?;
    Ok(())
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

fn read_events_jsonl(member: &MemberSlice) -> Result<Vec<(u32, u32, Event)>> {
    let mut bytes = Vec::new();
    member.get_read(0)?.read_to_end(&mut bytes)?;
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        return Err(Error::invalid("events.jsonl must not contain a UTF-8 BOM"));
    }
    if bytes.contains(&b'\r') {
        return Err(Error::invalid("events.jsonl must use LF line endings"));
    }
    if !bytes.is_empty() && !bytes.ends_with(b"\n") {
        return Err(Error::invalid("events.jsonl final line must end with LF"));
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| Error::invalid("events.jsonl is not valid UTF-8"))?;
    text.split_terminator('\n')
        .enumerate()
        .map(|(line_index, line)| {
            if line.is_empty() || line.trim() != line {
                return Err(Error::invalid(format!(
                    "events.jsonl line {} is empty or has surrounding whitespace",
                    line_index + 1
                )));
            }
            parse_event_json_line(line)
        })
        .collect()
}

#[allow(
    clippy::match_same_arms,
    reason = "the payload arms stay in variant order so each event's JSON shape reads in one place"
)]
#[allow(clippy::too_many_lines)]
fn event_json_line(tick: u32, ordinal: u32, event: &Event) -> Result<String> {
    validate_event_refs(event)?;
    let mut line = format!(
        "{{\"tick\":{tick},\"ordinal\":{ordinal},\"type\":\"{}\",\"object\":{}",
        event_type_name(event.payload.kind()),
        object_ref_json(event.subject),
    );
    if !matches!(event.payload, EventPayload::TerrainCreated { .. }) {
        write!(line, ",\"source\":{}", object_ref_json(event.source))
            .expect("writing to String cannot fail");
    }
    write!(
        line,
        ",\"source_team_id\":{},\"target\":{}",
        event
            .source_team_id
            .map_or_else(|| "null".to_owned(), |value| value.to_string()),
        object_ref_json(event.target),
    )
    .expect("writing to String cannot fail");
    match &event.payload {
        EventPayload::ProjectileReleased {
            skill_slot,
            weapon_index,
        } => write!(
            line,
            ",\"skill_slot\":{},\"weapon_index\":{}",
            option_u16_json(*skill_slot),
            option_i32_json(*weapon_index)
        )
        .expect("writing to String cannot fail"),
        EventPayload::ProjectileRemoved {
            position,
            intercepted,
            absorbed_by,
        } => write!(
            line,
            ",\"position_q32_32\":{},\"intercepted\":{},\"absorbed_by\":{}",
            qvec3_json(*position),
            intercepted,
            object_ref_json(*absorbed_by),
        )
        .expect("writing to String cannot fail"),
        EventPayload::Damage { amount } => {
            write!(line, ",\"amount\":{amount}").expect("writing to String cannot fail");
        }
        EventPayload::UnitCreated {
            team_id,
            formation_id,
            unit_type_id,
            position,
        } => write!(
            line,
            ",\"team_id\":{team_id},\"formation_id\":\"{formation_id}\",\"unit_type_id\":{unit_type_id},\"position_q32_32\":{}",
            qvec3_json(*position)
        )
        .expect("writing to String cannot fail"),
        EventPayload::UnitDied { position } | EventPayload::BuildingDestroyed { position } => {
            write!(line, ",\"position_q32_32\":{}", qvec3_json(*position))
                .expect("writing to String cannot fail");
        }
        EventPayload::UnitTeamChanged {
            previous_team_id,
            new_team_id,
        } => write!(
            line,
            ",\"previous_team_id\":{previous_team_id},\"new_team_id\":{new_team_id}"
        )
        .expect("writing to String cannot fail"),
        EventPayload::ShieldCreated {
            team_id,
            source_kind,
            position,
        } => write!(
            line,
            ",\"team_id\":{team_id},\"source_kind\":\"{}\",\"position_q32_32\":{}",
            shield_source_name(*source_kind),
            qvec3_json(*position)
        )
        .expect("writing to String cannot fail"),
        EventPayload::ShieldDestroyed { position, reason } => {
            write!(
                line,
                ",\"position_q32_32\":{},\"reason\":\"{}\"",
                qvec3_json(*position),
                shield_destroyed_reason_name(*reason)
            )
            .expect("writing to String cannot fail");
        }
        EventPayload::TerrainCreated {
            team_id,
            terrain_type,
            position,
            radius,
        } => write!(
            line,
            ",\"team_id\":{},\"terrain_type\":\"{}\",\"position_q32_32\":{},\"radius_q32_32\":\"{}\"",
            team_id.map_or_else(|| "null".to_owned(), |value| value.to_string()),
            terrain_type_name(*terrain_type),
            qvec3_json(*position),
            radius,
        )
        .expect("writing to String cannot fail"),
        EventPayload::TerrainRemoved { position, reason } => write!(
            line,
            ",\"position_q32_32\":{},\"reason\":\"{}\"",
            qvec3_json(*position),
            terrain_removed_reason_name(*reason),
        )
        .expect("writing to String cannot fail"),
        EventPayload::TerrainConverted { position } => write!(
            line,
            ",\"position_q32_32\":{}",
            qvec3_json(*position),
        )
        .expect("writing to String cannot fail"),
        EventPayload::Healing { amount } => {
            write!(line, ",\"amount\":{amount}").expect("writing to String cannot fail");
        }
    }
    line.push('}');
    Ok(line)
}

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
    );
    if required_target && event.target.is_none() {
        return Err(Error::invalid(format!(
            "{} event requires target",
            event_type_name(event.payload.kind())
        )));
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
    }
}

fn object_ref_json(value: Option<ObjectRef>) -> String {
    value.map_or_else(
        || "null".to_owned(),
        |value| {
            format!(
                "{{\"kind\":\"{}\",\"id\":\"{}\"}}",
                object_kind_name(value.kind),
                value.id
            )
        },
    )
}

fn object_kind_name(kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Unit => "unit",
        ObjectKind::Projectile => "projectile",
        ObjectKind::Building => "building",
        ObjectKind::Shield => "shield",
        ObjectKind::Terrain => "terrain",
    }
}

fn qvec3_json(value: QVec3) -> String {
    format!(
        "{{\"x\":\"{}\",\"y\":\"{}\",\"z\":\"{}\"}}",
        value.x, value.y, value.z
    )
}

fn option_i32_json(value: Option<i32>) -> String {
    value.map_or_else(|| "null".to_owned(), |value| value.to_string())
}

fn option_u16_json(value: Option<u16>) -> String {
    value.map_or_else(|| "null".to_owned(), |value| value.to_string())
}

#[allow(clippy::too_many_lines)]
fn parse_event_json_line(line: &str) -> Result<(u32, u32, Event)> {
    let value: Value = serde_json::from_str(line)
        .map_err(|error| Error::invalid(format!("invalid events.jsonl line: {error}")))?;
    let object = value
        .as_object()
        .ok_or_else(|| Error::invalid("events.jsonl line must be a JSON object"))?;
    let event_type = json_string(object, "type")?;
    let expected = event_field_names(event_type)?;
    let actual = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(Error::invalid(format!(
            "{event_type} event field set is not canonical"
        )));
    }
    let tick = json_u32(object, "tick")?;
    if tick == 0 {
        return Err(Error::invalid("events.jsonl cannot contain E(0)"));
    }
    let ordinal = json_u32(object, "ordinal")?;
    let subject = parse_object_ref(object.get("object").expect("checked field set"))?;
    let source = if event_type == "terrain_created" {
        None
    } else {
        parse_object_ref(object.get("source").expect("checked field set"))?
    };
    let source_team_id = parse_optional_u32(
        object.get("source_team_id").expect("checked field set"),
        "source_team_id",
    )?;
    let target = parse_object_ref(object.get("target").expect("checked field set"))?;
    let payload = match event_type {
        "projectile_released" => EventPayload::ProjectileReleased {
            skill_slot: parse_optional_u16(
                object.get("skill_slot").expect("checked field set"),
                "skill_slot",
            )?,
            weapon_index: parse_optional_i32(
                object.get("weapon_index").expect("checked field set"),
                "weapon_index",
            )?,
        },
        "projectile_removed" => EventPayload::ProjectileRemoved {
            position: parse_qvec3(object, "position_q32_32")?,
            intercepted: json_bool(object, "intercepted")?,
            absorbed_by: parse_object_ref(object.get("absorbed_by").expect("checked field set"))?,
        },
        "damage" => EventPayload::Damage {
            amount: json_i32(object, "amount")?,
        },
        "unit_created" => EventPayload::UnitCreated {
            team_id: json_u32(object, "team_id")?,
            formation_id: parse_canonical_u64(json_string(object, "formation_id")?)?,
            unit_type_id: json_u32(object, "unit_type_id")?,
            position: parse_qvec3(object, "position_q32_32")?,
        },
        "unit_died" => EventPayload::UnitDied {
            position: parse_qvec3(object, "position_q32_32")?,
        },
        "building_destroyed" => EventPayload::BuildingDestroyed {
            position: parse_qvec3(object, "position_q32_32")?,
        },
        "unit_team_changed" => EventPayload::UnitTeamChanged {
            previous_team_id: json_u32(object, "previous_team_id")?,
            new_team_id: json_u32(object, "new_team_id")?,
        },
        "shield_created" => EventPayload::ShieldCreated {
            team_id: json_u32(object, "team_id")?,
            source_kind: parse_shield_source(json_string(object, "source_kind")?)?,
            position: parse_qvec3(object, "position_q32_32")?,
        },
        "shield_destroyed" => EventPayload::ShieldDestroyed {
            position: parse_qvec3(object, "position_q32_32")?,
            reason: parse_shield_destroyed_reason(json_string(object, "reason")?)?,
        },
        "terrain_created" => EventPayload::TerrainCreated {
            team_id: parse_optional_u32(
                object.get("team_id").expect("checked field set"),
                "team_id",
            )?,
            terrain_type: parse_terrain_type(json_string(object, "terrain_type")?)?,
            position: parse_qvec3(object, "position_q32_32")?,
            radius: parse_canonical_i64(json_string(object, "radius_q32_32")?)?,
        },
        "terrain_removed" => EventPayload::TerrainRemoved {
            position: parse_qvec3(object, "position_q32_32")?,
            reason: parse_terrain_removed_reason(json_string(object, "reason")?)?,
        },
        "terrain_converted" => EventPayload::TerrainConverted {
            position: parse_qvec3(object, "position_q32_32")?,
        },
        "healing" => EventPayload::Healing {
            amount: json_i32(object, "amount")?,
        },
        _ => unreachable!("validated event type"),
    };
    let event = Event {
        subject,
        source,
        source_team_id,
        target,
        payload,
    };
    validate_event_refs(&event)?;
    Ok((tick, ordinal, event))
}

#[allow(
    clippy::match_same_arms,
    reason = "the event-type table stays in schema order so each event's columns read in one place"
)]
fn event_field_names(event_type: &str) -> Result<BTreeSet<&'static str>> {
    let mut fields = [
        "tick",
        "ordinal",
        "type",
        "object",
        "source_team_id",
        "target",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    let extra: &[&str] = match event_type {
        "projectile_released" => &["skill_slot", "weapon_index"],
        "projectile_removed" => &["position_q32_32", "intercepted", "absorbed_by"],
        "damage" => &["amount"],
        "unit_created" => &["team_id", "formation_id", "unit_type_id", "position_q32_32"],
        "unit_died" | "building_destroyed" => &["position_q32_32"],
        "unit_team_changed" => &["previous_team_id", "new_team_id"],
        "shield_created" => &["team_id", "source_kind", "position_q32_32"],
        "shield_destroyed" => &["position_q32_32", "reason"],
        "terrain_created" => &[
            "team_id",
            "terrain_type",
            "position_q32_32",
            "radius_q32_32",
        ],
        "terrain_removed" => &["position_q32_32", "reason"],
        "terrain_converted" => &["position_q32_32"],
        "healing" => &["amount"],
        _ => return Err(Error::invalid(format!("unknown event type {event_type:?}"))),
    };
    if event_type != "terrain_created" {
        fields.insert("source");
    }
    fields.extend(extra.iter().copied());
    Ok(fields)
}

fn json_string<'a>(object: &'a Map<String, Value>, name: &str) -> Result<&'a str> {
    object
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid(format!("event field {name} is not a string")))
}

fn json_u32(object: &Map<String, Value>, name: &str) -> Result<u32> {
    let value = object
        .get(name)
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::invalid(format!("event field {name} is not a u32")))?;
    u32::try_from(value).map_err(|_| Error::invalid(format!("event field {name} is not a u32")))
}

fn json_i32(object: &Map<String, Value>, name: &str) -> Result<i32> {
    let value = object
        .get(name)
        .and_then(Value::as_i64)
        .ok_or_else(|| Error::invalid(format!("event field {name} is not an i32")))?;
    i32::try_from(value).map_err(|_| Error::invalid(format!("event field {name} is not an i32")))
}

fn json_bool(object: &Map<String, Value>, name: &str) -> Result<bool> {
    object
        .get(name)
        .and_then(Value::as_bool)
        .ok_or_else(|| Error::invalid(format!("event field {name} is not a bool")))
}

fn parse_object_ref(value: &Value) -> Result<Option<ObjectRef>> {
    if value.is_null() {
        return Ok(None);
    }
    let object = value
        .as_object()
        .ok_or_else(|| Error::invalid("event ObjectRef is not an object or null"))?;
    let keys = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
    if keys != ["kind", "id"].into_iter().collect() {
        return Err(Error::invalid("event ObjectRef field set is not canonical"));
    }
    let kind = match json_string(object, "kind")? {
        "unit" => ObjectKind::Unit,
        "projectile" => ObjectKind::Projectile,
        "building" => ObjectKind::Building,
        "shield" => ObjectKind::Shield,
        "terrain" => ObjectKind::Terrain,
        value => return Err(Error::invalid(format!("invalid ObjectRef kind {value:?}"))),
    };
    let id = parse_canonical_u64(json_string(object, "id")?)?;
    if id == 0 {
        return Err(Error::invalid("event ObjectRef id must be positive"));
    }
    Ok(Some(ObjectRef::new(kind, id)))
}

const fn shield_source_name(value: ShieldSourceKind) -> &'static str {
    match value {
        ShieldSourceKind::Contraption => "contraption",
        ShieldSourceKind::CommanderSkill => "commander_skill",
        ShieldSourceKind::OwnerAdvanced => "owner_advanced",
        ShieldSourceKind::SpawnedTemporary => "spawned_temporary",
    }
}

fn parse_shield_source(value: &str) -> Result<ShieldSourceKind> {
    match value {
        "contraption" => Ok(ShieldSourceKind::Contraption),
        "commander_skill" => Ok(ShieldSourceKind::CommanderSkill),
        "owner_advanced" => Ok(ShieldSourceKind::OwnerAdvanced),
        "spawned_temporary" => Ok(ShieldSourceKind::SpawnedTemporary),
        _ => Err(Error::invalid(format!(
            "invalid shield source kind {value:?}"
        ))),
    }
}

const fn shield_destroyed_reason_name(value: ShieldDestroyedReason) -> &'static str {
    match value {
        ShieldDestroyedReason::EnergyDepleted => "energy_depleted",
        ShieldDestroyedReason::OwnerDestroyed => "owner_destroyed",
        ShieldDestroyedReason::RoundEnd => "round_end",
        ShieldDestroyedReason::Scripted => "scripted",
        ShieldDestroyedReason::Unknown => "unknown",
    }
}

fn parse_shield_destroyed_reason(value: &str) -> Result<ShieldDestroyedReason> {
    match value {
        "energy_depleted" => Ok(ShieldDestroyedReason::EnergyDepleted),
        "owner_destroyed" => Ok(ShieldDestroyedReason::OwnerDestroyed),
        "round_end" => Ok(ShieldDestroyedReason::RoundEnd),
        "scripted" => Ok(ShieldDestroyedReason::Scripted),
        "unknown" => Ok(ShieldDestroyedReason::Unknown),
        _ => Err(Error::invalid(format!(
            "invalid shield destroyed reason {value:?}"
        ))),
    }
}

const fn terrain_type_name(value: TerrainType) -> &'static str {
    match value {
        TerrainType::Fire => "fire",
        TerrainType::Oil => "oil",
        TerrainType::Fog => "fog",
        TerrainType::Acid => "acid",
        TerrainType::RecoveryZone => "recovery_zone",
        TerrainType::FogSand => "fog_sand",
    }
}

fn parse_terrain_type(value: &str) -> Result<TerrainType> {
    match value {
        "fire" => Ok(TerrainType::Fire),
        "oil" => Ok(TerrainType::Oil),
        "fog" => Ok(TerrainType::Fog),
        "acid" => Ok(TerrainType::Acid),
        "recovery_zone" => Ok(TerrainType::RecoveryZone),
        "fog_sand" => Ok(TerrainType::FogSand),
        _ => Err(Error::invalid(format!("invalid terrain type {value:?}"))),
    }
}

const fn terrain_removed_reason_name(value: TerrainRemovedReason) -> &'static str {
    match value {
        TerrainRemovedReason::TimeExpired => "time_expired",
        TerrainRemovedReason::RoundExpired => "round_expired",
        TerrainRemovedReason::GridDepleted => "grid_depleted",
        TerrainRemovedReason::Cleared => "cleared",
        TerrainRemovedReason::Unknown => "unknown",
    }
}

fn parse_terrain_removed_reason(value: &str) -> Result<TerrainRemovedReason> {
    match value {
        "time_expired" => Ok(TerrainRemovedReason::TimeExpired),
        "round_expired" => Ok(TerrainRemovedReason::RoundExpired),
        "grid_depleted" => Ok(TerrainRemovedReason::GridDepleted),
        "cleared" => Ok(TerrainRemovedReason::Cleared),
        "unknown" => Ok(TerrainRemovedReason::Unknown),
        _ => Err(Error::invalid(format!(
            "invalid terrain removed reason {value:?}"
        ))),
    }
}

fn parse_qvec3(object: &Map<String, Value>, name: &str) -> Result<QVec3> {
    let value = object
        .get(name)
        .and_then(Value::as_object)
        .ok_or_else(|| Error::invalid(format!("event field {name} is not a QVec3 object")))?;
    let keys = value.keys().map(String::as_str).collect::<BTreeSet<_>>();
    if keys != ["x", "y", "z"].into_iter().collect() {
        return Err(Error::invalid(format!(
            "event field {name} is not canonical"
        )));
    }
    Ok(QVec3 {
        x: parse_canonical_i64(json_string(value, "x")?)?,
        y: parse_canonical_i64(json_string(value, "y")?)?,
        z: parse_canonical_i64(json_string(value, "z")?)?,
    })
}

fn parse_canonical_u64(value: &str) -> Result<u64> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| Error::invalid("event u64 string is invalid"))?;
    if parsed.to_string() != value {
        return Err(Error::invalid("event u64 string is not canonical"));
    }
    Ok(parsed)
}

fn parse_canonical_i64(value: &str) -> Result<i64> {
    let parsed = value
        .parse::<i64>()
        .map_err(|_| Error::invalid("event i64 string is invalid"))?;
    if parsed.to_string() != value {
        return Err(Error::invalid("event i64 string is not canonical"));
    }
    Ok(parsed)
}

fn parse_optional_u32(value: &Value, label: &str) -> Result<Option<u32>> {
    if value.is_null() {
        return Ok(None);
    }
    let parsed = value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| Error::invalid(format!("event field {label} is not a u32 or null")))?;
    Ok(Some(parsed))
}

fn parse_optional_u16(value: &Value, label: &str) -> Result<Option<u16>> {
    if value.is_null() {
        return Ok(None);
    }
    let parsed = value
        .as_u64()
        .and_then(|value| u16::try_from(value).ok())
        .ok_or_else(|| Error::invalid(format!("event field {label} is not a u16 or null")))?;
    Ok(Some(parsed))
}

fn parse_optional_i32(value: &Value, label: &str) -> Result<Option<i32>> {
    if value.is_null() {
        return Ok(None);
    }
    let parsed = value
        .as_i64()
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| Error::invalid(format!("event field {label} is not an i32 or null")))?;
    Ok(Some(parsed))
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
    pub(crate) game_build: String,
    pub(crate) context: DurableContext,
    pub(crate) tick_count: u32,
    pub(crate) terminal_tick: u32,
    pub(crate) hashes: Hashes,
}

struct TickMetadata {
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
    physics_tick_hashes: Vec<[u8; canonical::HASH_BYTES]>,
    content_tick_hashes: Vec<[u8; canonical::HASH_BYTES]>,
    units: Vec<Vec<LiveUnitState>>,
    projectiles: Vec<Vec<ProjectileState>>,
    buildings: Vec<Vec<BuildingState>>,
    shields: Vec<Vec<ShieldState>>,
    terrains: Vec<Vec<TerrainState>>,
    events: Vec<Vec<Event>>,
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
        let (tick_metadata, physics_tick_hashes, content_tick_hashes) = read_ticks(ticks.clone())?;
        let (layout_yaml, match_seed) = read_layout_yaml(
            &member(&members, "layout.yaml")?,
            tick_metadata.context.combat_round,
        )?;
        let tick_count = tick_metadata.tick_count;
        let metadata = StoredMetadata {
            game_build: tick_metadata.game_build,
            context: tick_metadata.context.with_match_seed(match_seed),
            tick_count,
            terminal_tick: tick_metadata.terminal_tick,
            hashes: tick_metadata.hashes,
        };
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
        let shields = group_state_rows(
            read_shields(member(&members, "shields.parquet")?)?,
            tick_count,
            |row| row.shield_id,
            "shield",
        )?;
        let terrains = group_state_rows(
            read_terrains(member(&members, "terrains.parquet")?)?,
            tick_count,
            |row| row.terrain_id,
            "terrain",
        )?;
        let events = group_event_rows(
            read_events_jsonl(&member(&members, "events.jsonl")?)?,
            tick_count,
        )?;
        Ok(Self {
            metadata,
            layout_yaml,
            member_sizes,
            physics_tick_hashes,
            content_tick_hashes,
            units,
            projectiles,
            buildings,
            shields,
            terrains,
            events,
        })
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

    pub(crate) fn physics_tick_hash(&self, tick: u32) -> Result<[u8; canonical::HASH_BYTES]> {
        if tick == 0 || tick > self.metadata.tick_count {
            return Err(Error::invalid(format!("tick {tick} is out of range")));
        }
        self.physics_tick_hashes
            .get(usize::try_from(tick - 1).map_err(|_| Error::invalid("tick is too large"))?)
            .copied()
            .ok_or_else(|| Error::invalid(format!("tick {tick} is out of range")))
    }

    pub(crate) fn content_tick_hash(&self, tick: u32) -> Result<[u8; canonical::HASH_BYTES]> {
        if tick == 0 || tick > self.metadata.tick_count {
            return Err(Error::invalid(format!("tick {tick} is out of range")));
        }
        self.content_tick_hashes
            .get(usize::try_from(tick - 1).map_err(|_| Error::invalid("tick is too large"))?)
            .copied()
            .ok_or_else(|| Error::invalid(format!("tick {tick} is out of range")))
    }

    pub(crate) fn physics_tick_hashes(&self) -> &[[u8; canonical::HASH_BYTES]] {
        &self.physics_tick_hashes
    }

    pub(crate) fn state(&self, tick: u32) -> Result<WorldSnapshot> {
        let index = state_tick_index(tick, self.metadata.tick_count)?;
        Ok(WorldSnapshot {
            live_units: self.units[index].clone(),
            projectiles: self.projectiles[index].clone(),
            buildings: self.buildings[index].clone(),
            shields: self.shields[index].clone(),
            terrains: self.terrains[index].clone(),
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
type TickColumns = (
    TickMetadata,
    Vec<[u8; canonical::HASH_BYTES]>,
    Vec<[u8; canonical::HASH_BYTES]>,
);

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
    let game_build = required_metadata(metadata, "game_build")?.to_owned();
    if game_build.trim().is_empty() {
        return Err(Error::invalid(
            "ticks.parquet metadata game_build must not be empty",
        ));
    }
    if required_metadata(metadata, "physics_hash_profile")? != PHYSICS_HASH_PROFILE {
        return Err(Error::invalid("unsupported physics hash profile"));
    }
    if required_metadata(metadata, "content_hash_profile")? != CONTENT_HASH_PROFILE {
        return Err(Error::invalid("unsupported content hash profile"));
    }
    let hashes = Hashes {
        physics_result_hash: required_metadata(metadata, "physics_result_hash")?.to_owned(),
        content_result_hash: required_metadata(metadata, "content_result_hash")?.to_owned(),
    };
    hashes.validate_encoding()?;
    let tick_count = parse_metadata_u32(metadata, "tick_count")?;
    let terminal_tick = parse_metadata_u32(metadata, "terminal_tick")?;
    if tick_count == 0 || terminal_tick != tick_count {
        return Err(Error::invalid(
            "MCFR must contain T(1) and terminal_tick must equal tick_count",
        ));
    }
    let mut physics_hashes_out = Vec::new();
    let mut content_hashes_out = Vec::new();
    let mut expected_tick = 1_u32;
    for batch in builder.build()? {
        let batch = batch?;
        let ticks = column::<UInt32Array>(&batch, "tick")?;
        let physics_hashes = column::<FixedSizeBinaryArray>(&batch, "physics_tick_hash")?;
        let content_hashes = column::<FixedSizeBinaryArray>(&batch, "content_tick_hash")?;
        for index in 0..batch.num_rows() {
            if ticks.value(index) != expected_tick {
                return Err(Error::invalid(format!(
                    "ticks.parquet expected tick {expected_tick}, found {}",
                    ticks.value(index)
                )));
            }
            let physics_value: [u8; canonical::HASH_BYTES] = physics_hashes
                .value(index)
                .try_into()
                .map_err(|_| Error::invalid("physics_tick_hash is not 32 bytes"))?;
            let content_value: [u8; canonical::HASH_BYTES] = content_hashes
                .value(index)
                .try_into()
                .map_err(|_| Error::invalid("content_tick_hash is not 32 bytes"))?;
            physics_hashes_out.push(physics_value);
            content_hashes_out.push(content_value);
            expected_tick += 1;
        }
    }
    if expected_tick != tick_count.saturating_add(1) {
        return Err(Error::invalid(format!(
            "tick_count metadata is {tick_count}, found {} tick rows",
            expected_tick.saturating_sub(1)
        )));
    }
    Ok((
        TickMetadata {
            game_build,
            context,
            tick_count,
            terminal_tick,
            hashes,
        },
        physics_hashes_out,
        content_hashes_out,
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
        let status_mask = column::<UInt64Array>(&batch, "status_mask")?;
        let buff_modifiers = struct_column(&batch, "buff_modifiers")?;
        let unit_dynamic_modifiers = struct_column(&batch, "unit_dynamic_modifiers")?;
        let skill_dynamic_modifiers = column::<ListArray>(&batch, "skill_dynamic_modifiers")?;
        let shield = struct_column(&batch, "personal_shield")?;
        let weapon_aims = column::<ListArray>(&batch, "weapon_aims")?;
        let derived = struct_column(&batch, "derived")?;
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
                    velocity: read_vec3(velocity, index)?,
                    motion_state: decode_motion(motion.value(index))?,
                    mech_lock_target: read_optional_ref(target, index)?,
                    collision_radius: radius.value(index),
                    life: read_gauge(life, index)?,
                    active: active.value(index),
                    targetable: targetable.value(index),
                    visibility: decode_visibility(visibility.value(index))?,
                    status_mask: status_mask.value(index),
                    buff_modifiers: read_modifier_set(buff_modifiers, index)?,
                    unit_dynamic_modifiers: read_unit_modifier_set(unit_dynamic_modifiers, index)?,
                    skill_dynamic_modifiers: read_skill_modifier_list(
                        skill_dynamic_modifiers,
                        index,
                    )?,
                    personal_shield: read_shield(shield, index)?,
                    derived: read_derived(derived, index)?,
                    weapon_aims: read_weapon_aim_list(weapon_aims, index)?,
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
        let orientation = column::<Int64Array>(&batch, "orientation")?;
        let target = struct_column(&batch, "target")?;
        let cached = struct_column(&batch, "cached_target_position")?;
        let radius = column::<Int64Array>(&batch, "cached_target_radius")?;
        let released = column::<BooleanArray>(&batch, "released")?;
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
                    orientation: orientation.value(index),
                    target: read_optional_ref(target, index)?,
                    cached_target_position: read_vec3(cached, index)?,
                    cached_target_radius: radius.value(index),
                    released: released.value(index),
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

fn read_shield(array: &StructArray, index: usize) -> Result<PersonalShieldState> {
    Ok(PersonalShieldState {
        active: struct_child::<BooleanArray>(array, "active")?.value(index),
        enabled: struct_child::<BooleanArray>(array, "enabled")?.value(index),
        energy: read_gauge(struct_child::<StructArray>(array, "energy")?, index)?,
    })
}

fn read_derived(array: &StructArray, index: usize) -> Result<DerivedStats> {
    Ok(DerivedStats {
        move_speed: struct_child::<Int64Array>(array, "move_speed")?.value(index),
        attack_range: struct_child::<Int64Array>(array, "attack_range")?.value(index),
        attack_damage: struct_child::<Int32Array>(array, "attack_damage")?.value(index),
        current_attack_interval: struct_child::<Int32Array>(array, "current_attack_interval")?
            .value(index),
    })
}

fn read_gauge(array: &StructArray, index: usize) -> Result<GaugeI32> {
    Ok(GaugeI32 {
        current: struct_child::<Int32Array>(array, "current")?.value(index),
        maximum: struct_child::<Int32Array>(array, "maximum")?.value(index),
    })
}

fn read_i64_or_zero(array: &Int64Array, index: usize) -> i64 {
    if array.is_null(index) {
        0
    } else {
        array.value(index)
    }
}

fn read_i32_or_zero(array: &Int32Array, index: usize) -> i32 {
    if array.is_null(index) {
        0
    } else {
        array.value(index)
    }
}

fn read_rate_modifier(array: &StructArray, index: usize) -> Result<RateModifier> {
    if array.is_null(index) {
        return Ok(RateModifier::default());
    }
    Ok(RateModifier {
        add: read_i64_or_zero(struct_child(array, "add")?, index),
        reduce: read_i64_or_zero(struct_child(array, "reduce")?, index),
    })
}

fn read_value_modifier(array: &StructArray, index: usize) -> Result<ValueModifier> {
    if array.is_null(index) {
        return Ok(ValueModifier::default());
    }
    Ok(ValueModifier {
        add: read_i32_or_zero(struct_child(array, "add")?, index),
        reduce: read_i32_or_zero(struct_child(array, "reduce")?, index),
    })
}

fn read_modifier_set(array: &StructArray, index: usize) -> Result<BuffModifierSet> {
    if array.is_null(index) {
        return Ok(BuffModifierSet::default());
    }
    Ok(BuffModifierSet {
        move_speed_rate: read_rate_modifier(struct_child(array, "move_speed_rate")?, index)?,
        move_speed_value: read_value_modifier(struct_child(array, "move_speed_value")?, index)?,
        damage_rate: read_rate_modifier(struct_child(array, "damage_rate")?, index)?,
        attack_interval_rate: read_rate_modifier(
            struct_child(array, "attack_interval_rate")?,
            index,
        )?,
        extra_attack_interval_rate: read_rate_modifier(
            struct_child(array, "extra_attack_interval_rate")?,
            index,
        )?,
        amplify_damage_rate: read_rate_modifier(
            struct_child(array, "amplify_damage_rate")?,
            index,
        )?,
        attack_range_value: read_value_modifier(struct_child(array, "attack_range_value")?, index)?,
        extra_attack_range_value: read_value_modifier(
            struct_child(array, "extra_attack_range_value")?,
            index,
        )?,
        attack_range_rate: read_rate_modifier(struct_child(array, "attack_range_rate")?, index)?,
        extra_attack_range_rate: read_rate_modifier(
            struct_child(array, "extra_attack_range_rate")?,
            index,
        )?,
    })
}

fn read_unit_modifier_set(array: &StructArray, index: usize) -> Result<UnitDynamicModifierSet> {
    if array.is_null(index) {
        return Ok(UnitDynamicModifierSet::default());
    }
    Ok(UnitDynamicModifierSet {
        gf_range_value: read_i64_or_zero(struct_child(array, "gf_range_value")?, index),
        gf_life_time_value: read_i64_or_zero(struct_child(array, "gf_life_time_value")?, index),
        mech_group_distance: read_i64_or_zero(struct_child(array, "mech_group_distance")?, index),
        life_rate: read_rate_modifier(struct_child(array, "life_rate")?, index)?,
        life_rate_by_kill_count: read_rate_modifier(
            struct_child(array, "life_rate_by_kill_count")?,
            index,
        )?,
        reduce_damage_from_remote: read_rate_modifier(
            struct_child(array, "reduce_damage_from_remote")?,
            index,
        )?,
        move_ability_exit_time_change_rate: read_rate_modifier(
            struct_child(array, "move_ability_exit_time_change_rate")?,
            index,
        )?,
        move_speed_change_rate: read_rate_modifier(
            struct_child(array, "move_speed_change_rate")?,
            index,
        )?,
        amplify_damage_rate: read_rate_modifier(
            struct_child(array, "amplify_damage_rate")?,
            index,
        )?,
        move_speed_value: read_i32_or_zero(struct_child(array, "move_speed_value")?, index),
        reduce_damage_value: read_i32_or_zero(struct_child(array, "reduce_damage_value")?, index),
        child_inherit_technology_effect: read_i32_or_zero(
            struct_child(array, "child_inherit_technology_effect")?,
            index,
        ),
    })
}

fn read_skill_modifier_set(array: &StructArray, index: usize) -> Result<SkillDynamicModifierSet> {
    Ok(SkillDynamicModifierSet {
        min_attack_range_value: read_i64_or_zero(
            struct_child(array, "min_attack_range_value")?,
            index,
        ),
        attack_range_value: read_i64_or_zero(struct_child(array, "attack_range_value")?, index),
        attack_air_range_add_value: read_i64_or_zero(
            struct_child(array, "attack_air_range_add_value")?,
            index,
        ),
        attack_ground_range_add_value: read_i64_or_zero(
            struct_child(array, "attack_ground_range_add_value")?,
            index,
        ),
        attack_interval_value: read_i64_or_zero(
            struct_child(array, "attack_interval_value")?,
            index,
        ),
        damage_change_rate_ground: read_i64_or_zero(
            struct_child(array, "damage_change_rate_ground")?,
            index,
        ),
        damage_change_rate_air: read_i64_or_zero(
            struct_child(array, "damage_change_rate_air")?,
            index,
        ),
        splash_range_value: read_i64_or_zero(struct_child(array, "splash_range_value")?, index),
        cb_life_recovery_rate: read_i64_or_zero(
            struct_child(array, "cb_life_recovery_rate")?,
            index,
        ),
        projectile_speed_value: read_i64_or_zero(
            struct_child(array, "projectile_speed_value")?,
            index,
        ),
        attack_point_change_value: read_i64_or_zero(
            struct_child(array, "attack_point_change_value")?,
            index,
        ),
        projectile_duration_value: read_i64_or_zero(
            struct_child(array, "projectile_duration_value")?,
            index,
        ),
        projectile_random_range: read_i64_or_zero(
            struct_child(array, "projectile_random_range")?,
            index,
        ),
        additional_damage_by_target_life: read_i64_or_zero(
            struct_child(array, "additional_damage_by_target_life")?,
            index,
        ),
        damage_rate: read_rate_modifier(struct_child(array, "damage_rate")?, index)?,
        damage_rate_by_kill_count: read_rate_modifier(
            struct_child(array, "damage_rate_by_kill_count")?,
            index,
        )?,
        attack_range_rate: read_rate_modifier(struct_child(array, "attack_range_rate")?, index)?,
        attack_interval_rate: read_rate_modifier(
            struct_child(array, "attack_interval_rate")?,
            index,
        )?,
        damage_reduce_rate_base: read_rate_modifier(
            struct_child(array, "damage_reduce_rate_base")?,
            index,
        )?,
        projectile_life_rate: read_rate_modifier(
            struct_child(array, "projectile_life_rate")?,
            index,
        )?,
        projectile_count_value: read_i32_or_zero(
            struct_child(array, "projectile_count_value")?,
            index,
        ),
        air_attack_value: read_i32_or_zero(struct_child(array, "air_attack_value")?, index),
        ground_attack_value: read_i32_or_zero(struct_child(array, "ground_attack_value")?, index),
        attack_range_value_air: read_i32_or_zero(
            struct_child(array, "attack_range_value_air")?,
            index,
        ),
        attack_range_value_ground: read_i32_or_zero(
            struct_child(array, "attack_range_value_ground")?,
            index,
        ),
        is_lock_target: read_i32_or_zero(struct_child(array, "is_lock_target")?, index),
    })
}

fn read_skill_modifier_list(
    array: &ListArray,
    index: usize,
) -> Result<Vec<SkillNumericModifierState>> {
    let items = list_struct_items(array, index, "skill_dynamic_modifiers")?;
    let slots = struct_child::<UInt16Array>(&items, "skill_slot")?;
    let modifiers = struct_child::<StructArray>(&items, "modifiers")?;
    (0..items.len())
        .map(|item| {
            Ok(SkillNumericModifierState {
                skill_slot: slots.value(item),
                modifiers: read_skill_modifier_set(modifiers, item)?,
            })
        })
        .collect()
}

fn read_weapon_aim_list(array: &ListArray, index: usize) -> Result<Vec<WeaponAimState>> {
    let items = list_struct_items(array, index, "weapon_aims")?;
    let slots = struct_child::<UInt16Array>(&items, "skill_slot")?;
    let weapon_indexes = struct_child::<Int32Array>(&items, "weapon_index")?;
    let targets = struct_child::<StructArray>(&items, "attack_target")?;
    let positions = struct_child::<StructArray>(&items, "position")?;
    let rotations = struct_child::<Int64Array>(&items, "rotation")?;
    (0..items.len())
        .map(|item| {
            let position_is_null = positions.is_null(item);
            let rotation_is_null = rotations.is_null(item);
            if position_is_null != rotation_is_null {
                return Err(Error::invalid(
                    "weapon aim position and rotation nullability differ",
                ));
            }
            Ok(WeaponAimState {
                skill_slot: slots.value(item),
                weapon_index: weapon_indexes.value(item),
                attack_target: read_optional_ref(targets, item)?,
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
        ObjectKind::Shield => 3,
        ObjectKind::Terrain => 4,
    }
}

fn decode_kind(value: u8) -> Result<ObjectKind> {
    match value {
        0 => Ok(ObjectKind::Unit),
        1 => Ok(ObjectKind::Projectile),
        2 => Ok(ObjectKind::Building),
        3 => Ok(ObjectKind::Shield),
        4 => Ok(ObjectKind::Terrain),
        _ => Err(Error::invalid(format!("invalid ObjectKind tag {value}"))),
    }
}

const fn encode_terrain_type(value: TerrainType) -> u8 {
    match value {
        TerrainType::Fire => 0,
        TerrainType::Oil => 1,
        TerrainType::Fog => 2,
        TerrainType::Acid => 3,
        TerrainType::RecoveryZone => 4,
        TerrainType::FogSand => 5,
    }
}

fn decode_terrain_type(value: u8) -> Result<TerrainType> {
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

const fn encode_shield_source(value: ShieldSourceKind) -> u8 {
    match value {
        ShieldSourceKind::Contraption => 0,
        ShieldSourceKind::CommanderSkill => 1,
        ShieldSourceKind::OwnerAdvanced => 2,
        ShieldSourceKind::SpawnedTemporary => 3,
    }
}

fn decode_shield_source(value: u8) -> Result<ShieldSourceKind> {
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
