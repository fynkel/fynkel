//! Translation between the wire buffers and `fynkel_core` values.

use fynkel_contracts::{AssetRef, ComponentId, Entity, EventKind};
use fynkel_core::input::{ActionState, Control, Device, InputEvent, Phase};
use fynkel_core::physics::{Hit, QueryKind, QuerySpec};
use fynkel_core::schema::*;
use fynkel_core::{Diagnostic, Event, EventPayload, ProjectionInfo, SnapshotRecord, World};
use fynkel_scene::Transform;
use fynkel_wire::WireRecord;

use crate::buffer::{self, BufferKind, ProtocolError, Writer};

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

/// `strings` is the string tail's buffer, kept by the caller so a warm
/// encode allocates nothing (foundation 4.8).
pub fn encode_events(buf: Vec<u8>, events: &[Event], strings: &mut Vec<u8>) -> Vec<u8> {
    let mut w = Writer::begin_reusing(buf, BufferKind::Events, std::mem::take(strings));
    for e in events {
        let kind = e.payload.kind();
        // header + body count as one record; the body's strings (a zone's
        // event name) go to the buffer's tail like any record's.
        w.raw_with(|out, strings| {
            let header_at = out.len();
            out.resize(header_at + EventHeader::SIZE, 0);
            let body_at = out.len();
            encode_event_body(&e.payload, out, strings);
            let size = (out.len() - body_at) as u32;
            EventHeader {
                kind: kind.0,
                size,
                tick_lo: e.tick as u32,
                flags: 0,
            }
            .encode(&mut out[header_at..body_at], strings);
        });
    }
    let (out, tail) = w.finish_reusing();
    *strings = tail;
    out
}

fn encode_event_body(p: &EventPayload, out: &mut Vec<u8>, strings: &mut fynkel_wire::StringTable) {
    fn push<T: WireRecord>(out: &mut Vec<u8>, v: &T, s: &mut fynkel_wire::StringTable) {
        let start = out.len();
        out.resize(start + T::SIZE, 0);
        v.encode(&mut out[start..], s);
    }
    match p {
        EventPayload::Destroyed { entity } => {
            push(out, &DestroyedEvent { entity: *entity }, strings)
        }
        EventPayload::ParentChanged { child, parent } => push(
            out,
            &ParentChangedEvent {
                child: *child,
                parent: *parent,
            },
            strings,
        ),
        EventPayload::MissedDeadline { dropped_seconds } => push(
            out,
            &MissedDeadlineEvent {
                dropped_seconds: *dropped_seconds,
            },
            strings,
        ),
        EventPayload::SceneReady { scene } => push(
            out,
            &SceneReadyEvent {
                scene: AssetRef::new(*scene),
            },
            strings,
        ),
        EventPayload::SceneUnloaded { scene, entities } => push(
            out,
            &SceneUnloadedEvent {
                scene: AssetRef::new(*scene),
                entities: *entities,
            },
            strings,
        ),
        EventPayload::ZoneEntered { zone, other, event }
        | EventPayload::ZoneExited { zone, other, event } => push(
            out,
            &ZoneEvent {
                zone: *zone,
                other: *other,
                event: event.clone(),
            },
            strings,
        ),
        EventPayload::AnimationEnded { entity, clip } => push(
            out,
            &AnimationEndedEvent {
                entity: *entity,
                clip: clip.clone(),
            },
            strings,
        ),
        EventPayload::AnimationMarker { entity, clip, name } => push(
            out,
            &AnimationMarkerEvent {
                entity: *entity,
                clip: clip.clone(),
                name: name.clone(),
            },
            strings,
        ),
        EventPayload::AnimatorStateEntered {
            entity,
            layer,
            state,
            from,
        } => push(
            out,
            &AnimatorStateEnteredEvent {
                entity: *entity,
                layer: *layer,
                state: state.clone(),
                from: from.clone(),
            },
            strings,
        ),
        EventPayload::AnimatorStateExited {
            entity,
            layer,
            state,
        } => push(
            out,
            &AnimatorStateExitedEvent {
                entity: *entity,
                layer: *layer,
                state: state.clone(),
            },
            strings,
        ),
        EventPayload::PlaybackEnded { entity, clip } => push(
            out,
            &PlaybackEndedEvent {
                entity: *entity,
                clip: clip.clone(),
            },
            strings,
        ),
        EventPayload::CollisionEnter(c)
        | EventPayload::CollisionStay(c)
        | EventPayload::CollisionExit(c)
        | EventPayload::TriggerEnter(c)
        | EventPayload::TriggerExit(c)
        | EventPayload::TriggerStay(c)
        | EventPayload::ControllerHit(c) => push(
            out,
            &ContactEvent {
                a: c.a,
                b: c.b,
                point: c.point,
                normal: c.normal,
                impulse: c.impulse,
                body_a: c.body_a,
                body_b: c.body_b,
                relative_velocity: c.relative_velocity,
                contact_count: c.point_count,
                contact0: contact_point(&c.points[0]),
                contact1: contact_point(&c.points[1]),
                contact2: contact_point(&c.points[2]),
                contact3: contact_point(&c.points[3]),
            },
            strings,
        ),
    }
}

fn contact_point(p: &fynkel_core::ContactPoint) -> ContactPointRecord {
    ContactPointRecord {
        point: p.point,
        normal: p.normal,
        separation: p.separation,
        impulse: p.impulse,
    }
}

/// Sanity: every event kind has a body encoder (compile-time exhaustiveness
/// above) and the physics range maps to `ContactEvent`.
pub fn event_body_size(kind: EventKind) -> Option<usize> {
    Some(match kind {
        EventKind::DESTROYED => DestroyedEvent::SIZE,
        EventKind::PARENT_CHANGED => ParentChangedEvent::SIZE,
        EventKind::MISSED_DEADLINE => MissedDeadlineEvent::SIZE,
        EventKind::SCENE_READY => SceneReadyEvent::SIZE,
        EventKind::SCENE_UNLOADED => SceneUnloadedEvent::SIZE,
        EventKind::ZONE_ENTERED | EventKind::ZONE_EXITED => ZoneEvent::SIZE,
        EventKind::ANIMATION_ENDED => AnimationEndedEvent::SIZE,
        EventKind::ANIMATION_MARKER => AnimationMarkerEvent::SIZE,
        EventKind::PLAYBACK_ENDED => PlaybackEndedEvent::SIZE,
        EventKind::ANIMATOR_STATE_ENTERED => AnimatorStateEnteredEvent::SIZE,
        EventKind::ANIMATOR_STATE_EXITED => AnimatorStateExitedEvent::SIZE,
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
pub const QUERY_BODY: u32 = 7;
pub const QUERY_ENTITIES: u32 = 8;
pub const QUERY_CHARACTER: u32 = 9;
pub const QUERY_SYSTEMS: u32 = 10;
pub const QUERY_ANIMATOR: u32 = 11;
pub const QUERY_AUDIO: u32 = 12;
pub const QUERY_AUDIO_STATS: u32 = 13;
pub const QUERY_ANIMATOR_CONTROLLER: u32 = 14;
pub const QUERY_ANIMATOR_PARAMETERS: u32 = 15;
/// The highest query kind: kinds `1..=QUERY_KINDS` each answer into their
/// own buffer (foundation 4.9).
pub const QUERY_KINDS: u32 = 15;

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
            // A destroyed target reads as None (ADR-0035), as Unity's
            // destroyed-object `null`.
            let live = |r: fynkel_contracts::ObjectRef| {
                if r.get().is_some_and(|t| ecs.contains(t)) {
                    r
                } else {
                    fynkel_contracts::ObjectRef::NONE
                }
            };
            // The table of built-in components (foundation 6.2), with
            // `fynkel.Name` after the Transform to keep id order.
            macro_rules! write_components {
                ($($(#[$doc:meta])* $variant:ident($ty:ty) = $id:ident [$($field:ident),*];)*) => {
                    $(
                        if let Ok(v) = ecs.get::<$ty>(entity) {
                            let refs: &[&str] = &[$(stringify!($field)),*];
                            if refs.is_empty() {
                                w.component(ComponentId::$id.0, &*v);
                            } else {
                                #[allow(unused_mut)]
                                let mut v = (*v).clone();
                                $(v.$field = live(v.$field);)*
                                w.component(ComponentId::$id.0, &v);
                            }
                        }
                        if ComponentId::$id == ComponentId::TRANSFORM {
                            if let Ok(n) = ecs.get::<fynkel_core::Name>(entity) {
                                w.component(ComponentId::NAME.0, &*n);
                            }
                        }
                    )*
                };
            }
            fynkel_core::builtin_components!(write_components);
        }
        // A source's playback state (M3-11); no source is an empty
        // response.
        QUERY_AUDIO => {
            if let Some(s) = world.audio_state(entity) {
                w.record(&s);
            }
        }
        // The audio engine's counters (M3-11); the entity is ignored.
        QUERY_AUDIO_STATS => {
            w.record(&world.audio_stats());
        }
        // An animator's playback state (M3-19); no animator is an empty
        // response.
        QUERY_ANIMATOR => {
            if let Some(s) = world.animator_state(entity) {
                w.record(&s);
            }
        }
        // The controller an animator runs (ADR-0032): one record per
        // layer, then one per parameter; none without a running controller.
        QUERY_ANIMATOR_CONTROLLER => {
            if let Some((layers, _)) = world.animator_controller_state(entity) {
                for l in &layers {
                    w.record(l);
                }
            }
        }
        QUERY_ANIMATOR_PARAMETERS => {
            if let Some((_, parameters)) = world.animator_controller_state(entity) {
                for p in &parameters {
                    w.record(p);
                }
            }
        }
        // The last step's per-phase wall time (M3-10), one record per
        // `SystemKind`; the entity is ignored.
        QUERY_SYSTEMS => {
            for t in world.system_times() {
                w.record(t);
            }
        }
        // The body's motion state (M3-08); no body is an empty response.
        QUERY_BODY => {
            if let Some(s) = world.physics().body_state(entity) {
                w.record(&BodyStateRecord {
                    entity,
                    linear_velocity: s.linear_velocity,
                    angular_velocity: s.angular_velocity,
                    sleeping: s.sleeping,
                });
            }
        }
        // A character's held state (M3-09); no controller is an empty
        // response.
        QUERY_CHARACTER => {
            if let Some(s) = world.physics().character_state(entity) {
                w.record(&CharacterStateRecord {
                    entity,
                    grounded: s.grounded,
                    sliding: s.sliding,
                    velocity: s.velocity,
                    collision_flags: s.collision_flags,
                });
            }
        }
        // Every live entity with its parent, name, layer, tags and own
        // active flag (M3-08b, ADR-0020), entity order: the hierarchy, the
        // names and the tags in one call, for `find` and `findWithTag`.
        QUERY_ENTITIES => {
            for e in ecs.entities() {
                let parent = fynkel_scene::parent_of(ecs, e).unwrap_or(Entity::NULL);
                let name = ecs
                    .get::<fynkel_core::Name>(e)
                    .map(|n| n.value.clone())
                    .unwrap_or_default();
                w.record(&EntityInfoRecord {
                    entity: e,
                    parent,
                    name,
                    layer: ecs.copy::<fynkel_scene::Layer>(e).map_or(0, |l| l.layer),
                    tags: ecs.copy::<fynkel_scene::Tags>(e).map_or(0, |t| t.bits),
                    active: fynkel_scene::is_active_self(ecs, e),
                });
            }
        }
        _ => return None,
    }
    Some(w.finish())
}

/// Answers a `PhysicsQueries` buffer (M3-08): one `PhysicsHitRecord` per
/// hit, in query order, nearest first within a query; a malformed record
/// is a protocol error, a query that cannot be answered (`mesh` shape,
/// zero direction, non-finite values) yields no hits.
pub fn physics_queries(world: &World, buf: &[u8], out: Vec<u8>) -> Result<Vec<u8>, ProtocolError> {
    let (h, records, tail) = buffer::validate(buf, BufferKind::PhysicsQueries)?;
    let size = PhysicsQueryRecord::SIZE;
    let mut w = Writer::begin(out, BufferKind::Query);
    let mut hits: Vec<Hit> = Vec::new();
    for i in 0..h.record_count as usize {
        let off = i * size;
        if off + size > records.len() {
            return Err(ProtocolError::Truncated { at: off });
        }
        let Ok(q) = PhysicsQueryRecord::decode(&records[off..off + size], tail) else {
            return Err(ProtocolError::Truncated { at: off });
        };
        let spec = QuerySpec {
            kind: match q.kind {
                PhysicsQueryKind::Raycast => QueryKind::Raycast,
                PhysicsQueryKind::ShapeCast => QueryKind::ShapeCast,
                PhysicsQueryKind::Overlap => QueryKind::Overlap,
            },
            shape: q.shape,
            size: q.size,
            origin: q.origin,
            rotation: q.rotation,
            direction: q.direction,
            max_distance: q.max_distance,
            mask: q.mask,
            exclude: q.exclude,
            max_hits: q.max_hits,
            hit_sensors: q.hit_sensors,
        };
        hits.clear();
        world.physics().query(&spec, &mut hits);
        for hit in &hits {
            w.record(&PhysicsHitRecord {
                query: q.id,
                entity: hit.entity,
                point: hit.point,
                normal: hit.normal,
                distance: hit.distance,
            });
        }
    }
    Ok(w.finish())
}

/// Answers a `ProjectionAnchors` buffer (M3-13): every anchor projected
/// into a `width` × `height` viewport with the frame's camera, one
/// `ProjectedPointRecord` per anchor in order, in a `Query` buffer. At
/// most `MAX_PROJECTION_ANCHORS` are projected (the rest `DROPPED`, with
/// the world's `TOO_MANY_ANCHORS` diagnostic).
pub fn project(
    world: &mut World,
    width: u32,
    height: u32,
    buf: &[u8],
    out: Vec<u8>,
    anchors: &mut Vec<ProjectionAnchorRecord>,
    points: &mut Vec<ProjectedPointRecord>,
) -> Result<(Vec<u8>, ProjectionInfo), ProtocolError> {
    let (h, records, tail) = buffer::validate(buf, BufferKind::ProjectionAnchors)?;
    let size = ProjectionAnchorRecord::SIZE;
    anchors.clear();
    for i in 0..h.record_count as usize {
        let off = i * size;
        if off + size > records.len() {
            return Err(ProtocolError::Truncated { at: off });
        }
        let Ok(a) = ProjectionAnchorRecord::decode(&records[off..off + size], tail) else {
            return Err(ProtocolError::Truncated { at: off });
        };
        anchors.push(a);
    }
    let info = world.project(width, height, anchors, points);
    let mut w = Writer::begin(out, BufferKind::Query);
    for p in points.iter() {
        w.record(p);
    }
    Ok((w.finish(), info))
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

/// `record` and `strings` are scratch the caller keeps: the record's name
/// and the string tail reuse their buffers, so a warm view encodes without
/// allocating (foundation 4.8).
pub fn encode_input_view(
    buf: Vec<u8>,
    actions: &[ActionState],
    record: &mut InputActionRecord,
    strings: &mut Vec<u8>,
) -> Vec<u8> {
    let mut w = Writer::begin_reusing(buf, BufferKind::InputActions, std::mem::take(strings));
    for a in actions {
        record.name.clear();
        record.name.push_str(&a.name);
        record.kind = a.kind as u32;
        record.held = a.held;
        record.pressed = a.pressed;
        record.released = a.released;
        record.frame_pressed = a.frame_pressed;
        record.frame_released = a.frame_released;
        record.x = a.x;
        record.y = a.y;
        record.frame_x = a.frame_x;
        record.frame_y = a.frame_y;
        w.record(record);
    }
    let (out, tail) = w.finish_reusing();
    *strings = tail;
    out
}

pub fn empty_input_record() -> InputActionRecord {
    InputActionRecord {
        name: String::new(),
        kind: 0,
        held: false,
        pressed: false,
        released: false,
        frame_pressed: false,
        frame_released: false,
        x: 0.0,
        y: 0.0,
        frame_x: 0.0,
        frame_y: 0.0,
    }
}
