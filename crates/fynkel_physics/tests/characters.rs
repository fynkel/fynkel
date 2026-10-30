//! Character controllers (docs/stage-5.md, step 7): up steps below the
//! step offset and not above, up slopes below the slope limit, along
//! walls, grounded, and pushing bodies.

use fynkel_physics::*;

const DT: f32 = 1.0 / 60.0;

fn spec(shape: ColliderShape) -> ColliderSpec {
    ColliderSpec { shape, center: Vector::ZERO, is_trigger: false, layer: 0, material: Material::DEFAULT, listens: 0 }
}

fn at(x: f32, y: f32, z: f32) -> ObjectPose {
    ObjectPose { position: Vector::new(x, y, z), ..ObjectPose::IDENTITY }
}

/// A box `size` across with its middle at `middle`, turned `degrees` about Z.
fn block(physics: &mut Physics, key: u64, size: Vector, middle: Vector, degrees: f32) -> u64 {
    let object =
        ObjectPose { position: middle, rotation: Rotation::from_rotation_z(degrees.to_radians()), scale: Vector::ONE };
    physics.add_static_collider(key, spec(ColliderShape::Box { size }), object).unwrap()
}

/// A world with a floor, its top at y = 0.
fn floor() -> Physics {
    let mut physics = Physics::new();
    block(&mut physics, 100, Vector::new(100.0, 1.0, 100.0), Vector::new(0.0, -0.5, 0.0), 0.0);
    physics
}

/// A character: a capsule 2 m tall, 0.5 m round, standing at (x, z).
struct Character {
    id: u64,
    object: ObjectPose,
    settings: CharacterSettings,
    hits: Vec<CharacterHit>,
}

impl Character {
    fn new(physics: &mut Physics, x: f32, z: f32) -> Self {
        let settings = CharacterSettings::default();
        let object = at(x, 1.0 + settings.skin_width, z);
        let shape = ColliderShape::Capsule { radius: 0.5, height: 2.0, direction: Axis::Y };
        let id = physics.add_static_collider(1, spec(shape), object).unwrap();
        Self { id, object, settings, hits: Vec::new() }
    }

    /// Moves it by `motion`, as Unity's `Move` does, and its object with it.
    fn go(&mut self, physics: &mut Physics, motion: Vector) -> CharacterMove {
        let moved = physics.move_character(self.id, motion, DT, self.settings, &mut self.hits).unwrap();
        self.object.position += moved.translation;
        physics.follow(&[self.id], &[self.object]).unwrap();
        moved
    }

    /// Walks it `seconds` at `velocity`, falling as gravity would have it.
    fn walk(&mut self, physics: &mut Physics, velocity: Vector, seconds: f32) -> CharacterMove {
        let mut fall = 0.0;
        let mut last = None;
        for _ in 0..(seconds / DT) as usize {
            let moved = self.go(physics, (velocity + Vector::new(0.0, fall, 0.0)) * DT);
            fall = if moved.flags & COLLIDED_BELOW != 0 { -1.0 } else { fall - 9.81 * DT };
            physics.step(DT).unwrap();
            last = Some(moved);
        }
        last.expect("it walked")
    }

    fn position(&self) -> Vector {
        self.object.position
    }

    /// How high its feet are.
    fn feet(&self) -> f32 {
        self.position().y - 1.0 - self.settings.skin_width
    }
}

#[test]
fn it_walks_up_a_step_below_its_step_offset_and_not_one_above() {
    for (height, climbs) in [(0.2, true), (0.28, true), (0.4, false), (0.6, false)] {
        let mut physics = floor();
        block(&mut physics, 2, Vector::new(4.0, height, 4.0), Vector::new(3.0, height / 2.0, 0.0), 0.0);
        let mut character = Character::new(&mut physics, 0.0, 0.0);
        character.walk(&mut physics, Vector::new(2.0, 0.0, 0.0), 2.0);
        let (x, feet) = (character.position().x, character.feet());
        if climbs {
            assert!(x > 2.0, "a {height} m step: it stopped at x = {x}");
            assert!((feet - height).abs() < 0.05, "a {height} m step: its feet are at {feet}");
        } else {
            // The step's side is at x = 1: it stops with its radius and skin from it.
            assert!(x < 0.5 && x > 0.3, "a {height} m step: it went to x = {x}");
            assert!(feet.abs() < 0.05, "a {height} m step: its feet are at {feet}");
        }
    }
}

#[test]
fn it_walks_up_a_slope_below_its_slope_limit_and_not_one_above() {
    for (degrees, climbs) in [(20.0_f32, true), (40.0, true), (50.0, false), (70.0, false)] {
        let mut physics = floor();
        // A ramp rising along +X from x = 2, 10 m long, its bottom edge under the floor.
        let slope = degrees.to_radians();
        let (length, thick) = (10.0, 1.0);
        let middle = Vector::new(2.0, 0.0, 0.0)
            + Vector::new(slope.cos(), slope.sin(), 0.0) * (length / 2.0)
            + Vector::new(slope.sin(), -slope.cos(), 0.0) * (thick / 2.0);
        block(&mut physics, 2, Vector::new(length, thick, 4.0), middle, degrees);
        let mut character = Character::new(&mut physics, 0.0, 0.0);
        character.walk(&mut physics, Vector::new(2.0, 0.0, 0.0), 3.0);
        let feet = character.feet();
        if climbs {
            assert!(feet > 1.0, "a {degrees}° slope: its feet are at {feet}");
        } else {
            assert!(feet < 0.5, "a {degrees}° slope: it went up to {feet}");
        }
    }
}

#[test]
fn it_slides_along_a_wall() {
    let mut physics = floor();
    // A wall along X, its face at z = 1.
    block(&mut physics, 2, Vector::new(40.0, 4.0, 1.0), Vector::new(0.0, 2.0, 1.5), 0.0);
    let mut character = Character::new(&mut physics, 0.0, 0.0);
    let last = character.walk(&mut physics, Vector::new(2.0, 0.0, 2.0), 2.0);
    let p = character.position();
    // Its whole way along X, and no further into the wall than its radius and skin.
    assert!(p.x > 3.5, "x = {}", p.x);
    assert!(p.z < 0.5 && p.z > 0.35, "z = {}", p.z);
    assert!(last.flags & COLLIDED_SIDES != 0, "flags {}", last.flags);
    assert!(character.hits.iter().any(|h| h.key == 2 && h.normal.z < -0.9), "{:?}", character.hits);
}

#[test]
fn it_knows_when_it_is_grounded_and_what_is_above() {
    let mut physics = floor();
    let mut character = Character::new(&mut physics, 0.0, 0.0);
    let standing = character.go(&mut physics, Vector::new(0.0, -0.1, 0.0));
    assert_eq!(standing.flags, COLLIDED_BELOW);
    assert!(character.hits.iter().any(|h| h.key == 100 && h.normal.y > 0.99), "{:?}", character.hits);
    // Lifted 3 m: nothing under it.
    character.object.position.y += 3.0;
    physics.follow(&[character.id], &[character.object]).unwrap();
    let falling = character.go(&mut physics, Vector::new(0.0, -0.1, 0.0));
    assert_eq!(falling.flags, 0);
    assert!(character.hits.is_empty());
    // A ceiling 0.2 m over its head: it goes up to it, and hits it.
    let top = character.position().y + 1.0;
    block(&mut physics, 3, Vector::new(10.0, 1.0, 10.0), Vector::new(0.0, top + 0.2 + 0.5, 0.0), 0.0);
    let jumping = character.go(&mut physics, Vector::new(0.0, 1.0, 0.0));
    assert_eq!(jumping.flags, COLLIDED_ABOVE);
    assert!(jumping.translation.y > 0.05 && jumping.translation.y < 0.2, "{}", jumping.translation.y);
    // Falling back, it lands.
    let landed = character.walk(&mut physics, Vector::ZERO, 2.0);
    assert!(landed.flags & COLLIDED_BELOW != 0);
    assert!(character.feet().abs() < 0.02, "feet at {}", character.feet());
}

#[test]
fn it_pushes_a_dynamic_body_and_a_massless_one_pushes_nothing() {
    for mass in [70.0, 0.0] {
        let mut physics = floor();
        let crate_ =
            physics.add_rigidbody(2, BodySpec::default(), Pose::from_translation(Vector::new(2.0, 0.5, 0.0))).unwrap();
        let unit = ColliderShape::Box { size: Vector::ONE };
        let crate_object = at(2.0, 0.5, 0.0);
        physics.add_object_collider(2, spec(unit), crate_object, Some(crate_)).unwrap();
        let mut character = Character::new(&mut physics, 0.0, 0.0);
        character.settings.mass = mass;
        character.walk(&mut physics, Vector::new(1.5, 0.0, 0.0), 3.0);
        let x = physics.body_pose(crate_).unwrap().translation.x;
        if mass > 0.0 {
            assert!(x > 3.0, "pushed to {x}");
            assert!(character.position().x > 2.0, "it went to {}", character.position().x);
        } else {
            assert!((x - 2.0).abs() < 0.01, "a massless character moved it to {x}");
            assert!(character.position().x < 1.0, "it went to {}", character.position().x);
        }
    }
}

#[test]
fn it_goes_through_triggers_and_layers_it_doesnt_meet() {
    let mut physics = floor();
    let mut trigger = spec(ColliderShape::Box { size: Vector::new(1.0, 4.0, 4.0) });
    trigger.is_trigger = true;
    physics.add_static_collider(2, trigger, at(2.0, 2.0, 0.0)).unwrap();
    let mut other_layer = spec(ColliderShape::Box { size: Vector::new(1.0, 4.0, 4.0) });
    other_layer.layer = 5;
    physics.add_static_collider(3, other_layer, at(4.0, 2.0, 0.0)).unwrap();
    physics.set_layer_collision(0, 5, false).unwrap();
    let mut character = Character::new(&mut physics, 0.0, 0.0);
    character.walk(&mut physics, Vector::new(2.0, 0.0, 0.0), 3.0);
    assert!(character.position().x > 5.5, "it stopped at {}", character.position().x);
    // With the layers meeting again, the wall stops it.
    physics.set_layer_collision(0, 5, true).unwrap();
    let mut character = Character::new(&mut physics, 0.0, 2.0);
    character.walk(&mut physics, Vector::new(2.0, 0.0, 0.0), 3.0);
    assert!(character.position().x < 3.0, "it went through to {}", character.position().x);
}

#[test]
fn it_enters_a_static_trigger_as_a_kinematic_body_would() {
    let mut physics = floor();
    let mut coin = spec(ColliderShape::Sphere { radius: 0.5 });
    coin.is_trigger = true;
    coin.listens = LISTEN_TRIGGERS;
    physics.add_static_collider(2, coin, at(2.0, 1.0, 0.0)).unwrap();
    let mut character = Character::new(&mut physics, 0.0, 0.0);
    let mut entered = false;
    for _ in 0..60 {
        character.go(&mut physics, Vector::new(2.0 * DT, 0.0, 0.0));
        physics.step(DT).unwrap();
        entered |= physics.contact_events().iter().any(|e| e.kind == ContactKind::Enter && e.trigger);
    }
    assert!(entered, "it never entered the coin");
}

#[test]
fn one_marked_a_character_enters_a_trigger_it_starts_in_before_it_moves() {
    let mut physics = floor();
    let mut zone = spec(ColliderShape::Sphere { radius: 3.0 });
    zone.is_trigger = true;
    zone.listens = LISTEN_TRIGGERS;
    physics.add_static_collider(2, zone, at(0.0, 1.0, 0.0)).unwrap();
    let character = Character::new(&mut physics, 0.0, 0.0);
    physics.set_character(character.id, true).unwrap();
    let mut entered = 0;
    for _ in 0..5 {
        physics.step(DT).unwrap();
        entered += physics.contact_events().iter().filter(|e| e.kind == ContactKind::Enter && e.trigger).count();
    }
    assert_eq!(entered, 1);
}

#[test]
fn bad_moves_are_refused() {
    let mut physics = floor();
    let mut character = Character::new(&mut physics, 0.0, 0.0);
    let mut hits = Vec::new();
    let s = character.settings;
    let id = character.id;
    for (motion, dt, settings) in [
        (Vector::new(f32::NAN, 0.0, 0.0), DT, s),
        (Vector::X, 0.0, s),
        (Vector::X, DT, CharacterSettings { slope_limit: -1.0, ..s }),
        (Vector::X, DT, CharacterSettings { step_offset: f32::INFINITY, ..s }),
        (Vector::X, DT, CharacterSettings { skin_width: 0.0, ..s }),
        (Vector::X, DT, CharacterSettings { mass: -1.0, ..s }),
    ] {
        assert!(
            matches!(physics.move_character(id, motion, dt, settings, &mut hits), Err(PhysicsError::Value(_))),
            "{motion} {dt} {settings:?}"
        );
    }
    assert_eq!(physics.move_character(999, Vector::X, DT, s, &mut hits), Err(PhysicsError::NoCollider(999)));
    character.go(&mut physics, Vector::X * 0.1);
}

#[test]
fn a_move_says_what_it_stands_on() {
    let mut physics = Physics::new();
    let floor = block(&mut physics, 100, Vector::new(100.0, 1.0, 100.0), Vector::new(0.0, -0.5, 0.0), 0.0);
    let crate_ = block(&mut physics, 7, Vector::new(4.0, 1.0, 4.0), Vector::new(10.0, 0.5, 0.0), 0.0);
    let mut character = Character::new(&mut physics, 0.0, 0.0);
    assert_eq!(character.walk(&mut physics, Vector::ZERO, 0.2).ground, Some(floor));

    character.object.position = Vector::new(10.0, 2.0 + character.settings.skin_width, 0.0);
    physics.follow(&[character.id], &[character.object]).unwrap();
    assert_eq!(character.walk(&mut physics, Vector::ZERO, 0.2).ground, Some(crate_));
    assert_eq!(character.go(&mut physics, Vector::ZERO).ground, Some(crate_), "standing still too");

    character.object.position = Vector::new(30.0, 5.0, 0.0);
    physics.follow(&[character.id], &[character.object]).unwrap();
    assert_eq!(character.go(&mut physics, Vector::new(0.0, -0.01, 0.0)).ground, None, "in the air");
}

#[test]
fn walking_on_open_ground_it_never_loses_a_frame() {
    // The Ring Ring Deals player: 1.6 m tall, 0.3 m round, steps of 0.1 m,
    // walking circles at 3.2 m/s, held down as that game does.
    let mut physics = floor();
    let settings = CharacterSettings { step_offset: 0.1, mass: 0.0, ..CharacterSettings::default() };
    let shape = ColliderShape::Capsule { radius: 0.3, height: 1.6, direction: Axis::Y };
    let mut object = at(0.0, 0.8 + settings.skin_width, 0.0);
    let id = physics.add_static_collider(1, spec(shape), object).unwrap();
    let mut hits = Vec::new();
    let (mut fall, mut grounded, mut stalls) = (0.0f32, false, Vec::new());
    for frame in 0..3600 {
        let turn = frame as f32 * DT / 6.0 * std::f32::consts::TAU;
        let velocity = Vector::new(turn.cos(), 0.0, turn.sin()) * 3.2;
        if grounded {
            fall = -1.0;
        }
        fall -= 9.81 * DT;
        let moved =
            physics.move_character(id, velocity * DT + Vector::new(0.0, fall * DT, 0.0), DT, settings, &mut hits);
        let moved = moved.unwrap();
        grounded = moved.flags & COLLIDED_BELOW != 0;
        object.position += moved.translation;
        physics.follow(&[id], &[object]).unwrap();
        physics.step(DT).unwrap();
        let along = Vector::new(moved.translation.x, 0.0, moved.translation.z).length();
        if frame > 10 && along < 0.5 * 3.2 * DT {
            stalls.push((frame, along, object.position.y));
        }
        let feet = object.position.y - 0.8 - settings.skin_width;
        assert!(frame < 10 || (grounded && feet.abs() < 2.0e-4), "frame {frame}: feet {feet}, grounded {grounded}");
    }
    assert!(stalls.is_empty(), "{} frames of 3600 lost their step: {:?}", stalls.len(), &stalls[..stalls.len().min(8)]);
}

#[test]
fn up_and_down_a_ramp_and_a_step_it_never_loses_a_frame() {
    // A 15° ramp up to a deck 0.5 m high, and a 0.08 m step off its far
    // side, walked over and back at 3.2 m/s.
    let mut physics = floor();
    let rise = 0.5f32;
    let run = rise / 15f32.to_radians().tan();
    let slope = (rise * rise + run * run).sqrt();
    block(&mut physics, 2, Vector::new(slope, 0.2, 4.0), Vector::new(run / 2.0, rise / 2.0 - 0.1, 0.0), 15.0);
    block(&mut physics, 3, Vector::new(3.0, rise, 4.0), Vector::new(run + 1.5, rise / 2.0, 0.0), 0.0);
    block(&mut physics, 4, Vector::new(3.0, rise - 0.08, 4.0), Vector::new(run + 4.5, (rise - 0.08) / 2.0, 0.0), 0.0);
    let settings = CharacterSettings { step_offset: 0.1, mass: 0.0, ..CharacterSettings::default() };
    let shape = ColliderShape::Capsule { radius: 0.3, height: 1.6, direction: Axis::Y };
    let mut object = at(-2.0, 0.8 + settings.skin_width, 0.0);
    let id = physics.add_static_collider(1, spec(shape), object).unwrap();
    let mut hits = Vec::new();
    let (mut fall, mut grounded, mut stalls) = (0.0f32, false, Vec::new());
    let mut furthest = 0.0f32;
    for frame in 0..1200 {
        // Out for 5 s, back for 5 s, twice.
        let way = if (frame / 300) % 2 == 0 { 1.0 } else { -1.0 };
        let velocity = Vector::new(3.2 * way * 0.5, 0.0, 0.0);
        if grounded {
            fall = -1.0;
        }
        fall -= 9.81 * DT;
        let moved =
            physics.move_character(id, velocity * DT + Vector::new(0.0, fall * DT, 0.0), DT, settings, &mut hits);
        let moved = moved.unwrap();
        grounded = moved.flags & COLLIDED_BELOW != 0;
        object.position += moved.translation;
        physics.follow(&[id], &[object]).unwrap();
        physics.step(DT).unwrap();
        furthest = furthest.max(object.position.x);
        if frame % 300 > 10 && moved.translation.x * way < 0.5 * 1.6 * DT {
            stalls.push((frame, moved.translation.x, object.position));
        }
    }
    assert!(furthest > run + 3.5, "it went over the deck and down the step: furthest {furthest}");
    assert!(stalls.is_empty(), "{} frames of 1200 lost their step: {:?}", stalls.len(), &stalls[..stalls.len().min(8)]);
}

/// What moving 20 characters takes, one move each, as in a frame, among
/// boxes on a floor, held to a bar (stage 14's GT1): `cargo test --release
/// -p fynkel_physics -- --ignored bench_`.
#[test]
#[ignore = "a benchmark: run in release"]
fn bench_moving_characters() {
    let mut physics = floor();
    for i in 0..20 {
        let (x, z) = ((i % 5) as f32 * 4.0 - 8.0, (i / 5) as f32 * 4.0 - 6.0);
        block(&mut physics, 10 + i, Vector::new(1.0, 0.8, 1.0), Vector::new(x + 2.0, 0.4, z + 2.0), 0.0);
    }
    let settings = CharacterSettings { step_offset: 0.1, mass: 0.0, ..CharacterSettings::default() };
    let shape = ColliderShape::Capsule { radius: 0.3, height: 1.6, direction: Axis::Y };
    let mut characters: Vec<(u64, ObjectPose)> = (0..20)
        .map(|i| {
            let object = at((i % 5) as f32 * 4.0 - 8.0, 0.8 + settings.skin_width, (i / 5) as f32 * 4.0 - 6.0);
            (physics.add_static_collider(1000 + i, spec(shape), object).unwrap(), object)
        })
        .collect();
    physics.step(DT).unwrap();
    let mut hits = Vec::new();
    let mut frame = 0u32;
    fynkel_bench::bench("20 characters moved", 200, 0.36, || {
        frame += 1;
        let turn = frame as f32 * DT / 6.0 * std::f32::consts::TAU;
        let motion = Vector::new(turn.cos(), -1.2, turn.sin()) * (3.2 * DT);
        for (id, object) in &mut characters {
            let moved = physics.move_character(*id, motion, DT, settings, &mut hits).unwrap();
            object.position += moved.translation;
            physics.follow(&[*id], &[*object]).unwrap();
        }
    });
}
