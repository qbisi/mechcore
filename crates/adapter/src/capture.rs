use crate::{
    il2cpp::{Api, Class, FieldInfo, MethodInfo, Object, argument, object_argument},
    runtime::Runtime,
};
use jpeg_encoder::{ColorType, Encoder};
use mechcore_mcfr::{
    BuildingState, Domain, DurableContext, Event, EventPayload, Gauge, IdentityContract,
    MCFR_SCHEMA_VERSION, MotionState, NumericConvention, ObjectKind, ObjectRef,
    PersonalShieldState, Pose, ProjectileState, Rational, StatusState, TransitionEvents, UnitState,
    Vec3, Visibility, WorldSnapshot,
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
const Q32_ONE: i128 = 1_i128 << 32;
const DISTANCE_UNITS_PER_METER: u64 = 1_000;
const ROTATION_UNITS_PER_DEGREE: u64 = 1_000;
const TIME_UNITS_PER_SECOND: u64 = 2_000;
const CAPTURE_WIDTH: u16 = 2_560;
const CAPTURE_HEIGHT: u16 = 1_600;
const CAPTURE_FRAME_RATE: i32 = 20;
pub(crate) const CALIBRATION_VIEW: &str = "calibration_topdown";
pub(crate) const CALIBRATION_CAMERA_HEIGHT: f32 = 1_070.0;
pub(crate) const CALIBRATION_CAMERA_Z: f32 = -1_070.0;
pub(crate) const CALIBRATION_CAMERA_PITCH_DEGREES: f32 = 45.0;
pub(crate) const CALIBRATION_FIELD_OF_VIEW_DEGREES: f32 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CaptureInstrumentationProfile {
    TargetRefsV1,
    TargetRefsRvoV1,
    SkillAttackableCheckerV1,
    SelectorScoreV1,
}

impl CaptureInstrumentationProfile {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::TargetRefsV1 => "target_refs_v1",
            Self::TargetRefsRvoV1 => "target_refs_rvo_v1",
            Self::SkillAttackableCheckerV1 => "skill_attackable_checker_v1",
            Self::SelectorScoreV1 => "selector_score_v1",
        }
    }

    pub(crate) const fn channel(self) -> &'static str {
        match self {
            Self::TargetRefsV1 => "target_refs",
            Self::TargetRefsRvoV1 => "target_refs_rvo",
            Self::SkillAttackableCheckerV1 => "skill_attackable_checker",
            Self::SelectorScoreV1 => "selector_score",
        }
    }

    const fn includes_target_refs(self) -> bool {
        matches!(self, Self::TargetRefsV1 | Self::TargetRefsRvoV1)
    }

    const fn includes_rvo(self) -> bool {
        matches!(self, Self::TargetRefsRvoV1)
    }

    const fn includes_skill_attackable_checker(self) -> bool {
        matches!(self, Self::SkillAttackableCheckerV1)
    }

    const fn includes_selector_score(self) -> bool {
        matches!(self, Self::SelectorScoreV1)
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
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub(crate) struct SelectorScoreObservation {
    pub(crate) score_calculations: Vec<SelectorScoreCalculation>,
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
    pub(crate) symmetry_breaking_bias_raw: i64,
    pub(crate) agents: Vec<RvoAgentObservation>,
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
#[derive(Clone, Copy)]
struct UnityRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[derive(Clone)]
pub(crate) enum CaptureMessage {
    Initial {
        context: DurableContext,
        state: WorldSnapshot,
        instrumentation: Option<CaptureInstrumentationObservation>,
        frame: Option<Vec<u8>>,
    },
    Transition {
        events: TransitionEvents,
        state: WorldSnapshot,
        instrumentation: Option<CaptureInstrumentationObservation>,
        terminal: bool,
        frame: Option<Vec<u8>>,
    },
    Failure(String),
}

enum PendingVisualMessage {
    Initial {
        context: DurableContext,
        state: WorldSnapshot,
        instrumentation: Option<CaptureInstrumentationObservation>,
    },
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
    projectile_controllers: usize,
    buff_list: usize,
    buff_duration_time: usize,
    buff_max_duration_time: usize,
    buff_step_time: usize,
    buff_step_time_config: usize,
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
    armed: bool,
    initialized: bool,
    queue: VecDeque<CaptureMessage>,
    unit_ids: BTreeMap<usize, u64>,
    building_ids: BTreeMap<usize, u64>,
    projectile_ids: BTreeMap<usize, u64>,
    status_ids: BTreeMap<usize, u64>,
    formation_ids: BTreeMap<usize, u64>,
    rvo_agent_refs: BTreeMap<usize, ObjectRef>,
    rvo_agent_owners: BTreeMap<usize, usize>,
    rvo_internal_agent_ids: BTreeMap<usize, u64>,
    next_unit_id: u64,
    next_building_id: u64,
    next_projectile_id: u64,
    next_status_id: u64,
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
        self.instrumentation_profile = None;
        self.queue.clear();
        self.unit_ids.clear();
        self.building_ids.clear();
        self.projectile_ids.clear();
        self.status_ids.clear();
        self.formation_ids.clear();
        self.rvo_agent_refs.clear();
        self.rvo_agent_owners.clear();
        self.rvo_internal_agent_ids.clear();
        self.next_unit_id = 1;
        self.next_building_id = 1;
        self.next_projectile_id = 1;
        self.next_status_id = 1;
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

    fn frame(&mut self) -> Result<Vec<u8>, String> {
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
        let row_bytes = usize::from(width) * channels;
        let mut rgb = Vec::with_capacity(pixels * 3);
        for row in raw.chunks_exact(row_bytes).rev() {
            for pixel in row.chunks_exact(channels) {
                rgb.extend_from_slice(&pixel[..3]);
            }
        }
        let mut jpeg = Vec::new();
        Encoder::new(&mut jpeg, 90)
            .encode(&rgb, width, height, ColorType::Rgb)
            .map_err(|error| format!("cannot encode captured JPEG: {error}"))?;
        Ok(jpeg)
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
static ORIGINAL_POST_RENDER: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_PROJECTILE_ADD: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_PROJECTILE_DESTROY: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_DAMAGE_PERFORM: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
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
}

enum NativeTrace {
    ProjectileReleased {
        projectile_id: u64,
        owner: usize,
        target: usize,
    },
    ProjectileRemoved {
        projectile_id: u64,
        owner: usize,
        target: usize,
        position: Vec3,
        intercepted: bool,
    },
    Damage {
        source: Option<ObjectRef>,
        target: usize,
        amount: i64,
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
        let camera = api
            .class("UnityEngine.CoreModule.dll", "UnityEngine", "Camera")
            .map_err(|error| error.to_string())?;
        let post_render = api
            .method(camera, "FireOnPostRender", 1)
            .map_err(|error| error.to_string())?;
        let projectile_system = api
            .class("GRFight.dll", "GameRiver.Fight", "ProjectileSystem")
            .map_err(|error| error.to_string())?;
        let projectile_controllers = api
            .field(projectile_system, "projectileControllers")
            .map_err(|error| error.to_string())?;
        let buff_manager = api
            .class("GRFight.dll", "GameRiver.Fight", "BuffManager")
            .map_err(|error| error.to_string())?;
        let buff_list = api
            .field(buff_manager, "buffs")
            .map_err(|error| error.to_string())?;
        let buff = api
            .class("GRFight.dll", "GameRiver.Fight", "Buff")
            .map_err(|error| error.to_string())?;
        let buff_duration_time = api
            .field(buff, "durationTime")
            .map_err(|error| error.to_string())?;
        let buff_max_duration_time = api
            .field(buff, "maxDurationtime")
            .map_err(|error| error.to_string())?;
        let buff_step_time = api
            .field(buff, "stepTime")
            .map_err(|error| error.to_string())?;
        let buff_step_time_config = api
            .field(buff, "stepTimeConfig")
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
        let projectile_add = api
            .method(projectile_system, "AddProjectile", 1)
            .map_err(|error| error.to_string())?;
        let projectile_destroy = api
            .method(projectile_system, "Destroy", 2)
            .map_err(|error| error.to_string())?;
        let damage_perform = api
            .method(damage_performer, "Perform", 3)
            .map_err(|error| error.to_string())?;
        install_projectile_add_hook(api, projectile_add)?;
        install_projectile_destroy_hook(api, projectile_destroy)?;
        install_damage_perform_hook(api, damage_perform)?;
        install_update_hook(api, update)?;
        install_match_update_hook(api, match_update)?;
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
            projectile_controllers: projectile_controllers as usize,
            buff_list: buff_list as usize,
            buff_duration_time: buff_duration_time as usize,
            buff_max_duration_time: buff_max_duration_time as usize,
            buff_step_time: buff_step_time as usize,
            buff_step_time_config: buff_step_time_config as usize,
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
    visual: bool,
    instrumentation_profile: Option<CaptureInstrumentationProfile>,
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
        return Err("recording requires Training Ground deployment before fighting".into());
    }
    let current_match = runtime.current_match();
    if current_match.is_null() {
        return Err("active match disappeared before recording started".into());
    }
    state.reset_session();
    RVO_UPDATE_ORDINAL.store(0, Ordering::Release);
    RVO_SOURCE_CALL_ORDINAL.store(0, Ordering::Release);
    RVO_VO_CALL_ORDINAL.store(0, Ordering::Release);
    ACTIVE_RVO_UPDATE.store(u64::MAX, Ordering::Release);
    CURRENT_RVO_FIXED_UPDATE.store(u64::MAX, Ordering::Release);
    CURRENT_RVO_ACTIVATION_COUNT.store(0, Ordering::Release);
    RVO_ACTIVATION_COUNT.store(0, Ordering::Release);
    state.instrumentation_profile = instrumentation_profile;
    if visual {
        state.visual = Some(VisualCapture::new(runtime)?);
    }
    state.armed = true;
    drop(state);

    if let Err(error) = runtime
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
    if instrumentation_profile.is_some_and(CaptureInstrumentationProfile::includes_rvo)
        && metadata.rvo.is_none()
    {
        Err(format!(
            "target_refs_rvo_v1 is unavailable: {}",
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
    if instrumentation_profile.is_some_and(CaptureInstrumentationProfile::includes_selector_score)
        && !metadata.selector_score_available
    {
        Err(format!(
            "selector_score_v1 is unavailable: {}",
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
type PostRenderFn = unsafe extern "C" fn(*mut Object, *const MethodInfo);
type ProjectileAddFn = unsafe extern "C" fn(*mut Object, *mut Object, *const MethodInfo);
type ProjectileDestroyFn = unsafe extern "C" fn(*mut Object, *mut Object, bool, *const MethodInfo);
type DamagePerformFn = unsafe extern "C" fn(
    *mut Object,
    *mut Object,
    *mut Object,
    *mut Object,
    *const MethodInfo,
) -> i32;
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
        finish_rvo_update(update_ordinal);
    }
}

unsafe extern "C" fn rvo_pre_calculation_hook(simulator: *mut Object, method: *const MethodInfo) {
    let original = ORIGINAL_RVO_PRE_CALCULATION.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    // SAFETY: the installer stores the trampoline for this exact method ABI.
    let original: RvoPreCalculationFn = unsafe { std::mem::transmute(original) };
    activate_rvo_update();
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
    };
    let update_ordinal = active_rvo_update();
    let before = update_ordinal.and_then(|_| {
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

fn finish_rvo_update(update_ordinal: u64) {
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

fn activate_rvo_update() {
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
        let Some(metadata) = state.metadata.rvo else {
            state.fail("RVO metadata disappeared during instrumentation".into());
            return;
        };
        if !state.rvo_agent_sets.contains_key(&update_ordinal) {
            match read_native_rvo_agent_set(runtime.api, agent, metadata) {
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
    source: *mut Object,
    metadata: RvoMetadata,
) -> Result<Vec<NativeRvoAgentState>, String> {
    const INSTRUMENTATION_AGENT_CAP: i32 = 4_096;
    let simulator: *mut Object = api
        .field_value(source, metadata.agent_simulator as *mut FieldInfo)
        .map_err(|error| error.to_string())?;
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
                if !fighting {
                    state.traces.clear();
                    return Ok(());
                }
                if !state.traces.is_empty() {
                    return Err("combat events occurred before the fighting-entry snapshot".into());
                }
                let initial = snapshot(runtime, &mut state, true)?;
                let context = durable_context(runtime)?;
                state.initialized = true;
                if let Some(visual) = state.visual.as_ref() {
                    visual.apply_calibration()?;
                    state.render_completed = false;
                    state.pending_visual = Some(PendingVisualMessage::Initial {
                        context,
                        state: initial.world,
                        instrumentation: initial.instrumentation,
                    });
                } else {
                    state.push(CaptureMessage::Initial {
                        context,
                        state: initial.world,
                        instrumentation: initial.instrumentation,
                        frame: None,
                    })?;
                }
                return Ok(());
            }
            let next = snapshot(runtime, &mut state, false)?;
            let traces = std::mem::take(&mut state.traces);
            let events = transition_events(&traces, &state);
            let terminal = !fighting;
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
        PendingVisualMessage::Initial {
            context,
            state: world,
            instrumentation,
        } => state.push(CaptureMessage::Initial {
            context,
            state: world,
            instrumentation,
            frame: Some(frame),
        }),
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
    // SAFETY: IL2CPP arguments are forwarded unchanged.
    let result = unsafe { original(performer, provider, target, advanced_shield, method) };
    let _ = catch_unwind(AssertUnwindSafe(|| record_damage(provider, target, result)));
    result
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
        state.traces.push(NativeTrace::ProjectileReleased {
            projectile_id: id,
            owner,
            target,
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
        )?)?;
        state.traces.push(NativeTrace::ProjectileRemoved {
            projectile_id: id,
            owner,
            target,
            position,
            intercepted,
        });
        state.projectile_ids.remove(&pointer);
        Ok::<(), String>(())
    })();
    if let Err(error) = result {
        state.fail(format!("projectile removal trace failed: {error}"));
    }
}

fn record_damage(provider: *mut Object, target: *mut Object, result: i32) {
    let mut state = capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !state.armed || !state.in_update || result <= 0 {
        return;
    }
    let source = object_ref_from_pointer(provider as usize, &state);
    state.traces.push(NativeTrace::Damage {
        source,
        target: target as usize,
        amount: i64::from(result),
    });
}

fn durable_context(runtime: &Runtime) -> Result<DurableContext, String> {
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
    Ok(DurableContext {
        schema_version: MCFR_SCHEMA_VERSION,
        game_build: version,
        logic_step: Rational {
            numerator: 1,
            denominator: 20,
        },
        numeric_convention: NumericConvention {
            distance_units_per_meter: DISTANCE_UNITS_PER_METER,
            rotation_units_per_degree: ROTATION_UNITS_PER_DEGREE,
            time_units_per_second: TIME_UNITS_PER_SECOND,
        },
        combat_round: u32::try_from(round).map_err(|_| "combat round overflow".to_owned())?,
        match_seed,
        identity_contract: IdentityContract::TeamZxSequentialV1,
    })
}

struct RawUnit {
    pointer: usize,
    formation: usize,
    rvo_agent: Option<usize>,
    mech_lock_target: usize,
    state: UnitState,
    statuses: Vec<RawStatus>,
    target_refs: Option<RawTargetRefs>,
}

struct RawTargetRefs {
    mech_lock_target: usize,
    normal_skill_fields_available: bool,
    skill_lock_target: usize,
    skill_attack_target: usize,
}

struct RawStatus {
    pointer: usize,
    source: usize,
    target: usize,
    status_type_id: u32,
    additive_stack: i32,
    duration_time: i32,
    max_duration_time: i32,
    step_time: i32,
    step_time_config: i32,
    finished: bool,
    frozen: bool,
}

struct RawBuilding {
    pointer: usize,
    rvo_agent: Option<usize>,
    native_index: i32,
    state: BuildingState,
}

struct CapturedSnapshot {
    world: WorldSnapshot,
    instrumentation: Option<CaptureInstrumentationObservation>,
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
    let mut raw_units = Vec::new();
    let mut raw_buildings = Vec::new();
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
            raw_units.push(read_unit(
                runtime.api,
                unit,
                team_id,
                &capture.metadata,
                capture.instrumentation_profile,
            )?);
        }
        let buildings = runtime
            .api
            .invoke(team, "GetTowers", &mut [])
            .map_err(|error| error.to_string())?;
        for index in 0..list_count(runtime.api, buildings, 64)? {
            let building = list_item(runtime.api, buildings, index)?;
            raw_buildings.push(read_building(
                runtime.api,
                building,
                team_id,
                &capture.metadata,
                capture.instrumentation_profile,
            )?);
        }
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
    let mut raw_statuses = Vec::new();
    let mut raw_mech_lock_targets = Vec::with_capacity(raw_units.len());
    let mut raw_target_refs = Vec::new();
    for mut unit in raw_units {
        let unit_id = match capture.unit_ids.get(&unit.pointer) {
            Some(id) => *id,
            None => allocate(&mut capture.next_unit_id, "unit")?,
        };
        capture.unit_ids.entry(unit.pointer).or_insert(unit_id);
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
        unit.state.formation_id = formation_id;
        raw_mech_lock_targets.push((units.len(), unit.mech_lock_target));
        if let Some(target_refs) = unit.target_refs {
            raw_target_refs.push((unit_id, target_refs));
        }
        raw_statuses.append(&mut unit.statuses);
        units.push(unit.state);
    }

    raw_buildings.sort_by_key(|building| (building.state.team_id, building.native_index));
    let mut buildings = Vec::with_capacity(raw_buildings.len());
    for mut building in raw_buildings {
        let id = match capture.building_ids.get(&building.pointer) {
            Some(id) => *id,
            None => allocate(&mut capture.next_building_id, "building")?,
        };
        capture.building_ids.entry(building.pointer).or_insert(id);
        building.state.building_id = id;
        if let Some(agent) = building.rvo_agent {
            capture
                .rvo_agent_refs
                .insert(agent, ObjectRef::new(ObjectKind::Building, id));
        }
        buildings.push(building.state);
    }
    for (unit_index, target_pointer) in raw_mech_lock_targets {
        units[unit_index].mech_lock_target =
            resolve_target_ref(target_pointer, "FightMech.lockTarget", capture)?;
    }
    let instrumentation = match capture.instrumentation_profile {
        Some(profile) if profile.includes_target_refs() => {
            let mut observations = Vec::with_capacity(raw_target_refs.len());
            for (unit_id, refs) in raw_target_refs {
                observations.push(UnitTargetRefsObservation {
                    unit: ObjectRef::new(ObjectKind::Unit, unit_id),
                    mech_lock_target: resolve_target_ref(
                        refs.mech_lock_target,
                        "FightMech.lockTarget",
                        capture,
                    )?,
                    normal_skill_fields_available: refs.normal_skill_fields_available,
                    skill_lock_target: resolve_target_ref(
                        refs.skill_lock_target,
                        "FightSkill.lockTarget",
                        capture,
                    )?,
                    skill_attack_target: resolve_target_ref(
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
                Some(CaptureInstrumentationObservation::TargetRefsRvo(
                    resolve_rvo_observation(target_refs, native_tick, capture)?,
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
        None => None,
        Some(_) => unreachable!("all capture instrumentation profiles are handled"),
    };
    let projectiles = read_projectiles(runtime, capture)?;
    let mut seen_statuses = BTreeSet::new();
    let mut statuses = Vec::with_capacity(raw_statuses.len());
    for status in raw_statuses {
        seen_statuses.insert(status.pointer);
        let status_id = match capture.status_ids.get(&status.pointer) {
            Some(id) => *id,
            None => allocate(&mut capture.next_status_id, "status")?,
        };
        capture
            .status_ids
            .entry(status.pointer)
            .or_insert(status_id);
        let Some(target) = object_ref_from_pointer(status.target, capture) else {
            continue;
        };
        statuses.push(StatusState {
            status_id,
            status_type_id: status.status_type_id,
            source: object_ref_from_pointer(status.source, capture),
            target,
            additive_stack: status.additive_stack,
            duration_time: status.duration_time,
            max_duration_time: status.max_duration_time,
            step_time: status.step_time,
            step_time_config: status.step_time_config,
            finished: status.finished,
            frozen: status.frozen,
        });
    }
    capture
        .status_ids
        .retain(|pointer, _| seen_statuses.contains(pointer));
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
        world: WorldSnapshot {
            units,
            projectiles,
            buildings,
            statuses,
        },
        instrumentation,
    })
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
    let transform = invoke_object(api, unit, "GetFightTransform")?;
    let fixed_position = invoke_value::<FixedVec3>(api, transform, "GetPositionInt3D")?;
    let fixed_rotation = invoke_value::<FixedPoint>(api, transform, "GetRotationInt")?;
    let motion = invoke_object(api, unit, "GetMotionController")?;
    let velocity = invoke_value::<FixedVec3>(api, motion, "GetCurrentVelocity")?;
    let alive = invoke_value::<bool>(api, unit, "IsAlive")?;
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
    let position = vec3(fixed_position)?;
    let body_rotation = q32_to_units(fixed_rotation.raw, ROTATION_UNITS_PER_DEGREE)?;
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
    let aim_transform = invoke_object(api, main_skill, "GetMainTransform")?;
    let aim_position = vec3(invoke_value::<FixedVec3>(
        api,
        aim_transform,
        "GetPositionInt3D",
    )?)?;
    let aim_rotation = q32_to_units(
        invoke_value::<FixedPoint>(api, aim_transform, "GetRotationInt")?.raw,
        ROTATION_UNITS_PER_DEGREE,
    )?;
    let aim_pose = Pose {
        position: aim_position,
        rotation: aim_rotation,
    };
    let shield = invoke_object(api, unit, "GetEnergyShieldController")?;
    let max_energy = invoke_value::<i32>(api, shield, "GetMaxEnergy")?;
    let personal_shield = PersonalShieldState {
        active: invoke_value::<bool>(api, shield, "IsActive")?,
        enabled: invoke_value::<bool>(api, shield, "IsEnable")?,
        energy: i64::from(invoke_value::<i32>(api, shield, "GetEnergy")?),
        max_energy: i64::from(max_energy),
    };
    let buff_manager = invoke_object(api, unit, "GetBuffManager")?;
    let buffs: *mut Object = api
        .field_value(buff_manager, metadata.buff_list as *mut FieldInfo)
        .map_err(|error| error.to_string())?;
    let mut statuses = Vec::new();
    for index in 0..list_count(api, buffs, 1_024)? {
        let buff = list_item(api, buffs, index)?;
        let status_type = invoke_value::<i32>(api, buff, "GetBuffID")?;
        let source = api
            .invoke(buff, "GetSource", &mut [])
            .map_err(|error| error.to_string())?;
        statuses.push(RawStatus {
            pointer: buff as usize,
            source: source as usize,
            target: unit as usize,
            status_type_id: u32::try_from(status_type)
                .map_err(|_| format!("invalid buff type {status_type}"))?,
            additive_stack: invoke_value::<i32>(api, buff, "GetAdditiveStack")?,
            duration_time: api
                .field_value(buff, metadata.buff_duration_time as *mut FieldInfo)
                .map_err(|error| error.to_string())?,
            max_duration_time: api
                .field_value(buff, metadata.buff_max_duration_time as *mut FieldInfo)
                .map_err(|error| error.to_string())?,
            step_time: api
                .field_value(buff, metadata.buff_step_time as *mut FieldInfo)
                .map_err(|error| error.to_string())?,
            step_time_config: api
                .field_value(buff, metadata.buff_step_time_config as *mut FieldInfo)
                .map_err(|error| error.to_string())?,
            finished: invoke_value::<bool>(api, buff, "IsFinish")?,
            frozen: invoke_value::<bool>(api, buff, "IsFreeze")?,
        });
    }
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
        state: UnitState {
            unit_id: 0,
            team_id,
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
            aim_pose,
            velocity: vec3(velocity)?,
            motion_state,
            mech_lock_target: None,
            collision_radius: q32_to_units(
                invoke_value::<FixedPoint>(api, unit, "GetRadius")?.raw,
                DISTANCE_UNITS_PER_METER,
            )?,
            life: i64::from(life),
            max_life: i64::from(max_life),
            alive,
            active,
            targetable,
            visibility,
            personal_shield,
        },
        statuses,
        target_refs,
    })
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
            ready
                .contains(&update_ordinal)
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
                    symmetry_breaking_bias_raw,
                    agents: Vec::new(),
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
        for (ordinal, agent) in agents.into_iter().enumerate() {
            resolved.push(RvoAgentObservation {
                ordinal: u32::try_from(ordinal)
                    .map_err(|_| "RVO agent ordinal overflow".to_owned())?,
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
            });
        }
        updates
            .get_mut(&update_ordinal)
            .ok_or_else(|| format!("unknown native RVO update {update_ordinal}"))?
            .agents = resolved;
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
    let observation = TargetRefsRvoObservation {
        target_refs,
        rvo_updates: updates.into_values().collect(),
    };
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
    Ok(observation)
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
    let transform = invoke_object(api, building, "GetFightTransform")?;
    let position = vec3(invoke_value::<FixedVec3>(
        api,
        transform,
        "GetPositionInt3D",
    )?)?;
    let rotation = q32_to_units(
        invoke_value::<FixedPoint>(api, transform, "GetRotationInt")?.raw,
        ROTATION_UNITS_PER_DEGREE,
    )?;
    let bounds = invoke_value::<FixedRect>(api, building, "GetBoundsRect")?;
    let native_index = invoke_value::<i32>(api, building, "GetBuildingIndex")?;
    let building_type = invoke_value::<i32>(api, building, "GetBuildingType")?;
    let life = invoke_value::<i32>(api, building, "GetLife")?;
    let max_life = invoke_value::<i32>(api, building, "GetMaxLife")?;
    let alive = invoke_value::<bool>(api, building, "IsAlive")?;
    let destroyed = invoke_value::<bool>(api, building, "IsDestroyed")?;
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
        native_index,
        state: BuildingState {
            building_id: 0,
            team_id,
            building_type_id: u32::try_from(building_type)
                .map_err(|_| format!("invalid building type {building_type}"))?,
            position,
            rotation,
            bounds_width: q32_to_units(bounds.size.x.raw, DISTANCE_UNITS_PER_METER)?,
            bounds_height: q32_to_units(bounds.size.y.raw, DISTANCE_UNITS_PER_METER)?,
            life: i64::from(life),
            max_life: i64::from(max_life),
            alive,
            destroyed,
            available,
            targetable,
            collision_enabled,
        },
    })
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
    let mut system = ptr::null_mut();
    for index in 0..list_count(runtime.api, modules, 128)? {
        let candidate = list_item(runtime.api, modules, index)?;
        if runtime
            .api
            .object_class(candidate)
            .map(|class| class as usize)
            == Some(capture.metadata.projectile_system_class)
        {
            system = candidate;
            break;
        }
    }
    if system.is_null() {
        return Err("ProjectileSystem module is unavailable".into());
    }
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
        projectiles.push(read_projectile(
            runtime.api,
            controller,
            projectile,
            id,
            capture,
        )?);
    }
    capture
        .projectile_ids
        .retain(|pointer, _| seen.contains(pointer));
    Ok(projectiles)
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
    let transform = invoke_object(api, projectile, "GetFightTransform")?;
    let position = vec3(invoke_value::<FixedVec3>(
        api,
        transform,
        "GetPositionInt3D",
    )?)?;
    let orientation = q32_to_units(
        invoke_value::<FixedPoint>(api, transform, "GetRotationInt")?.raw,
        ROTATION_UNITS_PER_DEGREE,
    )?;
    let target_info = invoke_object(api, projectile, "GetTargetInfo")?;
    let cached_target_position = vec3(invoke_value::<FixedVec3>(api, target_info, "GetPosition")?)?;
    let cached_target_radius = q32_to_units(
        invoke_value::<FixedPoint>(api, target_info, "GetRadius")?.raw,
        DISTANCE_UNITS_PER_METER,
    )?;
    let life = invoke_value::<i32>(api, projectile, "GetLife")?;
    let max_life = invoke_value::<i32>(api, projectile, "GetMaxLife")?;
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
        life: Gauge {
            current: i64::from(life),
            maximum: i64::from(max_life),
        },
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
            } => {
                events.push(event(
                    Some(ObjectRef::new(ObjectKind::Projectile, projectile_id)),
                    object_ref_from_pointer(owner, capture),
                    object_ref_from_pointer(target, capture),
                    EventPayload::ProjectileReleased,
                ));
            }
            NativeTrace::ProjectileRemoved {
                projectile_id,
                owner,
                target,
                position,
                intercepted,
            } => {
                let subject = Some(ObjectRef::new(ObjectKind::Projectile, projectile_id));
                events.push(event(
                    subject,
                    object_ref_from_pointer(owner, capture),
                    object_ref_from_pointer(target, capture),
                    EventPayload::ProjectileRemoved {
                        position,
                        intercepted,
                    },
                ));
            }
            NativeTrace::Damage {
                source,
                target,
                amount,
            } => {
                let Some(target) = object_ref_from_pointer(target, capture) else {
                    continue;
                };
                events.push(event(
                    None,
                    source,
                    Some(target),
                    EventPayload::Damage { amount },
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
}

fn resolve_target_ref(
    pointer: usize,
    field: &str,
    capture: &CaptureState,
) -> Result<Option<ObjectRef>, String> {
    if pointer == 0 {
        return Ok(None);
    }
    object_ref_from_pointer(pointer, capture)
        .map(Some)
        .ok_or_else(|| format!("{field} references an actor absent from the MCFR world snapshot"))
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
                .units
                .iter()
                .find(|unit| unit.unit_id == target.target_ref.id)
                .ok_or_else(|| "checker unit is absent from paired MCFR row".to_owned())?;
            CheckerQualifyingStatus::Unit {
                alive: unit.alive,
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
                alive: building.alive,
                active_or_available: building.available,
                targetable: building.targetable,
                destroyed: building.destroyed,
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
    target: Option<ObjectRef>,
    payload: EventPayload,
) -> Event {
    Event {
        subject,
        source,
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

fn vec3(value: FixedVec3) -> Result<Vec3, String> {
    Ok(Vec3 {
        x: q32_to_units(value.x.raw, DISTANCE_UNITS_PER_METER)?,
        y: q32_to_units(value.y.raw, DISTANCE_UNITS_PER_METER)?,
        z: q32_to_units(value.z.raw, DISTANCE_UNITS_PER_METER)?,
    })
}

fn q32_to_units(raw: i64, scale: u64) -> Result<i64, String> {
    let scaled = i128::from(raw)
        .checked_mul(i128::from(scale))
        .ok_or_else(|| "fixed-point conversion overflow".to_owned())?;
    let rounded = if scaled >= 0 {
        (scaled + Q32_ONE / 2) / Q32_ONE
    } else {
        (scaled - Q32_ONE / 2) / Q32_ONE
    };
    i64::try_from(rounded).map_err(|_| "fixed-point conversion exceeds i64".into())
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

    fn unit(id: u64, team: u32, formation: u64) -> UnitState {
        UnitState {
            unit_id: id,
            team_id: team,
            formation_id: formation,
            unit_type_id: 1,
            domain: Domain::Ground,
            position: Vec3 {
                x: i64::from(team) * 1_000,
                y: 0,
                z: 0,
            },
            body_rotation: 0,
            aim_pose: Pose {
                position: Vec3 {
                    x: i64::from(team) * 1_000,
                    y: 0,
                    z: 0,
                },
                rotation: 0,
            },
            velocity: Vec3 { x: 0, y: 0, z: 0 },
            motion_state: MotionState::Idle,
            mech_lock_target: None,
            collision_radius: 100,
            life: 10,
            max_life: 10,
            alive: true,
            active: true,
            targetable: true,
            visibility: Visibility::Normal,
            personal_shield: PersonalShieldState {
                active: false,
                enabled: false,
                energy: 0,
                max_energy: 0,
            },
        }
    }

    fn building(id: u64) -> BuildingState {
        BuildingState {
            building_id: id,
            team_id: 1,
            building_type_id: 1,
            position: Vec3 { x: 0, y: 0, z: 0 },
            rotation: 0,
            bounds_width: 1_000,
            bounds_height: 1_000,
            life: 10,
            max_life: 10,
            alive: true,
            destroyed: false,
            available: true,
            targetable: true,
            collision_enabled: true,
        }
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
        assert_eq!(profile.as_str(), "selector_score_v1");
        assert_eq!(profile.channel(), "selector_score");
        assert!(profile.includes_selector_score());
        assert!(!profile.includes_target_refs());
        assert!(!profile.includes_rvo());
        assert!(!profile.includes_skill_attackable_checker());
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

        let available = Metadata {
            selector_score_available: true,
            ..Metadata::default()
        };
        assert!(validate_selector_score_profile_availability(Some(profile), &available).is_ok());
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
            units: vec![paired],
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
        assert_eq!(reader.scenario_hash(), "11".repeat(32));
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
        assert_eq!(target_profile.as_str(), "target_refs_v1");
        assert_eq!(target_profile.channel(), "target_refs");
        assert!(target_profile.includes_target_refs());
        assert!(!target_profile.includes_rvo());
        assert_eq!(rvo_profile.as_str(), "target_refs_rvo_v1");
        assert_eq!(rvo_profile.channel(), "target_refs_rvo");
        assert!(rvo_profile.includes_target_refs());
        assert!(rvo_profile.includes_rvo());

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

        let directory = tempfile::tempdir().unwrap();
        let scenario_hash = "00".repeat(32);
        for (index, profile, payload, expected) in [
            (0, target_profile, &target_payload, &target_json),
            (1, rvo_profile, &rvo_payload, &rvo_json),
        ] {
            let path = directory.path().join(format!("profile-{index}.h5"));
            let mut writer = mechcore_mcfr::InstrumentationWriter::create(
                &path,
                &scenario_hash,
                profile.as_str(),
                "adapter-offline-test",
            )
            .unwrap();
            writer
                .record_json(index, profile.channel(), payload)
                .unwrap();
            writer.finish().unwrap();
            let reader = mechcore_mcfr::InstrumentationReader::open(path).unwrap();
            assert_eq!(reader.scenario_hash(), scenario_hash);
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
    fn q32_conversion_rounds_to_mcfr_scale() {
        assert_eq!(q32_to_units(1_i64 << 32, 1_000).unwrap(), 1_000);
        assert_eq!(q32_to_units(-(1_i64 << 32), 1_000).unwrap(), -1_000);
        assert_eq!(q32_to_units(1_i64 << 31, 1_000).unwrap(), 500);
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
            units: vec![unit(1, 0, 1), unit(2, 1, 2)],
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
            },
            NativeTrace::Damage {
                source: Some(ObjectRef::new(ObjectKind::Projectile, 1)),
                target: 22,
                amount: 10,
            },
            NativeTrace::ProjectileRemoved {
                projectile_id: 1,
                owner: 11,
                target: 22,
                position: Vec3 { x: 1, y: 2, z: 0 },
                intercepted: false,
            },
        ];
        let events = transition_events(&traces, &capture);
        let context = DurableContext {
            schema_version: MCFR_SCHEMA_VERSION,
            game_build: "test".into(),
            logic_step: Rational {
                numerator: 1,
                denominator: 20,
            },
            numeric_convention: NumericConvention {
                distance_units_per_meter: 1_000,
                rotation_units_per_degree: 1_000,
                time_units_per_second: 2_000,
            },
            combat_round: 1,
            match_seed: 0,
            identity_contract: IdentityContract::TeamZxSequentialV1,
        };
        let mut writer = mechcore_mcfr::McfrWriter::create(&path, &context).unwrap();
        writer
            .append_tick(
                state.clone(),
                &mechcore_mcfr::TransitionEvents { events: Vec::new() },
            )
            .unwrap();
        writer.append_tick(state, &events).unwrap();
        writer.finish().unwrap();
        let reader = mechcore_mcfr::McfrReader::open(path).unwrap();
        let events = reader.events(1).unwrap();
        assert_eq!(events.events.len(), 3);
        assert!(matches!(
            events.events[0].payload,
            EventPayload::ProjectileReleased
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
}
