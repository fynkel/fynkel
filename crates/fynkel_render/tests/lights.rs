//! Point and spot lights, more than one sun, and the caps on how many light
//! a frame and an object (stage 8 phase 2), checked on the real GPU.

mod common;

use common::*;
use fynkel_render::glam::{Mat4, Vec3};
use fynkel_render::*;

const SRGB: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// A rough grey square `size` metres across at the origin, facing the
/// camera 3 m away along +Z, lit by `lights` alone.
fn wall(size: f32, lights: Vec<Light>, quality: QualityLevel) -> RenderScene {
    let albedo = linear(0.5);
    RenderScene {
        joints: Vec::new(),
        camera: Some(CameraView::looking_at(Vec3::new(0.0, 0.0, 3.0), Vec3::ZERO)),
        lights,
        quality: Some(quality.quality()),
        time: [0.0; 2],
        shader_values: Vec::new(),
        effects: Vec::new(),
        ambient: Vec3::ZERO,
        lines: Vec::new(),
        sky: None,
        fog: None,
        draws: vec![Draw {
            skin: None,
            own: [0.0; 4],
            mesh: PLANE,
            model: Mat4::from_rotation_x(90f32.to_radians()) * Mat4::from_scale(Vec3::splat(size)),
            material: PbrMaterial {
                base_color: [albedo, albedo, albedo, 1.0],
                metallic: 0.0,
                roughness: 1.0,
                ..PbrMaterial::DEFAULT
            },
            cast_shadows: true,
            receive_shadows: true,
        }],
    }
}

/// The wall's centre pixel as forward.wgsl's maths says it should be, lit
/// head-on with `irradiance` (a sun's colour).
fn head_on(irradiance: f32) -> [u8; 3] {
    let surface = Surface { albedo: Vec3::splat(linear(0.5)), metallic: 0.0, roughness: 1.0 };
    to_srgb8(tonemap(shade(&surface, Vec3::Z, Vec3::Z, Vec3::Z, Vec3::splat(irradiance), Vec3::ZERO)))
}

fn assert_close(got: [u8; 4], want: [u8; 3], what: &str) {
    for c in 0..3 {
        assert!(got[c].abs_diff(want[c]) <= 1, "{what}: GPU {got:?} vs maths {want:?}");
    }
}

/// The units: a lamp of 1 lights a surface 1 m away as a sun of 1 does, a
/// quarter as much at 2 m, and fades smoothly to nothing at its range
/// (Karis's window, (1 - (d / range)⁴)²).
#[test]
fn a_lamp_one_metre_away_lights_as_a_sun_does_and_fades_with_distance() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let render_centre =
        |r: &mut Renderer, lights| render(&gpu, r, &wall(4.0, lights, QualityLevel::High), 64, 64).0.pixel(32, 32);

    let sun = render_centre(&mut r, vec![Light::sun(Vec3::NEG_Z, Vec3::splat(2.0))]);
    assert_close(sun, head_on(2.0), "a sun of 2");
    let window = |d: f32, range: f32| (1.0 - (d / range).powi(4)).max(0.0).powi(2);
    for (distance, range) in [(1.0, 100.0), (2.0, 100.0), (1.0, 2.0), (1.5, 1.6)] {
        let lamp = Light::point(Vec3::new(0.0, 0.0, distance), range, Vec3::splat(2.0));
        let got = render_centre(&mut r, vec![lamp]);
        let want = head_on(2.0 / (distance * distance) * window(distance, range));
        assert_close(got, want, &format!("a lamp of 2, {distance} m away, reaching {range} m"));
    }
    assert_close(render_centre(&mut r, vec![Light::sun(Vec3::NEG_Z, Vec3::splat(2.0))]), head_on(2.0), "again");
    let out_of_reach = render_centre(&mut r, vec![Light::point(Vec3::new(0.0, 0.0, 1.0), 0.9, Vec3::splat(50.0))]);
    assert_eq!(out_of_reach, [0, 0, 0, 255], "a lamp lights nothing past its range");
}

/// A spotlight lights inside its cone only. Its axis gets the full light,
/// the same as a lamp's; a soft edge fades towards it.
#[test]
fn a_spot_lights_only_inside_its_cone() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    // 2 m from the wall, 60° across: a circle 1.15 m in radius. The camera
    // sees 1.73 m either side of the centre.
    let spot = |softness| {
        Light::spot(Vec3::new(0.0, 0.0, 2.0), Vec3::NEG_Z, 100.0, 60f32.to_radians(), softness, Vec3::splat(8.0))
    };
    let (hard, _) = render(&gpu, &mut r, &wall(4.0, vec![spot(0.0)], QualityLevel::High), 64, 64);
    let (soft, _) = render(&gpu, &mut r, &wall(4.0, vec![spot(1.0)], QualityLevel::High), 64, 64);
    let (lamp, _) = render(
        &gpu,
        &mut r,
        &wall(4.0, vec![Light::point(Vec3::new(0.0, 0.0, 2.0), 100.0, Vec3::splat(8.0))], QualityLevel::High),
        64,
        64,
    );

    assert_eq!(hard.pixel(32, 32), lamp.pixel(32, 32), "on the axis, a spot is a lamp");
    assert_eq!(soft.pixel(32, 32), lamp.pixel(32, 32), "even a soft one");
    assert_eq!(hard.pixel(2, 2), [0, 0, 0, 255], "the corners are outside the cone");
    // Halfway out (0.58 m, 15° off the axis): the hard cone's full light,
    // the soft one's fading.
    let halfway = 32 + 11;
    assert_eq!(hard.pixel(halfway, 32), lamp.pixel(halfway, 32), "inside a hard cone, a lamp's light");
    assert!(soft.pixel(halfway, 32)[0] < hard.pixel(halfway, 32)[0], "a soft edge fades in from the rim");
    assert!(soft.pixel(halfway, 32)[0] > 0, "but reaches halfway");
}

/// Every sun lights the scene, up to `MAX_SUNS`; the rest are left out and
/// counted.
#[test]
fn every_sun_lights_the_scene_up_to_the_cap() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let suns = |n: usize, each: f32| (0..n).map(|_| Light::sun(Vec3::NEG_Z, Vec3::splat(each))).collect();
    let (one, _) = render(&gpu, &mut r, &wall(4.0, suns(1, 1.0), QualityLevel::High), 64, 64);
    let (two, _) = render(&gpu, &mut r, &wall(4.0, suns(2, 0.5), QualityLevel::High), 64, 64);
    assert_eq!(one.pixel(32, 32), two.pixel(32, 32), "two suns of 0.5 make one of 1");

    let (four, stats) = render(&gpu, &mut r, &wall(4.0, suns(Renderer::MAX_SUNS, 0.25), QualityLevel::Low), 64, 64);
    assert_eq!((four.pixel(32, 32), stats.lights_left_out), (one.pixel(32, 32), 0));
    let (five, stats) = render(&gpu, &mut r, &wall(4.0, suns(Renderer::MAX_SUNS + 1, 0.25), QualityLevel::Low), 64, 64);
    assert_eq!((five.pixel(32, 32), stats.lights_left_out), (one.pixel(32, 32), 1), "the fifth is left out");
}

/// Red lamps around the wall's middle, `distance` metres out, each `red`
/// bright.
fn reds(n: usize, distance: f32, red: f32) -> Vec<Light> {
    (0..n)
        .map(|i| {
            let angle = i as f32 * std::f32::consts::TAU / n as f32;
            let at = Vec3::new(0.2 * angle.cos(), 0.2 * angle.sin(), distance);
            Light::point(at, 10.0, Vec3::new(red, 0.0, 0.0))
        })
        .collect()
}

/// Hardening L1, again: a part of the view is lit by every lamp that
/// reaches it, however many, at every level. A cell that kept only some
/// showed its edge where its neighbour kept others (squares of light in
/// the lab's lamps room at low and medium). Twenty dim lamps around the
/// wall's middle each change the pixel there.
#[test]
fn a_cell_is_lit_by_every_lamp_that_reaches_it() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    for quality in [QualityLevel::Low, QualityLevel::Medium, QualityLevel::High] {
        let lamps = reds(20, 1.0, 0.05);
        let centre = |r: &mut Renderer, lights| render(&gpu, r, &wall(1.0, lights, quality), 64, 64).0.pixel(32, 32);
        let all = centre(&mut r, lamps.clone());
        for k in 0..lamps.len() {
            let mut fewer = lamps.clone();
            fewer.remove(k);
            assert!(centre(&mut r, fewer) != all, "{quality:?}: lamp {k} lights the middle ({all:?})");
        }
    }
}

/// A frame takes `MAX_LIGHTS` point and spot lights: past that, the ones
/// furthest from the camera are left out, and counted. Lamps out of view,
/// or off, don't count.
#[test]
fn a_frame_keeps_the_lamps_nearest_the_camera() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    // Lamps in view next to the camera, too short to reach the wall.
    let near: Vec<Light> = (0..Renderer::MAX_LIGHTS)
        .map(|i| Light::point(Vec3::new(0.01 * i as f32 - 0.3, 0.0, 2.0), 0.5, Vec3::ONE))
        .collect();
    // One by the wall, the furthest from the camera.
    let by_the_wall = Light::point(Vec3::new(0.0, 0.0, 0.5), 10.0, Vec3::ONE);
    // Off (a lamp by day): no light, no place.
    let off = Light::point(Vec3::new(0.0, 0.0, 1.0), 10.0, Vec3::ZERO);
    // Behind the camera, out of view, with a short reach.
    let behind = Light::point(Vec3::new(0.0, 0.0, 10.0), 1.0, Vec3::ONE);

    let lights = [vec![by_the_wall, behind, off], near.clone()].concat();
    let (image, stats) = render(&gpu, &mut r, &wall(4.0, lights, QualityLevel::High), 64, 64);
    assert_eq!((stats.lights, stats.lights_left_out), (Renderer::MAX_LIGHTS as u32, 1));
    assert_eq!(image.pixel(32, 32), [0, 0, 0, 255], "the lamp by the wall is the one left out");

    let lights = [vec![by_the_wall, behind, off], near[1..].to_vec()].concat();
    let (image, stats) = render(&gpu, &mut r, &wall(4.0, lights, QualityLevel::High), 64, 64);
    assert_eq!((stats.lights, stats.lights_left_out), (Renderer::MAX_LIGHTS as u32, 0));
    assert_ne!(image.pixel(32, 32), [0, 0, 0, 255], "with room for it, it lights the wall");
}

/// At low, a lamp lights a surface without a highlight, but still in the
/// surface's own colour, metal too: a polished metal wall (no matte colour
/// of its own) is lit at every level, not black at low (the lab's chrome
/// balls were). A matte wall is lit about alike at every level.
#[test]
fn lamps_light_metal_at_every_level() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let lamp = || vec![Light::point(Vec3::new(0.0, 0.0, 1.0), 10.0, Vec3::ONE)];
    let centre = |r: &mut Renderer, scene: &RenderScene| render(&gpu, r, scene, 64, 64).0.pixel(32, 32);
    for quality in [QualityLevel::Low, QualityLevel::Medium, QualityLevel::High] {
        let mut metal = wall(1.0, lamp(), quality);
        metal.draws[0].material.metallic = 1.0;
        metal.draws[0].material.roughness = 0.3;
        let shine = centre(&mut r, &metal);
        assert!(shine[0] > 20, "{quality:?}: the lamp lights the metal: {shine:?}");
    }
    let low = centre(&mut r, &wall(1.0, lamp(), QualityLevel::Low));
    let high = centre(&mut r, &wall(1.0, lamp(), QualityLevel::High));
    // Even a fully rough surface has a faint highlight above low; low
    // spreads it evenly instead, a little brighter where it's seen.
    assert!(low[0] > 20 && low[0].abs_diff(high[0]) <= 10, "matte: low {low:?}, high {high:?}");
}

/// Hardening L1: a big surface is lit by every lamp over it, not only the
/// few brightest at its middle. Six short lamps along one wall each light
/// the wall where they are.
#[test]
fn a_big_wall_is_lit_by_every_lamp_over_it() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let lamps: Vec<Light> =
        (0..6).map(|i| Light::point(Vec3::new(-0.75 + 0.3 * i as f32, 0.0, 0.2), 0.4, Vec3::ONE)).collect();
    let image = |r: &mut Renderer, lights| render(&gpu, r, &wall(4.0, lights, QualityLevel::Low), 64, 64).0;
    let all = image(&mut r, lamps.clone());
    for k in 0..lamps.len() {
        let mut fewer = lamps.clone();
        fewer.remove(k);
        assert!(image(&mut r, fewer) != all, "lamp {k} lights the wall");
    }
}

/// Hardening L1: a lamp's light doesn't stop at an object's edge. Two
/// tiles side by side; brighter red lamps fill the left tile's places, and
/// a green lamp hangs over the seam. The green either side of the seam is
/// alike.
#[test]
fn a_lamp_over_a_seam_lights_both_sides_alike() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let quality = QualityLevel::Low;
    let mut scene = wall(1.0, Vec::new(), quality);
    let tile = scene.draws[0];
    scene.draws = [-0.5f32, 0.5]
        .iter()
        .map(|&x| Draw { model: Mat4::from_translation(Vec3::new(x, 0.0, 0.0)) * tile.model, ..tile })
        .collect();
    // Far left, reaching the left tile only.
    let reds: Vec<Light> = (0..8)
        .map(|i| Light::point(Vec3::new(-0.9, -0.3 + 0.08 * i as f32, 0.2), 0.3, Vec3::new(4.0, 0.0, 0.0)))
        .collect();
    let green = Light::point(Vec3::new(0.0, 0.0, 0.2), 0.6, Vec3::new(0.0, 0.3, 0.0));
    scene.lights = [reds, vec![green]].concat();
    let (image, _) = render(&gpu, &mut r, &scene, 64, 64);
    // The seam is between pixels 31 and 32: 2.5 pixels either side.
    let (left, right) = (image.pixel(29, 32), image.pixel(34, 32));
    assert!(right[1] > 20, "the green lamp lights the right tile: {right:?}");
    assert!(left[1].abs_diff(right[1]) <= 3, "green left of the seam {left:?}, right {right:?}");
}
