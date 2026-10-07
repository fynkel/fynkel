//! Errors at the C boundary: a status code for the caller to branch on, and
//! a message for people, kept per thread for `fy_last_error`.

use std::cell::RefCell;
use std::ffi::{CString, c_char};
use std::panic::{self, AssertUnwindSafe};

use fynkel_world::WorldError;

use crate::worker::panic_message;

/// What a `fy_*` call returns. Anything but `Ok` leaves a message for
/// `fy_last_error`.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FyStatus {
    Ok = 0,
    /// No object has this id: it was despawned, or never spawned.
    NoObject = 1,
    /// The object doesn't have that component.
    NoComponent = 2,
    NoMesh = 3,
    NoMaterial = 4,
    /// A mesh or material can't be removed while an object uses it.
    InUse = 5,
    /// A value was out of range, not finite, or not one of the choices.
    InvalidValue = 6,
    /// A pointer argument was null.
    NullPointer = 7,
    /// No GPU could be had.
    NoGpu = 8,
    /// A frame didn't finish in time.
    Timeout = 9,
    /// The render worker stopped after a panic.
    WorkerFailed = 10,
    /// A buffer had the wrong size.
    WrongSize = 11,
    /// The call panicked. Nothing crossed into the caller; the message says
    /// what happened.
    Panic = 12,
    /// An earlier call panicked while changing this world, so it can't be
    /// trusted any more.
    Poisoned = 13,
    /// Not available on this platform, or not for this kind of view.
    Unsupported = 14,
}

/// A failed call: its status and its message.
#[derive(Debug)]
pub(crate) struct Error {
    pub(crate) status: FyStatus,
    pub(crate) message: String,
}

impl Error {
    pub(crate) fn new(status: FyStatus, message: impl Into<String>) -> Self {
        Self { status, message: message.into() }
    }

    pub(crate) fn null(what: &str) -> Self {
        Self::new(FyStatus::NullPointer, format!("{what} is null"))
    }
}

impl From<WorldError> for Error {
    fn from(e: WorldError) -> Self {
        let status = match e {
            WorldError::NoObject(_) => FyStatus::NoObject,
            WorldError::NoComponent { .. } => FyStatus::NoComponent,
            WorldError::NoMesh(_) => FyStatus::NoMesh,
            WorldError::NoMaterial(_) => FyStatus::NoMaterial,
            WorldError::MeshInUse { .. } | WorldError::MaterialInUse { .. } => FyStatus::InUse,
            WorldError::Field(_) => FyStatus::InvalidValue,
        };
        Self::new(status, e.to_string())
    }
}

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

/// Runs the body of a `fy_*` call: a panic becomes [`FyStatus::Panic`] and
/// never unwinds into the caller, and any error's message is kept for
/// [`last_error`].
pub(crate) fn guard(body: impl FnOnce() -> Result<(), Error>) -> FyStatus {
    let error = match panic::catch_unwind(AssertUnwindSafe(body)) {
        Ok(Ok(())) => return FyStatus::Ok,
        Ok(Err(error)) => error,
        Err(payload) => Error::new(FyStatus::Panic, format!("panicked: {}", panic_message(&*payload))),
    };
    let message = CString::new(error.message.replace('\0', "")).expect("nul bytes removed");
    LAST_ERROR.with(|last| *last.borrow_mut() = message);
    error.status
}

/// The message of the last failed call on this thread ("" before any).
/// Valid until the next failed call on this thread.
pub(crate) fn last_error() -> *const c_char {
    LAST_ERROR.with(|last| last.borrow().as_ptr())
}

#[cfg(test)]
mod tests {
    use std::ffi::CStr;

    use super::*;

    fn message() -> String {
        // SAFETY: last_error points at the thread's current message, which
        // lives until the next failed call.
        unsafe { CStr::from_ptr(last_error()) }.to_str().unwrap().to_owned()
    }

    #[test]
    fn a_panic_becomes_a_status_and_a_message() {
        assert_eq!(guard(|| panic!("the {} broke", "thing")), FyStatus::Panic);
        assert_eq!(message(), "panicked: the thing broke");
    }

    #[test]
    fn an_error_keeps_its_message_and_success_keeps_the_last_one() {
        assert_eq!(guard(|| Err(Error::new(FyStatus::Timeout, "too slow"))), FyStatus::Timeout);
        assert_eq!(guard(|| Ok(())), FyStatus::Ok);
        assert_eq!(message(), "too slow");
    }

    #[test]
    fn a_message_with_a_nul_byte_still_arrives() {
        guard(|| Err(Error::new(FyStatus::InvalidValue, "a\0b")));
        assert_eq!(message(), "ab");
    }
}
