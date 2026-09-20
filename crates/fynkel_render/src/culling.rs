//! Frustum culling (blueprint §19.2d item 6, ADR-0011): every instance
//! carries its mesh's cooked bounding sphere into world space and is
//! tested against the camera frustum before it is uploaded — and, when the
//! sun casts, against the light's box, so casters outside the view still
//! shadow what is in it. The test is conservative (a sphere that touches a
//! plane is kept), so culling never changes the picture.

use glam::{Mat4, Vec3, Vec4};

/// A bounding sphere in the space of the mesh it belongs to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sphere {
    pub center: Vec3,
    pub radius: f32,
}

impl Sphere {
    /// The sphere around a cooked mesh's axis-aligned bounds (its centre
    /// and half diagonal).
    pub fn from_bounds(min: [f32; 3], max: [f32; 3]) -> Sphere {
        let (min, max) = (Vec3::from(min), Vec3::from(max));
        Sphere {
            center: (min + max) * 0.5,
            radius: (max - min).length() * 0.5,
        }
    }

    /// The sphere under `model`: the centre transformed, the radius scaled
    /// by the largest axis scale (non-uniform scale stays conservative).
    pub fn transformed(&self, model: &Mat4) -> Sphere {
        let scale = model
            .x_axis
            .truncate()
            .length()
            .max(model.y_axis.truncate().length())
            .max(model.z_axis.truncate().length());
        Sphere {
            center: model.transform_point3(self.center),
            radius: self.radius * scale,
        }
    }
}

/// The six planes of a clip volume, each as `(n, d)` with the inside
/// where `n · p + d >= 0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frustum {
    planes: [Vec4; 6],
}

impl Frustum {
    /// The frustum of a `view_proj` matrix with clip depth in `0..=1`
    /// (Gribb & Hartmann: the planes are sums and differences of the
    /// matrix rows).
    pub fn from_view_proj(m: &Mat4) -> Frustum {
        let r = |i: usize| m.row(i);
        let planes = [
            r(3) + r(0), // left:   x >= -w
            r(3) - r(0), // right:  x <=  w
            r(3) + r(1), // bottom: y >= -w
            r(3) - r(1), // top:    y <=  w
            r(2),        // near:   z >=  0
            r(3) - r(2), // far:    z <=  w
        ]
        .map(|p| {
            let n = p.truncate().length();
            if n > 0.0 {
                p / n
            } else {
                p
            }
        });
        Frustum { planes }
    }

    /// Whether any part of the sphere may be inside.
    pub fn intersects(&self, s: &Sphere) -> bool {
        let c = s.center.extend(1.0);
        self.planes.iter().all(|p| p.dot(c) >= -s.radius)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frustum() -> Frustum {
        let view =
            glam::camera::rh::view::look_at_mat4(Vec3::new(0.0, 0.0, 10.0), Vec3::ZERO, Vec3::Y);
        let proj = glam::camera::rh::proj::directx::perspective(
            60f32.to_radians(),
            16.0 / 9.0,
            0.1,
            100.0,
        );
        Frustum::from_view_proj(&(proj * view))
    }

    #[test]
    fn spheres_inside_touching_and_outside() {
        let f = frustum();
        let unit = |c: Vec3| Sphere {
            center: c,
            radius: 1.0,
        };
        assert!(f.intersects(&unit(Vec3::ZERO)));
        // Behind the camera, beyond the far plane, far to the side.
        assert!(!f.intersects(&unit(Vec3::new(0.0, 0.0, 20.0))));
        assert!(!f.intersects(&unit(Vec3::new(0.0, 0.0, -95.0))));
        assert!(!f.intersects(&unit(Vec3::new(50.0, 0.0, 0.0))));
        // At the origin plane (10 m away, half height 5.77 m at 60°): a
        // sphere whose edge touches the top plane is kept, one a little
        // further is not.
        let half_h = 10.0 * 30f32.to_radians().tan();
        assert!(f.intersects(&unit(Vec3::new(0.0, half_h + 0.9, 0.0))));
        assert!(!f.intersects(&unit(Vec3::new(0.0, half_h + 1.2, 0.0))));
        // Conservative under non-uniform scale: the radius follows the
        // largest axis.
        let s = Sphere::from_bounds([-0.5; 3], [0.5; 3]);
        assert!((s.radius - 0.75f32.sqrt()).abs() < 1e-6);
        let t = s.transformed(&Mat4::from_scale_rotation_translation(
            Vec3::new(1.0, 4.0, 1.0),
            glam::Quat::IDENTITY,
            Vec3::new(3.0, 0.0, 0.0),
        ));
        assert_eq!(t.center, Vec3::new(3.0, 0.0, 0.0));
        assert!((t.radius - 4.0 * 0.75f32.sqrt()).abs() < 1e-5);
    }
}
