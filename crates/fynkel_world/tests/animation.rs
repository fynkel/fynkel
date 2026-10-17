//! Playing a model's clips (stage 9 phase 1): poses at given times worked
//! out by hand, for each kind of key, looping and the end, a crossfade
//! halfway, and an animator's life with its model.

mod toy;

use fynkel_world::*;
use toy::*;

#[test]
fn a_model_with_clips_gets_an_animator_that_knows_them() {
    let toy = Toy::new();
    assert!(toy.world.has_animator(toy.root));
    assert_eq!(
        toy.world.clips(toy.root).unwrap(),
        [("slide", 2.0), ("hop", 1.0), ("grow", 2.0), ("sway", 2.0), ("swing", 1.0)]
    );
    assert_eq!(toy.world.animated_parts(toy.root).unwrap(), [toy.body, toy.arm]);
    let state = toy.world.animator_state(toy.root).unwrap();
    assert_eq!((state.motion, state.time, state.speed), (None, 0.0, 1.0));
    // Nothing plays until asked: the rest pose.
    let mut toy = toy;
    toy.step(0.5);
    assert_eq!(toy.body(), Vec3::new(0.0, 0.0, 5.0));
    // A model without clips has none.
    let truck = ModelData::from_glb(&box_glb()).unwrap();
    let ids = toy.world.add_model(&truck).unwrap();
    let plain = toy.world.spawn_model(&truck, &ids.meshes, &ids.materials, Transform::default()).unwrap();
    assert!(!toy.world.has_animator(plain));
    assert!(matches!(toy.world.play(plain, 0, 0.0, true), Err(WorldError::NoComponent { .. })));
    assert!(matches!(toy.world.play(toy.root, 5, 0.0, true), Err(WorldError::NoClip { count: 5, .. })));
}

fn box_glb() -> Vec<u8> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/assets/gltf/Box.glb");
    std::fs::read(path).unwrap()
}

#[test]
fn linear_keys_move_and_turn_in_a_straight_line_and_a_loop_comes_round() {
    let mut toy = Toy::new();
    toy.world.set_animation_reporting(toy.root, true).unwrap();
    toy.world.play(toy.root, SLIDE, 0.0, true).unwrap();
    toy.step(0.5);
    // A quarter of the way: x 1, a quarter of 90°. The clip replaces the
    // rest's z, since it moves the body's whole position.
    assert!(near(toy.body(), Vec3::new(1.0, 0.0, 0.0)), "{}", toy.body());
    assert!(same_turn(toy.arm().rotation, turn_z(22.5)), "{}", toy.arm().rotation);
    toy.step(1.0);
    assert!(near(toy.body(), Vec3::new(3.0, 0.0, 0.0)));
    assert!(same_turn(toy.arm().rotation, turn_z(67.5)));
    // Past the end, round again: 2.5 s is 0.5 s into the second time.
    toy.step(1.0);
    assert!(near(toy.body(), Vec3::new(1.0, 0.0, 0.0)), "{}", toy.body());
    let [event] = toy.events.as_slice() else { panic!("{:?}", toy.events) };
    assert_eq!(
        (event.root, event.clip, event.from, event.to, event.loops, event.ended),
        (toy.root, SLIDE, 1.5, 0.5, 1, false)
    );
    // The arm isn't scaled by a clip that doesn't scale it.
    assert_eq!(toy.arm().scale, Vec3::ONE);
}

#[test]
fn a_clip_that_doesnt_loop_holds_its_last_key_and_ends_once() {
    let mut toy = Toy::new();
    toy.world.set_animation_reporting(toy.root, true).unwrap();
    toy.world.play(toy.root, SLIDE, 0.0, false).unwrap();
    toy.step(3.0);
    assert!(near(toy.body(), Vec3::new(4.0, 0.0, 0.0)));
    assert!(same_turn(toy.arm().rotation, turn_z(90.0)));
    assert!(toy.events[0].ended);
    toy.step(0.5);
    assert!(toy.events.is_empty(), "{:?}", toy.events);
    assert_eq!(toy.world.animator_state(toy.root).unwrap().time, 2.0);
    // Played again, it starts over.
    toy.world.play(toy.root, SLIDE, 0.0, false).unwrap();
    toy.step(1.0);
    assert!(near(toy.body(), Vec3::new(2.0, 0.0, 0.0)));
}

#[test]
fn step_keys_hold_and_cubic_keys_ease() {
    let mut toy = Toy::new();
    toy.world.play(toy.root, HOP, 0.0, false).unwrap();
    toy.step(0.99);
    assert!(near(toy.body(), Vec3::new(0.0, 1.0, 0.0)));
    toy.step(0.02);
    assert!(near(toy.body(), Vec3::new(0.0, 3.0, 0.0)));

    let mut toy = Toy::new();
    toy.world.play(toy.root, GROW, 0.0, false).unwrap();
    toy.step(1.0);
    // Halfway, with flat tangents: halfway.
    assert!(near(toy.arm().scale, Vec3::splat(2.0)), "{}", toy.arm().scale);
    // A quarter of the way in, slower than a line: 1 + 2 × (3s² − 2s³) at
    // s = ¼ is 1.3125.
    let mut toy = Toy::new();
    toy.world.play(toy.root, GROW, 0.0, false).unwrap();
    toy.step(0.5);
    assert!(near(toy.arm().scale, Vec3::splat(1.3125)), "{}", toy.arm().scale);
}

#[test]
fn a_crossfade_halfway_is_halfway_and_what_the_new_clip_doesnt_move_goes_to_rest() {
    let mut toy = Toy::new();
    toy.world.play(toy.root, SLIDE, 0.0, true).unwrap();
    toy.step(1.0);
    toy.world.play(toy.root, HOP, 1.0, false).unwrap();
    toy.step(0.5);
    // Slide at 1.5 s (x 3) and hop at 0.5 s (y 1), half each.
    assert!(near(toy.body(), Vec3::new(1.5, 0.5, 0.0)), "{}", toy.body());
    // Hop doesn't turn the arm: its half is the rest, no turn. Half of
    // 67.5° is 33.75°.
    assert!(same_turn(toy.arm().rotation, turn_z(33.75)), "{}", toy.arm().rotation);
    assert_eq!(toy.world.clip_weight(toy.root, SLIDE).unwrap(), 0.5);
    assert_eq!(toy.world.animator_state(toy.root).unwrap().motion, Some(Motion::Clip(HOP)));
    // Faded: hop alone (at 1 s, y 3), and the arm back at rest.
    toy.step(0.5);
    assert!(near(toy.body(), Vec3::new(0.0, 3.0, 0.0)), "{}", toy.body());
    assert!(same_turn(toy.arm().rotation, Quat::IDENTITY), "{}", toy.arm().rotation);
    assert_eq!(toy.world.clip_weight(toy.root, SLIDE).unwrap(), 0.0);
    // After that, hop's own: the arm is the game's to move.
    toy.world.insert(toy.arm, Transform { rotation: turn_z(10.0), ..toy.arm() }).unwrap();
    toy.step(0.1);
    assert!(same_turn(toy.arm().rotation, turn_z(10.0)));
}

#[test]
fn speed_time_and_stop() {
    let mut toy = Toy::new();
    toy.world.play(toy.root, SLIDE, 0.0, true).unwrap();
    toy.world.set_animation_speed(toy.root, 2.0).unwrap();
    toy.step(0.5);
    assert!(near(toy.body(), Vec3::new(2.0, 0.0, 0.0)));
    toy.world.set_animation_speed(toy.root, -1.0).unwrap();
    toy.step(0.25);
    assert!(near(toy.body(), Vec3::new(1.5, 0.0, 0.0)), "{}", toy.body());
    toy.world.set_animation_time(toy.root, 9.0).unwrap();
    assert_eq!(toy.world.animator_state(toy.root).unwrap().time, 2.0);
    assert!(toy.world.set_animation_speed(toy.root, f32::NAN).is_err());
    // Stopped, the pose stays as it is.
    toy.world.set_animation_speed(toy.root, 1.0).unwrap();
    toy.world.set_animation_time(toy.root, 1.0).unwrap();
    toy.step(0.0);
    toy.world.stop(toy.root).unwrap();
    toy.step(1.0);
    assert!(near(toy.body(), Vec3::new(2.0, 0.0, 0.0)));
    assert_eq!(toy.world.animator_state(toy.root).unwrap().motion, None);
}

#[test]
fn an_animator_lives_and_goes_with_its_model() {
    let mut toy = Toy::new();
    toy.world.play(toy.root, SLIDE, 0.0, true).unwrap();
    // A hidden model keeps time but isn't posed.
    toy.world.set_active(toy.root, false).unwrap();
    toy.step(0.5);
    assert_eq!(toy.body(), Vec3::new(0.0, 0.0, 5.0));
    toy.world.set_active(toy.root, true).unwrap();
    toy.step(0.5);
    assert!(near(toy.body(), Vec3::new(2.0, 0.0, 0.0)));
    // A node the game destroyed is skipped; the rest still moves.
    toy.world.despawn(toy.arm).unwrap();
    assert_eq!(toy.world.animated_parts(toy.root).unwrap(), [toy.body]);
    toy.step(0.5);
    assert!(near(toy.body(), Vec3::new(3.0, 0.0, 0.0)));
    // The model destroyed, its animator goes too.
    assert_eq!(toy.world.animator_count(), 1);
    toy.world.despawn(toy.root).unwrap();
    assert_eq!(toy.world.animator_count(), 0);
    toy.step(0.5);
    // Many copies each keep their own time.
    let model = ModelData::from_glb(&glb()).unwrap();
    let ids = toy.world.add_model(&model).unwrap();
    let [a, b] = std::array::from_fn(|_| {
        toy.world.spawn_model(&model, &ids.meshes, &ids.materials, Transform::default()).unwrap()
    });
    toy.world.play(a, SLIDE, 0.0, true).unwrap();
    toy.step(0.5);
    toy.world.play(b, SLIDE, 0.0, true).unwrap();
    toy.step(0.5);
    let x = |root| toy.world.get::<Transform>(toy.world.find(root, "body").unwrap().unwrap()).unwrap().position.x;
    assert!(((x(a) - 2.0).abs() < 1e-4) && ((x(b) - 1.0).abs() < 1e-4), "{} {}", x(a), x(b));
}

#[test]
fn crossfading_to_the_clip_already_fading_in_every_frame_ends_the_fade_on_time() {
    // As a game's update does: say what it wants each frame.
    let mut toy = Toy::new();
    toy.world.play(toy.root, SLIDE, 0.0, true).unwrap();
    for _ in 0..10 {
        toy.world.play(toy.root, HOP, 0.5, true).unwrap();
        toy.step(0.05);
    }
    assert_eq!(toy.world.clip_weight(toy.root, HOP).unwrap(), 1.0);
    assert_eq!(toy.world.clip_weight(toy.root, SLIDE).unwrap(), 0.0);
    // Its time went on all along.
    assert!((toy.world.animator_state(toy.root).unwrap().time - 0.5).abs() < 1e-5);
}
