//! Animation (blueprint §19.2d item 19, docs/animation.md): `Animator`
//! plays a registered clip over the nodes found by name below it in the
//! pre-tick slot; `Command::Animate` edits playback at the tick boundary;
//! a crossfade fades the outgoing clip out; `Skin` resolves its joints and
//! the extract carries the palette.

use fynkel_contracts::{AssetRef, DiagnosticCode, Uuid, DEFAULT_FIXED_DT};
use fynkel_core::animation::{Channel, Clip, Interpolation, Joint, Property};
use fynkel_core::components::ComponentValue;
use fynkel_core::schema::SystemKind;
use fynkel_core::*;
use glam::{Mat4, Quat, Vec3};

const DT: f32 = DEFAULT_FIXED_DT as f32;

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

fn animate(w: &mut World, entity: Entity, op: AnimateOp) -> StepReport {
    w.submit(Lane::Simulation, vec![Command::Animate { entity, op }]);
    w.step()
}

fn local(w: &World, e: Entity) -> Transform {
    w.ecs().copy::<Transform>(e).unwrap()
}

fn uuid(n: u128) -> Uuid {
    Uuid::from_bytes(n.to_be_bytes())
}

fn clip_ref(n: u128) -> AssetRef {
    AssetRef::with_sub(uuid(n), format!("anim/{n}"))
}

/// "rise": `arm` translates 0 → (0, 1, 0) over 1 s, linear.
fn rise() -> Clip {
    Clip {
        name: "rise".into(),
        duration: 1.0,
        channels: vec![Channel {
            target: "arm".into(),
            property: Property::Translation,
            interpolation: Interpolation::Linear,
            times: vec![0.0, 1.0],
            values: vec![0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        }],
    }
}

/// "turn": `arm` holds y = 2, rotates identity → 90° about Y at 0.5 s
/// (step), and scales to 2 over 2 s.
fn turn() -> Clip {
    let q = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
    Clip {
        name: "turn".into(),
        duration: 2.0,
        channels: vec![
            Channel {
                target: "arm".into(),
                property: Property::Translation,
                interpolation: Interpolation::Linear,
                times: vec![0.0, 2.0],
                values: vec![0.0, 2.0, 0.0, 0.0, 2.0, 0.0],
            },
            Channel {
                target: "arm".into(),
                property: Property::Rotation,
                interpolation: Interpolation::Step,
                times: vec![0.0, 0.5],
                values: vec![0.0, 0.0, 0.0, 1.0, q.x, q.y, q.z, q.w],
            },
            Channel {
                target: "arm".into(),
                property: Property::Scale,
                interpolation: Interpolation::Linear,
                times: vec![0.0, 2.0],
                values: vec![1.0, 1.0, 1.0, 2.0, 2.0, 2.0],
            },
        ],
    }
}

fn animator(clip: AssetRef, looping: bool) -> ComponentValue {
    ComponentValue::Animator(Animator {
        clip,
        speed: 1.0,
        looping,
        playing: true,
    })
}

fn ended(w: &mut World) -> Vec<(Entity, AssetRef)> {
    w.drain_events()
        .into_iter()
        .filter_map(|e| match e.payload {
            EventPayload::AnimationEnded { entity, clip } => Some((entity, clip)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_looping_clip_drives_the_named_node_below_the_animator() {
    let mut w = world();
    w.register_clip(clip_ref(1), rise());
    let root = spawn(&mut w, 1, Vec3::ZERO, None, "Rig");
    // A same-named entity outside the subtree is never the target.
    let decoy = spawn(&mut w, 2, Vec3::new(5.0, 0.0, 0.0), None, "arm");
    let arm = spawn(&mut w, 3, Vec3::new(0.0, 0.0, 1.0), Some(root), "arm");
    set(&mut w, root, animator(clip_ref(1), true));
    // The tick that applied the component sampled t = 0: a translation
    // channel writes the whole vector (z included).
    assert!(local(&w, arm).position.abs_diff_eq(Vec3::ZERO, 1e-6));
    for _ in 0..30 {
        w.step();
    }
    let arm_pose = local(&w, arm);
    assert!(
        (arm_pose.position.y - 30.0 * DT).abs() < 1e-4,
        "{arm_pose:?}"
    );
    assert_eq!(local(&w, decoy).position, Vec3::new(5.0, 0.0, 0.0));
    let s = w.animator_state(root).unwrap();
    assert!((s.time - 30.0 * DT).abs() < 1e-4 && s.playing && s.looping);
    assert_eq!((s.duration, s.speed, s.fade_remaining), (1.0, 1.0, 0.0));
    assert_eq!(s.clip, clip_ref(1));
    // Wrap: 90 ticks in (1.5 s) the loop is at 0.5.
    for _ in 0..60 {
        w.step();
    }
    assert!((local(&w, arm).position.y - 0.5).abs() < 1e-3);
    assert!(ended(&mut w).is_empty(), "a loop never ends");
    // The system is counted in its slot, before the catalogue.
    let kinds: Vec<SystemKind> = w.system_times().iter().map(|t| t.system).collect();
    let at = |k: SystemKind| kinds.iter().position(|x| *x == k).unwrap();
    assert!(at(SystemKind::Commands) < at(SystemKind::Animation));
    assert!(at(SystemKind::Animation) < at(SystemKind::Rotator));
    assert_eq!(
        w.system_times()[at(SystemKind::Animation)].entities,
        1,
        "one animator, no skin"
    );
}

#[test]
fn a_one_shot_clip_ends_once_and_holds_its_last_pose() {
    let mut w = world();
    w.register_clip(clip_ref(1), rise());
    let root = spawn(&mut w, 1, Vec3::ZERO, None, "Rig");
    let arm = spawn(&mut w, 2, Vec3::ZERO, Some(root), "arm");
    set(&mut w, root, animator(clip_ref(1), false));
    let mut events = Vec::new();
    for _ in 0..70 {
        w.step();
        events.extend(ended(&mut w));
    }
    assert_eq!(events, vec![(root, clip_ref(1))], "exactly one end");
    assert!(
        (local(&w, arm).position.y - 1.0).abs() < 1e-5,
        "held at the end"
    );
    let s = w.animator_state(root).unwrap();
    assert!(!s.playing && (s.time - 1.0).abs() < 1e-5);
    assert!(!w.ecs().copy::<Animator>(root).unwrap().playing);
    // Resume at the end does not fire again; Play restarts from 0.
    animate(&mut w, root, AnimateOp::Resume);
    w.step();
    assert!(ended(&mut w).is_empty());
    animate(
        &mut w,
        root,
        AnimateOp::Play {
            clip: clip_ref(1),
            looping: false,
            speed: 2.0,
        },
    );
    assert!(
        (local(&w, arm).position.y).abs() < 1e-6,
        "t = 0 sampled at once"
    );
    for _ in 0..30 {
        w.step();
    }
    assert!(
        (local(&w, arm).position.y - 1.0).abs() < 1e-5,
        "twice as fast"
    );
    assert_eq!(ended(&mut w).len(), 1);
}

#[test]
fn playback_commands_edit_the_clock_and_are_validated() {
    let mut w = world();
    w.register_clip(clip_ref(1), rise());
    let root = spawn(&mut w, 1, Vec3::ZERO, None, "Rig");
    let arm = spawn(&mut w, 2, Vec3::ZERO, Some(root), "arm");
    let bare = spawn(&mut w, 3, Vec3::ZERO, None, "Bare");
    set(&mut w, root, animator(clip_ref(1), true));

    // Rejections: no Animator, non-finite speed, negative time/fade.
    let r = animate(&mut w, bare, AnimateOp::Pause);
    assert_eq!(r.rejected, 1);
    assert_eq!(w.diagnostics()[0].code, DiagnosticCode::MISSING_COMPONENT);
    let r = animate(&mut w, root, AnimateOp::SetSpeed(f32::NAN));
    assert_eq!(r.rejected, 1);
    assert_eq!(w.diagnostics()[0].code, DiagnosticCode::INVALID_PAYLOAD);
    let r = animate(&mut w, root, AnimateOp::SetTime(-1.0));
    assert_eq!(r.rejected, 1);
    let r = animate(
        &mut w,
        root,
        AnimateOp::Crossfade {
            clip: clip_ref(1),
            looping: true,
            speed: 1.0,
            seconds: -0.1,
        },
    );
    assert_eq!(r.rejected, 1);
    assert_eq!(w.diagnostics()[0].code, DiagnosticCode::INVALID_PAYLOAD);

    // SetTime jumps (clamped to the duration), Pause holds, SetSpeed
    // scales, Stop rewinds to 0 and holds there.
    animate(&mut w, root, AnimateOp::SetTime(0.25));
    assert!((local(&w, arm).position.y - 0.25).abs() < 1e-5);
    animate(&mut w, root, AnimateOp::SetTime(9.0));
    assert!((local(&w, arm).position.y - 1.0).abs() < 1e-5, "clamped");
    animate(&mut w, root, AnimateOp::SetTime(0.5));
    animate(&mut w, root, AnimateOp::Pause);
    for _ in 0..10 {
        w.step();
    }
    assert!((local(&w, arm).position.y - 0.5).abs() < 1e-5, "paused");
    assert!(!w.animator_state(root).unwrap().playing);
    animate(&mut w, root, AnimateOp::SetSpeed(-1.0));
    animate(&mut w, root, AnimateOp::Resume);
    for _ in 0..28 {
        w.step();
    }
    assert!(
        (w.animator_state(root).unwrap().time - DT).abs() < 1e-4,
        "29 ticks backwards from 0.5: {:?}",
        w.animator_state(root)
    );
    animate(&mut w, root, AnimateOp::Stop);
    for _ in 0..5 {
        w.step();
    }
    let s = w.animator_state(root).unwrap();
    assert!(!s.playing && s.time == 0.0);
    assert!((local(&w, arm).position.y).abs() < 1e-5);
}

#[test]
fn a_crossfade_blends_the_outgoing_clip_out() {
    let mut w = world();
    w.register_clip(clip_ref(1), rise());
    w.register_clip(clip_ref(2), turn());
    let root = spawn(&mut w, 1, Vec3::ZERO, None, "Rig");
    let arm = spawn(&mut w, 2, Vec3::ZERO, Some(root), "arm");
    set(&mut w, root, animator(clip_ref(1), true));
    // 30 ticks: arm at y = 0.5.
    for _ in 0..30 {
        w.step();
    }
    let r = animate(
        &mut w,
        root,
        AnimateOp::Crossfade {
            clip: clip_ref(2),
            looping: true,
            speed: 1.0,
            seconds: 0.5,
        },
    );
    assert_eq!(r.rejected, 0);
    // The command's tick: the new clip at t = 0 (y 2, scale 1, identity)
    // blended over the outgoing clip one tick further (y = 0.5 + dt) with
    // the incoming weight 1 - (0.5 - dt)/0.5 = 2·dt. Scale, which only
    // the incoming clip keys, is taken as is.
    let s = w.animator_state(root).unwrap();
    assert!((s.fade_remaining - (0.5 - DT)).abs() < 1e-5, "{s:?}");
    assert_eq!(s.clip, clip_ref(2));
    let t = local(&w, arm);
    let w_in = 2.0 * DT;
    let expect_y = (0.5 + DT) * (1.0 - w_in) + 2.0 * w_in;
    assert!(
        (t.position.y - expect_y).abs() < 1e-4,
        "{t:?} vs {expect_y}"
    );
    assert!((t.scale.x - 1.0).abs() < 1e-5);
    // Half-way through the fade (15 ticks, 0.25 s left): the outgoing
    // clip is at 0.75 (y 0.75), the incoming holds y 2 → 1.375; the
    // scale is the incoming clip's own (1 + 14·dt/2).
    for _ in 0..14 {
        w.step();
    }
    let t = local(&w, arm);
    assert!((t.position.y - 1.375).abs() < 1e-3, "{t:?}");
    assert!((t.scale.x - (1.0 + 14.0 * DT / 2.0)).abs() < 1e-4, "{t:?}");
    assert!(
        t.rotation.abs_diff_eq(Quat::IDENTITY, 1e-5),
        "the step key at 0.5 s is ahead"
    );
    // The fade over: the incoming clip alone (y 2), its step rotation
    // reached.
    for _ in 0..20 {
        w.step();
    }
    let s = w.animator_state(root).unwrap();
    assert_eq!(s.fade_remaining, 0.0);
    let t = local(&w, arm);
    assert!((t.position.y - 2.0).abs() < 1e-5, "{t:?}");
    assert!(
        t.rotation
            .dot(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2))
            .abs()
            > 0.999
    );
    // A crossfade to the same clip, or an instant one, has no outgoing side.
    animate(
        &mut w,
        root,
        AnimateOp::Crossfade {
            clip: clip_ref(2),
            looping: true,
            speed: 1.0,
            seconds: 0.0,
        },
    );
    assert_eq!(w.animator_state(root).unwrap().fade_remaining, 0.0);
}

#[test]
fn a_missing_clip_is_reported_once_and_plays_once_registered() {
    let mut w = world();
    let root = spawn(&mut w, 1, Vec3::ZERO, None, "Rig");
    let arm = spawn(&mut w, 2, Vec3::ZERO, Some(root), "arm");
    set(&mut w, root, animator(clip_ref(7), true));
    let reports: Vec<_> = w
        .diagnostics()
        .iter()
        .filter(|d| d.code == DiagnosticCode::CLIP_UNAVAILABLE)
        .cloned()
        .collect();
    assert_eq!(reports.len(), 1, "{:?}", w.diagnostics());
    assert_eq!(reports[0].entity, root);
    assert_eq!(
        reports[0].component,
        fynkel_contracts::ComponentId::ANIMATOR
    );
    for _ in 0..5 {
        w.step();
        assert!(w.diagnostics().is_empty(), "{:?}", w.diagnostics());
    }
    assert_eq!(local(&w, arm).position, Vec3::ZERO);
    assert!(w.animator_state(root).unwrap().duration == 0.0);
    w.register_clip(clip_ref(7), rise());
    w.step();
    assert!(w.diagnostics().is_empty());
    for _ in 0..5 {
        w.step();
    }
    assert!(local(&w, arm).position.y > 0.0, "playing now");
    // A nil clip plays nothing and reports nothing.
    set(&mut w, root, animator(AssetRef::default(), true));
    for _ in 0..3 {
        w.step();
    }
    assert!(w.diagnostics().is_empty());
    // Renaming the target drops it from the pose; a new entity with the
    // name is picked up.
    set(&mut w, root, animator(clip_ref(7), true));
    w.submit(
        Lane::Simulation,
        vec![Command::SetName {
            entity: arm,
            name: "leg".into(),
        }],
    );
    w.step();
    let frozen = local(&w, arm).position.y;
    let arm2 = spawn(&mut w, 3, Vec3::new(2.0, 0.0, 0.0), Some(root), "arm");
    for _ in 0..5 {
        w.step();
    }
    assert_eq!(local(&w, arm).position.y, frozen);
    assert!(local(&w, arm2).position.y > 0.0);
}

#[test]
fn a_skin_resolves_its_joints_and_the_extract_carries_the_palette() {
    let mut w = world();
    // Two joints: hips at the origin, head 1 up; bind pose as spawned.
    let skin = fynkel_core::animation::Skin {
        skeleton: "hips".into(),
        joints: vec![
            Joint {
                name: "hips".into(),
                parent: None,
                inverse_bind: Mat4::IDENTITY,
                rest: Transform::IDENTITY,
            },
            Joint {
                name: "head".into(),
                parent: Some(0),
                inverse_bind: Mat4::from_translation(Vec3::new(0.0, -1.0, 0.0)),
                rest: Transform::from_position(Vec3::new(0.0, 1.0, 0.0)),
            },
        ],
    };
    let skin_ref = AssetRef::with_sub(uuid(9), "skin/0");
    w.register_skin(skin_ref.clone(), skin);
    let root = spawn(&mut w, 1, Vec3::new(3.0, 0.0, 0.0), None, "Rig");
    let body = spawn(&mut w, 2, Vec3::ZERO, Some(root), "Body");
    let hips = spawn(&mut w, 3, Vec3::ZERO, Some(root), "hips");
    let head = spawn(&mut w, 4, Vec3::new(0.0, 1.0, 0.0), Some(hips), "head");
    set(
        &mut w,
        body,
        ComponentValue::MeshRenderer(MeshRenderer {
            mesh: AssetRef::with_sub(uuid(9), "mesh/0"),
            ..MeshRenderer::default()
        }),
    );
    // An unknown skin is reported once.
    set(
        &mut w,
        body,
        ComponentValue::Skin(Skin {
            skin: AssetRef::with_sub(uuid(9), "skin/9"),
            skeleton: "hips".into(),
        }),
    );
    assert_eq!(w.diagnostics().len(), 1, "{:?}", w.diagnostics());
    assert_eq!(w.diagnostics()[0].code, DiagnosticCode::SKIN_UNAVAILABLE);
    w.step();
    assert!(w.diagnostics().is_empty());
    let extract = w.extract();
    assert!(
        extract.meshes[0].skin.is_none(),
        "drawn static without a skin"
    );
    set(
        &mut w,
        body,
        ComponentValue::Skin(Skin {
            skin: skin_ref.clone(),
            skeleton: "hips".into(),
        }),
    );
    assert!(w.diagnostics().is_empty());
    let extract = w.extract();
    let es = extract.meshes[0].skin.expect("skinned");
    assert_eq!((es.first_joint, es.joint_count), (0, 2));
    assert_eq!(extract.joints.len(), 2);
    // At the bind pose every palette entry is the identity (the root's
    // own offset cancels: the palette is in mesh space).
    for m in &extract.joints {
        assert!(m.abs_diff_eq(Mat4::IDENTITY, 1e-5), "{m:?}");
    }
    // Move the head joint: its palette entry becomes the delta.
    w.submit(
        Lane::Simulation,
        vec![Command::SetTransform {
            entity: head,
            transform: Transform::from_position(Vec3::new(0.0, 1.5, 0.0)),
        }],
    );
    w.step();
    w.step();
    let extract = w.extract();
    assert!(extract.joints[0].abs_diff_eq(Mat4::IDENTITY, 1e-5));
    assert!(
        (extract.joints[1].transform_point3(Vec3::ZERO) - Vec3::new(0.0, 0.5, 0.0)).length() < 1e-4,
        "{:?}",
        extract.joints[1]
    );
    // A skeleton name that is nowhere: the mesh draws in its bind pose
    // (identity palette), no diagnostic — the joints simply resolve to
    // nothing.
    set(
        &mut w,
        body,
        ComponentValue::Skin(Skin {
            skin: skin_ref,
            skeleton: "pelvis".into(),
        }),
    );
    let extract = w.extract();
    assert!(extract.meshes[0].skin.is_some());
    for m in &extract.joints {
        assert!(m.abs_diff_eq(Mat4::IDENTITY, 1e-5));
    }
    assert!(w.diagnostics().is_empty(), "{:?}", w.diagnostics());
    assert_eq!(
        w.system_times()
            .iter()
            .find(|t| t.system == SystemKind::Animation)
            .unwrap()
            .entities,
        1,
        "the skin is counted"
    );
    let _ = root;
}
