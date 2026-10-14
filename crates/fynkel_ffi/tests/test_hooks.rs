//! The test hooks: refused outside a test process, and a render worker
//! that panics. In a file of its own: it sets the process's environment.
#![allow(unsafe_code)]

mod common;

use std::time::{Duration, Instant};

use common::*;
use fynkel_ffi::*;
use fynkel_golden::assert_golden;

#[test]
fn the_hooks_run_only_in_tests_and_a_panicked_worker_fails_its_view_alone() {
    let world = World::new();
    stage1(&world);
    let view = View::headless(&world, 256, 256);
    let other = View::headless(&world, 256, 256);
    view.read(view.frame(), 256, 256);

    // A game's process: neither variable set.
    // SAFETY: a live view.
    assert_eq!(unsafe { fy_view_fail_worker(view.0) }, FyStatus::Unsupported);
    assert!(last_error().contains("only in tests"), "{}", last_error());
    // SAFETY: a live view.
    assert_eq!(unsafe { fy_view_lose_device(view.0) }, FyStatus::Unsupported);

    // SAFETY: the only test in this process, so nothing reads the
    // environment meanwhile.
    unsafe { std::env::set_var("FYNKEL_TEST_HOOKS", "1") };
    // SAFETY: a live view.
    ok(unsafe { fy_view_fail_worker(view.0) });
    let start = Instant::now();
    loop {
        let mut number = 0;
        // SAFETY: a live view and a local out-value.
        match unsafe { fy_view_frame(view.0, &mut number) } {
            FyStatus::Ok => {}
            FyStatus::WorkerFailed => break,
            other => panic!("{other:?}: {}", last_error()),
        }
        assert!(start.elapsed() < Duration::from_secs(10), "the worker never failed");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(last_error().contains("a test made the render worker panic"), "{}", last_error());
    // The other view, on the same device, draws on.
    assert_golden("cube", &other.read(other.frame(), 256, 256));
}
