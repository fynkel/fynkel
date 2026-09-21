//! The cache and the cooker (blueprint §19.2c item 12): a clean corpus
//! cooks to one container per asset; cooking again hits every key;
//! editing a texture (its pixels or its sidecar settings) recooks exactly
//! the material that uses it and the scene that uses that material;
//! changing the profile misses everything; the containers read back
//! through `fynkel_cooked` with bounded decoding; cycles and unresolved
//! references fail only the assets involved.

use std::path::{Path, PathBuf};

use fynkel_contracts::Code;
use fynkel_cooked::{ChunkFile, Container};
use fynkel_core::material::AlphaMode;
use fynkel_document::asset::AssetDatabase;
use fynkel_document::SchemaSet;
use fynkel_import::cook::{
    decode_clip, decode_mesh, decode_texture, TEXTURE_FORMAT_RGBA8_LINEAR,
    TEXTURE_FORMAT_RGBA8_SRGB,
};
use fynkel_import::{cook, Cache, CookOptions, CookReport, Status};
use fynkel_wire::material::decode_material;
use uuid::Uuid;

const ROBOT: &str = "c0000000-0000-4000-8000-000000000001";
const SWATCH: &str = "c0000000-0000-4000-8000-000000000002";
const TONE: &str = "c0000000-0000-4000-8000-000000000003";
const NOTES: &str = "c0000000-0000-4000-8000-000000000004";
const RUSTED: &str = "c0000000-0000-4000-8000-000000000020";
const MAIN: &str = "c0000000-0000-4000-8000-000000000030";

fn id(s: &str) -> Uuid {
    Uuid::parse_str(s).unwrap()
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn schemas() -> SchemaSet {
    SchemaSet::embedded()
}

/// A fresh copy of the corpus and an empty cache, both under a temporary
/// directory that is removed on drop.
struct Project {
    dir: PathBuf,
}

impl Project {
    fn new(name: &str) -> Project {
        let dir = std::env::temp_dir().join(format!("fynkel-cook-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        copy_dir(&fixtures().join("project/assets"), &dir.join("assets"));
        Project { dir }
    }

    fn assets(&self) -> PathBuf {
        self.dir.join("assets")
    }

    fn cache(&self) -> Cache {
        Cache::at(&self.dir.join("cache"))
    }

    fn cook_with(&self, opts: &CookOptions) -> CookReport {
        let (db, diags) = AssetDatabase::scan(&self.assets(), &schemas());
        assert!(
            !fynkel_contracts::has_errors(&diags),
            "the corpus must scan clean: {diags:?}"
        );
        cook(&db, &schemas(), &self.cache(), opts)
    }

    fn cook(&self) -> CookReport {
        self.cook_with(&CookOptions::default())
    }

    fn write(&self, rel: &str, text: &str) {
        std::fs::write(self.assets().join(rel), text).unwrap();
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn status(r: &CookReport, asset: &str) -> Status {
    r.get(id(asset))
        .unwrap_or_else(|| panic!("{asset} not in the report"))
        .status
}

fn statuses(r: &CookReport) -> Vec<(&str, Status)> {
    [ROBOT, SWATCH, TONE, NOTES, RUSTED, MAIN]
        .iter()
        .map(|a| (*a, status(r, a)))
        .collect()
}

fn all(r: &CookReport, s: Status) -> bool {
    r.assets.iter().all(|a| a.status == s)
}

#[test]
fn a_clean_corpus_cooks_then_hits() {
    let p = Project::new("clean");
    let first = p.cook();
    assert!(!first.has_errors(), "{:?}", first.diagnostics);
    assert_eq!(first.assets.len(), 6);
    assert!(all(&first, Status::Cooked), "{:?}", statuses(&first));
    for a in &first.assets {
        let key = a.key.expect("cooked assets have a key");
        assert!(p.cache().has(&key));
        assert_eq!(
            a.file.as_deref(),
            Some(p.cache().object_path(&key).as_path())
        );
        assert!(a.bytes > 0);
        assert_eq!(a.chunks[0].kind, "META");
    }
    assert!(p.cache().index_path().is_file());
    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(p.cache().index_path()).unwrap()).unwrap();
    assert_eq!(index["profile"], "dev");
    assert_eq!(index["assets"].as_array().unwrap().len(), 6);

    let second = p.cook();
    assert!(!second.has_errors());
    assert!(all(&second, Status::Cached), "{:?}", statuses(&second));
    for (a, b) in first.assets.iter().zip(&second.assets) {
        assert_eq!(a.key, b.key);
        assert_eq!(a.chunks, b.chunks);
        assert_eq!(a.bytes, b.bytes);
    }

    // Dependencies: the material uses the texture, the scene uses the
    // model and the material; sources depend on nothing.
    let by = |a: &str| first.get(id(a)).unwrap().dependencies.clone();
    assert_eq!(by(RUSTED), vec![id(SWATCH)]);
    assert_eq!(by(MAIN), vec![id(ROBOT), id(RUSTED)]);
    assert!(by(ROBOT).is_empty() && by(SWATCH).is_empty() && by(TONE).is_empty());

    // `--force` cooks again to the same keys and bytes.
    let forced = p.cook_with(&CookOptions {
        force: true,
        ..CookOptions::default()
    });
    assert!(all(&forced, Status::Cooked));
    for (a, b) in first.assets.iter().zip(&forced.assets) {
        assert_eq!(a.key, b.key);
        assert_eq!(a.chunks, b.chunks);
    }
}

#[test]
fn editing_a_texture_invalidates_exactly_its_dependents() {
    let p = Project::new("edit-png");
    let first = p.cook();
    assert!(all(&first, Status::Cooked));

    // Swap the pixels: same dimensions, different bytes.
    let png = fixtures().join("textures/swatch_rgb.png");
    std::fs::copy(&png, p.assets().join("textures/swatch.png")).unwrap();
    let second = p.cook();
    assert!(!second.has_errors(), "{:?}", second.diagnostics);
    assert_eq!(
        statuses(&second),
        [
            (ROBOT, Status::Cached),
            (SWATCH, Status::Cooked),
            (TONE, Status::Cached),
            (NOTES, Status::Cached),
            (RUSTED, Status::Cooked),
            (MAIN, Status::Cooked),
        ]
    );
    let key = |r: &CookReport, a: &str| r.get(id(a)).unwrap().key.unwrap();
    assert_ne!(key(&first, SWATCH), key(&second, SWATCH));
    assert_ne!(key(&first, RUSTED), key(&second, RUSTED));
    assert_ne!(key(&first, MAIN), key(&second, MAIN));
    assert_eq!(key(&first, ROBOT), key(&second, ROBOT));
    // The old objects are still there (content-addressed, never edited).
    assert!(p.cache().has(&key(&first, SWATCH)));
    assert!(p.cache().has(&key(&second, SWATCH)));

    // The material's manifest names the new texture key.
    let manifest = p.cache().read_manifest(&key(&second, RUSTED)).unwrap();
    assert_eq!(manifest.dependencies.len(), 1);
    assert_eq!(manifest.dependencies[0].id, SWATCH);
    assert_eq!(manifest.dependencies[0].key, key(&second, SWATCH).hex());

    // Then the same again: everything hits.
    let third = p.cook();
    assert!(all(&third, Status::Cached));

    // Putting the original pixels back finds the first objects again.
    std::fs::copy(
        fixtures().join("textures/swatch.png"),
        p.assets().join("textures/swatch.png"),
    )
    .unwrap();
    let fourth = p.cook();
    assert!(all(&fourth, Status::Cached), "{:?}", statuses(&fourth));
    assert_eq!(key(&fourth, MAIN), key(&first, MAIN));
}

#[test]
fn editing_import_settings_invalidates_the_same_set() {
    let p = Project::new("edit-settings");
    let first = p.cook();
    assert!(all(&first, Status::Cooked));
    p.write(
        "textures/swatch.png.meta.json",
        &format!(
            r#"{{"$schema":"https://fynkel.dev/schemas/meta.schema.json","formatVersion":1,"id":"{SWATCH}","name":"swatch.png","importer":"fynkel.image","importerVersion":1,"settings":{{"colorSpace":"linear"}},"subAssets":{{}}}}"#
        ),
    );
    let second = p.cook();
    assert!(!second.has_errors(), "{:?}", second.diagnostics);
    assert_eq!(
        statuses(&second),
        [
            (ROBOT, Status::Cached),
            (SWATCH, Status::Cooked),
            (TONE, Status::Cached),
            (NOTES, Status::Cached),
            (RUSTED, Status::Cooked),
            (MAIN, Status::Cooked),
        ]
    );
    // The texture chunk now says linear.
    let swatch = second.get(id(SWATCH)).unwrap();
    let bytes = std::fs::read(swatch.file.as_ref().unwrap()).unwrap();
    let c = Container::parse(&bytes).unwrap();
    let texture = decode_texture(c.read(1, 1 << 20).unwrap()).unwrap();
    assert_eq!(
        texture.format,
        fynkel_import::cook::TEXTURE_FORMAT_RGBA8_LINEAR
    );
}

#[test]
fn editing_a_document_invalidates_it_and_its_dependents_only() {
    let p = Project::new("edit-doc");
    let first = p.cook();
    assert!(all(&first, Status::Cooked));
    let path = p.assets().join("materials/rusted.material.json");
    let text = std::fs::read_to_string(&path)
        .unwrap()
        .replace("\"roughness\": 0.65", "\"roughness\": 0.9");
    std::fs::write(&path, text).unwrap();
    let second = p.cook();
    assert_eq!(
        statuses(&second),
        [
            (ROBOT, Status::Cached),
            (SWATCH, Status::Cached),
            (TONE, Status::Cached),
            (NOTES, Status::Cached),
            (RUSTED, Status::Cooked),
            (MAIN, Status::Cooked),
        ]
    );
}

#[test]
fn the_profile_is_part_of_the_key() {
    let p = Project::new("profile");
    let dev = p.cook();
    assert!(all(&dev, Status::Cooked));
    let ios = p.cook_with(&CookOptions {
        profile: "ios".to_string(),
        ..CookOptions::default()
    });
    assert!(all(&ios, Status::Cooked), "{:?}", statuses(&ios));
    for (a, b) in dev.assets.iter().zip(&ios.assets) {
        assert_ne!(a.key, b.key);
        // Same inputs otherwise: the payload chunks are identical.
        assert_eq!(a.chunks[1..], b.chunks[1..]);
    }
    assert!(all(&p.cook(), Status::Cached));
}

#[test]
fn containers_read_back_with_bounded_decoding() {
    let p = Project::new("readback");
    let r = p.cook();
    assert!(!r.has_errors());

    // The model: meshes, materials, a decoded image, the prefab.
    let robot = r.get(id(ROBOT)).unwrap();
    let mut f = ChunkFile::open_path(robot.file.as_ref().unwrap()).unwrap();
    f.verify().unwrap();
    let meta: serde_json::Value = serde_json::from_slice(&f.meta(1 << 20).unwrap()).unwrap();
    assert_eq!(meta["id"], ROBOT);
    assert_eq!(meta["kind"], "fynkel.gltf");
    assert_eq!(meta["importer"], "fynkel.gltf");
    assert_eq!(meta["profile"], "dev");
    assert_eq!(meta["key"], robot.key.unwrap().hex());
    assert_eq!(
        meta["subAssets"]["mesh/0"],
        "cbac6df1-3f63-5f74-89cb-152ae43389d9"
    );
    let chunks = meta["chunks"].as_array().unwrap();
    assert_eq!(chunks.len(), f.chunks().len());
    let kinds: Vec<String> = f.chunks().iter().map(|c| c.kind_str()).collect();
    // Every material has its document and its typed MATL chunk; the one
    // image is cooked twice (colour and data use).
    assert_eq!(
        kinds,
        ["META", "MESH", "MESH", "MESH", "JSON", "MATL", "JSON", "MATL", "TEXR", "TEXR", "JSON"]
    );
    let names: Vec<&str> = chunks
        .iter()
        .map(|c| c["sub"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(
        names,
        [
            "",
            "mesh/0",
            "mesh/1/0",
            "mesh/1/1",
            "material/0",
            "material/0",
            "material/1",
            "material/1",
            "image/0",
            "image/0/linear",
            "prefab"
        ]
    );
    assert_eq!(chunks[1]["id"], "cbac6df1-3f63-5f74-89cb-152ae43389d9");
    let mesh = decode_mesh(&f.read(1, 1 << 24).unwrap()).unwrap();
    assert_eq!(
        mesh.vertices.len(),
        chunks[1]["vertices"].as_u64().unwrap() as usize
    );
    assert_eq!(
        mesh.indices.len(),
        chunks[1]["indices"].as_u64().unwrap() as usize
    );
    assert!(mesh
        .indices
        .iter()
        .all(|i| (*i as usize) < mesh.vertices.len()));
    // The robot authors no tangents: the cook generated an orthonormal
    // frame per vertex (M3-04) and says so in the chunk info.
    assert_eq!(chunks[1]["tangents"], "generated");
    assert!(mesh.vertices.iter().all(|v| {
        let t = glam::Vec3::from([v.tangent[0], v.tangent[1], v.tangent[2]]);
        (t.length() - 1.0).abs() < 1e-4
            && t.dot(glam::Vec3::from(v.normal)).abs() < 1e-3
            && v.tangent[3].abs() == 1.0
    }));
    assert!(mesh
        .bounds_min
        .iter()
        .zip(&mesh.bounds_max)
        .all(|(a, b)| a <= b));
    let material: serde_json::Value = serde_json::from_slice(&f.read(4, 1 << 20).unwrap()).unwrap();
    assert_eq!(material["shader"], "fynkel.pbr@1");
    let painted = decode_material(&f.read(5, 1 << 20).unwrap()).unwrap();
    assert_eq!(painted.base_color.to_array(), [0.8, 0.2, 0.2, 1.0]);
    assert_eq!(painted.emissive_strength, 3.0);
    assert_eq!(
        painted.normal_texture.as_ref().unwrap().sub,
        "image/0/linear"
    );
    assert_eq!(
        chunks[5]["textures"],
        serde_json::json!([
            "base_color_texture",
            "normal_texture",
            "metallic_roughness_texture",
            "occlusion_texture",
            "emissive_texture"
        ])
    );
    let glass = decode_material(&f.read(7, 1 << 20).unwrap()).unwrap();
    assert_eq!(glass.alpha_mode, AlphaMode::Blend);
    assert!(glass.double_sided);
    assert_eq!(chunks[7]["alphaMode"], "blend");
    let image = decode_texture(&f.read(8, 1 << 24).unwrap()).unwrap();
    assert_eq!(
        (image.width, image.height),
        (
            chunks[8]["width"].as_u64().unwrap() as u32,
            chunks[8]["height"].as_u64().unwrap() as u32
        )
    );
    assert_eq!(image.format, TEXTURE_FORMAT_RGBA8_SRGB);
    assert_eq!(chunks[8]["colorSpace"], "srgb");
    assert_eq!(chunks[8]["mipLevels"], image.mip_levels);
    let linear = decode_texture(&f.read(9, 1 << 24).unwrap()).unwrap();
    assert_eq!(linear.format, TEXTURE_FORMAT_RGBA8_LINEAR);
    assert_eq!(
        linear.base(),
        image.base(),
        "the base level is the source either way"
    );
    assert_eq!(linear.mip_levels, image.mip_levels);
    assert_ne!(
        linear.level(linear.mip_levels - 1),
        image.level(image.mip_levels - 1),
        "the sRGB chain averages in linear light, the linear one as stored"
    );
    assert_eq!(chunks[9]["colorSpace"], "linear");
    let prefab: serde_json::Value = serde_json::from_slice(&f.read(10, 1 << 20).unwrap()).unwrap();
    assert!(prefab["entities"].is_object());

    // Bounded: a mesh larger than the budget is refused before it is read.
    let err = f.read(1, 16).unwrap_err();
    assert!(
        matches!(err, fynkel_cooked::Error::ChunkTooLarge { index: 1, .. }),
        "{err}"
    );
    assert!(matches!(
        f.read(99, 1 << 20).unwrap_err(),
        fynkel_cooked::Error::ChunkOutOfRange(99)
    ));

    // The texture asset itself.
    let swatch = r.get(id(SWATCH)).unwrap();
    let bytes = std::fs::read(swatch.file.as_ref().unwrap()).unwrap();
    let c = Container::parse(&bytes).unwrap();
    assert_eq!(c.chunks().len(), 2);
    let t = decode_texture(c.read(1, 1 << 20).unwrap()).unwrap();
    assert_eq!(
        (t.width, t.height, t.format),
        (4, 3, TEXTURE_FORMAT_RGBA8_SRGB)
    );
    assert_eq!(t.mip_levels, 3, "4×3 → 2×1 → 1×1: the full chain");
    assert_eq!(t.rgba8.len(), 48 + 8 + 4);
    let base = t.base();
    assert_eq!(base.len(), 48);
    assert!(base[0] < base[12], "red rises along x, as in the source");
    let top = t.level(2).unwrap();
    assert_eq!(top.len(), 4);
    assert!(
        base[0] < top[0] && top[0] < base[12],
        "the 1×1 level averages the base (sRGB-aware, so between its samples)"
    );

    // The clip.
    let tone = r.get(id(TONE)).unwrap();
    let bytes = std::fs::read(tone.file.as_ref().unwrap()).unwrap();
    let c = Container::parse(&bytes).unwrap();
    let clip = decode_clip(c.read(1, 1 << 24).unwrap()).unwrap();
    assert_eq!((clip.sample_rate, clip.channels), (8000, 2));
    assert_eq!(clip.samples.len() as u64, clip.frames * 2);

    // The plain file: its bytes, untouched.
    let notes = r.get(id(NOTES)).unwrap();
    let bytes = std::fs::read(notes.file.as_ref().unwrap()).unwrap();
    let c = Container::parse(&bytes).unwrap();
    assert_eq!(c.chunks()[1].kind_str(), "BLOB");
    assert_eq!(
        c.read(1, 1 << 20).unwrap(),
        b"Design notes for the cook fixture.\n"
    );

    // Documents: canonical JSON, and the META lists the dependency ids.
    let main = r.get(id(MAIN)).unwrap();
    let bytes = std::fs::read(main.file.as_ref().unwrap()).unwrap();
    let c = Container::parse(&bytes).unwrap();
    let meta: serde_json::Value = serde_json::from_slice(c.meta(1 << 20).unwrap()).unwrap();
    assert_eq!(meta["kind"], "document/scene");
    assert_eq!(meta["dependencies"], serde_json::json!([ROBOT, RUSTED]));
    let scene: serde_json::Value = serde_json::from_slice(c.read(1, 1 << 20).unwrap()).unwrap();
    assert_eq!(scene["id"], MAIN);

    // A flipped byte in a payload is caught by its checksum.
    let mut bad = bytes.clone();
    let at = c.chunks()[1].offset as usize + 4;
    bad[at] ^= 0xff;
    let c = Container::parse(&bad).unwrap();
    assert!(matches!(
        c.read(1, 1 << 20),
        Err(fynkel_cooked::Error::Checksum { index: 1 })
    ));
}

#[test]
fn cycles_and_unresolved_references_fail_only_the_assets_involved() {
    let p = Project::new("failures");
    // Scene B references scene A, scene A references scene B (a scene's
    // entities may carry any assetRef-shaped data; the cooker does not
    // care which component).
    let a = "c0000000-0000-4000-8000-0000000000a0";
    let b = "c0000000-0000-4000-8000-0000000000b0";
    let scene = |own: &str, other: &str| {
        format!(
            r#"{{"$schema":"https://fynkel.dev/schemas/scene.schema.json","formatVersion":3,"id":"{own}","name":"x","entities":{{"c0000000-0000-4000-8000-0000000000e9":{{"name":"E","parent":null,"children":[],"components":{{"c0000000-0000-4000-8000-0000000000c9":{{"type":"fynkel.MeshRenderer","version":1,"data":{{"mesh":{{"asset":"{other}"}},"material":{{"asset":"{RUSTED}"}},"cast_shadows":true,"receive_shadows":true,"visible":true}}}}}}}}}}}}"#
        )
    };
    p.write("scenes/a.scene.json", &scene(a, b));
    p.write("scenes/b.scene.json", &scene(b, a));
    // Scene C references an asset that does not exist.
    let c = "c0000000-0000-4000-8000-0000000000c0";
    p.write(
        "scenes/c.scene.json",
        &scene(c, "c0000000-0000-4000-8000-0000000000ff"),
    );
    // Scene D depends on the cycle (on A).
    let d = "c0000000-0000-4000-8000-0000000000d0";
    p.write("scenes/d.scene.json", &scene(d, a));

    let r = p.cook();
    assert!(r.has_errors());
    assert_eq!(status(&r, a), Status::Failed);
    assert_eq!(status(&r, b), Status::Failed);
    assert_eq!(status(&r, c), Status::Failed);
    assert_eq!(status(&r, d), Status::Failed);
    assert!(
        statuses(&r).iter().all(|(_, s)| *s == Status::Cooked),
        "{:?}",
        statuses(&r)
    );
    let codes: Vec<Code> = r.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(
        codes
            .iter()
            .filter(|c| **c == Code::IMPORT_INVALID_DATA)
            .count(),
        2,
        "{:?}",
        r.diagnostics
    );
    assert_eq!(
        codes
            .iter()
            .filter(|c| **c == Code::ASSET_UNRESOLVED)
            .count(),
        1
    );
    let blocked = r
        .diagnostics
        .iter()
        .filter(|d| d.code == Code::IMPORT_SKIPPED && d.message.contains("not cooked because"))
        .count();
    assert_eq!(blocked, 1, "{:?}", r.diagnostics);
    assert!(r.get(id(d)).unwrap().key.is_none());

    // Fixing the cycle cooks the four scenes; nothing else moves.
    p.write("scenes/b.scene.json", &scene(b, RUSTED));
    p.write("scenes/c.scene.json", &scene(c, SWATCH));
    let r = p.cook();
    assert!(!r.has_errors(), "{:?}", r.diagnostics);
    assert!(statuses(&r).iter().all(|(_, s)| *s == Status::Cached));
    for s in [a, b, c, d] {
        assert_eq!(status(&r, s), Status::Cooked);
    }
}

#[test]
fn a_sidecar_behind_its_import_warns_and_a_broken_source_fails() {
    let p = Project::new("stale");
    p.write(
        "models/robot.glb.meta.json",
        &format!(
            r#"{{"$schema":"https://fynkel.dev/schemas/meta.schema.json","formatVersion":1,"id":"{ROBOT}","name":"robot.glb","importer":"fynkel.gltf","importerVersion":1,"settings":{{}},"subAssets":{{}}}}"#
        ),
    );
    // The scene's `mesh/0` reference no longer resolves against the sidecar.
    let r = p.cook();
    assert_eq!(status(&r, ROBOT), Status::Cooked);
    assert_eq!(status(&r, MAIN), Status::Failed);
    assert!(r
        .diagnostics
        .iter()
        .any(|d| d.code == Code::IMPORT_SKIPPED && d.message.contains("subAssets are behind")));
    // The cooked model still carries every sub-asset id (derived).
    let bytes = std::fs::read(r.get(id(ROBOT)).unwrap().file.as_ref().unwrap()).unwrap();
    let c = Container::parse(&bytes).unwrap();
    let meta: serde_json::Value = serde_json::from_slice(c.meta(1 << 20).unwrap()).unwrap();
    assert_eq!(
        meta["subAssets"]["mesh/0"],
        "cbac6df1-3f63-5f74-89cb-152ae43389d9"
    );

    // A truncated source fails its own asset; the rest cook.
    std::fs::copy(
        fixtures().join("audio/truncated.wav"),
        p.assets().join("audio/tone.wav"),
    )
    .unwrap();
    let r = p.cook();
    assert_eq!(status(&r, TONE), Status::Failed);
    assert!(r.get(id(TONE)).unwrap().key.is_some());
    assert!(!p.cache().has(&r.get(id(TONE)).unwrap().key.unwrap()));
    assert_eq!(status(&r, SWATCH), Status::Cached);
}

#[test]
fn prune_removes_objects_not_kept() {
    let p = Project::new("prune");
    let first = p.cook();
    std::fs::copy(
        fixtures().join("textures/swatch_rgb.png"),
        p.assets().join("textures/swatch.png"),
    )
    .unwrap();
    let second = p.cook();
    let keep: Vec<_> = second.assets.iter().filter_map(|a| a.key).collect();
    assert_eq!(p.cache().prune(&keep).unwrap(), 3);
    for a in &first.assets {
        let stale = second.get(a.id).unwrap().key != a.key;
        assert_eq!(p.cache().has(&a.key.unwrap()), !stale);
    }
    assert!(all(&p.cook(), Status::Cached));
}

/// M3-03: `cook_project` cooks a project directory — the expanded scene
/// (prefabs applied, ids derived, behaviour types recorded) is the scene's
/// JSON chunk, the index records the project, and an invalid scene is a
/// failed cook, never an object the runtime would refuse later.
#[test]
fn cook_project_expands_scenes_and_records_the_project() {
    use fynkel_import::{cook_project, project_cache_dir};
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fynkel_document/fixtures/project")
        .canonicalize()
        .unwrap();
    let dir = std::env::temp_dir().join(format!("fynkel-cook-project-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(&src, &dir);

    let r = cook_project(&dir, None, &CookOptions::default()).unwrap_or_else(|d| panic!("{d:#?}"));
    assert!(!r.has_errors(), "{}", r.text());
    assert_eq!(
        r.assets.len(),
        15,
        "five scenes, three prefabs, the column rig, robot, rig, material, swatch, normal, metallic-roughness"
    );
    let cache = project_cache_dir(&dir);
    assert_eq!(cache, dir.join(".fynkel/cache"));
    let index: serde_json::Value =
        serde_json::from_slice(&std::fs::read(cache.join("index.json")).unwrap()).unwrap();
    assert_eq!(index["fynkel"], "cache-index");
    assert_eq!(index["project"]["name"], "Fixture Game");
    assert_eq!(index["project"]["startupScene"], MAIN);
    assert_eq!(
        index["schemaFingerprint"],
        fynkel_import::cook::schema_fingerprint_hex()
    );
    assert_eq!(index["containerVersion"], fynkel_cooked::VERSION);
    let main_entry = index["assets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == MAIN)
        .unwrap();
    assert_eq!(main_entry["name"], "Main");
    assert_eq!(main_entry["status"], "cooked");

    // The scene chunk is the expanded scene: the crate prefab's entity is
    // under the instance with a derived id, the override applied, and no
    // `prefab` block remains. META says so, and names the behaviour types.
    let main = r.get(id(MAIN)).unwrap();
    let bytes = std::fs::read(main.file.as_ref().unwrap()).unwrap();
    let c = Container::parse(&bytes).unwrap();
    let meta: serde_json::Value = serde_json::from_slice(c.meta(1 << 20).unwrap()).unwrap();
    let info = &meta["chunks"][1];
    assert_eq!(info["kind"], "JSON");
    assert_eq!(info["documentKind"], "scene");
    assert_eq!(info["expanded"], true);
    assert_eq!(info["entities"], 7);
    assert_eq!(info["instances"], 1);
    assert_eq!(
        info["behaviourTypes"],
        serde_json::json!([{ "type": "game.Spinner", "version": 1 }])
    );
    let scene: serde_json::Value = serde_json::from_slice(c.read(1, 1 << 20).unwrap()).unwrap();
    let entities = scene["entities"].as_object().unwrap();
    assert_eq!(entities.len(), 7);
    let host = &entities["c0000000-0000-4000-8000-0000000000e2"];
    assert!(host.get("prefab").is_none(), "expanded: {host}");
    let child = host["children"][0].as_str().unwrap();
    assert_ne!(child, "c0000000-0000-4000-8000-0000000000b1");
    let crate_entity = &entities[child];
    assert_eq!(
        crate_entity["parent"],
        "c0000000-0000-4000-8000-0000000000e2"
    );
    let mesh_renderer = crate_entity["components"]
        .as_object()
        .unwrap()
        .values()
        .find(|c| c["type"] == "fynkel.MeshRenderer")
        .unwrap();
    assert_eq!(mesh_renderer["data"]["cast_shadows"], false);
    // The prefab's own chunk keeps its template form.
    let prefab = r.get(id("c0000000-0000-4000-8000-000000000040")).unwrap();
    let bytes = std::fs::read(prefab.file.as_ref().unwrap()).unwrap();
    let c = Container::parse(&bytes).unwrap();
    let meta: serde_json::Value = serde_json::from_slice(c.meta(1 << 20).unwrap()).unwrap();
    assert_eq!(meta["chunks"][1]["documentKind"], "prefab");
    assert_eq!(meta["chunks"][1]["entities"], 1);

    // Cooking again hits everything; the same scene key.
    let again = cook_project(&dir, None, &CookOptions::default()).unwrap();
    assert!(all(&again, Status::Cached));
    assert_eq!(again.get(id(MAIN)).unwrap().key, main.key);

    // An invalid scene (dangling parent) fails its own cook with the
    // document diagnostic; the rest of the project is untouched.
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fynkel_document/fixtures/scene/dangling_parent.scene.json"),
        dir.join("assets/scenes/bad.scene.json"),
    )
    .unwrap();
    let r = cook_project(&dir, None, &CookOptions::default()).unwrap();
    assert!(r.has_errors());
    let bad = r
        .assets
        .iter()
        .find(|a| a.path == "scenes/bad.scene.json")
        .unwrap();
    assert_eq!(bad.status, Status::Failed);
    assert!(r
        .diagnostics
        .iter()
        .any(|d| d.code == Code::SCENE_DANGLING_PARENT));
    assert_eq!(status(&r, MAIN), Status::Cached);
    let index: serde_json::Value =
        serde_json::from_slice(&std::fs::read(cache.join("index.json")).unwrap()).unwrap();
    let bad_entry = index["assets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["path"] == "scenes/bad.scene.json")
        .unwrap();
    assert_eq!(bad_entry["status"], "failed");
    assert!(bad_entry["file"].is_null());

    // A project with errors cooks nothing.
    std::fs::remove_file(dir.join("assets/scenes/bad.scene.json")).unwrap();
    std::fs::remove_file(dir.join("fynkel.project.json")).unwrap();
    let err = cook_project(&dir, None, &CookOptions::default()).unwrap_err();
    assert_eq!(err[0].code, Code::PROJECT_MISSING);
    let _ = std::fs::remove_dir_all(&dir);
}

/// M3-19b: an FBX source cooks through the same path as a glTF one —
/// its meshes, skin, clips and prefab land as the same chunk kinds
/// `rig.glb` gives, under `fynkel.fbx`, and cook again from the cache.
#[test]
fn an_fbx_source_cooks_like_a_gltf_one() {
    const RIG: &str = "c0000000-0000-4000-8000-0000000000fb";
    let p = Project::new("fbx");
    std::fs::copy(
        fixtures().join("rig.fbx"),
        p.assets().join("models/rig.fbx"),
    )
    .unwrap();
    p.write(
        "models/rig.fbx.meta.json",
        &format!(
            r#"{{
  "$schema": "https://fynkel.dev/schemas/meta.schema.json",
  "formatVersion": 1,
  "id": "{RIG}",
  "name": "rig.fbx",
  "importer": "fynkel.fbx",
  "importerVersion": 1,
  "settings": {{}},
  "subAssets": {{}}
}}
"#
        ),
    );
    let r = p.cook();
    assert!(!r.has_errors(), "{:?}", r.diagnostics);
    assert_eq!(r.assets.len(), 7);
    let rig = r.get(id(RIG)).unwrap();
    assert_eq!(rig.status, Status::Cooked);
    let mut f = ChunkFile::open_path(rig.file.as_ref().unwrap()).unwrap();
    f.verify().unwrap();
    let meta: serde_json::Value = serde_json::from_slice(&f.meta(1 << 20).unwrap()).unwrap();
    assert_eq!(meta["kind"], "fynkel.fbx");
    assert_eq!(meta["importer"], "fynkel.fbx");
    assert_eq!(meta["importerVersion"], 1);
    let kinds: Vec<String> = f.chunks().iter().map(|c| c.kind_str()).collect();
    assert_eq!(
        kinds,
        ["META", "MESH", "MESH", "SKIN", "ANIM", "ANIM", "JSON"]
    );
    let subs: Vec<&str> = meta["chunks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["sub"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(
        subs,
        ["", "mesh/0", "mesh/1", "skin/0", "anim/0", "anim/1", "prefab"]
    );
    assert_eq!(meta["chunks"][3]["joints"], 3);
    assert_eq!(meta["chunks"][4]["name"], "bob");
    assert_eq!(meta["chunks"][5]["name"], "wave");
    assert_eq!(meta["chunks"][6]["documentKind"], "prefab");
    assert_eq!(meta["chunks"][6]["entities"], 6);
    let again = p.cook();
    assert_eq!(status(&again, RIG), Status::Cached);
}

/// M3-19b: a rig document cooks into one object a game names — the
/// skeleton file's hierarchy as its `prefab` (the entity ids kept) and
/// one `ANIM` per clip under the rig's names, clips taken from the
/// skeleton file and from another file (the FBX twin of the rig) matched
/// by bone name; at runtime the rig depends on the skeleton only, while
/// its key covers every clip source (a clip file's re-import recooks the
/// rig, nothing else). A clip whose bones the skeleton lacks is
/// `FYN-A0106` and fails the rig; a scene naming it is blocked.
#[test]
fn a_rig_document_cooks_its_skeleton_and_clips() {
    const GLB: &str = "b0000000-0000-4000-8000-000000000012";
    const FBX: &str = "c0000000-0000-4000-8000-0000000000fb";
    const HERO: &str = "c0000000-0000-4000-8000-000000000050";
    let p = Project::new("rig-doc");
    for name in ["rig.glb", "rig.fbx"] {
        std::fs::copy(fixtures().join(name), p.assets().join("models").join(name)).unwrap();
    }
    // Sidecars as `fynkel import` writes them: every sub-asset listed.
    let sidecar = |name: &str, asset: &str, importer: &str, settings: &str| {
        let file = p.assets().join("models").join(name);
        let text = format!(
            r#"{{"$schema":"https://fynkel.dev/schemas/meta.schema.json","formatVersion":1,"id":"{asset}","name":"{name}","importer":"{importer}","importerVersion":1,"settings":{settings},"subAssets":{{}}}}"#
        );
        let mut meta = fynkel_document::Meta::from_document(
            fynkel_document::Document::parse(fynkel_document::DocumentKind::Meta, None, &text)
                .unwrap(),
        );
        let imported = fynkel_import::import(&file, &meta, &fynkel_import::Limits::default());
        assert!(!imported.has_errors(), "{:?}", imported.diagnostics);
        meta.sub_assets = imported.sub_assets;
        p.write(
            &format!("models/{name}.meta.json"),
            &meta.to_document().write().unwrap(),
        );
    };
    sidecar("rig.glb", GLB, "fynkel.gltf", "{}");
    sidecar("rig.fbx", FBX, "fynkel.fbx", "{}");
    std::fs::create_dir_all(p.assets().join("rigs")).unwrap();
    let rig_doc = |skeleton: &str, clips: &str| {
        format!(
            r#"{{"$schema":"https://fynkel.dev/schemas/rig.schema.json","formatVersion":1,"id":"{HERO}","name":"Hero","skeleton":{{"asset":"{skeleton}"}},"clips":{{{clips}}}}}"#
        )
    };
    // The glTF's wave (anim/0) and the FBX's bob (anim/0) and wave (anim/1).
    p.write(
        "rigs/hero.rig.json",
        &rig_doc(
            GLB,
            &format!(
                r#""wave":{{"asset":"{GLB}","sub":"anim/0"}},"bob":{{"asset":"{FBX}","sub":"anim/0"}},"wave_fbx":{{"asset":"{FBX}","sub":"anim/1"}}"#
            ),
        ),
    );
    let r = p.cook();
    assert!(!r.has_errors(), "{:?}", r.diagnostics);
    assert_eq!(r.assets.len(), 9);
    let hero = r.get(id(HERO)).unwrap();
    assert_eq!(hero.status, Status::Cooked);
    assert_eq!(hero.kind, "document/rig");
    assert_eq!(
        hero.dependencies,
        vec![id(GLB)],
        "the runtime needs the skeleton only"
    );
    let mut f = ChunkFile::open_path(hero.file.as_ref().unwrap()).unwrap();
    f.verify().unwrap();
    let meta: serde_json::Value = serde_json::from_slice(&f.meta(1 << 20).unwrap()).unwrap();
    assert_eq!(meta["dependencies"], serde_json::json!([GLB]));
    let kinds: Vec<String> = f.chunks().iter().map(|c| c.kind_str()).collect();
    assert_eq!(kinds, ["META", "JSON", "JSON", "ANIM", "ANIM", "ANIM"]);
    let chunks = meta["chunks"].as_array().unwrap();
    let subs: Vec<&str> = chunks
        .iter()
        .map(|c| c["sub"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(
        subs,
        ["", "", "prefab", "anim/wave", "anim/bob", "anim/wave_fbx"]
    );
    assert_eq!(chunks[1]["documentKind"], "rig");
    assert_eq!(chunks[2]["documentKind"], "prefab");
    assert_eq!(chunks[2]["entities"], 6);
    assert_eq!(chunks[2]["skeleton"], GLB);
    let derived = |sub: &str| Uuid::new_v5(&id(HERO), sub.as_bytes()).to_string();
    assert_eq!(chunks[2]["id"], derived("prefab"));
    assert_eq!(chunks[3]["id"], derived("anim/wave"));
    assert_eq!(chunks[3]["channels"], 3);
    assert_eq!(chunks[3]["source"], "models/rig.glb#anim/0");
    // The FBX clips are baked over every node (M3-19b part 1), and every
    // node name of the FBX twin exists in the glTF hierarchy: nothing
    // dropped, nothing missing.
    let fbx = r.get(id(FBX)).unwrap();
    let mut fbx_file = ChunkFile::open_path(fbx.file.as_ref().unwrap()).unwrap();
    let fbx_meta: serde_json::Value =
        serde_json::from_slice(&fbx_file.meta(1 << 20).unwrap()).unwrap();
    let fbx_channels = |sub: &str| {
        fbx_meta["chunks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["sub"] == sub)
            .map(|c| c["channels"].clone())
            .unwrap()
    };
    assert_eq!(chunks[4]["channels"], fbx_channels("anim/0"));
    assert_eq!(chunks[4]["source"], "models/rig.fbx#anim/0");
    assert_eq!(chunks[4]["dropped"], serde_json::json!([]));
    assert_eq!(chunks[5]["channels"], fbx_channels("anim/1"));
    assert_eq!(chunks[5]["dropped"], serde_json::json!([]));
    assert_eq!(
        meta["subAssets"]["anim/bob"],
        serde_json::Value::from(derived("anim/bob"))
    );
    // The prefab chunk is the skeleton's hierarchy under the rig's id.
    let prefab: serde_json::Value = serde_json::from_slice(&f.read(2, 1 << 20).unwrap()).unwrap();
    assert_eq!(prefab["id"], derived("prefab"));
    assert_eq!(prefab["name"], "Hero");
    let source = r.get(id(GLB)).unwrap();
    let mut g = ChunkFile::open_path(source.file.as_ref().unwrap()).unwrap();
    let source_meta: serde_json::Value = serde_json::from_slice(&g.meta(1 << 20).unwrap()).unwrap();
    let source_prefab_index = source_meta["chunks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["sub"] == "prefab")
        .unwrap()["index"]
        .as_u64()
        .unwrap() as usize;
    let source_prefab: serde_json::Value =
        serde_json::from_slice(&g.read(source_prefab_index, 1 << 20).unwrap()).unwrap();
    assert_eq!(prefab["entities"], source_prefab["entities"]);
    assert_eq!(prefab["roots"], source_prefab["roots"]);
    // The FBX's bob moves the glTF hierarchy's Prop node.
    let bob = fynkel_cooked::payload::decode_animation(&f.read(4, 1 << 20).unwrap()).unwrap();
    assert!(bob
        .channels
        .iter()
        .any(|c| c.target == "Prop"
            && c.property == fynkel_cooked::payload::ANIM_PROPERTY_TRANSLATION));
    assert!((bob.duration - 2.0).abs() < 1e-3);

    // Cached; a clip file's sidecar edit recooks it and the rig, not the
    // skeleton; a rig edit recooks the rig alone.
    let again = p.cook();
    assert!(all(&again, Status::Cached), "{:?}", statuses(&again));
    sidecar("rig.fbx", FBX, "fynkel.fbx", r#"{"x-note":1}"#);
    let r = p.cook();
    assert!(!r.has_errors(), "{:?}", r.diagnostics);
    assert_eq!(status(&r, FBX), Status::Cooked);
    assert_eq!(status(&r, HERO), Status::Cooked);
    assert_eq!(status(&r, GLB), Status::Cached);
    p.write(
        "rigs/hero.rig.json",
        &rig_doc(
            GLB,
            &format!(r#""wave":{{"asset":"{GLB}","sub":"anim/0"}}"#),
        ),
    );
    let r = p.cook();
    assert!(!r.has_errors(), "{:?}", r.diagnostics);
    assert_eq!(status(&r, HERO), Status::Cooked);
    assert_eq!(status(&r, FBX), Status::Cached);
    assert_eq!(status(&r, GLB), Status::Cached);
    assert_eq!(r.get(id(HERO)).unwrap().chunks.len(), 4);

    // The robot has no skin: the glTF rig's wave animates hips/spine/head,
    // which it lacks — A0106 naming the first, the rig fails, a scene
    // naming it is blocked.
    p.write(
        "rigs/hero.rig.json",
        &rig_doc(
            ROBOT,
            &format!(r#""wave":{{"asset":"{GLB}","sub":"anim/0"}}"#),
        ),
    );
    let mut scene: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(p.assets().join("scenes/main.scene.json")).unwrap(),
    )
    .unwrap();
    scene["entities"]["c0000000-0000-4000-8000-0000000000e9"] = serde_json::json!({
        "name": "Hero", "parent": null, "children": [],
        "components": {
            "c0000000-0000-4000-8000-0000000000c9": {
                "type": "fynkel.Animator", "version": 1,
                "data": { "clip": { "asset": HERO, "sub": "anim/wave" }, "speed": 1.0, "looping": true, "playing": true, "weight": 1.0, "additive": false, "root_motion": false }
            }
        },
        "prefab": { "asset": { "asset": HERO, "sub": "prefab" }, "instance": "c0000000-0000-4000-8000-0000000000f9" }
    });
    p.write(
        "scenes/main.scene.json",
        &serde_json::to_string_pretty(&scene).unwrap(),
    );
    let r = p.cook();
    assert_eq!(status(&r, HERO), Status::Failed);
    assert_eq!(status(&r, MAIN), Status::Failed);
    let mismatch: Vec<&fynkel_contracts::Diagnostic> = r
        .diagnostics
        .iter()
        .filter(|d| d.code == fynkel_contracts::Code::RIG_BONE_MISMATCH)
        .collect();
    assert_eq!(mismatch.len(), 1, "{:?}", r.diagnostics);
    assert!(
        mismatch[0].message.contains("\"head\"") && mismatch[0].message.contains("2 more"),
        "{}",
        mismatch[0].message
    );
    assert_eq!(mismatch[0].pointer.as_deref(), Some("/clips/wave"));
    assert!(r
        .diagnostics
        .iter()
        .any(|d| d.code == fynkel_contracts::Code::IMPORT_SKIPPED
            && d.message.contains("main.scene.json")
            && d.message.contains("hero.rig.json failed")));
}
