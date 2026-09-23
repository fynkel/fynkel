//! Scene document → runtime description (blueprint §19.2c item 5): turns a
//! validated [`Scene`] into the typed `fynkel_core::SceneDescription` that
//! `World::load_scene` instantiates, plus the authored behaviours
//! ([`AuthoredBehaviour`]) the Dart SDK instantiates from the load report
//! (M2-16). The cooker describes a scene to check it and to record its
//! behaviour types; the runtime (`fynkel_assets`) describes the expanded
//! chunk it reads from the cache. Nothing here opens a file: reading and
//! expanding source scenes for the runtime ended with M3-03 (D2). A non-native component whose type the schema set knows as a
//! behaviour is one of those; any other unknown type is skipped with the
//! same `FYN-D0205` warning the validator gives (its data is still handed
//! over, so an SDK that registered the type can attach it anyway). Values
//! that do not decode are `FYN-D0206` errors (they cannot happen after a
//! clean `Scene::validate`, but a caller may skip validation).

use std::path::Path;

use fynkel_contracts::{Code, Diagnostic, Severity, Uuid};
use fynkel_core::{EntityDescription, SceneDescription};

use crate::generated::decode_component;
use crate::project::Project;
use crate::scene::Scene;
use crate::SchemaSet;

/// One scene component of a non-native type: what the SDK needs to
/// `registry.instantiate(type_id, fields: data)` and attach it to the
/// spawned entity.
#[derive(Clone, Debug, PartialEq)]
pub struct AuthoredBehaviour {
    /// The entity's authoring id (spawned entity looked up through
    /// `LoadedScene::handle_of`).
    pub entity: Uuid,
    /// The component's authoring id.
    pub component: Uuid,
    pub type_id: String,
    pub version: u32,
    /// The authored `data` object as JSON.
    pub data: serde_json::Value,
    /// JSON pointer of the component in its document.
    pub pointer: String,
    /// Whether the schema set knew the type (else `FYN-D0205` was reported).
    pub known: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DescribedScene {
    pub description: SceneDescription,
    /// Document order (entity, then component order).
    pub behaviours: Vec<AuthoredBehaviour>,
    pub diagnostics: Vec<Diagnostic>,
}

impl DescribedScene {
    pub fn has_errors(&self) -> bool {
        fynkel_contracts::has_errors(&self.diagnostics)
    }
}

fn to_uuid(u: &uuid::Uuid) -> Uuid {
    Uuid::from_bytes(*u.as_bytes())
}

/// Builds the description in document order (entities as listed; components
/// as listed). Every diagnostic carries the document path and a pointer.
/// `schemas` says which non-native types are behaviours.
pub fn describe(scene: &Scene, schemas: &SchemaSet) -> DescribedScene {
    let mut diagnostics = Vec::new();
    let mut behaviours = Vec::new();
    let doc = scene.document.path.as_deref();
    let mut entities = Vec::with_capacity(scene.entities.len());
    for e in &scene.entities {
        let mut components = Vec::with_capacity(e.components.len());
        for c in &e.components {
            match decode_component(&c.type_id, &c.data) {
                None => {
                    let known = schemas.behaviour(&c.type_id).is_some();
                    if !known {
                        diagnostics.push(diag(
                            Code::SCENE_UNKNOWN_COMPONENT_TYPE,
                            Severity::Warning,
                            format!(
                                "unknown component type \"{}\" is not instantiated (data kept)",
                                c.type_id
                            ),
                            doc,
                            &format!("{}/type", c.pointer),
                            scene,
                        ));
                    }
                    behaviours.push(AuthoredBehaviour {
                        entity: to_uuid(&e.id),
                        component: to_uuid(&c.id),
                        type_id: c.type_id.clone(),
                        version: c.version,
                        data: c.data.to_serde(),
                        pointer: c.pointer.clone(),
                        known,
                    });
                }
                Some(Err(err)) => diagnostics.push(diag(
                    Code::SCENE_COMPONENT_DATA,
                    Severity::Error,
                    format!("{}: {}", c.type_id, err.message),
                    doc,
                    &format!("{}/data{}", c.pointer, err.pointer),
                    scene,
                )),
                Some(Ok(value)) => components.push(value),
            }
        }
        entities.push(EntityDescription {
            id: to_uuid(&e.id),
            name: e.name.clone(),
            active: e.active,
            parent: e.parent.as_ref().map(to_uuid),
            children: e.children.iter().map(to_uuid).collect(),
            components,
        });
    }
    DescribedScene {
        description: SceneDescription {
            id: to_uuid(&scene.document.envelope.id),
            name: scene.document.envelope.name.clone(),
            entities,
        },
        behaviours,
        diagnostics,
    }
}

fn diag(
    code: Code,
    severity: Severity,
    message: String,
    doc: Option<&str>,
    pointer: &str,
    scene: &Scene,
) -> Diagnostic {
    let mut d = Diagnostic::new(code, severity, message).with_pointer(pointer);
    if let Some(doc) = doc {
        d = d.with_document(doc);
    }
    // A missing field has no node of its own: anchor at the nearest parent.
    let mut anchor = pointer.to_string();
    loop {
        if let Some(span) = scene.document.root.pointer_span(&anchor) {
            d = d.at(span.start.line, span.start.column);
            break;
        }
        match anchor.rfind('/') {
            Some(i) => anchor.truncate(i),
            None => break,
        }
    }
    d
}

/// Where a project keeps its generated behaviour schemas (`fynkel codegen`
/// pipeline (b)): `<dir>/schemas/behaviours/<id>.schema.json`.
pub const PROJECT_SCHEMAS_DIR: &str = "schemas";

/// The embedded schemas plus the project's own (`<dir>/schemas`), with every
/// behaviour the project registry declares made known.
pub fn project_schemas(dir: &Path, project: &Project) -> Result<SchemaSet, Diagnostic> {
    project.schemas(&SchemaSet::embedded(), dir)
}
