use crate::capture::{self, CaptureMessage, CaptureProfile as _, ValidateRvoScope as _};
use crate::il2cpp::{Api, Class, Error as Il2CppError, FieldInfo, Object};
use crate::layout::{self, Plan};
use crate::operations;
use mechcore_protocol::{
    Busy, Claim, EVICTED_CODE, Evicted, Hello, MAX_LEVEL, MAX_STAGED_ROUND,
    MAX_WATCH_MATCH_TIMEOUT_SECONDS, MAX_WATCH_SCENE_WAIT_SECONDS, Operation, PROTOCOL,
    RecordBattleArguments, RecordBattleInstrumentation, RecordReplayRoundArguments,
    RecordWatchReplayArguments, Refused, Request, Response,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::env;
use std::ffi::{CString, c_void};
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

const SOCKET_ENV: &str = "MECHCORE_ADAPTER_SOCKET";
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
/// How long a new connection has to state its level.
const CLAIM_DEADLINE: Duration = Duration::from_secs(3);
/// How often a served client's socket is checked while it is quiet.
const CLIENT_POLL_INTERVAL: Duration = Duration::from_millis(250);
/// How long a served client may say nothing before it is dropped.
const CLIENT_SILENCE_TIMEOUT: Duration = Duration::from_secs(30);
const LAYOUT_SERIES_TIMEOUT: Duration = Duration::from_secs(55);
const LAYOUT_STATUS_INTERVAL: Duration = Duration::from_millis(50);
const LAYOUT_DEPLOYMENT_STABLE_SAMPLES: usize = 3;
const RECORDING_TIMEOUT: Duration = Duration::from_secs(175);
const RECORDING_POLL_INTERVAL: Duration = Duration::from_millis(5);
const REPLAY_LOAD_TIMEOUT: Duration = Duration::from_secs(60);
const WATCH_LIST_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const WATCH_LIST_SETTLE_TIME: Duration = Duration::from_secs(1);
const WATCH_ENTRY_TIMEOUT: Duration = Duration::from_secs(180);
const WATCH_FINISH_POLL_INTERVAL: Duration = Duration::from_secs(1);
const WATCH_FILE_POLL_INTERVAL: Duration = Duration::from_millis(250);
const WATCH_AUTOSAVE_GRACE: Duration = Duration::from_secs(10);
const WATCH_EXPLICIT_SAVE_TIMEOUT: Duration = Duration::from_secs(30);
const WATCH_REPLAY_STABLE_SAMPLES: usize = 3;

fn validate_instrumentation_arguments(
    instrumentation: Option<&RecordBattleInstrumentation>,
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

struct ReplayFightInvocation {
    runtime: *mut Runtime,
    request_id: u64,
    path: *const PathBuf,
    round: i32,
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

extern "C" fn fight_replay_on_main(context: *mut c_void) {
    // SAFETY: dispatch_sync_f invokes this callback before returning, while the
    // stack-owned invocation, runtime and path remain alive.
    let invocation = unsafe { &mut *context.cast::<ReplayFightInvocation>() };
    let runtime = unsafe { &mut *invocation.runtime };
    let path = unsafe { &*invocation.path };
    invocation.response = Some(operations::fight_replay(
        runtime,
        invocation.request_id,
        path,
        invocation.round,
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

fn execute_replay_fight_on_main(
    runtime: &mut Runtime,
    request_id: u64,
    path: &PathBuf,
    round: i32,
) -> Response<Value> {
    let mut invocation = ReplayFightInvocation {
        runtime,
        request_id,
        path,
        round,
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
                fight_replay_on_main,
            );
        }
    }
    #[cfg(not(target_os = "macos"))]
    fight_replay_on_main(std::ptr::from_mut(&mut invocation).cast());

    invocation.response.unwrap_or_else(|| {
        Response::failure(
            request_id,
            "main_thread_dispatch_failed",
            "main-thread replay fight returned no response",
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
        // An evicted client leaves the game wherever its last operation
        // stopped. The adapter owes the next client a main menu, and only
        // then is the slot free: admitting anyone earlier would hand over a
        // half-finished match.
        if evicting() {
            if let Err(response) = return_to_main_menu(&mut runtime, 0) {
                let detail = response
                    .error
                    .as_ref()
                    .map_or("unknown error", |error| error.message.as_str());
                eprintln!("mechcore-adapter: cannot reach the main menu after eviction: {detail}");
            }
            EVICTING_FOR.store(NO_CLAIM, Ordering::SeqCst);
        }
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
/// unresponsive adapter. Answering here keeps occupancy a protocol fact, and
/// the claim's level is what decides between being refused and taking over.
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
        let level = match read_claim(&mut stream) {
            Ok(level) => level,
            Err(error) => {
                eprintln!("mechcore-adapter: rejected claim: {error}");
                continue;
            }
        };
        if serving.swap(true, Ordering::SeqCst) {
            let holder = HOLDER_LEVEL.load(Ordering::SeqCst);
            // Equal levels do not preempt: two clients that matter the same
            // amount cannot each decide the other should stop.
            let evicting = level > holder;
            if evicting {
                claim_eviction(level);
            }
            if let Err(error) = write_json_line(&mut stream, &Busy::current(holder, evicting)) {
                eprintln!("mechcore-adapter: cannot answer busy: {error}");
            }
            continue;
        }
        HOLDER_LEVEL.store(level, Ordering::SeqCst);
        if sender.send(stream).is_err() {
            serving.store(false, Ordering::SeqCst);
            return;
        }
    }
}

/// Read the claim that opens a connection, and greet the client it admits.
///
/// The level arrives before the greeting because the greeting is the answer to
/// it. A client that says nothing is dropped rather than served: the slot is
/// the scarce thing here, and an unidentified client cannot be ranked.
fn read_claim(stream: &mut UnixStream) -> io::Result<u8> {
    stream.set_read_timeout(Some(CLAIM_DEADLINE))?;
    let mut line = String::new();
    {
        let mut reader = BufReader::new(&*stream).take(MAX_MESSAGE_BYTES as u64);
        if reader.read_line(&mut line)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "client closed before claiming a level",
            ));
        }
    }
    let refuse = |reason: &str| {
        let mut answer = stream.try_clone()?;
        write_json_line(&mut answer, &Refused::current(reason))?;
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            reason.to_owned(),
        ))
    };
    let Ok(claim) = serde_json::from_str::<Claim>(&line) else {
        return refuse("first message must be a claim");
    };
    if claim.kind != "claim" || claim.protocol != PROTOCOL {
        return refuse("claim protocol mismatch");
    }
    if claim.level > MAX_LEVEL {
        return refuse("claim level is above the highest run level");
    }
    Ok(claim.level)
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
    // Short reads rather than one long one: a client that is between steps is
    // still holding the game, and a higher claim must not have to wait for it
    // to speak before the game can be taken back.
    stream.set_read_timeout(Some(CLIENT_POLL_INTERVAL))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let hello = Hello::current();
    write_json_line(&mut stream, &hello)?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut bytes = Vec::new();
    let mut quiet_since = Instant::now();
    loop {
        if evicting() {
            return evict(&mut stream);
        }
        let remaining = (MAX_MESSAGE_BYTES + 1 - bytes.len()) as u64;
        match reader
            .by_ref()
            .take(remaining)
            .read_until(b'\n', &mut bytes)
        {
            Ok(0) => return Ok(()),
            Ok(_) => {}
            Err(error) if would_block(&error) => {
                if quiet_since.elapsed() >= CLIENT_SILENCE_TIMEOUT {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "client said nothing for 30s",
                    ));
                }
                continue;
            }
            Err(error) => return Err(error),
        }
        if bytes.len() > MAX_MESSAGE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request exceeds size limit",
            ));
        }
        if !bytes.ends_with(b"\n") {
            return Ok(());
        }
        quiet_since = Instant::now();
        let parsed = serde_json::from_slice::<Request>(&bytes);
        bytes.clear();
        let request: Request = match parsed {
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
                None,
            ),
            Operation::RecordReplayRound => execute_replay_recording_series(runtime, &request),
            Operation::RecordWatchReplay => execute_watch_replay_series(runtime, &request),
            _ => execute_on_main(runtime, &request),
        };
        write_json_line(&mut stream, &response)?;
        if evicting() {
            return evict(&mut stream);
        }
    }
}

fn would_block(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

/// Tell a client it lost the game, then close the connection.
///
/// The notice is the difference between a taken game and a crashed one: a
/// client that reads it knows the game process is still there and being kept
/// for someone else, so it must not shut it down on its way out.
fn evict(stream: &mut UnixStream) -> io::Result<()> {
    write_json_line(stream, &Evicted::current(evicting_level()))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReplayFileState {
    len: u64,
    modified: SystemTime,
}

/// Own one watched match from main menu back to main menu. Keeping scene
/// selection, native save detection and cleanup in one adapter request prevents
/// another client from interleaving operations with a long unattended capture.
#[allow(clippy::too_many_lines)]
fn execute_watch_replay_series(runtime: &mut Runtime, request: &Request) -> Response<Value> {
    let arguments: RecordWatchReplayArguments =
        match serde_json::from_value(request.arguments.clone()) {
            Ok(arguments) => arguments,
            Err(error) => {
                return Response::failure(request.id, "invalid_arguments", error.to_string());
            }
        };
    if arguments.wait_for_scene_seconds == 0
        || arguments.wait_for_scene_seconds > MAX_WATCH_SCENE_WAIT_SECONDS
    {
        return Response::failure(
            request.id,
            "invalid_arguments",
            format!("wait_for_scene_seconds must be 1..={MAX_WATCH_SCENE_WAIT_SECONDS}"),
        );
    }
    if arguments.match_timeout_seconds < 60
        || arguments.match_timeout_seconds > MAX_WATCH_MATCH_TIMEOUT_SECONDS
    {
        return Response::failure(
            request.id,
            "invalid_arguments",
            format!("match_timeout_seconds must be 60..={MAX_WATCH_MATCH_TIMEOUT_SECONDS}"),
        );
    }
    let output_dir = match arguments.output_dir.as_deref() {
        Some(path) if !path.is_absolute() || !path.is_dir() => {
            return Response::failure(
                request.id,
                "invalid_arguments",
                "record_watch_replay output_dir must be an existing absolute directory",
            );
        }
        Some(path) => match path.canonicalize() {
            Ok(path) => Some(path),
            Err(error) => {
                return Response::failure(
                    request.id,
                    "invalid_arguments",
                    format!("cannot resolve output_dir: {error}"),
                );
            }
        },
        None => None,
    };
    let replay_dir = match native_replay_directory() {
        Ok(path) => path,
        Err(error) => {
            return Response::failure(request.id, "native_replay_directory", error);
        }
    };
    let baseline = match replay_snapshot(&replay_dir) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return Response::failure(
                request.id,
                "native_replay_directory",
                format!("cannot snapshot {}: {error}", replay_dir.display()),
            );
        }
    };
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
            format!("record_watch_replay requires main_menu: {before}"),
        );
    }

    let scene_deadline = Instant::now() + Duration::from_secs(arguments.wait_for_scene_seconds);
    let mut last_selection = Value::Null;
    let scene = loop {
        if evicting() {
            return evicted_response(request.id);
        }
        if Instant::now() >= scene_deadline {
            return watch_failure_after_cleanup(
                runtime,
                request.id,
                "operation_timeout",
                format!(
                    "timed out waiting for an eligible round-one standard 1v1 watch scene; last selection: {last_selection}"
                ),
            );
        }
        if let Err(response) = successful_result(execute_internal_on_main(
            runtime,
            request.id,
            operations::InternalOperation::RefreshWatchScenes,
        )) {
            return watch_response_after_cleanup(runtime, request.id, &response);
        }
        thread::sleep(WATCH_LIST_SETTLE_TIME);
        let selection = match successful_result(execute_internal_on_main(
            runtime,
            request.id,
            operations::InternalOperation::StartEligibleWatch,
        )) {
            Ok(selection) => selection,
            Err(response) => return watch_response_after_cleanup(runtime, request.id, &response),
        };
        last_selection = selection.clone();
        if selection.get("started").and_then(Value::as_bool) == Some(true) {
            let entry = wait_status(
                runtime,
                request.id,
                &StatusWait {
                    deadline: Instant::now() + WATCH_ENTRY_TIMEOUT,
                    interval: LAYOUT_STATUS_INTERVAL,
                    description: "round-one watch match entry",
                    stable_samples: LAYOUT_DEPLOYMENT_STABLE_SAMPLES,
                },
                evicting,
                |status| {
                    status.get("status").and_then(Value::as_str) == Some("spectating")
                        && status.get("round_count").and_then(Value::as_i64) == Some(1)
                        && status.get("fight_ready").and_then(Value::as_bool) == Some(true)
                },
            );
            match entry {
                Ok(_) => break selection,
                Err(response) if evicted(&response) => return response,
                Err(response)
                    if response
                        .error
                        .as_ref()
                        .is_some_and(|error| error.code == "operation_timeout") =>
                {
                    if let Err(cleanup) = return_to_main_menu(runtime, request.id) {
                        let cleanup = cleanup
                            .error
                            .as_ref()
                            .map_or("unknown cleanup error", |error| error.message.as_str());
                        return Response::failure(
                            request.id,
                            "watch_cleanup_failed",
                            format!(
                                "stale watch scene did not enter and cleanup failed: {cleanup}"
                            ),
                        );
                    }
                    continue;
                }
                Err(response) => {
                    return watch_response_after_cleanup(runtime, request.id, &response);
                }
            }
        }
        thread::sleep(WATCH_LIST_REFRESH_INTERVAL.saturating_sub(WATCH_LIST_SETTLE_TIME));
    };

    let finished = wait_status(
        runtime,
        request.id,
        &StatusWait {
            deadline: Instant::now() + Duration::from_secs(arguments.match_timeout_seconds),
            interval: WATCH_FINISH_POLL_INTERVAL,
            description: "watched match finish",
            stable_samples: 1,
        },
        evicting,
        |status| {
            status.get("status").and_then(Value::as_str) == Some("spectating")
                && status.get("finished").and_then(Value::as_bool) == Some(true)
        },
    );
    if let Err(response) = finished {
        // The match is abandoned mid-way and writes no recording. The adapter
        // returns the game to the main menu once this client is gone, so the
        // operation does not clean up on its way out.
        if evicted(&response) {
            return response;
        }
        return watch_response_after_cleanup(runtime, request.id, &response);
    }

    let source = match wait_for_stable_replay(
        &replay_dir,
        &baseline,
        Instant::now() + WATCH_AUTOSAVE_GRACE,
        evicting,
    ) {
        Ok(Some(path)) => path,
        Ok(None) => {
            // Waiting for the file to settle is a wait like any other: a claim
            // ends it, and the game's own recording stays where it wrote it.
            if evicting() {
                return evicted_response(request.id);
            }
            if let Err(response) = successful_result(execute_internal_on_main(
                runtime,
                request.id,
                operations::InternalOperation::SaveCurrentReplay,
            )) {
                return watch_response_after_cleanup(runtime, request.id, &response);
            }
            match wait_for_stable_replay(
                &replay_dir,
                &baseline,
                Instant::now() + WATCH_EXPLICIT_SAVE_TIMEOUT,
                evicting,
            ) {
                Ok(Some(path)) => path,
                Ok(None) if evicting() => return evicted_response(request.id),
                Ok(None) => {
                    return watch_failure_after_cleanup(
                        runtime,
                        request.id,
                        "native_replay_missing",
                        format!(
                            "finished watch produced no stable .grbr in {}",
                            replay_dir.display()
                        ),
                    );
                }
                Err(error) => {
                    return watch_failure_after_cleanup(
                        runtime,
                        request.id,
                        "native_replay_io",
                        error,
                    );
                }
            }
        }
        Err(error) => {
            return watch_failure_after_cleanup(runtime, request.id, "native_replay_io", error);
        }
    };
    let Some(file_name) = source.file_name() else {
        return watch_failure_after_cleanup(
            runtime,
            request.id,
            "native_replay_io",
            format!("native replay has no file name: {}", source.display()),
        );
    };
    let (output, published_copy) = match output_dir.as_deref() {
        Some(directory) if directory != replay_dir => {
            let output = directory.join(file_name);
            if let Err(error) = copy_new_file(&source, &output) {
                return watch_failure_after_cleanup(
                    runtime,
                    request.id,
                    "publish_replay_failed",
                    format!(
                        "cannot publish {} to {}: {error}",
                        source.display(),
                        output.display()
                    ),
                );
            }
            (output, true)
        }
        _ => (source.clone(), false),
    };

    let cleanup = match return_to_main_menu(runtime, request.id) {
        Ok(status) => status,
        Err(response) => {
            let message = response
                .error
                .as_ref()
                .map_or("unknown cleanup error", |error| error.message.as_str());
            return Response::failure(
                request.id,
                "watch_cleanup_failed",
                format!(
                    "published {}, but could not return to main_menu: {message}",
                    output.display()
                ),
            );
        }
    };
    Response::success(
        request.id,
        serde_json::json!({
            "recorded": true,
            "output": output,
            "native_source": source,
            "published_copy": published_copy,
            "scene": scene,
            "cleanup": {"match_exited": true},
            "status": cleanup,
        }),
    )
}

fn native_replay_directory() -> Result<PathBuf, String> {
    let executable =
        env::current_exe().map_err(|error| format!("cannot locate game executable: {error}"))?;
    native_replay_directory_from_executable(&executable)
}

fn native_replay_directory_from_executable(executable: &Path) -> Result<PathBuf, String> {
    let app = executable.ancestors().nth(3).ok_or_else(|| {
        format!(
            "game executable is not inside a macOS app bundle: {}",
            executable.display()
        )
    })?;
    if app.extension().and_then(|value| value.to_str()) != Some("app") {
        return Err(format!(
            "game executable is not inside a macOS app bundle: {}",
            executable.display()
        ));
    }
    app.join("ProjectDatas")
        .join("Replay")
        .canonicalize()
        .map_err(|error| format!("cannot resolve native Replay directory: {error}"))
}

fn replay_snapshot(directory: &Path) -> io::Result<BTreeMap<PathBuf, ReplayFileState>> {
    let mut snapshot = BTreeMap::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("grbr") {
            continue;
        }
        let metadata = entry.metadata()?;
        if metadata.is_file() {
            snapshot.insert(
                path,
                ReplayFileState {
                    len: metadata.len(),
                    modified: metadata.modified()?,
                },
            );
        }
    }
    Ok(snapshot)
}

fn newest_changed_replay(
    baseline: &BTreeMap<PathBuf, ReplayFileState>,
    current: &BTreeMap<PathBuf, ReplayFileState>,
) -> Option<(PathBuf, ReplayFileState)> {
    current
        .iter()
        .filter(|(path, state)| state.len > 0 && baseline.get(*path) != Some(*state))
        .max_by(|(left_path, left), (right_path, right)| {
            left.modified
                .cmp(&right.modified)
                .then_with(|| left_path.cmp(right_path))
        })
        .map(|(path, state)| (path.clone(), state.clone()))
}

fn wait_for_stable_replay(
    directory: &Path,
    baseline: &BTreeMap<PathBuf, ReplayFileState>,
    deadline: Instant,
    stop: impl Fn() -> bool,
) -> Result<Option<PathBuf>, String> {
    let mut last = None;
    let mut stable = 0;
    while Instant::now() < deadline && !stop() {
        let current = replay_snapshot(directory)
            .map_err(|error| format!("cannot read {}: {error}", directory.display()))?;
        // An absent file is not a stable one: waiting out the whole deadline is
        // what gives the game's own autosave its grace period.
        if let Some(candidate) = newest_changed_replay(baseline, &current) {
            if last.as_ref() == Some(&candidate) {
                stable += 1;
            } else {
                last = Some(candidate);
                stable = 1;
            }
            if stable >= WATCH_REPLAY_STABLE_SAMPLES {
                return Ok(last.map(|(path, _)| path));
            }
        } else {
            last = None;
            stable = 0;
        }
        thread::sleep(WATCH_FILE_POLL_INTERVAL);
    }
    Ok(None)
}

fn copy_new_file(source: &Path, destination: &Path) -> io::Result<()> {
    let mut input = fs::File::open(source)?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let result = io::copy(&mut input, &mut output).and_then(|_| output.sync_all());
    if let Err(error) = result {
        drop(output);
        if let Err(cleanup) = fs::remove_file(destination) {
            eprintln!(
                "mechcore-adapter: cannot remove partial watched replay {}: {cleanup}",
                destination.display()
            );
        }
        return Err(error);
    }
    Ok(())
}

/// Whether a failed wait stopped because a higher claim took the game.
fn evicted(response: &Response<Value>) -> bool {
    response
        .error
        .as_ref()
        .is_some_and(|error| error.code == EVICTED_CODE)
}

fn evicted_response(request_id: u64) -> Response<Value> {
    Response::failure(
        request_id,
        EVICTED_CODE,
        format!("level {} claimed the game", evicting_level()),
    )
}

fn watch_response_after_cleanup(
    runtime: &mut Runtime,
    request_id: u64,
    response: &Response<Value>,
) -> Response<Value> {
    let code = response.error.as_ref().map_or_else(
        || "record_watch_replay_failed".into(),
        |error| error.code.clone(),
    );
    let message = response.error.as_ref().map_or_else(
        || "record_watch_replay failed without an error body".into(),
        |error| error.message.clone(),
    );
    watch_failure_after_cleanup(runtime, request_id, &code, message)
}

fn watch_failure_after_cleanup(
    runtime: &mut Runtime,
    request_id: u64,
    code: &str,
    message: String,
) -> Response<Value> {
    match return_to_main_menu(runtime, request_id) {
        Ok(_) => Response::failure(request_id, code, message),
        Err(cleanup) => {
            let cleanup = cleanup
                .error
                .as_ref()
                .map_or("unknown cleanup error", |error| error.message.as_str());
            Response::failure(
                request_id,
                code,
                format!("{message}; watch cleanup also failed: {cleanup}"),
            )
        }
    }
}

/// Leave whatever match is running and settle at the main menu.
///
/// This is what the adapter owes its next client, and what an operation that
/// stopped early owes the game.
fn return_to_main_menu(runtime: &mut Runtime, request_id: u64) -> Result<Value, Response<Value>> {
    if runtime.current_match().is_null() {
        let status = successful_result(execute_internal_on_main(
            runtime,
            request_id,
            operations::InternalOperation::Status,
        ))?;
        if status.get("status").and_then(Value::as_str) == Some("main_menu") {
            return Ok(status);
        }
        return wait_layout_status(
            runtime,
            request_id,
            Instant::now() + REPLAY_LOAD_TIMEOUT,
            "main_menu after watch transition",
            LAYOUT_DEPLOYMENT_STABLE_SAMPLES,
            |value| value.get("status").and_then(Value::as_str) == Some("main_menu"),
        );
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
        "main_menu after watch exit",
        LAYOUT_DEPLOYMENT_STABLE_SAMPLES,
        |value| value.get("status").and_then(Value::as_str) == Some("main_menu"),
    )
}

#[allow(clippy::too_many_lines)]
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
    if arguments.round < 1 {
        return Response::failure(
            request.id,
            "invalid_arguments",
            "record_replay_round round must be at least 1",
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

    // The fight runs to its end inside one main-thread call, and the capture
    // armed before it records the requested round from the queue afterwards.
    let capture_request = Request {
        id: request.id,
        operation: Operation::RecordBattle,
        arguments: serde_json::json!({
            "output": arguments.output,
            "speed_up": false,
            "instrumentation": arguments.instrumentation,
        }),
    };
    let mut response = execute_recording_series(
        runtime,
        &capture_request,
        capture::CaptureStartMode::Replay(arguments.round),
        Some(&arguments.grbr),
    );
    if let Some(result) = response.result.as_mut().and_then(Value::as_object_mut) {
        result.insert("grbr".into(), serde_json::json!(arguments.grbr));
        result.insert("round".into(), serde_json::json!(arguments.round));
    }
    response
}

#[allow(clippy::too_many_lines)]
fn execute_recording_series(
    runtime: &mut Runtime,
    request: &Request,
    mode: capture::CaptureStartMode,
    replay: Option<&PathBuf>,
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
    // A recording with video is paced by its render barrier, not by scaled
    // time, so capture scales time only without it.
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
    if let (capture::CaptureStartMode::Replay(round), Some(grbr)) = (mode, replay)
        && let Err(response) = successful_result(execute_replay_fight_on_main(
            runtime, request.id, grbr, round,
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
        // A capture in flight is abandoned like any other wait. Nothing is
        // published, the sidecar and video are torn down with it, and the
        // claim is answered now rather than up to three minutes from now.
        if evicting() {
            return recording_failure(
                runtime,
                request.id,
                EVICTED_CODE,
                format!("level {} claimed the game", evicting_level()),
            );
        }
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
                        instrumentation_records.push((recorded_tick, observation));
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
    if plan.round > MAX_STAGED_ROUND {
        return Response::failure(
            request.id,
            "unsupported",
            format!(
                "apply_layout advances through every earlier round inside one timeout budget \
                 and stages at most round {MAX_STAGED_ROUND}, so round {} cannot be reached",
                plan.round
            ),
        );
    }
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
    let unit_count = plan.unit_count();
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
            "unit_count": unit_count,
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
    wait_status(
        runtime,
        request_id,
        &StatusWait {
            deadline,
            interval: LAYOUT_STATUS_INTERVAL,
            description,
            stable_samples,
        },
        evicting,
        predicate,
    )
}

/// One status-polling wait: how long, how often, and what it is called.
///
/// The interval is the caller's because every sample is a synchronous
/// main-thread dispatch: a deployment that resolves in a second is worth 50ms,
/// a match that lasts an hour is not.
struct StatusWait<'a> {
    deadline: Instant,
    interval: Duration,
    description: &'a str,
    stable_samples: usize,
}

/// Poll status until it satisfies `predicate`, `stop` asks to give up, or the
/// deadline passes. A `stop` that fires is reported as its own error code, so
/// a caller can tell an abandoned wait from a failed one.
///
/// Every wait gives up for a higher claim. An operation abandoned that way
/// leaves the game part-way through, which is what the adapter's own return to
/// the main menu is for: the claim is answered in seconds rather than after
/// however long this operation would have taken.
fn wait_status(
    runtime: &mut Runtime,
    request_id: u64,
    wait: &StatusWait<'_>,
    stop: impl Fn() -> bool,
    predicate: impl Fn(&Value) -> bool,
) -> Result<Value, Response<Value>> {
    let StatusWait {
        deadline,
        interval,
        description,
        stable_samples,
    } = *wait;
    let mut stable = 0;
    let mut last = Value::Null;
    loop {
        if stop() {
            return Err(Response::failure(
                request_id,
                EVICTED_CODE,
                format!(
                    "level {} claimed the game while waiting for {description}",
                    evicting_level()
                ),
            ));
        }
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
        thread::sleep(interval);
    }
}

fn is_training_state(status: &Value, round: i32, deploying: bool, fighting: bool) -> bool {
    status.get("status").and_then(Value::as_str) == Some("training_ground")
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

/// `EVICTING_FOR` when no claim is taking the game.
///
/// The level of a claim is stored one above its value, which leaves zero to
/// mean "nobody", and lets a second, higher claim raise the pending one with a
/// single `fetch_max`.
const NO_CLAIM: u8 = 0;

/// The level of the client currently being served.
static HOLDER_LEVEL: AtomicU8 = AtomicU8::new(0);

/// The level that is taking the game, set by the accept thread.
///
/// It outlives the operation it interrupts, so a claim cannot be lost in the
/// gap between two of them, and is cleared once the game is back at the main
/// menu and the slot is free.
static EVICTING_FOR: AtomicU8 = AtomicU8::new(NO_CLAIM);

/// Whether a higher claim is taking the game from the serving client.
fn evicting() -> bool {
    EVICTING_FOR.load(Ordering::SeqCst) != NO_CLAIM
}

/// The level that is taking the game, meaningless unless [`evicting`].
fn evicting_level() -> u8 {
    EVICTING_FOR.load(Ordering::SeqCst).saturating_sub(1)
}

/// Record that a claim is taking the game, keeping the highest one.
fn claim_eviction(level: u8) {
    EVICTING_FOR.fetch_max(level + 1, Ordering::SeqCst);
}

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
    fn native_replay_directory_is_derived_from_the_app_bundle() {
        let root = tempfile::tempdir().unwrap();
        let app = root.path().join("Mechabellum.app");
        let replay = app.join("ProjectDatas/Replay");
        fs::create_dir_all(&replay).unwrap();
        let executable = app.join("Contents/MacOS/Mechabellum");
        assert_eq!(
            native_replay_directory_from_executable(&executable).unwrap(),
            replay.canonicalize().unwrap()
        );
        assert!(native_replay_directory_from_executable(Path::new("/tmp/game")).is_err());
    }

    #[test]
    fn newest_changed_replay_ignores_the_baseline_and_uses_mtime() {
        let old = PathBuf::from("old.grbr");
        let first = PathBuf::from("first.grbr");
        let newest = PathBuf::from("newest.grbr");
        let unchanged = ReplayFileState {
            len: 10,
            modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1),
        };
        let baseline = BTreeMap::from([(old.clone(), unchanged.clone())]);
        let current = BTreeMap::from([
            (old, unchanged),
            (
                first,
                ReplayFileState {
                    len: 20,
                    modified: SystemTime::UNIX_EPOCH + Duration::from_secs(2),
                },
            ),
            (
                newest.clone(),
                ReplayFileState {
                    len: 30,
                    modified: SystemTime::UNIX_EPOCH + Duration::from_secs(3),
                },
            ),
        ]);
        assert_eq!(
            newest_changed_replay(&baseline, &current).map(|(path, _)| path),
            Some(newest)
        );
    }

    #[test]
    fn waiting_for_a_replay_gives_autosave_its_whole_grace_period() {
        let root = tempfile::tempdir().unwrap();
        let baseline = BTreeMap::new();
        let grace = WATCH_FILE_POLL_INTERVAL * 4;
        let started = Instant::now();
        // This property is about an uninterrupted wait. Supplying that premise
        // keeps a concurrent test of process-global eviction from changing it.
        assert_eq!(
            wait_for_stable_replay(root.path(), &baseline, started + grace, || false).unwrap(),
            None
        );
        assert!(started.elapsed() >= grace, "returned before the deadline");
    }

    #[test]
    fn replay_publication_is_create_new() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("native.grbr");
        let output = root.path().join("corpus.grbr");
        fs::write(&source, b"native recording").unwrap();
        copy_new_file(&source, &output).unwrap();
        assert_eq!(fs::read(&output).unwrap(), b"native recording");
        assert!(copy_new_file(&source, &output).is_err());
        assert_eq!(fs::read(&output).unwrap(), b"native recording");
    }

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

    /// A claim is how a connection identifies itself, and the only thing that
    /// decides between being served, being refused, and taking the game.
    #[test]
    fn a_claim_decides_between_being_served_refused_and_taking_over() {
        let path = PathBuf::from(format!(
            "/tmp/mechcore-adapter-claim-{}.sock",
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

        // The first client claims level 1 and takes the single serving slot.
        let held = claim(&path, 1);
        let served = receiver.recv().unwrap();
        assert!(serving.load(Ordering::SeqCst));
        assert_eq!(HOLDER_LEVEL.load(Ordering::SeqCst), 1);
        assert!(!evicting());

        // An equal claim is refused: two clients that matter the same amount
        // cannot each decide the other should stop.
        let (equal, answer) = claim_and_read(&path, 1);
        assert_eq!(answer["kind"], "busy");
        assert_eq!(answer["holder_level"], 1);
        assert_eq!(answer["evicting"], false);
        assert!(!evicting());

        // A higher claim takes the game, and is told to come back for it.
        let (higher, answer) = claim_and_read(&path, 3);
        assert_eq!(answer["kind"], "busy");
        assert_eq!(answer["holder_level"], 1);
        assert_eq!(answer["evicting"], true);
        assert!(evicting());
        assert_eq!(evicting_level(), 3);

        // Anything that is not a claim is refused as such, not left to look
        // like an adapter that stopped answering.
        let mut stranger = UnixStream::connect(&path).unwrap();
        let mut reader = BufReader::new(stranger.try_clone().unwrap());
        stranger
            .write_all(b"{\"id\":1,\"operation\":\"status\"}\n")
            .unwrap();
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&line).unwrap()["kind"],
            "refused"
        );

        EVICTING_FOR.store(NO_CLAIM, Ordering::SeqCst);
        drop(held);
        drop(equal);
        drop(higher);
        drop(stranger);
        drop(served);

        // Free the slot, close the channel, then knock once: the greeter
        // observes the dropped receiver on its next send and returns. Without
        // the knock it would stay blocked in accept forever.
        serving.store(false, Ordering::SeqCst);
        drop(receiver);
        let knock = claim(&path, 0);
        let _ = greeter.join();
        drop(knock);
        let _ = fs::remove_file(path);
    }

    fn claim(path: &Path, level: u8) -> UnixStream {
        let mut stream = UnixStream::connect(path).unwrap();
        write_json_line(&mut stream, &Claim::current(level)).unwrap();
        stream
    }

    fn claim_and_read(path: &Path, level: u8) -> (UnixStream, Value) {
        let stream = claim(path, level);
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).unwrap();
        let answer = serde_json::from_str(&line).unwrap();
        (stream, answer)
    }
}
