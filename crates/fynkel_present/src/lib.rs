//! Where finished frames go: the frame ring (Apple), the Android surface and
//! the headless readback, behind one `Target` trait.
//!
//! See `docs/stage-1.md` §3.5. Unsafe code is only allowed in the platform
//! modules.

#[cfg(target_os = "android")]
#[allow(unsafe_code)]
mod android;
#[cfg(target_vendor = "apple")]
#[allow(unsafe_code)]
mod apple;
#[cfg(target_vendor = "apple")]
#[allow(unsafe_code)]
mod layer;
// Used by the Android and Apple layer targets; tested everywhere.
#[cfg_attr(not(any(target_os = "android", target_vendor = "apple")), allow(dead_code))]
mod handoff;
mod headless;
mod ring;
mod target;

#[cfg(target_os = "android")]
pub use android::{Android, AndroidError, AndroidPresenter};
#[cfg(target_vendor = "apple")]
pub use apple::{Apple, AppleError, ApplePresenter, FrameReady, Presented};
pub use headless::{Headless, HeadlessFrames, ReadError};
#[cfg(target_vendor = "apple")]
pub use layer::{Layer, LayerFrame, LayerPresenter};
pub use ring::{MAX_IN_FLIGHT, MAX_QUEUED, Ring, RingError, RingStats, SLOTS, Shown, SlotState, Ticket};
pub use target::{Target, Wake};
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
