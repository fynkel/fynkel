//! The M1 demo scene, driven the way gameplay drives the engine: through
//! commands on the simulation lane, one batch per fixed tick, and through
//! pulled events for the spawn hand-shake. Shared by the headless and the
//! standalone hosts; the Flutter host builds the same scene from Dart.
//!
//! Layout: a "hub" cube at the origin that tumbles, and `satellites`
//! children parented to it, each on its own ring and with its own spin, so
//! the frame exercises hierarchy propagation and extract interpolation.

use fynkel_core::{Command, Entity, Event, EventPayload, Lane, Tick, Transform, World};
use glam::{Quat, Vec3};

const HUB_TEMP_ID: u64 = 1;

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
    hub_requested: bool,
    satellites_requested: bool,
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
            hub_requested: false,
            satellites_requested: false,
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

    /// Feed the events the host pulled (`World::drain_events`) so spawn
    /// hand-shakes resolve temp ids to handles.
    pub fn on_events(&mut self, events: &[Event]) {
        for ev in events {
            if matches!(ev.payload, EventPayload::SceneReady) {
                self.scene_ready = true;
            }
            if let EventPayload::Spawned { temp_id, entity } = ev.payload {
                if temp_id == HUB_TEMP_ID {
                    self.hub = Some(entity);
                } else if let Some(s) = self.satellites.get_mut((temp_id - 2) as usize) {
                    s.entity = Some(entity);
                }
            }
        }
    }

    /// Gameplay for one fixed tick: stages the commands the next `step()`
    /// applies. `tick` is the tick about to be simulated.
    pub fn fixed_update(&mut self, world: &mut World, tick: Tick) {
        if self.passive {
            return;
        }
        self.fixed_dt = world.config().fixed_dt;
        self.time = tick as f32 * self.fixed_dt;
        let mut commands = Vec::with_capacity(self.satellites.len() + 1);

        if !self.hub_requested {
            self.hub_requested = true;
            commands.push(Command::Spawn {
                temp_id: HUB_TEMP_ID,
                transform: Transform::IDENTITY,
                parent: None,
                name: Some("hub".into()),
            });
        } else if let Some(hub) = self.hub {
            if !self.satellites_requested {
                self.satellites_requested = true;
                for (i, s) in self.satellites.iter().enumerate() {
                    commands.push(Command::Spawn {
                        temp_id: 2 + i as u64,
                        transform: satellite_local(s, self.time),
                        parent: Some(hub),
                        name: None,
                    });
                }
            }
            commands.push(Command::SetTransform {
                entity: hub,
                transform: hub_local(self.time),
            });
            for s in &self.satellites {
                if let Some(e) = s.entity {
                    commands.push(Command::SetTransform {
                        entity: e,
                        transform: satellite_local(s, self.time),
                    });
                }
            }
        }
        if !commands.is_empty() {
            world.submit(Lane::Simulation, commands);
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
