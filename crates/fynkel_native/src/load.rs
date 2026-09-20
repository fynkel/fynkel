//! `fynkel_world_load_project` / `fynkel_world_load_scene` /
//! `fynkel_world_load_report`: cooked scenes into the world through
//! `fynkel_assets` (M3-03: a host reads the cache only — no source
//! document is ever opened here), with a JSON report the host can show.

use std::path::{Path, PathBuf};

use fynkel_assets::{Assets, CookedScene};
use fynkel_contracts::{Diagnostic, Status, Uuid};
use fynkel_core::{LoadError, LoadedScene};
use fynkel_document::diagnostic_json;
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

fn failed(diagnostics: &[Diagnostic], cache: Value) -> Outcome {
    Outcome {
        status: Status::LoadFailed,
        report: json!({
            "ok": false,
            "scene": Value::Null,
            "cache": cache,
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

fn cache_json(assets: &Assets) -> Value {
    let project = assets.project();
    json!({
        "dir": assets.root().display().to_string(),
        "profile": assets.profile(),
        "project": project.as_ref().map(|p| p.name.clone()),
        "startupScene": project.and_then(|p| p.startup_scene).map(|u| u.to_string()),
    })
}

/// The store for `cache_dir`: the world's own when it is the same
/// directory (residency accumulates across loads), else a fresh one.
// A `Diagnostic` is the error type on purpose (failure path only).
#[allow(clippy::result_large_err)]
fn open(w: &mut FynkelWorld, cache_dir: &Path) -> Result<Assets, Diagnostic> {
    if let Some(a) = &w.assets {
        if same_dir(&a.root(), cache_dir) {
            return Ok(a.clone());
        }
    }
    Assets::open(cache_dir)
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// Spawns the cooked scene, retains its closure and builds the report.
fn instantiate(w: &mut FynkelWorld, assets: Assets, scene: CookedScene) -> Outcome {
    let cache = cache_json(&assets);
    match w.core_mut().load_scene(&scene.description) {
        Ok(loaded) => {
            let residency = assets.retain(&scene.closure);
            let stats = assets.stats();
            // The project's actions become the world's binding table
            // (§19.2d item 7); validation reported bad bindings before the
            // cook, so any here are a report entry, never a failure.
            let input = assets.project().map(|p| {
                let errors = w.core_mut().bind_input(&p.input_actions);
                json!({
                    "actions": p.input_actions.iter().map(|a| json!({
                        "name": a.name, "type": a.kind.name(), "bindings": a.bindings,
                    })).collect::<Vec<_>>(),
                    "bindingErrors": errors.iter().map(|(action, binding, e)| json!({
                        "action": action, "binding": binding, "message": e.to_string(),
                    })).collect::<Vec<_>>(),
                })
            });
            w.residency.push(residency);
            w.assets = Some(assets);
            Outcome {
                status: Status::Ok,
                report: json!({
                    "ok": true,
                    "scene": scene_json(&loaded, &scene),
                    "cache": cache,
                    "residency": {
                        "retained": stats.retained,
                        "meshes": stats.meshes,
                        "textures": stats.textures,
                        "materials": stats.materials,
                        "bytes": stats.bytes,
                    },
                    "diagnostics": scene.diagnostics.iter().map(diagnostic_json).collect::<Vec<_>>(),
                    "input": input,
                }),
            }
        }
        Err(e) => {
            let mut diags = scene.diagnostics;
            diags.push(load_error_diagnostic(&e));
            failed(&diags, cache)
        }
    }
}

fn scene_json(loaded: &LoadedScene, scene: &CookedScene) -> Value {
    // `entity` is the spawned handle (what `World.attach` takes); an
    // authored behaviour whose entity was not spawned cannot happen after
    // a successful load, but is dropped rather than reported with a
    // dangling handle.
    let behaviours: Vec<Value> = scene
        .behaviours
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
        "path": scene.path,
        "key": scene.key,
        "entities": loaded.entities().len(),
        "rejectedComponents": loaded.rejected_components,
        "assets": scene.closure.len(),
        "behaviours": behaviours,
    })
}

fn finish(w: &mut FynkelWorld, out: Outcome) -> Status {
    w.load_report = serde_json::to_vec(&out.report).unwrap_or_default();
    w.stats.bytes_out += w.load_report.len() as u64;
    w.rebuild_diagnostics();
    out.status
}

fn load(w: &mut FynkelWorld, cache_dir: &Path, scene: Option<Uuid>) -> Outcome {
    let assets = match open(w, cache_dir) {
        Ok(a) => a,
        Err(d) => return failed(&[d], Value::Null),
    };
    let cache = cache_json(&assets);
    let id = match scene.map(Ok).unwrap_or_else(|| assets.startup_scene()) {
        Ok(id) => id,
        Err(d) => return failed(&[d], cache),
    };
    match assets.scene(id) {
        Ok(scene) => instantiate(w, assets, scene),
        Err(diags) => failed(&diags, cache),
    }
}

/// Loads the startup scene of the cooked project at `cache_utf8` (the
/// `.fynkel/cache` directory `fynkel cook` wrote from the project root).
///
/// # Safety
/// `world` live or NULL; `cache_utf8` points at `cache_len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_load_project(
    world: *mut FynkelWorld,
    cache_utf8: *const u8,
    cache_len: u32,
) -> Status {
    let Some(dir) = utf8(cache_utf8, cache_len) else {
        return Status::InvalidArgument;
    };
    let dir = PathBuf::from(dir);
    guarded(world, move |w| {
        let out = load(w, &dir, None);
        finish(w, out)
    })
}

/// Loads the cooked scene `scene_id_utf8` (an asset id) from the cache at
/// `cache_utf8`.
///
/// # Safety
/// `world` live or NULL; the two strings point at their lengths' bytes.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_load_scene(
    world: *mut FynkelWorld,
    cache_utf8: *const u8,
    cache_len: u32,
    scene_id_utf8: *const u8,
    scene_id_len: u32,
) -> Status {
    let (Some(dir), Some(id)) = (
        utf8(cache_utf8, cache_len),
        utf8(scene_id_utf8, scene_id_len),
    ) else {
        return Status::InvalidArgument;
    };
    let Ok(id) = id.parse::<Uuid>() else {
        return Status::InvalidArgument;
    };
    let dir = PathBuf::from(dir);
    guarded(world, move |w| {
        let out = load(w, &dir, Some(id));
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
