//! Instrument channels: what a recording observed of the fight's inside.
//!
//! A channel is one Parquet member, `instrument/<channel>.parquet`, beside the
//! recording's own tables. Its rows are a Rust type, whose Arrow schema is traced
//! from the type, and every row carries the tick it was observed on. Neither hash
//! reads a channel: what the fight did is the recording, and a channel is what a
//! study asked to see of how it did it, so asking for one never changes a pin.

use std::sync::Arc;

use arrow_array::{Array, ArrayRef, RecordBatch, UInt32Array};
use arrow_schema::{DataType, Field, FieldRef, Schema, SchemaRef};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_arrow::schema::{SchemaLike, TracingOptions};

use crate::{Error, ObjectRef, Result};

/// The column every channel starts with.
pub const INSTRUMENT_TICK_COLUMN: &str = "tick";

/// A row of one channel.
pub trait InstrumentRow: Serialize + DeserializeOwned {
    /// The channel's name, which is also its member's file stem.
    const CHANNEL: &'static str;
}

/// Whether a name can be a channel: a lowercase identifier, which is what
/// keeps a member name inside `instrument/`.
#[must_use]
pub fn valid_channel_name(name: &str) -> bool {
    name.len() <= 64
        && name.starts_with(|first: char| first.is_ascii_lowercase())
        && name
            .chars()
            .all(|next| next.is_ascii_lowercase() || next.is_ascii_digit() || next == '_')
}

pub(crate) struct ChannelSchema {
    pub(crate) fields: Vec<FieldRef>,
    pub(crate) schema: SchemaRef,
}

impl ChannelSchema {
    pub(crate) fn of<R: InstrumentRow>() -> Result<Self> {
        if !valid_channel_name(R::CHANNEL) {
            return Err(Error::invalid(format!(
                "{:?} is not a channel name",
                R::CHANNEL
            )));
        }
        let fields = Vec::<FieldRef>::from_type::<R>(
            TracingOptions::default().enums_without_data_as_strings(true),
        )?;
        if fields
            .iter()
            .any(|field| field.name() == INSTRUMENT_TICK_COLUMN)
        {
            return Err(Error::invalid(format!(
                "channel {} has its own {INSTRUMENT_TICK_COLUMN} field",
                R::CHANNEL
            )));
        }
        let mut columns = vec![Arc::new(Field::new(
            INSTRUMENT_TICK_COLUMN,
            DataType::UInt32,
            false,
        ))];
        columns.extend(fields.iter().cloned());
        Ok(Self {
            fields,
            schema: Arc::new(Schema::new(columns)),
        })
    }

    pub(crate) fn batch<R: InstrumentRow>(&self, tick: u32, rows: &[R]) -> Result<RecordBatch> {
        let body = serde_arrow::to_record_batch(&self.fields, &rows)?;
        let mut columns: Vec<ArrayRef> = vec![Arc::new(UInt32Array::from(vec![tick; rows.len()]))];
        columns.extend(body.columns().iter().cloned());
        Ok(RecordBatch::try_new(Arc::clone(&self.schema), columns)?)
    }
}

/// The rows of one stored batch, each with its tick.
pub(crate) fn batch_rows<R: InstrumentRow>(batch: &RecordBatch) -> Result<Vec<(u32, R)>> {
    if batch.num_columns() == 0 || batch.schema().field(0).name() != INSTRUMENT_TICK_COLUMN {
        return Err(Error::invalid(format!(
            "channel {} does not start with {INSTRUMENT_TICK_COLUMN}",
            R::CHANNEL
        )));
    }
    let ticks = batch
        .column(0)
        .as_any()
        .downcast_ref::<UInt32Array>()
        .filter(|ticks| ticks.null_count() == 0)
        .ok_or_else(|| {
            Error::invalid(format!(
                "channel {} has a {INSTRUMENT_TICK_COLUMN} that is not a u32 on every row",
                R::CHANNEL
            ))
        })?;
    let body = batch.project(&(1..batch.num_columns()).collect::<Vec<_>>())?;
    let rows: Vec<R> = serde_arrow::from_record_batch(&body)?;
    Ok(ticks.values().iter().copied().zip(rows).collect())
}

/// What a unit and its main skill were aiming at, and where the skill's state
/// machine stood, at a snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetRefs {
    pub unit: ObjectRef,
    pub mech_lock_target: Option<ObjectRef>,
    pub normal_skill_fields_available: bool,
    pub skill_lock_target: Option<ObjectRef>,
    pub skill_attack_target: Option<ObjectRef>,
    /// The main skill's `SkillStateController` state, by class name.
    pub skill_state: Option<String>,
    /// Which of `SkillAttackController`'s phases is current: `before`,
    /// `attacking` or `after`, while the skill attacks.
    pub skill_attack_phase: Option<String>,
    /// `FightSkillBase.IsIdle`.
    pub skill_is_idle: Option<bool>,
}

impl InstrumentRow for TargetRefs {
    const CHANNEL: &'static str = "target_refs";
}

/// One `SkillAttackableChecker.Check` call: whose skill, and what the skill
/// held and which state it was in on either side of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillAttackableCheck {
    pub invocation_ordinal: u64,
    pub source_actor: ObjectRef,
    pub is_attacking_check: bool,
    pub before: CheckedSkill,
    pub after: CheckedSkill,
    pub check_return: bool,
}

impl InstrumentRow for SkillAttackableCheck {
    const CHANNEL: &'static str = "skill_attackable_checker";
}

/// A skill's lock, attack target, state and attack phase, read field by field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckedSkill {
    pub lock_target: Option<ObjectRef>,
    pub attack_target: Option<ObjectRef>,
    pub skill_state: Option<String>,
    pub skill_attack_phase: Option<String>,
}

/// One `ScoreRatingTargetSelector.CalculateScore` call: its arguments, raw, and
/// the score it returned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectorScore {
    pub invocation_ordinal: u64,
    pub distance_raw: i64,
    pub distance_score_raw: i64,
    pub angle_raw: i64,
    pub angle_score_raw: i64,
    pub max_attack_range_raw: i64,
    pub source_rotation_raw: i64,
    pub min_rotation_raw: i64,
    pub max_rotation_raw: i64,
    pub is_left_side: bool,
    pub score_raw: i64,
}

impl InstrumentRow for SelectorScore {
    const CHANNEL: &'static str = "selector_score";
}
