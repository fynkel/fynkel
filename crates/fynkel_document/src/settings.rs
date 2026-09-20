//! The user's `settings.json` (blueprint §19.2d item 12, scope rule
//! "Scenes and settings"): a [`Settings`](fynkel_core::settings::Settings)
//! record flat on the document plus `input.rebinds` — the two things a
//! wire record cannot hold together, a map and a list. Not an asset: no
//! envelope, no id, no `fynkel.lock.json` entry; the kind and format
//! version only, like an input recording. `schemas/settings.schema.json`
//! is generated from the record (`fynkel codegen`).
//!
//! Lenient on the way in (a missing field is its default, an older file
//! reads with defaults for what it lacks), strict on the way out (the
//! canonical writer, every field present). Semantic rules (`FYN-U`):
//! - the file must be a `settings` object at format version 1 (`U0001`);
//! - volumes in `0..=1`, a `quality` the project has when a project is
//!   given, every rebind a binding the grammar accepts for its action's
//!   kind — an action the project lacks is reported too (`U0002`);
//! - reading or writing the file failed (`U0003`).

use std::collections::BTreeMap;
use std::path::Path;

use fynkel_contracts::{Code, Diagnostic, Severity};
use fynkel_core::input::{parse_binding, ActionKind};
pub use fynkel_core::settings::Settings;

use crate::codec::DocumentCodec;
use crate::json::{self, Node, Value};

/// The `fynkel` kind of a settings file.
pub const KIND: &str = "settings";
pub const FORMAT_VERSION: i64 = 1;

/// A settings file as a whole.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SettingsDocument {
    pub settings: Settings,
    /// Action name → the bindings that replace the project's for it.
    pub rebinds: BTreeMap<String, Vec<String>>,
}

/// An action a settings file may rebind: its name and kind, from the
/// project file (`InputAction`) or the cooked cache (`ActionSpec`).
#[derive(Clone, Debug, PartialEq)]
pub struct KnownAction {
    pub name: String,
    pub kind: ActionKind,
}

/// What the validator knows about the project, when it does.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProjectContext {
    pub actions: Vec<KnownAction>,
    pub quality_profiles: Vec<String>,
}

impl SettingsDocument {
    /// Reads the text of a settings file. Returns the document (defaults
    /// for what is absent) and its diagnostics; an `Err` is a file that
    /// is not a settings document at all (`U0001`, or a JSON error).
    pub fn parse(text: &str) -> Result<(SettingsDocument, Vec<Diagnostic>), Diagnostic> {
        let root = json::parse(text)?;
        let mut diags = Vec::new();
        match root.get("fynkel").and_then(Node::as_str) {
            Some(KIND) => {}
            other => {
                return Err(Diagnostic::error(
                    Code::SETTINGS_NOT_A_DOCUMENT,
                    format!("not a settings document: fynkel is {other:?}, expected \"{KIND}\""),
                )
                .with_pointer("/fynkel"));
            }
        }
        match root.get("formatVersion").and_then(Node::as_i64) {
            Some(FORMAT_VERSION) => {}
            other => {
                return Err(Diagnostic::error(
                    Code::SETTINGS_NOT_A_DOCUMENT,
                    format!("formatVersion is {other:?}, expected {FORMAT_VERSION}"),
                )
                .with_pointer("/formatVersion"));
            }
        }
        // The record: absent fields default, a mistyped one is reported
        // and defaults too.
        let settings = match Settings::from_node(&root) {
            Ok(s) => s,
            Err(e) => {
                diags.push(
                    Diagnostic::error(Code::SETTINGS_VALUE, e.message)
                        .with_pointer(e.pointer.clone()),
                );
                Settings::default()
            }
        };
        let mut rebinds = BTreeMap::new();
        if let Some(input) = root.get("input") {
            match input.get("rebinds").map(|n| &n.value) {
                None => {}
                Some(Value::Object(members)) => {
                    for m in members {
                        let Some(items) = m.value.as_array() else {
                            diags.push(
                                Diagnostic::error(
                                    Code::SETTINGS_VALUE,
                                    format!("rebinds.{}: expected an array of bindings", m.key),
                                )
                                .with_pointer(format!(
                                    "/input/rebinds/{}",
                                    crate::scene::escape(&m.key)
                                )),
                            );
                            continue;
                        };
                        let mut bindings = Vec::new();
                        for (i, item) in items.iter().enumerate() {
                            match item.as_str() {
                                Some(b) => bindings.push(b.to_string()),
                                None => diags.push(
                                    Diagnostic::error(
                                        Code::SETTINGS_VALUE,
                                        format!(
                                            "rebinds.{}[{i}]: expected a binding string",
                                            m.key
                                        ),
                                    )
                                    .with_pointer(format!(
                                        "/input/rebinds/{}/{i}",
                                        crate::scene::escape(&m.key)
                                    )),
                                ),
                            }
                        }
                        rebinds.insert(m.key.clone(), bindings);
                    }
                }
                Some(_) => diags.push(
                    Diagnostic::error(Code::SETTINGS_VALUE, "input.rebinds must be an object")
                        .with_pointer("/input/rebinds"),
                ),
            }
        }
        let doc = SettingsDocument { settings, rebinds };
        diags.extend(doc.validate(None));
        Ok((doc, diags))
    }

    /// The semantic rules (`U0002`), against the project when given.
    pub fn validate(&self, project: Option<&ProjectContext>) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for (field, message) in self.settings.violations() {
            out.push(
                Diagnostic::error(Code::SETTINGS_VALUE, message).with_pointer(format!("/{field}")),
            );
        }
        let Some(p) = project else {
            return out;
        };
        if !self.settings.quality.is_empty() && !p.quality_profiles.contains(&self.settings.quality)
        {
            out.push(
                Diagnostic::new(
                    Code::SETTINGS_VALUE,
                    Severity::Warning,
                    format!(
                        "quality profile {:?} is not in the project (has {:?}); the default applies",
                        self.settings.quality, p.quality_profiles
                    ),
                )
                .with_pointer("/quality"),
            );
        }
        for (action, bindings) in &self.rebinds {
            let pointer = format!("/input/rebinds/{}", crate::scene::escape(action));
            let Some(known) = p.actions.iter().find(|a| a.name == *action) else {
                out.push(
                    Diagnostic::new(
                        Code::SETTINGS_VALUE,
                        Severity::Warning,
                        format!("rebinds an action the project does not have: {action}"),
                    )
                    .with_pointer(pointer),
                );
                continue;
            };
            for (i, b) in bindings.iter().enumerate() {
                if let Err(e) = parse_binding(known.kind, b) {
                    out.push(
                        Diagnostic::error(
                            Code::SETTINGS_VALUE,
                            format!("{action}: binding {b:?} is not valid: {e}"),
                        )
                        .with_pointer(format!("{pointer}/{i}")),
                    );
                }
            }
        }
        out
    }

    /// The canonical text of the document (every field present).
    pub fn to_text(&self) -> String {
        let record = self.settings.to_node();
        let mut members: Vec<(String, Node)> = vec![
            ("fynkel".into(), Node::string(KIND)),
            ("formatVersion".into(), Node::integer(FORMAT_VERSION)),
        ];
        if let Some(fields) = record.as_object() {
            for m in fields {
                members.push((m.key.clone(), m.value.clone()));
            }
        }
        let rebinds: Vec<(String, Node)> = self
            .rebinds
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    Node::synthetic(Value::Array(v.iter().map(Node::string).collect())),
                )
            })
            .collect();
        members.push((
            "input".into(),
            Node::object(vec![("rebinds".into(), Node::object(rebinds))]),
        ));
        json::write(&Node::object(members))
    }

    /// Reads `path`; a missing file is the defaults with no diagnostics
    /// (a first launch), anything else unreadable is `U0003`.
    pub fn read(path: &Path) -> Result<(SettingsDocument, Vec<Diagnostic>), Diagnostic> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok((SettingsDocument::default(), Vec::new()));
            }
            Err(e) => {
                return Err(Diagnostic::error(
                    Code::SETTINGS_IO,
                    format!("cannot read {}: {e}", path.display()),
                )
                .with_document(path.display().to_string()));
            }
        };
        Self::parse(&text).map_err(|d| d.with_document(path.display().to_string()))
    }

    /// Writes the canonical text to `path`, creating its directory,
    /// through a temporary file so a crash leaves the old file (`U0003`).
    pub fn write(&self, path: &Path) -> Result<(), Diagnostic> {
        let io = |e: std::io::Error| {
            Diagnostic::error(
                Code::SETTINGS_IO,
                format!("cannot write {}: {e}", path.display()),
            )
            .with_document(path.display().to_string())
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, self.to_text()).map_err(io)?;
        std::fs::rename(&tmp, path).map_err(io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> ProjectContext {
        ProjectContext {
            actions: vec![
                KnownAction {
                    name: "jump".into(),
                    kind: ActionKind::Button,
                },
                KnownAction {
                    name: "move".into(),
                    kind: ActionKind::Axis2d,
                },
            ],
            quality_profiles: vec!["low".into(), "medium".into(), "high".into()],
        }
    }

    #[test]
    fn a_missing_field_is_its_default_and_the_text_round_trips() {
        let (doc, diags) = SettingsDocument::parse(
            r#"{"fynkel":"settings","formatVersion":1,"music_volume":0.25,"input":{"rebinds":{"jump":["keyboard:J"]}}}"#,
        )
        .unwrap();
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(doc.settings.music_volume, 0.25);
        assert_eq!(doc.settings.master_volume, 1.0);
        assert_eq!(doc.rebinds["jump"], vec!["keyboard:J".to_string()]);
        let text = doc.to_text();
        assert!(text.starts_with("{\n  \"fynkel\": \"settings\",\n  \"formatVersion\": 1,\n"));
        let (again, diags) = SettingsDocument::parse(&text).unwrap();
        assert!(diags.is_empty());
        assert_eq!(again, doc);
        assert_eq!(again.to_text(), text);
    }

    #[test]
    fn not_a_settings_document_is_u0001() {
        let e = SettingsDocument::parse(r#"{"fynkel":"scene","formatVersion":1}"#).unwrap_err();
        assert_eq!(e.code, Code::SETTINGS_NOT_A_DOCUMENT);
        let e = SettingsDocument::parse(r#"{"fynkel":"settings","formatVersion":2}"#).unwrap_err();
        assert_eq!(e.code, Code::SETTINGS_NOT_A_DOCUMENT);
        assert_eq!(e.pointer.as_deref(), Some("/formatVersion"));
    }

    #[test]
    fn values_and_rebinds_are_checked_against_the_project() {
        let (doc, diags) = SettingsDocument::parse(
            r#"{"fynkel":"settings","formatVersion":1,"master_volume":2,"quality":"ultra","input":{"rebinds":{"jump":["keyboard:Nope!"],"fly":["keyboard:F"],"move":["keyboard:WASD"]}}}"#,
        )
        .unwrap();
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].pointer.as_deref(), Some("/master_volume"));
        let diags = doc.validate(Some(&project()));
        let codes: Vec<_> = diags
            .iter()
            .map(|d| (d.pointer.clone().unwrap(), d.severity))
            .collect();
        assert_eq!(
            codes,
            vec![
                ("/master_volume".to_string(), Severity::Error),
                ("/quality".to_string(), Severity::Warning),
                ("/input/rebinds/fly".to_string(), Severity::Warning),
                ("/input/rebinds/jump/0".to_string(), Severity::Error),
            ]
        );
        assert!(diags.iter().all(|d| d.code == Code::SETTINGS_VALUE));
    }

    #[test]
    fn read_and_write_through_a_file() {
        let dir = std::env::temp_dir().join(format!("fynkel-settings-{}", std::process::id()));
        let path = dir.join("nested").join("settings.json");
        let (fresh, diags) = SettingsDocument::read(&path).unwrap();
        assert!(diags.is_empty());
        assert_eq!(fresh, SettingsDocument::default());
        let mut doc = fresh;
        doc.settings.muted = true;
        doc.rebinds
            .insert("jump".into(), vec!["gamepad:south".into()]);
        doc.write(&path).unwrap();
        let (back, _) = SettingsDocument::read(&path).unwrap();
        assert_eq!(back, doc);
        assert!(!path.with_extension("json.tmp").exists());
        std::fs::write(&path, "{").unwrap();
        assert!(SettingsDocument::read(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
