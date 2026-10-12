//! What a raycast promises (stage 2 plan §3.4): the closest drawn object
//! along the ray, with the point, the face normal and the distance in the
//! world, through any hierarchy, scale or mirror, and only what a view
//! would draw from the side it would draw it.

use fynkel_world::*;

struct Scene {
    world: World,
    cube: MeshId,
    plane: MeshId,
    sphere: MeshId,
    one_sided: MaterialId,
    two_sided: MaterialId,
}

fn scene() -> Scene {
    let mut world = World::new();
    let cube = world.add_mesh(MeshData::cube());
    let plane = world.add_mesh(MeshData::plane());
    let sphere = world.add_mesh(MeshData::sphere(32, 16));
    let one_sided = world.add_material(Material::default()).unwrap();
    let two_sided = world.add_material(Material { double_sided: true, ..Material::default() }).unwrap();
    Scene { world, cube, plane, sphere, one_sided, two_sided }
}

impl Scene {
    fn spawn(&mut self, mesh: MeshId, material: MaterialId, t: Transform) -> ObjectId {
        let id = self.world.spawn();
        self.world.insert(id, t).unwrap();
        self.world
            .insert(id, MeshRenderer { mesh: Some(mesh), material: Some(material), ..Default::default() })
            .unwrap();
        id
    }

    fn cube_at(&mut self, position: Vec3) -> ObjectId {
        self.spawn(self.cube, self.one_sided, Transform::from_position(position))
    }

    fn cast(&self, origin: Vec3, direction: Vec3) -> Option<RayHit> {
        self.world.raycast(Ray::new(origin, direction).unwrap(), f32::INFINITY, u32::MAX)
    }
}

fn close(a: Vec3, b: Vec3) -> bool {
    a.abs_diff_eq(b, 1e-5)
}

#[track_caller]
fn assert_hit(hit: Option<RayHit>, object: ObjectId, point: Vec3, normal: Vec3, distance: f32) {
    let hit = hit.expect("a hit");
    assert_eq!(hit.object, object);
    assert!(close(hit.point, point), "point {} isn't {point}", hit.point);
    assert!(close(hit.normal, normal), "normal {} isn't {normal}", hit.normal);
    assert!((hit.distance - distance).abs() < 1e-5, "distance {} isn't {distance}", hit.distance);
}

#[test]
fn a_ray_needs_a_direction_and_a_finite_origin() {
    assert!(Ray::new(Vec3::ZERO, Vec3::ZERO).is_none());
    assert!(Ray::new(Vec3::ZERO, Vec3::NAN).is_none());
    assert!(Ray::new(Vec3::INFINITY, Vec3::X).is_none());
    let ray = Ray::new(Vec3::ONE, Vec3::new(0.0, 0.0, -3.0)).unwrap();
    assert_eq!(ray.direction(), Vec3::NEG_Z, "the direction is made unit length");
    assert_eq!(ray.at(2.0), Vec3::new(1.0, 1.0, -1.0));
}

#[test]
fn the_closest_object_is_hit_whichever_was_spawned_first() {
    let mut s = scene();
    let far = s.cube_at(Vec3::new(0.0, 0.0, -10.0));
    let near = s.cube_at(Vec3::new(0.0, 0.0, -4.0));
    assert_hit(s.cast(Vec3::ZERO, Vec3::NEG_Z), near, Vec3::new(0.0, 0.0, -3.5), Vec3::Z, 3.5);
    assert_hit(s.cast(Vec3::new(0.0, 0.0, -20.0), Vec3::Z), far, Vec3::new(0.0, 0.0, -10.5), Vec3::NEG_Z, 9.5);
    // From the side, through the near cube's top face.
    assert_hit(s.cast(Vec3::new(0.2, 3.0, -4.1), Vec3::NEG_Y), near, Vec3::new(0.2, 0.5, -4.1), Vec3::Y, 2.5);
}

#[test]
fn a_miss_is_none_and_the_reach_is_kept() {
    let mut s = scene();
    s.cube_at(Vec3::new(0.0, 0.0, -4.0));
    assert!(s.cast(Vec3::ZERO, Vec3::Z).is_none(), "behind the ray");
    assert!(s.cast(Vec3::new(0.6, 0.0, 0.0), Vec3::NEG_Z).is_none(), "beside the cube");
    let ray = Ray::new(Vec3::ZERO, Vec3::NEG_Z).unwrap();
    assert!(s.world.raycast(ray, 3.4, u32::MAX).is_none());
    assert!(s.world.raycast(ray, 3.6, u32::MAX).is_some());
}

#[test]
fn a_ray_inside_the_box_but_past_the_triangles_misses() {
    let mut s = scene();
    let sphere = s.spawn(s.sphere, s.one_sided, Transform::from_position(Vec3::ZERO));
    // Through the corner of the sphere's box, which the sphere doesn't fill.
    let origin = Vec3::new(0.45, 0.45, 5.0);
    assert!(s.cast(origin, Vec3::NEG_Z).is_none());
    let hit = s.cast(Vec3::new(0.0, 0.0, 5.0), Vec3::NEG_Z).unwrap();
    assert_eq!(hit.object, sphere);
    assert!((hit.distance - 4.5).abs() < 1e-3, "a sphere of radius 0.5: {}", hit.distance);
}

#[test]
fn children_are_hit_where_the_hierarchy_puts_them() {
    let mut s = scene();
    let root = s.world.spawn();
    let turned = Transform {
        position: Vec3::new(10.0, 0.0, 0.0),
        rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        scale: Vec3::splat(2.0),
    };
    s.world.insert(root, turned).unwrap();
    // 1 along the parent's -Z, which the turn points along world -X, and the
    // scale makes 2.
    let child = s.spawn(s.cube, s.one_sided, Transform::from_position(Vec3::new(0.0, 0.0, -1.0)));
    s.world.set_parent(child, Some(root)).unwrap();
    // The child is a 2 × 2 × 2 cube centred at (8, 0, 0).
    assert_hit(s.cast(Vec3::new(8.0, 10.0, 0.0), Vec3::NEG_Y), child, Vec3::new(8.0, 1.0, 0.0), Vec3::Y, 9.0);
    assert_hit(s.cast(Vec3::new(0.0, 0.5, 0.5), Vec3::X), child, Vec3::new(7.0, 0.5, 0.5), Vec3::NEG_X, 7.0);
    assert!(s.cast(Vec3::new(10.0, 10.0, 0.0), Vec3::NEG_Y).is_none(), "the parent draws nothing");
}

#[test]
fn distances_and_normals_are_in_the_world_under_an_uneven_scale() {
    let mut s = scene();
    let t = Transform {
        position: Vec3::ZERO,
        rotation: Quat::from_rotation_z(std::f32::consts::FRAC_PI_4),
        scale: Vec3::new(4.0, 1.0, 1.0),
    };
    let long = s.spawn(s.cube, s.one_sided, t);
    // A box 4 long, turned 45° about Z: its +Y face is the plane x·(-1) + y = 0
    // shifted out by 0.5 along (-1, 1)/√2.
    let normal = Vec3::new(-1.0, 1.0, 0.0).normalize();
    let hit = s.cast(normal * 5.0, -normal).unwrap();
    assert_hit(Some(hit), long, normal * 0.5, normal, 4.5);
}

#[test]
fn back_faces_are_hit_only_on_double_sided_materials() {
    let mut s = scene();
    let floor = s.spawn(s.plane, s.one_sided, Transform::from_position(Vec3::ZERO));
    assert_hit(s.cast(Vec3::new(0.1, 2.0, 0.1), Vec3::NEG_Y), floor, Vec3::new(0.1, 0.0, 0.1), Vec3::Y, 2.0);
    assert!(s.cast(Vec3::new(0.1, -2.0, 0.1), Vec3::Y).is_none(), "from below, where it's culled");

    s.world
        .insert(floor, MeshRenderer { mesh: Some(s.plane), material: Some(s.two_sided), ..Default::default() })
        .unwrap();
    // The normal faces the ray, as the renderer flips a back face's.
    assert_hit(s.cast(Vec3::new(0.1, -2.0, 0.1), Vec3::Y), floor, Vec3::new(0.1, 0.0, 0.1), Vec3::NEG_Y, 2.0);

    // From inside a one-sided cube, every face is a back face.
    s.world.despawn(floor).unwrap();
    s.cube_at(Vec3::ZERO);
    assert!(s.cast(Vec3::ZERO, Vec3::X).is_none());
}

#[test]
fn a_mirrored_object_keeps_its_front_faces() {
    let mut s = scene();
    let t = Transform { position: Vec3::ZERO, rotation: Quat::IDENTITY, scale: Vec3::new(1.0, -1.0, 1.0) };
    let flipped = s.spawn(s.plane, s.one_sided, t);
    // The plane faced +Y; mirrored in Y it faces -Y, and is drawn from below.
    assert_hit(s.cast(Vec3::new(0.1, -2.0, 0.1), Vec3::Y), flipped, Vec3::new(0.1, 0.0, 0.1), Vec3::NEG_Y, 2.0);
    assert!(s.cast(Vec3::new(0.1, 2.0, 0.1), Vec3::NEG_Y).is_none());
    // A mirrored cube is still solid from outside.
    let t =
        Transform { position: Vec3::new(5.0, 0.0, 0.0), rotation: Quat::IDENTITY, scale: Vec3::new(-1.0, 1.0, 1.0) };
    let cube = s.spawn(s.cube, s.one_sided, t);
    assert_hit(s.cast(Vec3::new(5.0, 0.0, 3.0), Vec3::NEG_Z), cube, Vec3::new(5.0, 0.0, 0.5), Vec3::Z, 2.5);
}

#[test]
fn only_what_a_view_draws_is_hit() {
    let mut s = scene();
    let target = s.cube_at(Vec3::new(0.0, 0.0, -4.0));
    let ray = (Vec3::ZERO, Vec3::NEG_Z);
    let missing = [
        MeshRenderer { mesh: Some(s.cube), material: None, ..Default::default() },
        MeshRenderer { mesh: None, material: Some(s.one_sided), ..Default::default() },
    ];
    for renderer in missing {
        s.world.insert(target, renderer).unwrap();
        assert!(s.cast(ray.0, ray.1).is_none(), "{renderer:?} draws nothing");
    }
    s.world
        .insert(target, MeshRenderer { mesh: Some(s.cube), material: Some(s.one_sided), ..Default::default() })
        .unwrap();
    s.world.remove::<Transform>(target).unwrap();
    assert!(s.cast(Vec3::ZERO, Vec3::NEG_Z).is_none(), "no Transform, not drawn");
    s.world.insert(target, Transform::from_position(Vec3::new(0.0, 0.0, -4.0))).unwrap();

    // A flattened object covers nothing, and doesn't stop the ray.
    let flat = Transform { position: Vec3::new(0.0, 0.0, -2.0), rotation: Quat::IDENTITY, scale: Vec3::ZERO };
    s.spawn(s.cube, s.one_sided, flat);
    assert_eq!(s.cast(ray.0, ray.1).unwrap().object, target);
}

#[test]
fn an_inactive_object_and_everything_below_it_are_not_hit() {
    let mut s = scene();
    let behind = s.cube_at(Vec3::new(0.0, 0.0, -8.0));
    let parent = s.cube_at(Vec3::new(0.0, 0.0, -2.0));
    let child = s.cube_at(Vec3::new(0.0, 0.0, -3.0));
    s.world.set_parent(child, Some(parent)).unwrap();
    let mut caster = Raycaster::default();
    let ray = Ray::new(Vec3::ZERO, Vec3::NEG_Z).unwrap();
    let hit = |caster: &mut Raycaster, s: &Scene| caster.cast(&s.world, ray, f32::INFINITY, u32::MAX).map(|h| h.object);

    assert_eq!(hit(&mut caster, &s), Some(parent));
    s.world.set_active(parent, false).unwrap();
    assert_eq!(hit(&mut caster, &s), Some(behind), "the parent and its child are passed through");
    assert_eq!(s.world.raycast(ray, f32::INFINITY, u32::MAX).map(|h| h.object), Some(behind), "a fresh cast agrees");
    s.world.set_active(parent, true).unwrap();
    s.world.set_active(child, false).unwrap();
    s.world.set_parent(child, None).unwrap();
    s.world.insert(parent, Transform::from_position(Vec3::new(5.0, 0.0, 0.0))).unwrap();
    assert_eq!(hit(&mut caster, &s), Some(behind), "a loose inactive object isn't hit either");
    s.world.set_active(child, true).unwrap();
    assert_eq!(hit(&mut caster, &s), Some(child));
}

#[test]
fn a_ray_along_a_face_or_through_an_edge_still_finds_the_cube() {
    let mut s = scene();
    let cube = s.cube_at(Vec3::ZERO);
    // Along the plane of the top face: it enters through the front face's
    // top edge.
    let hit = s.cast(Vec3::new(0.0, 0.5, 5.0), Vec3::NEG_Z).unwrap();
    assert_eq!(hit.object, cube);
    assert!((hit.distance - 4.5).abs() < 1e-5);
    // Through the diagonal every face is cut along.
    assert_eq!(s.cast(Vec3::new(0.0, 0.0, 5.0), Vec3::NEG_Z).unwrap().distance, 4.5);
}

#[test]
fn a_raycaster_sees_changes_made_since_its_last_cast() {
    let mut s = scene();
    let cube = s.cube_at(Vec3::new(0.0, 0.0, -4.0));
    let parent = s.world.spawn();
    s.world.insert(parent, Transform::from_position(Vec3::ZERO)).unwrap();
    s.world.set_parent(cube, Some(parent)).unwrap();
    let mut caster = Raycaster::default();
    let ray = Ray::new(Vec3::ZERO, Vec3::NEG_Z).unwrap();
    assert_eq!(caster.cast(&s.world, ray, f32::INFINITY, u32::MAX).unwrap().distance, 3.5);

    s.world.insert(parent, Transform::from_position(Vec3::new(0.0, 0.0, -2.0))).unwrap();
    assert_eq!(caster.cast(&s.world, ray, f32::INFINITY, u32::MAX).unwrap().distance, 5.5, "the parent moved");
    s.world.set_parent(cube, None).unwrap();
    assert_eq!(caster.cast(&s.world, ray, f32::INFINITY, u32::MAX).unwrap().distance, 3.5, "unparented");
    s.world.despawn(cube).unwrap();
    assert!(caster.cast(&s.world, ray, f32::INFINITY, u32::MAX).is_none(), "despawned");
}

fn triangle(s: &mut Scene, positions: &[[f32; 3]; 3]) -> MeshId {
    s.world.add_mesh(MeshData::new(MeshParts { positions, indices: &[0, 1, 2], ..MeshParts::default() }).unwrap())
}

#[test]
fn a_ray_just_past_any_edge_of_a_triangle_misses() {
    let mut s = scene();
    let mesh = triangle(&mut s, &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
    let id = s.spawn(mesh, s.one_sided, Transform::from_position(Vec3::ZERO));
    let at = |x: f32, y: f32| s.cast(Vec3::new(x, y, 5.0), Vec3::NEG_Z).map(|h| h.object);
    assert_eq!(at(0.3, 0.3), Some(id));
    assert_eq!(at(0.49, 0.49), Some(id));
    for (x, y) in [(0.51, 0.51), (-0.01, 0.5), (0.5, -0.01)] {
        assert_eq!(at(x, y), None, "({x}, {y}) is outside");
    }
}

#[test]
fn a_sloped_face_keeps_its_normal_at_right_angles_under_an_uneven_scale() {
    let mut s = scene();
    // The plane x + y = 0 around the origin, double-sided so its winding
    // doesn't matter here.
    let mesh = triangle(&mut s, &[[2.0, -2.0, -1.0], [2.0, -2.0, 1.0], [-2.0, 2.0, 0.0]]);
    let t = Transform { position: Vec3::ZERO, rotation: Quat::IDENTITY, scale: Vec3::new(2.0, 1.0, 1.0) };
    let ramp = s.spawn(mesh, s.two_sided, t);
    // Stretched along X it's the plane x / 2 + y = 0, whose normal is
    // (1, 2, 0) / √5, not the stretched (2, 1, 0) / √5.
    let normal = Vec3::new(1.0, 2.0, 0.0).normalize();
    assert_hit(s.cast(Vec3::new(0.0, 5.0, 0.0), Vec3::NEG_Y), ramp, Vec3::ZERO, normal, 5.0);
}

#[test]
fn a_turned_object_is_hit_out_to_its_corners() {
    let mut s = scene();
    let t = Transform {
        position: Vec3::ZERO,
        rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_4),
        scale: Vec3::ONE,
    };
    let diamond = s.spawn(s.cube, s.one_sided, t);
    // Seen from above it's a diamond reaching 0.707 along X and Z. A ray
    // along X at z = 0.6 meets its slanted face near the corner.
    let hit = s.cast(Vec3::new(-5.0, 0.0, 0.6), Vec3::X).unwrap();
    assert_eq!(hit.object, diamond);
    let corner = std::f32::consts::FRAC_1_SQRT_2;
    assert!((hit.point.x - (0.6 - corner)).abs() < 1e-5, "{}", hit.point);
    assert!(s.cast(Vec3::new(-5.0, 0.0, 0.75), Vec3::X).is_none(), "past the corner");
}

#[test]
fn only_objects_on_the_masks_layers_are_hit() {
    let mut s = scene();
    let near = s.cube_at(Vec3::new(0.0, 0.0, -2.0));
    let far = s.cube_at(Vec3::new(0.0, 0.0, -6.0));
    s.world.set_layer(near, 5).unwrap();
    let ray = Ray::new(Vec3::ZERO, Vec3::NEG_Z).unwrap();
    let hit = |mask: u32| s.world.raycast(ray, f32::INFINITY, mask).map(|h| h.object);
    assert_eq!(hit(u32::MAX), Some(near));
    assert_eq!(hit(!(1 << 5)), Some(far), "layer 5 left out: the ray goes through it");
    assert_eq!(hit(1 << 5), Some(near));
    assert_eq!(hit(1), Some(far), "layer 0 only");
    assert_eq!(hit(0), None, "no layers");
}
