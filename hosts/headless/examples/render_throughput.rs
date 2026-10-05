//! Offscreen render capacity of a cooked scene: no window, no display sync, no
//! readback. Frames are submitted back to back with a few in flight, so the
//! result is what the CPU (extract + prepare + encode) and the GPU can sustain
//! together, which a vsynced window can never show on macOS.
//!
//! cargo run --release -p fynkel_host_headless --example render_throughput -- CACHE \
//!     [--scene UUID] [--size 2400x1600] [--quality NAME] [--ticks 120] [--frames 600] \
//!     [--live] [--inflight 3]
//!
//! `--live` steps the world every other frame (a 120 Hz loop over 60 Hz ticks)
//! and re-extracts each frame, so the sim and extract costs are inside the
//! number; without it the extract is static and only prepare/draw/GPU count.
use fynkel_assets::Assets;
use fynkel_core::{World, WorldConfig};
use fynkel_render::{headless_device, ExtractRenderer};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

fn pct(v: &mut [f64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() - 1) as f64 * p).round() as usize]
}

fn main() {
    let mut args = std::env::args().skip(1);
    let cache = args.next().expect("cooked cache path");
    let (mut scene, mut quality) = (None, None);
    let (mut width, mut height) = (2400u32, 1600u32);
    let (mut ticks, mut frames, mut inflight, mut live) = (120u32, 600u32, 3u64, false);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--scene" => scene = args.next(),
            "--quality" => quality = args.next(),
            "--size" => {
                let v = args.next().unwrap();
                let (w, h) = v.split_once('x').expect("WxH");
                width = w.parse().unwrap();
                height = h.parse().unwrap();
            }
            "--ticks" => ticks = args.next().unwrap().parse().unwrap(),
            "--frames" => frames = args.next().unwrap().parse().unwrap(),
            "--inflight" => inflight = args.next().unwrap().parse().unwrap(),
            "--live" => live = true,
            other => panic!("unknown argument {other}"),
        }
    }
    let assets = Assets::open(std::path::Path::new(&cache)).expect("open cache");
    let scene_id = match scene {
        Some(s) => s.parse().expect("scene uuid"),
        None => assets.startup_scene().expect("startup scene"),
    };
    let cooked = assets.scene(scene_id).expect("scene");
    let _resident = assets.retain(&cooked.closure);
    let mut world = World::new(WorldConfig::default());
    if let Some(project) = assets.project() {
        world
            .physics_mut()
            .set_gravity(project.physics.gravity.into());
    }
    let (_, d) = assets.register_mesh_colliders(&cooked.description, world.physics_mut());
    assert!(d.is_empty(), "{d:?}");
    let (_, d) = assets.register_prefabs(&cooked.description, &mut world);
    assert!(d.is_empty(), "{d:?}");
    let (_, _, d) = assets.register_animation(&cooked.description, &mut world);
    assert!(d.is_empty(), "{d:?}");
    let (_, d) = assets.register_audio(&cooked.description, &mut world);
    assert!(d.is_empty(), "{d:?}");
    world.load_scene(&cooked.description).expect("load scene");
    for _ in 0..ticks {
        world.begin_frame(1.0 / 60.0);
        world.step();
    }
    let (device, queue) = headless_device().expect("no GPU adapter");
    let mut renderer = ExtractRenderer::new(&device, wgpu::TextureFormat::Bgra8Unorm, width, height);
    renderer.set_assets(Some(assets.clone()));
    if !renderer.set_quality_profile(quality.as_deref()) {
        panic!("unknown quality profile; have {:?}", renderer.quality_profiles());
    }
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("throughput target"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let mut extract = world.extract();
    // Settle assets and pipelines before timing.
    renderer
        .prepare_ready(&device, &queue, &extract, std::time::Duration::from_secs(30))
        .expect("assets");
    for _ in 0..8 {
        let mut e = device.create_command_encoder(&Default::default());
        renderer.draw(&mut e, &view);
        queue.submit(Some(e.finish()));
    }
    let _ = device.poll(wgpu::PollType::wait_indefinitely());

    let (mut phase_us, mut vis_us) = ([0.0f64; 9], [0.0f64; 3]);
    let mut system_us: Vec<f64> = Vec::new();
    let mut system_ticks = 0u32;
    let done = Arc::new(AtomicU64::new(0));
    let mut submitted = 0u64;
    let (mut sim_us, mut extract_us, mut prepare_us, mut encode_us, mut gpu_us) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (mut ao, mut scene_pass, mut post, mut shadow) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut last_timed = renderer.stats().gpu_timed;
    let start = Instant::now();
    for frame in 0..frames {
        if live {
            if frame % 2 == 0 {
                let t = Instant::now();
                world.begin_frame(1.0 / 60.0);
                world.step();
                sim_us.push(t.elapsed().as_secs_f64() * 1e6);
                let times = world.system_times();
                if system_us.is_empty() {
                    system_us = vec![0.0; times.len()];
                }
                for (acc, t) in system_us.iter_mut().zip(times) {
                    *acc += f64::from(t.micros);
                }
                system_ticks += 1;
            }
            let t = Instant::now();
            world.extract_into(&mut extract, false);
            extract_us.push(t.elapsed().as_secs_f64() * 1e6);
        }
        let t = Instant::now();
        renderer.prepare(&device, &queue, &extract);
        prepare_us.push(t.elapsed().as_secs_f64() * 1e6);
        let st = renderer.stats();
        for (acc, v) in phase_us.iter_mut().zip(st.prepare_phases_us) {
            *acc += f64::from(v);
        }
        for (acc, v) in vis_us.iter_mut().zip(st.visibility_phases_us) {
            *acc += f64::from(v);
        }
        let t = Instant::now();
        let mut e = device.create_command_encoder(&Default::default());
        renderer.draw(&mut e, &view);
        queue.submit(Some(e.finish()));
        encode_us.push(t.elapsed().as_secs_f64() * 1e6);
        submitted += 1;
        let d = done.clone();
        queue.on_submitted_work_done(move || {
            d.fetch_add(1, Ordering::Release);
        });
        while submitted - done.load(Ordering::Acquire) > inflight {
            let _ = device.poll(wgpu::PollType::Poll);
            std::thread::yield_now();
        }
        let s = renderer.stats();
        if s.gpu_timed != last_timed {
            last_timed = s.gpu_timed;
            gpu_us.push(f64::from(s.gpu_frame_us));
            ao.push(f64::from(s.gpu_ao_us));
            scene_pass.push(f64::from(s.gpu_scene_us));
            post.push(f64::from(s.gpu_post_us));
            shadow.push(f64::from(s.gpu_shadow_us));
        }
    }
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    let wall = start.elapsed().as_secs_f64();
    let s = renderer.stats();
    let ms = |v: &mut Vec<f64>, p: f64| pct(v, p) / 1000.0;
    println!(
        "{width}x{height} {} instances {} draws {} | {:.1} fps pipelined ({} frames, {:.2} s, {} in flight, {})",
        s.instances,
        s.draws,
        s.shadow_casters,
        f64::from(frames) / wall,
        frames,
        wall,
        inflight,
        if live { "live sim+extract" } else { "static extract" },
    );
    if live {
        println!(
            "  sim (tick frames only) {:.2}/{:.2}  extract {:.2}/{:.2}  (ms p50/p95)",
            ms(&mut sim_us, 0.5),
            ms(&mut sim_us, 0.95),
            ms(&mut extract_us, 0.5),
            ms(&mut extract_us, 0.95)
        );
    }
    if live {
        let line: Vec<String> = world
            .system_times()
            .iter()
            .zip(&system_us)
            .filter(|(_, us)| **us / f64::from(system_ticks) >= 20.0)
            .map(|(t, us)| format!("{}={:.2}({})", t.system.name(), us / f64::from(system_ticks) / 1000.0, t.entities))
            .collect();
        println!("  systems ms/tick: {}  physics sync candidates (last tick): {}", line.join(" "), world.physics().sync_candidates());
    }
    println!(
        "  prepare {:.2}/{:.2}  encode+submit {:.2}/{:.2}  (ms p50/p95)",
        ms(&mut prepare_us, 0.5),
        ms(&mut prepare_us, 0.95),
        ms(&mut encode_us, 0.5),
        ms(&mut encode_us, 0.95)
    );
    let n = f64::from(frames) * 1000.0;
    println!(
        "  prepare phases ms (setup, collect, visibility, lights, instances, batches/uploads, shadow/post, overlay, accounting): {}  | visibility (bounds, index, queries): {}",
        phase_us.iter().map(|v| format!("{:.2}", v / n)).collect::<Vec<_>>().join(" "),
        vis_us.iter().map(|v| format!("{:.2}", v / n)).collect::<Vec<_>>().join(" ")
    );
    println!(
        "  gpu frame {:.2}/{:.2}  shadow {:.2}  ao {:.2}  scene {:.2}  post {:.2}  (ms p50/p95; spans overlap)  timed={}",
        ms(&mut gpu_us, 0.5),
        ms(&mut gpu_us, 0.95),
        ms(&mut shadow, 0.5),
        ms(&mut ao, 0.5),
        ms(&mut scene_pass, 0.5),
        ms(&mut post, 0.5),
        gpu_us.len()
    );
}
