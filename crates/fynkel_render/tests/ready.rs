//! Stage 10 phase 4b: a renderer that doesn't wait for its own shaders'
//! pipelines (a view on screen) draws the frame without their objects
//! while they're made on threads of their own, so a shader the driver
//! hasn't seen doesn't hold the frame up, then draws them as one that
//! waits does: their surfaces, shadows, ambient occlusion and outlines.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use common::*;
use fynkel_render::glam::{Mat4, Vec3};
use fynkel_render::*;

const SRGB: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const SIZE: u32 = 96;

/// Red and moved a little by its `vertex`, so it has a shadow and an
/// ambient occlusion depth of its own; with a number no driver has seen,
/// so its pipelines are compiled afresh.
fn source() -> String {
    let fresh = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().subsec_nanos();
    format!(
        "fn vertex(in: VertexInput) -> VertexInput {{\n    var v = in;\n    v.position.y += 0.01;\n    return v;\n}}\n\n\
         fn unlit(in: SurfaceInput) -> vec4<f32> {{\n    return vec4<f32>(1.0, 0.0, f32({fresh}u) * 1e-20, 1.0);\n}}\n"
    )
}

/// A red cube with an outline, on a floor, under a sun with shadows, at
/// high (ambient occlusion on).
fn scene() -> RenderScene {
    let shader = ShaderUse { shader: ShaderKey(1), values: 0, textures: [None; MAX_TEXTURES], reach: 0.05 };
    let cube = PbrMaterial {
        shader: Some(shader),
        outline: Some(Outline { width: 0.05, color: [0.0, 1.0, 0.0] }),
        ..PbrMaterial::DEFAULT
    };
    RenderScene {
        camera: Some(CameraView::looking_at(Vec3::new(0.0, 1.6, 3.2), Vec3::new(0.0, 0.5, 0.0))),
        lights: vec![Light::sun(Vec3::new(0.4, -1.0, -0.3), Vec3::ONE).with_shadows(true)],
        quality: Some(QualityLevel::High.quality()),
        ambient: Vec3::splat(0.3),
        draws: vec![
            Draw::new(PLANE, Mat4::from_scale(Vec3::splat(8.0)), PbrMaterial::DEFAULT),
            Draw::new(CUBE, Mat4::from_translation(Vec3::new(0.0, 0.5, 0.0)), cube),
        ],
        ..RenderScene::default()
    }
}

fn red(image: &Image) -> bool {
    let at = ((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize;
    let [r, g, b] = [image.rgba[at], image.rgba[at + 1], image.rgba[at + 2]];
    r > 200 && g < 40 && b < 40
}

#[test]
fn a_new_shader_is_made_while_the_frames_go_on() {
    let gpu = gpu();
    let shader = Arc::new(CustomShader::new("fresh.wgsl", &source()).unwrap_or_else(|e| panic!("{e}")));
    let mut waits = renderer(&gpu, SRGB);
    waits.upload_shader(ShaderKey(1), shader.clone());
    let (expected, _) = render(&gpu, &mut waits, &scene(), SIZE, SIZE);
    assert!(red(&expected), "a renderer that waits draws it in its first frame");

    // Its own kit, so nothing's made for it yet.
    let mut r = renderer(&gpu, SRGB);
    r.set_wait_for_shaders(false);
    r.upload_shader(ShaderKey(1), shader);
    let (first, _) = render(&gpu, &mut r, &scene(), SIZE, SIZE);
    assert!(!red(&first), "the first frame goes without it");
    let (_, stats) = render(&gpu, &mut r, &scene(), SIZE, SIZE);
    assert!(stats.shaders_waiting > 0, "{stats:?}");

    let start = Instant::now();
    loop {
        let (image, stats) = render(&gpu, &mut r, &scene(), SIZE, SIZE);
        if stats.shaders_waiting == 0 && red(&image) {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(20), "still waiting: {stats:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
    // A frame after the last was made: a pipeline made mid-frame may have
    // missed the one before.
    let (last, _) = render(&gpu, &mut r, &scene(), SIZE, SIZE);
    let off = last.rgba.chunks(4).zip(expected.rgba.chunks(4)).filter(|(a, b)| a != b).count();
    assert_eq!(off, 0, "then it's drawn as a renderer that waits draws it: shadow, occlusion, outline");
}
