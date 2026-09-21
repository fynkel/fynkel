//! Asset database corpus (`fixtures/assets/`): a small project tree with a
//! duplicate id, an orphaned sidecar, a file without one, a sub-asset
//! reusing its asset's id and documents with good and bad references. The
//! move test is the M2 gate "asset moves preserve identity".

use std::path::{Path, PathBuf};

use fynkel_contracts::AssetRef;
use fynkel_document::asset::{asset_refs, sidecar_path, source_path, Meta};
use fynkel_document::{AssetDatabase, AssetKind, Document, DocumentKind, SchemaSet};
use uuid::Uuid;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/assets")
}

fn schemas() -> SchemaSet {
    SchemaSet::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas")).unwrap()
}

fn id(s: &str) -> Uuid {
    s.parse().unwrap()
}

const ROBOT: &str = "a0000000-0000-4000-8000-000000000001";
const RUST: &str = "a0000000-0000-4000-8000-000000000002";
const BODY: &str = "a0000000-0000-4000-8000-000000000011";
const RUSTED: &str = "a0000000-0000-4000-8000-000000000020";

#[test]
fn scan_reports_exact_diagnostics_and_indexes_the_rest() {
    let (db, diags) = AssetDatabase::scan(&fixtures(), &schemas());
    let shown: Vec<String> = diags.iter().map(|d| d.to_string()).collect();
    let summary: Vec<(&str, Option<&str>, &str)> = diags
        .iter()
        .map(|d| {
            (
                d.code.as_str(),
                d.pointer.as_deref(),
                d.document
                    .as_deref()
                    .and_then(|p| p.rsplit('/').next())
                    .unwrap_or(""),
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            ("FYN-A0003", Some("/subAssets/clip"), "hum.ogg.meta.json"),
            ("FYN-A0001", None, "loose.jpg"),
            ("FYN-A0002", None, "orphan.png.meta.json"),
            ("FYN-A0003", Some("/id"), "rust_copy.png.meta.json"),
        ],
        "{shown:#?}"
    );
    assert_eq!(diags[0].line, Some(10));
    assert_eq!(diags[3].line, Some(4));
    assert!(diags[3].message.contains("textures/rust.png"));

    // Documents and valid sources, in path order.
    let paths: Vec<&str> = db.entries().iter().map(|e| e.path.as_str()).collect();
    assert_eq!(
        paths,
        vec![
            "materials/broken.material.json",
            "materials/rusted.material.json",
            "models/robot.glb",
            "scenes/broken.scene.json",
            "scenes/main.scene.json",
            "textures/rust.png",
        ]
    );
    let robot = db.get(id(ROBOT)).unwrap();
    assert_eq!(
        robot.kind,
        AssetKind::Source {
            importer: "fynkel.gltf".into(),
            importer_version: 1
        }
    );
    assert_eq!(robot.sub_assets.len(), 3);
    assert_eq!(
        robot.sidecar(db.root()).unwrap(),
        fixtures().join("models/robot.glb.meta.json")
    );
    assert_eq!(
        db.get(id(RUSTED)).unwrap().kind,
        AssetKind::Document(DocumentKind::Material)
    );
    assert!(db.by_path("textures/rust.png").is_some());
    // The duplicate lost; the orphan and the sidecar-less file are absent.
    assert!(db.by_path("textures/rust_copy.png").is_none());
    assert!(db.by_path("textures/loose.jpg").is_none());
    assert!(db.get(id("a0000000-0000-4000-8000-000000000003")).is_none());
}

#[test]
fn references_resolve_by_id_and_sub_id() {
    let (db, _) = AssetDatabase::scan(&fixtures(), &schemas());
    let r = db.resolve(&AssetRef::new(cid(ROBOT))).unwrap();
    assert_eq!(r.entry.path, "models/robot.glb");
    assert!(r.sub.is_none());
    let r = db
        .resolve(&AssetRef::with_sub(cid(ROBOT), "mesh/Body"))
        .unwrap();
    assert_eq!(r.sub, Some(("mesh/Body".to_string(), id(BODY))));
    // A sub-asset's own uuid resolves to the same place.
    let r = db.resolve(&AssetRef::new(cid(BODY))).unwrap();
    assert_eq!(r.entry.path, "models/robot.glb");
    assert_eq!(r.sub, Some(("mesh/Body".to_string(), id(BODY))));
    assert_eq!(
        db.resolve(&AssetRef::with_sub(cid(ROBOT), "mesh/Nope"))
            .unwrap_err()
            .code
            .as_str(),
        "FYN-A0005"
    );
    assert_eq!(
        db.resolve(&AssetRef::new(cid("a0000000-0000-4000-8000-0000000000ee")))
            .unwrap_err()
            .code
            .as_str(),
        "FYN-A0004"
    );

    let schemas = schemas();
    for (file, expected) in [
        ("materials/rusted.material.json", vec![]),
        ("scenes/main.scene.json", vec![]),
        (
            "materials/broken.material.json",
            vec![
                ("FYN-A0004", "/base_color_texture", 7),
                ("FYN-A0004", "/normal_texture", 10),
            ],
        ),
        (
            "scenes/broken.scene.json",
            vec![
                ("FYN-A0005", "/entities/a0000000-0000-4000-8000-0000000000e1/components/a0000000-0000-4000-8000-0000000000c1/data/mesh", 16),
                ("FYN-A0005", "/entities/a0000000-0000-4000-8000-0000000000e1/components/a0000000-0000-4000-8000-0000000000c1/data/material", 20),
            ],
        ),
    ] {
        let doc = Document::read(&fixtures().join(file), None).unwrap();
        assert!(doc.validate(&schemas).is_empty(), "{file}");
        let diags = db.check_references(&doc);
        let got: Vec<(&str, &str, u32)> = diags
            .iter()
            .map(|d| (d.code.as_str(), d.pointer.as_deref().unwrap(), d.line.unwrap()))
            .collect();
        assert_eq!(got, expected, "{file}: {diags:#?}");
        assert!(diags.iter().all(|d| d.document.as_deref() == doc.path.as_deref()));
    }
    let doc = Document::read(&fixtures().join("scenes/main.scene.json"), None).unwrap();
    assert_eq!(asset_refs(&doc.root).len(), 2);
}

fn cid(s: &str) -> fynkel_contracts::Uuid {
    fynkel_contracts::Uuid::from_bytes(*id(s).as_bytes())
}

/// Copies the valid part of the corpus into a fresh directory.
fn valid_tree(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fynkel-asset-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for rel in [
        "models/robot.glb",
        "models/robot.glb.meta.json",
        "textures/rust.png",
        "textures/rust.png.meta.json",
        "materials/rusted.material.json",
        "scenes/main.scene.json",
    ] {
        let to = dir.join(rel);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(fixtures().join(rel), to).unwrap();
    }
    dir
}

#[test]
fn moving_a_file_with_its_sidecar_preserves_identity() {
    let schemas = schemas();
    let dir = valid_tree("move");
    let (before, diags) = AssetDatabase::scan(&dir, &schemas);
    assert!(diags.is_empty(), "{diags:#?}");
    assert_eq!(before.len(), 4);

    // Move the model into a subfolder under a new name, the texture too.
    let rename = |from: &str, to: &str| {
        let (from, to) = (dir.join(from), dir.join(to));
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::rename(&from, &to).unwrap();
        std::fs::rename(sidecar_path(&from), sidecar_path(&to)).unwrap();
    };
    rename("models/robot.glb", "models/props/bot.glb");
    rename("textures/rust.png", "textures/metal/rust.png");

    let (after, diags) = AssetDatabase::scan(&dir, &schemas);
    assert!(diags.is_empty(), "{diags:#?}");
    assert_eq!(after.len(), 4);
    let robot = after.get(id(ROBOT)).unwrap();
    assert_eq!(robot.path, "models/props/bot.glb");
    assert_eq!(robot.sub_assets, before.get(id(ROBOT)).unwrap().sub_assets);
    assert_eq!(after.get(id(RUST)).unwrap().path, "textures/metal/rust.png");
    assert_eq!(
        after.resolve(&AssetRef::new(cid(BODY))).unwrap().entry.path,
        "models/props/bot.glb"
    );
    // Every reference in the documents still resolves; the documents did
    // not change (they never held a path).
    for rel in ["materials/rusted.material.json", "scenes/main.scene.json"] {
        let doc = Document::read(&dir.join(rel), None).unwrap();
        assert!(after.check_references(&doc).is_empty(), "{rel}");
        assert_eq!(
            std::fs::read(dir.join(rel)).unwrap(),
            std::fs::read(fixtures().join(rel)).unwrap()
        );
    }

    // Moving the file alone breaks it in both directions.
    std::fs::rename(
        dir.join("textures/metal/rust.png"),
        dir.join("textures/rust.png"),
    )
    .unwrap();
    let (broken, diags) = AssetDatabase::scan(&dir, &schemas);
    let codes: Vec<&str> = diags.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(codes, vec!["FYN-A0002", "FYN-A0001"]);
    assert!(broken.get(id(RUST)).is_none());
    let doc = Document::read(&dir.join("materials/rusted.material.json"), None).unwrap();
    assert_eq!(broken.check_references(&doc).len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fresh_sidecars_validate_and_name_the_importer() {
    let schemas = schemas();
    let dir = valid_tree("init");
    std::fs::write(dir.join("textures/new.PNG"), b"png").unwrap();
    std::fs::write(dir.join("models/thing.gltf"), b"{}").unwrap();
    std::fs::write(dir.join("notes.txt"), b"hi").unwrap();
    let (_, diags) = AssetDatabase::scan(&dir, &schemas);
    assert_eq!(diags.len(), 3);
    assert!(diags.iter().all(|d| d.code.as_str() == "FYN-A0001"));

    let mut ids = Vec::new();
    for (rel, importer) in [
        ("textures/new.PNG", "fynkel.image"),
        ("models/thing.gltf", "fynkel.gltf"),
        ("notes.txt", "fynkel.file"),
    ] {
        let source = dir.join(rel);
        let meta = Meta::new(&source);
        assert_eq!(meta.importer, importer);
        assert_eq!(
            meta.document.envelope.name,
            source.file_name().unwrap().to_str().unwrap()
        );
        let doc = meta.to_document();
        assert!(
            doc.validate(&schemas).is_empty(),
            "{:#?}",
            doc.validate(&schemas)
        );
        let path = sidecar_path(&source);
        assert_eq!(source_path(&path).unwrap(), source);
        std::fs::write(&path, doc.write().unwrap()).unwrap();
        ids.push(meta.id());
    }
    let (db, diags) = AssetDatabase::scan(&dir, &schemas);
    assert!(diags.is_empty(), "{diags:#?}");
    assert_eq!(db.len(), 7);
    for id in ids {
        assert!(db.get(id).is_some());
    }
    // Re-reading a written sidecar gives the same view.
    let doc = Document::read(&dir.join("notes.txt.meta.json"), None).unwrap();
    let meta = Meta::from_document(doc);
    assert_eq!(meta.importer, "fynkel.file");
    assert_eq!(meta.importer_version, 1);
    assert!(meta.sub_assets.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// `(id, path, kind, sub-assets)` per entry, sorted by path.
type Snapshot = Vec<(String, String, String, Vec<(String, String)>)>;

/// The entries and the resolution of every id, as a full scan sees them.
fn snapshot(db: &AssetDatabase) -> Snapshot {
    db.entries()
        .iter()
        .map(|e| {
            (
                e.path.clone(),
                e.id.to_string(),
                e.name.clone(),
                e.sub_assets
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_string()))
                    .collect(),
            )
        })
        .collect()
}

#[test]
fn update_re_reads_only_the_named_files_and_agrees_with_a_full_scan() {
    let schemas = schemas();
    let dir = valid_tree("update");
    let (mut db, diags) = AssetDatabase::scan(&dir, &schemas);
    assert!(diags.is_empty(), "{diags:#?}");
    let full = |dir: &Path| AssetDatabase::scan(dir, &schemas).0;

    // A new material appears: the one path re-read, the entry there.
    let extra = dir.join("materials/extra.material.json");
    let mut doc = Document::new(DocumentKind::Material, "Extra");
    doc.path = Some(extra.display().to_string());
    std::fs::write(&extra, doc.write().unwrap()).unwrap();
    let diags = db.update(std::slice::from_ref(&extra), &schemas);
    assert!(diags.is_empty(), "{diags:#?}");
    assert_eq!(db.len(), 5);
    assert_eq!(
        db.by_path("materials/extra.material.json").unwrap().name,
        "Extra"
    );
    assert_eq!(snapshot(&db), snapshot(&full(&dir)));

    // A source and its sidecar move to a new folder: the old paths are
    // reported gone, the new ones read — the id and the sub-assets survive.
    let rename = |from: &str, to: &str| {
        let (from, to) = (dir.join(from), dir.join(to));
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::rename(&from, &to).unwrap();
        std::fs::rename(sidecar_path(&from), sidecar_path(&to)).unwrap();
    };
    rename("models/robot.glb", "models/props/bot.glb");
    let diags = db.update(
        &[
            dir.join("models/robot.glb"),
            dir.join("models/robot.glb.meta.json"),
            dir.join("models/props/bot.glb"),
            dir.join("models/props/bot.glb.meta.json"),
        ],
        &schemas,
    );
    assert!(diags.is_empty(), "{diags:#?}");
    assert_eq!(db.get(id(ROBOT)).unwrap().path, "models/props/bot.glb");
    assert_eq!(
        db.resolve(&AssetRef::new(cid(BODY))).unwrap().entry.path,
        "models/props/bot.glb"
    );
    assert_eq!(snapshot(&db), snapshot(&full(&dir)));

    // Only the sidecar is named (what a watcher may report first): the
    // pair is still read as one.
    let moved_sidecar = dir.join("models/props/bot.glb.meta.json");
    let text = std::fs::read_to_string(&moved_sidecar).unwrap();
    std::fs::write(
        &moved_sidecar,
        text.replace("\"name\": \"robot.glb\"", "\"name\": \"Bot\""),
    )
    .unwrap();
    let diags = db.update(std::slice::from_ref(&moved_sidecar), &schemas);
    assert!(diags.is_empty(), "{diags:#?}");
    assert_eq!(db.get(id(ROBOT)).unwrap().name, "Bot");
    assert_eq!(snapshot(&db), snapshot(&full(&dir)));

    // The texture's sidecar goes: the source is reported without one and
    // the entry is gone; putting it back restores the entry.
    let rust_sidecar = dir.join("textures/rust.png.meta.json");
    let sidecar_text = std::fs::read_to_string(&rust_sidecar).unwrap();
    std::fs::remove_file(&rust_sidecar).unwrap();
    let diags = db.update(std::slice::from_ref(&rust_sidecar), &schemas);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code.as_str(), "FYN-A0001");
    assert!(db.get(id(RUST)).is_none());
    assert_eq!(snapshot(&db), snapshot(&full(&dir)));
    std::fs::write(&rust_sidecar, &sidecar_text).unwrap();
    assert!(db
        .update(std::slice::from_ref(&rust_sidecar), &schemas)
        .is_empty());
    assert_eq!(db.get(id(RUST)).unwrap().path, "textures/rust.png");

    // A copy with the same id is kept out (FYN-A0003, the original wins);
    // deleting the original then admits the copy without naming it.
    let copy = dir.join("textures/copy.png");
    std::fs::copy(dir.join("textures/rust.png"), &copy).unwrap();
    std::fs::write(sidecar_path(&copy), &sidecar_text).unwrap();
    let diags = db.update(std::slice::from_ref(&copy), &schemas);
    assert_eq!(diags.len(), 1, "{diags:#?}");
    assert_eq!(diags[0].code.as_str(), "FYN-A0003");
    assert_eq!(db.get(id(RUST)).unwrap().path, "textures/rust.png");
    std::fs::remove_file(dir.join("textures/rust.png")).unwrap();
    std::fs::remove_file(&rust_sidecar).unwrap();
    let diags = db.update(&[dir.join("textures/rust.png")], &schemas);
    assert!(diags.is_empty(), "{diags:#?}");
    assert_eq!(db.get(id(RUST)).unwrap().path, "textures/copy.png");
    assert_eq!(snapshot(&db), snapshot(&full(&dir)));

    // A whole folder goes: naming the (now missing) directory drops what it held.
    std::fs::remove_dir_all(dir.join("models")).unwrap();
    let diags = db.update(&[dir.join("models")], &schemas);
    assert!(diags.is_empty(), "{diags:#?}");
    assert!(db.get(id(ROBOT)).is_none());
    assert_eq!(snapshot(&db), snapshot(&full(&dir)));

    // A path outside the root is ignored; a relative path works.
    assert!(db
        .update(&[PathBuf::from("/definitely/elsewhere.png")], &schemas)
        .is_empty());
    assert!(db
        .update(&[PathBuf::from("materials/extra.material.json")], &schemas)
        .is_empty());
    assert_eq!(db.len(), 4);
    let _ = std::fs::remove_dir_all(&dir);
}
