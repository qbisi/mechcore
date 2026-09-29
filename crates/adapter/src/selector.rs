//! The target search channels: every search a skill, unit or construction
//! makes, and its best-scored candidates.
//!
//! The build scores targets three ways, and each shows a candidate's identity
//! differently:
//!
//! - `ScoreRatingTargetSelector.Select` over fewer than 51 actors scores on the
//!   main thread, calling `CalculateScore` and then
//!   `Selector.CheckResultTarget(score, candidate)` for each candidate;
//! - `Select` over 51 or more scores on worker threads and then calls
//!   `CheckResultTarget` on the main thread, so the score and the candidate are
//!   seen but not the terms;
//! - a main skill's search is batched for the whole team by
//!   `TeamScoreRatingTargetSelectJob` on worker threads, which keeps only the
//!   winners' indices. `TrySelect` reads them back; there, each of the
//!   source's candidates is scored again with the job's own native functions,
//!   and the winner the job chose must be the lowest score.
//!
//! A search is one `target_search` row and at most [`TARGET_CANDIDATES`]
//! `target_candidate` rows, so the channels grow with the number of searches,
//! not with searches times candidates.

use crate::capture::{CaptureState, FixedPoint, FixedVec2, capture_state, object_ref_from_pointer};
use crate::il2cpp::{Api, FieldInfo, MethodInfo, Object};
use mechcore_mcfr::{TARGET_CANDIDATES, TargetCandidate, TargetSearch, TargetSearchPath};
use std::{
    cell::RefCell,
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
    sync::atomic::{AtomicBool, AtomicPtr, Ordering},
};

/// `Select` hands scoring to `ScoreRatingTargetSelectJob` from this many actors.
const JOB_ACTOR_COUNT: i32 = 51;
/// A list longer than this is not the build's.
const LIST_CAP: usize = 1 << 20;

static ARMED: AtomicBool = AtomicBool::new(false);
static ORIGINAL_CALCULATE_SCORE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_CHECK_RESULT_TARGET: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_SELECT: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_TRY_SELECT: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

thread_local! {
    /// The `Select` calls running on this thread, innermost last.
    static FRAMES: RefCell<Vec<Frame>> = const { RefCell::new(Vec::new()) };
}

type CalculateScoreFn = unsafe extern "C" fn(
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
type CheckResultTargetFn =
    unsafe extern "C" fn(*mut Object, FixedPoint, *mut Object, *const MethodInfo);
type SelectFn = unsafe extern "C" fn(
    *mut Object,
    *mut Object,
    *mut Object,
    *mut *mut Object,
    *const MethodInfo,
) -> *mut Object;
type TrySelectFn = unsafe extern "C" fn(
    *mut Object,
    *mut Object,
    *mut *mut Object,
    *mut *mut Object,
    *const MethodInfo,
) -> bool;
type DistanceFn = unsafe extern "C" fn(
    *const u8,
    FixedVec2,
    FixedPoint,
    FixedVec2,
    FixedPoint,
    *const MethodInfo,
) -> FixedPoint;
type DistanceScoreFn =
    unsafe extern "C" fn(*const u8, FixedPoint, FixedPoint, *const MethodInfo) -> FixedPoint;
type AngleFn = unsafe extern "C" fn(
    FixedVec2,
    FixedVec2,
    FixedPoint,
    *mut FixedPoint,
    *const MethodInfo,
) -> bool;
type AngleScoreFn = unsafe extern "C" fn(*const u8, FixedPoint, *const MethodInfo) -> FixedPoint;
type PointPairFn = unsafe extern "C" fn(FixedPoint, FixedPoint, *const MethodInfo) -> FixedPoint;
type PointCompareFn = unsafe extern "C" fn(FixedPoint, FixedPoint, *const MethodInfo) -> bool;

/// A native method: its entry and its `MethodInfo`.
#[derive(Clone, Copy)]
struct Native {
    entry: usize,
    method: usize,
}

impl Native {
    fn resolve(
        api: Api,
        class: *mut crate::il2cpp::Class,
        name: &str,
        arguments: i32,
    ) -> Result<Self, String> {
        let method = api
            .method(class, name, arguments)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            entry: api
                .method_pointer(method)
                .map_err(|error| error.to_string())? as usize,
            method: method as usize,
        })
    }

    const fn info(self) -> *const MethodInfo {
        self.method as *const MethodInfo
    }
}

/// The fields and native functions a search reads, resolved once.
#[derive(Clone, Copy)]
pub(crate) struct SelectorMetadata {
    use_job_system: usize,
    attackers: usize,
    targets: usize,
    invisible_offset: usize,
    team_job: usize,
    fight_skill_base: usize,
    skill_owner: usize,
    distance: Native,
    distance_score: Native,
    angle: Native,
    angle_score: Native,
    subtract: Native,
    greater_or_equal: Native,
    calculate_score_method: usize,
}

/// `TeamScoreRatingTargetSelectJob`, as `FightCalculator` holds it.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TeamJob {
    results: usize,
    results_length: i32,
    _results_allocator: i32,
    source_datas: usize,
    target_datas: usize,
    _calculators: [u8; 8],
}

const _: () = assert!(std::mem::size_of::<TeamJob>() == 0x28);

/// The head of an `UnsafeList<T>`, which a `NativeList<T>` points at.
#[repr(C)]
#[derive(Clone, Copy)]
struct UnsafeList {
    data: usize,
    length: i32,
    capacity: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SourceData {
    position: FixedVec2,
    radius: FixedPoint,
    rotation: FixedPoint,
    min_rotation: FixedPoint,
    max_rotation: FixedPoint,
    min_attack_range: FixedPoint,
    max_attack_range: FixedPoint,
    air_offset: FixedPoint,
    ground_offset: FixedPoint,
    attack_target_type: i32,
    target_start: i32,
    target_end: i32,
    _padding: i32,
}

const _: () = assert!(std::mem::size_of::<SourceData>() == 0x60);

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TargetData {
    position: FixedVec2,
    radius: FixedPoint,
    is_fly: bool,
    is_visible: bool,
    _padding: [u8; 6],
}

const _: () = assert!(std::mem::size_of::<TargetData>() == 0x20);

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SelectResult {
    min_score_index: i32,
    next_index: i32,
    min_distance_index: i32,
    valid: bool,
    _padding: [u8; 3],
}

const _: () = assert!(std::mem::size_of::<SelectResult>() == 0x10);

/// `CalculateScore`'s per-candidate arguments and its result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Terms {
    distance: i64,
    distance_score: i64,
    angle: i64,
    angle_score: i64,
    is_left_side: bool,
    max_attack_range: i64,
    source_rotation: i64,
    min_rotation: i64,
    max_rotation: i64,
    score: i64,
}

struct Frame {
    path: TargetSearchPath,
    /// The last `CalculateScore` of a main-thread `Select`, waiting for the
    /// `CheckResultTarget` that names its candidate.
    stash: Option<Terms>,
    candidates: Vec<RawCandidate>,
}

#[derive(Clone, Copy)]
struct RawCandidate {
    actor: usize,
    score: i64,
    terms: Option<Terms>,
}

/// One search as the hooks saw it, before its actors are named.
pub(crate) struct RawSearch {
    owner: usize,
    skill_slot: Option<u16>,
    path: TargetSearchPath,
    candidates: Vec<RawCandidate>,
    target: usize,
    nearest: usize,
}

/// Which channels a recording asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TargetChannels {
    pub(crate) search: bool,
    pub(crate) candidate: bool,
}

impl TargetChannels {
    pub(crate) const fn any(self) -> bool {
        self.search || self.candidate
    }
}

/// Turns the hooks' reads on for a recording that asked for a target channel.
pub(crate) fn arm(channels: TargetChannels) {
    ARMED.store(channels.any(), Ordering::Release);
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) fn initialize(api: Api) -> Result<SelectorMetadata, String> {
    let error = |error: crate::il2cpp::Error| error.to_string();
    let class =
        |image: &str, namespace: &str, name: &str| api.class(image, namespace, name).map_err(error);
    let field = |class, name: &str| {
        api.field(class, name)
            .map(|field| field as usize)
            .map_err(error)
    };
    let selector = class(
        "GRFight.dll",
        "GameRiver.Fight",
        "ScoreRatingTargetSelector",
    )?;
    let inner = class(
        "GRFight.dll",
        "GameRiver.Fight",
        "ScoreRatingTargetSelector/Selector",
    )?;
    let calculator = class("GRFight.dll", "GameRiver.Fight", "FightCalculator")?;
    let fight_skill_base = class("GRFight.dll", "GameRiver.Fight", "FightSkillBase")?;
    let owner_behaviour = class("GRFight.dll", "GameRiver.Fight", "SkillOwnerBehaviour")?;
    let distance = class("GRFight.dll", "GameRiver.Fight", "DistanceCalculator")?;
    let distance_score = class("GRFight.dll", "GameRiver.Fight", "DistanceScoreCalculator")?;
    let angle_score = class("GRFight.dll", "GameRiver.Fight", "AngleScoreCalculator")?;
    let utility = class("GRFight.dll", "GameRiver.Fight", "FightUtility")?;
    let point = class("GRUtility.dll", "FixedMath", "FPoint")?;
    let calculate_score = api.method(selector, "CalculateScore", 9).map_err(error)?;
    let metadata = SelectorMetadata {
        use_job_system: field(selector, "useJobSystem")?,
        attackers: field(selector, "attackers")?,
        targets: field(selector, "targets")?,
        invisible_offset: field(selector, "invisibleActorDistanceScoreOffset")?,
        team_job: field(calculator, "teamScoreRatingTargetSelectJob")?,
        fight_skill_base: fight_skill_base as usize,
        skill_owner: field(owner_behaviour, "skillOwner")?,
        distance: Native::resolve(api, distance, "Calculate", 4)?,
        distance_score: Native::resolve(api, distance_score, "Calculate", 2)?,
        angle: Native::resolve(api, utility, "CalculateAngle", 4)?,
        angle_score: Native::resolve(api, angle_score, "Calculate", 1)?,
        subtract: Native::resolve(api, point, "op_Subtraction", 2)?,
        greater_or_equal: Native::resolve(api, point, "op_GreaterThanOrEqual", 2)?,
        calculate_score_method: calculate_score as usize,
    };
    let hook = |method, replacement: *const c_void, slot, label| {
        crate::capture::install_inline_hook(api, method, replacement, slot, label)
    };
    hook(
        calculate_score,
        calculate_score_hook as *const c_void,
        &ORIGINAL_CALCULATE_SCORE,
        "ScoreRatingTargetSelector.CalculateScore",
    )?;
    hook(
        api.method(inner, "CheckResultTarget", 2).map_err(error)?,
        check_result_target_hook as *const c_void,
        &ORIGINAL_CHECK_RESULT_TARGET,
        "ScoreRatingTargetSelector.Selector.CheckResultTarget",
    )?;
    hook(
        api.method(selector, "Select", 3).map_err(error)?,
        select_hook as *const c_void,
        &ORIGINAL_SELECT,
        "ScoreRatingTargetSelector.Select",
    )?;
    hook(
        api.method(selector, "TrySelect", 4).map_err(error)?,
        try_select_hook as *const c_void,
        &ORIGINAL_TRY_SELECT,
        "ScoreRatingTargetSelector.TrySelect",
    )?;
    Ok(metadata)
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn calculate_score_hook(
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
    let original = ORIGINAL_CALCULATE_SCORE.load(Ordering::Acquire);
    if original.is_null() {
        return FixedPoint::default();
    }
    // SAFETY: the installer stores the trampoline for this exact IL2CPP method ABI.
    let original: CalculateScoreFn = unsafe { std::mem::transmute(original) };
    // SAFETY: all arguments are forwarded unchanged.
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
    if ARMED.load(Ordering::Acquire) {
        FRAMES.with(|frames| {
            if let Some(frame) = frames.borrow_mut().last_mut()
                && frame.path == TargetSearchPath::Select
            {
                frame.stash = Some(Terms {
                    distance: distance.raw,
                    distance_score: distance_score.raw,
                    angle: angle.raw,
                    angle_score: angle_score.raw,
                    is_left_side,
                    max_attack_range: max_attack_range.raw,
                    source_rotation: source_rotation.raw,
                    min_rotation: min_rotation.raw,
                    max_rotation: max_rotation.raw,
                    score: score.raw,
                });
            }
        });
    }
    score
}

unsafe extern "C" fn check_result_target_hook(
    selector: *mut Object,
    score: FixedPoint,
    actor: *mut Object,
    method: *const MethodInfo,
) {
    let original = ORIGINAL_CHECK_RESULT_TARGET.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    // SAFETY: the installer stores the trampoline for this exact IL2CPP method ABI.
    let original: CheckResultTargetFn = unsafe { std::mem::transmute(original) };
    // SAFETY: all arguments are forwarded unchanged.
    unsafe { original(selector, score, actor, method) };
    if ARMED.load(Ordering::Acquire) {
        FRAMES.with(|frames| {
            if let Some(frame) = frames.borrow_mut().last_mut() {
                let terms = frame.stash.take().filter(|terms| terms.score == score.raw);
                frame.candidates.push(RawCandidate {
                    actor: actor as usize,
                    score: score.raw,
                    terms,
                });
            }
        });
    }
}

unsafe extern "C" fn select_hook(
    selector: *mut Object,
    attacker: *mut Object,
    actors: *mut Object,
    nearest: *mut *mut Object,
    method: *const MethodInfo,
) -> *mut Object {
    let original = ORIGINAL_SELECT.load(Ordering::Acquire);
    if original.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: the installer stores the trampoline for this exact IL2CPP method ABI.
    let original: SelectFn = unsafe { std::mem::transmute(original) };
    if !ARMED.load(Ordering::Acquire) {
        // SAFETY: all arguments are forwarded unchanged.
        return unsafe { original(selector, attacker, actors, nearest, method) };
    }
    let path = catch_unwind(AssertUnwindSafe(|| select_path(selector, actors)))
        .unwrap_or_else(|_| Err("reading a Select panicked".into()));
    FRAMES.with(|frames| {
        frames.borrow_mut().push(Frame {
            path: *path.as_ref().unwrap_or(&TargetSearchPath::Select),
            stash: None,
            candidates: Vec::new(),
        });
    });
    // SAFETY: all arguments are forwarded unchanged.
    let target = unsafe { original(selector, attacker, actors, nearest, method) };
    let frame = FRAMES.with(|frames| frames.borrow_mut().pop());
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let search = path.and_then(|path| {
            let frame = frame.ok_or("a Select lost its frame")?;
            let nearest = if nearest.is_null() {
                0
            } else {
                // SAFETY: `nearest` is the `out` argument the method has written.
                unsafe { nearest.read() as usize }
            };
            let (owner, skill_slot) = searcher(attacker)?;
            Ok(RawSearch {
                owner,
                skill_slot,
                path,
                candidates: frame.candidates,
                target: target as usize,
                nearest,
            })
        });
        record(search);
    }));
    target
}

unsafe extern "C" fn try_select_hook(
    calculator: *mut Object,
    attacker: *mut Object,
    target: *mut *mut Object,
    nearest: *mut *mut Object,
    method: *const MethodInfo,
) -> bool {
    let original = ORIGINAL_TRY_SELECT.load(Ordering::Acquire);
    if original.is_null() {
        return false;
    }
    // SAFETY: the installer stores the trampoline for this exact IL2CPP method ABI.
    let original: TrySelectFn = unsafe { std::mem::transmute(original) };
    // SAFETY: all arguments are forwarded unchanged.
    let selected = unsafe { original(calculator, attacker, target, nearest, method) };
    if selected && ARMED.load(Ordering::Acquire) && !target.is_null() && !nearest.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            // SAFETY: both are the `out` arguments the method has written.
            let (target, nearest) = unsafe { (target.read() as usize, nearest.read() as usize) };
            record(
                team_search(calculator, attacker).map(|(owner, skill_slot, candidates)| {
                    RawSearch {
                        owner,
                        skill_slot,
                        path: TargetSearchPath::Team,
                        candidates,
                        target,
                        nearest,
                    }
                }),
            );
        }));
    }
    selected
}

fn record(search: Result<RawSearch, String>) {
    let mut state = capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !state.armed || !state.in_update || !state.instruments.target.any() {
        return;
    }
    match search {
        Ok(search) => state.target_searches.push(search),
        Err(error) => state.fail(format!("target search: {error}")),
    }
}

fn runtime() -> Result<(Api, SelectorMetadata), String> {
    let api = crate::capture::runtime_api().ok_or("the adapter runtime is gone")?;
    let metadata = capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .metadata
        .selector
        .ok_or("selector metadata is unavailable")?;
    Ok((api, metadata))
}

fn select_path(selector: *mut Object, actors: *mut Object) -> Result<TargetSearchPath, String> {
    let (api, metadata) = runtime()?;
    let use_job: bool = api
        .field_value(selector, metadata.use_job_system as *mut FieldInfo)
        .map_err(|error| format!("useJobSystem: {error}"))?;
    let count: i32 = api
        .invoke_value(actors, "get_Count", &mut [])
        .map_err(|error| format!("actors.Count: {error}"))?;
    Ok(if use_job && count >= JOB_ACTOR_COUNT {
        TargetSearchPath::SelectJob
    } else {
        TargetSearchPath::Select
    })
}

/// The unit or construction behind an attacker, and the skill's slot when the
/// attacker is a skill.
fn searcher(attacker: *mut Object) -> Result<(usize, Option<u16>), String> {
    let (api, metadata) = runtime()?;
    let is_skill = api.object_class(attacker).is_some_and(|class| {
        api.class_is_or_inherits(
            class,
            metadata.fight_skill_base as *mut crate::il2cpp::Class,
        )
    });
    if !is_skill {
        return Ok((attacker as usize, None));
    }
    let owner: *mut Object = api
        .field_value(attacker, metadata.skill_owner as *mut FieldInfo)
        .map_err(|error| format!("skillOwner: {error}"))?;
    if owner.is_null() {
        return Err("a searching skill has no owner".into());
    }
    let skills = api
        .invoke(owner, "GetSkills", &mut [])
        .map_err(|error| format!("GetSkills: {error}"))?;
    let count = crate::capture::list_count(api, skills, i32::from(u16::MAX))?;
    for slot in 0..count {
        if crate::capture::list_item(api, skills, slot)? == attacker {
            return Ok((owner as usize, u16::try_from(slot).ok()));
        }
    }
    Err("a searching skill is absent from its owner's GetSkills".into())
}

/// A managed `List<T>`'s elements `start..end`, read from its backing array.
fn list_range<T: Copy>(
    api: Api,
    list: *mut Object,
    start: usize,
    end: usize,
) -> Result<Vec<T>, String> {
    let class = api.object_class(list).ok_or("a list has no class")?;
    let items: *mut Object = api
        .field_value(
            list,
            api.field(class, "_items")
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    let size: i32 = api
        .field_value(
            list,
            api.field(class, "_size")
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    let size = usize::try_from(size).map_err(|_| format!("a list's size is {size}"))?;
    if start > end || end > size || size > LIST_CAP {
        return Err(format!(
            "elements {start}..{end} lie outside a list of {size}"
        ));
    }
    api.value_array_range(items, start, end - start)
        .map_err(|error| error.to_string())
}

fn list_length(api: Api, list: *mut Object) -> Result<usize, String> {
    let class = api.object_class(list).ok_or("a list has no class")?;
    let size: i32 = api
        .field_value(
            list,
            api.field(class, "_size")
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    usize::try_from(size).map_err(|_| format!("a list's size is {size}"))
}

/// Element `index` of a native list.
fn native_element<T: Copy>(list: usize, index: usize, label: &str) -> Result<T, String> {
    if list == 0 {
        return Err(format!("{label} is not allocated"));
    }
    // SAFETY: `list` is the `UnsafeList` a `NativeList` field points at.
    let head = unsafe { (list as *const UnsafeList).read() };
    let length =
        usize::try_from(head.length).map_err(|_| format!("{label} length {}", head.length))?;
    if index >= length || head.data == 0 {
        return Err(format!(
            "{label}[{index}] lies outside its {length} elements"
        ));
    }
    // SAFETY: the index was checked against the list's length.
    Ok(unsafe { (head.data as *const T).add(index).read() })
}

type TeamCandidates = (usize, Option<u16>, Vec<RawCandidate>);

/// A batched search's candidates, scored again the way
/// `TeamScoreRatingTargetSelectJob.Execute` scores them, and checked against
/// the winner the job kept.
fn team_search(calculator: *mut Object, attacker: *mut Object) -> Result<TeamCandidates, String> {
    let (api, metadata) = runtime()?;
    let attackers = api.static_object(metadata.attackers as *mut FieldInfo);
    let targets = api.static_object(metadata.targets as *mut FieldInfo);
    let everyone: Vec<usize> = list_range(api, attackers, 0, list_length(api, attackers)?)?;
    let index = everyone
        .iter()
        .position(|pointer| *pointer == attacker as usize)
        .ok_or("TrySelect answered for an attacker the batch does not hold")?;
    let job: TeamJob = api
        .field_value(calculator, metadata.team_job as *mut FieldInfo)
        .map_err(|error| format!("teamScoreRatingTargetSelectJob: {error}"))?;
    if index >= usize::try_from(job.results_length).unwrap_or(0) {
        return Err(format!("the batch has no result {index}"));
    }
    // SAFETY: the index was checked against the results' length.
    let result = unsafe { (job.results as *const SelectResult).add(index).read() };
    let source: SourceData = native_element(job.source_datas, index, "sourceDatas")?;
    let start = usize::try_from(source.target_start).map_err(|_| "negative target start")?;
    let end = usize::try_from(source.target_end).map_err(|_| "negative target end")?;
    let actors: Vec<usize> = list_range(api, targets, start, end)?;
    let invisible: FixedPoint = api.static_value(metadata.invisible_offset as *mut FieldInfo);
    let calculate_score: CalculateScoreFn = {
        let original = ORIGINAL_CALCULATE_SCORE.load(Ordering::Acquire);
        if original.is_null() {
            return Err("CalculateScore is not hooked".into());
        }
        // SAFETY: the trampoline runs the unhooked method.
        unsafe { std::mem::transmute(original) }
    };
    let mut candidates = Vec::new();
    let mut ordinals = Vec::new();
    for (offset, actor) in actors.iter().enumerate() {
        let target: TargetData = native_element(job.target_datas, start + offset, "targetDatas")?;
        if let Some(terms) = score_again(&metadata, calculate_score, &source, &target, invisible) {
            ordinals.push(start + offset);
            candidates.push(RawCandidate {
                actor: *actor,
                score: terms.score,
                terms: Some(terms),
            });
        }
    }
    check_team_winner(&result, &ordinals, &candidates)?;
    let (owner, skill_slot) = searcher(attacker)?;
    Ok((owner, skill_slot, candidates))
}

/// One candidate scored the way `TeamScoreRatingTargetSelectJob.Execute`
/// scores it, or nothing when it is inside the minimum attack range.
fn score_again(
    metadata: &SelectorMetadata,
    calculate_score: CalculateScoreFn,
    source: &SourceData,
    target: &TargetData,
    invisible: FixedPoint,
) -> Option<Terms> {
    let calculators = [0_u8; 1];
    let this = calculators.as_ptr();
    // SAFETY (all native calls below): each pointer is the named method's
    // entry, called with its IL2CPP arguments; the calculators are empty
    // structs, whose `this` is never read.
    let distance = unsafe {
        std::mem::transmute::<usize, DistanceFn>(metadata.distance.entry)(
            this,
            source.position,
            source.radius,
            target.position,
            target.radius,
            metadata.distance.info(),
        )
    };
    let in_range = unsafe {
        std::mem::transmute::<usize, PointCompareFn>(metadata.greater_or_equal.entry)(
            distance,
            source.min_attack_range,
            metadata.greater_or_equal.info(),
        )
    };
    if !in_range {
        return None;
    }
    let mut offset = if target.is_fly {
        source.air_offset
    } else {
        source.ground_offset
    };
    if !target.is_visible {
        offset = unsafe {
            std::mem::transmute::<usize, PointPairFn>(metadata.subtract.entry)(
                offset,
                invisible,
                metadata.subtract.info(),
            )
        };
    }
    let distance_score = unsafe {
        std::mem::transmute::<usize, DistanceScoreFn>(metadata.distance_score.entry)(
            this,
            distance,
            offset,
            metadata.distance_score.info(),
        )
    };
    let mut angle = FixedPoint::default();
    let is_left_side = unsafe {
        std::mem::transmute::<usize, AngleFn>(metadata.angle.entry)(
            source.position,
            target.position,
            source.rotation,
            &raw mut angle,
            metadata.angle.info(),
        )
    };
    let angle_score = unsafe {
        std::mem::transmute::<usize, AngleScoreFn>(metadata.angle_score.entry)(
            this,
            angle,
            metadata.angle_score.info(),
        )
    };
    let score = unsafe {
        calculate_score(
            distance,
            distance_score,
            angle,
            angle_score,
            source.max_attack_range,
            source.rotation,
            source.min_rotation,
            source.max_rotation,
            is_left_side,
            metadata.calculate_score_method as *const MethodInfo,
        )
    };
    Some(Terms {
        distance: distance.raw,
        distance_score: distance_score.raw,
        angle: angle.raw,
        angle_score: angle_score.raw,
        is_left_side,
        max_attack_range: source.max_attack_range.raw,
        source_rotation: source.rotation.raw,
        min_rotation: source.min_rotation.raw,
        max_rotation: source.max_rotation.raw,
        score: score.raw,
    })
}

/// The job keeps the lowest score; no candidate scored again may beat the one
/// it kept by more than `FPoint.op_LessThan`'s tolerance of 43 raw, which
/// covers both a raw and a tolerant comparison in the job.
fn check_team_winner(
    result: &SelectResult,
    ordinals: &[usize],
    candidates: &[RawCandidate],
) -> Result<(), String> {
    let chosen = usize::try_from(result.min_score_index).unwrap_or(0);
    let Some(winner) = chosen.checked_sub(1) else {
        return if candidates.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "the batch chose nothing among {} scored candidates",
                candidates.len()
            ))
        };
    };
    let position = ordinals
        .iter()
        .position(|ordinal| *ordinal == winner)
        .ok_or_else(|| format!("the batch chose target {winner}, which scored nothing again"))?;
    let best = candidates[position].score;
    if let Some(lower) = candidates
        .iter()
        .find(|candidate| candidate.score < best.saturating_sub(43))
    {
        return Err(format!(
            "the batch chose score {best}, but scoring again found {}",
            lower.score
        ));
    }
    Ok(())
}

/// This tick's rows of each target channel asked for.
pub(crate) fn drain(
    capture: &mut CaptureState,
) -> (Option<Vec<TargetSearch>>, Option<Vec<TargetCandidate>>) {
    let channels = capture.instruments.target;
    let searches = std::mem::take(&mut capture.target_searches);
    let mut search_rows = channels.search.then(Vec::new);
    let mut candidate_rows = channels.candidate.then(Vec::new);
    for (ordinal, search) in searches.into_iter().enumerate() {
        let ordinal = u32::try_from(ordinal).unwrap_or(u32::MAX);
        let name = |pointer: usize| object_ref_from_pointer(pointer, capture);
        if let Some(rows) = search_rows.as_mut() {
            rows.push(TargetSearch {
                search: ordinal,
                source: name(search.owner),
                skill_slot: search.skill_slot,
                path: search.path,
                candidates: u32::try_from(search.candidates.len()).unwrap_or(u32::MAX),
                target: name(search.target),
                nearest: name(search.nearest),
            });
        }
        if let Some(rows) = candidate_rows.as_mut() {
            let mut best = search.candidates;
            // Stable: equal scores keep the order the build scored them in.
            best.sort_by_key(|candidate| candidate.score);
            best.truncate(TARGET_CANDIDATES);
            rows.extend(
                best.into_iter()
                    .enumerate()
                    .map(|(rank, candidate)| TargetCandidate {
                        search: ordinal,
                        rank: u32::try_from(rank).unwrap_or(u32::MAX),
                        candidate: name(candidate.actor),
                        score_raw: candidate.score,
                        distance_raw: candidate.terms.map(|terms| terms.distance),
                        distance_score_raw: candidate.terms.map(|terms| terms.distance_score),
                        angle_raw: candidate.terms.map(|terms| terms.angle),
                        angle_score_raw: candidate.terms.map(|terms| terms.angle_score),
                        is_left_side: candidate.terms.map(|terms| terms.is_left_side),
                        max_attack_range_raw: candidate.terms.map(|terms| terms.max_attack_range),
                        source_rotation_raw: candidate.terms.map(|terms| terms.source_rotation),
                        min_rotation_raw: candidate.terms.map(|terms| terms.min_rotation),
                        max_rotation_raw: candidate.terms.map(|terms| terms.max_rotation),
                    }),
            );
        }
    }
    (search_rows, candidate_rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(score: i64) -> RawCandidate {
        RawCandidate {
            actor: 1,
            score,
            terms: None,
        }
    }

    #[test]
    fn a_batched_winner_must_be_the_lowest_rescored_candidate() {
        let result = |index| SelectResult {
            min_score_index: index,
            ..SelectResult::default()
        };
        let scored = [candidate(500), candidate(300), candidate(300)];
        // Indices are 1-based absolute: target 11 is the second candidate.
        assert!(check_team_winner(&result(12), &[10, 11, 12], &scored).is_ok());
        assert!(check_team_winner(&result(11), &[10, 11, 12], &scored).is_err());
        assert!(check_team_winner(&result(14), &[10, 11, 12], &scored).is_err());
        assert!(check_team_winner(&result(0), &[10, 11, 12], &scored).is_err());
        assert!(check_team_winner(&result(0), &[], &[]).is_ok());
        // Within the tolerance, either is the lowest.
        let close = [candidate(300), candidate(343)];
        assert!(check_team_winner(&result(12), &[10, 11], &close).is_ok());
    }
}
