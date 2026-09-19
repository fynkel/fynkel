//! Cooked assets as the renderer sees them: a cache directory written by
//! `fynkel cook` (`index.json` + one `.fyna` container per asset), read
//! through `fynkel_cooked` only. The renderer asks for a mesh, a texture or
//! a material by `AssetRef`; everything is loaded on demand and bounded.
//!
//! Nothing here knows about the importers or the document layer: a
//! material is read from its cooked JSON chunk with the handful of
//! `fynkel.pbr@1` fields the V1 shading uses.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use fynkel_contracts::{AssetRef, Uuid};
use fynkel_cooked::payload::{decode_mesh, decode_texture, MeshPayload, TexturePayload};
use fynkel_cooked::{ChunkFile, KIND_JSON, KIND_MESH, KIND_TEXTURE};

/// Largest payload the renderer will read (per chunk).
pub const MAX_CHUNK_BYTES: u64 = 256 << 20;
const MAX_META_BYTES: u64 = 16 << 20;

/// What the V1 shading reads from a `fynkel.pbr@1` material.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialDesc {
    /// Linear RGBA factor; multiplies the base colour texture.
    pub base_color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    pub emissive: [f32; 3],
    /// The `baseColor` slot's texture, when bound.
    pub base_color_texture: Option<AssetRef>,
}

impl Default for MaterialDesc {
    fn default() -> Self {
        MaterialDesc {
            base_color: [1.0; 4],
            metallic: 0.0,
            roughness: 0.5,
            emissive: [0.0; 3],
            base_color_texture: None,
        }
    }
}

/// A cooked cache opened for reading.
#[derive(Debug)]
pub struct CookedAssets {
    root: PathBuf,
    profile: String,
    files: HashMap<Uuid, PathBuf>,
}

impl CookedAssets {
    /// Opens `<cache>/index.json`. Assets the cook failed on are left out;
    /// asking for them reports "not in the cache".
    pub fn open(cache_dir: &Path) -> Result<CookedAssets, String> {
        let index_path = cache_dir.join("index.json");
        let text = std::fs::read_to_string(&index_path)
            .map_err(|e| format!("{}: {e} (run `fynkel cook` first)", index_path.display()))?;
        let index: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("{}: not JSON: {e}", index_path.display()))?;
        if index["fynkel"] != "cache-index" {
            return Err(format!("{}: not a cache index", index_path.display()));
        }
        let mut files = HashMap::new();
        for a in index["assets"].as_array().into_iter().flatten() {
            let (Some(id), Some(file)) = (a["id"].as_str(), a["file"].as_str()) else {
                continue;
            };
            let Ok(id) = id.parse::<Uuid>() else { continue };
            files.insert(id, cache_dir.join(file));
        }
        Ok(CookedAssets {
            root: cache_dir.to_path_buf(),
            profile: index["profile"].as_str().unwrap_or("").to_string(),
            files,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn profile(&self) -> &str {
        &self.profile
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn contains(&self, id: Uuid) -> bool {
        self.files.contains_key(&id)
    }

    pub fn mesh(&self, r: &AssetRef) -> Result<MeshPayload, String> {
        let bytes = self.chunk(r, KIND_MESH)?;
        decode_mesh(&bytes).ok_or_else(|| format!("{r}: malformed MESH payload"))
    }

    pub fn texture(&self, r: &AssetRef) -> Result<TexturePayload, String> {
        let bytes = self.chunk(r, KIND_TEXTURE)?;
        decode_texture(&bytes).ok_or_else(|| format!("{r}: malformed TEXR payload"))
    }

    pub fn material(&self, r: &AssetRef) -> Result<MaterialDesc, String> {
        let bytes = self.chunk(r, KIND_JSON)?;
        let doc: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|e| format!("{r}: material is not JSON: {e}"))?;
        Ok(material_from_json(&doc))
    }

    /// The payload of the chunk of `kind` whose `sub` is the reference's.
    fn chunk(&self, r: &AssetRef, kind: [u8; 4]) -> Result<Vec<u8>, String> {
        let path = self
            .files
            .get(&r.asset)
            .ok_or_else(|| format!("{r}: not in the cache"))?;
        let mut file =
            ChunkFile::open_path(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let meta = file
            .meta(MAX_META_BYTES)
            .map_err(|e| format!("{}: META: {e}", path.display()))?;
        let meta: serde_json::Value =
            serde_json::from_slice(&meta).map_err(|e| format!("{}: META: {e}", path.display()))?;
        let want = String::from_utf8_lossy(&kind).to_string();
        let index = meta["chunks"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|c| c["kind"] == want.as_str() && c["sub"].as_str().unwrap_or("") == r.sub)
            .and_then(|c| c["index"].as_u64())
            .ok_or_else(|| format!("{r}: no {want} chunk in {}", path.display()))?;
        file.read(index as usize, MAX_CHUNK_BYTES)
            .map_err(|e| format!("{r}: chunk {index}: {e}"))
    }
}

/// The V1 fields of a `fynkel.pbr@1` material document; anything missing
/// takes the manifest default.
pub fn material_from_json(doc: &serde_json::Value) -> MaterialDesc {
    let mut m = MaterialDesc::default();
    let p = &doc["parameters"];
    if let Some(c) = floats(&p["baseColor"]) {
        if c.len() == 4 {
            m.base_color = [c[0], c[1], c[2], c[3]];
        }
    }
    if let Some(v) = p["metallic"].as_f64() {
        m.metallic = v as f32;
    }
    if let Some(v) = p["roughness"].as_f64() {
        m.roughness = v as f32;
    }
    if let Some(c) = floats(&p["emissive"]) {
        if c.len() == 3 {
            m.emissive = [c[0], c[1], c[2]];
        }
    }
    let t = &doc["textures"]["baseColor"]["texture"];
    if let Some(asset) = t["asset"].as_str().and_then(|s| s.parse::<Uuid>().ok()) {
        m.base_color_texture = Some(AssetRef {
            asset,
            sub: t["sub"].as_str().unwrap_or("").to_string(),
        });
    }
    m
}

fn floats(v: &serde_json::Value) -> Option<Vec<f32>> {
    v.as_array()?
        .iter()
        .map(|x| x.as_f64().map(|f| f as f32))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_fields_and_defaults() {
        let m = material_from_json(&serde_json::json!({
            "parameters": { "baseColor": [0.8, 0.5, 0.3, 1.0], "metallic": 1.0 },
            "textures": { "baseColor": { "texture": { "asset": "c0000000-0000-4000-8000-000000000002" } } }
        }));
        assert_eq!(m.base_color, [0.8, 0.5, 0.3, 1.0]);
        assert_eq!((m.metallic, m.roughness), (1.0, 0.5));
        let t = m.base_color_texture.unwrap();
        assert_eq!(t.asset.0[0], 0xc0);
        assert_eq!(t.sub, "");
        assert_eq!(
            material_from_json(&serde_json::json!({})),
            MaterialDesc::default()
        );
    }

    #[test]
    fn missing_index_says_to_cook() {
        let dir =
            std::env::temp_dir().join(format!("fynkel-render-nocache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let err = CookedAssets::open(&dir).unwrap_err();
        assert!(err.contains("fynkel cook"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
