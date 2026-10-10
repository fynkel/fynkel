//! Debug lines (stage 4 plan §3.9): segments a game asks to see, drawn on
//! top of the scene by every view of the world until they're replaced.
//! Whoever owns the world sends the whole set each frame it changes; the
//! world only keeps it.

use glam::Vec3;

use crate::{Color, World};

/// One segment, from `a` to `b`, in the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DebugLine {
    pub a: Vec3,
    pub b: Vec3,
    pub color: Color,
}

impl World {
    /// Replaces the debug lines. Keeps its memory, so a steady number of
    /// lines allocates nothing.
    pub fn set_debug_lines(&mut self, lines: impl IntoIterator<Item = DebugLine>) {
        self.debug_lines.clear();
        self.debug_lines.extend(lines);
    }

    pub fn debug_lines(&self) -> &[DebugLine] {
        &self.debug_lines
    }
}
