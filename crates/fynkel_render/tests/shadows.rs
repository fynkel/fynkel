//! Stage 8 phase 3: shadows. The sun's (cascaded) and spot lights' fall
//! where something stands between them and a surface; a draw can choose
//! not to cast them, or not to receive them; and each level draws its own
//! number of maps. That a still shadow doesn't shimmer as the camera moves
//! is checked on the maps' maths, in `shadows.rs`.

mod common;

use common::*;
use fynkel_render::glam::{Mat4, Vec3};
use fynkel_render::*;

const SRGB: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const SIZE: u32 = 128;

fn material(grey: f32) -> PbrMaterial {
    let g = linear(grey);
    PbrMaterial { base_color: [g, g, g, 1.0], metallic: 0.0, roughness: 1.0, ..PbrMaterial::DEFAULT }
}

/// A 12 m floor, and a 1 m cube whose bottom is 1 m above it, at the
/// origin; seen from above and in front.
fn stage(lights: Vec<Light>, quality: Quality) -> RenderScene {
    RenderScene {
        camera: Some(CameraView::looking_at(Vec3::new(0.0, 7.0, 5.0), Vec3::ZERO)),
        lights,
        quality: Some(quality),
        ambient: Vec3::splat(0.1),
        lines: Vec::new(),
        draws: vec![
            Draw::new(PLANE, Mat4::from_scale(Vec3::splat(12.0)), material(0.7)),
            Draw::new(CUBE, Mat4::from_translation(Vec3::new(0.0, 1.5, 0.0)), material(0.5)),
        ],
    }
}

/// The sun, from up and to the left (-x): the cube's shadow falls about
/// 0.75 m to its right.
fn sun() -> Light {
    Light::sun(Vec3::new(1.0, -2.0, 0.0), Vec3::ONE).with_shadows(true)
}

/// Where `p` shows in a `SIZE`² frame of `scene`'s camera.
fn pixel_of(scene: &RenderScene, p: Vec3) -> (u32, u32) {
    let camera = scene.camera.expect("a camera");
    let projection =
        fynkel_render::glam::camera::rh::proj::directx::perspective(camera.fov_y, 1.0, camera.far, camera.near);
    let clip = (projection * camera.view).project_point3(p);
    (((clip.x * 0.5 + 0.5) * SIZE as f32) as u32, ((0.5 - clip.y * 0.5) * SIZE as f32) as u32)
}

fn brightness(image: &Image, (x, y): (u32, u32)) -> u32 {
    let [r, g, b, _] = image.pixel(x, y);
    u32::from(r) + u32::from(g) + u32::from(b)
}

/// How much darker `p` is in `scene` than with no light casting shadows:
/// 1 for no change, less where a shadow falls.
fn shade_at(gpu: &Gpu, scene: &RenderScene, p: Vec3) -> (f32, FrameStats) {
    let mut r = renderer(gpu, SRGB);
    let (shadowed, stats) = render(gpu, &mut r, scene, SIZE, SIZE);
    let mut plain = scene.clone();
    plain.lights.iter_mut().for_each(|l| l.shadows = false);
    let (lit, _) = render(gpu, &mut r, &plain, SIZE, SIZE);
    let at = pixel_of(scene, p);
    (brightness(&shadowed, at) as f32 / brightness(&lit, at).max(1) as f32, stats)
}

const IN_THE_SUNS_SHADOW: Vec3 = Vec3::new(0.75, 0.0, 0.0);
const IN_THE_OPEN: Vec3 = Vec3::new(-2.0, 0.0, 1.5);

#[test]
fn the_sun_casts_shadows_at_every_level() {
    let gpu = gpu();
    for quality in [Quality::Low, Quality::Medium, Quality::High] {
        let scene = stage(vec![sun()], quality);
        let (shade, stats) = shade_at(&gpu, &scene, IN_THE_SUNS_SHADOW);
        assert!(shade < 0.5, "{quality:?}: under the cube, the floor is in shadow ({shade})");
        assert_eq!(stats.shadow_maps as usize, quality.shadows().cascades, "{quality:?}: a map per cascade");
        assert!(stats.shadow_casters >= 2, "{quality:?}: the floor and the cube cast ({stats:?})");
        let (open, _) = shade_at(&gpu, &scene, IN_THE_OPEN);
        assert!(open > 0.97, "{quality:?}: out in the open, it's lit as without shadows ({open})");
    }
}

#[test]
fn a_draw_can_choose_not_to_cast_or_receive_shadows() {
    let gpu = gpu();
    let mut scene = stage(vec![sun()], Quality::Medium);
    scene.draws[1].cast_shadows = false;
    let (shade, stats) = shade_at(&gpu, &scene, IN_THE_SUNS_SHADOW);
    assert!(shade > 0.97, "the cube casts none ({shade})");
    assert_eq!(stats.shadow_casters, 2, "only the floor casts, into each cascade: {stats:?}");

    let mut scene = stage(vec![sun()], Quality::Medium);
    scene.draws[0].receive_shadows = false;
    let (shade, _) = shade_at(&gpu, &scene, IN_THE_SUNS_SHADOW);
    assert!(shade > 0.97, "the floor takes none ({shade})");
}

#[test]
fn a_spot_light_casts_shadows() {
    let gpu = gpu();
    // Up to the left, aimed past the cube: its shadow falls about 0.9 m to
    // the cube's right.
    let from = Vec3::new(-1.5, 4.0, 0.0);
    let torch = Light::spot(from, Vec3::new(0.5, 0.0, 0.0) - from, 12.0, 1.4, 0.2, Vec3::splat(4.0));
    let scene = stage(vec![torch.with_shadows(true)], Quality::Medium);
    let (shade, stats) = shade_at(&gpu, &scene, Vec3::new(0.9, 0.0, 0.0));
    assert!(shade < 0.5, "the cube's shadow ({shade})");
    assert_eq!(stats.shadow_maps, 1);
    let (open, _) = shade_at(&gpu, &scene, Vec3::new(-1.0, 0.0, 1.0));
    assert!(open > 0.97, "lit where nothing blocks it ({open})");
}

#[test]
fn each_level_draws_its_own_number_of_maps() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    // Six spot lights with shadows, and a lamp, whose shadows are for a
    // later stage.
    let mut lights: Vec<Light> = (0..6)
        .map(|i| {
            Light::spot(Vec3::new(i as f32 - 3.0, 4.0, 0.0), Vec3::NEG_Y, 8.0, 1.0, 0.2, Vec3::ONE).with_shadows(true)
        })
        .collect();
    lights.push(Light::point(Vec3::new(0.0, 3.0, 2.0), 5.0, Vec3::ONE).with_shadows(true));
    lights.push(sun());
    for quality in [Quality::Low, Quality::Medium, Quality::High] {
        let level = quality.shadows();
        let (_, stats) = render(&gpu, &mut r, &stage(lights.clone(), quality), SIZE, SIZE);
        assert_eq!(stats.shadow_maps as usize, level.cascades + level.spot_maps, "{quality:?}");
    }
    // No light asks for them: no maps.
    lights.iter_mut().for_each(|l| l.shadows = false);
    let (_, stats) = render(&gpu, &mut r, &stage(lights, Quality::High), SIZE, SIZE);
    assert_eq!((stats.shadow_maps, stats.shadow_casters), (0, 0));
}
