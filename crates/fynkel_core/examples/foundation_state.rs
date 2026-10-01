//! Explicit F0 state candidate library. No production ABI exports.
mod state_kernel;
use serde_json::{json, Value};
use state_kernel::{Kernel, Strategy};
use std::panic::{catch_unwind, AssertUnwindSafe};

struct Host {
    kernel: Kernel,
    output: Vec<u8>,
}

#[no_mangle]
pub extern "C" fn foundation_state_create(adapter: u32) -> *mut std::ffi::c_void {
    catch_unwind(|| {
        Some(Box::into_raw(Box::new(Host {
            kernel: Kernel::new(if adapter == 0 {
                Strategy::Shared
            } else {
                Strategy::Adapter
            }),
            output: Vec::new(),
        })) as *mut std::ffi::c_void)
    })
    .ok()
    .flatten()
    .unwrap_or(std::ptr::null_mut())
}

/// # Safety
/// Exclusive live allocation, readable input of len bytes and writable output_len.
/// Output is borrowed until the next call/disposal; copy before retaining it.
#[no_mangle]
pub unsafe extern "C" fn foundation_state_call(
    pointer: *mut std::ffi::c_void,
    input: *const u8,
    len: usize,
    output_len: *mut usize,
) -> *const u8 {
    if pointer.is_null() || input.is_null() || output_len.is_null() {
        return std::ptr::null();
    }
    let host = &mut *(pointer as *mut Host);
    let result = catch_unwind(AssertUnwindSafe(|| {
        let request: Value = serde_json::from_slice(std::slice::from_raw_parts(input, len))
            .map_err(|e| e.to_string())?;
        host.kernel.call(request)
    }));
    let response = match result {
        Ok(Ok(value)) => json!({"ok":value}),
        Ok(Err(error)) => json!({"error":error}),
        Err(_) => {
            host.kernel.core.poison();
            json!({"error":"Native panic; state kernel poisoned"})
        }
    };
    host.output = serde_json::to_vec(&response).unwrap();
    *output_len = host.output.len();
    host.output.as_ptr()
}

/// # Safety
/// Dispose a live allocation once, with no concurrent calls or outstanding views.
#[no_mangle]
pub unsafe extern "C" fn foundation_state_dispose(pointer: *mut std::ffi::c_void) {
    if !pointer.is_null() {
        drop(Box::from_raw(pointer as *mut Host));
    }
}
