//! In-process Mechabellum adapter.
//!
//! The `cdylib` is loaded into the game process. It exposes a Unix socket for
//! MCP clients and executes validated operations on Unity's main thread.

mod il2cpp;
mod layout;
mod operations;
mod runtime;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::thread;

#[cfg(target_os = "macos")]
#[used]
#[unsafe(link_section = "__DATA,__mod_init_func")]
static INITIALIZER: extern "C" fn() = initialize;

extern "C" fn initialize() {
    // Never execute Rust initialization or IL2CPP work in dyld's loader lock.
    // The worker waits for runtime exports, then resolves metadata and executes
    // requests on Unity's already-attached main thread.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if let Err(error) = thread::Builder::new()
            .name("mechcore-adapter".into())
            .spawn(runtime::worker)
        {
            eprintln!("cannot start mechcore adapter worker: {error}");
        }
    }));
}
