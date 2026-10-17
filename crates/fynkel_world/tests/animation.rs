//! Playing a model's clips (stage 9 phase 1): poses at given times worked
//! out by hand, for each kind of key, looping and the end, a crossfade
//! halfway, and an animator's life with its model.

use std::f32::consts::FRAC_PI_2;

use fynkel_world::*;

/// A `.glb` with two nodes, `body` (rest at z 5) and its child `arm`, and
/// three clips:
///
/// - `slide` (2 s, linear): the body from x 0 to 4, the arm from no turn
///   to 90° about z;
/// - `hop` (1 s, step): the body at y 1, then from 1 s at y 3;
/// - `grow` (2 s, cubic, flat tangents): the arm's scale from 1 to 3.
fn glb() -> Vec<u8> {
    let (s, c) = (FRAC_PI_2 / 2.0).sin_cos();
    let accessors: [(&str, Vec<f32>); 7] = [
        ("SCALAR", vec![0.0, 2.0]),
        ("VEC3", vec![0.0, 0.0, 0.0, 4.0, 0.0, 0.0]),
        ("VEC4", vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, s, c]),
        ("SCALAR", vec![0.0, 1.0]),
        ("VEC3", vec![0.0, 1.0, 0.0, 0.0, 3.0, 0.0]),
        ("SCALAR", vec![0.0, 2.0]),
        ("VEC3", [[0.0; 3], [1.0; 3], [0.0; 3], [0.0; 3], [3.0; 3], [0.0; 3]].concat()),
    ];
    let mut bin = Vec::new();
    let mut views = Vec::new();
    let mut access = Vec::new();
    for (i, (kind, values)) in accessors.iter().enumerate() {
        let size = match *kind {
            "SCALAR" => 1,
            "VEC3" => 3,
            _ => 4,
        };
        views.push(format!(r#"{{"buffer":0,"byteOffset":{},"byteLength":{}}}"#, bin.len(), values.len() * 4));
        let bounds = if *kind == "SCALAR" {
            let (min, max) = values.iter().fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
            format!(r#","min":[{min}],"max":[{max}]"#)
        } else {
            String::new()
        };
        access.push(format!(
            r#"{{"bufferView":{i},"componentType":5126,"count":{},"type":"{kind}"{bounds}}}"#,
            values.len() / size
        ));
        bin.extend(values.iter().flat_map(|v| v.to_le_bytes()));
    }
    let json = format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"name":"toy","nodes":[0]}}],
        "nodes":[{{"name":"body","translation":[0,0,5],"children":[1]}},{{"name":"arm"}}],
        "buffers":[{{"byteLength":{}}}],"bufferViews":[{}],"accessors":[{}],
        "animations":[
          {{"name":"slide","samplers":[{{"input":0,"output":1}},{{"input":0,"output":2}}],
            "channels":[{{"sampler":0,"target":{{"node":0,"path":"translation"}}}},{{"sampler":1,"target":{{"node":1,"path":"rotation"}}}}]}},
          {{"name":"hop","samplers":[{{"input":3,"output":4,"interpolation":"STEP"}}],
            "channels":[{{"sampler":0,"target":{{"node":0,"path":"translation"}}}}]}},
          {{"name":"grow","samplers":[{{"input":5,"output":6,"interpolation":"CUBICSPLINE"}}],
            "channels":[{{"sampler":0,"target":{{"node":1,"path":"scale"}}}}]}}
        ]}}"#,
        bin.len(),
        views.join(","),
        access.join(",")
    );
    let mut text = json.into_bytes();
    text.resize(text.len().next_multiple_of(4), b' ');
    bin.resize(bin.len().next_multiple_of(4), 0);
    let mut out = Vec::new();
    let total = 12 + 8 + text.len() + 8 + bin.len();
    for word in [0x4654_6C67, 2, total as u32, text.len() as u32, 0x4E4F_534A] {
        out.extend(u32::to_le_bytes(word));
    }
    out.extend(&text);
    out.extend(u32::to_le_bytes(bin.len() as u32));
    out.extend(u32::to_le_bytes(0x004E_4942));
    out.extend(&bin);
    out
}

const SLIDE: u32 = 0;
const HOP: u32 = 1;
const GROW: u32 = 2;

struct Toy {
    world: World,
    root: ObjectId,
    body: ObjectId,
    arm: ObjectId,
    events: Vec<AnimationEvent>,
}

impl Toy {
    fn new() -> Self {
        let model = ModelData::from_glb(&glb()).unwrap();
        assert!(model.warnings().is_empty(), "{:?}", model.warnings());
        let mut world = World::new();
        let ids = world.add_model(&model).unwrap();
        let root = world.spawn_model(&model, &ids.meshes, &ids.materials, Transform::default()).unwrap();
        let body = world.find(root, "body").unwrap().unwrap();
        let arm = world.find(root, "arm").unwrap().unwrap();
        Self { world, root, body, arm, events: Vec::new() }
    }

    fn step(&mut self, dt: f32) {
        self.events.clear();
        self.world.animate(dt, &mut self.events);
    }

    fn body(&self) -> Vec3 {
        self.world.get::<Transform>(self.body).unwrap().position
    }

    fn arm(&self) -> Transform {
        self.world.get::<Transform>(self.arm).unwrap()
    }
}

fn near(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-4
}

fn turn_z(degrees: f32) -> Quat {
    Quat::from_rotation_z(degrees.to_radians())
}

fn same_turn(a: Quat, b: Quat) -> bool {
    a.dot(b).abs() > 1.0 - 1e-6
}

#[test]
fn a_model_with_clips_gets_an_animator_that_knows_them() {
    let toy = Toy::new();
    assert!(toy.world.has_animator(toy.root));
    assert_eq!(toy.world.clips(toy.root).unwrap(), [("slide", 2.0), ("hop", 1.0), ("grow", 2.0)]);
    let state = toy.world.animator_state(toy.root).unwrap();
    assert_eq!((state.clip, state.time, state.speed), (None, 0.0, 1.0));
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
    assert!(matches!(toy.world.play(toy.root, 3, 0.0, true), Err(WorldError::NoClip { count: 3, .. })));
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
    assert_eq!(toy.world.animator_state(toy.root).unwrap().clip, Some(HOP));
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
    assert_eq!(toy.world.animator_state(toy.root).unwrap().clip, None);
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
