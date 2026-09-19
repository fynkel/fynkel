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
//! What it refuses: buffers or images outside the file unless the
//! sidecar's settings say `"allowExternalFiles": true` (and then only
//! plain relative paths inside the source's directory), required
//! extensions it does not implement (Draco, meshopt, ...), and anything
//! past the [`Limits`]. Non-triangle primitives, skins, morph targets,
//! animations and cameras are skipped with a warning — this is the static
//! mesh path of M2.

use std::path::Path;

use base64::Engine;
use fynkel_contracts::{Code, Diagnostic};
use fynkel_document::asset::Meta;
use fynkel_document::DocumentKind;
use gltf::mesh::Mode;
use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::intermediate::{ColorSpace, ImageData, Intermediate, MaterialData, MeshData, Vertex};
use crate::{derive_sub_id, read_source, Imported, Limits};

pub const IMPORTER: &str = fynkel_document::asset::GLTF_IMPORTER;
pub const IMPORTER_VERSION: u32 = 2;

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
    shown: String,
    asset: Uuid,
    limits: &'a Limits,
    settings: Settings,
    diagnostics: Vec<Diagnostic>,
    buffer_bytes: u64,
    /// Textures already warned about (one warning per texture, not per use).
    warned_textures: Vec<usize>,
}

impl Ctx<'_> {
    fn error(&mut self, code: Code, msg: impl Into<String>) {
        self.diagnostics
            .push(Diagnostic::error(code, msg).with_document(self.shown.clone()));
    }

    fn warn(&mut self, code: Code, msg: impl Into<String>) {
        self.diagnostics
            .push(Diagnostic::warning(code, msg).with_document(self.shown.clone()));
    }

    fn has_errors(&self) -> bool {
        fynkel_contracts::has_errors(&self.diagnostics)
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
            self.diagnostics.push(
                Diagnostic::error(
                    Code::IMPORT_EXTERNAL_URI,
                    format!("{what} lives outside the file ({uri}); external files are not read"),
                )
                .with_document(self.shown.clone())
                .with_suggestion(
                    "export a self-contained .glb, or set \"allowExternalFiles\": true in the sidecar's settings for plain relative paths",
                ),
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
                self.diagnostics.push(
                    d.with_document(self.shown.clone())
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
        shown: path.display().to_string(),
        asset: meta.id(),
        limits,
        settings: Settings::from_meta(meta),
        diagnostics: Vec::new(),
        buffer_bytes: 0,
        warned_textures: Vec::new(),
    };
    let fail = |ctx: Ctx| Imported {
        intermediate: None,
        sub_assets: meta.sub_assets.clone(),
        diagnostics: ctx.diagnostics,
    };

    let bytes = match read_source(path, limits) {
        Ok(b) => b,
        Err(d) => {
            ctx.diagnostics.push(d);
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
            ctx.diagnostics.push(
                Diagnostic::error(
                    Code::IMPORT_UNSUPPORTED,
                    format!(
                        "the file requires an extension this importer does not support ({})",
                        which.join(", ")
                    ),
                )
                .with_document(ctx.shown.clone())
                .with_suggestion("re-export without mesh compression or other required extensions"),
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
            ctx.diagnostics.push(
                Diagnostic::error(
                    Code::IMPORT_UNSUPPORTED,
                    format!("required extension {ext} is not supported"),
                )
                .with_document(ctx.shown.clone())
                .with_suggestion("re-export without mesh compression or other required extensions"),
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
    let mut mesh_subs: Vec<Vec<(String, String)>> = vec![Vec::new(); doc.meshes().len()];
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

    // Skipped features, once each.
    if doc.skins().len() > 0 {
        ctx.warn(
            Code::IMPORT_SKIPPED,
            format!(
                "{} skin(s) skipped (skinning is not imported yet)",
                doc.skins().len()
            ),
        );
    }
    if doc.animations().len() > 0 {
        ctx.warn(
            Code::IMPORT_SKIPPED,
            format!(
                "{} animation(s) skipped (animation import is not part of M2)",
                doc.animations().len()
            ),
        );
    }
    if doc.cameras().len() > 0 {
        ctx.warn(
            Code::IMPORT_SKIPPED,
            format!("{} camera(s) skipped", doc.cameras().len()),
        );
    }

    // Nodes → prefab.
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "model".to_string());
    let prefab = match prefab_document(&mut ctx, doc, &mesh_subs, &stem, meta) {
        Some(p) => p,
        None => return fail(ctx),
    };
    if ctx.has_errors() {
        return fail(ctx);
    }

    // Sub-asset ids: sidecar first, derived otherwise.
    let mut sub_assets = Vec::new();
    for sub in meshes
        .iter()
        .map(|m| m.sub_id.as_str())
        .chain(materials.iter().map(|m| m.sub_id.as_str()))
        .chain(images.iter().map(|i| i.sub_id.as_str()))
        .chain(std::iter::once("prefab"))
    {
        let id = meta
            .sub_assets
            .iter()
            .find(|(k, _)| k == sub)
            .map(|(_, v)| *v)
            .unwrap_or_else(|| derive_sub_id(ctx.asset, sub));
        sub_assets.push((sub.to_string(), id));
    }
    let dropped: Vec<&str> = meta
        .sub_assets
        .iter()
        .filter(|(k, _)| !sub_assets.iter().any(|(s, _)| s == k))
        .map(|(k, _)| k.as_str())
        .collect();
    if !dropped.is_empty() {
        ctx.warn(
            Code::IMPORT_SKIPPED,
            format!(
                "sidecar sub-asset(s) no longer produced by the file: {} (references to them will not resolve)",
                dropped.join(", ")
            ),
        );
    }
    // Patch the ids the sub-assets were given into the documents that carry them.
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
    let mut prefab = prefab;
    prefab["id"] = Value::String(lookup("prefab").to_string());

    Imported {
        intermediate: Some(Intermediate {
            importer: IMPORTER.to_string(),
            importer_version: IMPORTER_VERSION,
            source: path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            meshes,
            materials,
            images,
            textures: Vec::new(),
            clips: Vec::new(),
            prefab: Some(prefab),
            sub_assets: sub_assets.clone(),
        }),
        sub_assets,
        diagnostics: ctx.diagnostics,
    }
}

fn sniff_mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG") {
        "image/png"
    } else if bytes.starts_with(&[0xff, 0xd8]) {
        "image/jpeg"
    } else {
        "application/octet-stream"
    }
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
    let n = positions.len();
    for (attr, len) in [
        ("NORMAL", normals.as_ref().map(Vec::len)),
        ("TANGENT", tangents.as_ref().map(Vec::len)),
        ("TEXCOORD_0", uv0.as_ref().map(Vec::len)),
        ("TEXCOORD_1", uv1.as_ref().map(Vec::len)),
        ("COLOR_0", colors.as_ref().map(Vec::len)),
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

    let vertex = |i: usize| Vertex {
        position: positions[i],
        normal: normals.as_ref().map(|v| v[i]).unwrap_or([0.0, 0.0, 1.0]),
        tangent: tangents.as_ref().map(|v| v[i]).unwrap_or([0.0; 4]),
        uv0: uv0.as_ref().map(|v| v[i]).unwrap_or([0.0; 2]),
        uv1: uv1.as_ref().map(|v| v[i]).unwrap_or([0.0; 2]),
        color: colors.as_ref().map(|v| v[i]).unwrap_or([1.0; 4]),
    };
    let (vertices, indices) = if normals.is_some() {
        ((0..n).map(vertex).collect::<Vec<_>>(), indices)
    } else {
        // No normals: the spec asks for flat shading, which needs one vertex
        // per corner. Unweld and give each triangle its face normal.
        let mut out = Vec::with_capacity(indices.len());
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
            }
        }
        let count = out.len() as u32;
        (out, (0..count).collect())
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
    }))
}

/// A number as the document should show it: the shortest text that
/// round-trips the `f32`, so `0.8f32` is `0.8`, not `0.800000011920929`.
fn num(x: f32) -> Value {
    let text = if x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{x:.1}")
    } else {
        x.to_string()
    };
    serde_json::Number::from_f64(text.parse::<f64>().unwrap_or(x as f64))
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

fn nums(v: &[f32]) -> Value {
    Value::Array(v.iter().map(|x| num(*x)).collect())
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
    let mut doc = Map::new();
    doc.insert("$schema".into(), json!(DocumentKind::Material.schema_id()));
    doc.insert(
        "formatVersion".into(),
        json!(DocumentKind::Material.supported_format_version()),
    );
    doc.insert("id".into(), json!(Uuid::nil().to_string())); // patched by the caller
    doc.insert("name".into(), json!(name));
    doc.insert(
        "description".into(),
        json!(format!("Imported from {} ({sub_id}).", ctx.shown_file())),
    );
    doc.insert("shader".into(), json!("fynkel.pbr@1"));
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
            json!({ "asset": meta.id().to_string(), "sub": format!("image/{}", image.index()) }),
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
fn split_dual_use_images(materials: &mut [MaterialData], images: &mut Vec<ImageData>) {
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

impl Ctx<'_> {
    fn shown_file(&self) -> String {
        self.path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// The default scene as a prefab document body (`id` patched by the caller).
fn prefab_document(
    ctx: &mut Ctx,
    doc: &gltf::Document,
    mesh_subs: &[Vec<(String, String)>],
    stem: &str,
    meta: &Meta,
) -> Option<Value> {
    let scene = doc.default_scene().or_else(|| doc.scenes().next());
    let (scene_name, roots): (String, Vec<gltf::Node>) = match &scene {
        Some(s) => (
            s.name()
                .map(str::to_string)
                .unwrap_or_else(|| stem.to_string()),
            s.nodes().collect(),
        ),
        None => {
            // No scenes: every node nobody points at is a root.
            let mut has_parent = vec![false; doc.nodes().len()];
            for n in doc.nodes() {
                for c in n.children() {
                    has_parent[c.index()] = true;
                }
            }
            (
                stem.to_string(),
                doc.nodes().filter(|n| !has_parent[n.index()]).collect(),
            )
        }
    };
    if doc.nodes().len() > ctx.limits.max_nodes {
        ctx.error(
            Code::IMPORT_LIMIT,
            format!(
                "{} nodes; the limit is {}",
                doc.nodes().len(),
                ctx.limits.max_nodes
            ),
        );
        return None;
    }

    let asset = ctx.asset;
    let entity_id = |node: usize| derive_sub_id(asset, &format!("node/{node}")).to_string();
    let mut entities: Vec<(String, Value)> = Vec::new();
    let mut visited = vec![false; doc.nodes().len()];
    let root_ids: Vec<String> = roots.iter().map(|r| entity_id(r.index())).collect();

    // Explicit stack: (node, parent entity id, depth); children pushed in
    // reverse so document order is depth-first, first child first.
    let mut stack: Vec<(gltf::Node, Option<String>, usize)> =
        roots.iter().rev().map(|r| (r.clone(), None, 1)).collect();

    while let Some((node, parent, depth)) = stack.pop() {
        if depth > ctx.limits.max_node_depth {
            ctx.error(
                Code::IMPORT_LIMIT,
                format!(
                    "node {} is nested deeper than {} levels",
                    node.index(),
                    ctx.limits.max_node_depth
                ),
            );
            return None;
        }
        if visited[node.index()] {
            ctx.error(
                Code::IMPORT_INVALID_DATA,
                format!(
                    "node {} ({}) is reached twice: the node graph has a cycle or a node with two parents",
                    node.index(),
                    node.name().unwrap_or("")
                ),
            );
            return None;
        }
        visited[node.index()] = true;
        let id = entity_id(node.index());
        let name = node
            .name()
            .map(str::to_string)
            .unwrap_or_else(|| format!("node{}", node.index()));
        let (translation, rotation, scale) = node.transform().decomposed();

        let mut components: Vec<(String, Value)> = vec![(
            derive_sub_id(asset, &format!("node/{}/transform", node.index())).to_string(),
            transform_component(translation, rotation, scale),
        )];
        let mut children: Vec<String> = Vec::new();
        let mut primitive_entities: Vec<(String, Value)> = Vec::new();
        if let Some(mesh) = node.mesh() {
            let subs = &mesh_subs[mesh.index()];
            if subs.len() == 1 {
                components.push((
                    derive_sub_id(asset, &format!("node/{}/renderer", node.index())).to_string(),
                    renderer_component(meta, &subs[0].0, mesh_material(doc, mesh.index(), 0)),
                ));
            } else {
                for (p, (sub, sub_name)) in subs.iter().enumerate() {
                    let child_id =
                        derive_sub_id(asset, &format!("node/{}/primitive/{p}", node.index()))
                            .to_string();
                    children.push(child_id.clone());
                    let prim_components: Vec<(String, Value)> = vec![
                        (
                            derive_sub_id(
                                asset,
                                &format!("node/{}/primitive/{p}/transform", node.index()),
                            )
                            .to_string(),
                            transform_component([0.0; 3], [0.0, 0.0, 0.0, 1.0], [1.0; 3]),
                        ),
                        (
                            derive_sub_id(
                                asset,
                                &format!("node/{}/primitive/{p}/renderer", node.index()),
                            )
                            .to_string(),
                            renderer_component(
                                meta,
                                sub,
                                mesh_material_by_sub(doc, mesh.index(), sub),
                            ),
                        ),
                    ];
                    primitive_entities.push((
                        child_id.clone(),
                        entity_value(sub_name, Some(&id), Vec::new(), prim_components),
                    ));
                }
            }
        }
        let node_children: Vec<gltf::Node> = node.children().collect();
        for c in &node_children {
            children.push(entity_id(c.index()));
        }
        entities.push((
            id.clone(),
            entity_value(&name, parent.as_deref(), children, components),
        ));
        entities.extend(primitive_entities);
        for c in node_children.into_iter().rev() {
            stack.push((c, Some(id.clone()), depth + 1));
        }
    }

    let mut prefab = Map::new();
    prefab.insert("$schema".into(), json!(DocumentKind::Prefab.schema_id()));
    prefab.insert("formatVersion".into(), json!(1));
    prefab.insert("id".into(), json!(Uuid::nil().to_string()));
    prefab.insert("name".into(), json!(scene_name));
    prefab.insert(
        "description".into(),
        json!(format!("Imported from {}.", ctx.shown_file())),
    );
    let mut ents = Map::new();
    for (k, v) in entities {
        ents.insert(k, v);
    }
    prefab.insert("entities".into(), Value::Object(ents));
    prefab.insert("roots".into(), json!(root_ids));
    Some(Value::Object(prefab))
}

fn mesh_material(doc: &gltf::Document, mesh: usize, primitive: usize) -> Option<String> {
    doc.meshes()
        .nth(mesh)
        .and_then(|m| m.primitives().nth(primitive))
        .and_then(|p| p.material().index())
        .map(|m| format!("material/{m}"))
}

fn mesh_material_by_sub(doc: &gltf::Document, mesh: usize, sub: &str) -> Option<String> {
    let p: usize = sub.rsplit('/').next()?.parse().ok()?;
    mesh_material(doc, mesh, p)
}

fn entity_value(
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

fn transform_component(t: [f32; 3], r: [f32; 4], s: [f32; 3]) -> Value {
    json!({
        "type": "fynkel.Transform",
        "version": 1,
        "data": { "position": nums(&t), "rotation": nums(&r), "scale": nums(&s) },
    })
}

fn renderer_component(meta: &Meta, mesh_sub: &str, material_sub: Option<String>) -> Value {
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
