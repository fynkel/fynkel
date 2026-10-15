//! `.glb` models through the C interface (stage 2 step 4), called the way
//! `package:fynkel` calls it: load, take the meshes, textures and
//! materials into a world, spawn. Each sample renders to a golden, and
//! spawning a model again uploads nothing new.
#![allow(unsafe_code)]

mod common;

use std::ptr;

use common::*;
use fynkel_ffi::*;
use fynkel_golden::assert_golden;
use fynkel_world::{Mat4, MeshRenderer, ModelData, Quat, Transform, Vec3};

/// A sample, built into the test so it also runs on a phone or simulator.
fn sample(name: &str) -> &'static [u8] {
    macro_rules! samples {
        ($($file:literal),*) => {
            match name {
                $($file => include_bytes!(concat!("../../../tests/assets/gltf/", $file)),)*
                other => panic!("no sample {other}"),
            }
        };
    }
    samples!(
        "Box.glb",
        "BoxTextured.glb",
        "NormalTangentMirrorTest.glb",
        "AlphaBlendModeTest.glb",
        "OrientationTest.glb",
        "CesiumMilkTruck.glb",
        "Fox.glb",
        "tower.glb"
    )
}

/// A model handle that frees itself.
struct Model(*mut FyModel);

impl Model {
    fn load(bytes: &[u8]) -> Result<Self, FyStatus> {
        let mut model = ptr::null_mut();
        // SAFETY: a slice and a pointer to a local.
        match unsafe { fy_model_load(bytes.as_ptr(), bytes.len(), &mut model) } {
            FyStatus::Ok => Ok(Self(model)),
            status => Err(status),
        }
    }

    fn counts(&self) -> FyModelCounts {
        let mut counts = FyModelCounts::default();
        // SAFETY: a live model and a pointer to a local.
        ok(unsafe { fy_model_counts(self.0, &mut counts) });
        counts
    }

    fn warnings(&self) -> Vec<String> {
        (0..self.counts().warnings)
            .map(|i| {
                let mut buf = [0u8; 256];
                let mut len = 0;
                // SAFETY: a live model, a buffer of the size passed, a local.
                ok(unsafe { fy_model_warning(self.0, i, buf.as_mut_ptr(), buf.len(), &mut len) });
                String::from_utf8(buf[..len].to_vec()).unwrap()
            })
            .collect()
    }

    /// What `package:fynkel`'s `World.spawnModel` does the first time:
    /// adds each mesh, texture and material, the materials' texture
    /// numbers mapped to the world's ids. Returns the mesh and material ids.
    fn add_to(&self, world: &World) -> (Vec<u64>, Vec<u64>) {
        let counts = self.counts();
        let w = world.0;
        // SAFETY: a live world and model, and pointers to locals.
        unsafe {
            let meshes = (0..counts.meshes)
                .map(|i| {
                    let (mut mesh, mut id) = (ptr::null_mut(), 0);
                    ok(fy_model_mesh(self.0, i, &mut mesh));
                    ok(fy_mesh_add(w, mesh, &mut id));
                    fy_mesh_free(mesh);
                    id
                })
                .collect();
            let textures: Vec<u64> = (0..counts.textures)
                .map(|i| {
                    let (mut texture, mut id) = (ptr::null_mut(), 0);
                    ok(fy_model_texture(self.0, i, &mut texture));
                    ok(fy_texture_add(w, texture, &mut id));
                    fy_texture_free(texture);
                    id
                })
                .collect();
            let world_id = |n: u64| if n == 0 { 0 } else { textures[n as usize - 1] };
            let materials = (0..counts.materials)
                .map(|i| {
                    let (mut m, mut id) = (FyMaterial::default(), 0);
                    ok(fy_model_material(self.0, i, &mut m));
                    m.base_color_texture = world_id(m.base_color_texture);
                    m.normal_texture = world_id(m.normal_texture);
                    m.metallic_roughness_texture = world_id(m.metallic_roughness_texture);
                    m.occlusion_texture = world_id(m.occlusion_texture);
                    m.emissive_texture = world_id(m.emissive_texture);
                    ok(fy_material_add(w, &m, &mut id));
                    id
                })
                .collect();
            (meshes, materials)
        }
    }

    fn spawn(&self, world: &World, (meshes, materials): &(Vec<u64>, Vec<u64>), at: Transform) -> Result<u64, FyStatus> {
        let mut root = 0;
        // SAFETY: a live world and model, slices with their lengths, locals.
        let status = unsafe {
            fy_model_spawn(
                world.0,
                self.0,
                meshes.as_ptr(),
                meshes.len(),
                materials.as_ptr(),
                materials.len(),
                &transform(at),
                &mut root,
            )
        };
        if status == FyStatus::Ok { Ok(root) } else { Err(status) }
    }
}

impl Drop for Model {
    fn drop(&mut self) {
        // SAFETY: made by fy_model_load, not used again.
        unsafe { fy_model_free(self.0) };
    }
}

/// The centre and radius of the sphere around everything the model draws
/// at rest, found by spawning it in a Rust world and walking its
/// vertices.
fn bounds(bytes: &[u8]) -> (Vec3, f32) {
    let model = ModelData::from_glb(bytes).unwrap();
    let mut world = fynkel_world::World::new();
    let ids = world.add_model(&model).unwrap();
    world.spawn_model(&model, &ids.meshes, &ids.materials, Transform::default()).unwrap();
    let (mut low, mut high) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    let mut drawn = Vec::new();
    world.for_each::<MeshRenderer>(|id, r| drawn.push((id, r.mesh.unwrap())));
    for (id, mesh) in drawn {
        let matrix: Mat4 = world.world_matrix(id).unwrap();
        for vertex in world.mesh(mesh).unwrap().vertices() {
            let p = matrix.transform_point3(Vec3::from_array(vertex.position));
            (low, high) = (low.min(p), high.max(p));
        }
    }
    ((low + high) / 2.0, (high - low).length() / 2.0)
}

/// The model spawned once, seen from `from` (a direction from its centre),
/// framed to fit its bounding sphere (`zoom` closer: a flat model fills
/// less of it), under a sun from over the camera's left shoulder.
fn render(name: &str, from: Vec3, zoom: f32) -> fynkel_golden::Image {
    render_at(name, from, zoom, FY_QUALITY_AUTO)
}

/// [`render`] at a quality `level`.
fn render_at(name: &str, from: Vec3, zoom: f32, level: u32) -> fynkel_golden::Image {
    let bytes = sample(name);
    let (centre, radius) = bounds(bytes);
    let world = World::new();
    let model = Model::load(bytes).unwrap();
    let ids = model.add_to(&world);
    model.spawn(&world, &ids, Transform::default()).unwrap();
    let w = world.0;
    const FOV: f32 = 40.0;
    // SAFETY: a live world and pointers to locals.
    unsafe {
        let camera = world.spawn();
        let far = radius * 8.0 + 10.0;
        let clear = srgb(0.16, 0.17, 0.2);
        ok(fy_camera_set(
            w,
            camera,
            &FyCamera {
                fov_y: FOV,
                near_clip: radius * 0.05,
                far_clip: far,
                clear_color: clear,
                orthographic: false,
                size: 5.0,
                culling_mask: u32::MAX,
                show_sky: true,
            },
        ));
        let distance = radius / (FOV.to_radians() / 2.0).sin() * 1.05 / zoom;
        let at = Transform::from_position(centre + from.normalize() * distance).looking_at(centre);
        ok(fy_transform_set(w, camera, &transform(at)));

        let sun = world.spawn();
        let white = srgb(1.0, 1.0, 1.0);
        ok(fy_light_set(
            w,
            sun,
            &FyLight { kind: FY_LIGHT_DIRECTIONAL, color: white, intensity: 1.0, ..Default::default() },
        ));
        let shoulder = at.rotation * Vec3::new(-0.5, 0.8, 0.6);
        ok(fy_transform_set(w, sun, &transform(Transform::default().looking_at(-shoulder))));
        let ambient = world.spawn();
        ok(fy_light_set(
            w,
            ambient,
            &FyLight { kind: FY_LIGHT_AMBIENT, color: white, intensity: 0.25, ..Default::default() },
        ));
    }
    // SAFETY: a live world.
    ok(unsafe { fy_world_quality_set(w, level) });
    let view = View::headless(&world, 256, 256);
    view.read(view.frame(), 256, 256)
}

#[test]
fn box_matches_its_golden() {
    assert_golden("gltf_box", &render("Box.glb", Vec3::new(1.0, 0.8, 1.4), 1.0));
}

#[test]
fn box_textured_matches_its_golden() {
    assert_golden("gltf_box_textured", &render("BoxTextured.glb", Vec3::new(1.0, 0.8, 1.4), 1.0));
}

/// Normal maps on mirrored UVs: every sphere must look lit from the same
/// side as its neighbours (the sample's reference picture).
#[test]
fn normal_tangent_mirror_test_matches_its_golden() {
    assert_golden("gltf_normal_tangent_mirror", &render("NormalTangentMirrorTest.glb", Vec3::Z, 1.4));
}

/// MASK cut-outs at three cutoffs; BLEND drawn opaque. The same at every
/// quality level: cut-out edges are where the scene image could differ.
#[test]
fn alpha_blend_mode_test_matches_its_golden() {
    for level in [FY_QUALITY_AUTO, FY_QUALITY_LOW, FY_QUALITY_MEDIUM, FY_QUALITY_HIGH] {
        let image = render_at("AlphaBlendModeTest.glb", Vec3::new(0.0, 0.3, 1.0), 1.1, level);
        assert_golden("gltf_alpha_blend_mode", &image);
    }
}

/// Each arrow must point at the target of its own colour.
#[test]
fn orientation_test_matches_its_golden() {
    assert_golden("gltf_orientation", &render("OrientationTest.glb", Vec3::new(1.0, 0.7, 1.2), 1.0));
}

/// Several primitives on one node, a texture shared by two materials, and
/// one wheel mesh drawn twice.
#[test]
fn cesium_milk_truck_matches_its_golden() {
    assert_golden("gltf_milk_truck", &render("CesiumMilkTruck.glb", Vec3::new(1.0, 0.6, 1.3), 1.0));
}

/// Skinned and animated, drawn in its bind pose.
#[test]
fn fox_matches_its_golden() {
    assert_golden("gltf_fox", &render("Fox.glb", Vec3::new(1.0, 0.5, 1.0), 1.0));
}

/// 138 nodes, a matrix-placed block, and an L-shape beside its mirror
/// image.
#[test]
fn tower_matches_its_golden() {
    assert_golden("gltf_tower", &render("tower.glb", Vec3::new(0.3, 0.5, 1.0), 1.0));
}

#[test]
fn a_model_reports_what_it_has_and_what_was_skipped() {
    let model = Model::load(sample("CesiumMilkTruck.glb")).unwrap();
    assert_eq!(model.counts(), FyModelCounts { meshes: 4, textures: 1, materials: 4, warnings: 1 });
    assert_eq!(model.warnings(), ["animations are skipped; the model shows its rest pose"]);
    let name = |i| {
        let mut buf = [0u8; 32];
        let mut len = 0;
        // SAFETY: a live model, a buffer of the size passed, a local.
        ok(unsafe { fy_model_material_name(model.0, i, buf.as_mut_ptr(), buf.len(), &mut len) });
        String::from_utf8(buf[..len].to_vec()).unwrap()
    };
    assert_eq!([name(0), name(1)], ["truck", "glass"]);
    // Textures are numbered from 1 within the model.
    let mut truck = FyMaterial::default();
    // SAFETY: a live model and a pointer to a local.
    ok(unsafe { fy_model_material(model.0, 0, &mut truck) });
    assert_eq!(truck.base_color_texture, 1);
    assert_eq!(truck.normal_texture, 0);

    let mut mesh = ptr::null_mut();
    // SAFETY: a live model and a pointer to a local.
    assert_eq!(unsafe { fy_model_mesh(model.0, 4, &mut mesh) }, FyStatus::InvalidValue);
    assert_eq!(last_error(), "the model has 4 meshes; there's no meshes 4");
}

#[test]
fn a_file_that_cant_be_read_is_refused_with_a_clear_message() {
    assert_eq!(Model::load(b"").err(), Some(FyStatus::InvalidValue));
    assert_eq!(last_error(), "the model is empty (0 bytes)");
    assert_eq!(Model::load(b"{\"asset\":{}}").err(), Some(FyStatus::InvalidValue));
    assert!(last_error().starts_with("this is a .gltf (JSON) file"), "{}", last_error());
    let box_glb = sample("Box.glb");
    assert_eq!(Model::load(&box_glb[..box_glb.len() - 40]).err(), Some(FyStatus::InvalidValue));
    assert!(last_error().starts_with("the .glb is broken"), "{}", last_error());

    let mut model = ptr::null_mut();
    // SAFETY: null bytes with a length, and a pointer to a local.
    assert_eq!(unsafe { fy_model_load(ptr::null(), 4, &mut model) }, FyStatus::NullPointer);
    // SAFETY: a slice, and null for the out.
    assert_eq!(unsafe { fy_model_load(box_glb.as_ptr(), box_glb.len(), ptr::null_mut()) }, FyStatus::NullPointer);
}

/// Spawning the truck a second time adds objects but no GPU data: the
/// view's mesh and texture bytes stay as they were.
#[test]
fn spawning_a_model_again_shares_its_gpu_data() {
    let world = World::new();
    stage1(&world);
    let model = Model::load(sample("CesiumMilkTruck.glb")).unwrap();
    let ids = model.add_to(&world);
    model.spawn(&world, &ids, Transform::from_position(Vec3::new(-3.0, 0.0, -6.0))).unwrap();
    let view = View::headless(&world, 64, 64);
    let stats = |view: &View| {
        let mut stats = FyViewStats::default();
        // SAFETY: a live view and a pointer to a local.
        ok(unsafe { fy_view_stats(view.0, &mut stats) });
        (stats.mesh_bytes, stats.texture_bytes)
    };
    view.read(view.frame(), 64, 64);
    let once = stats(&view);
    assert!(once.0 > 0 && once.1 > 0, "{once:?}");
    let turned =
        Transform { rotation: Quat::from_rotation_y(1.0), ..Transform::from_position(Vec3::new(3.0, 0.0, -6.0)) };
    let second = model.spawn(&world, &ids, turned).unwrap();
    view.read(view.frame(), 64, 64);
    assert_eq!(stats(&view), once);
    // SAFETY: a live world.
    ok(unsafe { fy_despawn(world.0, second) });
}

#[test]
fn a_spawn_with_the_wrong_ids_spawns_nothing() {
    let world = World::new();
    let model = Model::load(sample("Box.glb")).unwrap();
    let (meshes, materials) = model.add_to(&world);
    let before = world.spawn();
    assert_eq!(
        model.spawn(&world, &(Vec::new(), materials.clone()), Transform::default()),
        Err(FyStatus::InvalidValue)
    );
    assert_eq!(last_error(), "the model has 1 meshes, but 0 ids were given for them");
    assert_eq!(model.spawn(&world, &(meshes.clone(), vec![999]), Transform::default()), Err(FyStatus::NoMaterial));
    assert_eq!(model.spawn(&world, &(meshes, vec![0]), Transform::default()), Err(FyStatus::NoMaterial));
    // The failed spawns made objects partway and took them all away again.
    let after = world.spawn();
    assert!(after > before + 1, "the material was found missing partway through");
    for id in before + 1..after {
        // SAFETY: a live world.
        assert!(!unsafe { fy_exists(world.0, id) }, "object {id} was left behind");
    }
}

#[test]
fn a_ray_at_the_tower_hits_a_named_block_that_is_part_of_it() {
    let world = World::new();
    let model = Model::load(sample("tower.glb")).unwrap();
    let ids = model.add_to(&world);
    let tower = model.spawn(&world, &ids, Transform::from_position(Vec3::new(0.0, 0.0, -10.0))).unwrap();
    // Level with the bottom floor, at the block in front (+Z), which floor
    // 0 doesn't turn.
    let ray = FyRay { origin: FyVec3 { x: 0.0, y: 0.5, z: 0.0 }, direction: FyVec3 { x: 0.0, y: 0.0, z: -1.0 } };
    let mut hit = FyRayHit::default();
    let mut part = false;
    let mut name = [0u8; 64];
    let mut len = 0;
    // SAFETY: a live world and pointers to locals.
    unsafe {
        ok(fy_raycast(world.0, &ray, f32::INFINITY, u32::MAX, &mut hit));
        ok(fy_is_part_of(world.0, hit.object, tower, &mut part));
        ok(fy_name_get(world.0, hit.object, name.as_mut_ptr(), name.len(), &mut len));
    }
    assert!(part);
    assert_eq!(std::str::from_utf8(&name[..len]).unwrap(), "Block 0.3");
    // The block is turned to face the centre, so its 0.5 (X) is its depth
    // here; it stands 2.2 out from the centre at z = -10.
    assert!((hit.distance - (10.0 - 2.2 - 0.25)).abs() < 1e-4, "{}", hit.distance);
}
