//! Blending (stage 9 phase 3): a layer over the base on part of the
//! skeleton, an additive layer, and a 1D blend whose clips keep the same
//! pace, each at given weights against numbers worked out by hand; every
//! fade starting from the pose shown; and changing clips every frame
//! never making the model jump.
//!
//! The toy: `body` (rest z 5) and its child `arm`. `slide` (2 s) moves
//! the body from x 0 to 4 and turns the arm from 0° to 90° about z; `hop`
//! (1 s, step keys) puts the body at y 1, then from 1 s at y 3; `grow`
//! (2 s) scales the arm from 1 to 3; `sway` (2 s) and `swing` (1 s) go
//! there and back, so they loop smoothly.

mod toy;

use fynkel_world::*;
use toy::*;

/// The arm's turn about z, in degrees.
fn arm_angle(toy: &Toy) -> f32 {
    let (axis, angle) = toy.arm().rotation.to_axis_angle();
    if angle.abs() < 1e-6 { 0.0 } else { (angle * axis.z.signum()).to_degrees() }
}

/// Holds the clips where they are: each test sets the times itself.
fn frozen() -> Toy {
    let mut toy = Toy::new();
    toy.world.set_animation_speed(toy.root, 0.0).unwrap();
    toy
}

/// A layer from the arm down plays over the base: the arm takes the
/// layer's turn by the layer's weight, the body keeps the base's.
#[test]
fn a_layer_from_a_joint_down_plays_over_the_base() {
    let mut toy = frozen();
    let root = toy.root;
    toy.world.play(root, SLIDE, 0.0, false).unwrap();
    toy.world.set_animation_time(root, 1.0).unwrap();
    toy.world.set_animation_layer(root, 1, Some("arm"), false, 1.0).unwrap();
    toy.world.play_on(root, 1, Motion::Clip(SLIDE), 0.0, false).unwrap();
    toy.world.set_animation_time_on(root, 1, 2.0).unwrap();
    toy.step(0.0);
    // The base's slide halfway: body at x 2, arm 45°; the layer's at its
    // end: arm 90°, its body left out by the mask.
    assert!(near(toy.body(), Vec3::new(2.0, 0.0, 0.0)), "{}", toy.body());
    assert!((arm_angle(&toy) - 90.0).abs() < 1e-3, "{}", arm_angle(&toy));

    toy.world.set_animation_layer_weight(root, 1, 0.5).unwrap();
    for _ in 0..5 {
        toy.step(0.0);
        assert!((arm_angle(&toy) - 67.5).abs() < 1e-3, "{}", arm_angle(&toy));
        assert!(near(toy.body(), Vec3::new(2.0, 0.0, 0.0)));
    }

    // A mask from the body takes in the arm below it.
    toy.world.set_animation_layer(root, 1, Some("body"), false, 0.5).unwrap();
    toy.step(0.0);
    assert!(near(toy.body(), Vec3::new(3.0, 0.0, 0.0)), "{}", toy.body());
    assert!((arm_angle(&toy) - 67.5).abs() < 1e-3);
}

/// A layer at half weight on a part the base doesn't move stays halfway
/// between the part's pose and the layer's, frame after frame, and fades
/// back to the pose as the layer stops.
#[test]
fn a_layer_on_a_part_the_base_doesnt_move_stays_where_its_weight_puts_it() {
    let mut toy = Toy::new();
    let root = toy.root;
    toy.world.play(root, HOP, 0.0, true).unwrap();
    toy.world.set_animation_layer(root, 1, Some("arm"), false, 0.5).unwrap();
    toy.world.play_on(root, 1, Motion::Clip(SLIDE), 0.0, false).unwrap();
    toy.world.set_animation_time_on(root, 1, 2.0).unwrap();
    for _ in 0..10 {
        toy.step(0.1);
        assert!((arm_angle(&toy) - 45.0).abs() < 1e-3, "{}", arm_angle(&toy));
    }
    toy.world.stop_on(root, 1, 0.5).unwrap();
    toy.step(0.25);
    assert!(same_turn(toy.arm().rotation, Quat::IDENTITY.lerp(turn_z(90.0), 0.25)), "{}", arm_angle(&toy));
    toy.step(0.25);
    assert!(arm_angle(&toy).abs() < 1e-3, "{}", arm_angle(&toy));
    assert_eq!(toy.world.animator_state_on(root, 1).unwrap().motion, None);
}

/// An additive layer adds its clip's change from its first pose.
#[test]
fn an_additive_layer_adds_its_change() {
    let mut toy = frozen();
    let root = toy.root;
    toy.world.play(root, SLIDE, 0.0, false).unwrap();
    toy.world.set_animation_time(root, 1.0).unwrap();
    toy.world.set_animation_layer(root, 1, None, true, 1.0).unwrap();
    toy.world.play_on(root, 1, Motion::Clip(SLIDE), 0.0, false).unwrap();
    toy.world.set_animation_time_on(root, 1, 1.0).unwrap();
    toy.step(0.0);
    // The slide's change at 1 s is x + 2 and 45°: added to the base's
    // pose at 1 s, the slide's end.
    assert!(near(toy.body(), Vec3::new(4.0, 0.0, 0.0)), "{}", toy.body());
    assert!((arm_angle(&toy) - 90.0).abs() < 1e-3, "{}", arm_angle(&toy));

    toy.world.set_animation_layer_weight(root, 1, 0.5).unwrap();
    toy.step(0.0);
    assert!(near(toy.body(), Vec3::new(3.0, 0.0, 0.0)), "{}", toy.body());
    assert!((arm_angle(&toy) - 67.5).abs() < 1e-3, "{}", arm_angle(&toy));

    // The hop's change from its first key (y 1) to its second (y 3): up 2,
    // added over the slide, the arm's turn left as the base has it.
    toy.world.set_animation_layer_weight(root, 1, 1.0).unwrap();
    toy.world.play_on(root, 1, Motion::Clip(HOP), 0.0, false).unwrap();
    toy.world.set_animation_time_on(root, 1, 1.0).unwrap();
    toy.step(0.0);
    assert!(near(toy.body(), Vec3::new(2.0, 2.0, 0.0)), "{}", toy.body());
    assert!((arm_angle(&toy) - 45.0).abs() < 1e-3);
}

/// A blend of the slide (2 s) at 0 and the hop (1 s) at 1: at 0.25, three
/// parts slide to one of hop, a round taking 1.75 s, both clips the same
/// way through it.
#[test]
fn a_blend_mixes_its_clips_by_its_number_at_one_pace() {
    let mut toy = Toy::new();
    let root = toy.root;
    let blend = toy.world.define_blend(root, &[(HOP, 1.0), (SLIDE, 0.0)]).unwrap();
    toy.world.set_blend(root, blend, 0.25).unwrap();
    toy.world.play_on(root, 0, Motion::Blend(blend), 0.0, false).unwrap();
    assert_eq!(toy.world.clip_weight(root, SLIDE).unwrap(), 0.75);
    assert_eq!(toy.world.clip_weight(root, HOP).unwrap(), 0.25);
    let state = toy.world.animator_state(root).unwrap();
    assert_eq!((state.motion, state.length), (Some(Motion::Blend(blend)), 1.75));

    // Halfway round: the slide at 1 s (x 2, 45°), the hop at 0.5 s (y 1);
    // the hop doesn't turn the arm, so its share of the arm is at rest.
    toy.step(0.875);
    assert!(near(toy.body(), Vec3::new(1.5, 0.25, 0.0)), "{}", toy.body());
    assert!(same_turn(toy.arm().rotation, Quat::IDENTITY.lerp(turn_z(45.0), 0.75)));
    assert!((toy.world.animator_state(root).unwrap().time - 0.875).abs() < 1e-5);

    // Its number moved, the clips keep their place in the round.
    toy.world.set_blend(root, blend, 1.0).unwrap();
    toy.step(0.0);
    assert!(near(toy.body(), Vec3::new(0.0, 1.0, 0.0)), "{}", toy.body());
    toy.world.set_blend(root, blend, -3.0).unwrap();
    toy.step(0.0);
    assert!(near(toy.body(), Vec3::new(2.0, 0.0, 0.0)), "{}", toy.body());
    // At 0 the round is the slide's 2 s: a quarter more is half a second.
    toy.step(0.5);
    assert!(near(toy.body(), Vec3::new(3.0, 0.0, 0.0)), "{}", toy.body());
}

/// A blend's clips each report their time, while they're in its mix.
#[test]
fn a_blends_clips_report_their_times() {
    let mut toy = Toy::new();
    let root = toy.root;
    toy.world.set_animation_reporting(root, true).unwrap();
    let blend = toy.world.define_blend(root, &[(SLIDE, 0.0), (HOP, 1.0)]).unwrap();
    toy.world.set_blend(root, blend, 0.5).unwrap();
    toy.world.play_on(root, 0, Motion::Blend(blend), 0.0, false).unwrap();
    // A round of 1.5 s; 0.75 s is halfway, the slide at 1 s, the hop 0.5 s.
    toy.step(0.75);
    let seen: Vec<_> = toy.events.iter().map(|e| (e.clip, e.from, e.to)).collect();
    assert_eq!(seen, [(SLIDE, 0.0, 1.0), (HOP, 0.0, 0.5)]);
    toy.world.set_blend(root, blend, 0.0).unwrap();
    toy.step(0.5);
    assert_eq!(toy.events.iter().map(|e| e.clip).collect::<Vec<_>>(), [SLIDE]);
}

/// After a stop, a crossfade goes in a straight line from the pose held.
#[test]
fn a_crossfade_from_a_held_pose_goes_straight() {
    let mut toy = Toy::new();
    let root = toy.root;
    toy.world.play(root, SLIDE, 0.0, false).unwrap();
    toy.step(1.0);
    toy.world.stop(root).unwrap();
    toy.step(0.5);
    assert!(near(toy.body(), Vec3::new(2.0, 0.0, 0.0)));
    toy.world.play(root, HOP, 1.0, false).unwrap();
    // A quarter, a half and three quarters of the way to the hop's y 1.
    for (dt, t) in [(0.25, 0.25), (0.25, 0.5), (0.25, 0.75)] {
        toy.step(dt);
        let expected = Vec3::new(2.0, 0.0, 0.0).lerp(Vec3::new(0.0, 1.0, 0.0), t);
        assert!(near(toy.body(), expected), "{} at {t}, not {expected}", toy.body());
    }
}

/// The base fading out goes back to the pose it was in when the fade
/// began: here, the pose it held.
#[test]
fn the_base_fading_out_leaves_the_pose_it_was_in() {
    let mut toy = frozen();
    let root = toy.root;
    toy.world.play(root, HOP, 0.0, false).unwrap();
    toy.step(0.0);
    toy.world.set_animation_speed(root, 1.0).unwrap();
    toy.world.stop(root).unwrap();
    toy.world.play(root, SLIDE, 0.0, false).unwrap();
    toy.world.set_animation_time(root, 2.0).unwrap();
    toy.step(0.0);
    assert!(near(toy.body(), Vec3::new(4.0, 0.0, 0.0)));
    toy.world.stop_on(root, 0, 1.0).unwrap();
    toy.step(0.5);
    // Half of the slide's end left: the pose doesn't drift, it fades.
    assert!(near(toy.body(), Vec3::new(4.0, 0.0, 0.0)), "{}", toy.body());
    toy.step(0.5);
    assert!(near(toy.body(), Vec3::new(4.0, 0.0, 0.0)));
    assert_eq!(toy.world.animator_state(root).unwrap().motion, None);
}

/// A game that changes its mind every frame: each frame a crossfade to
/// another clip or blend, the blend's number swinging, the layer above
/// switched on and off. However it lands, no frame moves a part further
/// than the fastest fade or clip would.
#[test]
fn changing_clips_every_frame_never_jumps() {
    let mut toy = Toy::new();
    let root = toy.root;
    let blend = toy.world.define_blend(root, &[(SWAY, 0.0), (SWING, 1.0)]).unwrap();
    toy.world.set_animation_layer(root, 1, Some("arm"), false, 1.0).unwrap();
    toy.world.play(root, SWAY, 0.0, true).unwrap();
    toy.step(0.0);
    let dt = 1.0 / 60.0;
    let fade = 0.3;
    let motions = [Motion::Clip(SWAY), Motion::Blend(blend), Motion::Clip(SWING), Motion::Clip(SWAY)];
    let (mut body, mut angle, mut scale) = (toy.body(), arm_angle(&toy), toy.arm().scale.x);
    let (mut worst_body, mut worst_angle, mut worst_scale) = (0.0f32, 0.0f32, 0.0f32);
    // A pseudo-random walk, the same each run.
    let mut seed = 12345u32;
    let mut next = || {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
        (seed >> 16) as usize
    };
    for frame in 0..600 {
        let motion = motions[next() % motions.len()];
        toy.world.play_on(root, 0, motion, fade, true).unwrap();
        toy.world.set_blend(root, blend, (frame as f32 * 0.05).sin() * 0.5 + 0.5).unwrap();
        match next() % 4 {
            0 => toy.world.play_on(root, 1, Motion::Clip(SWING), fade, true).unwrap(),
            1 => toy.world.stop_on(root, 1, fade).unwrap(),
            _ => {}
        }
        toy.step(dt);
        let (b, a, s) = (toy.body(), arm_angle(&toy), toy.arm().scale.x);
        worst_body = worst_body.max((b - body).length());
        worst_angle = worst_angle.max((a - angle).abs());
        worst_scale = worst_scale.max((s - scale).abs());
        (body, angle, scale) = (b, a, s);
    }
    // The furthest a frame can take a part: from one clip's pose to the
    // other's over the fade (the body 3 m apart at most, the arm 135° and
    // a scale of 1), and the clips' own pace (2 m/s, 90°/s and 1 a second
    // at most, the blend's pace sped up by its shorter clip).
    let most = |apart: f32, pace: f32| (apart / fade + 2.0 * pace) * dt;
    assert!(worst_body <= most(3.0, 2.0), "the body moved {worst_body} m in a frame");
    assert!(worst_angle <= most(135.0, 90.0), "the arm turned {worst_angle}° in a frame");
    assert!(worst_scale <= most(1.0, 1.0), "the arm grew {worst_scale} in a frame");
    // And it moved: the test isn't standing still.
    assert!(worst_body > 0.01 && worst_angle > 0.1);
}

#[test]
fn what_a_layer_or_a_blend_cant_be() {
    let mut toy = Toy::new();
    let root = toy.root;
    let world = &mut toy.world;
    assert!(world.play_on(root, MAX_LAYERS, Motion::Clip(SLIDE), 0.0, true).is_err());
    assert!(world.set_animation_layer(root, 1, Some("tail"), false, 1.0).is_err());
    assert!(world.set_animation_layer(root, 0, Some("arm"), false, 1.0).is_err());
    assert!(world.set_animation_layer(root, 0, None, true, 1.0).is_err());
    assert!(world.set_animation_layer_weight(root, 0, 0.5).is_err());
    assert!(world.set_animation_layer(root, 0, None, false, 1.0).is_ok());
    assert!(world.define_blend(root, &[]).is_err());
    assert!(world.define_blend(root, &[(9, 0.0)]).is_err());
    assert!(world.define_blend(root, &[(SLIDE, f32::NAN)]).is_err());
    assert!(world.play_on(root, 0, Motion::Blend(0), 0.0, true).is_err());
    assert!(world.set_blend(root, 0, 1.0).is_err());
    for _ in 0..MAX_BLENDS {
        world.define_blend(root, &[(SLIDE, 0.0)]).unwrap();
    }
    assert!(world.define_blend(root, &[(SLIDE, 0.0)]).is_err());
    // A layer never played reads as nothing.
    assert_eq!(world.animator_state_on(root, 5).unwrap().motion, None);
    assert_eq!(world.clip_weight_on(root, 5, SLIDE).unwrap(), 0.0);
}
