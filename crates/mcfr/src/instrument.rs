//! Instrument channels: what a recording observed of the fight's inside.
//!
//! A channel is one Parquet member, `instrument/<channel>.parquet`, beside the
//! recording's own tables. Its rows are a Rust type, whose Arrow schema is traced
//! from the type, and every row carries the tick it was observed on. The hash never
//! reads a channel: what the fight did is the recording, and a channel is what a
//! study asked to see of how it did it, so asking for one never changes a pin.

use std::sync::Arc;

use arrow_array::{Array, ArrayRef, RecordBatch, UInt32Array};
use arrow_schema::{DataType, Field, FieldRef, Schema, SchemaRef};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_arrow::schema::{SchemaLike, TracingOptions};

use crate::{Error, ObjectRef, QVec3, Result};

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
    /// The checked skill's index in its owner's `GetSkills()`: which of a
    /// grouped unit's slots made the call.
    pub skill_slot: Option<u16>,
    pub is_attacking_check: bool,
    pub before: CheckedSkill,
    pub after: CheckedSkill,
    pub check_return: bool,
}

impl InstrumentRow for SkillAttackableCheck {
    const CHANNEL: &'static str = "skill_attackable_checker";
}

/// One projectile's reach check: the `FightCalculator.IsInRange3D` that
/// `FightProjectile.Update` asks right after `CalculateMaxMoveDistance`,
/// against what that returned. A projectile out of reach is released with no
/// damage, so this is what decides a shot spent on nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectileReach {
    pub projectile: ObjectRef,
    /// The `ISkillOwner` that released it, if the capture names it.
    pub owner: Option<ObjectRef>,
    /// `FightProjectile.moveRange`, raw `FPoint`.
    pub move_range_raw: i64,
    /// What `CalculateMaxMoveDistance` returned, raw, which is the range
    /// `IsInRange3D` is asked against.
    pub max_move_raw: i64,
    /// The `FightTransform.position3D` it measures from.
    pub transform_position: QVec3,
    /// The radius it subtracts, raw.
    pub radius_raw: i64,
    /// Where the projectile stands.
    pub position: QVec3,
    pub in_range: bool,
}

impl InstrumentRow for ProjectileReach {
    const CHANNEL: &'static str = "projectile_reach";
}

/// One kill's search for the formations near enough to share its experience:
/// a call of `ExpSystem.AddRangeUnit`, which asks the killer's side's
/// `mechQuadtree` for a square `assistExpRange` wide around the target and
/// adds the formation of each unit it finds whose edge stands in range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpRange {
    /// The unit or building killed.
    pub target: Option<ObjectRef>,
    /// The formations sharing before the search, those that hit the target,
    /// in the order the build lists them.
    pub before: Vec<u64>,
    /// The formations the search added, in the order it added them.
    pub added: Vec<u64>,
}

impl InstrumentRow for ExpRange {
    const CHANNEL: &'static str = "exp_range";
}

/// One animator layer of a unit's model at a snapshot: the state the view's
/// `Animator` plays on that layer, how far through it, and the clips it
/// blends there. The pose a unit is drawn in is the view's, which the fight
/// does not read; this is what lets a reader draw the unit as the game did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnitPose {
    pub unit: ObjectRef,
    /// The layer's index in the controller.
    pub layer: u8,
    /// `Animator.GetLayerName`.
    pub layer_name: String,
    /// `Animator.GetLayerWeight`.
    pub layer_weight: f32,
    /// `AnimatorStateInfo.fullPathHash`: `Animator.StringToHash` of
    /// `<layer name>.<state name>`.
    pub state: i32,
    /// `AnimatorStateInfo.shortNameHash`: `Animator.StringToHash` of the
    /// state's name alone.
    pub state_name: i32,
    /// `AnimatorStateInfo.normalizedTime`: cycles played, the integer part
    /// counting loops.
    pub normalized_time: f32,
    /// `AnimatorStateInfo.length`, seconds.
    pub state_length: f32,
    /// `AnimatorStateInfo.speed` times its `speedMultiplier`.
    pub state_speed: f32,
    /// The state the layer is blending into, while `IsInTransition`.
    pub next_state: Option<i32>,
    /// The clips the current state plays, with their blend weights:
    /// `GetCurrentAnimatorClipInfo`.
    pub clips: Vec<PoseClip>,
    /// `Animator.speed`, the whole animator's playback rate.
    pub animator_speed: f32,
}

impl InstrumentRow for UnitPose {
    const CHANNEL: &'static str = "unit_pose";
}

/// One clip a layer plays, by its asset name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PoseClip {
    pub name: String,
    pub weight: f32,
}

/// A skill's lock, attack target, state and attack phase, read field by field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckedSkill {
    pub lock_target: Option<ObjectRef>,
    pub attack_target: Option<ObjectRef>,
    pub skill_state: Option<String>,
    pub skill_attack_phase: Option<String>,
    /// `FightSkill.attackTime`: the updates since the attack clock was last
    /// reset, which `CanPerformAttack` holds against `attackInterval`.
    pub attack_time: Option<i32>,
    /// `FightSkill.attackInterval`, in logic ticks.
    pub attack_interval: Option<i32>,
    /// `SkillAttackController.performCount`.
    pub perform_count: Option<i32>,
}

/// Which of the build's three ways a target search took.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetSearchPath {
    /// `ScoreRatingTargetSelector.Select` scoring on the main thread.
    Select,
    /// `Select` over 51 or more actors, scored by `ScoreRatingTargetSelectJob`
    /// on worker threads: the score terms are not seen, only the scores.
    SelectJob,
    /// A main skill's search batched for its whole team by
    /// `TeamScoreRatingTargetSelectJob` and read back by `TrySelect`.
    Team,
}

/// One target search: who searched, how, and what it chose. Its best
/// candidates are `target_candidate` rows with the same `search`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetSearch {
    /// The search's ordinal within its tick, in the order searches returned.
    pub search: u32,
    /// The unit or building that searched; null for an attacker the
    /// recording does not hold.
    pub source: Option<ObjectRef>,
    /// The searching skill's index in its owner's `GetSkills()`; null when the
    /// unit or construction searched for itself.
    pub skill_slot: Option<u16>,
    pub path: TargetSearchPath,
    /// How many candidates were scored.
    pub candidates: u32,
    /// What the search returned, which may be the second best when the best
    /// is not visible.
    pub target: Option<ObjectRef>,
    pub nearest: Option<ObjectRef>,
}

impl InstrumentRow for TargetSearch {
    const CHANNEL: &'static str = "target_search";
}

/// One of a search's best-scored candidates, lowest score first; at most
/// [`TARGET_CANDIDATES`] of them per search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetCandidate {
    pub search: u32,
    /// 0 for the lowest score, which the build calls best.
    pub rank: u32,
    /// Null for an actor the recording does not hold.
    pub candidate: Option<ObjectRef>,
    pub score_raw: i64,
    /// `CalculateScore`'s per-candidate arguments; null on the `select_job`
    /// path, whose worker threads compute them unseen.
    pub distance_raw: Option<i64>,
    pub distance_score_raw: Option<i64>,
    pub angle_raw: Option<i64>,
    pub angle_score_raw: Option<i64>,
    pub is_left_side: Option<bool>,
    /// `CalculateScore`'s per-search arguments: the range past which a
    /// candidate is penalised, and the rotation window a candidate in range
    /// must lie in (`Selector.CalculateRotationData`).
    pub max_attack_range_raw: Option<i64>,
    pub source_rotation_raw: Option<i64>,
    pub min_rotation_raw: Option<i64>,
    pub max_rotation_raw: Option<i64>,
}

impl InstrumentRow for TargetCandidate {
    const CHANNEL: &'static str = "target_candidate";
}

/// How many of a search's candidates `target_candidate` keeps.
pub const TARGET_CANDIDATES: usize = 5;

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
    /// Null for an agent the recording does not hold: a map `FightCrystal`
    /// in neither side's building lists.
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
