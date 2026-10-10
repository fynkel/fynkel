//! Times stage 5 step 1's scene without Dart: 1,000 cubes dropped on the
//! ground in columns of 10, 400 fixed steps (8 s), as
//! packages/fynkel_physics/example runs it.
//!
//!   cargo run --release -p fynkel_physics --example tumble [cubes]

use std::time::Instant;

use fynkel_physics::{BodyKind, Moved, Physics, Pose, Rotation, Shape, Vector};

fn main() {
    let count: usize = std::env::args().nth(1).map_or(1000, |n| n.parse().expect("a number of cubes"));
    let mut physics = Physics::new();
    physics
        .add_collider(
            None,
            Shape::Box { half_extents: Vector::new(20.0, 0.5, 20.0) },
            Pose::from_translation(Vector::new(0.0, -0.5, 0.0)),
        )
        .unwrap();
    let side = ((count as f32 / 10.0).sqrt()).ceil() as usize;
    for i in 0..count {
        let column = i / 10;
        let position = Vector::new(
            ((column % side) as f32 - side as f32 / 2.0) * 0.8,
            2.0 + (i % 10) as f32 * 0.7,
            ((column / side) as f32 - side as f32 / 2.0) * 0.8,
        );
        let rotation = Rotation::from_axis_angle(Vector::new(1.0, 1.0, 0.0).normalize(), i as f32 * 0.37);
        let body = physics.add_body(BodyKind::Dynamic, i as u64, Pose::from_parts(position, rotation)).unwrap();
        physics.add_collider(Some(body), Shape::Box { half_extents: Vector::splat(0.25) }, Pose::IDENTITY).unwrap();
    }
    let mut moved: Vec<Moved> = Vec::new();
    let mut times = Vec::new();
    for _ in 0..400 {
        let start = Instant::now();
        physics.step(1.0 / 50.0).unwrap();
        physics.moved(&mut moved);
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    let total: f64 = times.iter().sum();
    times.sort_by(f64::total_cmp);
    println!(
        "{count} cubes, 400 steps: mean {:.2} ms, median {:.2} ms, slowest {:.2} ms",
        total / 400.0,
        times[200],
        times[399]
    );
}
