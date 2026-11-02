//! Bloom (stage 8 phase 6), checked on the real GPU: light brighter than
//! white glows into what's around it, at medium and high, and nothing
//! changes where nothing is that bright, or at low.

mod common;

use common::*;
use fynkel_render::glam::{Mat4, Vec3};
use fynkel_render::*;

const SRGB: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const SIZE: u32 = 64;

/// A square glowing `glow` (linear), a fifth of the view across, on black,
/// seen through a camera with `bloom`.
fn glowing(glow: f32, bloom: Option<Bloom>, quality: QualityLevel) -> RenderScene {
    let mut camera = CameraView::looking_at(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO);
    camera.bloom = bloom;
    let material = PbrMaterial { base_color: [0.0, 0.0, 0.0, 1.0], emissive: [glow; 3], ..PbrMaterial::DEFAULT };
    let model = Mat4::from_rotation_x(90f32.to_radians()) * Mat4::from_scale(Vec3::splat(1.2));
    RenderScene {
        camera: Some(camera),
        quality: Some(quality.quality()),
        time: [0.0; 2],
        shader_values: Vec::new(),
        effects: Vec::new(),
        draws: vec![Draw::new(PLANE, model, material)],
        ..Default::default()
    }
}

/// The red of the pixel `by` pixels left of the square's edge, in the
/// middle row.
fn beside(image: &Image, by: u32) -> u8 {
    // The square is about 12 pixels across, from x = 26.
    image.pixel(26 - by, SIZE / 2)[0]
}

fn max_difference(a: &Image, b: &Image) -> u8 {
    a.rgba.iter().zip(&b.rgba).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0)
}

#[test]
fn light_brighter_than_white_glows_at_medium_and_high() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    for quality in [QualityLevel::Medium, QualityLevel::High] {
        let plain = render(&gpu, &mut r, &glowing(8.0, None, quality), SIZE, SIZE).0;
        assert_eq!(beside(&plain, 3), 0, "no bloom: black beside the square ({quality:?})");
        let bloom = Bloom::DEFAULT;
        let glow = render(&gpu, &mut r, &glowing(8.0, Some(bloom), quality), SIZE, SIZE).0;
        let (near, far) = (beside(&glow, 3), beside(&glow, 10));
        assert!(near > 20, "the glow spreads beside it: {near} ({quality:?})");
        assert!(far > 0 && far < near, "fading further away: {far} < {near} ({quality:?})");
        assert!(glow.pixel(0, 0)[0] < near, "and fainter in the corner ({quality:?})");

        let stronger =
            render(&gpu, &mut r, &glowing(8.0, Some(Bloom { strength: 2.0, ..bloom }), quality), SIZE, SIZE).0;
        assert!(beside(&stronger, 3) > near, "more strength, more glow ({quality:?})");
    }
}

#[test]
fn nothing_glows_at_low() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let plain = render(&gpu, &mut r, &glowing(8.0, None, QualityLevel::Low), SIZE, SIZE).0;
    let bloom = render(&gpu, &mut r, &glowing(8.0, Some(Bloom::DEFAULT), QualityLevel::Low), SIZE, SIZE).0;
    assert_eq!(max_difference(&plain, &bloom), 0);
}

/// Bloom on changes nothing where nothing is brighter than white, and at a
/// strength of 0 it changes nothing at all.
#[test]
fn bloom_changes_nothing_that_is_not_bright() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    for quality in [QualityLevel::Medium, QualityLevel::High] {
        let plain = render(&gpu, &mut r, &glowing(0.5, None, quality), SIZE, SIZE).0;
        let bloom = render(&gpu, &mut r, &glowing(0.5, Some(Bloom::DEFAULT), quality), SIZE, SIZE).0;
        assert!(max_difference(&plain, &bloom) <= 1, "a dim square ({quality:?})");

        let plain = render(&gpu, &mut r, &glowing(8.0, None, quality), SIZE, SIZE).0;
        let none = Bloom { strength: 0.0, ..Bloom::DEFAULT };
        let bloom = render(&gpu, &mut r, &glowing(8.0, Some(none), quality), SIZE, SIZE).0;
        assert!(max_difference(&plain, &bloom) <= 1, "strength 0 ({quality:?})");
    }

    // The stage 1 cube, lit no brighter than white, is its golden with
    // bloom on.
    let mut scene = stage1_scene();
    for quality in [QualityLevel::Medium, QualityLevel::High] {
        scene.quality = Some(quality.quality());
        if let Some(camera) = &mut scene.camera {
            camera.bloom = Some(Bloom::DEFAULT);
        }
        assert_golden("cube", &render(&gpu, &mut r, &scene, 256, 256).0);
    }
}

/// A view of odd size, and one resized between frames, keep glowing.
#[test]
fn bloom_follows_the_view_size() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    for size in [33, 64, 25, 200] {
        let image = render(&gpu, &mut r, &glowing(8.0, Some(Bloom::DEFAULT), QualityLevel::High), size, size).0;
        let centre = image.pixel(size / 2, size / 2);
        assert!(centre[0] >= 250, "the square itself is white: {centre:?} ({size})");
        let left = size / 2 - size / 10 - 2;
        assert!(image.pixel(left, size / 2)[0] > 0, "and glows beside it ({size})");
    }
}

/// Glowing balls over a dim floor, at high: how bloom looks.
#[test]
fn bloom_looks_like_its_golden() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let mut camera = CameraView::looking_at(Vec3::new(0.0, 2.0, 6.0), Vec3::new(0.0, 0.5, 0.0));
    camera.bloom = Some(Bloom::DEFAULT);
    let floor = Draw::new(
        PLANE,
        Mat4::from_scale(Vec3::splat(20.0)),
        PbrMaterial { base_color: [0.3, 0.3, 0.32, 1.0], ..PbrMaterial::DEFAULT },
    );
    let ball = |x: f32, glow: [f32; 3]| {
        Draw::new(
            SPHERE,
            Mat4::from_translation(Vec3::new(x, 0.5, 0.0)) * Mat4::from_scale(Vec3::splat(0.6)),
            PbrMaterial { base_color: [0.0, 0.0, 0.0, 1.0], emissive: glow, ..PbrMaterial::DEFAULT },
        )
    };
    let scene = RenderScene {
        camera: Some(camera),
        quality: Some(QualityLevel::High.quality()),
        time: [0.0; 2],
        shader_values: Vec::new(),
        effects: Vec::new(),
        ambient: Vec3::splat(0.1),
        lights: vec![Light::sun(Vec3::new(-0.3, -1.0, -0.4), Vec3::splat(0.6))],
        draws: vec![floor, ball(-1.6, [6.0, 1.5, 0.3]), ball(0.0, [0.5, 4.0, 1.0]), ball(1.6, [0.4, 0.8, 0.9])],
        ..Default::default()
    };
    assert_golden("bloom", &render(&gpu, &mut r, &scene, 256, 256).0);
}

/// Colours shown as they're picked, the clear colour and a gradient sky,
/// aren't light: however bright, they don't glow.
#[test]
fn colours_shown_as_picked_do_not_glow() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let mut scene = glowing(0.5, Some(Bloom::DEFAULT), QualityLevel::High);
    if let Some(camera) = &mut scene.camera {
        camera.clear_color = [1.0, 0.0, 0.0];
    }
    let image = render(&gpu, &mut r, &scene, SIZE, SIZE).0;
    assert_eq!(image.pixel(2, 2), [255, 0, 0, 255], "a red clear colour");

    let white = Vec3::ONE;
    scene.sky = Some(Sky { kind: SkyKind::Gradient { zenith: white, horizon: white, ground: white }, lighting: 0.0 });
    let image = render(&gpu, &mut r, &scene, SIZE, SIZE).0;
    if let Some(camera) = &mut scene.camera {
        camera.bloom = None;
    }
    let plain = render(&gpu, &mut r, &scene, SIZE, SIZE).0;
    assert!(max_difference(&image, &plain) <= 1, "a white sky, around a dim square");
}

/// A game's own quality decides, not the level it started from: bloom
/// turned on at low glows, and turned off at high doesn't.
#[test]
fn a_games_own_quality_decides_bloom() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let mut scene = glowing(8.0, Some(Bloom::DEFAULT), QualityLevel::Low);
    scene.quality = Some(Quality { bloom: true, ..Quality::LOW });
    let glow = render(&gpu, &mut r, &scene, SIZE, SIZE).0;
    assert!(beside(&glow, 3) > 20, "low, with bloom: it glows ({})", beside(&glow, 3));

    scene.quality = Some(Quality { bloom: false, ..Quality::HIGH });
    let plain = render(&gpu, &mut r, &scene, SIZE, SIZE).0;
    assert_eq!(beside(&plain, 3), 0, "high, without bloom: black beside the square");
}

/// A black sphere with an outline of `color`, a fifth of a metre wide, on
/// black, seen through a camera with `bloom`.
fn outlined(color: [f32; 3], bloom: Option<Bloom>, quality: QualityLevel) -> RenderScene {
    let mut camera = CameraView::looking_at(Vec3::new(0.0, 0.0, 6.0), Vec3::ZERO);
    camera.bloom = bloom;
    let material = PbrMaterial {
        base_color: [0.0, 0.0, 0.0, 1.0],
        outline: Some(Outline { width: 0.2, color }),
        ..PbrMaterial::DEFAULT
    };
    RenderScene {
        camera: Some(camera),
        quality: Some(quality.quality()),
        draws: vec![Draw::new(SPHERE, Mat4::IDENTITY, material)],
        ..RenderScene::default()
    }
}

/// Stage 14's RN1: an outline is a colour shown as picked, not light, so
/// a white or pure green one doesn't glow, as a sky's gradient doesn't.
#[test]
fn outlines_dont_glow() {
    // Big enough for the outline to be a few pixels wide.
    const WIDE: u32 = 96;
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    for color in [[1.0, 1.0, 1.0], [0.0, 1.0, 0.0]] {
        for quality in [QualityLevel::Medium, QualityLevel::High] {
            let plain = render(&gpu, &mut r, &outlined(color, None, quality), WIDE, WIDE).0;
            let glow = render(&gpu, &mut r, &outlined(color, Some(Bloom::DEFAULT), quality), WIDE, WIDE).0;
            assert!(max_difference(&plain, &glow) <= 2, "a {color:?} outline glows at {quality:?}");
        }
    }
}

/// Bloom's images are kept a second after the last frame with bloom, so a
/// camera that turns it on and off doesn't make them again each time,
/// then let go (stage 14's RN9 ½: they were kept for good).
#[test]
fn bloom_is_let_go_a_second_after_the_last_frame_with_it() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let plain = glowing(8.0, None, QualityLevel::Medium);
    assert_eq!(render(&gpu, &mut r, &plain, SIZE, SIZE).1.bloom_bytes, 0);
    let (_, stats) = render(&gpu, &mut r, &glowing(8.0, Some(Bloom::DEFAULT), QualityLevel::Medium), SIZE, SIZE);
    assert!(stats.bloom_bytes > 0);
    let mut frames = 0;
    while render(&gpu, &mut r, &plain, SIZE, SIZE).1.bloom_bytes > 0 {
        frames += 1;
        assert!(frames < 100, "never let go");
    }
    assert_eq!(frames, 60, "kept about a second");
}
