//! World teardown is leak-safe (M1 gate): the counting global allocator sees
//! the bytes in use return to the post-warm-up baseline after every
//! create → populate → step → query → destroy cycle through the C ABI, the
//! path the Dart SDK takes. Runs single-threaded (one test in this file) so
//! the counter is not disturbed by other tests.

use std::ptr;

use fynkel_contracts::Status;
use fynkel_native::*;
use fynkel_scene::Transform;
use glam::Vec3;
#[path = "support/edit.rs"]
mod edit;
use edit::Edit;

// The counting allocator is `fynkel_native`'s own (`fynkel_core::memory`,
// M3-16): this test binary links the crate that installs it, so the
// counters below are the engine's.
use fynkel_core::memory::{allocation_count, heap_bytes};

const ENTITIES: u64 = 500;
const TICKS: usize = 30;

/// One SDK-shaped lifetime: runtime, world, spawns under a hub, subscribe,
/// move everything every tick, queries, destroy half, teardown.
fn cycle() {
    let rt = fynkel_runtime_create();
    let w = unsafe { fynkel_world_create(rt, ptr::null(), std::ptr::null_mut()) };
    assert!(!w.is_null());

    let mut edit = Edit::new(w);
    let hub = edit.spawn("hub", Transform::IDENTITY, None);
    let mut pending = Vec::new();
    for i in 0..ENTITIES {
        pending.push(edit.spawn(
            &format!("e{i}"),
            Transform::from_position(Vec3::new(i as f32, 0.0, 0.0)),
            Some(hub),
        ));
    }
    let commit = edit.commit();
    let hub = commit.resolve(hub);
    let entities: Vec<_> = pending
        .into_iter()
        .map(|reference| commit.resolve(reference))
        .collect();
    assert_eq!(entities.len(), ENTITIES as usize);
    step(w);
    let bits = entities.clone();
    unsafe { fynkel_world_subscribe(w, 1, bits.as_ptr(), bits.len() as u32, 1) };

    for t in 0..TICKS {
        let mut edit = Edit::new(w);
        for (i, entity) in entities.iter().enumerate() {
            edit.set_transform(
                *entity,
                Transform::from_position(Vec3::new(i as f32, t as f32, 0.0)),
            );
        }
        edit.commit();
        step(w);
        let snap = unsafe { fynkel_world_snapshot(w) };
        assert!(snap.length > 0);
        unsafe { fynkel_world_drain_events(w) };
    }

    let mut out = FynkelBufferView {
        data: ptr::null(),
        length: 0,
    };
    let q = FynkelQueryDesc {
        kind: 2, // children
        request_id: 1,
        entity: hub,
    };
    unsafe { fynkel_world_query(w, &q, &mut out) };
    assert!(out.length > 0);

    let mut edit = Edit::new(w);
    for entity in entities.iter().take(ENTITIES as usize / 2) {
        edit.destroy(*entity);
    }
    edit.commit();
    step(w);
    unsafe { fynkel_world_drain_events(w) };

    unsafe {
        fynkel_world_destroy(w);
        fynkel_runtime_destroy(rt);
    }
}

fn step(w: *mut FynkelWorld) {
    let mut r = FynkelStepResult::default();
    assert_eq!(unsafe { fynkel_world_step(w, &mut r) }, Status::Ok);
    assert_eq!(r.commands_rejected, 0);
}

#[test]
fn world_teardown_returns_every_byte() {
    // Warm up: lazy statics, thread-local caches, allocator arenas.
    cycle();
    cycle();
    let baseline = heap_bytes();
    let allocs_before = allocation_count();

    const CYCLES: usize = 20;
    let mut peak = 0;
    for _ in 0..CYCLES {
        cycle();
        peak = peak.max(heap_bytes());
    }
    let after = heap_bytes();
    let allocs = allocation_count() - allocs_before;
    eprintln!(
        "[leak] baseline {baseline} B, after {CYCLES} cycles {after} B (peak {peak} B), {} allocations/cycle",
        allocs / CYCLES as u64
    );
    assert_eq!(
        after,
        baseline,
        "{} bytes retained after {CYCLES} world lifetimes",
        after as i64 - baseline as i64
    );
}
