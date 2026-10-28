//! How long raycasts take (stage 14's WO4): many casts a frame into a
//! world of trees and a big ground, as a game's sight lines, ground checks
//! and cursor do. In release, in CI:
//! `cargo test --release -p fynkel_world -- --ignored bench_`.

use fynkel_world::*;

/// 750 trees of 4 cubes, one below the other, over a ground of about
/// 50,000 triangles, and the rays: straight down from above, spread over
/// it, so some hit cubes and the rest the ground.
fn world() -> (World, Vec<Ray>) {
    let mut world = World::new();
    let cube = world.add_mesh(MeshData::cube());
    let ground = world.add_mesh(MeshData::grid(Vec2::new(100.0, 100.0), [158, 158], 1.0));
    let material = world.add_material(Material::default()).unwrap();
    let renderer = |mesh| MeshRenderer { mesh: Some(mesh), material: Some(material), ..Default::default() };
    let floor = world.spawn();
    world.insert(floor, Transform::default()).unwrap();
    world.insert(floor, renderer(ground)).unwrap();
    for i in 0..750 {
        let mut parent = None;
        for _ in 0..4 {
            let id = world.spawn();
            let position = match parent {
                None => Vec3::new((i % 30) as f32 * 3.0 - 45.0, 0.5, (i / 30) as f32 * 3.0 - 45.0),
                Some(_) => Vec3::new(0.0, 1.1, 0.0),
            };
            world.insert(id, Transform { scale: Vec3::splat(0.9), ..Transform::from_position(position) }).unwrap();
            world.insert(id, renderer(cube)).unwrap();
            if let Some(parent) = parent {
                world.set_parent(id, Some(parent)).unwrap();
            }
            parent = Some(id);
        }
    }
    // A fixed spread: x and z by two different steps, wrapping.
    let rays = (0..50)
        .map(|i| {
            let (x, z) = ((i * 37 % 97) as f32 - 48.0, (i * 61 % 89) as f32 - 45.0);
            Ray::new(Vec3::new(x, 20.0, z), Vec3::NEG_Y).unwrap()
        })
        .collect();
    (world, rays)
}

#[test]
fn the_benchmarks_world_is_hit_where_it_should_be() {
    let (world, rays) = world();
    let mut caster = Raycaster::default();
    let hits: Vec<_> = rays.iter().map(|&ray| caster.cast(&world, ray, f32::INFINITY, u32::MAX)).collect();
    // Every ray hits something: a cube's top or the ground.
    assert!(hits.iter().all(Option::is_some));
    let on_ground = hits.iter().flatten().filter(|h| h.point.y.abs() < 1e-4).count();
    assert!(on_ground > 10 && on_ground < 50, "{on_ground} on the ground");
}

/// 50 casts into the world: 11.3 ms on a Mac before WO4 (every matrix
/// worked out again each cast, every triangle of the ground tested), 1.5
/// after. The bar is that doubled, as the other benchmarks'.
#[test]
#[ignore = "a benchmark: run in release"]
fn bench_fifty_raycasts() {
    let (world, rays) = world();
    let mut caster = Raycaster::default();
    fynkel_bench::bench("50 raycasts into 3000 objects and 50k triangles", 20, 3.0, || {
        for &ray in &rays {
            std::hint::black_box(caster.cast(&world, ray, f32::INFINITY, u32::MAX));
        }
    });
}
