//! FFI bindings for C code used by Rylus.
//!
//! This crate compiles X11 capture C source files (when the `x11` feature is enabled)
//! and exposes their functions via `extern "C"` declarations.
//! The actual C code is compiled by this crate's build.rs.
//!
//! Video encoding has been moved to the `rylus-encode` crate (using ffmpeg-sys-next).

#[cfg(feature = "x11")]
use std::os::raw::{c_char, c_float, c_int, c_uchar, c_uint, c_void};

// Re-export CError for use in FFI function signatures
#[cfg(feature = "x11")]
pub use rylus_core::error::CError;

// ============================================================
// X11 capture FFI (from lib/linux/xcapture.c + xhelper.c)
// Only available when the `x11` feature is enabled.
// ============================================================

/// Image struct returned by X11 capture, matches `struct Image` in xcapture.c.
#[cfg(feature = "x11")]
#[repr(C)]
pub struct CImage {
    pub data: *const c_uchar,
    pub width: c_uint,
    pub height: c_uint,
}

// SAFETY: All X11 functions require `disp` to be a valid Display pointer from `XOpenDisplay`.
// `XLockDisplay`/`XUnlockDisplay` must bracket all display operations.
// `handles` buffer in `create_capturables` must have at least `size` entries.
#[cfg(all(target_os = "linux", feature = "x11"))]
extern "C" {
    pub fn XOpenDisplay(name: *const c_char) -> *mut c_void;
    pub fn XCloseDisplay(disp: *mut c_void) -> c_int;
    pub fn XInitThreads() -> c_int;
    pub fn XLockDisplay(disp: *mut c_void);
    pub fn XUnlockDisplay(disp: *mut c_void);

    pub fn x11_set_error_handler();

    pub fn create_capturables(
        disp: *mut c_void,
        handles: *mut *mut c_void,
        num_monitors: *mut c_int,
        size: c_int,
        err: *mut CError,
    ) -> c_int;

    pub fn clone_capturable(handle: *const c_void) -> *mut c_void;
    pub fn destroy_capturable(handle: *mut c_void);
    pub fn get_capturable_name(handle: *const c_void) -> *const c_char;
    pub fn capturable_before_input(handle: *mut c_void, err: *mut CError);
    pub fn get_geometry_relative(
        handle: *const c_void,
        x: *mut c_float,
        y: *mut c_float,
        width: *mut c_float,
        height: *mut c_float,
        err: *mut CError,
    );

    pub fn map_input_device_to_entire_screen(
        disp: *mut c_void,
        device_name: *const c_char,
        libinput: c_int,
        err: *mut CError,
    );
    pub fn start_capture(handle: *const c_void, ctx: *mut c_void, err: *mut CError) -> *mut c_void;
    // capture_screen is declared locally in rylus-capture/x11.rs
    // because it uses a local CImage type with helper methods
    pub fn stop_capture(handle: *mut c_void, err: *mut CError);
}

// uinput FFI has been replaced by the evdev crate in rylus-input
