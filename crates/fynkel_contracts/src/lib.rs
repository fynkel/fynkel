//! Contracts shared by every Fynkel crate, the C ABI and (through codegen)
//! the Dart SDK. Nothing here depends on `hecs`, wgpu, or any host.
//!
//! Rules (blueprint §3.1, §4.2, ADR-0003/0004/0014): runtime handles carry
//! world identity, slot and generation; `hecs::Entity` never appears here;
//! codes are stable numbers, never reordered, only appended.

#![forbid(unsafe_code)]

pub mod asset;
pub mod codes;
pub mod diagnostic;
pub mod handle;
pub mod ids;
pub mod object;
pub mod schema;
pub mod uuid;

pub use asset::{AssetId, AssetRef};
pub use codes::{DiagnosticCode, Status};
pub use diagnostic::{has_errors, Code, Diagnostic, Severity};
pub use handle::{
    allocate_world_id, attach_identity_authority, identity_authority, AuthorityMismatch, Entity,
    IdentityAuthority, IdentityExhausted, WorldId,
};
pub use ids::{ComponentId, EventKind};
pub use object::ObjectRef;
pub use schema::{
    EnumSchema, FieldSchema, FieldType, HasEnumSchema, HasSchema, RecordSchema, SchemaField,
    SchemaKind,
};
pub use uuid::Uuid;

/// Bumped when the C ABI (function signatures, buffer headers) changes.
pub const ABI_VERSION: u16 = 3;

/// Fixed simulation step default (blueprint §5.1): 60 Hz.
pub const DEFAULT_FIXED_DT: f64 = 1.0 / 60.0;
/// Frame dt clamp before it enters the accumulator (§5.1: "e.g. 250 ms").
pub const MAX_FRAME_DT: f64 = 0.25;
/// Catch-up ticks per host frame before time is dropped with `MissedDeadline`.
pub const DEFAULT_MAX_STEPS_PER_FRAME: u32 = 4;

/// Tick counter. Starts at 0 for a fresh world; `step()` publishes tick n+1.
pub type Tick = u64;
