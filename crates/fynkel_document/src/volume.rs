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
use fynkel_core::volume::{
    AoOverrides, BloomOverrides, ContactOverrides, GradingOverrides, Tonemapper, VolumeProfile,
};
use glam::Vec3;
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
        if let Some(mut block) = Block::open(root, "ambientOcclusion", &mut diags) {
            profile.ambient_occlusion = AoOverrides {
                radius: block.number("radius"),
                intensity: block.number("intensity"),
            };
        }
        if let Some(mut block) = Block::open(root, "bloom", &mut diags) {
            profile.bloom = BloomOverrides {
                intensity: block.number("intensity"),
                threshold: block.number("threshold"),
                scatter: block.number("scatter"),
                tint: block.color("tint"),
            };
        }
        if let Some(mut block) = Block::open(root, "colorGrading", &mut diags) {
            profile.color_grading = GradingOverrides {
                tonemapper: block.tonemapper("tonemapper"),
                post_exposure: block.number("postExposure"),
                temperature: block.number("temperature"),
                tint: block.number("tint"),
                contrast: block.number("contrast"),
                saturation: block.number("saturation"),
                hue_shift: block.number("hueShift"),
                color_filter: block.color("colorFilter"),
                lift: block.color("lift"),
                gamma: block.color("gamma"),
                gain: block.color("gain"),
            };
        }
        if let Some(mut block) = Block::open(root, "contactShadows", &mut diags) {
            profile.contact_shadows = ContactOverrides {
                intensity: block.number("intensity"),
                length: block.number("length"),
                thickness: block.number("thickness"),
                distance: block.number("distance"),
            };
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

/// One override block being read: every field that does not read is a
/// `FYN-D0700` at its pointer and stays absent (inherited).
struct Block<'a> {
    root: &'a Node,
    node: &'a Node,
    name: &'static str,
    diags: &'a mut Vec<Diagnostic>,
}

impl<'a> Block<'a> {
    /// The block, when present and an object; a present non-object is
    /// reported and skipped.
    fn open(
        root: &'a Node,
        name: &'static str,
        diags: &'a mut Vec<Diagnostic>,
    ) -> Option<Block<'a>> {
        let node = root.get(name)?;
        if node.as_object().is_none() {
            diags.push(anchor(
                root,
                Diagnostic::error(
                    Code::VOLUME_MALFORMED_ENTRY,
                    format!("{name} must be an object"),
                ),
                &format!("/{name}"),
            ));
            return None;
        }
        Some(Block {
            root,
            node,
            name,
            diags,
        })
    }

    fn malformed(&mut self, field: &str, what: &str) {
        let name = self.name;
        self.diags.push(anchor(
            self.root,
            Diagnostic::error(
                Code::VOLUME_MALFORMED_ENTRY,
                format!("{name}.{field} must be {what}"),
            ),
            &format!("/{name}/{field}"),
        ));
    }

    fn number(&mut self, field: &str) -> Option<f32> {
        let node = self.node.get(field)?;
        match node.as_f64() {
            Some(v) if v.is_finite() => Some(v as f32),
            _ => {
                self.malformed(field, "a finite number");
                None
            }
        }
    }

    /// One of [`Tonemapper::ALL`] by name.
    fn tonemapper(&mut self, field: &str) -> Option<Tonemapper> {
        let node = self.node.get(field)?;
        match node.as_str().and_then(Tonemapper::from_name) {
            Some(t) => Some(t),
            None => {
                self.malformed(field, "one of \"aces\", \"neutral\", \"none\"");
                None
            }
        }
    }

    /// Linear RGB as `[r, g, b]`.
    fn color(&mut self, field: &str) -> Option<Vec3> {
        let node = self.node.get(field)?;
        let rgb: Option<Vec<f32>> = node.as_array().and_then(|items| {
            items
                .iter()
                .map(|n| n.as_f64().filter(|v| v.is_finite()).map(|v| v as f32))
                .collect()
        });
        match rgb.as_deref() {
            Some(&[r, g, b]) => Some(Vec3::new(r, g, b)),
            _ => {
                self.malformed(field, "three finite numbers [r, g, b]");
                None
            }
        }
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
