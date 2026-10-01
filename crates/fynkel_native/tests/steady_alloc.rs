//! Foundation 4.8: reading the frame's input and events allocates nothing
//! once warm. `fynkel_world_input_view` and `fynkel_world_drain_events`
//! run every frame; the event queue kept its capacity only until the
//! first drain, and the input view cloned every action's name.

use std::ptr;

use fynkel_contracts::Entity;
use fynkel_core::event::EventPayload;
use fynkel_core::input::{ActionKind, ActionSpec, Device, InputEvent};
use fynkel_core::memory::allocation_count;
use fynkel_native::*;

#[test]
fn input_and_events_read_without_allocating() {
    let rt = fynkel_runtime_create();
    let w = unsafe { fynkel_world_create(rt, ptr::null(), std::ptr::null_mut()) };
    let spec = |name: &str, kind, bindings: &[&str]| ActionSpec {
        name: name.into(),
        kind,
        bindings: bindings.iter().map(|b| b.to_string()).collect(),
    };
    let world = unsafe { &mut *w };
    let errors = world.core_mut().bind_input(&[
        spec("jump", ActionKind::Button, &["keyboard:Space"]),
        spec("fire", ActionKind::Button, &["mouse:left"]),
        spec("move", ActionKind::Axis2d, &["keyboard:W/S/A/D"]),
        spec("look", ActionKind::Axis2d, &["mouse:delta"]),
    ]);
    assert!(errors.is_empty(), "{errors:?}");

    let frame = |w: *mut FynkelWorld, press: bool| {
        let world = unsafe { &mut *w };
        let e = if press {
            InputEvent::down(Device::Keyboard, "Space")
        } else {
            InputEvent::up(Device::Keyboard, "Space")
        };
        world.core_mut().capture_input(&[e]);
        for i in 0..3 {
            world.core_mut().push_event(EventPayload::Destroyed {
                entity: Entity::from_bits(i + 1),
            });
        }
        let before = allocation_count();
        let view = unsafe { fynkel_world_input_view(w) };
        let events = unsafe { fynkel_world_drain_events(w) };
        let allocs = allocation_count() - before;
        assert!(view.length > 0 && events.length > 0);
        allocs
    };
    // Warm: the buffers grow to their size once.
    for i in 0..8 {
        frame(w, i % 2 == 0);
    }
    let steady: u64 = (0..100).map(|i| frame(w, i % 2 == 0)).sum();
    assert_eq!(steady, 0, "allocations in 100 warm frames");
    unsafe {
        fynkel_world_destroy(w);
        fynkel_runtime_destroy(rt);
    }
}
