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
    quality: Quality,
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
        quality: Some(quality),
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
    // Where the foot's middle, (0, 0, 0.5), is in the view.
    let camera = CameraView::looking_at(Vec3::new(0.0, 3.0, 3.0), Vec3::ZERO);
    let projection =
        fynkel_render::glam::camera::rh::proj::directx::perspective(camera.fov_y, 1.0, camera.near, camera.far);
    let p = (projection * camera.view).project_point3(Vec3::new(0.0, 0.0, 0.5));
    let y = ((1.0 - p.y) / 2.0 * SIZE as f32) as u32;
    (image.pixel(SIZE / 2, y + 2)[0], image.pixel(8, y + 2)[0])
}

#[test]
fn the_floor_darkens_where_a_box_stands_on_it_at_high() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let ao = Some(AmbientOcclusion::DEFAULT);
    let plain = render(&gpu, &mut r, &floor_scene(true, None, Quality::High, 1.0, vec![]), SIZE, SIZE).0;
    let (foot, far) = at_the_foot(&plain);
    assert_eq!(foot, far, "without it, the floor is even");

    let shaded = render(&gpu, &mut r, &floor_scene(true, ao, Quality::High, 1.0, vec![]), SIZE, SIZE).0;
    let (foot, far) = at_the_foot(&shaded);
    assert!(foot + 10 < far, "darker at the box's foot: {foot} against {far}");
    assert!(far.abs_diff(at_the_foot(&plain).1) <= 2, "and as it was away from it");

    let stronger = Some(AmbientOcclusion { intensity: 3.0, ..AmbientOcclusion::DEFAULT });
    let darker = render(&gpu, &mut r, &floor_scene(true, stronger, Quality::High, 1.0, vec![]), SIZE, SIZE).0;
    assert!(at_the_foot(&darker).0 < foot, "more intensity, darker");
}

#[test]
fn nothing_darkens_below_high() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    for quality in [Quality::Low, Quality::Medium] {
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
    let plain = render(&gpu, &mut r, &floor_scene(false, None, Quality::High, 1.0, vec![]), SIZE, SIZE).0;
    let ao = Some(AmbientOcclusion::DEFAULT);
    let shaded = render(&gpu, &mut r, &floor_scene(false, ao, Quality::High, 1.0, vec![]), SIZE, SIZE).0;
    assert!(max_difference(&plain, &shaded) <= 2, "{}", max_difference(&plain, &shaded));
}

/// Light from a sun isn't darkened, only the ambient.
#[test]
fn light_from_lights_is_left_as_it_is() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let sun = || vec![Light::sun(Vec3::new(0.0, -1.0, -0.2), Vec3::ONE)];
    let plain = render(&gpu, &mut r, &floor_scene(true, None, Quality::High, 0.0, sun()), SIZE, SIZE).0;
    let ao = Some(AmbientOcclusion::DEFAULT);
    let shaded = render(&gpu, &mut r, &floor_scene(true, ao, Quality::High, 0.0, sun()), SIZE, SIZE).0;
    assert_eq!(max_difference(&plain, &shaded), 0);
}

/// A view of odd size, and resized between frames.
#[test]
fn ambient_occlusion_follows_the_view_size() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let ao = Some(AmbientOcclusion::DEFAULT);
    for size in [SIZE, 65, 200, SIZE] {
        let shaded = render(&gpu, &mut r, &floor_scene(true, ao, Quality::High, 1.0, vec![]), size, size).0;
        let plain = render(&gpu, &mut r, &floor_scene(true, None, Quality::High, 1.0, vec![]), size, size).0;
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
        Quality::High,
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
