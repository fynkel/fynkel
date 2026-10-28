//! The game world: objects with stable ids, and the components they carry.
//!
//! See `docs/stage-1.md` §3.1–3.2 and `docs/stage-2.md` §3.1 (the
//! hierarchy). Nothing here knows about rendering, platforms, Flutter or
//! Dart.

mod animation;
mod bvh;
mod color;
mod components;
mod debug;
mod describe;
mod hierarchy;
mod id;
mod model;
mod particles;
pub mod raycast;
mod resources;
mod skin;
mod trails;
mod world;

pub use animation::{AnimationEvent, AnimatorState, MAX_BLENDS, MAX_LAYERS, Motion};
pub use color::Color;
pub use components::{Camera, Component, Light, LightKind, MeshRenderer, Transform};
pub use debug::DebugLine;
pub use describe::{Describe, Field, FieldError, FieldKind, Range, Value};
pub use fynkel_load::{ClipData, HdrData, ImageData, ImageError, MaterialData, ModelData, ModelError};
pub use fynkel_mesh::{MeshChange, MeshData, MeshError, MeshParts, PathPoint, Sweep, Vertex};
pub use glam::{Mat4, Quat, Vec2, Vec3};
pub use hierarchy::WorldMatrices;
pub use id::{MaterialId, MeshId, ObjectId, ShaderId, TextureId};
pub use model::ModelIds;
pub use particles::{
    Burst, BySpeed, Collision as ParticleCollision, Curve as ParticleCurve, DrawnParticle, GRAVITY, INHERIT_COLOR,
    INHERIT_ROTATION, INHERIT_SIZE, InheritMode, InheritVelocity, LimitVelocity, MAX_BURSTS, MAX_PARTICLES, MAX_PLANES,
    MAX_SUB_EMITTERS, Noise as ParticleNoise, ParticleCommand, ParticleEvent, ParticleEventKind, ParticleSettings,
    ParticleSystem, Range as ParticleRange, RenderMode, Shape, ShapeKind, Space, StopAction, SubEmitter,
    SubEmitterKind, TABLE, TextureSheet, VectorOverLifetime,
};
pub use raycast::{Ray, RayHit, Raycaster};
pub use resources::{Blend, MAX_SHADER_TEXTURES, Material, ScreenEffect, Shader};
pub use skin::Skin;
pub use trails::{MAX_TRAIL_POINTS, Trail, TrailSettings};
pub use world::{Result, World, WorldError};
