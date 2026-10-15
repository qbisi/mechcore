//! The live state of a match being watched.
//!
//! A spectator runs about `BattleInfo.WatchDelay` seconds behind the match it
//! watches, so its own round and state say where the match was, not where it
//! is. What the fight server says the match is doing arrives once, as the
//! `MatchStageInfo` answering the spectator's join, and the client keeps none of
//! it: `CurrentRound` goes to the snapshot processor, `FightState` is read for
//! `Ending` only, and `StateTime` only on a reconnection. The Adapter keeps a
//! copy from `MH_MatchStageInfo.DoProcess` for `status` to report.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::il2cpp::{Api, MethodInfo, Object};

/// Visits survive separate collector commands and game restarts in this work directory.
#[derive(Clone, Default, Deserialize, Serialize)]
pub(crate) struct Visits(BTreeMap<i32, u64>);

impl Visits {
    pub(crate) fn load(directory: &Path) -> Result<Self, String> {
        match std::fs::read(directory.join(".watch-visits.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| error.to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.to_string()),
        }
    }

    /// Never visited first, then oldest visits; exclude rooms left less than 60s ago.
    pub(crate) fn priority(&self, scene: i32, now: u64) -> Option<(bool, u64)> {
        match self.0.get(&scene).copied() {
            None => Some((false, 0)),
            Some(left) if now.saturating_sub(left) >= 60_000 => Some((true, left)),
            Some(_) => None,
        }
    }

    pub(crate) fn left(&mut self, directory: &Path, scene: i32) -> Result<(), String> {
        self.0.insert(scene, now());
        let mut pending = tempfile::NamedTempFile::new_in(directory).map_err(|e| e.to_string())?;
        serde_json::to_writer(pending.as_file_mut(), self).map_err(|e| e.to_string())?;
        pending
            .persist(directory.join(".watch-visits.json"))
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

/// Publish the native serializer's bytes atomically, without replacing an existing round.
pub(crate) fn publish(
    directory: &Path,
    battle_id: &str,
    round: i32,
    write: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(PathBuf, bool), String> {
    if battle_id.is_empty()
        || !battle_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        || round < 1
    {
        return Err(format!(
            "invalid replay identity {battle_id:?}, round {round}"
        ));
    }
    let output = directory.join(format!("{battle_id}-r{round}.grbr"));
    if output.try_exists().map_err(|e| e.to_string())? {
        return Ok((output, true));
    }
    let pending = tempfile::NamedTempFile::new_in(directory).map_err(|e| e.to_string())?;
    write(pending.path())?;
    let file = pending.reopen().map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() == 0 {
        return Err("native replay serializer wrote an empty file".into());
    }
    file.sync_all().map_err(|e| e.to_string())?;
    match pending.persist_noclobber(&output) {
        Ok(_) => Ok((output, false)),
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => Ok((output, true)),
        Err(error) => Err(error.to_string()),
    }
}

/// What the server said the match was doing when the spectator joined.
#[derive(Clone, Copy)]
struct Stage {
    round: i32,
    /// `EFightState`: 0 zero, 1 loading, 2 prepare, 3 deploy, 4 fighting,
    /// 5 ending.
    state: i32,
    /// When that state began, server Unix seconds.
    state_time: i32,
    /// When the match began, server Unix seconds.
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
/// and the match began, through `ServerProxy.GetTimeSpanToCurrentServerTime`.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visits_exclude_sixty_seconds_and_prefer_unvisited_rooms() {
        let visits = Visits(BTreeMap::from([(10, 100_000), (20, 80_000)]));
        assert_eq!(visits.priority(10, 159_999), None);
        assert_eq!(visits.priority(10, 160_000), Some((true, 100_000)));
        assert_eq!(visits.priority(10, 99_000), None);
        assert!(visits.priority(30, 160_000) < visits.priority(20, 160_000));
        assert!(visits.priority(20, 160_000) < visits.priority(10, 160_000));
        let directory = tempfile::tempdir().unwrap();
        let mut saved = Visits::default();
        saved.left(directory.path(), 10).unwrap();
        assert_eq!(
            Visits::load(directory.path()).unwrap().priority(10, now()),
            None
        );
    }

    #[test]
    fn publication_deduplicates_rounds_and_leaves_no_partial_file() {
        let directory = tempfile::tempdir().unwrap();
        let write = |path: &Path| std::fs::write(path, b"native replay").map_err(|e| e.to_string());
        let (output, duplicate) = publish(directory.path(), "20261015--123", 2, write).unwrap();
        assert!(!duplicate);
        assert_eq!(output.file_name().unwrap(), "20261015--123-r2.grbr");
        assert!(
            publish(directory.path(), "20261015--123", 2, |_| panic!(
                "must not serialize a duplicate"
            ))
            .unwrap()
            .1
        );
        assert!(
            publish(directory.path(), "20261015--123", 3, |path| {
                write(path)?;
                Err("interrupted".into())
            })
            .is_err()
        );
        assert!(publish(directory.path(), "../escape", 2, write).is_err());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        assert_eq!(std::fs::read(output).unwrap(), b"native replay");
    }
}
