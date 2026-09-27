//! The live state of a match being watched.
//!
//! A spectator runs about `BattleInfo.WatchDelay` seconds behind the match it
//! watches, so its own round and state say where the match was, not where it
//! is. What the fight server says the match is doing arrives once, as the
//! `MatchStageInfo` answering the spectator's join, and the client keeps none of
//! it: `CurrentRound` goes to the snapshot processor, `FightState` is read for
//! `Ending` only, and `StateTime` only on a reconnection. The Adapter keeps a
//! copy from `MH_MatchStageInfo.DoProcess` for `status` to report.

use std::sync::Mutex;

use serde_json::{Value, json};

use crate::il2cpp::{Api, MethodInfo, Object};

/// What the server said the match was doing when the spectator joined.
#[derive(Clone, Copy)]
struct Stage {
    round: i32,
    /// `EFightState`: 0 zero, 1 loading, 2 prepare, 3 deploy, 4 fighting,
    /// 5 ending.
    state: i32,
    /// When that state began, server Unix seconds.
    state_time: i32,
    /// When the battle began, server Unix seconds.
    start_time: i32,
    /// `BattleInfo.DeployTime`, seconds.
    deploy_time: i32,
}

static STAGE: Mutex<Option<Stage>> = Mutex::new(None);

/// Forget the stage of the match last joined, as a new watch or a quit does.
pub(crate) fn forget() {
    *STAGE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
static ORIGINAL: std::sync::atomic::AtomicPtr<std::ffi::c_void> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) fn initialize(api: Api) -> Result<(), String> {
    let method = api
        .class("GRClient.dll", "GameRiver.Client", "MH_MatchStageInfo")
        .and_then(|class| api.method(class, "DoProcess", 2))
        .map_err(|error| error.to_string())?;
    crate::capture::install_inline_hook(
        api,
        method,
        stage_info_hook as *const std::ffi::c_void,
        &ORIGINAL,
        "MH_MatchStageInfo.DoProcess",
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
type DoProcessFn = unsafe extern "C" fn(*mut Object, *mut Object, *mut Object, *const MethodInfo);

/// `MH_MatchStageInfo.DoProcess(user, data)`, read before it runs.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
unsafe extern "C" fn stage_info_hook(
    handler: *mut Object,
    user: *mut Object,
    data: *mut Object,
    method: *const MethodInfo,
) {
    use std::sync::atomic::Ordering;
    let original = ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    if let Some(api) = crate::capture::runtime_api() {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            match read_stage(api, data) {
                Ok(stage) => {
                    *STAGE
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(stage);
                }
                Err(error) => eprintln!("mechcore-adapter: MatchStageInfo: {error}"),
            }
        }));
    }
    // SAFETY: hook installer stored the trampoline for this exact method ABI.
    let original: DoProcessFn = unsafe { std::mem::transmute(original) };
    // SAFETY: IL2CPP arguments are forwarded unchanged.
    unsafe { original(handler, user, data, method) };
}

fn read_stage(api: Api, data: *mut Object) -> Result<Stage, String> {
    let error = |error: crate::il2cpp::Error| error.to_string();
    let class = api
        .object_class(data)
        .ok_or("MatchStageInfo has no class")?;
    let field = |object: *mut Object, class, name: &str| -> Result<i32, String> {
        let field = api.field(class, name).map_err(error)?;
        api.field_value(object, field).map_err(error)
    };
    let info_field = api.field(class, "battleInfo").map_err(error)?;
    let info: *mut Object = api.field_value(data, info_field).map_err(error)?;
    if info.is_null() {
        return Err("MatchStageInfo carries no battleInfo".into());
    }
    let info_class = api.object_class(info).ok_or("BattleInfo has no class")?;
    Ok(Stage {
        round: field(data, class, "<CurrentRound>k__BackingField")?,
        state: field(data, class, "<FightState>k__BackingField")?,
        state_time: field(data, class, "<StateTime>k__BackingField")?,
        start_time: field(info, info_class, "<StartTime>k__BackingField")?,
        deploy_time: field(info, info_class, "<DeployTime>k__BackingField")?,
    })
}

/// The stage last joined, with how long ago, by the server's clock, the state
/// and the battle began, through `ServerProxy.GetTimeSpanToCurrentServerTime`.
/// `null` until a join has been answered.
pub(crate) fn live(api: Api, server: Option<*mut Object>) -> Value {
    let Some(stage) = *STAGE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
    else {
        return Value::Null;
    };
    let since = |stamp: i32| -> Option<f64> {
        let server = server?;
        let (mut stamp, mut future) = (stamp, false);
        let ticks = api
            .invoke_value::<i64>(
                server,
                "GetTimeSpanToCurrentServerTime",
                &mut [
                    crate::il2cpp::argument(&mut stamp),
                    crate::il2cpp::argument(&mut future),
                ],
            )
            .ok()?;
        // A TimeSpan counts 100 ns ticks.
        #[allow(clippy::cast_precision_loss)]
        Some(ticks as f64 / 1e7)
    };
    let state_elapsed = since(stage.state_time);
    let state_name = match stage.state {
        0 => "zero",
        1 => "loading",
        2 => "prepare",
        3 => "deploy",
        4 => "fighting",
        5 => "ending",
        _ => "unknown",
    };
    json!({
        "round": stage.round,
        "state": state_name,
        "state_elapsed_seconds": state_elapsed,
        "since_start_seconds": since(stage.start_time),
        "deploy_time_seconds": stage.deploy_time,
        // The latest the fight can begin: players may finish sooner.
        "deploy_remaining_seconds": (stage.state == 3)
            .then(|| state_elapsed.map(|elapsed| f64::from(stage.deploy_time) - elapsed))
            .flatten(),
    })
}
