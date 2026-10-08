//! Removed textures give their GPU memory back (stage 2 plan §3.2).
//!
//! Measured as the process's own GPU memory (`footprint`), so this is its
//! own test binary: nothing else in the process may allocate on the GPU.
#![cfg(target_os = "macos")]

mod common;

use std::time::{Duration, Instant};

use common::*;
use fynkel_render::*;

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

/// Waits up to `seconds` for `done`, checking every 100 ms; returns the
/// last measurement.
fn settle(seconds: f64, done: impl Fn(f64) -> bool) -> f64 {
    let start = Instant::now();
    loop {
        let mb = gpu_dirty_mb();
        if done(mb) || start.elapsed().as_secs_f64() > seconds {
            return mb;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn removed_textures_give_their_gpu_memory_back() {
    let gpu = gpu();
    let mut r = Renderer::new(&gpu, wgpu::TextureFormat::Rgba8UnormSrgb);
    // Device creation and the renderer's own 1×1 textures copy once; let
    // the driver settle first.
    std::thread::sleep(Duration::from_secs(3));
    let before = gpu_dirty_mb();

    // Ten 2048² textures: 10 × 22.4 MB with their mipmaps.
    let pixels = vec![200; 2048 * 2048 * 4];
    for key in 0..10 {
        r.upload_texture(TextureKey(key), ColorSpace::Srgb, 2048, 2048, &pixels);
    }
    gpu.device.poll(wgpu::PollType::wait_indefinitely()).expect("the uploads finish");
    let expected = r.texture_bytes() as f64 / (1 << 20) as f64;
    let loading = gpu_dirty_mb();
    // Uploading also uses staging copies and the driver's copy pool, which
    // go a moment after the copies stop; then what's left is the textures.
    let loaded = settle(10.0, |mb| mb < before + expected * 1.1);

    for key in 0..10 {
        assert!(r.remove_texture(TextureKey(key)));
    }
    gpu.device.poll(wgpu::PollType::wait_indefinitely()).expect("the GPU is idle");
    assert_eq!(r.texture_bytes(), 0);
    // The driver frees its copy pool a moment after the copies stop.
    let after = settle(10.0, |mb| mb < before + 16.0);

    eprintln!(
        "dirty GPU memory: before {before:.1} MB, while loading {loading:.1} MB, then with the textures {loaded:.1} MB \
         (they hold {expected:.1} MB), after removing them {after:.1} MB"
    );
    let held = loaded - before;
    assert!(
        (held - expected).abs() < expected * 0.1,
        "the textures hold {held:.1} MB of GPU memory once loaded; texture_bytes says {expected:.1} MB"
    );
    assert!(after < before + 16.0, "removing the textures left {:.1} MB of GPU memory behind", after - before);
}
