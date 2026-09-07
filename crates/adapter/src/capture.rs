use crate::{
    il2cpp::{Api, Class, FieldInfo, MethodInfo, Object, argument, object_argument},
    runtime::Runtime,
};
use jpeg_encoder::{ColorType, Encoder};
use mechcore_layout::{
    BattleSkillDefinition, ContraptionPlacement, EnergyTower, Formation, Layout, Position,
    ResearchCenter, Side, Sides, StaticPlacement, Techs, Terrain as LayoutTerrain,
    TerrainType as LayoutTerrainType, battle_skill_type_from_id, canonical_embedded_yaml,
    construction_type_from_id, contraption_type_from_id, unit_type_from_id,
};
use mechcore_mcfr::{
    BuffModifierSet, BuildingState, Domain, DurableContext, Event, EventPayload, GaugeI32,
    LiveUnitState, MotionState, ObjectKind, ObjectRef, PersonalShieldState, ProjectileState, QPose,
    QVec3, RateModifier, Rational, ShieldDestroyedReason, ShieldRoundPolicy, ShieldSourceKind,
    ShieldState, SkillDynamicModifierSet, SkillNumericModifierState, TerrainApplicationState,
    TerrainEffectClock, TerrainGridState, TerrainLogicLifetime, TerrainRemovedReason, TerrainState,
    TerrainType, TransitionEvents, UnitDynamicModifierSet, ValueModifier, Visibility,
    WeaponAimState, WorldSnapshot,
};
use serde::{Deserialize, Serialize};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet, VecDeque},
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicPtr, AtomicU64, Ordering},
    },
};

const QUEUE_CAPACITY: usize = 4096;
const TIME_UNITS_PER_SECOND: u32 = 2_000;
const CAPTURE_WIDTH: u16 = 1_920;
const CAPTURE_HEIGHT: u16 = 1_080;
const CAPTURE_FRAME_RATE: i32 = 20;
const FIXED_ONE_RAW: i64 = 1_i64 << 32;
const ENERGY_TOWER_KIND: i32 = 1;
const RESEARCH_CENTER_KIND: i32 = 2;
const RANGE_ENHANCEMENT_SKILL: i32 = 5;
const MOVEMENT_ENHANCEMENT_SKILL: i32 = 6;
pub(crate) const CALIBRATION_VIEW: &str = "calibration_topdown";
pub(crate) const CALIBRATION_CAMERA_HEIGHT: f32 = 1_070.0;
pub(crate) const CALIBRATION_CAMERA_Z: f32 = -1_070.0;
pub(crate) const CALIBRATION_CAMERA_PITCH_DEGREES: f32 = 45.0;
pub(crate) const CALIBRATION_FIELD_OF_VIEW_DEGREES: f32 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CaptureInstrumentationProfile {
    TargetRefsV1,
    TargetRefsRvoV1,
    SkillAttackableCheckerV1,
    SelectorScoreV1,
    SelectorScoreRvoV1,
}

impl CaptureInstrumentationProfile {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::TargetRefsV1 => "target_refs_v1",
            Self::TargetRefsRvoV1 => "target_refs_rvo_v1",
            Self::SkillAttackableCheckerV1 => "skill_attackable_checker_v1",
            Self::SelectorScoreV1 => "selector_score_v1",
            Self::SelectorScoreRvoV1 => "selector_score_rvo_v1",
        }
    }

    pub(crate) const fn channel(self) -> &'static str {
        match self {
            Self::TargetRefsV1 => "target_refs",
            Self::TargetRefsRvoV1 => "target_refs_rvo",
            Self::SkillAttackableCheckerV1 => "skill_attackable_checker",
            Self::SelectorScoreV1 => "selector_score",
            Self::SelectorScoreRvoV1 => "selector_score_rvo",
        }
    }

    const fn includes_target_refs(self) -> bool {
        matches!(self, Self::TargetRefsV1 | Self::TargetRefsRvoV1)
    }

    const fn includes_rvo(self) -> bool {
        matches!(self, Self::TargetRefsRvoV1 | Self::SelectorScoreRvoV1)
    }

    const fn includes_skill_attackable_checker(self) -> bool {
        matches!(self, Self::SkillAttackableCheckerV1)
    }

    const fn includes_selector_score(self) -> bool {
        matches!(self, Self::SelectorScoreV1 | Self::SelectorScoreRvoV1)
    }
}

/// Research-only filter using one-based MCFR combat ticks. In build 2259,
/// FightController.Update advances the native time counter by 100 per tick.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RvoCaptureScope {
    pub(crate) start_tick: u64,
    pub(crate) end_tick: u64,
    pub(crate) unit_ids: Vec<u64>,
}

impl RvoCaptureScope {
    pub(crate) fn validate(&self, profile: CaptureInstrumentationProfile) -> Result<(), String> {
        if profile != CaptureInstrumentationProfile::TargetRefsRvoV1 {
            return Err("rvo_scope requires target_refs_rvo_v1".into());
        }
        if self.start_tick == 0
            || self.start_tick > self.end_tick
            || self.end_tick - self.start_tick >= 64
            || self.unit_ids.is_empty()
            || self.unit_ids.len() > 8
            || self.unit_ids.contains(&0)
            || self.unit_ids.iter().copied().collect::<BTreeSet<_>>().len() != self.unit_ids.len()
        {
            return Err("rvo_scope requires 1..=8 unique positive MCFR unit_ids and an inclusive window of 1..=64 positive MCFR ticks".into());
        }
        Ok(())
    }

    fn includes_native_tick(&self, tick: u64) -> bool {
        tick.is_multiple_of(100) && (self.start_tick..=self.end_tick).contains(&(tick / 100))
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct TargetRefsObservation {
    pub(crate) units: Vec<UnitTargetRefsObservation>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct UnitTargetRefsObservation {
    pub(crate) unit: ObjectRef,
    pub(crate) mech_lock_target: Option<ObjectRef>,
    pub(crate) normal_skill_fields_available: bool,
    pub(crate) skill_lock_target: Option<ObjectRef>,
    pub(crate) skill_attack_target: Option<ObjectRef>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum CaptureInstrumentationObservation {
    TargetRefs(TargetRefsObservation),
    TargetRefsRvo(TargetRefsRvoObservation),
    SkillAttackableChecker(SkillAttackableCheckerObservation),
    SelectorScore(SelectorScoreObservation),
    SelectorScoreRvo(SelectorScoreRvoObservation),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub(crate) struct SelectorScoreObservation {
    pub(crate) score_calculations: Vec<SelectorScoreCalculation>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct SelectorScoreRvoObservation {
    pub(crate) selector_score: SelectorScoreObservation,
    pub(crate) rvo_updates: Vec<RvoUpdateObservation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct SelectorScoreCalculation {
    pub(crate) invocation_ordinal: u64,
    pub(crate) distance_raw: i64,
    pub(crate) distance_score_raw: i64,
    pub(crate) angle_raw: i64,
    pub(crate) angle_score_raw: i64,
    pub(crate) max_attack_range_raw: i64,
    pub(crate) source_rotation_raw: i64,
    pub(crate) min_rotation_raw: i64,
    pub(crate) max_rotation_raw: i64,
    pub(crate) is_left_side: bool,
    pub(crate) score_raw: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub(crate) struct SkillAttackableCheckerObservation {
    pub(crate) checker_calls: Vec<SkillAttackableCheckerCall>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct SkillAttackableCheckerCall {
    pub(crate) invocation_ordinal: u64,
    pub(crate) native_logic_tick: u64,
    pub(crate) source_actor: ObjectRef,
    pub(crate) source_skill_id: i32,
    pub(crate) is_attacking_check: bool,
    pub(crate) previous_attack_target: Option<CheckerTargetObservation>,
    pub(crate) post_attack_target_candidate: Option<CheckerTargetObservation>,
    pub(crate) quick_switch_enabled: bool,
    pub(crate) check_return: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct CheckerTargetObservation {
    pub(crate) target_ref: ObjectRef,
    pub(crate) qualifying_status: CheckerQualifyingStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the checker contract remains offline-only until its native hook is reviewed"
    )
)]
pub(crate) enum CheckerQualifyingStatus {
    Unit {
        alive: bool,
        active_or_available: bool,
        targetable: bool,
    },
    Building {
        alive: bool,
        active_or_available: bool,
        targetable: bool,
        destroyed: bool,
    },
}

impl CheckerQualifyingStatus {
    const fn kind(self) -> ObjectKind {
        match self {
            Self::Unit { .. } => ObjectKind::Unit,
            Self::Building { .. } => ObjectKind::Building,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct TargetRefsRvoObservation {
    pub(crate) target_refs: TargetRefsObservation,
    pub(crate) rvo_updates: Vec<RvoUpdateObservation>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct RvoUpdateObservation {
    pub(crate) update_ordinal: u64,
    pub(crate) start_native_tick: u64,
    pub(crate) publish_native_tick: u64,
    pub(crate) double_buffering: bool,
    pub(crate) multithreaded: bool,
    pub(crate) symmetry_breaking_bias_raw: i64,
    pub(crate) agents: Vec<RvoAgentObservation>,
    pub(crate) published_agents: Vec<RvoAgentObservation>,
    pub(crate) neighbour_sets: Vec<RvoNeighbourSetObservation>,
    pub(crate) vo_buffers: Vec<RvoVoBufferObservation>,
    pub(crate) opponent_vos: Vec<RvoOpponentVoObservation>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct RvoAgentObservation {
    pub(crate) ordinal: u32,
    pub(crate) agent: RvoAgentRefObservation,
    pub(crate) radius_inner_raw: i64,
    pub(crate) size: i32,
    pub(crate) radius_outer_raw: i64,
    pub(crate) max_speed_raw: i64,
    pub(crate) desired_speed_raw: i64,
    pub(crate) agent_time_horizon_raw: i64,
    pub(crate) priority_raw: i64,
    pub(crate) published_calculated_speed_raw: i64,
    pub(crate) current_velocity_x_raw: i64,
    pub(crate) current_velocity_y_raw: i64,
    pub(crate) desired_velocity_x_raw: i64,
    pub(crate) desired_velocity_y_raw: i64,
    pub(crate) desired_target_x_raw: i64,
    pub(crate) desired_target_y_raw: i64,
    pub(crate) calculated_target_x_raw: i64,
    pub(crate) calculated_target_y_raw: i64,
    pub(crate) locked: bool,
    pub(crate) layer: i32,
    pub(crate) collides_with: i32,
    pub(crate) max_neighbours: i32,
    pub(crate) main_layer: i32,
    pub(crate) sync_main_layer: i32,
    pub(crate) group: i32,
    pub(crate) sync_group: i32,
    pub(crate) ignore_same_group: bool,
    pub(crate) sync_ignore_same_group: bool,
    pub(crate) team_id: i32,
    pub(crate) team_radius_raw: i64,
    pub(crate) sync_team_id: i32,
    pub(crate) sync_team_radius_raw: i64,
    pub(crate) position_x_raw: i64,
    pub(crate) position_y_raw: i64,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct RvoNeighbourSetObservation {
    pub(crate) source_call_ordinal: u64,
    pub(crate) source: RvoAgentRefObservation,
    pub(crate) neighbour_count: u32,
    pub(crate) neighbours: Vec<RvoNeighbourObservation>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct RvoNeighbourObservation {
    pub(crate) ordinal: u32,
    pub(crate) target: RvoAgentRefObservation,
    pub(crate) distance_sq_raw: i64,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct RvoOpponentVoObservation {
    pub(crate) update_ordinal: u64,
    pub(crate) call_ordinal: u64,
    pub(crate) source: RvoAgentRefObservation,
    pub(crate) target: RvoAgentRefObservation,
    pub(crate) vo_buffer_length_before: u32,
    pub(crate) vo_buffer_length_after: u32,
    pub(crate) appended_colliding: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct RvoVoBufferObservation {
    pub(crate) call_ordinal: u64,
    pub(crate) source: RvoAgentRefObservation,
    pub(crate) vos: Vec<RvoVoObservation>,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) struct RvoVoObservation {
    pub(crate) line1_x_raw: i64,
    pub(crate) line1_y_raw: i64,
    pub(crate) line2_x_raw: i64,
    pub(crate) line2_y_raw: i64,
    pub(crate) dir1_x_raw: i64,
    pub(crate) dir1_y_raw: i64,
    pub(crate) dir2_x_raw: i64,
    pub(crate) dir2_y_raw: i64,
    pub(crate) cutoff_line_x_raw: i64,
    pub(crate) cutoff_line_y_raw: i64,
    pub(crate) cutoff_dir_x_raw: i64,
    pub(crate) cutoff_dir_y_raw: i64,
    pub(crate) circle_center_x_raw: i64,
    pub(crate) circle_center_y_raw: i64,
    pub(crate) colliding: bool,
    pub(crate) radius_raw: i64,
    pub(crate) weight_factor_raw: i64,
    pub(crate) weight_bonus_raw: i64,
    pub(crate) segment_start_x_raw: i64,
    pub(crate) segment_start_y_raw: i64,
    pub(crate) segment_end_x_raw: i64,
    pub(crate) segment_end_y_raw: i64,
    pub(crate) segment: bool,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum RvoAgentRefObservation {
    Entity(ObjectRef),
    Internal { internal_agent_ordinal: u64 },
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FixedPoint {
    raw: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FixedVec2 {
    x: FixedPoint,
    y: FixedPoint,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FixedVec3 {
    x: FixedPoint,
    y: FixedPoint,
    z: FixedPoint,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NativeHitDamageInfo {
    source_team: *mut Object,
    source_skill_owner: *mut Object,
    target_actor: *mut Object,
    is_direct_hit: bool,
    is_suicide: bool,
    _padding_1a: [u8; 2],
    damage_distance: i32,
    damage: i32,
    damage_real: i32,
    hit_point: FixedVec3,
    is_special_attack: bool,
    _padding_41: [u8; 7],
    damage_provider: *mut Object,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FixedRvoTeam {
    id: i32,
    _padding: i32,
    radius: FixedPoint,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NativeRvoVo {
    line1: FixedVec2,
    line2: FixedVec2,
    dir1: FixedVec2,
    dir2: FixedVec2,
    cutoff_line: FixedVec2,
    cutoff_dir: FixedVec2,
    circle_center: FixedVec2,
    colliding: bool,
    radius: FixedPoint,
    weight_factor: FixedPoint,
    weight_bonus: FixedPoint,
    segment_start: FixedVec2,
    segment_end: FixedVec2,
    segment: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FixedRect {
    center: FixedVec2,
    size: FixedVec2,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UnityVec3 {
    x: f32,
    y: f32,
    z: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct UnityVec2Int {
    x: i32,
    y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct MapVector {
    x: i32,
    y: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UnityRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[derive(Clone)]
/// Rendered pixels as Unity returns them: bottom-up rows, 3 or 4 channels.
pub(crate) struct RawFrame {
    pixels: Vec<u8>,
    width: u16,
    height: u16,
    channels: usize,
}

impl RawFrame {
    /// Flip to top-down, drop any alpha, and encode. Called off the Unity main
    /// thread by the recording loop.
    pub(crate) fn encode_jpeg(&self) -> Result<Vec<u8>, String> {
        let row_bytes = usize::from(self.width) * self.channels;
        let pixels = usize::from(self.width) * usize::from(self.height);
        let mut rgb = Vec::with_capacity(pixels * 3);
        for row in self.pixels.chunks_exact(row_bytes).rev() {
            for pixel in row.chunks_exact(self.channels) {
                rgb.extend_from_slice(&pixel[..3]);
            }
        }
        let mut jpeg = Vec::new();
        Encoder::new(&mut jpeg, 90)
            .encode(&rgb, self.width, self.height, ColorType::Rgb)
            .map_err(|error| format!("cannot encode captured JPEG: {error}"))?;
        Ok(jpeg)
    }
}

pub(crate) enum CaptureMessage {
    Initial {
        game_build: String,
        context: DurableContext,
        layout_yaml: String,
    },
    Transition {
        events: TransitionEvents,
        state: WorldSnapshot,
        instrumentation: Option<CaptureInstrumentationObservation>,
        terminal: bool,
        frame: Option<RawFrame>,
    },
    Failure(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CaptureStartMode {
    TrainingGround,
    Replay,
}

enum PendingVisualMessage {
    Transition {
        events: TransitionEvents,
        state: WorldSnapshot,
        instrumentation: Option<CaptureInstrumentationObservation>,
        terminal: bool,
    },
}

struct VisualCapture {
    api: Api,
    camera_handle: u32,
    camera_transform_handle: u32,
    controlled_handles: [u32; 4],
    original_controlled_enabled: [bool; 4],
    texture_handle: u32,
    screen_class: usize,
    application_class: usize,
    set_resolution: usize,
    destroy_immediate: usize,
    width: u16,
    height: u16,
    original_screen_width: i32,
    original_screen_height: i32,
    original_fullscreen: bool,
    original_target_frame_rate: i32,
    original_camera_position: UnityVec3,
    original_camera_euler_angles: UnityVec3,
    original_camera_orthographic: bool,
    original_camera_orthographic_size: f32,
    original_camera_field_of_view: f32,
    original_camera_far_clip_plane: f32,
}

#[derive(Default)]
struct Metadata {
    projectile_system_class: usize,
    range_item_system_class: usize,
    fight_ground_fire_class: usize,
    advanced_energy_shield_system_class: usize,
    energy_shield_contraption_class: usize,
    commander_energy_shield_class: usize,
    owner_advanced_shield_class: usize,
    spawned_temporary_shield_class: usize,
    fight_team_buildings: usize,
    fight_team_constructions: usize,
    projectile_controllers: usize,
    projectile_in_energy_shields: usize,
    range_item_affected_units: usize,
    range_item_affected_unit_times: usize,
    range_item_effect_time_duration: usize,
    range_item_time: usize,
    range_item_life_time: usize,
    ground_fire_time: usize,
    ground_fire_life_time: usize,
    grid_position_x: usize,
    grid_position_y: usize,
    grid_rows: usize,
    grid_size: usize,
    motion_fsm: usize,
    motion_idle_state_class: usize,
    motion_move_state_class: usize,
    motion_attack_state_class: usize,
    motion_stop_state_class: usize,
    fight_mech_lock_target: usize,
    fight_skill_class: Option<usize>,
    fight_skill_lock_target: Option<usize>,
    fight_skill_attack_target: Option<usize>,
    rvo: Option<RvoMetadata>,
    rvo_error: Option<String>,
    selector_score_available: bool,
    selector_score_error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RawSelectorScoreCalculation {
    invocation_ordinal: u64,
    distance_raw: i64,
    distance_score_raw: i64,
    angle_raw: i64,
    angle_score_raw: i64,
    max_attack_range_raw: i64,
    source_rotation_raw: i64,
    min_rotation_raw: i64,
    max_rotation_raw: i64,
    is_left_side: bool,
    score_raw: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RawCheckerTarget {
    pointer: usize,
    qualifying_status: CheckerQualifyingStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OpenCheckerCall {
    invocation_ordinal: u64,
    native_logic_tick: u64,
    skill: usize,
    source_actor: usize,
    source_skill_id: i32,
    is_attacking_check: bool,
    previous_attack_target: Option<RawCheckerTarget>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CompletedCheckerCall {
    entry: OpenCheckerCall,
    post_attack_target_candidate: Option<RawCheckerTarget>,
    quick_switch_enabled: bool,
    check_return: bool,
}

#[derive(Clone, Copy)]
struct RvoMetadata {
    fight_actor_rvo_controller: usize,
    rvo_controller_agent: usize,
    rvo_controller_owner: usize,
    simulator_double_buffering: usize,
    simulator_symmetry_breaking_bias: usize,
    simulator_workers: usize,
    simulator_agents: usize,
    agent_radius_inner: usize,
    agent_size: usize,
    agent_radius_outer: usize,
    agent_max_speed: usize,
    agent_desired_speed: usize,
    agent_time_horizon: usize,
    agent_priority: usize,
    agent_published_calculated_speed: usize,
    agent_current_velocity: usize,
    agent_desired_velocity: usize,
    agent_desired_target: usize,
    agent_calculated_target: usize,
    agent_locked: usize,
    agent_layer: usize,
    agent_collides_with: usize,
    agent_internal_max_neighbours: usize,
    agent_position: usize,
    agent_simulator: usize,
    rvo_agent_class: usize,
    rvo_agent_main_layer: usize,
    rvo_agent_sync_main_layer: usize,
    rvo_agent_group: usize,
    rvo_agent_sync_group: usize,
    rvo_agent_ignore_same_group: usize,
    rvo_agent_sync_ignore_same_group: usize,
    rvo_agent_team: usize,
    rvo_agent_sync_team: usize,
    agent_max_neighbours: usize,
    agent_neighbour_count: usize,
    agent_neighbours: usize,
    agent_neighbour_dists: usize,
    vo_buffer: usize,
    vo_buffer_length: usize,
}

#[derive(Clone, Copy, Default)]
struct NativeRvoAgentState {
    ordinal: u32,
    pointer: usize,
    radius_inner: FixedPoint,
    size: i32,
    radius_outer: FixedPoint,
    max_speed: FixedPoint,
    desired_speed: FixedPoint,
    agent_time_horizon: FixedPoint,
    priority: FixedPoint,
    published_calculated_speed: FixedPoint,
    current_velocity: FixedVec2,
    desired_velocity: FixedVec2,
    desired_target: FixedVec2,
    calculated_target: FixedVec2,
    locked: bool,
    layer: i32,
    collides_with: i32,
    max_neighbours: i32,
    main_layer: i32,
    sync_main_layer: i32,
    group: i32,
    sync_group: i32,
    ignore_same_group: bool,
    sync_ignore_same_group: bool,
    team: FixedRvoTeam,
    sync_team: FixedRvoTeam,
    position: FixedVec2,
}

struct NativeRvoNeighbourSet {
    update_ordinal: u64,
    source_call_ordinal: u64,
    source: usize,
    neighbours: Vec<(usize, i64)>,
}

struct NativeOpponentVo {
    update_ordinal: u64,
    call_ordinal: u64,
    source: usize,
    target: usize,
    vo_buffer_length_before: i32,
    vo_buffer_length_after: i32,
    appended_colliding: bool,
}

struct NativeRvoVoBuffer {
    update_ordinal: u64,
    call_ordinal: u64,
    source: usize,
    vos: Vec<NativeRvoVo>,
}

#[derive(Default)]
#[allow(clippy::struct_excessive_bools)] // The booleans mirror independent native hook boundaries.
struct CaptureState {
    availability: Option<String>,
    metadata: Metadata,
    instrumentation_profile: Option<CaptureInstrumentationProfile>,
    rvo_scope: Option<RvoCaptureScope>,
    armed: bool,
    initialized: bool,
    entered_fighting: bool,
    /// Whether the caller asked for native combat speed-up.
    speed_up_allowed: bool,
    speed_up_requested: bool,
    await_replay_deployment: bool,
    last_native_tick: Option<u64>,
    native_tick_step: Option<u64>,
    deployment_layout_yaml: Option<String>,
    queue: VecDeque<CaptureMessage>,
    unit_ids: BTreeMap<usize, u64>,
    building_ids: BTreeMap<usize, u64>,
    projectile_ids: BTreeMap<usize, u64>,
    shield_ids: BTreeMap<usize, u64>,
    shield_ids_finalized: bool,
    terrain_ids: BTreeMap<usize, u64>,
    live_shield_pointers: BTreeSet<usize>,
    retired_shield_pointers: BTreeSet<usize>,
    shield_last_states: BTreeMap<usize, ShieldState>,
    live_terrain_pointers: BTreeSet<usize>,
    retired_terrain_pointers: BTreeSet<usize>,
    terrain_last_states: BTreeMap<usize, TerrainState>,
    pending_projectile_absorptions: BTreeMap<u64, ObjectRef>,
    original_unit_teams: BTreeMap<usize, u32>,
    object_teams: BTreeMap<ObjectRef, u32>,
    emitted_deaths: BTreeSet<ObjectRef>,
    last_damage_sources: BTreeMap<ObjectRef, DamageAttribution>,
    formation_ids: BTreeMap<usize, u64>,
    rvo_agent_refs: BTreeMap<usize, ObjectRef>,
    rvo_agent_owners: BTreeMap<usize, usize>,
    rvo_internal_agent_ids: BTreeMap<usize, u64>,
    next_unit_id: u64,
    next_building_id: u64,
    next_projectile_id: u64,
    next_shield_id: u64,
    next_terrain_id: u64,
    next_formation_id: u64,
    next_rvo_internal_agent_id: u64,
    next_checker_invocation_ordinal: u64,
    next_selector_invocation_ordinal: u64,
    selector_score_calculations: Vec<RawSelectorScoreCalculation>,
    in_update: bool,
    traces: Vec<NativeTrace>,
    open_checker_calls: BTreeMap<u64, OpenCheckerCall>,
    completed_checker_calls: Vec<CompletedCheckerCall>,
    rvo_neighbour_sets: Vec<NativeRvoNeighbourSet>,
    rvo_agent_sets: BTreeMap<u64, Vec<NativeRvoAgentState>>,
    rvo_published_agent_sets: BTreeMap<u64, Vec<NativeRvoAgentState>>,
    rvo_vo_buffers: Vec<NativeRvoVoBuffer>,
    opponent_vos: Vec<NativeOpponentVo>,
    rvo_update_modes: BTreeMap<u64, bool>,
    rvo_update_symmetry_breaking_biases: BTreeMap<u64, FixedPoint>,
    rvo_update_start_native_ticks: BTreeMap<u64, u64>,
    rvo_update_publish_native_ticks: BTreeMap<u64, u64>,
    rvo_update_multithreaded: BTreeMap<u64, bool>,
    visual: Option<VisualCapture>,
    pending_visual: Option<PendingVisualMessage>,
    render_completed: bool,
}

impl CaptureState {
    fn reset_session(&mut self) {
        self.armed = false;
        self.initialized = false;
        self.entered_fighting = false;
        self.speed_up_allowed = false;
        self.speed_up_requested = false;
        self.await_replay_deployment = false;
        self.last_native_tick = None;
        self.native_tick_step = None;
        self.deployment_layout_yaml = None;
        self.instrumentation_profile = None;
        self.rvo_scope = None;
        self.queue.clear();
        self.unit_ids.clear();
        self.building_ids.clear();
        self.projectile_ids.clear();
        self.shield_ids.clear();
        self.shield_ids_finalized = false;
        self.terrain_ids.clear();
        self.live_shield_pointers.clear();
        self.retired_shield_pointers.clear();
        self.shield_last_states.clear();
        self.live_terrain_pointers.clear();
        self.retired_terrain_pointers.clear();
        self.terrain_last_states.clear();
        self.pending_projectile_absorptions.clear();
        self.original_unit_teams.clear();
        self.object_teams.clear();
        self.emitted_deaths.clear();
        self.last_damage_sources.clear();
        self.formation_ids.clear();
        self.rvo_agent_refs.clear();
        self.rvo_agent_owners.clear();
        self.rvo_internal_agent_ids.clear();
        self.next_unit_id = 1;
        self.next_building_id = 1;
        self.next_projectile_id = 1;
        self.next_shield_id = 1;
        self.next_terrain_id = 1;
        self.next_formation_id = 1;
        self.next_rvo_internal_agent_id = 0;
        self.next_checker_invocation_ordinal = 0;
        self.next_selector_invocation_ordinal = 0;
        self.selector_score_calculations.clear();
        self.in_update = false;
        self.traces.clear();
        self.open_checker_calls.clear();
        self.completed_checker_calls.clear();
        self.rvo_neighbour_sets.clear();
        self.rvo_agent_sets.clear();
        self.rvo_published_agent_sets.clear();
        self.rvo_vo_buffers.clear();
        self.opponent_vos.clear();
        self.rvo_update_modes.clear();
        self.rvo_update_symmetry_breaking_biases.clear();
        self.rvo_update_start_native_ticks.clear();
        self.rvo_update_publish_native_ticks.clear();
        self.rvo_update_multithreaded.clear();
        self.visual = None;
        self.pending_visual = None;
        self.render_completed = false;
    }

    fn push(&mut self, message: CaptureMessage) -> Result<(), String> {
        if self.queue.len() >= QUEUE_CAPACITY {
            self.armed = false;
            return Err("capture queue overflowed before MCFR writer drained it".into());
        }
        self.queue.push_back(message);
        Ok(())
    }

    fn fail(&mut self, reason: String) {
        self.armed = false;
        if self.queue.len() < QUEUE_CAPACITY {
            self.queue.push_back(CaptureMessage::Failure(reason));
        }
    }
}

impl VisualCapture {
    #[allow(clippy::too_many_lines)]
    fn new(runtime: &Runtime) -> Result<Self, String> {
        let api = runtime.api;
        let screen_class = api
            .class("UnityEngine.CoreModule.dll", "UnityEngine", "Screen")
            .map_err(|error| error.to_string())?;
        let application_class = api
            .class("UnityEngine.CoreModule.dll", "UnityEngine", "Application")
            .map_err(|error| error.to_string())?;
        let original_target_frame_rate = api
            .invoke_static(application_class, "get_targetFrameRate", &mut [])
            .and_then(|value| api.unbox::<i32>(value, "Application.targetFrameRate"))
            .map_err(|error| error.to_string())?;
        let (original_screen_width, original_screen_height, original_fullscreen) =
            screen_state(api, screen_class)?;
        if original_screen_width <= 0 || original_screen_height <= 0 {
            return Err("screen dimensions must be positive".into());
        }
        let set_resolution = api
            .class_method_with_parameter_types(
                screen_class,
                "SetResolution",
                &["System.Int32", "System.Int32", "System.Boolean"],
            )
            .map_err(|error| error.to_string())?;
        let camera_class = api
            .class("UnityEngine.CoreModule.dll", "UnityEngine", "Camera")
            .map_err(|error| error.to_string())?;
        let camera = api
            .invoke_static(camera_class, "get_main", &mut [])
            .map_err(|error| error.to_string())?;
        if camera.is_null() {
            return Err("Unity main camera is unavailable".into());
        }
        let camera_transform = api
            .invoke(camera, "get_transform", &mut [])
            .map_err(|error| error.to_string())?;
        if camera_transform.is_null() {
            return Err("Unity main camera transform is unavailable".into());
        }
        let original_camera_position = api
            .invoke_value::<UnityVec3>(camera_transform, "get_position", &mut [])
            .map_err(|error| error.to_string())?;
        let original_camera_euler_angles = api
            .invoke_value::<UnityVec3>(camera_transform, "get_eulerAngles", &mut [])
            .map_err(|error| error.to_string())?;
        let original_camera_orthographic = api
            .invoke_value::<bool>(camera, "get_orthographic", &mut [])
            .map_err(|error| error.to_string())?;
        let original_camera_orthographic_size = api
            .invoke_value::<f32>(camera, "get_orthographicSize", &mut [])
            .map_err(|error| error.to_string())?;
        let original_camera_field_of_view = api
            .invoke_value::<f32>(camera, "get_fieldOfView", &mut [])
            .map_err(|error| error.to_string())?;
        let original_camera_far_clip_plane = api
            .invoke_value::<f32>(camera, "get_farClipPlane", &mut [])
            .map_err(|error| error.to_string())?;
        let (controlled, original_controlled_enabled) = controlled_camera_behaviours(api)?;
        let destroy_immediate = destroy_immediate_method(api)? as usize;
        let [
            camera_handle,
            camera_transform_handle,
            brain,
            horizontal,
            orbit,
            zoom,
        ] = gc_handles(
            api,
            [
                camera,
                camera_transform,
                controlled[0],
                controlled[1],
                controlled[2],
                controlled[3],
            ],
        )?;
        let capture = Self {
            api,
            camera_handle,
            camera_transform_handle,
            controlled_handles: [brain, horizontal, orbit, zoom],
            original_controlled_enabled,
            texture_handle: 0,
            screen_class: screen_class as usize,
            application_class: application_class as usize,
            set_resolution: set_resolution as usize,
            destroy_immediate,
            width: 0,
            height: 0,
            original_screen_width,
            original_screen_height,
            original_fullscreen,
            original_target_frame_rate,
            original_camera_position,
            original_camera_euler_angles,
            original_camera_orthographic,
            original_camera_orthographic_size,
            original_camera_field_of_view,
            original_camera_far_clip_plane,
        };
        let setup = capture.configure();
        if let Err(error) = setup {
            return match capture.restore(true) {
                Ok(()) => Err(error),
                Err(restore) => Err(format!("{error}; cannot restore capture state: {restore}")),
            };
        }
        Ok(capture)
    }

    fn configure(&self) -> Result<(), String> {
        self.set_target_frame_rate(CAPTURE_FRAME_RATE)
            .and_then(|()| {
                self.set_screen_resolution(
                    i32::from(CAPTURE_WIDTH),
                    i32::from(CAPTURE_HEIGHT),
                    false,
                )
            })
            .and_then(|()| self.apply_calibration())
    }

    fn apply_calibration(&self) -> Result<(), String> {
        let mut disabled = false;
        for handle in self.controlled_handles {
            let controller = self
                .api
                .gc_handle_target(handle)
                .map_err(|error| error.to_string())?;
            self.api
                .invoke_void(controller, "set_enabled", &mut [argument(&mut disabled)])
                .map_err(|error| error.to_string())?;
        }
        let camera = self
            .api
            .gc_handle_target(self.camera_handle)
            .map_err(|error| error.to_string())?;
        let transform = self
            .api
            .gc_handle_target(self.camera_transform_handle)
            .map_err(|error| error.to_string())?;
        let mut position = UnityVec3 {
            x: 0.0,
            y: CALIBRATION_CAMERA_HEIGHT,
            z: CALIBRATION_CAMERA_Z,
        };
        let mut rotation = UnityVec3 {
            x: CALIBRATION_CAMERA_PITCH_DEGREES,
            y: 0.0,
            z: 0.0,
        };
        let mut orthographic = false;
        let mut field_of_view = CALIBRATION_FIELD_OF_VIEW_DEGREES;
        let mut far_clip_plane = 4_000.0_f32;
        self.api
            .invoke_void(transform, "set_position", &mut [argument(&mut position)])
            .and_then(|()| {
                self.api
                    .invoke_void(transform, "set_eulerAngles", &mut [argument(&mut rotation)])
            })
            .and_then(|()| {
                self.api.invoke_void(
                    camera,
                    "set_orthographic",
                    &mut [argument(&mut orthographic)],
                )
            })
            .and_then(|()| {
                self.api.invoke_void(
                    camera,
                    "set_fieldOfView",
                    &mut [argument(&mut field_of_view)],
                )
            })
            .and_then(|()| {
                self.api.invoke_void(
                    camera,
                    "set_farClipPlane",
                    &mut [argument(&mut far_clip_plane)],
                )
            })
            .map_err(|error| error.to_string())
    }

    /// Read the rendered pixels. Conversion and JPEG encoding deliberately
    /// happen on the consumer thread; doing them here blocked the render
    /// barrier and dominated per-frame cost.
    fn frame(&mut self) -> Result<RawFrame, String> {
        self.ensure_capture_texture()?;
        let texture = self
            .api
            .gc_handle_target(self.texture_handle)
            .map_err(|error| error.to_string())?;
        let mut source = UnityRect {
            x: 0.0,
            y: 0.0,
            width: f32::from(self.width),
            height: f32::from(self.height),
        };
        let mut destination_x = 0_i32;
        let mut destination_y = 0_i32;
        let mut recalculate_mipmaps = false;
        self.api
            .invoke_void(
                texture,
                "ReadPixels",
                &mut [
                    argument(&mut source),
                    argument(&mut destination_x),
                    argument(&mut destination_y),
                    argument(&mut recalculate_mipmaps),
                ],
            )
            .map_err(|error| error.to_string())?;
        let mut update_mipmaps = false;
        let mut make_no_longer_readable = false;
        self.api
            .invoke_void(
                texture,
                "Apply",
                &mut [
                    argument(&mut update_mipmaps),
                    argument(&mut make_no_longer_readable),
                ],
            )
            .map_err(|error| error.to_string())?;
        let raw = self
            .api
            .invoke(texture, "GetRawTextureData", &mut [])
            .and_then(|bytes| self.api.byte_array(bytes))
            .map_err(|error| error.to_string());
        let width = self.width;
        let height = self.height;
        let raw = raw?;
        let pixels = usize::from(width)
            .checked_mul(usize::from(height))
            .ok_or("captured image dimensions overflow")?;
        let channels = raw
            .len()
            .checked_div(pixels)
            .filter(|channels| pixels * channels == raw.len() && matches!(channels, 3 | 4))
            .ok_or_else(|| {
                format!(
                    "unsupported captured texture layout: {} bytes for {width}x{height}",
                    raw.len()
                )
            })?;
        Ok(RawFrame {
            pixels: raw,
            width,
            height,
            channels,
        })
    }

    fn restore(mut self, restore_camera: bool) -> Result<(), String> {
        let texture_result = self.destroy_texture();
        let camera_result = if restore_camera {
            self.restore_camera_and_controls()
        } else {
            Ok(())
        };
        let resolution_result = self.set_screen_resolution(
            self.original_screen_width,
            self.original_screen_height,
            self.original_fullscreen,
        );
        let frame_rate_result = self.set_target_frame_rate(self.original_target_frame_rate);
        self.api.free_gc_handle(self.camera_handle);
        self.api.free_gc_handle(self.camera_transform_handle);
        for handle in self.controlled_handles {
            self.api.free_gc_handle(handle);
        }
        if self.texture_handle != 0 {
            self.api.free_gc_handle(self.texture_handle);
        }
        self.texture_handle = 0;
        let failures: Vec<_> = [
            texture_result,
            camera_result,
            resolution_result,
            frame_rate_result,
        ]
        .into_iter()
        .filter_map(Result::err)
        .collect();
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    }

    fn destroy_texture(&self) -> Result<(), String> {
        if self.texture_handle == 0 {
            return Ok(());
        }
        let texture = self
            .api
            .gc_handle_target(self.texture_handle)
            .map_err(|error| error.to_string())?;
        let mut allow_destroying_assets = false;
        self.api
            .invoke_raw(
                self.destroy_immediate as *const MethodInfo,
                ptr::null_mut(),
                &mut [
                    object_argument(texture),
                    argument(&mut allow_destroying_assets),
                ],
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn ensure_capture_texture(&mut self) -> Result<(), String> {
        if self.texture_handle != 0 {
            return Ok(());
        }
        let screen_class = self.screen_class as *mut crate::il2cpp::Class;
        let width = self
            .api
            .invoke_static(screen_class, "get_width", &mut [])
            .and_then(|value| self.api.unbox::<i32>(value, "Screen.width"))
            .map_err(|error| error.to_string())?;
        let height = self
            .api
            .invoke_static(screen_class, "get_height", &mut [])
            .and_then(|value| self.api.unbox::<i32>(value, "Screen.height"))
            .map_err(|error| error.to_string())?;
        if (width, height) != (i32::from(CAPTURE_WIDTH), i32::from(CAPTURE_HEIGHT)) {
            return Err(format!(
                "capture resolution did not settle at {CAPTURE_WIDTH}x{CAPTURE_HEIGHT}: {width}x{height}"
            ));
        }
        let texture_class = self
            .api
            .class("UnityEngine.CoreModule.dll", "UnityEngine", "Texture2D")
            .map_err(|error| error.to_string())?;
        let texture =
            create_capture_texture(self.api, texture_class, CAPTURE_WIDTH, CAPTURE_HEIGHT)?;
        self.texture_handle = self
            .api
            .gc_handle(texture)
            .map_err(|error| error.to_string())?;
        self.width = CAPTURE_WIDTH;
        self.height = CAPTURE_HEIGHT;
        Ok(())
    }

    fn set_screen_resolution(
        &self,
        mut width: i32,
        mut height: i32,
        mut fullscreen: bool,
    ) -> Result<(), String> {
        self.api
            .invoke_raw(
                self.set_resolution as *const MethodInfo,
                ptr::null_mut(),
                &mut [
                    argument(&mut width),
                    argument(&mut height),
                    argument(&mut fullscreen),
                ],
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn set_target_frame_rate(&self, mut frame_rate: i32) -> Result<(), String> {
        self.api
            .invoke_static(
                self.application_class as *mut crate::il2cpp::Class,
                "set_targetFrameRate",
                &mut [argument(&mut frame_rate)],
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn restore_camera_and_controls(&self) -> Result<(), String> {
        let camera = self
            .api
            .gc_handle_target(self.camera_handle)
            .map_err(|error| error.to_string())?;
        let transform = self
            .api
            .gc_handle_target(self.camera_transform_handle)
            .map_err(|error| error.to_string())?;
        let mut position = self.original_camera_position;
        let mut rotation = self.original_camera_euler_angles;
        let mut orthographic = self.original_camera_orthographic;
        let mut orthographic_size = self.original_camera_orthographic_size;
        let mut field_of_view = self.original_camera_field_of_view;
        let mut far_clip_plane = self.original_camera_far_clip_plane;
        self.api
            .invoke_void(transform, "set_position", &mut [argument(&mut position)])
            .and_then(|()| {
                self.api
                    .invoke_void(transform, "set_eulerAngles", &mut [argument(&mut rotation)])
            })
            .and_then(|()| {
                self.api.invoke_void(
                    camera,
                    "set_orthographic",
                    &mut [argument(&mut orthographic)],
                )
            })
            .and_then(|()| {
                self.api.invoke_void(
                    camera,
                    "set_orthographicSize",
                    &mut [argument(&mut orthographic_size)],
                )
            })
            .and_then(|()| {
                self.api.invoke_void(
                    camera,
                    "set_fieldOfView",
                    &mut [argument(&mut field_of_view)],
                )
            })
            .and_then(|()| {
                self.api.invoke_void(
                    camera,
                    "set_farClipPlane",
                    &mut [argument(&mut far_clip_plane)],
                )
            })
            .map_err(|error| error.to_string())?;
        for (index, handle) in self.controlled_handles.iter().copied().enumerate() {
            let controller = self
                .api
                .gc_handle_target(handle)
                .map_err(|error| error.to_string())?;
            let mut enabled = self.original_controlled_enabled[index];
            self.api
                .invoke_void(controller, "set_enabled", &mut [argument(&mut enabled)])
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

fn controlled_camera_behaviours(api: Api) -> Result<([*mut Object; 4], [bool; 4]), String> {
    let manual_camera_class = api
        .class("GRClient.dll", "GameRiver.Client", "GROverAllManualCam")
        .map_err(|error| error.to_string())?;
    let manual_camera = api
        .find_object_of_class(manual_camera_class)
        .map_err(|error| error.to_string())?;
    let horizontal = api
        .invoke(
            manual_camera,
            "GetHorizontalCameraMovementController",
            &mut [],
        )
        .map_err(|error| error.to_string())?;
    let orbit = api
        .invoke(manual_camera, "GetOrbitCameraController", &mut [])
        .map_err(|error| error.to_string())?;
    let zoom = api
        .invoke(manual_camera, "GetZoomCameraController", &mut [])
        .map_err(|error| error.to_string())?;
    let brain_class = api
        .class("Cinemachine.dll", "Cinemachine", "CinemachineBrain")
        .map_err(|error| error.to_string())?;
    let brain = api
        .find_object_of_class(brain_class)
        .map_err(|error| error.to_string())?;
    let controlled = [brain, horizontal, orbit, zoom];
    if controlled.iter().any(|object| object.is_null()) {
        return Err("one or more native camera controllers are unavailable".into());
    }
    let mut enabled = [false; 4];
    for (index, object) in controlled.iter().copied().enumerate() {
        enabled[index] = api
            .invoke_value::<bool>(object, "get_enabled", &mut [])
            .map_err(|error| error.to_string())?;
    }
    Ok((controlled, enabled))
}

fn destroy_immediate_method(api: Api) -> Result<*const MethodInfo, String> {
    let object = api
        .class("UnityEngine.CoreModule.dll", "UnityEngine", "Object")
        .map_err(|error| error.to_string())?;
    api.method(object, "DestroyImmediate", 2)
        .map_err(|error| error.to_string())
}

fn gc_handles<const N: usize>(api: Api, objects: [*mut Object; N]) -> Result<[u32; N], String> {
    let mut handles = [0_u32; N];
    for (index, object) in objects.into_iter().enumerate() {
        match api.gc_handle(object) {
            Ok(handle) => handles[index] = handle,
            Err(error) => {
                for handle in handles {
                    api.free_gc_handle(handle);
                }
                return Err(error.to_string());
            }
        }
    }
    Ok(handles)
}

fn screen_state(api: Api, screen: *mut crate::il2cpp::Class) -> Result<(i32, i32, bool), String> {
    let width = api
        .invoke_static(screen, "get_width", &mut [])
        .and_then(|value| api.unbox::<i32>(value, "Screen.width"))
        .map_err(|error| error.to_string())?;
    let height = api
        .invoke_static(screen, "get_height", &mut [])
        .and_then(|value| api.unbox::<i32>(value, "Screen.height"))
        .map_err(|error| error.to_string())?;
    let fullscreen = api
        .invoke_static(screen, "get_fullScreen", &mut [])
        .and_then(|value| api.unbox::<bool>(value, "Screen.fullScreen"))
        .map_err(|error| error.to_string())?;
    Ok((width, height, fullscreen))
}

fn create_capture_texture(
    api: Api,
    texture_class: *mut crate::il2cpp::Class,
    width: u16,
    height: u16,
) -> Result<*mut Object, String> {
    let texture = api
        .allocate_object(texture_class)
        .map_err(|error| error.to_string())?;
    let constructor = api
        .method_with_parameter_types(
            texture,
            ".ctor",
            &[
                "System.Int32",
                "System.Int32",
                "UnityEngine.TextureFormat",
                "System.Boolean",
            ],
        )
        .map_err(|error| error.to_string())?;
    let mut texture_width = i32::from(width);
    let mut texture_height = i32::from(height);
    let mut rgb24 = 3_i32;
    let mut mip_chain = false;
    api.invoke_raw(
        constructor,
        texture.cast(),
        &mut [
            argument(&mut texture_width),
            argument(&mut texture_height),
            argument(&mut rgb24),
            argument(&mut mip_chain),
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(texture)
}

static CAPTURE: OnceLock<Mutex<CaptureState>> = OnceLock::new();
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_UPDATE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_MATCH_UPDATE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_PLAYER_FINISH_DEPLOY: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_POST_RENDER: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_PROJECTILE_CREATE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_PROJECTILE_ADD: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_PROJECTILE_DESTROY: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_DAMAGE_PERFORM: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_FIGHT_ACTOR_REDUCE_LIFE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_FIGHT_CONTROLLER_ON_ACTOR_HITTED: AtomicPtr<c_void> =
    AtomicPtr::new(ptr::null_mut());
static ORIGINAL_ADVANCED_SHIELD_DAMAGE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_FIGHT_MECH_ON_DEAD: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_FIGHT_CRYSTAL_ON_DEAD: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_RVO_CONTROLLER_ACTIVE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_RVO_ADD_AGENT_FIXED: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_RVO_FIXED_UPDATE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_RVO_PRE_CALCULATION: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_RVO_CALCULATE_NEIGHBOURS: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_RVO_GENERATE_NEIGHBOUR_VOS: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_RVO_GENERATE_OPPONENT_VOS: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_SELECTOR_CALCULATE_SCORE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static RVO_UPDATE_ORDINAL: AtomicU64 = AtomicU64::new(0);
static RVO_SOURCE_CALL_ORDINAL: AtomicU64 = AtomicU64::new(0);
static RVO_VO_CALL_ORDINAL: AtomicU64 = AtomicU64::new(0);
static ACTIVE_RVO_UPDATE: AtomicU64 = AtomicU64::new(u64::MAX);
static CURRENT_RVO_FIXED_UPDATE: AtomicU64 = AtomicU64::new(u64::MAX);
static CURRENT_RVO_ACTIVATION_COUNT: AtomicU64 = AtomicU64::new(0);
static RVO_ACTIVATION_COUNT: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static ACTIVE_RVO_CONTROLLER: Cell<usize> = const { Cell::new(0) };
    static ACTIVE_PROJECTILE_CHANNEL: Cell<Option<(usize, i32)>> = const { Cell::new(None) };
    static ACTIVE_DAMAGE_CONTEXT: Cell<Option<DamageContext>> = const { Cell::new(None) };
}

#[derive(Clone, Copy, Default)]
struct DamageContext {
    source: Option<ObjectRef>,
    source_team_id: Option<u32>,
    provider: Option<ObjectRef>,
}

#[derive(Clone, Copy, Default)]
struct DamageAttribution {
    source: Option<ObjectRef>,
    source_team_id: Option<u32>,
}

enum NativeTrace {
    ProjectileReleased {
        projectile_id: u64,
        owner: usize,
        target: usize,
        skill_slot: Option<u16>,
        weapon_index: Option<i32>,
    },
    ProjectileRemoved {
        projectile_id: u64,
        owner: usize,
        target: usize,
        position: QVec3,
        intercepted: bool,
        absorbed_by: Option<ObjectRef>,
    },
    Damage {
        source: Option<ObjectRef>,
        source_team_id: Option<u32>,
        target: ObjectRef,
        amount: i32,
    },
    ShieldCreated {
        shield_id: u64,
        team_id: u32,
        source_kind: ShieldSourceKind,
        position: QVec3,
    },
    ShieldDestroyed {
        shield_id: u64,
        position: QVec3,
    },
    TerrainCreated {
        terrain_id: u64,
        team_id: Option<u32>,
        terrain_type: TerrainType,
        position: QVec3,
        radius: i64,
    },
    TerrainRemoved {
        terrain_id: u64,
        position: QVec3,
    },
    UnitDied {
        unit_id: u64,
        position: QVec3,
        source: Option<ObjectRef>,
        source_team_id: Option<u32>,
    },
    BuildingDestroyed {
        building_id: u64,
        position: QVec3,
    },
}

fn capture_state() -> &'static Mutex<CaptureState> {
    CAPTURE.get_or_init(|| Mutex::new(CaptureState::default()))
}

pub(crate) fn initialize(runtime: &mut Runtime) {
    RUNTIME.store(ptr::from_mut(runtime), Ordering::Release);
    let result = initialize_inner(runtime);
    let mut state = capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    state.rvo_agent_owners.clear();
    match result {
        Ok(metadata) => {
            state.metadata = metadata;
            state.availability = None;
        }
        Err(error) => state.availability = Some(error),
    }
}

#[allow(clippy::too_many_lines)]
fn initialize_inner(runtime: &Runtime) -> Result<Metadata, String> {
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    return Err("logic-tick recording is supported only by the macOS aarch64 adapter".into());

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        let api = runtime.api;
        let fight = api
            .class("GRFight.dll", "GameRiver.Fight", "FightController")
            .map_err(|error| error.to_string())?;
        let update = api
            .method(fight, "Update", 0)
            .map_err(|error| error.to_string())?;
        let match_client = api
            .class("GRClient.dll", "GameRiver.Client", "MatchClient")
            .map_err(|error| error.to_string())?;
        let match_update = api
            .method(match_client, "Update", 0)
            .map_err(|error| error.to_string())?;
        let player_finish_deploy = api
            .class("GRCore.dll", "GameRiver", "PlayerController")
            .and_then(|class| api.method(class, "FinishDeploy", 0))
            .map_err(|error| error.to_string())?;
        let camera = api
            .class("UnityEngine.CoreModule.dll", "UnityEngine", "Camera")
            .map_err(|error| error.to_string())?;
        let post_render = api
            .method(camera, "FireOnPostRender", 1)
            .map_err(|error| error.to_string())?;
        let projectile_system = api
            .class("GRFight.dll", "GameRiver.Fight", "ProjectileSystem")
            .map_err(|error| error.to_string())?;
        let range_item_system = api
            .class("GRFight.dll", "GameRiver.Fight", "RangeItemSystem")
            .map_err(|error| error.to_string())?;
        let range_item_controller = api
            .class("GRFight.dll", "GameRiver.Fight", "RangeItemController")
            .map_err(|error| error.to_string())?;
        let range_item = api
            .class("GRFight.dll", "GameRiver.Fight", "RangeItem")
            .map_err(|error| error.to_string())?;
        let fight_ground_fire = api
            .class("GRFight.dll", "GameRiver.Fight", "FightGroundFire")
            .map_err(|error| error.to_string())?;
        let grid_block = api
            .class("GRFight.dll", "GameRiver.Fight", "GridBlockInt")
            .map_err(|error| error.to_string())?;
        let advanced_energy_shield_system = api
            .class(
                "GRFight.dll",
                "GameRiver.Fight",
                "AdvancedEnergyShieldSystem",
            )
            .map_err(|error| error.to_string())?;
        let energy_shield_contraption = api
            .class("GRCore.dll", "GameRiver", "EnergyShieldContraption")
            .map_err(|error| error.to_string())?;
        let commander_energy_shield = api
            .class("GRCore.dll", "GameRiver", "CS_EnergyShield")
            .map_err(|error| error.to_string())?;
        let owner_advanced_shield = api
            .class(
                "GRFight.dll",
                "GameRiver.Fight",
                "AdvancedEnergyShieldController",
            )
            .map_err(|error| error.to_string())?;
        let spawned_temporary_shield = api
            .class(
                "GRFight.dll",
                "GameRiver.Fight",
                "SpawnAdvancedShieldController",
            )
            .map_err(|error| error.to_string())?;
        let fight_team = api
            .class("GRFight.dll", "GameRiver.Fight", "FightTeam")
            .map_err(|error| error.to_string())?;
        let fight_team_buildings = api
            .field(fight_team, "buildings")
            .map_err(|error| error.to_string())?;
        let fight_team_constructions = api
            .field(fight_team, "constructions")
            .map_err(|error| error.to_string())?;
        let projectile_controllers = api
            .field(projectile_system, "projectileControllers")
            .map_err(|error| error.to_string())?;
        let projectile_in_energy_shields = api
            .class("GRFight.dll", "GameRiver.Fight", "ProjectileController")
            .and_then(|class| api.field(class, "inEnergyShields"))
            .map_err(|error| error.to_string())?;
        let range_item_affected_units = api
            .field(range_item_controller, "affectedUnits")
            .map_err(|error| error.to_string())?;
        let range_item_affected_unit_times = api
            .field(range_item_controller, "affectedUnitTimes")
            .map_err(|error| error.to_string())?;
        let range_item_effect_time_duration = api
            .field(range_item_controller, "effectTimeDuration")
            .map_err(|error| error.to_string())?;
        let range_item_time = api
            .field(range_item, "time")
            .map_err(|error| error.to_string())?;
        let range_item_life_time = api
            .field(range_item, "lifeTime")
            .map_err(|error| error.to_string())?;
        let ground_fire_time = api
            .field(fight_ground_fire, "time")
            .map_err(|error| error.to_string())?;
        let ground_fire_life_time = api
            .field(fight_ground_fire, "lifeTime")
            .map_err(|error| error.to_string())?;
        let grid_position_x = api
            .field(grid_block, "positionX")
            .map_err(|error| error.to_string())?;
        let grid_position_y = api
            .field(grid_block, "positionY")
            .map_err(|error| error.to_string())?;
        let grid_rows = api
            .field(grid_block, "grids")
            .map_err(|error| error.to_string())?;
        let grid_size = api
            .field(grid_block, "<Size>k__BackingField")
            .map_err(|error| error.to_string())?;
        let motion_controller = api
            .class("GRFight.dll", "GameRiver.Fight", "MotionController")
            .map_err(|error| error.to_string())?;
        let motion_fsm = api
            .field(motion_controller, "fsm")
            .map_err(|error| error.to_string())?;
        let motion_idle_state = api
            .class("GRFight.dll", "GameRiver.Fight", "MotionIdleState")
            .map_err(|error| error.to_string())?;
        let motion_move_state = api
            .class("GRFight.dll", "GameRiver.Fight", "MotionMoveState")
            .map_err(|error| error.to_string())?;
        let motion_attack_state = api
            .class("GRFight.dll", "GameRiver.Fight", "MotionAttackState")
            .map_err(|error| error.to_string())?;
        let motion_stop_state = api
            .class("GRFight.dll", "GameRiver.Fight", "MotionStopState")
            .map_err(|error| error.to_string())?;
        let fight_mech_lock_target = api
            .class("GRFight.dll", "GameRiver.Fight", "FightMech")
            .and_then(|class| api.field(class, "lockTarget"))
            .map_err(|error| error.to_string())? as usize;
        let fight_skill = api
            .class("GRFight.dll", "GameRiver.Fight", "FightSkill")
            .ok();
        let fight_skill_lock_target = fight_skill
            .and_then(|class| api.field(class, "lockTarget").ok())
            .map(|field| field as usize);
        let fight_skill_attack_target = fight_skill
            .and_then(|class| api.field(class, "attackTarget").ok())
            .map(|field| field as usize);
        let damage_performer = api
            .class("GRFight.dll", "GameRiver.Fight", "DamagePerformer")
            .map_err(|error| error.to_string())?;
        let fight_actor = api
            .class("GRFight.dll", "GameRiver.Fight", "FightActor")
            .map_err(|error| error.to_string())?;
        let projectile_add = api
            .method(projectile_system, "AddProjectile", 1)
            .map_err(|error| error.to_string())?;
        let projectile_create = api
            .method(projectile_system, "Create", 5)
            .map_err(|error| error.to_string())?;
        let projectile_destroy = api
            .method(projectile_system, "Destroy", 2)
            .map_err(|error| error.to_string())?;
        let damage_perform = api
            .method(damage_performer, "Perform", 3)
            .map_err(|error| error.to_string())?;
        let fight_actor_reduce_life = api
            .method(fight_actor, "ReduceLife", 1)
            .map_err(|error| error.to_string())?;
        let fight_controller_on_actor_hitted = api
            .method(fight, "OnActorHitted", 1)
            .map_err(|error| error.to_string())?;
        let advanced_shield_damage = api
            .method(damage_performer, "PerformHitAdvancedEndergyShieldEffect", 4)
            .map_err(|error| error.to_string())?;
        let fight_mech_on_dead = api
            .class("GRFight.dll", "GameRiver.Fight", "FightMech")
            .and_then(|class| api.method(class, "OnDead", 0))
            .map_err(|error| error.to_string())?;
        let fight_crystal_on_dead = api
            .class("GRFight.dll", "GameRiver.Fight", "FightCrystal")
            .and_then(|class| api.method(class, "OnDead", 0))
            .map_err(|error| error.to_string())?;
        install_projectile_create_hook(api, projectile_create)?;
        install_projectile_add_hook(api, projectile_add)?;
        install_projectile_destroy_hook(api, projectile_destroy)?;
        install_damage_perform_hook(api, damage_perform)?;
        install_fight_actor_reduce_life_hook(api, fight_actor_reduce_life)?;
        install_fight_controller_on_actor_hitted_hook(api, fight_controller_on_actor_hitted)?;
        install_advanced_shield_damage_hook(api, advanced_shield_damage)?;
        install_fight_mech_on_dead_hook(api, fight_mech_on_dead)?;
        install_fight_crystal_on_dead_hook(api, fight_crystal_on_dead)?;
        install_update_hook(api, update)?;
        install_match_update_hook(api, match_update)?;
        install_player_finish_deploy_hook(api, player_finish_deploy)?;
        install_post_render_hook(api, post_render)?;
        let (rvo, rvo_error) = match initialize_rvo_instrumentation(api) {
            Ok(metadata) => (Some(metadata), None),
            Err(error) => (None, Some(error)),
        };
        let (selector_score_available, selector_score_error) =
            match initialize_selector_score_instrumentation(api) {
                Ok(()) => (true, None),
                Err(error) => (false, Some(error)),
            };
        Ok(Metadata {
            projectile_system_class: projectile_system as usize,
            range_item_system_class: range_item_system as usize,
            fight_ground_fire_class: fight_ground_fire as usize,
            advanced_energy_shield_system_class: advanced_energy_shield_system as usize,
            energy_shield_contraption_class: energy_shield_contraption as usize,
            commander_energy_shield_class: commander_energy_shield as usize,
            owner_advanced_shield_class: owner_advanced_shield as usize,
            spawned_temporary_shield_class: spawned_temporary_shield as usize,
            fight_team_buildings: fight_team_buildings as usize,
            fight_team_constructions: fight_team_constructions as usize,
            projectile_controllers: projectile_controllers as usize,
            projectile_in_energy_shields: projectile_in_energy_shields as usize,
            range_item_affected_units: range_item_affected_units as usize,
            range_item_affected_unit_times: range_item_affected_unit_times as usize,
            range_item_effect_time_duration: range_item_effect_time_duration as usize,
            range_item_time: range_item_time as usize,
            range_item_life_time: range_item_life_time as usize,
            ground_fire_time: ground_fire_time as usize,
            ground_fire_life_time: ground_fire_life_time as usize,
            grid_position_x: grid_position_x as usize,
            grid_position_y: grid_position_y as usize,
            grid_rows: grid_rows as usize,
            grid_size: grid_size as usize,
            motion_fsm: motion_fsm as usize,
            motion_idle_state_class: motion_idle_state as usize,
            motion_move_state_class: motion_move_state as usize,
            motion_attack_state_class: motion_attack_state as usize,
            motion_stop_state_class: motion_stop_state as usize,
            fight_mech_lock_target,
            fight_skill_class: fight_skill.map(|class| class as usize),
            fight_skill_lock_target,
            fight_skill_attack_target,
            rvo,
            rvo_error,
            selector_score_available,
            selector_score_error,
        })
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn initialize_selector_score_instrumentation(api: Api) -> Result<(), String> {
    let score_selector = api
        .class(
            "GRFight.dll",
            "GameRiver.Fight",
            "ScoreRatingTargetSelector",
        )
        .map_err(|error| error.to_string())?;
    let calculate_score = api
        .method(score_selector, "CalculateScore", 9)
        .map_err(|error| error.to_string())?;
    install_selector_calculate_score_hook(api, calculate_score)
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn initialize_rvo_instrumentation(api: Api) -> Result<RvoMetadata, String> {
    let fight_actor = api
        .class("GRFight.dll", "GameRiver.Fight", "FightActor")
        .map_err(|error| error.to_string())?;
    let fight_actor_rvo_controller = api
        .field(fight_actor, "rvoController")
        .map_err(|error| error.to_string())?;
    let rvo_controller = api
        .class("GRFight.dll", "GameRiver.Fight.GRPF.RVO", "RVOController")
        .map_err(|error| error.to_string())?;
    let rvo_controller_agent = api
        .field(rvo_controller, "<rvoAgent>k__BackingField")
        .map_err(|error| error.to_string())?;
    let rvo_controller_fixed = api
        .class("GRFight.dll", "GameRiver.Fight", "RVOControllerFixed")
        .map_err(|error| error.to_string())?;
    let rvo_controller_owner = api
        .field(rvo_controller_fixed, "owner")
        .map_err(|error| error.to_string())?;
    let rvo_controller_active = api
        .method(rvo_controller_fixed, "Active", 0)
        .map_err(|error| error.to_string())?;
    let simulator = api
        .class("GRFight.dll", "GameRiver.Fight.GRPF.RVO", "Simulator")
        .map_err(|error| error.to_string())?;
    let simulator_double_buffering = api
        .field(simulator, "doubleBuffering")
        .map_err(|error| error.to_string())?;
    let simulator_symmetry_breaking_bias = api
        .field(simulator, "symmetryBreakingBias")
        .map_err(|error| error.to_string())?;
    let simulator_workers = api
        .field(simulator, "workers")
        .map_err(|error| error.to_string())?;
    let simulator_agents = api
        .field(simulator, "agents")
        .map_err(|error| error.to_string())?;
    let fixed_update = api
        .method(simulator, "FixedUpdate", 0)
        .map_err(|error| error.to_string())?;
    let pre_calculation = api
        .method(simulator, "PreCalculation", 0)
        .map_err(|error| error.to_string())?;
    let add_agent_fixed = api
        .method(simulator, "AddAgentFixed", 1)
        .map_err(|error| error.to_string())?;
    let agent = api
        .class("GRFight.dll", "GameRiver.Fight.GRPF.RVO.Sampled", "Agent")
        .map_err(|error| error.to_string())?;
    let agent_neighbour_count = api
        .field(agent, "<NeighbourCount>k__BackingField")
        .map_err(|error| error.to_string())?;
    let agent_radius_inner = api
        .field(agent, "radiusInner")
        .map_err(|error| error.to_string())?;
    let agent_size = api
        .field(agent, "<Size>k__BackingField")
        .map_err(|error| error.to_string())?;
    let agent_radius_outer = api
        .field(agent, "radius")
        .map_err(|error| error.to_string())?;
    let agent_max_speed = api
        .field(agent, "maxSpeed")
        .map_err(|error| error.to_string())?;
    let agent_desired_speed = api
        .field(agent, "desiredSpeed")
        .map_err(|error| error.to_string())?;
    let agent_time_horizon = api
        .field(agent, "agentTimeHorizon")
        .map_err(|error| error.to_string())?;
    let agent_priority = api
        .field(agent, "<Priority>k__BackingField")
        .map_err(|error| error.to_string())?;
    let agent_published_calculated_speed = api
        .field(agent, "<CalculatedSpeed>k__BackingField")
        .map_err(|error| error.to_string())?;
    let agent_current_velocity = api
        .field(agent, "currentVelocity")
        .map_err(|error| error.to_string())?;
    let agent_desired_velocity = api
        .field(agent, "desiredVelocity")
        .map_err(|error| error.to_string())?;
    let agent_desired_target = api
        .field(agent, "desiredTargetPointInVelocitySpace")
        .map_err(|error| error.to_string())?;
    let agent_calculated_target = api
        .field(agent, "<CalculatedTargetPoint>k__BackingField")
        .map_err(|error| error.to_string())?;
    let agent_locked = api
        .field(agent, "locked")
        .map_err(|error| error.to_string())?;
    let agent_layer = api
        .field(agent, "layer")
        .map_err(|error| error.to_string())?;
    let agent_collides_with = api
        .field(agent, "collidesWith")
        .map_err(|error| error.to_string())?;
    let agent_internal_max_neighbours = api
        .field(agent, "maxNeighbours")
        .map_err(|error| error.to_string())?;
    let agent_position = api
        .field(agent, "position")
        .map_err(|error| error.to_string())?;
    let agent_simulator = api
        .field(agent, "simulator")
        .map_err(|error| error.to_string())?;
    let agent_max_neighbours = api
        .field(agent, "<MaxNeighbours>k__BackingField")
        .map_err(|error| error.to_string())?;
    let agent_neighbours = api
        .field(agent, "neighbours")
        .map_err(|error| error.to_string())?;
    let agent_neighbour_dists = api
        .field(agent, "neighbourDists")
        .map_err(|error| error.to_string())?;
    let calculate_neighbours = api
        .method(agent, "CalculateNeighbours", 0)
        .map_err(|error| error.to_string())?;
    let rvo_agent = api
        .class(
            "GRFight.dll",
            "GameRiver.Fight.GRPF.RVO.Sampled",
            "RVOAgentFixed",
        )
        .map_err(|error| error.to_string())?;
    let rvo_agent_main_layer = api
        .field(rvo_agent, "mainLayer")
        .map_err(|error| error.to_string())?;
    let rvo_agent_sync_main_layer = api
        .field(rvo_agent, "sync_mainLayer")
        .map_err(|error| error.to_string())?;
    let rvo_agent_group = api
        .field(rvo_agent, "group")
        .map_err(|error| error.to_string())?;
    let rvo_agent_sync_group = api
        .field(rvo_agent, "sync_group")
        .map_err(|error| error.to_string())?;
    let rvo_agent_ignore_same_group = api
        .field(rvo_agent, "ignoreSameGroup")
        .map_err(|error| error.to_string())?;
    let rvo_agent_sync_ignore_same_group = api
        .field(rvo_agent, "sync_ignoreSameGroup")
        .map_err(|error| error.to_string())?;
    let rvo_agent_team = api
        .field(rvo_agent, "team")
        .map_err(|error| error.to_string())?;
    let rvo_agent_sync_team = api
        .field(rvo_agent, "sync_team")
        .map_err(|error| error.to_string())?;
    let generate_neighbour_vos = api
        .method(rvo_agent, "GenerateNeighbourAgentVOs", 1)
        .map_err(|error| error.to_string())?;
    let generate_opponent_vos = api
        .method(rvo_agent, "GenerateOpponentVOs", 2)
        .map_err(|error| error.to_string())?;
    let vo_buffer = api
        .class(
            "GRFight.dll",
            "GameRiver.Fight.GRPF.RVO.Sampled",
            "Agent/VOBuffer",
        )
        .map_err(|error| error.to_string())?;
    let vo_buffer_buffer = api
        .field(vo_buffer, "buffer")
        .map_err(|error| error.to_string())?;
    let vo_buffer_length = api
        .field(vo_buffer, "length")
        .map_err(|error| error.to_string())?;
    run_rvo_hook_install_sequence(|index| match index {
        0 => install_rvo_controller_active_hook(api, rvo_controller_active),
        1 => install_rvo_add_agent_fixed_hook(api, add_agent_fixed),
        2 => install_rvo_fixed_update_hook(api, fixed_update),
        3 => install_rvo_pre_calculation_hook(api, pre_calculation),
        4 => install_rvo_calculate_neighbours_hook(api, calculate_neighbours),
        5 => install_rvo_generate_neighbour_vos_hook(api, generate_neighbour_vos),
        6 => install_rvo_generate_opponent_vos_hook(api, generate_opponent_vos),
        _ => unreachable!("RVO hook sequence has exactly seven entries"),
    })?;
    Ok(RvoMetadata {
        fight_actor_rvo_controller: fight_actor_rvo_controller as usize,
        rvo_controller_agent: rvo_controller_agent as usize,
        rvo_controller_owner: rvo_controller_owner as usize,
        simulator_double_buffering: simulator_double_buffering as usize,
        simulator_symmetry_breaking_bias: simulator_symmetry_breaking_bias as usize,
        simulator_workers: simulator_workers as usize,
        simulator_agents: simulator_agents as usize,
        agent_radius_inner: agent_radius_inner as usize,
        agent_size: agent_size as usize,
        agent_radius_outer: agent_radius_outer as usize,
        agent_max_speed: agent_max_speed as usize,
        agent_desired_speed: agent_desired_speed as usize,
        agent_time_horizon: agent_time_horizon as usize,
        agent_priority: agent_priority as usize,
        agent_published_calculated_speed: agent_published_calculated_speed as usize,
        agent_current_velocity: agent_current_velocity as usize,
        agent_desired_velocity: agent_desired_velocity as usize,
        agent_desired_target: agent_desired_target as usize,
        agent_calculated_target: agent_calculated_target as usize,
        agent_locked: agent_locked as usize,
        agent_layer: agent_layer as usize,
        agent_collides_with: agent_collides_with as usize,
        agent_internal_max_neighbours: agent_internal_max_neighbours as usize,
        agent_position: agent_position as usize,
        agent_simulator: agent_simulator as usize,
        rvo_agent_class: rvo_agent as usize,
        rvo_agent_main_layer: rvo_agent_main_layer as usize,
        rvo_agent_sync_main_layer: rvo_agent_sync_main_layer as usize,
        rvo_agent_group: rvo_agent_group as usize,
        rvo_agent_sync_group: rvo_agent_sync_group as usize,
        rvo_agent_ignore_same_group: rvo_agent_ignore_same_group as usize,
        rvo_agent_sync_ignore_same_group: rvo_agent_sync_ignore_same_group as usize,
        rvo_agent_team: rvo_agent_team as usize,
        rvo_agent_sync_team: rvo_agent_sync_team as usize,
        agent_max_neighbours: agent_max_neighbours as usize,
        agent_neighbour_count: agent_neighbour_count as usize,
        agent_neighbours: agent_neighbours as usize,
        agent_neighbour_dists: agent_neighbour_dists as usize,
        vo_buffer: vo_buffer_buffer as usize,
        vo_buffer_length: vo_buffer_length as usize,
    })
}

pub(crate) fn start(
    runtime: &Runtime,
    mode: CaptureStartMode,
    visual: bool,
    speed_up: bool,
    instrumentation_profile: Option<CaptureInstrumentationProfile>,
    rvo_scope: Option<RvoCaptureScope>,
) -> Result<(), String> {
    let mut state = capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(error) = &state.availability {
        return Err(format!("native capture is unavailable: {error}"));
    }
    if state.armed {
        return Err("a battle recording is already active".into());
    }
    validate_checker_profile_start(instrumentation_profile)?;
    if let Some(scope) = &rvo_scope {
        scope.validate(instrumentation_profile.ok_or("rvo_scope requires instrumentation")?)?;
    }
    if instrumentation_profile.is_some_and(CaptureInstrumentationProfile::includes_target_refs)
        && (state.metadata.fight_skill_class.is_none()
            || state.metadata.fight_skill_lock_target.is_none()
            || state.metadata.fight_skill_attack_target.is_none())
    {
        return Err(
            "target-reference instrumentation is unavailable because native target fields could not be resolved"
                .into(),
        );
    }
    validate_rvo_profile_availability(instrumentation_profile, &state.metadata)?;
    validate_selector_score_profile_availability(instrumentation_profile, &state.metadata)?;
    let fight = runtime.current_fight();
    if fight.is_null() {
        return Err("fight controller is unavailable".into());
    }
    let deploying = runtime
        .api
        .invoke_value::<bool>(fight, "IsDeploying", &mut [])
        .map_err(|error| error.to_string())?;
    let fighting = runtime
        .api
        .invoke_value::<bool>(fight, "IsFighting", &mut [])
        .map_err(|error| error.to_string())?;
    if !deploying || fighting {
        return Err("recording requires deployment before fighting".into());
    }
    let current_match = runtime.current_match();
    if current_match.is_null() {
        return Err("active match disappeared before recording started".into());
    }
    let layout_yaml = match mode {
        CaptureStartMode::TrainingGround => {
            let (_, context) = recording_context(runtime)?;
            Some(read_native_layout(runtime, &context, &state.metadata)?)
        }
        CaptureStartMode::Replay => None,
    };
    state.reset_session();
    state.deployment_layout_yaml = layout_yaml;
    state.speed_up_allowed = speed_up;
    state.await_replay_deployment = mode == CaptureStartMode::Replay;
    RVO_UPDATE_ORDINAL.store(0, Ordering::Release);
    RVO_SOURCE_CALL_ORDINAL.store(0, Ordering::Release);
    RVO_VO_CALL_ORDINAL.store(0, Ordering::Release);
    ACTIVE_RVO_UPDATE.store(u64::MAX, Ordering::Release);
    CURRENT_RVO_FIXED_UPDATE.store(u64::MAX, Ordering::Release);
    CURRENT_RVO_ACTIVATION_COUNT.store(0, Ordering::Release);
    RVO_ACTIVATION_COUNT.store(0, Ordering::Release);
    state.instrumentation_profile = instrumentation_profile;
    state.rvo_scope = rvo_scope;
    if visual {
        state.visual = Some(VisualCapture::new(runtime)?);
    }
    state.armed = true;
    drop(state);

    if mode == CaptureStartMode::TrainingGround
        && let Err(error) = runtime
            .api
            .invoke_void(current_match, "ChangeProcessState", &mut [])
    {
        let message = format!("cannot start fight: {error}");
        abort(&message);
        if let Err(restore_error) = stop() {
            return Err(format!("{message}; cannot restore camera: {restore_error}"));
        }
        return Err(message);
    }
    Ok(())
}

fn validate_checker_profile_start(
    instrumentation_profile: Option<CaptureInstrumentationProfile>,
) -> Result<(), String> {
    if instrumentation_profile
        .is_some_and(CaptureInstrumentationProfile::includes_skill_attackable_checker)
    {
        Err(
            "skill_attackable_checker_v1 is unsupported: build2259 Check prologue and a safe instruction-relocation contract require independent evidence"
                .into(),
        )
    } else {
        Ok(())
    }
}

fn validate_rvo_profile_availability(
    instrumentation_profile: Option<CaptureInstrumentationProfile>,
    metadata: &Metadata,
) -> Result<(), String> {
    if let Some(profile) =
        instrumentation_profile.filter(|profile| profile.includes_rvo() && metadata.rvo.is_none())
    {
        Err(format!(
            "{} is unavailable: {}",
            profile.as_str(),
            metadata
                .rvo_error
                .as_deref()
                .unwrap_or("native RVO fields or hooks could not be resolved")
        ))
    } else {
        Ok(())
    }
}

fn validate_selector_score_profile_availability(
    instrumentation_profile: Option<CaptureInstrumentationProfile>,
    metadata: &Metadata,
) -> Result<(), String> {
    if let Some(profile) = instrumentation_profile
        .filter(|profile| profile.includes_selector_score() && !metadata.selector_score_available)
    {
        Err(format!(
            "{} is unavailable: {}",
            profile.as_str(),
            metadata
                .selector_score_error
                .as_deref()
                .unwrap_or("native selector method or hook could not be resolved")
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn stop() -> Result<(), String> {
    let mut state = capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    state.armed = false;
    state.pending_visual = None;
    state.traces.clear();
    clear_pending_rvo_state(&mut state);
    let checker_error = reject_pending_checker_calls(&mut state, "explicit stop").err();
    reset_rvo_sentinels();
    let visual = state.visual.take();
    drop(state);
    if let Some(visual) = visual {
        visual.restore(true)?;
    }
    checker_error.map_or(Ok(()), Err)
}

fn reject_pending_checker_calls(state: &mut CaptureState, boundary: &str) -> Result<(), String> {
    if state.open_checker_calls.is_empty() && state.completed_checker_calls.is_empty() {
        return Ok(());
    }
    let error = format!(
        "checker probe {boundary} has {} open and {} undrained calls",
        state.open_checker_calls.len(),
        state.completed_checker_calls.len()
    );
    state.open_checker_calls.clear();
    state.completed_checker_calls.clear();
    Err(error)
}

fn clear_pending_rvo_state(state: &mut CaptureState) {
    state.rvo_neighbour_sets.clear();
    state.rvo_agent_sets.clear();
    state.rvo_published_agent_sets.clear();
    state.rvo_vo_buffers.clear();
    state.opponent_vos.clear();
    state.rvo_update_modes.clear();
    state.rvo_update_symmetry_breaking_biases.clear();
    state.rvo_update_start_native_ticks.clear();
    state.rvo_update_publish_native_ticks.clear();
    state.rvo_update_multithreaded.clear();
}

fn reset_rvo_sentinels() {
    ACTIVE_RVO_UPDATE.store(u64::MAX, Ordering::Release);
    CURRENT_RVO_FIXED_UPDATE.store(u64::MAX, Ordering::Release);
    CURRENT_RVO_ACTIVATION_COUNT.store(0, Ordering::Release);
    RVO_ACTIVATION_COUNT.store(0, Ordering::Release);
}

pub(crate) fn poll() -> Option<CaptureMessage> {
    capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .queue
        .pop_front()
}

pub(crate) fn abort(reason: &str) {
    let mut state = capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    state.fail(reason.to_owned());
}

type UpdateFn = unsafe extern "C" fn(*mut Object, *const MethodInfo);
type MatchUpdateFn = unsafe extern "C" fn(*mut Object, *const MethodInfo);
type PlayerFinishDeployFn = unsafe extern "C" fn(*mut Object, *const MethodInfo);
type PostRenderFn = unsafe extern "C" fn(*mut Object, *const MethodInfo);
type ProjectileCreateFn = unsafe extern "C" fn(
    *mut Object,
    *mut Object,
    FixedVec3,
    FixedVec3,
    i32,
    *mut Object,
    *const MethodInfo,
);
type ProjectileAddFn = unsafe extern "C" fn(*mut Object, *mut Object, *const MethodInfo);
type ProjectileDestroyFn = unsafe extern "C" fn(*mut Object, *mut Object, bool, *const MethodInfo);
type DamagePerformFn = unsafe extern "C" fn(
    *mut Object,
    *mut Object,
    *mut Object,
    *mut Object,
    *const MethodInfo,
) -> i32;
type FightActorReduceLifeFn =
    unsafe extern "C" fn(*mut Object, NativeHitDamageInfo, *const MethodInfo) -> i32;
type FightControllerOnActorHittedFn =
    unsafe extern "C" fn(*mut Object, NativeHitDamageInfo, *const MethodInfo);
type AdvancedShieldDamageFn =
    unsafe extern "C" fn(*mut Object, *mut Object, i32, FixedVec3, bool, *const MethodInfo) -> i32;
type ActorOnDeadFn = unsafe extern "C" fn(*mut Object, *const MethodInfo);
type RvoControllerActiveFn = unsafe extern "C" fn(*mut Object, *const MethodInfo);
type RvoAddAgentFixedFn =
    unsafe extern "C" fn(*mut Object, *mut Object, *const MethodInfo) -> *mut Object;
type RvoFixedUpdateFn = unsafe extern "C" fn(*mut Object, *const MethodInfo);
type RvoPreCalculationFn = unsafe extern "C" fn(*mut Object, *const MethodInfo);
type RvoCalculateNeighboursFn = unsafe extern "C" fn(*mut Object, *const MethodInfo);
type RvoGenerateNeighbourVosFn = unsafe extern "C" fn(*mut Object, *mut Object, *const MethodInfo);
type RvoGenerateOpponentVosFn =
    unsafe extern "C" fn(*mut Object, *mut Object, *mut Object, *const MethodInfo);
type SelectorCalculateScoreFn = unsafe extern "C" fn(
    FixedPoint,
    FixedPoint,
    FixedPoint,
    FixedPoint,
    FixedPoint,
    FixedPoint,
    FixedPoint,
    FixedPoint,
    bool,
    *const MethodInfo,
) -> FixedPoint;

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn selector_calculate_score_hook(
    distance: FixedPoint,
    distance_score: FixedPoint,
    angle: FixedPoint,
    angle_score: FixedPoint,
    max_attack_range: FixedPoint,
    source_rotation: FixedPoint,
    min_rotation: FixedPoint,
    max_rotation: FixedPoint,
    is_left_side: bool,
    method: *const MethodInfo,
) -> FixedPoint {
    let original = ORIGINAL_SELECTOR_CALCULATE_SCORE.load(Ordering::Acquire);
    if original.is_null() {
        return FixedPoint::default();
    }
    // SAFETY: the installer stores the trampoline for this exact IL2CPP method ABI.
    let original: SelectorCalculateScoreFn = unsafe { std::mem::transmute(original) };
    // SAFETY: all arguments are forwarded unchanged to the native method.
    let score = unsafe {
        original(
            distance,
            distance_score,
            angle,
            angle_score,
            max_attack_range,
            source_rotation,
            min_rotation,
            max_rotation,
            is_left_side,
            method,
        )
    };
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let mut state = capture_state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.armed
            || !state.in_update
            || !state
                .instrumentation_profile
                .is_some_and(CaptureInstrumentationProfile::includes_selector_score)
        {
            return;
        }
        let invocation_ordinal = state.next_selector_invocation_ordinal;
        let Some(next) = invocation_ordinal.checked_add(1) else {
            state.fail("selector invocation ordinal overflow".into());
            return;
        };
        state.next_selector_invocation_ordinal = next;
        state
            .selector_score_calculations
            .push(RawSelectorScoreCalculation {
                invocation_ordinal,
                distance_raw: distance.raw,
                distance_score_raw: distance_score.raw,
                angle_raw: angle.raw,
                angle_score_raw: angle_score.raw,
                max_attack_range_raw: max_attack_range.raw,
                source_rotation_raw: source_rotation.raw,
                min_rotation_raw: min_rotation.raw,
                max_rotation_raw: max_rotation.raw,
                is_left_side,
                score_raw: score.raw,
            });
    }));
    score
}

#[cfg(test)]
fn run_checker_wrapper_offline<Before, Original, After, Fail>(
    active: bool,
    receiver: usize,
    is_attacking_check: bool,
    method: usize,
    mut before: Before,
    mut original: Original,
    mut after: After,
    mut fail: Fail,
) -> bool
where
    Before: FnMut() -> Result<u64, String>,
    Original: FnMut(usize, bool, usize) -> bool,
    After: FnMut(u64, bool) -> Result<(), String>,
    Fail: FnMut(String),
{
    if !active {
        return original(receiver, is_attacking_check, method);
    }
    let ordinal = match before() {
        Ok(ordinal) => Some(ordinal),
        Err(error) => {
            fail(error);
            None
        }
    };
    let check_return = original(receiver, is_attacking_check, method);
    if let Some(ordinal) = ordinal
        && let Err(error) = after(ordinal, check_return)
    {
        fail(error);
    }
    check_return
}

#[cfg(test)]
fn open_checker_call(
    capture: &mut CaptureState,
    mut entry: OpenCheckerCall,
) -> Result<u64, String> {
    let invocation_ordinal = capture.next_checker_invocation_ordinal;
    capture.next_checker_invocation_ordinal = invocation_ordinal
        .checked_add(1)
        .ok_or_else(|| "checker invocation ordinal overflow".to_owned())?;
    entry.invocation_ordinal = invocation_ordinal;
    if capture
        .open_checker_calls
        .insert(invocation_ordinal, entry)
        .is_some()
    {
        return Err(format!(
            "checker invocation ordinal {invocation_ordinal} was opened twice"
        ));
    }
    Ok(invocation_ordinal)
}

#[cfg(test)]
fn complete_checker_call(
    capture: &mut CaptureState,
    invocation_ordinal: u64,
    post_attack_target_candidate: Option<RawCheckerTarget>,
    quick_switch_enabled: bool,
    check_return: bool,
) -> Result<(), String> {
    let entry = capture
        .open_checker_calls
        .remove(&invocation_ordinal)
        .ok_or_else(|| format!("checker invocation {invocation_ordinal} has no open entry"))?;
    if capture
        .completed_checker_calls
        .iter()
        .any(|call| call.entry.invocation_ordinal == invocation_ordinal)
    {
        return Err(format!(
            "checker invocation {invocation_ordinal} completed twice"
        ));
    }
    capture.completed_checker_calls.push(CompletedCheckerCall {
        entry,
        post_attack_target_candidate,
        quick_switch_enabled,
        check_return,
    });
    Ok(())
}

const RVO_HOOK_COUNT: usize = 7;

fn run_rvo_hook_install_sequence(
    mut install: impl FnMut(usize) -> Result<(), String>,
) -> Result<(), String> {
    for index in 0..RVO_HOOK_COUNT {
        install(index)?;
    }
    Ok(())
}

unsafe extern "C" fn rvo_controller_active_hook(
    controller: *mut Object,
    method: *const MethodInfo,
) {
    let original = ORIGINAL_RVO_CONTROLLER_ACTIVE.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    // SAFETY: the installer stores the trampoline for this exact method ABI.
    let original: RvoControllerActiveFn = unsafe { std::mem::transmute(original) };
    let previous = ACTIVE_RVO_CONTROLLER.with(|active| active.replace(controller as usize));
    // SAFETY: arguments are forwarded unchanged from IL2CPP.
    unsafe { original(controller, method) };
    ACTIVE_RVO_CONTROLLER.with(|active| active.set(previous));
    observe_active_rvo_controller(controller);
}

unsafe extern "C" fn rvo_add_agent_fixed_hook(
    simulator: *mut Object,
    agent: *mut Object,
    method: *const MethodInfo,
) -> *mut Object {
    let original = ORIGINAL_RVO_ADD_AGENT_FIXED.load(Ordering::Acquire);
    if original.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: the installer stores the trampoline for this exact method ABI.
    let original: RvoAddAgentFixedFn = unsafe { std::mem::transmute(original) };
    // SAFETY: arguments are forwarded unchanged from IL2CPP.
    let added = unsafe { original(simulator, agent, method) };
    ACTIVE_RVO_CONTROLLER.with(|active| {
        let controller = active.get();
        if controller != 0 && !added.is_null() {
            observe_rvo_agent_owner(controller as *mut Object, added);
        }
    });
    added
}

fn observe_active_rvo_controller(controller: *mut Object) {
    let runtime = RUNTIME.load(Ordering::Acquire);
    if runtime.is_null() {
        return;
    }
    // SAFETY: runtime is boxed for the adapter process lifetime.
    let runtime = unsafe { &*runtime };
    let metadata = {
        capture_state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .metadata
            .rvo
    };
    let Some(metadata) = metadata else {
        return;
    };
    let agent: Result<*mut Object, _> = runtime
        .api
        .field_value(controller, metadata.rvo_controller_agent as *mut FieldInfo);
    if let Ok(agent) = agent
        && !agent.is_null()
    {
        observe_rvo_agent_owner(controller, agent);
    }
}

fn observe_rvo_agent_owner(controller: *mut Object, agent: *mut Object) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let runtime = RUNTIME.load(Ordering::Acquire);
        if runtime.is_null() {
            return;
        }
        // SAFETY: runtime is boxed for the adapter process lifetime.
        let runtime = unsafe { &*runtime };
        let mut state = capture_state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(metadata) = state.metadata.rvo else {
            return;
        };
        let owner: Result<*mut Object, _> = runtime
            .api
            .field_value(controller, metadata.rvo_controller_owner as *mut FieldInfo);
        let owner = match owner {
            Ok(owner) if !owner.is_null() => owner,
            Ok(_) => return,
            Err(error) => {
                if state.armed {
                    state.fail(format!(
                        "cannot observe native RVO controller owner: {error}"
                    ));
                }
                return;
            }
        };
        record_rvo_agent_owner_mapping(&mut state, agent as usize, owner as usize);
    }));
}

fn record_rvo_agent_owner_mapping(state: &mut CaptureState, agent: usize, owner: usize) {
    if let Some(previous) = state.rvo_agent_owners.insert(agent, owner)
        && previous != owner
        && state.armed
    {
        state.fail("native RVO agent was assigned to two FightActors".into());
    }
}

unsafe extern "C" fn rvo_fixed_update_hook(simulator: *mut Object, method: *const MethodInfo) {
    let original = ORIGINAL_RVO_FIXED_UPDATE.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    // SAFETY: the installer stores the trampoline for this exact method ABI.
    let original: RvoFixedUpdateFn = unsafe { std::mem::transmute(original) };
    let update_ordinal = begin_rvo_update(simulator);
    // SAFETY: arguments are forwarded unchanged from IL2CPP.
    unsafe { original(simulator, method) };
    if let Some(update_ordinal) = update_ordinal {
        finish_rvo_update(update_ordinal, simulator);
    }
}

unsafe extern "C" fn rvo_pre_calculation_hook(simulator: *mut Object, method: *const MethodInfo) {
    let original = ORIGINAL_RVO_PRE_CALCULATION.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    // SAFETY: the installer stores the trampoline for this exact method ABI.
    let original: RvoPreCalculationFn = unsafe { std::mem::transmute(original) };
    activate_rvo_update(simulator);
    // PreCalculation is called after any previous double-buffered workers were
    // joined/published and before this update's worker tasks are signalled.
    // SAFETY: arguments are forwarded unchanged from IL2CPP.
    unsafe { original(simulator, method) };
}

unsafe extern "C" fn rvo_calculate_neighbours_hook(agent: *mut Object, method: *const MethodInfo) {
    let original = ORIGINAL_RVO_CALCULATE_NEIGHBOURS.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    // SAFETY: the installer stores the trampoline for this exact method ABI.
    let original: RvoCalculateNeighboursFn = unsafe { std::mem::transmute(original) };
    // SAFETY: arguments are forwarded unchanged from IL2CPP.
    unsafe { original(agent, method) };
    record_rvo_neighbours(agent);
}

unsafe extern "C" fn rvo_generate_neighbour_vos_hook(
    agent: *mut Object,
    vos: *mut Object,
    method: *const MethodInfo,
) {
    let original = ORIGINAL_RVO_GENERATE_NEIGHBOUR_VOS.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    // SAFETY: the installer stores the trampoline for this exact method ABI.
    let original: RvoGenerateNeighbourVosFn = unsafe { std::mem::transmute(original) };
    let update_ordinal = active_rvo_update();
    // SAFETY: arguments are forwarded unchanged from IL2CPP.
    unsafe { original(agent, vos, method) };
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let runtime = RUNTIME.load(Ordering::Acquire);
        if runtime.is_null() {
            return;
        }
        // SAFETY: runtime is boxed for the adapter process lifetime.
        let runtime = unsafe { &*runtime };
        let mut state = capture_state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.armed
            || !state
                .instrumentation_profile
                .is_some_and(CaptureInstrumentationProfile::includes_rvo)
        {
            return;
        }
        let Some(update_ordinal) = update_ordinal else {
            if RVO_ACTIVATION_COUNT.load(Ordering::Acquire) != 0 {
                let error = rvo_boundary_diagnostic(
                    "GenerateNeighbourAgentVOs completed outside an active RVO update",
                    &state,
                );
                state.fail(error);
            }
            return;
        };
        if !rvo_update_selected(&state, update_ordinal)
            || !rvo_source_selected(&state, agent as usize)
        {
            return;
        }
        let Some(metadata) = state.metadata.rvo else {
            state.fail("RVO metadata disappeared during instrumentation".into());
            return;
        };
        match read_native_rvo_vo_buffer(runtime.api, vos, metadata) {
            Ok(vos) => state.rvo_vo_buffers.push(NativeRvoVoBuffer {
                update_ordinal,
                call_ordinal: RVO_VO_CALL_ORDINAL.fetch_add(1, Ordering::AcqRel),
                source: agent as usize,
                vos,
            }),
            Err(error) => state.fail(error),
        }
    }));
}

unsafe extern "C" fn rvo_generate_opponent_vos_hook(
    agent: *mut Object,
    vos: *mut Object,
    other: *mut Object,
    method: *const MethodInfo,
) {
    let original = ORIGINAL_RVO_GENERATE_OPPONENT_VOS.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    // SAFETY: the installer stores the trampoline for this exact method ABI.
    let original: RvoGenerateOpponentVosFn = unsafe { std::mem::transmute(original) };
    let profile_active = {
        let state = capture_state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.armed
            && state
                .instrumentation_profile
                .is_some_and(CaptureInstrumentationProfile::includes_rvo)
            && rvo_source_selected(&state, agent as usize)
            && active_rvo_update().is_none_or(|update| rvo_update_selected(&state, update))
    };
    let update_ordinal = active_rvo_update();
    let before = update_ordinal.filter(|_| profile_active).and_then(|_| {
        let runtime = RUNTIME.load(Ordering::Acquire);
        if runtime.is_null() {
            return None;
        }
        // SAFETY: runtime is boxed for the adapter process lifetime.
        let runtime = unsafe { &*runtime };
        let state = capture_state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.armed
            || !state
                .instrumentation_profile
                .is_some_and(CaptureInstrumentationProfile::includes_rvo)
        {
            return None;
        }
        state
            .metadata
            .rvo
            .map(|metadata| read_vo_buffer_length(runtime.api, vos, metadata))
    });
    // SAFETY: arguments are forwarded unchanged from IL2CPP.
    unsafe { original(agent, vos, other, method) };
    if profile_active && update_ordinal.is_none() {
        if RVO_ACTIVATION_COUNT.load(Ordering::Acquire) == 0 {
            return;
        }
        let mut state = capture_state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.armed {
            let error = rvo_boundary_diagnostic(
                "GenerateOpponentVOs completed outside an active RVO update",
                &state,
            );
            state.fail(error);
        }
        return;
    }
    if let (Some(update_ordinal), Some(before)) = (update_ordinal, before) {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let runtime = RUNTIME.load(Ordering::Acquire);
            if runtime.is_null() {
                return;
            }
            // SAFETY: runtime is boxed for the adapter process lifetime.
            let runtime = unsafe { &*runtime };
            let mut state = capture_state()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !state.armed
                || !state
                    .instrumentation_profile
                    .is_some_and(CaptureInstrumentationProfile::includes_rvo)
            {
                return;
            }
            let Some(metadata) = state.metadata.rvo else {
                state.fail("RVO metadata disappeared during instrumentation".into());
                return;
            };
            let observation = before.and_then(|before| {
                let after = read_vo_buffer_length(runtime.api, vos, metadata)?;
                let colliding =
                    read_appended_vo_colliding(runtime.api, vos, before, after, metadata)?;
                Ok(NativeOpponentVo {
                    update_ordinal,
                    call_ordinal: RVO_VO_CALL_ORDINAL.fetch_add(1, Ordering::AcqRel),
                    source: agent as usize,
                    target: other as usize,
                    vo_buffer_length_before: before,
                    vo_buffer_length_after: after,
                    appended_colliding: colliding,
                })
            });
            match observation {
                Ok(observation) => state.opponent_vos.push(observation),
                Err(error) => state.fail(error),
            }
        }));
    }
}

fn begin_rvo_update(simulator: *mut Object) -> Option<u64> {
    let runtime = RUNTIME.load(Ordering::Acquire);
    if runtime.is_null() {
        return None;
    }
    // SAFETY: runtime is boxed for the adapter process lifetime.
    let runtime = unsafe { &*runtime };
    let mut state = capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !state.armed
        || !state
            .instrumentation_profile
            .is_some_and(CaptureInstrumentationProfile::includes_rvo)
    {
        return None;
    }
    if !state.in_update {
        state.fail("native RVO FixedUpdate began outside the captured logic tick".into());
        return None;
    }
    let Some(metadata) = state.metadata.rvo else {
        state.fail("RVO metadata disappeared during instrumentation".into());
        return None;
    };
    let double_buffering: bool = match runtime.api.field_value(
        simulator,
        metadata.simulator_double_buffering as *mut FieldInfo,
    ) {
        Ok(value) => value,
        Err(error) => {
            state.fail(format!("cannot read native RVO doubleBuffering: {error}"));
            return None;
        }
    };
    let symmetry_breaking_bias: FixedPoint = match runtime.api.field_value(
        simulator,
        metadata.simulator_symmetry_breaking_bias as *mut FieldInfo,
    ) {
        Ok(value) => value,
        Err(error) => {
            state.fail(format!(
                "cannot read native RVO symmetryBreakingBias: {error}"
            ));
            return None;
        }
    };
    let workers: *mut Object = match runtime
        .api
        .field_value(simulator, metadata.simulator_workers as *mut FieldInfo)
    {
        Ok(value) => value,
        Err(error) => {
            state.fail(format!("cannot read native RVO workers: {error}"));
            return None;
        }
    };
    let worker_count = match managed_array_length(workers, "RVO Simulator.workers", 256) {
        Ok(value) => value,
        Err(error) => {
            state.fail(error);
            return None;
        }
    };
    let fight = runtime.current_fight();
    if fight.is_null() {
        state.fail("fight controller disappeared at native RVO update".into());
        return None;
    }
    let native_tick = match runtime
        .api
        .invoke_value::<i32>(fight, "get_Tick", &mut [])
        .map_err(|error| error.to_string())
        .and_then(|tick| {
            u64::try_from(tick).map_err(|_| format!("negative native logic tick {tick}"))
        }) {
        Ok(value) => value,
        Err(error) => {
            state.fail(error);
            return None;
        }
    };
    let ordinal = RVO_UPDATE_ORDINAL.fetch_add(1, Ordering::AcqRel);
    if CURRENT_RVO_FIXED_UPDATE
        .compare_exchange(u64::MAX, ordinal, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        state.fail("native RVO FixedUpdate calls overlapped during instrumentation".into());
        return None;
    }
    CURRENT_RVO_ACTIVATION_COUNT.store(0, Ordering::Release);
    state.rvo_update_modes.insert(ordinal, double_buffering);
    state
        .rvo_update_symmetry_breaking_biases
        .insert(ordinal, symmetry_breaking_bias);
    state
        .rvo_update_start_native_ticks
        .insert(ordinal, native_tick);
    state
        .rvo_update_multithreaded
        .insert(ordinal, worker_count != 0);
    Some(ordinal)
}

fn managed_array_length(array: *mut Object, label: &str, cap: usize) -> Result<usize, String> {
    const ARRAY_LENGTH_OFFSET: usize = 0x18;
    if array.is_null() {
        return Ok(0);
    }
    // The target IL2CPP array ABI stores max_length at 0x18. This is the same
    // build-2259 ABI used below for the directly observed VO array.
    // SAFETY: `array` is a non-null managed array read from a typed field.
    let length = unsafe {
        array
            .cast::<u8>()
            .add(ARRAY_LENGTH_OFFSET)
            .cast::<usize>()
            .read_unaligned()
    };
    if length > cap {
        return Err(format!("native {label} length {length} exceeds cap {cap}"));
    }
    Ok(length)
}

fn finish_rvo_update(update_ordinal: u64, simulator: *mut Object) {
    if CURRENT_RVO_FIXED_UPDATE
        .compare_exchange(
            update_ordinal,
            u64::MAX,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_err()
    {
        let mut state = capture_state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.armed {
            state.fail("native RVO FixedUpdate boundary changed before completion".into());
        }
        return;
    }
    let activation_count = CURRENT_RVO_ACTIVATION_COUNT.swap(0, Ordering::AcqRel);
    let mut state = capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !state.armed {
        return;
    }
    let Some(&double_buffering) = state.rvo_update_modes.get(&update_ordinal) else {
        state.fail(format!("native RVO update {update_ordinal} lost its mode"));
        return;
    };
    let Some(&multithreaded) = state.rvo_update_multithreaded.get(&update_ordinal) else {
        state.fail(format!(
            "native RVO update {update_ordinal} lost its worker mode"
        ));
        return;
    };
    if activation_count == 0 {
        discard_unpublished_rvo_update(&mut state, update_ordinal);
        return;
    }
    if activation_count != 1 {
        state.fail(format!(
            "native RVO update {update_ordinal} crossed PreCalculation {activation_count} times"
        ));
        return;
    }
    if !multithreaded || !double_buffering {
        capture_rvo_published_agents(&mut state, simulator, update_ordinal);
        let Some(&publish_tick) = state.rvo_update_start_native_ticks.get(&update_ordinal) else {
            state.fail(format!(
                "native RVO update {update_ordinal} lost its start tick"
            ));
            return;
        };
        if state
            .rvo_update_publish_native_ticks
            .insert(update_ordinal, publish_tick)
            .is_some()
        {
            state.fail(format!(
                "native RVO update {update_ordinal} was published twice"
            ));
            return;
        }
        if ACTIVE_RVO_UPDATE
            .compare_exchange(
                update_ordinal,
                u64::MAX,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            state.fail(format!(
                "native RVO update {update_ordinal} was not active at synchronous publication"
            ));
        }
    }
}

fn discard_unpublished_rvo_update(state: &mut CaptureState, update_ordinal: u64) {
    state.rvo_update_modes.remove(&update_ordinal);
    state
        .rvo_update_symmetry_breaking_biases
        .remove(&update_ordinal);
    state.rvo_update_start_native_ticks.remove(&update_ordinal);
    state.rvo_update_multithreaded.remove(&update_ordinal);
}

fn activate_rvo_update(simulator: *mut Object) {
    let update_ordinal = CURRENT_RVO_FIXED_UPDATE.load(Ordering::Acquire);
    if update_ordinal == u64::MAX {
        return;
    }
    CURRENT_RVO_ACTIVATION_COUNT.fetch_add(1, Ordering::AcqRel);
    RVO_ACTIVATION_COUNT.fetch_add(1, Ordering::AcqRel);
    let active = ACTIVE_RVO_UPDATE.swap(update_ordinal, Ordering::AcqRel);
    if active == update_ordinal {
        return;
    }
    let mut state = capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !state.armed
        || !state
            .instrumentation_profile
            .is_some_and(CaptureInstrumentationProfile::includes_rvo)
    {
        return;
    }
    if active != u64::MAX {
        capture_rvo_published_agents(&mut state, simulator, active);
        let Some(&publish_tick) = state.rvo_update_start_native_ticks.get(&update_ordinal) else {
            state.fail(format!(
                "native RVO update {update_ordinal} lost its start tick before scheduling"
            ));
            return;
        };
        if state
            .rvo_update_publish_native_ticks
            .insert(active, publish_tick)
            .is_some()
        {
            state.fail(format!("native RVO update {active} was published twice"));
        }
    }
}

fn active_rvo_update() -> Option<u64> {
    let ordinal = ACTIVE_RVO_UPDATE.load(Ordering::Acquire);
    (ordinal != u64::MAX).then_some(ordinal)
}

fn rvo_update_selected(state: &CaptureState, ordinal: u64) -> bool {
    state.rvo_scope.as_ref().is_none_or(|scope| {
        state
            .rvo_update_start_native_ticks
            .get(&ordinal)
            .is_some_and(|&tick| scope.includes_native_tick(tick))
    })
}

fn rvo_source_selected(state: &CaptureState, agent: usize) -> bool {
    state.rvo_scope.as_ref().is_none_or(|scope| {
        let reference = state.rvo_agent_refs.get(&agent).copied().or_else(|| {
            state
                .rvo_agent_owners
                .get(&agent)
                .and_then(|&owner| object_ref_from_pointer(owner, state))
        });
        reference.is_some_and(|reference| {
            reference.kind == ObjectKind::Unit && scope.unit_ids.contains(&reference.id)
        })
    })
}

fn capture_rvo_published_agents(state: &mut CaptureState, simulator: *mut Object, ordinal: u64) {
    if !rvo_update_selected(state, ordinal) {
        return;
    }
    let runtime = RUNTIME.load(Ordering::Acquire);
    if runtime.is_null() {
        state.fail("runtime disappeared at RVO publication".into());
        return;
    }
    // SAFETY: runtime lives for the adapter process lifetime; publication is
    // on the main thread after native workers have completed.
    let runtime = unsafe { &*runtime };
    let Some(metadata) = state.metadata.rvo else {
        state.fail("RVO metadata disappeared at publication".into());
        return;
    };
    match read_native_rvo_agent_set(runtime.api, simulator, metadata, state) {
        Ok(agents) => {
            state.rvo_published_agent_sets.insert(ordinal, agents);
        }
        Err(error) => state.fail(error),
    }
}

fn record_rvo_neighbours(agent: *mut Object) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let runtime = RUNTIME.load(Ordering::Acquire);
        if runtime.is_null() {
            return;
        }
        // SAFETY: runtime is boxed for the adapter process lifetime.
        let runtime = unsafe { &*runtime };
        let mut state = capture_state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.armed
            || !state
                .instrumentation_profile
                .is_some_and(CaptureInstrumentationProfile::includes_rvo)
        {
            return;
        }
        let Some(update_ordinal) = active_rvo_update() else {
            if RVO_ACTIVATION_COUNT.load(Ordering::Acquire) == 0 {
                return;
            }
            let error = rvo_boundary_diagnostic(
                "CalculateNeighbours completed outside an active RVO update",
                &state,
            );
            state.fail(error);
            return;
        };
        if !rvo_update_selected(&state, update_ordinal)
            || !rvo_source_selected(&state, agent as usize)
        {
            return;
        }
        let Some(metadata) = state.metadata.rvo else {
            state.fail("RVO metadata disappeared during instrumentation".into());
            return;
        };
        if !state.rvo_agent_sets.contains_key(&update_ordinal) {
            let snapshot = runtime
                .api
                .field_value::<*mut Object>(agent, metadata.agent_simulator as *mut FieldInfo)
                .map_err(|error| error.to_string())
                .and_then(|simulator| {
                    read_native_rvo_agent_set(runtime.api, simulator, metadata, &state)
                });
            match snapshot {
                Ok(agents) => {
                    state.rvo_agent_sets.insert(update_ordinal, agents);
                }
                Err(error) => {
                    state.fail(error);
                    return;
                }
            }
        }
        let source_call_ordinal = RVO_SOURCE_CALL_ORDINAL.fetch_add(1, Ordering::AcqRel);
        match read_native_rvo_neighbour_set(
            runtime.api,
            agent,
            update_ordinal,
            source_call_ordinal,
            metadata,
        ) {
            Ok(observation) => state.rvo_neighbour_sets.push(observation),
            Err(error) => state.fail(error),
        }
    }));
}

fn read_native_rvo_agent_set(
    api: Api,
    simulator: *mut Object,
    metadata: RvoMetadata,
    state: &CaptureState,
) -> Result<Vec<NativeRvoAgentState>, String> {
    const INSTRUMENTATION_AGENT_CAP: i32 = 4_096;
    if simulator.is_null() {
        return Err("native RVO source has no simulator".into());
    }
    let agents: *mut Object = api
        .field_value(simulator, metadata.simulator_agents as *mut FieldInfo)
        .map_err(|error| error.to_string())?;
    let count = list_count(api, agents, INSTRUMENTATION_AGENT_CAP)?;
    let mut result = Vec::with_capacity(count as usize);
    for index in 0..count {
        let agent = list_item(api, agents, index)?;
        if agent.is_null() {
            return Err(format!("native RVO agent {index} is null"));
        }
        // Keep the native list index, but never read detailed state for
        // unselected agents. Filtering only serialized output is too late.
        if !rvo_source_selected(state, agent as usize) {
            continue;
        }
        let class = api
            .object_class(agent)
            .ok_or_else(|| format!("native RVO agent {index} has no runtime class"))?;
        if !api.class_is_or_inherits(class, metadata.rvo_agent_class as *mut Class) {
            return Err(format!(
                "native RVO agent {index} is {}, expected RVOAgentFixed",
                api.object_class_name(agent)
            ));
        }
        let read = |field: usize| {
            api.field_value::<FixedPoint>(agent, field as *mut FieldInfo)
                .map_err(|error| error.to_string())
        };
        let radius_inner = read(metadata.agent_radius_inner)?;
        let radius_outer = read(metadata.agent_radius_outer)?;
        let max_speed = read(metadata.agent_max_speed)?;
        let desired_speed = read(metadata.agent_desired_speed)?;
        let agent_time_horizon = read(metadata.agent_time_horizon)?;
        let priority = read(metadata.agent_priority)?;
        let published_calculated_speed = read(metadata.agent_published_calculated_speed)?;
        let current_velocity = api
            .field_value::<FixedVec2>(agent, metadata.agent_current_velocity as *mut FieldInfo)
            .map_err(|error| error.to_string())?;
        let desired_velocity = api
            .field_value::<FixedVec2>(agent, metadata.agent_desired_velocity as *mut FieldInfo)
            .map_err(|error| error.to_string())?;
        let desired_target = api
            .field_value::<FixedVec2>(agent, metadata.agent_desired_target as *mut FieldInfo)
            .map_err(|error| error.to_string())?;
        let calculated_target = api
            .field_value::<FixedVec2>(agent, metadata.agent_calculated_target as *mut FieldInfo)
            .map_err(|error| error.to_string())?;
        let size = api
            .field_value::<i32>(agent, metadata.agent_size as *mut FieldInfo)
            .map_err(|error| error.to_string())?;
        let locked = api
            .field_value::<bool>(agent, metadata.agent_locked as *mut FieldInfo)
            .map_err(|error| error.to_string())?;
        let layer = api
            .field_value::<i32>(agent, metadata.agent_layer as *mut FieldInfo)
            .map_err(|error| error.to_string())?;
        let collides_with = api
            .field_value::<i32>(agent, metadata.agent_collides_with as *mut FieldInfo)
            .map_err(|error| error.to_string())?;
        let max_neighbours = api
            .field_value::<i32>(
                agent,
                metadata.agent_internal_max_neighbours as *mut FieldInfo,
            )
            .map_err(|error| error.to_string())?;
        let main_layer = api
            .field_value::<i32>(agent, metadata.rvo_agent_main_layer as *mut FieldInfo)
            .map_err(|error| error.to_string())?;
        let sync_main_layer = api
            .field_value::<i32>(agent, metadata.rvo_agent_sync_main_layer as *mut FieldInfo)
            .map_err(|error| error.to_string())?;
        let group = api
            .field_value::<i32>(agent, metadata.rvo_agent_group as *mut FieldInfo)
            .map_err(|error| error.to_string())?;
        let sync_group = api
            .field_value::<i32>(agent, metadata.rvo_agent_sync_group as *mut FieldInfo)
            .map_err(|error| error.to_string())?;
        let ignore_same_group = api
            .field_value::<bool>(
                agent,
                metadata.rvo_agent_ignore_same_group as *mut FieldInfo,
            )
            .map_err(|error| error.to_string())?;
        let sync_ignore_same_group = api
            .field_value::<bool>(
                agent,
                metadata.rvo_agent_sync_ignore_same_group as *mut FieldInfo,
            )
            .map_err(|error| error.to_string())?;
        let team = api
            .field_value::<FixedRvoTeam>(agent, metadata.rvo_agent_team as *mut FieldInfo)
            .map_err(|error| error.to_string())?;
        let sync_team = api
            .field_value::<FixedRvoTeam>(agent, metadata.rvo_agent_sync_team as *mut FieldInfo)
            .map_err(|error| error.to_string())?;
        let position = api
            .field_value::<FixedVec2>(agent, metadata.agent_position as *mut FieldInfo)
            .map_err(|error| error.to_string())?;
        result.push(NativeRvoAgentState {
            ordinal: index as u32,
            pointer: agent as usize,
            radius_inner,
            size,
            radius_outer,
            max_speed,
            desired_speed,
            agent_time_horizon,
            priority,
            published_calculated_speed,
            current_velocity,
            desired_velocity,
            desired_target,
            calculated_target,
            locked,
            layer,
            collides_with,
            max_neighbours,
            main_layer,
            sync_main_layer,
            group,
            sync_group,
            ignore_same_group,
            sync_ignore_same_group,
            team,
            sync_team,
            position,
        });
    }
    Ok(result)
}

fn rvo_boundary_diagnostic(label: &str, state: &CaptureState) -> String {
    format!(
        "{label}: current_fixed={}, activation_count={}, total_activations={}, allocated_updates={}, initialized={}, in_update={}",
        CURRENT_RVO_FIXED_UPDATE.load(Ordering::Acquire),
        CURRENT_RVO_ACTIVATION_COUNT.load(Ordering::Acquire),
        RVO_ACTIVATION_COUNT.load(Ordering::Acquire),
        RVO_UPDATE_ORDINAL.load(Ordering::Acquire),
        state.initialized,
        state.in_update,
    )
}

fn read_native_rvo_neighbour_set(
    api: Api,
    agent: *mut Object,
    update_ordinal: u64,
    source_call_ordinal: u64,
    metadata: RvoMetadata,
) -> Result<NativeRvoNeighbourSet, String> {
    const INSTRUMENTATION_NEIGHBOUR_CAP: i32 = 4_096;
    let max_neighbours: i32 = api
        .field_value(agent, metadata.agent_max_neighbours as *mut FieldInfo)
        .map_err(|error| error.to_string())?;
    let neighbour_count: i32 = api
        .field_value(agent, metadata.agent_neighbour_count as *mut FieldInfo)
        .map_err(|error| error.to_string())?;
    if !(0..=INSTRUMENTATION_NEIGHBOUR_CAP).contains(&max_neighbours)
        || !(0..=max_neighbours).contains(&neighbour_count)
    {
        return Err(format!(
            "native RVO neighbour bounds are invalid: count={neighbour_count}, max={max_neighbours}"
        ));
    }
    let neighbours: *mut Object = api
        .field_value(agent, metadata.agent_neighbours as *mut FieldInfo)
        .map_err(|error| error.to_string())?;
    let neighbour_dists: *mut Object = api
        .field_value(agent, metadata.agent_neighbour_dists as *mut FieldInfo)
        .map_err(|error| error.to_string())?;
    let neighbours_len = list_count(api, neighbours, INSTRUMENTATION_NEIGHBOUR_CAP)?;
    let distances_len = list_count(api, neighbour_dists, INSTRUMENTATION_NEIGHBOUR_CAP)?;
    if neighbours_len != neighbour_count || distances_len != neighbour_count {
        return Err(format!(
            "native RVO neighbour fields disagree: count={neighbour_count}, neighbours={neighbours_len}, distances={distances_len}"
        ));
    }
    let mut observations = Vec::with_capacity(neighbour_count as usize);
    for index in 0..neighbour_count {
        let neighbour = list_item(api, neighbours, index)?;
        if neighbour.is_null() {
            return Err(format!("native RVO neighbour {index} is null"));
        }
        let mut value_index = index;
        let distance = api
            .invoke_value::<FixedPoint>(
                neighbour_dists,
                "get_Item",
                &mut [argument(&mut value_index)],
            )
            .map_err(|error| error.to_string())?;
        observations.push((neighbour as usize, distance.raw));
    }
    Ok(NativeRvoNeighbourSet {
        update_ordinal,
        source_call_ordinal,
        source: agent as usize,
        neighbours: observations,
    })
}

fn read_vo_buffer_length(api: Api, vos: *mut Object, metadata: RvoMetadata) -> Result<i32, String> {
    let length: i32 = api
        .field_value(vos, metadata.vo_buffer_length as *mut FieldInfo)
        .map_err(|error| error.to_string())?;
    if (0..=64).contains(&length) {
        Ok(length)
    } else {
        Err(format!("native RVO VOBuffer length {length} is invalid"))
    }
}

fn read_native_rvo_vo_buffer(
    api: Api,
    vos: *mut Object,
    metadata: RvoMetadata,
) -> Result<Vec<NativeRvoVo>, String> {
    const ARRAY_LENGTH_OFFSET: usize = 0x18;
    const ARRAY_DATA_OFFSET: usize = 0x20;
    const VO_SIZE: usize = 0xb8;
    if std::mem::size_of::<NativeRvoVo>() != VO_SIZE {
        return Err(format!(
            "native RVO VO ABI size is {}, expected {VO_SIZE}",
            std::mem::size_of::<NativeRvoVo>()
        ));
    }
    let length = usize::try_from(read_vo_buffer_length(api, vos, metadata)?)
        .map_err(|_| "negative RVO VOBuffer length".to_owned())?;
    let buffer: *mut Object = api
        .field_value(vos, metadata.vo_buffer as *mut FieldInfo)
        .map_err(|error| error.to_string())?;
    if buffer.is_null() {
        return Err("native RVO VOBuffer array is null".into());
    }
    // SAFETY: buffer is a live managed array read directly from VOBuffer.buffer.
    let capacity = unsafe {
        buffer
            .cast::<u8>()
            .add(ARRAY_LENGTH_OFFSET)
            .cast::<usize>()
            .read_unaligned()
    };
    if length > capacity || capacity > 4_096 {
        return Err(format!(
            "native RVO VOBuffer length {length} exceeds capacity {capacity}"
        ));
    }
    let mut result = Vec::with_capacity(length);
    for index in 0..length {
        // SAFETY: the validated managed-array capacity contains this target-build
        // value entry, whose 0xb8 layout is mirrored by NativeRvoVo.
        result.push(unsafe {
            buffer
                .cast::<u8>()
                .add(ARRAY_DATA_OFFSET + index * VO_SIZE)
                .cast::<NativeRvoVo>()
                .read_unaligned()
        });
    }
    Ok(result)
}

fn read_appended_vo_colliding(
    api: Api,
    vos: *mut Object,
    before: i32,
    after: i32,
    metadata: RvoMetadata,
) -> Result<bool, String> {
    const ARRAY_LENGTH_OFFSET: usize = 0x18;
    const ARRAY_DATA_OFFSET: usize = 0x20;
    const VO_SIZE: usize = 0xb8;
    const VO_COLLIDING_OFFSET: usize = 0x70;
    if after != before + 1 {
        return Err(format!(
            "GenerateOpponentVOs appended {} VOs instead of one",
            after - before
        ));
    }
    let buffer: *mut Object = api
        .field_value(vos, metadata.vo_buffer as *mut FieldInfo)
        .map_err(|error| error.to_string())?;
    if buffer.is_null() {
        return Err("native RVO VOBuffer array is null".into());
    }
    // The target IL2CPP array ABI stores max_length at 0x18 and value data at 0x20.
    // SAFETY: buffer is a live managed array read directly from VOBuffer.buffer.
    let capacity = unsafe {
        buffer
            .cast::<u8>()
            .add(ARRAY_LENGTH_OFFSET)
            .cast::<usize>()
            .read_unaligned()
    };
    let index = usize::try_from(before).map_err(|_| "negative VO index".to_owned())?;
    if index >= capacity || capacity > 4_096 {
        return Err(format!(
            "native RVO VOBuffer capacity {capacity} does not contain index {index}"
        ));
    }
    // DiffableCs for build 2259 gives VO stride 0xb8 and colliding offset 0x70.
    // SAFETY: capacity was validated above and the byte lies within that value entry.
    let raw = unsafe {
        buffer
            .cast::<u8>()
            .add(ARRAY_DATA_OFFSET + index * VO_SIZE + VO_COLLIDING_OFFSET)
            .read()
    };
    match raw {
        0 => Ok(false),
        1 => Ok(true),
        value => Err(format!(
            "native RVO VO.colliding contains invalid bool {value}"
        )),
    }
}

#[allow(clippy::too_many_lines)]
unsafe extern "C" fn update_hook(controller: *mut Object, method: *const MethodInfo) {
    let original = ORIGINAL_UPDATE.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    // SAFETY: install_update_hook stores the trampoline for this exact method ABI.
    let original: UpdateFn = unsafe { std::mem::transmute(original) };
    let runtime = RUNTIME.load(Ordering::Acquire);
    let mut skip_update = false;
    {
        let mut state = capture_state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Keep the pending logic state on screen until the main camera renders it.
        // Pixel readback happens on the following update so screen-space UI is complete.
        if state.armed && state.visual.is_some() && state.pending_visual.is_some() {
            if state.render_completed {
                state.render_completed = false;
                if let Err(error) = flush_visual_frame(&mut state) {
                    state.fail(error);
                }
            } else {
                skip_update = true;
            }
        }
        if !skip_update {
            state.in_update = state.armed;
            state.traces.clear();
        }
    }
    if skip_update {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if runtime.is_null() {
            return;
        }
        // SAFETY: runtime is boxed for the adapter process lifetime.
        let runtime = unsafe { &*runtime };
        let mut state = capture_state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.armed || state.initialized {
            return;
        }
        let result = (|| {
            if state.await_replay_deployment {
                let fight = runtime.current_fight();
                if fight.is_null() {
                    return Err("fight controller disappeared during replay deployment".into());
                }
                let fighting = runtime
                    .api
                    .invoke_value::<bool>(fight, "IsFighting", &mut [])
                    .map_err(|error| error.to_string())?;
                if fighting {
                    return Err(
                        "replay entered fighting before its completed deployment could be sampled"
                            .into(),
                    );
                }
                let current = runtime.current_match();
                if current.is_null() {
                    return Err("active replay disappeared during deployment".into());
                }
                let player_manager = invoke_object(runtime.api, current, "GetPlayerManager")?;
                let prepared = runtime
                    .api
                    .invoke_value::<bool>(player_manager, "IsAllPlayerPrepareOver", &mut [])
                    .map_err(|error| error.to_string())?;
                if !prepared {
                    state.in_update = false;
                    return Ok(());
                }
                if state.deployment_layout_yaml.is_none() {
                    let (_, context) = recording_context(runtime)?;
                    state.deployment_layout_yaml =
                        Some(read_native_layout(runtime, &context, &state.metadata)?);
                }
                state.await_replay_deployment = false;
            }
            let initial = snapshot(runtime, &mut state, true)?;
            let (game_build, context) = recording_context(runtime)?;
            let layout_yaml = state
                .deployment_layout_yaml
                .take()
                .ok_or_else(|| "deployment layout disappeared before initial capture".to_owned())?;
            state.initialized = true;
            state.last_native_tick = Some(initial.native_tick);
            if let Some(visual) = state.visual.as_ref() {
                visual.apply_calibration()?;
            }
            state.push(CaptureMessage::Initial {
                game_build,
                context,
                layout_yaml,
            })?;
            Ok::<(), String>(())
        })();
        if let Err(error) = result {
            state.in_update = false;
            state.fail(error);
        }
    }));
    // SAFETY: controller and MethodInfo are forwarded unchanged from IL2CPP.
    unsafe { original(controller, method) };

    let _ = catch_unwind(AssertUnwindSafe(|| {
        if runtime.is_null() {
            return;
        }
        // SAFETY: runtime is boxed for the adapter process lifetime.
        let runtime = unsafe { &*runtime };
        let mut state = capture_state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.in_update = false;
        if !state.armed {
            return;
        }
        let result = (|| {
            let fighting = runtime
                .api
                .invoke_value::<bool>(controller, "IsFighting", &mut [])
                .map_err(|error| error.to_string())?;
            if !state.initialized {
                if fighting && state.await_replay_deployment {
                    let (game_build, context) = recording_context(runtime)?;
                    let layout_yaml = read_native_layout(runtime, &context, &state.metadata)?;
                    let initial = snapshot(runtime, &mut state, true)?;
                    if initial.native_tick != 0 {
                        return Err(format!(
                            "replay first became observable in fighting at native tick {}; pre-fight boundary was missed",
                            initial.native_tick
                        ));
                    }
                    state.await_replay_deployment = false;
                    state.initialized = true;
                    state.entered_fighting = true;
                    state.last_native_tick = Some(initial.native_tick);
                    state.push(CaptureMessage::Initial {
                        game_build,
                        context,
                        layout_yaml,
                    })?;
                } else if fighting {
                    return Err(
                        "fight entered during FightController.Update before its pre-fight boundary could be sampled"
                            .into(),
                    );
                } else {
                    state.traces.clear();
                    return Ok(());
                }
            }
            if fighting
                && state.speed_up_allowed
                && state.visual.is_none()
                && !state.speed_up_requested
            {
                let current_match = runtime.current_match();
                if current_match.is_null() {
                    return Err("active match disappeared before recording speed-up".into());
                }
                let action_controller =
                    invoke_object(runtime.api, current_match, "GetMatchActionController")?;
                runtime
                    .api
                    .invoke_void(action_controller, "RequestSpeedUp", &mut [])
                    .map_err(|error| format!("cannot request recording speed-up: {error}"))?;
                state.speed_up_requested = true;
            }
            let next = snapshot(runtime, &mut state, false)?;
            let previous_native_tick = state
                .last_native_tick
                .ok_or_else(|| "capture lost its previous native tick".to_owned())?;
            if next.native_tick == previous_native_tick {
                if !state.traces.is_empty() {
                    return Err(
                        "combat events occurred without an advancing native logic tick".into(),
                    );
                }
                state.entered_fighting |= fighting;
                return Ok(());
            }
            if !state.entered_fighting && !fighting {
                if !state.traces.is_empty() {
                    return Err(
                        "combat events occurred before FightController entered fighting".into(),
                    );
                }
                state.last_native_tick = Some(next.native_tick);
                return Ok(());
            }
            if state.native_tick_step.is_none() && next.native_tick < previous_native_tick {
                if !state.traces.is_empty() {
                    return Err(
                        "combat events occurred while the native fight clock was resetting".into(),
                    );
                }
                state.entered_fighting = true;
                state.last_native_tick = Some(next.native_tick);
                return Ok(());
            }
            let terminal = state.entered_fighting && !fighting;
            let native_tick_step = next.native_tick.checked_sub(previous_native_tick);
            if native_tick_step.is_none() && !terminal {
                return Err(format!(
                    "native logic tick moved backwards from {previous_native_tick} to {}",
                    next.native_tick
                ));
            }
            state.last_native_tick = Some(next.native_tick);
            state.entered_fighting |= fighting;
            if let Some(native_tick_step) = native_tick_step {
                match state.native_tick_step {
                    None => state.native_tick_step = Some(native_tick_step),
                    Some(expected) if native_tick_step == expected => {}
                    Some(expected) => {
                        return Err(format!(
                            "native logic tick step changed from {expected} to {native_tick_step}"
                        ));
                    }
                }
                if native_tick_step == 0 {
                    return Err(format!(
                        "native logic tick did not advance from {previous_native_tick}"
                    ));
                }
            }
            if !state.shield_ids_finalized {
                return Err("combat snapshot preceded final shield identity assignment".into());
            }
            let traces = std::mem::take(&mut state.traces);
            let events = transition_events(&traces, &state);
            if let Some(visual) = state.visual.as_ref() {
                visual.apply_calibration()?;
                state.render_completed = false;
                state.pending_visual = Some(PendingVisualMessage::Transition {
                    events,
                    state: next.world,
                    instrumentation: next.instrumentation,
                    terminal,
                });
            } else {
                state.push(CaptureMessage::Transition {
                    events,
                    state: next.world,
                    instrumentation: next.instrumentation,
                    terminal,
                    frame: None,
                })?;
                if terminal {
                    state.armed = false;
                }
            }
            Ok::<(), String>(())
        })();
        if let Err(error) = result {
            state.fail(error);
        }
    }));
}

unsafe extern "C" fn match_update_hook(current: *mut Object, method: *const MethodInfo) {
    let original = ORIGINAL_MATCH_UPDATE.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    // SAFETY: install_match_update_hook stores the trampoline for this exact method ABI.
    let original: MatchUpdateFn = unsafe { std::mem::transmute(original) };
    // SAFETY: current and MethodInfo are forwarded unchanged from IL2CPP.
    unsafe { original(current, method) };

    let _ = catch_unwind(AssertUnwindSafe(|| {
        let mut state = capture_state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.armed
            || state.visual.is_none()
            || !pending_visual_is_terminal(&state)
            || !state.render_completed
        {
            return;
        }
        state.render_completed = false;
        if let Err(error) = flush_visual_frame(&mut state) {
            state.fail(error);
        }
    }));
}

unsafe extern "C" fn player_finish_deploy_hook(player: *mut Object, method: *const MethodInfo) {
    let original = ORIGINAL_PLAYER_FINISH_DEPLOY.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let runtime = RUNTIME.load(Ordering::Acquire);
        if runtime.is_null() {
            return;
        }
        // SAFETY: runtime is boxed for the adapter process lifetime.
        let runtime = unsafe { &*runtime };
        let mut state = capture_state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.armed || !state.await_replay_deployment {
            return;
        }
        let result = capture_replay_initial_before_final_deploy(runtime, &mut state, player);
        if let Err(error) = result {
            state.fail(error);
        }
    }));
    // SAFETY: the installer stores the trampoline for this exact no-argument void ABI, and
    // player and MethodInfo are forwarded unchanged from IL2CPP exactly once.
    let original: PlayerFinishDeployFn = unsafe { std::mem::transmute(original) };
    unsafe { original(player, method) };
}

fn capture_replay_initial_before_final_deploy(
    runtime: &Runtime,
    state: &mut CaptureState,
    player: *mut Object,
) -> Result<(), String> {
    if !is_final_player_finish_deploy(runtime, player)? {
        return Ok(());
    }
    let (game_build, context) = recording_context(runtime)?;
    let layout_yaml = read_native_layout(runtime, &context, &state.metadata)?;
    state.traces.clear();
    let initial = snapshot(runtime, state, true)?;
    if initial.native_tick != 0 {
        return Err(format!(
            "final replay deployment reached native tick {}; pre-fight boundary was missed",
            initial.native_tick
        ));
    }
    state.await_replay_deployment = false;
    state.initialized = true;
    state.last_native_tick = Some(initial.native_tick);
    state.push(CaptureMessage::Initial {
        game_build,
        context,
        layout_yaml,
    })
}

fn is_final_player_finish_deploy(runtime: &Runtime, player: *mut Object) -> Result<bool, String> {
    if player.is_null() {
        return Err("PlayerController.FinishDeploy received a null player".into());
    }
    let current = runtime.current_match();
    if current.is_null() {
        return Err("active replay disappeared while finishing deployment".into());
    }
    let player_manager = invoke_object(runtime.api, current, "GetPlayerManager")?;
    let controllers = invoke_object(runtime.api, player_manager, "GetPlayerControllers")?;
    let count = list_count(runtime.api, controllers, 16)?;
    if count < 2 {
        return Err(format!(
            "replay deployment requires at least two player controllers, found {count}"
        ));
    }
    let mut found_player = false;
    for index in 0..count {
        let controller = list_item(runtime.api, controllers, index)?;
        if controller.is_null() {
            return Err(format!("player controller {index} is null"));
        }
        let deployed = runtime
            .api
            .invoke_value::<bool>(controller, "IsDeployOver", &mut [])
            .map_err(|error| error.to_string())?;
        if controller == player {
            found_player = true;
            if deployed {
                return Ok(false);
            }
        } else if !deployed {
            return Ok(false);
        }
    }
    if !found_player {
        return Err("FinishDeploy player is absent from PlayerManager".into());
    }
    Ok(true)
}

unsafe extern "C" fn post_render_hook(camera: *mut Object, method: *const MethodInfo) {
    let original = ORIGINAL_POST_RENDER.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    // SAFETY: install_post_render_hook stores the trampoline for this exact method ABI.
    let original: PostRenderFn = unsafe { std::mem::transmute(original) };
    // SAFETY: camera and MethodInfo are forwarded unchanged from IL2CPP.
    unsafe { original(camera, method) };

    let _ = catch_unwind(AssertUnwindSafe(|| {
        let mut state = capture_state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.armed || state.pending_visual.is_none() {
            return;
        }
        let Some(visual) = state.visual.as_ref() else {
            return;
        };
        match visual.api.gc_handle_target(visual.camera_handle) {
            Ok(capture_camera) if capture_camera == camera => state.render_completed = true,
            Ok(_) => {}
            Err(error) => state.fail(error.to_string()),
        }
    }));
}

fn pending_visual_is_terminal(state: &CaptureState) -> bool {
    matches!(
        state.pending_visual,
        Some(PendingVisualMessage::Transition { terminal: true, .. })
    )
}

fn flush_visual_frame(state: &mut CaptureState) -> Result<(), String> {
    let frame = state
        .visual
        .as_mut()
        .ok_or("visual capture disappeared before its pending logic frame")?
        .frame()?;
    let pending = state
        .pending_visual
        .take()
        .ok_or("visual capture has no pending logic frame")?;
    let terminal = matches!(
        pending,
        PendingVisualMessage::Transition { terminal: true, .. }
    );
    if terminal {
        state.armed = false;
        state
            .visual
            .take()
            .ok_or("visual capture disappeared before camera restoration")?
            .restore(true)?;
    }
    match pending {
        PendingVisualMessage::Transition {
            events,
            state: world,
            instrumentation,
            terminal,
        } => state.push(CaptureMessage::Transition {
            events,
            state: world,
            instrumentation,
            terminal,
            frame: Some(frame),
        }),
    }
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn projectile_create_hook(
    system: *mut Object,
    fight_skill: *mut Object,
    target_position: FixedVec3,
    target_position_offset: FixedVec3,
    skill_index: i32,
    target: *mut Object,
    method: *const MethodInfo,
) {
    let original = ORIGINAL_PROJECTILE_CREATE.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    // SAFETY: hook installer stored a trampoline with this method ABI.
    let original: ProjectileCreateFn = unsafe { std::mem::transmute(original) };
    ACTIVE_PROJECTILE_CHANNEL.with(|active| {
        let previous = active.replace(Some((fight_skill as usize, skill_index)));
        // SAFETY: IL2CPP arguments are forwarded unchanged.
        unsafe {
            original(
                system,
                fight_skill,
                target_position,
                target_position_offset,
                skill_index,
                target,
                method,
            );
        }
        active.set(previous);
    });
}

unsafe extern "C" fn projectile_add_hook(
    system: *mut Object,
    controller: *mut Object,
    method: *const MethodInfo,
) {
    let original = ORIGINAL_PROJECTILE_ADD.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    // SAFETY: hook installer stored a trampoline with this method ABI.
    let original: ProjectileAddFn = unsafe { std::mem::transmute(original) };
    // SAFETY: IL2CPP arguments are forwarded unchanged.
    unsafe { original(system, controller, method) };
    let _ = catch_unwind(AssertUnwindSafe(|| record_projectile_release(controller)));
}

unsafe extern "C" fn projectile_destroy_hook(
    system: *mut Object,
    controller: *mut Object,
    intercepted: bool,
    method: *const MethodInfo,
) {
    let original = ORIGINAL_PROJECTILE_DESTROY.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        record_projectile_removal(controller, intercepted);
    }));
    // SAFETY: hook installer stored a trampoline with this method ABI.
    let original: ProjectileDestroyFn = unsafe { std::mem::transmute(original) };
    // SAFETY: IL2CPP arguments are forwarded unchanged.
    unsafe { original(system, controller, intercepted, method) };
}

unsafe extern "C" fn damage_perform_hook(
    performer: *mut Object,
    provider: *mut Object,
    target: *mut Object,
    advanced_shield: *mut Object,
    method: *const MethodInfo,
) -> i32 {
    let original = ORIGINAL_DAMAGE_PERFORM.load(Ordering::Acquire);
    if original.is_null() {
        return 0;
    }
    // SAFETY: hook installer stored a trampoline with this method ABI.
    let original: DamagePerformFn = unsafe { std::mem::transmute(original) };
    let context = resolve_damage_context(provider);
    let previous_context = ACTIVE_DAMAGE_CONTEXT.with(|active| active.replace(Some(context)));
    // SAFETY: IL2CPP arguments are forwarded unchanged.
    let result = unsafe { original(performer, provider, target, advanced_shield, method) };
    ACTIVE_DAMAGE_CONTEXT.with(|active| active.set(previous_context));
    result
}

fn resolve_damage_context(provider: *mut Object) -> DamageContext {
    if provider.is_null() {
        return DamageContext::default();
    }
    let runtime = RUNTIME.load(Ordering::Acquire);
    if runtime.is_null() {
        return DamageContext::default();
    }
    // SAFETY: runtime is boxed for the adapter process lifetime.
    let runtime = unsafe { &*runtime };
    let provider_reference = {
        let state = capture_state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.armed || !state.in_update {
            return DamageContext::default();
        }
        object_ref_from_pointer(provider as usize, &state)
    };
    let owner = runtime
        .api
        .invoke(
            provider,
            "GameRiver.Fight.IDamageProvider.GetOwner",
            &mut [],
        )
        .or_else(|_| runtime.api.invoke(provider, "GetOwner", &mut []))
        .unwrap_or(ptr::null_mut());
    let owner_actor = if owner.is_null() {
        ptr::null_mut()
    } else {
        runtime
            .api
            .invoke(owner, "GameRiver.Fight.ISkillOwner.GetFightActor", &mut [])
            .or_else(|_| runtime.api.invoke(owner, "GetFightActor", &mut []))
            .unwrap_or(ptr::null_mut())
    };
    let team_controller = runtime
        .api
        .invoke(
            provider,
            "GameRiver.Fight.IDamageProvider.GetTeamController",
            &mut [],
        )
        .or_else(|_| runtime.api.invoke(provider, "GetTeamController", &mut []))
        .unwrap_or(ptr::null_mut());
    let native_team_id = if team_controller.is_null() {
        None
    } else {
        invoke_value::<i32>(runtime.api, team_controller, "GetTeamIndex")
            .ok()
            .and_then(|value| u32::try_from(value).ok())
    };
    let state = capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let source = object_ref_from_pointer(owner as usize, &state)
        .or_else(|| object_ref_from_pointer(owner_actor as usize, &state))
        .or(provider_reference);
    DamageContext {
        source,
        source_team_id: native_team_id
            .or_else(|| source.and_then(|value| state.object_teams.get(&value).copied())),
        provider: provider_reference,
    }
}

fn resolve_hit_damage_context(hit: NativeHitDamageInfo) -> DamageContext {
    let provider_context = resolve_damage_context(hit.damage_provider);
    let runtime = RUNTIME.load(Ordering::Acquire);
    if runtime.is_null() {
        return provider_context;
    }
    // SAFETY: runtime is boxed for the adapter process lifetime.
    let runtime = unsafe { &*runtime };
    let source_actor = if hit.source_skill_owner.is_null() {
        ptr::null_mut()
    } else {
        runtime
            .api
            .invoke(
                hit.source_skill_owner,
                "GameRiver.Fight.ISkillOwner.GetFightActor",
                &mut [],
            )
            .or_else(|_| {
                runtime
                    .api
                    .invoke(hit.source_skill_owner, "GetFightActor", &mut [])
            })
            .unwrap_or(ptr::null_mut())
    };
    let native_team_id = if hit.source_team.is_null() {
        None
    } else {
        invoke_value::<i32>(runtime.api, hit.source_team, "GetTeamIndex")
            .ok()
            .and_then(|value| u32::try_from(value).ok())
    };
    let state = capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let source = object_ref_from_pointer(hit.source_skill_owner as usize, &state)
        .or_else(|| object_ref_from_pointer(source_actor as usize, &state))
        .or(provider_context.source);
    DamageContext {
        source,
        source_team_id: native_team_id
            .or(provider_context.source_team_id)
            .or_else(|| source.and_then(|value| state.object_teams.get(&value).copied())),
        provider: provider_context.provider,
    }
}

unsafe extern "C" fn fight_actor_reduce_life_hook(
    actor: *mut Object,
    hit: NativeHitDamageInfo,
    method: *const MethodInfo,
) -> i32 {
    let original = ORIGINAL_FIGHT_ACTOR_REDUCE_LIFE.load(Ordering::Acquire);
    if original.is_null() {
        return 0;
    }
    // SAFETY: hook installer stored the trampoline for this exact method ABI.
    let original: FightActorReduceLifeFn = unsafe { std::mem::transmute(original) };
    let context = resolve_hit_damage_context(hit);
    let previous_context = ACTIVE_DAMAGE_CONTEXT.with(|active| active.replace(Some(context)));
    // SAFETY: IL2CPP arguments are forwarded unchanged.
    let result = unsafe { original(actor, hit, method) };
    ACTIVE_DAMAGE_CONTEXT.with(|active| active.set(previous_context));
    result
}

unsafe extern "C" fn fight_controller_on_actor_hitted_hook(
    controller: *mut Object,
    hit: NativeHitDamageInfo,
    method: *const MethodInfo,
) {
    let original = ORIGINAL_FIGHT_CONTROLLER_ON_ACTOR_HITTED.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    // SAFETY: hook installer stored the trampoline for this exact method ABI.
    let original: FightControllerOnActorHittedFn = unsafe { std::mem::transmute(original) };
    // SAFETY: IL2CPP arguments are forwarded unchanged.
    unsafe { original(controller, hit, method) };
    let _ = catch_unwind(AssertUnwindSafe(|| {
        record_damage(
            resolve_hit_damage_context(hit),
            hit.target_actor,
            ptr::null_mut(),
            hit.damage_real,
        );
    }));
}

unsafe extern "C" fn advanced_shield_damage_hook(
    performer: *mut Object,
    shield: *mut Object,
    damage: i32,
    position: FixedVec3,
    is_main_target: bool,
    method: *const MethodInfo,
) -> i32 {
    let original = ORIGINAL_ADVANCED_SHIELD_DAMAGE.load(Ordering::Acquire);
    if original.is_null() {
        return 0;
    }
    // SAFETY: hook installer stored the trampoline for this exact method ABI.
    let original: AdvancedShieldDamageFn = unsafe { std::mem::transmute(original) };
    let context = ACTIVE_DAMAGE_CONTEXT.with(Cell::get).unwrap_or_default();
    // SAFETY: IL2CPP arguments are forwarded unchanged.
    let result = unsafe { original(performer, shield, damage, position, is_main_target, method) };
    let _ = catch_unwind(AssertUnwindSafe(|| {
        record_damage(context, ptr::null_mut(), shield, result);
    }));
    result
}

unsafe extern "C" fn fight_mech_on_dead_hook(actor: *mut Object, method: *const MethodInfo) {
    let original = ORIGINAL_FIGHT_MECH_ON_DEAD.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        record_actor_death(actor, ObjectKind::Unit);
    }));
    // SAFETY: hook installer stored the trampoline for FightMech.OnDead.
    let original: ActorOnDeadFn = unsafe { std::mem::transmute(original) };
    // SAFETY: IL2CPP arguments are forwarded unchanged.
    unsafe { original(actor, method) };
}

unsafe extern "C" fn fight_crystal_on_dead_hook(actor: *mut Object, method: *const MethodInfo) {
    let original = ORIGINAL_FIGHT_CRYSTAL_ON_DEAD.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        record_actor_death(actor, ObjectKind::Building);
    }));
    // SAFETY: hook installer stored the trampoline for FightCrystal.OnDead.
    let original: ActorOnDeadFn = unsafe { std::mem::transmute(original) };
    // SAFETY: IL2CPP arguments are forwarded unchanged.
    unsafe { original(actor, method) };
}

fn record_actor_death(actor: *mut Object, kind: ObjectKind) {
    let runtime = RUNTIME.load(Ordering::Acquire);
    if runtime.is_null() {
        return;
    }
    // SAFETY: runtime is boxed for the adapter process lifetime.
    let runtime = unsafe { &*runtime };
    let mut state = capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !state.armed || !state.in_update {
        return;
    }
    let pointer = actor as usize;
    let active_damage_context = ACTIVE_DAMAGE_CONTEXT.with(Cell::get);
    let reference = match kind {
        ObjectKind::Unit => state
            .unit_ids
            .get(&pointer)
            .copied()
            .map(|id| ObjectRef::new(ObjectKind::Unit, id)),
        ObjectKind::Building => state
            .building_ids
            .get(&pointer)
            .copied()
            .map(|id| ObjectRef::new(ObjectKind::Building, id)),
        ObjectKind::Projectile | ObjectKind::Shield | ObjectKind::Terrain => None,
    };
    let Some(reference) = reference else {
        if kind == ObjectKind::Building {
            return;
        }
        state.fail(format!(
            "{kind:?}.OnDead referenced an unallocated MCFR object"
        ));
        return;
    };
    if !state.emitted_deaths.insert(reference) {
        state.fail(format!(
            "{kind:?} {} emitted OnDead more than once",
            reference.id
        ));
        return;
    }
    let attribution = match active_damage_context {
        Some(context) => DamageAttribution {
            source: context.source,
            source_team_id: context.source_team_id,
        },
        None => state
            .last_damage_sources
            .remove(&reference)
            .unwrap_or_default(),
    };
    state.last_damage_sources.remove(&reference);
    let result = (|| {
        let transform = invoke_object(runtime.api, actor, "GetFightTransform")?;
        let position = vec3(invoke_value::<FixedVec3>(
            runtime.api,
            transform,
            "GetPositionInt3D",
        )?);
        state.traces.push(match kind {
            ObjectKind::Unit => NativeTrace::UnitDied {
                unit_id: reference.id,
                position,
                source: attribution.source,
                source_team_id: attribution.source_team_id,
            },
            ObjectKind::Building => NativeTrace::BuildingDestroyed {
                building_id: reference.id,
                position,
            },
            ObjectKind::Projectile | ObjectKind::Shield | ObjectKind::Terrain => unreachable!(),
        });
        Ok::<(), String>(())
    })();
    if let Err(error) = result {
        state.fail(format!("{kind:?}.OnDead trace failed: {error}"));
    }
}

fn resolve_projectile_channel(
    api: Api,
    owner: *mut Object,
    fight_skill: usize,
    skill_index: i32,
) -> Result<(u16, i32), String> {
    if owner.is_null() || fight_skill == 0 {
        return Err("projectile skill channel has a null owner or skill".to_owned());
    }
    let all_skills = invoke_object(api, owner, "GetSkills")?;
    let skill_count = list_count(api, all_skills, i32::from(u16::MAX))?;
    let mut skill_slot = None;
    for slot in 0..skill_count {
        if list_item(api, all_skills, slot)? as usize == fight_skill {
            skill_slot =
                Some(u16::try_from(slot).map_err(|_| "projectile skill slot overflow".to_owned())?);
            break;
        }
    }
    let skill_slot = skill_slot.ok_or_else(|| {
        format!("projectile FightSkill is absent from owner GetSkills (index {skill_index})")
    })?;
    let weapons = invoke_object(api, fight_skill as *mut Object, "GetWeapons")?;
    let weapon_count = list_count(api, weapons, 1_024)?;
    let list_candidate = usize::try_from(skill_index)
        .ok()
        .filter(|index| *index < weapon_count as usize)
        .map(|index| list_item(api, weapons, index as i32))
        .transpose()?
        .map(|weapon| {
            let weapon_data = invoke_object(api, weapon, "GetWeaponData")?;
            invoke_value::<i32>(api, weapon_data, "get_Index")
        })
        .transpose()?;
    let mut indexed_candidate = None;
    for index in 0..weapon_count {
        let weapon = list_item(api, weapons, index)?;
        let weapon_data = invoke_object(api, weapon, "GetWeaponData")?;
        let weapon_index = invoke_value::<i32>(api, weapon_data, "get_Index")?;
        if weapon_index == skill_index {
            indexed_candidate = Some(weapon_index);
            break;
        }
    }
    let weapon_index = match (list_candidate, indexed_candidate) {
        (Some(list), Some(indexed)) if list != indexed => {
            return Err(format!(
                "projectile skill index {skill_index} ambiguously maps to weapon indices {list} and {indexed}"
            ));
        }
        (Some(index), _) | (_, Some(index)) => index,
        (None, None) => {
            return Err(format!(
                "projectile skill index {skill_index} is absent from {weapon_count} weapons"
            ));
        }
    };
    Ok((skill_slot, weapon_index))
}

fn record_projectile_release(controller: *mut Object) {
    let runtime = RUNTIME.load(Ordering::Acquire);
    if runtime.is_null() {
        return;
    }
    // SAFETY: runtime is boxed for the process lifetime.
    let runtime = unsafe { &*runtime };
    let mut state = capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !state.armed || !state.in_update {
        return;
    }
    let result = (|| {
        let projectile = invoke_object(runtime.api, controller, "GetFightProjectile")?;
        let pointer = projectile as usize;
        let id = match state.projectile_ids.get(&pointer) {
            Some(id) => *id,
            None => allocate(&mut state.next_projectile_id, "projectile")?,
        };
        state.projectile_ids.entry(pointer).or_insert(id);
        let owner = runtime
            .api
            .invoke(projectile, "GetOwner", &mut [])
            .map_err(|error| error.to_string())? as usize;
        let target = runtime
            .api
            .invoke(projectile, "GetTarget", &mut [])
            .map_err(|error| error.to_string())? as usize;
        let channel = ACTIVE_PROJECTILE_CHANNEL
            .with(Cell::get)
            .map(|(fight_skill, skill_index)| {
                resolve_projectile_channel(
                    runtime.api,
                    owner as *mut Object,
                    fight_skill,
                    skill_index,
                )
            })
            .transpose()?;
        state.traces.push(NativeTrace::ProjectileReleased {
            projectile_id: id,
            owner,
            target,
            skill_slot: channel.map(|value| value.0),
            weapon_index: channel.map(|value| value.1),
        });
        Ok::<(), String>(())
    })();
    if let Err(error) = result {
        state.fail(format!("projectile release trace failed: {error}"));
    }
}

fn record_projectile_removal(controller: *mut Object, intercepted: bool) {
    let runtime = RUNTIME.load(Ordering::Acquire);
    if runtime.is_null() {
        return;
    }
    // SAFETY: runtime is boxed for the process lifetime.
    let runtime = unsafe { &*runtime };
    let mut state = capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !state.armed || !state.in_update {
        return;
    }
    let result = (|| {
        let projectile = invoke_object(runtime.api, controller, "GetFightProjectile")?;
        let pointer = projectile as usize;
        let id = match state.projectile_ids.get(&pointer) {
            Some(id) => *id,
            None => allocate(&mut state.next_projectile_id, "projectile")?,
        };
        state.projectile_ids.entry(pointer).or_insert(id);
        let owner = runtime
            .api
            .invoke(projectile, "GetOwner", &mut [])
            .map_err(|error| error.to_string())? as usize;
        let target = runtime
            .api
            .invoke(projectile, "GetTarget", &mut [])
            .map_err(|error| error.to_string())? as usize;
        let transform = invoke_object(runtime.api, projectile, "GetFightTransform")?;
        let position = vec3(invoke_value::<FixedVec3>(
            runtime.api,
            transform,
            "GetPositionInt3D",
        )?);
        let absorbed_by = state.pending_projectile_absorptions.remove(&id);
        if intercepted && absorbed_by.is_some() {
            return Err("intercepted projectile also has a pending shield absorption".into());
        }
        state.traces.push(NativeTrace::ProjectileRemoved {
            projectile_id: id,
            owner,
            target,
            position,
            intercepted,
            absorbed_by,
        });
        state.projectile_ids.remove(&pointer);
        Ok::<(), String>(())
    })();
    if let Err(error) = result {
        state.fail(format!("projectile removal trace failed: {error}"));
    }
}

fn record_damage(
    context: DamageContext,
    target: *mut Object,
    advanced_shield: *mut Object,
    result: i32,
) {
    let mut state = capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !state.armed || !state.in_update || result <= 0 {
        return;
    }
    let recorded = (|| {
        let target_pointer = if advanced_shield.is_null() {
            target as usize
        } else {
            advanced_shield as usize
        };
        let target = object_ref_from_pointer(target_pointer, &state).ok_or_else(|| {
            format!("damage target 0x{target_pointer:x} is absent from the MCFR identity map")
        })?;
        if target.kind == ObjectKind::Unit && !state.emitted_deaths.contains(&target) {
            state.last_damage_sources.insert(
                target,
                DamageAttribution {
                    source: context.source,
                    source_team_id: context.source_team_id,
                },
            );
        }
        if target.kind == ObjectKind::Shield
            && let Some(projectile) = context
                .provider
                .filter(|value| value.kind == ObjectKind::Projectile)
        {
            state
                .pending_projectile_absorptions
                .insert(projectile.id, target);
        }
        state.traces.push(NativeTrace::Damage {
            source: context.source,
            source_team_id: context.source_team_id,
            target,
            amount: result,
        });
        Ok::<(), String>(())
    })();
    if let Err(error) = recorded {
        state.fail(format!("damage trace failed: {error}"));
    }
}

fn recording_context(runtime: &Runtime) -> Result<(String, DurableContext), String> {
    let current_match = runtime.current_match();
    let round = runtime
        .api
        .invoke_value::<i32>(current_match, "get_RoundCount", &mut [])
        .map_err(|error| error.to_string())?;
    if round <= 0 {
        return Err(format!("invalid combat round {round}"));
    }
    let application = runtime
        .api
        .class("UnityEngine.CoreModule.dll", "UnityEngine", "Application")
        .map_err(|error| error.to_string())?;
    let version = runtime
        .api
        .invoke_static(application, "get_version", &mut [])
        .and_then(|value| runtime.api.string_to_rust(value.cast()))
        .map_err(|error| error.to_string())?;
    let random = invoke_object(runtime.api, current_match, "GetRandom")?;
    let match_seed = invoke_value::<i32>(runtime.api, random, "GetSeed")?;
    Ok((
        version,
        DurableContext {
            logic_step: Rational {
                numerator: 1,
                denominator: 20,
            },
            time_units_per_second: TIME_UNITS_PER_SECOND,
            combat_round: u32::try_from(round).map_err(|_| "combat round overflow".to_owned())?,
            match_seed,
        },
    ))
}

struct RawUnit {
    pointer: usize,
    formation: usize,
    rvo_agent: Option<usize>,
    mech_lock_target: usize,
    weapon_targets: Vec<usize>,
    state: LiveUnitState,
    target_refs: Option<RawTargetRefs>,
}

struct RawTargetRefs {
    mech_lock_target: usize,
    normal_skill_fields_available: bool,
    skill_lock_target: usize,
    skill_attack_target: usize,
}

struct RawBuilding {
    pointer: usize,
    rvo_agent: Option<usize>,
    state: BuildingState,
}

// Capture-local identity is independent of layout order and native counters.
// Existing pointer -> ID bindings survive later snapshots and object removal.
fn sort_buildings(buildings: &mut [RawBuilding]) -> Result<(), String> {
    let key = |building: &RawBuilding| {
        let state = &building.state;
        (
            state.team_id,
            state.building_type_id,
            state.position.x,
            state.position.y,
            state.position.z,
        )
    };
    buildings.sort_by_key(key);
    if buildings
        .windows(2)
        .any(|pair| key(&pair[0]) == key(&pair[1]))
    {
        return Err("ambiguous building identity: duplicate team/type/position".into());
    }
    Ok(())
}

#[derive(Clone)]
struct RawShield {
    pointer: usize,
    owner: usize,
    state: ShieldState,
}

#[derive(Clone, Copy)]
struct RawTerrainApplication {
    unit_pointer: usize,
    elapsed: i32,
}

struct CapturedSnapshot {
    native_tick: u64,
    world: WorldSnapshot,
    instrumentation: Option<CaptureInstrumentationObservation>,
}

/// Reads the replay layout from the match's deployment objects before entering combat.
///
/// This deliberately does not consume the just-captured `WorldSnapshot`: `CardElement` and
/// `ConstructionElement` retain the side-local placement, level, rotation and equipment data
/// that the fight representation no longer exposes directly. The caller caches the resulting
/// YAML before `Match.ChangeProcessState` consumes the deployment state.
fn read_native_layout(
    runtime: &Runtime,
    context: &DurableContext,
    metadata: &Metadata,
) -> Result<String, String> {
    let current = runtime.current_match();
    if current.is_null() {
        return Err("active match disappeared while reading the embedded layout".into());
    }
    let player_manager = invoke_object(runtime.api, current, "GetPlayerManager")
        .map_err(|error| format!("Match.GetPlayerManager: {error}"))?;
    let controllers = invoke_object(runtime.api, player_manager, "GetPlayerControllers")?;
    let super_deployment =
        find_match_module(runtime.api, current, "GameRiver", "SuperDeploymentSystem")?;
    let shield_system = find_match_module(
        runtime.api,
        runtime.current_fight(),
        "GameRiver.Fight",
        "AdvancedEnergyShieldSystem",
    )?;
    let range_item_system = find_match_module(
        runtime.api,
        runtime.current_fight(),
        "GameRiver.Fight",
        "RangeItemSystem",
    )?;
    let round = i32::try_from(context.combat_round)
        .map_err(|_| format!("round {} exceeds layout range", context.combat_round))?;
    let mut sides: [Option<Side>; 2] = [None, None];
    for index in 0..list_count(runtime.api, controllers, 32)? {
        let player_controller = list_item(runtime.api, controllers, index)?;
        let team = invoke_value::<i32>(runtime.api, player_controller, "GetTeamIndex")?;
        let team = usize::try_from(team).map_err(|_| "negative native team index".to_owned())?;
        if team >= sides.len() {
            return Err(format!("unsupported native team index {team}"));
        }
        if sides[team].is_some() {
            return Err(format!("duplicate native team index {team}"));
        }
        sides[team] = Some(
            read_native_side(
                runtime.api,
                player_controller,
                super_deployment,
                shield_system,
                range_item_system,
                team,
                metadata,
            )
            .map_err(|error| format!("team {team}: {error}"))?,
        );
    }
    let layout = Layout {
        seed: Some(context.match_seed),
        round,
        sides: Sides {
            blue: sides[0]
                .take()
                .ok_or_else(|| "native layout has no blue side".to_owned())?,
            red: sides[1]
                .take()
                .ok_or_else(|| "native layout has no red side".to_owned())?,
        },
    };
    canonical_embedded_yaml(layout).map_err(|error| format!("cannot encode native layout: {error}"))
}

fn read_native_side(
    api: Api,
    controller: *mut Object,
    super_deployment: *mut Object,
    shield_system: *mut Object,
    range_item_system: *mut Object,
    team: usize,
    metadata: &Metadata,
) -> Result<Side, String> {
    let unit_manager = invoke_object(api, controller, "GetUnitManager")?;
    let elements = invoke_object(api, unit_manager, "GetUnits")?;
    let unit_count = list_count(api, elements, 10_000)?;
    let mut indexed_units = Vec::with_capacity(
        usize::try_from(unit_count).map_err(|_| "negative native unit count".to_owned())?,
    );
    for index in 0..unit_count {
        let unit = list_item(api, elements, index)?;
        let native_id = invoke_value::<i32>(api, unit, "GetID")?;
        let (type_name, _) = unit_type_from_id(native_id)
            .ok_or_else(|| format!("unknown build-2259 unit type ID {native_id}"))?;
        let native_level = invoke_value::<i32>(api, unit, "GetLevel")?;
        let displayed_level = native_level
            .checked_add(1)
            .ok_or_else(|| format!("unit type {native_id} level overflow"))?;
        let mech_team = invoke_object(api, unit, "GetMechTeam")?;
        let exp = invoke_value::<i32>(api, mech_team, "GetExpInt")?;
        let map_element = invoke_object(api, unit, "GetMapElement")?;
        let position = invoke_value::<MapVector>(api, map_element, "GetPosition")?;
        let rotated = invoke_value::<bool>(api, map_element, "IsRotate")?;
        let equipment = api
            .invoke(unit, "GetEquipment", &mut [])
            .map_err(|error| error.to_string())?;
        let equipment = if equipment.is_null() {
            None
        } else {
            Some(invoke_value::<i32>(api, equipment, "GetID")?)
        };
        let travelling = api
            .invoke_value::<bool>(
                super_deployment,
                "IsTravellingUnit",
                &mut [object_argument(unit)],
            )
            .map_err(|error| error.to_string())?;
        let (x, y) = side_local_position(position, team)?;
        let native_index = api
            .invoke_value::<i32>(unit_manager, "GetUnitIndex", &mut [object_argument(unit)])
            .map_err(|error| error.to_string())?;
        indexed_units.push((
            native_index,
            Formation {
                type_name: type_name.to_owned(),
                index: native_index,
                x,
                y,
                level: Some(displayed_level),
                exp: Some(exp),
                rotated: Some(rotated),
                equipment,
                travelling: Some(travelling),
            },
        ));
    }
    indexed_units.sort_by_key(|(index, _)| *index);
    let native_indices = indexed_units
        .iter()
        .map(|(index, _)| *index)
        .collect::<Vec<_>>();
    validate_native_indices("unit", &native_indices)?;
    let formations = indexed_units
        .into_iter()
        .map(|(_, formation)| formation)
        .collect::<Vec<_>>();

    let construction_manager = invoke_object(api, controller, "GetConstructionManager")?;
    let native_constructions = invoke_object(api, construction_manager, "GetConstructionElements")?;
    let mut constructions = Vec::new();
    for index in 0..list_count(api, native_constructions, 10_000)? {
        let construction = list_item(api, native_constructions, index)?;
        let data = invoke_object(api, construction, "GetConstructionData")?;
        let native_id = invoke_value::<i32>(api, data, "GetID")?;
        let (type_name, _) = construction_type_from_id(native_id)
            .ok_or_else(|| format!("unknown build-2259 construction type ID {native_id}"))?;
        let position = invoke_value::<MapVector>(api, construction, "GetPosition")?;
        let (x, y) = side_local_position(position, team)?;
        constructions.push(StaticPlacement {
            type_name: type_name.to_owned(),
            x,
            y,
        });
    }
    constructions.sort_by(|a, b| (&a.type_name, a.x, a.y).cmp(&(&b.type_name, b.x, b.y)));
    Ok(Side {
        techs: Techs {
            officers: read_native_officers(api, controller)?,
            units: read_native_unit_technologies(api, controller)?,
        },
        research_center: read_native_research_center(api, controller)?,
        energy_tower: read_native_energy_tower(api, controller)?,
        formations,
        constructions,
        contraptions: read_native_contraptions(api, controller, team, shield_system, metadata)?,
        terrains: read_native_terrains(api, controller, range_item_system, team, metadata)?,
        battle_skills: read_native_battle_skills(api, controller, team)?,
    })
}

fn read_native_terrains(
    api: Api,
    player_controller: *mut Object,
    range_item_system: *mut Object,
    team: usize,
    metadata: &Metadata,
) -> Result<Vec<LayoutTerrain>, String> {
    struct TerrainGroup {
        point_count: usize,
        centers: BTreeMap<u32, [i64; 3]>,
        grid_rows: BTreeMap<u32, Vec<u32>>,
    }

    let fight_team_controller = invoke_object(api, player_controller, "GetFightTeamController")?;
    let mut groups = Vec::<TerrainGroup>::new();
    let mut group_by_provider = BTreeMap::<usize, usize>::new();
    for type_tag in 0_i32..=5 {
        let mut type_argument = type_tag;
        let controller = api
            .invoke(
                range_item_system,
                "GetRangeItemController",
                &mut [argument(&mut type_argument)],
            )
            .map_err(|error| error.to_string())?;
        if controller.is_null() {
            continue;
        }
        let items = invoke_object(api, controller, "GetItems")?;
        for index in 0..list_count(api, items, 10_000)? {
            let item = list_item(api, items, index)?;
            if invoke_object(api, item, "GetTeamController")? != fight_team_controller {
                continue;
            }
            if type_tag != 1 {
                return Err(format!(
                    "unsupported retained terrain type {type_tag} for team {team}"
                ));
            }
            let provider = invoke_object(api, item, "GetProvider")?;
            if provider.is_null() || invoke_value::<i32>(api, provider, "GetID")? != 400_002 {
                return Err("retained oil terrain provider is not Sticky Oil Bomb 400002".into());
            }
            let radius = invoke_value::<FixedPoint>(api, item, "GetRange")?.raw;
            if radius != 30 * FIXED_ONE_RAW {
                return Err(format!(
                    "retained oil terrain has radius {radius}, expected 30 m"
                ));
            }
            let round = invoke_value::<i32>(api, item, "get_Round")?;
            let duration = invoke_value::<i32>(api, item, "GetDuration")?;
            if round <= 0 || duration <= round {
                return Err(format!(
                    "retained oil terrain is not a live cross-round item: round={round}, duration={duration}"
                ));
            }
            let native_index = invoke_value::<i32>(api, item, "get_Index")?;
            let center = invoke_value::<FixedVec3>(api, item, "GetPosition")?;
            if center.y.raw != 0 {
                return Err(format!(
                    "retained oil terrain has non-ground height {}",
                    center.y.raw
                ));
            }
            let point_count = invoke_value::<i32>(api, provider, "GetSubEffectCount")?;
            if point_count != 7 || !(0..point_count).contains(&native_index) {
                return Err(format!(
                    "retained oil terrain has index {native_index} for point count {point_count}, expected seven points"
                ));
            }
            let mut grid_rows = if invoke_value::<bool>(api, item, "IsGridMode")? {
                let grid = read_terrain_grid(api, item, metadata)?;
                if grid.size_x != 12 || grid.size_y != 12 || grid.rows.len() != 12 {
                    return Err(format!(
                        "retained oil terrain grid is {}x{}, expected 12x12",
                        grid.size_x, grid.size_y
                    ));
                }
                grid.rows
            } else {
                Vec::new()
            };
            if team != 0 {
                grid_rows = crate::operations::rotate_terrain_grid_rows(&grid_rows);
            }
            let group_index =
                if let Some(&group_index) = group_by_provider.get(&(provider as usize)) {
                    group_index
                } else {
                    let group_index = groups.len();
                    groups.push(TerrainGroup {
                        point_count: usize::try_from(point_count)
                            .map_err(|_| "negative oil terrain point count".to_owned())?,
                        centers: BTreeMap::new(),
                        grid_rows: BTreeMap::new(),
                    });
                    group_by_provider.insert(provider as usize, group_index);
                    group_index
                };
            let point_index = u32::try_from(native_index)
                .map_err(|_| "negative retained oil terrain index".to_owned())?;
            if groups[group_index]
                .centers
                .insert(point_index, [center.x.raw, center.y.raw, center.z.raw])
                .is_some()
            {
                return Err(format!(
                    "duplicate retained oil terrain point index {native_index}"
                ));
            }
            if groups[group_index]
                .grid_rows
                .insert(point_index, grid_rows)
                .is_some()
            {
                return Err(format!(
                    "duplicate retained oil terrain point index {native_index}"
                ));
            }
        }
    }
    Ok(groups
        .into_iter()
        .map(|mut group| -> Result<LayoutTerrain, String> {
            let start = group.centers.get(&0).ok_or_else(|| {
                "live retained-oil export requires surviving native endpoint index 0".to_owned()
            })?;
            let end_index = u32::try_from(group.point_count - 1)
                .map_err(|_| "oil terrain point count exceeds u32".to_owned())?;
            let end = group.centers.get(&end_index).ok_or_else(|| {
                format!(
                    "live retained-oil export requires surviving native endpoint index {end_index}"
                )
            })?;
            for (&index, center) in &group.centers {
                for axis in [0, 2] {
                    let delta = i128::from(end[axis]) - i128::from(start[axis]);
                    let expected =
                        i128::from(start[axis]) + delta * i128::from(index) / i128::from(end_index);
                    // CalculateAttackPositions normalizes and clamps a fixed-point
                    // vector, so its intermediate points differ slightly from
                    // component-wise linear interpolation. This 2^-16 m bound
                    // only rejects centers that cannot belong to the endpoint line.
                    if (i128::from(center[axis]) - expected).abs() > 65_536 {
                        return Err(format!(
                            "retained oil point {index} is inconsistent with its endpoint line"
                        ));
                    }
                }
            }
            let mut positions = Vec::with_capacity(2);
            for (label, center) in [("start", start), ("end", end)] {
                if center[0] % FIXED_ONE_RAW != 0 || center[2] % FIXED_ONE_RAW != 0 {
                    return Err(format!(
                        "retained oil {label} point is not an integer MapVector"
                    ));
                }
                let world = MapVector {
                    x: i32::try_from(center[0] / FIXED_ONE_RAW)
                        .map_err(|_| format!("retained oil {label} x exceeds i32"))?,
                    y: i32::try_from(center[2] / FIXED_ONE_RAW)
                        .map_err(|_| format!("retained oil {label} y exceeds i32"))?,
                };
                let (x, y) = side_local_position(world, team)?;
                positions.push(Position { x, y });
            }
            if group.grid_rows.len() == group.point_count
                && group.grid_rows.values().all(Vec::is_empty)
            {
                group.grid_rows.clear();
            }
            Ok(LayoutTerrain {
                terrain_type: LayoutTerrainType::Oil,
                positions,
                grid_rows: group.grid_rows,
            })
        })
        .collect::<Result<Vec<_>, _>>()?)
}

fn validate_native_indices(kind: &str, indices: &[i32]) -> Result<(), String> {
    let mut previous = None;
    for native_index in indices.iter().copied() {
        if native_index < 0 {
            return Err(format!("native {kind} index is negative: {native_index}"));
        }
        if previous == Some(native_index) {
            return Err(format!("duplicate native {kind} index {native_index}"));
        }
        previous = Some(native_index);
    }
    Ok(())
}

fn find_match_module(
    api: Api,
    current: *mut Object,
    namespace: &str,
    name: &str,
) -> Result<*mut Object, String> {
    let modules = invoke_object(api, current, "GetModules")?;
    let mut found: *mut Object = ptr::null_mut();
    for index in 0..list_count(api, modules, 256)? {
        let module = list_item(api, modules, index)?;
        let class = api
            .object_class(module)
            .ok_or_else(|| "match module has no runtime class".to_owned())?;
        if api.class_namespace(class) != namespace || api.class_name(class) != name {
            continue;
        }
        if !found.is_null() {
            return Err(format!("duplicate match module {namespace}.{name}"));
        }
        found = module;
    }
    if found.is_null() {
        Err(format!("match module {namespace}.{name} is absent"))
    } else {
        Ok(found)
    }
}

fn read_native_officers(api: Api, controller: *mut Object) -> Result<Vec<i32>, String> {
    let manager = invoke_object(api, controller, "GetOfficerManager")?;
    let officers = invoke_object(api, manager, "GetOfficers")?;
    let mut ids = Vec::new();
    let mut seen = BTreeSet::new();
    for index in 0..list_count(api, officers, 10_000)? {
        let id = invoke_value::<i32>(api, list_item(api, officers, index)?, "GetID")?;
        if id <= 0 || !seen.insert(id) {
            return Err(format!("invalid or duplicate native officer ID {id}"));
        }
        ids.push(id);
    }
    Ok(ids)
}

fn read_native_unit_technologies(api: Api, controller: *mut Object) -> Result<Vec<i32>, String> {
    let technology_manager = invoke_object(api, controller, "GetTechnologyManager")?;
    let mut active = BTreeSet::new();
    for mut unit_type_id in (1..=31).chain(std::iter::once(2_002)) {
        let manager = api
            .invoke(
                technology_manager,
                "GetTechnologyManager",
                &mut [argument(&mut unit_type_id)],
            )
            .map_err(|error| error.to_string())?;
        if manager.is_null() {
            continue;
        }
        let technologies = invoke_object(api, manager, "GetTechnologies")?;
        for index in 0..list_count(api, technologies, 10_000)? {
            let technology = list_item(api, technologies, index)?;
            if !invoke_value::<bool>(api, technology, "IsActive")? {
                continue;
            }
            let id = invoke_value::<i32>(api, technology, "GetID")?;
            if id <= 0 || !active.insert(id) {
                return Err(format!("invalid or duplicate active technology ID {id}"));
            }
        }
    }
    Ok(active.into_iter().collect())
}

fn read_native_research_center(
    api: Api,
    controller: *mut Object,
) -> Result<ResearchCenter, String> {
    let manager = invoke_object(api, controller, "GetBlueprintManager")?;
    Ok(ResearchCenter {
        strength_level: read_tower_strength(api, controller, RESEARCH_CENTER_KIND)?,
        attack_level: read_blueprint_level(api, manager, 4, 401, "attack")?,
        defense_level: read_blueprint_level(api, manager, 5, 501, "defense")?,
    })
}

fn read_blueprint_level(
    api: Api,
    manager: *mut Object,
    first_id: i32,
    second_id: i32,
    label: &str,
) -> Result<i32, String> {
    let first = read_blueprint_state(api, manager, first_id)?;
    let second = read_blueprint_state(api, manager, second_id)?;
    decode_blueprint_level(first, second, label)
}

fn decode_blueprint_level(
    first: Option<bool>,
    second: Option<bool>,
    label: &str,
) -> Result<i32, String> {
    match (first, second) {
        (Some(false), None) => Ok(0),
        (None, Some(false)) => Ok(1),
        (None, Some(true)) => Ok(2),
        state => Err(format!(
            "research_center {label} blueprint chain has invalid native state {state:?}"
        )),
    }
}

fn read_blueprint_state(
    api: Api,
    manager: *mut Object,
    mut id: i32,
) -> Result<Option<bool>, String> {
    let blueprint = api
        .invoke(manager, "GetBlueprint", &mut [argument(&mut id)])
        .map_err(|error| error.to_string())?;
    if blueprint.is_null() {
        return Ok(None);
    }
    let researching = api
        .invoke_value::<bool>(manager, "IsResearching", &mut [argument(&mut id)])
        .map_err(|error| error.to_string())?;
    if researching {
        return Err(format!(
            "research_center blueprint {id} is still researching at capture"
        ));
    }
    invoke_value(api, blueprint, "IsActive").map(Some)
}

fn read_native_energy_tower(api: Api, controller: *mut Object) -> Result<EnergyTower, String> {
    let manager = invoke_object(api, controller, "GetEnergyTowerManager")?;
    Ok(EnergyTower {
        strength_level: read_tower_strength(api, controller, ENERGY_TOWER_KIND)?,
        range_enhancement: read_energy_tower_skill(api, manager, RANGE_ENHANCEMENT_SKILL)?,
        movement_enhancement: read_energy_tower_skill(api, manager, MOVEMENT_ENHANCEMENT_SKILL)?,
    })
}

fn read_energy_tower_skill(api: Api, manager: *mut Object, mut id: i32) -> Result<bool, String> {
    let skill = api
        .invoke(manager, "GetSkill", &mut [argument(&mut id)])
        .map_err(|error| error.to_string())?;
    if skill.is_null() {
        return Err(format!("energy_tower skill {id} is absent"));
    }
    invoke_value(api, skill, "IsActive")
}

fn read_tower_strength(
    api: Api,
    controller: *mut Object,
    expected_kind: i32,
) -> Result<i32, String> {
    let manager = invoke_object(api, controller, "GetBuildingManager")?;
    let buildings = invoke_object(api, manager, "GetBuildings")?;
    let mut level = None;
    for index in 0..list_count(api, buildings, 256)? {
        let building = list_item(api, buildings, index)?;
        let data = invoke_object(api, building, "GetBuildingData")?;
        if invoke_value::<i32>(api, data, "get_BuildingType")? != expected_kind {
            continue;
        }
        if level.is_some() {
            return Err(format!("multiple core towers have kind {expected_kind}"));
        }
        let strength = api
            .invoke(building, "GetTowerStrengthenData", &mut [])
            .map_err(|error| error.to_string())?;
        level = Some(if strength.is_null() {
            0
        } else {
            invoke_value::<i32>(api, strength, "GetLevel")?
        });
    }
    level.ok_or_else(|| format!("core tower kind {expected_kind} is absent"))
}

fn read_native_contraptions(
    api: Api,
    controller: *mut Object,
    team: usize,
    shield_system: *mut Object,
    metadata: &Metadata,
) -> Result<Vec<ContraptionPlacement>, String> {
    let manager = invoke_object(api, controller, "GetContraptionManager")?;
    let fight_controller = invoke_object(api, controller, "GetFightTeamController")?;
    let fight_team = invoke_object(api, fight_controller, "GetTeam")?;
    let group = invoke_object(api, fight_team, "GetFightGroup")?;
    let shields = read_team_shields(
        api,
        shield_system,
        group,
        fight_controller,
        u32::try_from(team).map_err(|_| "layout shield team overflow")?,
        metadata,
    )?;
    let mut shield_id = 10_001_i32;
    let source = api
        .invoke(manager, "GetContraption", &mut [argument(&mut shield_id)])
        .map_err(|error| error.to_string())?;
    let expected_energy = invoke_value::<i32>(api, source, "GetAdvancedEnergyShieldValue")?;
    let mut airdrop_defaults = None;
    for shield in &shields {
        if shield.state.source_kind != ShieldSourceKind::CommanderSkill {
            continue;
        }
        let data = invoke_object(api, shield.pointer as *mut Object, "get_EnergyShieldData")?;
        if invoke_value::<i32>(api, data, "GetID")? != 800_001 {
            return Err("unsupported retained commander shield source ID".into());
        }
        if airdrop_defaults.is_none() {
            let source =
                crate::operations::airdrop_shield_source(api).map_err(|error| error.to_string())?;
            let handle = api.gc_handle(source).map_err(|error| error.to_string())?;
            let defaults = (|| -> Result<_, String> {
                Ok((
                    invoke_value::<FixedPoint>(api, source, "GetSubEffectRange")?.raw,
                    invoke_value::<i32>(
                        api,
                        source,
                        "GameRiver.IAdvancedEnergyShieldDataSource.GetAdvancedEnergyShieldValue",
                    )?,
                ))
            })();
            api.free_gc_handle(handle);
            airdrop_defaults = Some(defaults?);
        }
    }
    let mut result = layout_shield_placements(shields, team, expected_energy, airdrop_defaults)?;

    let mine_manager = invoke_object(api, fight_controller, "GetMineManager")?;
    let mines = invoke_object(api, mine_manager, "GetLandMines")?;
    let mut seen = BTreeSet::new();
    for index in 0..list_count(api, mines, 10_000)? {
        let mine = list_item(api, mines, index)?;
        if !seen.insert(mine as usize) {
            return Err("duplicate missile in the live mine list".into());
        }
        if invoke_object(api, mine, "GetTeamController")? != fight_controller {
            return Err("live missile belongs to a different team".into());
        }
        let source = invoke_object(api, mine, "GetDataSource")?;
        let id = invoke_value::<i32>(api, source, "get_ID")?;
        if contraption_type_from_id(id) != Some("missile") {
            return Err(format!("unsupported live mine contraption ID {id}"));
        }
        let position = vec3(invoke_value::<FixedVec3>(api, mine, "GetPosition")?);
        result.push(layout_contraption_position("missile", position, team)?);
    }

    let intercept_manager = invoke_object(api, fight_controller, "GetInterceptSourceManager")?;
    let class = api
        .object_class(intercept_manager)
        .ok_or_else(|| "intercept manager has no runtime class".to_owned())?;
    let field = api
        .field(class, "interceptControllerRecords")
        .map_err(|error| error.to_string())?;
    let records: *mut Object = api
        .field_value(intercept_manager, field)
        .map_err(|error| error.to_string())?;
    let sources = invoke_object(api, records, "GetValues")?;
    let interceptor_class = api
        .class(
            "GRFight.dll",
            "GameRiver.Fight",
            "InterceptCtrGroup_Interceptor",
        )
        .map_err(|error| error.to_string())?;
    for index in 0..list_count(api, sources, 10_000)? {
        let source = list_item(api, sources, index)?;
        if api.object_class(source) != Some(interceptor_class) {
            continue;
        }
        let interceptor = invoke_object(api, source, "get_Interceptor")?;
        if !seen.insert(interceptor as usize) {
            return Err("duplicate interceptor in the live intercept sources".into());
        }
        let building = invoke_object(api, interceptor, "get_Building")?;
        if !invoke_value::<bool>(api, building, "IsAlive")? {
            continue;
        }
        if invoke_object(api, building, "GetCurrentTeamController")? != fight_controller {
            return Err("live interceptor belongs to a different team".into());
        }
        let position = vec3(invoke_value::<FixedVec3>(api, interceptor, "GetPos")?);
        result.push(layout_contraption_position("interceptor", position, team)?);
    }
    Ok(result)
}

fn layout_shield_placements(
    shields: Vec<RawShield>,
    team: usize,
    expected_energy: i32,
    airdrop_defaults: Option<(i64, i32)>,
) -> Result<Vec<ContraptionPlacement>, String> {
    let mut placements = Vec::new();
    for shield in shields {
        let state = shield.state;
        let isairdrop = state.source_kind == ShieldSourceKind::CommanderSkill;
        let (radius, energy) = match state.source_kind {
            ShieldSourceKind::Contraption => (70 * FIXED_ONE_RAW, expected_energy),
            ShieldSourceKind::CommanderSkill => {
                airdrop_defaults.ok_or("airdrop shield defaults are unavailable")?
            }
            _ => continue,
        };
        if shield.owner != 0
            || state.team_id as usize != team
            || state.round_policy != ShieldRoundPolicy::ResetToMax
            || radius <= 0
            || state.radius != radius
            || energy <= 0
            || state.energy.maximum != energy
        {
            return Err(
                "existing contraption shield cannot be represented by layout shield defaults"
                    .into(),
            );
        }
        let mut placement = layout_contraption_position("shield", state.position, team)?;
        placement.isairdrop = isairdrop.then_some(true);
        placements.push(placement);
    }
    Ok(placements)
}

fn layout_contraption_position(
    type_name: &str,
    position: QVec3,
    team: usize,
) -> Result<ContraptionPlacement, String> {
    // Missile/interceptor placement is the XZ projection; their native object
    // may have a built-in vertical offset. Shield sphere height is significant.
    if (type_name == "shield" && position.y != 0)
        || position.x % FIXED_ONE_RAW != 0
        || position.z % FIXED_ONE_RAW != 0
    {
        return Err(format!(
            "existing {type_name} center {position:?} is not an integer layout position"
        ));
    }
    let (x, y) = side_local_position(
        MapVector {
            x: i32::try_from(position.x / FIXED_ONE_RAW).map_err(|_| "contraption x overflow")?,
            y: i32::try_from(position.z / FIXED_ONE_RAW).map_err(|_| "contraption y overflow")?,
        },
        team,
    )?;
    Ok(ContraptionPlacement {
        type_name: type_name.into(),
        x,
        y,
        isairdrop: None,
    })
}

/// Canonicalize the temporary shield namespace once, before resolving S(1)
/// references. Retain first-tick-removed shields so E(1) can still name them.
fn finalize_initial_shield_ids(
    capture: &mut CaptureState,
    current: &[RawShield],
) -> Result<(), String> {
    if capture.shield_ids_finalized {
        return Ok(());
    }
    let mut candidates = capture
        .shield_last_states
        .iter()
        .map(|(&pointer, state)| (pointer, (state.clone(), false)))
        .collect::<BTreeMap<_, _>>();
    let mut seen = BTreeSet::new();
    let mut active_orders = BTreeSet::new();
    for raw in current {
        if !seen.insert(raw.pointer) || capture.retired_shield_pointers.contains(&raw.pointer) {
            return Err("duplicate or retired shield while finalizing initial identities".into());
        }
        let mut state = raw.state.clone();
        if state.active != state.active_order.is_some() {
            return Err("initial shield active flag and order disagree".into());
        }
        if let Some(order) = state.active_order
            && !active_orders.insert((state.team_id, order))
        {
            return Err("duplicate initial shield active order within a team".into());
        }
        state.owner = if raw.owner == 0 {
            None
        } else {
            Some(
                object_ref_from_pointer(raw.owner, capture)
                    .ok_or_else(|| "initial shield owner has no MCFR identity".to_owned())?,
            )
        };
        candidates.insert(raw.pointer, (state, true));
    }
    let mut ordered = candidates
        .iter()
        .map(|(&pointer, (state, present))| {
            let category = if !present {
                2_u8
            } else if state.active {
                0
            } else {
                1
            };
            let key = (
                !*present,
                state.team_id,
                category,
                if category == 0 {
                    state.active_order.unwrap_or_default()
                } else {
                    0
                },
                state.source_kind as u8,
                state.owner,
                [state.position.x, state.position.y, state.position.z],
                state.radius,
                state.round_policy as u8,
                state.energy.maximum,
                state.energy.current,
            );
            (key, pointer)
        })
        .collect::<Vec<_>>();
    ordered.sort_by_key(|(key, _)| *key);
    if ordered.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err("initial inactive/removed shield identities are indistinguishable".into());
    }
    let mut next_id = 1;
    let mut ids = BTreeMap::new();
    for (_, pointer) in ordered {
        ids.insert(pointer, allocate(&mut next_id, "shield")?);
    }
    let remap = capture
        .shield_ids
        .iter()
        .map(|(pointer, &old)| {
            ids.get(pointer)
                .copied()
                .map(|new| (old, new))
                .ok_or_else(|| "temporary shield identity has no captured state".to_owned())
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let map_id = |old: u64| {
        remap
            .get(&old)
            .copied()
            .ok_or_else(|| format!("unresolved temporary shield ID {old}"))
    };
    let map_ref = |mut reference: ObjectRef| -> Result<ObjectRef, String> {
        if reference.kind == ObjectKind::Shield {
            reference.id = map_id(reference.id)?;
        }
        Ok(reference)
    };
    for state in capture.shield_last_states.values_mut() {
        state.shield_id = map_id(state.shield_id)?;
        state.owner = state.owner.map(map_ref).transpose()?;
    }
    capture.object_teams = std::mem::take(&mut capture.object_teams)
        .into_iter()
        .map(|(reference, team)| Ok((map_ref(reference)?, team)))
        .collect::<Result<_, String>>()?;
    capture.last_damage_sources = std::mem::take(&mut capture.last_damage_sources)
        .into_iter()
        .map(|(reference, mut attribution)| {
            attribution.source = attribution.source.map(map_ref).transpose()?;
            Ok((map_ref(reference)?, attribution))
        })
        .collect::<Result<_, String>>()?;
    capture.emitted_deaths = std::mem::take(&mut capture.emitted_deaths)
        .into_iter()
        .map(map_ref)
        .collect::<Result<_, _>>()?;
    for reference in capture
        .pending_projectile_absorptions
        .values_mut()
        .chain(capture.rvo_agent_refs.values_mut())
    {
        *reference = map_ref(*reference)?;
    }
    for trace in &mut capture.traces {
        match trace {
            NativeTrace::Damage { source, target, .. } => {
                *source = source.map(map_ref).transpose()?;
                *target = map_ref(*target)?;
            }
            NativeTrace::ProjectileRemoved { absorbed_by, .. } => {
                *absorbed_by = absorbed_by.map(map_ref).transpose()?;
            }
            NativeTrace::UnitDied { source, .. } => {
                *source = source.map(map_ref).transpose()?;
            }
            NativeTrace::ShieldCreated { shield_id, .. }
            | NativeTrace::ShieldDestroyed { shield_id, .. } => *shield_id = map_id(*shield_id)?,
            NativeTrace::ProjectileReleased { .. }
            | NativeTrace::BuildingDestroyed { .. }
            | NativeTrace::TerrainCreated { .. }
            | NativeTrace::TerrainRemoved { .. } => {}
        }
    }
    capture.shield_ids = ids;
    capture.next_shield_id = next_id;
    capture.shield_ids_finalized = true;
    Ok(())
}

fn read_native_battle_skills(
    api: Api,
    controller: *mut Object,
    team: usize,
) -> Result<Vec<BattleSkillDefinition>, String> {
    let manager = invoke_object(api, controller, "GetCommanderSkillManager")?;
    let skills = invoke_object(api, manager, "GetCommanderSkills")?;
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    for index in 0..list_count(api, skills, 10_000)? {
        let skill = list_item(api, skills, index)?;
        if !invoke_value::<bool>(api, skill, "get_IsActive")? {
            continue;
        }
        let id = invoke_value::<i32>(api, skill, "GetID")?;
        let mut release_data: *mut Object = ptr::null_mut();
        let found = api
            .invoke_value::<bool>(
                manager,
                "TryGetReleaseCommanderSkillData",
                &mut [object_argument(skill), argument(&mut release_data)],
            )
            .map_err(|error| error.to_string())?;
        if !found || release_data.is_null() {
            continue;
        }
        if !seen.insert(id) {
            return Err(format!("duplicate released commander skill ID {id}"));
        }
        let type_name = battle_skill_type_from_id(id)
            .ok_or_else(|| format!("unknown released build-2259 commander skill ID {id}"))?;
        let release_skill = api
            .invoke(
                release_data,
                "GameRiver.Fight.IReleaseCommanderSkillInfo.GetSkill",
                &mut [],
            )
            .map_err(|error| error.to_string())?;
        if release_skill != skill {
            return Err(format!(
                "commander skill {id} release data points to another skill"
            ));
        }
        let positions = api
            .invoke(
                release_data,
                "GameRiver.Fight.IReleaseCommanderSkillInfo.GetPositions",
                &mut [],
            )
            .map_err(|error| error.to_string())?;
        let count = list_count(api, positions, 64)?;
        if count == 0 {
            return Err(format!("commander skill {id} has no release position"));
        }
        let mut local_positions = Vec::with_capacity(
            usize::try_from(count).map_err(|_| "negative skill position count".to_owned())?,
        );
        for mut position_index in 0..count {
            let position = api
                .invoke_value::<MapVector>(
                    positions,
                    "get_Item",
                    &mut [argument(&mut position_index)],
                )
                .map_err(|error| error.to_string())?;
            let (x, y) = side_local_position(position, team)?;
            local_positions.push(Position { x, y });
        }
        result.push(BattleSkillDefinition {
            type_name: type_name.to_owned(),
            positions: local_positions,
        });
    }
    Ok(result)
}

fn side_local_position(position: MapVector, team: usize) -> Result<(i32, i32), String> {
    if team == 0 {
        return Ok((position.x, position.y));
    }
    Ok((
        position
            .x
            .checked_neg()
            .ok_or_else(|| "red layout x coordinate cannot be negated".to_owned())?,
        position
            .y
            .checked_neg()
            .ok_or_else(|| "red layout y coordinate cannot be negated".to_owned())?,
    ))
}

#[allow(clippy::too_many_lines)]
fn snapshot(
    runtime: &Runtime,
    capture: &mut CaptureState,
    initial: bool,
) -> Result<CapturedSnapshot, String> {
    let fight = runtime.current_fight();
    if fight.is_null() {
        return Err("fight controller disappeared during capture".into());
    }
    let tick_before = runtime
        .api
        .invoke_value::<i32>(fight, "get_Tick", &mut [])
        .map_err(|error| error.to_string())?;
    let native_tick = u64::try_from(tick_before)
        .map_err(|_| format!("negative native logic tick {tick_before}"))?;
    let teams = runtime
        .api
        .invoke(fight, "GetTeamControllers", &mut [])
        .map_err(|error| error.to_string())?;
    let team_count = list_count(runtime.api, teams, 32)?;
    let modules = runtime
        .api
        .invoke(fight, "GetModules", &mut [])
        .map_err(|error| error.to_string())?;
    let shield_system = find_module(
        runtime.api,
        modules,
        capture.metadata.advanced_energy_shield_system_class,
        "AdvancedEnergyShieldSystem",
    )?;
    let range_item_system = find_module(
        runtime.api,
        modules,
        capture.metadata.range_item_system_class,
        "RangeItemSystem",
    )?;
    let mut raw_units = Vec::new();
    let mut raw_buildings = Vec::new();
    let mut raw_shields = Vec::new();
    let mut raw_building_teams = BTreeMap::new();
    for team_offset in 0..team_count {
        let controller = list_item(runtime.api, teams, team_offset)?;
        let team_index = runtime
            .api
            .invoke_value::<i32>(controller, "GetTeamIndex", &mut [])
            .map_err(|error| error.to_string())?;
        let team_id = u32::try_from(team_index)
            .map_err(|_| format!("invalid native team index {team_index}"))?;
        let team = runtime
            .api
            .invoke(controller, "GetTeam", &mut [])
            .map_err(|error| error.to_string())?;
        let units = runtime
            .api
            .invoke(team, "GetMeches", &mut [])
            .map_err(|error| error.to_string())?;
        for index in 0..list_count(runtime.api, units, 100_000)? {
            let unit = list_item(runtime.api, units, index)?;
            if !invoke_value::<bool>(runtime.api, unit, "IsAlive")? {
                continue;
            }
            raw_units.push(read_unit(
                runtime.api,
                unit,
                team_id,
                &capture.metadata,
                capture.instrumentation_profile,
            )?);
        }
        let towers = invoke_object(runtime.api, team, "GetTowers")?;
        let team_buildings: *mut Object = runtime
            .api
            .field_value(
                team,
                capture.metadata.fight_team_buildings as *mut FieldInfo,
            )
            .map_err(|error| error.to_string())?;
        let constructions: *mut Object = runtime
            .api
            .field_value(
                team,
                capture.metadata.fight_team_constructions as *mut FieldInfo,
            )
            .map_err(|error| error.to_string())?;
        for list in [towers, team_buildings, constructions] {
            for index in 0..list_count(runtime.api, list, 100_000)? {
                let building = list_item(runtime.api, list, index)?;
                if !invoke_value::<bool>(runtime.api, building, "IsAlive")? {
                    continue;
                }
                let pointer = building as usize;
                if let Some(existing_team) = raw_building_teams.insert(pointer, team_id) {
                    if existing_team != team_id {
                        return Err(format!(
                            "FightCrystal at 0x{pointer:x} belongs to conflicting teams"
                        ));
                    }
                    continue;
                }
                raw_buildings.push(read_building(
                    runtime.api,
                    building,
                    team_id,
                    &capture.metadata,
                    capture.instrumentation_profile,
                )?);
            }
        }
        raw_shields.extend(read_team_shields(
            runtime.api,
            shield_system,
            invoke_object(runtime.api, team, "GetFightGroup")
                .map_err(|error| format!("FightTeam.GetFightGroup: {error}"))?,
            controller,
            team_id,
            &capture.metadata,
        )?);
    }
    raw_units.sort_by_key(|unit| {
        (
            unit.state.team_id,
            unit.state.position.z,
            unit.state.position.x,
            unit.pointer,
        )
    });
    if initial {
        for pair in raw_units.windows(2) {
            if pair[0].state.team_id == pair[1].state.team_id
                && pair[0].state.position.x == pair[1].state.position.x
                && pair[0].state.position.z == pair[1].state.position.z
            {
                return Err("two initial same-team units have equal world coordinates".into());
            }
        }
    }
    let mut units = Vec::with_capacity(raw_units.len());
    let mut raw_mech_lock_targets = Vec::with_capacity(raw_units.len());
    let mut raw_weapon_targets = Vec::new();
    let mut raw_target_refs = Vec::new();
    for mut unit in raw_units {
        let unit_id = match capture.unit_ids.get(&unit.pointer) {
            Some(id) => *id,
            None => allocate(&mut capture.next_unit_id, "unit")?,
        };
        capture.unit_ids.entry(unit.pointer).or_insert(unit_id);
        let original_team_id = *capture
            .original_unit_teams
            .entry(unit.pointer)
            .or_insert(unit.state.team_id);
        let formation_key = if unit.formation == 0 {
            unit.pointer
        } else {
            unit.formation
        };
        let formation_id = match capture.formation_ids.get(&formation_key) {
            Some(id) => *id,
            None => allocate(&mut capture.next_formation_id, "formation")?,
        };
        capture
            .formation_ids
            .entry(formation_key)
            .or_insert(formation_id);
        if let Some(agent) = unit.rvo_agent {
            capture
                .rvo_agent_refs
                .insert(agent, ObjectRef::new(ObjectKind::Unit, unit_id));
        }
        unit.state.unit_id = unit_id;
        unit.state.original_team_id = original_team_id;
        capture.object_teams.insert(
            ObjectRef::new(ObjectKind::Unit, unit_id),
            unit.state.team_id,
        );
        unit.state.formation_id = formation_id;
        raw_mech_lock_targets.push((units.len(), unit.mech_lock_target));
        raw_weapon_targets.extend(
            unit.weapon_targets
                .into_iter()
                .enumerate()
                .map(|(aim_index, target)| (units.len(), aim_index, target)),
        );
        if let Some(target_refs) = unit.target_refs {
            raw_target_refs.push((unit_id, target_refs));
        }
        units.push(unit.state);
    }

    sort_buildings(&mut raw_buildings)?;
    let mut buildings = Vec::with_capacity(raw_buildings.len());
    for mut building in raw_buildings {
        let id = match capture.building_ids.get(&building.pointer) {
            Some(id) => *id,
            None => allocate(&mut capture.next_building_id, "building")?,
        };
        capture.building_ids.entry(building.pointer).or_insert(id);
        building.state.building_id = id;
        capture.object_teams.insert(
            ObjectRef::new(ObjectKind::Building, id),
            building.state.team_id,
        );
        if let Some(agent) = building.rvo_agent {
            capture
                .rvo_agent_refs
                .insert(agent, ObjectRef::new(ObjectKind::Building, id));
        }
        buildings.push(building.state);
    }
    // Earlier snapshots establish temporary identities for native event hooks.
    // Finalize only on the first advancing combat snapshot, not deployment or
    // a zero/backwards clock transition. All reference resolution below then
    // uses the final IDs, including projectiles and instrumentation.
    if !initial
        && !capture.shield_ids_finalized
        && capture
            .last_native_tick
            .is_some_and(|previous| native_tick > previous)
        && (capture.entered_fighting || invoke_value::<bool>(runtime.api, fight, "IsFighting")?)
    {
        finalize_initial_shield_ids(capture, &raw_shields)?;
    }
    let mut shields = Vec::with_capacity(raw_shields.len());
    let mut current_shield_pointers = BTreeSet::new();
    for mut shield in raw_shields {
        if !current_shield_pointers.insert(shield.pointer) {
            return Err(format!(
                "FightEnergyShield at 0x{:x} appears more than once",
                shield.pointer
            ));
        }
        if capture.retired_shield_pointers.contains(&shield.pointer) {
            return Err(format!(
                "retired FightEnergyShield pointer 0x{:x} was reused",
                shield.pointer
            ));
        }
        let id = match capture.shield_ids.get(&shield.pointer) {
            Some(id) => *id,
            None => allocate(&mut capture.next_shield_id, "shield")?,
        };
        capture.shield_ids.entry(shield.pointer).or_insert(id);
        shield.state.shield_id = id;
        shield.state.owner = resolve_target_ref(
            runtime.api,
            shield.owner,
            "FightEnergyShield.owner",
            capture,
        )?;
        let reference = ObjectRef::new(ObjectKind::Shield, id);
        capture.object_teams.insert(reference, shield.state.team_id);
        capture
            .shield_last_states
            .insert(shield.pointer, shield.state.clone());
        shields.push(shield.state);
    }
    if initial {
        capture.live_shield_pointers = current_shield_pointers.clone();
    } else {
        for pointer in current_shield_pointers.difference(&capture.live_shield_pointers) {
            let state = capture
                .shield_last_states
                .get(pointer)
                .ok_or_else(|| "new shield is missing its captured state".to_owned())?;
            capture.traces.push(NativeTrace::ShieldCreated {
                shield_id: state.shield_id,
                team_id: state.team_id,
                source_kind: state.source_kind,
                position: state.position,
            });
        }
        for pointer in capture
            .live_shield_pointers
            .difference(&current_shield_pointers)
        {
            let state = capture
                .shield_last_states
                .get(pointer)
                .ok_or_else(|| "destroyed shield is missing its last state".to_owned())?;
            capture.traces.push(NativeTrace::ShieldDestroyed {
                shield_id: state.shield_id,
                position: state.position,
            });
            capture.retired_shield_pointers.insert(*pointer);
        }
        capture.live_shield_pointers = current_shield_pointers.clone();
    }
    let terrains = read_terrains(runtime.api, range_item_system, capture, initial)
        .map_err(|error| format!("dynamic terrain snapshot failed: {error}"))?;
    for (unit_index, target_pointer) in raw_mech_lock_targets {
        units[unit_index].mech_lock_target =
            resolve_target_ref(runtime.api, target_pointer, "FightMech.lockTarget", capture)?;
    }
    for (unit_index, aim_index, target_pointer) in raw_weapon_targets {
        units[unit_index].weapon_aims[aim_index].attack_target = resolve_target_ref(
            runtime.api,
            target_pointer,
            "FightSkill.attackTarget",
            capture,
        )?;
    }
    let instrumentation = match capture.instrumentation_profile {
        Some(profile) if profile.includes_target_refs() => {
            let mut observations = Vec::with_capacity(raw_target_refs.len());
            for (unit_id, refs) in raw_target_refs {
                if capture.rvo_scope.as_ref().is_some_and(|scope| {
                    !scope.includes_native_tick(native_tick) || !scope.unit_ids.contains(&unit_id)
                }) {
                    continue;
                }
                observations.push(UnitTargetRefsObservation {
                    unit: ObjectRef::new(ObjectKind::Unit, unit_id),
                    mech_lock_target: resolve_target_ref(
                        runtime.api,
                        refs.mech_lock_target,
                        "FightMech.lockTarget",
                        capture,
                    )?,
                    normal_skill_fields_available: refs.normal_skill_fields_available,
                    skill_lock_target: resolve_target_ref(
                        runtime.api,
                        refs.skill_lock_target,
                        "FightSkill.lockTarget",
                        capture,
                    )?,
                    skill_attack_target: resolve_target_ref(
                        runtime.api,
                        refs.skill_attack_target,
                        "FightSkill.attackTarget",
                        capture,
                    )?,
                });
            }
            let target_refs = TargetRefsObservation {
                units: observations,
            };
            if profile.includes_rvo() {
                let observation = resolve_rvo_observation(target_refs, native_tick, capture)?;
                // Still drain pending asynchronous publications after the end
                // of the requested start window, but omit empty sidecar rows.
                (capture.rvo_scope.is_none()
                    || !observation.target_refs.units.is_empty()
                    || !observation.rvo_updates.is_empty())
                .then_some(CaptureInstrumentationObservation::TargetRefsRvo(
                    observation,
                ))
            } else {
                Some(CaptureInstrumentationObservation::TargetRefs(target_refs))
            }
        }
        Some(CaptureInstrumentationProfile::SkillAttackableCheckerV1) => {
            Some(CaptureInstrumentationObservation::SkillAttackableChecker(
                drain_skill_attackable_checker_calls(native_tick, capture)?,
            ))
        }
        Some(CaptureInstrumentationProfile::SelectorScoreV1) => Some(
            CaptureInstrumentationObservation::SelectorScore(drain_selector_score_calls(capture)?),
        ),
        Some(CaptureInstrumentationProfile::SelectorScoreRvoV1) => Some(
            CaptureInstrumentationObservation::SelectorScoreRvo(SelectorScoreRvoObservation {
                selector_score: drain_selector_score_calls(capture)?,
                rvo_updates: resolve_rvo_updates(native_tick, capture)?,
            }),
        ),
        None => None,
        Some(_) => unreachable!("all capture instrumentation profiles are handled"),
    };
    let projectiles = read_projectiles(runtime, capture)?;
    if !capture.pending_projectile_absorptions.is_empty() {
        return Err(format!(
            "{} projectile shield absorptions were not followed by projectile removal",
            capture.pending_projectile_absorptions.len()
        ));
    }
    let tick_after = runtime
        .api
        .invoke_value::<i32>(fight, "get_Tick", &mut [])
        .map_err(|error| error.to_string())?;
    if tick_before != tick_after {
        return Err(format!(
            "logic tick changed during snapshot ({tick_before} -> {tick_after})"
        ));
    }
    Ok(CapturedSnapshot {
        native_tick,
        world: WorldSnapshot {
            live_units: units,
            projectiles,
            buildings,
            shields,
            terrains,
        },
        instrumentation,
    })
}

fn read_terrains(
    api: Api,
    system: *mut Object,
    capture: &mut CaptureState,
    initial: bool,
) -> Result<Vec<TerrainState>, String> {
    let mut terrains = Vec::new();
    let mut current_pointers = BTreeSet::new();
    for type_tag in 0_i32..=5 {
        let mut type_argument = type_tag;
        let controller = api
            .invoke(
                system,
                "GetRangeItemController",
                &mut [argument(&mut type_argument)],
            )
            .map_err(|error| {
                format!("RangeItemSystem.GetRangeItemController({type_tag}) failed: {error}")
            })?;
        if controller.is_null() {
            continue;
        }
        let controller_type = decode_terrain_type(type_tag)?;
        let native_type = invoke_value::<i32>(api, controller, "GetRangeItemType")
            .map_err(|error| format!("terrain controller {type_tag} GetRangeItemType: {error}"))?;
        if native_type != type_tag {
            return Err(format!(
                "RangeItemController type {native_type} disagrees with requested {type_tag}"
            ));
        }
        let applications = read_terrain_applications(api, controller, capture)
            .map_err(|error| format!("terrain controller {type_tag} applications: {error}"))?;
        let effect_duration: i32 = api
            .field_value(
                controller,
                capture.metadata.range_item_effect_time_duration as *mut FieldInfo,
            )
            .map_err(|error| {
                format!("terrain controller {type_tag} effectTimeDuration: {error}")
            })?;
        let items = invoke_object(api, controller, "GetItems")
            .map_err(|error| format!("terrain controller {type_tag} GetItems: {error}"))?;
        if items.is_null() {
            continue;
        }
        let item_count = list_count(api, items, 100_000)
            .map_err(|error| format!("terrain controller {type_tag} item count: {error}"))?;
        for index in 0..item_count {
            let item = list_item(api, items, index)
                .map_err(|error| format!("terrain controller {type_tag} item {index}: {error}"))?;
            let pointer = item as usize;
            if !current_pointers.insert(pointer) {
                return Err(format!("RangeItem at 0x{pointer:x} appears more than once"));
            }
            if capture.retired_terrain_pointers.contains(&pointer) {
                return Err(format!(
                    "retired RangeItem pointer 0x{pointer:x} was reused"
                ));
            }
            let item_type =
                invoke_value::<i32>(api, item, "GetRangeItemType").map_err(|error| {
                    format!("terrain controller {type_tag} item {index} GetRangeItemType: {error}")
                })?;
            if item_type != type_tag {
                return Err(format!(
                    "RangeItem at 0x{pointer:x} type {item_type} disagrees with controller {type_tag}"
                ));
            }
            let id = match capture.terrain_ids.get(&pointer) {
                Some(id) => *id,
                None => allocate(&mut capture.next_terrain_id, "terrain")?,
            };
            capture.terrain_ids.entry(pointer).or_insert(id);
            let team_controller = api
                .invoke(item, "GetTeamController", &mut [])
                .map_err(|error| format!("terrain {id} item {index} GetTeamController: {error}"))?;
            let team_id = if team_controller.is_null() {
                None
            } else {
                let native = invoke_value::<i32>(api, team_controller, "GetTeamIndex")
                    .map_err(|error| format!("terrain {id} GetTeamIndex: {error}"))?;
                Some(
                    u32::try_from(native)
                        .map_err(|_| format!("invalid terrain team index {native}"))?,
                )
            };
            let position = vec3(
                invoke_value::<FixedVec3>(api, item, "GetPosition")
                    .map_err(|error| format!("terrain {id} GetPosition: {error}"))?,
            );
            let radius = invoke_value::<FixedPoint>(api, item, "GetRange")
                .map_err(|error| format!("terrain {id} GetRange: {error}"))?
                .raw;
            let grid = if invoke_value::<bool>(api, item, "IsGridMode")
                .map_err(|error| format!("terrain {id} IsGridMode: {error}"))?
            {
                Some(
                    read_terrain_grid(api, item, &capture.metadata)
                        .map_err(|error| format!("terrain {id} grid: {error}"))?,
                )
            } else {
                None
            };
            let round = invoke_value::<i32>(api, item, "get_Round")
                .map_err(|error| format!("terrain {id} get_Round: {error}"))?;
            let duration = invoke_value::<i32>(api, item, "GetDuration")
                .map_err(|error| format!("terrain {id} GetDuration: {error}"))?;
            let remaining_rounds = if duration > 1 {
                Some(
                    u32::try_from(
                        duration
                            .checked_sub(round)
                            .ok_or_else(|| format!("terrain {id} round subtraction overflow"))?,
                    )
                    .map_err(|_| {
                        format!("terrain {id} has round {round} beyond duration {duration}")
                    })?,
                )
            } else {
                None
            };
            let logic_lifetime = read_terrain_lifetime(api, item, capture)
                .map_err(|error| format!("terrain {id} lifetime: {error}"))?;
            let mut item_applications = applications
                .get(&pointer)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|application| {
                    let unit_id = capture
                        .unit_ids
                        .get(&application.unit_pointer)
                        .copied()
                        .ok_or_else(|| {
                            format!(
                                "terrain {id} affectedUnits references unknown FightMech 0x{:x}",
                                application.unit_pointer
                            )
                        })?;
                    Ok(TerrainApplicationState {
                        unit_id,
                        periodic_clock: (effect_duration > 0).then_some(TerrainEffectClock {
                            elapsed: application.elapsed,
                            duration: effect_duration,
                        }),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            item_applications.sort_by_key(|application| application.unit_id);
            let state = TerrainState {
                terrain_id: id,
                team_id,
                terrain_type: controller_type,
                position,
                radius,
                grid,
                remaining_rounds,
                logic_lifetime,
                applications: item_applications,
            };
            let reference = ObjectRef::new(ObjectKind::Terrain, id);
            if let Some(team_id) = team_id {
                capture.object_teams.insert(reference, team_id);
            }
            capture.terrain_last_states.insert(pointer, state.clone());
            terrains.push(state);
        }
    }
    if initial {
        capture.live_terrain_pointers = current_pointers;
    } else {
        for pointer in current_pointers.difference(&capture.live_terrain_pointers) {
            let state = capture
                .terrain_last_states
                .get(pointer)
                .ok_or_else(|| "new terrain is missing its captured state".to_owned())?;
            capture.traces.push(NativeTrace::TerrainCreated {
                terrain_id: state.terrain_id,
                team_id: state.team_id,
                terrain_type: state.terrain_type,
                position: state.position,
                radius: state.radius,
            });
        }
        for pointer in capture.live_terrain_pointers.difference(&current_pointers) {
            let state = capture
                .terrain_last_states
                .get(pointer)
                .ok_or_else(|| "removed terrain is missing its last state".to_owned())?;
            capture.traces.push(NativeTrace::TerrainRemoved {
                terrain_id: state.terrain_id,
                position: state.position,
            });
            capture.retired_terrain_pointers.insert(*pointer);
        }
        capture.live_terrain_pointers = current_pointers;
    }
    terrains.sort_by_key(|terrain| terrain.terrain_id);
    Ok(terrains)
}

fn read_terrain_applications(
    api: Api,
    controller: *mut Object,
    capture: &CaptureState,
) -> Result<BTreeMap<usize, Vec<RawTerrainApplication>>, String> {
    let affected: *mut Object = api
        .field_value(
            controller,
            capture.metadata.range_item_affected_units as *mut FieldInfo,
        )
        .map_err(|error| error.to_string())?;
    let times: *mut Object = api
        .field_value(
            controller,
            capture.metadata.range_item_affected_unit_times as *mut FieldInfo,
        )
        .map_err(|error| error.to_string())?;
    let count = invoke_value::<i32>(api, affected, "get_Count")?;
    let time_count = list_count(api, times, 100_000)?;
    if count != time_count {
        return Err(format!(
            "RangeItemController affectedUnits count {count} disagrees with affectedUnitTimes {time_count}"
        ));
    }
    let mut result = BTreeMap::<usize, Vec<RawTerrainApplication>>::new();
    for index in 0..count {
        let mut key_index = index;
        let unit = api
            .invoke(affected, "GetKeyByIndex", &mut [argument(&mut key_index)])
            .map_err(|error| error.to_string())?;
        let mut value_index = index;
        let terrain = api
            .invoke(
                affected,
                "GetValueByIndex",
                &mut [argument(&mut value_index)],
            )
            .map_err(|error| error.to_string())?;
        if unit.is_null() || terrain.is_null() {
            return Err("RangeItemController affectedUnits contains null".into());
        }
        let elapsed = list_i32_item(api, times, index)?;
        result
            .entry(terrain as usize)
            .or_default()
            .push(RawTerrainApplication {
                unit_pointer: unit as usize,
                elapsed,
            });
    }
    Ok(result)
}

fn read_terrain_grid(
    api: Api,
    item: *mut Object,
    metadata: &Metadata,
) -> Result<TerrainGridState, String> {
    let grid = invoke_object(api, item, "GetGridBlock")?;
    let origin_x: FixedPoint = api
        .field_value(grid, metadata.grid_position_x as *mut FieldInfo)
        .map_err(|error| error.to_string())?;
    let origin_y: FixedPoint = api
        .field_value(grid, metadata.grid_position_y as *mut FieldInfo)
        .map_err(|error| error.to_string())?;
    let size: UnityVec2Int = api
        .field_value(grid, metadata.grid_size as *mut FieldInfo)
        .map_err(|error| error.to_string())?;
    let size_x =
        u32::try_from(size.x).map_err(|_| format!("invalid terrain grid width {}", size.x))?;
    let size_y =
        u32::try_from(size.y).map_err(|_| format!("invalid terrain grid height {}", size.y))?;
    if !(1..=32).contains(&size_x) || !(1..=32).contains(&size_y) {
        return Err(format!("terrain grid size {size_x}x{size_y} exceeds 32x32"));
    }
    let rows_array: *mut Object = api
        .field_value(grid, metadata.grid_rows as *mut FieldInfo)
        .map_err(|error| error.to_string())?;
    let columns = api
        .value_array::<u32>(rows_array, 32)
        .map_err(|error| error.to_string())?;
    let rows = terrain_grid_rows_from_native_columns(&columns, size_x, size_y)?;
    Ok(TerrainGridState {
        origin_x: origin_x.raw,
        origin_y: origin_y.raw,
        size_x,
        size_y,
        rows,
    })
}

pub(crate) fn terrain_grid_rows_from_native_columns(
    columns: &[u32],
    size_x: u32,
    size_y: u32,
) -> Result<Vec<u32>, String> {
    let column_count =
        usize::try_from(size_x).map_err(|_| "grid width exceeds usize".to_owned())?;
    if columns.len() < column_count {
        return Err(format!(
            "terrain grid has {} columns, expected at least {column_count}",
            columns.len()
        ));
    }
    let valid_y_mask = if size_y == 32 {
        u32::MAX
    } else {
        u32::MAX << (32 - size_y)
    };
    if columns[..column_count]
        .iter()
        .any(|column| column & !valid_y_mask != 0)
    {
        return Err("terrain grid column uses bits outside size_y".into());
    }
    let row_count = usize::try_from(size_y).map_err(|_| "grid height exceeds usize".to_owned())?;
    let mut rows = vec![0_u32; row_count];
    for (x, column) in columns[..column_count].iter().copied().enumerate() {
        for (y, row) in rows.iter_mut().enumerate() {
            if column & (1_u32 << (31 - y)) != 0 {
                *row |= 1_u32 << x;
            }
        }
    }
    Ok(rows)
}

fn read_terrain_lifetime(
    api: Api,
    item: *mut Object,
    capture: &CaptureState,
) -> Result<Option<TerrainLogicLifetime>, String> {
    let is_ground_fire = api.object_class(item).map(|class| class as usize)
        == Some(capture.metadata.fight_ground_fire_class);
    let (time_field, life_time_field) = if is_ground_fire {
        (
            capture.metadata.ground_fire_time,
            capture.metadata.ground_fire_life_time,
        )
    } else {
        (
            capture.metadata.range_item_time,
            capture.metadata.range_item_life_time,
        )
    };
    let elapsed: i32 = api
        .field_value(item, time_field as *mut FieldInfo)
        .map_err(|error| error.to_string())?;
    let limit: i32 = api
        .field_value(item, life_time_field as *mut FieldInfo)
        .map_err(|error| error.to_string())?;
    if limit > 0 {
        if elapsed < 0 {
            return Err(format!("terrain lifetime elapsed {elapsed} is negative"));
        }
        Ok(Some(TerrainLogicLifetime { elapsed, limit }))
    } else {
        Ok(None)
    }
}

fn decode_terrain_type(value: i32) -> Result<TerrainType, String> {
    match value {
        0 => Ok(TerrainType::Fire),
        1 => Ok(TerrainType::Oil),
        2 => Ok(TerrainType::Fog),
        3 => Ok(TerrainType::Acid),
        4 => Ok(TerrainType::RecoveryZone),
        5 => Ok(TerrainType::FogSand),
        _ => Err(format!("unknown build-2259 RangeItemType {value}")),
    }
}

#[allow(clippy::too_many_lines)]
fn read_unit(
    api: Api,
    unit: *mut Object,
    team_id: u32,
    metadata: &Metadata,
    instrumentation_profile: Option<CaptureInstrumentationProfile>,
) -> Result<RawUnit, String> {
    if unit.is_null() {
        return Err("team contains a null unit".into());
    }
    let transform = invoke_object(api, unit, "GetFightTransform")
        .map_err(|error| format!("FightMech.GetFightTransform: {error}"))?;
    let fixed_position = invoke_value::<FixedVec3>(api, transform, "GetPositionInt3D")?;
    let fixed_rotation = invoke_value::<FixedPoint>(api, transform, "GetRotationInt")?;
    let motion = invoke_object(api, unit, "GetMotionController")?;
    let velocity = invoke_value::<FixedVec3>(api, motion, "GetCurrentVelocity")?;
    let active = invoke_value::<bool>(api, unit, "get_IsActive")?;
    let fsm: *mut Object = api
        .field_value(motion, metadata.motion_fsm as *mut FieldInfo)
        .map_err(|error| error.to_string())?;
    let current_motion_state = invoke_object(api, fsm, "GetCurrentState")?;
    let current_motion_class = api
        .object_class(current_motion_state)
        .ok_or_else(|| "native MotionFSM current state has no class".to_owned())?
        as usize;
    let motion_state = if current_motion_class == metadata.motion_idle_state_class {
        MotionState::Idle
    } else if current_motion_class == metadata.motion_move_state_class {
        MotionState::Moving
    } else if current_motion_class == metadata.motion_attack_state_class {
        MotionState::Attacking
    } else if current_motion_class == metadata.motion_stop_state_class {
        MotionState::Stopped
    } else {
        return Err("unsupported native MotionFSM current state".into());
    };
    let mut visibility = 0_i32;
    let targetable = api
        .invoke_value::<bool>(unit, "IsValidTarget", &mut [argument(&mut visibility)])
        .map_err(|error| error.to_string())?;
    let visibility = match invoke_value::<i32>(api, unit, "GetVisibility")? {
        0 => Visibility::Normal,
        1 => Visibility::Disappear,
        2 => Visibility::Stealth,
        3 => Visibility::Hide,
        value => return Err(format!("unsupported native visibility {value}")),
    };
    let position = vec3(fixed_position);
    let body_rotation = fixed_rotation.raw;
    let main_skill = invoke_object(api, unit, "GetMainSkill")?;
    let mech_lock_target = api
        .field_value::<*mut Object>(unit, metadata.fight_mech_lock_target as *mut FieldInfo)
        .map_err(|error| error.to_string())? as usize;
    let target_refs = match instrumentation_profile {
        Some(profile) if profile.includes_target_refs() => {
            let normal_skill_fields_available = api.class_is_or_inherits(
                api.object_class(main_skill).unwrap_or(ptr::null_mut()),
                metadata
                    .fight_skill_class
                    .expect("profile fields checked at capture start") as *mut _,
            );
            let (skill_lock_target, skill_attack_target) = if normal_skill_fields_available {
                (
                    api.field_value::<*mut Object>(
                        main_skill,
                        metadata
                            .fight_skill_lock_target
                            .expect("profile fields checked at capture start")
                            as *mut FieldInfo,
                    )
                    .map_err(|error| error.to_string())? as usize,
                    api.field_value::<*mut Object>(
                        main_skill,
                        metadata
                            .fight_skill_attack_target
                            .expect("profile fields checked at capture start")
                            as *mut FieldInfo,
                    )
                    .map_err(|error| error.to_string())? as usize,
                )
            } else {
                (0, 0)
            };
            Some(RawTargetRefs {
                mech_lock_target,
                normal_skill_fields_available,
                skill_lock_target,
                skill_attack_target,
            })
        }
        None => None,
        Some(_) => None,
    };
    let shield = invoke_object(api, unit, "GetEnergyShieldController")?;
    let max_energy = invoke_value::<i32>(api, shield, "GetMaxEnergy")?;
    let personal_shield = PersonalShieldState {
        active: invoke_value::<bool>(api, shield, "IsActive")?,
        enabled: invoke_value::<bool>(api, shield, "IsEnable")?,
        energy: GaugeI32 {
            current: invoke_value::<i32>(api, shield, "GetEnergy")?,
            maximum: max_energy,
        },
    };
    let buff_manager = invoke_object(api, unit, "GetBuffManager")?;
    let status_mask = u64::from(invoke_value::<bool>(api, buff_manager, "IsInvincible")?)
        | (u64::from(invoke_value::<bool>(api, buff_manager, "IsFreeze")?) << 1)
        | (u64::from(invoke_value::<bool>(api, unit, "IsTechnologyDisabled")?) << 2)
        | (u64::from(invoke_value::<bool>(api, unit, "IsRecoverDisabled")?) << 3);
    let buff_modifiers = read_buff_modifiers(api, buff_manager)?;
    let unit_dynamic_modifiers = read_unit_modifiers(api, unit)?;
    let (skill_dynamic_modifiers, weapon_aims, weapon_targets) = read_skill_state(api, unit)?;
    let formation = api
        .invoke(unit, "GetMechTeam", &mut [])
        .map_err(|error| error.to_string())?;
    let unit_type = invoke_value::<i32>(api, unit, "GetMechID")?;
    let life = invoke_value::<i32>(api, unit, "GetLife")?;
    let max_life = invoke_value::<i32>(api, unit, "GetMaxLife")?;
    Ok(RawUnit {
        pointer: unit as usize,
        formation: formation as usize,
        rvo_agent: if instrumentation_profile.is_some_and(|profile| profile.includes_rvo()) {
            read_rvo_agent(api, unit, metadata)?
        } else {
            None
        },
        mech_lock_target,
        weapon_targets,
        state: LiveUnitState {
            unit_id: 0,
            team_id,
            original_team_id: team_id,
            formation_id: 0,
            unit_type_id: u32::try_from(unit_type)
                .map_err(|_| format!("invalid unit type {unit_type}"))?,
            domain: if invoke_value::<bool>(api, unit, "IsFly")? {
                Domain::Air
            } else {
                Domain::Ground
            },
            position,
            body_rotation,
            velocity: vec3(velocity),
            motion_state,
            mech_lock_target: None,
            collision_radius: invoke_value::<FixedPoint>(api, unit, "GetRadius")?.raw,
            life: GaugeI32 {
                current: life,
                maximum: max_life,
            },
            active,
            targetable,
            visibility,
            status_mask,
            buff_modifiers,
            unit_dynamic_modifiers,
            skill_dynamic_modifiers,
            personal_shield,
            weapon_aims,
        },
        target_refs,
    })
}

fn read_buff_modifiers(api: Api, manager: *mut Object) -> Result<BuffModifierSet, String> {
    Ok(BuffModifierSet {
        move_speed_rate: named_rate(
            api,
            manager,
            "GetMoveSpeedChangeAddRate",
            "GetMoveSpeedChangeReduceRate",
        )?,
        move_speed_value: split_signed(invoke_value::<i32>(
            api,
            manager,
            "GetMoveSpeedChangeValue",
        )?)?,
        damage_rate: named_rate(
            api,
            manager,
            "GetDamageChangeAddRate",
            "GetDamageChangeReduceRate",
        )?,
        attack_interval_rate: named_rate(
            api,
            manager,
            "GetAttackIntervalChangeAddRate",
            "GetAttackIntervalChangeReduceRate",
        )?,
        extra_attack_interval_rate: named_rate(
            api,
            manager,
            "GetExtraAttackIntervalChangeAddRate",
            "GetExtraAttackIntervalChangeReduceRate",
        )?,
        amplify_damage_rate: named_rate(
            api,
            manager,
            "GetAmplifyDamageAddRate",
            "GetAmplifyDamageReduceRate",
        )?,
        attack_range_value: named_value(
            api,
            manager,
            "GetAttackRangeAddValue",
            "GetAttackRangeReduceValue",
        )?,
        extra_attack_range_value: named_value(
            api,
            manager,
            "GetExtraAttackRangeAddValue",
            "GetExtraAttackRangeReduceValue",
        )?,
        attack_range_rate: named_rate(
            api,
            manager,
            "GetAttackRangeAddRate",
            "GetAttackRangeReduceRate",
        )?,
        extra_attack_range_rate: named_rate(
            api,
            manager,
            "GetExtraAttackRangeAddRate",
            "GetExtraAttackRangeReduceRate",
        )?,
    })
}

fn read_unit_modifiers(api: Api, unit: *mut Object) -> Result<UnitDynamicModifierSet, String> {
    Ok(UnitDynamicModifierSet {
        gf_range_value: enum_fixed(
            api,
            unit,
            "GetDataFloat",
            "GameRiver.MechDataChangeFloat",
            0,
        )?,
        gf_life_time_value: enum_fixed(
            api,
            unit,
            "GetDataFloat",
            "GameRiver.MechDataChangeFloat",
            1,
        )?,
        mech_group_distance: enum_fixed(
            api,
            unit,
            "GetDataFloat",
            "GameRiver.MechDataChangeFloat",
            2,
        )?,
        life_rate: enum_rate(api, unit, "GameRiver.MechDataChangeFloatRate", 0)?,
        life_rate_by_kill_count: enum_rate(api, unit, "GameRiver.MechDataChangeFloatRate", 1)?,
        reduce_damage_from_remote: enum_rate(api, unit, "GameRiver.MechDataChangeFloatRate", 2)?,
        move_ability_exit_time_change_rate: enum_rate(
            api,
            unit,
            "GameRiver.MechDataChangeFloatRate",
            3,
        )?,
        move_speed_change_rate: enum_rate(api, unit, "GameRiver.MechDataChangeFloatRate", 4)?,
        amplify_damage_rate: enum_rate(api, unit, "GameRiver.MechDataChangeFloatRate", 5)?,
        move_speed_value: enum_int(api, unit, "GetDataInt", "GameRiver.MechDataChangeInt", 0)?,
        reduce_damage_value: enum_int(api, unit, "GetDataInt", "GameRiver.MechDataChangeInt", 1)?,
        child_inherit_technology_effect: enum_int(
            api,
            unit,
            "GetDataInt",
            "GameRiver.MechDataChangeInt",
            2,
        )?,
    })
}

type SkillState = (
    Vec<SkillNumericModifierState>,
    Vec<WeaponAimState>,
    Vec<usize>,
);

fn read_skill_state(api: Api, unit: *mut Object) -> Result<SkillState, String> {
    let all_skills = invoke_object(api, unit, "GetSkills")?;
    let count = list_count(api, all_skills, i32::from(u16::MAX))?;
    let capacity = usize::try_from(count).map_err(|_| "skill count is negative".to_owned())?;
    let mut modifiers = Vec::with_capacity(capacity);
    let mut aims = Vec::new();
    let mut targets = Vec::new();
    for slot in 0..count {
        let skill = list_item(api, all_skills, slot)?;
        let skill_slot = u16::try_from(slot).map_err(|_| "skill slot overflow".to_owned())?;
        modifiers.push(SkillNumericModifierState {
            skill_slot,
            modifiers: read_skill_modifiers(api, skill)?,
        });
        let target = api
            .invoke(skill, "GetAttackTarget", &mut [])
            .map_err(|error| error.to_string())? as usize;
        let weapons = invoke_object(api, skill, "GetWeapons")?;
        for weapon_slot in 0..list_count(api, weapons, 1_024)? {
            let weapon = list_item(api, weapons, weapon_slot)?;
            let weapon_data = invoke_object(api, weapon, "GetWeaponData")?;
            let weapon_index = invoke_value::<i32>(api, weapon_data, "get_Index")?;
            let transform = api
                .invoke(weapon, "GetFightTransform", &mut [])
                .map_err(|error| error.to_string())?;
            let pose = if transform.is_null() {
                None
            } else {
                Some(QPose {
                    position: vec3(invoke_value::<FixedVec3>(
                        api,
                        transform,
                        "GetPositionInt3D",
                    )?),
                    rotation: invoke_value::<FixedPoint>(api, transform, "GetRotationInt")?.raw,
                })
            };
            aims.push(WeaponAimState {
                skill_slot,
                weapon_index,
                attack_target: None,
                pose,
            });
            targets.push(target);
        }
    }
    let mut paired = aims.into_iter().zip(targets).collect::<Vec<_>>();
    paired.sort_by_key(|(aim, _)| (aim.skill_slot, aim.weapon_index));
    let (aims, targets) = paired.into_iter().unzip();
    Ok((modifiers, aims, targets))
}

fn read_skill_modifiers(api: Api, skill: *mut Object) -> Result<SkillDynamicModifierSet, String> {
    const FLOAT_TYPE: &str = "GameRiver.SkillDataChangeFloat";
    const RATE_TYPE: &str = "GameRiver.SkillDataChangeFloatRate";
    const INT_TYPE: &str = "GameRiver.SkillDataChangeInt";
    Ok(SkillDynamicModifierSet {
        min_attack_range_value: enum_fixed(api, skill, "GetData", FLOAT_TYPE, 0)?,
        attack_range_value: enum_fixed(api, skill, "GetData", FLOAT_TYPE, 1)?,
        attack_air_range_add_value: enum_fixed(api, skill, "GetData", FLOAT_TYPE, 2)?,
        attack_ground_range_add_value: enum_fixed(api, skill, "GetData", FLOAT_TYPE, 3)?,
        attack_interval_value: enum_fixed(api, skill, "GetData", FLOAT_TYPE, 4)?,
        damage_change_rate_ground: enum_fixed(api, skill, "GetData", FLOAT_TYPE, 5)?,
        damage_change_rate_air: enum_fixed(api, skill, "GetData", FLOAT_TYPE, 6)?,
        splash_range_value: enum_fixed(api, skill, "GetData", FLOAT_TYPE, 7)?,
        cb_life_recovery_rate: enum_fixed(api, skill, "GetData", FLOAT_TYPE, 8)?,
        projectile_speed_value: enum_fixed(api, skill, "GetData", FLOAT_TYPE, 9)?,
        attack_point_change_value: enum_fixed(api, skill, "GetData", FLOAT_TYPE, 10)?,
        projectile_duration_value: enum_fixed(api, skill, "GetData", FLOAT_TYPE, 11)?,
        projectile_random_range: enum_fixed(api, skill, "GetData", FLOAT_TYPE, 12)?,
        additional_damage_by_target_life: enum_fixed(api, skill, "GetData", FLOAT_TYPE, 13)?,
        damage_rate: enum_rate(api, skill, RATE_TYPE, 0)?,
        damage_rate_by_kill_count: enum_rate(api, skill, RATE_TYPE, 1)?,
        attack_range_rate: enum_rate(api, skill, RATE_TYPE, 2)?,
        attack_interval_rate: enum_rate(api, skill, RATE_TYPE, 3)?,
        damage_reduce_rate_base: enum_rate(api, skill, RATE_TYPE, 4)?,
        projectile_life_rate: enum_rate(api, skill, RATE_TYPE, 5)?,
        projectile_count_value: enum_int(api, skill, "GetData", INT_TYPE, 0)?,
        air_attack_value: enum_int(api, skill, "GetData", INT_TYPE, 1)?,
        ground_attack_value: enum_int(api, skill, "GetData", INT_TYPE, 2)?,
        attack_range_value_air: enum_int(api, skill, "GetData", INT_TYPE, 3)?,
        attack_range_value_ground: enum_int(api, skill, "GetData", INT_TYPE, 4)?,
        is_lock_target: enum_int(api, skill, "GetData", INT_TYPE, 5)?,
    })
}

fn named_rate(
    api: Api,
    object: *mut Object,
    add: &str,
    reduce: &str,
) -> Result<RateModifier, String> {
    let add = invoke_value::<FixedPoint>(api, object, add)?.raw;
    let native_reduce_factor = invoke_value::<FixedPoint>(api, object, reduce)?.raw;
    normalize_native_rate(add, native_reduce_factor)
}

fn normalize_native_rate(add: i64, native_reduce_factor: i64) -> Result<RateModifier, String> {
    if add < 0 || !(0..=FIXED_ONE_RAW).contains(&native_reduce_factor) {
        return Err("native rate aggregate is outside the supported range".to_owned());
    }
    let reduce = FIXED_ONE_RAW - native_reduce_factor;
    Ok(RateModifier { add, reduce })
}

fn named_value(
    api: Api,
    object: *mut Object,
    add: &str,
    reduce: &str,
) -> Result<ValueModifier, String> {
    let add = invoke_value::<i32>(api, object, add)?;
    let reduce = invoke_value::<i32>(api, object, reduce)?;
    if add < 0 || reduce < 0 {
        return Err("native value add/reduce aggregate is negative".to_owned());
    }
    Ok(ValueModifier { add, reduce })
}

fn split_signed(value: i32) -> Result<ValueModifier, String> {
    if value >= 0 {
        Ok(ValueModifier {
            add: value,
            reduce: 0,
        })
    } else {
        Ok(ValueModifier {
            add: 0,
            reduce: value
                .checked_abs()
                .ok_or_else(|| "i32 modifier magnitude overflow".to_owned())?,
        })
    }
}

fn enum_rate(
    api: Api,
    object: *mut Object,
    parameter_type: &str,
    index: i32,
) -> Result<RateModifier, String> {
    let add = enum_fixed(api, object, "GetDataFloatAddRate", parameter_type, index)?;
    let native_reduce_factor =
        enum_fixed(api, object, "GetDataFloatReduceRate", parameter_type, index)?;
    normalize_native_rate(add, native_reduce_factor)
}

fn enum_fixed(
    api: Api,
    object: *mut Object,
    method_name: &str,
    parameter_type: &str,
    index: i32,
) -> Result<i64, String> {
    Ok(invoke_enum_value::<FixedPoint>(api, object, method_name, parameter_type, index)?.raw)
}

fn enum_int(
    api: Api,
    object: *mut Object,
    method_name: &str,
    parameter_type: &str,
    index: i32,
) -> Result<i32, String> {
    invoke_enum_value(api, object, method_name, parameter_type, index)
}

fn invoke_enum_value<T: Copy>(
    api: Api,
    object: *mut Object,
    method_name: &str,
    parameter_type: &str,
    mut index: i32,
) -> Result<T, String> {
    let method = api
        .method_with_parameter_types(object, method_name, &[parameter_type])
        .map_err(|error| error.to_string())?;
    let boxed = api
        .invoke_raw(method, object.cast(), &mut [argument(&mut index)])
        .map_err(|error| error.to_string())?;
    api.unbox(boxed, method_name)
        .map_err(|error| error.to_string())
}

fn read_rvo_agent(
    api: Api,
    actor: *mut Object,
    metadata: &Metadata,
) -> Result<Option<usize>, String> {
    let rvo = metadata
        .rvo
        .ok_or_else(|| "native RVO metadata is unavailable".to_owned())?;
    let controller: *mut Object = api
        .field_value(actor, rvo.fight_actor_rvo_controller as *mut FieldInfo)
        .map_err(|error| error.to_string())?;
    if controller.is_null() {
        return Ok(None);
    }
    let owner: *mut Object = api
        .field_value(controller, rvo.rvo_controller_owner as *mut FieldInfo)
        .map_err(|error| error.to_string())?;
    if owner != actor {
        return Err("RVOControllerFixed.owner does not reference its FightActor".into());
    }
    let agent: *mut Object = api
        .field_value(controller, rvo.rvo_controller_agent as *mut FieldInfo)
        .map_err(|error| error.to_string())?;
    if agent.is_null() {
        return Ok(None);
    }
    Ok(Some(agent as usize))
}

fn resolve_rvo_observation(
    target_refs: TargetRefsObservation,
    native_tick: u64,
    capture: &mut CaptureState,
) -> Result<TargetRefsRvoObservation, String> {
    Ok(TargetRefsRvoObservation {
        target_refs,
        rvo_updates: resolve_rvo_updates(native_tick, capture)?,
    })
}

fn resolve_rvo_updates(
    native_tick: u64,
    capture: &mut CaptureState,
) -> Result<Vec<RvoUpdateObservation>, String> {
    let ready: BTreeSet<u64> = capture
        .rvo_update_publish_native_ticks
        .iter()
        .filter_map(|(&update_ordinal, &publish_tick)| {
            (publish_tick <= native_tick).then_some(update_ordinal)
        })
        .collect();
    let mut updates: BTreeMap<u64, RvoUpdateObservation> = capture
        .rvo_update_publish_native_ticks
        .iter()
        .filter_map(|(&update_ordinal, &publish_native_tick)| {
            (ready.contains(&update_ordinal) && rvo_update_selected(capture, update_ordinal))
                .then_some((update_ordinal, (update_ordinal, publish_native_tick)))
        })
        .map(|(update_ordinal, (_, publish_native_tick))| {
            let start_native_tick = capture
                .rvo_update_start_native_ticks
                .get(&update_ordinal)
                .copied()
                .ok_or_else(|| format!("native RVO update {update_ordinal} lost its start tick"))?;
            let double_buffering = capture
                .rvo_update_modes
                .get(&update_ordinal)
                .copied()
                .ok_or_else(|| format!("native RVO update {update_ordinal} lost its mode"))?;
            let symmetry_breaking_bias_raw = capture
                .rvo_update_symmetry_breaking_biases
                .get(&update_ordinal)
                .copied()
                .ok_or_else(|| {
                    format!("native RVO update {update_ordinal} lost its symmetry bias")
                })?
                .raw;
            if !capture
                .rvo_update_multithreaded
                .contains_key(&update_ordinal)
            {
                return Err(format!(
                    "native RVO update {update_ordinal} lost its worker mode"
                ));
            }
            Ok((
                update_ordinal,
                RvoUpdateObservation {
                    update_ordinal,
                    start_native_tick,
                    publish_native_tick,
                    double_buffering,
                    multithreaded: capture.rvo_update_multithreaded[&update_ordinal],
                    symmetry_breaking_bias_raw,
                    agents: Vec::new(),
                    published_agents: Vec::new(),
                    neighbour_sets: Vec::new(),
                    vo_buffers: Vec::new(),
                    opponent_vos: Vec::new(),
                },
            ))
        })
        .collect::<Result<_, String>>()?;
    let raw_agent_sets = std::mem::take(&mut capture.rvo_agent_sets);
    for (update_ordinal, agents) in raw_agent_sets {
        if !ready.contains(&update_ordinal) {
            capture.rvo_agent_sets.insert(update_ordinal, agents);
            continue;
        }
        let mut resolved = Vec::with_capacity(agents.len());
        for agent in agents {
            resolved.push(resolve_rvo_agent_state(agent, capture)?);
        }
        updates
            .get_mut(&update_ordinal)
            .ok_or_else(|| format!("unknown native RVO update {update_ordinal}"))?
            .agents = resolved;
    }
    for (ordinal, agents) in std::mem::take(&mut capture.rvo_published_agent_sets) {
        if !ready.contains(&ordinal) {
            capture.rvo_published_agent_sets.insert(ordinal, agents);
            continue;
        }
        let resolved = agents
            .into_iter()
            .map(|agent| resolve_rvo_agent_state(agent, capture))
            .collect::<Result<Vec<_>, _>>()?;
        updates
            .get_mut(&ordinal)
            .ok_or_else(|| format!("unknown published RVO update {ordinal}"))?
            .published_agents = resolved;
    }
    let (mut raw_neighbour_sets, pending_neighbour_sets): (Vec<_>, Vec<_>) =
        std::mem::take(&mut capture.rvo_neighbour_sets)
            .into_iter()
            .partition(|set| ready.contains(&set.update_ordinal));
    capture.rvo_neighbour_sets = pending_neighbour_sets;
    raw_neighbour_sets.sort_by_key(|set| set.source_call_ordinal);
    let mut update_sources = BTreeSet::new();
    for set in raw_neighbour_sets {
        let source = resolve_rvo_agent_ref(set.source, capture)?;
        let mut neighbours = Vec::with_capacity(set.neighbours.len());
        for (ordinal, (target, distance_sq_raw)) in set.neighbours.into_iter().enumerate() {
            neighbours.push(RvoNeighbourObservation {
                ordinal: u32::try_from(ordinal)
                    .map_err(|_| "RVO neighbour ordinal overflow".to_owned())?,
                target: resolve_rvo_agent_ref(target, capture)?,
                distance_sq_raw,
            });
        }
        let observation = RvoNeighbourSetObservation {
            source_call_ordinal: set.source_call_ordinal,
            source,
            neighbour_count: u32::try_from(neighbours.len())
                .map_err(|_| "RVO neighbour count overflow".to_owned())?,
            neighbours,
        };
        update_sources.insert((set.update_ordinal, set.source));
        updates
            .get_mut(&set.update_ordinal)
            .ok_or_else(|| format!("unknown native RVO update {}", set.update_ordinal))?
            .neighbour_sets
            .push(observation);
    }
    let (mut raw_vo_buffers, pending_vo_buffers): (Vec<_>, Vec<_>) =
        std::mem::take(&mut capture.rvo_vo_buffers)
            .into_iter()
            .partition(|observation| ready.contains(&observation.update_ordinal));
    capture.rvo_vo_buffers = pending_vo_buffers;
    raw_vo_buffers.sort_by_key(|observation| observation.call_ordinal);
    for observation in raw_vo_buffers {
        if !update_sources.contains(&(observation.update_ordinal, observation.source)) {
            return Err(format!(
                "VO buffer call {} has no CalculateNeighbours source in RVO update {}",
                observation.call_ordinal, observation.update_ordinal
            ));
        }
        let resolved = RvoVoBufferObservation {
            call_ordinal: observation.call_ordinal,
            source: resolve_rvo_agent_ref(observation.source, capture)?,
            vos: observation
                .vos
                .into_iter()
                .map(rvo_vo_observation)
                .collect(),
        };
        updates
            .get_mut(&observation.update_ordinal)
            .ok_or_else(|| format!("unknown native RVO update {}", observation.update_ordinal))?
            .vo_buffers
            .push(resolved);
    }
    let (mut raw_opponent_vos, pending_opponent_vos): (Vec<_>, Vec<_>) =
        std::mem::take(&mut capture.opponent_vos)
            .into_iter()
            .partition(|observation| ready.contains(&observation.update_ordinal));
    capture.opponent_vos = pending_opponent_vos;
    raw_opponent_vos.sort_by_key(|observation| observation.call_ordinal);
    for observation in raw_opponent_vos {
        if !update_sources.contains(&(observation.update_ordinal, observation.source)) {
            return Err(format!(
                "opponent VO call {} has no CalculateNeighbours source in RVO update {}",
                observation.call_ordinal, observation.update_ordinal
            ));
        }
        let source = resolve_rvo_agent_ref(observation.source, capture)?;
        let resolved = RvoOpponentVoObservation {
            update_ordinal: observation.update_ordinal,
            call_ordinal: observation.call_ordinal,
            source,
            target: resolve_rvo_agent_ref(observation.target, capture)?,
            vo_buffer_length_before: u32::try_from(observation.vo_buffer_length_before)
                .map_err(|_| "negative opponent VO buffer length".to_owned())?,
            vo_buffer_length_after: u32::try_from(observation.vo_buffer_length_after)
                .map_err(|_| "negative opponent VO buffer length".to_owned())?,
            appended_colliding: observation.appended_colliding,
        };
        updates
            .get_mut(&observation.update_ordinal)
            .ok_or_else(|| format!("unknown native RVO update {}", observation.update_ordinal))?
            .opponent_vos
            .push(resolved);
    }
    let updates = updates.into_values().collect();
    for update_ordinal in ready {
        capture.rvo_update_modes.remove(&update_ordinal);
        capture
            .rvo_update_symmetry_breaking_biases
            .remove(&update_ordinal);
        capture
            .rvo_update_start_native_ticks
            .remove(&update_ordinal);
        capture
            .rvo_update_publish_native_ticks
            .remove(&update_ordinal);
        capture.rvo_update_multithreaded.remove(&update_ordinal);
    }
    Ok(updates)
}

fn resolve_rvo_agent_state(
    agent: NativeRvoAgentState,
    capture: &mut CaptureState,
) -> Result<RvoAgentObservation, String> {
    Ok(RvoAgentObservation {
        ordinal: agent.ordinal,
        agent: resolve_rvo_agent_ref(agent.pointer, capture)?,
        radius_inner_raw: agent.radius_inner.raw,
        size: agent.size,
        radius_outer_raw: agent.radius_outer.raw,
        max_speed_raw: agent.max_speed.raw,
        desired_speed_raw: agent.desired_speed.raw,
        agent_time_horizon_raw: agent.agent_time_horizon.raw,
        priority_raw: agent.priority.raw,
        published_calculated_speed_raw: agent.published_calculated_speed.raw,
        current_velocity_x_raw: agent.current_velocity.x.raw,
        current_velocity_y_raw: agent.current_velocity.y.raw,
        desired_velocity_x_raw: agent.desired_velocity.x.raw,
        desired_velocity_y_raw: agent.desired_velocity.y.raw,
        desired_target_x_raw: agent.desired_target.x.raw,
        desired_target_y_raw: agent.desired_target.y.raw,
        calculated_target_x_raw: agent.calculated_target.x.raw,
        calculated_target_y_raw: agent.calculated_target.y.raw,
        locked: agent.locked,
        layer: agent.layer,
        collides_with: agent.collides_with,
        max_neighbours: agent.max_neighbours,
        main_layer: agent.main_layer,
        sync_main_layer: agent.sync_main_layer,
        group: agent.group,
        sync_group: agent.sync_group,
        ignore_same_group: agent.ignore_same_group,
        sync_ignore_same_group: agent.sync_ignore_same_group,
        team_id: agent.team.id,
        team_radius_raw: agent.team.radius.raw,
        sync_team_id: agent.sync_team.id,
        sync_team_radius_raw: agent.sync_team.radius.raw,
        position_x_raw: agent.position.x.raw,
        position_y_raw: agent.position.y.raw,
    })
}

fn rvo_vo_observation(vo: NativeRvoVo) -> RvoVoObservation {
    RvoVoObservation {
        line1_x_raw: vo.line1.x.raw,
        line1_y_raw: vo.line1.y.raw,
        line2_x_raw: vo.line2.x.raw,
        line2_y_raw: vo.line2.y.raw,
        dir1_x_raw: vo.dir1.x.raw,
        dir1_y_raw: vo.dir1.y.raw,
        dir2_x_raw: vo.dir2.x.raw,
        dir2_y_raw: vo.dir2.y.raw,
        cutoff_line_x_raw: vo.cutoff_line.x.raw,
        cutoff_line_y_raw: vo.cutoff_line.y.raw,
        cutoff_dir_x_raw: vo.cutoff_dir.x.raw,
        cutoff_dir_y_raw: vo.cutoff_dir.y.raw,
        circle_center_x_raw: vo.circle_center.x.raw,
        circle_center_y_raw: vo.circle_center.y.raw,
        colliding: vo.colliding,
        radius_raw: vo.radius.raw,
        weight_factor_raw: vo.weight_factor.raw,
        weight_bonus_raw: vo.weight_bonus.raw,
        segment_start_x_raw: vo.segment_start.x.raw,
        segment_start_y_raw: vo.segment_start.y.raw,
        segment_end_x_raw: vo.segment_end.x.raw,
        segment_end_y_raw: vo.segment_end.y.raw,
        segment: vo.segment,
    }
}

fn resolve_rvo_agent_ref(
    pointer: usize,
    capture: &mut CaptureState,
) -> Result<RvoAgentRefObservation, String> {
    if let Some(reference) = capture.rvo_agent_refs.get(&pointer).copied() {
        return Ok(RvoAgentRefObservation::Entity(reference));
    }
    if let Some(owner) = capture.rvo_agent_owners.get(&pointer).copied()
        && let Some(reference) = object_ref_from_pointer(owner, capture)
    {
        return Ok(RvoAgentRefObservation::Entity(reference));
    }
    let internal_agent_ordinal = match capture.rvo_internal_agent_ids.get(&pointer) {
        Some(ordinal) => *ordinal,
        None => {
            let ordinal = capture.next_rvo_internal_agent_id;
            capture.next_rvo_internal_agent_id = ordinal
                .checked_add(1)
                .ok_or_else(|| "RVO internal agent identity overflow".to_owned())?;
            capture.rvo_internal_agent_ids.insert(pointer, ordinal);
            ordinal
        }
    };
    Ok(RvoAgentRefObservation::Internal {
        internal_agent_ordinal,
    })
}

fn read_building(
    api: Api,
    building: *mut Object,
    team_id: u32,
    metadata: &Metadata,
    instrumentation_profile: Option<CaptureInstrumentationProfile>,
) -> Result<RawBuilding, String> {
    let transform = invoke_object(api, building, "GetFightTransform")
        .map_err(|error| format!("FightCrystal.GetFightTransform: {error}"))?;
    let position = vec3(invoke_value::<FixedVec3>(
        api,
        transform,
        "GetPositionInt3D",
    )?);
    let bounds = invoke_value::<FixedRect>(api, building, "GetBoundsRect")?;
    let building_type = invoke_value::<i32>(api, building, "GetBuildingType")?;
    let life = invoke_value::<i32>(api, building, "GetLife")?;
    let max_life = invoke_value::<i32>(api, building, "GetMaxLife")?;
    let available = invoke_value::<bool>(api, building, "IsAvaliable")?;
    let mut visibility = 0_i32;
    let targetable = api
        .invoke_value::<bool>(building, "IsValidTarget", &mut [argument(&mut visibility)])
        .map_err(|error| error.to_string())?;
    let data = invoke_object(api, building, "GetBuildingData")?;
    let collision_enabled = invoke_value::<bool>(api, data, "get_EnableCollision")?;
    Ok(RawBuilding {
        pointer: building as usize,
        rvo_agent: if instrumentation_profile.is_some_and(|profile| profile.includes_rvo()) {
            read_rvo_agent(api, building, metadata)?
        } else {
            None
        },
        state: BuildingState {
            building_id: 0,
            team_id,
            building_type_id: u32::try_from(building_type)
                .map_err(|_| format!("invalid building type {building_type}"))?,
            position,
            bounds_width: bounds.size.x.raw,
            bounds_height: bounds.size.y.raw,
            life: GaugeI32 {
                current: life,
                maximum: max_life,
            },
            available,
            targetable,
            collision_enabled,
        },
    })
}

fn read_team_shields(
    api: Api,
    shield_system: *mut Object,
    fight_group: *mut Object,
    team_controller: *mut Object,
    team_id: u32,
    metadata: &Metadata,
) -> Result<Vec<RawShield>, String> {
    let all = api
        .invoke(
            shield_system,
            "GetEnergyShields",
            &mut [object_argument(fight_group)],
        )
        .map_err(|error| format!("AdvancedEnergyShieldSystem.GetEnergyShields: {error}"))?;
    let active = api
        .invoke(
            shield_system,
            "GetActiveEnergyShields",
            &mut [object_argument(fight_group)],
        )
        .map_err(|error| format!("AdvancedEnergyShieldSystem.GetActiveEnergyShields: {error}"))?;
    if all.is_null() || active.is_null() {
        return Err("AdvancedEnergyShieldSystem returned a null shield list".into());
    }
    let mut active_orders = BTreeMap::new();
    for index in 0..list_count(api, active, 100_000)? {
        let shield = list_item(api, active, index)?;
        if shield.is_null() {
            return Err(format!("active shield list contains null at index {index}"));
        }
        if active_orders
            .insert(
                shield as usize,
                u32::try_from(index).map_err(|_| "active shield order overflow".to_owned())?,
            )
            .is_some()
        {
            return Err("active shield list contains a duplicate object".into());
        }
    }
    let mut result = Vec::new();
    let mut all_pointers = BTreeSet::new();
    for index in 0..list_count(api, all, 100_000)? {
        let shield = list_item(api, all, index)?;
        if shield.is_null() {
            return Err(format!("shield list contains null at index {index}"));
        }
        let pointer = shield as usize;
        if !all_pointers.insert(pointer) {
            return Err("shield list contains a duplicate object".into());
        }
        let active = invoke_value::<bool>(api, shield, "get_IsActive")
            .map_err(|error| format!("FightEnergyShield.get_IsActive: {error}"))?;
        let active_order = active_orders.get(&pointer).copied();
        if active != active_order.is_some() {
            return Err(format!(
                "FightEnergyShield at 0x{pointer:x} active flag disagrees with active list membership"
            ));
        }
        let native_team = invoke_object(api, shield, "GetTeamController")
            .map_err(|error| format!("FightEnergyShield.GetTeamController: {error}"))?;
        if native_team != team_controller {
            return Err(format!(
                "FightEnergyShield at 0x{pointer:x} returned a different team controller"
            ));
        }
        let native_team_index = invoke_value::<i32>(api, native_team, "GetTeamIndex")
            .map_err(|error| format!("FightTeamController.GetTeamIndex: {error}"))?;
        if u32::try_from(native_team_index).ok() != Some(team_id) {
            return Err(format!(
                "FightEnergyShield at 0x{pointer:x} belongs to team {native_team_index}, enumerated under {team_id}"
            ));
        }
        let data = invoke_object(api, shield, "get_EnergyShieldData")
            .map_err(|error| format!("FightEnergyShield.get_EnergyShieldData: {error}"))?;
        let source_kind = shield_source_kind(api, data, metadata)?;
        let short_lived = invoke_value::<bool>(api, shield, "IsShortLifeTime")
            .map_err(|error| format!("FightEnergyShield.IsShortLifeTime: {error}"))?;
        let reset_next_round = invoke_value::<bool>(api, shield, "IsResetNextRound")
            .map_err(|error| format!("FightEnergyShield.IsResetNextRound: {error}"))?;
        let round_policy = if short_lived {
            ShieldRoundPolicy::DestroyAtRoundEnd
        } else if reset_next_round {
            ShieldRoundPolicy::ResetToMax
        } else {
            ShieldRoundPolicy::RetainState
        };
        let owner = api
            .invoke(shield, "GetOwner", &mut [])
            .map_err(|error| format!("FightEnergyShield.GetOwner: {error}"))?
            as usize;
        let transform = invoke_object(api, shield, "GetFightTransform")
            .map_err(|error| format!("FightEnergyShield.GetFightTransform: {error}"))?;
        let position = vec3(
            invoke_value::<FixedVec3>(api, transform, "GetPositionInt3D")
                .map_err(|error| format!("FightTransform.GetPositionInt3D: {error}"))?,
        );
        result.push(RawShield {
            pointer,
            owner,
            state: ShieldState {
                shield_id: 0,
                team_id,
                source_kind,
                owner: None,
                position,
                radius: invoke_value::<FixedPoint>(api, shield, "GetRadius")
                    .map_err(|error| format!("FightEnergyShield.GetRadius: {error}"))?
                    .raw,
                energy: GaugeI32 {
                    current: invoke_value::<i32>(api, shield, "GetEnergy")
                        .map_err(|error| format!("FightEnergyShield.GetEnergy: {error}"))?,
                    maximum: invoke_value::<i32>(api, shield, "GetMaxEnergy")
                        .map_err(|error| format!("FightEnergyShield.GetMaxEnergy: {error}"))?,
                },
                round_policy,
                active,
                active_order,
            },
        });
    }
    for pointer in active_orders.keys() {
        if !all_pointers.contains(pointer) {
            return Err(format!(
                "active shield 0x{pointer:x} is absent from the full shield list"
            ));
        }
    }
    Ok(result)
}

fn shield_source_kind(
    api: Api,
    data: *mut Object,
    metadata: &Metadata,
) -> Result<ShieldSourceKind, String> {
    let class =
        api.object_class(data)
            .ok_or_else(|| "shield data source has no runtime class".to_owned())? as usize;
    if class == metadata.energy_shield_contraption_class {
        Ok(ShieldSourceKind::Contraption)
    } else if class == metadata.commander_energy_shield_class {
        Ok(ShieldSourceKind::CommanderSkill)
    } else if class == metadata.owner_advanced_shield_class {
        Ok(ShieldSourceKind::OwnerAdvanced)
    } else if class == metadata.spawned_temporary_shield_class {
        Ok(ShieldSourceKind::SpawnedTemporary)
    } else {
        Err(format!(
            "unsupported shield data source {}",
            api.object_class_name(data)
        ))
    }
}

fn read_projectiles(
    runtime: &Runtime,
    capture: &mut CaptureState,
) -> Result<Vec<ProjectileState>, String> {
    let fight = runtime.current_fight();
    let modules = runtime
        .api
        .invoke(fight, "GetModules", &mut [])
        .map_err(|error| error.to_string())?;
    let system = find_module(
        runtime.api,
        modules,
        capture.metadata.projectile_system_class,
        "ProjectileSystem",
    )?;
    let controllers: *mut Object = runtime
        .api
        .field_value(
            system,
            capture.metadata.projectile_controllers as *mut FieldInfo,
        )
        .map_err(|error| error.to_string())?;
    let mut seen = BTreeSet::new();
    let mut projectiles = Vec::new();
    for index in 0..list_count(runtime.api, controllers, 100_000)? {
        let controller = list_item(runtime.api, controllers, index)?;
        let projectile = invoke_object(runtime.api, controller, "GetFightProjectile")?;
        let pointer = projectile as usize;
        seen.insert(pointer);
        let id = match capture.projectile_ids.get(&pointer) {
            Some(id) => *id,
            None => allocate(&mut capture.next_projectile_id, "projectile")?,
        };
        capture.projectile_ids.entry(pointer).or_insert(id);
        let state = read_projectile(runtime.api, controller, projectile, id, capture)?;
        capture
            .object_teams
            .insert(ObjectRef::new(ObjectKind::Projectile, id), state.team_id);
        projectiles.push(state);
    }
    capture
        .projectile_ids
        .retain(|pointer, _| seen.contains(pointer));
    Ok(projectiles)
}

fn find_module(
    api: Api,
    modules: *mut Object,
    class: usize,
    label: &str,
) -> Result<*mut Object, String> {
    for index in 0..list_count(api, modules, 128)? {
        let candidate = list_item(api, modules, index)?;
        if api.object_class(candidate).map(|value| value as usize) == Some(class) {
            return Ok(candidate);
        }
    }
    Err(format!("{label} module is unavailable"))
}

fn read_projectile(
    api: Api,
    controller: *mut Object,
    projectile: *mut Object,
    id: u64,
    capture: &CaptureState,
) -> Result<ProjectileState, String> {
    let owner = api
        .invoke(projectile, "GetOwner", &mut [])
        .map_err(|error| error.to_string())?;
    let target = api
        .invoke(projectile, "GetTarget", &mut [])
        .map_err(|error| error.to_string())?;
    let owner_ref = object_ref_from_pointer(owner as usize, capture);
    let target_ref = object_ref_from_pointer(target as usize, capture);
    let team_controller = invoke_object(api, controller, "GetTeamController")?;
    let team_index = invoke_value::<i32>(api, team_controller, "GetTeamIndex")?;
    let team_id = u32::try_from(team_index)
        .map_err(|_| format!("invalid projectile team index {team_index}"))?;
    let transform = invoke_object(api, projectile, "GetFightTransform")
        .map_err(|error| format!("FightProjectile.GetFightTransform: {error}"))?;
    let position = vec3(invoke_value::<FixedVec3>(
        api,
        transform,
        "GetPositionInt3D",
    )?);
    let orientation = invoke_value::<FixedPoint>(api, transform, "GetRotationInt")?.raw;
    let target_info = invoke_object(api, projectile, "GetTargetInfo")?;
    let cached_target_position = vec3(invoke_value::<FixedVec3>(api, target_info, "GetPosition")?);
    let cached_target_radius = invoke_value::<FixedPoint>(api, target_info, "GetRadius")?.raw;
    let life = invoke_value::<i32>(api, projectile, "GetLife")?;
    let max_life = invoke_value::<i32>(api, projectile, "GetMaxLife")?;
    let in_energy_shields: *mut Object = api
        .field_value(
            controller,
            capture.metadata.projectile_in_energy_shields as *mut FieldInfo,
        )
        .map_err(|error| error.to_string())?;
    let mut spawn_containing_shields = Vec::new();
    for index in 0..list_count(api, in_energy_shields, 100_000)? {
        let shield = list_item(api, in_energy_shields, index)?;
        let shield_id = capture.shield_ids.get(&(shield as usize)).ok_or_else(|| {
            format!(
                "ProjectileController.inEnergyShields references unknown shield 0x{:x}",
                shield as usize
            )
        })?;
        spawn_containing_shields.push(ObjectRef::new(ObjectKind::Shield, *shield_id));
    }
    spawn_containing_shields.sort_unstable();
    spawn_containing_shields.dedup();
    Ok(ProjectileState {
        projectile_id: id,
        team_id,
        owner: owner_ref,
        position,
        orientation,
        target: target_ref,
        cached_target_position,
        cached_target_radius,
        released: invoke_value::<bool>(api, projectile, "IsRelease")?,
        life: GaugeI32 {
            current: life,
            maximum: max_life,
        },
        spawn_containing_shields,
    })
}

fn transition_events(traces: &[NativeTrace], capture: &CaptureState) -> TransitionEvents {
    let mut events = Vec::new();
    for trace in traces {
        match *trace {
            NativeTrace::ProjectileReleased {
                projectile_id,
                owner,
                target,
                skill_slot,
                weapon_index,
            } => {
                let source = object_ref_from_pointer(owner, capture);
                events.push(event(
                    Some(ObjectRef::new(ObjectKind::Projectile, projectile_id)),
                    source,
                    source.and_then(|value| capture.object_teams.get(&value).copied()),
                    object_ref_from_pointer(target, capture),
                    EventPayload::ProjectileReleased {
                        skill_slot,
                        weapon_index,
                    },
                ));
            }
            NativeTrace::ProjectileRemoved {
                projectile_id,
                owner,
                target,
                position,
                intercepted,
                absorbed_by,
            } => {
                let subject = Some(ObjectRef::new(ObjectKind::Projectile, projectile_id));
                let source = object_ref_from_pointer(owner, capture);
                events.push(event(
                    subject,
                    source,
                    source.and_then(|value| capture.object_teams.get(&value).copied()),
                    object_ref_from_pointer(target, capture),
                    EventPayload::ProjectileRemoved {
                        position,
                        intercepted,
                        absorbed_by,
                    },
                ));
            }
            NativeTrace::Damage {
                source,
                source_team_id,
                target,
                amount,
            } => {
                events.push(event(
                    None,
                    source,
                    source_team_id.or_else(|| {
                        source.and_then(|value| capture.object_teams.get(&value).copied())
                    }),
                    Some(target),
                    EventPayload::Damage { amount },
                ));
            }
            NativeTrace::UnitDied {
                unit_id,
                position,
                source,
                source_team_id,
            } => {
                events.push(event(
                    Some(ObjectRef::new(ObjectKind::Unit, unit_id)),
                    source,
                    source_team_id.or_else(|| {
                        source.and_then(|value| capture.object_teams.get(&value).copied())
                    }),
                    None,
                    EventPayload::UnitDied { position },
                ));
            }
            NativeTrace::BuildingDestroyed {
                building_id,
                position,
            } => {
                events.push(event(
                    Some(ObjectRef::new(ObjectKind::Building, building_id)),
                    None,
                    None,
                    None,
                    EventPayload::BuildingDestroyed { position },
                ));
            }
            NativeTrace::ShieldCreated {
                shield_id,
                team_id,
                source_kind,
                position,
            } => {
                events.push(event(
                    Some(ObjectRef::new(ObjectKind::Shield, shield_id)),
                    None,
                    None,
                    None,
                    EventPayload::ShieldCreated {
                        team_id,
                        source_kind,
                        position,
                    },
                ));
            }
            NativeTrace::ShieldDestroyed {
                shield_id,
                position,
            } => {
                events.push(event(
                    Some(ObjectRef::new(ObjectKind::Shield, shield_id)),
                    None,
                    None,
                    None,
                    EventPayload::ShieldDestroyed {
                        position,
                        reason: ShieldDestroyedReason::Unknown,
                    },
                ));
            }
            NativeTrace::TerrainCreated {
                terrain_id,
                team_id,
                terrain_type,
                position,
                radius,
            } => {
                events.push(event(
                    Some(ObjectRef::new(ObjectKind::Terrain, terrain_id)),
                    None,
                    team_id,
                    None,
                    EventPayload::TerrainCreated {
                        team_id,
                        terrain_type,
                        position,
                        radius,
                    },
                ));
            }
            NativeTrace::TerrainRemoved {
                terrain_id,
                position,
            } => {
                events.push(event(
                    Some(ObjectRef::new(ObjectKind::Terrain, terrain_id)),
                    None,
                    None,
                    None,
                    EventPayload::TerrainRemoved {
                        position,
                        reason: TerrainRemovedReason::Unknown,
                    },
                ));
            }
        }
    }
    TransitionEvents { events }
}

fn object_ref_from_pointer(pointer: usize, capture: &CaptureState) -> Option<ObjectRef> {
    if pointer == 0 {
        return None;
    }
    capture
        .unit_ids
        .get(&pointer)
        .map(|id| ObjectRef::new(ObjectKind::Unit, *id))
        .or_else(|| {
            capture
                .building_ids
                .get(&pointer)
                .map(|id| ObjectRef::new(ObjectKind::Building, *id))
        })
        .or_else(|| {
            capture
                .projectile_ids
                .get(&pointer)
                .map(|id| ObjectRef::new(ObjectKind::Projectile, *id))
        })
        .or_else(|| {
            capture
                .shield_ids
                .get(&pointer)
                .map(|id| ObjectRef::new(ObjectKind::Shield, *id))
        })
        .or_else(|| {
            capture
                .terrain_ids
                .get(&pointer)
                .map(|id| ObjectRef::new(ObjectKind::Terrain, *id))
        })
}

fn resolve_target_ref(
    api: Api,
    pointer: usize,
    field: &str,
    capture: &CaptureState,
) -> Result<Option<ObjectRef>, String> {
    if pointer == 0 {
        return Ok(None);
    }
    object_ref_from_pointer(pointer, capture)
        .map(Some)
        .ok_or_else(|| {
            let class = api.object_class_name(pointer as *mut Object);
            format!(
                "{field} references {class} at 0x{pointer:x}, absent from the MCFR world snapshot"
            )
        })
}

fn drain_selector_score_calls(
    capture: &mut CaptureState,
) -> Result<SelectorScoreObservation, String> {
    let score_calculations = std::mem::take(&mut capture.selector_score_calculations)
        .into_iter()
        .map(|calculation| SelectorScoreCalculation {
            invocation_ordinal: calculation.invocation_ordinal,
            distance_raw: calculation.distance_raw,
            distance_score_raw: calculation.distance_score_raw,
            angle_raw: calculation.angle_raw,
            angle_score_raw: calculation.angle_score_raw,
            max_attack_range_raw: calculation.max_attack_range_raw,
            source_rotation_raw: calculation.source_rotation_raw,
            min_rotation_raw: calculation.min_rotation_raw,
            max_rotation_raw: calculation.max_rotation_raw,
            is_left_side: calculation.is_left_side,
            score_raw: calculation.score_raw,
        })
        .collect();
    Ok(SelectorScoreObservation { score_calculations })
}

fn drain_skill_attackable_checker_calls(
    snapshot_native_tick: u64,
    capture: &mut CaptureState,
) -> Result<SkillAttackableCheckerObservation, String> {
    if !capture.open_checker_calls.is_empty() {
        capture.completed_checker_calls.clear();
        return Err(format!(
            "checker snapshot at native tick {snapshot_native_tick} has {} unreturned calls",
            capture.open_checker_calls.len()
        ));
    }
    if let Some(call) = capture
        .completed_checker_calls
        .iter()
        .find(|call| call.entry.native_logic_tick != snapshot_native_tick)
    {
        let call_tick = call.entry.native_logic_tick;
        let ordinal = call.entry.invocation_ordinal;
        capture.completed_checker_calls.clear();
        return Err(format!(
            "checker invocation {ordinal} belongs to native tick {call_tick}, not snapshot tick {snapshot_native_tick}"
        ));
    }
    let mut raw_calls = std::mem::take(&mut capture.completed_checker_calls);
    raw_calls.sort_by_key(|call| call.entry.invocation_ordinal);
    if let Some(pair) = raw_calls
        .windows(2)
        .find(|pair| pair[0].entry.invocation_ordinal == pair[1].entry.invocation_ordinal)
    {
        return Err(format!(
            "checker invocation {} completed more than once",
            pair[0].entry.invocation_ordinal
        ));
    }
    let mut checker_calls = Vec::with_capacity(raw_calls.len());
    for call in raw_calls {
        let source_actor =
            resolve_checker_actor_ref(call.entry.source_actor, "checker source actor", capture)?;
        checker_calls.push(SkillAttackableCheckerCall {
            invocation_ordinal: call.entry.invocation_ordinal,
            native_logic_tick: call.entry.native_logic_tick,
            source_actor,
            source_skill_id: call.entry.source_skill_id,
            is_attacking_check: call.entry.is_attacking_check,
            previous_attack_target: resolve_checker_target(
                call.entry.previous_attack_target,
                "checker previous attack target",
                capture,
            )?,
            post_attack_target_candidate: resolve_checker_target(
                call.post_attack_target_candidate,
                "checker post attack target candidate",
                capture,
            )?,
            quick_switch_enabled: call.quick_switch_enabled,
            check_return: call.check_return,
        });
    }
    Ok(SkillAttackableCheckerObservation { checker_calls })
}

fn resolve_checker_actor_ref(
    pointer: usize,
    field: &str,
    capture: &CaptureState,
) -> Result<ObjectRef, String> {
    let reference = object_ref_from_pointer(pointer, capture)
        .ok_or_else(|| format!("{field} is absent from the MCFR world snapshot"))?;
    if matches!(reference.kind, ObjectKind::Unit | ObjectKind::Building) {
        Ok(reference)
    } else {
        Err(format!(
            "{field} resolved to unsupported kind {:?}",
            reference.kind
        ))
    }
}

fn resolve_checker_target(
    target: Option<RawCheckerTarget>,
    field: &str,
    capture: &CaptureState,
) -> Result<Option<CheckerTargetObservation>, String> {
    let Some(target) = target else {
        return Ok(None);
    };
    let target_ref = resolve_checker_actor_ref(target.pointer, field, capture)?;
    if target_ref.kind != target.qualifying_status.kind() {
        return Err(format!(
            "{field} direct status kind {:?} disagrees with resolved ObjectRef kind {:?}",
            target.qualifying_status.kind(),
            target_ref.kind
        ));
    }
    Ok(Some(CheckerTargetObservation {
        target_ref,
        qualifying_status: target.qualifying_status,
    }))
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CheckerStatusCorroboration {
    Consistent,
    Disagreement {
        direct: CheckerQualifyingStatus,
        mcfr: CheckerQualifyingStatus,
    },
}

#[cfg(test)]
fn corroborate_checker_status(
    target: &CheckerTargetObservation,
    world: &WorldSnapshot,
) -> Result<CheckerStatusCorroboration, String> {
    let mcfr = match target.target_ref.kind {
        ObjectKind::Unit => {
            let unit = world
                .live_units
                .iter()
                .find(|unit| unit.unit_id == target.target_ref.id)
                .ok_or_else(|| "checker unit is absent from paired MCFR row".to_owned())?;
            CheckerQualifyingStatus::Unit {
                alive: true,
                active_or_available: unit.active,
                targetable: unit.targetable,
            }
        }
        ObjectKind::Building => {
            let building = world
                .buildings
                .iter()
                .find(|building| building.building_id == target.target_ref.id)
                .ok_or_else(|| "checker building is absent from paired MCFR row".to_owned())?;
            CheckerQualifyingStatus::Building {
                alive: true,
                active_or_available: building.available,
                targetable: building.targetable,
                destroyed: false,
            }
        }
        kind => return Err(format!("unsupported checker corroboration kind {kind:?}")),
    };
    Ok(if target.qualifying_status == mcfr {
        CheckerStatusCorroboration::Consistent
    } else {
        CheckerStatusCorroboration::Disagreement {
            direct: target.qualifying_status,
            mcfr,
        }
    })
}

const fn event(
    subject: Option<ObjectRef>,
    source: Option<ObjectRef>,
    source_team_id: Option<u32>,
    target: Option<ObjectRef>,
    payload: EventPayload,
) -> Event {
    Event {
        subject,
        source,
        source_team_id,
        target,
        payload,
    }
}

fn list_count(api: Api, list: *mut Object, maximum: i32) -> Result<i32, String> {
    if list.is_null() {
        return Err("managed list is null".into());
    }
    let count = invoke_value::<i32>(api, list, "get_Count")?;
    if (0..=maximum).contains(&count) {
        Ok(count)
    } else {
        Err(format!("managed list count {count} exceeds {maximum}"))
    }
}

fn list_item(api: Api, list: *mut Object, mut index: i32) -> Result<*mut Object, String> {
    api.invoke(list, "get_Item", &mut [argument(&mut index)])
        .map_err(|error| error.to_string())
}

fn list_i32_item(api: Api, list: *mut Object, mut index: i32) -> Result<i32, String> {
    api.invoke_value(list, "get_Item", &mut [argument(&mut index)])
        .map_err(|error| error.to_string())
}

fn invoke_object(api: Api, object: *mut Object, method: &str) -> Result<*mut Object, String> {
    let value = api
        .invoke(object, method, &mut [])
        .map_err(|error| error.to_string())?;
    if value.is_null() {
        Err(format!("{method} returned null"))
    } else {
        Ok(value)
    }
}

fn invoke_value<T: Copy>(api: Api, object: *mut Object, method: &str) -> Result<T, String> {
    api.invoke_value(object, method, &mut [])
        .map_err(|error| error.to_string())
}

const fn vec3(value: FixedVec3) -> QVec3 {
    QVec3 {
        x: value.x.raw,
        y: value.y.raw,
        z: value.z.raw,
    }
}

fn allocate(next: &mut u64, label: &str) -> Result<u64, String> {
    let id = *next;
    *next = next
        .checked_add(1)
        .ok_or_else(|| format!("{label} identity overflow"))?;
    Ok(id)
}

const RVO_CONTROLLER_ACTIVE_LABEL: &str = "RVOControllerFixed.Active";
const RVO_ADD_AGENT_FIXED_LABEL: &str = "RVO Simulator.AddAgentFixed";
const RVO_FIXED_UPDATE_LABEL: &str = "RVO Simulator.FixedUpdate";
const RVO_PRE_CALCULATION_LABEL: &str = "RVO Simulator.PreCalculation";
const RVO_CALCULATE_NEIGHBOURS_LABEL: &str = "RVO Agent.CalculateNeighbours";
const RVO_GENERATE_NEIGHBOUR_VOS_LABEL: &str = "RVOAgentFixed.GenerateNeighbourAgentVOs";
const RVO_GENERATE_OPPONENT_VOS_LABEL: &str = "RVOAgentFixed.GenerateOpponentVOs";
const RVO_CONTROLLER_ACTIVE_PROLOGUE: [u8; 16] = [
    0xff, 0xc3, 0x01, 0xd1, 0xf8, 0x5f, 0x03, 0xa9, 0xf6, 0x57, 0x04, 0xa9, 0xf4, 0x4f, 0x05, 0xa9,
];
const RVO_ADD_AGENT_FIXED_PROLOGUE: [u8; 16] = [
    0xf6, 0x57, 0xbd, 0xa9, 0xf4, 0x4f, 0x01, 0xa9, 0xfd, 0x7b, 0x02, 0xa9, 0xfd, 0x83, 0x00, 0x91,
];
const RVO_FIXED_UPDATE_PROLOGUE: [u8; 16] = [
    0xf8, 0x5f, 0xbc, 0xa9, 0xf6, 0x57, 0x01, 0xa9, 0xf4, 0x4f, 0x02, 0xa9, 0xfd, 0x7b, 0x03, 0xa9,
];
const RVO_PRE_CALCULATION_PROLOGUE: [u8; 16] = RVO_ADD_AGENT_FIXED_PROLOGUE;
const RVO_CALCULATE_NEIGHBOURS_PROLOGUE: [u8; 16] = [
    0xf4, 0x4f, 0xbe, 0xa9, 0xfd, 0x7b, 0x01, 0xa9, 0xfd, 0x43, 0x00, 0x91, 0xf3, 0x03, 0x00, 0xaa,
];
const RVO_GENERATE_NEIGHBOUR_VOS_PROLOGUE: [u8; 16] = [
    0xed, 0x33, 0xb7, 0x6d, 0xeb, 0x2b, 0x01, 0x6d, 0xe9, 0x23, 0x02, 0x6d, 0xfc, 0x6f, 0x03, 0xa9,
];
const RVO_GENERATE_OPPONENT_VOS_PROLOGUE: [u8; 16] = [
    0xff, 0xc3, 0x07, 0xd1, 0xfc, 0x6f, 0x19, 0xa9, 0xfa, 0x67, 0x1a, 0xa9, 0xf8, 0x5f, 0x1b, 0xa9,
];
const SELECTOR_CALCULATE_SCORE_PROLOGUE: [u8; 16] = [
    0xff, 0x03, 0x02, 0xd1, 0xfc, 0x6f, 0x02, 0xa9, 0xfa, 0x67, 0x03, 0xa9, 0xf8, 0x5f, 0x04, 0xa9,
];

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_selector_calculate_score_hook(
    api: Api,
    method: *const MethodInfo,
) -> Result<(), String> {
    install_inline_hook(
        api,
        method,
        &SELECTOR_CALCULATE_SCORE_PROLOGUE,
        selector_calculate_score_hook as *const c_void,
        &ORIGINAL_SELECTOR_CALCULATE_SCORE,
        "ScoreRatingTargetSelector.CalculateScore",
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_rvo_controller_active_hook(api: Api, method: *const MethodInfo) -> Result<(), String> {
    install_inline_hook(
        api,
        method,
        &RVO_CONTROLLER_ACTIVE_PROLOGUE,
        rvo_controller_active_hook as *const c_void,
        &ORIGINAL_RVO_CONTROLLER_ACTIVE,
        RVO_CONTROLLER_ACTIVE_LABEL,
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_rvo_add_agent_fixed_hook(api: Api, method: *const MethodInfo) -> Result<(), String> {
    install_inline_hook(
        api,
        method,
        &RVO_ADD_AGENT_FIXED_PROLOGUE,
        rvo_add_agent_fixed_hook as *const c_void,
        &ORIGINAL_RVO_ADD_AGENT_FIXED,
        RVO_ADD_AGENT_FIXED_LABEL,
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_rvo_fixed_update_hook(api: Api, method: *const MethodInfo) -> Result<(), String> {
    install_inline_hook(
        api,
        method,
        &RVO_FIXED_UPDATE_PROLOGUE,
        rvo_fixed_update_hook as *const c_void,
        &ORIGINAL_RVO_FIXED_UPDATE,
        RVO_FIXED_UPDATE_LABEL,
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_rvo_pre_calculation_hook(api: Api, method: *const MethodInfo) -> Result<(), String> {
    install_inline_hook(
        api,
        method,
        &RVO_PRE_CALCULATION_PROLOGUE,
        rvo_pre_calculation_hook as *const c_void,
        &ORIGINAL_RVO_PRE_CALCULATION,
        RVO_PRE_CALCULATION_LABEL,
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_rvo_calculate_neighbours_hook(
    api: Api,
    method: *const MethodInfo,
) -> Result<(), String> {
    install_inline_hook(
        api,
        method,
        &RVO_CALCULATE_NEIGHBOURS_PROLOGUE,
        rvo_calculate_neighbours_hook as *const c_void,
        &ORIGINAL_RVO_CALCULATE_NEIGHBOURS,
        RVO_CALCULATE_NEIGHBOURS_LABEL,
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_rvo_generate_opponent_vos_hook(
    api: Api,
    method: *const MethodInfo,
) -> Result<(), String> {
    install_inline_hook(
        api,
        method,
        &RVO_GENERATE_OPPONENT_VOS_PROLOGUE,
        rvo_generate_opponent_vos_hook as *const c_void,
        &ORIGINAL_RVO_GENERATE_OPPONENT_VOS,
        RVO_GENERATE_OPPONENT_VOS_LABEL,
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_rvo_generate_neighbour_vos_hook(
    api: Api,
    method: *const MethodInfo,
) -> Result<(), String> {
    install_inline_hook(
        api,
        method,
        &RVO_GENERATE_NEIGHBOUR_VOS_PROLOGUE,
        rvo_generate_neighbour_vos_hook as *const c_void,
        &ORIGINAL_RVO_GENERATE_NEIGHBOUR_VOS,
        RVO_GENERATE_NEIGHBOUR_VOS_LABEL,
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_update_hook(api: Api, method: *const MethodInfo) -> Result<(), String> {
    const EXPECTED: [u8; 16] = [
        0xf4, 0x4f, 0xbe, 0xa9, 0xfd, 0x7b, 0x01, 0xa9, 0xfd, 0x43, 0x00, 0x91, 0xf3, 0x03, 0x00,
        0xaa,
    ];
    install_inline_hook(
        api,
        method,
        &EXPECTED,
        update_hook as *const c_void,
        &ORIGINAL_UPDATE,
        "FightController.Update",
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_match_update_hook(api: Api, method: *const MethodInfo) -> Result<(), String> {
    const EXPECTED: [u8; 16] = [
        0xf4, 0x4f, 0xbe, 0xa9, 0xfd, 0x7b, 0x01, 0xa9, 0xfd, 0x43, 0x00, 0x91, 0xf3, 0x03, 0x00,
        0xaa,
    ];
    install_inline_hook(
        api,
        method,
        &EXPECTED,
        match_update_hook as *const c_void,
        &ORIGINAL_MATCH_UPDATE,
        "MatchClient.Update",
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_player_finish_deploy_hook(api: Api, method: *const MethodInfo) -> Result<(), String> {
    const EXPECTED: [u8; 16] = [
        0xf4, 0x4f, 0xbe, 0xa9, 0xfd, 0x7b, 0x01, 0xa9, 0xfd, 0x43, 0x00, 0x91, 0xf3, 0x03, 0x00,
        0xaa,
    ];
    install_inline_hook(
        api,
        method,
        &EXPECTED,
        player_finish_deploy_hook as *const c_void,
        &ORIGINAL_PLAYER_FINISH_DEPLOY,
        "PlayerController.FinishDeploy",
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_post_render_hook(api: Api, method: *const MethodInfo) -> Result<(), String> {
    const EXPECTED: [u8; 16] = [
        0xf6, 0x57, 0xbd, 0xa9, 0xf4, 0x4f, 0x01, 0xa9, 0xfd, 0x7b, 0x02, 0xa9, 0xfd, 0x83, 0x00,
        0x91,
    ];
    install_inline_hook(
        api,
        method,
        &EXPECTED,
        post_render_hook as *const c_void,
        &ORIGINAL_POST_RENDER,
        "Camera.FireOnPostRender",
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_projectile_create_hook(api: Api, method: *const MethodInfo) -> Result<(), String> {
    const EXPECTED: [u8; 16] = [
        0xff, 0x03, 0x05, 0xd1, 0xfc, 0x6f, 0x0e, 0xa9, 0xfa, 0x67, 0x0f, 0xa9, 0xf8, 0x5f, 0x10,
        0xa9,
    ];
    install_inline_hook(
        api,
        method,
        &EXPECTED,
        projectile_create_hook as *const c_void,
        &ORIGINAL_PROJECTILE_CREATE,
        "ProjectileSystem.Create(FightProjectileSkill,...)",
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_projectile_add_hook(api: Api, method: *const MethodInfo) -> Result<(), String> {
    const EXPECTED: [u8; 16] = [
        0xf6, 0x57, 0xbd, 0xa9, 0xf4, 0x4f, 0x01, 0xa9, 0xfd, 0x7b, 0x02, 0xa9, 0xfd, 0x83, 0x00,
        0x91,
    ];
    install_inline_hook(
        api,
        method,
        &EXPECTED,
        projectile_add_hook as *const c_void,
        &ORIGINAL_PROJECTILE_ADD,
        "ProjectileSystem.AddProjectile",
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_projectile_destroy_hook(api: Api, method: *const MethodInfo) -> Result<(), String> {
    const EXPECTED: [u8; 16] = [
        0xff, 0x43, 0x01, 0xd1, 0xf8, 0x5f, 0x01, 0xa9, 0xf6, 0x57, 0x02, 0xa9, 0xf4, 0x4f, 0x03,
        0xa9,
    ];
    install_inline_hook(
        api,
        method,
        &EXPECTED,
        projectile_destroy_hook as *const c_void,
        &ORIGINAL_PROJECTILE_DESTROY,
        "ProjectileSystem.Destroy",
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_damage_perform_hook(api: Api, method: *const MethodInfo) -> Result<(), String> {
    const EXPECTED: [u8; 16] = [
        0xff, 0xc3, 0x04, 0xd1, 0xfc, 0x6f, 0x0d, 0xa9, 0xfa, 0x67, 0x0e, 0xa9, 0xf8, 0x5f, 0x0f,
        0xa9,
    ];
    install_inline_hook(
        api,
        method,
        &EXPECTED,
        damage_perform_hook as *const c_void,
        &ORIGINAL_DAMAGE_PERFORM,
        "DamagePerformer.Perform",
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_fight_actor_reduce_life_hook(api: Api, method: *const MethodInfo) -> Result<(), String> {
    const EXPECTED: [u8; 16] = [
        0xff, 0xc3, 0x02, 0xd1, 0xfc, 0x6f, 0x05, 0xa9, 0xfa, 0x67, 0x06, 0xa9, 0xf8, 0x5f, 0x07,
        0xa9,
    ];
    install_inline_hook(
        api,
        method,
        &EXPECTED,
        fight_actor_reduce_life_hook as *const c_void,
        &ORIGINAL_FIGHT_ACTOR_REDUCE_LIFE,
        "FightActor.ReduceLife",
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_fight_controller_on_actor_hitted_hook(
    api: Api,
    method: *const MethodInfo,
) -> Result<(), String> {
    const EXPECTED: [u8; 16] = [
        0xf8, 0x5f, 0xbc, 0xa9, 0xf6, 0x57, 0x01, 0xa9, 0xf4, 0x4f, 0x02, 0xa9, 0xfd, 0x7b, 0x03,
        0xa9,
    ];
    install_inline_hook(
        api,
        method,
        &EXPECTED,
        fight_controller_on_actor_hitted_hook as *const c_void,
        &ORIGINAL_FIGHT_CONTROLLER_ON_ACTOR_HITTED,
        "FightController.OnActorHitted",
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_advanced_shield_damage_hook(api: Api, method: *const MethodInfo) -> Result<(), String> {
    const EXPECTED: [u8; 16] = [
        0xf8, 0x5f, 0xbc, 0xa9, 0xf6, 0x57, 0x01, 0xa9, 0xf4, 0x4f, 0x02, 0xa9, 0xfd, 0x7b, 0x03,
        0xa9,
    ];
    install_inline_hook(
        api,
        method,
        &EXPECTED,
        advanced_shield_damage_hook as *const c_void,
        &ORIGINAL_ADVANCED_SHIELD_DAMAGE,
        "DamagePerformer.PerformHitAdvancedEndergyShieldEffect",
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_fight_mech_on_dead_hook(api: Api, method: *const MethodInfo) -> Result<(), String> {
    const EXPECTED: [u8; 16] = [
        0xf4, 0x4f, 0xbe, 0xa9, 0xfd, 0x7b, 0x01, 0xa9, 0xfd, 0x43, 0x00, 0x91, 0xf3, 0x03, 0x00,
        0xaa,
    ];
    install_inline_hook(
        api,
        method,
        &EXPECTED,
        fight_mech_on_dead_hook as *const c_void,
        &ORIGINAL_FIGHT_MECH_ON_DEAD,
        "FightMech.OnDead",
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_fight_crystal_on_dead_hook(api: Api, method: *const MethodInfo) -> Result<(), String> {
    const EXPECTED: [u8; 16] = [
        0xf4, 0x4f, 0xbe, 0xa9, 0xfd, 0x7b, 0x01, 0xa9, 0xfd, 0x43, 0x00, 0x91, 0xf3, 0x03, 0x00,
        0xaa,
    ];
    install_inline_hook(
        api,
        method,
        &EXPECTED,
        fight_crystal_on_dead_hook as *const c_void,
        &ORIGINAL_FIGHT_CRYSTAL_ON_DEAD,
        "FightCrystal.OnDead",
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_inline_hook(
    api: Api,
    method: *const MethodInfo,
    expected: &[u8; 16],
    replacement: *const c_void,
    original_slot: &AtomicPtr<c_void>,
    label: &str,
) -> Result<(), String> {
    let target = api
        .method_pointer(method)
        .map_err(|error| error.to_string())?;
    // SAFETY: target points to at least the generated method prologue.
    let actual = unsafe { std::slice::from_raw_parts(target.cast::<u8>(), expected.len()) };
    verify_rvo_hook_prologue(actual, expected, label)?;
    // SAFETY: anonymous mapping is checked before use.
    let trampoline = unsafe {
        libc::mmap(
            ptr::null_mut(),
            32,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANON,
            -1,
            0,
        )
    };
    if trampoline == libc::MAP_FAILED {
        return Err(format!("cannot allocate {label} trampoline"));
    }
    // SAFETY: both source and destination are valid for the fixed lengths.
    unsafe { ptr::copy_nonoverlapping(target.cast::<u8>(), trampoline.cast::<u8>(), 16) };
    write_absolute_jump(unsafe { trampoline.cast::<u8>().add(16) }, unsafe {
        target.cast::<u8>().add(16).cast()
    });
    // SAFETY: trampoline is the mapping created above.
    if unsafe { libc::mprotect(trampoline, 32, libc::PROT_READ | libc::PROT_EXEC) } != 0 {
        // SAFETY: mapping belongs to this function after mprotect failure.
        unsafe { libc::munmap(trampoline, 32) };
        return Err(format!("cannot make {label} trampoline executable"));
    }
    let mut jump = [0_u8; 16];
    write_absolute_jump(jump.as_mut_ptr(), replacement);
    set_code_bytes(target, &jump)?;
    original_slot.store(trampoline, Ordering::Release);
    Ok(())
}

#[cfg(any(test, all(target_os = "macos", target_arch = "aarch64")))]
fn verify_rvo_hook_prologue(actual: &[u8], expected: &[u8; 16], label: &str) -> Result<(), String> {
    if actual == expected {
        Ok(())
    } else {
        Err(format!("{label} prologue mismatch: {}", bytes_hex(actual)))
    }
}

#[cfg(any(test, all(target_os = "macos", target_arch = "aarch64")))]
fn bytes_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    bytes.iter().fold(
        String::with_capacity(bytes.len() * 2),
        |mut output, byte| {
            let _ = write!(output, "{byte:02x}");
            output
        },
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn write_absolute_jump(destination: *mut u8, target: *const c_void) {
    let load_x16_pc_plus_8 = 0x5800_0050_u32;
    let branch_x16 = 0xd61f_0200_u32;
    // SAFETY: caller provides a writable 16-byte destination.
    unsafe {
        destination
            .cast::<u32>()
            .write_unaligned(load_x16_pc_plus_8);
        destination.add(4).cast::<u32>().write_unaligned(branch_x16);
        destination
            .add(8)
            .cast::<*const c_void>()
            .write_unaligned(target);
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn set_code_bytes(address: *mut c_void, bytes: &[u8]) -> Result<(), String> {
    const VM_PROT_READ: i32 = 1;
    const VM_PROT_WRITE: i32 = 2;
    const VM_PROT_EXECUTE: i32 = 4;
    const VM_PROT_COPY: i32 = 0x10;
    unsafe extern "C" {
        static mach_task_self_: u32;
        fn vm_protect(
            task: u32,
            address: usize,
            size: usize,
            set_maximum: bool,
            new_protection: i32,
        ) -> i32;
        fn sys_icache_invalidate(start: *mut c_void, length: usize);
    }
    // SAFETY: sysconf has no memory-safety preconditions.
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page_size <= 0 {
        return Err("cannot determine code page size".into());
    }
    let page_size = usize::try_from(page_size).map_err(|_| "invalid page size".to_owned())?;
    let begin = (address as usize) & !(page_size - 1);
    let end = (address as usize + bytes.len() + page_size - 1) & !(page_size - 1);
    // SAFETY: the page range contains the validated generated method target.
    let writable = unsafe {
        vm_protect(
            mach_task_self_,
            begin,
            end - begin,
            false,
            VM_PROT_READ | VM_PROT_WRITE | VM_PROT_COPY,
        )
    };
    if writable != 0 {
        return Err(format!("vm_protect writable failed with {writable}"));
    }
    // SAFETY: page is writable and destination covers bytes.len().
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), address.cast::<u8>(), bytes.len());
        sys_icache_invalidate(address, bytes.len());
        let _ = vm_protect(
            mach_task_self_,
            begin,
            end - begin,
            false,
            VM_PROT_READ | VM_PROT_EXECUTE,
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    static RVO_GLOBAL_TEST_LOCK: Mutex<()> = Mutex::new(());
    static TEST_HOOK_CALLS: [AtomicU64; RVO_HOOK_COUNT] =
        [const { AtomicU64::new(0) }; RVO_HOOK_COUNT];
    static TEST_HOOK_ARGUMENTS: [[AtomicU64; 4]; RVO_HOOK_COUNT] =
        [const { [const { AtomicU64::new(0) }; 4] }; RVO_HOOK_COUNT];

    #[test]
    fn native_unit_indices_allow_stable_gaps_but_not_invalid_identity() {
        assert!(validate_native_indices("unit", &[0, 1, 3, 7]).is_ok());
        assert!(validate_native_indices("construction", &[1, 2, 3]).is_ok());
        assert!(validate_native_indices("unit", &[0, 1, 1]).is_err());
        assert!(validate_native_indices("construction", &[-1, 0]).is_err());
    }

    #[test]
    fn research_blueprint_chain_decodes_replaced_native_slots() {
        assert_eq!(decode_blueprint_level(Some(false), None, "attack"), Ok(0));
        assert_eq!(decode_blueprint_level(None, Some(false), "attack"), Ok(1));
        assert_eq!(decode_blueprint_level(None, Some(true), "attack"), Ok(2));
        assert!(decode_blueprint_level(Some(true), None, "attack").is_err());
        assert!(decode_blueprint_level(Some(false), Some(false), "attack").is_err());
    }

    #[test]
    fn native_hit_damage_info_layout_matches_build_2259() {
        assert_eq!(std::mem::size_of::<NativeHitDamageInfo>(), 0x50);
        assert_eq!(std::mem::align_of::<NativeHitDamageInfo>(), 8);
        assert_eq!(std::mem::offset_of!(NativeHitDamageInfo, source_team), 0x00);
        assert_eq!(
            std::mem::offset_of!(NativeHitDamageInfo, source_skill_owner),
            0x08
        );
        assert_eq!(
            std::mem::offset_of!(NativeHitDamageInfo, target_actor),
            0x10
        );
        assert_eq!(std::mem::offset_of!(NativeHitDamageInfo, damage), 0x20);
        assert_eq!(std::mem::offset_of!(NativeHitDamageInfo, damage_real), 0x24);
        assert_eq!(std::mem::offset_of!(NativeHitDamageInfo, hit_point), 0x28);
        assert_eq!(
            std::mem::offset_of!(NativeHitDamageInfo, damage_provider),
            0x48
        );
    }

    #[test]
    fn native_terrain_grid_columns_are_transposed_to_canonical_rows() {
        assert_eq!(
            terrain_grid_rows_from_native_columns(&[0x8000_0000, 0xc000_0000, 0], 2, 2).unwrap(),
            vec![0b11, 0b10]
        );
        assert!(terrain_grid_rows_from_native_columns(&[0x2000_0000], 1, 2).is_err());
    }

    #[test]
    fn native_rate_neutral_factor_normalizes_to_zero() {
        assert_eq!(
            normalize_native_rate(0, FIXED_ONE_RAW).unwrap(),
            RateModifier::default()
        );
    }

    #[test]
    fn native_rate_reduction_preserves_q32_delta() {
        assert_eq!(
            normalize_native_rate(0, FIXED_ONE_RAW - 123).unwrap(),
            RateModifier {
                add: 0,
                reduce: 123,
            }
        );
    }

    fn record_test_hook_call(index: usize, arguments: &[usize]) {
        TEST_HOOK_CALLS[index].fetch_add(1, Ordering::AcqRel);
        for (slot, argument) in TEST_HOOK_ARGUMENTS[index].iter().zip(arguments) {
            slot.store(*argument as u64, Ordering::Release);
        }
    }

    unsafe extern "C" fn test_rvo_controller_active(
        controller: *mut Object,
        method: *const MethodInfo,
    ) {
        record_test_hook_call(0, &[controller as usize, method as usize]);
    }

    unsafe extern "C" fn test_rvo_add_agent_fixed(
        simulator: *mut Object,
        agent: *mut Object,
        method: *const MethodInfo,
    ) -> *mut Object {
        record_test_hook_call(1, &[simulator as usize, agent as usize, method as usize]);
        0x204_usize as *mut Object
    }

    unsafe extern "C" fn test_rvo_fixed_update(simulator: *mut Object, method: *const MethodInfo) {
        record_test_hook_call(2, &[simulator as usize, method as usize]);
    }

    unsafe extern "C" fn test_rvo_pre_calculation(
        simulator: *mut Object,
        method: *const MethodInfo,
    ) {
        record_test_hook_call(3, &[simulator as usize, method as usize]);
    }

    unsafe extern "C" fn test_rvo_calculate_neighbours(
        agent: *mut Object,
        method: *const MethodInfo,
    ) {
        record_test_hook_call(4, &[agent as usize, method as usize]);
    }

    unsafe extern "C" fn test_rvo_generate_neighbour_vos(
        agent: *mut Object,
        vos: *mut Object,
        method: *const MethodInfo,
    ) {
        record_test_hook_call(5, &[agent as usize, vos as usize, method as usize]);
    }

    unsafe extern "C" fn test_rvo_generate_opponent_vos(
        agent: *mut Object,
        vos: *mut Object,
        other: *mut Object,
        method: *const MethodInfo,
    ) {
        record_test_hook_call(
            6,
            &[
                agent as usize,
                vos as usize,
                other as usize,
                method as usize,
            ],
        );
    }

    fn target_refs() -> TargetRefsObservation {
        TargetRefsObservation { units: Vec::new() }
    }

    fn seed_rvo_update(
        state: &mut CaptureState,
        ordinal: u64,
        start_tick: u64,
        publish_tick: u64,
        double_buffering: bool,
        symmetry_bias_raw: i64,
        multithreaded: bool,
    ) {
        state.rvo_update_modes.insert(ordinal, double_buffering);
        state.rvo_update_symmetry_breaking_biases.insert(
            ordinal,
            FixedPoint {
                raw: symmetry_bias_raw,
            },
        );
        state
            .rvo_update_start_native_ticks
            .insert(ordinal, start_tick);
        state
            .rvo_update_publish_native_ticks
            .insert(ordinal, publish_tick);
        state
            .rvo_update_multithreaded
            .insert(ordinal, multithreaded);
    }

    fn assert_internal(reference: RvoAgentRefObservation, expected: u64) {
        assert!(matches!(
            reference,
            RvoAgentRefObservation::Internal {
                internal_agent_ordinal
            } if internal_agent_ordinal == expected
        ));
    }

    fn unit(id: u64, team: u32, formation: u64) -> LiveUnitState {
        LiveUnitState {
            unit_id: id,
            team_id: team,
            original_team_id: team,
            formation_id: formation,
            unit_type_id: 1,
            domain: Domain::Ground,
            position: QVec3 {
                x: i64::from(team) * 1_000,
                y: 0,
                z: 0,
            },
            body_rotation: 0,
            velocity: QVec3 { x: 0, y: 0, z: 0 },
            motion_state: MotionState::Idle,
            mech_lock_target: None,
            collision_radius: 100,
            life: GaugeI32 {
                current: 10,
                maximum: 10,
            },
            active: true,
            targetable: true,
            visibility: Visibility::Normal,
            status_mask: 0,
            buff_modifiers: BuffModifierSet::default(),
            unit_dynamic_modifiers: UnitDynamicModifierSet::default(),
            skill_dynamic_modifiers: Vec::new(),
            personal_shield: PersonalShieldState {
                active: false,
                enabled: false,
                energy: GaugeI32 {
                    current: 0,
                    maximum: 0,
                },
            },
            weapon_aims: Vec::new(),
        }
    }

    fn building(id: u64) -> BuildingState {
        BuildingState {
            building_id: id,
            team_id: 1,
            building_type_id: 1,
            position: QVec3 { x: 0, y: 0, z: 0 },
            bounds_width: 1_000,
            bounds_height: 1_000,
            life: GaugeI32 {
                current: 10,
                maximum: 10,
            },
            available: true,
            targetable: true,
            collision_enabled: true,
        }
    }

    #[test]
    fn layout_shields_include_inactive_carry_over_without_release_records() {
        let placement = |kind: &str, x, y| ContraptionPlacement {
            type_name: kind.into(),
            x,
            y,
            isairdrop: None,
        };
        let shields = vec![
            layout_test_shield(1, 215, 86, None),
            layout_test_shield(2, -230, 120, Some(0)),
            layout_test_shield(3, 106, 103, Some(1)),
        ];
        let layout = layout_shield_placements(shields, 1, 40_000, None).unwrap();
        assert_eq!(
            layout,
            vec![
                placement("shield", -215, -86),
                placement("shield", 230, -120),
                placement("shield", -106, -103),
            ]
        );
        let inherited_only =
            layout_shield_placements(vec![layout_test_shield(1, 215, 86, None)], 1, 40_000, None)
                .unwrap();
        assert_eq!(inherited_only, vec![placement("shield", -215, -86)]);
    }

    #[test]
    fn layout_shields_include_airdrops_in_full_list_order() {
        let mut airdrop = layout_test_shield(2, 300, -20, None);
        airdrop.state.source_kind = ShieldSourceKind::CommanderSkill;
        airdrop.state.radius = 100 * FIXED_ONE_RAW;
        airdrop.state.energy.maximum = 90_000;
        let defaults = Some((100 * FIXED_ONE_RAW, 90_000));
        let placements = layout_shield_placements(
            vec![layout_test_shield(1, 215, 86, Some(1)), airdrop.clone()],
            1,
            40_000,
            defaults,
        )
        .unwrap();
        assert_eq!(
            placements
                .iter()
                .map(|placement| (placement.x, placement.y, placement.isairdrop))
                .collect::<Vec<_>>(),
            [(-215, -86, None), (-300, 20, Some(true))]
        );
        assert!(layout_shield_placements(vec![airdrop.clone()], 1, 40_000, None).is_err());
        airdrop.state.round_policy = ShieldRoundPolicy::RetainState;
        assert!(layout_shield_placements(vec![airdrop], 1, 40_000, defaults).is_err());
    }

    #[test]
    fn layout_shields_reject_unrepresentable_energy_and_fractional_center() {
        let mut shield = layout_test_shield(1, 215, 86, None);
        shield.state.energy.maximum = 80_000;
        assert!(layout_shield_placements(vec![shield], 1, 40_000, None).is_err());
        let mut shield = layout_test_shield(1, 215, 86, None);
        shield.state.position.x += 1;
        assert!(layout_shield_placements(vec![shield], 1, 40_000, None).is_err());
    }

    #[test]
    fn live_contraption_positions_share_exact_side_local_conversion() {
        for kind in ["shield", "missile", "interceptor"] {
            let position = QVec3 {
                x: 215 * FIXED_ONE_RAW,
                y: 0,
                z: 86 * FIXED_ONE_RAW,
            };
            let blue = layout_contraption_position(kind, position, 0).unwrap();
            let red = layout_contraption_position(kind, position, 1).unwrap();
            assert_eq!((blue.x, blue.y), (215, 86));
            assert_eq!((red.x, red.y), (-215, -86));
            assert_eq!(red.type_name, kind);
            assert!(
                layout_contraption_position(
                    kind,
                    QVec3 {
                        x: position.x + 1,
                        ..position
                    },
                    1
                )
                .is_err()
            );
            let elevated = QVec3 {
                y: FIXED_ONE_RAW,
                ..position
            };
            assert_eq!(
                layout_contraption_position(kind, elevated, 1).is_ok(),
                kind != "shield"
            );
        }
    }

    #[test]
    fn initial_shield_ids_use_s1_active_order_and_then_remain_stable() {
        let mut replay = shield_identity_test_capture(&[10, 20, 30]);
        let mut training = shield_identity_test_capture(&[20, 30, 10]);
        let current = vec![
            layout_test_shield(10, 215, 86, Some(2)),
            layout_test_shield(20, -230, 120, Some(0)),
            layout_test_shield(30, 106, 103, Some(1)),
        ];
        finalize_initial_shield_ids(&mut replay, &current).unwrap();
        finalize_initial_shield_ids(&mut training, &current).unwrap();
        assert_eq!(replay.shield_ids, training.shield_ids);
        assert_eq!(
            replay.shield_ids,
            BTreeMap::from([(10, 3), (20, 1), (30, 2)])
        );
        assert_eq!(replay.next_shield_id, 4);
        assert!(
            replay
                .shield_last_states
                .iter()
                .all(|(pointer, state)| { state.shield_id == replay.shield_ids[pointer] })
        );
        // Later reactivation/order changes must not renumber any existing shield.
        let reordered = vec![layout_test_shield(10, 215, 86, Some(0))];
        finalize_initial_shield_ids(&mut replay, &reordered).unwrap();
        assert_eq!(replay.shield_ids, training.shield_ids);
        let new_id = allocate(&mut replay.next_shield_id, "shield").unwrap();
        assert_eq!(new_id, 4);
        replay.reset_session();
        assert!(!replay.shield_ids_finalized);
        assert!(replay.shield_ids.is_empty());
    }

    #[test]
    fn initial_shield_ids_remap_e1_caches_and_first_tick_removed_objects() {
        let mut capture = shield_identity_test_capture(&[10, 20, 30]);
        let shield = |id| ObjectRef::new(ObjectKind::Shield, id);
        let unit = ObjectRef::new(ObjectKind::Unit, 1);
        // A removed blue shield must follow all S(1) rows, including red rows,
        // so the persisted initial namespace remains contiguous from one.
        capture.shield_last_states.get_mut(&10).unwrap().team_id = 0;
        capture.object_teams.insert(shield(1), 0);
        capture.unit_ids.insert(100, 1);
        capture.object_teams.insert(unit, 0);
        capture.pending_projectile_absorptions.insert(8, shield(1));
        capture.rvo_agent_refs.insert(99, shield(1));
        capture.last_damage_sources.insert(
            unit,
            DamageAttribution {
                source: Some(shield(1)),
                source_team_id: Some(1),
            },
        );
        capture.traces = vec![
            NativeTrace::Damage {
                source: Some(unit),
                source_team_id: Some(0),
                target: shield(1),
                amount: 100,
            },
            NativeTrace::ProjectileRemoved {
                projectile_id: 8,
                owner: 100,
                target: 10,
                position: QVec3 { x: 0, y: 0, z: 0 },
                intercepted: false,
                absorbed_by: Some(shield(1)),
            },
            NativeTrace::ShieldDestroyed {
                shield_id: 1,
                position: QVec3 { x: 0, y: 0, z: 0 },
            },
            NativeTrace::ShieldCreated {
                shield_id: 2,
                team_id: 1,
                source_kind: ShieldSourceKind::Contraption,
                position: QVec3 { x: 0, y: 0, z: 0 },
            },
            NativeTrace::UnitDied {
                unit_id: 1,
                position: QVec3 { x: 0, y: 0, z: 0 },
                source: Some(shield(1)),
                source_team_id: Some(1),
            },
        ];
        // Pointer 10 disappeared during E(1); its ID and event references still exist.
        let mut blue = layout_test_shield(40, 5, -90, Some(0));
        blue.state.team_id = 0;
        finalize_initial_shield_ids(
            &mut capture,
            &[
                layout_test_shield(20, -230, 120, Some(0)),
                layout_test_shield(30, 106, 103, Some(1)),
                blue,
            ],
        )
        .unwrap();
        assert_eq!(
            capture.shield_ids,
            BTreeMap::from([(10, 4), (20, 2), (30, 3), (40, 1)])
        );
        assert_eq!(capture.shield_last_states[&10].shield_id, 4);
        assert_eq!(capture.object_teams[&shield(4)], 0);
        assert_eq!(capture.object_teams[&unit], 0);
        assert_eq!(capture.pending_projectile_absorptions[&8], shield(4));
        assert_eq!(capture.rvo_agent_refs[&99], shield(4));
        assert_eq!(capture.last_damage_sources[&unit].source, Some(shield(4)));
        let events = transition_events(&capture.traces, &capture).events;
        assert_eq!(events[0].target, Some(shield(4)));
        assert_eq!(events[1].target, Some(shield(4)));
        assert!(
            matches!(events[1].payload, EventPayload::ProjectileRemoved {
            absorbed_by: Some(reference), ..
        } if reference == shield(4))
        );
        assert_eq!(events[2].subject, Some(shield(4)));
        assert_eq!(events[3].subject, Some(shield(2)));
        assert_eq!(events[4].source, Some(shield(4)));
    }

    #[test]
    fn initial_inactive_shields_use_deterministic_keys_and_reject_ambiguity() {
        let mut left = CaptureState::default();
        left.reset_session();
        let mut right = CaptureState::default();
        right.reset_session();
        let current = vec![
            layout_test_shield(10, 20, 90, None),
            layout_test_shield(20, -20, 90, None),
        ];
        finalize_initial_shield_ids(&mut left, &current).unwrap();
        let reversed = vec![
            layout_test_shield(20, -20, 90, None),
            layout_test_shield(10, 20, 90, None),
        ];
        finalize_initial_shield_ids(&mut right, &reversed).unwrap();
        assert_eq!(left.shield_ids, right.shield_ids);
        assert_eq!(left.shield_ids[&20], 1);
        right.reset_session();
        assert!(
            finalize_initial_shield_ids(
                &mut right,
                &[
                    layout_test_shield(10, 20, 90, None),
                    layout_test_shield(20, 20, 90, None),
                ]
            )
            .unwrap_err()
            .contains("indistinguishable")
        );
        right.reset_session();
        assert!(
            finalize_initial_shield_ids(
                &mut right,
                &[
                    layout_test_shield(10, 20, 90, Some(0)),
                    layout_test_shield(20, -20, 90, Some(0)),
                ]
            )
            .unwrap_err()
            .contains("duplicate initial shield active order")
        );
    }

    fn shield_identity_test_capture(pointers: &[usize]) -> CaptureState {
        let mut capture = CaptureState::default();
        capture.reset_session();
        for &pointer in pointers {
            let mut shield = layout_test_shield(pointer, pointer as i64, 90, Some(0)).state;
            shield.shield_id = allocate(&mut capture.next_shield_id, "shield").unwrap();
            capture.shield_ids.insert(pointer, shield.shield_id);
            capture
                .object_teams
                .insert(ObjectRef::new(ObjectKind::Shield, shield.shield_id), 1);
            capture.shield_last_states.insert(pointer, shield);
            capture.live_shield_pointers.insert(pointer);
        }
        capture
    }

    fn layout_test_shield(pointer: usize, x: i64, z: i64, active_order: Option<u32>) -> RawShield {
        RawShield {
            pointer,
            owner: 0,
            state: ShieldState {
                shield_id: 0,
                team_id: 1,
                source_kind: ShieldSourceKind::Contraption,
                owner: None,
                position: QVec3 {
                    x: x * FIXED_ONE_RAW,
                    y: 0,
                    z: z * FIXED_ONE_RAW,
                },
                radius: 70 * FIXED_ONE_RAW,
                energy: GaugeI32 {
                    current: if active_order.is_some() { 40_000 } else { 0 },
                    maximum: 40_000,
                },
                round_policy: ShieldRoundPolicy::ResetToMax,
                active: active_order.is_some(),
                active_order,
            },
        }
    }

    #[test]
    fn building_ids_ignore_native_enumeration_and_pointer_order() {
        let ordered = |order: [usize; 4]| {
            let mut rows = order
                .into_iter()
                .map(|id| {
                    let mut state = building(0);
                    state.team_id = if id == 3 { 0 } else { 1 };
                    state.building_type_id = if id == 2 { 2 } else { 1 };
                    state.position.x = id as i64;
                    RawBuilding {
                        pointer: 100 - id,
                        rvo_agent: None,
                        state,
                    }
                })
                .collect::<Vec<_>>();
            sort_buildings(&mut rows).unwrap();
            rows.into_iter()
                .map(|row| row.state.position.x)
                .collect::<Vec<_>>()
        };
        assert_eq!(ordered([0, 1, 2, 3]), vec![3, 0, 1, 2]);
        assert_eq!(ordered([2, 3, 1, 0]), vec![3, 0, 1, 2]);
        let mut duplicate = [
            RawBuilding {
                pointer: 1,
                rvo_agent: None,
                state: building(0),
            },
            RawBuilding {
                pointer: 2,
                rvo_agent: None,
                state: building(0),
            },
        ];
        assert!(
            sort_buildings(&mut duplicate)
                .unwrap_err()
                .contains("ambiguous building identity")
        );
    }

    fn raw_target(pointer: usize, kind: ObjectKind) -> RawCheckerTarget {
        RawCheckerTarget {
            pointer,
            qualifying_status: match kind {
                ObjectKind::Unit => CheckerQualifyingStatus::Unit {
                    alive: true,
                    active_or_available: true,
                    targetable: true,
                },
                ObjectKind::Building => CheckerQualifyingStatus::Building {
                    alive: true,
                    active_or_available: true,
                    targetable: true,
                    destroyed: false,
                },
                _ => panic!("unsupported test kind"),
            },
        }
    }

    fn completed_checker_call(ordinal: u64, tick: u64) -> CompletedCheckerCall {
        CompletedCheckerCall {
            entry: OpenCheckerCall {
                invocation_ordinal: ordinal,
                native_logic_tick: tick,
                skill: 700,
                source_actor: 11,
                source_skill_id: 5001,
                is_attacking_check: ordinal % 2 == 0,
                previous_attack_target: Some(raw_target(22, ObjectKind::Unit)),
            },
            post_attack_target_candidate: Some(raw_target(33, ObjectKind::Building)),
            quick_switch_enabled: false,
            check_return: false,
        }
    }

    fn observe_test_target(
        target: Option<RawCheckerTarget>,
        boundary: &str,
        events: &std::cell::RefCell<Vec<String>>,
        fail_method: Option<&str>,
    ) -> Result<Option<RawCheckerTarget>, String> {
        let Some(target) = target else {
            events.borrow_mut().push(format!("{boundary}:null"));
            return Ok(None);
        };
        let methods: &[&str] = match target.qualifying_status {
            CheckerQualifyingStatus::Unit { .. } => {
                &["IsAlive", "get_IsActive", "IsValidTarget(0)"]
            }
            CheckerQualifyingStatus::Building { .. } => {
                &["IsAlive", "IsAvaliable", "IsValidTarget(0)", "IsDestroyed"]
            }
        };
        for method in methods {
            events.borrow_mut().push(format!("{boundary}:{method}"));
            if fail_method == Some(method) {
                return Err(format!("forced {boundary} {method} failure"));
            }
        }
        Ok(Some(target))
    }

    #[test]
    fn checker_profile_is_private_mutually_selected_and_default_off() {
        let profile = CaptureInstrumentationProfile::SkillAttackableCheckerV1;
        assert_eq!(profile.as_str(), "skill_attackable_checker_v1");
        assert_eq!(profile.channel(), "skill_attackable_checker");
        assert!(profile.includes_skill_attackable_checker());
        assert!(!profile.includes_target_refs());
        assert!(!profile.includes_rvo());
        assert_eq!(CaptureState::default().instrumentation_profile, None);
        for inactive in [
            None,
            Some(CaptureInstrumentationProfile::TargetRefsV1),
            Some(CaptureInstrumentationProfile::TargetRefsRvoV1),
        ] {
            assert!(validate_checker_profile_start(inactive).is_ok());
        }
        let error = validate_checker_profile_start(Some(profile)).unwrap_err();
        assert!(error.contains("unsupported"));
        assert!(error.contains("independent evidence"));
    }

    #[test]
    fn selector_score_profile_is_private_mutually_selected_and_default_off() {
        let profile = CaptureInstrumentationProfile::SelectorScoreV1;
        let combined_profile = CaptureInstrumentationProfile::SelectorScoreRvoV1;
        assert_eq!(profile.as_str(), "selector_score_v1");
        assert_eq!(profile.channel(), "selector_score");
        assert!(profile.includes_selector_score());
        assert!(!profile.includes_target_refs());
        assert!(!profile.includes_rvo());
        assert!(!profile.includes_skill_attackable_checker());
        assert_eq!(combined_profile.as_str(), "selector_score_rvo_v1");
        assert_eq!(combined_profile.channel(), "selector_score_rvo");
        assert!(combined_profile.includes_selector_score());
        assert!(combined_profile.includes_rvo());
        assert!(!combined_profile.includes_target_refs());
        assert!(!combined_profile.includes_skill_attackable_checker());
        assert_eq!(CaptureState::default().instrumentation_profile, None);

        let unavailable = Metadata {
            selector_score_error: Some("forced selector hook failure".into()),
            ..Metadata::default()
        };
        for inactive in [
            None,
            Some(CaptureInstrumentationProfile::TargetRefsV1),
            Some(CaptureInstrumentationProfile::TargetRefsRvoV1),
            Some(CaptureInstrumentationProfile::SkillAttackableCheckerV1),
        ] {
            assert!(validate_selector_score_profile_availability(inactive, &unavailable).is_ok());
        }
        let error =
            validate_selector_score_profile_availability(Some(profile), &unavailable).unwrap_err();
        assert!(error.contains("selector_score_v1 is unavailable"));
        assert!(error.contains("forced selector hook failure"));
        let combined_error =
            validate_selector_score_profile_availability(Some(combined_profile), &unavailable)
                .unwrap_err();
        assert!(combined_error.contains("selector_score_rvo_v1 is unavailable"));
        assert!(combined_error.contains("forced selector hook failure"));

        let available = Metadata {
            selector_score_available: true,
            ..Metadata::default()
        };
        assert!(validate_selector_score_profile_availability(Some(profile), &available).is_ok());
        assert!(
            validate_selector_score_profile_availability(Some(combined_profile), &available)
                .is_ok()
        );
    }

    #[test]
    fn selector_score_drain_preserves_raw_values_and_clears_tick_buffer() {
        let expected = RawSelectorScoreCalculation {
            invocation_ordinal: 7,
            distance_raw: 11,
            distance_score_raw: 13,
            angle_raw: 17,
            angle_score_raw: 19,
            max_attack_range_raw: 23,
            source_rotation_raw: 29,
            min_rotation_raw: 31,
            max_rotation_raw: 37,
            is_left_side: true,
            score_raw: 41,
        };
        let mut capture = CaptureState::default();
        capture.selector_score_calculations.push(expected);

        let drained = drain_selector_score_calls(&mut capture).unwrap();
        assert_eq!(
            drained.score_calculations,
            vec![SelectorScoreCalculation {
                invocation_ordinal: expected.invocation_ordinal,
                distance_raw: expected.distance_raw,
                distance_score_raw: expected.distance_score_raw,
                angle_raw: expected.angle_raw,
                angle_score_raw: expected.angle_score_raw,
                max_attack_range_raw: expected.max_attack_range_raw,
                source_rotation_raw: expected.source_rotation_raw,
                min_rotation_raw: expected.min_rotation_raw,
                max_rotation_raw: expected.max_rotation_raw,
                is_left_side: expected.is_left_side,
                score_raw: expected.score_raw,
            }]
        );
        assert!(capture.selector_score_calculations.is_empty());
        assert!(
            drain_selector_score_calls(&mut capture)
                .unwrap()
                .score_calculations
                .is_empty()
        );
    }

    #[test]
    fn checker_offline_wrapper_forwards_exact_abi_once_when_inactive_and_active() {
        for expected_return in [false, true] {
            let calls = Cell::new(0_u32);
            let result = run_checker_wrapper_offline(
                false,
                0x11,
                expected_return,
                0x22,
                || panic!("inactive wrapper performed an entry observation"),
                |receiver, argument, method| {
                    calls.set(calls.get() + 1);
                    assert_eq!((receiver, argument, method), (0x11, expected_return, 0x22));
                    expected_return
                },
                |_, _| panic!("inactive wrapper performed a return observation"),
                |error| panic!("inactive wrapper failed: {error}"),
            );
            assert_eq!(result, expected_return);
            assert_eq!(calls.get(), 1);

            let order = std::cell::RefCell::new(Vec::new());
            calls.set(0);
            let result = run_checker_wrapper_offline(
                true,
                0x33,
                !expected_return,
                0x44,
                || {
                    order.borrow_mut().push("entry".to_owned());
                    Ok(7)
                },
                |receiver, argument, method| {
                    calls.set(calls.get() + 1);
                    order.borrow_mut().push("original".to_owned());
                    assert_eq!((receiver, argument, method), (0x33, !expected_return, 0x44));
                    expected_return
                },
                |ordinal, check_return| {
                    order.borrow_mut().push("return".to_owned());
                    assert_eq!(ordinal, 7);
                    assert_eq!(check_return, expected_return);
                    Ok(())
                },
                |error| panic!("successful active wrapper failed: {error}"),
            );
            assert_eq!(result, expected_return);
            assert_eq!(calls.get(), 1);
            assert_eq!(&*order.borrow(), &["entry", "original", "return"]);
        }
    }

    #[test]
    fn checker_offline_entry_and_return_failures_preserve_original_result_and_fail_capture() {
        for fail_at_return in [false, true] {
            let capture = std::cell::RefCell::new(CaptureState {
                armed: true,
                ..CaptureState::default()
            });
            let original_calls = Cell::new(0_u32);
            let result = run_checker_wrapper_offline(
                true,
                1,
                true,
                2,
                || {
                    if fail_at_return {
                        Ok(9)
                    } else {
                        Err("forced entry observation failure".into())
                    }
                },
                |receiver, argument, method| {
                    original_calls.set(original_calls.get() + 1);
                    assert_eq!((receiver, argument, method), (1, true, 2));
                    true
                },
                |ordinal, check_return| {
                    assert!(fail_at_return);
                    assert_eq!((ordinal, check_return), (9, true));
                    Err("forced normal-return observation failure".into())
                },
                |error| capture.borrow_mut().fail(error),
            );
            assert!(result);
            assert_eq!(original_calls.get(), 1);
            let capture = capture.borrow();
            assert!(!capture.armed);
            assert!(capture.completed_checker_calls.is_empty());
            assert!(matches!(
                capture.queue.back(),
                Some(CaptureMessage::Failure(_))
            ));
        }
    }

    #[test]
    fn checker_offline_reads_a_before_original_and_b_then_quick_switch_after_return() {
        let capture = std::cell::RefCell::new(CaptureState::default());
        let events = std::cell::RefCell::new(Vec::new());
        let previous = raw_target(22, ObjectKind::Unit);
        let post = raw_target(33, ObjectKind::Building);
        let result = run_checker_wrapper_offline(
            true,
            10,
            true,
            20,
            || {
                let previous = observe_test_target(Some(previous), "A", &events, None)?;
                open_checker_call(
                    &mut capture.borrow_mut(),
                    OpenCheckerCall {
                        invocation_ordinal: u64::MAX,
                        native_logic_tick: 50,
                        skill: 70,
                        source_actor: 11,
                        source_skill_id: 5001,
                        is_attacking_check: true,
                        previous_attack_target: previous,
                    },
                )
            },
            |receiver, argument, method| {
                events.borrow_mut().push("original".to_owned());
                assert_eq!((receiver, argument, method), (10, true, 20));
                false
            },
            |ordinal, check_return| {
                let post = observe_test_target(Some(post), "B", &events, None)?;
                events.borrow_mut().push("quick-switch-getter".to_owned());
                complete_checker_call(
                    &mut capture.borrow_mut(),
                    ordinal,
                    post,
                    false,
                    check_return,
                )
            },
            |error| panic!("successful boundary observation failed: {error}"),
        );
        assert!(!result);
        assert_eq!(
            &*events.borrow(),
            &[
                "A:IsAlive",
                "A:get_IsActive",
                "A:IsValidTarget(0)",
                "original",
                "B:IsAlive",
                "B:IsAvaliable",
                "B:IsValidTarget(0)",
                "B:IsDestroyed",
                "quick-switch-getter",
            ]
        );
        let capture = capture.borrow();
        assert!(capture.open_checker_calls.is_empty());
        assert_eq!(capture.completed_checker_calls.len(), 1);
        assert_eq!(capture.completed_checker_calls[0].entry.skill, 70);
        assert!(!capture.completed_checker_calls[0].check_return);
    }

    #[test]
    fn checker_offline_building_a_and_unit_b_use_their_exact_status_getters() {
        let events = std::cell::RefCell::new(Vec::new());
        let result = run_checker_wrapper_offline(
            true,
            1,
            false,
            2,
            || {
                let _ = observe_test_target(
                    Some(raw_target(33, ObjectKind::Building)),
                    "A",
                    &events,
                    None,
                )?;
                Ok(0)
            },
            |_, _, _| {
                events.borrow_mut().push("original".to_owned());
                false
            },
            |_, _| {
                let _ = observe_test_target(
                    Some(raw_target(22, ObjectKind::Unit)),
                    "B",
                    &events,
                    None,
                )?;
                events.borrow_mut().push("quick-switch-getter".to_owned());
                Ok(())
            },
            |error| panic!("building/unit boundary failed: {error}"),
        );
        assert!(!result);
        assert_eq!(
            &*events.borrow(),
            &[
                "A:IsAlive",
                "A:IsAvaliable",
                "A:IsValidTarget(0)",
                "A:IsDestroyed",
                "original",
                "B:IsAlive",
                "B:get_IsActive",
                "B:IsValidTarget(0)",
                "quick-switch-getter",
            ]
        );
    }

    #[test]
    fn checker_offline_null_boundaries_read_no_status_and_getter_failures_fail_closed() {
        let events = std::cell::RefCell::new(Vec::new());
        let capture = std::cell::RefCell::new(CaptureState::default());
        let result = run_checker_wrapper_offline(
            true,
            1,
            false,
            2,
            || {
                let previous = observe_test_target(None, "A", &events, None)?;
                open_checker_call(
                    &mut capture.borrow_mut(),
                    OpenCheckerCall {
                        invocation_ordinal: u64::MAX,
                        native_logic_tick: 60,
                        skill: 70,
                        source_actor: 11,
                        source_skill_id: 5001,
                        is_attacking_check: false,
                        previous_attack_target: previous,
                    },
                )
            },
            |_, _, _| {
                events.borrow_mut().push("original".to_owned());
                true
            },
            |ordinal, check_return| {
                let post = observe_test_target(None, "B", &events, None)?;
                events.borrow_mut().push("quick-switch-getter".to_owned());
                complete_checker_call(
                    &mut capture.borrow_mut(),
                    ordinal,
                    post,
                    false,
                    check_return,
                )
            },
            |error| panic!("null-boundary observation failed: {error}"),
        );
        assert!(result);
        assert_eq!(
            &*events.borrow(),
            &["A:null", "original", "B:null", "quick-switch-getter"]
        );

        for failure in ["IsAlive", "IsDestroyed", "quick-switch-getter"] {
            let events = std::cell::RefCell::new(Vec::new());
            let failures = std::cell::RefCell::new(Vec::new());
            let result = run_checker_wrapper_offline(
                true,
                1,
                true,
                2,
                || {
                    let previous = observe_test_target(
                        Some(raw_target(22, ObjectKind::Unit)),
                        "A",
                        &events,
                        (failure == "IsAlive").then_some("IsAlive"),
                    )?;
                    open_checker_call(
                        &mut CaptureState::default(),
                        OpenCheckerCall {
                            invocation_ordinal: u64::MAX,
                            native_logic_tick: 1,
                            skill: 1,
                            source_actor: 1,
                            source_skill_id: 5001,
                            is_attacking_check: true,
                            previous_attack_target: previous,
                        },
                    )
                },
                |_, _, _| {
                    events.borrow_mut().push("original".to_owned());
                    true
                },
                |_, _| {
                    let _ = observe_test_target(
                        Some(raw_target(33, ObjectKind::Building)),
                        "B",
                        &events,
                        (failure == "IsDestroyed").then_some("IsDestroyed"),
                    )?;
                    events.borrow_mut().push("quick-switch-getter".to_owned());
                    if failure == "quick-switch-getter" {
                        Err("forced quick-switch getter failure".into())
                    } else {
                        Ok(())
                    }
                },
                |error| failures.borrow_mut().push(error),
            );
            assert!(result);
            assert_eq!(failures.borrow().len(), 1);
            assert!(events.borrow().contains(&"original".to_owned()));
            if failure == "IsAlive" {
                assert!(!events.borrow().iter().any(|event| event.starts_with("B:")));
            }
        }
    }

    #[test]
    fn checker_call_json_has_exact_nine_fields_and_kind_selected_status() {
        let call = SkillAttackableCheckerCall {
            invocation_ordinal: 4,
            native_logic_tick: 90,
            source_actor: ObjectRef::new(ObjectKind::Unit, 1),
            source_skill_id: 5001,
            is_attacking_check: true,
            previous_attack_target: Some(CheckerTargetObservation {
                target_ref: ObjectRef::new(ObjectKind::Building, 2),
                qualifying_status: CheckerQualifyingStatus::Building {
                    alive: true,
                    active_or_available: true,
                    targetable: true,
                    destroyed: false,
                },
            }),
            post_attack_target_candidate: Some(CheckerTargetObservation {
                target_ref: ObjectRef::new(ObjectKind::Unit, 3),
                qualifying_status: CheckerQualifyingStatus::Unit {
                    alive: true,
                    active_or_available: true,
                    targetable: true,
                },
            }),
            quick_switch_enabled: false,
            check_return: false,
        };
        let value = serde_json::to_value(&call).unwrap();
        let object = value.as_object().unwrap();
        assert_eq!(object.len(), 9);
        let keys: BTreeSet<_> = object.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            BTreeSet::from([
                "invocation_ordinal",
                "native_logic_tick",
                "source_actor",
                "source_skill_id",
                "is_attacking_check",
                "previous_attack_target",
                "post_attack_target_candidate",
                "quick_switch_enabled",
                "check_return",
            ])
        );
        let previous = object["previous_attack_target"].as_object().unwrap();
        assert_eq!(previous.len(), 2);
        let building_status = previous["qualifying_status"].as_object().unwrap();
        assert_eq!(building_status.len(), 4);
        let post = object["post_attack_target_candidate"].as_object().unwrap();
        let unit_status = post["qualifying_status"].as_object().unwrap();
        assert_eq!(unit_status.len(), 3);
        let json = serde_json::to_string(&call).unwrap();
        for forbidden in ["post_lock_target", "pointer", "life", "qualifying_kind"] {
            assert!(!json.contains(forbidden), "unexpected {forbidden}: {json}");
        }
    }

    #[test]
    fn checker_calls_drain_by_entry_ordinal_and_never_cross_ticks() {
        let mut capture = CaptureState::default();
        capture.unit_ids.insert(11, 1);
        capture.unit_ids.insert(22, 2);
        capture.building_ids.insert(33, 1);
        capture.completed_checker_calls = vec![
            completed_checker_call(2, 40),
            completed_checker_call(0, 40),
            completed_checker_call(1, 40),
        ];
        let tick_40 = drain_skill_attackable_checker_calls(40, &mut capture).unwrap();
        assert_eq!(
            tick_40
                .checker_calls
                .iter()
                .map(|call| call.invocation_ordinal)
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert!(capture.completed_checker_calls.is_empty());

        capture.completed_checker_calls = vec![completed_checker_call(3, 41)];
        let tick_41 = drain_skill_attackable_checker_calls(41, &mut capture).unwrap();
        assert_eq!(tick_41.checker_calls[0].native_logic_tick, 41);
        assert!(capture.completed_checker_calls.is_empty());
        assert!(
            drain_skill_attackable_checker_calls(42, &mut capture)
                .unwrap()
                .checker_calls
                .is_empty()
        );
    }

    #[test]
    fn checker_nested_offline_wrappers_pair_by_local_token_and_reverse_return_order() {
        let capture = std::cell::RefCell::new(CaptureState::default());
        let events = std::cell::RefCell::new(Vec::new());
        let outer_result = run_checker_wrapper_offline(
            true,
            100,
            true,
            200,
            || {
                events.borrow_mut().push("outer-entry".to_owned());
                open_checker_call(
                    &mut capture.borrow_mut(),
                    completed_checker_call(u64::MAX, 70).entry,
                )
            },
            |_, _, _| {
                events.borrow_mut().push("outer-original-enter".to_owned());
                let inner_result = run_checker_wrapper_offline(
                    true,
                    101,
                    false,
                    201,
                    || {
                        events.borrow_mut().push("inner-entry".to_owned());
                        open_checker_call(
                            &mut capture.borrow_mut(),
                            completed_checker_call(u64::MAX, 70).entry,
                        )
                    },
                    |receiver, argument, method| {
                        events.borrow_mut().push("inner-original".to_owned());
                        assert_eq!((receiver, argument, method), (101, false, 201));
                        false
                    },
                    |ordinal, check_return| {
                        events.borrow_mut().push("inner-return".to_owned());
                        complete_checker_call(
                            &mut capture.borrow_mut(),
                            ordinal,
                            Some(raw_target(33, ObjectKind::Building)),
                            false,
                            check_return,
                        )
                    },
                    |error| panic!("inner wrapper failed: {error}"),
                );
                assert!(!inner_result);
                events.borrow_mut().push("outer-original-exit".to_owned());
                true
            },
            |ordinal, check_return| {
                events.borrow_mut().push("outer-return".to_owned());
                complete_checker_call(
                    &mut capture.borrow_mut(),
                    ordinal,
                    Some(raw_target(33, ObjectKind::Building)),
                    false,
                    check_return,
                )
            },
            |error| panic!("outer wrapper failed: {error}"),
        );
        assert!(outer_result);
        assert_eq!(
            &*events.borrow(),
            &[
                "outer-entry",
                "outer-original-enter",
                "inner-entry",
                "inner-original",
                "inner-return",
                "outer-original-exit",
                "outer-return",
            ]
        );
        {
            let capture = capture.borrow();
            assert_eq!(
                capture
                    .completed_checker_calls
                    .iter()
                    .map(|call| call.entry.invocation_ordinal)
                    .collect::<Vec<_>>(),
                vec![1, 0]
            );
        }
        let mut capture = capture.into_inner();
        capture.unit_ids.insert(11, 1);
        capture.unit_ids.insert(22, 2);
        capture.building_ids.insert(33, 1);
        let drained = drain_skill_attackable_checker_calls(70, &mut capture).unwrap();
        assert_eq!(
            drained
                .checker_calls
                .iter()
                .map(|call| (call.invocation_ordinal, call.check_return))
                .collect::<Vec<_>>(),
            vec![(0, true), (1, false)]
        );
    }

    #[test]
    fn checker_tick_mismatch_and_duplicate_completion_fail_without_carry() {
        let capture = std::cell::RefCell::new(CaptureState::default());
        let native_result = run_checker_wrapper_offline(
            true,
            1,
            true,
            2,
            || {
                open_checker_call(
                    &mut capture.borrow_mut(),
                    completed_checker_call(u64::MAX, 7).entry,
                )
            },
            |_, _, _| true,
            |ordinal, check_return| {
                complete_checker_call(
                    &mut capture.borrow_mut(),
                    ordinal,
                    Some(raw_target(33, ObjectKind::Building)),
                    false,
                    check_return,
                )
            },
            |error| panic!("tick-mismatch setup failed: {error}"),
        );
        assert!(native_result);
        assert!(capture.borrow().completed_checker_calls[0].check_return);
        let mut capture = capture.into_inner();
        capture.unit_ids.insert(11, 1);
        capture.unit_ids.insert(22, 2);
        capture.building_ids.insert(33, 1);
        assert!(drain_skill_attackable_checker_calls(8, &mut capture).is_err());
        assert!(capture.completed_checker_calls.is_empty());
        assert!(
            drain_skill_attackable_checker_calls(9, &mut capture)
                .unwrap()
                .checker_calls
                .is_empty()
        );

        capture.completed_checker_calls =
            vec![completed_checker_call(1, 10), completed_checker_call(1, 10)];
        assert!(drain_skill_attackable_checker_calls(10, &mut capture).is_err());
        assert!(capture.completed_checker_calls.is_empty());
    }

    #[test]
    fn checker_open_call_and_kind_mismatch_fail_closed_and_reset_clears_session() {
        let mut capture = CaptureState::default();
        let open = completed_checker_call(0, 12).entry;
        capture.open_checker_calls.insert(0, open);
        capture.completed_checker_calls = vec![completed_checker_call(1, 12)];
        assert!(drain_skill_attackable_checker_calls(12, &mut capture).is_err());
        assert!(capture.completed_checker_calls.is_empty());
        capture.reset_session();
        assert!(capture.open_checker_calls.is_empty());
        assert!(capture.completed_checker_calls.is_empty());
        assert_eq!(capture.next_checker_invocation_ordinal, 0);

        capture.unit_ids.insert(11, 1);
        capture.unit_ids.insert(22, 2);
        capture.building_ids.insert(33, 1);
        let mut call = completed_checker_call(0, 13);
        call.post_attack_target_candidate = Some(raw_target(33, ObjectKind::Unit));
        capture.completed_checker_calls.push(call);
        assert!(drain_skill_attackable_checker_calls(13, &mut capture).is_err());
        assert!(capture.completed_checker_calls.is_empty());
    }

    #[test]
    fn checker_timeout_failed_close_and_explicit_stop_reject_unreturned_calls() {
        for boundary in ["timeout", "failed-session close", "explicit stop"] {
            let mut capture = CaptureState::default();
            capture
                .open_checker_calls
                .insert(0, completed_checker_call(0, 80).entry);
            capture.completed_checker_calls = vec![completed_checker_call(1, 80)];
            let error = reject_pending_checker_calls(&mut capture, boundary).unwrap_err();
            assert!(error.contains(boundary));
            assert!(error.contains("1 open and 1 undrained"));
            assert!(capture.open_checker_calls.is_empty());
            assert!(capture.completed_checker_calls.is_empty());
            capture.next_checker_invocation_ordinal = 9;
            capture.reset_session();
            assert_eq!(capture.next_checker_invocation_ordinal, 0);
        }
    }

    #[test]
    fn checker_mcfr_disagreement_retains_both_observations() {
        let direct = CheckerTargetObservation {
            target_ref: ObjectRef::new(ObjectKind::Unit, 1),
            qualifying_status: CheckerQualifyingStatus::Unit {
                alive: true,
                active_or_available: true,
                targetable: true,
            },
        };
        let mut paired = unit(1, 0, 1);
        paired.targetable = false;
        let world = WorldSnapshot {
            live_units: vec![paired],
            buildings: vec![building(1)],
            ..WorldSnapshot::default()
        };
        assert_eq!(
            corroborate_checker_status(&direct, &world).unwrap(),
            CheckerStatusCorroboration::Disagreement {
                direct: direct.qualifying_status,
                mcfr: CheckerQualifyingStatus::Unit {
                    alive: true,
                    active_or_available: true,
                    targetable: false,
                },
            }
        );
        let building_direct = CheckerTargetObservation {
            target_ref: ObjectRef::new(ObjectKind::Building, 1),
            qualifying_status: CheckerQualifyingStatus::Building {
                alive: true,
                active_or_available: true,
                targetable: true,
                destroyed: false,
            },
        };
        assert_eq!(
            corroborate_checker_status(&building_direct, &world).unwrap(),
            CheckerStatusCorroboration::Consistent
        );
    }

    #[test]
    fn checker_profile_reuses_one_generic_sidecar_row_per_mcfr_tick() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("checker.h5");
        let profile = CaptureInstrumentationProfile::SkillAttackableCheckerV1;
        let mut writer = mechcore_mcfr::InstrumentationWriter::create(
            &path,
            &"11".repeat(32),
            profile.as_str(),
            "adapter",
        )
        .unwrap();
        for step in 0..2 {
            writer
                .record_json(
                    step,
                    profile.channel(),
                    &SkillAttackableCheckerObservation::default(),
                )
                .unwrap();
        }
        writer.finish().unwrap();
        let reader = mechcore_mcfr::InstrumentationReader::open(&path).unwrap();
        assert_eq!(reader.physics_result_hash(), "11".repeat(32));
        assert_eq!(reader.profile(), "skill_attackable_checker_v1");
        assert_eq!(reader.producer(), "adapter");
        assert_eq!(reader.len(), 2);
        for index in 0..2 {
            let entry = reader.entry(index).unwrap();
            assert_eq!(entry.step, index as u64);
            assert_eq!(entry.channel, "skill_attackable_checker");
            assert_eq!(entry.content_type, "application/json");
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&entry.payload).unwrap(),
                serde_json::json!({"checker_calls": []})
            );
        }
    }

    #[test]
    fn checker_profile_does_not_change_existing_instrumentation_payload_types() {
        let target_refs = CaptureInstrumentationObservation::TargetRefs(TargetRefsObservation {
            units: Vec::new(),
        });
        let target_refs_rvo =
            CaptureInstrumentationObservation::TargetRefsRvo(TargetRefsRvoObservation {
                target_refs: TargetRefsObservation { units: Vec::new() },
                rvo_updates: Vec::new(),
            });
        let checker = CaptureInstrumentationObservation::SkillAttackableChecker(
            SkillAttackableCheckerObservation::default(),
        );
        assert_eq!(
            serde_json::to_value(target_refs).unwrap(),
            serde_json::json!({"units": []})
        );
        assert_eq!(
            serde_json::to_value(target_refs_rvo).unwrap(),
            serde_json::json!({"target_refs": {"units": []}, "rvo_updates": []})
        );
        assert_eq!(
            serde_json::to_value(checker).unwrap(),
            serde_json::json!({"checker_calls": []})
        );
    }

    #[test]
    fn rvo_profile_payload_and_generic_sidecar_contract() {
        let target_profile = CaptureInstrumentationProfile::TargetRefsV1;
        let rvo_profile = CaptureInstrumentationProfile::TargetRefsRvoV1;
        let combined_profile = CaptureInstrumentationProfile::SelectorScoreRvoV1;
        assert_eq!(target_profile.as_str(), "target_refs_v1");
        assert_eq!(target_profile.channel(), "target_refs");
        assert!(target_profile.includes_target_refs());
        assert!(!target_profile.includes_rvo());
        assert_eq!(rvo_profile.as_str(), "target_refs_rvo_v1");
        assert_eq!(rvo_profile.channel(), "target_refs_rvo");
        assert!(rvo_profile.includes_target_refs());
        assert!(rvo_profile.includes_rvo());
        assert_eq!(combined_profile.as_str(), "selector_score_rvo_v1");
        assert_eq!(combined_profile.channel(), "selector_score_rvo");
        assert!(!combined_profile.includes_target_refs());
        assert!(combined_profile.includes_selector_score());
        assert!(combined_profile.includes_rvo());

        let target_payload = CaptureInstrumentationObservation::TargetRefs(target_refs());
        let target_json = serde_json::to_value(&target_payload).unwrap();
        assert_eq!(target_json, serde_json::json!({"units": []}));
        let rvo_payload =
            CaptureInstrumentationObservation::TargetRefsRvo(TargetRefsRvoObservation {
                target_refs: target_refs(),
                rvo_updates: Vec::new(),
            });
        let rvo_json = serde_json::to_value(&rvo_payload).unwrap();
        let keys: BTreeSet<_> = rvo_json.as_object().unwrap().keys().cloned().collect();
        assert_eq!(
            keys,
            BTreeSet::from(["rvo_updates".to_owned(), "target_refs".to_owned()])
        );
        assert_eq!(rvo_json["target_refs"], target_json);
        assert_eq!(rvo_json["rvo_updates"], serde_json::json!([]));
        let combined_payload =
            CaptureInstrumentationObservation::SelectorScoreRvo(SelectorScoreRvoObservation {
                selector_score: SelectorScoreObservation::default(),
                rvo_updates: Vec::new(),
            });
        let combined_json = serde_json::to_value(&combined_payload).unwrap();
        assert_eq!(
            combined_json,
            serde_json::json!({
                "selector_score": {"score_calculations": []},
                "rvo_updates": []
            })
        );

        let directory = tempfile::tempdir().unwrap();
        let physics_result_hash = "00".repeat(32);
        for (index, profile, payload, expected) in [
            (0, target_profile, &target_payload, &target_json),
            (1, rvo_profile, &rvo_payload, &rvo_json),
            (2, combined_profile, &combined_payload, &combined_json),
        ] {
            let path = directory.path().join(format!("profile-{index}.h5"));
            let mut writer = mechcore_mcfr::InstrumentationWriter::create(
                &path,
                &physics_result_hash,
                profile.as_str(),
                "adapter-offline-test",
            )
            .unwrap();
            writer
                .record_json(index, profile.channel(), payload)
                .unwrap();
            writer.finish().unwrap();
            let reader = mechcore_mcfr::InstrumentationReader::open(path).unwrap();
            assert_eq!(reader.physics_result_hash(), physics_result_hash);
            assert_eq!(reader.profile(), profile.as_str());
            assert_eq!(reader.producer(), "adapter-offline-test");
            assert_eq!(reader.len(), 1);
            let entry = reader.entry(0).unwrap();
            assert_eq!(entry.step, index);
            assert_eq!(entry.channel, profile.channel());
            assert_eq!(entry.content_type, "application/json");
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&entry.payload).unwrap(),
                *expected
            );
        }
    }

    #[test]
    fn rvo_agent_identity_precedence_and_stability() {
        let unit_ref = ObjectRef::new(ObjectKind::Unit, 1);
        let other_unit_ref = ObjectRef::new(ObjectKind::Unit, 2);
        let building_ref = ObjectRef::new(ObjectKind::Building, 3);
        let mut state = CaptureState::default();
        state.unit_ids.insert(10, 1);
        state.unit_ids.insert(11, 2);
        state.building_ids.insert(12, 3);
        state.rvo_agent_refs.insert(100, other_unit_ref);
        state.rvo_agent_refs.insert(101, building_ref);
        state.rvo_agent_owners.insert(100, 10);
        state.rvo_agent_owners.insert(102, 10);

        assert!(matches!(
            resolve_rvo_agent_ref(100, &mut state).unwrap(),
            RvoAgentRefObservation::Entity(reference) if reference == other_unit_ref
        ));
        assert!(matches!(
            resolve_rvo_agent_ref(101, &mut state).unwrap(),
            RvoAgentRefObservation::Entity(reference) if reference == building_ref
        ));
        assert!(matches!(
            resolve_rvo_agent_ref(102, &mut state).unwrap(),
            RvoAgentRefObservation::Entity(reference) if reference == unit_ref
        ));
        assert_internal(resolve_rvo_agent_ref(200, &mut state).unwrap(), 0);
        assert_internal(resolve_rvo_agent_ref(200, &mut state).unwrap(), 0);
        assert_internal(resolve_rvo_agent_ref(201, &mut state).unwrap(), 1);
        state.next_rvo_internal_agent_id = u64::MAX;
        let error = resolve_rvo_agent_ref(202, &mut state).unwrap_err();
        assert_eq!(error, "RVO internal agent identity overflow");
        assert!(!state.rvo_internal_agent_ids.contains_key(&202));
    }

    #[test]
    fn rvo_scope_rejects_unbounded_or_ambiguous_requests() {
        let scope = RvoCaptureScope {
            start_tick: 8,
            end_tick: 14,
            unit_ids: vec![124, 282, 363, 246],
        };
        assert!(
            scope
                .validate(CaptureInstrumentationProfile::TargetRefsRvoV1)
                .is_ok()
        );
        assert!(
            scope
                .validate(CaptureInstrumentationProfile::TargetRefsV1)
                .is_err()
        );
        assert!(!scope.includes_native_tick(7));
        assert!(!scope.includes_native_tick(700));
        assert!(scope.includes_native_tick(800));
        assert!(scope.includes_native_tick(1400));
        assert!(!scope.includes_native_tick(1500));
        for ids in [vec![], vec![0], vec![1, 1], (1..=9).collect()] {
            assert!(
                RvoCaptureScope {
                    unit_ids: ids,
                    ..scope.clone()
                }
                .validate(CaptureInstrumentationProfile::TargetRefsRvoV1)
                .is_err()
            );
        }
        for (start, end) in [(14, 8), (1, 65), (u64::MAX, 0)] {
            assert!(
                RvoCaptureScope {
                    start_tick: start,
                    end_tick: end,
                    ..scope.clone()
                }
                .validate(CaptureInstrumentationProfile::TargetRefsRvoV1)
                .is_err()
            );
        }
    }

    #[test]
    fn rvo_scope_filters_sources_and_drains_delayed_publication_with_native_ordinals() {
        let mut state = CaptureState::default();
        state.rvo_scope = Some(RvoCaptureScope {
            start_tick: 8,
            end_tick: 14,
            unit_ids: vec![124],
        });
        state
            .rvo_agent_refs
            .insert(10, ObjectRef::new(ObjectKind::Unit, 124));
        state
            .rvo_agent_refs
            .insert(20, ObjectRef::new(ObjectKind::Unit, 246));
        state.unit_ids.insert(30, 124);
        state.rvo_agent_owners.insert(40, 30);
        assert!(rvo_source_selected(&state, 10));
        assert!(rvo_source_selected(&state, 40));
        assert!(!rvo_source_selected(&state, 20));
        assert!(!rvo_source_selected(&state, 99));
        seed_rvo_update(&mut state, 1, 700, 800, true, 0, true);
        seed_rvo_update(&mut state, 2, 1400, 1600, true, 0, true);
        state.rvo_agent_sets.insert(
            2,
            vec![NativeRvoAgentState {
                ordinal: 501,
                pointer: 10,
                ..NativeRvoAgentState::default()
            }],
        );
        state.rvo_published_agent_sets.insert(
            2,
            vec![NativeRvoAgentState {
                ordinal: 501,
                pointer: 10,
                published_calculated_speed: FixedPoint {
                    raw: 9_007_199_254_740_993,
                },
                ..NativeRvoAgentState::default()
            }],
        );
        assert!(resolve_rvo_updates(1400, &mut state).unwrap().is_empty());
        assert!(!state.rvo_update_modes.contains_key(&1));
        assert!(state.rvo_published_agent_sets.contains_key(&2));
        let updates = resolve_rvo_updates(1600, &mut state).unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].agents[0].ordinal, 501);
        assert_eq!(updates[0].publish_native_tick, 1600);
        assert!(updates[0].multithreaded);
        assert_eq!(
            updates[0].published_agents[0].published_calculated_speed_raw,
            9_007_199_254_740_993
        );
        assert!(state.rvo_published_agent_sets.is_empty());
        state.reset_session();
        assert!(state.rvo_scope.is_none());
    }

    #[test]
    fn rvo_drain_readiness_ordering_and_source_join() {
        let mut state = CaptureState::default();
        state
            .rvo_agent_refs
            .insert(10, ObjectRef::new(ObjectKind::Unit, 1));
        state
            .rvo_agent_refs
            .insert(20, ObjectRef::new(ObjectKind::Unit, 2));
        seed_rvo_update(&mut state, 1, 4, 4, false, 11, false);
        seed_rvo_update(&mut state, 2, 4, 5, true, 22, true);
        seed_rvo_update(&mut state, 3, 5, 6, true, 33, true);
        state.rvo_agent_sets.insert(
            1,
            vec![
                NativeRvoAgentState {
                    pointer: 20,
                    ..NativeRvoAgentState::default()
                },
                NativeRvoAgentState {
                    pointer: 10,
                    ordinal: 1,
                    ..NativeRvoAgentState::default()
                },
            ],
        );
        state.rvo_neighbour_sets.extend([
            NativeRvoNeighbourSet {
                update_ordinal: 1,
                source_call_ordinal: 9,
                source: 10,
                neighbours: vec![(20, 90)],
            },
            NativeRvoNeighbourSet {
                update_ordinal: 1,
                source_call_ordinal: 3,
                source: 10,
                neighbours: vec![(20, 30)],
            },
            NativeRvoNeighbourSet {
                update_ordinal: 3,
                source_call_ordinal: 1,
                source: 10,
                neighbours: Vec::new(),
            },
        ]);
        state.rvo_vo_buffers.extend([
            NativeRvoVoBuffer {
                update_ordinal: 1,
                call_ordinal: 8,
                source: 10,
                vos: vec![NativeRvoVo::default()],
            },
            NativeRvoVoBuffer {
                update_ordinal: 1,
                call_ordinal: 4,
                source: 10,
                vos: vec![NativeRvoVo::default()],
            },
        ]);
        state.opponent_vos.extend([
            NativeOpponentVo {
                update_ordinal: 1,
                call_ordinal: 7,
                source: 10,
                target: 20,
                vo_buffer_length_before: 0,
                vo_buffer_length_after: 1,
                appended_colliding: false,
            },
            NativeOpponentVo {
                update_ordinal: 1,
                call_ordinal: 2,
                source: 10,
                target: 20,
                vo_buffer_length_before: 1,
                vo_buffer_length_after: 2,
                appended_colliding: true,
            },
        ]);

        let observation = resolve_rvo_observation(target_refs(), 5, &mut state).unwrap();
        assert_eq!(
            observation
                .rvo_updates
                .iter()
                .map(|update| update.update_ordinal)
                .collect::<Vec<_>>(),
            [1, 2]
        );
        assert!(!observation.rvo_updates[0].double_buffering);
        assert!(observation.rvo_updates[1].double_buffering);
        assert_eq!(observation.rvo_updates[0].publish_native_tick, 4);
        assert_eq!(observation.rvo_updates[1].publish_native_tick, 5);
        assert_eq!(
            observation.rvo_updates[0]
                .agents
                .iter()
                .map(|agent| agent.ordinal)
                .collect::<Vec<_>>(),
            [0, 1]
        );
        assert_eq!(
            observation.rvo_updates[0]
                .neighbour_sets
                .iter()
                .map(|set| set.source_call_ordinal)
                .collect::<Vec<_>>(),
            [3, 9]
        );
        assert_eq!(
            observation.rvo_updates[0]
                .vo_buffers
                .iter()
                .map(|buffer| buffer.call_ordinal)
                .collect::<Vec<_>>(),
            [4, 8]
        );
        assert_eq!(
            observation.rvo_updates[0]
                .opponent_vos
                .iter()
                .map(|opponent| opponent.call_ordinal)
                .collect::<Vec<_>>(),
            [2, 7]
        );
        assert_eq!(state.rvo_update_publish_native_ticks.len(), 1);
        assert_eq!(state.rvo_update_publish_native_ticks.get(&3), Some(&6));
        assert_eq!(state.rvo_neighbour_sets.len(), 1);
        assert_eq!(state.rvo_neighbour_sets[0].update_ordinal, 3);

        for opponent_only in [false, true] {
            let mut missing_source = CaptureState::default();
            missing_source
                .rvo_agent_refs
                .insert(10, ObjectRef::new(ObjectKind::Unit, 1));
            missing_source
                .rvo_agent_refs
                .insert(20, ObjectRef::new(ObjectKind::Unit, 2));
            seed_rvo_update(&mut missing_source, 1, 1, 1, false, 1, false);
            if opponent_only {
                missing_source.opponent_vos.push(NativeOpponentVo {
                    update_ordinal: 1,
                    call_ordinal: 1,
                    source: 10,
                    target: 20,
                    vo_buffer_length_before: 0,
                    vo_buffer_length_after: 1,
                    appended_colliding: false,
                });
            } else {
                missing_source.rvo_vo_buffers.push(NativeRvoVoBuffer {
                    update_ordinal: 1,
                    call_ordinal: 1,
                    source: 10,
                    vos: Vec::new(),
                });
            }
            let error = resolve_rvo_observation(target_refs(), 1, &mut missing_source).unwrap_err();
            assert!(error.contains("has no CalculateNeighbours source"));
        }
    }

    #[test]
    fn rvo_symmetry_bias_serialization_and_ready_cleanup() {
        let mut state = CaptureState::default();
        seed_rvo_update(&mut state, 4, 8, 9, true, 429_496_729, true);
        let observation = resolve_rvo_observation(target_refs(), 9, &mut state).unwrap();
        assert_eq!(observation.rvo_updates.len(), 1);
        assert_eq!(
            observation.rvo_updates[0].symmetry_breaking_bias_raw,
            429_496_729
        );
        assert_eq!(
            serde_json::to_value(&observation).unwrap()["rvo_updates"][0]["symmetry_breaking_bias_raw"],
            serde_json::json!(429_496_729)
        );
        assert!(!state.rvo_update_modes.contains_key(&4));
        assert!(!state.rvo_update_symmetry_breaking_biases.contains_key(&4));
        assert!(!state.rvo_update_start_native_ticks.contains_key(&4));
        assert!(!state.rvo_update_publish_native_ticks.contains_key(&4));
        assert!(!state.rvo_update_multithreaded.contains_key(&4));

        let mut zero_activation = CaptureState::default();
        zero_activation.rvo_update_modes.insert(5, false);
        zero_activation
            .rvo_update_symmetry_breaking_biases
            .insert(5, FixedPoint { raw: 99 });
        zero_activation.rvo_update_start_native_ticks.insert(5, 10);
        zero_activation.rvo_update_multithreaded.insert(5, false);
        discard_unpublished_rvo_update(&mut zero_activation, 5);
        assert!(zero_activation.rvo_update_modes.is_empty());
        assert!(
            zero_activation
                .rvo_update_symmetry_breaking_biases
                .is_empty()
        );
        assert!(zero_activation.rvo_update_start_native_ticks.is_empty());
        assert!(zero_activation.rvo_update_multithreaded.is_empty());

        let mut missing_bias = CaptureState::default();
        seed_rvo_update(&mut missing_bias, 6, 1, 1, false, 1, false);
        missing_bias.rvo_update_symmetry_breaking_biases.clear();
        let error = resolve_rvo_observation(target_refs(), 1, &mut missing_bias).unwrap_err();
        assert_eq!(error, "native RVO update 6 lost its symmetry bias");
    }

    #[test]
    fn rvo_stop_and_failure_cleanup() {
        let _guard = RVO_GLOBAL_TEST_LOCK.lock().unwrap();
        let mut state = CaptureState {
            armed: true,
            ..CaptureState::default()
        };
        state.rvo_agent_owners.insert(100, 10);
        state.rvo_neighbour_sets.push(NativeRvoNeighbourSet {
            update_ordinal: 1,
            source_call_ordinal: 1,
            source: 100,
            neighbours: Vec::new(),
        });
        state
            .rvo_agent_sets
            .insert(1, vec![NativeRvoAgentState::default()]);
        state.rvo_vo_buffers.push(NativeRvoVoBuffer {
            update_ordinal: 1,
            call_ordinal: 1,
            source: 100,
            vos: Vec::new(),
        });
        state.opponent_vos.push(NativeOpponentVo {
            update_ordinal: 1,
            call_ordinal: 2,
            source: 100,
            target: 200,
            vo_buffer_length_before: 0,
            vo_buffer_length_after: 1,
            appended_colliding: false,
        });
        seed_rvo_update(&mut state, 1, 1, 2, true, 3, true);
        state.fail("forced capture failure".into());
        assert!(!state.armed);
        assert!(matches!(
            state.queue.back(),
            Some(CaptureMessage::Failure(reason)) if reason == "forced capture failure"
        ));
        clear_pending_rvo_state(&mut state);
        assert!(state.rvo_neighbour_sets.is_empty());
        assert!(state.rvo_agent_sets.is_empty());
        assert!(state.rvo_vo_buffers.is_empty());
        assert!(state.opponent_vos.is_empty());
        assert!(state.rvo_update_modes.is_empty());
        assert!(state.rvo_update_symmetry_breaking_biases.is_empty());
        assert!(state.rvo_update_start_native_ticks.is_empty());
        assert!(state.rvo_update_publish_native_ticks.is_empty());
        assert!(state.rvo_update_multithreaded.is_empty());
        assert_eq!(state.rvo_agent_owners.get(&100), Some(&10));

        ACTIVE_RVO_UPDATE.store(1, Ordering::Release);
        CURRENT_RVO_FIXED_UPDATE.store(2, Ordering::Release);
        CURRENT_RVO_ACTIVATION_COUNT.store(3, Ordering::Release);
        RVO_ACTIVATION_COUNT.store(4, Ordering::Release);
        reset_rvo_sentinels();
        assert_eq!(ACTIVE_RVO_UPDATE.load(Ordering::Acquire), u64::MAX);
        assert_eq!(CURRENT_RVO_FIXED_UPDATE.load(Ordering::Acquire), u64::MAX);
        assert_eq!(CURRENT_RVO_ACTIVATION_COUNT.load(Ordering::Acquire), 0);
        assert_eq!(RVO_ACTIVATION_COUNT.load(Ordering::Acquire), 0);
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn rvo_seven_hook_contract_and_inactive_profile_behavior() {
        let _guard = RVO_GLOBAL_TEST_LOCK.lock().unwrap();
        struct Contract {
            method: &'static str,
            abi: &'static str,
            wrapper: &'static str,
            installer: &'static str,
            original_slot: &'static str,
            prologue: [u8; 16],
        }
        let contracts = [
            Contract {
                method: RVO_CONTROLLER_ACTIVE_LABEL,
                abi: "unsafe extern C fn(controller, method_info) -> ()",
                wrapper: "rvo_controller_active_hook",
                installer: "install_rvo_controller_active_hook",
                original_slot: "ORIGINAL_RVO_CONTROLLER_ACTIVE",
                prologue: RVO_CONTROLLER_ACTIVE_PROLOGUE,
            },
            Contract {
                method: RVO_ADD_AGENT_FIXED_LABEL,
                abi: "unsafe extern C fn(simulator, agent, method_info) -> object",
                wrapper: "rvo_add_agent_fixed_hook",
                installer: "install_rvo_add_agent_fixed_hook",
                original_slot: "ORIGINAL_RVO_ADD_AGENT_FIXED",
                prologue: RVO_ADD_AGENT_FIXED_PROLOGUE,
            },
            Contract {
                method: RVO_FIXED_UPDATE_LABEL,
                abi: "unsafe extern C fn(simulator, method_info) -> ()",
                wrapper: "rvo_fixed_update_hook",
                installer: "install_rvo_fixed_update_hook",
                original_slot: "ORIGINAL_RVO_FIXED_UPDATE",
                prologue: RVO_FIXED_UPDATE_PROLOGUE,
            },
            Contract {
                method: RVO_PRE_CALCULATION_LABEL,
                abi: "unsafe extern C fn(simulator, method_info) -> ()",
                wrapper: "rvo_pre_calculation_hook",
                installer: "install_rvo_pre_calculation_hook",
                original_slot: "ORIGINAL_RVO_PRE_CALCULATION",
                prologue: RVO_PRE_CALCULATION_PROLOGUE,
            },
            Contract {
                method: RVO_CALCULATE_NEIGHBOURS_LABEL,
                abi: "unsafe extern C fn(agent, method_info) -> ()",
                wrapper: "rvo_calculate_neighbours_hook",
                installer: "install_rvo_calculate_neighbours_hook",
                original_slot: "ORIGINAL_RVO_CALCULATE_NEIGHBOURS",
                prologue: RVO_CALCULATE_NEIGHBOURS_PROLOGUE,
            },
            Contract {
                method: RVO_GENERATE_NEIGHBOUR_VOS_LABEL,
                abi: "unsafe extern C fn(agent, vo_buffer, method_info) -> ()",
                wrapper: "rvo_generate_neighbour_vos_hook",
                installer: "install_rvo_generate_neighbour_vos_hook",
                original_slot: "ORIGINAL_RVO_GENERATE_NEIGHBOUR_VOS",
                prologue: RVO_GENERATE_NEIGHBOUR_VOS_PROLOGUE,
            },
            Contract {
                method: RVO_GENERATE_OPPONENT_VOS_LABEL,
                abi: "unsafe extern C fn(agent, vo_buffer, other, method_info) -> ()",
                wrapper: "rvo_generate_opponent_vos_hook",
                installer: "install_rvo_generate_opponent_vos_hook",
                original_slot: "ORIGINAL_RVO_GENERATE_OPPONENT_VOS",
                prologue: RVO_GENERATE_OPPONENT_VOS_PROLOGUE,
            },
        ];
        assert_eq!(contracts.len(), RVO_HOOK_COUNT);
        assert_eq!(
            contracts
                .iter()
                .map(|entry| entry.method)
                .collect::<Vec<_>>(),
            [
                "RVOControllerFixed.Active",
                "RVO Simulator.AddAgentFixed",
                "RVO Simulator.FixedUpdate",
                "RVO Simulator.PreCalculation",
                "RVO Agent.CalculateNeighbours",
                "RVOAgentFixed.GenerateNeighbourAgentVOs",
                "RVOAgentFixed.GenerateOpponentVOs",
            ]
        );
        assert_eq!(
            contracts
                .iter()
                .map(|entry| entry.wrapper)
                .collect::<Vec<_>>(),
            [
                "rvo_controller_active_hook",
                "rvo_add_agent_fixed_hook",
                "rvo_fixed_update_hook",
                "rvo_pre_calculation_hook",
                "rvo_calculate_neighbours_hook",
                "rvo_generate_neighbour_vos_hook",
                "rvo_generate_opponent_vos_hook",
            ]
        );
        assert_eq!(
            contracts
                .iter()
                .map(|entry| entry.installer)
                .collect::<Vec<_>>(),
            [
                "install_rvo_controller_active_hook",
                "install_rvo_add_agent_fixed_hook",
                "install_rvo_fixed_update_hook",
                "install_rvo_pre_calculation_hook",
                "install_rvo_calculate_neighbours_hook",
                "install_rvo_generate_neighbour_vos_hook",
                "install_rvo_generate_opponent_vos_hook",
            ]
        );
        assert_eq!(
            contracts
                .iter()
                .map(|entry| entry.original_slot)
                .collect::<Vec<_>>(),
            [
                "ORIGINAL_RVO_CONTROLLER_ACTIVE",
                "ORIGINAL_RVO_ADD_AGENT_FIXED",
                "ORIGINAL_RVO_FIXED_UPDATE",
                "ORIGINAL_RVO_PRE_CALCULATION",
                "ORIGINAL_RVO_CALCULATE_NEIGHBOURS",
                "ORIGINAL_RVO_GENERATE_NEIGHBOUR_VOS",
                "ORIGINAL_RVO_GENERATE_OPPONENT_VOS",
            ]
        );
        assert_eq!(
            contracts.iter().map(|entry| entry.abi).collect::<Vec<_>>(),
            [
                "unsafe extern C fn(controller, method_info) -> ()",
                "unsafe extern C fn(simulator, agent, method_info) -> object",
                "unsafe extern C fn(simulator, method_info) -> ()",
                "unsafe extern C fn(simulator, method_info) -> ()",
                "unsafe extern C fn(agent, method_info) -> ()",
                "unsafe extern C fn(agent, vo_buffer, method_info) -> ()",
                "unsafe extern C fn(agent, vo_buffer, other, method_info) -> ()",
            ]
        );
        assert_eq!(
            contracts
                .iter()
                .map(|entry| entry.prologue)
                .collect::<Vec<_>>(),
            [
                [
                    0xff, 0xc3, 0x01, 0xd1, 0xf8, 0x5f, 0x03, 0xa9, 0xf6, 0x57, 0x04, 0xa9, 0xf4,
                    0x4f, 0x05, 0xa9
                ],
                [
                    0xf6, 0x57, 0xbd, 0xa9, 0xf4, 0x4f, 0x01, 0xa9, 0xfd, 0x7b, 0x02, 0xa9, 0xfd,
                    0x83, 0x00, 0x91
                ],
                [
                    0xf8, 0x5f, 0xbc, 0xa9, 0xf6, 0x57, 0x01, 0xa9, 0xf4, 0x4f, 0x02, 0xa9, 0xfd,
                    0x7b, 0x03, 0xa9
                ],
                [
                    0xf6, 0x57, 0xbd, 0xa9, 0xf4, 0x4f, 0x01, 0xa9, 0xfd, 0x7b, 0x02, 0xa9, 0xfd,
                    0x83, 0x00, 0x91
                ],
                [
                    0xf4, 0x4f, 0xbe, 0xa9, 0xfd, 0x7b, 0x01, 0xa9, 0xfd, 0x43, 0x00, 0x91, 0xf3,
                    0x03, 0x00, 0xaa
                ],
                [
                    0xed, 0x33, 0xb7, 0x6d, 0xeb, 0x2b, 0x01, 0x6d, 0xe9, 0x23, 0x02, 0x6d, 0xfc,
                    0x6f, 0x03, 0xa9
                ],
                [
                    0xff, 0xc3, 0x07, 0xd1, 0xfc, 0x6f, 0x19, 0xa9, 0xfa, 0x67, 0x1a, 0xa9, 0xf8,
                    0x5f, 0x1b, 0xa9
                ],
            ]
        );

        let mismatch = verify_rvo_hook_prologue(
            &[0; 16],
            &RVO_CONTROLLER_ACTIVE_PROLOGUE,
            RVO_CONTROLLER_ACTIVE_LABEL,
        )
        .unwrap_err();
        assert!(mismatch.contains("RVOControllerFixed.Active prologue mismatch"));
        assert!(mismatch.ends_with("00000000000000000000000000000000"));

        let mut attempted = Vec::new();
        let install_error = run_rvo_hook_install_sequence(|index| {
            attempted.push(index);
            if index == 3 {
                Err("forced RVO hook 3 failure".into())
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert_eq!(attempted, [0, 1, 2, 3]);
        let unavailable = Metadata {
            rvo_error: Some(install_error.clone()),
            ..Metadata::default()
        };
        assert!(validate_rvo_profile_availability(None, &unavailable).is_ok());
        assert!(
            validate_rvo_profile_availability(
                Some(CaptureInstrumentationProfile::TargetRefsV1),
                &unavailable
            )
            .is_ok()
        );
        let profile_error = validate_rvo_profile_availability(
            Some(CaptureInstrumentationProfile::TargetRefsRvoV1),
            &unavailable,
        )
        .unwrap_err();
        assert!(profile_error.contains(&install_error));

        for counter in &TEST_HOOK_CALLS {
            counter.store(0, Ordering::Release);
        }
        for arguments in &TEST_HOOK_ARGUMENTS {
            for argument in arguments {
                argument.store(0, Ordering::Release);
            }
        }
        RUNTIME.store(ptr::null_mut(), Ordering::Release);
        reset_rvo_sentinels();
        ORIGINAL_RVO_CONTROLLER_ACTIVE
            .store(test_rvo_controller_active as *mut c_void, Ordering::Release);
        ORIGINAL_RVO_ADD_AGENT_FIXED
            .store(test_rvo_add_agent_fixed as *mut c_void, Ordering::Release);
        ORIGINAL_RVO_FIXED_UPDATE.store(test_rvo_fixed_update as *mut c_void, Ordering::Release);
        ORIGINAL_RVO_PRE_CALCULATION
            .store(test_rvo_pre_calculation as *mut c_void, Ordering::Release);
        ORIGINAL_RVO_CALCULATE_NEIGHBOURS.store(
            test_rvo_calculate_neighbours as *mut c_void,
            Ordering::Release,
        );
        ORIGINAL_RVO_GENERATE_NEIGHBOUR_VOS.store(
            test_rvo_generate_neighbour_vos as *mut c_void,
            Ordering::Release,
        );
        ORIGINAL_RVO_GENERATE_OPPONENT_VOS.store(
            test_rvo_generate_opponent_vos as *mut c_void,
            Ordering::Release,
        );
        // SAFETY: the test slots contain functions with the exact declared ABIs,
        // and the opaque pointer values are only forwarded and recorded.
        let added = unsafe {
            rvo_controller_active_hook(
                0x101_usize as *mut Object,
                0x102_usize as *const MethodInfo,
            );
            let added = rvo_add_agent_fixed_hook(
                0x201_usize as *mut Object,
                0x202_usize as *mut Object,
                0x203_usize as *const MethodInfo,
            );
            rvo_fixed_update_hook(0x301_usize as *mut Object, 0x302_usize as *const MethodInfo);
            rvo_pre_calculation_hook(0x401_usize as *mut Object, 0x402_usize as *const MethodInfo);
            rvo_calculate_neighbours_hook(
                0x501_usize as *mut Object,
                0x502_usize as *const MethodInfo,
            );
            rvo_generate_neighbour_vos_hook(
                0x601_usize as *mut Object,
                0x602_usize as *mut Object,
                0x603_usize as *const MethodInfo,
            );
            rvo_generate_opponent_vos_hook(
                0x701_usize as *mut Object,
                0x702_usize as *mut Object,
                0x703_usize as *mut Object,
                0x704_usize as *const MethodInfo,
            );
            added
        };
        assert_eq!(added as usize, 0x204);
        assert_eq!(
            TEST_HOOK_CALLS
                .iter()
                .map(|count| count.load(Ordering::Acquire))
                .collect::<Vec<_>>(),
            [1; RVO_HOOK_COUNT]
        );
        assert_eq!(
            TEST_HOOK_ARGUMENTS
                .iter()
                .map(|arguments| {
                    std::array::from_fn(|index| arguments[index].load(Ordering::Acquire))
                })
                .collect::<Vec<_>>(),
            [
                [0x101, 0x102, 0, 0],
                [0x201, 0x202, 0x203, 0],
                [0x301, 0x302, 0, 0],
                [0x401, 0x402, 0, 0],
                [0x501, 0x502, 0, 0],
                [0x601, 0x602, 0x603, 0],
                [0x701, 0x702, 0x703, 0x704],
            ]
        );
        for slot in [
            &ORIGINAL_RVO_CONTROLLER_ACTIVE,
            &ORIGINAL_RVO_ADD_AGENT_FIXED,
            &ORIGINAL_RVO_FIXED_UPDATE,
            &ORIGINAL_RVO_PRE_CALCULATION,
            &ORIGINAL_RVO_CALCULATE_NEIGHBOURS,
            &ORIGINAL_RVO_GENERATE_NEIGHBOUR_VOS,
            &ORIGINAL_RVO_GENERATE_OPPONENT_VOS,
        ] {
            slot.store(ptr::null_mut(), Ordering::Release);
        }

        for profile in [None, Some(CaptureInstrumentationProfile::TargetRefsV1)] {
            let mut state = CaptureState {
                armed: true,
                instrumentation_profile: profile,
                ..CaptureState::default()
            };
            record_rvo_agent_owner_mapping(&mut state, 100, 10);
            assert_eq!(state.rvo_agent_owners.get(&100), Some(&10));
            assert!(state.armed);
            record_rvo_agent_owner_mapping(&mut state, 100, 10);
            assert!(state.armed);
            record_rvo_agent_owner_mapping(&mut state, 100, 11);
            assert_eq!(state.rvo_agent_owners.get(&100), Some(&11));
            assert!(!state.armed);
            assert!(matches!(
                state.queue.back(),
                Some(CaptureMessage::Failure(reason))
                    if reason == "native RVO agent was assigned to two FightActors"
            ));
        }
        let mut unarmed = CaptureState::default();
        record_rvo_agent_owner_mapping(&mut unarmed, 200, 20);
        assert_eq!(unarmed.rvo_agent_owners.get(&200), Some(&20));
        assert!(unarmed.queue.is_empty());
    }

    #[test]
    fn capture_session_reset_allows_reused_rvo_agent_pointer() {
        let mut capture = CaptureState::default();
        capture
            .rvo_agent_refs
            .insert(11, ObjectRef::new(ObjectKind::Unit, 1));
        capture.rvo_agent_owners.insert(11, 22);
        capture.rvo_internal_agent_ids.insert(44, 7);
        capture.next_rvo_internal_agent_id = 8;
        capture
            .rvo_agent_sets
            .insert(1, vec![NativeRvoAgentState::default()]);
        capture.last_damage_sources.insert(
            ObjectRef::new(ObjectKind::Unit, 1),
            DamageAttribution {
                source: Some(ObjectRef::new(ObjectKind::Unit, 2)),
                source_team_id: Some(1),
            },
        );
        capture.rvo_neighbour_sets.push(NativeRvoNeighbourSet {
            update_ordinal: 1,
            source_call_ordinal: 1,
            source: 11,
            neighbours: Vec::new(),
        });
        capture.rvo_vo_buffers.push(NativeRvoVoBuffer {
            update_ordinal: 1,
            call_ordinal: 1,
            source: 11,
            vos: Vec::new(),
        });
        capture.opponent_vos.push(NativeOpponentVo {
            update_ordinal: 1,
            call_ordinal: 2,
            source: 11,
            target: 33,
            vo_buffer_length_before: 0,
            vo_buffer_length_after: 1,
            appended_colliding: false,
        });
        seed_rvo_update(&mut capture, 1, 1, 2, true, 3, true);

        capture.reset_session();

        assert!(capture.rvo_agent_refs.is_empty());
        assert!(capture.rvo_agent_owners.is_empty());
        assert!(capture.last_damage_sources.is_empty());
        assert!(capture.rvo_internal_agent_ids.is_empty());
        assert_eq!(capture.next_rvo_internal_agent_id, 0);
        assert!(capture.rvo_agent_sets.is_empty());
        assert!(capture.rvo_neighbour_sets.is_empty());
        assert!(capture.rvo_vo_buffers.is_empty());
        assert!(capture.opponent_vos.is_empty());
        assert!(capture.rvo_update_modes.is_empty());
        assert!(capture.rvo_update_symmetry_breaking_biases.is_empty());
        assert!(capture.rvo_update_start_native_ticks.is_empty());
        assert!(capture.rvo_update_publish_native_ticks.is_empty());
        assert!(capture.rvo_update_multithreaded.is_empty());
        assert_eq!(capture.rvo_agent_owners.insert(11, 33), None);
    }

    #[test]
    fn same_tick_native_projectile_events_are_preserved_in_order() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("same-tick.mcfr");
        let state = WorldSnapshot {
            live_units: vec![unit(1, 0, 1), unit(2, 1, 2)],
            ..WorldSnapshot::default()
        };
        let mut capture = CaptureState::default();
        capture.unit_ids.insert(11, 1);
        capture.unit_ids.insert(22, 2);
        let traces = vec![
            NativeTrace::ProjectileReleased {
                projectile_id: 1,
                owner: 11,
                target: 22,
                skill_slot: Some(1),
                weapon_index: Some(4),
            },
            NativeTrace::Damage {
                source: Some(ObjectRef::new(ObjectKind::Projectile, 1)),
                source_team_id: Some(0),
                target: ObjectRef::new(ObjectKind::Unit, 2),
                amount: 10,
            },
            NativeTrace::ProjectileRemoved {
                projectile_id: 1,
                owner: 11,
                target: 22,
                position: QVec3 { x: 1, y: 2, z: 0 },
                intercepted: false,
                absorbed_by: None,
            },
        ];
        let events = transition_events(&traces, &capture);
        let context = DurableContext {
            logic_step: Rational {
                numerator: 1,
                denominator: 20,
            },
            time_units_per_second: 2_000,
            combat_round: 1,
            match_seed: 0,
        };
        let layout = "seed: 0\nround: 1\nsides:\n  blue:\n    formations:\n    - type: marksman\n      index: 0\n      x: 0\n      y: -50\n  red:\n    formations:\n    - type: arclight\n      index: 0\n      x: 0\n      y: -50\n";
        let mut writer =
            mechcore_mcfr::McfrWriter::create(&path, "test", &context, layout).unwrap();
        writer.append_tick(state, &events).unwrap();
        writer.finish().unwrap();
        let reader = mechcore_mcfr::McfrReader::open(path).unwrap();
        let events = reader.events(1).unwrap();
        assert_eq!(events.events.len(), 3);
        assert!(matches!(
            events.events[0].payload,
            EventPayload::ProjectileReleased {
                skill_slot: Some(1),
                weapon_index: Some(4)
            }
        ));
        assert_eq!(
            events.events[1].source,
            Some(ObjectRef::new(ObjectKind::Projectile, 1))
        );
        assert!(matches!(
            events.events[1].payload,
            EventPayload::Damage { amount: 10 }
        ));
        assert!(matches!(
            events.events[2].payload,
            EventPayload::ProjectileRemoved { .. }
        ));
    }

    #[test]
    fn unit_death_preserves_lethal_damage_source() {
        let source = ObjectRef::new(ObjectKind::Unit, 1);
        let mut capture = CaptureState::default();
        capture.object_teams.insert(source, 0);
        let traces = [NativeTrace::UnitDied {
            unit_id: 2,
            position: QVec3 { x: 1, y: 2, z: 3 },
            source: Some(source),
            source_team_id: Some(0),
        }];

        let events = transition_events(&traces, &capture);

        assert_eq!(events.events.len(), 1);
        assert_eq!(events.events[0].source, Some(source));
        assert_eq!(events.events[0].source_team_id, Some(0));
        assert!(matches!(
            events.events[0].payload,
            EventPayload::UnitDied { .. }
        ));
    }
}
