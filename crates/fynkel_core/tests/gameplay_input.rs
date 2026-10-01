use fynkel_core::*;

fn button(name: &str, binding: &str) -> ActionSpec {
    ActionSpec {
        name: name.into(),
        kind: ActionKind::Button,
        bindings: vec![binding.into()],
    }
}
fn actions(world: &World) -> Vec<String> {
    world
        .input()
        .actions()
        .map(|(name, _)| name.to_owned())
        .collect()
}

#[test]
fn aborted_and_sticky_failed_edits_preserve_bindings_and_world_data() {
    let mut world = World::new(WorldConfig::default());
    world.bind_input(&[button("original", "keyboard:Space")]);
    world.capture_input(&[InputEvent::down(Device::Keyboard, "Space")]);
    let counters = world.input().counters();
    for invalid in [false, true] {
        assert!(world
            .edit_gameplay(|edit| {
                edit.spawn("discarded", Transform::IDENTITY, None)?;
                edit.bind_input(&[button("changed", "keyboard:A")])?;
                if invalid {
                    let bad = Transform {
                        position: glam::Vec3::splat(f32::NAN),
                        ..Default::default()
                    };
                    assert!(edit.spawn("bad", bad, None).is_err());
                    Ok(())
                } else {
                    Err(EditError::Aborted)
                }
            })
            .is_err());
        assert_eq!(world.live_entities(), 0);
        assert_eq!(actions(&world), ["original"]);
        assert_eq!(world.input().counters(), counters);
        let state = &world.input_view()[0];
        assert!(state.held && state.pressed && state.frame_pressed);
    }
}

#[test]
fn last_prepared_table_wins_and_invalid_grammar_matches_direct_report_and_skip() {
    let mut world = World::new(WorldConfig::default());
    let desired = vec![ActionSpec {
        name: "jump".into(),
        kind: ActionKind::Button,
        bindings: vec![
            "keyboard:Space".into(),
            "not-a-binding".into(),
            "keyboard:NoSuchKey".into(),
        ],
    }];
    let mut legacy = Input::default();
    let expected_errors = legacy.bind(&desired);
    assert_eq!(expected_errors.len(), 2);
    let commit = world
        .edit_gameplay(|edit| {
            edit.bind_input(&[button("discarded first table", "keyboard:A")])?;
            edit.bind_input(&desired)
        })
        .unwrap();
    assert_eq!(commit.value, expected_errors);
    assert_eq!(actions(&world), ["jump"]);
    world.capture_input(&[InputEvent::down(Device::Keyboard, "Space")]);
    assert!(world.input_view()[0].held);
    world.edit_gameplay(|edit| edit.bind_input(&[])).unwrap();
    assert!(
        actions(&world).is_empty(),
        "an empty final table clears bindings"
    );
}

#[test]
fn publish_preserves_raw_edges_relative_motion_capture_counters_and_clock() {
    let mut world = World::new(WorldConfig {
        fixed_dt: 1.0 / 120.0,
        ..Default::default()
    });
    world.bind_input(&[button("original", "keyboard:Space")]);
    world.capture_input(&[
        InputEvent::down(Device::Keyboard, "Space"),
        InputEvent::moved(Device::Mouse, "delta", 2.0, 3.0),
    ]);
    let counters = world.input().counters();
    let clocks = (
        world.tick(),
        world.clock().frame(),
        world.clock().real_elapsed(),
        world.clock().gameplay_elapsed(),
    );
    world
        .edit_gameplay(|edit| {
            edit.bind_input(&[
                button("renamed", "keyboard:Space"),
                ActionSpec {
                    name: "look".into(),
                    kind: ActionKind::Axis2d,
                    bindings: vec!["mouse:delta".into()],
                },
            ])
        })
        .unwrap();
    assert_eq!(world.input().counters(), counters);
    assert_eq!(
        (
            world.tick(),
            world.clock().frame(),
            world.clock().real_elapsed(),
            world.clock().gameplay_elapsed()
        ),
        clocks
    );
    let view = world.input_view();
    assert_eq!(view[0].name, "renamed");
    assert!(view[0].held && view[0].pressed && view[0].frame_pressed);
    assert!(
        (view[1].x - 240.0).abs() < 1e-4,
        "relative motion keeps the world's 120 Hz tick duration"
    );
    assert_eq!((view[1].frame_x, view[1].frame_y), (2.0, 3.0));
    world.step();
    let view = world.input_view();
    assert!(view[0].held && !view[0].pressed && view[0].frame_pressed);
    assert_eq!((view[1].x, view[1].y), (0.0, 0.0));
    world.capture_input(&[InputEvent::up(Device::Keyboard, "Space")]);
    assert!(world.input_view()[0].released);
    assert!(world.input_view()[0].frame_released);
}

#[test]
fn owned_session_queries_stay_committed_until_binding_publication_and_abort_is_inert() {
    let mut world = World::new(WorldConfig::default());
    world.bind_input(&[button("original", "keyboard:Space")]);
    world.capture_input(&[InputEvent::down(Device::Keyboard, "Space")]);
    let counters = world.input().counters();
    let mut session = GameplaySession::begin(world).unwrap_or_else(|_| panic!("begin"));
    session
        .access(|edit| edit.bind_input(&[button("aborted", "keyboard:A")]))
        .unwrap();
    assert_eq!(actions(session.committed()), ["original"]);
    assert!(session.committed().input_view()[0].pressed);
    let world = session.abort();
    assert_eq!(actions(&world), ["original"]);
    let mut session = GameplaySession::begin(world).unwrap_or_else(|_| panic!("begin"));
    session
        .access(|edit| edit.bind_input(&[button("committed", "keyboard:Space")]))
        .unwrap();
    assert_eq!(actions(session.committed()), ["original"]);
    let (world, result) = session.finish();
    result.unwrap();
    assert_eq!(actions(&world), ["committed"]);
    assert_eq!(world.input().counters(), counters);
    assert!(world.input_view()[0].pressed);
}

#[test]
fn final_reference_failure_does_not_publish_prepared_bindings() {
    let mut world = World::new(WorldConfig::default());
    world.bind_input(&[button("original", "keyboard:Space")]);
    assert!(world
        .edit_gameplay(|edit| {
            let owner = edit.spawn("owner", Transform::IDENTITY, None)?;
            let target = edit.spawn("target", Transform::IDENTITY, None)?;
            edit.set_component(
                owner,
                GameplayComponentValue::Follow(
                    Follow::default().map_object_refs(|_| target.into()),
                ),
            )?;
            edit.bind_input(&[button("discarded", "keyboard:A")])?;
            edit.destroy(target)?;
            Ok(())
        })
        .is_err());
    assert_eq!(world.live_entities(), 0);
    assert_eq!(actions(&world), ["original"]);
}
