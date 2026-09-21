//! Read-only queries by name, for the ABI: `query(kind, args)` → JSON.
//! Every mutation has its own entry point; nothing here changes the
//! session.

use std::str::FromStr;

use fynkel_contracts::{AssetRef, Code, Diagnostic};
use fynkel_document::asset::resolved_json;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{AuthoringSession, Handle, COMMANDS};

/// The query kinds, with the arguments each reads:
///
/// | kind | args | result |
/// | --- | --- | --- |
/// | `project` | | [`crate::OpenProject::json`] |
/// | `documents` | | the open documents' states |
/// | `document` | `handle`, `root?` (default true) | [`AuthoringSession::document_json`] / `state_json` |
/// | `inspect` | `handle` | [`AuthoringSession::inspect_json`] |
/// | `validate` | `handle` | `{ ok, diagnostics }` |
/// | `validateProject` | | the `ProjectReport` JSON |
/// | `assets` | `kind?` (`document`/`source`), `document?` (a kind name) | the entries |
/// | `asset` | `asset` (UUID text), `sub?` | the resolved entry |
/// | `dependents` | `asset` (UUID text) | the references to it |
/// | `assetDetail` | `asset` (UUID text) | the entry plus the file's size and time, its sidecar (source) or content summary (document), its dependents ([`AuthoringSession::asset_detail`]) |
/// | `assetHistory` | | the asset operations' undo/redo state ([`crate::AssetHistory::json`]) |
/// | `schemas` | | component and behaviour types with schemas |
/// | `schema` | `type` | one schema |
/// | `metadata` | | `behaviours.g.json` or null |
/// | `migrations` | | [`crate::OpenProject::migrations_json`]: the outdated documents and what `fynkel migrate` would do |
/// | `defaults` | `type` | `{ type, data }`: a native component's `Default` as document data ([`crate::component_default`]) |
/// | `revision` | `path` | the revision and identity of a document file on disk now ([`AuthoringSession::file_revision`]) |
/// | `commands` | | the command names |
/// | `operations` | | the asset operation names ([`crate::OPERATIONS`]) |
pub const KINDS: &[&str] = &[
    "project",
    "documents",
    "document",
    "inspect",
    "validate",
    "validateProject",
    "assets",
    "asset",
    "dependents",
    "assetDetail",
    "assetHistory",
    "schemas",
    "schema",
    "metadata",
    "migrations",
    "defaults",
    "revision",
    "commands",
    "operations",
];

impl AuthoringSession {
    /// Answers one query. `FYN-T0004` for an unknown kind, `FYN-T0003` for
    /// a missing or malformed argument, `FYN-T0001`/`T0002` when there is
    /// no project or no such document.
    pub fn query(&self, kind: &str, args: &Value) -> Result<Value, Vec<Diagnostic>> {
        let one = |d: Diagnostic| vec![d];
        match kind {
            "project" => Ok(self.project().ok_or_else(no_project).map_err(one)?.json()),
            "documents" => Ok(Value::Array(self.documents_json())),
            "document" => {
                let handle = handle_arg(args)?;
                if let Some(ids) = args.get("ids") {
                    let ids: Vec<String> = ids
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(Value::as_str)
                                .map(str::to_string)
                                .collect()
                        })
                        .ok_or_else(|| {
                            one(invalid("document: \"ids\" must be an array of entity ids"))
                        })?;
                    self.entities_json(handle, &ids).map_err(one)
                } else if args.get("root").and_then(Value::as_bool).unwrap_or(true) {
                    self.document_json(handle).map_err(one)
                } else {
                    self.state_json(handle).map_err(one)
                }
            }
            "inspect" => self.inspect_json(handle_arg(args)?).map_err(one),
            "validate" => {
                let diags = self.validate(handle_arg(args)?).map_err(one)?;
                Ok(fynkel_document::diagnostics_json(&diags))
            }
            "validateProject" => Ok(self.validate_project().map_err(one)?.json()),
            "assets" => {
                let p = self.project().ok_or_else(no_project).map_err(one)?;
                let kind = args.get("kind").and_then(Value::as_str);
                let document = args.get("document").and_then(Value::as_str);
                if let Some(k) = kind {
                    if k != "document" && k != "source" {
                        return Err(one(invalid(format!(
                            "assets: kind must be \"document\" or \"source\", not {k:?}"
                        ))));
                    }
                }
                Ok(Value::Array(p.assets_json(kind, document)))
            }
            "asset" => {
                let p = self.project().ok_or_else(no_project).map_err(one)?;
                let id = uuid_arg(args, "asset")?;
                let mut r = AssetRef::new(crate::project::to_contract(id));
                if let Some(sub) = args.get("sub").and_then(Value::as_str) {
                    r.sub = sub.to_string();
                }
                let resolved = p.assets.resolve(&r).map_err(one)?;
                Ok(resolved_json(&resolved))
            }
            "dependents" => {
                let p = self.project().ok_or_else(no_project).map_err(one)?;
                let id = uuid_arg(args, "asset")?;
                Ok(Value::Array(
                    p.dependents(id).iter().map(|r| r.json()).collect(),
                ))
            }
            "assetDetail" => {
                self.project().ok_or_else(no_project).map_err(one)?;
                let id = uuid_arg(args, "asset")?;
                self.asset_detail(id)
            }
            "assetHistory" => {
                self.project().ok_or_else(no_project).map_err(one)?;
                Ok(self.asset_history().json())
            }
            "schemas" => Ok(self
                .project()
                .ok_or_else(no_project)
                .map_err(one)?
                .schemas_json()),
            "schema" => {
                let p = self.project().ok_or_else(no_project).map_err(one)?;
                let ty = args
                    .get("type")
                    .and_then(Value::as_str)
                    .ok_or_else(|| one(invalid("schema: \"type\" (a type id) is required")))?;
                p.schema(ty).cloned().ok_or_else(|| {
                    one(Diagnostic::error(
                        Code::SESSION_INVALID_REQUEST,
                        format!("no schema for type {ty}"),
                    )
                    .with_suggestion(
                        "component types come from the embedded schemas, behaviour types from the project's schemas/ (fynkel codegen)",
                    ))
                })
            }
            "metadata" => Ok(self
                .project()
                .ok_or_else(no_project)
                .map_err(one)?
                .metadata
                .clone()
                .unwrap_or(Value::Null)),
            "migrations" => Ok(self
                .project()
                .ok_or_else(no_project)
                .map_err(one)?
                .migrations_json()),
            "defaults" => {
                let ty = args.get("type").and_then(Value::as_str).ok_or_else(|| {
                    one(invalid(
                        "defaults: \"type\" (a component type id) is required",
                    ))
                })?;
                let data = crate::component_default(ty).ok_or_else(|| {
                    one(invalid(format!(
                        "defaults: {ty:?} is not a built-in authored component"
                    ))
                    .with_suggestion("a behaviour's defaults come from its schema"))
                })?;
                Ok(json!({ "type": ty, "data": data.to_serde() }))
            }
            "revision" => {
                let path = args.get("path").and_then(Value::as_str).ok_or_else(|| {
                    one(invalid("revision: \"path\" (a document file) is required"))
                })?;
                self.file_revision(path)
            }
            "commands" => Ok(json!(COMMANDS)),
            "operations" => Ok(json!(crate::OPERATIONS)),
            other => Err(one(Diagnostic::error(
                Code::SESSION_UNKNOWN_QUERY,
                format!("unknown query {other:?}"),
            )
            .with_suggestion(format!("one of: {}", KINDS.join(", "))))),
        }
    }
}

fn no_project() -> Diagnostic {
    Diagnostic::error(Code::SESSION_NO_PROJECT, "no project is open")
        .with_suggestion("open a project directory first")
}

fn invalid(message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(Code::SESSION_INVALID_REQUEST, message)
}

fn handle_arg(args: &Value) -> Result<Handle, Vec<Diagnostic>> {
    args.get("handle")
        .and_then(Value::as_u64)
        .and_then(|h| Handle::try_from(h).ok())
        .ok_or_else(|| {
            vec![invalid(
                "\"handle\" (an open document's handle) is required",
            )]
        })
}

fn uuid_arg(args: &Value, key: &str) -> Result<Uuid, Vec<Diagnostic>> {
    let text = args
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| vec![invalid(format!("\"{key}\" (UUID text) is required"))])?;
    Uuid::from_str(text).map_err(|e| vec![invalid(format!("\"{key}\": {e}"))])
}
