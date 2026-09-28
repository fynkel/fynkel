//! Scene → runtime instantiation (blueprint §19.2c item 5).
//!
//! A [`SceneDescription`] is the typed, already-validated content of a
//! `.scene.json` (built by `fynkel_document::runtime`); this module knows
//! nothing about JSON. [`World::load_scene`] spawns the hierarchy in
//! document order (roots in order, children in their listed order) and
//! applies every other component through `Command::SetComponent`, so a
//! value that the bridge would reject is rejected here too and shows up in
//! `World::diagnostics()`. The returned [`LoadedScene`] maps authoring ids to
//! runtime handles both ways.

use std::collections::HashMap;

use fynkel_contracts::{ComponentId, Entity, ObjectRef, Uuid};
use fynkel_scene::{self as scene, Transform};

use crate::command::Command;
use crate::components::{
    Animator, ComponentValue, Emitter, Follow, LifetimeDespawn, LookAtCamera, Oscillate, Rotator,
    Skin, ThirdPersonCamera, TriggerZone,
};
use crate::event::EventPayload;
use crate::world::{Diagnostic, Name, World};

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
}

impl core::fmt::Display for LoadError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
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
    /// Component values the command path rejected.
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
    /// Component values rejected by the command path (see the world's
    /// diagnostics); the entities themselves exist.
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
    /// Structural errors load nothing (and unload nothing); component
    /// values that fail the command path are counted in
    /// `rejected_components` and reported in [`World::diagnostics`]
    /// (command index `u32::MAX`).
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
        if self.is_poisoned() {
            return Err(LoadError::Poisoned);
        }
        // The structure is checked before anything is taken down.
        spawn_order(scene)?;
        match mode {
            LoadMode::Replace => {
                self.unload_all();
            }
            LoadMode::Additive => {
                if self.scenes.iter().any(|s| s.id == scene.id) {
                    return Err(LoadError::AlreadyLoaded(scene.id));
                }
            }
        }
        let tick = self.tick();
        let loaded = self.load_under(scene, None, None, tick, true)?;
        self.scenes.push(loaded.clone());
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
        let Some(i) = self.scenes.iter().position(|s| s.id == id) else {
            return Err(LoadError::NotLoaded(id));
        };
        let scene = self.scenes.remove(i);
        Ok(self.take_down(scene))
    }

    /// Spawns one entity now, at the current tick, the way a scene load
    /// does — the editor's live Play mode (ADR-0021 P7) creating an object
    /// in a running, possibly paused, game: `name`, a local `transform`,
    /// last among `parent`'s children, then `components` through the
    /// simulation lane's checks. No system runs and no event is emitted
    /// (a load emits none per entity either). With `authoring` =
    /// `(scene, id)` the entity joins the loaded scene `scene` under the
    /// authoring id `id`, as if that scene had spawned it. Returns the
    /// entity and its rejected components (also in
    /// [`World::diagnostics`]); a component that fails leaves the entity.
    pub fn spawn_now(
        &mut self,
        name: &str,
        transform: Transform,
        parent: Option<Entity>,
        components: Vec<ComponentValue>,
        authoring: Option<(Uuid, Uuid)>,
    ) -> Result<(Entity, Vec<Diagnostic>), LoadError> {
        if self.is_poisoned() {
            return Err(LoadError::Poisoned);
        }
        let scene_index = match authoring {
            None => None,
            Some((scene, id)) => {
                let i = self
                    .scenes
                    .iter()
                    .position(|s| s.id == scene)
                    .ok_or(LoadError::NotLoaded(scene))?;
                if self.scenes.iter().any(|s| s.by_id.contains_key(&id)) {
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
        let e = scene::spawn(self.ecs_mut(), transform, parent)
            .map_err(|_| LoadError::Spawn(authoring.map(|a| a.1).unwrap_or(Uuid::NIL)))?;
        if !name.is_empty() {
            let _ = self.ecs_mut().insert(e, Name::new(name.to_owned()));
        }
        if let Some((i, id)) = scene_index {
            let s = &mut self.scenes[i];
            s.order.push((id, e));
            s.by_id.insert(id, e);
            s.by_entity.insert(e, id);
        }
        let first = self.diagnostics().len();
        let tick = self.tick();
        for c in components {
            if matches!(c, ComponentValue::Transform(_)) {
                continue;
            }
            self.apply_loaded(e, c, tick);
        }
        if parent.is_some() {
            self.mark_active_dirty();
            self.refresh_active();
        }
        scene::propagate(self.ecs_mut());
        Ok((e, self.diagnostics()[first..].to_vec()))
    }

    /// Every loaded scene, in load order.
    pub fn loaded_scenes(&self) -> &[LoadedScene] {
        &self.scenes
    }

    /// The loaded scene `entity` was spawned by, if any (an entity
    /// instantiated or spawned at runtime belongs to none).
    pub fn scene_of(&self, entity: Entity) -> Option<&LoadedScene> {
        self.scenes.iter().find(|s| s.id_of(entity).is_some())
    }

    /// A replace load: the loaded scenes (last first) and then every
    /// remaining entity, so nothing of the previous state survives.
    fn unload_all(&mut self) {
        while let Some(scene) = self.scenes.pop() {
            self.take_down(scene);
        }
        let tick = self.tick();
        let roots: Vec<Entity> = self
            .ecs()
            .entities()
            .filter(|&e| scene::parent_of(self.ecs(), e).is_none())
            .collect();
        for root in roots {
            if self.ecs().contains(root) {
                let _ = self.destroy_subtree(root, tick);
            }
        }
    }

    fn take_down(&mut self, scene: LoadedScene) -> UnloadedScene {
        let tick = self.tick();
        let mut entities = 0u32;
        // Parents before children in spawn order: a parent's subtree takes
        // its children, which then read as gone.
        for &(_, e) in scene.entities() {
            if self.ecs().contains(e) {
                if let Ok(n) = self.destroy_subtree(e, tick) {
                    entities += n as u32;
                }
            }
        }
        self.push_event(EventPayload::SceneUnloaded {
            scene: scene.id,
            entities,
        });
        UnloadedScene {
            id: scene.id,
            name: scene.name,
            entities,
        }
    }

    /// Instantiates a cooked prefab now (M3-08b, blueprint item 12's
    /// `world.instantiate`): the prefab's **root** (ADR-0022) spawns under
    /// `parent` with `transform` as its Transform and `name` as its name
    /// (the root's own when empty), its descendants under it — exactly
    /// the shape a scene instance expands to. Components go through the
    /// command path as for a scene; no `SceneReady`. The root's handle
    /// and the prefab's entities are returned; `parent` must be live.
    pub fn instantiate(
        &mut self,
        prefab: &SceneDescription,
        name: &str,
        transform: Transform,
        parent: Option<Entity>,
    ) -> Result<(Entity, LoadedScene), LoadError> {
        let tick = self.tick();
        self.instantiate_inner(prefab, name, transform, parent, tick, true)
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
        for d in &mut described {
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
    /// `parent`, as `instantiate` places a prefab.
    pub fn clone_subtree(
        &mut self,
        source: Entity,
        name: Option<&str>,
        transform: Transform,
        parent: Option<Entity>,
    ) -> Result<Cloned, LoadError> {
        let Some((description, sources)) = self.describe_subtree(source) else {
            return Err(LoadError::UnknownSource(source));
        };
        let name = name.unwrap_or(&description.entities[0].name).to_owned();
        let (root, loaded) = self.instantiate(&description, &name, transform, parent)?;
        let pairs = sources
            .iter()
            .enumerate()
            .map(|(i, &s)| (s, loaded.by_id[&synthetic_id(i + 1)]))
            .collect();
        Ok(Cloned {
            root,
            pairs,
            rejected_components: loaded.rejected_components,
        })
    }

    /// `instantiate` with the tick its component writes belong to and
    /// whether to propagate afterwards (an `Emitter` inside `step` leaves
    /// that to the tick's own propagate).
    pub(crate) fn instantiate_inner(
        &mut self,
        prefab: &SceneDescription,
        name: &str,
        transform: Transform,
        parent: Option<Entity>,
        tick: u64,
        propagate: bool,
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
        let loaded = self.load_under(prefab, parent, Some(placed), tick, propagate)?;
        Ok((loaded.by_id[&root], loaded))
    }

    /// Spawns `scene`'s entities, its roots under `root_parent`, then their
    /// components through the command path; propagates. `placed` gives
    /// an instantiated prefab's root its place and name.
    fn load_under(
        &mut self,
        scene: &SceneDescription,
        root_parent: Option<Entity>,
        placed: Option<Placed<'_>>,
        tick: u64,
        propagate: bool,
    ) -> Result<LoadedScene, LoadError> {
        if self.is_poisoned() {
            return Err(LoadError::Poisoned);
        }
        let order = spawn_order(scene)?;
        let markers_current = self.active_markers_current();
        let mut loaded = LoadedScene {
            id: scene.id,
            name: scene.name.clone(),
            ..LoadedScene::default()
        };
        for &i in &order {
            let d = &scene.entities[i];
            let parent = d.parent.map(|p| loaded.by_id[&p]).or(root_parent);
            let here = placed.as_ref().filter(|p| p.root == d.id);
            let transform = match here {
                Some(p) => p.transform,
                None => d
                    .components
                    .iter()
                    .find_map(|c| match c {
                        ComponentValue::Transform(t) => Some(*t),
                        _ => None,
                    })
                    .unwrap_or(Transform::IDENTITY),
            };
            let e = scene::spawn(self.ecs_mut(), transform, parent)
                .map_err(|_| LoadError::Spawn(d.id))?;
            let name = here
                .map(|p| p.name)
                .filter(|n| !n.is_empty())
                .unwrap_or(&d.name);
            if !name.is_empty() {
                let _ = self.ecs_mut().insert(e, Name::new(name.to_string()));
            }
            if !d.active {
                let _ = scene::set_active(self.ecs_mut(), e, false);
                self.mark_active_dirty();
            }
            loaded.order.push((d.id, e));
            loaded.by_id.insert(d.id, e);
            loaded.by_entity.insert(e, d.id);
        }
        // Components after the whole hierarchy exists, through the command
        // path (validation and diagnostics identical to the bridge).
        for &i in &order {
            let d = &scene.entities[i];
            let e = loaded.by_id[&d.id];
            for c in &d.components {
                if matches!(c, ComponentValue::Transform(_)) {
                    // The spawn transform; a non-finite one is rejected by
                    // `spawn` itself, so re-applying would be redundant.
                    continue;
                }
                let mut c = c.clone();
                // Object references (ADR-0035) resolve once, here, against
                // this load: a prefab's reach each instance's own copies.
                c.for_each_object_ref(|component, field, r| {
                    if r.id.is_nil() {
                        return;
                    }
                    match loaded.by_id.get(&r.id) {
                        Some(&target) => r.entity = target,
                        None if r.entity.is_null() => {
                            loaded.missing_references.push(MissingReference {
                                entity: d.id,
                                component,
                                field,
                                target: r.id,
                            });
                            *r = ObjectRef::NONE;
                        }
                        // A copy's reference to an object outside the
                        // copied tree keeps pointing at it.
                        None => r.id = Uuid::NIL,
                    }
                });
                if !self.apply_loaded(e, c, tick) {
                    loaded.rejected_components += 1;
                }
            }
        }
        // The new subtrees alone (foundation 3.1): their active markers
        // from their parents' when every other marker was current, their
        // world matrices from their parents' unless an ancestor is itself
        // waiting for a propagate.
        let roots: Vec<Entity> = scene
            .entities
            .iter()
            .filter(|d| d.parent.is_none())
            .map(|d| loaded.by_id[&d.id])
            .collect();
        if markers_current {
            self.refresh_active_subtrees(&roots);
        } else {
            self.mark_active_dirty();
            self.refresh_active();
        }
        if propagate {
            for &root in &roots {
                if scene::propagate_subtree(self.ecs_mut(), root).is_none() {
                    scene::propagate(self.ecs_mut());
                    break;
                }
            }
        }
        Ok(loaded)
    }

    fn apply_loaded(&mut self, entity: Entity, value: ComponentValue, tick: u64) -> bool {
        self.apply_simulation(u32::MAX, Command::SetComponent { entity, value }, tick)
    }
}

/// `n` as a big-endian UUID: the ids of a described subtree.
fn synthetic_id(n: usize) -> Uuid {
    Uuid::from_bytes((n as u128).to_be_bytes())
}

/// Validates the structure and returns indexes in spawn order.
fn spawn_order(scene: &SceneDescription) -> Result<Vec<usize>, LoadError> {
    let mut index: HashMap<Uuid, usize> = HashMap::with_capacity(scene.entities.len());
    for (i, d) in scene.entities.iter().enumerate() {
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

impl ComponentValue {
    /// Every built-in component on `entity`, in registry order.
    pub fn all_of(world: &World, entity: Entity) -> Vec<ComponentValue> {
        use crate::components::{
            Camera, DirectionalLight, Environment, MeshRenderer, PointLight, SpotLight,
        };
        let ecs = world.ecs();
        let mut out = Vec::new();
        if let Ok(t) = ecs.copy::<Transform>(entity) {
            out.push(ComponentValue::Transform(t));
        }
        if let Ok(v) = ecs.copy::<MeshRenderer>(entity) {
            out.push(ComponentValue::MeshRenderer(v));
        }
        if let Ok(v) = ecs.copy::<Camera>(entity) {
            out.push(ComponentValue::Camera(v));
        }
        if let Ok(v) = ecs.copy::<DirectionalLight>(entity) {
            out.push(ComponentValue::DirectionalLight(v));
        }
        if let Ok(v) = ecs.copy::<PointLight>(entity) {
            out.push(ComponentValue::PointLight(v));
        }
        if let Ok(v) = ecs.copy::<SpotLight>(entity) {
            out.push(ComponentValue::SpotLight(v));
        }
        if let Ok(v) = ecs.get::<Environment>(entity) {
            out.push(ComponentValue::Environment(v.clone()));
        }
        if let Ok(v) = ecs.copy::<fynkel_physics::RigidBody>(entity) {
            out.push(ComponentValue::RigidBody(v));
        }
        if let Ok(v) = ecs.get::<fynkel_physics::Collider>(entity) {
            out.push(ComponentValue::Collider(v.clone()));
        }
        if let Ok(v) = ecs.get::<fynkel_physics::CharacterController>(entity) {
            out.push(ComponentValue::CharacterController(v.clone()));
        }
        if let Ok(v) = ecs.get::<ThirdPersonCamera>(entity) {
            out.push(ComponentValue::ThirdPersonCamera(v.clone()));
        }
        if let Ok(v) = ecs.copy::<Rotator>(entity) {
            out.push(ComponentValue::Rotator(v));
        }
        if let Ok(v) = ecs.copy::<Oscillate>(entity) {
            out.push(ComponentValue::Oscillate(v));
        }
        if let Ok(v) = ecs.get::<Follow>(entity) {
            out.push(ComponentValue::Follow(v.clone()));
        }
        if let Ok(v) = ecs.copy::<LookAtCamera>(entity) {
            out.push(ComponentValue::LookAtCamera(v));
        }
        if let Ok(v) = ecs.copy::<LifetimeDespawn>(entity) {
            out.push(ComponentValue::LifetimeDespawn(v));
        }
        if let Ok(v) = ecs.get::<Emitter>(entity) {
            out.push(ComponentValue::Emitter(v.clone()));
        }
        if let Ok(v) = ecs.get::<TriggerZone>(entity) {
            out.push(ComponentValue::TriggerZone(v.clone()));
        }
        if let Ok(v) = ecs.get::<Animator>(entity) {
            out.push(ComponentValue::Animator(v.clone()));
        }
        if let Ok(v) = ecs.get::<Skin>(entity) {
            out.push(ComponentValue::Skin(v.clone()));
        }
        if let Ok(v) = ecs.get::<fynkel_audio::AudioSource>(entity) {
            out.push(ComponentValue::AudioSource(v.clone()));
        }
        if let Ok(v) = ecs.copy::<fynkel_audio::AudioListener>(entity) {
            out.push(ComponentValue::AudioListener(v));
        }
        if let Ok(v) = ecs.get::<crate::components::Volume>(entity) {
            out.push(ComponentValue::Volume(v.clone()));
        }
        if let Ok(v) = ecs.copy::<fynkel_scene::Layer>(entity) {
            out.push(ComponentValue::Layer(v));
        }
        if let Ok(v) = ecs.copy::<fynkel_scene::Tags>(entity) {
            out.push(ComponentValue::Tags(v));
        }
        out
    }
}
