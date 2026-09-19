//! Document migrations (blueprint §19.2c item 9, §6): a document whose
//! `formatVersion` is behind the current one is brought up to date by an
//! ordered chain of per-kind steps, each `from → from + 1`. A migration is
//! a pure function on the JSON tree; the framework sets `formatVersion`,
//! validates the result, produces a unified diff for `--dry-run`, and
//! writes with a `.bak` copy of the original. Newer documents are never
//! touched (`FYN-D0013`).

use std::path::{Path, PathBuf};

use fynkel_contracts::{Code, Diagnostic};

use crate::json::{Member, Node};
use crate::{Document, DocumentKind, SchemaSet};

/// One step: `kind` documents at `from` become `from + 1`.
#[derive(Clone)]
pub struct Migration {
    pub kind: DocumentKind,
    pub from: u32,
    /// Short stable name, reported in `applied[]` (`scene/0-1/entities-keyed-by-id`).
    pub name: &'static str,
    pub description: &'static str,
    /// Rewrites the body in place. `formatVersion` is set by the framework.
    pub apply: fn(&mut Node) -> Result<(), String>,
}

impl Migration {
    pub fn to(&self) -> u32 {
        self.from + 1
    }

    pub fn id(&self) -> String {
        format!(
            "{}/{}-{}/{}",
            self.kind.name(),
            self.from,
            self.to(),
            self.name
        )
    }
}

/// The registry: every known step, looked up by kind and version.
#[derive(Clone, Default)]
pub struct Migrations {
    steps: Vec<Migration>,
}

impl Migrations {
    pub fn empty() -> Migrations {
        Migrations::default()
    }

    /// The steps shipped with this build.
    pub fn builtin() -> Migrations {
        let mut m = Migrations::empty();
        m.add(Migration {
            kind: DocumentKind::Scene,
            from: 0,
            name: "entities-keyed-by-id",
            description: "entities and components were arrays carrying their id; they become objects keyed by it (order kept)",
            apply: scene_0_to_1,
        });
        m
    }

    /// Registers a step; a second step for the same kind and version replaces the first.
    pub fn add(&mut self, step: Migration) {
        self.steps
            .retain(|s| !(s.kind == step.kind && s.from == step.from));
        self.steps.push(step);
        self.steps.sort_by_key(|s| (s.kind.name(), s.from));
    }

    pub fn steps(&self) -> &[Migration] {
        &self.steps
    }

    pub fn step(&self, kind: DocumentKind, from: u32) -> Option<&Migration> {
        self.steps.iter().find(|s| s.kind == kind && s.from == from)
    }

    /// The chain from `from` to `to`, or the first version with no step.
    pub fn path(&self, kind: DocumentKind, from: u32, to: u32) -> Result<Vec<&Migration>, u32> {
        let mut out = Vec::new();
        let mut v = from;
        while v < to {
            match self.step(kind, v) {
                Some(s) => {
                    out.push(s);
                    v = s.to();
                }
                None => return Err(v),
            }
        }
        Ok(out)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Applied {
    pub from: u32,
    pub to: u32,
    pub name: String,
    pub description: String,
}

/// The outcome of [`migrate`]: the (possibly unchanged) document, the steps
/// taken, a unified diff of the canonical text, and diagnostics — an error
/// among them means `document` is the original, untouched.
#[derive(Clone, Debug)]
pub struct Migrated {
    pub document: Document,
    pub from: u32,
    pub to: u32,
    pub applied: Vec<Applied>,
    pub diff: String,
    pub diagnostics: Vec<Diagnostic>,
}

impl Migrated {
    pub fn changed(&self) -> bool {
        !self.applied.is_empty()
    }

    pub fn has_errors(&self) -> bool {
        fynkel_contracts::has_errors(&self.diagnostics)
    }

    /// Writes the migrated document over `path`, after copying the original
    /// to `<path>.bak` (replacing an older backup). Refuses when nothing
    /// changed or the migration has errors. Returns the backup path.
    pub fn write_with_backup(&self, path: &Path) -> Result<PathBuf, Diagnostic> {
        let shown = path.display().to_string();
        if self.has_errors() {
            return Err(Diagnostic::error(
                Code::MIGRATION_FAILED,
                "not written: the migration reported errors",
            )
            .with_document(shown));
        }
        if !self.changed() {
            return Err(Diagnostic::error(
                Code::MIGRATION_FAILED,
                "not written: the document is already at the current version",
            )
            .with_document(shown));
        }
        let text = self.document.write()?;
        let backup = backup_path(path);
        std::fs::copy(path, &backup).map_err(|e| {
            Diagnostic::error(
                Code::IO,
                format!("cannot back up to {}: {e}", backup.display()),
            )
            .with_document(shown.clone())
        })?;
        std::fs::write(path, text).map_err(|e| {
            Diagnostic::error(Code::IO, format!("cannot write: {e}")).with_document(shown)
        })?;
        Ok(backup)
    }
}

/// `<file>.bak` next to the file.
pub fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".bak");
    path.with_file_name(name)
}

/// Brings `doc` to its kind's current `formatVersion`. The result is
/// validated against `schemas`; a step that fails, a missing step, or an
/// invalid result leaves `document` as the original and reports why.
pub fn migrate(doc: &Document, migrations: &Migrations, schemas: &SchemaSet) -> Migrated {
    let from = doc.envelope.format_version;
    let to = doc.kind.supported_format_version();
    let shown = doc.path.clone();
    let with_doc = |d: Diagnostic| match &shown {
        Some(p) if d.document.is_none() => d.with_document(p.clone()),
        _ => d,
    };
    let unchanged = |diagnostics: Vec<Diagnostic>| Migrated {
        document: doc.clone(),
        from,
        to,
        applied: Vec::new(),
        diff: String::new(),
        diagnostics,
    };

    if from > to {
        return unchanged(vec![with_doc(
            anchor(
                doc,
                Diagnostic::error(
                    Code::UNSUPPORTED_FORMAT_VERSION,
                    format!(
                        "formatVersion {from} is newer than the supported {to}; the document is read-only"
                    ),
                )
                .with_suggestion("update the Fynkel tools"),
                "/formatVersion",
            ),
        )]);
    }
    if from == to {
        return unchanged(Vec::new());
    }
    let chain = match migrations.path(doc.kind, from, to) {
        Ok(chain) => chain,
        Err(stuck) => {
            return unchanged(vec![with_doc(anchor(
                doc,
                Diagnostic::error(
                    Code::NO_MIGRATION_PATH,
                    format!(
                        "no migration from {} formatVersion {stuck} to {}",
                        doc.kind.name(),
                        if stuck + 1 == to {
                            to.to_string()
                        } else {
                            format!("{} (needed on the way to {to})", stuck + 1)
                        }
                    ),
                )
                .with_suggestion("the document may predate this tool's oldest supported version"),
                "/formatVersion",
            ))]);
        }
    };

    let before = match doc.write() {
        Ok(t) => t,
        Err(d) => return unchanged(vec![with_doc(d)]),
    };
    let mut out = doc.clone();
    let mut applied = Vec::new();
    for step in chain {
        if let Err(msg) = (step.apply)(&mut out.root) {
            return unchanged(vec![with_doc(
                Diagnostic::error(
                    Code::MIGRATION_FAILED,
                    format!("{} failed: {msg}", step.id()),
                )
                .with_pointer("")
                .with_suggestion(
                    "fix the document by hand at the reported place, then migrate again",
                ),
            )]);
        }
        out.root
            .set("formatVersion", Node::integer(step.to() as i64));
        out.envelope.format_version = step.to();
        applied.push(Applied {
            from: step.from,
            to: step.to(),
            name: step.id(),
            description: step.description.to_string(),
        });
    }

    // A migration must produce a document the current schema accepts.
    let diagnostics: Vec<Diagnostic> = out.validate(schemas);
    if fynkel_contracts::has_errors(&diagnostics) {
        let mut diags = vec![with_doc(
            Diagnostic::error(
                Code::MIGRATION_FAILED,
                format!(
                    "the migrated document does not validate against {}; nothing written",
                    doc.kind.schema_file()
                ),
            )
            .with_pointer(""),
        )];
        diags.extend(diagnostics);
        return unchanged(diags);
    }
    let after = match out.write() {
        Ok(t) => t,
        Err(d) => return unchanged(vec![with_doc(d)]),
    };
    let label = shown.clone().unwrap_or_else(|| "document".to_string());
    let diff = similar::TextDiff::from_lines(&before, &after)
        .unified_diff()
        .context_radius(3)
        .header(
            &format!("{label} (formatVersion {from})"),
            &format!("{label} (formatVersion {to})"),
        )
        .to_string();
    Migrated {
        document: out,
        from,
        to,
        applied,
        diff,
        diagnostics,
    }
}

fn anchor(doc: &Document, d: Diagnostic, pointer: &str) -> Diagnostic {
    let d = d.with_pointer(pointer);
    match doc.root.pointer_span(pointer) {
        Some(s) => d.at(s.start.line, s.start.column),
        None => d,
    }
}

/// `migrate --json` shape for one document.
pub fn migrated_json(m: &Migrated) -> serde_json::Value {
    serde_json::json!({
        "kind": m.document.kind.name(),
        "from": m.from,
        "to": m.to,
        "applied": m.applied.iter().map(|a| serde_json::json!({
            "from": a.from, "to": a.to, "name": a.name, "description": a.description,
        })).collect::<Vec<_>>(),
        "diff": m.diff,
    })
}

// ---------------------------------------------------------------------------
// Built-in steps
// ---------------------------------------------------------------------------

/// Scene 0 → 1: `entities` (and each entity's `components`) were arrays of
/// objects carrying their own `id`; they become objects keyed by that id,
/// in the same order, with the `id` member dropped. Anything else is kept.
fn scene_0_to_1(root: &mut Node) -> Result<(), String> {
    let entities = root.get_mut("entities").ok_or("entities is missing")?;
    if entities.as_object().is_some() {
        return Ok(()); // already keyed (a v0 writer that got ahead of itself)
    }
    let items = entities
        .as_array()
        .ok_or("/entities: expected an array")?
        .to_vec();
    let mut keyed = Vec::with_capacity(items.len());
    for (i, item) in items.into_iter().enumerate() {
        let (id, mut entity) = take_id(item, &format!("/entities/{i}"))?;
        if let Some(components) = entity.get_mut("components") {
            if let Some(list) = components.as_array().map(<[Node]>::to_vec) {
                let mut by_id = Vec::with_capacity(list.len());
                for (j, c) in list.into_iter().enumerate() {
                    let (cid, component) = take_id(c, &format!("/entities/{i}/components/{j}"))?;
                    by_id.push((cid, component));
                }
                *components = Node::object(by_id);
            }
        }
        keyed.push((id, entity));
    }
    *entities = Node::object(keyed);
    Ok(())
}

/// Splits `{ "id": "<uuid>", ...rest }` into the id and the rest.
fn take_id(node: Node, at: &str) -> Result<(String, Node), String> {
    let members: Vec<Member> = node
        .as_object()
        .ok_or_else(|| format!("{at}: expected an object"))?
        .to_vec();
    let mut id = None;
    let mut rest = Vec::with_capacity(members.len());
    for m in members {
        if m.key == "id" {
            id = Some(
                m.value
                    .as_str()
                    .ok_or_else(|| format!("{at}/id: expected a string"))?
                    .to_string(),
            );
        } else {
            rest.push((m.key, m.value));
        }
    }
    let id = id.ok_or_else(|| format!("{at}: id is missing"))?;
    crate::parse_uuid(&id).map_err(|e| format!("{at}/id: {e}"))?;
    Ok((id, Node::object(rest)))
}
