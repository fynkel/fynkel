//! F0 storage comparison only. Uses the existing ECS; no shipping extension ABI.
//! cargo run --release -p fynkel_ecs --example foundation_storage -- 10000 120
use fynkel_ecs::{EntityBuilder, World};
use std::{hint::black_box, time::Instant};

#[derive(Clone, Copy)]
struct Health {
    hp: f64,
    poison_per_second: f64,
}

fn poison(world: &mut World) {
    for health in world.hecs_mut().query_mut::<&mut Health>() {
        health.hp = (health.hp - health.poison_per_second / 60.0).max(0.0);
        assert!(health.hp.is_finite() && health.poison_per_second >= 0.0);
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let count: usize = args.next().unwrap_or("10000".into()).parse().unwrap();
    let rounds: usize = args.next().unwrap_or("120".into()).parse().unwrap();
    assert!(count > 0 && rounds > 0);
    let mut world = World::new();
    for _ in 0..count {
        world.spawn(EntityBuilder::new().with(Health {
            hp: 100.0,
            poison_per_second: 2.0,
        }));
    }
    for _ in 0..20 {
        poison(&mut world);
    }
    for health in world.hecs_mut().query_mut::<&mut Health>() {
        health.hp = 100.0;
    }
    let mut samples = Vec::with_capacity(rounds);
    for _ in 0..rounds {
        let start = Instant::now();
        poison(black_box(&mut world));
        samples.push(start.elapsed().as_micros());
    }
    let checksum: f64 = world
        .hecs()
        .query::<&Health>()
        .iter()
        .map(|health| health.hp)
        .sum();
    let expected = count as f64 * (100.0 - rounds as f64 * 2.0 / 60.0).max(0.0);
    assert!((checksum - expected).abs() < 0.0001 * count as f64);
    samples.sort_unstable();
    println!(
        "{{\"entities\":{count},\"rounds\":{rounds},\"p50Us\":{},\"p95Us\":{},\"checksum\":{checksum},\"bridgeMeasured\":false,\"atomicJournalMeasured\":false}}",
        samples[rounds / 2],
        samples[(rounds * 95 / 100).min(rounds - 1)]
    );
}
