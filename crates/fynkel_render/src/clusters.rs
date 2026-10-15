//! Which lamps light which part of the view (hardening L1).
//!
//! The view is cut into cells: [`TILES_X`] × [`TILES_Y`] across the screen,
//! by [`SLICES`] of depth, spaced exponentially from the camera's near
//! plane to the furthest any lamp reaches (evenly for an orthographic
//! camera). Each frame every lamp of the frame's list is entered into the
//! cells it reaches, and a pixel is lit by its cell's lamps
//! (forward.wgsl's `cell_of`). A lamp's light then ends where its range
//! does, wherever that falls, rather than at an object's edge.
//!
//! A cell holds every lamp that reaches it, as Unity's Forward+ and
//! Godot's clustered renderer do: a bit for each of the frame's
//! [`MAX_LIGHTS`](crate::Renderer::MAX_LIGHTS) lamps, two `u32` words a
//! cell, sent as one storage buffer. A cell that kept only some would show
//! its edge where its neighbour kept others.

use glam::{Mat4, Vec3};

use crate::scene::LightKind;

pub(crate) const TILES_X: usize = 16;
pub(crate) const TILES_Y: usize = 9;
pub(crate) const SLICES: usize = 16;
pub(crate) const CELLS: usize = TILES_X * TILES_Y * SLICES;
/// A cell's words: a bit for each of the frame's lamps.
const WORDS: usize = crate::renderer::MAX_LIGHTS / 32;
/// The buffer's size.
pub(crate) const BYTES: u64 = (CELLS * WORDS * 4) as u64;

/// A point or spot light's reach, as the cells are tested against it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Reach {
    pub(crate) position: Vec3,
    pub(crate) range: f32,
    /// A spot light's axis and the sine and cosine of half its angle.
    cone: Option<(Vec3, f32, f32)>,
}

impl Reach {
    /// `light`'s reach. Not for suns.
    pub(crate) fn of(light: &crate::Light) -> Self {
        let (position, range, cone) = match light.kind {
            LightKind::Sun { .. } => unreachable!("suns aren't in the frame's list"),
            LightKind::Point { position, range } => (position, range, None),
            LightKind::Spot { position, direction, range, angle, .. } => {
                let half = (angle * 0.5).clamp(0.0, std::f32::consts::FRAC_PI_2);
                (position, range, Some((direction.normalize_or_zero(), half.sin(), half.cos())))
            }
        };
        Self { position, range, cone }
    }

    /// The same reach, in the space `matrix` takes world space to (a
    /// rotation and translation).
    fn moved(&self, matrix: &Mat4) -> Self {
        let cone = self.cone.map(|(axis, sin, cos)| (matrix.transform_vector3(axis).normalize_or_zero(), sin, cos));
        Self { position: matrix.transform_point3(self.position), cone, ..*self }
    }

    /// Whether it reaches a sphere at `at` with radius `r`.
    pub(crate) fn reaches(&self, at: Vec3, r: f32) -> bool {
        let to = at - self.position;
        if to.length() - r >= self.range {
            return false;
        }
        if let Some((axis, sin, cos)) = self.cone {
            // Outside the cone: the sphere's centre is further from the
            // cone's side than its radius (Wronski, "Cull that cone!").
            let along = to.dot(axis);
            let across = (to.length_squared() - along * along).max(0.0).sqrt();
            if cos * across - sin * along > r || along < -r {
                return false;
            }
        }
        true
    }
}

/// The camera the cells are cut for.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Grid {
    /// World → view: the camera looks down -Z.
    pub(crate) view: Mat4,
    /// Half the view's width and height: at 1 m for a perspective camera
    /// (the tangents of half its angles), in metres for an orthographic
    /// one.
    pub(crate) half: (f32, f32),
    pub(crate) orthographic: bool,
    pub(crate) near: f32,
    /// Where the last slice ends.
    pub(crate) far: f32,
}

impl Grid {
    /// How far along the view slice `k`'s near side is (`k` = [`SLICES`]:
    /// the far side of the last).
    fn depth(&self, k: usize) -> f32 {
        let t = k as f32 / SLICES as f32;
        if self.orthographic {
            self.near + (self.far - self.near) * t
        } else {
            self.near * (self.far / self.near).powf(t)
        }
    }

    /// The slice a point `depth` along the view is in, as a scale and
    /// offset on the depth (orthographic) or its logarithm (perspective),
    /// as forward.wgsl's `cell_of` reads it.
    pub(crate) fn slicing(&self) -> (f32, f32) {
        if self.orthographic {
            let scale = SLICES as f32 / (self.far - self.near);
            (scale, -self.near * scale)
        } else {
            let scale = SLICES as f32 / (self.far / self.near).ln();
            (scale, -self.near.ln() * scale)
        }
    }

    /// The slice `depth` falls in, clamped to the grid.
    fn slice(&self, depth: f32) -> usize {
        let (scale, offset) = self.slicing();
        let at = if self.orthographic { depth } else { depth.max(self.near).ln() };
        (at * scale + offset).floor().clamp(0.0, SLICES as f32 - 1.0) as usize
    }

    /// The view-space bounding sphere of cell (`x`, `y`, `k`), tiles
    /// counted from the left and the top.
    fn cell_sphere(&self, x: usize, y: usize, k: usize) -> (Vec3, f32) {
        let across = |t: usize, n: usize| -1.0 + 2.0 * t as f32 / n as f32;
        let (x0, x1) = (across(x, TILES_X), across(x + 1, TILES_X));
        let (y0, y1) = (-across(y + 1, TILES_Y), -across(y, TILES_Y));
        let (d0, d1) = (self.depth(k), self.depth(k + 1));
        // Its box: a perspective cell widens with depth.
        let (s0, s1) = if self.orthographic { (1.0, 1.0) } else { (d0, d1) };
        let (hx, hy) = self.half;
        let xs = [x0 * hx * s0, x1 * hx * s0, x0 * hx * s1, x1 * hx * s1];
        let ys = [y0 * hy * s0, y1 * hy * s0, y0 * hy * s1, y1 * hy * s1];
        let min = Vec3::new(xs.into_iter().fold(f32::MAX, f32::min), ys.into_iter().fold(f32::MAX, f32::min), -d1);
        let max = Vec3::new(xs.into_iter().fold(f32::MIN, f32::max), ys.into_iter().fold(f32::MIN, f32::max), -d0);
        ((min + max) * 0.5, (max - min).length() * 0.5)
    }

    /// The tiles a view-space sphere may cover, along one axis: `centre`
    /// and `r` across, at `depth`, over `half` a view at 1 m (or in metres).
    fn tiles(&self, centre: f32, r: f32, depth: f32, half: f32, n: usize) -> (usize, usize) {
        let near_side = depth - r;
        if !self.orthographic && near_side <= self.near {
            // It reaches past the camera's near plane: anywhere across.
            return (0, n - 1);
        }
        let (lo, hi) = if self.orthographic {
            ((centre - r) / half, (centre + r) / half)
        } else {
            // Its box's sides seen from the camera: nearest and furthest.
            let far_side = depth + r;
            let (left, right) = (centre - r, centre + r);
            let all = [left / near_side, left / far_side, right / near_side, right / far_side];
            (all.iter().copied().fold(f32::MAX, f32::min) / half, all.iter().copied().fold(f32::MIN, f32::max) / half)
        };
        let tile = |ndc: f32| ((ndc + 1.0) * 0.5 * n as f32).floor().clamp(0.0, n as f32 - 1.0) as usize;
        (tile(lo), tile(hi))
    }
}

/// The cells and their lamps, rebuilt each frame. Allocates nothing once
/// built.
pub(crate) struct Clusters {
    /// Each cell's lamps, a bit each.
    masks: Vec<[u32; WORDS]>,
    /// Each cell's view-space bounding sphere, for the grid in `cut_for`.
    spheres: Vec<(Vec3, f32)>,
    /// The grid's shape the spheres were worked out for (its half sizes,
    /// whether orthographic, near and far), or `None` before the first.
    cut_for: Option<((f32, f32), bool, f32, f32)>,
}

impl Clusters {
    pub(crate) fn new() -> Self {
        Self { masks: vec![[0; WORDS]; CELLS], spheres: vec![(Vec3::ZERO, 0.0); CELLS], cut_for: None }
    }

    /// Enters each of `lights` (the frame's list, in its order, at most
    /// [`MAX_LIGHTS`](crate::Renderer::MAX_LIGHTS)) into every cell of
    /// `grid` it reaches.
    pub(crate) fn build(&mut self, grid: &Grid, lights: &[Reach]) {
        debug_assert!(lights.len() <= WORDS * 32);
        self.masks.fill([0; WORDS]);
        // The spheres stay as long as the grid's shape does; the view
        // matrix doesn't move them.
        let shape = Some((grid.half, grid.orthographic, grid.near, grid.far));
        if !lights.is_empty() && self.cut_for != shape {
            for (i, sphere) in self.spheres.iter_mut().enumerate() {
                *sphere = grid.cell_sphere(i % TILES_X, i / TILES_X % TILES_Y, i / (TILES_X * TILES_Y));
            }
            self.cut_for = shape;
        }
        for (index, light) in lights.iter().enumerate() {
            let seen = light.moved(&grid.view);
            let (c, r) = (seen.position, seen.range);
            let depth = -c.z;
            if depth + r <= grid.near || depth - r >= grid.far {
                continue;
            }
            let (k0, k1) = (grid.slice(depth - r), grid.slice(depth + r));
            let (x0, x1) = grid.tiles(c.x, r, depth, grid.half.0, TILES_X);
            // Tiles count down from the top; y counts up.
            let (y0, y1) = grid.tiles(-c.y, r, depth, grid.half.1, TILES_Y);
            let (word, bit) = (index / 32, 1 << (index % 32));
            for k in k0..=k1 {
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        let i = (k * TILES_Y + y) * TILES_X + x;
                        let (at, cell_r) = self.spheres[i];
                        if seen.reaches(at, cell_r) {
                            self.masks[i][word] |= bit;
                        }
                    }
                }
            }
        }
    }

    /// The buffer forward.wgsl reads as `cells`.
    pub(crate) fn bytes(&self) -> &[u8] {
        bytemuck::cast_slice(&self.masks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CameraView, Light};

    fn grid(camera: &CameraView, aspect: f32, far: f32) -> Grid {
        let half = match camera.orthographic {
            Some(h) => (h * aspect, h),
            None => {
                let t = (camera.fov_y * 0.5).tan();
                (t * aspect, t)
            }
        };
        Grid { view: camera.view, half, orthographic: camera.orthographic.is_some(), near: camera.near, far }
    }

    /// The cells holding lamp `lamp`.
    fn holding(c: &Clusters, lamp: usize) -> Vec<usize> {
        (0..CELLS).filter(|&i| c.masks[i][lamp / 32] & (1 << (lamp % 32)) != 0).collect()
    }

    /// Whether a view-space sphere could touch cell `i`: brute force, by
    /// sampling the cell's volume.
    fn touches(g: &Grid, i: usize, at: Vec3, r: f32) -> bool {
        let (x, y, k) = (i % TILES_X, i / TILES_X % TILES_Y, i / (TILES_X * TILES_Y));
        let steps = 6;
        for a in 0..=steps {
            for b in 0..=steps {
                for c in 0..=steps {
                    let f = |t: usize| t as f32 / steps as f32;
                    let ndc_x = -1.0 + 2.0 * (x as f32 + f(a)) / TILES_X as f32;
                    let ndc_y = 1.0 - 2.0 * (y as f32 + f(b)) / TILES_Y as f32;
                    let d = g.depth(k) + (g.depth(k + 1) - g.depth(k)) * f(c);
                    let s = if g.orthographic { 1.0 } else { d };
                    let p = Vec3::new(ndc_x * g.half.0 * s, ndc_y * g.half.1 * s, -d);
                    if p.distance(at) < r * 0.98 {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// A lamp is in every cell its sphere touches, and in no cell its
    /// sphere's box can't reach, for both kinds of camera. One `Clusters`
    /// for all, so a frame keeps nothing of the last's lamps or grid.
    #[test]
    fn a_lamp_is_in_the_cells_it_reaches() {
        let mut perspective = CameraView::looking_at(Vec3::new(0.0, 1.0, 5.0), Vec3::ZERO);
        perspective.near = 0.1;
        let mut orthographic = perspective;
        orthographic.orthographic = Some(3.0);
        let mut c = Clusters::new();
        for camera in [perspective, orthographic] {
            let g = grid(&camera, 16.0 / 9.0, 20.0);
            for (at, range) in
                [(Vec3::new(0.5, 0.2, 0.0), 1.0), (Vec3::new(-2.0, 0.0, -3.0), 2.5), (camera.position, 0.5)]
            {
                let lamp = Light::point(at, range, Vec3::ONE);
                c.build(&g, &[Reach::of(&lamp)]);
                let cells = holding(&c, 0);
                let seen = g.view.transform_point3(at);
                for i in 0..CELLS {
                    if touches(&g, i, seen, range) {
                        assert!(cells.contains(&i), "{:?} lamp at {at} misses cell {i}", camera.orthographic);
                    }
                }
                for &i in &cells {
                    let (x, y, k) = (i % TILES_X, i / TILES_X % TILES_Y, i / (TILES_X * TILES_Y));
                    let (centre, r) = g.cell_sphere(x, y, k);
                    assert!(centre.distance(seen) < r + range, "cell {i} is out of the lamp's reach");
                }
            }
        }
    }

    /// A cell reached by all of the frame's lamps holds every one, and
    /// the buffer says so.
    #[test]
    fn a_cell_holds_every_lamp_that_reaches_it() {
        let camera = CameraView::looking_at(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO);
        let g = grid(&camera, 1.0, 20.0);
        let lamps: Vec<Reach> = (0..crate::renderer::MAX_LIGHTS)
            .map(|i| Reach::of(&Light::point(Vec3::splat(0.01 * i as f32), 100.0, Vec3::ONE)))
            .collect();
        let mut c = Clusters::new();
        c.build(&g, &lamps);
        for lamp in 0..lamps.len() {
            assert_eq!(holding(&c, lamp).len(), CELLS, "lamp {lamp} is in every cell");
        }
        assert!(c.bytes().iter().all(|&b| b == 0xFF));
        c.build(&g, &lamps[..1]);
        assert_eq!(&c.bytes()[..WORDS * 4], [1, 0, 0, 0, 0, 0, 0, 0], "a frame keeps nothing of the last");
    }
}
