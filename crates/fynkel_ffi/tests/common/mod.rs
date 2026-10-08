//! Calling the C interface from Rust tests the way Dart calls it.
#![allow(dead_code, unreachable_pub)]

use std::ffi::CStr;

use fynkel_ffi::*;
use fynkel_world::{Quat, Transform, Vec3};

/// The message `fy_last_error` holds now.
pub fn last_error() -> String {
    // SAFETY: fy_last_error always points at a NUL-terminated string that
    // lives until the next failed call on this thread.
    unsafe { CStr::from_ptr(fy_last_error()) }.to_str().unwrap().to_owned()
}

/// Panics with the error message unless `status` is `Ok`.
#[track_caller]
pub fn ok(status: FyStatus) {
    assert_eq!(status, FyStatus::Ok, "{}", last_error());
}

/// A world that frees itself.
pub struct World(pub *mut FyWorld);

impl World {
    pub fn new() -> Self {
        Self(fy_world_new())
    }

    pub fn spawn(&self) -> u64 {
        let mut id = 0;
        // SAFETY: a live world and a local out-value.
        ok(unsafe { fy_spawn(self.0, &mut id) });
        id
    }
}

impl Drop for World {
    fn drop(&mut self) {
        // SAFETY: made by fy_world_new and freed only here.
        unsafe { fy_world_free(self.0) };
    }
}

pub fn transform(t: Transform) -> FyTransform {
    FyTransform { position: t.position.into(), rotation: t.rotation.into(), scale: t.scale.into() }
}

pub const fn srgb(r: f32, g: f32, b: f32) -> FyColor {
    FyColor { r, g, b, a: 1.0 }
}

pub fn argb(argb: u32) -> FyColor {
    let c = |shift: u32| ((argb >> shift) & 0xFF) as f32 / 255.0;
    FyColor { r: c(16), g: c(8), b: c(0), a: c(24) }
}

/// Adds a built-in shape to the world and returns its id, freeing the
/// handle again as `package:fynkel` would for a mesh it no longer needs.
pub fn add_shape(world: &World, shape: u32) -> u64 {
    let (mut mesh, mut id) = (std::ptr::null_mut(), 0);
    // SAFETY: a live world and pointers to locals.
    unsafe {
        ok(fy_mesh_new_shape(shape, &mut mesh));
        ok(fy_mesh_add(world.0, mesh, &mut id));
        fy_mesh_free(mesh);
    }
    id
}

/// The stage 1 scene, made only through `fy_*` calls. Returns the cube.
pub fn stage1(world: &World) -> u64 {
    let w = world.0;
    // SAFETY: a live world, and pointers to locals.
    unsafe {
        let camera = world.spawn();
        let fy_camera = FyCamera { fov_y: 60.0, near_clip: 0.1, far_clip: 1000.0, clear_color: srgb(0.1, 0.1, 0.12) };
        ok(fy_camera_set(w, camera, &fy_camera));
        let looking = Transform::from_position(Vec3::new(0.0, 1.5, 4.0)).looking_at(Vec3::ZERO);
        ok(fy_transform_set(w, camera, &transform(looking)));

        let sun = world.spawn();
        let light = FyLight { kind: FY_LIGHT_DIRECTIONAL, color: srgb(1.0, 1.0, 1.0), intensity: 1.0 };
        ok(fy_light_set(w, sun, &light));
        let facing = Transform::default().looking_at(Vec3::new(-0.4, -1.0, -0.3));
        ok(fy_transform_set(w, sun, &transform(facing)));

        let ambient = world.spawn();
        let light = FyLight { kind: FY_LIGHT_AMBIENT, color: srgb(1.0, 1.0, 1.0), intensity: 0.15 };
        ok(fy_light_set(w, ambient, &light));

        let (mesh, mut material) = (add_shape(world, FY_MESH_CUBE), 0);
        let orange =
            FyMaterial { base_color: argb(0xFFE0613A), metallic: 0.0, roughness: 0.5, ..FyMaterial::default() };
        ok(fy_material_add(w, &orange, &mut material));
        let cube = world.spawn();
        let turned = Transform { rotation: Quat::from_rotation_y(30f32.to_radians()), ..Transform::default() };
        ok(fy_transform_set(w, cube, &transform(turned)));
        ok(fy_mesh_renderer_set(w, cube, &FyMeshRenderer { mesh, material }));
        cube
    }
}

/// A headless view that frees itself.
pub struct View(pub *mut FyView);

impl View {
    pub fn headless(world: &World, width: u32, height: u32) -> Self {
        let mut view = std::ptr::null_mut();
        // SAFETY: a live world and a local out-value.
        ok(unsafe { fy_view_new_headless(world.0, width, height, &mut view) });
        Self(view)
    }

    pub fn frame(&self) -> u64 {
        let mut number = 0;
        // SAFETY: a live view and a local out-value.
        ok(unsafe { fy_view_frame(self.0, &mut number) });
        number
    }

    /// Reads frame `number` (or a later one) back.
    pub fn read(&self, number: u64, width: u32, height: u32) -> fynkel_golden::Image {
        let mut rgba = vec![0; (width * height * 4) as usize];
        let mut read = 0;
        // SAFETY: a live view, and a buffer of the length passed.
        ok(unsafe { fy_view_read(self.0, number, 10_000, rgba.as_mut_ptr(), rgba.len(), &mut read) });
        assert!(read >= number);
        fynkel_golden::Image { width, height, rgba }
    }
}

impl Drop for View {
    fn drop(&mut self) {
        // SAFETY: made by fy_view_new_headless and freed only here.
        unsafe { fy_view_free(self.0) };
    }
}
