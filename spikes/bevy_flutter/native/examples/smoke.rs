//! Headless check: Bevy renders into the ring, frames complete, and the
//! presented IOSurface holds a picture (saved as smoke.ppm).
//!
//!   cargo run --release --example smoke -- <assets dir>

use std::ffi::CString;
use std::time::{Duration, Instant};

use bevy_flutter_spike::*;
use objc2_io_surface::{IOSurfaceLockOptions, IOSurfaceRef};

fn main() {
    let assets = std::env::args().nth(1).expect("assets dir");
    let assets = CString::new(assets).unwrap();
    let (w, h) = (960u32, 640u32);
    let cubes = 1000u32;
    let t0 = Instant::now();
    let ctx = bf_create(w, h, assets.as_ptr(), cubes);
    assert!(!ctx.is_null(), "bf_create failed");
    println!("create: {:?}, surfaces {}", t0.elapsed(), bf_surface_count(ctx));

    let mut transforms = vec![0f32; cubes as usize * 4];
    let start = Instant::now();
    let mut presented = -1;
    for frame in 0..600 {
        let t = start.elapsed().as_secs_f32();
        fill(&mut transforms, t);
        bf_frame(ctx, transforms.as_ptr(), cubes);
        std::thread::sleep(Duration::from_micros(8333));
        let p = bf_acquire(ctx as *mut _) & 0xff;
        if p >= 0 {
            presented = p;
        }
        if frame % 120 == 119 {
            let mut s = [0f64; 8];
            bf_stats(ctx, s.as_mut_ptr(), 8);
            println!(
                "frames {} dropped {} coalesced {} presented {} update p50 {:.2} p95 {:.2} gpu p50 {:.2} p95 {:.2}",
                s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]
            );
        }
    }
    assert!(presented >= 0, "no frame was ever presented");
    let surface = bf_surface(ctx, presented as u32) as *const IOSurfaceRef;
    let surface = unsafe { &*surface };
    unsafe { surface.lock(IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut()) };
    let (sw, sh, stride) = (surface.width(), surface.height(), surface.bytes_per_row());
    let base = surface.base_address().as_ptr() as *const u8;
    let mut ppm = format!("P6 {sw} {sh} 255\n").into_bytes();
    let mut nonblack = 0usize;
    for y in 0..sh {
        for x in 0..sw {
            let p = unsafe { std::slice::from_raw_parts(base.add(y * stride + x * 4), 4) };
            ppm.extend_from_slice(&[p[2], p[1], p[0]]);
            if p[0] as u32 + p[1] as u32 + p[2] as u32 > 30 {
                nonblack += 1;
            }
        }
    }
    unsafe { surface.unlock(IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut()) };
    std::fs::write("smoke.ppm", ppm).unwrap();
    println!("saved smoke.ppm ({sw}x{sh}), {:.1}% non-black", nonblack as f64 * 100.0 / (sw * sh) as f64);

    let t1 = Instant::now();
    assert_eq!(bf_resize(ctx, 1280, 720), 0);
    println!("resize: {:?}", t1.elapsed());
    for _ in 0..30 {
        bf_frame(ctx, transforms.as_ptr(), cubes);
        std::thread::sleep(Duration::from_millis(8));
        bf_acquire(ctx as *mut _);
    }
    bf_destroy(ctx);
    println!("destroyed");
}

/// The wave the Dart host computes: a 40-wide grid behind the character.
fn fill(out: &mut [f32], t: f32) {
    for i in 0..out.len() / 4 {
        let (col, row) = ((i % 40) as f32, (i / 40) as f32);
        out[i * 4] = (col - 19.5) * 0.9;
        out[i * 4 + 1] = 0.6 + 0.5 * (t * 2.0 + i as f32 * 0.15).sin();
        out[i * 4 + 2] = (row - 12.0) * 0.9 - 6.0;
        out[i * 4 + 3] = t + i as f32 * 0.01;
    }
}
