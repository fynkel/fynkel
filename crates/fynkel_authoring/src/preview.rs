//! The edit-preview world (ADR-0016 resolutions; blueprint §19.2e item 6):
//! what the Scene view draws. A `fynkel_core::World` in
//! `WorldConfig::edit_mode` holds the *live* document — dirty edits
//! included, prefab instances expanded from the open editors (or disk),
//! imported hierarchies from the importer — and never runs a system: the
//! picture is the document, not a game.
//!
//! The crate does not own the world (`fynkel_native` does, so the same
//! `FynkelWorld` a viewport renders is the one the editor edits). A
//! [`Preview`] maps authoring ids to entities and works on a `&mut World`
//! handed in:
//!
//! - [`Preview::load`] — a fresh `World::load_scene` of the described
//!   document (skins registered, the closure retained).
//! - [`Preview::sync`] — the **incremental sync rule**: the world is
//!   diffed against the description the document has *now* and only the
//!   differences become commands (spawns, destroys, reparents, renames,
//!   component sets and removes, sibling order), applied through the
//!   world's own command path over as many steps as the hierarchy needs.
//!   Undo, redo, prefab overrides, duplicates and edits to a prefab open in
//!   another tab all go through the same diff; nothing has a special case.
//!   When a round makes no progress (the world rejected a command) the
//!   preview falls back to a **reload** and says so.
//! - [`Preview::dump`] — the state keyed by authoring id, the shape of
//!   `fynkel state dump --json`, so a test can prove that an incrementally
//!   synced world equals a fresh load after every transaction kind.
//! - picking ([`Preview::pick`], [`Preview::pick_rect`]), the debug-line
//!   overlays ([`Preview::outline_lines`], [`Preview::icon_lines`]), the
//!   handles the renderer's selection outline names ([`Preview::entities`])
//!   and
//!   the gizmo's transform preview ([`Preview::set_transforms`]), the
//!   Inspector's live component, material and volume profile edits
//!   ([`Preview::set_components`], [`Preview::set_material`],
//!   [`Preview::set_volume`]).

use std::collections::{HashMap, HashSet};

use fynkel_assets::{Assets, MeshPayload, Residency};
use fynkel_contracts::{AssetRef, Code, Diagnostic, Entity};
use fynkel_core::components::{Camera, ComponentValue, MeshRenderer, Projection};
use fynkel_core::{
    projection_matrix, view_matrix, Command, EntityDescription, EventPayload, Lane, Name,
    SceneDescription, World,
};
use fynkel_document::generated::encode_component;
use fynkel_document::DocumentCodec;
use fynkel_physics::{Collider, ColliderShape};
use fynkel_scene::{self as scene, Transform};
use glam::{Mat4, Vec2, Vec3, Vec4};
use serde_json::{json, Value};
use uuid::Uuid;

/// Radius of the pick sphere and the icon cross of an entity that draws
/// nothing (an empty, a light, a camera), metres.
pub const ICON_RADIUS: f32 = 0.25;
/// How far past its near plane a camera's frustum outline reaches (its
/// `far` is 1 km by default — unreadable as an icon).
pub const CAMERA_OUTLINE_DEPTH: f32 = 4.0;
/// The alpha of the frustum of a camera that is not selected.
pub const UNSELECTED_CAMERA_ALPHA: f32 = 0.35;
/// Segments of every outline circle.
const CIRCLE_SEGMENTS: usize = 64;
/// Sync rounds before the preview gives up on the incremental path. Each
/// round spawns one more level of new hierarchy; no document is this deep.
const MAX_SYNC_ROUNDS: u32 = 64;

fn to_uuid(u: fynkel_contracts::Uuid) -> Uuid {
    Uuid::from_bytes(*u.as_bytes())
}

fn from_uuid(u: Uuid) -> fynkel_contracts::Uuid {
    fynkel_contracts::Uuid::from_bytes(*u.as_bytes())
}

/// One coloured world-space segment for the renderer's debug lines.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub a: Vec3,
    pub b: Vec3,
    pub color: Vec4,
}

impl Line {
    pub fn json(&self) -> Value {
        json!([
            self.a.x,
            self.a.y,
            self.a.z,
            self.b.x,
            self.b.y,
            self.b.z,
            self.color.x,
            self.color.y,
            self.color.z,
            self.color.w
        ])
    }
}

/// The editor camera as the Scene view looks through it: its world matrix,
/// its `Camera` (perspective or orthographic) and the viewport's aspect.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub world: Mat4,
    pub camera: Camera,
    pub aspect: f32,
}

impl View {
    /// Projection × view — the renderer's matrices (clip depth 0 = near,
    /// 1 = far).
    pub fn view_projection(&self) -> Mat4 {
        projection_matrix(&self.camera, self.aspect) * view_matrix(&self.world)
    }

    /// The ray through the pixel at `ndc` (−1..1 both axes, +Y up).
    pub fn ray(&self, ndc: Vec2) -> Ray {
        let inv = self.view_projection().inverse();
        let near = inv.project_point3(Vec3::new(ndc.x, ndc.y, 0.0));
        let far = inv.project_point3(Vec3::new(ndc.x, ndc.y, 1.0));
        Ray {
            origin: near,
            dir: (far - near).normalize_or(Vec3::NEG_Z),
        }
    }

    /// Where `point` lands in NDC, `None` when it is at or behind the
    /// camera plane.
    pub fn project(&self, point: Vec3) -> Option<Vec2> {
        let h = self.view_projection() * point.extend(1.0);
        (h.w > 1e-6).then(|| Vec2::new(h.x / h.w, h.y / h.w))
    }
}

/// A picking ray in world space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: Vec3,
    pub dir: Vec3,
}

impl Ray {
    /// Distance along the ray to a sphere, if hit (the near intersection;
    /// 0 when the origin is inside).
    pub fn hit_sphere(&self, center: Vec3, radius: f32) -> Option<f32> {
        let oc = self.origin - center;
        let b = oc.dot(self.dir);
        let c = oc.length_squared() - radius * radius;
        if c <= 0.0 {
            return Some(0.0);
        }
        let disc = b * b - c;
        if disc < 0.0 {
            return None;
        }
        let t = -b - disc.sqrt();
        (t >= 0.0).then_some(t)
    }

    /// Möller–Trumbore, both faces; a ray exactly along a shared edge
    /// (a click on a box's diagonal) still hits, hence the tolerance.
    fn hit_triangle(&self, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
        const EPS: f32 = 1e-5;
        let e1 = b - a;
        let e2 = c - a;
        let p = self.dir.cross(e2);
        let det = e1.dot(p);
        if det.abs() < 1e-9 {
            return None;
        }
        let inv = 1.0 / det;
        let s = self.origin - a;
        let u = s.dot(p) * inv;
        if !(-EPS..=1.0 + EPS).contains(&u) {
            return None;
        }
        let q = s.cross(e1);
        let v = self.dir.dot(q) * inv;
        if v < -EPS || u + v > 1.0 + EPS {
            return None;
        }
        let t = e2.dot(q) * inv;
        (t >= 0.0).then_some(t)
    }

    fn transformed(&self, m: &Mat4) -> Ray {
        Ray {
            origin: m.transform_point3(self.origin),
            dir: m.transform_vector3(self.dir),
        }
    }
}

/// One picked entity, nearest first in [`Preview::pick`]'s result.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    /// The id the editor knows: a prefab instance's anchor for anything
    /// inside it.
    pub id: Uuid,
    /// The object the ray met — `id` itself, or the child of the instance
    /// (a material dropped there goes onto it, as in Unity).
    pub object: Uuid,
    pub entity: Entity,
    /// Along the ray, metres.
    pub distance: f32,
    pub point: Vec3,
    /// A mesh triangle was hit (`false`: the icon sphere of an entity
    /// without a mesh).
    pub mesh: bool,
}

impl Hit {
    pub fn json(&self) -> Value {
        json!({
            "id": self.id.to_string(),
            "object": self.object.to_string(),
            "distance": self.distance,
            "point": [self.point.x, self.point.y, self.point.z],
            "mesh": self.mesh,
        })
    }
}

/// What one [`Preview::sync`] did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SyncReport {
    pub spawned: usize,
    pub destroyed: usize,
    /// Renames, reparents, transform and component sets and removes.
    pub updated: usize,
    pub reordered: usize,
    /// World steps taken (0 when nothing differed).
    pub steps: u32,
    /// The incremental path gave up and the scene was loaded fresh.
    pub reloaded: bool,
    pub diagnostics: Vec<Diagnostic>,
}

impl SyncReport {
    pub fn json(&self) -> Value {
        json!({
            "spawned": self.spawned,
            "destroyed": self.destroyed,
            "updated": self.updated,
            "reordered": self.reordered,
            "steps": self.steps,
            "reloaded": self.reloaded,
            "diagnostics": fynkel_document::diagnostics_json(&self.diagnostics)["diagnostics"],
        })
    }
}

/// The editor's view of one document inside a world (see the module docs).
pub struct Preview {
    /// The document's id.
    pub document: Uuid,
    ids: HashMap<Uuid, Entity>,
    by_entity: HashMap<Entity, Uuid>,
    /// Document order of the last description synced (the dump's order).
    order: Vec<Uuid>,
    /// The entities of the last description (a transform preview
    /// restores from here).
    described: HashMap<Uuid, EntityDescription>,
    /// The closure of the last description's asset references, kept
    /// resident while the preview lives.
    residency: Option<Residency>,
    /// Expanded entity → the document entity that anchors its prefab
    /// instance (outermost); see [`Preview::set_anchors`].
    anchors: HashMap<Uuid, Uuid>,
    /// Loads (the first one included) and incremental syncs so far.
    pub loads: u32,
    pub syncs: u32,
    /// The last load's or sync's problems (the world's own diagnostics of
    /// rejected components come along).
    pub diagnostics: Vec<Diagnostic>,
}

impl Preview {
    /// Loads `described` fresh into `world` (a replace load: whatever the
    /// world held goes). The document's structure was validated already;
    /// a load the world still refuses (a parent cycle the document's
    /// baseline tolerates) is `FYN-R0008` and leaves an empty preview.
    pub fn load(
        document: Uuid,
        world: &mut World,
        assets: Option<&Assets>,
        described: &SceneDescription,
    ) -> Preview {
        let mut p = Preview {
            document,
            ids: HashMap::new(),
            by_entity: HashMap::new(),
            order: Vec::new(),
            described: HashMap::new(),
            residency: None,
            anchors: HashMap::new(),
            loads: 0,
            syncs: 0,
            diagnostics: Vec::new(),
        };
        p.reload(world, assets, described);
        p
    }

    /// The fallback and the user's "Reload": the world emptied and
    /// `described` loaded fresh.
    pub fn reload(
        &mut self,
        world: &mut World,
        assets: Option<&Assets>,
        described: &SceneDescription,
    ) {
        self.loads += 1;
        self.diagnostics.clear();
        self.ids.clear();
        self.by_entity.clear();
        self.order.clear();
        self.described.clear();
        self.residency = None;
        let before = world.diagnostics().len();
        match world.load_scene(described) {
            Ok(loaded) => {
                for (id, e) in loaded.entities() {
                    let id = to_uuid(*id);
                    self.ids.insert(id, *e);
                    self.by_entity.insert(*e, id);
                }
            }
            Err(e) => {
                self.diagnostics.push(
                    Diagnostic::error(
                        Code::RUNTIME_SCENE_INVALID,
                        format!("the scene cannot be loaded for preview: {e}"),
                    )
                    .with_id(described.id.to_string())
                    .with_suggestion(
                        "fix the document's hierarchy errors; the Scene view stays empty",
                    ),
                );
            }
        }
        self.diagnostics
            .extend(world.diagnostics()[before..].iter().map(world_diagnostic));
        world.drain_events();
        self.remember(described);
        self.register_assets(world, assets, described);
        // One step so the transforms are propagated and the snapshot and
        // the first extract show the document (no system runs).
        world.step();
        world.drain_events();
    }

    fn remember(&mut self, described: &SceneDescription) {
        self.order = described.entities.iter().map(|e| to_uuid(e.id)).collect();
        self.described = described
            .entities
            .iter()
            .map(|e| (to_uuid(e.id), e.clone()))
            .collect();
    }

    /// Skins the description's `Skin` components name (a skinned mesh
    /// draws posed from its joints' document transforms) and the closure
    /// of every asset referenced, retained.
    fn register_assets(
        &mut self,
        world: &mut World,
        assets: Option<&Assets>,
        described: &SceneDescription,
    ) {
        let Some(assets) = assets else {
            return;
        };
        let mut closure: Vec<fynkel_contracts::Uuid> = Vec::new();
        for e in &described.entities {
            for c in &e.components {
                if let ComponentValue::Skin(k) = c {
                    if let Err(d) = assets.register_skin(&k.skin, world) {
                        self.diagnostics.push(d);
                    }
                }
                for r in asset_refs_of(c) {
                    if r.asset.is_nil() {
                        continue;
                    }
                    for id in assets.closure(r.asset) {
                        if !closure.contains(&id) {
                            closure.push(id);
                        }
                    }
                }
            }
        }
        self.residency = Some(assets.retain(&closure));
    }

    // ------------------------------------------------------------ mapping

    pub fn entity(&self, id: Uuid) -> Option<Entity> {
        self.ids.get(&id).copied()
    }

    pub fn id_of(&self, entity: Entity) -> Option<Uuid> {
        self.by_entity.get(&entity).copied()
    }

    /// Authoring ids in document order.
    pub fn ids(&self) -> &[Uuid] {
        &self.order
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// `id` and every descendant the document gives it, in document order
    /// (what "hide this entity" hides).
    pub fn subtree(&self, id: Uuid) -> Vec<Uuid> {
        let mut out = Vec::new();
        let mut stack = vec![id];
        while let Some(id) = stack.pop() {
            if !self.described.contains_key(&id) {
                continue;
            }
            out.push(id);
            if let Some(d) = self.described.get(&id) {
                stack.extend(d.children.iter().rev().map(|c| to_uuid(*c)));
            }
        }
        out
    }

    // --------------------------------------------------------------- sync

    /// Brings `world` to `desired` — the incremental sync rule of the
    /// module docs. Returns what changed; `reloaded` when the diff could
    /// not converge and the scene was loaded fresh instead.
    pub fn sync(
        &mut self,
        world: &mut World,
        assets: Option<&Assets>,
        desired: &SceneDescription,
    ) -> SyncReport {
        self.syncs += 1;
        let mut report = SyncReport::default();
        let before = world.diagnostics().len();
        let mut last_plan: Option<Vec<String>> = None;
        for _round in 0..MAX_SYNC_ROUNDS {
            let plan = self.plan(world, desired);
            if plan.is_empty() {
                break;
            }
            let signature: Vec<String> = plan.iter().map(|c| format!("{c:?}")).collect();
            if last_plan.as_ref() == Some(&signature) {
                // The world refused the same commands twice: give up on
                // the incremental path.
                report.reloaded = true;
                break;
            }
            last_plan = Some(signature);
            for c in &plan {
                match c {
                    Command::Spawn { .. } => report.spawned += 1,
                    Command::Destroy(_) => report.destroyed += 1,
                    _ => report.updated += 1,
                }
            }
            world.submit(Lane::Simulation, plan);
            world.step();
            report.steps += 1;
            for ev in world.drain_events() {
                match ev.payload {
                    EventPayload::Spawned { temp_id, entity } => {
                        let id = to_uuid(desired.entities[temp_id as usize - 1].id);
                        self.ids.insert(id, entity);
                        self.by_entity.insert(entity, id);
                    }
                    EventPayload::Destroyed { entity, .. } => {
                        if let Some(id) = self.by_entity.remove(&entity) {
                            self.ids.remove(&id);
                        }
                    }
                    _ => {}
                }
            }
        }
        if !report.reloaded && !self.plan(world, desired).is_empty() {
            report.reloaded = true;
        }
        if report.reloaded {
            report
                .diagnostics
                .extend(world.diagnostics()[before..].iter().map(world_diagnostic));
            self.reload(world, assets, desired);
            report.diagnostics.extend(self.diagnostics.iter().cloned());
            return report;
        }
        report.reordered = self.reorder(world, desired);
        self.diagnostics = world.diagnostics()[before..]
            .iter()
            .map(world_diagnostic)
            .collect();
        report.diagnostics = self.diagnostics.clone();
        self.remember(desired);
        self.register_assets(world, assets, desired);
        report
    }

    /// Everything that differs between the world and `desired` and can be
    /// commanded now (a spawn under a parent that does not exist yet
    /// waits for the next round).
    fn plan(&self, world: &World, desired: &SceneDescription) -> Vec<Command> {
        let ecs = world.ecs();
        let mut out = Vec::new();
        let wanted: HashSet<Uuid> = desired.entities.iter().map(|e| to_uuid(e.id)).collect();
        for (i, d) in desired.entities.iter().enumerate() {
            let id = to_uuid(d.id);
            let parent_entity = match d.parent {
                None => Some(None),
                Some(p) => self.ids.get(&to_uuid(p)).map(|e| Some(*e)),
            };
            let transform = d
                .components
                .iter()
                .find_map(|c| match c {
                    ComponentValue::Transform(t) => Some(*t),
                    _ => None,
                })
                .unwrap_or(Transform::IDENTITY);
            match self.ids.get(&id) {
                None => {
                    if let Some(parent) = parent_entity {
                        out.push(Command::Spawn {
                            temp_id: i as u64 + 1,
                            transform,
                            parent,
                            name: Some(d.name.clone()),
                        });
                    }
                }
                Some(&e) => {
                    if !ecs.contains(e) {
                        continue;
                    }
                    let name = ecs.copy::<Name>(e).map(|n| n.value).unwrap_or_default();
                    if name != d.name {
                        out.push(Command::SetName {
                            entity: e,
                            name: d.name.clone(),
                        });
                    }
                    if scene::is_active_self(ecs, e) != d.active {
                        out.push(Command::SetActive {
                            entity: e,
                            active: d.active,
                        });
                    }
                    let current_parent =
                        scene::parent_of(ecs, e).and_then(|p| self.by_entity.get(&p).copied());
                    let desired_parent = d.parent.map(to_uuid);
                    if current_parent != desired_parent {
                        if let Some(parent) = parent_entity {
                            out.push(Command::SetParent {
                                child: e,
                                parent,
                                keep_world: false,
                            });
                        }
                    }
                    if ecs.copy::<Transform>(e).ok() != Some(transform) {
                        out.push(Command::SetTransform {
                            entity: e,
                            transform,
                        });
                    }
                    let have = ComponentValue::all_of(world, e);
                    for c in &d.components {
                        if matches!(c, ComponentValue::Transform(_)) {
                            continue;
                        }
                        if !have.iter().any(|h| h == c) {
                            out.push(Command::SetComponent {
                                entity: e,
                                value: c.clone(),
                            });
                        }
                    }
                    for h in &have {
                        if matches!(h, ComponentValue::Transform(_)) {
                            continue;
                        }
                        if !d.components.iter().any(|c| c.id() == h.id()) {
                            out.push(Command::RemoveComponent {
                                entity: e,
                                component: h.id(),
                            });
                        }
                    }
                }
            }
        }
        // Gone entities: the topmost of each gone subtree (a destroy takes
        // the children; the ones the document kept were reparented above,
        // in the same batch, before the destroy).
        for (id, &e) in &self.ids {
            if wanted.contains(id) || !ecs.contains(e) {
                continue;
            }
            let mut ancestor_gone = false;
            let mut p = scene::parent_of(ecs, e);
            while let Some(pe) = p {
                match self.by_entity.get(&pe) {
                    Some(pid) if !wanted.contains(pid) => {
                        ancestor_gone = true;
                        break;
                    }
                    _ => {}
                }
                p = scene::parent_of(ecs, pe);
            }
            if !ancestor_gone {
                out.push(Command::Destroy(e));
            }
        }
        out
    }

    /// Sibling order as the document lists it (no command: the child
    /// lists are the ECS's; order never moves a transform).
    fn reorder(&self, world: &mut World, desired: &SceneDescription) -> usize {
        let mut moved = 0;
        for d in &desired.entities {
            let Some(&e) = self.ids.get(&to_uuid(d.id)) else {
                continue;
            };
            let want: Vec<Entity> = d
                .children
                .iter()
                .filter_map(|c| self.ids.get(&to_uuid(*c)).copied())
                .collect();
            let have = scene::children_of(world.ecs(), e);
            if have == want {
                continue;
            }
            for (i, c) in want.iter().enumerate() {
                let _ = scene::set_sibling_index(world.ecs_mut(), *c, i);
                moved += 1;
            }
        }
        moved
    }

    // --------------------------------------------------------------- dump

    /// The runtime state keyed by authoring id in document order — the
    /// shape of `fynkel state dump --json` (`fynkel_headless`): name,
    /// the active flags, parent, children, local and world transforms,
    /// every built-in component encoded as the document would.
    pub fn dump(&self, world: &World) -> Value {
        let ecs = world.ecs();
        let mut entities = Vec::new();
        for id in &self.order {
            let Some(&e) = self.ids.get(id) else {
                continue;
            };
            if !ecs.contains(e) {
                continue;
            }
            let name = ecs.copy::<Name>(e).map(|n| n.value).unwrap_or_default();
            let parent = scene::parent_of(ecs, e)
                .and_then(|p| self.by_entity.get(&p))
                .map(|u| u.to_string());
            let children: Vec<Value> = scene::children_of(ecs, e)
                .into_iter()
                .filter_map(|c| self.by_entity.get(&c))
                .map(|u| Value::String(u.to_string()))
                .collect();
            let local = ecs.copy::<Transform>(e).unwrap_or_default();
            let world_t = scene::world_matrix(ecs, e)
                .map(Transform::from_matrix)
                .unwrap_or_default();
            let components: Vec<Value> = ComponentValue::all_of(world, e)
                .iter()
                .map(|c| {
                    let (type_id, data) = encode_component(c);
                    json!({ "type": type_id, "data": data.to_serde() })
                })
                .collect();
            entities.push(json!({
                "id": id.to_string(),
                "name": name,
                "activeSelf": scene::is_active_self(ecs, e),
                "activeInHierarchy": scene::is_active_in_hierarchy(ecs, e),
                "parent": parent,
                "children": children,
                "local": local.to_node().to_serde(),
                "world": world_t.to_node().to_serde(),
                "components": components,
            }));
        }
        json!({
            "document": self.document.to_string(),
            "entityCount": entities.len(),
            "entities": entities,
        })
    }

    /// Counts for the stats overlay: entities in the preview, live
    /// entities in the world, meshes, lights, cameras, hidden.
    pub fn stats(&self, world: &World) -> Value {
        let extract = world.extract();
        json!({
            "entities": self.ids.len(),
            "live": world.ecs().entities().count(),
            "meshes": extract.meshes.len(),
            "lights": extract.directional_lights.len() + extract.point_lights.len() + extract.spot_lights.len(),
            "hidden": world.edit_hidden().count(),
            "tick": world.tick(),
            "loads": self.loads,
            "syncs": self.syncs,
        })
    }

    // ------------------------------------------------------------ anchors

    /// The anchor map of the description last loaded or synced (from
    /// `AuthoringSession::describe_with_anchors`): picks and marquees
    /// answer with the anchor of an entity a prefab instance expanded,
    /// the outline of an anchor covers its instance, hiding an anchor
    /// hides its instance. Survives loads and syncs until set again.
    pub fn set_anchors(&mut self, anchors: HashMap<Uuid, Uuid>) {
        self.anchors = anchors;
    }

    /// The id the editor knows for `id`: its instance's anchor, or itself.
    pub fn anchor_of(&self, id: Uuid) -> Uuid {
        self.anchors.get(&id).copied().unwrap_or(id)
    }

    /// `ids` plus every expanded entity anchored to one of them.
    fn with_anchored(&self, ids: &[Uuid]) -> Vec<Uuid> {
        let set: HashSet<Uuid> = ids.iter().copied().collect();
        let mut out: Vec<Uuid> = ids.to_vec();
        for id in &self.order {
            if !set.contains(id) && set.contains(&self.anchor_of(*id)) {
                out.push(*id);
            }
        }
        out
    }

    // ------------------------------------------------------------ editing

    /// The gizmo's preview: the entities' local transforms set directly
    /// (no document change) and one step so the picture follows the
    /// drag. `Err` names an id the preview does not have. The document's
    /// transaction at the drag's end brings the document to the same
    /// values and the next `sync` finds nothing to do.
    pub fn set_transforms(
        &self,
        world: &mut World,
        transforms: &[(Uuid, Transform)],
    ) -> Result<(), Diagnostic> {
        let mut commands = Vec::with_capacity(transforms.len());
        for (id, t) in transforms {
            let Some(&e) = self.ids.get(id) else {
                return Err(unknown_entity(*id));
            };
            commands.push(Command::SetTransform {
                entity: e,
                transform: *t,
            });
        }
        world.submit(Lane::Simulation, commands);
        world.step();
        world.drain_events();
        Ok(())
    }

    /// The local transforms back to the document's (a cancelled drag).
    pub fn restore_transforms(&self, world: &mut World, ids: &[Uuid]) -> Result<(), Diagnostic> {
        let mut transforms = Vec::with_capacity(ids.len());
        for id in ids {
            let Some(d) = self.described.get(id) else {
                return Err(unknown_entity(*id));
            };
            let t = d
                .components
                .iter()
                .find_map(|c| match c {
                    ComponentValue::Transform(t) => Some(*t),
                    _ => None,
                })
                .unwrap_or(Transform::IDENTITY);
            transforms.push((*id, t));
        }
        self.set_transforms(world, &transforms)
    }

    /// The Inspector's live edit of a component (a colour being picked):
    /// each `(id, type, data)` set on the entity directly (no document
    /// change) and one step so the picture follows. `Err` names an id the
    /// preview does not have, or data the type does not decode; a type
    /// that is not built in (a behaviour) is skipped — nothing draws it.
    /// The commit's transaction brings the document to the same values.
    pub fn set_components(
        &self,
        world: &mut World,
        components: &[(Uuid, String, Value)],
    ) -> Result<usize, Diagnostic> {
        let mut commands = Vec::with_capacity(components.len());
        for (id, type_id, data) in components {
            let Some(&e) = self.ids.get(id) else {
                return Err(unknown_entity(*id));
            };
            let node = fynkel_document::Node::from_serde(data);
            let value = match fynkel_document::generated::decode_component(type_id, &node) {
                None => continue,
                Some(Ok(v)) => v,
                Some(Err(err)) => {
                    return Err(Diagnostic::error(
                        Code::SESSION_INVALID_REQUEST,
                        format!("{type_id} at {}: {}", err.pointer, err.message),
                    )
                    .with_id(id.to_string()))
                }
            };
            commands.push(Command::SetComponent { entity: e, value });
        }
        let count = commands.len();
        if count > 0 {
            world.submit(Lane::Simulation, commands);
            world.step();
            world.drain_events();
        }
        Ok(count)
    }

    /// The components of `ids` back to the document's (a cancelled live
    /// edit): every described component but the Transform set again.
    pub fn restore_components(&self, world: &mut World, ids: &[Uuid]) -> Result<(), Diagnostic> {
        let mut commands = Vec::new();
        for id in ids {
            let (Some(d), Some(&e)) = (self.described.get(id), self.ids.get(id)) else {
                return Err(unknown_entity(*id));
            };
            for c in &d.components {
                if !matches!(c, ComponentValue::Transform(_)) {
                    commands.push(Command::SetComponent {
                        entity: e,
                        value: c.clone(),
                    });
                }
            }
        }
        if !commands.is_empty() {
            world.submit(Lane::Simulation, commands);
            world.step();
            world.drain_events();
        }
        Ok(())
    }

    /// The Inspector's live edit of a material document: `data` (a
    /// `fynkel.pbr@1` document's fields; anything else in it ignored)
    /// decoded and drawn in place of the cooked material `material` by
    /// every renderer on `assets`, the file untouched. The save's recook
    /// brings the cache to the same values; [`Preview::restore_material`]
    /// puts the cooked one back (a cancelled edit).
    pub fn set_material(assets: &Assets, material: Uuid, data: &Value) -> Result<(), Diagnostic> {
        let node = fynkel_document::Node::from_serde(data);
        let m = fynkel_document::material::PbrMaterial::from_node(&node).map_err(|err| {
            Diagnostic::error(
                Code::SESSION_INVALID_REQUEST,
                format!("material at {}: {}", err.pointer, err.message),
            )
            .with_id(material.to_string())
        })?;
        assets.override_material(
            &AssetRef::new(fynkel_contracts::Uuid(*material.as_bytes())),
            m,
        );
        Ok(())
    }

    /// What the renderers on `assets` draw for `material` now — the live
    /// edit while one is set, else the cooked record — as its document
    /// fields.
    pub fn material(assets: &Assets, material: Uuid) -> Result<Value, Diagnostic> {
        let m = assets.material(&AssetRef::new(fynkel_contracts::Uuid(*material.as_bytes())))?;
        Ok(m.to_node().to_serde())
    }

    /// The cooked material `material` again after [`Preview::set_material`].
    pub fn restore_material(assets: &Assets, material: Uuid) {
        assets.restore_material(&AssetRef::new(fynkel_contracts::Uuid(*material.as_bytes())));
    }

    /// Every renderer on `assets` draws `volume` (a volume profile's id)
    /// with the fields of `data` — the profile document as the editor
    /// holds it, unsaved — until [`Preview::restore_volume`]: the
    /// profile Inspector's live edit (its envelope, if any, is not
    /// read). A field that does not read is refused.
    pub fn set_volume(assets: &Assets, volume: Uuid, data: &Value) -> Result<(), Diagnostic> {
        let refuse = |message: String| {
            Diagnostic::error(Code::SESSION_INVALID_REQUEST, message).with_id(volume.to_string())
        };
        let kind = fynkel_document::DocumentKind::Volume;
        let mut fields = fynkel_document::Document::new(kind, "live").root.to_serde();
        let fields = fields.as_object_mut().expect("a document is an object");
        fields.retain(|k, _| {
            matches!(
                k.as_str(),
                "$schema" | "kind" | "formatVersion" | "id" | "name"
            )
        });
        for (k, v) in data.as_object().into_iter().flatten() {
            fields.entry(k.clone()).or_insert_with(|| v.clone());
        }
        let document = fynkel_document::Document::parse(
            kind,
            None,
            &Value::Object(fields.clone()).to_string(),
        )
        .map_err(|d| refuse(format!("volume profile: {}", d[0].message)))?;
        let v = fynkel_document::VolumeDocument::from_document(document);
        if let Some(d) = v.read_diagnostics().first() {
            return Err(refuse(format!("volume profile: {}", d.message)));
        }
        assets.override_volume(
            &AssetRef::new(fynkel_contracts::Uuid(*volume.as_bytes())),
            v.profile,
        );
        Ok(())
    }

    /// What the renderers on `assets` blend for `volume` now — the live
    /// edit while one is set, else the cooked profile — as its present
    /// fields (`{ "bloom": { "intensity": 2 } }`).
    pub fn volume(assets: &Assets, volume: Uuid) -> Result<Value, Diagnostic> {
        let p =
            assets.volume_profile(&AssetRef::new(fynkel_contracts::Uuid(*volume.as_bytes())))?;
        let mut out = serde_json::Map::new();
        let mut block = |name: &str, fields: Vec<(&str, Option<Value>)>| {
            let present: serde_json::Map<String, Value> = fields
                .into_iter()
                .filter_map(|(k, v)| Some((k.to_string(), v?)))
                .collect();
            if !present.is_empty() {
                out.insert(name.to_string(), Value::Object(present));
            }
        };
        let n = |v: Option<f32>| v.map(|v| serde_json::json!(v));
        let ao = p.ambient_occlusion;
        block(
            "ambientOcclusion",
            vec![("radius", n(ao.radius)), ("intensity", n(ao.intensity))],
        );
        let b = p.bloom;
        block(
            "bloom",
            vec![
                ("intensity", n(b.intensity)),
                ("threshold", n(b.threshold)),
                ("scatter", n(b.scatter)),
                ("tint", b.tint.map(|t| serde_json::json!([t.x, t.y, t.z]))),
            ],
        );
        let c = p.color_grading;
        let rgb = |v: Option<glam::Vec3>| v.map(|t| serde_json::json!([t.x, t.y, t.z]));
        block(
            "colorGrading",
            vec![
                (
                    "tonemapper",
                    c.tonemapper.map(|t| serde_json::json!(t.name())),
                ),
                ("postExposure", n(c.post_exposure)),
                ("temperature", n(c.temperature)),
                ("tint", n(c.tint)),
                ("contrast", n(c.contrast)),
                ("saturation", n(c.saturation)),
                ("hueShift", n(c.hue_shift)),
                ("colorFilter", rgb(c.color_filter)),
                ("lift", rgb(c.lift)),
                ("gamma", rgb(c.gamma)),
                ("gain", rgb(c.gain)),
            ],
        );
        let cs = p.contact_shadows;
        block(
            "contactShadows",
            vec![
                ("intensity", n(cs.intensity)),
                ("length", n(cs.length)),
                ("thickness", n(cs.thickness)),
                ("distance", n(cs.distance)),
            ],
        );
        Ok(Value::Object(out))
    }

    /// The cooked profile `volume` again after [`Preview::set_volume`].
    pub fn restore_volume(assets: &Assets, volume: Uuid) {
        assets.restore_volume(&AssetRef::new(fynkel_contracts::Uuid(*volume.as_bytes())));
    }

    /// The editor's hidden flag: exactly `ids` (the caller passes the
    /// document subtrees; the entities of the prefab instances among
    /// them come along) left out of the picture, and out of picking.
    pub fn set_hidden(&self, world: &mut World, ids: &[Uuid]) {
        let entities: Vec<Entity> = self
            .with_anchored(ids)
            .iter()
            .filter_map(|id| self.ids.get(id).copied())
            .collect();
        world.set_edit_hidden(&entities);
    }

    /// The world matrix of `id` now.
    pub fn world_matrix(&self, world: &World, id: Uuid) -> Option<Mat4> {
        let e = self.ids.get(&id)?;
        scene::world_matrix(world.ecs(), *e).ok()
    }

    // ------------------------------------------------------------ picking

    /// Every pickable entity `ray` hits, nearest first: a visible mesh
    /// through its bounding sphere then its triangles (in mesh space; a
    /// skinned mesh is tested unposed), any other entity through an
    /// [`ICON_RADIUS`] sphere at its origin. Hidden entities never hit. A
    /// hit inside a prefab instance is reported as its anchor (once, at
    /// the nearest hit); `object` and `entity` stay the one the ray met.
    pub fn pick(&self, world: &World, assets: Option<&Assets>, ray: &Ray) -> Vec<Hit> {
        let mut hits = self.pick_entities(world, assets, ray);
        for h in hits.iter_mut() {
            h.id = self.anchor_of(h.id);
        }
        let mut seen = HashSet::new();
        hits.retain(|h| seen.insert(h.id));
        hits
    }

    fn pick_entities(&self, world: &World, assets: Option<&Assets>, ray: &Ray) -> Vec<Hit> {
        let hidden: HashSet<Entity> = world.edit_hidden().collect();
        let mut hits = Vec::new();
        for id in &self.order {
            let Some(&e) = self.ids.get(id) else {
                continue;
            };
            if hidden.contains(&e) || !scene::is_active_in_hierarchy(world.ecs(), e) {
                continue;
            }
            if let Some((distance, point, mesh)) = pick_entity(world, assets, ray, e) {
                hits.push(Hit {
                    id: *id,
                    object: *id,
                    entity: e,
                    distance,
                    point,
                    mesh,
                });
            }
        }
        hits.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        hits
    }

    /// Marquee selection: the ids whose projected bounds (a mesh's
    /// world-space bounding box; an entity without one, its origin)
    /// overlap the rectangle `min..max` in NDC. An entity partly behind
    /// the camera counts by its origin alone. Hidden and inactive entities
    /// never count.
    pub fn pick_rect(
        &self,
        world: &World,
        assets: Option<&Assets>,
        view: &View,
        min: Vec2,
        max: Vec2,
    ) -> Vec<Uuid> {
        let hidden: HashSet<Entity> = world.edit_hidden().collect();
        let (min, max) = (min.min(max), min.max(max));
        let mut out = Vec::new();
        for id in &self.order {
            let Some(&e) = self.ids.get(id) else {
                continue;
            };
            if hidden.contains(&e) || !scene::is_active_in_hierarchy(world.ecs(), e) {
                continue;
            }
            if rect_hits(world, assets, view, e, min, max) {
                let id = self.anchor_of(*id);
                if !out.contains(&id) {
                    out.push(id);
                }
            }
        }
        out
    }

    // ----------------------------------------------------------- overlays

    /// The selection outline: each id's bounds as twelve world-space
    /// edges (a mesh's box under its world matrix; an entity without a
    /// mesh, an [`ICON_RADIUS`] cross at its origin). An anchor's
    /// instance is outlined with it, mesh by mesh.
    pub fn outline_lines(
        &self,
        world: &World,
        assets: Option<&Assets>,
        ids: &[Uuid],
        color: Vec4,
    ) -> Vec<Line> {
        self.outline(world, assets, ids, color, true)
    }

    /// [`outline_lines`](Self::outline_lines) without the boxes: only the
    /// crosses of the entities that draw no mesh. The meshes are ringed by
    /// the renderer's silhouette outline instead
    /// (`fynkel_render::Overlay::outline`, fed by [`entities`](Self::entities)).
    pub fn outline_crosses(
        &self,
        world: &World,
        assets: Option<&Assets>,
        ids: &[Uuid],
        color: Vec4,
    ) -> Vec<Line> {
        self.outline(world, assets, ids, color, false)
    }

    fn outline(
        &self,
        world: &World,
        assets: Option<&Assets>,
        ids: &[Uuid],
        color: Vec4,
        boxes: bool,
    ) -> Vec<Line> {
        let mut out = Vec::new();
        let ids = self.with_anchored(ids);
        for id in &ids {
            let Some(&e) = self.ids.get(id) else {
                continue;
            };
            outline_entity(world, assets, e, color, boxes, &mut out);
        }
        out
    }

    /// The world's handles for `ids` — with the entities of the prefab
    /// instances they anchor, so a selected instance names every mesh it
    /// expanded to — leaving out what is gone and what the editor hides.
    /// What the renderer's selection outline takes
    /// (`fynkel_render::Overlay::outline`, by `Entity::to_bits` over the
    /// ABI); an inactive entity is named but draws nothing to ring.
    pub fn entities(&self, world: &World, ids: &[Uuid]) -> Vec<Entity> {
        let hidden: HashSet<Entity> = world.edit_hidden().collect();
        let mut out: Vec<Entity> = Vec::new();
        for id in self.with_anchored(ids) {
            let Some(&e) = self.ids.get(&id) else {
                continue;
            };
            if world.ecs().contains(e) && !hidden.contains(&e) && !out.contains(&e) {
                out.push(e);
            }
        }
        out
    }

    /// The icons of every light, camera and collider in the preview (the
    /// hidden and the inactive ones left out): a point light's range as
    /// three circles, a spot's cone (outer angle, inner circle) to its
    /// range, a directional light's arrows along its −Z, a camera's
    /// frustum to [`CAMERA_OUTLINE_DEPTH`] past its near plane at `aspect`, a
    /// collider's shape (box, sphere, capsule; a mesh collider its mesh's
    /// box) at its offset. Colours: lights their own colour, cameras
    /// white, colliders green. Colliders are drawn for the `selected` ids
    /// only (with their anchored instances), as Unity does — a level whose
    /// every wall collides would otherwise be traced in green — and a
    /// camera that is not selected draws its frustum at a third of its
    /// alpha.
    pub fn icon_lines(
        &self,
        world: &World,
        assets: Option<&Assets>,
        aspect: f32,
        selected: &[Uuid],
    ) -> Vec<Line> {
        let ecs = world.ecs();
        let hidden: HashSet<Entity> = world.edit_hidden().collect();
        let selected: HashSet<Uuid> = self.with_anchored(selected).into_iter().collect();
        let mut out = Vec::new();
        for id in &self.order {
            let Some(&e) = self.ids.get(id) else {
                continue;
            };
            if hidden.contains(&e) || !ecs.contains(e) || !scene::is_active_in_hierarchy(ecs, e) {
                continue;
            }
            icon_entity(world, assets, aspect, e, selected.contains(id), &mut out);
        }
        out
    }
}

/// `e`'s icons (see [`Preview::icon_lines`]); its collider only when
/// `is_selected`.
pub(crate) fn icon_entity(
    world: &World,
    assets: Option<&Assets>,
    aspect: f32,
    e: Entity,
    is_selected: bool,
    out: &mut Vec<Line>,
) {
    let ecs = world.ecs();
    let Ok(m) = scene::world_matrix(ecs, e) else {
        return;
    };
    let origin = m.transform_point3(Vec3::ZERO);
    let rigid = rigid_of(&m);
    if let Ok(l) = ecs.copy::<fynkel_core::components::PointLight>(e) {
        let color = l.color.extend(1.0);
        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
            circle_lines(origin, axis, l.range, color, out);
        }
    }
    if let Ok(l) = ecs.copy::<fynkel_core::components::SpotLight>(e) {
        let color = l.color.extend(1.0);
        let forward = rigid.transform_vector3(Vec3::NEG_Z);
        let end = origin + forward * l.range;
        let outer = l.range * l.outer_angle.tan();
        let inner = l.range * l.inner_angle.tan();
        circle_lines(end, forward, outer, color, out);
        circle_lines(
            end,
            forward,
            inner,
            color * Vec4::new(1.0, 1.0, 1.0, 0.5),
            out,
        );
        for dir in [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y] {
            let rim = end + rigid.transform_vector3(dir) * outer;
            out.push(Line {
                a: origin,
                b: rim,
                color,
            });
        }
    }
    if let Ok(l) = ecs.copy::<fynkel_core::components::DirectionalLight>(e) {
        let color = l.color.extend(1.0);
        let forward = rigid.transform_vector3(Vec3::NEG_Z);
        for dir in [Vec3::ZERO, Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y] {
            let start = origin + rigid.transform_vector3(dir) * 0.2;
            arrow_lines(start, start + forward, &rigid, color, out);
        }
        circle_lines(origin, forward, 0.2, color, out);
    }
    if let Ok(c) = ecs.copy::<Camera>(e) {
        let alpha = if is_selected {
            1.0
        } else {
            UNSELECTED_CAMERA_ALPHA
        };
        camera_lines(&rigid, &c, aspect, Vec4::new(1.0, 1.0, 1.0, alpha), out);
    }
    if is_selected {
        if let Ok(c) = ecs.get::<Collider>(e) {
            collider_lines(&m, &c, assets, Vec4::new(0.2, 1.0, 0.3, 1.0), out);
        }
    }
}

/// A world diagnostic (a rejected command: a component value the
/// runtime refused, a stale entity) as the session reports it — the
/// document's content was valid, so this is the runtime's `R0008`.
fn world_diagnostic(d: &fynkel_core::Diagnostic) -> Diagnostic {
    Diagnostic::warning(
        Code::RUNTIME_SCENE_INVALID,
        format!("preview world: {} (code {})", d.message, d.code.0),
    )
}

fn unknown_entity(id: Uuid) -> Diagnostic {
    Diagnostic::error(
        Code::SESSION_INVALID_REQUEST,
        format!("entity {id} is not in the preview"),
    )
    .with_id(id.to_string())
    .with_suggestion(
        "the preview holds the document's entities after the last sync; resync or reload",
    )
}

/// Where `ray` meets `e` (see [`Preview::pick`]): distance, point, and
/// whether a mesh triangle was hit.
pub(crate) fn pick_entity(
    world: &World,
    assets: Option<&Assets>,
    ray: &Ray,
    e: Entity,
) -> Option<(f32, Vec3, bool)> {
    let m = scene::world_matrix(world.ecs(), e).ok()?;
    match mesh_of(world, assets, e) {
        Some(mesh) => {
            let sphere = bounds_sphere(&mesh, &m);
            ray.hit_sphere(sphere.0, sphere.1)?;
            let local = ray.transformed(&m.inverse());
            let mut best: Option<f32> = None;
            for tri in mesh.indices.chunks_exact(3) {
                let a = Vec3::from(mesh.vertices[tri[0] as usize].position);
                let b = Vec3::from(mesh.vertices[tri[1] as usize].position);
                let c = Vec3::from(mesh.vertices[tri[2] as usize].position);
                if let Some(t) = local.hit_triangle(a, b, c) {
                    if best.is_none_or(|b| t < b) {
                        best = Some(t);
                    }
                }
            }
            let point = m.transform_point3(local.origin + local.dir * best?);
            Some(((point - ray.origin).dot(ray.dir), point, true))
        }
        None => {
            let center = m.transform_point3(Vec3::ZERO);
            let t = ray.hit_sphere(center, ICON_RADIUS)?;
            Some((t, ray.origin + ray.dir * t, false))
        }
    }
}

/// Whether `e`'s projected bounds overlap `min..max` in NDC (see
/// [`Preview::pick_rect`]; `min` ≤ `max`).
pub(crate) fn rect_hits(
    world: &World,
    assets: Option<&Assets>,
    view: &View,
    e: Entity,
    min: Vec2,
    max: Vec2,
) -> bool {
    let Ok(m) = scene::world_matrix(world.ecs(), e) else {
        return false;
    };
    let corners: Vec<Vec3> = match mesh_of(world, assets, e) {
        Some(mesh) => aabb_corners(Vec3::from(mesh.bounds_min), Vec3::from(mesh.bounds_max))
            .iter()
            .map(|c| m.transform_point3(*c))
            .collect(),
        None => vec![m.transform_point3(Vec3::ZERO)],
    };
    let projected: Option<Vec<Vec2>> = corners.iter().map(|c| view.project(*c)).collect();
    let points = match projected {
        Some(p) => p,
        None => match view.project(m.transform_point3(Vec3::ZERO)) {
            Some(p) => vec![p],
            None => return false,
        },
    };
    let lo = points.iter().fold(Vec2::splat(f32::MAX), |a, p| a.min(*p));
    let hi = points.iter().fold(Vec2::splat(f32::MIN), |a, p| a.max(*p));
    lo.x <= max.x && hi.x >= min.x && lo.y <= max.y && hi.y >= min.y
}

/// `e`'s selection outline (see [`Preview::outline_lines`]); without
/// `boxes`, only the cross of an entity that draws no mesh (see
/// [`Preview::outline_crosses`]).
pub(crate) fn outline_entity(
    world: &World,
    assets: Option<&Assets>,
    e: Entity,
    color: Vec4,
    boxes: bool,
    out: &mut Vec<Line>,
) {
    let Ok(m) = scene::world_matrix(world.ecs(), e) else {
        return;
    };
    if !boxes {
        // Whatever the renderer draws for it — the mesh, or the magenta
        // cube of one the cache lacks — carries the silhouette.
        if !draws_mesh(world, e) {
            cross_lines(m.transform_point3(Vec3::ZERO), ICON_RADIUS, color, out);
        }
        return;
    }
    match mesh_of(world, assets, e) {
        Some(mesh) => box_lines(
            &m,
            Vec3::from(mesh.bounds_min),
            Vec3::from(mesh.bounds_max),
            color,
            out,
        ),
        None => cross_lines(m.transform_point3(Vec3::ZERO), ICON_RADIUS, color, out),
    }
}

/// Whether the renderer draws a mesh for `e`: a visible `MeshRenderer`
/// naming one.
fn draws_mesh(world: &World, e: Entity) -> bool {
    world
        .ecs()
        .copy::<MeshRenderer>(e)
        .is_ok_and(|r| r.visible && !r.mesh.asset.is_nil())
}

/// The visible mesh an entity draws, read from the assets.
fn mesh_of(
    world: &World,
    assets: Option<&Assets>,
    e: Entity,
) -> Option<std::sync::Arc<MeshPayload>> {
    let assets = assets?;
    let r = world.ecs().copy::<MeshRenderer>(e).ok()?;
    if !r.visible || r.mesh.asset.is_nil() {
        return None;
    }
    assets.mesh(&r.mesh).ok()
}

/// The bounding sphere of a mesh's box under `m` (centre and radius; the
/// radius scaled by the largest axis, conservative for non-uniform scale).
fn bounds_sphere(mesh: &MeshPayload, m: &Mat4) -> (Vec3, f32) {
    let (min, max) = (Vec3::from(mesh.bounds_min), Vec3::from(mesh.bounds_max));
    let scale = m
        .x_axis
        .truncate()
        .length()
        .max(m.y_axis.truncate().length())
        .max(m.z_axis.truncate().length());
    (
        m.transform_point3((min + max) * 0.5),
        (max - min).length() * 0.5 * scale,
    )
}

/// `m` without its scale (icons keep their size).
fn rigid_of(m: &Mat4) -> Mat4 {
    let (_, rotation, translation) = m.to_scale_rotation_translation();
    Mat4::from_rotation_translation(rotation, translation)
}

fn aabb_corners(min: Vec3, max: Vec3) -> [Vec3; 8] {
    [
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(max.x, max.y, max.z),
        Vec3::new(min.x, max.y, max.z),
    ]
}

/// The twelve edges of the box `min..max` under `m`.
pub fn box_lines(m: &Mat4, min: Vec3, max: Vec3, color: Vec4, out: &mut Vec<Line>) {
    let c: Vec<Vec3> = aabb_corners(min, max)
        .iter()
        .map(|p| m.transform_point3(*p))
        .collect();
    const EDGES: [(usize, usize); 12] = [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0),
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 4),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ];
    for (a, b) in EDGES {
        out.push(Line {
            a: c[a],
            b: c[b],
            color,
        });
    }
}

/// Three axis-aligned segments through `center`.
pub fn cross_lines(center: Vec3, radius: f32, color: Vec4, out: &mut Vec<Line>) {
    for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
        out.push(Line {
            a: center - axis * radius,
            b: center + axis * radius,
            color,
        });
    }
}

/// A circle of `radius` around `center` in the plane normal to `normal`.
pub fn circle_lines(center: Vec3, normal: Vec3, radius: f32, color: Vec4, out: &mut Vec<Line>) {
    if radius <= 0.0 {
        return;
    }
    let n = normal.normalize_or(Vec3::Y);
    let u = n.any_orthonormal_vector();
    let v = n.cross(u);
    let mut prev = center + u * radius;
    for i in 1..=CIRCLE_SEGMENTS {
        let a = i as f32 / CIRCLE_SEGMENTS as f32 * std::f32::consts::TAU;
        let p = center + (u * a.cos() + v * a.sin()) * radius;
        out.push(Line {
            a: prev,
            b: p,
            color,
        });
        prev = p;
    }
}

/// A segment with a small head at `to`.
fn arrow_lines(from: Vec3, to: Vec3, rigid: &Mat4, color: Vec4, out: &mut Vec<Line>) {
    out.push(Line {
        a: from,
        b: to,
        color,
    });
    let dir = (to - from).normalize_or(Vec3::NEG_Z);
    let side = rigid.transform_vector3(Vec3::X);
    let up = rigid.transform_vector3(Vec3::Y);
    for w in [side, -side, up, -up] {
        out.push(Line {
            a: to,
            b: to - dir * 0.15 + w * 0.06,
            color,
        });
    }
}

/// A camera's frustum from its near plane to [`CAMERA_OUTLINE_DEPTH`]
/// past it (or its far plane when nearer), at `aspect`.
fn camera_lines(rigid: &Mat4, camera: &Camera, aspect: f32, color: Vec4, out: &mut Vec<Line>) {
    let near = camera.near.max(1e-3);
    let far = camera.far.min(near + CAMERA_OUTLINE_DEPTH);
    let half = |d: f32| -> (f32, f32) {
        match camera.projection {
            Projection::Perspective => {
                let h = d * (camera.fov_y * 0.5).tan();
                (h * aspect, h)
            }
            Projection::Orthographic => {
                let h = camera.ortho_height * 0.5;
                (h * aspect, h)
            }
        }
    };
    let corners = |d: f32| -> [Vec3; 4] {
        let (w, h) = half(d);
        [
            rigid.transform_point3(Vec3::new(-w, -h, -d)),
            rigid.transform_point3(Vec3::new(w, -h, -d)),
            rigid.transform_point3(Vec3::new(w, h, -d)),
            rigid.transform_point3(Vec3::new(-w, h, -d)),
        ]
    };
    let n = corners(near);
    let f = corners(far);
    for i in 0..4 {
        let j = (i + 1) % 4;
        out.push(Line {
            a: n[i],
            b: n[j],
            color,
        });
        out.push(Line {
            a: f[i],
            b: f[j],
            color,
        });
        out.push(Line {
            a: n[i],
            b: f[i],
            color,
        });
    }
    // The "up" triangle above the near plane, as every 3D editor draws it.
    let (w, h) = half(near);
    out.push(Line {
        a: n[3],
        b: rigid.transform_point3(Vec3::new(0.0, h + w * 0.4, -near)),
        color,
    });
    out.push(Line {
        a: n[2],
        b: rigid.transform_point3(Vec3::new(0.0, h + w * 0.4, -near)),
        color,
    });
}

/// A collider's shape in the entity's space (scale included, as the
/// physics adapter sees it), at its `offset`.
fn collider_lines(
    m: &Mat4,
    c: &Collider,
    assets: Option<&Assets>,
    color: Vec4,
    out: &mut Vec<Line>,
) {
    let at = *m * Mat4::from_translation(c.offset);
    match c.shape {
        ColliderShape::Box => box_lines(&at, -c.size, c.size, color, out),
        ColliderShape::Sphere => {
            let center = at.transform_point3(Vec3::ZERO);
            let r = c.size.x * max_scale(m);
            for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
                circle_lines(center, axis, r, color, out);
            }
        }
        ColliderShape::Capsule => {
            let rigid = rigid_of(&at);
            let s = max_scale(m);
            let r = c.size.x * s;
            let h = c.size.y * s;
            let up = rigid.transform_vector3(Vec3::Y);
            let center = at.transform_point3(Vec3::ZERO);
            let top = center + up * h;
            let bottom = center - up * h;
            circle_lines(top, up, r, color, out);
            circle_lines(bottom, up, r, color, out);
            for dir in [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z] {
                let w = rigid.transform_vector3(dir) * r;
                out.push(Line {
                    a: bottom + w,
                    b: top + w,
                    color,
                });
            }
            // The caps: half circles in the two vertical planes.
            for plane in [Vec3::X, Vec3::Z] {
                let axis = rigid.transform_vector3(plane);
                half_circle_lines(top, axis, up, r, color, out);
                half_circle_lines(bottom, axis, -up, r, color, out);
            }
        }
        ColliderShape::Mesh => {
            let mesh = c
                .mesh
                .as_ref()
                .filter(|r| !r.asset.is_nil())
                .and_then(|r| assets.and_then(|a| a.mesh(r).ok()));
            match mesh {
                Some(mesh) => box_lines(
                    &at,
                    Vec3::from(mesh.bounds_min),
                    Vec3::from(mesh.bounds_max),
                    color,
                    out,
                ),
                None => cross_lines(at.transform_point3(Vec3::ZERO), ICON_RADIUS, color, out),
            }
        }
    }
}

/// The half circle around `center` in the plane spanned by `axis` and
/// `pole`, on `pole`'s side.
fn half_circle_lines(
    center: Vec3,
    axis: Vec3,
    pole: Vec3,
    radius: f32,
    color: Vec4,
    out: &mut Vec<Line>,
) {
    let steps = CIRCLE_SEGMENTS / 2;
    let mut prev = center + axis * radius;
    for i in 1..=steps {
        let a = i as f32 / steps as f32 * std::f32::consts::PI;
        let p = center + (axis * a.cos() + pole * a.sin()) * radius;
        out.push(Line {
            a: prev,
            b: p,
            color,
        });
        prev = p;
    }
}

fn max_scale(m: &Mat4) -> f32 {
    m.x_axis
        .truncate()
        .length()
        .max(m.y_axis.truncate().length())
        .max(m.z_axis.truncate().length())
}

/// The asset references a built-in component carries (for the closure).
fn asset_refs_of(c: &ComponentValue) -> Vec<AssetRef> {
    let (_, node) = encode_component(c);
    fynkel_document::asset::asset_refs(&node)
        .into_iter()
        .map(|(_, r)| r)
        .collect()
}

/// `id` as the contracts' UUID (the world's key type).
pub fn contracts_uuid(id: Uuid) -> fynkel_contracts::Uuid {
    from_uuid(id)
}
