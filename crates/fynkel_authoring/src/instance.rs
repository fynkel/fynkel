//! One prefab instance as Prefab mode shows it (M4-07): its expansion —
//! the prefab's root as the instance itself (ADR-0022), the other
//! entities under the ids the runtime derives,
//! with the overrides applied and marked — plus the prefab it comes from
//! and why it did not expand when it did not. Read-only; the `instance`
//! query.

use std::collections::HashMap;

use fynkel_contracts::{Code, Diagnostic};
use fynkel_document::scene::OverrideOp;
use fynkel_document::{AssetKind, DocumentKind, Node, Prefab};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{AuthoringSession, Handle, OpenProject};

impl AuthoringSession {
    /// The expansion of the instance anchored at `entity` in the document
    /// `handle` (a scene, or a prefab with a nested instance), against the
    /// prefabs as they are now (open editors first — the same library the
    /// preview uses):
    ///
    /// ```text
    /// { anchor, instance, asset: { asset, sub }, expanded,
    ///   prefab: { id, name, path, kind: prefab|source|rig, open, handle, readOnly } | null,
    ///   diagnostics: [...],                       // about this instance
    ///   entities: [ { id, local, name, active, activeOverridden, parent, children: [id], added,
    ///                 components: { <id>: { id, local, type, version, data, added, overridden: [field] } } } ],
    ///   overrides: [ { index, op, entity, entityId, entityName, component?, componentId?, componentType?, field?, value?, pointer } ] }
    /// ```
    ///
    /// `entities` are in the expansion's document order, the first is the
    /// instance itself (the prefab's root, with the instance's id, name and
    /// parent); the others' ids are the derived scene ids. `local` is the
    /// prefab's id.
    /// `overridden` lists the `setField`/`unsetField` fields of a
    /// component (the Inspector's bold), `activeOverridden` a `setActive`
    /// on the entity, `added` marks an `addComponent`
    /// or `addObject`. A prefab that is not available (`FYN-D0401`) or an
    /// instance whose overrides do not apply is `expanded: false` with
    /// the diagnostics and no entities — the anchor keeps its data. Not an
    /// instance: `FYN-E0008`.
    pub fn instance_json(&self, handle: Handle, entity: Uuid) -> Result<Value, Diagnostic> {
        let project = self.require_project()?;
        let doc = self.document(handle)?;
        let document = doc.editor.document();
        let scene = match document.kind {
            DocumentKind::Scene => doc.editor.scene(),
            DocumentKind::Prefab => Prefab::from_document(document.clone()).scene(),
            other => {
                return Err(Diagnostic::error(
                    Code::EDIT_NOT_AN_INSTANCE,
                    format!("a {} document has no prefab instances", other.name()),
                )
                .with_document(doc.shown()));
            }
        };
        let anchor = scene.entity(entity).ok_or_else(|| {
            Diagnostic::error(Code::EDIT_UNKNOWN_ENTITY, format!("no entity {entity}"))
                .with_document(doc.shown())
                .with_id(entity.to_string())
        })?;
        let inst = anchor.prefab.as_ref().ok_or_else(|| {
            Diagnostic::error(
                Code::EDIT_NOT_AN_INSTANCE,
                format!(
                    "entity {entity} ({:?}) is not a prefab instance",
                    anchor.name
                ),
            )
            .with_document(doc.shown())
            .with_pointer(anchor.pointer.clone())
            .with_id(entity.to_string())
        })?;

        let (library, library_diagnostics) = self.prefab_library(project, &scene);
        let expanded = fynkel_document::expand(&scene, &library, &project.schemas);
        let text = entity.to_string();
        // What concerns this instance: anchored at it, or under its pointer.
        let mut diagnostics: Vec<Value> = Vec::new();
        for d in library_diagnostics
            .iter()
            .chain(expanded.diagnostics.iter())
        {
            let mine = d.id.as_deref() == Some(text.as_str())
                || d.pointer
                    .as_deref()
                    .is_some_and(|p| p.starts_with(&format!("{}/", anchor.pointer)));
            if mine {
                diagnostics.push(fynkel_document::diagnostic_json(d));
            }
        }
        let prefab = library.get_ref(&inst.asset);
        let prefab_json =
            prefab.map(|p| self.prefab_json(project, p, inst.prefab_id(), &inst.asset.sub));
        let expansion = expanded.instances.iter().find(|i| i.anchor == entity);

        // Derived ids, then the overrides marked on them.
        let mut entities: Vec<Value> = Vec::new();
        let mut entity_index: HashMap<Uuid, usize> = HashMap::new();
        let mut component_ids: HashMap<Uuid, Uuid> = HashMap::new();
        if let Some(x) = expansion {
            let out = fynkel_document::Scene::from_document(expanded.document.clone());
            for (local, sid) in &x.entities {
                let Some(e) = out.entity(*sid) else { continue };
                let mut components = serde_json::Map::new();
                for c in &e.components {
                    let local_c = x
                        .components
                        .iter()
                        .find(|(_, s)| *s == c.id)
                        .map(|(l, _)| *l);
                    component_ids.insert(local_c.unwrap_or(c.id), c.id);
                    components.insert(
                        c.id.to_string(),
                        json!({
                            "id": c.id.to_string(),
                            "local": local_c.map(|l| l.to_string()),
                            "type": c.type_id,
                            "version": c.version,
                            "data": c.data.to_serde(),
                            "added": false,
                            "overridden": Vec::<String>::new(),
                        }),
                    );
                }
                entity_index.insert(*local, entities.len());
                entities.push(json!({
                    "id": sid.to_string(),
                    "local": local.to_string(),
                    "name": e.name,
                    "active": e.active,
                    "activeOverridden": false,
                    "parent": e.parent.map(|p| p.to_string()),
                    "children": e.children.iter().map(|c| c.to_string()).collect::<Vec<_>>(),
                    "added": false,
                    "components": components,
                }));
            }
        }
        let mark = |entities: &mut Vec<Value>, local_e: Uuid, f: &dyn Fn(&mut Value)| {
            if let Some(i) = entity_index.get(&local_e) {
                f(&mut entities[*i]);
            }
        };
        let mut overrides: Vec<Value> = Vec::new();
        let raw = document
            .root
            .pointer(&format!("{}/overrides", inst.pointer))
            .and_then(Node::as_array);
        for (i, o) in inst.overrides.iter().enumerate() {
            let mut v = raw
                .and_then(|items| items.get(i))
                .map(Node::to_serde)
                .unwrap_or_else(|| json!({}));
            let local_e = o.op.entity();
            let derived_e = expansion.and_then(|x| x.entity(local_e));
            v["index"] = json!(i);
            v["op"] = json!(o.op.name());
            v["pointer"] = json!(o.pointer);
            v["entityId"] = json!(derived_e.map(|d| d.to_string()));
            v["entityName"] = entity_index
                .get(&local_e)
                .map(|i| entities[*i]["name"].clone())
                .unwrap_or(Value::Null);
            let component_of = |local_c: Uuid| component_ids.get(&local_c).copied();
            match &o.op {
                OverrideOp::SetField {
                    component, field, ..
                }
                | OverrideOp::UnsetField {
                    component, field, ..
                } => {
                    let derived_c = component_of(*component);
                    v["componentId"] = json!(derived_c.map(|d| d.to_string()));
                    if let Some(cid) = derived_c {
                        mark(&mut entities, local_e, &|e: &mut Value| {
                            let c = &mut e["components"][cid.to_string()];
                            v_push(&mut c["overridden"], json!(field));
                        });
                        v["componentType"] = component_type(&entities, &entity_index, local_e, cid);
                    }
                }
                OverrideOp::AddComponent { component, .. } => {
                    let derived_c = component_of(component.id);
                    v["componentId"] = json!(derived_c.map(|d| d.to_string()));
                    v["componentType"] = json!(component.type_id);
                    if let Some(cid) = derived_c {
                        mark(&mut entities, local_e, &|e: &mut Value| {
                            e["components"][cid.to_string()]["added"] = json!(true);
                        });
                    }
                }
                OverrideOp::RemoveComponent { component, .. } => {
                    v["componentId"] = Value::Null;
                    v["componentType"] = prefab
                        .and_then(|p| p.entities.iter().find(|e| e.id == local_e))
                        .and_then(|e| e.components.iter().find(|c| c.id == *component))
                        .map(|c| json!(c.type_id))
                        .unwrap_or(Value::Null);
                }
                OverrideOp::AddObject { components, .. } => {
                    mark(&mut entities, local_e, &|e: &mut Value| {
                        e["added"] = json!(true);
                        for c in components {
                            if let Some(cid) = component_of(c.id) {
                                e["components"][cid.to_string()]["added"] = json!(true);
                            }
                        }
                    });
                }
                OverrideOp::SetActive { .. } => {
                    mark(&mut entities, local_e, &|e: &mut Value| {
                        e["activeOverridden"] = json!(true);
                    });
                }
                OverrideOp::RemoveObject { .. } => {
                    v["entityName"] = prefab
                        .and_then(|p| p.entities.iter().find(|e| e.id == local_e))
                        .map(|e| json!(e.name))
                        .unwrap_or(Value::Null);
                }
            }
            overrides.push(v);
        }

        Ok(json!({
            "anchor": text,
            "instance": inst.instance.to_string(),
            "asset": { "asset": inst.prefab_id().to_string(), "sub": inst.asset.sub },
            "expanded": expansion.is_some(),
            "prefab": prefab_json,
            "diagnostics": diagnostics,
            "entities": entities,
            "overrides": overrides,
        }))
    }

    /// The `prefab` block: the resolved prefab and how to open it.
    fn prefab_json(&self, project: &OpenProject, prefab: &Prefab, asset: Uuid, sub: &str) -> Value {
        let imported = !sub.is_empty();
        let entry = project.assets.get(asset);
        let kind = match entry.map(|e| &e.kind) {
            Some(AssetKind::Source { .. }) => "source",
            Some(AssetKind::Document(DocumentKind::Rig)) => "rig",
            _ => "prefab",
        };
        let path = entry.map(|e| e.path.clone());
        let file = entry
            .map(|e| e.file(project.assets.root()))
            .and_then(|f| f.canonicalize().ok());
        let handle = self
            .documents
            .iter()
            .find(|(_, d)| {
                Some(&d.path) == file.as_ref()
                    && d.sub.as_deref() == if imported { Some(sub) } else { None }
            })
            .map(|(h, _)| *h);
        let read_only = if imported {
            Some(crate::imported_reason(
                &format!(
                    "{}#{sub}",
                    path.clone().unwrap_or_else(|| asset.to_string())
                ),
                kind,
            ))
        } else {
            project.read_only.clone()
        };
        json!({
            "id": prefab.id().to_string(),
            "name": prefab.document.envelope.name,
            "path": path,
            "kind": kind,
            "open": if imported { format!("{asset}#{sub}") } else { asset.to_string() },
            "handle": handle,
            "readOnly": read_only,
        })
    }
}

fn v_push(array: &mut Value, item: Value) {
    if let Value::Array(items) = array {
        if !items.contains(&item) {
            items.push(item);
        }
    }
}

fn component_type(
    entities: &[Value],
    index: &HashMap<Uuid, usize>,
    local_e: Uuid,
    cid: Uuid,
) -> Value {
    index
        .get(&local_e)
        .map(|i| entities[*i]["components"][cid.to_string()]["type"].clone())
        .unwrap_or(Value::Null)
}
