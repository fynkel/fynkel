use fynkel_core::*;
use glam::Vec3;
#[test]
fn compact_snapshot_tracks_membership_dependencies_and_owns_its_data() {
    let mut world = World::new(Default::default());
    let parent = fynkel_scene::spawn(
        world.ecs_mut(),
        Transform::from_position(Vec3::X * 3.0),
        None,
    )
    .unwrap();
    let child = fynkel_scene::spawn(
        world.ecs_mut(),
        Transform::from_position(Vec3::Y),
        Some(parent),
    )
    .unwrap();
    world
        .ecs_mut()
        .insert(child, MeshRenderer::default())
        .unwrap();
    for _ in 0..1000 {
        fynkel_scene::spawn(world.ecs_mut(), Transform::IDENTITY, None).unwrap();
    }
    world.refresh_physics();
    let mut frame = Extract::default();
    world.extract_into(&mut frame, false);
    assert_eq!(frame.records.len(), 1);
    assert_eq!(frame.records[0].entity, child);
    assert_eq!(
        frame.records[0].world.w_axis.truncate(),
        Vec3::new(3.0, 1.0, 0.0)
    );
    let published = frame.clone();
    let capacity = frame.records.capacity();
    world.extract_into(&mut frame, false);
    assert_eq!(frame.records.capacity(), capacity);
    assert_eq!(world.render_membership_candidates(), 0);
    world.ecs_mut().remove::<MeshRenderer>(child).unwrap();
    world
        .ecs_mut()
        .insert(parent, PointLight::default())
        .unwrap();
    world.extract_into(&mut frame, false);
    assert_eq!(frame.records.len(), 1);
    assert_eq!(frame.records[0].entity, parent);
    assert_eq!(published.records[0].entity, child);
    assert_eq!(published.meshes.len(), 1);
    world.extract_into(&mut frame, true);
    assert_eq!(frame.records.len(), 1002);
}

#[test]
#[ignore = "manual foundation extraction qualification"]
fn extraction_qualification() {
    let mut world = World::new(Default::default());
    let mut entities = Vec::new();
    for i in 0..10000 {
        let e = fynkel_scene::spawn(
            world.ecs_mut(),
            Transform::from_position(Vec3::X * i as f32),
            None,
        )
        .unwrap();
        if i % 10 == 0 {
            world.ecs_mut().insert(e, MeshRenderer::default()).unwrap();
        }
        entities.push(e);
    }
    world.refresh_physics();
    let mut frame = Extract::default();
    world.extract_into(&mut frame, false);
    let mut results = Vec::new();
    for (name, count) in [("static", 0), ("sparse", 1), ("dense", 10000)] {
        let mut samples = Vec::new();
        for _ in 0..60 {
            for &e in &entities[..count] {
                world.ecs().get_mut::<Transform>(e).unwrap().position.y += 0.01;
            }
            world.refresh_physics();
            let start = std::time::Instant::now();
            world.extract_into(&mut frame, false);
            samples.push(start.elapsed().as_secs_f64() * 1e6);
            assert_eq!(frame.records.len(), 1000);
        }
        samples.sort_by(f64::total_cmp);
        results.push(serde_json::json!({"mode":name,"entities":10000,"render_records":1000,"samples_us":samples}));
    }
    let value = serde_json::json!({"extract":results});
    if let Ok(path) = std::env::var("FYNKEL_F6_EXTRACT_OUT") {
        std::fs::write(path, value.to_string()).unwrap();
    }
    println!("{value}");
}
