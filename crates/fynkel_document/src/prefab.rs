//! Prefab documents, instances and expansion (blueprint §6 Prefabs, §19.2c
//! item 7).
//!
//! A `.prefab.json` is the scene entity model plus declared `roots`
//! ([`Prefab`]). A scene entity becomes an **instance** by carrying
//! `prefab: { asset, instance, overrides[] }` ([`crate::scene::PrefabInstance`]):
//! on expansion the prefab's roots become children of that entity (after
//! its authored children), every prefab-local entity and component id is
//! remapped to an id **derived from the instance id** (UUID v5 of the local
//! id in the instance's namespace — deterministic, so expanding twice gives
//! the same scene), and internal references — `parent`, `children`, and
//! any string inside component `data` that equals a prefab-local entity id
//! — are remapped with them. External references (assets) are shared.
//!
//! Overrides are an explicit ordered op list addressed by prefab-local ids
//! (`setField`, `unsetField`, `addComponent`, `removeComponent`,
//! `addObject`, `removeObject`); `setField` with `null` sets null,
//! `unsetField` removes the field. No array indices, no names, no deep
//! merge.
//!
//! Out of scope until after M2: nested instances (a prefab containing an
//! instance is `D0402`), variants, apply/revert.
//!
//! Codes: `D0400` roots mismatch, `D0401` unknown prefab, `D0402` nested,
//! `D0403` override target missing, `D0404` override not applicable,
//! `D0405` duplicate instance id, `D0406` derived id collision, `D0407`
//! malformed entry.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use fynkel_contracts::{Code, Diagnostic};
use uuid::Uuid;

use crate::json::{Member, Node, Span, Value};
use crate::scene::{read_entities, validate_entities, Component, Entity, OverrideOp, Scene};
use crate::schema::SchemaSet;
use crate::{Document, DocumentKind};

/// Typed read view of a prefab document.
#[derive(Clone, Debug)]
pub struct Prefab {
    pub document: Document,
    /// Document order.
    pub entities: Vec<Entity>,
    /// As declared.
    pub roots: Vec<Uuid>,
    index: HashMap<Uuid, usize>,
    read_diagnostics: Vec<Diagnostic>,
}

impl Prefab {
    pub fn from_document(document: Document) -> Prefab {
        let mut diags = Vec::new();
        if document.kind != DocumentKind::Prefab {
            diags.push(
                Diagnostic::error(
                    Code::UNKNOWN_DOCUMENT_KIND,
                    format!("expected a prefab document, got {}", document.kind.name()),
                )
                .with_pointer(""),
            );
        }
        let (entities, index) = read_entities(&document.root, &mut diags);
        let mut roots = Vec::new();
        match document.root.get("roots").and_then(Node::as_array) {
            Some(items) => {
                for (i, item) in items.iter().enumerate() {
                    match item.as_str().map(crate::parse_uuid) {
                        Some(Ok(u)) => roots.push(u),
                        _ => diags.push(anchor(
                            &document.root,
                            Diagnostic::error(Code::MALFORMED_ID, "root must be a canonical UUID"),
                            &format!("/roots/{i}"),
                        )),
                    }
                }
            }
            None => diags.push(anchor(
                &document.root,
                Diagnostic::error(Code::PREFAB_MALFORMED_ENTRY, "roots is required")
                    .with_suggestion("list the parentless entities in instantiation order"),
                "",
            )),
        }
        for d in &mut diags {
            if d.document.is_none() {
                d.document = document.path.clone();
            }
        }
        Prefab {
            document,
            entities,
            roots,
            index,
            read_diagnostics: diags,
        }
    }

    pub fn parse(path: Option<&str>, text: &str) -> Result<Prefab, Vec<Diagnostic>> {
        Document::parse(DocumentKind::Prefab, path, text).map(Prefab::from_document)
    }

    /// The prefab's asset id (its document id).
    pub fn id(&self) -> Uuid {
        self.document.envelope.id
    }

    pub fn entity(&self, id: Uuid) -> Option<&Entity> {
        self.index.get(&id).map(|i| &self.entities[*i])
    }

    pub fn read_diagnostics(&self) -> &[Diagnostic] {
        &self.read_diagnostics
    }

    /// Envelope + JSON Schema, read problems, hierarchy/component rules,
    /// and the prefab rules (`roots`, no nesting).
    pub fn validate(&self, schemas: &SchemaSet) -> Vec<Diagnostic> {
        let mut out = self.document.validate(schemas);
        let schema_pointers: Vec<String> = out.iter().filter_map(|d| d.pointer.clone()).collect();
        out.extend(self.read_diagnostics.iter().cloned());
        for mut d in self.validate_semantics(schemas) {
            if (d.code == Code::SCENE_COMPONENT_DATA || d.code == Code::PREFAB_NESTED)
                && d.pointer
                    .as_ref()
                    .is_some_and(|p| schema_pointers.contains(p))
            {
                continue;
            }
            if d.document.is_none() {
                d.document = self.document.path.clone();
            }
            out.push(d);
        }
        out
    }

    /// The semantic rules only (no schema pass).
    pub fn validate_semantics(&self, schemas: &SchemaSet) -> Vec<Diagnostic> {
        let root = &self.document.root;
        let mut out = validate_entities(root, &self.entities, &|id| self.entity(id), schemas);
        // roots ⇔ parentless entities, each once.
        let parentless: Vec<Uuid> = self
            .entities
            .iter()
            .filter(|e| e.parent.is_none())
            .map(|e| e.id)
            .collect();
        let mut seen = HashSet::new();
        for (i, r) in self.roots.iter().enumerate() {
            let pointer = format!("/roots/{i}");
            if !seen.insert(*r) {
                out.push(anchor(
                    root,
                    Diagnostic::error(
                        Code::PREFAB_ROOTS_MISMATCH,
                        format!("root {r} is listed twice"),
                    ),
                    &pointer,
                ));
                continue;
            }
            match self.entity(*r) {
                None => out.push(anchor(
                    root,
                    Diagnostic::error(
                        Code::PREFAB_ROOTS_MISMATCH,
                        format!("root {r} is not an entity of this prefab"),
                    )
                    .with_suggestion("remove it from roots"),
                    &pointer,
                )),
                Some(e) if e.parent.is_some() => out.push(anchor(
                    root,
                    Diagnostic::error(
                        Code::PREFAB_ROOTS_MISMATCH,
                        format!("root {r} ({:?}) has a parent", e.name),
                    )
                    .with_id(e.id.to_string())
                    .with_suggestion("roots are the parentless entities only"),
                    &pointer,
                )),
                Some(_) => {}
            }
        }
        for id in parentless.iter().filter(|id| !seen.contains(id)) {
            let e = self.entity(*id).unwrap();
            out.push(anchor(
                root,
                Diagnostic::error(
                    Code::PREFAB_ROOTS_MISMATCH,
                    format!(
                        "parentless entity {id} ({:?}) is missing from roots",
                        e.name
                    ),
                )
                .with_id(id.to_string())
                .with_suggestion(format!("append {id} to roots")),
                "/roots",
            ));
        }
        for e in self.entities.iter().filter(|e| e.prefab.is_some()) {
            out.push(anchor(
                root,
                Diagnostic::error(
                    Code::PREFAB_NESTED,
                    "a prefab cannot contain a prefab instance (nested prefabs are not supported yet)",
                )
                .with_id(e.id.to_string())
                .with_suggestion("expand the instance into plain entities"),
                &format!("{}/prefab", e.pointer),
            ));
        }
        for d in &mut out {
            if d.document.is_none() {
                d.document = self.document.path.clone();
            }
        }
        out
    }
}

/// The prefabs a scene may instantiate, keyed by document id.
#[derive(Clone, Debug, Default)]
pub struct PrefabLibrary {
    prefabs: HashMap<Uuid, Prefab>,
}

impl PrefabLibrary {
    pub fn empty() -> PrefabLibrary {
        PrefabLibrary::default()
    }

    /// Adds (or replaces, by id) a prefab.
    pub fn add(&mut self, prefab: Prefab) {
        self.prefabs.insert(prefab.id(), prefab);
    }

    pub fn get(&self, id: Uuid) -> Option<&Prefab> {
        self.prefabs.get(&id)
    }

    pub fn len(&self) -> usize {
        self.prefabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.prefabs.is_empty()
    }

    /// Prefabs sorted by id.
    pub fn prefabs(&self) -> Vec<&Prefab> {
        let mut v: Vec<&Prefab> = self.prefabs.values().collect();
        v.sort_by_key(|p| p.id());
        v
    }

    /// Every `*.prefab.json` under `dir` (recursively), in path order.
    /// Documents that fail to parse are reported and skipped; the library
    /// still holds the others. Prefabs with the same id: the later path wins
    /// and is reported.
    pub fn scan(dir: &Path) -> (PrefabLibrary, Vec<Diagnostic>) {
        let mut lib = PrefabLibrary::empty();
        let mut diags = Vec::new();
        let mut files = Vec::new();
        if let Err(e) = collect(dir, &mut files) {
            diags.push(
                Diagnostic::error(Code::IO, format!("cannot scan for prefabs: {e}"))
                    .with_document(dir.display().to_string()),
            );
            return (lib, diags);
        }
        files.sort();
        for path in files {
            match Document::read(&path, Some(DocumentKind::Prefab)) {
                Ok(doc) => {
                    let prefab = Prefab::from_document(doc);
                    if let Some(existing) = lib.get(prefab.id()) {
                        diags.push(
                            Diagnostic::warning(
                                Code::MALFORMED_ID,
                                format!(
                                    "prefab id {} is also used by {}; this file wins",
                                    prefab.id(),
                                    existing.document.path.as_deref().unwrap_or("?")
                                ),
                            )
                            .with_document(path.display().to_string())
                            .with_pointer("/id"),
                        );
                    }
                    lib.add(prefab);
                }
                Err(d) => diags.extend(d),
            }
        }
        (lib, diags)
    }
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(&path, out)?;
        } else if path.to_string_lossy().ends_with(".prefab.json") {
            out.push(path);
        }
    }
    Ok(())
}

/// Prefab-local id → scene id for one instance: UUID v5 in the instance's
/// namespace. Deterministic; the same instance always yields the same ids.
pub fn derive_id(instance: Uuid, local: Uuid) -> Uuid {
    Uuid::new_v5(&instance, local.as_bytes())
}

/// One expanded instance: where it came from and how its ids were mapped.
#[derive(Clone, Debug, PartialEq)]
pub struct ExpandedInstance {
    /// The scene entity carrying `prefab`.
    pub anchor: Uuid,
    pub instance: Uuid,
    pub prefab: Uuid,
    /// (prefab-local entity id, scene id), instantiation order.
    pub entities: Vec<(Uuid, Uuid)>,
    /// (prefab-local component id, scene id).
    pub components: Vec<(Uuid, Uuid)>,
}

impl ExpandedInstance {
    pub fn entity(&self, local: Uuid) -> Option<Uuid> {
        self.entities
            .iter()
            .find(|(l, _)| *l == local)
            .map(|(_, s)| *s)
    }
}

/// A scene with every instance replaced by plain entities.
#[derive(Clone, Debug)]
pub struct Expanded {
    /// A scene document (same path and envelope) with no `prefab` keys;
    /// expanded entities follow their anchor. Synthetic nodes carry no
    /// source positions.
    pub document: Document,
    pub instances: Vec<ExpandedInstance>,
    /// Anchored in the *source* scene (override pointers).
    pub diagnostics: Vec<Diagnostic>,
}

impl Expanded {
    pub fn has_errors(&self) -> bool {
        fynkel_contracts::has_errors(&self.diagnostics)
    }

    pub fn scene(&self) -> Scene {
        Scene::from_document(self.document.clone())
    }
}

/// Expands every instance of `scene`. Instances that cannot be expanded
/// (unknown prefab, nested prefab, invalid overrides) are reported and left
/// as their anchor entity without `prefab`; the rest expand. Component
/// `data` touched by overrides is validated against the component schema
/// (`D0404`, at the last op that touched it).
pub fn expand(scene: &Scene, library: &PrefabLibrary, schemas: &SchemaSet) -> Expanded {
    let src = &scene.document.root;
    let mut diagnostics = Vec::new();
    let mut root = src.clone();
    let mut instances = Vec::new();

    // Ids already in the scene (entities and components), for collisions.
    let mut taken: HashSet<Uuid> = HashSet::new();
    for e in &scene.entities {
        taken.insert(e.id);
        for c in &e.components {
            taken.insert(c.id);
        }
    }
    let mut seen_instances: HashSet<Uuid> = HashSet::new();

    for e in scene.instances() {
        let inst = e.prefab.as_ref().unwrap();
        // The expanded document has no `prefab` keys either way.
        if let Some(node) = root
            .get_mut("entities")
            .and_then(|n| n.get_mut(&e.id.to_string()))
        {
            if let Some(members) = node.as_object_mut() {
                members.retain(|m| m.key != "prefab");
            }
        }
        if !seen_instances.insert(inst.instance) {
            // Reported by Scene::validate (D0405); do not expand twice.
            continue;
        }
        let Some(prefab) = library.get(inst.prefab_id()) else {
            diagnostics.push(anchor(
                src,
                Diagnostic::error(
                    Code::PREFAB_UNKNOWN,
                    format!("prefab {} is not available", inst.prefab_id()),
                )
                .with_id(e.id.to_string())
                .with_suggestion(if library.is_empty() {
                    "pass the prefab directory (--prefabs <dir>)".to_string()
                } else {
                    format!("{} prefab(s) loaded; check the asset id", library.len())
                }),
                &format!("{}/asset", inst.pointer),
            ));
            continue;
        };
        if prefab.entities.iter().any(|p| p.prefab.is_some()) {
            diagnostics.push(anchor(
                src,
                Diagnostic::error(
                    Code::PREFAB_NESTED,
                    format!(
                        "prefab {} contains a prefab instance; nested prefabs are not supported yet",
                        prefab.id()
                    ),
                )
                .with_id(e.id.to_string()),
                &format!("{}/asset", inst.pointer),
            ));
            continue;
        }

        // Working copy of the prefab, then the overrides in order.
        let mut work = Working::new(prefab);
        let mut touched: HashMap<(Uuid, Uuid), String> = HashMap::new();
        let mut ok = true;
        for o in &inst.overrides {
            let fail = |code: Code, msg: String| {
                anchor(
                    src,
                    Diagnostic::error(code, msg).with_id(e.id.to_string()),
                    &o.pointer,
                )
            };
            let entity_id = o.op.entity();
            let exists = work.entity(entity_id).is_some();
            if !exists && !matches!(o.op, OverrideOp::AddObject { .. }) {
                diagnostics.push(fail(
                    Code::PREFAB_OVERRIDE_TARGET,
                    format!("{}: entity {entity_id} is not in the prefab", o.op.name()),
                ));
                ok = false;
                continue;
            }
            let result: Result<(), Diagnostic> = match &o.op {
                OverrideOp::AddObject {
                    entity,
                    name,
                    parent,
                    components,
                } => {
                    let mut seen_types = HashSet::new();
                    if exists {
                        Err(fail(
                            Code::PREFAB_OVERRIDE_INVALID,
                            format!("addObject: entity {entity} already exists"),
                        ))
                    } else if let Some(p) = parent.filter(|p| work.entity(*p).is_none()) {
                        Err(fail(
                            Code::PREFAB_OVERRIDE_TARGET,
                            format!("addObject: parent {p} is not in the prefab"),
                        ))
                    } else if let Some(dup) = components.iter().find(|c| work.has_component(c.id)) {
                        Err(fail(
                            Code::PREFAB_OVERRIDE_INVALID,
                            format!("addObject: component id {} already exists", dup.id),
                        ))
                    } else if let Some(dup) =
                        components.iter().find(|c| !seen_types.insert(&c.type_id))
                    {
                        Err(fail(
                            Code::PREFAB_OVERRIDE_INVALID,
                            format!("addObject: two {} components", dup.type_id),
                        ))
                    } else {
                        for c in components {
                            touched.insert((*entity, c.id), o.pointer.clone());
                        }
                        work.add_object(*entity, name, *parent, components.clone(), &o.pointer);
                        Ok(())
                    }
                }
                OverrideOp::SetField {
                    component,
                    field,
                    value,
                    ..
                } => {
                    let target = work.entity_mut(entity_id).unwrap();
                    match target.components.iter_mut().find(|c| c.id == *component) {
                        Some(c) => {
                            c.data.set(field, value.clone());
                            touched.insert((entity_id, c.id), o.pointer.clone());
                            Ok(())
                        }
                        None => Err(fail(
                            Code::PREFAB_OVERRIDE_TARGET,
                            format!("setField: component {component} is not on entity {entity_id}"),
                        )),
                    }
                }
                OverrideOp::UnsetField {
                    component, field, ..
                } => {
                    let target = work.entity_mut(entity_id).unwrap();
                    match target.components.iter_mut().find(|c| c.id == *component) {
                        Some(c) => {
                            let had = c
                                .data
                                .as_object_mut()
                                .map(|m| {
                                    let before = m.len();
                                    m.retain(|x| x.key != *field);
                                    before != m.len()
                                })
                                .unwrap_or(false);
                            if had {
                                touched.insert((entity_id, c.id), o.pointer.clone());
                                Ok(())
                            } else {
                                Err(fail(
                                    Code::PREFAB_OVERRIDE_TARGET,
                                    format!(
                                        "unsetField: {field:?} is not set on component {component}"
                                    ),
                                ))
                            }
                        }
                        None => Err(fail(
                            Code::PREFAB_OVERRIDE_TARGET,
                            format!(
                                "unsetField: component {component} is not on entity {entity_id}"
                            ),
                        )),
                    }
                }
                OverrideOp::AddComponent { component, .. } => {
                    if work.has_component(component.id) {
                        Err(fail(
                            Code::PREFAB_OVERRIDE_INVALID,
                            format!("addComponent: component id {} already exists", component.id),
                        ))
                    } else {
                        let target = work.entity_mut(entity_id).unwrap();
                        match target
                            .components
                            .iter()
                            .find(|c| c.type_id == component.type_id)
                        {
                            Some(existing) => Err(fail(
                                Code::PREFAB_OVERRIDE_INVALID,
                                format!(
                                    "addComponent: entity {entity_id} already has a {} ({})",
                                    component.type_id, existing.id
                                ),
                            )),
                            None => {
                                let mut c = component.clone();
                                c.pointer = o.pointer.clone();
                                touched.insert((entity_id, c.id), o.pointer.clone());
                                target.components.push(c);
                                Ok(())
                            }
                        }
                    }
                }
                OverrideOp::RemoveComponent { component, .. } => {
                    let target = work.entity_mut(entity_id).unwrap();
                    let before = target.components.len();
                    target.components.retain(|c| c.id != *component);
                    if before == target.components.len() {
                        Err(fail(
                            Code::PREFAB_OVERRIDE_TARGET,
                            format!("removeComponent: component {component} is not on entity {entity_id}"),
                        ))
                    } else {
                        touched.remove(&(entity_id, *component));
                        Ok(())
                    }
                }
                OverrideOp::RemoveObject { entity } => {
                    let removed = work.remove_subtree(*entity);
                    touched.retain(|(en, _), _| !removed.contains(en));
                    Ok(())
                }
            };
            if let Err(d) = result {
                diagnostics.push(d);
                ok = false;
            }
        }
        // Touched component data must still fit its schema (in op order).
        let mut touched: Vec<((Uuid, Uuid), String)> = touched.into_iter().collect();
        touched.sort_by(|a, b| a.1.cmp(&b.1));
        for ((entity, component), pointer) in &touched {
            let Some(c) = work
                .entity(*entity)
                .and_then(|e| e.components.iter().find(|c| c.id == *component))
            else {
                continue;
            };
            for d in schemas.validate_component(&c.type_id, &c.data) {
                diagnostics.push(anchor(
                    src,
                    Diagnostic::error(
                        Code::PREFAB_OVERRIDE_INVALID,
                        format!(
                            "{} data{}: {}",
                            c.type_id,
                            d.pointer.as_deref().unwrap_or(""),
                            d.message
                        ),
                    )
                    .with_id(e.id.to_string()),
                    pointer,
                ));
                ok = false;
            }
        }
        if !ok {
            continue;
        }

        // Remap and emit.
        let local_ids: HashSet<Uuid> = work.entities.iter().map(|w| w.id).collect();
        let map_entity = |id: Uuid| derive_id(inst.instance, id);
        let mut expanded = ExpandedInstance {
            anchor: e.id,
            instance: inst.instance,
            prefab: prefab.id(),
            entities: Vec::new(),
            components: Vec::new(),
        };
        let mut collision = None;
        for w in &work.entities {
            let sid = map_entity(w.id);
            if !taken.insert(sid) {
                collision = Some(sid);
            }
            expanded.entities.push((w.id, sid));
            for c in &w.components {
                let cid = derive_id(inst.instance, c.id);
                if !taken.insert(cid) {
                    collision = Some(cid);
                }
                expanded.components.push((c.id, cid));
            }
        }
        if let Some(id) = collision {
            diagnostics.push(anchor(
                src,
                Diagnostic::error(
                    Code::PREFAB_ID_COLLISION,
                    format!("derived id {id} collides with an existing id"),
                )
                .with_id(e.id.to_string())
                .with_suggestion("change the instance id"),
                &format!("{}/instance", inst.pointer),
            ));
            continue;
        }
        let mut new_members: Vec<Member> = Vec::with_capacity(work.entities.len());
        for w in &work.entities {
            let parent = match w.parent {
                Some(p) => Node::string(map_entity(p).to_string()),
                None => Node::string(e.id.to_string()),
            };
            let children: Vec<Node> = w
                .children
                .iter()
                .map(|c| Node::string(map_entity(*c).to_string()))
                .collect();
            let components: Vec<(String, Node)> = w
                .components
                .iter()
                .map(|c| {
                    let mut data = c.data.clone();
                    remap_strings(&mut data, &local_ids, &map_entity);
                    (
                        derive_id(inst.instance, c.id).to_string(),
                        Node::object(vec![
                            ("type".into(), Node::string(c.type_id.clone())),
                            ("version".into(), Node::integer(c.version as i64)),
                            ("data".into(), data),
                        ]),
                    )
                })
                .collect();
            new_members.push(Member {
                key: map_entity(w.id).to_string(),
                key_span: Span::default(),
                value: Node::object(vec![
                    ("name".into(), Node::string(w.name.clone())),
                    ("parent".into(), parent),
                    ("children".into(), Node::synthetic(Value::Array(children))),
                    ("components".into(), Node::object(components)),
                ]),
            });
        }
        let entities = root
            .get_mut("entities")
            .and_then(Node::as_object_mut)
            .expect("scene has an entities object");
        let anchor_index = entities
            .iter()
            .position(|m| m.key == e.id.to_string())
            .expect("anchor entity exists");
        // Instance roots become the anchor's trailing children.
        if let Some(children) = entities[anchor_index].value.get_mut("children") {
            if let Value::Array(items) = &mut children.value {
                for r in &work.roots {
                    items.push(Node::string(map_entity(*r).to_string()));
                }
            }
        }
        let tail = entities.split_off(anchor_index + 1);
        entities.extend(new_members);
        entities.extend(tail);
        instances.push(expanded);
    }

    for d in &mut diagnostics {
        if d.document.is_none() {
            d.document = scene.document.path.clone();
        }
    }
    Expanded {
        document: Document {
            kind: DocumentKind::Scene,
            path: scene.document.path.clone(),
            root,
            envelope: scene.document.envelope.clone(),
        },
        instances,
        diagnostics,
    }
}

/// A prefab's entities being edited by overrides.
struct Working {
    entities: Vec<Entity>,
    roots: Vec<Uuid>,
}

impl Working {
    fn new(prefab: &Prefab) -> Working {
        Working {
            entities: prefab.entities.clone(),
            roots: prefab.roots.clone(),
        }
    }

    fn entity(&self, id: Uuid) -> Option<&Entity> {
        self.entities.iter().find(|e| e.id == id)
    }

    fn entity_mut(&mut self, id: Uuid) -> Option<&mut Entity> {
        self.entities.iter_mut().find(|e| e.id == id)
    }

    fn has_component(&self, id: Uuid) -> bool {
        self.entities
            .iter()
            .any(|e| e.components.iter().any(|c| c.id == id))
    }

    fn add_object(
        &mut self,
        id: Uuid,
        name: &str,
        parent: Option<Uuid>,
        components: Vec<Component>,
        pointer: &str,
    ) {
        match parent {
            Some(p) => self.entity_mut(p).unwrap().children.push(id),
            None => self.roots.push(id),
        }
        self.entities.push(Entity {
            id,
            name: name.to_string(),
            parent,
            children: Vec::new(),
            components,
            prefab: None,
            pointer: pointer.to_string(),
        });
    }

    /// Removes `id` and everything under it; returns the removed ids.
    fn remove_subtree(&mut self, id: Uuid) -> HashSet<Uuid> {
        let mut removed = HashSet::new();
        let mut stack = vec![id];
        while let Some(cur) = stack.pop() {
            if !removed.insert(cur) {
                continue;
            }
            if let Some(e) = self.entity(cur) {
                stack.extend(e.children.iter().copied());
            }
        }
        self.entities.retain(|e| !removed.contains(&e.id));
        for e in &mut self.entities {
            e.children.retain(|c| !removed.contains(c));
        }
        self.roots.retain(|r| !removed.contains(r));
        removed
    }
}

/// Replaces every string equal to a prefab-local entity id (references
/// inside component data) with the derived id.
fn remap_strings(node: &mut Node, local_ids: &HashSet<Uuid>, map: &dyn Fn(Uuid) -> Uuid) {
    match &mut node.value {
        Value::String(s) => {
            if let Ok(u) = crate::parse_uuid(s) {
                if local_ids.contains(&u) {
                    *s = map(u).to_string();
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                remap_strings(item, local_ids, map);
            }
        }
        Value::Object(members) => {
            for m in members {
                remap_strings(&mut m.value, local_ids, map);
            }
        }
        _ => {}
    }
}

fn anchor(root: &Node, d: Diagnostic, pointer: &str) -> Diagnostic {
    let d = d.with_pointer(pointer);
    match root.pointer_span(pointer) {
        Some(s) => d.at(s.start.line, s.start.column),
        None => d,
    }
}

/// `inspect --json` payload for a prefab.
pub fn inspect_json(prefab: &Prefab) -> serde_json::Value {
    serde_json::json!({
        "kind": "prefab",
        "id": prefab.id().to_string(),
        "name": prefab.document.envelope.name,
        "formatVersion": prefab.document.envelope.format_version,
        "entityCount": prefab.entities.len(),
        "roots": prefab.roots.iter().map(|r| r.to_string()).collect::<Vec<_>>(),
        "entities": prefab.entities.iter().map(|e| serde_json::json!({
            "id": e.id.to_string(),
            "name": e.name,
            "parent": e.parent.map(|p| p.to_string()),
            "children": e.children.iter().map(|c| c.to_string()).collect::<Vec<_>>(),
            "components": e.components.iter().map(|c| serde_json::json!({
                "id": c.id.to_string(), "type": c.type_id, "version": c.version,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

/// Human `inspect` output: roots first, indented by depth.
pub fn inspect_text(prefab: &Prefab) -> String {
    fn walk(prefab: &Prefab, e: &Entity, depth: usize, out: &mut String) {
        let types: Vec<&str> = e.components.iter().map(|c| c.type_id.as_str()).collect();
        out.push_str(&format!(
            "{}{} ({}){}\n",
            "  ".repeat(depth),
            if e.name.is_empty() {
                "<unnamed>"
            } else {
                &e.name
            },
            e.id,
            if types.is_empty() {
                String::new()
            } else {
                format!("  [{}]", types.join(", "))
            }
        ));
        if depth < 512 {
            for c in e.children.iter().filter_map(|c| prefab.entity(*c)) {
                if c.parent == Some(e.id) {
                    walk(prefab, c, depth + 1, out);
                }
            }
        }
    }
    let mut out = format!(
        "prefab {:?} ({}) formatVersion {}: {} entities, {} roots\n",
        prefab.document.envelope.name,
        prefab.id(),
        prefab.document.envelope.format_version,
        prefab.entities.len(),
        prefab.roots.len()
    );
    for r in prefab.roots.iter().filter_map(|r| prefab.entity(*r)) {
        walk(prefab, r, 0, &mut out);
    }
    out
}
