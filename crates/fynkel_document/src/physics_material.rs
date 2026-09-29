//! Physics materials (ADR-0025, `docs/physics.md` § Physics materials):
//! the friction, bounciness and combine rules every `fynkel.Collider`
//! whose `material` names a `.physicsmaterial.json` collides with —
//! Unity's Physics Material. Every field is optional; an absent one takes
//! Unity's default (friction 0.6, bounciness 0, both rules `average`).
//! The cook checks the document and writes it as it is; the asset store
//! reads it back into a [`PhysicsMaterial`] for the physics world.
//!
//! Like [`crate::volume`], the view is lenient: a broken document still
//! describes itself, and every field that does not read is a
//! `FYN-D0800` diagnostic with a pointer that leaves the field at its
//! default (the schema's ranges are the schema's to report).

use fynkel_contracts::{Code, Diagnostic};
use fynkel_physics::{CombineRule, PhysicsMaterial};
use uuid::Uuid;

use crate::json::Node;
#[cfg(feature = "validate")]
use crate::schema::SchemaSet;
use crate::{Document, DocumentKind};

#[derive(Clone, Debug)]
pub struct PhysicsMaterialDocument {
    pub document: Document,
    /// What read; a field that did not (or is absent) is the default.
    pub material: PhysicsMaterial,
    read_diagnostics: Vec<Diagnostic>,
}

impl PhysicsMaterialDocument {
    pub fn from_document(document: Document) -> PhysicsMaterialDocument {
        let mut diags = Vec::new();
        if document.kind != DocumentKind::PhysicsMaterial {
            diags.push(
                Diagnostic::error(
                    Code::UNKNOWN_DOCUMENT_KIND,
                    format!("expected a physics material, got {}", document.kind.name()),
                )
                .with_pointer(""),
            );
        }
        let root = &document.root;
        let mut material = PhysicsMaterial::default();
        let mut malformed = |field: &str, what: &str| {
            diags.push(anchor(
                root,
                Diagnostic::error(
                    Code::PHYSICS_MATERIAL_MALFORMED_ENTRY,
                    format!("{field} must be {what}"),
                ),
                &format!("/{field}"),
            ));
        };
        if let Some(node) = root.get("friction") {
            match node.as_f64() {
                Some(v) if v.is_finite() && v >= 0.0 => material.friction = v as f32,
                _ => malformed("friction", "a finite number >= 0"),
            }
        }
        if let Some(node) = root.get("bounciness") {
            match node.as_f64() {
                Some(v) if (0.0..=1.0).contains(&v) => material.bounciness = v as f32,
                _ => malformed("bounciness", "a number in 0..=1"),
            }
        }
        for (field, slot) in [
            ("friction_combine", &mut material.friction_combine),
            ("bounce_combine", &mut material.bounce_combine),
        ] {
            if let Some(node) = root.get(field) {
                match node.as_str().and_then(CombineRule::parse) {
                    Some(rule) => *slot = rule,
                    None => malformed(
                        field,
                        "\"average\", \"minimum\", \"multiply\" or \"maximum\"",
                    ),
                }
            }
        }
        PhysicsMaterialDocument {
            document,
            material,
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
    #[cfg(feature = "validate")]
    pub fn validate(&self, schemas: &SchemaSet) -> Vec<Diagnostic> {
        let mut out = self.document.validate(schemas);
        let schema_pointers: Vec<String> = out.iter().filter_map(|d| d.pointer.clone()).collect();
        for d in &self.read_diagnostics {
            if d.code == Code::PHYSICS_MATERIAL_MALFORMED_ENTRY
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
