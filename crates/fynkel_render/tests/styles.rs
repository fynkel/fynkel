//! Stage 10 phase 4: whole styles. A shader's `light` lights it in place
//! of the engine, light by light: one that returns `standard_light` looks
//! as the built-in material does; banded light gives few shades; the
//! shadow is passed to it; and a `light` that's wrong is told.

mod common;

use std::collections::HashMap;
use std::sync::Arc;

use common::*;
use fynkel_render::glam::{Mat4, Quat, Vec3};
use fynkel_render::*;

const SRGB: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const SIZE: u32 = 160;
const LEVELS: [QualityLevel; 3] = [QualityLevel::Low, QualityLevel::Medium, QualityLevel::High];

fn level_name(quality: QualityLevel) -> &'static str {
    match quality {
        QualityLevel::Low => "low",
        QualityLevel::Medium => "medium",
        QualityLevel::High => "high",
    }
}

/// How many pixels of `a` and `b` differ by more than `by` in a channel.
fn differing(a: &Image, b: &Image, by: u8) -> usize {
    a.rgba.chunks(4).zip(b.rgba.chunks(4)).filter(|(p, q)| p.iter().zip(*q).any(|(x, y)| x.abs_diff(*y) > by)).count()
}

fn material(argb: u32, roughness: f32) -> PbrMaterial {
    PbrMaterial { base_color: linear_rgb(argb), metallic: 0.0, roughness, ..PbrMaterial::DEFAULT }
}

fn shader(r: &mut Renderer, key: ShaderKey, name: &str, source: &str) -> Arc<CustomShader> {
    let shader = Arc::new(CustomShader::new(name, source).unwrap_or_else(|e| panic!("{e}")));
    r.upload_shader(key, shader.clone());
    shader
}

fn used(shader: ShaderKey) -> Option<ShaderUse> {
    Some(ShaderUse { shader, values: 0, textures: [None; MAX_TEXTURES], reach: 0.0 })
}

/// A floor, a sphere and a cube under a sun that casts shadows and a
/// lamp, a sky behind.
fn stage(quality: QualityLevel, shader: Option<ShaderKey>) -> RenderScene {
    let camera = CameraView::looking_at(Vec3::new(0.6, 2.2, 4.4), Vec3::new(0.0, 0.5, 0.0));
    let with = |m: PbrMaterial| PbrMaterial { shader: shader.and_then(used), ..m };
    RenderScene {
        camera: Some(camera),
        lights: vec![
            Light::sun(Vec3::new(1.0, -1.2, -0.4), Vec3::ONE).with_shadows(true),
            Light::point(Vec3::new(-1.4, 1.0, 1.0), 4.0, Vec3::new(3.0, 1.5, 0.5)),
        ],
        quality: Some(quality.quality()),
        ambient: Vec3::splat(0.2),
        sky: Some(Sky {
            kind: SkyKind::Gradient {
                zenith: Vec3::new(0.2, 0.4, 0.9),
                horizon: Vec3::new(0.8, 0.85, 0.9),
                ground: Vec3::splat(0.3),
            },
            lighting: 0.5,
        }),
        draws: vec![
            Draw::new(PLANE, Mat4::from_scale(Vec3::splat(12.0)), with(material(0xFFB0B0B0, 0.8))),
            Draw::new(SPHERE, Mat4::from_translation(Vec3::new(-0.7, 0.5, -0.4)), with(material(0xFFD06040, 0.4))),
            Draw::new(
                CUBE,
                Mat4::from_rotation_translation(Quat::from_rotation_y(0.6), Vec3::new(0.8, 0.5, -0.2)),
                with(material(0xFF4070C0, 0.5)),
            ),
        ],
        ..RenderScene::default()
    }
}

const SURFACE: &str = "fn surface(in: SurfaceInput) -> Surface {\n    return standard(in);\n}\n";

/// Lights it as the engine does.
const STANDARD: &str = "fn light(in: LightInput) -> vec3<f32> {\n    return standard_light(in);\n}\n";

/// Three bands of light, with no highlight: toon.
const BANDS: &str = "fn light(in: LightInput) -> vec3<f32> {
    let lit = max(dot(in.normal, in.direction), 0.0) * in.shadow;
    let band = select(select(0.2, 0.6, lit > 0.15), 1.0, lit > 0.6);
    return in.surface.color * in.color * band;
}
";

/// Only the shadow, white: lit is white, shadowed black.
const SHADOW_ONLY: &str = "fn light(in: LightInput) -> vec3<f32> {\n    return vec3<f32>(in.shadow) * 0.5;\n}\n";

fn source(light: &str) -> String {
    format!("{SURFACE}\n{light}")
}

#[test]
fn a_light_that_is_wrong_is_told() {
    let lights = |source: &str| CustomShader::new("s.wgsl", source).unwrap_or_else(|e| panic!("{e}")).lights;
    assert!(lights(&source(STANDARD)));
    assert!(!lights(SURFACE));
    for (wrong, says) in [
        ("fn light(in: LightInput) -> vec4<f32> {\n    return vec4<f32>(1.0);\n}\n", "fn light(in: LightInput)"),
        ("fn light(d: vec3<f32>) -> vec3<f32> {\n    return d;\n}\n", "fn light(in: LightInput)"),
    ] {
        let error = CustomShader::new("s.wgsl", &source(wrong)).expect_err("a wrong light");
        assert!(error.message.contains(says), "{error}");
    }
    let error = CustomShader::new(
        "u.wgsl",
        &format!("fn unlit(in: SurfaceInput) -> vec4<f32> {{\n    return vec4<f32>(1.0);\n}}\n{STANDARD}"),
    )
    .expect_err("an unlit shader isn't lit");
    assert!(error.message.contains("unlit"), "{error}");
}

/// A `light` returning `standard_light` looks as the built-in material,
/// sun, shadow and lamp, at every level.
#[test]
fn standard_light_looks_as_the_engine_does() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    shader(&mut r, ShaderKey(1), "standard.wgsl", &source(STANDARD));
    for quality in LEVELS {
        let (expected, _) = render(&gpu, &mut r, &stage(quality, None), SIZE, SIZE);
        let (image, _) = render(&gpu, &mut r, &stage(quality, Some(ShaderKey(1))), SIZE, SIZE);
        let off = differing(&image, &expected, 2);
        assert!(off <= 10, "{off} pixels differ at {}", level_name(quality));
    }
}

/// Banded light gives the sphere few shades, where the engine's gives many;
/// and the shadow reaches `light`: under the sphere, it's dark.
#[test]
fn banded_light_gives_few_shades_and_sees_the_shadow() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    shader(&mut r, ShaderKey(1), "bands.wgsl", &source(BANDS));
    shader(&mut r, ShaderKey(2), "shadow.wgsl", &source(SHADOW_ONLY));
    // Only the sun, no sky light: the bands are all there is.
    let alone = |shader| {
        let mut scene = stage(QualityLevel::Medium, shader);
        scene.lights.truncate(1);
        scene.sky = None;
        scene.ambient = Vec3::ZERO;
        scene
    };
    // The shades in the sphere's middle, and how many cover more than 2%
    // of it: the edges between bands, smoothed, don't count.
    let shades = |image: &Image| {
        let mut seen = HashMap::<[u8; 3], usize>::new();
        let (xs, ys) = (52..70, 64..82);
        for y in ys.clone() {
            for x in xs.clone() {
                let at = ((y * SIZE + x) * 4) as usize;
                *seen.entry([image.rgba[at] / 4, image.rgba[at + 1] / 4, image.rgba[at + 2] / 4]).or_default() += 1;
            }
        }
        let count = xs.len() * ys.len();
        (seen.len(), seen.values().filter(|&&n| n * 50 > count).count())
    };
    let (smooth, _) = render(&gpu, &mut r, &alone(None), SIZE, SIZE);
    let (banded, _) = render(&gpu, &mut r, &alone(Some(ShaderKey(1))), SIZE, SIZE);
    let (smooth, banded) = (shades(&smooth), shades(&banded));
    assert!(smooth.0 > 40, "{smooth:?} shades lit by the engine");
    assert!(banded.0 <= 8 && banded.1 <= 3, "{banded:?} shades in bands, {smooth:?} smooth");

    let (shadowed, _) = render(&gpu, &mut r, &alone(Some(ShaderKey(2))), SIZE, SIZE);
    let grey = |x: u32, y: u32| shadowed.rgba[((y * SIZE + x) * 4) as usize];
    // Below the horizon, where there's no sky to be black.
    let dark = (90..SIZE).flat_map(|y| (0..SIZE).map(move |x| (x, y))).map(|(x, y)| grey(x, y)).min().unwrap();
    let lit = grey(150, 150);
    assert!(lit > 150, "lit floor {lit}");
    assert!(dark < 20, "the darkest shadow {dark}");
}
