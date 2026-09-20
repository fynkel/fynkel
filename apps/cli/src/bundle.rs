//! The bundled cache (blueprint §13.6 "compiled/cooked game assets"): a
//! game that runs anywhere but the developer's Mac — an iPhone, a shipped
//! build — cannot open `<project>/.fynkel/cache` by path, so the app
//! carries the cooked cache as Flutter assets. `stage` mirrors the cache
//! into `<project>/fynkel_cache/` — `index.json` and the objects flat
//! beside it (a Flutter asset directory is not recursive, and an empty
//! `objects/` could not be kept in version control); the per-object
//! manifests are the cook's business, the runtime never reads them — which
//! the created app's `pubspec.yaml` lists under `assets:`.
//! At start-up `package:fynkel_flutter`'s `unpackBundledCache` writes the
//! bundle to a writable directory and `Game.load` opens that when the
//! project directory is not on this machine. `fynkel cook` and `fynkel
//! run` stage after cooking; a project without a `pubspec.yaml` (the
//! fixture, the stress scene) has no app to stage for.

use std::path::{Path, PathBuf};

/// Where a project's bundled cache is staged, relative to the project.
pub const STAGE_DIR: &str = "fynkel_cache";

/// The file that keeps the directory in version control (Flutter needs the
/// asset directory to exist to build at all; the objects are ignored).
pub const KEEP_FILE: &str = "README.md";

pub const KEEP_TEXT: &str = "# fynkel_cache\n\nThe cooked cache `fynkel cook` stages here for the app to bundle as Flutter\nassets (`pubspec.yaml` → `assets:`), so a build runs on a device that cannot\nsee this directory. Generated; everything but this file is git-ignored.\n";

pub struct Staged {
    pub dir: PathBuf,
    pub copied: usize,
    pub removed: usize,
    pub objects: usize,
}

/// Whether `project` is a Flutter app (something to stage for).
pub fn is_app(project: &Path) -> bool {
    project.join("pubspec.yaml").is_file()
}

/// Mirrors `cache` into `<project>/fynkel_cache/`: `index.json` rewritten,
/// the objects it names copied when missing (they are content-addressed,
/// an existing one is current — objects of other profiles or of earlier
/// cooks stay out of the bundle), objects no longer named removed.
pub fn stage(project: &Path, cache: &Path) -> std::io::Result<Staged> {
    let dir = project.join(STAGE_DIR);
    std::fs::create_dir_all(&dir)?;
    let keep = dir.join(KEEP_FILE);
    if !keep.is_file() {
        std::fs::write(&keep, KEEP_TEXT)?;
    }
    let index = std::fs::read(cache.join("index.json"))?;
    let parsed: serde_json::Value = serde_json::from_slice(&index)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(dir.join("index.json"), &index)?;
    let mut wanted = std::collections::HashSet::new();
    let mut copied = 0;
    for asset in parsed["assets"].as_array().into_iter().flatten() {
        let Some(file) = asset["file"].as_str() else {
            continue;
        };
        let Some(name) = Path::new(file).file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.ends_with(".fyna") || !wanted.insert(name.to_string()) {
            continue;
        }
        let to = dir.join(name);
        if !to.is_file() {
            std::fs::copy(cache.join(file), &to)?;
            copied += 1;
        }
    }
    let mut removed = 0;
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.ends_with(".fyna") && !wanted.contains(name) {
            std::fs::remove_file(entry.path())?;
            removed += 1;
        }
    }
    Ok(Staged {
        dir,
        copied,
        removed,
        objects: wanted.len(),
    })
}
