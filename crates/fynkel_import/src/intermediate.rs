//! The normalized intermediate asset and its byte form.
//!
//! Layout (all little-endian):
//!
//! ```text
//! "FYNI"  u32 version  u32 header_len  header (JSON, header_len bytes)  blobs…
//! ```
//!
//! The header is a JSON object listing meshes, materials, images, the
//! generated prefab and the sub-asset ids, with every blob given as
//! `[offset, length]` into the blob section (offsets count from the end of
//! the header). Vertices are [`Vertex::STRIDE`] bytes each, indices `u32`.
//! Serialization is deterministic: field order is fixed, there are no
//! maps with unstable order, floats are written as IEEE bits.

use fynkel_contracts::Diagnostic;
use serde::{Deserialize, Serialize};

pub const MAGIC: &[u8; 4] = b"FYNI";
pub const INTERMEDIATE_VERSION: u32 = 1;

/// The one vertex layout every mesh is normalized to (72 bytes).
/// Missing attributes take defaults: flat normals, zero tangents (tangent
/// generation is M3's), `[0, 0]` uvs, white colour.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub tangent: [f32; 4],
    pub uv0: [f32; 2],
    pub uv1: [f32; 2],
    pub color: [f32; 4],
}

impl Default for Vertex {
    fn default() -> Self {
        Vertex {
            position: [0.0; 3],
            normal: [0.0, 0.0, 1.0],
            tangent: [0.0; 4],
            uv0: [0.0; 2],
            uv1: [0.0; 2],
            color: [1.0; 4],
        }
    }
}

impl Vertex {
    pub const STRIDE: usize = 72;

    pub fn write(&self, out: &mut Vec<u8>) {
        for f in self
            .position
            .iter()
            .chain(&self.normal)
            .chain(&self.tangent)
            .chain(&self.uv0)
            .chain(&self.uv1)
            .chain(&self.color)
        {
            out.extend_from_slice(&f.to_le_bytes());
        }
    }

    pub fn read(bytes: &[u8]) -> Option<Vertex> {
        if bytes.len() < Self::STRIDE {
            return None;
        }
        let mut floats = [0f32; 18];
        for (i, f) in floats.iter_mut().enumerate() {
            *f = f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
        }
        Some(Vertex {
            position: [floats[0], floats[1], floats[2]],
            normal: [floats[3], floats[4], floats[5]],
            tangent: [floats[6], floats[7], floats[8], floats[9]],
            uv0: [floats[10], floats[11]],
            uv1: [floats[12], floats[13]],
            color: [floats[14], floats[15], floats[16], floats[17]],
        })
    }
}

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
    /// Sub-asset id (`image/<i>`).
    pub sub_id: String,
    pub name: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
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
    /// The node hierarchy as a `.prefab.json` body (sub-asset `prefab`).
    pub prefab: serde_json::Value,
    /// Every sub-asset in this intermediate with its uuid, in order.
    pub sub_assets: Vec<(String, uuid::Uuid)>,
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
    prefab: serde_json::Value,
    #[serde(rename = "subAssets")]
    sub_assets: Vec<(String, String)>,
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
            "prefabEntities": self.prefab.get("entities").and_then(|e| e.as_object()).map(|o| o.len()).unwrap_or(0),
            "subAssets": self.sub_assets.iter().map(|(k, v)| serde_json::json!({ "id": k, "uuid": v.to_string() })).collect::<Vec<_>>(),
        })
    }

    pub fn summary_text(&self) -> String {
        let mut out = format!(
            "{} ({} v{}): {} mesh(es), {} material(s), {} image(s), prefab with {} entities\n",
            self.source,
            self.importer,
            self.importer_version,
            self.meshes.len(),
            self.materials.len(),
            self.images.len(),
            self.prefab
                .get("entities")
                .and_then(|e| e.as_object())
                .map(|o| o.len())
                .unwrap_or(0)
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
        out
    }
}
