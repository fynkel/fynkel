//! Light ambient occlusion (stage 8 phase 6), checked on the real GPU:
//! where surfaces meet, the ambient light darkens, at high only; open
//! surfaces, and light from lights, stay as they are.

mod common;

use common::*;
use fynkel_render::glam::{Mat4, Vec3};
use fynkel_render::*;

const SRGB: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const SIZE: u32 = 128;

fn grey() -> PbrMaterial {
    PbrMaterial { base_color: [0.5, 0.5, 0.5, 1.0], roughness: 1.0, ..PbrMaterial::DEFAULT }
}

/// A floor, with a box 1 m across standing on it if `boxed`, seen from
/// above and in front, lit by `ambient` and `lights` alone.
fn floor_scene(
    boxed: bool,
    ao: Option<AmbientOcclusion>,
    quality: QualityLevel,
    ambient: f32,
    lights: Vec<Light>,
) -> RenderScene {
    let mut camera = CameraView::looking_at(Vec3::new(0.0, 3.0, 3.0), Vec3::new(0.0, 0.0, 0.0));
    camera.ambient_occlusion = ao;
    let mut draws = vec![Draw::new(PLANE, Mat4::from_scale(Vec3::splat(20.0)), grey())];
    if boxed {
        draws.push(Draw::new(CUBE, Mat4::from_translation(Vec3::new(0.0, 0.5, 0.0)), grey()));
    }
    RenderScene {
        camera: Some(camera),
        quality: Some(quality.quality()),
        time: [0.0; 2],
        ambient: Vec3::splat(ambient),
        lights,
        draws,
        ..Default::default()
    }
}

fn max_difference(a: &Image, b: &Image) -> u8 {
    a.rgba.iter().zip(&b.rgba).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0)
}

/// The floor just in front of the box's foot, and as far from the camera
/// but well to the side.
fn at_the_foot(image: &Image) -> (u8, u8) {
    let y = foot_row();
    (image.pixel(SIZE / 2, y + 2)[0], image.pixel(8, y + 2)[0])
}

/// The row of the view the foot's middle, (0, 0, 0.5), is in.
fn foot_row() -> u32 {
    let camera = CameraView::looking_at(Vec3::new(0.0, 3.0, 3.0), Vec3::ZERO);
    let projection =
        fynkel_render::glam::camera::rh::proj::directx::perspective(camera.fov_y, 1.0, camera.near, camera.far);
    let p = (projection * camera.view).project_point3(Vec3::new(0.0, 0.0, 0.5));
    ((1.0 - p.y) / 2.0 * SIZE as f32) as u32
}

#[test]
fn the_floor_darkens_where_a_box_stands_on_it_at_high() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let ao = Some(AmbientOcclusion::DEFAULT);
    let plain = render(&gpu, &mut r, &floor_scene(true, None, QualityLevel::High, 1.0, vec![]), SIZE, SIZE).0;
    let (foot, far) = at_the_foot(&plain);
    assert_eq!(foot, far, "without it, the floor is even");

    let shaded = render(&gpu, &mut r, &floor_scene(true, ao, QualityLevel::High, 1.0, vec![]), SIZE, SIZE).0;
    let (foot, far) = at_the_foot(&shaded);
    assert!(foot + 10 < far, "darker at the box's foot: {foot} against {far}");
    assert!(far.abs_diff(at_the_foot(&plain).1) <= 2, "and as it was away from it");

    let stronger = Some(AmbientOcclusion { intensity: 3.0, ..AmbientOcclusion::DEFAULT });
    let darker = render(&gpu, &mut r, &floor_scene(true, stronger, QualityLevel::High, 1.0, vec![]), SIZE, SIZE).0;
    assert!(at_the_foot(&darker).0 < foot, "more intensity, darker");
}

#[test]
fn nothing_darkens_below_high() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    for quality in [QualityLevel::Low, QualityLevel::Medium] {
        let plain = render(&gpu, &mut r, &floor_scene(true, None, quality, 1.0, vec![]), SIZE, SIZE).0;
        let ao = Some(AmbientOcclusion::DEFAULT);
        let shaded = render(&gpu, &mut r, &floor_scene(true, ao, quality, 1.0, vec![]), SIZE, SIZE).0;
        assert_eq!(max_difference(&plain, &shaded), 0, "{quality:?}");
    }
}

/// A floor with nothing on it is open: it doesn't darken itself.
#[test]
fn an_open_floor_stays_as_it_is() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let plain = render(&gpu, &mut r, &floor_scene(false, None, QualityLevel::High, 1.0, vec![]), SIZE, SIZE).0;
    let ao = Some(AmbientOcclusion::DEFAULT);
    let shaded = render(&gpu, &mut r, &floor_scene(false, ao, QualityLevel::High, 1.0, vec![]), SIZE, SIZE).0;
    assert!(max_difference(&plain, &shaded) <= 2, "{}", max_difference(&plain, &shaded));
}

/// Light from a sun isn't darkened, only the ambient.
#[test]
fn light_from_lights_is_left_as_it_is() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let sun = || vec![Light::sun(Vec3::new(0.0, -1.0, -0.2), Vec3::ONE)];
    let plain = render(&gpu, &mut r, &floor_scene(true, None, QualityLevel::High, 0.0, sun()), SIZE, SIZE).0;
    let ao = Some(AmbientOcclusion::DEFAULT);
    let shaded = render(&gpu, &mut r, &floor_scene(true, ao, QualityLevel::High, 0.0, sun()), SIZE, SIZE).0;
    assert_eq!(max_difference(&plain, &shaded), 0);
}

/// A view of odd size, and resized between frames.
#[test]
fn ambient_occlusion_follows_the_view_size() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let ao = Some(AmbientOcclusion::DEFAULT);
    for size in [SIZE, 65, 200, SIZE] {
        let shaded = render(&gpu, &mut r, &floor_scene(true, ao, QualityLevel::High, 1.0, vec![]), size, size).0;
        let plain = render(&gpu, &mut r, &floor_scene(true, None, QualityLevel::High, 1.0, vec![]), size, size).0;
        assert!(max_difference(&plain, &shaded) > 10, "it darkens something ({size})");
    }
}

/// Boxes on a floor, and one on another, in ambient light and a sun: how
/// ambient occlusion looks.
#[test]
fn ambient_occlusion_looks_like_its_golden() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let mut scene = floor_scene(
        true,
        Some(AmbientOcclusion::DEFAULT),
        QualityLevel::High,
        0.6,
        vec![Light::sun(Vec3::new(-0.5, -1.0, -0.3), Vec3::splat(0.8))],
    );
    let cube =
        |at: Vec3, size: f32| Draw::new(CUBE, Mat4::from_translation(at) * Mat4::from_scale(Vec3::splat(size)), grey());
    scene.draws.push(cube(Vec3::new(1.2, 0.25, 0.6), 0.5));
    scene.draws.push(cube(Vec3::new(0.0, 1.25, 0.0), 0.5));
    scene.draws.push(Draw::new(
        SPHERE,
        Mat4::from_translation(Vec3::new(-1.2, 0.4, 0.5)) * Mat4::from_scale(Vec3::splat(0.8)),
        grey(),
    ));
    assert_golden("ambient_occlusion", &render(&gpu, &mut r, &scene, 256, 256).0);
}

/// With nothing drawn, or only see-through things, there's no depth to
/// work from: the view is drawn all the same.
#[test]
fn ambient_occlusion_with_nothing_solid_to_draw() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let mut scene = floor_scene(false, Some(AmbientOcclusion::DEFAULT), QualityLevel::High, 1.0, vec![]);
    scene.draws.clear();
    render(&gpu, &mut r, &scene, SIZE, SIZE);
    scene.draws = vec![Draw::new(PLANE, Mat4::IDENTITY, PbrMaterial { blend: Blend::Transparent, ..grey() })];
    render(&gpu, &mut r, &scene, SIZE, SIZE);
}

/// A box alone has no corners its faces close in: its convex edges stay as
/// they were, so the stage 1 cube is its golden with ambient occlusion on.
#[test]
fn a_box_alone_does_not_darken_its_own_edges() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let mut scene = stage1_scene();
    scene.quality = Some(QualityLevel::High.quality());
    if let Some(camera) = &mut scene.camera {
        camera.ambient_occlusion = Some(AmbientOcclusion::DEFAULT);
    }
    assert_golden("cube", &render(&gpu, &mut r, &scene, 256, 256).0);
}

/// Along the foot of a long wall the darkening is smooth: each pixel's
/// samples turn differently, and the blur evens that out rather than
/// leaving blotches.
#[test]
fn the_darkening_along_a_wall_is_smooth() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let mut scene = floor_scene(false, Some(AmbientOcclusion::DEFAULT), QualityLevel::High, 1.0, vec![]);
    let wall = Mat4::from_translation(Vec3::new(0.0, 0.5, 0.0)) * Mat4::from_scale(Vec3::new(10.0, 1.0, 1.0));
    scene.draws.push(Draw::new(CUBE, wall, grey()));
    let image = render(&gpu, &mut r, &scene, SIZE, SIZE).0;
    let y = foot_row();
    // Across the middle half of the view, on the floor just past the foot.
    let row: Vec<u8> = (SIZE / 4..SIZE * 3 / 4).map(|x| image.pixel(x, y + 2)[0]).collect();
    let (darkest, lightest) = (*row.iter().min().unwrap(), *row.iter().max().unwrap());
    assert!(lightest - darkest <= 3, "even along the foot: {darkest}..{lightest} in {row:?}");
}

/// A game's own quality decides: ambient occlusion turned on at low (no
/// scene image) darkens as at high, and turned off at high doesn't.
#[test]
fn a_games_own_quality_decides_ambient_occlusion() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let ao = Some(AmbientOcclusion::DEFAULT);
    for (base, on) in [(Quality::LOW, true), (Quality::MEDIUM, true), (Quality::HIGH, false)] {
        let mut plain = floor_scene(true, None, QualityLevel::High, 1.0, vec![]);
        plain.quality = Some(Quality { ambient_occlusion: on, ..base });
        let mut shaded = floor_scene(true, ao, QualityLevel::High, 1.0, vec![]);
        shaded.quality = plain.quality;
        let (plain, shaded) = (render(&gpu, &mut r, &plain, SIZE, SIZE).0, render(&gpu, &mut r, &shaded, SIZE, SIZE).0);
        let changed = max_difference(&plain, &shaded);
        assert_eq!(changed > 8, on, "{base:?} with ambient occlusion {on}: {changed}");
    }
}
