//! Rig documents (blueprint §19.2d item 19b): one character assembled from
//! downloads. A `.rig.json` names the imported model whose skeleton, skin
//! and meshes the character uses and the animation clips it plays, each
//! clip taken from any imported model that carries it — a Mixamo pack is
//! one FBX per clip, all sharing the skeleton's bone names. The game names
//! the rig, never the files: `{ asset: <rig>, sub: "prefab" }` instances
//! the skeleton file's hierarchy, `{ asset: <rig>, sub: "anim/<name>" }`
//! plays a clip under the name the rig gives it. The cook (`fynkel_import`)
//! copies the skeleton's hierarchy into the rig's cooked object and
//! re-emits every clip matched to the skeleton by bone name.
//!
//! Like [`crate::material`], the tree stays the source of truth and the
//! view is lenient: a broken document still describes itself, every
//! problem a diagnostic with a pointer.
//!
//! Semantic rules (`FYN-D05xx`): `skeleton` must name an imported model —
//! a `fynkel.gltf`/`fynkel.fbx` source, no `sub` (`D0500`); every clip
//! must name such a source's `anim/<n>` sub-asset (`D0501`); a malformed
//! entry — an `assetRef` that does not read, a clip name that is not an
//! identifier — is `D0502`. Unknown ids are the asset database's
//! `A0004`/`A0005`, as for every document.

use fynkel_contracts::{AssetRef, Code, Diagnostic};
use uuid::Uuid;

use crate::asset::{AssetDatabase, AssetKind, FBX_IMPORTER, GLTF_IMPORTER};
use crate::json::Node;
use crate::scene::escape;
use crate::schema::SchemaSet;
use crate::{Document, DocumentKind};

/// The rig's own hierarchy sub-asset: the skeleton file's, copied.
pub const RIG_PREFAB_SUB: &str = "prefab";
/// Prefix of the rig's clip sub-assets: `anim/<clip name>`.
pub const RIG_CLIP_PREFIX: &str = "anim/";

/// One clip the rig plays, under the name the game uses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RigClip {
    /// The name the game uses: `anim/<name>` on the rig.
    pub name: String,
    /// The imported clip: a model source's `anim/<n>`.
    pub asset: AssetRef,
    /// `/clips/<name>`.
    pub pointer: String,
}

#[derive(Clone, Debug)]
pub struct Rig {
    pub document: Document,
    /// `None` when the field does not read (reported).
    pub skeleton: Option<AssetRef>,
    /// Document order.
    pub clips: Vec<RigClip>,
    read_diagnostics: Vec<Diagnostic>,
}

impl Rig {
    pub fn from_document(document: Document) -> Rig {
        let mut diags = Vec::new();
        if document.kind != DocumentKind::Rig {
            diags.push(
                Diagnostic::error(
                    Code::UNKNOWN_DOCUMENT_KIND,
                    format!("expected a rig, got {}", document.kind.name()),
                )
                .with_pointer(""),
            );
        }
        let root = &document.root;
        let malformed = |msg: String, pointer: &str| {
            anchor(
                root,
                Diagnostic::error(Code::RIG_MALFORMED_ENTRY, msg),
                pointer,
            )
        };
        let skeleton = match root.as_object() {
            Some(members) => match crate::codec::asset_ref_field(members, "skeleton") {
                Ok(r) => Some(r),
                Err(e) => {
                    diags.push(malformed(format!("skeleton: {}", e.message), &e.pointer));
                    None
                }
            },
            None => {
                diags.push(malformed("the document must be an object".into(), ""));
                None
            }
        };
        let mut clips = Vec::new();
        match root.get("clips") {
            None => {}
            Some(node) => match node.as_object() {
                Some(members) => {
                    for m in members {
                        let pointer = format!("/clips/{}", escape(&m.key));
                        if !is_clip_name(&m.key) {
                            diags.push(malformed(
                                format!(
                                    "clip name {:?} is not an identifier (letters, digits and _, not starting with a digit)",
                                    m.key
                                ),
                                &pointer,
                            ));
                            continue;
                        }
                        match crate::codec::asset_ref_field(members, &m.key) {
                            Ok(asset) => clips.push(RigClip {
                                name: m.key.clone(),
                                asset,
                                pointer,
                            }),
                            Err(e) => diags.push(malformed(
                                format!("clip {:?}: {}", m.key, e.message),
                                &format!("/clips{}", e.pointer),
                            )),
                        }
                    }
                }
                None => diags.push(malformed("clips must be an object".into(), "/clips")),
            },
        }
        Rig {
            document,
            skeleton,
            clips,
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

    /// The stable uuid of one of the rig's sub-assets: UUID v5 in the
    /// rig's namespace (the same derivation an importer uses for a
    /// source's sub-assets), so a reference survives re-cooks and
    /// machines.
    pub fn sub_id(rig: Uuid, sub: &str) -> Uuid {
        Uuid::new_v5(&rig, sub.as_bytes())
    }

    /// The sub-assets the rig exposes, in order: `prefab`, then
    /// `anim/<name>` per clip. What the asset database registers for a rig
    /// document and what its cooked object carries.
    pub fn sub_assets(&self) -> Vec<(String, Uuid)> {
        let mut out = vec![(
            RIG_PREFAB_SUB.to_string(),
            Rig::sub_id(self.id(), RIG_PREFAB_SUB),
        )];
        for c in &self.clips {
            let sub = format!("{RIG_CLIP_PREFIX}{}", c.name);
            let id = Rig::sub_id(self.id(), &sub);
            out.push((sub, id));
        }
        out
    }

    /// The `anim/<name>` sub-asset id of a clip.
    pub fn clip_sub(name: &str) -> String {
        format!("{RIG_CLIP_PREFIX}{name}")
    }

    /// The skeleton file's hierarchy as the rig's own `prefab` sub-asset:
    /// the imported prefab document (`text`, as the importer or the
    /// cooked object gives it) with the rig's derived sub-asset uuid as
    /// its `id` and the rig's name, everything else verbatim — the entity
    /// ids stay the skeleton file's, so an override written against the
    /// file keeps working against the rig. `shown` names the document in
    /// diagnostics.
    pub fn adopt_prefab(&self, text: &str, shown: &str) -> Result<Document, Vec<Diagnostic>> {
        let mut value: serde_json::Value = serde_json::from_str(text).map_err(|e| {
            vec![Diagnostic::error(
                Code::JSON_SYNTAX,
                format!("the imported prefab does not parse: {e}"),
            )
            .with_document(shown.to_string())]
        })?;
        let id = Rig::sub_id(self.id(), RIG_PREFAB_SUB);
        value["id"] = serde_json::Value::from(id.to_string());
        value["name"] = serde_json::Value::from(self.name().to_string());
        let text = serde_json::to_string(&value).expect("a JSON value serializes");
        Document::parse(DocumentKind::Prefab, Some(shown), &text)
    }

    /// Envelope + JSON Schema and the read problems; the rules that need
    /// the asset database are [`Rig::validate_references`].
    pub fn validate(&self, schemas: &SchemaSet) -> Vec<Diagnostic> {
        let mut out = self.document.validate(schemas);
        let schema_pointers: Vec<String> = out.iter().filter_map(|d| d.pointer.clone()).collect();
        for d in &self.read_diagnostics {
            // JSON Schema already said it (wrong type / missing field).
            if d.code == Code::RIG_MALFORMED_ENTRY
                && d.pointer
                    .as_ref()
                    .is_some_and(|p| schema_pointers.contains(p))
            {
                continue;
            }
            out.push(d.clone());
        }
        finish(out, &self.document)
    }

    /// The rules against the asset database: the skeleton is an imported
    /// model (`D0500`), every clip an imported model's `anim/<n>`
    /// (`D0501`). A reference the database cannot resolve is left to
    /// [`AssetDatabase::check_references`] (`A0004`/`A0005`).
    pub fn validate_references(&self, db: &AssetDatabase) -> Vec<Diagnostic> {
        let root = &self.document.root;
        let mut out = Vec::new();
        if let Some(skeleton) = &self.skeleton {
            if skeleton.is_nil() {
                out.push(anchor(
                    root,
                    Diagnostic::error(Code::RIG_SKELETON, "skeleton is not set (a nil asset id)")
                        .with_suggestion(
                            "name the imported model (glTF, FBX) whose skeleton the rig uses",
                        ),
                    "/skeleton/asset",
                ));
            } else if let Ok(resolved) = db.resolve(skeleton) {
                let model = is_model(&resolved.entry.kind);
                if !model || resolved.sub.is_some() {
                    out.push(anchor(
                        root,
                        Diagnostic::error(
                            Code::RIG_SKELETON,
                            if !model {
                                format!(
                                    "skeleton names {} ({}), which is not an imported model",
                                    resolved.entry.path,
                                    resolved.entry.kind.name()
                                )
                            } else {
                                format!(
                                    "skeleton names a sub-asset of {}; the skeleton is the model itself",
                                    resolved.entry.path
                                )
                            },
                        )
                        .with_id(skeleton.to_string())
                        .with_suggestion(
                            "skeleton is `{ \"asset\": <id of a .glb/.gltf/.fbx source> }` with no sub",
                        ),
                        "/skeleton",
                    ));
                }
            }
        }
        for c in &self.clips {
            if c.asset.is_nil() {
                continue;
            }
            let Ok(resolved) = db.resolve(&c.asset) else {
                continue;
            };
            let clip_sub = resolved
                .sub
                .as_ref()
                .is_some_and(|(sub, _)| sub.starts_with(RIG_CLIP_PREFIX));
            if !is_model(&resolved.entry.kind) || !clip_sub {
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
                        Code::RIG_CLIP,
                        format!(
                            "clip {:?} names {}{}, which is not an imported model's animation clip (anim/<n>)",
                            c.name,
                            resolved.entry.path,
                            match &resolved.sub {
                                Some((sub, _)) => format!("#{sub}"),
                                None => String::new(),
                            }
                        ),
                    )
                    .with_id(c.asset.to_string())
                    .with_suggestion(if listed.is_empty() {
                        format!(
                            "{} has no animation clips; `fynkel import` lists what a model carries",
                            resolved.entry.path
                        )
                    } else {
                        format!("{} carries: {}", resolved.entry.path, listed.join(", "))
                    }),
                    &c.pointer,
                ));
            }
        }
        finish(out, &self.document)
    }
}

/// A `fynkel.gltf` / `fynkel.fbx` source: what carries a hierarchy, a skin
/// and clips.
pub fn is_model(kind: &AssetKind) -> bool {
    matches!(kind, AssetKind::Source { importer, .. } if importer == GLTF_IMPORTER || importer == FBX_IMPORTER)
}

/// `schemas/rig.schema.json`'s `clipName`: an identifier of at most 64
/// characters.
pub fn is_clip_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && name.len() <= 64
}

/// A clip name derived from free text (a file stem, a clip's own name):
/// lower-cased, every run of other characters one `_`, a leading digit
/// prefixed; `clip` when nothing is left. `fynkel import --rig` names
/// clips with it.
pub fn clip_name_from(text: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            if gap && !out.is_empty() {
                out.push('_');
            }
            gap = false;
            out.push(c.to_ascii_lowercase());
        } else {
            gap = true;
        }
    }
    if out.is_empty() {
        return "clip".to_string();
    }
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out.truncate(64);
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_names() {
        assert_eq!(clip_name_from("Fast Run"), "fast_run");
        assert_eq!(clip_name_from("Idle"), "idle");
        assert_eq!(clip_name_from("mixamo.com"), "mixamo_com");
        assert_eq!(clip_name_from("  --  "), "clip");
        assert_eq!(clip_name_from("2 handed"), "_2_handed");
        assert!(is_clip_name("fast_run"));
        assert!(!is_clip_name("fast run"));
        assert!(!is_clip_name("2x"));
        assert!(!is_clip_name(""));
    }

    #[test]
    fn sub_assets_are_stable() {
        let id: Uuid = "c0000000-0000-4000-8000-000000000050".parse().unwrap();
        assert_eq!(
            Rig::sub_id(id, "anim/walk"),
            Uuid::new_v5(&id, b"anim/walk")
        );
        assert_eq!(Rig::clip_sub("walk"), "anim/walk");
    }
}
