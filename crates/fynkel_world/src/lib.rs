//! The game world: objects with stable ids, and the components they carry.
//!
//! See `docs/stage-1.md` §3.1–3.2 and `docs/stage-2.md` §3.1 (the
//! hierarchy). Nothing here knows about rendering, platforms, Flutter or
//! Dart.

mod color;
mod components;
mod describe;
mod hierarchy;
mod id;
mod resources;
mod world;

pub use color::Color;
pub use components::{Camera, Component, Light, LightKind, MeshRenderer, Transform};
pub use describe::{Describe, Field, FieldError, FieldKind, Range, Value};
pub use glam::{Mat4, Quat, Vec3};
pub use hierarchy::WorldMatrices;
pub use id::{MaterialId, MeshId, ObjectId};
pub use resources::{Material, MeshSource};
pub use world::{Result, World, WorldError};
