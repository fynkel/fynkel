//! F2 owned edit sessions. No Rust borrow or host pointer survives an exchange.
use crate::{FynkelBufferView, FynkelWorld};
use fynkel_contracts::{ComponentId, Entity, ObjectRef, Status};
use fynkel_core::{
    ComponentValue, EditError, GameplayComponentValue, GameplayEdit, GameplayEntity,
    GameplayObjectRef, GameplaySession, PendingEntity, Transform, World,
};
use fynkel_wire::{StringTable, WireRecord};
use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};
use std::panic::{catch_unwind, AssertUnwindSafe};

struct PendingBindings {
    rebinds: BTreeMap<String, Vec<String>>,
    report: Vec<u8>,
}

pub(crate) struct WorldSlot {
    world: Option<World>,
    batches: Vec<fynkel_core::RegisteredBatch>,
    session: Option<GameplaySession>,
    prepared: Vec<fynkel_assets::PreparedPrefab>,
    resources: fynkel_assets::PreparedGameplayResources,
    published: Vec<fynkel_assets::PublishedPrefab>,
    pending_bindings: Option<PendingBindings>,
    materials: BTreeMap<fynkel_contracts::Uuid, RuntimeMaterial>,
    meshes: BTreeMap<fynkel_contracts::Uuid, RuntimeMesh>,
    clips: BTreeMap<fynkel_contracts::Uuid, Option<fynkel_core::animation::Clip>>,
}
impl WorldSlot {
    pub fn new(world: World) -> Self {
        Self {
            world: Some(world),
            batches: Vec::new(),
            session: None,
            prepared: Vec::new(),
            resources: Default::default(),
            published: Vec::new(),
            pending_bindings: None,
            materials: BTreeMap::new(),
            meshes: BTreeMap::new(),
            clips: BTreeMap::new(),
        }
    }
    pub fn is_editing(&self) -> bool {
        self.session.is_some()
    }
    pub fn reject_during_edit(&mut self) -> bool {
        if let Some(session) = &mut self.session {
            let _: Result<(), _> = session.access(|_| Err(EditError::Aborted));
            true
        } else {
            false
        }
    }
    fn abort(&mut self) {
        self.batches.clear();
        self.prepared.clear();
        self.resources = Default::default();
        self.pending_bindings = None;
        self.clips.clear();
        self.materials.clear();
        self.meshes.clear();
        if let Some(session) = self.session.take() {
            self.world = Some(session.abort());
        }
    }
}
impl Deref for WorldSlot {
    type Target = World;
    fn deref(&self) -> &World {
        self.world
            .as_ref()
            .unwrap_or_else(|| self.session.as_ref().expect("owned world").committed())
    }
}
impl DerefMut for WorldSlot {
    fn deref_mut(&mut self) -> &mut World {
        self.world
            .as_mut()
            .expect("world exclusively owned by gameplay edit")
    }
}
pub(crate) fn boundary(
    world: *mut FynkelWorld,
    f: impl FnOnce(&mut FynkelWorld) -> Status,
) -> Status {
    let Some(w) = (unsafe { world.as_mut() }) else {
        return Status::InvalidArgument;
    };
    if w.inner.is_poisoned() {
        w.gameplay_reply = b"world is poisoned".to_vec();
        return Status::Poisoned;
    }
    match catch_unwind(AssertUnwindSafe(|| f(w))) {
        Ok(s) => s,
        Err(_) => {
            w.inner.abort();
            w.inner.poison();
            w.panic = Some("gameplay bridge panic".into());
            w.gameplay_reply = b"gameplay bridge panic".to_vec();
            Status::Panic
        }
    }
}
fn reject(w: &mut FynkelWorld, status: Status, message: impl ToString) -> Status {
    if let Some(session) = &mut w.inner.session {
        let _: Result<(), _> = session.access(|_| Err(EditError::Aborted));
    }
    w.gameplay_reply = message.to_string().into_bytes();
    status
}
/// # Safety
/// World must be live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_gameplay_tick_prepare(world: *mut FynkelWorld) -> Status {
    boundary(world, |w| {
        if w.inner.reject_during_edit() {
            return Status::CommandsRejected;
        }
        w.inner.prepare_gameplay_tick();
        Status::Ok
    })
}
/// # Safety
/// World must be live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_gameplay_tick_cancel(world: *mut FynkelWorld) -> Status {
    boundary(world, |w| {
        if w.inner.reject_during_edit() {
            return Status::CommandsRejected;
        }
        w.inner.cancel_gameplay_tick();
        Status::Ok
    })
}
/// # Safety
/// World must be live or NULL; out_sequence must be writable or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_gameplay_begin(
    world: *mut FynkelWorld,
    out_sequence: *mut u64,
) -> Status {
    boundary(world, |w| {
        let Some(out) = out_sequence.as_mut() else {
            return reject(w, Status::InvalidArgument, "missing sequence output");
        };
        *out = 0;
        if !w.gameplay_allowed {
            return Status::Unsupported;
        }
        if w.inner.session.is_some() {
            return reject(w, Status::CommandsRejected, "nested gameplay edit");
        }
        let core = w.inner.world.take().expect("world");
        match GameplaySession::begin(core) {
            Ok(session) => {
                *out = session.sequence();
                w.inner.session = Some(session);
                w.inner.batches.clear();
                w.gameplay_pending.clear();
                w.gameplay_reply.clear();
                Status::Ok
            }
            Err(failure) => {
                let (core, error) = *failure;
                w.inner.world = Some(core);
                reject(w, Status::CommandsRejected, error)
            }
        }
    })
}
/// Register a runtime-defined schema outside a gameplay edit. Encoding is the
/// versioned gameplay scalar protocol; identity/layout conflicts are rejected.
/// # Safety
/// World must be live or null; input readable for length, output writable or null.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_component_register(
    world: *mut FynkelWorld,
    data: *const u8,
    length: u32,
    out: *mut FynkelBufferView,
) -> Status {
    if let Some(out) = out.as_mut() {
        *out = FynkelBufferView::empty();
    }
    let status = boundary(world, |w| {
        if data.is_null() || out.is_null() || length == 0 {
            return reject(w, Status::InvalidArgument, "invalid registration buffer");
        }
        if w.inner.reject_during_edit() {
            return reject(w, Status::CommandsRejected, "registration during edit");
        }
        let mut reader = Reader {
            bytes: std::slice::from_raw_parts(data, length as usize),
            at: 0,
            sequence: 0,
            world: w.inner.id().0,
            pending: &[],
        };
        let schema = (|| -> Result<_, &'static str> {
            if reader.u32()? != 1 {
                return Err("unsupported schema encoding");
            }
            let id = reader.string()?;
            let version = reader.u32()?;
            let count = reader.u32()?;
            let mut fields = Vec::new();
            for _ in 0..count {
                fields.push(fynkel_core::RegisteredField {
                    name: reader.string()?,
                    default: reader.scalar()?,
                });
            }
            if reader.at != reader.bytes.len() {
                return Err("trailing registration data");
            }
            Ok(fynkel_core::ComponentSchema {
                id,
                version,
                fields,
            })
        })();
        let schema = match schema {
            Ok(s) => s,
            Err(e) => return reject(w, Status::InvalidArgument, e),
        };
        match w.inner.register_dynamic_component(schema) {
            Ok(()) => {
                w.gameplay_reply.clear();
                Status::Ok
            }
            Err(e) => reject(w, Status::CommandsRejected, e),
        }
    });
    if let (Some(w), Some(out)) = (world.as_mut(), out.as_mut()) {
        *out = FynkelBufferView::of(&w.gameplay_reply);
    }
    status
}

/// Read-only live identity/name snapshot for SDK lifecycle reconciliation.
/// Success: u32 live count and (entity, u32 name length, UTF-8 name) pairs.
/// # Safety
/// World must be live or NULL; out must be writable or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_gameplay_state(
    world: *mut FynkelWorld,
    out: *mut FynkelBufferView,
) -> Status {
    gameplay_state(world, None, out)
}

/// Conditional identity/name snapshot, using a world-local consumer cursor.
/// Success: u64 revision, u32 changed (0 or 1), then the full snapshot if changed.
/// Zero always requests a full snapshot. Other reads do not consume this cursor.
/// # Safety
/// World must be live or NULL; out must be writable or NULL. Cursor belongs to world.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_gameplay_state_since(
    world: *mut FynkelWorld,
    since: u64,
    out: *mut FynkelBufferView,
) -> Status {
    gameplay_state(world, Some(since), out)
}

/// Bounded-journal identity delta. Zero requests initial state. Success: cursor
/// u64, mode u32 (0 unchanged, 1 reset, 2 delta), then count u32 and rows
/// (entity, alive u32, name length u32 + UTF-8 when alive) for modes 1/2.
/// # Safety
/// World live or NULL; out writable or NULL. Cursor is local to this world.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_gameplay_state_delta(
    world: *mut FynkelWorld,
    since: u64,
    out: *mut FynkelBufferView,
) -> Status {
    if let Some(view) = out.as_mut() {
        *view = FynkelBufferView::empty();
    }
    let status = boundary(world, |w| {
        if out.is_null() {
            return reject(w, Status::InvalidArgument, "missing state output");
        }
        if !w.gameplay_allowed {
            return reject(w, Status::Unsupported, "gameplay state unavailable");
        }
        if w.inner.is_editing() {
            return reject(
                w,
                Status::CommandsRejected,
                "state reconciliation inside an edit",
            );
        }
        let Some(changes) = w.inner.identity_changes_since(since) else {
            return reject(w, Status::CommandsRejected, "unsettled native write");
        };
        let mut reply = std::mem::take(&mut w.gameplay_reply);
        reply.clear();
        reply.extend(changes.cursor.token().to_le_bytes());
        let mode = if changes.reset {
            1
        } else if changes.entities.is_empty() {
            0
        } else {
            2
        };
        put32(&mut reply, mode);
        if mode != 0 {
            let entities = if changes.reset {
                w.inner.ecs().entities().collect()
            } else {
                changes.entities
            };
            put32(&mut reply, entities.len() as u32);
            for e in entities {
                entity(&mut reply, e);
                let alive = w.inner.ecs().contains(e);
                put32(&mut reply, u32::from(alive));
                if alive {
                    if let Ok(name) = w.inner.ecs().get::<fynkel_core::Name>(e) {
                        put32(&mut reply, name.value.len() as u32);
                        reply.extend(name.value.as_bytes());
                    } else {
                        put32(&mut reply, 0);
                    }
                }
            }
        }
        w.gameplay_reply = reply;
        Status::Ok
    });
    if let (Some(w), Some(view)) = (world.as_ref(), out.as_mut()) {
        *view = FynkelBufferView::of(&w.gameplay_reply);
    }
    status
}

unsafe fn gameplay_state(
    world: *mut FynkelWorld,
    since: Option<u64>,
    out: *mut FynkelBufferView,
) -> Status {
    if let Some(view) = out.as_mut() {
        *view = FynkelBufferView::empty();
    }
    let status = boundary(world, |w| {
        if out.is_null() {
            return reject(w, Status::InvalidArgument, "missing state output");
        }
        if !w.gameplay_allowed {
            return reject(w, Status::Unsupported, "gameplay state unavailable");
        }
        if w.inner.is_editing() {
            return reject(
                w,
                Status::CommandsRejected,
                "state reconciliation inside an edit",
            );
        }
        let mut reply = std::mem::take(&mut w.gameplay_reply);
        reply.clear();
        if let Some(since) = since {
            let Some(revision) = w.inner.identity_revision() else {
                return reject(
                    w,
                    Status::CommandsRejected,
                    "unsettled native component write",
                );
            };
            reply.extend(revision.to_le_bytes());
            put32(&mut reply, u32::from(since != revision));
            if since == revision {
                w.gameplay_reply = reply;
                return Status::Ok;
            }
        }
        put32(&mut reply, w.inner.live_entities());
        for e in w.inner.ecs().entities() {
            entity(&mut reply, e);
            let name = w
                .inner
                .ecs()
                .get::<fynkel_core::Name>(e)
                .map(|n| n.value.clone())
                .unwrap_or_default();
            put32(&mut reply, name.len() as u32);
            reply.extend(name.as_bytes());
        }
        w.gameplay_reply = reply;
        Status::Ok
    });
    if let (Some(w), Some(view)) = (world.as_ref(), out.as_mut()) {
        *view = FynkelBufferView::of(&w.gameplay_reply);
    }
    status
}

/// # Safety
/// World live or NULL; non-NULL data readable for len bytes, output writable.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_gameplay_exchange(
    world: *mut FynkelWorld,
    sequence: u64,
    data: *const u8,
    len: u32,
    out: *mut FynkelBufferView,
) -> Status {
    if let Some(out) = out.as_mut() {
        *out = FynkelBufferView::empty();
    }
    let status = boundary(world, |w| {
        if out.is_null() || data.is_null() || len > 32 * 1024 * 1024 {
            return reject(w, Status::InvalidArgument, "invalid exchange buffer");
        }
        if w.inner
            .session
            .as_ref()
            .is_none_or(|s| s.sequence() != sequence)
        {
            return reject(w, Status::CommandsRejected, "expired edit sequence");
        }
        let bytes = std::slice::from_raw_parts(data, len as usize);
        w.stats.bytes_in += bytes.len() as u64;
        let mut reader = Reader {
            bytes,
            at: 0,
            sequence,
            world: w.inner.id().0,
            pending: &w.gameplay_pending,
        };
        let request = match reader.request().and_then(|r| {
            if reader.at == bytes.len() {
                Ok(r)
            } else {
                Err("trailing request bytes")
            }
        }) {
            Ok(r) => r,
            Err(e) => return reject(w, Status::Protocol, e),
        };
        if let Request::AssetJson(id) = request {
            if let Err(error) = w.inner.session.as_mut().unwrap().access(|_| Ok(())) {
                return reject(w, Status::CommandsRejected, error);
            }
            let Some(assets) = &w.services.assets else {
                return reject(w, Status::CommandsRejected, "no cooked project is loaded");
            };
            return match assets.document_json(id) {
                Ok((_, json)) => {
                    w.gameplay_reply.clear();
                    put32(&mut w.gameplay_reply, json.len() as u32);
                    w.gameplay_reply.extend(json.as_bytes());
                    Status::Ok
                }
                Err(error) => reject(w, Status::CommandsRejected, error),
            };
        }
        if let Request::RuntimeResource(kind, id, payload) = request {
            if let Err(error) = w.inner.session.as_mut().unwrap().access(|_| Ok(())) {
                return reject(w, Status::CommandsRejected, error);
            }
            let Some(assets) = &w.services.assets else {
                return reject(
                    w,
                    Status::CommandsRejected,
                    "runtime resources require a loaded asset store",
                );
            };
            match kind {
                29 => match decode_runtime_material(&payload) {
                    Ok(material) => {
                        w.inner.materials.insert(id, RuntimeMaterial::Pbr(material));
                    }
                    Err(error) => return reject(w, Status::CommandsRejected, error),
                },
                30 => match decode_runtime_mesh(&payload) {
                    Ok(mesh) => {
                        w.inner.meshes.insert(id, mesh);
                    }
                    Err(error) => return reject(w, Status::CommandsRejected, error),
                },
                31 => {
                    let material = if payload.is_empty() {
                        None
                    } else {
                        let (status, diagnostics, material) =
                            crate::prepare_shader_material(assets, &payload);
                        if status != Status::Ok {
                            let message = diagnostics
                                .iter()
                                .map(|d| fynkel_document::diagnostic_json(d).to_string())
                                .collect::<Vec<_>>()
                                .join("; ");
                            return reject(
                                w,
                                Status::CommandsRejected,
                                format!("invalid shader material: {message}"),
                            );
                        }
                        material
                    };
                    w.inner
                        .materials
                        .insert(id, RuntimeMaterial::Shader(material));
                }
                _ => unreachable!(),
            }
            w.gameplay_reply.clear();
            return Status::Ok;
        }
        if let Request::RuntimeClip(id, payload) = request {
            if let Err(error) = w.inner.session.as_mut().unwrap().access(|_| Ok(())) {
                return reject(w, Status::CommandsRejected, error);
            }
            if w.services.assets.is_none() {
                return reject(
                    w,
                    Status::CommandsRejected,
                    "runtime clips require a loaded asset store",
                );
            }
            let key = fynkel_contracts::AssetRef::new(id);
            let clip = match decode_runtime_clip(&key, &payload) {
                Ok(clip) => clip,
                Err(error) => return reject(w, Status::CommandsRejected, error),
            };
            w.inner.clips.insert(id, clip);
            w.gameplay_reply.clear();
            return Status::Ok;
        }
        if let Request::RegisteredQuery(id) = &request {
            let result = w
                .inner
                .session
                .as_mut()
                .unwrap()
                .access(|edit| edit.query_registered(id));
            return match result {
                Ok(batch) => {
                    let token = w.inner.batches.len() as u32;
                    let mut reply = Vec::new();
                    put32(&mut reply, token);
                    put32(&mut reply, batch.rows().len() as u32);
                    for (owner, fields) in batch.rows() {
                        reference(
                            &mut reply,
                            Some(*owner),
                            sequence,
                            w.inner.id().0,
                            &mut w.gameplay_pending,
                        );
                        encode_scalars(
                            &mut reply,
                            fields,
                            sequence,
                            w.inner.id().0,
                            &mut w.gameplay_pending,
                        );
                    }
                    w.inner.batches.push(batch);
                    w.gameplay_reply = reply;
                    Status::Ok
                }
                Err(e) => reject(w, Status::CommandsRejected, e),
            };
        }
        if let Request::RegisteredApply(token, values) = request {
            let Some(batch) = w.inner.batches.get(token as usize) else {
                return reject(w, Status::CommandsRejected, "unknown bulk view");
            };
            return match w
                .inner
                .session
                .as_mut()
                .unwrap()
                .access(|edit| edit.apply_registered_batch(batch, values))
            {
                Ok(()) => {
                    w.gameplay_reply.clear();
                    Status::Ok
                }
                Err(e) => reject(w, Status::CommandsRejected, e),
            };
        }
        if let Request::BindActions(rebinds) = request {
            return bind_actions(w, rebinds);
        }
        if let Request::Instantiate(id, transform, parent, name) = request {
            return instantiate(w, id, transform, parent, name, sequence);
        }
        if let Some(assets) = &w.services.assets {
            let slot = &mut w.inner;
            let session = slot.session.as_mut().unwrap();
            let core = session.committed();
            match &request {
                Request::Set(_, value) => slot.resources.component(
                    assets,
                    core,
                    &value.clone().into_native(|_| ObjectRef::default()),
                ),
                Request::AnimateControl(
                    _,
                    fynkel_core::AnimateOp::Play { clip, .. }
                    | fynkel_core::AnimateOp::Crossfade { clip, .. }
                    | fynkel_core::AnimateOp::PlayOneShot { motion: clip, .. },
                ) => slot.resources.motion_request(assets, core, clip),
                Request::Sound(sound) => slot.resources.sound_request(assets, core, &sound.clip),
                _ => {}
            }
            if let Err(error) = slot.resources.stage_controllers(session) {
                return reject(w, Status::CommandsRejected, error);
            }
        }
        let result = w
            .inner
            .session
            .as_mut()
            .unwrap()
            .access(|edit| request.apply(edit));
        match result {
            Ok(answer) => {
                w.gameplay_reply = answer.encode(sequence, w.inner.id().0, &mut w.gameplay_pending);
                Status::Ok
            }
            Err(e) => reject(w, Status::CommandsRejected, e),
        }
    });
    if let (Some(w), Some(out)) = (world.as_mut(), out.as_mut()) {
        w.stats.bytes_out += w.gameplay_reply.len() as u64;
        *out = FynkelBufferView::of(&w.gameplay_reply);
    }
    status
}
/// # Safety
/// World live or NULL; output writable or NULL. commit is exactly 0 or 1.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_gameplay_end(
    world: *mut FynkelWorld,
    sequence: u64,
    commit: u32,
    out: *mut FynkelBufferView,
) -> Status {
    if let Some(out) = out.as_mut() {
        *out = FynkelBufferView::empty();
    }
    let status = boundary(world, |w| {
        if out.is_null() || commit > 1 {
            return reject(w, Status::InvalidArgument, "invalid end arguments");
        }
        if w.inner
            .session
            .as_ref()
            .is_none_or(|s| s.sequence() != sequence)
        {
            return reject(w, Status::CommandsRejected, "expired edit sequence");
        }
        w.gameplay_reply.clear();
        w.inner.batches.clear();
        let session = w.inner.session.take().unwrap();
        let bindings = w.inner.pending_bindings.take();
        let clips = std::mem::take(&mut w.inner.clips);
        let materials = std::mem::take(&mut w.inner.materials);
        let meshes = std::mem::take(&mut w.inner.meshes);
        if commit == 0 {
            w.inner.world = Some(session.abort());
            w.inner.prepared.clear();
            w.inner.resources = Default::default();
            return Status::Ok;
        }
        let plans = std::mem::take(&mut w.inner.prepared);
        let (core, result) = fynkel_assets::PreparedPrefab::finish_session_with_resources(
            plans,
            std::mem::take(&mut w.inner.resources),
            session,
        );
        w.inner.world = Some(core);
        match result {
            Ok((report, published)) => {
                for (id, material) in materials {
                    publish_runtime_material(w, fynkel_contracts::AssetRef::new(id), material);
                }
                for (id, mesh) in meshes {
                    publish_runtime_mesh(w, fynkel_contracts::AssetRef::new(id), mesh);
                }
                for (id, clip) in clips {
                    publish_runtime_clip(w, fynkel_contracts::AssetRef::new(id), clip);
                }
                if let Some(bindings) = bindings {
                    w.services.rebinds = bindings.rebinds;
                    w.load_report = bindings.report;
                }
                w.inner.published.extend(published);
                let resolved: Vec<_> = w
                    .gameplay_pending
                    .iter()
                    .enumerate()
                    .filter_map(|(i, p)| report.resolve(*p).map(|e| (i, e)))
                    .collect();
                put32(&mut w.gameplay_reply, resolved.len() as u32);
                for (i, e) in resolved {
                    put32(&mut w.gameplay_reply, i as u32);
                    entity(&mut w.gameplay_reply, e);
                }
                Status::Ok
            }
            Err(error) => {
                if w.inner.is_poisoned() {
                    w.panic = Some("gameplay publication or final validation panicked".into());
                    reject(w, Status::Panic, error)
                } else {
                    reject(w, Status::CommandsRejected, error)
                }
            }
        }
    });
    if let (Some(w), Some(out)) = (world.as_mut(), out.as_mut()) {
        w.stats.bytes_out += w.gameplay_reply.len() as u64;
        *out = FynkelBufferView::of(&w.gameplay_reply);
    }
    status
}
fn bind_actions(w: &mut FynkelWorld, rebinds: BTreeMap<String, Vec<String>>) -> Status {
    if let Err(error) = w.inner.session.as_mut().unwrap().access(|_| Ok(())) {
        return reject(w, Status::CommandsRejected, error);
    }
    let Some(assets) = w.services.assets.clone() else {
        return reject(w, Status::CommandsRejected, "no cooked project is loaded");
    };
    let specs = match crate::load::prepare_project_input(&assets, &rebinds) {
        Ok(specs) => specs,
        Err(error) => return reject(w, Status::CommandsRejected, format!("{error:?}")),
    };
    let errors = match w
        .inner
        .session
        .as_mut()
        .unwrap()
        .access(|edit| edit.bind_input(&specs))
    {
        Ok(errors) => errors,
        Err(error) => return reject(w, Status::CommandsRejected, error),
    };
    let input = crate::load::project_input_json(&specs, &rebinds, &errors);
    let report = serde_json::to_vec(&crate::load::binding_report(&assets, input))
        .expect("binding report JSON");
    w.gameplay_reply.clear();
    put32(&mut w.gameplay_reply, report.len() as u32);
    w.gameplay_reply.extend_from_slice(&report);
    w.inner.pending_bindings = Some(PendingBindings { rebinds, report });
    Status::Ok
}
fn instantiate(
    w: &mut FynkelWorld,
    id: fynkel_contracts::Uuid,
    transform: Transform,
    parent: Option<GameplayEntity>,
    name: String,
    sequence: u64,
) -> Status {
    let Some(assets) = w.services.assets.clone() else {
        return reject(w, Status::CommandsRejected, "no cooked project is loaded");
    };
    let prepared = match assets.prepare_prefab(id) {
        Ok(p) => p,
        Err(errors) => return reject(w, Status::CommandsRejected, format!("{errors:?}")),
    };
    if let Err(error) = prepared.stage_controllers(w.inner.session.as_mut().unwrap()) {
        return reject(w, Status::CommandsRejected, error);
    }
    let prefab = prepared.prefab();
    let label = if name.is_empty() { &prefab.name } else { &name };
    let instance = match w
        .inner
        .session
        .as_mut()
        .unwrap()
        .access(|edit| edit.instantiate(&prefab.description, label, transform, parent))
    {
        Ok(instance) => instance,
        Err(error) => return reject(w, Status::CommandsRejected, error),
    };
    let metadata = serde_json::json!({
        "prefab": prefab.id.to_string(), "path": prefab.path, "name": label,
        "assets": prefab.closure.len(), "residency": crate::load::residency_json(&assets),
        "diagnostics": prefab.diagnostics.iter().map(fynkel_document::diagnostic_json).collect::<Vec<_>>(),
        "behaviours": prefab.behaviours.iter().map(|b| serde_json::json!({
            "authoringId": b.entity.to_string(), "component": b.component.to_string(),
            "type": b.type_id, "version": b.version, "known": b.known,
            "data": b.data, "pointer": b.pointer,
        })).collect::<Vec<_>>(),
    }).to_string();
    let mut reply = Vec::new();
    let world = w.inner.id().0;
    reference(
        &mut reply,
        Some(instance.root.into()),
        sequence,
        world,
        &mut w.gameplay_pending,
    );
    put32(&mut reply, instance.entities.len() as u32);
    for (id, pending) in instance.entities {
        let id = id.to_string();
        put32(&mut reply, id.len() as u32);
        reply.extend(id.as_bytes());
        reference(
            &mut reply,
            Some(pending.into()),
            sequence,
            world,
            &mut w.gameplay_pending,
        );
    }
    put32(&mut reply, metadata.len() as u32);
    reply.extend(metadata.as_bytes());
    w.inner.prepared.push(prepared);
    w.gameplay_reply = reply;
    Status::Ok
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    sequence: u64,
    world: u64,
    pending: &'a [PendingEntity],
}
impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], &'static str> {
        let end = self.at.checked_add(n).ok_or("length overflow")?;
        let b = self.bytes.get(self.at..end).ok_or("truncated request")?;
        self.at = end;
        Ok(b)
    }
    fn u32(&mut self) -> Result<u32, &'static str> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, &'static str> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn string(&mut self) -> Result<String, &'static str> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| "invalid UTF-8")
    }
    fn scalar(&mut self) -> Result<fynkel_core::ScalarValue, &'static str> {
        use fynkel_core::ScalarValue as V;
        Ok(match self.u32()? {
            0 => V::Float(f64::from_bits(self.u64()?)),
            1 => V::Int(self.u64()? as i64),
            2 => V::Bool(match self.u32()? {
                0 => false,
                1 => true,
                _ => return Err("invalid bool"),
            }),
            3 => V::Text(self.string()?),
            4 => V::Object(self.reference()?),
            5 => V::Asset(fynkel_core::AssetRef::with_sub(
                self.string()?.parse().map_err(|_| "invalid asset UUID")?,
                self.string()?,
            )),
            _ => return Err("invalid scalar kind"),
        })
    }
    fn scalars(&mut self) -> Result<Vec<fynkel_core::ScalarValue>, &'static str> {
        let count = self.u32()?;
        let mut values = Vec::new();
        for _ in 0..count {
            values.push(self.scalar()?);
        }
        Ok(values)
    }
    fn reference(&mut self) -> Result<Option<GameplayEntity>, &'static str> {
        let kind = self.u32()?;
        let index = self.u32()?;
        let world = self.u64()?;
        let slot = self.u32()?;
        let generation = self.u32()?;
        let sequence = self.u64()?;
        match kind {
            0 if index == 0 && world == 0 && slot == 0 && generation == 0 && sequence == 0 => {
                Ok(None)
            }
            1 if index == 0 && sequence == 0 && world != 0 && generation != 0 => Ok(Some(
                Entity::new(fynkel_contracts::WorldId(world), slot, generation).into(),
            )),
            2 if world == self.world
                && sequence == self.sequence
                && slot == 0
                && generation == 0 =>
            {
                self.pending
                    .get(index as usize)
                    .copied()
                    .map(|p| Some(p.into()))
                    .ok_or("unknown pending reference")
            }
            _ => Err("invalid or expired reference"),
        }
    }
    fn owner(&mut self) -> Result<GameplayEntity, &'static str> {
        self.reference()?.ok_or("missing owner")
    }
    fn audio_bus(&mut self) -> Result<fynkel_core::AudioBus, &'static str> {
        match self.u32()? {
            0 => Ok(fynkel_core::AudioBus::Master),
            1 => Ok(fynkel_core::AudioBus::Music),
            2 => Ok(fynkel_core::AudioBus::Sfx),
            _ => Err("invalid audio bus"),
        }
    }
    fn boolean(&mut self) -> Result<bool, &'static str> {
        match self.u32()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err("invalid boolean"),
        }
    }
    fn component(&mut self, id: ComponentId) -> Result<GameplayComponentValue, &'static str> {
        let size = self.u32()? as usize;
        let body = self.take(size)?.to_vec();
        let size = self.u32()? as usize;
        let tail = self.take(size)?.to_vec();
        let native = fynkel_wire::decode_component_value(id.0, &body, &tail)
            .ok_or("unknown component")?
            .map_err(|_| "invalid component bytes")?;
        if fynkel_wire::component_value_size(&native) != body.len() {
            return Err("invalid component size");
        }
        let expected = ComponentValue::object_ref_fields(id)
            .ok_or("unknown component")?
            .len();
        let n = self.u32()? as usize;
        if n != expected {
            return Err("reference count mismatch");
        }
        let mut refs = Vec::new();
        for _ in 0..n {
            refs.push(
                self.reference()?
                    .map_or(GameplayObjectRef::NONE, Into::into),
            );
        }
        // Reject conflicting reference sources: wire scalar bodies carry None.
        let mut native = native;
        let mut invalid = false;
        native.for_each_object_ref(|_, _, r| invalid |= !r.is_none());
        if invalid {
            return Err("reference must use scoped field");
        }
        let mut refs = refs.into_iter();
        Ok(GameplayComponentValue::from(native).map_object_refs(|_| refs.next().unwrap()))
    }
    fn request(&mut self) -> Result<Request, &'static str> {
        if self.u32()? != 1 {
            return Err("unsupported gameplay protocol version");
        }
        Ok(match self.u32()? {
            1 => {
                let t = Transform::decode(self.take(40)?, &[]).map_err(|_| "invalid transform")?;
                let p = self.reference()?;
                Request::Spawn(t, p, self.string()?)
            }
            2 => Request::Destroy(self.owner()?),
            3 => {
                let e = self.owner()?;
                let id = ComponentId(self.u32()?);
                Request::Set(e, self.component(id)?)
            }
            4 => Request::Get(self.owner()?, ComponentId(self.u32()?)),
            5 => {
                let n = self.u32()?;
                if n > 1024 {
                    return Err("too many filters");
                };
                let mut ids = Vec::new();
                for _ in 0..n {
                    ids.push(ComponentId(self.u32()?))
                }
                Request::Query(ids)
            }
            6 => Request::Name(self.owner()?),
            7 => Request::SetName(self.owner()?, self.string()?),
            8 => Request::Parent(self.owner()?),
            9 => Request::SetParent(self.owner()?, self.reference()?, self.boolean()?),
            10 => Request::Children(self.owner()?),
            11 => Request::Active(self.owner()?),
            12 => Request::SetActive(self.owner()?, self.boolean()?),
            13 => Request::Matrix(self.owner()?),
            14 => Request::Contains(self.owner()?),
            15 => Request::Remove(self.owner()?, ComponentId(self.u32()?)),
            16 => Request::Find(self.string()?),
            17 => {
                let id = self.string()?.parse().map_err(|_| "invalid prefab UUID")?;
                let t = Transform::decode(self.take(40)?, &[]).map_err(|_| "invalid transform")?;
                Request::Instantiate(id, t, self.reference()?, self.string()?)
            }
            18 => Request::AssetJson(self.string()?.parse().map_err(|_| "invalid asset UUID")?),
            19 => {
                let owner = self.owner()?;
                let command = fynkel_core::schema::BodyCommandRecord::decode(self.take(48)?, &[])
                    .map_err(|_| "invalid body effect")?;
                if command.entity != Entity::NULL {
                    return Err("body effect owner must use scoped reference");
                }
                Request::Body(owner, command.operation())
            }
            20 => {
                let body = self.take(40)?.to_vec();
                let size = self.u32()? as usize;
                let tail = self.take(size)?;
                Request::Sound(
                    fynkel_core::schema::PlaySoundCommandRecord::decode(&body, tail)
                        .map_err(|_| "invalid sound effect")?,
                )
            }
            21 => {
                let owner = self.owner()?;
                let record = fynkel_core::schema::AudioCommandRecord::decode(self.take(24)?, &[])
                    .map_err(|_| "invalid audio control")?;
                if record.entity != Entity::NULL {
                    return Err("audio owner must use scoped reference");
                }
                Request::Audio(owner, record.operation())
            }
            25 => Request::SetAudioBus(
                self.audio_bus()?,
                f32::from_bits(self.u32()?),
                self.boolean()?,
            ),
            26 => Request::AudioBus(self.audio_bus()?),
            32 => Request::IdentityChanges,
            33 => Request::Persist(self.owner()?),
            34 => Request::PersistentRoots,
            kind @ 29..=31 => {
                let id = self
                    .string()?
                    .parse()
                    .map_err(|_| "invalid resource UUID")?;
                let length = self.u32()? as usize;
                Request::RuntimeResource(kind, id, self.take(length)?.to_vec())
            }
            28 => {
                let id = self.string()?.parse().map_err(|_| "invalid clip UUID")?;
                let length = self.u32()? as usize;
                Request::RuntimeClip(id, self.take(length)?.to_vec())
            }
            27 => Request::BindActions(
                serde_json::from_str(&self.string()?).map_err(|_| "invalid bindings JSON")?,
            ),
            23 => Request::ControllerParameter(self.owner()?, self.string()?),
            24 => {
                let source = self.owner()?;
                let parent = self.reference()?;
                let transform = if self.boolean()? {
                    Some(Transform::decode(self.take(40)?, &[]).map_err(|_| "invalid transform")?)
                } else {
                    None
                };
                let name = if self.boolean()? {
                    Some(self.string()?)
                } else {
                    None
                };
                Request::Clone(source, parent, transform, name)
            }
            22 => {
                let owner = self.owner()?;
                let body = self.take(88)?.to_vec();
                let size = self.u32()? as usize;
                let tail = self.take(size)?;
                let record = fynkel_core::schema::AnimateCommandRecord::decode(&body, tail)
                    .map_err(|_| "invalid animation control")?;
                if record.entity != Entity::NULL {
                    return Err("animator owner must use scoped reference");
                }
                if matches!(
                    record.op,
                    fynkel_core::schema::AnimateOp::PlayState
                        | fynkel_core::schema::AnimateOp::CrossFadeState
                ) && record.layer < -1
                {
                    return Err("invalid animator layer");
                }
                Request::AnimateControl(owner, record.operation())
            }
            35 => Request::RegisteredGet(self.owner()?, self.string()?),
            36 => Request::RegisteredSet(self.owner()?, self.string()?, self.scalars()?),
            37 => Request::RegisteredRemove(self.owner()?, self.string()?),
            38 => Request::RegisteredQuery(self.string()?),
            39 => {
                let token = self.u32()?;
                let count = self.u32()?;
                let mut rows = Vec::new();
                for _ in 0..count {
                    rows.push(self.scalars()?);
                }
                Request::RegisteredApply(token, rows)
            }
            _ => return Err("unknown operation"),
        })
    }
}
enum Request {
    RegisteredGet(GameplayEntity, String),
    RegisteredSet(GameplayEntity, String, Vec<fynkel_core::ScalarValue>),
    RegisteredRemove(GameplayEntity, String),
    RegisteredQuery(String),
    RegisteredApply(u32, Vec<Vec<fynkel_core::ScalarValue>>),
    IdentityChanges,
    Persist(GameplayEntity),
    PersistentRoots,
    RuntimeClip(fynkel_contracts::Uuid, Vec<u8>),
    RuntimeResource(u32, fynkel_contracts::Uuid, Vec<u8>),
    SetAudioBus(fynkel_core::AudioBus, f32, bool),
    AudioBus(fynkel_core::AudioBus),
    BindActions(BTreeMap<String, Vec<String>>),
    Clone(
        GameplayEntity,
        Option<GameplayEntity>,
        Option<Transform>,
        Option<String>,
    ),
    AssetJson(fynkel_contracts::Uuid),
    Instantiate(
        fynkel_contracts::Uuid,
        Transform,
        Option<GameplayEntity>,
        String,
    ),
    Spawn(Transform, Option<GameplayEntity>, String),
    Destroy(GameplayEntity),
    Set(GameplayEntity, GameplayComponentValue),
    Get(GameplayEntity, ComponentId),
    Query(Vec<ComponentId>),
    Name(GameplayEntity),
    SetName(GameplayEntity, String),
    Parent(GameplayEntity),
    SetParent(GameplayEntity, Option<GameplayEntity>, bool),
    Children(GameplayEntity),
    Active(GameplayEntity),
    SetActive(GameplayEntity, bool),
    Matrix(GameplayEntity),
    Contains(GameplayEntity),
    Remove(GameplayEntity, ComponentId),
    Find(String),
    Body(GameplayEntity, fynkel_core::BodyCommand),
    Audio(GameplayEntity, fynkel_core::AudioOp),
    AnimateControl(GameplayEntity, fynkel_core::AnimateOp),
    ControllerParameter(GameplayEntity, String),
    Sound(fynkel_core::schema::PlaySoundCommandRecord),
}
enum Answer {
    Registered(Option<Vec<fynkel_core::ScalarValue>>),
    IdentityChanges(Vec<(GameplayEntity, Option<String>)>),
    Clone(fynkel_core::GameplayClone),
    Empty,
    Ref(Option<GameplayEntity>),
    Refs(Vec<GameplayEntity>),
    Component(Option<GameplayComponentValue>),
    Name(Option<String>),
    Words(Vec<u32>),
}
impl Request {
    fn apply(self, e: &mut GameplayEdit<'_>) -> Result<Answer, EditError> {
        Ok(match self {
            Self::RegisteredGet(owner, id) => Answer::Registered(e.registered_values(owner, &id)?),
            Self::RegisteredSet(owner, id, fields) => {
                e.set_registered_values(owner, &id, fields)?;
                Answer::Empty
            }
            Self::RegisteredRemove(owner, id) => {
                e.remove_registered(owner, &id)?;
                Answer::Empty
            }
            Self::RegisteredQuery(_) | Self::RegisteredApply(..) => return Err(EditError::Aborted),
            Self::IdentityChanges => Answer::IdentityChanges(e.identity_changes()?),
            Self::Persist(owner) => Answer::Words(vec![e.dont_destroy_on_load(owner)? as u32]),
            Self::PersistentRoots => Answer::Refs(e.persistent_roots()?),
            Self::Instantiate(..)
            | Self::AssetJson(..)
            | Self::BindActions(..)
            | Self::RuntimeClip(..)
            | Self::RuntimeResource(..) => return Err(EditError::Aborted),
            Self::SetAudioBus(bus, volume, muted) => {
                e.set_audio_bus(bus, volume, muted)?;
                Answer::Empty
            }
            Self::AudioBus(bus) => {
                let (volume, muted) = e.audio_bus(bus)?;
                Answer::Words(vec![volume.to_bits(), muted as u32])
            }
            Self::Clone(source, parent, transform, name) => {
                Answer::Clone(e.clone_tree(source, parent, transform, name)?)
            }
            Self::Spawn(t, p, n) => Answer::Ref(Some(e.spawn(n, t, p)?.into())),
            Self::Destroy(x) => {
                e.destroy(x)?;
                Answer::Empty
            }
            Self::Set(x, v) => {
                e.set_component(x, v)?;
                Answer::Empty
            }
            Self::Get(x, id) => Answer::Component(e.component(x, id)?),
            Self::Query(ids) => {
                let mut q = e.query_with(&ids)?;
                let mut all = Vec::new();
                while let Some(x) = q.next(e)? {
                    all.push(x)
                }
                Answer::Refs(all)
            }
            Self::Name(x) => Answer::Name(e.name(x)?),
            Self::SetName(x, n) => {
                e.set_name(x, n)?;
                Answer::Empty
            }
            Self::Parent(x) => Answer::Ref(e.parent(x)?),
            Self::SetParent(x, p, k) => {
                e.set_parent(x, p, k)?;
                Answer::Empty
            }
            Self::Children(x) => Answer::Refs(e.children(x)?),
            Self::Active(x) => Answer::Words(vec![
                e.active_self(x)? as u32,
                e.active_in_hierarchy(x)? as u32,
            ]),
            Self::SetActive(x, a) => {
                e.set_active(x, a)?;
                Answer::Empty
            }
            Self::Matrix(x) => Answer::Words(
                e.world_matrix(x)?
                    .to_cols_array()
                    .into_iter()
                    .map(f32::to_bits)
                    .collect(),
            ),
            Self::Contains(x) => Answer::Words(vec![e.contains(x)? as u32]),
            Self::Remove(x, id) => {
                e.remove_component(x, id)?;
                Answer::Empty
            }
            Self::Find(n) => Answer::Ref(e.find(&n)?),
            Self::Sound(c) => {
                e.play_sound(
                    c.clip,
                    fynkel_core::PlayParams {
                        bus: c.bus,
                        volume: c.volume,
                        pitch: c.pitch,
                        priority: c.priority,
                        looping: false,
                    },
                )?;
                Answer::Empty
            }
            Self::ControllerParameter(owner, name) => {
                use fynkel_core::animation::controller::{ParameterKind as K, Value};
                Answer::Words(match e.controller_parameter(owner, &name)? {
                    None => vec![0],
                    Some((kind, value)) => vec![
                        match kind {
                            K::Float => 1,
                            K::Int => 2,
                            K::Bool => 3,
                            K::Trigger => 4,
                        },
                        match value {
                            Value::Float(v) => v.to_bits(),
                            Value::Int(v) => v as u32,
                            Value::Bool(v) => v as u32,
                        },
                    ],
                })
            }
            Self::AnimateControl(owner, op) => {
                e.animate_control(owner, op)?;
                Answer::Empty
            }
            Self::Audio(owner, op) => {
                e.audio_control(owner, op)?;
                Answer::Empty
            }
            Self::Body(owner, op) => {
                e.body_effect(owner, op)?;
                Answer::Empty
            }
        })
    }
}
fn put32(b: &mut Vec<u8>, v: u32) {
    b.extend(v.to_le_bytes())
}
fn put64(b: &mut Vec<u8>, v: u64) {
    b.extend(v.to_le_bytes())
}
fn entity(b: &mut Vec<u8>, e: Entity) {
    put64(b, e.world().0);
    put32(b, e.slot());
    put32(b, e.generation());
}
fn reference(
    b: &mut Vec<u8>,
    r: Option<GameplayEntity>,
    seq: u64,
    world: u64,
    pending: &mut Vec<PendingEntity>,
) {
    match r {
        None => b.extend([0; 32]),
        Some(GameplayEntity::Live(e)) => {
            put32(b, 1);
            put32(b, 0);
            entity(b, e);
            put64(b, 0)
        }
        Some(GameplayEntity::Pending(p)) => {
            let i = pending.iter().position(|x| *x == p).unwrap_or_else(|| {
                pending.push(p);
                pending.len() - 1
            });
            put32(b, 2);
            put32(b, i as u32);
            put64(b, world);
            put32(b, 0);
            put32(b, 0);
            put64(b, seq)
        }
    }
}
fn encode_scalars(
    b: &mut Vec<u8>,
    fields: &[fynkel_core::ScalarValue],
    seq: u64,
    world: u64,
    pending: &mut Vec<PendingEntity>,
) {
    use fynkel_core::ScalarValue as V;
    put32(b, fields.len() as u32);
    for field in fields {
        match field {
            V::Float(v) => {
                put32(b, 0);
                put64(b, v.to_bits());
            }
            V::Int(v) => {
                put32(b, 1);
                put64(b, *v as u64);
            }
            V::Bool(v) => {
                put32(b, 2);
                put32(b, *v as u32);
            }
            V::Text(v) => {
                put32(b, 3);
                put32(b, v.len() as u32);
                b.extend(v.as_bytes());
            }
            V::Object(v) => {
                put32(b, 4);
                reference(b, *v, seq, world, pending);
            }
            V::Asset(v) => {
                put32(b, 5);
                let id = v.asset.to_string();
                put32(b, id.len() as u32);
                b.extend(id.as_bytes());
                put32(b, v.sub.len() as u32);
                b.extend(v.sub.as_bytes());
            }
        }
    }
}
impl Answer {
    fn encode(self, seq: u64, world: u64, pending: &mut Vec<PendingEntity>) -> Vec<u8> {
        let mut b = Vec::new();
        match self {
            Self::Registered(fields) => {
                put32(&mut b, fields.is_some() as u32);
                if let Some(fields) = fields {
                    encode_scalars(&mut b, &fields, seq, world, pending);
                }
            }
            Self::IdentityChanges(changes) => {
                put32(&mut b, changes.len() as u32);
                for (owner, name) in changes {
                    reference(&mut b, Some(owner), seq, world, pending);
                    put32(&mut b, name.is_some() as u32);
                    if let Some(name) = name {
                        put32(&mut b, name.len() as u32);
                        b.extend(name.as_bytes());
                    }
                }
            }
            Self::Empty => {}
            Self::Clone(tree) => {
                reference(&mut b, Some(tree.root.into()), seq, world, pending);
                put32(&mut b, tree.entities.len() as u32);
                for (source, copy) in tree.entities {
                    reference(&mut b, Some(source), seq, world, pending);
                    reference(&mut b, Some(copy.into()), seq, world, pending);
                }
            }
            Self::Ref(r) => reference(&mut b, r, seq, world, pending),
            Self::Refs(refs) => {
                put32(&mut b, refs.len() as u32);
                for r in refs {
                    reference(&mut b, Some(r), seq, world, pending)
                }
            }
            Self::Name(n) => {
                put32(&mut b, n.is_some() as u32);
                if let Some(n) = n {
                    put32(&mut b, n.len() as u32);
                    b.extend(n.as_bytes())
                }
            }
            Self::Words(words) => {
                for w in words {
                    put32(&mut b, w)
                }
            }
            Self::Component(c) => {
                put32(&mut b, c.is_some() as u32);
                if let Some(c) = c {
                    let mut refs = Vec::new();
                    c.for_each_reference(|r| refs.push(r.entity()));
                    let native = c.into_native(|_| ObjectRef::NONE);
                    let size = fynkel_wire::component_value_size(&native);
                    let mut body = vec![0; size];
                    let mut strings = StringTable::new();
                    fynkel_wire::encode_component_value(&native, &mut body, &mut strings);
                    put32(&mut b, size as u32);
                    b.extend(body);
                    let tail = strings.into_bytes();
                    put32(&mut b, tail.len() as u32);
                    b.extend(tail);
                    put32(&mut b, refs.len() as u32);
                    for r in refs {
                        reference(&mut b, r, seq, world, pending)
                    }
                }
            }
        }
        b
    }
}

/// Shared validation for the owned edit protocol and the direct native adapter.
pub(crate) enum RuntimeMaterial {
    Pbr(Option<fynkel_assets::PbrMaterial>),
    Shader(Option<fynkel_assets::CustomMaterialPayload>),
}
pub(crate) struct RuntimeMesh {
    mesh: Option<fynkel_assets::MeshPayload>,
    collision: Option<fynkel_core::physics::PreparedMesh>,
}
pub(crate) fn decode_runtime_material(
    payload: &[u8],
) -> Result<Option<fynkel_assets::PbrMaterial>, &'static str> {
    if payload.is_empty() {
        return Ok(None);
    }
    let m = fynkel_wire::material::decode_material(payload).ok_or("invalid material payload")?;
    if !m.base_color.is_finite()
        || !m.emissive.is_finite()
        || ![
            m.metallic,
            m.roughness,
            m.emissive_strength,
            m.normal_scale,
            m.occlusion_strength,
            m.alpha_cutoff,
        ]
        .iter()
        .all(|v| v.is_finite())
    {
        return Err("material values must be finite");
    }
    Ok(Some(m))
}
pub(crate) fn publish_runtime_material(
    w: &mut FynkelWorld,
    key: fynkel_contracts::AssetRef,
    material: RuntimeMaterial,
) {
    let assets = w.services.assets.as_ref().expect("prepared asset store");
    match material {
        RuntimeMaterial::Pbr(m) => assets.set_runtime_material(&key, m),
        RuntimeMaterial::Shader(m) => assets.set_runtime_custom_material(&key, m),
    }
}
pub(crate) fn decode_runtime_mesh(payload: &[u8]) -> Result<RuntimeMesh, &'static str> {
    if payload.is_empty() {
        return Ok(RuntimeMesh {
            mesh: None,
            collision: None,
        });
    }
    let mesh = fynkel_assets::decode_mesh(payload).ok_or("invalid mesh payload")?;
    if mesh.is_skinned()
        || mesh.indices.is_empty()
        || mesh.indices.len() % 3 != 0
        || !mesh.vertices.iter().all(|v| {
            v.position
                .iter()
                .chain(&v.normal)
                .chain(&v.tangent)
                .chain(&v.uv0)
                .chain(&v.uv1)
                .chain(&v.color)
                .all(|x| x.is_finite())
        })
    {
        return Err("invalid runtime mesh geometry");
    }
    let positions: Vec<_> = mesh.vertices.iter().map(|v| v.position).collect();
    // Renderable meshes above the collision budget remain valid render assets.
    // As before, colliders report meshUnavailable at the next physics step.
    let collision = fynkel_core::physics::PreparedMesh::new(&positions, &mesh.indices).ok();
    Ok(RuntimeMesh {
        mesh: Some(mesh),
        collision,
    })
}
pub(crate) fn publish_runtime_mesh(
    w: &mut FynkelWorld,
    key: fynkel_contracts::AssetRef,
    mesh: RuntimeMesh,
) {
    w.core_mut()
        .physics_mut()
        .replace_prepared_mesh(&key, mesh.collision);
    w.services
        .assets
        .as_ref()
        .expect("prepared asset store")
        .set_runtime_mesh(&key, mesh.mesh);
}

pub(crate) fn decode_runtime_clip(
    key: &fynkel_contracts::AssetRef,
    payload: &[u8],
) -> Result<Option<fynkel_core::animation::Clip>, &'static str> {
    if payload.is_empty() {
        return Ok(None);
    }
    let clip = fynkel_assets::decode_animation(payload)
        .and_then(|p| fynkel_assets::clip_from_payload(key.to_string(), p))
        .ok_or("invalid runtime animation clip")?;
    clip.validate()
        .map_err(|_| "invalid runtime animation clip")?;
    Ok(Some(clip))
}

/// Only validated clips and a stable asset store may reach publication.
pub(crate) fn publish_runtime_clip(
    world: &mut FynkelWorld,
    key: fynkel_contracts::AssetRef,
    clip: Option<fynkel_core::animation::Clip>,
) {
    world
        .services
        .assets
        .as_ref()
        .expect("validated clip asset store")
        .set_runtime_clip(&key, clip.clone())
        .expect("validated runtime clip");
    // Removal preserves already-playing clips, as the direct API has always done.
    if let Some(clip) = clip {
        world.core_mut().register_clip(key, clip);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;
    fn fixture() -> *mut FynkelWorld {
        Box::into_raw(Box::new(FynkelWorld::with_core(World::new(
            Default::default(),
        ))))
    }
    unsafe fn begin(w: *mut FynkelWorld) -> u64 {
        let mut seq = 0;
        assert_eq!(fynkel_world_gameplay_begin(w, &mut seq), Status::Ok);
        seq
    }
    fn spawn_request() -> Vec<u8> {
        let mut b = Vec::new();
        put32(&mut b, 1);
        put32(&mut b, 1);
        let mut t = vec![0; 40];
        Transform::default().encode(&mut t, &mut StringTable::new());
        b.extend(t);
        b.extend([0; 32]);
        put32(&mut b, 0);
        b
    }
    unsafe fn exchange(w: *mut FynkelWorld, s: u64, b: &[u8]) -> Status {
        let mut view = FynkelBufferView::empty();
        fynkel_world_gameplay_exchange(w, s, b.as_ptr(), b.len() as u32, &mut view)
    }
    unsafe fn end(w: *mut FynkelWorld, s: u64, c: u32) -> Status {
        let mut view = FynkelBufferView::empty();
        fynkel_world_gameplay_end(w, s, c, &mut view)
    }
    fn bus_request(bus: u32, volume: f32, muted: u32) -> Vec<u8> {
        [1, 25, bus, volume.to_bits(), muted]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect()
    }

    fn component_registration() -> Vec<u8> {
        let mut b = Vec::new();
        put32(&mut b, 1);
        let text = "game.Health";
        put32(&mut b, text.len() as u32);
        b.extend(text.as_bytes());
        put32(&mut b, 1);
        put32(&mut b, 1);
        put32(&mut b, 2);
        b.extend(b"hp");
        put32(&mut b, 0); // float
        b.extend(100.0f64.to_le_bytes());
        b
    }
    unsafe fn register(w: *mut FynkelWorld, bytes: &[u8]) -> Status {
        let mut out = FynkelBufferView::empty();
        fynkel_world_component_register(w, bytes.as_ptr(), bytes.len() as u32, &mut out)
    }
    #[test]
    fn registered_schema_protocol_rejects_truncation_trailing_data_and_active_edits() {
        unsafe {
            let w = fixture();
            let valid = component_registration();
            let mut cases: Vec<_> = (0..valid.len()).map(|n| valid[..n].to_vec()).collect();
            let mut trailing = valid.clone();
            trailing.push(0);
            cases.push(trailing);
            let mut unknown_kind = valid.clone();
            let tag = unknown_kind.len() - 12;
            unknown_kind[tag..tag + 4].copy_from_slice(&99u32.to_le_bytes());
            cases.push(unknown_kind);
            for bytes in cases {
                assert_ne!(register(w, &bytes), Status::Ok);
                assert_eq!((*w).inner.registered_schemas().count(), 0);
                assert!(!(*w).inner.is_poisoned());
            }
            let seq = begin(w);
            assert_eq!(exchange(w, seq, &spawn_request()), Status::Ok);
            assert_eq!(register(w, &valid), Status::CommandsRejected);
            assert_eq!(end(w, seq, 1), Status::CommandsRejected);
            assert_eq!((*w).inner.live_entities(), 0);
            assert_eq!(register(w, &valid), Status::Ok);
            assert_eq!((*w).inner.registered_schemas().count(), 1);
            fynkel_world_destroy(w);
        }
    }
    #[test]
    fn registered_bulk_tokens_expire_and_malformed_rows_roll_back_the_whole_edit() {
        unsafe {
            let w = fixture();
            assert_eq!(register(w, &component_registration()), Status::Ok);
            let mut query = vec![];
            put32(&mut query, 1);
            put32(&mut query, 38);
            put32(&mut query, 11);
            query.extend(b"game.Health");
            let apply: Vec<_> = [1u32, 39, 0, 0]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect();
            let seq = begin(w);
            assert_eq!(exchange(w, seq, &query), Status::Ok);
            assert_eq!(exchange(w, seq, &apply), Status::Ok);
            assert_eq!(end(w, seq, 1), Status::Ok);
            for bytes in
                std::iter::once(apply.clone()).chain((0..apply.len()).map(|n| apply[..n].to_vec()))
            {
                let seq = begin(w);
                assert_eq!(exchange(w, seq, &spawn_request()), Status::Ok);
                assert_ne!(exchange(w, seq, &bytes), Status::Ok);
                assert_eq!(end(w, seq, 1), Status::CommandsRejected);
                assert_eq!((*w).inner.live_entities(), 0);
            }
            let seq = begin(w);
            assert_eq!(exchange(w, seq, &query), Status::Ok);
            assert_eq!(exchange(w, seq, &spawn_request()), Status::Ok);
            assert_eq!(exchange(w, seq, &apply), Status::CommandsRejected);
            assert_eq!(end(w, seq, 1), Status::CommandsRejected);
            assert_eq!((*w).inner.live_entities(), 0);
            fynkel_world_destroy(w);
        }
    }

    #[test]
    fn runtime_resource_protocol_rejects_truncation_and_missing_assets_atomically() {
        unsafe {
            let w = fixture();
            for operation in 28..=31 {
                let mut request = Vec::new();
                put32(&mut request, 1);
                put32(&mut request, operation);
                let id = "d0000000-0000-4000-8000-000000000099";
                put32(&mut request, id.len() as u32);
                request.extend(id.as_bytes());
                put32(&mut request, 0);
                for length in 0..=request.len() {
                    let sequence = begin(w);
                    assert_eq!(exchange(w, sequence, &spawn_request()), Status::Ok);
                    assert_eq!(
                        exchange(w, sequence, &request[..length]),
                        if length == request.len() {
                            Status::CommandsRejected
                        } else {
                            Status::Protocol
                        }
                    );
                    assert_eq!(end(w, sequence, 1), Status::CommandsRejected);
                    assert_eq!((*w).inner.live_entities(), 0);
                    assert!((*w).inner.clips.is_empty());
                    assert!((*w).inner.materials.is_empty());
                    assert!((*w).inner.meshes.is_empty());
                }
            }
            drop(Box::from_raw(w));
        }
    }

    #[test]
    fn input_binding_protocol_rejects_bad_json_and_missing_project_stickily() {
        unsafe {
            let w = fixture();
            for (json, expected) in [
                ("[]", Status::Protocol),
                (r#"{"jump":[1]}"#, Status::Protocol),
                ("{", Status::Protocol),
                (r#"{"jump":["Key.Space"]}"#, Status::CommandsRejected),
            ] {
                let sequence = begin(w);
                assert_eq!(exchange(w, sequence, &spawn_request()), Status::Ok);
                let mut request = Vec::new();
                put32(&mut request, 1);
                put32(&mut request, 27);
                put32(&mut request, json.len() as u32);
                request.extend_from_slice(json.as_bytes());
                assert_eq!(exchange(w, sequence, &request), expected);
                assert_ne!(end(w, sequence, 1), Status::Ok);
                assert_eq!((*w).inner.live_entities(), 0);
                assert!((*w).services.rebinds.is_empty());
                assert!(!(*w).inner.is_poisoned());
            }
            fynkel_world_destroy(w);
        }
    }

    #[test]
    fn malformed_bus_payloads_abort_data_and_leave_committed_audio_unchanged() {
        unsafe {
            let w = fixture();
            let valid = bus_request(0, 0.25, 1);
            let mut cases: Vec<Vec<u8>> = (0..valid.len()).map(|n| valid[..n].to_vec()).collect();
            cases.push(bus_request(3, 0.25, 0));
            cases.push(bus_request(0, 0.25, 2));
            let mut trailing = valid.clone();
            trailing.push(0);
            cases.push(trailing);
            for request in cases {
                let sequence = begin(w);
                assert_eq!(exchange(w, sequence, &spawn_request()), Status::Ok);
                assert_eq!(exchange(w, sequence, &request), Status::Protocol);
                assert_ne!(end(w, sequence, 1), Status::Ok);
                assert_eq!((*w).inner.live_entities(), 0);
                assert!(!(*w).inner.is_poisoned());
                let sequence = begin(w);
                let read: Vec<u8> = [1_u32, 26, 0]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect();
                assert_eq!(exchange(w, sequence, &read), Status::Ok);
                assert_eq!(
                    (*w).gameplay_reply,
                    [1_f32.to_bits(), 0]
                        .into_iter()
                        .flat_map(u32::to_le_bytes)
                        .collect::<Vec<_>>()
                );
                assert_eq!(end(w, sequence, 0), Status::Ok);
            }
            fynkel_world_destroy(w);
        }
    }

    #[cfg(feature = "audio")]
    #[test]
    fn audio_bus_protocol_reads_staged_values_but_only_publishes_on_commit() {
        unsafe {
            let w = fixture();
            for commit in [0, 1] {
                let sequence = begin(w);
                assert_eq!(exchange(w, sequence, &bus_request(1, 0.25, 1)), Status::Ok);
                let read: Vec<u8> = [1_u32, 26, 1]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect();
                assert_eq!(exchange(w, sequence, &read), Status::Ok);
                assert_eq!(
                    (*w).gameplay_reply,
                    [0.25_f32.to_bits(), 1]
                        .into_iter()
                        .flat_map(u32::to_le_bytes)
                        .collect::<Vec<_>>()
                );
                assert_eq!((*w).inner.audio().bus(fynkel_core::AudioBus::Music), 1.0);
                assert_eq!(end(w, sequence, commit), Status::Ok);
                assert_eq!(
                    (*w).inner.audio().bus(fynkel_core::AudioBus::Music),
                    if commit == 0 { 1.0 } else { 0.25 }
                );
                assert_eq!((*w).inner.audio().muted(), commit == 1);
            }
            let sequence = begin(w);
            assert_eq!(
                exchange(w, sequence, &bus_request(0, f32::NAN, 0)),
                Status::CommandsRejected
            );
            assert_ne!(end(w, sequence, 1), Status::Ok);
            assert!((*w).inner.audio().muted());
            fynkel_world_destroy(w);
        }
    }

    fn clone_request(source: &[u8]) -> Vec<u8> {
        let mut request = Vec::new();
        put32(&mut request, 1);
        put32(&mut request, 24);
        request.extend(source);
        request.extend([0; 32]); // root parent
        put32(&mut request, 0); // source placement
        put32(&mut request, 1); // explicit empty name is distinct from no override
        put32(&mut request, 0);
        request
    }

    #[test]
    fn clone_protocol_maps_pending_identity_and_commits_or_aborts_together() {
        unsafe {
            let w = fixture();
            for commit in [0, 1] {
                let sequence = begin(w);
                assert_eq!(exchange(w, sequence, &spawn_request()), Status::Ok);
                let source = (*w).gameplay_reply.clone();
                assert_eq!(exchange(w, sequence, &clone_request(&source)), Status::Ok);
                let reply = &(*w).gameplay_reply;
                assert_eq!(reply.len(), 100); // root + count + source/copy pair
                assert_eq!(u32::from_le_bytes(reply[32..36].try_into().unwrap()), 1);
                assert_eq!(&reply[36..68], &source);
                assert_eq!(&reply[..32], &reply[68..100]);
                assert_ne!(&reply[..32], &source);
                assert_eq!((*w).inner.live_entities(), 0);
                assert_eq!(end(w, sequence, commit), Status::Ok);
                assert_eq!((*w).inner.live_entities(), if commit == 0 { 0 } else { 2 });
            }
            fynkel_world_destroy(w);
        }
    }

    #[test]
    fn malformed_clone_requests_abort_before_publication_and_do_not_poison() {
        unsafe {
            let w = fixture();
            // Every truncation, bad optional flag, and trailing payload is rejected.
            for case in 0..87 {
                let sequence = begin(w);
                assert_eq!(exchange(w, sequence, &spawn_request()), Status::Ok);
                let mut request = clone_request(&(*w).gameplay_reply);
                match case {
                    0..=83 => request.truncate(case),
                    84 => request[72..76].copy_from_slice(&2_u32.to_le_bytes()),
                    85 => request[76..80].copy_from_slice(&2_u32.to_le_bytes()),
                    _ => request.push(0),
                }
                assert_eq!(exchange(w, sequence, &request), Status::Protocol);
                assert_ne!(end(w, sequence, 1), Status::Ok);
                assert_eq!((*w).inner.live_entities(), 0);
                assert!(!(*w).inner.is_poisoned());
            }
            fynkel_world_destroy(w);
        }
    }

    #[test]
    fn malformed_live_reference_world_or_generation_aborts_without_poisoning() {
        unsafe {
            let w = fixture();
            for (world_id, generation) in [(0, 1), (1, 0), (0, 0)] {
                let s = begin(w);
                assert_eq!(exchange(w, s, &spawn_request()), Status::Ok);
                let mut request = Vec::new();
                put32(&mut request, 1); // protocol
                put32(&mut request, 14); // contains
                put32(&mut request, 1); // live reference
                put32(&mut request, 0); // pending index
                put64(&mut request, world_id);
                put32(&mut request, 0); // slot zero is valid
                put32(&mut request, generation);
                put64(&mut request, 0); // session sequence
                assert_eq!(exchange(w, s, &request), Status::Protocol);
                assert_ne!(end(w, s, 1), Status::Ok);
                assert_eq!((*w).inner.live_entities(), 0);
                assert!(!(*w).inner.is_poisoned());
            }
            let s = begin(w);
            assert_eq!(exchange(w, s, &spawn_request()), Status::Ok);
            assert_eq!(end(w, s, 1), Status::Ok);
            assert_eq!((*w).inner.live_entities(), 1);
            fynkel_world_destroy(w);
        }
    }

    #[test]
    fn malformed_requests_fail_stickily_before_any_publication() {
        unsafe {
            let w = fixture();
            let valid = spawn_request();
            for n in 0..valid.len() {
                let s = begin(w);
                assert_ne!(exchange(w, s, &valid[..n]), Status::Ok);
                assert_ne!(end(w, s, 1), Status::Ok);
                assert_eq!((*w).inner.live_entities(), 0);
                assert!(!(*w).inner.is_poisoned());
            }
            let s = begin(w);
            let mut trailing = valid.clone();
            trailing.push(0);
            assert_eq!(exchange(w, s, &trailing), Status::Protocol);
            assert_ne!(end(w, s, 1), Status::Ok);
            let s = begin(w);
            let mut version = valid.clone();
            version[0] = 99;
            assert_eq!(exchange(w, s, &version), Status::Protocol);
            assert_ne!(end(w, s, 1), Status::Ok);
            let s = begin(w);
            assert_eq!(exchange(w, s, &valid), Status::Ok);
            assert_eq!(end(w, s, 1), Status::Ok);
            assert_eq!((*w).inner.live_entities(), 1);
            fynkel_world_destroy(w);
        }
    }
    #[test]
    fn session_tokens_expire_and_legacy_mutation_aborts_without_poisoning() {
        unsafe {
            let w = fixture();
            let a = begin(w);
            assert_eq!(end(w, a, 0), Status::Ok);
            let b = begin(w);
            assert_ne!(a, b);
            assert_eq!(exchange(w, a, &spawn_request()), Status::CommandsRejected);
            assert_eq!(end(w, b, 1), Status::CommandsRejected);
            let s = begin(w);
            assert_eq!(exchange(w, s, &spawn_request()), Status::Ok);
            assert_eq!(
                fynkel_world_step(w, std::ptr::null_mut()),
                Status::CommandsRejected
            );
            assert_eq!(end(w, s, 1), Status::CommandsRejected);
            assert_eq!((*w).inner.live_entities(), 0);
            assert!(!(*w).inner.is_poisoned());
            fynkel_world_destroy(w);
        }
    }
    #[test]
    fn null_outputs_and_destroying_open_sessions_are_safe() {
        unsafe {
            assert_eq!(
                fynkel_world_gameplay_begin(std::ptr::null_mut(), std::ptr::null_mut()),
                Status::InvalidArgument
            );
            let w = fixture();
            let s = begin(w);
            assert_eq!(
                fynkel_world_gameplay_exchange(w, s, std::ptr::null(), 4, std::ptr::null_mut()),
                Status::InvalidArgument
            );
            assert_ne!(end(w, s, 1), Status::Ok);
            let s = begin(w);
            assert_eq!(exchange(w, s, &spawn_request()), Status::Ok);
            fynkel_world_destroy(w);
        }
    }
    #[test]
    fn panic_restores_world_and_returns_text_instead_of_a_stale_binary_reply() {
        unsafe {
            let w = fixture();
            let sequence = begin(w);
            assert_eq!(exchange(w, sequence, &spawn_request()), Status::Ok);
            assert_eq!(
                boundary(w, |_| panic!("injected bridge failure")),
                Status::Panic
            );
            assert!((*w).inner.session.is_none());
            assert!((*w).inner.is_poisoned());
            assert_eq!((*w).inner.live_entities(), 0);
            assert_eq!(
                std::str::from_utf8(&(*w).gameplay_reply).unwrap(),
                "gameplay bridge panic"
            );
            assert_eq!(exchange(w, sequence, &spawn_request()), Status::Poisoned);
            assert_eq!(
                std::str::from_utf8(&(*w).gameplay_reply).unwrap(),
                "world is poisoned"
            );
            fynkel_world_destroy(w);
        }
    }
    #[test]
    fn cooked_prefab_without_project_fails_stickily_and_preserves_world() {
        unsafe {
            let w = fixture();
            let seq = begin(w);
            assert_eq!(exchange(w, seq, &spawn_request()), Status::Ok);
            let mut request = Vec::new();
            put32(&mut request, 1);
            put32(&mut request, 17);
            let id = "c0000000-0000-4000-8000-000000000040";
            put32(&mut request, id.len() as u32);
            request.extend(id.as_bytes());
            let mut pose = vec![0; 40];
            Transform::default().encode(&mut pose, &mut StringTable::new());
            request.extend(pose);
            request.extend([0; 32]);
            put32(&mut request, 0);
            assert_eq!(exchange(w, seq, &request), Status::CommandsRejected);
            let mut out = FynkelBufferView::empty();
            assert_eq!(
                fynkel_world_gameplay_end(w, seq, 1, &mut out),
                Status::CommandsRejected
            );
            assert!((*w).inner.prepared.is_empty());
            assert_eq!((*w).inner.live_entities(), 0);
            fynkel_world_destroy(w);
        }
    }
    #[test]
    fn asset_document_without_project_aborts_earlier_journal_writes() {
        unsafe {
            let w = fixture();
            let seq = begin(w);
            assert_eq!(exchange(w, seq, &spawn_request()), Status::Ok);
            let mut request = Vec::new();
            put32(&mut request, 1);
            put32(&mut request, 18);
            let id = "c0000000-0000-4000-8000-000000000040";
            put32(&mut request, id.len() as u32);
            request.extend(id.as_bytes());
            assert_eq!(exchange(w, seq, &request), Status::CommandsRejected);
            let mut out = FynkelBufferView::empty();
            assert_eq!(
                fynkel_world_gameplay_end(w, seq, 1, &mut out),
                Status::CommandsRejected
            );
            assert_eq!((*w).inner.live_entities(), 0);
            fynkel_world_destroy(w);
        }
    }
    #[test]
    fn committed_playback_queries_do_not_abort_or_publish_the_session() {
        unsafe {
            let w = fixture();
            let seq = begin(w);
            assert_eq!(exchange(w, seq, &spawn_request()), Status::Ok);
            for kind in [
                bridge::QUERY_BODY,
                bridge::QUERY_CHARACTER,
                bridge::QUERY_AUDIO,
                bridge::QUERY_AUDIO_STATS,
                bridge::QUERY_ANIMATOR,
                bridge::QUERY_ANIMATOR_CONTROLLER,
                bridge::QUERY_ANIMATOR_PARAMETERS,
            ] {
                let desc = FynkelQueryDesc {
                    kind,
                    request_id: 3,
                    entity: Entity::NULL,
                };
                let mut out = FynkelBufferView::empty();
                assert_eq!(fynkel_world_query(w, &desc, &mut out), Status::Ok);
                assert_eq!((*w).inner.live_entities(), 0);
            }
            end(w, seq, 1);
            assert_eq!((*w).inner.live_entities(), 1);
            fynkel_world_destroy(w);
        }
    }
    #[test]
    fn lifecycle_snapshot_is_read_only_and_reports_current_identity_and_name() {
        unsafe {
            let w = fixture();
            let commit = (*w)
                .inner
                .edit_gameplay(|edit| {
                    let kept = edit.spawn("kept", Transform::IDENTITY, None)?;
                    let canceled = edit.spawn("canceled", Transform::IDENTITY, None)?;
                    edit.destroy(canceled)?;
                    Ok(kept)
                })
                .unwrap();
            let kept = commit.resolve(commit.value).unwrap();
            let mut out = FynkelBufferView::empty();
            for _ in 0..2 {
                assert_eq!(fynkel_world_gameplay_state(w, &mut out), Status::Ok);
                let bytes = &(*w).gameplay_reply;
                assert_eq!(u32::from_le_bytes(bytes[..4].try_into().unwrap()), 1);
                assert_eq!(&bytes[24..], b"kept");
                assert_eq!((*w).inner.tick(), 0);
            }
            (*w).inner.edit_gameplay(|edit| edit.destroy(kept)).unwrap();
            assert_eq!(fynkel_world_gameplay_state(w, &mut out), Status::Ok);
            assert_eq!((*w).gameplay_reply, 0u32.to_le_bytes());
            assert!((*w)
                .inner
                .drain_events()
                .iter()
                .any(|event| matches!(event.payload,
                fynkel_core::EventPayload::Destroyed { entity, .. } if entity == kept)));
            fynkel_world_destroy(w);
        }
    }

    #[test]
    fn conditional_identity_snapshots_are_consumer_local_and_cover_native_changes() {
        unsafe {
            let w = fixture();
            let commit = (*w)
                .inner
                .edit_gameplay(|edit| edit.spawn("first", Transform::IDENTITY, None))
                .unwrap();
            let e = commit.resolve(commit.value).unwrap();
            let mut out = FynkelBufferView::empty();
            let read = move |since| {
                assert_eq!(
                    fynkel_world_gameplay_state_since(w, since, &mut FynkelBufferView::empty()),
                    Status::Ok
                );
                let bytes = &(*w).gameplay_reply;
                (
                    u64::from_le_bytes(bytes[..8].try_into().unwrap()),
                    u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
                    bytes.len(),
                )
            };
            let (first, changed, len) = read(0);
            assert_eq!(changed, 1);
            assert!(len > 12);
            assert_eq!(read(first), (first, 0, 12));
            (*w).inner.ecs().get_mut::<Transform>(e).unwrap().position.x = 4.;
            assert_eq!(read(first), (first, 0, 12));
            // A full read or a second consumer cannot consume the first cursor.
            assert_eq!(fynkel_world_gameplay_state(w, &mut out), Status::Ok);
            assert_eq!(read(0).1, 1);
            assert_eq!(read(first), (first, 0, 12));
            (*w).inner
                .ecs()
                .get_mut::<fynkel_core::Name>(e)
                .unwrap()
                .value = "renamed".into();
            let (renamed, changed, _) = read(first);
            assert_eq!(changed, 1);
            assert_ne!(renamed, first);
            let bytes = &(*w).gameplay_reply;
            assert_eq!(&bytes[36..], b"renamed");
            assert_eq!(read(first).1, 1);
            assert_eq!(read(renamed), (renamed, 0, 12));
            (*w).inner.ecs_mut().remove::<fynkel_core::Name>(e).unwrap();
            let (unnamed, changed, len) = read(renamed);
            assert_eq!((changed, len), (1, 36));
            (*w).inner.edit_gameplay(|edit| edit.destroy(e)).unwrap();
            let (destroyed, changed, len) = read(unnamed);
            assert_eq!((changed, len), (1, 16));
            assert_eq!(read(destroyed), (destroyed, 0, 12));
            assert_eq!((*w).inner.tick(), 0);
            fynkel_world_destroy(w);
        }
    }

    #[test]
    fn conditional_snapshot_rejects_invalid_boundaries_without_accepting_cursor() {
        unsafe {
            for read in [
                fynkel_world_gameplay_state_since,
                fynkel_world_gameplay_state_delta,
            ] {
                let w = fixture();
                let mut out = FynkelBufferView::empty();
                assert_eq!(read(w, 0, std::ptr::null_mut()), Status::InvalidArgument);
                assert_eq!(
                    read(std::ptr::null_mut(), 0, &mut out),
                    Status::InvalidArgument
                );
                (*w).gameplay_allowed = false;
                assert_eq!(read(w, 0, &mut out), Status::Unsupported);
                (*w).gameplay_allowed = true;
                let seq = begin(w);
                assert_eq!(exchange(w, seq, &spawn_request()), Status::Ok);
                assert_eq!(read(w, 0, &mut out), Status::CommandsRejected);
                assert_eq!(end(w, seq, 1), Status::CommandsRejected);
                assert_eq!((*w).inner.live_entities(), 0);
                assert_eq!(read(w, 0, &mut out), Status::Ok);
                (*w).inner.poison();
                assert_eq!(read(w, 0, &mut out), Status::Poisoned);
                fynkel_world_destroy(w);
            }
        }
    }

    #[test]
    fn lifecycle_snapshot_checks_nulls_and_rejects_active_session_stickily() {
        unsafe {
            let w = fixture();
            let mut out = FynkelBufferView::empty();
            assert_eq!(
                fynkel_world_gameplay_state(w, std::ptr::null_mut()),
                Status::InvalidArgument
            );
            assert_eq!(
                fynkel_world_gameplay_state(std::ptr::null_mut(), &mut out),
                Status::InvalidArgument
            );
            let seq = begin(w);
            assert_eq!(exchange(w, seq, &spawn_request()), Status::Ok);
            assert_eq!(
                fynkel_world_gameplay_state(w, &mut out),
                Status::CommandsRejected
            );
            assert_eq!(end(w, seq, 1), Status::CommandsRejected);
            assert_eq!((*w).inner.live_entities(), 0);
            fynkel_world_destroy(w);
        }
    }
    #[test]
    fn physics_queries_preserve_the_owned_session_and_fail_stickily() {
        unsafe {
            let w = fixture();
            let seq = begin(w);
            assert_eq!(exchange(w, seq, &spawn_request()), Status::Ok);
            let bytes = buffer::empty(BufferKind::PhysicsQueries);
            let mut out = FynkelBufferView::empty();
            assert_eq!(
                fynkel_world_physics_query(w, bytes.as_ptr(), bytes.len() as u32, &mut out),
                Status::Ok
            );
            assert_eq!((*w).inner.live_entities(), 0);
            assert_eq!(end(w, seq, 1), Status::Ok);
            assert_eq!((*w).inner.live_entities(), 1);
            let seq = begin(w);
            assert_eq!(exchange(w, seq, &spawn_request()), Status::Ok);
            let bad = [0_u8];
            assert_eq!(
                fynkel_world_physics_query(w, bad.as_ptr(), 1, &mut out),
                Status::Protocol
            );
            assert_eq!(end(w, seq, 1), Status::CommandsRejected);
            assert_eq!((*w).inner.live_entities(), 1);
            fynkel_world_destroy(w);
        }
    }
    #[test]
    fn controller_parameter_read_failure_aborts_prior_writes() {
        unsafe {
            let w = fixture();
            let seq = begin(w);
            assert_eq!(exchange(w, seq, &spawn_request()), Status::Ok);
            let mut request = Vec::new();
            put32(&mut request, 1);
            put32(&mut request, 23);
            request.extend((*w).gameplay_reply.clone());
            put32(&mut request, 5);
            request.extend(b"speed");
            assert_eq!(exchange(w, seq, &request), Status::CommandsRejected);
            assert_eq!(end(w, seq, 1), Status::CommandsRejected);
            assert_eq!((*w).inner.live_entities(), 0);
            fynkel_world_destroy(w);
        }
    }
    #[test]
    fn malformed_animation_configuration_aborts_prior_writes() {
        unsafe {
            let w = fixture();
            for variant in 0..5 {
                let seq = begin(w);
                assert_eq!(exchange(w, seq, &spawn_request()), Status::Ok);
                let mut request = Vec::new();
                put32(&mut request, 1);
                put32(&mut request, 22);
                request.extend((*w).gameplay_reply.clone());
                let mut body = vec![0; 88];
                body[16..20]
                    .copy_from_slice(&(fynkel_core::schema::AnimateOp::Pause as u32).to_le_bytes());
                if variant == 0 {
                    body[0] = 1;
                }
                if variant == 1 {
                    body[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
                }
                if variant == 2 {
                    body.pop();
                }
                if variant == 4 {
                    body[16..20].copy_from_slice(
                        &(fynkel_core::schema::AnimateOp::PlayState as u32).to_le_bytes(),
                    );
                    body[72..76].copy_from_slice(&(-2_i32).to_le_bytes());
                }
                request.extend(body);
                put32(&mut request, 0);
                assert_eq!(
                    exchange(w, seq, &request),
                    if variant == 3 {
                        Status::CommandsRejected
                    } else {
                        Status::Protocol
                    }
                );
                assert_eq!(end(w, seq, 1), Status::CommandsRejected);
                assert_eq!((*w).inner.live_entities(), 0);
            }
            fynkel_world_destroy(w);
        }
    }
    #[test]
    fn immediate_sound_protocol_and_edit_exclusion_preserve_pending_data() {
        unsafe {
            let w = fixture();
            let record = fynkel_core::schema::PlaySoundCommandRecord {
                clip: fynkel_core::AssetRef::default(),
                bus: fynkel_core::AudioBus::Sfx,
                volume: 1.0,
                pitch: 1.0,
                priority: 0,
            };
            let mut body = vec![0; 40];
            record.encode(&mut body, &mut StringTable::new());
            assert_eq!(
                fynkel_world_audio_play(std::ptr::null_mut(), body.as_ptr(), 40),
                Status::InvalidArgument
            );
            assert_eq!(
                fynkel_world_audio_play(w, std::ptr::null(), 40),
                Status::InvalidArgument
            );
            assert_eq!(
                fynkel_world_audio_play(w, body.as_ptr(), 39),
                Status::InvalidArgument
            );
            let mut invalid = body.clone();
            invalid[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
            assert_eq!(
                fynkel_world_audio_play(w, invalid.as_ptr(), 40),
                Status::Protocol
            );
            let seq = begin(w);
            assert_eq!(exchange(w, seq, &spawn_request()), Status::Ok);
            assert_eq!(
                fynkel_world_audio_play(w, body.as_ptr(), 40),
                Status::CommandsRejected
            );
            assert_eq!(end(w, seq, 1), Status::CommandsRejected);
            assert_eq!((*w).inner.live_entities(), 0);
            assert_eq!((*w).inner.audio_stats().started, 0);
            assert_eq!((*w).inner.tick(), 0);
            assert_eq!(
                fynkel_world_audio_play(w, body.as_ptr(), 40),
                Status::CommandsRejected
            );
            assert_eq!(
                (*w).inner.diagnostics().last().unwrap().code,
                fynkel_contracts::DiagnosticCode::SOUND_UNAVAILABLE
            );
            fynkel_world_destroy(w);
        }
    }

    #[test]
    fn malformed_audio_controls_fail_stickily() {
        unsafe {
            let w = fixture();
            for variant in 0..4 {
                let seq = begin(w);
                assert_eq!(exchange(w, seq, &spawn_request()), Status::Ok);
                let mut request = Vec::new();
                put32(&mut request, 1);
                put32(&mut request, 21);
                request.extend((*w).gameplay_reply.clone());
                let mut body = vec![0; 24];
                let record = fynkel_core::schema::AudioCommandRecord {
                    entity: if variant == 0 {
                        Entity::new((*w).inner.id(), 0, 1)
                    } else {
                        Entity::NULL
                    },
                    op: fynkel_core::schema::AudioOp::Play,
                    value: 0.0,
                };
                record.encode(&mut body, &mut StringTable::new());
                if variant == 1 {
                    body[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
                }
                if variant == 2 {
                    body.pop();
                }
                request.extend(body);
                assert_eq!(
                    exchange(w, seq, &request),
                    if variant == 3 {
                        Status::CommandsRejected
                    } else {
                        Status::Protocol
                    }
                );
                let mut out = FynkelBufferView::empty();
                assert_eq!(
                    fynkel_world_gameplay_end(w, seq, 1, &mut out),
                    Status::CommandsRejected
                );
                assert_eq!((*w).inner.live_entities(), 0);
            }
            fynkel_world_destroy(w);
        }
    }
    #[test]
    fn malformed_sound_records_abort_prior_writes() {
        unsafe {
            let w = fixture();
            for variant in 0..3 {
                let seq = begin(w);
                assert_eq!(exchange(w, seq, &spawn_request()), Status::Ok);
                let mut request = Vec::new();
                put32(&mut request, 1);
                put32(&mut request, 20);
                let mut body = vec![0; 40];
                body[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
                if variant == 0 {
                    body.truncate(39);
                }
                request.extend(body);
                if variant != 0 {
                    put32(&mut request, if variant == 2 { 100 } else { 0 });
                }
                assert_eq!(exchange(w, seq, &request), Status::Protocol);
                let mut out = FynkelBufferView::empty();
                assert_eq!(
                    fynkel_world_gameplay_end(w, seq, 1, &mut out),
                    Status::CommandsRejected
                );
                assert_eq!((*w).inner.live_entities(), 0);
            }
            fynkel_world_destroy(w);
        }
    }
    #[test]
    fn malformed_body_effect_owners_and_records_abort_the_whole_session() {
        unsafe {
            let w = fixture();
            for variant in 0..4 {
                let seq = begin(w);
                assert_eq!(exchange(w, seq, &spawn_request()), Status::Ok);
                let owner = (*w).gameplay_reply.clone();
                let mut request = Vec::new();
                put32(&mut request, 1);
                put32(&mut request, 19);
                request.extend(owner);
                let mut body = vec![0; 48];
                let record = fynkel_core::schema::BodyCommandRecord {
                    entity: if variant == 0 {
                        Entity::new((*w).inner.id(), 0, 1)
                    } else {
                        Entity::NULL
                    },
                    op: fynkel_core::schema::BodyOp::ApplyImpulse,
                    vector: glam::Vec3::X,
                    rotation: glam::Quat::IDENTITY,
                };
                record.encode(&mut body, &mut StringTable::new());
                if variant == 1 {
                    body[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
                }
                request.extend(body);
                if variant == 2 {
                    request.pop();
                }
                assert_eq!(
                    exchange(w, seq, &request),
                    if variant == 3 {
                        Status::CommandsRejected
                    } else {
                        Status::Protocol
                    }
                );
                assert_ne!(end(w, seq, 1), Status::Ok);
                assert_eq!((*w).inner.live_entities(), 0);
            }
            fynkel_world_destroy(w);
        }
    }
}
