//! Document → typed description → world (blueprint §19.2c item 5).

use std::path::Path;

use fynkel_core::{Camera, ComponentValue, Projection, World, WorldConfig};
use fynkel_document::generated::{decode_component, encode_component, AUTHORED_COMPONENTS};
use fynkel_document::{describe, Document, DocumentCodec, DocumentKind, Node, Scene, SchemaSet};
use glam::Vec3;

fn fixture(rel: &str) -> Scene {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(rel);
    Scene::from_document(Document::read(&path, Some(DocumentKind::Scene)).unwrap())
}

#[test]
fn training_room_describes_and_loads() {
    let scene = fixture("valid/training_room.scene.json");
    let schemas =
        SchemaSet::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas")).unwrap();
    assert!(scene.validate(&schemas).is_empty());
    let described = describe(&scene, &schemas);
    assert!(
        described.diagnostics.is_empty(),
        "{:#?}",
        described.diagnostics
    );
    let d = &described.description;
    assert_eq!(d.name, "TrainingRoom");
    assert_eq!(d.entities.len(), 5);
    let player = &d.entities[0];
    assert_eq!(player.name, "Player");
    assert_eq!(player.components.len(), 2);
    assert!(
        matches!(player.components[1], ComponentValue::MeshRenderer(ref m) if m.mesh.sub == "mesh/0" && m.material.sub.is_empty())
    );
    let camera = &d.entities[1];
    assert_eq!(camera.parent, Some(player.id));
    assert!(matches!(
        camera.components[1],
        ComponentValue::Camera(Camera {
            projection: Projection::Perspective,
            ..
        })
    ));

    let mut world = World::new(WorldConfig::default());
    let loaded = world.load_scene(d).unwrap();
    assert_eq!(loaded.len(), 5);
    assert_eq!(loaded.rejected_components, 0, "{:?}", world.diagnostics());
    let cam = loaded.handle_of(camera.id).unwrap();
    let m = fynkel_scene::world_matrix(world.ecs(), cam).unwrap();
    assert_eq!(
        m.transform_point3(Vec3::ZERO),
        Vec3::new(0.0, 2.5, 4.0),
        "child under Player at y=1"
    );
    // Everything that was loaded reads back as the same typed values.
    for e in &d.entities {
        let all = ComponentValue::all_of(&world, loaded.handle_of(e.id).unwrap());
        assert_eq!(all.len(), e.components.len(), "{}", e.name);
        for c in &e.components {
            assert!(all.contains(c), "{}: {c:?}", e.name);
        }
    }
}

#[test]
fn component_values_round_trip_through_nodes() {
    let scene = fixture("valid/training_room.scene.json");
    let mut seen = 0;
    for e in &scene.entities {
        for c in &e.components {
            let value = decode_component(&c.type_id, &c.data).unwrap().unwrap();
            let (type_id, node) = encode_component(&value);
            assert_eq!(type_id, c.type_id);
            let again = decode_component(type_id, &node).unwrap().unwrap();
            assert_eq!(again, value);
            seen += 1;
        }
    }
    assert_eq!(seen, 11);
    assert_eq!(AUTHORED_COMPONENTS.len(), 17);
    assert!(!AUTHORED_COMPONENTS.contains(&"fynkel.Name"));
    // Encoded numbers are the shortest f32 text, so files stay readable.
    let node = Camera::default().to_node();
    assert_eq!(node.get("near").unwrap().as_f64(), Some(0.1));
    assert_eq!(
        fynkel_document::json::write(&Node::object(vec![("c".into(), node)]))
            .lines()
            .nth(3)
            .unwrap()
            .trim(),
        "\"fov_y\": 1.0471976,"
    );
}

#[test]
fn unknown_and_undecodable_components_are_diagnosed() {
    let scene = fixture("scene/unknown_component.scene.json");
    let described = describe(&scene, &SchemaSet::embedded());
    assert_eq!(described.diagnostics.len(), 1);
    assert_eq!(described.diagnostics[0].code.as_str(), "FYN-D0205");
    assert!(!described.has_errors());
    assert!(described.description.entities[0].components.is_empty());
    // The unknown component is still handed over as an authored behaviour
    // (an SDK that registered the type attaches it), flagged as unknown.
    assert_eq!(described.behaviours.len(), 1);
    assert!(!described.behaviours[0].known);
    assert_eq!(
        described.behaviours[0].entity,
        described.description.entities[0].id
    );

    // Declaring the type (a project registry entry) makes it a known
    // behaviour: no warning, same hand-over.
    let mut schemas = SchemaSet::embedded();
    schemas.declare_behaviour(&described.behaviours[0].type_id, 1);
    let described = describe(&scene, &schemas);
    assert!(described.diagnostics.is_empty());
    assert_eq!(described.behaviours.len(), 1);
    assert!(described.behaviours[0].known);
    assert!(scene
        .validate(&schemas)
        .iter()
        .all(|d| d.code.as_str() != "FYN-D0205"));

    let scene = fixture("scene/bad_component_values.scene.json");
    let described = describe(&scene, &SchemaSet::embedded());
    let codes: Vec<&str> = described
        .diagnostics
        .iter()
        .map(|d| d.code.as_str())
        .collect();
    assert_eq!(
        codes,
        vec!["FYN-D0206", "FYN-D0206"],
        "{:#?}",
        described.diagnostics
    );
    assert!(described.has_errors());
    assert!(described.diagnostics[0]
        .pointer
        .as_deref()
        .unwrap()
        .ends_with("/data/mesh/asset"));
    assert!(described.diagnostics[1]
        .pointer
        .as_deref()
        .unwrap()
        .ends_with("/data/projection"));
    assert!(described.diagnostics.iter().all(|d| d.line.is_some()));
}
