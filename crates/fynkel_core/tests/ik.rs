//! Masks per node and IK (ADR-0033 § 5): a layer's mask keeps it off the
//! nodes it excludes; the IK goals bend the pose after all layers — an
//! arm reaching a target from code, a head turning to one, the feet
//! landing on uneven ground — and what they bend is taken back when they
//! stop. Deterministic.

use fynkel_contracts::{AssetRef, DiagnosticCode, Uuid};
use fynkel_core::animation::controller::{
    Blending, ControllerDef, IkDef, IkKind, IkWeight, LayerDef, MaskEntry, StateDef,
};
use fynkel_core::animation::{Channel, Clip, Interpolation, Property};
use fynkel_core::components::ComponentValue;
use fynkel_core::*;
use fynkel_scene as scene;
use glam::Vec3;

fn world() -> World {
    World::new(WorldConfig::default())
}

fn spawn(w: &mut World, temp_id: u64, at: Vec3, parent: Option<Entity>, name: &str) -> Entity {
    w.submit(
        Lane::Simulation,
        vec![Command::Spawn {
            temp_id,
            transform: Transform::from_position(at),
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

fn animate(w: &mut World, entity: Entity, op: AnimateOp) {
    w.submit(Lane::Simulation, vec![Command::Animate { entity, op }]);
    let r = w.step();
    assert_eq!(r.rejected, 0, "{:?}", w.diagnostics());
}

fn steps(w: &mut World, n: usize) {
    for _ in 0..n {
        w.step();
    }
}

fn at(w: &World, e: Entity) -> Vec3 {
    scene::world_matrix(w.ecs(), e).unwrap().w_axis.truncate()
}

fn controller_ref() -> AssetRef {
    AssetRef::new(Uuid::from_bytes(0xc1u128.to_be_bytes()))
}

fn clip_ref(n: u128) -> AssetRef {
    AssetRef::with_sub(Uuid::from_bytes(n.to_be_bytes()), format!("anim/{n}"))
}

/// Holds every target at `y` (a one-key clip).
fn hold(targets: &[&str], y: f32) -> Clip {
    Clip {
        name: format!("hold {y}"),
        duration: 1.0,
        markers: Vec::new(),
        channels: targets
            .iter()
            .map(|t| Channel {
                target: (*t).into(),
                property: Property::Translation,
                interpolation: Interpolation::Linear,
                times: vec![0.0],
                values: vec![0.0, y, 0.0],
            })
            .collect(),
    }
}

fn layer(name: &str, clip: Option<AssetRef>, mask: Vec<MaskEntry>) -> LayerDef {
    let states = clip
        .map(|c| {
            vec![StateDef {
                name: "Hold".into(),
                motion: Some(c.into()),
                speed: 1.0,
                speed_parameter: None,
                looping: true,
                root_motion: None,
            }]
        })
        .unwrap_or_default();
    LayerDef {
        name: name.into(),
        weight: 1.0,
        blending: Blending::Override,
        root: None,
        default_state: (!states.is_empty()).then_some(0),
        states,
        transitions: vec![],
        machines: vec![],
        mask,
    }
}

fn run(w: &mut World, root: Entity, def: ControllerDef) {
    w.register_controller(controller_ref(), def).unwrap();
    set(
        w,
        root,
        ComponentValue::Animator(Animator {
            controller: Some(controller_ref()),
            speed: 1.0,
            playing: true,
            ..Animator::default()
        }),
    );
}

#[test]
fn a_mask_keeps_the_layer_off_what_it_excludes() {
    let mut w = world();
    let names = ["Spine", "Head", "Eye", "Leg"];
    w.register_clip(clip_ref(1), hold(&names, 1.0));
    w.register_clip(clip_ref(2), hold(&names, 5.0));
    let root = spawn(&mut w, 1, Vec3::ZERO, None, "Rig");
    let spine = spawn(&mut w, 2, Vec3::ZERO, Some(root), "Spine");
    let head = spawn(&mut w, 3, Vec3::ZERO, Some(spine), "Head");
    let eye = spawn(&mut w, 4, Vec3::ZERO, Some(head), "Eye");
    let leg = spawn(&mut w, 5, Vec3::ZERO, Some(root), "Leg");
    let e = |node: &str, include, subtree| MaskEntry {
        node: node.into(),
        include,
        subtree,
    };
    run(
        &mut w,
        root,
        ControllerDef {
            name: "Masked".into(),
            parameters: vec![],
            layers: vec![
                layer("Base Layer", Some(clip_ref(1)), vec![]),
                // The ADR's example: the spine up, not the head itself.
                layer(
                    "Upper",
                    Some(clip_ref(2)),
                    vec![e("Spine", true, true), e("Head", false, false)],
                ),
            ],
            ik: vec![],
        },
    );
    steps(&mut w, 2);
    let y = |e: Entity| w.ecs().copy::<Transform>(e).unwrap().position.y;
    assert_eq!(
        [y(spine), y(head), y(eye), y(leg)],
        [5.0, 1.0, 5.0, 1.0],
        "the upper layer poses the spine and the eye, the base the rest"
    );
}

/// Rig ▸ Upper (1, 2, 0) ▸ Lower (0, -1, 0) ▸ Hand (0, -1, 0): a straight
/// arm down, two meters long.
fn arm(w: &mut World, ik: Vec<IkDef>) -> (Entity, [Entity; 3]) {
    let root = spawn(w, 1, Vec3::ZERO, None, "Rig");
    let upper = spawn(w, 2, Vec3::new(1.0, 2.0, 0.0), Some(root), "Upper");
    let lower = spawn(w, 3, Vec3::new(0.0, -1.0, 0.0), Some(upper), "Lower");
    let hand = spawn(w, 4, Vec3::new(0.0, -1.0, 0.0), Some(lower), "Hand");
    run(
        w,
        root,
        ControllerDef {
            name: "Arm".into(),
            parameters: vec![],
            layers: vec![layer("Base Layer", None, vec![])],
            ik,
        },
    );
    (root, [upper, lower, hand])
}

fn reach(pole: Option<&str>) -> IkDef {
    IkDef {
        name: "hand".into(),
        kind: IkKind::TwoBone {
            root: None,
            middle: None,
            tip: "Hand".into(),
            pole: pole.map(Into::into),
        },
        weight: IkWeight::Value(1.0),
        target: None,
    }
}

#[test]
fn two_bone_ik_reaches_a_target_from_code_and_lets_go() {
    let mut w = world();
    let (root, [upper, lower, hand]) = arm(&mut w, vec![reach(None)]);
    let target = Vec3::new(2.0, 1.0, 0.5);
    animate(
        &mut w,
        root,
        AnimateOp::SetIkTarget {
            goal: "hand".into(),
            position: target,
            weight: 1.0,
        },
    );
    w.step();
    assert!(at(&w, hand).distance(target) < 1e-3, "{}", at(&w, hand));
    // The bones keep their lengths.
    assert!((at(&w, upper).distance(at(&w, lower)) - 1.0).abs() < 1e-4);
    assert!((at(&w, lower).distance(at(&w, hand)) - 1.0).abs() < 1e-4);
    // Out of reach: as far as it goes, towards it.
    let far = Vec3::new(5.0, 2.0, 0.0);
    animate(
        &mut w,
        root,
        AnimateOp::SetIkTarget {
            goal: "hand".into(),
            position: far,
            weight: 1.0,
        },
    );
    w.step();
    let a = at(&w, upper);
    assert!((a.distance(at(&w, hand)) - 2.0).abs() < 1e-3);
    assert!((at(&w, hand) - a).normalize().dot((far - a).normalize()) > 0.999);
    // Half the weight: part of the way.
    animate(
        &mut w,
        root,
        AnimateOp::SetIkTarget {
            goal: "hand".into(),
            position: target,
            weight: 0.5,
        },
    );
    w.step();
    let d = at(&w, hand).distance(target);
    assert!(
        d > 1e-2 && d < Vec3::new(1.0, 0.0, 0.0).distance(target),
        "{d}"
    );
    // Cleared: back to the pose (nothing compounds, no layer poses it).
    animate(
        &mut w,
        root,
        AnimateOp::ClearIkTarget {
            goal: "hand".into(),
        },
    );
    w.step();
    assert!(
        at(&w, hand).distance(Vec3::new(1.0, 0.0, 0.0)) < 1e-4,
        "{}",
        at(&w, hand)
    );
    // A goal the controller does not have is rejected.
    w.submit(
        Lane::Simulation,
        vec![Command::Animate {
            entity: root,
            op: AnimateOp::ClearIkTarget {
                goal: "foot".into(),
            },
        }],
    );
    let r = w.step();
    assert_eq!(r.rejected, 1);
    assert!(
        w.diagnostics()
            .iter()
            .any(|d| d.message.contains("no IK goal `foot`")),
        "{:?}",
        w.diagnostics()
    );
}

#[test]
fn the_elbow_points_to_the_pole_and_a_target_can_be_an_object() {
    let mut w = world();
    let mut goal = reach(Some("Pole"));
    goal.target = Some("Target".into());
    let (_root, [_, lower, hand]) = arm(&mut w, vec![goal]);
    spawn(&mut w, 8, Vec3::new(1.0, 1.0, 3.0), None, "Pole");
    spawn(&mut w, 9, Vec3::new(1.0, 0.8, 0.0), None, "Target");
    steps(&mut w, 2);
    assert!(
        at(&w, hand).distance(Vec3::new(1.0, 0.8, 0.0)) < 1e-3,
        "{}",
        at(&w, hand)
    );
    assert!(
        at(&w, lower).z > 0.5,
        "the elbow bends towards the pole: {}",
        at(&w, lower)
    );
}

#[test]
fn look_at_turns_the_head_within_its_limit() {
    let mut w = world();
    let root = spawn(&mut w, 1, Vec3::ZERO, None, "Rig");
    let spine = spawn(&mut w, 2, Vec3::new(0.0, 1.0, 0.0), Some(root), "Spine");
    let head = spawn(&mut w, 3, Vec3::new(0.0, 0.6, 0.0), Some(spine), "Head");
    run(
        &mut w,
        root,
        ControllerDef {
            name: "Look".into(),
            parameters: vec![],
            layers: vec![layer("Base Layer", None, vec![])],
            ik: vec![IkDef {
                name: "look".into(),
                kind: IkKind::LookAt {
                    head: "Head".into(),
                    spine: vec!["Spine".into()],
                    limit: 60f32.to_radians(),
                    body: 0.25,
                },
                weight: IkWeight::Value(1.0),
                target: None,
            }],
        },
    );
    let forward = |w: &World, e: Entity| {
        scene::world_matrix(w.ecs(), e)
            .unwrap()
            .transform_vector3(Vec3::NEG_Z)
            .normalize()
    };
    // Ahead and a little up: it eases there.
    let ahead = Vec3::new(0.0, 2.6, -5.0);
    animate(
        &mut w,
        root,
        AnimateOp::SetIkTarget {
            goal: "look".into(),
            position: ahead,
            weight: 1.0,
        },
    );
    let first = forward(&w, head);
    steps(&mut w, 120);
    let settled = forward(&w, head);
    let want = (ahead - at(&w, head)).normalize();
    assert!(settled.dot(want) > 0.999, "{settled} vs {want}");
    assert!(first.dot(want) < settled.dot(want), "eased, not snapped");
    // The spine took a share of it.
    assert!(forward(&w, spine).y > 0.01);
    // Right behind the shoulder: at most the limit.
    animate(
        &mut w,
        root,
        AnimateOp::SetIkTarget {
            goal: "look".into(),
            position: Vec3::new(5.0, 1.6, 1.0),
            weight: 1.0,
        },
    );
    steps(&mut w, 200);
    let f = forward(&w, head);
    let yaw = f.x.atan2(-f.z).to_degrees();
    assert!((yaw - 60.0).abs() < 0.5, "{yaw}");
}

#[test]
fn a_goal_whose_node_is_missing_is_reported_once() {
    let mut w = world();
    let root = spawn(&mut w, 1, Vec3::ZERO, None, "Rig");
    run(
        &mut w,
        root,
        ControllerDef {
            name: "Look".into(),
            parameters: vec![],
            layers: vec![layer("Base Layer", None, vec![])],
            ik: vec![IkDef {
                name: "look".into(),
                kind: IkKind::LookAt {
                    head: "Head".into(),
                    spine: vec![],
                    limit: 1.0,
                    body: 0.0,
                },
                weight: IkWeight::Value(1.0),
                target: Some("Coin".into()),
            }],
        },
    );
    let mut seen = w
        .diagnostics()
        .iter()
        .filter(|d| d.code == DiagnosticCode::IK_UNAVAILABLE)
        .count();
    for _ in 0..5 {
        w.step();
        seen += w
            .diagnostics()
            .iter()
            .filter(|d| d.code == DiagnosticCode::IK_UNAVAILABLE)
            .count();
    }
    assert_eq!(seen, 1);
}

/// Rig ▸ Hips (0, 1, 0) ▸ each leg a thigh (±0.2, 0, 0), a knee
/// (0, -0.5, 0.02) and a foot (0, -0.5, -0.02): the feet at y = 0.
#[test]
fn foot_placement_lands_each_foot_and_lowers_the_hips() {
    let mut w = world();
    // Ground at 0 under the left foot, at -0.15 under the right.
    let left_floor = spawn(&mut w, 20, Vec3::new(-0.5, -0.5, 0.0), None, "Left floor");
    let right_floor = spawn(&mut w, 21, Vec3::new(0.5, -0.65, 0.0), None, "Right floor");
    for f in [left_floor, right_floor] {
        set(
            &mut w,
            f,
            ComponentValue::Collider(Collider {
                size: Vec3::new(0.5, 0.5, 2.0),
                ..Collider::default()
            }),
        );
    }
    let root = spawn(&mut w, 1, Vec3::ZERO, None, "Rig");
    let hips = spawn(&mut w, 2, Vec3::new(0.0, 1.0, 0.0), Some(root), "Hips");
    let mut feet = Vec::new();
    for (k, (side, x)) in [("Left", -0.2), ("Right", 0.2)].into_iter().enumerate() {
        let t = 3 + 3 * k as u64;
        let thigh = spawn(
            &mut w,
            t,
            Vec3::new(x, 0.0, 0.0),
            Some(hips),
            &format!("{side}UpLeg"),
        );
        let knee = spawn(
            &mut w,
            t + 1,
            Vec3::new(0.0, -0.5, 0.02),
            Some(thigh),
            &format!("{side}Leg"),
        );
        feet.push(spawn(
            &mut w,
            t + 2,
            Vec3::new(0.0, -0.5, -0.02),
            Some(knee),
            &format!("{side}Foot"),
        ));
    }
    run(
        &mut w,
        root,
        ControllerDef {
            name: "Feet".into(),
            parameters: vec![],
            layers: vec![layer("Base Layer", None, vec![])],
            ik: vec![IkDef {
                name: "feet".into(),
                kind: IkKind::FootPlacement {
                    hips: "Hips".into(),
                    left: "LeftFoot".into(),
                    right: "RightFoot".into(),
                    reach: 0.5,
                },
                weight: IkWeight::Value(1.0),
                target: None,
            }],
        },
    );
    steps(&mut w, 120);
    let (l, r) = (at(&w, feet[0]), at(&w, feet[1]));
    assert!(l.y.abs() < 5e-3, "the left foot on its ground: {l}");
    assert!(
        (r.y + 0.15).abs() < 5e-3,
        "the right foot on the lower ground: {r}"
    );
    assert!((at(&w, hips).y - 0.85).abs() < 5e-3, "{}", at(&w, hips));
    assert!(
        w.diagnostics()
            .iter()
            .all(|d| d.code != DiagnosticCode::IK_UNAVAILABLE),
        "{:?}",
        w.diagnostics()
    );

    // Physics off: skipped, said once, and the pose goes back.
    let mut modules = w.modules();
    modules.physics = false;
    w.set_modules(modules);
    w.step();
    assert!(
        w.diagnostics()
            .iter()
            .any(|d| d.code == DiagnosticCode::IK_UNAVAILABLE && d.message.contains("physics")),
        "{:?}",
        w.diagnostics()
    );
    assert!((at(&w, hips).y - 1.0).abs() < 1e-4, "{}", at(&w, hips));
}
