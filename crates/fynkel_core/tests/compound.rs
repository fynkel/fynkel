//! Child colliders under a Rigidbody (docs/foundation.md 2.1). In Unity a
//! `Collider` on a child of a `Rigidbody` is part of that body — a compound:
//! the child moves with the body, its shape stops the body, and its
//! contacts are the body's.

use fynkel_core::components::ComponentValue;
use fynkel_core::*;
use glam::Vec3;

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
            name: None,
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
    let cmds: Vec<Command> = components
        .into_iter()
        .map(|value| Command::SetComponent { entity, value })
        .collect();
    if !cmds.is_empty() {
        w.submit(Lane::Simulation, cmds);
    }
    entity
}

// Fails today: the body falls through the floor (y −75 after 4 s). On once
// ADR-0034 is confirmed.
#[test]
#[ignore = "waits for ADR-0034 (foundation 2.1)"]
fn a_child_collider_is_part_of_its_parents_body() {
    let mut w = World::new(WorldConfig::default());
    let floor = spawn(
        &mut w,
        1,
        Vec3::new(0.0, -0.5, 0.0),
        None,
        vec![ComponentValue::Collider(Collider {
            size: Vec3::new(20.0, 0.5, 20.0),
            ..Collider::default()
        })],
    );
    // A body with no collider of its own; its shape is a child's box, 1 m
    // below the body's origin.
    let body = spawn(
        &mut w,
        2,
        Vec3::new(0.0, 4.0, 0.0),
        None,
        vec![ComponentValue::RigidBody(RigidBody::default())],
    );
    let child = spawn(
        &mut w,
        3,
        Vec3::new(0.0, -1.0, 0.0),
        Some(body),
        vec![ComponentValue::Collider(Collider::default())],
    );

    let mut enters = Vec::new();
    for _ in 0..240 {
        w.step();
        for e in w.after_physics_events() {
            if let EventPayload::CollisionEnter(c) = &e.payload {
                enters.push((c.a, c.b));
            }
        }
    }
    let body_y = w.ecs().copy::<Transform>(body).unwrap().position.y;
    assert!(
        (body_y - 1.5).abs() < 0.05,
        "the child's box (0.5 half height, 1 m below) rests on the floor: body at y 1.5, got {body_y}"
    );
    let child_local = w.ecs().copy::<Transform>(child).unwrap().position;
    assert!(
        (child_local - Vec3::new(0.0, -1.0, 0.0)).length() < 1e-4,
        "the child keeps its place in the body: {child_local}"
    );
    assert_eq!(enters.len(), 1, "one landing: {enters:?}");
    let (a, b) = enters[0];
    assert!(
        (a == child && b == floor) || (a == floor && b == child),
        "the contact names the collider's entity (the child), as Unity's Collision.collider: {enters:?}"
    );
}
