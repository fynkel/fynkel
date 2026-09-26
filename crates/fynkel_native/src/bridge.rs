//! Translation between the wire buffers and `fynkel_core` values.

use fynkel_contracts::{AssetRef, ComponentId, DiagnosticCode, Entity, EventKind};
use fynkel_core::input::{ActionState, Control, Device, InputEvent, Phase};
use fynkel_core::physics::{Hit, QueryKind, QuerySpec};
use fynkel_core::schema::*;
use fynkel_core::{
    AnimateOp as CoreAnimateOp, AudioOp as CoreAudioOp, BodyCommand, Command, Diagnostic, Event,
    EventPayload, ProjectionInfo, Rejected, SnapshotRecord, World,
};
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
            CommandKind::SetActive => {
                let c = SetActiveCommand::decode(body, tail).map_err(bad)?;
                Command::SetActive {
                    entity: c.entity,
                    active: c.active,
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
            CommandKind::Body => {
                let c = BodyCommandRecord::decode(body, tail).map_err(bad)?;
                let (v, r) = (c.vector, c.rotation);
                let op = match c.op {
                    BodyOp::ApplyForce => BodyCommand::ApplyForce(v),
                    BodyOp::ApplyImpulse => BodyCommand::ApplyImpulse(v),
                    BodyOp::ApplyTorque => BodyCommand::ApplyTorque(v),
                    BodyOp::ApplyTorqueImpulse => BodyCommand::ApplyTorqueImpulse(v),
                    BodyOp::SetLinearVelocity => BodyCommand::SetLinearVelocity(v),
                    BodyOp::SetAngularVelocity => BodyCommand::SetAngularVelocity(v),
                    BodyOp::SetKinematicTarget => BodyCommand::SetKinematicTarget {
                        position: v,
                        rotation: r,
                    },
                    BodyOp::Teleport => BodyCommand::Teleport {
                        position: v,
                        rotation: r,
                    },
                    BodyOp::Move => BodyCommand::Move(v),
                    BodyOp::Jump => BodyCommand::Jump(v.y),
                };
                Command::Body {
                    entity: c.entity,
                    op,
                }
            }
            CommandKind::Animate => {
                let c = AnimateCommandRecord::decode(body, tail).map_err(bad)?;
                let op = match c.op {
                    AnimateOp::Play => CoreAnimateOp::Play {
                        clip: c.clip,
                        looping: c.looping,
                        speed: c.speed,
                    },
                    AnimateOp::Crossfade => CoreAnimateOp::Crossfade {
                        clip: c.clip,
                        looping: c.looping,
                        speed: c.speed,
                        seconds: c.seconds,
                    },
                    AnimateOp::Stop => CoreAnimateOp::Stop,
                    AnimateOp::Pause => CoreAnimateOp::Pause,
                    AnimateOp::Resume => CoreAnimateOp::Resume,
                    AnimateOp::SetSpeed => CoreAnimateOp::SetSpeed(c.speed),
                    AnimateOp::SetTime => CoreAnimateOp::SetTime(c.seconds),
                    AnimateOp::SetWeight => CoreAnimateOp::SetWeight(c.value),
                    AnimateOp::SetParameter => CoreAnimateOp::SetParameter {
                        name: c.name,
                        value: c.value,
                    },
                    AnimateOp::SetFloat
                    | AnimateOp::SetInteger
                    | AnimateOp::SetBool
                    | AnimateOp::SetTrigger
                    | AnimateOp::ResetTrigger => {
                        use fynkel_core::animation::controller::Value;
                        let value = match c.op {
                            AnimateOp::SetFloat => Value::Float(c.value),
                            AnimateOp::SetInteger => Value::Int(c.integer),
                            AnimateOp::SetBool => Value::Bool(c.value != 0.0),
                            AnimateOp::SetTrigger => Value::Bool(true),
                            _ => Value::Bool(false),
                        };
                        CoreAnimateOp::SetControllerParameter {
                            name: c.name,
                            value,
                        }
                    }
                    AnimateOp::PlayState | AnimateOp::CrossFadeState => CoreAnimateOp::PlayState {
                        state: c.name,
                        layer: u32::try_from(c.layer).ok(),
                        seconds: if c.op == AnimateOp::CrossFadeState {
                            c.seconds
                        } else {
                            0.0
                        },
                        offset: c.value,
                    },
                    AnimateOp::PlayOneShot => CoreAnimateOp::PlayOneShot {
                        motion: c.clip,
                        slot: c.name,
                        blend_in: c.seconds,
                        blend_out: c.value,
                        speed: c.speed,
                    },
                    AnimateOp::StopOneShot => CoreAnimateOp::StopOneShot {
                        slot: c.name,
                        blend_out: c.value,
                    },
                    AnimateOp::SetIkTarget => CoreAnimateOp::SetIkTarget {
                        goal: c.name,
                        position: c.point,
                        weight: c.value,
                    },
                    AnimateOp::ClearIkTarget => CoreAnimateOp::ClearIkTarget { goal: c.name },
                };
                Command::Animate {
                    entity: c.entity,
                    op,
                }
            }
            CommandKind::Audio => {
                let c = AudioCommandRecord::decode(body, tail).map_err(bad)?;
                let op = match c.op {
                    AudioOp::Play => CoreAudioOp::Play,
                    AudioOp::Stop => CoreAudioOp::Stop,
                    AudioOp::Pause => CoreAudioOp::Pause,
                    AudioOp::Resume => CoreAudioOp::Resume,
                    AudioOp::SetVolume => CoreAudioOp::SetVolume(c.value),
                    AudioOp::SetPitch => CoreAudioOp::SetPitch(c.value),
                };
                Command::Audio {
                    entity: c.entity,
                    op,
                }
            }
            CommandKind::PlaySound => {
                let c = PlaySoundCommandRecord::decode(body, tail).map_err(bad)?;
                Command::PlaySound {
                    clip: c.clip,
                    bus: c.bus,
                    volume: c.volume,
                    pitch: c.pitch,
                    priority: c.priority,
                }
            }
            CommandKind::AudioBus => {
                let c = AudioBusCommandRecord::decode(body, tail).map_err(bad)?;
                Command::AudioBus {
                    bus: c.bus,
                    volume: c.volume,
                    muted: c.muted,
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
    w.finish()
}

fn encode_event_body(p: &EventPayload, out: &mut Vec<u8>, strings: &mut fynkel_wire::StringTable) {
    fn push<T: WireRecord>(out: &mut Vec<u8>, v: &T, s: &mut fynkel_wire::StringTable) {
        let start = out.len();
        out.resize(start + T::SIZE, 0);
        v.encode(&mut out[start..], s);
    }
    match p {
        EventPayload::Spawned { temp_id, entity } => push(
            out,
            &SpawnedEvent {
                temp_id: *temp_id,
                entity: *entity,
            },
            strings,
        ),
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
        | EventPayload::TriggerExit(c) => push(
            out,
            &ContactEvent {
                a: c.a,
                b: c.b,
                point: c.point,
                normal: c.normal,
                impulse: c.impulse,
            },
            strings,
        ),
    }
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
            if let Ok(v) = ecs.get::<fynkel_core::RigidBody>(entity) {
                w.component(ComponentId::RIGID_BODY.0, &*v);
            }
            if let Ok(v) = ecs.get::<fynkel_core::Collider>(entity) {
                w.component(ComponentId::COLLIDER.0, &*v);
            }
            if let Ok(v) = ecs.get::<fynkel_core::CharacterController>(entity) {
                w.component(ComponentId::CHARACTER_CONTROLLER.0, &*v);
            }
            if let Ok(v) = ecs.get::<fynkel_core::ThirdPersonCamera>(entity) {
                w.component(ComponentId::THIRD_PERSON_CAMERA.0, &*v);
            }
            if let Ok(v) = ecs.get::<fynkel_core::Rotator>(entity) {
                w.component(ComponentId::ROTATOR.0, &*v);
            }
            if let Ok(v) = ecs.get::<fynkel_core::Oscillate>(entity) {
                w.component(ComponentId::OSCILLATE.0, &*v);
            }
            if let Ok(v) = ecs.get::<fynkel_core::Follow>(entity) {
                w.component(ComponentId::FOLLOW.0, &*v);
            }
            if let Ok(v) = ecs.get::<fynkel_core::LookAtCamera>(entity) {
                w.component(ComponentId::LOOK_AT_CAMERA.0, &*v);
            }
            if let Ok(v) = ecs.get::<fynkel_core::LifetimeDespawn>(entity) {
                w.component(ComponentId::LIFETIME_DESPAWN.0, &*v);
            }
            if let Ok(v) = ecs.get::<fynkel_core::Emitter>(entity) {
                w.component(ComponentId::EMITTER.0, &*v);
            }
            if let Ok(v) = ecs.get::<fynkel_core::TriggerZone>(entity) {
                w.component(ComponentId::TRIGGER_ZONE.0, &*v);
            }
            if let Ok(v) = ecs.get::<fynkel_core::Animator>(entity) {
                w.component(ComponentId::ANIMATOR.0, &*v);
            }
            if let Ok(v) = ecs.get::<fynkel_core::Skin>(entity) {
                w.component(ComponentId::SKIN.0, &*v);
            }
            if let Ok(v) = ecs.get::<fynkel_core::AudioSource>(entity) {
                w.component(ComponentId::AUDIO_SOURCE.0, &*v);
            }
            if let Ok(v) = ecs.get::<fynkel_core::AudioListener>(entity) {
                w.component(ComponentId::AUDIO_LISTENER.0, &*v);
            }
            if let Ok(v) = ecs.get::<fynkel_core::components::Volume>(entity) {
                w.component(ComponentId::VOLUME.0, &*v);
            }
            if let Ok(v) = ecs.get::<fynkel_scene::Layer>(entity) {
                w.component(ComponentId::LAYER.0, &*v);
            }
            if let Ok(v) = ecs.get::<fynkel_scene::Tags>(entity) {
                w.component(ComponentId::TAGS.0, &*v);
            }
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
            frame_x: a.frame_x,
            frame_y: a.frame_y,
        });
    }
    w.finish()
}
