//! The RVO instrument channels: each `RVOAgentFixed.CalculateVelocity`, read on
//! either side of the call, with the bias and the two traces it made.
//!
//! A solve only ever sees its own neighbour list, at most `maxNeighbours`
//! agents, and makes at most one VO from each; so a solve is one `rvo_solve`
//! row, at most `maxNeighbours` `rvo_neighbour` rows and as many `rvo_vo` rows,
//! and the channels grow with the number of agents, not with its square.

use crate::capture::{CaptureState, FixedPoint, FixedVec2, capture_state, object_ref_from_pointer};
use crate::il2cpp::{Api, FieldInfo, MethodInfo, Object};
use mechcore_mcfr::{ObjectRef, RvoExit, RvoNeighbour, RvoNeighbourKind, RvoSolve, RvoVec, RvoVo};
use std::{
    cell::RefCell,
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
    sync::atomic::{AtomicBool, AtomicPtr, Ordering},
};

/// A neighbour list longer than this is not the build's.
const NEIGHBOUR_CAP: i32 = 256;

static ARMED: AtomicBool = AtomicBool::new(false);
static ORIGINAL_CALCULATE_VELOCITY: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_BIAS_DESIRED_VELOCITY: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_TRACE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_FIXED_UPDATE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
/// `Simulator.BlockUntilSimulationStepIsDone` and its `MethodInfo`.
static BLOCK_UNTIL_DONE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static BLOCK_UNTIL_DONE_METHOD: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

thread_local! {
    /// The solve running on this thread, which the bias and trace hooks add to.
    static OPEN_SOLVE: RefCell<Option<OpenSolve>> = const { RefCell::new(None) };
}

type CalculateVelocityFn = unsafe extern "C" fn(*mut Object, *mut Object, *const MethodInfo);
type SimulatorFn = unsafe extern "C" fn(*mut Object, *const MethodInfo);
type BiasDesiredVelocityFn = unsafe extern "C" fn(
    *mut Object,
    *mut FixedVec2,
    *mut FixedVec2,
    FixedPoint,
    *const MethodInfo,
) -> bool;
type TraceFn = unsafe extern "C" fn(
    *mut Object,
    *mut Object,
    FixedVec2,
    *mut FixedPoint,
    *const MethodInfo,
) -> FixedVec2;
type VoGradientFn = unsafe extern "C" fn(
    *const NativeVo,
    FixedVec2,
    *mut FixedPoint,
    *const MethodInfo,
) -> FixedVec2;

/// `Agent.VO`, field by field.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct NativeVo {
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

const _: () = assert!(std::mem::size_of::<NativeVo>() == 0xB8);

/// `RVOTeam`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct NativeTeam {
    id: i32,
    radius: FixedPoint,
}

/// The fields a solve reads, resolved once.
#[derive(Clone, Copy)]
pub(crate) struct RvoMetadata {
    fight_actor_rvo_controller: usize,
    rvo_controller_agent: usize,
    radius_inner: usize,
    size: usize,
    radius: usize,
    height: usize,
    desired_speed: usize,
    max_speed: usize,
    locked: usize,
    layer: usize,
    collides_with: usize,
    max_neighbours: usize,
    position: usize,
    elevation: usize,
    current_velocity: usize,
    desired_target: usize,
    manually_controlled: usize,
    priority: usize,
    calculated_speed: usize,
    calculated_target: usize,
    neighbours: usize,
    neighbour_dists: usize,
    sync_group: usize,
    sync_ignore_same_group: usize,
    sync_team: usize,
    sync_desired_velocity: usize,
    context_vos: usize,
    vo_buffer: usize,
    vo_buffer_length: usize,
    vo_gradient: usize,
    vo_gradient_method: usize,
    vo_scaled_gradient: usize,
    vo_scaled_gradient_method: usize,
}

/// What an agent brings to a solve, and what a neighbour's VO depends on.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) struct AgentFields {
    position: FixedVec2,
    elevation: FixedPoint,
    height: FixedPoint,
    current_velocity: FixedVec2,
    desired_velocity: FixedVec2,
    desired_target: FixedVec2,
    desired_speed: FixedPoint,
    max_speed: FixedPoint,
    radius_outer: FixedPoint,
    radius_inner: FixedPoint,
    size: i32,
    priority: FixedPoint,
    layer: i32,
    collides_with: i32,
    group: i32,
    ignore_same_group: bool,
    team: NativeTeam,
    max_neighbours: i32,
    locked: bool,
    manual: bool,
}

struct OpenSolve {
    agent: usize,
    bias: Option<Bias>,
    traces: Vec<(FixedVec2, FixedPoint)>,
}

#[derive(Clone, Copy)]
struct Bias {
    inside: bool,
    velocity: FixedVec2,
    target: FixedVec2,
}

/// One solve as read, before its agents are named.
pub(crate) struct RawSolve {
    agent: usize,
    fields: AgentFields,
    exit: RvoExit,
    bias: Option<Bias>,
    traces: Vec<(FixedVec2, FixedPoint)>,
    output: Option<(FixedVec2, FixedPoint)>,
    neighbours: Vec<RawNeighbour>,
    vos: Vec<NativeVo>,
    /// `VO.Gradient`'s weight at the unbiased desired velocity, per VO.
    penetrations: Vec<i64>,
    /// `VO.ScaledGradient`'s weight at the output velocity, per VO, for an
    /// avoided solve.
    weights: Vec<i64>,
}

struct RawNeighbour {
    agent: usize,
    distance_sq: FixedPoint,
    fields: AgentFields,
}

/// Which channels a recording asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RvoChannels {
    pub(crate) solve: bool,
    pub(crate) neighbour: bool,
    pub(crate) vo: bool,
}

impl RvoChannels {
    pub(crate) const fn any(self) -> bool {
        self.solve || self.neighbour || self.vo
    }
}

/// One tick's rows of each RVO channel asked for.
#[derive(Default)]
pub(crate) struct RvoRows {
    pub(crate) solve: Option<Vec<RvoSolve>>,
    pub(crate) neighbour: Option<Vec<RvoNeighbour>>,
    pub(crate) vo: Option<Vec<RvoVo>>,
}

/// Turns hook reads on for a recording that asked for an RVO channel, and off
/// otherwise; unarmed, the hooks only forward.
pub(crate) fn arm(channels: RvoChannels) {
    ARMED.store(channels.any(), Ordering::Release);
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) fn initialize(api: Api) -> Result<RvoMetadata, String> {
    let error = |error: crate::il2cpp::Error| error.to_string();
    let class =
        |namespace: &str, name: &str| api.class("GRFight.dll", namespace, name).map_err(error);
    let field = |class, name: &str| {
        api.field(class, name)
            .map(|field| field as usize)
            .map_err(error)
    };
    let fight_actor = class("GameRiver.Fight", "FightActor")?;
    let rvo_controller = class("GameRiver.Fight.GRPF.RVO", "RVOController")?;
    let agent = class("GameRiver.Fight.GRPF.RVO.Sampled", "Agent")?;
    let rvo_agent = class("GameRiver.Fight.GRPF.RVO.Sampled", "RVOAgentFixed")?;
    let vo = class("GameRiver.Fight.GRPF.RVO.Sampled", "Agent/VO")?;
    let vo_buffer = class("GameRiver.Fight.GRPF.RVO.Sampled", "Agent/VOBuffer")?;
    let context = class("GameRiver.Fight.GRPF.RVO", "Simulator/WorkerContext")?;
    let simulator = class("GameRiver.Fight.GRPF.RVO", "Simulator")?;
    let fixed_update = api.method(simulator, "FixedUpdate", 0).map_err(error)?;
    let block_until_done = api
        .method(simulator, "BlockUntilSimulationStepIsDone", 0)
        .map_err(error)?;
    BLOCK_UNTIL_DONE.store(
        api.method_pointer(block_until_done).map_err(error)?,
        Ordering::Release,
    );
    BLOCK_UNTIL_DONE_METHOD.store(block_until_done.cast_mut().cast(), Ordering::Release);
    let vo_gradient_method = api.method(vo, "Gradient", 2).map_err(error)?;
    let vo_scaled_gradient_method = api.method(vo, "ScaledGradient", 2).map_err(error)?;
    let metadata = RvoMetadata {
        fight_actor_rvo_controller: field(fight_actor, "rvoController")?,
        rvo_controller_agent: field(rvo_controller, "<rvoAgent>k__BackingField")?,
        radius_inner: field(agent, "radiusInner")?,
        size: field(agent, "<Size>k__BackingField")?,
        radius: field(agent, "radius")?,
        height: field(agent, "height")?,
        desired_speed: field(agent, "desiredSpeed")?,
        max_speed: field(agent, "maxSpeed")?,
        locked: field(agent, "locked")?,
        layer: field(agent, "layer")?,
        collides_with: field(agent, "collidesWith")?,
        max_neighbours: field(agent, "maxNeighbours")?,
        position: field(agent, "position")?,
        elevation: field(agent, "elevationCoordinate")?,
        current_velocity: field(agent, "currentVelocity")?,
        desired_target: field(agent, "desiredTargetPointInVelocitySpace")?,
        manually_controlled: field(agent, "manuallyControlled")?,
        priority: field(agent, "<Priority>k__BackingField")?,
        calculated_speed: field(agent, "calculatedSpeed")?,
        calculated_target: field(agent, "calculatedTargetPoint")?,
        neighbours: field(agent, "neighbours")?,
        neighbour_dists: field(agent, "neighbourDists")?,
        sync_group: field(rvo_agent, "sync_group")?,
        sync_ignore_same_group: field(rvo_agent, "sync_ignoreSameGroup")?,
        sync_team: field(rvo_agent, "sync_team")?,
        sync_desired_velocity: field(rvo_agent, "sync_desiredVelocity")?,
        context_vos: field(context, "vos")?,
        vo_buffer: field(vo_buffer, "buffer")?,
        vo_buffer_length: field(vo_buffer, "length")?,
        vo_gradient: api.method_pointer(vo_gradient_method).map_err(error)? as usize,
        vo_gradient_method: vo_gradient_method as usize,
        vo_scaled_gradient: api
            .method_pointer(vo_scaled_gradient_method)
            .map_err(error)? as usize,
        vo_scaled_gradient_method: vo_scaled_gradient_method as usize,
    };
    let calculate_velocity = api
        .method(rvo_agent, "CalculateVelocity", 1)
        .map_err(error)?;
    let bias_desired_velocity = api.method(agent, "BiasDesiredVelocity", 4).map_err(error)?;
    let trace = api.method(agent, "Trace", 3).map_err(error)?;
    crate::capture::install_inline_hook(
        api,
        calculate_velocity,
        calculate_velocity_hook as *const c_void,
        &ORIGINAL_CALCULATE_VELOCITY,
        "RVOAgentFixed.CalculateVelocity",
    )?;
    crate::capture::install_inline_hook(
        api,
        bias_desired_velocity,
        bias_desired_velocity_hook as *const c_void,
        &ORIGINAL_BIAS_DESIRED_VELOCITY,
        "Agent.BiasDesiredVelocity",
    )?;
    crate::capture::install_inline_hook(
        api,
        fixed_update,
        fixed_update_hook as *const c_void,
        &ORIGINAL_FIXED_UPDATE,
        "Simulator.FixedUpdate",
    )?;
    crate::capture::install_inline_hook(
        api,
        trace,
        trace_hook as *const c_void,
        &ORIGINAL_TRACE,
        "Agent.Trace",
    )?;
    Ok(metadata)
}

/// A multithreaded simulator starts its solves on worker threads and joins
/// them at the next update, four ticks later. An armed recording joins them
/// before the tick that started them ends, with the build's own wait, so every
/// solve belongs to that tick. The wait is on each worker's `ManualResetEvent`,
/// which the next update waits on again harmlessly, and the solves read only
/// the buffers this update synchronised, so the result is the same.
unsafe extern "C" fn fixed_update_hook(simulator: *mut Object, method: *const MethodInfo) {
    let original = ORIGINAL_FIXED_UPDATE.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    // SAFETY: the installer stores the trampoline for this exact IL2CPP method ABI.
    let original: SimulatorFn = unsafe { std::mem::transmute(original) };
    // SAFETY: arguments are forwarded unchanged.
    unsafe { original(simulator, method) };
    let block = BLOCK_UNTIL_DONE.load(Ordering::Acquire);
    if ARMED.load(Ordering::Acquire) && !block.is_null() {
        // SAFETY: the pointer is the method's native entry, taking the
        // simulator and its `MethodInfo`.
        let block: SimulatorFn = unsafe { std::mem::transmute(block) };
        let block_method = BLOCK_UNTIL_DONE_METHOD.load(Ordering::Acquire);
        // SAFETY: see above.
        unsafe { block(simulator, block_method.cast_const().cast()) };
    }
}

unsafe extern "C" fn calculate_velocity_hook(
    agent: *mut Object,
    context: *mut Object,
    method: *const MethodInfo,
) {
    let original = ORIGINAL_CALCULATE_VELOCITY.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    // SAFETY: the installer stores the trampoline for this exact IL2CPP method ABI.
    let original: CalculateVelocityFn = unsafe { std::mem::transmute(original) };
    if !ARMED.load(Ordering::Acquire) {
        // SAFETY: arguments are forwarded unchanged.
        unsafe { original(agent, context, method) };
        return;
    }
    let before = catch_unwind(AssertUnwindSafe(|| read_before(agent)))
        .unwrap_or_else(|_| Err("reading an RVO agent panicked".into()));
    OPEN_SOLVE.with(|open| {
        *open.borrow_mut() = Some(OpenSolve {
            agent: agent as usize,
            bias: None,
            traces: Vec::new(),
        });
    });
    // SAFETY: arguments are forwarded unchanged.
    unsafe { original(agent, context, method) };
    let open = OPEN_SOLVE.with(|open| open.borrow_mut().take());
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let solve = before.and_then(|(metadata, fields, neighbours)| {
            let open = open.ok_or("the RVO solve lost its open record")?;
            read_after(agent, context, &metadata, fields, neighbours, open)
        });
        let mut state = capture_state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.armed || !state.instruments.rvo.any() {
            return;
        }
        if !state.in_update {
            state.fail("an RVO solve ran outside the captured logic tick".into());
            return;
        }
        match solve {
            Ok(solve) => state.rvo_solves.push(solve),
            Err(error) => state.fail(format!("RVO solve: {error}")),
        }
    }));
}

unsafe extern "C" fn bias_desired_velocity_hook(
    vos: *mut Object,
    desired_velocity: *mut FixedVec2,
    target: *mut FixedVec2,
    max_bias_radians: FixedPoint,
    method: *const MethodInfo,
) -> bool {
    let original = ORIGINAL_BIAS_DESIRED_VELOCITY.load(Ordering::Acquire);
    if original.is_null() {
        return false;
    }
    // SAFETY: the installer stores the trampoline for this exact IL2CPP method ABI.
    let original: BiasDesiredVelocityFn = unsafe { std::mem::transmute(original) };
    // SAFETY: arguments are forwarded unchanged.
    let inside = unsafe { original(vos, desired_velocity, target, max_bias_radians, method) };
    if ARMED.load(Ordering::Acquire) && !desired_velocity.is_null() && !target.is_null() {
        // SAFETY: both are the `ref` arguments the method has just written.
        let (velocity, target) = unsafe { (desired_velocity.read(), target.read()) };
        OPEN_SOLVE.with(|open| {
            if let Some(open) = open.borrow_mut().as_mut() {
                open.bias = Some(Bias {
                    inside,
                    velocity,
                    target,
                });
            }
        });
    }
    inside
}

unsafe extern "C" fn trace_hook(
    agent: *mut Object,
    vos: *mut Object,
    point: FixedVec2,
    score: *mut FixedPoint,
    method: *const MethodInfo,
) -> FixedVec2 {
    let original = ORIGINAL_TRACE.load(Ordering::Acquire);
    if original.is_null() {
        return FixedVec2::default();
    }
    // SAFETY: the installer stores the trampoline for this exact IL2CPP method ABI.
    let original: TraceFn = unsafe { std::mem::transmute(original) };
    // SAFETY: arguments are forwarded unchanged.
    let best = unsafe { original(agent, vos, point, score, method) };
    if ARMED.load(Ordering::Acquire) && !score.is_null() {
        // SAFETY: `score` is the `out` argument the method has just written.
        let score = unsafe { score.read() };
        OPEN_SOLVE.with(|open| {
            if let Some(open) = open.borrow_mut().as_mut()
                && open.agent == agent as usize
            {
                open.traces.push((best, score));
            }
        });
    }
    best
}

fn runtime_api() -> Result<Api, String> {
    crate::capture::runtime_api().ok_or_else(|| "the adapter runtime is gone".to_owned())
}

fn metadata() -> Result<RvoMetadata, String> {
    capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .metadata
        .rvo
        .ok_or_else(|| "RVO metadata is unavailable".to_owned())
}

fn value<T: Copy>(api: Api, object: *mut Object, field: usize, name: &str) -> Result<T, String> {
    api.field_value(object, field as *mut FieldInfo)
        .map_err(|error| format!("cannot read {name}: {error}"))
}

fn read_fields(
    api: Api,
    metadata: &RvoMetadata,
    agent: *mut Object,
) -> Result<AgentFields, String> {
    Ok(AgentFields {
        position: value(api, agent, metadata.position, "position")?,
        elevation: value(api, agent, metadata.elevation, "elevationCoordinate")?,
        height: value(api, agent, metadata.height, "height")?,
        current_velocity: value(api, agent, metadata.current_velocity, "currentVelocity")?,
        desired_velocity: value(
            api,
            agent,
            metadata.sync_desired_velocity,
            "sync_desiredVelocity",
        )?,
        desired_target: value(
            api,
            agent,
            metadata.desired_target,
            "desiredTargetPointInVelocitySpace",
        )?,
        desired_speed: value(api, agent, metadata.desired_speed, "desiredSpeed")?,
        max_speed: value(api, agent, metadata.max_speed, "maxSpeed")?,
        radius_outer: value(api, agent, metadata.radius, "radius")?,
        radius_inner: value(api, agent, metadata.radius_inner, "radiusInner")?,
        size: value(api, agent, metadata.size, "Size")?,
        priority: value(api, agent, metadata.priority, "Priority")?,
        layer: value(api, agent, metadata.layer, "layer")?,
        collides_with: value(api, agent, metadata.collides_with, "collidesWith")?,
        group: value(api, agent, metadata.sync_group, "sync_group")?,
        ignore_same_group: value(
            api,
            agent,
            metadata.sync_ignore_same_group,
            "sync_ignoreSameGroup",
        )?,
        team: value(api, agent, metadata.sync_team, "sync_team")?,
        max_neighbours: value(api, agent, metadata.max_neighbours, "maxNeighbours")?,
        locked: value(api, agent, metadata.locked, "locked")?,
        manual: value(
            api,
            agent,
            metadata.manually_controlled,
            "manuallyControlled",
        )?,
    })
}

type Before = (RvoMetadata, AgentFields, Vec<RawNeighbour>);

fn read_before(agent: *mut Object) -> Result<Before, String> {
    let api = runtime_api()?;
    let metadata = metadata()?;
    let fields = read_fields(api, &metadata, agent)?;
    let list: *mut Object = value(api, agent, metadata.neighbours, "neighbours")?;
    let dists: *mut Object = value(api, agent, metadata.neighbour_dists, "neighbourDists")?;
    let count = crate::capture::list_count(api, list, NEIGHBOUR_CAP)?;
    let mut neighbours = Vec::with_capacity(usize::try_from(count).unwrap_or_default());
    for index in 0..count {
        let other = crate::capture::list_item(api, list, index)?;
        let mut at = index;
        let distance_sq = api
            .invoke_value::<FixedPoint>(dists, "get_Item", &mut [crate::il2cpp::argument(&mut at)])
            .map_err(|error| format!("neighbourDists[{index}]: {error}"))?;
        neighbours.push(RawNeighbour {
            agent: other as usize,
            distance_sq,
            fields: read_fields(api, &metadata, other)?,
        });
    }
    Ok((metadata, fields, neighbours))
}

fn read_after(
    agent: *mut Object,
    context: *mut Object,
    metadata: &RvoMetadata,
    fields: AgentFields,
    neighbours: Vec<RawNeighbour>,
    open: OpenSolve,
) -> Result<RawSolve, String> {
    let api = runtime_api()?;
    let exit = if fields.manual {
        RvoExit::Manual
    } else if fields.locked {
        RvoExit::Locked
    } else {
        match open.bias {
            Some(Bias { inside: true, .. }) => RvoExit::Avoided,
            Some(Bias { inside: false, .. }) => RvoExit::Free,
            None => return Err("an unlocked solve made no bias".into()),
        }
    };
    let expected_traces = if exit == RvoExit::Avoided { 2 } else { 0 };
    if open.traces.len() != expected_traces {
        return Err(format!(
            "a {exit:?} solve made {} traces, not {expected_traces}",
            open.traces.len()
        ));
    }
    let output = if exit == RvoExit::Manual {
        None
    } else {
        Some((
            value(
                api,
                agent,
                metadata.calculated_target,
                "calculatedTargetPoint",
            )?,
            value(api, agent, metadata.calculated_speed, "calculatedSpeed")?,
        ))
    };
    // A locked or manual solve returns before clearing the buffer, which
    // still holds the previous agent's VOs.
    let vos = if matches!(exit, RvoExit::Free | RvoExit::Avoided) {
        read_vos(api, metadata, context)?
    } else {
        Vec::new()
    };
    let penetrations = vos
        .iter()
        .map(|vo| vo_weight(metadata, false, vo, fields.desired_velocity))
        .collect();
    let weights = match (exit, output) {
        (RvoExit::Avoided, Some((target, _))) => {
            let velocity = sub(target, fields.position);
            vos.iter()
                .map(|vo| vo_weight(metadata, true, vo, velocity))
                .collect()
        }
        _ => Vec::new(),
    };
    Ok(RawSolve {
        agent: agent as usize,
        fields,
        exit,
        bias: open.bias,
        traces: open.traces,
        output,
        neighbours,
        vos,
        penetrations,
        weights,
    })
}

fn read_vos(
    api: Api,
    metadata: &RvoMetadata,
    context: *mut Object,
) -> Result<Vec<NativeVo>, String> {
    let buffer: *mut Object = value(api, context, metadata.context_vos, "WorkerContext.vos")?;
    let array: *mut Object = value(api, buffer, metadata.vo_buffer, "VOBuffer.buffer")?;
    let length: i32 = value(api, buffer, metadata.vo_buffer_length, "VOBuffer.length")?;
    let length = usize::try_from(length).map_err(|_| format!("VOBuffer.length is {length}"))?;
    let mut vos = api
        .value_array::<NativeVo>(array, usize::try_from(NEIGHBOUR_CAP).unwrap_or_default())
        .map_err(|error| format!("VOBuffer.buffer: {error}"))?;
    if length > vos.len() {
        return Err(format!(
            "VOBuffer.length {length} exceeds its buffer of {}",
            vos.len()
        ));
    }
    vos.truncate(length);
    Ok(vos)
}

/// The build's own `VO.Gradient` or `VO.ScaledGradient` weight at a point.
fn vo_weight(metadata: &RvoMetadata, scaled: bool, vo: &NativeVo, point: FixedVec2) -> i64 {
    let (function, method) = if scaled {
        (
            metadata.vo_scaled_gradient,
            metadata.vo_scaled_gradient_method,
        )
    } else {
        (metadata.vo_gradient, metadata.vo_gradient_method)
    };
    // SAFETY: the pointer is the method's native entry, whose ABI takes the
    // unboxed struct as `this`; the VO is a copy the method only reads.
    let function: VoGradientFn = unsafe { std::mem::transmute(function) };
    let mut weight = FixedPoint::default();
    // SAFETY: see above; `weight` is the `out` argument.
    unsafe { function(vo, point, &raw mut weight, method as *const MethodInfo) };
    weight.raw
}

const fn sub(left: FixedVec2, right: FixedVec2) -> FixedVec2 {
    FixedVec2 {
        x: FixedPoint {
            raw: left.x.raw.wrapping_sub(right.x.raw),
        },
        y: FixedPoint {
            raw: left.y.raw.wrapping_sub(right.y.raw),
        },
    }
}

/// What `GenerateNeighbourAgentVOs` makes of a neighbour, branch by branch as
/// the build takes them.
fn classify(agent: &AgentFields, other: &AgentFields) -> RvoNeighbourKind {
    let top = |fields: &AgentFields| fields.elevation.raw.wrapping_add(fields.height.raw);
    let overlap = fpoint_min(top(agent), top(other))
        .wrapping_sub(fpoint_max(agent.elevation.raw, other.elevation.raw));
    if fpoint_less_than(overlap, 0) {
        return RvoNeighbourKind::OtherElevation;
    }
    if agent.group != other.group {
        return RvoNeighbourKind::Opponent;
    }
    // The neighbour's flag, not the agent's.
    if other.ignore_same_group {
        return RvoNeighbourKind::IgnoredSameGroup;
    }
    if agent.team.id > 0 && agent.team.id == other.team.id {
        return RvoNeighbourKind::TeamRadius;
    }
    RvoNeighbourKind::SameGroup
}

/// `FPoint.op_LessThan`, which calls values within 43 raw of each other
/// equal; `docs/spec/simulation/rvo.md` has the whole rule.
#[allow(
    clippy::cast_sign_loss,
    reason = "the sign reinterpretation is the build's comparison"
)]
const fn fpoint_less_than(left: i64, right: i64) -> bool {
    const SENTINEL: i64 = i64::MIN + 1;
    if left == SENTINEL || right == SENTINEL {
        return false;
    }
    let difference = left.wrapping_sub(right);
    difference < 0 && (difference.wrapping_add(43) as u64) >= 87
}

/// `FPoint.Min` and `FPoint.Max`, which return the second argument on a tie.
const fn fpoint_min(first: i64, second: i64) -> i64 {
    if fpoint_less_than(first, second) {
        first
    } else {
        second
    }
}

const fn fpoint_max(first: i64, second: i64) -> i64 {
    if fpoint_less_than(second, first) {
        first
    } else {
        second
    }
}

const fn makes_vo(kind: RvoNeighbourKind) -> bool {
    !matches!(
        kind,
        RvoNeighbourKind::IgnoredSameGroup | RvoNeighbourKind::OtherElevation
    )
}

const fn vec(value: FixedVec2) -> RvoVec {
    RvoVec {
        x: value.x.raw,
        y: value.y.raw,
    }
}

/// The unit or building behind each RVO agent: the live actor whose
/// `rvoController` holds it. Rebuilt from the live actors whenever a tick has
/// solves, since a dead unit's agent may be freed and its address reused.
fn name_agents(api: Api, metadata: &RvoMetadata, capture: &mut CaptureState) -> Result<(), String> {
    let actors: Vec<(usize, ObjectRef)> = capture
        .unit_ids
        .keys()
        .chain(capture.building_ids.keys())
        .filter_map(|pointer| Some((*pointer, object_ref_from_pointer(*pointer, capture)?)))
        .filter(|(_, object)| !capture.emitted_deaths.contains(object))
        .collect();
    capture.rvo_agent_owners.clear();
    for (actor, object) in actors {
        let controller: *mut Object = value(
            api,
            actor as *mut Object,
            metadata.fight_actor_rvo_controller,
            "FightActor.rvoController",
        )?;
        if controller.is_null() {
            continue;
        }
        let agent: *mut Object = value(
            api,
            controller,
            metadata.rvo_controller_agent,
            "RVOController.rvoAgent",
        )?;
        if !agent.is_null() {
            capture.rvo_agent_owners.insert(agent as usize, object);
        }
    }
    Ok(())
}

/// This tick's rows of each RVO channel asked for, from the solves the hooks
/// read during it.
pub(crate) fn drain(api: Api, capture: &mut CaptureState) -> Result<RvoRows, String> {
    let channels = capture.instruments.rvo;
    if !channels.any() {
        return Ok(RvoRows::default());
    }
    let solves = std::mem::take(&mut capture.rvo_solves);
    let metadata = capture
        .metadata
        .rvo
        .ok_or_else(|| "RVO metadata is unavailable".to_owned())?;
    if solves.is_empty() {
        return Ok(rows_for(channels));
    }
    name_agents(api, &metadata, capture)?;
    let mut named = Vec::with_capacity(solves.len());
    for solve in solves {
        match capture.rvo_agent_owners.get(&solve.agent) {
            Some(agent) => named.push((*agent, solve)),
            // An agent the MCFR does not record, a map `FightCrystal` in
            // neither side's building lists, holds its place: its solve is
            // left out.
            None if solve.exit == RvoExit::Locked => {}
            None => {
                return Err(format!(
                    "a {:?} RVO agent at 0x{:x} belongs to no live unit or building",
                    solve.exit, solve.agent
                ));
            }
        }
    }
    named.sort_by_key(|(agent, _)| *agent);
    if named.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err("an RVO agent was solved twice in one tick".into());
    }
    let mut rows = rows_for(channels);
    for (agent, solve) in named {
        let neighbours = neighbour_rows(capture, agent, &solve)?;
        if let Some(rows) = rows.solve.as_mut() {
            rows.push(solve_row(agent, &solve));
        }
        if let Some(rows) = rows.neighbour.as_mut() {
            rows.extend(neighbours);
        }
        if let Some(rows) = rows.vo.as_mut() {
            rows.extend(
                solve
                    .vos
                    .iter()
                    .enumerate()
                    .map(|(index, vo)| vo_row(agent, index, vo)),
            );
        }
    }
    Ok(rows)
}

fn rows_for(channels: RvoChannels) -> RvoRows {
    RvoRows {
        solve: channels.solve.then(Vec::new),
        neighbour: channels.neighbour.then(Vec::new),
        vo: channels.vo.then(Vec::new),
    }
}

fn neighbour_rows(
    capture: &CaptureState,
    agent: ObjectRef,
    solve: &RawSolve,
) -> Result<Vec<RvoNeighbour>, String> {
    let reads_vos = matches!(solve.exit, RvoExit::Free | RvoExit::Avoided);
    let mut next_vo = 0_usize;
    let mut rows = Vec::with_capacity(solve.neighbours.len());
    for (slot, neighbour) in solve.neighbours.iter().enumerate() {
        if neighbour.agent == solve.agent {
            // The build skips it without a VO; the quadtree never returns it.
            return Err(format!("{agent:?} is its own RVO neighbour"));
        }
        let kind = classify(&solve.fields, &neighbour.fields);
        let vo = (reads_vos && makes_vo(kind)).then(|| {
            next_vo += 1;
            next_vo - 1
        });
        let at = |values: &[i64]| vo.and_then(|index| values.get(index).copied());
        rows.push(RvoNeighbour {
            agent,
            slot: u32::try_from(slot).map_err(|_| "neighbour slot overflow")?,
            neighbour: capture.rvo_agent_owners.get(&neighbour.agent).copied(),
            distance_sq_raw: neighbour.distance_sq.raw,
            kind,
            vo: vo.map(|index| u32::try_from(index).unwrap_or(u32::MAX)),
            radius_raw: vo
                .and_then(|index| solve.vos.get(index))
                .map(|vo| vo.radius.raw),
            colliding: vo
                .and_then(|index| solve.vos.get(index))
                .map(|vo| vo.colliding),
            penetration_raw: at(&solve.penetrations),
            weight_raw: at(&solve.weights),
        });
    }
    if reads_vos && next_vo != solve.vos.len() {
        return Err(format!(
            "{agent:?} has {} VOs, but its {} neighbours account for {next_vo}",
            solve.vos.len(),
            solve.neighbours.len()
        ));
    }
    Ok(rows)
}

fn solve_row(agent: ObjectRef, solve: &RawSolve) -> RvoSolve {
    let fields = &solve.fields;
    let trace = |index: usize| solve.traces.get(index).copied();
    RvoSolve {
        agent,
        exit: solve.exit,
        position: vec(fields.position),
        elevation_raw: fields.elevation.raw,
        height_raw: fields.height.raw,
        current_velocity: vec(fields.current_velocity),
        desired_velocity: vec(fields.desired_velocity),
        desired_target: vec(fields.desired_target),
        desired_speed_raw: fields.desired_speed.raw,
        max_speed_raw: fields.max_speed.raw,
        radius_outer_raw: fields.radius_outer.raw,
        radius_inner_raw: fields.radius_inner.raw,
        size: fields.size,
        priority_raw: fields.priority.raw,
        layer: fields.layer,
        collides_with: fields.collides_with,
        group: fields.group,
        ignore_same_group: fields.ignore_same_group,
        team_id: fields.team.id,
        team_radius_raw: fields.team.radius.raw,
        max_neighbours: fields.max_neighbours,
        neighbour_count: u32::try_from(solve.neighbours.len()).unwrap_or(u32::MAX),
        biased_velocity: solve.bias.map(|bias| vec(bias.velocity)),
        biased_target: solve.bias.map(|bias| vec(bias.target)),
        first_trace_point: trace(0).map(|(point, _)| vec(point)),
        first_trace_score_raw: trace(0).map(|(_, score)| score.raw),
        second_trace_point: trace(1).map(|(point, _)| vec(point)),
        second_trace_score_raw: trace(1).map(|(_, score)| score.raw),
        output_target: solve.output.map(|(target, _)| vec(target)),
        output_speed_raw: solve.output.map(|(_, speed)| speed.raw),
    }
}

fn vo_row(agent: ObjectRef, index: usize, vo: &NativeVo) -> RvoVo {
    RvoVo {
        agent,
        vo: u32::try_from(index).unwrap_or(u32::MAX),
        line1: vec(vo.line1),
        line2: vec(vo.line2),
        dir1: vec(vo.dir1),
        dir2: vec(vo.dir2),
        cutoff_line: vec(vo.cutoff_line),
        cutoff_dir: vec(vo.cutoff_dir),
        circle_center: vec(vo.circle_center),
        colliding: vo.colliding,
        radius_raw: vo.radius.raw,
        weight_factor_raw: vo.weight_factor.raw,
        weight_bonus_raw: vo.weight_bonus.raw,
        segment_start: vec(vo.segment_start),
        segment_end: vec(vo.segment_end),
        segment: vo.segment,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn standing(elevation: i64, height: i64) -> AgentFields {
        AgentFields {
            elevation: FixedPoint { raw: elevation },
            height: FixedPoint { raw: height },
            ..AgentFields::default()
        }
    }

    #[test]
    fn a_neighbour_is_classified_as_the_build_branches() {
        let agent = standing(0, 100);
        // Vertical ranges overlap unless the gap is at least 44 raw, the
        // tolerance of `FPoint.op_LessThan`.
        assert_eq!(
            classify(&agent, &standing(143, 10)),
            RvoNeighbourKind::SameGroup
        );
        assert_eq!(
            classify(&agent, &standing(144, 10)),
            RvoNeighbourKind::OtherElevation
        );
        let opponent = AgentFields {
            group: 1,
            ..standing(0, 100)
        };
        assert_eq!(classify(&agent, &opponent), RvoNeighbourKind::Opponent);
        // The neighbour's flag decides, not the agent's.
        let passable = AgentFields {
            ignore_same_group: true,
            ..standing(0, 100)
        };
        assert_eq!(
            classify(&agent, &passable),
            RvoNeighbourKind::IgnoredSameGroup
        );
        assert_eq!(classify(&passable, &agent), RvoNeighbourKind::SameGroup);
        let team = |id| AgentFields {
            team: NativeTeam {
                id,
                radius: FixedPoint { raw: 7 },
            },
            ..standing(0, 100)
        };
        assert_eq!(classify(&team(2), &team(2)), RvoNeighbourKind::TeamRadius);
        assert_eq!(classify(&team(0), &team(0)), RvoNeighbourKind::SameGroup);
        assert_eq!(classify(&team(2), &team(3)), RvoNeighbourKind::SameGroup);
        assert!(!makes_vo(RvoNeighbourKind::OtherElevation));
        assert!(makes_vo(RvoNeighbourKind::TeamRadius));
    }
}
