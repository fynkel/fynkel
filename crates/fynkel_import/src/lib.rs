//! Development-only asset import (blueprint §6 "Asset identity and import",
//! §19.2c items 10–12; ADR-0006).
//!
//! `source file + import settings + importer version → normalized
//! intermediate asset`. The intermediate is the one shape the rest of the
//! pipeline (cache, cooking, the renderer's loaders) reads, whatever the
//! source format was: meshes in a fixed vertex layout, materials as
//! `fynkel.pbr@1` material documents, the node hierarchy as a prefab
//! document, images as the encoded bytes they came as (decoding is the
//! texture importer's job). It serializes to bytes deterministically —
//! the same source, settings and sidecar always give the same bytes — so
//! it can be hashed, cached and diffed.
//!
//! Importers never run scripts, never follow a URI unless the sidecar's
//! settings say so, and stop at explicit [`Limits`].
//!
//! [`cook`] takes a whole asset database through the importers into the
//! content-addressed [`Cache`] as `fynkel_cooked` containers (item 12).

#![forbid(unsafe_code)]
// As in `fynkel_document`: a `Diagnostic` is the error type on purpose.
#![allow(clippy::result_large_err)]

pub mod audio;
pub mod cache;
pub mod cook;
pub mod gltf;
pub mod image;
pub mod intermediate;
pub mod tangents;

pub use cache::{Cache, CacheKey, KeyInputs, Manifest, CACHE_VERSION};
pub use cook::{
    cook, cook_project, project_cache_dir, CookOptions, CookProject, CookReport, CookedAsset,
    Status,
};
pub use intermediate::{
    AudioData, ColorSpace, ImageData, Intermediate, MaterialData, MeshData, TextureData, Vertex,
    INTERMEDIATE_VERSION,
};

use std::path::Path;

use fynkel_contracts::Diagnostic;
use fynkel_document::asset::Meta;

/// Every importer with its version — the asset compiler fingerprint
/// `fynkel.lock.json` records (`fynkel.file` has no importer; the cooker
/// copies such files, so its "version" is the cooker's).
pub const IMPORTERS: &[(&str, u32)] = &[
    (
        fynkel_document::asset::GLTF_IMPORTER,
        gltf::IMPORTER_VERSION,
    ),
    (
        fynkel_document::asset::IMAGE_IMPORTER,
        image::IMPORTER_VERSION,
    ),
    (
        fynkel_document::asset::AUDIO_IMPORTER,
        audio::IMPORTER_VERSION,
    ),
    (fynkel_document::asset::FILE_IMPORTER, 1),
];

/// The lock this build writes (`fynkel project lock`) and checks
/// (`fynkel validate`).
pub fn expected_lock() -> fynkel_document::Lock {
    fynkel_document::Lock::expected(IMPORTERS, cache::CACHE_VERSION, fynkel_cooked::VERSION)
}

/// Resource budgets every importer honours (`FYN-A0101` when exceeded).
#[derive(Clone, Debug, PartialEq)]
pub struct Limits {
    /// Source file bytes.
    pub max_file_bytes: u64,
    /// Total decoded buffer bytes (GLB `BIN` chunk, data URIs, external files).
    pub max_buffer_bytes: u64,
    pub max_nodes: usize,
    pub max_node_depth: usize,
    /// Per primitive, after unwelding.
    pub max_vertices: usize,
    pub max_images: usize,
    /// Bytes of one embedded image.
    pub max_image_bytes: u64,
    /// Width and height of a decoded texture.
    pub max_texture_dimension: u32,
    /// Decoded audio length, in frames (`sample_rate * seconds`).
    pub max_audio_frames: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_file_bytes: 256 << 20,
            max_buffer_bytes: 256 << 20,
            max_nodes: 100_000,
            max_node_depth: 64,
            max_vertices: 1 << 24,
            max_images: 256,
            max_image_bytes: 64 << 20,
            max_texture_dimension: 16_384,
            max_audio_frames: 48_000 * 60 * 10,
        }
    }
}

/// What an import produces besides the intermediate: the sub-asset ids the
/// sidecar should now carry (existing ids kept, new ones derived), and the
/// diagnostics — an error among them means `intermediate` is `None`.
#[derive(Debug)]
pub struct Imported {
    pub intermediate: Option<Intermediate>,
    /// Sub-asset id → uuid, in intermediate order; replaces `Meta::sub_assets`.
    pub sub_assets: Vec<(String, uuid::Uuid)>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Imported {
    pub fn has_errors(&self) -> bool {
        fynkel_contracts::has_errors(&self.diagnostics)
    }

    /// True when the sidecar's `subAssets` differ from what was produced.
    pub fn sidecar_changed(&self, meta: &Meta) -> bool {
        meta.sub_assets != self.sub_assets
    }
}

/// Imports `path` with its sidecar's importer. The sidecar supplies the
/// asset id (the namespace for every derived id), the settings and the
/// sub-asset ids already assigned.
pub fn import(path: &Path, meta: &Meta, limits: &Limits) -> Imported {
    match meta.importer.as_str() {
        fynkel_document::asset::GLTF_IMPORTER => gltf::import(path, meta, limits),
        fynkel_document::asset::IMAGE_IMPORTER => image::import(path, meta, limits),
        fynkel_document::asset::AUDIO_IMPORTER => audio::import(path, meta, limits),
        other => Imported {
            intermediate: None,
            sub_assets: meta.sub_assets.clone(),
            diagnostics: vec![Diagnostic::error(
                fynkel_contracts::Code::IMPORT_UNSUPPORTED,
                format!("no importer for {other}"),
            )
            .with_document(path.display().to_string())
            .with_suggestion(
                "importers: fynkel.gltf (.gltf/.glb), fynkel.image (.png/.jpg), fynkel.audio (.wav/.ogg); fynkel.file has none (`fynkel cook` carries such files as they are)",
            )],
        },
    }
}

/// The stable uuid of a sub-asset that has none yet: UUID v5 in the
/// asset's namespace, so re-importing (or importing on another machine)
/// derives the same id. Ids already in the sidecar win over this.
pub fn derive_sub_id(asset: uuid::Uuid, sub: &str) -> uuid::Uuid {
    uuid::Uuid::new_v5(&asset, sub.as_bytes())
}

/// Reads the source file within `limits.max_file_bytes`.
pub(crate) fn read_source(path: &Path, limits: &Limits) -> Result<Vec<u8>, Diagnostic> {
    let shown = path.display().to_string();
    let len = std::fs::metadata(path)
        .map_err(|e| {
            Diagnostic::error(fynkel_contracts::Code::IO, format!("cannot read: {e}"))
                .with_document(shown.clone())
        })?
        .len();
    if len > limits.max_file_bytes {
        return Err(Diagnostic::error(
            fynkel_contracts::Code::IMPORT_LIMIT,
            format!(
                "file is {len} bytes; the limit is {} bytes",
                limits.max_file_bytes
            ),
        )
        .with_document(shown));
    }
    std::fs::read(path).map_err(|e| {
        Diagnostic::error(fynkel_contracts::Code::IO, format!("cannot read: {e}"))
            .with_document(shown)
    })
}
