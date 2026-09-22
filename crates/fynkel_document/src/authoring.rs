//! Authoring commands and transactions (blueprint §11.1, ADR-0016, §19.2c
//! item 14): the one implementation of document mutation the editor, the
//! CLI and agents share.
//!
//! An [`Editor`] holds one scene, prefab or material document. A [`Request`] — a
//! transaction of [`Command`]s against a **base revision** — is applied
//! atomically: every command compiles to primitive [`Edit`]s on the JSON
//! tree (each recording its exact inverse), the result is validated, and a
//! transaction that fails a command, introduces validation errors or cannot
//! perform its side effects is rolled back to the byte. A stale base
//! revision is a conflict (`FYN-E0001`) and nothing is touched. Undo and
//! redo replay the recorded inverses/edits, so positions and key order come
//! back exactly.
//!
//! A [`Revision`] is the blake3 hash of the canonical text, so two tools
//! that read the same bytes agree on it without shared state
//! (`fynkel scene inspect --json` prints it).
//!
//! `CreateMaterial` and `SaveDocument` are side effects performed after the
//! transaction validated; they have no undo (a saved file stays saved).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use fynkel_contracts::{AssetRef, Code, Diagnostic};
use serde_json::{json, Value as J};
use uuid::Uuid;

use crate::json::{Member, Node, Span, Value};
use crate::material::{Material, ShaderLibrary};
use crate::prefab::{self, Prefab, PrefabLibrary};
use crate::scene::{escape, Scene};
use crate::{diagnostic_json, has_errors, parse_uuid, Document, DocumentKind, Envelope, SchemaSet};

// ------------------------------------------------------------------ revision

/// The identity of one document state: blake3 of its canonical text.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Revision(String);

impl Revision {
    pub fn of(document: &Document) -> Result<Revision, Diagnostic> {
        Ok(Revision::of_text(&document.write()?))
    }

    pub fn of_text(text: &str) -> Revision {
        Revision(blake3::hash(text.as_bytes()).to_hex().to_string())
    }

    pub fn parse(s: &str) -> Option<Revision> {
        (s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| Revision(s.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Revision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// ------------------------------------------------------------------ commands

/// A component to add: `id` new when `None`, `version` 1 when `None`,
/// `data` an empty object when `None`.
#[derive(Clone, Debug, PartialEq)]
pub struct ComponentSpec {
    pub id: Option<Uuid>,
    pub type_id: String,
    pub version: Option<u32>,
    pub data: Option<Node>,
}

/// The §11.1 command set. Entity commands address persistent ids; `field`
/// is a key of the component's `data` or a JSON pointer into it (`/position/1`).
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    CreateObject {
        id: Option<Uuid>,
        name: String,
        parent: Option<Uuid>,
        /// Position among the parent's children (or the document's roots);
        /// appended when `None`.
        index: Option<usize>,
        components: Vec<ComponentSpec>,
    },
    /// Removes the entity and its subtree.
    DeleteObject {
        id: Uuid,
    },
    RenameObject {
        id: Uuid,
        name: String,
    },
    ReparentObject {
        id: Uuid,
        parent: Option<Uuid>,
        index: Option<usize>,
    },
    AddComponent {
        entity: Uuid,
        component: ComponentSpec,
    },
    RemoveComponent {
        entity: Uuid,
        component: Uuid,
    },
    SetField {
        entity: Uuid,
        component: Uuid,
        field: String,
        value: Node,
    },
    /// Removes a field of a component's `data` (distinct from setting null).
    UnsetField {
        entity: Uuid,
        component: Uuid,
        field: String,
    },
    /// `SetField` with an `assetRef` value.
    AssignAsset {
        entity: Uuid,
        component: Uuid,
        field: String,
        asset: AssetRef,
    },
    /// Creates an entity that is a prefab instance (scene documents only).
    InstantiatePrefab {
        id: Option<Uuid>,
        name: String,
        parent: Option<Uuid>,
        index: Option<usize>,
        prefab: AssetRef,
        instance: Option<Uuid>,
        components: Vec<ComponentSpec>,
    },
    /// Adds an override (the JSON op object of the prefab model) to an
    /// instance, replacing an existing override with the same target.
    SetPrefabOverride {
        entity: Uuid,
        r#override: Node,
    },
    /// Removes the override at `index`, or every override when `None`.
    RevertPrefabOverride {
        entity: Uuid,
        index: Option<usize>,
    },
    /// Replaces an instance by its expansion (M4-07): the anchor loses its
    /// `prefab` key and gains the instance roots as trailing children, the
    /// expanded entities — the prefab's entities with the overrides applied
    /// and the ids the runtime derives — are inserted right after the anchor
    /// in `entities`, so the document reads exactly like the expansion.
    /// Needs the prefab library ([`Editor::apply_with_prefabs`]); an
    /// instance that does not expand (unknown prefab, invalid override,
    /// id collision) is refused with its expansion diagnostics.
    UnpackPrefab {
        entity: Uuid,
    },
    /// Copies an entity and its subtree next to the original (M4-01):
    /// fresh entity, component and prefab-instance ids, every reference
    /// to a copied id inside the copy remapped, the copy named `name` (or
    /// the original's name) and placed at `index` of the same parent (or
    /// right after the original). The first affected id is the copy's
    /// root; `id` picks it when given.
    DuplicateObject {
        id: Uuid,
        new_id: Option<Uuid>,
        name: Option<String>,
        index: Option<usize>,
    },
    /// Sets the node at a document-level JSON pointer (a material's
    /// `/base_color`, a scene's `/settings/...`; M4-01). An existing node
    /// is replaced, a missing last token is inserted as a member of its
    /// object parent or appended to its array parent. The envelope's
    /// `kind`, `formatVersion` and `id`, and the entity tree, are refused
    /// (`FYN-E0012`): entities have their own commands.
    SetDocumentField {
        pointer: String,
        value: Node,
    },
    /// Removes the node at a document-level JSON pointer (same rules).
    UnsetDocumentField {
        pointer: String,
    },
    /// Writes a new material document to `path` (refuses to overwrite).
    CreateMaterial {
        id: Option<Uuid>,
        name: String,
        shader: Option<String>,
        path: PathBuf,
    },
    /// Writes the document to `path` (or where it was read from).
    SaveDocument {
        path: Option<PathBuf>,
    },
}

impl Command {
    pub const fn name(&self) -> &'static str {
        match self {
            Command::CreateObject { .. } => "createObject",
            Command::DeleteObject { .. } => "deleteObject",
            Command::RenameObject { .. } => "renameObject",
            Command::ReparentObject { .. } => "reparentObject",
            Command::AddComponent { .. } => "addComponent",
            Command::RemoveComponent { .. } => "removeComponent",
            Command::SetField { .. } => "setField",
            Command::UnsetField { .. } => "unsetField",
            Command::AssignAsset { .. } => "assignAsset",
            Command::InstantiatePrefab { .. } => "instantiatePrefab",
            Command::SetPrefabOverride { .. } => "setPrefabOverride",
            Command::RevertPrefabOverride { .. } => "revertPrefabOverride",
            Command::UnpackPrefab { .. } => "unpackPrefab",
            Command::DuplicateObject { .. } => "duplicateObject",
            Command::SetDocumentField { .. } => "setDocumentField",
            Command::UnsetDocumentField { .. } => "unsetDocumentField",
            Command::CreateMaterial { .. } => "createMaterial",
            Command::SaveDocument { .. } => "saveDocument",
        }
    }

    /// Reads one command from its JSON form (`{ "command": "setField", ... }`).
    pub fn from_json(v: &J) -> Result<Command, Diagnostic> {
        let invalid = |msg: String| Diagnostic::error(Code::EDIT_INVALID_COMMAND, msg);
        let Some(name) = v.get("command").and_then(J::as_str) else {
            return Err(invalid("command needs a \"command\" name".into()));
        };
        let uuid = |key: &str| -> Result<Uuid, Diagnostic> {
            let s = v
                .get(key)
                .and_then(J::as_str)
                .ok_or_else(|| invalid(format!("{name}: \"{key}\" must be a UUID string")))?;
            parse_uuid(s).map_err(|why| invalid(format!("{name}: \"{key}\": {why}")))
        };
        let opt_uuid = |key: &str| -> Result<Option<Uuid>, Diagnostic> {
            match v.get(key) {
                None | Some(J::Null) => Ok(None),
                Some(_) => uuid(key).map(Some),
            }
        };
        let string = |key: &str| -> Result<String, Diagnostic> {
            v.get(key)
                .and_then(J::as_str)
                .map(str::to_string)
                .ok_or_else(|| invalid(format!("{name}: \"{key}\" must be a string")))
        };
        let opt_string =
            |key: &str| -> Option<String> { v.get(key).and_then(J::as_str).map(str::to_string) };
        let index = || -> Result<Option<usize>, Diagnostic> {
            match v.get("index") {
                None | Some(J::Null) => Ok(None),
                Some(J::Number(n)) if n.as_u64().is_some() => Ok(n.as_u64().map(|n| n as usize)),
                Some(_) => Err(invalid(format!(
                    "{name}: \"index\" must be a non-negative integer"
                ))),
            }
        };
        let asset_ref = |key: &str| -> Result<AssetRef, Diagnostic> {
            let o = v
                .get(key)
                .and_then(J::as_object)
                .ok_or_else(|| invalid(format!("{name}: \"{key}\" must be {{ asset, sub? }}")))?;
            let id = o
                .get("asset")
                .and_then(J::as_str)
                .and_then(|s| s.parse::<fynkel_contracts::Uuid>().ok())
                .ok_or_else(|| invalid(format!("{name}: \"{key}\".asset must be a UUID")))?;
            Ok(AssetRef {
                asset: id,
                sub: o.get("sub").and_then(J::as_str).unwrap_or("").to_string(),
            })
        };
        let components = || -> Result<Vec<ComponentSpec>, Diagnostic> {
            let mut out = Vec::new();
            for (i, c) in v
                .get("components")
                .and_then(J::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                out.push(
                    component_spec(c)
                        .map_err(|why| invalid(format!("{name}: components[{i}]: {why}")))?,
                );
            }
            Ok(out)
        };
        Ok(match name {
            "createObject" => Command::CreateObject {
                id: opt_uuid("id")?,
                name: opt_string("name").unwrap_or_default(),
                parent: opt_uuid("parent")?,
                index: index()?,
                components: components()?,
            },
            "deleteObject" => Command::DeleteObject { id: uuid("id")? },
            "renameObject" => Command::RenameObject {
                id: uuid("id")?,
                name: string("name")?,
            },
            "reparentObject" => Command::ReparentObject {
                id: uuid("id")?,
                parent: opt_uuid("parent")?,
                index: index()?,
            },
            "addComponent" => Command::AddComponent {
                entity: uuid("entity")?,
                component: component_spec(v.get("component").unwrap_or(&J::Null))
                    .map_err(|why| invalid(format!("{name}: \"component\": {why}")))?,
            },
            "removeComponent" => Command::RemoveComponent {
                entity: uuid("entity")?,
                component: uuid("component")?,
            },
            "setField" => Command::SetField {
                entity: uuid("entity")?,
                component: uuid("component")?,
                field: string("field")?,
                value: Node::from_serde(
                    v.get("value")
                        .ok_or_else(|| invalid(format!("{name}: \"value\" is required")))?,
                ),
            },
            "unsetField" => Command::UnsetField {
                entity: uuid("entity")?,
                component: uuid("component")?,
                field: string("field")?,
            },
            "assignAsset" => Command::AssignAsset {
                entity: uuid("entity")?,
                component: uuid("component")?,
                field: string("field")?,
                asset: asset_ref("asset")?,
            },
            "instantiatePrefab" => Command::InstantiatePrefab {
                id: opt_uuid("id")?,
                name: opt_string("name").unwrap_or_default(),
                parent: opt_uuid("parent")?,
                index: index()?,
                prefab: asset_ref("prefab")?,
                instance: opt_uuid("instance")?,
                components: components()?,
            },
            "setPrefabOverride" => Command::SetPrefabOverride {
                entity: uuid("entity")?,
                r#override: Node::from_serde(
                    v.get("override").filter(|o| o.is_object()).ok_or_else(|| {
                        invalid(format!("{name}: \"override\" must be an op object"))
                    })?,
                ),
            },
            "revertPrefabOverride" => Command::RevertPrefabOverride {
                entity: uuid("entity")?,
                index: index()?,
            },
            "unpackPrefab" => Command::UnpackPrefab {
                entity: uuid("entity")?,
            },
            "duplicateObject" => Command::DuplicateObject {
                id: uuid("id")?,
                new_id: opt_uuid("newId")?,
                name: opt_string("name"),
                index: index()?,
            },
            "setDocumentField" => Command::SetDocumentField {
                pointer: string("pointer")?,
                value: Node::from_serde(
                    v.get("value")
                        .ok_or_else(|| invalid(format!("{name}: \"value\" is required")))?,
                ),
            },
            "unsetDocumentField" => Command::UnsetDocumentField {
                pointer: string("pointer")?,
            },
            "createMaterial" => Command::CreateMaterial {
                id: opt_uuid("id")?,
                name: string("name")?,
                shader: opt_string("shader"),
                path: PathBuf::from(string("path")?),
            },
            "saveDocument" => Command::SaveDocument {
                path: opt_string("path").map(PathBuf::from),
            },
            other => return Err(invalid(format!("unknown command {other:?}"))),
        })
    }

    pub fn to_json(&self) -> J {
        let id_or_null = |u: &Option<Uuid>| u.map(|u| J::String(u.to_string())).unwrap_or(J::Null);
        let specs = |c: &[ComponentSpec]| J::Array(c.iter().map(spec_json).collect());
        let mut v = match self {
            Command::CreateObject {
                id,
                name,
                parent,
                index,
                components,
            } => {
                json!({ "id": id_or_null(id), "name": name, "parent": id_or_null(parent), "index": index, "components": specs(components) })
            }
            Command::DeleteObject { id } => json!({ "id": id.to_string() }),
            Command::RenameObject { id, name } => json!({ "id": id.to_string(), "name": name }),
            Command::ReparentObject { id, parent, index } => {
                json!({ "id": id.to_string(), "parent": id_or_null(parent), "index": index })
            }
            Command::AddComponent { entity, component } => {
                json!({ "entity": entity.to_string(), "component": spec_json(component) })
            }
            Command::RemoveComponent { entity, component } => {
                json!({ "entity": entity.to_string(), "component": component.to_string() })
            }
            Command::SetField {
                entity,
                component,
                field,
                value,
            } => {
                json!({ "entity": entity.to_string(), "component": component.to_string(), "field": field, "value": value.to_serde() })
            }
            Command::UnsetField {
                entity,
                component,
                field,
            } => {
                json!({ "entity": entity.to_string(), "component": component.to_string(), "field": field })
            }
            Command::AssignAsset {
                entity,
                component,
                field,
                asset,
            } => {
                json!({ "entity": entity.to_string(), "component": component.to_string(), "field": field, "asset": asset_json(asset) })
            }
            Command::InstantiatePrefab {
                id,
                name,
                parent,
                index,
                prefab,
                instance,
                components,
            } => {
                json!({ "id": id_or_null(id), "name": name, "parent": id_or_null(parent), "index": index, "prefab": asset_json(prefab), "instance": id_or_null(instance), "components": specs(components) })
            }
            Command::SetPrefabOverride { entity, r#override } => {
                json!({ "entity": entity.to_string(), "override": r#override.to_serde() })
            }
            Command::RevertPrefabOverride { entity, index } => {
                json!({ "entity": entity.to_string(), "index": index })
            }
            Command::UnpackPrefab { entity } => json!({ "entity": entity.to_string() }),
            Command::DuplicateObject {
                id,
                new_id,
                name,
                index,
            } => {
                json!({ "id": id.to_string(), "newId": id_or_null(new_id), "name": name, "index": index })
            }
            Command::SetDocumentField { pointer, value } => {
                json!({ "pointer": pointer, "value": value.to_serde() })
            }
            Command::UnsetDocumentField { pointer } => json!({ "pointer": pointer }),
            Command::CreateMaterial {
                id,
                name,
                shader,
                path,
            } => {
                json!({ "id": id_or_null(id), "name": name, "shader": shader, "path": path.display().to_string() })
            }
            Command::SaveDocument { path } => {
                json!({ "path": path.as_ref().map(|p| p.display().to_string()) })
            }
        };
        if let Some(o) = v.as_object_mut() {
            o.insert("command".into(), J::String(self.name().into()));
            // `command` first.
            let mut ordered = serde_json::Map::new();
            ordered.insert("command".into(), J::String(self.name().into()));
            for (k, val) in o.iter() {
                if k != "command" {
                    ordered.insert(k.clone(), val.clone());
                }
            }
            *o = ordered;
        }
        v
    }
}

fn component_spec(v: &J) -> Result<ComponentSpec, String> {
    let o = v
        .as_object()
        .ok_or("must be { id?, type, version?, data? }")?;
    let type_id = o
        .get("type")
        .and_then(J::as_str)
        .ok_or("\"type\" must be a string")?
        .to_string();
    let id = match o.get("id") {
        None | Some(J::Null) => None,
        Some(J::String(s)) => Some(parse_uuid(s).map_err(|w| format!("\"id\": {w}"))?),
        Some(_) => return Err("\"id\" must be a UUID string".into()),
    };
    let version = match o.get("version") {
        None | Some(J::Null) => None,
        Some(J::Number(n)) if n.as_u64().is_some_and(|n| n >= 1) => n.as_u64().map(|n| n as u32),
        Some(_) => return Err("\"version\" must be a positive integer".into()),
    };
    let data = match o.get("data") {
        None => None,
        Some(J::Null) => None,
        Some(d) if d.is_object() => Some(Node::from_serde(d)),
        Some(_) => return Err("\"data\" must be an object".into()),
    };
    Ok(ComponentSpec {
        id,
        type_id,
        version,
        data,
    })
}

fn spec_json(c: &ComponentSpec) -> J {
    json!({
        "id": c.id.map(|u| u.to_string()),
        "type": c.type_id,
        "version": c.version,
        "data": c.data.as_ref().map(Node::to_serde),
    })
}

fn asset_json(a: &AssetRef) -> J {
    if a.sub.is_empty() {
        json!({ "asset": a.asset.to_string() })
    } else {
        json!({ "asset": a.asset.to_string(), "sub": a.sub })
    }
}

// ------------------------------------------------------------------ edits

/// A primitive, exactly reversible change to the JSON tree. Commands
/// compile to these; undo data is their inverses.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    /// Replaces the node at `pointer` (which must exist).
    Replace {
        pointer: String,
        value: Node,
    },
    /// Inserts an object member at `index` of the object at `parent`.
    InsertMember {
        parent: String,
        index: usize,
        key: String,
        value: Node,
    },
    RemoveMember {
        parent: String,
        key: String,
    },
    /// Inserts an array item at `index` of the array at `parent`.
    InsertItem {
        parent: String,
        index: usize,
        value: Node,
    },
    RemoveItem {
        parent: String,
        index: usize,
    },
}

impl Edit {
    /// Applies the edit and returns its inverse. `Err` leaves the tree as it
    /// was (the message names what was missing).
    pub fn apply(&self, root: &mut Node) -> Result<Edit, String> {
        match self {
            Edit::Replace { pointer, value } => {
                let target = root
                    .pointer_mut(pointer)
                    .ok_or_else(|| format!("no node at {pointer:?}"))?;
                let previous = std::mem::replace(target, value.clone());
                Ok(Edit::Replace {
                    pointer: pointer.clone(),
                    value: previous,
                })
            }
            Edit::InsertMember {
                parent,
                index,
                key,
                value,
            } => {
                let members = object_at(root, parent)?;
                if members.iter().any(|m| &m.key == key) {
                    return Err(format!("{parent:?} already has member {key:?}"));
                }
                let index = (*index).min(members.len());
                members.insert(
                    index,
                    Member {
                        key: key.clone(),
                        key_span: Span::default(),
                        value: value.clone(),
                    },
                );
                Ok(Edit::RemoveMember {
                    parent: parent.clone(),
                    key: key.clone(),
                })
            }
            Edit::RemoveMember { parent, key } => {
                let members = object_at(root, parent)?;
                let index = members
                    .iter()
                    .position(|m| &m.key == key)
                    .ok_or_else(|| format!("{parent:?} has no member {key:?}"))?;
                let m = members.remove(index);
                Ok(Edit::InsertMember {
                    parent: parent.clone(),
                    index,
                    key: m.key,
                    value: m.value,
                })
            }
            Edit::InsertItem {
                parent,
                index,
                value,
            } => {
                let items = array_at(root, parent)?;
                let index = (*index).min(items.len());
                items.insert(index, value.clone());
                Ok(Edit::RemoveItem {
                    parent: parent.clone(),
                    index,
                })
            }
            Edit::RemoveItem { parent, index } => {
                let items = array_at(root, parent)?;
                if *index >= items.len() {
                    return Err(format!("{parent:?} has no item {index}"));
                }
                let value = items.remove(*index);
                Ok(Edit::InsertItem {
                    parent: parent.clone(),
                    index: *index,
                    value,
                })
            }
        }
    }

    pub fn to_json(&self) -> J {
        match self {
            Edit::Replace { pointer, value } => {
                json!({ "op": "replace", "pointer": pointer, "value": value.to_serde() })
            }
            Edit::InsertMember {
                parent,
                index,
                key,
                value,
            } => {
                json!({ "op": "insertMember", "parent": parent, "index": index, "key": key, "value": value.to_serde() })
            }
            Edit::RemoveMember { parent, key } => {
                json!({ "op": "removeMember", "parent": parent, "key": key })
            }
            Edit::InsertItem {
                parent,
                index,
                value,
            } => {
                json!({ "op": "insertItem", "parent": parent, "index": index, "value": value.to_serde() })
            }
            Edit::RemoveItem { parent, index } => {
                json!({ "op": "removeItem", "parent": parent, "index": index })
            }
        }
    }
}

fn object_at<'a>(root: &'a mut Node, pointer: &str) -> Result<&'a mut Vec<Member>, String> {
    root.pointer_mut(pointer)
        .ok_or_else(|| format!("no node at {pointer:?}"))?
        .as_object_mut()
        .ok_or_else(|| format!("{pointer:?} is not an object"))
}

fn array_at<'a>(root: &'a mut Node, pointer: &str) -> Result<&'a mut Vec<Node>, String> {
    match &mut root
        .pointer_mut(pointer)
        .ok_or_else(|| format!("no node at {pointer:?}"))?
        .value
    {
        Value::Array(items) => Ok(items),
        _ => Err(format!("{pointer:?} is not an array")),
    }
}

/// Applies `edits` in order; on failure, undoes the ones applied and returns
/// the failing edit's index and message.
fn apply_all(root: &mut Node, edits: &[Edit]) -> Result<Vec<Edit>, (usize, String)> {
    let mut inverse = Vec::with_capacity(edits.len());
    for (i, e) in edits.iter().enumerate() {
        match e.apply(root) {
            Ok(inv) => inverse.push(inv),
            Err(msg) => {
                rollback(root, &inverse);
                return Err((i, msg));
            }
        }
    }
    Ok(inverse)
}

/// Applies inverses in reverse order. They were produced by successful
/// forward edits, so they cannot fail; a failure would mean the tree was
/// changed behind our back and is reported as a panic in debug builds.
fn rollback(root: &mut Node, inverse: &[Edit]) {
    for e in inverse.iter().rev() {
        let r = e.apply(root);
        debug_assert!(r.is_ok(), "rollback failed: {r:?}");
    }
}

// ------------------------------------------------------------------ results

/// One command's outcome inside a transaction.
#[derive(Clone, Debug, PartialEq)]
pub struct Mutation {
    pub command_id: Uuid,
    pub command: &'static str,
    pub transaction_id: Uuid,
    pub base_revision: Revision,
    /// Entities, components and documents the command touched or created.
    pub affected_ids: Vec<Uuid>,
    pub diagnostics: Vec<Diagnostic>,
    /// The inverses of this command's edits, in the order to apply them
    /// (already reversed). Empty for side-effect commands.
    pub undo: Vec<Edit>,
    /// The forward edits (redo data).
    pub edits: Vec<Edit>,
}

impl Mutation {
    pub fn json(&self) -> J {
        json!({
            "commandId": self.command_id.to_string(),
            "command": self.command,
            "transactionId": self.transaction_id.to_string(),
            "baseRevision": self.base_revision.as_str(),
            "affectedIds": self.affected_ids.iter().map(|u| u.to_string()).collect::<Vec<_>>(),
            "ok": !has_errors(&self.diagnostics),
            "diagnostics": self.diagnostics.iter().map(diagnostic_json).collect::<Vec<_>>(),
            "undo": self.undo.iter().map(Edit::to_json).collect::<Vec<_>>(),
        })
    }
}

/// A committed transaction: the history unit for undo/redo.
#[derive(Clone, Debug, PartialEq)]
pub struct Transaction {
    pub id: Uuid,
    pub base_revision: Revision,
    /// The document's revision after the transaction.
    pub revision: Revision,
    pub mutations: Vec<Mutation>,
}

impl Transaction {
    fn edits(&self) -> Vec<Edit> {
        self.mutations
            .iter()
            .flat_map(|m| m.edits.clone())
            .collect()
    }

    fn inverse(&self) -> Vec<Edit> {
        self.mutations
            .iter()
            .rev()
            .flat_map(|m| m.undo.clone())
            .collect()
    }
}

/// What [`Editor::apply`] returns: the transaction when it committed, the
/// mutations up to the failure and why when it did not.
#[derive(Clone, Debug, PartialEq)]
pub struct Applied {
    pub ok: bool,
    pub transaction_id: Uuid,
    pub base_revision: Revision,
    /// The revision now current: new when `ok`, unchanged otherwise.
    pub revision: Revision,
    pub mutations: Vec<Mutation>,
    /// Transaction-level diagnostics (conflict, validation rejection).
    pub diagnostics: Vec<Diagnostic>,
}

impl Applied {
    pub fn all_diagnostics(&self) -> Vec<Diagnostic> {
        let mut out = self.diagnostics.clone();
        for m in &self.mutations {
            out.extend(m.diagnostics.iter().cloned());
        }
        out
    }

    pub fn json(&self) -> J {
        json!({
            "ok": self.ok,
            "transactionId": self.transaction_id.to_string(),
            "baseRevision": self.base_revision.as_str(),
            "revision": self.revision.as_str(),
            "mutations": self.mutations.iter().map(Mutation::json).collect::<Vec<_>>(),
            "diagnostics": self.diagnostics.iter().map(diagnostic_json).collect::<Vec<_>>(),
        })
    }
}

/// A transaction to apply.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Request {
    /// New when `None`.
    pub id: Option<Uuid>,
    /// Must equal the editor's current revision when given.
    pub base_revision: Option<Revision>,
    pub commands: Vec<Requested>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Requested {
    /// New when `None`.
    pub id: Option<Uuid>,
    pub command: Command,
}

impl From<Command> for Requested {
    fn from(command: Command) -> Self {
        Requested { id: None, command }
    }
}

impl Request {
    pub fn new(commands: Vec<Command>) -> Request {
        Request {
            id: None,
            base_revision: None,
            commands: commands.into_iter().map(Requested::from).collect(),
        }
    }

    /// Reads a patch: either an array of commands or
    /// `{ "transactionId"?, "baseRevision"?, "commands": [...] }`; each
    /// command may carry a `"commandId"`.
    pub fn from_json(v: &J) -> Result<Request, Vec<Diagnostic>> {
        let invalid = |msg: &str| vec![Diagnostic::error(Code::EDIT_INVALID_COMMAND, msg)];
        let (id, base_revision, list) = match v {
            J::Array(list) => (None, None, list),
            J::Object(o) => {
                let id = match o.get("transactionId") {
                    None | Some(J::Null) => None,
                    Some(J::String(s)) => {
                        Some(parse_uuid(s).map_err(|w| invalid(&format!("transactionId: {w}")))?)
                    }
                    Some(_) => return Err(invalid("transactionId must be a UUID string")),
                };
                let base =
                    match o.get("baseRevision") {
                        None | Some(J::Null) => None,
                        Some(J::String(s)) => Some(Revision::parse(s).ok_or_else(|| {
                            invalid("baseRevision must be a 64-hex-digit revision")
                        })?),
                        Some(_) => return Err(invalid("baseRevision must be a string")),
                    };
                let list = o
                    .get("commands")
                    .and_then(J::as_array)
                    .ok_or_else(|| invalid("\"commands\" must be an array"))?;
                (id, base, list)
            }
            _ => {
                return Err(invalid(
                    "a patch is an array of commands or { commands: [...] }",
                ))
            }
        };
        let mut commands = Vec::with_capacity(list.len());
        let mut errors = Vec::new();
        for (i, c) in list.iter().enumerate() {
            let cid = match c.get("commandId") {
                None | Some(J::Null) => None,
                Some(J::String(s)) => match parse_uuid(s) {
                    Ok(u) => Some(u),
                    Err(w) => {
                        errors.push(
                            Diagnostic::error(
                                Code::EDIT_INVALID_COMMAND,
                                format!("commandId: {w}"),
                            )
                            .with_pointer(format!("/commands/{i}/commandId")),
                        );
                        None
                    }
                },
                Some(_) => {
                    errors.push(
                        Diagnostic::error(
                            Code::EDIT_INVALID_COMMAND,
                            "commandId must be a UUID string",
                        )
                        .with_pointer(format!("/commands/{i}/commandId")),
                    );
                    None
                }
            };
            match Command::from_json(c) {
                Ok(command) => commands.push(Requested { id: cid, command }),
                Err(d) => errors.push(d.with_pointer(format!("/commands/{i}"))),
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(Request {
            id,
            base_revision,
            commands,
        })
    }
}

// ------------------------------------------------------------------ editor

/// One open scene, prefab or material document with its revision and
/// history.
#[derive(Clone, Debug)]
pub struct Editor {
    document: Document,
    revision: Revision,
    /// The shaders a material document may name (built-ins unless the
    /// caller added the project's manifests with [`Editor::with_shaders`]).
    shaders: ShaderLibrary,
    /// Errors the document had when opened (tolerated: a transaction is
    /// rejected only for errors it introduces).
    baseline: HashSet<ErrorKey>,
    undo_stack: Vec<Transaction>,
    redo_stack: Vec<Transaction>,
    /// Revision last saved (or opened).
    saved_revision: Revision,
}

type ErrorKey = (String, Option<String>, String);

fn error_keys(diags: &[Diagnostic]) -> HashSet<ErrorKey> {
    diags
        .iter()
        .filter(|d| d.is_error())
        .map(|d| (d.code.0.to_string(), d.pointer.clone(), d.message.clone()))
        .collect()
}

impl Editor {
    /// Opens a scene, prefab or material document. Read-only (newer) and
    /// outdated documents and other kinds are refused (`FYN-E0007`); the
    /// document's current errors, if any, are tolerated afterwards (a
    /// transaction is rejected only for errors it introduces). Materials
    /// are validated against the built-in shaders until
    /// [`Editor::with_shaders`] adds a project's manifests.
    pub fn open(document: Document, schemas: &SchemaSet) -> Result<Editor, Vec<Diagnostic>> {
        Editor::open_with(document, schemas, ShaderLibrary::builtin())
    }

    /// [`Editor::open`] with the shaders a material may name.
    pub fn open_with(
        document: Document,
        schemas: &SchemaSet,
        shaders: ShaderLibrary,
    ) -> Result<Editor, Vec<Diagnostic>> {
        let not_editable = |msg: String| {
            let mut d = Diagnostic::error(Code::EDIT_NOT_EDITABLE, msg);
            if let Some(p) = &document.path {
                d = d.with_document(p.clone());
            }
            vec![d]
        };
        if !matches!(
            document.kind,
            DocumentKind::Scene | DocumentKind::Prefab | DocumentKind::Material
        ) {
            return Err(not_editable(format!(
                "{} documents are not edited by commands (scenes, prefabs and materials are)",
                document.kind.name()
            )));
        }
        if document.is_read_only() {
            return Err(not_editable(format!(
                "formatVersion {} is newer than the supported {}",
                document.envelope.format_version,
                document.kind.supported_format_version()
            )));
        }
        if document.is_outdated() {
            return Err(not_editable(format!(
                "formatVersion {} is older than the supported {}; run `fynkel migrate` first",
                document.envelope.format_version,
                document.kind.supported_format_version()
            )));
        }
        let revision = Revision::of(&document).map_err(|d| vec![d])?;
        let baseline = error_keys(&validate(&document, schemas, &shaders));
        Ok(Editor {
            document,
            saved_revision: revision.clone(),
            revision,
            shaders,
            baseline,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        })
    }

    /// Replaces the shaders a material document is validated against
    /// (the baseline of tolerated errors is recomputed).
    pub fn with_shaders(mut self, shaders: ShaderLibrary, schemas: &SchemaSet) -> Editor {
        self.shaders = shaders;
        self.baseline = error_keys(&validate(&self.document, schemas, &self.shaders));
        self
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    /// The document's file moved (the session renamed or moved the asset):
    /// `save(None)` writes there from now on and diagnostics name it.
    /// Nothing else changes — not the revision, not the history.
    pub fn relocate(&mut self, path: &Path) {
        self.document.path = Some(path.display().to_string());
    }

    pub fn into_document(self) -> Document {
        self.document
    }

    pub fn revision(&self) -> &Revision {
        &self.revision
    }

    /// True when the document differs from what was last opened or saved.
    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    pub fn history(&self) -> &[Transaction] {
        &self.undo_stack
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// A fresh typed view (scene documents; a prefab reads as a scene too;
    /// a material has no entities and reads as an empty scene).
    pub fn scene(&self) -> Scene {
        Scene::from_document(self.document.clone())
    }

    /// The document's validation now (baseline errors included).
    pub fn validate(&self, schemas: &SchemaSet) -> Vec<Diagnostic> {
        validate(&self.document, schemas, &self.shaders)
    }

    /// Applies one transaction atomically. See the module docs.
    pub fn apply(&mut self, request: Request, schemas: &SchemaSet) -> Applied {
        self.apply_with_prefabs(request, schemas, &PrefabLibrary::empty())
    }

    /// [`Editor::apply`] with the prefabs the document can see: only
    /// `unpackPrefab` reads them (an empty library makes every unpack
    /// fail with `D0401`); the other commands never look.
    pub fn apply_with_prefabs(
        &mut self,
        request: Request,
        schemas: &SchemaSet,
        prefabs: &PrefabLibrary,
    ) -> Applied {
        let transaction_id = request.id.unwrap_or_else(Uuid::new_v4);
        let base_revision = self.revision.clone();
        let mut out = Applied {
            ok: false,
            transaction_id,
            base_revision: base_revision.clone(),
            revision: base_revision.clone(),
            mutations: Vec::new(),
            diagnostics: Vec::new(),
        };
        if let Some(base) = &request.base_revision {
            if base != &self.revision {
                out.diagnostics.push(
                    self.diag(Diagnostic::error(
                        Code::EDIT_CONFLICT,
                        format!(
                            "base revision {} is stale; the document is at {}",
                            short(base),
                            short(&self.revision)
                        ),
                    ))
                    .with_suggestion("re-read the document and rebase the transaction"),
                );
                return out;
            }
        }

        let mut applied_inverse: Vec<Edit> = Vec::new();
        let mut side_effects: Vec<(usize, SideEffect)> = Vec::new();
        for (i, r) in request.commands.iter().enumerate() {
            let mut m = Mutation {
                command_id: r.id.unwrap_or_else(Uuid::new_v4),
                command: r.command.name(),
                transaction_id,
                base_revision: base_revision.clone(),
                affected_ids: Vec::new(),
                diagnostics: Vec::new(),
                undo: Vec::new(),
                edits: Vec::new(),
            };
            let outcome = {
                let mut cx = Cx {
                    root: &mut self.document.root,
                    kind: self.document.kind,
                    envelope: &self.document.envelope,
                    prefabs,
                    schemas,
                    edits: Vec::new(),
                };
                let res = cx.run(&r.command);
                (res, cx.edits)
            };
            match outcome {
                (Ok(Planned { affected, effect }), edits) => {
                    match apply_all(&mut self.document.root, &edits) {
                        Ok(inverse) => {
                            m.affected_ids = affected;
                            m.undo = inverse.iter().rev().cloned().collect();
                            m.edits = edits;
                            applied_inverse.extend(inverse);
                            if let Some(e) = effect {
                                side_effects.push((i, e));
                            }
                        }
                        Err((at, why)) => {
                            m.diagnostics.push(self.diag(Diagnostic::error(
                                Code::EDIT_INVALID_COMMAND,
                                format!("{}: edit {at} cannot be applied: {why}", r.command.name()),
                            )));
                            out.mutations.push(m);
                            rollback(&mut self.document.root, &applied_inverse);
                            return out;
                        }
                    }
                }
                (Err(d), _) => {
                    m.diagnostics.push(self.diag(d));
                    out.mutations.push(m);
                    rollback(&mut self.document.root, &applied_inverse);
                    return out;
                }
            }
            out.mutations.push(m);
        }

        // Validation: errors this transaction introduces reject it whole.
        let diags = validate(&self.document, schemas, &self.shaders);
        let new_errors: Vec<Diagnostic> = diags
            .iter()
            .filter(|d| d.is_error())
            .filter(|d| {
                !self.baseline.contains(&(
                    d.code.0.to_string(),
                    d.pointer.clone(),
                    d.message.clone(),
                ))
            })
            .cloned()
            .collect();
        if !new_errors.is_empty() {
            let mut d = self.diag(Diagnostic::error(
                Code::EDIT_REJECTED,
                format!(
                    "transaction rejected: it would leave the document with {} error(s); nothing was applied",
                    new_errors.len()
                ),
            ));
            for e in new_errors {
                d = d.with_related(e);
            }
            out.diagnostics.push(d);
            rollback(&mut self.document.root, &applied_inverse);
            return out;
        }
        // Warnings are reported (all of them: they are cheap to repeat), never fatal.
        out.diagnostics
            .extend(diags.iter().filter(|d| !d.is_error()).cloned());

        // Side effects, after the document is known good.
        for (i, e) in side_effects {
            match e.perform(&self.document) {
                Ok(effect_diags) => {
                    if let SideEffect::Save { .. } = e {
                        self.saved_revision =
                            Revision::of(&self.document).unwrap_or_else(|_| self.revision.clone());
                    }
                    out.mutations[i].diagnostics.extend(effect_diags);
                }
                Err(d) => {
                    out.mutations[i].diagnostics.push(self.diag(d));
                    rollback(&mut self.document.root, &applied_inverse);
                    return out;
                }
            }
        }

        self.revision = Revision::of(&self.document).unwrap_or_else(|_| self.revision.clone());
        out.ok = true;
        out.revision = self.revision.clone();
        self.undo_stack.push(Transaction {
            id: transaction_id,
            base_revision,
            revision: self.revision.clone(),
            mutations: out.mutations.clone(),
        });
        self.redo_stack.clear();
        out
    }

    /// Reverts the last committed transaction. Returns it (its `revision`
    /// is the one the document now left).
    pub fn undo(&mut self) -> Result<Transaction, Diagnostic> {
        let Some(t) = self.undo_stack.pop() else {
            return Err(self.diag(Diagnostic::error(Code::EDIT_NO_HISTORY, "nothing to undo")));
        };
        rollback_checked(&mut self.document.root, &t.inverse());
        self.revision = Revision::of(&self.document).unwrap_or_else(|_| t.base_revision.clone());
        self.redo_stack.push(t.clone());
        Ok(t)
    }

    /// Re-applies the last undone transaction.
    pub fn redo(&mut self) -> Result<Transaction, Diagnostic> {
        let Some(t) = self.redo_stack.pop() else {
            return Err(self.diag(Diagnostic::error(Code::EDIT_NO_HISTORY, "nothing to redo")));
        };
        rollback_checked(&mut self.document.root, &t.edits());
        self.revision = Revision::of(&self.document).unwrap_or_else(|_| t.revision.clone());
        self.undo_stack.push(t.clone());
        Ok(t)
    }

    /// Writes the document to `path` or where it was read from.
    pub fn save(&mut self, path: Option<&Path>) -> Result<PathBuf, Diagnostic> {
        SideEffect::Save {
            path: path.map(Path::to_path_buf),
        }
        .perform(&self.document)?;
        self.saved_revision = self.revision.clone();
        save_target(&self.document, path)
    }

    fn diag(&self, d: Diagnostic) -> Diagnostic {
        match (&d.document, &self.document.path) {
            (None, Some(p)) => d.with_document(p.clone()),
            _ => d,
        }
    }
}

/// Edits already proven to apply (they are a committed transaction's
/// inverses or forward edits).
fn rollback_checked(root: &mut Node, edits: &[Edit]) {
    for e in edits {
        let r = e.apply(root);
        debug_assert!(r.is_ok(), "history replay failed: {r:?}");
    }
}

fn short(r: &Revision) -> String {
    r.as_str()[..12].to_string()
}

fn validate(document: &Document, schemas: &SchemaSet, shaders: &ShaderLibrary) -> Vec<Diagnostic> {
    match document.kind {
        DocumentKind::Prefab => Prefab::from_document(document.clone()).validate(schemas),
        DocumentKind::Material => {
            Material::from_document(document.clone()).validate(schemas, shaders)
        }
        _ => Scene::from_document(document.clone()).validate(schemas),
    }
}

fn save_target(document: &Document, path: Option<&Path>) -> Result<PathBuf, Diagnostic> {
    path.map(Path::to_path_buf)
        .or_else(|| document.path.as_ref().map(PathBuf::from))
        .ok_or_else(|| {
            Diagnostic::error(
                Code::EDIT_IO,
                "the document has no path; saveDocument needs \"path\"",
            )
        })
}

// ------------------------------------------------------------------ execution

/// What a command produced besides edits.
struct Planned {
    affected: Vec<Uuid>,
    effect: Option<SideEffect>,
}

enum SideEffect {
    Save { path: Option<PathBuf> },
    CreateMaterial { document: Document, path: PathBuf },
}

impl SideEffect {
    fn perform(&self, current: &Document) -> Result<Vec<Diagnostic>, Diagnostic> {
        match self {
            SideEffect::Save { path } => {
                let target = save_target(current, path.as_deref())?;
                current.save(&target).map_err(|d| {
                    Diagnostic::error(Code::EDIT_IO, d.message)
                        .with_document(target.display().to_string())
                })?;
                Ok(Vec::new())
            }
            SideEffect::CreateMaterial { document, path } => {
                if path.exists() {
                    return Err(Diagnostic::error(
                        Code::EDIT_IO,
                        format!("{} exists; createMaterial never overwrites", path.display()),
                    )
                    .with_document(path.display().to_string()));
                }
                if let Some(dir) = path.parent() {
                    if !dir.as_os_str().is_empty() {
                        std::fs::create_dir_all(dir).map_err(|e| {
                            Diagnostic::error(
                                Code::EDIT_IO,
                                format!("cannot create {}: {e}", dir.display()),
                            )
                        })?;
                    }
                }
                document.save(path).map_err(|d| {
                    Diagnostic::error(Code::EDIT_IO, d.message)
                        .with_document(path.display().to_string())
                })?;
                Ok(Vec::new())
            }
        }
    }
}

/// Command execution context: plans edits against the current tree
/// (nothing is applied until the command's edits are all known).
struct Cx<'a> {
    root: &'a mut Node,
    kind: DocumentKind,
    envelope: &'a Envelope,
    prefabs: &'a PrefabLibrary,
    schemas: &'a SchemaSet,
    edits: Vec<Edit>,
}

impl Cx<'_> {
    fn run(&mut self, command: &Command) -> Result<Planned, Diagnostic> {
        let effect = None;
        let entity_command = !matches!(
            command,
            Command::SetDocumentField { .. }
                | Command::UnsetDocumentField { .. }
                | Command::CreateMaterial { .. }
                | Command::SaveDocument { .. }
        );
        if entity_command && self.kind == DocumentKind::Material {
            return Err(Diagnostic::error(
                Code::EDIT_NOT_EDITABLE,
                format!(
                    "{}: material documents have no entities; use setDocumentField",
                    command.name()
                ),
            ));
        }
        let affected = match command {
            Command::CreateObject {
                id,
                name,
                parent,
                index,
                components,
            } => self.create_object(*id, name, *parent, *index, components, None)?,
            Command::DeleteObject { id } => self.delete_object(*id)?,
            Command::RenameObject { id, name } => {
                let p = self.entity_pointer(*id)?;
                self.set_member(&p, "name", Node::string(name.clone()));
                vec![*id]
            }
            Command::ReparentObject { id, parent, index } => self.reparent(*id, *parent, *index)?,
            Command::AddComponent { entity, component } => {
                let p = self.entity_pointer(*entity)?;
                let cid = self.add_component(&p, component)?;
                vec![*entity, cid]
            }
            Command::RemoveComponent { entity, component } => {
                let p = self.component_pointer(*entity, *component)?;
                let (parent, key) = split_pointer(&p);
                self.edits.push(Edit::RemoveMember { parent, key });
                vec![*entity, *component]
            }
            Command::SetField {
                entity,
                component,
                field,
                value,
            } => {
                self.set_field(*entity, *component, field, value.clone())?;
                vec![*entity, *component]
            }
            Command::UnsetField {
                entity,
                component,
                field,
            } => {
                let p = self.data_pointer(*entity, *component)?;
                let target = field_pointer(&p, field);
                if self.root.pointer(&target).is_none() {
                    return Err(Diagnostic::error(
                        Code::EDIT_INVALID_COMMAND,
                        format!("unsetField: {field:?} is not set"),
                    )
                    .with_pointer(target)
                    .with_id(component.to_string()));
                }
                let (parent, key) = split_pointer(&target);
                match self.root.pointer(&parent).map(|n| &n.value) {
                    Some(Value::Object(_)) => self.edits.push(Edit::RemoveMember { parent, key }),
                    Some(Value::Array(_)) => self.edits.push(Edit::RemoveItem {
                        parent,
                        index: key.parse().unwrap_or(usize::MAX),
                    }),
                    _ => unreachable!("target exists under a container"),
                }
                vec![*entity, *component]
            }
            Command::AssignAsset {
                entity,
                component,
                field,
                asset,
            } => {
                let mut members =
                    vec![("asset".to_string(), Node::string(asset.asset.to_string()))];
                if !asset.sub.is_empty() {
                    members.push(("sub".to_string(), Node::string(asset.sub.clone())));
                }
                self.set_field(*entity, *component, field, Node::object(members))?;
                vec![*entity, *component]
            }
            Command::InstantiatePrefab {
                id,
                name,
                parent,
                index,
                prefab,
                instance,
                components,
            } => {
                if self.kind != DocumentKind::Scene {
                    return Err(Diagnostic::error(
                        Code::EDIT_NOT_EDITABLE,
                        "instantiatePrefab: prefab instances live in scene documents only",
                    ));
                }
                let instance = instance.unwrap_or_else(Uuid::new_v4);
                let mut asset = vec![("asset".to_string(), Node::string(prefab.asset.to_string()))];
                if !prefab.sub.is_empty() {
                    asset.push(("sub".to_string(), Node::string(prefab.sub.clone())));
                }
                let prefab_node = Node::object(vec![
                    ("asset".to_string(), Node::object(asset)),
                    ("instance".to_string(), Node::string(instance.to_string())),
                    (
                        "overrides".to_string(),
                        Node::synthetic(Value::Array(Vec::new())),
                    ),
                ]);
                let mut affected =
                    self.create_object(*id, name, *parent, *index, components, Some(prefab_node))?;
                affected.push(instance);
                affected
            }
            Command::SetPrefabOverride { entity, r#override } => {
                self.set_override(*entity, r#override)?
            }
            Command::RevertPrefabOverride { entity, index } => {
                let p = self.overrides_pointer(*entity)?;
                let count = self
                    .root
                    .pointer(&p)
                    .and_then(Node::as_array)
                    .map(|a| a.len())
                    .unwrap_or(0);
                match index {
                    Some(i) if *i >= count => {
                        return Err(Diagnostic::error(
                            Code::EDIT_NOT_AN_INSTANCE,
                            format!(
                                "revertPrefabOverride: no override {i} (the instance has {count})"
                            ),
                        )
                        .with_pointer(p)
                        .with_id(entity.to_string()));
                    }
                    Some(i) => self.edits.push(Edit::RemoveItem {
                        parent: p,
                        index: *i,
                    }),
                    None => {
                        for i in (0..count).rev() {
                            self.edits.push(Edit::RemoveItem {
                                parent: p.clone(),
                                index: i,
                            });
                        }
                    }
                }
                vec![*entity]
            }
            Command::UnpackPrefab { entity } => self.unpack(*entity)?,
            Command::DuplicateObject {
                id,
                new_id,
                name,
                index,
            } => self.duplicate(*id, *new_id, name.as_deref(), *index)?,
            Command::SetDocumentField { pointer, value } => {
                self.set_document_field(pointer, value.clone())?;
                Vec::new()
            }
            Command::UnsetDocumentField { pointer } => {
                self.unset_document_field(pointer)?;
                Vec::new()
            }
            Command::CreateMaterial {
                id,
                name,
                shader,
                path,
            } => {
                let mut doc = Document::new(DocumentKind::Material, name);
                if let Some(id) = id {
                    doc.envelope.id = *id;
                    doc.root.set("id", Node::string(id.to_string()));
                }
                if let Some(s) = shader {
                    doc.root.set("shader", Node::string(s.clone()));
                    if s != crate::material::PBR_SHADER_ID {
                        crate::material::strip_pbr_fields(&mut doc.root);
                    }
                }
                let material = Material::from_document(doc.clone());
                let diags = material.validate_semantics(&ShaderLibrary::builtin());
                if has_errors(&diags) {
                    let mut d = Diagnostic::error(
                        Code::EDIT_INVALID_COMMAND,
                        format!("createMaterial: {:?} is not a valid material", name),
                    );
                    for e in diags {
                        d = d.with_related(e);
                    }
                    return Err(d);
                }
                return Ok(Planned {
                    affected: vec![doc.envelope.id],
                    effect: Some(SideEffect::CreateMaterial {
                        document: doc,
                        path: path.clone(),
                    }),
                });
            }
            Command::SaveDocument { path } => {
                return Ok(Planned {
                    affected: Vec::new(),
                    effect: Some(SideEffect::Save { path: path.clone() }),
                })
            }
        };
        Ok(Planned { affected, effect })
    }

    // ---- lookups

    fn entity_pointer(&self, id: Uuid) -> Result<String, Diagnostic> {
        let p = format!("/entities/{id}");
        if self.root.pointer(&p).and_then(Node::as_object).is_some() {
            Ok(p)
        } else {
            Err(
                Diagnostic::error(Code::EDIT_UNKNOWN_ENTITY, format!("no entity {id}"))
                    .with_id(id.to_string()),
            )
        }
    }

    fn component_pointer(&self, entity: Uuid, component: Uuid) -> Result<String, Diagnostic> {
        let e = self.entity_pointer(entity)?;
        let p = format!("{e}/components/{component}");
        if self.root.pointer(&p).and_then(Node::as_object).is_some() {
            Ok(p)
        } else {
            Err(Diagnostic::error(
                Code::EDIT_UNKNOWN_COMPONENT,
                format!("entity {entity} has no component {component}"),
            )
            .with_pointer(format!("{e}/components"))
            .with_id(component.to_string()))
        }
    }

    /// The component's `data` pointer, creating the member when absent.
    fn data_pointer(&mut self, entity: Uuid, component: Uuid) -> Result<String, Diagnostic> {
        let c = self.component_pointer(entity, component)?;
        let data = format!("{c}/data");
        if self.root.pointer(&data).is_none() {
            self.edits.push(Edit::InsertMember {
                parent: c,
                index: usize::MAX,
                key: "data".into(),
                value: Node::object(Vec::new()),
            });
        }
        Ok(data)
    }

    fn overrides_pointer(&mut self, entity: Uuid) -> Result<String, Diagnostic> {
        let e = self.entity_pointer(entity)?;
        let prefab = format!("{e}/prefab");
        if self
            .root
            .pointer(&prefab)
            .and_then(Node::as_object)
            .is_none()
        {
            return Err(Diagnostic::error(
                Code::EDIT_NOT_AN_INSTANCE,
                format!("entity {entity} is not a prefab instance"),
            )
            .with_pointer(e)
            .with_id(entity.to_string()));
        }
        let p = format!("{prefab}/overrides");
        if self.root.pointer(&p).and_then(Node::as_array).is_none() {
            if self.root.pointer(&p).is_some() {
                self.edits.push(Edit::Replace {
                    pointer: p.clone(),
                    value: Node::synthetic(Value::Array(Vec::new())),
                });
            } else {
                self.edits.push(Edit::InsertMember {
                    parent: prefab,
                    index: usize::MAX,
                    key: "overrides".into(),
                    value: Node::synthetic(Value::Array(Vec::new())),
                });
            }
        }
        Ok(p)
    }

    // ---- primitives

    /// Replace-or-append an object member.
    fn set_member(&mut self, object: &str, key: &str, value: Node) {
        let p = format!("{object}/{}", escape(key));
        if self.root.pointer(&p).is_some() {
            self.edits.push(Edit::Replace { pointer: p, value });
        } else {
            self.edits.push(Edit::InsertMember {
                parent: object.to_string(),
                index: usize::MAX,
                key: key.to_string(),
                value,
            });
        }
    }

    fn set_field(
        &mut self,
        entity: Uuid,
        component: Uuid,
        field: &str,
        value: Node,
    ) -> Result<(), Diagnostic> {
        let data = self.data_pointer(entity, component)?;
        let target = field_pointer(&data, field);
        if self.root.pointer(&target).is_some() {
            self.edits.push(Edit::Replace {
                pointer: target,
                value,
            });
            return Ok(());
        }
        let (parent, key) = split_pointer(&target);
        let planned_parent = parent == data && self.root.pointer(&data).is_none();
        match self.root.pointer(&parent).map(|n| &n.value) {
            Some(Value::Object(_)) => self.edits.push(Edit::InsertMember {
                parent,
                index: usize::MAX,
                key,
                value,
            }),
            Some(Value::Array(items)) if key.parse::<usize>().ok() == Some(items.len()) => {
                self.edits.push(Edit::InsertItem {
                    parent,
                    index: items.len(),
                    value,
                })
            }
            None if planned_parent => self.edits.push(Edit::InsertMember {
                parent,
                index: usize::MAX,
                key,
                value,
            }),
            _ => {
                return Err(Diagnostic::error(
                    Code::EDIT_INVALID_COMMAND,
                    format!(
                    "setField: cannot set {field:?}: {parent:?} is not an object or an array end"
                ),
                )
                .with_pointer(target)
                .with_id(component.to_string()))
            }
        }
        Ok(())
    }

    /// Adds a component member; returns its id.
    fn add_component(
        &mut self,
        entity_pointer: &str,
        spec: &ComponentSpec,
    ) -> Result<Uuid, Diagnostic> {
        let cid = spec.id.unwrap_or_else(Uuid::new_v4);
        let components = format!("{entity_pointer}/components");
        if self
            .root
            .pointer(&components)
            .and_then(Node::as_object)
            .is_none()
        {
            self.set_member(entity_pointer, "components", Node::object(Vec::new()));
        } else if self.root.pointer(&format!("{components}/{cid}")).is_some() {
            return Err(Diagnostic::error(
                Code::EDIT_DUPLICATE_ID,
                format!("component {cid} already exists"),
            )
            .with_pointer(components)
            .with_id(cid.to_string()));
        }
        self.edits.push(Edit::InsertMember {
            parent: components,
            index: usize::MAX,
            key: cid.to_string(),
            value: component_node(spec),
        });
        Ok(cid)
    }

    fn create_object(
        &mut self,
        id: Option<Uuid>,
        name: &str,
        parent: Option<Uuid>,
        index: Option<usize>,
        components: &[ComponentSpec],
        prefab: Option<Node>,
    ) -> Result<Vec<Uuid>, Diagnostic> {
        let id = id.unwrap_or_else(Uuid::new_v4);
        if self.root.pointer(&format!("/entities/{id}")).is_some() {
            return Err(Diagnostic::error(
                Code::EDIT_DUPLICATE_ID,
                format!("entity {id} already exists"),
            )
            .with_pointer("/entities")
            .with_id(id.to_string()));
        }
        let parent_pointer = match parent {
            Some(p) => Some(self.entity_pointer(p)?),
            None => None,
        };
        let mut affected = vec![id];
        let mut comps = Vec::with_capacity(components.len());
        let mut seen = HashSet::new();
        for c in components {
            let cid = c.id.unwrap_or_else(Uuid::new_v4);
            if !seen.insert(cid) {
                return Err(Diagnostic::error(
                    Code::EDIT_DUPLICATE_ID,
                    format!("component {cid} is listed twice"),
                )
                .with_id(cid.to_string()));
            }
            comps.push((cid.to_string(), component_node(c)));
            affected.push(cid);
        }
        let mut members = vec![
            ("name".to_string(), Node::string(name)),
            (
                "parent".to_string(),
                match parent {
                    Some(p) => Node::string(p.to_string()),
                    None => Node::synthetic(Value::Null),
                },
            ),
            (
                "children".to_string(),
                Node::synthetic(Value::Array(Vec::new())),
            ),
            ("components".to_string(), Node::object(comps)),
        ];
        if let Some(p) = prefab {
            members.push(("prefab".to_string(), p));
        }
        if self
            .root
            .pointer("/entities")
            .and_then(Node::as_object)
            .is_none()
        {
            self.set_member("", "entities", Node::object(Vec::new()));
        }
        self.edits.push(Edit::InsertMember {
            parent: "/entities".into(),
            index: usize::MAX,
            key: id.to_string(),
            value: Node::object(members),
        });
        self.attach(id, parent_pointer.as_deref(), index);
        if let Some(p) = parent {
            affected.push(p);
        }
        Ok(affected)
    }

    /// Adds `id` to a parent's `children` (or the prefab's `roots`).
    fn attach(&mut self, id: Uuid, parent_pointer: Option<&str>, index: Option<usize>) {
        let list = match parent_pointer {
            Some(p) => {
                let children = format!("{p}/children");
                if self
                    .root
                    .pointer(&children)
                    .and_then(Node::as_array)
                    .is_none()
                {
                    self.set_member(p, "children", Node::synthetic(Value::Array(Vec::new())));
                }
                children
            }
            None => {
                if self.kind != DocumentKind::Prefab {
                    return;
                }
                if self
                    .root
                    .pointer("/roots")
                    .and_then(Node::as_array)
                    .is_none()
                {
                    self.set_member("", "roots", Node::synthetic(Value::Array(Vec::new())));
                }
                "/roots".to_string()
            }
        };
        self.edits.push(Edit::InsertItem {
            parent: list,
            index: index.unwrap_or(usize::MAX),
            value: Node::string(id.to_string()),
        });
    }

    /// Removes `id` from its parent's `children` (or `roots`), when listed.
    fn detach(&mut self, id: Uuid, parent: Option<Uuid>) {
        let list = match parent {
            Some(p) => format!("/entities/{p}/children"),
            None if self.kind == DocumentKind::Prefab => "/roots".to_string(),
            None => return,
        };
        let text = id.to_string();
        if let Some(items) = self.root.pointer(&list).and_then(Node::as_array) {
            // Every occurrence (a malformed document may list it twice).
            for (i, item) in items.iter().enumerate().rev() {
                if item.as_str() == Some(&text) {
                    self.edits.push(Edit::RemoveItem {
                        parent: list.clone(),
                        index: i,
                    });
                }
            }
        }
    }

    fn parent_of(&self, id: Uuid) -> Option<Uuid> {
        self.root
            .pointer(&format!("/entities/{id}/parent"))
            .and_then(Node::as_str)
            .and_then(|s| parse_uuid(s).ok())
    }

    fn children_of(&self, id: Uuid) -> Vec<Uuid> {
        self.root
            .pointer(&format!("/entities/{id}/children"))
            .and_then(Node::as_array)
            .into_iter()
            .flatten()
            .filter_map(|c| c.as_str().and_then(|s| parse_uuid(s).ok()))
            .collect()
    }

    fn delete_object(&mut self, id: Uuid) -> Result<Vec<Uuid>, Diagnostic> {
        self.entity_pointer(id)?;
        // Subtree by children lists, each entity once, only real children.
        let mut order = Vec::new();
        let mut seen = HashSet::new();
        let mut stack = vec![id];
        while let Some(e) = stack.pop() {
            if !seen.insert(e) {
                continue;
            }
            order.push(e);
            for c in self.children_of(e) {
                if self.parent_of(c) == Some(e)
                    && self.root.pointer(&format!("/entities/{c}")).is_some()
                {
                    stack.push(c);
                }
            }
        }
        let parent = self.parent_of(id);
        self.detach(id, parent);
        for e in &order {
            self.edits.push(Edit::RemoveMember {
                parent: "/entities".into(),
                key: e.to_string(),
            });
        }
        let mut affected = order;
        if let Some(p) = parent {
            affected.push(p);
        }
        Ok(affected)
    }

    fn reparent(
        &mut self,
        id: Uuid,
        new_parent: Option<Uuid>,
        index: Option<usize>,
    ) -> Result<Vec<Uuid>, Diagnostic> {
        let p = self.entity_pointer(id)?;
        let new_parent_pointer = match new_parent {
            Some(np) => Some(self.entity_pointer(np)?),
            None => None,
        };
        // Cycle: the new parent must not be the entity or below it.
        let mut cur = new_parent;
        let mut hops = 0;
        while let Some(a) = cur {
            if a == id {
                return Err(Diagnostic::error(
                    Code::EDIT_CYCLE,
                    format!("reparentObject: {id} cannot be its own ancestor"),
                )
                .with_pointer(format!("{p}/parent"))
                .with_id(id.to_string()));
            }
            cur = self.parent_of(a);
            hops += 1;
            if hops > 100_000 {
                break;
            }
        }
        let old_parent = self.parent_of(id);
        self.detach(id, old_parent);
        self.set_member(
            &p,
            "parent",
            match new_parent {
                Some(np) => Node::string(np.to_string()),
                None => Node::synthetic(Value::Null),
            },
        );
        self.attach(id, new_parent_pointer.as_deref(), index);
        let mut affected = vec![id];
        affected.extend(old_parent);
        affected.extend(new_parent);
        Ok(affected)
    }

    // ---- document-level fields (M4-01)

    /// A document pointer a command may change: not the envelope's
    /// identity, not the entity tree (which has its own commands), and
    /// with a parent that exists.
    fn document_pointer(&self, pointer: &str, what: &str) -> Result<(String, String), Diagnostic> {
        let refuse = |why: &str| {
            Err(Diagnostic::error(
                Code::EDIT_INVALID_POINTER,
                format!("{what}: {pointer:?} {why}"),
            )
            .with_pointer(pointer.to_string()))
        };
        if pointer.is_empty() || !pointer.starts_with('/') {
            return refuse("is not a JSON pointer to a member of the document");
        }
        let (parent, key) = split_pointer(pointer);
        let first = pointer[1..].split('/').next().unwrap_or("");
        if parent.is_empty() && matches!(first, "$schema" | "kind" | "formatVersion" | "id") {
            return refuse("is the document's identity; migrations own it");
        }
        if matches!(first, "entities" | "roots") && self.kind != DocumentKind::Material {
            return refuse("is the entity tree; use the entity commands");
        }
        if self.root.pointer(&parent).is_none() {
            return refuse("has no parent node");
        }
        Ok((parent, key))
    }

    fn set_document_field(&mut self, pointer: &str, value: Node) -> Result<(), Diagnostic> {
        let (parent, key) = self.document_pointer(pointer, "setDocumentField")?;
        if self.root.pointer(pointer).is_some() {
            self.edits.push(Edit::Replace {
                pointer: pointer.to_string(),
                value,
            });
            return Ok(());
        }
        match self.root.pointer(&parent).map(|n| &n.value) {
            Some(Value::Object(_)) => self.edits.push(Edit::InsertMember {
                parent,
                index: usize::MAX,
                key,
                value,
            }),
            Some(Value::Array(items)) if key.parse::<usize>().ok() == Some(items.len()) => {
                self.edits.push(Edit::InsertItem {
                    parent,
                    index: items.len(),
                    value,
                })
            }
            _ => {
                return Err(Diagnostic::error(
                    Code::EDIT_INVALID_POINTER,
                    format!(
                        "setDocumentField: cannot set {pointer:?}: {parent:?} is not an object or an array end"
                    ),
                )
                .with_pointer(pointer.to_string()))
            }
        }
        Ok(())
    }

    fn unset_document_field(&mut self, pointer: &str) -> Result<(), Diagnostic> {
        let (parent, key) = self.document_pointer(pointer, "unsetDocumentField")?;
        if self.root.pointer(pointer).is_none() {
            return Err(Diagnostic::error(
                Code::EDIT_INVALID_POINTER,
                format!("unsetDocumentField: {pointer:?} is not set"),
            )
            .with_pointer(pointer.to_string()));
        }
        if parent.is_empty() && key == "name" {
            return Err(Diagnostic::error(
                Code::EDIT_INVALID_POINTER,
                "unsetDocumentField: every document has a name; set it instead",
            )
            .with_pointer(pointer.to_string()));
        }
        match self.root.pointer(&parent).map(|n| &n.value) {
            Some(Value::Object(_)) => self.edits.push(Edit::RemoveMember { parent, key }),
            Some(Value::Array(_)) => self.edits.push(Edit::RemoveItem {
                parent,
                index: key.parse().unwrap_or(usize::MAX),
            }),
            _ => unreachable!("target exists under a container"),
        }
        Ok(())
    }

    // ---- duplication (M4-01)

    /// Copies `id`'s subtree with fresh ids. The subtree is gathered like
    /// `delete_object`'s (children whose parent agrees); every entity,
    /// component and prefab-instance id in it gets a new UUID and every
    /// string equal to a remapped id inside the copy follows, so
    /// references between copied entities stay internal to the copy.
    /// `unpackPrefab`: the expansion of one instance written into the
    /// document. The whole scene is expanded (the way the runtime and
    /// `fynkel scene expand` do it, so nested prefabs and imported
    /// hierarchies come out the same); only this anchor's part is taken.
    fn unpack(&mut self, entity: Uuid) -> Result<Vec<Uuid>, Diagnostic> {
        let e = self.entity_pointer(entity)?;
        if self
            .root
            .pointer(&format!("{e}/prefab"))
            .and_then(Node::as_object)
            .is_none()
        {
            return Err(Diagnostic::error(
                Code::EDIT_NOT_AN_INSTANCE,
                format!("unpackPrefab: entity {entity} is not a prefab instance"),
            )
            .with_pointer(e)
            .with_id(entity.to_string()));
        }
        let scene = Scene::from_document(Document {
            kind: DocumentKind::Scene,
            path: None,
            root: self.root.clone(),
            envelope: self.envelope.clone(),
        });
        let expanded = prefab::expand(&scene, self.prefabs, self.schemas);
        let Some(inst) = expanded.instances.iter().find(|i| i.anchor == entity) else {
            // Why it did not expand: the diagnostics about this anchor, or
            // all of them when none names it (a nested prefab's problem).
            let text = entity.to_string();
            let mut why: Vec<Diagnostic> = expanded
                .diagnostics
                .iter()
                .filter(|d| d.id.as_deref() == Some(text.as_str()))
                .cloned()
                .collect();
            if why.is_empty() {
                why = expanded.diagnostics.clone();
            }
            let mut d = Diagnostic::error(
                Code::EDIT_INVALID_COMMAND,
                format!(
                    "unpackPrefab: the instance at {entity} cannot be expanded ({} problem(s))",
                    why.len()
                ),
            )
            .with_pointer(format!("{e}/prefab"))
            .with_id(text)
            .with_suggestion(
                "fix the instance's prefab and overrides first; an instance whose expansion fails has nothing to unpack",
            );
            d.related = why;
            return Err(d);
        };
        let ex_entities = expanded
            .document
            .root
            .get("entities")
            .and_then(Node::as_object)
            .expect("the expansion has an entities object");
        let member = |id: Uuid| -> &Node {
            let key = id.to_string();
            &ex_entities
                .iter()
                .find(|m| m.key == key)
                .expect("the expansion has every derived entity")
                .value
        };

        // The anchor: no `prefab`, the instance roots appended to its children.
        self.edits.push(Edit::RemoveMember {
            parent: e.clone(),
            key: "prefab".into(),
        });
        if let Some(children) = member(entity).get("children") {
            let p = format!("{e}/children");
            if self.root.pointer(&p).is_some() {
                self.edits.push(Edit::Replace {
                    pointer: p,
                    value: children.clone(),
                });
            } else {
                self.edits.push(Edit::InsertMember {
                    parent: e.clone(),
                    index: usize::MAX,
                    key: "children".into(),
                    value: children.clone(),
                });
            }
        }
        // The derived entities right after the anchor, in expansion order.
        let position = self
            .root
            .get("entities")
            .and_then(Node::as_object)
            .and_then(|m| m.iter().position(|m| m.key == entity.to_string()))
            .expect("the anchor is an entity");
        let mut affected = vec![entity];
        for (i, (_, sid)) in inst.entities.iter().enumerate() {
            self.edits.push(Edit::InsertMember {
                parent: "/entities".into(),
                index: position + 1 + i,
                key: sid.to_string(),
                value: member(*sid).clone(),
            });
            affected.push(*sid);
        }
        affected.extend(inst.components.iter().map(|(_, sid)| *sid));
        Ok(affected)
    }

    fn duplicate(
        &mut self,
        id: Uuid,
        new_id: Option<Uuid>,
        name: Option<&str>,
        index: Option<usize>,
    ) -> Result<Vec<Uuid>, Diagnostic> {
        self.entity_pointer(id)?;
        let mut order = Vec::new();
        let mut seen = HashSet::new();
        let mut stack = vec![id];
        while let Some(e) = stack.pop() {
            if !seen.insert(e) {
                continue;
            }
            order.push(e);
            for c in self.children_of(e) {
                if self.parent_of(c) == Some(e)
                    && self.root.pointer(&format!("/entities/{c}")).is_some()
                {
                    stack.push(c);
                }
            }
        }
        // Old id → new id for entities, components and instances.
        let mut map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let root_copy = new_id.unwrap_or_else(Uuid::new_v4);
        if self
            .root
            .pointer(&format!("/entities/{root_copy}"))
            .is_some()
        {
            return Err(Diagnostic::error(
                Code::EDIT_DUPLICATE_ID,
                format!("duplicateObject: entity {root_copy} already exists"),
            )
            .with_pointer("/entities")
            .with_id(root_copy.to_string()));
        }
        map.insert(id.to_string(), root_copy.to_string());
        for e in &order[1..] {
            map.insert(e.to_string(), Uuid::new_v4().to_string());
        }
        for e in &order {
            let node = self
                .root
                .pointer(&format!("/entities/{e}"))
                .expect("gathered");
            if let Some(components) = node.get("components").and_then(Node::as_object) {
                for m in components {
                    map.entry(m.key.clone())
                        .or_insert_with(|| Uuid::new_v4().to_string());
                }
            }
            if let Some(inst) = node.pointer("/prefab/instance").and_then(Node::as_str) {
                map.entry(inst.to_string())
                    .or_insert_with(|| Uuid::new_v4().to_string());
            }
        }
        let mut affected = Vec::with_capacity(order.len() * 2);
        let mut copies = Vec::with_capacity(order.len());
        for (i, e) in order.iter().enumerate() {
            let node = self
                .root
                .pointer(&format!("/entities/{e}"))
                .expect("gathered");
            let mut copy = remap(node, &map);
            if i == 0 {
                if let Some(n) = name {
                    copy.set("name", Node::string(n));
                }
                // The root copy keeps the original's parent (already not
                // in the map) and is listed by `attach` below.
            }
            let new_key = map[&e.to_string()].clone();
            affected.push(parse_uuid(&new_key).expect("fresh uuid"));
            if let Some(components) = copy.get("components").and_then(Node::as_object) {
                for m in components {
                    if let Ok(c) = parse_uuid(&m.key) {
                        affected.push(c);
                    }
                }
            }
            if let Some(inst) = copy
                .pointer("/prefab/instance")
                .and_then(Node::as_str)
                .and_then(|s| parse_uuid(s).ok())
            {
                affected.push(inst);
            }
            copies.push((new_key, copy));
        }
        for (key, copy) in copies {
            self.edits.push(Edit::InsertMember {
                parent: "/entities".into(),
                index: usize::MAX,
                key,
                value: copy,
            });
        }
        // Next to the original in its parent's list (or the roots).
        let parent = self.parent_of(id);
        let parent_pointer = parent.map(|p| format!("/entities/{p}"));
        let list = match parent {
            Some(p) => Some(format!("/entities/{p}/children")),
            None if self.kind == DocumentKind::Prefab => Some("/roots".to_string()),
            None => None,
        };
        let after_original = list.and_then(|l| {
            self.root
                .pointer(&l)
                .and_then(Node::as_array)
                .and_then(|items| {
                    items
                        .iter()
                        .position(|n| n.as_str() == Some(&id.to_string()))
                })
                .map(|i| i + 1)
        });
        self.attach(
            root_copy,
            parent_pointer.as_deref(),
            index.or(after_original),
        );
        if let Some(p) = parent {
            affected.push(p);
        }
        Ok(affected)
    }

    fn set_override(&mut self, entity: Uuid, op: &Node) -> Result<Vec<Uuid>, Diagnostic> {
        let list = self.overrides_pointer(entity)?;
        // The op must read as one of the prefab model's ops.
        let mut diags = Vec::new();
        let parsed = crate::scene::read_override(op, "", &mut diags, &|d, _| d);
        let Some(parsed) = parsed else {
            let mut d = Diagnostic::error(
                Code::EDIT_INVALID_COMMAND,
                "setPrefabOverride: the override is not a valid op",
            )
            .with_pointer(list)
            .with_id(entity.to_string());
            for r in diags {
                d = d.with_related(r);
            }
            return Err(d);
        };
        let target = override_target(&parsed);
        let existing: Vec<Option<String>> = self
            .root
            .pointer(&list)
            .and_then(Node::as_array)
            .into_iter()
            .flatten()
            .map(|n| {
                let mut sink = Vec::new();
                crate::scene::read_override(n, "", &mut sink, &|d, _| d)
                    .map(|o| override_target(&o))
            })
            .collect();
        match existing
            .iter()
            .position(|t| t.as_deref() == Some(target.as_str()))
        {
            Some(i) => self.edits.push(Edit::Replace {
                pointer: format!("{list}/{i}"),
                value: op.clone(),
            }),
            None => self.edits.push(Edit::InsertItem {
                parent: list,
                index: usize::MAX,
                value: op.clone(),
            }),
        }
        Ok(vec![entity, parsed.entity()])
    }
}

/// A deep copy of `node` with every object key and string value that is a
/// remapped id replaced (spans dropped: the copy is new text).
fn remap(node: &Node, map: &std::collections::HashMap<String, String>) -> Node {
    let value = match &node.value {
        Value::String(s) => Value::String(map.get(s).cloned().unwrap_or_else(|| s.clone())),
        Value::Array(items) => Value::Array(items.iter().map(|n| remap(n, map)).collect()),
        Value::Object(members) => Value::Object(
            members
                .iter()
                .map(|m| Member {
                    key: map.get(&m.key).cloned().unwrap_or_else(|| m.key.clone()),
                    key_span: Span::default(),
                    value: remap(&m.value, map),
                })
                .collect(),
        ),
        other => other.clone(),
    };
    Node::synthetic(value)
}

/// Two overrides with the same target replace each other.
fn override_target(op: &crate::scene::OverrideOp) -> String {
    use crate::scene::OverrideOp as O;
    match op {
        O::SetField {
            entity,
            component,
            field,
            ..
        }
        | O::UnsetField {
            entity,
            component,
            field,
        } => format!("field:{entity}:{component}:{field}"),
        O::AddComponent { entity, component } => format!("component:{entity}:{}", component.id),
        O::RemoveComponent { entity, component } => format!("component:{entity}:{component}"),
        O::AddObject { entity, .. } | O::RemoveObject { entity } => format!("object:{entity}"),
    }
}

fn component_node(spec: &ComponentSpec) -> Node {
    Node::object(vec![
        ("type".to_string(), Node::string(spec.type_id.clone())),
        (
            "version".to_string(),
            Node::integer(spec.version.unwrap_or(1) as i64),
        ),
        (
            "data".to_string(),
            spec.data
                .clone()
                .unwrap_or_else(|| Node::object(Vec::new())),
        ),
    ])
}

/// `field` as a pointer under `data`: a `/`-prefixed pointer is used as is,
/// anything else is one escaped key.
fn field_pointer(data: &str, field: &str) -> String {
    if field.starts_with('/') {
        format!("{data}{field}")
    } else {
        format!("{data}/{}", escape(field))
    }
}

/// `(parent pointer, last token unescaped)`.
fn split_pointer(pointer: &str) -> (String, String) {
    match pointer.rsplit_once('/') {
        Some((parent, last)) => (
            parent.to_string(),
            last.replace("~1", "/").replace("~0", "~"),
        ),
        None => (String::new(), pointer.to_string()),
    }
}

/// A unified diff between two documents' canonical texts (for `--dry-run`).
pub fn diff(before: &str, after: &str, name: &str) -> String {
    use similar::TextDiff;
    TextDiff::from_lines(before, after)
        .unified_diff()
        .context_radius(3)
        .header(&format!("a/{name}"), &format!("b/{name}"))
        .to_string()
}
