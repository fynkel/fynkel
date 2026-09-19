//! `fynkel_world_load_scene` / `fynkel_world_load_project` /
//! `fynkel_world_load_report`: documents into the world through
//! `fynkel_document::runtime`, with a JSON report the host can show.

use std::path::Path;

use fynkel_contracts::{Diagnostic, Status};
use fynkel_core::{LoadError, LoadedScene, SceneDescription};
use fynkel_document::{diagnostic_json, load_project, read_scene, AuthoredBehaviour};
use serde_json::{json, Value};

use crate::{guarded, FynkelBufferView, FynkelWorld};

/// What one load call produced.
struct Outcome {
    status: Status,
    report: Value,
}

fn utf8<'a>(ptr: *const u8, len: u32) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: the caller promises `len` readable bytes at `ptr`.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    std::str::from_utf8(bytes).ok()
}

fn failed(diagnostics: &[Diagnostic], project: Value) -> Outcome {
    Outcome {
        status: Status::LoadFailed,
        report: json!({
            "ok": false,
            "scene": Value::Null,
            "project": project,
            "diagnostics": diagnostics.iter().map(diagnostic_json).collect::<Vec<_>>(),
        }),
    }
}

fn load_error_diagnostic(e: &LoadError) -> Diagnostic {
    use fynkel_contracts::Code;
    let code = match e {
        // Not document codes: the world's state, not the file's.
        LoadError::Poisoned | LoadError::Spawn(_) => Code::IO,
        LoadError::DuplicateEntity(_) => Code::SCENE_MALFORMED_ENTRY,
        LoadError::UnknownParent { .. } => Code::SCENE_DANGLING_PARENT,
        LoadError::UnknownChild { .. } => Code::SCENE_UNKNOWN_CHILD,
        LoadError::ChildParentMismatch { .. } => Code::SCENE_CHILD_PARENT_MISMATCH,
        LoadError::Unreachable(_) => Code::SCENE_HIERARCHY_CYCLE,
        LoadError::DuplicateComponent { .. } => Code::SCENE_DUPLICATE_COMPONENT,
    };
    Diagnostic::error(code, e.to_string())
}

/// Spawns `scene` and builds the report; `warnings` are the document
/// diagnostics of a successful read, `behaviours` the authored Dart
/// behaviours the SDK attaches to the spawned entities.
fn instantiate(
    w: &mut FynkelWorld,
    scene: &SceneDescription,
    behaviours: &[AuthoredBehaviour],
    path: &Path,
    warnings: Vec<Diagnostic>,
    project: Value,
) -> Outcome {
    match w.core_mut().load_scene(scene) {
        Ok(loaded) => Outcome {
            status: Status::Ok,
            report: json!({
                "ok": true,
                "scene": scene_json(&loaded, behaviours, path),
                "project": project,
                "diagnostics": warnings.iter().map(diagnostic_json).collect::<Vec<_>>(),
            }),
        },
        Err(e) => {
            let mut diags = warnings;
            diags.push(load_error_diagnostic(&e));
            failed(&diags, project)
        }
    }
}

fn scene_json(loaded: &LoadedScene, behaviours: &[AuthoredBehaviour], path: &Path) -> Value {
    // `entity` is the spawned handle (what `World.attach` takes); an
    // authored behaviour whose entity was not spawned cannot happen after
    // a successful load, but is dropped rather than reported with a
    // dangling handle.
    let behaviours: Vec<Value> = behaviours
        .iter()
        .filter_map(|b| {
            let entity = loaded.handle_of(b.entity)?;
            Some(json!({
                "entity": entity.to_bits(),
                "authoringId": b.entity.to_string(),
                "component": b.component.to_string(),
                "type": b.type_id,
                "version": b.version,
                "known": b.known,
                "data": b.data,
                "pointer": b.pointer,
            }))
        })
        .collect();
    json!({
        "id": loaded.id.to_string(),
        "name": loaded.name,
        "path": path.display().to_string(),
        "entities": loaded.entities().len(),
        "rejectedComponents": loaded.rejected_components,
        "behaviours": behaviours,
    })
}

fn finish(w: &mut FynkelWorld, out: Outcome) -> Status {
    w.load_report = serde_json::to_vec(&out.report).unwrap_or_default();
    w.stats.bytes_out += w.load_report.len() as u64;
    w.rebuild_diagnostics();
    out.status
}

/// # Safety
/// `world` live or NULL; `path_utf8` points at `path_len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_load_scene(
    world: *mut FynkelWorld,
    path_utf8: *const u8,
    path_len: u32,
) -> Status {
    let Some(path) = utf8(path_utf8, path_len) else {
        return Status::InvalidArgument;
    };
    let path = Path::new(path).to_path_buf();
    guarded(world, move |w| {
        let prefabs = path.parent().map(Path::to_path_buf);
        let out = match read_scene(&path, prefabs.as_deref()) {
            Ok(d) => instantiate(
                w,
                &d.description,
                &d.behaviours,
                &path,
                d.diagnostics,
                Value::Null,
            ),
            Err(diags) => failed(&diags, Value::Null),
        };
        finish(w, out)
    })
}

/// # Safety
/// `world` live or NULL; `dir_utf8` points at `dir_len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_load_project(
    world: *mut FynkelWorld,
    dir_utf8: *const u8,
    dir_len: u32,
) -> Status {
    let Some(dir) = utf8(dir_utf8, dir_len) else {
        return Status::InvalidArgument;
    };
    let dir = Path::new(dir).to_path_buf();
    guarded(world, move |w| {
        let out = match load_project(&dir) {
            Ok(p) => {
                let project = json!({
                    "name": p.project_name,
                    "dir": p.dir.display().to_string(),
                    "assets": p.assets.display().to_string(),
                    "cache": p.cache.display().to_string(),
                });
                instantiate(
                    w,
                    &p.scene,
                    &p.behaviours,
                    &p.scene_path,
                    p.warnings,
                    project,
                )
            }
            Err(diags) => failed(&diags, Value::Null),
        };
        finish(w, out)
    })
}

/// # Safety
/// `world` live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_load_report(world: *const FynkelWorld) -> FynkelBufferView {
    match world.as_ref() {
        Some(w) if !w.load_report.is_empty() => FynkelBufferView::of(&w.load_report),
        _ => FynkelBufferView::empty(),
    }
}
