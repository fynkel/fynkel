//! `fynkel_native` — the C ABI (`include/fynkel.h`) and the panic boundary.
//!
//! ABI v0 is the M0 prototype of the real contract (blueprint §5.3,
//! ADR-0004, ADR-0014). Every exported function:
//! * accepts NULL and returns an error status instead of crashing;
//! * runs inside `catch_unwind`; a panic poisons the world and returns
//!   `FYNKEL_STATUS_PANIC`;
//! * never hands out pointers into world storage except the three read-only
//!   output buffers, which are valid until the next mutating call.

pub mod wire;
pub mod world;

use std::panic::{catch_unwind, AssertUnwindSafe};

use world::{Lane, World};

// Status codes, mirroring `FynkelStatus`.
pub const STATUS_OK: i32 = 0;
pub const STATUS_COMMANDS_REJECTED: i32 = 1;
pub const STATUS_INVALID_ARGUMENT: i32 = 2;
pub const STATUS_PROTOCOL: i32 = 3;
pub const STATUS_POISONED: i32 = 4;
pub const STATUS_PANIC: i32 = 5;

pub struct FynkelRuntime {
    worlds_created: u64,
}

pub struct FynkelWorld {
    inner: World,
}

#[repr(C)]
pub struct FynkelBufferView {
    pub data: *const u8,
    pub length: u32,
}

impl FynkelBufferView {
    fn empty() -> Self {
        Self {
            data: std::ptr::null(),
            length: 0,
        }
    }
    fn of(b: &[u8]) -> Self {
        Self {
            data: b.as_ptr(),
            length: b.len() as u32,
        }
    }
}

#[repr(C)]
pub struct FynkelWorldDesc {
    pub initial_entity_capacity: u32,
    pub flags: u32,
}

#[repr(C)]
pub struct FynkelStepDesc {
    pub fixed_dt_seconds: f32,
    pub flags: u32,
}

#[repr(C)]
#[derive(Default)]
pub struct FynkelStepResult {
    pub tick: u64,
    pub commands_applied: u32,
    pub commands_rejected: u32,
    pub snapshot_records: u32,
    pub event_records: u32,
    pub diagnostic_records: u32,
    pub live_entities: u32,
}

/// Runs `f` on a live, non-poisoned world inside the panic boundary.
fn guarded(world: *mut FynkelWorld, f: impl FnOnce(&mut World) -> i32) -> i32 {
    let Some(w) = (unsafe { world.as_mut() }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    if w.inner.poisoned {
        return STATUS_POISONED;
    }
    match catch_unwind(AssertUnwindSafe(|| f(&mut w.inner))) {
        Ok(status) => status,
        Err(payload) => {
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "non-string panic".into());
            log::error!("fynkel_native: panic caught at ABI edge, world poisoned: {msg}");
            w.inner.poisoned = true;
            STATUS_PANIC
        }
    }
}

#[no_mangle]
pub extern "C" fn fynkel_abi_version() -> u32 {
    wire::ABI_VERSION as u32
}

#[no_mangle]
pub extern "C" fn fynkel_schema_fingerprint() -> u64 {
    wire::SCHEMA_FINGERPRINT
}

#[no_mangle]
pub extern "C" fn fynkel_runtime_create() -> *mut FynkelRuntime {
    Box::into_raw(Box::new(FynkelRuntime { worlds_created: 0 }))
}

/// # Safety
/// `runtime` must come from `fynkel_runtime_create` and not be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn fynkel_runtime_destroy(runtime: *mut FynkelRuntime) {
    if !runtime.is_null() {
        drop(Box::from_raw(runtime));
    }
}

/// # Safety
/// `runtime` must be live; `desc` may be NULL (defaults).
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_create(
    runtime: *mut FynkelRuntime,
    desc: *const FynkelWorldDesc,
) -> *mut FynkelWorld {
    let Some(rt) = runtime.as_mut() else {
        return std::ptr::null_mut();
    };
    let capacity = desc
        .as_ref()
        .map(|d| d.initial_entity_capacity)
        .unwrap_or(0);
    rt.worlds_created += 1;
    Box::into_raw(Box::new(FynkelWorld {
        inner: World::new(capacity),
    }))
}

/// # Safety
/// `world` must come from `fynkel_world_create` and not be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_destroy(world: *mut FynkelWorld) {
    if !world.is_null() {
        drop(Box::from_raw(world));
    }
}

/// # Safety
/// `buffer` must point to `length` readable bytes for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_submit_commands(
    world: *mut FynkelWorld,
    lane: u32,
    buffer: *const u8,
    length: u32,
) -> i32 {
    let lane = match lane {
        0 => Lane::Simulation,
        1 => Lane::Presentation,
        _ => return STATUS_INVALID_ARGUMENT,
    };
    if buffer.is_null() && length != 0 {
        return STATUS_INVALID_ARGUMENT;
    }
    let bytes: &[u8] = if length == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(buffer, length as usize)
    };
    guarded(world, |w| match w.submit(lane, bytes) {
        Ok(true) => STATUS_OK,
        Ok(false) => STATUS_COMMANDS_REJECTED,
        Err(e) => {
            log::warn!("fynkel_world_submit_commands: protocol error {e:?}");
            STATUS_PROTOCOL
        }
    })
}

/// # Safety
/// `desc` and `out_result` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_step(
    world: *mut FynkelWorld,
    desc: *const FynkelStepDesc,
    out_result: *mut FynkelStepResult,
) -> i32 {
    let dt = desc
        .as_ref()
        .map(|d| d.fixed_dt_seconds)
        .unwrap_or(1.0 / 60.0);
    let out = out_result.as_mut();
    guarded(world, |w| {
        let o = w.step(dt);
        if let Some(out) = out {
            *out = FynkelStepResult {
                tick: o.tick,
                commands_applied: o.applied,
                commands_rejected: o.rejected,
                snapshot_records: o.snapshot_records,
                event_records: o.event_records,
                diagnostic_records: o.diagnostic_records,
                live_entities: o.live_entities,
            };
        }
        if o.rejected > 0 {
            STATUS_COMMANDS_REJECTED
        } else {
            STATUS_OK
        }
    })
}

/// # Safety
/// `entities` must point to `count` u64s for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_subscribe(
    world: *mut FynkelWorld,
    component: u32,
    entities: *const u64,
    count: u32,
    enable: i32,
) -> i32 {
    if component != wire::COMPONENT_TRANSFORM {
        return STATUS_INVALID_ARGUMENT;
    }
    if entities.is_null() && count != 0 {
        return STATUS_INVALID_ARGUMENT;
    }
    let list: &[u64] = if count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(entities, count as usize)
    };
    guarded(world, |w| {
        w.subscribe(list, enable != 0);
        STATUS_OK
    })
}

/// # Safety
/// `world` must be live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_snapshot(world: *const FynkelWorld) -> FynkelBufferView {
    world
        .as_ref()
        .map(|w| FynkelBufferView::of(w.inner.snapshot()))
        .unwrap_or_else(FynkelBufferView::empty)
}

/// # Safety
/// `world` must be live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_events(world: *const FynkelWorld) -> FynkelBufferView {
    world
        .as_ref()
        .map(|w| FynkelBufferView::of(w.inner.events()))
        .unwrap_or_else(FynkelBufferView::empty)
}

/// # Safety
/// `world` must be live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_diagnostics(world: *const FynkelWorld) -> FynkelBufferView {
    world
        .as_ref()
        .map(|w| FynkelBufferView::of(w.inner.diagnostics()))
        .unwrap_or_else(FynkelBufferView::empty)
}

/// # Safety
/// `world` must be live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_is_poisoned(world: *const FynkelWorld) -> i32 {
    world.as_ref().map(|w| w.inner.poisoned as i32).unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wire::*;

    fn commands(cmds: &[(u32, u64, Option<Transform>)]) -> Vec<u8> {
        let mut w = BufferWriter::begin(Vec::new(), BufferKind::Commands);
        for (kind, target, t) in cmds {
            let r = w.record();
            r.extend_from_slice(&kind.to_le_bytes());
            r.extend_from_slice(&0u32.to_le_bytes());
            r.extend_from_slice(&target.to_le_bytes());
            let start = r.len();
            if let Some(t) = t {
                t.write(r);
            }
            r.resize(start + 48, 0);
        }
        w.finish()
    }

    fn events(w: &World) -> Vec<(u32, u64, u64)> {
        let b = w.events();
        let n = read_u32(b, 8) as usize;
        (0..n)
            .map(|i| {
                let o = HEADER_SIZE + i * EVENT_SIZE;
                (read_u32(b, o), read_u64(b, o + 8), read_u64(b, o + 16))
            })
            .collect()
    }

    #[test]
    fn round_trip_spawn_set_destroy_with_diagnostics() {
        let mut w = World::new(0);
        let t = Transform {
            position: [1.0, 2.0, 3.0],
            ..Transform::IDENTITY
        };
        // Spawn two (temp ids 7 and 8) in the simulation lane: nothing
        // happens until step().
        assert!(w
            .submit(
                Lane::Simulation,
                &commands(&[(1, 7, Some(t)), (1, 8, Some(t))])
            )
            .unwrap());
        assert_eq!(events(&w).len(), 0);
        let o = w.step(1.0 / 60.0);
        assert_eq!(
            (o.tick, o.applied, o.rejected, o.live_entities),
            (1, 2, 0, 2)
        );
        let ev = events(&w);
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].0, EVENT_SPAWNED);
        let (e7, e8) = (ev[0].2, ev[1].2);
        assert_eq!(ev[0].1, 7);
        assert_ne!(e7, 0, "handle 0 is never valid");
        assert_ne!(e7, e8);

        // Subscribe one; snapshot has exactly it.
        w.subscribe(&[e7], true);
        let o = w.step(1.0 / 60.0);
        assert_eq!(o.snapshot_records, 1);
        let s = w.snapshot();
        assert_eq!(read_u64(s, HEADER_SIZE), e7);
        assert_eq!(Transform::read(&s[HEADER_SIZE + 8..]), t);

        // Presentation-lane set applies immediately (visible in the store),
        // simulation-lane set applies at the next step.
        let t2 = Transform {
            position: [9.0, 9.0, 9.0],
            ..Transform::IDENTITY
        };
        assert!(w
            .submit(Lane::Presentation, &commands(&[(3, e7, Some(t2))]))
            .unwrap());
        assert_eq!(w.transform(e7).unwrap().position, [9.0; 3]);
        let t3 = Transform {
            position: [4.0, 4.0, 4.0],
            ..Transform::IDENTITY
        };
        assert!(w
            .submit(Lane::Simulation, &commands(&[(3, e7, Some(t3))]))
            .unwrap());
        assert_eq!(w.transform(e7).unwrap().position, [9.0; 3], "not yet");
        w.step(1.0 / 60.0);
        assert_eq!(w.transform(e7).unwrap().position, [4.0; 3]);

        // Destroy e8, then a stale set on e8 and a bad spawn are rejected
        // with diagnostics naming the command index.
        assert!(w
            .submit(Lane::Simulation, &commands(&[(2, e8, None)]))
            .unwrap());
        w.step(1.0 / 60.0);
        assert!(w.transform(e8).is_none());
        assert!(w
            .submit(
                Lane::Simulation,
                &commands(&[
                    (3, e7, Some(t)),
                    (3, e8, Some(t)),
                    (1, 0, Some(t)),
                    (99, 0, None)
                ])
            )
            .unwrap());
        let o = w.step(1.0 / 60.0);
        assert_eq!((o.applied, o.rejected), (1, 3));
        let d = w.diagnostics();
        assert_eq!(read_u32(d, 8), 3);
        let first = HEADER_SIZE;
        assert_eq!(read_u32(d, first), 1, "command index");
        assert_eq!(read_u32(d, first + 4), DIAG_STALE_ENTITY);
        assert_eq!(read_u64(d, first + 8), e8);
        let (off, len) = (
            read_u32(d, first + 20) as usize,
            read_u32(d, first + 24) as usize,
        );
        assert_eq!(
            &d[off..off + len],
            b"set_transform: stale or unknown entity"
        );
        assert_eq!(
            read_u32(d, first + DIAGNOSTIC_SIZE + 4),
            DIAG_INVALID_TEMP_ID
        );
        assert_eq!(
            read_u32(d, first + 2 * DIAGNOSTIC_SIZE + 4),
            DIAG_UNKNOWN_COMMAND
        );

        // Slot reuse gives a new generation: the old handle stays stale.
        w.submit(Lane::Simulation, &commands(&[(1, 9, Some(t))]))
            .unwrap();
        w.step(1.0 / 60.0);
        let e9 = events(&w)[0].2;
        assert_eq!(e9 & 0xffff_ffff, e8 & 0xffff_ffff, "slot reused");
        assert_ne!(e9, e8, "generation bumped");
        assert!(w.transform(e8).is_none());
    }

    #[test]
    fn protocol_errors_apply_nothing() {
        let mut w = World::new(0);
        let mut good = commands(&[(1, 1, Some(Transform::IDENTITY))]);
        assert_eq!(
            w.submit(Lane::Simulation, &good[..10]),
            Err(ProtocolError::TooShort)
        );
        good[0] ^= 0xff;
        assert_eq!(
            w.submit(Lane::Simulation, &good),
            Err(ProtocolError::BadMagic)
        );
        good[0] ^= 0xff;
        good.truncate(good.len() - 1);
        assert!(matches!(
            w.submit(Lane::Simulation, &good),
            Err(ProtocolError::LengthMismatch { .. })
        ));
        let o = w.step(1.0 / 60.0);
        assert_eq!(o.live_entities, 0);
    }

    #[test]
    fn abi_null_safety_and_panic_boundary() {
        unsafe {
            assert_eq!(
                fynkel_world_submit_commands(std::ptr::null_mut(), 0, std::ptr::null(), 0),
                STATUS_INVALID_ARGUMENT
            );
            assert_eq!(
                fynkel_world_step(std::ptr::null_mut(), std::ptr::null(), std::ptr::null_mut()),
                STATUS_INVALID_ARGUMENT
            );
            assert_eq!(fynkel_world_snapshot(std::ptr::null()).length, 0);
            assert_eq!(fynkel_world_is_poisoned(std::ptr::null()), 1);

            let rt = fynkel_runtime_create();
            let world = fynkel_world_create(rt, std::ptr::null());
            assert_eq!(fynkel_world_is_poisoned(world), 0);
            // Simulate an internal panic through the guard.
            let status = guarded(world, |_| panic!("boom"));
            assert_eq!(status, STATUS_PANIC);
            assert_eq!(fynkel_world_is_poisoned(world), 1);
            assert_eq!(
                fynkel_world_step(world, std::ptr::null(), std::ptr::null_mut()),
                STATUS_POISONED
            );
            fynkel_world_destroy(world);
            fynkel_runtime_destroy(rt);
        }
    }
}
