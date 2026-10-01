use fynkel_contracts::{AssetRef, Uuid};
use fynkel_physics::{Physics, PreparedMesh};
#[test]
fn prepared_mesh_publishes_only_when_consumed() {
    let mut physics = Physics::new();
    let key = AssetRef::new(Uuid::from_bytes(1u128.to_be_bytes()));
    let mesh =
        PreparedMesh::new(&[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], &[0, 1, 2]).unwrap();
    assert!(!physics.has_mesh(&key));
    assert_eq!(physics.register_prepared_mesh(key.clone(), mesh), 1);
    assert!(physics.has_mesh(&key));
}
#[test]
fn malformed_meshes_fail_before_registry_mutation() {
    let mut physics = Physics::new();
    let key = AssetRef::new(Uuid::from_bytes(1u128.to_be_bytes()));
    for (vertices, indices) in [
        (vec![[0.0; 3]], vec![0, 0, 0, 0]),
        (vec![[f32::NAN; 3]], vec![0, 0, 0]),
        (vec![[0.0; 3]], vec![0, 0, 1]),
    ] {
        assert!(physics
            .register_mesh(key.clone(), &vertices, &indices)
            .is_err());
        assert!(!physics.has_mesh(&key));
    }
}
