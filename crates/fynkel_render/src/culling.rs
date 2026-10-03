//! Frustum culling (blueprint §19.2d item 6, ADR-0011): every instance
//! carries a conservative static or posed bound into world space and is
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
    /// Unknown/invalid geometry is kept, never culled or used to tighten shadows.
    pub(crate) const UNBOUNDED: Self = Self {
        center: Vec3::ZERO,
        radius: f32::INFINITY,
    };

    pub(crate) fn is_finite(&self) -> bool {
        self.center.is_finite() && self.radius.is_finite() && self.radius >= 0.0
    }

    /// The smallest sphere this implementation builds around both: the
    /// one that contains the other when it does, else the sphere through
    /// their far sides. Used to gather what casts into one bound (R1).
    pub fn union(&self, other: &Sphere) -> Sphere {
        if !self.is_finite() || !other.is_finite() {
            return Self::UNBOUNDED;
        }
        let a = self.center.as_dvec3();
        let b = other.center.as_dvec3();
        let d = a.distance(b);
        if d + f64::from(other.radius) <= f64::from(self.radius) {
            return *self;
        }
        if d + f64::from(self.radius) <= f64::from(other.radius) {
            return *other;
        }
        let radius = (d + f64::from(self.radius) + f64::from(other.radius)) * 0.5;
        let center = (a + (b - a) * ((radius - f64::from(self.radius)) / d)).as_vec3();
        let radius = radius as f32;
        Sphere {
            center,
            radius: radius + 4.0 * f32::EPSILON * (center.abs().max_element() + radius),
        }
    }

    /// The sphere around a cooked mesh's axis-aligned bounds (its centre
    /// and half diagonal).
    pub fn from_bounds(min: [f32; 3], max: [f32; 3]) -> Sphere {
        let (min, max) = (Vec3::from(min), Vec3::from(max));
        Sphere {
            center: (min + max) * 0.5,
            radius: (max - min).length() * 0.5,
        }
    }

    /// Conservative under any finite affine map, including shear and collapse.
    /// The largest absolute row sum of A^T A bounds its largest eigenvalue;
    /// its square root bounds the stretch of every direction. For orthogonal
    /// columns this reduces to the largest axis scale.
    pub fn transformed(&self, model: &Mat4) -> Sphere {
        if !self.is_finite() || !affine(model) {
            return Self::UNBOUNDED;
        }
        let w = f64::from(model.w_axis.w);
        let axes = [
            model.x_axis.truncate().as_dvec3() / w,
            model.y_axis.truncate().as_dvec3() / w,
            model.z_axis.truncate().as_dvec3() / w,
        ];
        let stretch_squared = axes
            .iter()
            .map(|a| axes.iter().map(|b| a.dot(*b).abs()).sum::<f64>())
            .fold(0.0, f64::max);
        let center = model.transform_point3(self.center) / model.w_axis.w;
        let radius = (f64::from(self.radius) * stretch_squared.sqrt()) as f32;
        // Outward rounding also covers the centre and f32 shader arithmetic.
        let radius = radius + 8.0 * f32::EPSILON * (center.abs().max_element() + radius);
        let result = Sphere { center, radius };
        if result.is_finite() {
            result
        } else {
            Self::UNBOUNDED
        }
    }
}

fn affine(m: &Mat4) -> bool {
    m.is_finite() && m.x_axis.w == 0.0 && m.y_axis.w == 0.0 && m.z_axis.w == 0.0 && m.w_axis.w > 0.0
}

/// Bind-space spheres of vertices influenced by each joint. Constructed once
/// per mesh upload; O(influenced joints) per draw, with no CPU vertex skinning.
pub(crate) struct SkinBounds {
    joints: Vec<(usize, Sphere)>,
}

impl SkinBounds {
    pub(crate) fn new(vertices: &[crate::Vertex], skin: &[crate::SkinWeight]) -> Option<Self> {
        if skin.is_empty() || skin.len() != vertices.len() {
            return None;
        }
        let mut boxes = std::collections::BTreeMap::<usize, (Vec3, Vec3)>::new();
        for (vertex, weights) in vertices.iter().zip(skin) {
            let p = Vec3::from(vertex.position);
            if !p.is_finite()
                || weights.weights.iter().any(|w| !w.is_finite() || *w < 0.0)
                || weights.weights.iter().sum::<f32>() <= 0.0
            {
                return None;
            }
            for (&joint, &weight) in weights.joints.iter().zip(&weights.weights) {
                if weight == 0.0 {
                    continue;
                }
                let bounds = boxes.entry(usize::from(joint)).or_insert((p, p));
                bounds.0 = bounds.0.min(p);
                bounds.1 = bounds.1.max(p);
            }
        }
        Some(Self {
            joints: boxes
                .into_iter()
                .map(|(j, (min, max))| (j, Sphere::from_bounds(min.to_array(), max.to_array())))
                .collect(),
        })
    }

    pub(crate) fn joint_count(&self) -> u32 {
        self.joints.len() as u32
    }

    /// A sphere containing every transformed influence also contains their
    /// nonnegative weighted blends. Work in world space to avoid compounding
    /// an extra approximate sphere transform. Invalid palettes fail open.
    pub(crate) fn posed(&self, world: &Mat4, palette: &[Mat4]) -> Sphere {
        let mut bound: Option<Sphere> = None;
        for &(joint, sphere) in &self.joints {
            let Some(matrix) = palette.get(joint) else {
                return Sphere::UNBOUNDED;
            };
            if !affine(matrix) {
                return Sphere::UNBOUNDED;
            }
            let next = sphere.transformed(&(*world * *matrix));
            bound = Some(bound.map_or(next, |b| b.union(&next)));
        }
        bound.unwrap_or(Sphere::UNBOUNDED)
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

    /// Whether the complete finite sphere lies inside all six planes.
    pub(crate) fn contains(&self, s: &Sphere) -> bool {
        s.is_finite()
            && self
                .planes
                .iter()
                .all(|p| p.dot(s.center.extend(1.0)) >= s.radius)
    }

    /// Whether any part of the sphere may be inside.
    pub fn intersects(&self, s: &Sphere) -> bool {
        if !s.is_finite() {
            return true;
        }
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
    fn shear_bound_keeps_geometry_at_camera_and_shadow_planes() {
        let local = Sphere {
            center: Vec3::ZERO,
            radius: 1.0,
        };
        let linear = Mat4::from_scale(Vec3::new(2.0, 1.0, 1.0))
            * Mat4::from_rotation_z(std::f32::consts::FRAC_PI_4);
        let model = Mat4::from_translation(Vec3::new(2.8, 0.0, 0.5)) * linear;
        let witness = model.transform_point3(Vec3::new(-0.5f32.sqrt(), 0.5f32.sqrt(), 0.0));
        let old = Sphere {
            center: model.w_axis.truncate(),
            radius: linear.x_axis.truncate().length(),
        };
        let bound = local.transformed(&model);
        let camera = Frustum::from_view_proj(&Mat4::IDENTITY);
        assert!(
            !camera.intersects(&old),
            "old bound incorrectly drops visible geometry"
        );
        assert!(camera.intersects(&Sphere {
            center: witness,
            radius: 0.0
        }));
        assert!(camera.intersects(&bound));
        // A light's orthographic clip box is subject to the same error.
        let light = Frustum::from_view_proj(&Mat4::from_scale(Vec3::new(1.0, 0.5, 1.0)));
        assert!(!light.intersects(&old));
        assert!(light.intersects(&bound));
        // Exercise arbitrary directions, reflection and singular flattening.
        for scale in [Vec3::new(2.0, 1.0, -3.0), Vec3::new(0.0, 4.0, 1.0)] {
            let m =
                Mat4::from_scale(scale) * Mat4::from_rotation_z(0.7) * Mat4::from_rotation_x(0.4);
            let b = local.transformed(&m);
            for i in 0..1000 {
                let v = Vec3::new(
                    (i as f32 * 1.3).sin(),
                    (i as f32 * 0.7).cos(),
                    (i as f32 * 0.9).sin(),
                )
                .normalize();
                assert!(m.transform_point3(v).distance(b.center) <= b.radius);
            }
        }
    }

    #[test]
    fn influence_bounds_contain_weighted_deformation_with_shear_and_extra_placement() {
        let vertices: Vec<_> = (0..80)
            .map(|i| crate::Vertex {
                position: [(i as f32).sin(), i as f32 * 0.1, (i as f32).cos()],
                ..Default::default()
            })
            .collect();
        let weights: Vec<_> = (0..80)
            .map(|i| crate::SkinWeight {
                joints: [0, 1, 2, 0],
                weights: [i as f32 / 100.0, 0.2, 0.8 - i as f32 / 100.0, 0.0],
            })
            .collect();
        let bounds = SkinBounds::new(&vertices, &weights).unwrap();
        assert_eq!(bounds.joint_count(), 3);
        let world = Mat4::from_translation(Vec3::new(100.0, -50.0, 3.0))
            * Mat4::from_scale(Vec3::new(-2.0, 1.0, 3.0))
            * Mat4::from_rotation_z(0.4);
        for frame in 0..30 {
            let t = frame as f32 * 0.1;
            let joints = [
                Mat4::from_translation(Vec3::new(20.0, t, 0.0)),
                Mat4::from_rotation_z(t),
                Mat4::from_translation(Vec3::new(-5.0, 0.0, t))
                    * Mat4::from_scale(Vec3::new(1.0, 0.0, 1.0)),
            ];
            let bound = bounds.posed(&world, &joints);
            assert!(bound.is_finite());
            for (v, w) in vertices.iter().zip(&weights) {
                let mut point = Vec3::ZERO;
                for (&j, &weight) in w.joints.iter().zip(&w.weights) {
                    point +=
                        (world * joints[j as usize]).transform_point3(v.position.into()) * weight;
                }
                assert!(
                    point.distance(bound.center) <= bound.radius,
                    "frame={frame}"
                );
            }
        }
    }

    #[test]
    fn invalid_bounds_and_palettes_fail_open() {
        let vertices = [crate::Vertex::default()];
        let weights = [crate::SkinWeight {
            joints: [1, 0, 0, 0],
            weights: [1.0, 0.0, 0.0, 0.0],
        }];
        let bounds = SkinBounds::new(&vertices, &weights).unwrap();
        assert!(!bounds.posed(&Mat4::IDENTITY, &[Mat4::IDENTITY]).is_finite());
        assert!(!bounds
            .posed(
                &Mat4::IDENTITY,
                &[Mat4::IDENTITY, Mat4::from_cols_array(&[f32::NAN; 16])]
            )
            .is_finite());
        assert!(SkinBounds::new(&vertices, &[crate::SkinWeight::default()]).is_none());
        let invalid = Sphere {
            center: Vec3::NAN,
            radius: f32::NAN,
        };
        assert!(frustum().intersects(&invalid));
        assert!(!invalid.transformed(&Mat4::IDENTITY).is_finite());
        // Micro-scale union must not clamp the centre interpolation denominator.
        let a = Sphere {
            center: Vec3::ZERO,
            radius: 1e-9,
        };
        let b = Sphere {
            center: Vec3::X * 1e-7,
            radius: 2e-9,
        };
        let u = a.union(&b);
        for s in [a, b] {
            assert!(u.center.distance(s.center) + s.radius <= u.radius);
        }
    }

    #[test]
    #[ignore = "manual release timing; not a performance assertion"]
    fn benchmark_animated_bounds() {
        use std::{hint::black_box, time::Instant};
        let vertices: Vec<_> = (0..64)
            .map(|i| crate::Vertex {
                position: [0.0, i as f32 * 0.03, 0.0],
                ..Default::default()
            })
            .collect();
        let weights: Vec<_> = (0..64)
            .map(|i| crate::SkinWeight {
                joints: [i, 0, 0, 0],
                weights: [1.0, 0.0, 0.0, 0.0],
            })
            .collect();
        let bounds = SkinBounds::new(&vertices, &weights).unwrap();
        let palette: Vec<_> = (0..64)
            .map(|i| Mat4::from_rotation_z(i as f32 * 0.02))
            .collect();
        for rigs in [1, 200, 1000] {
            let mut micros = Vec::new();
            for _ in 0..30 {
                let start = Instant::now();
                for _ in 0..rigs {
                    black_box(bounds.posed(black_box(&Mat4::IDENTITY), black_box(&palette)));
                }
                micros.push(start.elapsed().as_secs_f64() * 1e6);
            }
            micros.sort_by(f64::total_cmp);
            eprintln!(
                "bounds_bench rigs={rigs} joints_per_rig=64 median_us={:.3} p95_us={:.3}",
                micros[15], micros[28]
            );
        }
    }

    #[test]
    fn a_union_contains_both_spheres() {
        let a = Sphere {
            center: Vec3::new(-2.0, 0.0, 0.0),
            radius: 1.0,
        };
        let b = Sphere {
            center: Vec3::new(6.0, 0.0, 0.0),
            radius: 3.0,
        };
        let u = a.union(&b);
        for s in [a, b] {
            assert!(u.center.distance(s.center) + s.radius <= u.radius + 1e-4);
        }
        // Tight: the union spans exactly the two far sides.
        assert!((u.radius - 6.0).abs() < 1e-4, "{}", u.radius);
        // A sphere already inside the other is swallowed, either way round.
        let inner = Sphere {
            center: Vec3::new(6.5, 0.0, 0.0),
            radius: 0.5,
        };
        assert_eq!(b.union(&inner), b);
        assert_eq!(inner.union(&b), b);
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
