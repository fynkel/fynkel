//! `setLayersAndTags` (ADR-0020, L4): the project's `layers` and `tags`
//! replaced as one asset operation, with every scene and prefab that uses
//! them rewritten so each object keeps its meaning.
//!
//! A document stores a layer as its index and a tag set as bits, so a
//! rename or an entry added at the end changes the project file only. A
//! reorder or a removal rewrites, in every scene and prefab:
//!
//! - `fynkel.Layer.layer` — to the entry's new index; a removed layer (or
//!   an index no entry had) becomes `default` (0);
//! - `fynkel.Tags.bits` — each bit moved to its tag's new place; a removed
//!   tag's bit (and a bit no tag had) is dropped;
//! - every layer mask — `fynkel.Collider.mask`,
//!   `fynkel.CharacterController.mask`, `fynkel.TriggerZone.mask`, the
//!   `culling_mask` of `fynkel.Camera`, `fynkel.PointLight` and
//!   `fynkel.SpotLight`, and a behaviour field hinted `kind: layerMask` —
//!   bit by bit the same way. A mask that had every bit past the old list
//!   set ("Everything") keeps every bit past the new one and gains the new
//!   entries;
//! - the same values inside prefab overrides: an `addComponent`'s and an
//!   `addObject`'s component data, and a `setField` whose component the
//!   prefab (flattened, with the earlier overrides' added components)
//!   resolves. A `setField` of a field named like one of these whose
//!   component cannot be resolved refuses the operation with its pointer.
//!
//! Refused, with nothing written, when a document that needs the rewrite
//! is open with unsaved edits. Open documents without edits are listed in
//! the result's `reload` (handles) for the client to re-read. Undo and
//! redo put every file back as the operation found or left it (refused
//! when a file changed since).

use std::collections::HashMap;
use std::path::PathBuf;

use fynkel_contracts::{AssetRef, Code, Diagnostic};
use fynkel_document::project::LayersAndTags;
use fynkel_document::{AssetKind, Document, DocumentKind, Node, Prefab, PrefabLibrary, Scene};
use serde_json::{json, Value};

use crate::ops::{io, AssetOp, Done, FileRewrite};
use crate::{AuthoringSession, Handle, OpenProject};

/// The operation's name.
pub const OP: &str = "setLayersAndTags";

/// Native fields holding a layer mask.
pub const LAYER_MASK_FIELDS: &[(&str, &str)] = &[
    ("fynkel.Collider", "mask"),
    ("fynkel.CharacterController", "mask"),
    ("fynkel.TriggerZone", "mask"),
    ("fynkel.Camera", "culling_mask"),
    ("fynkel.PointLight", "culling_mask"),
    ("fynkel.SpotLight", "culling_mask"),
];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    LayerIndex,
    TagBits,
    LayerMask,
}

/// Old index → new index for each old entry (`None`: removed), and how
/// many entries the new list has, with which new indices are new entries.
#[derive(Clone, Debug)]
struct Map {
    from_old: Vec<Option<u32>>,
    new_len: usize,
    added: Vec<u32>,
}

impl Map {
    /// From the new list's `from` (the old index each new entry comes from).
    fn new(old_len: usize, from: &[Option<u32>]) -> Map {
        let mut from_old = vec![None; old_len];
        let mut added = Vec::new();
        for (new, old) in from.iter().enumerate() {
            match old {
                Some(o) => from_old[*o as usize] = Some(new as u32),
                None => added.push(new as u32),
            }
        }
        Map {
            from_old,
            new_len: from.len(),
            added,
        }
    }

    /// Nothing moved and nothing was removed: indices keep their meaning.
    fn is_identity(&self) -> bool {
        self.from_old
            .iter()
            .enumerate()
            .all(|(i, to)| *to == Some(i as u32))
    }

    fn index(&self, i: u32) -> u32 {
        self.from_old
            .get(i as usize)
            .copied()
            .flatten()
            .unwrap_or(0)
    }

    fn bits(&self, bits: u32) -> u32 {
        let mut out = 0;
        for (i, to) in self.from_old.iter().enumerate() {
            if let Some(j) = to {
                if bits & (1 << i) != 0 {
                    out |= 1 << j;
                }
            }
        }
        out
    }

    fn mask(&self, mask: u32) -> u32 {
        let mut out = self.bits(mask);
        let past = |len: usize| if len >= 32 { 0 } else { u32::MAX << len };
        let old_past = past(self.from_old.len());
        if old_past != 0 && mask & old_past == old_past {
            out |= past(self.new_len);
            for a in &self.added {
                out |= 1 << a;
            }
        }
        out
    }
}

/// A new list and where each entry comes from, checked.
fn read_list(
    args: &Value,
    key: &str,
    from_key: &str,
    old: &[String],
    max: usize,
) -> Result<(Vec<String>, Vec<Option<u32>>), Diagnostic> {
    let invalid =
        |m: String| Diagnostic::error(Code::SESSION_INVALID_REQUEST, format!("{OP}: {m}"));
    let names: Vec<String> = match args.get(key) {
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| invalid(format!("\"{key}\" holds names (strings)")))
            })
            .collect::<Result<_, _>>()?,
        _ => {
            return Err(invalid(format!(
                "\"{key}\" (an array of names) is required"
            )))
        }
    };
    if names.len() > max {
        return Err(invalid(format!("at most {max} {key}, not {}", names.len())));
    }
    for (i, n) in names.iter().enumerate() {
        if n.trim().is_empty() {
            return Err(invalid(format!("{key}[{i}] is empty")));
        }
        if names[..i].contains(n) {
            return Err(invalid(format!("{key}: \"{n}\" is there twice")));
        }
    }
    let from: Vec<Option<u32>> = match args.get(from_key) {
        None | Some(Value::Null) => (0..names.len())
            .map(|i| (i < old.len()).then_some(i as u32))
            .collect(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| match v {
                Value::Null => Ok(None),
                v => v
                    .as_u64()
                    .filter(|i| (*i as usize) < old.len())
                    .map(|i| Some(i as u32))
                    .ok_or_else(|| {
                        invalid(format!(
                            "\"{from_key}\" holds an old index below {} or null, not {v}",
                            old.len()
                        ))
                    }),
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(invalid(format!("\"{from_key}\" is an array"))),
    };
    if from.len() != names.len() {
        return Err(invalid(format!(
            "\"{from_key}\" has {} entries for {} {key}",
            from.len(),
            names.len()
        )));
    }
    for (i, f) in from.iter().enumerate() {
        if f.is_some() && from[..i].contains(f) {
            return Err(invalid(format!(
                "\"{from_key}\": old entry {} is used twice",
                f.unwrap()
            )));
        }
    }
    Ok((names, from))
}

/// What the rewrite of one document needs to know.
struct Rewriter<'a> {
    layers: Map,
    tags: Map,
    library: &'a PrefabLibrary,
    /// Behaviour fields hinted `layerMask`, by type (read from the schemas once).
    behaviour_masks: HashMap<String, Vec<String>>,
}

impl Rewriter<'_> {
    fn kinds(&self, type_id: &str) -> Vec<(String, Kind)> {
        match type_id {
            "fynkel.Layer" => return vec![("layer".into(), Kind::LayerIndex)],
            "fynkel.Tags" => return vec![("bits".into(), Kind::TagBits)],
            _ => {}
        }
        let mut out: Vec<(String, Kind)> = LAYER_MASK_FIELDS
            .iter()
            .filter(|(t, _)| *t == type_id)
            .map(|(_, f)| ((*f).to_string(), Kind::LayerMask))
            .collect();
        if let Some(fields) = self.behaviour_masks.get(type_id) {
            out.extend(fields.iter().map(|f| (f.clone(), Kind::LayerMask)));
        }
        out
    }

    /// Whether some type's layer or tag field has this name (a `setField`
    /// that names it cannot be let through unresolved).
    fn could_be_layered(&self, field: &str) -> bool {
        matches!(field, "layer" | "bits")
            || LAYER_MASK_FIELDS.iter().any(|(_, f)| *f == field)
            || self.behaviour_masks.values().flatten().any(|f| f == field)
    }

    fn value(&self, kind: Kind, v: &Node) -> Option<Node> {
        let n = v.as_i64()?;
        if !(0..=u32::MAX as i64).contains(&n) {
            return None;
        }
        let n = n as u32;
        let m = match kind {
            Kind::LayerIndex => self.layers.index(n),
            Kind::TagBits => self.tags.bits(n),
            Kind::LayerMask => self.layers.mask(n),
        };
        (m != n).then(|| Node::integer(m as i64))
    }

    /// Rewrites a component's `data` in place; whether it changed.
    fn data(&self, type_id: &str, data: &mut Node) -> bool {
        let mut changed = false;
        for (field, kind) in self.kinds(type_id) {
            let Some(v) = data.get(&field) else { continue };
            if let Some(n) = self.value(kind, v) {
                data.set(&field, n);
                changed = true;
            }
        }
        changed
    }

    /// Every component of a `components` object (`{ <id>: { type, data } }`).
    fn components(&self, components: &mut Node) -> bool {
        let mut changed = false;
        if let Some(members) = components.as_object_mut() {
            for m in members.iter_mut() {
                let ty = m
                    .value
                    .get("type")
                    .and_then(Node::as_str)
                    .map(str::to_string);
                if let (Some(ty), Some(data)) = (ty, m.value.get_mut("data")) {
                    changed |= self.data(&ty, data);
                }
            }
        }
        changed
    }

    /// One document; `Err` with the pointer of an override it cannot carry
    /// over.
    fn document(&self, doc: &mut Document, file: &str) -> Result<bool, Diagnostic> {
        // The prefab each instance anchor names, by entity key.
        let scene = match doc.kind {
            DocumentKind::Prefab => Prefab::from_document(doc.clone()).scene(),
            _ => Scene::from_document(doc.clone()),
        };
        let instances: HashMap<String, AssetRef> = scene
            .entities
            .iter()
            .filter_map(|e| Some((e.id.to_string(), e.prefab.as_ref()?.asset.clone())))
            .collect();
        let root = &mut doc.root;
        let mut changed = false;
        let Some(entities) = root.get_mut("entities").and_then(Node::as_object_mut) else {
            return Ok(false);
        };
        for entity in entities.iter_mut() {
            let at = format!("/entities/{}", entity.key);
            if let Some(c) = entity.value.get_mut("components") {
                changed |= self.components(c);
            }
            let Some(prefab) = entity.value.get_mut("prefab") else {
                continue;
            };
            let source = instances
                .get(&entity.key)
                .and_then(|r| self.library.get_ref(r));
            let Some(overrides) = prefab.get_mut("overrides").and_then(Node::as_array_mut) else {
                continue;
            };
            // Components the earlier overrides added, by id.
            let mut added: HashMap<String, String> = HashMap::new();
            for (i, o) in overrides.iter_mut().enumerate() {
                let op = o
                    .get("op")
                    .and_then(Node::as_str)
                    .unwrap_or_default()
                    .to_string();
                match op.as_str() {
                    "addComponent" => {
                        let ty = o.get("type").and_then(Node::as_str).map(str::to_string);
                        if let (Some(c), Some(ty)) = (
                            o.get("component")
                                .and_then(Node::as_str)
                                .map(str::to_string),
                            ty.clone(),
                        ) {
                            added.insert(c, ty);
                        }
                        if let (Some(ty), Some(data)) = (ty, o.get_mut("data")) {
                            changed |= self.data(&ty, data);
                        }
                    }
                    "addObject" => {
                        if let Some(members) = o.get("components").and_then(Node::as_object) {
                            for m in members {
                                if let Some(ty) = m.value.get("type").and_then(Node::as_str) {
                                    added.insert(m.key.clone(), ty.to_string());
                                }
                            }
                        }
                        if let Some(c) = o.get_mut("components") {
                            changed |= self.components(c);
                        }
                    }
                    "setField" => {
                        let field = o
                            .get("field")
                            .and_then(Node::as_str)
                            .unwrap_or_default()
                            .to_string();
                        let component = o
                            .get("component")
                            .and_then(Node::as_str)
                            .unwrap_or_default()
                            .to_string();
                        let entity_id = o
                            .get("entity")
                            .and_then(Node::as_str)
                            .unwrap_or_default()
                            .to_string();
                        let ty = added.get(&component).cloned().or_else(|| {
                            let (e, c) = (
                                fynkel_document::parse_uuid(&entity_id).ok()?,
                                fynkel_document::parse_uuid(&component).ok()?,
                            );
                            source?
                                .entities
                                .iter()
                                .find(|x| x.id == e)?
                                .components
                                .iter()
                                .find(|x| x.id == c)
                                .map(|x| x.type_id.clone())
                        });
                        let Some(ty) = ty else {
                            if self.could_be_layered(&field) {
                                return Err(Diagnostic::error(
                                    Code::SESSION_INVALID_REQUEST,
                                    format!(
                                        "{OP}: the override sets `{field}` on a component its prefab does not resolve, so its layer bits cannot be carried over; revert it or make the prefab available first"
                                    ),
                                )
                                .with_document(file.to_string())
                                .with_pointer(format!("{at}/prefab/overrides/{i}")));
                            }
                            continue;
                        };
                        let kind = self
                            .kinds(&ty)
                            .into_iter()
                            .find(|(f, _)| *f == field)
                            .map(|(_, k)| k);
                        if let (Some(kind), Some(v)) = (kind, o.get("value")) {
                            if let Some(n) = self.value(kind, v) {
                                o.set("value", n);
                                changed = true;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(changed)
    }
}

/// The fields of `schema` (a behaviour's JSON Schema) hinted `layerMask`.
fn layer_mask_fields(schema: &Value) -> Vec<String> {
    schema["properties"]
        .as_object()
        .map(|props| {
            props
                .iter()
                .filter(|(_, p)| p["x-fynkel"]["kind"].as_str() == Some("layerMask"))
                .map(|(k, _)| k.clone())
                .collect()
        })
        .unwrap_or_default()
}

impl AuthoringSession {
    /// `setLayersAndTags { layers, tags, layersFrom?, tagsFrom? }`: see the
    /// module docs. `layersFrom[i]` is the old index new layer `i` comes
    /// from, or null for a new one (absent: the lists line up by index —
    /// renames and additions). `layers[0]` is `default` and comes from 0.
    pub(crate) fn set_layers_and_tags(&mut self, args: &Value) -> Result<Value, Diagnostic> {
        let invalid =
            |m: String| Diagnostic::error(Code::SESSION_INVALID_REQUEST, format!("{OP}: {m}"));
        let project = self.project().expect("operate checked the project");
        let old = project.project.layers_and_tags.clone();
        let (layers, layers_from) = read_list(
            args,
            "layers",
            "layersFrom",
            &old.layers,
            LayersAndTags::MAX_LAYERS,
        )?;
        let (tags, tags_from) =
            read_list(args, "tags", "tagsFrom", &old.tags, LayersAndTags::MAX_TAGS)?;
        if layers.first().map(String::as_str) != Some("default")
            || layers_from.first() != Some(&Some(0))
        {
            return Err(invalid(
                "layer 0 is always \"default\" and stays first (it is where an object without a layer is)".into(),
            ));
        }
        let project_file = project.dir.join(fynkel_document::project::PROJECT_FILE);
        let layer_map = Map::new(old.layers.len(), &layers_from);
        let tag_map = Map::new(old.tags.len(), &tags_from);

        // The project file.
        let mut rewrites: Vec<FileRewrite> = Vec::new();
        let before = std::fs::read_to_string(&project_file)
            .map_err(|e| io(&project_file, format!("{OP}: cannot read: {e}")))?;
        let mut doc = project.project.document.clone();
        doc.root.set("layers", Node::from_serde(&json!(layers)));
        doc.root.set("tags", Node::from_serde(&json!(tags)));
        let after = doc.write()?;
        if after != before {
            rewrites.push(FileRewrite {
                path: project_file.clone(),
                before,
                after,
            });
        }

        // The documents, when an index moved.
        let mut warnings: Vec<Diagnostic> = Vec::new();
        if !layer_map.is_identity() || !tag_map.is_identity() {
            let mut library = PrefabLibrary::empty();
            let mut files: Vec<(PathBuf, DocumentKind)> = Vec::new();
            for entry in project.assets.entries() {
                let AssetKind::Document(kind @ (DocumentKind::Scene | DocumentKind::Prefab)) =
                    entry.kind
                else {
                    continue;
                };
                let file = entry.file(project.assets.root());
                let file = file.canonicalize().unwrap_or(file);
                if kind == DocumentKind::Prefab {
                    if let Ok(d) = Document::read(&file, Some(kind)) {
                        library.add(Prefab::from_document(d));
                    }
                }
                files.push((file, kind));
            }
            let (library, _) = library.flattened(&project.schemas);
            let behaviour_masks = project
                .schemas
                .behaviour_types()
                .iter()
                .filter_map(|b| {
                    let fields = layer_mask_fields(project.schema(&b.type_id)?);
                    (!fields.is_empty()).then(|| (b.type_id.clone(), fields))
                })
                .collect();
            let rewriter = Rewriter {
                layers: layer_map.clone(),
                tags: tag_map.clone(),
                library: &library,
                behaviour_masks,
            };
            files.sort_by(|a, b| a.0.cmp(&b.0));
            for (file, kind) in files {
                let shown = file.display().to_string();
                let mut doc = match Document::read(&file, Some(kind)) {
                    Ok(d) => d,
                    Err(diags) => {
                        warnings.push(
                            Diagnostic::warning(
                                Code::SESSION_IO,
                                format!(
                                    "{OP}: not rewritten, it does not read: {}",
                                    diags.first().map(|d| d.message.as_str()).unwrap_or("")
                                ),
                            )
                            .with_document(shown),
                        );
                        continue;
                    }
                };
                if !rewriter.document(&mut doc, &shown)? {
                    continue;
                }
                if self
                    .documents
                    .values()
                    .any(|d| d.path == file && d.sub.is_none() && d.editor.is_dirty())
                {
                    return Err(invalid(format!(
                        "{} is open with unsaved changes and uses the layers or tags that move; save or revert it first",
                        file.display()
                    ))
                    .with_document(shown));
                }
                let before = std::fs::read_to_string(&file)
                    .map_err(|e| io(&file, format!("{OP}: cannot read: {e}")))?;
                let after = doc.write()?;
                rewrites.push(FileRewrite {
                    path: file,
                    before,
                    after,
                });
            }
        }
        if rewrites.is_empty() {
            return Err(invalid("the lists are what the project already has".into()));
        }
        let label = "Edit Tags and Layers".to_string();
        self.reload_after = self.write_rewrites(&rewrites, false, OP)?;
        let rewritten: Vec<String> = rewrites
            .iter()
            .map(|r| r.path.display().to_string())
            .collect();
        self.assets_history.push(Done {
            op: AssetOp::Rewrote { files: rewrites },
            label: label.clone(),
        });
        Ok(self.finish(
            OP,
            &label,
            None,
            json!({
                "layers": layers,
                "tags": tags,
                "rewritten": rewritten,
                "diagnostics": warnings.iter().map(fynkel_document::diagnostic_json).collect::<Vec<_>>(),
            }),
        ))
    }

    /// Writes each file's `after` (`before` when [inverse]), refusing when a
    /// file is not what it should be now or an open document with edits
    /// would be overwritten; re-reads the project when its file is among
    /// them and rescans the rest. The handles of open documents to reload.
    pub(crate) fn write_rewrites(
        &mut self,
        files: &[FileRewrite],
        inverse: bool,
        op: &str,
    ) -> Result<Vec<Handle>, Diagnostic> {
        let invalid =
            |m: String| Diagnostic::error(Code::SESSION_INVALID_REQUEST, format!("{op}: {m}"));
        for f in files {
            let expected = if inverse { &f.after } else { &f.before };
            let now = std::fs::read_to_string(&f.path).unwrap_or_default();
            if now != *expected {
                return Err(invalid(format!(
                    "{} changed since; it is not overwritten",
                    f.path.display()
                ))
                .with_document(f.path.display().to_string()));
            }
            if self
                .documents
                .values()
                .any(|d| d.path == f.path && d.sub.is_none() && d.editor.is_dirty())
            {
                return Err(invalid(format!(
                    "{} is open with unsaved changes; save or revert it first",
                    f.path.display()
                ))
                .with_document(f.path.display().to_string()));
            }
        }
        let mut written: Vec<&FileRewrite> = Vec::new();
        for f in files {
            let text = if inverse { &f.before } else { &f.after };
            if let Err(e) = std::fs::write(&f.path, text) {
                // Put back what was written so far.
                for w in written {
                    let _ = std::fs::write(&w.path, if inverse { &w.after } else { &w.before });
                }
                return Err(io(&f.path, format!("{op}: cannot write: {e}")));
            }
            written.push(f);
        }
        let dir = self.project().unwrap().dir.clone();
        let project_file = dir.join(fynkel_document::project::PROJECT_FILE);
        if files.iter().any(|f| f.path == project_file) {
            match OpenProject::open(&dir) {
                Ok(p) => self.project = Some(p),
                Err(diags) => {
                    return Err(diags
                        .into_iter()
                        .next()
                        .unwrap_or_else(|| invalid("the project no longer opens".into())))
                }
            }
        }
        let paths: Vec<PathBuf> = files
            .iter()
            .filter(|f| f.path != project_file)
            .map(|f| f.path.clone())
            .collect();
        if !paths.is_empty() {
            self.project_mut().unwrap().rescan(&paths);
        }
        Ok(self
            .documents
            .iter()
            .filter(|(_, d)| d.sub.is_none() && paths.contains(&d.path))
            .map(|(h, _)| *h)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::Map;

    #[test]
    fn maps_move_indices_bits_and_masks() {
        // default, a, b, c → default, c, a  (b removed, c moved up)
        let m = Map::new(4, &[Some(0), Some(3), Some(1)]);
        assert!(!m.is_identity());
        assert_eq!(
            (m.index(0), m.index(1), m.index(2), m.index(3)),
            (0, 2, 0, 1)
        );
        assert_eq!(m.index(9), 0, "an undeclared index becomes default");
        assert_eq!(m.bits(0b1010), 0b0110);
        // Everything stays everything; a partial mask does not grow.
        assert_eq!(m.mask(u32::MAX), u32::MAX);
        assert_eq!(m.mask(0b0011), 0b0101);
        // A new entry joins the masks that had every undeclared bit.
        let grow = Map::new(2, &[Some(1), Some(0), None]);
        assert_eq!(grow.mask(u32::MAX), u32::MAX);
        assert_eq!(grow.mask(0b01 | (u32::MAX << 2)), 0b110 | (u32::MAX << 3));
        assert_eq!(grow.mask(0b01), 0b10);
        assert!(Map::new(2, &[Some(0), Some(1), None]).is_identity());
    }
}
