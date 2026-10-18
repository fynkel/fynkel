//! Stage 10 phase 4: whole styles. A shader's `light` lights it in place
//! of the engine, light by light: one that returns `standard_light` looks
//! as the built-in material does; banded light gives few shades; the
//! shadow is passed to it; and a `light` that's wrong is told. Outlines
//! ring an object in their colour, as wide as they say, follow a mesh a
//! shader moves or a skeleton bends, and aren't drawn for see-through
//! surfaces; and a toon scene at every level.

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
