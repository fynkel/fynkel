//! Data assets (ADR-0027): `.data.json` holding one instance of a
//! game-declared type, checked against `schemas/data/<type>.schema.json`
//! (what `fynkel codegen` writes from an `@FynkelDataAsset` class), and
//! in a project: validated with the rest, a registry entry of kind `data`
//! declaring the type, and `A0007` for a field's reference of the wrong kind.

use std::path::{Path, PathBuf};

use fynkel_document::{
    validate_project_with, Code, DataDocument, Document, DocumentKind, SchemaSet, Severity,
};

/// With the importer's provider, as `fynkel validate` (the fixture's Rig
/// scene instances an imported hierarchy).
fn validate_project(
    dir: &Path,
    schemas: &SchemaSet,
    lock: Option<&fynkel_document::Lock>,
) -> fynkel_document::ProjectReport {
    validate_project_with(
        dir,
        schemas,
        lock,
        &fynkel_import::ImportedPrefabs::default(),
    )
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The generator's shape for `game.Weapon { int damage (min 0); double
/// cooldown; AssetRef? projectile (asset(prefab)); List<AssetRef> sounds
/// (asset(audio)) }`.
fn weapon_schema(version: u32) -> serde_json::Value {
    let asset_ref = "https://fynkel.dev/schemas/envelope.schema.json#/$defs/assetRef";
    serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://fynkel.dev/schemas/data/game.Weapon.schema.json",
        "title": "game.Weapon",
        "type": "object",
        "properties": {
            "damage": { "type": "integer", "minimum": 0, "default": 1,
                        "x-fynkel": { "type": "integer", "min": 0 } },
            "cooldown": { "type": "number", "default": 0.5, "x-fynkel": { "type": "number" } },
            "projectile": {
                "anyOf": [{ "$ref": asset_ref }, { "type": "null" }],
                "x-fynkel": { "type": "assetRef", "nullable": true, "kind": "asset(prefab)" }
            },
            "sounds": {
                "type": "array",
                "items": { "$ref": asset_ref,
                           "x-fynkel": { "type": "assetRef", "kind": "asset(audio)" } },
                "x-fynkel": { "type": "list", "items": "assetRef" }
            }
        },
        "additionalProperties": false,
        "x-fynkel": { "kind": "data", "id": "game.Weapon", "version": version,
                      "dart": "package:demo/weapon.dart#Weapon", "menu": "Game/Weapon" }
    })
}

fn schemas_with_weapon(version: u32) -> SchemaSet {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("fynkel-data-schemas-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("data")).unwrap();
    std::fs::write(
        dir.join("data/game.Weapon.schema.json"),
        serde_json::to_string_pretty(&weapon_schema(version)).unwrap(),
    )
    .unwrap();
    let set = SchemaSet::embedded().extended_with(&dir).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    set
}

fn doc(body: &str) -> DataDocument {
    let text = format!(
        r#"{{"$schema":"https://fynkel.dev/schemas/data.schema.json","formatVersion":1,"id":"c0000000-0000-4000-8000-000000000a01","name":"Sword"{body}}}"#
    );
    DataDocument::from_document(
        Document::parse(DocumentKind::Data, Some("sword.data.json"), &text).unwrap(),
    )
}

fn codes(d: &DataDocument, schemas: &SchemaSet) -> Vec<(String, String)> {
    d.validate(schemas)
        .iter()
        .map(|d| (d.code.0.to_string(), d.pointer.clone().unwrap_or_default()))
        .collect()
}

#[test]
fn a_data_asset_is_checked_against_its_type() {
    assert_eq!(
        DocumentKind::from_path(Path::new("a/sword.data.json")),
        Some(DocumentKind::Data)
    );
    let schemas = schemas_with_weapon(1);
    let info = schemas.data_type("game.Weapon").expect("loaded by type id");
    assert_eq!(info.version, 1);
    assert!(
        schemas.behaviour("game.Weapon").is_none(),
        "a data type is not a behaviour"
    );
    assert_eq!(schemas.asset_fields("game.Weapon").len(), 2);

    let sword = doc(r#","type":"game.Weapon","version":1,"data":{"damage":3,"cooldown":0.25}"#);
    assert_eq!((sword.type_id.as_str(), sword.version), ("game.Weapon", 1));
    assert_eq!(codes(&sword, &schemas), []);

    // Data that does not fit: a negative damage, a field the type lacks.
    let bad = doc(r#","type":"game.Weapon","version":1,"data":{"damage":-1,"range":4}"#);
    let found = codes(&bad, &schemas);
    assert!(
        found.contains(&("FYN-D1002".into(), "/data/damage".into())),
        "{found:?}"
    );
    assert!(found.iter().all(|(c, _)| c == "FYN-D1002"), "{found:?}");

    // Written for another version of the type.
    let old = doc(r#","type":"game.Weapon","version":2,"data":{}"#);
    assert_eq!(
        codes(&old, &schemas),
        [("FYN-D1002".into(), "/version".into())]
    );

    // An unknown type is a warning; the data is kept.
    let unknown = doc(r#","type":"game.Shield","version":1,"data":{"x":1}"#);
    let d = unknown.validate(&schemas);
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(
        (d[0].code, d[0].severity),
        (Code::DATA_UNKNOWN_TYPE, Severity::Warning)
    );

    // A malformed envelope body: the schema reports it (and the lenient
    // read, where the schema did not).
    let broken = doc(r#","type":"Weapon","version":0,"data":[]"#);
    let found = codes(&broken, &schemas);
    for pointer in ["/type", "/version", "/data"] {
        assert!(
            found.iter().any(|(_, p)| p == pointer),
            "{pointer}: {found:?}"
        );
    }
    assert!(broken.type_id.is_empty());
}

#[test]
fn new_data_documents_and_declared_types() {
    let fresh = Document::new(DocumentKind::Data, "Thing");
    let text = fresh.write().unwrap();
    assert!(text.contains(r#""type": "game.Data""#), "{text}");

    let sword = Document::new_data("Sword", "game.Weapon", 1, None);
    let view = DataDocument::from_document(sword.clone());
    assert_eq!(
        view.data().and_then(|d| d.as_object()).map(|m| m.len()),
        Some(0)
    );
    assert_eq!(codes(&view, &schemas_with_weapon(1)), []);

    // A declared type (a registry entry of kind `data`) takes any object.
    let mut schemas = SchemaSet::embedded();
    schemas.declare_data("game.Weapon", 1);
    let any = doc(r#","type":"game.Weapon","version":1,"data":{"anything":true}"#);
    assert_eq!(codes(&any, &schemas), []);
    assert!(!schemas.data_type("game.Weapon").unwrap().has_schema);
}

/// In a project: a data asset validates with the rest, its type comes from
/// the project's `schemas/data/`, and a field naming the wrong kind of
/// asset is `A0007` (a material where a prefab belongs, a scene in a list
/// of sounds).
#[test]
fn data_assets_validate_in_a_project() {
    let dir = std::env::temp_dir().join(format!("fynkel-data-project-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(
        &root().join("crates/fynkel_document/fixtures/project"),
        &dir,
    );
    std::fs::create_dir_all(dir.join("schemas/data")).unwrap();
    std::fs::write(
        dir.join("schemas/data/game.Weapon.schema.json"),
        serde_json::to_string_pretty(&weapon_schema(1)).unwrap(),
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("assets/data")).unwrap();
    let write = |data: &str| {
        let text = format!(
            r#"{{"$schema":"https://fynkel.dev/schemas/data.schema.json","formatVersion":1,"id":"c0000000-0000-4000-8000-000000000a01","name":"Sword","type":"game.Weapon","version":1,"data":{data}}}"#
        );
        std::fs::write(dir.join("assets/data/sword.data.json"), text).unwrap();
    };
    let schemas = SchemaSet::load(&root().join("schemas")).unwrap();
    let codes = || {
        validate_project(&dir, &schemas, None)
            .diagnostics
            .iter()
            .filter(|d| d.code != Code::LOCK_MISMATCH)
            .map(|d| (d.code.0.to_string(), d.pointer.clone().unwrap_or_default()))
            .collect::<Vec<_>>()
    };

    write(r#"{"damage":3}"#);
    let report = validate_project(&dir, &schemas, None);
    assert_eq!(codes(), Vec::<(String, String)>::new());
    assert_eq!(report.documents, 12, "the data asset counts");

    // A crate prefab is right; a material is not.
    write(r#"{"projectile":{"asset":"c0000000-0000-4000-8000-000000000020"}}"#);
    assert_eq!(codes(), [("FYN-A0007".into(), "/data/projectile".into())]);
    // The tone is a sound; the main scene is not.
    let scene = "c0000000-0000-4000-8000-000000000030";
    let tone = "c0000000-0000-4000-8000-000000000006";
    write(&format!(
        r#"{{"sounds":[{{"asset":"{tone}"}},{{"asset":"{scene}"}}]}}"#
    ));
    assert_eq!(codes(), [("FYN-A0007".into(), "/data/sounds/1".into())]);
    write(r#"{"damage":-2}"#);
    assert_eq!(codes(), [("FYN-D1002".into(), "/data/damage".into())]);
    std::fs::remove_dir_all(&dir).unwrap();
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            copy_dir(&path, &to.join(&name));
        } else {
            std::fs::copy(&path, to.join(&name)).unwrap();
        }
    }
}
