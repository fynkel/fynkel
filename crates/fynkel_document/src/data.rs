//! Data assets (ADR-0027, `docs/documents.md` § Data assets): a
//! `<name>.data.json` holding one instance of a game-declared type —
//! `{ type, version, data }`, the shape a component instance has. The
//! engine owns the kind; the type is a Dart class (`@FynkelDataAsset`)
//! whose schema `fynkel codegen` writes to `schemas/data/<type>.schema.json`
//! and [`SchemaSet`] loads by type id. The cook checks the document and
//! writes it as the generic JSON chunk; the SDK makes the instance.
//!
//! Like [`crate::volume`], the view is lenient: a broken document still
//! describes itself, and an envelope field that does not read is
//! `FYN-D1000`. A type no schema or registry entry declares is the
//! warning `FYN-D1001` (its data is kept as-is); data that does not fit
//! the type's schema, or was written for another version of it, is
//! `FYN-D1002`.

use fynkel_contracts::{Code, Diagnostic, Severity};
use uuid::Uuid;

use crate::json::{Locator, Node};
use crate::schema::SchemaSet;
use crate::{Document, DocumentKind};

/// The type `Document::new(DocumentKind::Data, …)` writes until one is
/// named ([`Document::new_data`]).
pub const PLACEHOLDER_TYPE: &str = "game.Data";

#[derive(Clone, Debug)]
pub struct DataDocument {
    pub document: Document,
    /// The type id, or empty when it does not read.
    pub type_id: String,
    pub version: u32,
    read_diagnostics: Vec<Diagnostic>,
}

impl DataDocument {
    pub fn from_document(document: Document) -> DataDocument {
        let mut diags = Vec::new();
        if document.kind != DocumentKind::Data {
            diags.push(
                Diagnostic::error(
                    Code::UNKNOWN_DOCUMENT_KIND,
                    format!("expected a data asset, got {}", document.kind.name()),
                )
                .with_pointer(""),
            );
        }
        let root = &document.root;
        let at = Locator::new(root);
        let mut malformed = |field: &str, what: &str| {
            diags.push(at.anchor(
                Diagnostic::error(
                    Code::DATA_MALFORMED_ENTRY,
                    format!("{field} must be {what}"),
                ),
                &format!("/{field}"),
            ));
        };
        let type_id = match root.get("type").and_then(Node::as_str) {
            Some(t) if is_type_id(t) => t.to_string(),
            _ => {
                malformed("type", "a namespaced type id such as game.Weapon");
                String::new()
            }
        };
        let version = match root.get("version").map(Node::as_i64) {
            None => 1,
            Some(Some(v)) if v >= 1 => v as u32,
            Some(_) => {
                malformed("version", "an integer >= 1");
                1
            }
        };
        if root.get("data").and_then(Node::as_object).is_none() {
            malformed("data", "an object of the type's fields");
        }
        DataDocument {
            document,
            type_id,
            version,
            read_diagnostics: diags,
        }
    }

    pub fn id(&self) -> Uuid {
        self.document.envelope.id
    }

    pub fn name(&self) -> &str {
        &self.document.envelope.name
    }

    /// The fields, or `None` when `data` is not an object.
    pub fn data(&self) -> Option<&Node> {
        self.document
            .root
            .get("data")
            .filter(|d| d.as_object().is_some())
    }

    pub fn read_diagnostics(&self) -> &[Diagnostic] {
        &self.read_diagnostics
    }

    /// Envelope + JSON Schema, the read problems the schema did not already
    /// report, then the data against its type.
    pub fn validate(&self, schemas: &SchemaSet) -> Vec<Diagnostic> {
        let mut out = self.document.validate(schemas);
        let schema_pointers: Vec<String> = out.iter().filter_map(|d| d.pointer.clone()).collect();
        for d in &self.read_diagnostics {
            if d.code == Code::DATA_MALFORMED_ENTRY
                && d.pointer
                    .as_ref()
                    .is_some_and(|p| schema_pointers.contains(p))
            {
                continue;
            }
            out.push(d.clone());
        }
        let at = Locator::new(&self.document.root);
        if let (false, Some(data)) = (self.type_id.is_empty(), self.data()) {
            match schemas.data_type(&self.type_id) {
                None => out.push(at.anchor(
                    Diagnostic::new(
                        Code::DATA_UNKNOWN_TYPE,
                        Severity::Warning,
                        format!("unknown data type {}; its data is kept as-is", self.type_id),
                    )
                    .with_suggestion(
                        "run `fynkel codegen` so schemas/data/ has the type, or register it in fynkel.project.json",
                    ),
                    "/type",
                )),
                Some(info) => {
                    if info.version != self.version {
                        out.push(at.anchor(
                            Diagnostic::error(
                                Code::DATA_INVALID,
                                format!(
                                    "written for version {} of {}, which is at version {}",
                                    self.version, self.type_id, info.version
                                ),
                            )
                            .with_suggestion("update the data to the type's current fields and version"),
                            "/version",
                        ));
                    }
                    for mut d in schemas.validate_data(&self.type_id, data) {
                        d.code = Code::DATA_INVALID;
                        let pointer = format!("/data{}", d.pointer.as_deref().unwrap_or(""));
                        out.push(at.anchor(d, &pointer));
                    }
                }
            }
        }
        for d in &mut out {
            if d.document.is_none() {
                d.document = self.document.path.clone();
            }
        }
        out
    }
}

impl Document {
    /// A fresh data asset of `type_id` at `version` holding `data` (an
    /// empty object: every field at the class default).
    pub fn new_data(name: &str, type_id: &str, version: u32, data: Option<Node>) -> Document {
        let mut doc = Document::new(DocumentKind::Data, name);
        doc.root.set("type", Node::string(type_id));
        doc.root.set("version", Node::integer(version as i64));
        doc.root
            .set("data", data.unwrap_or_else(|| Node::object(Vec::new())));
        doc
    }
}

/// `game.Weapon`: dot-separated identifiers, at least two.
pub fn is_type_id(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() >= 2
        && parts.iter().all(|p| {
            let mut chars = p.chars();
            chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}
