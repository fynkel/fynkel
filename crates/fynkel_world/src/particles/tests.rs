use super::*;
use crate::components::Transform;
use glam::Quat;

/// A world with one object carrying a system with `settings`, at the
/// origin.
fn world_with(settings: ParticleSettings) -> (World, ObjectId) {
    let mut world = World::new();
    let id = world.spawn();
    world.insert(id, Transform::default()).unwrap();
    world.set_particles(id, settings).unwrap();
    (world, id)
}

/// A system that only bursts, `count` at once, living `lifetime` seconds,
/// not looping.
fn burst(count: f32, lifetime: f32) -> ParticleSettings {
    ParticleSettings {
        looping: false,
        duration: 1.0,
        rate_over_time: 0.0,
        start_lifetime: Range::constant(lifetime),
        bursts: vec![Burst { count: Range::constant(count), ..Burst::default() }],
        seed: Some(7),
        ..ParticleSettings::default()
    }
}

fn step(world: &mut World, seconds: f32, dt: f32) -> Vec<ParticleEvent> {
    let mut events = Vec::new();
    let steps = (seconds / dt).round() as u32;
    for _ in 0..steps {
        world.step_particles(dt, dt, &mut events);
    }
    events
}

fn positions(world: &World, id: ObjectId) -> Vec<Vec3> {
    let mut out = Vec::new();
    world.particles(id).unwrap().drawn(world.world_matrix(id).unwrap(), &mut out);
    out.iter().map(|p| p.position).collect()
}

#[test]
fn a_burst_makes_its_count_on_the_first_frame() {
    let (mut world, id) = world_with(burst(30.0, 2.0));
    step(&mut world, 1.0 / 60.0, 1.0 / 60.0);
    assert_eq!(world.particles(id).unwrap().count(), 30);
}

#[test]
fn particles_die_at_their_lifetime_and_the_system_is_done_once() {
    let (mut world, id) = world_with(ParticleSettings { stop_action: StopAction::Destroy, ..burst(30.0, 0.5) });
    let events = step(&mut world, 0.4, 0.01);
    assert_eq!(world.particles(id).unwrap().count(), 30);
    assert!(events.is_empty());
    // The timeline is a second long: it's done after that, with its
    // particles long gone.
    let events = step(&mut world, 0.7, 0.01);
    assert_eq!(world.particles(id).unwrap().count(), 0);
    assert_eq!(events, [ParticleEvent { object: id, kind: ParticleEventKind::Done(StopAction::Destroy) }]);
    assert!(!world.particles(id).unwrap().is_playing());
    assert!(step(&mut world, 1.0, 0.01).is_empty(), "said once");
}

#[test]
fn the_rate_emits_so_many_a_second() {
    let (mut world, id) = world_with(ParticleSettings {
        rate_over_time: 50.0,
        start_lifetime: Range::constant(10.0),
        ..ParticleSettings::default()
    });
    step(&mut world, 2.0, 1.0 / 60.0);
    let n = world.particles(id).unwrap().count();
    assert!((99..=101).contains(&n), "{n}");
}

#[test]
fn particles_made_in_one_frame_are_spread_through_it() {
    // One long frame of 50: they're as old as they'd be if made through it,
    // so they're at different heights, not in a lump.
    let (mut world, id) = world_with(ParticleSettings {
        rate_over_time: 50.0,
        start_speed: Range::constant(1.0),
        start_lifetime: Range::constant(10.0),
        shape: Shape { kind: ShapeKind::Edge, radius: 0.0, ..Shape::default() },
        ..ParticleSettings::default()
    });
    step(&mut world, 1.0, 1.0);
    let mut heights: Vec<f32> = positions(&world, id).iter().map(|p| p.y).collect();
    heights.sort_by(f32::total_cmp);
    assert_eq!(heights.len(), 50);
    assert!(heights[0] < 0.05 && heights[49] > 0.95, "{heights:?}");
}

#[test]
fn a_seed_makes_the_same_particles() {
    let settings =
        ParticleSettings { shape: Shape { kind: ShapeKind::Sphere, ..Shape::default() }, ..burst(20.0, 2.0) };
    let (mut a, ia) = world_with(settings.clone());
    let (mut b, ib) = world_with(settings);
    step(&mut a, 0.5, 0.02);
    step(&mut b, 0.5, 0.02);
    assert_eq!(positions(&a, ia), positions(&b, ib));
    // Without one, two systems differ.
    let settings = ParticleSettings { seed: None, ..burst(20.0, 2.0) };
    let mut world = World::new();
    let [x, y] = std::array::from_fn(|_| world.spawn());
    world.set_particles(x, settings.clone()).unwrap();
    world.set_particles(y, settings).unwrap();
    step(&mut world, 0.1, 0.02);
    assert_ne!(positions(&world, x), positions(&world, y));
}

#[test]
fn world_space_leaves_particles_behind_and_local_space_takes_them_along() {
    for space in [Space::World, Space::Local] {
        let (mut world, id) = world_with(ParticleSettings {
            space,
            start_speed: Range::constant(0.0),
            shape: Shape { kind: ShapeKind::Point, ..Shape::default() },
            ..burst(5.0, 5.0)
        });
        step(&mut world, 0.1, 0.02);
        world.insert(id, Transform::from_position(Vec3::new(10.0, 0.0, 0.0))).unwrap();
        step(&mut world, 0.1, 0.02);
        let xs: Vec<f32> = positions(&world, id).iter().map(|p| p.x).collect();
        let expected = if space == Space::World { 0.0 } else { 10.0 };
        assert!(xs.iter().all(|x| (x - expected).abs() < 1e-4), "{space:?}: {xs:?}");
    }
}

#[test]
fn gravity_pulls_them_down_in_the_world_however_the_object_is_turned() {
    let (mut world, id) =
        world_with(ParticleSettings { gravity_modifier: 1.0, start_speed: Range::constant(0.0), ..burst(1.0, 5.0) });
    // Upside down, in local space: down is still the world's down.
    let turned = Transform { rotation: Quat::from_rotation_x(std::f32::consts::PI), ..Transform::default() };
    world.insert(id, turned).unwrap();
    step(&mut world, 1.0, 0.001);
    let y = positions(&world, id)[0].y;
    assert!((y + GRAVITY / 2.0).abs() < 0.02, "{y}");
}

#[test]
fn a_system_holds_no_more_than_its_max() {
    let (mut world, id) = world_with(ParticleSettings { max_particles: 10, ..burst(30.0, 2.0) });
    step(&mut world, 0.1, 0.02);
    assert_eq!(world.particles(id).unwrap().count(), 10);
}

#[test]
fn size_and_colour_follow_their_tables_over_a_life() {
    let mut sizes = [0.0; TABLE];
    let mut colors = [Color::WHITE; TABLE];
    for (i, (s, c)) in sizes.iter_mut().zip(colors.iter_mut()).enumerate() {
        let t = i as f32 / (TABLE - 1) as f32;
        *s = 1.0 + t;
        c.a = 1.0 - t;
    }
    let (mut world, id) = world_with(ParticleSettings {
        start_size: Range::constant(2.0),
        size_over_lifetime: Some(Box::new(sizes)),
        color_over_lifetime: Some(Box::new(colors)),
        ..burst(1.0, 1.0)
    });
    step(&mut world, 0.5, 0.01);
    let mut out = Vec::new();
    world.particles(id).unwrap().drawn(Mat4::IDENTITY, &mut out);
    assert!((out[0].size - 3.0).abs() < 0.05, "{}", out[0].size);
    assert!((out[0].color[3] - 0.5).abs() < 0.02, "{:?}", out[0].color);
}

#[test]
fn paused_nothing_moves_and_unscaled_time_goes_on() {
    let settings = ParticleSettings { start_speed: Range::constant(1.0), ..burst(1.0, 5.0) };
    let (mut world, id) = world_with(settings.clone());
    step(&mut world, 0.1, 0.02);
    world.particle_command(id, ParticleCommand::Pause).unwrap();
    let before = positions(&world, id);
    step(&mut world, 0.5, 0.02);
    assert_eq!(before, positions(&world, id));
    assert!(world.particles(id).unwrap().is_paused());
    world.particle_command(id, ParticleCommand::Play).unwrap();
    step(&mut world, 0.1, 0.02);
    assert_ne!(before, positions(&world, id));

    // The game paused: its time doesn't pass, but an unscaled system's does.
    let (mut world, id) = world_with(ParticleSettings { unscaled_time: true, ..settings });
    let mut events = Vec::new();
    world.step_particles(0.02, 0.02, &mut events);
    let before = positions(&world, id);
    world.step_particles(0.0, 0.02, &mut events);
    assert_ne!(before, positions(&world, id));
}

#[test]
fn stopped_it_emits_no_more_and_its_particles_live_on_unless_cleared() {
    let (mut world, id) = world_with(ParticleSettings {
        rate_over_time: 100.0,
        start_lifetime: Range::constant(1.0),
        ..ParticleSettings::default()
    });
    step(&mut world, 1.5, 0.01);
    world.particle_command(id, ParticleCommand::Stop { clear: false }).unwrap();
    let n = world.particles(id).unwrap().count();
    step(&mut world, 0.2, 0.01);
    assert!(world.particles(id).unwrap().count() < n);
    assert!(world.particles(id).unwrap().count() > 0);
    assert!(!world.particles(id).unwrap().is_emitting());
    assert!(world.particles(id).unwrap().is_playing(), "until the last one's gone");
    step(&mut world, 1.0, 0.01);
    assert!(!world.particles(id).unwrap().is_playing());

    world.particle_command(id, ParticleCommand::Play).unwrap();
    step(&mut world, 0.5, 0.01);
    world.particle_command(id, ParticleCommand::Stop { clear: true }).unwrap();
    assert_eq!(world.particles(id).unwrap().count(), 0);
}

#[test]
fn a_system_that_doesnt_play_on_awake_waits_and_emit_works_without_playing() {
    let (mut world, id) = world_with(ParticleSettings { play_on_awake: false, ..burst(10.0, 1.0) });
    step(&mut world, 0.5, 0.01);
    assert_eq!(world.particles(id).unwrap().count(), 0);
    assert!(!world.particles(id).unwrap().is_playing());
    world.particle_command(id, ParticleCommand::Emit(4)).unwrap();
    assert_eq!(world.particles(id).unwrap().count(), 4);
    step(&mut world, 0.5, 0.01);
    assert_eq!(world.particles(id).unwrap().count(), 4, "they live their lives");
    world.particle_command(id, ParticleCommand::Play).unwrap();
    step(&mut world, 0.02, 0.02);
    assert_eq!(world.particles(id).unwrap().count(), 14);
}

#[test]
fn hidden_it_clears_and_shown_again_it_plays_from_the_start() {
    let (mut world, id) = world_with(burst(10.0, 5.0));
    step(&mut world, 0.1, 0.02);
    assert_eq!(world.particles(id).unwrap().count(), 10);
    world.set_active(id, false).unwrap();
    step(&mut world, 0.1, 0.02);
    assert_eq!(world.particles(id).unwrap().count(), 0);
    world.set_active(id, true).unwrap();
    step(&mut world, 0.02, 0.02);
    assert_eq!(world.particles(id).unwrap().count(), 10, "the burst again");
}

#[test]
fn a_looping_timeline_bursts_each_cycle() {
    let (mut world, id) = world_with(ParticleSettings { looping: true, ..burst(5.0, 10.0) });
    step(&mut world, 2.5, 0.01);
    assert_eq!(world.particles(id).unwrap().count(), 15);
}

#[test]
fn prewarmed_it_starts_full() {
    let (mut world, id) = world_with(ParticleSettings {
        prewarm: true,
        duration: 2.0,
        rate_over_time: 10.0,
        start_lifetime: Range::constant(2.0),
        ..ParticleSettings::default()
    });
    step(&mut world, 0.02, 0.02);
    let n = world.particles(id).unwrap().count();
    assert!((18..=21).contains(&n), "{n}");
}

#[test]
fn a_start_delay_holds_the_first_particle() {
    let (mut world, id) = world_with(ParticleSettings { start_delay: 0.5, ..burst(3.0, 2.0) });
    step(&mut world, 0.4, 0.02);
    assert_eq!(world.particles(id).unwrap().count(), 0);
    step(&mut world, 0.2, 0.02);
    assert_eq!(world.particles(id).unwrap().count(), 3);
}

#[test]
fn settings_that_cant_be_are_refused_and_change_nothing() {
    let (mut world, id) = world_with(burst(3.0, 2.0));
    for bad in [
        ParticleSettings { max_particles: 0, ..ParticleSettings::default() },
        ParticleSettings { duration: f32::NAN, ..ParticleSettings::default() },
        ParticleSettings { start_lifetime: Range::constant(-1.0), ..ParticleSettings::default() },
        ParticleSettings { bursts: vec![Burst::default(); 9], ..ParticleSettings::default() },
        ParticleSettings {
            shape: Shape { angle: std::f32::consts::FRAC_PI_2, ..Shape::default() },
            ..ParticleSettings::default()
        },
    ] {
        assert!(matches!(world.set_particles(id, bad), Err(WorldError::Particles(_))));
    }
    assert_eq!(world.particles(id).unwrap().settings().bursts.len(), 1);
}

#[test]
fn a_material_in_use_by_a_system_cant_be_removed_and_despawning_takes_the_system() {
    let mut world = World::new();
    let material = world.add_material(crate::Material::default()).unwrap();
    let id = world.spawn();
    world.set_particles(id, ParticleSettings { material: Some(material), ..burst(3.0, 2.0) }).unwrap();
    assert!(matches!(world.remove_material(material), Err(WorldError::MaterialInUse { .. })));
    world.despawn(id).unwrap();
    assert!(!world.has_particles());
    world.remove_material(material).unwrap();
}

#[test]
fn shapes_put_particles_where_they_should() {
    let mut rng = Rng::new(3);
    let cone = Shape { kind: ShapeKind::Cone, radius: 2.0, ..Shape::default() };
    let hemisphere = Shape { kind: ShapeKind::Hemisphere, radius: 1.0, radius_thickness: 0.0, ..Shape::default() };
    let r#box = Shape { kind: ShapeKind::Box, size: Vec3::new(2.0, 4.0, 6.0), ..Shape::default() };
    for _ in 0..1000 {
        let (p, d) = shape_point(&cone, &mut rng);
        assert!(p.y == 0.0 && p.length() <= 2.0 + 1e-4 && d.y >= cone.angle.cos() - 1e-4);
        let (p, d) = shape_point(&hemisphere, &mut rng);
        assert!((p.length() - 1.0).abs() < 1e-4 && p.y >= 0.0 && d.y >= 0.0);
        let (p, _) = shape_point(&r#box, &mut rng);
        assert!(p.abs().cmple(Vec3::new(1.0, 2.0, 3.0)).all());
    }
}

#[test]
fn a_steady_frame_allocates_nothing() {
    // The pool is made when the settings are set; drawing into a buffer
    // that has grown makes nothing either.
    let (mut world, id) = world_with(ParticleSettings {
        rate_over_time: 200.0,
        start_lifetime: Range::constant(1.0),
        ..ParticleSettings::default()
    });
    let mut events = Vec::with_capacity(4);
    let mut out = Vec::with_capacity(1000);
    for _ in 0..120 {
        world.step_particles(1.0 / 60.0, 1.0 / 60.0, &mut events);
    }
    let pointer = world.particles(id).unwrap().position.as_ptr();
    for _ in 0..120 {
        world.step_particles(1.0 / 60.0, 1.0 / 60.0, &mut events);
        out.clear();
        world.particles(id).unwrap().drawn(Mat4::IDENTITY, &mut out);
    }
    assert_eq!(pointer, world.particles(id).unwrap().position.as_ptr());
    assert!(out.capacity() == 1000);
}

/// One particle, standing still at the origin, living `lifetime` seconds,
/// no gravity: a module's effect on it alone.
fn still_one(lifetime: f32) -> ParticleSettings {
    ParticleSettings {
        shape: Shape { kind: ShapeKind::Point, ..Shape::default() },
        start_speed: Range::constant(0.0),
        gravity_modifier: 0.0,
        ..burst(1.0, lifetime)
    }
}

fn one(world: &World, id: ObjectId) -> DrawnParticle {
    let mut out = Vec::new();
    world.particles(id).unwrap().drawn(world.world_matrix(id).unwrap(), &mut out);
    out[0]
}

fn ramp(from: f32, to: f32) -> Box<[[f32; TABLE]; 2]> {
    let table = std::array::from_fn(|i| from + (to - from) * i as f32 / (TABLE - 1) as f32);
    Box::new([table; 2])
}

#[test]
fn a_curve_picks_between_its_two_by_a_particles_own_number() {
    assert_eq!(Curve::Constant(Range { min: 2.0, max: 4.0 }).at(0.3, 0.5), 3.0);
    let curves = Curve::Curves(Box::new([[0.0; TABLE], [10.0; TABLE]]));
    assert_eq!(curves.at(0.7, 0.25), 2.5);
    assert_eq!(Curve::Curves(ramp(0.0, 1.0)).at(0.5, 0.9), 0.5);
    // Each particle's numbers stay put, and differ from use to use.
    assert_eq!(pick(42, 3), pick(42, 3));
    assert_ne!(pick(42, 3), pick(42, 4));
    let mean = (0..10_000).map(|s| pick(s * 7919, 1)).sum::<f32>() / 10_000.0;
    assert!((mean - 0.5).abs() < 0.02, "{mean}");
}

#[test]
fn velocity_over_lifetime_moves_without_changing_the_velocity() {
    let (mut world, id) = world_with(ParticleSettings {
        velocity_over_lifetime: Some(VectorOverLifetime {
            x: Curve::constant(2.0),
            y: Curve::constant(0.0),
            z: Curve::constant(0.0),
            space: Space::World,
        }),
        ..still_one(5.0)
    });
    step(&mut world, 1.0, 0.01);
    let p = one(&world, id);
    assert!((p.position.x - 2.0).abs() < 0.05, "{:?}", p.position);
    // Drawn, it goes the way it moves.
    assert!((p.velocity - Vec3::X * 2.0).length() < 1e-4);
    assert_eq!(world.particles(id).unwrap().velocity[0], Vec3::ZERO);
}

#[test]
fn a_local_velocity_over_lifetime_turns_with_the_object() {
    let (mut world, id) = world_with(ParticleSettings {
        space: Space::World,
        velocity_over_lifetime: Some(VectorOverLifetime {
            x: Curve::constant(1.0),
            y: Curve::constant(0.0),
            z: Curve::constant(0.0),
            space: Space::Local,
        }),
        ..still_one(5.0)
    });
    // Turned a quarter round up, its x is the world's -z.
    world
        .insert(id, Transform { rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2), ..Transform::default() })
        .unwrap();
    step(&mut world, 1.0, 0.01);
    let p = one(&world, id).position;
    assert!((p - Vec3::new(0.0, 0.0, -1.0)).length() < 0.05, "{p:?}");
}

#[test]
fn force_over_lifetime_speeds_it_up() {
    let (mut world, id) = world_with(ParticleSettings {
        force_over_lifetime: Some(VectorOverLifetime {
            x: Curve::constant(0.0),
            y: Curve::constant(4.0),
            z: Curve::constant(0.0),
            space: Space::World,
        }),
        ..still_one(5.0)
    });
    step(&mut world, 1.0, 0.001);
    let p = one(&world, id);
    assert!((p.velocity.y - 4.0).abs() < 0.01);
    assert!((p.position.y - 2.0).abs() < 0.02, "{:?}", p.position);
}

#[test]
fn a_speed_limit_holds_and_drag_slows_the_same_at_any_frame_rate() {
    let limited = |dampen: f32, drag: f32| ParticleSettings {
        start_speed: Range::constant(10.0),
        limit_velocity: Some(LimitVelocity { limit: Curve::constant(3.0), dampen, drag: Curve::constant(drag) }),
        ..still_one(5.0)
    };
    let (mut world, id) = world_with(limited(1.0, 0.0));
    step(&mut world, 0.1, 0.01);
    assert!((one(&world, id).velocity.length() - 3.0).abs() < 1e-3);

    let speed_after = |dt: f32| {
        let (mut world, id) = world_with(limited(0.5, 1.0));
        step(&mut world, 1.0, dt);
        one(&world, id).velocity.length()
    };
    let (fast, slow) = (speed_after(1.0 / 120.0), speed_after(1.0 / 30.0));
    assert!((fast - slow).abs() < 0.02, "{fast} {slow}");
    assert!(fast < 3.0);
}

#[test]
fn rotation_over_lifetime_turns_it() {
    let (mut world, id) =
        world_with(ParticleSettings { rotation_over_lifetime: Some(Curve::constant(2.0)), ..still_one(5.0) });
    step(&mut world, 0.5, 0.01);
    assert!((one(&world, id).rotation - 1.0).abs() < 0.03);
}

#[test]
fn size_and_colour_by_speed_follow_how_fast_it_goes() {
    let (mut world, id) = world_with(ParticleSettings {
        start_speed: Range::constant(5.0),
        start_size: Range::constant(1.0),
        size_by_speed: Some(BySpeed { table: Box::new(ramp(1.0, 3.0)[0]), min_speed: 0.0, max_speed: 10.0 }),
        color_by_speed: Some(BySpeed {
            table: Box::new([Color::rgba(1.0, 1.0, 1.0, 0.5); TABLE]),
            min_speed: 0.0,
            max_speed: 1.0,
        }),
        ..still_one(5.0)
    });
    step(&mut world, 0.01, 0.01);
    let p = one(&world, id);
    assert!((p.size - 2.0).abs() < 1e-3);
    assert!((p.color[3] - 0.5).abs() < 1e-3);
}

#[test]
fn noise_moves_particles_apart_smoothly() {
    let (mut world, id) = world_with(ParticleSettings {
        noise: Some(Noise { strength: Curve::constant(1.0), frequency: 0.5, scroll_speed: 0.3, octaves: 2 }),
        shape: Shape { kind: ShapeKind::Box, size: Vec3::splat(4.0), ..Shape::default() },
        ..ParticleSettings { start_speed: Range::constant(0.0), gravity_modifier: 0.0, ..burst(50.0, 5.0) }
    });
    let before = positions(&world, id);
    step(&mut world, 1.0 / 60.0, 1.0 / 60.0);
    let before = if before.is_empty() { positions(&world, id) } else { before };
    step(&mut world, 1.0, 1.0 / 60.0);
    let after = positions(&world, id);
    let moved: Vec<f32> = before.iter().zip(&after).map(|(a, b)| (*a - *b).length()).collect();
    assert!(moved.iter().all(|&m| m < 1.01), "{moved:?}");
    assert!(moved.iter().sum::<f32>() / moved.len() as f32 > 0.05, "{moved:?}");
    // Smooth: near points, near pushes.
    let n = Noise { strength: Curve::constant(1.0), frequency: 1.0, scroll_speed: 0.0, octaves: 1 };
    let (a, b) = (noise_at(&n, Vec3::new(0.3, 0.2, 0.1), 0.0), noise_at(&n, Vec3::new(0.31, 0.2, 0.1), 0.0));
    assert!((a - b).length() < 0.05);
}

#[test]
fn a_texture_sheet_plays_its_frames_over_a_life() {
    let sheet = TextureSheet { columns: 4, rows: 2, ..TextureSheet::default() };
    let frames: Vec<u32> = [0.0, 0.2, 0.5, 0.99].iter().map(|&t| sheet_frame(&sheet, t, 9)).collect();
    assert_eq!(frames, [0, 1, 4, 7]);
    let twice = TextureSheet { cycles: 2.0, ..sheet.clone() };
    assert_eq!(sheet_frame(&twice, 0.5, 9), 0);
    // A random row plays its own row's four.
    let rows = TextureSheet { random_row: true, ..sheet.clone() };
    for seed in 0..50 {
        let start = sheet_frame(&rows, 0.0, seed);
        assert!(start == 0 || start == 4);
        assert_eq!(sheet_frame(&rows, 0.99, seed), start + 3);
    }
    let (mut world, id) = world_with(ParticleSettings { texture_sheet: Some(sheet), ..still_one(1.0) });
    step(&mut world, 0.55, 0.01);
    assert_eq!(one(&world, id).frame, 4);
}

#[test]
fn a_stretched_particle_reaches_past_its_size_in_its_box() {
    let (mut world, id) = world_with(ParticleSettings {
        start_speed: Range::constant(10.0),
        start_size: Range::constant(0.1),
        render_mode: RenderMode::Stretched { length_scale: 1.0, speed_scale: 0.1 },
        ..still_one(5.0)
    });
    step(&mut world, 0.01, 0.01);
    let mut out = Vec::new();
    let (lo, hi) = world.particles(id).unwrap().drawn(Mat4::IDENTITY, &mut out).unwrap();
    assert!(hi.y - lo.y >= 0.2 - 1e-4, "{lo:?} {hi:?}");
}

#[test]
fn a_mesh_drawn_by_a_system_must_exist_and_cant_be_removed() {
    let mut world = World::new();
    let id = world.spawn();
    let gone = world.add_mesh(fynkel_mesh::MeshData::cube());
    world.remove_mesh(gone).unwrap();
    let settings = |mesh| ParticleSettings { render_mode: RenderMode::Mesh(mesh), ..burst(3.0, 2.0) };
    assert!(matches!(world.set_particles(id, settings(gone)), Err(WorldError::NoMesh(_))));
    let mesh = world.add_mesh(fynkel_mesh::MeshData::cube());
    world.set_particles(id, settings(mesh)).unwrap();
    assert!(matches!(world.remove_mesh(mesh), Err(WorldError::MeshInUse { .. })));
    world.despawn(id).unwrap();
    world.remove_mesh(mesh).unwrap();
}

#[test]
fn phase_two_settings_that_cant_be_are_refused() {
    let (mut world, id) = world_with(burst(3.0, 2.0));
    let nan = Curve::constant(f32::NAN);
    for bad in [
        ParticleSettings { rotation_over_lifetime: Some(nan.clone()), ..ParticleSettings::default() },
        ParticleSettings {
            limit_velocity: Some(LimitVelocity {
                limit: Curve::constant(1.0),
                dampen: 2.0,
                drag: Curve::constant(0.0),
            }),
            ..ParticleSettings::default()
        },
        ParticleSettings {
            size_by_speed: Some(BySpeed { table: Box::new([1.0; TABLE]), min_speed: 2.0, max_speed: 1.0 }),
            ..ParticleSettings::default()
        },
        ParticleSettings {
            noise: Some(Noise { strength: Curve::constant(1.0), frequency: 1.0, scroll_speed: 0.0, octaves: 5 }),
            ..ParticleSettings::default()
        },
        ParticleSettings {
            texture_sheet: Some(TextureSheet { columns: 0, ..TextureSheet::default() }),
            ..ParticleSettings::default()
        },
        ParticleSettings { softness: -1.0, ..ParticleSettings::default() },
        ParticleSettings {
            render_mode: RenderMode::Stretched { length_scale: -1.0, speed_scale: 0.0 },
            ..ParticleSettings::default()
        },
    ] {
        assert!(matches!(world.set_particles(id, bad), Err(WorldError::Particles(_))));
    }
}

#[test]
fn a_steady_frame_with_every_module_allocates_nothing() {
    let curves = Curve::Curves(ramp(0.0, 1.0));
    let vector =
        || VectorOverLifetime { x: curves.clone(), y: Curve::constant(0.5), z: curves.clone(), space: Space::Local };
    let (mut world, id) = world_with(ParticleSettings {
        rate_over_time: 200.0,
        start_lifetime: Range::constant(1.0),
        velocity_over_lifetime: Some(vector()),
        force_over_lifetime: Some(vector()),
        limit_velocity: Some(LimitVelocity { limit: Curve::constant(2.0), dampen: 0.5, drag: curves.clone() }),
        rotation_over_lifetime: Some(curves.clone()),
        size_by_speed: Some(BySpeed { table: Box::new([1.0; TABLE]), min_speed: 0.0, max_speed: 1.0 }),
        noise: Some(Noise { strength: curves.clone(), frequency: 1.0, scroll_speed: 1.0, octaves: 3 }),
        texture_sheet: Some(TextureSheet { columns: 4, rows: 4, random_row: true, ..TextureSheet::default() }),
        render_mode: RenderMode::Stretched { length_scale: 2.0, speed_scale: 0.1 },
        ..ParticleSettings::default()
    });
    let mut events = Vec::with_capacity(4);
    let mut out = Vec::with_capacity(1000);
    for _ in 0..120 {
        world.step_particles(1.0 / 60.0, 1.0 / 60.0, &mut events);
    }
    let pointer = world.particles(id).unwrap().random.as_ptr();
    for _ in 0..120 {
        world.step_particles(1.0 / 60.0, 1.0 / 60.0, &mut events);
        out.clear();
        world.particles(id).unwrap().drawn(Mat4::IDENTITY, &mut out);
    }
    assert_eq!(pointer, world.particles(id).unwrap().random.as_ptr());
    assert!(out.capacity() == 1000);
}

/// `id` moved to `position`.
fn put(world: &mut World, id: ObjectId, position: Vec3) {
    world.insert(id, Transform { position, ..Transform::default() }).unwrap();
}

fn velocities(world: &World, id: ObjectId) -> Vec<Vec3> {
    let mut out = Vec::new();
    world.particles(id).unwrap().drawn(world.world_matrix(id).unwrap(), &mut out);
    out.iter().map(|p| p.velocity).collect()
}

/// A floor: an object at the origin, its up the plane's normal.
fn floor(world: &mut World) -> ObjectId {
    let id = world.spawn();
    world.insert(id, Transform::default()).unwrap();
    id
}

#[test]
fn rate_over_distance_leaves_particles_along_the_way_it_went() {
    let (mut world, id) = world_with(ParticleSettings {
        rate_over_time: 0.0,
        rate_over_distance: 10.0,
        start_speed: Range::constant(0.0),
        space: Space::World,
        shape: Shape { kind: ShapeKind::Point, ..Shape::default() },
        seed: Some(1),
        ..ParticleSettings::default()
    });
    step(&mut world, 1.0 / 60.0, 1.0 / 60.0);
    assert_eq!(world.particles(id).unwrap().count(), 0, "standing still, none");
    for k in 1..=20 {
        put(&mut world, id, Vec3::new(k as f32 * 0.1, 0.0, 0.0));
        step(&mut world, 1.0 / 60.0, 1.0 / 60.0);
    }
    let xs: Vec<f32> = positions(&world, id).iter().map(|p| p.x).collect();
    // Two metres, ten a metre (a hair short of 2 in adding up the tenths).
    assert!((19..=20).contains(&xs.len()), "{}", xs.len());
    assert!(xs.iter().all(|x| (0.0..=2.0).contains(x)));
    let spread = xs.iter().fold(0.0f32, |m, x| m.max(*x)) - xs.iter().fold(2.0f32, |m, x| m.min(*x));
    assert!(spread > 1.8, "left along the way: {xs:?}");
}

#[test]
fn particles_inherit_their_objects_velocity_at_birth_or_as_it_goes() {
    let settings = |mode| ParticleSettings {
        rate_over_time: 60.0,
        start_speed: Range::constant(0.0),
        space: Space::World,
        shape: Shape { kind: ShapeKind::Point, ..Shape::default() },
        inherit_velocity: Some(InheritVelocity { mode, curve: Curve::constant(1.0) }),
        seed: Some(1),
        ..ParticleSettings::default()
    };
    let (mut world, id) = world_with(settings(InheritMode::Initial));
    for k in 0..30 {
        put(&mut world, id, Vec3::new(k as f32 * 0.05, 0.0, 0.0));
        step(&mut world, 1.0 / 60.0, 1.0 / 60.0);
    }
    // 0.05 m a 60th: 3 m/s; the first ones were made standing still.
    let fast = velocities(&world, id).iter().filter(|v| (v.x - 3.0).abs() < 1e-3).count();
    assert!(fast >= 25, "{:?}", velocities(&world, id));

    // Current: carried along while the object moves, left when it stops.
    let (mut world, id) = world_with(settings(InheritMode::Current));
    for k in 0..30 {
        put(&mut world, id, Vec3::new(k as f32 * 0.05, 0.0, 0.0));
        step(&mut world, 1.0 / 60.0, 1.0 / 60.0);
    }
    let before = positions(&world, id);
    step(&mut world, 0.5, 1.0 / 60.0);
    let after = positions(&world, id);
    assert!(velocities(&world, id).iter().all(|v| *v == Vec3::ZERO), "their own velocity untouched");
    assert!(after.iter().zip(&before).all(|(a, b)| (a.x - b.x).abs() < 1e-4), "stopped, so they stay");
}

#[test]
fn particles_bounce_off_a_plane_losing_what_the_collision_says() {
    let mut world = World::new();
    let ground = floor(&mut world);
    let id = world.spawn();
    put(&mut world, id, Vec3::new(0.0, 2.0, 0.0));
    let settings = |collision: Collision| ParticleSettings {
        rate_over_time: 0.0,
        start_speed: Range::constant(0.0),
        start_size: Range::constant(0.2),
        start_lifetime: Range::constant(10.0),
        gravity_modifier: 1.0,
        space: Space::World,
        shape: Shape { kind: ShapeKind::Point, ..Shape::default() },
        collision: Some(Collision { planes: vec![ground], ..collision }),
        ..ParticleSettings::default()
    };
    world.set_particles(id, settings(Collision { bounce: Curve::constant(0.5), ..Collision::default() })).unwrap();
    world.particle_command(id, ParticleCommand::Emit(1)).unwrap();
    // Falling 2 m from rest: about 6.2 m/s when it meets the floor.
    let mut lowest = f32::INFINITY;
    let mut up = 0.0f32;
    for _ in 0..90 {
        step(&mut world, 1.0 / 120.0, 1.0 / 120.0);
        lowest = lowest.min(positions(&world, id)[0].y);
        up = up.max(velocities(&world, id)[0].y);
    }
    assert!(lowest >= 0.1 - 1e-4, "never below its radius: {lowest}");
    assert!((2.9..3.3).contains(&up), "half its speed back: {up}");

    // Losing all its lifetime, it dies when it hits; so does one slower
    // than the kill speed.
    for collision in [
        Collision { lifetime_loss: Curve::constant(1.0), ..Collision::default() },
        Collision { bounce: Curve::constant(0.1), min_kill_speed: 1.0, ..Collision::default() },
    ] {
        world.set_particles(id, settings(collision)).unwrap();
        world.particle_command(id, ParticleCommand::Clear).unwrap();
        world.particle_command(id, ParticleCommand::Emit(1)).unwrap();
        step(&mut world, 0.5, 1.0 / 120.0);
        assert_eq!(world.particles(id).unwrap().count(), 1, "still falling");
        step(&mut world, 0.3, 1.0 / 120.0);
        assert_eq!(world.particles(id).unwrap().count(), 0);
    }
}

#[test]
fn hits_are_told_by_plane_with_send_messages_and_a_tilted_plane_bounces_its_way() {
    let mut world = World::new();
    let wall = world.spawn();
    // A wall at x = 1, facing back along -x: its up turned to -x.
    world
        .insert(
            wall,
            Transform {
                position: Vec3::new(1.0, 0.0, 0.0),
                rotation: Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
                ..Transform::default()
            },
        )
        .unwrap();
    let (id, settings) = (
        world.spawn(),
        ParticleSettings {
            rate_over_time: 0.0,
            start_speed: Range::constant(4.0),
            start_size: Range::constant(0.0),
            start_lifetime: Range::constant(10.0),
            space: Space::Local,
            shape: Shape { kind: ShapeKind::Point, ..Shape::default() },
            collision: Some(Collision { planes: vec![wall], send_messages: true, ..Collision::default() }),
            ..ParticleSettings::default()
        },
    );
    // Emitting along +x: the object turned so its up is +x.
    world
        .insert(id, Transform { rotation: Quat::from_rotation_z(-std::f32::consts::FRAC_PI_2), ..Transform::default() })
        .unwrap();
    world
        .set_particles(
            id,
            ParticleSettings {
                shape: Shape { kind: ShapeKind::Cone, angle: 0.0, radius: 0.0, ..Shape::default() },
                ..settings
            },
        )
        .unwrap();
    world.particle_command(id, ParticleCommand::Emit(5)).unwrap();
    // 4 m/s: at the wall in a quarter of a second.
    let events = step(&mut world, 0.4, 1.0 / 120.0);
    assert_eq!(events, [ParticleEvent { object: id, kind: ParticleEventKind::Collision { plane: wall, count: 5 } }]);
    for v in velocities(&world, id) {
        assert!((v - Vec3::new(-4.0, 0.0, 0.0)).length() < 1e-3, "straight back: {v}");
    }
    assert!(positions(&world, id).iter().all(|p| p.x <= 1.0 + 1e-4));
}

/// A parent bursting `count` at the origin, each living `lifetime`, and a
/// sub system that bursts 10 (and would emit 100 a second on its own).
fn parent_and_sub(kind: SubEmitterKind, count: f32, lifetime: f32) -> (World, ObjectId, ObjectId) {
    let mut world = World::new();
    let sub = world.spawn();
    world.insert(sub, Transform::default()).unwrap();
    world
        .set_particles(
            sub,
            ParticleSettings {
                rate_over_time: 100.0,
                bursts: vec![Burst { count: Range::constant(10.0), ..Burst::default() }],
                start_lifetime: Range::constant(5.0),
                start_speed: Range::constant(0.0),
                space: Space::World,
                shape: Shape { kind: ShapeKind::Point, ..Shape::default() },
                seed: Some(2),
                ..ParticleSettings::default()
            },
        )
        .unwrap();
    let parent = world.spawn();
    world.insert(parent, Transform::default()).unwrap();
    world
        .set_particles(
            parent,
            ParticleSettings {
                start_speed: Range::constant(2.0),
                space: Space::World,
                sub_emitters: vec![SubEmitter { system: sub, kind, inherit: 0, probability: 1.0 }],
                ..burst(count, lifetime)
            },
        )
        .unwrap();
    (world, parent, sub)
}

#[test]
fn a_death_sub_emitter_bursts_where_each_particle_dies_and_never_on_its_own() {
    let (mut world, parent, sub) = parent_and_sub(SubEmitterKind::Death, 5.0, 0.5);
    step(&mut world, 0.4, 0.01);
    assert_eq!(world.particles(sub).unwrap().count(), 0, "a sub emitter emits only for its parent");
    let ends = positions(&world, parent);
    step(&mut world, 0.2, 0.01);
    assert_eq!(world.particles(parent).unwrap().count(), 0);
    let made = positions(&world, sub);
    assert_eq!(made.len(), 50, "ten for each of five");
    // Each where a parent was, a little further on.
    for p in &made {
        assert!(ends.iter().any(|e| (*p - *e).length() < 0.3), "{p} from {ends:?}");
    }
    // Its own particles going doesn't stop or destroy anything.
    let events = step(&mut world, 6.0, 0.05);
    assert!(events.is_empty(), "{events:?}");
}

#[test]
fn a_birth_sub_emitter_emits_from_each_particle_all_its_life() {
    let (mut world, parent, sub) = parent_and_sub(SubEmitterKind::Birth, 3.0, 2.0);
    step(&mut world, 0.5, 0.01);
    assert_eq!(world.particles(parent).unwrap().count(), 3);
    // 100 a second from each of three, for half a second.
    let n = world.particles(sub).unwrap().count();
    assert!((140..=152).contains(&n), "{n}");
}

#[test]
fn a_collision_sub_emitter_bursts_where_they_hit_and_inherits_what_it_takes() {
    let (mut world, parent, sub) = parent_and_sub(SubEmitterKind::Collision, 4.0, 5.0);
    let ground = world.spawn();
    world.insert(ground, Transform { position: Vec3::new(0.0, -1.0, 0.0), ..Transform::default() }).unwrap();
    let mut settings = world.particles(parent).unwrap().settings().clone();
    settings.start_speed = Range::constant(0.0);
    settings.start_size = Range::constant(0.5);
    settings.start_color = [Color::rgb(1.0, 0.0, 0.0); 2];
    settings.gravity_modifier = 1.0;
    // Dying as they hit, so each hits once.
    settings.collision =
        Some(Collision { planes: vec![ground], lifetime_loss: Curve::constant(1.0), ..Collision::default() });
    settings.sub_emitters[0].inherit = INHERIT_COLOR | INHERIT_SIZE;
    world.set_particles(parent, settings).unwrap();
    world.particle_command(parent, ParticleCommand::Play).unwrap();
    step(&mut world, 0.3, 0.01);
    assert_eq!(world.particles(sub).unwrap().count(), 0, "still falling");
    step(&mut world, 0.3, 0.01);
    assert_eq!(world.particles(sub).unwrap().count(), 40, "ten where each of four hit");
    let mut drawn = Vec::new();
    world.particles(sub).unwrap().drawn(Mat4::IDENTITY, &mut drawn);
    for p in &drawn {
        assert!((p.position.y - -0.75).abs() < 0.05, "on the floor, a radius up: {}", p.position.y);
        assert_eq!((p.color, p.size), ([1.0, 0.0, 0.0, 1.0], 0.5), "its parent's colour and size");
    }
}

#[test]
fn sub_emitters_and_planes_are_refused_when_they_cant_be() {
    let mut world = World::new();
    let id = world.spawn();
    let gone = world.spawn();
    world.despawn(gone).unwrap();
    let sub = |system| SubEmitter { system, kind: SubEmitterKind::Death, inherit: 0, probability: 1.0 };
    assert!(world.set_particles(id, ParticleSettings { sub_emitters: vec![sub(id)], ..Default::default() }).is_err());
    assert!(world.set_particles(id, ParticleSettings { sub_emitters: vec![sub(gone)], ..Default::default() }).is_err());
    let planes = |planes: Vec<ObjectId>| ParticleSettings {
        collision: Some(Collision { planes, ..Collision::default() }),
        ..ParticleSettings::default()
    };
    assert!(world.set_particles(id, planes(vec![gone])).is_err());
    assert!(world.set_particles(id, planes(vec![id; 7])).is_err());
    assert!(world.set_particles(id, planes(vec![id; 6])).is_ok());
    let bad = ParticleSettings {
        collision: Some(Collision { dampen: Curve::constant(2.0), ..Collision::default() }),
        ..ParticleSettings::default()
    };
    assert!(world.set_particles(id, bad).is_err());
}

/// Runs `f`, failing if it takes longer than `most` (stage 14, WO6: huge
/// counts and spans ran for seconds to minutes, holding the world).
fn quickly(most: std::time::Duration, what: &str, f: impl FnOnce()) {
    let start = std::time::Instant::now();
    f();
    assert!(start.elapsed() < most, "{what} took {:?}", start.elapsed());
}

#[test]
fn huge_counts_and_spans_take_no_longer_than_what_fits() {
    let ms = std::time::Duration::from_millis;
    // Emit, a burst and a sub-emitter's burst of billions, on a pool of 10.
    let small = ParticleSettings { max_particles: 10, rate_over_time: 0.0, play_on_awake: false, ..Default::default() };
    let (mut world, id) = world_with(small.clone());
    quickly(ms(50), "Emit(u32::MAX)", || world.particle_command(id, ParticleCommand::Emit(u32::MAX)).unwrap());
    assert_eq!(world.particles(id).unwrap().count(), 10);
    let (mut world, id) = world_with(ParticleSettings { max_particles: 10, ..burst(4e9, 1.0) });
    quickly(ms(50), "a burst of 4e9", || {
        step(&mut world, 1.0 / 60.0, 1.0 / 60.0);
    });
    assert_eq!(world.particles(id).unwrap().count(), 10);
    // A teleport of 10,000 km leaving 100 a metre.
    let (mut world, id) = world_with(ParticleSettings { rate_over_distance: 100.0, ..small.clone() });
    world.particle_command(id, ParticleCommand::Play).unwrap();
    step(&mut world, 1.0 / 60.0, 1.0 / 60.0);
    world.insert(id, Transform { position: Vec3::new(1e7, 0.0, 0.0), ..Transform::default() }).unwrap();
    quickly(ms(50), "a 10,000 km teleport", || {
        step(&mut world, 1.0 / 60.0, 1.0 / 60.0);
    });
    // Simulating ten million seconds, and a prewarm of a long loop.
    let steady = ParticleSettings { rate_over_time: 50.0, ..Default::default() };
    let (mut world, id) = world_with(steady.clone());
    step(&mut world, 1.0 / 60.0, 1.0 / 60.0);
    quickly(ms(500), "simulate(1e7)", || world.simulate_particles(id, 1e7).unwrap());
    let count = world.particles(id).unwrap().count();
    assert!(count.abs_diff(50 * 5) <= 2, "{count}: the steady state is 50 a second living 5 s");
    let long = ParticleSettings { prewarm: true, duration: 100_000.0, ..steady };
    let (mut world, id) = world_with(long.clone());
    quickly(ms(500), "a prewarm of 100,000 s", || {
        step(&mut world, 1.0 / 60.0, 1.0 / 60.0);
    });
    assert!(world.particles(id).unwrap().count() > 0);
    // Past Unity's longest, refused.
    let (mut world, id) = world_with(ParticleSettings::default());
    assert!(world.set_particles(id, ParticleSettings { duration: 1e9, ..long }).is_err());
}

// Stage 14's WO7: a prewarm ran before the planes were found, so its
// particles fell through the floor and the first step threw them up at
// the speed they'd gained (19 m/s from 1 m).
#[test]
fn a_prewarm_bounces_off_the_planes_already() {
    let mut world = World::new();
    let ground = floor(&mut world);
    let id = world.spawn();
    put(&mut world, id, Vec3::Y);
    let settings = ParticleSettings {
        prewarm: true,
        duration: 2.0,
        rate_over_time: 20.0,
        start_speed: Range::constant(0.0),
        start_lifetime: Range::constant(2.0),
        space: Space::World,
        shape: Shape { kind: ShapeKind::Point, ..Shape::default() },
        collision: Some(Collision { planes: vec![ground], ..Collision::default() }),
        seed: Some(3),
        ..ParticleSettings::default()
    };
    world.set_particles(id, ParticleSettings { gravity_modifier: 1.0, ..settings.clone() }).unwrap();
    step(&mut world, 1.0 / 60.0, 1.0 / 60.0);
    // Dropped from 1 m with a full bounce, none goes up faster than
    // the square root of 2 g, 4.43 m/s.
    let fastest = velocities(&world, id).iter().map(|v| v.y).fold(f32::NEG_INFINITY, f32::max);
    assert!(fastest < 4.6, "thrown up at {fastest} m/s");
    // Played again by hand, the same.
    world.particle_command(id, ParticleCommand::Play).unwrap();
    let fastest = velocities(&world, id).iter().map(|v| v.y).fold(f32::NEG_INFINITY, f32::max);
    assert!(fastest < 4.6, "played again, thrown up at {fastest} m/s");
}

// Stage 14's WO7: a prewarm's deaths fired the sub emitter on the first
// frame, 30 at once.
#[test]
fn a_prewarm_fires_no_sub_emitter() {
    let (mut world, parent, sub) = parent_and_sub(SubEmitterKind::Death, 0.0, 0.5);
    world
        .set_particles(
            sub,
            ParticleSettings {
                rate_over_time: 0.0,
                max_particles: 10_000,
                ..world.particles(sub).unwrap().settings.clone()
            },
        )
        .unwrap();
    let looping = ParticleSettings {
        prewarm: true,
        looping: true,
        duration: 2.0,
        rate_over_time: 20.0,
        bursts: Vec::new(),
        ..world.particles(parent).unwrap().settings.clone()
    };
    world.set_particles(parent, looping).unwrap();
    step(&mut world, 1.0 / 60.0, 1.0 / 60.0);
    // A steady parent loses 20 a second, a third of one a frame: each
    // death bursts 10.
    let first = world.particles(sub).unwrap().count();
    assert!(first <= 10, "{first} on the first frame");
}

// Stage 14's WO5: a local system on a stretched object turned the plane's
// normal by the inverse, not its transpose, so the plane leant the wrong
// way and particles stopped on the wrong side.
#[test]
fn a_plane_seen_from_a_stretched_local_system_is_where_it_is_drawn() {
    let mut world = World::new();
    let plane = world.spawn();
    let tilt = Quat::from_rotation_z(std::f32::consts::FRAC_PI_4);
    world.insert(plane, Transform { rotation: tilt, ..Transform::default() }).unwrap();
    let id = world.spawn();
    world.insert(id, Transform { scale: Vec3::new(4.0, 1.0, 1.0), ..Transform::default() }).unwrap();
    let collision = Some(Collision { planes: vec![plane], ..Collision::default() });
    world
        .set_particles(
            id,
            ParticleSettings { space: Space::Local, rate_over_time: 0.0, collision, ..Default::default() },
        )
        .unwrap();
    step(&mut world, 1.0 / 60.0, 1.0 / 60.0);
    let (point, normal, _) = world.particles(id).unwrap().planes[0];
    // Local points either side: the system must say the side the world
    // does.
    let m = world.world_matrix(id).unwrap();
    let up = tilt * Vec3::Y;
    for local in [Vec3::new(1.0, 0.3, 0.0), Vec3::new(-0.2, 0.5, 0.0), Vec3::new(0.3, -1.0, 0.0)] {
        let in_world = m.transform_point3(local).dot(up);
        let seen = (local - point).dot(normal);
        assert_eq!(in_world > 0.0, seen > 0.0, "{local}: the world says {in_world}, the system {seen}");
    }
    assert!((normal.length() - 1.0).abs() < 1e-5);
}
