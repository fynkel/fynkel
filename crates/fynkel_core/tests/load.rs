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
        active: true,
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
            payload: EventPayload::SceneReady { .. },
            ..
        }]
    ));
    assert_eq!(w.live_entities(), 4);
    // The world keeps ticking normally afterwards.
    w.begin_frame(1.0 / 60.0);
    assert_eq!(w.step().status, Status::Ok);
}

/// The scene above without its second root: a prefab has one (ADR-0022).
fn prefab() -> SceneDescription {
    let mut s = scene();
    s.entities.retain(|e| e.id != id(4));
    s
}

/// `World::instantiate` (M3-08b, ADR-0022): the description's one root
/// spawns under `parent` with the given transform and name — the shape a
/// scene instance expands to — and is returned; its descendants keep
/// their own transforms; the structural checks run before anything is
/// spawned.
#[test]
fn instantiate_places_the_prefab_root() {
    let mut w = World::new(WorldConfig::default());
    let base = w.load_scene(&scene()).unwrap();
    let e1 = base.handle_of(id(1)).unwrap();
    let live = w.live_entities();

    let placed = Transform::from_position(Vec3::new(0.0, 5.0, 0.0));
    let (root, loaded) = w.instantiate(&prefab(), "Copy", placed, Some(e1)).unwrap();
    assert_eq!(loaded.len(), 3);
    assert_eq!(loaded.rejected_components, 0, "{:?}", w.diagnostics());
    assert_eq!(
        w.live_entities(),
        live + 3,
        "no entity besides the prefab's"
    );
    assert_eq!(loaded.handle_of(id(1)), Some(root));
    assert_ne!(
        root, e1,
        "fresh handles; the same description twice is fine"
    );
    assert_eq!(w.ecs().copy::<Name>(root).unwrap().value, "Copy");
    assert_eq!(
        w.ecs().copy::<Transform>(root).unwrap(),
        placed,
        "the given transform replaces the root's own"
    );
    assert_eq!(scene::parent_of(w.ecs(), root), Some(e1));
    let c2 = loaded.handle_of(id(2)).unwrap();
    let c3 = loaded.handle_of(id(3)).unwrap();
    assert_eq!(scene::children_of(w.ecs(), root), vec![c3, c2]);
    // e1 at x 10 → the root at y 5 under it → c2 at x 1.
    let m = scene::world_matrix(w.ecs(), c2).unwrap();
    assert_eq!(m.transform_point3(Vec3::ZERO), Vec3::new(11.0, 5.0, 0.0));
    // Components went through the command path, like a scene's.
    assert!(w.ecs().has::<Camera>(c2));

    // No name: the root keeps its own. No parent: a root of the world.
    let (bare, _) = w
        .instantiate(&prefab(), "", Transform::IDENTITY, None)
        .unwrap();
    assert_eq!(w.ecs().copy::<Name>(bare).unwrap().value, "e1");
    assert_eq!(scene::parent_of(w.ecs(), bare), None);

    // Refusals spawn nothing.
    let live = w.live_entities();
    let stale = Entity::new(w.id(), 7, 60_000);
    assert_eq!(
        w.instantiate(&prefab(), "", Transform::IDENTITY, Some(stale))
            .unwrap_err(),
        LoadError::UnknownAnchorParent(stale)
    );
    assert_eq!(
        w.instantiate(
            &prefab(),
            "",
            Transform::from_position(Vec3::new(f32::NAN, 0.0, 0.0)),
            None
        )
        .unwrap_err(),
        LoadError::AnchorTransform
    );
    // Two parentless entities: not a prefab.
    assert_eq!(
        w.instantiate(&scene(), "", Transform::IDENTITY, None)
            .unwrap_err(),
        LoadError::NotOneRoot(id(99))
    );
    let mut s = prefab();
    s.entities.push(entity(1, None, &[], vec![]));
    assert_eq!(
        w.instantiate(&s, "", Transform::IDENTITY, None)
            .unwrap_err(),
        LoadError::DuplicateEntity(id(1))
    );
    assert_eq!(w.live_entities(), live);
}

/// `World::clone_subtree` (ADR-0026 §4, Unity's `Instantiate(gameObject)`):
/// a live subtree copied with its names, own active flags, child order and
/// components, placed as `instantiate` places a prefab; the source is
/// untouched.
#[test]
fn clone_subtree_copies_a_live_hierarchy() {
    let mut w = World::new(WorldConfig::default());
    let mut s = prefab();
    s.entities
        .iter_mut()
        .find(|e| e.id == id(3))
        .unwrap()
        .active = false;
    let base = w.load_scene(&s).unwrap();
    let e1 = base.handle_of(id(1)).unwrap();
    let (e2, e3) = (
        base.handle_of(id(2)).unwrap(),
        base.handle_of(id(3)).unwrap(),
    );
    let (desc, sources) = w.describe_subtree(e2).unwrap();
    assert_eq!(
        (desc.entities.len(), sources),
        (1, vec![e2]),
        "a leaf alone"
    );

    let live = w.live_entities();
    let placed = Transform::from_position(Vec3::new(0.0, 2.0, 0.0));
    let Cloned {
        root,
        pairs,
        rejected_components: rejected,
    } = w.clone_subtree(e1, None, placed, None).unwrap();
    assert_eq!(rejected, 0, "{:?}", w.diagnostics());
    assert_eq!(w.live_entities(), live + 3);
    assert_eq!(pairs[0], (e1, root));
    assert_eq!(
        pairs.iter().map(|p| p.0).collect::<Vec<_>>(),
        vec![e1, e3, e2]
    );
    assert_eq!(
        w.ecs().copy::<Name>(root).unwrap().value,
        "e1",
        "the source's name"
    );
    assert_eq!(w.ecs().copy::<Transform>(root).unwrap(), placed);
    let (c3, c2) = (pairs[1].1, pairs[2].1);
    assert_eq!(
        scene::children_of(w.ecs(), root),
        vec![c3, c2],
        "child order kept"
    );
    assert_eq!(w.ecs().copy::<Name>(c2).unwrap().value, "e2");
    assert!(w.ecs().has::<Camera>(c2));
    assert_eq!(
        w.ecs().copy::<Transform>(c2).unwrap(),
        Transform::from_position(Vec3::new(1.0, 0.0, 0.0)),
        "descendants keep their local transforms"
    );
    assert!(
        !scene::is_active_self(w.ecs(), c3),
        "the own active flag copied"
    );
    assert!(w.ecs().has::<PointLight>(c3));
    assert_eq!(
        scene::children_of(w.ecs(), e1),
        vec![e3, e2],
        "the source is untouched"
    );

    // A name and a parent; a copy of a copy.
    let named = w
        .clone_subtree(c2, Some("Cam"), Transform::IDENTITY, Some(root))
        .unwrap()
        .root;
    assert_eq!(w.ecs().copy::<Name>(named).unwrap().value, "Cam");
    assert_eq!(scene::parent_of(w.ecs(), named), Some(root));

    let stale = Entity::new(w.id(), 7, 60_000);
    let live = w.live_entities();
    assert_eq!(
        w.clone_subtree(stale, None, Transform::IDENTITY, None)
            .unwrap_err(),
        LoadError::UnknownSource(stale)
    );
    assert_eq!(
        w.clone_subtree(e1, None, Transform::IDENTITY, Some(stale))
            .unwrap_err(),
        LoadError::UnknownAnchorParent(stale)
    );
    assert_eq!(w.live_entities(), live);
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

#[test]
fn extract_resolves_renderable_components_against_records() {
    let mut world = World::new(WorldConfig::default());
    let mut s = scene();
    // A visible and an invisible mesh, and a second, higher-priority camera.
    s.entities[1]
        .components
        .push(ComponentValue::MeshRenderer(MeshRenderer {
            mesh: AssetRef {
                asset: id(50),
                sub: "mesh/0".into(),
            },
            material: AssetRef {
                asset: id(51),
                sub: String::new(),
            },
            ..MeshRenderer::default()
        }));
    s.entities[2]
        .components
        .push(ComponentValue::MeshRenderer(MeshRenderer {
            visible: false,
            ..MeshRenderer::default()
        }));
    s.entities[3]
        .components
        .push(ComponentValue::Camera(Camera {
            priority: 5,
            ..Camera::default()
        }));
    // The mesh's entity on layer 3 (ADR-0020); the others on default.
    s.entities[1]
        .components
        .push(ComponentValue::Layer(scene::Layer { layer: 3 }));
    let loaded = world.load_scene(&s).unwrap();
    let ex = world.extract();
    assert_eq!(ex.records.len(), 4);
    let record_of = |n: u8| {
        let e = loaded.handle_of(id(n)).unwrap();
        ex.records.iter().position(|r| r.entity == e).unwrap() as u32
    };
    assert_eq!(ex.meshes.len(), 1, "invisible renderers are not extracted");
    assert_eq!(ex.meshes[0].record, record_of(1));
    assert_eq!(ex.records[record_of(1) as usize].layer_bit, 1 << 3);
    assert_eq!(
        ex.records[record_of(4) as usize].layer_bit,
        1,
        "no Layer: default"
    );
    assert_eq!(ex.meshes[0].mesh.sub, "mesh/0");
    assert_eq!(ex.meshes[0].material.asset, id(51));
    let cam = ex.camera.unwrap();
    assert_eq!(cam.record, record_of(4), "the highest priority camera wins");
    assert_eq!(cam.camera.priority, 5);
    assert_eq!(ex.directional_lights.len(), 1);
    assert_eq!(ex.directional_lights[0].record, record_of(4));
    // Identity rotation: the light shines along -Z.
    assert!((ex.directional_lights[0].direction - Vec3::NEG_Z).length() < 1e-6);
    assert_eq!(ex.point_lights.len(), 1);
    assert_eq!(ex.point_lights[0].record, record_of(3));
    // e3 is a child of e1 at x=10 with an identity local transform.
    assert!((ex.point_lights[0].position - Vec3::new(10.0, 0.0, 0.0)).length() < 1e-6);
}
