//! Isolated native full-tick timing and allocator traffic for F3's owned batches.
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
fn damage(e: &mut GameplayEdit<'_>, _: SystemContext) -> EditResult<u32> {
    let batch = e.query_registered("bench.Health")?;
    let values = batch
        .rows()
        .iter()
        .map(|(_, fields)| {
            let ScalarValue::Float(hp) = fields[0] else {
                unreachable!()
            };
            vec![
                ScalarValue::Float(if hp == 0. { 100. } else { hp - 1. }),
                ScalarValue::Bool(false),
            ]
        })
        .collect();
    e.apply_registered_batch(&batch, values)?;
    Ok(batch.rows().len() as u32)
}
fn main() {
    for count in [1, 1000, 10000] {
        let mut w = World::new(Default::default());
        w.register_dynamic_component(ComponentSchema {
            id: "bench.Health".into(),
            version: 1,
            fields: vec![
                RegisteredField {
                    name: "hp".into(),
                    default: ScalarValue::Float(100.),
                },
                RegisteredField {
                    name: "invulnerable".into(),
                    default: ScalarValue::Bool(false),
                },
            ],
        })
        .unwrap();
        w.edit_gameplay(|e| {
            for _ in 0..count {
                let p = e.spawn("", Transform::IDENTITY, None)?;
                e.add_registered_defaults(p, "bench.Health")?;
            }
            Ok(())
        })
        .unwrap();
        let mut spec = SystemSpec::new("bench.Damage", SystemPhase::Simulation);
        spec.writes.push("bench.Health".into());
        w.register_systems(vec![RegisteredSystem { spec, run: damage }])
            .unwrap();
        for _ in 0..40 {
            assert_eq!(w.step().rejected, 0);
        }
        let mut samples = Vec::with_capacity(60);
        let calls = ALLOCS.load(Relaxed);
        let bytes = BYTES.load(Relaxed);
        for _ in 0..60 {
            let start = Instant::now();
            assert_eq!(w.step().rejected, 0);
            samples.push(start.elapsed().as_micros());
        }
        let calls = (ALLOCS.load(Relaxed) - calls) as f64 / 60.;
        let bytes = (BYTES.load(Relaxed) - bytes) as f64 / 60.;
        samples.sort_unstable();
        println!("{{\"entities\":{count},\"native_tick_p50_us\":{},\"native_tick_p95_us\":{},\"native_allocation_calls_per_tick\":{calls},\"native_bytes_requested_per_tick\":{bytes}}}", samples[30], samples[57]);
    }
}
