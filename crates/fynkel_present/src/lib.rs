//! Where finished frames go: the frame ring (Apple), the Android surface and
//! the headless readback, behind one `Target` trait.
//!
//! See `docs/stage-1.md` §3.5. Unsafe code is only allowed in the platform
//! modules.

#[cfg(target_vendor = "apple")]
#[allow(unsafe_code)]
mod apple;
mod headless;
mod ring;
mod target;

#[cfg(target_vendor = "apple")]
pub use apple::{Apple, AppleError, ApplePresenter, FrameReady, Presented};
pub use headless::{Headless, HeadlessFrames, ReadError};
pub use ring::{MAX_IN_FLIGHT, MAX_QUEUED, Ring, RingError, RingStats, SLOTS, Shown, SlotState, Ticket};
pub use target::Target;
pub use wgpu;

/// Runs `f` inside an autorelease pool on Apple platforms, and as is
/// elsewhere. Threads Rust starts have no pool of their own, so the
/// Objective-C objects Metal and wgpu autorelease on them (texture
/// descriptors, command buffers) would only be freed when the thread ends:
/// a render thread wraps each frame in one.
pub fn autorelease_pool<R>(f: impl FnOnce() -> R) -> R {
    #[cfg(target_vendor = "apple")]
    return objc2::rc::autoreleasepool(|_| f());
    #[cfg(not(target_vendor = "apple"))]
    f()
}
