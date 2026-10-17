//! The game world: objects with stable ids, and the components they carry.
//!
//! See `docs/stage-1.md` §3.1–3.2 and `docs/stage-2.md` §3.1 (the
//! hierarchy). Nothing here knows about rendering, platforms, Flutter or
//! Dart.

mod animation;
mod color;
mod components;
mod debug;
mod describe;
mod hierarchy;
mod id;
mod model;
pub mod raycast;
mod resources;
mod skin;
mod world;

pub use animation::{AnimationEvent, AnimatorState, MAX_BLENDS, MAX_LAYERS, Motion};
pub use color::Color;
pub use components::{Camera, Component, Light, LightKind, MeshRenderer, Transform};
pub use debug::DebugLine;
pub use describe::{Describe, Field, FieldError, FieldKind, Range, Value};
pub use fynkel_load::{ClipData, HdrData, ImageData, ImageError, MaterialData, ModelData, ModelError};
pub use fynkel_mesh::{MeshData, MeshError, MeshParts, Vertex};
pub use glam::{Mat4, Quat, Vec2, Vec3};
pub use hierarchy::WorldMatrices;
pub use id::{MaterialId, MeshId, ObjectId, TextureId};
pub use model::ModelIds;
pub use raycast::{Ray, RayHit, Raycaster};
pub use resources::{Blend, Material};
pub use skin::Skin;
pub use world::{Result, World, WorldError};
