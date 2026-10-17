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
use std::{
    path::PathBuf,
    ptr, thread,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, String>;

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

fn sub_match(runtime: &Runtime, mut match_id: i32, mut sub_id: i32) -> Result<Option<*mut Object>> {
    let api = runtime.api;
    let proxy = proxy(runtime, "CompetitionProxy")?;
    let info = api
        .invoke(
            proxy,
            "GetHistoryCompetitionInfo",
            &mut [argument(&mut match_id)],
        )
        .map_err(|error| error.to_string())?;
    if info.is_null() {
        return Err(format!(
            "tournament {match_id} is not in the loaded history"
        ));
    }
    let mut sub = ptr::null_mut::<Object>();
    let found = api
        .invoke_value::<bool>(
            info,
            "TryGetSubCompetition",
            &mut [argument(&mut sub_id), argument(&mut sub)],
        )
        .map_err(|error| error.to_string())?;
    Ok(found.then_some(sub))
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
            let list = history(runtime)?;
            let marker: i32 = field(api, list, "_version")?;
            api.invoke_void(
                proxy(runtime, "CompetitionProxy")?,
                "RequestHistoryMatch",
                &mut [],
            )
            .map_err(|error| error.to_string())?;
            if !objects(api, list)?.is_empty() {
                return list_history(runtime, list);
            }
            Ok(json!({"marker": marker}))
        }
        Arguments::Page {
            match_id,
            sub_id,
            page,
        } => {
            if let Some(sub) = sub_match(runtime, *match_id, *sub_id)? {
                let pagination: [i32; 2] = field(api, sub, "RankPage")?;
                // Only the current page is known to be loaded: a UI caller can
                // jump pages, so a later page does not prove earlier coverage.
                if pagination[0] == *page {
                    return page_replays(api, sub, [*page, pagination[1]]);
                }
            }
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
            let list = history(runtime)?;
            if field::<i32>(runtime.api, list, "_version")? == marker {
                return Ok(json!({"ready": false}));
            }
            list_history(runtime, list)
        }
        Arguments::Page {
            match_id,
            sub_id,
            page,
        } => {
            let Some(sub) = sub_match(runtime, *match_id, *sub_id)? else {
                return Ok(json!({"ready": false}));
            };
            let pagination: [i32; 2] = field(runtime.api, sub, "RankPage")?;
            if pagination[0] != *page {
                return Ok(json!({"ready": false}));
            }
            page_replays(runtime.api, sub, pagination)
        }
        Arguments::Download {
            replay_path,
            output,
        } => download_result(runtime, replay_path, output),
    }
}

fn list_history(runtime: &Runtime, list: *mut Object) -> Result<Value> {
    let api = runtime.api;
    let mut entries = Vec::new();
    for info in objects(api, list)? {
        let config = api
            .invoke(info, "get_Config", &mut [])
            .map_err(|error| error.to_string())?;
        let subs: *mut Object = field(api, info, "SubIds")?;
        let sub_ids = api
            .value_array::<i32>(subs, 16384)
            .map_err(|error| error.to_string())?;
        let config_text = |name| {
            api.invoke(config, name, &mut [])
                .map_err(|error| error.to_string())
                .and_then(|object| text(api, object))
        };
        let params: Vec<_> = ["get_param0", "get_param1", "get_param2"]
            .into_iter()
            .map(config_text)
            .collect::<Result<_>>()?;
        entries.push(json!({
            "match_id": field::<i32>(api, info, "Id")?, "sub_ids": sub_ids,
            "start_time": field::<i32>(api, info, "StartTime")?,
            "title": config_text("GetTitle")?, "params": params,
            "division_type": api.invoke_value::<i32>(config, "get_division_type", &mut []).map_err(|error| error.to_string())?,
        }));
    }
    Ok(json!({"ready": true, "tournaments": entries}))
}

fn page_replays(api: Api, sub: *mut Object, pagination: [i32; 2]) -> Result<Value> {
    let mut replays = std::collections::BTreeSet::new();
    let ranks = field::<*mut Object>(api, sub, "RankList")?;
    let rows = objects(api, ranks)?;
    for rank in &rows {
        let rounds = field::<*mut Object>(api, *rank, "RoundRecords")?;
        for round in objects(api, rounds)? {
            let path = text(api, field(api, round, "ReplayPath")?)?;
            if !path.is_empty() {
                replays.insert(path);
            }
        }
    }
    Ok(
        json!({"ready": true, "match_id": field::<i32>(api, sub, "ID")?,
        "sub_id": field::<i32>(api, sub, "Subid")?, "page": pagination[0], "pages": pagination[1],
        "rank_count": rows.len(), "replays": replays}),
    )
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
