//! `fynkel_native` — the C ABI (`include/fynkel.h`) over `fynkel_core`.
//!
//! ABI v1 (blueprint §5.3, ADR-0004, ADR-0014). Every exported function:
//! * accepts NULL and returns an error status instead of crashing;
//! * runs inside `catch_unwind`; a panic poisons the world and returns
//!   `FYNKEL_STATUS_PANIC`;
//! * never hands out pointers into world storage except the read-only
//!   output buffers, which are valid until the next mutating call.
//!
//! With the cargo feature `authoring` (on by default, off in a game's
//! runtime) the crate also exports the editor's authoring ABI
//! (`include/fynkel_authoring.h`, [`authoring`]).

#[cfg(feature = "authoring")]
pub mod authoring;
pub mod bridge;
pub mod buffer;
pub mod load;

use std::panic::{catch_unwind, AssertUnwindSafe};

use fynkel_contracts::{ComponentId, Entity, Status, WorldId, ABI_VERSION};
use fynkel_core::{Lane, World, WorldConfig};

use buffer::BufferKind;

/// The counting allocator (`fynkel_core::memory`, M3-16): this crate is a
/// final artifact (the dylib the Flutter host loads, on its own or inside
/// `fynkel_platform_apple`), so the Rust heap it reports in
/// `FynkelWorldStats::native_bytes` is the whole engine's.
#[global_allocator]
static ALLOCATOR: fynkel_core::memory::CountingAllocator = fynkel_core::memory::CountingAllocator;

pub struct FynkelRuntime {
    worlds_created: u64,
}

pub struct FynkelWorld {
    inner: World,
    snapshot_buf: Vec<u8>,
    events_buf: Vec<u8>,
    after_physics_buf: Vec<u8>,
    diagnostics_buf: Vec<u8>,
    query_buf: Vec<u8>,
    input_buf: Vec<u8>,
    /// JSON of the last `fynkel_world_load_*` call; empty before any.
    load_report: Vec<u8>,
    /// The cache the last load opened (a renderer drawing this world adopts
    /// it, so both see one residency store).
    assets: Option<fynkel_assets::Assets>,
    /// One token per loaded scene (keyed by its id) or instantiated prefab
    /// (`None`): the closure stays resident until the scene is unloaded, a
    /// replace load empties the world, or the world is destroyed (M3-12).
    residency: Vec<(Option<fynkel_contracts::Uuid>, fynkel_assets::Residency)>,
    /// Action → bindings the user chose over the project's
    /// (`fynkel_world_bind_actions`, M3-12), re-applied after every load.
    rebinds: std::collections::BTreeMap<String, Vec<String>>,
    /// Scratch of `fynkel_world_project`, kept for its capacity (M3-13).
    anchors: Vec<fynkel_core::schema::ProjectionAnchorRecord>,
    points: Vec<fynkel_core::schema::ProjectedPointRecord>,
    stats: Counters,
}

#[derive(Default)]
struct Counters {
    submit_calls: u64,
    step_calls: u64,
    query_calls: u64,
    bytes_in: u64,
    bytes_out: u64,
    /// The largest snapshot a step published (M3-16).
    snapshot_peak_bytes: u64,
    /// `fynkel_world_set_snapshot_budget`; 0 = no ceiling (C5).
    snapshot_budget: u32,
    /// The last step's snapshot was over the budget (one diagnostic per
    /// crossing, not per tick).
    over_budget: bool,
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
    pub world_id: u8,
    pub reserved: [u8; 3],
    pub fixed_dt_seconds: f32,
    pub max_steps_per_frame: u32,
    pub max_frame_dt_seconds: f32,
    pub flags: u32,
}

#[repr(C)]
#[derive(Default)]
pub struct FynkelFramePlan {
    pub steps: u32,
    pub dropped_seconds: f32,
    pub clamped: u32,
    pub alpha: f32,
}

#[repr(C)]
#[derive(Default)]
pub struct FynkelSubmitResult {
    pub accepted: u32,
    pub rejected: u32,
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

#[repr(C)]
pub struct FynkelInstantiateDesc {
    pub prefab_id_utf8: *const u8,
    pub prefab_id_len: u32,
    pub name_utf8: *const u8,
    pub name_len: u32,
    pub parent: u64,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

#[repr(C)]
pub struct FynkelCloneDesc {
    pub source: u64,
    pub name_utf8: *const u8,
    pub name_len: u32,
    pub parent: u64,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

#[repr(C)]
pub struct FynkelQueryDesc {
    pub kind: u32,
    pub request_id: u32,
    pub entity: u64,
}

#[repr(C)]
#[derive(Default)]
pub struct FynkelWorldStats {
    pub tick: u64,
    pub frame: u64,
    pub submit_calls: u64,
    pub step_calls: u64,
    pub query_calls: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub live_entities: u32,
    pub subscriptions: u32,
    pub staged_commands: u32,
    pub poisoned: u32,
    /// Memory and the subscription budget (M3-16, blueprint §15.4 / C5):
    /// the last snapshot's bytes, the largest so far, the host's ceiling
    /// (0 = none), the Rust heap in use (the whole engine's — the counting
    /// allocator is this crate's), the process's resident set (0 where not
    /// reported) and the resident assets' bytes.
    pub snapshot_bytes: u64,
    pub snapshot_peak_bytes: u64,
    pub snapshot_budget: u32,
    pub reserved: u32,
    pub native_bytes: u64,
    pub resident_bytes: u64,
    pub residency_bytes: u64,
}

/// Runs `f` on a live, non-poisoned world inside the panic boundary.
fn guarded(world: *mut FynkelWorld, f: impl FnOnce(&mut FynkelWorld) -> Status) -> Status {
    let Some(w) = (unsafe { world.as_mut() }) else {
        return Status::InvalidArgument;
    };
    if w.inner.is_poisoned() {
        return Status::Poisoned;
    }
    match catch_unwind(AssertUnwindSafe(|| f(w))) {
        Ok(status) => status,
        Err(payload) => {
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "non-string panic".into());
            log::error!("fynkel_native: panic caught at ABI edge, world poisoned: {msg}");
            w.inner.poison();
            Status::Panic
        }
    }
}

impl FynkelWorld {
    fn new(config: WorldConfig) -> Self {
        Self {
            inner: World::new(config),
            snapshot_buf: buffer::empty(BufferKind::Snapshot),
            events_buf: buffer::empty(BufferKind::Events),
            after_physics_buf: buffer::empty(BufferKind::Events),
            diagnostics_buf: buffer::empty(BufferKind::Diagnostics),
            query_buf: buffer::empty(BufferKind::Query),
            input_buf: buffer::empty(BufferKind::InputActions),
            load_report: Vec::new(),
            assets: None,
            residency: Vec::new(),
            rebinds: std::collections::BTreeMap::new(),
            anchors: Vec::new(),
            points: Vec::new(),
            stats: Counters::default(),
        }
    }

    pub fn core(&self) -> &World {
        &self.inner
    }
    pub fn core_mut(&mut self) -> &mut World {
        &mut self.inner
    }

    /// The residency store of the last successful load, if any.
    pub fn assets(&self) -> Option<&fynkel_assets::Assets> {
        self.assets.as_ref()
    }

    /// Registers from the cache the clip, skin or sound a command names
    /// before the batch is applied (M3-19, M3-11): a `Play`/`Crossfade` of
    /// a clip a loaded scene never named, an `Animator`/`Skin`/`AudioSource`
    /// set from a behaviour, a `PlaySound`. A clip the cache cannot read
    /// stays unregistered and the component reports `CLIP_UNAVAILABLE` /
    /// `SOUND_UNAVAILABLE` at the tick.
    fn register_animation_assets(&mut self, commands: &[bridge::IndexedCommand]) {
        use fynkel_core::components::ComponentValue;
        use fynkel_core::{AnimateOp, Command};
        let Some(assets) = self.assets.clone() else {
            return;
        };
        for (_, c) in commands {
            let Ok(c) = c else { continue };
            match c {
                Command::PlaySound { clip, .. } => {
                    let _ = assets.register_sound(clip, &mut self.inner);
                }
                Command::SetComponent {
                    value: ComponentValue::AudioSource(a),
                    ..
                } => {
                    let _ = assets.register_sound(&a.clip, &mut self.inner);
                }
                Command::Animate {
                    op: AnimateOp::Play { clip, .. } | AnimateOp::Crossfade { clip, .. },
                    ..
                } => {
                    let _ = assets.register_clip(clip, &mut self.inner);
                }
                Command::SetComponent {
                    value: ComponentValue::Animator(a),
                    ..
                } => {
                    let _ = assets.register_clip(&a.clip, &mut self.inner);
                }
                Command::SetComponent {
                    value: ComponentValue::Skin(k),
                    ..
                } => {
                    let _ = assets.register_skin(&k.skin, &mut self.inner);
                }
                // A mesh collider or physics material from code (ADR-0026
                // §4): what a load would have registered for it.
                Command::SetComponent {
                    value: ComponentValue::Collider(c),
                    ..
                } => {
                    let _ = assets.register_collider(c, self.inner.physics_mut());
                }
                _ => {}
            }
        }
    }

    /// The subscription budget diagnostic (C5): `SNAPSHOT_BUDGET` once
    /// when a tick's snapshot crosses the host's ceiling, again only after
    /// it came back under.
    fn check_snapshot_budget(&mut self, snapshot_bytes: u64) {
        let budget = self.stats.snapshot_budget as u64;
        let over = budget > 0 && snapshot_bytes > budget;
        if over && !self.stats.over_budget {
            let subscriptions = self.inner.subscription_count();
            self.inner.push_diagnostic(
                u32::MAX,
                Lane::Simulation,
                fynkel_contracts::DiagnosticCode::SNAPSHOT_BUDGET,
                Entity::NULL,
                ComponentId::NONE,
                format!(
                    "{}: snapshot of {snapshot_bytes} bytes over the {budget}-byte budget ({subscriptions} subscriptions)",
                    fynkel_contracts::Code::RUNTIME_SNAPSHOT_BUDGET.0
                ),
            );
        }
        self.stats.over_budget = over;
    }

    fn rebuild_diagnostics(&mut self) {
        let buf = std::mem::take(&mut self.diagnostics_buf);
        self.diagnostics_buf = bridge::encode_diagnostics(buf, self.inner.diagnostics());
        self.stats.bytes_out += self.diagnostics_buf.len() as u64;
    }
}

#[no_mangle]
pub extern "C" fn fynkel_abi_version() -> u32 {
    ABI_VERSION as u32
}

#[no_mangle]
pub extern "C" fn fynkel_schema_fingerprint() -> u64 {
    fynkel_wire::SCHEMA_FINGERPRINT
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
    let mut config = WorldConfig::default();
    if let Some(d) = desc.as_ref() {
        if d.world_id != 0 {
            config.id = WorldId(d.world_id);
        }
        if d.fixed_dt_seconds > 0.0 && d.fixed_dt_seconds.is_finite() {
            config.fixed_dt = d.fixed_dt_seconds;
        }
        if d.max_steps_per_frame != 0 {
            config.max_steps_per_frame = d.max_steps_per_frame;
        }
        if d.max_frame_dt_seconds > 0.0 && d.max_frame_dt_seconds.is_finite() {
            config.max_frame_dt = d.max_frame_dt_seconds;
        }
    }
    match catch_unwind(|| FynkelWorld::new(config)) {
        Ok(w) => {
            rt.worlds_created += 1;
            Box::into_raw(Box::new(w))
        }
        Err(_) => std::ptr::null_mut(),
    }
}

/// # Safety
/// `world` must come from `fynkel_world_create` and not be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_destroy(world: *mut FynkelWorld) {
    if !world.is_null() {
        // A panicking Drop must not unwind into the host either.
        let _ = catch_unwind(AssertUnwindSafe(|| drop(Box::from_raw(world))));
    }
}

/// # Safety
/// `world` live; `out_plan` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_begin_frame(
    world: *mut FynkelWorld,
    dt_seconds: f32,
    out_plan: *mut FynkelFramePlan,
) -> Status {
    guarded(world, |w| {
        let plan = w.inner.begin_frame(dt_seconds);
        if plan.dropped_seconds > 0.0 {
            w.rebuild_diagnostics();
        }
        if let Some(out) = out_plan.as_mut() {
            *out = FynkelFramePlan {
                steps: plan.steps,
                dropped_seconds: plan.dropped_seconds,
                clamped: plan.clamped as u32,
                alpha: w.inner.alpha(),
            };
        }
        Status::Ok
    })
}

/// # Safety
/// `buffer` must point to `length` readable bytes for the call; `out_result` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_submit_commands(
    world: *mut FynkelWorld,
    lane: u32,
    buffer: *const u8,
    length: u32,
    out_result: *mut FynkelSubmitResult,
) -> Status {
    let Some(lane) = Lane::from_u32(lane) else {
        return Status::InvalidArgument;
    };
    if buffer.is_null() && length != 0 {
        return Status::InvalidArgument;
    }
    let bytes: &[u8] = if length == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(buffer, length as usize)
    };
    guarded(world, |w| {
        w.stats.submit_calls += 1;
        w.stats.bytes_in += bytes.len() as u64;
        let commands = match bridge::decode_commands(bytes) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("fynkel_world_submit_commands: protocol error {e:?}");
                return Status::Protocol;
            }
        };
        w.register_animation_assets(&commands);
        let report = w.inner.submit_indexed(lane, commands);
        if lane == Lane::Presentation {
            w.rebuild_diagnostics();
        }
        if let Some(out) = out_result.as_mut() {
            *out = FynkelSubmitResult {
                accepted: report.accepted,
                rejected: report.rejected,
            };
        }
        report.status
    })
}

/// A material made at runtime (ADR-0026 §4): see the header.
///
/// # Safety
/// `world` live or NULL; `id` points at 16 readable bytes; `payload` at
/// `payload_len` readable bytes (NULL when 0).
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_set_material(
    world: *mut FynkelWorld,
    id: *const u8,
    payload: *const u8,
    payload_len: u32,
) -> Status {
    if id.is_null() || (payload.is_null() && payload_len != 0) {
        return Status::InvalidArgument;
    }
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(std::slice::from_raw_parts(id, 16));
    let key = fynkel_contracts::AssetRef::new(fynkel_contracts::Uuid(bytes));
    let material = if payload_len == 0 {
        None
    } else {
        let payload = std::slice::from_raw_parts(payload, payload_len as usize);
        match fynkel_wire::material::decode_material(payload) {
            Some(m) => Some(m),
            None => return Status::InvalidArgument,
        }
    };
    guarded(world, move |w| {
        let Some(assets) = &w.assets else {
            return Status::LoadFailed;
        };
        assets.set_runtime_material(&key, material);
        Status::Ok
    })
}

/// A mesh made at runtime (ADR-0026 §4): see the header.
///
/// # Safety
/// `world` live or NULL; `id` points at 16 readable bytes; `payload` at
/// `payload_len` readable bytes (NULL when 0).
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_set_mesh(
    world: *mut FynkelWorld,
    id: *const u8,
    payload: *const u8,
    payload_len: u32,
) -> Status {
    if id.is_null() || (payload.is_null() && payload_len != 0) {
        return Status::InvalidArgument;
    }
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(std::slice::from_raw_parts(id, 16));
    let key = fynkel_contracts::AssetRef::new(fynkel_contracts::Uuid(bytes));
    let mesh = if payload_len == 0 {
        None
    } else {
        let payload = std::slice::from_raw_parts(payload, payload_len as usize);
        match fynkel_assets::decode_mesh(payload) {
            Some(m) if !m.is_skinned() && !m.indices.is_empty() && m.indices.len() % 3 == 0 => {
                let finite = m.vertices.iter().all(|v| {
                    v.position
                        .iter()
                        .chain(&v.normal)
                        .chain(&v.tangent)
                        .chain(&v.uv0)
                        .chain(&v.uv1)
                        .chain(&v.color)
                        .all(|x| x.is_finite())
                });
                if !finite {
                    return Status::InvalidArgument;
                }
                Some(m)
            }
            _ => return Status::InvalidArgument,
        }
    };
    guarded(world, move |w| {
        let Some(assets) = w.assets.clone() else {
            return Status::LoadFailed;
        };
        // The mesh colliders built from it follow at the next step (one
        // over the triangle limit reports it there).
        let physics = w.core_mut().physics_mut();
        if physics.uses_mesh(&key) {
            let positions: Option<Vec<[f32; 3]>> = mesh
                .as_ref()
                .map(|m| m.vertices.iter().map(|v| v.position).collect());
            let triangles = mesh.as_ref().map(|m| m.indices.as_slice());
            let _ = physics.replace_mesh(&key, positions.as_deref().zip(triangles));
        }
        assets.set_runtime_mesh(&key, mesh);
        Status::Ok
    })
}

/// # Safety
/// `world` live; `out_result` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_step(
    world: *mut FynkelWorld,
    out_result: *mut FynkelStepResult,
) -> Status {
    guarded(world, |w| {
        w.stats.step_calls += 1;
        let r = w.inner.step();
        let buf = std::mem::take(&mut w.snapshot_buf);
        w.snapshot_buf = bridge::encode_snapshot(buf, w.inner.snapshot());
        let snapshot_bytes = w.snapshot_buf.len() as u64;
        w.stats.bytes_out += snapshot_bytes;
        w.stats.snapshot_peak_bytes = w.stats.snapshot_peak_bytes.max(snapshot_bytes);
        w.check_snapshot_budget(snapshot_bytes);
        let buf = std::mem::take(&mut w.after_physics_buf);
        w.after_physics_buf = bridge::encode_events(buf, w.inner.after_physics_events());
        w.rebuild_diagnostics();
        if let Some(out) = out_result.as_mut() {
            *out = FynkelStepResult {
                tick: r.tick,
                commands_applied: r.applied,
                commands_rejected: r.rejected,
                snapshot_records: r.snapshot_records,
                event_records: r.event_records,
                diagnostic_records: r.diagnostic_records,
                live_entities: r.live_entities,
            };
        }
        r.status
    })
}

/// Applies a host frame's raw control events (`FYNKEL_BUFFER_INPUT_EVENTS`;
/// §5.1): once per frame, before the ticks. Edges wait for the next tick,
/// held state is immediate.
///
/// # Safety
/// `buffer` must point to `length` readable bytes for the call.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_capture_input(
    world: *mut FynkelWorld,
    buffer: *const u8,
    length: u32,
) -> Status {
    if buffer.is_null() && length != 0 {
        return Status::InvalidArgument;
    }
    let bytes: &[u8] = if length == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(buffer, length as usize)
    };
    guarded(world, |w| {
        w.stats.bytes_in += bytes.len() as u64;
        let events = if bytes.is_empty() {
            Vec::new()
        } else {
            match bridge::decode_input_events(bytes) {
                Ok(v) => v,
                Err(e) => {
                    log::warn!("fynkel_world_capture_input: protocol error {e:?}");
                    return Status::Protocol;
                }
            }
        };
        w.inner.capture_input(&events);
        Status::Ok
    })
}

/// The bound actions as the next tick — and this frame — see them
/// (`FYNKEL_BUFFER_INPUT_ACTIONS`, one record per action in declaration
/// order). Valid until the next call or mutating call.
///
/// # Safety
/// `world` live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_input_view(world: *mut FynkelWorld) -> FynkelBufferView {
    let Some(w) = world.as_mut() else {
        return FynkelBufferView::empty();
    };
    let status = guarded(world, |w| {
        let view = w.inner.input_view();
        let buf = std::mem::take(&mut w.input_buf);
        w.input_buf = bridge::encode_input_view(buf, &view);
        w.stats.bytes_out += w.input_buf.len() as u64;
        Status::Ok
    });
    if status == Status::Ok {
        FynkelBufferView::of(&w.input_buf)
    } else {
        FynkelBufferView::empty()
    }
}

/// Answers a `FYNKEL_BUFFER_PHYSICS_QUERIES` buffer against the last
/// stepped physics state (M3-08): the view is a `Query` buffer of
/// `fynkel.rec.PhysicsHit` records, in query order, nearest first within
/// a query, valid until the next query call or mutating call. A framing
/// error is `Status::Protocol` with an empty view; the world stays usable.
///
/// # Safety
/// `buffer` must point to `length` readable bytes for the call;
/// `out_view` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_physics_query(
    world: *mut FynkelWorld,
    buffer: *const u8,
    length: u32,
    out_view: *mut FynkelBufferView,
) -> Status {
    if buffer.is_null() && length != 0 {
        return Status::InvalidArgument;
    }
    let bytes: &[u8] = if length == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(buffer, length as usize)
    };
    let status = guarded(world, |w| {
        w.stats.query_calls += 1;
        w.stats.bytes_in += bytes.len() as u64;
        let buf = std::mem::take(&mut w.query_buf);
        match bridge::physics_queries(&w.inner, bytes, buf) {
            Ok(b) => {
                w.query_buf = b;
                w.stats.bytes_out += w.query_buf.len() as u64;
                Status::Ok
            }
            Err(e) => {
                log::warn!("fynkel_world_physics_query: protocol error {e:?}");
                w.query_buf = buffer::empty(BufferKind::Query);
                Status::Protocol
            }
        }
    });
    if let Some(out) = out_view.as_mut() {
        *out = match (status, world.as_ref()) {
            (Status::Ok, Some(w)) => FynkelBufferView::of(&w.query_buf),
            _ => FynkelBufferView::empty(),
        };
    }
    status
}

/// What `fynkel_world_project` projected from (M3-13).
#[repr(C)]
#[derive(Default)]
pub struct FynkelProjectionInfo {
    /// The camera entity the frame is rendered from; 0 when the world has
    /// no enabled camera (every point is then `PROJECTED_NO_CAMERA`).
    pub camera: u64,
    /// Anchors projected (at most `FYNKEL_MAX_PROJECTION_ANCHORS`).
    pub projected: u32,
    /// Anchors beyond that bound (`PROJECTED_DROPPED`).
    pub dropped: u32,
}

/// Answers a `FYNKEL_BUFFER_PROJECTION_ANCHORS` buffer (M3-13, blueprint
/// §10): every anchor projected into a `width` × `height` viewport from
/// the frame's camera and interpolated poses — the same matrices the
/// renderer uses — as a `Query` buffer of `fynkel.rec.ProjectedPoint`
/// records in request order, valid until the next query call or mutating
/// call. At most 64 anchors are projected; the rest come back
/// `PROJECTED_DROPPED` and the world reports `TOO_MANY_ANCHORS`. A framing
/// error is `Status::Protocol` with an empty view; the world stays usable.
///
/// # Safety
/// `buffer` must point to `length` readable bytes for the call;
/// `out_info` and `out_view` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_project(
    world: *mut FynkelWorld,
    width: u32,
    height: u32,
    buffer: *const u8,
    length: u32,
    out_info: *mut FynkelProjectionInfo,
    out_view: *mut FynkelBufferView,
) -> Status {
    if buffer.is_null() && length != 0 {
        return Status::InvalidArgument;
    }
    let bytes: &[u8] = if length == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(buffer, length as usize)
    };
    let mut info = FynkelProjectionInfo::default();
    let status = guarded(world, |w| {
        w.stats.query_calls += 1;
        w.stats.bytes_in += bytes.len() as u64;
        let buf = std::mem::take(&mut w.query_buf);
        let mut anchors = std::mem::take(&mut w.anchors);
        let mut points = std::mem::take(&mut w.points);
        let r = bridge::project(
            &mut w.inner,
            width,
            height,
            bytes,
            buf,
            &mut anchors,
            &mut points,
        );
        w.anchors = anchors;
        w.points = points;
        match r {
            Ok((b, i)) => {
                w.query_buf = b;
                w.stats.bytes_out += w.query_buf.len() as u64;
                if i.dropped > 0 {
                    w.rebuild_diagnostics();
                }
                info = FynkelProjectionInfo {
                    camera: i.camera.map(|e| e.to_bits()).unwrap_or(0),
                    projected: i.projected,
                    dropped: i.dropped,
                };
                Status::Ok
            }
            Err(e) => {
                log::warn!("fynkel_world_project: protocol error {e:?}");
                w.query_buf = buffer::empty(BufferKind::Query);
                Status::Protocol
            }
        }
    });
    if let Some(out) = out_info.as_mut() {
        *out = info;
    }
    if let Some(out) = out_view.as_mut() {
        *out = match (status, world.as_ref()) {
            (Status::Ok, Some(w)) => FynkelBufferView::of(&w.query_buf),
            _ => FynkelBufferView::empty(),
        };
    }
    status
}

/// Opens the default output device and moves the mixer into its callback
/// (M3-11): what a host that plays calls once after the world is created.
/// `Status::Unsupported` when the library was built without the `device`
/// feature or no device could be opened — the world reports
/// `AUDIO_DEVICE` in its diagnostics and keeps playing silently on the
/// null backend, so a game runs the same without sound.
///
/// # Safety
/// `world` must be live.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_audio_open(world: *mut FynkelWorld) -> Status {
    guarded(world, |w| {
        if w.inner.open_audio_device() {
            Status::Ok
        } else {
            w.rebuild_diagnostics();
            Status::Unsupported
        }
    })
}

/// Pauses (`suspended != 0`) or resumes the output device (M3-11): the
/// host calls it from its lifecycle (the app going to the background).
/// A no-op without a device.
///
/// # Safety
/// `world` must be live.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_audio_suspend(
    world: *mut FynkelWorld,
    suspended: u32,
) -> Status {
    guarded(world, |w| {
        w.inner.audio_mut().suspend(suspended != 0);
        Status::Ok
    })
}

/// Closes the output device (M3-11); the world continues on the null
/// backend. A no-op without a device.
///
/// # Safety
/// `world` must be live.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_audio_close(world: *mut FynkelWorld) -> Status {
    guarded(world, |w| {
        w.inner.audio_mut().close_device();
        Status::Ok
    })
}

/// # Safety
/// `entities` must point to `count` handles for the call.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_subscribe(
    world: *mut FynkelWorld,
    component: u32,
    entities: *const u64,
    count: u32,
    enable: i32,
) -> Status {
    if ComponentId(component) != ComponentId::TRANSFORM {
        return Status::InvalidArgument;
    }
    if entities.is_null() && count != 0 {
        return Status::InvalidArgument;
    }
    let handles: Vec<Entity> = if count == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(entities, count as usize)
            .iter()
            .map(|&bits| Entity::from_bits(bits))
            .collect()
    };
    guarded(world, |w| {
        w.inner.subscribe(&handles, enable != 0);
        Status::Ok
    })
}

/// # Safety
/// `world` live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_snapshot(world: *const FynkelWorld) -> FynkelBufferView {
    match world.as_ref() {
        Some(w) => FynkelBufferView::of(&w.snapshot_buf),
        None => FynkelBufferView::empty(),
    }
}

/// # Safety
/// `world` live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_diagnostics(world: *const FynkelWorld) -> FynkelBufferView {
    match world.as_ref() {
        Some(w) => FynkelBufferView::of(&w.diagnostics_buf),
        None => FynkelBufferView::empty(),
    }
}

/// # Safety
/// `world` live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_drain_events(world: *mut FynkelWorld) -> FynkelBufferView {
    let Some(w) = world.as_mut() else {
        return FynkelBufferView::empty();
    };
    let status = guarded(world, |w| {
        let events = w.inner.drain_events();
        let buf = std::mem::take(&mut w.events_buf);
        w.events_buf = bridge::encode_events(buf, &events);
        w.stats.bytes_out += w.events_buf.len() as u64;
        Status::Ok
    });
    if status == Status::Ok {
        FynkelBufferView::of(&w.events_buf)
    } else {
        FynkelBufferView::empty()
    }
}

/// # Safety
/// `world` live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_after_physics_events(
    world: *const FynkelWorld,
) -> FynkelBufferView {
    match world.as_ref() {
        Some(w) => FynkelBufferView::of(&w.after_physics_buf),
        None => FynkelBufferView::empty(),
    }
}

/// # Safety
/// `desc` must be non-NULL; `out_view` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_query(
    world: *mut FynkelWorld,
    desc: *const FynkelQueryDesc,
    out_view: *mut FynkelBufferView,
) -> Status {
    let Some(d) = desc.as_ref() else {
        return Status::InvalidArgument;
    };
    let (kind, request_id, entity) = (d.kind, d.request_id, Entity::from_bits(d.entity));
    let status = guarded(world, |w| {
        w.stats.query_calls += 1;
        let buf = std::mem::take(&mut w.query_buf);
        match bridge::query(&w.inner, kind, request_id, entity, buf) {
            Some(b) => {
                w.query_buf = b;
                w.stats.bytes_out += w.query_buf.len() as u64;
                Status::Ok
            }
            None => {
                w.query_buf = buffer::empty(BufferKind::Query);
                Status::InvalidArgument
            }
        }
    });
    if let Some(out) = out_view.as_mut() {
        *out = match (status, world.as_ref()) {
            (Status::Ok, Some(w)) => FynkelBufferView::of(&w.query_buf),
            _ => FynkelBufferView::empty(),
        };
    }
    status
}

/// # Safety
/// `world` live or NULL; `out_stats` non-NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_stats(
    world: *const FynkelWorld,
    out_stats: *mut FynkelWorldStats,
) -> Status {
    let (Some(w), Some(out)) = (world.as_ref(), out_stats.as_mut()) else {
        return Status::InvalidArgument;
    };
    *out = FynkelWorldStats {
        tick: w.inner.tick(),
        frame: w.inner.clock().frame(),
        submit_calls: w.stats.submit_calls,
        step_calls: w.stats.step_calls,
        query_calls: w.stats.query_calls,
        bytes_in: w.stats.bytes_in,
        bytes_out: w.stats.bytes_out,
        live_entities: w.inner.live_entities(),
        subscriptions: w.inner.subscription_count() as u32,
        staged_commands: w.inner.staged_count() as u32,
        poisoned: w.inner.is_poisoned() as u32,
        snapshot_bytes: w.snapshot_buf.len() as u64,
        snapshot_peak_bytes: w.stats.snapshot_peak_bytes,
        snapshot_budget: w.stats.snapshot_budget,
        reserved: 0,
        native_bytes: fynkel_core::memory::heap_bytes(),
        resident_bytes: fynkel_core::memory::resident_bytes(),
        residency_bytes: w.assets.as_ref().map(|a| a.stats().bytes).unwrap_or(0),
    };
    Status::Ok
}

/// The subscription budget (M3-16, blueprint C5): a ceiling in bytes on a
/// tick's snapshot. The first step whose snapshot exceeds it reports
/// `FYNKEL_DIAGNOSTIC_SNAPSHOT_BUDGET` (once, until the snapshot is back
/// under); the snapshot is still delivered whole. 0 removes the ceiling.
///
/// # Safety
/// `world` live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_set_snapshot_budget(
    world: *mut FynkelWorld,
    bytes: u32,
) -> Status {
    let Some(w) = world.as_mut() else {
        return Status::InvalidArgument;
    };
    w.stats.snapshot_budget = bytes;
    w.stats.over_budget = false;
    Status::Ok
}

/// # Safety
/// `world` live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_tick(world: *const FynkelWorld) -> u64 {
    world.as_ref().map(|w| w.inner.tick()).unwrap_or(0)
}

/// # Safety
/// `world` live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_alpha(world: *const FynkelWorld) -> f32 {
    world.as_ref().map(|w| w.inner.alpha()).unwrap_or(0.0)
}

/// # Safety
/// `world` live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_is_poisoned(world: *const FynkelWorld) -> i32 {
    world
        .as_ref()
        .map(|w| w.inner.is_poisoned() as i32)
        .unwrap_or(1)
}
