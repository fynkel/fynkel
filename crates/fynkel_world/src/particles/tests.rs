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
    assert_eq!(events, [ParticleEvent { object: id, action: StopAction::Destroy }]);
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
