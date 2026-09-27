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

/// Let a game without graphics log in.
///
/// With `-nographics` the screen reads 640x480, below the smallest resolution
/// the game supports, and `StartUpCommand.OnPlatformInitCb` then takes the
/// branch of `VideoSetting.TryFixResolution()` returning true: it asks for a
/// restart in a popup and returns without opening the login window, so the
/// lobby's proxies are never registered and nothing can be watched. The
/// method is answered `false`, as a game whose screen is large enough gets,
/// and the boot goes on unchanged; `-screen-width` and `-screen-height` are
/// not honoured without graphics.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) fn skip_the_resolution_check(api: crate::il2cpp::Api) -> Result<(), String> {
    use std::sync::atomic::AtomicPtr;

    static ORIGINAL: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

    unsafe extern "C" fn resolution_is_fine(
        _setting: *mut crate::il2cpp::Object,
        _method: *const crate::il2cpp::MethodInfo,
    ) -> bool {
        false
    }

    if !without_graphics() {
        return Ok(());
    }
    let method = api
        .class("GRCore.dll", "GameRiver", "VideoSetting")
        .and_then(|class| api.method(class, "TryFixResolution", 0))
        .map_err(|error| error.to_string())?;
    crate::capture::install_inline_hook(
        api,
        method,
        resolution_is_fine as *const std::ffi::c_void,
        &ORIGINAL,
        "VideoSetting.TryFixResolution",
    )
}
