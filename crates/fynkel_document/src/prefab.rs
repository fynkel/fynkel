//! Prefab documents, instances and expansion (blueprint §6 Prefabs, §19.2c
//! item 7).
//!
//! A `.prefab.json` is the scene entity model plus its one `root`
//! ([`Prefab`], ADR-0022): the root is the only parentless entity and
//! every other entity descends from it. A scene entity becomes an
//! **instance** by carrying `prefab: { asset, instance, overrides[] }`
//! ([`crate::scene::PrefabInstance`]), and on expansion that entity **is**
//! the prefab's root: it keeps its own id, `name` and `active`, takes the
//! root's components (it has none of its own, `D0408`) and the root's
//! children, followed by the children the scene added under it. Every
//! other prefab-local entity id, and every component id, is remapped to
//! an id **derived from the instance id** (UUID v5 of the local id in the
//! instance's namespace — deterministic, so expanding twice gives the same
//! scene), and internal references — `parent`, `children`, and any string
//! inside component `data` that equals a prefab-local entity id — are
//! remapped with them (the root's to the instance's id). External
//! references (assets) are shared.
//!
//! Overrides are an explicit ordered op list addressed by prefab-local ids
//! (`setField`, `unsetField`, `addComponent`, `removeComponent`,
//! `addObject`, `removeObject`); `setField` with `null` sets null,
//! `unsetField` removes the field. No array indices, no names, no deep
//! merge.
//!
//! A prefab may itself contain instances (nested prefabs, M3-08b):
//! [`PrefabLibrary::flattened`] expands every prefab's instances against
//! the library first, innermost first, so a scene's expansion sees plain
//! entities only. Ids nest the same way — an inner entity's scene id is
//! `derive_id(outer_instance, derive_id(inner_instance, local))` — and an
//! outer override addresses an inner entity by that derived (inner) id. A
//! prefab that instantiates itself, directly or through others, is
//! `D0402` and is left unexpanded.
//!
//! Out of scope: variants, apply/revert.
//!
//! Codes: `D0400` the root is not the one parentless entity, `D0401`
//! unknown prefab, `D0402` instantiation cycle, `D0403` override target
//! missing, `D0404` override not applicable, `D0405` duplicate instance
//! id, `D0406` derived id collision, `D0407` malformed entry, `D0408` an
//! instance with components of its own.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use fynkel_contracts::{AssetRef, Code, Diagnostic};
use uuid::Uuid;

use crate::json::{Locator, Member, Node, Span, Value};
use crate::scene::{
    read_entities, validate_entities, Component, Entity, OverrideOp, PrefabInstance, Scene,
};
use crate::schema::SchemaSet;
use crate::{Document, DocumentKind};

/// Typed read view of a prefab document.
#[derive(Clone, Debug)]
pub struct Prefab {
    pub document: Document,
    /// Document order.
    pub entities: Vec<Entity>,
    /// As declared (nil when missing or malformed; reported).
    pub root: Uuid,
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
        let root = match document.root.get("root") {
            Some(node) => match node.as_str().map(crate::parse_uuid) {
                Some(Ok(u)) => u,
                _ => {
                    diags.push(anchor(
                        &document.root,
                        Diagnostic::error(Code::MALFORMED_ID, "root must be a canonical UUID"),
                        "/root",
                    ));
                    Uuid::nil()
                }
            },
            None => {
                diags.push(anchor(
                    &document.root,
                    Diagnostic::error(Code::PREFAB_MALFORMED_ENTRY, "root is required")
                        .with_suggestion("name the prefab's one parentless entity"),
                    "",
                ));
                Uuid::nil()
            }
        };
        for d in &mut diags {
            if d.document.is_none() {
                d.document = document.path.clone();
            }
        }
        Prefab {
            document,
            entities,
            root,
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

    /// Whether an entity of this prefab is itself a prefab instance.
    pub fn has_instances(&self) -> bool {
        self.entities.iter().any(|e| e.prefab.is_some())
    }

    /// The prefab read as a scene (same entities, `root` ignored), for
    /// expansion and description with the scene machinery.
    pub fn scene(&self) -> Scene {
        Scene::from_document(Document {
            kind: DocumentKind::Scene,
            path: self.document.path.clone(),
            root: self.document.root.clone(),
            envelope: self.document.envelope.clone(),
        })
    }

    /// Envelope + JSON Schema, read problems, hierarchy/component rules,
    /// and the prefab rules (`root`). Its own instances are checked by
    /// expansion (`PrefabLibrary::flattened`), as a scene's are.
    pub fn validate(&self, schemas: &SchemaSet) -> Vec<Diagnostic> {
        let mut out = self.document.validate(schemas);
        let schema_pointers: HashSet<String> =
            out.iter().filter_map(|d| d.pointer.clone()).collect();
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
        let mut out = validate_entities(
            &Locator::new(root),
            &self.entities,
            &|id| self.entity(id),
            schemas,
        );
        // One root: an entity of the prefab, without a parent, and the only
        // parentless entity (`/root` never crosses the entity map, so the
        // plain anchor is fine here).
        let mismatch = |msg: String| Diagnostic::error(Code::PREFAB_ROOT, msg);
        match self.entity(self.root) {
            _ if self.root.is_nil() => {} // reported when read
            None => out.push(anchor(
                root,
                mismatch(format!(
                    "root {} is not an entity of this prefab",
                    self.root
                ))
                .with_suggestion("name the prefab's one parentless entity"),
                "/root",
            )),
            Some(e) if e.parent.is_some() => out.push(anchor(
                root,
                mismatch(format!("root {} ({:?}) has a parent", e.id, e.name))
                    .with_id(e.id.to_string())
                    .with_suggestion("the root is the prefab's one parentless entity"),
                "/root",
            )),
            Some(_) => {}
        }
        for e in self
            .entities
            .iter()
            .filter(|e| e.parent.is_none() && e.id != self.root)
        {
            out.push(anchor(
                root,
                mismatch(format!(
                    "{} ({:?}) has no parent but is not the root: a prefab has one root",
                    e.id, e.name
                ))
                .with_id(e.id.to_string())
                .with_suggestion(format!("make it a child of the root {}", self.root)),
                &format!("{}/parent", e.pointer),
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

/// The prefabs a scene may instantiate, keyed by document id. An
/// imported source's hierarchy (`hero.glb` → its `prefab` sub-asset,
/// M3-19a) is a prefab too: added with its source and sub so a reference
/// `{ asset: <source>, sub: "prefab" }` resolves to it.
#[derive(Clone, Debug, Default)]
pub struct PrefabLibrary {
    prefabs: HashMap<Uuid, Prefab>,
    /// `(source asset, sub)` → the imported prefab's own id (the sidecar's
    /// sub-asset id, which is also the document's `id`).
    imported: HashMap<(Uuid, String), Uuid>,
    /// Prefabs whose own instances could not be expanded (reported by the
    /// `flattened` call that found out); kept as they are, not retried.
    unresolved: HashSet<Uuid>,
}

impl PrefabLibrary {
    pub fn empty() -> PrefabLibrary {
        PrefabLibrary::default()
    }

    /// Adds (or replaces, by id) a prefab.
    pub fn add(&mut self, prefab: Prefab) {
        self.prefabs.insert(prefab.id(), prefab);
    }

    /// Adds an imported source's hierarchy: the prefab document the
    /// importer derived (its `id` is the `sub` sub-asset's id), reachable
    /// by its own id and by `{ asset: source, sub }`.
    pub fn add_imported(&mut self, prefab: Prefab, source: Uuid, sub: &str) {
        self.imported.insert((source, sub.to_string()), prefab.id());
        self.add(prefab);
    }

    pub fn get(&self, id: Uuid) -> Option<&Prefab> {
        self.prefabs.get(&id)
    }

    /// The prefab id a scene's `prefab.asset` names: the asset itself
    /// without a `sub`, the imported prefab registered under
    /// `(asset, sub)` with one (`None` when no such import was added).
    pub fn resolve(&self, r: &AssetRef) -> Option<Uuid> {
        let asset = Uuid::from_bytes(r.asset.0);
        if r.sub.is_empty() {
            Some(asset)
        } else {
            self.imported.get(&(asset, r.sub.clone())).copied()
        }
    }

    /// [`resolve`](Self::resolve) then [`get`](Self::get).
    pub fn get_ref(&self, r: &AssetRef) -> Option<&Prefab> {
        self.resolve(r).and_then(|id| self.get(id))
    }

    pub fn len(&self) -> usize {
        self.prefabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.prefabs.is_empty()
    }

    /// Whether any prefab still carries an instance.
    pub fn has_instances(&self) -> bool {
        self.prefabs.values().any(Prefab::has_instances)
    }

    /// The same library with every prefab's own instances expanded into
    /// plain entities (nested prefabs), innermost first; a prefab whose
    /// instances cannot be expanded (unknown prefab, a cycle — `D0402` —,
    /// invalid overrides) is reported and left as it is, so a scene
    /// instantiating it is refused for the instance it still carries. The
    /// diagnostics are anchored in the prefab documents.
    pub fn flattened(&self, schemas: &SchemaSet) -> (PrefabLibrary, Vec<Diagnostic>) {
        let mut out = PrefabLibrary::empty();
        out.imported = self.imported.clone();
        let mut diagnostics = Vec::new();
        let mut ids: Vec<Uuid> = self.prefabs.keys().copied().collect();
        ids.sort();
        for id in ids {
            let mut visiting = Vec::new();
            self.flatten_one(id, schemas, &mut visiting, &mut out, &mut diagnostics);
        }
        (out, diagnostics)
    }

    fn flatten_one(
        &self,
        id: Uuid,
        schemas: &SchemaSet,
        visiting: &mut Vec<Uuid>,
        out: &mut PrefabLibrary,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        if out.get(id).is_some() || visiting.contains(&id) {
            return;
        }
        let Some(prefab) = self.get(id) else {
            return;
        };
        if !prefab.has_instances() || self.unresolved.contains(&id) {
            out.add(prefab.clone());
            if self.unresolved.contains(&id) {
                out.unresolved.insert(id);
            }
            return;
        }
        visiting.push(id);
        // The prefabs this one instantiates, flattened first; one that is
        // being flattened further up the stack is a cycle.
        let mut cycle = None;
        for e in prefab.entities.iter().filter(|e| e.prefab.is_some()) {
            let inner = e.prefab.as_ref().unwrap();
            let Some(target) = self.resolve(&inner.asset) else {
                continue; // unknown: expand reports D0401
            };
            if visiting.contains(&target) {
                cycle = Some((e, target));
                break;
            }
            self.flatten_one(target, schemas, visiting, out, diagnostics);
        }
        visiting.pop();
        if let Some((e, target)) = cycle {
            let inst = e.prefab.as_ref().unwrap();
            let mut d = anchor(
                &prefab.document.root,
                Diagnostic::error(
                    Code::PREFAB_NESTED,
                    format!("prefab {target} instantiates itself through this instance (a cycle)"),
                )
                .with_id(e.id.to_string())
                .with_suggestion(
                    "a prefab cannot contain itself; expand the instance into plain entities",
                ),
                &format!("{}/asset", inst.pointer),
            );
            d.document = prefab.document.path.clone();
            diagnostics.push(d);
            out.add(prefab.clone());
            out.unresolved.insert(id);
            return;
        }
        let expanded = expand_in(&prefab.scene(), out, schemas);
        if expanded.has_errors() {
            diagnostics.extend(expanded.diagnostics);
            out.add(prefab.clone());
            out.unresolved.insert(id);
            return;
        }
        diagnostics.extend(expanded.diagnostics);
        out.add(Prefab::from_document(Document {
            kind: DocumentKind::Prefab,
            path: prefab.document.path.clone(),
            root: expanded.document.root,
            envelope: prefab.document.envelope.clone(),
        }));
    }

    /// The prefabs reachable from `ids` through their instances (the ids
    /// themselves included, when the library has them).
    pub fn reachable_from(&self, ids: impl IntoIterator<Item = Uuid>) -> HashSet<Uuid> {
        let mut seen = HashSet::new();
        let mut stack: Vec<Uuid> = ids.into_iter().collect();
        while let Some(id) = stack.pop() {
            let Some(prefab) = self.get(id) else {
                continue;
            };
            if !seen.insert(id) {
                continue;
            }
            for e in prefab.entities.iter() {
                if let Some(id) = e.prefab.as_ref().and_then(|inst| self.resolve(&inst.asset)) {
                    stack.push(id);
                }
            }
        }
        seen
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

impl PrefabLibrary {
    /// Adds the imported hierarchies `wanted` names — `(source id, sub)`
    /// pairs from instances with a `sub` — through `sources` (M3-19a):
    /// a `wanted` entry that is not a source of `db`, or whose sidecar
    /// lacks the sub-asset, is skipped (expansion reports it as `D0401`
    /// with the hint); a source that does not import is reported here,
    /// once. A rig document's `prefab` (M3-19b) is its skeleton source's
    /// hierarchy adopted under the rig's id ([`crate::rig::Rig::adopt_prefab`]);
    /// a rig whose skeleton is not an imported model is skipped here and
    /// reported by the rig's own validation. Returns the provider's
    /// diagnostics.
    pub fn add_sources(
        &mut self,
        db: &crate::asset::AssetDatabase,
        wanted: &[(Uuid, String)],
        sources: &dyn crate::project::SourcePrefabs,
    ) -> Vec<Diagnostic> {
        use crate::asset::AssetKind;
        let mut diags = Vec::new();
        for (source, sub) in wanted {
            if self.imported.contains_key(&(*source, sub.clone())) {
                continue;
            }
            let Some(entry) = db.get(*source) else {
                continue;
            };
            if !entry.sub_assets.contains_key(sub) {
                continue;
            }
            match &entry.kind {
                AssetKind::Source { .. } => match sources.prefab(db.root(), entry, sub) {
                    Ok(Some(doc)) => self.add_imported(Prefab::from_document(doc), *source, sub),
                    Ok(None) => {}
                    Err(d) => diags.extend(d),
                },
                AssetKind::Document(DocumentKind::Rig) if sub == crate::rig::RIG_PREFAB_SUB => {
                    let rig = match Document::read(&entry.file(db.root()), Some(DocumentKind::Rig))
                    {
                        Ok(doc) => crate::rig::Rig::from_document(doc),
                        Err(d) => {
                            diags.extend(d);
                            continue;
                        }
                    };
                    let Some(skeleton) = rig
                        .skeleton
                        .as_ref()
                        .filter(|s| s.sub.is_empty())
                        .and_then(|s| db.get(Uuid::from_bytes(s.asset.0)))
                    else {
                        continue;
                    };
                    if !crate::rig::is_model(&skeleton.kind)
                        || !skeleton.sub_assets.contains_key(sub)
                    {
                        continue;
                    }
                    let shown = format!("{}#{sub}", entry.path);
                    let adopted =
                        sources
                            .prefab(db.root(), skeleton, sub)
                            .and_then(|doc| match doc {
                                Some(doc) => doc
                                    .write()
                                    .map_err(|d| vec![d])
                                    .and_then(|text| rig.adopt_prefab(&text, &shown))
                                    .map(Some),
                                None => Ok(None),
                            });
                    match adopted {
                        Ok(Some(doc)) => {
                            self.add_imported(Prefab::from_document(doc), *source, sub)
                        }
                        Ok(None) => {}
                        Err(d) => diags.extend(d),
                    }
                }
                AssetKind::Document(_) => {}
            }
        }
        diags
    }

    /// The `(source id, sub)` pairs the instances of `scene` name — the
    /// input to [`PrefabLibrary::add_sources`].
    pub fn wanted_sources(scene: &Scene) -> Vec<(Uuid, String)> {
        let mut wanted = Vec::new();
        for e in scene.instances() {
            let inst = e.prefab.as_ref().unwrap();
            if !inst.asset.sub.is_empty() {
                let key = (inst.prefab_id(), inst.asset.sub.clone());
                if !wanted.contains(&key) {
                    wanted.push(key);
                }
            }
        }
        wanted
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

/// The default Transform's `data`: at the origin, unrotated, unit scale.
pub fn identity_transform() -> Node {
    let v =
        |xs: &[f64]| Node::synthetic(Value::Array(xs.iter().map(|x| Node::float(*x)).collect()));
    Node::object(vec![
        ("position".into(), v(&[0.0, 0.0, 0.0])),
        ("rotation".into(), v(&[0.0, 0.0, 0.0, 1.0])),
        ("scale".into(), v(&[1.0, 1.0, 1.0])),
    ])
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
/// (unknown prefab, a prefab whose own instances did not expand, invalid
/// overrides) are reported and left as their anchor entity without
/// `prefab`; the rest expand. Component `data` touched by overrides is
/// validated against the component schema (`D0404`, at the last op that
/// touched it). The library is flattened first (nested prefabs); its
/// diagnostics come along, anchored in the prefab documents.
pub fn expand(scene: &Scene, library: &PrefabLibrary, schemas: &SchemaSet) -> Expanded {
    let (flat, flat_diagnostics) = library.flattened(schemas);
    let mut expanded = expand_in(scene, &flat, schemas);
    // Only the flattening problems of prefabs this scene reaches are its
    // business (a project validation reports every prefab's, once).
    let reachable = library.reachable_from(scene.instances().map(|e| {
        e.prefab
            .as_ref()
            .map(PrefabInstance::prefab_id)
            .unwrap_or_default()
    }));
    let paths: HashSet<&str> = reachable
        .iter()
        .filter_map(|id| library.get(*id))
        .filter_map(|p| p.document.path.as_deref())
        .collect();
    expanded.diagnostics.splice(
        0..0,
        flat_diagnostics
            .into_iter()
            .filter(|d| d.document.as_deref().is_none_or(|p| paths.contains(p))),
    );
    expanded
}

/// [`expand`] against a library already flattened.
fn expand_in(scene: &Scene, library: &PrefabLibrary, schemas: &SchemaSet) -> Expanded {
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
        if !e.components.is_empty() {
            // Reported by validation (D0408): the root's components are
            // the instance's; there is nothing to put its own next to.
            continue;
        }
        let Some(prefab) = library.get_ref(&inst.asset) else {
            diagnostics.push(anchor(
                src,
                Diagnostic::error(
                    Code::PREFAB_UNKNOWN,
                    format!("prefab {} is not available", inst.describe()),
                )
                .with_id(e.id.to_string())
                .with_suggestion(if !inst.asset.sub.is_empty() && inst.asset.sub != "prefab" {
                    format!(
                        "an imported source's hierarchy is its \"prefab\" sub-asset, not \"{}\"",
                        inst.asset.sub
                    )
                } else if !inst.asset.sub.is_empty() {
                    "the asset must be a rig document or a source that imports as a model with a node hierarchy (fynkel.gltf, fynkel.fbx); prefab documents need no sub".to_string()
                } else if library.is_empty() {
                    "pass the prefab directory (--prefabs <dir>)".to_string()
                } else {
                    format!("{} prefab(s) loaded; check the asset id", library.len())
                }),
                &format!("{}/asset", inst.pointer),
            ));
            continue;
        };
        if prefab.has_instances() {
            // Left unflattened: its own expansion failed (reported there).
            diagnostics.push(anchor(
                src,
                Diagnostic::error(
                    Code::PREFAB_NESTED,
                    format!(
                        "prefab {} still contains a prefab instance its own expansion could not resolve",
                        prefab.id()
                    ),
                )
                .with_id(e.id.to_string())
                .with_suggestion("fix the prefab's own instances first (see its diagnostics)"),
                &format!("{}/asset", inst.pointer),
            ));
            continue;
        }

        if prefab
            .entity(prefab.root)
            .is_none_or(|r| r.parent.is_some())
        {
            // Reported by the prefab's own validation (D0400).
            diagnostics.push(anchor(
                src,
                Diagnostic::error(
                    Code::PREFAB_ROOT,
                    format!("prefab {} has no valid root", inst.describe()),
                )
                .with_id(e.id.to_string())
                .with_suggestion("fix the prefab first (see its diagnostics)"),
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
                    } else if parent.is_none() {
                        Err(fail(
                            Code::PREFAB_OVERRIDE_INVALID,
                            format!("addObject: entity {entity} needs a parent in the prefab (the root at the top)"),
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
                OverrideOp::RemoveObject { entity } if *entity == work.root => Err(fail(
                    Code::PREFAB_OVERRIDE_INVALID,
                    "removeObject: the root is the instance itself; delete the instance".to_string(),
                )),
                OverrideOp::RemoveObject { entity } => {
                    let removed = work.remove_subtree(*entity);
                    touched.retain(|(en, _), _| !removed.contains(en));
                    Ok(())
                }
                OverrideOp::SetActive { entity, .. } if *entity == work.root => Err(fail(
                    Code::PREFAB_OVERRIDE_INVALID,
                    "setActive: the root's active flag is the instance's own; set it on the instance".to_string(),
                )),
                OverrideOp::SetActive { value, .. } => {
                    work.entity_mut(entity_id).unwrap().active = *value;
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

        // Remap and emit: the root is the instance entity itself.
        let root_local = work.root;
        let local_ids: HashSet<Uuid> = work.entities.iter().map(|w| w.id).collect();
        let map_entity = |id: Uuid| {
            if id == root_local {
                e.id
            } else {
                derive_id(inst.instance, id)
            }
        };
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
            if w.id != root_local && !taken.insert(sid) {
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
        let components_of = |w: &Entity| -> Node {
            Node::object(
                w.components
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
                    .collect(),
            )
        };
        let children_of = |w: &Entity| -> Vec<Node> {
            w.children
                .iter()
                .map(|c| Node::string(map_entity(*c).to_string()))
                .collect()
        };
        let mut new_members: Vec<Member> = Vec::with_capacity(work.entities.len());
        let mut root_entity = None;
        for w in &work.entities {
            if w.id == root_local {
                root_entity = Some(w);
                continue;
            }
            let parent = Node::string(
                map_entity(w.parent.expect("only the root is parentless")).to_string(),
            );
            let mut members = vec![("name".into(), Node::string(w.name.clone()))];
            if !w.active {
                members.push(("active".into(), Node::synthetic(Value::Bool(false))));
            }
            members.extend([
                ("parent".into(), parent),
                (
                    "children".into(),
                    Node::synthetic(Value::Array(children_of(w))),
                ),
                ("components".into(), components_of(w)),
            ]);
            new_members.push(Member {
                key: map_entity(w.id).to_string(),
                key_span: Span::default(),
                value: Node::object(members),
            });
        }
        let root_entity = root_entity.expect("the root is never removed");
        let entities = root
            .get_mut("entities")
            .and_then(Node::as_object_mut)
            .expect("scene has an entities object");
        let anchor_index = entities
            .iter()
            .position(|m| m.key == e.id.to_string())
            .expect("anchor entity exists");
        // The instance takes the root's components, and the root's children
        // before the ones the scene added under it.
        let node = &mut entities[anchor_index].value;
        let mut children = children_of(root_entity);
        if let Some(Value::Array(own)) = node.get("children").map(|c| &c.value) {
            children.extend(own.iter().cloned());
        }
        node.set("children", Node::synthetic(Value::Array(children)));
        node.set("components", components_of(root_entity));
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
    root: Uuid,
}

impl Working {
    fn new(prefab: &Prefab) -> Working {
        Working {
            entities: prefab.entities.clone(),
            root: prefab.root,
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
        if let Some(p) = parent {
            self.entity_mut(p).unwrap().children.push(id);
        }
        self.entities.push(Entity {
            id,
            name: name.to_string(),
            active: true,
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

/// Pointer + position for the cold paths (a handful of lookups per
/// document); passes with a lookup per entity go through a [`Locator`].
fn anchor(root: &Node, d: Diagnostic, pointer: &str) -> Diagnostic {
    Locator::new(root).anchor(d, pointer)
}

/// `inspect --json` payload for a prefab.
pub fn inspect_json(prefab: &Prefab) -> serde_json::Value {
    serde_json::json!({
        "kind": "prefab",
        "id": prefab.id().to_string(),
        "name": prefab.document.envelope.name,
        "formatVersion": prefab.document.envelope.format_version,
        "revision": crate::authoring::Revision::of(&prefab.document).map(|r| r.to_string()).ok(),
        "entityCount": prefab.entities.len(),
        "root": prefab.root.to_string(),
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

/// Human `inspect` output: the root first, indented by depth.
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
        "prefab {:?} ({}) formatVersion {}: {} entities\n",
        prefab.document.envelope.name,
        prefab.id(),
        prefab.document.envelope.format_version,
        prefab.entities.len(),
    );
    if let Some(r) = prefab.entity(prefab.root) {
        walk(prefab, r, 0, &mut out);
    }
    out
}
