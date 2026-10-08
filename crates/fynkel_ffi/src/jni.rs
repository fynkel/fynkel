//! The calls of package:fynkel_flutter's Kotlin plugin (its class
//! `dev.fynkel.fynkel_flutter.FynkelNative`). An Android `Surface` is a Java
//! object, so it can only be handed over through JNI; everything else goes
//! through the `fy_*` functions from Dart.
//!
//! A view is passed as the `long` address Dart's `AndroidSurfaceView` gave the
//! plugin. None of these may unwind into the JVM: a panic becomes an error
//! message or is swallowed.

use std::ffi::CString;
use std::panic::{self, AssertUnwindSafe};
use std::ptr;

use fynkel_present::AndroidPresenter;
use jni_sys::{JNIEnv, jclass, jint, jlong, jobject, jstring};

use crate::abi::{FyView, fy_view_free};
use crate::worker::panic_message;

/// The Android side of the view at `view`.
///
/// # Safety
///
/// `view` is 0 or the address of a live view.
unsafe fn presenter<'a>(view: jlong) -> Result<&'a AndroidPresenter, String> {
    // SAFETY: per this function's contract.
    let view = unsafe { (view as usize as *const FyView).as_ref() }.ok_or("the view is null")?;
    view.android().ok_or_else(|| "not an Android view".to_owned())
}

/// Runs `body`, turning a panic into its message.
fn catch(body: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    panic::catch_unwind(AssertUnwindSafe(body))
        .unwrap_or_else(|payload| Err(format!("panicked: {}", panic_message(&*payload))))
}

/// `FynkelNative.attach(view, surface, width, height)`: the view's next
/// frames go to `surface`, at the size its `SurfaceProducer` was given.
/// Waits for a frame in progress. Returns null, or why it failed.
///
/// # Safety
///
/// Called by the JVM: `env` is this thread's, `surface` an
/// `android.view.Surface`, and `view` 0 or a live view.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_fynkel_fynkel_1flutter_FynkelNative_attach(
    env: *mut JNIEnv,
    _class: jclass,
    view: jlong,
    surface: jobject,
    width: jint,
    height: jint,
) -> jstring {
    let result = catch(|| {
        // SAFETY: per this function's contract.
        let presenter = unsafe { presenter(view) }?;
        let (Ok(w @ 1..), Ok(h @ 1..)) = (u32::try_from(width), u32::try_from(height)) else {
            return Err(format!("a surface can't be {width} × {height}"));
        };
        // SAFETY: per this function's contract.
        unsafe { presenter.attach(env.cast(), surface.cast(), w, h) }.map_err(|e| e.to_string())
    });
    match result {
        Ok(()) => ptr::null_mut(),
        // SAFETY: `env` is this thread's.
        Err(message) => unsafe { new_string(env, &message) },
    }
}

/// `FynkelNative.detach(view)`: frames stop going to the surface, after
/// the one in progress. Afterwards the surface may be released.
///
/// # Safety
///
/// `view` is 0 or a live view.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_fynkel_fynkel_1flutter_FynkelNative_detach(
    _env: *mut JNIEnv,
    _class: jclass,
    view: jlong,
) {
    // Nothing to report: a view that isn't one has no surface either.
    let _ = catch(|| {
        // SAFETY: per this function's contract.
        unsafe { presenter(view) }?.detach();
        Ok(())
    });
}

/// `FynkelNative.free(view)`: `fy_view_free`, for the views of an isolate
/// that is gone (a hot restart disposes nothing).
///
/// # Safety
///
/// `view` is 0 or a live view, not used again after this.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_fynkel_fynkel_1flutter_FynkelNative_free(
    _env: *mut JNIEnv,
    _class: jclass,
    view: jlong,
) {
    let _ = catch(|| {
        // SAFETY: per this function's contract.
        unsafe { fy_view_free(view as usize as *mut FyView) };
        Ok(())
    });
}

/// `message` as a Java string, or null if even that fails.
///
/// # Safety
///
/// `env` is this thread's `JNIEnv`.
unsafe fn new_string(env: *mut JNIEnv, message: &str) -> jstring {
    // JNI reads "modified UTF-8": the same as UTF-8 here, with the nul
    // bytes gone and the engine's messages within the BMP.
    let Ok(message) = CString::new(message.replace('\0', "")) else { return ptr::null_mut() };
    // SAFETY: per this function's contract; every JVM has NewStringUTF.
    unsafe {
        match (**env).NewStringUTF {
            Some(new_string_utf) => new_string_utf(env, message.as_ptr()),
            None => ptr::null_mut(),
        }
    }
}
