//! Times raycasts (stage 2 step 5): many small objects, one big mesh, and
//! the sample models, each with a ray that hits and one that misses
//! everything.
//!
//! `cargo run --release -p fynkel_world --example raycast`
//!
//! Each line is the best of 200 casts with one `Raycaster`, as a game
//! casting every frame would have.

use std::time::{Duration, Instant};

use fynkel_world::*;

fn best(world: &World, ray: Ray) -> (Duration, bool) {
    let mut caster = Raycaster::default();
    let mut hit = false;
    let time = (0..200)
        .map(|_| {
            let t = Instant::now();
            hit = std::hint::black_box(caster.cast(world, ray, f32::INFINITY, u32::MAX)).is_some();
            t.elapsed()
        })
        .min()
        .unwrap();
    (time, hit)
}

fn report(name: &str, world: &World, hit: Ray, miss: Ray) {
    let triangles: usize = {
        let mut n = 0;
        world.for_each::<MeshRenderer>(|_, r| n += r.mesh.map_or(0, |m| world.mesh(m).unwrap().indices().len() / 3));
        n
    };
    let (h, was_hit) = best(world, hit);
    let (m, was_missed) = best(world, miss);
    assert!(was_hit && !was_missed, "{name}: the rays don't do what they should");
    println!(
        "  {name:<34} {:>6} objects {:>8} triangles   hit {:>8.1} µs   miss {:>8.1} µs",
        world.len(),
        triangles,
        h.as_secs_f64() * 1e6,
        m.as_secs_f64() * 1e6
    );
}

fn material(world: &mut World) -> MaterialId {
    world.add_material(Material::default()).unwrap()
}

fn ray(origin: Vec3, direction: Vec3) -> Ray {
    Ray::new(origin, direction).unwrap()
}

/// 100 × 100 cubes on the ground, seen from above at an angle.
fn grid(deep: bool) -> World {
    let mut world = World::new();
    let cube = world.add_mesh(MeshData::cube());
    let material = material(&mut world);
    let mut parent = None;
    for i in 0..100 {
        for j in 0..100 {
            let id = world.spawn();
            let position = Vec3::new(i as f32 * 2.0 - 99.0, 0.0, j as f32 * 2.0 - 99.0);
            let local = match parent {
                // Each in a chain of 100 under the one before, placed by
                // the step from it.
                Some((_, at)) if deep && j > 0 => position - at,
                _ => position,
            };
            world.insert(id, Transform::from_position(local)).unwrap();
            world
                .insert(id, MeshRenderer { mesh: Some(cube), material: Some(material), ..Default::default() })
                .unwrap();
            if deep && j > 0 {
                world.set_parent(id, parent.map(|(p, _)| p)).unwrap();
            }
            parent = Some((id, position));
        }
    }
    world
}

fn sphere(segments: u32, rings: u32) -> World {
    let mut world = World::new();
    let mesh = world.add_mesh(MeshData::sphere(segments, rings));
    let material = material(&mut world);
    let id = world.spawn();
    world.insert(id, Transform::default()).unwrap();
    world.insert(id, MeshRenderer { mesh: Some(mesh), material: Some(material), ..Default::default() }).unwrap();
    world
}

fn model(file: &str) -> World {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/assets/gltf").join(file);
    let model = ModelData::from_glb(&std::fs::read(path).unwrap()).unwrap();
    let mut world = World::new();
    let ids = world.add_model(&model).unwrap();
    world.spawn_model(&model, &ids.meshes, &ids.materials, Transform::default()).unwrap();
    world
}

fn main() {
    println!("best of 200 casts:");
    let down = ray(Vec3::new(1.0, 50.0, 1.0), Vec3::new(0.0, -1.0, 0.01));
    let away = ray(Vec3::new(0.3, 50.0, 0.3), Vec3::Y);
    report("100 × 100 cubes, no tree", &grid(false), down, away);
    report("100 × 100 cubes, 100 chains 100 deep", &grid(true), down, away);

    let into = ray(Vec3::new(0.01, 0.02, 5.0), Vec3::NEG_Z);
    let past = ray(Vec3::new(0.7, 0.7, 5.0), Vec3::NEG_Z);
    report("sphere 32 × 16", &sphere(32, 16), into, past);
    report("sphere 256 × 128", &sphere(256, 128), into, past);

    // Each from the front, through the middle of its box, and well above.
    for (file, hit) in [
        ("tower.glb", ray(Vec3::new(0.0, 5.0, 20.0), Vec3::new(0.0, 0.0, -1.0))),
        ("CesiumMilkTruck.glb", ray(Vec3::new(0.0, 1.0, 20.0), Vec3::new(0.0, 0.0, -1.0))),
        ("Fox.glb", ray(Vec3::new(0.0, 40.0, 200.0), Vec3::new(0.0, 0.0, -1.0))),
    ] {
        let miss = ray(Vec3::new(0.0, 1000.0, 0.0), Vec3::Y);
        report(file, &model(file), hit, miss);
    }
}
