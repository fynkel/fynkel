//! Particles (stage 12 phase 1), checked on the real GPU: squares of their
//! size facing the camera, unlit, among the see-through surfaces back to
//! front, hidden behind what's solid, additive ones adding light.

mod common;

use common::*;
use fynkel_render::glam::{Mat4, Quat, Vec3};
use fynkel_render::*;

const SRGB: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

const RED: Vec3 = Vec3::new(0.5, 0.0, 0.0);
const GREEN: Vec3 = Vec3::new(0.0, 0.5, 0.0);

/// A material that shows `glow` (black, unlit), `opacity` see-through by
/// `blend`, as blend.rs's.
fn glow(glow: Vec3, opacity: f32, blend: Blend) -> PbrMaterial {
    PbrMaterial { base_color: [0.0, 0.0, 0.0, opacity], emissive: glow.into(), blend, ..PbrMaterial::DEFAULT }
}

/// A material that shows its particles' colours as they are.
fn plain(blend: Blend) -> PbrMaterial {
    PbrMaterial { blend, ..PbrMaterial::DEFAULT }
}

fn particle(position: Vec3, size: f32, color: Vec3, alpha: f32) -> Particle {
    Particle { position, size, color: color.extend(alpha).into(), rotation: 0.0 }
}

/// `particles` drawn as one system with `material` into `scene`.
fn add(scene: &mut RenderScene, particles: &[Particle], material: PbrMaterial) {
    let first = scene.particles.len() as u32;
    scene.particles.extend_from_slice(particles);
    let center = particles.iter().map(|p| p.position).sum::<Vec3>() / particles.len() as f32;
    let radius = particles.iter().map(|p| p.position.distance(center) + p.size).fold(0.0, f32::max);
    scene.particle_draws.push(ParticleDraw { material, first, count: particles.len() as u32, center, radius });
}

/// A square `size` m across facing the camera, `z` m along Z.
fn square(z: f32, size: f32, material: PbrMaterial) -> Draw {
    let model = Mat4::from_translation(Vec3::new(0.0, 0.0, z))
        * Mat4::from_rotation_x(90f32.to_radians())
        * Mat4::from_scale(Vec3::splat(size));
    Draw { cast_shadows: false, ..Draw::new(PLANE, model, material) }
}

/// Seen from 5 m along +Z, on black, with no light.
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

fn over(front: Vec3, opacity: f32, behind: Vec3) -> Vec3 {
    front * opacity + behind * (1.0 - opacity)
}

#[test]
fn a_particle_is_a_square_of_its_size_facing_the_camera() {
    let gpu = gpu();
    for quality in [QualityLevel::Low, QualityLevel::Medium, QualityLevel::High] {
        let mut r = renderer(&gpu, SRGB);
        let surface =
            render(&gpu, &mut r, &scene(vec![square(0.0, 1.0, glow(GREEN, 0.5, Blend::Transparent))], quality), 33, 33)
                .0;
        let mut s = scene(vec![], quality);
        add(&mut s, &[particle(Vec3::ZERO, 1.0, Vec3::ONE, 1.0)], glow(GREEN, 0.5, Blend::Transparent));
        let (image, stats) = render(&gpu, &mut r, &s, 33, 33);
        assert_eq!((stats.particles, stats.draw_calls), (1, 1));
        assert_near(image.pixel(16, 16), over(GREEN, 0.5, Vec3::ZERO), &format!("its middle ({quality:?})"));
        let differ = surface.rgba.chunks(4).zip(image.rgba.chunks(4)).filter(|(a, b)| a != b).count();
        assert!(differ <= 4, "{differ} pixels differ from a square surface of its size ({quality:?})");
    }
}

#[test]
fn its_colour_times_the_materials_and_turned_it_stays_a_square() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let mut s = scene(vec![], QualityLevel::High);
    let material = PbrMaterial { base_color: [1.0, 0.5, 1.0, 1.0], ..plain(Blend::Transparent) };
    let turned = Particle { rotation: 45f32.to_radians(), ..particle(Vec3::ZERO, 1.0, Vec3::new(0.5, 0.4, 0.0), 1.0) };
    add(&mut s, &[turned], material);
    let image = render(&gpu, &mut r, &s, 33, 33).0;
    assert_near(image.pixel(16, 16), Vec3::new(0.5, 0.2, 0.0), "its colour times the material's");
    // Turned 45°, its corners point up and across: a pixel by a corner of
    // the unturned square is off it, one by its top corner on. (A metre
    // is about 6 pixels here.)
    assert_near(image.pixel(13, 13), Vec3::ZERO, "the unturned corner");
    assert_ne!(image.pixel(16, 12), [0, 0, 0, 255], "its top corner");
}

#[test]
fn particles_are_drawn_among_see_through_surfaces_back_to_front() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    // In front of the glass: over it.
    let mut s = scene(vec![square(0.0, 4.0, glow(RED, 0.5, Blend::Transparent))], QualityLevel::High);
    add(&mut s, &[particle(Vec3::new(0.0, 0.0, 1.0), 1.0, GREEN, 0.5)], plain(Blend::Transparent));
    let image = render(&gpu, &mut r, &s, 33, 33).0;
    assert_near(image.pixel(16, 16), over(GREEN, 0.5, over(RED, 0.5, Vec3::ZERO)), "in front of the glass");
    // Behind it: under it.
    let mut s = scene(vec![square(0.0, 4.0, glow(RED, 0.5, Blend::Transparent))], QualityLevel::High);
    add(&mut s, &[particle(Vec3::new(0.0, 0.0, -1.0), 2.0, GREEN, 0.5)], plain(Blend::Transparent));
    let image = render(&gpu, &mut r, &s, 33, 33).0;
    assert_near(image.pixel(16, 16), over(RED, 0.5, over(GREEN, 0.5, Vec3::ZERO)), "behind the glass");
}

#[test]
fn particles_are_hidden_behind_solid_things_and_additive_ones_add_light() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let mut s = scene(vec![square(0.0, 1.0, glow(RED, 1.0, Blend::Opaque))], QualityLevel::High);
    add(&mut s, &[particle(Vec3::new(0.0, 0.0, -1.0), 3.0, GREEN, 1.0)], plain(Blend::Transparent));
    let image = render(&gpu, &mut r, &s, 33, 33).0;
    assert_near(image.pixel(16, 16), RED, "behind the wall");
    assert_ne!(image.pixel(10, 16), [0, 0, 0, 255], "round it");

    let mut s = scene(vec![square(0.0, 4.0, glow(RED, 1.0, Blend::Opaque))], QualityLevel::High);
    add(&mut s, &[particle(Vec3::new(0.0, 0.0, 1.0), 1.0, GREEN, 0.5)], plain(Blend::Additive));
    let image = render(&gpu, &mut r, &s, 33, 33).0;
    assert_near(image.pixel(16, 16), RED + GREEN * 0.5, "added to the wall");
}

#[test]
fn a_system_out_of_view_is_culled_and_many_systems_draw() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let mut s = scene(vec![], QualityLevel::High);
    add(&mut s, &[particle(Vec3::new(0.0, 0.0, 10.0), 1.0, GREEN, 1.0)], plain(Blend::Transparent));
    add(&mut s, &[particle(Vec3::ZERO, 1.0, GREEN, 1.0); 3], plain(Blend::Additive));
    add(&mut s, &[particle(Vec3::new(1.0, 0.0, 0.0), 1.0, RED, 1.0); 2], plain(Blend::Transparent));
    let stats = render(&gpu, &mut r, &s, 33, 33).1;
    assert_eq!((stats.culled, stats.particles, stats.draw_calls), (1, 5, 2));
}

/// A ring of particles of every colour, size and turn, some see-through,
/// over a floor and in fog: the golden.
#[test]
fn a_ring_of_particles_matches_its_golden() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let mut camera = CameraView::looking_at(Vec3::new(0.0, 1.0, 6.0), Vec3::new(0.0, 0.3, 0.0));
    camera.clear_color = [0.05, 0.05, 0.08];
    let floor = Draw::new(
        PLANE,
        Mat4::from_scale_rotation_translation(Vec3::splat(10.0), Quat::IDENTITY, Vec3::new(0.0, -1.0, 0.0)),
        PbrMaterial { base_color: [0.2, 0.2, 0.25, 1.0], ..PbrMaterial::DEFAULT },
    );
    let mut s = RenderScene {
        camera: Some(camera),
        quality: Some(QualityLevel::High.quality()),
        lights: vec![Light::sun(Vec3::new(-0.3, -1.0, -0.4), Vec3::ONE)],
        ambient: Vec3::splat(0.3),
        draws: vec![floor],
        fog: Some(Fog { color: None, start: 4.0, end: 20.0, height: None }),
        ..Default::default()
    };
    let ring: Vec<Particle> = (0..24)
        .map(|i| {
            let t = i as f32 / 24.0;
            let angle = t * std::f32::consts::TAU;
            Particle {
                position: Vec3::new(angle.cos() * 2.0, 0.4 + angle.sin() * 1.1, angle.sin() * 0.8),
                size: 0.2 + 0.4 * t,
                color: [1.0 - t, 0.3, t, 0.4 + 0.6 * t],
                rotation: angle,
            }
        })
        .collect();
    add(&mut s, &ring[..12], plain(Blend::Transparent));
    add(&mut s, &ring[12..], PbrMaterial { emissive: [0.1, 0.05, 0.0], ..plain(Blend::Additive) });
    let (image, stats) = render(&gpu, &mut r, &s, 256, 160);
    assert_eq!(stats.particles, 24);
    assert_golden("particles_ring", &image);
}

/// A transparent system's particles go furthest first, whatever order
/// they're given in.
#[test]
fn a_transparent_systems_particles_are_drawn_back_to_front() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let near = particle(Vec3::new(0.0, 0.0, 1.0), 1.0, GREEN, 0.5);
    let far = particle(Vec3::new(0.0, 0.0, -1.0), 2.0, RED, 0.5);
    for given in [[near, far], [far, near]] {
        let mut s = scene(vec![], QualityLevel::High);
        add(&mut s, &given, plain(Blend::Transparent));
        let image = render(&gpu, &mut r, &s, 33, 33).0;
        assert_near(image.pixel(16, 16), over(GREEN, 0.5, over(RED, 0.5, Vec3::ZERO)), "near over far");
    }
}
