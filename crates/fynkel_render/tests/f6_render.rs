use fynkel_core::{
    Camera, Entity, Extract, ExtractCamera, ExtractPointLight, ExtractRecord, PointLight, WorldId,
};
use fynkel_render::{headless_device, render_offscreen, ExtractRenderer, LightBudget, Quality};
use glam::{Mat4, Vec3};
fn scene(count: usize) -> Extract {
    Extract {
        camera: Some(ExtractCamera {
            record: u32::MAX,
            world: Mat4::from_translation(Vec3::new(0.0, 0.0, 20.0)),
            camera: Camera::default(),
        }),
        records: (0..count)
            .map(|i| ExtractRecord {
                entity: Entity::new(WorldId(1), i as u32 + 1, 1),
                world: Mat4::from_translation(Vec3::new(
                    (i % 100) as f32 * 3.0 - 150.0,
                    (i / 100) as f32 * 3.0 - 150.0,
                    0.0,
                )),
                layer_bit: 1,
            })
            .collect(),
        ..Default::default()
    }
}
#[test]
fn recreation_preserves_settings_and_resize_quality_cycles_release_old_targets() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let mut renderer = ExtractRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64);
    let mut frame = scene(1);
    frame.records[0].world = Mat4::IDENTITY;
    let quality = Quality {
        msaa: 1,
        shadows: false,
        ambient_occlusion: false,
        ..Default::default()
    };
    renderer.set_quality(quality);
    renderer
        .set_light_budget(LightBudget {
            pool: 16,
            per_object: 4,
        })
        .unwrap();
    let expected = render_offscreen(&device, &queue, &mut renderer, &frame);
    let baseline = renderer.stats().gpu_bytes;
    for _ in 0..4 {
        renderer.resize(&device, 96, 48);
        renderer.set_quality(Quality {
            msaa: 4,
            render_scale: 0.5,
            ..quality
        });
        render_offscreen(&device, &queue, &mut renderer, &frame);
        renderer.resize(&device, 64, 64);
        renderer.set_quality(quality);
        assert_eq!(
            render_offscreen(&device, &queue, &mut renderer, &frame),
            expected
        );
        assert_eq!(renderer.stats().gpu_bytes, baseline);
    }
    let (replacement, new_queue) = headless_device().unwrap();
    renderer.recreate(&replacement, wgpu::TextureFormat::Rgba8Unorm);
    assert_eq!(
        renderer.light_budget(),
        LightBudget {
            pool: 16,
            per_object: 4
        }
    );
    assert_eq!(
        render_offscreen(&replacement, &new_queue, &mut renderer, &frame),
        expected
    );
    assert_eq!(renderer.stats().gpu_bytes, baseline);
}
#[test]
fn masked_lights_do_not_exhaust_the_visible_budget_and_overflow_is_explicit() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let mut renderer = ExtractRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 32, 32);
    renderer
        .set_light_budget(LightBudget {
            pool: 4,
            per_object: 2,
        })
        .unwrap();
    let mut frame = scene(1);
    frame.records[0].world = Mat4::IDENTITY;
    frame.camera.as_mut().unwrap().camera.culling_mask = 1;
    for i in 0..1000 {
        frame.point_lights.push(ExtractPointLight {
            record: 0,
            position: Vec3::ZERO,
            light: PointLight {
                range: 100.0,
                culling_mask: if i < 990 { 2 } else { 1 },
                ..Default::default()
            },
        });
    }
    render_offscreen(&device, &queue, &mut renderer, &frame);
    assert_eq!(renderer.stats().lights, 4);
    assert_eq!(renderer.stats().lights_dropped, 6);
    assert_eq!(renderer.stats().lights_max_per_object, 2);
    assert_eq!(renderer.stats().lights_overflow, 1);
    assert_eq!(renderer.lighting_diagnostics().len(), 2);
    assert!(renderer
        .set_light_budget(LightBudget {
            pool: fynkel_render::MAX_LIGHT_POOL + 1,
            per_object: 8
        })
        .is_err());
    assert_eq!(
        renderer.light_budget(),
        LightBudget {
            pool: 4,
            per_object: 2
        }
    );
}
#[test]
#[ignore = "manual F6 rendering qualification"]
fn rendering_qualification() {
    let (device, queue) = headless_device().expect("qualification needs a GPU");
    let mut renderer = ExtractRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64);
    renderer.set_quality(Quality {
        msaa: 1,
        shadows: false,
        ambient_occlusion: false,
        ..Default::default()
    });
    renderer
        .set_light_budget(LightBudget {
            pool: 1024,
            per_object: 8,
        })
        .unwrap();
    let mut frame = scene(10000);
    let mut results = Vec::new();
    for (name, changed, lights) in [
        ("static", 0, 0),
        ("sparse", 1, 0),
        ("dense", 10000, 0),
        ("dense_lights", 0, 256),
        ("all_visible", 0, 0),
        ("all_visible_lights", 0, 1024),
    ] {
        if name.starts_with("all_visible") {
            renderer.resize(&device, 1280, 720);
            frame.camera.as_mut().unwrap().world =
                Mat4::from_translation(Vec3::new(0.0, 0.0, 400.0));
        }
        frame.point_lights = (0..lights)
            .map(|i| ExtractPointLight {
                record: 0,
                position: Vec3::new((i % 16) as f32 - 8.0, (i / 16) as f32 - 8.0, 3.0),
                light: PointLight {
                    range: 20.0,
                    ..Default::default()
                },
            })
            .collect();
        let mut samples = Vec::new();
        for _ in 0..40 {
            for r in &mut frame.records[..changed] {
                r.world.w_axis.x += 0.001;
            }
            render_offscreen(&device, &queue, &mut renderer, &frame);
            let stats = renderer.stats();
            samples.push(serde_json::json!({"prepare_us":stats.prepare_us,"encode_us":stats.encode_us,"gpu_us":stats.gpu_frame_us,"gpu_timed":stats.gpu_timed,"gpu_bytes":stats.gpu_bytes,"instances":stats.instances,"culled":stats.culled,"lights":stats.lights,"lights_dropped":stats.lights_dropped,"lights_overflow":stats.lights_overflow,"visibility_nodes":stats.visibility_nodes,"light_candidates":stats.light_candidates}));
        }
        results.push(serde_json::json!({"mode":name,"records":10000,"samples":samples}));
    }
    assert!(
        renderer.stats().gpu_timestamps && renderer.stats().gpu_timed > 0,
        "GPU timings must actually be available"
    );
    let value = serde_json::json!({"renderer":results,"backend":"Metal release 64x64 culling workloads and 1280x720 all-visible workloads; spatial forward lighting budget 1024/8; GPU timestamp frames lag CPU samples"});
    if let Ok(path) = std::env::var("FYNKEL_F6_RENDER_OUT") {
        std::fs::write(path, value.to_string()).unwrap();
    }
    println!("{value}");
}

#[test]
fn gpu_light_indices_above_255_are_not_truncated() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let mut renderer = ExtractRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 32, 32);
    renderer
        .set_light_budget(LightBudget {
            pool: 512,
            per_object: 8,
        })
        .unwrap();
    let mut frame = scene(1);
    frame.records[0].world = Mat4::IDENTITY;
    let bright = ExtractPointLight {
        record: 0,
        position: Vec3::new(0.0, 0.0, 3.0),
        light: PointLight {
            intensity: 100.0,
            range: 100.0,
            ..Default::default()
        },
    };
    frame.point_lights.push(bright);
    let expected = render_offscreen(&device, &queue, &mut renderer, &frame);
    frame.point_lights.clear();
    for _ in 0..300 {
        frame.point_lights.push(ExtractPointLight {
            record: 0,
            position: Vec3::new(0.0, 0.0, 19.0),
            light: PointLight {
                intensity: 1e-10,
                range: 100.0,
                ..Default::default()
            },
        });
    }
    frame.point_lights.push(bright);
    let actual = render_offscreen(&device, &queue, &mut renderer, &frame);
    assert_eq!(renderer.stats().lights, 301);
    assert_eq!(renderer.stats().lights_dropped, 0);
    assert_eq!(
        actual, expected,
        "the strong light at storage index 300 must shade the object"
    );
}
