//! Particle systems: their settings, commands, state and events.

use std::sync::PoisonError;

use fynkel_world::{
    Burst, BySpeed, Color, InheritMode, InheritVelocity, LimitVelocity, MaterialId, ParticleCollision, ParticleCommand,
    ParticleCurve, ParticleEventKind, ParticleNoise, ParticleRange, ParticleSettings, RenderMode, Shape, ShapeKind,
    Space, StopAction, SubEmitter, SubEmitterKind, TextureSheet, VectorOverLifetime,
};

use crate::error::{Error, FyStatus, guard};

use super::*;

// --- Particles ---------------------------------------------------------------

/// A number for each particle: `min` when they're the same, else one at
/// random from `min` to `max`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyRange {
    pub min: f32,
    pub max: f32,
}

impl From<FyRange> for ParticleRange {
    fn from(r: FyRange) -> Self {
        Self { min: r.min, max: r.max }
    }
}

/// `FyParticleSettings::space`: particles move with the object.
pub const FY_PARTICLE_SPACE_LOCAL: u32 = 0;
/// Particles stay where they were made as the object moves.
pub const FY_PARTICLE_SPACE_WORLD: u32 = 1;

/// `FyParticleSettings::stop_action`, and `FyParticleEvent::action`.
pub const FY_PARTICLE_STOP_NONE: u32 = 0;
pub const FY_PARTICLE_STOP_DISABLE: u32 = 1;
pub const FY_PARTICLE_STOP_DESTROY: u32 = 2;
pub const FY_PARTICLE_STOP_CALLBACK: u32 = 3;

/// `FyParticleShape::kind`.
pub const FY_PARTICLE_SHAPE_POINT: u32 = 0;
pub const FY_PARTICLE_SHAPE_SPHERE: u32 = 1;
pub const FY_PARTICLE_SHAPE_HEMISPHERE: u32 = 2;
pub const FY_PARTICLE_SHAPE_CONE: u32 = 3;
pub const FY_PARTICLE_SHAPE_BOX: u32 = 4;
pub const FY_PARTICLE_SHAPE_CIRCLE: u32 = 5;
pub const FY_PARTICLE_SHAPE_EDGE: u32 = 6;

/// The steps of `FyParticleSettings`' curves over a particle's life.
pub const FY_PARTICLE_TABLE: u32 = 64;

/// The most bursts in `FyParticleSettings`.
pub const FY_PARTICLE_MAX_BURSTS: u32 = 8;

/// `FyCurve::mode`: between `min` and `max`.
pub const FY_CURVE_CONSTANT: u32 = 0;
/// Between `tables[0]` and `tables[1]` over a particle's life.
pub const FY_CURVE_CURVES: u32 = 1;

/// A number over a particle's life (Unity's `MinMaxCurve`): each
/// particle's is somewhere between two constants or two curves, by a
/// random number of its own.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FyCurve {
    /// `FY_CURVE_*`.
    pub mode: u32,
    pub min: f32,
    pub max: f32,
    pub tables: [[f32; FY_PARTICLE_TABLE as usize]; 2],
}

impl FyCurve {
    /// `value` for every particle, all its life.
    pub const fn constant(value: f32) -> Self {
        Self { mode: FY_CURVE_CONSTANT, min: value, max: value, tables: [[value; FY_PARTICLE_TABLE as usize]; 2] }
    }

    fn curve(&self, what: &str) -> Result<ParticleCurve, Error> {
        match self.mode {
            FY_CURVE_CONSTANT => Ok(ParticleCurve::Constant(ParticleRange { min: self.min, max: self.max })),
            FY_CURVE_CURVES => Ok(ParticleCurve::Curves(Box::new(self.tables))),
            _ => Err(Error::new(FyStatus::InvalidValue, format!("{what}'s mode isn't one of the FY_CURVE_* values"))),
        }
    }
}

/// A vector over a particle's life, an axis each, in `space`
/// (`FY_PARTICLE_SPACE_*`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FyParticleVector {
    pub x: FyCurve,
    pub y: FyCurve,
    pub z: FyCurve,
    pub space: u32,
}

impl FyParticleVector {
    fn vector(&self, what: &str) -> Result<VectorOverLifetime, Error> {
        Ok(VectorOverLifetime {
            x: self.x.curve(what)?,
            y: self.y.curve(what)?,
            z: self.z.curve(what)?,
            space: particle_space(self.space).ok_or_else(|| {
                Error::new(FyStatus::InvalidValue, format!("{what}.space isn't one of the FY_PARTICLE_SPACE_* values"))
            })?,
        })
    }
}

/// A speed past `limit` is brought down by `dampen`, 0 to 1; `drag` slows
/// every particle by that much of its speed a second.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FyParticleLimit {
    pub limit: FyCurve,
    pub dampen: f32,
    pub drag: FyCurve,
}

/// A wander through a smooth random field: `strength` in metres a
/// second, `frequency` swirls a metre, the field changing `scroll_speed`
/// a second, 1 to 4 `octaves`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FyParticleNoise {
    pub strength: FyCurve,
    pub frequency: f32,
    pub scroll_speed: f32,
    pub octaves: u32,
}

/// The material's texture as `columns` × `rows` frames, `frame_over_time`
/// (0 the first, 1 past the last) `cycles` times a life, starting
/// `start_frame` on; with `random_row`, a row each.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FyParticleSheet {
    pub columns: u32,
    pub rows: u32,
    pub frame_over_time: FyCurve,
    pub start_frame: FyRange,
    pub cycles: f32,
    pub random_row: bool,
}

/// `FyParticleSettings::render_mode`: a square facing the camera.
pub const FY_PARTICLE_RENDER_BILLBOARD: u32 = 0;
/// Long the way it moves, `length_scale` and `speed_scale`.
pub const FY_PARTICLE_RENDER_STRETCHED: u32 = 1;
/// Lying flat, facing up.
pub const FY_PARTICLE_RENDER_HORIZONTAL: u32 = 2;
/// Standing up, turned to the camera.
pub const FY_PARTICLE_RENDER_VERTICAL: u32 = 3;
/// `mesh`, turned by its rotation round up.
pub const FY_PARTICLE_RENDER_MESH: u32 = 4;

/// Where particles are made and which way they go. Shapes face up (+Y).
/// Angles are radians.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyParticleShape {
    /// `FY_PARTICLE_SHAPE_*`.
    pub kind: u32,
    pub radius: f32,
    /// 0 on the rim alone to 1 all through.
    pub radius_thickness: f32,
    /// A cone's, from up to its side: 0 to under π/2.
    pub angle: f32,
    /// Up to 2π.
    pub arc: f32,
    /// A box's.
    pub size: FyVec3,
    /// 0 to 1.
    pub random_direction: f32,
}

/// `FyParticleInherit::mode`: the velocity a particle starts with.
pub const FY_PARTICLE_INHERIT_INITIAL: u32 = 0;
/// The velocity its object has each frame, carrying it along.
pub const FY_PARTICLE_INHERIT_CURRENT: u32 = 1;

/// A particle takes `curve` of its object's velocity (Unity's *Inherit
/// Velocity*): `FY_PARTICLE_INHERIT_*`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FyParticleInherit {
    pub mode: u32,
    pub curve: FyCurve,
}

/// The most planes in `FyParticleCollision`.
pub const FY_PARTICLE_MAX_PLANES: u32 = 6;

/// Bouncing off planes: each the object's, through its position, facing
/// its up. As `fynkel_world::ParticleCollision`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FyParticleCollision {
    /// How many of `planes` there are.
    pub plane_count: u32,
    pub planes: [u64; FY_PARTICLE_MAX_PLANES as usize],
    pub dampen: FyCurve,
    pub bounce: FyCurve,
    pub lifetime_loss: FyCurve,
    pub min_kill_speed: f32,
    pub radius_scale: f32,
    /// Its hits are events (`FY_PARTICLE_EVENT_COLLISION`).
    pub send_messages: bool,
}

/// The most sub emitters in `FyParticleSettings`.
pub const FY_PARTICLE_MAX_SUB_EMITTERS: u32 = 4;

/// `FyParticleSubEmitter::kind`: as each particle is born, all its life.
pub const FY_PARTICLE_SUB_BIRTH: u32 = 0;
/// As one hits a plane.
pub const FY_PARTICLE_SUB_COLLISION: u32 = 1;
/// As one dies.
pub const FY_PARTICLE_SUB_DEATH: u32 = 2;

/// `FyParticleSubEmitter::inherit`, or'd: the particle's colour, size and
/// rotation, times the sub system's own.
pub const FY_PARTICLE_INHERIT_COLOR: u32 = 1;
pub const FY_PARTICLE_INHERIT_SIZE: u32 = 2;
pub const FY_PARTICLE_INHERIT_ROTATION: u32 = 4;

/// Another object's system that emits where this one's particles are
/// born, hit or die, with `probability`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyParticleSubEmitter {
    pub system: u64,
    /// `FY_PARTICLE_SUB_*`.
    pub kind: u32,
    /// `FY_PARTICLE_INHERIT_COLOR` and the others.
    pub inherit: u32,
    pub probability: f32,
}

/// `count` particles at once, `time` seconds into the timeline, `cycles`
/// times (0 for as long as it plays) every `interval` seconds, each time
/// with `probability`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyParticleBurst {
    pub time: f32,
    pub count: FyRange,
    pub cycles: u32,
    pub interval: f32,
    pub probability: f32,
}

/// A particle system's settings: Unity's main, emission and shape
/// modules, its over-lifetime and by-speed modules, noise, a texture
/// sheet, and how it's drawn. Sent when they change, not each frame.
/// Seconds, metres and radians; colours sRGB.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FyParticleSettings {
    pub duration: f32,
    pub looping: bool,
    pub prewarm: bool,
    pub play_on_awake: bool,
    pub unscaled_time: bool,
    pub start_delay: f32,
    pub start_lifetime: FyRange,
    pub start_speed: FyRange,
    pub start_size: FyRange,
    pub start_rotation: FyRange,
    /// Each particle's is between the two at random.
    pub start_color: [FyColor; 2],
    pub gravity_modifier: f32,
    /// `FY_PARTICLE_SPACE_*`.
    pub space: u32,
    pub simulation_speed: f32,
    pub max_particles: u32,
    /// `FY_PARTICLE_STOP_*`.
    pub stop_action: u32,
    /// The same particles each play when `seeded`.
    pub seeded: bool,
    pub seed: u32,
    pub rate_over_time: f32,
    /// How many of `bursts` there are.
    pub burst_count: u32,
    pub bursts: [FyParticleBurst; FY_PARTICLE_MAX_BURSTS as usize],
    pub shape: FyParticleShape,
    /// A particle's size times `size_table` over its life, when set.
    pub has_size_table: bool,
    pub size_table: [f32; FY_PARTICLE_TABLE as usize],
    /// A particle's colour times `color_table` over its life, when set.
    pub has_color_table: bool,
    pub color_table: [FyColor; FY_PARTICLE_TABLE as usize],
    pub has_velocity_over_lifetime: bool,
    pub velocity_over_lifetime: FyParticleVector,
    pub has_limit_velocity: bool,
    pub limit_velocity: FyParticleLimit,
    pub has_force_over_lifetime: bool,
    pub force_over_lifetime: FyParticleVector,
    /// Radians a second.
    pub has_rotation_over_lifetime: bool,
    pub rotation_over_lifetime: FyCurve,
    /// A particle's size times `size_by_speed`, from `size_speeds.min` m/s
    /// (its first step) to `size_speeds.max` (its last).
    pub has_size_by_speed: bool,
    pub size_by_speed: [f32; FY_PARTICLE_TABLE as usize],
    pub size_speeds: FyRange,
    pub has_color_by_speed: bool,
    pub color_by_speed: [FyColor; FY_PARTICLE_TABLE as usize],
    pub color_speeds: FyRange,
    pub has_noise: bool,
    pub noise: FyParticleNoise,
    pub has_texture_sheet: bool,
    pub texture_sheet: FyParticleSheet,
    /// What it's drawn with, or 0 for nothing drawn.
    pub material: u64,
    /// `FY_PARTICLE_RENDER_*`.
    pub render_mode: u32,
    /// A stretched particle's length: `length_scale` times its size, plus
    /// `speed_scale` times its speed.
    pub length_scale: f32,
    pub speed_scale: f32,
    /// `FY_PARTICLE_RENDER_MESH`'s mesh.
    pub mesh: u64,
    /// Lit as a surface facing the camera.
    pub lit: bool,
    /// Metres it fades out in front of what's behind it; 0 for none.
    pub softness: f32,
    /// Particles a metre its object moves.
    pub rate_over_distance: f32,
    pub has_inherit_velocity: bool,
    pub inherit_velocity: FyParticleInherit,
    pub has_collision: bool,
    pub collision: FyParticleCollision,
    /// How many of `sub_emitters` there are.
    pub sub_emitter_count: u32,
    pub sub_emitters: [FyParticleSubEmitter; FY_PARTICLE_MAX_SUB_EMITTERS as usize],
}

impl Default for FyParticleSettings {
    /// Unity's: five seconds looping, ten a second living five seconds
    /// each, at 5 m/s, a size of 1, white, from a cone of 25°.
    fn default() -> Self {
        let d = ParticleSettings::default();
        let range = |r: ParticleRange| FyRange { min: r.min, max: r.max };
        Self {
            duration: d.duration,
            looping: d.looping,
            prewarm: d.prewarm,
            play_on_awake: d.play_on_awake,
            unscaled_time: d.unscaled_time,
            start_delay: d.start_delay,
            start_lifetime: range(d.start_lifetime),
            start_speed: range(d.start_speed),
            start_size: range(d.start_size),
            start_rotation: range(d.start_rotation),
            start_color: d.start_color.map(FyColor::from),
            gravity_modifier: d.gravity_modifier,
            space: FY_PARTICLE_SPACE_LOCAL,
            simulation_speed: d.simulation_speed,
            max_particles: d.max_particles,
            stop_action: FY_PARTICLE_STOP_NONE,
            seeded: false,
            seed: 0,
            rate_over_time: d.rate_over_time,
            burst_count: 0,
            bursts: [FyParticleBurst::default(); FY_PARTICLE_MAX_BURSTS as usize],
            shape: FyParticleShape {
                kind: FY_PARTICLE_SHAPE_CONE,
                radius: d.shape.radius,
                radius_thickness: d.shape.radius_thickness,
                angle: d.shape.angle,
                arc: d.shape.arc,
                size: d.shape.size.into(),
                random_direction: d.shape.random_direction,
            },
            has_size_table: false,
            size_table: [1.0; FY_PARTICLE_TABLE as usize],
            has_color_table: false,
            color_table: [FyColor::from(Color::WHITE); FY_PARTICLE_TABLE as usize],
            has_velocity_over_lifetime: false,
            velocity_over_lifetime: FyParticleVector::ZERO,
            has_limit_velocity: false,
            limit_velocity: FyParticleLimit {
                limit: FyCurve::constant(1.0),
                dampen: 0.0,
                drag: FyCurve::constant(0.0),
            },
            has_force_over_lifetime: false,
            force_over_lifetime: FyParticleVector::ZERO,
            has_rotation_over_lifetime: false,
            rotation_over_lifetime: FyCurve::constant(0.0),
            has_size_by_speed: false,
            size_by_speed: [1.0; FY_PARTICLE_TABLE as usize],
            size_speeds: FyRange { min: 0.0, max: 1.0 },
            has_color_by_speed: false,
            color_by_speed: [FyColor::from(Color::WHITE); FY_PARTICLE_TABLE as usize],
            color_speeds: FyRange { min: 0.0, max: 1.0 },
            has_noise: false,
            noise: FyParticleNoise { strength: FyCurve::constant(1.0), frequency: 0.5, scroll_speed: 0.0, octaves: 1 },
            has_texture_sheet: false,
            texture_sheet: FyParticleSheet {
                columns: 1,
                rows: 1,
                frame_over_time: FyCurve::constant(0.0),
                start_frame: FyRange::default(),
                cycles: 1.0,
                random_row: false,
            },
            material: 0,
            render_mode: FY_PARTICLE_RENDER_BILLBOARD,
            length_scale: 2.0,
            speed_scale: 0.0,
            mesh: 0,
            lit: false,
            softness: 0.0,
            rate_over_distance: 0.0,
            has_inherit_velocity: false,
            inherit_velocity: FyParticleInherit { mode: FY_PARTICLE_INHERIT_INITIAL, curve: FyCurve::constant(0.0) },
            has_collision: false,
            collision: FyParticleCollision {
                plane_count: 0,
                planes: [0; FY_PARTICLE_MAX_PLANES as usize],
                dampen: FyCurve::constant(0.0),
                bounce: FyCurve::constant(1.0),
                lifetime_loss: FyCurve::constant(0.0),
                min_kill_speed: 0.0,
                radius_scale: 1.0,
                send_messages: false,
            },
            sub_emitter_count: 0,
            sub_emitters: [FyParticleSubEmitter::default(); FY_PARTICLE_MAX_SUB_EMITTERS as usize],
        }
    }
}

impl FyParticleSettings {
    fn settings(&self) -> Result<ParticleSettings, Error> {
        let bad =
            |what: &str| Error::new(FyStatus::InvalidValue, format!("{what} isn't one of the FY_PARTICLE_* values"));
        let space = particle_space(self.space).ok_or_else(|| bad("space"))?;
        let render_mode = match self.render_mode {
            FY_PARTICLE_RENDER_BILLBOARD => RenderMode::Billboard,
            FY_PARTICLE_RENDER_STRETCHED => {
                RenderMode::Stretched { length_scale: self.length_scale, speed_scale: self.speed_scale }
            }
            FY_PARTICLE_RENDER_HORIZONTAL => RenderMode::Horizontal,
            FY_PARTICLE_RENDER_VERTICAL => RenderMode::Vertical,
            FY_PARTICLE_RENDER_MESH => RenderMode::Mesh(mesh(self.mesh)?),
            _ => return Err(bad("render_mode")),
        };
        let by_speed = |speeds: FyRange| (speeds.min, speeds.max);
        let stop_action = stop_action(self.stop_action).ok_or_else(|| bad("stop_action"))?;
        let kind = match self.shape.kind {
            FY_PARTICLE_SHAPE_POINT => ShapeKind::Point,
            FY_PARTICLE_SHAPE_SPHERE => ShapeKind::Sphere,
            FY_PARTICLE_SHAPE_HEMISPHERE => ShapeKind::Hemisphere,
            FY_PARTICLE_SHAPE_CONE => ShapeKind::Cone,
            FY_PARTICLE_SHAPE_BOX => ShapeKind::Box,
            FY_PARTICLE_SHAPE_CIRCLE => ShapeKind::Circle,
            FY_PARTICLE_SHAPE_EDGE => ShapeKind::Edge,
            _ => return Err(bad("shape.kind")),
        };
        if self.burst_count > FY_PARTICLE_MAX_BURSTS {
            return Err(Error::new(FyStatus::InvalidValue, "a particle system has at most 8 bursts"));
        }
        if self.sub_emitter_count > FY_PARTICLE_MAX_SUB_EMITTERS {
            return Err(Error::new(FyStatus::InvalidValue, "a particle system has at most 4 sub emitters"));
        }
        let c = &self.collision;
        if self.has_collision && c.plane_count > FY_PARTICLE_MAX_PLANES {
            return Err(Error::new(FyStatus::InvalidValue, "a particle system collides with at most 6 planes"));
        }
        let s = &self.shape;
        Ok(ParticleSettings {
            duration: self.duration,
            looping: self.looping,
            prewarm: self.prewarm,
            start_delay: self.start_delay,
            start_lifetime: self.start_lifetime.into(),
            start_speed: self.start_speed.into(),
            start_size: self.start_size.into(),
            start_rotation: self.start_rotation.into(),
            start_color: self.start_color.map(Color::from),
            gravity_modifier: self.gravity_modifier,
            space,
            simulation_speed: self.simulation_speed,
            unscaled_time: self.unscaled_time,
            play_on_awake: self.play_on_awake,
            max_particles: self.max_particles,
            stop_action,
            seed: self.seeded.then_some(self.seed),
            rate_over_time: self.rate_over_time,
            bursts: self.bursts[..self.burst_count as usize]
                .iter()
                .map(|b| Burst {
                    time: b.time,
                    count: b.count.into(),
                    cycles: b.cycles,
                    interval: b.interval,
                    probability: b.probability,
                })
                .collect(),
            shape: Shape {
                kind,
                radius: s.radius,
                radius_thickness: s.radius_thickness,
                angle: s.angle,
                arc: s.arc,
                size: s.size.into(),
                random_direction: s.random_direction,
            },
            size_over_lifetime: self.has_size_table.then(|| Box::new(self.size_table)),
            color_over_lifetime: self.has_color_table.then(|| Box::new(self.color_table.map(Color::from))),
            velocity_over_lifetime: self
                .has_velocity_over_lifetime
                .then(|| self.velocity_over_lifetime.vector("velocity_over_lifetime"))
                .transpose()?,
            limit_velocity: self
                .has_limit_velocity
                .then(|| -> Result<_, Error> {
                    let l = &self.limit_velocity;
                    Ok(LimitVelocity { limit: l.limit.curve("limit")?, dampen: l.dampen, drag: l.drag.curve("drag")? })
                })
                .transpose()?,
            force_over_lifetime: self
                .has_force_over_lifetime
                .then(|| self.force_over_lifetime.vector("force_over_lifetime"))
                .transpose()?,
            rotation_over_lifetime: self
                .has_rotation_over_lifetime
                .then(|| self.rotation_over_lifetime.curve("rotation_over_lifetime"))
                .transpose()?,
            size_by_speed: self.has_size_by_speed.then(|| {
                let (min_speed, max_speed) = by_speed(self.size_speeds);
                BySpeed { table: Box::new(self.size_by_speed), min_speed, max_speed }
            }),
            color_by_speed: self.has_color_by_speed.then(|| {
                let (min_speed, max_speed) = by_speed(self.color_speeds);
                BySpeed { table: Box::new(self.color_by_speed.map(Color::from)), min_speed, max_speed }
            }),
            noise: self
                .has_noise
                .then(|| -> Result<_, Error> {
                    let n = &self.noise;
                    Ok(ParticleNoise {
                        strength: n.strength.curve("noise.strength")?,
                        frequency: n.frequency,
                        scroll_speed: n.scroll_speed,
                        octaves: n.octaves,
                    })
                })
                .transpose()?,
            texture_sheet: self
                .has_texture_sheet
                .then(|| -> Result<_, Error> {
                    let t = &self.texture_sheet;
                    Ok(TextureSheet {
                        columns: t.columns,
                        rows: t.rows,
                        frame_over_time: t.frame_over_time.curve("texture_sheet.frame_over_time")?,
                        start_frame: t.start_frame.into(),
                        cycles: t.cycles,
                        random_row: t.random_row,
                    })
                })
                .transpose()?,
            material: MaterialId::from_raw(self.material),
            render_mode,
            lit: self.lit,
            softness: self.softness,
            rate_over_distance: self.rate_over_distance,
            inherit_velocity: self
                .has_inherit_velocity
                .then(|| -> Result<_, Error> {
                    let i = &self.inherit_velocity;
                    let mode = match i.mode {
                        FY_PARTICLE_INHERIT_INITIAL => InheritMode::Initial,
                        FY_PARTICLE_INHERIT_CURRENT => InheritMode::Current,
                        _ => return Err(bad("inherit_velocity.mode")),
                    };
                    Ok(InheritVelocity { mode, curve: i.curve.curve("inherit_velocity.curve")? })
                })
                .transpose()?,
            collision: self
                .has_collision
                .then(|| -> Result<_, Error> {
                    Ok(ParticleCollision {
                        planes: c.planes[..c.plane_count as usize]
                            .iter()
                            .map(|&p| object(p))
                            .collect::<Result<_, _>>()?,
                        dampen: c.dampen.curve("collision.dampen")?,
                        bounce: c.bounce.curve("collision.bounce")?,
                        lifetime_loss: c.lifetime_loss.curve("collision.lifetime_loss")?,
                        min_kill_speed: c.min_kill_speed,
                        radius_scale: c.radius_scale,
                        send_messages: c.send_messages,
                    })
                })
                .transpose()?,
            sub_emitters: self.sub_emitters[..self.sub_emitter_count as usize]
                .iter()
                .map(|e| {
                    let kind = match e.kind {
                        FY_PARTICLE_SUB_BIRTH => SubEmitterKind::Birth,
                        FY_PARTICLE_SUB_COLLISION => SubEmitterKind::Collision,
                        FY_PARTICLE_SUB_DEATH => SubEmitterKind::Death,
                        _ => return Err(bad("sub_emitters.kind")),
                    };
                    Ok(SubEmitter { system: object(e.system)?, kind, inherit: e.inherit, probability: e.probability })
                })
                .collect::<Result<_, _>>()?,
        })
    }
}

impl FyParticleVector {
    const ZERO: Self = Self {
        x: FyCurve::constant(0.0),
        y: FyCurve::constant(0.0),
        z: FyCurve::constant(0.0),
        space: FY_PARTICLE_SPACE_LOCAL,
    };
}

fn particle_space(value: u32) -> Option<Space> {
    match value {
        FY_PARTICLE_SPACE_LOCAL => Some(Space::Local),
        FY_PARTICLE_SPACE_WORLD => Some(Space::World),
        _ => None,
    }
}

fn stop_action(value: u32) -> Option<StopAction> {
    Some(match value {
        FY_PARTICLE_STOP_NONE => StopAction::None,
        FY_PARTICLE_STOP_DISABLE => StopAction::Disable,
        FY_PARTICLE_STOP_DESTROY => StopAction::Destroy,
        FY_PARTICLE_STOP_CALLBACK => StopAction::Callback,
        _ => return None,
    })
}

fn stop_action_value(action: StopAction) -> u32 {
    match action {
        StopAction::None => FY_PARTICLE_STOP_NONE,
        StopAction::Disable => FY_PARTICLE_STOP_DISABLE,
        StopAction::Destroy => FY_PARTICLE_STOP_DESTROY,
        StopAction::Callback => FY_PARTICLE_STOP_CALLBACK,
    }
}

/// What a particle system is doing, from `fy_particles_get`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyParticleState {
    /// How many particles it has.
    pub count: u32,
    /// From play until it's stopped and its last particle is gone.
    pub playing: bool,
    pub paused: bool,
    pub emitting: bool,
}

/// `FyParticleEvent::kind`: a system was done (stopped, its last particle
/// gone), with a stop action for the game to do.
pub const FY_PARTICLE_EVENT_DONE: u32 = 0;
/// A system's particles hit a plane, with `send_messages` on (Unity's
/// `OnParticleCollision`), `count` of them this step.
pub const FY_PARTICLE_EVENT_COLLISION: u32 = 1;

/// What happened in the last `fy_world_particles_step`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyParticleEvent {
    pub object: u64,
    /// `FY_PARTICLE_EVENT_*`.
    pub kind: u32,
    /// `FY_PARTICLE_EVENT_DONE`'s: `FY_PARTICLE_STOP_*`, not
    /// `FY_PARTICLE_STOP_NONE`.
    pub action: u32,
    /// `FY_PARTICLE_EVENT_COLLISION`'s: the plane's object, and how many
    /// hit it.
    pub plane: u64,
    pub count: u32,
}

/// `fy_particles_command`: plays from the start (or on from a pause).
pub const FY_PARTICLES_PLAY: u32 = 0;
/// Emits no more; its particles live out their lives.
pub const FY_PARTICLES_STOP: u32 = 1;
/// Emits no more, and its particles go at once.
pub const FY_PARTICLES_STOP_AND_CLEAR: u32 = 2;
pub const FY_PARTICLES_PAUSE: u32 = 3;
/// Its particles go; it plays on.
pub const FY_PARTICLES_CLEAR: u32 = 4;
/// Makes `count` particles now, playing or not.
pub const FY_PARTICLES_EMIT: u32 = 5;

/// Gives the object a particle system with `settings`, or changes the one
/// it has, keeping its particles and where it is. A new one plays when
/// it's first stepped, if it plays on awake. Settings that can't be are
/// `FY_STATUS_INVALID_VALUE` and change nothing.
///
/// # Safety
///
/// `world` is null or live; `settings` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_particles_set(
    world: *const FyWorld,
    id: u64,
    settings: *const FyParticleSettings,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, settings) =
            unsafe { (handle(world, "world")?, settings.as_ref().ok_or_else(|| Error::null("settings"))?) };
        let settings = settings.settings()?;
        world.lock()?.set_particles(object(id)?, settings)?;
        Ok(())
    })
}

/// Takes the object's particle system away, its particles too.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_particles_remove(world: *const FyWorld, id: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        world.lock()?.remove_particles(object(id)?)?;
        Ok(())
    })
}

/// Plays, stops, pauses, clears or emits from the object's system:
/// `FY_PARTICLES_*`. `count` is `FY_PARTICLES_EMIT`'s, ignored otherwise.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_particles_command(world: *const FyWorld, id: u64, command: u32, count: u32) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let command = match command {
            FY_PARTICLES_PLAY => ParticleCommand::Play,
            FY_PARTICLES_STOP => ParticleCommand::Stop { clear: false },
            FY_PARTICLES_STOP_AND_CLEAR => ParticleCommand::Stop { clear: true },
            FY_PARTICLES_PAUSE => ParticleCommand::Pause,
            FY_PARTICLES_CLEAR => ParticleCommand::Clear,
            FY_PARTICLES_EMIT => ParticleCommand::Emit(count),
            _ => return Err(Error::new(FyStatus::InvalidValue, "command isn't one of FY_PARTICLES_*")),
        };
        world.lock()?.particle_command(object(id)?, command)?;
        Ok(())
    })
}

/// Moves the object's system `seconds` on at once (Unity's `Simulate`).
/// `seconds` is read through a pointer, as `fy_world_animate`'s `dt`.
///
/// # Safety
///
/// `world` is null or live; `seconds` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_particles_simulate(world: *const FyWorld, id: u64, seconds: *const f32) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, seconds) = unsafe { (handle(world, "world")?, read(seconds, "seconds")?) };
        world.lock()?.simulate_particles(object(id)?, seconds)?;
        Ok(())
    })
}

/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_particles_get(world: *const FyWorld, id: u64, out: *mut FyParticleState) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let inner = world.lock()?;
        let system = inner.particles(object(id)?)?;
        let state = FyParticleState {
            count: system.count() as u32,
            playing: system.is_playing(),
            paused: system.is_paused(),
            emitting: system.is_emitting(),
        };
        // SAFETY: per this function's contract.
        unsafe { write(out, state, "out") }
    })
}

/// Moves every particle system on, once a frame, after the animators and
/// before the game's late update (where Unity steps them): `dt` seconds
/// of the game's time, `unscaled` for those that ask; and the trails, by
/// `dt`. Writes how many events there were (systems done, with a stop
/// action, and planes hit) to `out_count`; `fy_particle_events_get` reads
/// them. Both times are read through
/// pointers, as `fy_world_animate`'s `dt`.
///
/// # Safety
///
/// `world` is null or live; `dt` and `unscaled` are null or readable;
/// `out_count` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_world_particles_step(
    world: *const FyWorld,
    dt: *const f32,
    unscaled: *const f32,
    out_count: *mut usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, dt, unscaled) = unsafe { (handle(world, "world")?, read(dt, "dt")?, read(unscaled, "unscaled")?) };
        let mut inner = world.lock()?;
        let mut events = world.particle_events.lock().unwrap_or_else(PoisonError::into_inner);
        events.clear();
        if inner.has_particles() || inner.has_trails() {
            inner.step_particles(dt, unscaled, &mut events);
        }
        // SAFETY: per this function's contract.
        unsafe { write(out_count, events.len(), "out_count") }
    })
}

/// The last `fy_world_particles_step`'s events, as `fy_children_get`
/// writes ids.
///
/// # Safety
///
/// `world` is null or live; `buf` is null or valid for `cap` writes;
/// `out_count` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_particle_events_get(
    world: *const FyWorld,
    buf: *mut FyParticleEvent,
    cap: usize,
    out_count: *mut usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let _inner = world.lock()?;
        let events = world.particle_events.lock().unwrap_or_else(PoisonError::into_inner);
        let items = events.iter().map(|e| match e.kind {
            ParticleEventKind::Done(action) => FyParticleEvent {
                object: e.object.raw(),
                kind: FY_PARTICLE_EVENT_DONE,
                action: stop_action_value(action),
                ..FyParticleEvent::default()
            },
            ParticleEventKind::Collision { plane, count } => FyParticleEvent {
                object: e.object.raw(),
                kind: FY_PARTICLE_EVENT_COLLISION,
                plane: plane.raw(),
                count,
                ..FyParticleEvent::default()
            },
        });
        // SAFETY: per this function's contract.
        unsafe { write_all(items, buf, cap, out_count) }
    })
}
