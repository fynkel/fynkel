//! World teardown is leak-safe (M1 gate): a counting global allocator sees
//! the bytes in use return to the post-warm-up baseline after every
//! create → populate → step → query → destroy cycle through the C ABI, the
//! path the Dart SDK takes. Runs single-threaded (one test in this file) so
//! the counter is not disturbed by other tests.

use std::alloc::{GlobalAlloc, Layout, System};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};

use fynkel_contracts::{Entity, EventKind, Status};
use fynkel_core::schema::*;
use fynkel_core::Lane;
use fynkel_native::buffer::{self, BufferKind, Writer};
use fynkel_native::*;
use fynkel_scene::Transform;
use fynkel_wire::{get_u32, WireRecord};
use glam::Vec3;

struct Counting;

static IN_USE: AtomicUsize = AtomicUsize::new(0);
static ALLOCS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = System.alloc(l);
        if !p.is_null() {
            IN_USE.fetch_add(l.size(), Ordering::Relaxed);
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        IN_USE.fetch_sub(l.size(), Ordering::Relaxed);
        System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        let q = System.realloc(p, l, new);
        if !q.is_null() {
            IN_USE.fetch_add(new, Ordering::Relaxed);
            IN_USE.fetch_sub(l.size(), Ordering::Relaxed);
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        q
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

const ENTITIES: u64 = 500;
const TICKS: usize = 30;

/// One SDK-shaped lifetime: runtime, world, spawns under a hub, subscribe,
/// move everything every tick, queries, destroy half, teardown.
fn cycle() {
    let rt = fynkel_runtime_create();
    let w = unsafe { fynkel_world_create(rt, ptr::null()) };
    assert!(!w.is_null());

    let mut writer = Writer::begin(Vec::new(), BufferKind::Commands);
    writer.command(
        1,
        &SpawnCommand {
            temp_id: 1,
            transform: Transform::IDENTITY,
            parent: None,
            name: "hub".into(),
        },
    );
    submit(w, &writer.finish());
    step(w);
    let hub = spawned(w)[0];

    let mut writer = Writer::begin(Vec::new(), BufferKind::Commands);
    for i in 0..ENTITIES {
        writer.command(
            1,
            &SpawnCommand {
                temp_id: 2 + i,
                transform: Transform::from_position(Vec3::new(i as f32, 0.0, 0.0)),
                parent: Some(hub),
                name: format!("e{i}"),
            },
        );
    }
    submit(w, &writer.finish());
    step(w);
    let entities = spawned(w);
    assert_eq!(entities.len(), ENTITIES as usize);
    let bits: Vec<u64> = entities.iter().map(|e| e.to_bits()).collect();
    unsafe { fynkel_world_subscribe(w, 1, bits.as_ptr(), bits.len() as u32, 1) };

    for t in 0..TICKS {
        let mut writer = Writer::begin(Vec::new(), BufferKind::Commands);
        for (i, e) in entities.iter().enumerate() {
            writer.command(
                3,
                &SetTransformCommand {
                    entity: *e,
                    transform: Transform::from_position(Vec3::new(i as f32, t as f32, 0.0)),
                },
            );
        }
        submit(w, &writer.finish());
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
        entity: hub.to_bits(),
    };
    unsafe { fynkel_world_query(w, &q, &mut out) };
    assert!(out.length > 0);

    let mut writer = Writer::begin(Vec::new(), BufferKind::Commands);
    for e in entities.iter().take(ENTITIES as usize / 2) {
        writer.command(2, &DestroyCommand { entity: *e });
    }
    submit(w, &writer.finish());
    step(w);
    unsafe { fynkel_world_drain_events(w) };

    unsafe {
        fynkel_world_destroy(w);
        fynkel_runtime_destroy(rt);
    }
}

fn submit(w: *mut FynkelWorld, buf: &[u8]) {
    let mut r = FynkelSubmitResult::default();
    let s = unsafe {
        fynkel_world_submit_commands(
            w,
            Lane::Simulation as u32,
            buf.as_ptr(),
            buf.len() as u32,
            &mut r,
        )
    };
    assert_eq!((s, r.rejected), (Status::Ok, 0));
}

fn step(w: *mut FynkelWorld) {
    let mut r = FynkelStepResult::default();
    assert_eq!(unsafe { fynkel_world_step(w, &mut r) }, Status::Ok);
    assert_eq!(r.commands_rejected, 0);
}

fn spawned(w: *mut FynkelWorld) -> Vec<Entity> {
    let v = unsafe { fynkel_world_drain_events(w) };
    let b = unsafe { std::slice::from_raw_parts(v.data, v.length as usize) };
    let (h, records, _) = buffer::validate(b, BufferKind::Events).unwrap();
    let mut out = Vec::new();
    let mut off = 0;
    for _ in 0..h.record_count {
        let kind = get_u32(records, off);
        let size = get_u32(records, off + 4) as usize;
        off += EventHeader::SIZE;
        if kind == EventKind::SPAWNED.0 {
            out.push(
                SpawnedEvent::decode(&records[off..off + size], &[])
                    .unwrap()
                    .entity,
            );
        }
        off += size;
    }
    out
}

#[test]
fn world_teardown_returns_every_byte() {
    // Warm up: lazy statics, thread-local caches, allocator arenas.
    cycle();
    cycle();
    let baseline = IN_USE.load(Ordering::Relaxed);
    let allocs_before = ALLOCS.load(Ordering::Relaxed);

    const CYCLES: usize = 20;
    let mut peak = 0;
    for _ in 0..CYCLES {
        cycle();
        peak = peak.max(IN_USE.load(Ordering::Relaxed));
    }
    let after = IN_USE.load(Ordering::Relaxed);
    let allocs = ALLOCS.load(Ordering::Relaxed) - allocs_before;
    eprintln!(
        "[leak] baseline {baseline} B, after {CYCLES} cycles {after} B (peak {peak} B), {} allocations/cycle",
        allocs / CYCLES
    );
    assert_eq!(
        after,
        baseline,
        "{} bytes retained after {CYCLES} world lifetimes",
        after as i64 - baseline as i64
    );
}
