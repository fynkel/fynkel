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
    s.world.set_active(parent, false).unwrap();
    assert!(caster.cast(&s.world, ray, f32::INFINITY, u32::MAX).is_none(), "its parent hidden");
    s.world.set_active(parent, true).unwrap();
    assert_eq!(caster.cast(&s.world, ray, f32::INFINITY, u32::MAX).unwrap().distance, 5.5, "shown again");
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

/// Every object from `root` down.
fn tree(world: &World, root: ObjectId) -> Vec<ObjectId> {
    let mut all = vec![root];
    for &child in world.children(root).unwrap() {
        all.extend(tree(world, child));
    }
    all
}

// Stage 14's WO3: a skinned mesh was hit where it was bound, not where its
// joints have bent it to and it's drawn.
#[test]
fn a_skinned_mesh_is_hit_where_its_joints_have_put_it() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/assets/gltf/Knight.glb");
    let model = ModelData::from_glb(&std::fs::read(path).unwrap()).unwrap();
    let mut world = World::new();
    let ids = world.add_model(&model).unwrap();
    let knight = world.spawn_model(&model, &ids.meshes, &ids.materials, Transform::default()).unwrap();
    let all = tree(&world, knight);
    let part = all.iter().copied().find(|&id| world.skin(id).is_some()).unwrap();
    let mesh = world.mesh(world.get::<MeshRenderer>(part).unwrap().mesh.unwrap()).unwrap();
    let (lo, hi) = mesh.bounds();
    let centre = world.world_matrix(part).unwrap().transform_point3((lo + hi) * 0.5);
    // Cast at it alone, on a layer of its own.
    world.set_layer(part, 7).unwrap();
    let down = |at: Vec3| Ray::new(at + Vec3::new(0.0, 0.0, 20.0), Vec3::NEG_Z).unwrap();
    let hit = |world: &World, at: Vec3| world.raycast(down(at), 100.0, 1 << 7).map(|h| h.object);
    assert_eq!(hit(&world, centre), Some(part), "at rest, where it was bound");
    // The skeleton moved 10 m along X: the part goes with it.
    let root = all.iter().copied().find(|&id| world.name(id).unwrap() == Some("root")).unwrap();
    let mut t = world.get::<Transform>(root).unwrap();
    t.position.x += 10.0;
    world.insert(root, t).unwrap();
    assert_eq!(hit(&world, centre), None, "where it was bound is empty");
    assert_eq!(hit(&world, centre + Vec3::new(10.0, 0.0, 0.0)), Some(part));
}

// Stage 14's WO3: every submesh was hit, drawn or not, and all from the
// sides the first material is seen from.
#[test]
fn only_the_submeshes_drawn_are_hit_each_from_its_own_sides() {
    // Two squares facing +Z, at z = 0 and z = -2, a submesh each.
    let positions = [
        [-1.0, -1.0, 0.0],
        [1.0, -1.0, 0.0],
        [1.0, 1.0, 0.0],
        [-1.0, 1.0, 0.0],
        [-1.0, -1.0, -2.0],
        [1.0, -1.0, -2.0],
        [1.0, 1.0, -2.0],
        [-1.0, 1.0, -2.0],
    ];
    let indices = [0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7];
    let parts = MeshParts { positions: &positions, indices: &indices, submeshes: &[6], ..MeshParts::default() };
    let two = |s: &mut Scene, first: MaterialId| {
        let mesh = s.world.add_mesh(MeshData::new(parts).unwrap());
        s.spawn(mesh, first, Transform::default())
    };
    let from_behind = |s: &Scene| s.cast(Vec3::new(0.0, 0.0, -5.0), Vec3::Z).map(|h| h.point.z);
    // The first double-sided, the second with no material: it isn't
    // drawn, so it's passed through and the first's back is hit.
    let mut s = scene();
    let first = s.two_sided;
    let object = two(&mut s, first);
    assert_eq!(from_behind(&s), Some(0.0));
    // The second drawn one-sided: from behind, still passed through.
    s.world.set_submesh_materials(object, &[s.one_sided]).unwrap();
    assert_eq!(from_behind(&s), Some(0.0));
    // The first one-sided, the second double-sided: its back is hit.
    let mut s = scene();
    let first = s.one_sided;
    let object = two(&mut s, first);
    s.world.set_submesh_materials(object, &[s.two_sided]).unwrap();
    assert_eq!(from_behind(&s), Some(-2.0));
}

/// The closest hit along `ray` on `mesh` placed by `model`, triangle by
/// triangle in the world: what a cast through the mesh's tree must find.
/// `sides` is each submesh's: not drawn, or drawn and whether double-sided.
fn one_by_one(mesh: &MeshData, model: Mat4, sides: &[Option<bool>], ray: Ray) -> Option<f32> {
    let (positions, indices) = (mesh.vertices(), mesh.indices());
    let mut best: Option<f32> = None;
    for (range, side) in mesh.submeshes().iter().zip(sides) {
        let Some(double_sided) = *side else { continue };
        for t in indices[range.start as usize..range.end as usize].chunks_exact(3) {
            let [a, b, c] = [0, 1, 2].map(|i| model.transform_point3(Vec3::from(positions[t[i] as usize].position)));
            let (ab, ac) = (b - a, c - a);
            let p = ray.direction().cross(ac);
            let det = ab.dot(p);
            if det.abs() < 1e-12 || (det < 0.0 && !double_sided) {
                continue;
            }
            let s = ray.origin() - a;
            let u = s.dot(p) / det;
            let q = s.cross(ab);
            let v = ray.direction().dot(q) / det;
            let d = ac.dot(q) / det;
            if (0.0..=1.0).contains(&u) && v >= 0.0 && u + v <= 1.0 && d >= 0.0 && best.is_none_or(|b| d < b) {
                best = Some(d);
            }
        }
    }
    best
}

/// Rays from all round at the middle of things, spread by a fixed rule.
fn rays(count: u32) -> impl Iterator<Item = Ray> {
    let mut seed = 0x2545_f491_u32;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32 * 2.0 - 1.0
    };
    (0..count).map(move |_| {
        let from = Vec3::new(next(), next(), next()).normalize_or(Vec3::Y) * 6.0;
        let to = Vec3::new(next(), next(), next()) * 1.5;
        Ray::new(from, to - from).unwrap()
    })
}

#[track_caller]
fn same_as_one_by_one(s: &Scene, mesh: MeshId, model: Mat4, sides: &[Option<bool>]) {
    let data = s.world.mesh(mesh).unwrap();
    assert!(data.indices().len() / 3 >= 64, "big enough to have a tree");
    let mut caster = Raycaster::default();
    let (mut hits, mut wrong) = (0, Vec::new());
    for ray in rays(2000) {
        let cast = caster.cast(&s.world, ray, f32::INFINITY, u32::MAX).map(|h| h.distance);
        let want = one_by_one(data, model, sides, ray);
        hits += usize::from(want.is_some());
        let same = match (cast, want) {
            (Some(a), Some(b)) => (a - b).abs() < 1e-3 * b.max(1.0),
            (a, b) => a.is_none() && b.is_none(),
        };
        if !same {
            wrong.push((ray, cast, want));
        }
    }
    // A ray through an edge may be a hit one way and a miss the other.
    assert!(wrong.len() <= 2, "{} of 2000 differ: {:?}", wrong.len(), &wrong[..wrong.len().min(5)]);
    assert!(hits > 200, "{hits} hits");
}

#[test]
fn a_big_mesh_is_hit_through_its_tree_where_each_triangle_would_be() {
    // A sphere turned and stretched: one submesh, one-sided.
    let mut s = scene();
    let t = Transform {
        position: Vec3::new(0.2, -0.1, 0.3),
        rotation: Quat::from_euler(glam::EulerRot::XYZ, 0.4, 1.1, -0.3),
        scale: Vec3::new(2.0, 1.0, 1.5),
    };
    let (sphere, material) = (s.sphere, s.one_sided);
    s.spawn(sphere, material, t);
    same_as_one_by_one(&s, sphere, t.matrix(), &[Some(false)]);

    // A wavy sheet of 1800 triangles in two submeshes, the halves.
    let n = 30;
    let positions: Vec<[f32; 3]> = (0..=n)
        .flat_map(|z| {
            (0..=n).map(move |x| {
                let (x, z) = (x as f32 / n as f32 * 3.0 - 1.5, z as f32 / n as f32 * 3.0 - 1.5);
                [x, (x * 2.0).sin() * 0.4 + (z * 3.0).cos() * 0.3, z]
            })
        })
        .collect();
    let row = n + 1;
    let indices: Vec<u32> = (0..n)
        .flat_map(|z| {
            (0..n).flat_map(move |x| {
                let a = z * row + x;
                [a, a + row, a + 1, a + 1, a + row, a + row + 1]
            })
        })
        .collect();
    let half = [n * n * 3];
    let sheet = || {
        MeshData::new(MeshParts { positions: &positions, indices: &indices, submeshes: &half, ..MeshParts::default() })
            .unwrap()
    };
    let t = Transform::from_position(Vec3::new(0.0, 0.1, 0.0));
    // The second half not drawn; then drawn double-sided.
    let mut s = scene();
    let mesh = s.world.add_mesh(sheet());
    let one_sided = s.one_sided;
    let object = s.spawn(mesh, one_sided, t);
    same_as_one_by_one(&s, mesh, t.matrix(), &[Some(false), None]);
    let two_sided = s.two_sided;
    s.world.set_submesh_materials(object, &[two_sided]).unwrap();
    same_as_one_by_one(&s, mesh, t.matrix(), &[Some(false), Some(true)]);
}

#[test]
fn a_changed_mesh_is_hit_where_it_is_now() {
    // The tree made for the first sphere isn't used for the second.
    let mut s = scene();
    let mesh = s.world.add_mesh(MeshData::sphere_sized(1.0, 32, 16));
    let material = s.one_sided;
    s.spawn(mesh, material, Transform::default());
    let mut caster = Raycaster::default();
    let ray = Ray::new(Vec3::new(0.0, 0.0, 5.0), Vec3::NEG_Z).unwrap();
    let at = |caster: &mut Raycaster, s: &Scene| caster.cast(&s.world, ray, f32::INFINITY, u32::MAX).unwrap().distance;
    // Enough casts at each that each gets its tree.
    for _ in 0..20 {
        assert!((at(&mut caster, &s) - 4.0).abs() < 1e-3);
    }
    s.world.set_mesh(mesh, MeshData::sphere_sized(2.0, 32, 16)).unwrap();
    for _ in 0..20 {
        assert!((at(&mut caster, &s) - 3.0).abs() < 1e-3);
    }
}

/// A world of `count` cubes, some on layer 1, some double-sided, two at
/// the same place (a tie: the first in the world's order is hit).
fn crowd(count: u32) -> (Scene, Vec<ObjectId>) {
    let mut s = scene();
    let mut ids = Vec::new();
    for i in 0..count {
        let position =
            Vec3::new((i % 10) as f32 * 1.5 - 7.0, (i / 10 % 10) as f32 * 1.5 - 7.0, -((i / 100) as f32) * 1.5);
        let material = if i % 7 == 0 { s.two_sided } else { s.one_sided };
        let mesh = if i % 11 == 0 { s.sphere } else { s.cube };
        let id = s.spawn(mesh, material, Transform::from_position(position));
        if i % 5 == 0 {
            s.world.set_layer(id, 1).unwrap();
        }
        ids.push(id);
    }
    let first = ids[44];
    let at = s.world.get::<Transform>(first).unwrap();
    ids.push(s.cube_at(at.position));
    (s, ids)
}

/// Rays from in front of the crowd, at it.
fn at_the_crowd(count: u32) -> Vec<Ray> {
    rays(count)
        .map(|r| {
            Ray::new(r.origin() * 3.0 + Vec3::new(0.0, 0.0, 12.0), r.direction() + Vec3::new(0.0, 0.0, -1.0)).unwrap()
        })
        .collect()
}

/// A caster that has kept its tree hits what a new one, testing every
/// object, hits: the same object, at the same point.
#[track_caller]
fn same_as_every_object(caster: &mut Raycaster, s: &Scene, rays: &[Ray]) -> usize {
    let mut hits = 0;
    for &ray in rays {
        for mask in [u32::MAX, 1, 2] {
            let kept = caster.cast(&s.world, ray, f32::INFINITY, mask);
            let fresh = s.world.raycast(ray, f32::INFINITY, mask);
            assert_eq!(kept, fresh, "{ray:?}, mask {mask}");
            hits += usize::from(fresh.is_some());
        }
    }
    hits
}

#[test]
fn a_kept_caster_hits_what_testing_every_object_hits_as_the_world_changes() {
    let (mut s, ids) = crowd(300);
    let rays = at_the_crowd(200);
    let mut caster = Raycaster::default();
    // The first casts go one by one, the rest through the tree.
    assert!(same_as_every_object(&mut caster, &s, &rays) > 200);
    // The tie: straight at the two cubes in one place.
    let at = s.world.get::<Transform>(ids[44]).unwrap().position;
    let ray = Ray::new(at + Vec3::new(0.0, 0.0, 20.0), Vec3::NEG_Z).unwrap();
    let hit = caster.cast(&s.world, ray, f32::INFINITY, u32::MAX).unwrap();
    assert_eq!(hit, s.world.raycast(ray, f32::INFINITY, u32::MAX).unwrap());
    // Moved: some a little, some far, so the tree is refitted and then
    // made again.
    for (step, far) in [(0.3, false), (0.6, false), (8.0, true)] {
        for (i, &id) in ids.iter().enumerate().step_by(3) {
            let mut t = s.world.get::<Transform>(id).unwrap();
            t.position += Vec3::new(step, if far { (i % 4) as f32 * step } else { 0.0 }, 0.0);
            s.world.insert(id, t).unwrap();
        }
        same_as_every_object(&mut caster, &s, &rays);
    }
    // A few at a time, which the caster follows by what moved: one moved,
    // a parent with its child, a parent turned off and on again.
    s.world.set_parent(ids[50], Some(ids[51])).unwrap();
    same_as_every_object(&mut caster, &s, &rays);
    for k in 0..3 {
        for (id, by) in [(ids[51], Vec3::new(0.0, 0.4, 0.0)), (ids[60 + k], Vec3::new(0.5, 0.0, 0.0))] {
            let mut t = s.world.get::<Transform>(id).unwrap();
            t.position += by;
            s.world.insert(id, t).unwrap();
        }
        same_as_every_object(&mut caster, &s, &rays);
    }
    for on in [false, true] {
        s.world.set_active(ids[51], on).unwrap();
        same_as_every_object(&mut caster, &s, &rays);
    }
    // Turned off, given another mesh, removed, despawned, added.
    s.world.set_active(ids[10], false).unwrap();
    s.world
        .insert(ids[20], MeshRenderer { mesh: Some(s.sphere), material: Some(s.one_sided), ..Default::default() })
        .unwrap();
    s.world.remove::<MeshRenderer>(ids[30]).unwrap();
    s.world.despawn(ids[40]).unwrap();
    s.cube_at(Vec3::new(0.0, 0.0, 3.0));
    same_as_every_object(&mut caster, &s, &rays);
    // A mesh changed in place: every object drawn with it is bigger.
    s.world.set_mesh(s.cube, MeshData::box_shape(Vec3::splat(1.4), [1, 1, 1], 1.0).unwrap()).unwrap();
    same_as_every_object(&mut caster, &s, &rays);
}
