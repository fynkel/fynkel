//! What the scene importers share (`fynkel.gltf`, `fynkel.fbx`): the
//! diagnostics sink, the document builders (`fynkel.pbr@1` material
//! header, the prefab's entities and components) and the tail of an
//! import — sub-asset ids, the id patches, the [`Intermediate`].
//!
//! An importer reduces its file to a [`SourceGraph`] — nodes with a
//! name, a local TRS, children, an imported mesh and a skin — and hands
//! it to [`prefab_document`], which is the one place the prefab's shape
//! is decided: an entity per node (`node/<i>`) with a `fynkel.Transform`,
//! a `fynkel.MeshRenderer` (+ `fynkel.Skin`) on the node for a single
//! imported primitive or on one child entity per primitive otherwise,
//! depth-first, first child first.

use std::path::Path;

use fynkel_contracts::{Code, Diagnostic};
use fynkel_cooked::payload::AnimationChannel;
use fynkel_document::asset::Meta;
use fynkel_document::DocumentKind;
use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::intermediate::{
    AnimationData, ColorSpace, ImageData, Intermediate, MaterialData, MeshData, SkinData,
};
use crate::{derive_sub_id, Imported, Limits};

/// The diagnostics of one import, every entry attributed to the file.
pub(crate) struct Report {
    pub shown: String,
    pub diagnostics: Vec<Diagnostic>,
}

impl Report {
    pub fn new(path: &Path) -> Report {
        Report {
            shown: path.display().to_string(),
            diagnostics: Vec::new(),
        }
    }

    pub fn error(&mut self, code: Code, msg: impl Into<String>) {
        self.diagnostics
            .push(Diagnostic::error(code, msg).with_document(self.shown.clone()));
    }

    pub fn error_with(
        &mut self,
        code: Code,
        msg: impl Into<String>,
        suggestion: impl Into<String>,
    ) {
        self.diagnostics.push(
            Diagnostic::error(code, msg)
                .with_document(self.shown.clone())
                .with_suggestion(suggestion),
        );
    }

    pub fn warn(&mut self, code: Code, msg: impl Into<String>) {
        self.diagnostics
            .push(Diagnostic::warning(code, msg).with_document(self.shown.clone()));
    }

    pub fn has_errors(&self) -> bool {
        fynkel_contracts::has_errors(&self.diagnostics)
    }

    /// The import that failed: no intermediate, the sidecar's ids untouched.
    pub fn fail(self, meta: &Meta) -> Imported {
        Imported {
            intermediate: None,
            renames: Default::default(),
            sub_assets: meta.sub_assets.clone(),
            diagnostics: self.diagnostics,
        }
    }
}

/// The file's name alone, for document descriptions.
pub(crate) fn shown_file(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The file's stem: the prefab's name when the file names no scene.
pub(crate) fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "model".to_string())
}

pub(crate) fn sniff_mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG") {
        "image/png"
    } else if bytes.starts_with(&[0xff, 0xd8]) {
        "image/jpeg"
    } else {
        "application/octet-stream"
    }
}

/// A number as the document should show it: the shortest text that
/// round-trips the `f32`, so `0.8f32` is `0.8`, not `0.800000011920929`.
pub(crate) fn num(x: f32) -> Value {
    let text = if x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{x:.1}")
    } else {
        x.to_string()
    };
    serde_json::Number::from_f64(text.parse::<f64>().unwrap_or(x as f64))
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

pub(crate) fn nums(v: &[f32]) -> Value {
    Value::Array(v.iter().map(|x| num(*x)).collect())
}

/// The head of a `fynkel.pbr@1` material document (`id` patched by
/// [`finish`]); the importer appends the factors and texture slots.
pub(crate) fn material_header(name: &str, sub_id: &str, shown_file: &str) -> Map<String, Value> {
    let mut doc = Map::new();
    doc.insert("$schema".into(), json!(DocumentKind::Material.schema_id()));
    doc.insert(
        "formatVersion".into(),
        json!(DocumentKind::Material.supported_format_version()),
    );
    doc.insert("id".into(), json!(Uuid::nil().to_string()));
    doc.insert("name".into(), json!(name));
    doc.insert(
        "description".into(),
        json!(format!("Imported from {shown_file} ({sub_id}).")),
    );
    doc.insert("shader".into(), json!("fynkel.pbr@1"));
    doc
}

/// A texture slot's value: the image sub-asset of this file.
pub(crate) fn image_ref(meta: &Meta, image_sub: &str) -> Value {
    json!({ "asset": meta.id().to_string(), "sub": image_sub })
}

/// Colour slots of the PBR model (sRGB); the others carry linear data.
const COLOR_SLOTS: [&str; 2] = ["base_color_texture", "emissive_texture"];
const DATA_SLOTS: [&str; 3] = [
    "normal_texture",
    "metallic_roughness_texture",
    "occlusion_texture",
];

/// Gives every image its colour space from the slots that bind it. An
/// image bound both as a colour and as data is cooked twice: `image/<i>`
/// stays sRGB and a linear copy `image/<i>/linear` is added, with the
/// materials' data slots re-pointed at it. Unbound images are sRGB.
pub(crate) fn split_dual_use_images(materials: &mut [MaterialData], images: &mut Vec<ImageData>) {
    let sub_of = |m: &MaterialData, slot: &str| -> Option<String> {
        m.document
            .get(slot)?
            .get("sub")?
            .as_str()
            .map(str::to_string)
    };
    let mut extra = Vec::new();
    for image in images.iter_mut() {
        let colour = materials.iter().any(|m| {
            COLOR_SLOTS
                .iter()
                .any(|s| sub_of(m, s).as_deref() == Some(&image.sub_id))
        });
        let data = materials.iter().any(|m| {
            DATA_SLOTS
                .iter()
                .any(|s| sub_of(m, s).as_deref() == Some(&image.sub_id))
        });
        image.color_space = if data && !colour {
            ColorSpace::Linear
        } else {
            ColorSpace::Srgb
        };
        if colour && data {
            let linear_sub = format!("{}/linear", image.sub_id);
            for m in materials.iter_mut() {
                for slot in DATA_SLOTS {
                    if sub_of(m, slot).as_deref() == Some(&image.sub_id) {
                        m.document[slot]["sub"] = json!(linear_sub);
                    }
                }
            }
            extra.push(ImageData {
                sub_id: linear_sub,
                name: format!("{} (linear)", image.name),
                mime_type: image.mime_type.clone(),
                bytes: image.bytes.clone(),
                color_space: ColorSpace::Linear,
            });
        }
    }
    images.extend(extra);
}

/// The checks every animation channel passes before it is kept: keys
/// present, the value count matching, times finite, non-negative and
/// strictly increasing, values finite. `Err` after recording the error.
pub(crate) fn check_channel(
    report: &mut Report,
    sub_id: &str,
    name: &str,
    c: usize,
    channel: &AnimationChannel,
) -> Result<(), ()> {
    if channel.times.is_empty() {
        report.error(
            Code::IMPORT_INVALID_DATA,
            format!("{sub_id} ({name}): channel {c} has no keys"),
        );
        return Err(());
    }
    if channel.values.len() != channel.times.len() * channel.key_floats() {
        report.error(
            Code::IMPORT_INVALID_DATA,
            format!(
                "{sub_id} ({name}): channel {c} has {} values for {} keys",
                channel.values.len(),
                channel.times.len()
            ),
        );
        return Err(());
    }
    if channel
        .times
        .windows(2)
        .any(|w| w[1] <= w[0] || !w[1].is_finite())
        || !channel.times[0].is_finite()
        || channel.times[0] < 0.0
    {
        report.error(
            Code::IMPORT_INVALID_DATA,
            format!(
                "{sub_id} ({name}): channel {c} key times must be finite, non-negative and strictly increasing"
            ),
        );
        return Err(());
    }
    if channel.values.iter().any(|v| !v.is_finite()) {
        report.error(
            Code::IMPORT_INVALID_DATA,
            format!("{sub_id} ({name}): channel {c} has a non-finite value"),
        );
        return Err(());
    }
    Ok(())
}

/// Says once per name when two different nodes share a name and both
/// are animated: only one entity named that way will be driven.
#[derive(Default)]
pub(crate) struct AnimatedNames {
    animated: Vec<(usize, String)>,
    warned: Vec<String>,
}

impl AnimatedNames {
    pub fn note(&mut self, report: &mut Report, node: usize, name: &str) {
        match self.animated.iter().find(|(_, n)| n == name) {
            Some((other, _)) if *other != node && !self.warned.iter().any(|w| w == name) => {
                self.warned.push(name.to_string());
                report.warn(
                    Code::IMPORT_SKIPPED,
                    format!(
                        "nodes {other} and {node} are both named {name:?} and both animated; only one entity named that way will be driven"
                    ),
                );
            }
            Some(_) => {}
            None => self.animated.push((node, name.to_string())),
        }
    }
}

/// A source node as the prefab needs it. `index` is the file's index for
/// the node (entity ids derive from it: `node/<index>`); `mesh` indexes
/// the importer's `mesh_subs` (the imported primitives of a source mesh);
/// `skin` indexes the imported skins.
pub(crate) struct SourceNode {
    pub index: usize,
    pub name: String,
    /// False when `name` is the importer's stand-in (`node<n>`): the
    /// node's key keeps its position then (`node/#<n>`).
    pub named: bool,
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    pub children: Vec<usize>,
    pub mesh: Option<usize>,
    pub skin: Option<usize>,
}

/// The file's node graph: `nodes[i]` is the node with index `i`; `roots`
/// the top-level nodes (the prefab's root when there is one, the children
/// of a root named `name` when there are several); `name` the prefab's.
pub(crate) struct SourceGraph {
    pub name: String,
    pub nodes: Vec<SourceNode>,
    pub roots: Vec<usize>,
}

/// The imported primitives of one source mesh: `(sub_id, name)` each.
pub(crate) type MeshSubs = Vec<(String, String)>;

/// The node graph as a prefab document body (`id` patched by [`finish`]),
/// and the derived ids that differ between position and name keys.
pub(crate) fn prefab_document(
    report: &mut Report,
    limits: &Limits,
    graph: &SourceGraph,
    mesh_subs: &[MeshSubs],
    meshes: &[MeshData],
    skins: &[SkinData],
    meta: &Meta,
) -> Option<(Value, Vec<(Uuid, Uuid)>)> {
    let asset = meta.id();
    if graph.nodes.len() > limits.max_nodes {
        report.error(
            Code::IMPORT_LIMIT,
            format!(
                "{} nodes; the limit is {}",
                graph.nodes.len(),
                limits.max_nodes
            ),
        );
        return None;
    }

    // Every derived id goes through `derive`: `node/<i>…` names the node by
    // position, its name path (`subkey::node_paths`) by name; the sidecar
    // decides which one the document carries, and both are recorded.
    let names: Vec<Option<&str>> = graph
        .nodes
        .iter()
        .map(|n| n.named.then_some(n.name.as_str()))
        .collect();
    let children: Vec<Vec<usize>> = graph.nodes.iter().map(|n| n.children.clone()).collect();
    let paths = crate::subkey::node_paths(&names, &children, &graph.roots);
    let by_name = crate::subkey::uses_name_keys(meta);
    let moved = std::cell::RefCell::new(Vec::<(Uuid, Uuid)>::new());
    let derive = |node: usize, rest: &str| -> String {
        let old = derive_sub_id(asset, &format!("node/{node}{rest}"));
        let new = derive_sub_id(asset, &format!("{}{rest}", paths[node]));
        if old != new {
            moved.borrow_mut().push((old, new));
        }
        if by_name { new } else { old }.to_string()
    };
    let entity_id = |node: usize| derive(node, "");
    // A skin's skeleton root: a reference to the node's entity (ADR-0035),
    // None when the file names none (the tree root at runtime).
    let skeleton_of = |skin: &SkinData| -> Option<String> {
        let name = skin.payload.skeleton.as_str();
        let node = graph
            .nodes
            .iter()
            .find(|n| !name.is_empty() && n.name == name)?;
        let id = if by_name {
            derive_sub_id(asset, &paths[node.index])
        } else {
            derive_sub_id(asset, &format!("node/{}", node.index))
        };
        Some(id.to_string())
    };
    let material_of = |sub: &str| -> Option<String> {
        meshes
            .iter()
            .find(|m| m.sub_id == sub)
            .and_then(|m| m.material.clone())
    };
    let mut entities: Vec<(String, Value)> = Vec::new();
    let mut visited = vec![false; graph.nodes.len()];
    let root_ids: Vec<String> = graph.roots.iter().map(|r| entity_id(*r)).collect();

    // One root (ADR-0022): the file's one top-level node, or a new root
    // named after the file over several.
    let (root, top_parent) = match root_ids.as_slice() {
        [one] => (one.clone(), None),
        _ => {
            let wrapper = derive_sub_id(asset, "node").to_string();
            entities.push((
                wrapper.clone(),
                entity_value(
                    &graph.name,
                    None,
                    root_ids.clone(),
                    vec![(
                        derive_sub_id(asset, "node/transform").to_string(),
                        transform_component([0.0; 3], [0.0, 0.0, 0.0, 1.0], [1.0; 3]),
                    )],
                ),
            ));
            (wrapper.clone(), Some(wrapper))
        }
    };

    // Explicit stack: (node, parent entity id, depth); children pushed in
    // reverse so document order is depth-first, first child first.
    let mut stack: Vec<(usize, Option<String>, usize)> = graph
        .roots
        .iter()
        .rev()
        .map(|r| (*r, top_parent.clone(), 1))
        .collect();

    while let Some((index, parent, depth)) = stack.pop() {
        let node = &graph.nodes[index];
        if depth > limits.max_node_depth {
            report.error(
                Code::IMPORT_LIMIT,
                format!(
                    "node {} is nested deeper than {} levels",
                    node.index, limits.max_node_depth
                ),
            );
            return None;
        }
        if visited[index] {
            report.error(
                Code::IMPORT_INVALID_DATA,
                format!(
                    "node {} ({}) is reached twice: the node graph has a cycle or a node with two parents",
                    node.index, node.name
                ),
            );
            return None;
        }
        visited[index] = true;
        let id = entity_id(node.index);
        let skin = node_skin(report, node, mesh_subs, meshes, skins)?;

        let mut components: Vec<(String, Value)> = vec![(
            derive(node.index, "/transform"),
            transform_component(node.translation, node.rotation, node.scale),
        )];
        let mut children: Vec<String> = Vec::new();
        let mut primitive_entities: Vec<(String, Value)> = Vec::new();
        if let Some(mesh) = node.mesh {
            let subs = &mesh_subs[mesh];
            if subs.len() == 1 {
                components.push((
                    derive(node.index, "/renderer"),
                    renderer_component(meta, &subs[0].0, material_of(&subs[0].0)),
                ));
                if let Some(skin) = skin {
                    components.push((
                        derive(node.index, "/skin"),
                        skin_component(meta, skin, skeleton_of(skin)),
                    ));
                }
            } else {
                for (p, (sub, sub_name)) in subs.iter().enumerate() {
                    let child_id = derive(node.index, &format!("/primitive/{p}"));
                    children.push(child_id.clone());
                    let mut prim_components: Vec<(String, Value)> = vec![
                        (
                            derive(node.index, &format!("/primitive/{p}/transform")),
                            transform_component([0.0; 3], [0.0, 0.0, 0.0, 1.0], [1.0; 3]),
                        ),
                        (
                            derive(node.index, &format!("/primitive/{p}/renderer")),
                            renderer_component(meta, sub, material_of(sub)),
                        ),
                    ];
                    if let Some(skin) = skin {
                        if meshes.iter().any(|m| &m.sub_id == sub && m.is_skinned()) {
                            prim_components.push((
                                derive(node.index, &format!("/primitive/{p}/skin")),
                                skin_component(meta, skin, skeleton_of(skin)),
                            ));
                        }
                    }
                    primitive_entities.push((
                        child_id.clone(),
                        entity_value(sub_name, Some(&id), Vec::new(), prim_components),
                    ));
                }
            }
        }
        for c in &node.children {
            children.push(entity_id(graph.nodes[*c].index));
        }
        entities.push((
            id.clone(),
            entity_value(&node.name, parent.as_deref(), children, components),
        ));
        entities.extend(primitive_entities);
        for c in node.children.iter().rev() {
            stack.push((*c, Some(id.clone()), depth + 1));
        }
    }

    let mut prefab = Map::new();
    prefab.insert("$schema".into(), json!(DocumentKind::Prefab.schema_id()));
    prefab.insert(
        "formatVersion".into(),
        json!(DocumentKind::Prefab.supported_format_version()),
    );
    prefab.insert("id".into(), json!(Uuid::nil().to_string()));
    prefab.insert("name".into(), json!(graph.name));
    prefab.insert(
        "description".into(),
        json!(format!("Imported from {}.", report_file(report))),
    );
    let mut ents = Map::new();
    for (k, v) in entities {
        ents.insert(k, v);
    }
    prefab.insert("entities".into(), Value::Object(ents));
    prefab.insert("root".into(), json!(root));
    Some((Value::Object(prefab), moved.into_inner()))
}

fn report_file(report: &Report) -> String {
    shown_file(Path::new(&report.shown))
}

/// The skin a node's mesh deforms with, checked against the mesh: `None`
/// (with a warning) for a skin on a node without a skinned mesh or a
/// skinned mesh on a node without a skin (it draws in its bind pose),
/// `Err` when a joint index points past the skin.
fn node_skin<'a>(
    report: &mut Report,
    node: &SourceNode,
    mesh_subs: &[MeshSubs],
    meshes: &[MeshData],
    skins: &'a [SkinData],
) -> Option<Option<&'a SkinData>> {
    let subs: &[(String, String)] = node.mesh.map(|m| mesh_subs[m].as_slice()).unwrap_or(&[]);
    let skinned: Vec<&MeshData> = meshes
        .iter()
        .filter(|m| m.is_skinned() && subs.iter().any(|(s, _)| *s == m.sub_id))
        .collect();
    let Some(skin) = node.skin else {
        if let Some(m) = skinned.first() {
            report.warn(
                Code::IMPORT_SKIPPED,
                format!(
                    "node {} ({}) has the skinned mesh {} but no skin; it will draw in its bind pose",
                    node.index, node.name, m.sub_id
                ),
            );
        }
        return Some(None);
    };
    let skin = &skins[skin];
    if skinned.is_empty() {
        report.warn(
            Code::IMPORT_SKIPPED,
            format!(
                "node {} ({}) has {} but no skinned mesh; the skin is not applied",
                node.index, node.name, skin.sub_id
            ),
        );
        return Some(None);
    }
    let joints = skin.payload.joints.len();
    for m in skinned {
        if let Some(bad) = m
            .skin
            .iter()
            .flat_map(|w| w.joints.iter().zip(w.weights))
            .find(|(j, w)| **j as usize >= joints && *w > 0.0)
        {
            report.error(
                Code::IMPORT_INVALID_DATA,
                format!(
                    "{} ({}): joint index {} out of range ({} has {joints} joints)",
                    m.sub_id, m.name, bad.0, skin.sub_id
                ),
            );
            return None;
        }
    }
    Some(Some(skin))
}

pub(crate) fn entity_value(
    name: &str,
    parent: Option<&str>,
    children: Vec<String>,
    components: Vec<(String, Value)>,
) -> Value {
    let mut e = Map::new();
    e.insert("name".into(), json!(name));
    e.insert(
        "parent".into(),
        parent.map(|p| json!(p)).unwrap_or(Value::Null),
    );
    e.insert("children".into(), json!(children));
    let mut c = Map::new();
    for (k, v) in components {
        c.insert(k, v);
    }
    e.insert("components".into(), Value::Object(c));
    Value::Object(e)
}

pub(crate) fn transform_component(t: [f32; 3], r: [f32; 4], s: [f32; 3]) -> Value {
    json!({
        "type": "fynkel.Transform",
        "version": 1,
        "data": { "position": nums(&t), "rotation": nums(&r), "scale": nums(&s) },
    })
}

/// `fynkel.Skin`: the skin sub-asset and a reference to the skeleton root
/// node's entity (ADR-0035; null when the file names none: the runtime
/// then searches the prefab's whole tree).
pub(crate) fn skin_component(meta: &Meta, skin: &SkinData, skeleton: Option<String>) -> Value {
    json!({
        "type": "fynkel.Skin",
        "version": 1,
        "data": {
            "skin": { "asset": meta.id().to_string(), "sub": skin.sub_id },
            "skeleton": skeleton.map(|id| json!({ "entity": id })),
        },
    })
}

pub(crate) fn renderer_component(
    meta: &Meta,
    mesh_sub: &str,
    material_sub: Option<String>,
) -> Value {
    let material = match material_sub {
        Some(sub) => json!({ "asset": meta.id().to_string(), "sub": sub }),
        None => json!({ "asset": Uuid::nil().to_string() }),
    };
    json!({
        "type": "fynkel.MeshRenderer",
        "version": 1,
        "data": {
            "mesh": { "asset": meta.id().to_string(), "sub": mesh_sub },
            "material": material,
            "cast_shadows": true,
            "receive_shadows": true,
            "visible": true,
        },
    })
}

/// Everything an import produced, in one intermediate.
pub(crate) struct Produced {
    pub meshes: Vec<MeshData>,
    pub materials: Vec<MaterialData>,
    pub images: Vec<ImageData>,
    pub skins: Vec<SkinData>,
    pub animations: Vec<AnimationData>,
    pub prefab: Value,
    /// The prefab's derived ids that differ by key scheme ([`prefab_document`]).
    pub prefab_ids: Vec<(Uuid, Uuid)>,
    /// Position → name for every part the importer registered.
    pub keys: crate::subkey::Keys,
}

/// The tail of a scene import: every sub-asset gets its id (the sidecar's
/// when it has one, derived otherwise; ids the file no longer produces
/// are reported), the ids are patched into the documents that carry
/// them, and the intermediate is assembled.
pub(crate) fn finish(
    mut report: Report,
    meta: &Meta,
    path: &Path,
    importer: &str,
    importer_version: u32,
    produced: Produced,
) -> Imported {
    let Produced {
        mut meshes,
        mut materials,
        mut images,
        mut skins,
        mut animations,
        mut prefab,
        prefab_ids,
        keys,
    } = produced;
    let asset = meta.id();

    // The Model tab's settings (scale, normals, animation).
    let settings = match crate::model_settings::ModelSettings::from_meta(meta) {
        Ok(s) => s,
        Err(d) => {
            report
                .diagnostics
                .push(d.with_document(report.shown.clone()));
            return report.fail(meta);
        }
    };
    settings.apply(&mut meshes, &mut skins, &mut animations, &mut prefab);

    // Position keys → name keys (`subkey`): always recorded for `fynkel
    // migrate`, applied when the sidecar has moved to name keys.
    let renames = crate::subkey::Renames {
        subs: meshes
            .iter()
            .map(|m| m.sub_id.as_str())
            .chain(materials.iter().map(|m| m.sub_id.as_str()))
            .chain(images.iter().map(|i| i.sub_id.as_str()))
            .chain(skins.iter().map(|s| s.sub_id.as_str()))
            .chain(animations.iter().map(|a| a.sub_id.as_str()))
            .map(|old| (old.to_string(), keys.rename(old)))
            .filter(|(old, new)| old != new)
            .collect(),
        ids: prefab_ids,
    };
    if crate::subkey::uses_name_keys(meta) && !renames.subs.is_empty() {
        let rename = |key: &mut String| *key = keys.rename(key);
        for m in &mut meshes {
            rename(&mut m.sub_id);
            if let Some(material) = &mut m.material {
                rename(material);
            }
        }
        for m in &mut materials {
            let old = m.sub_id.clone();
            rename(&mut m.sub_id);
            if let Some(Value::String(d)) = m.document.get_mut("description") {
                *d = d.replace(&format!("({old})"), &format!("({})", m.sub_id));
            }
            rename_refs(&mut m.document, &asset.to_string(), &keys);
        }
        for i in &mut images {
            rename(&mut i.sub_id);
        }
        for s in &mut skins {
            rename(&mut s.sub_id);
        }
        for a in &mut animations {
            rename(&mut a.sub_id);
        }
        rename_refs(&mut prefab, &asset.to_string(), &keys);
    }

    remap_materials(&mut report, meta, &materials, &mut prefab);

    let mut sub_assets = Vec::new();
    for sub in meshes
        .iter()
        .map(|m| m.sub_id.as_str())
        .chain(materials.iter().map(|m| m.sub_id.as_str()))
        .chain(images.iter().map(|i| i.sub_id.as_str()))
        .chain(skins.iter().map(|s| s.sub_id.as_str()))
        .chain(animations.iter().map(|a| a.sub_id.as_str()))
        .chain(std::iter::once("prefab"))
    {
        let id = meta
            .sub_assets
            .iter()
            .find(|(k, _)| k == sub)
            .map(|(_, v)| *v)
            .unwrap_or_else(|| derive_sub_id(asset, sub));
        sub_assets.push((sub.to_string(), id));
    }
    let dropped: Vec<&str> = meta
        .sub_assets
        .iter()
        .filter(|(k, _)| !sub_assets.iter().any(|(s, _)| s == k))
        .filter(|(k, _)| settings.import_animation || !k.starts_with("anim/"))
        .map(|(k, _)| k.as_str())
        .collect();
    if !dropped.is_empty() {
        // One key gone and another of the same kind new is almost always a
        // rename in the modelling tool: name the new ones, never guess.
        let kind = |k: &str| k.split('/').next().unwrap_or("").to_string();
        let appeared: Vec<&str> = sub_assets
            .iter()
            .map(|(k, _)| k.as_str())
            .filter(|k| !meta.sub_assets.iter().any(|(s, _)| s == k))
            .filter(|k| dropped.iter().any(|d| kind(d) == kind(k)))
            .collect();
        let mut msg = format!(
            "sidecar sub-asset(s) no longer produced by the file: {} (references to them will not resolve)",
            dropped.join(", ")
        );
        if !appeared.is_empty() {
            msg.push_str(&format!(
                "; new in this import: {} — if a part was renamed in the modelling tool, point its references at the new key",
                appeared.join(", ")
            ));
        }
        report.warn(Code::IMPORT_SKIPPED, msg);
    }
    let lookup = |sub: &str| -> Uuid {
        sub_assets
            .iter()
            .find(|(k, _)| k == sub)
            .map(|(_, v)| *v)
            .unwrap()
    };
    for m in &mut materials {
        m.document["id"] = Value::String(lookup(&m.sub_id).to_string());
    }
    prefab["id"] = Value::String(lookup("prefab").to_string());

    Imported {
        intermediate: Some(Intermediate {
            importer: importer.to_string(),
            importer_version,
            source: shown_file(path),
            meshes,
            materials,
            images,
            textures: Vec::new(),
            clips: Vec::new(),
            skins,
            animations,
            prefab: Some(prefab),
            sub_assets: sub_assets.clone(),
        }),
        sub_assets,
        diagnostics: report.diagnostics,
        renames,
    }
}

/// The sidecar's material remaps (`settings.materials`,
/// `docs/design/material-remapping.md`): every `fynkel.MeshRenderer` in
/// the model's hierarchy whose material is a remapped embedded material
/// points at the project material instead. The embedded materials stay
/// sub-assets; a remap for a key the file no longer has is a warning.
fn remap_materials(
    report: &mut Report,
    meta: &Meta,
    materials: &[MaterialData],
    prefab: &mut Value,
) {
    let remaps = meta.material_remaps();
    if remaps.is_empty() {
        return;
    }
    let unknown: Vec<&str> = remaps
        .iter()
        .map(|(k, _)| k.as_str())
        .filter(|k| !materials.iter().any(|m| m.sub_id == *k))
        .collect();
    if !unknown.is_empty() {
        let current: Vec<&str> = materials.iter().map(|m| m.sub_id.as_str()).collect();
        report.warn(
            Code::IMPORT_SKIPPED,
            format!(
                "settings.materials remaps {}, which the file no longer has (its materials: {}); if a material was renamed in the modelling tool, move the remap to its new key",
                unknown.join(", "),
                if current.is_empty() { "none".to_string() } else { current.join(", ") }
            ),
        );
    }
    let own = meta.id().to_string();
    let Some(entities) = prefab.get_mut("entities").and_then(Value::as_object_mut) else {
        return;
    };
    for entity in entities.values_mut() {
        let Some(components) = entity.get_mut("components").and_then(Value::as_object_mut) else {
            continue;
        };
        for c in components.values_mut() {
            if c.get("type").and_then(Value::as_str) != Some("fynkel.MeshRenderer") {
                continue;
            }
            let Some(material) = c.pointer_mut("/data/material") else {
                continue;
            };
            if material.get("asset").and_then(Value::as_str) != Some(own.as_str()) {
                continue;
            }
            let sub = material.get("sub").and_then(Value::as_str).unwrap_or("");
            if let Some((_, target)) = remaps.iter().find(|(k, _)| k == sub) {
                *material = json!({ "asset": target.asset.to_string() });
            }
        }
    }
}

/// Renames the `sub` of every reference into this file (`asset` equal to
/// `own`) inside an imported document.
fn rename_refs(value: &mut Value, own: &str, keys: &crate::subkey::Keys) {
    match value {
        Value::Object(map) => {
            if map.get("asset").and_then(Value::as_str) == Some(own) {
                if let Some(Value::String(sub)) = map.get_mut("sub") {
                    *sub = keys.rename(sub);
                }
            }
            for v in map.values_mut() {
                rename_refs(v, own, keys);
            }
        }
        Value::Array(items) => {
            for v in items {
                rename_refs(v, own, keys);
            }
        }
        _ => {}
    }
}
