//! Stage 10 phase 3: shaders that read what's behind them. A see-through
//! surface whose shader returns `scene_color` as it is can't be seen;
//! `depth_behind` says how far behind it the scene is, in metres; what
//! else is see-through is drawn as it would be without it; a frame with
//! no such shader keeps no images for it, and a view lets them go a
//! second after its last; and a pond, at every level.

mod common;

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

/// A floor, a sphere and a cube under a sun that casts shadows, a sky
/// behind, with ambient occlusion and bloom where the level has them.
fn stage(quality: QualityLevel) -> RenderScene {
    let mut camera = CameraView::looking_at(Vec3::new(0.6, 2.2, 4.4), Vec3::new(0.0, 0.5, 0.0));
    camera.ambient_occlusion = Some(AmbientOcclusion::DEFAULT);
    camera.bloom = Some(Bloom::DEFAULT);
    RenderScene {
        camera: Some(camera),
        lights: vec![Light::sun(Vec3::new(1.0, -1.2, -0.4), Vec3::ONE).with_shadows(true)],
        quality: Some(quality.quality()),
        ambient: Vec3::splat(0.25),
        sky: Some(Sky {
            kind: SkyKind::Gradient {
                zenith: Vec3::new(0.2, 0.4, 0.9),
                horizon: Vec3::new(0.8, 0.85, 0.9),
                ground: Vec3::splat(0.3),
            },
            lighting: 0.5,
        }),
        draws: vec![
            Draw::new(PLANE, Mat4::from_scale(Vec3::splat(12.0)), material(0xFFB0B0B0, 0.8)),
            Draw::new(SPHERE, Mat4::from_translation(Vec3::new(-0.7, 0.5, -0.4)), material(0xFFD06040, 0.4)),
            Draw::new(
                CUBE,
                Mat4::from_rotation_translation(Quat::from_rotation_y(0.6), Vec3::new(0.8, 0.5, -0.2)),
                material(0xFF4070C0, 0.5),
            ),
        ],
        ..RenderScene::default()
    }
}

/// Shows what's behind it as it is.
const CLEAR: &str = "fn unlit(in: SurfaceInput) -> vec4<f32> {
    return vec4<f32>(scene_color(in.screen), 1.0);
}
";

/// A pane facing the camera, `size` metres across, at `at`.
fn pane(at: Vec3, size: f32, material: PbrMaterial) -> Draw {
    let model = Mat4::from_scale_rotation_translation(
        Vec3::splat(size),
        Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
        at,
    );
    Draw { cast_shadows: false, ..Draw::new(PLANE, model, PbrMaterial { double_sided: true, ..material }) }
}

#[test]
fn a_shader_that_reads_whats_behind_is_told_apart() {
    let reads = |source: &str| CustomShader::new("s.wgsl", source).unwrap_or_else(|e| panic!("{e}")).reads_behind;
    assert!(reads(CLEAR));
    assert!(reads(
        "fn surface(in: SurfaceInput) -> Surface {\n    var s = standard(in);\n    \
         s.alpha = saturate(depth_behind(in));\n    return s;\n}\n"
    ));
    assert!(!reads("fn surface(in: SurfaceInput) -> Surface {\n    return standard(in);\n}\n"));
    // A vertex has no scene to read: it's drawn before there is one.
    let error = CustomShader::new(
        "v.wgsl",
        "fn vertex(in: VertexInput) -> VertexInput {\n    var v = in;\n    \
         v.position.y += textureLoad(fy_scene_depth, vec2<i32>(0), 0);\n    return v;\n}\n\
         fn surface(in: SurfaceInput) -> Surface {\n    return standard(in);\n}\n",
    )
    .expect_err("a vertex can't read the scene");
    assert!(error.message.contains("fy_scene_depth"), "{error}");
}

/// A pane over most of the view that shows what's behind it as it is
/// can't be seen, at every level and with 1, 2 or 4 samples a pixel: what's
/// behind is the scene, lit, with its sky, shadows, occlusion and glow.
/// Opaque or see-through, it's drawn over the scene.
#[test]
fn a_surface_showing_whats_behind_it_as_it_is_cant_be_seen() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    shader(&mut r, ShaderKey(1), "clear.wgsl", CLEAR);
    for (quality, samples) in LEVELS.into_iter().flat_map(|q| [4, 1, 2].map(|n| (q, n))) {
        let scene = RenderScene { quality: Some(Quality { samples, ..quality.quality() }), ..stage(quality) };
        let (expected, stats) = render(&gpu, &mut r, &scene, SIZE, SIZE);
        assert!(!stats.behind);
        for blend in [Blend::Opaque, Blend::Transparent] {
            let mut covered = scene.clone();
            let glass = PbrMaterial { shader: used(ShaderKey(1)), blend, ..material(0xFFFFFFFF, 0.5) };
            covered.draws.push(pane(Vec3::new(0.0, 0.7, 1.6), 2.4, glass));
            let (image, stats) = render(&gpu, &mut r, &covered, SIZE, SIZE);
            assert!(stats.behind, "{blend:?}");
            let off = differing(&image, &expected, 2);
            assert!(off <= 20, "{off} pixels differ at {}, {samples} samples ({blend:?})", level_name(quality));
        }
    }
}

/// Shows how far behind it the scene is: a quarter grey a metre.
const DEPTH: &str = "fn unlit(in: SurfaceInput) -> vec4<f32> {
    return vec4<f32>(vec3<f32>(depth_behind(in) * 0.25), 1.0);
}
";

/// A pane a metre in front of a wall shows a metre; past the wall's edge,
/// where there's only sky, the far end.
#[test]
fn depth_behind_says_how_far_behind_the_scene_is() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    shader(&mut r, ShaderKey(1), "depth.wgsl", DEPTH);
    let expected = to_srgb8(tonemap(Vec3::splat(0.25)));
    for quality in LEVELS {
        let mut camera = CameraView::looking_at(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO);
        camera.far = 50.0;
        let scene = RenderScene {
            camera: Some(camera),
            quality: Some(quality.quality()),
            ambient: Vec3::splat(0.3),
            draws: vec![
                // Its front at z = -1, across the left half of the view.
                Draw::new(
                    CUBE,
                    Mat4::from_scale_rotation_translation(
                        Vec3::new(10.0, 10.0, 1.0),
                        Quat::IDENTITY,
                        Vec3::new(-5.0, 0.0, -1.5),
                    ),
                    material(0xFF808080, 0.8),
                ),
                pane(Vec3::ZERO, 3.0, PbrMaterial { shader: used(ShaderKey(1)), ..material(0xFFFFFFFF, 0.5) }),
            ],
            ..RenderScene::default()
        };
        let (image, _) = render(&gpu, &mut r, &scene, SIZE, SIZE);
        let at = |x: u32, y: u32| {
            let i = ((y * SIZE + x) * 4) as usize;
            [image.rgba[i], image.rgba[i + 1], image.rgba[i + 2]]
        };
        let near = at(SIZE / 2 - 20, SIZE / 2);
        for (got, want) in near.iter().zip(expected) {
            assert!(got.abs_diff(want) <= 2, "{near:?}, not {expected:?}, at {}", level_name(quality));
        }
        // Only sky: 45 m behind it, far past white.
        let far = at(SIZE / 2 + 20, SIZE / 2);
        assert!(far.iter().all(|c| *c >= 250), "{far:?} at {}", level_name(quality));
    }
}

/// With a surface that reads what's behind it in view, the rest is drawn
/// as it is without: a cut-out in front of it, a see-through sphere over
/// it, and the debug lines on top.
#[test]
fn the_rest_is_drawn_as_without_it() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    shader(&mut r, ShaderKey(1), "clear.wgsl", CLEAR);
    for quality in LEVELS {
        let mut scene = stage(quality);
        let cut = PbrMaterial { alpha_cutoff: 0.5, ..material(0xFF30A050, 0.6) };
        scene.draws.push(Draw::new(
            CUBE,
            Mat4::from_scale_rotation_translation(Vec3::new(0.3, 1.2, 0.3), Quat::IDENTITY, Vec3::new(0.3, 0.6, 2.2)),
            cut,
        ));
        let glass =
            PbrMaterial { blend: Blend::Transparent, base_color: [0.9, 0.95, 1.0, 0.4], ..material(0xFFFFFFFF, 0.1) };
        scene.draws.push(Draw::new(SPHERE, Mat4::from_translation(Vec3::new(-0.2, 0.7, 2.4)), glass));
        scene.lines.push(Line {
            a: Vec3::new(-1.0, 0.1, 1.0),
            b: Vec3::new(1.0, 1.5, 1.0),
            color: [1.0, 1.0, 0.0, 1.0],
        });
        let (expected, _) = render(&gpu, &mut r, &scene, SIZE, SIZE);

        let mut with = scene.clone();
        let clear = PbrMaterial { shader: used(ShaderKey(1)), blend: Blend::Transparent, ..material(0xFFFFFFFF, 0.5) };
        with.draws.push(pane(Vec3::new(0.0, 0.7, 1.4), 2.4, clear));
        let (image, stats) = render(&gpu, &mut r, &with, SIZE, SIZE);
        assert!(stats.behind);
        let off = differing(&image, &expected, 2);
        assert!(off <= 20, "{off} pixels differ at {}", level_name(quality));
    }
}

/// A frame without such a shader in view keeps nothing for it; with one,
/// the depth and colour; after its last, for a second, then nothing.
#[test]
fn whats_behind_is_kept_only_while_its_read() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    shader(&mut r, ShaderKey(1), "clear.wgsl", CLEAR);
    let mut plain = stage(QualityLevel::Medium);
    let glass = PbrMaterial { blend: Blend::Transparent, base_color: [1.0, 1.0, 1.0, 0.5], ..material(0, 0.2) };
    plain.draws.push(pane(Vec3::new(0.0, 0.7, 1.6), 1.0, glass));
    let (_, stats) = render(&gpu, &mut r, &plain, 64, 64);
    assert_eq!((stats.behind, stats.behind_bytes), (false, 0), "an ordinary see-through surface");

    let mut reading = plain.clone();
    let clear = PbrMaterial { shader: used(ShaderKey(1)), ..material(0xFFFFFFFF, 0.5) };
    reading.draws.push(pane(Vec3::new(0.0, 0.7, 1.6), 1.0, clear));
    // Out of view, it's not drawn, so nothing reads.
    let mut away = reading.clone();
    away.draws.last_mut().expect("the pane").model = Mat4::from_translation(Vec3::new(0.0, 0.0, 40.0));
    let (_, stats) = render(&gpu, &mut r, &away, 64, 64);
    assert_eq!((stats.behind, stats.behind_bytes), (false, 0), "out of view");

    let (_, stats) = render(&gpu, &mut r, &reading, 64, 64);
    assert!(stats.behind);
    // 4 depth samples, one more, and a half-float colour, a pixel.
    assert_eq!(stats.behind_bytes, 64 * 64 * (16 + 4 + 8));
    let mut frames = 0;
    loop {
        let (_, stats) = render(&gpu, &mut r, &plain, 64, 64);
        assert!(!stats.behind);
        if stats.behind_bytes == 0 {
            break;
        }
        frames += 1;
        assert!(frames < 100, "never let go");
    }
    assert_eq!(frames, 60, "kept about a second");
}

/// Water over stones: foam where it meets them, the stones bent and
/// darkened the deeper they are, a sun's highlight.
const POND: &str = "struct Values { deep: vec3<f32> }

fn surface(in: SurfaceInput) -> Surface {
    var s = standard(in);
    let depth = depth_behind(in);
    let ripple = vec2<f32>(noise(in.position.xz * 7.0), noise(in.position.zx * 7.0 + 5.0)) - 0.5;
    var screen = in.screen + ripple * 0.04 * saturate(depth * 3.0);
    // Bent onto something in front of the water: straight through.
    if scene_depth(screen) < view_depth(in.position) {
        screen = in.screen;
    }
    let murk = saturate(depth / 0.5);
    let foam = 1.0 - smoothstep(0.0, 0.05, depth);
    s.color = mix(values.deep * murk, vec3<f32>(0.9), foam);
    s.emission = scene_color(screen) * vec3<f32>(0.75, 0.9, 0.95) * (1.0 - murk * 0.8) * (1.0 - foam);
    s.roughness = mix(0.06, 0.7, foam);
    s.alpha = 1.0;
    return s;
}
";

/// The pond's golden at every level: stones in the water, and a bank
/// that slopes up out of it in front, foaming where it comes out.
#[test]
fn a_pond_at_every_level() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let pond = shader(&mut r, ShaderKey(1), "pond.wgsl", POND);
    for quality in LEVELS {
        let mut scene = stage(quality);
        let camera = scene.camera.expect("a camera");
        scene.camera =
            Some(CameraView { ..CameraView::looking_at(Vec3::new(0.3, 2.1, 2.9), Vec3::new(0.0, 0.1, -0.3)) });
        let view = scene.camera.as_mut().expect("set");
        (view.bloom, view.ambient_occlusion) = (camera.bloom, camera.ambient_occlusion);
        scene.draws.truncate(1);
        let stones = [(-0.8, -0.5, 0.6, 0xFFC0A080), (0.5, -0.1, 0.45, 0xFF708090), (0.0, -1.1, 0.8, 0xFFA0B070)];
        for (x, z, size, color) in stones {
            scene.draws.push(Draw::new(
                SPHERE,
                Mat4::from_scale_rotation_translation(Vec3::splat(size), Quat::IDENTITY, Vec3::new(x, 0.12, z)),
                material(color, 0.7),
            ));
        }
        // Its top rises from under the water to above it, towards the
        // camera.
        scene.draws.push(Draw::new(
            CUBE,
            Mat4::from_scale_rotation_translation(
                Vec3::new(4.0, 0.2, 1.2),
                Quat::from_rotation_x(-0.3),
                Vec3::new(0.0, 0.2, 0.5),
            ),
            material(0xFFB89E78, 0.9),
        ));
        let mut words = [0f32; 12];
        let deep = pond.values.iter().find(|v| v.name == "deep").expect("deep");
        words[deep.offset as usize / 4..][..3].copy_from_slice(&[0.02, 0.12, 0.18]);
        let row = |k: usize| [words[k], words[k + 1], words[k + 2], words[k + 3]];
        scene.shader_values = vec![[row(0), row(4), row(8)]];
        let water = PbrMaterial { shader: used(ShaderKey(1)), ..material(0xFF336680, 0.1) };
        let surface =
            Mat4::from_scale_rotation_translation(Vec3::new(4.0, 1.0, 3.0), Quat::IDENTITY, Vec3::new(0.0, 0.3, -0.4));
        scene.draws.push(Draw { cast_shadows: false, ..Draw::new(PLANE, surface, water) });
        let (image, stats) = render(&gpu, &mut r, &scene, SIZE, SIZE);
        assert!(stats.behind);
        assert_golden(&format!("pond_{}", level_name(quality)), &image);
    }
}
