//! Where finished frames go: the frame ring (Apple), the Android surface and
//! the headless readback, behind one `Target` trait.
//!
//! See `docs/stage-1.md` §3.5. Unsafe code is only allowed in the platform
//! modules.
