//! Renderer V1 (a), (b) and (c) feature gate (blueprint §19.2d items 4–6):
//! one pixel test per feature. Hand-built extracts (the renderer reads
//! nothing else) for the camera, exposure, lights, environment, shadows,
//! MSAA, render scale, culling and debug lines; a project cooked on the
//! fly for everything that needs a material or a quality profile.

use std::path::{Path, PathBuf};

use fynkel_contracts::{AssetRef, Entity, Uuid};
use fynkel_core::{
    Camera, DirectionalLight, Environment, ExposureMode, Extract, ExtractCamera,
    ExtractDirectionalLight, ExtractEnvironment, ExtractPointLight, ExtractRecord,
    ExtractSpotLight, PointLight, Projection, SpotLight, World, WorldConfig,
};
use fynkel_import::{cook_project, CookOptions};
use fynkel_render::{
    clear_rgba8, exposure, headless_device, render_offscreen, Assets, ExtractRenderer, Quality,
    MAX_LIGHTS, PIPELINE_VARIANTS,
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
    assert_eq!(PIPELINE_VARIANTS, 14);
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
const SKY_HDR: &str = "c0000000-0000-4000-8000-000000000007";

/// A copy of the fixture project whose scene the test writes: a camera at
/// `eye`, one robot cube per material (spread along +X), an optional sun.
struct Lab {
    dir: PathBuf,
    materials: Vec<(Uuid, Value)>,
    /// The scene's sun casts shadows.
    sun_shadows: bool,
}

/// One `MeshRenderer` entity in the lab scene.
#[derive(Clone, Copy)]
struct LabCube {
    position: Vec3,
    scale: Vec3,
    material: Uuid,
    cast_shadows: bool,
    receive_shadows: bool,
}

impl LabCube {
    fn new(position: Vec3, material: Uuid) -> LabCube {
        LabCube {
            position,
            scale: Vec3::ONE,
            material,
            cast_shadows: true,
            receive_shadows: true,
        }
    }
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
            sun_shadows: false,
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

    /// Copies the importer's HDR fixture into the project as a sky (the
    /// cook writes its ENVM chunk): above 45° the blue sky (0.8, 1.6,
    /// 4.0), below it red toward -X and green toward +X.
    fn sky_hdr(&self) -> AssetRef {
        let from = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fynkel_import/fixtures/textures/sky.hdr");
        std::fs::copy(from, self.dir.join("assets/textures/sky.hdr")).unwrap();
        let meta = json!({
            "$schema": "https://fynkel.dev/schemas/meta.schema.json",
            "formatVersion": 1,
            "id": SKY_HDR,
            "name": "sky.hdr",
            "importer": "fynkel.image",
            "importerVersion": 1,
            "settings": {},
            "subAssets": {},
        });
        std::fs::write(
            self.dir.join("assets/textures/sky.hdr.meta.json"),
            serde_json::to_string_pretty(&meta).unwrap(),
        )
        .unwrap();
        AssetRef::new(SKY_HDR.parse().unwrap())
    }

    /// Writes the scene, cooks, loads and extracts it.
    fn extract(&self, eye: Vec3, cubes: &[(Vec3, Uuid)], sun: bool) -> (Assets, Extract) {
        let cubes: Vec<LabCube> = cubes.iter().map(|(p, m)| LabCube::new(*p, *m)).collect();
        self.extract_cubes(eye, &cubes, sun)
    }

    fn extract_cubes(&self, eye: Vec3, cubes: &[LabCube], sun: bool) -> (Assets, Extract) {
        let mut entities = serde_json::Map::new();
        let transform = |p: Vec3, q: Quat, s: Vec3| {
            json!({ "type": "fynkel.Transform", "version": 1, "data": {
                "position": [p.x, p.y, p.z], "rotation": [q.x, q.y, q.z, q.w], "scale": [s.x, s.y, s.z] } })
        };
        entities.insert(
            "c0000000-0000-4000-8000-0000000000e0".into(),
            json!({ "name": "Camera", "parent": null, "children": [], "components": {
                "c0000000-0000-4000-8000-0000000000c0": transform(eye, Quat::IDENTITY, Vec3::ONE),
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
                    "c0000000-0000-4000-8000-0000000000c5": transform(Vec3::new(0.0, 10.0, 0.0), q, Vec3::ONE),
                    "c0000000-0000-4000-8000-0000000000c6": { "type": "fynkel.DirectionalLight", "version": 1, "data": {
                        "color": [1, 1, 1], "intensity": 10000.0, "cast_shadows": self.sun_shadows } } } }),
            );
        }
        for (i, cube) in cubes.iter().enumerate() {
            entities.insert(
                format!("c0000000-0000-4000-8000-0000000000f{i:x}"),
                json!({ "name": format!("Cube {i}"), "parent": null, "children": [], "components": {
                    format!("c0000000-0000-4000-8000-0000000000d{i:x}"): transform(cube.position, Quat::IDENTITY, cube.scale),
                    format!("c0000000-0000-4000-8000-0000000000b{i:x}"): { "type": "fynkel.MeshRenderer", "version": 1, "data": {
                        "mesh": { "asset": ROBOT_MESH, "sub": "mesh/0" },
                        "material": { "asset": cube.material.to_string() },
                        "cast_shadows": cube.cast_shadows, "receive_shadows": cube.receive_shadows, "visible": true } } } }),
            );
        }
        let scene = json!({
            "$schema": "https://fynkel.dev/schemas/scene.schema.json",
            "formatVersion": 3,
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

#[test]
fn an_hdr_sky_lights_the_scene_through_its_environment_cubes() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut lab = Lab::new("ibl");
    let sky = lab.sky_hdr();
    // A white dielectric with a little gloss: what it shows is the
    // environment, through the irradiance cube and the prefiltered one.
    let m = lab.material(json!({ "base_color": [1.0, 1.0, 1.0, 1.0], "roughness": 0.5 }));
    let eye = Vec3::new(0.0, 0.0, 3.0);
    let (assets, mut extract) = lab.extract(eye, &[(Vec3::ZERO, m)], false);
    // The cube fills the middle of the frame; the sky is behind it.
    let centre = |px: &[[u8; 4]]| px[(H / 2 * W + W / 2) as usize];

    // No sky and no ambient: nothing lights the cube at all.
    extract.environment = Some(environment(Vec3::ONE, 0.0));
    let (unlit, r) = render_lab(&device, &queue, &assets, &extract);
    assert!(!r.stats().ibl);
    assert!(luminance(centre(&unlit)) < 8.0, "nothing lights it yet");

    let mut render = |asset: &AssetRef, rotation: f32| {
        extract.environment = Some(ExtractEnvironment {
            record: 0,
            environment: Environment {
                ambient_intensity: 0.0,
                sky: Some(asset.clone()),
                sky_intensity: 1500.0,
                sky_rotation: rotation,
                ..Environment::default()
            },
        });
        render_lab(&device, &queue, &assets, &extract)
    };

    // A cooked PNG is a sky but not an environment: the constant-ambient
    // path stays.
    let swatch = AssetRef::new(SWATCH.parse().unwrap());
    let (px, r) = render(&swatch, 0.0);
    assert!(
        r.stats().sky && !r.stats().ibl,
        "a PNG carries no ENVM chunk"
    );
    assert!(luminance(centre(&px)) < 8.0);

    // The HDR one lights it.
    let (lit, r) = render(&sky, 0.0);
    dump(&lit, "ibl");
    assert!(r.stats().ibl);
    let lit_luma = luminance(centre(&lit));
    assert!(lit_luma > 40.0, "lit by the environment alone: {lit_luma}");

    // The sky rotation turns the lookup with it: a quarter turn puts the
    // ground's green half where the front face looks, the other way its
    // red half. (`rotate_sky` maps +Z to +X at +90°.)
    let (green, _) = render(&sky, std::f32::consts::FRAC_PI_2);
    let (red, _) = render(&sky, -std::f32::consts::FRAC_PI_2);
    dump(&green, "ibl-green");
    dump(&red, "ibl-red");
    let g = centre(&green);
    let r = centre(&red);
    assert!(g[1] > g[0] + 20 && g[1] > g[2], "+90°: green {g:?}");
    assert!(r[0] > r[1] + 20 && r[0] > r[2], "-90°: red {r:?}");
}

// ------------------------------------------------- renderer V1 (b): shadows

/// The pixel `world` lands on through the renderer's fallback camera.
fn pixel_of(r: &ExtractRenderer, world: Vec3) -> (u32, u32) {
    let ndc = r.camera.view_proj(W, H).project_point3(world);
    (
        ((ndc.x * 0.5 + 0.5) * W as f32) as u32,
        ((0.5 - ndc.y * 0.5) * H as f32) as u32,
    )
}

fn luminance(p: [u8; 4]) -> f32 {
    (p[0] as f32 + p[1] as f32 + p[2] as f32) / 3.0
}

#[test]
fn the_sun_casts_a_pcf_shadow_that_the_quality_profile_can_turn_off() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut r = renderer(&device);
    r.camera.eye = Vec3::new(0.0, 8.0, 6.0);
    // A flat placeholder slab (top at y = -0.5) under a unit cube; the sun
    // from above and -X, so the cube's shadow falls on the slab's +X side
    // (x in 0.5..1.1 at the slab's surface).
    let mut e = Extract::default();
    e.records.push(record(
        0,
        Mat4::from_scale_rotation_translation(
            Vec3::new(10.0, 0.2, 10.0),
            Quat::IDENTITY,
            Vec3::new(0.0, -0.6, 0.0),
        ),
    ));
    e.records.push(record(1, Mat4::IDENTITY));
    let mut sun = sun(Vec3::new(0.6, -1.0, 0.0), 10_000.0);
    sun.light.cast_shadows = true;
    e.directional_lights.push(sun);
    let shadowed_at = pixel_of(&r, Vec3::new(0.8, -0.5, 0.0));
    let lit_at = pixel_of(&r, Vec3::new(-2.0, -0.5, 0.0));
    let sample = |px: &[[u8; 4]], (x, y): (u32, u32)| px[(y * W + x) as usize];

    let px = render_offscreen(&device, &queue, &mut r, &e);
    dump(&px, "shadow");
    let stats = r.stats();
    assert_eq!((stats.shadow_map, stats.shadow_casters), (2048, 2));
    assert_eq!(stats.msaa, 4);
    let (shadowed, lit) = (sample(&px, shadowed_at), sample(&px, lit_at));
    assert!(!is_background(shadowed) && !is_background(lit));
    assert!(
        luminance(shadowed) < luminance(lit) * 0.7,
        "shadow {shadowed:?} vs lit {lit:?}"
    );
    // The shadow's edge is filtered: along +X out of the shadow the slab
    // brightens over more than one pixel.
    let row: Vec<f32> = (shadowed_at.0..shadowed_at.0 + 24)
        .map(|x| luminance(sample(&px, (x, shadowed_at.1))))
        .collect();
    let steps = row.windows(2).filter(|w| w[1] > w[0] + 1.0).count();
    assert!(steps >= 2, "PCF edge over several pixels: {row:?}");

    // A profile without shadows: the slab is lit everywhere.
    r.set_quality(Quality {
        shadows: false,
        ..Quality::default()
    });
    let px = render_offscreen(&device, &queue, &mut r, &e);
    let stats = r.stats();
    assert_eq!((stats.shadow_map, stats.shadow_casters), (0, 0));
    assert_eq!(r.quality().profile, "custom");
    let (unshadowed, lit) = (sample(&px, shadowed_at), sample(&px, lit_at));
    assert!(
        (luminance(unshadowed) - luminance(lit)).abs() < 12.0,
        "{unshadowed:?} vs {lit:?}"
    );
    // A smaller map still shadows.
    r.set_quality(Quality {
        shadow_resolution: 512,
        ..Quality::default()
    });
    let px = render_offscreen(&device, &queue, &mut r, &e);
    assert_eq!(r.stats().shadow_map, 512);
    assert!(luminance(sample(&px, shadowed_at)) < luminance(sample(&px, lit_at)) * 0.7);
    // A sun that does not cast leaves the map unused.
    e.directional_lights[0].light.cast_shadows = false;
    render_offscreen(&device, &queue, &mut r, &e);
    assert_eq!(r.stats().shadow_map, 0);
}

#[test]
fn culling_drops_what_is_off_screen_without_changing_the_picture() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut r = renderer(&device);
    r.camera.eye = Vec3::new(0.0, 8.0, 6.0);
    // The shadow test's slab and cube, plus: a cube high up and to -X,
    // outside the view but whose shadow lands on the slab at (0, 2); a
    // cube behind the camera; one beyond the far plane; one far to +X.
    let mut e = Extract::default();
    e.records.push(record(
        0,
        Mat4::from_scale_rotation_translation(
            Vec3::new(10.0, 0.2, 10.0),
            Quat::IDENTITY,
            Vec3::new(0.0, -0.6, 0.0),
        ),
    ));
    e.records.push(record(1, Mat4::IDENTITY));
    e.records
        .push(record(2, Mat4::from_translation(Vec3::new(-6.0, 9.5, 2.0))));
    e.records
        .push(record(3, Mat4::from_translation(Vec3::new(0.0, 8.0, 20.0))));
    e.records.push(record(
        4,
        Mat4::from_translation(Vec3::new(0.0, 0.0, -300.0)),
    ));
    e.records
        .push(record(5, Mat4::from_translation(Vec3::new(80.0, 0.0, 0.0))));
    let mut sun = sun(Vec3::new(0.6, -1.0, 0.0), 10_000.0);
    sun.light.cast_shadows = true;
    e.directional_lights.push(sun);
    let sample = |px: &[[u8; 4]], (x, y): (u32, u32)| px[(y * W + x) as usize];
    let shadowed_at = pixel_of(&r, Vec3::new(0.0, -0.5, 2.0));
    let lit_at = pixel_of(&r, Vec3::new(-2.0, -0.5, -2.0));

    let culled = render_offscreen(&device, &queue, &mut r, &e);
    dump(&culled, "culling");
    let stats = r.stats();
    assert_eq!(
        (
            stats.instances,
            stats.culled,
            stats.draws,
            stats.shadow_draws
        ),
        (2, 4, 1, 1),
        "{stats:?}"
    );
    // The off-screen cube above the slab is still a caster.
    assert!(stats.shadow_casters >= 3, "{stats:?}");
    assert!(
        luminance(sample(&culled, shadowed_at)) < luminance(sample(&culled, lit_at)) * 0.7,
        "an off-screen caster shadows the view"
    );

    r.frustum_culling = false;
    let all = render_offscreen(&device, &queue, &mut r, &e);
    let stats = r.stats();
    assert_eq!(
        (stats.instances, stats.culled, stats.shadow_casters),
        (6, 0, 6)
    );
    assert!(culled == all, "culling changed the picture");
}

#[test]
fn casters_and_receivers_opt_out_per_mesh_renderer() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut lab = Lab::new("casters");
    let m = lab.material(json!({ "base_color": [0.8, 0.8, 0.8, 1.0], "roughness": 0.9 }));
    let eye = Vec3::new(0.0, 3.0, 6.0);
    let mut render = |cube_casts: bool, floor_receives: bool, sun_casts: bool| {
        lab.sun_shadows = sun_casts;
        let floor = LabCube {
            scale: Vec3::new(8.0, 0.1, 8.0),
            receive_shadows: floor_receives,
            ..LabCube::new(Vec3::new(0.0, -0.55, 0.0), m)
        };
        let cube = LabCube {
            cast_shadows: cube_casts,
            ..LabCube::new(Vec3::ZERO, m)
        };
        let (assets, extract) = lab.extract_cubes(eye, &[floor, cube], true);
        assert_eq!(extract.meshes.len(), 2);
        let (px, r) = render_lab(&device, &queue, &assets, &extract);
        (px, r.stats())
    };
    let darker_by = |a: &[[u8; 4]], b: &[[u8; 4]], min: f32| {
        a.iter()
            .zip(b)
            .filter(|(p, q)| luminance(**q) - luminance(**p) > min)
            .count()
    };
    let (with, stats) = render(true, true, true);
    assert_eq!(stats.shadow_casters, 2);
    let (without, stats) = render(false, true, true);
    assert_eq!(stats.shadow_casters, 1, "the floor still casts");
    dump(&with, "casters-with");
    dump(&without, "casters-without");
    let shadow_px = darker_by(&with, &without, 20.0);
    assert!(shadow_px > 40, "the cube shadows {shadow_px} floor pixels");
    let (unreceived, _) = render(true, false, true);
    assert_eq!(
        darker_by(&unreceived, &without, 20.0),
        0,
        "a floor that does not receive is lit as if nothing cast"
    );
    let (no_sun_shadow, stats) = render(true, true, false);
    assert_eq!(stats.shadow_map, 0);
    assert_eq!(darker_by(&no_sun_shadow, &without, 20.0), 0);
}

// -------------------------------------------- renderer V1 (b): transparency

#[test]
fn blend_materials_composite_back_to_front_whatever_the_record_order() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut lab = Lab::new("blend");
    lab.sun_shadows = true;
    let red_glass = lab.material(json!({
        "base_color": [1.0, 0.0, 0.0, 0.5], "alpha_mode": "blend", "roughness": 1.0 }));
    let green_glass = lab.material(json!({
        "base_color": [0.0, 1.0, 0.0, 0.5], "alpha_mode": "blend", "roughness": 1.0 }));
    let blue = lab.material(json!({ "base_color": [0.0, 0.0, 1.0, 1.0], "roughness": 1.0 }));
    let eye = Vec3::new(0.0, 0.0, 4.0);
    let centre = |px: &[[u8; 4]]| px[(H / 2 * W + W / 2) as usize];
    let render = |cubes: &[(Vec3, Uuid)]| {
        let (assets, extract) = lab.extract(eye, cubes, true);
        let (px, r) = render_lab(&device, &queue, &assets, &extract);
        (px, r.stats())
    };
    // Opaque blue behind red glass: both show through at the centre.
    let (front_first, stats) =
        render(&[(Vec3::ZERO, red_glass), (Vec3::new(0.0, 0.0, -1.5), blue)]);
    dump(&front_first, "blend");
    assert_eq!((stats.batches, stats.transparent), (1, 1));
    assert_eq!(stats.shadow_casters, 1, "glass casts no shadow");
    let c = centre(&front_first);
    assert!(c[0] > 60 && c[2] > 60 && c[1] < 40, "red over blue: {c:?}");
    let (back_first, _) = render(&[(Vec3::new(0.0, 0.0, -1.5), blue), (Vec3::ZERO, red_glass)]);
    assert_eq!(front_first, back_first, "record order does not matter");
    // The blue alone has no red; the glass alone over the background is
    // half clear colour.
    let (opaque_only, _) = render(&[(Vec3::new(0.0, 0.0, -1.5), blue)]);
    assert!(centre(&opaque_only)[0] < 20, "{:?}", centre(&opaque_only));
    let (glass_only, _) = render(&[(Vec3::ZERO, red_glass)]);
    let g = centre(&glass_only);
    assert!(
        !is_background(g) && g[0] > 60 && g[1] < 40 && g[2] < 60,
        "{g:?}"
    );
    // Two glasses: the nearer one is composited last, so red (near)
    // dominates green (far) whichever way the records are ordered.
    let stack = [
        (Vec3::ZERO, red_glass),
        (Vec3::new(0.0, 0.0, -1.0), green_glass),
        (Vec3::new(0.0, 0.0, -2.0), blue),
    ];
    let (stacked, stats) = render(&stack);
    assert_eq!(stats.transparent, 2);
    let s = centre(&stacked);
    assert!(s[0] > s[1] && s[1] > 20, "red over green over blue: {s:?}");
    let mut reversed = stack;
    reversed.reverse();
    assert_eq!(render(&reversed).0, stacked);
}

// ------------------------------------------ renderer V1 (b): MSAA and scale

#[test]
fn msaa_4_blends_silhouette_edges_and_msaa_1_does_not() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut r = renderer(&device);
    let mut e = cube_scene(Vec3::new(0.0, 1.5, 4.0), Camera::default());
    e.records[1].world = Mat4::from_rotation_y(0.4);
    e.directional_lights
        .push(sun(Vec3::new(-0.3, -1.0, -0.5), 10_000.0));
    let render = |r: &mut ExtractRenderer, msaa: u32| {
        r.set_quality(Quality {
            msaa,
            ..Quality::default()
        });
        let px = render_offscreen(&device, &queue, r, &e);
        assert_eq!(r.stats().msaa, msaa);
        px
    };
    let aliased = render(&mut r, 1);
    let smooth = render(&mut r, 4);
    dump(&aliased, "msaa1");
    dump(&smooth, "msaa4");
    // Interior pixels are shaded once per pixel either way, so only the
    // edges differ: silhouette pixels that were background at one sample
    // become a mix of the cube and the clear colour at four.
    let differ = aliased.iter().zip(&smooth).filter(|(a, b)| a != b).count();
    assert!(differ > 50, "{differ} edge pixels differ");
    let mixed = aliased
        .iter()
        .zip(&smooth)
        .filter(|(a, b)| is_background(**a) && !is_background(**b))
        .count();
    assert!(
        mixed > 20,
        "{mixed} silhouette pixels blend with the background"
    );
    let interior = aliased
        .iter()
        .zip(&smooth)
        .filter(|(a, b)| a == b && !is_background(**a))
        .count();
    assert!(interior > 500, "{interior} interior pixels identical");
    // MSAA 2 rounds down to 1, 8 to 4.
    r.set_quality(Quality {
        msaa: 8,
        ..Quality::default()
    });
    assert_eq!(render_offscreen(&device, &queue, &mut r, &e), smooth);
    r.set_quality(Quality {
        msaa: 2,
        ..Quality::default()
    });
    assert_eq!(render_offscreen(&device, &queue, &mut r, &e), aliased);
}

#[test]
fn render_scale_shades_fewer_pixels_and_presents_at_the_target_size() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut r = renderer(&device);
    let mut e = cube_scene(Vec3::new(0.0, 1.5, 4.0), Camera::default());
    e.directional_lights
        .push(sun(Vec3::new(-0.3, -1.0, -0.5), 10_000.0));
    let native = render_offscreen(&device, &queue, &mut r, &e);
    assert_eq!(r.quality().internal_size, (W, H));
    let native_drawn = drawn_mean(&native).1;
    for (scale, size) in [
        (0.5, (W / 2, H / 2)),
        (0.75, (96, 96)),
        (2.0, (W * 2, H * 2)),
    ] {
        r.set_quality(Quality {
            render_scale: scale,
            ..Quality::default()
        });
        let px = render_offscreen(&device, &queue, &mut r, &e);
        assert_eq!(px.len(), (W * H) as usize);
        let stats = r.stats();
        assert_eq!((stats.render_width, stats.render_height), size);
        assert_eq!(r.quality().internal_size, size);
        dump(&px, &format!("scale-{scale}"));
        // The picture is the same picture, resampled: the cube covers
        // about the same area, the corners are exactly the clear colour.
        let drawn = drawn_mean(&px).1;
        let ratio = drawn as f32 / native_drawn as f32;
        assert!((0.8..=1.25).contains(&ratio), "scale {scale}: {ratio}");
        assert!(is_background(px[(2 * W + 2) as usize]));
        let c = px[(H / 2 * W + W / 2) as usize];
        let n = native[(H / 2 * W + W / 2) as usize];
        assert!(
            (0..3).all(|i| (c[i] as i32 - n[i] as i32).abs() < 24),
            "centre {c:?} vs native {n:?}"
        );
    }
    // Back to native: the frame is pixel-identical again.
    r.set_quality(Quality::default());
    assert_eq!(render_offscreen(&device, &queue, &mut r, &e), native);
}

// ----------------------------------------- renderer V1 (b): quality profiles

#[test]
fn quality_profiles_come_from_the_project_and_switch_at_runtime() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut lab = Lab::new("quality");
    lab.sun_shadows = true;
    let m = lab.material(json!({}));
    let (assets, extract) = lab.extract(Vec3::new(0.0, 0.0, 3.0), &[(Vec3::ZERO, m)], true);
    let mut r = renderer(&device);
    r.set_assets(Some(assets.clone()));
    assert_eq!(r.quality_profiles(), ["low", "medium", "high"]);
    // Before the first frame the engine defaults are in force.
    assert_eq!(r.quality().profile, "default");
    render_offscreen(&device, &queue, &mut r, &extract);
    let q = r.quality();
    assert_eq!(q.profile, "medium");
    assert_eq!(q.unknown_profile, None);
    assert_eq!(
        (
            q.settings.shadow_resolution,
            q.settings.msaa,
            q.internal_size
        ),
        (1024, 4, (W, H))
    );
    assert_eq!(r.stats().shadow_map, 1024);

    assert!(r.set_quality_profile(Some("low")));
    render_offscreen(&device, &queue, &mut r, &extract);
    let q = r.quality();
    assert_eq!(q.profile, "low");
    assert_eq!(
        (
            q.settings.shadows,
            q.settings.msaa,
            q.settings.anisotropy,
            q.internal_size
        ),
        (false, 1, 1, (96, 96))
    );
    let stats = r.stats();
    assert_eq!(
        (stats.shadow_map, stats.msaa, stats.render_width),
        (0, 1, 96)
    );

    assert!(r.set_quality_profile(Some("high")));
    render_offscreen(&device, &queue, &mut r, &extract);
    let q = r.quality();
    assert_eq!(q.profile, "high");
    assert_eq!(
        (
            q.settings.shadow_resolution,
            q.settings.msaa,
            q.settings.anisotropy
        ),
        (4096, 4, 16),
        "msaa 8 clamps to 4"
    );

    // An unknown profile is refused and the project default applies.
    assert!(!r.set_quality_profile(Some("ultra")));
    render_offscreen(&device, &queue, &mut r, &extract);
    let q = r.quality();
    assert_eq!(q.profile, "medium");
    assert_eq!(q.unknown_profile.as_deref(), Some("ultra"));
    // None: the project default, explicitly.
    assert!(r.set_quality_profile(None));
    render_offscreen(&device, &queue, &mut r, &extract);
    assert_eq!(r.quality().profile, "medium");
    // An explicit override wins until a profile is selected again.
    r.set_quality(Quality {
        msaa: 1,
        ..Quality::default()
    });
    render_offscreen(&device, &queue, &mut r, &extract);
    assert_eq!(r.quality().profile, "custom");
    assert_eq!(r.stats().msaa, 1);
    assert!(r.set_quality_profile(Some("medium")));
    render_offscreen(&device, &queue, &mut r, &extract);
    assert_eq!(r.stats().msaa, 4);
    assert_eq!(r.asset_errors(), &[] as &[String]);
}

// ------------------------------------------- renderer V1 (b): debug lines

#[test]
fn debug_lines_draw_on_top_and_are_depth_tested() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut r = renderer(&device);
    r.set_quality(Quality {
        msaa: 1,
        ..Quality::default()
    });
    let mut e = cube_scene(Vec3::new(0.0, 1.5, 4.0), Camera::default());
    e.directional_lights
        .push(sun(Vec3::new(-0.3, -1.0, -0.5), 10_000.0));
    let red = |px: &[[u8; 4]]| {
        px.iter()
            .filter(|p| p[0] > 200 && p[1] < 30 && p[2] < 30)
            .count()
    };
    let plain = render_offscreen(&device, &queue, &mut r, &e);
    assert_eq!(red(&plain), 0);
    assert_eq!(r.stats().debug_lines, 0);
    // A line across the view at the cube's height: display-referred pure
    // red where it is not hidden by the cube.
    r.debug().line(
        Vec3::new(-3.0, 0.0, 1.0),
        Vec3::new(3.0, 0.0, 1.0),
        [1.0, 0.0, 0.0, 1.0],
    );
    let with_line = render_offscreen(&device, &queue, &mut r, &e);
    dump(&with_line, "debug-line");
    assert_eq!(r.stats().debug_lines, 1);
    assert!(red(&with_line) > 60, "{}", red(&with_line));
    // The queue is per frame: the next frame has no line.
    let next = render_offscreen(&device, &queue, &mut r, &e);
    assert_eq!(next, plain);
    // A line inside the cube is hidden by it.
    r.debug().line(
        Vec3::new(-0.4, 0.0, 0.0),
        Vec3::new(0.4, 0.0, 0.0),
        [1.0, 0.0, 0.0, 1.0],
    );
    let hidden = render_offscreen(&device, &queue, &mut r, &e);
    assert_eq!(red(&hidden), 0, "depth tested");
    // An AABB round the cube: its front edges show over the cube.
    r.debug()
        .aabb(Vec3::splat(-0.5), Vec3::splat(0.5), [1.0, 0.0, 0.0, 1.0]);
    let boxed = render_offscreen(&device, &queue, &mut r, &e);
    dump(&boxed, "debug-aabb");
    assert_eq!(r.stats().debug_lines, 12);
    assert!(red(&boxed) > 30, "{}", red(&boxed));
}

/// M3-16: the instrumentation. Pass timestamps come back two or three
/// frames after the draw (never blocking), the CPU stages are timed, and
/// the GPU memory estimate counts what the renderer holds.
#[test]
fn pass_timestamps_and_the_memory_estimate() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut r = renderer(&device);
    let e = cube_scene(Vec3::new(0.0, 0.0, 4.0), Camera::default());
    // Each offscreen render prepares, draws, submits and waits for the
    // GPU: frame n's stamps are resolved by frame n + 1's draw, mapped by
    // frame n + 2's prepare and read by frame n + 3's.
    for _ in 0..5 {
        render_offscreen(&device, &queue, &mut r, &e);
    }
    let s = r.stats();
    eprintln!("{s:?}");
    assert!(s.prepare_us > 0 || s.encode_us > 0, "{s:?}");
    // Attachments at 128² (depth ×4 samples, HDR ×4 + resolve) and the
    // shadow map alone are megabytes.
    assert!(s.gpu_bytes > 1 << 20, "{s:?}");
    if device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
        assert!(s.gpu_timestamps, "{s:?}");
        assert!(
            s.gpu_scene_us > 0 && s.gpu_frame_us >= s.gpu_scene_us,
            "{s:?}"
        );
        assert_eq!(s.gpu_untimed, 0, "{s:?}");
    } else {
        eprintln!("no timestamp queries on this adapter");
        assert!(!s.gpu_timestamps);
    }
}
