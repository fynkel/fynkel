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
use fynkel_render::{
    clear_rgba8, headless_device, render_offscreen, Assets, ExtraWorld, ExtractRenderer,
    OutlineStyle,
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
    // Camera, Robot, Sun, the "Crate A" instance (the crate prefab's
    // root), the physics Floor and Crate B.
    assert_eq!(loaded.len(), 6);
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

    // A render extract owns the resources it references even after its
    // scene unloads (editor previews and persistent/runtime objects).
    let before = assets.stats();
    assert_eq!(
        (before.meshes, before.textures, before.materials),
        (1, 3, 1)
    );
    drop(residency);
    let px2 = render_offscreen(&device, &queue, &mut renderer, &extract);
    assert_eq!(px2, px, "the live extract keeps its resources");
    assert_eq!(assets.stats().meshes, 1);
    render_offscreen(
        &device,
        &queue,
        &mut renderer,
        &fynkel_core::Extract::default(),
    );
    let after = assets.stats();
    assert_eq!((after.meshes, after.textures, after.materials), (0, 0, 0));
    assert_eq!(after.retained, 0);
    assert!(after.evictions > before.evictions);

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
        2,
        "the shared mesh and material each report their unavailable source once"
    );
    assert_eq!(
        renderer
            .asset_errors()
            .iter()
            .filter(|e| e.starts_with("mesh "))
            .count(),
        1
    );
    assert_eq!(
        renderer
            .asset_errors()
            .iter()
            .filter(|e| e.starts_with("material "))
            .count(),
        1
    );
    assert!(renderer
        .asset_errors()
        .iter()
        .all(|e| e.contains("set_assets")));
    let magenta = px
        .iter()
        .filter(|p| !is_background(**p) && p[0] > 100 && p[2] > 100 && p[1] < 60)
        .count();
    assert!(magenta > 500, "{magenta} magenta pixels");
}

/// A mesh the game makes at runtime (`world.createMesh`, ADR-0026 §4):
/// drawn from the store like a cooked one, re-uploaded when it changes,
/// missing once removed.
#[test]
fn runtime_meshes_draw_and_change() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let project = TempProject::new("runtime-mesh");
    let assets = Assets::open(&project.cook()).unwrap_or_else(|d| panic!("{d}"));
    let (_world, mut extract, _residency) = load_startup(&assets);
    let (w, h) = (128, 128);
    let mut renderer = ExtractRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, w, h);
    renderer.placeholders = false;
    renderer.set_assets(Some(assets.clone()));
    let cooked_px = render_offscreen(&device, &queue, &mut renderer, &extract);
    let drawn = |px: &[[u8; 4]]| px.iter().filter(|p| !is_background(**p)).count();

    // Both cubes drawn through a runtime copy of their cooked mesh: the
    // same picture.
    let cooked = (*assets.mesh(&extract.meshes[0].mesh).unwrap()).clone();
    let made = fynkel_contracts::AssetRef::new(fynkel_contracts::Uuid(*b"runtime-mesh-002"));
    assets
        .set_runtime_mesh(&made, Some(cooked.clone()))
        .unwrap();
    for m in &mut extract.meshes {
        m.mesh = made.clone();
    }
    let px = render_offscreen(&device, &queue, &mut renderer, &extract);
    assert_eq!(renderer.asset_errors(), &[] as &[String]);
    assert_eq!(px, cooked_px);

    // Changed: the cubes shrunk to a tenth — the next frame uploads it.
    let mut small = cooked;
    for v in &mut small.vertices {
        v.position = v.position.map(|x| x * 0.1);
    }
    assets.set_runtime_mesh(&made, Some(small)).unwrap();
    let px = render_offscreen(&device, &queue, &mut renderer, &extract);
    assert!(
        drawn(&px) * 20 < drawn(&cooked_px),
        "{} of {} pixels",
        drawn(&px),
        drawn(&cooked_px)
    );

    // Removed: both draw as missing.
    assets.set_runtime_mesh(&made, None).unwrap();
    render_offscreen(&device, &queue, &mut renderer, &extract);
    assert_eq!(renderer.stats().missing, 2);
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
    // No assets, placeholders on: the camera and the physics Floor and
    // Crate B become cubes too (the Sun's cube sits outside the view:
    // culled).
    render_offscreen(&device, &queue, &mut renderer, &extract);
    let s = renderer.stats();
    assert_eq!(
        (s.instances, s.culled, s.placeholders, s.missing),
        (5, 1, 4, 2)
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

    // The selection outline is drawn through the palette too: its ring
    // hugs the bent columns, and part of it lies well away from where the
    // straight ones stood.
    let skinned: Vec<fynkel_contracts::Entity> = posed
        .meshes
        .iter()
        .filter(|m| m.skin.is_some())
        .map(|m| posed.records[m.record as usize].entity)
        .collect();
    assert_eq!(skinned.len(), 2);
    renderer
        .overlay()
        .outline(&skinned, &[], OutlineStyle::default());
    let px_ringed = render_offscreen(&device, &queue, &mut renderer, &posed);
    assert_eq!(renderer.stats().outlined, 2);
    let within = |cover: &[bool], i: usize, r: i32| {
        let (x, y) = ((i as u32 % w) as i32, (i as u32 / w) as i32);
        (-r..=r).any(|dy| {
            (-r..=r).any(|dx| {
                let (u, v) = (x + dx, y + dy);
                u >= 0
                    && v >= 0
                    && u < w as i32
                    && v < h as i32
                    && cover[(v * w as i32 + u) as usize]
            })
        })
    };
    let ring: Vec<usize> = (0..px_posed.len())
        .filter(|&i| px_ringed[i] != px_posed[i])
        .collect();
    assert!(ring.len() > 100, "{} ring pixels", ring.len());
    assert!(ring.iter().all(|&i| within(&bent, i, 3)));
    let away = ring.iter().filter(|&&i| !within(&straight, i, 4)).count();
    assert!(away > 20, "{away} ring pixels away from the bind pose");
}

/// Multi-scene editing and Prefab Mode in context: an extra world's
/// skinned meshes are posed by its own palettes, uploaded after the main
/// extract's. The rig scene posed half a second into "wave", drawn as an
/// extra world beside a main extract that draws nothing but holds the
/// bind pose's palettes, is the posed picture — reading the main
/// extract's palettes instead would draw the straight columns.
#[test]
fn an_extra_worlds_skinned_meshes_follow_its_own_palette() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let project = TempProject::new("rig-extra");
    let assets = Assets::open(&project.cook()).unwrap_or_else(|d| panic!("{d}"));
    let id: fynkel_contracts::AssetId = "c0000000-0000-4000-8000-000000000034".parse().unwrap();
    let cooked = assets.scene(id).unwrap_or_else(|d| panic!("{d:#?}"));
    let mut world = World::new(WorldConfig::default());
    world.load_scene(&cooked.description).unwrap();
    assets.register_animation(&cooked.description, &mut world);
    let _residency = assets.retain(&cooked.closure);
    let mut renderer = ExtractRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 320, 240);
    renderer.placeholders = false;
    renderer.set_assets(Some(assets.clone()));

    world.begin_frame(1.0 / 60.0);
    world.step();
    let bind = world.extract();
    for _ in 0..29 {
        world.begin_frame(1.0 / 60.0);
        world.step();
    }
    let posed = world.extract();
    assert_ne!(bind.joints, posed.joints);
    let px_posed = render_offscreen(&device, &queue, &mut renderer, &posed);
    assert_eq!(renderer.stats().skinned, 2);

    let mut main = posed.clone();
    main.meshes.clear();
    main.joints = bind.joints.clone();
    let mut extra = posed.clone();
    extra.directional_lights.clear();
    extra.point_lights.clear();
    extra.spot_lights.clear();
    renderer.push_extra_world(ExtraWorld {
        extract: extra,
        transform: glam::Mat4::IDENTITY,
        gray: false,
    });
    let px = render_offscreen(&device, &queue, &mut renderer, &main);
    let s = renderer.stats();
    assert_eq!((s.extra_worlds, s.skinned, s.joints), (1, 2, 12));
    assert_eq!(px, px_posed);
}

/// ADR-0038 §5: while a scene loads in the background the renderer puts
/// what the load made resident on the GPU, within a per-frame budget, so
/// the first frames after the switch upload nothing.
#[test]
fn the_renderer_warms_what_a_background_load_made_resident() {
    use std::sync::atomic::{AtomicBool, AtomicUsize};

    let Some((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let project = TempProject::new("warm");
    let cache = project.cook();
    let assets = Assets::open(&cache).unwrap_or_else(|d| panic!("{d}"));
    let mut renderer = ExtractRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64);
    renderer.placeholders = false;
    renderer.set_assets(Some(assets.clone()));
    let empty = World::new(WorldConfig::default()).extract();

    let id = assets.startup_scene().unwrap();
    let closure = assets.scene(id).unwrap().closure;
    let _token = assets.retain(&closure);
    assets
        .preload(&closure, &AtomicUsize::new(0), &AtomicBool::new(false))
        .unwrap();

    // A zero budget warms nothing; the default does, while drawing none
    // of it.
    renderer.warm_budget_us = 0;
    render_offscreen(&device, &queue, &mut renderer, &empty);
    assert_eq!(renderer.stats().warmed, 0);
    renderer.warm_budget_us = 1_000_000;
    render_offscreen(&device, &queue, &mut renderer, &empty);
    let warmed = renderer.stats().warmed;
    assert!(
        warmed >= 3,
        "the robot's mesh, texture and material at least: {warmed}"
    );
    let bytes = renderer.stats().gpu_bytes;
    render_offscreen(&device, &queue, &mut renderer, &empty);
    assert_eq!(renderer.stats().warmed, 0, "drained");

    // The scene then draws with nothing more to upload.
    let (_world, extract, _residency) = load_startup(&assets);
    render_offscreen(&device, &queue, &mut renderer, &extract);
    assert_eq!(renderer.asset_errors(), &[] as &[String]);
    assert_eq!(renderer.stats().gpu_bytes, bytes, "already on the GPU");
}

/// The bind mesh can be far outside both clip volumes while its posed vertices
/// remain visible. Culling and shadow fitting must use the actual palette.
#[test]
fn animated_bounds_keep_displaced_skins_and_extra_worlds_visible() {
    use glam::{Mat4, Vec3};
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let project = TempProject::new("rig-bounds");
    let assets = Assets::open(&project.cook()).unwrap();
    let cooked = assets
        .scene("c0000000-0000-4000-8000-000000000034".parse().unwrap())
        .unwrap();
    let mut world = World::new(WorldConfig::default());
    world.load_scene(&cooked.description).unwrap();
    assets.register_animation(&cooked.description, &mut world);
    let _residency = assets.retain(&cooked.closure);
    world.step();
    let mut extract = world.extract();
    for light in &mut extract.directional_lights {
        light.light.cast_shadows = true;
    }
    // A model/palette pair that leaves vertices at their original positions,
    // although its bind-space bound is 100 metres off-screen.
    for mesh in &extract.meshes {
        if let Some(skin) = mesh.skin {
            extract.records[mesh.record as usize].world *= Mat4::from_translation(Vec3::X * 100.0);
            for joint in &mut extract.joints
                [skin.first_joint as usize..(skin.first_joint + skin.joint_count) as usize]
            {
                *joint = Mat4::from_translation(Vec3::X * -100.0) * *joint;
            }
        }
    }
    let mut renderer = ExtractRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 320, 240);
    renderer.placeholders = false;
    renderer.set_assets(Some(assets));
    let culled = render_offscreen(&device, &queue, &mut renderer, &extract);
    let stats = renderer.stats();
    assert_eq!(stats.skinned, 2);
    assert!(stats.shadow_casters >= 2, "{stats:?}");
    assert_eq!(
        stats.bounds_unbounded, 0,
        "valid palettes must produce finite bounds"
    );
    assert!(stats.bounds_joints > 0);
    assert!(culled.iter().filter(|p| !is_background(**p)).count() > 500);
    renderer.frustum_culling = false;
    let all = render_offscreen(&device, &queue, &mut renderer, &extract);
    assert!(
        culled == all,
        "camera or shadow culling changed the posed image"
    );
    // Extra-world placement and its independent palette range participate too.
    let mut main = extract.clone();
    main.meshes.clear();
    main.joints.clear();
    let extra = ExtraWorld {
        extract: extract.clone(),
        transform: Mat4::from_translation(Vec3::new(0.1, 0.0, 0.0)),
        gray: false,
    };
    renderer.push_extra_world(extra.clone());
    renderer.frustum_culling = true;
    let extra_culled = render_offscreen(&device, &queue, &mut renderer, &main);
    assert_eq!(renderer.stats().skinned, 2);
    assert_eq!(renderer.stats().bounds_unbounded, 0);
    renderer.frustum_culling = false;
    renderer.push_extra_world(extra);
    assert!(
        extra_culled == render_offscreen(&device, &queue, &mut renderer, &main),
        "extra-world culling changed the image"
    );
}

#[test]
fn cold_draws_defer_cpu_work_and_share_upload_limits_with_visible_resources() {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let project = TempProject::new("f5-streaming");
    let assets = Assets::open(&project.cook()).unwrap();
    let (_world, extract, _lease) = load_startup(&assets);
    let mut renderer = ExtractRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64);
    renderer.set_assets(Some(assets.clone()));
    renderer.upload_budget.operations_per_frame = 1;
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    assets
        .schedule_preparation(fynkel_assets::PreparationPriority::Visible, move || {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        })
        .unwrap();
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    renderer.prepare(&device, &queue, &extract);
    assert_eq!(
        assets.stats().meshes,
        0,
        "draw must not decode on the caller"
    );
    assert_eq!(renderer.stats().asset_upload_operations, 0);
    assert!(
        renderer.assets_pending(),
        "idle hosts must request another frame"
    );
    release_tx.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut uploads = 0;
    loop {
        renderer.prepare(&device, &queue, &extract);
        let stats = renderer.stats();
        assert!(stats.asset_upload_operations <= 1);
        assert!(stats.asset_upload_bytes <= renderer.upload_budget.bytes_per_frame);
        uploads += stats.asset_upload_operations;
        if uploads >= 5 {
            break;
        } // mesh, three maps, material
        assert!(Instant::now() < deadline, "{:?}", renderer.asset_errors());
        std::thread::sleep(Duration::from_millis(1));
    }
    renderer.prepare(&device, &queue, &extract);
    assert_eq!(renderer.stats().asset_upload_operations, 0, "warm frame");
    assert!(
        renderer.asset_errors().is_empty(),
        "{:?}",
        renderer.asset_errors()
    );
}

#[test]
fn insufficient_gpu_budget_is_an_explicit_fallback_and_can_be_retried() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let project = TempProject::new("f5-gpu-budget");
    let assets = Assets::open(&project.cook()).unwrap();
    let (_world, extract, _lease) = load_startup(&assets);
    let mut renderer = ExtractRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64);
    renderer.set_assets(Some(assets));
    renderer.set_gpu_asset_budget(0).unwrap();
    renderer
        .prepare_ready(&device, &queue, &extract, std::time::Duration::from_secs(5))
        .unwrap_err();
    assert!(renderer
        .asset_errors()
        .iter()
        .any(|e| e.contains("GPU residency budget")));
    assert!(renderer.stats().missing > 0);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while renderer.assets_pending() {
        renderer.prepare(&device, &queue, &extract);
        assert!(
            std::time::Instant::now() < deadline,
            "terminal failures must eventually settle"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    renderer.set_gpu_asset_budget(1024 * 1024).unwrap();
    renderer.clear_assets();
    renderer
        .prepare_ready(&device, &queue, &extract, std::time::Duration::from_secs(5))
        .unwrap();
    assert!(
        renderer.asset_errors().is_empty(),
        "{:?}",
        renderer.asset_errors()
    );
    assert_eq!(renderer.stats().missing, 0);
    assert!(!renderer.assets_pending());
    assert!(
        renderer.set_gpu_asset_budget(0).is_err(),
        "shrinking below live GPU ownership must fail"
    );
}

/// Reproducible qualification workload, not a timing assertion in ordinary CI.
#[test]
#[ignore = "explicit GPU asset-streaming qualification workload"]
fn asset_streaming_qualification() {
    use std::time::{Duration, Instant};
    let (device, queue) = headless_device().expect("GPU adapter required for qualification");
    let project = TempProject::new("f5-streaming-bench");
    let cache = project.cook();
    project.remove_sources();
    let mut cold = Vec::new();
    let mut warm = Vec::new();
    let mut simulation = Vec::new();
    let mut cycles = Vec::new();
    let mut high_cpu = 0;
    let mut high_gpu = 0;
    let mut max_upload = 0;
    for cycle in 0..12 {
        let assets = Assets::open(&cache).unwrap();
        let (mut world, _, lease) = load_startup(&assets);
        let mut renderer = ExtractRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64);
        renderer.upload_budget.operations_per_frame = 2;
        renderer.set_assets(Some(assets.clone()));
        let loading = Instant::now();
        let deadline = loading + Duration::from_secs(10);
        let mut frames = 0;
        loop {
            let start = Instant::now();
            world.begin_frame(1.0 / 60.0);
            world.step();
            simulation.push(start.elapsed().as_secs_f64() * 1e6);
            let extract = world.extract();
            let start = Instant::now();
            renderer.prepare(&device, &queue, &extract);
            cold.push(start.elapsed().as_secs_f64() * 1e6);
            frames += 1;
            high_cpu = high_cpu.max(assets.stats().bytes);
            high_gpu = high_gpu.max(renderer.stats().gpu_bytes);
            max_upload = max_upload.max(renderer.stats().asset_upload_bytes);
            assert!(renderer.stats().asset_upload_operations <= 2);
            assert!(renderer.stats().asset_upload_bytes <= renderer.upload_budget.bytes_per_frame);
            assert!(
                renderer.asset_errors().is_empty(),
                "{:?}",
                renderer.asset_errors()
            );
            if renderer.assets_ready(&extract) && renderer.stats().asset_upload_operations == 0 {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        let loading_ms = loading.elapsed().as_secs_f64() * 1e3;
        for _ in 0..240 {
            world.begin_frame(1.0 / 60.0);
            let start = Instant::now();
            world.step();
            simulation.push(start.elapsed().as_secs_f64() * 1e6);
            let extract = world.extract();
            let start = Instant::now();
            renderer.prepare(&device, &queue, &extract);
            warm.push(start.elapsed().as_secs_f64() * 1e6);
            assert_eq!(renderer.stats().asset_upload_operations, 0);
        }
        world.unload_scene(assets.startup_scene().unwrap()).unwrap();
        world.step();
        drop(lease);
        renderer.prepare(&device, &queue, &world.extract());
        assert_eq!(assets.stats().retained, 0);
        assert_eq!(assets.stats().bytes, 0);
        cycles.push(serde_json::json!({"cycle":cycle,"cold_frames":frames,"loading_ms":loading_ms,"post_unload_cpu_bytes":assets.stats().bytes,"post_unload_gpu_bytes":renderer.stats().gpu_bytes}));
    }
    fn summary(v: &mut [f64]) -> serde_json::Value {
        v.sort_by(f64::total_cmp);
        serde_json::json!({"samples":v.len(),"p50_us":v[v.len()/2],"p95_us":v[(v.len()-1)*95/100],"p99_us":v[(v.len()-1)*99/100],"max_us":v.last()})
    }
    let result = serde_json::json!({
        "workload":"fixture Main, 6 entities, 12 independent engine-cache cold loads, 240 warm simulation/prepare frames per cycle; source files removed; OS file cache uncontrolled; release Metal 64x64",
        "draw_prepare_cold":summary(&mut cold),"draw_prepare_warm":summary(&mut warm),"simulation_step":summary(&mut simulation),
        "cpu_cache_high_water_bytes":high_cpu,"gpu_estimate_high_water_bytes":high_gpu,"max_asset_upload_bytes_per_frame":max_upload,"upload_operations_limit":2,"cycles":cycles,
        "limits":"GPU estimate includes fixed frame resources but excludes driver/pipeline allocations. CPU accounting excludes externally held payload copies and allocator overhead. Timing is observational, not a production frame-time guarantee."
    });
    if let Ok(path) = std::env::var("FYNKEL_F5_BENCH_OUT") {
        std::fs::write(path, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
    }
    println!("{result}");
}

#[test]
fn idle_readiness_finishes_healthy_assets_after_failure_and_ignores_inactive_inputs() {
    use std::time::{Duration, Instant};
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let project = TempProject::new("f5-partial-readiness");
    let assets = Assets::open(&project.cook()).unwrap();
    let (_world, mut extract, _lease) = load_startup(&assets);
    let missing = fynkel_contracts::AssetRef::new(fynkel_contracts::Uuid(*b"missing-gpu-test"));
    extract.environment = Some(fynkel_core::ExtractEnvironment {
        record: 0,
        environment: fynkel_core::Environment {
            sky: Some(missing.clone()),
            ..Default::default()
        },
    });
    let mut renderer = ExtractRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64);
    renderer.set_assets(Some(assets.clone()));
    renderer.upload_budget.operations_per_frame = 0;
    let deadline = Instant::now() + Duration::from_secs(5);
    while renderer.asset_errors().is_empty() {
        renderer.prepare(&device, &queue, &extract);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        renderer.assets_pending(),
        "a bad sky must not strand the healthy meshes"
    );
    renderer.upload_budget.operations_per_frame = 16;
    while renderer.assets_pending() {
        renderer.prepare(&device, &queue, &extract);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(renderer.stats().meshes > 0);
    assert!(
        !renderer.assets_ready(&extract),
        "a terminal fallback is not successful readiness"
    );

    assets
        .set_runtime_material(
            &fynkel_contracts::AssetRef::new(fynkel_contracts::Uuid(*b"unrelated-mat-01")),
            Some(fynkel_assets::PbrMaterial::default()),
        )
        .unwrap();
    renderer.prepare(&device, &queue, &extract);
    assert!(
        !renderer.assets_ready(&extract),
        "an unrelated live edit cannot hide the sky failure"
    );

    // Unloading an unrelated asset must not erase the still-live sky failure.
    let unrelated = fynkel_contracts::Uuid(*b"unrelated-asset1");
    drop(assets.retain(&[unrelated]));
    renderer.prepare(&device, &queue, &extract);
    assert!(!renderer.assets_ready(&extract));
    assert!(!renderer.asset_errors().is_empty());
    assert!(renderer
        .prepare_ready(&device, &queue, &extract, Duration::from_secs(5))
        .is_err());
    // Another owner may retain the failed asset, but it is no longer required
    // by this view once its sky is removed.
    let _other_owner = assets.retain(&[missing.asset]);
    // Removing the actual failed reference must allow readiness to recover.
    extract.environment = None;
    renderer
        .prepare_ready(&device, &queue, &extract, Duration::from_secs(5))
        .unwrap();
    assert!(renderer.assets_ready(&extract));

    let mut renderer = ExtractRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64);
    renderer.set_assets(Some(assets));
    extract.environment = None;
    extract.camera.as_mut().unwrap().camera.culling_mask = 1;
    let record = extract.meshes[0].record as usize;
    extract.records[record].layer_bit = 2;
    extract.meshes[0].mesh = missing.clone();
    extract.volumes.push(fynkel_core::ExtractVolume {
        record: 0,
        world: glam::Mat4::IDENTITY,
        volume: fynkel_core::Volume {
            profile: missing,
            weight: 0.0,
            ..Default::default()
        },
    });
    renderer
        .prepare_ready(&device, &queue, &extract, Duration::from_secs(5))
        .unwrap();
    assert!(renderer.assets_ready(&extract));
    assert!(
        !renderer.assets_pending(),
        "hidden layers and zero-weight volumes need no work"
    );
}

#[test]
fn explicit_sky_without_an_asset_store_is_not_gpu_ready() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let extract = fynkel_core::Extract {
        environment: Some(fynkel_core::ExtractEnvironment {
            record: 0,
            environment: fynkel_core::Environment {
                sky: Some(fynkel_contracts::AssetRef::new(fynkel_contracts::Uuid(
                    *b"missing-gpu-test",
                ))),
                ..Default::default()
            },
        }),
        ..Default::default()
    };
    let mut renderer = ExtractRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64);
    let error = renderer
        .prepare_ready(&device, &queue, &extract, std::time::Duration::from_secs(1))
        .unwrap_err();
    assert!(error.contains("no asset source"));
    assert!(!renderer.assets_ready(&extract));
}
