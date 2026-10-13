//! Losing the GPU device every view shares. In a file of its own: the
//! test process's views all lose it.
#![allow(unsafe_code)]

mod common;

use std::time::{Duration, Instant};

use common::*;
use fynkel_ffi::*;
use fynkel_golden::assert_golden;

/// Frames until one fails with the device lost; panics after 10 s.
fn until_lost(view: &View) {
    let start = Instant::now();
    loop {
        let mut number = 0;
        // SAFETY: a live view and a local out-value.
        match unsafe { fy_view_frame(view.0, &mut number) } {
            FyStatus::Ok => {}
            FyStatus::DeviceLost => return,
            other => panic!("{other:?}: {}", last_error()),
        }
        assert!(start.elapsed() < Duration::from_secs(10), "the device was never lost");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn losing_the_device_stops_every_view_and_a_new_view_starts_over() {
    let (world, other) = (World::new(), World::new());
    stage1(&world);
    stage1(&other);
    let a = View::headless(&world, 256, 256);
    let b = View::headless(&world, 256, 256);
    let c = View::headless(&other, 256, 256);
    for view in [&a, &b, &c] {
        view.read(view.frame(), 256, 256);
    }
    // SAFETY: a live view.
    ok(unsafe { fy_view_lose_device(a.0) });
    for view in [&a, &b, &c] {
        until_lost(view);
    }
    // New views get a new device, and upload each world's meshes again.
    let d = View::headless(&world, 256, 256);
    assert_golden("cube", &d.read(d.frame(), 256, 256));
    let e = View::headless(&other, 256, 256);
    assert_golden("cube", &e.read(e.frame(), 256, 256));
}
