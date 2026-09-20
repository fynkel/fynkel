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

use fynkel_contracts::{ComponentId, Entity, Uuid};
use fynkel_scene::{self as scene, Transform};

use crate::command::Command;
use crate::components::{
    ComponentValue, Emitter, Follow, LifetimeDespawn, LookAtCamera, Oscillate, Rotator,
    ThirdPersonCamera,
};
use crate::event::EventPayload;
use crate::world::{Name, World};

#[derive(Clone, Debug, PartialEq)]
pub struct EntityDescription {
    pub id: Uuid,
    pub name: String,
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
        }
    }
}

impl std::error::Error for LoadError {}

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

impl World {
    /// Instantiates `scene` now, at the current tick boundary. Emits
    /// `SceneReady`. Structural errors load nothing; component values that
    /// fail the command path are counted in `rejected_components` and
    /// reported in [`World::diagnostics`] (command index `u32::MAX`).
    pub fn load_scene(&mut self, scene: &SceneDescription) -> Result<LoadedScene, LoadError> {
        let tick = self.tick();
        let loaded = self.load_under(scene, None, tick, true)?;
        self.push_event(EventPayload::SceneReady);
        Ok(loaded)
    }

    /// Instantiates a cooked prefab now (M3-08b, blueprint item 12's
    /// `world.instantiate`): an **anchor** entity (`name`, `transform`,
    /// under `parent`) whose children are the prefab's roots, exactly the
    /// shape a scene instance expands to. Components go through the
    /// command path as for a scene; no `SceneReady`. The anchor's handle
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
        let anchor = scene::spawn(self.ecs_mut(), transform, parent)
            .map_err(|_| LoadError::Spawn(prefab.id))?;
        if !name.is_empty() {
            let _ = self.ecs_mut().insert(anchor, Name::new(name.to_string()));
        }
        let loaded = self.load_under(prefab, Some(anchor), tick, propagate)?;
        Ok((anchor, loaded))
    }

    /// Spawns `scene`'s entities, its roots under `root_parent`, then their
    /// components through the command path; propagates.
    fn load_under(
        &mut self,
        scene: &SceneDescription,
        root_parent: Option<Entity>,
        tick: u64,
        propagate: bool,
    ) -> Result<LoadedScene, LoadError> {
        if self.is_poisoned() {
            return Err(LoadError::Poisoned);
        }
        let order = spawn_order(scene)?;
        let mut loaded = LoadedScene {
            id: scene.id,
            name: scene.name.clone(),
            ..LoadedScene::default()
        };
        for &i in &order {
            let d = &scene.entities[i];
            let parent = d.parent.map(|p| loaded.by_id[&p]).or(root_parent);
            let transform = d
                .components
                .iter()
                .find_map(|c| match c {
                    ComponentValue::Transform(t) => Some(*t),
                    _ => None,
                })
                .unwrap_or(Transform::IDENTITY);
            let e = scene::spawn(self.ecs_mut(), transform, parent)
                .map_err(|_| LoadError::Spawn(d.id))?;
            if !d.name.is_empty() {
                let _ = self.ecs_mut().insert(e, Name::new(d.name.clone()));
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
                if !self.apply_loaded(e, c.clone(), tick) {
                    loaded.rejected_components += 1;
                }
            }
        }
        if propagate {
            scene::propagate(self.ecs_mut());
        }
        Ok(loaded)
    }

    fn apply_loaded(&mut self, entity: Entity, value: ComponentValue, tick: u64) -> bool {
        self.apply_simulation(u32::MAX, Command::SetComponent { entity, value }, tick)
    }
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
        out
    }
}
