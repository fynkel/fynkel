//! The Animator Controller in the tick (ADR-0032 step 3): an `Animator`
//! whose `controller` names a registered controller runs its states
//! instead of its clip — typed parameters and states by name from
//! `Command::Animate`, the state-entered and state-exited events, the
//! layers over each other, and the query with the state. Deterministic.

use fynkel_contracts::{AssetRef, DiagnosticCode, Uuid, DEFAULT_FIXED_DT};
use fynkel_core::animation::blend_tree::{ChildDef, MotionDef, TreeDef, TreeKind};
use fynkel_core::animation::controller::{
    Blending, Condition, ControllerDef, Interruption, LayerDef, ParameterDef, ParameterKind,
    StateDef, Test, TransitionDef, Value,
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
        motion: motion.map(Into::into),
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
        interruptible_by: Interruption::None,
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
        machines: vec![],
        mask: vec![],
    }
}

/// The ADR's player: Locomotion (rise, y 0 → 1 over 1 s, looping) and
/// Jump (y 5 → 6 over 0.5 s, once); `jump` fades into Jump over 0.1 s,
/// Jump returns at its end when `grounded`.
fn player() -> ControllerDef {
    ControllerDef {
        ik: vec![],
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
        ik: vec![],
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

/// A clip that holds `arm` at `at` for `seconds`.
fn hold(at: Vec3, seconds: f32) -> Clip {
    slide("arm", at, at, seconds)
}

fn child(motion: MotionDef, position: [f32; 2]) -> ChildDef {
    ChildDef {
        motion,
        position,
        time_scale: 1.0,
        parameter: None,
    }
}

/// ADR-0033 step 1: a Simple Directional tree on `moveX`/`moveY` —
/// idle at the centre (y 0, 1 s), forward (y 10, 2 s) and at 0.75 right
/// a 1D tree on `moveX` (x 10 at 0.5 s … x 20 at 1 s; its leaf at
/// time scale 2).
fn strafer() -> ControllerDef {
    let float = |name: &str| ParameterDef {
        name: name.into(),
        kind: ParameterKind::Float,
        default: Value::Float(0.0),
    };
    let right = TreeDef {
        kind: TreeKind::OneD,
        parameters: vec![0],
        children: vec![
            child(clip_ref(12).into(), [0.5, 0.0]),
            ChildDef {
                time_scale: 2.0,
                ..child(clip_ref(13).into(), [1.0, 0.0])
            },
        ],
    };
    let tree = TreeDef {
        kind: TreeKind::SimpleDirectional,
        parameters: vec![0, 1],
        children: vec![
            child(clip_ref(10).into(), [0.0, 0.0]),
            child(clip_ref(11).into(), [0.0, 1.0]),
            child(MotionDef::Tree(Box::new(right)), [0.75, 0.0]),
        ],
    };
    ControllerDef {
        ik: vec![],
        name: "Strafer".into(),
        parameters: vec![float("moveX"), float("moveY")],
        layers: vec![layer(
            "Base Layer",
            vec![StateDef {
                motion: Some(MotionDef::Tree(Box::new(tree))),
                ..state("Strafe", None, true)
            }],
            vec![],
        )],
    }
}

#[test]
fn a_blend_tree_weighs_its_leaves_and_their_lengths() {
    let mut w = world();
    w.register_clip(clip_ref(10), hold(Vec3::ZERO, 1.0));
    w.register_clip(clip_ref(11), hold(Vec3::new(0.0, 10.0, 0.0), 2.0));
    w.register_clip(clip_ref(12), hold(Vec3::new(10.0, 0.0, 0.0), 1.0));
    w.register_clip(clip_ref(13), hold(Vec3::new(20.0, 0.0, 0.0), 1.0));
    w.register_controller(controller_ref(2), strafer()).unwrap();
    let root = spawn(&mut w, 1, None, "Rig");
    let arm = spawn(&mut w, 2, Some(root), "arm");
    set(&mut w, root, controlled(controller_ref(2)));
    let at = |w: &World| w.ecs().copy::<Transform>(arm).unwrap().position;

    // At the centre: idle alone.
    w.step();
    assert!(at(&w).length() < 1e-5, "{}", at(&w));
    assert!((base(&w, root).length - 1.0).abs() < 1e-5);

    // Halfway forward: half idle, half forward.
    animate(&mut w, root, param("moveY", Value::Float(0.5)));
    w.step();
    assert!(
        (at(&w) - Vec3::new(0.0, 5.0, 0.0)).length() < 1e-4,
        "{}",
        at(&w)
    );
    let s = base(&w, root);
    assert!((s.length - 1.5).abs() < 1e-4, "{s:?}");

    // On the right child: the 1D tree is halfway between its two clips;
    // the second one plays twice as fast (1 s / 2).
    animate(&mut w, root, param("moveY", Value::Float(0.0)));
    animate(&mut w, root, param("moveX", Value::Float(0.75)));
    w.step();
    assert!(
        (at(&w) - Vec3::new(15.0, 0.0, 0.0)).length() < 1e-4,
        "{}",
        at(&w)
    );
    let s = base(&w, root);
    assert!((s.length - 0.75).abs() < 1e-4, "{s:?}");
}

#[test]
fn a_clips_root_travel_is_per_second_of_a_lap() {
    let mut w = world();
    // `arm` is the shallowest translation: 3 m forward (-Z) over 2 s.
    w.register_clip(
        clip_ref(20),
        slide("arm", Vec3::ZERO, Vec3::new(1.0, 0.5, -3.0), 2.0),
    );
    w.register_clip(clip_ref(21), hold(Vec3::ONE, 1.0));
    let root = spawn(&mut w, 1, None, "Rig");
    let _arm = spawn(&mut w, 2, Some(root), "arm");
    let (v, yaw) = w.clip_root_travel(root, &clip_ref(20)).unwrap();
    assert!((v - Vec3::new(0.5, 0.25, -1.5)).length() < 1e-6, "{v}");
    assert_eq!(yaw, 0.0);
    // In place: no travel. Not registered: nothing.
    assert_eq!(
        w.clip_root_travel(root, &clip_ref(21)).unwrap().0,
        Vec3::ZERO
    );
    assert!(w.clip_root_travel(root, &clip_ref(22)).is_none());
}

/// Base: Move (arm y 0 → 1 over 1 s, looping); UpperBody: no states,
/// only its slot (ADR-0033 § 2). Clip 3 holds the arm at y 10 for 1 s.
fn slotted(w: &mut World) -> (Entity, Entity) {
    w.register_clip(
        clip_ref(1),
        slide("arm", Vec3::ZERO, Vec3::new(0.0, 1.0, 0.0), 1.0),
    );
    w.register_clip(
        clip_ref(3),
        slide(
            "arm",
            Vec3::new(0.0, 10.0, 0.0),
            Vec3::new(0.0, 10.0, 0.0),
            1.0,
        ),
    );
    let def = ControllerDef {
        ik: vec![],
        name: "Slots".into(),
        parameters: Vec::new(),
        layers: vec![
            layer("Base", vec![state("Move", Some(clip_ref(1)), true)], vec![]),
            LayerDef {
                default_state: None,
                ..layer("UpperBody", vec![], vec![])
            },
        ],
    };
    w.register_controller(controller_ref(3), def).unwrap();
    let root = spawn(w, 1, None, "Rig");
    let arm = spawn(w, 2, Some(root), "arm");
    set(w, root, controlled(controller_ref(3)));
    (root, arm)
}

fn one_shot(slot: &str, blend_in: f32, blend_out: f32) -> AnimateOp {
    AnimateOp::PlayOneShot {
        motion: clip_ref(3),
        slot: slot.into(),
        blend_in,
        blend_out,
        speed: 1.0,
    }
}

fn ended(w: &mut World) -> Vec<AssetRef> {
    w.drain_events()
        .into_iter()
        .filter_map(|e| match e.payload {
            EventPayload::AnimationEnded { clip, .. } => Some(clip),
            _ => None,
        })
        .collect()
}

/// The arm's height for the machine's time `n` under a one-shot at `w`.
fn covered(n: f32, w: f32) -> f32 {
    let machine = n - n.floor();
    machine * (1.0 - w) + 10.0 * w
}

#[test]
fn a_one_shot_covers_the_machine_blending_in_and_out_and_ends() {
    let mut w = world();
    let (root, arm) = slotted(&mut w);
    for _ in 0..10 {
        w.step();
    }
    let report = animate(&mut w, root, one_shot("", 0.2, 0.2));
    assert_eq!(report.rejected, 0, "{:?}", w.diagnostics());
    let b = base(&w, root);
    assert_eq!(b.slot, "Base");
    assert_eq!(b.one_shot, clip_ref(3), "it plays in the base layer's slot");
    assert_eq!(b.one_shot_weight, 0.0, "its first tick is its start");
    ended(&mut w);
    let mut seen = Vec::new();
    for _ in 0..70 {
        w.step();
        let b = base(&w, root);
        let n = b.normalized_time;
        assert!(
            (y(&w, arm) - covered(n, b.one_shot_weight)).abs() < 1e-4,
            "{} vs {} at weight {}",
            y(&w, arm),
            covered(n, b.one_shot_weight),
            b.one_shot_weight
        );
        seen.push(b.one_shot_weight);
        let e = ended(&mut w);
        if !e.is_empty() {
            assert_eq!(e, [clip_ref(3)]);
            assert!(b.one_shot.is_nil(), "gone at its end");
            break;
        }
    }
    // It blended in, held at 1 in the middle, and blended out.
    assert!(seen.iter().any(|&x| x > 0.0 && x < 1.0));
    assert!(seen.contains(&1.0));
    assert!(seen.len() >= 59 && seen.len() <= 61, "{} ticks", seen.len());
    // The machine ran underneath: Move kept its clock all along.
    let b = base(&w, root);
    assert_eq!(b.state, "Move");
    assert!(
        (b.normalized_time - 71.0 * DT).abs() < 0.05,
        "{}",
        b.normalized_time
    );
}

#[test]
fn a_layer_without_states_shows_only_its_slot_and_a_stop_blends_out() {
    let mut w = world();
    let (root, arm) = slotted(&mut w);
    for _ in 0..3 {
        w.step();
    }
    let n = base(&w, root).normalized_time;
    assert!((y(&w, arm) - n).abs() < 1e-5, "an empty slot shows nothing");
    animate(&mut w, root, one_shot("UpperBody", 0.0, 0.0));
    w.step();
    assert!((y(&w, arm) - 10.0).abs() < 1e-5, "{}", y(&w, arm));
    let (layers, _) = w.animator_controller_state(root).unwrap();
    assert_eq!(layers[1].slot, "UpperBody");
    assert_eq!(layers[1].one_shot, clip_ref(3));
    assert!(layers[0].one_shot.is_nil());

    // Stopped: not playing at once, blending out over 0.1 s.
    let report = animate(
        &mut w,
        root,
        AnimateOp::StopOneShot {
            slot: "UpperBody".into(),
            blend_out: 0.1,
        },
    );
    assert_eq!(report.rejected, 0, "{:?}", w.diagnostics());
    let (layers, _) = w.animator_controller_state(root).unwrap();
    assert!(layers[1].one_shot.is_nil());
    let h = y(&w, arm);
    assert!(h > 1.0 && h < 10.0, "halfway out: {h}");
    for _ in 0..8 {
        w.step();
    }
    let n = base(&w, root).normalized_time;
    assert!((y(&w, arm) - n).abs() < 1e-4, "blended out: {}", y(&w, arm));
    assert!(ended(&mut w).is_empty(), "a stopped one-shot does not end");
}

#[test]
fn another_one_shot_blends_from_the_first() {
    let mut w = world();
    let (root, arm) = slotted(&mut w);
    w.register_clip(
        clip_ref(4),
        slide(
            "arm",
            Vec3::new(0.0, 20.0, 0.0),
            Vec3::new(0.0, 20.0, 0.0),
            1.0,
        ),
    );
    animate(&mut w, root, one_shot("UpperBody", 0.0, 0.0));
    w.step();
    assert!((y(&w, arm) - 10.0).abs() < 1e-5);
    animate(
        &mut w,
        root,
        AnimateOp::PlayOneShot {
            motion: clip_ref(4),
            slot: "UpperBody".into(),
            blend_in: 0.2,
            blend_out: 0.0,
            speed: 1.0,
        },
    );
    for _ in 0..6 {
        w.step();
    }
    let h = y(&w, arm);
    assert!(h > 10.0 && h < 20.0, "between the two: {h}");
    for _ in 0..10 {
        w.step();
    }
    assert!((y(&w, arm) - 20.0).abs() < 1e-4, "{}", y(&w, arm));
}

#[test]
fn a_one_shot_is_checked() {
    let mut w = world();
    let (root, _) = slotted(&mut w);
    fn why(w: &World, r: StepReport) -> String {
        assert_eq!(r.rejected, 1);
        w.diagnostics()[0].message.clone()
    }
    let r = animate(&mut w, root, one_shot("Arms", 0.1, 0.1));
    assert!(why(&w, r).contains("no slot `Arms` (a slot is a layer: Base, UpperBody)"));
    let r = animate(&mut w, root, one_shot("", -1.0, 0.1));
    assert!(why(&w, r).contains("blends must be finite"));
    let r = animate(
        &mut w,
        root,
        AnimateOp::PlayOneShot {
            motion: clip_ref(9),
            slot: String::new(),
            blend_in: 0.0,
            blend_out: 0.0,
            speed: 1.0,
        },
    );
    assert!(why(&w, r).contains("not a clip or blend space the world has"));
    // Without a controller: play the clip instead.
    let lone = spawn(&mut w, 9, None, "Lone");
    set(
        &mut w,
        lone,
        ComponentValue::Animator(Animator {
            speed: 1.0,
            ..Animator::default()
        }),
    );
    let r = animate(&mut w, lone, one_shot("", 0.1, 0.1));
    assert!(why(&w, r).contains("a one-shot plays in a controller's slot"));
}

/// ADR-0033 § 4: a crossfade that its transition lets the next state's
/// transitions interrupt fades on from the pose of that moment, held —
/// the arm does not jump to the state it was fading into.
#[test]
fn an_interruption_fades_from_the_pose_it_held() {
    let mut w = world();
    for (n, y) in [(11, 0.0), (12, 10.0), (13, 20.0)] {
        w.register_clip(clip_ref(n), hold(Vec3::new(0.0, y, 0.0), 1.0));
    }
    let go = |to: usize, from: usize, value: i32| TransitionDef {
        duration: 0.5,
        interruptible_by: Interruption::Next,
        ..transition(
            Some(from),
            to,
            vec![Condition {
                parameter: 0,
                test: Test::Equals(value),
            }],
        )
    };
    let def = ControllerDef {
        ik: vec![],
        name: "Interrupt".into(),
        parameters: vec![ParameterDef {
            name: "go".into(),
            kind: ParameterKind::Int,
            default: Value::Int(0),
        }],
        layers: vec![layer(
            "Base Layer",
            vec![
                state("A", Some(clip_ref(11)), true),
                state("B", Some(clip_ref(12)), true),
                state("C", Some(clip_ref(13)), true),
            ],
            vec![go(1, 0, 1), go(2, 1, 2)],
        )],
    };
    w.register_controller(controller_ref(4), def).unwrap();
    let root = spawn(&mut w, 1, None, "Rig");
    let arm = spawn(&mut w, 2, Some(root), "arm");
    set(&mut w, root, controlled(controller_ref(4)));
    w.step();
    animate(&mut w, root, param("go", Value::Int(1)));
    for _ in 0..6 {
        w.step();
    }
    let held = y(&w, arm);
    assert!(held > 0.5 && held < 9.5, "fading A → B: {held}");
    states(&mut w);
    // B → C cuts the fade: A exits, and the arm starts from where it was.
    animate(&mut w, root, param("go", Value::Int(2)));
    assert_eq!(states(&mut w), [exited("A"), entered("C", "B")]);
    assert!((y(&w, arm) - held).abs() < 1e-4, "{} vs {held}", y(&w, arm));
    let b = base(&w, root);
    assert_eq!((b.state.as_str(), b.next.as_str()), ("B", "C"));
    for _ in 0..3 {
        w.step();
    }
    let n = y(&w, arm);
    let weight = 3.0 * DEFAULT_FIXED_DT as f32 / 0.5;
    let want = held + (20.0 - held) * weight;
    assert!((n - want).abs() < 1e-3, "{n} vs {want}");
}
