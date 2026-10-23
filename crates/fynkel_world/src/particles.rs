//! Particle systems (`docs/stage-12.md` phases 1 and 2): a
//! [`ParticleSystem`] on an object emits particles from a shape, and
//! [`World::step_particles`] moves every system's once a frame, as Unity's
//! Particle System does, with its modules: velocity, force and rotation
//! over a particle's life, a speed limit and drag, size and colour by
//! speed, noise, and frames of a texture sheet.
//!
//! The game sets a system's [`ParticleSettings`] when they change, not
//! each frame; a frame only steps. Each system is a fixed pool of
//! `max_particles`, kept as arrays (all positions, then all velocities, and
//! so on), so a steady frame allocates nothing. Curves and gradients over
//! a particle's life come as tables of [`TABLE`] steps, read with a lerp;
//! a [`Curve`] between two of them, or two constants, picks for each
//! particle by a random number of its own, as Unity's *Random Between
//! Two Curves*.
//!
//! A system plays a timeline of `duration` seconds, looping or not: it
//! emits at `rate_over_time` and in its bursts while that runs. Its
//! particles move whether it's emitting or not, until they die; stopped,
//! it emits no more and its particles live out their lives. Paused,
//! nothing moves. Once it's stopped and its last particle is gone, it's
//! done: [`ParticleEvent`] says so once, for its stop action.
//!
//! Shapes face up (+Y), so a system on an object that isn't turned emits
//! upwards. (Unity's face +Z, and Unity turns a new Particle System's
//! object to point it up; code makes objects unturned, so up is the same
//! start.) A system in world space leaves its particles where they were
//! made as its object moves; in local space they move with it.
//!
//! An object that's hidden (inactive, or below an inactive object) stops
//! its system and clears it, as Unity does when an object is turned off.
//! Shown again, a system that plays on awake plays from the start, so a
//! pooled explosion plays each time it's used.

use glam::{Mat4, Vec3};

use crate::world::{Result, World, WorldError};
use crate::{Color, MaterialId, MeshId, ObjectId};

/// Steps in a curve or gradient over a particle's life.
pub const TABLE: usize = 64;

/// The most bursts a system has.
pub const MAX_BURSTS: usize = 8;

/// The most particles one system holds.
pub const MAX_PARTICLES: u32 = 100_000;

/// Gravity at `gravity_modifier` 1, in metres a second a second, down.
pub const GRAVITY: f32 = 9.81;

/// A number for each particle: `min` when it's the same for all, else a
/// random one from `min` to `max` (Unity's *Random Between Two Constants*).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
    pub min: f32,
    pub max: f32,
}

impl Range {
    pub const fn constant(value: f32) -> Self {
        Self { min: value, max: value }
    }

    fn pick(self, rng: &mut Rng) -> f32 {
        if self.min == self.max { self.min } else { self.min + (self.max - self.min) * rng.unit() }
    }

    fn is_finite(self) -> bool {
        self.min.is_finite() && self.max.is_finite()
    }
}

/// Where a system's particles live: moved with its object, or left where
/// they were made.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Space {
    #[default]
    Local,
    World,
}

/// What happens to a system's object when the system is done: stopped,
/// with no particle left.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StopAction {
    #[default]
    None,
    /// The object is turned off.
    Disable,
    /// The object is destroyed.
    Destroy,
    /// The game is told.
    Callback,
}

/// The shape particles are made in, and which way they go.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShapeKind {
    /// From the object's position, every way.
    #[default]
    Point,
    /// In a ball, outwards.
    Sphere,
    /// In the top half of a ball, outwards.
    Hemisphere,
    /// From a disc, up and out by `angle`.
    Cone,
    /// In a box, up.
    Box,
    /// In a flat ring (x and z), outwards.
    Circle,
    /// Along a line across (x), up.
    Edge,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub kind: ShapeKind,
    /// A sphere's, hemisphere's, circle's or cone's base's radius; half an
    /// edge's length.
    pub radius: f32,
    /// How much of the radius makes particles, from the rim in: 0 on the
    /// rim (the surface) alone, 1 all through.
    pub radius_thickness: f32,
    /// A cone's angle from up to its side, in radians, 0 to just under 90°.
    pub angle: f32,
    /// How much of a circle or cone goes round, in radians, up to 2π.
    pub arc: f32,
    /// A box's size.
    pub size: Vec3,
    /// How far each particle's direction is turned somewhere at random: 0
    /// not at all, 1 any way.
    pub random_direction: f32,
}

impl Default for Shape {
    fn default() -> Self {
        Self {
            kind: ShapeKind::Point,
            radius: 1.0,
            radius_thickness: 1.0,
            angle: 25f32.to_radians(),
            arc: std::f32::consts::TAU,
            size: Vec3::ONE,
            random_direction: 0.0,
        }
    }
}

/// So many particles at once, `time` seconds into the system's timeline,
/// then `cycles` times in all (0 for every `interval` while it plays),
/// each time with `probability` 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Burst {
    pub time: f32,
    pub count: Range,
    pub cycles: u32,
    pub interval: f32,
    pub probability: f32,
}

impl Default for Burst {
    fn default() -> Self {
        Self { time: 0.0, count: Range::constant(30.0), cycles: 1, interval: 0.01, probability: 1.0 }
    }
}

/// A number over a particle's life (Unity's `MinMaxCurve`): two constants,
/// or two curves of [`TABLE`] steps from its birth (0) to its death (1).
/// Each particle is somewhere between the two, by a random number of its
/// own that stays the same all its life; the same two are one constant or
/// one curve.
#[derive(Clone, Debug, PartialEq)]
pub enum Curve {
    Constant(Range),
    Curves(Box<[[f32; TABLE]; 2]>),
}

impl Curve {
    pub const fn constant(value: f32) -> Self {
        Curve::Constant(Range::constant(value))
    }

    /// At `t` (0 to 1) through a life, for a particle whose own random
    /// number is `r` (0 to 1).
    fn at(&self, t: f32, r: f32) -> f32 {
        match self {
            Curve::Constant(range) => range.min + (range.max - range.min) * r,
            Curve::Curves(tables) => {
                let a = sample(&tables[0], t);
                a + (sample(&tables[1], t) - a) * r
            }
        }
    }

    fn is_finite(&self) -> bool {
        match self {
            Curve::Constant(range) => range.is_finite(),
            Curve::Curves(tables) => tables.iter().flatten().all(|v| v.is_finite()),
        }
    }

    fn min(&self) -> f32 {
        match self {
            Curve::Constant(range) => range.min.min(range.max),
            Curve::Curves(tables) => tables.iter().flatten().fold(f32::INFINITY, |m, &v| m.min(v)),
        }
    }
}

/// A vector over a particle's life, an axis each, in the system's own
/// space or the world's: velocity or force over lifetime.
#[derive(Clone, Debug, PartialEq)]
pub struct VectorOverLifetime {
    pub x: Curve,
    pub y: Curve,
    pub z: Curve,
    pub space: Space,
}

impl VectorOverLifetime {
    fn at(&self, t: f32, r: [f32; 3]) -> Vec3 {
        Vec3::new(self.x.at(t, r[0]), self.y.at(t, r[1]), self.z.at(t, r[2]))
    }

    fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

/// Unity's *Limit Velocity over Lifetime*: a speed past `limit` is brought
/// down by `dampen` (0 not at all, 1 to the limit at once; as much again
/// each thirtieth of a second, so the same at any frame rate), and `drag`
/// slows every particle, by that much of its speed a second.
#[derive(Clone, Debug, PartialEq)]
pub struct LimitVelocity {
    pub limit: Curve,
    pub dampen: f32,
    pub drag: Curve,
}

/// Something by how fast a particle goes: `table` from `min_speed` (its
/// first step) to `max_speed` (its last), in metres a second.
#[derive(Clone, Debug, PartialEq)]
pub struct BySpeed<T> {
    pub table: Box<[T; TABLE]>,
    pub min_speed: f32,
    pub max_speed: f32,
}

impl<T> BySpeed<T> {
    fn t(&self, speed: f32) -> f32 {
        ((speed - self.min_speed) / (self.max_speed - self.min_speed).max(1e-6)).clamp(0.0, 1.0)
    }
}

/// Unity's *Noise*: each particle wanders through a smooth random field,
/// as smoke curls and embers drift. `strength` is how fast it pushes, in
/// metres a second, over a particle's life; `frequency` how many swirls a
/// metre; `scroll_speed` how fast the field itself changes, a second;
/// `octaves` 1 to 4 layers, each finer and half as strong.
#[derive(Clone, Debug, PartialEq)]
pub struct Noise {
    pub strength: Curve,
    pub frequency: f32,
    pub scroll_speed: f32,
    pub octaves: u32,
}

/// Unity's *Texture Sheet Animation*: the material's texture is a grid of
/// `columns` × `rows` frames, read left to right and top to bottom, and
/// each particle shows one. `frame_over_time` says which through its life,
/// 0 the first frame and 1 past the last, `cycles` times a life; it starts
/// `start_frame` frames on. With `random_row`, each particle plays one row
/// of `columns` frames, chosen at random.
#[derive(Clone, Debug, PartialEq)]
pub struct TextureSheet {
    pub columns: u32,
    pub rows: u32,
    pub frame_over_time: Curve,
    pub start_frame: Range,
    pub cycles: f32,
    pub random_row: bool,
}

impl Default for TextureSheet {
    /// Every frame, once over a life.
    fn default() -> Self {
        let mut ramp = [0.0; TABLE];
        for (i, v) in ramp.iter_mut().enumerate() {
            *v = i as f32 / (TABLE - 1) as f32;
        }
        Self {
            columns: 1,
            rows: 1,
            frame_over_time: Curve::Curves(Box::new([ramp; 2])),
            start_frame: Range::constant(0.0),
            cycles: 1.0,
            random_row: false,
        }
    }
}

/// How each particle is drawn (Unity's renderer module).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum RenderMode {
    /// A square facing the camera.
    #[default]
    Billboard,
    /// Facing the camera, but long the way it moves: `length_scale` times
    /// its size, plus `speed_scale` times its speed (sparks, rain).
    Stretched { length_scale: f32, speed_scale: f32 },
    /// Lying flat, facing up (a ripple, a ring on the ground).
    Horizontal,
    /// Standing up, turned round its upright to face the camera.
    Vertical,
    /// A mesh, its size across a metre of the mesh, turned by its rotation
    /// round up (debris, coins).
    Mesh(MeshId),
}

/// Everything a game sets on a system: Unity's main, emission, shape and
/// over-lifetime modules, its noise and texture sheet, and how it's drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleSettings {
    /// How long the timeline is, in seconds.
    pub duration: f32,
    pub looping: bool,
    /// A looping system starts as if it had played a whole `duration`.
    pub prewarm: bool,
    /// Seconds from play to the first particle.
    pub start_delay: f32,
    pub start_lifetime: Range,
    pub start_speed: Range,
    pub start_size: Range,
    /// Radians.
    pub start_rotation: Range,
    /// A colour for each particle: between the two at random, the same
    /// when they are. sRGB, as picked.
    pub start_color: [Color; 2],
    /// Times [`GRAVITY`].
    pub gravity_modifier: f32,
    pub space: Space,
    /// How fast its time goes, times the game's.
    pub simulation_speed: f32,
    /// Its time goes on while the game's is paused or slowed.
    pub unscaled_time: bool,
    pub play_on_awake: bool,
    pub max_particles: u32,
    pub stop_action: StopAction,
    /// The same particles each play, or `None` for new ones each play.
    pub seed: Option<u32>,
    /// Particles a second while the timeline runs.
    pub rate_over_time: f32,
    pub bursts: Vec<Burst>,
    pub shape: Shape,
    /// A particle's size times this, over its life.
    pub size_over_lifetime: Option<Box<[f32; TABLE]>>,
    /// A particle's colour times this, over its life: sRGB, as picked.
    pub color_over_lifetime: Option<Box<[Color; TABLE]>>,
    /// Added to its velocity, over its life, without changing it.
    pub velocity_over_lifetime: Option<VectorOverLifetime>,
    pub limit_velocity: Option<LimitVelocity>,
    /// Added to its velocity each second, over its life.
    pub force_over_lifetime: Option<VectorOverLifetime>,
    /// How fast it turns, in radians a second, over its life.
    pub rotation_over_lifetime: Option<Curve>,
    /// Its size times this, by its speed.
    pub size_by_speed: Option<BySpeed<f32>>,
    /// Its colour times this, by its speed: sRGB, as picked.
    pub color_by_speed: Option<BySpeed<Color>>,
    pub noise: Option<Noise>,
    pub texture_sheet: Option<TextureSheet>,
    /// What it's drawn with; nothing is drawn without one.
    pub material: Option<MaterialId>,
    pub render_mode: RenderMode,
    /// Lit as a surface facing the camera, rather than shown as it is.
    pub lit: bool,
    /// How far, in metres, a particle fades out in front of what's behind
    /// it (Unity's soft particles); 0 for a hard edge.
    pub softness: f32,
}

impl Default for ParticleSettings {
    /// Unity's: five seconds looping, ten a second for five seconds each,
    /// at 5 m/s, a size of 1, white, from a cone.
    fn default() -> Self {
        Self {
            duration: 5.0,
            looping: true,
            prewarm: false,
            start_delay: 0.0,
            start_lifetime: Range::constant(5.0),
            start_speed: Range::constant(5.0),
            start_size: Range::constant(1.0),
            start_rotation: Range::constant(0.0),
            start_color: [Color::WHITE; 2],
            gravity_modifier: 0.0,
            space: Space::Local,
            simulation_speed: 1.0,
            unscaled_time: false,
            play_on_awake: true,
            max_particles: 1000,
            stop_action: StopAction::None,
            seed: None,
            rate_over_time: 10.0,
            bursts: Vec::new(),
            shape: Shape { kind: ShapeKind::Cone, ..Shape::default() },
            size_over_lifetime: None,
            color_over_lifetime: None,
            velocity_over_lifetime: None,
            limit_velocity: None,
            force_over_lifetime: None,
            rotation_over_lifetime: None,
            size_by_speed: None,
            color_by_speed: None,
            noise: None,
            texture_sheet: None,
            material: None,
            render_mode: RenderMode::Billboard,
            lit: false,
            softness: 0.0,
        }
    }
}

impl ParticleSettings {
    /// Why these can't be, if they can't.
    pub fn refused(&self) -> Option<&'static str> {
        let finite = [
            self.duration,
            self.start_delay,
            self.gravity_modifier,
            self.simulation_speed,
            self.rate_over_time,
            self.shape.radius,
            self.shape.radius_thickness,
            self.shape.angle,
            self.shape.arc,
            self.shape.random_direction,
        ]
        .iter()
        .all(|v| v.is_finite())
            && self.shape.size.is_finite()
            && [self.start_lifetime, self.start_speed, self.start_size, self.start_rotation]
                .iter()
                .all(|r| r.is_finite());
        if !finite {
            return Some("a particle system's numbers must be finite");
        }
        if self.duration < 0.05 {
            return Some("a particle system's duration is at least 0.05 s");
        }
        if self.start_lifetime.min < 0.0 || self.start_lifetime.max < 0.0 {
            return Some("a particle's start lifetime can't be below 0");
        }
        if self.start_delay < 0.0 || self.rate_over_time < 0.0 || self.simulation_speed < 0.0 {
            return Some("a particle system's start delay, rate over time and simulation speed can't be below 0");
        }
        if !(1..=MAX_PARTICLES).contains(&self.max_particles) {
            return Some("a particle system holds 1 to 100,000 particles");
        }
        if self.bursts.len() > MAX_BURSTS {
            return Some("a particle system has at most 8 bursts");
        }
        for b in &self.bursts {
            let ok = b.time.is_finite()
                && b.time >= 0.0
                && b.count.is_finite()
                && b.count.min >= 0.0
                && b.count.max >= 0.0
                && b.interval.is_finite()
                && b.interval >= 0.01
                && (0.0..=1.0).contains(&b.probability);
            if !ok {
                return Some(
                    "a burst's time and count can't be below 0, its interval is at least 0.01 s, and its \
                     probability is 0 to 1",
                );
            }
        }
        let s = &self.shape;
        if s.radius < 0.0 || s.size.min_element() < 0.0 {
            return Some("a particle system's shape can't be smaller than nothing");
        }
        if !(0.0..=1.0).contains(&s.radius_thickness) || !(0.0..=1.0).contains(&s.random_direction) {
            return Some("a shape's radius thickness and random direction are 0 to 1");
        }
        if !(0.0..std::f32::consts::FRAC_PI_2).contains(&s.angle) {
            return Some("a cone's angle is 0 to under 90°");
        }
        if !(0.0..=std::f32::consts::TAU + 1e-4).contains(&s.arc) {
            return Some("a shape's arc is 0 to 360°");
        }
        let tables = self.size_over_lifetime.as_ref().is_none_or(|t| t.iter().all(|v| v.is_finite()))
            && self
                .color_over_lifetime
                .as_ref()
                .is_none_or(|t| t.iter().all(|c| [c.r, c.g, c.b, c.a].iter().all(|v| v.is_finite())));
        if !tables {
            return Some("a particle system's curves must be finite");
        }
        let curves = self.velocity_over_lifetime.as_ref().is_none_or(VectorOverLifetime::is_finite)
            && self.force_over_lifetime.as_ref().is_none_or(VectorOverLifetime::is_finite)
            && self.rotation_over_lifetime.as_ref().is_none_or(Curve::is_finite)
            && self
                .limit_velocity
                .as_ref()
                .is_none_or(|l| l.limit.is_finite() && l.drag.is_finite() && l.dampen.is_finite())
            && self.size_by_speed.as_ref().is_none_or(|b| {
                b.table.iter().all(|v| v.is_finite()) && b.min_speed.is_finite() && b.max_speed.is_finite()
            })
            && self.color_by_speed.as_ref().is_none_or(|b| {
                b.table.iter().all(|c| [c.r, c.g, c.b, c.a].iter().all(|v| v.is_finite()))
                    && b.min_speed.is_finite()
                    && b.max_speed.is_finite()
            })
            && self
                .noise
                .as_ref()
                .is_none_or(|n| n.strength.is_finite() && n.frequency.is_finite() && n.scroll_speed.is_finite())
            && self
                .texture_sheet
                .as_ref()
                .is_none_or(|t| t.frame_over_time.is_finite() && t.start_frame.is_finite() && t.cycles.is_finite())
            && self.softness.is_finite()
            && match self.render_mode {
                RenderMode::Stretched { length_scale, speed_scale } => {
                    length_scale.is_finite() && speed_scale.is_finite()
                }
                _ => true,
            };
        if !curves {
            return Some("a particle system's curves must be finite");
        }
        if let Some(l) = &self.limit_velocity
            && (!(0.0..=1.0).contains(&l.dampen) || l.limit.min() < 0.0 || l.drag.min() < 0.0)
        {
            return Some("a speed limit and drag can't be below 0, and dampen is 0 to 1");
        }
        let speeds = |min: f32, max: f32| min >= 0.0 && max > min;
        if self.size_by_speed.as_ref().is_some_and(|b| !speeds(b.min_speed, b.max_speed))
            || self.color_by_speed.as_ref().is_some_and(|b| !speeds(b.min_speed, b.max_speed))
        {
            return Some("a by-speed range goes from 0 or more up to a higher speed");
        }
        if let Some(n) = &self.noise
            && (n.frequency <= 0.0 || !(1..=4).contains(&n.octaves))
        {
            return Some("noise has a frequency over 0 and 1 to 4 octaves");
        }
        if let Some(t) = &self.texture_sheet
            && (!(1..=64).contains(&t.columns)
                || !(1..=64).contains(&t.rows)
                || t.cycles <= 0.0
                || t.start_frame.min < 0.0
                || t.start_frame.max < 0.0)
        {
            return Some("a texture sheet has 1 to 64 columns and rows, cycles over 0, and a start frame of 0 or more");
        }
        if self.softness < 0.0 {
            return Some("softness can't be below 0");
        }
        if let RenderMode::Stretched { length_scale, speed_scale } = self.render_mode
            && (length_scale < 0.0 || speed_scale < 0.0)
        {
            return Some("a stretched particle's length scale and speed scale can't be below 0");
        }
        None
    }
}

/// What a system did this frame that the game hears of: it's done
/// (stopped, its last particle gone), and its stop action is due.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParticleEvent {
    pub object: ObjectId,
    pub action: StopAction,
}

/// What a system is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// Its timeline runs.
    Playing,
    /// Nothing moves.
    Paused,
    /// Emits nothing; its particles live out their lives.
    Stopped,
}

/// A small fast random number generator (xorshift64*): each system has
/// its own, so a seed gives the same particles each play.
#[derive(Clone, Debug)]
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        // Mixed, so near seeds start far apart, and never 0.
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        Self((z ^ (z >> 31)) | 1)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// 0 to under 1.
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// A direction, every way alike.
    fn direction(&mut self) -> Vec3 {
        let z = self.unit() * 2.0 - 1.0;
        let a = self.unit() * std::f32::consts::TAU;
        let r = (1.0 - z * z).max(0.0).sqrt();
        Vec3::new(r * a.cos(), z, r * a.sin())
    }
}

/// One particle as it's drawn, in the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawnParticle {
    pub position: Vec3,
    pub size: f32,
    /// Linear RGBA.
    pub color: [f32; 4],
    /// Radians, round the way to the camera.
    pub rotation: f32,
    /// How it moves, its velocity over lifetime too: the way a stretched
    /// particle is long.
    pub velocity: Vec3,
    /// Its texture sheet frame, 0 without one.
    pub frame: u32,
    /// How far through its life, 0 to 1.
    pub t: f32,
    /// A number of its own, 0 to 1, the same all its life: a shader's
    /// `particle_random()`.
    pub random: f32,
}

/// A particle system: its settings, and its particles.
#[derive(Clone, Debug)]
pub struct ParticleSystem {
    settings: ParticleSettings,
    /// Linear colours: the start's two, and the table's.
    start_linear: [[f32; 4]; 2],
    color_table: Option<Box<[[f32; 4]; TABLE]>>,
    speed_color_table: Option<Box<[[f32; 4]; TABLE]>>,
    // The particles, `count` of each.
    position: Vec<Vec3>,
    velocity: Vec<Vec3>,
    age: Vec<f32>,
    lifetime: Vec<f32>,
    size: Vec<f32>,
    rotation: Vec<f32>,
    color: Vec<[f32; 4]>,
    /// Each particle's own seed, for its picks along curves.
    random: Vec<u32>,
    count: usize,
    playing: State,
    /// Whether its timeline is emitting: playing, and not past the end of
    /// a timeline that doesn't loop.
    emitting: bool,
    /// Seconds into the timeline's cycle.
    time: f32,
    /// Seconds it has run: where the noise field has scrolled to.
    noise_time: f32,
    /// Seconds left of the start delay.
    delay: f32,
    /// The part of a particle owed by the rate.
    carry: f32,
    /// How many times each burst went off in this cycle.
    fired: [u32; MAX_BURSTS],
    rng: Rng,
    /// Plays since it was made: an automatic seed's next.
    plays: u64,
    /// Whether it's done, and was told.
    done: bool,
    /// Whether its object was shown at the last step; `None` before the
    /// first.
    shown: Option<bool>,
}

impl ParticleSystem {
    fn new(settings: ParticleSettings, seed: u64) -> Self {
        let mut s = Self {
            start_linear: [[1.0; 4]; 2],
            color_table: None,
            speed_color_table: None,
            settings: ParticleSettings::default(),
            position: Vec::new(),
            velocity: Vec::new(),
            age: Vec::new(),
            lifetime: Vec::new(),
            size: Vec::new(),
            rotation: Vec::new(),
            color: Vec::new(),
            random: Vec::new(),
            count: 0,
            playing: State::Stopped,
            emitting: false,
            time: 0.0,
            noise_time: 0.0,
            delay: 0.0,
            carry: 0.0,
            fired: [0; MAX_BURSTS],
            rng: Rng::new(seed),
            plays: seed,
            done: true,
            shown: None,
        };
        s.set(settings);
        s
    }

    pub fn settings(&self) -> &ParticleSettings {
        &self.settings
    }

    /// Changes its settings, keeping its particles and where it is in its
    /// timeline. Particles past a smaller `max_particles` go.
    fn set(&mut self, settings: ParticleSettings) {
        self.start_linear = settings.start_color.map(Color::to_linear);
        self.color_table = settings.color_over_lifetime.as_ref().map(|t| Box::new(t.map(Color::to_linear)));
        self.speed_color_table = settings.color_by_speed.as_ref().map(|b| Box::new(b.table.map(Color::to_linear)));
        let max = settings.max_particles as usize;
        self.count = self.count.min(max);
        for v in [&mut self.age, &mut self.lifetime, &mut self.size, &mut self.rotation] {
            v.resize(max, 0.0);
        }
        self.position.resize(max, Vec3::ZERO);
        self.velocity.resize(max, Vec3::ZERO);
        self.color.resize(max, [1.0; 4]);
        self.random.resize(max, 0);
        self.settings = settings;
    }

    /// How many particles it has.
    pub fn count(&self) -> usize {
        self.count
    }

    /// Unity's `isPlaying`: from play until it's stopped and its last
    /// particle is gone, not while it's paused. (Stopped and cleared, it's
    /// done at its next step, which tells of it.)
    pub fn is_playing(&self) -> bool {
        match self.playing {
            State::Playing => !self.done,
            State::Paused => false,
            State::Stopped => !self.done && self.count > 0,
        }
    }

    pub fn is_paused(&self) -> bool {
        self.playing == State::Paused
    }

    pub fn is_emitting(&self) -> bool {
        self.emitting && self.playing == State::Playing
    }

    /// Plays its timeline from the start, keeping the particles it has; a
    /// paused system goes on from where it was.
    fn play(&mut self) {
        if self.playing == State::Paused {
            self.playing = State::Playing;
            return;
        }
        self.playing = State::Playing;
        self.emitting = true;
        self.done = false;
        self.time = 0.0;
        self.delay = self.settings.start_delay;
        self.carry = 0.0;
        self.fired = [0; MAX_BURSTS];
        self.plays += 1;
        self.rng = Rng::new(match self.settings.seed {
            Some(seed) => u64::from(seed),
            None => self.plays.wrapping_mul(0x9E37_79B9),
        });
    }

    /// Plays, then moves it a whole timeline on, for one that's prewarmed.
    fn play_from_start(&mut self, emitter: Mat4) {
        self.play();
        if self.settings.prewarm && self.settings.looping {
            let delay = std::mem::take(&mut self.delay);
            let steps = (self.settings.duration * 30.0).ceil().max(1.0) as u32;
            let dt = self.settings.duration / steps as f32;
            for _ in 0..steps {
                self.step(dt / self.settings.simulation_speed.max(1e-6), emitter);
            }
            self.delay = delay;
        }
    }

    /// Stops emitting; with `clear`, its particles go too.
    fn stop(&mut self, clear: bool) {
        self.playing = State::Stopped;
        self.emitting = false;
        if clear {
            self.count = 0;
        }
    }

    fn pause(&mut self) {
        if !self.done {
            self.playing = State::Paused;
        }
    }

    fn clear(&mut self) {
        self.count = 0;
    }

    /// Makes `n` particles now, as its shape would, played or not.
    fn emit(&mut self, n: u32, emitter: Mat4) {
        for _ in 0..n {
            self.spawn(emitter, 0.0);
        }
        if self.count > 0 {
            self.done = false;
        }
    }

    /// Moves it `dt` seconds on (its own time: the game's times its
    /// simulation speed is applied here), its object where `emitter` puts
    /// it. Returns whether it just became done.
    fn step(&mut self, dt: f32, emitter: Mat4) -> bool {
        if self.playing == State::Paused || self.done {
            return false;
        }
        let mut dt = dt * self.settings.simulation_speed;
        if dt <= 0.0 {
            return false;
        }
        // Gravity is the world's down, in a local system's own space too.
        let mut gravity = Vec3::new(0.0, -GRAVITY * self.settings.gravity_modifier, 0.0);
        if self.settings.space == Space::Local {
            gravity = emitter.inverse().transform_vector3(gravity);
        }
        // A module in the other space than the particles' is turned into
        // theirs.
        let into = |space: Space| match (space, self.settings.space) {
            (Space::Local, Space::World) => Some(emitter),
            (Space::World, Space::Local) => Some(emitter.inverse()),
            _ => None,
        };
        let force_into = self.settings.force_over_lifetime.as_ref().and_then(|f| into(f.space));
        let velocity_into = self.settings.velocity_over_lifetime.as_ref().and_then(|v| into(v.space));
        self.noise_time += dt;
        // Out of the way while the particles change; nothing below sets it.
        let s = std::mem::take(&mut self.settings);
        // How much of a speed past the limit is kept this step: the same
        // over a second at any frame rate.
        let kept = s.limit_velocity.as_ref().map_or(1.0, |l| (1.0 - l.dampen).powf(dt * 30.0));
        let mut i = 0;
        while i < self.count {
            self.age[i] += dt;
            if self.age[i] >= self.lifetime[i] {
                self.kill(i);
                continue;
            }
            let t = self.age[i] / self.lifetime[i];
            let seed = self.random[i];
            let mut v = self.velocity[i];
            if let Some(force) = &s.force_over_lifetime {
                let mut f = force.at(t, [pick(seed, 4), pick(seed, 5), pick(seed, 6)]);
                if let Some(m) = force_into {
                    f = m.transform_vector3(f);
                }
                v += f * dt;
            }
            v += gravity * dt;
            if let Some(limit) = &s.limit_velocity {
                let most = limit.limit.at(t, pick(seed, 7));
                let speed = v.length();
                if speed > most {
                    v *= (most + (speed - most) * kept) / speed;
                }
                v *= (-limit.drag.at(t, pick(seed, 8)) * dt).exp();
            }
            self.velocity[i] = v;
            if let Some(over) = &s.velocity_over_lifetime {
                let mut extra = over.at(t, [pick(seed, 1), pick(seed, 2), pick(seed, 3)]);
                if let Some(m) = velocity_into {
                    extra = m.transform_vector3(extra);
                }
                v += extra;
            }
            if let Some(noise) = &s.noise {
                v += noise_at(noise, self.position[i], self.noise_time) * noise.strength.at(t, pick(seed, 9));
            }
            self.position[i] += v * dt;
            if let Some(turn) = &s.rotation_over_lifetime {
                self.rotation[i] += turn.at(t, pick(seed, 10)) * dt;
            }
            i += 1;
        }
        self.settings = s;
        if self.playing == State::Playing && self.emitting {
            if self.delay > 0.0 {
                self.delay -= dt;
                if self.delay > 0.0 {
                    return false;
                }
                dt = -self.delay;
                self.delay = 0.0;
            }
            self.run_timeline(dt, emitter);
        }
        if !self.emitting && self.count == 0 {
            self.done = true;
            self.playing = State::Stopped;
            return true;
        }
        false
    }

    /// Emits what its timeline does over `dt` seconds, at the end of which
    /// each particle made is as old as it would be.
    fn run_timeline(&mut self, mut dt: f32, emitter: Mat4) {
        let duration = self.settings.duration;
        while dt > 0.0 {
            let span = dt.min(duration - self.time);
            let (from, to) = (self.time, self.time + span);
            // The rate's, evenly through the span.
            self.carry += self.settings.rate_over_time * span;
            let n = self.carry.floor();
            self.carry -= n;
            let n = n as u32;
            for k in 0..n {
                let born = from + span * (k as f32 + 0.5) / n as f32;
                self.spawn(emitter, to - born + (dt - span));
            }
            for b in 0..self.settings.bursts.len() {
                let burst = self.settings.bursts[b];
                loop {
                    let fired = self.fired[b];
                    if burst.cycles != 0 && fired >= burst.cycles {
                        break;
                    }
                    let at = burst.time + fired as f32 * burst.interval;
                    if at >= to {
                        break;
                    }
                    self.fired[b] += 1;
                    if burst.probability < 1.0 && self.rng.unit() >= burst.probability {
                        continue;
                    }
                    let count = burst.count.pick(&mut self.rng).round() as u32;
                    for _ in 0..count {
                        self.spawn(emitter, (to - at.max(from)) + (dt - span));
                    }
                }
            }
            self.time = to;
            dt -= span;
            if self.time >= duration - 1e-6 {
                if self.settings.looping {
                    self.time = 0.0;
                    self.fired = [0; MAX_BURSTS];
                } else {
                    self.emitting = false;
                    return;
                }
            }
        }
    }

    /// Makes one particle, `age` seconds old already, if there's room.
    fn spawn(&mut self, emitter: Mat4, age: f32) {
        if self.count >= self.settings.max_particles as usize {
            return;
        }
        let s = &self.settings;
        let rng = &mut self.rng;
        let lifetime = s.start_lifetime.pick(rng);
        if lifetime <= age || lifetime <= 0.0 {
            return;
        }
        let (mut position, mut direction) = shape_point(&s.shape, rng);
        if s.shape.random_direction > 0.0 {
            direction = direction.lerp(rng.direction(), s.shape.random_direction).normalize_or(Vec3::Y);
        }
        let mut velocity = direction * s.start_speed.pick(rng);
        if s.space == Space::World {
            position = emitter.transform_point3(position);
            velocity = emitter.transform_vector3(velocity);
        }
        let i = self.count;
        self.count += 1;
        self.position[i] = position + velocity * age;
        self.velocity[i] = velocity;
        self.age[i] = age;
        self.lifetime[i] = lifetime;
        self.size[i] = s.start_size.pick(rng);
        self.rotation[i] = s.start_rotation.pick(rng);
        self.random[i] = (rng.next() >> 32) as u32;
        let [a, b] = self.start_linear;
        self.color[i] = if a == b {
            a
        } else {
            let t = rng.unit();
            std::array::from_fn(|c| a[c] + (b[c] - a[c]) * t)
        };
    }

    fn kill(&mut self, i: usize) {
        let last = self.count - 1;
        self.position[i] = self.position[last];
        self.velocity[i] = self.velocity[last];
        self.age[i] = self.age[last];
        self.lifetime[i] = self.lifetime[last];
        self.size[i] = self.size[last];
        self.rotation[i] = self.rotation[last];
        self.color[i] = self.color[last];
        self.random[i] = self.random[last];
        self.count = last;
    }

    /// Its particles as they're drawn, in the world, its object at
    /// `emitter`; and the box round them, if it has any.
    pub fn drawn(&self, emitter: Mat4, out: &mut Vec<DrawnParticle>) -> Option<(Vec3, Vec3)> {
        if self.count == 0 {
            return None;
        }
        let local = self.settings.space == Space::Local;
        let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        let s = &self.settings;
        let sizes = s.size_over_lifetime.as_deref();
        let colors = self.color_table.as_deref();
        let speed_colors = self.speed_color_table.as_deref();
        // The world's velocity over lifetime, in the particles' space.
        let over_into = s.velocity_over_lifetime.as_ref().and_then(|v| match (v.space, s.space) {
            (Space::World, Space::Local) => Some(emitter.inverse()),
            (Space::Local, Space::World) => Some(emitter),
            _ => None,
        });
        // How far a stretched particle reaches past its size.
        let stretch = match s.render_mode {
            RenderMode::Stretched { length_scale, speed_scale } => Some((length_scale, speed_scale)),
            _ => None,
        };
        out.reserve(self.count);
        for i in 0..self.count {
            let t = (self.age[i] / self.lifetime[i]).clamp(0.0, 1.0);
            let seed = self.random[i];
            let mut velocity = self.velocity[i];
            if let Some(over) = &s.velocity_over_lifetime {
                let mut extra = over.at(t, [pick(seed, 1), pick(seed, 2), pick(seed, 3)]);
                if let Some(m) = over_into {
                    extra = m.transform_vector3(extra);
                }
                velocity += extra;
            }
            let (position, velocity) = if local {
                (emitter.transform_point3(self.position[i]), emitter.transform_vector3(velocity))
            } else {
                (self.position[i], velocity)
            };
            let speed = velocity.length();
            let mut size = self.size[i] * sizes.map_or(1.0, |table| sample(table, t));
            if let Some(by) = &s.size_by_speed {
                size *= sample(&by.table, by.t(speed));
            }
            let mut color = self.color[i];
            if let Some(table) = colors {
                let over = sample_color(table, t);
                for c in 0..4 {
                    color[c] *= over[c];
                }
            }
            if let (Some(table), Some(by)) = (speed_colors, &s.color_by_speed) {
                let over = sample_color(table, by.t(speed));
                for c in 0..4 {
                    color[c] *= over[c];
                }
            }
            let mut half = size * 0.5;
            if let Some((length, by_speed)) = stretch {
                half = half.max(size * (length + by_speed * speed) * 0.5);
            }
            lo = lo.min(position - Vec3::splat(half));
            hi = hi.max(position + Vec3::splat(half));
            let frame = s.texture_sheet.as_ref().map_or(0, |sheet| sheet_frame(sheet, t, seed));
            out.push(DrawnParticle {
                position,
                size,
                color,
                rotation: self.rotation[i],
                velocity,
                frame,
                t,
                random: pick(seed, 0),
            });
        }
        Some((lo, hi))
    }
}

/// A particle's own number for use `k`, 0 to under 1: a hash of its seed,
/// so it costs nothing to keep and is the same each frame.
fn pick(seed: u32, k: u32) -> f32 {
    let mut x = seed ^ k.wrapping_mul(0x9E37_79B9);
    x = (x ^ (x >> 16)).wrapping_mul(0x7FEB_352D);
    x = (x ^ (x >> 15)).wrapping_mul(0x846C_A68B);
    x ^= x >> 16;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// Which frame of `sheet` a particle shows, `t` through its life.
fn sheet_frame(sheet: &TextureSheet, t: f32, seed: u32) -> u32 {
    let (columns, rows) = (sheet.columns, sheet.rows);
    let frames = if sheet.random_row { columns } else { columns * rows };
    let cycle = (t * sheet.cycles).fract();
    let start = sheet.start_frame.min + (sheet.start_frame.max - sheet.start_frame.min) * pick(seed, 11);
    let at = sheet.frame_over_time.at(cycle, pick(seed, 12)).clamp(0.0, 1.0) * frames as f32 + start;
    let frame = (at.floor() as u32).min(u32::MAX - 1) % frames;
    if sheet.random_row {
        let row = ((pick(seed, 13) * rows as f32) as u32).min(rows - 1);
        row * columns + frame
    } else {
        frame
    }
}

/// A smooth random vector at `p`, each axis -1 to 1: values at the
/// corners of a unit grid, blended. One hash a corner gives all three.
fn value_noise(p: Vec3) -> Vec3 {
    let corner = |x: i32, y: i32, z: i32| {
        let mut h = (x as u32).wrapping_mul(0x8DA6_B343)
            ^ (y as u32).wrapping_mul(0xD816_3841)
            ^ (z as u32).wrapping_mul(0xCB1A_B31F);
        h = (h ^ (h >> 16)).wrapping_mul(0x7FEB_352D);
        h = (h ^ (h >> 15)).wrapping_mul(0x846C_A68B);
        h ^= h >> 16;
        let axis = |shift: u32| ((h >> shift) & 0x3FF) as f32 * (2.0 / 1023.0) - 1.0;
        Vec3::new(axis(0), axis(11), axis(22))
    };
    let f = p.floor();
    let (x, y, z) = (f.x as i32, f.y as i32, f.z as i32);
    let d = p - f;
    let w = d * d * (Vec3::splat(3.0) - 2.0 * d);
    let x00 = corner(x, y, z).lerp(corner(x + 1, y, z), w.x);
    let x10 = corner(x, y + 1, z).lerp(corner(x + 1, y + 1, z), w.x);
    let x01 = corner(x, y, z + 1).lerp(corner(x + 1, y, z + 1), w.x);
    let x11 = corner(x, y + 1, z + 1).lerp(corner(x + 1, y + 1, z + 1), w.x);
    x00.lerp(x10, w.y).lerp(x01.lerp(x11, w.y), w.z)
}

/// The noise module's push at `p`, `time` seconds in, before its strength:
/// its octaves finer and fainter.
fn noise_at(noise: &Noise, p: Vec3, time: f32) -> Vec3 {
    let mut q = p * noise.frequency + Vec3::splat(time * noise.scroll_speed);
    let (mut out, mut weight, mut total) = (Vec3::ZERO, 1.0, 0.0);
    for _ in 0..noise.octaves {
        out += weight * value_noise(q);
        total += weight;
        weight *= 0.5;
        // Each octave off to one side, so they don't line up at the origin.
        q = q * 2.0 + Vec3::new(17.3, 31.7, 47.1);
    }
    out / total
}

/// `table` at `t` (0 to 1), between its steps.
fn sample(table: &[f32; TABLE], t: f32) -> f32 {
    let x = t * (TABLE - 1) as f32;
    let i = (x as usize).min(TABLE - 2);
    let f = x - i as f32;
    table[i] + (table[i + 1] - table[i]) * f
}

fn sample_color(table: &[[f32; 4]; TABLE], t: f32) -> [f32; 4] {
    let x = t * (TABLE - 1) as f32;
    let i = (x as usize).min(TABLE - 2);
    let f = x - i as f32;
    std::array::from_fn(|c| table[i][c] + (table[i + 1][c] - table[i][c]) * f)
}

/// Where in `shape` a particle starts, and the way it goes, in the
/// object's space.
fn shape_point(shape: &Shape, rng: &mut Rng) -> (Vec3, Vec3) {
    // How far out from the centre: on the rim, or anywhere in the
    // thickness, evenly by area or volume.
    let out = |rng: &mut Rng, dims: i32| {
        let inner = 1.0 - shape.radius_thickness;
        let u = rng.unit();
        let inner_p = inner.powi(dims);
        (inner_p + (1.0 - inner_p) * u).powf(1.0 / dims as f32)
    };
    let round = |rng: &mut Rng| rng.unit() * shape.arc;
    match shape.kind {
        ShapeKind::Point => (Vec3::ZERO, rng.direction()),
        ShapeKind::Sphere => {
            let d = rng.direction();
            (d * shape.radius * out(rng, 3), d)
        }
        ShapeKind::Hemisphere => {
            let mut d = rng.direction();
            d.y = d.y.abs();
            (d * shape.radius * out(rng, 3), d)
        }
        ShapeKind::Cone => {
            let a = round(rng);
            let r = out(rng, 2);
            let (sin, cos) = a.sin_cos();
            let at = Vec3::new(cos, 0.0, sin) * r;
            // Out from the axis by as much of the angle as it's out from
            // the centre, as Unity's cone.
            let tilt = shape.angle * r;
            let d = Vec3::new(cos * tilt.sin(), tilt.cos(), sin * tilt.sin());
            (at * shape.radius, d)
        }
        ShapeKind::Box => {
            let p = Vec3::new(rng.unit() - 0.5, rng.unit() - 0.5, rng.unit() - 0.5) * shape.size;
            (p, Vec3::Y)
        }
        ShapeKind::Circle => {
            let a = round(rng);
            let (sin, cos) = a.sin_cos();
            let d = Vec3::new(cos, 0.0, sin);
            (d * shape.radius * out(rng, 2), d)
        }
        ShapeKind::Edge => (Vec3::new((rng.unit() * 2.0 - 1.0) * shape.radius, 0.0, 0.0), Vec3::Y),
    }
}

/// What the game asks a system to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParticleCommand {
    Play,
    Stop { clear: bool },
    Pause,
    Clear,
    Emit(u32),
}

impl World {
    /// Gives `object` a particle system with `settings`, or changes the
    /// one it has (keeping its particles). A new one plays if it plays on
    /// awake. Refused for a missing object or material, or settings that
    /// can't be.
    pub fn set_particles(&mut self, object: ObjectId, settings: ParticleSettings) -> Result<()> {
        self.entity(object)?;
        if let Some(material) = settings.material {
            self.material(material)?;
        }
        if let RenderMode::Mesh(mesh) = settings.render_mode {
            self.mesh(mesh)?;
        }
        if let Some(why) = settings.refused() {
            return Err(WorldError::Particles(why));
        }
        match self.particles.get_mut(&object) {
            Some(system) => system.set(settings),
            None => {
                let seed = self.particle_seeds;
                self.particle_seeds = self.particle_seeds.wrapping_add(0x5851_F42D);
                self.particles.insert(object, ParticleSystem::new(settings, seed));
            }
        }
        Ok(())
    }

    /// Takes `object`'s particle system away, its particles too.
    pub fn remove_particles(&mut self, object: ObjectId) -> Result<()> {
        self.entity(object)?;
        self.particles
            .remove(&object)
            .map(|_| ())
            .ok_or(WorldError::NoComponent { object, component: "ParticleSystem" })
    }

    pub fn particles(&self, object: ObjectId) -> Result<&ParticleSystem> {
        self.entity(object)?;
        self.particles.get(&object).ok_or(WorldError::NoComponent { object, component: "ParticleSystem" })
    }

    /// Every particle system, by its object, in no particular order.
    pub fn particle_systems(&self) -> impl Iterator<Item = (ObjectId, &ParticleSystem)> + '_ {
        self.particles.iter().map(|(&id, s)| (id, s))
    }

    pub fn has_particles(&self) -> bool {
        !self.particles.is_empty()
    }

    /// Plays, stops, pauses, clears or emits from `object`'s system.
    pub fn particle_command(&mut self, object: ObjectId, command: ParticleCommand) -> Result<()> {
        self.entity(object)?;
        let emitter = self.world_matrix(object)?;
        let system =
            self.particles.get_mut(&object).ok_or(WorldError::NoComponent { object, component: "ParticleSystem" })?;
        match command {
            ParticleCommand::Play => system.play_from_start(emitter),
            ParticleCommand::Stop { clear } => system.stop(clear),
            ParticleCommand::Pause => system.pause(),
            ParticleCommand::Clear => system.clear(),
            ParticleCommand::Emit(n) => system.emit(n, emitter),
        }
        Ok(())
    }

    /// Moves `object`'s system `seconds` on at once, as if that much time
    /// had passed (Unity's `Simulate`), in steps of at most 1/30 s.
    pub fn simulate_particles(&mut self, object: ObjectId, seconds: f32) -> Result<()> {
        self.entity(object)?;
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(WorldError::Particles("simulate takes seconds of 0 or more"));
        }
        let emitter = self.world_matrix(object)?;
        let system =
            self.particles.get_mut(&object).ok_or(WorldError::NoComponent { object, component: "ParticleSystem" })?;
        let steps = (seconds * 30.0).ceil() as u32;
        for _ in 0..steps {
            system.step(seconds / steps as f32, emitter);
        }
        Ok(())
    }

    /// Moves every system on, once a frame: by `dt` seconds of the game's
    /// time, or `unscaled` for those that ask. A system whose object is
    /// hidden stops and clears; shown again, one that plays on awake
    /// plays. Those that became done are added to `events`.
    pub fn step_particles(&mut self, dt: f32, unscaled: f32, events: &mut Vec<ParticleEvent>) {
        let clean = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
        let (dt, unscaled) = (clean(dt), clean(unscaled));
        let mut systems = std::mem::take(&mut self.particles);
        for (&object, system) in &mut systems {
            let shown = self.active_in_hierarchy(object).unwrap_or(false);
            let Ok(emitter) = self.world_matrix(object) else { continue };
            match (system.shown.replace(shown), shown) {
                // New, shown: it plays on awake, unless it was played
                // already, keeping what it was given.
                (None, true) if system.settings.play_on_awake && system.done => system.play_from_start(emitter),
                // Shown again: from the start.
                (Some(false), true) if system.settings.play_on_awake => {
                    system.clear();
                    system.playing = State::Stopped;
                    system.play_from_start(emitter);
                }
                (Some(true), false) => {
                    system.stop(true);
                    system.done = true;
                }
                _ => {}
            }
            if !shown {
                continue;
            }
            let dt = if system.settings.unscaled_time { unscaled } else { dt };
            if system.step(dt, emitter) && system.settings.stop_action != StopAction::None {
                events.push(ParticleEvent { object, action: system.settings.stop_action });
            }
        }
        self.particles = systems;
    }
}

#[cfg(test)]
mod tests;
