//! The sun's shadow map (blueprint §19.2d item 5; R1 made it cascaded).
//! The view out to [`SHADOW_DISTANCE`] is split into [`CASCADES`] slices,
//! each fitted to the bounding sphere of its own slice of the camera
//! frustum — so the slice nearest the camera, which is what a player
//! looks at, gets a whole quadrant of the map to itself instead of a
//! share of one fit around the whole view. Every fit is snapped to its
//! texel grid so the map does not shimmer as the camera turns, extended
//! toward the sun so casters outside the view still shadow what is in
//! it, and tightened to the casters' own bounds: a scene whose casters
//! span twelve metres is never fitted to sixty.
//!
//! The four fits share one map as a 2×2 atlas (`rect` per cascade), so
//! cascades cost no GPU memory over the single map they replaced.

use crate::culling::Sphere;
use glam::{Mat4, Vec3};

/// How far from the camera (metres) the shadow map reaches; the
/// camera's `far` when that is nearer, or the casters' reach when they
/// stop sooner.
pub const SHADOW_DISTANCE: f32 = 60.0;

/// Slices the shadowed range is split into — the 2×2 atlas.
pub const CASCADES: usize = 4;

/// How much of a cascade's edge blends into the next one, as a fraction
/// of its half-extent: the seam between two texel densities.
pub const CASCADE_BLEND: f32 = 0.1;

/// Mixes the uniform and logarithmic split schemes (Lauritzen's
/// "practical" split): 0 is uniform, 1 logarithmic.
const SPLIT_LAMBDA: f32 = 0.75;

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
        self.corners_between(self.near, self.far.min(SHADOW_DISTANCE))
    }

    /// The eight world-space corners of the frustum slice between the
    /// view-space distances `z0` and `z1`.
    pub fn corners_between(&self, z0: f32, z1: f32) -> [Vec3; 8] {
        let near = z0.max(1e-3);
        let far = z1.max(near + 1e-3);
        let half = |z: f32| -> (f32, f32) {
            let h = match self.shape {
                Shape::Perspective { fov_y } => (fov_y * 0.5).tan() * z,
                Shape::Orthographic { height } => height * 0.5,
            };
            (h * self.aspect, h)
        };
        let inv = self.view.inverse();
        let mut out = [Vec3::ZERO; 8];
        for (i, z) in [near, far].into_iter().enumerate() {
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
    fit_sphere(to_sun, bounding_sphere(corners), resolution)
}

/// The sphere the eight `corners` sit in: their mean and the farthest.
pub fn bounding_sphere(corners: &[Vec3; 8]) -> Sphere {
    let center = corners.iter().copied().sum::<Vec3>() / 8.0;
    let radius = corners
        .iter()
        .map(|c| c.distance(center))
        .fold(0.0f32, f32::max)
        .max(1e-2);
    Sphere { center, radius }
}

/// Fits the sun's orthographic view around `sphere` for a `resolution`²
/// map (one cascade's quadrant of the atlas).
pub fn fit_sphere(to_sun: Vec3, sphere: Sphere, resolution: u32) -> SunFit {
    let to_sun = to_sun.normalize_or(Vec3::Y);
    let centre = sphere.center;
    let radius = sphere.radius.max(1e-2);
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

/// The sun's shadow for one frame: a fit per cascade and where each one
/// stops (view-space metres from the camera).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cascades {
    pub fits: [SunFit; CASCADES],
    pub splits: [f32; CASCADES],
    /// Where the shadowed range ends: the last split.
    pub reach: f32,
}

impl Cascades {
    /// The fit a caster must be culled against: the last cascade covers
    /// the whole shadowed range, so it is the one that decides whether a
    /// caster can shadow anything at all.
    pub fn outermost(&self) -> &SunFit {
        &self.fits[CASCADES - 1]
    }
}

/// Splits the view into [`CASCADES`] slices and fits the sun to each.
///
/// `casters` is the world sphere everything that casts sits in, when the
/// frame knows it: a cascade whose slice is bigger than that sphere is
/// tightened to it, which is what makes a small scene sharp (its casters
/// span metres, the frustum tens of them). `resolution` is one cascade's
/// side in texels, not the atlas's.
pub fn fit_cascades(
    to_sun: Vec3,
    cam: &CameraShape,
    resolution: u32,
    casters: Option<Sphere>,
) -> Cascades {
    let eye = cam.view.inverse().transform_point3(Vec3::ZERO);
    let mut reach = cam.far.min(SHADOW_DISTANCE);
    if let Some(c) = casters {
        // Nothing casts past the far side of the casters' sphere.
        reach = reach.min(eye.distance(c.center) + c.radius);
    }
    let near = cam.near.max(reach / 200.0).min(reach * 0.5).max(1e-3);
    reach = reach.max(near + 1e-2);

    let mut fits = [SunFit {
        view_proj: Mat4::IDENTITY,
        texel_world: 0.0,
    }; CASCADES];
    let mut splits = [reach; CASCADES];
    let mut z0 = cam.near.max(1e-3);
    for i in 0..CASCADES {
        let t = (i + 1) as f32 / CASCADES as f32;
        let uniform = near + (reach - near) * t;
        let logarithmic = near * (reach / near).powf(t);
        let z1 = if i + 1 == CASCADES {
            reach
        } else {
            uniform + (logarithmic - uniform) * SPLIT_LAMBDA
        };
        let corners = cam.corners_between(z0, z1);
        let sphere = tighten(bounding_sphere(&corners), casters);
        fits[i] = fit_sphere(to_sun, sphere, resolution);
        splits[i] = z1;
        z0 = z1;
    }
    Cascades {
        fits,
        splits,
        reach,
    }
}

/// A slice that is bigger than everything casting into it is replaced by
/// the casters' own sphere — exact, since a receiver outside that sphere
/// has nothing above it to be shadowed by.
fn tighten(slice: Sphere, casters: Option<Sphere>) -> Sphere {
    match casters {
        Some(c)
            if c.radius < slice.radius
                && slice.center.distance(c.center) + c.radius <= slice.radius =>
        {
            c
        }
        _ => slice,
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
    fn the_cascades_tile_the_shadowed_range_and_sharpen_toward_the_camera() {
        let cam = camera();
        let to_sun = Vec3::new(0.3, 1.0, 0.2).normalize();
        // One cascade's quadrant of a 2048² atlas.
        let c = fit_cascades(to_sun, &cam, 1024, None);
        assert!((c.reach - SHADOW_DISTANCE).abs() < 1e-3, "{}", c.reach);
        assert!((c.splits[CASCADES - 1] - c.reach).abs() < 1e-3);
        // Nearer cascades are finer, and the nearest is far finer than
        // one fit around the whole view at the full map (the point of R1).
        for i in 1..CASCADES {
            assert!(
                c.fits[i - 1].texel_world < c.fits[i].texel_world,
                "{:?}",
                c.fits.map(|f| f.texel_world)
            );
        }
        let single = fit(to_sun, &cam.shadow_corners(), 2048);
        assert!(
            c.fits[0].texel_world * 6.0 < single.texel_world,
            "{} vs {}",
            c.fits[0].texel_world,
            single.texel_world
        );
        // Every point of the shadowed frustum lands in some cascade: the
        // selection in `sun_visibility` never runs off the end.
        let inv = cam.view.inverse();
        for d in 1..=60 {
            let z = d as f32;
            let h = (30f32.to_radians()).tan() * z;
            for (sx, sy) in [(0.0, 0.0), (-0.99, -0.99), (0.99, 0.99)] {
                let p = inv.transform_point3(Vec3::new(h * cam.aspect * sx, h * sy, -z));
                let hit = c.fits.iter().any(|f| {
                    let q = f.view_proj.project_point3(p);
                    q.x.abs() <= 1.0 && q.y.abs() <= 1.0 && (0.0..=1.0).contains(&q.z)
                });
                assert!(hit, "no cascade covers {p:?} at {z} m");
            }
        }
    }

    #[test]
    fn the_casters_bounds_tighten_every_cascade_they_are_smaller_than() {
        let cam = camera();
        let to_sun = Vec3::new(0.3, 1.0, 0.2).normalize();
        let loose = fit_cascades(to_sun, &cam, 1024, None);
        // A twelve-metre scene in front of the camera: the showcase's
        // size, against a sixty-metre frustum.
        let casters = Sphere {
            center: Vec3::ZERO,
            radius: 6.0,
        };
        let tight = fit_cascades(to_sun, &cam, 1024, Some(casters));
        assert!(tight.reach < loose.reach, "{}", tight.reach);
        assert!(
            tight.fits[CASCADES - 1].texel_world * 4.0 < loose.fits[CASCADES - 1].texel_world,
            "{} vs {}",
            tight.fits[CASCADES - 1].texel_world,
            loose.fits[CASCADES - 1].texel_world
        );
        // Tightening never drops a caster: every point of the casters'
        // sphere still projects into the outermost cascade.
        let f = tight.outermost();
        for axis in [
            Vec3::X,
            Vec3::NEG_X,
            Vec3::Y,
            Vec3::NEG_Y,
            Vec3::Z,
            Vec3::NEG_Z,
        ] {
            let p = casters.center + axis * casters.radius;
            let q = f.view_proj.project_point3(p);
            assert!(
                q.x.abs() <= 1.0 + 1e-4 && q.y.abs() <= 1.0 + 1e-4 && (0.0..=1.0).contains(&q.z),
                "{q:?}"
            );
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
