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

/// A horizontal vector in RVO space, raw Q32.32 on each axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RvoVec {
    pub x: i64,
    pub y: i64,
}

/// How an RVO solve ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RvoExit {
    /// `locked`: speed 0, target the agent's own position; no neighbour is read.
    Locked,
    /// `manuallyControlled`: the solve returns before writing anything.
    Manual,
    /// The desired velocity lies outside every VO and is kept, biased.
    Free,
    /// The desired velocity lies inside a VO, and two gradient traces avoid it.
    Avoided,
}

/// One `RVOAgentFixed.CalculateVelocity`: what the agent brought to the solve
/// and what it left with. Its neighbours are `rvo_neighbour` rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RvoSolve {
    pub agent: ObjectRef,
    pub exit: RvoExit,
    pub position: RvoVec,
    pub elevation_raw: i64,
    pub height_raw: i64,
    pub current_velocity: RvoVec,
    /// `sync_desiredVelocity`, which the bias and both traces read.
    pub desired_velocity: RvoVec,
    /// `desiredTargetPointInVelocitySpace`.
    pub desired_target: RvoVec,
    pub desired_speed_raw: i64,
    pub max_speed_raw: i64,
    pub radius_outer_raw: i64,
    pub radius_inner_raw: i64,
    pub size: i32,
    pub priority_raw: i64,
    pub layer: i32,
    pub collides_with: i32,
    pub group: i32,
    pub ignore_same_group: bool,
    pub team_id: i32,
    pub team_radius_raw: i64,
    pub max_neighbours: i32,
    pub neighbour_count: u32,
    /// The desired velocity and target after `BiasDesiredVelocity`; absent
    /// for a locked or manual exit.
    pub biased_velocity: Option<RvoVec>,
    pub biased_target: Option<RvoVec>,
    /// The two `Trace`s of an avoided solve, each its best point and score.
    /// The first starts from the current velocity, the second from the biased
    /// desired one; the solve keeps the first only when it scores lower.
    pub first_trace_point: Option<RvoVec>,
    pub first_trace_score_raw: Option<i64>,
    pub second_trace_point: Option<RvoVec>,
    pub second_trace_score_raw: Option<i64>,
    /// `calculatedTargetPoint` and `calculatedSpeed` after the solve; absent
    /// for a manual exit, which writes neither.
    pub output_target: Option<RvoVec>,
    pub output_speed_raw: Option<i64>,
}

impl InstrumentRow for RvoSolve {
    const CHANNEL: &'static str = "rvo_solve";
}

/// What `GenerateNeighbourAgentVOs` made of one neighbour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RvoNeighbourKind {
    /// Another RVO group: `GenerateOpponentVOs`.
    Opponent,
    /// The same group, both in one positive RVO team: the team radii summed.
    TeamRadius,
    /// The same group otherwise: inner or outer radii summed by size.
    SameGroup,
    /// The same group, and the neighbour lets it pass: no VO.
    IgnoredSameGroup,
    /// The vertical ranges do not overlap: no VO.
    OtherElevation,
}

/// One entry of an agent's neighbour list at a solve, at most
/// `max_neighbours` of them per solve.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RvoNeighbour {
    pub agent: ObjectRef,
    /// Position in the neighbour list, nearest first.
    pub slot: u32,
    /// Null for an agent the recording does not hold, such as a neutral
    /// crystal of the map.
    pub neighbour: Option<ObjectRef>,
    pub distance_sq_raw: i64,
    pub kind: RvoNeighbourKind,
    /// The VO's index in the agent's buffer, for a kind that makes one.
    pub vo: Option<u32>,
    pub radius_raw: Option<i64>,
    pub colliding: Option<bool>,
    /// `VO.Gradient`'s weight at the desired velocity before the bias: how
    /// far it lies inside this VO. The largest positive one sets the bias.
    pub penetration_raw: Option<i64>,
    /// `VO.ScaledGradient`'s weight at the solve's output velocity, for an
    /// avoided solve. The largest is the VO that bound the solution.
    pub weight_raw: Option<i64>,
}

impl InstrumentRow for RvoNeighbour {
    const CHANNEL: &'static str = "rvo_neighbour";
}

/// One VO of an agent's buffer at a solve, field by field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RvoVo {
    pub agent: ObjectRef,
    pub vo: u32,
    pub line1: RvoVec,
    pub line2: RvoVec,
    pub dir1: RvoVec,
    pub dir2: RvoVec,
    pub cutoff_line: RvoVec,
    pub cutoff_dir: RvoVec,
    pub circle_center: RvoVec,
    pub colliding: bool,
    pub radius_raw: i64,
    pub weight_factor_raw: i64,
    pub weight_bonus_raw: i64,
    pub segment_start: RvoVec,
    pub segment_end: RvoVec,
    pub segment: bool,
}

impl InstrumentRow for RvoVo {
    const CHANNEL: &'static str = "rvo_vo";
}
