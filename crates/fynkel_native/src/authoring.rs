//! The authoring ABI (`include/fynkel_authoring.h`; blueprint §19.2e item 1,
//! ADR-0016): the editor's C entry points over
//! [`fynkel_authoring::AuthoringSession`]. Compiled only with the cargo
//! feature `authoring`, so a game's runtime never carries it.
//!
//! The shape is "one buffer in, one out, JSON": every call that does
//! something takes UTF-8 text (a path, a request, a query) and writes one
//! JSON result the host reads with [`fynkel_authoring_result`] — `{ "ok":
//! true, ... }` on success, `{ "ok": false, "diagnostics": [...] }` (the
//! ADR-0014 record) when refused. The returned status says which of the
//! two it is without parsing:
//!
//! | status | meaning |
//! | --- | --- |
//! | `OK` | done; the result carries the payload |
//! | `COMMANDS_REJECTED` | an edit was refused by the editor's rules (`FYN-E####`: conflict, invalid command, validation, read-only, nothing to undo) |
//! | `LOAD_FAILED` | a project or document could not be opened, read or written (`FYN-D`/`FYN-P`/`FYN-T0006`) |
//! | `INVALID_ARGUMENT` | NULL, non-UTF-8 or malformed input, no project, an unknown handle or query (`FYN-T0001..T0004`) |
//! | `PANIC` / `POISONED` | a panic was caught (`FYN-T0005`); the session answers `POISONED` from then on — destroy it |
//!
//! Every ABI v1 rule holds: NULL-safe, `catch_unwind` around every call,
//! the result view valid until the next call on the same session, not
//! thread-safe.
//!
//! Version 3 (item 6, the Scene view) adds the edit-preview world: per
//! open scene or prefab a [`FynkelWorld`](crate::FynkelWorld) in
//! `WorldConfig::edit_mode` that [`fynkel_authoring::Preview`] keeps equal
//! to the live document — resynced after every transaction, undo, redo,
//! reload and rescan on this session — and that the viewport ABI draws
//! like any world (`fynkel_apple_viewport_render` over
//! [`fynkel_authoring_preview_world`]). The session still runs no game code
//! (ADR-0016/0018): the world has no systems, and it reads the cooked cache
//! [`fynkel_authoring::OpenProject::cook`] writes (`fynkel cook` in
//! process) for meshes and materials only.

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use fynkel_assets::Assets;
use fynkel_authoring::{AuthoringSession, Handle, Preview, Ray, View};
use fynkel_contracts::{Code, Diagnostic, Status};
use fynkel_core::components::{Camera, ExposureMode, Projection};
use fynkel_core::WorldConfig;
use fynkel_document::diagnostic_json;
use fynkel_scene::Transform;
use glam::{Mat4, Quat, Vec2, Vec3, Vec4};
use serde_json::{json, Value};

use crate::{FynkelBufferView, FynkelWorld};

/// Version of `fynkel_authoring.h`. Bumped on any incompatible change;
/// independent of `FYNKEL_ABI_VERSION`.
pub const AUTHORING_ABI_VERSION: u32 = 3;

/// One editor session: the open project, its documents, their edit-preview
/// worlds, and the last result. Opaque to the host.
pub struct FynkelAuthoringSession {
    inner: AuthoringSession,
    /// The edit-preview worlds, one per document the host opened one for
    /// (`fynkel_authoring_preview_open`). Boxed so the pointer
    /// `fynkel_authoring_preview_world` hands out stays put while the map
    /// changes.
    previews: BTreeMap<Handle, Box<PreviewSlot>>,
    /// The project's cooked cache every preview draws from: opened by the
    /// first preview (a cook in process), replaced by `recook`, dropped
    /// with the project.
    assets: Option<Assets>,
    result: Vec<u8>,
    poisoned: bool,
}

/// One document's edit-preview world and the preview that keeps it equal
/// to the document.
struct PreviewSlot {
    world: FynkelWorld,
    preview: Preview,
}

impl FynkelAuthoringSession {
    /// Brings every preview up to date with its document — after any call
    /// that may have changed a document or what a scene's prefabs expand
    /// to — and adds the reports as `"previews"` to `v` (an object) when
    /// there is at least one preview.
    fn resync_all(&mut self, mut v: Value) -> Value {
        if self.previews.is_empty() {
            return v;
        }
        let mut reports = Vec::with_capacity(self.previews.len());
        for (handle, slot) in self.previews.iter_mut() {
            let mut report = match self.inner.describe_with_anchors(*handle) {
                Ok((described, anchors)) => {
                    let mut r = slot
                        .preview
                        .sync_world(&mut slot.world, &described.description)
                        .json();
                    slot.preview.set_anchors(anchors);
                    if !described.diagnostics.is_empty() {
                        r["describeDiagnostics"] = diags(&described.diagnostics);
                    }
                    r
                }
                Err(d) => json!({ "diagnostics": [diagnostic_json(&d)] }),
            };
            report["handle"] = json!(*handle);
            reports.push(report);
        }
        if v.is_object() {
            v["previews"] = Value::Array(reports);
        }
        v
    }

    /// The cooked cache, cooking the project first when no preview opened
    /// it yet. `Some(report)` when it cooked.
    fn ensure_assets(&mut self) -> Result<(Assets, Option<Value>), Vec<Diagnostic>> {
        if let Some(a) = &self.assets {
            return Ok((a.clone(), None));
        }
        let (assets, report) = self.cook(false)?;
        Ok((assets, Some(report)))
    }

    /// `fynkel cook` in process over the open project and the cache
    /// reopened: every preview's world points at the new store. The cook
    /// report is returned even when some assets failed (the preview draws
    /// the others); a cache that cannot be opened at all is refused.
    /// [live]: the documents with unsaved edits cook as the editor holds
    /// them, their files untouched (Play, ADR-0021 P5).
    fn cook(&mut self, live: bool) -> Result<(Assets, Value), Vec<Diagnostic>> {
        let project = self.inner.project().ok_or_else(|| {
            one(Diagnostic::error(
                Code::SESSION_NO_PROJECT,
                "preview: no project is open",
            ))
        })?;
        let report = if live {
            let texts = self.inner.unsaved_texts();
            project.write_recovery(&texts);
            project.cook_live(texts)
        } else {
            project.cook()
        };
        let assets = Assets::open(&project.cache_dir()).map_err(one)?;
        self.assets = Some(assets.clone());
        for slot in self.previews.values_mut() {
            slot.world.assets = Some(assets.clone());
        }
        Ok((assets, report.json()))
    }

    fn slot(&mut self, handle: Handle) -> Result<&mut PreviewSlot, Vec<Diagnostic>> {
        self.previews
            .get_mut(&handle)
            .map(|b| &mut **b)
            .ok_or_else(|| {
                one(Diagnostic::error(
                    Code::SESSION_UNKNOWN_DOCUMENT,
                    format!("preview: document {handle} has no edit-preview world"),
                )
                .with_suggestion("open one with fynkel_authoring_preview_open first"))
            })
    }

    fn drop_previews(&mut self) {
        self.previews.clear();
        self.assets = None;
    }

    fn set_result(&mut self, v: &Value) {
        self.result.clear();
        // Writing into a Vec cannot fail.
        serde_json::to_writer(&mut self.result, v).expect("JSON into a Vec");
    }

    fn refuse(&mut self, diagnostics: &[Diagnostic]) {
        self.set_result(&json!({
            "ok": false,
            "diagnostics": diagnostics.iter().map(diagnostic_json).collect::<Vec<_>>(),
        }));
    }
}

fn utf8<'a>(ptr: *const u8, len: u32) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: the caller promises `len` readable bytes at `ptr` for the
    // duration of the call.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    std::str::from_utf8(bytes).ok()
}

fn invalid(message: impl Into<String>) -> Vec<Diagnostic> {
    vec![Diagnostic::error(Code::SESSION_INVALID_REQUEST, message)]
}

/// The status a refusal maps to, from its first diagnostic's namespace.
fn status_for(diagnostics: &[Diagnostic]) -> Status {
    let code = diagnostics.first().map(|d| d.code.as_str()).unwrap_or("");
    if code.starts_with("FYN-E") {
        Status::CommandsRejected
    } else if matches!(code, "FYN-T0001" | "FYN-T0002" | "FYN-T0003" | "FYN-T0004") {
        Status::InvalidArgument
    } else {
        Status::LoadFailed
    }
}

type Outcome = Result<Value, Vec<Diagnostic>>;

/// Runs `f` on a live, non-poisoned session inside the panic boundary and
/// stores its result. A payload whose own `ok` is false (a refused
/// transaction) is `COMMANDS_REJECTED`; a missing `ok` becomes `true`.
fn guarded(
    session: *mut FynkelAuthoringSession,
    f: impl FnOnce(&mut AuthoringSession) -> Outcome,
) -> Status {
    guarded_session(session, |s| f(&mut s.inner))
}

/// [`guarded`] over the whole session — for the calls that touch the
/// edit-preview worlds next to the authoring session.
fn guarded_session(
    session: *mut FynkelAuthoringSession,
    f: impl FnOnce(&mut FynkelAuthoringSession) -> Outcome,
) -> Status {
    // SAFETY: a non-NULL session comes from `fynkel_authoring_session_create`
    // and is used from one thread at a time (the header's contract).
    let Some(s) = (unsafe { session.as_mut() }) else {
        return Status::InvalidArgument;
    };
    if s.poisoned {
        s.refuse(&[Diagnostic::error(
            Code::SESSION_PANIC,
            "the session is poisoned by an earlier panic",
        )
        .with_suggestion("destroy the session and create a new one")]);
        return Status::Poisoned;
    }
    match catch_unwind(AssertUnwindSafe(|| f(s))) {
        Ok(Ok(mut v)) => {
            let refused = v.get("ok") == Some(&Value::Bool(false));
            if !v.is_object() {
                v = json!({ "result": v });
            }
            if v.get("ok").is_none() {
                v["ok"] = Value::Bool(true);
            }
            s.set_result(&v);
            if refused {
                Status::CommandsRejected
            } else {
                Status::Ok
            }
        }
        Ok(Err(diagnostics)) => {
            s.refuse(&diagnostics);
            status_for(&diagnostics)
        }
        Err(payload) => {
            let msg = payload
                .downcast_ref::<&str>()
                .map(|m| m.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "non-string panic".into());
            log::error!(
                "fynkel_native: panic caught at the authoring ABI edge, session poisoned: {msg}"
            );
            s.poisoned = true;
            s.refuse(&[Diagnostic::error(
                Code::SESSION_PANIC,
                format!("panic in the authoring session: {msg}"),
            )
            .with_suggestion("destroy the session and create a new one; report the message")]);
            Status::Panic
        }
    }
}

/// Like [`guarded`] for a call whose input failed to decode before it ran.
fn refused(session: *mut FynkelAuthoringSession, diagnostics: Vec<Diagnostic>) -> Status {
    guarded(session, move |_| Err(diagnostics))
}

fn one(d: Diagnostic) -> Vec<Diagnostic> {
    vec![d]
}

/// The world and its store, borrowed apart (the preview writes the one
/// and reads the other).
fn parts(w: &mut FynkelWorld) -> (&mut fynkel_core::World, Option<&Assets>) {
    (&mut w.inner, w.assets.as_ref())
}

/// [`Preview::sync`] over a [`FynkelWorld`].
trait SyncWorld {
    fn sync_world(
        &mut self,
        w: &mut FynkelWorld,
        desired: &fynkel_core::SceneDescription,
    ) -> fynkel_authoring::SyncReport;
}

impl SyncWorld for Preview {
    fn sync_world(
        &mut self,
        w: &mut FynkelWorld,
        desired: &fynkel_core::SceneDescription,
    ) -> fynkel_authoring::SyncReport {
        let (world, assets) = parts(w);
        self.sync(world, assets, desired)
    }
}

fn diags(list: &[Diagnostic]) -> Value {
    Value::Array(list.iter().map(diagnostic_json).collect())
}

// ---------------------------------------------------------------- exports

#[no_mangle]
pub extern "C" fn fynkel_authoring_abi_version() -> u32 {
    AUTHORING_ABI_VERSION
}

/// A new session with no project. NULL only if the allocation panicked.
#[no_mangle]
pub extern "C" fn fynkel_authoring_session_create() -> *mut FynkelAuthoringSession {
    match catch_unwind(|| FynkelAuthoringSession {
        inner: AuthoringSession::new(),
        previews: BTreeMap::new(),
        assets: None,
        result: Vec::new(),
        poisoned: false,
    }) {
        Ok(s) => Box::into_raw(Box::new(s)),
        Err(_) => std::ptr::null_mut(),
    }
}

/// # Safety
/// `session` comes from `fynkel_authoring_session_create` and is not used
/// afterwards; NULL is ignored.
#[no_mangle]
pub unsafe extern "C" fn fynkel_authoring_session_destroy(session: *mut FynkelAuthoringSession) {
    if !session.is_null() {
        // A panicking Drop must not unwind into the host either.
        let _ = catch_unwind(AssertUnwindSafe(|| drop(Box::from_raw(session))));
    }
}

/// # Safety
/// `session` live or NULL; `dir_utf8` readable for `dir_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn fynkel_authoring_open_project(
    session: *mut FynkelAuthoringSession,
    dir_utf8: *const u8,
    dir_len: u32,
) -> Status {
    let Some(dir) = utf8(dir_utf8, dir_len) else {
        return refused(session, invalid("open_project: dir must be UTF-8 text"));
    };
    let dir = PathBuf::from(dir);
    guarded_session(session, move |s| {
        s.drop_previews();
        let project = s.inner.open_project(&dir)?;
        Ok(json!({ "ok": true, "project": project.json() }))
    })
}

/// # Safety
/// `session` live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_authoring_close_project(
    session: *mut FynkelAuthoringSession,
) -> Status {
    guarded_session(session, |s| {
        s.drop_previews();
        s.inner.close_project();
        Ok(json!({ "ok": true }))
    })
}

/// # Safety
/// `session` live or NULL; `path_or_id_utf8` readable for `len` bytes;
/// `out_handle` writable or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_authoring_open_document(
    session: *mut FynkelAuthoringSession,
    path_or_id_utf8: *const u8,
    len: u32,
    out_handle: *mut u32,
) -> Status {
    let Some(text) = utf8(path_or_id_utf8, len) else {
        return refused(
            session,
            invalid("open_document: path or id must be UTF-8 text"),
        );
    };
    let text = text.to_string();
    guarded(session, move |s| {
        let handle = s.open_document(&text)?;
        if !out_handle.is_null() {
            // SAFETY: the caller promises a writable u32 or NULL.
            unsafe { *out_handle = handle };
        }
        Ok(json!({ "ok": true, "handle": handle, "state": s.state_json(handle).map_err(one)? }))
    })
}

/// # Safety
/// `session` live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_authoring_close_document(
    session: *mut FynkelAuthoringSession,
    handle: u32,
) -> Status {
    guarded_session(session, move |s| {
        s.inner.close_document(handle as Handle).map_err(one)?;
        let preview = s.previews.remove(&(handle as Handle)).is_some();
        Ok(json!({ "ok": true, "handle": handle, "previewClosed": preview }))
    })
}

/// # Safety
/// `session` live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_authoring_reload_document(
    session: *mut FynkelAuthoringSession,
    handle: u32,
) -> Status {
    guarded_session(session, move |s| {
        s.inner.reload(handle as Handle)?;
        let v = json!({ "ok": true, "state": s.inner.state_json(handle as Handle).map_err(one)? });
        Ok(s.resync_all(v))
    })
}

/// # Safety
/// `session` live or NULL; `json_utf8` readable for `json_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn fynkel_authoring_apply(
    session: *mut FynkelAuthoringSession,
    handle: u32,
    json_utf8: *const u8,
    json_len: u32,
) -> Status {
    let Some(text) = utf8(json_utf8, json_len) else {
        return refused(session, invalid("apply: the request must be UTF-8 JSON"));
    };
    let request: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            return refused(
                session,
                invalid(format!("apply: the request is not JSON: {e}")),
            )
        }
    };
    guarded_session(session, move |s| {
        let v = s.inner.apply_json(handle as Handle, &request)?;
        Ok(s.resync_all(v))
    })
}

/// # Safety
/// `session` live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_authoring_undo(
    session: *mut FynkelAuthoringSession,
    handle: u32,
) -> Status {
    guarded_session(session, move |s| {
        let t = s.inner.undo(handle as Handle).map_err(one)?;
        let v = s
            .inner
            .transaction_json(handle as Handle, &t)
            .map_err(one)?;
        Ok(s.resync_all(v))
    })
}

/// # Safety
/// `session` live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_authoring_redo(
    session: *mut FynkelAuthoringSession,
    handle: u32,
) -> Status {
    guarded_session(session, move |s| {
        let t = s.inner.redo(handle as Handle).map_err(one)?;
        let v = s
            .inner
            .transaction_json(handle as Handle, &t)
            .map_err(one)?;
        Ok(s.resync_all(v))
    })
}

/// # Safety
/// `session` live or NULL; `path_utf8` readable for `path_len` bytes, or
/// NULL with `path_len` 0 to save in place.
#[no_mangle]
pub unsafe extern "C" fn fynkel_authoring_save(
    session: *mut FynkelAuthoringSession,
    handle: u32,
    path_utf8: *const u8,
    path_len: u32,
) -> Status {
    let path = if path_utf8.is_null() && path_len == 0 {
        None
    } else {
        match utf8(path_utf8, path_len) {
            Some(p) if !p.is_empty() => Some(PathBuf::from(p)),
            Some(_) => None,
            None => return refused(session, invalid("save: path must be UTF-8 text or NULL")),
        }
    };
    guarded_session(session, move |s| {
        let written = s
            .inner
            .save(handle as Handle, path.as_deref())
            .map_err(one)?;
        let v = json!({
            "ok": true,
            "path": written.display().to_string(),
            "state": s.inner.state_json(handle as Handle).map_err(one)?,
        });
        Ok(s.resync_all(v))
    })
}

/// # Safety
/// `session` live or NULL; `paths_json_utf8` readable for `len` bytes, or
/// NULL with `len` 0 for a full rescan.
#[no_mangle]
pub unsafe extern "C" fn fynkel_authoring_rescan(
    session: *mut FynkelAuthoringSession,
    paths_json_utf8: *const u8,
    len: u32,
) -> Status {
    let paths: Vec<PathBuf> = if paths_json_utf8.is_null() && len == 0 {
        Vec::new()
    } else {
        let Some(text) = utf8(paths_json_utf8, len) else {
            return refused(session, invalid("rescan: paths must be UTF-8 JSON or NULL"));
        };
        match serde_json::from_str::<Value>(text) {
            Ok(Value::Array(items)) if items.iter().all(Value::is_string) => items
                .iter()
                .map(|p| PathBuf::from(p.as_str().unwrap_or_default()))
                .collect(),
            Ok(Value::Null) => Vec::new(),
            Ok(_) => {
                return refused(
                    session,
                    invalid("rescan: paths must be a JSON array of strings"),
                )
            }
            Err(e) => return refused(session, invalid(format!("rescan: paths are not JSON: {e}"))),
        }
    };
    guarded_session(session, move |s| {
        let scan = s.inner.rescan(&paths).map_err(one)?;
        let project = s.inner.project().expect("rescan needs a project");
        let v = json!({
            "ok": true,
            "assetCount": project.assets.len(),
            "scanned": paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            "sidecarsWritten": project.sidecars_written.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            "diagnostics": scan.iter().map(diagnostic_json).collect::<Vec<_>>(),
        });
        Ok(s.resync_all(v))
    })
}

/// # Safety
/// `session` live or NULL; `kind_utf8` readable for `kind_len` bytes;
/// `args_json_utf8` readable for `args_len` bytes, or NULL with `args_len`
/// 0 for no arguments.
#[no_mangle]
pub unsafe extern "C" fn fynkel_authoring_query(
    session: *mut FynkelAuthoringSession,
    kind_utf8: *const u8,
    kind_len: u32,
    args_json_utf8: *const u8,
    args_len: u32,
) -> Status {
    let (kind, args) = match kind_and_args("query", kind_utf8, kind_len, args_json_utf8, args_len) {
        Ok(pair) => pair,
        Err(diagnostics) => return refused(session, diagnostics),
    };
    guarded(session, move |s| {
        let result = s.query(&kind, &args)?;
        Ok(json!({ "ok": true, "kind": kind, "result": result }))
    })
}

/// Parses the `(kind, args)` pair `query` and `operate` share.
unsafe fn kind_and_args(
    what: &str,
    kind_utf8: *const u8,
    kind_len: u32,
    args_json_utf8: *const u8,
    args_len: u32,
) -> Result<(String, Value), Vec<Diagnostic>> {
    let Some(kind) = utf8(kind_utf8, kind_len) else {
        return Err(invalid(format!("{what}: kind must be UTF-8 text")));
    };
    let kind = kind.to_string();
    let args: Value = if args_json_utf8.is_null() && args_len == 0 {
        json!({})
    } else {
        let Some(text) = utf8(args_json_utf8, args_len) else {
            return Err(invalid(format!("{what}: args must be UTF-8 JSON or NULL")));
        };
        match serde_json::from_str::<Value>(text) {
            Ok(v @ Value::Object(_)) => v,
            Ok(Value::Null) => json!({}),
            Ok(_) => return Err(invalid(format!("{what}: args must be a JSON object"))),
            Err(e) => return Err(invalid(format!("{what}: args are not JSON: {e}"))),
        }
    };
    Ok((kind, args))
}

/// # Safety
/// `session` live or NULL; `kind_utf8` readable for `kind_len` bytes;
/// `args_json_utf8` readable for `args_len` bytes, or NULL with `args_len`
/// 0 for no arguments.
#[no_mangle]
pub unsafe extern "C" fn fynkel_authoring_operate(
    session: *mut FynkelAuthoringSession,
    kind_utf8: *const u8,
    kind_len: u32,
    args_json_utf8: *const u8,
    args_len: u32,
) -> Status {
    let (kind, args) = match kind_and_args("operate", kind_utf8, kind_len, args_json_utf8, args_len)
    {
        Ok(pair) => pair,
        Err(diagnostics) => return refused(session, diagnostics),
    };
    guarded_session(session, move |s| {
        let v = s.inner.operate(&kind, &args)?;
        Ok(s.resync_all(v))
    })
}

// ---------------------------------------------------------------- previews

/// Opens the edit-preview world of the scene or prefab at `handle`
/// (version 3): the project cooked into its cache when no preview did yet,
/// an edit-mode world loaded fresh from the live document. Idempotent: a
/// preview already open is left as it is (`"opened": false`).
///
/// # Safety
/// `session` live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_authoring_preview_open(
    session: *mut FynkelAuthoringSession,
    handle: u32,
) -> Status {
    guarded_session(session, move |s| {
        let handle = handle as Handle;
        if let Some(slot) = s.previews.get(&handle) {
            return Ok(json!({
                "ok": true,
                "handle": handle,
                "opened": false,
                "stats": slot.preview.stats(slot.world.core()),
                "diagnostics": diags(&slot.preview.diagnostics),
            }));
        }
        // Described first: a material (or a closed handle) is refused
        // before anything is cooked.
        let (described, anchors) = s.inner.describe_with_anchors(handle).map_err(one)?;
        let document = s
            .inner
            .document(handle)
            .map_err(one)?
            .editor
            .document()
            .envelope
            .id;
        let (assets, cook) = s.ensure_assets()?;
        let mut world = FynkelWorld::new(WorldConfig {
            edit_mode: true,
            ..WorldConfig::default()
        });
        world.assets = Some(assets.clone());
        let mut preview = Preview::load(
            document,
            world.core_mut(),
            Some(&assets),
            &described.description,
        );
        preview.set_anchors(anchors);
        let mut diagnostics = described.diagnostics.clone();
        diagnostics.extend(preview.diagnostics.iter().cloned());
        let stats = preview.stats(world.core());
        s.previews
            .insert(handle, Box::new(PreviewSlot { world, preview }));
        Ok(json!({
            "ok": true,
            "handle": handle,
            "opened": true,
            "cook": cook,
            "stats": stats,
            "diagnostics": diags(&diagnostics),
        }))
    })
}

/// Drops the edit-preview world of `handle` (the document stays open).
/// The host must not render its world pointer afterwards.
///
/// # Safety
/// `session` live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_authoring_preview_close(
    session: *mut FynkelAuthoringSession,
    handle: u32,
) -> Status {
    guarded_session(session, move |s| {
        let closed = s.previews.remove(&(handle as Handle)).is_some();
        Ok(json!({ "ok": true, "handle": handle, "closed": closed }))
    })
}

/// The edit-preview world of `handle` for the viewport ABI
/// (`fynkel_apple_viewport_render`, and `fynkel_world_*` reads such as
/// `fynkel_world_stats`); NULL when none is open, the session is NULL or
/// poisoned. Valid until `fynkel_authoring_preview_close`, the document's
/// close, the project's close or the session's destruction. Never write to
/// it through `fynkel_world_submit`/`fynkel_world_step`: the preview owns
/// its commands and steps.
///
/// # Safety
/// `session` live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_authoring_preview_world(
    session: *const FynkelAuthoringSession,
    handle: u32,
) -> *const FynkelWorld {
    match session.as_ref() {
        Some(s) if !s.poisoned => s
            .previews
            .get(&(handle as Handle))
            .map(|slot| &slot.world as *const FynkelWorld)
            .unwrap_or(std::ptr::null()),
        _ => std::ptr::null(),
    }
}

/// A call on the edit-preview world of `handle`, by kind with JSON
/// arguments (see `include/fynkel_authoring.h`): `camera`, `hidden`,
/// `pick`, `pickRect`, `lines`, `worldMatrix`, `setTransforms`,
/// `restoreTransforms`, `setComponents`, `restoreComponents`, `resync`, `reload`, `recook`, `dump`, `stats`.
///
/// # Safety
/// `session` live or NULL; `kind_utf8` readable for `kind_len` bytes;
/// `args_json_utf8` readable for `args_len` bytes, or NULL with `args_len`
/// 0 for no arguments.
#[no_mangle]
pub unsafe extern "C" fn fynkel_authoring_preview_call(
    session: *mut FynkelAuthoringSession,
    handle: u32,
    kind_utf8: *const u8,
    kind_len: u32,
    args_json_utf8: *const u8,
    args_len: u32,
) -> Status {
    let (kind, args) = match kind_and_args("preview", kind_utf8, kind_len, args_json_utf8, args_len)
    {
        Ok(pair) => pair,
        Err(diagnostics) => return refused(session, diagnostics),
    };
    guarded_session(session, move |s| {
        preview_call(s, handle as Handle, &kind, &args)
    })
}

const PREVIEW_KINDS: [&str; 15] = [
    "camera",
    "hidden",
    "pick",
    "pickRect",
    "lines",
    "worldMatrix",
    "setTransforms",
    "restoreTransforms",
    "setComponents",
    "restoreComponents",
    "resync",
    "reload",
    "recook",
    "dump",
    "stats",
];

fn preview_call(
    s: &mut FynkelAuthoringSession,
    handle: Handle,
    kind: &str,
    args: &Value,
) -> Outcome {
    match kind {
        "camera" => {
            let world = mat4_arg(args, "world")?;
            let camera = camera_arg(args)?;
            let slot = s.slot(handle)?;
            slot.world.core_mut().set_edit_camera(Some((world, camera)));
            Ok(json!({ "ok": true, "handle": handle }))
        }
        "hidden" => {
            let ids = ids_arg(args, "ids")?;
            let slot = s.slot(handle)?;
            // The flag hides a subtree: the ids' descendants go with them.
            let mut all: Vec<uuid::Uuid> = Vec::new();
            for id in &ids {
                for e in slot.preview.subtree(*id) {
                    if !all.contains(&e) {
                        all.push(e);
                    }
                }
            }
            slot.preview.set_hidden(slot.world.core_mut(), &all);
            Ok(json!({ "ok": true, "handle": handle, "hidden": all.len() }))
        }
        "pick" => {
            let ray = if args.get("ndc").is_some() {
                view_arg(args)?.ray(vec2_arg(args, "ndc")?)
            } else {
                let ray = args
                    .get("ray")
                    .ok_or_else(|| invalid("pick: needs `ray` or `view` + `ndc`"))?;
                Ray {
                    origin: vec3_arg(ray, "origin")?,
                    dir: vec3_arg(ray, "dir")?.normalize_or(Vec3::NEG_Z),
                }
            };
            let slot = s.slot(handle)?;
            let hits = slot
                .preview
                .pick(slot.world.core(), slot.world.assets(), &ray);
            Ok(json!({
                "ok": true,
                "handle": handle,
                "ray": { "origin": ray.origin.to_array(), "dir": ray.dir.to_array() },
                "hits": hits.iter().map(|h| h.json()).collect::<Vec<_>>(),
            }))
        }
        "pickRect" => {
            let view = view_arg(args)?;
            let min = vec2_arg(args, "min")?;
            let max = vec2_arg(args, "max")?;
            let slot = s.slot(handle)?;
            let ids =
                slot.preview
                    .pick_rect(slot.world.core(), slot.world.assets(), &view, min, max);
            Ok(json!({
                "ok": true,
                "handle": handle,
                "ids": ids.iter().map(|id| id.to_string()).collect::<Vec<_>>(),
            }))
        }
        "lines" => {
            let outline = match args.get("outline") {
                Some(o) if !o.is_null() => {
                    let color = match o.get("color") {
                        Some(c) if !c.is_null() => vec4_of(c, "outline.color")?,
                        _ => Vec4::new(1.0, 0.6, 0.0, 1.0),
                    };
                    Some((ids_arg(o, "ids")?, color))
                }
                _ => None,
            };
            let icons = match args.get("icons") {
                Some(i) if !i.is_null() => Some(f32_arg(i, "aspect")?),
                _ => None,
            };
            let slot = s.slot(handle)?;
            let mut lines = Vec::new();
            let selected = outline
                .as_ref()
                .map(|(ids, _)| ids.clone())
                .unwrap_or_default();
            if let Some((ids, color)) = outline {
                lines.extend(slot.preview.outline_lines(
                    slot.world.core(),
                    slot.world.assets(),
                    &ids,
                    color,
                ));
            }
            if let Some(aspect) = icons {
                lines.extend(slot.preview.icon_lines(
                    slot.world.core(),
                    slot.world.assets(),
                    aspect,
                    &selected,
                ));
            }
            Ok(json!({
                "ok": true,
                "handle": handle,
                "count": lines.len(),
                "lines": lines.iter().map(|l| l.json()).collect::<Vec<_>>(),
            }))
        }
        "worldMatrix" => {
            let ids = ids_arg(args, "ids")?;
            let slot = s.slot(handle)?;
            let mut matrices = serde_json::Map::new();
            for id in ids {
                let m = slot
                    .preview
                    .world_matrix(slot.world.core(), id)
                    .ok_or_else(|| {
                        one(Diagnostic::error(
                            Code::SESSION_INVALID_REQUEST,
                            format!("preview: entity {id} is not in the edit-preview world"),
                        ))
                    })?;
                matrices.insert(id.to_string(), json!(m.to_cols_array()));
            }
            Ok(json!({ "ok": true, "handle": handle, "matrices": matrices }))
        }
        "setTransforms" => {
            let list = args
                .get("transforms")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid("setTransforms: `transforms` must be an array"))?;
            let mut transforms = Vec::with_capacity(list.len());
            for t in list {
                transforms.push((uuid_of(t.get("id"), "transforms[].id")?, transform_of(t)?));
            }
            let slot = s.slot(handle)?;
            slot.preview
                .set_transforms(slot.world.core_mut(), &transforms)
                .map_err(one)?;
            Ok(json!({ "ok": true, "handle": handle, "count": transforms.len() }))
        }
        "restoreTransforms" => {
            let ids = ids_arg(args, "ids")?;
            let slot = s.slot(handle)?;
            slot.preview
                .restore_transforms(slot.world.core_mut(), &ids)
                .map_err(one)?;
            Ok(json!({ "ok": true, "handle": handle, "count": ids.len() }))
        }
        "setComponents" => {
            let list = args
                .get("components")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid("setComponents: `components` must be an array"))?;
            let mut components = Vec::with_capacity(list.len());
            for c in list {
                let type_id = c.get("type").and_then(Value::as_str).ok_or_else(|| {
                    invalid("setComponents: `components[].type` must be a string")
                })?;
                let data = c.get("data").filter(|d| d.is_object()).ok_or_else(|| {
                    invalid("setComponents: `components[].data` must be an object")
                })?;
                components.push((
                    uuid_of(c.get("id"), "components[].id")?,
                    type_id.to_string(),
                    data.clone(),
                ));
            }
            let slot = s.slot(handle)?;
            let count = slot
                .preview
                .set_components(slot.world.core_mut(), &components)
                .map_err(one)?;
            Ok(json!({ "ok": true, "handle": handle, "count": count }))
        }
        "restoreComponents" => {
            let ids = ids_arg(args, "ids")?;
            let slot = s.slot(handle)?;
            slot.preview
                .restore_components(slot.world.core_mut(), &ids)
                .map_err(one)?;
            Ok(json!({ "ok": true, "handle": handle, "count": ids.len() }))
        }
        "resync" => {
            let (described, anchors) = s.inner.describe_with_anchors(handle).map_err(one)?;
            let slot = s.slot(handle)?;
            let mut report = slot
                .preview
                .sync_world(&mut slot.world, &described.description)
                .json();
            slot.preview.set_anchors(anchors);
            report["ok"] = json!(true);
            report["handle"] = json!(handle);
            report["stats"] = slot.preview.stats(slot.world.core());
            Ok(report)
        }
        "reload" => {
            let (described, anchors) = s.inner.describe_with_anchors(handle).map_err(one)?;
            let slot = s.slot(handle)?;
            let (world, assets) = parts(&mut slot.world);
            slot.preview.reload(world, assets, &described.description);
            slot.preview.set_anchors(anchors);
            let mut diagnostics = described.diagnostics.clone();
            diagnostics.extend(slot.preview.diagnostics.iter().cloned());
            Ok(json!({
                "ok": true,
                "handle": handle,
                "stats": slot.preview.stats(slot.world.core()),
                "diagnostics": diags(&diagnostics),
            }))
        }
        "recook" => {
            s.slot(handle)?;
            let live = args.get("live").and_then(Value::as_bool).unwrap_or(false);
            let (assets, cook) = s.cook(live)?;
            let mut previews = Vec::with_capacity(s.previews.len());
            for (h, slot) in s.previews.iter_mut() {
                let mut entry = json!({ "handle": *h });
                match s.inner.describe_with_anchors(*h) {
                    Ok((described, anchors)) => {
                        slot.preview.reload(
                            slot.world.core_mut(),
                            Some(&assets),
                            &described.description,
                        );
                        slot.preview.set_anchors(anchors);
                        entry["stats"] = slot.preview.stats(slot.world.core());
                        entry["diagnostics"] = diags(&slot.preview.diagnostics);
                    }
                    Err(d) => entry["diagnostics"] = json!([diagnostic_json(&d)]),
                }
                previews.push(entry);
            }
            Ok(json!({ "ok": true, "handle": handle, "cook": cook, "previews": previews }))
        }
        "dump" => {
            let slot = s.slot(handle)?;
            Ok(
                json!({ "ok": true, "handle": handle, "dump": slot.preview.dump(slot.world.core()) }),
            )
        }
        "stats" => {
            let slot = s.slot(handle)?;
            Ok(json!({
                "ok": true,
                "handle": handle,
                "stats": slot.preview.stats(slot.world.core()),
                "diagnostics": diags(&slot.preview.diagnostics),
            }))
        }
        other => Err(one(Diagnostic::error(
            Code::SESSION_UNKNOWN_QUERY,
            format!("preview: unknown call `{other}`"),
        )
        .with_suggestion(format!("one of: {}", PREVIEW_KINDS.join(", "))))),
    }
}

// ---- JSON arguments of the preview calls

fn floats_of(v: &Value, what: &str, n: usize) -> Result<Vec<f32>, Vec<Diagnostic>> {
    let items = v
        .as_array()
        .filter(|a| a.len() == n)
        .ok_or_else(|| invalid(format!("preview: `{what}` must be an array of {n} numbers")))?;
    items
        .iter()
        .map(|x| {
            x.as_f64()
                .map(|f| f as f32)
                .filter(|f| f.is_finite())
                .ok_or_else(|| invalid(format!("preview: `{what}` must hold finite numbers")))
        })
        .collect()
}

fn field<'a>(args: &'a Value, name: &str) -> Result<&'a Value, Vec<Diagnostic>> {
    args.get(name)
        .filter(|v| !v.is_null())
        .ok_or_else(|| invalid(format!("preview: `{name}` is required")))
}

fn f32_arg(args: &Value, name: &str) -> Result<f32, Vec<Diagnostic>> {
    field(args, name)?
        .as_f64()
        .map(|f| f as f32)
        .filter(|f| f.is_finite())
        .ok_or_else(|| invalid(format!("preview: `{name}` must be a finite number")))
}

fn vec2_arg(args: &Value, name: &str) -> Result<Vec2, Vec<Diagnostic>> {
    let f = floats_of(field(args, name)?, name, 2)?;
    Ok(Vec2::new(f[0], f[1]))
}

fn vec3_arg(args: &Value, name: &str) -> Result<Vec3, Vec<Diagnostic>> {
    let f = floats_of(field(args, name)?, name, 3)?;
    Ok(Vec3::new(f[0], f[1], f[2]))
}

fn vec4_of(v: &Value, what: &str) -> Result<Vec4, Vec<Diagnostic>> {
    let f = floats_of(v, what, 4)?;
    Ok(Vec4::new(f[0], f[1], f[2], f[3]))
}

/// A column-major 4×4 (`Mat4::to_cols_array` order).
fn mat4_arg(args: &Value, name: &str) -> Result<Mat4, Vec<Diagnostic>> {
    let f = floats_of(field(args, name)?, name, 16)?;
    Ok(Mat4::from_cols_array(&f.try_into().expect("16 floats")))
}

fn uuid_of(v: Option<&Value>, what: &str) -> Result<uuid::Uuid, Vec<Diagnostic>> {
    v.and_then(Value::as_str)
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .ok_or_else(|| invalid(format!("preview: `{what}` must be a UUID string")))
}

fn ids_arg(args: &Value, name: &str) -> Result<Vec<uuid::Uuid>, Vec<Diagnostic>> {
    field(args, name)?
        .as_array()
        .ok_or_else(|| {
            invalid(format!(
                "preview: `{name}` must be an array of UUID strings"
            ))
        })?
        .iter()
        .map(|v| uuid_of(Some(v), name))
        .collect()
}

/// The editor camera: `projection` ("perspective" | "orthographic"),
/// `fovY` (radians), `orthoHeight`, `near`, `far` — each defaulting to
/// `Camera::default()`'s.
fn camera_arg(args: &Value) -> Result<Camera, Vec<Diagnostic>> {
    let mut camera = Camera::default();
    if let Some(p) = args.get("projection").filter(|v| !v.is_null()) {
        camera.projection = match p.as_str() {
            Some("perspective") => Projection::Perspective,
            Some("orthographic") => Projection::Orthographic,
            _ => {
                return Err(invalid(
                    "preview: `projection` must be 'perspective' or 'orthographic'",
                ))
            }
        };
    }
    if let Some(m) = args.get("exposureMode").filter(|v| !v.is_null()) {
        camera.exposure_mode = match m.as_str() {
            Some("auto") => ExposureMode::Auto,
            Some("manual") => ExposureMode::Manual,
            _ => {
                return Err(invalid(
                    "preview: `exposureMode` must be 'auto' or 'manual'",
                ))
            }
        };
    }
    for (name, slot) in [
        ("fovY", &mut camera.fov_y),
        ("orthoHeight", &mut camera.ortho_height),
        ("near", &mut camera.near),
        ("far", &mut camera.far),
        ("exposure", &mut camera.exposure),
    ] {
        if args.get(name).is_some_and(|v| !v.is_null()) {
            *slot = f32_arg(args, name)?;
        }
    }
    if camera.near <= 0.0
        || camera.far <= camera.near
        || camera.fov_y <= 0.0
        || camera.ortho_height <= 0.0
    {
        return Err(invalid(
            "preview: the camera needs 0 < near < far, fovY > 0 and orthoHeight > 0",
        ));
    }
    Ok(camera)
}

/// `view`: the camera (as `camera`) plus `aspect`.
fn view_arg(args: &Value) -> Result<View, Vec<Diagnostic>> {
    let view = field(args, "view")?;
    Ok(View {
        world: mat4_arg(view, "world")?,
        camera: camera_arg(view)?,
        aspect: f32_arg(view, "aspect")?,
    })
}

fn transform_of(v: &Value) -> Result<Transform, Vec<Diagnostic>> {
    let rotation = floats_of(field(v, "rotation")?, "transforms[].rotation", 4)?;
    let rotation = Quat::from_xyzw(rotation[0], rotation[1], rotation[2], rotation[3]);
    if !rotation.is_normalized() {
        return Err(invalid(
            "preview: `transforms[].rotation` must be a unit quaternion [x, y, z, w]",
        ));
    }
    Ok(Transform {
        position: vec3_arg(v, "position")?,
        rotation,
        scale: vec3_arg(v, "scale")?,
    })
}

// ------------------------------------------------------------ live worlds

/// A live call on a running game's world (ADR-0021 P7, live Play mode;
/// [`fynkel_authoring::live`]): `entities`, `entity`, `set`, `remove`,
/// `rename`, `parent`, `destroy`, `spawn`, and the Scene view's `pick`, `pickRect`, `lines` and
/// `worldMatrix` (meshes read from the cache this world loaded). No
/// session: the world is the game's, and the call reads it
/// and changes it in the scene document's terms, writing at once without
/// a tick. `out_view` gets the JSON result, `{ "ok": true, ... }` or
/// `{ "ok": false, "rejected" | "diagnostics": [...] }`, valid until the
/// next query or mutating call on the world.
///
/// # Safety
/// `world` live or NULL; `kind_utf8` readable for `kind_len` bytes;
/// `args_json_utf8` readable for `args_len` bytes, or NULL with `args_len`
/// 0 for no arguments; `out_view` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_world_live_call(
    world: *mut FynkelWorld,
    kind_utf8: *const u8,
    kind_len: u32,
    args_json_utf8: *const u8,
    args_len: u32,
    out_view: *mut FynkelBufferView,
) -> Status {
    let parsed = kind_and_args("live", kind_utf8, kind_len, args_json_utf8, args_len);
    let status = crate::guarded(world, |w| {
        w.stats.query_calls += 1;
        let (value, status) = match parsed.and_then(|(kind, args)| {
            fynkel_authoring::live::call_with(&mut w.inner, w.assets.as_ref(), &kind, &args)
        }) {
            Ok(mut v) => {
                let refused = v.get("ok") == Some(&Value::Bool(false));
                if v.get("ok").is_none() {
                    v["ok"] = Value::Bool(true);
                }
                let status = if refused {
                    Status::CommandsRejected
                } else {
                    Status::Ok
                };
                (v, status)
            }
            Err(diagnostics) => (
                json!({
                    "ok": false,
                    "diagnostics": diagnostics.iter().map(diagnostic_json).collect::<Vec<_>>(),
                }),
                status_for(&diagnostics),
            ),
        };
        w.query_buf.clear();
        serde_json::to_writer(&mut w.query_buf, &value).expect("JSON into a Vec");
        w.stats.bytes_out += w.query_buf.len() as u64;
        status
    });
    if let Some(out) = out_view.as_mut() {
        *out = match (status, world.as_ref()) {
            (Status::Ok | Status::CommandsRejected | Status::InvalidArgument, Some(w)) => {
                FynkelBufferView::of(&w.query_buf)
            }
            _ => FynkelBufferView::empty(),
        };
    }
    status
}

/// # Safety
/// `session` live or NULL (an empty view).
#[no_mangle]
pub unsafe extern "C" fn fynkel_authoring_result(
    session: *const FynkelAuthoringSession,
) -> FynkelBufferView {
    match session.as_ref() {
        Some(s) => FynkelBufferView::of(&s.result),
        None => FynkelBufferView::empty(),
    }
}

/// # Safety
/// `session` live or NULL (0).
#[no_mangle]
pub unsafe extern "C" fn fynkel_authoring_is_poisoned(
    session: *const FynkelAuthoringSession,
) -> i32 {
    match session.as_ref() {
        Some(s) => s.poisoned as i32,
        None => 0,
    }
}

/// The project directory of the open project, for hosts (tests) that hold
/// only the pointer.
pub fn project_dir(session: &FynkelAuthoringSession) -> Option<&Path> {
    session.inner.project().map(|p| p.dir.as_path())
}
