//! Scene → runtime instantiation (blueprint §19.2c item 5).
//!
//! A [`SceneDescription`] is the typed, already-validated content of a
//! `.scene.json` (built by `fynkel_document::runtime`); this module knows
//! nothing about JSON. [`World::load_scene`] spawns the hierarchy in
//! document order (roots in order, children in their listed order) and
//! stages component payloads in the shared transaction before publication.
//! Loading owns forward-reference resolution; invalid payloads reject the load.
//! The returned [`LoadedScene`] maps authoring ids to
//! runtime handles both ways.

use std::collections::{HashMap, HashSet};

use fynkel_contracts::{ComponentId, Entity, ObjectRef, Uuid};
use fynkel_scene::{self as scene, Transform};

use crate::components::ComponentValue;
use crate::event::EventPayload;
use crate::world::{Diagnostic, Name, World};
use crate::{EditResult, GameplayComponentValue, GameplayEdit, GameplayObjectRef, PendingEntity};

#[derive(Clone, Debug, PartialEq)]
pub struct EntityDescription {
    pub id: Uuid,
    pub name: String,
    /// The entity's own flag (activeSelf); the subtree under an inactive
    /// entity is inactive in the hierarchy.
    pub active: bool,
    pub parent: Option<Uuid>,
    /// Ordered; every child's `parent` must be this entity.
    pub children: Vec<Uuid>,
    /// At most one value per component type; a missing `Transform` means
    /// identity.
    pub components: Vec<ComponentValue>,
    pub registered: Vec<crate::RegisteredDescription>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SceneDescription {
    pub id: Uuid,
    pub name: String,
    /// Document order. Roots are the entries with `parent: None`.
    pub entities: Vec<EntityDescription>,
}

/// Structural problems the document validator would have reported; the
/// world refuses the whole load rather than spawning half a hierarchy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    Poisoned,
    /// Shared transaction validation rejected the complete scene operation.
    SceneRejected(String),
    DuplicateEntity(Uuid),
    UnknownParent {
        entity: Uuid,
        parent: Uuid,
    },
    UnknownChild {
        entity: Uuid,
        child: Uuid,
    },
    /// `child` is listed under `entity` but its `parent` says otherwise, or
    /// an entity's parent does not list it (exactly once).
    ChildParentMismatch {
        entity: Uuid,
        child: Uuid,
    },
    /// Not reachable from any root (a parent cycle).
    Unreachable(Uuid),
    DuplicateComponent {
        entity: Uuid,
        component: ComponentId,
    },
    /// The ECS refused a spawn (a stale parent handle mid-load).
    Spawn(Uuid),
    /// `instantiate`: the anchor's parent is not a live entity of this world.
    UnknownAnchorParent(Entity),
    /// `instantiate`: the anchor's transform is not finite.
    AnchorTransform,
    /// `instantiate`: the prefab does not have exactly one root (ADR-0022).
    NotOneRoot(Uuid),
    /// An additive load named a scene the world already holds (M3-12).
    AlreadyLoaded(Uuid),
    /// `unload_scene` named a scene the world does not hold (M3-12).
    NotLoaded(Uuid),
    /// `clone_subtree`: the source is not a live entity of this world.
    UnknownSource(Entity),
    /// The shared gameplay journal rejected a clone.
    CloneRejected(String),
    /// Atomic live creation failed before publishing the entity or scene identity.
    LiveSpawnRejected(String),
    /// `dont_destroy_on_load`: the entity has a parent; only a root can be
    /// kept across loads (ADR-0037, Unity's rule).
    NotARoot(Entity),
}

impl core::fmt::Display for LoadError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            LoadError::SceneRejected(message) => write!(f, "scene transaction: {message}"),
            LoadError::Poisoned => write!(f, "world is poisoned"),
            LoadError::DuplicateEntity(id) => write!(f, "entity {id} appears twice"),
            LoadError::UnknownParent { entity, parent } => {
                write!(f, "entity {entity}: parent {parent} does not exist")
            }
            LoadError::UnknownChild { entity, child } => {
                write!(f, "entity {entity}: child {child} does not exist")
            }
            LoadError::ChildParentMismatch { entity, child } => write!(
                f,
                "entity {entity} and child {child} disagree about their relation"
            ),
            LoadError::Unreachable(id) => write!(f, "entity {id} is in a parent cycle"),
            LoadError::DuplicateComponent { entity, component } => write!(
                f,
                "entity {entity}: {} appears twice",
                component.name().unwrap_or("component")
            ),
            LoadError::Spawn(id) => write!(f, "entity {id}: spawn failed"),
            LoadError::UnknownAnchorParent(e) => {
                write!(
                    f,
                    "instantiate: parent {e} is not a live entity of this world"
                )
            }
            LoadError::AnchorTransform => write!(f, "instantiate: transform is not finite"),
            LoadError::NotOneRoot(id) => {
                write!(
                    f,
                    "instantiate: prefab {id} does not have exactly one root (recook it)"
                )
            }
            LoadError::AlreadyLoaded(id) => write!(f, "scene {id} is already loaded"),
            LoadError::NotLoaded(id) => write!(f, "scene {id} is not loaded"),
            LoadError::UnknownSource(e) => {
                write!(f, "clone: source {e} is not a live entity of this world")
            }
            LoadError::CloneRejected(message) => write!(f, "clone: {message}"),
            LoadError::LiveSpawnRejected(message) => write!(f, "live spawn: {message}"),
            LoadError::NotARoot(e) => write!(
                f,
                "dont_destroy_on_load: {e} has a parent; only a root object can be kept across loads"
            ),
        }
    }
}

impl std::error::Error for LoadError {}

/// What `World::clone_subtree` made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cloned {
    pub root: Entity,
    /// Every (source, copy) pair, breadth first, the root first.
    pub pairs: Vec<(Entity, Entity)>,
    /// Always zero: the shared journal rejects an invalid clone atomically.
    pub rejected_components: u32,
}

/// Where and as what an instantiated prefab's root spawns.
struct Placed<'a> {
    root: Uuid,
    name: &'a str,
    transform: Transform,
}

/// Handles of a loaded scene, queryable both ways.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LoadedScene {
    pub id: Uuid,
    pub name: String,
    /// Spawn order (document order, parents before children).
    order: Vec<(Uuid, Entity)>,
    by_id: HashMap<Uuid, Entity>,
    by_entity: HashMap<Entity, Uuid>,
    /// Always zero: invalid component values reject the complete transaction.
    pub rejected_components: u32,
    /// Object references (ADR-0035) whose target is not in the load: they
    /// are None. For the load report.
    pub missing_references: Vec<MissingReference>,
}

/// An object reference a load could not resolve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissingReference {
    /// The authoring id of the entity holding the reference.
    pub entity: Uuid,
    /// The component's schema name and the field, `fynkel.Follow.target`.
    pub component: &'static str,
    pub field: &'static str,
    /// The authoring id it names.
    pub target: Uuid,
}

impl LoadedScene {
    pub fn handle_of(&self, id: Uuid) -> Option<Entity> {
        self.by_id.get(&id).copied()
    }

    pub fn id_of(&self, entity: Entity) -> Option<Uuid> {
        self.by_entity.get(&entity).copied()
    }

    /// `(authoring id, handle)` in spawn order.
    pub fn entities(&self) -> &[(Uuid, Entity)] {
        &self.order
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

/// How `World::load_scene_with` treats what the world already holds
/// (M3-12, blueprint §19.2d item 12).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LoadMode {
    /// Unload every loaded scene and destroy every other entity first — the
    /// world starts the new scene empty. The default of `load_scene`.
    #[default]
    Replace,
    /// Keep everything; the scene's id must not be loaded already.
    Additive,
}

/// What `World::unload_scene` (or a replace load) took down.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UnloadedScene {
    pub id: Uuid,
    pub name: String,
    /// Entities destroyed (the scene's still-alive ones with their
    /// subtrees; one `Destroyed` event each).
    pub entities: u32,
}

impl World {
    /// Instantiates `scene` now, at the current tick boundary, in
    /// [`LoadMode::Replace`]: every loaded scene is unloaded and every
    /// other entity destroyed first. Emits `SceneReady { scene }`.
    /// Structure, components and live references validate in one transaction.
    /// Rejection preserves the previous world and emits no lifecycle events.
    pub fn load_scene(&mut self, scene: &SceneDescription) -> Result<LoadedScene, LoadError> {
        self.load_scene_with(scene, LoadMode::Replace)
    }

    /// [`load_scene`](World::load_scene) with an explicit mode. Additive
    /// keeps the world and refuses an id already loaded
    /// ([`LoadError::AlreadyLoaded`]).
    pub fn load_scene_with(
        &mut self,
        scene: &SceneDescription,
        mode: LoadMode,
    ) -> Result<LoadedScene, LoadError> {
        self.load_scene_prepared(scene, mode, None)
    }

    /// Publishes scene data and an optional owned resource plan at one boundary.
    /// Preflight and staged controller definitions run before the shared journal
    /// commits. Rejection leaves entities, scene metadata and registries intact.
    pub fn load_scene_prepared(
        &mut self,
        scene: &SceneDescription,
        mode: LoadMode,
        resources: Option<Box<dyn crate::PreparedCommandResources>>,
    ) -> Result<LoadedScene, LoadError> {
        self.load_scene_replacing(scene, mode, resources, &[])
    }

    /// As scene activation, additionally retiring host-owned roots in the same
    /// transaction. Used when replacing a project's persistent objects.
    pub fn load_scene_replacing(
        &mut self,
        scene: &SceneDescription,
        mode: LoadMode,
        resources: Option<Box<dyn crate::PreparedCommandResources>>,
        retire: &[Entity],
    ) -> Result<LoadedScene, LoadError> {
        if self.is_poisoned() {
            return Err(LoadError::Poisoned);
        }
        let order = spawn_order(scene)?;
        if mode == LoadMode::Additive && self.lifecycle.scenes().iter().any(|s| s.id == scene.id) {
            return Err(LoadError::AlreadyLoaded(scene.id));
        }
        if let Some(resources) = &resources {
            resources
                .preflight(self)
                .map_err(|e| LoadError::SceneRejected(e.to_string()))?;
        }
        let mut removed = HashSet::new();
        let mut unloads = Vec::new();
        if mode == LoadMode::Replace {
            for old in self.lifecycle.scenes().iter().rev() {
                let before = removed.len();
                self.collect_scene_removals(old, &mut removed);
                unloads.push((old.id, (removed.len() - before) as u32));
            }
            for entity in self.ecs().entities() {
                if !self.is_persistent(entity) {
                    removed.insert(entity);
                }
            }
        }
        let commit = self
            .edit_gameplay(|edit| {
                if let Some(resources) = &resources {
                    resources.stage(edit)?;
                }
                // The whole replacement is staged before anything is destroyed.
                for &entity in &removed {
                    if edit.contains(entity)? {
                        edit.destroy(entity)?;
                    }
                }
                for &entity in retire {
                    if edit.contains(entity)? {
                        edit.destroy(entity)?;
                    }
                }
                stage_description(edit, scene, &order, None, None)
            })
            .map_err(|e| LoadError::SceneRejected(e.to_string()))?;
        self.publish_command_resources(resources);
        let loaded = resolve_description(scene, &commit);
        if mode == LoadMode::Replace {
            self.lifecycle.scenes_mut().clear();
            *self.lifecycle.persistent_mut() = self.persistent_roots();
            for (scene, entities) in unloads {
                self.push_event(EventPayload::SceneUnloaded { scene, entities });
            }
        }
        self.lifecycle.scenes_mut().push(loaded.clone());
        self.push_event(EventPayload::SceneReady { scene: scene.id });
        Ok(loaded)
    }

    /// Destroys the still-alive entities of the loaded scene `id` (with
    /// their subtrees, `Destroyed` events as for `Command::Destroy`) and
    /// forgets it; emits `SceneUnloaded`. Entities the scene's
    /// behaviours spawned since are not the scene's and stay.
    pub fn unload_scene(&mut self, id: Uuid) -> Result<UnloadedScene, LoadError> {
        if self.is_poisoned() {
            return Err(LoadError::Poisoned);
        }
        let i = self
            .lifecycle
            .scenes()
            .iter()
            .position(|s| s.id == id)
            .ok_or(LoadError::NotLoaded(id))?;
        let mut removed = HashSet::new();
        self.collect_scene_removals(&self.lifecycle.scenes()[i], &mut removed);
        self.edit_gameplay(|edit| {
            for &entity in &removed {
                if edit.contains(entity)? {
                    edit.destroy(entity)?;
                }
            }
            Ok(())
        })
        .map_err(|e| LoadError::SceneRejected(e.to_string()))?;
        let old = self.lifecycle.scenes_mut().remove(i);
        let entities = removed.len() as u32;
        self.push_event(EventPayload::SceneUnloaded {
            scene: id,
            entities,
        });
        Ok(UnloadedScene {
            id,
            name: old.name,
            entities,
        })
    }

    /// Spawns one entity now, at the current tick, the way a scene load
    /// does — the editor's live Play mode (ADR-0021 P7) creating an object
    /// in a running, possibly paused, game: `name`, a local `transform`,
    /// last among `parent`'s children, then `components` through the
    /// shared journal's checks. No system runs and no event is emitted
    /// (a load emits none per entity either). With `authoring` =
    /// `(scene, id)` the entity joins the loaded scene `scene` under the
    /// authoring id `id`, as if that scene had spawned it. Returns the
    /// entity and an empty compatibility diagnostics list. Invalid components
    /// reject the entire creation through the gameplay journal, before publishing
    /// an entity or scene identity.
    pub fn spawn_now(
        &mut self,
        name: &str,
        transform: Transform,
        parent: Option<Entity>,
        components: Vec<ComponentValue>,
        authoring: Option<(Uuid, Uuid)>,
    ) -> Result<(Entity, Vec<Diagnostic>), LoadError> {
        self.spawn_now_registered(name, transform, parent, components, Vec::new(), authoring)
    }

    pub fn spawn_now_registered(
        &mut self,
        name: &str,
        transform: Transform,
        parent: Option<Entity>,
        components: Vec<ComponentValue>,
        registered: Vec<crate::RegisteredDescription>,
        authoring: Option<(Uuid, Uuid)>,
    ) -> Result<(Entity, Vec<Diagnostic>), LoadError> {
        if self.is_poisoned() {
            return Err(LoadError::Poisoned);
        }
        validate_registered_descriptions(&registered)?;
        let scene_index = match authoring {
            None => None,
            Some((scene, id)) => {
                let i = self
                    .lifecycle
                    .scenes()
                    .iter()
                    .position(|s| s.id == scene)
                    .ok_or(LoadError::NotLoaded(scene))?;
                if self
                    .lifecycle
                    .scenes()
                    .iter()
                    .any(|s| s.by_id.contains_key(&id))
                {
                    return Err(LoadError::DuplicateEntity(id));
                }
                Some((i, id))
            }
        };
        if let Some(p) = parent {
            if !self.ecs().contains(p) {
                return Err(LoadError::UnknownAnchorParent(p));
            }
        }
        if !transform.is_finite() {
            return Err(LoadError::AnchorTransform);
        }
        let commit = self
            .edit_gameplay(|edit| {
                let pending = edit.spawn(name, transform, parent.map(Into::into))?;
                for component in components {
                    // The explicit transform argument owns the initial pose.
                    if !matches!(component, ComponentValue::Transform(_)) {
                        edit.set_component(pending, component)?;
                    }
                }
                for component in &registered {
                    edit.set_registered_authored(pending.into(), component, |_| None)?;
                }
                Ok(pending)
            })
            .map_err(|error| LoadError::LiveSpawnRejected(error.to_string()))?;
        let e = commit.resolve(commit.value).expect("committed live spawn");
        if let Some((i, id)) = scene_index {
            let s = &mut self.lifecycle.scenes_mut()[i];
            s.order.push((id, e));
            s.by_id.insert(id, e);
            s.by_entity.insert(e, id);
        }
        Ok((e, Vec::new()))
    }

    /// Every loaded scene, in load order.
    pub fn loaded_scenes(&self) -> &[LoadedScene] {
        self.lifecycle.scenes()
    }

    /// The loaded scene `entity` was spawned by, if any (an entity
    /// instantiated or spawned at runtime belongs to none).
    pub fn scene_of(&self, entity: Entity) -> Option<&LoadedScene> {
        self.lifecycle
            .scenes()
            .iter()
            .find(|s| s.id_of(entity).is_some())
    }

    /// Keeps the root `entity` and its subtree across loads (ADR-0037,
    /// Unity's `DontDestroyOnLoad`): a replace load leaves it, and so
    /// does unloading the scene it came from. Children made under it
    /// later go with it; given a parent, it stops being kept (it belongs
    /// to its new root). Marking it twice is fine. A live entity with a
    /// parent is [`LoadError::NotARoot`]; one not live,
    /// [`LoadError::UnknownSource`].
    pub fn dont_destroy_on_load(&mut self, entity: Entity) -> Result<(), LoadError> {
        if !self.ecs().contains(entity) {
            return Err(LoadError::UnknownSource(entity));
        }
        let marked = self
            .edit_gameplay(|edit| edit.dont_destroy_on_load(entity))
            .map_err(|e| LoadError::SceneRejected(e.to_string()))?
            .value;
        if marked {
            Ok(())
        } else {
            Err(LoadError::NotARoot(entity))
        }
    }

    /// Whether `entity` is kept across loads: its root is a persistent
    /// root still without a parent.
    pub fn is_persistent(&self, entity: Entity) -> bool {
        if self.lifecycle.persistent().is_empty() || !self.ecs().contains(entity) {
            return false;
        }
        let mut at = entity;
        while let Some(p) = scene::parent_of(self.ecs(), at) {
            at = p;
        }
        self.lifecycle.persistent().contains(&at)
    }

    /// Stops keeping `root` and destroys it with its subtree now, with
    /// the usual `Destroyed` events (ADR-0039: a second `load_project`
    /// replaces the persistent objects the first one made). Returns how
    /// many entities went; 0 when it is gone already.
    pub fn destroy_persistent(&mut self, root: Entity) -> usize {
        if !self.ecs().contains(root) {
            return 0;
        }
        let count = scene::subtree(self.ecs(), root).len();
        if self.edit_gameplay(|edit| edit.destroy(root)).is_err() {
            return 0;
        }
        self.lifecycle.persistent_mut().retain(|&e| e != root);
        count
    }

    /// The persistent roots alive and still roots, in marking order.
    pub fn persistent_roots(&self) -> Vec<Entity> {
        self.lifecycle
            .persistent()
            .iter()
            .copied()
            .filter(|&e| self.ecs().contains(e) && scene::parent_of(self.ecs(), e).is_none())
            .collect()
    }

    /// A replace load: the loaded scenes (last first) and then every
    /// remaining entity but the persistent roots' subtrees (ADR-0037).
    fn collect_scene_removals(&self, loaded: &LoadedScene, removed: &mut HashSet<Entity>) {
        for &(_, entity) in loaded.entities() {
            if self.ecs().contains(entity) && !self.is_persistent(entity) {
                removed.extend(scene::subtree(self.ecs(), entity));
            }
        }
    }

    /// Instantiates a cooked prefab now (M3-08b, blueprint item 12's
    /// `world.instantiate`): the prefab's **root** (ADR-0022) spawns under
    /// `parent` with `transform` as its Transform and `name` as its name
    /// (the root's own when empty), its descendants under it — exactly
    /// the shape a scene instance expands to. Components go through the
    /// shared journal as for a scene; no `SceneReady`. The root's handle
    /// and the prefab's entities are returned; `parent` must be live.
    pub fn instantiate(
        &mut self,
        prefab: &SceneDescription,
        name: &str,
        transform: Transform,
        parent: Option<Entity>,
    ) -> Result<(Entity, LoadedScene), LoadError> {
        self.instantiate_prepared(prefab, name, transform, parent, None)
    }

    /// Atomically publishes prefab entities and an already decoded resource plan.
    pub fn instantiate_prepared(
        &mut self,
        prefab: &SceneDescription,
        name: &str,
        transform: Transform,
        parent: Option<Entity>,
        resources: Option<Box<dyn crate::PreparedCommandResources>>,
    ) -> Result<(Entity, LoadedScene), LoadError> {
        if self.is_poisoned() {
            return Err(LoadError::Poisoned);
        }
        if !transform.is_finite() {
            return Err(LoadError::AnchorTransform);
        }
        if let Some(p) = parent {
            if p.world() != self.id() || !self.ecs().contains(p) {
                return Err(LoadError::UnknownAnchorParent(p));
            }
        }
        // The structure is checked before anything is spawned.
        spawn_order(prefab)?;
        let mut roots = prefab.entities.iter().filter(|d| d.parent.is_none());
        let (Some(root), None) = (roots.next(), roots.next()) else {
            return Err(LoadError::NotOneRoot(prefab.id));
        };
        let root = root.id;
        let placed = Placed {
            root,
            name,
            transform,
        };
        let loaded = self.load_under(prefab, parent, Some(placed), resources)?;
        Ok((loaded.by_id[&root], loaded))
    }

    /// The live subtree under `root` as a one-root description (ADR-0026
    /// §4, Unity's `Instantiate(gameObject)`): every entity's name, own
    /// active flag, children in order and built-in components, with
    /// synthetic ids — entity `i` of the returned list is
    /// `i + 1` as a big-endian UUID. None when `root` is not live.
    pub fn describe_subtree(&self, root: Entity) -> Option<(SceneDescription, Vec<Entity>)> {
        if root.world() != self.id() || !self.ecs().contains(root) {
            return None;
        }
        let ecs = self.ecs();
        // Breadth first; `(entity, parent's id)`.
        let mut entities = vec![(root, None)];
        let mut described: Vec<EntityDescription> = Vec::new();
        let mut i = 0;
        while i < entities.len() {
            let (e, parent) = entities[i];
            let id = synthetic_id(i + 1);
            let mut children = Vec::new();
            for c in scene::children_of(ecs, e) {
                if ecs.contains(c) {
                    entities.push((c, Some(id)));
                    children.push(synthetic_id(entities.len()));
                }
            }
            described.push(EntityDescription {
                registered: Vec::new(),
                id,
                name: ecs
                    .get::<Name>(e)
                    .map(|n| n.value.clone())
                    .unwrap_or_default(),
                active: scene::is_active_self(ecs, e),
                parent,
                children,
                components: ComponentValue::all_of(self, e),
            });
            i += 1;
        }
        // A reference into the copied tree points at the copy, one to an
        // object outside it at that object, as Unity's `Instantiate`
        // (ADR-0035).
        let inside: HashMap<Entity, Uuid> = entities
            .iter()
            .enumerate()
            .map(|(i, &(e, _))| (e, synthetic_id(i + 1)))
            .collect();
        for (d, (entity, _)) in described.iter_mut().zip(&entities) {
            d.registered = self
                .gameplay
                .registered
                .schemas()
                .filter_map(|schema| {
                    let values = self
                        .gameplay
                        .registered
                        .get(&schema.id)
                        .ok()?
                        .read(self.ecs(), *entity)?;
                    Some(crate::RegisteredDescription {
                        id: schema.id.clone(),
                        version: schema.version,
                        fields: schema
                            .fields
                            .iter()
                            .zip(values)
                            .map(|(field, value)| {
                                let value = match value {
                                    crate::ScalarValue::Object(target) => {
                                        crate::AuthoredRegisteredValue::Object(ObjectRef {
                                            id: target
                                                .and_then(|t| t.live())
                                                .and_then(|e| inside.get(&e).copied())
                                                .unwrap_or(Uuid::NIL),
                                            entity: target
                                                .and_then(|t| t.live())
                                                .filter(|e| self.ecs().contains(*e))
                                                .unwrap_or(Entity::NULL),
                                        })
                                    }
                                    other => crate::AuthoredRegisteredValue::Scalar(other),
                                };
                                (field.name.clone(), value)
                            })
                            .collect(),
                    })
                })
                .collect();
            for c in &mut d.components {
                c.for_each_object_ref(|_, _, r| {
                    r.id = inside.get(&r.entity).copied().unwrap_or(Uuid::NIL);
                });
            }
        }
        let description = SceneDescription {
            id: synthetic_id(0),
            name: String::new(),
            entities: described,
        };
        Some((description, entities.into_iter().map(|(e, _)| e).collect()))
    }

    /// A copy of the live subtree under `source` (see [`describe_subtree`](Self::describe_subtree)),
    /// its root named `name` (the source's when None) at `transform` under
    /// `parent`, as `instantiate` places a prefab. Uses the shared gameplay
    /// journal; invalid configuration rejects the entire clone. Earlier committed
    /// edits survive a clone failure; cloning never publishes unrelated work.
    pub fn clone_subtree(
        &mut self,
        source: Entity,
        name: Option<&str>,
        transform: Transform,
        parent: Option<Entity>,
    ) -> Result<Cloned, LoadError> {
        if self.is_poisoned() {
            return Err(LoadError::Poisoned);
        }
        if source.world() != self.id() || !self.ecs().contains(source) {
            return Err(LoadError::UnknownSource(source));
        }
        if !transform.is_finite() {
            return Err(LoadError::AnchorTransform);
        }
        if let Some(parent) = parent {
            if parent.world() != self.id() || !self.ecs().contains(parent) {
                return Err(LoadError::UnknownAnchorParent(parent));
            }
        }
        // Keep this legacy report's breadth-first pair order; allocation and
        // all cloning semantics belong to the shared gameplay journal.
        let mut sources = vec![source];
        let mut cursor = 0;
        while cursor < sources.len() {
            sources.extend(scene::children_of(self.ecs(), sources[cursor]));
            cursor += 1;
        }
        let commit = self
            .edit_gameplay(|edit| {
                edit.clone_tree(
                    source.into(),
                    parent.map(Into::into),
                    Some(transform),
                    name.map(str::to_owned),
                )
            })
            .map_err(|error| LoadError::CloneRejected(error.to_string()))?;
        let root = commit
            .resolve(commit.value.root)
            .expect("committed clone root");
        let pairs = sources
            .into_iter()
            .map(|source| {
                let pending = commit
                    .value
                    .handle_of(source.into())
                    .expect("cloned source");
                (
                    source,
                    commit.resolve(pending).expect("committed clone member"),
                )
            })
            .collect();
        Ok(Cloned {
            root,
            pairs,
            rejected_components: 0,
        })
    }

    /// Spawns `scene`'s entities, its roots under `root_parent`, then their
    /// components through the shared journal; propagates. `placed` gives
    /// an instantiated prefab's root its place and name.
    fn load_under(
        &mut self,
        scene: &SceneDescription,
        root_parent: Option<Entity>,
        placed: Option<Placed<'_>>,
        resources: Option<Box<dyn crate::PreparedCommandResources>>,
    ) -> Result<LoadedScene, LoadError> {
        let order = spawn_order(scene)?;
        if let Some(resources) = &resources {
            resources
                .preflight(self)
                .map_err(|e| LoadError::SceneRejected(e.to_string()))?;
        }
        let commit = self
            .edit_gameplay(|edit| {
                if let Some(resources) = &resources {
                    resources.stage(edit)?;
                }
                stage_description(edit, scene, &order, root_parent, placed.as_ref())
            })
            .map_err(|e| LoadError::SceneRejected(e.to_string()))?;
        self.publish_command_resources(resources);
        Ok(resolve_description(scene, &commit))
    }
}

/// `n` as a big-endian UUID: the ids of a described subtree.
fn synthetic_id(n: usize) -> Uuid {
    Uuid::from_bytes((n as u128).to_be_bytes())
}

/// Validates the structure and returns indexes in spawn order.
fn validate_registered_descriptions(
    values: &[crate::RegisteredDescription],
) -> Result<(), LoadError> {
    let mut seen = std::collections::HashSet::new();
    for value in values {
        if !seen.insert(&value.id) {
            return Err(LoadError::SceneRejected(format!(
                "registered component {} appears twice",
                value.id
            )));
        }
    }
    Ok(())
}

pub(crate) fn spawn_order(scene: &SceneDescription) -> Result<Vec<usize>, LoadError> {
    let mut index: HashMap<Uuid, usize> = HashMap::with_capacity(scene.entities.len());
    for (i, d) in scene.entities.iter().enumerate() {
        validate_registered_descriptions(&d.registered)?;
        if index.insert(d.id, i).is_some() {
            return Err(LoadError::DuplicateEntity(d.id));
        }
    }
    for d in &scene.entities {
        if let Some(p) = d.parent {
            let Some(&pi) = index.get(&p) else {
                return Err(LoadError::UnknownParent {
                    entity: d.id,
                    parent: p,
                });
            };
            let listed = scene.entities[pi]
                .children
                .iter()
                .filter(|c| **c == d.id)
                .count();
            if listed != 1 {
                return Err(LoadError::ChildParentMismatch {
                    entity: p,
                    child: d.id,
                });
            }
        }
        for c in &d.children {
            let Some(&ci) = index.get(c) else {
                return Err(LoadError::UnknownChild {
                    entity: d.id,
                    child: *c,
                });
            };
            if scene.entities[ci].parent != Some(d.id) {
                return Err(LoadError::ChildParentMismatch {
                    entity: d.id,
                    child: *c,
                });
            }
        }
        let mut seen = Vec::with_capacity(d.components.len());
        for c in &d.components {
            if seen.contains(&c.id()) {
                return Err(LoadError::DuplicateComponent {
                    entity: d.id,
                    component: c.id(),
                });
            }
            seen.push(c.id());
        }
    }
    // Roots in document order, then depth-first in listed child order.
    let mut order = Vec::with_capacity(scene.entities.len());
    let mut stack: Vec<usize> = scene
        .entities
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, d)| d.parent.is_none())
        .map(|(i, _)| i)
        .collect();
    while let Some(i) = stack.pop() {
        order.push(i);
        for c in scene.entities[i].children.iter().rev() {
            stack.push(index[c]);
        }
    }
    if order.len() != scene.entities.len() {
        let reached: std::collections::HashSet<usize> = order.iter().copied().collect();
        let lost = (0..scene.entities.len())
            .find(|i| !reached.contains(i))
            .unwrap();
        return Err(LoadError::Unreachable(scene.entities[lost].id));
    }
    Ok(order)
}

/// One construction path for scenes and the immediate prefab adapter. Missing
/// authored targets retain the loader's reported-null policy; live targets use
/// the same transaction validation as ordinary gameplay.
struct StagedDescription {
    entities: Vec<(Uuid, PendingEntity)>,
    missing_references: Vec<MissingReference>,
}

fn stage_description(
    edit: &mut GameplayEdit<'_>,
    scene: &SceneDescription,
    order: &[usize],
    root_parent: Option<Entity>,
    placed: Option<&Placed<'_>>,
) -> EditResult<StagedDescription> {
    let mut by_id = HashMap::new();
    let mut entities = Vec::new();
    let mut missing_references = Vec::new();
    for &i in order {
        let d = &scene.entities[i];
        let here = placed.filter(|p| p.root == d.id);
        let pose = here.map(|p| p.transform).unwrap_or_else(|| {
            d.components
                .iter()
                .find_map(|c| {
                    if let ComponentValue::Transform(t) = c {
                        Some(*t)
                    } else {
                        None
                    }
                })
                .unwrap_or(Transform::IDENTITY)
        });
        let parent = d
            .parent
            .map(|id| crate::GameplayEntity::Pending(by_id[&id]))
            .or(root_parent.map(Into::into));
        let name = here
            .map(|p| p.name)
            .filter(|n| !n.is_empty())
            .unwrap_or(&d.name);
        let entity = edit.spawn(name, pose, parent)?;
        edit.set_active(entity, d.active)?;
        by_id.insert(d.id, entity);
        entities.push((d.id, entity));
    }
    for &i in order {
        let d = &scene.entities[i];
        for component in &d.components {
            if matches!(component, ComponentValue::Transform(_)) {
                continue;
            }
            let mut component = component.clone();
            component.for_each_object_ref(|component, field, r| {
                if !r.id.is_nil() && !by_id.contains_key(&r.id) {
                    if r.entity.is_null() {
                        missing_references.push(MissingReference {
                            entity: d.id,
                            component,
                            field,
                            target: r.id,
                        });
                        *r = ObjectRef::NONE;
                    } else {
                        r.id = Uuid::NIL;
                    }
                }
            });
            let value = GameplayComponentValue::from(component).map_object_refs(|r| match r {
                GameplayObjectRef::Stored(r) if !r.id.is_nil() => {
                    GameplayObjectRef::Pending(by_id[&r.id])
                }
                other => other,
            });
            edit.set_component(by_id[&d.id], value)?;
        }
        for component in &d.registered {
            edit.set_registered_authored(by_id[&d.id].into(), component, |id| {
                by_id.get(&id).copied().map(Into::into)
            })?;
        }
    }
    Ok(StagedDescription {
        entities,
        missing_references,
    })
}

fn resolve_description(
    scene: &SceneDescription,
    commit: &crate::EditCommit<StagedDescription>,
) -> LoadedScene {
    let order: Vec<_> = commit
        .value
        .entities
        .iter()
        .map(|&(id, pending)| (id, commit.resolve(pending).expect("committed scene entity")))
        .collect();
    LoadedScene {
        id: scene.id,
        name: scene.name.clone(),
        by_id: order.iter().copied().collect(),
        by_entity: order.iter().map(|&(id, entity)| (entity, id)).collect(),
        order,
        rejected_components: 0,
        missing_references: commit.value.missing_references.clone(),
    }
}
