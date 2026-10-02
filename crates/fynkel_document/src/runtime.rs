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
#[cfg(feature = "validate")]
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
    pub registered_schemas: Vec<fynkel_core::ComponentSchema>,
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
#[cfg(feature = "validate")]
pub fn describe(scene: &Scene, schemas: &SchemaSet) -> DescribedScene {
    let mut described = describe_registered(
        scene,
        |type_id| schemas.behaviour(type_id).is_some(),
        |type_id| {
            schemas
                .component(type_id)
                .is_some_and(|s| s.numeric_id == 0)
        },
    );
    match schemas.registered_schemas() {
        Ok(schemas) => described.registered_schemas = schemas,
        Err(message) => described.diagnostics.push(Diagnostic::new(
            Code::SCENE_COMPONENT_DATA,
            Severity::Error,
            message,
        )),
    }
    described
}

/// [`describe`] with the behaviour types given as a test: a cooked load
/// (ADR-0041) answers from the cook's `behaviourTypes` and builds no
/// schema set.
pub fn describe_with(scene: &Scene, is_behaviour: impl Fn(&str) -> bool) -> DescribedScene {
    describe_registered(scene, is_behaviour, |_| false)
}

pub fn describe_registered(
    scene: &Scene,
    is_behaviour: impl Fn(&str) -> bool,
    is_registered: impl Fn(&str) -> bool,
) -> DescribedScene {
    let mut diagnostics = Vec::new();
    let mut behaviours = Vec::new();
    let doc = scene.document.path.as_deref();
    let mut entities = Vec::with_capacity(scene.entities.len());
    for e in &scene.entities {
        let mut components = Vec::with_capacity(e.components.len());
        let mut registered = Vec::new();
        for c in &e.components {
            if is_registered(&c.type_id) {
                match registered_description(&c.type_id, c.version, &c.data.to_serde()) {
                    Ok(value) => registered.push(value),
                    Err(message) => diagnostics.push(diag(
                        Code::SCENE_COMPONENT_DATA,
                        Severity::Error,
                        message,
                        doc,
                        &format!("{}/data", c.pointer),
                        scene,
                    )),
                }
                continue;
            }
            match decode_component(&c.type_id, &c.data) {
                None => {
                    let known = is_behaviour(&c.type_id);
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
            registered,
            id: to_uuid(&e.id),
            name: e.name.clone(),
            active: e.active,
            parent: e.parent.as_ref().map(to_uuid),
            children: e.children.iter().map(to_uuid).collect(),
            components,
        });
    }
    DescribedScene {
        registered_schemas: Vec::new(),
        description: SceneDescription {
            id: to_uuid(&scene.document.envelope.id),
            name: scene.document.envelope.name.clone(),
            entities,
        },
        behaviours,
        diagnostics,
    }
}

/// Decode authored custom data without executing game code. Defaults, version
/// compatibility and field kinds are checked against the world's registry at
/// activation; authoring retains the original JSON, including unknown types.
pub fn registered_description(
    id: &str,
    version: u32,
    data: &serde_json::Value,
) -> Result<fynkel_core::RegisteredDescription, String> {
    use fynkel_core::{AuthoredRegisteredValue as A, ScalarValue as V};
    let object = data.as_object().ok_or("component data must be an object")?;
    let mut fields = std::collections::BTreeMap::new();
    for (name, value) in object {
        let value = match value {
            serde_json::Value::Null => A::Object(fynkel_contracts::ObjectRef::NONE),
            serde_json::Value::Bool(v) => A::Scalar(V::Bool(*v)),
            serde_json::Value::Number(v) => A::Scalar(if let Some(i) = v.as_i64() {
                V::Int(i)
            } else {
                V::Float(v.as_f64().ok_or("invalid number")?)
            }),
            serde_json::Value::String(v) => A::Scalar(V::Text(v.clone())),
            serde_json::Value::Object(v) if v.len() == 1 && v.contains_key("entity") => {
                A::Object(fynkel_contracts::ObjectRef {
                    id: v["entity"]
                        .as_str()
                        .ok_or("invalid object reference")?
                        .parse()
                        .map_err(|_| "invalid object UUID")?,
                    entity: fynkel_contracts::Entity::NULL,
                })
            }
            serde_json::Value::Object(v)
                if v.contains_key("asset") && v.keys().all(|k| k == "asset" || k == "sub") =>
            {
                A::Scalar(V::Asset(fynkel_core::AssetRef::with_sub(
                    v["asset"]
                        .as_str()
                        .ok_or("invalid asset reference")?
                        .parse()
                        .map_err(|_| "invalid asset UUID")?,
                    v.get("sub")
                        .map(|v| v.as_str().ok_or("invalid asset sub-id"))
                        .transpose()?
                        .unwrap_or(""),
                )))
            }
            _ => return Err(format!("{id}.{name}: unsupported registered value layout")),
        };
        fields.insert(name.clone(), value);
    }
    Ok(fynkel_core::RegisteredDescription {
        id: id.into(),
        version,
        fields,
    })
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
#[cfg(feature = "validate")]
pub fn project_schemas(dir: &Path, project: &Project) -> Result<SchemaSet, Diagnostic> {
    project.schemas(&SchemaSet::embedded(), dir)
}

/// The generated registry metadata is shared by document validation and editor
/// preview registration. Runtime games register the matching typed declaration.
pub fn registered_schema(
    value: &serde_json::Value,
) -> Result<fynkel_core::ComponentSchema, String> {
    use fynkel_core::{RegisteredField, ScalarValue as V};
    let id = value["id"]
        .as_str()
        .ok_or("registration requires id")?
        .to_string();
    let version = value["version"]
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .filter(|v| *v > 0)
        .ok_or("registration requires positive version")?;
    let mut fields = Vec::new();
    for field in value["fields"]
        .as_array()
        .ok_or("registration requires fields")?
    {
        let name = field["name"]
            .as_str()
            .ok_or("field requires name")?
            .to_string();
        let v = &field["default"];
        let default = match field["kind"].as_str() {
            Some("number") => V::Float(v.as_f64().ok_or("number requires default")?),
            Some("integer") => V::Int(v.as_i64().ok_or("integer requires default")?),
            Some("boolean") => V::Bool(v.as_bool().ok_or("boolean requires default")?),
            Some("string") => V::Text(v.as_str().ok_or("string requires default")?.into()),
            Some("gameObject") if v.is_null() => V::Object(None),
            Some("assetRef") => V::Asset(fynkel_core::AssetRef::with_sub(
                v["asset"]
                    .as_str()
                    .ok_or("asset requires default")?
                    .parse()
                    .map_err(|_| "invalid asset UUID")?,
                v.get("sub").and_then(|v| v.as_str()).unwrap_or(""),
            )),
            _ => return Err(format!("{id}.{name}: unsupported field or default")),
        };
        fields.push(RegisteredField { name, default });
    }
    Ok(fynkel_core::ComponentSchema {
        id,
        version,
        fields,
    })
}
