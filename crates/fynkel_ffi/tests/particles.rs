//! Particle systems through the C interface (stage 12 phases 1 and 3), the
//! way `package:fynkel` drives them: settings when they change, a step a
//! frame, commands, the done and collision events, sub emitters, trails,
//! and frames drawn.
#![allow(unsafe_code)]

mod common;

use std::ptr;

use common::*;
use fynkel_ffi::*;
use fynkel_golden::assert_golden;
use fynkel_world::{Transform, Vec3};

fn srgb(r: f32, g: f32, b: f32, a: f32) -> FyColor {
    FyColor { r, g, b, a }
}

fn step(world: &World, seconds: f32) -> Vec<FyParticleEvent> {
    let (dt, mut count, mut events) = (1.0f32 / 60.0, 0usize, Vec::new());
    for _ in 0..(seconds * 60.0).round() as u32 {
        // SAFETY: a live world and pointers to locals.
        unsafe {
            ok(fy_world_particles_step(world.0, &dt, &dt, &mut count));
            let mut got = vec![FyParticleEvent::default(); count];
            ok(fy_particle_events_get(world.0, got.as_mut_ptr(), count, &mut count));
            events.extend(got);
        }
    }
    events
}

fn state(world: &World, id: u64) -> FyParticleState {
    let mut out = FyParticleState::default();
    // SAFETY: a live world and a pointer to a local.
    unsafe { ok(fy_particles_get(world.0, id, &mut out)) };
    out
}

/// A burst of `count`, not looping, living `lifetime`.
fn burst(count: f32, lifetime: f32) -> FyParticleSettings {
    let mut bursts = [FyParticleBurst::default(); FY_PARTICLE_MAX_BURSTS as usize];
    bursts[0] = FyParticleBurst {
        time: 0.0,
        count: FyRange { min: count, max: count },
        cycles: 1,
        interval: 0.01,
        probability: 1.0,
    };
    FyParticleSettings {
        looping: false,
        duration: 1.0,
        rate_over_time: 0.0,
        start_lifetime: FyRange { min: lifetime, max: lifetime },
        burst_count: 1,
        bursts,
        seeded: true,
        seed: 3,
        ..FyParticleSettings::default()
    }
}

#[test]
fn a_burst_plays_and_its_stop_action_comes_back_once() {
    let world = World::new();
    let id = world.spawn();
    let settings = FyParticleSettings { stop_action: FY_PARTICLE_STOP_DESTROY, ..burst(25.0, 0.5) };
    // SAFETY: a live world and pointers to locals.
    unsafe { ok(fy_particles_set(world.0, id, &settings)) };
    assert!(step(&world, 0.1).is_empty());
    assert_eq!(state(&world, id), FyParticleState { count: 25, playing: true, paused: false, emitting: true });
    let events = step(&world, 1.5);
    let done = FyParticleEvent {
        object: id,
        kind: FY_PARTICLE_EVENT_DONE,
        action: FY_PARTICLE_STOP_DESTROY,
        ..Default::default()
    };
    assert_eq!(events, [done]);
    assert_eq!(state(&world, id).count, 0);
    assert!(!state(&world, id).playing);
}

#[test]
fn commands_play_pause_stop_clear_and_emit() {
    let world = World::new();
    let id = world.spawn();
    let settings = FyParticleSettings { play_on_awake: false, ..burst(10.0, 5.0) };
    // SAFETY: a live world and pointers to locals.
    unsafe {
        ok(fy_particles_set(world.0, id, &settings));
        step(&world, 0.1);
        assert_eq!(state(&world, id).count, 0);
        ok(fy_particles_command(world.0, id, FY_PARTICLES_EMIT, 3));
        assert_eq!(state(&world, id).count, 3);
        ok(fy_particles_command(world.0, id, FY_PARTICLES_PLAY, 0));
        step(&world, 0.05);
        assert_eq!(state(&world, id).count, 13);
        ok(fy_particles_command(world.0, id, FY_PARTICLES_PAUSE, 0));
        assert!(state(&world, id).paused);
        ok(fy_particles_command(world.0, id, FY_PARTICLES_CLEAR, 0));
        assert_eq!(state(&world, id).count, 0);
        // Played again while paused, it goes on, its burst gone by; from
        // a stop, it starts again.
        ok(fy_particles_command(world.0, id, FY_PARTICLES_STOP_AND_CLEAR, 0));
        ok(fy_particles_command(world.0, id, FY_PARTICLES_PLAY, 0));
        ok(fy_particles_simulate(world.0, id, &0.5));
        assert_eq!(state(&world, id).count, 10);
        ok(fy_particles_command(world.0, id, FY_PARTICLES_STOP_AND_CLEAR, 0));
        assert_eq!(state(&world, id), FyParticleState::default());
        assert_eq!(fy_particles_command(world.0, id, 99, 0), FyStatus::InvalidValue);
    }
}

#[test]
fn settings_that_cant_be_and_missing_things_are_errors() {
    let world = World::new();
    let id = world.spawn();
    // SAFETY: a live world and pointers to locals.
    unsafe {
        let wrong = [
            FyParticleSettings { space: 2, ..Default::default() },
            FyParticleSettings { stop_action: 9, ..Default::default() },
            FyParticleSettings { shape: FyParticleShape { kind: 7, ..Default::default() }, ..Default::default() },
            FyParticleSettings { burst_count: 9, ..Default::default() },
            FyParticleSettings { max_particles: 0, ..Default::default() },
            FyParticleSettings { start_size: FyRange { min: f32::NAN, max: 1.0 }, ..Default::default() },
        ];
        for settings in wrong {
            assert_eq!(fy_particles_set(world.0, id, &settings), FyStatus::InvalidValue, "{}", last_error());
        }
        let missing = FyParticleSettings { material: 77, ..Default::default() };
        assert_eq!(fy_particles_set(world.0, id, &missing), FyStatus::NoMaterial);
        assert_eq!(fy_particles_get(world.0, id, &mut FyParticleState::default()), FyStatus::NoComponent);
        assert_eq!(fy_particles_set(world.0, id, ptr::null()), FyStatus::NullPointer);
        ok(fy_particles_set(world.0, id, &FyParticleSettings::default()));
        ok(fy_particles_remove(world.0, id));
        assert_eq!(fy_particles_remove(world.0, id), FyStatus::NoComponent);
    }
}

/// A fountain made and stepped through the C interface, drawn: the
/// golden.
#[test]
fn a_fountain_drawn_through_the_c_interface_matches_its_golden() {
    let world = World::new();
    let w = world.0;
    // SAFETY: a live world and pointers to locals.
    unsafe {
        let camera = world.spawn();
        ok(fy_camera_set(
            w,
            camera,
            &FyCamera {
                fov_y: 50.0,
                near_clip: 0.1,
                far_clip: 100.0,
                clear_color: srgb(0.1, 0.1, 0.15, 1.0),
                size: 5.0,
                culling_mask: u32::MAX,
                ..Default::default()
            },
        ));
        let looking = Transform::from_position(Vec3::new(0.0, 2.0, 7.0)).looking_at(Vec3::new(0.0, 1.5, 0.0));
        ok(fy_transform_set(w, camera, &transform(looking)));

        let mut material = 0;
        let glow =
            FyMaterial { base_color: srgb(1.0, 0.8, 0.4, 0.8), blend: FY_BLEND_ADDITIVE, ..FyMaterial::default() };
        ok(fy_material_add(w, &glow, &mut material));
        let mut size_table = [0.0; FY_PARTICLE_TABLE as usize];
        let mut color_table = [srgb(1.0, 1.0, 1.0, 1.0); FY_PARTICLE_TABLE as usize];
        for i in 0..FY_PARTICLE_TABLE as usize {
            let t = i as f32 / (FY_PARTICLE_TABLE - 1) as f32;
            size_table[i] = 1.0 - 0.7 * t;
            color_table[i] = srgb(1.0, 1.0 - 0.6 * t, 1.0 - t, 1.0 - t);
        }
        let fountain = world.spawn();
        ok(fy_transform_set(w, fountain, &transform(Transform::default())));
        let settings = FyParticleSettings {
            start_speed: FyRange { min: 5.0, max: 6.0 },
            start_size: FyRange { min: 0.15, max: 0.3 },
            start_lifetime: FyRange { min: 1.0, max: 1.4 },
            gravity_modifier: 0.8,
            rate_over_time: 120.0,
            space: FY_PARTICLE_SPACE_WORLD,
            seeded: true,
            seed: 11,
            shape: FyParticleShape {
                kind: FY_PARTICLE_SHAPE_CONE,
                radius: 0.1,
                angle: 15f32.to_radians(),
                ..FyParticleSettings::default().shape
            },
            has_size_table: true,
            size_table,
            has_color_table: true,
            color_table,
            material,
            ..FyParticleSettings::default()
        };
        ok(fy_particles_set(w, fountain, &settings));
    }
    step(&world, 2.0);
    let view = View::headless(&world, 160, 160);
    assert_golden("particles_fountain", &view.read(view.frame(), 160, 160));
}

#[test]
fn planes_send_collision_events_and_sub_emitters_burst_where_particles_die() {
    let world = World::new();
    let w = world.0;
    let (shell, sparks, floor) = (world.spawn(), world.spawn(), world.spawn());
    // SAFETY: a live world and pointers to locals.
    unsafe {
        for id in [shell, sparks] {
            ok(fy_transform_set(w, id, &transform(Transform::default())));
        }
        ok(fy_transform_set(w, floor, &transform(Transform::from_position(Vec3::new(0.0, -1.0, 0.0)))));
        ok(fy_particles_set(w, sparks, &FyParticleSettings { rate_over_time: 100.0, ..burst(10.0, 5.0) }));
        let mut planes = [0; FY_PARTICLE_MAX_PLANES as usize];
        planes[0] = floor;
        let mut subs = [FyParticleSubEmitter::default(); FY_PARTICLE_MAX_SUB_EMITTERS as usize];
        subs[0] = FyParticleSubEmitter { system: sparks, kind: FY_PARTICLE_SUB_DEATH, inherit: 0, probability: 1.0 };
        let settings = FyParticleSettings {
            start_speed: FyRange { min: 0.0, max: 0.0 },
            gravity_modifier: 1.0,
            space: FY_PARTICLE_SPACE_WORLD,
            shape: FyParticleShape { kind: FY_PARTICLE_SHAPE_POINT, ..Default::default() },
            has_collision: true,
            collision: FyParticleCollision {
                plane_count: 1,
                planes,
                lifetime_loss: FyCurve::constant(1.0),
                send_messages: true,
                ..FyParticleSettings::default().collision
            },
            sub_emitter_count: 1,
            sub_emitters: subs,
            ..burst(4.0, 5.0)
        };
        ok(fy_particles_set(w, shell, &settings));
    }
    let events = step(&world, 0.3);
    assert!(events.is_empty(), "{events:?}");
    assert_eq!(state(&world, sparks).count, 0, "a sub emitter emits only for its parent");
    // A metre down from rest: about 0.45 s.
    let events = step(&world, 0.3);
    let hit = FyParticleEvent {
        object: shell,
        kind: FY_PARTICLE_EVENT_COLLISION,
        plane: floor,
        count: 4,
        ..Default::default()
    };
    assert_eq!(events, [hit]);
    assert_eq!((state(&world, shell).count, state(&world, sparks).count), (0, 40), "they die as they hit");

    // SAFETY: a live world and pointers to locals.
    unsafe {
        let too_many = FyParticleSettings { sub_emitter_count: 5, ..Default::default() };
        assert_eq!(fy_particles_set(w, shell, &too_many), FyStatus::InvalidValue);
        let mut own = FyParticleSettings { sub_emitter_count: 1, ..Default::default() };
        own.sub_emitters[0] = FyParticleSubEmitter { system: shell, probability: 1.0, ..Default::default() };
        assert_eq!(fy_particles_set(w, shell, &own), FyStatus::InvalidValue);
        let mut wrong_kind = FyParticleSettings { sub_emitter_count: 1, ..Default::default() };
        wrong_kind.sub_emitters[0] = FyParticleSubEmitter { system: sparks, kind: 9, ..Default::default() };
        assert_eq!(fy_particles_set(w, shell, &wrong_kind), FyStatus::InvalidValue);
        let planes = FyParticleSettings {
            has_collision: true,
            collision: FyParticleCollision { plane_count: 7, ..FyParticleSettings::default().collision },
            ..Default::default()
        };
        assert_eq!(fy_particles_set(w, shell, &planes), FyStatus::InvalidValue);
    }
}

/// A trail left by an object going round a curve, tapering and fading,
/// drawn: the golden.
#[test]
fn a_trail_drawn_through_the_c_interface_matches_its_golden() {
    let world = World::new();
    let w = world.0;
    let ball = world.spawn();
    // SAFETY: a live world and pointers to locals.
    unsafe {
        let camera = world.spawn();
        ok(fy_camera_set(
            w,
            camera,
            &FyCamera {
                fov_y: 50.0,
                near_clip: 0.1,
                far_clip: 100.0,
                clear_color: srgb(0.1, 0.1, 0.15, 1.0),
                size: 5.0,
                culling_mask: u32::MAX,
                ..Default::default()
            },
        ));
        let looking = Transform::from_position(Vec3::new(0.0, 4.0, 5.0)).looking_at(Vec3::new(0.0, 0.0, 0.0));
        ok(fy_transform_set(w, camera, &transform(looking)));
        let mut material = 0;
        let glow =
            FyMaterial { base_color: srgb(1.0, 1.0, 1.0, 1.0), blend: FY_BLEND_TRANSPARENT, ..FyMaterial::default() };
        ok(fy_material_add(w, &glow, &mut material));
        let mut width_table = [0.0; FY_PARTICLE_TABLE as usize];
        let mut color_table = [srgb(1.0, 1.0, 1.0, 1.0); FY_PARTICLE_TABLE as usize];
        for i in 0..FY_PARTICLE_TABLE as usize {
            let t = i as f32 / (FY_PARTICLE_TABLE - 1) as f32;
            width_table[i] = 1.0 - t;
            color_table[i] = srgb(1.0, 0.9 - 0.7 * t, 0.2 + 0.7 * t, 1.0 - t);
        }
        let settings = FyTrailSettings {
            time: 1.0,
            min_vertex_distance: 0.05,
            width: 0.4,
            has_width_table: true,
            width_table,
            has_color_table: true,
            color_table,
            material,
            ..FyTrailSettings::default()
        };
        ok(fy_trail_set(w, ball, &settings));
        // Round a circle of 1.5 m, once in two seconds.
        let dt = 1.0f32 / 60.0;
        let mut count = 0;
        for i in 0..=90 {
            let a = i as f32 * dt * std::f32::consts::PI;
            let at = Vec3::new(a.cos() * 1.5, 0.0, a.sin() * 1.5);
            ok(fy_transform_set(w, ball, &transform(Transform::from_position(at))));
            ok(fy_world_particles_step(w, &dt, &dt, &mut count));
        }
        let mut points = 0;
        ok(fy_trail_count(w, ball, &mut points));
        assert!((40..=62).contains(&points), "{points}");
        assert_eq!(fy_trail_set(w, ball, &FyTrailSettings { width: -1.0, ..settings }), FyStatus::InvalidValue);
    }
    let view = View::headless(&world, 160, 160);
    assert_golden("trail_arc", &view.read(view.frame(), 160, 160));
    // SAFETY: a live world and pointers to locals.
    unsafe {
        ok(fy_trail_clear(w, ball));
        let mut points = 1;
        ok(fy_trail_count(w, ball, &mut points));
        assert_eq!(points, 0);
        ok(fy_trail_remove(w, ball));
        assert_eq!(fy_trail_remove(w, ball), FyStatus::NoComponent);
    }
}
