//! What the engine's C interface (`fy_*`, crates/fynkel_ffi) and the
//! physics' (`fp_*`, crates/fynkel_physics_ffi) share. Each library links
//! its own copy, so each keeps its own last error.
//!
//! # The convention
//!
//! - Every call that can fail returns a status (`FyStatus`, `FpStatus`)
//!   and writes its results through out-pointers. 0 is success. The
//!   statuses both have use the same numbers ([`status`]); each library's
//!   own start at 1 and stay clear of those.
//! - A failed call leaves a message for `fy_last_error` / `fp_last_error`,
//!   per thread, until the next failed call ([`guard`], [`last_error`]).
//! - A panic never crosses into the caller: it becomes the `Panic`
//!   status. A panic while changing a world poisons it.
//! - Ids are `uint64_t`, and 0 means "none".
//! - Structs go by pointer; a list comes back as a count, then a `_read`
//!   call into a buffer the caller made of that size.
//! - Flags: the engine's are `bool` (Dart writes only 0 or 1); the
//!   physics' are `uint32_t` 0 or 1, and any other value is refused.
//! - Any change to the functions or types bumps the library's ABI version:
//!   each header test fails if the header changed and the version didn't
//!   ([`check_header`]).

use std::any::Any;
use std::cell::RefCell;
use std::ffi::{CString, c_char};
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;

/// The numbers of the statuses both interfaces have. `FyStatus` and
/// `FpStatus` give these the same values, and their tests say so.
pub mod status {
    pub const OK: i32 = 0;
    /// A value was out of range, not finite, or not one of the choices.
    pub const INVALID_VALUE: i32 = 6;
    /// A pointer argument was null.
    pub const NULL_POINTER: i32 = 7;
    /// A buffer had the wrong size.
    pub const WRONG_SIZE: i32 = 11;
    /// The call panicked.
    pub const PANIC: i32 = 12;
    /// An earlier call panicked while changing this world.
    pub const POISONED: i32 = 13;
}

/// Where the fields of an object's transform are, in bytes: the engine's
/// `FyTransform` and the physics' `FpTransform` both have this layout, so
/// one buffer serves both libraries. Their tests say so.
pub mod transform {
    pub const SIZE: usize = 40;
    pub const POSITION: usize = 0;
    pub const ROTATION: usize = 12;
    pub const SCALE: usize = 28;
}

/// A library's status type: what [`guard`] returns.
pub trait Status: Copy {
    const OK: Self;
    const PANIC: Self;
    const NULL_POINTER: Self;
}

/// A failed call: its status and its message.
#[derive(Debug, PartialEq, Eq)]
pub struct Error<S> {
    pub status: S,
    pub message: String,
}

impl<S: Status> Error<S> {
    pub fn new(status: S, message: impl Into<String>) -> Self {
        Self { status, message: message.into() }
    }

    /// A null pointer argument, named.
    pub fn null(what: &str) -> Self {
        Self::new(S::NULL_POINTER, format!("{what} is null"))
    }
}

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

/// Runs the body of a C call: a panic becomes `S::PANIC` and never unwinds
/// into the caller, and any error's message is kept for [`last_error`].
pub fn guard<S: Status>(body: impl FnOnce() -> Result<(), Error<S>>) -> S {
    let error = match panic::catch_unwind(AssertUnwindSafe(body)) {
        Ok(Ok(())) => return S::OK,
        Ok(Err(error)) => error,
        Err(payload) => Error::new(S::PANIC, format!("panicked: {}", panic_message(&*payload))),
    };
    let message = CString::new(error.message.replace('\0', "")).expect("nul bytes removed");
    LAST_ERROR.with(|last| *last.borrow_mut() = message);
    error.status
}

/// The message of the last failed call on this thread ("" before any).
/// Valid until the next failed call on this thread.
pub fn last_error() -> *const c_char {
    LAST_ERROR.with(|last| last.borrow().as_ptr())
}

/// What a panic said, if it said it with a string.
pub fn panic_message(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("(no message)")
}

/// For a header test: `generated` is what cbindgen makes of the crate now,
/// `header` the committed header, with `abi.lock` next to it holding the
/// ABI version and a hash of the header it was last blessed with (its
/// comments left out). Fails if the header is out of date, or if its code
/// changed and `version` didn't.
/// `bless` (`FYNKEL_BLESS=1`) writes both, after the same version check.
pub fn check_header(header: &Path, generated: &str, version: u32, bless: bool, regenerate: &str) {
    let lock = header.with_file_name("abi.lock");
    // Comments don't change the interface: a doc fix needs no new version.
    let code = generated.lines().filter(|line| !line.trim_start().starts_with("//") && !line.contains("_ABI_VERSION "));
    let hash = fnv1a(code.collect::<Vec<_>>().join("\n"));
    let locked = std::fs::read_to_string(&lock).unwrap_or_default();
    let (locked_version, locked_hash) = locked.trim().split_once(' ').unwrap_or(("", ""));
    if locked_hash != format!("{hash:016x}") && locked_version == version.to_string() {
        panic!("{} changed but its ABI version is still {version}: bump it, then bless the header", header.display());
    }
    if bless {
        std::fs::write(header, generated).unwrap();
        std::fs::write(&lock, format!("{version} {hash:016x}\n")).unwrap();
        return;
    }
    let current = std::fs::read_to_string(header).unwrap_or_default();
    assert!(
        current == generated && locked_version == version.to_string(),
        "{} is out of date; {regenerate}",
        header.display()
    );
}

fn fnv1a(text: String) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3))
}

#[cfg(test)]
mod tests {
    use std::ffi::CStr;

    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum TestStatus {
        Ok,
        Null,
        Panic,
        Timeout,
    }

    impl Status for TestStatus {
        const OK: Self = Self::Ok;
        const PANIC: Self = Self::Panic;
        const NULL_POINTER: Self = Self::Null;
    }

    fn message() -> String {
        // SAFETY: last_error points at the thread's current message, which
        // lives until the next failed call.
        #[allow(unsafe_code)]
        unsafe { CStr::from_ptr(last_error()) }.to_str().unwrap().to_owned()
    }

    #[test]
    fn a_panic_becomes_a_status_and_a_message() {
        assert_eq!(guard::<TestStatus>(|| panic!("the {} broke", "thing")), TestStatus::Panic);
        assert_eq!(message(), "panicked: the thing broke");
    }

    #[test]
    fn an_error_keeps_its_message_and_success_keeps_the_last_one() {
        assert_eq!(guard(|| Err(Error::new(TestStatus::Timeout, "too slow"))), TestStatus::Timeout);
        assert_eq!(guard::<TestStatus>(|| Ok(())), TestStatus::Ok);
        assert_eq!(message(), "too slow");
        assert_eq!(guard(|| Err(Error::<TestStatus>::null("world"))), TestStatus::Null);
        assert_eq!(message(), "world is null");
    }

    #[test]
    fn a_message_with_a_nul_byte_still_arrives() {
        guard(|| Err(Error::new(TestStatus::Timeout, "a\0b")));
        assert_eq!(message(), "ab");
    }

    fn header_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fynkel_cabi_{name}_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("x.h")
    }

    #[test]
    fn a_header_that_changed_without_a_new_version_fails() {
        let header = header_dir("bump");
        check_header(&header, "#define X_ABI_VERSION 1\nint a(void);\n", 1, true, "bless");
        check_header(&header, "#define X_ABI_VERSION 1\nint a(void);\n", 1, false, "bless");
        let changed = "#define X_ABI_VERSION 1\nint a(int);\n";
        let refused = std::panic::catch_unwind(|| check_header(&header, changed, 1, true, "bless"));
        assert!(refused.is_err(), "blessing a change without a bump is refused");
        check_header(&header, "#define X_ABI_VERSION 1\n// says more\nint a(void);\n", 1, true, "bless");
        check_header(&header, "#define X_ABI_VERSION 2\nint a(int);\n", 2, true, "bless");
        check_header(&header, "#define X_ABI_VERSION 2\nint a(int);\n", 2, false, "bless");
    }

    #[test]
    fn a_header_out_of_date_fails() {
        let header = header_dir("stale");
        check_header(&header, "int a(void);\n", 1, true, "bless");
        let stale =
            std::panic::catch_unwind(|| check_header(&header, "int a(void);\nint b(void);\n", 2, false, "bless"));
        assert!(stale.is_err());
    }
}
