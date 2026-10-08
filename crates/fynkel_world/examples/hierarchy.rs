//! Times world matrices for 10,000 objects in trees (stage 2 step 1).
//!
//! `cargo run --release -p fynkel_world --example hierarchy`
//!
//! Three shapes: 100 parents with 100 children each, 100 chains 100 deep,
//! and 10,000 objects in no tree (what stage 1 had). Each line is the best
//! of 5 runs.

use std::time::{Duration, Instant};

use fynkel_world::*;

const N: usize = 10_000;
const RUNS: usize = 5;

#[derive(Clone, Copy)]
enum Shape {
    Wide,
    Deep,
    Loose,
}

/// Spawns `N` objects with transforms and links them into `shape`. Returns
/// the world and the time `set_parent` took.
fn build(shape: Shape) -> (World, Vec<ObjectId>, Duration) {
    let mut world = World::new();
    let ids: Vec<ObjectId> = (0..N)
        .map(|i| {
            let id = world.spawn();
            let t = Transform {
                position: Vec3::new(0.0, 0.01, 0.1),
                rotation: Quat::from_rotation_y(i as f32 * 1e-3),
                scale: Vec3::ONE,
            };
            world.insert(id, t).unwrap();
            id
        })
        .collect();
    let t = Instant::now();
    for (i, &id) in ids.iter().enumerate() {
        let parent = match shape {
            // Every 100th object is a parent; the 99 after it are its children.
            Shape::Wide if !i.is_multiple_of(100) => Some(ids[i - i % 100]),
            // Each object hangs under the one before it, in runs of 100.
            Shape::Deep if !i.is_multiple_of(100) => Some(ids[i - 1]),
            _ => None,
        };
        if parent.is_some() {
            world.set_parent(id, parent).unwrap();
        }
    }
    (world, ids, t.elapsed())
}

fn main() {
    println!("{N} objects, best of {RUNS}:");
    for (name, shape) in
        [("100 × 100 wide", Shape::Wide), ("100 chains 100 deep", Shape::Deep), ("no tree", Shape::Loose)]
    {
        let mut best = [Duration::MAX; 4];
        for _ in 0..RUNS {
            let (world, ids, link) = build(shape);
            let mut matrices = WorldMatrices::default();

            let t = Instant::now();
            matrices.update(&world);
            let first = t.elapsed();

            let t = Instant::now();
            matrices.update(&world);
            let steady = t.elapsed();

            // What the extract does for each drawable: its own transform in,
            // its world matrix out.
            let t = Instant::now();
            let mut sum = 0.0;
            world.for_each::<Transform>(|id, local| sum += matrices.world(id, local).w_axis.y);
            let read = t.elapsed();
            std::hint::black_box(sum);
            assert!(world.world_matrix(ids[N - 1]).is_ok());

            for (b, d) in best.iter_mut().zip([link, first, steady, read]) {
                *b = (*b).min(d);
            }
        }
        let ms = best.map(|d| d.as_secs_f64() * 1e3);
        println!(
            "  {name:<20} set_parent {:>6.2} ms   first update {:>6.2} ms   steady update {:>6.3} ms   read all {:>6.3} ms",
            ms[0], ms[1], ms[2], ms[3]
        );
    }
}
