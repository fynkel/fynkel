//! Translation between the wire buffers and `fynkel_core` values.

use fynkel_contracts::{ComponentId, DiagnosticCode, Entity, EventKind};
use fynkel_core::input::{ActionState, Control, Device, InputEvent, Phase};
use fynkel_core::schema::*;
use fynkel_core::{Command, Diagnostic, Event, EventPayload, Rejected, SnapshotRecord, World};
use fynkel_scene::Transform;
use fynkel_wire::{decode_component_value, get_u32, WireRecord};

use crate::buffer::{self, BufferKind, ProtocolError, Writer};

/// Decodes a command buffer. Framing errors reject the whole buffer
/// (`Status::Protocol`); a bad body or unknown kind becomes an `Err` entry
/// that the world rejects with a diagnostic at the same batch index while
/// the rest is still applied.
/// Batch index plus the decoded command or why it could not be decoded.
pub type IndexedCommand = (u32, Result<Command, Rejected>);

pub fn decode_commands(buf: &[u8]) -> Result<Vec<IndexedCommand>, ProtocolError> {
    let (h, records, tail) = buffer::validate(buf, BufferKind::Commands)?;
    let mut out = Vec::with_capacity(h.record_count as usize);
    let mut off = 0;
    for index in 0..h.record_count {
        if off + CommandHeader::SIZE > records.len() {
            return Err(ProtocolError::Truncated { at: off });
        }
        let kind = get_u32(records, off);
        let size = get_u32(records, off + 4) as usize;
        off += CommandHeader::SIZE;
        if off + size > records.len() {
            return Err(ProtocolError::Truncated { at: off });
        }
        let body = &records[off..off + size];
        off += size;
        let decoded = decode_command(kind, body, tail).map_err(|code| Rejected {
            code,
            message: match code {
                DiagnosticCode::UNKNOWN_COMMAND => format!("unknown command kind {kind}"),
                _ => format!("command body of kind {kind} does not decode"),
            },
        });
        out.push((index, decoded));
    }
    Ok(out)
}

fn decode_command(kind: u32, body: &[u8], tail: &[u8]) -> Result<Command, DiagnosticCode> {
    let bad = |_| DiagnosticCode::INVALID_PAYLOAD;
    Ok(
        match CommandKind::from_u32(kind).ok_or(DiagnosticCode::UNKNOWN_COMMAND)? {
            CommandKind::Spawn => {
                let c = SpawnCommand::decode(body, tail).map_err(bad)?;
                Command::Spawn {
                    temp_id: c.temp_id,
                    transform: c.transform,
                    parent: c.parent,
                    name: if c.name.is_empty() {
                        None
                    } else {
                        Some(c.name)
                    },
                }
            }
            CommandKind::Destroy => {
                Command::Destroy(DestroyCommand::decode(body, tail).map_err(bad)?.entity)
            }
            CommandKind::SetTransform => {
                let c = SetTransformCommand::decode(body, tail).map_err(bad)?;
                Command::SetTransform {
                    entity: c.entity,
                    transform: c.transform,
                }
            }
            CommandKind::SetParent => {
                let c = SetParentCommand::decode(body, tail).map_err(bad)?;
                Command::SetParent {
                    child: c.child,
                    parent: c.parent,
                    keep_world: c.keep_world,
                }
            }
            CommandKind::SetName => {
                let c = SetNameCommand::decode(body, tail).map_err(bad)?;
                Command::SetName {
                    entity: c.entity,
                    name: c.name,
                }
            }
            // The component's bytes follow the header record; a component
            // id the wire cannot carry as a value (`Name`, the hierarchy,
            // an id this build does not know) is an invalid payload.
            CommandKind::SetComponent => {
                let c = SetComponentCommand::decode(body, tail).map_err(bad)?;
                let value = decode_component_value(
                    c.component,
                    &body[SetComponentCommand::SIZE.min(body.len())..],
                    tail,
                )
                .ok_or(DiagnosticCode::INVALID_PAYLOAD)?
                .map_err(bad)?;
                Command::SetComponent {
                    entity: c.entity,
                    value,
                }
            }
            CommandKind::RemoveComponent => {
                let c = RemoveComponentCommand::decode(body, tail).map_err(bad)?;
                Command::RemoveComponent {
                    entity: c.entity,
                    component: ComponentId(c.component),
                }
            }
        },
    )
}

pub fn encode_snapshot(buf: Vec<u8>, records: &[SnapshotRecord]) -> Vec<u8> {
    let mut w = Writer::begin(buf, BufferKind::Snapshot);
    for r in records {
        w.record(&SnapshotTransformRecord {
            entity: r.entity,
            local: r.local,
            world: r.world,
        });
    }
    w.finish()
}

pub fn encode_diagnostics(buf: Vec<u8>, diags: &[Diagnostic]) -> Vec<u8> {
    let mut w = Writer::begin(buf, BufferKind::Diagnostics);
    for d in diags {
        w.record(&DiagnosticRecord {
            command_index: d.command_index,
            code: d.code.0,
            entity: d.entity,
            component: d.component.0,
            lane: d.lane as u32,
            message: d.message.clone(),
        });
    }
    w.finish()
}

pub fn encode_events(buf: Vec<u8>, events: &[Event]) -> Vec<u8> {
    let mut w = Writer::begin(buf, BufferKind::Events);
    let mut scratch = Vec::new();
    for e in events {
        scratch.clear();
        let kind = e.payload.kind();
        encode_event_body(&e.payload, &mut scratch);
        let header = EventHeader {
            kind: kind.0,
            size: scratch.len() as u32,
            tick_lo: e.tick as u32,
            flags: 0,
        };
        // header + body count as one record
        let mut rec = vec![0u8; EventHeader::SIZE];
        header.encode(&mut rec, &mut fynkel_wire::StringTable::new());
        rec.extend_from_slice(&scratch);
        w.raw(&rec);
    }
    w.finish()
}

fn encode_event_body(p: &EventPayload, out: &mut Vec<u8>) {
    let mut strings = fynkel_wire::StringTable::new();
    fn push<T: WireRecord>(out: &mut Vec<u8>, v: &T, s: &mut fynkel_wire::StringTable) {
        let start = out.len();
        out.resize(start + T::SIZE, 0);
        v.encode(&mut out[start..], s);
    }
    match *p {
        EventPayload::Spawned { temp_id, entity } => {
            push(out, &SpawnedEvent { temp_id, entity }, &mut strings)
        }
        EventPayload::Destroyed { entity } => push(out, &DestroyedEvent { entity }, &mut strings),
        EventPayload::ParentChanged { child, parent } => {
            push(out, &ParentChangedEvent { child, parent }, &mut strings)
        }
        EventPayload::MissedDeadline { dropped_seconds } => {
            push(out, &MissedDeadlineEvent { dropped_seconds }, &mut strings)
        }
        EventPayload::SceneReady => push(out, &SceneReadyEvent, &mut strings),
        EventPayload::CollisionEnter { a, b }
        | EventPayload::CollisionStay { a, b }
        | EventPayload::CollisionExit { a, b }
        | EventPayload::TriggerEnter { a, b }
        | EventPayload::TriggerExit { a, b } => push(out, &ContactEvent { a, b }, &mut strings),
    }
    debug_assert!(strings.is_empty(), "event bodies carry no strings");
}

/// Sanity: every event kind has a body encoder (compile-time exhaustiveness
/// above) and the physics range maps to `ContactEvent`.
pub fn event_body_size(kind: EventKind) -> Option<usize> {
    Some(match kind {
        EventKind::SPAWNED => SpawnedEvent::SIZE,
        EventKind::DESTROYED => DestroyedEvent::SIZE,
        EventKind::PARENT_CHANGED => ParentChangedEvent::SIZE,
        EventKind::MISSED_DEADLINE => MissedDeadlineEvent::SIZE,
        EventKind::SCENE_READY => SceneReadyEvent::SIZE,
        k if k.is_physics() => ContactEvent::SIZE,
        _ => return None,
    })
}

// ---------------------------------------------------------------- queries

pub const QUERY_TRANSFORM: u32 = 1;
pub const QUERY_CHILDREN: u32 = 2;
pub const QUERY_PARENT: u32 = 3;
pub const QUERY_NAME: u32 = 4;
pub const QUERY_ALL_ENTITIES: u32 = 5;
pub const QUERY_COMPONENTS: u32 = 6;

pub fn query(
    world: &World,
    kind: u32,
    request_id: u32,
    entity: Entity,
    buf: Vec<u8>,
) -> Option<Vec<u8>> {
    let mut w = Writer::begin(buf, BufferKind::Query).request_id(request_id);
    let ecs = world.ecs();
    match kind {
        QUERY_TRANSFORM => {
            if let (Ok(local), Ok(wt)) = (
                ecs.copy::<Transform>(entity),
                ecs.get::<fynkel_scene::WorldTransform>(entity),
            ) {
                w.record(&SnapshotTransformRecord {
                    entity,
                    local,
                    world: Transform::from_matrix(wt.matrix),
                });
            }
        }
        QUERY_CHILDREN => {
            for c in fynkel_scene::children_of(ecs, entity) {
                w.raw(&c.to_bits().to_le_bytes());
            }
        }
        QUERY_PARENT => {
            let p = fynkel_scene::parent_of(ecs, entity)
                .map(Entity::to_bits)
                .unwrap_or(0);
            w.raw(&p.to_le_bytes());
        }
        QUERY_NAME => {
            if let Ok(n) = ecs.get::<fynkel_core::Name>(entity) {
                w.record(&*n);
            }
        }
        QUERY_ALL_ENTITIES => {
            for e in ecs.entities() {
                w.raw(&e.to_bits().to_le_bytes());
            }
        }
        // Every native component of the entity, each framed by a
        // `ComponentHeader` (the id and the body's size), id order; an
        // unknown entity is an empty response. What Dart's
        // `GameObject.get<T>()` reads (M3-07a).
        QUERY_COMPONENTS => {
            use fynkel_core::components::{
                Camera, DirectionalLight, Environment, MeshRenderer, PointLight, SpotLight,
            };
            if let Ok(t) = ecs.get::<Transform>(entity) {
                w.component(ComponentId::TRANSFORM.0, &*t);
            }
            if let Ok(n) = ecs.get::<fynkel_core::Name>(entity) {
                w.component(ComponentId::NAME.0, &*n);
            }
            if let Ok(v) = ecs.get::<MeshRenderer>(entity) {
                w.component(ComponentId::MESH_RENDERER.0, &*v);
            }
            if let Ok(v) = ecs.get::<Camera>(entity) {
                w.component(ComponentId::CAMERA.0, &*v);
            }
            if let Ok(v) = ecs.get::<DirectionalLight>(entity) {
                w.component(ComponentId::DIRECTIONAL_LIGHT.0, &*v);
            }
            if let Ok(v) = ecs.get::<PointLight>(entity) {
                w.component(ComponentId::POINT_LIGHT.0, &*v);
            }
            if let Ok(v) = ecs.get::<SpotLight>(entity) {
                w.component(ComponentId::SPOT_LIGHT.0, &*v);
            }
            if let Ok(v) = ecs.get::<Environment>(entity) {
                w.component(ComponentId::ENVIRONMENT.0, &*v);
            }
        }
        _ => return None,
    }
    Some(w.finish())
}

/// Decodes a host frame's raw control events. A record with an unknown
/// device or phase is skipped (a newer host); framing errors reject the
/// buffer.
pub fn decode_input_events(buf: &[u8]) -> Result<Vec<InputEvent>, ProtocolError> {
    let (h, records, tail) = buffer::validate(buf, BufferKind::InputEvents)?;
    let mut out = Vec::with_capacity(h.record_count as usize);
    let size = InputEventRecord::SIZE;
    for i in 0..h.record_count as usize {
        let off = i * size;
        if off + size > records.len() {
            return Err(ProtocolError::Truncated { at: off });
        }
        let Ok(r) = InputEventRecord::decode(&records[off..off + size], tail) else {
            return Err(ProtocolError::Truncated { at: off });
        };
        let (Some(device), Some(phase)) = (Device::from_u32(r.device), Phase::from_u32(r.phase))
        else {
            continue;
        };
        out.push(InputEvent {
            control: Control::new(device, r.control),
            phase,
            x: r.x,
            y: r.y,
        });
    }
    Ok(out)
}

pub fn encode_input_view(buf: Vec<u8>, actions: &[ActionState]) -> Vec<u8> {
    let mut w = Writer::begin(buf, BufferKind::InputActions);
    for a in actions {
        w.record(&InputActionRecord {
            name: a.name.clone(),
            kind: a.kind as u32,
            held: a.held,
            pressed: a.pressed,
            released: a.released,
            frame_pressed: a.frame_pressed,
            frame_released: a.frame_released,
            x: a.x,
            y: a.y,
        });
    }
    w.finish()
}
