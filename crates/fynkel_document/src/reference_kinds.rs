//! References of the wrong kind (`FYN-A0007`, ADR-0027 §4): a component
//! field that names an asset of one kind — a script's `asset(<kind>)`
//! hint, a list's item hint, or a built-in field in [`NATIVE`] — holding
//! a reference to an asset of another kind. Before this check such a
//! reference passed `fynkel validate` and failed only at runtime.
//!
//! What a target *is* follows the editor's pickers
//! (`AssetEntry.offeredKinds`): a document is its kind; a source's
//! sub-asset is its key's head (`mesh/0` → `mesh`, `anim/0` → `clip`,
//! `image/0` → `texture`); a whole audio source is a `sound`, a whole
//! image a `texture`. A reference the database cannot resolve is left to
//! [`AssetDatabase::check_references`] (`A0004`/`A0005`).

use fynkel_contracts::diagnostic::{Code, Diagnostic};

use crate::asset::{asset_refs, AssetDatabase, AssetKind, Resolved};
use crate::json::{Locator, Node};
use crate::scene::escape;
use crate::schema::SchemaSet;
use crate::{Document, DocumentKind};

/// One field (or list of fields) of a type that names assets of `kinds`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetField {
    /// Keys from the type's `data`; `*` is every item of an array.
    pub path: Vec<String>,
    /// The kinds it accepts (normally one).
    pub kinds: Vec<String>,
}

/// The built-in components' asset fields: their schemas are generated
/// from Rust and carry no `kind` hints (the editor keeps the same table
/// in `native_hints.dart`).
pub const NATIVE: &[(&str, &str, &[&str])] = &[
    ("fynkel.MeshRenderer", "mesh", &["mesh"]),
    ("fynkel.MeshRenderer", "material", &["material"]),
    ("fynkel.Collider", "mesh", &["mesh"]),
    ("fynkel.Collider", "material", &["physics_material"]),
    ("fynkel.AudioSource", "clip", &["sound"]),
    ("fynkel.Environment", "sky", &["texture"]),
    ("fynkel.Volume", "profile", &["volume"]),
    ("fynkel.Skin", "skin", &["skin"]),
    ("fynkel.Animator", "clip", &["clip", "blend"]),
    ("fynkel.Emitter", "prefab", &["prefab"]),
];

/// A kind as the rest of the tools name it: a script's `audio` is `sound`.
fn normal(kind: &str) -> String {
    match kind {
        "audio" => "sound".into(),
        k => k.into(),
    }
}

/// Every `asset(<kind>)` hint in a component or behaviour schema, at its
/// place in the data: a property's own `x-fynkel.kind`, an array's
/// `items`, a nested object's properties.
pub fn asset_fields_of(schema: &serde_json::Value) -> Vec<AssetField> {
    let mut out = Vec::new();
    if let Some(props) = schema.get("properties").and_then(|p| p.as_object()) {
        for (name, p) in props {
            walk(p, vec![name.clone()], &mut out);
        }
    }
    out
}

fn walk(p: &serde_json::Value, path: Vec<String>, out: &mut Vec<AssetField>) {
    let hint = p
        .get("x-fynkel")
        .and_then(|x| x.get("kind"))
        .and_then(|k| k.as_str());
    if let Some(kind) = hint
        .and_then(|k| k.strip_prefix("asset("))
        .and_then(|k| k.strip_suffix(')'))
    {
        out.push(AssetField {
            path,
            kinds: vec![normal(kind)],
        });
        return;
    }
    if let Some(items) = p.get("items").filter(|i| i.is_object()) {
        let mut inner = path.clone();
        inner.push("*".into());
        walk(items, inner, out);
    }
    if let Some(props) = p.get("properties").and_then(|v| v.as_object()) {
        for (name, child) in props {
            let mut inner = path.clone();
            inner.push(name.clone());
            walk(child, inner, out);
        }
    }
}

/// The kinds a resolved reference can stand for.
pub fn offered_kinds(r: &Resolved<'_>) -> Vec<String> {
    if let Some((sub, _)) = &r.sub {
        let head = sub.split('/').next().unwrap_or(sub);
        return vec![match head {
            "anim" => "clip",
            "image" => "texture",
            h => h,
        }
        .to_string()];
    }
    match &r.entry.kind {
        AssetKind::Document(k) => vec![k.name().to_string()],
        AssetKind::Source { importer, .. } => match importer.as_str() {
            "fynkel.audio" => vec!["sound".into()],
            "fynkel.image" => vec!["texture".into()],
            _ => Vec::new(),
        },
    }
}

/// `A0007` for every component field of a scene or prefab whose reference
/// names an asset of another kind. Prefab overrides are not typed here.
pub fn check_reference_kinds(
    doc: &Document,
    schemas: &SchemaSet,
    db: &AssetDatabase,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    if !matches!(doc.kind, DocumentKind::Scene | DocumentKind::Prefab) {
        return out;
    }
    let Some(entities) = doc.root.get("entities").and_then(Node::as_object) else {
        return out;
    };
    let at = Locator::new(&doc.root);
    for entity in entities {
        let Some(components) = entity.value.get("components").and_then(Node::as_object) else {
            continue;
        };
        for c in components {
            let Some(type_id) = c.value.get("type").and_then(Node::as_str) else {
                continue;
            };
            let Some(data) = c.value.get("data") else {
                continue;
            };
            let fields = schemas.asset_fields(type_id);
            if fields.is_empty() {
                continue;
            }
            let base = format!(
                "/entities/{}/components/{}/data",
                escape(&entity.key),
                escape(&c.key)
            );
            for field in fields {
                let mut found = Vec::new();
                select(data, &field.path, base.clone(), &mut found);
                for (pointer, node) in found {
                    for (_, r) in asset_refs(node) {
                        if r.is_nil() {
                            continue;
                        }
                        let Ok(resolved) = db.resolve(&r) else {
                            continue;
                        };
                        let offered = offered_kinds(&resolved);
                        if offered.iter().any(|k| field.kinds.contains(k)) {
                            continue;
                        }
                        let what = match &resolved.sub {
                            Some((sub, _)) => format!("{}#{sub}", resolved.entry.path),
                            None => resolved.entry.path.clone(),
                        };
                        let is = offered.first().map_or_else(
                            || format!("a {} asset", resolved.entry.kind.name()),
                            |k| format!("a {}", k.replace('_', " ")),
                        );
                        let wants = field
                            .kinds
                            .iter()
                            .map(|k| k.replace('_', " "))
                            .collect::<Vec<_>>()
                            .join(" or ");
                        let d = Diagnostic::error(
                            Code::ASSET_WRONG_KIND,
                            format!(
                                "{type_id}.{} takes a {wants}, but {what} is {is}",
                                field.path.join(".").replace(".*", "[]")
                            ),
                        )
                        .with_id(r.to_string())
                        .with_suggestion(format!("pick a {wants} for this field"));
                        let d = at.anchor(d, &pointer);
                        out.push(match &doc.path {
                            Some(p) => d.with_document(p.clone()),
                            None => d,
                        });
                    }
                }
            }
        }
    }
    out
}

/// The nodes at `path` under `node` (`*` fans out over an array).
fn select<'a>(node: &'a Node, path: &[String], pointer: String, out: &mut Vec<(String, &'a Node)>) {
    let Some((head, rest)) = path.split_first() else {
        out.push((pointer, node));
        return;
    };
    if head == "*" {
        for (i, item) in node.as_array().unwrap_or_default().iter().enumerate() {
            select(item, rest, format!("{pointer}/{i}"), out);
        }
    } else if let Some(child) = node.get(head) {
        select(child, rest, format!("{pointer}/{}", escape(head)), out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_are_found_on_fields_list_items_and_nested_objects() {
        let schema = serde_json::json!({
            "properties": {
                "speed": { "type": "number" },
                "showcase": { "x-fynkel": { "type": "assetRef", "kind": "asset(mesh)" } },
                "sounds": {
                    "type": "array",
                    "items": { "x-fynkel": { "type": "assetRef", "kind": "asset(audio)" } },
                    "x-fynkel": { "type": "list", "items": "assetRef" }
                },
                "wave": { "type": "object", "properties": {
                    "enemy": { "x-fynkel": { "kind": "asset(prefab)" } }
                } }
            }
        });
        assert_eq!(
            asset_fields_of(&schema),
            vec![
                AssetField {
                    path: vec!["showcase".into()],
                    kinds: vec!["mesh".into()]
                },
                AssetField {
                    path: vec!["sounds".into(), "*".into()],
                    kinds: vec!["sound".into()]
                },
                AssetField {
                    path: vec!["wave".into(), "enemy".into()],
                    kinds: vec!["prefab".into()]
                },
            ]
        );
    }
}
