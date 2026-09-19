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
