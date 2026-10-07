//! The C functions (`fy_*`) and the plain types they pass (stage 1 plan
//! §3.4). `include/fynkel.h` is generated from this file by cbindgen.
//!
//! Every function that can fail returns an [`FyStatus`] and writes its
//! results through out-pointers. Ids are `uint64_t`, with 0 meaning "none".
//! Colours are sRGB, 0–1 per channel. Angles are degrees.
//!
//! Handles: an `FyWorld` lives until `fy_world_free`; an `FyView` until
//! `fy_view_free`. A view keeps its world's memory alive, so they can be
//! freed in either order. Passing a handle after freeing it is undefined
//! behaviour; `package:fynkel` makes that impossible from Dart.

use std::ffi::c_char;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use fynkel_present::{Headless, HeadlessFrames, ReadError};
use fynkel_render::{Gpu, Renderer};
use fynkel_world::{
    Camera, Color, Component, Light, LightKind, Material, MaterialId, MeshId, MeshRenderer, MeshSource, ObjectId, Quat,
    Transform, Vec3, World,
};

use crate::error::{Error, FyStatus, guard, last_error};
use crate::extract::{Extract, MeshOp};
use crate::worker::Worker;

/// Bumped on any change to the functions or types in this file.
pub const FY_ABI_VERSION: u32 = 1;

// --- Plain types ---------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyVec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// A rotation, as a unit quaternion.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyQuat {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

/// sRGB, 0–1 per channel.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyColor {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyTransform {
    pub position: FyVec3,
    pub rotation: FyQuat,
    pub scale: FyVec3,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyMeshRenderer {
    /// 0 for none.
    pub mesh: u64,
    /// 0 for none.
    pub material: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyCamera {
    /// Vertical field of view in degrees, 1–179.
    pub fov_y: f32,
    /// Not `near`: Windows headers define `near` and `far` as macros.
    pub near_clip: f32,
    pub far_clip: f32,
    pub clear_color: FyColor,
}

/// `FyLight::kind`.
pub const FY_LIGHT_DIRECTIONAL: u32 = 0;
/// `FyLight::kind`.
pub const FY_LIGHT_AMBIENT: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyLight {
    /// `FY_LIGHT_DIRECTIONAL` or `FY_LIGHT_AMBIENT`.
    pub kind: u32,
    pub color: FyColor,
    pub intensity: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyMaterial {
    pub base_color: FyColor,
    /// 0–1.
    pub metallic: f32,
    /// 0–1.
    pub roughness: f32,
}

/// `fy_mesh_add`'s shape: a 1×1×1 cube.
pub const FY_MESH_CUBE: u32 = 0;
/// `fy_mesh_add`'s shape: a sphere of radius 0.5.
pub const FY_MESH_SPHERE: u32 = 1;
/// `fy_mesh_add`'s shape: a 1×1 plane facing +Y.
pub const FY_MESH_PLANE: u32 = 2;

// --- Conversions ---------------------------------------------------------------

impl From<FyVec3> for Vec3 {
    fn from(v: FyVec3) -> Self {
        Vec3::new(v.x, v.y, v.z)
    }
}

impl From<Vec3> for FyVec3 {
    fn from(v: Vec3) -> Self {
        Self { x: v.x, y: v.y, z: v.z }
    }
}

impl From<FyQuat> for Quat {
    fn from(q: FyQuat) -> Self {
        Quat::from_xyzw(q.x, q.y, q.z, q.w)
    }
}

impl From<Quat> for FyQuat {
    fn from(q: Quat) -> Self {
        Self { x: q.x, y: q.y, z: q.z, w: q.w }
    }
}

impl From<FyColor> for Color {
    fn from(c: FyColor) -> Self {
        Color::rgba(c.r, c.g, c.b, c.a)
    }
}

impl From<Color> for FyColor {
    fn from(c: Color) -> Self {
        Self { r: c.r, g: c.g, b: c.b, a: c.a }
    }
}

/// A C struct that stands for a component.
trait CComponent: Copy {
    type Component: Component;
    fn to_component(self) -> Result<Self::Component, Error>;
    fn from_component(c: Self::Component) -> Self;
}

impl CComponent for FyTransform {
    type Component = Transform;
    fn to_component(self) -> Result<Transform, Error> {
        Ok(Transform { position: self.position.into(), rotation: self.rotation.into(), scale: self.scale.into() })
    }
    fn from_component(t: Transform) -> Self {
        Self { position: t.position.into(), rotation: t.rotation.into(), scale: t.scale.into() }
    }
}

impl CComponent for FyMeshRenderer {
    type Component = MeshRenderer;
    fn to_component(self) -> Result<MeshRenderer, Error> {
        Ok(MeshRenderer { mesh: MeshId::from_raw(self.mesh), material: MaterialId::from_raw(self.material) })
    }
    fn from_component(r: MeshRenderer) -> Self {
        Self { mesh: r.mesh.map_or(0, MeshId::raw), material: r.material.map_or(0, MaterialId::raw) }
    }
}

impl CComponent for FyCamera {
    type Component = Camera;
    fn to_component(self) -> Result<Camera, Error> {
        Ok(Camera { fov_y: self.fov_y, near: self.near_clip, far: self.far_clip, clear_color: self.clear_color.into() })
    }
    fn from_component(c: Camera) -> Self {
        Self { fov_y: c.fov_y, near_clip: c.near, far_clip: c.far, clear_color: c.clear_color.into() }
    }
}

impl CComponent for FyLight {
    type Component = Light;
    fn to_component(self) -> Result<Light, Error> {
        let kind = match self.kind {
            FY_LIGHT_DIRECTIONAL => LightKind::Directional,
            FY_LIGHT_AMBIENT => LightKind::Ambient,
            other => {
                return Err(Error::new(
                    FyStatus::InvalidValue,
                    format!("Light.kind {other} doesn't exist (0 = directional, 1 = ambient)"),
                ));
            }
        };
        Ok(Light { kind, color: self.color.into(), intensity: self.intensity })
    }
    fn from_component(l: Light) -> Self {
        let kind = match l.kind {
            LightKind::Directional => FY_LIGHT_DIRECTIONAL,
            LightKind::Ambient => FY_LIGHT_AMBIENT,
        };
        Self { kind, color: l.color.into(), intensity: l.intensity }
    }
}

impl From<FyMaterial> for Material {
    fn from(m: FyMaterial) -> Self {
        Material { base_color: m.base_color.into(), metallic: m.metallic, roughness: m.roughness }
    }
}

impl From<Material> for FyMaterial {
    fn from(m: Material) -> Self {
        Self { base_color: m.base_color.into(), metallic: m.metallic, roughness: m.roughness }
    }
}

// --- Handles -----------------------------------------------------------------

/// A world. Opaque to C.
#[derive(Debug)]
pub struct FyWorld {
    world: Mutex<World>,
}

impl FyWorld {
    /// The world, unless an earlier call panicked while changing it.
    fn lock(&self) -> Result<MutexGuard<'_, World>, Error> {
        self.world.lock().map_err(|_| {
            Error::new(
                FyStatus::Poisoned,
                "an earlier call panicked while changing this world; it can't be used any more",
            )
        })
    }
}

/// A view: a world drawn into a target by its own render worker. Opaque to
/// C.
#[derive(Debug)]
pub struct FyView {
    world: Arc<FyWorld>,
    /// The frame clock's side: the extract and the frame numbers.
    clock: Mutex<Clock>,
    worker: Worker,
    /// Headless views only.
    frames: Option<HeadlessFrames>,
}

#[derive(Debug, Default)]
struct Clock {
    extract: Extract,
    last_number: u64,
    ops: Vec<MeshOp>,
}

/// `p` as a reference, or an error when it's null.
///
/// # Safety
///
/// `p` is null or points at a live `T`.
unsafe fn handle<'a, T>(p: *const T, what: &str) -> Result<&'a T, Error> {
    // SAFETY: the caller promises p is null or live.
    unsafe { p.as_ref() }.ok_or_else(|| Error::null(what))
}

/// Writes `value` through `out`, or fails when it's null.
///
/// # Safety
///
/// `out` is null or valid for a write of `T`.
unsafe fn write<T>(out: *mut T, value: T, what: &str) -> Result<(), Error> {
    if out.is_null() {
        return Err(Error::null(what));
    }
    // SAFETY: not null, and the caller promises it's valid for writes.
    unsafe { out.write(value) };
    Ok(())
}

/// Reads `*p`, or fails when it's null.
///
/// # Safety
///
/// `p` is null or valid for a read of `T`.
unsafe fn read<T: Copy>(p: *const T, what: &str) -> Result<T, Error> {
    // SAFETY: the caller promises p is null or valid.
    unsafe { p.as_ref() }.copied().ok_or_else(|| Error::null(what))
}

fn object(id: u64) -> Result<ObjectId, Error> {
    ObjectId::from_raw(id).ok_or_else(|| Error::new(FyStatus::NoObject, "0 is not an object id"))
}

fn mesh(id: u64) -> Result<MeshId, Error> {
    MeshId::from_raw(id).ok_or_else(|| Error::new(FyStatus::NoMesh, "0 is not a mesh id"))
}

fn material(id: u64) -> Result<MaterialId, Error> {
    MaterialId::from_raw(id).ok_or_else(|| Error::new(FyStatus::NoMaterial, "0 is not a material id"))
}

// --- Library -----------------------------------------------------------------

/// [`FY_ABI_VERSION`]. `package:fynkel` checks it when it loads.
#[unsafe(no_mangle)]
pub extern "C" fn fy_abi_version() -> u32 {
    FY_ABI_VERSION
}

/// The message of the last failed call on this thread, as UTF-8 ("" before
/// any). Valid until the next failed call on this thread.
#[unsafe(no_mangle)]
pub extern "C" fn fy_last_error() -> *const c_char {
    last_error()
}

// --- Worlds and objects ------------------------------------------------------

/// A new, empty world. Free it with `fy_world_free`.
#[unsafe(no_mangle)]
pub extern "C" fn fy_world_new() -> *mut FyWorld {
    Arc::into_raw(Arc::new(FyWorld { world: Mutex::new(World::new()) })).cast_mut()
}

/// Frees a world. Views of it keep working until they are freed too. Null
/// is ignored.
///
/// # Safety
///
/// `world` is null or a live world, not used again after this.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_world_free(world: *mut FyWorld) {
    if !world.is_null() {
        // SAFETY: made by Arc::into_raw in fy_world_new; the caller gives
        // up its reference.
        drop(unsafe { Arc::from_raw(world) });
    }
}

/// Spawns an object with no components and writes its id to `out`.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_spawn(world: *const FyWorld, out: *mut u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let id = world.lock()?.spawn();
        // SAFETY: per this function's contract.
        unsafe { write(out, id.raw(), "out") }
    })
}

/// Despawns an object. Its id is never used again.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_despawn(world: *const FyWorld, id: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        Ok(world.lock()?.despawn(object(id)?)?)
    })
}

/// Whether the object exists. False for a null world.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_exists(world: *const FyWorld, id: u64) -> bool {
    let mut exists = false;
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        exists = ObjectId::from_raw(id).is_some_and(|id| world.lock().is_ok_and(|w| w.contains(id)));
        Ok(())
    });
    exists
}

// --- Components --------------------------------------------------------------

/// # Safety
///
/// `world` is null or live; `value` is null or readable.
unsafe fn set<C: CComponent>(world: *const FyWorld, id: u64, value: *const C) -> FyStatus {
    guard(|| {
        // SAFETY: per the caller's contract.
        let (world, value) = unsafe { (handle(world, "world")?, read(value, "value")?) };
        let component = value.to_component()?;
        Ok(world.lock()?.insert(object(id)?, component)?)
    })
}

/// # Safety
///
/// `world` is null or live; `out` is null or writable.
unsafe fn get<C: CComponent>(world: *const FyWorld, id: u64, out: *mut C) -> FyStatus {
    guard(|| {
        // SAFETY: per the caller's contract.
        let world = unsafe { handle(world, "world") }?;
        let component = world.lock()?.get::<C::Component>(object(id)?)?;
        // SAFETY: per the caller's contract.
        unsafe { write(out, C::from_component(component), "out") }
    })
}

/// # Safety
///
/// `world` is null or live.
unsafe fn remove<C: CComponent>(world: *const FyWorld, id: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per the caller's contract.
        let world = unsafe { handle(world, "world") }?;
        world.lock()?.remove::<C::Component>(object(id)?)?;
        Ok(())
    })
}

/// Adds or replaces the object's `Transform`. The rotation must be a unit
/// quaternion and every value finite.
///
/// # Safety
///
/// `world` is null or live; `value` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_transform_set(world: *const FyWorld, id: u64, value: *const FyTransform) -> FyStatus {
    // SAFETY: the same contract.
    unsafe { set(world, id, value) }
}

/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_transform_get(world: *const FyWorld, id: u64, out: *mut FyTransform) -> FyStatus {
    // SAFETY: the same contract.
    unsafe { get(world, id, out) }
}

/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_transform_remove(world: *const FyWorld, id: u64) -> FyStatus {
    // SAFETY: the same contract.
    unsafe { remove::<FyTransform>(world, id) }
}

/// Adds or replaces the object's `MeshRenderer`. Its mesh and material must
/// exist (or be 0).
///
/// # Safety
///
/// `world` is null or live; `value` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_renderer_set(
    world: *const FyWorld,
    id: u64,
    value: *const FyMeshRenderer,
) -> FyStatus {
    // SAFETY: the same contract.
    unsafe { set(world, id, value) }
}

/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_renderer_get(world: *const FyWorld, id: u64, out: *mut FyMeshRenderer) -> FyStatus {
    // SAFETY: the same contract.
    unsafe { get(world, id, out) }
}

/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_renderer_remove(world: *const FyWorld, id: u64) -> FyStatus {
    // SAFETY: the same contract.
    unsafe { remove::<FyMeshRenderer>(world, id) }
}

/// Adds or replaces the object's `Camera`.
///
/// # Safety
///
/// `world` is null or live; `value` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_camera_set(world: *const FyWorld, id: u64, value: *const FyCamera) -> FyStatus {
    // SAFETY: the same contract.
    unsafe { set(world, id, value) }
}

/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_camera_get(world: *const FyWorld, id: u64, out: *mut FyCamera) -> FyStatus {
    // SAFETY: the same contract.
    unsafe { get(world, id, out) }
}

/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_camera_remove(world: *const FyWorld, id: u64) -> FyStatus {
    // SAFETY: the same contract.
    unsafe { remove::<FyCamera>(world, id) }
}

/// Adds or replaces the object's `Light`.
///
/// # Safety
///
/// `world` is null or live; `value` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_light_set(world: *const FyWorld, id: u64, value: *const FyLight) -> FyStatus {
    // SAFETY: the same contract.
    unsafe { set(world, id, value) }
}

/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_light_get(world: *const FyWorld, id: u64, out: *mut FyLight) -> FyStatus {
    // SAFETY: the same contract.
    unsafe { get(world, id, out) }
}

/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_light_remove(world: *const FyWorld, id: u64) -> FyStatus {
    // SAFETY: the same contract.
    unsafe { remove::<FyLight>(world, id) }
}

// --- Meshes and materials ----------------------------------------------------

/// Adds a built-in mesh (`FY_MESH_CUBE`, `FY_MESH_SPHERE` or
/// `FY_MESH_PLANE`) and writes its id to `out`.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_add(world: *const FyWorld, shape: u32, out: *mut u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let source = match shape {
            FY_MESH_CUBE => MeshSource::Cube,
            FY_MESH_SPHERE => MeshSource::Sphere,
            FY_MESH_PLANE => MeshSource::Plane,
            other => {
                return Err(Error::new(
                    FyStatus::InvalidValue,
                    format!("mesh shape {other} doesn't exist (0 = cube, 1 = sphere, 2 = plane)"),
                ));
            }
        };
        let id = world.lock()?.add_mesh(source);
        // SAFETY: per this function's contract.
        unsafe { write(out, id.raw(), "out") }
    })
}

/// Removes a mesh. Refused while an object uses it.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_remove(world: *const FyWorld, id: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        Ok(world.lock()?.remove_mesh(mesh(id)?)?)
    })
}

/// Adds a material and writes its id to `out`.
///
/// # Safety
///
/// `world` is null or live; `value` is null or readable; `out` is null or
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_material_add(world: *const FyWorld, value: *const FyMaterial, out: *mut u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, value) = unsafe { (handle(world, "world")?, read(value, "value")?) };
        let id = world.lock()?.add_material(value.into())?;
        // SAFETY: per this function's contract.
        unsafe { write(out, id.raw(), "out") }
    })
}

/// Changes a material. Every object using it changes with it.
///
/// # Safety
///
/// `world` is null or live; `value` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_material_set(world: *const FyWorld, id: u64, value: *const FyMaterial) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, value) = unsafe { (handle(world, "world")?, read(value, "value")?) };
        Ok(world.lock()?.set_material(material(id)?, value.into())?)
    })
}

/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_material_get(world: *const FyWorld, id: u64, out: *mut FyMaterial) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let value = world.lock()?.material(material(id)?)?;
        // SAFETY: per this function's contract.
        unsafe { write(out, value.into(), "out") }
    })
}

/// Removes a material. Refused while an object uses it.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_material_remove(world: *const FyWorld, id: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        Ok(world.lock()?.remove_material(material(id)?)?)
    })
}

// --- Views -------------------------------------------------------------------

/// A view of `world` that draws `width` × `height` frames offscreen and
/// reads them back (`fy_view_read`). Starts its render worker. Free it with
/// `fy_view_free`.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_new_headless(
    world: *const FyWorld,
    width: u32,
    height: u32,
    out: *mut *mut FyView,
) -> FyStatus {
    guard(|| {
        if out.is_null() {
            return Err(Error::null("out"));
        }
        if world.is_null() {
            return Err(Error::null("world"));
        }
        // SAFETY: not null, and made by fy_world_new (Arc::into_raw); this
        // adds the view's own reference.
        let world = unsafe {
            Arc::increment_strong_count(world);
            Arc::from_raw(world)
        };
        if width == 0 || height == 0 {
            return Err(Error::new(FyStatus::InvalidValue, format!("a view can't be {width} × {height}")));
        }
        let gpu = Gpu::headless().map_err(|e| Error::new(FyStatus::NoGpu, e.to_string()))?;
        let (target, frames) = Headless::new(&gpu.device, &gpu.queue, width, height);
        let renderer = Renderer::new(&gpu, fynkel_present::Target::format(&target));
        let worker = Worker::start(gpu, renderer, target)
            .map_err(|e| Error::new(FyStatus::WorkerFailed, format!("couldn't start the render worker: {e}")))?;
        let view = FyView { world, clock: Mutex::default(), worker, frames: Some(frames) };
        // SAFETY: checked non-null above; the caller promises it's writable.
        unsafe { out.write(Box::into_raw(Box::new(view))) };
        Ok(())
    })
}

/// Stops a view's render worker (after the frame it's drawing) and frees
/// the view. Null is ignored.
///
/// # Safety
///
/// `view` is null or a live view, not used again after this, and no other
/// thread is inside a call on it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_free(view: *mut FyView) {
    if !view.is_null() {
        // SAFETY: made by Box::into_raw in fy_view_new_headless; the caller
        // gives it up.
        drop(unsafe { Box::from_raw(view) });
    }
}

/// One frame: copies the world into a scene and hands it to the render
/// worker, without waiting for the GPU. Writes the frame's number (1, 2, …)
/// to `out_number`.
///
/// # Safety
///
/// `view` is null or live; `out_number` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_frame(view: *const FyView, out_number: *mut u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let view = unsafe { handle(view, "view") }?;
        if out_number.is_null() {
            return Err(Error::null("out_number"));
        }
        let mut clock = view.clock.lock().map_err(|_| Error::new(FyStatus::Poisoned, "the view's clock panicked"))?;
        let clock = &mut *clock;
        let mut scene = view.worker.scene();
        clock.extract.run(&*view.world.lock()?, &mut scene, &mut clock.ops);
        let number = clock.last_number + 1;
        view.worker.post(number, scene, &mut clock.ops).map_err(|e| Error::new(FyStatus::WorkerFailed, e))?;
        clock.last_number = number;
        // SAFETY: checked non-null above; the caller promises it's writable.
        unsafe { out_number.write(number) };
        Ok(())
    })
}

/// Headless views: waits up to `timeout_ms` for frame `number` (or a later
/// one) to finish, copies its pixels into `rgba` (width × height × 4 bytes,
/// RGBA8 sRGB, rows top to bottom) and writes the number of the frame read
/// to `out_number`. Blocks; never call it from a UI thread.
///
/// # Safety
///
/// `view` is null or live; `rgba` is null or valid for `len` bytes of
/// writes; `out_number` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_read(
    view: *const FyView,
    number: u64,
    timeout_ms: u32,
    rgba: *mut u8,
    len: usize,
    out_number: *mut u64,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let view = unsafe { handle(view, "view") }?;
        if rgba.is_null() {
            return Err(Error::null("rgba"));
        }
        if out_number.is_null() {
            return Err(Error::null("out_number"));
        }
        let frames = view.frames.as_ref().ok_or_else(|| Error::new(FyStatus::InvalidValue, "not a headless view"))?;
        // SAFETY: not null, and the caller promises len writable bytes.
        let rgba = unsafe { std::slice::from_raw_parts_mut(rgba, len) };
        let read = frames.read(number, Duration::from_millis(timeout_ms.into()), rgba).map_err(|e| {
            // A frame that never came may be the worker's fault.
            if let Some(failure) = view.worker.failure() {
                return Error::new(FyStatus::WorkerFailed, failure);
            }
            let status = match e {
                ReadError::Timeout { .. } => FyStatus::Timeout,
                ReadError::WrongSize { .. } => FyStatus::WrongSize,
            };
            Error::new(status, e.to_string())
        })?;
        // SAFETY: checked non-null above; the caller promises it's writable.
        unsafe { out_number.write(read) };
        Ok(())
    })
}
