//! Stage 10 phase 4: whole styles. A shader's `light` lights it in place
//! of the engine, light by light: one that returns `standard_light` looks
//! as the built-in material does; banded light gives few shades; the
//! shadow is passed to it; and a `light` that's wrong is told. Outlines
//! ring an object in their colour, as wide as they say, follow a mesh a
//! shader moves or a skeleton bends, and aren't drawn for see-through
//! surfaces; and a toon scene at every level. The pixel look draws the
//! scene small and enlarges it with hard edges, and holds still as the
//! camera moves less than a pixel, or moves by whole ones. Screen effects
//! see the picture as it was and where, run in order, work at every level
//! and with the pixel look, and an effect that's wrong is told.

mod common;

use std::collections::HashMap;
use std::sync::Arc;

use common::*;
use fynkel_render::MeshParts;
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

const BAR: MeshKey = MeshKey(10);
const PLAIN_BAR: MeshKey = MeshKey(11);

/// Two boxes, one above the other, each moved by a joint of its own if
/// `skinned`.
fn bar(skinned: bool) -> MeshData {
    let cube = MeshData::cube();
    let (mut positions, mut normals, mut tangents, mut uvs) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (mut indices, mut joints, mut weights) = (Vec::new(), Vec::new(), Vec::new());
    for (joint, lift) in [(0u16, -0.5), (1, 0.5)] {
        let first = positions.len() as u32;
        for v in cube.vertices() {
            positions.push([v.position[0] * 0.4, v.position[1] + lift, v.position[2] * 0.4]);
            normals.push(v.normal);
            tangents.push(v.tangent);
            uvs.push(v.uv);
            joints.push([joint, 0, 0, 0]);
            weights.push([1.0, 0.0, 0.0, 0.0]);
        }
        indices.extend(cube.indices().iter().map(|i| i + first));
    }
    MeshData::new(MeshParts {
        positions: &positions,
        normals: Some(&normals),
        tangents: Some(&tangents),
        uvs: Some(&uvs),
        indices: &indices,
        skin: skinned.then_some((&joints[..], &weights[..])),
    })
    .expect("a bar")
}

const GREEN: Outline = Outline { width: 0.05, color: [0.0, 1.0, 0.0] };

/// The pixels the outline's green is in.
fn green(image: &Image) -> usize {
    image.rgba.chunks(4).filter(|p| p[1] > 200 && p[0] < 60 && p[2] < 60).count()
}

/// A sphere on the floor, outlined with `outline`.
fn outlined(quality: QualityLevel, outline: Option<Outline>, blend: Blend) -> RenderScene {
    let mut scene = stage(quality, None);
    scene.draws.truncate(1);
    let m = PbrMaterial { outline, blend, ..material(0xFFD06040, 0.4) };
    scene.draws.push(Draw::new(SPHERE, Mat4::from_translation(Vec3::new(0.0, 0.6, 0.0)), m));
    scene
}

/// An outline rings the sphere in its green at every level, twice as wide
/// takes twice the pixels or more (a thin one's edges are softened), and none is drawn without one or for a
/// see-through sphere.
#[test]
fn an_outline_rings_an_object_in_its_colour() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    for quality in LEVELS {
        let at = level_name(quality);
        let (plain, _) = render(&gpu, &mut r, &outlined(quality, None, Blend::Opaque), SIZE, SIZE);
        assert_eq!(green(&plain), 0, "{at}");
        let (thin, stats) = render(&gpu, &mut r, &outlined(quality, Some(GREEN), Blend::Opaque), SIZE, SIZE);
        // The ring around a sphere about 50 pixels across.
        let thin = green(&thin);
        assert!(thin > 60, "{thin} green pixels at {at}");
        assert_eq!(stats.draw_calls, 2);
        let wide = Outline { width: 0.1, ..GREEN };
        let (wide, _) = render(&gpu, &mut r, &outlined(quality, Some(wide), Blend::Opaque), SIZE, SIZE);
        let wide = green(&wide);
        assert!(wide as f32 > thin as f32 * 1.6 && (wide as f32) < thin as f32 * 3.2, "{thin} then {wide} at {at}");
        let (glass, _) = render(&gpu, &mut r, &outlined(quality, Some(GREEN), Blend::Transparent), SIZE, SIZE);
        assert_eq!(green(&glass), 0, "{at}");
    }
}

/// Moves every vertex by `values.by`, in the world.
const SHIFT: &str = "struct Values { by: vec3<f32> }

fn vertex(in: VertexInput) -> VertexInput {
    var v = in;
    v.position += values.by;
    return v;
}

fn surface(in: SurfaceInput) -> Surface {
    return standard(in);
}
";

/// `values` laid out as `shader`'s `Values`, in the scene's blocks.
fn values(shader: &CustomShader, name: &str, value: &[f32]) -> Vec<[[f32; 4]; 3]> {
    let mut words = vec![0f32; (shader.values_size as usize).div_ceil(48) * 12];
    let v = shader.values.iter().find(|v| v.name == name).unwrap_or_else(|| panic!("no value {name}"));
    let at = v.offset as usize / 4;
    words[at..at + value.len()].copy_from_slice(value);
    words.chunks(12).map(|c| [[c[0], c[1], c[2], c[3]], [c[4], c[5], c[6], c[7]], [c[8], c[9], c[10], c[11]]]).collect()
}

/// A sphere its shader moves is outlined where it's moved to, as one
/// placed there; a bar its skeleton bends, as one placed as its joints
/// put it.
#[test]
fn an_outline_follows_a_mesh_its_shader_moves_or_its_skeleton_bends() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    r.upload_mesh(BAR, &bar(true));
    r.upload_mesh(PLAIN_BAR, &bar(false));
    let shift = shader(&mut r, ShaderKey(1), "shift.wgsl", SHIFT);
    shader(&mut r, ShaderKey(2), "same.wgsl", SURFACE);
    let by = Vec3::new(-0.8, 0.3, 0.2);
    let sphere = |shader: ShaderKey, at: Vec3| {
        let m = PbrMaterial { outline: Some(GREEN), shader: used(shader), ..material(0xFFD06040, 0.4) };
        Draw::new(SPHERE, Mat4::from_translation(at), m)
    };
    let joint = Mat4::from_rotation_translation(Quat::from_rotation_z(0.5), Vec3::new(0.9, 1.0, 0.0));
    let bar = |mesh: MeshKey, model: Mat4| {
        let m = PbrMaterial { outline: Some(GREEN), ..material(0xFF4070C0, 0.5) };
        Draw { skin: (mesh == BAR).then_some(0), ..Draw::new(mesh, model, m) }
    };
    for quality in LEVELS {
        let mut placed = stage(quality, None);
        placed.draws.truncate(1);
        placed.draws.push(sphere(ShaderKey(2), Vec3::new(0.0, 0.6, 0.0) + by));
        placed.draws.push(bar(PLAIN_BAR, joint));
        let (expected, _) = render(&gpu, &mut r, &placed, SIZE, SIZE);
        assert!(green(&expected) > 100, "{} green pixels", green(&expected));

        let mut moved = stage(quality, None);
        moved.draws.truncate(1);
        moved.shader_values = values(&shift, "by", &by.to_array());
        let mut draw = sphere(ShaderKey(1), Vec3::new(0.0, 0.6, 0.0));
        draw.material.shader.as_mut().unwrap().reach = 1.0;
        moved.draws.push(draw);
        moved.joints = vec![joint; 2];
        moved.draws.push(bar(BAR, Mat4::IDENTITY));
        let (image, _) = render(&gpu, &mut r, &moved, SIZE, SIZE);
        let off = differing(&image, &expected, 2);
        assert!(off <= 4, "{off} pixels differ at {}", level_name(quality));
    }
}

/// Toon: banded light and dark outlines, a sun and a lamp, at every level.
#[test]
fn picture_toon() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    shader(&mut r, ShaderKey(1), "bands.wgsl", &source(BANDS));
    let ink = Outline { width: 0.03, color: [0.02, 0.02, 0.03] };
    for quality in LEVELS {
        let mut scene = stage(quality, Some(ShaderKey(1)));
        // Little light but the sun's and the lamp's, so the bands show.
        scene.ambient = Vec3::splat(0.08);
        if let Some(sky) = &mut scene.sky {
            sky.lighting = 0.1;
        }
        for draw in &mut scene.draws[1..] {
            draw.material.outline = Some(ink);
        }
        let (image, _) = render(&gpu, &mut r, &scene, SIZE, SIZE);
        assert_golden(&format!("toon_{}", level_name(quality)), &image);
    }
}

/// The pixel look's view: 40 pixels high in a 160-pixel view, each
/// pixel 4 × 4 of the view's.
fn pixelated(quality: QualityLevel, camera: CameraView) -> RenderScene {
    let mut scene = stage(quality, None);
    scene.camera = Some(CameraView { pixels: Some(40), ..camera });
    scene
}

/// Whether every 4 × 4 block of `image` is one colour.
fn blocky(image: &Image) -> bool {
    (0..SIZE).all(|y| {
        (0..SIZE).all(|x| {
            let at = |x: u32, y: u32| &image.rgba[((y * SIZE + x) * 4) as usize..][..4];
            at(x, y) == at(x & !3, y & !3)
        })
    })
}

/// At every level, every pixel of the small picture covers 4 × 4 of the
/// view's, and the picture is the scene drawn 40 pixels high: as the
/// scene drawn at 40 × 40, near enough.
#[test]
fn the_pixel_look_draws_the_scene_small_and_enlarges_it_with_hard_edges() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let camera = CameraView::looking_at(Vec3::new(0.6, 2.2, 4.4), Vec3::new(0.0, 0.5, 0.0));
    for quality in LEVELS {
        let (image, _) = render(&gpu, &mut r, &pixelated(quality, camera), SIZE, SIZE);
        assert!(blocky(&image), "{}", level_name(quality));
        let mut small = stage(quality, None);
        small.camera = Some(camera);
        let (drawn, _) = render(&gpu, &mut r, &small, 40, 40);
        let mut off = 0;
        for y in 0..40 {
            for x in 0..40 {
                let a = &drawn.rgba[((y * 40 + x) * 4) as usize..][..3];
                let b = &image.rgba[((y * 4 * SIZE + x * 4) * 4) as usize..][..3];
                off += usize::from(a.iter().zip(b).any(|(p, q)| p.abs_diff(*q) > 12));
            }
        }
        // The camera moved by under half a pixel, at the edges of things.
        assert!(off < 160, "{off} of 1600 pixels differ at {}", level_name(quality));
    }
}

/// An orthographic camera moved less than half a pixel of the picture
/// draws the same frame; moved a whole pixel, the same frame moved over
/// by one (4 of the view's). A perspective one moved a fifth of a pixel
/// draws the same frame.
#[test]
fn the_pixel_look_holds_still_as_the_camera_moves_slowly() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let eye = Vec3::new(0.6, 4.0, 4.4);
    let ortho = |shift: f32| {
        let mut camera = CameraView::looking_at(eye, Vec3::new(0.0, 0.5, 0.0));
        camera.orthographic = Some(2.0);
        let right = camera.view.inverse().x_axis.truncate();
        // A texel is 4 m / 40.
        let at = eye + right * (0.1 * shift + 0.013);
        CameraView { view: camera.view * Mat4::from_translation(eye - at), position: at, ..camera }
    };
    for quality in LEVELS {
        let at = level_name(quality);
        let (still, _) = render(&gpu, &mut r, &pixelated(quality, ortho(0.0)), SIZE, SIZE);
        for shift in [0.1, 0.2, 0.3, -0.2] {
            let (moved, _) = render(&gpu, &mut r, &pixelated(quality, ortho(shift)), SIZE, SIZE);
            assert_eq!(differing(&still, &moved, 0), 0, "moved {shift} of a pixel at {at}");
        }
        // A whole pixel to the right: everything 4 view pixels to the left.
        let (moved, _) = render(&gpu, &mut r, &pixelated(quality, ortho(1.0)), SIZE, SIZE);
        let mut off = 0;
        for y in 0..SIZE {
            for x in 0..SIZE - 4 {
                let a = &still.rgba[((y * SIZE + x + 4) * 4) as usize..][..3];
                let b = &moved.rgba[((y * SIZE + x) * 4) as usize..][..3];
                off += usize::from(a.iter().zip(b).any(|(p, q)| p.abs_diff(*q) > 2));
            }
        }
        // Shadows and lighting follow the world, not the picture: a few
        // pixels where a shadow's edge falls differently.
        assert!(off <= 64, "{off} pixels differ moved a pixel at {at}");

        // Perspective: where it looks at the ground.
        let looking = |shift: f32| {
            let camera = CameraView::looking_at(eye, Vec3::new(0.0, 0.0, 0.0));
            let right = camera.view.inverse().x_axis.truncate();
            let texel = 2.0 * (camera.fov_y * 0.5).tan() * eye.length() / 40.0;
            let at = eye + right * texel * shift;
            CameraView { view: camera.view * Mat4::from_translation(eye - at), position: at, ..camera }
        };
        let (still, _) = render(&gpu, &mut r, &pixelated(quality, looking(0.0)), SIZE, SIZE);
        let (moved, _) = render(&gpu, &mut r, &pixelated(quality, looking(0.2)), SIZE, SIZE);
        assert_eq!(differing(&still, &moved, 0), 0, "perspective at {at}");
    }
}

/// The pixel look, 48 pixels high, at every level: the stage with its
/// lamp, sky and shadows, enlarged with hard edges.
#[test]
fn picture_pixels() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let mut camera = CameraView::looking_at(Vec3::new(0.6, 2.2, 4.4), Vec3::new(0.0, 0.5, 0.0));
    camera.pixels = Some(48);
    for quality in LEVELS {
        let mut scene = stage(quality, None);
        scene.camera = Some(camera);
        let (image, _) = render(&gpu, &mut r, &scene, SIZE, SIZE);
        assert_golden(&format!("pixels_{}", level_name(quality)), &image);
    }
}

// --- Screen effects ---------------------------------------------------------

const KEEP: &str = "fn effect(in: EffectInput) -> vec4<f32> {\n    return picture(in.uv);\n}\n";
const INVERT: &str =
    "fn effect(in: EffectInput) -> vec4<f32> {\n    return vec4<f32>(1.0 - picture(in.uv).rgb, 1.0);\n}\n";
const GAIN: &str = "struct Values { gain: vec3<f32> }

fn effect(in: EffectInput) -> vec4<f32> {
    return vec4<f32>(picture_at(vec2<i32>(in.pixel)).rgb * values.gain, 1.0);
}
";
/// Where each pixel is, as its colour.
const WHERE: &str = "fn effect(in: EffectInput) -> vec4<f32> {
    return vec4<f32>(in.uv, in.pixel.x / in.size.x, 1.0);
}
";

const KEEP_KEY: ShaderKey = ShaderKey(20);
const INVERT_KEY: ShaderKey = ShaderKey(21);
const GAIN_KEY: ShaderKey = ShaderKey(22);
const WHERE_KEY: ShaderKey = ShaderKey(23);

/// The stage with `effects` (their values from the scene's first block).
fn with_effects(quality: QualityLevel, effects: &[ShaderKey]) -> RenderScene {
    let mut scene = stage(quality, None);
    scene.effects = effects.iter().map(|&shader| ScreenEffect { shader, values: 0 }).collect();
    scene
}

/// `image`'s pixels in linear light, each changed by `f`, as an sRGB view
/// stores them.
fn changed(image: &Image, f: impl Fn([f32; 3]) -> [f32; 3]) -> Image {
    let encode = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        (s * 255.0).round() as u8
    };
    let mut rgba = image.rgba.clone();
    for p in rgba.chunks_mut(4) {
        let c = f([0, 1, 2].map(|i| linear(p[i] as f32 / 255.0)));
        p[..3].copy_from_slice(&c.map(encode));
    }
    Image { rgba, ..image.clone() }
}

#[test]
fn a_screen_effect_that_is_wrong_is_told() {
    let wrong = |source: &str| CustomShader::new("fx.wgsl", source).err().map(|e| e.to_string());
    let told = |source: &str, says: &str| {
        let error = wrong(source).unwrap_or_else(|| panic!("compiled:\n{source}"));
        assert!(error.contains(says), "{error:?} doesn't say {says:?}");
    };
    told(
        "fn effect(uv: vec2<f32>) -> vec4<f32> {\n    return vec4<f32>(uv, 0.0, 1.0);\n}\n",
        "fn effect(in: EffectInput)",
    );
    told("fn effect(in: EffectInput) -> vec3<f32> {\n    return picture(in.uv).rgb;\n}\n", "-> vec4<f32>");
    told(&format!("{KEEP}{SURFACE}"), "only `effect`");
    told(&format!("struct PerObject {{ hit: f32 }}\n{KEEP}"), "PerObject");
    told(&format!("var picture2: color_texture;\n{KEEP}"), "textures");
    told("fn effect(in: EffectInput) -> vec4<f32> {\n    return picture(in.uv)\n}\n", "fx.wgsl:3:1");
    let keep = CustomShader::new("keep.wgsl", KEEP).unwrap();
    assert!(keep.effect && keep.lighting == Lighting::Unlit);
    assert!(!CustomShader::new("s.wgsl", SURFACE).unwrap().effect);
}

/// An effect that keeps the picture keeps it, at every level and with the
/// pixel look; a frame without effects keeps no pictures for them.
#[test]
fn an_effect_that_keeps_the_picture_keeps_it() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    shader(&mut r, KEEP_KEY, "keep.wgsl", KEEP);
    for quality in LEVELS {
        for pixels in [None, Some(40)] {
            let mut plain = stage(quality, None);
            plain.camera.as_mut().unwrap().pixels = pixels;
            let mut kept = with_effects(quality, &[KEEP_KEY]);
            kept.camera = plain.camera;
            let mut fresh = renderer(&gpu, SRGB);
            let (before, stats) = render(&gpu, &mut fresh, &plain, SIZE, SIZE);
            assert_eq!((stats.effects, stats.effect_bytes), (0, 0));
            let (after, stats) = render(&gpu, &mut r, &kept, SIZE, SIZE);
            assert_eq!(stats.effects, 1);
            assert_eq!(stats.effect_bytes, u64::from(SIZE * SIZE * 4));
            let off = differing(&before, &after, 1);
            assert!(off == 0, "{quality:?} {pixels:?}: {off} pixels changed");
        }
    }
}

/// Each effect sees what the one before made: gain then invert isn't
/// invert then gain, and each is as worked out from the plain picture.
#[test]
fn effects_compose_in_order() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    shader(&mut r, INVERT_KEY, "invert.wgsl", INVERT);
    let gain = shader(&mut r, GAIN_KEY, "gain.wgsl", GAIN);
    let g = [1.0, 0.5, 0.25];
    for quality in LEVELS {
        let (plain, _) = render(&gpu, &mut r, &stage(quality, None), SIZE, SIZE);
        for (order, f) in [
            (
                [GAIN_KEY, INVERT_KEY],
                &(|c: [f32; 3]| [0, 1, 2].map(|i| 1.0 - c[i] * g[i])) as &dyn Fn([f32; 3]) -> [f32; 3],
            ),
            ([INVERT_KEY, GAIN_KEY], &|c: [f32; 3]| [0, 1, 2].map(|i| (1.0 - c[i]) * g[i])),
        ] {
            let mut scene = with_effects(quality, &order);
            scene.shader_values = values(&gain, "gain", &g);
            let (image, stats) = render(&gpu, &mut r, &scene, SIZE, SIZE);
            assert_eq!(stats.effects, 2);
            let off = differing(&image, &changed(&plain, f), 3);
            assert!(off < 40, "{quality:?} {order:?}: {off} pixels aren't as worked out");
        }
    }
}

/// An effect is told where each pixel is: from the view's top left, its
/// centre, and the view's size, with the pixel look too.
#[test]
fn an_effect_knows_where_each_pixel_is() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    shader(&mut r, WHERE_KEY, "where.wgsl", WHERE);
    let (w, h) = (SIZE, SIZE / 2);
    for pixels in [None, Some(20)] {
        let mut scene = with_effects(QualityLevel::Low, &[WHERE_KEY]);
        scene.camera.as_mut().unwrap().pixels = pixels;
        let (image, _) = render(&gpu, &mut r, &scene, w, h);
        let expected = Image {
            rgba: (0..h)
                .flat_map(|y| (0..w).map(move |x| (x, y)))
                .flat_map(|(x, y)| {
                    let u = (x as f32 + 0.5) / w as f32;
                    let v = (y as f32 + 0.5) / h as f32;
                    [u, v, u, 1.0]
                })
                .collect::<Vec<f32>>()
                .chunks(4)
                .flat_map(changed_one)
                .collect(),
            ..image.clone()
        };
        let off = differing(&image, &expected, 1);
        assert!(off == 0, "{pixels:?}: {off} pixels told the wrong place");
    }
}

/// Linear `c` (rgba) as an sRGB view stores it.
fn changed_one(c: &[f32]) -> [u8; 4] {
    let encode = |c: f32| {
        let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        (s * 255.0).round() as u8
    };
    [encode(c[0]), encode(c[1]), encode(c[2]), 255]
}

const OLD_TV: &str = "struct Values { lines: f32, curve: f32 }

fn effect(in: EffectInput) -> vec4<f32> {
    // Bent like a tube's glass, dark at its edges, with scan lines.
    let c = in.uv - 0.5;
    let uv = 0.5 + c * (1.0 + values.curve * dot(c, c));
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let lines = 0.75 + 0.25 * cos(uv.y * values.lines * 6.2831853);
    let dark = 1.0 - 1.6 * dot(c, c);
    return vec4<f32>(picture(uv).rgb * lines * dark, 1.0);
}
";

#[test]
fn picture_effects() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let tv = shader(&mut r, ShaderKey(24), "old_tv.wgsl", OLD_TV);
    shader(&mut r, GAIN_KEY, "gain.wgsl", GAIN);
    let mut blocks = values(&tv, "lines", &[40.0]);
    let curve = tv.values.iter().find(|v| v.name == "curve").unwrap().offset as usize / 4;
    blocks[curve / 12][curve % 12 / 4][curve % 4] = 0.4;
    // A warm tint after it, its values in the next block.
    let gain = CustomShader::new("gain.wgsl", GAIN).unwrap();
    blocks.extend(values(&gain, "gain", &[1.0, 0.85, 0.6]));
    for quality in LEVELS {
        let mut scene = stage(quality, None);
        scene.effects =
            vec![ScreenEffect { shader: ShaderKey(24), values: 0 }, ScreenEffect { shader: GAIN_KEY, values: 1 }];
        scene.shader_values = blocks.clone();
        let (image, _) = render(&gpu, &mut r, &scene, SIZE, SIZE);
        assert_golden(&format!("effects_{}", level_name(quality)), &image);
    }
}
