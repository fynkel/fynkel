//! A scene file reaches pixels: the fixture project is cooked into a cache,
//! its startup scene is read through `fynkel_document`, loaded into a core
//! world and rendered offscreen from the scene's own camera — the cooked
//! robot mesh with the rusted material (base colour × swatch texture) and
//! the crate prefab instance. A mesh the cache lacks draws magenta.

use std::path::{Path, PathBuf};

use fynkel_core::{World, WorldConfig};
use fynkel_document::asset::AssetDatabase;
use fynkel_document::{load_project, SchemaSet};
use fynkel_import::{cook, Cache, CookOptions};
use fynkel_render::{
    clear_rgba8, headless_device, render_offscreen, CookedAssets, ExtractRenderer,
};

fn fixture_project() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fynkel_document/fixtures/project")
        .canonicalize()
        .unwrap()
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

struct TempProject {
    dir: PathBuf,
}

impl TempProject {
    fn new(name: &str) -> TempProject {
        let dir = std::env::temp_dir().join(format!("fynkel-render-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        copy_dir(&fixture_project(), &dir);
        TempProject { dir }
    }

    /// `fynkel cook` for this project: `<dir>/.fynkel/cache`.
    fn cook(&self) -> PathBuf {
        let schemas = SchemaSet::embedded();
        let (db, diags) = AssetDatabase::scan(&self.dir.join("assets"), &schemas);
        assert!(!fynkel_contracts::has_errors(&diags), "{diags:?}");
        let cache = self.dir.join(".fynkel/cache");
        let report = cook(&db, &schemas, &Cache::at(&cache), &CookOptions::default());
        assert!(!report.has_errors(), "{}", report.text());
        cache
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn is_background(p: [u8; 4]) -> bool {
    p == clear_rgba8()
}

#[test]
fn fixture_project_renders_its_cooked_meshes_from_the_scene_camera() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let project = TempProject::new("fixture");
    let cache = project.cook();

    let loaded = load_project(&project.dir).unwrap_or_else(|d| panic!("{d:?}"));
    assert_eq!(loaded.project_name, "Fixture Game");
    assert_eq!(loaded.cache, cache);
    // Camera, Robot, the "Crate A" instance and the crate it expands to.
    assert_eq!(loaded.scene.entities.len(), 4);

    let mut world = World::new(WorldConfig::default());
    world.load_scene(&loaded.scene).unwrap();
    world.begin_frame(1.0 / 60.0);
    world.step();
    let extract = world.extract();
    assert!(
        extract.camera.is_some(),
        "the scene's Camera drives the view"
    );
    assert_eq!(extract.meshes.len(), 2, "robot + crate");

    let (w, h) = (256, 256);
    let mut renderer = ExtractRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, w, h);
    renderer.placeholders = false;
    renderer.set_assets(Some(CookedAssets::open(&cache).unwrap()));
    let px = render_offscreen(&device, &queue, &mut renderer, &extract);
    assert_eq!(renderer.asset_errors(), &[] as &[String]);
    let stats = renderer.stats();
    assert_eq!(
        (
            stats.instances,
            stats.meshes,
            stats.missing,
            stats.placeholders
        ),
        (2, 2, 0, 0)
    );
    assert!(stats.scene_camera && !stats.scene_light);
    assert_eq!(stats.batches, 1, "same mesh + same texture = one draw");

    // Rusted base colour [0.8, 0.5, 0.3] × the swatch (red ramps 0→180
    // along u over a constant blue 200): the left of each face is blue,
    // the right is red — so the texture, not just the factor, is on screen.
    let drawn: Vec<[u8; 4]> = px.iter().copied().filter(|p| !is_background(*p)).collect();
    assert!(
        drawn.len() > 500,
        "two unit cubes at 6 m fill more than {} pixels",
        drawn.len()
    );
    if let Ok(out) = std::env::var("FYNKEL_RENDER_DUMP") {
        let mut ppm = format!("P6 {w} {h} 255\n").into_bytes();
        for p in &px {
            ppm.extend_from_slice(&p[..3]);
        }
        std::fs::write(out, ppm).unwrap();
    }
    let blue = drawn.iter().filter(|p| p[2] > p[0] + 20).count();
    let red = drawn.iter().filter(|p| p[0] > p[2] + 20).count();
    assert!(
        blue > 50 && red > 50,
        "blue {blue}, red {red} of {}",
        drawn.len()
    );
    // The camera at (0, 2, 6) looks down -Z; the top-left corner is sky.
    assert!(is_background(px[(2 * w + 2) as usize]));

    // Without the cache nothing is on the GPU: both meshes draw as missing
    // (magenta) and the errors say why.
    renderer.set_assets(None);
    let px = render_offscreen(&device, &queue, &mut renderer, &extract);
    assert_eq!(renderer.stats().missing, 2);
    assert_eq!(
        renderer.asset_errors().len(),
        1,
        "the shared mesh is tried once"
    );
    assert!(renderer.asset_errors()[0].contains("set_assets"));
    let magenta = px
        .iter()
        .filter(|p| !is_background(**p) && p[0] > 100 && p[2] > 100 && p[1] < 60)
        .count();
    assert!(magenta > 500, "{magenta} magenta pixels");
}

#[test]
fn placeholders_off_hides_entities_without_meshes() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let project = TempProject::new("placeholders");
    let loaded = load_project(&project.dir).unwrap_or_else(|d| panic!("{d:?}"));
    let mut world = World::new(WorldConfig::default());
    world.load_scene(&loaded.scene).unwrap();
    world.begin_frame(1.0 / 60.0);
    world.step();
    let extract = world.extract();

    let mut renderer = ExtractRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64);
    // No assets, placeholders on: the camera and the instance host entity
    // become cubes too.
    render_offscreen(&device, &queue, &mut renderer, &extract);
    let s = renderer.stats();
    assert_eq!((s.instances, s.placeholders, s.missing), (4, 2, 2));
    renderer.placeholders = false;
    render_offscreen(&device, &queue, &mut renderer, &extract);
    let s = renderer.stats();
    assert_eq!((s.instances, s.placeholders, s.missing), (2, 0, 2));
}
