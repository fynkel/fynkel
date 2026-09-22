//! Image-based lighting products (E1; `docs/renderer.md` § Image-based
//! lighting): an HDR equirectangular environment prefiltered at cook time
//! into what the split-sum approximation samples per pixel — a specular
//! cube map whose mip levels are the environment convolved with GGX at
//! rising roughness (importance-sampled, the lookups filtered by the
//! solid angle each sample covers so the result is smooth at 256
//! samples), and an irradiance cube map holding the cosine-weighted
//! irradiance for every normal, stored as E/π. The equirect mapping and
//! the cube face convention are the shaders' (`sky.wgsl`; WebGPU's +X −X
//! +Y −Y +Z −Z order with the GL texel-to-direction rule), so the
//! reflection in a mirror lines up with the sky behind it.
//!
//! Everything here is deterministic: fixed sample sets, fixed order, `f32`
//! throughout — the cooked bytes are a pure function of the source.

use std::f32::consts::PI;

use fynkel_cooked::payload::{f32_to_f16, EnvironmentPayload};

/// Base size of the specular cube's faces.
pub const SPECULAR_SIZE: u32 = 128;
/// Specular mip levels: roughness 0 (the mirror) at level 0, roughness 1
/// at the last, linearly between.
pub const SPECULAR_MIPS: u32 = 6;
/// Face size of the irradiance cube (a cosine lobe has no detail).
pub const IRRADIANCE_SIZE: u32 = 32;
/// GGX samples per specular texel (levels 1 and up).
const SAMPLES: u32 = 256;
/// The irradiance integral walks the equirect level no wider than this.
const IRRADIANCE_SOURCE_WIDTH: usize = 128;

type Vec3 = [f32; 3];

/// Prefilters a `width`×`height` linear RGB `f32` equirect image.
pub fn prefilter(width: u32, height: u32, rgb: &[f32]) -> EnvironmentPayload {
    let source = Equirect::new(width as usize, height as usize, rgb);
    let mut specular = Vec::new();
    for mip in 0..SPECULAR_MIPS {
        let roughness = mip as f32 / (SPECULAR_MIPS - 1) as f32;
        let size = (SPECULAR_SIZE >> mip).max(1) as usize;
        specular.extend(specular_level(&source, size, roughness));
    }
    let (irradiance, mean_irradiance) = irradiance_cube(&source, IRRADIANCE_SIZE as usize);
    EnvironmentPayload {
        specular_size: SPECULAR_SIZE,
        specular_mips: SPECULAR_MIPS,
        irradiance_size: IRRADIANCE_SIZE,
        mean_irradiance,
        specular: specular.iter().map(|v| f32_to_f16(*v)).collect(),
        irradiance: irradiance.iter().map(|v| f32_to_f16(*v)).collect(),
    }
}

/// One specular level: six faces of `size²` RGBA texels.
fn specular_level(source: &Equirect, size: usize, roughness: f32) -> Vec<f32> {
    let mut out = Vec::with_capacity(size * size * 6 * 4);
    // Solid angle of one cube texel and of one source texel: the mirror
    // level reads the source filtered to the cube's resolution, the rough
    // levels filter each sample by the lobe it stands for.
    let cube_texel = 4.0 * PI / (6.0 * (size * size) as f32);
    let source_texel = 4.0 * PI / (source.levels[0].width * source.levels[0].height) as f32;
    let mirror_lod = 0.5 * (cube_texel / source_texel).log2();
    let a2 = (roughness * roughness) * (roughness * roughness);
    // The GGX lobe sampled once, in tangent space around +Z, with the lod
    // its pdf asks for (Ωs = 1 / (N · pdf), pdf = D / 4 with V = N).
    let samples: Vec<(Vec3, f32)> = if roughness == 0.0 {
        Vec::new()
    } else {
        (0..SAMPLES)
            .map(|i| {
                let (x1, x2) = hammersley(i, SAMPLES);
                let phi = 2.0 * PI * x1;
                let cos_theta = ((1.0 - x2) / (1.0 + (a2 - 1.0) * x2)).sqrt();
                let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
                let h = [sin_theta * phi.cos(), sin_theta * phi.sin(), cos_theta];
                let d = a2 / (PI * (cos_theta * cos_theta * (a2 - 1.0) + 1.0).powi(2));
                let pdf = (d / 4.0).max(1e-6);
                let omega_s = 1.0 / (SAMPLES as f32 * pdf);
                let lod = 0.5 * (omega_s / source_texel).log2() + 1.0;
                (h, lod.max(0.0))
            })
            .collect()
    };
    for face in 0..6 {
        for y in 0..size {
            for x in 0..size {
                let n = face_direction(face, x, y, size);
                let rgb = if samples.is_empty() {
                    source.sample(n, mirror_lod)
                } else {
                    let (t, b) = tangent_frame(n);
                    let mut sum = [0.0f32; 3];
                    let mut weight = 0.0f32;
                    for (h, lod) in &samples {
                        let h = [
                            t[0] * h[0] + b[0] * h[1] + n[0] * h[2],
                            t[1] * h[0] + b[1] * h[1] + n[1] * h[2],
                            t[2] * h[0] + b[2] * h[1] + n[2] * h[2],
                        ];
                        // L = reflect(-V, H) with V = N.
                        let vh = dot(n, h);
                        let l = [
                            2.0 * vh * h[0] - n[0],
                            2.0 * vh * h[1] - n[1],
                            2.0 * vh * h[2] - n[2],
                        ];
                        let nl = dot(n, l);
                        if nl > 0.0 {
                            let c = source.sample(l, *lod);
                            sum[0] += c[0] * nl;
                            sum[1] += c[1] * nl;
                            sum[2] += c[2] * nl;
                            weight += nl;
                        }
                    }
                    if weight > 0.0 {
                        [sum[0] / weight, sum[1] / weight, sum[2] / weight]
                    } else {
                        [0.0; 3]
                    }
                };
                out.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 1.0]);
            }
        }
    }
    out
}

/// The irradiance cube (E/π per texel) and the mean of E over the sphere.
fn irradiance_cube(source: &Equirect, size: usize) -> (Vec<f32>, Vec3) {
    // Integrate over a coarse level: the cosine lobe is wide, the level's
    // texels are pre-averaged, and the sum stays exact for what it sees.
    let level = source
        .levels
        .iter()
        .find(|l| l.width <= IRRADIANCE_SOURCE_WIDTH)
        .unwrap_or(source.levels.last().unwrap());
    let (w, h) = (level.width, level.height);
    // Every source texel's direction, radiance and solid angle.
    let mut texels = Vec::with_capacity(w * h);
    for y in 0..h {
        let theta = PI * (y as f32 + 0.5) / h as f32;
        let d_omega = (2.0 * PI / w as f32) * (PI / h as f32) * theta.sin();
        for x in 0..w {
            let phi = ((x as f32 + 0.5) / w as f32 - 0.5) * 2.0 * PI;
            let dir = [
                theta.sin() * phi.sin(),
                theta.cos(),
                -theta.sin() * phi.cos(),
            ];
            let at = (y * w + x) * 3;
            texels.push((
                dir,
                [level.rgb[at], level.rgb[at + 1], level.rgb[at + 2]],
                d_omega,
            ));
        }
    }
    let mut out = Vec::with_capacity(size * size * 6 * 4);
    let mut mean = [0.0f32; 3];
    let mut mean_weight = 0.0f32;
    for face in 0..6 {
        for y in 0..size {
            for x in 0..size {
                let n = face_direction(face, x, y, size);
                let mut e = [0.0f32; 3];
                for (dir, rgb, d_omega) in &texels {
                    let cos = dot(n, *dir);
                    if cos > 0.0 {
                        let k = cos * d_omega;
                        e[0] += rgb[0] * k;
                        e[1] += rgb[1] * k;
                        e[2] += rgb[2] * k;
                    }
                }
                // Weight the mean by the texel's solid angle (a cube's
                // corners cover less sky than its centres).
                let s = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let t = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let solid = 1.0 / (1.0 + s * s + t * t).powf(1.5);
                mean[0] += e[0] * solid;
                mean[1] += e[1] * solid;
                mean[2] += e[2] * solid;
                mean_weight += solid;
                out.extend_from_slice(&[e[0] / PI, e[1] / PI, e[2] / PI, 1.0]);
            }
        }
    }
    let mean = [
        mean[0] / mean_weight,
        mean[1] / mean_weight,
        mean[2] / mean_weight,
    ];
    (out, mean)
}

/// The direction through the centre of texel (`x`, `y`) of cube face
/// `face` (+X −X +Y −Y +Z −Z), row 0 at the top — the GL/WebGPU rule.
fn face_direction(face: usize, x: usize, y: usize, size: usize) -> Vec3 {
    let s = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
    let t = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
    normalize(match face {
        0 => [1.0, -t, -s],
        1 => [-1.0, -t, s],
        2 => [s, 1.0, t],
        3 => [s, -1.0, -t],
        4 => [s, -t, 1.0],
        _ => [-s, -t, -1.0],
    })
}

/// A tangent and bitangent around `n`.
fn tangent_frame(n: Vec3) -> (Vec3, Vec3) {
    let up = if n[1].abs() < 0.999 {
        [0.0, 1.0, 0.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let t = normalize(cross(up, n));
    let b = cross(n, t);
    (t, b)
}

/// The Hammersley point set: `(i / n, radical inverse of i)`.
fn hammersley(i: u32, n: u32) -> (f32, f32) {
    let mut bits = i;
    bits = bits.rotate_right(16);
    bits = ((bits & 0x5555_5555) << 1) | ((bits & 0xAAAA_AAAA) >> 1);
    bits = ((bits & 0x3333_3333) << 2) | ((bits & 0xCCCC_CCCC) >> 2);
    bits = ((bits & 0x0F0F_0F0F) << 4) | ((bits & 0xF0F0_F0F0) >> 4);
    bits = ((bits & 0x00FF_00FF) << 8) | ((bits & 0xFF00_FF00) >> 8);
    (i as f32 / n as f32, bits as f32 * 2.328_306_4e-10)
}

fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(v: Vec3) -> Vec3 {
    let len = dot(v, v).sqrt().max(1e-20);
    [v[0] / len, v[1] / len, v[2] / len]
}

struct Level {
    width: usize,
    height: usize,
    /// RGB, top row first.
    rgb: Vec<f32>,
}

/// An equirect image with its box-filtered chain down to 1×1, sampled
/// trilinearly (u wraps, v clamps) so a lookup can ask for the level
/// whose texels match a solid angle.
struct Equirect {
    levels: Vec<Level>,
}

impl Equirect {
    fn new(width: usize, height: usize, rgb: &[f32]) -> Equirect {
        // Negative or non-finite radiance (a broken file) is clamped: one
        // NaN would poison every texel that sees it.
        let base: Vec<f32> = rgb
            .iter()
            .map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 })
            .collect();
        let mut levels = vec![Level {
            width,
            height,
            rgb: base,
        }];
        while levels.last().map(|l| l.width > 1 || l.height > 1) == Some(true) {
            let prev = levels.last().unwrap();
            let (w, h) = (prev.width, prev.height);
            let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
            let mut next = vec![0.0f32; nw * nh * 3];
            for y in 0..nh {
                for x in 0..nw {
                    let (x0, y0) = (2 * x, 2 * y);
                    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
                    for c in 0..3 {
                        next[(y * nw + x) * 3 + c] = 0.25
                            * (prev.rgb[(y0 * w + x0) * 3 + c]
                                + prev.rgb[(y0 * w + x1) * 3 + c]
                                + prev.rgb[(y1 * w + x0) * 3 + c]
                                + prev.rgb[(y1 * w + x1) * 3 + c]);
                    }
                }
            }
            levels.push(Level {
                width: nw,
                height: nh,
                rgb: next,
            });
        }
        Equirect { levels }
    }

    fn sample(&self, d: Vec3, lod: f32) -> Vec3 {
        // The shaders' mapping (`sky.wgsl`): u = 0.5 looks down −Z and
        // grows toward +X, v = 0 is up.
        let u = 0.5 + d[0].atan2(-d[2]) / (2.0 * PI);
        let v = d[1].clamp(-1.0, 1.0).acos() / PI;
        let last = (self.levels.len() - 1) as f32;
        let lod = lod.clamp(0.0, last);
        let l0 = lod.floor() as usize;
        let f = lod - l0 as f32;
        let a = self.levels[l0].bilinear(u, v);
        if f <= 0.0 || l0 + 1 >= self.levels.len() {
            return a;
        }
        let b = self.levels[l0 + 1].bilinear(u, v);
        [
            a[0] + (b[0] - a[0]) * f,
            a[1] + (b[1] - a[1]) * f,
            a[2] + (b[2] - a[2]) * f,
        ]
    }
}

impl Level {
    fn bilinear(&self, u: f32, v: f32) -> Vec3 {
        let (w, h) = (self.width as isize, self.height as isize);
        let x = u * self.width as f32 - 0.5;
        let y = v * self.height as f32 - 0.5;
        let (xf, yf) = (x.floor(), y.floor());
        let (fx, fy) = (x - xf, y - yf);
        let x0 = (xf as isize).rem_euclid(w);
        let x1 = (x0 + 1).rem_euclid(w);
        let y0 = (yf as isize).clamp(0, h - 1);
        let y1 = (yf as isize + 1).clamp(0, h - 1);
        let at = |x: isize, y: isize| (y * w + x) as usize * 3;
        let (p00, p10, p01, p11) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
        let mut out = [0.0f32; 3];
        for (c, o) in out.iter_mut().enumerate() {
            let top = self.rgb[p00 + c] + (self.rgb[p10 + c] - self.rgb[p00 + c]) * fx;
            let bottom = self.rgb[p01 + c] + (self.rgb[p11 + c] - self.rgb[p01 + c]) * fx;
            *o = top + (bottom - top) * fy;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fynkel_cooked::payload::f16_to_f32;

    fn texel(halves: &[u16], size: usize, face: usize, x: usize, y: usize) -> Vec3 {
        let at = ((face * size + y) * size + x) * 4;
        [
            f16_to_f32(halves[at]),
            f16_to_f32(halves[at + 1]),
            f16_to_f32(halves[at + 2]),
        ]
    }

    #[test]
    fn a_uniform_environment_is_uniform_everywhere() {
        let rgb: Vec<f32> = (0..64 * 32).flat_map(|_| [1.0, 0.5, 0.25]).collect();
        let env = prefilter(64, 32, &rgb);
        for mip in 0..SPECULAR_MIPS {
            let level = env.specular_level(mip).unwrap();
            let size = (SPECULAR_SIZE >> mip) as usize;
            for face in 0..6 {
                let t = texel(level, size, face, size / 3, size / 2);
                assert!(
                    (t[0] - 1.0).abs() < 0.02 && (t[1] - 0.5).abs() < 0.01,
                    "mip {mip} face {face}: {t:?}"
                );
            }
        }
        for face in 0..6 {
            let t = texel(&env.irradiance, IRRADIANCE_SIZE as usize, face, 0, 0);
            assert!(
                (t[0] - 1.0).abs() < 0.03 && (t[2] - 0.25).abs() < 0.01,
                "face {face}: {t:?}"
            );
        }
        assert!(
            (env.mean_irradiance[0] - PI).abs() < 0.1,
            "{:?}",
            env.mean_irradiance
        );
    }

    #[test]
    fn hemispheres_land_on_the_right_faces() {
        // Sky white above the horizon, black below.
        let rgb: Vec<f32> = (0..64 * 32)
            .flat_map(|i| if i / 64 < 16 { [1.0; 3] } else { [0.0; 3] })
            .collect();
        let env = prefilter(64, 32, &rgb);
        let size = SPECULAR_SIZE as usize;
        let mirror = env.specular_level(0).unwrap();
        let up = texel(mirror, size, 2, size / 2, size / 2);
        let down = texel(mirror, size, 3, size / 2, size / 2);
        assert!(up[0] > 0.99 && down[0] < 0.01, "{up:?} {down:?}");
        // The side faces: top rows see the sky, bottom rows the ground.
        for face in [0, 1, 4, 5] {
            assert!(texel(mirror, size, face, 3, 2)[0] > 0.99, "face {face} top");
            assert!(
                texel(mirror, size, face, 3, size - 3)[0] < 0.01,
                "face {face} bottom"
            );
        }
        // Irradiance: E = π up (stored 1), 0 down, π/2 at the horizon.
        let isz = IRRADIANCE_SIZE as usize;
        let e_up = texel(&env.irradiance, isz, 2, isz / 2, isz / 2)[0];
        let e_down = texel(&env.irradiance, isz, 3, isz / 2, isz / 2)[0];
        let e_side = texel(&env.irradiance, isz, 0, isz / 2, isz / 2)[0];
        assert!((e_up - 1.0).abs() < 0.03, "{e_up}");
        assert!(e_down < 0.03, "{e_down}");
        assert!((e_side - 0.5).abs() < 0.05, "{e_side}");
        assert!(
            (env.mean_irradiance[0] - PI / 2.0).abs() < 0.15,
            "{:?}",
            env.mean_irradiance
        );
        // Rough levels blur across the horizon: a side face's centre sees
        // half sky at roughness 1, the mirror sees a hard edge.
        let rough = env.specular_level(SPECULAR_MIPS - 1).unwrap();
        let rsz = (SPECULAR_SIZE >> (SPECULAR_MIPS - 1)) as usize;
        let side = texel(rough, rsz, 0, rsz / 2, rsz / 2)[0];
        assert!(side > 0.3 && side < 0.7, "{side}");
    }

    #[test]
    fn the_equirect_axes_are_the_shaders() {
        // Red ramps with u (0 at the +Z seam, 0.5 toward −Z, 0.75 toward
        // +X), green with v (0 up, 1 down): every face and its
        // orientation can be read off the mirror level.
        let (w, h) = (64, 32);
        let rgb: Vec<f32> = (0..w * h)
            .flat_map(|i| {
                [
                    ((i % w) as f32 + 0.5) / w as f32,
                    ((i / w) as f32 + 0.5) / h as f32,
                    0.0,
                ]
            })
            .collect();
        let env = prefilter(w as u32, h as u32, &rgb);
        let size = SPECULAR_SIZE as usize;
        let mirror = env.specular_level(0).unwrap();
        let at = |face, x, y| texel(mirror, size, face, x, y);
        let near = |v: f32, want: f32| (v - want).abs() < 0.03;
        let c = size / 2;
        // Face centres: +X looks at u 0.75, −X at 0.25, −Z at 0.5; +Y at
        // v 0 (the top row), −Y at v 1.
        assert!(
            near(at(0, c, c)[0], 0.75) && near(at(0, c, c)[1], 0.5),
            "{:?}",
            at(0, c, c)
        );
        assert!(near(at(1, c, c)[0], 0.25), "{:?}", at(1, c, c));
        assert!(near(at(5, c, c)[0], 0.5), "{:?}", at(5, c, c));
        assert!(near(at(2, c, c)[1], 0.5 / h as f32), "{:?}", at(2, c, c));
        assert!(
            near(at(3, c, c)[1], 1.0 - 0.5 / h as f32),
            "{:?}",
            at(3, c, c)
        );
        // +Z's centre is the seam; its left column looks toward −X.
        assert!(
            near(at(4, 2, c)[0], 0.122) && near(at(4, size - 3, c)[0], 0.878),
            "{:?}",
            at(4, 2, c)
        );
        // Orientation: the +X face's left column looks toward +Z (u 0.87),
        // its right toward −Z (0.63); its top row is up (v 0.26).
        assert!(
            near(at(0, 2, c)[0], 0.872) && near(at(0, size - 3, c)[0], 0.628),
            "{:?}",
            at(0, 2, c)
        );
        assert!(
            near(at(0, c, 2)[1], 0.256) && near(at(0, c, size - 3)[1], 0.744),
            "{:?}",
            at(0, c, 2)
        );
        // +Y: the left column looks toward −X (0.25), the right toward +X
        // (0.75); the top row toward −Z, the bottom toward +Z, so the
        // top-left corner is u 0.375 and the bottom-left 0.125.
        assert!(
            near(at(2, 2, c)[0], 0.25) && near(at(2, size - 3, c)[0], 0.75),
            "{:?}",
            at(2, 2, c)
        );
        assert!(
            near(at(2, 2, 2)[0], 0.375) && near(at(2, 2, size - 3)[0], 0.125),
            "{:?}",
            at(2, 2, 2)
        );
    }
}
