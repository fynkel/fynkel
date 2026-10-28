//! The GPU's time for each part of a frame (stage 14 phase 4): with timing
//! on, each pass is timed on the GPU and the times add up per part; with
//! it off (the default) nothing is timed, and nothing is copied back.

mod common;

use common::*;
use fynkel_render::glam::Vec3;
use fynkel_render::*;

const SRGB: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// The stage 1 cube at high, its sun casting shadows, with bloom and
/// ambient occlusion.
fn scene() -> RenderScene {
    let mut scene = stage1_scene();
    scene.lights[0] = scene.lights[0].with_shadows(true);
    let mut camera = CameraView::looking_at(Vec3::new(0.0, 2.0, 5.0), Vec3::ZERO);
    camera.bloom = Some(Bloom::DEFAULT);
    camera.ambient_occlusion = Some(AmbientOcclusion::default());
    scene.camera = Some(camera);
    scene.quality = Some(QualityLevel::High.quality());
    scene
}

/// Draws up to 30 frames, waiting for the GPU after each, till `frames`
/// of them are timed.
fn draw(gpu: &Gpu, r: &mut Renderer, frames: u64) -> GpuTimes {
    let scene = scene();
    let mut times = GpuTimes::default();
    for _ in 0..30 {
        render(gpu, r, &scene, 128, 128);
        gpu.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        times = r.gpu_times();
        if times.frames >= frames {
            break;
        }
    }
    times
}

#[test]
fn each_part_of_a_frame_is_timed_on_the_gpu() {
    let gpu = gpu();
    // Every Apple GPU since the A11 and M1 times passes; the Mac CI runs
    // on must.
    let timed = gpu.device.features().contains(wgpu::Features::TIMESTAMP_QUERY);
    assert!(timed || !cfg!(target_os = "macos"), "a Mac's GPU times its passes");
    if !timed {
        return;
    }
    let mut r = renderer(&gpu, SRGB);
    r.set_gpu_timing(true);
    let times = draw(&gpu, &mut r, 3);
    assert!(times.frames >= 3, "frames are timed: {times:?}");
    for (part, nanos) in [
        ("shadows", times.shadows),
        ("ambient occlusion", times.ambient_occlusion),
        ("scene", times.scene),
        ("bloom", times.bloom),
        ("finish", times.finish),
    ] {
        assert!(nanos > 0, "{part} took some time: {times:?}");
    }
    assert_eq!(times.effects, 0, "no screen effects, no time for them");
    // Now and then the GPU leaves a clock unread, but not in every frame.
    assert!(times.untimed < times.frames, "the passes' clocks are read: {times:?}");
    // The times only grow: they're counted from the start.
    let later = draw(&gpu, &mut r, times.frames + 2);
    assert!(later.frames > times.frames && later.scene > times.scene, "{later:?}");
}

#[test]
fn nothing_is_timed_unless_asked() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let scene = scene();
    for _ in 0..5 {
        render(&gpu, &mut r, &scene, 128, 128);
        gpu.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    }
    assert_eq!(r.gpu_times(), GpuTimes::default());
    // Turned on and off again: what was timed stays, nothing more is.
    r.set_gpu_timing(true);
    let on = draw(&gpu, &mut r, 2);
    r.set_gpu_timing(false);
    let off = draw(&gpu, &mut r, u64::MAX);
    assert!(off.frames <= on.frames + 2, "at most the frames already sent are counted: {on:?} {off:?}");
}

#[test]
fn frames_sent_one_after_another_keep_their_own_times() {
    // A frame's clocks are its own, not the next frame's: frames sent
    // without waiting run on the GPU while the one before is copied back,
    // and can't add up to more GPU time than the time they took.
    let gpu = gpu();
    if !gpu.device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
        return;
    }
    let mut r = renderer(&gpu, SRGB);
    r.set_gpu_timing(true);
    let scene = scene();
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: 1024, height: 1024, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: SRGB,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    // Warmed up first: pipelines made, images allocated.
    for _ in 0..5 {
        r.render(&scene, &view, 1024, 1024);
    }
    gpu.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let before = r.gpu_times();
    let start = std::time::Instant::now();
    for _ in 0..120 {
        r.render(&scene, &view, 1024, 1024);
    }
    gpu.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let took = start.elapsed().as_nanos() as u64;
    // The last frames' clocks come back as more are drawn.
    let mut times = r.gpu_times();
    for _ in 0..3 {
        r.render(&scene, &view, 1024, 1024);
        gpu.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        times = r.gpu_times();
    }
    let frames = times.frames - before.frames;
    assert!(frames >= 60, "most frames are timed: {frames}");
    for (part, nanos) in [
        ("shadows", times.shadows - before.shadows),
        ("ambient occlusion", times.ambient_occlusion - before.ambient_occlusion),
        ("scene", times.scene - before.scene),
        ("bloom", times.bloom - before.bloom),
        ("finish", times.finish - before.finish),
    ] {
        let (each, frame) = (nanos / frames, took / 120);
        assert!(each > 0 && each < frame, "{part}: {each} ns a frame, of {frame} ns a frame in all");
    }
}
