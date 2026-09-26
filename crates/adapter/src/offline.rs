//! A game with no network.
//!
//! `mechcore` takes a game's network away by starting it in a sandbox that
//! refuses every IP connection, so the Adapter reads whether the process is
//! sandboxed rather than trusting a client to say it is: no other launch puts
//! the game in one.

/// Whether the game runs in a sandbox.
#[cfg(target_os = "macos")]
pub fn sandboxed() -> bool {
    unsafe extern "C" {
        fn sandbox_check(
            pid: libc::pid_t,
            operation: *const std::ffi::c_char,
            kind: i32,
            ...
        ) -> i32;
    }
    const SANDBOX_FILTER_NONE: i32 = 0;
    // SAFETY: with no operation named, sandbox_check only asks whether the
    // process is sandboxed at all, and reads no further arguments.
    unsafe { sandbox_check(libc::getpid(), std::ptr::null(), SANDBOX_FILTER_NONE) != 0 }
}

#[cfg(not(target_os = "macos"))]
pub fn sandboxed() -> bool {
    false
}
