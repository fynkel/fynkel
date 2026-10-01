//! Test client for the real owned C gameplay protocol; no command queue or emulator.
#![allow(dead_code)]
use fynkel_contracts::{ComponentId, Entity, Status};
use fynkel_native::gameplay::{
    fynkel_world_gameplay_begin, fynkel_world_gameplay_end, fynkel_world_gameplay_exchange,
};
use fynkel_native::{FynkelBufferView, FynkelWorld};
use fynkel_scene::Transform;
use fynkel_wire::{get_entity, get_u32, StringTable, WireRecord};
use std::collections::BTreeMap;

fn empty_view() -> FynkelBufferView {
    FynkelBufferView {
        data: std::ptr::null(),
        length: 0,
    }
}

pub type Begin = unsafe extern "C" fn(*mut FynkelWorld, *mut u64) -> Status;
pub type Exchange =
    unsafe extern "C" fn(*mut FynkelWorld, u64, *const u8, u32, *mut FynkelBufferView) -> Status;
pub type End = unsafe extern "C" fn(*mut FynkelWorld, u64, u32, *mut FynkelBufferView) -> Status;
#[derive(Clone, Copy)]
pub struct Api {
    pub begin: Begin,
    pub exchange: Exchange,
    pub end: End,
}
impl Default for Api {
    fn default() -> Self {
        Self {
            begin: fynkel_world_gameplay_begin,
            exchange: fynkel_world_gameplay_exchange,
            end: fynkel_world_gameplay_end,
        }
    }
}
#[derive(Clone, Copy)]
pub struct Reference(pub [u8; 32]);
impl Reference {
    pub fn live(entity: Entity) -> Self {
        let mut bytes = [0; 32];
        bytes[..4].copy_from_slice(&1u32.to_le_bytes());
        bytes[8..16].copy_from_slice(&entity.world().0.to_le_bytes());
        bytes[16..20].copy_from_slice(&entity.slot().to_le_bytes());
        bytes[20..24].copy_from_slice(&entity.generation().to_le_bytes());
        Self(bytes)
    }
}
pub struct Commit {
    entities: BTreeMap<u32, Entity>,
}
impl Commit {
    pub fn resolve(&self, reference: Reference) -> Entity {
        assert_eq!(get_u32(&reference.0, 0), 2);
        self.entities[&get_u32(&reference.0, 4)]
    }
}
pub struct Edit {
    world: *mut FynkelWorld,
    sequence: u64,
    api: Api,
    ended: bool,
}
impl Edit {
    pub fn new(world: *mut FynkelWorld) -> Self {
        Self::with_api(world, Api::default())
    }
    pub fn with_api(world: *mut FynkelWorld, api: Api) -> Self {
        let mut sequence = 0;
        assert_eq!(unsafe { (api.begin)(world, &mut sequence) }, Status::Ok);
        Self {
            world,
            sequence,
            api,
            ended: false,
        }
    }
    pub fn request_status(&mut self, opcode: u32, payload: &[u8]) -> (Status, Vec<u8>) {
        let mut bytes = Vec::from(1u32.to_le_bytes());
        bytes.extend(opcode.to_le_bytes());
        bytes.extend(payload);
        let mut out = empty_view();
        let status = unsafe {
            (self.api.exchange)(
                self.world,
                self.sequence,
                bytes.as_ptr(),
                bytes.len() as u32,
                &mut out,
            )
        };
        let reply = if out.length == 0 {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(out.data, out.length as usize) }.to_vec()
        };
        (status, reply)
    }
    fn request(&mut self, opcode: u32, payload: &[u8]) -> Vec<u8> {
        let (status, reply) = self.request_status(opcode, payload);
        assert_eq!(status, Status::Ok, "{}", String::from_utf8_lossy(&reply));
        reply
    }
    pub fn try_destroy(&mut self, entity: Entity) -> Status {
        self.request_status(2, &Reference::live(entity).0).0
    }

    pub fn spawn(
        &mut self,
        name: &str,
        transform: Transform,
        parent: Option<Reference>,
    ) -> Reference {
        let mut body = vec![0; 40];
        transform.encode(&mut body, &mut StringTable::new());
        body.extend(parent.map_or([0; 32], |reference| reference.0));
        body.extend((name.len() as u32).to_le_bytes());
        body.extend(name.as_bytes());
        Reference(self.request(1, &body).try_into().unwrap())
    }
    pub fn set_transform(&mut self, entity: Entity, transform: Transform) {
        let mut body = Vec::from(Reference::live(entity).0);
        body.extend(ComponentId::TRANSFORM.0.to_le_bytes());
        body.extend(40u32.to_le_bytes());
        let mut pose = vec![0; 40];
        transform.encode(&mut pose, &mut StringTable::new());
        body.extend(pose);
        body.extend(0u32.to_le_bytes()); // String tail length.
        body.extend(0u32.to_le_bytes()); // Reference field count.
        self.request(3, &body);
    }
    pub fn destroy(&mut self, entity: Entity) {
        self.request(2, &Reference::live(entity).0);
    }
    pub fn operation(&mut self, opcode: u32, payload: &[u8]) -> Result<(), Status> {
        let (status, _) = self.request_status(opcode, payload);
        if status == Status::Ok {
            Ok(())
        } else {
            Err(status)
        }
    }
    pub fn component(
        &mut self,
        entity: Entity,
        value: &fynkel_core::ComponentValue,
    ) -> Result<(), Status> {
        let mut value = value.clone();
        let mut references = Vec::new();
        value.for_each_object_ref(|_, _, reference| {
            references.push(reference.get().map_or([0; 32], |e| Reference::live(e).0));
            *reference = fynkel_contracts::ObjectRef::NONE;
        });
        let mut scalar = vec![0; fynkel_wire::component_value_size(&value)];
        let mut strings = StringTable::new();
        fynkel_wire::encode_component_value(&value, &mut scalar, &mut strings);
        let mut payload = Vec::from(Reference::live(entity).0);
        payload.extend(value.id().0.to_le_bytes());
        payload.extend((scalar.len() as u32).to_le_bytes());
        payload.extend(scalar);
        payload.extend((strings.as_bytes().len() as u32).to_le_bytes());
        payload.extend(strings.as_bytes());
        payload.extend((references.len() as u32).to_le_bytes());
        for reference in references {
            payload.extend(reference);
        }
        self.operation(3, &payload)
    }
    pub fn name(&mut self, entity: Entity, name: &str) -> Result<(), Status> {
        let mut payload = Vec::from(Reference::live(entity).0);
        payload.extend((name.len() as u32).to_le_bytes());
        payload.extend(name.as_bytes());
        self.operation(7, &payload)
    }
    pub fn parent(
        &mut self,
        entity: Entity,
        parent: Option<Entity>,
        keep_world: bool,
    ) -> Result<(), Status> {
        let mut payload = Vec::from(Reference::live(entity).0);
        payload.extend(parent.map_or([0; 32], |e| Reference::live(e).0));
        payload.extend((keep_world as u32).to_le_bytes());
        self.operation(9, &payload)
    }
    pub fn remove(&mut self, entity: Entity, component: u32) -> Result<(), Status> {
        let mut payload = Vec::from(Reference::live(entity).0);
        payload.extend(component.to_le_bytes());
        self.operation(15, &payload)
    }
    pub fn body(
        &mut self,
        mut record: fynkel_core::schema::BodyCommandRecord,
    ) -> Result<(), Status> {
        let owner = record.entity;
        record.entity = Entity::NULL;
        self.effect(19, owner, &record, false)
    }
    pub fn animate(
        &mut self,
        mut record: fynkel_core::schema::AnimateCommandRecord,
    ) -> Result<(), Status> {
        let owner = record.entity;
        record.entity = Entity::NULL;
        self.effect(22, owner, &record, true)
    }
    pub fn audio(
        &mut self,
        mut record: fynkel_core::schema::AudioCommandRecord,
    ) -> Result<(), Status> {
        let owner = record.entity;
        record.entity = Entity::NULL;
        self.effect(21, owner, &record, false)
    }
    fn effect<T: WireRecord>(
        &mut self,
        opcode: u32,
        owner: Entity,
        record: &T,
        with_tail: bool,
    ) -> Result<(), Status> {
        let mut payload = Vec::from(Reference::live(owner).0);
        let mut body = vec![0; T::SIZE];
        let mut strings = StringTable::new();
        record.encode(&mut body, &mut strings);
        payload.extend(body);
        if with_tail {
            payload.extend((strings.as_bytes().len() as u32).to_le_bytes());
            payload.extend(strings.as_bytes());
        }
        self.operation(opcode, &payload)
    }
    pub fn bus(
        &mut self,
        bus: fynkel_core::AudioBus,
        volume: f32,
        muted: bool,
    ) -> Result<(), Status> {
        let mut payload = Vec::from((bus as u32).to_le_bytes());
        payload.extend(volume.to_le_bytes());
        payload.extend((muted as u32).to_le_bytes());
        self.operation(25, &payload)
    }
    pub fn finish(mut self) -> Status {
        let status = unsafe { (self.api.end)(self.world, self.sequence, 1, &mut empty_view()) };
        self.ended = true;
        status
    }
    pub fn commit(mut self) -> Commit {
        let mut out = empty_view();
        let status = unsafe { (self.api.end)(self.world, self.sequence, 1, &mut out) };
        self.ended = true;
        assert_eq!(status, Status::Ok);
        let bytes = unsafe { std::slice::from_raw_parts(out.data, out.length as usize) };
        let entities = (0..get_u32(bytes, 0) as usize)
            .map(|i| {
                let offset = 4 + i * 20;
                (get_u32(bytes, offset), get_entity(bytes, offset + 4))
            })
            .collect();
        Commit { entities }
    }
}
impl Drop for Edit {
    fn drop(&mut self) {
        if !self.ended {
            unsafe { (self.api.end)(self.world, self.sequence, 0, &mut empty_view()) };
        }
    }
}
