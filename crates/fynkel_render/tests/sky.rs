//! The sky, the light it gives, and fog (stage 8 phase 4), checked on the
//! real GPU.

mod common;

use std::sync::Arc;

use common::*;
use fynkel_render::glam::{Mat4, Vec3};
use fynkel_render::*;

const SRGB: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

const BLUE: Vec3 = Vec3::new(0.05, 0.2, 0.8);
const ORANGE: Vec3 = Vec3::new(0.8, 0.4, 0.1);
const RED: Vec3 = Vec3::new(0.6, 0.05, 0.02);

fn gradient(zenith: Vec3, horizon: Vec3, ground: Vec3) -> Sky {
    Sky { kind: SkyKind::Gradient { zenith, horizon, ground }, lighting: 1.0 }
}

/// Nothing but a camera at the origin looking at `target`, under `sky`.
fn empty(target: Vec3, sky: Sky, quality: QualityLevel) -> RenderScene {
    let up_or_down = target.x == 0.0 && target.z == 0.0;
    let mut camera = CameraView::looking_at(Vec3::ZERO, target);
    if up_or_down {
        camera.view = fynkel_render::glam::camera::rh::view::look_at_mat4(Vec3::ZERO, target, Vec3::Z);
    }
    RenderScene { camera: Some(camera), sky: Some(sky), quality: Some(quality.quality()), ..Default::default() }
}

/// A rough grey square 4 m across at `at`, facing a camera 3 m in front of
/// it along +Z, with `ambient` and `sky`.
fn wall(at: Vec3, ambient: Vec3, sky: Option<Sky>, quality: QualityLevel) -> RenderScene {
    let albedo = linear(0.5);
    RenderScene {
        camera: Some(CameraView::looking_at(at + Vec3::new(0.0, 0.0, 3.0), at)),
        ambient,
        sky,
        quality: Some(quality.quality()),
        time: [0.0; 2],
        shader_values: Vec::new(),
        effects: Vec::new(),
        draws: vec![Draw::new(
            PLANE,
            Mat4::from_translation(at) * Mat4::from_rotation_x(90f32.to_radians()) * Mat4::from_scale(Vec3::splat(4.0)),
            PbrMaterial { base_color: [albedo, albedo, albedo, 1.0], roughness: 1.0, ..PbrMaterial::DEFAULT },
        )],
        ..Default::default()
    }
}

/// A sphere 1 m across, 1.5 m in front of the camera (64 pixels high, it
/// is about 40 across), of `metallic` and `roughness`, under `sky` alone.
fn ball(metallic: f32, roughness: f32, sky: Sky) -> RenderScene {
    RenderScene {
        camera: Some(CameraView::looking_at(Vec3::new(0.0, 0.0, 1.5), Vec3::ZERO)),
        sky: Some(sky),
        quality: Some(QualityLevel::High.quality()),
        time: [0.0; 2],
        shader_values: Vec::new(),
        effects: Vec::new(),
        draws: vec![Draw::new(
            SPHERE,
            Mat4::IDENTITY,
            PbrMaterial { base_color: [0.9, 0.9, 0.9, 1.0], metallic, roughness, ..PbrMaterial::DEFAULT },
        )],
        ..Default::default()
    }
}

fn rgb(p: [u8; 4]) -> [u8; 3] {
    [p[0], p[1], p[2]]
}

/// The wall's colour lit by `ambient` alone, as forward.wgsl's maths says.
fn wall_lit(ambient: Vec3) -> Vec3 {
    let surface = Surface { albedo: Vec3::splat(linear(0.5)), metallic: 0.0, roughness: 1.0 };
    tonemap(shade(&surface, Vec3::Z, Vec3::Z, Vec3::Z, Vec3::ZERO, ambient))
}

fn assert_near(got: [u8; 4], want: [u8; 3], within: u8, what: &str) {
    for c in 0..3 {
        assert!(got[c].abs_diff(want[c]) <= within, "{what}: got {got:?}, want {want:?}");
    }
}

#[test]
fn a_gradient_sky_is_drawn_behind_everything_as_its_colours_are_picked() {
    let gpu = gpu();
    for quality in [QualityLevel::Low, QualityLevel::High] {
        let mut r = renderer(&gpu, SRGB);
        let sky = gradient(BLUE, ORANGE, RED);
        // 33 pixels: the middle one's centre is the view's.
        let mut centre =
            |target: Vec3| render(&gpu, &mut r, &empty(target, sky.clone(), quality), 33, 33).0.pixel(16, 16);
        // Not tone mapped: the colour picked is the colour seen.
        assert_near(centre(Vec3::Y), to_srgb8(BLUE), 1, "straight up: the zenith");
        assert_near(centre(Vec3::NEG_Z), to_srgb8(ORANGE), 1, "level: the horizon");
        assert_near(centre(Vec3::NEG_Y), to_srgb8(RED), 1, "straight down: the ground");
        let halfway = rgb(centre(Vec3::new(0.0, 1.0, -1.0)));
        assert!(halfway[2] > to_srgb8(ORANGE)[2] && halfway[2] < to_srgb8(BLUE)[2], "45° up is between: {halfway:?}");

        // In front of it, a wall hides it (the sky's light off, so the wall
        // is ambient's alone); and a camera can leave it out.
        let dark = Sky { lighting: 0.0, ..sky.clone() };
        let mut scene = wall(Vec3::ZERO, Vec3::splat(1.0), Some(dark), quality);
        let shown = render(&gpu, &mut r, &scene, 32, 32).0;
        assert_near(shown.pixel(16, 16), to_srgb8(wall_lit(Vec3::ONE)), 1, "the wall");
        if let Some(camera) = &mut scene.camera {
            camera.sky = false;
            camera.clear_color = [0.0, 1.0, 0.0];
            camera.fov_y = 120f32.to_radians();
        }
        let hidden = render(&gpu, &mut r, &scene, 32, 32).0;
        assert_eq!(
            hidden.pixel(0, 0),
            [0, 255, 0, 255],
            "a camera without the sky shows its clear colour ({quality:?})"
        );
    }
}

#[test]
fn a_plain_sky_lights_as_ambient_light_does() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let c = Vec3::new(0.6, 0.5, 0.4);
    let lit_by_ambient = render(&gpu, &mut r, &wall(Vec3::ZERO, c, None, QualityLevel::High), 32, 32).0.pixel(16, 16);
    let plain = Sky { kind: SkyKind::Color(c), lighting: 1.0 };
    let lit_by_sky = render(&gpu, &mut r, &wall(Vec3::ZERO, Vec3::ZERO, Some(plain), QualityLevel::High), 32, 32).0;
    assert_near(lit_by_sky.pixel(16, 16), rgb(lit_by_ambient), 1, "a plain sky of c is ambient c");
    // A gradient of one colour and an image of one colour are the same sky.
    let even =
        render(&gpu, &mut r, &wall(Vec3::ZERO, Vec3::ZERO, Some(gradient(c, c, c)), QualityLevel::High), 32, 32).0;
    assert_near(even.pixel(16, 16), rgb(lit_by_ambient), 2, "a gradient of one colour");
    let image = Arc::new(SkyImage::from_rgb(16, 8, &[c.x, c.y, c.z].repeat(16 * 8)).unwrap());
    let flat = Sky { kind: SkyKind::Image { image, intensity: 1.0, rotation: 0.0 }, lighting: 1.0 };
    let from_image = render(&gpu, &mut r, &wall(Vec3::ZERO, Vec3::ZERO, Some(flat), QualityLevel::High), 32, 32).0;
    assert_near(from_image.pixel(16, 16), rgb(lit_by_ambient), 2, "an image of one colour");
    // `lighting` scales the light, not what's seen of the sky.
    let half = Sky { kind: SkyKind::Color(c), lighting: 0.5 };
    let lit_by_half = render(&gpu, &mut r, &wall(Vec3::ZERO, Vec3::ZERO, Some(half), QualityLevel::High), 32, 32).0;
    let want = render(&gpu, &mut r, &wall(Vec3::ZERO, c * 0.5, None, QualityLevel::High), 32, 32).0;
    assert_near(lit_by_half.pixel(16, 16), rgb(want.pixel(16, 16)), 1, "lighting 0.5");
}

#[test]
fn a_rough_ball_is_lit_by_the_sky_above_it_and_the_ground_below() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let image = render(&gpu, &mut r, &ball(0.0, 1.0, gradient(BLUE, BLUE, RED)), 64, 64).0;
    let (top, bottom) = (rgb(image.pixel(32, 20)), rgb(image.pixel(32, 44)));
    assert!(top[2] > top[0], "its top is lit blue: {top:?}");
    assert!(bottom[0] > top[0] + 40 && top[2] > bottom[2] + 40, "its bottom redder: {top:?} above {bottom:?}");
}

#[test]
fn a_shiny_ball_mirrors_the_sky_and_a_rough_one_only_its_colour() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let sky = gradient(BLUE, ORANGE, RED);
    let shiny = render(&gpu, &mut r, &ball(1.0, 0.0, sky.clone()), 64, 64).0;
    // Near its edge, a mirror reflects what's behind and beside it: the
    // horizon. Near its top, the zenith; near its bottom, the ground.
    let (top, side, bottom) = (rgb(shiny.pixel(32, 17)), rgb(shiny.pixel(17, 32)), rgb(shiny.pixel(32, 47)));
    assert!(top[2] > top[0], "the top mirrors the zenith: {top:?}");
    assert!(side[0] > side[2] && side[1] > side[2], "the side the horizon: {side:?}");
    assert!(bottom[0] > 2 * bottom[1].max(bottom[2]), "the bottom the ground: {bottom:?}");

    let rough = render(&gpu, &mut r, &ball(1.0, 1.0, sky), 64, 64).0;
    let spread = |i: &Image| {
        let (a, b) = (rgb(i.pixel(32, 17)), rgb(i.pixel(32, 47)));
        (0..3).map(|c| a[c].abs_diff(b[c]) as u32).sum::<u32>()
    };
    assert!(spread(&rough) * 2 < spread(&shiny), "a rough one blurs it: {} vs {}", spread(&rough), spread(&shiny));
}

/// An image 8 × 4: each column a colour, so which one is seen says which
/// way the camera looks. Column 4 (u 0.5) is straight ahead (−Z), column 6
/// (u 0.75) to the right (+X).
fn columns() -> Arc<SkyImage> {
    let colour = |x: usize| [Vec3::new(0.1, 0.1, 0.1), Vec3::new(0.2, 0.05, 0.05), Vec3::new(0.05, 0.2, 0.05)][x % 3];
    let rgb: Vec<f32> = (0..4).flat_map(|_| (0..8).flat_map(move |x| colour(x).to_array())).collect();
    Arc::new(SkyImage::from_rgb(8, 4, &rgb).unwrap())
}

#[test]
fn an_image_sky_is_seen_the_way_the_camera_looks_tone_mapped_turned_and_scaled() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let colour = |x: usize| [Vec3::new(0.1, 0.1, 0.1), Vec3::new(0.2, 0.05, 0.05), Vec3::new(0.05, 0.2, 0.05)][x % 3];
    let image = columns();
    let sky = |intensity: f32, rotation: f32| Sky {
        kind: SkyKind::Image { image: image.clone(), intensity, rotation },
        lighting: 1.0,
    };
    // Each column is 45° wide: look at a column's middle.
    let at = |degrees: f32| Vec3::new(degrees.to_radians().sin(), 0.0, -degrees.to_radians().cos());
    let mut seen = |sky: Sky, degrees: f32| {
        render(&gpu, &mut r, &empty(at(degrees), sky, QualityLevel::High), 17, 17).0.pixel(8, 8)
    };
    assert_near(seen(sky(1.0, 0.0), 22.5), to_srgb8(tonemap(colour(4))), 2, "ahead and a little right: column 4");
    assert_near(seen(sky(1.0, 0.0), 112.5), to_srgb8(tonemap(colour(6))), 2, "right: column 6");
    assert_near(seen(sky(4.0, 0.0), 22.5), to_srgb8(tonemap(colour(4) * 4.0)), 2, "four times as bright");
    // Turned 45° to the right, column 4 is where column 5 was.
    assert_near(seen(sky(1.0, 45f32.to_radians()), 67.5), to_srgb8(tonemap(colour(4))), 2, "turned");
}

#[test]
fn an_image_sky_is_read_from_rgb_and_says_its_horizon() {
    assert!(SkyImage::from_rgb(4, 2, &[0.0; 3]).is_err(), "too few values");
    assert!(SkyImage::from_rgb(0, 0, &[]).is_err(), "empty");
    assert!(SkyImage::from_rgb(2, 1, &[f32::NAN, 0.0, 0.0, 0.0, 0.0, 0.0]).is_err(), "not a number");
    // The horizon is the middle rows' average.
    let rows = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let rgb: Vec<f32> = rows.iter().flat_map(|row| row.repeat(2)).collect();
    let image = SkyImage::from_rgb(2, 4, &rgb).unwrap();
    assert!(image.horizon().abs_diff_eq(Vec3::Y, 1e-3), "{}", image.horizon());
}

#[test]
fn fog_fades_what_is_far_into_its_colour() {
    let gpu = gpu();
    for quality in [QualityLevel::Low, QualityLevel::High] {
        let mut r = renderer(&gpu, SRGB);
        let wall_colour = wall_lit(Vec3::ONE);
        let mut seen = |fog: Fog, at: Vec3| {
            let mut scene = wall(at, Vec3::ONE, None, quality);
            scene.fog = Some(fog);
            render(&gpu, &mut r, &scene, 32, 32).0.pixel(16, 16)
        };
        let fog = |start: f32, end: f32, height: Option<f32>| Fog { color: Some(BLUE), start, end, height };
        // The wall is 3 m away.
        assert_near(seen(fog(10.0, 20.0, None), Vec3::ZERO), to_srgb8(wall_colour), 1, "nearer than the fog's start");
        assert_near(seen(fog(0.0, 2.0, None), Vec3::ZERO), to_srgb8(BLUE), 1, "past its end: all fog");
        let half = to_srgb8(wall_colour.lerp(BLUE, 0.5));
        assert_near(seen(fog(2.0, 4.0, None), Vec3::ZERO), half, 2, "halfway: half and half ({quality:?})");
        // Height: as thick as given at y = 0, half as thick `height` above.
        assert_near(seen(fog(0.0, 2.0, Some(5.0)), Vec3::ZERO), to_srgb8(BLUE), 1, "at the ground");
        assert_near(seen(fog(0.0, 2.0, Some(5.0)), Vec3::new(0.0, 5.0, 0.0)), half, 2, "5 m up: half");
    }
}

#[test]
fn fog_without_a_colour_takes_the_skys_horizon() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let mut scene = wall(Vec3::ZERO, Vec3::ONE, Some(gradient(BLUE, ORANGE, RED)), QualityLevel::High);
    scene.fog = Some(Fog { color: None, start: 0.0, end: 1.0, height: None });
    assert_near(render(&gpu, &mut r, &scene, 32, 32).0.pixel(16, 16), to_srgb8(ORANGE), 1, "a gradient's horizon");
    // An image's is tone mapped, as the sky behind is.
    let image = columns();
    let horizon = image.horizon();
    scene.sky = Some(Sky { kind: SkyKind::Image { image, intensity: 2.0, rotation: 0.0 }, lighting: 1.0 });
    let want = to_srgb8(tonemap(horizon * 2.0));
    assert_near(render(&gpu, &mut r, &scene, 32, 32).0.pixel(16, 16), want, 1, "an image's horizon");
    // No sky: the camera's clear colour.
    scene.sky = None;
    if let Some(camera) = &mut scene.camera {
        camera.clear_color = RED.to_array();
    }
    assert_near(render(&gpu, &mut r, &scene, 32, 32).0.pixel(16, 16), to_srgb8(RED), 1, "the clear colour");
}

#[test]
fn a_new_sky_lights_the_next_frame() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let blue = render(&gpu, &mut r, &ball(0.0, 1.0, gradient(BLUE, BLUE, BLUE)), 32, 32).0.pixel(16, 16);
    let red = render(&gpu, &mut r, &ball(0.0, 1.0, gradient(RED, RED, RED)), 32, 32).0.pixel(16, 16);
    assert!(blue[2] > blue[0] && red[0] > red[2], "blue {blue:?}, then red {red:?}");
}
