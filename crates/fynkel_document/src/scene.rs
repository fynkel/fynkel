//! Scene documents (blueprint §6, §19.2c item 3): the typed view over a
//! `.scene.json` tree and the semantic rules JSON Schema cannot express.
//!
//! The tree ([`Document::root`]) stays the source of truth; [`Scene`] is a
//! read view built from it, lenient enough to describe a broken document
//! (every problem becomes a diagnostic with a pointer) so `inspect` and the
//! editor can still show what is there.
//!
//! Semantic rules (codes `FYN-D02xx`):
//! - parents and children must exist (`D0200`, `D0201`);
//! - `children` and `parent` must agree both ways (`D0202`); a child listed
//!   twice is `D0208`;
//! - the hierarchy is a forest (`D0203`);
//! - one component per type per entity (`D0204`) — the runtime stores one;
//! - unknown component types are kept and reported (`D0205`, warning);
//! - component `data` must match the type's schema (`D0206`, via the
//!   loaded component schemas);
//! - engine-managed components may not be authored (`D0207`).

use std::collections::{HashMap, HashSet};

use fynkel_contracts::{Code, Diagnostic, Severity};
use uuid::Uuid;

use crate::json::{Node, Value};
use crate::schema::SchemaSet;
use crate::{parse_uuid, Document, DocumentKind};

#[derive(Clone, Debug, PartialEq)]
pub struct Component {
    pub id: Uuid,
    pub type_id: String,
    pub version: u32,
    pub data: Node,
    /// Pointer to the component record in the document.
    pub pointer: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entity {
    pub id: Uuid,
    pub name: String,
    pub parent: Option<Uuid>,
    /// As authored (may be inconsistent; validation says so).
    pub children: Vec<Uuid>,
    pub components: Vec<Component>,
    pub pointer: String,
}

impl Entity {
    pub fn component_of(&self, type_id: &str) -> Option<&Component> {
        self.components.iter().find(|c| c.type_id == type_id)
    }
}

/// Typed read view of a scene document.
#[derive(Clone, Debug)]
pub struct Scene {
    pub document: Document,
    /// In document (key) order.
    pub entities: Vec<Entity>,
    index: HashMap<Uuid, usize>,
    /// Problems found while building the view (malformed entries).
    read_diagnostics: Vec<Diagnostic>,
}

impl Scene {
    /// Builds the view; malformed entries are skipped and reported by
    /// [`Scene::validate`] (and returned here for callers that only read).
    pub fn from_document(document: Document) -> Scene {
        let mut diags = Vec::new();
        let mut entities = Vec::new();
        let mut index = HashMap::new();
        if document.kind != DocumentKind::Scene {
            diags.push(
                Diagnostic::error(
                    Code::UNKNOWN_DOCUMENT_KIND,
                    format!("expected a scene document, got {}", document.kind.name()),
                )
                .with_pointer(""),
            );
        }
        let root = &document.root;
        let anchor = |d: Diagnostic, pointer: &str| -> Diagnostic {
            let d = d.with_pointer(pointer);
            match root.pointer_span(pointer) {
                Some(s) => d.at(s.start.line, s.start.column),
                None => d,
            }
        };
        match root.get("entities").map(|n| &n.value) {
            Some(Value::Object(members)) => {
                for m in members {
                    let pointer = format!("/entities/{}", escape(&m.key));
                    let id = match parse_uuid(&m.key) {
                        Ok(id) => id,
                        Err(why) => {
                            diags.push(anchor(
                                Diagnostic::error(
                                    Code::MALFORMED_ID,
                                    format!("entity key is not a canonical UUID: {why}"),
                                ),
                                &pointer,
                            ));
                            continue;
                        }
                    };
                    match read_entity(id, &m.value, &pointer, &mut diags, &anchor) {
                        Some(e) => {
                            if index.insert(id, entities.len()).is_some() {
                                // Cannot happen: the strict parser rejects duplicate keys.
                                continue;
                            }
                            entities.push(e);
                        }
                        None => continue,
                    }
                }
            }
            Some(_) => diags.push(anchor(
                Diagnostic::error(
                    Code::SCENE_MALFORMED_ENTRY,
                    "entities must be an object keyed by UUID",
                ),
                "/entities",
            )),
            None => diags.push(anchor(
                Diagnostic::error(Code::SCENE_MALFORMED_ENTRY, "entities is required")
                    .with_suggestion("add \"entities\": {}"),
                "",
            )),
        }
        for d in &mut diags {
            if d.document.is_none() {
                d.document = document.path.clone();
            }
        }
        Scene {
            document,
            entities,
            index,
            read_diagnostics: diags,
        }
    }

    pub fn entity(&self, id: Uuid) -> Option<&Entity> {
        self.index.get(&id).map(|i| &self.entities[*i])
    }

    /// Entities with no parent, in document order.
    pub fn roots(&self) -> impl Iterator<Item = &Entity> {
        self.entities.iter().filter(|e| e.parent.is_none())
    }

    /// Problems found while reading the tree into the view.
    pub fn read_diagnostics(&self) -> &[Diagnostic] {
        &self.read_diagnostics
    }

    /// Everything: envelope + JSON Schema ([`Document::validate`]), the read
    /// problems, and the semantic rules above. Ordered by kind of check,
    /// then document position.
    pub fn validate(&self, schemas: &SchemaSet) -> Vec<Diagnostic> {
        let mut out = self.document.validate(schemas);
        out.extend(self.read_diagnostics.iter().cloned());
        // The scene schema already binds `data` of built-in types to their
        // component schemas; do not say the same thing twice.
        let schema_pointers: Vec<String> = out.iter().filter_map(|d| d.pointer.clone()).collect();
        for mut d in self.validate_semantics(schemas) {
            if d.code == Code::SCENE_COMPONENT_DATA
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
        let anchor = |d: Diagnostic, pointer: &str| -> Diagnostic {
            let d = d.with_pointer(pointer);
            match root.pointer_span(pointer) {
                Some(s) => d.at(s.start.line, s.start.column),
                None => d,
            }
        };
        let mut out = Vec::new();

        // Parent/child references and agreement.
        for e in &self.entities {
            if let Some(p) = e.parent {
                match self.entity(p) {
                    None => out.push(anchor(
                        Diagnostic::error(
                            Code::SCENE_DANGLING_PARENT,
                            format!("parent {p} does not exist"),
                        )
                        .with_id(e.id.to_string())
                        .with_suggestion("set parent to null or to an existing entity"),
                        &format!("{}/parent", e.pointer),
                    )),
                    Some(parent) if !parent.children.contains(&e.id) => out.push(anchor(
                        Diagnostic::error(
                            Code::SCENE_CHILD_PARENT_MISMATCH,
                            format!(
                                "parent {} ({:?}) does not list this entity in its children",
                                p, parent.name
                            ),
                        )
                        .with_id(e.id.to_string())
                        .with_suggestion(format!("append {} to {}/children", e.id, parent.pointer)),
                        &format!("{}/parent", e.pointer),
                    )),
                    Some(_) => {}
                }
            }
            let mut seen = HashSet::new();
            for (i, c) in e.children.iter().enumerate() {
                let pointer = format!("{}/children/{i}", e.pointer);
                if !seen.insert(*c) {
                    out.push(anchor(
                        Diagnostic::error(
                            Code::SCENE_DUPLICATE_CHILD,
                            format!("child {c} is listed twice"),
                        )
                        .with_id(e.id.to_string()),
                        &pointer,
                    ));
                    continue;
                }
                match self.entity(*c) {
                    None => out.push(anchor(
                        Diagnostic::error(
                            Code::SCENE_UNKNOWN_CHILD,
                            format!("child {c} does not exist"),
                        )
                        .with_id(e.id.to_string())
                        .with_suggestion("remove it from children"),
                        &pointer,
                    )),
                    Some(child) if child.parent != Some(e.id) => out.push(anchor(
                        Diagnostic::error(
                            Code::SCENE_CHILD_PARENT_MISMATCH,
                            format!(
                                "child {} ({:?}) has parent {}",
                                c,
                                child.name,
                                child
                                    .parent
                                    .map(|p| p.to_string())
                                    .unwrap_or_else(|| "null".into())
                            ),
                        )
                        .with_id(e.id.to_string())
                        .with_suggestion(format!("set {}/parent to {}", child.pointer, e.id)),
                        &pointer,
                    )),
                    Some(_) => {}
                }
            }
        }

        // Cycles: follow parent links; anything that never reaches a root
        // (within N steps) is on or under a cycle. Report each cycle once,
        // at its lowest-pointer member.
        let mut on_cycle: Vec<&Entity> = Vec::new();
        for e in &self.entities {
            let mut cur = e;
            let mut steps = 0;
            let mut reached_root = false;
            while steps <= self.entities.len() {
                match cur.parent.and_then(|p| self.entity(p)) {
                    Some(p) => {
                        if p.id == e.id {
                            break; // back where we started: a cycle through e
                        }
                        cur = p;
                        steps += 1;
                    }
                    None => {
                        reached_root = true;
                        break;
                    }
                }
            }
            if !reached_root && steps <= self.entities.len() {
                on_cycle.push(e);
            }
        }
        let mut reported: HashSet<Uuid> = HashSet::new();
        for e in on_cycle {
            if reported.contains(&e.id) {
                continue;
            }
            // Collect the members of this cycle.
            let mut members = vec![e.id];
            let mut cur = e;
            while let Some(p) = cur.parent.and_then(|p| self.entity(p)) {
                if p.id == e.id {
                    break;
                }
                members.push(p.id);
                cur = p;
            }
            for m in &members {
                reported.insert(*m);
            }
            out.push(anchor(
                Diagnostic::error(
                    Code::SCENE_HIERARCHY_CYCLE,
                    format!(
                        "hierarchy cycle: {}",
                        members
                            .iter()
                            .map(|m| m.to_string())
                            .collect::<Vec<_>>()
                            .join(" -> ")
                    ),
                )
                .with_id(e.id.to_string())
                .with_suggestion("set one member's parent to null"),
                &format!("{}/parent", e.pointer),
            ));
        }

        // Components.
        for e in &self.entities {
            let mut types: HashMap<&str, &Component> = HashMap::new();
            for c in &e.components {
                if let Some(first) = types.get(c.type_id.as_str()) {
                    out.push(anchor(
                        Diagnostic::error(
                            Code::SCENE_DUPLICATE_COMPONENT,
                            format!(
                                "entity already has a {} component ({})",
                                c.type_id, first.id
                            ),
                        )
                        .with_id(c.id.to_string())
                        .with_suggestion("an entity holds one component per type; remove one"),
                        &c.pointer,
                    ));
                    continue;
                }
                types.insert(&c.type_id, c);
                match schemas.component(&c.type_id) {
                    None => out.push(anchor(
                        Diagnostic::new(
                            Code::SCENE_UNKNOWN_COMPONENT_TYPE,
                            Severity::Warning,
                            format!("unknown component type {}; its data is kept as-is", c.type_id),
                        )
                        .with_id(c.id.to_string())
                        .with_suggestion("a build fails on unknown types; install the plugin or remove the component"),
                        &format!("{}/type", c.pointer),
                    )),
                    Some(info) if !info.authored => out.push(anchor(
                        Diagnostic::error(
                            Code::SCENE_COMPONENT_NOT_AUTHORED,
                            format!("{} is engine-managed and cannot be authored", c.type_id),
                        )
                        .with_id(c.id.to_string())
                        .with_suggestion(if c.type_id == "fynkel.Name" {
                            "set the entity's name field instead".to_string()
                        } else {
                            "remove the component".to_string()
                        }),
                        &format!("{}/type", c.pointer),
                    )),
                    Some(_) => {
                        for mut d in schemas.validate_component(&c.type_id, &c.data) {
                            d.code = Code::SCENE_COMPONENT_DATA;
                            d.id = Some(c.id.to_string());
                            let pointer = format!("{}/data{}", c.pointer, d.pointer.as_deref().unwrap_or(""));
                            out.push(anchor(d, &pointer));
                        }
                    }
                }
            }
        }
        out
    }
}

fn read_entity(
    id: Uuid,
    node: &Node,
    pointer: &str,
    diags: &mut Vec<Diagnostic>,
    anchor: &dyn Fn(Diagnostic, &str) -> Diagnostic,
) -> Option<Entity> {
    let Some(_) = node.as_object() else {
        diags.push(anchor(
            Diagnostic::error(Code::SCENE_MALFORMED_ENTRY, "entity must be an object"),
            pointer,
        ));
        return None;
    };
    let name = node
        .get("name")
        .and_then(Node::as_str)
        .unwrap_or("")
        .to_string();
    let parent = match node.get("parent") {
        None
        | Some(Node {
            value: Value::Null, ..
        }) => None,
        Some(p) => match p.as_str().map(parse_uuid) {
            Some(Ok(u)) => Some(u),
            _ => {
                diags.push(anchor(
                    Diagnostic::error(
                        Code::MALFORMED_ID,
                        "parent must be null or a canonical UUID",
                    ),
                    &format!("{pointer}/parent"),
                ));
                None
            }
        },
    };
    let mut children = Vec::new();
    if let Some(items) = node.get("children").and_then(Node::as_array) {
        for (i, c) in items.iter().enumerate() {
            match c.as_str().map(parse_uuid) {
                Some(Ok(u)) => children.push(u),
                _ => diags.push(anchor(
                    Diagnostic::error(Code::MALFORMED_ID, "child must be a canonical UUID"),
                    &format!("{pointer}/children/{i}"),
                )),
            }
        }
    }
    let mut components = Vec::new();
    if let Some(Value::Object(members)) = node.get("components").map(|n| &n.value) {
        for m in members {
            let cpointer = format!("{pointer}/components/{}", escape(&m.key));
            let cid = match parse_uuid(&m.key) {
                Ok(u) => u,
                Err(why) => {
                    diags.push(anchor(
                        Diagnostic::error(
                            Code::MALFORMED_ID,
                            format!("component key is not a canonical UUID: {why}"),
                        ),
                        &cpointer,
                    ));
                    continue;
                }
            };
            let Some(type_id) = m.value.get("type").and_then(Node::as_str) else {
                diags.push(anchor(
                    Diagnostic::error(Code::SCENE_MALFORMED_ENTRY, "component needs a string type"),
                    &cpointer,
                ));
                continue;
            };
            let version = m
                .value
                .get("version")
                .and_then(Node::as_i64)
                .unwrap_or(1)
                .max(1) as u32;
            let data = m
                .value
                .get("data")
                .cloned()
                .unwrap_or_else(|| Node::object(Vec::new()));
            components.push(Component {
                id: cid,
                type_id: type_id.to_string(),
                version,
                data,
                pointer: cpointer,
            });
        }
    }
    Some(Entity {
        id,
        name,
        parent,
        children,
        components,
        pointer: pointer.to_string(),
    })
}

/// RFC 6901 token escaping.
pub fn escape(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

/// `inspect --json` payload: envelope, counts, and the tree.
pub fn inspect_json(scene: &Scene) -> serde_json::Value {
    fn tree(scene: &Scene, e: &Entity, depth: usize) -> serde_json::Value {
        let children: Vec<serde_json::Value> = e
            .children
            .iter()
            .filter_map(|c| scene.entity(*c))
            .filter(|c| c.parent == Some(e.id) && depth < 512)
            .map(|c| tree(scene, c, depth + 1))
            .collect();
        serde_json::json!({
            "id": e.id.to_string(),
            "name": e.name,
            "components": e.components.iter().map(|c| serde_json::json!({
                "id": c.id.to_string(), "type": c.type_id, "version": c.version,
            })).collect::<Vec<_>>(),
            "children": children,
        })
    }
    let mut by_type: Vec<(String, usize)> = Vec::new();
    for e in &scene.entities {
        for c in &e.components {
            match by_type.iter_mut().find(|(t, _)| *t == c.type_id) {
                Some((_, n)) => *n += 1,
                None => by_type.push((c.type_id.clone(), 1)),
            }
        }
    }
    serde_json::json!({
        "kind": "scene",
        "id": scene.document.envelope.id.to_string(),
        "name": scene.document.envelope.name,
        "formatVersion": scene.document.envelope.format_version,
        "entityCount": scene.entities.len(),
        "componentCount": scene.entities.iter().map(|e| e.components.len()).sum::<usize>(),
        "componentTypes": by_type.iter().map(|(t, n)| serde_json::json!({"type": t, "count": n})).collect::<Vec<_>>(),
        "roots": scene.roots().map(|e| tree(scene, e, 0)).collect::<Vec<_>>(),
    })
}

/// Human `inspect` output: one line per entity, indented by depth.
pub fn inspect_text(scene: &Scene) -> String {
    fn walk(scene: &Scene, e: &Entity, depth: usize, out: &mut String) {
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
            for c in e.children.iter().filter_map(|c| scene.entity(*c)) {
                if c.parent == Some(e.id) {
                    walk(scene, c, depth + 1, out);
                }
            }
        }
    }
    let mut out = format!(
        "scene {:?} ({}) formatVersion {}: {} entities\n",
        scene.document.envelope.name,
        scene.document.envelope.id,
        scene.document.envelope.format_version,
        scene.entities.len()
    );
    for r in scene.roots() {
        walk(scene, r, 0, &mut out);
    }
    out
}
