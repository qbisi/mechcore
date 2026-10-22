//! The game's tournament history and native replay download path.
//!
//! Python displays dates/regions, selects a tournament ID and walks pages. Each request owns one
//! scheduler turn; all native reads and calls run on Unity's main thread.

use crate::il2cpp::{Api, Object, argument, object_argument};
use crate::{
    operations,
    runtime::{self, Runtime},
};
use mechcore_protocol::{Request, Response};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicI32, Ordering},
};
use std::{
    path::PathBuf,
    ptr, thread,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, String>;

static LIST_GENERATION: AtomicI32 = AtomicI32::new(0);
#[derive(Default)]
struct PageReply {
    key: Option<(i32, i32, i32)>,
    reply: Option<Result<Value>>,
}
static PAGE_REPLY: Mutex<PageReply> = Mutex::new(PageReply {
    key: None,
    reply: None,
});

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
static ORIGINAL_LIST: std::sync::atomic::AtomicPtr<std::ffi::c_void> =
    std::sync::atomic::AtomicPtr::new(ptr::null_mut());
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
static ORIGINAL_PAGE: std::sync::atomic::AtomicPtr<std::ffi::c_void> =
    std::sync::atomic::AtomicPtr::new(ptr::null_mut());

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) fn initialize(api: Api) -> Result<()> {
    let class = api
        .class("GRClient.dll", "GameRiver.Client", "CompetitionProxy")
        .map_err(|error| error.to_string())?;
    for (name, hook, original) in [
        (
            "OnResponseMatchList",
            list_hook as *const std::ffi::c_void,
            &ORIGINAL_LIST,
        ),
        (
            "OnResponseHistoryMatchDetail",
            page_hook as *const std::ffi::c_void,
            &ORIGINAL_PAGE,
        ),
    ] {
        let method = api
            .method(class, name, 2)
            .map_err(|error| error.to_string())?;
        crate::capture::install_inline_hook(api, method, hook, original, name)?;
    }
    Ok(())
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
type ResponseFn =
    unsafe extern "C" fn(*mut Object, *mut Object, *mut Object, *const crate::il2cpp::MethodInfo);

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
unsafe extern "C" fn list_hook(
    owner: *mut Object,
    sender: *mut Object,
    message: *mut Object,
    method: *const crate::il2cpp::MethodInfo,
) {
    let pointer = ORIGINAL_LIST.load(Ordering::Acquire);
    if pointer.is_null() {
        return;
    }
    // SAFETY: the installer stores the trampoline for this exact response ABI.
    let original: ResponseFn = unsafe { std::mem::transmute(pointer) };
    // SAFETY: native arguments are forwarded unchanged, before observing its cache.
    unsafe { original(owner, sender, message, method) };
    LIST_GENERATION.fetch_add(1, Ordering::Release);
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
unsafe extern "C" fn page_hook(
    owner: *mut Object,
    sender: *mut Object,
    message: *mut Object,
    method: *const crate::il2cpp::MethodInfo,
) {
    let pointer = ORIGINAL_PAGE.load(Ordering::Acquire);
    if pointer.is_null() {
        return;
    }
    if let Some(api) = crate::capture::runtime_api() {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let result = page_response(api, message);
            let mut pending = PAGE_REPLY
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match result {
                Ok((key, value)) if pending.key == Some(key) => pending.reply = Some(Ok(value)),
                Err(error) if pending.key.is_some() => pending.reply = Some(Err(error)),
                _ => (),
            }
        }));
    }
    // SAFETY: the installer stores the trampoline for this exact response ABI.
    let original: ResponseFn = unsafe { std::mem::transmute(pointer) };
    // SAFETY: observing the reply does not change its native arguments or handler.
    unsafe { original(owner, sender, message, method) };
}

/// Read the server reply while native `OnResponseHistoryMatchDetail` owns it.
/// Ended mini tournaments are absent from `m_HistoryInfo`, so the native handler
/// otherwise retains these rows only in an open results window.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn page_response(api: Api, message: *mut Object) -> Result<((i32, i32, i32), Value)> {
    let id = field::<i32>(api, message, "id_")?;
    let sub = field::<*mut Object>(api, message, "subInfo_")?;
    let sub_id = field::<i32>(api, sub, "subid_")?;
    let page = field::<i32>(api, message, "page_")?;
    let pages = field::<i32>(api, message, "pageNum_")?;
    let ranks = objects(api, field(api, message, "rankList_")?)?;
    let mut replays = std::collections::BTreeSet::new();
    for rank in &ranks {
        for record in objects(api, field(api, *rank, "roundRecord_")?)? {
            let path = text(api, field(api, record, "replayPath_")?)?;
            if !path.is_empty() {
                replays.insert(path);
            }
        }
    }
    Ok((
        (id, sub_id, page),
        json!({"ready": true, "match_id": id, "sub_id": sub_id,
        "page": page, "pages": pages, "rank_count": ranks.len(), "replays": replays}),
    ))
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Arguments {
    List,
    Page {
        match_id: i32,
        sub_id: i32,
        page: i32,
    },
    Download {
        replay_path: String,
        output: PathBuf,
    },
}

#[derive(Clone)]
pub(crate) enum Step {
    Begin(Arguments),
    Poll(Arguments, i32),
}

pub(crate) fn execute(runtime: &mut Runtime, request: &Request) -> Response<Value> {
    if crate::offline::sandboxed() {
        return Response::failure(
            request.id,
            "invalid_game_state",
            "tournament needs an online game",
        );
    }
    let args: Arguments = match serde_json::from_value(request.arguments.clone()) {
        Ok(args) => args,
        Err(error) => return Response::failure(request.id, "invalid_arguments", error.to_string()),
    };
    if let Err(error) = validate(&args) {
        return Response::failure(request.id, "invalid_arguments", error);
    }
    let deadline = Instant::now() + Duration::from_secs(65);
    let begin = loop {
        if runtime::abandoning() {
            let (code, message) = runtime::abandonment();
            return Response::failure(request.id, code, message);
        }
        let begin = runtime::execute_internal_on_main(
            runtime,
            request.id,
            operations::InternalOperation::Tournament(Step::Begin(args.clone())),
        );
        if !begin.ok
            || !begin
                .result
                .as_ref()
                .is_some_and(|value| value["waiting"] == true)
        {
            break begin;
        }
        if Instant::now() >= deadline {
            return Response::failure(
                request.id,
                "operation_timeout",
                "the game did not finish online login and replay cache preparation in 65 seconds",
            );
        }
        thread::sleep(Duration::from_millis(100));
    };
    if !begin.ok {
        return begin;
    }
    if begin
        .result
        .as_ref()
        .is_some_and(|value| value["ready"] == true)
    {
        return begin;
    }
    let marker = begin
        .result
        .as_ref()
        .and_then(|value| value["marker"].as_i64())
        .and_then(|value| i32::try_from(value).ok())
        .unwrap_or_default();
    while Instant::now() < deadline {
        if runtime::abandoning() {
            let (code, message) = runtime::abandonment();
            return Response::failure(request.id, code, message);
        }
        let response = runtime::execute_internal_on_main(
            runtime,
            request.id,
            operations::InternalOperation::Tournament(Step::Poll(args.clone(), marker)),
        );
        if !response.ok
            || response
                .result
                .as_ref()
                .is_some_and(|value| value["ready"] == true)
        {
            return response;
        }
        thread::sleep(Duration::from_millis(100));
    }
    Response::failure(
        request.id,
        "operation_timeout",
        "the tournament request received no completed native response in 65 seconds",
    )
}

fn validate(args: &Arguments) -> Result<()> {
    match args {
        Arguments::Page {
            match_id,
            sub_id,
            page,
        } if *match_id <= 0 || *sub_id < 0 || *page <= 0 => {
            Err("match_id and page must be positive; sub_id must be nonnegative".into())
        }
        Arguments::Download {
            replay_path,
            output,
        } => {
            let parts: Vec<_> = replay_path.split('/').collect();
            if parts.len() != 2
                || parts[0].is_empty()
                || !parts[0].bytes().all(|c| c.is_ascii_digit() || c == b'.')
                || parts[0].split('.').any(str::is_empty)
                || !parts[1]
                    .strip_suffix(".rep")
                    .is_some_and(|id| !id.is_empty() && id.bytes().all(|c| c.is_ascii_digit()))
            {
                return Err("replay_path must be a native <version>/<numeric-id>.rep path".into());
            }
            if !output.is_absolute()
                || output.extension().is_none_or(|ext| ext != "grbr")
                || output.exists()
            {
                return Err("output must be a new absolute .grbr path".into());
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

pub(crate) fn on_main(runtime: &Runtime, id: u64, step: Step) -> Response<Value> {
    let result = match step {
        Step::Begin(args) => begin(runtime, &args),
        Step::Poll(args, marker) => poll(runtime, &args, marker),
    };
    match result {
        Ok(value) => Response::success(id, value),
        Err(error) => Response::failure(id, "tournament_failed", error),
    }
}

fn proxy(runtime: &Runtime, name: &str) -> Result<*mut Object> {
    operations::find_proxy(runtime, name, &format!("GameRiver.Client.{name}"))
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("{name} is unavailable; wait for online login at the main menu"))
}

fn field<T: Copy>(api: Api, object: *mut Object, name: &str) -> Result<T> {
    let class = api
        .object_class(object)
        .ok_or_else(|| format!("{name}: native object is null"))?;
    api.field(class, name)
        .and_then(|field| api.field_value(object, field))
        .map_err(|error| error.to_string())
}

fn objects(api: Api, list: *mut Object) -> Result<Vec<*mut Object>> {
    let count = operations::list_count(api, list).map_err(|error| error.to_string())?;
    (0..count)
        .map(|at| operations::list_item(api, list, at).map_err(|error| error.to_string()))
        .collect()
}

fn text(api: Api, object: *mut Object) -> Result<String> {
    if object.is_null() {
        return Ok(String::new());
    }
    api.string_to_rust(object.cast())
        .map_err(|error| error.to_string())
}

fn history(runtime: &Runtime) -> Result<*mut Object> {
    field(
        runtime.api,
        proxy(runtime, "CompetitionProxy")?,
        "m_HistoryInfo",
    )
}

/// `CompetitionController`'s current/recent list, also used by `MatchListProxy`.
fn recent(runtime: &Runtime) -> Result<*mut Object> {
    let api = runtime.api;
    let class = api
        .class(
            "GRClient.dll",
            "GameRiver.Client.Competition",
            "CompetitionController",
        )
        .map_err(|error| error.to_string())?;
    let instance = api
        .field(class, "Instance")
        .map_err(|error| error.to_string())?;
    let controller = api.static_object(instance);
    let system = api
        .invoke(controller, "get_C_SystemData", &mut [])
        .map_err(|error| error.to_string())?;
    let list_data = api
        .invoke(system, "get_C_MatchListData", &mut [])
        .map_err(|error| error.to_string())?;
    api.invoke(list_data, "get_MatchBaseInfos", &mut [])
        .map_err(|error| error.to_string())
}

fn begin(runtime: &Runtime, args: &Arguments) -> Result<Value> {
    let api = runtime.api;
    let online = api
        .invoke_value::<bool>(proxy(runtime, "ServerProxy")?, "IsOnline", &mut [])
        .map_err(|error| error.to_string())?;
    if !online {
        return Ok(json!({"waiting": true}));
    }
    match args {
        Arguments::List => {
            let marker = LIST_GENERATION.load(Ordering::Acquire);
            let proxy = proxy(runtime, "CompetitionProxy")?;
            api.invoke_void(proxy, "RequestHistoryMatch", &mut [])
                .map_err(|error| error.to_string())?;
            let mut page = 1_i32;
            let mut interval = 1.0_f32;
            let sent = api
                .invoke_value::<i32>(
                    proxy,
                    "RequestMatchList",
                    &mut [argument(&mut page), argument(&mut interval)],
                )
                .map_err(|error| error.to_string())?;
            if sent == 0 {
                if !objects(api, history(runtime)?)?.is_empty()
                    && !objects(api, recent(runtime)?)?.is_empty()
                {
                    return list_tournaments(runtime);
                }
                return Ok(json!({"waiting": true}));
            }
            Ok(json!({"marker": marker}))
        }
        Arguments::Page {
            match_id,
            sub_id,
            page,
        } => {
            *PAGE_REPLY
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = PageReply {
                key: Some((*match_id, *sub_id, *page)),
                reply: None,
            };
            let (mut id, mut sub, mut page) = (*match_id, *sub_id, *page);
            api.invoke_void(
                proxy(runtime, "CompetitionProxy")?,
                "RequestHistoryMatchDetail",
                &mut [argument(&mut id), argument(&mut sub), argument(&mut page)],
            )
            .map_err(|error| error.to_string())?;
            Ok(json!({}))
        }
        Arguments::Download { replay_path, .. } => {
            let replay_proxy = proxy(runtime, "ReplayProxy")?;
            let mut force = false;
            if !api
                .invoke_value::<bool>(replay_proxy, "TryLoadReplay", &mut [argument(&mut force)])
                .map_err(|error| error.to_string())?
            {
                return Ok(json!({"waiting": true}));
            }
            let path = api.string(replay_path).map_err(|error| error.to_string())?;
            api.invoke_void(
                replay_proxy,
                "DowloadServerReplay",
                &mut [object_argument(path)],
            )
            .map_err(|error| error.to_string())?;
            Ok(json!({}))
        }
    }
}

fn poll(runtime: &Runtime, args: &Arguments, marker: i32) -> Result<Value> {
    match args {
        Arguments::List => {
            if LIST_GENERATION.load(Ordering::Acquire) == marker
                || objects(runtime.api, history(runtime)?)?.is_empty()
            {
                return Ok(json!({"ready": false}));
            }
            list_tournaments(runtime)
        }
        Arguments::Page {
            match_id,
            sub_id,
            page,
        } => {
            let pending = PAGE_REPLY
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if pending.key != Some((*match_id, *sub_id, *page)) {
                return Err("tournament page request was replaced".into());
            }
            pending
                .reply
                .clone()
                .unwrap_or_else(|| Ok(json!({"ready": false})))
        }
        Arguments::Download {
            replay_path,
            output,
        } => download_result(runtime, replay_path, output),
    }
}

fn describe(
    api: Api,
    info: *mut Object,
    id: i32,
    start_time: i32,
    sub_ids: &[i32],
    source: &str,
) -> Result<Value> {
    let config = api
        .invoke(info, "get_Config", &mut [])
        .map_err(|error| error.to_string())?;
    let config_text = |name| {
        api.invoke(config, name, &mut [])
            .map_err(|error| error.to_string())
            .and_then(|object| text(api, object))
    };
    let params = ["get_param0", "get_param1", "get_param2"]
        .into_iter()
        .map(config_text)
        .collect::<Result<Vec<_>>>()?;
    Ok(
        json!({"match_id": id, "sub_ids": sub_ids, "start_time": start_time,
        "title": config_text("GetTitle")?, "params": params, "source": source,
        "subtype": api.invoke_value::<i32>(config, "get_subtype", &mut []).map_err(|error| error.to_string())?,
        "division_type": api.invoke_value::<i32>(config, "get_division_type", &mut []).map_err(|error| error.to_string())?}),
    )
}

fn list_tournaments(runtime: &Runtime) -> Result<Value> {
    let api = runtime.api;
    let mut entries = std::collections::BTreeMap::new();
    for info in objects(api, history(runtime)?)? {
        let id = field::<i32>(api, info, "Id")?;
        let subs: *mut Object = field(api, info, "SubIds")?;
        let sub_ids = api
            .value_array::<i32>(subs, 16384)
            .map_err(|error| error.to_string())?;
        entries.insert(
            id,
            describe(
                api,
                info,
                id,
                field(api, info, "StartTime")?,
                &sub_ids,
                "history",
            )?,
        );
    }
    for info in objects(api, recent(runtime)?)? {
        // MatchListProxy and Competition_EndList display native State 5 as ended.
        if field::<i32>(api, info, "State")? != 5 {
            continue;
        }
        let id = field::<i32>(api, info, "ID")?;
        if entries.contains_key(&id) {
            continue;
        }
        let sub_ids = objects(api, field(api, info, "SubMatchs")?)?
            .into_iter()
            .map(|sub| field::<i32>(api, sub, "Subid"))
            .collect::<Result<Vec<_>>>()?;
        entries.insert(
            id,
            describe(
                api,
                info,
                id,
                field(api, info, "StartTime")?,
                &sub_ids,
                "recent",
            )?,
        );
    }
    Ok(json!({"ready": true, "tournaments": entries.into_values().collect::<Vec<_>>()}))
}

fn download_result(
    runtime: &Runtime,
    replay_path: &str,
    output: &std::path::Path,
) -> Result<Value> {
    let api = runtime.api;
    let proxy = proxy(runtime, "ReplayProxy")?;
    let path = api.string(replay_path).map_err(|error| error.to_string())?;
    let mut replay = ptr::null_mut::<Object>();
    let found = api
        .invoke_value::<bool>(
            proxy,
            "TryGetReplayByPath",
            &mut [object_argument(path), argument(&mut replay)],
        )
        .map_err(|error| error.to_string())?;
    if !found {
        if api
            .invoke_value::<bool>(
                proxy,
                "CheckIsDownloadingByServer",
                &mut [object_argument(path)],
            )
            .map_err(|error| error.to_string())?
        {
            return Ok(json!({"ready": false}));
        }
        return Err(format!("native replay download failed: {replay_path}"));
    }
    let battle_id = api
        .invoke(replay, "GetBattleID", &mut [])
        .map_err(|error| error.to_string())
        .and_then(|id| text(api, id))?;
    let directory = output.parent().ok_or("output has no parent")?;
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let pending = tempfile::NamedTempFile::new_in(directory).map_err(|error| error.to_string())?;
    api.invoke_void(replay, "GenerateBattleRecordData", &mut [])
        .map_err(|error| error.to_string())?;
    let file = api
        .class("GRUtility.dll", "GameRiver", "FileUtility")
        .map_err(|error| error.to_string())?;
    let save = api
        .class_method_with_parameter_types(file, "SaveBinary", &["System.String", "System.Object"])
        .map_err(|error| error.to_string())?;
    let target = api
        .string(&pending.path().to_string_lossy())
        .map_err(|error| error.to_string())?;
    api.invoke_raw(
        save,
        ptr::null_mut(),
        &mut [object_argument(target), object_argument(replay)],
    )
    .map_err(|error| error.to_string())?;
    let bytes = std::fs::read(pending.path()).map_err(|error| error.to_string())?;
    mechcore_document::record::read(&bytes)?;
    pending
        .persist_noclobber(output)
        .map_err(|error| error.to_string())?;
    Ok(json!({"ready": true, "battle_id": battle_id, "replay_path": replay_path, "output": output}))
}
