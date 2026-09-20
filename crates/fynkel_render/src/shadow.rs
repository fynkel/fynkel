//! The sun's shadow map (blueprint §19.2d item 5: one shadowed directional
//! light, one cascade). The light's orthographic view is fitted to the
//! bounding sphere of the camera frustum out to [`SHADOW_DISTANCE`], its
//! origin snapped to the shadow texel grid so the map does not shimmer as
//! the camera turns, and extended toward the sun so casters outside the
//! view still shadow what is in it.

use glam::{Mat4, Vec3};

/// How far from the camera (metres) the shadow map reaches; the
/// camera's `far` when that is nearer.
pub const SHADOW_DISTANCE: f32 = 60.0;

/// The camera as the fit needs it: the view matrix and the frustum shape.
#[derive(Clone, Copy, Debug)]
pub struct CameraShape {
    pub view: Mat4,
    pub near: f32,
    pub far: f32,
    pub aspect: f32,
    pub shape: Shape,
}

#[derive(Clone, Copy, Debug)]
pub enum Shape {
    Perspective { fov_y: f32 },
    Orthographic { height: f32 },
}

impl CameraShape {
    /// The eight world-space corners of the frustum between `near` and
    /// `min(far, SHADOW_DISTANCE)`.
    pub fn shadow_corners(&self) -> [Vec3; 8] {
        let far = self.far.min(SHADOW_DISTANCE).max(self.near + 1e-3);
        let half = |z: f32| -> (f32, f32) {
            let h = match self.shape {
                Shape::Perspective { fov_y } => (fov_y * 0.5).tan() * z,
                Shape::Orthographic { height } => height * 0.5,
            };
            (h * self.aspect, h)
        };
        let inv = self.view.inverse();
        let mut out = [Vec3::ZERO; 8];
        for (i, z) in [self.near, far].into_iter().enumerate() {
            let (w, h) = half(z);
            for (j, (sx, sy)) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                .into_iter()
                .enumerate()
            {
                out[i * 4 + j] = inv.transform_point3(Vec3::new(w * sx, h * sy, -z));
            }
        }
        out
    }
}

/// The fitted light: what the shadow pass renders with and what the mesh
/// pass samples with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunFit {
    /// World → light clip (orthographic, depth 0..1).
    pub view_proj: Mat4,
    /// World units per shadow texel (the receiver's normal offset scale).
    pub texel_world: f32,
}

/// Fits the sun's orthographic view around the frustum `corners` for a
/// `resolution`² map; `to_sun` is the unit vector toward the sun.
pub fn fit(to_sun: Vec3, corners: &[Vec3; 8], resolution: u32) -> SunFit {
    let to_sun = to_sun.normalize_or(Vec3::Y);
    let centre = corners.iter().copied().sum::<Vec3>() / 8.0;
    let radius = corners
        .iter()
        .map(|c| c.distance(centre))
        .fold(0.0f32, f32::max)
        .max(1e-2);
    // Texel-sized quantisation keeps the projected grid fixed in world
    // space as the camera moves.
    let texel = 2.0 * radius / resolution.max(1) as f32;
    let up = if to_sun.y.abs() > 0.99 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    // The light looks along -to_sun; `rot` is that view with the eye at
    // the origin.
    let rot = glam::camera::rh::view::look_to_mat4(Vec3::ZERO, -to_sun, up);
    let c = rot.transform_point3(centre);
    let cx = (c.x / texel).round() * texel;
    let cy = (c.y / texel).round() * texel;
    // Depth: the sphere spans c.z ± radius in front of the eye (view -Z);
    // reach back toward the sun for casters outside the frustum.
    let back = radius * 3.0 + 10.0;
    let near = -(c.z + radius + back);
    let far = -(c.z - radius);
    let proj = glam::camera::rh::proj::directx::orthographic(
        cx - radius,
        cx + radius,
        cy - radius,
        cy + radius,
        near,
        far,
    );
    SunFit {
        view_proj: proj * rot,
        texel_world: texel,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera() -> CameraShape {
        CameraShape {
            view: glam::camera::rh::view::look_at_mat4(
                Vec3::new(0.0, 5.0, 10.0),
                Vec3::ZERO,
                Vec3::Y,
            ),
            near: 0.1,
            far: 100.0,
            aspect: 16.0 / 9.0,
            shape: Shape::Perspective {
                fov_y: 60f32.to_radians(),
            },
        }
    }

    #[test]
    fn the_frustum_corners_are_clamped_to_the_shadow_distance() {
        let cam = camera();
        let c = cam.shadow_corners();
        for near in &c[..4] {
            let z = cam.view.transform_point3(*near).z;
            assert!((z + cam.near).abs() < 1e-4, "{z}");
        }
        for far in &c[4..] {
            let z = cam.view.transform_point3(*far).z;
            assert!((z + SHADOW_DISTANCE).abs() < 1e-3, "{z}");
        }
    }

    #[test]
    fn every_corner_projects_inside_the_map_and_the_origin_is_snapped() {
        let cam = camera();
        let corners = cam.shadow_corners();
        let to_sun = Vec3::new(0.3, 1.0, 0.2).normalize();
        let f = fit(to_sun, &corners, 2048);
        for c in &corners {
            let p = f.view_proj.project_point3(*c);
            assert!(p.x.abs() <= 1.0 + 1e-4 && p.y.abs() <= 1.0 + 1e-4, "{p:?}");
            assert!((0.0..=1.0).contains(&p.z), "{p:?}");
        }
        // A caster well above the frustum, toward the sun, still lands in
        // the depth range.
        let above = corners[4] + to_sun * 30.0;
        let p = f.view_proj.project_point3(above);
        assert!((0.0..=1.0).contains(&p.z), "{p:?}");
        // Moving the camera by a fraction of a texel does not move the
        // grid: the projection of a fixed world point changes by a whole
        // number of texels (or not at all).
        let mut moved = cam;
        moved.view = glam::camera::rh::view::look_at_mat4(
            Vec3::new(f.texel_world * 0.3, 5.0, 10.0),
            Vec3::new(f.texel_world * 0.3, 0.0, 0.0),
            Vec3::Y,
        );
        let g = fit(to_sun, &moved.shadow_corners(), 2048);
        assert!((g.texel_world - f.texel_world).abs() < 1e-6);
        let point = Vec3::new(1.0, 0.0, -3.0);
        let a = f.view_proj.project_point3(point);
        let b = g.view_proj.project_point3(point);
        let texels_x = (a.x - b.x) * 1024.0;
        let texels_y = (a.y - b.y) * 1024.0;
        assert!((texels_x - texels_x.round()).abs() < 1e-3, "{texels_x}");
        assert!((texels_y - texels_y.round()).abs() < 1e-3, "{texels_y}");
    }
}
