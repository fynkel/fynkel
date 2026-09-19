//! The normalized intermediate asset and its byte form.
//!
//! Layout (all little-endian):
//!
//! ```text
//! "FYNI"  u32 version  u32 header_len  header (JSON, header_len bytes)  blobs…
//! ```
//!
//! The header is a JSON object listing meshes, materials, images,
//! textures, audio clips, the generated prefab and the sub-asset ids, with
//! every blob given as `[offset, length]` into the blob section (offsets
//! count from the end of the header). Vertices are [`Vertex::STRIDE`]
//! bytes each, indices `u32`, texels RGBA8, samples `f32` interleaved.
//! Serialization is deterministic: field order is fixed, there are no
//! maps with unstable order, floats are written as IEEE bits.

use fynkel_contracts::Diagnostic;
use serde::{Deserialize, Serialize};

pub const MAGIC: &[u8; 4] = b"FYNI";
pub const INTERMEDIATE_VERSION: u32 = 2;

/// The one vertex layout every mesh is normalized to (72 bytes); the
/// cooked `MESH` payload uses the same one, so it is defined next to the
/// container (`fynkel_cooked::payload::Vertex`).
pub use fynkel_cooked::payload::Vertex;

/// One drawable: a triangle list with one material.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshData {
    /// Sub-asset id (`mesh/<n>` or `mesh/<n>/<p>`).
    pub sub_id: String,
    pub name: String,
    /// Sub-asset id of the material (`material/<m>`), `None` for the default.
    pub material: Option<String>,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub bounds_min: [f32; 3],
    pub bounds_max: [f32; 3],
}

impl MeshData {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}

/// A material mapped onto `fynkel.pbr@1`, as a `.material.json` body.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialData {
    /// Sub-asset id (`material/<m>`).
    pub sub_id: String,
    pub name: String,
    pub document: serde_json::Value,
}

/// An image as it came (PNG/JPEG bytes); decoding is the texture importer's.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageData {
    /// Sub-asset id (`image/<i>`, or `image/<i>/linear` for the data copy
    /// of an image that is also used as a colour).
    pub sub_id: String,
    pub name: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
    /// Decided by the importer from the material slots that bind it.
    pub color_space: ColorSpace,
}

/// How the texels of a texture are to be read (§9; the material schema's
/// `colorSpace`). Colour textures are sRGB, data textures (normal,
/// roughness, masks) are linear — the importer cannot tell, so the sidecar
/// says (`settings.colorSpace`, default `srgb`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorSpace {
    Srgb,
    Linear,
}

impl ColorSpace {
    pub fn parse(s: &str) -> Option<ColorSpace> {
        match s {
            "srgb" => Some(ColorSpace::Srgb),
            "linear" => Some(ColorSpace::Linear),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ColorSpace::Srgb => "srgb",
            ColorSpace::Linear => "linear",
        }
    }
}

/// A decoded texture: RGBA8, row-major, top row first, no mipmaps (M3
/// generates them at load or cook time).
#[derive(Clone, Debug, PartialEq)]
pub struct TextureData {
    /// Sub-asset id, `None` when the asset itself is the texture (a `.png`).
    pub sub_id: Option<String>,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub color_space: ColorSpace,
    /// Whether the source carried an alpha channel (the RGBA8 always has one).
    pub source_has_alpha: bool,
    /// `width * height * 4` bytes.
    pub rgba8: Vec<u8>,
}

/// A decoded audio clip: interleaved `f32` PCM in `[-1, 1]`.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioData {
    /// Sub-asset id, `None` when the asset itself is the clip (a `.wav`).
    pub sub_id: Option<String>,
    pub name: String,
    /// What the source was encoded as (`pcm`, `vorbis`).
    pub codec: String,
    pub sample_rate: u32,
    pub channels: u16,
    /// `frames * channels` samples.
    pub samples: Vec<f32>,
}

impl AudioData {
    pub fn frames(&self) -> u64 {
        if self.channels == 0 {
            0
        } else {
            (self.samples.len() / self.channels as usize) as u64
        }
    }

    pub fn duration_seconds(&self) -> f64 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.frames() as f64 / self.sample_rate as f64
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Intermediate {
    pub importer: String,
    pub importer_version: u32,
    /// The source file name (not its path).
    pub source: String,
    pub meshes: Vec<MeshData>,
    pub materials: Vec<MaterialData>,
    pub images: Vec<ImageData>,
    pub textures: Vec<TextureData>,
    pub clips: Vec<AudioData>,
    /// The node hierarchy as a `.prefab.json` body (sub-asset `prefab`),
    /// when the source has one.
    pub prefab: Option<serde_json::Value>,
    /// Every sub-asset in this intermediate with its uuid, in order.
    pub sub_assets: Vec<(String, uuid::Uuid)>,
}

impl Intermediate {
    /// An intermediate with nothing in it yet.
    pub fn empty(importer: &str, importer_version: u32, source: &str) -> Intermediate {
        Intermediate {
            importer: importer.to_string(),
            importer_version,
            source: source.to_string(),
            meshes: Vec::new(),
            materials: Vec::new(),
            images: Vec::new(),
            textures: Vec::new(),
            clips: Vec::new(),
            prefab: None,
            sub_assets: Vec::new(),
        }
    }

    pub fn prefab_entity_count(&self) -> usize {
        self.prefab
            .as_ref()
            .and_then(|p| p.get("entities"))
            .and_then(|e| e.as_object())
            .map(|o| o.len())
            .unwrap_or(0)
    }
}

#[derive(Serialize, Deserialize)]
struct Header {
    importer: String,
    #[serde(rename = "importerVersion")]
    importer_version: u32,
    source: String,
    meshes: Vec<MeshHeader>,
    materials: Vec<MaterialHeader>,
    images: Vec<ImageHeader>,
    textures: Vec<TextureHeader>,
    clips: Vec<AudioHeader>,
    prefab: Option<serde_json::Value>,
    #[serde(rename = "subAssets")]
    sub_assets: Vec<(String, String)>,
}

#[derive(Serialize, Deserialize)]
struct TextureHeader {
    #[serde(rename = "subId")]
    sub_id: Option<String>,
    name: String,
    width: u32,
    height: u32,
    #[serde(rename = "colorSpace")]
    color_space: ColorSpace,
    #[serde(rename = "sourceHasAlpha")]
    source_has_alpha: bool,
    rgba8: [u64; 2],
}

#[derive(Serialize, Deserialize)]
struct AudioHeader {
    #[serde(rename = "subId")]
    sub_id: Option<String>,
    name: String,
    codec: String,
    #[serde(rename = "sampleRate")]
    sample_rate: u32,
    channels: u16,
    frames: u64,
    samples: [u64; 2],
}

#[derive(Serialize, Deserialize)]
struct MeshHeader {
    #[serde(rename = "subId")]
    sub_id: String,
    name: String,
    material: Option<String>,
    #[serde(rename = "vertexCount")]
    vertex_count: u64,
    #[serde(rename = "indexCount")]
    index_count: u64,
    vertices: [u64; 2],
    indices: [u64; 2],
    #[serde(rename = "boundsMin")]
    bounds_min: [f32; 3],
    #[serde(rename = "boundsMax")]
    bounds_max: [f32; 3],
}

#[derive(Serialize, Deserialize)]
struct MaterialHeader {
    #[serde(rename = "subId")]
    sub_id: String,
    name: String,
    document: serde_json::Value,
}

#[derive(Serialize, Deserialize)]
struct ImageHeader {
    #[serde(rename = "subId")]
    sub_id: String,
    name: String,
    #[serde(rename = "mimeType")]
    mime_type: String,
    bytes: [u64; 2],
    #[serde(rename = "colorSpace")]
    color_space: ColorSpace,
}

impl Intermediate {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut blobs: Vec<u8> = Vec::new();
        let mut push = |data: &[u8]| -> [u64; 2] {
            let off = blobs.len() as u64;
            blobs.extend_from_slice(data);
            // Keep every blob 4-byte aligned so vertex/index data maps directly.
            while !blobs.len().is_multiple_of(4) {
                blobs.push(0);
            }
            [off, data.len() as u64]
        };
        let meshes = self
            .meshes
            .iter()
            .map(|m| {
                let mut v = Vec::with_capacity(m.vertices.len() * Vertex::STRIDE);
                for vertex in &m.vertices {
                    vertex.write(&mut v);
                }
                let mut i = Vec::with_capacity(m.indices.len() * 4);
                for index in &m.indices {
                    i.extend_from_slice(&index.to_le_bytes());
                }
                MeshHeader {
                    sub_id: m.sub_id.clone(),
                    name: m.name.clone(),
                    material: m.material.clone(),
                    vertex_count: m.vertices.len() as u64,
                    index_count: m.indices.len() as u64,
                    vertices: push(&v),
                    indices: push(&i),
                    bounds_min: m.bounds_min,
                    bounds_max: m.bounds_max,
                }
            })
            .collect();
        let images = self
            .images
            .iter()
            .map(|im| ImageHeader {
                sub_id: im.sub_id.clone(),
                name: im.name.clone(),
                mime_type: im.mime_type.clone(),
                bytes: push(&im.bytes),
                color_space: im.color_space,
            })
            .collect();
        let textures = self
            .textures
            .iter()
            .map(|t| TextureHeader {
                sub_id: t.sub_id.clone(),
                name: t.name.clone(),
                width: t.width,
                height: t.height,
                color_space: t.color_space,
                source_has_alpha: t.source_has_alpha,
                rgba8: push(&t.rgba8),
            })
            .collect();
        let clips = self
            .clips
            .iter()
            .map(|c| {
                let mut bytes = Vec::with_capacity(c.samples.len() * 4);
                for s in &c.samples {
                    bytes.extend_from_slice(&s.to_le_bytes());
                }
                AudioHeader {
                    sub_id: c.sub_id.clone(),
                    name: c.name.clone(),
                    codec: c.codec.clone(),
                    sample_rate: c.sample_rate,
                    channels: c.channels,
                    frames: c.frames(),
                    samples: push(&bytes),
                }
            })
            .collect();
        let header = Header {
            importer: self.importer.clone(),
            importer_version: self.importer_version,
            source: self.source.clone(),
            meshes,
            materials: self
                .materials
                .iter()
                .map(|m| MaterialHeader {
                    sub_id: m.sub_id.clone(),
                    name: m.name.clone(),
                    document: m.document.clone(),
                })
                .collect(),
            images,
            textures,
            clips,
            prefab: self.prefab.clone(),
            sub_assets: self
                .sub_assets
                .iter()
                .map(|(k, v)| (k.clone(), v.to_string()))
                .collect(),
        };
        let header = serde_json::to_vec(&header).expect("header serializes");
        let mut out = Vec::with_capacity(12 + header.len() + blobs.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&INTERMEDIATE_VERSION.to_le_bytes());
        out.extend_from_slice(&(header.len() as u32).to_le_bytes());
        out.extend_from_slice(&header);
        out.extend_from_slice(&blobs);
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Intermediate, Diagnostic> {
        let bad = |msg: &str| {
            Diagnostic::error(
                fynkel_contracts::Code::IMPORT_UNREADABLE,
                format!("not a Fynkel intermediate: {msg}"),
            )
        };
        if bytes.len() < 12 || &bytes[0..4] != MAGIC {
            return Err(bad("missing FYNI magic"));
        }
        let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        if version != INTERMEDIATE_VERSION {
            return Err(bad(&format!(
                "version {version}, this build reads {INTERMEDIATE_VERSION}"
            )));
        }
        let header_len = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        let header_end = 12usize
            .checked_add(header_len)
            .filter(|e| *e <= bytes.len())
            .ok_or_else(|| bad("header runs past the end"))?;
        let header: Header = serde_json::from_slice(&bytes[12..header_end])
            .map_err(|e| bad(&format!("header: {e}")))?;
        let blobs = &bytes[header_end..];
        let slice = |range: [u64; 2], what: &str| -> Result<&[u8], Diagnostic> {
            let start = range[0] as usize;
            let end = start
                .checked_add(range[1] as usize)
                .filter(|e| *e <= blobs.len())
                .ok_or_else(|| bad(&format!("{what} runs past the end")))?;
            Ok(&blobs[start..end])
        };
        let mut meshes = Vec::with_capacity(header.meshes.len());
        for m in header.meshes {
            let v = slice(m.vertices, &m.sub_id)?;
            if v.len() != m.vertex_count as usize * Vertex::STRIDE {
                return Err(bad(&format!("{}: vertex blob size", m.sub_id)));
            }
            let vertices = v
                .chunks_exact(Vertex::STRIDE)
                .map(|c| Vertex::read(c).unwrap())
                .collect();
            let i = slice(m.indices, &m.sub_id)?;
            if i.len() != m.index_count as usize * 4 {
                return Err(bad(&format!("{}: index blob size", m.sub_id)));
            }
            let indices = i
                .chunks_exact(4)
                .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
                .collect();
            meshes.push(MeshData {
                sub_id: m.sub_id,
                name: m.name,
                material: m.material,
                vertices,
                indices,
                bounds_min: m.bounds_min,
                bounds_max: m.bounds_max,
            });
        }
        let mut images = Vec::with_capacity(header.images.len());
        for im in header.images {
            let data = slice(im.bytes, &im.sub_id)?.to_vec();
            images.push(ImageData {
                sub_id: im.sub_id,
                name: im.name,
                mime_type: im.mime_type,
                bytes: data,
                color_space: im.color_space,
            });
        }
        let mut textures = Vec::with_capacity(header.textures.len());
        for t in header.textures {
            let label = t.sub_id.clone().unwrap_or_else(|| t.name.clone());
            let data = slice(t.rgba8, &label)?;
            if data.len() as u64 != t.width as u64 * t.height as u64 * 4 {
                return Err(bad(&format!("{label}: texel blob size")));
            }
            textures.push(TextureData {
                sub_id: t.sub_id,
                name: t.name,
                width: t.width,
                height: t.height,
                color_space: t.color_space,
                source_has_alpha: t.source_has_alpha,
                rgba8: data.to_vec(),
            });
        }
        let mut clips = Vec::with_capacity(header.clips.len());
        for c in header.clips {
            let label = c.sub_id.clone().unwrap_or_else(|| c.name.clone());
            let data = slice(c.samples, &label)?;
            if data.len() as u64 != c.frames * c.channels as u64 * 4 {
                return Err(bad(&format!("{label}: sample blob size")));
            }
            clips.push(AudioData {
                sub_id: c.sub_id,
                name: c.name,
                codec: c.codec,
                sample_rate: c.sample_rate,
                channels: c.channels,
                samples: data
                    .chunks_exact(4)
                    .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                    .collect(),
            });
        }
        let mut sub_assets = Vec::with_capacity(header.sub_assets.len());
        for (k, v) in header.sub_assets {
            let id: uuid::Uuid = v
                .parse()
                .map_err(|_| bad(&format!("sub-asset {k}: bad uuid")))?;
            sub_assets.push((k, id));
        }
        Ok(Intermediate {
            importer: header.importer,
            importer_version: header.importer_version,
            source: header.source,
            meshes,
            materials: header
                .materials
                .into_iter()
                .map(|m| MaterialData {
                    sub_id: m.sub_id,
                    name: m.name,
                    document: m.document,
                })
                .collect(),
            images,
            textures,
            clips,
            prefab: header.prefab,
            sub_assets,
        })
    }

    /// `fynkel import --json` summary (no vertex data).
    pub fn summary_json(&self) -> serde_json::Value {
        serde_json::json!({
            "importer": self.importer,
            "importerVersion": self.importer_version,
            "source": self.source,
            "meshes": self.meshes.iter().map(|m| serde_json::json!({
                "subId": m.sub_id, "name": m.name, "material": m.material,
                "vertices": m.vertices.len(), "triangles": m.triangle_count(),
                "boundsMin": m.bounds_min, "boundsMax": m.bounds_max,
            })).collect::<Vec<_>>(),
            "materials": self.materials.iter().map(|m| serde_json::json!({
                "subId": m.sub_id, "name": m.name,
            })).collect::<Vec<_>>(),
            "images": self.images.iter().map(|im| serde_json::json!({
                "subId": im.sub_id, "name": im.name, "mimeType": im.mime_type, "bytes": im.bytes.len(),
            })).collect::<Vec<_>>(),
            "textures": self.textures.iter().map(|t| serde_json::json!({
                "subId": t.sub_id, "name": t.name, "width": t.width, "height": t.height,
                "colorSpace": t.color_space.name(), "sourceHasAlpha": t.source_has_alpha,
            })).collect::<Vec<_>>(),
            "clips": self.clips.iter().map(|c| serde_json::json!({
                "subId": c.sub_id, "name": c.name, "codec": c.codec, "sampleRate": c.sample_rate,
                "channels": c.channels, "frames": c.frames(), "seconds": c.duration_seconds(),
            })).collect::<Vec<_>>(),
            "prefab": self.prefab.is_some(),
            "prefabEntities": self.prefab_entity_count(),
            "subAssets": self.sub_assets.iter().map(|(k, v)| serde_json::json!({ "id": k, "uuid": v.to_string() })).collect::<Vec<_>>(),
        })
    }

    pub fn summary_text(&self) -> String {
        let mut parts = Vec::new();
        if !self.meshes.is_empty() {
            parts.push(format!("{} mesh(es)", self.meshes.len()));
        }
        if !self.materials.is_empty() {
            parts.push(format!("{} material(s)", self.materials.len()));
        }
        if !self.images.is_empty() {
            parts.push(format!("{} image(s)", self.images.len()));
        }
        if !self.textures.is_empty() {
            parts.push(format!("{} texture(s)", self.textures.len()));
        }
        if !self.clips.is_empty() {
            parts.push(format!("{} clip(s)", self.clips.len()));
        }
        if self.prefab.is_some() {
            parts.push(format!(
                "prefab with {} entities",
                self.prefab_entity_count()
            ));
        }
        let mut out = format!(
            "{} ({} v{}): {}\n",
            self.source,
            self.importer,
            self.importer_version,
            if parts.is_empty() {
                "nothing".to_string()
            } else {
                parts.join(", ")
            }
        );
        for m in &self.meshes {
            out.push_str(&format!(
                "  {:<14} {:<20} {:>7} vertices {:>7} triangles  material {}\n",
                m.sub_id,
                m.name,
                m.vertices.len(),
                m.triangle_count(),
                m.material.as_deref().unwrap_or("(default)")
            ));
        }
        for m in &self.materials {
            out.push_str(&format!("  {:<14} {}\n", m.sub_id, m.name));
        }
        for im in &self.images {
            out.push_str(&format!(
                "  {:<14} {} ({}, {} bytes)\n",
                im.sub_id,
                im.name,
                im.mime_type,
                im.bytes.len()
            ));
        }
        for t in &self.textures {
            out.push_str(&format!(
                "  {:<14} {} ({}x{} RGBA8, {}{})\n",
                t.sub_id.as_deref().unwrap_or("(self)"),
                t.name,
                t.width,
                t.height,
                t.color_space.name(),
                if t.source_has_alpha { ", alpha" } else { "" }
            ));
        }
        for c in &self.clips {
            out.push_str(&format!(
                "  {:<14} {} ({}, {} Hz, {} channel(s), {} frames, {:.3} s)\n",
                c.sub_id.as_deref().unwrap_or("(self)"),
                c.name,
                c.codec,
                c.sample_rate,
                c.channels,
                c.frames(),
                c.duration_seconds()
            ));
        }
        out
    }
}
