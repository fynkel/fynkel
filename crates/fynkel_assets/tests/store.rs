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
const RIG: &str = "c0000000-0000-4000-8000-000000000005";
/// The Robot's tone (M3-11).
const TONE: &str = "c0000000-0000-4000-8000-000000000006";
const COLUMN: &str = "c0000000-0000-4000-8000-000000000050";
const ENERGY: &str = "c0000000-0000-4000-8000-000000000051";

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
        17,
        "five scenes, robot, rig, the column rig document and its blend space, material, three textures, three prefabs, the tone"
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
    // The scene depends on the prefab, the robot, the material and the
    // Robot's tone (M3-11); the closure adds the material's textures.
    assert_eq!(main.dependencies.len(), 4);
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
        id(TONE),
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
    // Camera, Robot, Sun, Crate A (the crate prefab's root), the physics
    // Floor and Crate B.
    assert_eq!(scene.description.entities.len(), 6);
    assert_eq!(scene.closure.len(), 8, "the Robot's tone too (M3-11)");
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
    assert_eq!(prefab.root, prefab.description.entities[0].id);
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

    // M3-19a: an imported model's hierarchy is a prefab too — the `prefab`
    // chunk of the cooked source, keyed by the source's id: the fixture
    // rig's six nodes with the Body's Skin, the closure the source's own
    // (meshes, skin, clips); a texture has no hierarchy.
    let rig = assets.prefab(id(RIG)).unwrap_or_else(|d| panic!("{d:#?}"));
    assert_eq!(rig.id, id(RIG));
    assert_eq!(
        (rig.name.as_str(), rig.path.as_str()),
        ("rig", "models/rig.glb"),
        "named after the file (the glTF scene has no name)"
    );
    assert_eq!(rig.diagnostics, vec![]);
    let names: Vec<&str> = rig
        .description
        .entities
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(names, ["Rig", "Body", "hips", "spine", "head", "Prop"]);
    assert_eq!(rig.root, rig.description.entities[0].id);
    assert!(rig.description.entities[1]
        .components
        .iter()
        .any(|c| matches!(c, fynkel_core::components::ComponentValue::Skin(_))));
    assert_eq!(rig.closure, vec![id(RIG)]);

    // M3-19b: a rig document is a prefab too — the skeleton file's
    // hierarchy adopted under the rig's id (the same six nodes, the same
    // entity ids), its closure the rig plus the skeleton source, whose
    // meshes and skin the hierarchy still references. Its clips answer
    // under the rig's names.
    let column = assets
        .prefab(id(COLUMN))
        .unwrap_or_else(|d| panic!("{d:#?}"));
    assert_eq!(column.id, id(COLUMN));
    assert_eq!(
        (column.name.as_str(), column.path.as_str()),
        ("Column", "rigs/column.rig.json")
    );
    assert_eq!(column.diagnostics, vec![]);
    let column_names: Vec<&str> = column
        .description
        .entities
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(column_names, names);
    assert_eq!(
        column
            .description
            .entities
            .iter()
            .map(|e| e.id)
            .collect::<Vec<_>>(),
        rig.description
            .entities
            .iter()
            .map(|e| e.id)
            .collect::<Vec<_>>(),
        "the entity ids are the skeleton file's"
    );
    let mut closure = column.closure.clone();
    closure.sort();
    assert_eq!(closure, vec![id(RIG), id(COLUMN)]);
    let wave = assets
        .clip(&AssetRef::with_sub(id(COLUMN), "anim/wave"))
        .unwrap_or_else(|d| panic!("{d}"));
    let source = assets
        .clip(&AssetRef::with_sub(id(RIG), "anim/wave"))
        .unwrap();
    assert_eq!(wave.duration, source.duration);
    assert_eq!(wave.channels.len(), source.channels.len());
    assert_eq!(
        wave.channels
            .iter()
            .map(|c| c.target.as_str())
            .collect::<Vec<_>>(),
        source
            .channels
            .iter()
            .map(|c| c.target.as_str())
            .collect::<Vec<_>>()
    );
    let bob = assets
        .clip(&AssetRef::with_sub(id(COLUMN), "anim/bob"))
        .unwrap();
    assert_eq!(bob.channels[0].target, "Prop");
    let d = assets
        .clip(&AssetRef::with_sub(id(COLUMN), "anim/0"))
        .unwrap_err();
    assert_eq!(d.code, Code::RUNTIME_CHUNK_MISSING, "{d}");
    let d = assets.prefab(id(SWATCH)).unwrap_err();
    assert_eq!(d[0].code, Code::RUNTIME_CHUNK_MISSING);
    assert!(
        d[0].message.contains("without a node hierarchy"),
        "{}",
        d[0]
    );

    // M3-19c: the blend space reads as the core's, and registering it
    // with a world brings its points' clips along.
    let energy = AssetRef::new(id(ENERGY));
    assert!(assets.is_blend(&energy));
    assert!(!assets.is_blend(&AssetRef::new(id(COLUMN))));
    let space = assets.blend(&energy).unwrap_or_else(|d| panic!("{d}"));
    assert_eq!(space.parameter, "energy");
    assert_eq!(
        space
            .points
            .iter()
            .map(|p| (p.at, p.clip.sub.as_str()))
            .collect::<Vec<_>>(),
        [(0.0, "anim/bob"), (1.0, "anim/wave")]
    );
    let mut world = fynkel_core::World::new(fynkel_core::WorldConfig::default());
    assert!(assets.register_clip(&energy, &mut world).unwrap());
    assert!(world.has_blend(&energy));
    assert!(world.has_clip(&AssetRef::with_sub(id(COLUMN), "anim/bob")));
    assert!(world.has_clip(&AssetRef::with_sub(id(COLUMN), "anim/wave")));
    assert!(!assets.register_clip(&energy, &mut world).unwrap());
    let d = assets.blend(&AssetRef::new(id(COLUMN))).unwrap_err();
    assert_eq!(d.code, Code::RUNTIME_CHUNK_MISSING, "{d}");
    let d = assets.clip(&energy).unwrap_err();
    assert_eq!(d.code, Code::RUNTIME_CHUNK_MISSING, "{d}");
}

#[test]
fn payloads_load_by_reference_and_residency_evicts() {
    let cooked = Cooked::new("residency");
    let assets = Assets::open(&cooked.0).unwrap();
    let robot_mesh = AssetRef::with_sub(id(ROBOT), "mesh/Cube");
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
    assert_eq!(assets.stats().retained, 8);
    let before = assets.evictions();
    a.release();
    assert_eq!(assets.stats().retained, 8);
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
