//! How long changing a particle system takes (stage 14's DF2): a game
//! sets one system's rate every frame, as an exhaust follows its speed,
//! in a world of many. In release, in CI:
//! `cargo test --release -p fynkel_world -- --ignored bench_`.

use fynkel_world::*;

#[test]
#[ignore = "a benchmark: run in release"]
fn bench_setting_one_of_a_thousand_particle_systems() {
    let mut world = World::new();
    let mut objects = Vec::new();
    for _ in 0..1000 {
        let id = world.spawn();
        world.insert(id, Transform::default()).unwrap();
        world.set_particles(id, ParticleSettings::default()).unwrap();
        objects.push(id);
    }
    let one = objects[500];
    let mut settings = ParticleSettings::default();
    let mut rate = 0.0;
    fynkel_bench::bench("setting one of 1000 particle systems, 1000 times", 20, 0.12, || {
        for _ in 0..1000 {
            rate = (rate + 1.0) % 50.0;
            settings.rate_over_time = rate;
            world.set_particles(one, settings.clone()).unwrap();
        }
    });
}
