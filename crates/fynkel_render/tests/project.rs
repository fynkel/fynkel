//! A scene file reaches pixels — from the cache alone (M3-03 gate): the
//! fixture project is cooked into a cache, its startup scene is read
//! through `fynkel_assets`, loaded into a core world and rendered
//! offscreen from the scene's own camera — the cooked robot mesh with the
//! rusted material (base colour × swatch texture) and the crate prefab
//! instance (expanded by the cook). With the source tree deleted the same
//! cache renders the identical image. A mesh the cache lacks draws
//! magenta.

use std::path::{Path, PathBuf};

use fynkel_core::{World, WorldConfig};
use fynkel_import::{cook_project, CookOptions};
use fynkel_render::{clear_rgba8, headless_device, render_offscreen, Assets, ExtractRenderer};

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
        let report = cook_project(&self.dir, None, &CookOptions::default())
            .unwrap_or_else(|d| panic!("{d:#?}"));
        assert!(!report.has_errors(), "{}", report.text());
        self.dir.join(".fynkel/cache")
    }

    /// Deletes everything but the cache: what a shipped game has.
    fn remove_sources(&self) {
        for entry in std::fs::read_dir(&self.dir).unwrap() {
            let entry = entry.unwrap();
            if entry.file_name() == ".fynkel" {
                continue;
            }
            if entry.file_type().unwrap().is_dir() {
                std::fs::remove_dir_all(entry.path()).unwrap();
            } else {
                std::fs::remove_file(entry.path()).unwrap();
            }
        }
        assert!(!self.dir.join("fynkel.project.json").exists());
        assert!(!self.dir.join("assets").exists());
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The startup scene, loaded and stepped once, with its closure retained.
fn load_startup(assets: &Assets) -> (World, fynkel_core::Extract, fynkel_assets::Residency) {
    let id = assets.startup_scene().unwrap();
    let cooked = assets.scene(id).unwrap_or_else(|d| panic!("{d:#?}"));
    assert_eq!(cooked.diagnostics, vec![]);
    assert_eq!(cooked.name, "Main");
    let mut world = World::new(WorldConfig::default());
    let loaded = world.load_scene(&cooked.description).unwrap();
    // Camera, Robot, Sun, the "Crate A" instance and the crate it expands
    // to, the physics Floor and Crate B.
    assert_eq!(loaded.len(), 7);
    let residency = assets.retain(&cooked.closure);
    world.begin_frame(1.0 / 60.0);
    world.step();
    let extract = world.extract();
    (world, extract, residency)
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
    let assets = Assets::open(&cache).unwrap_or_else(|d| panic!("{d}"));
    assert_eq!(assets.project().unwrap().name, "Fixture Game");
    let (_world, extract, residency) = load_startup(&assets);
    assert!(
        extract.camera.is_some(),
        "the scene's Camera drives the view"
    );
    assert_eq!(extract.meshes.len(), 2, "robot + crate");

    let (w, h) = (256, 256);
    let mut renderer = ExtractRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, w, h);
    renderer.placeholders = false;
    renderer.set_assets(Some(assets.clone()));
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
    assert!(stats.scene_camera && stats.scene_light, "the Sun lights it");
    assert_eq!(stats.batches, 1, "same mesh + same material = one draw");

    // Rusted base colour [1, 0.7, 0.5] × the swatch (red ramps 0→180
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

    // The store holds what the frame needed; releasing the scene's
    // residency evicts it, and the renderer drops its GPU copies on the
    // next frame (then reloads what the extract still references).
    let before = assets.stats();
    assert_eq!(
        (before.meshes, before.textures, before.materials),
        (1, 3, 1),
        "swatch, normal and metallic-roughness maps"
    );
    assert_eq!(before.retained, 7);
    drop(residency);
    let after = assets.stats();
    assert_eq!((after.meshes, after.textures, after.materials), (0, 0, 0));
    assert_eq!(after.retained, 0);
    assert_eq!(after.evictions, before.evictions + 1, "one eviction event");
    let px2 = render_offscreen(&device, &queue, &mut renderer, &extract);
    assert_eq!(px2, px, "reloaded from the cache: the same image");

    // Gate: the source tree is gone; the cache alone renders the same
    // image through a fresh store.
    project.remove_sources();
    let alone = Assets::open(&cache).unwrap_or_else(|d| panic!("{d}"));
    let (_world, extract2, _residency) = load_startup(&alone);
    let mut fresh = ExtractRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, w, h);
    fresh.placeholders = false;
    fresh.set_assets(Some(alone));
    let px3 = render_offscreen(&device, &queue, &mut fresh, &extract2);
    assert_eq!(fresh.asset_errors(), &[] as &[String]);
    assert_eq!(px3, px, "no source file was needed");

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
    let assets = Assets::open(&project.cook()).unwrap_or_else(|d| panic!("{d}"));
    let (_world, extract, _residency) = load_startup(&assets);

    let mut renderer = ExtractRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64);
    // No assets, placeholders on: the camera, the instance host entity and
    // the physics Floor and Crate B become cubes too (the Sun's cube sits
    // outside the view: culled).
    render_offscreen(&device, &queue, &mut renderer, &extract);
    let s = renderer.stats();
    assert_eq!(
        (s.instances, s.culled, s.placeholders, s.missing),
        (6, 1, 5, 2)
    );
    renderer.placeholders = false;
    render_offscreen(&device, &queue, &mut renderer, &extract);
    let s = renderer.stats();
    assert_eq!(
        (s.instances, s.culled, s.placeholders, s.missing),
        (2, 0, 0, 2)
    );
}

/// The rig scene (M3-19): the skinned column is drawn through the joint
/// palette — at the bind pose it is the cooked column, half a second into
/// "wave" the spine bends it 45° so it covers pixels the straight column
/// does not, and the same extract without palettes draws the bind pose.
/// Since M3-19a the scene holds the rig twice — hand-authored and
/// instanced from `rig.glb#prefab` — so every count is doubled and both
/// columns bend together.
#[test]
fn skinned_meshes_follow_their_joint_palette() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let project = TempProject::new("rig");
    let assets = Assets::open(&project.cook()).unwrap_or_else(|d| panic!("{d}"));
    let id: fynkel_contracts::AssetId = "c0000000-0000-4000-8000-000000000034".parse().unwrap();
    let cooked = assets.scene(id).unwrap_or_else(|d| panic!("{d:#?}"));
    assert_eq!(cooked.name, "Rig");
    let mut world = World::new(WorldConfig::default());
    world.load_scene(&cooked.description).unwrap();
    let (clips, skins, diags) = assets.register_animation(&cooked.description, &mut world);
    assert_eq!((clips, skins, diags.len()), (3, 1, 0));
    let _residency = assets.retain(&cooked.closure);

    let (w, h) = (320, 240);
    let mut renderer = ExtractRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, w, h);
    renderer.placeholders = false;
    renderer.set_assets(Some(assets.clone()));
    let coverage =
        |px: &[[u8; 4]]| -> Vec<bool> { px.iter().map(|p| !is_background(*p)).collect() };

    // Tick 1: the clip is at its start, the pose is the bind pose.
    world.begin_frame(1.0 / 60.0);
    world.step();
    let bind = world.extract();
    assert_eq!(
        bind.joints.len(),
        6,
        "hips, spine, head — authored and imported (M3-19a: `rig.glb#prefab` at x = −2)"
    );
    let px_bind = render_offscreen(&device, &queue, &mut renderer, &bind);
    assert_eq!(renderer.asset_errors(), &[] as &[String]);
    let s = renderer.stats();
    assert_eq!((s.instances, s.skinned, s.joints), (4, 2, 6));
    let straight = coverage(&px_bind);

    // The palette is identity at bind: the cooked column and the skinned
    // one are the same picture.
    let mut cooked_only = bind.clone();
    for m in &mut cooked_only.meshes {
        m.skin = None;
    }
    let px_cooked = render_offscreen(&device, &queue, &mut renderer, &cooked_only);
    assert_eq!(renderer.stats().skinned, 0);
    assert_eq!(px_cooked, px_bind);

    // Half a second in: the spine is 45° over, the hips lifted.
    for _ in 0..29 {
        world.begin_frame(1.0 / 60.0);
        world.step();
    }
    let posed = world.extract();
    let px_posed = render_offscreen(&device, &queue, &mut renderer, &posed);
    assert_eq!(renderer.stats().skinned, 2);
    let bent = coverage(&px_posed);
    let only_bent = bent
        .iter()
        .zip(&straight)
        .filter(|(b, s)| **b && !**s)
        .count();
    let only_straight = straight
        .iter()
        .zip(&bent)
        .filter(|(s, b)| **s && !**b)
        .count();
    assert!(
        only_bent > 200 && only_straight > 200,
        "the bent column covers {only_bent} new pixels and frees {only_straight}"
    );
    // The bend goes toward -X (45° about +Z): the column's upper half
    // leans left on screen, so the new pixels lie left of the centre.
    let left_new = bent
        .iter()
        .zip(&straight)
        .enumerate()
        .filter(|(i, (b, s))| **b && !**s && (*i as u32 % w) < w / 2)
        .count();
    assert!(
        left_new * 10 > only_bent * 8,
        "{left_new} of {only_bent} on the left"
    );

    // Without palettes the posed extract draws the bind pose again.
    let mut unposed = posed.clone();
    for m in &mut unposed.meshes {
        m.skin = None;
    }
    let px_unposed = render_offscreen(&device, &queue, &mut renderer, &unposed);
    let unposed_cov = coverage(&px_unposed);
    let body_diff = unposed_cov
        .iter()
        .zip(&straight)
        .filter(|(a, b)| a != b)
        .count();
    // Only the Prop moved (its "bob" clip animates the transform, not a
    // skin): a small box, a couple of hundred pixels at most.
    assert!(
        body_diff < 400,
        "{body_diff} pixels differ from the bind pose"
    );
}
