//! Post-processing volumes (R3a, `docs/renderer.md` § Volumes): the look
//! a frame is drawn with — ambient occlusion (R3a), bloom (R3b), colour
//! grading (R3c) — resolved once per frame on the CPU from the scene's [`Volume`]
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

use glam::{Mat3, Mat4, Vec3};

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

/// The curve that brings the graded scene's radiance into the display's
/// `0..=1` (R3c). ACES is what the engine always drew with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tonemapper {
    /// ACES filmic (Stephen Hill's fit): contrasty, highlights roll off
    /// toward white.
    #[default]
    Aces,
    /// Unity's Neutral curve: little hue or saturation shift, a gentler
    /// shoulder — a starting point for grading by hand.
    Neutral,
    /// No curve: clamped at white.
    None,
}

impl Tonemapper {
    pub const ALL: [Tonemapper; 3] = [Tonemapper::Aces, Tonemapper::Neutral, Tonemapper::None];

    /// The document's name for it.
    pub fn name(self) -> &'static str {
        match self {
            Tonemapper::Aces => "aces",
            Tonemapper::Neutral => "neutral",
            Tonemapper::None => "none",
        }
    }

    pub fn from_name(name: &str) -> Option<Tonemapper> {
        Tonemapper::ALL.into_iter().find(|t| t.name() == name)
    }
}

/// The colour grading block of a profile (R3c), Unity's Tonemapping,
/// White Balance, Color Adjustments and Lift Gamma Gain in one.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GradingOverrides {
    pub tonemapper: Option<Tonemapper>,
    /// Stops added to the camera's exposure.
    pub post_exposure: Option<f32>,
    /// `-100..=100`: cooler (blue) to warmer (yellow).
    pub temperature: Option<f32>,
    /// `-100..=100`: green to magenta.
    pub tint: Option<f32>,
    /// `-100..=100`: flatter to punchier, around middle grey.
    pub contrast: Option<f32>,
    /// `-100..=100`: grey to vivid.
    pub saturation: Option<f32>,
    /// Degrees, `-180..=180`: every hue turned round the colour wheel.
    pub hue_shift: Option<f32>,
    /// Linear RGB multiplied into the scene.
    pub color_filter: Option<Vec3>,
    /// Per channel, `-1..=1`: raises (or lowers) the darks.
    pub lift: Option<Vec3>,
    /// Per channel, `> 0`: above 1 brightens the midtones.
    pub gamma: Option<Vec3>,
    /// Per channel, `>= 0`: scales the brights.
    pub gain: Option<Vec3>,
}

/// A `.volume.json`: the override blocks, each field optional.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VolumeProfile {
    pub ambient_occlusion: AoOverrides,
    pub bloom: BloomOverrides,
    pub color_grading: GradingOverrides,
}

/// The resolved colour grading of one frame (R3c). The default is neutral:
/// ACES and nothing else, the picture the engine drew before grading.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grading {
    pub tonemapper: Tonemapper,
    pub post_exposure: f32,
    pub temperature: f32,
    pub tint: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub hue_shift: f32,
    pub color_filter: Vec3,
    pub lift: Vec3,
    pub gamma: Vec3,
    pub gain: Vec3,
}

impl Default for Grading {
    fn default() -> Self {
        Self {
            tonemapper: Tonemapper::Aces,
            post_exposure: 0.0,
            temperature: 0.0,
            tint: 0.0,
            contrast: 0.0,
            saturation: 0.0,
            hue_shift: 0.0,
            color_filter: Vec3::ONE,
            lift: Vec3::ZERO,
            gamma: Vec3::ONE,
            gain: Vec3::ONE,
        }
    }
}

/// Rec. 709 luma weights (linear sRGB primaries).
const LUMA: Vec3 = Vec3::new(0.2126, 0.7152, 0.0722);

impl Grading {
    /// Whether anything but the tone mapper differs from neutral: when
    /// not, the renderer skips the grade and the picture is the ungraded
    /// one to the bit.
    pub fn is_active(&self) -> bool {
        let neutral = Grading {
            tonemapper: self.tonemapper,
            ..Grading::default()
        };
        *self != neutral
    }

    /// What the grade does before the contrast curve, as one matrix on
    /// linear RGB: the post-exposure, then the white balance (Unity's:
    /// a von Kries scale in LMS toward the illuminant the temperature
    /// and tint name).
    pub fn pre_contrast(&self) -> Mat3 {
        let lms = white_balance_lms(self.temperature, self.tint);
        let wb = LMS_TO_LINEAR * Mat3::from_diagonal(lms) * LINEAR_TO_LMS;
        wb * 2f32.powf(self.post_exposure.clamp(-20.0, 20.0))
    }

    /// The contrast curve's exponent in log space around middle grey:
    /// `1 + contrast / 100`, at least 0.01.
    pub fn contrast_exponent(&self) -> f32 {
        (1.0 + self.contrast.clamp(-100.0, 100.0) / 100.0).max(0.01)
    }

    /// What the grade does after the contrast curve, as one matrix: the
    /// colour filter, the hue shift (a turn about the grey axis), then
    /// the saturation (toward or away from the pixel's luma).
    pub fn post_contrast(&self) -> Mat3 {
        let s = 1.0 + self.saturation.clamp(-100.0, 100.0) / 100.0;
        let luma = Mat3::from_cols(LUMA, LUMA, LUMA).transpose();
        let saturation = Mat3::IDENTITY * s + luma * (1.0 - s);
        saturation
            * hue_rotation(self.hue_shift.to_radians())
            * Mat3::from_diagonal(self.color_filter.max(Vec3::ZERO))
    }
}

/// Rodrigues' rotation by `angle` about the unit grey axis `(1,1,1)/√3`:
/// grey stays grey, every hue turns by the same angle.
fn hue_rotation(angle: f32) -> Mat3 {
    if angle == 0.0 {
        return Mat3::IDENTITY;
    }
    let (sin, cos) = angle.sin_cos();
    let k = 1.0 / 3.0;
    let r = 3f32.sqrt().recip();
    let a = cos + (1.0 - cos) * k;
    let b = (1.0 - cos) * k - r * sin;
    let c = (1.0 - cos) * k + r * sin;
    // Columns of [[a, b, c], [c, a, b], [b, c, a]].
    Mat3::from_cols(Vec3::new(a, c, b), Vec3::new(b, a, c), Vec3::new(c, b, a))
}

/// Linear sRGB → LMS (CAT02 via XYZ), and back: Unity's matrices.
const LINEAR_TO_LMS: Mat3 = Mat3::from_cols(
    Vec3::new(3.904_05e-1, 7.084_16e-2, 2.310_82e-2),
    Vec3::new(5.499_41e-1, 9.631_72e-1, 1.280_21e-1),
    Vec3::new(8.926_32e-3, 1.357_75e-3, 9.362_45e-1),
);
const LMS_TO_LINEAR: Mat3 = Mat3::from_cols(
    Vec3::new(2.858_47, -2.101_82e-1, -4.181_20e-2),
    Vec3::new(-1.628_79, 1.158_20, -1.181_69e-1),
    Vec3::new(-2.489_10e-2, 3.242_81e-4, 1.068_67),
);

/// Unity's `ColorBalanceToLMSCoeffs`: the LMS scale that takes the
/// illuminant a temperature and tint name (a CIE daylight chromaticity
/// shifted along the Planckian locus and across it) to D65. Both 0 is
/// (to within rounding) no change.
pub fn white_balance_lms(temperature: f32, tint: f32) -> Vec3 {
    let t1 = temperature.clamp(-100.0, 100.0) / 65.0;
    let t2 = tint.clamp(-100.0, 100.0) / 65.0;
    let x = 0.31271 - t1 * if t1 < 0.0 { 0.1 } else { 0.05 };
    let y = 2.87 * x - 3.0 * x * x - 0.275_095_07 + t2 * 0.05;
    let d65 = Vec3::new(0.949_237, 1.035_42, 1.087_28);
    d65 / xy_to_lms(x, y)
}

fn xy_to_lms(x: f32, y: f32) -> Vec3 {
    let big_x = x / y;
    let big_z = (1.0 - x - y) / y;
    Vec3::new(
        0.7328 * big_x + 0.4296 - 0.1624 * big_z,
        -0.7036 * big_x + 1.6975 + 0.0061 * big_z,
        0.0030 * big_x + 0.0136 + 0.9834 * big_z,
    )
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
    pub grading: Grading,
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
            grading: Grading::default(),
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
        blend3(&mut s.bloom_tint, bloom.tint, t);
        let c = &c.profile.color_grading;
        let g = &mut s.grading;
        // A choice does not blend: any part of a volume sets it (Unity's
        // rule for its enum parameters).
        if let Some(tonemapper) = c.tonemapper {
            g.tonemapper = tonemapper;
        }
        blend(&mut g.post_exposure, c.post_exposure, t);
        blend(&mut g.temperature, c.temperature, t);
        blend(&mut g.tint, c.tint, t);
        blend(&mut g.contrast, c.contrast, t);
        blend(&mut g.saturation, c.saturation, t);
        blend(&mut g.hue_shift, c.hue_shift, t);
        blend3(&mut g.color_filter, c.color_filter, t);
        blend3(&mut g.lift, c.lift, t);
        blend3(&mut g.gamma, c.gamma, t);
        blend3(&mut g.gain, c.gain, t);
    }
    (s, used)
}

fn blend(value: &mut f32, over: Option<f32>, t: f32) {
    if let Some(v) = over {
        *value += (v - *value) * t;
    }
}

/// Per channel.
fn blend3(value: &mut Vec3, over: Option<Vec3>, t: f32) {
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
    fn grading_is_neutral_by_default_and_blends_like_the_rest() {
        let base = PostSettings::default();
        assert!(!base.grading.is_active());
        assert_eq!(base.grading.tonemapper, Tonemapper::Aces);
        // Neutral is (to within rounding) the identity.
        let g = Grading::default();
        assert!(
            g.pre_contrast().abs_diff_eq(Mat3::IDENTITY, 1e-3),
            "{:?}",
            g.pre_contrast()
        );
        assert!(g.post_contrast().abs_diff_eq(Mat3::IDENTITY, 1e-6));
        assert_eq!(g.contrast_exponent(), 1.0);
        // A tone mapper alone is no grade.
        let neutral = VolumeProfile {
            color_grading: GradingOverrides {
                tonemapper: Some(Tonemapper::Neutral),
                ..GradingOverrides::default()
            },
            ..VolumeProfile::default()
        };
        let warm = VolumeProfile {
            color_grading: GradingOverrides {
                temperature: Some(40.0),
                gain: Some(Vec3::new(2.0, 1.0, 1.0)),
                ..GradingOverrides::default()
            },
            ..VolumeProfile::default()
        };
        let (s, _) = resolve(
            base,
            &mut [
                Contribution {
                    profile: &neutral,
                    amount: 0.25,
                    priority: 0,
                },
                Contribution {
                    profile: &warm,
                    amount: 0.5,
                    priority: 1,
                },
            ],
        );
        // The choice is taken by any part of a volume; numbers blend.
        assert_eq!(s.grading.tonemapper, Tonemapper::Neutral);
        assert!((s.grading.temperature - 20.0).abs() < 1e-5);
        assert!((s.grading.gain - Vec3::new(1.5, 1.0, 1.0)).length() < 1e-6);
        assert!(s.grading.is_active());
        // Warmer takes blue down against red.
        let m = s.grading.pre_contrast();
        let white = m * Vec3::ONE;
        assert!(white.x > white.z, "{white:?}");
    }

    #[test]
    fn saturation_and_hue_keep_grey_and_move_colour() {
        let grey = Vec3::splat(0.4);
        let red = Vec3::new(0.8, 0.1, 0.1);
        let flat = Grading {
            saturation: -100.0,
            ..Grading::default()
        };
        let out = flat.post_contrast() * red;
        assert!(
            (out.x - out.y).abs() < 1e-6 && (out.y - out.z).abs() < 1e-6,
            "{out:?}"
        );
        assert!((out.x - red.dot(LUMA)).abs() < 1e-6);
        let turned = Grading {
            hue_shift: 120.0,
            ..Grading::default()
        };
        assert!((turned.post_contrast() * grey - grey).length() < 1e-5);
        // A third of the way round, red becomes green.
        let out = turned.post_contrast() * red;
        assert!((out - Vec3::new(0.1, 0.8, 0.1)).length() < 1e-4, "{out:?}");
        assert_eq!(Tonemapper::from_name("neutral"), Some(Tonemapper::Neutral));
        assert_eq!(Tonemapper::from_name("filmic"), None);
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
