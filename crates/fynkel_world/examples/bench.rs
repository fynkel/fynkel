//! Times the world's basic operations at 100,000 objects (stage 1 step 1).
//!
//! `cargo run --release -p fynkel_world --example bench`
//!
//! Each line is the best of 5 runs, so it shows the cost of the code, not
//! of whatever else the machine was doing.

use std::time::{Duration, Instant};

use fynkel_world::*;

const N: usize = 100_000;
const RUNS: usize = 5;

fn main() {
    let mut best = [Duration::MAX; 5];
    for _ in 0..RUNS {
        let mut world = World::new();
        let mesh = world.add_mesh(MeshData::cube());
        let material = world.add_material(Material::default()).unwrap();
        let renderer = MeshRenderer { mesh: Some(mesh), material: Some(material) };
        let mut ids = Vec::with_capacity(N);

        let t = Instant::now();
        for i in 0..N {
            let id = world.spawn();
            world.insert(id, Transform::from_position(Vec3::new(i as f32, 0.0, 0.0))).unwrap();
            world.insert(id, renderer).unwrap();
            ids.push(id);
        }
        let spawn = t.elapsed();

        let t = Instant::now();
        for (i, &id) in ids.iter().enumerate() {
            let turn = Quat::from_rotation_y(i as f32 * 0.001);
            world.insert(id, Transform { rotation: turn, ..Transform::default() }).unwrap();
        }
        let set = t.elapsed();

        let t = Instant::now();
        let mut sum = 0.0;
        for &id in &ids {
            sum += world.get::<Transform>(id).unwrap().position.x;
        }
        let get = t.elapsed();

        let t = Instant::now();
        let mut visited = 0;
        world.for_each2::<Transform, MeshRenderer>(|_, t, _| {
            sum += t.rotation.y;
            visited += 1;
        });
        let iterate = t.elapsed();
        assert_eq!(visited, N);
        std::hint::black_box(sum);

        let t = Instant::now();
        for &id in &ids {
            world.despawn(id).unwrap();
        }
        let despawn = t.elapsed();
        assert!(world.is_empty());

        for (b, d) in best.iter_mut().zip([spawn, set, get, iterate, despawn]) {
            *b = (*b).min(d);
        }
    }

    let names = [
        "spawn + Transform + MeshRenderer",
        "set Transform (validated)",
        "get Transform",
        "for_each2 Transform+MeshRenderer",
        "despawn",
    ];
    println!("{N} objects, best of {RUNS}:");
    for (name, d) in names.iter().zip(best) {
        let ms = d.as_secs_f64() * 1e3;
        println!("  {name:<34} {ms:>8.2} ms  {:>7.1} ns/object", ms * 1e6 / N as f64);
    }
}
