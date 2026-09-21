//! `fynkel_world_load_project` / `fynkel_world_load_scene` /
//! `fynkel_world_load_scene_with` / `fynkel_world_unload_scene` /
//! `fynkel_world_bind_actions` / `fynkel_world_load_report`: cooked scenes
//! into the world through `fynkel_assets` (M3-03: a host reads the cache
//! only — no source document is ever opened here), with a JSON report the
//! host can show. Loads replace by default (M3-12: every loaded scene
//! unloaded, every other entity destroyed, every residency token
//! dropped); `FYNKEL_LOAD_ADDITIVE` keeps the world.

use std::path::{Path, PathBuf};

use fynkel_assets::{Assets, CookedScene};
use fynkel_contracts::{Diagnostic, Entity, Status, Uuid};
use fynkel_core::{LoadError, LoadMode, LoadedScene, Transform};
use fynkel_document::diagnostic_json;
use glam::{Quat, Vec3};
use serde_json::{json, Value};

use crate::{guarded, FynkelBufferView, FynkelInstantiateDesc, FynkelWorld};

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
        LoadError::UnknownAnchorParent(_) | LoadError::AnchorTransform => Code::IO,
        LoadError::AlreadyLoaded(_) => Code::RUNTIME_SCENE_ALREADY_LOADED,
        LoadError::NotLoaded(_) => Code::RUNTIME_SCENE_NOT_LOADED,
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

/// The world's binding table from the project's actions with the user's
/// rebinds over them (M3-12), as the report's `input` entry.
fn bind_project_input(w: &mut FynkelWorld, assets: &Assets) -> Value {
    let Some(p) = assets.project() else {
        return Value::Null;
    };
    let mut specs = p.input_actions.clone();
    for spec in &mut specs {
        if let Some(b) = w.rebinds.get(&spec.name) {
            spec.bindings = b.clone();
        }
    }
    let errors = w.core_mut().bind_input(&specs);
    json!({
        "actions": specs.iter().map(|a| json!({
            "name": a.name, "type": a.kind.name(), "bindings": a.bindings,
            "rebound": w.rebinds.contains_key(&a.name),
        })).collect::<Vec<_>>(),
        "bindingErrors": errors.iter().map(|(action, binding, e)| json!({
            "action": action, "binding": binding, "message": e.to_string(),
        })).collect::<Vec<_>>(),
    })
}

fn residency_json(assets: &Assets) -> Value {
    let stats = assets.stats();
    json!({
        "retained": stats.retained,
        "meshes": stats.meshes,
        "textures": stats.textures,
        "materials": stats.materials,
        "bytes": stats.bytes,
    })
}

fn scenes_json(w: &FynkelWorld) -> Vec<Value> {
    w.core()
        .loaded_scenes()
        .iter()
        .map(|s| json!({"id": s.id.to_string(), "name": s.name, "entities": s.len()}))
        .collect()
}

/// Spawns the cooked scene, retains its closure and builds the report.
fn instantiate(w: &mut FynkelWorld, assets: Assets, scene: CookedScene, mode: LoadMode) -> Outcome {
    let cache = cache_json(&assets);
    // Physics (M3-08): the project's gravity, and every mesh collider's
    // mesh registered before the first step builds the colliders.
    let (physics, animation, audio) = {
        let core = w.core_mut();
        if let Some(p) = assets.project() {
            core.physics_mut()
                .set_gravity(fynkel_core::physics::Vec3::from(p.physics.gravity));
        }
        let (meshes, mut diags) =
            assets.register_mesh_colliders(&scene.description, core.physics_mut());
        // The catalogue (M3-10): every prefab an emitter names.
        let (prefabs, prefab_diags) = assets.register_prefabs(&scene.description, core);
        diags.extend(prefab_diags);
        // Animation (M3-19): every clip an animator plays and every skin.
        let (clips, skins, anim_diags) = assets.register_animation(&scene.description, core);
        diags.extend(anim_diags);
        // Audio (M3-11): every sound a source names.
        let (sounds, audio_diags) = assets.register_audio(&scene.description, core);
        diags.extend(audio_diags);
        let gravity = core.physics().gravity();
        let stats = core.audio_stats();
        (
            json!({
                "gravity": [gravity.x, gravity.y, gravity.z],
                "layers": assets.project().map(|p| p.physics.layers.clone()).unwrap_or_default(),
                "meshColliders": meshes,
                "prefabs": prefabs,
                "diagnostics": diags.iter().map(diagnostic_json).collect::<Vec<_>>(),
            }),
            json!({ "clips": clips, "skins": skins }),
            json!({
                "sounds": sounds,
                "clips": stats.clips,
                "streams": stats.streams,
                "device": stats.device,
                "sampleRate": stats.sample_rate,
                "channels": stats.channels,
                "maxVoices": stats.max_voices,
            }),
        )
    };
    match w.core_mut().load_scene_with(&scene.description, mode) {
        Ok(loaded) => {
            // A replace load emptied the world: nothing keeps the previous
            // closures resident.
            if mode == LoadMode::Replace {
                w.residency.clear();
            }
            let residency = assets.retain(&scene.closure);
            w.residency.push((Some(loaded.id), residency));
            let input = bind_project_input(w, &assets);
            let residency = residency_json(&assets);
            w.assets = Some(assets);
            Outcome {
                status: Status::Ok,
                report: json!({
                    "ok": true,
                    "mode": match mode {
                        LoadMode::Replace => "replace",
                        LoadMode::Additive => "additive",
                    },
                    "scene": scene_json(&loaded, &scene),
                    "scenes": scenes_json(w),
                    "cache": cache,
                    "residency": residency,
                    "diagnostics": scene.diagnostics.iter().map(diagnostic_json).collect::<Vec<_>>(),
                    "input": input,
                    "physics": physics,
                    "animation": animation,
                    "audio": audio,
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

/// Unloads a loaded scene (M3-12): its entities go, its residency token
/// with them.
fn unload(w: &mut FynkelWorld, id: Uuid) -> Outcome {
    let cache = w.assets.as_ref().map(cache_json).unwrap_or(Value::Null);
    match w.core_mut().unload_scene(id) {
        Ok(gone) => {
            w.residency.retain(|(scene, _)| *scene != Some(id));
            let residency = w.assets.as_ref().map(residency_json).unwrap_or(Value::Null);
            Outcome {
                status: Status::Ok,
                report: json!({
                    "ok": true,
                    "scene": Value::Null,
                    "unloaded": {
                        "id": gone.id.to_string(),
                        "name": gone.name,
                        "entities": gone.entities,
                    },
                    "scenes": scenes_json(w),
                    "cache": cache,
                    "residency": residency,
                    "diagnostics": [],
                }),
            }
        }
        Err(e) => failed(&[load_error_diagnostic(&e)], cache),
    }
}

/// Replaces the user's rebinds and rebinds the world's actions from the
/// loaded project with them (M3-12).
fn bind_actions(
    w: &mut FynkelWorld,
    rebinds: std::collections::BTreeMap<String, Vec<String>>,
) -> Outcome {
    w.rebinds = rebinds;
    let Some(assets) = w.assets.clone() else {
        return failed(
            &[Diagnostic::error(
                fynkel_contracts::Code::RUNTIME_CACHE_UNREADABLE,
                "bind_actions: no cooked project is loaded in this world",
            )
            .with_suggestion("load the project (fynkel_world_load_project) first")],
            Value::Null,
        );
    };
    let input = bind_project_input(w, &assets);
    Outcome {
        status: Status::Ok,
        report: json!({
            "ok": true,
            "scene": Value::Null,
            "cache": cache_json(&assets),
            "diagnostics": [],
            "input": input,
        }),
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

/// Writes the report with the call's wall time (`timing.ms`, M3-16: the
/// load/startup time of blueprint §15.4) and refreshes the diagnostics.
fn finish(w: &mut FynkelWorld, mut out: Outcome, started: std::time::Instant) -> Status {
    if let Some(o) = out.report.as_object_mut() {
        let ms = started.elapsed().as_secs_f64() * 1e3;
        o.insert("timing".into(), json!({ "ms": (ms * 1e3).round() / 1e3 }));
    }
    w.load_report = serde_json::to_vec(&out.report).unwrap_or_default();
    w.stats.bytes_out += w.load_report.len() as u64;
    w.rebuild_diagnostics();
    out.status
}

fn load(w: &mut FynkelWorld, cache_dir: &Path, scene: Option<Uuid>, mode: LoadMode) -> Outcome {
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
        Ok(scene) => instantiate(w, assets, scene, mode),
        Err(diags) => failed(&diags, cache),
    }
}

/// Instantiates a cooked prefab from the world's store (M3-08b).
fn instantiate_prefab(
    w: &mut FynkelWorld,
    id: Uuid,
    name: Option<&str>,
    transform: Transform,
    parent: Option<Entity>,
) -> Outcome {
    let Some(assets) = w.assets.clone() else {
        return failed(
            &[Diagnostic::error(
                fynkel_contracts::Code::RUNTIME_CACHE_UNREADABLE,
                "instantiate: no cooked project is loaded in this world",
            )
            .with_suggestion("load the project (fynkel_world_load_project) first")],
            Value::Null,
        );
    };
    let cache = cache_json(&assets);
    let prefab = match assets.prefab(id) {
        Ok(p) => p,
        Err(diags) => return failed(&diags, cache),
    };
    let (meshes, mut mesh_diags) =
        assets.register_mesh_colliders(&prefab.description, w.core_mut().physics_mut());
    let (prefabs, prefab_diags) = assets.register_prefabs(&prefab.description, w.core_mut());
    mesh_diags.extend(prefab_diags);
    let (_, _, anim_diags) = assets.register_animation(&prefab.description, w.core_mut());
    mesh_diags.extend(anim_diags);
    let (_, audio_diags) = assets.register_audio(&prefab.description, w.core_mut());
    mesh_diags.extend(audio_diags);
    let name = name.unwrap_or(&prefab.name);
    match w
        .core_mut()
        .instantiate(&prefab.description, name, transform, parent)
    {
        Ok((anchor, loaded)) => {
            let residency = assets.retain(&prefab.closure);
            let stats = assets.stats();
            w.residency.push((None, residency));
            let behaviours: Vec<Value> = prefab
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
            let mut diagnostics: Vec<Value> =
                prefab.diagnostics.iter().map(diagnostic_json).collect();
            diagnostics.extend(mesh_diags.iter().map(diagnostic_json));
            Outcome {
                status: Status::Ok,
                report: json!({
                    "ok": true,
                    "scene": Value::Null,
                    "instance": {
                        "prefab": prefab.id.to_string(),
                        "name": name,
                        "path": prefab.path,
                        "anchor": anchor.to_bits(),
                        "entities": loaded.entities().len(),
                        "rejectedComponents": loaded.rejected_components,
                        "assets": prefab.closure.len(),
                        "meshColliders": meshes,
                        "prefabs": prefabs,
                        "behaviours": behaviours,
                    },
                    "cache": cache,
                    "residency": {
                        "retained": stats.retained,
                        "meshes": stats.meshes,
                        "textures": stats.textures,
                        "materials": stats.materials,
                        "bytes": stats.bytes,
                    },
                    "diagnostics": diagnostics,
                }),
            }
        }
        Err(e) => {
            let mut diags = prefab.diagnostics;
            diags.push(load_error_diagnostic(&e));
            failed(&diags, cache)
        }
    }
}

/// Instantiates the cooked prefab `desc.prefab_id_utf8` as the children
/// of a new anchor entity (see the header).
///
/// # Safety
/// `world` live or NULL; `desc` NULL or valid, its strings readable for
/// their lengths.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_instantiate(
    world: *mut FynkelWorld,
    desc: *const FynkelInstantiateDesc,
) -> Status {
    let Some(d) = desc.as_ref() else {
        return Status::InvalidArgument;
    };
    let Some(id) = utf8(d.prefab_id_utf8, d.prefab_id_len) else {
        return Status::InvalidArgument;
    };
    let Ok(id) = id.parse::<Uuid>() else {
        return Status::InvalidArgument;
    };
    let name = if d.name_len == 0 {
        None
    } else {
        match utf8(d.name_utf8, d.name_len) {
            Some(n) => Some(n.to_string()),
            None => return Status::InvalidArgument,
        }
    };
    let transform = Transform {
        position: Vec3::from_array(d.position),
        rotation: Quat::from_array(d.rotation),
        scale: Vec3::from_array(d.scale),
    };
    let parent = (d.parent != 0).then(|| Entity::from_bits(d.parent));
    guarded(world, move |w| {
        let started = std::time::Instant::now();
        let out = instantiate_prefab(w, id, name.as_deref(), transform, parent);
        finish(w, out, started)
    })
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
        let started = std::time::Instant::now();
        let out = load(w, &dir, None, LoadMode::Replace);
        finish(w, out, started)
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
        let started = std::time::Instant::now();
        let out = load(w, &dir, Some(id), LoadMode::Replace);
        finish(w, out, started)
    })
}

/// `fynkel_world_load_scene` with `flags` (M3-12): `0` replaces (every
/// loaded scene unloaded and every other entity destroyed first),
/// `FYNKEL_LOAD_ADDITIVE` (1) keeps the world and refuses a scene id that
/// is loaded already (`FYN-R0011`). Unknown flag bits are
/// `InvalidArgument`.
///
/// # Safety
/// `world` live or NULL; the two strings point at their lengths' bytes.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_load_scene_with(
    world: *mut FynkelWorld,
    cache_utf8: *const u8,
    cache_len: u32,
    scene_id_utf8: *const u8,
    scene_id_len: u32,
    flags: u32,
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
    let mode = match flags {
        0 => LoadMode::Replace,
        FYNKEL_LOAD_ADDITIVE => LoadMode::Additive,
        _ => return Status::InvalidArgument,
    };
    let dir = PathBuf::from(dir);
    guarded(world, move |w| {
        let started = std::time::Instant::now();
        let out = load(w, &dir, Some(id), mode);
        finish(w, out, started)
    })
}

/// `fynkel_world_load_scene_with` flag: keep what the world holds.
pub const FYNKEL_LOAD_ADDITIVE: u32 = 1;

/// Unloads the loaded scene `scene_id_utf8` (M3-12): its still-alive
/// entities are destroyed (`DESTROYED` events, then `SCENE_UNLOADED`) and
/// its assets released from residency. A scene the world does not hold
/// is `LoadFailed` with `FYN-R0010` in the report.
///
/// # Safety
/// `world` live or NULL; the string points at `scene_id_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_unload_scene(
    world: *mut FynkelWorld,
    scene_id_utf8: *const u8,
    scene_id_len: u32,
) -> Status {
    let Some(id) = utf8(scene_id_utf8, scene_id_len) else {
        return Status::InvalidArgument;
    };
    let Ok(id) = id.parse::<Uuid>() else {
        return Status::InvalidArgument;
    };
    guarded(world, move |w| {
        let started = std::time::Instant::now();
        let out = unload(w, id);
        finish(w, out, started)
    })
}

/// Replaces the user's input rebinds with `json_utf8` — a JSON object
/// `{ "<action>": ["<device>:<control>", …], … }` (an empty object clears
/// them) — and rebinds the world's actions from the loaded project with
/// those bindings over its own (M3-12; `settings.json` → `input.rebinds`).
/// They stay over every later load. The report's `input` entry lists the
/// table and its binding errors (a bad binding is reported, never fatal).
/// `LoadFailed` before any project is loaded.
///
/// # Safety
/// `world` live or NULL; the string points at `json_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_bind_actions(
    world: *mut FynkelWorld,
    json_utf8: *const u8,
    json_len: u32,
) -> Status {
    let Some(text) = utf8(json_utf8, json_len) else {
        return Status::InvalidArgument;
    };
    let Ok(rebinds) = serde_json::from_str::<std::collections::BTreeMap<String, Vec<String>>>(text)
    else {
        return Status::InvalidArgument;
    };
    guarded(world, move |w| {
        let started = std::time::Instant::now();
        let out = bind_actions(w, rebinds);
        finish(w, out, started)
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
