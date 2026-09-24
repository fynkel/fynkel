//! Post-processing volumes (R3a, `docs/renderer.md` § Volumes): the look
//! a frame is drawn with — ambient occlusion (R3a), bloom (R3b) —
//! resolved once per frame on the CPU from the scene's [`Volume`]
//! components and their profiles.
//!
//! A [`VolumeProfile`] (a `.volume.json`) holds *overrides*: a field that
//! is present replaces the engine default, a field that is absent leaves
//! whatever is below it. [`resolve`] starts from [`PostSettings::default`]
//! (every effect off: a scene without volumes has no post effects)
//! and blends the contributing volumes over it in priority order (ties in
//! scene order), each by its weight × [`factor`] — 1 inside the shape,
//! falling linearly to 0 at `blend_distance` outside it. The result is
//! the frame's settings; what the quality profile turns off stays off
//! (the renderer's job, not this module's).

use glam::{Mat4, Vec3};

use crate::components::{Volume, VolumeShape};

/// The ambient occlusion block of a profile (R2's constants, R3a).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AoOverrides {
    /// Hemisphere radius in metres, `> 0`.
    pub radius: Option<f32>,
    /// The exponent the occlusion is shaped by: 0 turns it off, 1 is
    /// linear, larger is darker.
    pub intensity: Option<f32>,
}

/// The bloom block of a profile (R3b): light brighter than the threshold
/// spreads into its neighbourhood.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BloomOverrides {
    /// How much of the blurred light is added back; 0 is off.
    pub intensity: Option<f32>,
    /// The exposed brightness (1 = white after exposure) above which a
    /// pixel starts to glow, with a soft knee of half the threshold.
    pub threshold: Option<f32>,
    /// `0..=1`: how far the glow spreads — how much of each coarser level
    /// is kept against the finer one.
    pub scatter: Option<f32>,
    /// Linear RGB multiplied into the glow.
    pub tint: Option<Vec3>,
}

/// A `.volume.json`: the override blocks, each field optional.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VolumeProfile {
    pub ambient_occlusion: AoOverrides,
    pub bloom: BloomOverrides,
}

/// The resolved look of one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PostSettings {
    pub ao_radius: f32,
    pub ao_intensity: f32,
    pub bloom_intensity: f32,
    pub bloom_threshold: f32,
    pub bloom_scatter: f32,
    pub bloom_tint: Vec3,
}

/// Ambient occlusion is off until a volume gives it an intensity (as in
/// Unity: no volume, no post effects; 2026-09-24 — before, R2's 1.35 was
/// always on and a volume could barely be told apart). The radius is what
/// an intensity alone gets.
pub const DEFAULT_AO_RADIUS: f32 = 0.6;
pub const DEFAULT_AO_INTENSITY: f32 = 0.0;
/// Bloom is off until a volume gives it an intensity (R3b); the other
/// defaults are what an intensity alone gets.
pub const DEFAULT_BLOOM_INTENSITY: f32 = 0.0;
pub const DEFAULT_BLOOM_THRESHOLD: f32 = 0.9;
pub const DEFAULT_BLOOM_SCATTER: f32 = 0.7;

impl Default for PostSettings {
    fn default() -> Self {
        Self {
            ao_radius: DEFAULT_AO_RADIUS,
            ao_intensity: DEFAULT_AO_INTENSITY,
            bloom_intensity: DEFAULT_BLOOM_INTENSITY,
            bloom_threshold: DEFAULT_BLOOM_THRESHOLD,
            bloom_scatter: DEFAULT_BLOOM_SCATTER,
            bloom_tint: Vec3::ONE,
        }
    }
}

/// How much of a volume reaches `point` (world space), before its
/// `weight`: 1 inside the shape, `1 − d / blend_distance` at world
/// distance `d` outside it, 0 beyond. A box or sphere is measured in the
/// entity's local space (its rotation and scale apply) and the distance
/// back in world metres; a transform that cannot be inverted reaches
/// nothing.
pub fn factor(volume: &Volume, world: &Mat4, point: Vec3) -> f32 {
    let local_closest = |local: Vec3| -> Vec3 {
        match volume.shape {
            VolumeShape::Global => local,
            VolumeShape::Box => {
                let half = volume.size * 0.5;
                local.clamp(-half, half)
            }
            VolumeShape::Sphere => {
                let len = local.length();
                if len <= volume.radius {
                    local
                } else {
                    local * (volume.radius / len)
                }
            }
        }
    };
    if volume.shape == VolumeShape::Global {
        return 1.0;
    }
    if world.determinant().abs() <= f32::EPSILON * f32::EPSILON || !world.is_finite() {
        return 0.0;
    }
    let local = world.inverse().transform_point3(point);
    let closest = world.transform_point3(local_closest(local));
    let d = closest.distance(point);
    if d <= 1e-5 {
        return 1.0;
    }
    if volume.blend_distance <= 0.0 {
        return 0.0;
    }
    (1.0 - d / volume.blend_distance).clamp(0.0, 1.0)
}

/// One volume as [`resolve`] takes it: its profile, and how much of it
/// applies this frame (`weight × factor`, already computed).
#[derive(Clone, Copy, Debug)]
pub struct Contribution<'a> {
    pub profile: &'a VolumeProfile,
    pub amount: f32,
    pub priority: i32,
}

/// The frame's settings: `base` with every contribution blended over it,
/// lowest priority first; a stable sort keeps scene order among equals,
/// so a later volume blends over an earlier one of the same priority.
/// Returns the settings and how many volumes contributed (amount > 0).
pub fn resolve(base: PostSettings, contributions: &mut [Contribution<'_>]) -> (PostSettings, u32) {
    contributions.sort_by_key(|c| c.priority);
    let mut s = base;
    let mut used = 0;
    for c in contributions.iter() {
        let t = c.amount.clamp(0.0, 1.0);
        if t <= 0.0 {
            continue;
        }
        used += 1;
        let ao = &c.profile.ambient_occlusion;
        blend(&mut s.ao_radius, ao.radius, t);
        blend(&mut s.ao_intensity, ao.intensity, t);
        let bloom = &c.profile.bloom;
        blend(&mut s.bloom_intensity, bloom.intensity, t);
        blend(&mut s.bloom_threshold, bloom.threshold, t);
        blend(&mut s.bloom_scatter, bloom.scatter, t);
        if let Some(tint) = bloom.tint {
            s.bloom_tint += (tint - s.bloom_tint) * t;
        }
    }
    (s, used)
}

fn blend(value: &mut f32, over: Option<f32>, t: f32) {
    if let Some(v) = over {
        *value += (v - *value) * t;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;

    fn volume(shape: VolumeShape) -> Volume {
        Volume {
            shape,
            size: Vec3::new(4.0, 2.0, 6.0),
            radius: 3.0,
            blend_distance: 2.0,
            ..Volume::default()
        }
    }

    #[test]
    fn a_global_volume_reaches_everywhere() {
        let v = volume(VolumeShape::Global);
        assert_eq!(factor(&v, &Mat4::IDENTITY, Vec3::splat(1e6)), 1.0);
    }

    #[test]
    fn a_box_is_full_inside_and_fades_over_the_blend_distance() {
        let v = volume(VolumeShape::Box);
        let at = Mat4::from_translation(Vec3::new(10.0, 0.0, 0.0));
        assert_eq!(factor(&v, &at, Vec3::new(11.9, 0.9, -2.9)), 1.0);
        // 1 m past the +X face (at x = 12) of a 2 m fade: half.
        assert!((factor(&v, &at, Vec3::new(13.0, 0.0, 0.0)) - 0.5).abs() < 1e-5);
        assert_eq!(factor(&v, &at, Vec3::new(14.5, 0.0, 0.0)), 0.0);
        // Rotation and scale apply: turned 90° about Y and doubled, the
        // box's local X (4 m → 8 m) runs along world −Z.
        let turned = Mat4::from_scale_rotation_translation(
            Vec3::splat(2.0),
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            Vec3::ZERO,
        );
        assert_eq!(factor(&v, &turned, Vec3::new(0.0, 0.0, -3.9)), 1.0);
        assert!((factor(&v, &turned, Vec3::new(0.0, 0.0, -5.0)) - 0.5).abs() < 1e-4);
    }

    #[test]
    fn a_sphere_fades_from_its_surface_and_a_hard_edge_is_binary() {
        let mut v = volume(VolumeShape::Sphere);
        assert_eq!(factor(&v, &Mat4::IDENTITY, Vec3::new(0.0, 2.9, 0.0)), 1.0);
        assert!((factor(&v, &Mat4::IDENTITY, Vec3::new(0.0, 3.5, 0.0)) - 0.75).abs() < 1e-5);
        v.blend_distance = 0.0;
        assert_eq!(factor(&v, &Mat4::IDENTITY, Vec3::new(0.0, 3.01, 0.0)), 0.0);
        assert_eq!(factor(&v, &Mat4::ZERO, Vec3::ZERO), 0.0);
    }

    #[test]
    fn resolve_blends_present_fields_in_priority_order() {
        let radius = VolumeProfile {
            ambient_occlusion: AoOverrides {
                radius: Some(1.6),
                intensity: None,
            },
            ..VolumeProfile::default()
        };
        let off = VolumeProfile {
            ambient_occlusion: AoOverrides {
                radius: None,
                intensity: Some(0.0),
            },
            ..VolumeProfile::default()
        };
        let base = PostSettings::default();
        // Absent fields inherit: the radius profile leaves intensity alone.
        let (s, n) = resolve(
            base,
            &mut [Contribution {
                profile: &radius,
                amount: 1.0,
                priority: 0,
            }],
        );
        assert_eq!(
            (s.ao_radius, s.ao_intensity, n),
            (1.6, DEFAULT_AO_INTENSITY, 1)
        );
        // Half a volume is half way.
        let (s, _) = resolve(
            base,
            &mut [Contribution {
                profile: &radius,
                amount: 0.5,
                priority: 0,
            }],
        );
        assert!((s.ao_radius - 1.1).abs() < 1e-6);
        // The higher priority lands last whatever the order given, and a
        // volume that does not reach the camera does not count.
        let mut both = [
            Contribution {
                profile: &off,
                amount: 1.0,
                priority: 5,
            },
            Contribution {
                profile: &VolumeProfile {
                    ambient_occlusion: AoOverrides {
                        radius: None,
                        intensity: Some(2.0),
                    },
                    ..VolumeProfile::default()
                },
                amount: 1.0,
                priority: 1,
            },
            Contribution {
                profile: &radius,
                amount: 0.0,
                priority: 9,
            },
        ];
        let (s, n) = resolve(base, &mut both);
        assert_eq!(
            (s.ao_radius, s.ao_intensity, n),
            (DEFAULT_AO_RADIUS, 0.0, 2)
        );
    }

    #[test]
    fn bloom_is_off_by_default_and_its_tint_blends_per_channel() {
        let base = PostSettings::default();
        assert_eq!(base.bloom_intensity, 0.0);
        assert_eq!(base.ao_intensity, 0.0, "no volume, no ambient occlusion");
        let warm = VolumeProfile {
            bloom: BloomOverrides {
                intensity: Some(2.0),
                tint: Some(Vec3::new(1.0, 0.5, 0.0)),
                ..BloomOverrides::default()
            },
            ..VolumeProfile::default()
        };
        let (s, n) = resolve(
            base,
            &mut [Contribution {
                profile: &warm,
                amount: 0.5,
                priority: 0,
            }],
        );
        assert_eq!(n, 1);
        assert!((s.bloom_intensity - 1.0).abs() < 1e-6);
        assert!((s.bloom_tint - Vec3::new(1.0, 0.75, 0.5)).length() < 1e-6);
        // Absent fields inherit, and the AO block is untouched.
        assert_eq!(
            (s.bloom_threshold, s.bloom_scatter, s.ao_radius),
            (
                DEFAULT_BLOOM_THRESHOLD,
                DEFAULT_BLOOM_SCATTER,
                DEFAULT_AO_RADIUS
            )
        );
    }
}
