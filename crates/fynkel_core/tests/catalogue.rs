//! The native component catalogue (blueprint §3.5, ADR-0011, §19.2d item
//! 10): `Rotator`, `Oscillate`, `LifetimeDespawn` and `Emitter` in the
//! pre-tick slot, `Follow` and `LookAtCamera` after physics and the rigs;
//! every pass counted in `system_times`.

use std::f32::consts::TAU;

use fynkel_contracts::{AssetRef, DiagnosticCode, ObjectRef, Uuid, DEFAULT_FIXED_DT};
use fynkel_core::components::ComponentValue;
use fynkel_core::load::{EntityDescription, MissingReference, SceneDescription};
use fynkel_core::schema::SystemKind;
use fynkel_core::*;
use fynkel_scene::WorldTransform;
use glam::{Quat, Vec3};

const DT: f32 = DEFAULT_FIXED_DT as f32;

fn world() -> World {
    World::new(WorldConfig::default())
}

fn spawn(
    w: &mut World,
    _fixture_id: u64,
    at: Vec3,
    parent: Option<Entity>,
    name: Option<&str>,
    components: Vec<ComponentValue>,
) -> Entity {
    let commit = w
        .edit_gameplay(|edit| {
            edit.spawn(
                name.unwrap_or_default(),
                Transform::from_position(at),
                parent.map(Into::into),
            )
        })
        .unwrap();
    let entity = commit.resolve(commit.value).unwrap();
    assert_eq!(w.step().status, Status::Ok);
    w.drain_events();
    if !components.is_empty() {
        w.edit_gameplay(|edit| {
            for value in components {
                edit.set_component(entity, value)?;
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(w.step().status, Status::Ok);
    }
    entity
}

fn local(w: &World, e: Entity) -> Transform {
    w.ecs().copy::<Transform>(e).unwrap()
}

fn world_pose(w: &World, e: Entity) -> (Vec3, Quat) {
    let (_, r, t) = w
        .ecs()
        .copy::<WorldTransform>(e)
        .unwrap()
        .matrix
        .to_scale_rotation_translation();
    (t, r)
}

fn close(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-4
}

fn same_rotation(a: Quat, b: Quat) -> bool {
    a.dot(b).abs() > 0.9999
}

fn time_of(w: &World, kind: SystemKind) -> (u32, u32) {
    let t = w.system_times().iter().find(|t| t.system == kind).unwrap();
    (t.entities, t.micros)
}

#[test]
fn a_rotator_turns_locally_or_about_a_world_axis() {
    let mut w = world();
    let spinner = spawn(
        &mut w,
        1,
        Vec3::ZERO,
        None,
        None,
        vec![ComponentValue::Rotator(Rotator {
            axis: Vec3::Y,
            speed: 2.0,
            space: MotionSpace::Local,
        })],
    );
    // The component applied at the tick above and turned once already.
    for _ in 0..9 {
        w.step();
    }
    let expected = Quat::from_axis_angle(Vec3::Y, 2.0 * DT * 10.0);
    assert!(same_rotation(local(&w, spinner).rotation, expected));
    assert_eq!(time_of(&w, SystemKind::Rotator).0, 1);

    // A world-axis rotator under a tilted parent still turns about world Y.
    let parent = spawn(&mut w, 2, Vec3::new(3.0, 0.0, 0.0), None, None, vec![]);
    let tilt = Quat::from_axis_angle(Vec3::X, 0.7);
    w.edit_gameplay(|edit| {
        edit.set_transform(
            parent,
            Transform {
                position: Vec3::new(3.0, 0.0, 0.0),
                rotation: tilt,
                scale: Vec3::ONE,
            },
        )?;
        Ok(())
    })
    .unwrap();
    w.step();
    let child = spawn(
        &mut w,
        3,
        Vec3::ZERO,
        Some(parent),
        None,
        vec![ComponentValue::Rotator(Rotator {
            axis: Vec3::Y,
            speed: 1.0,
            space: MotionSpace::World,
        })],
    );
    for _ in 0..5 {
        w.step();
    }
    let (_, rotation) = world_pose(&w, child);
    let expected = Quat::from_axis_angle(Vec3::Y, DT * 6.0) * tilt;
    assert!(
        same_rotation(rotation, expected),
        "{rotation:?} vs {expected:?}"
    );
    // The subtree was propagated: the child's world matrix is not stale.
    assert!(!w.ecs().copy::<WorldTransform>(child).unwrap().dirty);
}

#[test]
fn an_oscillator_bobs_and_keeps_a_moved_rest_position() {
    let mut w = world();
    let bob = spawn(
        &mut w,
        1,
        Vec3::new(1.0, 2.0, 3.0),
        None,
        None,
        vec![ComponentValue::Oscillate(Oscillate {
            axis: Vec3::Y,
            amplitude: 0.5,
            frequency: 1.0,
            phase: 0.0,
            space: MotionSpace::World,
        })],
    );
    // One tick has run since the component appeared.
    let expect_y = |ticks: u32, rest: f32| rest + 0.5 * (TAU * DT * ticks as f32).sin();
    assert!((local(&w, bob).position.y - expect_y(1, 2.0)).abs() < 1e-4);
    for _ in 0..14 {
        w.step();
    }
    assert!((local(&w, bob).position.y - expect_y(15, 2.0)).abs() < 1e-4);
    assert!(
        (local(&w, bob).position.y - 2.5).abs() < 1e-4,
        "peak at a quarter period"
    );
    assert_eq!(local(&w, bob).position.x, 1.0);

    // Gameplay moves the entity: the motion continues about the new rest.
    let mut moved = local(&w, bob);
    moved.position += Vec3::new(0.0, 10.0, 0.0);
    w.edit_gameplay(|edit| {
        edit.set_transform(bob, moved)?;
        Ok(())
    })
    .unwrap();
    w.step();
    assert!((local(&w, bob).position.y - expect_y(16, 12.0)).abs() < 1e-4);
    for _ in 0..44 {
        w.step();
    }
    assert!((local(&w, bob).position.y - expect_y(60, 12.0)).abs() < 1e-3);
    assert!(
        (local(&w, bob).position.y - 12.0).abs() < 1e-3,
        "back at rest after a period"
    );

    // Local space: the axis turns with the entity.
    let side = spawn(
        &mut w,
        2,
        Vec3::ZERO,
        None,
        None,
        vec![
            ComponentValue::Transform(Transform {
                position: Vec3::ZERO,
                rotation: Quat::from_axis_angle(Vec3::Y, std::f32::consts::FRAC_PI_2),
                scale: Vec3::ONE,
            }),
            ComponentValue::Oscillate(Oscillate {
                axis: Vec3::X,
                amplitude: 1.0,
                frequency: 0.25,
                phase: 0.0,
                space: MotionSpace::Local,
            }),
        ],
    );
    for _ in 0..59 {
        w.step();
    }
    // A quarter period at 0.25 Hz is one second: the local +X is world −Z.
    let p = local(&w, side).position;
    assert!(close(p, Vec3::new(0.0, 0.0, -1.0)), "{p:?}");
    assert_eq!(time_of(&w, SystemKind::Oscillate).0, 2);
}

#[test]
fn a_follower_snaps_smooths_and_looks_at_its_target() {
    let mut w = world();
    let target = spawn(
        &mut w,
        1,
        Vec3::new(5.0, 0.0, 0.0),
        None,
        Some("Hero"),
        vec![],
    );
    let snap = spawn(
        &mut w,
        2,
        Vec3::ZERO,
        None,
        None,
        vec![ComponentValue::Follow(Follow {
            target: ObjectRef::to(target),
            offset: Vec3::new(0.0, 2.0, 0.0),
            smoothing: 0.0,
            look_at: false,
        })],
    );
    assert!(close(world_pose(&w, snap).0, Vec3::new(5.0, 2.0, 0.0)));
    // The target moves this tick; the follower sees the propagated pose.
    w.edit_gameplay(|edit| {
        edit.set_transform(target, Transform::from_position(Vec3::new(5.0, 0.0, 4.0)))?;
        Ok(())
    })
    .unwrap();
    w.step();
    assert!(close(world_pose(&w, snap).0, Vec3::new(5.0, 2.0, 4.0)));

    let smooth = spawn(
        &mut w,
        3,
        Vec3::ZERO,
        None,
        None,
        vec![ComponentValue::Follow(Follow {
            target: ObjectRef::to(target),
            offset: Vec3::ZERO,
            smoothing: 6.0,
            look_at: true,
        })],
    );
    let first = world_pose(&w, smooth).0;
    let expected = Vec3::new(5.0, 0.0, 4.0) * (1.0 - (-6.0 * DT).exp());
    assert!(close(first, expected), "{first:?} vs {expected:?}");
    for _ in 0..600 {
        w.step();
    }
    let (p, r) = world_pose(&w, smooth);
    assert!((p - Vec3::new(5.0, 0.0, 4.0)).length() < 1e-2, "{p:?}");
    // Its −Z faces the target.
    let forward = r * Vec3::NEG_Z;
    let dir = (Vec3::new(5.0, 0.0, 4.0) - first).normalize();
    assert!(forward.dot(dir) > 0.99, "forward {forward:?} dir {dir:?}");

    // No target leaves the follower where it is.
    let lost = spawn(
        &mut w,
        4,
        Vec3::new(-1.0, -1.0, -1.0),
        None,
        None,
        vec![ComponentValue::Follow(Follow {
            target: ObjectRef::NONE,
            ..Follow::default()
        })],
    );
    assert!(close(world_pose(&w, lost).0, Vec3::new(-1.0, -1.0, -1.0)));
    assert_eq!(time_of(&w, SystemKind::Follow).0, 3);
}

#[test]
fn a_billboard_faces_the_active_camera() {
    let mut w = world();
    let sign = spawn(
        &mut w,
        1,
        Vec3::ZERO,
        None,
        None,
        vec![ComponentValue::LookAtCamera(LookAtCamera { lock_y: true })],
    );
    // No camera: untouched.
    assert!(same_rotation(local(&w, sign).rotation, Quat::IDENTITY));
    let low = spawn(
        &mut w,
        2,
        Vec3::new(10.0, 5.0, 0.0),
        None,
        None,
        vec![ComponentValue::Camera(Camera {
            priority: 0,
            ..Camera::default()
        })],
    );
    let facing = local(&w, sign).rotation * Vec3::Z;
    assert!(close(facing, Vec3::X), "yaw only: {facing:?}");
    // A higher-priority camera wins; without the lock the sign tilts up.
    spawn(
        &mut w,
        3,
        Vec3::new(0.0, 5.0, 10.0),
        None,
        None,
        vec![ComponentValue::Camera(Camera {
            priority: 5,
            ..Camera::default()
        })],
    );
    let facing = local(&w, sign).rotation * Vec3::Z;
    assert!(close(facing, Vec3::Z), "{facing:?}");
    w.edit_gameplay(|edit| {
        edit.set_component(
            sign,
            ComponentValue::LookAtCamera(LookAtCamera { lock_y: false }),
        )?;
        Ok(())
    })
    .unwrap();
    w.step();
    let facing = local(&w, sign).rotation * Vec3::Z;
    assert!(
        close(facing, Vec3::new(0.0, 5.0, 10.0).normalize()),
        "{facing:?}"
    );
    let _ = low;
    assert_eq!(time_of(&w, SystemKind::LookAtCamera).0, 1);
}

#[test]
fn a_lifetime_despawns_the_subtree_with_events() {
    let mut w = world();
    let root = spawn(
        &mut w,
        1,
        Vec3::ZERO,
        None,
        Some("Puff"),
        vec![ComponentValue::LifetimeDespawn(LifetimeDespawn {
            seconds: DT * 3.5,
        })],
    );
    let child = spawn(&mut w, 2, Vec3::ZERO, Some(root), None, vec![]);
    w.drain_events();
    // Ticks so far since the component applied: 1 (its own) + 1 (child).
    w.step();
    assert!(w.ecs().contains(root));
    w.step();
    assert!(!w.ecs().contains(root), "expired at the fourth tick");
    assert!(!w.ecs().contains(child));
    let destroyed: Vec<Entity> = w
        .drain_events()
        .into_iter()
        .filter_map(|e| match e.payload {
            EventPayload::Destroyed { entity } => Some(entity),
            _ => None,
        })
        .collect();
    assert_eq!(destroyed, vec![root, child]);
    assert_eq!(w.system_times().len(), 15);
}

fn prefab(id: Uuid, dynamic: bool) -> SceneDescription {
    let mut components = vec![ComponentValue::Transform(Transform::from_position(
        Vec3::new(0.0, 0.5, 0.0),
    ))];
    if dynamic {
        components.push(ComponentValue::RigidBody(RigidBody::default()));
        components.push(ComponentValue::Collider(Collider::default()));
    }
    SceneDescription {
        id,
        name: "Spark".into(),
        entities: vec![EntityDescription {
            registered: Vec::new(),
            id: Uuid::from_bytes(0x77u128.to_be_bytes()),
            name: "Ember".into(),
            active: true,
            parent: None,
            children: vec![],
            components,
        }],
    }
}

fn named(w: &World, name: &str) -> Vec<Entity> {
    let mut found: Vec<Entity> = w
        .ecs()
        .hecs()
        .query::<(&fynkel_ecs::internal::Handle, &Name)>()
        .iter()
        .filter(|(_, n)| n.value == name)
        .map(|(h, _)| h.0)
        .collect();
    found.sort();
    found
}

#[test]
fn an_emitter_spawns_its_prefab_at_rate_with_lifetime_ceiling_and_velocity() {
    let mut w = world();
    let prefab_id = Uuid::from_bytes(0x5000u128.to_be_bytes());
    w.register_prefab(prefab_id, prefab(prefab_id, true));
    assert!(w.has_prefab(prefab_id));
    let fountain = spawn(
        &mut w,
        1,
        Vec3::new(0.0, 4.0, 0.0),
        None,
        None,
        vec![ComponentValue::Emitter(Emitter {
            prefab: AssetRef::new(prefab_id),
            rate: 30.0,
            lifetime: DT * 10.0,
            max_alive: 4,
            velocity: Vec3::new(3.0, 0.0, 0.0),
            enabled: true,
        })],
    );
    // 30/s at 60 Hz: one instance every second tick (the fraction carries).
    for _ in 0..3 {
        w.step();
    }
    // Each instance is the prefab's root, named after the prefab, placed
    // at the emitter's world pose composed with the root's own Transform.
    let sparks = named(&w, "Spark");
    assert_eq!(sparks.len(), 2, "after 4 ticks");
    assert!(named(&w, "Ember").is_empty());
    assert!(w.ecs().has::<LifetimeDespawn>(sparks[0]));
    assert!(
        fynkel_scene::parent_of(w.ecs(), sparks[0]).is_none(),
        "unparented"
    );
    assert!(fynkel_scene::children_of(w.ecs(), sparks[0]).is_empty());
    let (p, _) = world_pose(&w, sparks[0]);
    assert!(p.y > 4.3 && p.y <= 4.5, "fell a little from 4.5: {p:?}");
    // Launched at 3 m/s (+X); the overlapping instances nudge each other.
    let state = w.physics().body_state(sparks[0]).unwrap();
    assert!((state.linear_velocity.x - 3.0).abs() < 0.5, "{state:?}");

    // The ceiling holds, then the lifetime frees a slot.
    for _ in 0..8 {
        w.step();
    }
    assert_eq!(named(&w, "Spark").len(), 4, "capped at max_alive");
    for _ in 0..40 {
        w.step();
    }
    let live = named(&w, "Spark").len();
    assert!(live == 4 || live == 3, "steady state: {live}");
    assert!(!w.ecs().contains(sparks[0]), "the first one expired");
    let (entities, _) = time_of(&w, SystemKind::Emitter);
    assert_eq!(entities, 1);

    // Disabled: nothing more, the survivors expire.
    w.edit_gameplay(|edit| {
        edit.set_component(
            fountain,
            ComponentValue::Emitter(Emitter {
                prefab: AssetRef::new(prefab_id),
                rate: 30.0,
                lifetime: DT * 10.0,
                max_alive: 4,
                velocity: Vec3::ZERO,
                enabled: false,
            }),
        )?;
        Ok(())
    })
    .unwrap();
    for _ in 0..12 {
        w.step();
    }
    assert!(named(&w, "Spark").is_empty());
    assert!(w.diagnostics().is_empty(), "{:?}", w.diagnostics());
}

#[test]
fn an_unregistered_prefab_is_reported_once_per_emitter() {
    let mut w = world();
    let missing = Uuid::from_bytes(0x6000u128.to_be_bytes());
    let e = spawn(
        &mut w,
        1,
        Vec3::ZERO,
        None,
        None,
        vec![ComponentValue::Emitter(Emitter {
            prefab: AssetRef::new(missing),
            rate: 120.0,
            ..Emitter::default()
        })],
    );
    // The tick that applied the component emitted (and reported) once;
    // later ticks stay quiet.
    let reports: Vec<Diagnostic> = w
        .diagnostics()
        .iter()
        .filter(|d| d.code == DiagnosticCode::PREFAB_UNAVAILABLE)
        .cloned()
        .collect();
    assert_eq!(reports.len(), 1, "{:?}", w.diagnostics());
    assert_eq!(reports[0].entity, e);
    assert_eq!(reports[0].component, fynkel_contracts::ComponentId::EMITTER);
    for _ in 0..5 {
        w.step();
        assert!(w.diagnostics().is_empty(), "{:?}", w.diagnostics());
    }
    assert!(named(&w, "Spark").is_empty());
    // Registering it later lets the emitter run.
    w.register_prefab(missing, prefab(missing, false));
    w.step();
    assert_eq!(named(&w, "Spark").len(), 2);
}

#[test]
fn system_times_cover_every_slot_in_order() {
    let mut w = world();
    spawn(&mut w, 1, Vec3::ZERO, None, None, vec![]);
    let kinds: Vec<SystemKind> = w.system_times().iter().map(|t| t.system).collect();
    assert_eq!(
        kinds,
        vec![
            SystemKind::Step,
            SystemKind::Commands,
            SystemKind::Characters,
            SystemKind::Animation,
            SystemKind::Rotator,
            SystemKind::Oscillate,
            SystemKind::LifetimeDespawn,
            SystemKind::Emitter,
            SystemKind::Physics,
            SystemKind::CameraRigs,
            SystemKind::Follow,
            SystemKind::LookAtCamera,
            SystemKind::Transforms,
            SystemKind::Audio,
            SystemKind::Publish,
        ]
    );
    let step = time_of(&w, SystemKind::Step).1;
    let sum: u32 = w
        .system_times()
        .iter()
        .filter(|t| t.system != SystemKind::Step)
        .map(|t| t.micros)
        .sum();
    assert!(step >= sum.saturating_sub(1), "step {step} >= parts {sum}");
    assert_eq!(
        time_of(&w, SystemKind::Commands).0,
        0,
        "data was committed before the tick; no effects to deliver"
    );
}

fn doc_id(n: u8) -> Uuid {
    let mut b = [0u8; 16];
    b[15] = n;
    b[6] = 0x40;
    b[8] = 0x80;
    Uuid(b)
}

#[test]
fn each_instance_follows_its_own_head() {
    // Foundation 3.5, ADR-0035: a prefab's reference to one of its own
    // objects points at each instance's copy.
    let prefab = SceneDescription {
        id: doc_id(100),
        name: "Enemy".into(),
        entities: vec![
            EntityDescription {
                registered: Vec::new(),
                id: doc_id(1),
                name: "Enemy".into(),
                active: true,
                parent: None,
                children: vec![doc_id(2), doc_id(3)],
                components: vec![ComponentValue::Transform(Transform::IDENTITY)],
            },
            EntityDescription {
                registered: Vec::new(),
                id: doc_id(2),
                name: "Head".into(),
                active: true,
                parent: Some(doc_id(1)),
                children: vec![],
                components: vec![ComponentValue::Transform(Transform::from_position(
                    Vec3::new(0.0, 2.0, 0.0),
                ))],
            },
            EntityDescription {
                registered: Vec::new(),
                id: doc_id(3),
                name: "Marker".into(),
                active: true,
                parent: Some(doc_id(1)),
                children: vec![],
                components: vec![
                    ComponentValue::Transform(Transform::IDENTITY),
                    ComponentValue::Follow(Follow {
                        target: ObjectRef::authored(doc_id(2)),
                        offset: Vec3::new(0.0, 1.0, 0.0),
                        smoothing: 0.0,
                        look_at: false,
                    }),
                ],
            },
        ],
    };
    let mut w = world();
    let mut markers = Vec::new();
    for x in [0.0, 10.0] {
        let (_, loaded) = w
            .instantiate(
                &prefab,
                "Enemy",
                Transform::from_position(Vec3::new(x, 0.0, 0.0)),
                None,
            )
            .unwrap();
        markers.push(loaded.handle_of(doc_id(3)).unwrap());
    }
    w.step();
    assert!(close(
        world_pose(&w, markers[0]).0,
        Vec3::new(0.0, 3.0, 0.0)
    ));
    assert!(
        close(world_pose(&w, markers[1]).0, Vec3::new(10.0, 3.0, 0.0)),
        "the second marker follows its own head: {:?}",
        world_pose(&w, markers[1]).0
    );
}

#[test]
fn a_reference_to_an_object_outside_the_load_is_none_and_reported() {
    // ADR-0035 §3: nothing searches the world for it.
    let prefab = SceneDescription {
        id: doc_id(100),
        name: "Marker".into(),
        entities: vec![EntityDescription {
            registered: Vec::new(),
            id: doc_id(1),
            name: "Marker".into(),
            active: true,
            parent: None,
            children: vec![],
            components: vec![
                ComponentValue::Transform(Transform::IDENTITY),
                ComponentValue::Follow(Follow {
                    target: ObjectRef::authored(doc_id(9)),
                    offset: Vec3::ZERO,
                    smoothing: 0.0,
                    look_at: false,
                }),
            ],
        }],
    };
    let mut w = world();
    let (_, loaded) = w
        .instantiate(&prefab, "Marker", Transform::IDENTITY, None)
        .unwrap();
    assert_eq!(
        loaded.missing_references,
        [MissingReference {
            entity: doc_id(1),
            component: "fynkel.Follow",
            field: "target",
            target: doc_id(9),
        }]
    );
    let marker = loaded.handle_of(doc_id(1)).unwrap();
    let follow = w.ecs().copy::<Follow>(marker).unwrap().target;
    assert!(follow.is_none(), "{follow:?}");
}
