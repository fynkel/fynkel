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
