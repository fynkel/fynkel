//! ECS facade (blueprint §4, ADR-0003).
//!
//! `hecs` provides storage and queries; Fynkel owns identity. Every entity is
//! addressed from outside by a [`Entity`] handle (world id + slot +
//! generation) that this crate maps to the current `hecs::Entity`. The
//! mapping is the only place the two meet.
//!
//! Visibility rule: no `pub` item outside [`internal`] names a `hecs` type.
//! Engine systems that need raw queries go through `internal` and stay
//! inside the runtime crates; hosts, the ABI and documents never do
//! (`tests/contract.rs` enforces the first half mechanically).

use fynkel_contracts::{allocate_world_id, Entity, IdentityExhausted, WorldId};

pub mod internal {
    //! Engine-internal access to the underlying `hecs` world for systems
    //! (queries, iteration). Not part of any public contract.
    //! Raw storage references are deliberately unavailable:
    //!
    //! ```compile_fail
    //! let mut world = fynkel_ecs::World::new();
    //! world.hecs_mut().clear();
    //! ```
    //! ```compile_fail
    //! let world = fynkel_ecs::World::new();
    //! world.hecs().query::<&mut i32>();
    //! ```
    pub use hecs;

    impl super::World {
        /// Shared queries cannot produce mutable component references.
        ///
        /// ```compile_fail
        /// let world = fynkel_ecs::World::new();
        /// let _ = world.query::<&mut i32>();
        /// ```
        pub fn query<Q: hecs::Query + hecs::QueryShared>(&self) -> hecs::QueryBorrow<'_, Q> {
            self.inner.query::<Q>()
        }

        /// A bulk write lease.
        ///
        /// ```compile_fail
        /// let mut world = fynkel_ecs::World::new();
        /// let mut query = world.query_mut::<&mut i32>();
        /// let value = query.into_iter().next().unwrap();
        /// let stamp = world.change_stamp();
        /// *value += 1;
        /// ```
        ///
        /// Conservatively invalidates component values even
        /// if the query is empty. Exclusive access prevents observing a change
        /// stamp until every reference yielded by the query has expired.
        pub fn query_mut<Q: hecs::Query>(&mut self) -> hecs::QueryMut<'_, Q> {
            use hecs::Fetch;
            let changes = self.changes.get_mut().unwrap();
            changes.advance(false, false);
            Q::Fetch::for_each_borrow(|ty, unique| {
                if unique {
                    changes.components.insert(ty, changes.values);
                }
            });
            self.inner.query_mut::<Q>()
        }
        /// Fynkel handle of a raw `hecs` entity (every spawned entity carries one).
        pub fn handle_of(&self, raw: hecs::Entity) -> Option<super::Entity> {
            self.inner.get::<&Handle>(raw).ok().map(|h| h.0)
        }
        /// Raw `hecs` entity behind a validated handle.
        pub fn raw(&self, entity: super::Entity) -> Option<hecs::Entity> {
            self.resolve(entity)
        }
    }

    /// Reverse-mapping component attached to every entity.
    #[derive(Clone, Copy, Debug)]
    pub struct Handle(pub super::Entity);
}

use internal::Handle;

/// Anything storable as a component. Blanket-implemented.
pub trait Component: Send + Sync + 'static {}
impl<T: Send + Sync + 'static> Component for T {}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EcsError {
    /// Handle is NULL, from another world, or its generation is stale.
    StaleEntity(Entity),
    OutOfEntities,
    /// `get`/`remove` of a component the entity does not have.
    MissingComponent(Entity),
}

pub type Result<T> = core::result::Result<T, EcsError>;

/// Shared borrow of a component. Holds the storage lock; drop before mutating the world.
pub struct Ref<'a, T: Component> {
    inner: hecs::Ref<'a, T>,
}
impl<T: Component> core::ops::Deref for Ref<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.inner
    }
}

/// Exclusive borrow of a component. Acquiring it invalidates value stamps;
/// no settled stamp is available until all such guards have dropped.
pub struct RefMut<'a, T: Component> {
    inner: hecs::RefMut<'a, T>,
    changes: &'a std::sync::Mutex<Changes>,
}
impl<T: Component> core::ops::Deref for RefMut<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.inner
    }
}
impl<T: Component> core::ops::DerefMut for RefMut<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

impl<T: Component> Drop for RefMut<'_, T> {
    fn drop(&mut self) {
        self.changes.lock().unwrap().writers -= 1;
    }
}

/// Opaque, world-specific stamp of settled ECS storage. Compare for equality;
/// for a snapshot built through shared access, compare stamps before and after
/// its reads and publish only if both are present and equal. A stamp is not a
/// lock: it cannot make subsequent concurrent reads atomic.
///
/// Counters represent possible writes, not a count of changed entities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChangeStamp {
    world: WorldId,
    entities: u64,
    structure: u64,
    values: u64,
}

impl ChangeStamp {
    /// Same entity membership (including generations).
    pub fn same_entities(self, other: Self) -> bool {
        self.world == other.world && self.entities == other.entities
    }

    /// Same entity membership and component presence.
    pub fn same_structure(self, other: Self) -> bool {
        self.same_entities(other) && self.structure == other.structure
    }
}

/// Settled entity membership and one component type's possible writes.
/// Subject to the same publication and interior-mutation rules as ChangeStamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComponentStamp {
    world: WorldId,
    component: std::any::TypeId,
    entities: u64,
    values: u64,
}

#[derive(Default)]
struct Changes {
    entities: u64,
    structure: u64,
    values: u64,
    writers: usize,
    components: std::collections::HashMap<std::any::TypeId, u64>,
}

impl Changes {
    fn advance(&mut self, entities: bool, structure: bool) {
        // Exhaustion fails before granting a write, never silently aliases an
        // old stamp. No component write is granted after exhaustion.
        self.values = self.values.checked_add(1).expect("ECS revision exhausted");
        if structure {
            self.structure = self
                .structure
                .checked_add(1)
                .expect("ECS revision exhausted");
        }
        if entities {
            self.entities = self
                .entities
                .checked_add(1)
                .expect("ECS revision exhausted");
        }
    }
}

/// Collects components for one `spawn`.
#[derive(Default)]
pub struct EntityBuilder {
    inner: hecs::EntityBuilder,
}

impl EntityBuilder {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add<T: Component>(&mut self, component: T) -> &mut Self {
        self.inner.add(component);
        self
    }
    pub fn with<T: Component>(mut self, component: T) -> Self {
        self.inner.add(component);
        self
    }
}

struct Slot {
    /// Starts at 1; bumped on despawn so old handles fail (`Entity::next_generation`).
    generation: u32,
    raw: Option<hecs::Entity>,
}

pub struct World {
    id: WorldId,
    changes: std::sync::Mutex<Changes>,
    inner: hecs::World,
    slots: Vec<Slot>,
    free: Vec<u32>,
    live: u32,
    /// `&self` so it can be queued while iterating or holding component borrows.
    deferred_despawn: std::sync::Mutex<Vec<Entity>>,
}

impl World {
    pub fn new() -> Self {
        Self::try_new().expect("world identity exhausted")
    }

    pub fn try_new() -> core::result::Result<Self, IdentityExhausted> {
        let id = allocate_world_id()?;
        Ok(Self {
            id,
            changes: std::sync::Mutex::new(Changes::default()),
            inner: hecs::World::new(),
            slots: Vec::new(),
            free: Vec::new(),
            live: 0,
            deferred_despawn: std::sync::Mutex::new(Vec::new()),
        })
    }

    pub fn id(&self) -> WorldId {
        self.id
    }

    /// Returns no stamp while a shared-access mutable component guard exists.
    /// Cache users must then defer reuse/rebuilding until writes are settled.
    /// Interior mutation through a component's own atomics/locks is outside
    /// storage tracking; such state needs its own publication contract.
    pub fn change_stamp(&self) -> Option<ChangeStamp> {
        let changes = self.changes.lock().unwrap();
        (changes.writers == 0).then_some(ChangeStamp {
            world: self.id,
            entities: changes.entities,
            structure: changes.structure,
            values: changes.values,
        })
    }

    /// Tracks entity membership plus writes/presence changes of T, including
    /// optional and filtered bulk query writes. Unrelated writes do not
    /// invalidate this stamp. No stamp is returned during outstanding guards.
    pub fn component_stamp<T: Component>(&self) -> Option<ComponentStamp> {
        let changes = self.changes.lock().unwrap();
        let component = std::any::TypeId::of::<T>();
        (changes.writers == 0).then_some(ComponentStamp {
            world: self.id,
            component,
            entities: changes.entities,
            values: changes.components.get(&component).copied().unwrap_or(0),
        })
    }

    pub fn len(&self) -> u32 {
        self.live
    }
    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    fn resolve(&self, entity: Entity) -> Option<hecs::Entity> {
        if entity.is_null() || entity.world() != self.id {
            return None;
        }
        let slot = self.slots.get(entity.slot() as usize)?;
        if slot.generation != entity.generation() {
            return None;
        }
        slot.raw
    }

    fn require(&self, entity: Entity) -> Result<hecs::Entity> {
        self.resolve(entity).ok_or(EcsError::StaleEntity(entity))
    }

    pub fn contains(&self, entity: Entity) -> bool {
        self.resolve(entity).is_some()
    }

    pub fn spawn(&mut self, builder: EntityBuilder) -> Entity {
        self.try_spawn(builder).expect("entity slots exhausted")
    }

    pub fn try_spawn(&mut self, mut builder: EntityBuilder) -> Result<Entity> {
        if self.live == u32::MAX {
            return Err(EcsError::OutOfEntities);
        }
        let slot_index = match self.free.pop() {
            Some(i) => i,
            None => {
                let index = u32::try_from(self.slots.len()).map_err(|_| EcsError::OutOfEntities)?;
                self.slots.push(Slot {
                    generation: 1,
                    raw: None,
                });
                index
            }
        };
        let slot = &mut self.slots[slot_index as usize];
        let entity = Entity::new(self.id, slot_index, slot.generation);
        self.changes.get_mut().unwrap().advance(true, true);
        builder.inner.add(Handle(entity));
        let raw = self.inner.spawn(builder.inner.build());
        slot.raw = Some(raw);
        self.live += 1;
        Ok(entity)
    }

    pub fn spawn_empty(&mut self) -> Entity {
        self.spawn(EntityBuilder::new())
    }

    /// Immediate removal. Use [`despawn_deferred`](Self::despawn_deferred)
    /// from inside iteration.
    pub fn despawn(&mut self, entity: Entity) -> Result<()> {
        let raw = self.require(entity)?;
        self.changes.get_mut().unwrap().advance(true, true);
        self.inner
            .despawn(raw)
            .expect("slot mapping and hecs disagree");
        let slot = &mut self.slots[entity.slot() as usize];
        slot.raw = None;
        if let Some(next) = Entity::next_generation(slot.generation) {
            slot.generation = next;
            self.free.push(entity.slot());
        }
        self.live -= 1;
        Ok(())
    }

    /// Queued until [`flush_deferred`](Self::flush_deferred); a stale handle
    /// at flush time is ignored (double-destroy is not an error).
    pub fn despawn_deferred(&self, entity: Entity) {
        self.deferred_despawn.lock().unwrap().push(entity);
    }

    /// The queued despawns, taken off the queue without applying them: for
    /// a caller that removes more than the entity (a hierarchy's subtree,
    /// its bookkeeping) — `fynkel_core`'s tick does (foundation 3.10).
    pub fn take_deferred(&mut self) -> Vec<Entity> {
        core::mem::take(&mut *self.deferred_despawn.lock().unwrap())
    }

    /// Applies queued structural changes; returns the entities that were live.
    /// Each goes alone: this crate knows no hierarchy (see
    /// [`take_deferred`](Self::take_deferred)).
    pub fn flush_deferred(&mut self) -> Vec<Entity> {
        let pending = core::mem::take(&mut *self.deferred_despawn.lock().unwrap());
        let mut done = Vec::with_capacity(pending.len());
        for e in pending {
            if self.despawn(e).is_ok() {
                done.push(e);
            }
        }
        done
    }

    pub fn insert<T: Component>(&mut self, entity: Entity, component: T) -> Result<()> {
        let raw = self.require(entity)?;
        let structural = !self.inner.satisfies::<&T>(raw);
        let changes = self.changes.get_mut().unwrap();
        changes.advance(false, structural);
        changes
            .components
            .insert(std::any::TypeId::of::<T>(), changes.values);
        self.inner.insert_one(raw, component).expect("live entity");
        Ok(())
    }

    pub fn remove<T: Component>(&mut self, entity: Entity) -> Result<T> {
        let raw = self.require(entity)?;
        if !self.inner.satisfies::<&T>(raw) {
            return Err(EcsError::MissingComponent(entity));
        }
        let changes = self.changes.get_mut().unwrap();
        changes.advance(false, true);
        changes
            .components
            .insert(std::any::TypeId::of::<T>(), changes.values);
        self.inner
            .remove_one::<T>(raw)
            .map_err(|_| EcsError::MissingComponent(entity))
    }

    pub fn has<T: Component>(&self, entity: Entity) -> bool {
        self.resolve(entity)
            .is_some_and(|raw| self.inner.satisfies::<&T>(raw))
    }

    pub fn get<T: Component>(&self, entity: Entity) -> Result<Ref<'_, T>> {
        let raw = self.require(entity)?;
        self.inner
            .get::<&T>(raw)
            .map(|inner| Ref { inner })
            .map_err(|_| EcsError::MissingComponent(entity))
    }

    pub fn get_mut<T: Component>(&self, entity: Entity) -> Result<RefMut<'_, T>> {
        let raw = self.require(entity)?;
        self.inner
            .get::<&mut T>(raw)
            .map(|inner| {
                let mut changes = self.changes.lock().unwrap();
                changes.advance(false, false);
                let revision = changes.values;
                changes
                    .components
                    .insert(std::any::TypeId::of::<T>(), revision);
                changes.writers = changes
                    .writers
                    .checked_add(1)
                    .expect("ECS writers exhausted");
                RefMut {
                    inner,
                    changes: &self.changes,
                }
            })
            .map_err(|_| EcsError::MissingComponent(entity))
    }

    /// Copies a component out (for snapshots and tests).
    pub fn copy<T: Component + Clone>(&self, entity: Entity) -> Result<T> {
        self.get::<T>(entity).map(|r| r.clone())
    }

    /// All live handles, in slot order (deterministic).
    pub fn entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.raw.map(|_| Entity::new(self.id, i as u32, s.generation)))
    }

    /// Destroys every entity; slots keep their generations so handles that
    /// survive a clear still fail.
    pub fn clear(&mut self) {
        let all: Vec<Entity> = self.entities().collect();
        for e in all {
            let _ = self.despawn(e);
        }
        self.deferred_despawn.lock().unwrap().clear();
    }
}

impl Drop for World {
    fn drop(&mut self) {
        // hecs drops component storage; nothing else is owned.
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;
    #[test]
    fn revision_exhaustion_fails_closed_before_writing() {
        let mut world = World::new();
        let e = world.spawn(EntityBuilder::new().with(1_i32));
        world.changes.get_mut().unwrap().values = u64::MAX;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            world.get_mut::<i32>(e).unwrap();
        }));
        assert!(result.is_err());
        assert_eq!(world.copy::<i32>(e).unwrap(), 1);
        assert!(world.changes.is_poisoned());
    }
    #[test]
    fn maximum_generation_retires_actual_ecs_slot() {
        let mut world = World::new();
        let first = world.spawn_empty();
        world.slots[first.slot() as usize].generation = u32::MAX;
        let last = Entity::new(world.id(), first.slot(), u32::MAX);
        assert!(world.contains(last));
        world.despawn(last).unwrap();
        assert!(world.free.is_empty());
        let next = world.spawn_empty();
        assert_ne!(last.slot(), next.slot());
        assert!(!world.contains(first));
        assert!(!world.contains(last));
    }
    #[test]
    fn slot_exhaustion_does_not_mutate_storage() {
        let mut world = World::new();
        world.live = u32::MAX;
        assert_eq!(
            world.try_spawn(EntityBuilder::new()),
            Err(EcsError::OutOfEntities)
        );
        assert!(world.slots.is_empty());
        assert!(world.inner.is_empty());
    }
}
