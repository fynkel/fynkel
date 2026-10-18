//! Stage 10 phase 1: materials' own shaders. A shader that gives the
//! standard surface draws as the built-in material does, at every level;
//! its values, its own textures, an object's own numbers and the game's
//! time reach it; and an unlit one shows its colour as it is.

mod common;

use std::sync::Arc;

use common::*;
use fynkel_render::glam::{Mat4, Quat, Vec3};
use fynkel_render::*;

const SRGB: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const SIZE: u32 = 160;
const LEVELS: [QualityLevel; 3] = [QualityLevel::Low, QualityLevel::Medium, QualityLevel::High];
const CHECKER: TextureKey = TextureKey(20);

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

/// A 4 × 4 checker of two colours.
fn checker() -> Vec<u8> {
    (0..16).flat_map(|i| if (i % 4 + i / 4) % 2 == 0 { [230, 200, 60, 255] } else { [40, 90, 200, 255] }).collect()
}

fn renderer_with_textures(gpu: &Gpu) -> Renderer {
    let mut r = renderer(gpu, SRGB);
    r.upload_texture(CHECKER, ColorSpace::Srgb, 4, 4, &checker());
    r.upload_texture(CHECKER, ColorSpace::Linear, 4, 4, &checker());
    r
}

fn material(argb: u32, roughness: f32) -> PbrMaterial {
    PbrMaterial { base_color: linear_rgb(argb), metallic: 0.0, roughness, ..PbrMaterial::DEFAULT }
}

/// A floor, a textured sphere and a turned cube under a sun that casts
/// shadows, seen from in front and above.
fn stage(quality: QualityLevel) -> RenderScene {
    let textured = PbrMaterial {
        textures: MaterialTextures { base_color: Some(CHECKER), ..MaterialTextures::NONE },
        uv_scale: [3.0, 2.0],
        ..material(0xFFFFFFFF, 0.4)
    };
    RenderScene {
        camera: Some(CameraView::looking_at(Vec3::new(0.8, 2.4, 4.0), Vec3::new(0.0, 0.6, 0.0))),
        lights: vec![Light::sun(Vec3::new(1.0, -1.5, -0.5), Vec3::ONE).with_shadows(true)],
        quality: Some(quality.quality()),
        ambient: Vec3::splat(0.15),
        draws: vec![
            Draw::new(PLANE, Mat4::from_scale(Vec3::splat(12.0)), material(0xFFB0B0B0, 0.8)),
            Draw::new(SPHERE, Mat4::from_translation(Vec3::new(-1.0, 1.0, 0.0)), textured),
            Draw::new(
                CUBE,
                Mat4::from_rotation_translation(Quat::from_rotation_y(0.5), Vec3::new(1.2, 0.5, 0.3)),
                PbrMaterial { metallic: 1.0, ..material(0xFFD08050, 0.3) },
            ),
        ],
        ..RenderScene::default()
    }
}

fn shader(r: &mut Renderer, key: ShaderKey, name: &str, source: &str) -> Arc<CustomShader> {
    let shader = Arc::new(CustomShader::new(name, source).unwrap_or_else(|e| panic!("{e}")));
    r.upload_shader(key, shader.clone());
    shader
}

/// Every material of `scene` drawn with `shader` instead, its values at 0.
fn with_shader(mut scene: RenderScene, shader: ShaderKey) -> RenderScene {
    for draw in &mut scene.draws {
        draw.material.shader = Some(ShaderUse { shader, values: 0, textures: [None; MAX_TEXTURES] });
    }
    scene
}

#[test]
fn a_shader_that_gives_the_standard_surface_draws_as_the_built_in_material() {
    let gpu = gpu();
    let mut r = renderer_with_textures(&gpu);
    let same = ShaderKey(1);
    shader(&mut r, same, "same.wgsl", "fn surface(in: SurfaceInput) -> Surface {\n    return standard(in);\n}\n");
    for quality in LEVELS {
        let (expected, _) = render(&gpu, &mut r, &stage(quality), SIZE, SIZE);
        let (image, stats) = render(&gpu, &mut r, &with_shader(stage(quality), same), SIZE, SIZE);
        assert_eq!(stats.draws, 3);
        let off = differing(&image, &expected, 2);
        assert!(off == 0, "{off} pixels differ at {}", level_name(quality));
    }
}

/// `values` laid out as `shader`'s `Values`, in the scene's blocks.
fn values(shader: &CustomShader, numbers: &[(&str, &[f32])]) -> Vec<[[f32; 4]; 3]> {
    let mut words = vec![0f32; (shader.values_size as usize).div_ceil(48) * 12];
    for (name, value) in numbers {
        let v = shader.values.iter().find(|v| v.name == *name).unwrap_or_else(|| panic!("no value {name}"));
        let at = v.offset as usize / 4;
        words[at..at + value.len()].copy_from_slice(value);
    }
    words.chunks(12).map(|c| [[c[0], c[1], c[2], c[3]], [c[4], c[5], c[6], c[7]], [c[8], c[9], c[10], c[11]]]).collect()
}

const LAVA: &str = "struct Values { hot: vec4<f32>, scale: f32, flow: f32 }

fn surface(in: SurfaceInput) -> Surface {
    var s = standard(in);
    let n = noise(in.uv * values.scale + vec2<f32>(0.0, frame_time() * values.flow));
    s.color = vec3<f32>(0.05);
    s.emission = values.hot.rgb * smoothstep(0.35, 0.75, n) * 2.0;
    return s;
}
";

const FLASH: &str = "struct PerObject { flash: f32 }
var picture: color_texture;

fn surface(in: SurfaceInput) -> Surface {
    var s = standard(in);
    s.color = sample(picture, in.uv * 2.0).rgb;
    s.emission = vec3<f32>(1.0, 0.2, 0.1) * per_object.flash;
    return s;
}
";

const HOLOGRAM: &str = "fn unlit(in: SurfaceInput) -> vec4<f32> {
    let lines = step(0.5, fract(in.position.y * 12.0 - frame_time()));
    let rim = fresnel(in, 2.0);
    return vec4<f32>(in.color.rgb * (rim * 1.5 + 0.2 * lines), 1.0);
}
";

/// Lava (values and time), two cubes sharing a material with their own
/// texture, one flashing (an object's own numbers), and a hologram (unlit,
/// additive) in front of them.
#[test]
fn values_textures_an_objects_numbers_and_time_reach_a_shader() {
    let gpu = gpu();
    let mut r = renderer_with_textures(&gpu);
    let lava = shader(&mut r, ShaderKey(1), "lava.wgsl", LAVA);
    let flash = shader(&mut r, ShaderKey(2), "flash.wgsl", FLASH);
    shader(&mut r, ShaderKey(3), "hologram.wgsl", HOLOGRAM);
    assert_eq!(flash.own, ["flash"]);
    for quality in LEVELS {
        let mut scene = stage(quality);
        scene.time = [2.5, 1.0 / 60.0];
        scene.draws.truncate(1);
        scene.shader_values = values(&lava, &[("hot", &[1.0, 0.35, 0.05, 1.0]), ("scale", &[6.0]), ("flow", &[0.4])]);
        let lava = PbrMaterial {
            shader: Some(ShaderUse { shader: ShaderKey(1), values: 0, textures: [None; MAX_TEXTURES] }),
            ..material(0xFFFFFFFF, 0.9)
        };
        scene.draws.push(Draw::new(
            PLANE,
            Mat4::from_translation(Vec3::new(0.0, 0.01, 0.0)) * Mat4::from_scale(Vec3::splat(2.5)),
            lava,
        ));
        let flashing = PbrMaterial {
            shader: Some(ShaderUse { shader: ShaderKey(2), values: 0, textures: [Some(CHECKER), None, None, None] }),
            ..material(0xFFFFFFFF, 0.5)
        };
        for (x, own) in [(-1.3, 0.0), (1.3, 1.0)] {
            scene.draws.push(Draw {
                own: [own, 0.0, 0.0, 0.0],
                ..Draw::new(CUBE, Mat4::from_translation(Vec3::new(x, 0.5, -0.5)), flashing)
            });
        }
        let hologram = PbrMaterial {
            shader: Some(ShaderUse { shader: ShaderKey(3), values: 0, textures: [None; MAX_TEXTURES] }),
            blend: Blend::Additive,
            ..material(0xFF40C0FF, 0.5)
        };
        scene.draws.push(Draw::new(
            SPHERE,
            Mat4::from_translation(Vec3::new(0.0, 1.0, 1.0)) * Mat4::from_scale(Vec3::splat(0.6)),
            hologram,
        ));
        let (image, stats) = render(&gpu, &mut r, &scene, SIZE, SIZE);
        assert_eq!(stats.draws, 5);
        // The two cubes share a shader, its textures and their mesh: one call.
        assert_eq!(stats.draw_calls, 4, "at {}", level_name(quality));
        fynkel_golden::assert_golden(&format!("shaders_{}", level_name(quality)), &image);
    }
}

#[test]
fn a_draw_whose_shader_isnt_uploaded_isnt_drawn() {
    let gpu = gpu();
    let mut r = renderer_with_textures(&gpu);
    let (image, stats) = render(&gpu, &mut r, &with_shader(stage(QualityLevel::Medium), ShaderKey(9)), SIZE, SIZE);
    assert_eq!(stats.draws, 0);
    let (empty, _) =
        render(&gpu, &mut r, &RenderScene { draws: Vec::new(), ..stage(QualityLevel::Medium) }, SIZE, SIZE);
    assert_eq!(differing(&image, &empty, 0), 0);
}

const HOLES: TextureKey = TextureKey(21);

/// A 4 × 4 checker with every other cell clear.
fn holes() -> Vec<u8> {
    (0..16).flat_map(|i| if (i % 4 + i / 4) % 2 == 0 { [230, 200, 60, 255] } else { [0, 0, 0, 0] }).collect()
}

/// [`stage`] with its cube and sphere cut out by [`HOLES`], so their
/// shadows have holes too, lit low and from the side so they're long.
fn cut_out_stage(quality: QualityLevel) -> RenderScene {
    let mut scene = stage(quality);
    scene.lights = vec![Light::sun(Vec3::new(1.0, -0.8, -0.3), Vec3::ONE).with_shadows(true)];
    for draw in &mut scene.draws[1..] {
        draw.material.textures = MaterialTextures { base_color: Some(HOLES), ..MaterialTextures::NONE };
        draw.material.alpha_cutoff = 0.5;
    }
    scene
}

const DISSOLVE: &str = "struct Values { amount: f32 }

fn surface(in: SurfaceInput) -> Surface {
    var s = standard(in);
    let n = noise(in.uv * 8.0);
    s.alpha = step(values.amount, n);
    s.emission = vec3<f32>(1.0, 0.4, 0.1) * (1.0 - smoothstep(0.0, 0.06, n - values.amount)) * 3.0;
    return s;
}
";

/// Every cut-out's shadow is cut where its own shader cuts it: one that
/// gives the standard surface draws its holes and their shadows as the
/// built-in material does, one whose alpha is 0 leaves no shadow, and a
/// dissolve's shadow dissolves with it.
#[test]
fn a_cut_out_shaders_shadow_is_cut_where_it_is() {
    let gpu = gpu();
    let mut r = renderer_with_textures(&gpu);
    r.upload_texture(HOLES, ColorSpace::Srgb, 4, 4, &holes());
    r.upload_texture(HOLES, ColorSpace::Linear, 4, 4, &holes());
    let same = ShaderKey(1);
    shader(&mut r, same, "same.wgsl", "fn surface(in: SurfaceInput) -> Surface {\n    return standard(in);\n}\n");
    let gone = ShaderKey(2);
    let source = "fn surface(in: SurfaceInput) -> Surface {\n    var s = standard(in);\n    s.alpha = 0.0;\n    return s;\n}\n";
    shader(&mut r, gone, "gone.wgsl", source);
    let dissolve = shader(&mut r, ShaderKey(3), "dissolve.wgsl", DISSOLVE);
    let own = |scene: RenderScene, key: ShaderKey| {
        let mut scene = scene;
        for draw in &mut scene.draws[1..] {
            draw.material.shader = Some(ShaderUse { shader: key, values: 0, textures: [None; MAX_TEXTURES] });
        }
        scene
    };
    for quality in LEVELS {
        let (expected, _) = render(&gpu, &mut r, &cut_out_stage(quality), SIZE, SIZE);
        let (image, stats) = render(&gpu, &mut r, &own(cut_out_stage(quality), same), SIZE, SIZE);
        assert_eq!(stats.draws, 3);
        let off = differing(&image, &expected, 2);
        assert!(off == 0, "{off} pixels differ at {}", level_name(quality));

        let mut floor_only = cut_out_stage(quality);
        floor_only.draws.truncate(1);
        let (expected, _) = render(&gpu, &mut r, &floor_only, SIZE, SIZE);
        let (image, _) = render(&gpu, &mut r, &own(cut_out_stage(quality), gone), SIZE, SIZE);
        let off = differing(&image, &expected, 2);
        assert!(off == 0, "{off} pixels differ at {}: its shadow is still there", level_name(quality));
    }
    let mut scene = own(stage(QualityLevel::Medium), ShaderKey(3));
    scene.lights = vec![Light::sun(Vec3::new(1.0, -0.8, -0.3), Vec3::ONE).with_shadows(true)];
    for draw in &mut scene.draws[1..] {
        draw.material.alpha_cutoff = 0.5;
    }
    scene.shader_values = values(&dissolve, &[("amount", &[0.45])]);
    let (image, _) = render(&gpu, &mut r, &scene, SIZE, SIZE);
    fynkel_golden::assert_golden("shaders_dissolve", &image);
}
