//! Scene document → runtime description (blueprint §19.2c item 5): turns a
//! validated [`Scene`] into the typed `fynkel_core::SceneDescription` that
//! `World::load_scene` instantiates, plus the authored behaviours
//! ([`AuthoredBehaviour`]) the Dart SDK instantiates from the load report
//! (M2-16). A non-native component whose type the schema set knows as a
//! behaviour is one of those; any other unknown type is skipped with the
//! same `FYN-D0205` warning the validator gives (its data is still handed
//! over, so an SDK that registered the type can attach it anyway). Values
//! that do not decode are `FYN-D0206` errors (they cannot happen after a
//! clean `Scene::validate`, but a caller may skip validation).

use std::path::{Path, PathBuf};

use fynkel_contracts::{Code, Diagnostic, Severity, Uuid};
use fynkel_core::{EntityDescription, SceneDescription};

use crate::asset::{AssetDatabase, AssetKind};
use crate::generated::decode_component;
use crate::prefab::{expand, Prefab, PrefabLibrary};
use crate::project::Project;
use crate::scene::Scene;
use crate::{Document, DocumentKind, SchemaSet};

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

/// Reads, validates (embedded schemas), expands prefab instances (from
/// `prefabs`, a directory scanned for `*.prefab.json`) and describes a
/// scene document. `Err` carries every diagnostic when any is an error;
/// `Ok` carries the description, the authored behaviours and the warnings.
/// No project is involved, so every non-native type is an unknown one
/// (`FYN-D0205`) whose data is still handed over.
pub fn read_scene(path: &Path, prefabs: Option<&Path>) -> Result<DescribedScene, Vec<Diagnostic>> {
    let (library, scan) = match prefabs {
        Some(dir) => PrefabLibrary::scan(dir),
        None => (PrefabLibrary::empty(), Vec::new()),
    };
    read_scene_with(path, &library, &SchemaSet::embedded(), scan)
}

fn read_scene_with(
    path: &Path,
    library: &PrefabLibrary,
    schemas: &SchemaSet,
    mut diagnostics: Vec<Diagnostic>,
) -> Result<DescribedScene, Vec<Diagnostic>> {
    let doc = Document::read(path, Some(DocumentKind::Scene)).map_err(|mut e| {
        e.extend(std::mem::take(&mut diagnostics));
        e
    })?;
    let mut scene = Scene::from_document(doc);
    let mut validation = scene.validate(schemas);
    diagnostics.append(&mut validation);
    if scene.instances().next().is_some() {
        let expanded = expand(&scene, library, schemas);
        scene = expanded.scene();
        diagnostics.extend(expanded.diagnostics);
    }
    if fynkel_contracts::has_errors(&diagnostics) {
        return Err(diagnostics);
    }
    let described = describe(&scene, schemas);
    // `describe` repeats the unknown-type warning; keep one per pointer.
    for d in described.diagnostics {
        if !diagnostics
            .iter()
            .any(|x| x.code == d.code && x.pointer == d.pointer)
        {
            diagnostics.push(d);
        }
    }
    if fynkel_contracts::has_errors(&diagnostics) {
        return Err(diagnostics);
    }
    Ok(DescribedScene {
        description: described.description,
        behaviours: described.behaviours,
        diagnostics,
    })
}

/// A project's startup scene, ready for `World::load_scene`.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadedProject {
    pub project_name: String,
    /// The project directory as given.
    pub dir: PathBuf,
    /// The asset root (`project.assets` under `dir`).
    pub assets: PathBuf,
    /// Where `fynkel cook` writes by default: `<dir>/.fynkel/cache`.
    pub cache: PathBuf,
    /// The startup scene's document path.
    pub scene_path: PathBuf,
    pub scene: SceneDescription,
    /// The startup scene's authored behaviours, for the SDK to attach.
    pub behaviours: Vec<AuthoredBehaviour>,
    pub warnings: Vec<Diagnostic>,
}

/// Where a project keeps its generated behaviour schemas (`fynkel codegen`
/// pipeline (b)): `<dir>/schemas/behaviours/<id>.schema.json`.
pub const PROJECT_SCHEMAS_DIR: &str = "schemas";

/// The embedded schemas plus the project's own (`<dir>/schemas`), with every
/// behaviour the project registry declares made known.
pub fn project_schemas(dir: &Path, project: &Project) -> Result<SchemaSet, Diagnostic> {
    project.schemas(&SchemaSet::embedded(), dir)
}

/// Reads `<dir>/fynkel.project.json`, scans its asset root, resolves the
/// startup scene and reads it with every prefab in the database available
/// for expansion. `Err` when the project, the database or the scene has an
/// error. The lock is not checked here (`fynkel project lock --check`).
/// Behaviour types come from `<dir>/schemas/behaviours` and the registry.
pub fn load_project(dir: &Path) -> Result<LoadedProject, Vec<Diagnostic>> {
    let project = Project::read(dir)?;
    let schemas = project_schemas(dir, &project).map_err(|d| vec![d])?;
    let mut diagnostics = project.read_diagnostics().to_vec();
    diagnostics.extend(project.validate(&schemas));
    if fynkel_contracts::has_errors(&diagnostics) {
        return Err(diagnostics);
    }
    let assets = project.assets_dir(dir);
    let (db, scan) = AssetDatabase::scan(&assets, &schemas);
    diagnostics.extend(scan);
    diagnostics.extend(project.validate_against(&db, &schemas));
    if fynkel_contracts::has_errors(&diagnostics) {
        return Err(diagnostics);
    }
    // `validate_against` has already reported an unset or unresolvable
    // startup scene as an error.
    let startup = project
        .startup_scene
        .as_ref()
        .and_then(|r| db.resolve(r).ok())
        .map(|r| r.entry.file(&assets))
        .ok_or_else(|| {
            vec![
                Diagnostic::error(Code::PROJECT_STARTUP_SCENE, "startupScene is unset")
                    .with_document(dir.join(crate::project::PROJECT_FILE).display().to_string()),
            ]
        })?;
    let mut library = PrefabLibrary::empty();
    for e in db.entries() {
        if e.kind != AssetKind::Document(DocumentKind::Prefab) {
            continue;
        }
        match Document::read(&e.file(&assets), Some(DocumentKind::Prefab)) {
            Ok(doc) => library.add(Prefab::from_document(doc)),
            Err(d) => diagnostics.extend(d),
        }
    }
    let described = read_scene_with(&startup, &library, &schemas, diagnostics)?;
    Ok(LoadedProject {
        project_name: project.name().to_string(),
        dir: dir.to_path_buf(),
        assets,
        cache: dir.join(".fynkel").join("cache"),
        scene_path: startup,
        scene: described.description,
        behaviours: described.behaviours,
        warnings: described.diagnostics,
    })
}
