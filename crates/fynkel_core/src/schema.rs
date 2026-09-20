//! Wire record definitions and the schema registry (ADR-0012).
//!
//! Every message that crosses the bridge is a `#[derive(Schema)]` struct
//! here or a component elsewhere; [`ALL`] is the registry `fynkel codegen`
//! reads. The wire *framing* (buffer headers, per-record kind tags) is the
//! ABI's job (`fynkel_native`); these are the record bodies.
//!
//! Ids: commands use [`CommandKind`] values, events use `EventKind` values,
//! components use `ComponentId` values. Append only; never renumber.

use fynkel_contracts::schema::{fingerprint, RecordSchema};
use fynkel_contracts::{Entity, HasSchema};
use fynkel_physics::{Collider, ColliderShape, RigidBody};
use fynkel_scene::Transform;
use fynkel_schema_derive::Schema;
use glam::{Quat, Vec3};

use crate::components::{
    Camera, DirectionalLight, Environment, MeshRenderer, PointLight, SpotLight,
};
use crate::material::PbrMaterial;
use crate::world::Name;

/// Stable command ids (wire `kind` of a command record).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum CommandKind {
    Spawn = 1,
    Destroy = 2,
    SetTransform = 3,
    SetParent = 4,
    SetName = 5,
    SetComponent = 6,
    RemoveComponent = 7,
    Body = 8,
}

impl CommandKind {
    pub fn from_u32(v: u32) -> Option<Self> {
        Some(match v {
            1 => Self::Spawn,
            2 => Self::Destroy,
            3 => Self::SetTransform,
            4 => Self::SetParent,
            5 => Self::SetName,
            6 => Self::SetComponent,
            7 => Self::RemoveComponent,
            8 => Self::Body,
            _ => return None,
        })
    }
}

/// Spawns an entity; `temp_id` is echoed by `Spawned` with the real handle.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "command", id = 1, name = "fynkel.cmd.Spawn")]
pub struct SpawnCommand {
    /// Non-zero, unique within the batch.
    pub temp_id: u64,
    pub transform: Transform,
    pub parent: Option<Entity>,
    /// Empty for no name.
    pub name: String,
}

/// Destroys the entity and its subtree.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "command", id = 2, name = "fynkel.cmd.Destroy")]
pub struct DestroyCommand {
    pub entity: Entity,
}

#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "command", id = 3, name = "fynkel.cmd.SetTransform")]
pub struct SetTransformCommand {
    pub entity: Entity,
    pub transform: Transform,
}

#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "command", id = 4, name = "fynkel.cmd.SetParent")]
pub struct SetParentCommand {
    pub child: Entity,
    pub parent: Option<Entity>,
    /// Keep the world transform (adjust local) instead of the local one.
    pub keep_world: bool,
}

#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "command", id = 5, name = "fynkel.cmd.SetName")]
pub struct SetNameCommand {
    pub entity: Entity,
    pub name: String,
}

/// Inserts or replaces one native component (M3-07a): the body is this
/// record followed by the component's own wire bytes (`component` is its
/// `ComponentId`; strings in the batch tail), so one command kind carries
/// every component `ComponentValue` knows. `fynkel.Transform` here is a
/// `SetTransform`; `fynkel.Name` is not a value (`SetName`).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "command", id = 6, name = "fynkel.cmd.SetComponent")]
pub struct SetComponentCommand {
    pub entity: Entity,
    /// `ComponentId`.
    pub component: u32,
}

/// Removes one native component; `Transform`, the hierarchy and `Name`
/// cannot be removed (`INVALID_PAYLOAD`).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "command", id = 7, name = "fynkel.cmd.RemoveComponent")]
pub struct RemoveComponentCommand {
    pub entity: Entity,
    /// `ComponentId`.
    pub component: u32,
}

/// What a `fynkel.cmd.Body` does with its `vector`/`rotation`
/// (`fynkel_physics::BodyCommand`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Schema)]
#[schema(name = "fynkel.BodyOp")]
pub enum BodyOp {
    /// `vector` newtons, this tick (dynamic).
    ApplyForce,
    /// `vector` newton-seconds, once (dynamic).
    ApplyImpulse,
    /// `vector` newton-metres, this tick (dynamic).
    ApplyTorque,
    /// `vector` newton-metre-seconds, once (dynamic).
    ApplyTorqueImpulse,
    /// `vector` metres per second (dynamic).
    SetLinearVelocity,
    /// `vector` radians per second (dynamic).
    SetAngularVelocity,
    /// `vector` + `rotation`: the world pose a kinematic body reaches at
    /// the end of this tick.
    SetKinematicTarget,
    /// `vector` + `rotation`: the world pose, now, without interpolation
    /// (any body type).
    Teleport,
}

/// Moves a `fynkel.RigidBody` (M3-08; simulation lane only). Rejected at
/// the tick with `MISSING_COMPONENT` (no body) or `INVALID_PAYLOAD` (wrong
/// body type for the op, non-finite value).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "command", id = 8, name = "fynkel.cmd.Body")]
pub struct BodyCommandRecord {
    pub entity: Entity,
    pub op: BodyOp,
    pub vector: Vec3,
    /// Used by `setKinematicTarget` and `teleport`; identity otherwise.
    pub rotation: Quat,
}

/// A spatial query kind (`fynkel.rec.PhysicsQuery`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Schema)]
#[schema(name = "fynkel.PhysicsQueryKind")]
pub enum PhysicsQueryKind {
    /// A ray from `origin` along `direction`, up to `max_distance`.
    Raycast,
    /// A shape swept from `origin` along `direction`; the first hit only.
    ShapeCast,
    /// Every collider intersecting a shape at `origin`.
    Overlap,
}

/// One spatial query in a `PhysicsQueries` buffer
/// (`fynkel_world_physics_query`); answered by `fynkel.rec.PhysicsHit`
/// records carrying its `id`. Colliders are candidates when their `layer`
/// shares a bit with `mask`; the `exclude` entity's colliders are skipped;
/// sensors are hit only with `hit_sensors`.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.rec.PhysicsQuery")]
pub struct PhysicsQueryRecord {
    /// Echoed by every hit of this query.
    pub id: u32,
    pub kind: PhysicsQueryKind,
    /// `shapeCast`/`overlap`: `box`, `sphere` or `capsule` (never `mesh`)
    /// with `size` as `fynkel.Collider` defines it.
    pub shape: ColliderShape,
    pub size: Vec3,
    pub origin: Vec3,
    /// The shape's orientation.
    pub rotation: Quat,
    /// `raycast`/`shapeCast`; need not be normalised.
    pub direction: Vec3,
    pub max_distance: f32,
    pub mask: u32,
    pub exclude: Option<Entity>,
    /// `raycast`/`overlap`: at most this many hits, nearest first (0 = 1).
    pub max_hits: u32,
    pub hit_sensors: bool,
}

/// One hit of a `fynkel.rec.PhysicsQuery`: `point`/`normal` on the hit
/// collider in world space (zero for `overlap`), `distance` along the
/// ray or sweep (0 for `overlap`).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.rec.PhysicsHit")]
pub struct PhysicsHitRecord {
    pub query: u32,
    pub entity: Entity,
    pub point: Vec3,
    pub normal: Vec3,
    pub distance: f32,
}

/// `FYNKEL_QUERY_BODY`: the motion state of an entity's `fynkel.RigidBody`
/// (`MISSING_COMPONENT` without one).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.rec.BodyState")]
pub struct BodyStateRecord {
    pub entity: Entity,
    pub linear_velocity: Vec3,
    pub angular_velocity: Vec3,
    pub sleeping: bool,
}

#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "event", id = 1, name = "fynkel.ev.Spawned")]
pub struct SpawnedEvent {
    pub temp_id: u64,
    pub entity: Entity,
}

#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "event", id = 2, name = "fynkel.ev.Destroyed")]
pub struct DestroyedEvent {
    pub entity: Entity,
}

#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "event", id = 3, name = "fynkel.ev.ParentChanged")]
pub struct ParentChangedEvent {
    pub child: Entity,
    pub parent: Option<Entity>,
}

/// Time dropped after `max_steps_per_frame` catch-up ticks (ADR-0008).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "event", id = 4, name = "fynkel.ev.MissedDeadline")]
pub struct MissedDeadlineEvent {
    pub dropped_seconds: f32,
}

#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "event", id = 5, name = "fynkel.ev.SceneReady")]
pub struct SceneReadyEvent;

/// Physics-lane events share one body (collision/trigger enter/stay/exit):
/// `point` and `normal` in world space, the normal pointing from `a`
/// towards `b`; `impulse` the pair's total impulse over the step (0 for
/// triggers and exits).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "event", id = 256, name = "fynkel.ev.Contact")]
pub struct ContactEvent {
    pub a: Entity,
    pub b: Entity,
    pub point: Vec3,
    pub normal: Vec3,
    pub impulse: f32,
}

/// Snapshot record for a subscribed transform.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.rec.SnapshotTransform")]
pub struct SnapshotTransformRecord {
    pub entity: Entity,
    pub local: Transform,
    pub world: Transform,
}

/// One rejected command or runtime diagnostic (ADR-0014).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.rec.Diagnostic")]
pub struct DiagnosticRecord {
    /// Index in the submitted batch; `u32::MAX` when not tied to a command.
    pub command_index: u32,
    /// `DiagnosticCode`.
    pub code: u32,
    pub entity: Entity,
    /// `ComponentId`, `0` when not component-specific.
    pub component: u32,
    /// `Lane`: 0 simulation, 1 presentation.
    pub lane: u32,
    pub message: String,
}

/// Command envelope in a command buffer: `kind` selects the body schema
/// (a `CommandKind`), `size` lets readers skip bodies they do not know.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.rec.CommandHeader")]
pub struct CommandHeader {
    pub kind: u32,
    /// Body bytes following this header.
    pub size: u32,
}

/// Component envelope in a `FYNKEL_QUERY_COMPONENTS` response: one per
/// native component the entity has, `component` its `ComponentId`, `size`
/// the body bytes following (the component's wire layout).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.rec.ComponentHeader")]
pub struct ComponentHeader {
    pub component: u32,
    /// Body bytes following this header.
    pub size: u32,
}

/// Event envelope in an event buffer: `kind` selects the body schema (an
/// `EventKind`; every physics kind uses `ContactEvent`).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.rec.EventHeader")]
pub struct EventHeader {
    pub kind: u32,
    /// Body bytes following this header.
    pub size: u32,
    /// Low 32 bits of the tick that produced the event.
    pub tick_lo: u32,
    pub flags: u32,
}

/// One raw control event of a host frame (`fynkel_world_capture_input`,
/// §5.1): `device` a `fynkel_core::input::Device`, `phase` a `Phase`;
/// `x`/`y` the axis value or movement (a button's `x` is 1 down, 0 up).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.rec.InputEvent")]
pub struct InputEventRecord {
    pub device: u32,
    /// The control's name in the binding grammar (`Space`, `left`,
    /// `leftStick`, `overlay/jump`).
    pub control: String,
    pub phase: u32,
    pub x: f32,
    pub y: f32,
}

/// One bound action's state (`fynkel_world_input_view`): `kind` an
/// `ActionKind`; `pressed`/`released` are the edges the next tick
/// consumes, `frame_pressed`/`frame_released` the current frame's.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.rec.InputAction")]
pub struct InputActionRecord {
    pub name: String,
    pub kind: u32,
    pub held: bool,
    pub pressed: bool,
    pub released: bool,
    pub frame_pressed: bool,
    pub frame_released: bool,
    /// `axis1d` value, `axis2d` x, a button 1/0.
    pub x: f32,
    pub y: f32,
}

/// The registry, in fingerprint order. Append only.
pub const ALL: &[&RecordSchema] = &[
    &Transform::SCHEMA,
    &Name::SCHEMA,
    &SpawnCommand::SCHEMA,
    &DestroyCommand::SCHEMA,
    &SetTransformCommand::SCHEMA,
    &SetParentCommand::SCHEMA,
    &SetNameCommand::SCHEMA,
    &SpawnedEvent::SCHEMA,
    &DestroyedEvent::SCHEMA,
    &ParentChangedEvent::SCHEMA,
    &MissedDeadlineEvent::SCHEMA,
    &SceneReadyEvent::SCHEMA,
    &ContactEvent::SCHEMA,
    &SnapshotTransformRecord::SCHEMA,
    &DiagnosticRecord::SCHEMA,
    &CommandHeader::SCHEMA,
    &EventHeader::SCHEMA,
    &MeshRenderer::SCHEMA,
    &Camera::SCHEMA,
    &DirectionalLight::SCHEMA,
    &PointLight::SCHEMA,
    &PbrMaterial::SCHEMA,
    &SpotLight::SCHEMA,
    &Environment::SCHEMA,
    &InputEventRecord::SCHEMA,
    &InputActionRecord::SCHEMA,
    &SetComponentCommand::SCHEMA,
    &RemoveComponentCommand::SCHEMA,
    &ComponentHeader::SCHEMA,
    &RigidBody::SCHEMA,
    &Collider::SCHEMA,
    &BodyCommandRecord::SCHEMA,
    &PhysicsQueryRecord::SCHEMA,
    &PhysicsHitRecord::SCHEMA,
    &BodyStateRecord::SCHEMA,
];

/// Derived, never hand-maintained (ADR-0012). Generated code embeds this
/// value and `fynkel codegen --check` fails when it drifts.
pub fn schema_fingerprint() -> u64 {
    fingerprint(ALL)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fynkel_contracts::schema::{FieldType, SchemaKind};

    #[test]
    fn transform_schema_is_forty_bytes() {
        let s = &Transform::SCHEMA;
        assert_eq!(s.kind, SchemaKind::Component);
        assert_eq!(s.id, 1);
        assert_eq!(s.rust_path, "fynkel_scene::Transform");
        assert_eq!(
            s.fields.iter().map(|f| f.ty).collect::<Vec<_>>(),
            vec![FieldType::Vec3, FieldType::Quat, FieldType::Vec3]
        );
        assert_eq!(s.wire_size(), 40);
        assert_eq!(s.fields[0].doc, "Metres.");
    }

    #[test]
    fn nested_and_string_layouts() {
        let s = &SpawnCommand::SCHEMA;
        // temp_id u64 @0, transform 40 @8, parent u64 @48, name (u32,u32) @56
        assert_eq!(s.offsets(), vec![0, 8, 48, 56]);
        assert_eq!(s.wire_size(), 64);
        assert!(s.has_strings());
        assert_eq!(SceneReadyEvent::SCHEMA.wire_size(), 0);
        assert_eq!(SnapshotTransformRecord::SCHEMA.wire_size(), 88);
        // M3-08: entity @0, op @8, vector @12, rotation @24 → 40.
        assert_eq!(BodyCommandRecord::SCHEMA.offsets(), vec![0, 8, 12, 24]);
        assert_eq!(BodyCommandRecord::SCHEMA.wire_size(), 40);
        assert_eq!(ContactEvent::SCHEMA.wire_size(), 48);
        assert_eq!(PhysicsQueryRecord::SCHEMA.wire_size(), 88);
        assert_eq!(PhysicsHitRecord::SCHEMA.wire_size(), 48);
        assert_eq!(BodyStateRecord::SCHEMA.wire_size(), 40);
    }

    #[test]
    fn component_ids_match_the_contracts_table() {
        for s in ALL.iter().filter(|s| s.kind == SchemaKind::Component) {
            assert_eq!(
                fynkel_contracts::ComponentId(s.id).name(),
                Some(s.name),
                "{} id {} disagrees with fynkel_contracts::ids",
                s.name,
                s.id
            );
        }
    }

    #[test]
    fn registry_ids_are_unique_per_kind() {
        let mut seen = std::collections::HashSet::new();
        for s in ALL {
            assert!(seen.insert(s.name), "duplicate name {}", s.name);
            if s.kind != SchemaKind::Record {
                assert!(
                    seen.insert(format!("{}#{}", s.kind.name(), s.id).leak() as &str),
                    "duplicate id {} {}",
                    s.kind.name(),
                    s.id
                );
            }
        }
        assert_ne!(schema_fingerprint(), 0);
    }
}
