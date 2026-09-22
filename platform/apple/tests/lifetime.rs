//! Viewport teardown is leak-safe (M1 gate): Metal devices, IOSurfaces and
//! wgpu resources live outside the Rust allocator, so this watches the
//! process's resident size across repeated create → render → resize →
//! destroy cycles. Growth after warm-up must stay under a small bound.

use std::time::Duration;

use fynkel_core::{Command, Lane, Transform, World, WorldConfig};
use fynkel_platform_apple::{SyncMode, Viewport};

fn resident_bytes() -> u64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<u64>()
        .expect("rss")
        * 1024
}

fn cycle(world: &World) {
    let mut vp = Viewport::new(512, 512, 4, SyncMode::MetalCompletion).expect("viewport");
    for _ in 0..8 {
        let _ = vp.render(&world.extract());
    }
    vp.ring().wait_idle();
    vp.resize(640, 400).expect("resize");
    for _ in 0..8 {
        let _ = vp.render(&world.extract());
    }
    vp.ring().wait_idle();
    // Let completion handlers finish before the ring is dropped.
    std::thread::sleep(Duration::from_millis(5));
}

#[test]
fn viewport_lifetimes_do_not_grow_the_process() {
    let mut world = World::new(WorldConfig::default());
    world.submit(
        Lane::Simulation,
        vec![Command::Spawn {
            temp_id: 1,
            transform: Transform::IDENTITY,
            parent: None,
            name: None,
        }],
    );
    world.begin_frame(1.0 / 60.0);
    world.step();

    // Warm up until the process plateaus: each lifetime builds the
    // renderer's pipelines, and Metal's caches settle over the first few
    // dozen (R3b's three bloom pipelines pushed ten past it — 240
    // lifetimes grow no more than 60, so it is slack, not a leak).
    for _ in 0..30 {
        cycle(&world);
    }
    let baseline = resident_bytes();
    const CYCLES: usize = 60;
    for _ in 0..CYCLES {
        cycle(&world);
    }
    let after = resident_bytes();
    let growth = after as i64 - baseline as i64;
    eprintln!(
        "[leak] viewport: rss {} MiB -> {} MiB after {CYCLES} lifetimes ({growth:+} B)",
        baseline >> 20,
        after >> 20
    );
    // One 512×512 BGRA ring is 4 MiB; a leaked ring per cycle would show as
    // hundreds of MiB. The bound absorbs allocator and Metal heap slack.
    assert!(
        growth < 4 << 20,
        "resident size grew by {} MiB over {CYCLES} viewport lifetimes",
        growth >> 20
    );
}
