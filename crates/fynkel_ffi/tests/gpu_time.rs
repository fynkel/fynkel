//! The GPU's time for each part of a view's frames (stage 14 phase 4), for
//! measuring: off till `fy_gpu_timing_set` turns it on for every view. Its
//! own test binary, since the switch is the process's.
#![allow(unsafe_code)]

mod common;

use common::*;
use fynkel_ffi::*;

fn stats(view: &View) -> FyViewStats {
    let mut stats = FyViewStats::default();
    // SAFETY: a live view and a pointer to a local.
    ok(unsafe { fy_view_stats(view.0, &mut stats) });
    stats
}

/// Draws and reads back frames till `frames` are timed, or 30 are drawn.
fn draw(view: &View, frames: u64) -> FyViewStats {
    for _ in 0..30 {
        view.read(view.frame(), 64, 64);
        if stats(view).gpu_frames >= frames {
            break;
        }
    }
    stats(view)
}

#[test]
fn a_views_frames_are_timed_on_the_gpu_when_asked() {
    let world = World::new();
    stage1(&world);
    let view = View::headless(&world, 64, 64);
    let off = draw(&view, 1);
    assert_eq!((off.gpu_frames, off.gpu_scene_micros), (0, 0), "nothing timed unless asked");

    fy_gpu_timing_set(true);
    let on = draw(&view, 3);
    fy_gpu_timing_set(false);
    if cfg!(target_os = "macos") {
        assert!(on.gpu_frames >= 3, "a Mac's GPU times its frames: {on:?}");
    }
    if on.gpu_frames > 0 {
        assert!(on.gpu_scene_micros + on.gpu_finish_micros > 0, "the scene took some time: {on:?}");
    }
}
