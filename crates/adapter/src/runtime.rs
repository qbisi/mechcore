use crate::capture::{self, CaptureMessage};
use crate::il2cpp::{Api, Class, Error as Il2CppError, FieldInfo, Object};
use crate::layout::{self, Plan};
use crate::operations;
use mechcore_protocol::{Busy, Hello, MAX_ACTIVATION_ROUND, Operation, Request, Response};
use serde::Deserialize;
use serde_json::Value;
use std::env;
use std::ffi::{CString, c_void};
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};

const SOCKET_ENV: &str = "MECHCORE_ADAPTER_SOCKET";
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
const LAYOUT_SERIES_TIMEOUT: Duration = Duration::from_secs(55);
const LAYOUT_STATUS_INTERVAL: Duration = Duration::from_millis(50);
const LAYOUT_DEPLOYMENT_STABLE_SAMPLES: usize = 3;
const RECORDING_TIMEOUT: Duration = Duration::from_secs(175);
const RECORDING_POLL_INTERVAL: Duration = Duration::from_millis(5);
const REPLAY_LOAD_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordBattleArguments {
    output: PathBuf,
    #[serde(default)]
    video_output: Option<PathBuf>,
    /// Request native combat speed-up. Defaults to true, with or without a
    /// visual recording.
    #[serde(default)]
    speed_up: Option<bool>,
    #[serde(default)]
    instrumentation: Option<RecordBattleInstrumentationArguments>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordReplayRoundArguments {
    grbr: PathBuf,
    round: i32,
    output: PathBuf,
    #[serde(default)]
    speed_up: Option<bool>,
    instrumentation: Option<RecordBattleInstrumentationArguments>,
}

#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct RecordBattleInstrumentationArguments {
    output: PathBuf,
    profile: capture::CaptureInstrumentationProfile,
    rvo_scope: Option<capture::RvoCaptureScope>,
}

fn validate_instrumentation_arguments(
    instrumentation: Option<&RecordBattleInstrumentationArguments>,
    output: &Path,
    video_output: Option<&Path>,
) -> Result<(), String> {
    let Some(instrumentation) = instrumentation else {
        return Ok(());
    };
    if let Some(scope) = &instrumentation.rvo_scope {
        scope.validate(instrumentation.profile)?;
    }
    if !instrumentation.output.is_absolute()
        || instrumentation
            .output
            .extension()
            .and_then(|value| value.to_str())
            != Some("h5")
        || instrumentation.output.exists()
        || instrumentation.output == output
        || video_output == Some(instrumentation.output.as_path())
    {
        return Err("instrumentation output must be a new, distinct absolute .h5 path".into());
    }
    Ok(())
}

#[derive(Debug)]
pub enum RuntimeError {
    Il2Cpp(Il2CppError),
    Io(io::Error),
    Configuration(String),
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Il2Cpp(error) => error.fmt(f),
            Self::Io(error) => error.fmt(f),
            Self::Configuration(error) => f.write_str(error),
        }
    }
}

impl From<Il2CppError> for RuntimeError {
    fn from(value: Il2CppError) -> Self {
        Self::Il2Cpp(value)
    }
}

impl From<io::Error> for RuntimeError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

pub struct Runtime {
    pub api: Api,
    pub match_current: *mut FieldInfo,
    pub fight_current: *mut FieldInfo,
    pub scene_manager_class: *mut Class,
    pub scene_class: *mut Class,
}

impl Runtime {
    fn load(api: Api) -> Result<Self, RuntimeError> {
        let match_class = api.class("GRClient.dll", "GameRiver.Client", "MatchClient")?;
        let match_current = api.field(match_class, "<Current>k__BackingField")?;
        let fight_class = api.class("GRFight.dll", "GameRiver.Fight", "FightController")?;
        let fight_current = api.field(fight_class, "<Current>k__BackingField")?;
        let scene_manager_class = api.class(
            "UnityEngine.CoreModule.dll",
            "UnityEngine.SceneManagement",
            "SceneManager",
        )?;
        let scene_class = api.class(
            "UnityEngine.CoreModule.dll",
            "UnityEngine.SceneManagement",
            "Scene",
        )?;
        Ok(Self {
            api,
            match_current,
            fight_current,
            scene_manager_class,
            scene_class,
        })
    }

    pub fn current_match(&self) -> *mut Object {
        self.api.static_object(self.match_current)
    }

    pub fn current_fight(&self) -> *mut Object {
        self.api.static_object(self.fight_current)
    }

    pub fn active_scene_name(&self) -> Result<String, Il2CppError> {
        let scene = self
            .api
            .invoke_static(self.scene_manager_class, "GetActiveScene", &mut [])?;
        let this = self.api.unboxed_this(scene)?;
        let method = self.api.method(self.scene_class, "get_name", 0)?;
        let name = self.api.invoke_raw(method, this, &mut [])?;
        self.api.string_to_rust(name.cast())
    }
}

struct MainInvocation {
    runtime: *mut Runtime,
    request: *const Request,
    request_id: u64,
    layout: *const Plan,
    action: MainAction,
    response: Option<Response<Value>>,
}

#[derive(Clone)]
enum MainAction {
    Public,
    Internal(operations::InternalOperation),
    Layout(operations::LayoutExecutionStage),
}

struct RuntimeLoadInvocation {
    api: Api,
    result: Option<Result<Box<Runtime>, RuntimeError>>,
}

struct ReplayLoadInvocation {
    runtime: *mut Runtime,
    request_id: u64,
    path: *const PathBuf,
    native_start_round: i32,
    response: Option<Response<Value>>,
}

extern "C" fn load_runtime_on_main(context: *mut c_void) {
    // SAFETY: dispatch_sync_f invokes this callback before returning, while the
    // stack-owned invocation remains alive.
    let invocation = unsafe { &mut *context.cast::<RuntimeLoadInvocation>() };
    invocation.result = Some(Runtime::load(invocation.api).map(|runtime| {
        let mut runtime = Box::new(runtime);
        capture::initialize(&mut runtime);
        runtime
    }));
}

extern "C" fn load_replay_on_main(context: *mut c_void) {
    // SAFETY: dispatch_sync_f invokes this callback before returning, while the
    // stack-owned invocation, runtime and path remain alive.
    let invocation = unsafe { &mut *context.cast::<ReplayLoadInvocation>() };
    let runtime = unsafe { &mut *invocation.runtime };
    let path = unsafe { &*invocation.path };
    invocation.response = Some(operations::load_replay(
        runtime,
        invocation.request_id,
        path,
        invocation.native_start_round,
    ));
}

extern "C" fn invoke_on_main(context: *mut c_void) {
    // SAFETY: dispatch_sync_f invokes this callback before returning, while the
    // stack-owned invocation, runtime and request remain alive.
    let invocation = unsafe { &mut *context.cast::<MainInvocation>() };
    // SAFETY: pointers are supplied by execute_action_on_main and remain valid
    // for the synchronous callback duration of the action that uses them.
    let runtime = unsafe { &mut *invocation.runtime };
    invocation.response = Some(match invocation.action.clone() {
        MainAction::Public => {
            // SAFETY: public actions always carry their live wire request.
            let request = unsafe { &*invocation.request };
            operations::execute(runtime, request)
        }
        MainAction::Internal(operation) => {
            operations::execute_internal(runtime, invocation.request_id, operation)
        }
        MainAction::Layout(stage) => {
            // SAFETY: layout actions are dispatched synchronously while the plan
            // passed by execute_layout_stage_on_main remains alive.
            let plan = unsafe { &*invocation.layout };
            operations::execute_layout_stage(runtime, invocation.request_id, plan, stage)
        }
    });
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    static _dispatch_main_q: c_void;
    fn dispatch_sync_f(queue: *mut c_void, context: *mut c_void, work: extern "C" fn(*mut c_void));
}

fn execute_on_main(runtime: &mut Runtime, request: &Request) -> Response<Value> {
    execute_action_on_main(runtime, Some(request), request.id, None, MainAction::Public)
}

fn execute_internal_on_main(
    runtime: &mut Runtime,
    request_id: u64,
    operation: operations::InternalOperation,
) -> Response<Value> {
    execute_action_on_main(
        runtime,
        None,
        request_id,
        None,
        MainAction::Internal(operation),
    )
}

fn execute_layout_stage_on_main(
    runtime: &mut Runtime,
    request_id: u64,
    plan: &Plan,
    stage: operations::LayoutExecutionStage,
) -> Response<Value> {
    execute_action_on_main(
        runtime,
        None,
        request_id,
        Some(plan),
        MainAction::Layout(stage),
    )
}

fn execute_replay_load_on_main(
    runtime: &mut Runtime,
    request_id: u64,
    path: &PathBuf,
    native_start_round: i32,
) -> Response<Value> {
    let mut invocation = ReplayLoadInvocation {
        runtime,
        request_id,
        path,
        native_start_round,
        response: None,
    };
    #[cfg(target_os = "macos")]
    {
        // SAFETY: queue is the process main queue and the callback/context obey
        // dispatch_sync_f's synchronous lifetime contract.
        unsafe {
            dispatch_sync_f(
                (&raw const _dispatch_main_q).cast_mut(),
                std::ptr::from_mut(&mut invocation).cast(),
                load_replay_on_main,
            );
        }
    }
    #[cfg(not(target_os = "macos"))]
    load_replay_on_main(std::ptr::from_mut(&mut invocation).cast());

    invocation.response.unwrap_or_else(|| {
        Response::failure(
            request_id,
            "main_thread_dispatch_failed",
            "main-thread replay load returned no response",
        )
    })
}

fn execute_action_on_main(
    runtime: &mut Runtime,
    request: Option<&Request>,
    request_id: u64,
    layout: Option<&Plan>,
    action: MainAction,
) -> Response<Value> {
    let mut invocation = MainInvocation {
        runtime,
        request: request.map_or(std::ptr::null(), std::ptr::from_ref),
        request_id,
        layout: layout.map_or(std::ptr::null(), std::ptr::from_ref),
        action,
        response: None,
    };
    #[cfg(target_os = "macos")]
    {
        // SAFETY: queue is the process main queue and the callback/context obey
        // dispatch_sync_f's synchronous lifetime contract.
        unsafe {
            dispatch_sync_f(
                (&raw const _dispatch_main_q).cast_mut(),
                std::ptr::from_mut(&mut invocation).cast(),
                invoke_on_main,
            );
        }
    }
    #[cfg(not(target_os = "macos"))]
    invoke_on_main(std::ptr::from_mut(&mut invocation).cast());

    invocation.response.unwrap_or_else(|| {
        Response::failure(
            request_id,
            "main_thread_dispatch_failed",
            "main-thread callback returned no response",
        )
    })
}

fn load_runtime_on_main_thread(api: Api) -> Result<Box<Runtime>, RuntimeError> {
    let mut invocation = RuntimeLoadInvocation { api, result: None };
    #[cfg(target_os = "macos")]
    {
        // SAFETY: the callback and context obey dispatch_sync_f's synchronous
        // lifetime contract. The main queue cannot execute this callback until
        // the main-thread IL2CPP initialization that exposed the symbols returns.
        unsafe {
            dispatch_sync_f(
                (&raw const _dispatch_main_q).cast_mut(),
                std::ptr::from_mut(&mut invocation).cast(),
                load_runtime_on_main,
            );
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _thread = api.attach()?;
        invocation.result = Some(Runtime::load(api).map(Box::new));
    }

    invocation.result.unwrap_or_else(|| {
        Err(RuntimeError::Configuration(
            "main-thread runtime initialization returned no result".into(),
        ))
    })
}

pub fn worker() {
    if let Err(error) = run() {
        eprintln!("mechcore-adapter: {error}");
    }
}

fn run() -> Result<(), RuntimeError> {
    let api = loop {
        // SAFETY: failure is handled and retried until GameAssembly has loaded.
        match unsafe { Api::load() } {
            Ok(api) => break api,
            Err(Il2CppError::MissingExport(_)) => thread::sleep(Duration::from_millis(100)),
            Err(error) => return Err(error.into()),
        }
    };
    let mut runtime = load_runtime_on_main_thread(api)?;
    let endpoint = socket_path()?;
    let listener = bind_listener(&endpoint)?;
    arm_endpoint_cleanup(&endpoint);
    let _cleanup = SocketCleanup(endpoint);

    let serving = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = mpsc::sync_channel::<UnixStream>(0);
    thread::spawn({
        let serving = Arc::clone(&serving);
        move || greet_clients(&listener, &serving, &sender)
    });

    for stream in receiver {
        let result = serve_client(&mut runtime, stream);
        serving.store(false, Ordering::SeqCst);
        if let Err(error) = result {
            eprintln!("mechcore-adapter: client disconnected: {error}");
        }
    }
    Ok(())
}

/// Accepts connections and hands the single serving slot to the worker loop.
///
/// The worker serves one client at a time, so a connection arriving while the
/// slot is taken would otherwise wait in the backlog and look identical to an
/// unresponsive adapter. Answering `busy` here keeps occupancy a protocol fact.
fn greet_clients(
    listener: &UnixListener,
    serving: &AtomicBool,
    sender: &mpsc::SyncSender<UnixStream>,
) {
    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(stream) => stream,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                eprintln!("mechcore-adapter: accept failed: {error}");
                return;
            }
        };
        if let Err(error) = verify_peer(&stream) {
            eprintln!("mechcore-adapter: rejected peer: {error}");
            continue;
        }
        if serving.swap(true, Ordering::SeqCst) {
            if let Err(error) = write_json_line(&mut stream, &Busy::current()) {
                eprintln!("mechcore-adapter: cannot answer busy: {error}");
            }
            continue;
        }
        if sender.send(stream).is_err() {
            serving.store(false, Ordering::SeqCst);
            return;
        }
    }
}

fn socket_path() -> Result<PathBuf, RuntimeError> {
    let configured = env::var_os(SOCKET_ENV).map(PathBuf::from);
    // SAFETY: geteuid has no preconditions.
    let uid = unsafe { libc::geteuid() };
    let path =
        configured.unwrap_or_else(|| PathBuf::from(format!("/tmp/mechcore-adapter-{uid}.sock")));
    if !path.is_absolute() {
        return Err(RuntimeError::Configuration(format!(
            "{SOCKET_ENV} must be absolute"
        )));
    }
    if path.as_os_str().as_encoded_bytes().len() > 100 {
        return Err(RuntimeError::Configuration(
            "Unix socket path exceeds 100 bytes".into(),
        ));
    }
    Ok(path)
}

fn bind_listener(path: &Path) -> Result<UnixListener, RuntimeError> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if !metadata.file_type().is_socket() {
            return Err(RuntimeError::Configuration(format!(
                "refusing to replace non-socket endpoint {}",
                path.display()
            )));
        }
        // SAFETY: geteuid has no preconditions.
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err(RuntimeError::Configuration(format!(
                "refusing to replace socket owned by another uid: {}",
                path.display()
            )));
        }
        if UnixStream::connect(path).is_ok() {
            return Err(RuntimeError::Configuration(format!(
                "adapter endpoint is already live: {}",
                path.display()
            )));
        }
        fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

fn serve_client(runtime: &mut Runtime, mut stream: UnixStream) -> io::Result<()> {
    verify_peer(&stream)?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let hello = Hello::current();
    write_json_line(&mut stream, &hello)?;
    let mut reader = BufReader::new(stream.try_clone()?);
    loop {
        let mut bytes = Vec::new();
        let read = reader
            .by_ref()
            .take((MAX_MESSAGE_BYTES + 1) as u64)
            .read_until(b'\n', &mut bytes)?;
        if read == 0 {
            return Ok(());
        }
        if bytes.len() > MAX_MESSAGE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request exceeds size limit",
            ));
        }
        let request: Request = match serde_json::from_slice(&bytes) {
            Ok(request) => request,
            Err(error) => {
                let response: Response<Value> =
                    Response::failure(0, "invalid_request", error.to_string());
                write_json_line(&mut stream, &response)?;
                continue;
            }
        };
        let response = match request.operation {
            Operation::ApplyLayout => execute_layout_series(runtime, &request),
            Operation::RecordBattle => execute_recording_series(
                runtime,
                &request,
                capture::CaptureStartMode::TrainingGround,
            ),
            Operation::RecordReplayRound => execute_replay_recording_series(runtime, &request),
            _ => execute_on_main(runtime, &request),
        };
        write_json_line(&mut stream, &response)?;
    }
}

fn execute_replay_recording_series(runtime: &mut Runtime, request: &Request) -> Response<Value> {
    let arguments: RecordReplayRoundArguments =
        match serde_json::from_value(request.arguments.clone()) {
            Ok(arguments) => arguments,
            Err(error) => {
                return Response::failure(request.id, "invalid_arguments", error.to_string());
            }
        };
    if let Err(error) = validate_instrumentation_arguments(
        arguments.instrumentation.as_ref(),
        &arguments.output,
        None,
    ) {
        return Response::failure(request.id, "invalid_arguments", error);
    }
    if !arguments.grbr.is_absolute() {
        return Response::failure(
            request.id,
            "invalid_arguments",
            "record_replay_round grbr must be absolute",
        );
    }
    if arguments.grbr.extension().and_then(|value| value.to_str()) != Some("grbr") {
        return Response::failure(
            request.id,
            "invalid_arguments",
            "record_replay_round input must use the .grbr extension",
        );
    }
    if !arguments.grbr.is_file() {
        return Response::failure(
            request.id,
            "invalid_arguments",
            format!("replay file does not exist: {}", arguments.grbr.display()),
        );
    }
    if !(1..=MAX_ACTIVATION_ROUND).contains(&arguments.round) {
        return Response::failure(
            request.id,
            "invalid_arguments",
            format!("record_replay_round round must be between 1 and {MAX_ACTIVATION_ROUND}"),
        );
    }
    if !arguments.output.is_absolute()
        || arguments
            .output
            .extension()
            .and_then(|value| value.to_str())
            != Some("mcfr")
        || arguments.output.exists()
    {
        return Response::failure(
            request.id,
            "invalid_arguments",
            "record_replay_round output must be a new absolute .mcfr path",
        );
    }
    let before = match successful_result(execute_internal_on_main(
        runtime,
        request.id,
        operations::InternalOperation::Status,
    )) {
        Ok(status) => status,
        Err(response) => return response,
    };
    if before.get("status").and_then(Value::as_str) != Some("main_menu") {
        return Response::failure(
            request.id,
            "invalid_game_state",
            format!("record_replay_round requires main_menu: {before}"),
        );
    }

    let native_start_round = arguments.round;
    if let Err(response) = successful_result(execute_replay_load_on_main(
        runtime,
        request.id,
        &arguments.grbr,
        native_start_round,
    )) {
        return replay_failure_after_cleanup(runtime, request.id, response);
    }
    let load_deadline = Instant::now() + REPLAY_LOAD_TIMEOUT;
    if let Err(response) = wait_layout_status(
        runtime,
        request.id,
        load_deadline,
        &format!("replay round {} deployment", arguments.round),
        LAYOUT_DEPLOYMENT_STABLE_SAMPLES,
        |status| is_replay_state(status, arguments.round, true, false),
    ) {
        return replay_failure_after_cleanup(runtime, request.id, response);
    }

    let capture_request = Request {
        id: request.id,
        operation: Operation::RecordBattle,
        arguments: serde_json::json!({
            "output": arguments.output,
            "speed_up": arguments.speed_up,
            "instrumentation": arguments.instrumentation,
        }),
    };
    let mut capture_response =
        execute_recording_series(runtime, &capture_request, capture::CaptureStartMode::Replay);
    if !capture_response.ok {
        return replay_failure_after_cleanup(runtime, request.id, capture_response);
    }
    let cleanup = match finish_replay_to_main_menu(runtime, request.id) {
        Ok(status) => status,
        Err(response) => {
            let output = arguments.output.display();
            let message = response
                .error
                .as_ref()
                .map(|error| error.message.as_str())
                .unwrap_or("unknown cleanup error");
            return Response::failure(
                request.id,
                "replay_cleanup_failed",
                format!("recorded {output}, but could not return replay to main_menu: {message}"),
            );
        }
    };
    let mut result = capture_response
        .result
        .take()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    result.insert("grbr".into(), serde_json::json!(arguments.grbr));
    result.insert("round".into(), serde_json::json!(arguments.round));
    result.insert(
        "fast_deployment".into(),
        serde_json::json!({"is_real_time": false, "step_time": 0.0}),
    );
    result.insert("cleanup".into(), serde_json::json!({"match_exited": true}));
    result.insert("status".into(), cleanup);
    Response::success(request.id, Value::Object(result))
}

fn replay_failure_after_cleanup(
    runtime: &mut Runtime,
    request_id: u64,
    response: Response<Value>,
) -> Response<Value> {
    let code = response
        .error
        .as_ref()
        .map(|error| error.code.clone())
        .unwrap_or_else(|| "record_replay_round_failed".into());
    let message = response
        .error
        .as_ref()
        .map(|error| error.message.clone())
        .unwrap_or_else(|| "record_replay_round failed without an error body".into());
    match finish_replay_to_main_menu(runtime, request_id) {
        Ok(_) => Response::failure(request_id, code, message),
        Err(cleanup) => {
            let cleanup = cleanup
                .error
                .as_ref()
                .map(|error| error.message.as_str())
                .unwrap_or("unknown cleanup error");
            Response::failure(
                request_id,
                code,
                format!("{message}; replay cleanup also failed: {cleanup}"),
            )
        }
    }
}

fn finish_replay_to_main_menu(
    runtime: &mut Runtime,
    request_id: u64,
) -> Result<Value, Response<Value>> {
    let status = successful_result(execute_internal_on_main(
        runtime,
        request_id,
        operations::InternalOperation::Status,
    ))?;
    if status.get("status").and_then(Value::as_str) == Some("main_menu") {
        return Ok(status);
    }
    if status.get("status").and_then(Value::as_str) != Some("replay") {
        return Err(Response::failure(
            request_id,
            "replay_cleanup_failed",
            format!("cannot exit replay from status {status}"),
        ));
    }
    let quit = Request {
        id: request_id,
        operation: Operation::QuitMatch,
        arguments: serde_json::json!({}),
    };
    successful_result(execute_on_main(runtime, &quit))?;
    wait_layout_status(
        runtime,
        request_id,
        Instant::now() + REPLAY_LOAD_TIMEOUT,
        "main_menu after replay exit",
        LAYOUT_DEPLOYMENT_STABLE_SAMPLES,
        |value| value.get("status").and_then(Value::as_str) == Some("main_menu"),
    )
}

#[allow(clippy::too_many_lines)]
fn execute_recording_series(
    runtime: &mut Runtime,
    request: &Request,
    mode: capture::CaptureStartMode,
) -> Response<Value> {
    let arguments: RecordBattleArguments = match serde_json::from_value(request.arguments.clone()) {
        Ok(arguments) => arguments,
        Err(error) => return Response::failure(request.id, "invalid_arguments", error.to_string()),
    };
    if !arguments.output.is_absolute() {
        return Response::failure(
            request.id,
            "invalid_arguments",
            "record_battle output must be absolute",
        );
    }
    if arguments
        .output
        .extension()
        .and_then(|value| value.to_str())
        != Some("mcfr")
    {
        return Response::failure(
            request.id,
            "invalid_arguments",
            "record_battle output must use the .mcfr extension",
        );
    }
    if arguments.output.exists() {
        return Response::failure(
            request.id,
            "invalid_arguments",
            format!("refusing to overwrite {}", arguments.output.display()),
        );
    }
    if let Some(video_output) = &arguments.video_output {
        if !video_output.is_absolute() {
            return Response::failure(
                request.id,
                "invalid_arguments",
                "record_battle video_output must be absolute",
            );
        }
        if video_output.extension().and_then(|value| value.to_str()) != Some("mov") {
            return Response::failure(
                request.id,
                "invalid_arguments",
                "record_battle video_output must use the .mov extension",
            );
        }
        if video_output == &arguments.output {
            return Response::failure(
                request.id,
                "invalid_arguments",
                "record_battle output and video_output must differ",
            );
        }
        if video_output.exists() {
            return Response::failure(
                request.id,
                "invalid_arguments",
                format!("refusing to overwrite {}", video_output.display()),
            );
        }
    }
    if let Err(error) = validate_instrumentation_arguments(
        arguments.instrumentation.as_ref(),
        &arguments.output,
        arguments.video_output.as_deref(),
    ) {
        return Response::failure(request.id, "invalid_arguments", error);
    }
    let visual = arguments.video_output.is_some();
    // Speed-up and video coexist. The render barrier holds the logic update
    // until the frame is captured, so a sped-up game cannot outrun the encoder
    // and cannot drop a frame; measured runs produce the same frame count and a
    // bit-identical MCFR. Its benefit is small under video because the barrier,
    // not the simulation rate, sets the pace.
    let speed_up = arguments.speed_up.unwrap_or(true);
    if let Err(response) = successful_result(execute_internal_on_main(
        runtime,
        request.id,
        operations::InternalOperation::StartCapture {
            mode,
            visual,
            speed_up,
            instrumentation_profile: arguments
                .instrumentation
                .as_ref()
                .map(|value| value.profile),
            rvo_scope: arguments
                .instrumentation
                .as_ref()
                .and_then(|value| value.rvo_scope.clone()),
        },
    )) {
        return response;
    }
    if mode == capture::CaptureStartMode::Replay
        && let Err(response) = successful_result(execute_internal_on_main(
            runtime,
            request.id,
            operations::InternalOperation::ReplayFastDeployment,
        ))
    {
        stop_capture_after_failure(runtime, request.id);
        return response;
    }
    let deadline = Instant::now() + RECORDING_TIMEOUT;
    let mut writer = None;
    let mut video = None;
    let mut instrumentation_records = Vec::new();
    let mut recorded_tick = 0_u64;
    loop {
        if Instant::now() >= deadline {
            return recording_failure(
                runtime,
                request.id,
                "operation_timeout",
                "recording timed out before fighting-to-over boundary".into(),
            );
        }
        match capture::poll() {
            Some(CaptureMessage::Initial {
                game_build,
                context,
                layout_yaml,
            }) => {
                if writer.is_some() {
                    return recording_failure(
                        runtime,
                        request.id,
                        "capture_failed",
                        "capture emitted more than one recording header".into(),
                    );
                }
                if let Some(video_output) = arguments.video_output.as_ref() {
                    let created =
                        match crate::video::MovWriter::create(video_output, context.logic_step) {
                            Ok(created) => created,
                            Err(error) => {
                                return recording_failure(
                                    runtime,
                                    request.id,
                                    "video_error",
                                    error,
                                );
                            }
                        };
                    video = Some(created);
                }
                match mechcore_mcfr::McfrWriter::create(
                    &arguments.output,
                    &game_build,
                    &context,
                    &layout_yaml,
                ) {
                    Ok(created) => writer = Some(created),
                    Err(error) => {
                        return recording_failure(
                            runtime,
                            request.id,
                            "mcfr_error",
                            error.to_string(),
                        );
                    }
                }
            }
            Some(CaptureMessage::Transition {
                events,
                state,
                instrumentation,
                terminal,
                frame,
            }) => {
                let Some(active) = writer.as_mut() else {
                    return recording_failure(
                        runtime,
                        request.id,
                        "capture_failed",
                        "capture tick preceded its recording header".into(),
                    );
                };
                if let Err(error) = active.append_tick(state, &events) {
                    return recording_failure(runtime, request.id, "mcfr_error", error.to_string());
                }
                recorded_tick += 1;
                match (&arguments.instrumentation, instrumentation) {
                    (Some(_), Some(observation)) => {
                        instrumentation_records.push((recorded_tick, observation))
                    }
                    (None, None) => {}
                    (Some(config), None) if config.rvo_scope.is_some() => {}
                    (Some(_), None) => {
                        return recording_failure(
                            runtime,
                            request.id,
                            "capture_failed",
                            "capture omitted requested instrumentation".into(),
                        );
                    }
                    (None, Some(_)) => {
                        return recording_failure(
                            runtime,
                            request.id,
                            "capture_failed",
                            "capture produced unrequested instrumentation".into(),
                        );
                    }
                }
                match (video.as_mut(), frame.as_ref()) {
                    (Some(active), Some(frame)) => {
                        // Encoding runs here, on the socket thread, so the game's
                        // main thread is free to render the next logic frame.
                        let jpeg = match frame.encode_jpeg() {
                            Ok(jpeg) => jpeg,
                            Err(error) => {
                                return recording_failure(
                                    runtime,
                                    request.id,
                                    "video_error",
                                    error,
                                );
                            }
                        };
                        if let Err(error) = active.append_jpeg(&jpeg) {
                            return recording_failure(runtime, request.id, "video_error", error);
                        }
                    }
                    (Some(_), None) => {
                        return recording_failure(
                            runtime,
                            request.id,
                            "capture_failed",
                            "visual capture omitted a logic frame".into(),
                        );
                    }
                    (None, Some(_)) => {
                        return recording_failure(
                            runtime,
                            request.id,
                            "capture_failed",
                            "visual capture produced a frame without video_output".into(),
                        );
                    }
                    (None, None) => {}
                }
                if terminal {
                    let video_summary = match video.take() {
                        Some(video) => match video.finish() {
                            Ok(summary) => Some(summary),
                            Err(error) => {
                                return recording_failure(
                                    runtime,
                                    request.id,
                                    "video_error",
                                    error,
                                );
                            }
                        },
                        None => None,
                    };
                    let hashes = match writer.take().expect("writer checked above").finish() {
                        Ok(hashes) => hashes,
                        Err(error) => {
                            remove_published(arguments.video_output.as_deref());
                            return Response::failure(request.id, "mcfr_error", error.to_string());
                        }
                    };
                    let published = match mechcore_mcfr::McfrReader::open(&arguments.output) {
                        Ok(reader) => reader,
                        Err(error) => {
                            remove_published(Some(&arguments.output));
                            remove_published(arguments.video_output.as_deref());
                            return Response::failure(
                                request.id,
                                "mcfr_reopen_failed",
                                error.to_string(),
                            );
                        }
                    };
                    if published.hashes() != &hashes {
                        remove_published(Some(&arguments.output));
                        remove_published(arguments.video_output.as_deref());
                        return Response::failure(
                            request.id,
                            "mcfr_reopen_failed",
                            "published MCFR hashes changed after reopening",
                        );
                    }
                    if let Some(summary) = &video_summary
                        && summary.frame_count != u64::from(published.tick_count())
                    {
                        remove_published(Some(&arguments.output));
                        remove_published(arguments.video_output.as_deref());
                        return Response::failure(
                            request.id,
                            "video_verification_failed",
                            format!(
                                "video frame count {} does not match MCFR state count {}",
                                summary.frame_count,
                                u64::from(published.tick_count())
                            ),
                        );
                    }
                    let instrumentation_result = match &arguments.instrumentation {
                        Some(instrumentation) => {
                            if instrumentation_records.is_empty()
                                || (instrumentation.rvo_scope.is_none()
                                    && instrumentation_records.len()
                                        != published.tick_count() as usize)
                            {
                                remove_published(Some(&arguments.output));
                                remove_published(arguments.video_output.as_deref());
                                return Response::failure(
                                    request.id,
                                    "instrumentation_verification_failed",
                                    format!(
                                        "instrumentation record count {} does not match MCFR state count {}",
                                        instrumentation_records.len(),
                                        published.tick_count()
                                    ),
                                );
                            }
                            let write_result = (|| {
                                let mut sidecar = mechcore_mcfr::InstrumentationWriter::create(
                                    &instrumentation.output,
                                    &hashes.physics_result_hash,
                                    instrumentation.profile.as_str(),
                                    "adapter",
                                )?;
                                for (tick, observation) in &instrumentation_records {
                                    sidecar.record_json(
                                        *tick,
                                        instrumentation.profile.channel(),
                                        observation,
                                    )?;
                                }
                                sidecar.finish()
                            })();
                            if let Err(error) = write_result {
                                remove_published(Some(&arguments.output));
                                remove_published(arguments.video_output.as_deref());
                                remove_published(Some(&instrumentation.output));
                                return Response::failure(
                                    request.id,
                                    "instrumentation_error",
                                    error.to_string(),
                                );
                            }
                            let sidecar = match mechcore_mcfr::InstrumentationReader::open(
                                &instrumentation.output,
                            ) {
                                Ok(sidecar) => sidecar,
                                Err(error) => {
                                    remove_published(Some(&arguments.output));
                                    remove_published(arguments.video_output.as_deref());
                                    remove_published(Some(&instrumentation.output));
                                    return Response::failure(
                                        request.id,
                                        "instrumentation_verification_failed",
                                        error.to_string(),
                                    );
                                }
                            };
                            let valid = sidecar.physics_result_hash() == hashes.physics_result_hash
                                && sidecar.profile() == instrumentation.profile.as_str()
                                && sidecar.producer() == "adapter"
                                && sidecar.len() == instrumentation_records.len()
                                && (0..sidecar.len()).all(|index| {
                                    sidecar.entry(index).is_ok_and(|entry| {
                                        entry.step == instrumentation_records[index].0
                                            && entry.channel == instrumentation.profile.channel()
                                            && entry.content_type == "application/json"
                                    })
                                });
                            if !valid {
                                remove_published(Some(&arguments.output));
                                remove_published(arguments.video_output.as_deref());
                                remove_published(Some(&instrumentation.output));
                                return Response::failure(
                                    request.id,
                                    "instrumentation_verification_failed",
                                    "published instrumentation metadata or records changed during verification",
                                );
                            }
                            Some(serde_json::json!({
                                "output": instrumentation.output,
                                "profile": instrumentation.profile.as_str(),
                                "producer": "adapter",
                                "physics_result_hash": sidecar.physics_result_hash(),
                                "record_count": sidecar.len(),
                                "rvo_scope": instrumentation.rvo_scope,
                            }))
                        }
                        None => None,
                    };
                    let video_result = video_summary.map(|summary| {
                        serde_json::json!({
                            "output": arguments.video_output,
                            "format": "quicktime_mjpeg",
                            "frame_count": summary.frame_count,
                            "width": summary.width,
                            "height": summary.height,
                            "view": capture::CALIBRATION_VIEW,
                            "projection": "perspective",
                            "camera_position": [
                                0.0,
                                capture::CALIBRATION_CAMERA_HEIGHT,
                                capture::CALIBRATION_CAMERA_Z,
                            ],
                            "camera_euler_degrees": [
                                capture::CALIBRATION_CAMERA_PITCH_DEGREES,
                                0.0,
                                0.0,
                            ],
                            "field_of_view_degrees": capture::CALIBRATION_FIELD_OF_VIEW_DEGREES,
                        })
                    });
                    return Response::success(
                        request.id,
                        serde_json::json!({
                            "recorded": true,
                            "output": arguments.output,
                            "tick_count": published.tick_count(),
                            "terminal_tick": published.terminal_tick(),
                            "hashes": hashes,
                            "video": video_result,
                            "instrumentation": instrumentation_result,
                        }),
                    );
                }
            }
            Some(CaptureMessage::Failure(error)) => {
                return recording_failure(runtime, request.id, "capture_failed", error);
            }
            None => thread::sleep(RECORDING_POLL_INTERVAL),
        }
    }
}

fn recording_failure(
    runtime: &mut Runtime,
    request_id: u64,
    code: &str,
    message: String,
) -> Response<Value> {
    capture::abort(&message);
    stop_capture_after_failure(runtime, request_id);
    Response::failure(request_id, code, message)
}

fn stop_capture_after_failure(runtime: &mut Runtime, request_id: u64) {
    let response = execute_internal_on_main(
        runtime,
        request_id,
        operations::InternalOperation::StopCapture,
    );
    if !response.ok {
        eprintln!(
            "mechcore-adapter: failed to restore capture state: {:?}",
            response.error
        );
    }
}

fn remove_published(path: Option<&Path>) {
    if let Some(path) = path
        && let Err(error) = fs::remove_file(path)
    {
        eprintln!(
            "mechcore-adapter: cannot remove failed recording artifact {}: {error}",
            path.display()
        );
    }
}

fn execute_layout_series(runtime: &mut Runtime, request: &Request) -> Response<Value> {
    let plan = match layout::compile(&request.arguments) {
        Ok(plan) => plan,
        Err(error) => return Response::failure(request.id, "invalid_arguments", error),
    };
    let deadline = Instant::now() + LAYOUT_SERIES_TIMEOUT;
    let prepare = execute_layout_stage_on_main(
        runtime,
        request.id,
        &plan,
        operations::LayoutExecutionStage::Prepare,
    );
    let prepare = match successful_result(prepare) {
        Ok(result) => result,
        Err(response) => return response,
    };
    let target_round = plan.round;
    let formation_count = plan.formation_count();
    let construction_count = plan.construction_count();
    let contraption_count = plan.contraption_count();

    let mut current_round = 1_i32;
    let mut skipped_rounds = Vec::new();
    let mut stages = vec![prepare];
    while current_round < target_round {
        if let Err(response) =
            advance_layout_round(runtime, request.id, current_round, false, deadline)
        {
            return response;
        }
        skipped_rounds.push(current_round);
        current_round += 1;
    }

    let activation = execute_layout_stage_on_main(
        runtime,
        request.id,
        &plan,
        operations::LayoutExecutionStage::Activation,
    );
    match successful_result(activation) {
        Ok(result) => stages.push(result),
        Err(response) => return response,
    }
    if let Err(response) = successful_result(execute_internal_on_main(
        runtime,
        request.id,
        operations::InternalOperation::ResetDeployment(target_round),
    )) {
        return response;
    }
    if let Err(response) = wait_layout_status(
        runtime,
        request.id,
        deadline,
        &format!("round {target_round} reset deployment"),
        LAYOUT_DEPLOYMENT_STABLE_SAMPLES,
        |status| is_training_state(status, target_round, true, false),
    ) {
        return response;
    }
    Response::success(
        request.id,
        serde_json::json!({
            "applied": true,
            "round": target_round,
            "formation_count": formation_count,
            "construction_count": construction_count,
            "contraption_count": contraption_count,
            "skipped_rounds": skipped_rounds,
            "stages": stages,
        }),
    )
}

fn successful_result(response: Response<Value>) -> Result<Value, Response<Value>> {
    if response.ok {
        Ok(response.result.unwrap_or(Value::Null))
    } else {
        Err(response)
    }
}

fn advance_layout_round(
    runtime: &mut Runtime,
    request_id: u64,
    round: i32,
    finish_if_fighting: bool,
    deadline: Instant,
) -> Result<(), Response<Value>> {
    successful_result(execute_internal_on_main(
        runtime,
        request_id,
        operations::InternalOperation::ExpireDeployment(round),
    ))?;

    if finish_if_fighting {
        let transition = wait_layout_status(
            runtime,
            request_id,
            deadline,
            &format!("round {round} fight start"),
            1,
            |status| {
                is_training_state(status, round, false, true)
                    || is_training_state(status, round + 1, true, false)
            },
        )?;
        if is_training_state(&transition, round, false, true) {
            successful_result(execute_internal_on_main(
                runtime,
                request_id,
                operations::InternalOperation::FinishPreparation(round),
            ))?;
        }
    }

    wait_layout_status(
        runtime,
        request_id,
        deadline,
        &format!("round {} deployment", round + 1),
        LAYOUT_DEPLOYMENT_STABLE_SAMPLES,
        |status| is_training_state(status, round + 1, true, false),
    )?;
    Ok(())
}

fn wait_layout_status(
    runtime: &mut Runtime,
    request_id: u64,
    deadline: Instant,
    description: &str,
    stable_samples: usize,
    predicate: impl Fn(&Value) -> bool,
) -> Result<Value, Response<Value>> {
    let mut stable = 0;
    let mut last = Value::Null;
    loop {
        if Instant::now() >= deadline {
            return Err(Response::failure(
                request_id,
                "operation_timeout",
                format!("timed out waiting for {description}; last status: {last}"),
            ));
        }
        last = successful_result(execute_internal_on_main(
            runtime,
            request_id,
            operations::InternalOperation::Status,
        ))?;
        if predicate(&last) {
            stable += 1;
            if stable >= stable_samples {
                return Ok(last);
            }
        } else {
            stable = 0;
        }
        thread::sleep(LAYOUT_STATUS_INTERVAL);
    }
}

fn is_training_state(status: &Value, round: i32, deploying: bool, fighting: bool) -> bool {
    status.get("status").and_then(Value::as_str) == Some("training_ground")
        && status.get("round_count").and_then(Value::as_i64) == Some(i64::from(round))
        && status.get("deploying").and_then(Value::as_bool) == Some(deploying)
        && status.get("fighting").and_then(Value::as_bool) == Some(fighting)
}

fn is_replay_state(status: &Value, round: i32, deploying: bool, fighting: bool) -> bool {
    status.get("status").and_then(Value::as_str) == Some("replay")
        && status.get("round_count").and_then(Value::as_i64) == Some(i64::from(round))
        && status.get("deploying").and_then(Value::as_bool) == Some(deploying)
        && status.get("fighting").and_then(Value::as_bool) == Some(fighting)
}

fn write_json_line(stream: &mut UnixStream, value: &impl serde::Serialize) -> io::Result<()> {
    serde_json::to_writer(&mut *stream, value)?;
    stream.write_all(b"\n")?;
    stream.flush()
}

fn verify_peer(stream: &UnixStream) -> io::Result<()> {
    let mut uid = 0;
    let mut gid = 0;
    // SAFETY: fd is live and uid/gid point to writable storage.
    let result = unsafe { libc::getpeereid(stream.as_raw_fd(), &raw mut uid, &raw mut gid) };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: geteuid has no preconditions.
    if uid != unsafe { libc::geteuid() } {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "peer uid does not match adapter uid",
        ));
    }
    Ok(())
}

struct SocketCleanup(PathBuf);

impl Drop for SocketCleanup {
    fn drop(&mut self) {
        // Disarm first: once we remove the endpoint ourselves, the exit handler
        // must not unlink a path another adapter may have rebound since.
        ENDPOINT_ARMED.store(false, Ordering::SeqCst);
        let _ = fs::remove_file(&self.0);
    }
}

/// Endpoint to unlink when the hosting process exits.
///
/// The adapter runs on a detached thread inside the game, so a Unity shutdown
/// tears the process down without unwinding us and `SocketCleanup` never runs.
/// An exit handler makes the endpoint's lifetime equal the process lifetime,
/// which also covers the player closing the window. Deleting it earlier, from
/// `quit_game`, would leave a window where the game is still alive with no
/// endpoint, which a client cannot distinguish from an uninjected game.
static ENDPOINT_PATH: OnceLock<CString> = OnceLock::new();
static ENDPOINT_ARMED: AtomicBool = AtomicBool::new(false);

extern "C" fn remove_endpoint_at_exit() {
    if !ENDPOINT_ARMED.load(Ordering::SeqCst) {
        return;
    }
    if let Some(path) = ENDPOINT_PATH.get() {
        // SAFETY: the stored CString is NUL-terminated and lives for the whole
        // process, so the pointer stays valid during teardown.
        unsafe { libc::unlink(path.as_ptr()) };
    }
}

fn arm_endpoint_cleanup(endpoint: &Path) {
    let Ok(path) = CString::new(endpoint.as_os_str().as_encoded_bytes()) else {
        return;
    };
    let first = ENDPOINT_PATH.set(path).is_ok();
    ENDPOINT_ARMED.store(true, Ordering::SeqCst);
    if first {
        // SAFETY: the handler is a plain extern "C" fn with no arguments and
        // touches only process-lifetime statics.
        unsafe { libc::atexit(remove_endpoint_at_exit) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_instrumentation_scope_is_validated_before_loading() {
        let arguments: RecordReplayRoundArguments = serde_json::from_value(serde_json::json!({
            "grbr": "/tmp/source.grbr", "round": 7, "output": "/tmp/scoped-rvo.mcfr",
            "instrumentation": {"output": "/tmp/scoped-rvo.h5", "profile": "target_refs_rvo_v1",
                "rvo_scope": {"start_tick": 8, "end_tick": 14, "unit_ids": [124, 246]}}
        }))
        .unwrap();
        assert!(
            validate_instrumentation_arguments(
                arguments.instrumentation.as_ref(),
                &arguments.output,
                None
            )
            .is_ok()
        );
        let mut config = arguments.instrumentation.unwrap();
        config.rvo_scope.as_mut().unwrap().unit_ids.clear();
        assert!(
            validate_instrumentation_arguments(Some(&config), &arguments.output, None).is_err()
        );
        config.rvo_scope = None;
        config.output = "relative.h5".into();
        assert!(
            validate_instrumentation_arguments(Some(&config), &arguments.output, None).is_err()
        );
    }

    #[test]
    fn listener_is_private_and_connectable() {
        let path = PathBuf::from(format!(
            "/tmp/mechcore-adapter-test-{}-{}.sock",
            std::process::id(),
            std::thread::current().name().unwrap_or("unnamed")
        ));
        let _ = fs::remove_file(&path);
        let listener = bind_listener(&path).unwrap();
        let metadata = fs::metadata(&path).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        let client = UnixStream::connect(&path).unwrap();
        let (_server, _) = listener.accept().unwrap();
        drop(client);
        drop(listener);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn socket_cleanup_removes_the_endpoint_and_disarms_the_exit_handler() {
        let path = PathBuf::from(format!(
            "/tmp/mechcore-adapter-cleanup-{}.sock",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        let listener = bind_listener(&path).unwrap();
        assert!(path.exists());

        // Arm as the runtime would, then let the RAII guard run.
        ENDPOINT_ARMED.store(true, Ordering::SeqCst);
        drop(SocketCleanup(path.clone()));

        assert!(!path.exists(), "cleanup must remove the endpoint");
        assert!(
            !ENDPOINT_ARMED.load(Ordering::SeqCst),
            "cleanup must disarm the exit handler so it cannot unlink a rebound path"
        );
        drop(listener);
    }

    #[test]
    fn second_client_is_answered_busy_while_the_slot_is_taken() {
        let path = PathBuf::from(format!(
            "/tmp/mechcore-adapter-busy-{}.sock",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        let listener = bind_listener(&path).unwrap();
        let serving = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::sync_channel::<UnixStream>(0);
        let greeter = thread::spawn({
            let serving = Arc::clone(&serving);
            move || greet_clients(&listener, &serving, &sender)
        });

        // The first client takes the single serving slot and holds it.
        let first = UnixStream::connect(&path).unwrap();
        let held = receiver.recv().unwrap();
        assert!(serving.load(Ordering::SeqCst));

        // A second client must be told so, not left waiting in the backlog.
        let second = UnixStream::connect(&path).unwrap();
        let mut line = String::new();
        BufReader::new(&second).read_line(&mut line).unwrap();
        let greeting: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(greeting["kind"], "busy");
        assert_eq!(greeting["protocol"], mechcore_protocol::PROTOCOL);

        // Releasing the slot lets the next client be served normally.
        drop(held);
        serving.store(false, Ordering::SeqCst);
        let third = UnixStream::connect(&path).unwrap();
        let served = receiver.recv().unwrap();
        assert!(serving.load(Ordering::SeqCst));

        drop(first);
        drop(second);
        drop(third);
        drop(served);

        // Free the slot, close the channel, then knock once: the greeter
        // observes the dropped receiver on its next send and returns. Without
        // the knock it would stay blocked in accept forever.
        serving.store(false, Ordering::SeqCst);
        drop(receiver);
        let _ = UnixStream::connect(&path);
        let _ = greeter.join();
        let _ = fs::remove_file(path);
    }
}
