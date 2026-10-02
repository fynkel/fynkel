//! Scoped native gameplay edits (F2.1 / ADR-0046).
//!
//! The migration covers names, local poses, hierarchy, activity, scoped spawns
//! and subtree destruction, plus all built-in configuration and scoped references.
//! The journal contains only edited fields; the ECS owns committed values.
//! The production wire bridge and callback routing
//! are subsequent F2 work.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};

use crate::{AudioBus, ComponentValue, PreparedInputBindings};
use fynkel_contracts::{ComponentId, Entity, WorldId};
use fynkel_ecs::EcsError;
use fynkel_scene::{self as scene, InactiveSelf, Transform, WorldTransform};
use glam::Mat4;

use super::{Event, EventPayload, Name, World};

/// A prefab staged in a gameplay edit. Handles have the same scope as ordinary
/// pending spawns; use the edit commit to resolve surviving entities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameplayPrefab {
    pub root: PendingEntity,
    /// Authoring ids and pending handles in hierarchy spawn order.
    pub entities: Vec<(fynkel_contracts::Uuid, PendingEntity)>,
}
impl GameplayPrefab {
    pub fn handle_of(&self, id: fynkel_contracts::Uuid) -> Option<PendingEntity> {
        self.entities
            .iter()
            .find_map(|&(key, value)| (key == id).then_some(value))
    }
}

/// A cloned logical subtree. Sources and pending copies are in preorder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameplayClone {
    pub root: PendingEntity,
    pub entities: Vec<(GameplayEntity, PendingEntity)>,
}
impl GameplayClone {
    pub fn handle_of(&self, source: GameplayEntity) -> Option<PendingEntity> {
        self.entities
            .iter()
            .find_map(|&(key, value)| (key == source).then_some(value))
    }
}

/// A pending reference is never a live ECS handle. Private identity fields
/// prevent construction or reuse as another transaction's pending object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PendingEntity {
    world: WorldId,
    transaction: u64,
    index: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GameplayEntity {
    Live(Entity),
    Pending(PendingEntity),
}
impl From<Entity> for GameplayEntity {
    fn from(entity: Entity) -> Self {
        Self::Live(entity)
    }
}
impl From<PendingEntity> for GameplayEntity {
    fn from(entity: PendingEntity) -> Self {
        Self::Pending(entity)
    }
}
impl GameplayEntity {
    pub fn live(self) -> Option<Entity> {
        match self {
            Self::Live(e) => Some(e),
            Self::Pending(_) => None,
        }
    }
}

/// Object references inside a gameplay edit. Stored references retain authoring
/// metadata; pending references can only be resolved by successful publication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameplayObjectRef {
    Stored(fynkel_contracts::ObjectRef),
    Pending(PendingEntity),
}
impl GameplayObjectRef {
    pub const NONE: Self = Self::Stored(fynkel_contracts::ObjectRef::NONE);
    pub fn entity(self) -> Option<GameplayEntity> {
        match self {
            Self::Stored(r) => r.get().map(GameplayEntity::Live),
            Self::Pending(p) => Some(GameplayEntity::Pending(p)),
        }
    }
}
impl From<fynkel_contracts::ObjectRef> for GameplayObjectRef {
    fn from(value: fynkel_contracts::ObjectRef) -> Self {
        Self::Stored(value)
    }
}
impl From<GameplayEntity> for GameplayObjectRef {
    fn from(value: GameplayEntity) -> Self {
        match value {
            GameplayEntity::Live(e) => Self::Stored(fynkel_contracts::ObjectRef::to(e)),
            GameplayEntity::Pending(p) => Self::Pending(p),
        }
    }
}
impl From<Entity> for GameplayObjectRef {
    fn from(value: Entity) -> Self {
        GameplayEntity::Live(value).into()
    }
}
impl From<PendingEntity> for GameplayObjectRef {
    fn from(value: PendingEntity) -> Self {
        Self::Pending(value)
    }
}

macro_rules! scoped_type {
    ($ty:ty, $variant:ident, []) => { $ty };
    ($ty:ty, $variant:ident, [$($field:ident),+]) => { crate::$variant<GameplayObjectRef> };
}
macro_rules! map_component_refs {
    ($value:expr, $map:expr, []) => {
        $value
    };
    ($value:expr, $map:expr, [$($field:ident),+]) => {
        $value.map_object_refs($map)
    };
}
macro_rules! scoped_components {
    ($($(#[$doc:meta])* $variant:ident($ty:ty) = $id:ident [$($field:ident),*];)*) => {
        /// The catalogue's component values with scoped object references.
        /// Configuration fields and validators are shared with stored components.
        /// Scoped reference instantiations deliberately have no native schema:
        /// ```compile_fail
        /// use fynkel_contracts::HasSchema;
        /// use fynkel_core::{Follow, GameplayObjectRef};
        /// let _ = Follow::<GameplayObjectRef>::SCHEMA;
        /// ```
        #[derive(Clone, Debug, PartialEq)]
        pub enum GameplayComponentValue {
            $($variant(scoped_type!($ty, $variant, [$($field),*])),)*
        }
        impl From<ComponentValue> for GameplayComponentValue {
            fn from(value: ComponentValue) -> Self {
                match value {
                    $(ComponentValue::$variant(value) => Self::$variant(
                        map_component_refs!(value, GameplayObjectRef::from, [$($field),*])
                    ),)*
                }
            }
        }
        impl GameplayComponentValue {
            pub fn id(&self) -> ComponentId {
                match self { $(Self::$variant(_) => ComponentId::$id,)* }
            }
            pub fn for_each_reference(&self, mut visit: impl FnMut(GameplayObjectRef)) {
                match self {
                    $(Self::$variant(_value) => { $(visit(_value.$field);)* },)*
                }
                let _ = &mut visit;
            }
            pub fn into_native(self, mut resolve: impl FnMut(GameplayObjectRef) -> fynkel_contracts::ObjectRef) -> ComponentValue {
                match self {
                    $(Self::$variant(value) => ComponentValue::$variant(
                        map_component_refs!(value, &mut resolve, [$($field),*])
                    ),)*
                }
            }
            pub fn map_object_refs(self, mut map: impl FnMut(GameplayObjectRef) -> GameplayObjectRef) -> Self {
                match self {
                    $(Self::$variant(value) => Self::$variant(map_component_refs!(value, &mut map, [$($field),*])),)*
                }
            }
            /// Uses the stored component's scalar validator. Reference validity
            /// is world/transaction-dependent and is checked by set_component.
            pub fn validate(&self) -> Result<(), String> {
                self.clone().into_native(|_| fynkel_contracts::ObjectRef::NONE).validate()
            }
        }
    }
}
crate::builtin_components!(scoped_components);

/// Only successful publication supplies pending-to-live resolutions. A spawn
/// destroyed before commit has no resolution and never allocates a live slot.
#[derive(Debug, PartialEq, Eq)]
pub struct EditCommit<R> {
    pub value: R,
    spawned: BTreeMap<PendingEntity, Entity>,
}
impl<R> EditCommit<R> {
    pub fn resolve(&self, pending: PendingEntity) -> Option<Entity> {
        self.spawned.get(&pending).copied()
    }
    pub fn spawned(&self) -> impl Iterator<Item = (PendingEntity, Entity)> + '_ {
        self.spawned.iter().map(|(&p, &e)| (p, e))
    }
}

/// Owns the world while a host performs several synchronous bridge exchanges.
/// No mutable world access is available until finish/abort returns ownership.
pub struct GameplaySession {
    world: World,
    journal: Option<GameplayJournal>,
}
impl GameplaySession {
    pub fn begin(mut world: World) -> Result<Self, Box<(World, EditError)>> {
        match world.begin_gameplay_journal() {
            Ok(journal) => Ok(Self {
                world,
                journal: Some(journal),
            }),
            Err(error) => Err(Box::new((world, error))),
        }
    }
    /// Committed read-only state, for explicitly named snapshot/diagnostic APIs.
    pub fn sequence(&self) -> u64 {
        self.journal.as_ref().expect("session journal").scope.1
    }
    pub fn committed(&self) -> &World {
        &self.world
    }
    pub fn access<R>(
        &mut self,
        callback: impl FnOnce(&mut GameplayEdit<'_>) -> EditResult<R>,
    ) -> EditResult<R> {
        let mut edit = GameplayEdit {
            world: &self.world,
            journal: self.journal.take().expect("session journal"),
        };
        let result = catch_unwind(AssertUnwindSafe(|| {
            edit.check()?;
            match callback(&mut edit) {
                Ok(value) => {
                    edit.check()?;
                    Ok(value)
                }
                Err(error) => edit.fail(error),
            }
        }));
        if result.is_err() {
            let _: EditResult<()> = edit.fail(EditError::Aborted);
        }
        self.journal = Some(edit.journal);
        match result {
            Ok(result) => result,
            Err(panic) => resume_unwind(panic),
        }
    }
    pub fn abort(self) -> World {
        self.world
    }
    pub fn finish(mut self) -> (World, EditResult<EditCommit<()>>) {
        // Ownership must return to the host even if validation itself panics.
        let validation = match catch_unwind(AssertUnwindSafe(|| {
            self.access(|edit| edit.validate_final_references())
        })) {
            Ok(result) => result,
            Err(_) => {
                self.world.poison();
                return (self.world, Err(EditError::Poisoned));
            }
        };
        if let Err(error) = validation {
            return (self.world, Err(error));
        }
        let journal = self.journal.take().expect("session journal");
        let result = catch_unwind(AssertUnwindSafe(|| self.world.commit_gameplay(journal)));
        let result = match result {
            Ok(result) => result.map(|spawned| EditCommit { value: (), spawned }),
            Err(_) => {
                self.world.poison();
                Err(EditError::Poisoned)
            }
        };
        (self.world, result)
    }
}

/// A frozen membership cursor. Values are read through the current edit;
/// candidates destroyed before visitation are skipped. New spawns require a
/// new query. A cursor cannot be reused in a subsequent edit or another world.
pub struct GameplayQuery {
    scope: (WorldId, u64),
    required: Vec<ComponentId>,
    candidates: std::vec::IntoIter<(GameplayEntity, Vec<u64>)>,
}
impl GameplayQuery {
    pub fn next(&mut self, edit: &GameplayEdit<'_>) -> EditResult<Option<GameplayEntity>> {
        edit.check()?;
        if self.scope != edit.journal.scope {
            return edit.fail(EditError::ExpiredQuery);
        }
        for (entity, revisions) in self.candidates.by_ref() {
            if !edit.contains(entity)? {
                continue;
            }
            let mut matches = true;
            for (&component, revision) in self.required.iter().zip(revisions) {
                if edit.component_revision(entity, component) != revision
                    || !edit.has_component(entity, component)?
                {
                    matches = false;
                    break;
                }
            }
            if matches {
                return Ok(Some(entity));
            }
        }
        Ok(None)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditError {
    Poisoned,
    InvalidPrefab(crate::LoadError),
    UnknownComponent(ComponentId),
    UnsupportedComponent(ComponentId),
    ProtectedComponent(ComponentId),
    MissingComponent(GameplayEntity, ComponentId),
    InvalidComponent {
        entity: GameplayEntity,
        component: ComponentId,
        message: String,
    },
    UnresolvedObjectReference(fynkel_contracts::Uuid),
    IdentityExhausted,
    OutOfEntities,
    ExpiredPending(PendingEntity),
    ExpiredQuery,
    Destroyed(GameplayEntity),
    /// Drain the legacy command boundary before entering the replacement API.
    StaleEntity(Entity),
    MissingTransform(GameplayEntity),
    InvalidTransform(GameplayEntity),
    UnsupportedTransform(GameplayEntity),
    HierarchyCycle(GameplayEntity),
    /// The callback explicitly rejected its data edits.
    Aborted,
    InvalidEffect(GameplayEntity, String),
    RegisteredComponent(crate::RegistryError),
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "gameplay edit: {self:?}")
    }
}
impl std::error::Error for EditError {}

pub type EditResult<T> = Result<T, EditError>;

#[derive(Default)]
struct Changes {
    registered: BTreeMap<String, Option<crate::registry::PreparedRecord>>,
    name: Option<String>,
    transform: Option<Transform>,
    parent: Option<Option<GameplayEntity>>,
    active: Option<bool>,
    components: BTreeMap<ComponentId, Option<GameplayComponentValue>>,
}

/// Access is borrowed for the callback and cannot survive commit or abort.
/// All getters resolve the journal before the committed ECS. Any invalid
/// access/edit permanently fails this transaction, even if its error is caught.
pub struct GameplayEdit<'world> {
    world: &'world World,
    journal: GameplayJournal,
}

pub(super) enum CommittedEffect {
    Body {
        entity: Entity,
        op: crate::BodyCommand,
    },
    Audio {
        entity: Entity,
        op: crate::AudioOp,
    },
    Sound {
        clip: crate::AssetRef,
        params: crate::PlayParams,
    },
    Animation {
        entity: Entity,
        op: crate::AnimateOp,
        before: crate::Animator,
        /// The selected component was removed after this request committed.
        invalidated: bool,
    },
}

impl CommittedEffect {
    pub(super) fn target(&self) -> Option<Entity> {
        match self {
            Self::Body { entity, .. }
            | Self::Audio { entity, .. }
            | Self::Animation { entity, .. } => Some(*entity),
            Self::Sound { .. } => None,
        }
    }
}

enum GameplayEffect {
    Body(GameplayEntity, crate::BodyCommand),
    Sound(crate::AssetRef, crate::PlayParams),
    Audio(GameplayEntity, crate::AudioOp),
    Animation(GameplayEntity, crate::AnimateOp, crate::Animator),
}

#[derive(Default)]
struct AudioBusChanges {
    gains: [Option<f32>; AudioBus::COUNT],
    muted: Option<bool>,
}

struct GameplayJournal {
    persistent: Vec<GameplayEntity>,
    publication_tick: fynkel_contracts::Tick,
    native_phase: bool,
    scope: (WorldId, u64),
    next_pending: u32,
    removed: BTreeSet<GameplayEntity>,
    changes: BTreeMap<GameplayEntity, Changes>,
    parent_order: Vec<GameplayEntity>,
    effects: Vec<GameplayEffect>,
    audio_buses: AudioBusChanges,
    input_bindings: Option<PreparedInputBindings>,
    prepared_controllers: super::controller::ControllerTable,
    component_revisions: BTreeMap<(GameplayEntity, ComponentId), u64>,
    registered_revision: u64,
    failed: RefCell<Option<EditError>>,
}

/// An owned, immutable population snapshot. It contains no borrowed native memory.
/// Applying it requires the same edit and unchanged registered data/membership.
#[derive(Clone, Debug)]
pub struct RegisteredBatch {
    scope: (WorldId, u64),
    stamp: (u64, u32, usize),
    id: String,
    rows: Vec<(GameplayEntity, Vec<crate::ScalarValue>)>,
}

impl RegisteredBatch {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn rows(&self) -> &[(GameplayEntity, Vec<crate::ScalarValue>)] {
        &self.rows
    }
}

impl GameplayEdit<'_> {
    /// Prepared immutable definitions for this edit's validation and logical reads.
    /// Resource ownership/publication remains with the host's prepared plan.
    pub fn prepare_controller(
        &mut self,
        key: fynkel_contracts::AssetRef,
        def: std::sync::Arc<crate::animation::controller::ControllerDef>,
    ) -> EditResult<()> {
        self.check()?;
        if def.validate().is_err() {
            return self.fail(EditError::Aborted);
        }
        self.journal.prepared_controllers.insert(key, def);
        Ok(())
    }

    fn check(&self) -> EditResult<()> {
        self.journal.failed.borrow().clone().map_or(Ok(()), Err)
    }

    fn fail<T>(&self, error: EditError) -> EditResult<T> {
        let first = self.journal.failed.borrow().clone().unwrap_or(error);
        self.journal.failed.replace(Some(first.clone()));
        Err(first)
    }

    pub fn set_registered_authored(
        &mut self,
        owner: GameplayEntity,
        component: &crate::RegisteredDescription,
        resolve: impl Fn(fynkel_contracts::Uuid) -> Option<GameplayEntity>,
    ) -> EditResult<()> {
        self.check()?;
        let schema = match self.world.registered.get(&component.id) {
            Ok(r) => &r.schema,
            Err(e) => return self.fail(EditError::RegisteredComponent(e)),
        };
        if component.version != schema.version
            || component
                .fields
                .keys()
                .any(|name| !schema.fields.iter().any(|f| f.name == *name))
        {
            return self.fail(EditError::RegisteredComponent(
                crate::RegistryError::InvalidValue(format!(
                    "{}: schema version or field mismatch; explicit migration required",
                    component.id
                )),
            ));
        }
        let mut values = Vec::new();
        for field in &schema.fields {
            let value = match component.fields.get(&field.name) {
                None => field.default.clone(),
                Some(crate::AuthoredRegisteredValue::Scalar(value)) => {
                    match (value, &field.default) {
                        (crate::ScalarValue::Int(n), crate::ScalarValue::Float(_)) => {
                            crate::ScalarValue::Float(*n as f64)
                        }
                        _ => value.clone(),
                    }
                }
                Some(crate::AuthoredRegisteredValue::Object(reference)) => {
                    let target = if !reference.id.is_nil() {
                        match resolve(reference.id) {
                            Some(e) => Some(e),
                            None => {
                                return self.fail(EditError::RegisteredComponent(
                                    crate::RegistryError::InvalidValue(
                                        "unresolved authored component reference".into(),
                                    ),
                                ))
                            }
                        }
                    } else if !reference.entity.is_null() {
                        Some(reference.entity.into())
                    } else {
                        None
                    };
                    crate::ScalarValue::Object(target)
                }
            };
            values.push(value);
        }
        self.set_registered_values(owner, &component.id, values)
    }

    pub fn query_registered(&self, id: &str) -> EditResult<RegisteredBatch> {
        self.check()?;
        if let Err(error) = self.world.registered.get(id) {
            return self.fail(EditError::RegisteredComponent(error));
        }
        let mut rows = Vec::new();
        for entity in self.entities()? {
            if let Some(fields) = self.registered_values(entity, id)? {
                rows.push((entity, fields));
            }
        }
        Ok(RegisteredBatch {
            scope: self.journal.scope,
            stamp: (
                self.journal.registered_revision,
                self.journal.next_pending,
                self.journal.removed.len(),
            ),
            id: id.into(),
            rows,
        })
    }
    /// A frozen population cannot be silently applied over intervening writes,
    /// membership changes, another world or a later transaction. Every row is
    /// validated before publication; one bad row rejects the entire edit.
    pub fn apply_registered_batch(
        &mut self,
        batch: &RegisteredBatch,
        values: Vec<Vec<crate::ScalarValue>>,
    ) -> EditResult<()> {
        self.check()?;
        if batch.scope != self.journal.scope
            || batch.stamp
                != (
                    self.journal.registered_revision,
                    self.journal.next_pending,
                    self.journal.removed.len(),
                )
            || values.len() != batch.rows.len()
        {
            return self.fail(EditError::RegisteredComponent(
                crate::RegistryError::InvalidValue("expired or mismatched bulk view".into()),
            ));
        }
        for ((owner, _), value) in batch.rows.iter().zip(values) {
            self.set_registered_values(*owner, &batch.id, value)?;
        }
        Ok(())
    }

    /// Registered scalar values through the same journal as built-in data.
    pub fn registered_values(
        &self,
        entity: impl Into<GameplayEntity>,
        id: &str,
    ) -> EditResult<Option<Vec<crate::ScalarValue>>> {
        let entity = entity.into();
        self.require(entity)?;
        let registration = match self.world.registered.get(id) {
            Ok(r) => r,
            Err(error) => return self.fail(EditError::RegisteredComponent(error)),
        };
        let fields = if let Some(value) = self
            .journal
            .changes
            .get(&entity)
            .and_then(|c| c.registered.get(id))
        {
            value.as_ref().map(|v| v.fields.clone())
        } else {
            entity
                .live()
                .and_then(|e| registration.read(&self.world.ecs, e))
        };
        Ok(fields.map(|mut fields| {
            for value in &mut fields {
                if let crate::ScalarValue::Object(target) = value {
                    if target.is_some_and(|r| {
                        self.journal.removed.contains(&r)
                            || r.live().is_some_and(|e| !self.world.ecs.contains(e))
                    }) {
                        *target = None;
                    }
                }
            }
            fields
        }))
    }

    pub fn set_registered_values(
        &mut self,
        entity: impl Into<GameplayEntity>,
        id: &str,
        fields: Vec<crate::ScalarValue>,
    ) -> EditResult<()> {
        let entity = entity.into();
        self.require(entity)?;
        for field in &fields {
            if let crate::ScalarValue::Object(Some(target)) = field {
                self.require(*target)?;
            }
        }
        let prepared = self
            .world
            .registered
            .get(id)
            .and_then(|r| r.prepare(fields));
        let prepared = match prepared {
            Ok(p) => p,
            Err(error) => return self.fail(EditError::RegisteredComponent(error)),
        };
        self.journal.registered_revision = self
            .journal
            .registered_revision
            .checked_add(1)
            .ok_or(EditError::IdentityExhausted)?;
        self.journal
            .changes
            .entry(entity)
            .or_default()
            .registered
            .insert(id.into(), Some(prepared));
        Ok(())
    }
    pub fn registered<T: crate::RegisteredComponent>(
        &self,
        entity: impl Into<GameplayEntity>,
    ) -> EditResult<Option<T>> {
        self.check()?;
        let registration = match self.world.registered.typed::<T>() {
            Ok(r) => r,
            Err(error) => return self.fail(EditError::RegisteredComponent(error)),
        };
        self.registered_values(entity, &registration.schema.id)?
            .map(|v| {
                T::decode(&v).or_else(|why| {
                    self.fail(EditError::RegisteredComponent(
                        crate::RegistryError::InvalidValue(why),
                    ))
                })
            })
            .transpose()
    }
    pub fn set_registered<T: crate::RegisteredComponent>(
        &mut self,
        entity: impl Into<GameplayEntity>,
        value: T,
    ) -> EditResult<()> {
        self.check()?;
        let id = match self.world.registered.typed::<T>() {
            Ok(r) => r.schema.id.clone(),
            Err(error) => return self.fail(EditError::RegisteredComponent(error)),
        };
        self.set_registered_values(entity, &id, value.encode())
    }
    pub fn add_registered_defaults(
        &mut self,
        entity: impl Into<GameplayEntity>,
        id: &str,
    ) -> EditResult<()> {
        self.check()?;
        let defaults = match self.world.registered.get(id) {
            Ok(r) => r.schema.fields.iter().map(|f| f.default.clone()).collect(),
            Err(error) => return self.fail(EditError::RegisteredComponent(error)),
        };
        self.set_registered_values(entity, id, defaults)
    }
    pub fn remove_registered(
        &mut self,
        entity: impl Into<GameplayEntity>,
        id: &str,
    ) -> EditResult<()> {
        let entity = entity.into();
        self.require(entity)?;
        if let Err(error) = self.world.registered.get(id) {
            return self.fail(EditError::RegisteredComponent(error));
        }
        self.journal.registered_revision = self
            .journal
            .registered_revision
            .checked_add(1)
            .ok_or(EditError::IdentityExhausted)?;
        self.journal
            .changes
            .entry(entity)
            .or_default()
            .registered
            .insert(id.into(), None);
        Ok(())
    }

    /// Marks a logical root for scene persistence. Non-roots are refused
    /// without changing the edit; destroyed/stale references remain errors.
    pub fn dont_destroy_on_load(&mut self, entity: impl Into<GameplayEntity>) -> EditResult<bool> {
        let entity = entity.into();
        self.require(entity)?;
        if self.parent(entity)?.is_some() {
            return Ok(false);
        }
        if !self.journal.persistent.contains(&entity) {
            self.journal.persistent.push(entity);
        }
        Ok(true)
    }
    pub fn persistent_roots(&self) -> EditResult<Vec<GameplayEntity>> {
        self.check()?;
        let mut roots = Vec::new();
        for entity in self
            .world
            .persistent
            .iter()
            .copied()
            .map(GameplayEntity::Live)
            .chain(self.journal.persistent.iter().copied())
        {
            if self.contains(entity)? && self.parent(entity)?.is_none() && !roots.contains(&entity)
            {
                roots.push(entity);
            }
        }
        Ok(roots)
    }

    /// Identity changes only: removed handles have no name; surviving spawns
    /// and name writes carry their final logical name. No whole-world scan.
    pub fn identity_changes(&self) -> EditResult<Vec<(GameplayEntity, Option<String>)>> {
        self.check()?;
        let mut changes: BTreeMap<_, _> = self
            .journal
            .changes
            .iter()
            .filter_map(|(&entity, change)| {
                change
                    .name
                    .as_ref()
                    .map(|name| (entity, Some(name.clone())))
            })
            .collect();
        for &entity in &self.journal.removed {
            changes.insert(entity, None);
        }
        Ok(changes.into_iter().collect())
    }

    pub fn contains(&self, entity: impl Into<GameplayEntity>) -> EditResult<bool> {
        self.check()?;
        let entity = entity.into();
        let exists = match entity {
            GameplayEntity::Live(e) => self.world.ecs.contains(e),
            GameplayEntity::Pending(p) => {
                if (p.world, p.transaction) != self.journal.scope
                    || !self.journal.changes.contains_key(&entity)
                {
                    return self.fail(EditError::ExpiredPending(p));
                }
                true
            }
        };
        Ok(exists && !self.journal.removed.contains(&entity))
    }

    fn require(&self, entity: GameplayEntity) -> EditResult<()> {
        if !self.contains(entity)? {
            return if self.journal.removed.contains(&entity) {
                self.fail(EditError::Destroyed(entity))
            } else {
                self.fail(EditError::StaleEntity(
                    entity.live().expect("pending checked"),
                ))
            };
        }
        Ok(())
    }

    fn spatial(&self, entity: GameplayEntity) -> EditResult<()> {
        self.local_transform(entity)?;
        if entity
            .live()
            .is_some_and(|e| !self.world.ecs.has::<WorldTransform>(e))
        {
            return self.fail(EditError::MissingTransform(entity));
        }
        Ok(())
    }

    /// Materialized membership at this call. Use query() to skip candidates
    /// destroyed during traversal. Live handles precede pending creation order.
    pub fn entities(&self) -> EditResult<Vec<GameplayEntity>> {
        self.check()?;
        let mut entities: Vec<_> = self
            .world
            .ecs
            .entities()
            .map(GameplayEntity::Live)
            .chain(
                self.journal
                    .changes
                    .keys()
                    .copied()
                    .filter(|e| matches!(e, GameplayEntity::Pending(_))),
            )
            .filter(|e| !self.journal.removed.contains(e))
            .collect();
        entities.sort_unstable();
        Ok(entities)
    }

    pub fn query(&self) -> EditResult<GameplayQuery> {
        self.query_with(&[])
    }

    /// Frozen intersection of component membership. Replacing values does not
    /// change membership; remove/re-add requires a new query to visit that member.
    pub fn query_with(&self, components: &[ComponentId]) -> EditResult<GameplayQuery> {
        let required: Vec<_> = components
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        for &id in &required {
            self.supported_component(id)?;
        }
        let mut candidates = Vec::new();
        for entity in self.entities()? {
            let mut matches = true;
            for &id in &required {
                matches &= self.has_component(entity, id)?;
            }
            if matches {
                let revisions = required
                    .iter()
                    .map(|&id| self.component_revision(entity, id))
                    .collect();
                candidates.push((entity, revisions));
            }
        }
        Ok(GameplayQuery {
            scope: self.journal.scope,
            required,
            candidates: candidates.into_iter(),
        })
    }

    fn supported_component(&self, component: ComponentId) -> EditResult<()> {
        self.check()?;
        match ComponentValue::object_ref_fields(component) {
            Some(_) => Ok(()),
            None if matches!(component, ComponentId::NAME | ComponentId::HIERARCHY) => {
                self.fail(EditError::UnsupportedComponent(component))
            }
            None => self.fail(EditError::UnknownComponent(component)),
        }
    }

    fn component_revision(&self, entity: GameplayEntity, component: ComponentId) -> u64 {
        self.journal
            .component_revisions
            .get(&(entity, component))
            .copied()
            .unwrap_or(0)
    }

    fn change_membership(
        &mut self,
        entity: GameplayEntity,
        component: ComponentId,
    ) -> EditResult<()> {
        let Some(next) = self.component_revision(entity, component).checked_add(1) else {
            return self.fail(EditError::IdentityExhausted);
        };
        self.journal
            .component_revisions
            .insert((entity, component), next);
        Ok(())
    }

    /// Reads current configuration, including live or pending object references.
    pub fn component(
        &self,
        entity: impl Into<GameplayEntity>,
        component: ComponentId,
    ) -> EditResult<Option<GameplayComponentValue>> {
        let entity = entity.into();
        self.require(entity)?;
        self.supported_component(component)?;
        if component == ComponentId::TRANSFORM {
            if let Some(t) = self.journal.changes.get(&entity).and_then(|c| c.transform) {
                return Ok(Some(GameplayComponentValue::Transform(t)));
            }
        }
        if let Some(value) = self
            .journal
            .changes
            .get(&entity)
            .and_then(|c| c.components.get(&component))
        {
            return Ok(value.clone());
        }
        Ok(entity
            .live()
            .and_then(|e| ComponentValue::read_from(&self.world.ecs, e, component))
            .map(Into::into))
    }

    pub fn has_component(
        &self,
        entity: impl Into<GameplayEntity>,
        component: ComponentId,
    ) -> EditResult<bool> {
        let entity = entity.into();
        self.require(entity)?;
        self.supported_component(component)?;
        if component == ComponentId::TRANSFORM
            && self
                .journal
                .changes
                .get(&entity)
                .is_some_and(|c| c.transform.is_some())
        {
            return Ok(true);
        }
        if let Some(value) = self
            .journal
            .changes
            .get(&entity)
            .and_then(|c| c.components.get(&component))
        {
            return Ok(value.is_some());
        }
        Ok(entity
            .live()
            .is_some_and(|e| ComponentValue::contains_in(&self.world.ecs, e, component)))
    }

    /// Configuration data only: asset readiness and subsystem effects reconcile
    /// at their normal boundaries. Disabled modules retain their component data.
    pub fn set_component(
        &mut self,
        entity: impl Into<GameplayEntity>,
        value: impl Into<GameplayComponentValue>,
    ) -> EditResult<()> {
        let value = value.into();
        let entity = entity.into();
        self.require(entity)?;
        let component = value.id();
        self.supported_component(component)?;
        if let GameplayComponentValue::Transform(transform) = value {
            return self.set_transform(entity, transform);
        }
        if let Err(message) = value.validate() {
            return self.fail(EditError::InvalidComponent {
                entity,
                component,
                message,
            });
        }
        self.validate_references(&value)?;
        if !self.has_component(entity, component)? {
            self.change_membership(entity, component)?;
        }
        self.journal
            .changes
            .entry(entity)
            .or_default()
            .components
            .insert(component, Some(value));
        Ok(())
    }

    fn validate_references(&self, value: &GameplayComponentValue) -> EditResult<()> {
        let mut result = Ok(());
        value.for_each_reference(|reference| {
            if result.is_err() {
                return;
            }
            result = match reference {
                GameplayObjectRef::Stored(r) if r.get().is_none() && !r.id.is_nil() => {
                    self.fail(EditError::UnresolvedObjectReference(r.id))
                }
                _ => reference
                    .entity()
                    .map_or(Ok(()), |entity| self.require(entity)),
            };
        });
        result
    }

    // Recheck staged references before any allocation: targets can be retired
    // after a component was written. Retired owners publish no values.
    fn validate_final_references(&self) -> EditResult<()> {
        for (entity, change) in &self.journal.changes {
            if self.journal.removed.contains(entity) {
                continue;
            }
            for value in change.components.values().flatten() {
                self.validate_references(value)?;
            }
        }
        for effect in &self.journal.effects {
            match effect {
                GameplayEffect::Body(entity, op) if !self.journal.removed.contains(entity) => {
                    self.validate_body_effect(*entity, *op)?
                }
                GameplayEffect::Audio(entity, _) if !self.journal.removed.contains(entity) => {
                    self.audio_source(*entity)?;
                }
                GameplayEffect::Animation(entity, op, before)
                    if !self.journal.removed.contains(entity) =>
                {
                    let current = self.validate_animation(*entity, op)?;
                    if op.controller_transport() && current.controller != before.controller {
                        return self.fail(EditError::InvalidEffect(
                            *entity,
                            "animator controller changed after request".into(),
                        ));
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Requests a one-shot body/character operation for the next physics tick.
    /// Abort publishes nothing. Destroyed targets in this edit cancel their
    /// requests; surviving targets must still satisfy the final configuration.
    pub fn body_effect(
        &mut self,
        entity: impl Into<GameplayEntity>,
        op: crate::BodyCommand,
    ) -> EditResult<()> {
        let entity = entity.into();
        self.require(entity)?;
        self.validate_body_effect(entity, op)?;
        self.journal.effects.push(GameplayEffect::Body(entity, op));
        Ok(())
    }

    /// Queues a fire-and-forget sound for the next simulation tick after commit.
    /// Clip availability is checked at execution, after prepared resources publish.
    pub fn play_sound(
        &mut self,
        clip: crate::AssetRef,
        params: crate::PlayParams,
    ) -> EditResult<()> {
        self.check()?;
        if let Err(error) = self.world.validate_sound_request(&params) {
            return self.fail(error);
        }
        self.journal
            .effects
            .push(GameplayEffect::Sound(clip, params));
        Ok(())
    }

    /// Stages a replacement binding table. Invalid binding strings are
    /// reported and skipped, matching direct binding; the last table wins.
    /// Captured input and sampled SDK views are never staged or consumed here.
    pub fn bind_input(
        &mut self,
        specs: &[crate::ActionSpec],
    ) -> EditResult<Vec<(String, String, crate::input::BindingError)>> {
        self.check()?;
        let (prepared, errors) = PreparedInputBindings::prepare(specs);
        self.journal.input_bindings = Some(prepared);
        Ok(errors)
    }

    /// Logical bus gain and global mute, including this edit's latest writes.
    pub fn audio_bus(&self, bus: AudioBus) -> EditResult<(f32, bool)> {
        self.check()?;
        Ok((
            self.journal.audio_buses.gains[bus.index()]
                .unwrap_or_else(|| self.world.audio.bus(bus)),
            self.journal
                .audio_buses
                .muted
                .unwrap_or_else(|| self.world.audio.muted()),
        ))
    }

    /// Stages mixer configuration. Gains are per bus; mute is global and the
    /// last write across any bus wins. Nothing is sent to the mixer until commit.
    pub fn set_audio_bus(&mut self, bus: AudioBus, volume: f32, muted: bool) -> EditResult<()> {
        self.check()?;
        if let Err(message) = self.world.validate_audio_bus(volume) {
            return self.fail(EditError::InvalidEffect(
                GameplayEntity::Live(Entity::NULL),
                message,
            ));
        }
        self.journal.audio_buses.gains[bus.index()] = Some(volume);
        self.journal.audio_buses.muted = Some(muted);
        Ok(())
    }

    fn validate_animation(
        &self,
        entity: GameplayEntity,
        op: &crate::AnimateOp,
    ) -> EditResult<crate::Animator> {
        self.require(entity)?;
        if !self.world.modules.animation {
            return self.fail(EditError::InvalidEffect(
                entity,
                "animation module is disabled".into(),
            ));
        }
        let animator = match self.component(entity, ComponentId::ANIMATOR)? {
            Some(GameplayComponentValue::Animator(a)) => a,
            _ => return self.fail(EditError::MissingComponent(entity, ComponentId::ANIMATOR)),
        };
        if !op.clip_transport()
            && !op.controller_transport()
            && op.configuration(animator.clone()).is_none()
        {
            return self.fail(EditError::InvalidEffect(
                entity,
                "unsupported animation operation".into(),
            ));
        }
        if op.requires_uncontrolled() && animator.controller.as_ref().is_some_and(|c| !c.is_nil()) {
            return self.fail(EditError::InvalidEffect(
                entity,
                "clip controls require an animator without a controller".into(),
            ));
        }
        if let Err(message) = op.validate_clip_transport() {
            return self.fail(EditError::InvalidEffect(entity, message));
        }
        if let Err(message) = op.validate_controller_transport() {
            return self.fail(EditError::InvalidEffect(entity, message));
        }
        if op.controller_transport() {
            let Some(controller_ref) = animator.controller.as_ref().filter(|c| !c.is_nil()) else {
                return self.fail(EditError::InvalidEffect(
                    entity,
                    "animator has no controller".into(),
                ));
            };
            let validation = match self.journal.prepared_controllers.get(controller_ref) {
                Some(def) => super::controller::validate_transport(def, op),
                None => self.world.validate_controller_transport(controller_ref, op),
            };
            if let Err(message) = validation {
                return self.fail(EditError::InvalidEffect(entity, message));
            }
        }
        Ok(animator)
    }

    /// Logical controller parameter: completed runtime/default followed by committed
    /// and current-edit requests. This never mutates the animation runtime.
    pub fn controller_parameter(
        &self,
        entity: impl Into<GameplayEntity>,
        name: &str,
    ) -> EditResult<
        Option<(
            crate::animation::controller::ParameterKind,
            crate::animation::controller::Value,
        )>,
    > {
        use crate::animation::controller::{ParameterKind, Value};
        let entity = entity.into();
        self.require(entity)?;
        let animator = match self.component(entity, ComponentId::ANIMATOR)? {
            Some(GameplayComponentValue::Animator(a)) => a,
            _ => return self.fail(EditError::MissingComponent(entity, ComponentId::ANIMATOR)),
        };
        let Some(selected) = animator.controller.as_ref().filter(|c| !c.is_nil()) else {
            return self.fail(EditError::InvalidEffect(
                entity,
                "animator has no controller".into(),
            ));
        };
        let mut result = None;
        if let Some(def) = self
            .journal
            .prepared_controllers
            .get(selected)
            .or_else(|| self.world.controller(selected))
        {
            let Some(i) = def.parameter_index(name) else {
                return self.fail(EditError::InvalidEffect(
                    entity,
                    format!("no parameter `{name}`"),
                ));
            };
            let p = &def.parameters[i];
            result = Some((p.kind, p.default));
            if let GameplayEntity::Live(e) = entity {
                if let Ok(run) = self.world.ecs.get::<super::AnimatorRun>(e) {
                    if let Some(play) = &run.controller {
                        if &play.controller == selected {
                            result = play.run.get(&play.def, name).ok();
                        }
                    }
                }
            }
        }
        let declared_kind = result.map(|(kind, _)| kind);
        let mut apply = |op: &crate::AnimateOp, before: &crate::Animator| {
            if before.controller.as_ref() != Some(selected) {
                return;
            }
            if let crate::AnimateOp::SetControllerParameter { name: n, value } = op {
                if n == name {
                    let kind = declared_kind.unwrap_or(match value {
                        Value::Float(_) => ParameterKind::Float,
                        Value::Int(_) => ParameterKind::Int,
                        Value::Bool(_) => ParameterKind::Bool,
                    });
                    result = Some((kind, *value));
                }
            }
        };
        if let GameplayEntity::Live(e) = entity {
            for effect in &self.world.gameplay_effects {
                if let CommittedEffect::Animation {
                    entity: owner,
                    op,
                    before,
                    invalidated,
                } = effect
                {
                    if *owner == e && !invalidated {
                        apply(op, before);
                    }
                }
            }
        }
        for effect in &self.journal.effects {
            if let GameplayEffect::Animation(owner, op, before) = effect {
                if *owner == entity {
                    apply(op, before);
                }
            }
        }
        Ok(result)
    }

    /// Publishes logical configuration on commit and runtime transport at the next tick.
    pub fn animate_control(
        &mut self,
        entity: impl Into<GameplayEntity>,
        op: crate::AnimateOp,
    ) -> EditResult<()> {
        let entity = entity.into();
        let before = self.validate_animation(entity, &op)?;
        let updated = op
            .configuration(before.clone())
            .unwrap_or_else(|| op.clip_configuration(before.clone()));
        self.set_component(entity, GameplayComponentValue::Animator(updated))?;
        if op.clip_transport() || op.controller_transport() {
            self.journal
                .effects
                .push(GameplayEffect::Animation(entity, op, before));
        }
        Ok(())
    }

    fn audio_source(&self, entity: GameplayEntity) -> EditResult<crate::AudioSource> {
        if !self.world.modules.audio {
            return self.fail(EditError::InvalidEffect(
                entity,
                "audio module is disabled".into(),
            ));
        }
        match self.component(entity, ComponentId::AUDIO_SOURCE)? {
            Some(GameplayComponentValue::AudioSource(source)) => Ok(source),
            _ => self.fail(EditError::MissingComponent(
                entity,
                ComponentId::AUDIO_SOURCE,
            )),
        }
    }

    /// Configuration changes are logical immediately; transport executes after
    /// commit without overwriting configuration published by later edits.
    pub fn audio_control(
        &mut self,
        entity: impl Into<GameplayEntity>,
        op: crate::AudioOp,
    ) -> EditResult<()> {
        let entity = entity.into();
        self.require(entity)?;
        let mut source = self.audio_source(entity)?;
        if let Err(message) = op.validate() {
            return self.fail(EditError::InvalidEffect(entity, message));
        }
        match op {
            crate::AudioOp::Play => source.playing = true,
            crate::AudioOp::Stop => source.playing = false,
            crate::AudioOp::SetVolume(v) => source.volume = v,
            crate::AudioOp::SetPitch(p) => source.pitch = p,
            crate::AudioOp::Pause | crate::AudioOp::Resume => {}
        }
        self.set_component(entity, GameplayComponentValue::AudioSource(source))?;
        if !matches!(
            op,
            crate::AudioOp::SetVolume(_) | crate::AudioOp::SetPitch(_)
        ) {
            self.journal.effects.push(GameplayEffect::Audio(entity, op));
        }
        Ok(())
    }

    fn validate_body_effect(
        &self,
        entity: GameplayEntity,
        op: crate::BodyCommand,
    ) -> EditResult<()> {
        if !self.world.modules.physics {
            return self.fail(EditError::InvalidEffect(
                entity,
                "physics module is disabled".into(),
            ));
        }
        let body = match self.component(entity, ComponentId::RIGID_BODY)? {
            Some(GameplayComponentValue::RigidBody(body)) => Some(body.body_type),
            _ => None,
        };
        let character = self.has_component(entity, ComponentId::CHARACTER_CONTROLLER)?;
        if let Err((_, message)) = op.validate_for(body, character) {
            return self.fail(EditError::InvalidEffect(entity, message));
        }
        Ok(())
    }

    pub fn remove_component(
        &mut self,
        entity: impl Into<GameplayEntity>,
        component: ComponentId,
    ) -> EditResult<()> {
        let entity = entity.into();
        self.require(entity)?;
        if matches!(
            component,
            ComponentId::TRANSFORM | ComponentId::NAME | ComponentId::HIERARCHY
        ) {
            return self.fail(EditError::ProtectedComponent(component));
        }
        self.supported_component(component)?;
        if !self.has_component(entity, component)? {
            return self.fail(EditError::MissingComponent(entity, component));
        }
        self.change_membership(entity, component)?;
        self.journal
            .changes
            .entry(entity)
            .or_default()
            .components
            .insert(component, None);
        Ok(())
    }

    pub fn spawn(
        &mut self,
        name: impl Into<String>,
        transform: Transform,
        parent: Option<GameplayEntity>,
    ) -> EditResult<PendingEntity> {
        self.check()?;
        let Some(next) = self.journal.next_pending.checked_add(1) else {
            return self.fail(EditError::IdentityExhausted);
        };
        let pending = PendingEntity {
            world: self.journal.scope.0,
            transaction: self.journal.scope.1,
            index: self.journal.next_pending,
        };
        let entity = pending.into();
        if !transform.is_valid() {
            return self.fail(EditError::InvalidTransform(entity));
        }
        self.journal.next_pending = next;
        self.journal.changes.insert(
            entity,
            Changes {
                name: Some(name.into()),
                transform: Some(transform),
                parent: Some(None),
                active: Some(true),
                components: BTreeMap::new(),
                registered: BTreeMap::new(),
            },
        );
        self.set_parent(entity, parent, false)?;
        Ok(pending)
    }

    /// Clones current logical configuration, preserving subtree order and
    /// remapping internal references. Runtime state and derived markers are not
    /// copied. An unparented clone defaults to the source's world-space pose;
    /// a parented clone defaults to its local pose. Explicit placement wins.
    pub fn clone_tree(
        &mut self,
        source: GameplayEntity,
        parent: Option<GameplayEntity>,
        transform: Option<Transform>,
        name: Option<String>,
    ) -> EditResult<GameplayClone> {
        self.spatial(source)?;
        if let Some(parent) = parent {
            self.spatial(parent)?;
        }
        let root_pose = match transform {
            Some(pose) => pose,
            None if parent.is_some() => self.local_transform(source)?,
            None => match Transform::try_from_matrix(self.world_matrix(source)?) {
                Some(pose) => pose,
                None => return self.fail(EditError::UnsupportedTransform(source)),
            },
        };
        // Freeze the original tree before creating anything: the requested
        // destination may itself be a descendant of the source.
        let mut frozen = Vec::new();
        let mut stack = vec![source];
        while let Some(entity) = stack.pop() {
            let mut children = self.children(entity)?;
            children.reverse();
            stack.extend(children);
            let mut components = Vec::new();
            for &id in ComponentValue::IDS {
                if id != ComponentId::TRANSFORM {
                    if let Some(value) = self.component(entity, id)? {
                        components.push(value);
                    }
                }
            }
            let mut registered = Vec::new();
            for schema in self.world.registered.schemas() {
                if let Some(fields) = self.registered_values(entity, &schema.id)? {
                    registered.push((schema.id.clone(), fields));
                }
            }
            frozen.push((
                entity,
                self.parent(entity)?,
                self.local_transform(entity)?,
                self.name(entity)?.unwrap_or_default(),
                self.active_self(entity)?,
                components,
                registered,
            ));
        }
        let mut mapping = BTreeMap::new();
        let mut entities = Vec::with_capacity(frozen.len());
        for (entity, old_parent, pose, label, active, _, _) in &frozen {
            let root = *entity == source;
            let owner = if root {
                parent
            } else {
                old_parent.map(|p| GameplayEntity::Pending(mapping[&p]))
            };
            let copy = self.spawn(
                if root {
                    name.as_deref().unwrap_or(label)
                } else {
                    label
                },
                if root { root_pose } else { *pose },
                owner,
            )?;
            self.set_active(copy, *active)?;
            mapping.insert(*entity, copy);
            entities.push((*entity, copy));
        }
        for (entity, _, _, _, _, components, registered) in frozen {
            for (id, mut fields) in registered {
                for value in &mut fields {
                    if let crate::ScalarValue::Object(Some(target)) = value {
                        if let Some(copy) = mapping.get(target) {
                            *target = (*copy).into();
                        }
                    }
                }
                self.set_registered_values(mapping[&entity], &id, fields)?;
            }
            for value in components {
                let value = value.map_object_refs(|reference| {
                    reference
                        .entity()
                        .and_then(|target| mapping.get(&target))
                        .copied()
                        .map(GameplayObjectRef::Pending)
                        .unwrap_or(reference)
                });
                self.set_component(mapping[&entity], value)?;
            }
        }
        Ok(GameplayClone {
            root: mapping[&source],
            entities,
        })
    }

    /// Stages a one-root prefab description through ordinary gameplay edits.
    /// This operation performs no asset loading, resource registration, script
    /// construction or lifecycle dispatch. Root placement replaces its authored
    /// transform; descendant poses and child ordering are preserved.
    ///
    /// Authored references must resolve within this instance. Unresolved ids,
    /// invalid components or malformed structure fail the entire edit stickily,
    /// unlike the legacy loader's partial component acceptance. Runtime-only
    /// references are validated by the normal world/transaction rules.
    pub fn instantiate(
        &mut self,
        prefab: &crate::SceneDescription,
        name: &str,
        transform: Transform,
        parent: Option<GameplayEntity>,
    ) -> EditResult<GameplayPrefab> {
        self.check()?;
        let order = match crate::load::spawn_order(prefab) {
            Ok(order) => order,
            Err(error) => return self.fail(EditError::InvalidPrefab(error)),
        };
        let mut roots = prefab.entities.iter().filter(|d| d.parent.is_none());
        let (Some(root), None) = (roots.next(), roots.next()) else {
            return self.fail(EditError::InvalidPrefab(crate::LoadError::NotOneRoot(
                prefab.id,
            )));
        };
        let root_id = root.id;
        let mut by_id = BTreeMap::new();
        let mut entities = Vec::with_capacity(order.len());
        for &i in &order {
            let d = &prefab.entities[i];
            let is_root = d.id == root_id;
            let pose = if is_root {
                transform
            } else {
                d.components
                    .iter()
                    .find_map(|c| match c {
                        ComponentValue::Transform(t) => Some(*t),
                        _ => None,
                    })
                    .unwrap_or(Transform::IDENTITY)
            };
            let owner = d
                .parent
                .map(|id| GameplayEntity::Pending(by_id[&id]))
                .or(parent);
            let label = if is_root && !name.is_empty() {
                name
            } else {
                &d.name
            };
            let entity = self.spawn(label, pose, owner)?;
            self.set_active(entity, d.active)?;
            by_id.insert(d.id, entity);
            entities.push((d.id, entity));
        }
        // Allocate the complete logical hierarchy before mapping forward and
        // cyclic references. No live ECS identity is allocated until commit.
        for &i in &order {
            let d = &prefab.entities[i];
            for component in &d.components {
                if matches!(component, ComponentValue::Transform(_)) {
                    continue;
                }
                let mut missing = None;
                let value =
                    GameplayComponentValue::from(component.clone()).map_object_refs(|reference| {
                        match reference {
                            GameplayObjectRef::Stored(r) if r.id != fynkel_contracts::Uuid::NIL => {
                                match by_id.get(&r.id) {
                                    Some(&target) => GameplayObjectRef::Pending(target),
                                    None => {
                                        missing.get_or_insert(r.id);
                                        GameplayObjectRef::NONE
                                    }
                                }
                            }
                            other => other,
                        }
                    });
                if let Some(id) = missing {
                    return self.fail(EditError::UnresolvedObjectReference(id));
                }
                self.set_component(by_id[&d.id], value)?;
            }
        }
        for d in &prefab.entities {
            for component in &d.registered {
                self.set_registered_authored(by_id[&d.id].into(), component, |id| {
                    by_id.get(&id).copied().map(Into::into)
                })?;
            }
        }
        Ok(GameplayPrefab {
            root: by_id[&root_id],
            entities,
        })
    }

    /// Retires the subtree in the current logical hierarchy, immediately.
    /// Moved-out descendants survive; moved-in and pending descendants retire.
    pub fn destroy(&mut self, entity: impl Into<GameplayEntity>) -> EditResult<()> {
        let entity = entity.into();
        self.require(entity)?;
        let mut stack = vec![entity];
        let mut removed = BTreeSet::new();
        while let Some(e) = stack.pop() {
            if removed.insert(e) {
                stack.extend(self.children(e)?);
            }
        }
        self.journal.removed.extend(removed);
        Ok(())
    }

    pub fn name(&self, entity: impl Into<GameplayEntity>) -> EditResult<Option<String>> {
        let entity = entity.into();
        self.require(entity)?;
        Ok(self
            .journal
            .changes
            .get(&entity)
            .and_then(|c| c.name.clone())
            .or_else(|| {
                entity
                    .live()
                    .and_then(|e| self.world.ecs.get::<Name>(e).ok().map(|n| n.value.clone()))
            }))
    }

    /// First exact name match in stable handle order, including this edit's renames.
    pub fn find(&self, name: &str) -> EditResult<Option<GameplayEntity>> {
        for entity in self.entities()? {
            if self.name(entity)?.as_deref() == Some(name) {
                return Ok(Some(entity));
            }
        }
        Ok(None)
    }

    pub fn local_transform(&self, entity: impl Into<GameplayEntity>) -> EditResult<Transform> {
        let entity = entity.into();
        self.require(entity)?;
        if let Some(t) = self.journal.changes.get(&entity).and_then(|c| c.transform) {
            return Ok(t);
        }
        match entity
            .live()
            .and_then(|e| self.world.ecs.copy::<Transform>(e).ok())
        {
            Some(transform) => Ok(transform),
            None => self.fail(EditError::MissingTransform(entity)),
        }
    }

    pub fn parent(&self, entity: impl Into<GameplayEntity>) -> EditResult<Option<GameplayEntity>> {
        let entity = entity.into();
        self.require(entity)?;
        Ok(self
            .journal
            .changes
            .get(&entity)
            .and_then(|c| c.parent)
            .unwrap_or_else(|| {
                entity
                    .live()
                    .and_then(|e| scene::parent_of(&self.world.ecs, e))
                    .map(GameplayEntity::Live)
            }))
    }

    /// Existing sibling order, with newly reparented children appended in edit order.
    pub fn children(&self, entity: impl Into<GameplayEntity>) -> EditResult<Vec<GameplayEntity>> {
        let entity = entity.into();
        self.require(entity)?;
        let mut children = Vec::new();
        for child in entity
            .live()
            .map(|e| scene::children_of(&self.world.ecs, e))
            .unwrap_or_default()
        {
            let child = GameplayEntity::Live(child);
            if self.contains(child)? && self.parent(child)? == Some(entity) {
                children.push(child);
            }
        }
        for &child in &self.journal.parent_order {
            if self.contains(child)?
                && self.parent(child)? == Some(entity)
                && !children.contains(&child)
            {
                children.push(child);
            }
        }
        Ok(children)
    }

    fn ancestry(&self, entity: GameplayEntity) -> EditResult<Vec<GameplayEntity>> {
        let mut chain = Vec::new();
        let mut seen = BTreeSet::new();
        let mut current = Some(entity);
        while let Some(e) = current {
            if !seen.insert(e) {
                return self.fail(EditError::HierarchyCycle(e));
            }
            chain.push(e);
            current = self.parent(e)?;
        }
        Ok(chain)
    }

    pub fn world_matrix(&self, entity: impl Into<GameplayEntity>) -> EditResult<Mat4> {
        let entity = entity.into();
        let mut matrix = Mat4::IDENTITY;
        for e in self.ancestry(entity)?.into_iter().rev() {
            matrix *= self.local_transform(e)?.matrix();
        }
        Ok(matrix)
    }

    pub fn active_self(&self, entity: impl Into<GameplayEntity>) -> EditResult<bool> {
        let entity = entity.into();
        self.require(entity)?;
        Ok(self
            .journal
            .changes
            .get(&entity)
            .and_then(|c| c.active)
            .unwrap_or_else(|| {
                entity
                    .live()
                    .is_none_or(|e| !self.world.ecs.has::<InactiveSelf>(e))
            }))
    }

    pub fn active_in_hierarchy(&self, entity: impl Into<GameplayEntity>) -> EditResult<bool> {
        let entity = entity.into();
        // Validate the entire chain even if an early ancestor is inactive.
        let mut active = true;
        for e in self.ancestry(entity)? {
            active &= self.active_self(e)?;
        }
        Ok(active)
    }

    pub fn set_name(
        &mut self,
        entity: impl Into<GameplayEntity>,
        name: impl Into<String>,
    ) -> EditResult<()> {
        let entity = entity.into();
        self.require(entity)?;
        self.journal.changes.entry(entity).or_default().name = Some(name.into());
        Ok(())
    }

    pub fn set_transform(
        &mut self,
        entity: impl Into<GameplayEntity>,
        transform: Transform,
    ) -> EditResult<()> {
        let entity = entity.into();
        self.spatial(entity)?;
        if !transform.is_valid() {
            return self.fail(EditError::InvalidTransform(entity));
        }
        self.journal.changes.entry(entity).or_default().transform = Some(transform);
        Ok(())
    }

    pub fn set_active(
        &mut self,
        entity: impl Into<GameplayEntity>,
        active: bool,
    ) -> EditResult<()> {
        let entity = entity.into();
        self.require(entity)?;
        self.journal.changes.entry(entity).or_default().active = Some(active);
        Ok(())
    }

    pub fn set_parent(
        &mut self,
        entity: impl Into<GameplayEntity>,
        parent: Option<GameplayEntity>,
        keep_world: bool,
    ) -> EditResult<()> {
        let entity = entity.into();
        self.spatial(entity)?;
        if let Some(p) = parent {
            self.spatial(p)?;
            if self.ancestry(p)?.contains(&entity) {
                return self.fail(EditError::HierarchyCycle(entity));
            }
        }
        if self.parent(entity)? == parent {
            return Ok(());
        }
        let local = if keep_world {
            let world = self.world_matrix(entity)?;
            let parent_world = match parent {
                Some(p) => self.world_matrix(p)?,
                None => Mat4::IDENTITY,
            };
            let Some(inverse) = scene::inverse_affine(parent_world) else {
                return self.fail(EditError::UnsupportedTransform(entity));
            };
            let Some(local) = Transform::try_from_matrix(inverse * world) else {
                return self.fail(EditError::UnsupportedTransform(entity));
            };
            Some(local)
        } else {
            None
        };
        let change = self.journal.changes.entry(entity).or_default();
        change.parent = Some(parent);
        if let Some(local) = local {
            change.transform = Some(local);
        }
        self.journal.parent_order.retain(|e| *e != entity);
        self.journal.parent_order.push(entity);
        Ok(())
    }
}

impl World {
    /// Deliver a committed subsystem request. Authored mutations cannot enter
    /// this queue: its type contains only body, playback and one-shot effects.
    pub(super) fn deliver_committed_effect(
        &mut self,
        index: u32,
        lane: crate::Lane,
        effect: CommittedEffect,
    ) -> bool {
        match effect {
            CommittedEffect::Body { entity, op } => {
                if !self.module_allows(index, lane, &crate::Command::Body { entity, op }) {
                    return false;
                }
                if matches!(op, crate::BodyCommand::Teleport { .. }) {
                    self.teleported.push(entity);
                }
                self.physics.queue(index, entity, op);
                true
            }
            CommittedEffect::Audio { entity, op } => self.apply_audio_intent(entity, op),
            CommittedEffect::Animation {
                entity,
                op,
                before,
                invalidated,
            } => self.apply_animate_intent(entity, op, before, invalidated),
            CommittedEffect::Sound { clip, params } => {
                if !self.module_allows(
                    index,
                    lane,
                    &crate::Command::PlaySound {
                        clip: clip.clone(),
                        bus: params.bus,
                        volume: params.volume,
                        pitch: params.pitch,
                        priority: params.priority,
                    },
                ) {
                    return false;
                }
                self.apply_play_sound(index, lane, clip, params)
            }
        }
    }

    /// Publishes pure gameplay edits without a tick. Errors (even caught ones)
    /// or callback panics discard the journal. Pending references resolve only
    /// through the returned commit report; aborted tokens expire permanently.
    /// Legacy queued commands must first be drained during this migration.
    /// Completed simulation snapshots remain at their recorded tick.
    ///
    /// Scoped access cannot escape the callback:
    /// ```compile_fail
    /// use fynkel_core::{World, WorldConfig};
    /// let mut world = World::new(WorldConfig::default());
    /// let escaped = world.edit_gameplay(|edit| Ok(edit));
    /// ```
    pub fn edit_gameplay<R>(
        &mut self,
        callback: impl FnOnce(&mut GameplayEdit<'_>) -> EditResult<R>,
    ) -> EditResult<EditCommit<R>> {
        self.edit_gameplay_in_phase(self.clock.tick(), false, callback)
    }

    /// Pre-physics native systems publish events at the tick being computed,
    /// without advancing the completed clock or changing public callback timing.
    /// Body effects reach the upcoming physics stage of this tick; callers must
    /// run after Commands and before PhysicsStep. Geometry settles at native stages.
    pub(super) fn edit_gameplay_at_tick<R>(
        &mut self,
        tick: fynkel_contracts::Tick,
        callback: impl FnOnce(&mut GameplayEdit<'_>) -> EditResult<R>,
    ) -> EditResult<EditCommit<R>> {
        self.edit_gameplay_in_phase(tick, true, callback)
    }

    /// Authored output from a native system. Data publishes atomically, while
    /// geometry/interpolation settle at the declared native stages. Runtime
    /// effects are forbidden here, including after the physics stage.
    pub(super) fn edit_native_data_at_tick<R>(
        &mut self,
        tick: fynkel_contracts::Tick,
        callback: impl FnOnce(&mut GameplayEdit<'_>) -> EditResult<R>,
    ) -> EditResult<EditCommit<R>> {
        self.edit_gameplay_in_phase(tick, true, |edit| {
            let value = callback(edit)?;
            if !edit.journal.effects.is_empty() {
                return edit.fail(EditError::InvalidEffect(
                    GameplayEntity::Live(Entity::NULL),
                    "native data publication does not accept runtime effects".into(),
                ));
            }
            Ok(value)
        })
    }

    fn edit_gameplay_in_phase<R>(
        &mut self,
        tick: fynkel_contracts::Tick,
        native_phase: bool,
        callback: impl FnOnce(&mut GameplayEdit<'_>) -> EditResult<R>,
    ) -> EditResult<EditCommit<R>> {
        let mut journal = self.begin_gameplay_journal()?;
        journal.publication_tick = tick;
        journal.native_phase = native_phase;
        let mut edit = GameplayEdit {
            world: self,
            journal,
        };
        let value = callback(&mut edit)?;
        edit.check()?;
        edit.validate_final_references()?;
        let journal = edit.journal;
        // Only publication mutates committed data; invariant panics poison it.
        let spawned = match catch_unwind(AssertUnwindSafe(|| self.commit_gameplay(journal))) {
            Ok(result) => result?,
            Err(panic) => {
                self.poison();
                resume_unwind(panic);
            }
        };
        Ok(EditCommit { value, spawned })
    }

    fn begin_gameplay_journal(&mut self) -> EditResult<GameplayJournal> {
        if self.poisoned {
            return Err(EditError::Poisoned);
        }
        self.next_gameplay_edit = self
            .next_gameplay_edit
            .checked_add(1)
            .ok_or(EditError::IdentityExhausted)?;
        let journal = GameplayJournal {
            publication_tick: self.clock.tick(),
            native_phase: false,
            scope: (self.id(), self.next_gameplay_edit),
            persistent: Vec::new(),
            next_pending: 1,
            changes: BTreeMap::new(),
            parent_order: Vec::new(),
            effects: Vec::new(),
            audio_buses: AudioBusChanges::default(),
            input_bindings: None,
            removed: BTreeSet::new(),
            prepared_controllers: Default::default(),
            component_revisions: BTreeMap::new(),
            registered_revision: 0,
            failed: RefCell::new(None),
        };
        Ok(journal)
    }

    fn commit_gameplay(
        &mut self,
        journal: GameplayJournal,
    ) -> EditResult<BTreeMap<PendingEntity, Entity>> {
        let GameplayJournal {
            persistent,
            changes,
            parent_order,
            removed,
            effects,
            audio_buses,
            input_bindings,
            publication_tick,
            native_phase,
            ..
        } = journal;
        if persistent.is_empty()
            && changes.is_empty()
            && removed.is_empty()
            && effects.is_empty()
            && audio_buses.muted.is_none()
            && input_bindings.is_none()
        {
            return Ok(BTreeMap::new());
        }
        let mut spawned = BTreeMap::new();
        // Allocate roots before touching existing entities. On slot exhaustion,
        // remove these unpublished roots; no names, events or links have escaped.
        for (&key, change) in &changes {
            if removed.contains(&key) {
                continue;
            }
            if let GameplayEntity::Pending(pending) = key {
                match scene::spawn(&mut self.ecs, change.transform.expect("pending pose"), None) {
                    Ok(entity) => {
                        spawned.insert(pending, entity);
                    }
                    Err(scene::SceneError::Ecs(EcsError::OutOfEntities)) => {
                        for &e in spawned.values() {
                            scene::despawn_subtree(&mut self.ecs, e).expect("unpublished root");
                        }
                        return Err(EditError::OutOfEntities);
                    }
                    Err(error) => panic!("validated spawn failed: {error:?}"),
                }
            }
        }
        let resolve = |key: GameplayEntity| match key {
            GameplayEntity::Live(e) => e,
            GameplayEntity::Pending(p) => spawned[&p],
        };
        let parents: Vec<_> = parent_order
            .into_iter()
            .filter(|e| !removed.contains(e))
            .filter_map(|key| {
                let parent = changes[&key].parent?.map(&resolve);
                let entity = resolve(key);
                (scene::parent_of(&self.ecs, entity) != parent).then_some((key, entity, parent))
            })
            .collect();
        // Detach surviving moved entities before removing old ancestors. Detach
        // retired entities too, so physical old edges cannot delete a survivor
        // or cause duplicate destruction when logical and old hierarchies differ.
        for &(_, entity, _) in &parents {
            scene::set_parent_keep_local(&mut self.ecs, entity, None).expect("validated detach");
        }
        for entity in removed.iter().filter_map(|e| e.live()) {
            scene::set_parent_keep_local(&mut self.ecs, entity, None).expect("retired detach");
        }
        for entity in removed.iter().filter_map(|e| e.live()) {
            self.destroy_subtree(entity, publication_tick)
                .expect("retired entity");
        }
        for (&key, change) in &changes {
            if removed.contains(&key) {
                continue;
            }
            let entity = resolve(key);
            if let Some(name) = &change.name {
                self.ecs
                    .insert(entity, Name::new(name.clone()))
                    .expect("validated entity");
            }
            if let Some(transform) = change.transform {
                scene::set_local(&self.ecs, entity, transform).expect("validated transform");
            }
            if let Some(active) = change.active {
                scene::set_active(&mut self.ecs, entity, active).expect("validated activity");
                self.active.roots.push(entity);
            }
            for (&component, value) in &change.components {
                match value {
                    Some(value) => value
                        .clone()
                        .into_native(|reference| match reference {
                            GameplayObjectRef::Stored(r) => r,
                            GameplayObjectRef::Pending(p) => {
                                fynkel_contracts::ObjectRef::to(spawned[&p])
                            }
                        })
                        .insert_into(&mut self.ecs, entity)
                        .expect("validated component"),
                    None => {
                        // Adding then removing in one journal is a no-op on
                        // storage; removal bookkeeping remains shared with the
                        // legacy data path and only runs at successful commit.
                        if ComponentValue::contains_in(&self.ecs, entity, component) {
                            assert_eq!(self.remove_component_data(entity, component), Some(true));
                        }
                    }
                }
            }
        }
        for &(_, entity, parent) in &parents {
            scene::set_parent_keep_local(&mut self.ecs, entity, parent)
                .expect("validated hierarchy");
            self.active.roots.push(entity);
        }
        self.refresh_active();
        // Native systems retain their declared propagation stages. Callback
        // publication settles immediately for reads between ticks.
        if !native_phase {
            scene::propagate(&mut self.ecs);
        }
        for (&key, change) in &changes {
            if !removed.contains(&key) && (change.transform.is_some() || change.parent.is_some()) {
                let entity = resolve(key);
                if !native_phase && !self.gameplay_tick_prepared {
                    self.teleported.push(entity);
                }
            }
        }
        if !native_phase {
            self.apply_teleports();
        }
        // Creation mappings use the typed report, never legacy wire temp IDs.
        // Only surviving live entities publish final reparent events.
        for (key, child, parent) in parents {
            if matches!(key, GameplayEntity::Live(_)) {
                self.events.push(Event {
                    tick: publication_tick,
                    payload: EventPayload::ParentChanged { child, parent },
                });
            }
        }
        for key in persistent {
            if !removed.contains(&key) {
                let entity = resolve(key);
                if scene::parent_of(&self.ecs, entity).is_none()
                    && !self.persistent.contains(&entity)
                {
                    self.persistent.push(entity);
                }
            }
        }
        for effect in effects {
            match effect {
                GameplayEffect::Body(entity, op) if !removed.contains(&entity) => {
                    let entity = resolve(entity);
                    if native_phase {
                        // Pre-physics native systems have already passed the
                        // Commands stage. Deliver validated effects to this
                        // tick's physics boundary, without next-tick replay.
                        if matches!(op, crate::BodyCommand::Teleport { .. }) {
                            self.teleported.push(entity);
                        }
                        self.physics.queue(u32::MAX, entity, op);
                    } else {
                        self.gameplay_effects
                            .push(CommittedEffect::Body { entity, op });
                    }
                }
                GameplayEffect::Body(..) => {}
                GameplayEffect::Audio(entity, op) if !removed.contains(&entity) => {
                    self.gameplay_effects.push(CommittedEffect::Audio {
                        entity: resolve(entity),
                        op,
                    });
                }
                GameplayEffect::Audio(..) => {}
                GameplayEffect::Animation(entity, op, before) if !removed.contains(&entity) => {
                    self.gameplay_effects.push(CommittedEffect::Animation {
                        entity: resolve(entity),
                        op,
                        before,
                        invalidated: false,
                    });
                }
                GameplayEffect::Animation(..) => {}
                GameplayEffect::Sound(clip, params) => {
                    self.gameplay_effects
                        .push(CommittedEffect::Sound { clip, params });
                }
            }
        }
        for bus in [AudioBus::Master, AudioBus::Music, AudioBus::Sfx] {
            if let Some(gain) = audio_buses.gains[bus.index()] {
                self.audio.set_bus(bus, gain);
            }
        }
        if let Some(muted) = audio_buses.muted {
            self.audio.set_muted(muted);
        }
        if let Some(prepared) = input_bindings {
            self.input.publish_bindings(prepared);
        }
        // Codecs have already run during staging. Publication only moves
        // prepared native values into the existing ECS, with no second store.
        for (key, change) in changes {
            if removed.contains(&key) {
                continue;
            }
            let entity = resolve(key);
            for (id, value) in change.registered {
                match value {
                    Some(value) => value.publish(&mut self.ecs, entity, id, |target| {
                        if removed.contains(&target) {
                            None
                        } else {
                            Some(resolve(target))
                        }
                    }),
                    None => self
                        .registered
                        .get(&id)
                        .expect("validated registration")
                        .remove(&mut self.ecs, entity),
                }
            }
        }
        Ok(spawned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WorldConfig;

    #[cfg(feature = "physics")]
    #[test]
    fn native_data_boundary_rejects_runtime_effects_and_rolls_back_output() {
        let mut world = World::new(WorldConfig::default());
        let commit = world
            .edit_gameplay(|edit| {
                let e = edit.spawn("body", Transform::IDENTITY, None)?;
                edit.set_component(e, ComponentValue::RigidBody(crate::RigidBody::default()))?;
                Ok(e)
            })
            .unwrap();
        let entity = commit.resolve(commit.value).unwrap();
        let tick = world.tick();
        let result = world.edit_native_data_at_tick(tick + 1, |edit| {
            edit.set_name(entity, "discarded")?;
            edit.body_effect(entity, crate::BodyCommand::SetLinearVelocity(glam::Vec3::X))
        });
        assert!(matches!(result, Err(EditError::InvalidEffect(..))));
        assert_eq!(world.ecs.copy::<Name>(entity).unwrap().value, "body");
        assert!(world.gameplay_effects.is_empty());
        world.step();
        assert_eq!(
            world
                .physics()
                .body_state(entity)
                .unwrap()
                .linear_velocity
                .x,
            0.0
        );
    }

    #[test]
    fn transaction_identity_exhaustion_is_permanent_and_does_not_run_callbacks() {
        let mut w = World::new(WorldConfig::default());
        w.next_gameplay_edit = u64::MAX;
        for _ in 0..2 {
            assert_eq!(
                w.edit_gameplay::<()>(|_| panic!("exhausted callback")),
                Err(EditError::IdentityExhausted)
            );
            assert_eq!(w.next_gameplay_edit, u64::MAX);
        }
        assert!(!w.is_poisoned());
    }

    #[test]
    fn pending_identity_exhaustion_is_sticky_and_discards_earlier_edits() {
        let mut w = World::new(WorldConfig::default());
        let entity = scene::spawn(w.ecs_mut(), Transform::IDENTITY, None).unwrap();
        assert_eq!(
            w.edit_gameplay(|edit| {
                edit.set_name(entity, "must not publish")?;
                edit.journal.next_pending = u32::MAX;
                let _ = edit.spawn("cannot allocate", Transform::IDENTITY, None);
                Ok(())
            }),
            Err(EditError::IdentityExhausted)
        );
        assert!(!w.ecs().has::<Name>(entity));
        assert_eq!(w.live_entities(), 1);
    }
    #[test]
    fn owned_session_restores_journal_after_callback_panic_and_aborts() {
        let world = World::new(WorldConfig::default());
        let mut session = GameplaySession::begin(world).unwrap_or_else(|_| panic!("begin"));
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _: EditResult<()> = session.access(|edit| {
                edit.spawn("discarded", Transform::default(), None)?;
                panic!("host callback");
            });
        }));
        assert!(result.is_err());
        let (world, result) = session.finish();
        assert_eq!(result, Err(EditError::Aborted));
        assert_eq!(world.live_entities(), 0);
        assert!(!world.is_poisoned());
    }

    #[test]
    fn owned_session_shares_one_journal_across_exchanges_and_returns_world_on_abort() {
        let world = World::new(WorldConfig::default());
        let id = world.id();
        let mut session = GameplaySession::begin(world).unwrap_or_else(|_| panic!("begin"));
        let pending = session
            .access(|edit| edit.spawn("pending", Transform::default(), None))
            .unwrap();
        session
            .access(|edit| {
                assert_eq!(edit.find("pending")?, Some(pending.into()));
                edit.set_name(pending, "renamed")
            })
            .unwrap();
        assert_eq!(session.committed().live_entities(), 0);
        let (mut world, result) = session.finish();
        assert_eq!(world.id(), id);
        let entity = result.unwrap().resolve(pending).unwrap();
        assert_eq!(world.ecs().get::<Name>(entity).unwrap().value, "renamed");
        world
            .edit_gameplay(|edit| {
                assert_eq!(edit.find("renamed")?, Some(entity.into()));
                Ok(())
            })
            .unwrap();
        let mut session = GameplaySession::begin(world).unwrap_or_else(|_| panic!("begin"));
        session.access(|edit| edit.destroy(entity)).unwrap();
        let world = session.abort();
        assert!(world.ecs().contains(entity));
    }
    #[test]
    fn owned_session_returns_poisoned_world_even_when_final_validation_panics() {
        let world = World::new(WorldConfig::default());
        let id = world.id();
        let mut session = GameplaySession::begin(world).unwrap_or_else(|_| panic!("begin"));
        // Force an internal invariant failure at the final validation boundary.
        session.journal = None;
        let (world, result) = session.finish();
        assert_eq!(world.id(), id);
        assert!(world.is_poisoned());
        assert_eq!(result, Err(EditError::Poisoned));
    }
}
