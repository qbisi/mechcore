use crate::il2cpp::{Api, Class, Error as Il2CppError, FieldInfo, Object};
use crate::operations;
use crate::protocol::{CAPABILITIES, Hello, Request, Response};
use serde_json::Value;
use std::env;
use std::ffi::c_void;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

const SOCKET_ENV: &str = "MECHCORE_ADAPTER_SOCKET";
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
const LAYOUT_SERIES_TIMEOUT: Duration = Duration::from_secs(55);
const LAYOUT_STATUS_INTERVAL: Duration = Duration::from_millis(50);
const LAYOUT_DEPLOYMENT_STABLE_SAMPLES: usize = 3;

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
    action: MainAction,
    response: Option<Response<Value>>,
}

#[derive(Clone, Copy)]
enum MainAction {
    Public,
    Internal,
    Layout(operations::LayoutExecutionStage),
}

struct RuntimeLoadInvocation {
    api: Api,
    result: Option<Result<Runtime, RuntimeError>>,
}

extern "C" fn load_runtime_on_main(context: *mut c_void) {
    // SAFETY: dispatch_sync_f invokes this callback before returning, while the
    // stack-owned invocation remains alive.
    let invocation = unsafe { &mut *context.cast::<RuntimeLoadInvocation>() };
    invocation.result = Some(Runtime::load(invocation.api));
}

extern "C" fn invoke_on_main(context: *mut c_void) {
    // SAFETY: dispatch_sync_f invokes this callback before returning, while the
    // stack-owned invocation, runtime and request remain alive.
    let invocation = unsafe { &mut *context.cast::<MainInvocation>() };
    // SAFETY: pointers are supplied by execute_on_main and remain valid for the
    // synchronous callback duration.
    let runtime = unsafe { &mut *invocation.runtime };
    let request = unsafe { &*invocation.request };
    invocation.response = Some(match invocation.action {
        MainAction::Public => operations::execute(runtime, request),
        MainAction::Internal => operations::execute_internal(runtime, request),
        MainAction::Layout(stage) => operations::execute_layout_stage(runtime, request, stage),
    });
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    static _dispatch_main_q: c_void;
    fn dispatch_sync_f(queue: *mut c_void, context: *mut c_void, work: extern "C" fn(*mut c_void));
}

fn execute_on_main(runtime: &mut Runtime, request: &Request) -> Response<Value> {
    execute_action_on_main(runtime, request, MainAction::Public)
}

fn execute_internal_on_main(runtime: &mut Runtime, request: &Request) -> Response<Value> {
    execute_action_on_main(runtime, request, MainAction::Internal)
}

fn execute_layout_stage_on_main(
    runtime: &mut Runtime,
    request: &Request,
    stage: operations::LayoutExecutionStage,
) -> Response<Value> {
    execute_action_on_main(runtime, request, MainAction::Layout(stage))
}

fn execute_action_on_main(
    runtime: &mut Runtime,
    request: &Request,
    action: MainAction,
) -> Response<Value> {
    let mut invocation = MainInvocation {
        runtime,
        request,
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
            request.id,
            "main_thread_dispatch_failed",
            "main-thread callback returned no response",
        )
    })
}

fn load_runtime_on_main_thread(api: Api) -> Result<Runtime, RuntimeError> {
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
        invocation.result = Some(Runtime::load(api));
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
    let _cleanup = SocketCleanup(endpoint);

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(error) = serve_client(&mut runtime, stream) {
                    eprintln!("mechcore-adapter: client disconnected: {error}");
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
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
    let hello = Hello {
        kind: "hello",
        protocol: crate::protocol::PROTOCOL,
        capabilities: CAPABILITIES,
    };
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
        let response = if request.operation == "apply_layout" {
            execute_layout_series(runtime, &request)
        } else {
            execute_on_main(runtime, &request)
        };
        write_json_line(&mut stream, &response)?;
    }
}

fn execute_layout_series(runtime: &mut Runtime, request: &Request) -> Response<Value> {
    let deadline = Instant::now() + LAYOUT_SERIES_TIMEOUT;
    let prepare =
        execute_layout_stage_on_main(runtime, request, operations::LayoutExecutionStage::Prepare);
    let prepare = match successful_result(prepare) {
        Ok(result) => result,
        Err(response) => return response,
    };
    let Some(target_round) = prepare.get("target_round").and_then(Value::as_i64) else {
        return Response::failure(
            request.id,
            "invalid_adapter_response",
            "prepare layout stage omitted target_round",
        );
    };
    let Some(formation_count) = prepare.get("formation_count").and_then(Value::as_u64) else {
        return Response::failure(
            request.id,
            "invalid_adapter_response",
            "prepare layout stage omitted formation_count",
        );
    };

    let mut current_round = 1_i64;
    let mut skipped_rounds = Vec::new();
    let mut stages = vec![prepare];
    while current_round < target_round {
        let is_pre_activation = is_pre_activation_round(current_round, target_round);
        if is_pre_activation {
            let pre_activation = execute_layout_stage_on_main(
                runtime,
                request,
                operations::LayoutExecutionStage::PreActivation,
            );
            match successful_result(pre_activation) {
                Ok(result) => stages.push(result),
                Err(response) => return response,
            }
        }
        if let Err(response) =
            advance_layout_round(runtime, request, current_round, is_pre_activation, deadline)
        {
            return response;
        }
        skipped_rounds.push(current_round);
        current_round += 1;
    }

    let activation = execute_layout_stage_on_main(
        runtime,
        request,
        operations::LayoutExecutionStage::Activation,
    );
    match successful_result(activation) {
        Ok(result) => stages.push(result),
        Err(response) => return response,
    }
    Response::success(
        request.id,
        serde_json::json!({
            "applied": true,
            "round": target_round,
            "formation_count": formation_count,
            "skipped_rounds": skipped_rounds,
            "stages": stages,
        }),
    )
}

const fn is_pre_activation_round(current_round: i64, target_round: i64) -> bool {
    target_round > 2 && current_round + 1 == target_round
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
    request: &Request,
    round: i64,
    finish_if_fighting: bool,
    deadline: Instant,
) -> Result<(), Response<Value>> {
    let toggle = Request {
        id: request.id,
        operation: "toggle_fight".into(),
        arguments: Value::Null,
    };
    successful_result(execute_internal_on_main(runtime, &toggle))?;

    if finish_if_fighting {
        let transition = wait_layout_status(
            runtime,
            request.id,
            deadline,
            &format!("round {round} fight start"),
            1,
            |status| {
                is_training_state(status, round, false, true)
                    || is_training_state(status, round + 1, true, false)
            },
        )?;
        if is_training_state(&transition, round, false, true) {
            let finish = Request {
                id: request.id,
                operation: "finish_preparation_fight".into(),
                arguments: serde_json::json!({"expected_round": round}),
            };
            successful_result(execute_internal_on_main(runtime, &finish))?;
        }
    }

    wait_layout_status(
        runtime,
        request.id,
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
    let status_request = Request {
        id: request_id,
        operation: "status".into(),
        arguments: Value::Null,
    };
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
        last = successful_result(execute_internal_on_main(runtime, &status_request))?;
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

fn is_training_state(status: &Value, round: i64, deploying: bool, fighting: bool) -> bool {
    status.get("status").and_then(Value::as_str) == Some("training_ground")
        && status.get("round_count").and_then(Value::as_i64) == Some(round)
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
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn only_the_round_before_activation_uses_pre_activation_actions() {
        assert!(!is_pre_activation_round(1, 1));
        assert!(!is_pre_activation_round(1, 2));
        for target in 3..=6 {
            for current in 1..target {
                assert_eq!(
                    is_pre_activation_round(current, target),
                    current == target - 1
                );
            }
        }
    }
}
