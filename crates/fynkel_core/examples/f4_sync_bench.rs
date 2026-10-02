//! F4 synchronization versus full extraction costs; allocator requests, not retained memory.
use fynkel_core::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::Instant;
struct CountingAllocator;
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(layout.size() as u64, Relaxed);
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        System.dealloc(p, layout)
    }
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(size as u64, Relaxed);
        System.realloc(p, layout, size)
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

use fynkel_scene::{self as scene, SceneSync};
use glam::Vec3;
fn measure(label: &str, count: usize, mut run: impl FnMut(usize) -> usize) {
    for i in 0..20 {
        run(i);
    }
    let mut ns = Vec::with_capacity(100);
    let a = ALLOCS.load(Relaxed);
    let b = BYTES.load(Relaxed);
    let mut candidates = 0;
    for i in 0..100 {
        let start = Instant::now();
        candidates += run(i);
        ns.push(start.elapsed().as_nanos());
    }
    let calls = (ALLOCS.load(Relaxed) - a) as f64 / 100.;
    let bytes = (BYTES.load(Relaxed) - b) as f64 / 100.;
    ns.sort_unstable();
    println!(
        "{}",
        serde_json::json!({"workload":label,"entities":count,"p50_ns":ns[50],"p95_ns":ns[95],"candidates_per_sample":candidates as f64/100.,"allocation_calls_per_sample":calls,"requested_bytes_per_sample":bytes})
    );
}
fn main() {
    for n in [1000, 10000] {
        let mut w = World::new(Default::default());
        let mut ids = Vec::new();
        for i in 0..n {
            let e = scene::spawn(
                w.ecs_mut(),
                Transform::from_position(Vec3::new(i as f32 * 4., 0., 0.)),
                None,
            )
            .unwrap();
            w.ecs_mut().insert(e, Collider::default()).unwrap();
            ids.push(e);
        }
        w.refresh_physics();
        w.extract();
        measure("static_scene_physics", n, |_| {
            w.refresh_physics();
            w.physics().sync_candidates() + w.scene_sync_candidates().0
        });
        measure("sparse_scene_physics", n, |i| {
            w.ecs().get_mut::<Transform>(ids[i % n]).unwrap().position.y += 1.;
            w.refresh_physics();
            w.physics().sync_candidates() + w.scene_sync_candidates().0
        });
        measure("unchanged_table_revision", n, |_| {
            std::hint::black_box(w.entity_table_revision());
            0
        });
        measure("full_extract_static", n, |_| {
            std::hint::black_box(w.extract());
            w.render_membership_candidates()
        });
        measure("churn_scene_physics", n, |i| {
            let old = ids[i % n];
            w.ecs_mut().despawn(old).unwrap();
            let e = scene::spawn(w.ecs_mut(), Transform::IDENTITY, None).unwrap();
            w.ecs_mut().insert(e, Collider::default()).unwrap();
            ids[i % n] = e;
            w.refresh_physics();
            w.physics().sync_candidates() + w.scene_sync_candidates().0
        });
        let mut ecs = fynkel_ecs::World::new();
        let mut sync = SceneSync::default();
        for i in 0..n {
            let _e = scene::spawn(
                &mut ecs,
                Transform::from_position(Vec3::new(i as f32 * 4., 0., 0.)),
                None,
            )
            .unwrap();
        }
        sync.propagate(&mut ecs);
        // The stateless reference deliberately scans the complete spatial population.
        measure("reference_full_scene_scan", n, |_| {
            scene::propagate(&mut ecs);
            n
        });
        let mut sleeping = World::new(Default::default());
        let mut sleepers = Vec::new();
        for i in 0..n {
            let e = scene::spawn(
                sleeping.ecs_mut(),
                Transform::from_position(Vec3::new(i as f32 * 4., 0., 0.)),
                None,
            )
            .unwrap();
            sleeping.ecs_mut().insert(e, Collider::default()).unwrap();
            sleeping
                .ecs_mut()
                .insert(
                    e,
                    RigidBody {
                        gravity_scale: 0.,
                        ..Default::default()
                    },
                )
                .unwrap();
            sleepers.push(e);
        }
        for _ in 0..240 {
            sleeping.step();
        }
        assert!(sleepers
            .iter()
            .all(|&e| sleeping.physics().body_state(e).unwrap().sleeping));
        measure("sleeping_scene_physics", n, |_| {
            sleeping.refresh_physics();
            sleeping.physics().sync_candidates() + sleeping.scene_sync_candidates().0
        });
        measure("sleeping_full_tick", n, |_| {
            sleeping.step();
            sleeping.physics().sync_candidates()
        });
    }
}
