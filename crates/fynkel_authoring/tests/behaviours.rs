//! `registerBehaviour` (roadmap Phase 2, Create → Behaviour) over a copy of
//! the fixture project: the registry gains the type once (one undoable
//! step), and the session re-reads the behaviour metadata and schemas the
//! CLI's codegen wrote, without reopening the project.

use std::path::{Path, PathBuf};

use fynkel_authoring::AuthoringSession;
use fynkel_document::Diagnostic;
use serde_json::{json, Value};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fynkel_document/fixtures/project")
        .canonicalize()
        .unwrap()
}

/// A private copy of the fixture project (removed on drop).
struct Copy {
    dir: PathBuf,
}

impl Copy {
    fn new(name: &str) -> Copy {
        let dir =
            std::env::temp_dir().join(format!("fynkel-behaviours-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        copy_dir(&fixture(), &dir);
        Copy {
            dir: dir.canonicalize().unwrap(),
        }
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.dir.join(rel)).unwrap()
    }

    fn json(&self, rel: &str) -> Value {
        serde_json::from_str(&self.read(rel)).unwrap()
    }
}

impl Drop for Copy {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let src = entry.path();
        let dst = to.join(&name);
        if src.is_dir() {
            copy_dir(&src, &dst);
        } else {
            std::fs::copy(&src, &dst).unwrap();
        }
    }
}

fn open(copy: &Copy) -> AuthoringSession {
    let mut s = AuthoringSession::new();
    s.open_project(&copy.dir)
        .map_err(|d| format!("{d:?}"))
        .unwrap();
    s
}

fn ok(r: Result<Value, Vec<Diagnostic>>) -> Value {
    r.unwrap_or_else(|d| panic!("{d:#?}"))
}

#[test]
fn a_new_behaviour_is_registered_once_and_its_metadata_read_at_once() {
    let copy = Copy::new("register");
    let mut s = open(&copy);
    let doc = s.open_document("assets/scenes/main.scene.json").unwrap();
    assert!(!s
        .project()
        .unwrap()
        .schema_sources
        .contains_key("game.Hover"));

    // What `fynkel codegen` leaves behind for a new `game.Hover`.
    let spinner = copy.read("schemas/behaviours/game.Spinner.schema.json");
    std::fs::write(
        copy.dir.join("schemas/behaviours/game.Hover.schema.json"),
        spinner.replace("game.Spinner", "game.Hover"),
    )
    .unwrap();
    std::fs::create_dir_all(copy.dir.join("lib/generated")).unwrap();
    std::fs::write(
        copy.dir.join("lib/generated/behaviours.g.json"),
        json!({ "behaviours": [{ "id": "game.Hover", "version": 1, "class": "Hover" }] })
            .to_string(),
    )
    .unwrap();

    let v = ok(s.operate(
        "registerBehaviour",
        &json!({ "id": "game.Hover", "description": "Bobs up and down (lib/game/hover.dart)." }),
    ));
    assert_eq!(v["registered"], true, "{v}");
    assert_eq!(v["inMetadata"], true, "{v}");
    assert_eq!(v["label"], "Register Behaviour game.Hover");
    assert_eq!(
        copy.json("fynkel.project.json")["components"]["game.Hover"],
        json!({ "kind": "behaviour", "version": 1, "description": "Bobs up and down (lib/game/hover.dart)." })
    );
    let p = s.project().unwrap();
    assert!(p.schema_sources.contains_key("game.Hover"));
    assert!(p.metadata.is_some());
    // The open document survives the re-read.
    assert!(s.state_json(doc).is_ok());

    // Again: nothing to write, the metadata is read again all the same.
    let again = ok(s.operate("registerBehaviour", &json!({ "id": "game.Hover" })));
    assert_eq!(again["registered"], false);
    assert_eq!(again["canUndo"], true);

    // Undo takes the registry entry back.
    ok(s.operate("undoAsset", &json!({})));
    assert!(copy.json("fynkel.project.json")["components"]
        .get("game.Hover")
        .is_none());

    // Not an id.
    let refused = s
        .operate("registerBehaviour", &json!({ "id": "Hover" }))
        .unwrap_err();
    assert!(refused[0].message.contains("namespace.Name"), "{refused:?}");
}

/// A data type (ADR-0027): `kind: "data"` registers it as one, and its
/// schema (`schemas/data/`) is a source like a behaviour's, listed under
/// `data` for the Inspector.
#[test]
fn a_new_data_type_is_registered_as_data() {
    let copy = Copy::new("register-data");
    let mut s = open(&copy);
    std::fs::create_dir_all(copy.dir.join("schemas/data")).unwrap();
    std::fs::write(
        copy.dir.join("schemas/data/game.Weapon.schema.json"),
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": "https://fynkel.dev/schemas/data/game.Weapon.schema.json",
            "title": "game.Weapon", "type": "object",
            "properties": { "damage": { "type": "integer", "default": 1,
                                        "x-fynkel": { "type": "integer" } } },
            "additionalProperties": false,
            "x-fynkel": { "kind": "data", "id": "game.Weapon", "version": 1, "menu": "Game/Weapon" }
        })
        .to_string(),
    )
    .unwrap();
    std::fs::create_dir_all(copy.dir.join("lib/generated")).unwrap();
    std::fs::write(
        copy.dir.join("lib/generated/behaviours.g.json"),
        json!({ "behaviours": [], "dataTypes": [{ "id": "game.Weapon", "version": 1, "class": "Weapon" }] })
            .to_string(),
    )
    .unwrap();
    let v = ok(s.operate(
        "registerBehaviour",
        &json!({ "id": "game.Weapon", "kind": "data" }),
    ));
    assert_eq!(
        (&v["registered"], &v["inMetadata"], &v["label"]),
        (
            &json!(true),
            &json!(true),
            &json!("Register Data Type game.Weapon")
        ),
        "{v}"
    );
    assert_eq!(
        copy.json("fynkel.project.json")["components"]["game.Weapon"],
        json!({ "kind": "data", "version": 1 })
    );
    let schemas = s.project().unwrap().schemas_json();
    let weapon = &schemas["data"][0];
    assert_eq!(weapon["type"], "game.Weapon", "{schemas}");
    assert_eq!(weapon["schema"]["x-fynkel"]["menu"], "Game/Weapon");
    let refused = s
        .operate(
            "registerBehaviour",
            &json!({ "id": "game.Weapon", "kind": "asset" }),
        )
        .unwrap_err();
    assert!(
        refused[0].message.contains("behaviour or data"),
        "{refused:?}"
    );
}

#[test]
fn reload_behaviours_reads_regenerated_metadata_and_writes_nothing() {
    let copy = Copy::new("reload");
    let mut s = open(&copy);
    let project_file = copy.read("fynkel.project.json");
    std::fs::create_dir_all(copy.dir.join("lib/generated")).unwrap();
    std::fs::write(
        copy.dir.join("lib/generated/behaviours.g.json"),
        json!({ "behaviours": [{ "id": "game.Spinner", "version": 1, "class": "Spinner",
            "fields": [{ "name": "speed2", "type": "number" }] }] })
        .to_string(),
    )
    .unwrap();

    // The schema file gains the field too, with its default.
    let mut schema = copy.json("schemas/behaviours/game.Spinner.schema.json");
    schema["properties"]["speed2"] = json!({ "type": "number", "default": 5.0 });
    std::fs::write(
        copy.dir.join("schemas/behaviours/game.Spinner.schema.json"),
        serde_json::to_string_pretty(&schema).unwrap(),
    )
    .unwrap();

    let v = ok(s.operate("reloadBehaviours", &json!({})));
    assert_eq!(v["types"], json!(["game.Spinner"]), "{v}");
    let schemas = s.project().unwrap().schemas_json();
    let spinner = schemas["behaviours"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["type"] == "game.Spinner")
        .unwrap();
    assert_eq!(
        spinner["schema"]["properties"]["speed2"]["default"], 5.0,
        "{spinner}"
    );
    let metadata = s.project().unwrap().metadata.clone().unwrap();
    assert_eq!(metadata["behaviours"][0]["fields"][0]["name"], "speed2");
    assert_eq!(copy.read("fynkel.project.json"), project_file);
    assert_eq!(v["canUndo"], false);
}

/// A Fynkel package (ADR-0029 §2) at `<copy>/packages/pickups`, a `path`
/// dependency of the copy, as `flutter pub get` and the package's own
/// `fynkel codegen` leave it: one behaviour, `pickups.Pickup`.
fn add_pickups(copy: &Copy) -> PathBuf {
    let root = copy.dir.join("packages/pickups");
    std::fs::create_dir_all(root.join("schemas/behaviours")).unwrap();
    std::fs::create_dir_all(root.join("lib/generated")).unwrap();
    std::fs::write(root.join("pubspec.yaml"), "name: pickups\n").unwrap();
    std::fs::write(root.join("fynkel.package.json"), r#"{"formatVersion": 1}"#).unwrap();
    let spinner = copy.read("schemas/behaviours/game.Spinner.schema.json");
    std::fs::write(
        root.join("schemas/behaviours/pickups.Pickup.schema.json"),
        spinner.replace("game.Spinner", "pickups.Pickup"),
    )
    .unwrap();
    std::fs::write(
        root.join("lib/generated/behaviours.g.json"),
        json!({ "behaviours": [{ "id": "pickups.Pickup", "version": 1, "class": "Pickup" }], "dataTypes": [] })
            .to_string(),
    )
    .unwrap();
    std::fs::write(
        copy.dir.join("pubspec.yaml"),
        "name: my_game\ndependencies:\n  pickups:\n    path: packages/pickups\n",
    )
    .unwrap();
    std::fs::create_dir_all(copy.dir.join(".dart_tool")).unwrap();
    std::fs::write(
        copy.dir.join(".dart_tool/package_config.json"),
        json!({ "configVersion": 2, "packages": [
            { "name": "my_game", "rootUri": "../", "packageUri": "lib/" },
            { "name": "pickups", "rootUri": "../packages/pickups", "packageUri": "lib/" },
        ]})
        .to_string(),
    )
    .unwrap();
    root
}

/// The game's own `game.Spinner` in the main scene gets a sibling
/// component of `type`, with the same data.
fn use_in_scene(copy: &Copy, type_id: &str) {
    let path = "assets/scenes/main.scene.json";
    let mut scene = copy.json(path);
    let entity = scene["entities"]
        .as_object_mut()
        .unwrap()
        .values_mut()
        .find(|e| {
            e["components"]
                .as_object()
                .unwrap()
                .values()
                .any(|c| c["type"] == "game.Spinner")
        })
        .expect("a spinner in the fixture scene");
    let components = entity["components"].as_object_mut().unwrap();
    let mut added = components
        .values()
        .find(|c| c["type"] == "game.Spinner")
        .unwrap()
        .clone();
    added["type"] = json!(type_id);
    components.insert("c0000000-0000-4000-8000-00000000f0f0".into(), added);
    std::fs::write(
        copy.dir.join(path),
        serde_json::to_string_pretty(&scene).unwrap(),
    )
    .unwrap();
}

fn validate(copy: &Copy) -> Vec<Diagnostic> {
    fynkel_document::project::validate_project_with(
        &copy.dir,
        &fynkel_document::SchemaSet::embedded(),
        None,
        &fynkel_import::ImportedPrefabs::default(),
    )
    .diagnostics
    .into_iter()
    .filter(|d| d.code.as_str() != "FYN-P0010")
    .collect()
}

/// ADR-0029 step 2: a package's behaviour is the game's too — its schema
/// is loaded, the Inspector has its metadata ahead of the game's, the
/// registry allows it without an entry, and a scene using it validates.
#[test]
fn a_packages_behaviours_are_the_games_without_a_registry_entry() {
    let copy = Copy::new("package-types");
    add_pickups(&copy);
    use_in_scene(&copy, "pickups.Pickup");
    let found = validate(&copy);
    assert!(
        found
            .iter()
            .all(|d| !d.is_error() && d.code.as_str() != "FYN-P0004"),
        "{found:#?}"
    );

    let s = open(&copy);
    let p = s.project().unwrap();
    assert!(p.schema_sources.contains_key("pickups.Pickup"));
    let listed = p.schemas_json();
    let pickup = listed["behaviours"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["type"] == "pickups.Pickup")
        .expect("listed");
    assert_eq!(pickup["allowed"], true);
    assert_eq!(pickup["hasSchema"], true);
    // The package's metadata merged ahead of the game's (the fixture has
    // none of its own).
    let ids: Vec<&str> = p.metadata.as_ref().unwrap()["behaviours"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["pickups.Pickup"]);
}

/// FYN-P0016 (the same type in the game and a package) and FYN-P0017 (a
/// package type outside its namespace), from `fynkel validate` and the
/// session alike.
#[test]
fn a_type_clash_is_p0016_and_a_type_outside_the_namespace_is_p0017() {
    let copy = Copy::new("package-clash");
    let root = add_pickups(&copy);
    let spinner = copy.read("schemas/behaviours/game.Spinner.schema.json");
    std::fs::write(
        copy.dir
            .join("schemas/behaviours/pickups.Pickup.schema.json"),
        spinner.replace("game.Spinner", "pickups.Pickup"),
    )
    .unwrap();
    std::fs::write(
        root.join("schemas/behaviours/game.Coin.schema.json"),
        spinner.replace("game.Spinner", "game.Coin"),
    )
    .unwrap();
    let codes = |ds: &[Diagnostic]| {
        let mut c: Vec<(String, String)> = ds
            .iter()
            .filter(|d| d.code.as_str().starts_with("FYN-P001"))
            .map(|d| {
                (
                    d.code.as_str().to_string(),
                    d.document.clone().unwrap_or_default(),
                )
            })
            .collect();
        c.sort();
        c
    };
    let expected = vec![
        (
            "FYN-P0016".to_string(),
            "schemas/behaviours/pickups.Pickup.schema.json".to_string(),
        ),
        (
            "FYN-P0017".to_string(),
            "pickups/schemas/behaviours/game.Coin.schema.json".to_string(),
        ),
    ];
    assert_eq!(codes(&validate(&copy)), expected);
    let s = open(&copy);
    assert_eq!(codes(&s.project().unwrap().diagnostics), expected);
}
