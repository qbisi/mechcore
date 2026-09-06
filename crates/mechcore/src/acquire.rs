//! Game acquisition: detection, classification, launch.
//!
//! Implements the state matrix in `docs/session.md`. Acquisition is always
//! explicitly declared: nothing here falls back from attach to launch, and no
//! native operation reaches this module on its own.

use crate::adapter::{Client, ConnectError};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::{Child, Command};

/// `PROC_ALL_PIDS` from `sys/proc_info.h`; not re-exported by the `libc` crate.
const PROC_ALL_PIDS: u32 = 1;
const GREETING_DEADLINE: Duration = Duration::from_secs(3);
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(60);
const LAUNCH_POLL_INTERVAL: Duration = Duration::from_millis(200);
const ADAPTER_DYLIB: &str = "libmechcore_adapter.dylib";
const GAME_ENV: &str = "MECHCORE_GAME";
const DEFAULT_GAME_SUFFIX: &str =
    "Library/Application Support/Steam/steamapps/common/Mechabellum/Mechabellum.app/Contents/MacOS/Mechabellum";

/// Which acquisition the caller declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Launch,
    Attach,
}

impl Mode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Launch => "launch",
            Self::Attach => "attach",
        }
    }
}

/// Whether this process is responsible for shutting the game down.
///
/// Never inferred: `launch` owns the process it started, `attach` never owns
/// the process it found.
pub(crate) enum Ownership {
    Owned(Child),
    Attached,
}

impl Ownership {
    pub(crate) const fn is_owned(&self) -> bool {
        matches!(self, Self::Owned(_))
    }
}

/// A refused acquisition, carrying the stable code from `docs/session.md`.
pub(crate) struct Failure {
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

impl Failure {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

/// What a single connection attempt observed at the endpoint.
enum Probe {
    NoListener,
    Idle(Box<Client>),
    Busy,
    Unresponsive(String),
    Protocol(String),
}

/// A live Mechabellum process.
pub(crate) struct GameProcess {
    pub(crate) pid: u32,
    pub(crate) path: PathBuf,
}

/// Acquire the game as declared, returning the connected client and ownership.
pub(crate) async fn acquire(
    mode: Mode,
    endpoint: &Path,
) -> Result<(Client, Ownership), Box<Failure>> {
    let running = game_processes();
    let probe = probe_endpoint(endpoint).await;

    match (mode, running.first(), probe) {
        // E and F apply to both verbs: something holds the endpoint, or the
        // adapter stopped answering. Neither verb may proceed.
        (_, _, Probe::Busy) => Err(Box::new(Failure::new(
            "adapter_busy",
            format!(
                "another mechcore process is serving {}",
                endpoint.display()
            ),
        ))),
        (_, _, Probe::Unresponsive(detail)) => Err(Box::new(Failure::new(
            "adapter_unresponsive",
            format!(
                "connected to {} but no greeting arrived within {}s: {detail}",
                endpoint.display(),
                GREETING_DEADLINE.as_secs()
            ),
        ))),
        (_, _, Probe::Protocol(detail)) => {
            Err(Box::new(Failure::new("protocol_mismatch", detail)))
        }

        // C: a game the player started, with no adapter injected. Injection is
        // impossible after start, and a second instance would corrupt both.
        (_, Some(process), Probe::NoListener) => Err(Box::new(Failure::new(
            "foreign_game",
            format!(
                "Mechabellum is running without the Adapter (pid {}, {}); \
                 no endpoint at {}. Quit that game first, or launch it through mechcore.",
                process.pid,
                process.path.display(),
                endpoint.display()
            ),
        ))),

        // D: an idle adapter is available. attach takes it; launch refuses
        // rather than silently degrading into an attach.
        (Mode::Attach, _, Probe::Idle(client)) => Ok((*client, Ownership::Attached)),
        (Mode::Launch, running, Probe::Idle(_)) => {
            let detail = running.map_or_else(
                || "an adapter is already serving".to_string(),
                |process| format!("Mechabellum is already running (pid {})", process.pid),
            );
            Err(Box::new(Failure::new(
                "already_running",
                format!("{detail}; use attach to join it"),
            )))
        }

        // A and B: nothing to join.
        // A and B are the same refusal for attach: whether the endpoint file
        // is absent or merely stale, nothing is listening to join.
        (Mode::Attach, None, Probe::NoListener) => Err(Box::new(Failure::new(
            "no_game",
            format!(
                "no adapter listening at {} ({})",
                endpoint.display(),
                if endpoint.exists() {
                    "stale endpoint, no game running"
                } else {
                    "no endpoint, no game running"
                }
            ),
        ))),
        (Mode::Launch, None, Probe::NoListener) => launch(endpoint).await,
    }
}

/// Start the game with the sibling Adapter and wait for its endpoint.
async fn launch(endpoint: &Path) -> Result<(Client, Ownership), Box<Failure>> {
    let dylib = adapter_dylib()?;
    let game = game_executable()?;
    let child = Command::new(&game)
        .env("DYLD_INSERT_LIBRARIES", &dylib)
        .env("MECHCORE_ADAPTER_SOCKET", endpoint)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| {
            Box::new(Failure::new(
                "launch_failed",
                format!("cannot start {}: {error}", game.display()),
            ))
        })?;

    let deadline = tokio::time::Instant::now() + LAUNCH_TIMEOUT;
    loop {
        match probe_endpoint(endpoint).await {
            Probe::Idle(client) => return Ok((*client, Ownership::Owned(child))),
            Probe::Protocol(detail) => {
                return Err(Box::new(Failure::new("protocol_mismatch", detail)));
            }
            Probe::Busy => {
                return Err(Box::new(Failure::new(
                    "adapter_busy",
                    format!(
                        "the game we started at {} was taken by another client",
                        endpoint.display()
                    ),
                )));
            }
            Probe::NoListener | Probe::Unresponsive(_) => {}
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(Box::new(Failure::new(
                "launch_failed",
                format!(
                    "started {} but no adapter endpoint appeared at {} within {}s",
                    game.display(),
                    endpoint.display(),
                    LAUNCH_TIMEOUT.as_secs()
                ),
            )));
        }
        tokio::time::sleep(LAUNCH_POLL_INTERVAL).await;
    }
}

/// One connection attempt, bounded by the greeting deadline.
async fn probe_endpoint(endpoint: &Path) -> Probe {
    match tokio::time::timeout(GREETING_DEADLINE, Client::connect(endpoint)).await {
        Ok(Ok(client)) => Probe::Idle(Box::new(client)),
        Ok(Err(ConnectError::Busy)) => Probe::Busy,
        Ok(Err(ConnectError::Unavailable(detail))) => {
            let _ = detail;
            Probe::NoListener
        }
        Ok(Err(ConnectError::Unresponsive(detail))) => Probe::Unresponsive(detail),
        Ok(Err(ConnectError::Protocol(detail))) => Probe::Protocol(detail),
        Err(_) => Probe::Unresponsive("greeting deadline elapsed".into()),
    }
}

/// The Adapter shipped beside the running executable.
///
/// Resolving it as a sibling keeps a built target directory relocatable.
fn adapter_dylib() -> Result<PathBuf, Box<Failure>> {
    let executable = std::env::current_exe().map_err(|error| {
        Box::new(Failure::new(
            "launch_failed",
            format!("cannot resolve the running executable: {error}"),
        ))
    })?;
    let directory = executable.parent().ok_or_else(|| {
        Box::new(Failure::new(
            "launch_failed",
            format!("{} has no parent directory", executable.display()),
        ))
    })?;
    let dylib = directory.join(ADAPTER_DYLIB);
    if !dylib.is_file() {
        return Err(Box::new(Failure::new(
            "launch_failed",
            format!("no Adapter beside the executable: {}", dylib.display()),
        )));
    }
    Ok(dylib)
}

/// `MECHCORE_GAME`, then the default Steam location.
fn game_executable() -> Result<PathBuf, Box<Failure>> {
    let mut tried = Vec::new();
    if let Some(configured) = std::env::var_os(GAME_ENV) {
        let path = PathBuf::from(configured);
        if path.is_file() {
            return Ok(path);
        }
        tried.push(format!("{GAME_ENV}={}", path.display()));
    }
    if let Some(home) = std::env::var_os("HOME") {
        let path = PathBuf::from(home).join(DEFAULT_GAME_SUFFIX);
        if path.is_file() {
            return Ok(path);
        }
        tried.push(path.display().to_string());
    }
    Err(Box::new(Failure::new(
        "launch_failed",
        format!("no game executable found; tried {}", tried.join(", ")),
    )))
}

/// Every running process whose executable is the Mechabellum binary.
pub(crate) fn game_processes() -> Vec<GameProcess> {
    let mut processes = Vec::new();
    for pid in all_pids() {
        let Some(path) = process_path(pid) else {
            continue;
        };
        if path.file_name().is_some_and(|name| name == "Mechabellum") {
            processes.push(GameProcess { pid, path });
        }
    }
    processes
}

fn all_pids() -> Vec<u32> {
    // SAFETY: a null buffer asks only for the required byte count.
    let bytes = unsafe { libc::proc_listpids(PROC_ALL_PIDS, 0, std::ptr::null_mut(), 0) };
    let Ok(bytes) = usize::try_from(bytes) else {
        return Vec::new();
    };
    let capacity = bytes / std::mem::size_of::<libc::c_int>();
    let mut pids = vec![0 as libc::c_int; capacity];
    let Ok(buffer_size) = libc::c_int::try_from(bytes) else {
        return Vec::new();
    };
    // SAFETY: the buffer holds `buffer_size` bytes, matching the reported need.
    let written = unsafe {
        libc::proc_listpids(
            PROC_ALL_PIDS,
            0,
            pids.as_mut_ptr().cast::<c_void>(),
            buffer_size,
        )
    };
    let Ok(written) = usize::try_from(written) else {
        return Vec::new();
    };
    pids.truncate(written / std::mem::size_of::<libc::c_int>());
    pids.into_iter().filter_map(|pid| u32::try_from(pid).ok()).collect()
}

fn process_path(pid: u32) -> Option<PathBuf> {
    let pid = libc::c_int::try_from(pid).ok()?;
    let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let length = u32::try_from(buffer.len()).ok()?;
    // SAFETY: the buffer is writable for `length` bytes and outlives the call.
    let written = unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast::<c_void>(), length) };
    let written = usize::try_from(written).ok()?;
    if written == 0 {
        return None;
    }
    buffer.truncate(written);
    String::from_utf8(buffer).ok().map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_enumeration_finds_this_process() {
        // The enumeration itself must work even when no game is running; our
        // own pid proves the libproc path resolves.
        let own = process_path(std::process::id()).expect("own executable path");
        assert!(own.is_absolute(), "{own:?} should be absolute");
        assert!(!all_pids().is_empty());
    }

    #[test]
    fn game_detection_is_specific_to_the_binary_name() {
        // Nothing is asserted about whether the game runs here; the contract is
        // only that every reported process really is the Mechabellum binary.
        for process in game_processes() {
            assert_eq!(process.path.file_name().unwrap(), "Mechabellum");
            assert!(process.pid > 0);
        }
    }

    #[test]
    fn failure_codes_match_the_documented_taxonomy() {
        let failure = Failure::new("foreign_game", "detail");
        assert_eq!(failure.code, "foreign_game");
        assert_eq!(failure.to_string(), "foreign_game: detail");
    }

    #[test]
    fn mode_names_are_the_declaration_spellings() {
        assert_eq!(Mode::Launch.as_str(), "launch");
        assert_eq!(Mode::Attach.as_str(), "attach");
    }
}
