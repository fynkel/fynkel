//! A mesh swept or changed again each frame allocates nothing once warm
//! (stage 14's WO8: a road swept again was 87 allocations, 800 KB). This
//! is its own test binary because it replaces the global allocator.
//!
//! Unsafe code is allowed here and only here: a global allocator is unsafe
//! by definition. It just counts, then forwards to the system allocator.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use fynkel_mesh::*;
use glam::{Vec2, Vec3};

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

fn count<T>(f: impl FnOnce() -> T) -> (T, u64) {
    COUNT.with(|c| c.set(0));
    COUNTING.with(|c| c.set(true));
    let out = f();
    COUNTING.with(|c| c.set(false));
    (out, COUNT.with(Cell::get))
}

/// A wavy path of `n` points, moved by `t`.
fn path(n: usize, t: f32) -> Vec<PathPoint> {
    (0..n)
        .map(|i| {
            let d = i as f32 * 0.5;
            PathPoint {
                position: Vec3::new(d, (d * 0.3 + t).sin(), 0.0),
                forward: Vec3::new(1.0, 0.3 * (d * 0.3 + t).cos(), 0.0),
                up: Vec3::Y,
                distance: d,
            }
        })
        .collect()
}

fn ring(points: usize, radius: f32) -> Vec<Vec2> {
    (0..points)
        .map(|i| {
            let a = i as f32 / points as f32 * std::f32::consts::TAU;
            Vec2::new(a.cos(), a.sin()) * radius
        })
        .collect()
}

#[test]
fn a_capped_pipe_swept_again_allocates_nothing() {
    let (outline, hole) = (ring(8, 1.0), ring(6, 0.5));
    let holes: [&[Vec2]; 1] = [&hole];
    let paths: Vec<Vec<PathPoint>> = (0..12).map(|k| path(200, k as f32 * 0.1)).collect();
    let sweep = |path| Sweep { path, outline: &outline, holes: &holes, caps: true, uv_per_metre: 1.0 };
    let mut mesh = MeshData::sweep(&sweep(&paths[0])).unwrap();
    mesh.sweep_into(&sweep(&paths[1])).unwrap();
    for path in &paths[2..] {
        let (done, allocations) = count(|| mesh.sweep_into(&sweep(path)));
        done.unwrap();
        assert_eq!(allocations, 0, "a sweep again");
    }
    // What it made is what a fresh sweep makes.
    let fresh = MeshData::sweep(&sweep(&paths[11])).unwrap();
    assert_eq!(mesh.vertices(), fresh.vertices());
    assert_eq!(mesh.indices(), fresh.indices());
    assert_eq!(mesh.submeshes(), fresh.submeshes());
    assert_eq!(mesh.bounds(), fresh.bounds());
}

#[test]
fn a_mesh_changed_with_its_tangents_worked_out_again_allocates_nothing() {
    let mut mesh = MeshData::grid(Vec2::new(10.0, 10.0), [40, 40], 1.0);
    let mut positions: Vec<[f32; 3]> = mesh.vertices().iter().map(|v| v.position).collect();
    let change = |mesh: &mut MeshData, positions: &mut [[f32; 3]], t: f32| {
        for p in positions.iter_mut() {
            p[1] = (p[0] + t).sin() * 0.2;
        }
        mesh.change(MeshChange {
            positions: Some(positions),
            recalculate_normals: true,
            recalculate_tangents: true,
            ..MeshChange::default()
        })
    };
    change(&mut mesh, &mut positions, 0.0).unwrap();
    for k in 1..6 {
        let (done, allocations) = count(|| change(&mut mesh, &mut positions, k as f32 * 0.1));
        done.unwrap();
        assert_eq!(allocations, 0, "a change");
    }
}
