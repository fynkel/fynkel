//! F0 strategy comparison: staging + validation + commit + native tick + extraction.
mod state_kernel;
use fynkel_contracts::Entity;
use fynkel_core::{memory, Command, Transform};
use serde_json::json;
use state_kernel::{Health, Kernel, Key, Strategy, Transaction};
use std::{hint::black_box, time::Instant};

#[global_allocator]
static ALLOCATOR: memory::CountingAllocator = memory::CountingAllocator;

fn workload(strategy: Strategy, count: usize, dense: bool) -> serde_json::Value {
    let mut kernel = Kernel::new(strategy);
    kernel.call(json!({"op":"begin"})).unwrap();
    for index in 0..count {
        let reference = kernel
            .call(json!({"op":"spawn","name":format!("entity-{index}")}))
            .unwrap();
        kernel.call(json!({"op":"set","ref":reference,"field":"Health","value":{"hp":100.0,"poisonPerSecond":2.0}})).unwrap();
    }
    let created = kernel.call(json!({"op":"commit"})).unwrap();
    let keys: Vec<_> = created["created"]
        .as_object()
        .unwrap()
        .values()
        .map(|v| {
            Key::Live(Entity::from_bits(
                u128::from_str_radix(v.as_str().unwrap(), 16).unwrap(),
            ))
        })
        .collect();
    let changed = if dense { count } else { 1 };
    let mut samples = Vec::with_capacity(30);
    let mut allocations = Vec::with_capacity(30);
    for round in 0..35 {
        let start_alloc = memory::allocation_count();
        let start = Instant::now();
        let mut tx = Transaction::all();
        for key in keys.iter().take(changed) {
            kernel
                .set_health(
                    &mut tx,
                    *key,
                    Health {
                        hp: 100.0 - (round + 1) as f64 / 30.0,
                        poison: 2.0,
                    },
                )
                .unwrap();
        }
        kernel.commit(tx).unwrap();
        kernel.core.begin_frame(kernel.core.config().fixed_dt);
        kernel.core.step();
        let extract = black_box(kernel.core.extract());
        assert_eq!(extract.records.len(), count);
        let elapsed = start.elapsed().as_micros();
        let allocated = memory::allocation_count() - start_alloc;
        if round >= 5 {
            samples.push(elapsed);
            allocations.push(allocated);
        }
    }
    samples.sort_unstable();
    allocations.sort_unstable();
    kernel
        .call(json!({"op":"begin","reads":["Health"],"writes":[]}))
        .unwrap();
    let rows = kernel.call(json!({"op":"query","field":"Health"})).unwrap();
    let checksum: f64 = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["value"]["hp"].as_f64().unwrap())
        .sum();
    kernel.call(json!({"op":"abort"})).unwrap();
    assert!(
        (checksum - (count as f64 * 100.0 - changed as f64 * 35.0 / 30.0)).abs()
            < 0.0001 * count as f64
    );
    json!({"strategy":if strategy==Strategy::Adapter{"preflight-adapter"}else{"shared-transaction-view"},"entities":count,"changedPerFrame":changed,"measuredFrames":30,"warmupFrames":5,"p50Us":samples[15],"p95Us":samples[28],"allocationsP50":allocations[15],"allocationsP95":allocations[28],"preflightEntityCopies":kernel.preflight_entities,"heapBytes":memory::heap_bytes(),"checksum":checksum})
}

fn current_partial_apply() -> serde_json::Value {
    let mut world = fynkel_core::World::new(fynkel_core::WorldConfig::default());
    let commit = world
        .edit_gameplay(|edit| edit.spawn("before", Transform::IDENTITY, None))
        .unwrap();
    let entity = commit.resolve(commit.value).unwrap();
    let rejections = world.apply_gameplay_now(vec![
        Command::SetName {
            entity,
            name: "after".into(),
        },
        Command::SetParent {
            child: entity,
            parent: Some(entity),
            keep_world: false,
        },
    ]);
    let name = world
        .ecs()
        .get::<fynkel_core::world::Name>(entity)
        .unwrap()
        .value
        .clone();
    json!({"earlierNameSurvivedLaterRejection":name=="after","rejections":rejections.len(),"interpretation":"Current per-command behavior observation, not a regression requiring partial apply"})
}

fn main() {
    let mut results = Vec::new();
    for count in [1000, 10000] {
        for dense in [false, true] {
            for strategy in [Strategy::Shared, Strategy::Adapter] {
                results.push(workload(strategy, count, dense));
            }
        }
    }
    println!(
        "{}",
        json!({"currentApplyObservation":current_partial_apply(),"workload":"Native-only fixed population; explicit staging/validation/commit + core tick + render extraction, no GPU/Dart/JSON transport or subsystem population","results":results})
    );
}
