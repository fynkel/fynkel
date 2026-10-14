//! Times the renderer (stage 1 step 2).
//!
//! `cargo run --release -p fynkel_render --example bench`
//!
//! For each quality level, screen size and scene size, with the sun alone
//! and with 32 lamps over the cubes as well: the CPU time of `prepare`, and
//! the wall time of a whole frame (prepare, encode, submit, wait for the
//! GPU), best of 50 frames after 10 warm-up frames.

use std::time::{Duration, Instant};

use fynkel_render::glam::{Mat4, Quat, Vec3};
use fynkel_render::*;

fn scene(n: u32, lamps: u32, t: f32) -> RenderScene {
    let side = (n as f32).sqrt().ceil() as u32;
    let spacing = 1.5;
    let extent = side as f32 * spacing;
    let material =
        PbrMaterial { base_color: [0.8, 0.3, 0.1, 1.0], metallic: 0.0, roughness: 0.5, ..PbrMaterial::DEFAULT };
    RenderScene {
        camera: Some(CameraView::looking_at(Vec3::new(0.0, extent * 0.6, extent * 0.9), Vec3::ZERO)),
        lights: std::iter::once(Light::sun(Vec3::new(-0.4, -1.0, -0.3), Vec3::ONE))
            .chain((0..lamps).map(|i| {
                // Spread over the cubes, each reaching a fifth of the way
                // across.
                let angle = i as f32 * 2.39996 + t;
                let r = extent * 0.45 * ((i as f32 + 0.5) / lamps as f32).sqrt();
                let at = Vec3::new(r * angle.cos(), 1.5, r * angle.sin());
                Light::point(at, extent * 0.2 + 2.0, Vec3::new(1.0, 0.8, 0.6) * 4.0)
            }))
            .collect(),
        quality: None,
        ambient: Vec3::splat(0.15),
        lines: Vec::new(),
        draws: (0..n)
            .map(|i| Draw {
                mesh: MeshKey(1),
                model: Mat4::from_rotation_translation(
                    Quat::from_rotation_y(t + i as f32 * 0.1),
                    Vec3::new(
                        ((i % side) as f32 - side as f32 / 2.0) * spacing,
                        (t * 2.0 + i as f32 * 0.15).sin() * 0.5,
                        ((i / side) as f32 - side as f32 / 2.0) * spacing,
                    ),
                ),
                material,
                cast_shadows: true,
                receive_shadows: true,
            })
            .collect(),
    }
}

fn main() {
    let gpu = Gpu::headless().expect("a GPU");
    println!("adapter: {}", gpu.adapter.get_info().name);
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    // 720p, and an iPhone Pro Max's whole screen.
    for (width, height) in [(1280, 720), (2796, 1290)] {
        let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());

        println!("{width}x{height}, 4x MSAA, best of 50:");
        for quality in [Quality::Low, Quality::Medium, Quality::High] {
            let mut renderer = Renderer::new(&gpu, format);
            renderer.upload_mesh(MeshKey(1), &MeshData::cube());
            for (n, lamps) in [(1, 0), (1_000, 0), (10_000, 0), (1, 32), (1_000, 32), (10_000, 32)] {
                let scenes: Vec<RenderScene> = (0..60)
                    .map(|f| RenderScene { quality: Some(quality), ..scene(n, lamps, f as f32 / 60.0) })
                    .collect();
                let (mut prepare, mut frame) = (Duration::MAX, Duration::MAX);
                for (f, s) in scenes.iter().enumerate() {
                    let start = Instant::now();
                    let stats = renderer.prepare(s, width, height);
                    let prepared = start.elapsed();
                    let mut encoder = gpu.device.create_command_encoder(&Default::default());
                    renderer.encode(&mut encoder, &view);
                    gpu.queue.submit([encoder.finish()]);
                    gpu.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                    let total = start.elapsed();
                    assert_eq!(stats.draw_calls, 1);
                    if f >= 10 {
                        prepare = prepare.min(prepared);
                        frame = frame.min(total);
                    }
                }
                println!(
                    "  {quality:?} {n:>6} cubes, {lamps:>2} lamps: prepare {:>7.3} ms, whole frame {:>7.3} ms",
                    prepare.as_secs_f64() * 1e3,
                    frame.as_secs_f64() * 1e3
                );
            }
        }
    }
}
