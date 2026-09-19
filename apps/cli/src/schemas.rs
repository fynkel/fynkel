//! Which schema set a command validates against. Inside the repository
//! (`--root` or the workspace found from the working directory or the
//! executable) the committed `schemas/` directory; anywhere else — a game
//! project created by `fynkel create`, say — the set embedded in this build,
//! which `fynkel codegen --check` keeps identical to the committed files.
//! `--schemas <dir>` overrides both.

use std::path::{Path, PathBuf};

use fynkel_document::{Diagnostic, SchemaSet};

// As elsewhere: a `Diagnostic` is the error type on purpose.
#[allow(clippy::result_large_err)]
pub fn load(root: Option<&Path>, explicit: Option<&Path>) -> Result<SchemaSet, Diagnostic> {
    match explicit
        .map(Path::to_path_buf)
        .or_else(|| root.map(|r| r.join("schemas")))
    {
        Some(dir) => SchemaSet::load(&dir),
        None => Ok(SchemaSet::embedded()),
    }
}

/// The Fynkel repository root: `explicit`, else the Cargo workspace above
/// the working directory, else the one above this executable (a `cargo run`
/// or `cargo install --path` build keeps that relation). `None` outside the
/// repository: commands then use the embedded schemas, and `fynkel codegen`
/// asks for `--root`.
pub fn repo_root(explicit: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(p);
    }
    let from_cwd = std::env::current_dir()
        .ok()
        .and_then(|d| find_workspace(&d));
    from_cwd.or_else(|| {
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
            .and_then(|d| find_workspace(&d))
    })
}

fn find_workspace(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        let cargo = dir.join("Cargo.toml");
        if cargo.is_file()
            && std::fs::read_to_string(&cargo)
                .map(|s| s.contains("[workspace]") && s.contains("fynkel_native"))
                .unwrap_or(false)
        {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// [`load`], then the project's own behaviour schemas and registry when
/// `path` lies inside a project (so a scene's `game.*` components validate
/// against their generated schemas instead of warning `FYN-D0205`). A
/// project file that does not read is ignored here; `fynkel validate`
/// reports it.
#[allow(clippy::result_large_err)]
pub fn load_for(
    root: Option<&Path>,
    explicit: Option<&Path>,
    path: &Path,
) -> Result<SchemaSet, Diagnostic> {
    let base = load(root, explicit)?;
    let Some(dir) = fynkel_document::Project::dir_of(path) else {
        return Ok(base);
    };
    match fynkel_document::Project::read(&dir) {
        Ok(project) => project.schemas(&base, &dir),
        Err(_) => Ok(base),
    }
}
