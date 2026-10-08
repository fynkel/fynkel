//! The frame clock's side of a frame allocates nothing (stage 1 plan
//! §3.5): `fy_view_frame` extracts into recycled scene buffers, world
//! matrices go into a kept buffer (stage 2 plan §3.1), and setting
//! components goes straight into the world. Only the calling
//! thread is counted. The render worker's own allocations (wgpu's) are
//! measured in fynkel_render's tests.
//!
//! Its own test binary, because it replaces the global allocator. A global
//! allocator and C calls are both unsafe.
#![allow(unsafe_code)]

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use common::*;
use fynkel_ffi::*;

/// Counts allocations made on this thread while counting is switched on.
struct Counting;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<u64> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.with(Cell::get) {
            COUNT.with(|c| c.set(c.get() + 1));
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if COUNTING.with(Cell::get) {
            COUNT.with(|c| c.set(c.get() + 1));
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn count(f: impl FnOnce()) -> u64 {
    COUNT.with(|c| c.set(0));
    COUNTING.with(|c| c.set(true));
    f();
    COUNTING.with(|c| c.set(false));
    COUNT.with(Cell::get)
}

#[test]
fn a_steady_frame_with_1000_moving_cubes_in_trees_allocates_nothing_on_the_calling_thread() {
    let world = World::new();
    let cube = stage1(&world);
    let mut renderer = FyMeshRenderer::default();
    // SAFETY: a live world and a local out-value.
    ok(unsafe { fy_mesh_renderer_get(world.0, cube, &mut renderer) });
    let cubes: Vec<u64> = (0..1000)
        .map(|_| {
            let id = world.spawn();
            // SAFETY: a live world and a pointer to a local.
            ok(unsafe { fy_mesh_renderer_set(world.0, id, &renderer) });
            id
        })
        .collect();
    // Chains of ten, so the extract computes world matrices through
    // parents (stage 2 plan §3.1). The order is built once, in warm-up.
    for (i, &id) in cubes.iter().enumerate().filter(|(i, _)| !i.is_multiple_of(10)) {
        // SAFETY: a live world.
        ok(unsafe { fy_parent_set(world.0, id, cubes[i - 1]) });
    }
    let view = View::headless(&world, 64, 64);

    let frame = |t: f32| {
        for (i, &id) in cubes.iter().enumerate() {
            let position = FyVec3 { x: i as f32 * 0.01 + t, y: 0.0, z: 0.0 };
            let value = FyTransform {
                position,
                rotation: FyQuat { w: 1.0, ..Default::default() },
                scale: FyVec3 { x: 1.0, y: 1.0, z: 1.0 },
            };
            // SAFETY: a live world and a pointer to a local.
            ok(unsafe { fy_transform_set(world.0, id, &value) });
        }
        view.frame()
    };
    // Warm up the way the frame clock runs: without waiting for the GPU,
    // so all three scene buffers come into use and grow to size.
    for i in 0..50 {
        frame(i as f32);
    }
    view.read(50, 64, 64);
    let mut numbers = Vec::with_capacity(100);
    let allocations = count(|| {
        for i in 0..100 {
            numbers.push(frame(i as f32));
        }
    });
    view.read(*numbers.last().unwrap(), 64, 64);
    assert_eq!(allocations, 0, "allocations in 100 frames of 1,000 transform writes each");
}

/// Flutter's raster thread calls `fy_apple_acquire` once per frame.
#[cfg(target_vendor = "apple")]
#[test]
fn acquiring_the_surface_to_show_allocates_nothing() {
    unsafe extern "C" {
        fn CFRelease(object: *mut std::ffi::c_void);
    }
    let world = World::new();
    stage1(&world);
    let mut view = std::ptr::null_mut();
    // SAFETY: a live world and a local out-value.
    ok(unsafe { fy_view_new_apple(world.0, 32, 32, &mut view) });
    let mut number = 0;
    let acquire = || {
        let mut surface = std::ptr::null_mut();
        // SAFETY: a live view and a local out-value.
        let flags = unsafe { fy_apple_acquire(view, &mut surface) };
        if flags & FY_APPLE_SHOWN != 0 {
            // SAFETY: retained for us by fy_apple_acquire.
            unsafe { CFRelease(surface) };
        }
        flags
    };
    // Warm up: frames finish on a Metal thread, so acquire until one is there.
    for _ in 0..50 {
        // SAFETY: a live view and a local out-value.
        ok(unsafe { fy_view_frame(view, &mut number) });
        while acquire() == 0 {
            std::thread::yield_now();
        }
    }
    let allocations = count(|| {
        for _ in 0..1000 {
            acquire();
        }
    });
    // SAFETY: a live view, not used again.
    unsafe { fy_view_free(view) };
    assert_eq!(allocations, 0, "allocations in 1,000 acquires");
}
