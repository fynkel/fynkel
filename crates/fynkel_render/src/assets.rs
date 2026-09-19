//! Cooked assets as the renderer sees them: a cache directory written by
//! `fynkel cook` (`index.json` + one `.fyna` container per asset), read
//! through `fynkel_cooked` only. The renderer asks for a mesh, a texture or
//! a material by `AssetRef`; everything is loaded on demand and bounded.
//!
//! Nothing here knows about the importers or the document layer: a
//! material is the cooked `PbrMaterial` (`MATL` chunk, decoded by
//! `fynkel_wire::material`), of which the V1 shading uses the base
//! colour, metallic and roughness.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use fynkel_contracts::{AssetRef, Uuid};
use fynkel_cooked::payload::{decode_mesh, decode_texture, MeshPayload, TexturePayload};
use fynkel_cooked::{ChunkFile, KIND_MATERIAL, KIND_MESH, KIND_TEXTURE};
pub use fynkel_core::material::PbrMaterial;
use fynkel_wire::material::decode_material;

/// Largest payload the renderer will read (per chunk).
pub const MAX_CHUNK_BYTES: u64 = 256 << 20;
const MAX_META_BYTES: u64 = 16 << 20;

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

    /// The cooked `PbrMaterial`. A custom-shader material has no `MATL`
    /// chunk and reports as such; a chunk another record layout wrote (a
    /// stale cache) is "malformed" and `fynkel cook` fixes it.
    pub fn material(&self, r: &AssetRef) -> Result<PbrMaterial, String> {
        let bytes = self.chunk(r, KIND_MATERIAL)?;
        decode_material(&bytes)
            .ok_or_else(|| format!("{r}: malformed MATL payload (re-run `fynkel cook`)"))
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

#[cfg(test)]
mod tests {
    use super::*;

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
