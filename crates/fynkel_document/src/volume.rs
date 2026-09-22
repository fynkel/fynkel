//! Volume profiles (R3a, `docs/renderer.md` § Volumes): the look
//! overrides a `fynkel.Volume` component applies to the frame. A
//! `.volume.json` holds optional blocks of optional fields — a present
//! field overrides what is below it, an absent one is inherited — and
//! the renderer blends the profiles that reach the camera once a frame
//! (`fynkel_core::volume`). The cook checks the document and writes it
//! as it is; the asset store reads it back into a [`VolumeProfile`].
//!
//! Like [`crate::blend`], the view is lenient: a broken document still
//! describes itself, and every field that does not read is a
//! `FYN-D0700` diagnostic with a pointer (the schema's ranges are the
//! schema's to report).

use fynkel_contracts::{Code, Diagnostic};
use fynkel_core::volume::{AoOverrides, VolumeProfile};
use uuid::Uuid;

use crate::json::Node;
use crate::schema::SchemaSet;
use crate::{Document, DocumentKind};

#[derive(Clone, Debug)]
pub struct VolumeDocument {
    pub document: Document,
    /// What read; a field that did not is absent (inherited).
    pub profile: VolumeProfile,
    read_diagnostics: Vec<Diagnostic>,
}

impl VolumeDocument {
    pub fn from_document(document: Document) -> VolumeDocument {
        let mut diags = Vec::new();
        if document.kind != DocumentKind::Volume {
            diags.push(
                Diagnostic::error(
                    Code::UNKNOWN_DOCUMENT_KIND,
                    format!("expected a volume profile, got {}", document.kind.name()),
                )
                .with_pointer(""),
            );
        }
        let root = &document.root;
        let mut profile = VolumeProfile::default();
        if let Some(block) = root.get("ambientOcclusion") {
            if block.as_object().is_some() {
                let mut number = |field: &str| -> Option<f32> {
                    let node = block.get(field)?;
                    match node.as_f64() {
                        Some(v) if v.is_finite() => Some(v as f32),
                        _ => {
                            diags.push(anchor(
                                root,
                                Diagnostic::error(
                                    Code::VOLUME_MALFORMED_ENTRY,
                                    format!("ambientOcclusion.{field} must be a finite number"),
                                ),
                                &format!("/ambientOcclusion/{field}"),
                            ));
                            None
                        }
                    }
                };
                profile.ambient_occlusion = AoOverrides {
                    radius: number("radius"),
                    intensity: number("intensity"),
                };
            } else {
                diags.push(anchor(
                    root,
                    Diagnostic::error(
                        Code::VOLUME_MALFORMED_ENTRY,
                        "ambientOcclusion must be an object",
                    ),
                    "/ambientOcclusion",
                ));
            }
        }
        VolumeDocument {
            document,
            profile,
            read_diagnostics: diags,
        }
    }

    pub fn id(&self) -> Uuid {
        self.document.envelope.id
    }

    pub fn name(&self) -> &str {
        &self.document.envelope.name
    }

    pub fn read_diagnostics(&self) -> &[Diagnostic] {
        &self.read_diagnostics
    }

    /// Envelope + JSON Schema (the ranges) and the read problems the
    /// schema did not already report.
    pub fn validate(&self, schemas: &SchemaSet) -> Vec<Diagnostic> {
        let mut out = self.document.validate(schemas);
        let schema_pointers: Vec<String> = out.iter().filter_map(|d| d.pointer.clone()).collect();
        for d in &self.read_diagnostics {
            if d.code == Code::VOLUME_MALFORMED_ENTRY
                && d.pointer
                    .as_ref()
                    .is_some_and(|p| schema_pointers.contains(p))
            {
                continue;
            }
            out.push(d.clone());
        }
        for d in &mut out {
            if d.document.is_none() {
                d.document = self.document.path.clone();
            }
        }
        out
    }
}

fn anchor(root: &Node, d: Diagnostic, pointer: &str) -> Diagnostic {
    let d = d.with_pointer(pointer);
    let mut at = pointer.to_string();
    loop {
        if let Some(s) = root.pointer_span(&at) {
            return d.at(s.start.line, s.start.column);
        }
        match at.rfind('/') {
            Some(i) => at.truncate(i),
            None => return d,
        }
    }
}
