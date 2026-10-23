//! Particles (stage 12 phases 1 and 2), checked on the real GPU: squares
//! of their size facing the camera, unlit, among the see-through surfaces
//! back to front, hidden behind what's solid, additive ones adding light;
//! stretched, flat, upright and mesh ones, texture sheet frames, soft,
//! lit, and drawn by a material's own shader.

mod common;

use common::*;
use fynkel_render::glam::{Mat4, Quat, Vec3};
use fynkel_render::*;
use std::sync::Arc;

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
    Particle {
        position,
        size,
        color: color.extend(alpha).into(),
        rotation: 0.0,
        velocity: Vec3::ZERO,
        frame: 0,
        t: 0.0,
        random: 0.0,
    }
}

/// `particles` drawn as one system with `material` into `scene`.
fn add(scene: &mut RenderScene, particles: &[Particle], material: PbrMaterial) {
    add_as(scene, particles, material, |d| d);
}

/// `particles` drawn as one system, its draw as `how` makes it.
fn add_as(
    scene: &mut RenderScene,
    particles: &[Particle],
    material: PbrMaterial,
    how: impl FnOnce(ParticleDraw) -> ParticleDraw,
) {
    let first = scene.particles.len() as u32;
    scene.particles.extend_from_slice(particles);
    let center = particles.iter().map(|p| p.position).sum::<Vec3>() / particles.len() as f32;
    let radius = particles.iter().map(|p| p.position.distance(center) + p.size * 2.0).fold(0.0, f32::max);
    let largest = particles.iter().map(|p| p.size).fold(0.0, f32::max);
    scene.particle_draws.push(how(ParticleDraw {
        material,
        mode: ParticleMode::Billboard,
        sheet: [1, 1],
        lit: false,
        softness: 0.0,
        largest,
        first,
        count: particles.len() as u32,
        center,
        radius,
    }));
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
                rotation: angle,
                ..particle(
                    Vec3::new(angle.cos() * 2.0, 0.4 + angle.sin() * 1.1, angle.sin() * 0.8),
                    0.2 + 0.4 * t,
                    Vec3::new(1.0 - t, 0.3, t),
                    0.4 + 0.6 * t,
                )
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

/// Whether a pixel shows anything over the black.
fn lit(pixel: [u8; 4]) -> bool {
    pixel[..3].iter().any(|&c| c > 8)
}

#[test]
fn a_stretched_particle_is_long_the_way_it_moves() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let mut s = scene(vec![], QualityLevel::High);
    let moving = Particle { velocity: Vec3::new(2.0, 0.0, 0.0), ..particle(Vec3::ZERO, 0.4, GREEN, 1.0) };
    add_as(&mut s, &[moving], plain(Blend::Transparent), |d| ParticleDraw {
        mode: ParticleMode::Stretched { length_scale: 1.0, speed_scale: 1.0 },
        ..d
    });
    let image = render(&gpu, &mut r, &s, 33, 33).0;
    // 0.4 + 2 m long, across; 0.4 m high. (A metre is about 6 pixels.)
    assert!(lit(image.pixel(16, 16)) && lit(image.pixel(22, 16)) && lit(image.pixel(10, 16)), "along");
    assert!(!lit(image.pixel(16, 20)) && !lit(image.pixel(16, 12)), "not up and down");
    // Still, it's a square of its size.
    let mut s = scene(vec![], QualityLevel::High);
    add_as(&mut s, &[particle(Vec3::ZERO, 0.4, GREEN, 1.0)], plain(Blend::Transparent), |d| ParticleDraw {
        mode: ParticleMode::Stretched { length_scale: 1.0, speed_scale: 1.0 },
        ..d
    });
    let image = render(&gpu, &mut r, &s, 33, 33).0;
    assert!(lit(image.pixel(16, 16)) && !lit(image.pixel(22, 16)), "still");
}

#[test]
fn a_flat_particle_lies_down_and_an_upright_one_faces_the_camera() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let one = [particle(Vec3::ZERO, 1.0, GREEN, 1.0)];
    let draw = |r: &mut Renderer, mode: ParticleMode, camera: Vec3| {
        let mut s = scene(vec![], QualityLevel::High);
        s.camera.as_mut().unwrap().position = camera;
        let mut view = CameraView::looking_at(camera, Vec3::ZERO);
        view.clear_color = [0.0; 3];
        s.camera = Some(view);
        add_as(&mut s, &one, plain(Blend::Transparent), |d| ParticleDraw { mode, ..d });
        render(&gpu, r, &s, 33, 33).0
    };
    let front = Vec3::new(0.0, 0.0, 5.0);
    let billboard = draw(&mut r, ParticleMode::Billboard, front);
    // Edge on, from the side, a flat one is hardly there; from above it's
    // its whole square.
    let edge = draw(&mut r, ParticleMode::Horizontal, front);
    assert!(lit(billboard.pixel(16, 14)) && !lit(edge.pixel(16, 14)), "edge on");
    let above = draw(&mut r, ParticleMode::Horizontal, Vec3::new(0.0, 5.0, 0.01));
    assert!(lit(above.pixel(16, 16)) && lit(above.pixel(16, 18)), "from above");
    // An upright one from the front is the billboard; from above, it's
    // edge on.
    assert_eq!(draw(&mut r, ParticleMode::Vertical, front).rgba, billboard.rgba);
    let upright_above = draw(&mut r, ParticleMode::Vertical, Vec3::new(0.0, 5.0, 0.01));
    assert!(!lit(upright_above.pixel(16, 18)), "upright, from above");
}

#[test]
fn a_texture_sheet_frame_shows_its_part_of_the_texture() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let quadrants = [[200, 40, 30, 255], [40, 180, 60, 255], [30, 60, 200, 255], [230, 230, 230, 255]];
    let mut pixels = Vec::new();
    for y in 0..4 {
        for x in 0..4 {
            pixels.extend_from_slice(&quadrants[(y / 2 * 2 + x / 2) as usize]);
        }
    }
    let texture = TextureKey(1);
    r.upload_texture(texture, ColorSpace::Srgb, 4, 4, &pixels);
    let material = PbrMaterial {
        textures: MaterialTextures { base_color: Some(texture), ..MaterialTextures::NONE },
        ..plain(Blend::Transparent)
    };
    for (frame, [cr, cg, cb, _]) in quadrants.into_iter().enumerate() {
        let mut s = scene(vec![], QualityLevel::High);
        let shown = Particle { frame: frame as u32, ..particle(Vec3::ZERO, 2.0, Vec3::ONE, 1.0) };
        add_as(&mut s, &[shown], material, |d| ParticleDraw { sheet: [2, 2], ..d });
        let image = render(&gpu, &mut r, &s, 33, 33).0;
        let want = Vec3::new(linear(cr as f32 / 255.0), linear(cg as f32 / 255.0), linear(cb as f32 / 255.0));
        assert_near(image.pixel(16, 16), tonemap(want), &format!("frame {frame}"));
    }
}

#[test]
fn a_soft_particle_fades_where_it_meets_what_is_behind_it() {
    let gpu = gpu();
    for quality in [QualityLevel::Low, QualityLevel::High] {
        let mut r = renderer(&gpu, SRGB);
        let wall = || vec![square(0.0, 4.0, glow(RED, 1.0, Blend::Opaque))];
        let near_wall = [particle(Vec3::new(0.0, 0.0, 0.1), 1.0, GREEN, 1.0)];
        let mut s = scene(wall(), quality);
        add(&mut s, &near_wall, plain(Blend::Transparent));
        let hard = render(&gpu, &mut r, &s, 33, 33).0.pixel(16, 16);
        assert_near(hard, GREEN, "hard");
        let mut s = scene(wall(), quality);
        add_as(&mut s, &near_wall, plain(Blend::Transparent), |d| ParticleDraw { softness: 1.0, ..d });
        let soft = render(&gpu, &mut r, &s, 33, 33).0.pixel(16, 16);
        // 0.1 m in front of the wall, with 1 m of softness: about a tenth.
        let want = to_srgb8(over(GREEN, 0.1, RED));
        for c in 0..3 {
            assert!(soft[c].abs_diff(want[c]) <= 12, "soft at {quality:?}: got {soft:?}, want {want:?}");
        }
        // Far from anything, it's as hard.
        let mut s = scene(vec![], quality);
        add_as(&mut s, &near_wall, plain(Blend::Transparent), |d| ParticleDraw { softness: 1.0, ..d });
        assert_near(render(&gpu, &mut r, &s, 33, 33).0.pixel(16, 16), GREEN, "over the sky");
    }
}

#[test]
fn a_lit_particle_takes_the_light_and_an_unlit_one_does_not() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let one = [particle(Vec3::ZERO, 1.0, Vec3::ONE, 1.0)];
    let material = PbrMaterial { base_color: [0.6, 0.6, 0.6, 1.0], roughness: 1.0, ..plain(Blend::Transparent) };
    let shown = |r: &mut Renderer, lit: bool, ambient: f32| {
        let mut s = scene(vec![], QualityLevel::High);
        s.ambient = Vec3::splat(ambient);
        add_as(&mut s, &one, material, |d| ParticleDraw { lit, ..d });
        render(&gpu, r, &s, 33, 33).0.pixel(16, 16)
    };
    assert_near(shown(&mut r, false, 0.0), tonemap(Vec3::splat(0.6)), "unlit, in the dark");
    assert_near(shown(&mut r, true, 0.0), Vec3::ZERO, "lit, in the dark");
    let lit_up = shown(&mut r, true, 1.0);
    assert!(lit_up[0] > 100 && lit_up[0] == lit_up[1], "lit by the ambient light: {lit_up:?}");
}

#[test]
fn a_mesh_particle_is_its_mesh_turned_round_up() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let shown = |r: &mut Renderer, turn: f32| {
        let mut s = scene(vec![], QualityLevel::High);
        let cube = Particle { rotation: turn, ..particle(Vec3::ZERO, 1.0, GREEN, 1.0) };
        add_as(&mut s, &[cube], plain(Blend::Transparent), |d| ParticleDraw { mode: ParticleMode::Mesh(CUBE), ..d });
        render(&gpu, r, &s, 65, 65)
    };
    let (square, stats) = shown(&mut r, 0.0);
    assert_eq!((stats.particles, stats.draw_calls), (1, 1));
    let across = |image: &Image| (0..65).filter(|&x| lit(image.pixel(x, 32))).count();
    let up = |image: &Image| (0..65).filter(|&y| lit(image.pixel(32, y))).count();
    let (turned, _) = shown(&mut r, 45f32.to_radians());
    // Turned round up, it's wider (its corner towards the camera) and as
    // high.
    assert!(across(&turned) > across(&square) + 2, "{} vs {}", across(&turned), across(&square));
    assert!(up(&turned).abs_diff(up(&square)) <= 2);
}

#[test]
fn a_materials_own_shader_draws_its_particles_and_knows_their_age_and_number() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let key = ShaderKey(1);
    let shader = Arc::new(
        CustomShader::new(
            "ages.wgsl",
            "fn unlit(in: SurfaceInput) -> vec4<f32> {\n    \
             return vec4<f32>(particle_random(), particle_age(), in.color.b, in.color.a);\n}\n",
        )
        .unwrap_or_else(|e| panic!("{e}")),
    );
    r.upload_shader(key, shader);
    let material = PbrMaterial {
        shader: Some(ShaderUse { shader: key, values: 0, textures: [None; MAX_TEXTURES], reach: 0.0 }),
        ..plain(Blend::Transparent)
    };
    let mut s = scene(vec![], QualityLevel::High);
    let aged = Particle { t: 0.5, random: 0.25, ..particle(Vec3::ZERO, 1.0, Vec3::new(0.0, 0.0, 0.3), 1.0) };
    add(&mut s, &[aged], material);
    let image = render(&gpu, &mut r, &s, 33, 33).0;
    assert_near(image.pixel(16, 16), tonemap(Vec3::new(0.25, 0.5, 0.3)), "random, age, and its colour");
}

/// Sparks, a texture sheet, lit smoke, a flat ring, soft mist and mesh
/// debris, over a floor in a sun's light: the golden.
#[test]
fn the_look_of_particles_matches_its_golden() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let mut pixels = Vec::new();
    for y in 0..8u32 {
        for x in 0..8u32 {
            let quadrant = (y / 4) * 2 + x / 4;
            let (cx, cy) = ((x % 4) as f32 - 1.5, (y % 4) as f32 - 1.5);
            let round = (1.0 - (cx * cx + cy * cy).sqrt() / (1.5 + quadrant as f32 * 0.2)).clamp(0.0, 1.0);
            pixels.extend_from_slice(&[255, 255, 255, (round * 255.0) as u8]);
        }
    }
    let puff = TextureKey(1);
    r.upload_texture(puff, ColorSpace::Srgb, 8, 8, &pixels);
    let mut camera = CameraView::looking_at(Vec3::new(0.0, 2.0, 7.0), Vec3::new(0.0, 0.5, 0.0));
    camera.clear_color = [0.05, 0.05, 0.08];
    let floor = Draw::new(
        PLANE,
        Mat4::from_scale_rotation_translation(Vec3::splat(10.0), Quat::IDENTITY, Vec3::new(0.0, -0.5, 0.0)),
        PbrMaterial { base_color: [0.25, 0.22, 0.2, 1.0], ..PbrMaterial::DEFAULT },
    );
    let mut s = RenderScene {
        camera: Some(camera),
        quality: Some(QualityLevel::High.quality()),
        lights: vec![Light::sun(Vec3::new(-0.3, -1.0, -0.4), Vec3::ONE * 2.0)],
        ambient: Vec3::splat(0.25),
        draws: vec![floor],
        ..Default::default()
    };
    let sparks: Vec<Particle> = (0..16)
        .map(|i| {
            let a = i as f32 / 16.0 * std::f32::consts::TAU;
            let v = Vec3::new(a.cos(), 1.0 + 0.5 * a.sin(), a.sin()) * 3.0;
            Particle {
                velocity: v,
                ..particle(Vec3::new(-2.0, 1.0, 0.0) + v * 0.15, 0.06, Vec3::new(1.0, 0.6, 0.2), 1.0)
            }
        })
        .collect();
    add_as(&mut s, &sparks, plain(Blend::Additive), |d| ParticleDraw {
        mode: ParticleMode::Stretched { length_scale: 1.0, speed_scale: 0.08 },
        ..d
    });
    let textured = |blend| PbrMaterial {
        textures: MaterialTextures { base_color: Some(puff), ..MaterialTextures::NONE },
        ..plain(blend)
    };
    let smoke: Vec<Particle> = (0..6)
        .map(|i| Particle {
            frame: i % 4,
            rotation: i as f32,
            ..particle(
                Vec3::new(0.2 * i as f32 - 0.5, 0.3 + 0.35 * i as f32, 0.0),
                0.8 + 0.1 * i as f32,
                Vec3::splat(0.8),
                0.8,
            )
        })
        .collect();
    add_as(&mut s, &smoke, textured(Blend::Transparent), |d| ParticleDraw { sheet: [2, 2], lit: true, ..d });
    let ring = [Particle { ..particle(Vec3::new(2.0, -0.45, 0.5), 1.5, Vec3::new(0.3, 0.6, 1.0), 0.7) }];
    add_as(&mut s, &ring, textured(Blend::Additive), |d| ParticleDraw { mode: ParticleMode::Horizontal, ..d });
    let mist = [particle(Vec3::new(2.0, -0.3, -1.0), 1.6, Vec3::new(0.7, 0.8, 0.9), 0.6)];
    add_as(&mut s, &mist, textured(Blend::Transparent), |d| ParticleDraw { softness: 0.5, ..d });
    let debris: Vec<Particle> = (0..5)
        .map(|i| Particle {
            rotation: i as f32 * 0.7,
            ..particle(
                Vec3::new(1.2 + 0.4 * i as f32, 0.6 + 0.3 * (i % 2) as f32, 1.0),
                0.25,
                Vec3::new(0.8, 0.5, 0.3),
                1.0,
            )
        })
        .collect();
    add_as(&mut s, &debris, PbrMaterial { roughness: 0.6, ..plain(Blend::Transparent) }, |d| ParticleDraw {
        mode: ParticleMode::Mesh(CUBE),
        lit: true,
        ..d
    });
    let (image, stats) = render(&gpu, &mut r, &s, 256, 160);
    assert_eq!(stats.particles, 16 + 6 + 1 + 1 + 5);
    assert_golden("particles_look", &image);
}
