use crate::capture::{self, CaptureMessage, InstrumentRows};
use crate::il2cpp::{Api, Class, Error as Il2CppError, FieldInfo, Object};
use crate::layout::{self, Plan};
use crate::operations;
use crate::scheduler::{ConnectionId, Next, Scheduler, Waiting};
use mechcore_protocol::{
    Admission, Claim, EVICTED_CODE, Evicted, GAME_STOPPED_CODE, GameIdentity, Hello,
    InstrumentChannel, Leaving, MAX_LEVEL, MAX_STAGED_ROUND, MAX_WATCH_MATCH_TIMEOUT_SECONDS,
    MAX_WATCH_SCENE_WAIT_SECONDS, Operation, PROTOCOL, Queued, RecordFightArguments,
    RecordReplayRoundArguments, RecordWatchReplayArguments, Refused, Request, Response, Started,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::env;
use std::ffi::{CString, c_void};
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::Shutdown;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

const SOCKET_ENV: &str = "MECHCORE_ADAPTER_SOCKET";
/// Seconds a game waits for its next client before it quits itself. `mechcore`
/// sets it on the games it launches; a game started any other way waits for
/// ever.
const LINGER_ENV: &str = "MECHCORE_ADAPTER_LINGER_SECONDS";
/// How long a game that asked Unity to quit is given to exit before the
/// Adapter ends the process itself.
const QUIT_GRACE: Duration = Duration::from_secs(30);
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
/// How long a new connection has to state its level.
const CLAIM_DEADLINE: Duration = Duration::from_secs(3);
/// How long a write to a client may block before the client is given up on.
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);
/// How often the game is asked its status while it is free, which is what a
/// client that does not hold it is told.
const STATUS_INTERVAL: Duration = Duration::from_secs(1);
/// How long the free game sleeps when nothing wakes it.
const WAKE_INTERVAL: Duration = Duration::from_millis(250);
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
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    if let Err(error) = crate::headless::skip_the_resolution_check(invocation.api) {
        eprintln!("mechcore-adapter: {error}");
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    if let Err(error) = crate::watch::initialize(invocation.api) {
        eprintln!("mechcore-adapter: {error}");
    }
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
    // SAFETY: the recording that started this fight leaves the runtime alone
    // until it has joined the fight's thread, so only a shared view exists.
    let runtime = unsafe { &*invocation.runtime };
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

/// The runtime's address, carried to the thread that hands a fight to the
/// main thread.
struct RuntimeAddress(*mut Runtime);

// SAFETY: the address is dereferenced only on the main thread, inside the
// synchronous dispatch, while the recording that started the fight leaves the
// runtime alone until the fight's thread has been joined.
unsafe impl Send for RuntimeAddress {}

impl RuntimeAddress {
    const fn get(self) -> *mut Runtime {
        self.0
    }
}

/// Fights replay round `round` on the main thread from a thread of its own,
/// so the caller can read the capture while it runs. The caller joins the
/// returned thread before it uses the runtime again.
fn start_replay_fight(
    runtime: *mut Runtime,
    request_id: u64,
    path: PathBuf,
    round: i32,
) -> thread::JoinHandle<Response<Value>> {
    let address = RuntimeAddress(runtime);
    thread::spawn(move || {
        let mut invocation = ReplayFightInvocation {
            runtime: address.get(),
            request_id,
            path: &raw const path,
            round,
            response: None,
        };
        #[cfg(target_os = "macos")]
        {
            // SAFETY: queue is the process main queue and the callback/context
            // obey dispatch_sync_f's synchronous lifetime contract.
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

/// What the sockets and the game share: the line, and how to reach whoever is
/// in it.
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

struct State {
    scheduler: Scheduler,
    writers: BTreeMap<ConnectionId, Writer>,
    /// What the game last answered `status`, which is what a client that does
    /// not hold the game is told.
    status: Value,
    /// Who asked `quit_game`, and with which request.
    stopping: Option<(ConnectionId, u64)>,
    /// A lease ended where its holder left the scene: the game owes the next
    /// turn a main menu.
    scene_left: bool,
    /// When the game was last asked its status.
    status_read: Option<Instant>,
}

type Writer = Arc<Mutex<UnixStream>>;

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// What the game does next, decided under the lock and done outside it.
enum Work {
    Run(Waiting, Option<Writer>),
    Revoke(Option<Writer>, u8),
    Stop {
        asker: Option<Writer>,
        request_id: u64,
        cancelled: Vec<(Option<Writer>, u64)>,
    },
    ReturnToMenu,
    ReadStatus,
    Quit(Duration),
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
    let linger = linger()?;
    let identity = GameIdentity {
        adapter: adapter_digest()?,
        headless: crate::headless::without_graphics(),
        offline: crate::offline::sandboxed(),
        linger_seconds: linger.map(|linger| linger.as_secs()),
    };
    let endpoint = socket_path()?;
    let listener = bind_listener(&endpoint)?;
    arm_endpoint_cleanup(&endpoint);
    let _cleanup = SocketCleanup(endpoint);

    let shared = Arc::new(Shared {
        state: Mutex::new(State {
            scheduler: Scheduler::new(Instant::now()),
            writers: BTreeMap::new(),
            status: serde_json::json!({"status": "unknown"}),
            stopping: None,
            scene_left: false,
            status_read: None,
        }),
        wake: Condvar::new(),
    });
    thread::spawn({
        let shared = Arc::clone(&shared);
        move || greet_clients(&listener, &shared, &identity)
    });

    loop {
        match next_work(&shared, linger) {
            Work::Run(waiting, writer) => serve(&mut runtime, &shared, &waiting, writer.as_ref()),
            Work::Revoke(writer, by_level) => {
                if let Some(writer) = writer {
                    let mut stream = writer.lock().unwrap_or_else(PoisonError::into_inner);
                    let _ = write_json_line(&mut stream, &Evicted::current(by_level));
                    let _ = stream.shutdown(Shutdown::Both);
                }
            }
            Work::Stop {
                asker,
                request_id,
                cancelled,
            } => {
                for (writer, id) in cancelled {
                    answer(
                        writer.as_ref(),
                        &Response::<Value>::failure(id, GAME_STOPPED_CODE, STOPPED_MESSAGE),
                    );
                }
                quit(
                    &mut runtime,
                    asker.as_ref(),
                    request_id,
                    "quit_game was asked",
                );
            }
            Work::ReturnToMenu => {
                if let Err(response) = return_to_main_menu(&mut runtime, 0) {
                    let detail = response
                        .error
                        .as_ref()
                        .map_or("unknown error", |error| error.message.as_str());
                    eprintln!(
                        "mechcore-adapter: cannot reach the main menu for the next turn: {detail}"
                    );
                }
            }
            Work::ReadStatus => {
                let status = execute_on_main(&mut runtime, &status_request());
                let mut state = shared.lock();
                if let Some(status) = status.result {
                    if status.get("status").and_then(Value::as_str) == Some("main_menu") {
                        state.scheduler.ready = true;
                    }
                    state.status = status;
                }
                state.status_read = Some(Instant::now());
            }
            Work::Quit(linger) => quit(
                &mut runtime,
                None,
                0,
                &format!("no client for {}s", linger.as_secs()),
            ),
        }
        shared.wake.notify_all();
    }
}

/// Wait until the game has something to do, and take it.
fn next_work(shared: &Shared, linger: Option<Duration>) -> Work {
    let mut state = shared.lock();
    loop {
        let now = Instant::now();
        if let Some((asker, request_id)) = state.stopping {
            let cancelled = state
                .scheduler
                .drain()
                .into_iter()
                .map(|(connection, id)| (state.writers.get(&connection).cloned(), id))
                .collect();
            return Work::Stop {
                asker: state.writers.get(&asker).cloned(),
                request_id,
                cancelled,
            };
        }
        if state.scene_left {
            state.scene_left = false;
            ABANDON.store(KEEP, Ordering::SeqCst);
            return Work::ReturnToMenu;
        }
        if state
            .status_read
            .is_none_or(|read| now.duration_since(read) >= STATUS_INTERVAL)
        {
            return Work::ReadStatus;
        }
        match state.scheduler.next(now) {
            Next::Run(waiting) => {
                // Whatever abandoned the last request is not this one's.
                ABANDON.store(KEEP, Ordering::SeqCst);
                let writer = state.writers.get(&waiting.connection).cloned();
                return Work::Run(waiting, writer);
            }
            Next::Revoke { holder, by_level } => {
                let writer = state.writers.remove(&holder);
                state.scheduler.close(holder, now);
                state.scene_left = true;
                return Work::Revoke(writer, by_level);
            }
            Next::Wait => {}
        }
        if let Some(linger) = linger
            && state
                .scheduler
                .idle_for(now)
                .is_some_and(|idle| idle >= linger)
        {
            // Under the lock, so that no claim is greeted by a game that is
            // about to go: the greeter reads this flag under the same lock.
            QUITTING.store(true, Ordering::SeqCst);
            return Work::Quit(linger);
        }
        state = shared
            .wake
            .wait_timeout(state, WAKE_INTERVAL)
            .unwrap_or_else(PoisonError::into_inner)
            .0;
    }
}

/// Run one request that waited for its turn, and answer it.
fn serve(runtime: &mut Runtime, shared: &Shared, waiting: &Waiting, writer: Option<&Writer>) {
    let request = &waiting.request;
    answer(writer, &Started::current(request.id));
    let response = match request.operation {
        Operation::Lease => Response::success(request.id, serde_json::json!({"lease": true})),
        Operation::ApplyLayout => execute_layout_series(runtime, request),
        Operation::RecordFight => execute_recording_series(
            runtime,
            request,
            capture::CaptureStartMode::TrainingGround,
            None,
        ),
        Operation::RecordReplayRound => execute_replay_recording_series(runtime, request),
        Operation::RecordWatchReplay => execute_watch_replay_series(runtime, request),
        _ => execute_on_main(runtime, request),
    };
    answer(writer, &response);
    let mut state = shared.lock();
    state.scheduler.finish(response.ok, Instant::now());
    // The next bystander is told what the game is now, not before this turn.
    state.status_read = None;
}

/// Write to a client, if it is still there to read it. A client that has
/// gone is found out by its reader, which closes it.
fn answer(writer: Option<&Writer>, message: &impl serde::Serialize) {
    if let Some(writer) = writer {
        let mut stream = writer.lock().unwrap_or_else(PoisonError::into_inner);
        if let Err(error) = write_json_line(&mut stream, message) {
            eprintln!("mechcore-adapter: cannot answer a client: {error}");
        }
    }
}

fn status_request() -> Request {
    Request {
        id: 0,
        operation: Operation::Status,
        arguments: serde_json::json!({}),
    }
}

/// How long this game waits for its next client, from [`LINGER_ENV`].
fn linger() -> Result<Option<Duration>, RuntimeError> {
    let Some(value) = env::var_os(LINGER_ENV) else {
        return Ok(None);
    };
    value
        .to_str()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .map(|seconds| Some(Duration::from_secs(seconds)))
        .ok_or_else(|| {
            RuntimeError::Configuration(format!(
                "{LINGER_ENV} must be a positive number of seconds"
            ))
        })
}

/// BLAKE3 of the library this code was loaded from.
///
/// The file is read once, as the Adapter starts: a build that replaces it
/// afterwards is exactly what the digest has to tell apart.
fn adapter_digest() -> Result<String, RuntimeError> {
    let mut info = libc::Dl_info {
        dli_fname: std::ptr::null(),
        dli_fbase: std::ptr::null_mut(),
        dli_sname: std::ptr::null(),
        dli_saddr: std::ptr::null_mut(),
    };
    // SAFETY: dladdr only reads the loader's image list, and `info` is a
    // valid out-parameter for the call.
    let found = unsafe { libc::dladdr(adapter_digest as *const c_void, &raw mut info) };
    if found == 0 || info.dli_fname.is_null() {
        return Err(RuntimeError::Configuration(
            "cannot find the Adapter library's own path".into(),
        ));
    }
    // SAFETY: the loader returns a NUL-terminated path that lives as long as
    // the image, which is the process.
    let path = unsafe { std::ffi::CStr::from_ptr(info.dli_fname) };
    let bytes = fs::read(Path::new(std::ffi::OsStr::from_bytes(path.to_bytes())))?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

/// End the game: a lingering one nobody wanted, or any one `quit_game` was
/// asked of.
///
/// Unity is asked to quit from the main menu. The process is ended here only
/// if that does not happen, so that a game whose main thread no longer answers
/// still goes away. Every claim from here on is told the game is leaving.
fn quit(runtime: &mut Runtime, asker: Option<&Writer>, request_id: u64, why: &str) -> ! {
    QUITTING.store(true, Ordering::SeqCst);
    eprintln!("mechcore-adapter: {why}; quitting");
    // Whatever stopped the last request has done so; leaving the match on
    // the way out is not to be stopped by it too.
    ABANDON.store(KEEP, Ordering::SeqCst);
    let quit = Request {
        id: request_id,
        operation: Operation::QuitGame,
        arguments: serde_json::json!({}),
    };
    let response = match return_to_main_menu(runtime, request_id) {
        Ok(_) => execute_on_main(runtime, &quit),
        Err(response) => response,
    };
    if let Some(error) = &response.error {
        eprintln!("mechcore-adapter: cannot quit the game: {}", error.message);
    }
    answer(asker, &response);
    thread::sleep(QUIT_GRACE);
    eprintln!(
        "mechcore-adapter: the game did not exit within {}s; ending it",
        QUIT_GRACE.as_secs()
    );
    std::process::exit(1);
}

/// Accepts connections and greets every claim.
///
/// Answering here, rather than leaving a connection in the backlog, keeps
/// readiness a protocol fact: a client that is not greeted within its deadline
/// is talking to an adapter that stopped answering, not one that is busy.
fn greet_clients(listener: &UnixListener, shared: &Arc<Shared>, identity: &GameIdentity) {
    let mut next_id: ConnectionId = 0;
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
        let claim = match read_claim(&mut stream) {
            Ok(claim) => claim,
            Err(error) => {
                eprintln!("mechcore-adapter: rejected claim: {error}");
                continue;
            }
        };
        let greeted = (|| {
            stream.set_read_timeout(None)?;
            stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
            let writer = Arc::new(Mutex::new(stream.try_clone()?));
            let mut state = shared.lock();
            // A game that is quitting is on its way to every claim alike:
            // each is told to wait, and finds the game gone.
            if QUITTING.load(Ordering::SeqCst) {
                drop(state);
                write_json_line(&mut stream, &Leaving::current())?;
                return Ok(None);
            }
            write_json_line(&mut stream, &Hello::current(identity.clone()))?;
            next_id += 1;
            state
                .scheduler
                .connect(next_id, claim.client, claim.level, Instant::now());
            state.writers.insert(next_id, writer);
            Ok::<_, io::Error>(Some(next_id))
        })();
        match greeted {
            Ok(Some(id)) => {
                let shared = Arc::clone(shared);
                thread::spawn(move || read_requests(id, stream, &shared));
            }
            Ok(None) => {}
            Err(error) => eprintln!("mechcore-adapter: cannot greet a client: {error}"),
        }
    }
}

/// Read the claim that opens a connection.
///
/// The claim arrives before the greeting because the greeting is the answer to
/// it. A client that says nothing is dropped rather than served: an
/// unidentified client cannot be ranked or given turns.
fn read_claim(stream: &mut UnixStream) -> io::Result<Claim> {
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
    if claim.client.is_empty() {
        return refuse("claim names no client");
    }
    Ok(claim)
}

/// Read one client's requests until it goes, then take it out of the line.
fn read_requests(id: ConnectionId, stream: UnixStream, shared: &Shared) {
    let mut reader = BufReader::new(stream);
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        match reader
            .by_ref()
            .take(MAX_MESSAGE_BYTES as u64 + 1)
            .read_until(b'\n', &mut bytes)
        {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if bytes.len() > MAX_MESSAGE_BYTES || !bytes.ends_with(b"\n") {
            break;
        }
        match serde_json::from_slice::<Request>(&bytes) {
            Ok(request) => admit(id, request, shared),
            Err(error) => {
                let writer = shared.lock().writers.get(&id).cloned();
                answer(
                    writer.as_ref(),
                    &Response::<Value>::failure(0, "invalid_request", error.to_string()),
                );
            }
        }
    }
    let mut state = shared.lock();
    state.writers.remove(&id);
    let closed = state.scheduler.close(id, Instant::now());
    if closed.running {
        ABANDON.store(DISCONNECTED, Ordering::SeqCst);
    }
    if closed.held_lease {
        state.scene_left = true;
    }
    drop(state);
    shared.wake.notify_all();
}

/// Answer a request at once, or put it in line for its turn.
fn admit(id: ConnectionId, request: Request, shared: &Shared) {
    let mut state = shared.lock();
    let writer = state.writers.get(&id).cloned();
    let request_id = request.id;
    let refuse = |code: &str, message: &str| {
        answer(
            writer.as_ref(),
            &Response::<Value>::failure(request_id, code, message),
        );
    };
    if state.stopping.is_some() && request.operation.admission() != Admission::Immediate {
        return refuse(GAME_STOPPED_CODE, STOPPED_MESSAGE);
    }
    let holder = state.scheduler.holds_lease(id);
    match (request.operation.admission(), request.operation) {
        (_, Operation::Queue) => {
            let snapshot = state.scheduler.snapshot(Instant::now());
            answer(writer.as_ref(), &Response::success(request.id, snapshot));
        }
        (_, Operation::QuitGame) => {
            if state.stopping.is_none() {
                state.stopping = Some((id, request.id));
                ABANDON.store(STOPPED, Ordering::SeqCst);
            }
        }
        // Whoever does not hold the game is told what it last said; asking
        // the game itself would wait for somebody else's turn to end.
        (_, Operation::Status) if !holder => {
            answer(
                writer.as_ref(),
                &Response::success(request.id, state.status.clone()),
            );
        }
        (_, Operation::Lease) if holder => {
            answer(
                writer.as_ref(),
                &Response::success(request.id, serde_json::json!({"lease": true})),
            );
        }
        (Admission::Leased, _) if !holder => refuse(
            "no_lease",
            "a scene operation needs the lease; ask for it with lease first",
        ),
        _ => {
            let position = state.scheduler.enqueue(id, request, Instant::now());
            answer(writer.as_ref(), &Queued::current(request_id, position));
            if let Some(level) = state.scheduler.outranks_running(id) {
                ABANDON_LEVEL.store(level, Ordering::SeqCst);
                ABANDON.store(OUTRANKED, Ordering::SeqCst);
            }
        }
    }
    drop(state);
    shared.wake.notify_all();
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
    if crate::offline::sandboxed() {
        return Response::failure(
            request.id,
            "invalid_game_state",
            "record_watch_replay watches the server's matches, and this game was started \
             offline; launch it with the network to watch",
        );
    }
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
        if abandoning() {
            return abandoned_response(request.id);
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
                abandoning,
                |status| {
                    status.get("status").and_then(Value::as_str) == Some("spectating")
                        && status.get("round_count").and_then(Value::as_i64) == Some(1)
                        && status.get("fight_ready").and_then(Value::as_bool) == Some(true)
                },
            );
            match entry {
                Ok(_) => break selection,
                Err(response) if abandoned(&response) => return response,
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
        abandoning,
        |status| {
            status.get("status").and_then(Value::as_str) == Some("spectating")
                && status.get("finished").and_then(Value::as_bool) == Some(true)
        },
    );
    if let Err(response) = finished {
        // The match is abandoned mid-way and writes no recording. The adapter
        // returns the game to the main menu once this client is gone, so the
        // operation does not clean up on its way out.
        if abandoned(&response) {
            return response;
        }
        return watch_response_after_cleanup(runtime, request.id, &response);
    }

    let source = match wait_for_stable_replay(
        &replay_dir,
        &baseline,
        Instant::now() + WATCH_AUTOSAVE_GRACE,
        abandoning,
    ) {
        Ok(Some(path)) => path,
        Ok(None) => {
            // Waiting for the file to settle is a wait like any other: a claim
            // ends it, and the game's own recording stays where it wrote it.
            if abandoning() {
                return abandoned_response(request.id);
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
                abandoning,
            ) {
                Ok(Some(path)) => path,
                Ok(None) if abandoning() => return abandoned_response(request.id),
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

/// Whether a failed wait stopped because the request was abandoned.
fn abandoned(response: &Response<Value>) -> bool {
    response.error.as_ref().is_some_and(|error| {
        [EVICTED_CODE, GAME_STOPPED_CODE, CANCELLED_CODE].contains(&error.code.as_str())
    })
}

fn abandoned_response(request_id: u64) -> Response<Value> {
    let (code, message) = abandonment();
    Response::failure(request_id, code, message)
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
        operation: Operation::RecordFight,
        arguments: serde_json::json!({
            "output": arguments.output,
            "speed_up": false,
            "instrument": arguments.instrument,
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
    let arguments: RecordFightArguments = match serde_json::from_value(request.arguments.clone()) {
        Ok(arguments) => arguments,
        Err(error) => return Response::failure(request.id, "invalid_arguments", error.to_string()),
    };
    if !arguments.output.is_absolute() {
        return Response::failure(
            request.id,
            "invalid_arguments",
            "record_fight output must be absolute",
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
            "record_fight output must use the .mcfr extension",
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
        if crate::headless::without_graphics() {
            return Response::failure(
                request.id,
                "invalid_game_state",
                "record_fight video_output needs rendered frames, and this game was \
                 started with -nographics; launch it with a window to record video",
            );
        }
        if !video_output.is_absolute() {
            return Response::failure(
                request.id,
                "invalid_arguments",
                "record_fight video_output must be absolute",
            );
        }
        if video_output.extension().and_then(|value| value.to_str()) != Some("mov") {
            return Response::failure(
                request.id,
                "invalid_arguments",
                "record_fight video_output must use the .mov extension",
            );
        }
        if video_output == &arguments.output {
            return Response::failure(
                request.id,
                "invalid_arguments",
                "record_fight output and video_output must differ",
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
            instruments: capture::Instruments::of(&arguments.instrument),
        },
    )) {
        return response;
    }
    // A replay is fought on the main thread while this one writes the ticks
    // it captures, so the fight and the file overlap rather than one waiting
    // for the other. Until the fight returns, the main thread holds the
    // runtime, so everything below that uses it waits for the fight first.
    let replayed = replay.is_some();
    let fight = match (mode, replay) {
        (capture::CaptureStartMode::Replay(round), Some(grbr)) => Some(start_replay_fight(
            std::ptr::from_mut(runtime),
            request.id,
            grbr.clone(),
            round,
        )),
        _ => None,
    };
    let drained = drain_recording(request, &arguments, fight.as_ref());
    // A failed recording stops capturing before it waits for the fight, which
    // then runs out without being snapshotted.
    if let Drained::Failed(_, message) = &drained {
        capture::abort(message);
    }
    let fought = fight.map(|fight| {
        fight.join().unwrap_or_else(|_| {
            Response::failure(
                request.id,
                "main_thread_dispatch_failed",
                "the replay fight's thread panicked",
            )
        })
    });
    if let Some(fought) = fought
        && !fought.ok
    {
        capture::abort("the replay fight failed");
        stop_capture(runtime, request.id);
        return fought;
    }
    match drained {
        Drained::Published(response) => {
            // A fight that ran out of time leaves the capture armed.
            if replayed {
                stop_capture(runtime, request.id);
            }
            response
        }
        Drained::Failed(code, message) => recording_failure(runtime, request.id, code, message),
    }
}

/// How reading a capture into its recording ended.
enum Drained {
    Published(Response<Value>),
    /// A failure, which stopping the capture has still to follow.
    Failed(&'static str, String),
}

/// Reads a capture's ticks into its recording until the terminal tick
/// publishes it. It never touches the runtime, which a replay's fight may be
/// holding on the main thread meanwhile.
#[allow(clippy::too_many_lines)] // One message loop, each arm short.
fn drain_recording(
    request: &Request,
    arguments: &RecordFightArguments,
    fight: Option<&thread::JoinHandle<Response<Value>>>,
) -> Drained {
    let deadline = Instant::now() + RECORDING_TIMEOUT;
    let mut pending = None;
    let mut writer = None;
    let mut video = None;
    let mut recorded_tick = 0_u64;
    loop {
        // A capture in flight is abandoned like any other wait. Nothing is
        // published, the video is torn down with it, and the
        // claim is answered now rather than up to three minutes from now.
        if abandoning() {
            let (code, message) = abandonment();
            return Drained::Failed(code, message);
        }
        if Instant::now() >= deadline {
            return Drained::Failed(
                "operation_timeout",
                "recording timed out before fighting-to-over boundary".into(),
            );
        }
        let message = pending.take().or_else(capture::poll);
        let finished = match message {
            Some(CaptureMessage::Initial {
                game_build,
                context,
                layout_yaml,
            }) => {
                if writer.is_some() {
                    return Drained::Failed(
                        "capture_failed",
                        "capture emitted more than one recording header".into(),
                    );
                }
                if let Some(video_output) = arguments.video_output.as_ref() {
                    let created =
                        match crate::video::MovWriter::create(video_output, context.logic_step) {
                            Ok(created) => created,
                            Err(error) => {
                                return Drained::Failed("video_error", error);
                            }
                        };
                    video = Some(created);
                }
                match mechcore_mcfr::McfrWriter::create(
                    &arguments.output,
                    mechcore_mcfr::Producer::Game,
                    &game_build,
                    &context,
                    &layout_yaml,
                ) {
                    Ok(created) => writer = Some(created),
                    Err(error) => {
                        return Drained::Failed("mcfr_error", error.to_string());
                    }
                }
                false
            }
            Some(CaptureMessage::Transition {
                events,
                state,
                instrument,
                terminal,
                frame,
            }) => {
                let Some(active) = writer.as_mut() else {
                    return Drained::Failed(
                        "capture_failed",
                        "capture tick preceded its recording header".into(),
                    );
                };
                if let Err(error) = active.append_tick(state, &events) {
                    return Drained::Failed("mcfr_error", error.to_string());
                }
                recorded_tick += 1;
                if let Err((code, error)) =
                    append_instrument(active, &arguments.instrument, instrument)
                {
                    return Drained::Failed(code, error);
                }
                match (video.as_mut(), frame.as_ref()) {
                    (Some(active), Some(frame)) => {
                        // Encoding runs here, on the socket thread, so the game's
                        // main thread is free to render the next logic frame.
                        let jpeg = match frame.encode_jpeg() {
                            Ok(jpeg) => jpeg,
                            Err(error) => {
                                return Drained::Failed("video_error", error);
                            }
                        };
                        if let Err(error) = active.append_jpeg(&jpeg) {
                            return Drained::Failed("video_error", error);
                        }
                    }
                    (Some(_), None) => {
                        return Drained::Failed(
                            "capture_failed",
                            "visual capture omitted a logic frame".into(),
                        );
                    }
                    (None, Some(_)) => {
                        return Drained::Failed(
                            "capture_failed",
                            "visual capture produced a frame without video_output".into(),
                        );
                    }
                    (None, None) => {}
                }
                terminal
            }
            Some(CaptureMessage::Failure(error)) => {
                return Drained::Failed("capture_failed", error);
            }
            // The headless call returns once the fight is over, having
            // captured each of its ticks. A fight that runs out of time ends
            // outside `FightController.Update`, where the capture never sees
            // it end, so the tick last captured is its terminal one; with none
            // captured, the round never reached the fight.
            None if fight.is_some_and(thread::JoinHandle::is_finished) => {
                if let Some(message) = capture::poll() {
                    pending = Some(message);
                    continue;
                }
                if writer.is_none() || recorded_tick == 0 {
                    return Drained::Failed(
                        "capture_failed",
                        "the replay ended without fighting the requested round".into(),
                    );
                }
                true
            }
            None => {
                thread::sleep(RECORDING_POLL_INTERVAL);
                false
            }
        };
        if finished {
            let video_summary = match video.take() {
                Some(video) => match video.finish() {
                    Ok(summary) => Some(summary),
                    Err(error) => {
                        return Drained::Failed("video_error", error);
                    }
                },
                None => None,
            };
            let hashes = match writer.take().expect("writer checked above").finish() {
                Ok(hashes) => hashes,
                Err(error) => {
                    remove_published(arguments.video_output.as_deref());
                    return Drained::Published(Response::failure(
                        request.id,
                        "mcfr_error",
                        error.to_string(),
                    ));
                }
            };
            // `finish` has read the packaged file back and matched its hashes
            // before publishing it, and a recording's terminal tick is its last.
            let Ok(tick_count) = u32::try_from(recorded_tick) else {
                remove_published(Some(&arguments.output));
                remove_published(arguments.video_output.as_deref());
                return Drained::Published(Response::failure(
                    request.id,
                    "mcfr_error",
                    "the recording holds more ticks than an MCFR counts",
                ));
            };
            if let Some(summary) = &video_summary
                && summary.frame_count != u64::from(tick_count)
            {
                remove_published(Some(&arguments.output));
                remove_published(arguments.video_output.as_deref());
                return Drained::Published(Response::failure(
                    request.id,
                    "video_verification_failed",
                    format!(
                        "video frame count {} does not match MCFR state count {}",
                        summary.frame_count,
                        u64::from(tick_count)
                    ),
                ));
            }
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
            return Drained::Published(Response::success(
                request.id,
                serde_json::json!({
                    "recorded": true,
                    "output": arguments.output,
                    "tick_count": tick_count,
                    "terminal_tick": tick_count,
                    "hashes": hashes,
                    "video": video_result,
                    "instrument": arguments.instrument,
                }),
            ));
        }
    }
}

/// Writes one tick's instrument rows, refusing a tick whose rows do not answer
/// exactly the channels the recording asked for.
fn append_instrument(
    writer: &mut mechcore_mcfr::McfrWriter,
    asked: &[InstrumentChannel],
    rows: InstrumentRows,
) -> Result<(), (&'static str, String)> {
    for channel in InstrumentChannel::ALL {
        let present = match channel {
            InstrumentChannel::TargetRefs => rows.target_refs.is_some(),
            InstrumentChannel::SkillAttackableChecker => rows.skill_attackable_checker.is_some(),
            InstrumentChannel::TargetSearch => rows.target_search.is_some(),
            InstrumentChannel::TargetCandidate => rows.target_candidate.is_some(),
            InstrumentChannel::RvoSolve => rows.rvo_solve.is_some(),
            InstrumentChannel::RvoNeighbour => rows.rvo_neighbour.is_some(),
            InstrumentChannel::RvoVo => rows.rvo_vo.is_some(),
            InstrumentChannel::UnitPose => rows.unit_pose.is_some(),
            InstrumentChannel::ProjectileReach => rows.projectile_reach.is_some(),
            InstrumentChannel::ExpRange => rows.exp_range.is_some(),
        };
        if present != asked.contains(&channel) {
            return Err((
                "capture_failed",
                format!(
                    "capture {} channel {}",
                    if present {
                        "produced unrequested"
                    } else {
                        "omitted requested"
                    },
                    channel.as_str()
                ),
            ));
        }
    }
    let mcfr = |error: mechcore_mcfr::Error| ("mcfr_error", error.to_string());
    if let Some(rows) = rows.target_refs {
        writer.append_instrument(rows.as_slice()).map_err(mcfr)?;
    }
    if let Some(rows) = rows.skill_attackable_checker {
        writer.append_instrument(rows.as_slice()).map_err(mcfr)?;
    }
    if let Some(rows) = rows.target_search {
        writer.append_instrument(rows.as_slice()).map_err(mcfr)?;
    }
    if let Some(rows) = rows.target_candidate {
        writer.append_instrument(rows.as_slice()).map_err(mcfr)?;
    }
    if let Some(rows) = rows.rvo_solve {
        writer.append_instrument(rows.as_slice()).map_err(mcfr)?;
    }
    if let Some(rows) = rows.rvo_neighbour {
        writer.append_instrument(rows.as_slice()).map_err(mcfr)?;
    }
    if let Some(rows) = rows.rvo_vo {
        writer.append_instrument(rows.as_slice()).map_err(mcfr)?;
    }
    if let Some(rows) = rows.unit_pose {
        writer.append_instrument(rows.as_slice()).map_err(mcfr)?;
    }
    if let Some(rows) = rows.projectile_reach {
        writer.append_instrument(rows.as_slice()).map_err(mcfr)?;
    }
    if let Some(rows) = rows.exp_range {
        writer.append_instrument(rows.as_slice()).map_err(mcfr)?;
    }
    Ok(())
}

fn recording_failure(
    runtime: &mut Runtime,
    request_id: u64,
    code: &str,
    message: String,
) -> Response<Value> {
    capture::abort(&message);
    stop_capture(runtime, request_id);
    Response::failure(request_id, code, message)
}

fn stop_capture(runtime: &mut Runtime, request_id: u64) {
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
        abandoning,
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
            let (code, message) = abandonment();
            return Err(Response::failure(
                request_id,
                code,
                format!("{message} while waiting for {description}"),
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

/// Why the running request is to be abandoned at its next polling point.
///
/// Nothing waits for a request to finish on its own once it has to stop: every
/// wait inside a long operation reads this, a capture included, and the
/// operation answers with the code [`abandonment`] names.
static ABANDON: AtomicU8 = AtomicU8::new(KEEP);
/// The level that outranked a running lease request, meaningful while
/// [`ABANDON`] is [`OUTRANKED`].
static ABANDON_LEVEL: AtomicU8 = AtomicU8::new(0);
const KEEP: u8 = 0;
/// A request of a strictly higher level is waiting.
const OUTRANKED: u8 = 1;
/// The client that asked has gone, so nobody is left to answer.
const DISCONNECTED: u8 = 2;
/// `quit_game` was asked.
const STOPPED: u8 = 3;

/// Set once the game has started to quit.
static QUITTING: AtomicBool = AtomicBool::new(false);

const STOPPED_MESSAGE: &str = "the game was stopped with quit_game";
const CANCELLED_CODE: &str = "cancelled";

/// Whether the running request is to stop where it is.
fn abandoning() -> bool {
    ABANDON.load(Ordering::SeqCst) != KEEP
}

/// The error code and message an abandoned request answers with.
fn abandonment() -> (&'static str, String) {
    match ABANDON.load(Ordering::SeqCst) {
        OUTRANKED => (
            EVICTED_CODE,
            format!(
                "a level {} request took the game",
                ABANDON_LEVEL.load(Ordering::SeqCst)
            ),
        ),
        STOPPED => (GAME_STOPPED_CODE, STOPPED_MESSAGE.to_owned()),
        _ => (
            CANCELLED_CODE,
            "the client closed its connection".to_owned(),
        ),
    }
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

    /// A tick's rows answer exactly the channels asked for: a capture that
    /// drops one, or adds one nobody asked for, fails the recording rather
    /// than publishing a channel that means something else.
    #[test]
    fn instrument_rows_answer_exactly_the_channels_asked_for() {
        let context = mechcore_mcfr::DurableContext {
            logic_step: mechcore_mcfr::Rational {
                numerator: 1,
                denominator: 10,
            },
            time_units_per_second: 10,
            combat_round: 1,
            match_seed: 1,
        };
        let mut writer = mechcore_mcfr::McfrWriter::hash_only(&context).unwrap();
        writer
            .append_tick(
                mechcore_mcfr::WorldSnapshot::default(),
                &mechcore_mcfr::TransitionEvents { events: Vec::new() },
            )
            .unwrap();
        let (code, error) = append_instrument(
            &mut writer,
            &[InstrumentChannel::TargetRefs],
            InstrumentRows::default(),
        )
        .unwrap_err();
        assert_eq!(code, "capture_failed");
        assert!(
            error.contains("omitted requested channel target_refs"),
            "{error}"
        );
        let unrequested = InstrumentRows {
            target_search: Some(Vec::new()),
            ..InstrumentRows::default()
        };
        let (_, error) = append_instrument(&mut writer, &[], unrequested).unwrap_err();
        assert!(
            error.contains("produced unrequested channel target_search"),
            "{error}"
        );
        let asked = InstrumentRows {
            skill_attackable_checker: Some(Vec::new()),
            ..InstrumentRows::default()
        };
        append_instrument(
            &mut writer,
            &[InstrumentChannel::SkillAttackableChecker],
            asked,
        )
        .unwrap();
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

    fn shared() -> Arc<Shared> {
        Arc::new(Shared {
            state: Mutex::new(State {
                scheduler: Scheduler::new(Instant::now()),
                writers: BTreeMap::new(),
                status: serde_json::json!({"status": "main_menu"}),
                stopping: None,
                scene_left: false,
                status_read: None,
            }),
            wake: Condvar::new(),
        })
    }

    /// Every claim is greeted: the game has many clients, and what one of
    /// them may do is decided per request, not per connection.
    #[test]
    fn every_claim_is_greeted_and_requests_wait_their_turn() {
        let path = PathBuf::from(format!(
            "/tmp/mechcore-adapter-claim-{}.sock",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        let listener = bind_listener(&path).unwrap();
        let shared = shared();
        let identity = GameIdentity {
            adapter: "ab".into(),
            headless: true,
            offline: true,
            linger_seconds: Some(30),
        };
        thread::spawn({
            let shared = Arc::clone(&shared);
            move || greet_clients(&listener, &shared, &identity)
        });

        let (mut first, answer) = claim_and_read(&path, 1, "batch");
        assert_eq!(answer["kind"], "hello");
        let (mut second, answer) = claim_and_read(&path, 1, "shell");
        assert_eq!(answer["kind"], "hello");

        // A round waits for its turn; the game is not ready yet.
        let answer = request(&mut first, 1, "record_replay_round");
        assert_eq!(
            answer,
            serde_json::json!({"kind": "queued", "id": 1, "position": 1})
        );
        // A scene operation needs the lease.
        let answer = request(&mut second, 1, "apply_layout");
        assert_eq!(answer["error"]["code"], "no_lease");
        // Status is what the game last said, without waiting.
        let answer = request(&mut second, 2, "status");
        assert_eq!(answer["result"]["status"], "main_menu");
        let answer = request(&mut second, 3, "queue");
        assert_eq!(answer["result"]["queued"][0]["client"], "batch");
        assert_eq!(answer["result"]["ready"], false);

        // A client that goes takes its waiting requests with it.
        drop(first);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let answer = request(&mut second, 4, "queue");
            if answer["result"]["queued"] == serde_json::json!([]) {
                assert_eq!(answer["result"]["clients"][0]["cancelled"], 1);
                break;
            }
            assert!(Instant::now() < deadline, "the closed client stayed queued");
            thread::sleep(Duration::from_millis(20));
        }

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
        drop(second);
        let _ = fs::remove_file(path);
    }

    fn claim_and_read(path: &Path, level: u8, client: &str) -> (UnixStream, Value) {
        let mut stream = UnixStream::connect(path).unwrap();
        write_json_line(&mut stream, &Claim::current(level, client)).unwrap();
        let answer = read_line(&stream);
        (stream, answer)
    }

    fn request(stream: &mut UnixStream, id: u64, operation: &str) -> Value {
        write_json_line(
            stream,
            &serde_json::json!({"id": id, "operation": operation, "arguments": {}}),
        )
        .unwrap();
        read_line(stream)
    }

    fn read_line(stream: &UnixStream) -> Value {
        let mut line = String::new();
        let mut byte = [0u8; 1];
        let mut stream = stream;
        while stream.read(&mut byte).unwrap() == 1 && byte[0] != b'\n' {
            line.push(byte[0] as char);
        }
        serde_json::from_str(&line).unwrap()
    }
}
