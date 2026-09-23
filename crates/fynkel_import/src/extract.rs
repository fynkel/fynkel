//! Extract Materials (`docs/design/material-remapping.md` §5): a model's
//! embedded materials written out as editable `<name>.material.json`
//! documents — a fresh id, the imported values, the texture slots still
//! pointing at the model's images — for the caller to write and remap
//! (`Meta::set_material_remap`). The editor's `extractMaterials`
//! operation and `fynkel asset extract-materials` share it.

use std::path::{Path, PathBuf};

use fynkel_contracts::Diagnostic;
use fynkel_document::asset::Meta;
use fynkel_document::{Document, DocumentKind};
use serde_json::Value;
use uuid::Uuid;

use crate::Limits;

/// The folder a model's materials are extracted to by default:
/// `Materials/` next to the model.
pub fn default_folder(source: &Path) -> PathBuf {
    source
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("Materials")
}

/// One material to write: the embedded material's key, the new
/// document's path, id and canonical text.
#[derive(Clone, Debug, PartialEq)]
pub struct Extracted {
    pub key: String,
    pub path: PathBuf,
    pub id: Uuid,
    pub text: String,
}

/// The embedded materials of `source` that are not remapped yet, as new
/// documents under `folder`. A name already taken there (on disk, or by
/// an earlier material of this batch) gets ` 2`, ` 3`, …, so models
/// sharing a folder never overwrite each other's copies. Nothing is
/// written; an empty list means every material is remapped already. `Err`
/// when the model does not import.
pub fn extract_materials(
    source: &Path,
    meta: &Meta,
    folder: &Path,
    limits: &Limits,
) -> Result<Vec<Extracted>, Vec<Diagnostic>> {
    let imported = crate::import(source, meta, limits);
    let Some(intermediate) = imported.intermediate else {
        return Err(imported.diagnostics);
    };
    let remapped: Vec<String> = meta.material_remaps().into_iter().map(|(k, _)| k).collect();
    let shown = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut taken: Vec<PathBuf> = Vec::new();
    let mut out = Vec::new();
    for m in &intermediate.materials {
        if remapped.contains(&m.sub_id) {
            continue;
        }
        let base = file_stem_for(&m.name, &m.sub_id);
        let path = (1..)
            .map(|n| {
                let stem = if n == 1 {
                    base.clone()
                } else {
                    format!("{base} {n}")
                };
                folder.join(format!("{stem}{}", DocumentKind::Material.suffix()))
            })
            .find(|p| !p.exists() && !taken.contains(p))
            .expect("an unbounded range finds a free name");
        taken.push(path.clone());
        let id = Uuid::new_v4();
        let mut body = m.document.clone();
        body["id"] = Value::String(id.to_string());
        body["description"] = Value::String(format!("Extracted from {shown} ({}).", m.sub_id));
        let shown_path = path.display().to_string();
        let text = serde_json::to_string(&body).expect("a JSON value serialises");
        let doc = Document::parse(DocumentKind::Material, Some(&shown_path), &text)?;
        out.push(Extracted {
            key: m.sub_id.clone(),
            path,
            id,
            text: doc.write().map_err(|d| vec![d])?,
        });
    }
    Ok(out)
}

/// A file stem from a material's name: characters a file name cannot
/// carry become `_`; an empty name falls back to the key's last part.
fn file_stem_for(name: &str, key: &str) -> String {
    let clean = |s: &str| -> String {
        s.chars()
            .map(|c| {
                if c.is_control()
                    || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
                {
                    '_'
                } else {
                    c
                }
            })
            .collect::<String>()
            .trim()
            .trim_start_matches('.')
            .to_string()
    };
    let stem = clean(name);
    if !stem.is_empty() {
        return stem;
    }
    let last = clean(key.rsplit('/').next().unwrap_or(""));
    if last.is_empty() || last.starts_with('#') {
        "Material".to_string()
    } else {
        last
    }
}
