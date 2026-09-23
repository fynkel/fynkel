//! glTF 2.0 / GLB importer (`fynkel.gltf`, blueprint §19.2c item 10).
//!
//! What it produces: one [`MeshData`] per triangle primitive (`mesh/<n>`,
//! or `mesh/<n>/<p>` when a glTF mesh has several primitives), one
//! `fynkel.pbr@1` material document per glTF material (`material/<m>`),
//! every image as its encoded bytes (`image/<i>`), and the default scene's
//! node hierarchy as a prefab (`prefab`): an entity per node with a
//! `fynkel.Transform`, plus a `fynkel.MeshRenderer` on the node for a
//! single-primitive mesh or on one child entity per primitive otherwise.
//! Every id is derived from the asset id (UUID v5), so the output is the
//! same on every machine.
//!
//! Rigged sources (M3-19, `docs/animation.md`): a primitive with
//! `JOINTS_0`/`WEIGHTS_0` keeps one normalized [`SkinWeight`] per vertex;
//! every skin becomes a [`SkinData`] (`skin/<s>`: joints by node name with
//! their inverse bind matrices, parents inside the skin and rest pose) and
//! every animation an [`AnimationData`] (`anim/<a>`: translation, rotation
//! and scale channels that target nodes by name). The prefab puts a
//! `fynkel.Skin` next to the `fynkel.MeshRenderer` of a node that has a
//! skin, naming the skin sub-asset and the skeleton root node. Names are
//! the contract because prefab instances get fresh entity ids: joint
//! names must be unique within a skin (an error), animated node names
//! should be unique in the file (a warning).
//!
//! What it refuses: buffers or images outside the file unless the
//! sidecar's settings say `"allowExternalFiles": true` (and then only
//! plain relative paths inside the source's directory), required
//! extensions it does not implement (Draco, meshopt, ...), and anything
//! past the [`Limits`]. Non-triangle primitives, morph targets (and their
//! weight animations) and cameras are skipped with a warning.

use std::path::Path;

use base64::Engine;
use fynkel_contracts::Code;
use fynkel_document::asset::Meta;
use gltf::mesh::Mode;
use serde_json::{json, Value};

use fynkel_cooked::payload::{
    AnimationChannel, AnimationPayload, SkinJoint, SkinPayload, SkinWeight,
    ANIM_INTERPOLATION_CUBIC_SPLINE, ANIM_INTERPOLATION_LINEAR, ANIM_INTERPOLATION_STEP,
    ANIM_PROPERTY_ROTATION, ANIM_PROPERTY_SCALE, ANIM_PROPERTY_TRANSLATION,
};

use crate::graph::{
    check_channel, file_stem, image_ref, material_header, num, nums, prefab_document, shown_file,
    sniff_mime, split_dual_use_images, AnimatedNames, MeshSubs, Produced, Report, SourceGraph,
    SourceNode,
};
use crate::intermediate::{
    AnimationData, ColorSpace, ImageData, MaterialData, MeshData, SkinData, Vertex,
};
use crate::subkey::Keys;
use crate::{read_source, Imported, Limits};

pub const IMPORTER: &str = fynkel_document::asset::GLTF_IMPORTER;
/// 6: sub-asset keys are the parts' names once the sidecar is at
/// `formatVersion` 2 (`subkey`). 7: `settings.materials` remaps embedded
/// materials in the model's hierarchy (`graph::remap_materials`).
pub const IMPORTER_VERSION: u32 = 7;

/// Extensions the importer understands when a file *requires* them.
const SUPPORTED_REQUIRED: &[&str] = &["KHR_materials_emissive_strength"];

/// Import settings read from the sidecar's `settings`.
#[derive(Clone, Debug, Default)]
struct Settings {
    allow_external_files: bool,
}

impl Settings {
    fn from_meta(meta: &Meta) -> Settings {
        Settings {
            allow_external_files: meta
                .settings
                .get("allowExternalFiles")
                .and_then(|n| n.as_bool())
                .unwrap_or(false),
        }
    }
}

struct Ctx<'a> {
    path: &'a Path,
    report: Report,
    limits: &'a Limits,
    settings: Settings,
    buffer_bytes: u64,
    /// Textures already warned about (one warning per texture, not per use).
    warned_textures: Vec<usize>,
}

impl Ctx<'_> {
    fn error(&mut self, code: Code, msg: impl Into<String>) {
        self.report.error(code, msg);
    }

    fn warn(&mut self, code: Code, msg: impl Into<String>) {
        self.report.warn(code, msg);
    }

    fn has_errors(&self) -> bool {
        self.report.has_errors()
    }

    /// Resolves a buffer or image URI: `data:` URIs inline, plain relative
    /// paths when allowed, everything else refused.
    fn load_uri(&mut self, uri: &str, what: &str) -> Option<Vec<u8>> {
        if let Some(rest) = uri.strip_prefix("data:") {
            let Some((_, data)) = rest.split_once(";base64,") else {
                self.error(
                    Code::IMPORT_UNSUPPORTED,
                    format!("{what}: data URI is not base64"),
                );
                return None;
            };
            return match base64::engine::general_purpose::STANDARD.decode(data) {
                Ok(bytes) => Some(bytes),
                Err(e) => {
                    self.error(
                        Code::IMPORT_INVALID_DATA,
                        format!("{what}: data URI does not decode: {e}"),
                    );
                    None
                }
            };
        }
        if !self.settings.allow_external_files {
            self.report.error_with(
                Code::IMPORT_EXTERNAL_URI,
                format!("{what} lives outside the file ({uri}); external files are not read"),
                "export a self-contained .glb, or set \"allowExternalFiles\": true in the sidecar's settings for plain relative paths",
            );
            return None;
        }
        let safe = !uri.is_empty()
            && !uri.contains("://")
            && !uri.starts_with('/')
            && !uri.starts_with('\\')
            && !uri.contains("..")
            && !uri.contains('\0');
        if !safe {
            self.error(
                Code::IMPORT_EXTERNAL_URI,
                format!("{what}: only plain relative paths inside the source's directory are read ({uri})"),
            );
            return None;
        }
        let decoded = percent_decode(uri);
        let file = self.path.parent().unwrap_or(Path::new(".")).join(decoded);
        match crate::read_source(&file, self.limits) {
            Ok(bytes) => Some(bytes),
            Err(d) => {
                self.report.diagnostics.push(
                    d.with_document(self.report.shown.clone())
                        .with_suggestion(format!("{what} → {}", file.display())),
                );
                None
            }
        }
    }

    fn count_bytes(&mut self, len: usize, what: &str) -> bool {
        self.buffer_bytes += len as u64;
        if self.buffer_bytes > self.limits.max_buffer_bytes {
            self.error(
                Code::IMPORT_LIMIT,
                format!(
                    "{what}: buffer data exceeds {} bytes",
                    self.limits.max_buffer_bytes
                ),
            );
            return false;
        }
        true
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn import(path: &Path, meta: &Meta, limits: &Limits) -> Imported {
    let mut ctx = Ctx {
        path,
        report: Report::new(path),
        limits,
        settings: Settings::from_meta(meta),
        buffer_bytes: 0,
        warned_textures: Vec::new(),
    };
    let fail = |ctx: Ctx| ctx.report.fail(meta);

    let bytes = match read_source(path, limits) {
        Ok(b) => b,
        Err(d) => {
            ctx.report.diagnostics.push(d);
            return fail(ctx);
        }
    };
    let gltf = match gltf::Gltf::from_slice(&bytes) {
        Ok(g) => g,
        Err(gltf::Error::Validation(errors))
            if errors
                .iter()
                .any(|(_, e)| matches!(e, gltf::json::validation::Error::Unsupported)) =>
        {
            // The crate refuses files that *require* an extension it does not
            // know (Draco, meshopt, ...); that is our A0103, not a broken file.
            let which: Vec<String> = errors
                .iter()
                .filter(|(_, e)| matches!(e, gltf::json::validation::Error::Unsupported))
                .map(|(p, _)| p.to_string())
                .collect();
            ctx.report.error_with(
                Code::IMPORT_UNSUPPORTED,
                format!(
                    "the file requires an extension this importer does not support ({})",
                    which.join(", ")
                ),
                "re-export without mesh compression or other required extensions",
            );
            return fail(ctx);
        }
        Err(e) => {
            ctx.error(Code::IMPORT_UNREADABLE, format!("not a glTF 2.0 file: {e}"));
            return fail(ctx);
        }
    };
    let doc = &gltf.document;

    // The crate lets through the extensions it parses; we implement fewer.
    for ext in doc.extensions_required() {
        if !SUPPORTED_REQUIRED.contains(&ext) {
            ctx.report.error_with(
                Code::IMPORT_UNSUPPORTED,
                format!("required extension {ext} is not supported"),
                "re-export without mesh compression or other required extensions",
            );
        }
    }
    if ctx.has_errors() {
        return fail(ctx);
    }

    // Buffers: the BIN chunk, data URIs, or (when allowed) files next to the source.
    let mut buffers: Vec<Vec<u8>> = Vec::with_capacity(doc.buffers().len());
    for buffer in doc.buffers() {
        let what = format!("buffer {}", buffer.index());
        let data = match buffer.source() {
            gltf::buffer::Source::Bin => match &gltf.blob {
                Some(blob) => Some(blob.clone()),
                None => {
                    ctx.error(
                        Code::IMPORT_INVALID_DATA,
                        format!("{what}: the GLB has no BIN chunk"),
                    );
                    None
                }
            },
            gltf::buffer::Source::Uri(uri) => ctx.load_uri(uri, &what),
        };
        let Some(data) = data else {
            return fail(ctx);
        };
        if data.len() < buffer.length() {
            ctx.error(
                Code::IMPORT_INVALID_DATA,
                format!(
                    "{what}: {} bytes available, {} declared",
                    data.len(),
                    buffer.length()
                ),
            );
            return fail(ctx);
        }
        if !ctx.count_bytes(data.len(), &what) {
            return fail(ctx);
        }
        buffers.push(data);
    }

    // Images: encoded bytes only.
    if doc.images().len() > limits.max_images {
        ctx.error(
            Code::IMPORT_LIMIT,
            format!(
                "{} images; the limit is {}",
                doc.images().len(),
                limits.max_images
            ),
        );
        return fail(ctx);
    }
    let mut images = Vec::with_capacity(doc.images().len());
    for image in doc.images() {
        let what = format!("image {}", image.index());
        let (bytes, mime) = match image.source() {
            gltf::image::Source::View { view, mime_type } => {
                let start = view.offset();
                let end = start + view.length();
                let buf = &buffers[view.buffer().index()];
                if end > buf.len() {
                    ctx.error(
                        Code::IMPORT_INVALID_DATA,
                        format!("{what}: buffer view runs past its buffer"),
                    );
                    return fail(ctx);
                }
                (buf[start..end].to_vec(), mime_type.to_string())
            }
            gltf::image::Source::Uri { uri, mime_type } => {
                let Some(bytes) = ctx.load_uri(uri, &what) else {
                    return fail(ctx);
                };
                let mime = mime_type
                    .map(str::to_string)
                    .or_else(|| {
                        uri.strip_prefix("data:")
                            .and_then(|r| r.split_once(';'))
                            .map(|(m, _)| m.to_string())
                    })
                    .unwrap_or_else(|| sniff_mime(&bytes).to_string());
                (bytes, mime)
            }
        };
        if bytes.len() as u64 > limits.max_image_bytes {
            ctx.error(
                Code::IMPORT_LIMIT,
                format!(
                    "{what}: {} bytes; the limit is {}",
                    bytes.len(),
                    limits.max_image_bytes
                ),
            );
            return fail(ctx);
        }
        images.push(ImageData {
            sub_id: format!("image/{}", image.index()),
            name: image
                .name()
                .map(str::to_string)
                .unwrap_or_else(|| format!("image{}", image.index())),
            mime_type: mime,
            bytes,
            color_space: ColorSpace::Srgb,
        });
    }

    // Materials → fynkel.pbr@1 documents.
    let mut materials = Vec::with_capacity(doc.materials().len());
    for material in doc.materials() {
        let index = material.index().unwrap_or(0);
        let sub_id = format!("material/{index}");
        let name = material
            .name()
            .map(str::to_string)
            .unwrap_or_else(|| format!("material{index}"));
        let document = material_document(&mut ctx, &material, &sub_id, &name, meta);
        materials.push(MaterialData {
            sub_id,
            name,
            document,
        });
    }
    split_dual_use_images(&mut materials, &mut images);

    // Meshes → one MeshData per triangle primitive.
    let mut meshes = Vec::new();
    // glTF mesh index → the sub ids of its imported primitives (prefab needs them).
    let mut mesh_subs: Vec<MeshSubs> = vec![Vec::new(); doc.meshes().len()];
    for mesh in doc.meshes() {
        let mesh_name = mesh
            .name()
            .map(str::to_string)
            .unwrap_or_else(|| format!("mesh{}", mesh.index()));
        let single = mesh.primitives().len() == 1;
        for primitive in mesh.primitives() {
            let sub_id = if single {
                format!("mesh/{}", mesh.index())
            } else {
                format!("mesh/{}/{}", mesh.index(), primitive.index())
            };
            let name = if single {
                mesh_name.clone()
            } else {
                format!("{mesh_name}[{}]", primitive.index())
            };
            match primitive_mesh(&mut ctx, &buffers, &primitive, &sub_id, &name) {
                Ok(Some(m)) => {
                    mesh_subs[mesh.index()].push((sub_id, name));
                    meshes.push(m);
                }
                Ok(None) => {}
                Err(()) => return fail(ctx),
            }
        }
    }

    // Skins and animations.
    let Ok(skins) = import_skins(&mut ctx, doc, &buffers) else {
        return fail(ctx);
    };
    let Ok(animations) = import_animations(&mut ctx, doc, &buffers) else {
        return fail(ctx);
    };

    // Skipped features, once each.
    if doc.cameras().len() > 0 {
        ctx.warn(
            Code::IMPORT_SKIPPED,
            format!("{} camera(s) skipped", doc.cameras().len()),
        );
    }

    // Every part's source name, for its name key (`subkey`).
    let mut keys = Keys::default();
    keys.kind(
        "image",
        doc.images().map(|i| {
            let name = i.name().map(str::to_string).or_else(|| match i.source() {
                gltf::image::Source::Uri { uri, .. } if !uri.starts_with("data:") => {
                    Path::new(&percent_decode(uri))
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                }
                _ => None,
            });
            (i.index().to_string(), name)
        }),
    );
    let index_of = |i: Option<usize>| i.unwrap_or(0).to_string();
    keys.kind(
        "material",
        doc.materials().map(|m| (index_of(m.index()), m.name())),
    );
    keys.kind(
        "mesh",
        doc.meshes().map(|m| (m.index().to_string(), m.name())),
    );
    keys.kind(
        "skin",
        doc.skins().map(|s| (s.index().to_string(), s.name())),
    );
    keys.kind(
        "anim",
        doc.animations().map(|a| (a.index().to_string(), a.name())),
    );

    // Nodes → prefab.
    let graph = source_graph(doc, &file_stem(path), &mesh_subs);
    let prefab = prefab_document(
        &mut ctx.report,
        limits,
        &graph,
        &mesh_subs,
        &meshes,
        &skins,
        meta,
    );
    let Some((prefab, prefab_ids)) = prefab else {
        return fail(ctx);
    };
    if ctx.has_errors() {
        return fail(ctx);
    }

    crate::graph::finish(
        ctx.report,
        meta,
        path,
        IMPORTER,
        IMPORTER_VERSION,
        Produced {
            meshes,
            materials,
            images,
            skins,
            animations,
            prefab,
            prefab_ids,
            keys,
        },
    )
}

/// The default scene as a [`SourceGraph`]: every node with its name,
/// TRS, children, imported mesh and skin; the scene's roots (or, with no
/// scene, every node nobody points at).
fn source_graph(doc: &gltf::Document, stem: &str, mesh_subs: &[MeshSubs]) -> SourceGraph {
    let scene = doc.default_scene().or_else(|| doc.scenes().next());
    let (name, roots): (String, Vec<usize>) = match &scene {
        Some(s) => (
            s.name()
                .map(str::to_string)
                .unwrap_or_else(|| stem.to_string()),
            s.nodes().map(|n| n.index()).collect(),
        ),
        None => {
            let mut has_parent = vec![false; doc.nodes().len()];
            for n in doc.nodes() {
                for c in n.children() {
                    has_parent[c.index()] = true;
                }
            }
            (
                stem.to_string(),
                doc.nodes()
                    .filter(|n| !has_parent[n.index()])
                    .map(|n| n.index())
                    .collect(),
            )
        }
    };
    let nodes = doc
        .nodes()
        .map(|node| {
            let (translation, rotation, scale) = node.transform().decomposed();
            SourceNode {
                index: node.index(),
                name: node_name(&node),
                named: node.name().is_some(),
                translation,
                rotation,
                scale,
                children: node.children().map(|c| c.index()).collect(),
                mesh: node
                    .mesh()
                    .map(|m| m.index())
                    .filter(|m| !mesh_subs[*m].is_empty()),
                skin: node.skin().map(|s| s.index()),
            }
        })
        .collect();
    SourceGraph { name, nodes, roots }
}

/// `Ok(None)` = skipped with a warning; `Err` = an error was recorded.
fn primitive_mesh(
    ctx: &mut Ctx,
    buffers: &[Vec<u8>],
    primitive: &gltf::Primitive,
    sub_id: &str,
    name: &str,
) -> Result<Option<MeshData>, ()> {
    if primitive.mode() != Mode::Triangles {
        ctx.warn(
            Code::IMPORT_SKIPPED,
            format!(
                "{sub_id} ({name}): primitive mode {:?} skipped; only triangle lists are imported",
                primitive.mode()
            ),
        );
        return Ok(None);
    }
    let reader = primitive.reader(|b| buffers.get(b.index()).map(Vec::as_slice));
    let Some(positions) = reader.read_positions() else {
        ctx.warn(
            Code::IMPORT_SKIPPED,
            format!("{sub_id} ({name}): no POSITION attribute; skipped"),
        );
        return Ok(None);
    };
    let positions: Vec<[f32; 3]> = positions.collect();
    let normals: Option<Vec<[f32; 3]>> = reader.read_normals().map(Iterator::collect);
    let tangents: Option<Vec<[f32; 4]>> = reader.read_tangents().map(Iterator::collect);
    let uv0: Option<Vec<[f32; 2]>> = reader.read_tex_coords(0).map(|t| t.into_f32().collect());
    let uv1: Option<Vec<[f32; 2]>> = reader.read_tex_coords(1).map(|t| t.into_f32().collect());
    let colors: Option<Vec<[f32; 4]>> = reader.read_colors(0).map(|c| c.into_rgba_f32().collect());
    let joints: Option<Vec<[u16; 4]>> = reader.read_joints(0).map(|j| j.into_u16().collect());
    let weights: Option<Vec<[f32; 4]>> = reader.read_weights(0).map(|w| w.into_f32().collect());
    let n = positions.len();
    for (attr, len) in [
        ("NORMAL", normals.as_ref().map(Vec::len)),
        ("TANGENT", tangents.as_ref().map(Vec::len)),
        ("TEXCOORD_0", uv0.as_ref().map(Vec::len)),
        ("TEXCOORD_1", uv1.as_ref().map(Vec::len)),
        ("COLOR_0", colors.as_ref().map(Vec::len)),
        ("JOINTS_0", joints.as_ref().map(Vec::len)),
        ("WEIGHTS_0", weights.as_ref().map(Vec::len)),
    ] {
        if let Some(len) = len {
            if len != n {
                ctx.error(
                    Code::IMPORT_INVALID_DATA,
                    format!("{sub_id} ({name}): {attr} has {len} entries, POSITION has {n}"),
                );
                return Err(());
            }
        }
    }
    let indices: Vec<u32> = match reader.read_indices() {
        Some(i) => i.into_u32().collect(),
        None => (0..n as u32).collect(),
    };
    if !indices.len().is_multiple_of(3) {
        ctx.error(
            Code::IMPORT_INVALID_DATA,
            format!(
                "{sub_id} ({name}): {} indices do not form triangles",
                indices.len()
            ),
        );
        return Err(());
    }
    if let Some(bad) = indices.iter().find(|i| **i as usize >= n) {
        ctx.error(
            Code::IMPORT_INVALID_DATA,
            format!("{sub_id} ({name}): index {bad} out of range (vertices: {n})"),
        );
        return Err(());
    }
    // Skinning: both attributes or neither; weights normalized so the
    // shader can trust them (a vertex bound to nothing follows joint 0).
    let skin: Option<Vec<SkinWeight>> = match (joints, weights) {
        (Some(joints), Some(weights)) => Some(
            joints
                .iter()
                .zip(&weights)
                .map(|(j, w)| {
                    let sum: f32 = w.iter().sum();
                    if sum > 0.0 {
                        SkinWeight {
                            joints: *j,
                            weights: w.map(|x| x / sum),
                        }
                    } else {
                        SkinWeight {
                            joints: [0; 4],
                            weights: [1.0, 0.0, 0.0, 0.0],
                        }
                    }
                })
                .collect(),
        ),
        (None, None) => None,
        (joints, _) => {
            let (has, missing) = if joints.is_some() {
                ("JOINTS_0", "WEIGHTS_0")
            } else {
                ("WEIGHTS_0", "JOINTS_0")
            };
            ctx.error(
                Code::IMPORT_INVALID_DATA,
                format!("{sub_id} ({name}): {has} without {missing}"),
            );
            return Err(());
        }
    };

    let vertex = |i: usize| Vertex {
        position: positions[i],
        normal: normals.as_ref().map(|v| v[i]).unwrap_or([0.0, 0.0, 1.0]),
        tangent: tangents.as_ref().map(|v| v[i]).unwrap_or([0.0; 4]),
        uv0: uv0.as_ref().map(|v| v[i]).unwrap_or([0.0; 2]),
        uv1: uv1.as_ref().map(|v| v[i]).unwrap_or([0.0; 2]),
        color: colors.as_ref().map(|v| v[i]).unwrap_or([1.0; 4]),
    };
    let (vertices, indices, skin) = if normals.is_some() {
        ((0..n).map(vertex).collect::<Vec<_>>(), indices, skin)
    } else {
        // No normals: the spec asks for flat shading, which needs one vertex
        // per corner. Unweld and give each triangle its face normal.
        let mut out = Vec::with_capacity(indices.len());
        let mut out_skin = skin.as_ref().map(|_| Vec::with_capacity(indices.len()));
        for tri in indices.chunks_exact(3) {
            let (a, b, c) = (
                glam::Vec3::from(positions[tri[0] as usize]),
                glam::Vec3::from(positions[tri[1] as usize]),
                glam::Vec3::from(positions[tri[2] as usize]),
            );
            let normal = (b - a).cross(c - a).normalize_or_zero();
            let normal = if normal == glam::Vec3::ZERO {
                [0.0, 0.0, 1.0]
            } else {
                normal.to_array()
            };
            for i in tri {
                let mut v = vertex(*i as usize);
                v.normal = normal;
                out.push(v);
                if let (Some(skin), Some(out_skin)) = (&skin, &mut out_skin) {
                    out_skin.push(skin[*i as usize]);
                }
            }
        }
        let count = out.len() as u32;
        (out, (0..count).collect(), out_skin)
    };
    if vertices.len() > ctx.limits.max_vertices {
        ctx.error(
            Code::IMPORT_LIMIT,
            format!(
                "{sub_id} ({name}): {} vertices; the limit is {}",
                vertices.len(),
                ctx.limits.max_vertices
            ),
        );
        return Err(());
    }
    let mut bounds_min = [f32::MAX; 3];
    let mut bounds_max = [f32::MIN; 3];
    for p in &positions {
        for k in 0..3 {
            bounds_min[k] = bounds_min[k].min(p[k]);
            bounds_max[k] = bounds_max[k].max(p[k]);
        }
    }
    if positions.is_empty() {
        bounds_min = [0.0; 3];
        bounds_max = [0.0; 3];
    }
    Ok(Some(MeshData {
        sub_id: sub_id.to_string(),
        name: name.to_string(),
        material: primitive
            .material()
            .index()
            .map(|m| format!("material/{m}")),
        vertices,
        indices,
        bounds_min,
        bounds_max,
        skin: skin.unwrap_or_default(),
    }))
}

/// A node's name as the prefab and the skins/animations use it: the glTF
/// name, or `node<n>` for an unnamed node.
fn node_name(node: &gltf::Node) -> String {
    node.name()
        .map(str::to_string)
        .unwrap_or_else(|| format!("node{}", node.index()))
}

/// One [`SkinData`] per glTF skin (`skin/<s>`): the joints in the skin's
/// order (what `JOINTS_0` indexes), each with its name, its parent's index
/// when the parent is a joint of the same skin, its inverse bind matrix
/// (identity when the file has none) and its rest pose from the node.
/// `Err` when a skin has two joints with the same name (targets resolve
/// by name at runtime) or more joints than the renderer's palette holds.
fn import_skins(
    ctx: &mut Ctx,
    doc: &gltf::Document,
    buffers: &[Vec<u8>],
) -> Result<Vec<SkinData>, ()> {
    let mut parent_of: Vec<Option<usize>> = vec![None; doc.nodes().len()];
    for node in doc.nodes() {
        for child in node.children() {
            parent_of[child.index()] = Some(node.index());
        }
    }
    let mut skins = Vec::new();
    for skin in doc.skins() {
        let sub_id = format!("skin/{}", skin.index());
        let name = skin
            .name()
            .map(str::to_string)
            .unwrap_or_else(|| format!("skin{}", skin.index()));
        let joint_nodes: Vec<gltf::Node> = skin.joints().collect();
        if joint_nodes.is_empty() {
            ctx.error(
                Code::IMPORT_INVALID_DATA,
                format!("{sub_id} ({name}): a skin needs at least one joint"),
            );
            return Err(());
        }
        if joint_nodes.len() > ctx.limits.max_joints {
            ctx.error(
                Code::IMPORT_LIMIT,
                format!(
                    "{sub_id} ({name}): {} joints; the limit is {}",
                    joint_nodes.len(),
                    ctx.limits.max_joints
                ),
            );
            return Err(());
        }
        let reader = skin.reader(|b| buffers.get(b.index()).map(Vec::as_slice));
        let inverse_bind: Vec<[[f32; 4]; 4]> = match reader.read_inverse_bind_matrices() {
            Some(m) => m.collect(),
            None => vec![glam::Mat4::IDENTITY.to_cols_array_2d(); joint_nodes.len()],
        };
        if inverse_bind.len() != joint_nodes.len() {
            ctx.error(
                Code::IMPORT_INVALID_DATA,
                format!(
                    "{sub_id} ({name}): {} inverse bind matrices for {} joints",
                    inverse_bind.len(),
                    joint_nodes.len()
                ),
            );
            return Err(());
        }
        let mut joints = Vec::with_capacity(joint_nodes.len());
        for (j, node) in joint_nodes.iter().enumerate() {
            let joint_name = node_name(node);
            if let Some(dup) = joints.iter().position(|x: &SkinJoint| x.name == joint_name) {
                ctx.error(
                    Code::IMPORT_INVALID_DATA,
                    format!(
                        "{sub_id} ({name}): joints {dup} and {j} are both named {joint_name:?}; joint names must be unique within a skin (animation targets resolve by name)"
                    ),
                );
                return Err(());
            }
            let parent = parent_of[node.index()]
                .and_then(|p| joint_nodes.iter().position(|n| n.index() == p))
                .map(|p| p as u32);
            let (t, r, s) = node.transform().decomposed();
            let mut ibm = [0.0f32; 16];
            for (c, column) in inverse_bind[j].iter().enumerate() {
                ibm[c * 4..c * 4 + 4].copy_from_slice(column);
            }
            joints.push(SkinJoint {
                name: joint_name,
                parent,
                inverse_bind: ibm,
                rest_translation: t,
                rest_rotation: r,
                rest_scale: s,
            });
        }
        skins.push(SkinData {
            sub_id,
            name,
            payload: SkinPayload {
                skeleton: skin.skeleton().map(|n| node_name(&n)).unwrap_or_default(),
                joints,
            },
        });
    }
    Ok(skins)
}

/// One [`AnimationData`] per glTF animation (`anim/<a>`): its translation,
/// rotation and scale channels keyed as authored (step, linear or cubic
/// spline with the spline's tangents kept), targeting nodes by name, and
/// the clip's duration (its last key). Morph-target weight channels are
/// skipped with a warning. `Err` on malformed samplers.
/// Clip markers authored in the animation's `extras` (M3-19c):
/// `{ "markers": [{ "at": <seconds>, "name": "<event>" }] }` — the same
/// shape a rig document uses. Sorted by time; one outside the clip or
/// without a name is invalid data.
fn read_markers(
    ctx: &mut Ctx,
    sub_id: &str,
    name: &str,
    extras: &gltf::json::Extras,
    duration: f32,
) -> Result<Vec<fynkel_cooked::payload::AnimationMarker>, ()> {
    let Some(raw) = extras.as_deref() else {
        return Ok(Vec::new());
    };
    let value: serde_json::Value = match serde_json::from_str(raw.get()) {
        Ok(v) => v,
        Err(_) => return Ok(Vec::new()),
    };
    let Some(list) = value.get("markers") else {
        return Ok(Vec::new());
    };
    let Some(list) = list.as_array() else {
        ctx.error(
            Code::IMPORT_INVALID_DATA,
            format!("{sub_id} ({name}): extras.markers must be an array"),
        );
        return Err(());
    };
    let mut markers = Vec::with_capacity(list.len());
    for (i, m) in list.iter().enumerate() {
        let at = m.get("at").and_then(|v| v.as_f64()).map(|v| v as f32);
        let marker_name = m.get("name").and_then(|v| v.as_str()).unwrap_or("");
        match at {
            Some(t) if t.is_finite() && t >= 0.0 && t <= duration && !marker_name.is_empty() => {
                markers.push(fynkel_cooked::payload::AnimationMarker {
                    time: t,
                    name: marker_name.to_string(),
                });
            }
            _ => {
                ctx.error(
                    Code::IMPORT_INVALID_DATA,
                    format!(
                        "{sub_id} ({name}): extras.markers[{i}] needs a name and an `at` within [0, {duration}]"
                    ),
                );
                return Err(());
            }
        }
    }
    markers.sort_by(|a, b| a.time.total_cmp(&b.time));
    Ok(markers)
}

fn import_animations(
    ctx: &mut Ctx,
    doc: &gltf::Document,
    buffers: &[Vec<u8>],
) -> Result<Vec<AnimationData>, ()> {
    // Animated node names must be unique for the channels to land where
    // the author meant; say so once per name.
    let mut animated = AnimatedNames::default();
    let mut animations = Vec::new();
    for animation in doc.animations() {
        let sub_id = format!("anim/{}", animation.index());
        let name = animation
            .name()
            .map(str::to_string)
            .unwrap_or_else(|| format!("anim{}", animation.index()));
        let mut channels = Vec::new();
        let mut duration = 0.0f32;
        for (c, channel) in animation.channels().enumerate() {
            let target = channel.target();
            let node = target.node();
            let property = match target.property() {
                gltf::animation::Property::Translation => ANIM_PROPERTY_TRANSLATION,
                gltf::animation::Property::Rotation => ANIM_PROPERTY_ROTATION,
                gltf::animation::Property::Scale => ANIM_PROPERTY_SCALE,
                gltf::animation::Property::MorphTargetWeights => {
                    ctx.warn(
                        Code::IMPORT_SKIPPED,
                        format!(
                            "{sub_id} ({name}): channel {c} animates morph target weights; skipped (morph targets are not imported)"
                        ),
                    );
                    continue;
                }
            };
            let interpolation = match channel.sampler().interpolation() {
                gltf::animation::Interpolation::Step => ANIM_INTERPOLATION_STEP,
                gltf::animation::Interpolation::Linear => ANIM_INTERPOLATION_LINEAR,
                gltf::animation::Interpolation::CubicSpline => ANIM_INTERPOLATION_CUBIC_SPLINE,
            };
            let reader = channel.reader(|b| buffers.get(b.index()).map(Vec::as_slice));
            let (Some(inputs), Some(outputs)) = (reader.read_inputs(), reader.read_outputs())
            else {
                ctx.error(
                    Code::IMPORT_INVALID_DATA,
                    format!("{sub_id} ({name}): channel {c} has an unreadable sampler"),
                );
                return Err(());
            };
            let times: Vec<f32> = inputs.collect();
            let values: Vec<f32> = match outputs {
                gltf::animation::util::ReadOutputs::Translations(t) => t.flatten().collect(),
                gltf::animation::util::ReadOutputs::Rotations(r) => {
                    r.into_f32().flatten().collect()
                }
                gltf::animation::util::ReadOutputs::Scales(s) => s.flatten().collect(),
                gltf::animation::util::ReadOutputs::MorphTargetWeights(_) => {
                    ctx.error(
                        Code::IMPORT_INVALID_DATA,
                        format!(
                            "{sub_id} ({name}): channel {c} targets a transform but samples weights"
                        ),
                    );
                    return Err(());
                }
            };
            let channel = AnimationChannel {
                target: node_name(&node),
                property,
                interpolation,
                times,
                values,
            };
            check_channel(&mut ctx.report, &sub_id, &name, c, &channel)?;
            duration = duration.max(*channel.times.last().unwrap());
            animated.note(&mut ctx.report, node.index(), &channel.target);
            channels.push(channel);
        }
        let markers = read_markers(ctx, &sub_id, &name, animation.extras(), duration)?;
        animations.push(AnimationData {
            sub_id,
            name,
            payload: AnimationPayload {
                duration,
                channels,
                markers,
            },
        });
    }
    Ok(animations)
}

/// `fynkel.PbrMaterial`'s document form (formatVersion 2): factors as
/// authored, one `<slot>_texture` reference per bound slot (`image/<i>`;
/// the data slots of an image that is also a colour are re-pointed at its
/// linear variant by [`split_dual_use_images`]). What the model has no
/// field for is reported once and dropped: a second UV set, and samplers
/// other than linear/repeat (samplers come from the texture's import
/// settings).
fn material_document(
    ctx: &mut Ctx,
    material: &gltf::Material,
    sub_id: &str,
    name: &str,
    meta: &Meta,
) -> Value {
    let pbr = material.pbr_metallic_roughness();
    let mut doc = material_header(name, sub_id, &shown_file(ctx.path));
    doc.insert("base_color".into(), nums(&pbr.base_color_factor()));
    doc.insert("metallic".into(), num(pbr.metallic_factor()));
    doc.insert("roughness".into(), num(pbr.roughness_factor()));
    doc.insert("emissive".into(), nums(&material.emissive_factor()));
    if let Some(strength) = material.emissive_strength() {
        doc.insert("emissive_strength".into(), num(strength));
    }
    if let Some(n) = material.normal_texture() {
        doc.insert("normal_scale".into(), num(n.scale()));
    }
    if let Some(o) = material.occlusion_texture() {
        doc.insert("occlusion_strength".into(), num(o.strength()));
    }
    let alpha = match material.alpha_mode() {
        gltf::material::AlphaMode::Opaque => "opaque",
        gltf::material::AlphaMode::Mask => "mask",
        gltf::material::AlphaMode::Blend => "blend",
    };
    if alpha != "opaque" {
        doc.insert("alpha_mode".into(), json!(alpha));
    }
    if alpha == "mask" {
        doc.insert(
            "alpha_cutoff".into(),
            num(material.alpha_cutoff().unwrap_or(0.5)),
        );
    }
    if material.double_sided() {
        doc.insert("double_sided".into(), json!(true));
    }

    let mut bind = |ctx: &mut Ctx, field: &str, texture: &gltf::Texture, tex_coord: u32| {
        let image = texture.source();
        doc.insert(
            field.into(),
            image_ref(meta, &format!("image/{}", image.index())),
        );
        if tex_coord != 0 {
            ctx.warn(
                Code::IMPORT_SKIPPED,
                format!(
                    "{sub_id} ({name}): {field} uses TEXCOORD_{tex_coord}; the PBR material samples set 0"
                ),
            );
        }
        let sampler = texture.sampler();
        if sampler.index().is_some() && !ctx.warned_textures.contains(&texture.index()) {
            let nearest = matches!(
                sampler.mag_filter(),
                Some(gltf::texture::MagFilter::Nearest)
            );
            let wrap = |w: gltf::texture::WrappingMode| match w {
                gltf::texture::WrappingMode::ClampToEdge => "clamp",
                gltf::texture::WrappingMode::MirroredRepeat => "mirror",
                gltf::texture::WrappingMode::Repeat => "repeat",
            };
            let wraps = (wrap(sampler.wrap_s()), wrap(sampler.wrap_t()));
            if nearest || wraps != ("repeat", "repeat") {
                ctx.warned_textures.push(texture.index());
                ctx.warn(
                    Code::IMPORT_SKIPPED,
                    format!(
                        "texture {}: sampler {} wrap S={} T={} is not kept; materials sample linear/repeat (sampler settings belong to the texture's sidecar)",
                        texture.index(),
                        if nearest { "nearest" } else { "linear" },
                        wraps.0,
                        wraps.1
                    ),
                );
            }
        }
    };
    if let Some(t) = pbr.base_color_texture() {
        bind(ctx, "base_color_texture", &t.texture(), t.tex_coord());
    }
    if let Some(t) = material.normal_texture() {
        bind(ctx, "normal_texture", &t.texture(), t.tex_coord());
    }
    if let Some(t) = pbr.metallic_roughness_texture() {
        bind(
            ctx,
            "metallic_roughness_texture",
            &t.texture(),
            t.tex_coord(),
        );
    }
    if let Some(t) = material.occlusion_texture() {
        bind(ctx, "occlusion_texture", &t.texture(), t.tex_coord());
    }
    if let Some(t) = material.emissive_texture() {
        bind(ctx, "emissive_texture", &t.texture(), t.tex_coord());
    }
    Value::Object(doc)
}
