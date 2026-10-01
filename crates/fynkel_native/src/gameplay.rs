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
    session: Option<GameplaySession>,
    prepared: Vec<fynkel_assets::PreparedPrefab>,
    resources: fynkel_assets::PreparedGameplayResources,
    published: Vec<fynkel_assets::PublishedPrefab>,
    pending_bindings: Option<PendingBindings>,
}
impl WorldSlot {
    pub fn new(world: World) -> Self {
        Self {
            world: Some(world),
            session: None,
            prepared: Vec::new(),
            resources: Default::default(),
            published: Vec::new(),
            pending_bindings: None,
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
        self.prepared.clear();
        self.resources = Default::default();
        self.pending_bindings = None;
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
/// Atomically adopts the native simulation queue before opening a callback edit.
/// Success: u32 spawn count, (u64 temp ID, entity) pairs (NULL if canceled),
/// then u32 live count and (entity, u32 name length, UTF-8 name) pairs.
/// # Safety
/// World must be live or NULL; out must be writable or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_gameplay_adopt(
    world: *mut FynkelWorld,
    out: *mut FynkelBufferView,
) -> Status {
    if let Some(view) = out.as_mut() {
        *view = FynkelBufferView::empty();
    }
    let status = boundary(world, |w| {
        if out.is_null() {
            return reject(w, Status::InvalidArgument, "missing adoption output");
        }
        if !w.gameplay_allowed {
            return reject(w, Status::Unsupported, "gameplay adoption unavailable");
        }
        if w.inner.is_editing() {
            return reject(w, Status::CommandsRejected, "adoption inside an edit");
        }
        let commit = match w.inner.adopt_gameplay_commands() {
            Ok(commit) => commit,
            Err(error) => return reject(w, Status::CommandsRejected, error),
        };
        let mut reply = Vec::new();
        put32(&mut reply, commit.value.len() as u32);
        for (id, pending) in &commit.value {
            put64(&mut reply, *id);
            entity(&mut reply, commit.resolve(*pending).unwrap_or(Entity::NULL));
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
            let Some(assets) = &w.assets else {
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
        if let Request::BindActions(rebinds) = request {
            return bind_actions(w, rebinds);
        }
        if let Request::Instantiate(id, transform, parent, name) = request {
            return instantiate(w, id, transform, parent, name, sequence);
        }
        if let Some(assets) = &w.assets {
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
        let session = w.inner.session.take().unwrap();
        let bindings = w.inner.pending_bindings.take();
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
                if let Some(bindings) = bindings {
                    w.rebinds = bindings.rebinds;
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
    let Some(assets) = w.assets.clone() else {
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
    let Some(assets) = w.assets.clone() else {
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
            _ => return Err("unknown operation"),
        })
    }
}
enum Request {
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
            Self::Instantiate(..) | Self::AssetJson(..) | Self::BindActions(..) => {
                return Err(EditError::Aborted)
            }
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
impl Answer {
    fn encode(self, seq: u64, world: u64, pending: &mut Vec<PendingEntity>) -> Vec<u8> {
        let mut b = Vec::new();
        match self {
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
                assert!((*w).rebinds.is_empty());
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
    fn adoption_report_resolves_live_and_canceled_spawns_without_replay() {
        unsafe {
            let w = fixture();
            (*w).inner.submit(
                fynkel_core::Lane::Simulation,
                vec![
                    fynkel_core::Command::Spawn {
                        temp_id: 3,
                        transform: Default::default(),
                        parent: None,
                        name: Some("kept".into()),
                    },
                    fynkel_core::Command::Spawn {
                        temp_id: 4,
                        transform: Default::default(),
                        parent: None,
                        name: None,
                    },
                    fynkel_core::Command::Destroy(Entity::temp(4)),
                ],
            );
            let mut out = FynkelBufferView::empty();
            assert_eq!(fynkel_world_gameplay_adopt(w, &mut out), Status::Ok);
            let bytes = &(*w).gameplay_reply;
            assert_eq!(u32::from_le_bytes(bytes[0..4].try_into().unwrap()), 2);
            assert_eq!(u64::from_le_bytes(bytes[4..12].try_into().unwrap()), 3);
            assert!(bytes[12..28].iter().any(|b| *b != 0));
            assert_eq!(u64::from_le_bytes(bytes[28..36].try_into().unwrap()), 4);
            assert_eq!(&bytes[36..52], &[0; 16]);
            assert_eq!(u32::from_le_bytes(bytes[52..56].try_into().unwrap()), 1);
            assert_eq!(&bytes[76..], b"kept");
            assert_eq!((*w).inner.tick(), 0);
            (*w).inner.step();
            assert_eq!((*w).inner.live_entities(), 1);
            assert!(!(*w)
                .inner
                .drain_events()
                .iter()
                .any(|e| matches!(e.payload, fynkel_core::EventPayload::Spawned { .. })));
            fynkel_world_destroy(w);
        }
    }

    #[test]
    fn adoption_rejection_preserves_queue_and_active_session_is_sticky() {
        unsafe {
            let w = fixture();
            (*w).inner.submit(
                fynkel_core::Lane::Simulation,
                vec![fynkel_core::Command::SetName {
                    entity: Entity::NULL,
                    name: "invalid".into(),
                }],
            );
            let mut out = FynkelBufferView::empty();
            assert_eq!(
                fynkel_world_gameplay_adopt(w, std::ptr::null_mut()),
                Status::InvalidArgument
            );
            for _ in 0..2 {
                assert_eq!(
                    fynkel_world_gameplay_adopt(w, &mut out),
                    Status::CommandsRejected
                );
            }
            let report = (*w).inner.step();
            assert_eq!(report.rejected, 1);
            let seq = begin(w);
            assert_eq!(exchange(w, seq, &spawn_request()), Status::Ok);
            assert_eq!(
                fynkel_world_gameplay_adopt(w, &mut out),
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
