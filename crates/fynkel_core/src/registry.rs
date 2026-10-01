//! F3 registered value components: native ECS storage, schema-owned scalar data.
//! The first slice supports flat finite f64, i64 and bool fields. Reference and
//! collection layouts, authored migration and the host bridge are not supported
//! yet. No registered type is added to the built-in ComponentValue catalogue.
use std::any::{Any, TypeId};
use std::collections::{BTreeMap, BTreeSet};

use crate::{Entity, World};
use fynkel_ecs::World as Ecs;

#[derive(Clone, Debug, PartialEq)]
pub enum ScalarValue {
    Float(f64),
    Int(i64),
    Bool(bool),
}
impl ScalarValue {
    fn valid(&self) -> bool {
        !matches!(self, Self::Float(v) if !v.is_finite())
    }
    fn same_kind(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RegisteredField {
    pub name: String,
    /// Defines both the scalar kind and the default. Defaults are validated by
    /// the component's codec when registering the complete schema.
    pub default: ScalarValue,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ComponentSchema {
    /// Stable, case-sensitive namespaced identity, e.g. `game.Health`.
    pub id: String,
    pub version: u32,
    pub fields: Vec<RegisteredField>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryError {
    InvalidSchema(String),
    IdentityConflict(String),
    Unregistered(String),
    InvalidValue(String),
}
impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "component registry: {self:?}")
    }
}
impl std::error::Error for RegistryError {}

/// A native value component, implemented by a game/package (eventually generated).
/// Codecs must be pure and round-trip all component data. Service handles,
/// shared mutable objects and external side effects are outside this contract.
/// Values are reconstructed before staging, never through a fallible codec at
/// commit. Committed T lives directly in the existing ECS's typed storage.
pub trait RegisteredComponent: Send + Sync + 'static + Sized {
    fn encode(&self) -> Vec<ScalarValue>;
    fn decode(fields: &[ScalarValue]) -> Result<Self, String>;
}

type BoxedValue = Box<dyn Any + Send + Sync>;
pub(crate) struct PreparedRecord {
    pub fields: Vec<ScalarValue>,
    value: BoxedValue,
    insert: fn(&mut Ecs, Entity, BoxedValue),
}
impl PreparedRecord {
    pub fn publish(self, ecs: &mut Ecs, entity: Entity) {
        (self.insert)(ecs, entity, self.value);
    }
}
pub(crate) struct Registration {
    pub schema: ComponentSchema,
    native_type: TypeId,
    prepare: fn(&[ScalarValue]) -> Result<BoxedValue, String>,
    read: fn(&Ecs, Entity) -> Option<Vec<ScalarValue>>,
    remove: fn(&mut Ecs, Entity),
    insert: fn(&mut Ecs, Entity, BoxedValue),
}
impl Registration {
    pub fn prepare(&self, fields: Vec<ScalarValue>) -> Result<PreparedRecord, RegistryError> {
        if fields.len() != self.schema.fields.len()
            || fields
                .iter()
                .zip(&self.schema.fields)
                .any(|(v, f)| !v.valid() || !v.same_kind(&f.default))
        {
            return Err(RegistryError::InvalidValue(format!(
                "{}: field count, kind or finite value",
                self.schema.id
            )));
        }
        let value = (self.prepare)(&fields)
            .map_err(|why| RegistryError::InvalidValue(format!("{}: {why}", self.schema.id)))?;
        Ok(PreparedRecord {
            fields,
            value,
            insert: self.insert,
        })
    }
    pub fn read(&self, ecs: &Ecs, entity: Entity) -> Option<Vec<ScalarValue>> {
        (self.read)(ecs, entity)
    }
    pub fn remove(&self, ecs: &mut Ecs, entity: Entity) {
        (self.remove)(ecs, entity);
    }
}
#[derive(Default)]
pub(crate) struct ComponentRegistry {
    entries: BTreeMap<String, Registration>,
}
impl ComponentRegistry {
    pub fn get(&self, id: &str) -> Result<&Registration, RegistryError> {
        self.entries
            .get(id)
            .ok_or_else(|| RegistryError::Unregistered(id.into()))
    }
    pub fn typed<T: RegisteredComponent>(&self) -> Result<&Registration, RegistryError> {
        self.entries
            .values()
            .find(|r| r.native_type == TypeId::of::<T>())
            .ok_or_else(|| RegistryError::Unregistered(std::any::type_name::<T>().into()))
    }
    pub fn schemas(&self) -> impl Iterator<Item = &ComponentSchema> {
        self.entries.values().map(|r| &r.schema)
    }
    fn register<T: RegisteredComponent>(
        &mut self,
        schema: ComponentSchema,
    ) -> Result<(), RegistryError> {
        fn word(s: &str) -> bool {
            let mut bytes = s.bytes();
            bytes
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
                && bytes.all(|c| c.is_ascii_alphanumeric() || c == b'_')
        }
        let mut names = BTreeSet::new();
        if schema.version == 0
            || !schema.id.contains('.')
            || schema.id.starts_with("fynkel.")
            || !schema.id.split('.').all(word)
            || schema.fields.is_empty()
            || schema
                .fields
                .iter()
                .any(|f| !word(&f.name) || !names.insert(&f.name) || !f.default.valid())
        {
            return Err(RegistryError::InvalidSchema(schema.id));
        }
        if let Some(old) = self.entries.get(&schema.id) {
            return if old.native_type == TypeId::of::<T>() && old.schema == schema {
                Ok(())
            } else {
                Err(RegistryError::IdentityConflict(schema.id))
            };
        }
        if self
            .entries
            .values()
            .any(|r| r.native_type == TypeId::of::<T>())
        {
            return Err(RegistryError::IdentityConflict(schema.id));
        }
        let registration = Registration {
            schema,
            native_type: TypeId::of::<T>(),
            prepare: |fields| {
                let value = T::decode(fields)?;
                if value.encode() != fields {
                    return Err("codec does not round-trip the schema values".into());
                }
                Ok(Box::new(value))
            },
            read: |ecs, e| ecs.get::<T>(e).ok().map(|value| value.encode()),
            remove: |ecs, e| {
                if ecs.has::<T>(e) {
                    ecs.remove::<T>(e).expect("registered component");
                }
            },
            insert: |ecs, e, value| {
                let value = *value
                    .downcast::<T>()
                    .unwrap_or_else(|_| panic!("registered native type mismatch"));
                ecs.insert(e, value).expect("validated registered owner");
            },
        };
        let defaults = registration
            .schema
            .fields
            .iter()
            .map(|f| f.default.clone())
            .collect();
        registration.prepare(defaults)?;
        self.entries
            .insert(registration.schema.id.clone(), registration);
        Ok(())
    }
}
impl World {
    /// Registration is a host setup operation, outside an owned gameplay edit.
    /// Exact repeats are idempotent; an ID/version/layout or Rust-type conflict
    /// is rejected without replacing the existing registration or data.
    pub fn register_component<T: RegisteredComponent>(
        &mut self,
        schema: ComponentSchema,
    ) -> Result<(), RegistryError> {
        self.registered.register::<T>(schema)
    }
    pub fn registered_schemas(&self) -> impl Iterator<Item = &ComponentSchema> {
        self.registered.schemas()
    }
}
