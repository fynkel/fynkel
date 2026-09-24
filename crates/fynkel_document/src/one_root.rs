//! `scene/4-5/one-root` and `prefab/4-5/one-root` (ADR-0022): a prefab has
//! one root, and an instance is that root.
//!
//! An instance cannot move to the new model without its prefab, so the
//! work is a project [`Rewrite`](crate::Rewrite) run when a document's
//! chain reaches version 4 ([`Plan::rewrite`], wired by
//! `fynkel_import::keys_migration::migrate_documents`); the per-document
//! step that follows ([`step_4_to_5`]) only finishes what needs no other
//! file and refuses what does.
//!
//! - **A prefab with several roots** gets a new root named after the
//!   prefab (id `uuid_v5(prefab id, "root")`, the default Transform); the
//!   old roots become its children. `roots` becomes `root`.
//! - **An anchor** (an entity carrying `prefab`) loses its components:
//!   its Transform composed with the prefab root's (as the instance saw
//!   it) is written as `setField` overrides on the root's Transform, only
//!   the fields that differ from the prefab; its other components become
//!   `addComponent` overrides on the root. Its own children, and objects
//!   an `addObject` added at the top (parent null, now the root), were
//!   placed relative to the anchor and are now relative to the root: their
//!   Transforms are corrected by the root's inverse. A `setActive` on the
//!   root folds into the instance's own flag, and a string equal to the
//!   id the old root derived becomes the instance's id.
//! - **Refused** (the document is left as it was, with the reason): a
//!   composition that is not a position/rotation/scale (non-uniform scale
//!   under a rotation), an anchor component whose type the root already
//!   has, a `removeObject` of the root, a prefab the project does not have.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::json::{Member, Node, Span, Value};
use crate::prefab::{derive_id, Prefab};
use crate::scene::{Entity, OverrideOp, Scene};
use crate::{parse_uuid, Document, DocumentKind};

/// The rewrite's name in `applied[]`.
pub const REWRITE: &str = "scene+prefab/4-5/one-root (project)";

pub(crate) const TRANSFORM: &str = "fynkel.Transform";

/// The new root of a prefab that had several.
pub fn wrapper_root(prefab: Uuid) -> Uuid {
    Uuid::new_v5(&prefab, b"root")
}

/// Its Transform component.
pub fn wrapper_transform(prefab: Uuid) -> Uuid {
    Uuid::new_v5(&prefab, b"root/transform")
}

// ------------------------------------------------------------------ math

/// A local transform: position, rotation `[x, y, z, w]`, scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Trs {
    p: [f64; 3],
    q: [f64; 4],
    s: [f64; 3],
}

const EPS: f64 = 1e-6;

impl Trs {
    const IDENTITY: Trs = Trs {
        p: [0.0; 3],
        q: [0.0, 0.0, 0.0, 1.0],
        s: [1.0; 3],
    };

    fn read(data: &Node) -> Trs {
        let mut t = Trs::IDENTITY;
        let nums = |key: &str, out: &mut [f64]| {
            if let Some(items) = data.get(key).and_then(Node::as_array) {
                if items.len() == out.len() {
                    for (o, n) in out.iter_mut().zip(items) {
                        if let Value::Number { value, .. } = n.value {
                            *o = value;
                        }
                    }
                }
            }
        };
        nums("position", &mut t.p);
        nums("rotation", &mut t.q);
        nums("scale", &mut t.s);
        t
    }

    fn field(&self, key: &str) -> Vec<f64> {
        match key {
            "position" => self.p.to_vec(),
            "rotation" => self.q.to_vec(),
            _ => self.s.to_vec(),
        }
    }

    fn is_identity(&self) -> bool {
        close(&self.p, &[0.0; 3]) && self.rotation_is_identity() && close(&self.s, &[1.0; 3])
    }

    fn rotation_is_identity(&self) -> bool {
        self.q[..3].iter().all(|v| v.abs() < EPS)
    }

    fn uniform(&self) -> bool {
        let max = self.s.iter().fold(0f64, |m, v| m.max(v.abs()));
        (self.s[0] - self.s[1]).abs() <= EPS * max.max(1.0)
            && (self.s[0] - self.s[2]).abs() <= EPS * max.max(1.0)
    }

    /// `self * child`, when that is a position/rotation/scale.
    fn then(&self, child: &Trs) -> Option<Trs> {
        if !self.uniform() && !child.rotation_is_identity() {
            return None;
        }
        let scaled = [
            self.s[0] * child.p[0],
            self.s[1] * child.p[1],
            self.s[2] * child.p[2],
        ];
        let r = rotate(&self.q, &scaled);
        Some(Trs {
            p: [self.p[0] + r[0], self.p[1] + r[1], self.p[2] + r[2]],
            q: mul(&self.q, &child.q),
            s: [
                self.s[0] * child.s[0],
                self.s[1] * child.s[1],
                self.s[2] * child.s[2],
            ],
        })
    }

    /// The inverse, when it is a position/rotation/scale.
    fn inverse(&self) -> Option<Trs> {
        if self.s.iter().any(|v| v.abs() < 1e-12) {
            return None;
        }
        if !self.uniform() && !self.rotation_is_identity() {
            return None;
        }
        let qi = [-self.q[0], -self.q[1], -self.q[2], self.q[3]];
        let s = [1.0 / self.s[0], 1.0 / self.s[1], 1.0 / self.s[2]];
        let r = rotate(&qi, &self.p);
        Some(Trs {
            p: [-r[0] * s[0], -r[1] * s[1], -r[2] * s[2]],
            q: qi,
            s,
        })
    }
}

fn close(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < EPS)
}

fn mul(a: &[f64; 4], b: &[f64; 4]) -> [f64; 4] {
    let [ax, ay, az, aw] = *a;
    let [bx, by, bz, bw] = *b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

fn rotate(q: &[f64; 4], v: &[f64; 3]) -> [f64; 3] {
    let p = mul(
        &mul(q, &[v[0], v[1], v[2], 0.0]),
        &[-q[0], -q[1], -q[2], q[3]],
    );
    [p[0], p[1], p[2]]
}

/// A number as the editor writes it, rounded to a nanometre-ish precision
/// (no `0.30000000000000004`); `-0.0` is `0.0`.
fn number(v: f64) -> Node {
    let r = (v * 1e9).round() / 1e9;
    Node::float(if r == 0.0 { 0.0 } else { r })
}

fn numbers(values: &[f64]) -> Node {
    Node::synthetic(Value::Array(values.iter().map(|v| number(*v)).collect()))
}

fn write_trs(data: &mut Node, t: &Trs) {
    data.set("position", numbers(&t.p));
    data.set("rotation", numbers(&t.q));
    data.set("scale", numbers(&t.s));
}

// ------------------------------------------------------------------ roots

/// A prefab's root as the new model sees it.
#[derive(Clone, Debug)]
struct Root {
    /// Prefab-local id (the old single root, or the new wrapper).
    id: Uuid,
    /// The prefab had several roots and gets a wrapper.
    wrapped: bool,
    /// The root's Transform: its prefab-local component id and value.
    transform: Option<(Uuid, Trs)>,
    /// The component types on the root.
    types: HashSet<String>,
    active: bool,
}

/// How one anchor becomes an instance.
struct Anchored {
    /// The Transform the instance ends with (the root's, composed).
    transform: Option<(Uuid, Trs)>,
    /// Overrides after the migration.
    overrides: Vec<Node>,
    active: bool,
    /// Where the anchor's children used to be placed from, seen from the
    /// root: their Transforms are corrected by its inverse.
    correction: Trs,
    types: HashSet<String>,
}

/// The project context of the rewrite: every prefab document, and the
/// hierarchies imported models derive.
pub struct Plan<'a> {
    prefabs: HashMap<Uuid, &'a Document>,
    imported: &'a dyn Fn(Uuid, &str) -> Option<Prefab>,
    memo: RefCell<HashMap<(Uuid, String), Result<Root, String>>>,
    busy: RefCell<HashSet<(Uuid, String)>>,
}

impl<'a> Plan<'a> {
    /// `documents` is the project (the prefabs are picked from it);
    /// `imported` returns the hierarchy an imported model (or a rig)
    /// derives for `{ asset, sub: "prefab" }`, already one-rooted.
    pub fn new(
        documents: impl IntoIterator<Item = &'a Document>,
        imported: &'a dyn Fn(Uuid, &str) -> Option<Prefab>,
    ) -> Plan<'a> {
        Plan {
            prefabs: documents
                .into_iter()
                .filter(|d| d.kind == DocumentKind::Prefab)
                .map(|d| (d.envelope.id, d))
                .collect(),
            imported,
            memo: RefCell::new(HashMap::new()),
            busy: RefCell::new(HashSet::new()),
        }
    }

    /// The rewrite for one document: a scene or prefab below version 5 is
    /// brought to the new model (the version is left to the step). True
    /// when something changed.
    pub fn rewrite(&self, doc: &mut Document) -> Result<bool, String> {
        if !matches!(doc.kind, DocumentKind::Scene | DocumentKind::Prefab)
            || doc.envelope.format_version >= 5
        {
            return Ok(false);
        }
        let mut changed = false;
        if doc.kind == DocumentKind::Prefab {
            changed |= wrap_roots(doc)?;
        }
        changed |= self.anchors(doc)?;
        Ok(changed)
    }

    /// The root of what `asset`/`sub` names.
    fn root(&self, asset: Uuid, sub: &str) -> Result<Root, String> {
        let key = (asset, sub.to_string());
        if let Some(r) = self.memo.borrow().get(&key) {
            return r.clone();
        }
        if !self.busy.borrow_mut().insert(key.clone()) {
            return Err(format!("prefab {asset} instantiates itself"));
        }
        let r = self.compute_root(asset, sub);
        self.busy.borrow_mut().remove(&key);
        self.memo.borrow_mut().insert(key, r.clone());
        r
    }

    fn compute_root(&self, asset: Uuid, sub: &str) -> Result<Root, String> {
        if !sub.is_empty() {
            let prefab = (self.imported)(asset, sub).ok_or_else(|| {
                format!(
                    "the hierarchy of {asset}#{sub} is not available (the model does not import)"
                )
            })?;
            return self.root_of(&prefab.document);
        }
        let doc = self
            .prefabs
            .get(&asset)
            .ok_or_else(|| format!("prefab {asset} is not in the project"))?;
        self.root_of(doc)
    }

    /// The root of a prefab document at any version.
    fn root_of(&self, doc: &Document) -> Result<Root, String> {
        let prefab = Prefab::from_document(doc.clone());
        let roots = declared_roots(&doc.root);
        if roots.len() != 1 {
            let id = doc.envelope.id;
            return Ok(Root {
                id: wrapper_root(id),
                wrapped: true,
                transform: Some((wrapper_transform(id), Trs::IDENTITY)),
                types: HashSet::from([TRANSFORM.to_string()]),
                active: true,
            });
        }
        let e = prefab.entity(roots[0]).ok_or_else(|| {
            format!(
                "root {} of prefab {} is not an entity",
                roots[0], doc.envelope.id
            )
        })?;
        let Some(inst) = &e.prefab else {
            return Ok(Root {
                id: e.id,
                wrapped: false,
                transform: e
                    .component_of(TRANSFORM)
                    .map(|c| (c.id, Trs::read(&c.data))),
                types: e.components.iter().map(|c| c.type_id.clone()).collect(),
                active: e.active,
            });
        };
        // A nested instance is the root: seen through it.
        let inner = self.root(inst.prefab_id(), &inst.asset.sub)?;
        let a = self.anchored(e, &inner, &format!("prefab {}", doc.envelope.id))?;
        Ok(Root {
            id: e.id,
            wrapped: false,
            transform: a.transform.map(|(c, t)| (derive_id(inst.instance, c), t)),
            types: a.types,
            active: a.active,
        })
    }

    /// Every anchor of `doc` becomes an instance.
    fn anchors(&self, doc: &mut Document) -> Result<bool, String> {
        let shown = doc
            .path
            .clone()
            .unwrap_or_else(|| doc.envelope.id.to_string());
        let scene = Scene::from_document(Document {
            kind: DocumentKind::Scene,
            ..doc.clone()
        });
        let mut plans = Vec::new();
        for e in scene.instances() {
            let inst = e.prefab.as_ref().unwrap();
            match self.root(inst.prefab_id(), &inst.asset.sub) {
                Ok(root) => plans.push((e.clone(), root)),
                // An instance that needs nothing of its prefab stays as it
                // is (its prefab's problem is reported by validation).
                Err(_) if !needs_prefab(e) => {}
                Err(why) => {
                    return Err(format!("{shown}: instance {:?} ({}): {why}", e.name, e.id))
                }
            }
        }
        if plans.is_empty() {
            return Ok(false);
        }
        // The anchors' children first: a child that is an anchor itself is
        // composed with its corrected Transform.
        let mut corrections: Vec<(Uuid, Trs)> = Vec::new();
        for (e, root) in &plans {
            let a = self.anchored(e, root, &shown)?;
            if !a.correction.is_identity() {
                let inv = a.correction.inverse().ok_or_else(|| {
                    format!(
                        "{shown}: instance {:?} ({}): its prefab's root Transform has no inverse position/rotation/scale, so the children placed under the instance cannot keep their place",
                        e.name, e.id
                    )
                })?;
                for c in &e.children {
                    corrections.push((*c, inv));
                }
            }
        }
        for (child, inv) in &corrections {
            correct(doc, &scene, *child, inv, &shown)?;
        }
        let scene = Scene::from_document(Document {
            kind: DocumentKind::Scene,
            ..doc.clone()
        });
        let mut rename: HashMap<String, String> = HashMap::new();
        for (old, root) in &plans {
            let e = scene.entity(old.id).expect("same entities");
            let a = self.anchored(e, root, &shown)?;
            let inst = e.prefab.as_ref().unwrap();
            let node = doc
                .root
                .get_mut("entities")
                .and_then(|n| n.get_mut(&e.id.to_string()))
                .expect("the anchor is an entity");
            node.set("components", Node::object(Vec::new()));
            if a.active != e.active {
                node.set("active", Node::synthetic(Value::Bool(a.active)));
            }
            let prefab = node.get_mut("prefab").expect("an anchor");
            if !a.overrides.is_empty() || prefab.get("overrides").is_some() {
                prefab.set("overrides", Node::synthetic(Value::Array(a.overrides)));
            }
            if !root.wrapped {
                rename.insert(
                    derive_id(inst.instance, root.id).to_string(),
                    e.id.to_string(),
                );
            }
        }
        if !rename.is_empty() {
            if let Some(entities) = doc.root.get_mut("entities") {
                rename_strings(entities, &rename);
            }
        }
        Ok(true)
    }

    /// What anchor `e` of a prefab whose root is `root` becomes.
    fn anchored(&self, e: &Entity, root: &Root, shown: &str) -> Result<Anchored, String> {
        let inst = e.prefab.as_ref().expect("an anchor");
        let refuse = |why: String| format!("{shown}: instance {:?} ({}): {why}", e.name, e.id);
        let mut active = e.active && root.active;
        let mut types = root.types.clone();
        // The root's Transform as this instance saw it (its overrides on
        // it are replaced below).
        let mut seen = root.transform;
        for o in &inst.overrides {
            match &o.op {
                OverrideOp::SetField {
                    entity,
                    component,
                    field,
                    value,
                } if *entity == root.id
                    && seen.is_some_and(|(c, _)| c == *component)
                    && matches!(field.as_str(), "position" | "rotation" | "scale") =>
                {
                    let (c, t) = seen.unwrap();
                    let mut data = Node::object(Vec::new());
                    write_trs(&mut data, &t);
                    data.set(field, value.clone());
                    seen = Some((c, Trs::read(&data)));
                }
                OverrideOp::AddComponent { entity, component }
                    if *entity == root.id && component.type_id == TRANSFORM && seen.is_none() =>
                {
                    seen = Some((component.id, Trs::read(&component.data)));
                }
                OverrideOp::RemoveComponent { entity, component }
                    if *entity == root.id && seen.is_some_and(|(c, _)| c == *component) =>
                {
                    seen = None;
                }
                _ => {}
            }
        }
        let correction = seen.map(|(_, t)| t).unwrap_or(Trs::IDENTITY);
        // Objects added at the top were placed from the anchor too.
        let inv = if correction.is_identity() {
            None
        } else {
            Some(correction.inverse().ok_or_else(|| {
                refuse("its prefab's root Transform has no inverse position/rotation/scale".into())
            })?)
        };
        let mut overrides = Vec::new();
        for o in &inst.overrides {
            match &o.op {
                OverrideOp::SetActive { entity, value } if *entity == root.id => {
                    active &= *value;
                }
                OverrideOp::RemoveObject { entity } if *entity == root.id => {
                    return Err(refuse(
                        "it removes the prefab's root; unpack it or remove the override first"
                            .into(),
                    ));
                }
                OverrideOp::AddComponent { entity, component } if *entity == root.id => {
                    types.insert(component.type_id.clone());
                    overrides.push(node_of(o));
                }
                OverrideOp::AddObject { parent: None, .. } => {
                    let mut n = node_of(o);
                    n.set("parent", Node::string(root.id.to_string()));
                    if let Some(inv) = &inv {
                        correct_components(&mut n, inv).map_err(refuse)?;
                    }
                    overrides.push(n);
                }
                _ => overrides.push(node_of(o)),
            }
        }

        let own_transform = e.component_of(TRANSFORM);
        let mut transform = seen;
        if let Some(a) = own_transform {
            let a_trs = Trs::read(&a.data);
            match seen {
                Some((c, t)) => {
                    let composed = a_trs.then(&t).ok_or_else(|| {
                        refuse("its Transform has a non-uniform scale and the prefab's root is rotated: the two do not make one position/rotation/scale".into())
                    })?;
                    transform = Some((c, composed));
                }
                None => {
                    types.insert(TRANSFORM.to_string());
                    transform = Some((a.id, a_trs));
                    overrides.push(add_component(root.id, a));
                }
            }
        }
        // The root's Transform fields: overrides where the result differs
        // from the prefab.
        if let (Some((c, t)), Some((rc, base))) = (transform, root.transform) {
            if c == rc {
                overrides.retain(|n| {
                    !(n.get("op").and_then(Node::as_str) == Some("setField")
                        && n.get("entity").and_then(Node::as_str) == Some(&root.id.to_string())
                        && n.get("component").and_then(Node::as_str) == Some(&c.to_string())
                        && matches!(
                            n.get("field").and_then(Node::as_str),
                            Some("position" | "rotation" | "scale")
                        ))
                });
                for field in ["position", "rotation", "scale"] {
                    let (now, then) = (t.field(field), base.field(field));
                    if !close(&now, &then) {
                        overrides.push(Node::object(vec![
                            ("op".into(), Node::string("setField")),
                            ("entity".into(), Node::string(root.id.to_string())),
                            ("component".into(), Node::string(c.to_string())),
                            ("field".into(), Node::string(field)),
                            ("value".into(), numbers(&now)),
                        ]));
                    }
                }
            }
        }
        for c in e.components.iter().filter(|c| c.type_id != TRANSFORM) {
            if !types.insert(c.type_id.clone()) {
                return Err(refuse(format!(
                    "it has a {} and so does its prefab's root; one object holds one of each",
                    c.type_id
                )));
            }
            overrides.push(add_component(root.id, c));
        }
        Ok(Anchored {
            transform,
            overrides,
            active,
            correction,
            types,
        })
    }
}

/// Whether anchor `e` changes with its prefab's root: components of its
/// own, or objects added at its top.
fn needs_prefab(e: &Entity) -> bool {
    !e.components.is_empty()
        || e.prefab.as_ref().is_some_and(|p| {
            p.overrides
                .iter()
                .any(|o| matches!(o.op, OverrideOp::AddObject { parent: None, .. }))
        })
}

/// The override node as written.
fn node_of(o: &crate::scene::Override) -> Node {
    op_node(&o.op)
}

fn op_node(op: &OverrideOp) -> Node {
    let s = |u: &Uuid| Node::string(u.to_string());
    let component = |c: &crate::scene::Component| -> Vec<(String, Node)> {
        vec![
            ("type".into(), Node::string(c.type_id.clone())),
            ("version".into(), Node::integer(c.version as i64)),
            ("data".into(), c.data.clone()),
        ]
    };
    let mut m: Vec<(String, Node)> = vec![("op".into(), Node::string(op.name()))];
    match op {
        OverrideOp::SetField {
            entity,
            component,
            field,
            value,
        } => m.extend([
            ("entity".into(), s(entity)),
            ("component".into(), s(component)),
            ("field".into(), Node::string(field.clone())),
            ("value".into(), value.clone()),
        ]),
        OverrideOp::UnsetField {
            entity,
            component,
            field,
        } => m.extend([
            ("entity".into(), s(entity)),
            ("component".into(), s(component)),
            ("field".into(), Node::string(field.clone())),
        ]),
        OverrideOp::AddComponent {
            entity,
            component: c,
        } => {
            m.extend([("entity".into(), s(entity)), ("component".into(), s(&c.id))]);
            m.extend(component(c));
        }
        OverrideOp::RemoveComponent { entity, component } => m.extend([
            ("entity".into(), s(entity)),
            ("component".into(), s(component)),
        ]),
        OverrideOp::AddObject {
            entity,
            name,
            parent,
            components,
        } => m.extend([
            ("entity".into(), s(entity)),
            ("name".into(), Node::string(name.clone())),
            (
                "parent".into(),
                parent
                    .map(|p| s(&p))
                    .unwrap_or(Node::synthetic(Value::Null)),
            ),
            (
                "components".into(),
                Node::object(
                    components
                        .iter()
                        .map(|c| (c.id.to_string(), Node::object(component(c))))
                        .collect(),
                ),
            ),
        ]),
        OverrideOp::RemoveObject { entity } => m.push(("entity".into(), s(entity))),
        OverrideOp::SetActive { entity, value } => m.extend([
            ("entity".into(), s(entity)),
            ("value".into(), Node::synthetic(Value::Bool(*value))),
        ]),
    }
    Node::object(m)
}

fn add_component(root: Uuid, c: &crate::scene::Component) -> Node {
    op_node(&OverrideOp::AddComponent {
        entity: root,
        component: c.clone(),
    })
}

/// `roots` (formatVersion ≤ 4) or `root`.
fn declared_roots(root: &Node) -> Vec<Uuid> {
    if let Some(r) = root
        .get("root")
        .and_then(Node::as_str)
        .and_then(|s| parse_uuid(s).ok())
    {
        return vec![r];
    }
    root.get("roots")
        .and_then(Node::as_array)
        .into_iter()
        .flatten()
        .filter_map(|n| n.as_str().and_then(|s| parse_uuid(s).ok()))
        .collect()
}

/// `roots` becomes `root`; several get a wrapper named after the prefab.
fn wrap_roots(doc: &mut Document) -> Result<bool, String> {
    let Some(pos) = doc
        .root
        .as_object()
        .and_then(|m| m.iter().position(|m| m.key == "roots"))
    else {
        return Ok(false);
    };
    let roots = declared_roots(&doc.root);
    let members = doc.root.as_object_mut().expect("a document is an object");
    let root = match roots.as_slice() {
        [one] => *one,
        _ => {
            let id = doc.envelope.id;
            let w = wrapper_root(id);
            let transform = Node::object(vec![
                ("type".into(), Node::string(TRANSFORM)),
                ("version".into(), Node::integer(1)),
                ("data".into(), crate::prefab::identity_transform()),
            ]);
            let entity = Node::object(vec![
                ("name".into(), Node::string(doc.envelope.name.clone())),
                ("parent".into(), Node::synthetic(Value::Null)),
                (
                    "children".into(),
                    Node::synthetic(Value::Array(
                        roots.iter().map(|r| Node::string(r.to_string())).collect(),
                    )),
                ),
                (
                    "components".into(),
                    Node::object(vec![(wrapper_transform(id).to_string(), transform)]),
                ),
            ]);
            let entities = members
                .iter_mut()
                .find(|m| m.key == "entities")
                .and_then(|m| m.value.as_object_mut())
                .ok_or("the prefab has no entities")?;
            for r in &roots {
                if let Some(e) = entities.iter_mut().find(|m| m.key == r.to_string()) {
                    e.value.set("parent", Node::string(w.to_string()));
                }
            }
            entities.insert(
                0,
                Member {
                    key: w.to_string(),
                    key_span: Span::default(),
                    value: entity,
                },
            );
            w
        }
    };
    let members = doc.root.as_object_mut().expect("a document is an object");
    members[pos] = Member {
        key: "root".into(),
        key_span: Span::default(),
        value: Node::string(root.to_string()),
    };
    Ok(true)
}

/// `inv * child` for the Transform of entity `id` (added when it has none).
fn correct(
    doc: &mut Document,
    scene: &Scene,
    id: Uuid,
    inv: &Trs,
    shown: &str,
) -> Result<(), String> {
    let Some(e) = scene.entity(id) else {
        return Ok(());
    };
    let node = doc
        .root
        .get_mut("entities")
        .and_then(|n| n.get_mut(&id.to_string()))
        .expect("a child is an entity");
    correct_entity(node, e.component_of(TRANSFORM).map(|c| c.id), id, inv).map_err(|why| {
        format!(
            "{shown}: {:?} ({id}), a child of a prefab instance: {why}",
            e.name
        )
    })
}

fn correct_entity(
    node: &mut Node,
    transform: Option<Uuid>,
    id: Uuid,
    inv: &Trs,
) -> Result<(), String> {
    let components = match node.get_mut("components") {
        Some(c) => c,
        None => {
            node.set("components", Node::object(Vec::new()));
            node.get_mut("components").unwrap()
        }
    };
    correct_in(components, transform, Uuid::new_v5(&id, b"transform"), inv)
}

/// The Transform among an `addObject`'s components.
fn correct_components(op: &mut Node, inv: &Trs) -> Result<(), String> {
    let id = op
        .get("entity")
        .and_then(Node::as_str)
        .and_then(|s| parse_uuid(s).ok())
        .unwrap_or_default();
    let transform = op
        .get("components")
        .and_then(Node::as_object)
        .and_then(|m| {
            m.iter()
                .find(|m| m.value.get("type").and_then(Node::as_str) == Some(TRANSFORM))
                .and_then(|m| parse_uuid(&m.key).ok())
        });
    let components = match op.get_mut("components") {
        Some(c) => c,
        None => {
            op.set("components", Node::object(Vec::new()));
            op.get_mut("components").unwrap()
        }
    };
    correct_in(components, transform, Uuid::new_v5(&id, b"transform"), inv)
}

fn correct_in(
    components: &mut Node,
    transform: Option<Uuid>,
    fresh: Uuid,
    inv: &Trs,
) -> Result<(), String> {
    let why = "its Transform under the prefab's root is not a position/rotation/scale (a non-uniform scale meets a rotation)";
    match transform {
        Some(c) => {
            let data = components
                .get_mut(&c.to_string())
                .and_then(|n| n.get_mut("data"))
                .ok_or("its Transform has no data")?;
            let t = inv.then(&Trs::read(data)).ok_or(why)?;
            write_trs(data, &t);
        }
        None => {
            let mut data = crate::prefab::identity_transform();
            let t = inv.then(&Trs::IDENTITY).ok_or(why)?;
            write_trs(&mut data, &t);
            let members = components
                .as_object_mut()
                .ok_or("components is not an object")?;
            members.push(Member {
                key: fresh.to_string(),
                key_span: Span::default(),
                value: Node::object(vec![
                    ("type".into(), Node::string(TRANSFORM)),
                    ("version".into(), Node::integer(1)),
                    ("data".into(), data),
                ]),
            });
        }
    }
    Ok(())
}

fn rename_strings(node: &mut Node, map: &HashMap<String, String>) {
    match &mut node.value {
        Value::String(s) => {
            if let Some(new) = map.get(s.as_str()) {
                *s = new.clone();
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|n| rename_strings(n, map)),
        Value::Object(members) => members
            .iter_mut()
            .for_each(|m| rename_strings(&mut m.value, map)),
        _ => {}
    }
}

/// The per-document step: what needs no other file. A prefab's single
/// `roots` entry becomes `root`; a prefab with several roots, or an anchor
/// with components of its own, needs the project ([`Plan`]) and is refused.
pub fn step_4_to_5(root: &mut Node) -> Result<(), String> {
    let project =
        "run `fynkel migrate` on the project directory: it reads the prefabs this document needs";
    if root.get("roots").is_some() {
        let roots = declared_roots(root);
        let [one] = roots.as_slice() else {
            return Err(format!(
                "the prefab has {} roots and gets a new root over them; {project}",
                roots.len()
            ));
        };
        let one = *one;
        let members = root.as_object_mut().expect("a document is an object");
        let pos = members.iter().position(|m| m.key == "roots").unwrap();
        members[pos] = Member {
            key: "root".into(),
            key_span: Span::default(),
            value: Node::string(one.to_string()),
        };
    }
    let anchored = root
        .get("entities")
        .and_then(Node::as_object)
        .into_iter()
        .flatten()
        .filter(|m| m.value.get("prefab").is_some())
        .find(|m| {
            let own = m
                .value
                .get("components")
                .and_then(Node::as_object)
                .is_some_and(|c| !c.is_empty());
            let top = m
                .value
                .get("prefab")
                .and_then(|p| p.get("overrides"))
                .and_then(Node::as_array)
                .into_iter()
                .flatten()
                .any(|o| {
                    o.get("op").and_then(Node::as_str) == Some("addObject")
                        && o.get("parent")
                            .is_some_and(|p| matches!(p.value, Value::Null))
                });
            own || top
        });
    if let Some(m) = anchored {
        return Err(format!(
            "prefab instance {} has components of its own (or objects added at its top), which become overrides of its prefab's root; {project}",
            m.key
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trs(p: [f64; 3], q: [f64; 4], s: [f64; 3]) -> Trs {
        Trs { p, q, s }
    }

    #[test]
    fn composition_and_inverse_agree() {
        let half = std::f64::consts::FRAC_1_SQRT_2;
        let a = trs([1.0, 2.0, 3.0], [0.0, half, 0.0, half], [2.0, 2.0, 2.0]);
        let b = trs([1.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0], [1.0, 3.0, 1.0]);
        let ab = a.then(&b).unwrap();
        // 90° about y takes +x to -z; scaled by 2.
        assert!(close(&ab.p, &[1.0, 2.0, 1.0]), "{:?}", ab.p);
        assert!(close(&ab.s, &[2.0, 6.0, 2.0]));
        let back = a.inverse().unwrap().then(&ab).unwrap();
        assert!(close(&back.p, &b.p) && close(&back.q, &b.q) && close(&back.s, &b.s));
    }

    #[test]
    fn a_non_uniform_scale_over_a_rotation_is_refused() {
        let half = std::f64::consts::FRAC_1_SQRT_2;
        let a = trs([0.0; 3], [0.0, 0.0, 0.0, 1.0], [1.0, 2.0, 1.0]);
        let b = trs([0.0; 3], [0.0, half, 0.0, half], [1.0; 3]);
        assert!(a.then(&b).is_none());
        assert!(b.then(&a).is_some());
    }
}
