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
use fynkel_scene::Transform;
use fynkel_schema_derive::Schema;

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
}

impl CommandKind {
    pub fn from_u32(v: u32) -> Option<Self> {
        Some(match v {
            1 => Self::Spawn,
            2 => Self::Destroy,
            3 => Self::SetTransform,
            4 => Self::SetParent,
            5 => Self::SetName,
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

/// Physics-lane events share one body (collision/trigger enter/stay/exit).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "event", id = 256, name = "fynkel.ev.Contact")]
pub struct ContactEvent {
    pub a: Entity,
    pub b: Entity,
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
