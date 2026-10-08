//! The Android view, through the C interface. A `Surface` needs a JVM, so
//! only the view without one is checked here (tools/test-mobile.sh runs it
//! in the emulator); examples/hello checks the rest on screen.
#![cfg(target_os = "android")]
#![allow(unsafe_code)]

mod common;

use std::time::{Duration, Instant};

use common::*;
use fynkel_ffi::*;

fn stats(view: *mut FyView) -> FyViewStats {
    let mut stats = FyViewStats::default();
    // SAFETY: a live view and a local out-value.
    ok(unsafe { fy_view_stats(view, &mut stats) });
    stats
}

#[test]
fn a_view_with_no_surface_skips_its_frames() {
    let world = World::new();
    stage1(&world);
    let mut view = std::ptr::null_mut();
    // SAFETY: a live world and a local out-value.
    ok(unsafe { fy_view_new_android(world.0, 64, 64, &mut view) });
    for _ in 0..20 {
        let mut number = 0;
        // SAFETY: a live view and a local out-value.
        ok(unsafe { fy_view_frame(view, &mut number) });
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut done = stats(view);
    while done.rendered + done.coalesced + done.skipped < 20 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
        done = stats(view);
    }
    assert_eq!(done.posted, 20);
    assert_eq!(done.rendered, 0, "nothing to draw into");
    assert_eq!(done.skipped + done.coalesced, 20);
    assert_eq!((done.presented, done.dropped), (0, 0));
    // A resize changes nothing on Android, and is no error.
    // SAFETY: a live view.
    ok(unsafe { fy_view_resize(view, 32, 32) });
    // SAFETY: made above, freed once.
    unsafe { fy_view_free(view) };
}

/// The process's PSS plus swapped PSS, in bytes.
fn pss() -> u64 {
    let rollup = std::fs::read_to_string("/proc/self/smaps_rollup").unwrap();
    let kb = |key: &str| -> u64 {
        let line = rollup.lines().find(|l| l.starts_with(key)).unwrap();
        line.split_whitespace().nth(1).unwrap().parse::<u64>().unwrap() * 1024
    };
    kb("Pss:") + kb("SwapPss:")
}

/// The process's anonymous memory (heap and driver allocations, without
/// the file pages PSS also counts), swapped or not, in bytes.
fn anonymous() -> u64 {
    let rollup = std::fs::read_to_string("/proc/self/smaps_rollup").unwrap();
    let kb = |key: &str| -> u64 {
        let line = rollup.lines().find(|l| l.starts_with(key)).unwrap();
        line.split_whitespace().nth(1).unwrap().parse::<u64>().unwrap() * 1024
    };
    kb("Anonymous:") + kb("Swap:")
}

/// A view made and freed, after drawing (or skipping) one frame.
fn view_made_and_freed(world: &World, headless: bool) {
    let mut view = std::ptr::null_mut();
    // SAFETY: a live world and a local out-value.
    ok(unsafe {
        if headless {
            fy_view_new_headless(world.0, 64, 64, &mut view)
        } else {
            fy_view_new_android(world.0, 64, 64, &mut view)
        }
    });
    let mut number = 0;
    // SAFETY: a live view and a local out-value.
    ok(unsafe { fy_view_frame(view, &mut number) });
    // SAFETY: made above, freed once.
    unsafe { fy_view_free(view) };
}

/// The emulator's Vulkan driver keeps 4 MB for every thread that ever
/// called it, even after the thread ends; a view made after another reuses
/// its render thread, so views come and go for free.
///
/// What's allowed is 50 KB a view. The first views fill pools that later
/// ones reuse, and a phone's allocator may keep a freed 1 MB block or give
/// it back (`probe_memory_over_many_views`), so the 40 views measured come
/// after 20 that aren't. The emulator's driver keeps about 3 KB of every
/// device made, ours or not (`probe_memory_over_many_view_parts`).
#[test]
fn views_made_and_freed_again_leave_nothing_behind() {
    let world = World::new();
    stage1(&world);
    for headless in [false, true] {
        for _ in 0..20 {
            view_made_and_freed(&world, headless);
        }
        let start = pss();
        for _ in 0..40 {
            view_made_and_freed(&world, headless);
        }
        let grown = pss().saturating_sub(start);
        assert!(grown < 2 << 20, "40 views (headless: {headless}) left {} KB behind", grown >> 10);
    }
}

/// Probe, run by hand on a new device (`--ignored --nocapture probe`):
/// what its Vulkan driver keeps of a thread that called it, once the thread
/// has ended. The emulator's keeps 4.0 MB in every case.
#[test]
#[ignore = "a measurement, run by hand"]
fn probe_what_the_driver_keeps_of_an_ended_thread() {
    let gpu = fynkel_render::Gpu::headless().unwrap();
    for kind in ["only allocates", "makes an instance", "submits to a device made elsewhere"] {
        let start = pss();
        for _ in 0..10 {
            let gpu = gpu.clone();
            std::thread::spawn(move || match kind {
                "only allocates" => drop(std::hint::black_box(vec![1u8; 1 << 20])),
                "makes an instance" => {
                    drop(fynkel_render::wgpu::Instance::new(fynkel_render::wgpu::InstanceDescriptor {
                        backends: fynkel_render::wgpu::Backends::VULKAN,
                        ..fynkel_render::wgpu::InstanceDescriptor::new_without_display_handle()
                    }))
                }
                _ => {
                    let encoder = gpu.device.create_command_encoder(&Default::default());
                    gpu.queue.submit([encoder.finish()]);
                    gpu.device.poll(fynkel_render::wgpu::PollType::wait_indefinitely()).unwrap();
                }
            })
            .join()
            .unwrap();
        }
        println!("a thread that {kind}: {:.2} MB kept", (pss() - start) as f64 / 10.0 / (1 << 20) as f64);
    }
}

/// Probe, run by hand on a new device (`--ignored --nocapture probe`): the
/// memory after each batch of 20 views. A leak grows with every batch; a
/// pool the driver or allocator fills once levels off.
#[test]
#[ignore = "a measurement, run by hand"]
fn probe_memory_over_many_views() {
    let world = World::new();
    stage1(&world);
    for headless in [false, true] {
        view_made_and_freed(&world, headless);
        let start = anonymous();
        let mut line = format!("headless: {headless:5}, anonymous MB after each 20 views:");
        for _ in 0..20 {
            for _ in 0..20 {
                view_made_and_freed(&world, headless);
            }
            line += &format!(" {:+.2}", (anonymous() as f64 - start as f64) / (1 << 20) as f64);
        }
        println!("{line}");
    }
}

/// Probe, with [`probe_memory_over_many_views`]: the same batches for the
/// parts of a view, to find whose memory grows.
#[test]
#[ignore = "a measurement, run by hand"]
fn probe_memory_over_many_view_parts() {
    use fynkel_render::{Gpu, Renderer, wgpu};
    let parts: [(&str, fn()); 3] = [
        ("an instance", || {
            drop(wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::VULKAN,
                flags: fynkel_render::instance_flags(),
                ..wgpu::InstanceDescriptor::new_without_display_handle()
            }))
        }),
        ("a device", || drop(Gpu::headless().unwrap())),
        ("a device and a renderer", || {
            let gpu = Gpu::headless().unwrap();
            drop(Renderer::new(&gpu, wgpu::TextureFormat::Rgba8UnormSrgb));
        }),
    ];
    for (name, make) in parts {
        make();
        let start = pss();
        let mut line = format!("{name}, MB after each 20:");
        for _ in 0..10 {
            for _ in 0..20 {
                make();
            }
            line += &format!(" {:+.2}", (pss() as f64 - start as f64) / (1 << 20) as f64);
        }
        println!("{line}");
    }
}
