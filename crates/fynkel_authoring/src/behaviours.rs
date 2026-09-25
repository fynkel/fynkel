//! `registerBehaviour { id, description? }` (roadmap Phase 2, Create →
//! Behaviour): after the editor wrote a behaviour's Dart file and
//! `fynkel codegen` generated its metadata and schema, the session takes
//! the new type in without reopening the project.
//!
//! - The project's component registry (`fynkel.project.json`
//!   `components`) gains `id → { kind: behaviour, version: 1,
//!   description? }` (`kind: data` with `{ kind: "data" }` in the
//!   arguments: a `@FynkelDataAsset` type, ADR-0027) when the project has a registry and the entry is
//!   missing (a scene using a type the registry leaves out is a warning).
//!   That write is one asset-history step, so ⌥⌘Z takes it back.
//! - The project is read again either way: the behaviour metadata
//!   (`lib/generated/behaviours.g.json`), the project's schemas
//!   (`schemas/behaviours/`) and the schema sources, which are read only
//!   when a project opens. Open documents keep their handles and edits.
//!
//! `reloadBehaviours {}` is the second half alone: after `fynkel codegen`
//! regenerated the metadata of types that exist already (a field added or
//! changed — the editor runs it when a script is saved, as Unity recompiles),
//! the session reads the metadata and schemas again. Nothing is written;
//! it answers `{ types }`, the behaviour ids the metadata names.

use fynkel_contracts::{Code, Diagnostic};
use fynkel_document::Node;
use serde_json::{json, Value};

use crate::ops::{io, AssetOp, Done, FileRewrite};
use crate::{AuthoringSession, OpenProject};

pub const OP: &str = "registerBehaviour";
pub const RELOAD_OP: &str = "reloadBehaviours";

/// Whether `id` can name a behaviour type: `namespace.Name`, letters,
/// digits and `_` in each dot-separated part.
pub fn is_behaviour_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('.').collect();
    parts.len() >= 2
        && parts.iter().all(|p| {
            p.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

impl AuthoringSession {
    pub(crate) fn register_behaviour(&mut self, args: &Value) -> Result<Value, Diagnostic> {
        let invalid =
            |m: String| Diagnostic::error(Code::SESSION_INVALID_REQUEST, format!("{OP}: {m}"));
        let id = args["id"]
            .as_str()
            .ok_or_else(|| invalid("`id` (a behaviour type id) is required".into()))?;
        if !is_behaviour_id(id) {
            return Err(invalid(format!(
                "{id:?} is not a behaviour type id (namespace.Name, e.g. game.Spinner)"
            )));
        }
        let description = args["description"].as_str().filter(|d| !d.is_empty());
        let kind = match args["kind"].as_str() {
            None | Some("behaviour") => "behaviour",
            Some("data") => "data",
            Some(other) => {
                return Err(invalid(format!(
                    "`kind` is behaviour or data, not {other:?}"
                )))
            }
        };
        let project = self.project().expect("operate checked the project");
        let dir = project.dir.clone();
        let project_file = dir.join(fynkel_document::project::PROJECT_FILE);
        let mut doc = project.project.document.clone();
        let registered = match doc.root.get_mut("components") {
            Some(registry) if registry.get(id).is_none() => {
                let mut entry = vec![
                    ("kind".to_string(), Node::string(kind)),
                    ("version".to_string(), Node::from_serde(&json!(1))),
                ];
                if let Some(d) = description {
                    entry.push(("description".to_string(), Node::string(d)));
                }
                registry.set(id, Node::object(entry));
                true
            }
            _ => false,
        };
        let label = match kind {
            "data" => format!("Register Data Type {id}"),
            _ => format!("Register Behaviour {id}"),
        };
        if registered {
            let before = std::fs::read_to_string(&project_file)
                .map_err(|e| io(&project_file, format!("{OP}: cannot read: {e}")))?;
            let after = doc.write()?;
            let files = vec![FileRewrite {
                path: project_file,
                before,
                after,
            }];
            // Re-reads the project (the project file is among the files).
            self.reload_after = self.write_rewrites(&files, false, OP)?;
            self.assets_history.push(Done {
                op: AssetOp::Rewrote { files },
                label: label.clone(),
            });
        } else {
            self.reopen(OP)?;
        }
        let known = self
            .project()
            .unwrap()
            .metadata
            .as_ref()
            .and_then(|m| {
                m[if kind == "data" {
                    "dataTypes"
                } else {
                    "behaviours"
                }]
                .as_array()
            })
            .is_some_and(|list| list.iter().any(|b| b["id"] == id));
        Ok(self.finish(
            OP,
            &label,
            None,
            json!({ "id": id, "registered": registered, "inMetadata": known }),
        ))
    }

    pub(crate) fn reload_behaviours(&mut self) -> Result<Value, Diagnostic> {
        self.reopen(RELOAD_OP)?;
        let types: Vec<Value> = self
            .project()
            .unwrap()
            .metadata
            .as_ref()
            .and_then(|m| m["behaviours"].as_array())
            .map(|list| list.iter().map(|b| b["id"].clone()).collect())
            .unwrap_or_default();
        Ok(self.finish(
            RELOAD_OP,
            "Reload Behaviours",
            None,
            json!({ "types": types }),
        ))
    }

    /// Reads the project again (metadata, schemas, schema sources); open
    /// documents keep their handles and edits.
    fn reopen(&mut self, op: &str) -> Result<(), Diagnostic> {
        let dir = self
            .project()
            .expect("operate checked the project")
            .dir
            .clone();
        let reopened = OpenProject::open(&dir).map_err(|diags| {
            diags.into_iter().next().unwrap_or_else(|| {
                Diagnostic::error(
                    Code::SESSION_INVALID_REQUEST,
                    format!("{op}: the project no longer opens"),
                )
            })
        })?;
        self.project = Some(reopened);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::is_behaviour_id;

    #[test]
    fn behaviour_ids() {
        assert!(is_behaviour_id("game.Spinner"));
        assert!(is_behaviour_id("my_game.enemies.Boss2"));
        assert!(!is_behaviour_id("Spinner"));
        assert!(!is_behaviour_id("game."));
        assert!(!is_behaviour_id("game.2Fast"));
        assert!(!is_behaviour_id("game.Spin ner"));
    }
}
