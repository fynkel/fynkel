//! `.glb` files (binary glTF 2.0) into [`ModelData`]: meshes, images,
//! materials and the node tree, as plain values (`docs/stage-2.md` §3.3).
//!
//! Animations come as [`ClipData`], one per glTF animation.
//!
//! What the engine can't draw yet (skins, morph targets,
//! cameras, lights, see-through materials, ...) is skipped with one
//! readable warning per kind, never an error. A file is refused only when
//! it can't be read at all, or needs an extension this reader doesn't
//! know (`extensionsRequired`).

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::sync::Arc;

use fynkel_mesh::{MeshData, MeshError, MeshParts};
use gltf::accessor::{self, DataType, Dimensions, Item};
use gltf::mesh::Mode;
use gltf::texture::{MagFilter, MinFilter, WrappingMode};

use crate::clip::{ChannelData, ClipData, Interpolation, Property};
use crate::{ImageData, ImageError};

/// Extensions this reader understands, so a file may require them.
const SUPPORTED: &[&str] = &["KHR_texture_transform", "KHR_materials_emissive_strength", "KHR_mesh_quantization"];

/// The largest UV scale or offset: the world's limit for a material.
const MAX_UV: f32 = 1e4;
/// The largest emissive strength: the world's limit for an intensity.
const MAX_EMISSIVE_STRENGTH: f32 = 1e6;

/// A model read from a `.glb`. Meshes and images are shared (`Arc`), so
/// every world and every copy of the model uses the same data.
#[derive(Clone, Debug, Default)]
pub struct ModelData {
    name: String,
    meshes: Vec<Arc<MeshData>>,
    images: Vec<Arc<ImageData>>,
    materials: Vec<MaterialData>,
    nodes: Vec<NodeData>,
    roots: Vec<usize>,
    clips: Vec<Arc<ClipData>>,
    warnings: Vec<String>,
}

/// A glTF material, as plain values. Colours are linear, as glTF stores
/// them. Each texture is an index into [`ModelData::images`]; the slot
/// decides whether it's read as colour (sRGB) or data.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialData {
    pub name: String,
    /// Linear RGBA.
    pub base_color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    /// Linear RGB.
    pub emissive: [f32; 3],
    /// `KHR_materials_emissive_strength`, 1 without it.
    pub emissive_strength: f32,
    /// 0 for opaque.
    pub alpha_cutoff: f32,
    /// glTF's BLEND: laid over what's behind by its alpha.
    pub transparent: bool,
    pub double_sided: bool,
    /// `KHR_texture_transform`, without rotation.
    pub uv_scale: [f32; 2],
    pub uv_offset: [f32; 2],
    pub base_color_texture: Option<usize>,
    pub normal_texture: Option<usize>,
    pub metallic_roughness_texture: Option<usize>,
    pub occlusion_texture: Option<usize>,
    pub emissive_texture: Option<usize>,
}

impl MaterialData {
    /// glTF's default material, for primitives that don't name one.
    pub const DEFAULT: Self = Self {
        name: String::new(),
        base_color: [1.0; 4],
        metallic: 1.0,
        roughness: 1.0,
        emissive: [0.0; 3],
        emissive_strength: 1.0,
        alpha_cutoff: 0.0,
        transparent: false,
        double_sided: false,
        uv_scale: [1.0; 2],
        uv_offset: [0.0; 2],
        base_color_texture: None,
        normal_texture: None,
        metallic_roughness_texture: None,
        occlusion_texture: None,
        emissive_texture: None,
    };
}

/// A node: an object when the model is spawned.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeData {
    pub name: String,
    pub translation: [f32; 3],
    /// A unit quaternion, xyzw.
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    /// What it draws: a glTF mesh's primitives, each a mesh with a
    /// material.
    pub parts: Vec<Part>,
    /// Indices into [`ModelData::nodes`].
    pub children: Vec<usize>,
}

/// One drawn primitive: indices into [`ModelData::meshes`] and
/// [`ModelData::materials`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Part {
    pub mesh: usize,
    pub material: usize,
}

impl ModelData {
    /// Reads a `.glb`. Slow for a big file (images are decoded and
    /// tangents generated here), so a game calls it on a helper isolate.
    pub fn from_glb(bytes: &[u8]) -> Result<Self, ModelError> {
        // The gltf crate panics on some damaged files (an index out of
        // range while it validates; tests/damaged.rs), so its panic becomes
        // the error it should have been.
        std::panic::catch_unwind(|| Reader::read(bytes)).unwrap_or_else(|panic| {
            let reason = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_default();
            Err(ModelError::Invalid(format!("the file is damaged ({reason})")))
        })
    }

    /// The scene's name, or `""`.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn meshes(&self) -> &[Arc<MeshData>] {
        &self.meshes
    }

    pub fn images(&self) -> &[Arc<ImageData>] {
        &self.images
    }

    pub fn materials(&self) -> &[MaterialData] {
        &self.materials
    }

    pub fn nodes(&self) -> &[NodeData] {
        &self.nodes
    }

    /// The scene's top-level nodes.
    pub fn roots(&self) -> &[usize] {
        &self.roots
    }

    /// The model's animations, in the file's order. Shared: every copy of
    /// the model plays the same ones.
    pub fn clips(&self) -> &[Arc<ClipData>] {
        &self.clips
    }

    /// What was skipped, one line per kind, in the order met.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }
}

/// Why a `.glb` couldn't be read.
#[derive(Clone, Debug, PartialEq)]
pub enum ModelError {
    Empty,
    /// A `.gltf` (JSON) file, which would need its other files.
    NotBinary,
    /// Not glTF at all.
    Unrecognized,
    /// The glTF itself is broken.
    Invalid(String),
    /// It needs an extension this reader doesn't know.
    Requires(String),
    /// Data stored outside the `.glb`.
    ExternalFile(String),
    Image {
        index: usize,
        error: ImageError,
    },
    Mesh {
        name: String,
        primitive: usize,
        error: String,
    },
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelError::Empty => write!(f, "the model is empty (0 bytes)"),
            ModelError::NotBinary => write!(
                f,
                "this is a .gltf (JSON) file; only .glb (binary glTF) can be loaded, since it holds everything in one file"
            ),
            ModelError::Unrecognized => write!(f, "the model isn't a .glb (binary glTF) file"),
            ModelError::Invalid(reason) => write!(f, "the .glb is broken: {reason}"),
            ModelError::Requires(extension) => {
                write!(f, "the model needs the glTF extension {extension}, which isn't supported")
            }
            ModelError::ExternalFile(uri) => {
                write!(f, "the model refers to a file outside the .glb ({uri}); only self-contained .glb files load")
            }
            ModelError::Image { index, error } => write!(f, "image {index} in the model: {error}"),
            ModelError::Mesh { name, primitive, error } => write!(f, "mesh '{name}', primitive {primitive}: {error}"),
        }
    }
}

impl std::error::Error for ModelError {}

/// The state of one read.
struct Reader<'a> {
    document: gltf::Document,
    blob: &'a [u8],
    model: ModelData,
    warned: BTreeSet<String>,
    /// Primitives' vertex data (accessors and mode) → our mesh, so a glTF
    /// mesh used by many nodes, or primitives that differ only in
    /// material, are read once.
    geometry: HashMap<Geometry, usize>,
    /// glTF material index (`None`: the default) → ours.
    materials: HashMap<Option<usize>, usize>,
}

impl<'a> Reader<'a> {
    fn read(bytes: &'a [u8]) -> Result<ModelData, ModelError> {
        if bytes.is_empty() {
            return Err(ModelError::Empty);
        }
        if !bytes.starts_with(b"glTF") {
            let text = bytes.iter().find(|b| !b.is_ascii_whitespace());
            return Err(if text == Some(&b'{') { ModelError::NotBinary } else { ModelError::Unrecognized });
        }
        let glb = gltf::binary::Glb::from_slice(bytes).map_err(|e| ModelError::Invalid(e.to_string()))?;
        let json: gltf::json::Root =
            gltf::json::deserialize::from_slice(&glb.json).map_err(|e| ModelError::Invalid(e.to_string()))?;
        let document = gltf::Document::from_json_without_validation(json);
        // Checked before validating, which would only say "Unsupported
        // extension".
        if let Some(missing) = document.extensions_required().find(|e| !SUPPORTED.contains(e)) {
            return Err(ModelError::Requires(missing.to_owned()));
        }
        // gltf has no feature for quantization and calls any required
        // extension it doesn't know unsupported; `floats` reads it.
        let mut json = document.into_json();
        json.extensions_required.retain(|e| e != "KHR_mesh_quantization");
        let document = gltf::Document::from_json(json).map_err(|e| ModelError::Invalid(e.to_string()))?;
        let blob = match glb.bin {
            Some(std::borrow::Cow::Borrowed(blob)) => blob,
            // `from_slice` borrows; an owned chunk can't happen.
            Some(std::borrow::Cow::Owned(_)) => unreachable!("Glb::from_slice borrows"),
            None => &[],
        };
        let mut reader = Reader {
            document,
            blob,
            model: ModelData::default(),
            warned: BTreeSet::new(),
            geometry: HashMap::new(),
            materials: HashMap::new(),
        };
        reader.read_all()?;
        Ok(reader.model)
    }

    fn warn(&mut self, message: impl Into<String>) {
        let message = message.into();
        if self.warned.insert(message.clone()) {
            self.model.warnings.push(message);
        }
    }

    fn read_all(&mut self) -> Result<(), ModelError> {
        let document = self.document.clone();
        for extension in document.extensions_used() {
            match extension {
                "KHR_texture_transform" | "KHR_materials_emissive_strength" | "KHR_mesh_quantization" => {}
                // Said per material, where it applies.
                "KHR_materials_unlit" => {}
                "KHR_lights_punctual" => self.warn("the model's lights are skipped; light the scene from code"),
                other => self.warn(format!("the glTF extension {other} isn't supported; the model is read without it")),
            }
        }
        if document.skins().len() > 0 {
            self.warn("skins are skipped; skinned meshes show their bind pose");
        }
        if document.cameras().len() > 0 {
            self.warn("the model's cameras are skipped");
        }

        // Every image, decoded once, whichever slots use it.
        for image in document.images() {
            let bytes = self.image_bytes(&image)?;
            let decoded =
                ImageData::decode(bytes).map_err(|error| ModelError::Image { index: image.index(), error })?;
            self.model.images.push(Arc::new(decoded));
        }

        let scene = document.default_scene().or_else(|| document.scenes().next());
        self.model.name = scene.as_ref().and_then(|s| s.name()).unwrap_or_default().to_owned();
        let roots: Vec<gltf::Node> = match &scene {
            Some(scene) => scene.nodes().collect(),
            // No scene: every node no other node lists as a child.
            None => {
                let children: BTreeSet<usize> =
                    document.nodes().flat_map(|n| n.children().map(|c| c.index())).collect();
                document.nodes().filter(|n| !children.contains(&n.index())).collect()
            }
        };

        // Nodes in the order met, depth first. A node listed under two
        // parents (or itself) is broken glTF; it's used once.
        let mut ours: HashMap<usize, usize> = HashMap::new();
        let mut stack: Vec<(gltf::Node, Option<usize>)> = roots.into_iter().rev().map(|n| (n, None)).collect();
        while let Some((node, parent)) = stack.pop() {
            if ours.contains_key(&node.index()) {
                self.warn("a node is listed more than once in the tree; it's used once");
                continue;
            }
            let index = self.model.nodes.len();
            ours.insert(node.index(), index);
            let data = self.node(&node)?;
            self.model.nodes.push(data);
            match parent {
                Some(parent) => self.model.nodes[parent].children.push(index),
                None => self.model.roots.push(index),
            }
            let children: Vec<_> = node.children().collect();
            stack.extend(children.into_iter().rev().map(|c| (c, Some(index))));
        }
        for animation in document.animations() {
            let clip = self.clip(&animation, &ours);
            self.model.clips.push(Arc::new(clip));
        }
        Ok(())
    }

    /// A glTF animation as a clip. A channel that can't be used (morph
    /// weights, a node outside the tree, broken keys) is left out with a
    /// warning; the rest of the clip plays.
    fn clip(&mut self, animation: &gltf::Animation, ours: &HashMap<usize, usize>) -> ClipData {
        use gltf::animation::{Interpolation as Gltf, Property as Target};
        let name = match animation.name() {
            Some(name) if !name.is_empty() => name.to_owned(),
            _ => format!("animation {}", animation.index()),
        };
        let mut channels = Vec::new();
        for channel in animation.channels() {
            let target = channel.target();
            let property = match target.property() {
                Target::Translation => Property::Translation,
                Target::Rotation => Property::Rotation,
                Target::Scale => Property::Scale,
                Target::MorphTargetWeights => {
                    self.warn("animated morph target weights are skipped");
                    continue;
                }
            };
            let Some(&node) = ours.get(&target.node().index()) else {
                self.warn("an animation moves a node that isn't in the scene; that channel is skipped");
                continue;
            };
            let sampler = channel.sampler();
            let interpolation = match sampler.interpolation() {
                Gltf::Step => Interpolation::Step,
                Gltf::Linear => Interpolation::Linear,
                Gltf::CubicSpline => Interpolation::Cubic,
            };
            // Times are plain floats. Each read checks the accessor's shape
            // first: gltf panics on a VEC4 read as a VEC3.
            let (input, output) = (sampler.input(), sampler.output());
            let shape = if property == Property::Rotation { Dimensions::Vec4 } else { Dimensions::Vec3 };
            let times: Option<Vec<f32>> = match (input.data_type(), input.dimensions()) {
                (DataType::F32, Dimensions::Scalar) => {
                    accessor::Iter::<f32>::new(input, |b: gltf::Buffer<'_>| match b.source() {
                        gltf::buffer::Source::Bin => Some(self.blob),
                        gltf::buffer::Source::Uri(_) => None,
                    })
                    .map(|t| t.collect())
                }
                _ => None,
            };
            let values: Option<Vec<[f32; 4]>> = match property {
                _ if output.dimensions() != shape => None,
                Property::Rotation => floats::<4>(output, self.blob),
                _ => floats::<3>(output, self.blob).map(|v| v.into_iter().map(|[x, y, z]| [x, y, z, 0.0]).collect()),
            };
            let per_key = if interpolation == Interpolation::Cubic { 3 } else { 1 };
            let usable = |times: &[f32], values: &[[f32; 4]]| {
                !times.is_empty()
                    && values.len() == times.len() * per_key
                    && times.iter().all(|t| t.is_finite() && *t >= 0.0)
                    && times.windows(2).all(|w| w[0] <= w[1])
                    && values.iter().flatten().all(|v| v.is_finite())
            };
            match (times, values) {
                (Some(times), Some(mut values)) if usable(&times, &values) => {
                    if property == Property::Rotation && interpolation != Interpolation::Cubic {
                        values.iter_mut().for_each(|q| *q = crate::clip::normalize(*q));
                    }
                    channels.push(ChannelData { node, property, interpolation, times, values });
                }
                _ => self.warn(format!("animation '{name}' has a channel with broken keys; it's skipped")),
            }
        }
        let length = channels.iter().filter_map(|c| c.times.last().copied()).fold(0.0, f32::max);
        ClipData { name, length, channels }
    }

    fn image_bytes(&self, image: &gltf::Image) -> Result<&'a [u8], ModelError> {
        match image.source() {
            gltf::image::Source::View { view, .. } => self.view_bytes(&view),
            gltf::image::Source::Uri { uri, .. } => Err(ModelError::ExternalFile(shorten(uri))),
        }
    }

    fn view_bytes(&self, view: &gltf::buffer::View) -> Result<&'a [u8], ModelError> {
        if let gltf::buffer::Source::Uri(uri) = view.buffer().source() {
            return Err(ModelError::ExternalFile(shorten(uri)));
        }
        let range = view.offset()..view.offset() + view.length();
        self.blob.get(range).ok_or_else(|| ModelError::Invalid("a buffer view goes past the end of the data".into()))
    }

    fn node(&mut self, node: &gltf::Node) -> Result<NodeData, ModelError> {
        let (translation, rotation, scale) = node.transform().decomposed();
        let (translation, rotation, scale) = self.usable_transform(translation, rotation, scale);
        if node.camera().is_some() {
            self.warn("the model's cameras are skipped");
        }
        if node.weights().is_some() || node.mesh().is_some_and(|m| m.weights().is_some()) {
            self.warn("morph targets are skipped; meshes show their base shape");
        }
        let parts = match node.mesh() {
            Some(mesh) => self.mesh_parts(&mesh)?,
            None => Vec::new(),
        };
        Ok(NodeData {
            name: node.name().unwrap_or_default().to_owned(),
            translation,
            rotation,
            scale,
            parts,
            children: Vec::new(),
        })
    }

    /// A transform the world accepts: finite, with a unit rotation. glTF
    /// requires both, but exporters slip, and a matrix with a zero scale
    /// decomposes into NaN.
    fn usable_transform(
        &mut self,
        translation: [f32; 3],
        rotation: [f32; 4],
        scale: [f32; 3],
    ) -> ([f32; 3], [f32; 4], [f32; 3]) {
        let finite = |v: &[f32]| v.iter().all(|x| x.is_finite());
        let length = rotation.iter().map(|x| x * x).sum::<f32>().sqrt();
        if !(finite(&translation) && finite(&rotation) && finite(&scale)) || length < 1e-6 {
            self.warn("a node's transform can't be used (not finite, or a zero scale in a matrix); it's left out");
            return ([0.0; 3], [0.0, 0.0, 0.0, 1.0], [1.0; 3]);
        }
        (translation, rotation.map(|x| x / length), scale)
    }

    fn mesh_parts(&mut self, mesh: &gltf::Mesh) -> Result<Vec<Part>, ModelError> {
        let mut parts = Vec::new();
        for primitive in mesh.primitives() {
            if let Some(mesh_index) = self.primitive(mesh, &primitive)? {
                let material = self.material(primitive.material());
                parts.push(Part { mesh: mesh_index, material });
            }
        }
        Ok(parts)
    }

    /// The primitive as a mesh, or `None` when it isn't triangles.
    fn primitive(&mut self, mesh: &gltf::Mesh, primitive: &gltf::Primitive) -> Result<Option<usize>, ModelError> {
        let accessor = |semantic| primitive.get(&semantic).map(|a| a.index());
        let geometry = Geometry {
            positions: accessor(gltf::Semantic::Positions),
            normals: accessor(gltf::Semantic::Normals),
            tangents: accessor(gltf::Semantic::Tangents),
            uvs: accessor(gltf::Semantic::TexCoords(0)),
            indices: primitive.indices().map(|a| a.index()),
            mode: primitive.mode().as_gl_enum(),
        };
        if let Some(&index) = self.geometry.get(&geometry) {
            return Ok(Some(index));
        }
        let fail = |error: String| ModelError::Mesh {
            name: mesh.name().unwrap_or("").to_owned(),
            primitive: primitive.index(),
            error,
        };
        let blob = self.blob;
        // A sparse accessor needs no bytes for the values it leaves at 0,
        // so a small file can claim billions of them. Every real one has a
        // byte or more each in the file: more than that, the file is
        // damaged.
        let accessors = primitive.attributes().map(|(_, a)| a).chain(primitive.indices());
        for accessor in accessors {
            if accessor.count() > blob.len() {
                return Err(fail(format!(
                    "accessor {} claims {} values, more than the file has bytes",
                    accessor.index(),
                    accessor.count()
                )));
            }
        }
        let reader = primitive.reader(|buffer| match buffer.source() {
            gltf::buffer::Source::Bin => Some(blob),
            gltf::buffer::Source::Uri(_) => None,
        });
        let attribute = |semantic| primitive.get(&semantic);
        let Some(positions) = attribute(gltf::Semantic::Positions).and_then(|a| floats::<3>(a, blob)) else {
            return Err(fail("it has no positions".into()));
        };
        let normals = attribute(gltf::Semantic::Normals).and_then(|a| floats::<3>(a, blob));
        // Without normals, glTF says flat shading; the mesh computes
        // smooth ones from shared vertices, which for an exporter's
        // per-face vertices is the same thing. Tangents are only used with
        // the normals they were made for.
        let tangents =
            attribute(gltf::Semantic::Tangents).filter(|_| normals.is_some()).and_then(|a| floats::<4>(a, blob));
        let uvs = attribute(gltf::Semantic::TexCoords(0)).and_then(|a| floats::<2>(a, blob));
        let listed: Vec<u32> = match reader.read_indices() {
            Some(indices) => indices.into_u32().collect(),
            None => (0..positions.len() as u32).collect(),
        };
        if reader.read_colors(0).is_some() {
            self.warn("vertex colours are skipped");
        }
        if primitive.morph_targets().len() > 0 {
            self.warn("morph targets are skipped; meshes show their base shape");
        }
        let indices = match primitive.mode() {
            Mode::Triangles => listed,
            Mode::TriangleStrip => strip(&listed),
            Mode::TriangleFan => fan(&listed),
            Mode::Points | Mode::Lines | Mode::LineLoop | Mode::LineStrip => {
                self.warn("points and lines are skipped; only triangles are drawn");
                return Ok(None);
            }
        };
        if indices.is_empty() {
            return Ok(None);
        }
        let (normals_given, tangents_given) = (normals, tangents);
        let given = |normals: bool, tangents: bool| {
            MeshData::new(MeshParts {
                positions: &positions,
                normals: if normals { normals_given.as_deref() } else { None },
                tangents: if tangents { tangents_given.as_deref() } else { None },
                uvs: uvs.as_deref(),
                indices: &indices,
            })
        };
        let mesh_data = match given(true, true) {
            Ok(mesh) => mesh,
            // Normals or tangents the mesh checks refuse (zero length, NaN,
            // a tangent w that isn't ±1) are computed instead: exporters
            // slip, and the shape is still fine.
            Err(
                MeshError::NotFinite { what: "normal" | "tangent", .. }
                | MeshError::ZeroLength { what: "normal" | "tangent", .. }
                | MeshError::TangentSign { .. },
            ) => {
                self.warn("some normals or tangents in the model can't be used; they're computed instead");
                given(false, false).map_err(|e| fail(e.to_string()))?
            }
            Err(e) => return Err(fail(e.to_string())),
        };
        self.model.meshes.push(Arc::new(mesh_data));
        let index = self.model.meshes.len() - 1;
        self.geometry.insert(geometry, index);
        Ok(Some(index))
    }
}

impl Reader<'_> {
    /// Ours for a glTF material, made once.
    fn material(&mut self, material: gltf::Material) -> usize {
        if let Some(&index) = self.materials.get(&material.index()) {
            return index;
        }
        let data = match material.index() {
            Some(_) => self.material_data(&material),
            None => MaterialData::DEFAULT,
        };
        self.model.materials.push(data);
        let index = self.model.materials.len() - 1;
        self.materials.insert(material.index(), index);
        index
    }

    fn material_data(&mut self, m: &gltf::Material) -> MaterialData {
        let pbr = m.pbr_metallic_roughness();
        let alpha_cutoff = match m.alpha_mode() {
            gltf::material::AlphaMode::Opaque => 0.0,
            gltf::material::AlphaMode::Mask => m.alpha_cutoff().unwrap_or(0.5).clamp(0.0, 1.0),
            gltf::material::AlphaMode::Blend => 0.0,
        };
        if m.extension_value("KHR_materials_unlit").is_some() {
            self.warn("unlit materials are drawn lit");
        }
        if m.normal_texture().is_some_and(|t| t.scale() != 1.0) {
            self.warn("a normal map's scale isn't supported; it's drawn at scale 1");
        }
        if m.occlusion_texture().is_some_and(|t| t.strength() != 1.0) {
            self.warn("an occlusion texture's strength isn't supported; it's drawn at strength 1");
        }

        // (texture, its UV set, its transform) per slot. The normal and
        // occlusion slots' own structs don't expose a transform.
        fn info(t: gltf::texture::Info<'_>) -> (gltf::Texture<'_>, u32, Option<UvTransform>) {
            let transform = t.texture_transform().map(|x| (x.scale(), x.offset(), x.rotation(), x.tex_coord()));
            (t.texture(), t.tex_coord(), transform)
        }
        let slots = [
            pbr.base_color_texture().map(info),
            m.normal_texture().map(|t| (t.texture(), t.tex_coord(), None)),
            pbr.metallic_roughness_texture().map(info),
            m.occlusion_texture().map(|t| (t.texture(), t.tex_coord(), None)),
            m.emissive_texture().map(info),
        ];
        // One UV transform per material: the first textured slot's.
        let mut transform: Option<([f32; 2], [f32; 2])> = None;
        let mut images = [None; 5];
        for (slot, entry) in slots.into_iter().enumerate() {
            let Some((texture, tex_coord, slot_transform)) = entry else { continue };
            let (scale, offset, rotation, set) = slot_transform.unwrap_or(([1.0; 2], [0.0; 2], 0.0, None));
            if set.unwrap_or(tex_coord) != 0 {
                self.warn("only the first set of texture coordinates (TEXCOORD_0) is used");
            }
            if rotation != 0.0 {
                self.warn("rotated texture transforms aren't supported; the rotation is left out");
            }
            match transform {
                None => transform = Some((scale, offset)),
                Some(first) if first != (scale, offset) => {
                    self.warn("a material's textures have different UV transforms; the first one is used for all")
                }
                Some(_) => {}
            }
            self.check_sampler(&texture.sampler());
            images[slot] = Some(texture.source().index());
        }
        let (uv_scale, uv_offset) = transform.unwrap_or(([1.0; 2], [0.0; 2]));
        let [base_color_texture, normal_texture, metallic_roughness_texture, occlusion_texture, emissive_texture] =
            images;
        // glTF gives these ranges but nothing in the format enforces them,
        // and the world refuses a material outside its own.
        let mut clamped = false;
        let mut clamp = |x: f32, low: f32, high: f32| {
            let y = x.clamp(low, high);
            clamped |= y != x;
            y
        };
        let data = MaterialData {
            name: m.name().unwrap_or_default().to_owned(),
            base_color: pbr.base_color_factor().map(|c| clamp(c, 0.0, 1.0)),
            metallic: clamp(pbr.metallic_factor(), 0.0, 1.0),
            roughness: clamp(pbr.roughness_factor(), 0.0, 1.0),
            emissive: m.emissive_factor().map(|c| clamp(c, 0.0, 1.0)),
            emissive_strength: clamp(m.emissive_strength().unwrap_or(1.0), 0.0, MAX_EMISSIVE_STRENGTH),
            alpha_cutoff,
            transparent: m.alpha_mode() == gltf::material::AlphaMode::Blend,
            double_sided: m.double_sided(),
            uv_scale: uv_scale.map(|x| clamp(x, -MAX_UV, MAX_UV)),
            uv_offset: uv_offset.map(|x| clamp(x, -MAX_UV, MAX_UV)),
            base_color_texture,
            normal_texture,
            metallic_roughness_texture,
            occlusion_texture,
            emissive_texture,
        };
        if clamped {
            self.warn("some material values are outside glTF's ranges; they're clamped");
        }
        data
    }

    fn check_sampler(&mut self, sampler: &gltf::texture::Sampler) {
        if sampler.wrap_s() != WrappingMode::Repeat || sampler.wrap_t() != WrappingMode::Repeat {
            self.warn("texture clamping and mirrored repeat aren't supported; textures repeat");
        }
        let nearest = sampler.mag_filter() == Some(MagFilter::Nearest)
            || matches!(
                sampler.min_filter(),
                Some(MinFilter::Nearest | MinFilter::NearestMipmapNearest | MinFilter::NearestMipmapLinear)
            );
        if nearest {
            self.warn("nearest-neighbour texture filtering isn't supported; textures are filtered smoothly");
        }
    }
}

/// Which accessors a primitive reads, and how: equal ones make the same
/// mesh.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Geometry {
    positions: Option<usize>,
    normals: Option<usize>,
    tangents: Option<usize>,
    uvs: Option<usize>,
    indices: Option<usize>,
    mode: u32,
}

/// `KHR_texture_transform`: scale, offset, rotation, and the UV set it
/// switches to.
type UvTransform = ([f32; 2], [f32; 2], f32, Option<u32>);

/// A triangle strip as a list: every three in a row, every other one
/// turned round so they all face the same way.
fn strip(indices: &[u32]) -> Vec<u32> {
    let mut list = Vec::with_capacity(indices.len().saturating_sub(2) * 3);
    for (i, w) in indices.windows(3).enumerate() {
        if i % 2 == 0 { list.extend([w[0], w[1], w[2]]) } else { list.extend([w[1], w[0], w[2]]) }
    }
    list
}

/// A triangle fan as a list: each pair after the first index, with it.
fn fan(indices: &[u32]) -> Vec<u32> {
    let Some((&centre, rest)) = indices.split_first() else { return Vec::new() };
    rest.windows(2).flat_map(|w| [centre, w[0], w[1]]).collect()
}

fn shorten(uri: &str) -> String {
    if uri.starts_with("data:") { "a data: URI".to_owned() } else { uri.to_owned() }
}

/// `accessor`'s values as floats. KHR_mesh_quantization lets positions,
/// normals, tangents and UVs be small integers, normalised (to -1..1 or
/// 0..1, as glTF says) or not; without it they're floats, or UVs
/// normalised u8 or u16. `None` for a type no attribute may have.
fn floats<const N: usize>(accessor: gltf::Accessor<'_>, blob: &[u8]) -> Option<Vec<[f32; N]>>
where
    [f32; N]: Item,
    [i8; N]: Item,
    [u8; N]: Item,
    [i16; N]: Item,
    [u16; N]: Item,
{
    fn each<T: Copy, const N: usize>(values: accessor::Iter<'_, [T; N]>, f: impl Fn(T) -> f32) -> Vec<[f32; N]>
    where
        [T; N]: Item,
    {
        values.map(|v| v.map(&f)).collect()
    }
    let get = |buffer: gltf::Buffer<'_>| match buffer.source() {
        gltf::buffer::Source::Bin => Some(blob),
        gltf::buffer::Source::Uri(_) => None,
    };
    let unit = accessor.normalized();
    Some(match accessor.data_type() {
        DataType::F32 => accessor::Iter::<[f32; N]>::new(accessor, get)?.collect(),
        DataType::I8 => each(accessor::Iter::<[i8; N]>::new(accessor, get)?, |c| {
            if unit { (f32::from(c) / 127.0).max(-1.0) } else { f32::from(c) }
        }),
        DataType::U8 => {
            each(
                accessor::Iter::<[u8; N]>::new(accessor, get)?,
                |c| if unit { f32::from(c) / 255.0 } else { f32::from(c) },
            )
        }
        DataType::I16 => each(accessor::Iter::<[i16; N]>::new(accessor, get)?, |c| {
            if unit { (f32::from(c) / 32767.0).max(-1.0) } else { f32::from(c) }
        }),
        DataType::U16 => each(accessor::Iter::<[u16; N]>::new(accessor, get)?, |c| {
            if unit { f32::from(c) / 65535.0 } else { f32::from(c) }
        }),
        DataType::U32 => return None,
    })
}
