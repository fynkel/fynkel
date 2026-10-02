//! Registered component data in the existing ECS and shared gameplay journal.
//! Flat scalar/text/asset layouts and runtime-managed nullable object references
//! use immutable world schemas. Native codecs use direct typed ECS storage;
//! runtime-defined layouts need no native rebuild or Dart sidecar authority.
use std::any::{Any, TypeId};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn identifier_word(s: &str) -> bool {
    let mut bytes = s.bytes();
    bytes
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
        && bytes.all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

use crate::{Entity, World};
use fynkel_ecs::World as Ecs;

#[derive(Clone, Debug, PartialEq)]
pub enum ScalarValue {
    Float(f64),
    Int(i64),
    Bool(bool),
    Text(String),
    Object(Option<crate::GameplayEntity>),
    Asset(crate::AssetRef),
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

/// Runtime-defined data is an ECS component, destroyed with its owner. It is
/// never mirrored in a Dart entity store. Keys refer to immutable world schemas.
#[derive(Default)]
pub(crate) struct DynamicPresence;

#[derive(Default)]
struct DynamicComponents(BTreeMap<String, Vec<ScalarValue>>);

type BoxedValue = Box<dyn Any + Send + Sync>;
type PrepareValue = fn(&[ScalarValue]) -> Result<BoxedValue, String>;
type ReadValue = fn(&Ecs, Entity) -> Option<Vec<ScalarValue>>;
pub(crate) struct PreparedRecord {
    pub fields: Vec<ScalarValue>,
    value: Option<BoxedValue>,
    insert: Option<fn(&mut Ecs, Entity, BoxedValue)>,
}
impl PreparedRecord {
    pub fn publish(
        mut self,
        ecs: &mut Ecs,
        entity: Entity,
        id: String,
        resolve: impl Fn(crate::GameplayEntity) -> Option<Entity>,
    ) {
        if let (Some(insert), Some(value)) = (self.insert, self.value) {
            insert(ecs, entity, value);
        } else {
            for value in &mut self.fields {
                if let ScalarValue::Object(target) = value {
                    *target = target.and_then(&resolve).map(Into::into);
                }
            }
            if !ecs.has::<DynamicComponents>(entity) {
                ecs.insert(entity, DynamicComponents::default())
                    .expect("validated owner");
            }
            let added = ecs
                .get_mut::<DynamicComponents>(entity)
                .expect("dynamic storage")
                .0
                .insert(id, self.fields)
                .is_none();
            if added {
                ecs.insert(entity, DynamicPresence).expect("dynamic owner");
            }
        }
    }
}
#[derive(Clone)]
pub(crate) struct Registration {
    pub schema: ComponentSchema,
    native_type: Option<TypeId>,
    prepare: Option<PrepareValue>,
    read: Option<ReadValue>,
    remove: Option<fn(&mut Ecs, Entity)>,
    insert: Option<fn(&mut Ecs, Entity, BoxedValue)>,
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
        let value = self
            .prepare
            .map(|prepare| prepare(&fields))
            .transpose()
            .map_err(|why| RegistryError::InvalidValue(format!("{}: {why}", self.schema.id)))?;
        Ok(PreparedRecord {
            fields,
            value,
            insert: self.insert,
        })
    }
    pub fn read(&self, ecs: &Ecs, entity: Entity) -> Option<Vec<ScalarValue>> {
        if let Some(read) = self.read {
            read(ecs, entity)
        } else {
            ecs.get::<DynamicComponents>(entity)
                .ok()
                .and_then(|v| v.0.get(&self.schema.id).cloned())
        }
    }
    pub fn remove(&self, ecs: &mut Ecs, entity: Entity) {
        if let Some(remove) = self.remove {
            remove(ecs, entity);
        } else {
            let removed = ecs
                .get_mut::<DynamicComponents>(entity)
                .ok()
                .is_some_and(|mut values| values.0.remove(&self.schema.id).is_some());
            if removed {
                ecs.insert(entity, DynamicPresence).expect("dynamic owner");
            }
        }
    }
}
#[derive(Clone, Default)]
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
            .find(|r| r.native_type == Some(TypeId::of::<T>()))
            .ok_or_else(|| RegistryError::Unregistered(std::any::type_name::<T>().into()))
    }
    pub fn schemas(&self) -> impl Iterator<Item = &ComponentSchema> {
        self.entries.values().map(|r| &r.schema)
    }
    fn validate_schema(schema: &ComponentSchema) -> Result<(), RegistryError> {
        let mut names = BTreeSet::new();
        if schema.version == 0
            || !schema.id.contains('.')
            || schema.id.starts_with("fynkel.")
            || !schema.id.split('.').all(identifier_word)
            || schema.fields.is_empty()
            || schema
                .fields
                .iter()
                .any(|f| !identifier_word(&f.name) || !names.insert(&f.name) || !f.default.valid())
        {
            return Err(RegistryError::InvalidSchema(schema.id.clone()));
        }
        if schema
            .fields
            .iter()
            .any(|f| matches!(f.default, ScalarValue::Object(Some(_))))
        {
            return Err(RegistryError::InvalidSchema(schema.id.clone()));
        }
        Ok(())
    }
    fn register_dynamic(&mut self, schema: ComponentSchema) -> Result<(), RegistryError> {
        Self::validate_schema(&schema)?;
        if let Some(old) = self.entries.get(&schema.id) {
            return if old.schema == schema {
                Ok(())
            } else {
                Err(RegistryError::IdentityConflict(schema.id))
            };
        }
        self.entries.insert(
            schema.id.clone(),
            Registration {
                schema,
                native_type: None,
                prepare: None,
                read: None,
                remove: None,
                insert: None,
            },
        );
        Ok(())
    }
    pub(crate) fn register<T: RegisteredComponent>(
        &mut self,
        schema: ComponentSchema,
    ) -> Result<(), RegistryError> {
        Self::validate_schema(&schema)?;
        // Native value codecs cannot carry unresolved references. Managed references
        // currently use runtime layouts, whose publication remaps without a codec.
        if schema
            .fields
            .iter()
            .any(|f| matches!(f.default, ScalarValue::Object(_)))
        {
            return Err(RegistryError::InvalidSchema(schema.id));
        }
        if let Some(old) = self.entries.get(&schema.id) {
            return if old.native_type == Some(TypeId::of::<T>()) && old.schema == schema {
                Ok(())
            } else {
                Err(RegistryError::IdentityConflict(schema.id))
            };
        }
        if self
            .entries
            .values()
            .any(|r| r.native_type == Some(TypeId::of::<T>()))
        {
            return Err(RegistryError::IdentityConflict(schema.id));
        }
        let registration = Registration {
            schema,
            native_type: Some(TypeId::of::<T>()),
            prepare: Some(|fields| {
                let value = T::decode(fields)?;
                if value.encode() != fields {
                    return Err("codec does not round-trip the schema values".into());
                }
                Ok(Box::new(value))
            }),
            read: Some(|ecs, e| ecs.get::<T>(e).ok().map(|value| value.encode())),
            remove: Some(|ecs, e| {
                if ecs.has::<T>(e) {
                    ecs.remove::<T>(e).expect("registered component");
                }
            }),
            insert: Some(|ecs, e, value| {
                let value = *value
                    .downcast::<T>()
                    .unwrap_or_else(|_| panic!("registered native type mismatch"));
                ecs.insert(e, value).expect("validated registered owner");
            }),
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
    /// Committed registered data for native hosts. Gameplay callbacks use the
    /// journal's accessor instead. Destroyed object references read as null.
    pub fn registered_values(
        &self,
        entity: Entity,
        id: &str,
    ) -> Result<Option<Vec<ScalarValue>>, RegistryError> {
        if !self.ecs().contains(entity) {
            return Err(RegistryError::InvalidValue("stale registered owner".into()));
        }
        Ok(self
            .gameplay
            .registered
            .get(id)?
            .read(self.ecs(), entity)
            .map(|mut fields| {
                for field in &mut fields {
                    if let ScalarValue::Object(target) = field {
                        if target.is_some_and(|t| t.live().is_none_or(|e| !self.ecs().contains(e)))
                        {
                            *target = None;
                        }
                    }
                }
                fields
            }))
    }
    /// Runtime schema registration for Dart and other hosts. No native rebuild.
    pub fn register_dynamic_component(
        &mut self,
        schema: ComponentSchema,
    ) -> Result<(), RegistryError> {
        if self.is_poisoned() {
            return Err(RegistryError::InvalidSchema("poisoned world".into()));
        }
        self.gameplay.registered.register_dynamic(schema)
    }

    /// Registration is a host setup operation, outside an owned gameplay edit.
    /// Exact repeats are idempotent; an ID/version/layout or Rust-type conflict
    /// is rejected without replacing the existing registration or data.
    pub fn register_component<T: RegisteredComponent>(
        &mut self,
        schema: ComponentSchema,
    ) -> Result<(), RegistryError> {
        if self.is_poisoned() {
            return Err(RegistryError::InvalidSchema("poisoned world".into()));
        }
        self.gameplay.registered.register::<T>(schema)
    }
    pub fn registered_schemas(&self) -> impl Iterator<Item = &ComponentSchema> {
        self.gameplay.registered.schemas()
    }
}

/// Authored references use stable document IDs until scene activation resolves
/// them. Runtime handles are retained only for in-memory subtree descriptions.
#[derive(Clone, Debug, PartialEq)]
pub enum AuthoredRegisteredValue {
    Scalar(ScalarValue),
    Object(fynkel_contracts::ObjectRef),
}
#[derive(Clone, Debug, PartialEq)]
pub struct RegisteredDescription {
    pub id: String,
    pub version: u32,
    pub fields: BTreeMap<String, AuthoredRegisteredValue>,
}
