//! Where finished frames go: the frame ring (Apple), the Android surface and
//! the headless readback, behind one `Target` trait.
//!
//! See `docs/stage-1.md` §3.5. Unsafe code is only allowed in the platform
//! modules.

mod headless;
mod ring;
mod target;

pub use headless::{Headless, HeadlessFrames, ReadError};
pub use ring::{MAX_IN_FLIGHT, MAX_QUEUED, Ring, RingError, RingStats, SLOTS, Shown, SlotState, Ticket};
pub use target::Target;
pub use wgpu;
