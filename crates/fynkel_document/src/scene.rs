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
//!   behaviour types known to the schema set (`schemas/behaviours/*`, or a
//!   project registry declaration) are not unknown: their data is checked
//!   like a component's and the Dart SDK instantiates them;
//! - component `data` must match the type's schema (`D0206`, via the
//!   loaded component schemas);
//! - engine-managed components may not be authored (`D0207`);
//! - prefab instance ids are unique per scene (`D0405`); resolving and
//!   expanding instances is [`crate::prefab`].
//!
//! The entity/component reading and the rules above are shared with prefab
//! documents ([`crate::prefab::Prefab`]), which add `roots`.

use std::collections::{HashMap, HashSet};

use fynkel_contracts::{AssetRef, Code, Diagnostic, Severity};
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
    /// Set when the entity is a prefab instance (scenes only).
    pub prefab: Option<PrefabInstance>,
    pub pointer: String,
}

/// `entity.prefab`: the prefab asset, the stable instance id and the
/// ordered override list.
#[derive(Clone, Debug, PartialEq)]
pub struct PrefabInstance {
    pub asset: AssetRef,
    pub instance: Uuid,
    pub overrides: Vec<Override>,
    pub pointer: String,
}

impl PrefabInstance {
    /// The prefab document id.
    pub fn prefab_id(&self) -> Uuid {
        Uuid::from_bytes(self.asset.asset.0)
    }
}

/// One override, addressed by prefab-local ids.
#[derive(Clone, Debug, PartialEq)]
pub struct Override {
    pub op: OverrideOp,
    pub pointer: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum OverrideOp {
    /// Sets a field of a component's `data` (a JSON `null` value sets null).
    SetField {
        entity: Uuid,
        component: Uuid,
        field: String,
        value: Node,
    },
    /// Removes a field from a component's `data` (distinct from null).
    UnsetField {
        entity: Uuid,
        component: Uuid,
        field: String,
    },
    AddComponent {
        entity: Uuid,
        component: Component,
    },
    RemoveComponent {
        entity: Uuid,
        component: Uuid,
    },
    /// Adds an object under `parent` (or as a new instance root when
    /// `None`), appended to the parent's children.
    AddObject {
        entity: Uuid,
        name: String,
        parent: Option<Uuid>,
        components: Vec<Component>,
    },
    /// Removes an object and its subtree.
    RemoveObject {
        entity: Uuid,
    },
}

impl OverrideOp {
    pub const fn name(&self) -> &'static str {
        match self {
            OverrideOp::SetField { .. } => "setField",
            OverrideOp::UnsetField { .. } => "unsetField",
            OverrideOp::AddComponent { .. } => "addComponent",
            OverrideOp::RemoveComponent { .. } => "removeComponent",
            OverrideOp::AddObject { .. } => "addObject",
            OverrideOp::RemoveObject { .. } => "removeObject",
        }
    }

    /// The prefab-local entity the op addresses.
    pub fn entity(&self) -> Uuid {
        match self {
            OverrideOp::SetField { entity, .. }
            | OverrideOp::UnsetField { entity, .. }
            | OverrideOp::AddComponent { entity, .. }
            | OverrideOp::RemoveComponent { entity, .. }
            | OverrideOp::AddObject { entity, .. }
            | OverrideOp::RemoveObject { entity } => *entity,
        }
    }
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
        if document.kind != DocumentKind::Scene {
            diags.push(
                Diagnostic::error(
                    Code::UNKNOWN_DOCUMENT_KIND,
                    format!("expected a scene document, got {}", document.kind.name()),
                )
                .with_pointer(""),
            );
        }
        let (entities, index) = read_entities(&document.root, &mut diags);
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
        let mut out = validate_entities(root, &self.entities, &|id| self.entity(id), schemas);
        // Instance ids must be unique: derived ids would collide otherwise.
        let mut seen: HashMap<Uuid, &Entity> = HashMap::new();
        for e in self.entities.iter().filter(|e| e.prefab.is_some()) {
            let inst = e.prefab.as_ref().unwrap();
            if let Some(first) = seen.get(&inst.instance) {
                out.push(anchor(
                    root,
                    Diagnostic::error(
                        Code::PREFAB_DUPLICATE_INSTANCE,
                        format!(
                            "instance id {} is already used by {} ({:?})",
                            inst.instance, first.id, first.name
                        ),
                    )
                    .with_id(e.id.to_string())
                    .with_suggestion("give every instance its own UUID"),
                    &format!("{}/instance", inst.pointer),
                ));
            } else {
                seen.insert(inst.instance, e);
            }
        }
        out
    }

    /// Entities that are prefab instances, document order.
    pub fn instances(&self) -> impl Iterator<Item = &Entity> {
        self.entities.iter().filter(|e| e.prefab.is_some())
    }
}

fn anchor(root: &Node, d: Diagnostic, pointer: &str) -> Diagnostic {
    let d = d.with_pointer(pointer);
    match root.pointer_span(pointer) {
        Some(s) => d.at(s.start.line, s.start.column),
        None => d,
    }
}

/// The hierarchy and component rules shared by scenes and prefabs.
pub(crate) fn validate_entities<'a>(
    root: &Node,
    entities: &'a [Entity],
    lookup: &dyn Fn(Uuid) -> Option<&'a Entity>,
    schemas: &SchemaSet,
) -> Vec<Diagnostic> {
    let anchor = |d: Diagnostic, pointer: &str| anchor(root, d, pointer);
    let mut out = Vec::new();

    // Parent/child references and agreement.
    for e in entities {
        if let Some(p) = e.parent {
            match lookup(p) {
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
            match lookup(*c) {
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
    for e in entities {
        let mut cur = e;
        let mut steps = 0;
        let mut reached_root = false;
        while steps <= entities.len() {
            match cur.parent.and_then(lookup) {
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
        if !reached_root && steps <= entities.len() {
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
        while let Some(p) = cur.parent.and_then(lookup) {
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
    for e in entities {
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
                    None if schemas.behaviour(&c.type_id).is_some() => {
                        // A Dart behaviour: its data fits the generated (or
                        // declared) schema; the SDK instantiates it.
                        for mut d in schemas.validate_component(&c.type_id, &c.data) {
                            d.code = Code::SCENE_COMPONENT_DATA;
                            d.id = Some(c.id.to_string());
                            let pointer = format!("{}/data{}", c.pointer, d.pointer.as_deref().unwrap_or(""));
                            out.push(anchor(d, &pointer));
                        }
                    }
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
                        let mut fits = true;
                        for mut d in schemas.validate_component(&c.type_id, &c.data) {
                            fits = false;
                            d.code = Code::SCENE_COMPONENT_DATA;
                            d.id = Some(c.id.to_string());
                            let pointer = format!("{}/data{}", c.pointer, d.pointer.as_deref().unwrap_or(""));
                            out.push(anchor(d, &pointer));
                        }
                        if fits {
                            physics_value_rules(c, &mut out, &anchor);
                        }
                    }
                }
        }
        physics_entity_rules(e, &types, &mut out, &anchor);
    }
    out
}

/// The physics value rules (`FYN-H0001`/`H0002`, M3-08; `H0005`/`H0007`,
/// M3-09): what the runtime rejects with `INVALID_PAYLOAD`, reported at
/// authoring time through the same `validate()`.
fn physics_value_rules(
    c: &Component,
    out: &mut Vec<Diagnostic>,
    anchor: &dyn Fn(Diagnostic, &str) -> Diagnostic,
) {
    use fynkel_core::components::ComponentValue;
    let code = match c.type_id.as_str() {
        "fynkel.RigidBody" => Code::PHYSICS_BODY_VALUE,
        "fynkel.Collider" => Code::PHYSICS_COLLIDER_VALUE,
        "fynkel.CharacterController" => Code::CHARACTER_VALUE,
        "fynkel.ThirdPersonCamera" => Code::CAMERA_RIG_VALUE,
        _ => return,
    };
    let Some(Ok(value)) = crate::generated::decode_component(&c.type_id, &c.data) else {
        return;
    };
    let result = match &value {
        ComponentValue::RigidBody(b) => b.validate(),
        ComponentValue::Collider(k) => k.validate(),
        ComponentValue::CharacterController(k) => k.validate(),
        ComponentValue::ThirdPersonCamera(k) => k.validate(),
        _ => Ok(()),
    };
    if let Err(msg) = result {
        out.push(anchor(
            Diagnostic::error(code, msg).with_id(c.id.to_string()),
            &format!("{}/data", c.pointer),
        ));
    }
}

/// The physics rules between an entity's components (`FYN-H0003`/`H0004`,
/// `H0006`).
fn physics_entity_rules(
    e: &Entity,
    types: &HashMap<&str, &Component>,
    out: &mut Vec<Diagnostic>,
    anchor: &dyn Fn(Diagnostic, &str) -> Diagnostic,
) {
    let body = types.get("fynkel.RigidBody");
    let collider = types.get("fynkel.Collider");
    let body_type = body.and_then(|b| b.data.get("body_type").and_then(Node::as_str));
    let moving = body.is_some() && body_type != Some("static");
    if let (true, Some(k)) = (moving, collider) {
        if k.data.get("shape").and_then(Node::as_str) == Some("mesh") {
            out.push(anchor(
                Diagnostic::error(
                    Code::PHYSICS_MESH_ON_MOVING_BODY,
                    "a mesh collider needs a static RigidBody (or none)",
                )
                .with_id(k.id.to_string())
                .with_suggestion("set body_type to static, or use a box, sphere or capsule"),
                &format!("{}/data/shape", k.pointer),
            ));
        }
    }
    if types.contains_key("fynkel.CharacterController") {
        for ignored in [body, collider].into_iter().flatten() {
            out.push(anchor(
                Diagnostic::error(
                    Code::CHARACTER_WITH_BODY,
                    format!(
                        "{} has a CharacterController: its own kinematic capsule replaces the {}",
                        e.name, ignored.type_id
                    ),
                )
                .with_id(ignored.id.to_string())
                .with_suggestion("remove the RigidBody/Collider, or the CharacterController"),
                &ignored.pointer,
            ));
        }
    }
    if let (Some(b), None) = (body, collider) {
        if body_type.is_none_or(|t| t == "dynamic") {
            out.push(anchor(
                Diagnostic::new(
                    Code::PHYSICS_BODY_WITHOUT_COLLIDER,
                    Severity::Warning,
                    format!(
                        "{} has a dynamic RigidBody but no Collider: it falls through everything",
                        e.name
                    ),
                )
                .with_id(b.id.to_string())
                .with_suggestion("add a fynkel.Collider"),
                &b.pointer,
            ));
        }
    }
}

/// Reads `entities` into views; malformed entries are reported and skipped.
pub(crate) fn read_entities(
    root: &Node,
    diags: &mut Vec<Diagnostic>,
) -> (Vec<Entity>, HashMap<Uuid, usize>) {
    let mut entities = Vec::new();
    let mut index = HashMap::new();
    let anchor = |d: Diagnostic, pointer: &str| anchor(root, d, pointer);
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
                if let Some(e) = read_entity(id, &m.value, &pointer, diags, &anchor) {
                    if index.insert(id, entities.len()).is_some() {
                        // Cannot happen: the strict parser rejects duplicate keys.
                        continue;
                    }
                    entities.push(e);
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
    (entities, index)
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
    let components = read_components(node.get("components"), pointer, diags, anchor);
    let prefab = node
        .get("prefab")
        .and_then(|p| read_prefab_instance(p, &format!("{pointer}/prefab"), diags, anchor));
    Some(Entity {
        id,
        name,
        parent,
        children,
        components,
        prefab,
        pointer: pointer.to_string(),
    })
}

/// Reads a `components` object (`pointer` is the owner's pointer).
fn read_components(
    node: Option<&Node>,
    pointer: &str,
    diags: &mut Vec<Diagnostic>,
    anchor: &dyn Fn(Diagnostic, &str) -> Diagnostic,
) -> Vec<Component> {
    let mut components = Vec::new();
    if let Some(Value::Object(members)) = node.map(|n| &n.value) {
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
            match read_component(cid, &m.value, &cpointer) {
                Some(c) => components.push(c),
                None => diags.push(anchor(
                    Diagnostic::error(Code::SCENE_MALFORMED_ENTRY, "component needs a string type"),
                    &cpointer,
                )),
            }
        }
    }
    components
}

fn read_component(id: Uuid, node: &Node, pointer: &str) -> Option<Component> {
    let type_id = node.get("type").and_then(Node::as_str)?;
    let version = node
        .get("version")
        .and_then(Node::as_i64)
        .unwrap_or(1)
        .max(1) as u32;
    let data = node
        .get("data")
        .cloned()
        .unwrap_or_else(|| Node::object(Vec::new()));
    Some(Component {
        id,
        type_id: type_id.to_string(),
        version,
        data,
        pointer: pointer.to_string(),
    })
}

fn read_prefab_instance(
    node: &Node,
    pointer: &str,
    diags: &mut Vec<Diagnostic>,
    anchor: &dyn Fn(Diagnostic, &str) -> Diagnostic,
) -> Option<PrefabInstance> {
    let malformed =
        |msg: &str, p: &str| anchor(Diagnostic::error(Code::PREFAB_MALFORMED_ENTRY, msg), p);
    let Some(members) = node.as_object() else {
        diags.push(malformed(
            "prefab must be an object { asset, instance, overrides? }",
            pointer,
        ));
        return None;
    };
    let asset = match crate::codec::asset_ref_field(members, "asset") {
        Ok(r) => r,
        Err(e) => {
            diags.push(malformed(
                &format!("asset: {}", e.message),
                &format!("{pointer}{}", e.pointer),
            ));
            return None;
        }
    };
    let instance = match node.get("instance").and_then(Node::as_str).map(parse_uuid) {
        Some(Ok(u)) => u,
        _ => {
            diags.push(malformed(
                "instance must be a canonical UUID",
                &format!("{pointer}/instance"),
            ));
            return None;
        }
    };
    let mut overrides = Vec::new();
    if let Some(items) = node.get("overrides").and_then(Node::as_array) {
        for (i, item) in items.iter().enumerate() {
            let p = format!("{pointer}/overrides/{i}");
            if let Some(op) = read_override(item, &p, diags, anchor) {
                overrides.push(Override { op, pointer: p });
            }
        }
    }
    Some(PrefabInstance {
        asset,
        instance,
        overrides,
        pointer: pointer.to_string(),
    })
}

pub(crate) fn read_override(
    node: &Node,
    pointer: &str,
    diags: &mut Vec<Diagnostic>,
    anchor: &dyn Fn(Diagnostic, &str) -> Diagnostic,
) -> Option<OverrideOp> {
    let mut fail = |msg: &str, p: &str| {
        diags.push(anchor(
            Diagnostic::error(Code::PREFAB_MALFORMED_ENTRY, msg),
            p,
        ));
        None
    };
    if node.as_object().is_none() {
        return fail("override must be an object", pointer);
    }
    let uuid_of = |key: &str| {
        node.get(key)
            .and_then(Node::as_str)
            .and_then(|s| parse_uuid(s).ok())
    };
    let Some(entity) = uuid_of("entity") else {
        return fail(
            "entity must be a prefab-local UUID",
            &format!("{pointer}/entity"),
        );
    };
    let Some(op) = node.get("op").and_then(Node::as_str) else {
        return fail("op is required", pointer);
    };
    let component = || uuid_of("component");
    let field = || node.get("field").and_then(Node::as_str).map(str::to_string);
    Some(match op {
        "setField" => match (component(), field(), node.get("value")) {
            (Some(component), Some(field), Some(value)) => OverrideOp::SetField {
                entity,
                component,
                field,
                value: value.clone(),
            },
            _ => return fail("setField needs component, field and value", pointer),
        },
        "unsetField" => match (component(), field()) {
            (Some(component), Some(field)) => OverrideOp::UnsetField {
                entity,
                component,
                field,
            },
            _ => return fail("unsetField needs component and field", pointer),
        },
        "addComponent" => match component().and_then(|id| read_component(id, node, pointer)) {
            Some(component) => OverrideOp::AddComponent { entity, component },
            None => {
                return fail(
                    "addComponent needs component, type, version and data",
                    pointer,
                )
            }
        },
        "removeComponent" => match component() {
            Some(component) => OverrideOp::RemoveComponent { entity, component },
            None => return fail("removeComponent needs component", pointer),
        },
        "addObject" => {
            let parent = match node.get("parent") {
                None => {
                    return fail(
                        "addObject needs parent (a prefab-local UUID or null)",
                        pointer,
                    )
                }
                Some(Node {
                    value: Value::Null, ..
                }) => None,
                Some(_) => match uuid_of("parent") {
                    Some(p) => Some(p),
                    None => {
                        return fail(
                            "parent must be a prefab-local UUID or null",
                            &format!("{pointer}/parent"),
                        )
                    }
                },
            };
            OverrideOp::AddObject {
                entity,
                name: node
                    .get("name")
                    .and_then(Node::as_str)
                    .unwrap_or("")
                    .to_string(),
                parent,
                components: read_components(node.get("components"), pointer, diags, anchor),
            }
        }
        "removeObject" => OverrideOp::RemoveObject { entity },
        other => return fail(&format!("unknown op {other:?}"), &format!("{pointer}/op")),
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
        let mut v = serde_json::json!({
            "id": e.id.to_string(),
            "name": e.name,
            "components": e.components.iter().map(|c| serde_json::json!({
                "id": c.id.to_string(), "type": c.type_id, "version": c.version,
            })).collect::<Vec<_>>(),
            "children": children,
        });
        if let Some(p) = &e.prefab {
            v["prefab"] = serde_json::json!({
                "asset": p.prefab_id().to_string(),
                "instance": p.instance.to_string(),
                "overrides": p.overrides.len(),
            });
        }
        v
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
        "revision": crate::authoring::Revision::of(&scene.document).map(|r| r.to_string()).ok(),
        "entityCount": scene.entities.len(),
        "instanceCount": scene.instances().count(),
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
            "{}{} ({}){}{}\n",
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
            },
            match &e.prefab {
                Some(p) => format!(
                    "  <prefab {} instance {}, {} override(s)>",
                    p.prefab_id(),
                    p.instance,
                    p.overrides.len()
                ),
                None => String::new(),
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
