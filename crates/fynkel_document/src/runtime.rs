//! Scene document → runtime description (blueprint §19.2c item 5): turns a
//! validated [`Scene`] into the typed `fynkel_core::SceneDescription` that
//! `World::load_scene` instantiates. Unknown component types are skipped
//! with the same `FYN-D0205` warning the validator gives; values that do
//! not decode are `FYN-D0206` errors (they cannot happen after a clean
//! `Scene::validate`, but a caller may skip validation).

use fynkel_contracts::{Code, Diagnostic, Severity, Uuid};
use fynkel_core::{EntityDescription, SceneDescription};

use crate::generated::decode_component;
use crate::scene::Scene;

#[derive(Clone, Debug, PartialEq)]
pub struct DescribedScene {
    pub description: SceneDescription,
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
pub fn describe(scene: &Scene) -> DescribedScene {
    let mut diagnostics = Vec::new();
    let doc = scene.document.path.as_deref();
    let mut entities = Vec::with_capacity(scene.entities.len());
    for e in &scene.entities {
        let mut components = Vec::with_capacity(e.components.len());
        for c in &e.components {
            match decode_component(&c.type_id, &c.data) {
                None => diagnostics.push(diag(
                    Code::SCENE_UNKNOWN_COMPONENT_TYPE,
                    Severity::Warning,
                    format!(
                        "unknown component type \"{}\" is not instantiated (data kept)",
                        c.type_id
                    ),
                    doc,
                    &format!("{}/type", c.pointer),
                    scene,
                )),
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
