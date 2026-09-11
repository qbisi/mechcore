//! Native replay deployment observations, independent of the pure transition.
//!
//! The trace preserves native snapshots and decision bookkeeping. It is a
//! native observation, not a serialized `State`: snapshot fields have native
//! meanings (notably shop counters, equipment and Energy Tower flags).

use crate::il2cpp::{Api, Error, MethodInfo, Object, argument, object_argument};
use crate::runtime::Runtime;
use serde_json::{Value, json};
use std::cell::Cell;
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Mutex, OnceLock};

static ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new(ptr::null_mut());
static ACTIVE: AtomicBool = AtomicBool::new(false);
static TRACE: OnceLock<Mutex<Trace>> = OnceLock::new();
thread_local! {
    static DEPTH: Cell<usize> = const { Cell::new(0) };
}

// Resource limits on this observer, not game rules.
const MAX_EVENTS: usize = 16_384;
const MAX_TRACE_BYTES: usize = 128 * 1024 * 1024;

#[derive(Default)]
struct Trace {
    round: i32,
    game_version: String,
    events: Vec<Value>,
    bytes: usize,
    finished: [bool; 2],
    terminal: Option<Value>,
    error: Option<String>,
}

impl Trace {
    fn push(&mut self, event: Value) -> Result<(), String> {
        let bytes = serde_json::to_vec(&event)
            .map_err(|error| error.to_string())?
            .len();
        if self.events.len() >= MAX_EVENTS || bytes > MAX_TRACE_BYTES.saturating_sub(self.bytes) {
            return Err("deployment trace exceeded its event or byte budget".into());
        }
        self.bytes += bytes;
        self.events.push(event);
        Ok(())
    }
}

fn trace() -> std::sync::MutexGuard<'static, Trace> {
    TRACE
        .get_or_init(|| Mutex::new(Trace::default()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) fn start(runtime: &mut Runtime, round: i32) -> Result<(), String> {
    if ACTIVE.load(Ordering::Acquire) {
        return Err("a deployment observation is already active".into());
    }
    if round < 1 || !runtime.current_match().is_null() {
        return Err("deployment observation starts at main_menu for a positive round".into());
    }
    let api = runtime.api;
    let game_version = (|| -> Result<String, Error> {
        let application = api.class("UnityEngine.CoreModule.dll", "UnityEngine", "Application")?;
        let version = api.invoke_static(application, "get_version", &mut [])?;
        api.string_to_rust(version.cast())
    })()
    .map_err(|error| error.to_string())?;
    if game_version != "1.11.1.3.2259" {
        return Err(format!(
            "deployment observation requires game build 1.11.1.3.2259, got {game_version}"
        ));
    }
    install(runtime)?;
    RUNTIME.store(ptr::from_mut(runtime), Ordering::Release);
    *trace() = Trace {
        round,
        game_version,
        ..Trace::default()
    };
    ACTIVE.store(true, Ordering::Release);
    Ok(())
}

pub(crate) fn stop() {
    ACTIVE.store(false, Ordering::Release);
    *trace() = Trace::default();
}

pub(crate) fn progress() -> Result<bool, String> {
    let trace = trace();
    if let Some(error) = &trace.error {
        return Err(error.clone());
    }
    Ok(trace.terminal.is_some())
}

pub(crate) fn take() -> Result<Value, String> {
    ACTIVE.store(false, Ordering::Release);
    let mut trace = trace();
    if let Some(error) = &trace.error {
        return Err(error.clone());
    }
    if trace.terminal.is_none() || trace.events.is_empty() {
        return Err("deployment trace did not reach both players' finish boundary".into());
    }
    Ok(json!({
        "schema": "mechcore.deployment-observation.v1",
        "round": trace.round,
        "game_version": trace.game_version,
        "events": std::mem::take(&mut trace.events),
        "terminal": trace.terminal.take(),
        "state_encoding": "native_snapshot_and_live_board",
    }))
}

fn fail(error: impl Into<String>) {
    let mut trace = trace();
    if trace.error.is_none() {
        trace.error = Some(error.into());
    }
    ACTIVE.store(false, Ordering::Release);
}

/// Only relocate register/stack instructions. PC-relative address loads,
/// branches and literal loads require a relocating trampoline and are refused.
fn validate_prologue(bytes: &[u8; 16]) -> Result<(), String> {
    for chunk in bytes.as_chunks::<4>().0 {
        let word = u32::from_le_bytes(*chunk);
        let stack_pair = word & 0xffc0_03e0 == 0xa980_03e0 || word & 0xffc0_03e0 == 0xa900_03e0;
        let add_sub_immediate =
            word & 0xff00_0000 == 0x9100_0000 || word & 0xff00_0000 == 0xd100_0000;
        let move_register = word & 0xffe0_ffe0 == 0xaa00_03e0;
        if !stack_pair && !add_sub_immediate && !move_register {
            return Err(format!("unsupported deployment hook prologue {bytes:02x?}"));
        }
    }
    Ok(())
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install(runtime: &Runtime) -> Result<(), String> {
    if !ORIGINAL.load(Ordering::Acquire).is_null() {
        return Ok(());
    }
    let api = runtime.api;
    let class = api
        .class("GRCore.dll", "GameRiver", "PlayerController")
        .map_err(|error| error.to_string())?;
    let method = api
        .class_method_with_parameter_types(
            class,
            "TryPerformAction",
            &["GameRiver.PlayerActionData"],
        )
        .map_err(|error| error.to_string())?;
    let target = api
        .method_pointer(method)
        .map_err(|error| error.to_string())?;
    // SAFETY: the resolved concrete method has at least four instructions.
    let bytes = unsafe { target.cast::<[u8; 16]>().read_unaligned() };
    validate_prologue(&bytes)?;
    crate::capture::install_inline_hook(
        api,
        method,
        &bytes,
        action_hook as *const c_void,
        &ORIGINAL,
        "PlayerController.TryPerformAction deployment observer",
    )
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn install(_: &Runtime) -> Result<(), String> {
    Err("deployment observation requires macOS aarch64".into())
}

type Perform = unsafe extern "C" fn(*mut Object, *mut Object, *const MethodInfo) -> bool;

unsafe extern "C" fn action_hook(
    player: *mut Object,
    action: *mut Object,
    method: *const MethodInfo,
) -> bool {
    let original = ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return false;
    }
    // SAFETY: install resolved this exact bool (this, action, MethodInfo) ABI.
    let perform: Perform = unsafe { std::mem::transmute(original) };
    let outer = DEPTH.with(|depth| {
        let old = depth.get();
        depth.set(old + 1);
        old == 0
    });
    let before = if outer && ACTIVE.load(Ordering::Acquire) {
        observe(|| before_action(player, action))
    } else {
        None
    };
    // SAFETY: the original receives all its arguments unchanged, exactly once.
    let accepted = unsafe { perform(player, action, method) };
    if let Some(Some(before)) = before {
        observe(|| after_action(&before, accepted));
    }
    DEPTH.with(|depth| depth.set(depth.get() - 1));
    accepted
}

fn observe<T>(read: impl FnOnce() -> Result<T, String>) -> Option<T> {
    match catch_unwind(AssertUnwindSafe(read)) {
        Ok(Ok(value)) => Some(value),
        Ok(Err(error)) => {
            fail(error);
            None
        }
        Err(_) => {
            fail("deployment observation panicked");
            None
        }
    }
}

fn runtime() -> Result<&'static Runtime, String> {
    let runtime = RUNTIME.load(Ordering::Acquire);
    if runtime.is_null() {
        return Err("deployment runtime is unavailable".into());
    }
    // SAFETY: called only from the Unity main thread; Runtime lives for the process.
    Ok(unsafe { &*runtime })
}

fn eligible(runtime: &Runtime) -> Result<bool, String> {
    if !ACTIVE.load(Ordering::Acquire) {
        return Ok(false);
    }
    let current = runtime.current_match();
    if current.is_null() {
        return Ok(false);
    }
    if crate::operations::classify_replay(runtime.api, current) != Some(true) {
        return Err("deployment observation left replay mode".into());
    }
    let round: i32 = runtime
        .api
        .invoke_value(current, "get_RoundCount", &mut [])
        .map_err(|error| error.to_string())?;
    Ok(round == trace().round)
}

struct Before {
    team: usize,
    kind: String,
    action: Value,
    state: Value,
}

fn before_action(player: *mut Object, action: *mut Object) -> Result<Option<Before>, String> {
    let runtime = runtime()?;
    if !eligible(runtime)? {
        return Ok(None);
    }
    let api = runtime.api;
    let team = team(api, player)?;
    let kind = api.object_class_name(action);
    if !kind.starts_with("PAD_") {
        return Err(format!("unexpected player action {kind}"));
    }
    let action = native_json(api, action).map_err(|error| error.to_string())?;
    if kind == "PAD_ChooseReinforceItem"
        && !["ID", "Index"].iter().all(|field| action[*field].is_i64())
    {
        return Err("reinforcement action is missing its native ID or Index".into());
    }
    Ok(Some(Before {
        team,
        kind,
        action,
        state: read_state(runtime)?,
    }))
}

fn after_action(before: &Before, accepted: bool) -> Result<(), String> {
    if trace().error.is_some() {
        return Ok(());
    }
    // FinishDeploy crosses a process boundary. Its pre-state is retained as a
    // checkpoint, never presented as a normal deployment transition.
    let after = if before.kind == "PAD_FinishDeploy" {
        None
    } else {
        Some(read_state(runtime()?)?)
    };
    let mut trace = trace();
    let ordinal = trace.events.len();
    trace.push(json!({
        "ordinal": ordinal, "team": before.team, "native_type": before.kind,
        "action": before.action, "accepted": accepted,
        "before": before.state, "after": after,
    }))
}

/// Called before the original `FinishDeploy`, while deployment objects still
/// describe the terminal position. No game state is written by this callback.
pub(crate) fn finish_boundary(player: *mut Object) {
    if !ACTIVE.load(Ordering::Acquire) {
        return;
    }
    observe(|| {
        let runtime = runtime()?;
        if !eligible(runtime)? {
            return Ok(());
        }
        let team = team(runtime.api, player)?;
        {
            let mut trace = trace();
            if trace.finished[team] {
                return Err(format!("team {team} finished deployment twice"));
            }
            trace.finished[team] = true;
            if !trace.finished.iter().all(|finished| *finished) {
                return Ok(());
            }
        }
        let terminal = read_state(runtime)?;
        trace().terminal = Some(terminal);
        ACTIVE.store(false, Ordering::Release);
        Ok(())
    });
}

fn team(api: Api, player: *mut Object) -> Result<usize, String> {
    match api
        .invoke_value::<i32>(player, "GetTeamIndex", &mut [])
        .map_err(|error| error.to_string())?
    {
        0 => Ok(0),
        1 => Ok(1),
        other => Err(format!("unsupported deployment team {other}")),
    }
}

struct Root(Api, u32);
impl Root {
    fn new(api: Api, object: *mut Object) -> Result<Self, Error> {
        Ok(Self(api, api.gc_handle(object)?))
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        self.0.free_gc_handle(self.1);
    }
}

fn native_json(api: Api, object: *mut Object) -> Result<Value, Error> {
    native_fields(api, object, 0)
}

fn native_fields(api: Api, object: *mut Object, depth: usize) -> Result<Value, Error> {
    if object.is_null() {
        return Ok(Value::Null);
    }
    if depth > 32 {
        return Err(Error::InvalidValue(
            "native observation nesting limit".into(),
        ));
    }
    let _root = Root::new(api, object)?;
    let class = api
        .object_class(object)
        .ok_or_else(|| Error::NullResult("native class".into()))?;
    let name = api.class_name(class);
    let namespace = api.class_namespace(class);
    if namespace == "System" {
        return Ok(match name.as_str() {
            "Int32" => json!(api.unbox::<i32>(object, &name)?),
            "UInt32" => json!(api.unbox::<u32>(object, &name)?),
            "Int64" => json!(api.unbox::<i64>(object, &name)?),
            "UInt64" => json!(api.unbox::<u64>(object, &name)?),
            "Int16" => json!(api.unbox::<i16>(object, &name)?),
            "UInt16" => json!(api.unbox::<u16>(object, &name)?),
            "Byte" => json!(api.unbox::<u8>(object, &name)?),
            "SByte" => json!(api.unbox::<i8>(object, &name)?),
            "Boolean" => json!(api.unbox::<bool>(object, &name)?),
            "Single" => finite_number(f64::from(api.unbox::<f32>(object, &name)?))?,
            "Double" => finite_number(api.unbox::<f64>(object, &name)?)?,
            "String" => json!(api.string_to_rust(object.cast())?),
            _ => {
                return Err(Error::InvalidValue(format!(
                    "unsupported native value {namespace}.{name}"
                )));
            }
        });
    }
    if namespace == "System.Collections.Generic" && name.starts_with("List`1") {
        let count: i32 = api.invoke_value(object, "get_Count", &mut [])?;
        if !(0..=16_384).contains(&count) {
            return Err(Error::InvalidValue(format!("native list size {count}")));
        }
        let mut values = Vec::new();
        for mut index in 0..count {
            let item = api.invoke(object, "get_Item", &mut [argument(&mut index)])?;
            values.push(native_fields(api, item, depth + 1)?);
        }
        return Ok(Value::Array(values));
    }
    if !(namespace.starts_with("GameRiver") || namespace == "UnityEngine" && name == "Vector2Int") {
        return Err(Error::InvalidValue(format!(
            "unsupported native observation class {namespace}.{name}"
        )));
    }
    let mut fields = serde_json::Map::new();
    let mut current = Some(class);
    while let Some(class) = current {
        if api.class_namespace(class) == "System" {
            break;
        }
        for (name, field) in api.instance_fields(class)? {
            let key = name
                .strip_prefix('<')
                .and_then(|s| s.strip_suffix(">k__BackingField"))
                .unwrap_or(&name);
            let value = native_fields(api, api.boxed_field(object, field)?, depth + 1)?;
            if fields.insert(key.to_owned(), value).is_some() {
                return Err(Error::InvalidValue(format!("duplicate native field {key}")));
            }
        }
        current = api.class_parent(class);
    }
    Ok(Value::Object(fields))
}

fn finite_number(value: f64) -> Result<Value, Error> {
    serde_json::Number::from_f64(value)
        .map(Value::Number)
        .ok_or_else(|| Error::InvalidValue("non-finite native value".into()))
}

fn object_list(api: Api, list: *mut Object) -> Result<Vec<*mut Object>, Error> {
    let count: i32 = api.invoke_value(list, "get_Count", &mut [])?;
    if !(0..=16_384).contains(&count) {
        return Err(Error::InvalidValue(format!("list count {count}")));
    }
    (0..count)
        .map(|mut index| api.invoke(list, "get_Item", &mut [argument(&mut index)]))
        .collect()
}

fn ids(api: Api, list: *mut Object) -> Result<Vec<i32>, Error> {
    object_list(api, list)?
        .into_iter()
        .map(|item| api.invoke_value(item, "GetID", &mut []))
        .collect()
}

fn read_player(api: Api, current: *mut Object, player: *mut Object) -> Result<Value, Error> {
    let class = api.class("GRCore.dll", "GameRiver", "PlayerSnapshotController")?;
    let snapshotter = api.allocate_object(class)?;
    let _snapshotter = Root::new(api, snapshotter)?;
    api.invoke_void(
        snapshotter,
        ".ctor",
        &mut [object_argument(current), object_argument(player)],
    )?;
    let data = api.new_object(api.class(
        "GRCore.dll",
        "GameRiver.Serialization",
        "PlayerSnapshotData",
    )?)?;
    let _data = Root::new(api, data)?;
    api.invoke_void(snapshotter, "TakeSnapshot", &mut [object_argument(data)])?;
    let snapshot = native_json(api, data)?;
    for field in [
        "supply",
        "reactorCore",
        "units",
        "shop",
        "randomStateData",
        "unitIndex",
    ] {
        if snapshot.get(field).is_none() {
            return Err(Error::InvalidValue(format!(
                "native snapshot has no {field}"
            )));
        }
    }
    let reinforcement = api.invoke(player, "GetReinforcementManager", &mut [])?;
    let class = api
        .object_class(reinforcement)
        .ok_or_else(|| Error::NullResult("reinforcement manager".into()))?;
    let groups: *mut Object =
        api.field_value(reinforcement, api.field(class, "roundReinforceItems")?)?;
    let offers = object_list(api, groups)?
        .into_iter()
        .map(|group| ids(api, group))
        .collect::<Result<Vec<_>, _>>()?;
    let chosen = ids(
        api,
        api.invoke(reinforcement, "GetChoosedReinforceItems", &mut [])?,
    )?;
    let manager = api.invoke(player, "GetEnergyTowerManager", &mut [])?;
    let mut active = Vec::new();
    for mut id in [1_i32, 3, 4, 5, 6] {
        let skill = api.invoke(manager, "GetSkill", &mut [argument(&mut id)])?;
        if skill.is_null() {
            return Err(Error::InvalidValue(format!("missing energy skill {id}")));
        }
        if api.invoke_value::<bool>(skill, "IsActive", &mut [])? {
            active.push(id);
        }
    }
    Ok(json!({
        "snapshot": snapshot,
        "reinforcement": {
            "offers": offers, "chosen": chosen,
            "remaining": api.invoke_value::<i32>(reinforcement, "GetChooseRemainCount", &mut [])?,
            "finished": api.invoke_value::<bool>(reinforcement, "IsChooseFinished", &mut [])?,
        },
        "active_energy_tower_skills": active,
        "deploy_over": api.invoke_value::<bool>(player, "IsDeployOver", &mut [])?,
    }))
}

fn read_state(runtime: &Runtime) -> Result<Value, String> {
    let api = runtime.api;
    let current = runtime.current_match();
    let read = || -> Result<Value, Error> {
        let manager = api.invoke(current, "GetPlayerManager", &mut [])?;
        let players = api.invoke(manager, "GetPlayerControllers", &mut [])?;
        let mut sides: [Option<Value>; 2] = [None, None];
        for player in object_list(api, players)? {
            let index = team(api, player).map_err(Error::InvalidValue)?;
            if sides[index].is_some() {
                return Err(Error::InvalidValue("duplicate native team".into()));
            }
            sides[index] = Some(read_player(api, current, player)?);
        }
        if sides.iter().any(Option::is_none) {
            return Err(Error::InvalidValue("missing native team".into()));
        }
        Ok(json!({"blue": sides[0], "red": sides[1]}))
    };
    let sides = read().map_err(|error| error.to_string())?;
    Ok(json!({"sides": sides, "board": crate::capture::deployment_board(runtime)?}))
}
