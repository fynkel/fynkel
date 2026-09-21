//! `WorldConfig::edit_mode` (ADR-0016, blueprint §19.2e item 6): the
//! editor's edit-preview world. A step applies commands, propagates
//! transforms and publishes the snapshot; no system runs, the picture is
//! never interpolated, and `extract()` honours the editor camera and the
//! hidden set.

use fynkel_contracts::DEFAULT_FIXED_DT;
use fynkel_core::components::{
    Camera, ComponentValue, MeshRenderer, PointLight, Projection, Rotator,
};
use fynkel_core::schema::SystemKind;
use fynkel_core::*;
use fynkel_physics::{Collider, RigidBody};
use fynkel_scene::WorldTransform;
use glam::{Mat4, Quat, Vec3};

const DT: f32 = DEFAULT_FIXED_DT as f32;

fn edit_world() -> World {
    World::new(WorldConfig {
        edit_mode: true,
        ..WorldConfig::default()
    })
}

fn spawn(
    w: &mut World,
    temp_id: u64,
    at: Vec3,
    parent: Option<Entity>,
    components: Vec<ComponentValue>,
) -> Entity {
    w.submit(
        Lane::Simulation,
        vec![Command::Spawn {
            temp_id,
            transform: Transform::from_position(at),
            parent,
            name: Some(format!("e{temp_id}")),
        }],
    );
    let r = w.step();
    assert_eq!(r.status, Status::Ok, "{:?}", w.diagnostics());
    let entity = w
        .drain_events()
        .into_iter()
        .find_map(|e| match e.payload {
            EventPayload::Spawned { temp_id: t, entity } if t == temp_id => Some(entity),
            _ => None,
        })
        .unwrap();
    if !components.is_empty() {
        let cmds = components
            .into_iter()
            .map(|value| Command::SetComponent { entity, value })
            .collect();
        w.submit(Lane::Simulation, cmds);
        let r = w.step();
        assert_eq!(r.status, Status::Ok, "{:?}", w.diagnostics());
    }
    entity
}

fn local(w: &World, e: Entity) -> Transform {
    w.ecs().copy::<Transform>(e).unwrap()
}

fn world_pos(w: &World, e: Entity) -> Vec3 {
    w.ecs()
        .copy::<WorldTransform>(e)
        .unwrap()
        .matrix
        .transform_point3(Vec3::ZERO)
}

#[test]
fn a_game_world_ignores_the_edit_calls() {
    let mut w = World::new(WorldConfig::default());
    assert!(!w.is_edit_mode());
    assert!(!w.set_edit_camera(Some((Mat4::IDENTITY, Camera::default()))));
    assert!(w.edit_camera().is_none());
    assert!(!w.set_edit_hidden(&[Entity::NULL]));
    assert_eq!(w.edit_hidden().count(), 0);
}

#[test]
fn nothing_ticks_in_edit_mode_for_a_minute() {
    let mut w = edit_world();
    let spinner = spawn(
        &mut w,
        1,
        Vec3::ZERO,
        None,
        vec![ComponentValue::Rotator(Rotator {
            axis: Vec3::Y,
            speed: 2.0,
            ..Rotator::default()
        })],
    );
    let _floor = spawn(
        &mut w,
        2,
        Vec3::new(0.0, -0.5, 0.0),
        None,
        vec![ComponentValue::Collider(Collider {
            size: Vec3::new(20.0, 0.5, 20.0),
            ..Collider::default()
        })],
    );
    let body = spawn(
        &mut w,
        3,
        Vec3::new(0.0, 3.0, 0.0),
        None,
        vec![
            ComponentValue::RigidBody(RigidBody::default()),
            ComponentValue::Collider(Collider::default()),
        ],
    );
    let before_spin = local(&w, spinner);
    let before_fall = local(&w, body);
    // 60 s of frames, every step taken.
    let steps = (60.0 / DT).round() as u32;
    for _ in 0..steps {
        w.step();
    }
    assert_eq!(local(&w, spinner), before_spin, "the rotator never turned");
    assert_eq!(local(&w, body), before_fall, "the body never fell");
    for kind in [
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
        SystemKind::Audio,
    ] {
        let t = w.system_times().iter().find(|t| t.system == kind).unwrap();
        assert_eq!(t.entities, 0, "{kind:?} ran");
    }
    let commands = w
        .system_times()
        .iter()
        .find(|t| t.system == SystemKind::Commands)
        .unwrap();
    assert_eq!(commands.entities, 0, "no command this tick");
}

#[test]
fn commands_propagation_and_snapshot_still_happen_and_never_interpolate() {
    let mut w = edit_world();
    let parent = spawn(&mut w, 1, Vec3::new(1.0, 0.0, 0.0), None, vec![]);
    let child = spawn(&mut w, 2, Vec3::new(0.0, 1.0, 0.0), Some(parent), vec![]);
    w.subscribe(&[child], true);
    // Move the parent: the child's world position follows in the same step.
    w.submit(
        Lane::Simulation,
        vec![Command::SetTransform {
            entity: parent,
            transform: Transform::from_position(Vec3::new(5.0, 0.0, 0.0)),
        }],
    );
    // A frame with dt 0 between the submit and the step: alpha stays 0 —
    // a game world would draw the previous transform; the edit world the
    // current one.
    w.begin_frame(0.0);
    let r = w.step();
    assert_eq!(r.status, Status::Ok);
    assert_eq!(world_pos(&w, child), Vec3::new(5.0, 1.0, 0.0));
    let snap = w.snapshot().iter().find(|s| s.entity == child).unwrap();
    assert_eq!(snap.world.position, Vec3::new(5.0, 1.0, 0.0));
    let extract = w.extract();
    let rec = extract.records.iter().find(|r| r.entity == child).unwrap();
    assert_eq!(
        rec.world.transform_point3(Vec3::ZERO),
        Vec3::new(5.0, 1.0, 0.0)
    );
    assert_eq!(extract.alpha, 0.0);

    // Structure too: reparent and destroy apply at the step.
    w.submit(
        Lane::Simulation,
        vec![
            Command::SetParent {
                child,
                parent: None,
                keep_world: true,
            },
            Command::Destroy(parent),
        ],
    );
    w.step();
    assert!(!w.ecs().contains(parent));
    assert_eq!(world_pos(&w, child), Vec3::new(5.0, 1.0, 0.0));
}

#[test]
fn the_editor_camera_replaces_the_scene_camera_and_hidden_entities_vanish() {
    let mut w = edit_world();
    let cam = spawn(
        &mut w,
        1,
        Vec3::new(0.0, 2.0, 8.0),
        None,
        vec![ComponentValue::Camera(Camera {
            priority: 10,
            ..Camera::default()
        })],
    );
    let mesh = spawn(
        &mut w,
        2,
        Vec3::ZERO,
        None,
        vec![ComponentValue::MeshRenderer(MeshRenderer::default())],
    );
    let light = spawn(
        &mut w,
        3,
        Vec3::new(0.0, 3.0, 0.0),
        Some(mesh),
        vec![ComponentValue::PointLight(PointLight::default())],
    );
    // Without an editor camera the scene camera is reported, as in a game.
    let e = w.extract();
    assert_eq!(
        e.camera.as_ref().map(|c| c.record),
        Some(e.records.iter().position(|r| r.entity == cam).unwrap() as u32)
    );
    assert_eq!(e.meshes.len(), 1);
    assert_eq!(e.point_lights.len(), 1);

    // The editor camera: an orthographic one looking down.
    let world = Mat4::from_rotation_translation(
        Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
        Vec3::new(0.0, 20.0, 0.0),
    );
    let camera = Camera {
        projection: Projection::Orthographic,
        ortho_height: 12.0,
        ..Camera::default()
    };
    assert!(w.set_edit_camera(Some((world, camera))));
    let e = w.extract();
    let c = e.camera.as_ref().unwrap();
    assert_eq!(c.record, u32::MAX);
    assert_eq!(c.world, world);
    assert_eq!(c.camera.projection, Projection::Orthographic);
    assert_eq!(c.camera.ortho_height, 12.0);
    // The view matrix the renderer derives looks down from 20 m.
    let view = view_matrix(&c.world);
    let p = view.transform_point3(Vec3::ZERO);
    assert!((p.z + 20.0).abs() < 1e-4, "{p:?}");

    // Hidden: the mesh's record goes, and only that record (the light on
    // its child stays; the caller decides what a subtree hide means).
    assert!(w.set_edit_hidden(&[mesh]));
    let e = w.extract();
    assert!(e.meshes.is_empty());
    assert!(e.records.iter().all(|r| r.entity != mesh));
    assert_eq!(e.point_lights.len(), 1);
    assert!(w.set_edit_hidden(&[mesh, light]));
    let e = w.extract();
    assert!(e.point_lights.is_empty());
    assert_eq!(w.edit_hidden().count(), 2);
    // Back to the scene camera and everything shown.
    assert!(w.set_edit_camera(None));
    assert!(w.set_edit_hidden(&[]));
    let e = w.extract();
    assert_ne!(e.camera.as_ref().unwrap().record, u32::MAX);
    assert_eq!(e.meshes.len(), 1);
    // A dead entity is dropped from the set.
    w.submit(Lane::Simulation, vec![Command::Destroy(light)]);
    w.step();
    assert!(w.set_edit_hidden(&[light]));
    assert_eq!(w.edit_hidden().count(), 0);
}
