//! The input contract at the world level (blueprint §5.1, §19.2d item 7):
//! a host frame's edges are consumed by exactly one tick, held state is
//! what every tick sees, a frame without a tick carries its edges over.

use fynkel_core::input::{ActionKind, ActionSpec, Device, InputEvent};
use fynkel_core::{World, WorldConfig};

fn world() -> World {
    let mut w = World::new(WorldConfig {
        fixed_dt: 1.0 / 60.0,
        max_steps_per_frame: 4,
        ..WorldConfig::default()
    });
    let errors = w.bind_input(&[
        ActionSpec {
            name: "jump".into(),
            kind: ActionKind::Button,
            bindings: vec!["keyboard:Space".into(), "gamepad:south".into()],
        },
        ActionSpec {
            name: "move".into(),
            kind: ActionKind::Axis2d,
            bindings: vec!["keyboard:WASD".into(), "gamepad:leftStick".into()],
        },
    ]);
    assert!(errors.is_empty(), "{errors:?}");
    w
}

fn jump(w: &World) -> (bool, bool, bool) {
    let a = w.input().action("jump").unwrap();
    (a.pressed, a.held, a.released)
}

#[test]
fn a_press_is_seen_by_exactly_one_of_three_catch_up_ticks() {
    let mut w = world();
    // A long frame: three ticks catch up. The key went down during it.
    let plan = w.begin_frame(3.0 / 60.0);
    assert_eq!(plan.steps, 3);
    w.capture_input(&[InputEvent::down(Device::Keyboard, "Space")]);

    assert_eq!(
        jump(&w),
        (true, true, false),
        "the first tick sees the edge"
    );
    w.step();
    assert_eq!(jump(&w), (false, true, false), "the second sees held only");
    w.step();
    assert_eq!(jump(&w), (false, true, false), "and so does the third");
    w.step();

    // The next frame releases: one tick sees `released`, none `pressed`.
    let plan = w.begin_frame(1.0 / 60.0);
    assert_eq!(plan.steps, 1);
    w.capture_input(&[InputEvent::up(Device::Keyboard, "Space")]);
    assert_eq!(jump(&w), (false, false, true));
    w.step();
    assert_eq!(jump(&w), (false, false, false));
}

#[test]
fn a_frame_without_a_tick_carries_its_edges_to_the_next_tick() {
    let mut w = world();
    // Two short frames (no tick each) then one that ticks: the press and
    // the release both wait for that tick.
    assert_eq!(w.begin_frame(0.004).steps, 0);
    w.capture_input(&[InputEvent::down(Device::Keyboard, "Space")]);
    assert_eq!(jump(&w), (true, true, false));
    assert_eq!(w.begin_frame(0.004).steps, 0);
    w.capture_input(&[InputEvent::up(Device::Keyboard, "Space")]);
    assert_eq!(jump(&w), (true, false, true), "both edges pending, key up");
    let a = w.input().action("jump").unwrap();
    assert!(
        !a.frame_pressed && a.frame_released,
        "this frame only released"
    );
    assert_eq!(w.begin_frame(0.010).steps, 1);
    w.capture_input(&[]);
    assert_eq!(jump(&w), (true, false, true));
    w.step();
    assert_eq!(jump(&w), (false, false, false));
}

#[test]
fn held_axes_compose_across_devices_and_reset_releases_everything() {
    let mut w = world();
    w.begin_frame(1.0 / 60.0);
    w.capture_input(&[
        InputEvent::down(Device::Keyboard, "W"),
        InputEvent::down(Device::Keyboard, "D"),
        InputEvent::axis(Device::Gamepad, "leftStick", -0.5, 0.0),
    ]);
    let m = w.input().action("move").unwrap();
    assert!(m.held);
    assert!(
        (m.x - 0.5).abs() < 1e-6 && (m.y - 1.0).abs() < 1e-6,
        "{m:?}"
    );
    w.step();
    // Focus lost: every control up, edges for the next tick.
    w.begin_frame(1.0 / 60.0);
    w.capture_input(&[
        InputEvent::down(Device::Keyboard, "Space"),
        InputEvent::reset(),
    ]);
    let m = w.input().action("move").unwrap();
    assert!(!m.held && m.x == 0.0 && m.y == 0.0);
    assert_eq!(
        jump(&w),
        (true, false, true),
        "pressed and released in one frame"
    );
    let v = w.input_view();
    assert_eq!(v.len(), 2);
    assert_eq!(v[0].name, "jump");
    assert_eq!(v[1].kind, ActionKind::Axis2d);
}
