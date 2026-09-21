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

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use fynkel_authoring::{AuthoringSession, Handle};
use fynkel_contracts::{Code, Diagnostic, Status};
use fynkel_document::diagnostic_json;
use serde_json::{json, Value};

use crate::FynkelBufferView;

/// Version of `fynkel_authoring.h`. Bumped on any incompatible change;
/// independent of `FYNKEL_ABI_VERSION`.
pub const AUTHORING_ABI_VERSION: u32 = 1;

/// One editor session: the open project, its documents, and the last
/// result. Opaque to the host.
pub struct FynkelAuthoringSession {
    inner: AuthoringSession,
    result: Vec<u8>,
    poisoned: bool,
}

impl FynkelAuthoringSession {
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
    match catch_unwind(AssertUnwindSafe(|| f(&mut s.inner))) {
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
    guarded(session, move |s| {
        let project = s.open_project(&dir)?;
        Ok(json!({ "ok": true, "project": project.json() }))
    })
}

/// # Safety
/// `session` live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_authoring_close_project(
    session: *mut FynkelAuthoringSession,
) -> Status {
    guarded(session, |s| {
        s.close_project();
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
    guarded(session, move |s| {
        s.close_document(handle as Handle).map_err(one)?;
        Ok(json!({ "ok": true, "handle": handle }))
    })
}

/// # Safety
/// `session` live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_authoring_reload_document(
    session: *mut FynkelAuthoringSession,
    handle: u32,
) -> Status {
    guarded(session, move |s| {
        s.reload(handle as Handle)?;
        Ok(json!({ "ok": true, "state": s.state_json(handle as Handle).map_err(one)? }))
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
    guarded(session, move |s| s.apply_json(handle as Handle, &request))
}

/// # Safety
/// `session` live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_authoring_undo(
    session: *mut FynkelAuthoringSession,
    handle: u32,
) -> Status {
    guarded(session, move |s| {
        let t = s.undo(handle as Handle).map_err(one)?;
        s.transaction_json(handle as Handle, &t).map_err(one)
    })
}

/// # Safety
/// `session` live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_authoring_redo(
    session: *mut FynkelAuthoringSession,
    handle: u32,
) -> Status {
    guarded(session, move |s| {
        let t = s.redo(handle as Handle).map_err(one)?;
        s.transaction_json(handle as Handle, &t).map_err(one)
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
    guarded(session, move |s| {
        let written = s.save(handle as Handle, path.as_deref()).map_err(one)?;
        Ok(json!({
            "ok": true,
            "path": written.display().to_string(),
            "state": s.state_json(handle as Handle).map_err(one)?,
        }))
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
    guarded(session, move |s| {
        let scan = s.rescan(&paths).map_err(one)?;
        let project = s.project().expect("rescan needs a project");
        Ok(json!({
            "ok": true,
            "assetCount": project.assets.len(),
            "scanned": paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            "diagnostics": scan.iter().map(diagnostic_json).collect::<Vec<_>>(),
        }))
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
    let Some(kind) = utf8(kind_utf8, kind_len) else {
        return refused(session, invalid("query: kind must be UTF-8 text"));
    };
    let kind = kind.to_string();
    let args: Value = if args_json_utf8.is_null() && args_len == 0 {
        json!({})
    } else {
        let Some(text) = utf8(args_json_utf8, args_len) else {
            return refused(session, invalid("query: args must be UTF-8 JSON or NULL"));
        };
        match serde_json::from_str::<Value>(text) {
            Ok(v @ Value::Object(_)) => v,
            Ok(Value::Null) => json!({}),
            Ok(_) => return refused(session, invalid("query: args must be a JSON object")),
            Err(e) => return refused(session, invalid(format!("query: args are not JSON: {e}"))),
        }
    };
    guarded(session, move |s| {
        let result = s.query(&kind, &args)?;
        Ok(json!({ "ok": true, "kind": kind, "result": result }))
    })
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
