//! Blend-space documents (blueprint §19.2d item 19c): animation clips
//! placed along one named parameter. A `.blend.json` is what an
//! `Animator` plays when one clip is not enough — idle, walk and run
//! along `speed` — and a behaviour drives it with one number instead of
//! switching clips itself. The document is data only: the runtime
//! (`fynkel_core::animation::BlendSpace`) blends the two points around
//! the parameter's value, phase-synced; the cook checks the document and
//! writes it as it is.
//!
//! Like [`crate::rig`], the tree stays the source of truth and the view
//! is lenient: a broken document still describes itself, every problem
//! a diagnostic with a pointer.
//!
//! Semantic rules (`FYN-D06xx`): the points are strictly increasing in
//! `at` (`D0600`); every point names an animation clip — a rig's or a
//! model source's `anim/<n>` sub-asset (`D0601`); a malformed entry — an
//! `assetRef` that does not read, an `at` that is not finite — is
//! `D0602`. Unknown ids are the asset database's `A0004`/`A0005`.

use fynkel_contracts::{AssetRef, Code, Diagnostic};
use uuid::Uuid;

use crate::asset::{AssetDatabase, AssetKind};
use crate::json::Node;
use crate::rig::{is_model, RIG_CLIP_PREFIX};
use crate::schema::SchemaSet;
use crate::{Document, DocumentKind};

/// The most points one blend space may have (`schemas/blend.schema.json`).
pub const MAX_POINTS: usize = 16;

/// One point: the clip that plays alone at `at`.
#[derive(Clone, Debug, PartialEq)]
pub struct BlendPoint {
    pub at: f32,
    pub clip: AssetRef,
    /// `/points/<i>`.
    pub pointer: String,
}

#[derive(Clone, Debug)]
pub struct Blend {
    pub document: Document,
    /// Empty when the field does not read (reported).
    pub parameter: String,
    /// Document order (the rule wants it increasing).
    pub points: Vec<BlendPoint>,
    /// `phase` (the only sync in V1).
    pub sync: String,
    read_diagnostics: Vec<Diagnostic>,
}

impl Blend {
    pub fn from_document(document: Document) -> Blend {
        let mut diags = Vec::new();
        if document.kind != DocumentKind::Blend {
            diags.push(
                Diagnostic::error(
                    Code::UNKNOWN_DOCUMENT_KIND,
                    format!("expected a blend space, got {}", document.kind.name()),
                )
                .with_pointer(""),
            );
        }
        let root = &document.root;
        let malformed = |msg: String, pointer: &str| {
            anchor(
                root,
                Diagnostic::error(Code::BLEND_MALFORMED_ENTRY, msg),
                pointer,
            )
        };
        // A missing required field is the schema's to report.
        let parameter = match root.get("parameter") {
            None => String::new(),
            Some(node) => match node.as_str() {
                Some(p) if !p.is_empty() => p.to_string(),
                _ => {
                    diags.push(malformed(
                        "parameter must be a non-empty string".into(),
                        "/parameter",
                    ));
                    String::new()
                }
            },
        };
        let sync = match root.get("sync") {
            None => "phase".to_string(),
            Some(node) => match node.as_str() {
                Some("phase") => "phase".to_string(),
                _ => {
                    diags.push(malformed("sync must be \"phase\"".into(), "/sync"));
                    "phase".to_string()
                }
            },
        };
        let mut points = Vec::new();
        match root.get("points") {
            None => {}
            Some(node) => match node.as_array() {
                Some(list) => {
                    for (i, item) in list.iter().enumerate() {
                        let pointer = format!("/points/{i}");
                        let Some(members) = item.as_object() else {
                            diags.push(malformed("a point must be an object".into(), &pointer));
                            continue;
                        };
                        let at = item.get("at").and_then(Node::as_f64);
                        let clip = crate::codec::asset_ref_field(members, "clip");
                        match (at, clip) {
                            (Some(at), Ok(clip)) if at.is_finite() => points.push(BlendPoint {
                                at: at as f32,
                                clip,
                                pointer,
                            }),
                            (_, Err(e)) => diags.push(malformed(
                                format!("point {i}: {}", e.message),
                                &format!("{pointer}{}", e.pointer),
                            )),
                            _ => diags.push(malformed(
                                "a point needs a finite `at`".into(),
                                &format!("{pointer}/at"),
                            )),
                        }
                    }
                }
                None => diags.push(malformed("points must be an array".into(), "/points")),
            },
        }
        Blend {
            document,
            parameter,
            points,
            sync,
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

    /// Envelope + JSON Schema, the read problems and the order rule
    /// (`D0600`); the rules that need the asset database are
    /// [`Blend::validate_references`].
    pub fn validate(&self, schemas: &SchemaSet) -> Vec<Diagnostic> {
        let mut out = self.document.validate(schemas);
        let schema_pointers: Vec<String> = out.iter().filter_map(|d| d.pointer.clone()).collect();
        for d in &self.read_diagnostics {
            // JSON Schema already said it (wrong type / missing field).
            if d.code == Code::BLEND_MALFORMED_ENTRY
                && d.pointer
                    .as_ref()
                    .is_some_and(|p| schema_pointers.contains(p))
            {
                continue;
            }
            out.push(d.clone());
        }
        let root = &self.document.root;
        if self.points.is_empty() && !schema_pointers.iter().any(|p| p == "/points") {
            out.push(anchor(
                root,
                Diagnostic::error(Code::BLEND_POINTS, "a blend space needs at least one point"),
                "/points",
            ));
        }
        for (i, w) in self.points.windows(2).enumerate() {
            if w[1].at <= w[0].at {
                out.push(anchor(
                    root,
                    Diagnostic::error(
                        Code::BLEND_POINTS,
                        format!(
                            "points must be strictly increasing in `at`: point {} ({}) after point {} ({})",
                            i + 1,
                            w[1].at,
                            i,
                            w[0].at
                        ),
                    )
                    .with_suggestion("order the points by `at`, one clip per value"),
                    &format!("{}/at", w[1].pointer),
                ));
            }
        }
        finish(out, &self.document)
    }

    /// The rule against the asset database: every point names an
    /// animation clip — a rig document's `anim/<name>` or a model
    /// source's `anim/<n>` (`D0601`). A reference the database cannot
    /// resolve is left to [`AssetDatabase::check_references`]
    /// (`A0004`/`A0005`).
    pub fn validate_references(&self, db: &AssetDatabase) -> Vec<Diagnostic> {
        let root = &self.document.root;
        let mut out = Vec::new();
        for p in &self.points {
            if p.clip.is_nil() {
                out.push(anchor(
                    root,
                    Diagnostic::error(
                        Code::BLEND_CLIP,
                        "the point's clip is not set (a nil asset id)",
                    )
                    .with_suggestion(
                        "name a rig's `anim/<name>` or an imported model's `anim/<n>`",
                    ),
                    &format!("{}/clip/asset", p.pointer),
                ));
                continue;
            }
            let Ok(resolved) = db.resolve(&p.clip) else {
                continue;
            };
            let clip_sub = resolved
                .sub
                .as_ref()
                .is_some_and(|(sub, _)| sub.starts_with(RIG_CLIP_PREFIX));
            let carrier = is_model(&resolved.entry.kind)
                || resolved.entry.kind == AssetKind::Document(DocumentKind::Rig);
            if !carrier || !clip_sub {
                let listed: Vec<&str> = resolved
                    .entry
                    .sub_assets
                    .keys()
                    .filter(|s| s.starts_with(RIG_CLIP_PREFIX))
                    .map(String::as_str)
                    .collect();
                out.push(anchor(
                    root,
                    Diagnostic::error(
                        Code::BLEND_CLIP,
                        format!(
                            "the point names {}{}, which is not an animation clip (anim/<n>)",
                            resolved.entry.path,
                            match &resolved.sub {
                                Some((sub, _)) => format!("#{sub}"),
                                None => String::new(),
                            }
                        ),
                    )
                    .with_id(p.clip.to_string())
                    .with_suggestion(if listed.is_empty() {
                        format!("{} carries no animation clips", resolved.entry.path)
                    } else {
                        format!("{} carries: {}", resolved.entry.path, listed.join(", "))
                    }),
                    &format!("{}/clip", p.pointer),
                ));
            }
        }
        finish(out, &self.document)
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

fn finish(mut out: Vec<Diagnostic>, document: &Document) -> Vec<Diagnostic> {
    for d in &mut out {
        if d.document.is_none() {
            d.document = document.path.clone();
        }
    }
    out
}
