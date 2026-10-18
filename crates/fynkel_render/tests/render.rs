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
    assert_eq!(
        stats,
        FrameStats { draws: 1, draw_calls: 1, missing_meshes: 0, culled: 0, lines: 0, ..Default::default() }
    );
    assert_golden("cube", &image);
}

/// Every level draws the same picture (stage 8 phase 1): the scene image
/// and the last pass change nothing on screen. A renderer can change level
/// from one frame to the next, and back.
#[test]
fn every_quality_level_draws_the_same_cube() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let (_, _) = render(&gpu, &mut r, &stage1_scene(), 256, 256);
    assert_eq!(
        r.quality(),
        r.assets().lock().unwrap().kit().device_quality().quality(),
        "the device's level, unless the scene picks"
    );
    for quality in [QualityLevel::Low, QualityLevel::High, QualityLevel::Medium, QualityLevel::Low] {
        let scene = RenderScene { quality: Some(quality.quality()), ..stage1_scene() };
        let (image, stats) = render(&gpu, &mut r, &scene, 256, 256);
        assert_eq!((r.quality(), stats.draws), (quality.quality(), 1));
        assert_golden("cube", &image);
    }
}

/// The device's level: Apple's GPUs and desktops high, Mali and Adreno
/// low (docs/android-pacing-report.md), a software GPU low.
#[test]
fn each_gpu_gets_its_quality_level() {
    let this = gpu().adapter.get_info();
    let gpu = |vendor: u32, device_type: wgpu::DeviceType, backend: wgpu::Backend| wgpu::AdapterInfo {
        vendor,
        device_type,
        backend,
        ..this.clone()
    };
    use wgpu::{Backend::*, DeviceType::*};
    let cases = [
        (gpu(0x106B, IntegratedGpu, Metal), QualityLevel::High),
        (gpu(0x13B5, IntegratedGpu, Vulkan), QualityLevel::Low),
        (gpu(0x5143, IntegratedGpu, Vulkan), QualityLevel::Low),
        (gpu(0x10DE, DiscreteGpu, Vulkan), QualityLevel::High),
        (gpu(0x8086, IntegratedGpu, Dx12), QualityLevel::High),
        (gpu(0x10005, Cpu, Vulkan), QualityLevel::Low),
        (gpu(0x1234, Other, Vulkan), QualityLevel::Low),
    ];
    for (info, want) in cases {
        assert_eq!(QualityLevel::for_adapter_on(&info, false), want, "{info:?}");
    }
    #[cfg(target_os = "macos")]
    assert_eq!(QualityLevel::for_adapter(&this), QualityLevel::High, "this Mac");
    #[cfg(target_os = "android")]
    assert_eq!(QualityLevel::for_adapter(&this), QualityLevel::Low, "this phone");

    // On Android (hardening R4), a phone's GPU is low whoever makes it,
    // not high because its maker isn't known: PowerVR, Samsung's Xclipse
    // (AMD's id), NVIDIA's Tegra. One that isn't a real GPU is low too.
    let android = [
        (gpu(0x13B5, IntegratedGpu, Vulkan), QualityLevel::Low),
        (gpu(0x5143, IntegratedGpu, Vulkan), QualityLevel::Low),
        (gpu(0x1010, IntegratedGpu, Vulkan), QualityLevel::Low),
        (gpu(0x1002, IntegratedGpu, Vulkan), QualityLevel::Low),
        (gpu(0x10DE, IntegratedGpu, Vulkan), QualityLevel::Low),
        (gpu(0x10DE, DiscreteGpu, Gl), QualityLevel::Low),
        (gpu(0x1234, Other, Vulkan), QualityLevel::Low),
        (gpu(0x1AE0, VirtualGpu, Vulkan), QualityLevel::Low),
        (gpu(0x10005, Cpu, Vulkan), QualityLevel::Low),
    ];
    for (info, want) in android {
        assert_eq!(QualityLevel::for_adapter_on(&info, true), want, "Android: {info:?}");
    }
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
        skin: None,
        own: [0.0; 4],
        mesh: PLANE,
        model: facing_camera,
        material: PbrMaterial {
            base_color: [albedo.x, albedo.y, albedo.z, 1.0],
            metallic: 0.0,
            roughness: 1.0,
            ..PbrMaterial::DEFAULT
        },
        cast_shadows: true,
        receive_shadows: true,
    };
    let surface = Surface { albedo, metallic: 0.0, roughness: 1.0 };
    let (n, v) = (Vec3::Z, Vec3::Z); // at the centre pixel

    for (sun, ambient) in [(1.0, 0.0), (0.0, 1.0), (0.6, 0.3), (3.0, 0.0)] {
        let scene = RenderScene {
            joints: Vec::new(),
            camera: Some(camera),
            lights: (sun > 0.0).then(|| Light::sun(Vec3::NEG_Z, Vec3::splat(sun))).into_iter().collect(),
            quality: None,
            time: [0.0; 2],
            shader_values: Vec::new(),
            ambient: Vec3::splat(ambient),
            lines: Vec::new(),
            sky: None,
            fog: None,
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
            joints: Vec::new(),
            camera: Some(camera),
            lights: vec![Light::sun(direction, Vec3::ONE)],
            quality: None,
            time: [0.0; 2],
            shader_values: Vec::new(),
            ambient: Vec3::splat(0.05),
            lines: Vec::new(),
            sky: None,
            fog: None,
            draws: vec![Draw {
                skin: None,
                own: [0.0; 4],
                mesh: SPHERE,
                model: Mat4::IDENTITY,
                material: PbrMaterial {
                    base_color: [0.8, 0.8, 0.8, 1.0],
                    metallic: 0.0,
                    roughness: 0.6,
                    ..PbrMaterial::DEFAULT
                },
                cast_shadows: true,
                receive_shadows: true,
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
    let material = |rgb: u32, metallic| PbrMaterial {
        base_color: linear_rgb(rgb),
        metallic,
        roughness: 0.4,
        ..PbrMaterial::DEFAULT
    };
    let at = |x: f32, z: f32| Mat4::from_translation(Vec3::new(x, 0.0, z));
    // Interleaved meshes and materials: cube, sphere, cube, sphere, cube.
    scene.draws = vec![
        Draw {
            skin: None,
            own: [0.0; 4],
            mesh: CUBE,
            model: at(-2.0, 0.0),
            material: material(0xFFE0613A, 0.0),
            cast_shadows: true,
            receive_shadows: true,
        },
        Draw {
            skin: None,
            own: [0.0; 4],
            mesh: SPHERE,
            model: at(-1.0, 1.0),
            material: material(0xFF3A8EE0, 0.0),
            cast_shadows: true,
            receive_shadows: true,
        },
        Draw {
            skin: None,
            own: [0.0; 4],
            mesh: CUBE,
            model: at(0.0, 0.0),
            material: material(0xFF4CC26B, 1.0),
            cast_shadows: true,
            receive_shadows: true,
        },
        Draw {
            skin: None,
            own: [0.0; 4],
            mesh: SPHERE,
            model: at(1.0, 1.0),
            material: material(0xFFE0D23A, 1.0),
            cast_shadows: true,
            receive_shadows: true,
        },
        Draw {
            skin: None,
            own: [0.0; 4],
            mesh: CUBE,
            model: at(2.0, 0.0),
            material: material(0xFFB03AE0, 0.0),
            cast_shadows: true,
            receive_shadows: true,
        },
    ];
    let (image, stats) = render(&gpu, &mut r, &scene, 256, 160);
    assert_eq!(
        stats,
        FrameStats { draws: 5, draw_calls: 2, missing_meshes: 0, culled: 0, lines: 0, ..Default::default() }
    );
    assert_golden("instanced", &image);
}

/// An orthographic camera draws things the same size however far away:
/// three cubes 1 m wide, at 10, 15 and 30 m, are each 32 pixels wide, as
/// the view is 8 m across (half its height is 2 m, and it's twice as wide
/// as high) over 256 pixels.
#[test]
fn an_orthographic_camera_keeps_sizes_at_any_distance() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let mut scene = stage1_scene();
    let mut camera = CameraView::looking_at(Vec3::new(0.0, 0.0, 10.0), Vec3::ZERO);
    camera.clear_color = [0.0; 3];
    camera.orthographic = Some(2.0);
    scene.camera = Some(camera);
    let material = scene.draws[0].material;
    scene.draws = [(-2.5, 0.0), (0.0, -5.0), (2.5, -20.0)]
        .map(|(x, z)| Draw {
            skin: None,
            own: [0.0; 4],
            mesh: CUBE,
            model: Mat4::from_translation(Vec3::new(x, 0.0, z)),
            material,
            cast_shadows: true,
            receive_shadows: true,
        })
        .to_vec();
    let (image, _) = render(&gpu, &mut r, &scene, 256, 128);
    // Across the middle row: where each cube starts, and how wide it is.
    let row = &image.rgba[64 * 256 * 4..65 * 256 * 4];
    let mut runs = Vec::new();
    let mut start = None;
    for (x, p) in row.chunks(4).chain([[0u8; 4].as_slice()]).enumerate() {
        match (p[..3] != [0, 0, 0], start) {
            (true, None) => start = Some(x),
            (false, Some(s)) => {
                runs.push((s, x - s));
                start = None;
            }
            _ => {}
        }
    }
    // Centred at 128 + 32 × x: 48, 128 and 208.
    assert_eq!(runs, [(32, 32), (112, 32), (192, 32)]);
    assert_golden("orthographic", &image);
}

#[test]
#[should_panic(expected = "the renderer draws into sRGB views")]
fn a_plain_format_view_is_refused() {
    let gpu = gpu();
    let _ = Renderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
}

/// A draw wholly outside the view isn't drawn; one partly in it is, and
/// a stretched one is judged by its stretched size.
#[test]
fn draws_outside_the_view_are_culled() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let mut camera = CameraView::looking_at(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO);
    camera.clear_color = [0.0; 3];
    let cube = |model: Mat4| Draw {
        skin: None,
        own: [0.0; 4],
        mesh: CUBE,
        model,
        material: PbrMaterial::DEFAULT,
        cast_shadows: true,
        receive_shadows: true,
    };
    let at = |x: f32, y: f32, z: f32| Mat4::from_translation(Vec3::new(x, y, z));
    // At 5 away the view is 2.89 either side of the middle (60°, square).
    let draws = vec![
        cube(Mat4::IDENTITY),
        cube(at(0.0, 0.0, 10.0)),    // behind the camera
        cube(at(-100.0, 0.0, 0.0)),  // far to the left
        cube(at(0.0, 0.0, -2000.0)), // beyond the far plane
        cube(at(-4.2, 0.0, 0.0)),    // just outside the left edge
        cube(at(0.0, 4.2, 0.0) * Mat4::from_scale(Vec3::new(1.0, 4.0, 1.0))), // stretched into the top
        cube(at(-3.2, 0.0, 0.0)),    // its corner over the left edge
    ];
    let scene = RenderScene { camera: Some(camera), ambient: Vec3::ONE, draws, ..Default::default() };
    let (image, stats) = render(&gpu, &mut r, &scene, 64, 64);
    assert_eq!(
        stats,
        FrameStats { draws: 3, draw_calls: 1, missing_meshes: 0, culled: 4, lines: 0, ..Default::default() }
    );
    let lit = |x: u32, y: u32| image.pixel(x, y) != [0, 0, 0, 255];
    assert!(lit(32, 32) && lit(32, 0) && lit(0, 32), "the middle, the top and the left edge are drawn");

    // An orthographic camera's box, 1 either side.
    camera.orthographic = Some(1.0);
    let scene = RenderScene {
        camera: Some(camera),
        ambient: Vec3::ONE,
        draws: vec![cube(at(3.0, 0.0, 0.0)), cube(at(1.2, 0.0, 0.0)), cube(at(0.0, -1.2, 0.0))],
        ..Default::default()
    };
    let (_, stats) = render(&gpu, &mut r, &scene, 64, 64);
    assert_eq!((stats.draws, stats.culled), (2, 1));
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
    assert_eq!(
        stats,
        FrameStats { draws: 1, draw_calls: 1, missing_meshes: 1, culled: 0, lines: 0, ..Default::default() }
    );

    assert!(r.remove_mesh(CUBE));
    assert!(!r.remove_mesh(CUBE));
    let (image, stats) = render(&gpu, &mut r, &stage1_scene(), 32, 32);
    assert_eq!(
        stats,
        FrameStats { draws: 0, draw_calls: 0, missing_meshes: 1, culled: 0, lines: 0, ..Default::default() }
    );
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
            skin: None,
            own: [0.0; 4],
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
                ..PbrMaterial::DEFAULT
            },
            cast_shadows: true,
            receive_shadows: true,
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
        joints: Vec::new(),
        camera: Some(camera),
        lights: vec![Light::sun(Vec3::NEG_Y, Vec3::ONE)],
        quality: None,
        time: [0.0; 2],
        shader_values: Vec::new(),
        ambient: Vec3::ZERO,
        lines: Vec::new(),
        sky: None,
        fog: None,
        draws: vec![Draw {
            skin: None,
            own: [0.0; 4],
            mesh: CUBE,
            model: Mat4::from_scale(squash) * Mat4::from_quat(turn),
            material: PbrMaterial {
                base_color: [0.5, 0.5, 0.5, 1.0],
                metallic: 0.0,
                roughness: 1.0,
                ..PbrMaterial::DEFAULT
            },
            cast_shadows: true,
            receive_shadows: true,
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

/// Debug lines (stage 4 plan §3.9) are drawn over the scene, through what's
/// in front of them, in their own colour: a red line across the cube and a
/// green one behind it both show on top.
#[test]
fn debug_lines_are_drawn_on_top_of_the_scene() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let mut scene = stage1_scene();
    let red = [1.0, 0.0, 0.0, 1.0];
    let green = [0.0, 1.0, 0.0, 1.0];
    scene.lines = vec![
        Line { a: Vec3::new(-2.0, 0.0, 0.0), b: Vec3::new(2.0, 0.0, 0.0), color: red },
        // Behind the cube, seen through it.
        Line { a: Vec3::new(0.0, -2.0, -3.0), b: Vec3::new(0.0, 2.0, -3.0), color: green },
    ];
    let (image, stats) = render(&gpu, &mut r, &scene, 256, 256);
    assert_eq!(
        stats,
        FrameStats { draws: 1, draw_calls: 1, missing_meshes: 0, culled: 0, lines: 2, ..Default::default() }
    );
    assert_golden("lines", &image);
    // The same at every level.
    for quality in [QualityLevel::Low, QualityLevel::Medium, QualityLevel::High] {
        let scene = RenderScene { quality: Some(quality.quality()), ..scene.clone() };
        assert_golden("lines", &render(&gpu, &mut r, &scene, 256, 256).0);
    }

    // The lines alone, no draws: still drawn.
    scene.draws.clear();
    let (image, stats) = render(&gpu, &mut r, &scene, 256, 256);
    assert_eq!((stats.draws, stats.lines), (0, 2));
    // Each falls on the edge between two rows (or columns) of pixels, so it
    // half covers both.
    let px = |x: usize, y: usize| &image.rgba[(y * 256 + x) * 4..][..3];
    for y in [127, 128] {
        assert!(px(40, y)[0] > 150 && px(40, y)[1] < 60, "red at row {y}: {:?}", px(40, y));
    }
    for x in [127, 128] {
        assert!(px(x, 40)[1] > 150 && px(x, 40)[0] < 60, "green at column {x}: {:?}", px(x, 40));
    }
    assert!(px(40, 126)[0] < 60 && px(40, 129)[0] < 60, "a line is no wider");

    // And a frame after them without any is the scene alone again.
    let (image, stats) = render(&gpu, &mut r, &stage1_scene(), 256, 256);
    assert_eq!(stats.lines, 0);
    assert_golden("cube", &image);
}
