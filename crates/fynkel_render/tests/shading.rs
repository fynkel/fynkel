//! Renderer V1 (a) feature gate (blueprint §19.2d item 4): one pixel test
//! per feature. Hand-built extracts (the renderer reads nothing else) for
//! the camera, exposure, lights and environment; a project cooked on the
//! fly for everything that needs a material.

use std::path::{Path, PathBuf};

use fynkel_contracts::{AssetRef, Entity, Uuid};
use fynkel_core::{
    Camera, DirectionalLight, Environment, ExposureMode, Extract, ExtractCamera,
    ExtractDirectionalLight, ExtractEnvironment, ExtractPointLight, ExtractRecord,
    ExtractSpotLight, PointLight, Projection, SpotLight, World, WorldConfig,
};
use fynkel_import::{cook_project, CookOptions};
use fynkel_render::{
    clear_rgba8, exposure, headless_device, render_offscreen, Assets, ExtractRenderer, MAX_LIGHTS,
    PIPELINE_VARIANTS,
};
use glam::{Mat4, Quat, Vec3};
use serde_json::{json, Value};

const W: u32 = 128;
const H: u32 = 128;

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let d = headless_device();
    if d.is_none() {
        eprintln!("no GPU adapter; skipping");
    }
    d
}

fn renderer(device: &wgpu::Device) -> ExtractRenderer {
    let mut r = ExtractRenderer::new(device, wgpu::TextureFormat::Rgba8Unorm, W, H);
    r.placeholders = true;
    r
}

fn record(slot: u32, world: Mat4) -> ExtractRecord {
    ExtractRecord {
        entity: Entity::from_bits(slot as u64 + 1),
        world,
        overridden: false,
    }
}

/// A camera at `eye` looking down -Z, and a placeholder cube at the origin.
fn cube_scene(eye: Vec3, camera: Camera) -> Extract {
    let mut e = Extract::default();
    e.records.push(record(0, Mat4::from_translation(eye)));
    e.records.push(record(1, Mat4::IDENTITY));
    e.camera = Some(ExtractCamera {
        record: 0,
        world: Mat4::from_translation(eye),
        camera,
    });
    // The camera entity's own placeholder cube would sit on the lens; the
    // renderer draws it, but it is behind the near plane.
    e
}

fn sun(direction: Vec3, lux: f32) -> ExtractDirectionalLight {
    ExtractDirectionalLight {
        record: 0,
        direction: direction.normalize(),
        light: DirectionalLight {
            color: Vec3::ONE,
            intensity: lux,
            cast_shadows: false,
        },
    }
}

fn environment(ambient_color: Vec3, ambient_lux: f32) -> ExtractEnvironment {
    ExtractEnvironment {
        record: 0,
        environment: Environment {
            ambient_color,
            ambient_intensity: ambient_lux,
            sky: None,
            sky_intensity: 0.0,
            sky_rotation: 0.0,
        },
    }
}

fn is_background(p: [u8; 4]) -> bool {
    p == clear_rgba8()
}

/// Mean brightness of the drawn (non-background) pixels, and their count.
fn drawn_mean(px: &[[u8; 4]]) -> (f32, usize) {
    let drawn: Vec<&[u8; 4]> = px.iter().filter(|p| !is_background(**p)).collect();
    if drawn.is_empty() {
        return (0.0, 0);
    }
    let sum: f32 = drawn
        .iter()
        .map(|p| (p[0] as f32 + p[1] as f32 + p[2] as f32) / 3.0)
        .sum();
    (sum / drawn.len() as f32, drawn.len())
}

fn channel_mean(px: &[[u8; 4]], c: usize) -> f32 {
    let drawn: Vec<&[u8; 4]> = px.iter().filter(|p| !is_background(**p)).collect();
    drawn.iter().map(|p| p[c] as f32).sum::<f32>() / drawn.len().max(1) as f32
}

fn dump(px: &[[u8; 4]], name: &str) {
    if let Ok(dir) = std::env::var("FYNKEL_RENDER_DUMP_DIR") {
        let mut ppm = format!("P6 {W} {H} 255\n").into_bytes();
        for p in px {
            ppm.extend_from_slice(&p[..3]);
        }
        std::fs::write(Path::new(&dir).join(format!("{name}.ppm")), ppm).unwrap();
    }
}

// ------------------------------------------------------------ exposure

#[test]
fn manual_exposure_is_the_cameras_and_auto_meters_the_sun() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut r = renderer(&device);
    let mut at = |ev: Option<f32>| {
        let camera = match ev {
            Some(exposure) => Camera {
                exposure_mode: ExposureMode::Manual,
                exposure,
                ..Camera::default()
            },
            None => Camera::default(),
        };
        let mut e = cube_scene(Vec3::new(0.0, 1.5, 4.0), camera);
        e.directional_lights
            .push(sun(Vec3::new(-0.3, -1.0, -0.5), 10_000.0));
        let px = render_offscreen(&device, &queue, &mut r, &e);
        (r.ev100(), drawn_mean(&px).0)
    };
    let (ev_auto, auto) = at(None);
    assert!((ev_auto - exposure::ev100_from_lux(10_000.0)).abs() < 1e-5);
    let (ev12, twelve) = at(Some(12.0));
    assert_eq!(ev12, 12.0);
    let (_, eight) = at(Some(8.0));
    let (_, sixteen) = at(Some(16.0));
    assert!(
        eight > twelve && twelve > sixteen,
        "EV8 {eight} > EV12 {twelve} > EV16 {sixteen}"
    );
    assert!((auto - twelve).abs() < 4.0, "auto {auto} ≈ EV12 {twelve}");
    assert!(sixteen > 0.0, "16 stops down is dark, not clipped away");
    // No scene light: the host's fallback sun meters like 10 000 lux.
    let e = cube_scene(Vec3::new(0.0, 1.5, 4.0), Camera::default());
    render_offscreen(&device, &queue, &mut r, &e);
    assert!((r.ev100() - exposure::ev100_from_lux(exposure::FALLBACK_SUN_LUX)).abs() < 1e-5);
    assert!(!r.stats().scene_light);
}

// ------------------------------------------------------------ lights

/// A dark scene: no sun, no ambient, a placeholder cube in front of the
/// camera at (0, 0, 4).
fn dark_scene() -> Extract {
    let mut e = cube_scene(Vec3::new(0.0, 0.0, 4.0), Camera::default());
    e.environment = Some(environment(Vec3::ONE, 0.0));
    e
}

/// Without a sun the meter falls back to the ambient, then to 1 lux, so a
/// dark scene is exposed very high; a manual camera keeps the test honest.
fn manual(e: &mut Extract, ev: f32) {
    let c = e.camera.as_mut().unwrap();
    c.camera.exposure_mode = ExposureMode::Manual;
    c.camera.exposure = ev;
}

#[test]
fn no_light_is_black_and_a_point_light_lights_its_side() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut r = renderer(&device);
    let mut e = dark_scene();
    manual(&mut e, 4.0);
    let px = render_offscreen(&device, &queue, &mut r, &e);
    let (black, n) = drawn_mean(&px);
    assert!(n > 500, "the cube is drawn ({n} px)");
    assert!(black < 1.0, "no light, no ambient: black ({black})");
    assert!(!r.stats().scene_light);

    // 300 cd at 2 m off to +X: E = 75 lux on the +X face.
    let point = |position: Vec3, intensity: f32, range: f32| ExtractPointLight {
        record: 0,
        position,
        light: PointLight {
            color: Vec3::ONE,
            intensity,
            range,
        },
    };
    e.point_lights
        .push(point(Vec3::new(2.5, 0.5, 2.5), 300.0, 20.0));
    let px = render_offscreen(&device, &queue, &mut r, &e);
    dump(&px, "point");
    let (lit, _) = drawn_mean(&px);
    assert!(lit > 20.0, "lit by the point light ({lit})");
    assert_eq!(r.stats().lights, 1);
    // Right half (toward the light) brighter than the left half.
    let half = |px: &[[u8; 4]], left: bool| {
        let mut sum = 0.0;
        let mut n = 0;
        for y in 0..H {
            for x in 0..W {
                let p = px[(y * W + x) as usize];
                if is_background(p) || (x < W / 2) != left {
                    continue;
                }
                sum += p[0] as f32;
                n += 1;
            }
        }
        sum / n.max(1) as f32
    };
    assert!(half(&px, false) > half(&px, true) + 10.0);

    // Beyond its range the light contributes nothing.
    e.point_lights[0].light.range = 1.0;
    let px = render_offscreen(&device, &queue, &mut r, &e);
    assert!(drawn_mean(&px).0 < 1.0, "windowed to zero past range");

    // The budget: the nearest MAX_LIGHTS are kept, the rest counted.
    e.point_lights.clear();
    for i in 0..20 {
        e.point_lights
            .push(point(Vec3::new(i as f32, 3.0, 0.0), 50.0, 30.0));
    }
    render_offscreen(&device, &queue, &mut r, &e);
    let s = r.stats();
    assert_eq!(
        (s.lights, s.lights_dropped),
        (MAX_LIGHTS as u32, 20 - MAX_LIGHTS as u32)
    );
}

#[test]
fn spot_light_lights_only_inside_its_cone() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut r = renderer(&device);
    let mut e = dark_scene();
    manual(&mut e, 4.0);
    let spot = |direction: Vec3| ExtractSpotLight {
        record: 0,
        position: Vec3::new(0.0, 0.0, 3.0),
        direction: direction.normalize(),
        light: SpotLight {
            color: Vec3::ONE,
            intensity: 500.0,
            range: 20.0,
            inner_angle: 15f32.to_radians(),
            outer_angle: 25f32.to_radians(),
        },
    };
    // Aimed at the cube (down -Z from in front of it).
    e.spot_lights.push(spot(Vec3::NEG_Z));
    let px = render_offscreen(&device, &queue, &mut r, &e);
    dump(&px, "spot");
    let (lit, _) = drawn_mean(&px);
    assert!(lit > 20.0, "in the cone ({lit})");
    assert_eq!(r.stats().lights, 1);
    // Aimed sideways: the cube is outside the cone.
    e.spot_lights[0] = spot(Vec3::X);
    let px = render_offscreen(&device, &queue, &mut r, &e);
    assert!(drawn_mean(&px).0 < 1.0, "outside the cone");
}

// ------------------------------------------------------------ environment

#[test]
fn ambient_lights_the_scene_in_its_colour() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut r = renderer(&device);
    let mut e = dark_scene();
    manual(&mut e, 8.0);
    e.environment = Some(environment(Vec3::new(1.0, 0.2, 0.2), 1000.0));
    let px = render_offscreen(&device, &queue, &mut r, &e);
    dump(&px, "ambient");
    let (lit, _) = drawn_mean(&px);
    assert!(lit > 10.0, "ambient alone lights it ({lit})");
    assert!(
        channel_mean(&px, 0) > channel_mean(&px, 2) + 20.0,
        "reddish"
    );
    // A scene without an Environment gets the component's defaults
    // (2000 lux white), not darkness.
    e.environment = None;
    let px = render_offscreen(&device, &queue, &mut r, &e);
    let (default_lit, _) = drawn_mean(&px);
    assert!(default_lit > lit, "{default_lit} > {lit}");
}

// ------------------------------------------------------------ camera

#[test]
fn orthographic_camera_has_no_perspective() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut r = renderer(&device);
    // ortho_height 4 at 128 px: the unit cube is 32 px tall whatever its
    // distance, and its front face's vertical edges are parallel.
    let camera = Camera {
        projection: Projection::Orthographic,
        ortho_height: 4.0,
        ..Camera::default()
    };
    let mut e = cube_scene(Vec3::new(0.0, 0.0, 6.0), camera);
    e.directional_lights
        .push(sun(Vec3::new(0.0, -0.5, -1.0), 10_000.0));
    let extent = |px: &[[u8; 4]]| {
        let col: Vec<u32> = (0..H)
            .filter(|y| !is_background(px[(y * W + W / 2) as usize]))
            .collect();
        (col.first().copied(), col.len())
    };
    let px = render_offscreen(&device, &queue, &mut r, &e);
    dump(&px, "ortho");
    let (_, tall) = extent(&px);
    assert!((31..=33).contains(&tall), "{tall} px tall");
    e.records[1].world = Mat4::from_translation(Vec3::new(0.0, 0.0, -20.0));
    let px = render_offscreen(&device, &queue, &mut r, &e);
    let (_, far) = extent(&px);
    assert_eq!(far, tall, "no foreshortening");
    // The same scene through a perspective camera shrinks with distance.
    e.camera.as_mut().unwrap().camera.projection = Projection::Perspective;
    e.records[1].world = Mat4::IDENTITY;
    let near = extent(&render_offscreen(&device, &queue, &mut r, &e)).1;
    e.records[1].world = Mat4::from_translation(Vec3::new(0.0, 0.0, -20.0));
    let far = extent(&render_offscreen(&device, &queue, &mut r, &e)).1;
    assert!(far < near / 2, "{far} < {near}/2");
}

#[test]
fn a_fixed_set_of_pipelines() {
    assert_eq!(PIPELINE_VARIANTS, 4);
}

// ------------------------------------------------------------ materials

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

const ROBOT_MESH: &str = "c0000000-0000-4000-8000-000000000001";
const SWATCH: &str = "c0000000-0000-4000-8000-000000000002";
const NORMAL: &str = "c0000000-0000-4000-8000-000000000003";
const MR: &str = "c0000000-0000-4000-8000-000000000004";

/// A copy of the fixture project whose scene the test writes: a camera at
/// `eye`, one robot cube per material (spread along +X), an optional sun.
struct Lab {
    dir: PathBuf,
    materials: Vec<(Uuid, Value)>,
}

impl Lab {
    fn new(name: &str) -> Lab {
        let dir =
            std::env::temp_dir().join(format!("fynkel-shading-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        copy_dir(&fixture_project(), &dir);
        Lab {
            dir,
            materials: Vec::new(),
        }
    }

    /// Adds a `fynkel.pbr@1` material document with `fields` on top of
    /// the defaults; returns its id.
    fn material(&mut self, fields: Value) -> Uuid {
        let n = self.materials.len() + 1;
        let id: Uuid = format!("c0000000-0000-4000-8000-0000000000a{n:x}")
            .parse()
            .unwrap();
        let mut doc = json!({
            "$schema": "https://fynkel.dev/schemas/material.schema.json",
            "formatVersion": 2,
            "id": id.to_string(),
            "name": format!("Lab {n}"),
            "shader": "fynkel.pbr@1",
        });
        for (k, v) in fields.as_object().unwrap() {
            doc[k] = v.clone();
        }
        std::fs::write(
            self.dir
                .join(format!("assets/materials/lab{n}.material.json")),
            serde_json::to_string_pretty(&doc).unwrap(),
        )
        .unwrap();
        self.materials.push((id, doc));
        id
    }

    /// Writes the scene, cooks, loads and extracts it.
    fn extract(&self, eye: Vec3, cubes: &[(Vec3, Uuid)], sun: bool) -> (Assets, Extract) {
        let mut entities = serde_json::Map::new();
        let transform = |p: Vec3, q: Quat| {
            json!({ "type": "fynkel.Transform", "version": 1, "data": {
                "position": [p.x, p.y, p.z], "rotation": [q.x, q.y, q.z, q.w], "scale": [1, 1, 1] } })
        };
        entities.insert(
            "c0000000-0000-4000-8000-0000000000e0".into(),
            json!({ "name": "Camera", "parent": null, "children": [], "components": {
                "c0000000-0000-4000-8000-0000000000c0": transform(eye, Quat::IDENTITY),
                "c0000000-0000-4000-8000-0000000000c9": { "type": "fynkel.Camera", "version": 1, "data": {
                    "projection": "perspective", "fov_y": 1.0, "ortho_height": 10.0, "near": 0.1, "far": 100.0,
                    "priority": 0, "exposure_mode": "manual", "exposure": 12.0 } } } }),
        );
        if sun {
            // From in front (the camera's side), a little above and to
            // the right (+X): the mirror direction of the +Z face is near
            // the camera, so the highlight is in view.
            let q = Quat::from_rotation_y(-20f32.to_radians())
                * Quat::from_rotation_x(-15f32.to_radians());
            entities.insert(
                "c0000000-0000-4000-8000-0000000000e3".into(),
                json!({ "name": "Sun", "parent": null, "children": [], "components": {
                    "c0000000-0000-4000-8000-0000000000c5": transform(Vec3::new(0.0, 10.0, 0.0), q),
                    "c0000000-0000-4000-8000-0000000000c6": { "type": "fynkel.DirectionalLight", "version": 1, "data": {
                        "color": [1, 1, 1], "intensity": 10000.0, "cast_shadows": false } } } }),
            );
        }
        for (i, (position, material)) in cubes.iter().enumerate() {
            entities.insert(
                format!("c0000000-0000-4000-8000-0000000000f{i:x}"),
                json!({ "name": format!("Cube {i}"), "parent": null, "children": [], "components": {
                    format!("c0000000-0000-4000-8000-0000000000d{i:x}"): transform(*position, Quat::IDENTITY),
                    format!("c0000000-0000-4000-8000-0000000000b{i:x}"): { "type": "fynkel.MeshRenderer", "version": 1, "data": {
                        "mesh": { "asset": ROBOT_MESH, "sub": "mesh/0" },
                        "material": { "asset": material.to_string() },
                        "cast_shadows": true, "receive_shadows": true, "visible": true } } } }),
            );
        }
        let scene = json!({
            "$schema": "https://fynkel.dev/schemas/scene.schema.json",
            "formatVersion": 2,
            "id": "c0000000-0000-4000-8000-000000000030",
            "name": "Lab",
            "entities": entities,
        });
        std::fs::write(
            self.dir.join("assets/scenes/main.scene.json"),
            serde_json::to_string_pretty(&scene).unwrap(),
        )
        .unwrap();
        let report = cook_project(&self.dir, None, &CookOptions::default())
            .unwrap_or_else(|d| panic!("{d:#?}"));
        assert!(!report.has_errors(), "{}", report.text());
        let assets =
            Assets::open(&self.dir.join(".fynkel/cache")).unwrap_or_else(|d| panic!("{d}"));
        let id = assets.startup_scene().unwrap();
        let cooked = assets.scene(id).unwrap_or_else(|d| panic!("{d:#?}"));
        assert_eq!(cooked.diagnostics, vec![]);
        let mut world = World::new(WorldConfig::default());
        world.load_scene(&cooked.description).unwrap();
        // The closure stays retained for the store's lifetime here.
        std::mem::forget(assets.retain(&cooked.closure));
        world.begin_frame(1.0 / 60.0);
        world.step();
        (assets, world.extract())
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn render_lab(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    assets: &Assets,
    extract: &Extract,
) -> (Vec<[u8; 4]>, ExtractRenderer) {
    let mut r = renderer(device);
    r.placeholders = false;
    r.set_assets(Some(assets.clone()));
    let px = render_offscreen(device, queue, &mut r, extract);
    assert_eq!(r.asset_errors(), &[] as &[String]);
    (px, r)
}

fn asset(id: &str) -> Value {
    json!({ "asset": id })
}

#[test]
fn normal_and_roughness_maps_change_the_shading() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut lab = Lab::new("maps");
    // The fixture's rusted maps (normal bands tilted ±30° about Y, a
    // roughness ramp) on a plain white base, against the same factors
    // without the normal map, and without either map.
    let both = lab.material(json!({
        "metallic": 0.25, "roughness": 0.65,
        "normal_texture": asset(NORMAL), "metallic_roughness_texture": asset(MR) }));
    let no_normal = lab.material(json!({
        "metallic": 0.25, "roughness": 0.65, "metallic_roughness_texture": asset(MR) }));
    let flat = lab.material(json!({ "metallic": 0.25, "roughness": 0.65 }));
    let eye = Vec3::new(0.0, 0.3, 3.0);
    let render = |m: Uuid| {
        let (assets, extract) = lab.extract(eye, &[(Vec3::ZERO, m)], true);
        assert_eq!(extract.meshes.len(), 1);
        render_lab(&device, &queue, &assets, &extract).0
    };
    let with_maps = render(both);
    let flat_px = render(flat);
    let no_normal_px = render(no_normal);
    dump(&with_maps, "maps");
    dump(&flat_px, "flat");
    let differ = |a: &[[u8; 4]], b: &[[u8; 4]]| {
        a.iter()
            .zip(b)
            .filter(|(p, q)| (0..3).any(|c| (p[c] as i32 - q[c] as i32).abs() > 8))
            .count()
    };
    let n = drawn_mean(&with_maps).1;
    assert!(n > 500);
    // The normal map's tilted bands shade every other column differently.
    let d = differ(&with_maps, &no_normal_px);
    assert!(d > n / 8, "normal map changes {d} of {n} drawn pixels");
    // The roughness ramp changes the highlight across the face.
    let d = differ(&no_normal_px, &flat_px);
    assert!(d > 0, "roughness map changes the shading");
}

#[test]
fn alpha_mask_discards_texels_below_the_cutoff() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut lab = Lab::new("mask");
    // The swatch's bottom-right texel is transparent (alpha 0).
    let opaque = lab.material(json!({ "base_color_texture": asset(SWATCH) }));
    let masked = lab.material(json!({
        "base_color_texture": asset(SWATCH), "alpha_mode": "mask", "alpha_cutoff": 0.5 }));
    let eye = Vec3::new(0.0, 0.0, 2.0);
    let count = |m: Uuid| {
        let (assets, extract) = lab.extract(eye, &[(Vec3::ZERO, m)], true);
        let (px, r) = render_lab(&device, &queue, &assets, &extract);
        assert_eq!(r.stats().meshes, 1);
        dump(&px, if m == masked { "masked" } else { "opaque" });
        drawn_mean(&px).1
    };
    let full = count(opaque);
    let holed = count(masked);
    assert!(full > 2000, "{full}");
    assert!(
        holed < full - full / 40 && holed > full / 2,
        "one of twelve texels is cut from each face: {holed} of {full} px"
    );
}

#[test]
fn double_sided_materials_draw_their_back_faces() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut lab = Lab::new("double");
    let single = lab.material(json!({ "base_color": [0.2, 0.9, 0.2, 1.0] }));
    let double = lab.material(json!({ "base_color": [0.2, 0.9, 0.2, 1.0], "double_sided": true }));
    // The camera inside the cube sees only back faces.
    let eye = Vec3::ZERO;
    let render = |m: Uuid| {
        let (assets, extract) = lab.extract(eye, &[(Vec3::ZERO, m)], false);
        let (px, r) = render_lab(&device, &queue, &assets, &extract);
        (drawn_mean(&px), r.stats())
    };
    let ((_, culled), _) = render(single);
    assert_eq!(culled, 0, "back faces are culled");
    let ((lit, drawn), stats) = render(double);
    assert_eq!(
        drawn,
        (W * H) as usize,
        "the inside of the cube fills the view"
    );
    assert_eq!(stats.batches, 1);
    // The flipped normal faces the camera: the default ambient lights it.
    assert!(lit > 10.0, "{lit}");
}

#[test]
fn emissive_glows_without_light() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut lab = Lab::new("emissive");
    let dark = lab.material(json!({ "base_color": [1.0, 1.0, 1.0, 1.0] }));
    let glow = lab.material(json!({
        "base_color": [1.0, 1.0, 1.0, 1.0], "emissive": [1.0, 0.0, 0.0], "emissive_strength": 1.0 }));
    let eye = Vec3::new(0.0, 0.0, 3.0);
    let render = |m: Uuid| {
        let (assets, mut extract) = lab.extract(eye, &[(Vec3::ZERO, m)], false);
        extract.environment = Some(environment(Vec3::ONE, 0.0));
        let (px, _) = render_lab(&device, &queue, &assets, &extract);
        px
    };
    let px = render(dark);
    assert!(drawn_mean(&px).0 < 1.0, "unlit and non-emissive: black");
    let px = render(glow);
    dump(&px, "emissive");
    let red = channel_mean(&px, 0);
    assert!(red > 150.0 && channel_mean(&px, 1) < 40.0, "red {red}");
}

#[test]
fn sky_texture_fills_the_background() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut lab = Lab::new("sky");
    let m = lab.material(json!({}));
    let eye = Vec3::new(0.0, 0.0, 3.0);
    let (assets, mut extract) = lab.extract(eye, &[(Vec3::ZERO, m)], true);
    let corner = |px: &[[u8; 4]]| px[(2 * W + 2) as usize];
    let (px, r) = render_lab(&device, &queue, &assets, &extract);
    assert!(!r.stats().sky);
    assert!(is_background(corner(&px)));
    extract.environment = Some(ExtractEnvironment {
        record: 0,
        environment: Environment {
            sky: Some(AssetRef::new(SWATCH.parse().unwrap())),
            sky_intensity: 5000.0,
            ..Environment::default()
        },
    });
    let (px, r) = render_lab(&device, &queue, &assets, &extract);
    dump(&px, "sky");
    assert!(r.stats().sky);
    assert!(!is_background(corner(&px)), "the sky is behind everything");
    // The cube is still in front of it (the centre is not sky-coloured
    // like the corner).
    let centre = px[(H / 2 * W + W / 2) as usize];
    assert_ne!(centre, corner(&px));
    // Rotating the sky half a turn changes what the corner sees.
    extract
        .environment
        .as_mut()
        .unwrap()
        .environment
        .sky_rotation = std::f32::consts::PI;
    let (rotated, _) = render_lab(&device, &queue, &assets, &extract);
    assert_ne!(corner(&rotated), corner(&px));
    // Intensity 0 leaves a black (not clear-coloured) backdrop.
    extract
        .environment
        .as_mut()
        .unwrap()
        .environment
        .sky_intensity = 0.0;
    let (black, _) = render_lab(&device, &queue, &assets, &extract);
    assert_eq!(corner(&black)[..3], [0, 0, 0]);
}
