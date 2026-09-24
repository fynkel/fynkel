//! The authoring log (blueprint §19.2e item 10): every change the session
//! makes to a project, one JSON line each, appended to
//! `<project>/.fynkel/authoring.log` (git-ignored with the rest of
//! `.fynkel/`). It is the record the M4 gate reads — "every mutation of
//! that session is in the authoring log as commands" — and what a
//! developer or an agent reads to see what an editing session did.
//!
//! A line is `{ "v": 1, "at": <unix ms>, "session": <uuid>, "op": … }`:
//!
//! - `open` — the project was opened; `session` is new from here on.
//! - `apply` — a committed transaction: `document` (relative to the
//!   project), `transactionId`, `baseRevision`, `revision` and `commands`,
//!   each in the JSON form `fynkel scene apply` reads, so a session can be
//!   replayed with the CLI, and `affectedIds` (per command: what it
//!   touched or created — the ids a `createObject` without one was given).
//!   A refused transaction changed nothing and is not written.
//! - `undo` / `redo` — `document`, the `transactionId` replayed and its
//!   `commands` by name.
//! - `save` — the `document` written (Save As: its new path).
//! - `asset` — an asset operation (`kind`, `args`) that succeeded.
//!
//! Writing the log never fails an edit: a line that cannot be written is
//! dropped (the edit itself is in the document and its undo history).

use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use uuid::Uuid;

/// Where the log is, relative to the project directory.
pub const LOG_FILE: &str = ".fynkel/authoring.log";

/// Appends `entry` (an object with its `op`) to the project's log.
pub(crate) fn append(dir: &Path, session: Uuid, entry: Value) {
    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let mut line = json!({ "v": 1, "at": at, "session": session.to_string() });
    if let (Some(line), Value::Object(entry)) = (line.as_object_mut(), entry) {
        line.extend(entry);
    }
    let path = dir.join(LOG_FILE);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(file, "{line}");
    }
}
