//! The residency store over the fixture project's cooked cache (blueprint
//! §19.2d item 3): the index opens and checks the build; a scene describes
//! from its expanded chunk with the cook's behaviour types; payloads load
//! by id and sub-asset and stay resident while retained; releasing evicts;
//! every miss is a `FYN-R` diagnostic naming the asset — and nothing here
//! opens a source file.

use std::path::{Path, PathBuf};

use fynkel_assets::{Assets, CacheIndex};
use fynkel_contracts::{AssetRef, Code, Uuid};
use fynkel_import::{cook_project, CookOptions};

const MAIN: &str = "c0000000-0000-4000-8000-000000000030";
const CRATE_PREFAB: &str = "c0000000-0000-4000-8000-000000000040";
const ROBOT: &str = "c0000000-0000-4000-8000-000000000001";
const SWATCH: &str = "c0000000-0000-4000-8000-000000000002";
const RUSTED: &str = "c0000000-0000-4000-8000-000000000020";
const RUSTED_NORMAL: &str = "c0000000-0000-4000-8000-000000000003";
const RUSTED_MR: &str = "c0000000-0000-4000-8000-000000000004";

fn id(s: &str) -> Uuid {
    s.parse().unwrap()
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fynkel_document/fixtures/project")
        .canonicalize()
        .unwrap()
}

/// The fixture cooked into its own cache directory (removed on drop).
struct Cooked(PathBuf);

impl Cooked {
    fn new(name: &str) -> Cooked {
        let dir = std::env::temp_dir().join(format!("fynkel-assets-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let report = cook_project(&fixture(), Some(&dir), &CookOptions::default())
            .unwrap_or_else(|d| panic!("{d:#?}"));
        assert!(!report.has_errors(), "{}", report.text());
        Cooked(dir)
    }
}

impl Drop for Cooked {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_index_opens_and_refuses_other_builds() {
    let cooked = Cooked::new("index");
    let index = CacheIndex::open(&cooked.0).unwrap_or_else(|d| panic!("{d}"));
    assert_eq!(index.profile(), "dev");
    assert_eq!(
        index.len(),
        14,
        "five scenes, robot, rig, material, three textures, three prefabs"
    );
    let p = index.project().unwrap();
    assert_eq!(p.name, "Fixture Game");
    assert_eq!(p.startup_scene, Some(id(MAIN)));
    assert_eq!(index.startup_scene().unwrap(), id(MAIN));
    let main = index.get(id(MAIN)).unwrap();
    assert_eq!(main.path, "scenes/main.scene.json");
    assert_eq!(main.name, "Main");
    assert_eq!(main.kind, "document/scene");
    assert_eq!(main.status, "cooked");
    assert!(main.file.as_ref().unwrap().is_file());
    // The scene depends on the prefab, the robot and the material; the
    // closure adds the material's texture.
    assert_eq!(main.dependencies.len(), 3);
    let mut closure = index.closure(id(MAIN));
    closure.sort();
    let mut expect = vec![
        id(MAIN),
        id(CRATE_PREFAB),
        id(ROBOT),
        id(RUSTED),
        id(SWATCH),
        id(RUSTED_NORMAL),
        id(RUSTED_MR),
    ];
    expect.sort();
    assert_eq!(closure, expect);
    assert_eq!(
        index
            .object(id("c0000000-0000-4000-8000-0000000000ff"))
            .unwrap_err()
            .code,
        Code::RUNTIME_ASSET_MISSING
    );

    // Not there, not JSON, not an index, another build.
    let missing = CacheIndex::open(Path::new("/nonexistent/cache")).unwrap_err();
    assert_eq!(missing.code, Code::RUNTIME_CACHE_UNREADABLE);
    assert!(missing
        .suggestion
        .as_deref()
        .unwrap()
        .contains("fynkel cook"));
    let index_path = cooked.0.join("index.json");
    let original = std::fs::read_to_string(&index_path).unwrap();
    std::fs::write(&index_path, "{").unwrap();
    assert_eq!(
        CacheIndex::open(&cooked.0).unwrap_err().code,
        Code::RUNTIME_CACHE_UNREADABLE
    );
    std::fs::write(&index_path, r#"{"fynkel":"something-else"}"#).unwrap();
    assert_eq!(
        CacheIndex::open(&cooked.0).unwrap_err().code,
        Code::RUNTIME_CACHE_UNREADABLE
    );
    let stale = original.replacen(
        &fynkel_import::cook::schema_fingerprint_hex(),
        "0x0123456789abcdef",
        1,
    );
    assert_ne!(stale, original);
    std::fs::write(&index_path, stale).unwrap();
    let d = CacheIndex::open(&cooked.0).unwrap_err();
    assert_eq!(d.code, Code::RUNTIME_CACHE_STALE);
    assert!(d.message.contains("0x0123456789abcdef"), "{d}");
    // No project record: no startup scene, by id still works.
    let bare = original.replace("\"project\"", "\"notProject\"");
    std::fs::write(&index_path, bare).unwrap();
    let index = CacheIndex::open(&cooked.0).unwrap();
    assert!(index.project().is_none());
    assert_eq!(
        index.startup_scene().unwrap_err().code,
        Code::RUNTIME_NO_STARTUP_SCENE
    );
    assert!(index.object(id(MAIN)).is_ok());
}

#[test]
fn scenes_describe_from_the_expanded_chunk() {
    let cooked = Cooked::new("scene");
    let assets = Assets::open(&cooked.0).unwrap();
    let scene = assets.scene(id(MAIN)).unwrap_or_else(|d| panic!("{d:#?}"));
    assert_eq!(scene.name, "Main");
    assert_eq!(scene.path, "scenes/main.scene.json");
    assert_eq!(scene.key.len(), 64);
    assert_eq!(
        scene.diagnostics,
        vec![],
        "game.Spinner is known to the cook"
    );
    // Camera, Robot, Sun, Crate A and the crate the prefab expands to,
    // the physics Floor and Crate B.
    assert_eq!(scene.description.entities.len(), 7);
    assert_eq!(scene.closure.len(), 7);
    assert_eq!(scene.behaviours.len(), 1);
    assert_eq!(scene.behaviours[0].type_id, "game.Spinner");
    assert!(scene.behaviours[0].known);

    // A prefab has no scene chunk; an unknown id is not in the cache.
    let d = assets.scene(id(CRATE_PREFAB)).unwrap_err();
    assert_eq!(d[0].code, Code::RUNTIME_CHUNK_MISSING);
    assert_eq!(d[0].id.as_deref(), Some(CRATE_PREFAB));
    let d = assets
        .scene(id("c0000000-0000-4000-8000-0000000000ff"))
        .unwrap_err();
    assert_eq!(d[0].code, Code::RUNTIME_ASSET_MISSING);
}

/// A prefab describes from its own cooked chunk (M3-08b): flat entities,
/// the declared roots, its closure; a scene is not a prefab.
#[test]
fn prefabs_describe_from_their_chunk() {
    let cooked = Cooked::new("prefab");
    let assets = Assets::open(&cooked.0).unwrap();
    let prefab = assets
        .prefab(id(CRATE_PREFAB))
        .unwrap_or_else(|d| panic!("{d:#?}"));
    assert_eq!(prefab.id, id(CRATE_PREFAB));
    assert_eq!(prefab.name, "Crate");
    assert_eq!(prefab.path, "prefabs/crate.prefab.json");
    assert_eq!(prefab.diagnostics, vec![]);
    assert_eq!(prefab.description.entities.len(), 1);
    assert_eq!(prefab.description.entities[0].name, "Crate");
    assert_eq!(prefab.roots, vec![prefab.description.entities[0].id]);
    assert!(prefab.behaviours.is_empty());
    // The prefab, the robot mesh, the material and its three maps.
    assert_eq!(prefab.closure.len(), 6);

    let d = assets.prefab(id(MAIN)).unwrap_err();
    assert_eq!(d[0].code, Code::RUNTIME_CHUNK_MISSING);
    assert_eq!(d[0].id.as_deref(), Some(MAIN));
    let d = assets
        .prefab(id("c0000000-0000-4000-8000-0000000000ff"))
        .unwrap_err();
    assert_eq!(d[0].code, Code::RUNTIME_ASSET_MISSING);
}

#[test]
fn payloads_load_by_reference_and_residency_evicts() {
    let cooked = Cooked::new("residency");
    let assets = Assets::open(&cooked.0).unwrap();
    let robot_mesh = AssetRef::with_sub(id(ROBOT), "mesh/0");
    let mesh = assets.mesh(&robot_mesh).unwrap_or_else(|d| panic!("{d}"));
    assert!(!mesh.vertices.is_empty());
    let again = assets.mesh(&robot_mesh).unwrap();
    assert!(
        std::sync::Arc::ptr_eq(&mesh, &again),
        "cached while resident"
    );
    let texture = assets.texture(&AssetRef::new(id(SWATCH))).unwrap();
    assert!(texture.width > 0);
    let material = assets.material(&AssetRef::new(id(RUSTED))).unwrap();
    assert!((material.roughness - 0.65).abs() < 1e-6);
    let stats = assets.stats();
    assert_eq!((stats.meshes, stats.textures, stats.materials), (1, 1, 1));
    assert!(stats.bytes > 0);
    assert_eq!(stats.retained, 0, "loaded on use, retained by nobody");

    // Two scenes share the closure: releasing one keeps everything;
    // releasing the other evicts it all (one eviction pass).
    let closure = assets.closure(id(MAIN));
    let a = assets.retain(&closure);
    let b = assets.retain(&closure);
    assert!(assets.is_resident(id(ROBOT)));
    assert_eq!(assets.stats().retained, 7);
    let before = assets.evictions();
    a.release();
    assert_eq!(assets.stats().retained, 7);
    assert_eq!(assets.stats().meshes, 1);
    assert_eq!(assets.evictions(), before);
    drop(b);
    assert!(!assets.is_resident(id(ROBOT)));
    let after = assets.stats();
    assert_eq!(
        (
            after.retained,
            after.meshes,
            after.textures,
            after.materials
        ),
        (0, 0, 0, 0)
    );
    assert_eq!(after.evictions, before + 1);
    // Reloads on the next use.
    let reloaded = assets.mesh(&robot_mesh).unwrap();
    assert!(!std::sync::Arc::ptr_eq(&mesh, &reloaded));
    assert_eq!(reloaded.vertices.len(), mesh.vertices.len());

    // Clones share the store.
    let other = assets.clone();
    assert!(other.same_store(&assets));
    assert_eq!(other.stats().meshes, 1);
    let separate = Assets::open(&cooked.0).unwrap();
    assert!(!separate.same_store(&assets));
    assert_eq!(separate.stats().meshes, 0);

    // Misses name the asset: a sub-asset the object lacks, a kind the
    // object lacks, an id the cache lacks.
    let d = assets
        .mesh(&AssetRef::with_sub(id(ROBOT), "mesh/9"))
        .unwrap_err();
    assert_eq!(d.code, Code::RUNTIME_CHUNK_MISSING);
    assert_eq!(d.id.as_deref(), Some(ROBOT));
    assert_eq!(d.pointer.as_deref(), Some("mesh/9"));
    let d = assets.texture(&AssetRef::new(id(MAIN))).unwrap_err();
    assert_eq!(d.code, Code::RUNTIME_CHUNK_MISSING);
    let d = assets
        .mesh(&AssetRef::with_sub(
            id("c0000000-0000-4000-8000-0000000000ff"),
            "mesh/0",
        ))
        .unwrap_err();
    assert_eq!(d.code, Code::RUNTIME_ASSET_MISSING);

    // An object that vanished (or was truncated) is unreadable, not a
    // fallback to source.
    let file = assets.entry(id(SWATCH)).unwrap().file.unwrap();
    std::fs::write(&file, b"FYNA").unwrap();
    assets.clear();
    let d = assets.texture(&AssetRef::new(id(SWATCH))).unwrap_err();
    assert_eq!(d.code, Code::RUNTIME_OBJECT_UNREADABLE);
    assert_eq!(d.id.as_deref(), Some(SWATCH));
    std::fs::remove_file(&file).unwrap();
    assets.clear();
    let d = assets.texture(&AssetRef::new(id(SWATCH))).unwrap_err();
    assert_eq!(d.code, Code::RUNTIME_OBJECT_UNREADABLE);
}

#[test]
fn a_failed_cook_is_reported_by_id() {
    let cooked = Cooked::new("failed");
    // Rewrite the swatch's entry as a failed cook.
    let index_path = cooked.0.join("index.json");
    let mut index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&index_path).unwrap()).unwrap();
    for a in index["assets"].as_array_mut().unwrap() {
        if a["id"] == SWATCH {
            a["status"] = "failed".into();
            a["file"] = serde_json::Value::Null;
            a["key"] = serde_json::Value::Null;
        }
    }
    std::fs::write(&index_path, index.to_string()).unwrap();
    let assets = Assets::open(&cooked.0).unwrap();
    let d = assets.texture(&AssetRef::new(id(SWATCH))).unwrap_err();
    assert_eq!(d.code, Code::RUNTIME_ASSET_FAILED);
    assert_eq!(d.id.as_deref(), Some(SWATCH));
    // The scene still describes (its closure names the failed asset; the
    // renderer reports the miss when it draws).
    assert!(assets.scene(id(MAIN)).is_ok());
}
