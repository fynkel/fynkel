//! Debug lines, drawn for a frame.

use crate::error::{FyStatus, guard};

use super::*;

// --- Debug drawing -------------------------------------------------------------

/// A line from `a` to `b` in world space.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyDebugLine {
    pub a: FyVec3,
    pub b: FyVec3,
    pub color: FyColor,
}

/// Replaces the lines views draw on top of the scene with these `count`
/// lines, until the next call: 0 lines clears them. A line with a point
/// that isn't finite is left out.
///
/// # Safety
///
/// `world` is null or live; `lines` is null or valid for `count` reads (it
/// may be null when `count` is 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_debug_lines_set(
    world: *const FyWorld,
    lines: *const FyDebugLine,
    count: usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let lines: &[FyDebugLine] = if count == 0 {
            &[]
        } else {
            // SAFETY: per this function's contract.
            unsafe { std::slice::from_raw_parts(handle(lines, "lines")?, count) }
        };
        let lines =
            lines.iter().map(|l| fynkel_world::DebugLine { a: l.a.into(), b: l.b.into(), color: l.color.into() });
        world.lock()?.set_debug_lines(lines.filter(|l| l.a.is_finite() && l.b.is_finite()));
        Ok(())
    })
}
