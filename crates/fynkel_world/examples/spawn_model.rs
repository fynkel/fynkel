//! Times spawning the 138-node tower (stage 2 step 4), and computing every
//! object's world matrix afterwards, as each frame's extract does.
//!
//! `cargo run --release -p fynkel_world --example spawn_model`
//!
//! Each line is the best of 20 runs.

use std::time::{Duration, Instant};

use fynkel_world::{ModelData, Transform, Vec3, World, WorldMatrices};

fn best(mut run: impl FnMut()) -> Duration {
    (0..20)
        .map(|_| {
            let t = Instant::now();
            run();
            t.elapsed()
        })
        .min()
        .unwrap()
}

fn main() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/assets/gltf/tower.glb");
    let model = ModelData::from_glb(&std::fs::read(path).unwrap()).unwrap();
    let mut world = World::new();
    let ids = world.add_model(&model).unwrap();
    let spawn = best(|| {
        let root = world.spawn_model(&model, &ids.meshes, &ids.materials, Transform::default()).unwrap();
        world.despawn(root).unwrap();
    });
    println!("spawn + despawn, {} objects: {:.1} µs", model.nodes().len() + 1, spawn.as_secs_f64() * 1e6);

    for copies in [1, 10] {
        let mut world = World::new();
        let ids = world.add_model(&model).unwrap();
        for i in 0..copies {
            let at = Transform::from_position(Vec3::X * 10.0 * i as f32);
            world.spawn_model(&model, &ids.meshes, &ids.materials, at).unwrap();
        }
        let mut matrices = WorldMatrices::default();
        matrices.update(&world);
        let update = best(|| matrices.update(&world));
        println!("world matrices, {copies} towers ({} objects): {:.1} µs", world.len(), update.as_secs_f64() * 1e6);
    }
}
