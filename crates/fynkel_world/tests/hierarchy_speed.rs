//! How long keeping world matrices takes (stage 14's WO10): every view
//! works them out each frame, so one object moving among 3,000 in trees
//! shouldn't work out all 3,000 again. In release, in CI:
//! `cargo test --release -p fynkel_world -- --ignored bench_`.

use fynkel_world::*;

/// 300 trees of 10 objects, a root and nine below it, one below the
/// other.
fn forest() -> (World, Vec<ObjectId>) {
    let mut world = World::new();
    let mut roots = Vec::new();
    for i in 0..300 {
        let mut parent = None;
        for _ in 0..10 {
            let id = world.spawn();
            let position = match parent {
                None => Vec3::new((i % 20) as f32 * 3.0, 0.0, (i / 20) as f32 * 3.0),
                Some(_) => Vec3::new(0.0, 1.0, 0.0),
            };
            world.insert(id, Transform::from_position(position)).unwrap();
            match parent {
                Some(parent) => world.set_parent(id, Some(parent)).unwrap(),
                None => roots.push(id),
            }
            parent = Some(id);
        }
    }
    (world, roots)
}

#[test]
#[ignore = "a benchmark: run in release"]
fn bench_one_tree_moved_among_300() {
    let (mut world, roots) = forest();
    let mut matrices = WorldMatrices::default();
    matrices.update(&world);
    let mut frame = 0;
    // 0.050 ms on the Mac when every matrix was worked out again, 0.003
    // with only the moved tree (2026-11-02).
    fynkel_bench::bench("world matrices, one tree of 300 moved", 200, 0.01, || {
        frame += 1;
        let root = roots[frame % roots.len()];
        world.insert(root, Transform::from_position(Vec3::new(frame as f32 * 0.001, 0.0, 0.0))).unwrap();
        matrices.update(&world);
    });
    let full = |world: &World| {
        let mut fresh = WorldMatrices::default();
        fresh.update(world);
        roots.iter().map(|&id| fresh.matrix(world, id)).collect::<Vec<_>>()
    };
    assert_eq!(roots.iter().map(|&id| matrices.matrix(&world, id)).collect::<Vec<_>>(), full(&world));
}
