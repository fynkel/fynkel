//! Exercises the C ABI from Rust exactly as a host would (§5.7: batched
//! traffic, stale handles fail safely, buffers validated, panic boundary).

use std::ptr;

use fynkel_contracts::{DiagnosticCode, Entity, EventKind, Status, WorldId};
use fynkel_core::schema::*;
use fynkel_core::Lane;
use fynkel_native::buffer::{self, BufferKind, Writer};
use fynkel_native::*;
use fynkel_scene::Transform;
use fynkel_wire::{get_u32, get_u64, WireRecord};
use glam::Vec3;

struct Host {
    rt: *mut FynkelRuntime,
    w: *mut FynkelWorld,
}

impl Host {
    fn new() -> Self {
        let rt = fynkel_runtime_create();
        let w = unsafe { fynkel_world_create(rt, ptr::null()) };
        assert!(!w.is_null());
        Self { rt, w }
    }

    fn submit(&self, lane: Lane, buf: &[u8]) -> (Status, FynkelSubmitResult) {
        let mut r = FynkelSubmitResult::default();
        let s = unsafe {
            fynkel_world_submit_commands(
                self.w,
                lane as u32,
                buf.as_ptr(),
                buf.len() as u32,
                &mut r,
            )
        };
        (s, r)
    }

    fn step(&self) -> (Status, FynkelStepResult) {
        let mut r = FynkelStepResult::default();
        let s = unsafe { fynkel_world_step(self.w, &mut r) };
        (s, r)
    }

    fn view(v: FynkelBufferView) -> &'static [u8] {
        if v.length == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(v.data, v.length as usize) }
        }
    }

    fn events(&self) -> Vec<(u32, Vec<u8>)> {
        let b = Self::view(unsafe { fynkel_world_drain_events(self.w) });
        let (h, records, _) = buffer::validate(b, BufferKind::Events).unwrap();
        let mut out = Vec::new();
        let mut off = 0;
        for _ in 0..h.record_count {
            let kind = get_u32(records, off);
            let size = get_u32(records, off + 4) as usize;
            off += EventHeader::SIZE;
            out.push((kind, records[off..off + size].to_vec()));
            off += size;
        }
        out
    }

    /// `events` with the batch's string tail (for records with strings).
    fn events_with_tail(&self) -> (Vec<(u32, Vec<u8>)>, Vec<u8>) {
        let b = Self::view(unsafe { fynkel_world_drain_events(self.w) });
        let (h, records, tail) = buffer::validate(b, BufferKind::Events).unwrap();
        let mut out = Vec::new();
        let mut off = 0;
        for _ in 0..h.record_count {
            let kind = get_u32(records, off);
            let size = get_u32(records, off + 4) as usize;
            off += EventHeader::SIZE;
            out.push((kind, records[off..off + size].to_vec()));
            off += size;
        }
        (out, tail.to_vec())
    }

    fn spawned(&self) -> Vec<Entity> {
        self.events()
            .iter()
            .filter(|(k, _)| *k == EventKind::SPAWNED.0)
            .map(|(_, b)| SpawnedEvent::decode(b, &[]).unwrap().entity)
            .collect()
    }

    fn diagnostics(&self) -> Vec<DiagnosticRecord> {
        let b = Self::view(unsafe { fynkel_world_diagnostics(self.w) });
        let (h, records, tail) = buffer::validate(b, BufferKind::Diagnostics).unwrap();
        (0..h.record_count as usize)
            .map(|i| {
                DiagnosticRecord::decode(&records[i * DiagnosticRecord::SIZE..], tail).unwrap()
            })
            .collect()
    }

    fn snapshot(&self) -> Vec<SnapshotTransformRecord> {
        let b = Self::view(unsafe { fynkel_world_snapshot(self.w) });
        let (h, records, tail) = buffer::validate(b, BufferKind::Snapshot).unwrap();
        (0..h.record_count as usize)
            .map(|i| {
                SnapshotTransformRecord::decode(&records[i * SnapshotTransformRecord::SIZE..], tail)
                    .unwrap()
            })
            .collect()
    }

    fn query(&self, kind: u32, entity: Entity) -> (Status, u32, Vec<u8>, Vec<u8>) {
        let desc = FynkelQueryDesc {
            kind,
            request_id: 77,
            entity: entity.to_bits(),
        };
        let mut v = FynkelBufferView {
            data: ptr::null(),
            length: 0,
        };
        let s = unsafe { fynkel_world_query(self.w, &desc, &mut v) };
        let b = Self::view(v);
        if b.is_empty() {
            return (s, 0, Vec::new(), Vec::new());
        }
        let (h, records, tail) = buffer::validate(b, BufferKind::Query).unwrap();
        assert_eq!(h.request_id, 77);
        (s, h.record_count, records.to_vec(), tail.to_vec())
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        unsafe {
            fynkel_world_destroy(self.w);
            fynkel_runtime_destroy(self.rt);
        }
    }
}

/// One command batch: each entry is (kind, body).
macro_rules! batch {
    ($( ($kind:expr, $body:expr) ),* $(,)?) => {{
        let mut w = Writer::begin(Vec::new(), BufferKind::Commands);
        $( w.command($kind, &$body); )*
        w.finish()
    }};
}

fn at(x: f32) -> Transform {
    Transform::from_position(Vec3::new(x, 0.0, 0.0))
}

fn spawn(temp_id: u64, x: f32, parent: Option<Entity>, name: &str) -> SpawnCommand {
    SpawnCommand {
        temp_id,
        transform: at(x),
        parent,
        name: name.into(),
    }
}

#[test]
fn version_and_fingerprint() {
    assert_eq!(fynkel_abi_version(), 1);
    assert_eq!(
        fynkel_schema_fingerprint(),
        fynkel_core::schema::schema_fingerprint()
    );
}

#[test]
fn spawn_set_destroy_round_trip_with_diagnostics() {
    let h = Host::new();
    let (s, r) = h.submit(
        Lane::Simulation,
        &batch![(1, spawn(1, 1.0, None, "")), (1, spawn(2, 1.0, None, ""))],
    );
    assert_eq!((s, r.accepted, r.rejected), (Status::Ok, 2, 0));
    let (s, r) = h.step();
    assert_eq!(
        (s, r.commands_applied, r.live_entities, r.event_records),
        (Status::Ok, 2, 2, 2)
    );

    let ev = h.events();
    assert_eq!(ev.len(), 2);
    let spawned: Vec<SpawnedEvent> = ev
        .iter()
        .map(|(_, b)| SpawnedEvent::decode(b, &[]).unwrap())
        .collect();
    assert_eq!((spawned[0].temp_id, spawned[1].temp_id), (1, 2));
    let (a, b) = (spawned[0].entity, spawned[1].entity);
    assert!(h.events().is_empty(), "drained");

    let handles = [a.to_bits(), b.to_bits()];
    assert_eq!(
        unsafe { fynkel_world_subscribe(h.w, 1, handles.as_ptr(), 2, 1) },
        Status::Ok
    );
    let (s, _) = h.submit(
        Lane::Simulation,
        &batch![
            (
                3,
                SetTransformCommand {
                    entity: a,
                    transform: at(5.0)
                }
            ),
            (2, DestroyCommand { entity: b }),
        ],
    );
    assert_eq!(s, Status::Ok);
    let (s, r) = h.step();
    assert_eq!(
        (s, r.commands_applied, r.snapshot_records, r.live_entities),
        (Status::Ok, 2, 1, 1)
    );
    let snap = h.snapshot();
    assert_eq!(snap[0].entity, a);
    assert_eq!(snap[0].local.position.x, 5.0);
    assert_eq!(snap[0].world.position.x, 5.0);

    // Stale handle, unknown kind, non-finite payload: three diagnostics with
    // their batch indexes; the valid command still applies.
    let (s, r) = h.submit(
        Lane::Simulation,
        &batch![
            (2, DestroyCommand { entity: b }),
            (99, DestroyCommand { entity: a }),
            (
                3,
                SetTransformCommand {
                    entity: a,
                    transform: Transform {
                        position: Vec3::NAN,
                        ..Transform::IDENTITY
                    }
                }
            ),
            (
                3,
                SetTransformCommand {
                    entity: a,
                    transform: at(6.0)
                }
            ),
        ],
    );
    assert_eq!(
        (s, r.accepted, r.rejected),
        (Status::Ok, 4, 0),
        "simulation lane validates at the tick"
    );
    let (s, r) = h.step();
    assert_eq!(s, Status::CommandsRejected);
    assert_eq!((r.commands_applied, r.commands_rejected), (1, 3));
    let d = h.diagnostics();
    let codes: Vec<(u32, u32)> = d.iter().map(|d| (d.command_index, d.code)).collect();
    assert_eq!(
        codes,
        vec![
            (0, DiagnosticCode::STALE_ENTITY.0),
            (1, DiagnosticCode::UNKNOWN_COMMAND.0),
            (2, DiagnosticCode::INVALID_PAYLOAD.0)
        ]
    );
    assert_eq!(d[0].entity, b);
    assert_eq!(d[0].message, "destroy: stale or unknown entity");
    assert_eq!(h.snapshot()[0].local.position.x, 6.0);
}

#[test]
fn presentation_lane_and_frame_plan() {
    let h = Host::new();
    h.submit(Lane::Simulation, &batch![(1, spawn(1, 0.0, None, "cam"))]);
    h.step();
    let e = h.spawned()[0];

    let mut plan = FynkelFramePlan::default();
    assert_eq!(
        unsafe { fynkel_world_begin_frame(h.w, 0.001, &mut plan) },
        Status::Ok
    );
    assert_eq!(plan.steps, 0);
    let (s, r) = h.submit(
        Lane::Presentation,
        &batch![
            (
                3,
                SetTransformCommand {
                    entity: e,
                    transform: at(3.0)
                }
            ),
            (2, DestroyCommand { entity: e }),  // wrong lane
            (99, DestroyCommand { entity: e }), // unknown, rejected now
        ],
    );
    assert_eq!(
        (s, r.accepted, r.rejected),
        (Status::CommandsRejected, 1, 2)
    );
    let d = h.diagnostics();
    assert_eq!(d[0].code, DiagnosticCode::WRONG_LANE.0);
    assert_eq!((d[0].command_index, d[0].lane), (1, 1));
    assert_eq!(
        (d[1].command_index, d[1].code),
        (2, DiagnosticCode::UNKNOWN_COMMAND.0)
    );
    assert_eq!(
        unsafe { (*h.w).core() }.extract().records[0].world.w_axis.x,
        3.0
    );

    let (s, n, body, tail) = h.query(bridge::QUERY_NAME, e);
    assert_eq!((s, n), (Status::Ok, 1));
    assert_eq!(
        fynkel_core::Name::decode(&body, &tail).unwrap().value,
        "cam"
    );

    let mut stats = FynkelWorldStats::default();
    assert_eq!(unsafe { fynkel_world_stats(h.w, &mut stats) }, Status::Ok);
    assert_eq!(
        (stats.submit_calls, stats.step_calls, stats.query_calls),
        (2, 1, 1)
    );
    assert!(stats.bytes_in > 0 && stats.bytes_out > 0);
    assert_eq!(
        stats.staged_commands, 1,
        "presentation transform staged for next tick"
    );
    assert_eq!(stats.subscriptions, 0);
}

#[test]
fn hierarchy_queries() {
    let h = Host::new();
    h.submit(Lane::Simulation, &batch![(1, spawn(1, 1.0, None, ""))]);
    h.step();
    let root = h.spawned()[0];
    h.submit(
        Lane::Simulation,
        &batch![
            (1, spawn(1, 1.0, Some(root), "")),
            (1, spawn(2, 2.0, Some(root), ""))
        ],
    );
    h.step();
    let kids = h.spawned();
    let (c1, c2) = (kids[0], kids[1]);

    let (s, n, body, _) = h.query(bridge::QUERY_CHILDREN, root);
    assert_eq!((s, n), (Status::Ok, 2));
    assert_eq!(get_u64(&body, 0), c1.to_bits());
    assert_eq!(get_u64(&body, 8), c2.to_bits());
    let (_, n, body, _) = h.query(bridge::QUERY_PARENT, c1);
    assert_eq!(n, 1);
    assert_eq!(get_u64(&body, 0), root.to_bits());
    let (_, n, body, _) = h.query(bridge::QUERY_TRANSFORM, c2);
    assert_eq!(n, 1);
    let t = SnapshotTransformRecord::decode(&body, &[]).unwrap();
    assert_eq!(t.world.position.x, 3.0, "root 1 + local 2");
    let (_, n, _, _) = h.query(bridge::QUERY_ALL_ENTITIES, Entity::NULL);
    assert_eq!(n, 3);
    // The whole table in one answer (M3-08b): entity, parent, name.
    h.submit(
        Lane::Simulation,
        &batch![(
            5,
            SetNameCommand {
                entity: c1,
                name: "Left".into()
            }
        )],
    );
    h.step();
    let (s, n, body, tail) = h.query(bridge::QUERY_ENTITIES, Entity::NULL);
    assert_eq!((s, n), (Status::Ok, 3));
    let rows: Vec<EntityInfoRecord> = (0..3)
        .map(|i| EntityInfoRecord::decode(&body[i * EntityInfoRecord::SIZE..], &tail).unwrap())
        .collect();
    let row = |e: Entity| rows.iter().find(|r| r.entity == e).unwrap();
    assert_eq!(
        (row(root).parent, row(root).name.as_str()),
        (Entity::NULL, "")
    );
    assert_eq!((row(c1).parent, row(c1).name.as_str()), (root, "Left"));
    assert_eq!(row(c2).parent, root);
    let (s, _, _, _) = h.query(999, root);
    assert_eq!(s, Status::InvalidArgument);
    let (_, n, _, _) = h.query(bridge::QUERY_TRANSFORM, Entity::NULL);
    assert_eq!(n, 0, "unknown entity: empty response, not an error");

    h.submit(
        Lane::Simulation,
        &batch![(
            4,
            SetParentCommand {
                child: c2,
                parent: None,
                keep_world: true
            }
        )],
    );
    h.step();
    let (_, _, body, _) = h.query(bridge::QUERY_TRANSFORM, c2);
    let t = SnapshotTransformRecord::decode(&body, &[]).unwrap();
    assert!((t.local.position.x - 3.0).abs() < 1e-5);
    assert_eq!(h.events()[0].0, EventKind::PARENT_CHANGED.0);
}

/// A `SetComponent` command: the header record then the value's bytes.
fn set_component(entity: Entity, value: &fynkel_core::components::ComponentValue) -> Vec<u8> {
    use fynkel_wire::{component_value_size, encode_component_value, StringTable};
    let mut w = Writer::begin(Vec::new(), BufferKind::Commands);
    let mut body = vec![0u8; SetComponentCommand::SIZE + component_value_size(value)];
    let mut strings = StringTable::new();
    SetComponentCommand {
        entity,
        component: value.id().0,
    }
    .encode(&mut body, &mut strings);
    encode_component_value(value, &mut body[SetComponentCommand::SIZE..], &mut strings);
    // The writer frames a typed record; hand-frame the composite body.
    w.raw_command(6, &body, strings);
    w.finish()
}

#[test]
fn component_commands_and_query() {
    use fynkel_core::components::{ComponentValue, MeshRenderer, PointLight};
    let h = Host::new();
    h.submit(Lane::Simulation, &batch![(1, spawn(1, 1.0, None, "lamp"))]);
    h.step();
    let lamp = h.spawned()[0];

    // Add a light and a renderer (with strings: the material's sub id).
    let light = PointLight {
        color: Vec3::new(1.0, 0.5, 0.25),
        intensity: 800.0,
        range: 12.0,
    };
    let renderer = MeshRenderer {
        mesh: fynkel_contracts::AssetRef::with_sub(
            fynkel_contracts::AssetId::from_bytes([7; 16]),
            "mesh/0",
        ),
        material: fynkel_contracts::AssetRef::new(fynkel_contracts::AssetId::from_bytes([8; 16])),
        cast_shadows: true,
        receive_shadows: false,
        visible: true,
    };
    let (s, r) = h.submit(
        Lane::Simulation,
        &set_component(lamp, &ComponentValue::PointLight(light)),
    );
    assert_eq!((s, r.rejected), (Status::Ok, 0));
    h.submit(
        Lane::Simulation,
        &set_component(lamp, &ComponentValue::MeshRenderer(renderer.clone())),
    );
    let (_, r) = h.step();
    assert_eq!(r.commands_rejected, 0);

    // The query frames every component, id order: Transform, Name, then ours.
    let (s, n, body, tail) = h.query(bridge::QUERY_COMPONENTS, lamp);
    assert_eq!((s, n), (Status::Ok, 4));
    let mut off = 0;
    let mut seen = Vec::new();
    for _ in 0..n {
        let hdr = ComponentHeader::decode(&body[off..], &tail).unwrap();
        off += ComponentHeader::SIZE;
        let value = fynkel_wire::decode_component_value(
            hdr.component,
            &body[off..off + hdr.size as usize],
            &tail,
        );
        seen.push((hdr.component, value.map(|v| v.unwrap())));
        off += hdr.size as usize;
    }
    assert_eq!(seen[0].0, 1);
    assert_eq!(seen[1].0, 3, "Name is framed too (no ComponentValue)");
    assert!(seen[1].1.is_none());
    assert_eq!(seen[2].1, Some(ComponentValue::MeshRenderer(renderer)));
    assert_eq!(seen[3].1, Some(ComponentValue::PointLight(light)));

    // Replace on the presentation lane (plain data applies immediately),
    // remove on the simulation lane, and the rejections.
    let dim = PointLight {
        intensity: 1.0,
        ..light
    };
    let (s, r) = h.submit(
        Lane::Presentation,
        &set_component(lamp, &ComponentValue::PointLight(dim)),
    );
    assert_eq!((s, r.rejected), (Status::Ok, 0));
    let (_, n, body, tail) = h.query(bridge::QUERY_COMPONENTS, lamp);
    assert_eq!(n, 4);
    let last = ComponentHeader::decode(
        &body[body.len() - ComponentHeader::SIZE - PointLight::SIZE..],
        &tail,
    )
    .unwrap();
    assert_eq!(last.component, 7);
    assert_eq!(
        PointLight::decode(&body[body.len() - PointLight::SIZE..], &tail).unwrap(),
        dim
    );
    h.submit(
        Lane::Simulation,
        &batch![
            (
                7,
                RemoveComponentCommand {
                    entity: lamp,
                    component: 4
                }
            ),
            (
                7,
                RemoveComponentCommand {
                    entity: lamp,
                    component: 1
                }
            ),
            (
                7,
                RemoveComponentCommand {
                    entity: lamp,
                    component: 4
                }
            ),
        ],
    );
    let (_, r) = h.step();
    assert_eq!(
        r.commands_rejected, 2,
        "Transform cannot be removed; the renderer is already gone"
    );
    let d = h.diagnostics();
    assert_eq!(d[0].code, DiagnosticCode::INVALID_PAYLOAD.0);
    assert_eq!(d[0].component, 1);
    assert_eq!(d[1].code, DiagnosticCode::MISSING_COMPONENT.0);
    let (_, n, _, _) = h.query(bridge::QUERY_COMPONENTS, lamp);
    assert_eq!(n, 3);

    // A value that fails validation, a component id the wire cannot carry
    // as a value, a truncated body: rejected at the batch index, the rest
    // still applies.
    let bad = PointLight {
        range: -1.0,
        ..light
    };
    let (s, r) = h.submit(
        Lane::Simulation,
        &set_component(lamp, &ComponentValue::PointLight(bad)),
    );
    assert_eq!((s, r.rejected), (Status::Ok, 0), "validated at the tick");
    let mut name_as_value = Writer::begin(Vec::new(), BufferKind::Commands);
    name_as_value.command(
        6,
        &SetComponentCommand {
            entity: lamp,
            component: 3,
        },
    );
    let (s, r) = h.submit(Lane::Simulation, &name_as_value.finish());
    assert_eq!(
        (s, r.rejected),
        (Status::Ok, 0),
        "a bad body is rejected at the tick, with the batch's others"
    );
    let mut short = Writer::begin(Vec::new(), BufferKind::Commands);
    short.command(
        6,
        &SetComponentCommand {
            entity: lamp,
            component: 7,
        },
    );
    let (s, r) = h.submit(Lane::Simulation, &short.finish());
    assert_eq!((s, r.rejected), (Status::Ok, 0));
    let (_, r) = h.step();
    assert_eq!(r.commands_rejected, 3);
    let d = h.diagnostics();
    assert!(
        d.iter()
            .all(|d| d.code == DiagnosticCode::INVALID_PAYLOAD.0),
        "{d:?}"
    );
    assert!(d[0].message.contains("range"), "{}", d[0].message);
    assert_eq!(d[1].component, 0, "undecodable: no component known");
    let (_, n, _, _) = h.query(bridge::QUERY_COMPONENTS, Entity::NULL);
    assert_eq!(n, 0);
}

#[test]
fn protocol_errors_apply_nothing() {
    let h = Host::new();
    let good = batch![(1, spawn(1, 0.0, None, ""))];
    let mut bad_magic = good.clone();
    bad_magic[0] ^= 0xff;
    assert_eq!(h.submit(Lane::Simulation, &bad_magic).0, Status::Protocol);
    let mut bad_version = good.clone();
    bad_version[4] = 9;
    assert_eq!(h.submit(Lane::Simulation, &bad_version).0, Status::Protocol);
    assert_eq!(
        h.submit(Lane::Simulation, &good[..good.len() - 1]).0,
        Status::Protocol
    );
    let mut wrong_kind = good.clone();
    wrong_kind[6] = 2;
    assert_eq!(h.submit(Lane::Simulation, &wrong_kind).0, Status::Protocol);
    let mut lying_size = good.clone();
    fynkel_wire::put_u32(&mut lying_size, buffer::HEADER_SIZE + 4, 1000);
    assert_eq!(h.submit(Lane::Simulation, &lying_size).0, Status::Protocol);
    assert_eq!(h.submit(Lane::Simulation, &[]).0, Status::Protocol);
    assert_eq!(
        h.submit(Lane::Simulation, &buffer::empty(BufferKind::Commands))
            .0,
        Status::Ok
    );
    let (_, r) = h.step();
    assert_eq!(r.live_entities, 0);
    assert_eq!(
        unsafe {
            fynkel_world_submit_commands(h.w, 7, good.as_ptr(), good.len() as u32, ptr::null_mut())
        },
        Status::InvalidArgument
    );
}

#[test]
fn null_safety_and_panic_boundary() {
    unsafe {
        assert!(fynkel_world_create(ptr::null_mut(), ptr::null()).is_null());
        assert_eq!(
            fynkel_world_step(ptr::null_mut(), ptr::null_mut()),
            Status::InvalidArgument
        );
        assert_eq!(
            fynkel_world_begin_frame(ptr::null_mut(), 0.0, ptr::null_mut()),
            Status::InvalidArgument
        );
        assert_eq!(fynkel_world_snapshot(ptr::null()).length, 0);
        assert_eq!(fynkel_world_drain_events(ptr::null_mut()).length, 0);
        assert_eq!(fynkel_world_is_poisoned(ptr::null()), 1);
        assert_eq!(
            fynkel_world_stats(ptr::null(), ptr::null_mut()),
            Status::InvalidArgument
        );
        fynkel_world_destroy(ptr::null_mut());
        fynkel_runtime_destroy(ptr::null_mut());
    }
    let h = Host::new();
    unsafe { (*h.w).core_mut().poison() };
    assert_eq!(unsafe { fynkel_world_is_poisoned(h.w) }, 1);
    assert_eq!(h.step().0, Status::Poisoned);
    assert_eq!(
        h.submit(Lane::Simulation, &buffer::empty(BufferKind::Commands))
            .0,
        Status::Poisoned
    );
}

#[test]
fn world_desc_and_entity_world_id() {
    let rt = fynkel_runtime_create();
    let desc = FynkelWorldDesc {
        world_id: 7,
        reserved: [0; 3],
        fixed_dt_seconds: 0.5,
        max_steps_per_frame: 1,
        max_frame_dt_seconds: 0.0,
        flags: 0,
    };
    let w = unsafe { fynkel_world_create(rt, &desc) };
    let h = Host { rt, w };
    h.submit(Lane::Simulation, &batch![(1, spawn(1, 0.0, None, ""))]);
    h.step();
    assert_eq!(h.spawned()[0].world(), WorldId(7));
    let mut plan = FynkelFramePlan::default();
    unsafe { fynkel_world_begin_frame(h.w, 2.0, &mut plan) };
    assert_eq!(plan.steps, 0, "2 s clamped to 0.25 < fixed_dt 0.5");
    unsafe { fynkel_world_begin_frame(h.w, 0.3, &mut plan) };
    assert_eq!(plan.steps, 1);
}

/// A `PhysicsQueries` buffer with one raycast down the Y axis from `origin`.
fn raycast_buffer(id: u32, origin: Vec3, exclude: Option<Entity>) -> Vec<u8> {
    let mut w = Writer::begin(Vec::new(), BufferKind::PhysicsQueries);
    w.record(&PhysicsQueryRecord {
        id,
        kind: PhysicsQueryKind::Raycast,
        shape: fynkel_core::ColliderShape::Box,
        size: Vec3::ZERO,
        origin,
        rotation: glam::Quat::IDENTITY,
        direction: Vec3::NEG_Y,
        max_distance: 100.0,
        mask: u32::MAX,
        exclude,
        max_hits: 8,
        hit_sensors: false,
    });
    w.finish()
}

#[test]
fn physics_bodies_commands_queries_and_contacts() {
    use fynkel_core::components::ComponentValue;
    use fynkel_core::{Collider, RigidBody};
    let h = Host::new();
    h.submit(
        Lane::Simulation,
        &batch![
            (1, spawn(1, 0.0, None, "floor")),
            (
                1,
                SpawnCommand {
                    temp_id: 2,
                    transform: Transform::from_position(Vec3::new(0.0, 3.0, 0.0)),
                    parent: None,
                    name: "crate".into(),
                }
            ),
        ],
    );
    h.step();
    let ids = h.spawned();
    let (floor, krate) = (ids[0], ids[1]);

    // Static floor: a collider alone. The crate: a dynamic body on a box.
    let floor_shape = Collider {
        size: Vec3::new(10.0, 0.5, 10.0),
        offset: Vec3::new(0.0, -0.5, 0.0),
        ..Collider::default()
    };
    let (s, r) = h.submit(
        Lane::Simulation,
        &set_component(floor, &ComponentValue::Collider(floor_shape.clone())),
    );
    assert_eq!((s, r.rejected), (Status::Ok, 0));
    h.submit(
        Lane::Simulation,
        &set_component(
            krate,
            &ComponentValue::RigidBody(RigidBody {
                mass: 2.0,
                ..RigidBody::default()
            }),
        ),
    );
    h.submit(
        Lane::Simulation,
        &set_component(krate, &ComponentValue::Collider(Collider::default())),
    );
    let (_, r) = h.step();
    assert_eq!(r.commands_rejected, 0);

    // The components come back through QUERY_COMPONENTS in id order.
    let (s, n, body, tail) = h.query(bridge::QUERY_COMPONENTS, krate);
    assert_eq!(
        (s, n),
        (Status::Ok, 4),
        "Transform, Name, RigidBody, Collider"
    );
    let mut off = 0;
    let mut seen = Vec::new();
    for _ in 0..n {
        let hdr = ComponentHeader::decode(&body[off..], &tail).unwrap();
        off += ComponentHeader::SIZE;
        seen.push((
            hdr.component,
            fynkel_wire::decode_component_value(
                hdr.component,
                &body[off..off + hdr.size as usize],
                &tail,
            )
            .map(|v| v.unwrap()),
        ));
        off += hdr.size as usize;
    }
    assert_eq!(seen[2].0, 10);
    assert!(matches!(seen[2].1, Some(ComponentValue::RigidBody(ref b)) if b.mass == 2.0));
    assert_eq!(
        seen[3].1,
        Some(ComponentValue::Collider(Collider::default()))
    );

    // QUERY_BODY: falling after one tick, no body on the floor.
    let (s, n, body, tail) = h.query(bridge::QUERY_BODY, krate);
    assert_eq!((s, n), (Status::Ok, 1));
    let state = BodyStateRecord::decode(&body, &tail).unwrap();
    assert_eq!(state.entity, krate);
    assert!(state.linear_velocity.y < 0.0, "{state:?}");
    assert!(!state.sleeping);
    let (s, n, _, _) = h.query(bridge::QUERY_BODY, floor);
    assert_eq!((s, n), (Status::Ok, 0));

    // A body command through the wire (kind 8): a sideways impulse.
    let (s, r) = h.submit(
        Lane::Simulation,
        &batch![(
            8,
            BodyCommandRecord {
                entity: krate,
                op: BodyOp::ApplyImpulse,
                vector: Vec3::new(4.0, 0.0, 0.0),
                rotation: glam::Quat::IDENTITY,
            }
        )],
    );
    assert_eq!((s, r.rejected), (Status::Ok, 0));
    let (_, r) = h.step();
    assert_eq!(r.commands_rejected, 0);
    let (_, _, body, tail) = h.query(bridge::QUERY_BODY, krate);
    let state = BodyStateRecord::decode(&body, &tail).unwrap();
    assert!((state.linear_velocity.x - 2.0).abs() < 1e-3, "{state:?}");

    // Rejected at the tick: a body command on the floor (no body), on the
    // presentation lane (wrong lane, rejected at submit).
    let (_, r) = h.submit(
        Lane::Simulation,
        &batch![(
            8,
            BodyCommandRecord {
                entity: floor,
                op: BodyOp::ApplyForce,
                vector: Vec3::Y,
                rotation: glam::Quat::IDENTITY,
            }
        )],
    );
    assert_eq!(r.rejected, 0);
    let (_, r) = h.step();
    assert_eq!(r.commands_rejected, 1);
    let d = h.diagnostics();
    assert_eq!(d[0].code, DiagnosticCode::MISSING_COMPONENT.0);
    assert_eq!(d[0].component, 10);
    let (s, r) = h.submit(
        Lane::Presentation,
        &batch![(
            8,
            BodyCommandRecord {
                entity: krate,
                op: BodyOp::ApplyForce,
                vector: Vec3::Y,
                rotation: glam::Quat::IDENTITY,
            }
        )],
    );
    assert_eq!((s, r.rejected), (Status::CommandsRejected, 1));
    assert_eq!(
        h.diagnostics().last().unwrap().code,
        DiagnosticCode::WRONG_LANE.0
    );

    // Let it land: a CollisionEnter with a contact (point, normal, impulse).
    let mut entered = Vec::new();
    for _ in 0..240 {
        h.step();
        entered.extend(
            h.events()
                .into_iter()
                .filter(|(k, _)| *k == EventKind::COLLISION_ENTER.0)
                .map(|(_, b)| ContactEvent::decode(&b, &[]).unwrap()),
        );
    }
    assert_eq!(entered.len(), 1, "{entered:?}");
    let c = &entered[0];
    assert_eq!(
        [c.a, c.b].iter().filter(|e| **e == krate).count(),
        1,
        "the crate is one side"
    );
    assert!(c.normal.y.abs() > 0.9, "{c:?}");
    assert!(c.impulse > 0.0);
    assert!((c.point.y).abs() < 0.2, "{c:?}");
    let (_, _, body, tail) = h.query(bridge::QUERY_BODY, krate);
    let state = BodyStateRecord::decode(&body, &tail).unwrap();
    assert!(state.sleeping, "{state:?}");

    // A raycast from above hits the crate first, then the floor;
    // excluding the crate leaves the floor.
    let (_, _, body, tail) = h.query(bridge::QUERY_TRANSFORM, krate);
    let crate_pos = SnapshotTransformRecord::decode(&body, &tail)
        .unwrap()
        .world
        .position;
    let origin = Vec3::new(crate_pos.x, 10.0, crate_pos.z);
    fn empty_view() -> FynkelBufferView {
        FynkelBufferView {
            data: ptr::null(),
            length: 0,
        }
    }
    let mut v = empty_view();
    let buf = raycast_buffer(3, origin, None);
    let s = unsafe { fynkel_world_physics_query(h.w, buf.as_ptr(), buf.len() as u32, &mut v) };
    assert_eq!(s, Status::Ok);
    let (hdr, records, tail) = buffer::validate(Host::view(v), BufferKind::Query).unwrap();
    assert_eq!(hdr.record_count, 2);
    let first = PhysicsHitRecord::decode(records, tail).unwrap();
    let second = PhysicsHitRecord::decode(&records[PhysicsHitRecord::SIZE..], tail).unwrap();
    assert_eq!((first.query, first.entity), (3, krate));
    assert_eq!(second.entity, floor);
    assert!(first.distance < second.distance);
    assert!(
        (first.point.y - (crate_pos.y + 0.5)).abs() < 0.05,
        "{first:?}"
    );
    assert!((second.point.y).abs() < 1e-3, "{second:?}");
    let buf = raycast_buffer(4, origin, Some(krate));
    let mut v = empty_view();
    let s = unsafe { fynkel_world_physics_query(h.w, buf.as_ptr(), buf.len() as u32, &mut v) };
    assert_eq!(s, Status::Ok);
    let (hdr, records, tail) = buffer::validate(Host::view(v), BufferKind::Query).unwrap();
    assert_eq!(hdr.record_count, 1);
    assert_eq!(
        PhysicsHitRecord::decode(records, tail).unwrap().entity,
        floor
    );

    // A malformed buffer is a protocol error with an empty view; the
    // world stays usable.
    let bad = buffer::empty(BufferKind::Commands);
    let mut v = empty_view();
    let s = unsafe { fynkel_world_physics_query(h.w, bad.as_ptr(), bad.len() as u32, &mut v) };
    assert_eq!(s, Status::Protocol);
    assert_eq!(v.length, 0);
    let (s, _) = h.step();
    assert_eq!(s, Status::Ok);
}

/// M3-09 through the ABI: `Move`/`Jump` body ops (a `Jump` carries its
/// speed in `vector.y`), `FYNKEL_QUERY_CHARACTER`, the two new component
/// ids in `QUERY_COMPONENTS`, and the rejections of a character command
/// on a plain entity and of a character next to a body.
#[test]
fn character_commands_and_query() {
    use fynkel_core::components::ComponentValue;
    use fynkel_core::{CharacterController, Collider, RigidBody, ThirdPersonCamera};
    let h = Host::new();
    h.submit(
        Lane::Simulation,
        &batch![
            (1, spawn(1, 0.0, None, "Floor")),
            (1, spawn(2, 0.0, None, "Player")),
            (1, spawn(3, 0.0, None, "Camera")),
        ],
    );
    h.step();
    let ids = h.spawned();
    let (floor, player, camera) = (ids[0], ids[1], ids[2]);
    h.submit(
        Lane::Simulation,
        &set_component(
            floor,
            &ComponentValue::Collider(Collider {
                size: Vec3::new(10.0, 0.5, 10.0),
                offset: Vec3::new(0.0, -0.5, 0.0),
                ..Collider::default()
            }),
        ),
    );
    h.submit(
        Lane::Simulation,
        &set_component(
            player,
            &ComponentValue::CharacterController(CharacterController::default()),
        ),
    );
    h.submit(
        Lane::Simulation,
        &set_component(
            camera,
            &ComponentValue::ThirdPersonCamera(ThirdPersonCamera {
                target: "Player".into(),
                ..ThirdPersonCamera::default()
            }),
        ),
    );
    let (_, r) = h.step();
    assert_eq!(r.commands_rejected, 0);
    let (s, n, body, tail) = h.query(bridge::QUERY_COMPONENTS, player);
    assert_eq!(
        (s, n),
        (Status::Ok, 3),
        "Transform, Name, CharacterController"
    );
    let mut at = 0;
    let mut seen = Vec::new();
    for _ in 0..n {
        let hd = ComponentHeader::decode(&body[at..], &tail).unwrap();
        at += ComponentHeader::SIZE;
        seen.push((hd.component, hd.size));
        at += hd.size as usize;
    }
    assert_eq!(seen[2], (12, CharacterController::SIZE as u32));
    let (_, n, body, tail) = h.query(bridge::QUERY_COMPONENTS, camera);
    let mut at = 0;
    for _ in 0..n - 1 {
        let hd = ComponentHeader::decode(&body[at..], &tail).unwrap();
        at += ComponentHeader::SIZE + hd.size as usize;
    }
    let hd = ComponentHeader::decode(&body[at..], &tail).unwrap();
    assert_eq!(
        (hd.component, hd.size),
        (13, ThirdPersonCamera::SIZE as u32)
    );
    let rig = ThirdPersonCamera::decode(&body[at + ComponentHeader::SIZE..], &tail).unwrap();
    assert_eq!(rig.target, "Player");

    // Walk one tick at 3 m/s along +X, then jump at 6 m/s.
    let walk = |x: f32| {
        batch![(
            8,
            BodyCommandRecord {
                entity: player,
                op: BodyOp::Move,
                vector: Vec3::new(x, 0.0, 0.0),
                rotation: glam::Quat::IDENTITY,
            }
        )]
    };
    h.submit(Lane::Simulation, &walk(3.0));
    h.step();
    h.submit(Lane::Simulation, &walk(3.0));
    let (_, r) = h.step();
    assert_eq!(r.commands_rejected, 0);
    let (s, n, body, tail) = h.query(bridge::QUERY_CHARACTER, player);
    assert_eq!((s, n), (Status::Ok, 1));
    let state = CharacterStateRecord::decode(&body, &tail).unwrap();
    assert_eq!(state.entity, player);
    assert!(state.grounded && !state.sliding, "{state:?}");
    assert!((state.velocity.x - 3.0).abs() < 1e-3, "{state:?}");
    let (s, n, _, _) = h.query(bridge::QUERY_CHARACTER, floor);
    assert_eq!((s, n), (Status::Ok, 0));
    h.submit(
        Lane::Simulation,
        &batch![(
            8,
            BodyCommandRecord {
                entity: player,
                op: BodyOp::Jump,
                vector: Vec3::new(0.0, 6.0, 0.0),
                rotation: glam::Quat::IDENTITY,
            }
        )],
    );
    let (_, r) = h.step();
    assert_eq!(r.commands_rejected, 0);
    let (_, _, body, tail) = h.query(bridge::QUERY_CHARACTER, player);
    let state = CharacterStateRecord::decode(&body, &tail).unwrap();
    assert!(!state.grounded && state.velocity.y > 5.0, "{state:?}");

    // A `Move` on the floor: MISSING_COMPONENT 12 at the tick. A
    // RigidBody next to the controller: INVALID_PAYLOAD 12, once.
    h.submit(
        Lane::Simulation,
        &batch![(
            8,
            BodyCommandRecord {
                entity: floor,
                op: BodyOp::Move,
                vector: Vec3::X,
                rotation: glam::Quat::IDENTITY,
            }
        )],
    );
    let (_, r) = h.step();
    assert_eq!(r.commands_rejected, 1);
    let d = h.diagnostics();
    assert_eq!(
        (d[0].code, d[0].component),
        (DiagnosticCode::MISSING_COMPONENT.0, 12)
    );
    h.submit(
        Lane::Simulation,
        &set_component(player, &ComponentValue::RigidBody(RigidBody::default())),
    );
    let (_, r) = h.step();
    assert_eq!(r.commands_rejected, 0);
    let d = h.diagnostics();
    assert_eq!(d.len(), 1);
    assert_eq!(
        (d[0].code, d[0].component),
        (DiagnosticCode::INVALID_PAYLOAD.0, 12)
    );
    assert_eq!(d[0].command_index, u32::MAX);
    h.step();
    assert!(h.diagnostics().is_empty());
}

/// M3-10: the six catalogue components travel through `SetComponent` and
/// come back from `QUERY_COMPONENTS` under ids 14–19; the entities move
/// without a command; `FYNKEL_QUERY_SYSTEMS` answers one record per
/// `SystemKind` in tick order with the counts of the last step.
#[test]
fn catalogue_components_and_the_systems_query() {
    use fynkel_core::components::ComponentValue;
    use fynkel_core::schema::{SystemKind, SystemTimeRecord};
    use fynkel_core::{
        Emitter, Follow, LifetimeDespawn, LookAtCamera, MotionSpace, Oscillate, Rotator,
    };
    let h = Host::new();
    h.submit(
        Lane::Simulation,
        &batch![
            (1, spawn(1, 0.0, None, "Hero")),
            (1, spawn(2, 5.0, None, "Spinner")),
            (1, spawn(3, 0.0, None, "Lamp")),
        ],
    );
    h.step();
    let ids = h.spawned();
    let (hero, spinner, lamp) = (ids[0], ids[1], ids[2]);
    let values: [(Entity, ComponentValue, u32, usize); 6] = [
        (
            spinner,
            ComponentValue::Rotator(Rotator {
                axis: Vec3::Y,
                speed: 1.0,
                space: MotionSpace::Local,
            }),
            14,
            Rotator::SIZE,
        ),
        (
            spinner,
            ComponentValue::Oscillate(Oscillate {
                amplitude: 0.25,
                ..Oscillate::default()
            }),
            15,
            Oscillate::SIZE,
        ),
        (
            lamp,
            ComponentValue::Follow(Follow {
                target: "Hero".into(),
                offset: Vec3::new(0.0, 2.0, 0.0),
                smoothing: 0.0,
                look_at: false,
            }),
            16,
            Follow::SIZE,
        ),
        (
            spinner,
            ComponentValue::LookAtCamera(LookAtCamera { lock_y: true }),
            17,
            LookAtCamera::SIZE,
        ),
        (
            hero,
            ComponentValue::LifetimeDespawn(LifetimeDespawn { seconds: 30.0 }),
            18,
            LifetimeDespawn::SIZE,
        ),
        (
            hero,
            ComponentValue::Emitter(Emitter {
                enabled: false,
                ..Emitter::default()
            }),
            19,
            Emitter::SIZE,
        ),
    ];
    for (entity, value, _, _) in &values {
        h.submit(Lane::Simulation, &set_component(*entity, value));
    }
    let (_, r) = h.step();
    assert_eq!(r.commands_rejected, 0, "{:?}", h.diagnostics());
    let components_of = |e: Entity| {
        let (s, n, body, tail) = h.query(bridge::QUERY_COMPONENTS, e);
        assert_eq!(s, Status::Ok);
        let mut at = 0;
        let mut seen = Vec::new();
        for _ in 0..n {
            let hd = ComponentHeader::decode(&body[at..], &tail).unwrap();
            at += ComponentHeader::SIZE;
            seen.push((
                hd.component,
                hd.size,
                body[at..at + hd.size as usize].to_vec(),
            ));
            at += hd.size as usize;
        }
        (seen, tail)
    };
    for (entity, _, id, size) in &values {
        let (seen, _) = components_of(*entity);
        assert!(
            seen.iter().any(|(c, s, _)| (*c, *s) == (*id, *size as u32)),
            "{id} on {entity:?}: {:?}",
            seen.iter().map(|(c, s, _)| (*c, *s)).collect::<Vec<_>>()
        );
    }
    let (seen, tail) = components_of(lamp);
    let follow = seen.iter().find(|(c, _, _)| *c == 16).unwrap();
    assert_eq!(Follow::decode(&follow.2, &tail).unwrap().target, "Hero");

    // They moved at the tick without a command: the lamp is 2 m over the
    // hero, the spinner turned.
    let snapshot = |e: Entity| {
        let handles = [e.to_bits()];
        let s = unsafe { fynkel_world_subscribe(h.w, 1, handles.as_ptr(), 1, 1) };
        assert_eq!(s, Status::Ok);
        h.step();
        h.snapshot().into_iter().find(|r| r.entity == e).unwrap()
    };
    let lamp_pose = snapshot(lamp);
    assert!((lamp_pose.world.position - Vec3::new(0.0, 2.0, 0.0)).length() < 1e-4);
    let spinner_pose = snapshot(spinner);
    assert!(
        spinner_pose.local.rotation.y.abs() > 0.01,
        "{spinner_pose:?}"
    );

    let (s, n, body, tail) = h.query(bridge::QUERY_SYSTEMS, Entity::NULL);
    assert_eq!((s, n), (Status::Ok, 14));
    let times: Vec<SystemTimeRecord> = (0..n as usize)
        .map(|i| SystemTimeRecord::decode(&body[i * SystemTimeRecord::SIZE..], &tail).unwrap())
        .collect();
    assert_eq!(times[0].system, SystemKind::Step);
    assert_eq!(times[0].entities, 3);
    let by = |k: SystemKind| times.iter().find(|t| t.system == k).unwrap();
    assert_eq!(by(SystemKind::Rotator).entities, 1);
    assert_eq!(by(SystemKind::Oscillate).entities, 1);
    assert_eq!(by(SystemKind::Follow).entities, 1);
    assert_eq!(
        by(SystemKind::LookAtCamera).entities,
        0,
        "no camera: skipped"
    );
    assert_eq!(by(SystemKind::LifetimeDespawn).entities, 1);
    assert_eq!(by(SystemKind::Emitter).entities, 1);
    assert_eq!(by(SystemKind::Publish).entities, 2, "two subscribed");
    assert_eq!(times.last().unwrap().system, SystemKind::Publish);
}

fn anchors_buffer(anchors: &[(Entity, Vec3, u32)]) -> Vec<u8> {
    let mut w = Writer::begin(Vec::new(), BufferKind::ProjectionAnchors);
    for (entity, offset, mask) in anchors {
        w.record(&ProjectionAnchorRecord {
            entity: *entity,
            offset: *offset,
            mask: *mask,
        });
    }
    w.finish()
}

#[test]
fn projection_of_world_anchors() {
    use fynkel_core::components::{Camera, ComponentValue};
    use fynkel_core::{
        Collider, PROJECTED_DROPPED, PROJECTED_IN_FRONT, PROJECTED_MISSING, PROJECTED_NO_CAMERA,
        PROJECTED_OCCLUDED, PROJECTED_ON_SCREEN,
    };
    let h = Host::new();
    let project = |buf: &[u8], width: u32, height: u32| {
        let mut info = FynkelProjectionInfo::default();
        let mut v = FynkelBufferView {
            data: ptr::null(),
            length: 0,
        };
        let s = unsafe {
            fynkel_world_project(
                h.w,
                width,
                height,
                buf.as_ptr(),
                buf.len() as u32,
                &mut info,
                &mut v,
            )
        };
        let b = Host::view(v);
        let points: Vec<ProjectedPointRecord> = if b.is_empty() {
            Vec::new()
        } else {
            let (hd, records, tail) = buffer::validate(b, BufferKind::Query).unwrap();
            (0..hd.record_count as usize)
                .map(|i| {
                    ProjectedPointRecord::decode(&records[i * ProjectedPointRecord::SIZE..], tail)
                        .unwrap()
                })
                .collect()
        };
        (s, info, points)
    };

    h.submit(
        Lane::Simulation,
        &batch![
            (
                1,
                SpawnCommand {
                    temp_id: 1,
                    transform: Transform::from_position(Vec3::new(0.0, 0.0, 5.0)),
                    parent: None,
                    name: "camera".into(),
                }
            ),
            (1, spawn(2, 0.0, None, "cube")),
            (
                1,
                SpawnCommand {
                    temp_id: 3,
                    transform: Transform::from_position(Vec3::new(0.0, 0.0, 2.5)),
                    parent: None,
                    name: "wall".into(),
                }
            ),
        ],
    );
    h.step();
    let ids = h.spawned();
    let (camera, cube, wall) = (ids[0], ids[1], ids[2]);

    // No camera yet: flagged, nothing placed.
    let (s, info, points) = project(&anchors_buffer(&[(cube, Vec3::ZERO, 0)]), 800, 600);
    assert_eq!(s, Status::Ok);
    assert_eq!((info.camera, info.projected, info.dropped), (0, 1, 0));
    assert_eq!(points[0].flags, PROJECTED_NO_CAMERA);

    h.submit(
        Lane::Simulation,
        &set_component(
            camera,
            &ComponentValue::Camera(Camera {
                fov_y: 60f32.to_radians(),
                ..Camera::default()
            }),
        ),
    );
    h.submit(
        Lane::Simulation,
        &set_component(
            wall,
            &ComponentValue::Collider(Collider {
                size: Vec3::new(4.0, 4.0, 0.2),
                layer: 0b10,
                ..Collider::default()
            }),
        ),
    );
    let (_, r) = h.step();
    assert_eq!(r.commands_rejected, 0);

    let (s, info, points) = project(
        &anchors_buffer(&[
            (cube, Vec3::ZERO, 0),
            (cube, Vec3::new(0.0, 1.0, 0.0), 0),
            (cube, Vec3::ZERO, 0b10),
            (Entity::from_bits(0xdead_0000_0001), Vec3::ZERO, 0),
        ]),
        800,
        600,
    );
    assert_eq!(s, Status::Ok);
    assert_eq!(info.camera, camera.to_bits());
    assert_eq!((info.projected, info.dropped), (4, 0));
    assert_eq!(points.len(), 4);
    let c = &points[0];
    assert_eq!(c.entity, cube);
    assert_eq!(c.flags, PROJECTED_IN_FRONT | PROJECTED_ON_SCREEN);
    assert!(
        (c.x - 400.0).abs() < 1e-3 && (c.y - 300.0).abs() < 1e-3,
        "{c:?}"
    );
    assert!((c.depth - 5.0).abs() < 1e-5);
    assert!(points[1].y < 300.0 - 100.0, "one metre up: {:?}", points[1]);
    assert_eq!(
        points[2].flags,
        PROJECTED_IN_FRONT | PROJECTED_ON_SCREEN | PROJECTED_OCCLUDED,
        "the wall on layer 2 is between the camera and the cube"
    );
    assert_eq!(points[3].flags, PROJECTED_MISSING);

    // The bound: 64 projected, the rest dropped, the diagnostic is in the
    // world's list (cleared by the next step like the others).
    let many: Vec<(Entity, Vec3, u32)> = (0..70).map(|_| (cube, Vec3::ZERO, 0)).collect();
    let (s, info, points) = project(&anchors_buffer(&many), 800, 600);
    assert_eq!(s, Status::Ok);
    assert_eq!((info.projected, info.dropped), (64, 6));
    assert_eq!(points.len(), 70);
    assert_eq!(points[63].flags, PROJECTED_IN_FRONT | PROJECTED_ON_SCREEN);
    assert_eq!(points[64].flags, PROJECTED_DROPPED);
    let d = h.diagnostics();
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].code, DiagnosticCode::TOO_MANY_ANCHORS.0);
    assert_eq!(d[0].command_index, u32::MAX);

    // A malformed buffer is a protocol error with an empty view; the world
    // keeps working.
    let bad = anchors_buffer(&[(cube, Vec3::ZERO, 0)]);
    let (s, _, points) = project(&bad[..bad.len() - 4], 800, 600);
    assert_eq!(s, Status::Protocol);
    assert!(points.is_empty());
    let (s, _, points) = project(&raycast_buffer(1, Vec3::ZERO, None), 800, 600);
    assert_eq!(s, Status::Protocol, "wrong buffer kind");
    assert!(points.is_empty());
    let (s, info, points) = project(&anchors_buffer(&[]), 320, 200);
    assert_eq!((s, info.projected, points.len()), (Status::Ok, 0, 0));
    assert_eq!(unsafe { fynkel_world_is_poisoned(h.w) }, 0);
    let (s, _) = h.step();
    assert_eq!(s, Status::Ok);
}

/// Animation (M3-19): `fynkel.Animator`/`fynkel.Skin` through
/// SetComponent, `AnimateCommandRecord` (kind 9) for every op, the
/// `AnimationEnded` event, `QUERY_ANIMATOR` and the rejections.
#[test]
fn animation_commands_events_and_query() {
    use fynkel_contracts::{AssetId, AssetRef};
    use fynkel_core::animation::{Channel, Clip, Interpolation, Property};
    use fynkel_core::components::ComponentValue;
    use fynkel_core::{Animator, Skin};
    let h = Host::new();
    h.submit(
        Lane::Simulation,
        &batch![
            (1, spawn(1, 0.0, None, "Rig")),
            (1, spawn(2, 0.0, None, "Bare"))
        ],
    );
    h.step();
    let ids = h.spawned();
    let (rig, bare) = (ids[0], ids[1]);
    h.submit(
        Lane::Simulation,
        &batch![(1, spawn(3, 0.0, Some(rig), "arm"))],
    );
    h.step();
    let arm = h.spawned()[0];

    // The clip a loaded project would have registered from its cache.
    let clip = AssetRef::with_sub(AssetId::from_bytes([9; 16]), "anim/0");
    unsafe {
        (*h.w).core_mut().register_clip(
            clip.clone(),
            Clip {
                name: "rise".into(),
                duration: 0.5,
                channels: vec![Channel {
                    target: "arm".into(),
                    property: Property::Translation,
                    interpolation: Interpolation::Linear,
                    times: vec![0.0, 0.5],
                    values: vec![0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
                }],
            },
        );
    }

    // The animator as a component (kind 6), one-shot; it queries back.
    let animator = Animator {
        clip: clip.clone(),
        speed: 1.0,
        looping: false,
        playing: true,
    };
    h.submit(
        Lane::Simulation,
        &set_component(rig, &ComponentValue::Animator(animator.clone())),
    );
    let (_, r) = h.step();
    assert_eq!(r.commands_rejected, 0);
    let (s, n, body, tail) = h.query(bridge::QUERY_COMPONENTS, rig);
    assert_eq!((s, n), (Status::Ok, 3), "Transform, Name, Animator");
    let mut at = 0;
    for _ in 0..n - 1 {
        let hd = ComponentHeader::decode(&body[at..], &tail).unwrap();
        at += ComponentHeader::SIZE + hd.size as usize;
    }
    let hd = ComponentHeader::decode(&body[at..], &tail).unwrap();
    assert_eq!((hd.component, hd.size), (21, Animator::SIZE as u32));
    at += ComponentHeader::SIZE;
    assert_eq!(
        fynkel_wire::decode_component_value(21, &body[at..], &tail)
            .unwrap()
            .unwrap(),
        ComponentValue::Animator(animator)
    );

    // Runs to its end: the arm at y = 1, one ANIMATION_ENDED, the state
    // query says so; nothing for an entity without an animator.
    let mut ended = Vec::new();
    for _ in 0..40 {
        h.step();
        let (events, tail) = h.events_with_tail();
        ended.extend(
            events
                .into_iter()
                .filter(|(k, _)| *k == EventKind::ANIMATION_ENDED.0)
                .map(|(_, b)| AnimationEndedEvent::decode(&b, &tail).unwrap()),
        );
    }
    let ended_tail: Vec<_> = h
        .events()
        .into_iter()
        .filter(|(k, _)| *k == EventKind::ANIMATION_ENDED.0)
        .collect();
    assert!(ended_tail.is_empty());
    assert_eq!(ended.len(), 1, "{ended:?}");
    assert_eq!(
        (ended[0].entity, &ended[0].clip.sub),
        (rig, &"anim/0".to_string())
    );
    let (_, _, body, _) = h.query(bridge::QUERY_TRANSFORM, arm);
    let arm_pose = SnapshotTransformRecord::decode(&body, &[]).unwrap();
    assert!(
        (arm_pose.local.position.y - 1.0).abs() < 1e-5,
        "{arm_pose:?}"
    );
    let (s, n, body, tail) = h.query(bridge::QUERY_ANIMATOR, rig);
    assert_eq!((s, n), (Status::Ok, 1));
    let state = AnimatorStateRecord::decode(&body, &tail).unwrap();
    assert_eq!(state.entity, rig);
    assert!(!state.playing && !state.looping, "{state:?}");
    assert!((state.time - 0.5).abs() < 1e-5 && (state.duration - 0.5).abs() < 1e-5);
    assert_eq!(state.clip, clip);
    let (s, n, _, _) = h.query(bridge::QUERY_ANIMATOR, bare);
    assert_eq!((s, n), (Status::Ok, 0));

    // Every op through the wire (kind 9): play looping at half speed,
    // set_time, pause, set_speed, resume, crossfade to the same clip
    // (no fade), stop.
    let cmd = |op: AnimateOp, seconds: f32, speed: f32, looping: bool| AnimateCommandRecord {
        entity: rig,
        op,
        seconds,
        speed,
        looping,
        clip: clip.clone(),
    };
    let (s, r) = h.submit(
        Lane::Simulation,
        &batch![(9, cmd(AnimateOp::Play, 0.0, 0.5, true))],
    );
    assert_eq!((s, r.rejected), (Status::Ok, 0));
    let (_, r) = h.step();
    assert_eq!(r.commands_rejected, 0);
    let (_, _, body, tail) = h.query(bridge::QUERY_ANIMATOR, rig);
    let state = AnimatorStateRecord::decode(&body, &tail).unwrap();
    assert!(state.playing && state.looping && state.time == 0.0 && state.speed == 0.5);
    h.submit(
        Lane::Simulation,
        &batch![(9, cmd(AnimateOp::SetTime, 0.25, 0.0, false))],
    );
    h.step();
    let (_, _, body, tail) = h.query(bridge::QUERY_ANIMATOR, rig);
    let state = AnimatorStateRecord::decode(&body, &tail).unwrap();
    assert!((state.time - 0.25).abs() < 1e-5, "{state:?}");
    h.submit(
        Lane::Simulation,
        &batch![(9, cmd(AnimateOp::Pause, 0.0, 0.0, false))],
    );
    h.step();
    h.step();
    let (_, _, body, tail) = h.query(bridge::QUERY_ANIMATOR, rig);
    let state = AnimatorStateRecord::decode(&body, &tail).unwrap();
    assert!(
        !state.playing && (state.time - 0.25).abs() < 1e-5,
        "{state:?}"
    );
    h.submit(
        Lane::Simulation,
        &batch![
            (9, cmd(AnimateOp::SetSpeed, 0.0, 2.0, false)),
            (9, cmd(AnimateOp::Resume, 0.0, 0.0, false)),
        ],
    );
    let (_, r) = h.step();
    assert_eq!(r.commands_rejected, 0);
    let (_, _, body, tail) = h.query(bridge::QUERY_ANIMATOR, rig);
    let state = AnimatorStateRecord::decode(&body, &tail).unwrap();
    assert!(state.playing && state.speed == 2.0, "{state:?}");
    assert!((state.time - (0.25 + 2.0 / 60.0)).abs() < 1e-4, "{state:?}");
    h.submit(
        Lane::Simulation,
        &batch![(9, cmd(AnimateOp::Crossfade, 0.2, 1.0, true))],
    );
    h.step();
    let (_, _, body, tail) = h.query(bridge::QUERY_ANIMATOR, rig);
    let state = AnimatorStateRecord::decode(&body, &tail).unwrap();
    assert_eq!(state.fade_remaining, 0.0, "same clip: nothing to fade");
    assert!(state.time == 0.0 && state.speed == 1.0);
    h.submit(
        Lane::Simulation,
        &batch![(9, cmd(AnimateOp::Stop, 0.0, 0.0, false))],
    );
    h.step();
    let (_, _, body, tail) = h.query(bridge::QUERY_ANIMATOR, rig);
    let state = AnimatorStateRecord::decode(&body, &tail).unwrap();
    assert!(!state.playing && state.time == 0.0);

    // Rejections: no animator (at the tick), a NaN speed, the wrong lane.
    h.submit(
        Lane::Simulation,
        &batch![(
            9,
            AnimateCommandRecord {
                entity: bare,
                op: AnimateOp::Pause,
                seconds: 0.0,
                speed: 0.0,
                looping: false,
                clip: AssetRef::default(),
            }
        )],
    );
    let (_, r) = h.step();
    assert_eq!(r.commands_rejected, 1);
    let d = h.diagnostics();
    assert_eq!(
        (d[0].code, d[0].component),
        (DiagnosticCode::MISSING_COMPONENT.0, 21)
    );
    h.submit(
        Lane::Simulation,
        &batch![(9, cmd(AnimateOp::SetSpeed, 0.0, f32::NAN, false))],
    );
    let (_, r) = h.step();
    assert_eq!(r.commands_rejected, 1);
    assert_eq!(h.diagnostics()[0].code, DiagnosticCode::INVALID_PAYLOAD.0);
    let (s, r) = h.submit(
        Lane::Presentation,
        &batch![(9, cmd(AnimateOp::Resume, 0.0, 0.0, false))],
    );
    assert_eq!((s, r.rejected), (Status::CommandsRejected, 1));
    assert_eq!(
        h.diagnostics().last().unwrap().code,
        DiagnosticCode::WRONG_LANE.0
    );

    // A clip nothing registered: CLIP_UNAVAILABLE once, at the tick.
    h.submit(
        Lane::Simulation,
        &batch![(
            9,
            AnimateCommandRecord {
                entity: rig,
                op: AnimateOp::Play,
                seconds: 0.0,
                speed: 1.0,
                looping: true,
                clip: AssetRef::with_sub(AssetId::from_bytes([9; 16]), "anim/7"),
            }
        )],
    );
    h.step();
    let d = h.diagnostics();
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(
        (d[0].code, d[0].component),
        (DiagnosticCode::CLIP_UNAVAILABLE.0, 21)
    );
    h.step();
    assert!(h.diagnostics().is_empty());

    // A Skin component round-trips and an unknown skin is reported once.
    let skin = Skin {
        skin: AssetRef::with_sub(AssetId::from_bytes([9; 16]), "skin/0"),
        skeleton: "hips".into(),
    };
    h.submit(
        Lane::Simulation,
        &set_component(arm, &ComponentValue::Skin(skin.clone())),
    );
    h.step();
    let d = h.diagnostics();
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(
        (d[0].code, d[0].component),
        (DiagnosticCode::SKIN_UNAVAILABLE.0, 22)
    );
    let (_, n, body, tail) = h.query(bridge::QUERY_COMPONENTS, arm);
    let mut at = 0;
    for _ in 0..n - 1 {
        let hd = ComponentHeader::decode(&body[at..], &tail).unwrap();
        at += ComponentHeader::SIZE + hd.size as usize;
    }
    let hd = ComponentHeader::decode(&body[at..], &tail).unwrap();
    assert_eq!((hd.component, hd.size), (22, Skin::SIZE as u32));
    at += ComponentHeader::SIZE;
    assert_eq!(
        fynkel_wire::decode_component_value(22, &body[at..], &tail)
            .unwrap()
            .unwrap(),
        ComponentValue::Skin(skin)
    );
}
