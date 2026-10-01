//! Animation (blueprint §19.2d item 19, docs/animation.md): `Animator`
//! plays a registered clip over the nodes found by name below it in the
//! pre-tick slot; `Command::Animate` edits playback at the tick boundary;
//! a crossfade fades the outgoing clip out; `Skin` resolves its joints and
//! the extract carries the palette. M3-19c (item 19c): markers raise
//! `ANIMATION_MARKER` per crossing, a nested animator is a layer at its
//! weight (additive or not), root motion leaves the pose and moves the
//! entity or its character.

use fynkel_contracts::{AssetRef, DiagnosticCode, ObjectRef, Uuid, DEFAULT_FIXED_DT};
use fynkel_core::animation::{Channel, Clip, Interpolation, Joint, Marker, Property};
use fynkel_core::components::ComponentValue;
use fynkel_core::schema::SystemKind;
use fynkel_core::*;
use fynkel_physics::{CharacterController, Collider};
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
        markers: Vec::new(),
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
        markers: Vec::new(),
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
        ..Animator::default()
    })
}

fn markers(w: &mut World) -> Vec<(Entity, AssetRef, String)> {
    w.drain_events()
        .into_iter()
        .filter_map(|e| match e.payload {
            EventPayload::AnimationMarker { entity, clip, name } => Some((entity, clip, name)),
            _ => None,
        })
        .collect()
}

/// A constant translation channel on `target`.
fn hold(target: &str, at: Vec3, duration: f32) -> Channel {
    Channel {
        target: target.into(),
        property: Property::Translation,
        interpolation: Interpolation::Linear,
        times: vec![0.0, duration],
        values: vec![at.x, at.y, at.z, at.x, at.y, at.z],
    }
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
            skeleton: ObjectRef::to(hips),
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
            skeleton: ObjectRef::to(hips),
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
    // A destroyed skeleton (ADR-0035 §5): the mesh draws in its bind pose
    // (identity palette), no diagnostic — nothing searches for another.
    let pelvis = spawn(&mut w, 5, Vec3::ZERO, Some(root), "pelvis");
    w.submit(Lane::Simulation, vec![Command::Destroy(pelvis)]);
    w.step();
    set(
        &mut w,
        body,
        ComponentValue::Skin(Skin {
            skin: skin_ref,
            skeleton: ObjectRef::to(pelvis),
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

#[test]
fn markers_fire_once_per_crossing_in_both_directions_and_across_wraps() {
    let mut rise = rise();
    rise.markers = vec![
        Marker {
            time: 0.25,
            name: "step".into(),
        },
        Marker {
            time: 1.0,
            name: "end".into(),
        },
    ];
    let mut w = world();
    w.register_clip(clip_ref(1), rise);
    let fwd = spawn(&mut w, 1, Vec3::ZERO, None, "Fwd");
    spawn(&mut w, 2, Vec3::ZERO, Some(fwd), "arm");
    let back = spawn(&mut w, 3, Vec3::ZERO, None, "Back");
    spawn(&mut w, 4, Vec3::ZERO, Some(back), "arm");
    set(&mut w, fwd, animator(clip_ref(1), true));
    set(
        &mut w,
        back,
        ComponentValue::Animator(Animator {
            clip: clip_ref(1),
            speed: -1.0,
            ..Animator::default()
        }),
    );
    // The ticks that set the clocks crossed nothing.
    assert!(markers(&mut w).is_empty());
    let mut seen: Vec<(Entity, AssetRef, String)> = Vec::new();
    for _ in 0..130 {
        w.step();
        seen.extend(markers(&mut w));
    }
    // 2.17 s forward: step at 0.25 and 1.25, end at 1.0 and 2.0 (the wrap
    // crosses the marker on the last frame); backward from 1.0: step at
    // 0.75 s and 1.75 s in, end (the frame the wrap lands on) at 1 s and
    // 2 s in.
    let of = |e: Entity, name: &str| {
        seen.iter()
            .filter(|(x, c, n)| *x == e && *c == clip_ref(1) && n == name)
            .count()
    };
    assert_eq!((of(fwd, "step"), of(fwd, "end")), (2, 2), "{seen:?}");
    assert_eq!((of(back, "step"), of(back, "end")), (2, 2), "{seen:?}");
    let fwd_order: Vec<&str> = seen
        .iter()
        .filter(|(x, ..)| *x == fwd)
        .map(|(_, _, n)| n.as_str())
        .collect();
    assert_eq!(fwd_order, ["step", "end", "step", "end"]);
    let back_order: Vec<&str> = seen
        .iter()
        .filter(|(x, ..)| *x == back)
        .map(|(_, _, n)| n.as_str())
        .collect();
    assert_eq!(back_order, ["step", "end", "step", "end"]);
    // A one-shot clip crosses its last-frame marker once and stops; a
    // seek does not fire what it jumps over.
    let r = animate(
        &mut w,
        fwd,
        AnimateOp::Play {
            clip: clip_ref(1),
            looping: false,
            speed: 1.0,
        },
    );
    assert_eq!(r.rejected, 0);
    let _ = markers(&mut w);
    let r = animate(&mut w, fwd, AnimateOp::SetTime(0.5));
    assert_eq!(r.rejected, 0);
    assert!(markers(&mut w).is_empty(), "a seek crosses nothing");
    let mut seen = Vec::new();
    for _ in 0..60 {
        w.step();
        seen.extend(markers(&mut w));
    }
    let names: Vec<&str> = seen
        .iter()
        .filter(|(x, ..)| *x == fwd)
        .map(|(_, _, n)| n.as_str())
        .collect();
    assert_eq!(names, ["end"], "{seen:?}");
    assert!(!w.animator_state(fwd).unwrap().playing);
}

#[test]
fn a_nested_animator_is_a_layer_at_its_weight_and_can_be_additive() {
    let mut w = world();
    w.register_clip(clip_ref(1), rise());
    // "up": arm held at y = 10.
    w.register_clip(
        clip_ref(2),
        Clip {
            name: "up".into(),
            duration: 1.0,
            markers: Vec::new(),
            channels: vec![hold("arm", Vec3::new(0.0, 10.0, 0.0), 1.0)],
        },
    );
    // "wave": arm slides x 0 → 1 over 1 s (its delta from frame 0 is +x).
    w.register_clip(
        clip_ref(3),
        Clip {
            name: "wave".into(),
            duration: 1.0,
            markers: Vec::new(),
            channels: vec![Channel {
                target: "arm".into(),
                property: Property::Translation,
                interpolation: Interpolation::Linear,
                times: vec![0.0, 1.0],
                values: vec![0.0, 5.0, 0.0, 1.0, 5.0, 0.0],
            }],
        },
    );
    let root = spawn(&mut w, 1, Vec3::ZERO, None, "Rig");
    let arm = spawn(&mut w, 2, Vec3::ZERO, Some(root), "arm");
    set(&mut w, root, animator(clip_ref(1), true));
    // The layer on the arm itself masks it: it applies after the base.
    set(
        &mut w,
        arm,
        ComponentValue::Animator(Animator {
            clip: clip_ref(2),
            weight: 0.5,
            ..Animator::default()
        }),
    );
    for _ in 0..30 {
        w.step();
    }
    // Base put the arm at its clock, the layer pulls it halfway to 10.
    let y = local(&w, arm).position.y;
    let base_y = w.animator_state(root).unwrap().time;
    assert!(
        (y - (base_y + 10.0) / 2.0).abs() < 1e-3,
        "{y} base at {base_y}"
    );
    assert_eq!(
        w.system_times()
            .iter()
            .find(|t| t.system == SystemKind::Animation)
            .unwrap()
            .entities,
        2
    );
    // Weight 0 leaves the base alone; weight 1 replaces it.
    let r = animate(&mut w, arm, AnimateOp::SetWeight(0.0));
    assert_eq!(r.rejected, 0);
    let y = local(&w, arm).position.y;
    let base_y = w.animator_state(root).unwrap().time;
    assert!((y - base_y).abs() < 1e-3, "{y} base at {base_y}");
    let r = animate(&mut w, arm, AnimateOp::SetWeight(1.0));
    assert_eq!(r.rejected, 0);
    assert!((local(&w, arm).position.y - 10.0).abs() < 1e-5);
    assert_eq!(w.animator_state(arm).unwrap().weight, 1.0);
    let r = animate(&mut w, arm, AnimateOp::SetWeight(1.5));
    assert_eq!(r.rejected, 1);
    assert_eq!(w.diagnostics()[0].code, DiagnosticCode::INVALID_PAYLOAD);
    // Additive: the wave's delta (x, not its y = 5) over the base pose.
    set(
        &mut w,
        arm,
        ComponentValue::Animator(Animator {
            clip: clip_ref(3),
            additive: true,
            weight: 0.5,
            ..Animator::default()
        }),
    );
    for _ in 0..30 {
        w.step();
    }
    let p = local(&w, arm).position;
    let base_y = w.animator_state(root).unwrap().time;
    assert!((p.x - 0.25).abs() < 1e-3, "{p:?}");
    assert!((p.y - base_y).abs() < 1e-3, "{p:?} base at {base_y}");
    // A parameter is kept per animator and reported; rejected unnamed.
    let r = animate(
        &mut w,
        arm,
        AnimateOp::SetParameter {
            name: "speed".into(),
            value: 3.5,
        },
    );
    assert_eq!(r.rejected, 0);
    assert_eq!(w.animator_state(arm).unwrap().parameter, 3.5);
    let r = animate(
        &mut w,
        arm,
        AnimateOp::SetParameter {
            name: String::new(),
            value: 1.0,
        },
    );
    assert_eq!(r.rejected, 1);
}

/// "walk": the hips (under the rig) move z 0 → -2 (forward) over 1 s at
/// height 1 and turn 0 → 90° about Y; the head above them holds its
/// place.
fn walk(turn: bool) -> Clip {
    let q = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
    let mut channels = vec![
        Channel {
            target: "hips".into(),
            property: Property::Translation,
            interpolation: Interpolation::Linear,
            times: vec![0.0, 1.0],
            values: vec![0.0, 1.0, 0.0, 0.0, 1.0, -2.0],
        },
        hold("head", Vec3::new(0.0, 0.5, 0.0), 1.0),
    ];
    if turn {
        channels.push(Channel {
            target: "hips".into(),
            property: Property::Rotation,
            interpolation: Interpolation::Linear,
            times: vec![0.0, 1.0],
            values: vec![0.0, 0.0, 0.0, 1.0, q.x, q.y, q.z, q.w],
        });
    }
    Clip {
        name: "walk".into(),
        duration: 1.0,
        markers: Vec::new(),
        channels,
    }
}

#[test]
fn root_motion_leaves_the_pose_and_moves_the_entity() {
    let mut w = world();
    w.register_clip(clip_ref(1), walk(false));
    w.register_clip(clip_ref(2), walk(true));
    let rig = spawn(&mut w, 1, Vec3::new(3.0, 0.0, 0.0), None, "Rig");
    let hips = spawn(&mut w, 2, Vec3::ZERO, Some(rig), "hips");
    let head = spawn(&mut w, 3, Vec3::ZERO, Some(hips), "head");
    set(
        &mut w,
        rig,
        ComponentValue::Animator(Animator {
            clip: clip_ref(1),
            root_motion: true,
            ..Animator::default()
        }),
    );
    // The tick that set the animator sampled the start; 30 more advance.
    for _ in 0..30 {
        w.step();
    }
    // The hips stay on their first-frame spot at the clip's height, the
    // head is posed as authored, the rig walked 1 m forward (-z).
    assert!(local(&w, hips)
        .position
        .abs_diff_eq(Vec3::new(0.0, 1.0, 0.0), 1e-5));
    assert!(local(&w, head)
        .position
        .abs_diff_eq(Vec3::new(0.0, 0.5, 0.0), 1e-5));
    let p = local(&w, rig).position;
    assert!(p.abs_diff_eq(Vec3::new(3.0, 0.0, -1.0), 1e-3), "{p:?}");
    let s = w.animator_state(rig).unwrap();
    assert!(
        s.root_delta
            .abs_diff_eq(Vec3::new(0.0, 0.0, -2.0 * DT), 1e-5),
        "{:?}",
        s.root_delta
    );
    assert_eq!(s.root_yaw, 0.0);
    // Across the wrap the lap's gain carries: 90 ticks = 3 m.
    for _ in 0..60 {
        w.step();
    }
    let p = local(&w, rig).position;
    assert!(p.abs_diff_eq(Vec3::new(3.0, 0.0, -3.0), 1e-2), "{p:?}");
    // Backwards it walks back (the tick that set the speed advanced too).
    let r = animate(&mut w, rig, AnimateOp::SetSpeed(-1.0));
    assert_eq!(r.rejected, 0);
    for _ in 0..59 {
        w.step();
    }
    let p = local(&w, rig).position;
    assert!(p.abs_diff_eq(Vec3::new(3.0, 0.0, -1.0), 1e-2), "{p:?}");
    // Yaw: the turning clip leaves the hips facing forward and turns the
    // rig; the walk then curves with the rig's heading.
    let r = animate(
        &mut w,
        rig,
        AnimateOp::Play {
            clip: clip_ref(2),
            looping: true,
            speed: 1.0,
        },
    );
    assert_eq!(r.rejected, 0);
    let start = local(&w, rig);
    for _ in 0..60 {
        w.step();
    }
    let hips_q = local(&w, hips).rotation;
    assert!(
        hips_q.abs_diff_eq(Quat::IDENTITY, 1e-3) || hips_q.abs_diff_eq(-Quat::IDENTITY, 1e-3),
        "{hips_q:?}"
    );
    let q = local(&w, rig).rotation;
    let (axis, angle) = q.to_axis_angle();
    assert!((angle - std::f32::consts::FRAC_PI_2).abs() < 1e-2, "{q:?}");
    assert!(axis.abs_diff_eq(Vec3::Y, 1e-2), "{axis:?}");
    let s = w.animator_state(rig).unwrap();
    assert!((s.root_yaw - std::f32::consts::FRAC_PI_2 * DT).abs() < 1e-4);
    let moved = local(&w, rig).position - start.position;
    // A quarter turn (towards -x) while walking 2 m traces an arc.
    assert!(
        moved.z < -0.5 && moved.x < -0.5 && moved.y.abs() < 1e-5,
        "{moved:?}"
    );
    // Without the flag the hips carry their own motion again.
    let r = animate(&mut w, rig, AnimateOp::SetTime(0.5));
    assert_eq!(r.rejected, 0);
    set(
        &mut w,
        rig,
        ComponentValue::Animator(Animator {
            clip: clip_ref(1),
            ..Animator::default()
        }),
    );
    w.step();
    assert!((local(&w, hips).position.z + 2.0 * DT).abs() < 1e-4);
}

#[test]
fn root_motion_walks_a_character_controller() {
    let mut w = world();
    w.register_clip(clip_ref(1), walk(false));
    spawn(&mut w, 9, Vec3::new(0.0, -0.5, 0.0), None, "Floor");
    let floor = w
        .ecs()
        .hecs()
        .query::<(&fynkel_ecs::internal::Handle, &Name)>()
        .iter()
        .find(|(_, n)| n.value == "Floor")
        .map(|(h, _)| h.0)
        .unwrap();
    set(
        &mut w,
        floor,
        ComponentValue::Collider(Collider {
            size: Vec3::new(20.0, 0.5, 20.0),
            ..Collider::default()
        }),
    );
    let rig = spawn(&mut w, 1, Vec3::new(0.0, 1.0, 0.0), None, "Rig");
    let hips = spawn(&mut w, 2, Vec3::ZERO, Some(rig), "hips");
    set(
        &mut w,
        rig,
        ComponentValue::CharacterController(CharacterController::default()),
    );
    for _ in 0..30 {
        w.step();
    }
    let landed = local(&w, rig).position;
    set(
        &mut w,
        rig,
        ComponentValue::Animator(Animator {
            clip: clip_ref(1),
            root_motion: true,
            ..Animator::default()
        }),
    );
    for _ in 0..60 {
        w.step();
    }
    // The controller walked the rig (physics owns its transform), the
    // hips stayed put in the pose.
    let p = local(&w, rig).position;
    assert!(
        p.z < -1.5 && p.z >= -2.0 - 1e-3 && p.x.abs() < 1e-4,
        "{p:?} from {landed:?}"
    );
    assert!((p.y - landed.y).abs() < 1e-3, "{p:?} from {landed:?}");
    assert!(local(&w, hips)
        .position
        .abs_diff_eq(Vec3::new(0.0, 1.0, 0.0), 1e-5));
    let state = w.physics().character_state(rig).unwrap();
    assert!(state.grounded);
    assert!(
        (state.velocity.z + 2.0).abs() < 1e-3,
        "{:?}",
        state.velocity
    );
}

/// "surge": `arm` rises 0 → 2 over the first second and holds for the
/// second (2 s); a marker `beat` at 1.5 s.
fn surge() -> Clip {
    Clip {
        name: "surge".into(),
        duration: 2.0,
        markers: vec![Marker {
            time: 1.5,
            name: "beat".into(),
        }],
        channels: vec![Channel {
            target: "arm".into(),
            property: Property::Translation,
            interpolation: Interpolation::Linear,
            times: vec![0.0, 1.0, 2.0],
            values: vec![0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 2.0, 0.0],
        }],
    }
}

fn blend_ref(n: u128) -> AssetRef {
    AssetRef::new(uuid(n))
}

fn space(points: &[(f32, AssetRef)]) -> fynkel_core::BlendSpace {
    fynkel_core::BlendSpace {
        name: "space".into(),
        parameter: "speed".into(),
        points: points
            .iter()
            .map(|(at, clip)| fynkel_core::BlendPoint {
                at: *at,
                clip: clip.clone(),
            })
            .collect(),
        sync: fynkel_core::BlendSync::Phase,
    }
}

#[test]
fn a_blend_space_plays_like_a_clip_phase_synced_by_its_parameter() {
    let mut w = world();
    let mut rise_marked = rise();
    rise_marked.markers.push(Marker {
        time: 0.5,
        name: "half".into(),
    });
    w.register_clip(clip_ref(1), rise_marked);
    w.register_clip(clip_ref(2), surge());
    w.register_blend(
        blend_ref(9),
        space(&[(0.0, clip_ref(1)), (1.0, clip_ref(2))]),
    );
    let root = spawn(&mut w, 1, Vec3::ZERO, None, "Rig");
    let arm = spawn(&mut w, 2, Vec3::ZERO, Some(root), "arm");
    // The parameter unset: the first point alone (rise, 1 s).
    set(&mut w, root, animator(blend_ref(9), true));
    assert!(w.diagnostics().is_empty(), "{:?}", w.diagnostics());
    for _ in 0..30 {
        w.step();
    }
    let s = w.animator_state(root).unwrap();
    assert_eq!(s.clip, blend_ref(9));
    assert!((s.duration - 1.0).abs() < 1e-6, "{s:?}");
    assert!((s.time - 0.5).abs() < 1e-5, "{s:?}");
    assert!((local(&w, arm).position.y - 0.5).abs() < 1e-5);
    let m = markers(&mut w);
    assert_eq!(m.len(), 1, "{m:?}");
    assert_eq!(m[0], (root, clip_ref(1), "half".to_string()));

    // Halfway between the points: the clock keeps its phase over the
    // blended length (1.5 s), each clip is sampled at that phase of its
    // own — rise at 0.5 s, surge at 1 s — and the poses mix 50/50.
    let r = animate(
        &mut w,
        root,
        AnimateOp::SetParameter {
            name: "speed".into(),
            value: 0.5,
        },
    );
    assert_eq!(r.status, Status::Ok);
    let s = w.animator_state(root).unwrap();
    assert!((s.duration - 1.5).abs() < 1e-6, "{s:?}");
    assert!((s.parameter - 0.5).abs() < 1e-6, "{s:?}");
    let phase = s.time / s.duration;
    assert!((phase - (0.5 + DT / 1.5)).abs() < 1e-4, "{s:?}");
    let expect = 0.5 * (phase * 1.0) + 0.5 * (2.0 * (2.0 * phase).min(1.0));
    assert!(
        (local(&w, arm).position.y - expect).abs() < 1e-4,
        "{} vs {expect}",
        local(&w, arm).position.y
    );
    // Time-synced sampling would have read surge at the blend clock's
    // time instead.
    let unsynced = 0.5 * s.time + 0.5 * 2.0 * s.time.min(1.0);
    assert!((local(&w, arm).position.y - unsynced).abs() > 1e-3);

    // Past the last point the parameter clamps: surge alone (2 s), and
    // its marker fires on its own clock — once per lap.
    let r = animate(
        &mut w,
        root,
        AnimateOp::SetParameter {
            name: "speed".into(),
            value: 7.0,
        },
    );
    assert_eq!(r.status, Status::Ok);
    let s = w.animator_state(root).unwrap();
    assert!((s.duration - 2.0).abs() < 1e-6, "{s:?}");
    assert!((s.parameter - 7.0).abs() < 1e-6);
    let _ = markers(&mut w);
    let before = s.time;
    let ticks = ((2.0 - before) / DT).ceil() as usize + 2;
    for _ in 0..ticks {
        w.step();
    }
    let m = markers(&mut w);
    assert_eq!(
        m.iter().filter(|m| m.2 == "beat").count(),
        1,
        "{m:?} from {before}"
    );
    assert!(m.iter().all(|m| m.1 == clip_ref(2)), "{m:?}");

    // A crossfade out of the space fades its dominant clip; a clip put
    // on the animator afterwards plays from its start.
    let r = animate(
        &mut w,
        root,
        AnimateOp::Crossfade {
            clip: clip_ref(1),
            looping: true,
            speed: 1.0,
            seconds: 0.5,
        },
    );
    assert_eq!(r.status, Status::Ok, "{:?}", w.diagnostics());
    let s = w.animator_state(root).unwrap();
    assert_eq!(s.clip, clip_ref(1));
    assert!(s.fade_remaining > 0.0 && s.fade_remaining < 0.5, "{s:?}");
    for _ in 0..40 {
        w.step();
    }
    assert!(w.diagnostics().is_empty(), "{:?}", w.diagnostics());
    let s = w.animator_state(root).unwrap();
    assert_eq!(s.fade_remaining, 0.0);
    assert!((s.duration - 1.0).abs() < 1e-6);
}

#[test]
fn a_blend_space_waits_for_its_clips_and_a_bad_parameter_is_refused() {
    let mut w = world();
    w.register_clip(clip_ref(1), rise());
    w.register_blend(
        blend_ref(9),
        space(&[(0.0, clip_ref(1)), (1.0, clip_ref(2))]),
    );
    let root = spawn(&mut w, 1, Vec3::ZERO, None, "Rig");
    let arm = spawn(&mut w, 2, Vec3::ZERO, Some(root), "arm");
    set(&mut w, root, animator(blend_ref(9), true));
    // The second point's clip is missing: reported once, naming it.
    let reports: Vec<_> = w
        .diagnostics()
        .iter()
        .filter(|d| d.code == DiagnosticCode::CLIP_UNAVAILABLE)
        .cloned()
        .collect();
    assert_eq!(reports.len(), 1, "{:?}", w.diagnostics());
    assert!(reports[0].message.contains(&clip_ref(2).to_string()));
    for _ in 0..5 {
        w.step();
        assert!(w.diagnostics().is_empty());
    }
    assert_eq!(local(&w, arm).position, Vec3::ZERO);
    assert_eq!(w.animator_state(root).unwrap().duration, 0.0);
    w.register_clip(clip_ref(2), surge());
    for _ in 0..6 {
        w.step();
    }
    assert!(w.diagnostics().is_empty());
    assert!(local(&w, arm).position.y > 0.0, "playing now");
    // A parameter that is not finite, or has no name, is refused.
    let r = animate(
        &mut w,
        root,
        AnimateOp::SetParameter {
            name: "speed".into(),
            value: f32::NAN,
        },
    );
    assert_eq!(r.rejected, 1, "{:?}", w.diagnostics());
    assert_eq!(w.diagnostics()[0].code, DiagnosticCode::INVALID_PAYLOAD);
    let r = animate(
        &mut w,
        root,
        AnimateOp::SetParameter {
            name: String::new(),
            value: 1.0,
        },
    );
    assert_eq!(r.rejected, 1);
    // A parameter of another name leaves the space where it is.
    let r = animate(
        &mut w,
        root,
        AnimateOp::SetParameter {
            name: "other".into(),
            value: 1.0,
        },
    );
    assert_eq!(r.rejected, 0);
    let s = w.animator_state(root).unwrap();
    assert!((s.duration - 1.0).abs() < 1e-6, "{s:?}");
    assert_eq!(s.parameter, 0.0, "the space's own parameter is unset");
}

#[test]
fn an_edit_world_poses_its_animators_only_when_asked() {
    // ADR-0030: the Inspector's preview stages seek an animator in an
    // edit-mode world; a plain edit world stays still.
    let mut w = World::new(WorldConfig {
        edit_mode: true,
        ..WorldConfig::default()
    });
    w.register_clip(clip_ref(1), rise());
    let root = spawn(&mut w, 1, Vec3::ZERO, None, "Rig");
    let arm = spawn(&mut w, 2, Vec3::new(0.0, 0.0, 1.0), Some(root), "arm");
    set(&mut w, root, animator(clip_ref(1), true));
    animate(&mut w, root, AnimateOp::SetTime(0.5));
    assert_eq!(local(&w, arm).position, Vec3::new(0.0, 0.0, 1.0));

    assert!(w.set_edit_animation(true));
    animate(&mut w, root, AnimateOp::SetTime(0.25));
    assert!((local(&w, arm).position.y - 0.25).abs() < 1e-5);
    // The propagated world transform follows at once (no interpolation).
    let world_y = w
        .ecs()
        .copy::<fynkel_scene::WorldTransform>(arm)
        .unwrap()
        .matrix
        .w_axis
        .y;
    assert!((world_y - 0.25).abs() < 1e-5, "{world_y}");

    // A game world ignores the switch.
    assert!(!world().set_edit_animation(true));
}

fn transport_fixture() -> (World, Entity, Entity) {
    let mut w = world();
    w.register_clip(clip_ref(1), rise());
    w.register_clip(clip_ref(2), turn());
    let root = spawn(&mut w, 1, Vec3::ZERO, None, "animator");
    let arm = spawn(&mut w, 2, Vec3::ZERO, Some(root), "arm");
    set(
        &mut w,
        root,
        ComponentValue::Animator(Animator {
            clip: clip_ref(1),
            playing: true,
            ..Animator::default()
        }),
    );
    for _ in 0..10 {
        w.step();
    }
    (w, root, arm)
}

#[test]
fn transactional_crossfade_matches_legacy_outgoing_pose_and_runs_once() {
    let (mut w, e, arm) = transport_fixture();
    let (mut legacy, le, la) = transport_fixture();
    let op = AnimateOp::Crossfade {
        clip: clip_ref(2),
        looping: true,
        speed: 1.0,
        seconds: 0.5,
    };
    let before = local(&w, arm);
    let time = w.animator_state(e).unwrap().time;
    w.edit_gameplay(|edit| edit.animate_control(e, op.clone()))
        .unwrap();
    assert_eq!(w.animator_state(e).unwrap().time, time);
    assert_eq!(local(&w, arm), before);
    animate(&mut legacy, le, op);
    w.step();
    assert_eq!(local(&w, arm), local(&legacy, la));
    assert!(w.animator_state(e).unwrap().fade_remaining > 0.0);
    for _ in 0..5 {
        w.step();
        legacy.step();
    }
    assert_eq!(local(&w, arm), local(&legacy, la));
    assert_eq!(
        w.animator_state(e).unwrap().fade_remaining,
        legacy.animator_state(le).unwrap().fade_remaining
    );
}

#[test]
fn animation_transport_keeps_later_configuration_and_orders_seek_stop_play() {
    let (mut w, e, _) = transport_fixture();
    w.edit_gameplay(|edit| {
        edit.animate_control(
            e,
            AnimateOp::Play {
                clip: clip_ref(2),
                looping: true,
                speed: 1.0,
            },
        )?;
        edit.animate_control(e, AnimateOp::SetTime(0.4))?;
        edit.animate_control(
            e,
            AnimateOp::SetParameter {
                name: "move".into(),
                value: 0.75,
            },
        )
    })
    .unwrap();
    w.edit_gameplay(|edit| {
        edit.set_component(
            e,
            ComponentValue::Animator(Animator {
                clip: clip_ref(1),
                playing: false,
                speed: 3.0,
                weight: 0.25,
                ..Animator::default()
            }),
        )
    })
    .unwrap();
    w.step();
    let s = w.animator_state(e).unwrap();
    assert_eq!(s.clip, clip_ref(1));
    assert!(!s.playing);
    assert_eq!(s.speed, 3.0);
    assert_eq!(s.weight, 0.25);
    assert_eq!(s.parameter, 0.75);
    assert!((s.time - 0.4).abs() < 1e-5);
    w.edit_gameplay(|edit| {
        edit.animate_control(e, AnimateOp::SetTime(0.8))?;
        edit.animate_control(e, AnimateOp::Stop)
    })
    .unwrap();
    w.step();
    assert_eq!(w.animator_state(e).unwrap().time, 0.0);
}

#[test]
fn animation_abort_invalid_fade_and_final_removal_preserve_runtime_and_data() {
    let (mut w, e, arm) = transport_fixture();
    let before = w.ecs().copy::<Animator>(e).unwrap();
    let pose = local(&w, arm);
    let time = w.animator_state(e).unwrap().time;
    for variant in 0..4 {
        assert!(w
            .edit_gameplay(|edit| {
                edit.animate_control(
                    e,
                    AnimateOp::Play {
                        clip: clip_ref(2),
                        looping: false,
                        speed: 2.0,
                    },
                )?;
                match variant {
                    0 => return Err(EditError::Aborted),
                    1 => edit.remove_component(e, ComponentId::ANIMATOR)?,
                    2 => {
                        let _ = edit.animate_control(
                            e,
                            AnimateOp::Crossfade {
                                clip: clip_ref(1),
                                looping: true,
                                speed: 1.0,
                                seconds: -1.0,
                            },
                        );
                    }
                    _ => edit.set_component(
                        e,
                        ComponentValue::Animator(Animator {
                            controller: Some(clip_ref(9)),
                            ..Animator::default()
                        }),
                    )?,
                }
                Ok(())
            })
            .is_err());
        assert_eq!(w.ecs().copy::<Animator>(e).unwrap(), before);
        assert_eq!(w.animator_state(e).unwrap().time, time);
        assert_eq!(local(&w, arm), pose);
    }
    w.step();
    assert!(w.animator_state(e).unwrap().time > time);
    assert_eq!(w.animator_state(e).unwrap().clip, clip_ref(1));
}

#[test]
fn animation_destruction_cancels_and_later_removal_rejects_once() {
    let (mut w, e, _) = transport_fixture();
    w.edit_gameplay(|edit| {
        edit.animate_control(e, AnimateOp::Stop)?;
        edit.destroy(e)
    })
    .unwrap();
    w.step();
    assert!(w.diagnostics().is_empty());
    let (mut w, e, _) = transport_fixture();
    w.edit_gameplay(|edit| edit.animate_control(e, AnimateOp::SetTime(0.5)))
        .unwrap();
    w.edit_gameplay(|edit| edit.remove_component(e, ComponentId::ANIMATOR))
        .unwrap();
    w.step();
    assert_eq!(w.diagnostics().len(), 1);
    assert_eq!(w.diagnostics()[0].code, DiagnosticCode::MISSING_COMPONENT);
    w.step();
    assert!(w.diagnostics().is_empty());
}

#[test]
fn invalidated_animation_intent_is_cancelled_if_its_entity_is_destroyed() {
    let (mut w, e, _) = transport_fixture();
    w.edit_gameplay(|edit| edit.animate_control(e, AnimateOp::SetTime(0.5)))
        .unwrap();
    w.edit_gameplay(|edit| edit.remove_component(e, ComponentId::ANIMATOR))
        .unwrap();
    w.edit_gameplay(|edit| edit.destroy(e)).unwrap();
    w.step();
    assert!(w.diagnostics().is_empty());
}

#[test]
fn invalid_derived_pose_rolls_back_every_target_and_animator_completion() {
    let mut w = world();
    let root = spawn(&mut w, 100, Vec3::ZERO, None, "Rig");
    let good = spawn(&mut w, 101, Vec3::ZERO, Some(root), "good");
    let huge = spawn(
        &mut w,
        102,
        Vec3::new(f32::MAX, 0.0, 0.0),
        Some(root),
        "huge",
    );
    let clip = Clip {
        name: "overflow additive output".into(),
        duration: DT,
        markers: Vec::new(),
        channels: [("good", 1.0), ("huge", f32::MAX)]
            .into_iter()
            .map(|(target, x)| Channel {
                target: target.into(),
                property: Property::Translation,
                interpolation: Interpolation::Linear,
                times: vec![0.0, DT],
                values: vec![0.0, 0.0, 0.0, x, 0.0, 0.0],
            })
            .collect(),
    };
    w.register_clip(clip_ref(90), clip);
    set(
        &mut w,
        root,
        ComponentValue::Animator(Animator {
            clip: clip_ref(90),
            additive: true,
            looping: false,
            ..Animator::default()
        }),
    );
    let before_good = local(&w, good);
    let before_huge = local(&w, huge);
    w.step();
    assert_eq!(
        local(&w, good),
        before_good,
        "valid sibling output must not publish alone"
    );
    assert_eq!(local(&w, huge), before_huge);
    assert!(
        w.ecs().get::<Animator>(root).unwrap().playing,
        "completion configuration belongs to the same rejected pose batch"
    );
    assert!(
        w.animator_state(root).unwrap().time >= DT,
        "runtime clock remains the animation system's responsibility"
    );
    assert!(w
        .diagnostics()
        .iter()
        .any(|d| d.code == DiagnosticCode::INVALID_PAYLOAD
            && d.entity == root
            && d.message.contains("animation pose")));
    assert!(
        w.drain_events().iter().any(|event| matches!(
            &event.payload, EventPayload::AnimationEnded { entity, clip }
                if *entity == root && *clip == clip_ref(90)
        )),
        "playback events report runtime completion, not acceptance of the authored pose"
    );
}
