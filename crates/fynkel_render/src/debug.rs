//! Debug primitives (blueprint §19.2d item 5): world-space lines drawn
//! on top of the scene by the transparent stage — depth tested against
//! the opaque scene, never written, display-referred colour (no lighting,
//! no exposure). A host queues primitives between frames through
//! [`ExtractRenderer::debug`](crate::ExtractRenderer::debug); `prepare`
//! uploads and clears the queue, so each frame draws what was queued
//! since the last one. Physics shapes come with the physics item.

use glam::Vec3;

/// One line vertex: position and linear RGBA colour (display-referred:
/// `[1, 0, 0, 1]` is pure red in the target).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DebugVertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
}

/// The queue of lines for the next frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DebugDraw {
    vertices: Vec<DebugVertex>,
}

impl DebugDraw {
    /// A segment from `a` to `b`.
    pub fn line(&mut self, a: Vec3, b: Vec3, color: [f32; 4]) {
        self.vertices.push(DebugVertex {
            position: a.to_array(),
            color,
        });
        self.vertices.push(DebugVertex {
            position: b.to_array(),
            color,
        });
    }

    /// The twelve edges of an axis-aligned box.
    pub fn aabb(&mut self, min: Vec3, max: Vec3, color: [f32; 4]) {
        let (lo, hi) = (min.min(max), min.max(max));
        let corner = |x: bool, y: bool, z: bool| {
            Vec3::new(
                if x { hi.x } else { lo.x },
                if y { hi.y } else { lo.y },
                if z { hi.z } else { lo.z },
            )
        };
        for y in [false, true] {
            // The four edges of the face at this height.
            self.line(corner(false, y, false), corner(true, y, false), color);
            self.line(corner(true, y, false), corner(true, y, true), color);
            self.line(corner(true, y, true), corner(false, y, true), color);
            self.line(corner(false, y, true), corner(false, y, false), color);
        }
        for (x, z) in [(false, false), (true, false), (true, true), (false, true)] {
            self.line(corner(x, false, z), corner(x, true, z), color);
        }
    }

    /// A three-axis cross of half-size `r`: red X, green Y, blue Z.
    pub fn axes(&mut self, origin: Vec3, r: f32) {
        self.line(origin, origin + Vec3::X * r, [1.0, 0.0, 0.0, 1.0]);
        self.line(origin, origin + Vec3::Y * r, [0.0, 1.0, 0.0, 1.0]);
        self.line(origin, origin + Vec3::Z * r, [0.0, 0.0, 1.0, 1.0]);
    }

    /// Lines queued.
    pub fn len(&self) -> usize {
        self.vertices.len() / 2
    }

    pub fn is_empty(&self) -> bool {
        self.vertices.is_empty()
    }

    pub fn clear(&mut self) {
        self.vertices.clear();
    }

    pub(crate) fn vertices(&self) -> &[DebugVertex] {
        &self.vertices
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_aabb_is_twelve_edges_and_axes_are_three() {
        let mut d = DebugDraw::default();
        d.aabb(Vec3::ONE, -Vec3::ONE, [1.0; 4]);
        assert_eq!(d.len(), 12);
        // Every vertex is a corner of the (normalised) box.
        for v in d.vertices() {
            assert!(v.position.iter().all(|c| c.abs() == 1.0), "{v:?}");
        }
        d.axes(Vec3::ZERO, 2.0);
        assert_eq!(d.len(), 15);
        d.clear();
        assert!(d.is_empty());
    }
}
