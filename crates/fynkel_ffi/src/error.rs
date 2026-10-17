//! Errors at the C boundary: a status code for the caller to branch on, and
//! a message for people, kept per thread for `fy_last_error`.

use fynkel_world::WorldError;

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
    /// A mesh or material can't be removed while an object uses it, or a
    /// texture while a material does.
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
    NoTexture = 15,
    /// The view's GPU device was lost, or the GPU reported an error, and
    /// the view stopped drawing. A new view gets a new device, and uploads
    /// everything again.
    DeviceLost = 16,
}

/// A failed call: its status and its message.
#[derive(Debug, PartialEq, Eq)]
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
            WorldError::NoTexture(_) => FyStatus::NoTexture,
            WorldError::MeshInUse { .. } | WorldError::MaterialInUse { .. } | WorldError::TextureInUse { .. } => {
                FyStatus::InUse
            }
            WorldError::Field(_)
            | WorldError::Cycle { .. }
            | WorldError::ModelIds { .. }
            | WorldError::Layer(_)
            | WorldError::NoClip { .. } => FyStatus::InvalidValue,
        };
        Self::new(status, e.to_string())
    }
}

/// Runs the body of a `fy_*` call ([`fynkel_cabi::guard`]): a panic
/// becomes [`FyStatus::Panic`] and never unwinds into the caller, and any
/// error's message is kept for [`last_error`].
pub(crate) fn guard(body: impl FnOnce() -> Result<(), Error>) -> FyStatus {
    fynkel_cabi::guard(|| body().map_err(|e| fynkel_cabi::Error::new(e.status, e.message)))
}

pub(crate) use fynkel_cabi::last_error;

impl fynkel_cabi::Status for FyStatus {
    const OK: Self = Self::Ok;
    const PANIC: Self = Self::Panic;
    const NULL_POINTER: Self = Self::NullPointer;
}

#[cfg(test)]
mod tests {
    use fynkel_cabi::status;

    use super::*;

    #[test]
    fn the_shared_statuses_have_the_shared_numbers() {
        for (ours, shared) in [
            (FyStatus::Ok, status::OK),
            (FyStatus::InvalidValue, status::INVALID_VALUE),
            (FyStatus::NullPointer, status::NULL_POINTER),
            (FyStatus::WrongSize, status::WRONG_SIZE),
            (FyStatus::Panic, status::PANIC),
            (FyStatus::Poisoned, status::POISONED),
        ] {
            assert_eq!(ours as i32, shared, "{ours:?}");
        }
    }

    #[test]
    fn an_error_goes_through_the_shared_guard() {
        assert_eq!(guard(|| Err(Error::null("world"))), FyStatus::NullPointer);
        assert_eq!(guard(|| panic!("broke")), FyStatus::Panic);
        assert_eq!(guard(|| Ok(())), FyStatus::Ok);
    }
}
