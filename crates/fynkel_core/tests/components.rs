//! The built-in components are listed once (foundation 6.2): the table
//! behind `ComponentValue` names every authored component of the schema
//! registry, and every one of them is set, read back and removed through
//! the paths that table generates.

use fynkel_contracts::schema::SchemaKind;
use fynkel_core::components::ComponentValue;
use fynkel_core::*;

#[test]
fn the_component_table_lists_every_authored_component_in_id_order() {
    let table: Vec<u32> = ComponentValue::IDS.iter().map(|id| id.0).collect();
    let mut schemas: Vec<u32> = schema::ALL
        .iter()
        .filter(|s| s.kind == SchemaKind::Component && s.authored)
        .map(|s| s.id)
        .collect();
    schemas.sort();
    assert_eq!(table, schemas);
}

/// One value per component, valid as it stands.
fn samples() -> Vec<ComponentValue> {
    vec![
        ComponentValue::Transform(Transform::default()),
        ComponentValue::MeshRenderer(Default::default()),
        ComponentValue::Camera(Default::default()),
        ComponentValue::DirectionalLight(Default::default()),
        ComponentValue::PointLight(Default::default()),
        ComponentValue::SpotLight(Default::default()),
        ComponentValue::Environment(Default::default()),
        ComponentValue::RigidBody(Default::default()),
        ComponentValue::Collider(Default::default()),
        ComponentValue::CharacterController(Default::default()),
        ComponentValue::ThirdPersonCamera(Default::default()),
        ComponentValue::Rotator(Default::default()),
        ComponentValue::Oscillate(Default::default()),
        ComponentValue::Follow(Default::default()),
        ComponentValue::LookAtCamera(Default::default()),
        ComponentValue::LifetimeDespawn(Default::default()),
        ComponentValue::Emitter(Default::default()),
        ComponentValue::TriggerZone(Default::default()),
        ComponentValue::Animator(Default::default()),
        ComponentValue::Skin(Default::default()),
        ComponentValue::AudioSource(Default::default()),
        ComponentValue::AudioListener(Default::default()),
        ComponentValue::Volume(Default::default()),
        ComponentValue::Layer(Default::default()),
        ComponentValue::Tags(Default::default()),
    ]
}

#[test]
fn every_component_is_set_read_back_and_removed() {
    let samples = samples();
    let ids: Vec<ComponentId> = samples.iter().map(|v| v.id()).collect();
    assert_eq!(ids, ComponentValue::IDS, "one sample per table entry");

    let mut w = World::new(WorldConfig::default());
    let (e, rejected) = w
        .spawn_now(
            "all",
            Transform::default(),
            None,
            samples[1..].to_vec(),
            None,
        )
        .unwrap();
    assert!(rejected.is_empty(), "{rejected:?}");

    let back = ComponentValue::all_of(&w, e);
    assert_eq!(back, samples, "registry order, the values as set");

    for id in ComponentValue::IDS.iter().skip(1) {
        let rejected = w.apply_now(vec![Command::RemoveComponent {
            entity: e,
            component: *id,
        }]);
        assert!(rejected.is_empty(), "remove {:?}: {rejected:?}", id.name());
    }
    assert_eq!(
        ComponentValue::all_of(&w, e),
        vec![ComponentValue::Transform(Transform::default())],
        "only the Transform is left; it cannot be removed"
    );
    let rejected = w.apply_now(vec![Command::RemoveComponent {
        entity: e,
        component: ComponentId::TRANSFORM,
    }]);
    assert_eq!(rejected.len(), 1);
}
