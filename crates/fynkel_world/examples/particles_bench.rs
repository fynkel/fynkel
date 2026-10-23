//! Times ten systems of a thousand particles, each with a size and a
//! colour over its life and gravity, stepped a frame at a time: stage 12's
//! bar of under 1 ms a frame on the Note10+ (docs/stage-12.md §6).
//!
//! `cargo run --release -p fynkel_world --example particles_bench`
//!
//! Prints the median and the slowest of 600 frames, after 120 to warm up,
//! of the step and of the copy extraction makes for the renderer.

use std::time::Instant;

use fynkel_world::*;

fn main() {
    let mut world = World::new();
    let mut ids = Vec::new();
    let mut sizes = [0.0; TABLE];
    let mut colors = [Color::WHITE; TABLE];
    for i in 0..TABLE {
        let t = i as f32 / (TABLE - 1) as f32;
        sizes[i] = 1.0 - t;
        colors[i] = Color::rgba(0.7, 1.0, 0.7, 1.0 - t);
    }
    for i in 0..10 {
        let id = world.spawn();
        world.insert(id, Transform::from_position(Vec3::new(i as f32, 0.0, 0.0))).unwrap();
        let settings = ParticleSettings {
            max_particles: 1000,
            start_lifetime: ParticleRange::constant(2.0),
            start_speed: ParticleRange { min: 1.5, max: 2.5 },
            gravity_modifier: 0.2,
            rate_over_time: 500.0,
            prewarm: true,
            size_over_lifetime: Some(Box::new(sizes)),
            color_over_lifetime: Some(Box::new(colors)),
            ..ParticleSettings::default()
        };
        world.set_particles(id, settings).unwrap();
        ids.push(id);
    }
    let (dt, mut events) = (1.0 / 60.0, Vec::new());
    let (mut times, mut copies, mut drawn) = (Vec::with_capacity(600), Vec::with_capacity(600), Vec::new());
    for frame in 0..720 {
        let t = Instant::now();
        world.step_particles(dt, dt, &mut events);
        let stepped = t.elapsed();
        // What extraction does for the renderer, each frame.
        let t = Instant::now();
        drawn.clear();
        for &id in &ids {
            let emitter = world.world_matrix(id).unwrap();
            world.particles(id).unwrap().drawn(emitter, &mut drawn);
        }
        if frame >= 120 {
            times.push(stepped);
            copies.push(t.elapsed());
        }
    }
    let count: usize = ids.iter().map(|&id| world.particles(id).unwrap().count()).sum();
    for (what, times) in [("stepped", &mut times), ("copied for drawing", &mut copies)] {
        times.sort();
        println!(
            "{count} particles in 10 systems {what}: median {:.3} ms, slowest {:.3} ms a frame",
            times[times.len() / 2].as_secs_f64() * 1e3,
            times[times.len() - 1].as_secs_f64() * 1e3,
        );
    }
}
