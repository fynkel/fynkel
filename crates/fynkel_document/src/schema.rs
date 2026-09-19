//! JSON Schema 2020-12 validation (§6) over the schemas in a `schemas/`
//! directory: the engine's own checkout or a project's copy. Every
//! `*.schema.json` under the directory is registered by its `$id` so
//! `$ref`s between them resolve locally; nothing is fetched.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use fynkel_contracts::{Code, Diagnostic, Severity};
use jsonschema::{Draft, Registry, Resource, ValidationError, Validator};

use crate::json::Node;
use crate::DocumentKind;

/// The shared envelope schema, embedded so validation works before any
/// `schemas/` directory exists (and as the fallback for kinds without a
/// generated schema yet).
pub const ENVELOPE_SCHEMA: &str = include_str!("../../../schemas/envelope.schema.json");

pub struct SchemaSet {
    validators: HashMap<DocumentKind, Validator>,
    envelope: Validator,
    /// Kinds that fell back to the envelope schema.
    missing: Vec<DocumentKind>,
}

impl SchemaSet {
    /// Envelope only: no `schemas/` directory involved.
    pub fn embedded() -> SchemaSet {
        SchemaSet::build(Vec::new()).expect("embedded schema is valid")
    }

    /// Loads every `*.schema.json` under `dir` (recursively).
    pub fn load(dir: &Path) -> Result<SchemaSet, Diagnostic> {
        let mut files = Vec::new();
        collect(dir, &mut files).map_err(|e| {
            Diagnostic::error(
                Code::SCHEMA_UNAVAILABLE,
                format!("cannot read schemas: {e}"),
            )
            .with_document(dir.display().to_string())
        })?;
        files.sort();
        let mut loaded = Vec::new();
        for path in files {
            let text = std::fs::read_to_string(&path).map_err(|e| {
                Diagnostic::error(Code::SCHEMA_UNAVAILABLE, format!("cannot read schema: {e}"))
                    .with_document(path.display().to_string())
            })?;
            let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
                Diagnostic::error(Code::SCHEMA_UNAVAILABLE, format!("schema is not JSON: {e}"))
                    .with_document(path.display().to_string())
                    .at(e.line() as u32, e.column() as u32)
            })?;
            loaded.push((path, value));
        }
        SchemaSet::build(loaded)
    }

    fn build(loaded: Vec<(PathBuf, serde_json::Value)>) -> Result<SchemaSet, Diagnostic> {
        let envelope_value: serde_json::Value =
            serde_json::from_str(ENVELOPE_SCHEMA).expect("embedded envelope schema");
        let envelope_id = DocumentKind::Scene
            .schema_id()
            .replace("scene.schema.json", "envelope.schema.json");

        let mut by_id: Vec<(String, serde_json::Value)> =
            vec![(envelope_id.clone(), envelope_value.clone())];
        for (path, value) in &loaded {
            let id = value
                .get("$id")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    format!(
                        "https://fynkel.dev/schemas/{}",
                        path.file_name().unwrap().to_string_lossy()
                    )
                });
            if id == envelope_id {
                by_id[0].1 = value.clone();
            } else {
                by_id.push((id, value.clone()));
            }
        }

        let mut builder = Registry::new().draft(Draft::Draft202012);
        for (id, value) in &by_id {
            builder = builder
                .add(id, Resource::from_contents(value.clone()))
                .map_err(|e| {
                    Diagnostic::error(Code::SCHEMA_UNAVAILABLE, format!("bad schema id {id}: {e}"))
                })?;
        }
        let registry = builder.prepare().map_err(|e| {
            Diagnostic::error(Code::SCHEMA_UNAVAILABLE, format!("schema registry: {e}"))
        })?;

        let compile = |value: &serde_json::Value, id: &str| -> Result<Validator, Diagnostic> {
            jsonschema::draft202012::options()
                .with_registry(&registry)
                .should_validate_formats(true)
                .build(value)
                .map_err(|e| {
                    Diagnostic::error(
                        Code::SCHEMA_UNAVAILABLE,
                        format!("schema {id} does not compile: {e}"),
                    )
                })
        };

        let envelope = compile(&by_id[0].1, &envelope_id)?;
        let mut validators = HashMap::new();
        let mut missing = Vec::new();
        for kind in DocumentKind::ALL {
            let id = kind.schema_id();
            match by_id.iter().find(|(i, _)| *i == id) {
                Some((_, value)) => {
                    validators.insert(kind, compile(value, &id)?);
                }
                None => missing.push(kind),
            }
        }
        Ok(SchemaSet {
            validators,
            envelope,
            missing,
        })
    }

    /// Kinds without their own schema (validated against the envelope only).
    pub fn missing_kinds(&self) -> &[DocumentKind] {
        &self.missing
    }

    /// Validates a document tree against the kind's schema (or the envelope
    /// schema when the kind has none, with a warning saying so).
    pub fn validate(&self, kind: DocumentKind, root: &Node) -> Vec<Diagnostic> {
        let instance = root.to_serde();
        let mut out = Vec::new();
        let validator = match self.validators.get(&kind) {
            Some(v) => v,
            None => {
                out.push(
                    Diagnostic::new(
                        Code::SCHEMA_UNAVAILABLE,
                        Severity::Warning,
                        format!(
                            "no {} is loaded; only the envelope was validated",
                            kind.schema_file()
                        ),
                    )
                    .with_pointer(""),
                );
                &self.envelope
            }
        };
        for err in validator.iter_errors(&instance) {
            out.extend(to_diagnostics(&err, root));
        }
        out
    }
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(&path, out)?;
        } else if path.to_string_lossy().ends_with(".schema.json") {
            out.push(path);
        }
    }
    Ok(())
}

fn to_diagnostics(err: &ValidationError<'_>, root: &Node) -> Vec<Diagnostic> {
    let base = err.instance_path().as_str().to_string();
    let anchor = |pointer: &str, d: Diagnostic| -> Diagnostic {
        let d = d.with_pointer(pointer);
        match root.pointer_span(pointer) {
            Some(s) => d.at(s.start.line, s.start.column),
            None => d,
        }
    };
    match err.kind() {
        // One diagnostic per unexpected property, anchored at its key.
        jsonschema::error::ValidationErrorKind::AdditionalProperties { unexpected }
        | jsonschema::error::ValidationErrorKind::UnevaluatedProperties { unexpected } => {
            unexpected
                .iter()
                .map(|key| {
                    let escaped = key.replace('~', "~0").replace('/', "~1");
                    anchor(
                        &format!("{base}/{escaped}"),
                        Diagnostic::error(
                            Code::SCHEMA_VIOLATION,
                            format!("unexpected property {key:?}"),
                        )
                        .with_suggestion("remove it or check the spelling"),
                    )
                })
                .collect()
        }
        _ => vec![anchor(
            &base,
            Diagnostic::error(Code::SCHEMA_VIOLATION, err.to_string())
                .with_suggestion(format!("schema rule {}", err.schema_path())),
        )],
    }
}
