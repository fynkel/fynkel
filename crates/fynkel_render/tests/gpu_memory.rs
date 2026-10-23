//! Steady frames don't blit (docs/stage-1.md §6 q6). On Apple GPUs a
//! process that runs a blit (a GPU copy or fill) every frame keeps a pool of
//! driver memory, 128 MB on an M3 Max, dirty for as long as it does; it is
//! freed a moment after the blits stop. `queue.write_buffer` copies with a
//! blit, so the renderer writes per-frame data where the GPU reads it
//! instead.
//!
//! Measured as the process's own GPU memory (`footprint`), so this is its
//! own test binary: nothing else in the process may draw or copy. It
//! compares against the same frames plus one small `write_buffer` each,
//! so it holds on any Apple GPU, whatever its pools weigh.
#![cfg(target_os = "macos")]

mod common;

use std::time::{Duration, Instant};

use common::*;

/// The process's dirty GPU memory, in MB: footprint's "Owned physical
/// footprint" rows, which hold what Metal's driver allocated for it.
fn gpu_dirty_mb() -> f64 {
    let out = std::process::Command::new("footprint")
        .args(["-p", &std::process::id().to_string(), "-f", "bytes"])
        .output()
        .expect("footprint runs");
    let text = String::from_utf8_lossy(&out.stdout);
    let bytes: u64 = text
        .lines()
        .filter(|line| line.contains("Owned physical footprint"))
        .filter_map(|line| line.split_whitespace().next()?.parse::<u64>().ok())
        .sum();
    bytes as f64 / (1 << 20) as f64
}

/// Draws the stage 1 cube at about 120 frames a second for `seconds`,
/// calling `extra` after each frame, and returns the least dirty GPU
/// memory it read, once a second from the first second on.
fn steady(seconds: f64, mut extra: impl FnMut()) -> f64 {
    let gpu = gpu();
    let mut r = renderer(&gpu, wgpu::TextureFormat::Bgra8UnormSrgb);
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: 256, height: 256, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let mut scene = stage1_scene();
    let (start, mut lowest, mut read) = (Instant::now(), f64::MAX, 1.0);
    while start.elapsed().as_secs_f64() < seconds {
        if start.elapsed().as_secs_f64() >= read {
            lowest = lowest.min(gpu_dirty_mb());
            read += 1.0;
        }
        scene.draws[0].model = fynkel_render::glam::Mat4::from_rotation_y(start.elapsed().as_secs_f32());
        r.render(&scene, &view, 256, 256);
        extra();
        gpu.device.poll(wgpu::PollType::wait_indefinitely()).expect("the frame finishes");
        std::thread::sleep(Duration::from_millis(8));
    }
    lowest.min(gpu_dirty_mb())
}

#[test]
fn steady_frames_hold_no_blit_memory() {
    // Device creation blits once (wgpu zeroes a buffer of its own), and
    // the driver frees that pool a few seconds later: a second after it
    // on an idle Mac, longer on a busy one. The lowest reading of 10 s is
    // after it.
    let ours = steady(10.0, || {});

    let gpu = gpu();
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 256,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let with_blits = steady(1.0, || {
        gpu.queue.write_buffer(&buffer, 0, &[1; 256]);
        gpu.queue.submit([]);
    });

    eprintln!("dirty GPU memory: steady frames {ours:.1} MB, with a write_buffer each {with_blits:.1} MB");
    assert!(
        ours + 64.0 < with_blits,
        "steady frames hold {ours:.1} MB of GPU memory, not clearly less than the {with_blits:.1} MB \
         they hold with a blit each: something in the frame blits",
    );
}
