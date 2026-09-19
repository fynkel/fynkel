//! Scene instantiation and component commands (blueprint §19.2c item 5).

use fynkel_core::*;
use fynkel_scene as scene;
use glam::Vec3;

fn id(n: u8) -> Uuid {
    let mut b = [0u8; 16];
    b[15] = n;
    b[6] = 0x40;
    b[8] = 0x80;
    Uuid(b)
}

fn entity(
    n: u8,
    parent: Option<u8>,
    children: &[u8],
    components: Vec<ComponentValue>,
) -> EntityDescription {
    EntityDescription {
        id: id(n),
        name: format!("e{n}"),
        parent: parent.map(id),
        children: children.iter().map(|c| id(*c)).collect(),
        components,
    }
}

fn at(x: f32) -> ComponentValue {
    ComponentValue::Transform(Transform::from_position(Vec3::new(x, 0.0, 0.0)))
}

fn scene() -> SceneDescription {
    SceneDescription {
        id: id(99),
        name: "Test".into(),
        entities: vec![
            // Root listed after its child on purpose: spawn order follows
            // the hierarchy, not the map order.
            entity(
                2,
                Some(1),
                &[],
                vec![at(1.0), ComponentValue::Camera(Camera::default())],
            ),
            entity(1, None, &[3, 2], vec![at(10.0)]),
            entity(
                3,
                Some(1),
                &[],
                vec![ComponentValue::PointLight(PointLight::default())],
            ),
            entity(
                4,
                None,
                &[],
                vec![ComponentValue::DirectionalLight(DirectionalLight::default())],
            ),
        ],
    }
}

#[test]
fn loads_hierarchy_in_document_order_with_components() {
    let mut w = World::new(WorldConfig::default());
    let loaded = w.load_scene(&scene()).unwrap();
    assert_eq!(loaded.len(), 4);
    assert_eq!(loaded.rejected_components, 0, "{:?}", w.diagnostics());
    let order: Vec<Uuid> = loaded.entities().iter().map(|(u, _)| *u).collect();
    assert_eq!(order, vec![id(1), id(3), id(2), id(4)]);
    let e1 = loaded.handle_of(id(1)).unwrap();
    let e2 = loaded.handle_of(id(2)).unwrap();
    let e3 = loaded.handle_of(id(3)).unwrap();
    assert_eq!(loaded.id_of(e2), Some(id(2)));
    assert_eq!(
        scene::children_of(w.ecs(), e1),
        vec![e3, e2],
        "listed child order"
    );
    assert_eq!(scene::parent_of(w.ecs(), e2), Some(e1));
    assert_eq!(w.ecs().copy::<Name>(e1).unwrap().value, "e1");
    // World matrices are valid before the first tick.
    let m = scene::world_matrix(w.ecs(), e2).unwrap();
    assert_eq!(m.transform_point3(Vec3::ZERO), Vec3::new(11.0, 0.0, 0.0));
    // Missing Transform → identity.
    assert_eq!(w.ecs().copy::<Transform>(e3).unwrap(), Transform::IDENTITY);
    assert_eq!(
        ComponentValue::all_of(&w, e2),
        vec![
            ComponentValue::Transform(Transform::from_position(Vec3::new(1.0, 0.0, 0.0))),
            ComponentValue::Camera(Camera::default())
        ]
    );
    assert!(w.ecs().has::<PointLight>(e3));
    let ev = w.drain_events();
    assert!(matches!(
        ev.as_slice(),
        [Event {
            payload: EventPayload::SceneReady,
            ..
        }]
    ));
    assert_eq!(w.live_entities(), 4);
    // The world keeps ticking normally afterwards.
    w.begin_frame(1.0 / 60.0);
    assert_eq!(w.step().status, Status::Ok);
}

#[test]
fn structural_errors_load_nothing() {
    let mut w = World::new(WorldConfig::default());
    let mut s = scene();
    s.entities[1].children = vec![id(2)]; // 3 says parent 1, 1 does not list it
    assert_eq!(
        w.load_scene(&s).unwrap_err(),
        LoadError::ChildParentMismatch {
            entity: id(1),
            child: id(3)
        }
    );
    let mut s = scene();
    s.entities[0].parent = Some(id(7));
    assert_eq!(
        w.load_scene(&s).unwrap_err(),
        LoadError::UnknownParent {
            entity: id(2),
            parent: id(7)
        }
    );
    let mut s = scene();
    s.entities.push(entity(1, None, &[], vec![]));
    assert_eq!(
        w.load_scene(&s).unwrap_err(),
        LoadError::DuplicateEntity(id(1))
    );
    // A two-node cycle passes the local checks but reaches no root.
    let s = SceneDescription {
        id: id(99),
        name: "Cycle".into(),
        entities: vec![
            entity(1, Some(2), &[2], vec![]),
            entity(2, Some(1), &[1], vec![]),
        ],
    };
    assert_eq!(w.load_scene(&s).unwrap_err(), LoadError::Unreachable(id(1)));
    let mut s = scene();
    s.entities[0]
        .components
        .push(ComponentValue::Camera(Camera::default()));
    assert_eq!(
        w.load_scene(&s).unwrap_err(),
        LoadError::DuplicateComponent {
            entity: id(2),
            component: ComponentId::CAMERA
        }
    );
    assert_eq!(w.live_entities(), 0);
    assert!(w.drain_events().is_empty());
}

#[test]
fn invalid_component_values_are_rejected_like_commands() {
    let mut w = World::new(WorldConfig::default());
    let mut s = scene();
    let cam = Camera {
        near: 0.0,
        ..Camera::default()
    };
    s.entities[0].components[1] = ComponentValue::Camera(cam);
    let loaded = w.load_scene(&s).unwrap();
    assert_eq!(loaded.rejected_components, 1);
    let d = &w.diagnostics()[0];
    assert_eq!(d.code, DiagnosticCode::INVALID_PAYLOAD);
    assert_eq!(d.component, ComponentId::CAMERA);
    assert_eq!(d.command_index, u32::MAX);
    assert_eq!(d.entity, loaded.handle_of(id(2)).unwrap());
    assert!(!w.ecs().has::<Camera>(d.entity));
    assert_eq!(w.live_entities(), 4, "entities still exist");
}

#[test]
fn set_and_remove_component_commands() {
    let mut w = World::new(WorldConfig::default());
    let loaded = w.load_scene(&scene()).unwrap();
    let e4 = loaded.handle_of(id(4)).unwrap();
    w.drain_events();
    // Presentation lane: plain-data writes apply immediately.
    let light = DirectionalLight {
        intensity: 5.0,
        ..DirectionalLight::default()
    };
    let r = w.submit(
        Lane::Presentation,
        vec![
            Command::SetComponent {
                entity: e4,
                value: ComponentValue::DirectionalLight(light),
            },
            Command::RemoveComponent {
                entity: e4,
                component: ComponentId::CAMERA,
            },
        ],
    );
    assert_eq!((r.accepted, r.rejected), (1, 1));
    assert_eq!(w.diagnostics()[0].code, DiagnosticCode::WRONG_LANE);
    assert_eq!(w.ecs().copy::<DirectionalLight>(e4).unwrap(), light);
    // Simulation lane: Transform through SetComponent == SetTransform; remove
    // a present and an absent component; protected components stay.
    w.begin_frame(1.0 / 60.0);
    w.submit(
        Lane::Simulation,
        vec![
            Command::SetComponent {
                entity: e4,
                value: at(3.0),
            },
            Command::RemoveComponent {
                entity: e4,
                component: ComponentId::DIRECTIONAL_LIGHT,
            },
            Command::RemoveComponent {
                entity: e4,
                component: ComponentId::DIRECTIONAL_LIGHT,
            },
            Command::RemoveComponent {
                entity: e4,
                component: ComponentId::TRANSFORM,
            },
            Command::RemoveComponent {
                entity: e4,
                component: ComponentId(999),
            },
            Command::SetComponent {
                entity: e4,
                value: ComponentValue::PointLight(PointLight {
                    range: -1.0,
                    ..PointLight::default()
                }),
            },
        ],
    );
    let r = w.step();
    assert_eq!((r.applied, r.rejected), (2, 4), "{:?}", w.diagnostics());
    let codes: Vec<DiagnosticCode> = w.diagnostics().iter().map(|d| d.code).collect();
    assert_eq!(
        codes,
        vec![
            DiagnosticCode::MISSING_COMPONENT,
            DiagnosticCode::INVALID_PAYLOAD,
            DiagnosticCode::UNKNOWN_COMPONENT,
            DiagnosticCode::INVALID_PAYLOAD
        ]
    );
    assert!(!w.ecs().has::<DirectionalLight>(e4));
    assert!(!w.ecs().has::<PointLight>(e4));
    assert_eq!(w.ecs().copy::<Transform>(e4).unwrap().position.x, 3.0);
}
