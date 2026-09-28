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

use fynkel_contracts::{Entity, WorldId};

pub mod internal {
    //! Engine-internal access to the underlying `hecs` world for systems
    //! (queries, iteration). Not part of any public contract.
    pub use hecs;

    impl super::World {
        pub fn hecs(&self) -> &hecs::World {
            &self.inner
        }
        pub fn hecs_mut(&mut self) -> &mut hecs::World {
            &mut self.inner
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

/// Exclusive borrow of a component.
pub struct RefMut<'a, T: Component> {
    inner: hecs::RefMut<'a, T>,
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
    inner: hecs::World,
    slots: Vec<Slot>,
    free: Vec<u32>,
    live: u32,
    /// `&self` so it can be queued while iterating or holding component borrows.
    deferred_despawn: std::sync::Mutex<Vec<Entity>>,
}

impl World {
    pub fn new(id: WorldId) -> Self {
        assert!(id != WorldId::NONE, "WorldId(0) is reserved");
        Self {
            id,
            inner: hecs::World::new(),
            slots: Vec::new(),
            free: Vec::new(),
            live: 0,
            deferred_despawn: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn id(&self) -> WorldId {
        self.id
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

    pub fn spawn(&mut self, mut builder: EntityBuilder) -> Entity {
        let slot_index = match self.free.pop() {
            Some(i) => i,
            None => {
                self.slots.push(Slot {
                    generation: 1,
                    raw: None,
                });
                (self.slots.len() - 1) as u32
            }
        };
        let slot = &mut self.slots[slot_index as usize];
        let entity = Entity::new(self.id, slot_index, slot.generation);
        builder.inner.add(Handle(entity));
        let raw = self.inner.spawn(builder.inner.build());
        slot.raw = Some(raw);
        self.live += 1;
        entity
    }

    pub fn spawn_empty(&mut self) -> Entity {
        self.spawn(EntityBuilder::new())
    }

    /// Immediate removal. Use [`despawn_deferred`](Self::despawn_deferred)
    /// from inside iteration.
    pub fn despawn(&mut self, entity: Entity) -> Result<()> {
        let raw = self.require(entity)?;
        self.inner
            .despawn(raw)
            .expect("slot mapping and hecs disagree");
        let slot = &mut self.slots[entity.slot() as usize];
        slot.raw = None;
        slot.generation = Entity::next_generation(slot.generation);
        self.free.push(entity.slot());
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
        self.inner.insert_one(raw, component).expect("live entity");
        Ok(())
    }

    pub fn remove<T: Component>(&mut self, entity: Entity) -> Result<T> {
        let raw = self.require(entity)?;
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
            .map(|inner| RefMut { inner })
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
