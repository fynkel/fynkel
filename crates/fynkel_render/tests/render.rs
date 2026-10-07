//! The renderer's promises (stage 1 plan, step 2), checked on the real GPU.

mod common;

use common::*;
use fynkel_render::glam::{Mat4, Quat, Vec3};
use fynkel_render::*;

const SRGB: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

#[test]
fn the_stage_1_cube_matches_its_golden() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let (image, stats) = render(&gpu, &mut r, &stage1_scene(), 256, 256);
    assert_eq!(stats, FrameStats { draws: 1, draw_calls: 1, missing_meshes: 0 });
    assert_golden("cube", &image);
}

#[test]
fn an_empty_scene_shows_exactly_the_clear_colour() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let mut camera = CameraView::looking_at(Vec3::Z, Vec3::ZERO);
    camera.clear_color = [linear(0.2), linear(0.4), linear(0.6)];
    let scene = RenderScene { camera: Some(camera), ..Default::default() };
    let (image, _) = render(&gpu, &mut r, &scene, 32, 32);
    for px in image.rgba.chunks(4) {
        assert_eq!(px, [51, 102, 153, 255], "the clear colour is shown as picked, not tonemapped");
    }

    // No camera: black.
    let (image, _) = render(&gpu, &mut r, &RenderScene::default(), 8, 8);
    assert!(image.rgba.chunks(4).all(|px| px == [0, 0, 0, 255]));
}

/// The units (plan §3.6), and the GPU's shading against the CPU copy of the
/// maths. A rough plane faces the camera; the sun shines from behind the
/// camera, straight onto it.
#[test]
fn light_units_match_the_documented_maths() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let albedo = Vec3::splat(linear(0.5));
    let camera = CameraView::looking_at(Vec3::new(0.0, 0.0, 3.0), Vec3::ZERO);
    let facing_camera = Mat4::from_rotation_x(90f32.to_radians()) * Mat4::from_scale(Vec3::splat(4.0));
    let draw = Draw {
        mesh: PLANE,
        model: facing_camera,
        material: PbrMaterial { base_color: [albedo.x, albedo.y, albedo.z, 1.0], metallic: 0.0, roughness: 1.0 },
    };
    let surface = Surface { albedo, metallic: 0.0, roughness: 1.0 };
    let (n, v) = (Vec3::Z, Vec3::Z); // at the centre pixel

    for (sun, ambient) in [(1.0, 0.0), (0.0, 1.0), (0.6, 0.3), (3.0, 0.0)] {
        let scene = RenderScene {
            camera: Some(camera),
            sun: (sun > 0.0).then_some(Sun { direction: Vec3::NEG_Z, color: Vec3::splat(sun) }),
            ambient: Vec3::splat(ambient),
            draws: vec![draw],
        };
        let (image, _) = render(&gpu, &mut r, &scene, 64, 64);
        let got = image.pixel(32, 32);
        let radiance = shade(&surface, n, v, Vec3::Z, Vec3::splat(sun), Vec3::splat(ambient));
        let want = to_srgb8(tonemap(radiance));
        for c in 0..3 {
            assert!(
                got[c].abs_diff(want[c]) <= 1,
                "sun {sun}, ambient {ambient}: GPU {got:?} vs maths {want:?} (radiance {radiance})"
            );
        }
        // Before tonemapping, a sun or ambient of 1.0 gives the albedo
        // (plus a small specular term).
        if sun + ambient == 1.0 {
            assert!((radiance.x - albedo.x).abs() < 0.02, "radiance {radiance} vs albedo {albedo}");
        }
    }
}

/// The lit side of a sphere follows the sun: the brightness-weighted centre
/// of the image moves towards the light.
#[test]
fn a_sphere_is_lit_from_where_the_sun_is() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let mut camera = CameraView::looking_at(Vec3::new(0.0, 0.0, 2.2), Vec3::ZERO);
    camera.fov_y = 40f32.to_radians();
    // (name, direction the light travels, where on screen the lit side should be)
    let cases = [
        ("left", Vec3::new(1.0, 0.0, -0.5), (-1.0, 0.0)),
        ("right", Vec3::new(-1.0, 0.0, -0.5), (1.0, 0.0)),
        ("top", Vec3::new(0.0, -1.0, -0.5), (0.0, -1.0)),
        ("front", Vec3::new(0.0, 0.0, -1.0), (0.0, 0.0)),
    ];
    for (name, direction, (want_x, want_y)) in cases {
        let scene = RenderScene {
            camera: Some(camera),
            sun: Some(Sun { direction, color: Vec3::ONE }),
            ambient: Vec3::splat(0.05),
            draws: vec![Draw {
                mesh: SPHERE,
                model: Mat4::IDENTITY,
                material: PbrMaterial { base_color: [0.8, 0.8, 0.8, 1.0], metallic: 0.0, roughness: 0.6 },
            }],
        };
        let (image, _) = render(&gpu, &mut r, &scene, 128, 128);
        let (mut sx, mut sy, mut total) = (0.0, 0.0, 0.0);
        for y in 0..128 {
            for x in 0..128 {
                let w = image.pixel(x, y)[1] as f32;
                sx += w * (x as f32 - 63.5);
                sy += w * (y as f32 - 63.5);
                total += w;
            }
        }
        let (cx, cy) = (sx / total, sy / total); // pixels; +y is down
        let tolerance = 2.0;
        let ok = |got: f32, want: f32| if want == 0.0 { got.abs() < tolerance } else { got * want > tolerance };
        assert!(ok(cx, want_x) && ok(cy, want_y), "{name}: brightness centre ({cx:.1}, {cy:.1})");
        assert_golden(&format!("sphere_{name}"), &image);
    }
}

#[test]
fn shading_is_the_same_whichever_target_format() {
    let gpu = gpu();
    let scene = stage1_scene();
    // (texture, view): plain textures are drawn through an sRGB view, as an
    // Android surface or a plain IOSurface would be.
    let cases = [
        (wgpu::TextureFormat::Rgba8UnormSrgb, wgpu::TextureFormat::Rgba8UnormSrgb),
        (wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureFormat::Rgba8UnormSrgb),
        (wgpu::TextureFormat::Bgra8UnormSrgb, wgpu::TextureFormat::Bgra8UnormSrgb),
        (wgpu::TextureFormat::Bgra8Unorm, wgpu::TextureFormat::Bgra8UnormSrgb),
    ];
    let images: Vec<Image> = cases
        .into_iter()
        .map(|(texture, view)| render_into(&gpu, &mut renderer(&gpu, view), &scene, 96, 96, texture).0)
        .collect();
    for (case, image) in cases.iter().zip(&images).skip(1) {
        let worst = image.rgba.iter().zip(&images[0].rgba).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
        assert!(worst <= 1, "{case:?} differs from Rgba8UnormSrgb by up to {worst}/255");
    }
}

/// Draws sharing a mesh become one instanced call, and each keeps its own
/// transform and material: the picture is the same as drawing them
/// interleaved one by one.
#[test]
fn draws_sharing_a_mesh_are_instanced_together() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let mut scene = stage1_scene();
    scene.camera = Some(CameraView::looking_at(Vec3::new(0.0, 4.0, 6.0), Vec3::ZERO));
    let material = |rgb: u32, metallic| PbrMaterial { base_color: linear_rgb(rgb), metallic, roughness: 0.4 };
    let at = |x: f32, z: f32| Mat4::from_translation(Vec3::new(x, 0.0, z));
    // Interleaved meshes and materials: cube, sphere, cube, sphere, cube.
    scene.draws = vec![
        Draw { mesh: CUBE, model: at(-2.0, 0.0), material: material(0xFFE0613A, 0.0) },
        Draw { mesh: SPHERE, model: at(-1.0, 1.0), material: material(0xFF3A8EE0, 0.0) },
        Draw { mesh: CUBE, model: at(0.0, 0.0), material: material(0xFF4CC26B, 1.0) },
        Draw { mesh: SPHERE, model: at(1.0, 1.0), material: material(0xFFE0D23A, 1.0) },
        Draw { mesh: CUBE, model: at(2.0, 0.0), material: material(0xFFB03AE0, 0.0) },
    ];
    let (image, stats) = render(&gpu, &mut r, &scene, 256, 160);
    assert_eq!(stats, FrameStats { draws: 5, draw_calls: 2, missing_meshes: 0 });
    assert_golden("instanced", &image);
}

#[test]
#[should_panic(expected = "the renderer draws into sRGB views")]
fn a_plain_format_view_is_refused() {
    let gpu = gpu();
    let _ = Renderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
}

#[test]
fn a_missing_mesh_is_skipped_and_counted() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let mut scene = stage1_scene();
    let mut ghost = scene.draws[0];
    ghost.mesh = MeshKey(404);
    scene.draws.push(ghost);
    let (_, stats) = render(&gpu, &mut r, &scene, 32, 32);
    assert_eq!(stats, FrameStats { draws: 1, draw_calls: 1, missing_meshes: 1 });

    assert!(r.remove_mesh(CUBE));
    assert!(!r.remove_mesh(CUBE));
    let (image, stats) = render(&gpu, &mut r, &stage1_scene(), 32, 32);
    assert_eq!(stats, FrameStats { draws: 0, draw_calls: 0, missing_meshes: 1 });
    let clear = image.pixel(16, 16);
    assert_eq!(clear, image.pixel(0, 0), "with the cube gone, the centre shows the clear colour");
}

#[test]
fn the_target_can_change_size_between_frames() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let scene = stage1_scene();
    let big = render(&gpu, &mut r, &scene, 128, 128).0;
    let _ = render(&gpu, &mut r, &scene, 200, 50);
    let _ = render(&gpu, &mut r, &scene, 1, 1);
    let again = render(&gpu, &mut r, &scene, 128, 128).0;
    assert!(big.rgba == again.rgba, "the same size renders the same pixels after resizes");
}

/// Each frame's uniforms and instances go into one of three buffers the
/// GPU reads in place. Nine frames of three scenes, submitted without
/// waiting, go round the buffers three times while the GPU still reads
/// earlier ones, and grow them on the way (the 200-cube scene). Each frame
/// must show its own scene, exactly as drawn alone.
#[test]
fn frames_in_flight_keep_their_own_data() {
    let gpu = gpu();
    let one = stage1_scene();
    let mut many = stage1_scene();
    many.camera = Some(CameraView::looking_at(Vec3::new(0.0, 12.0, 16.0), Vec3::ZERO));
    many.draws = (0..200)
        .map(|i| Draw {
            mesh: if i % 2 == 0 { CUBE } else { SPHERE },
            model: Mat4::from_scale_rotation_translation(
                Vec3::splat(0.5),
                Quat::from_rotation_y(i as f32),
                Vec3::new((i % 20) as f32 - 9.5, 0.0, (i / 20) as f32 * 1.2 - 6.0),
            ),
            material: PbrMaterial {
                base_color: linear_rgb(0xFF000000 | (i * 0x0A1B2C)),
                metallic: 0.0,
                roughness: 0.5,
            },
        })
        .collect();
    let mut empty = stage1_scene();
    empty.draws.clear();
    empty.camera.as_mut().expect("has a camera").clear_color = [0.0, 0.2, 0.0];
    let scenes = [&one, &many, &empty];

    let (width, height) = (96, 64);
    let alone: Vec<Image> =
        scenes.iter().map(|s| render(&gpu, &mut renderer(&gpu, SRGB), s, width, height).0).collect();

    let mut r = renderer(&gpu, SRGB);
    let targets: Vec<wgpu::Texture> = (0..9)
        .map(|_| {
            gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: SRGB,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
        })
        .collect();
    for (frame, target) in targets.iter().enumerate() {
        r.render(scenes[frame % 3], &target.create_view(&Default::default()), width, height);
    }
    for (frame, target) in targets.iter().enumerate() {
        assert!(read_back(&gpu, target).rgba == alone[frame % 3].rgba, "frame {frame} doesn't show its own scene");
    }
}

#[test]
fn non_uniform_scale_keeps_normals_right() {
    // A cube turned, then squashed to a slab: the squash shears it, so the
    // top face's normal must tilt back towards +Y (the inverse-transpose
    // rule). Using the model matrix itself would tilt it the other way.
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let camera = CameraView::looking_at(Vec3::new(0.0, 6.0, 0.001), Vec3::ZERO);
    let albedo = Vec3::splat(0.5);
    let squash = Vec3::new(3.0, 0.2, 3.0);
    let turn = Quat::from_rotation_z(0.6);
    let scene = RenderScene {
        camera: Some(camera),
        sun: Some(Sun { direction: Vec3::NEG_Y, color: Vec3::ONE }),
        ambient: Vec3::ZERO,
        draws: vec![Draw {
            mesh: CUBE,
            model: Mat4::from_scale(squash) * Mat4::from_quat(turn),
            material: PbrMaterial { base_color: [0.5, 0.5, 0.5, 1.0], metallic: 0.0, roughness: 1.0 },
        }],
    };
    let got = render(&gpu, &mut r, &scene, 64, 64).0.pixel(32, 32);

    let surface = Surface { albedo, metallic: 0.0, roughness: 1.0 };
    let top = turn * Vec3::Y;
    let expect = |n: Vec3| to_srgb8(tonemap(shade(&surface, n, Vec3::Y, Vec3::Y, Vec3::ONE, Vec3::ZERO)));
    let right = expect((top / squash).normalize());
    let wrong = expect((top * squash).normalize());
    assert!(right[0].abs_diff(wrong[0]) > 20, "the test must tell the two apart: {right:?} vs {wrong:?}");
    for c in 0..3 {
        assert!(got[c].abs_diff(right[c]) <= 2, "GPU {got:?}, inverse-transpose {right:?}, wrong {wrong:?}");
    }
}
