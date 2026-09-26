//! The Animator Controller in the tick (ADR-0032 step 3): an `Animator`
//! whose `controller` names a registered controller runs its states
//! instead of its clip — typed parameters and states by name from
//! `Command::Animate`, the state-entered and state-exited events, the
//! layers over each other, and the query with the state. Deterministic.

use fynkel_contracts::{AssetRef, DiagnosticCode, Uuid, DEFAULT_FIXED_DT};
use fynkel_core::animation::controller::{
    Blending, Condition, ControllerDef, LayerDef, ParameterDef, ParameterKind, StateDef, Test,
    TransitionDef, Value,
};
use fynkel_core::animation::{Channel, Clip, Interpolation, Property};
use fynkel_core::components::ComponentValue;
use fynkel_core::*;
use glam::Vec3;

const DT: f32 = DEFAULT_FIXED_DT as f32;

fn world() -> World {
    World::new(WorldConfig::default())
}

fn spawn(w: &mut World, temp_id: u64, parent: Option<Entity>, name: &str) -> Entity {
    w.submit(
        Lane::Simulation,
        vec![Command::Spawn {
            temp_id,
            transform: Transform::from_position(Vec3::ZERO),
            parent,
            name: Some(name.to_owned()),
        }],
    );
    let r = w.step();
    assert_eq!(r.status, Status::Ok, "{:?}", w.diagnostics());
    w.drain_events()
        .into_iter()
        .find_map(|e| match e.payload {
            EventPayload::Spawned { temp_id: t, entity } if t == temp_id => Some(entity),
            _ => None,
        })
        .unwrap()
}

fn set(w: &mut World, entity: Entity, value: ComponentValue) {
    w.submit(
        Lane::Simulation,
        vec![Command::SetComponent { entity, value }],
    );
    let r = w.step();
    assert_eq!(r.status, Status::Ok, "{:?}", w.diagnostics());
}

fn animate(w: &mut World, entity: Entity, op: AnimateOp) -> StepReport {
    w.submit(Lane::Simulation, vec![Command::Animate { entity, op }]);
    w.step()
}

fn param(name: &str, value: Value) -> AnimateOp {
    AnimateOp::SetControllerParameter {
        name: name.into(),
        value,
    }
}

fn play_state(state: &str, seconds: f32) -> AnimateOp {
    AnimateOp::PlayState {
        state: state.into(),
        layer: None,
        seconds,
        offset: 0.0,
    }
}

fn y(w: &World, e: Entity) -> f32 {
    w.ecs().copy::<Transform>(e).unwrap().position.y
}

fn uuid(n: u128) -> Uuid {
    Uuid::from_bytes(n.to_be_bytes())
}

fn clip_ref(n: u128) -> AssetRef {
    AssetRef::with_sub(uuid(n), format!("anim/{n}"))
}

fn controller_ref(n: u128) -> AssetRef {
    AssetRef::new(uuid(0xc000 + n))
}

/// `arm` translates from `from` to `to` over `seconds`, linear.
fn slide(target: &str, from: Vec3, to: Vec3, seconds: f32) -> Clip {
    Clip {
        name: target.into(),
        duration: seconds,
        markers: Vec::new(),
        channels: vec![Channel {
            target: target.into(),
            property: Property::Translation,
            interpolation: Interpolation::Linear,
            times: vec![0.0, seconds],
            values: vec![from.x, from.y, from.z, to.x, to.y, to.z],
        }],
    }
}

fn state(name: &str, motion: Option<AssetRef>, looping: bool) -> StateDef {
    StateDef {
        name: name.into(),
        motion,
        speed: 1.0,
        speed_parameter: None,
        looping,
        root_motion: None,
    }
}

fn transition(from: Option<usize>, to: usize, conditions: Vec<Condition>) -> TransitionDef {
    TransitionDef {
        from,
        to,
        conditions,
        exit_time: None,
        duration: 0.0,
        offset: 0.0,
        can_transition_to_self: false,
    }
}

fn layer(name: &str, states: Vec<StateDef>, transitions: Vec<TransitionDef>) -> LayerDef {
    LayerDef {
        name: name.into(),
        weight: 1.0,
        blending: Blending::Override,
        root: None,
        default_state: Some(0),
        states,
        transitions,
    }
}

/// The ADR's player: Locomotion (rise, y 0 → 1 over 1 s, looping) and
/// Jump (y 5 → 6 over 0.5 s, once); `jump` fades into Jump over 0.1 s,
/// Jump returns at its end when `grounded`.
fn player() -> ControllerDef {
    ControllerDef {
        name: "Player".into(),
        parameters: vec![
            ParameterDef {
                name: "grounded".into(),
                kind: ParameterKind::Bool,
                default: Value::Bool(true),
            },
            ParameterDef {
                name: "jump".into(),
                kind: ParameterKind::Trigger,
                default: Value::Bool(false),
            },
            ParameterDef {
                name: "weapon".into(),
                kind: ParameterKind::Int,
                default: Value::Int(0),
            },
        ],
        layers: vec![layer(
            "Base Layer",
            vec![
                state("Locomotion", Some(clip_ref(1)), true),
                state("Jump", Some(clip_ref(2)), false),
            ],
            vec![
                TransitionDef {
                    duration: 0.1,
                    ..transition(
                        Some(0),
                        1,
                        vec![Condition {
                            parameter: 1,
                            test: Test::Set,
                        }],
                    )
                },
                TransitionDef {
                    exit_time: Some(1.0),
                    ..transition(
                        Some(1),
                        0,
                        vec![Condition {
                            parameter: 0,
                            test: Test::Is(true),
                        }],
                    )
                },
            ],
        )],
    }
}

fn rig(w: &mut World) -> (Entity, Entity) {
    w.register_clip(
        clip_ref(1),
        slide("arm", Vec3::ZERO, Vec3::new(0.0, 1.0, 0.0), 1.0),
    );
    w.register_clip(
        clip_ref(2),
        slide(
            "arm",
            Vec3::new(0.0, 5.0, 0.0),
            Vec3::new(0.0, 6.0, 0.0),
            0.5,
        ),
    );
    w.register_controller(controller_ref(1), player()).unwrap();
    let root = spawn(w, 1, None, "Rig");
    let arm = spawn(w, 2, Some(root), "arm");
    (root, arm)
}

fn controlled(controller: AssetRef) -> ComponentValue {
    ComponentValue::Animator(Animator {
        controller: Some(controller),
        speed: 1.0,
        playing: true,
        ..Animator::default()
    })
}

/// `(entered, layer, state, from)` of the events drained.
fn states(w: &mut World) -> Vec<(bool, u32, String, String)> {
    w.drain_events()
        .into_iter()
        .filter_map(|e| match e.payload {
            EventPayload::AnimatorStateEntered {
                layer, state, from, ..
            } => Some((true, layer, state, from)),
            EventPayload::AnimatorStateExited { layer, state, .. } => {
                Some((false, layer, state, String::new()))
            }
            _ => None,
        })
        .collect()
}

fn entered(state: &str, from: &str) -> (bool, u32, String, String) {
    (true, 0, state.into(), from.into())
}

fn exited(state: &str) -> (bool, u32, String, String) {
    (false, 0, state.into(), String::new())
}

fn base(w: &World, root: Entity) -> schema::AnimatorLayerStateRecord {
    w.animator_controller_state(root).unwrap().0.remove(0)
}

#[test]
fn a_whole_jump_on_a_trigger_and_back_when_grounded() {
    let mut w = world();
    let (root, arm) = rig(&mut w);
    set(&mut w, root, controlled(controller_ref(1)));
    // The first tick enters the default state at its start.
    assert_eq!(states(&mut w), [entered("Locomotion", "")]);
    assert!(y(&w, arm).abs() < 1e-6);
    for _ in 0..30 {
        w.step();
    }
    assert!((y(&w, arm) - 30.0 * DT).abs() < 1e-4, "{}", y(&w, arm));
    let s = base(&w, root);
    assert_eq!(
        (s.state.as_str(), s.in_transition, s.length),
        ("Locomotion", false, 1.0)
    );
    assert!((s.normalized_time - 30.0 * DT).abs() < 1e-4);

    // The trigger: Jump fades in over 0.1 s and uses the trigger up.
    let r = animate(&mut w, root, param("jump", Value::Bool(true)));
    assert_eq!(r.rejected, 0, "{:?}", w.diagnostics());
    assert_eq!(states(&mut w), [entered("Jump", "Locomotion")]);
    let s = base(&w, root);
    // Unity's view: the state fading out is current, the next fades in.
    assert!(s.in_transition, "{s:?}");
    assert_eq!((s.state.as_str(), s.next.as_str()), ("Locomotion", "Jump"));
    assert_eq!(s.transition_progress, 0.0, "it starts on the tick it fires");
    let params = w.animator_controller_state(root).unwrap().1;
    let jump = params.iter().find(|p| p.name == "jump").unwrap();
    assert_eq!(
        (jump.kind, jump.flag),
        (schema::AnimatorParameterKind::Trigger, false)
    );
    // Between the two clips while fading.
    w.step();
    let s = base(&w, root);
    assert!((s.transition_progress - DT / 0.1).abs() < 1e-4, "{s:?}");
    assert!(y(&w, arm) > 0.5 && y(&w, arm) < 5.0, "{}", y(&w, arm));

    // The fade over: Locomotion exits, Jump alone.
    let mut seen = states(&mut w);
    for _ in 0..8 {
        w.step();
        seen.extend(states(&mut w));
    }
    assert_eq!(seen, [exited("Locomotion")]);
    let s = base(&w, root);
    assert_eq!((s.state.as_str(), s.in_transition), ("Jump", false));
    assert!(y(&w, arm) >= 5.0, "{}", y(&w, arm));

    // In the air: the end is reached but `grounded` is false — Jump holds
    // its last frame.
    animate(&mut w, root, param("grounded", Value::Bool(false)));
    for _ in 0..60 {
        w.step();
    }
    assert!(states(&mut w).is_empty());
    let s = base(&w, root);
    assert_eq!(s.state, "Jump");
    assert!(s.normalized_time >= 1.0, "{s:?}");
    assert!((y(&w, arm) - 6.0).abs() < 1e-5, "{}", y(&w, arm));

    // Landing: back to Locomotion at once (no fade), at its start.
    animate(&mut w, root, param("grounded", Value::Bool(true)));
    assert_eq!(
        states(&mut w),
        [exited("Jump"), entered("Locomotion", "Jump")]
    );
    let s = base(&w, root);
    assert_eq!((s.state.as_str(), s.in_transition), ("Locomotion", false));
    assert!(y(&w, arm) < 0.1, "{}", y(&w, arm));
}

/// The same commands give the same poses, events and states, bit for bit.
#[test]
fn a_controller_run_is_deterministic() {
    fn run() -> Vec<String> {
        let mut w = world();
        let (root, arm) = rig(&mut w);
        set(&mut w, root, controlled(controller_ref(1)));
        let mut log = Vec::new();
        for tick in 0..240 {
            let op = match tick {
                17 => Some(param("jump", Value::Bool(true))),
                40 => Some(param("grounded", Value::Bool(false))),
                90 => Some(param("grounded", Value::Bool(true))),
                120 => Some(play_state("Jump", 0.07)),
                150 => Some(param("weapon", Value::Int(3))),
                _ => None,
            };
            if let Some(op) = op {
                w.submit(
                    Lane::Simulation,
                    vec![Command::Animate { entity: root, op }],
                );
            }
            w.step();
            let t = w.ecs().copy::<Transform>(arm).unwrap();
            log.push(format!(
                "{:?} {:?} {:?}",
                t.position.to_array().map(f32::to_bits),
                states(&mut w),
                w.animator_controller_state(root)
            ));
        }
        log
    }
    assert_eq!(run(), run());
}

#[test]
fn states_and_parameters_are_set_by_name_and_checked() {
    let mut w = world();
    let (root, arm) = rig(&mut w);
    let bare = spawn(&mut w, 3, None, "Bare");
    set(
        &mut w,
        bare,
        ComponentValue::Animator(Animator {
            clip: clip_ref(1),
            ..Animator::default()
        }),
    );
    set(&mut w, root, controlled(controller_ref(1)));
    states(&mut w);

    let rejected = |w: &mut World, e: Entity, op: AnimateOp, says: &str| {
        let r = animate(w, e, op);
        assert_eq!(r.rejected, 1);
        let d = &w.diagnostics()[0];
        assert_eq!(d.code, DiagnosticCode::INVALID_PAYLOAD);
        assert!(d.message.contains(says), "{:?} lacks {says:?}", d.message);
    };
    // A clip animator has no states; a controlled one plays no clips.
    rejected(&mut w, bare, play_state("Jump", 0.0), "no controller");
    rejected(
        &mut w,
        root,
        AnimateOp::Play {
            clip: clip_ref(1),
            looping: true,
            speed: 1.0,
        },
        "runs a controller",
    );
    // Names and types, the names there listed.
    rejected(
        &mut w,
        root,
        param("run", Value::Bool(true)),
        "the parameters: grounded, jump, weapon",
    );
    rejected(&mut w, root, param("weapon", Value::Float(1.0)), "is a int");
    rejected(&mut w, root, param("grounded", Value::Int(1)), "is a bool");
    rejected(
        &mut w,
        root,
        play_state("Fly", 0.0),
        "the states: Locomotion, Jump",
    );
    rejected(
        &mut w,
        root,
        AnimateOp::PlayState {
            state: "Jump".into(),
            layer: Some(2),
            seconds: 0.0,
            offset: 0.0,
        },
        "in layer 2",
    );
    rejected(&mut w, root, play_state("Jump", -1.0), "seconds");
    rejected(
        &mut w,
        root,
        AnimateOp::PlayState {
            state: "Jump".into(),
            layer: None,
            seconds: 0.0,
            offset: 1.5,
        },
        "offset",
    );
    assert!(states(&mut w).is_empty(), "nothing rejected moved");

    // Play: the state at once, sampled at its start that tick.
    let r = animate(&mut w, root, play_state("Jump", 0.0));
    assert_eq!(r.rejected, 0, "{:?}", w.diagnostics());
    assert_eq!(
        states(&mut w),
        [exited("Locomotion"), entered("Jump", "Locomotion")]
    );
    assert!((y(&w, arm) - 5.0).abs() < 1e-5, "{}", y(&w, arm));
    // CrossFade with an offset: Locomotion from its middle, fading in.
    let r = animate(
        &mut w,
        root,
        AnimateOp::PlayState {
            state: "Locomotion".into(),
            layer: Some(0),
            seconds: 0.2,
            offset: 0.5,
        },
    );
    assert_eq!(r.rejected, 0, "{:?}", w.diagnostics());
    let s = base(&w, root);
    assert_eq!((s.state.as_str(), s.next.as_str()), ("Jump", "Locomotion"));
    assert!((s.next_normalized_time - 0.5).abs() < 1e-5, "{s:?}");
    // An int reads back as set.
    animate(&mut w, root, param("weapon", Value::Int(-3)));
    let params = w.animator_controller_state(root).unwrap().1;
    let weapon = params.iter().find(|p| p.name == "weapon").unwrap();
    assert_eq!((weapon.integer, weapon.value), (-3, -3.0));
    // A clip animator has no controller state.
    assert!(w.animator_controller_state(bare).is_none());
}

#[test]
fn a_layer_overrides_at_its_weight_or_adds_from_its_root_node() {
    let mut w = world();
    // Base: arm y 0 → 1; "Aim": arm held at y 10; "Wave": hand slides
    // x 0 → 1 (its delta from frame 0 is +x).
    w.register_clip(
        clip_ref(1),
        slide("arm", Vec3::ZERO, Vec3::new(0.0, 1.0, 0.0), 1.0),
    );
    w.register_clip(
        clip_ref(2),
        slide(
            "arm",
            Vec3::new(0.0, 10.0, 0.0),
            Vec3::new(0.0, 10.0, 0.0),
            1.0,
        ),
    );
    w.register_clip(
        clip_ref(3),
        slide(
            "hand",
            Vec3::new(0.0, 2.0, 0.0),
            Vec3::new(1.0, 2.0, 0.0),
            1.0,
        ),
    );
    let def = ControllerDef {
        name: "Layers".into(),
        parameters: Vec::new(),
        layers: vec![
            layer("Base", vec![state("Move", Some(clip_ref(1)), true)], vec![]),
            LayerDef {
                weight: 0.5,
                ..layer("Aim", vec![state("Aim", Some(clip_ref(2)), true)], vec![])
            },
            LayerDef {
                blending: Blending::Additive,
                // The mask by placement: only what is under `arm`.
                root: Some("arm".into()),
                ..layer("Wave", vec![state("Wave", Some(clip_ref(3)), true)], vec![])
            },
        ],
    };
    w.register_controller(controller_ref(2), def).unwrap();
    let root = spawn(&mut w, 1, None, "Rig");
    let arm = spawn(&mut w, 2, Some(root), "arm");
    let hand = spawn(&mut w, 3, Some(arm), "hand");
    // A `hand` beside the arm is outside the Wave layer's root.
    let other = spawn(&mut w, 4, Some(root), "hand");
    set(&mut w, root, controlled(controller_ref(2)));
    for _ in 0..30 {
        w.step();
    }
    let t = 30.0 * DT;
    // Override at 0.5: halfway between the base and the layer.
    assert!(
        (y(&w, arm) - (t * 0.5 + 10.0 * 0.5)).abs() < 1e-4,
        "{}",
        y(&w, arm)
    );
    // Additive: the layer's delta over the hand's own transform.
    let h = w.ecs().copy::<Transform>(hand).unwrap().position;
    assert!((h.x - t).abs() < 1e-4 && h.y.abs() < 1e-5, "{h:?}");
    assert_eq!(
        w.ecs().copy::<Transform>(other).unwrap().position,
        Vec3::ZERO
    );
    let (layers, _) = w.animator_controller_state(root).unwrap();
    let names: Vec<_> = layers.iter().map(|l| (l.layer, l.state.as_str())).collect();
    assert_eq!(names, [(0, "Move"), (1, "Aim"), (2, "Wave")]);
}

#[test]
fn a_missing_controller_is_reported_once_and_runs_once_registered() {
    let mut w = world();
    w.register_clip(
        clip_ref(1),
        slide("arm", Vec3::ZERO, Vec3::new(0.0, 1.0, 0.0), 1.0),
    );
    let root = spawn(&mut w, 1, None, "Rig");
    let arm = spawn(&mut w, 2, Some(root), "arm");
    set(&mut w, root, controlled(controller_ref(1)));
    let reports: Vec<_> = w
        .diagnostics()
        .iter()
        .filter(|d| d.code == DiagnosticCode::CLIP_UNAVAILABLE)
        .cloned()
        .collect();
    assert_eq!(reports.len(), 1, "{:?}", w.diagnostics());
    assert!(
        reports[0].message.contains("controller"),
        "{:?}",
        reports[0]
    );
    for _ in 0..3 {
        w.step();
        assert!(w.diagnostics().is_empty(), "{:?}", w.diagnostics());
    }
    assert!(w.animator_controller_state(root).is_none());
    assert_eq!(y(&w, arm), 0.0);

    // An invalid controller is refused; a valid one runs.
    let mut bad = player();
    bad.layers[0].default_state = Some(9);
    assert!(w.register_controller(controller_ref(1), bad).is_err());
    w.register_clip(
        clip_ref(2),
        slide(
            "arm",
            Vec3::new(0.0, 5.0, 0.0),
            Vec3::new(0.0, 6.0, 0.0),
            0.5,
        ),
    );
    w.register_controller(controller_ref(1), player()).unwrap();
    w.step();
    assert_eq!(states(&mut w), [entered("Locomotion", "")]);
    for _ in 0..10 {
        w.step();
    }
    assert!(y(&w, arm) > 0.0);
    // Back to a clip: the controller's run is dropped.
    set(
        &mut w,
        root,
        ComponentValue::Animator(Animator {
            clip: clip_ref(2),
            playing: true,
            speed: 1.0,
            ..Animator::default()
        }),
    );
    assert!(w.animator_controller_state(root).is_none());
    assert!(y(&w, arm) >= 5.0);
}
