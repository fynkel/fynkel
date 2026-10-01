//! The M1 demo scene, driven the way gameplay drives the engine: through
//! shared transactions, one edit per fixed tick. Shared by the headless and the
//! standalone hosts; the Flutter host builds the same scene from Dart.
//!
//! Layout: a "hub" cube at the origin that tumbles, and `satellites`
//! children parented to it, each on its own ring and with its own spin, so
//! the frame exercises hierarchy propagation and extract interpolation.

use fynkel_core::{Entity, Event, EventPayload, Tick, Transform, World};
use glam::{Quat, Vec3};

#[derive(Clone, Copy, Debug)]
struct Satellite {
    entity: Option<Entity>,
    radius: f32,
    phase: f32,
    angular_speed: f32,
    spin_speed: f32,
    height: f32,
    scale: f32,
}

pub struct DemoScene {
    satellites: Vec<Satellite>,
    hub: Option<Entity>,
    /// Simulation seconds elapsed at the last fixed update.
    time: f32,
    fixed_dt: f32,
    /// Passive driver for document scenes: spawns nothing, only watches.
    passive: bool,
    scene_ready: bool,
}

impl DemoScene {
    /// Deterministic for a given (`satellites`, `seed`).
    pub fn new(satellites: u32, seed: u64) -> Self {
        let mut rng = Lcg(seed ^ 0x9E37_79B9_7F4A_7C15);
        let satellites = (0..satellites)
            .map(|i| Satellite {
                entity: None,
                radius: 1.6 + rng.unit() * 2.2 + (i / 12) as f32 * 0.35,
                phase: rng.unit() * std::f32::consts::TAU,
                angular_speed: 0.3 + rng.unit() * 0.9,
                spin_speed: -2.0 + rng.unit() * 4.0,
                height: -0.6 + rng.unit() * 1.2,
                scale: 0.18 + rng.unit() * 0.22,
            })
            .collect();
        Self {
            satellites,
            hub: None,
            time: 0.0,
            fixed_dt: 0.0,
            passive: false,
            scene_ready: false,
        }
    }

    /// A driver that issues no commands (the world was filled by
    /// `World::load_scene`) and only records `SceneReady`.
    pub fn empty() -> Self {
        Self {
            passive: true,
            ..Self::new(0, 0)
        }
    }

    pub fn saw_scene_ready(&self) -> bool {
        self.scene_ready
    }

    pub fn hub(&self) -> Option<Entity> {
        self.hub
    }

    /// Entities the scene expects once every spawn has landed.
    pub fn expected_entities(&self) -> u32 {
        1 + self.satellites.len() as u32
    }

    pub fn is_ready(&self) -> bool {
        self.hub.is_some() && self.satellites.iter().all(|s| s.entity.is_some())
    }

    /// Observe scene loading events pulled by the host.
    pub fn on_events(&mut self, events: &[Event]) {
        for ev in events {
            if matches!(ev.payload, EventPayload::SceneReady { .. }) {
                self.scene_ready = true;
            }
        }
    }

    /// Commit gameplay for the tick about to be simulated. Handles resolve
    /// at commit, independently of the host draining lifecycle events.
    pub fn fixed_update(&mut self, world: &mut World, tick: Tick) {
        if self.passive {
            return;
        }
        self.fixed_dt = world.config().fixed_dt;
        self.time = tick as f32 * self.fixed_dt;
        world.prepare_gameplay_tick();
        if let Some(hub) = self.hub {
            let commit = world
                .edit_gameplay(|edit| {
                    let mut spawned = Vec::new();
                    for (index, satellite) in self.satellites.iter().enumerate() {
                        let transform = satellite_local(satellite, self.time);
                        if let Some(entity) = satellite.entity {
                            edit.set_transform(entity, transform)?;
                        } else {
                            spawned.push((index, edit.spawn("", transform, Some(hub.into()))?));
                        }
                    }
                    edit.set_transform(hub, hub_local(self.time))?;
                    Ok(spawned)
                })
                .expect("built-in demo edit must be valid");
            for (index, pending) in &commit.value {
                self.satellites[*index].entity = commit.resolve(*pending);
            }
        } else {
            let commit = world
                .edit_gameplay(|edit| edit.spawn("hub", Transform::IDENTITY, None))
                .expect("built-in demo spawn must be valid");
            self.hub = commit.resolve(commit.value);
        }
    }
}

fn hub_local(t: f32) -> Transform {
    Transform {
        position: Vec3::ZERO,
        rotation: Quat::from_rotation_y(t * 0.8) * Quat::from_rotation_x(t * 0.35),
        scale: Vec3::splat(0.8),
    }
}

fn satellite_local(s: &Satellite, t: f32) -> Transform {
    let a = s.phase + s.angular_speed * t;
    Transform {
        position: Vec3::new(a.cos() * s.radius, s.height, a.sin() * s.radius),
        rotation: Quat::from_rotation_y(s.spin_speed * t),
        scale: Vec3::splat(s.scale),
    }
}

/// 64-bit LCG (Knuth MMIX constants): deterministic across platforms, no
/// dependency.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// Uniform in `[0, 1)`.
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

#[cfg(test)]
mod transaction_tests {
    use super::*;
    use fynkel_core::WorldConfig;

    #[test]
    fn demo_transactions_preserve_interpolation_through_catch_up_ticks() {
        for hz in [30.0, 60.0, 120.0, 240.0] {
            let dt = 1.0 / hz;
            let mut world = World::new(WorldConfig {
                fixed_dt: dt,
                ..WorldConfig::default()
            });
            let mut demo = DemoScene::new(1, 123);
            demo.fixed_update(&mut world, 0);
            world.step();
            world.begin_frame(dt * 0.25);
            let plan = world.begin_frame(dt * 2.25);
            assert_eq!(plan.steps, 2);
            demo.fixed_update(&mut world, 1);
            world.step();
            demo.fixed_update(&mut world, 2);
            world.step();
            let previous = hub_local(dt);
            let current = hub_local(dt * 2.0);
            let expected = glam::Mat4::from_scale_rotation_translation(
                current.scale,
                previous.rotation.slerp(current.rotation, 0.5),
                Vec3::ZERO,
            );
            let extract = world.extract();
            let record = extract
                .records
                .iter()
                .find(|r| Some(r.entity) == demo.hub())
                .unwrap();
            assert!(
                record.world.abs_diff_eq(expected, 1e-5),
                "interpolation snapped at {hz} Hz"
            );
        }
    }

    #[test]
    fn demo_resolves_handles_without_event_delivery_or_queued_commands() {
        let mut world = World::new(WorldConfig::default());
        let mut demo = DemoScene::new(8, 123);
        demo.fixed_update(&mut world, 0);
        assert!(demo.hub().is_some());
        assert_eq!(world.live_entities(), 1);
        assert_eq!(world.step().applied, 0);
        // Deliberately never deliver events to the driver.
        demo.fixed_update(&mut world, 1);
        assert!(demo.is_ready());
        assert_eq!(world.live_entities(), demo.expected_entities());
        assert_eq!(world.step().applied, 0);
        for tick in 2..8 {
            demo.fixed_update(&mut world, tick);
            assert_eq!(world.step().applied, 0);
        }
        assert!(world.drain_events().is_empty());
        assert!(world.diagnostics().is_empty());
    }
}
