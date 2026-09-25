//! Renderer V1 (a), (b) and (c) feature gate (blueprint §19.2d items 4–6):
//! one pixel test per feature. Hand-built extracts (the renderer reads
//! nothing else) for the camera, exposure, lights, environment, shadows,
//! MSAA, render scale, culling and debug lines; a project cooked on the
//! fly for everything that needs a material or a quality profile.

use std::path::{Path, PathBuf};

use fynkel_contracts::{AssetRef, Entity, Uuid};
use fynkel_core::volume::PostSettings;

/// R2's ambient occlusion exponent: the engine default until 2026-09-24,
/// a volume's since (no volume, no post effects).
const R2_AO_INTENSITY: f32 = 1.35;

/// A global volume over the whole frame, record 9.
fn global_volume(profile: &AssetRef, priority: i32) -> ExtractVolume {
    ExtractVolume {
        record: 9,
        world: Mat4::IDENTITY,
        volume: Volume {
            profile: profile.clone(),
            priority,
            ..Volume::default()
        },
    }
}
use fynkel_core::{
    Camera, DirectionalLight, Environment, ExposureMode, Extract, ExtractCamera,
    ExtractDirectionalLight, ExtractEnvironment, ExtractPointLight, ExtractRecord,
    ExtractSpotLight, ExtractVolume, PointLight, Projection, SpotLight, Volume, VolumeShape, World,
    WorldConfig,
};
use fynkel_import::{cook_project, CookOptions};
use fynkel_render::{
    clear_rgba8, exposure, headless_device, render_offscreen, Assets, ExtraWorld, ExtractRenderer,
    Grid, OutlineStyle, Quality, LIGHT_POOL, MAX_LIGHTS, PIPELINE_VARIANTS,
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
        layer_bit: 1,
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
            culling_mask: u32::MAX,
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

    // The budgets (E1 light culling): every light that reaches the view
    // goes into the frame's pool, and each object takes the MAX_LIGHTS
    // strongest at its own bounds out of it.
    e.point_lights.clear();
    for i in 0..20 {
        e.point_lights
            .push(point(Vec3::new(i as f32, 3.0, 0.0), 50.0, 30.0));
    }
    render_offscreen(&device, &queue, &mut r, &e);
    let s = r.stats();
    assert_eq!(
        (s.lights, s.lights_dropped),
        (20, 0),
        "all of them reach it"
    );
    assert_eq!(s.lights_max_per_object, MAX_LIGHTS as u32);
    assert_eq!(
        s.lights_overflow, 2,
        "both drawn cubes — the scene's and the camera's placeholder — had          more lights on them than one object can take"
    );

    // Past the pool the nearest to the camera win, as before. (Clustered
    // over the cube: spread out, the far ones would miss the view and
    // never reach the pool at all — the case below.)
    e.point_lights.clear();
    for i in 0..(LIGHT_POOL + 20) {
        e.point_lights
            .push(point(Vec3::new(i as f32 * 0.01, 3.0, 0.0), 50.0, 30.0));
    }
    render_offscreen(&device, &queue, &mut r, &e);
    let s = r.stats();
    assert_eq!((s.lights, s.lights_dropped), (LIGHT_POOL as u32, 20));
    assert_eq!(s.lights_max_per_object, MAX_LIGHTS as u32);

    // A light whose range cannot reach the view is not in the pool at
    // all — nothing that is shaded could have seen it.
    e.point_lights.clear();
    e.point_lights
        .push(point(Vec3::new(2.5, 0.5, 2.5), 300.0, 20.0));
    e.point_lights
        .push(point(Vec3::new(400.0, 0.0, 0.0), 300.0, 20.0));
    render_offscreen(&device, &queue, &mut r, &e);
    assert_eq!((r.stats().lights, r.stats().lights_dropped), (1, 0));
}

#[test]
fn each_object_is_lit_by_the_lights_that_reach_it() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut r = renderer(&device);
    // Two cubes six metres apart, each with its own lights close by and
    // out of reach of the other: eight red ones around the left cube and
    // a single green one at the right. The red eight are nearer the
    // camera, so a frame budget of "the eight nearest the camera" would
    // spend itself on the left cube and leave the right one black.
    let eye = Vec3::new(0.0, 0.0, 12.0);
    let mut e = Extract::default();
    e.records.push(record(0, Mat4::from_translation(eye)));
    e.records
        .push(record(1, Mat4::from_translation(Vec3::new(-3.0, 0.0, 0.0))));
    e.records
        .push(record(2, Mat4::from_translation(Vec3::new(3.0, 0.0, 0.0))));
    e.camera = Some(ExtractCamera {
        record: 0,
        world: Mat4::from_translation(eye),
        camera: Camera::default(),
    });
    e.environment = Some(environment(Vec3::ONE, 0.0));
    manual(&mut e, 4.0);
    let light = |position: Vec3, color: Vec3| ExtractPointLight {
        record: 0,
        position,
        light: PointLight {
            color,
            intensity: 400.0,
            range: 4.0,
            culling_mask: u32::MAX,
        },
    };
    for i in 0..8 {
        e.point_lights.push(light(
            Vec3::new(-3.0 + i as f32 * 0.01, 0.0, 2.0),
            Vec3::new(1.0, 0.0, 0.0),
        ));
    }
    e.point_lights
        .push(light(Vec3::new(3.0, 0.0, 2.0), Vec3::new(0.0, 1.0, 0.0)));

    let px = render_offscreen(&device, &queue, &mut r, &e);
    dump(&px, "per-object-lights");
    let s = r.stats();
    assert_eq!(
        (s.lights, s.lights_dropped),
        (9, 0),
        "all nine are in the pool"
    );
    assert_eq!(
        s.lights_max_per_object, MAX_LIGHTS as u32,
        "the left cube took eight"
    );
    assert_eq!(s.lights_overflow, 0, "nobody had more than eight on it");

    // Each cube in its own light's colour, and neither in the other's.
    let half = |left: bool, c: usize| {
        let mut sum = 0.0;
        let mut n = 0;
        for y in 0..H {
            for x in 0..W {
                let p = px[(y * W + x) as usize];
                if is_background(p) || (x < W / 2) != left {
                    continue;
                }
                sum += p[c] as f32;
                n += 1;
            }
        }
        (sum / n.max(1) as f32, n)
    };
    let (left_red, left_n) = half(true, 0);
    let (left_green, _) = half(true, 1);
    let (right_green, right_n) = half(false, 1);
    let (right_red, _) = half(false, 0);
    assert!(
        left_n > 80 && right_n > 80,
        "both cubes are drawn ({left_n}, {right_n})"
    );
    assert!(
        left_red > left_green + 20.0,
        "the left cube is red ({left_red} vs {left_green})"
    );
    assert!(
        right_green > right_red + 20.0,
        "the right cube is lit by its own light, not left black ({right_green} vs {right_red})"
    );
}

#[test]
fn layer_masks_hide_objects_from_the_camera_and_from_lights() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut r = renderer(&device);
    // Two cubes three metres either side, the right one on layer 1; one
    // white light between them in front, reaching both.
    let eye = Vec3::new(0.0, 0.0, 12.0);
    let mut e = Extract::default();
    e.records.push(record(0, Mat4::from_translation(eye)));
    e.records
        .push(record(1, Mat4::from_translation(Vec3::new(-3.0, 0.0, 0.0))));
    let mut right = record(2, Mat4::from_translation(Vec3::new(3.0, 0.0, 0.0)));
    right.layer_bit = 1 << 1;
    e.records.push(right);
    e.camera = Some(ExtractCamera {
        record: 0,
        world: Mat4::from_translation(eye),
        camera: Camera::default(),
    });
    e.environment = Some(environment(Vec3::ONE, 0.0));
    manual(&mut e, 4.0);
    e.point_lights.push(ExtractPointLight {
        record: 0,
        position: Vec3::new(0.0, 0.0, 2.0),
        light: PointLight {
            color: Vec3::ONE,
            intensity: 800.0,
            range: 8.0,
            culling_mask: u32::MAX,
        },
    });
    let half = |px: &[[u8; 4]], left: bool| {
        let (mut sum, mut n) = (0.0, 0);
        for y in 0..H {
            for x in 0..W {
                let p = px[(y * W + x) as usize];
                if is_background(p) || (x < W / 2) != left {
                    continue;
                }
                sum += luminance(p);
                n += 1;
            }
        }
        (sum / n.max(1) as f32, n)
    };

    // Everything: both cubes drawn and lit alike.
    let px = render_offscreen(&device, &queue, &mut r, &e);
    let ((left, left_n), (right, right_n)) = (half(&px, true), half(&px, false));
    assert!(
        left_n > 80 && right_n > 80,
        "both drawn ({left_n}, {right_n})"
    );
    assert!((left - right).abs() < 10.0, "lit alike ({left} vs {right})");
    assert_eq!(r.stats().layer_hidden, 0);

    // The light leaves layer 1 out: the right cube is drawn, unlit.
    e.point_lights[0].light.culling_mask = !(1 << 1);
    let px = render_offscreen(&device, &queue, &mut r, &e);
    dump(&px, "layer-light-mask");
    let ((left_lit, _), (right_unlit, right_n)) = (half(&px, true), half(&px, false));
    assert!(right_n > 80, "still drawn ({right_n})");
    assert!(
        right_unlit < left_lit * 0.3,
        "the masked light does not light it ({right_unlit} vs {left_lit})"
    );

    // The camera leaves layer 1 out: the right cube is not drawn.
    e.point_lights[0].light.culling_mask = u32::MAX;
    e.camera.as_mut().unwrap().camera.culling_mask = 1;
    let px = render_offscreen(&device, &queue, &mut r, &e);
    dump(&px, "layer-camera-mask");
    let ((_, left_n), (_, right_n)) = (half(&px, true), half(&px, false));
    assert!(left_n > 80, "the default layer is drawn ({left_n})");
    assert_eq!(right_n, 0, "layer 1 is not drawn");
    assert_eq!(r.stats().layer_hidden, 1);

    // Nor does it cast a shadow in that camera's frame.
    let mut sun = sun(Vec3::new(0.3, -1.0, -0.2), 10_000.0);
    sun.light.cast_shadows = true;
    e.directional_lights.push(sun);
    e.camera.as_mut().unwrap().camera.culling_mask = u32::MAX;
    render_offscreen(&device, &queue, &mut r, &e);
    let casters = r.stats().shadow_casters;
    e.camera.as_mut().unwrap().camera.culling_mask = 1;
    render_offscreen(&device, &queue, &mut r, &e);
    assert_eq!(r.stats().shadow_casters, casters - 1, "{:?}", r.stats());
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
            culling_mask: u32::MAX,
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
    assert_eq!(PIPELINE_VARIANTS, 30);
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

    /// Writes a volume profile (R3a) with `fields`; returns its reference.
    fn volume(&self, n: u32, fields: Value) -> AssetRef {
        let id = format!("c0000000-0000-4000-8000-0000000007{n:02x}");
        let mut doc = json!({
            "$schema": "https://fynkel.dev/schemas/volume.schema.json",
            "formatVersion": 1,
            "id": id,
            "name": format!("Lab volume {n}"),
        });
        for (k, v) in fields.as_object().unwrap() {
            doc[k] = v.clone();
        }
        std::fs::write(
            self.dir.join(format!("assets/lab{n}.volume.json")),
            serde_json::to_string_pretty(&doc).unwrap(),
        )
        .unwrap();
        AssetRef::new(id.parse().unwrap())
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
            "formatVersion": 2,
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
                    "priority": 0, "exposure_mode": "manual", "exposure": 12.0, "culling_mask": 4294967295u32 } } } }),
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
                        "mesh": { "asset": ROBOT_MESH, "sub": "mesh/Cube" },
                        "material": { "asset": cube.material.to_string() },
                        "cast_shadows": cube.cast_shadows, "receive_shadows": cube.receive_shadows, "visible": true } } } }),
            );
        }
        let scene = json!({
            "$schema": "https://fynkel.dev/schemas/scene.schema.json",
            "formatVersion": 5,
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

/// R1: the shadowed range is split into cascades in a 2×2 atlas. A
/// corridor sixty metres deep with a caster at each end: both are
/// shadowed (so every quadrant of the atlas is drawn and sampled where
/// it should be), the near one keeps contact with the ground, and the
/// lit floor between them has no seam where the cascades change.
#[test]
fn cascades_shadow_the_whole_corridor_without_a_seam() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut r = renderer(&device);
    r.camera.eye = Vec3::new(0.0, 4.0, 10.0);
    r.camera.target = Vec3::new(0.0, 0.0, -25.0);
    let mut e = Extract::default();
    // The floor: 30 × 140 m, its top at y = -0.5.
    e.records.push(record(
        0,
        Mat4::from_scale_rotation_translation(
            Vec3::new(30.0, 0.2, 140.0),
            Quat::IDENTITY,
            Vec3::new(0.0, -0.6, -40.0),
        ),
    ));
    // A unit cube two metres from the camera, and a four-metre one forty
    // metres down the corridor — the first and the last cascade.
    e.records
        .push(record(1, Mat4::from_translation(Vec3::new(-3.0, 0.0, 2.0))));
    e.records.push(record(
        2,
        Mat4::from_scale_rotation_translation(
            Vec3::splat(4.0),
            Quat::IDENTITY,
            Vec3::new(5.0, 1.5, -40.0),
        ),
    ));
    let mut sun = sun(Vec3::new(0.6, -1.0, 0.0), 10_000.0);
    sun.light.cast_shadows = true;
    e.directional_lights.push(sun);

    let px = render_offscreen(&device, &queue, &mut r, &e);
    dump(&px, "cascades");
    let stats = r.stats();
    assert_eq!(
        stats.shadow_cascades,
        fynkel_render::shadow::CASCADES as u32
    );
    assert_eq!(stats.shadow_map, 2048);
    let sample = |world: Vec3| {
        let (x, y) = pixel_of(&r, world);
        px[(y.min(H - 1) * W + x.min(W - 1)) as usize]
    };

    // Both casters shadow the floor.
    let near_lit = sample(Vec3::new(1.0, -0.5, 2.0));
    let near_shadow = sample(Vec3::new(-2.4, -0.5, 2.0));
    assert!(!is_background(near_lit) && !is_background(near_shadow));
    assert!(
        luminance(near_shadow) < luminance(near_lit) * 0.7,
        "near {near_shadow:?} vs {near_lit:?}"
    );
    let far_lit = sample(Vec3::new(0.0, -0.5, -40.0));
    let far_shadow = sample(Vec3::new(7.4, -0.5, -40.0));
    assert!(!is_background(far_lit) && !is_background(far_shadow));
    assert!(
        luminance(far_shadow) < luminance(far_lit) * 0.7,
        "far {far_shadow:?} vs {far_lit:?}"
    );
    // The near cube's shadow starts at its own foot: a cascade fitted to
    // sixty metres offsets the lookup far enough to detach it.
    let contact = sample(Vec3::new(-2.45, -0.5, 2.0));
    assert!(
        luminance(contact) < luminance(near_lit) * 0.7,
        "the shadow keeps contact: {contact:?} vs {near_lit:?}"
    );

    // The lit floor down the middle crosses every cascade boundary; a
    // seam would show as a step in luminance.
    let walk: Vec<f32> = (0..40)
        .map(|i| luminance(sample(Vec3::new(0.0, -0.5, 5.0 - i as f32 * 1.2))))
        .collect();
    let step = walk
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0f32, f32::max);
    assert!(step < 8.0, "no cascade seam on the lit floor: {walk:?}");
}

/// R2: the ambient occlusion the profile can turn off. Lit by ambient
/// alone, so what the floor shows is the occlusion term and nothing else.
/// A 40 m floor with its top at y = 0 and a two-metre cube standing on
/// it, its -X face at x = -1 and its +X face at x = 1, under ambient
/// light only (R2's scene, reused by R3a's volumes).
fn ao_corner_scene() -> Extract {
    let mut e = Extract::default();
    e.records.push(record(
        0,
        Mat4::from_scale_rotation_translation(
            Vec3::new(40.0, 0.2, 40.0),
            Quat::IDENTITY,
            Vec3::new(0.0, -0.1, 0.0),
        ),
    ));
    e.records.push(record(
        1,
        Mat4::from_scale_rotation_translation(
            Vec3::splat(2.0),
            Quat::IDENTITY,
            Vec3::new(0.0, 1.0, 0.0),
        ),
    ));
    e.environment = Some(environment(Vec3::ONE, 6_000.0));
    e
}

#[test]
fn ambient_occlusion_darkens_what_a_wall_closes_in_and_leaves_the_open_floor() {
    let Some((device, queue)) = device() else {
        return;
    };
    // Ambient occlusion comes from a volume (none: none).
    let lab = Lab::new("ao");
    let on = lab.volume(
        1,
        json!({ "ambientOcclusion": { "intensity": R2_AO_INTENSITY } }),
    );
    let (assets, _) = lab.extract(Vec3::new(0.0, 0.3, 3.0), &[], false);
    let mut r = renderer(&device);
    r.set_assets(Some(assets));
    r.camera.eye = Vec3::new(4.0, 2.5, 6.0);
    r.camera.target = Vec3::new(0.5, 0.3, 0.0);
    let bare = ao_corner_scene();
    let mut e = bare.clone();
    e.volumes = vec![global_volume(&on, 0)];

    // The corner where the cube meets the floor, and open floor four
    // metres away — the same surface, the same light, the same normal.
    let corner = Vec3::new(1.15, 0.0, 0.0);
    let open = Vec3::new(5.0, 0.0, 0.0);
    // The camera does not move, so the pixels are fixed up front and the
    // sampler does not hold the renderer.
    let at = |world: Vec3| {
        let (x, y) = pixel_of(&r, world);
        (y.min(H - 1) * W + x.min(W - 1)) as usize
    };
    let corner_px = at(corner);
    let open_px = at(open);
    let walk_px: Vec<usize> = (0..12)
        .map(|i| at(Vec3::new(1.15 + i as f32 * 0.1, 0.0, 0.0)))
        .collect();
    let sample = |px: &[[u8; 4]], i: usize| px[i];

    let px = render_offscreen(&device, &queue, &mut r, &e);
    dump(&px, "ao");
    assert!(r.stats().ambient_occlusion);
    let (ao_corner, ao_open) = (sample(&px, corner_px), sample(&px, open_px));
    assert!(!is_background(ao_corner) && !is_background(ao_open));

    // Without a volume there is none: the same picture as the quality
    // profile's AO off.
    let none = render_offscreen(&device, &queue, &mut r, &bare);
    assert!(!r.stats().ambient_occlusion);
    // Without it the two are the same flat grey.
    r.set_quality(Quality {
        ambient_occlusion: false,
        ..Quality::default()
    });
    let flat = render_offscreen(&device, &queue, &mut r, &e);
    dump(&flat, "ao-off");
    assert!(!r.stats().ambient_occlusion);
    assert_eq!(none, flat, "no volume is the same as the profile's AO off");
    let (flat_corner, flat_open) = (sample(&flat, corner_px), sample(&flat, open_px));
    assert!(
        (luminance(flat_corner) - luminance(flat_open)).abs() < 4.0,
        "ambient alone is flat without AO: {flat_corner:?} vs {flat_open:?}"
    );

    // With it, the corner darkens and the open floor does not.
    assert!(
        luminance(ao_corner) < luminance(flat_corner) * 0.85,
        "the corner darkens: {ao_corner:?} vs {flat_corner:?}"
    );
    assert!(
        (luminance(ao_open) - luminance(flat_open)).abs() < 6.0,
        "open floor is untouched: {ao_open:?} vs {flat_open:?}"
    );

    // And the occlusion fades with distance from the wall rather than
    // stopping at an edge: walking away from it only ever lightens.
    let walk: Vec<f32> = walk_px.iter().map(|i| luminance(px[*i])).collect();
    assert!(
        walk[0] < walk[walk.len() - 1] - 3.0,
        "the occlusion fades outward: {walk:?}"
    );
}

#[test]
fn volumes_shape_the_ambient_occlusion_where_the_camera_stands() {
    let Some((device, queue)) = device() else {
        return;
    };
    // Profiles go through the real path: written as `.volume.json`,
    // cooked, read back by the asset store.
    let lab = Lab::new("volumes");
    let off = lab.volume(1, json!({ "ambientOcclusion": { "intensity": 0.0 } }));
    let wide = lab.volume(2, json!({ "ambientOcclusion": { "radius": 1.6 } }));
    let dark = lab.volume(3, json!({ "ambientOcclusion": { "intensity": 2.5 } }));
    let on = lab.volume(
        4,
        json!({ "ambientOcclusion": { "intensity": R2_AO_INTENSITY } }),
    );
    let (assets, _) = lab.extract(Vec3::new(0.0, 0.3, 3.0), &[], false);

    let mut r = renderer(&device);
    r.set_assets(Some(assets.clone()));
    r.camera.eye = Vec3::new(4.0, 2.5, 6.0);
    r.camera.target = Vec3::new(0.5, 0.3, 0.0);
    let base = ao_corner_scene();
    let at = |world: Vec3| {
        let (x, y) = pixel_of(&r, world);
        (y.min(H - 1) * W + x.min(W - 1)) as usize
    };
    let corner_px = at(Vec3::new(1.15, 0.0, 0.0));
    let near_px = at(Vec3::new(1.9, 0.0, 0.0));
    let open_px = at(Vec3::new(5.0, 0.0, 0.0));
    let lum = |px: &[[u8; 4]], i: usize| luminance(px[i]);
    let global = global_volume;
    let with = |volumes: Vec<ExtractVolume>| {
        let mut e = base.clone();
        e.volumes = volumes;
        e
    };

    // No volume: the engine defaults, every effect off.
    render_offscreen(&device, &queue, &mut r, &base);
    assert!(!r.stats().ambient_occlusion);
    assert_eq!(r.stats().volumes, 0);
    assert_eq!(r.post_settings(), PostSettings::default());
    // The reference picture: R2's occlusion from a global volume.
    let plain = render_offscreen(&device, &queue, &mut r, &with(vec![global(&on, 0)]));
    assert!(r.stats().ambient_occlusion);
    assert_eq!(r.stats().volumes, 1);

    // Intensity 0 turns the stage off: the corner is as bright as the
    // open floor, exactly as with the quality profile's AO off.
    let e = with(vec![global(&on, 0), global(&off, 1)]);
    let no_ao = render_offscreen(&device, &queue, &mut r, &e);
    dump(&no_ao, "volume-off");
    assert_eq!(r.stats().volumes, 2);
    assert!(!r.stats().ambient_occlusion);
    assert!(
        (lum(&no_ao, corner_px) - lum(&no_ao, open_px)).abs() < 4.0,
        "no occlusion at intensity 0: {:?} vs {:?}",
        no_ao[corner_px],
        no_ao[open_px]
    );
    r.set_quality(Quality {
        ambient_occlusion: false,
        ..Quality::default()
    });
    let quality_off = render_offscreen(&device, &queue, &mut r, &base);
    assert_eq!(no_ao, quality_off, "a volume's off is the profile's off");

    // The quality profile is the ceiling: with AO off there, a volume
    // that shapes it changes nothing.
    let e = with(vec![global(&dark, 0)]);
    let capped = render_offscreen(&device, &queue, &mut r, &e);
    assert!(!r.stats().ambient_occlusion);
    assert_eq!(capped, quality_off);
    r.set_quality(Quality::default());

    // A wider radius reaches further from the wall: 0.9 m out, past the
    // default 0.6 m hemisphere, the floor darkens.
    let e = with(vec![global(&on, 0), global(&wide, 1)]);
    let wider = render_offscreen(&device, &queue, &mut r, &e);
    dump(&wider, "volume-wide");
    assert_eq!(r.post_settings().ao_radius, 1.6);
    assert_eq!(r.post_settings().ao_intensity, R2_AO_INTENSITY);
    assert!(
        lum(&wider, near_px) < lum(&plain, near_px) - 3.0,
        "the wide radius darkens 0.9 m out: {:?} vs {:?}",
        wider[near_px],
        plain[near_px]
    );
    // A stronger exponent darkens the corner itself.
    let e = with(vec![global(&dark, 0)]);
    let darker = render_offscreen(&device, &queue, &mut r, &e);
    assert!(lum(&darker, corner_px) < lum(&plain, corner_px) - 3.0);

    // Priority, not order: the higher one blends last.
    let e = with(vec![global(&off, 2), global(&dark, 1)]);
    render_offscreen(&device, &queue, &mut r, &e);
    assert_eq!(r.post_settings().ao_intensity, 0.0);
    assert_eq!(r.stats().volumes, 2);

    // A local box around a spot 10 m away with a 2 m fade: the camera
    // outside the fade sees the plain picture to the bit; half-way
    // through the fade it gets half the profile; inside, all of it.
    let boxed = |centre: Vec3| ExtractVolume {
        record: 9,
        world: Mat4::from_translation(centre),
        volume: Volume {
            profile: off.clone(),
            shape: VolumeShape::Box,
            size: Vec3::splat(4.0),
            blend_distance: 2.0,
            priority: 1,
            ..Volume::default()
        },
    };
    let eye = r.camera.eye;
    let e = with(vec![global(&on, 0), boxed(eye + Vec3::new(10.0, 0.0, 0.0))]);
    let outside = render_offscreen(&device, &queue, &mut r, &e);
    assert_eq!(r.stats().volumes, 1);
    assert_eq!(
        outside, plain,
        "a volume that does not reach the camera changes nothing"
    );
    // The box's -X face is 2 m from its centre; 1 m beyond it is half.
    let e = with(vec![global(&on, 0), boxed(eye + Vec3::new(3.0, 0.0, 0.0))]);
    render_offscreen(&device, &queue, &mut r, &e);
    assert_eq!(r.stats().volumes, 2);
    let half = r.post_settings().ao_intensity;
    assert!((half - R2_AO_INTENSITY * 0.5).abs() < 1e-4, "{half}");
    let e = with(vec![global(&on, 0), boxed(eye)]);
    render_offscreen(&device, &queue, &mut r, &e);
    assert_eq!(r.post_settings().ao_intensity, 0.0);
    assert!(!r.stats().ambient_occlusion);
    assert_eq!(r.asset_errors(), &[] as &[String]);
    drop(assets);
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
        // One batch of casters, drawn once per cascade (R1).
        (2, 4, 1, fynkel_render::shadow::CASCADES as u32),
        "{stats:?}"
    );
    assert_eq!(
        stats.shadow_cascades,
        fynkel_render::shadow::CASCADES as u32
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

// --------------------------------------------- the overlay (ADR-0019, 9f)

fn pure(p: [u8; 4], c: usize) -> bool {
    (0..3).all(|i| if i == c { p[i] > 250 } else { p[i] < 6 })
}

fn changed(a: &[[u8; 4]], b: &[[u8; 4]]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}

/// A camera at `eye` pitched down by `pitch` radians, and no cube.
fn looking_down(eye: Vec3, pitch: f32) -> Extract {
    let world = Mat4::from_rotation_translation(Quat::from_rotation_x(-pitch), eye);
    let mut e = Extract::default();
    e.records.push(record(0, world));
    e.camera = Some(ExtractCamera {
        record: 0,
        world,
        camera: Camera::default(),
    });
    e
}

#[test]
fn overlay_lines_have_a_width_and_anti_aliased_edges() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut r = renderer(&device);
    // Nothing but the background: the line is the only thing drawn.
    let e = looking_down(Vec3::new(0.0, 0.0, 4.0), 0.0);
    let plain = render_offscreen(&device, &queue, &mut r, &e);
    let red = [1.0, 0.0, 0.0, 1.0];
    let (a, b) = (Vec3::new(-3.0, 0.3, 0.0), Vec3::new(3.0, 0.3, 0.0));
    r.overlay().line(a, b, red, 1.5);
    let thin = render_offscreen(&device, &queue, &mut r, &e);
    assert_eq!(r.stats().debug_lines, 1);
    r.overlay().line(a, b, red, 6.0);
    let thick = render_offscreen(&device, &queue, &mut r, &e);
    dump(&thick, "overlay-thick");
    let (n_thin, n_thick) = (changed(&plain, &thin), changed(&plain, &thick));
    // 1.5 px plus the fringe covers about three rows, 6 px about seven.
    assert!(n_thick > n_thin * 2, "{n_thick} vs {n_thin}");
    // Solid in the middle, blended at the edges — neither a hard
    // staircase nor a blur.
    let solid = thick.iter().filter(|p| pure(**p, 0)).count();
    let edge = thick
        .iter()
        .zip(&plain)
        .filter(|(p, q)| p != q && !pure(**p, 0))
        .count();
    assert!(solid > W as usize * 3, "{solid}");
    assert!(edge >= W as usize, "{edge}");
    // Per frame, like the debug queue.
    assert_eq!(render_offscreen(&device, &queue, &mut r, &e), plain);
}

#[test]
fn overlay_lines_fade_behind_the_scene_unless_on_top() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut r = renderer(&device);
    let mut e = cube_scene(Vec3::new(0.0, 0.0, 4.0), Camera::default());
    e.directional_lights
        .push(sun(Vec3::new(-0.3, -1.0, -0.5), 10_000.0));
    let plain = render_offscreen(&device, &queue, &mut r, &e);
    let centre = (H / 2 * W + W / 2) as usize;
    let edge = (H / 2 * W + 4) as usize;
    // Through the cube: in front of the background at the ends, behind the
    // cube's front face in the middle.
    let (a, b) = (Vec3::new(-3.0, 0.0, 0.0), Vec3::new(3.0, 0.0, 0.0));
    r.overlay().line(a, b, [1.0, 0.0, 0.0, 1.0], 4.0);
    let faded = render_offscreen(&device, &queue, &mut r, &e);
    dump(&faded, "overlay-occluded");
    assert!(pure(faded[edge], 0), "{:?}", faded[edge]);
    let (p, q) = (faded[centre], plain[centre]);
    assert!(p[0] > q[0] + 20, "tinted: {p:?} over {q:?}");
    assert!(p[1] > 20 && p[2] > 20, "not drawn solid: {p:?}");
    r.overlay().line_on_top(a, b, [1.0, 0.0, 0.0, 1.0], 4.0);
    let on_top = render_offscreen(&device, &queue, &mut r, &e);
    assert!(pure(on_top[centre], 0), "{:?}", on_top[centre]);
    // A line lying on the cube's front face (z = 0.5) is in front of it:
    // no z-fighting, solid from end to end.
    r.overlay().line(
        Vec3::new(-0.4, 0.0, 0.5),
        Vec3::new(0.4, 0.0, 0.5),
        [1.0, 0.0, 0.0, 1.0],
        4.0,
    );
    let on_face = render_offscreen(&device, &queue, &mut r, &e);
    assert!(pure(on_face[centre], 0), "{:?}", on_face[centre]);
}

#[test]
fn the_overlay_ignores_exposure_and_msaa() {
    let Some((device, queue)) = device() else {
        return;
    };
    let line = |r: &mut ExtractRenderer| {
        r.overlay().line(
            Vec3::new(-3.0, 1.0, 0.0),
            Vec3::new(3.0, 1.0, 0.0),
            [0.2, 0.6, 1.0, 1.0],
            3.0,
        );
    };
    let mut frames = Vec::new();
    for (msaa, ev) in [(1, 8.0), (4, 8.0), (4, 16.0)] {
        let mut r = renderer(&device);
        r.set_quality(Quality {
            msaa,
            ..Quality::default()
        });
        let camera = Camera {
            exposure_mode: ExposureMode::Manual,
            exposure: ev,
            ..Default::default()
        };
        let e = cube_scene(Vec3::new(0.0, 0.0, 4.0), camera);
        line(&mut r);
        let px = render_offscreen(&device, &queue, &mut r, &e);
        // The line's own pixels (well away from the cube).
        let band: Vec<[u8; 4]> = (0..H)
            .flat_map(|y| (0..16).map(move |x| (y * W + x) as usize))
            .map(|i| px[i])
            .collect();
        frames.push(band);
    }
    assert_eq!(frames[0], frames[1], "MSAA changes nothing");
    assert_eq!(frames[1], frames[2], "exposure changes nothing");
    assert!(
        frames[0]
            .iter()
            .any(|p| p[..3] == [51, 153, 255] || (p[2] == 255 && p[1] > 150)),
        "the line's own colour"
    );
}

#[test]
fn overlay_triangles_are_filled_and_anti_aliased() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut r = renderer(&device);
    let e = looking_down(Vec3::new(0.0, 0.0, 4.0), 0.0);
    let plain = render_offscreen(&device, &queue, &mut r, &e);
    r.overlay().triangle_on_top(
        Vec3::new(-1.0, -1.0, 0.0),
        Vec3::new(1.0, -1.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        [0.0, 1.0, 0.0, 1.0],
    );
    let tri = render_offscreen(&device, &queue, &mut r, &e);
    dump(&tri, "overlay-triangle");
    assert_eq!(r.stats().overlay_triangles, 1);
    let solid = tri.iter().filter(|p| pure(**p, 1)).count();
    let edge = tri
        .iter()
        .zip(&plain)
        .filter(|(p, q)| p != q && !pure(**p, 1))
        .count();
    assert!(solid > 1000, "{solid}");
    assert!(
        edge > 60 && edge < solid / 4,
        "{edge} edge pixels, {solid} solid"
    );
}

#[test]
fn the_ground_grid_lies_on_a_floor_without_fighting_it() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut r = renderer(&device);
    // A 40 m floor whose top is at `top`, seen from 2 m up, pitched down.
    let scene = |top: f32| {
        let mut e = looking_down(Vec3::new(0.0, 2.0, 6.0), 0.35);
        e.records.push(record(
            1,
            Mat4::from_scale_rotation_translation(
                Vec3::new(40.0, 1.0, 40.0),
                Quat::IDENTITY,
                Vec3::new(0.0, top - 0.5, 0.0),
            ),
        ));
        e.environment = Some(environment(Vec3::ONE, 20_000.0));
        e
    };
    let floor = scene(0.0);
    let plain = render_offscreen(&device, &queue, &mut r, &floor);
    r.overlay().grid(Grid::default());
    let grid = render_offscreen(&device, &queue, &mut r, &floor);
    dump(&grid, "overlay-grid");
    assert!(r.stats().grid);
    let on_floor = changed(&plain, &grid);
    assert!(on_floor > 500, "the grid shows on the floor: {on_floor}");
    // The same grid a millimetre above a floor sunk below the plane:
    // nearly the same pixels. Z-fighting would scatter them.
    let below = scene(-0.001);
    let plain_below = render_offscreen(&device, &queue, &mut r, &below);
    r.overlay().grid(Grid::default());
    let grid_below = render_offscreen(&device, &queue, &mut r, &below);
    let lines_on: Vec<bool> = plain.iter().zip(&grid).map(|(a, b)| a != b).collect();
    let lines_below: Vec<bool> = plain_below
        .iter()
        .zip(&grid_below)
        .map(|(a, b)| a != b)
        .collect();
    let disagree = lines_on
        .iter()
        .zip(&lines_below)
        .filter(|(a, b)| a != b)
        .count();
    assert!(
        disagree * 20 < on_floor,
        "{disagree} of {on_floor} pixels disagree"
    );
    // A floor 20 cm above the plane hides it.
    let above = scene(0.2);
    let plain_above = render_offscreen(&device, &queue, &mut r, &above);
    r.overlay().grid(Grid::default());
    let hidden = render_offscreen(&device, &queue, &mut r, &above);
    // (Past the floor's far edge nothing covers the plane, and the grid
    // rightly shows there: only the floor's own pixels are counted.)
    let through = plain_above
        .iter()
        .zip(&hidden)
        .filter(|(a, b)| !is_background(**a) && a != b)
        .count();
    assert_eq!(through, 0, "hidden under the floor");
    // It thins out toward the horizon: the bottom rows carry more of it
    // than the rows just under the horizon.
    let density = |rows: std::ops::Range<u32>| {
        let n = rows.len() as f32 * W as f32;
        rows.flat_map(|y| (0..W).map(move |x| (y * W + x) as usize))
            .filter(|&i| lines_on[i])
            .count() as f32
            / n
    };
    let horizon = H as f32 / 2.0 - (0.35f32.tan() / (30f32.to_radians().tan())) * H as f32 / 2.0;
    let near = density(H * 3 / 4..H);
    let far = density(horizon as u32 + 1..horizon as u32 + 6);
    assert!(near > far, "near {near} vs far {far}");
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

/// R3b: bloom, shaped by the volumes and capped by the quality profile.
/// A lit two-metre cube on the clear colour: the glow shows where there
/// is nothing but background, so what changes is the bloom and nothing
/// else.
#[test]
fn bloom_spreads_bright_light_past_its_edges_and_the_threshold_keeps_the_rest() {
    let Some((device, queue)) = device() else {
        return;
    };
    let lab = Lab::new("bloom");
    let glow = lab.volume(
        1,
        json!({ "bloom": { "intensity": 1.0, "threshold": 0.0 } }),
    );
    let high = lab.volume(
        2,
        json!({ "bloom": { "intensity": 1.0, "threshold": 20.0 } }),
    );
    let red = lab.volume(
        3,
        json!({ "bloom": { "intensity": 1.0, "threshold": 0.0, "tint": [1.0, 0.0, 0.0] } }),
    );
    let tight = lab.volume(
        4,
        json!({ "bloom": { "intensity": 1.0, "threshold": 0.0, "scatter": 0.0 } }),
    );
    let wide = lab.volume(
        5,
        json!({ "bloom": { "intensity": 1.0, "threshold": 0.0, "scatter": 1.0 } }),
    );
    let (assets, _) = lab.extract(Vec3::new(0.0, 0.3, 3.0), &[], false);

    let mut r = renderer(&device);
    r.set_assets(Some(assets.clone()));
    r.camera.eye = Vec3::new(0.0, 0.0, 7.0);
    r.camera.target = Vec3::ZERO;
    let mut base = Extract::default();
    base.records.push(record(
        0,
        Mat4::from_scale_rotation_translation(Vec3::splat(2.0), Quat::IDENTITY, Vec3::ZERO),
    ));
    base.environment = Some(environment(Vec3::ONE, 20_000.0));
    let with = |profile: &AssetRef| {
        let mut e = base.clone();
        e.volumes = vec![ExtractVolume {
            record: 9,
            world: Mat4::IDENTITY,
            volume: Volume {
                profile: profile.clone(),
                ..Volume::default()
            },
        }];
        e
    };
    let at = |world: Vec3| {
        let (x, y) = pixel_of(&r, world);
        (y.min(H - 1) * W + x.min(W - 1)) as usize
    };
    // Background a little past the cube's +X edge (x = 1 on the front
    // face), further out, and the image's corner.
    let near = at(Vec3::new(1.25, 0.0, 1.0));
    let far = at(Vec3::new(2.2, 0.0, 1.0));
    let corner = 0;

    let plain = render_offscreen(&device, &queue, &mut r, &base);
    assert!(!r.stats().bloom, "no volume, no bloom");
    assert!(is_background(plain[near]) && is_background(plain[far]));
    // Off is off to the bit: the profile without the chain renders the
    // same picture as the chain with no intensity.
    r.set_quality(Quality {
        bloom: false,
        ..Quality::default()
    });
    let quality_off = render_offscreen(&device, &queue, &mut r, &base);
    assert_eq!(plain, quality_off);
    // And it is the ceiling: a volume cannot turn it on.
    let capped = render_offscreen(&device, &queue, &mut r, &with(&glow));
    assert!(!r.stats().bloom);
    assert_eq!(capped, quality_off);
    r.set_quality(Quality::default());

    // With it, the light crosses the silhouette onto the background and
    // fades with distance; the image's far corner stays the clear colour
    // within a step or two.
    let bloomed = render_offscreen(&device, &queue, &mut r, &with(&glow));
    dump(&bloomed, "bloom");
    assert!(r.stats().bloom);
    assert_eq!(r.post_settings().bloom_intensity, 1.0);
    assert!(
        luminance(bloomed[near]) > luminance(plain[near]) + 8.0,
        "the glow reaches past the edge: {:?} vs {:?}",
        bloomed[near],
        plain[near]
    );
    assert!(
        luminance(bloomed[near]) > luminance(bloomed[far]),
        "and fades outward: {:?} vs {:?}",
        bloomed[near],
        bloomed[far]
    );
    assert!(
        (luminance(bloomed[corner]) - luminance(plain[corner])).abs() < 3.0,
        "the far corner is barely touched: {:?} vs {:?}",
        bloomed[corner],
        plain[corner]
    );

    // A threshold above everything in the frame leaves nothing to glow:
    // the stage runs and the picture is the plain one to the bit.
    let above = render_offscreen(&device, &queue, &mut r, &with(&high));
    assert!(r.stats().bloom);
    assert_eq!(above, plain, "nothing is brighter than the threshold");

    // The tint colours the glow: red only.
    let reddened = render_offscreen(&device, &queue, &mut r, &with(&red));
    let (p, c) = (reddened[near], plain[near]);
    assert!(p[0] > c[0] + 8, "red glows: {p:?} vs {c:?}");
    assert!(
        p[1].abs_diff(c[1]) <= 1 && p[2].abs_diff(c[2]) <= 1,
        "green and blue do not: {p:?} vs {c:?}"
    );

    // Scatter carries the glow further out.
    let tight_px = render_offscreen(&device, &queue, &mut r, &with(&tight));
    let wide_px = render_offscreen(&device, &queue, &mut r, &with(&wide));
    dump(&wide_px, "bloom-wide");
    assert!(
        luminance(wide_px[far]) > luminance(tight_px[far]) + 2.0,
        "a wide scatter reaches further: {:?} vs {:?}",
        wide_px[far],
        tight_px[far]
    );
}

/// R3c: colour grading, shaped by the volumes. An orange cube in the sun
/// on the clear colour: a neutral grade is no grade at all (to the bit),
/// each control moves the colour the way its name says, and the clear
/// colour is never graded.
#[test]
fn colour_grading_moves_what_the_scene_drew_and_a_neutral_grade_is_none() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut lab = Lab::new("grading");
    let orange = lab.material(json!({ "base_color": [0.8, 0.35, 0.1, 1.0], "roughness": 0.8 }));
    let grade = |n: u32, fields: Value| lab.volume(n, json!({ "colorGrading": fields }));
    let aces = grade(1, json!({ "tonemapper": "aces" }));
    let neutral_fields = grade(
        2,
        json!({ "postExposure": 0, "temperature": 0, "tint": 0, "contrast": 0, "saturation": 0,
                "hueShift": 0, "colorFilter": [1, 1, 1], "lift": [0, 0, 0], "gamma": [1, 1, 1], "gain": [1, 1, 1] }),
    );
    let grey = grade(3, json!({ "saturation": -100 }));
    let warm = grade(4, json!({ "temperature": 60 }));
    let cool = grade(5, json!({ "temperature": -60 }));
    let brighter = grade(6, json!({ "postExposure": 1 }));
    let lifted = grade(7, json!({ "lift": [0.3, 0.3, 0.3] }));
    let none = grade(8, json!({ "tonemapper": "none" }));
    let neutral_curve = grade(9, json!({ "tonemapper": "neutral" }));
    let punchy = grade(10, json!({ "contrast": 60 }));
    let turned = grade(11, json!({ "hueShift": 180 }));
    let (assets, base) = lab.extract(Vec3::new(0.0, 0.3, 3.0), &[(Vec3::ZERO, orange)], true);
    let mut r = renderer(&device);
    r.placeholders = false;
    r.set_assets(Some(assets));
    let with = |profile: &AssetRef| {
        let mut e = base.clone();
        e.volumes = vec![global_volume(profile, 0)];
        e
    };
    let mut render = |profile: Option<&AssetRef>| {
        let e = profile.map(with).unwrap_or_else(|| base.clone());
        let px = render_offscreen(&device, &queue, &mut r, &e);
        (px, r.stats().grading, r.post_settings().grading)
    };
    let mean = |px: &[[u8; 4]]| {
        [
            channel_mean(px, 0),
            channel_mean(px, 1),
            channel_mean(px, 2),
        ]
    };

    let (plain, graded, _) = render(None);
    dump(&plain, "grading-plain");
    assert!(!graded, "no volume, no grade");
    let background = plain
        .iter()
        .position(|p| is_background(*p))
        .expect("background in view");
    let [pr, pg, pb] = mean(&plain);
    assert!(pr > pg + 20.0 && pg > pb, "orange: {pr} {pg} {pb}");

    // Neutral is none, to the bit: ACES named, or every field at its
    // neutral value.
    for (profile, what) in [(&aces, "aces"), (&neutral_fields, "every field neutral")] {
        let (px, graded, _) = render(Some(profile));
        assert!(!graded, "{what}");
        assert_eq!(px, plain, "{what}");
    }

    // Saturation −100: grey.
    let (px, graded, g) = render(Some(&grey));
    dump(&px, "grading-grey");
    assert!(graded);
    assert_eq!(g.saturation, -100.0);
    let [r_, g_, b_] = mean(&px);
    assert!(
        (r_ - g_).abs() < 2.0 && (g_ - b_).abs() < 2.0,
        "grey: {r_} {g_} {b_}"
    );

    // Warmer: redder against blue; cooler the other way.
    let (warm_px, ..) = render(Some(&warm));
    let (cool_px, ..) = render(Some(&cool));
    dump(&warm_px, "grading-warm");
    let [wr, _, wb] = mean(&warm_px);
    let [cr, _, cb] = mean(&cool_px);
    assert!(wr - wb > cr - cb + 10.0, "warm {wr}/{wb} vs cool {cr}/{cb}");

    // A stop of post-exposure brightens.
    let (px, ..) = render(Some(&brighter));
    assert!(drawn_mean(&px).0 > drawn_mean(&plain).0 + 10.0);

    // Contrast spreads the tones: the brightest channel brighter, the
    // darkest darker.
    let (px, ..) = render(Some(&punchy));
    let [kr, _, kb] = mean(&px);
    assert!(kr > pr && kb < pb, "contrast: {kr}/{kb} vs {pr}/{pb}");

    // Half-way round the wheel: orange turns blue-ish.
    let (px, ..) = render(Some(&turned));
    let [tr, _, tb] = mean(&px);
    assert!(tb > tr, "turned: {tr} {tb}");

    // Lift raises the darks of what the scene drew — and never the clear
    // colour.
    let (px, ..) = render(Some(&lifted));
    dump(&px, "grading-lift");
    assert!(
        mean(&px)[2] > pb + 20.0,
        "lifted blue: {} vs {pb}",
        mean(&px)[2]
    );
    assert_eq!(
        px[background], plain[background],
        "the clear colour is not graded"
    );

    // The tone mappers differ, and choosing one is not a grade.
    let (none_px, graded, g) = render(Some(&none));
    assert!(!graded);
    assert_eq!(g.tonemapper, fynkel_core::volume::Tonemapper::None);
    let (neutral_px, ..) = render(Some(&neutral_curve));
    dump(&neutral_px, "grading-neutral");
    assert_ne!(none_px, plain);
    assert_ne!(neutral_px, plain);
    assert_ne!(neutral_px, none_px);
}

// ------------------------------------------ the selection outline (ADR-0019)

/// A linear colour as the Unorm target stores it (the overlay encodes
/// sRGB itself).
fn srgb8(c: [f32; 4]) -> [u8; 3] {
    let e = |v: f32| {
        let v = v.clamp(0.0, 1.0);
        let s = if v <= 0.003_130_8 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        };
        (s * 255.0).round() as u8
    };
    [e(c[0]), e(c[1]), e(c[2])]
}

fn near(p: [u8; 4], c: [u8; 3]) -> bool {
    (0..3).all(|i| (p[i] as i32 - c[i] as i32).abs() <= 2)
}

/// Whether some pixel within `r` of `(x, y)` is set in `cover`.
fn within(cover: &[bool], x: i32, y: i32, r: i32) -> bool {
    (-r..=r).any(|dy| {
        (-r..=r).any(|dx| {
            let (u, v) = (x + dx, y + dy);
            u >= 0 && v >= 0 && u < W as i32 && v < H as i32 && cover[(v * W as i32 + u) as usize]
        })
    })
}

/// Pixels whose whole `r` neighbourhood is set in `cover`.
fn deep_inside(cover: &[bool], r: i32) -> Vec<usize> {
    (0..(W * H) as usize)
        .filter(|&i| {
            let (x, y) = ((i as u32 % W) as i32, (i as u32 / W) as i32);
            (-r..=r).all(|dy| {
                (-r..=r).all(|dx| {
                    let (u, v) = (x + dx, y + dy);
                    u >= 0
                        && v >= 0
                        && u < W as i32
                        && v < H as i32
                        && cover[(v * W as i32 + u) as usize]
                })
            })
        })
        .collect()
}

/// The placeholder cube at the origin in `cube_scene` (record 1).
const CUBE: Entity = Entity::from_bits(2);

#[test]
fn a_selected_cube_is_ringed_just_outside_its_silhouette_and_untouched_inside() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut e = cube_scene(Vec3::new(0.0, 0.0, 4.0), Camera::default());
    e.records[1].world = Mat4::from_rotation_y(0.4);
    let orange = srgb8(OutlineStyle::default().selected);
    for msaa in [1, 4] {
        let mut r = renderer(&device);
        r.set_quality(Quality {
            msaa,
            ..Quality::default()
        });
        let plain = render_offscreen(&device, &queue, &mut r, &e);
        assert_eq!(r.stats().outlined, 0);
        r.overlay().outline(&[CUBE], &[], OutlineStyle::default());
        let ringed = render_offscreen(&device, &queue, &mut r, &e);
        dump(&ringed, &format!("outline-msaa{msaa}"));
        assert_eq!(r.stats().outlined, 1);
        let cover: Vec<bool> = plain.iter().map(|p| !is_background(*p)).collect();
        // Inside: the cube as it was.
        let inside = deep_inside(&cover, 1);
        assert!(inside.len() > 500, "{}", inside.len());
        for &i in &inside {
            assert_eq!(ringed[i], plain[i], "pixel {i} inside the cube");
        }
        // Outside: a ring hugging the silhouette, solid where it is
        // closest and nowhere further than width + 1.
        let mut solid = 0;
        for (i, (p, q)) in ringed.iter().zip(&plain).enumerate() {
            if p == q {
                continue;
            }
            let (x, y) = ((i as u32 % W) as i32, (i as u32 / W) as i32);
            assert!(within(&cover, x, y, 3), "{x},{y} is far from the cube");
            if near(*p, orange) {
                solid += 1;
            }
        }
        assert!(solid > 100, "msaa {msaa}: {solid} solid outline pixels");
        // Per frame, like the rest of the overlay.
        assert_eq!(render_offscreen(&device, &queue, &mut r, &e), plain);
        assert_eq!(r.stats().outlined, 0);
    }

    // At one sample the mask is the scene's own coverage: the pixel next
    // to the silhouette is the full colour, two away half of it, three
    // away untouched.
    let mut r = renderer(&device);
    r.set_quality(Quality {
        msaa: 1,
        ..Quality::default()
    });
    let e = cube_scene(Vec3::new(0.0, 0.0, 4.0), Camera::default());
    let plain = render_offscreen(&device, &queue, &mut r, &e);
    r.overlay().outline(&[CUBE], &[], OutlineStyle::default());
    let ringed = render_offscreen(&device, &queue, &mut r, &e);
    let row = (H / 2 * W) as usize;
    let left = (0..W as usize)
        .find(|&x| !is_background(plain[row + x]))
        .unwrap();
    assert!(
        near(ringed[row + left - 1], orange),
        "{:?}",
        ringed[row + left - 1]
    );
    let half = ringed[row + left - 2];
    assert!(
        half != plain[row + left - 2] && !near(half, orange),
        "{half:?}"
    );
    assert_eq!(ringed[row + left - 3], plain[row + left - 3]);
}

#[test]
fn children_are_ringed_in_their_own_colour_and_selected_wins() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut r = renderer(&device);
    r.set_quality(Quality {
        msaa: 1,
        ..Quality::default()
    });
    // Two cubes side by side, touching on screen.
    let mut e = cube_scene(Vec3::new(0.0, 0.0, 4.0), Camera::default());
    e.records[1].world = Mat4::from_translation(Vec3::new(-0.5, 0.0, 0.0));
    e.records
        .push(record(2, Mat4::from_translation(Vec3::new(0.5, 0.0, 0.0))));
    let (a, b) = (CUBE, Entity::from_bits(3));
    let style = OutlineStyle::default();
    let (orange, blue) = (srgb8(style.selected), srgb8(style.children));
    let plain = render_offscreen(&device, &queue, &mut r, &e);
    let row = (H / 2 * W) as usize;
    let left = (0..W as usize)
        .find(|&x| !is_background(plain[row + x]))
        .unwrap();
    let right = (0..W as usize)
        .rev()
        .find(|&x| !is_background(plain[row + x]))
        .unwrap();
    let mut colours = |selected: &[Entity], children: &[Entity]| {
        r.overlay().outline(selected, children, style);
        let px = render_offscreen(&device, &queue, &mut r, &e);
        (px[row + left - 1], px[row + right + 1])
    };
    // `a` selected, `b` its child.
    let (l, rr) = colours(&[a], &[b]);
    assert!(near(l, orange) && near(rr, blue), "{l:?} {rr:?}");
    // Both children.
    let (l, rr) = colours(&[], &[a, b]);
    assert!(near(l, blue) && near(rr, blue), "{l:?} {rr:?}");
    // In both lists: selected.
    let (l, rr) = colours(&[b], &[a, b]);
    assert!(near(l, blue) && near(rr, orange), "{l:?} {rr:?}");
    // Just above the seam between the two, the selected one's ring wins
    // over its child's.
    let top = (0..H as usize)
        .find(|&y| !is_background(plain[y * W as usize + W as usize / 2 - 1]))
        .unwrap();
    r.overlay().outline(&[a], &[b], style);
    let px = render_offscreen(&device, &queue, &mut r, &e);
    let above = px[(top - 1) * W as usize + W as usize / 2 - 1];
    assert!(near(above, orange), "{above:?}");
}

#[test]
fn the_outline_rings_what_other_geometry_hides() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut r = renderer(&device);
    // A big cube in front of the selected one hides it completely.
    let mut e = cube_scene(Vec3::new(0.0, 0.0, 4.0), Camera::default());
    e.records.push(record(
        2,
        Mat4::from_scale_rotation_translation(Vec3::splat(1.5), Quat::IDENTITY, Vec3::Z * 1.5),
    ));
    let plain = render_offscreen(&device, &queue, &mut r, &e);
    r.overlay().outline(&[CUBE], &[], OutlineStyle::default());
    let ringed = render_offscreen(&device, &queue, &mut r, &e);
    dump(&ringed, "outline-occluded");
    let orange = srgb8(OutlineStyle::default().selected);
    let over = ringed
        .iter()
        .zip(&plain)
        .filter(|(p, q)| !is_background(**q) && near(**p, orange))
        .count();
    assert!(over > 100, "{over} outline pixels over the occluder");
    // The occluder's own silhouette is not ringed: every change is on it.
    assert!(ringed
        .iter()
        .zip(&plain)
        .all(|(p, q)| p == q || !is_background(*q)));
}

#[test]
fn the_outline_width_is_in_target_pixels() {
    let Some((device, queue)) = device() else {
        return;
    };
    let e = cube_scene(Vec3::new(0.0, 0.0, 4.0), Camera::default());
    let ring = |r: &mut ExtractRenderer, width: f32| {
        let plain = render_offscreen(&device, &queue, r, &e);
        r.overlay().outline(
            &[CUBE],
            &[],
            OutlineStyle {
                width,
                ..OutlineStyle::default()
            },
        );
        let px = render_offscreen(&device, &queue, r, &e);
        (changed(&plain, &px), px)
    };
    let mut r = renderer(&device);
    let (thin, _) = ring(&mut r, 2.0);
    let (thick, px) = ring(&mut r, 6.0);
    dump(&px, "outline-wide");
    assert!(thick > thin * 2, "{thick} vs {thin}");
    // No width, no outline — and no outline passes.
    let (none, _) = ring(&mut r, 0.0);
    assert_eq!((none, r.stats().outlined), (0, 0));

    // The render scale resamples the scene, never the outline: its ring
    // is as wide at half and at double the internal size.
    let orange = srgb8(OutlineStyle::default().selected);
    for scale in [0.5, 2.0] {
        r.set_quality(Quality {
            render_scale: scale,
            ..Quality::default()
        });
        let (n, px) = ring(&mut r, 2.0);
        dump(&px, &format!("outline-scale-{scale}"));
        let ratio = n as f32 / thin as f32;
        assert!(
            (0.75..=1.35).contains(&ratio),
            "scale {scale}: {n} vs {thin}"
        );
        let solid = px.iter().filter(|p| near(**p, orange)).count();
        assert!(solid > 100, "scale {scale}: {solid}");
    }
}

#[test]
fn without_an_outline_nothing_changes() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut e = cube_scene(Vec3::new(0.0, 0.0, 4.0), Camera::default());
    e.directional_lights
        .push(sun(Vec3::new(-0.3, -1.0, -0.5), 10_000.0));
    // A renderer that never outlined anything is the baseline.
    let mut fresh = renderer(&device);
    let baseline = render_offscreen(&device, &queue, &mut fresh, &e);
    let mut r = renderer(&device);
    r.overlay().outline(&[CUBE], &[], OutlineStyle::default());
    render_offscreen(&device, &queue, &mut r, &e);
    assert_eq!(render_offscreen(&device, &queue, &mut r, &e), baseline);
    // Nothing named, an entity the extract lacks, one on a layer the
    // camera leaves out: no outline, the same pixels.
    let absent = Entity::from_bits(99);
    r.overlay().outline(&[], &[], OutlineStyle::default());
    assert_eq!(render_offscreen(&device, &queue, &mut r, &e), baseline);
    assert_eq!(r.stats().outlined, 0);
    r.overlay()
        .outline(&[absent], &[absent], OutlineStyle::default());
    assert_eq!(render_offscreen(&device, &queue, &mut r, &e), baseline);
    assert_eq!(r.stats().outlined, 0);
    let mut hidden = e.clone();
    hidden.records[1].layer_bit = 2;
    if let Some(c) = hidden.camera.as_mut() {
        c.camera.culling_mask = !2;
    }
    let hidden_plain = render_offscreen(&device, &queue, &mut r, &hidden);
    r.overlay().outline(&[CUBE], &[], OutlineStyle::default());
    assert_eq!(
        render_offscreen(&device, &queue, &mut r, &hidden),
        hidden_plain
    );
    assert_eq!(r.stats().outlined, 0);
}

#[test]
fn blended_meshes_are_outlined_by_their_whole_surface() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut lab = Lab::new("outline-blend");
    let glass = lab.material(json!({
        "base_color": [1.0, 0.0, 0.0, 0.3], "alpha_mode": "blend", "roughness": 1.0 }));
    let (assets, extract) = lab.extract(Vec3::new(0.0, 0.0, 3.0), &[(Vec3::ZERO, glass)], false);
    let (plain, mut r) = render_lab(&device, &queue, &assets, &extract);
    assert_eq!(r.stats().transparent, 1);
    let entity = extract.records[extract.meshes[0].record as usize].entity;
    r.overlay().outline(&[entity], &[], OutlineStyle::default());
    let ringed = render_offscreen(&device, &queue, &mut r, &extract);
    dump(&ringed, "outline-blend");
    assert_eq!(r.stats().outlined, 1);
    let orange = srgb8(OutlineStyle::default().selected);
    let cover: Vec<bool> = plain.iter().map(|p| !is_background(*p)).collect();
    let ring = ringed
        .iter()
        .zip(&plain)
        .filter(|(p, q)| p != q && near(**p, orange))
        .count();
    assert!(ring > 100, "{ring}");
    for i in deep_inside(&cover, 1) {
        assert_eq!(ringed[i], plain[i]);
    }
}

// ------------------ extra worlds (Prefab Mode in context, multi-scene)

/// A host-camera scene eight metres back: a cube at the origin, lit by a
/// sun.
fn lit_cube() -> Extract {
    let mut e = Extract::default();
    e.records.push(record(1, Mat4::IDENTITY));
    e.directional_lights
        .push(sun(Vec3::new(-0.3, -1.0, -0.5), 10_000.0));
    e
}

fn extra_renderer(device: &wgpu::Device) -> ExtractRenderer {
    let mut r = renderer(device);
    r.camera.eye = Vec3::new(0.0, 0.0, 8.0);
    r
}

fn one_cube(slot: u32, world: Mat4) -> Extract {
    let mut e = Extract::default();
    e.records.push(record(slot, world));
    e
}

fn at(p: (u32, u32)) -> usize {
    (p.1 * W + p.0) as usize
}

/// How far a pixel is from grey: its largest channel minus its smallest.
fn spread(p: [u8; 4]) -> f32 {
    let (hi, lo) = (p[0].max(p[1]).max(p[2]), p[0].min(p[1]).min(p[2]));
    (hi - lo) as f32
}

#[test]
fn an_extra_world_is_drawn_through_its_transform_for_one_frame() {
    let Some((device, queue)) = device() else {
        return;
    };
    let main = lit_cube();
    let baseline = render_offscreen(&device, &queue, &mut extra_renderer(&device), &main);
    let mut r = extra_renderer(&device);
    let (origin, right) = (
        pixel_of(&r, Vec3::ZERO),
        pixel_of(&r, Vec3::new(2.5, 0.0, 0.0)),
    );
    assert!(is_background(baseline[at(right)]));

    // A cube half a metre off its world's origin, turned and grown; the
    // transform puts that world 2 m to +X. The frame is the one where
    // the main world holds the cube where the transform puts it.
    let local = Mat4::from_scale_rotation_translation(
        Vec3::splat(1.2),
        Quat::from_rotation_y(0.5),
        Vec3::new(0.5, 0.0, 0.0),
    );
    let transform = Mat4::from_translation(Vec3::new(2.0, 0.0, 0.0));
    r.push_extra_world(ExtraWorld {
        extract: one_cube(4, local),
        transform,
        gray: false,
    });
    assert_eq!(r.extra_worlds().len(), 1);
    let px = render_offscreen(&device, &queue, &mut r, &main);
    dump(&px, "extra-world");
    assert!(r.extra_worlds().is_empty(), "the render consumed the queue");
    let s = r.stats();
    assert_eq!((s.extra_worlds, s.instances), (1, 2));
    assert!(!is_background(px[at(right)]));
    assert!(
        luminance(px[at(right)]) > 40.0,
        "lit by the main world's sun"
    );
    assert_eq!(px[at(origin)], baseline[at(origin)]);
    let mut merged = main.clone();
    merged.records.push(record(4, transform * local));
    assert_eq!(
        px,
        render_offscreen(&device, &queue, &mut extra_renderer(&device), &merged)
    );

    // Not queued again: the next frame is the main world alone, bit for
    // bit.
    assert_eq!(render_offscreen(&device, &queue, &mut r, &main), baseline);
    assert_eq!(r.stats().extra_worlds, 0);
    // An empty extra world (grey or not) changes nothing; a cleared queue
    // draws none.
    r.push_extra_world(ExtraWorld {
        extract: Extract::default(),
        transform: Mat4::IDENTITY,
        gray: true,
    });
    assert_eq!(render_offscreen(&device, &queue, &mut r, &main), baseline);
    assert_eq!(r.stats().extra_worlds, 1);
    r.push_extra_world(ExtraWorld {
        extract: one_cube(4, Mat4::IDENTITY),
        transform,
        gray: false,
    });
    r.clear_extra_worlds();
    assert_eq!(render_offscreen(&device, &queue, &mut r, &main), baseline);
}

#[test]
fn a_grey_extra_world_is_desaturated_and_dimmed() {
    let Some((device, queue)) = device() else {
        return;
    };
    let main = lit_cube();
    let mut r = extra_renderer(&device);
    let baseline = render_offscreen(&device, &queue, &mut r, &main);
    let place = Mat4::from_translation(Vec3::new(2.5, 0.0, 0.0));
    let mut frame = |gray: bool| {
        r.push_extra_world(ExtraWorld {
            // Slot 5: the placeholder palette's red.
            extract: one_cube(5, Mat4::IDENTITY),
            transform: place,
            gray,
        });
        render_offscreen(&device, &queue, &mut r, &main)
    };
    let (normal, grey) = (frame(false), frame(true));
    dump(&grey, "extra-world-grey");
    // The extra cube's texels, well inside its silhouette.
    let cover: Vec<bool> = normal.iter().zip(&baseline).map(|(p, b)| p != b).collect();
    let inside = deep_inside(&cover, 2);
    assert!(inside.len() > 100, "{} texels", inside.len());
    let mean = |px: &[[u8; 4]], f: fn([u8; 4]) -> f32| {
        inside.iter().map(|&i| f(px[i])).sum::<f32>() / inside.len() as f32
    };
    let (lum_normal, lum_grey) = (mean(&normal, luminance), mean(&grey, luminance));
    let (sat_normal, sat_grey) = (mean(&normal, spread), mean(&grey, spread));
    assert!(sat_normal > 60.0, "the red cube: spread {sat_normal}");
    assert!(sat_grey < 1.0, "grey: spread {sat_grey}");
    assert!(
        lum_grey < lum_normal * 0.8 && lum_grey > 5.0,
        "grey {lum_grey} vs normal {lum_normal}"
    );
    // The main world is not greyed.
    for i in (0..baseline.len()).filter(|&i| !cover[i]) {
        assert_eq!(grey[i], baseline[i]);
    }
}

#[test]
fn an_extra_worlds_lights_shine_where_its_transform_puts_them() {
    let Some((device, queue)) = device() else {
        return;
    };
    // No sun: the main world lights its cube with ambient alone.
    let mut main = Extract::default();
    main.records.push(record(1, Mat4::IDENTITY));
    main.environment = Some(environment(Vec3::ONE, 50.0));
    let mut r = extra_renderer(&device);
    let baseline = render_offscreen(&device, &queue, &mut r, &main);
    let face = at(pixel_of(&r, Vec3::new(0.0, 0.0, 0.5)));
    // A world holding nothing but a point light at its origin.
    let mut lamp = Extract::default();
    lamp.point_lights.push(ExtractPointLight {
        record: 0,
        position: Vec3::ZERO,
        light: PointLight {
            color: Vec3::ONE,
            intensity: 400.0,
            range: 10.0,
            culling_mask: u32::MAX,
        },
    });
    let mut lit_from = |z: f32| {
        r.push_extra_world(ExtraWorld {
            extract: lamp.clone(),
            transform: Mat4::from_translation(Vec3::new(0.0, 0.0, z)),
            gray: false,
        });
        let px = render_offscreen(&device, &queue, &mut r, &main);
        (px, r.stats())
    };
    // In front of the face the camera sees: brighter.
    let (front, s) = lit_from(2.0);
    assert_eq!((s.lights, s.extra_worlds, s.instances), (1, 1, 1));
    assert!(
        luminance(front[face]) > luminance(baseline[face]) + 30.0,
        "{:?} vs {:?}",
        front[face],
        baseline[face]
    );
    // Behind the cube: the face is as it was.
    let (behind, s) = lit_from(-2.0);
    assert_eq!(s.lights, 1);
    assert_eq!(behind[face], baseline[face]);
}

#[test]
fn extra_worlds_cast_and_receive_the_suns_shadow() {
    let Some((device, queue)) = device() else {
        return;
    };
    // The shadow test's slab and cube, split across two worlds.
    let slab = Mat4::from_scale_rotation_translation(
        Vec3::new(10.0, 0.2, 10.0),
        Quat::IDENTITY,
        Vec3::new(0.0, -0.6, 0.0),
    );
    let mut caster = sun(Vec3::new(0.6, -1.0, 0.0), 10_000.0);
    caster.light.cast_shadows = true;
    let mut r = renderer(&device);
    r.camera.eye = Vec3::new(0.0, 8.0, 6.0);
    let shadowed_at = at(pixel_of(&r, Vec3::new(0.8, -0.5, 0.0)));
    let lit_at = at(pixel_of(&r, Vec3::new(-2.0, -0.5, 0.0)));

    // The main world's slab, the cube in an extra world: its shadow
    // falls on the slab.
    let mut floor = one_cube(0, slab);
    floor.directional_lights.push(caster);
    let alone = render_offscreen(&device, &queue, &mut r, &floor);
    assert_eq!(r.stats().shadow_casters, 1);
    r.push_extra_world(ExtraWorld {
        extract: one_cube(1, Mat4::IDENTITY),
        transform: Mat4::IDENTITY,
        gray: false,
    });
    let with = render_offscreen(&device, &queue, &mut r, &floor);
    assert_eq!(r.stats().shadow_casters, 2);
    assert!(
        luminance(with[shadowed_at]) < luminance(alone[shadowed_at]) * 0.7,
        "{:?} vs {:?}",
        with[shadowed_at],
        alone[shadowed_at]
    );

    // The other way round, the slab grey in an extra world with the sun
    // (the main world has none): the main cube's shadow falls on it.
    let mut cube = one_cube(1, Mat4::IDENTITY);
    cube.directional_lights.clear();
    let mut grey_floor = one_cube(0, slab);
    grey_floor.directional_lights.push(caster);
    r.push_extra_world(ExtraWorld {
        extract: grey_floor,
        transform: Mat4::IDENTITY,
        gray: true,
    });
    let px = render_offscreen(&device, &queue, &mut r, &cube);
    dump(&px, "extra-world-shadow");
    let s = r.stats();
    assert!(s.scene_light, "the extra world's sun");
    assert_eq!((s.shadow_casters, s.extra_worlds), (2, 1));
    assert!(
        luminance(px[shadowed_at]) < luminance(px[lit_at]) * 0.7,
        "{:?} vs {:?}",
        px[shadowed_at],
        px[lit_at]
    );
    assert!(spread(px[lit_at]) < 1.0, "the slab is grey");
}
