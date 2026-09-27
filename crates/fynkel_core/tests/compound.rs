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

// Before ADR-0034 the body fell through the floor (y −75 after 4 s).
#[test]
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
                let (child_body, floor_body) = if c.a == child {
                    (c.body_a, c.body_b)
                } else {
                    (c.body_b, c.body_a)
                };
                assert_eq!(child_body, Some(body), "the child's side names its body");
                assert_eq!(floor_body, None, "static geometry has no body");
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

fn floor(w: &mut World) -> Entity {
    spawn(
        w,
        1,
        Vec3::new(0.0, -0.5, 0.0),
        None,
        vec![ComponentValue::Collider(Collider {
            size: Vec3::new(20.0, 0.5, 20.0),
            ..Collider::default()
        })],
    )
}

fn position(w: &World, e: Entity) -> Vec3 {
    w.ecs().copy::<Transform>(e).unwrap().position
}

fn run(w: &mut World, ticks: usize) {
    for _ in 0..ticks {
        w.step();
    }
}

#[test]
fn a_grandchild_joins_through_a_plain_child_and_a_moved_child_moves_its_shape() {
    let mut w = World::new(WorldConfig::default());
    floor(&mut w);
    let body = spawn(
        &mut w,
        2,
        Vec3::new(0.0, 4.0, 0.0),
        None,
        vec![ComponentValue::RigidBody(RigidBody::default())],
    );
    // An empty in between, as a model's node hierarchy has.
    let pivot = spawn(&mut w, 3, Vec3::new(0.0, -0.5, 0.0), Some(body), vec![]);
    let leg = spawn(
        &mut w,
        4,
        Vec3::new(0.0, -0.5, 0.0),
        Some(pivot),
        vec![ComponentValue::Collider(Collider::default())],
    );
    run(&mut w, 240);
    let y = position(&w, body).y;
    assert!(
        (y - 1.5).abs() < 0.05,
        "the leg, 1 m down, holds the body at 1.5: {y}"
    );
    // Moving the leg within the body moves its shape: 1 m lower, the body
    // rises by 1 m.
    let mut t = w.ecs().copy::<Transform>(leg).unwrap();
    t.position.y = -1.5;
    w.submit(
        Lane::Simulation,
        vec![Command::SetTransform {
            entity: leg,
            transform: t,
        }],
    );
    run(&mut w, 240);
    let y = position(&w, body).y;
    assert!(
        (y - 2.5).abs() < 0.05,
        "the lowered leg lifts the body to 2.5: {y}"
    );
}

#[test]
fn deactivating_a_joined_child_drops_its_shape_and_keeps_the_body_moving() {
    let mut w = World::new(WorldConfig::default());
    // No gravity: the body drifts; its two children are its shape.
    let body = spawn(
        &mut w,
        2,
        Vec3::ZERO,
        None,
        vec![ComponentValue::RigidBody(RigidBody {
            gravity_scale: 0.0,
            ..RigidBody::default()
        })],
    );
    let front = spawn(
        &mut w,
        3,
        Vec3::new(0.0, 0.0, 1.0),
        Some(body),
        vec![ComponentValue::Collider(Collider::default())],
    );
    let _back = spawn(
        &mut w,
        4,
        Vec3::new(0.0, 0.0, -1.0),
        Some(body),
        vec![ComponentValue::Collider(Collider::default())],
    );
    w.submit(
        Lane::Simulation,
        vec![Command::Body {
            entity: body,
            op: BodyCommand::SetLinearVelocity(Vec3::X * 2.0),
        }],
    );
    run(&mut w, 10);
    assert_eq!(w.physics().stats().colliders, 2);
    w.submit(
        Lane::Simulation,
        vec![Command::SetActive {
            entity: front,
            active: false,
        }],
    );
    run(&mut w, 10);
    assert_eq!(
        w.physics().stats().colliders,
        1,
        "the front's shape is gone"
    );
    assert_eq!(w.physics().stats().bodies, 1, "one body throughout");
    let v = w.physics().body_state(body).unwrap().linear_velocity;
    assert!(
        (v - Vec3::X * 2.0).length() < 1e-3,
        "the body keeps moving: {v}"
    );
    assert!(
        !w.ecs().has::<PhysicsOwned>(front),
        "only the body is physics-owned"
    );
}

#[test]
fn a_child_with_its_own_body_or_under_a_character_does_not_join() {
    let mut w = World::new(WorldConfig::default());
    floor(&mut w);
    let parent = spawn(
        &mut w,
        2,
        Vec3::new(0.0, 0.5, 0.0),
        None,
        vec![
            ComponentValue::RigidBody(RigidBody {
                body_type: BodyType::Kinematic,
                ..RigidBody::default()
            }),
            ComponentValue::Collider(Collider::default()),
        ],
    );
    // Its own dynamic body: it falls off the kinematic parent's side.
    let own = spawn(
        &mut w,
        3,
        Vec3::new(3.0, 3.0, 0.0),
        Some(parent),
        vec![
            ComponentValue::RigidBody(RigidBody::default()),
            ComponentValue::Collider(Collider::default()),
        ],
    );
    let hero = spawn(
        &mut w,
        4,
        Vec3::new(-4.0, 1.0, 0.0),
        None,
        vec![ComponentValue::CharacterController(
            CharacterController::default(),
        )],
    );
    let sword = spawn(
        &mut w,
        5,
        Vec3::new(0.5, 0.0, 0.0),
        Some(hero),
        vec![ComponentValue::Collider(Collider {
            size: Vec3::splat(0.1),
            is_sensor: true,
            ..Collider::default()
        })],
    );
    run(&mut w, 120);
    assert!(w.ecs().has::<PhysicsOwned>(own), "its own dynamic body");
    let p = position(&w, own);
    assert!(
        (p.y - 0.0).abs() < 0.05,
        "it fell onto the floor beside its parent (local y 0 = world 0.5): {p}"
    );
    assert_eq!(w.physics().body_state(sword), None, "the sword has no body");
    assert_eq!(
        w.physics().stats().bodies,
        3,
        "parent, own and the hero's capsule"
    );
}

#[test]
fn removing_the_body_leaves_its_children_as_static_geometry() {
    let mut w = World::new(WorldConfig::default());
    let body = spawn(
        &mut w,
        2,
        Vec3::new(0.0, 4.0, 0.0),
        None,
        vec![ComponentValue::RigidBody(RigidBody {
            gravity_scale: 0.0,
            ..RigidBody::default()
        })],
    );
    let child = spawn(
        &mut w,
        3,
        Vec3::new(0.0, -1.0, 0.0),
        Some(body),
        vec![ComponentValue::Collider(Collider::default())],
    );
    run(&mut w, 5);
    w.submit(
        Lane::Simulation,
        vec![Command::RemoveComponent {
            entity: body,
            component: ComponentId::RIGID_BODY,
        }],
    );
    run(&mut w, 5);
    assert_eq!(w.physics().stats().bodies, 0);
    assert_eq!(
        w.physics().stats().colliders,
        1,
        "the child is static geometry now"
    );
    // A ball dropped onto it lands on its top (y 3.5).
    let ball = spawn(
        &mut w,
        4,
        Vec3::new(0.0, 6.0, 0.0),
        None,
        vec![
            ComponentValue::RigidBody(RigidBody::default()),
            ComponentValue::Collider(Collider {
                shape: ColliderShape::Sphere,
                size: Vec3::new(0.5, 0.0, 0.0),
                ..Collider::default()
            }),
        ],
    );
    run(&mut w, 180);
    let y = position(&w, ball).y;
    assert!(
        (y - 4.0).abs() < 0.05,
        "the ball rests on the child's box: {y}"
    );
    let _ = child;
}
