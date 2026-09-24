//! `fynkel thumbnail` over a copy of the fixture project: a prefab, a
//! model's mesh, a material (on the stand-in sphere, then on the
//! project's primitives Sphere once installed) and a texture, which has
//! none. Each tile is a PNG of the requested size at the documented path,
//! transparent where nothing was drawn and opaque where the asset is.
//! Skipped with a note when there is no GPU adapter (exit 2 and the
//! message), as the renderer's own pixel tests do.

use std::path::{Path, PathBuf};
use std::process::Command;

const CRATE_PREFAB: &str = "c0000000-0000-4000-8000-000000000040";
const ROBOT: &str = "c0000000-0000-4000-8000-000000000001";
const RUSTED: &str = "c0000000-0000-4000-8000-000000000020";
const SWATCH: &str = "c0000000-0000-4000-8000-000000000002";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        if e.file_name() == ".fynkel" {
            continue;
        }
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

/// The fixture copied to a fresh directory (removed on drop).
struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Temp {
        let dir =
            std::env::temp_dir().join(format!("fynkel-thumbnail-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        copy_dir(
            &root().join("crates/fynkel_document/fixtures/project"),
            &dir.join("project"),
        );
        Temp(dir)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs the command; `None` when there is no GPU adapter.
fn thumbnail(t: &Temp, args: &[&str]) -> Option<(i32, serde_json::Value)> {
    let out = Command::new(env!("CARGO_BIN_EXE_fynkel"))
        .arg("thumbnail")
        .arg("--project")
        .arg(t.0.join("project"))
        .arg("--out-dir")
        .arg(t.0.join("out"))
        .arg("--json")
        .args(args)
        .output()
        .expect("run fynkel");
    let stderr = String::from_utf8_lossy(&out.stderr);
    if out.status.code() == Some(2) && stderr.contains("no GPU adapter") {
        eprintln!("no GPU adapter; skipping");
        return None;
    }
    let json = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}{stderr}", String::from_utf8_lossy(&out.stdout)));
    Some((out.status.code().unwrap(), json))
}

/// The tile is `size` square, clear in its corners and drawn around its
/// centre.
fn check_tile(file: &Path, size: u32) {
    let img = image::open(file)
        .unwrap_or_else(|e| panic!("{}: {e}", file.display()))
        .to_rgba8();
    assert_eq!(img.dimensions(), (size, size), "{}", file.display());
    let s = size - 1;
    for (x, y) in [(0, 0), (s, 0), (0, s), (s, s)] {
        assert_eq!(img.get_pixel(x, y).0[3], 0, "{} corner", file.display());
    }
    let c = size / 2;
    let opaque = (c - 4..c + 4)
        .flat_map(|x| (c - 4..c + 4).map(move |y| (x, y)))
        .filter(|&(x, y)| img.get_pixel(x, y).0[3] == 255)
        .count();
    assert!(
        opaque >= 48,
        "{}: {opaque}/64 drawn at the centre",
        file.display()
    );
}

#[test]
fn thumbnails_are_drawn_where_the_editor_looks_for_them() {
    let t = Temp::new("fixture");
    let mesh = format!("{ROBOT}#mesh/Cube");
    let Some((code, json)) = thumbnail(&t, &["--size", "96", CRATE_PREFAB, &mesh, RUSTED, SWATCH])
    else {
        return;
    };
    assert_eq!(code, 0, "{json}");
    let out = t.0.join("out").canonicalize().unwrap();
    let items = json["thumbnails"].as_array().unwrap();
    assert_eq!(items.len(), 4, "one entry per argument, in order: {json}");
    let expected = [
        (CRATE_PREFAB, None, format!("{CRATE_PREFAB}.png")),
        (ROBOT, Some("mesh/Cube"), format!("{ROBOT}__mesh_Cube.png")),
        (RUSTED, None, format!("{RUSTED}.png")),
    ];
    for (item, (asset, sub, name)) in items.iter().zip(&expected) {
        assert_eq!(item["asset"], *asset);
        assert_eq!(item["sub"].as_str(), *sub);
        assert!(item.get("error").is_none(), "{item}");
        let file = PathBuf::from(item["file"].as_str().unwrap());
        assert!(file.is_absolute());
        assert_eq!(file, out.join(name));
        check_tile(&file, 96);
    }
    assert_eq!(items[3]["asset"], SWATCH);
    assert!(items[3]["sub"].is_null());
    assert_eq!(items[3]["error"], "no thumbnail for a texture");
    assert!(items[3].get("file").is_none());
    assert!(!out.join(format!("{SWATCH}.png")).exists());
}

#[test]
fn a_material_is_drawn_on_the_projects_sphere_once_it_has_one() {
    let t = Temp::new("primitives");
    let fynkel = t.0.join("project/assets/Fynkel");
    std::fs::create_dir_all(&fynkel).unwrap();
    let assets = root().join("crates/fynkel_authoring/assets");
    for f in ["Primitives.glb", "Primitives.glb.meta.json"] {
        std::fs::copy(assets.join(f), fynkel.join(f)).unwrap();
    }
    let painted = format!("{ROBOT}#material/Painted");
    let Some((code, json)) = thumbnail(&t, &[RUSTED, &painted]) else {
        return;
    };
    assert_eq!(code, 0, "{json}");
    let items = json["thumbnails"].as_array().unwrap();
    check_tile(Path::new(items[0]["file"].as_str().unwrap()), 128);
    assert!(items[1]["file"]
        .as_str()
        .unwrap()
        .ends_with(&format!("{ROBOT}__material_Painted.png")));
    check_tile(Path::new(items[1]["file"].as_str().unwrap()), 128);
}

#[test]
fn usage_and_unknown_assets() {
    let t = Temp::new("usage");
    for bad in [
        vec!["--size", "8", RUSTED],
        vec!["--size", "2048", RUSTED],
        vec!["not-a-uuid"],
        vec![],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_fynkel"))
            .arg("thumbnail")
            .arg("--project")
            .arg(t.0.join("project"))
            .arg("--out-dir")
            .arg(t.0.join("out"))
            .args(&bad)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{bad:?}");
    }
    let unknown = "c0000000-0000-4000-8000-0000000000ff";
    let Some((code, json)) = thumbnail(&t, &[unknown]) else {
        return;
    };
    assert_eq!(code, 1, "{json}");
    assert_eq!(json["thumbnails"][0]["asset"], unknown);
    assert!(json["thumbnails"][0]["error"].is_string());
}
