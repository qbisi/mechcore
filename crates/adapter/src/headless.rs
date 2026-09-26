//! A game started with no window and no graphics device.
//!
//! Unity reads `-batchmode` and `-nographics` off the game's command line, and
//! so does the Adapter: what it may do follows how the game was started, not
//! what a client says about it.

use std::ffi::OsStr;

/// Whether the game was started without a graphics device, so that no frame
/// is ever rendered.
pub fn without_graphics() -> bool {
    started_with("-nographics")
}

fn started_with(switch: &str) -> bool {
    std::env::args_os().any(|argument| argument == OsStr::new(switch))
}

/// Keep a game started with `-batchmode` out of the Dock.
///
/// Launch Services files a process as a Dock application or a background one
/// from its bundle's `Info.plist` when the process checks in, which is before
/// the game's first frame and after this library's initializer. A game without
/// a window has nothing to show there, so the in-memory copy of the dictionary
/// is marked `LSBackgroundOnly` before the check-in reads it; the bundle on
/// disk is untouched. Setting the activation policy once `AppKit` is up instead
/// left the icon in the Dock for the ten seconds the game takes to load.
#[cfg(target_os = "macos")]
pub fn keep_out_of_the_dock() {
    use std::ffi::c_void;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFBooleanTrue: *const c_void;
        fn CFBundleGetMainBundle() -> *mut c_void;
        fn CFBundleGetInfoDictionary(bundle: *mut c_void) -> *mut c_void;
        fn CFStringCreateWithCString(
            allocator: *const c_void,
            text: *const std::ffi::c_char,
            encoding: u32,
        ) -> *const c_void;
        fn CFDictionarySetValue(dictionary: *mut c_void, key: *const c_void, value: *const c_void);
        fn CFRelease(object: *const c_void);
    }
    const UTF8: u32 = 0x0800_0100;

    if !started_with("-batchmode") {
        return;
    }
    // SAFETY: the main bundle and its info dictionary live for the process.
    // CoreFoundation builds the dictionary mutable, which is what lets the
    // override take effect in memory; the key is released once the dictionary
    // has retained it.
    unsafe {
        let bundle = CFBundleGetMainBundle();
        if bundle.is_null() {
            return;
        }
        let info = CFBundleGetInfoDictionary(bundle);
        if info.is_null() {
            return;
        }
        let key = CFStringCreateWithCString(std::ptr::null(), c"LSBackgroundOnly".as_ptr(), UTF8);
        if key.is_null() {
            return;
        }
        CFDictionarySetValue(info, key, kCFBooleanTrue);
        CFRelease(key);
    }
}
