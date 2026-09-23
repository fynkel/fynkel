//! Stable sub-asset keys (`docs/design/stable-sub-asset-ids.md`, S1): with
//! a name-key sidecar, reordering a file's meshes, materials, clips and
//! nodes keeps every key, uuid and prefab id on the same part; with a
//! position-key sidecar nothing changes, and the renames a migration needs
//! are reported.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fynkel_document::asset::Meta;
use fynkel_import::subkey::NAME_KEYS_FORMAT;
use fynkel_import::{import, Imported, Limits};
use serde_json::Value;
use uuid::Uuid;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fynkel-subkeys-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A sidecar for `source` with a fixed id, in the position-key format
/// (today's) or the name-key one.
fn meta(source: &Path, name_keys: bool) -> Meta {
    let mut meta = Meta::new(source);
    meta.document.envelope.id = Uuid::parse_str("c0000000-0000-4000-8000-000000000001").unwrap();
    if name_keys {
        meta.document.envelope.format_version = NAME_KEYS_FORMAT;
    }
    meta
}

fn ok(r: Imported) -> Imported {
    assert!(!r.has_errors(), "{:#?}", r.diagnostics);
    r
}

/// Reverses one top-level array and re-points every index into it.
fn reverse(json: &mut Value, array: &str, refs: &[&[&str]]) {
    let n = json[array].as_array().map_or(0, Vec::len);
    if n == 0 {
        return;
    }
    json[array].as_array_mut().unwrap().reverse();
    let flip = |v: &mut Value| {
        if let Some(i) = v.as_u64() {
            *v = Value::from(n as u64 - 1 - i);
        }
    };
    for path in refs {
        visit(json, path, &flip);
    }
}

/// Applies `f` to every value at `path`, where `*` walks an array.
fn visit(v: &mut Value, path: &[&str], f: &dyn Fn(&mut Value)) {
    let Some((head, rest)) = path.split_first() else {
        match v {
            Value::Array(items) => items.iter_mut().for_each(f),
            other => f(other),
        }
        return;
    };
    if *head == "*" {
        if let Value::Array(items) = v {
            for item in items {
                visit(item, rest, f);
            }
        }
    } else if let Some(next) = v.get_mut(*head) {
        visit(next, rest, f);
    }
}

/// Every part the importers name, reversed in the file.
fn reorder(json: &mut Value) {
    reverse(json, "meshes", &[&["nodes", "*", "mesh"]]);
    reverse(
        json,
        "materials",
        &[&["meshes", "*", "primitives", "*", "material"]],
    );
    reverse(json, "animations", &[]);
    reverse(
        json,
        "nodes",
        &[
            &["scenes", "*", "nodes"],
            &["nodes", "*", "children"],
            &["skins", "*", "joints"],
            &["skins", "*", "skeleton"],
            &["animations", "*", "channels", "*", "target", "node"],
        ],
    );
}

/// A GLB's JSON chunk rewritten by `f`, the binary chunk kept.
fn rewrite_glb(bytes: &[u8], f: impl FnOnce(&mut Value)) -> Vec<u8> {
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let mut json: Value = serde_json::from_slice(&bytes[20..20 + json_len]).unwrap();
    f(&mut json);
    let mut text = serde_json::to_vec(&json).unwrap();
    while !text.len().is_multiple_of(4) {
        text.push(b' ');
    }
    let rest = &bytes[20 + json_len..];
    let mut out = Vec::new();
    out.extend_from_slice(&bytes[0..8]);
    out.extend_from_slice(&((12 + 8 + text.len() + rest.len()) as u32).to_le_bytes());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&text);
    out.extend_from_slice(rest);
    out
}

/// What identifies a part regardless of its position: key → (uuid, what
/// the part is).
fn parts(r: &Imported) -> BTreeMap<String, (Uuid, String)> {
    let i = r.intermediate.as_ref().unwrap();
    let mut what: BTreeMap<&str, String> = BTreeMap::new();
    for m in &i.meshes {
        what.insert(&m.sub_id, format!("mesh {} {}", m.name, m.vertices.len()));
    }
    for m in &i.materials {
        what.insert(&m.sub_id, format!("material {}", m.name));
    }
    for m in &i.images {
        what.insert(&m.sub_id, format!("image {}", m.name));
    }
    for s in &i.skins {
        what.insert(&s.sub_id, format!("skin {}", s.name));
    }
    for a in &i.animations {
        what.insert(&a.sub_id, format!("anim {}", a.name));
    }
    r.sub_assets
        .iter()
        .map(|(k, id)| {
            let w = what.get(k.as_str()).cloned().unwrap_or_default();
            (k.clone(), (*id, w))
        })
        .collect()
}

/// The prefab's entities by name: (entity id, component ids, the `sub`s
/// they reference).
fn entities(r: &Imported) -> BTreeMap<String, (String, Vec<String>, Vec<String>)> {
    let prefab = r.intermediate.as_ref().unwrap().prefab.as_ref().unwrap();
    let mut out = BTreeMap::new();
    for (id, e) in prefab["entities"].as_object().unwrap() {
        let name = e["name"].as_str().unwrap().to_string();
        let comps = e["components"].as_object().unwrap();
        let mut subs = Vec::new();
        collect_subs(&Value::Object(comps.clone()), &mut subs);
        out.insert(name, (id.clone(), comps.keys().cloned().collect(), subs));
    }
    out
}

fn collect_subs(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            if let Some(Value::String(s)) = m.get("sub") {
                out.push(s.clone());
            }
            m.values().for_each(|v| collect_subs(v, out));
        }
        Value::Array(a) => a.iter().for_each(|v| collect_subs(v, out)),
        _ => {}
    }
}

fn import_pair(original: &Path, reordered: &Path, name_keys: bool) -> (Imported, Imported) {
    let a = ok(import(
        original,
        &meta(original, name_keys),
        &Limits::default(),
    ));
    let b = ok(import(
        reordered,
        &meta(reordered, name_keys),
        &Limits::default(),
    ));
    (a, b)
}

fn gltf_pair() -> (PathBuf, PathBuf) {
    let dir = scratch();
    let text = std::fs::read_to_string(fixture("robot_data_uri.gltf")).unwrap();
    let mut json: Value = serde_json::from_str(&text).unwrap();
    let a = dir.join("robot.gltf");
    std::fs::write(&a, &text).unwrap();
    reorder(&mut json);
    let b = dir.join("robot_reordered.gltf");
    std::fs::write(&b, serde_json::to_vec(&json).unwrap()).unwrap();
    (a, b)
}

fn glb_pair(name: &str) -> (PathBuf, PathBuf) {
    let dir = scratch();
    let bytes = std::fs::read(fixture(name)).unwrap();
    let a = dir.join(name);
    std::fs::write(&a, &bytes).unwrap();
    let b = dir.join(format!("reordered_{name}"));
    std::fs::write(&b, rewrite_glb(&bytes, reorder)).unwrap();
    (a, b)
}

#[test]
fn name_keys_survive_reordering_meshes_materials_and_nodes() {
    let (a, b) = gltf_pair();
    let (a, b) = import_pair(&a, &b, true);
    let keys: Vec<&str> = a.sub_assets.iter().map(|(k, _)| k.as_str()).collect();
    for key in [
        "mesh/Cube",
        "mesh/PyramidAndQuad/0",
        "mesh/PyramidAndQuad/1",
        "material/Painted",
        "material/Glass",
        "image/paint",
        "prefab",
    ] {
        assert!(keys.contains(&key), "{key} not in {keys:?}");
    }
    assert_eq!(parts(&a), parts(&b));
    assert_eq!(entities(&a), entities(&b));
}

#[test]
fn name_keys_survive_reordering_skins_clips_and_joints() {
    let (a, b) = glb_pair("rig.glb");
    let (a, b) = import_pair(&a, &b, true);
    let keys: Vec<&str> = a.sub_assets.iter().map(|(k, _)| k.as_str()).collect();
    for key in [
        "mesh/Body",
        "mesh/Prop",
        "skin/Skin",
        "anim/wave",
        "anim/bob",
    ] {
        assert!(keys.contains(&key), "{key} not in {keys:?}");
    }
    assert_eq!(parts(&a), parts(&b));
    assert_eq!(entities(&a), entities(&b));
}

#[test]
fn position_keys_are_unchanged_and_report_the_renames() {
    let (pa, pb) = glb_pair("rig.glb");
    let (a, b) = import_pair(&pa, &pb, false);
    let keys: Vec<&str> = a.sub_assets.iter().map(|(k, _)| k.as_str()).collect();
    assert!(
        keys.contains(&"anim/0") && keys.contains(&"skin/0"),
        "{keys:?}"
    );
    // The drift the name keys remove: the same key, another clip.
    assert_ne!(parts(&a)["anim/0"].1, parts(&b)["anim/0"].1);

    let subs: BTreeMap<&str, &str> = a
        .renames
        .subs
        .iter()
        .map(|(o, n)| (o.as_str(), n.as_str()))
        .collect();
    assert_eq!(subs["anim/0"], "anim/wave");
    assert_eq!(subs["anim/1"], "anim/bob");
    assert_eq!(subs["skin/0"], "skin/Skin");

    // Every derived prefab id moves to the id the name-key import derives.
    let named = ok(import(&pa, &meta(&pa, true), &Limits::default()));
    let ids: BTreeMap<Uuid, Uuid> = a.renames.ids.iter().copied().collect();
    let old = entities(&a);
    let new = entities(&named);
    for (name, (id, comps, _)) in &old {
        let moved = |x: &str| {
            let u = Uuid::parse_str(x).unwrap();
            ids.get(&u).copied().unwrap_or(u).to_string()
        };
        assert_eq!(moved(id), new[name].0, "{name}");
        let comps: Vec<String> = comps.iter().map(|c| moved(c)).collect();
        assert_eq!(comps, new[name].1, "{name}");
    }
    // And the prefab's own references follow the renamed keys.
    for (name, (_, _, subs)) in &old {
        let renamed: Vec<String> = subs
            .iter()
            .map(|s| {
                a.renames
                    .subs
                    .iter()
                    .find(|(o, _)| o == s)
                    .map_or(s.clone(), |(_, n)| n.clone())
            })
            .collect();
        assert_eq!(renamed, new[name].2, "{name}");
    }
}

#[test]
fn fbx_parts_are_keyed_by_name() {
    let path = fixture("rig.fbx");
    let named = ok(import(&path, &meta(&path, true), &Limits::default()));
    let positional = ok(import(&path, &meta(&path, false), &Limits::default()));
    let keys: Vec<&str> = named.sub_assets.iter().map(|(k, _)| k.as_str()).collect();
    // Nothing named keeps a bare position, and the two imports carry the
    // same parts under the renamed keys.
    assert!(
        keys.iter().all(|k| *k == "prefab"
            || !k
                .split('/')
                .nth(1)
                .unwrap()
                .chars()
                .all(|c| c.is_ascii_digit())),
        "{keys:?}"
    );
    let renamed: Vec<String> = positional
        .sub_assets
        .iter()
        .map(|(k, _)| {
            positional
                .renames
                .subs
                .iter()
                .find(|(o, _)| o == k)
                .map_or(k.clone(), |(_, n)| n.clone())
        })
        .collect();
    assert_eq!(renamed, keys);
    assert!(
        keys.contains(&"anim/wave") && keys.contains(&"mesh/Body"),
        "{keys:?}"
    );
}
