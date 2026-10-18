//! See-through materials (stage 8 phase 5), checked on the real GPU:
//! drawn after everything solid, back to front, over what's behind them;
//! additive ones adding light; fog on both; no shadows from either.

mod common;

use common::*;
use fynkel_render::glam::{Mat4, Vec3};
use fynkel_render::*;

const SRGB: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

const RED: Vec3 = Vec3::new(0.5, 0.0, 0.0);
const GREEN: Vec3 = Vec3::new(0.0, 0.5, 0.0);
const BLUE: Vec3 = Vec3::new(0.0, 0.0, 0.5);
const YELLOW: Vec3 = Vec3::new(0.4, 0.4, 0.0);

/// A material that shows `glow` and nothing else (black, unlit, no
/// highlight without lights), `opacity` see-through by `blend`. Each
/// colour has a channel at 0, so tone mapping leaves it as it is.
fn glow(glow: Vec3, opacity: f32, blend: Blend) -> PbrMaterial {
    PbrMaterial { base_color: [0.0, 0.0, 0.0, opacity], emissive: glow.into(), blend, ..PbrMaterial::DEFAULT }
}

/// A square `size` m across facing the camera, `z` m along Z.
fn square(z: f32, size: f32, material: PbrMaterial) -> Draw {
    let model = Mat4::from_translation(Vec3::new(0.0, 0.0, z))
        * Mat4::from_rotation_x(90f32.to_radians())
        * Mat4::from_scale(Vec3::splat(size));
    Draw { cast_shadows: false, ..Draw::new(PLANE, model, material) }
}

/// `draws` seen from 5 m along +Z, on black, with no light but theirs.
fn scene(draws: Vec<Draw>, quality: QualityLevel) -> RenderScene {
    let mut camera = CameraView::looking_at(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO);
    camera.clear_color = [0.0; 3];
    RenderScene { camera: Some(camera), quality: Some(quality.quality()), draws, ..Default::default() }
}

fn assert_near(got: [u8; 4], want: Vec3, what: &str) {
    let want = to_srgb8(want);
    for c in 0..3 {
        assert!(got[c].abs_diff(want[c]) <= 2, "{what}: got {got:?}, want {want:?}");
    }
}

/// `front` with `opacity` over `behind`.
fn over(front: Vec3, opacity: f32, behind: Vec3) -> Vec3 {
    front * opacity + behind * (1.0 - opacity)
}

#[test]
fn a_see_through_surface_shows_what_is_behind_it() {
    let gpu = gpu();
    for quality in [QualityLevel::Low, QualityLevel::Medium, QualityLevel::High] {
        let mut r = renderer(&gpu, SRGB);
        // Given first, the glass is still drawn over the wall behind it.
        let draws = vec![
            square(1.0, 1.0, glow(GREEN, 0.5, Blend::Transparent)),
            square(0.0, 4.0, glow(RED, 1.0, Blend::Opaque)),
        ];
        let (image, stats) = render(&gpu, &mut r, &scene(draws, quality), 33, 33);
        assert_near(image.pixel(16, 16), over(GREEN, 0.5, RED), &format!("glass over the wall ({quality:?})"));
        assert_near(image.pixel(9, 16), RED, &format!("the wall beside it ({quality:?})"));
        assert_eq!(stats.draw_calls, 2);

        // Behind the wall, it's hidden.
        let draws = vec![
            square(-1.0, 4.0, glow(GREEN, 0.5, Blend::Transparent)),
            square(0.0, 1.0, glow(RED, 1.0, Blend::Opaque)),
        ];
        let image = render(&gpu, &mut r, &scene(draws, quality), 33, 33).0;
        assert_near(image.pixel(16, 16), RED, &format!("glass behind the wall ({quality:?})"));
        assert_near(image.pixel(9, 16), over(GREEN, 0.5, Vec3::ZERO), &format!("glass on black ({quality:?})"));
    }
}

/// Three squares, half see-through, nearest the smallest: drawn furthest
/// first, each over the ones behind it, whatever order they're given in.
#[test]
fn see_through_surfaces_are_drawn_back_to_front() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let layers = [(1.0, RED), (0.0, GREEN), (-1.0, BLUE)];
    let want = over(RED, 0.5, over(GREEN, 0.5, over(BLUE, 0.5, Vec3::ZERO)));
    let squares = |order: &[usize]| {
        let draws = order
            .iter()
            .map(|&i| square(layers[i].0, 1.0 + i as f32, glow(layers[i].1, 0.5, Blend::Transparent)))
            .collect();
        scene(draws, QualityLevel::High)
    };
    for order in [[0, 1, 2], [2, 1, 0], [1, 2, 0]] {
        let (image, stats) = render(&gpu, &mut r, &squares(&order), 64, 64);
        assert_near(image.pixel(32, 32), want, &format!("all three, given in order {order:?}"));
        assert_eq!(stats.draw_calls, 1, "one instanced call, its instances in order");
        assert_golden("transparent_order", &image);
    }
}

#[test]
fn see_through_surfaces_of_one_kind_are_one_draw_call() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let draws = (0..5).map(|i| square(-(i as f32), 1.0, glow(GREEN, 0.3, Blend::Transparent))).collect();
    let stats = render(&gpu, &mut r, &scene(draws, QualityLevel::High), 32, 32).1;
    assert_eq!(stats.draw_calls, 1);
    assert_eq!(stats.draws, 5);
}

#[test]
fn a_see_through_surface_is_drawn_over_the_sky() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let mut s = scene(vec![square(0.0, 4.0, glow(GREEN, 0.5, Blend::Transparent))], QualityLevel::High);
    s.sky = Some(Sky { kind: SkyKind::Color(RED), lighting: 0.0 });
    let image = render(&gpu, &mut r, &s, 33, 33).0;
    assert_near(image.pixel(16, 16), over(GREEN, 0.5, RED), "glass over the sky");
}

#[test]
fn an_additive_surface_adds_its_light() {
    let gpu = gpu();
    for quality in [QualityLevel::Low, QualityLevel::High] {
        let mut r = renderer(&gpu, SRGB);
        let draws = vec![
            square(1.0, 1.0, glow(RED, 0.5, Blend::Additive)),
            square(0.5, 1.0, glow(BLUE, 1.0, Blend::Additive)),
            square(0.0, 4.0, glow(GREEN, 1.0, Blend::Opaque)),
        ];
        let image = render(&gpu, &mut r, &scene(draws, quality), 33, 33).0;
        assert_near(image.pixel(16, 16), GREEN + RED * 0.5 + BLUE, &format!("glow over the wall ({quality:?})"));
    }
}

#[test]
fn below_its_cutoff_a_see_through_surface_is_not_there() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let faint = PbrMaterial { alpha_cutoff: 0.5, ..glow(GREEN, 0.4, Blend::Transparent) };
    let solid = PbrMaterial { alpha_cutoff: 0.5, ..glow(GREEN, 0.6, Blend::Transparent) };
    let draws = vec![square(0.0, 4.0, glow(RED, 1.0, Blend::Opaque)), square(1.0, 1.0, faint)];
    assert_near(render(&gpu, &mut r, &scene(draws, QualityLevel::High), 33, 33).0.pixel(16, 16), RED, "under 0.5");
    let draws = vec![square(0.0, 4.0, glow(RED, 1.0, Blend::Opaque)), square(1.0, 1.0, solid)];
    let image = render(&gpu, &mut r, &scene(draws, QualityLevel::High), 33, 33).0;
    assert_near(image.pixel(16, 16), over(GREEN, 0.6, RED), "over 0.5, it blends");
}

/// Past the fog's end, glass and glow both vanish into it: glass shows
/// the fog, which is what's behind it too, and glow adds nothing.
#[test]
fn fog_hides_see_through_surfaces_as_it_hides_solid_ones() {
    let gpu = gpu();
    for quality in [QualityLevel::Low, QualityLevel::High] {
        let mut r = renderer(&gpu, SRGB);
        let fog = Fog { color: Some(YELLOW), start: 1.0, end: 2.0, height: None };
        for blend in [Blend::Transparent, Blend::Additive] {
            let draws =
                vec![square(0.0, 4.0, glow(RED, 1.0, Blend::Opaque)), square(1.0, 1.0, glow(GREEN, 0.5, blend))];
            let s = RenderScene { fog: Some(fog), ..scene(draws, quality) };
            let image = render(&gpu, &mut r, &s, 33, 33).0;
            assert_near(image.pixel(16, 16), YELLOW, &format!("{blend:?} in thick fog ({quality:?})"));
        }
    }
}

/// A see-through square between the sun and the ground casts no shadow.
#[test]
fn see_through_surfaces_cast_no_shadow() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let ground = Draw::new(
        PLANE,
        Mat4::from_scale(Vec3::splat(10.0)),
        PbrMaterial { base_color: [0.5, 0.5, 0.5, 1.0], ..PbrMaterial::DEFAULT },
    );
    let pane = |blend| Draw {
        cast_shadows: true,
        ..Draw::new(
            PLANE,
            Mat4::from_translation(Vec3::Y) * Mat4::from_scale(Vec3::splat(3.0)),
            PbrMaterial { base_color: [0.2, 0.4, 0.8, 0.5], double_sided: true, blend, ..PbrMaterial::DEFAULT },
        )
    };
    let mut lit = |draws: Vec<Draw>| {
        // Looking down at the ground from below the pane, so it's out of
        // view; only its shadow could show.
        let mut camera = CameraView::looking_at(Vec3::new(0.0, 0.5, 0.01), Vec3::ZERO);
        camera.fov_y = 60f32.to_radians();
        let s = RenderScene {
            camera: Some(camera),
            lights: vec![Light::sun(Vec3::NEG_Y, Vec3::ONE).with_shadows(true)],
            quality: Some(QualityLevel::High.quality()),
            time: [0.0; 2],
            shader_values: Vec::new(),
            draws,
            ..Default::default()
        };
        render(&gpu, &mut r, &s, 33, 33).0.pixel(16, 16)
    };
    let bare = lit(vec![ground]);
    assert!(lit(vec![ground, pane(Blend::Opaque)])[0] < bare[0] / 2, "an opaque pane shadows the ground");
    for blend in [Blend::Transparent, Blend::Additive] {
        assert_eq!(lit(vec![ground, pane(blend)]), bare, "a {blend:?} one doesn't");
    }
}
