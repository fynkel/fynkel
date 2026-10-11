//! The C interface (`fp_*`) that `package:fynkel_physics` calls through
//! `dart:ffi`: a physics world (crates/fynkel_physics), stepped by the
//! caller, with the bodies a step moved read out in one buffer
//! (docs/stage-5.md §3.1). `include/fynkel_physics.h` is generated from
//! this file by cbindgen.
//!
//! As in the engine's interface: every call that can fail returns an
//! [`FpStatus`] and writes its results through out-pointers, ids are
//! `uint64_t` with 0 meaning "none", and a panic never crosses into the
//! caller. A world is used from one thread at a time; passing it after
//! `fp_world_free` is undefined behaviour, which `package:fynkel_physics`
//! makes impossible from Dart.
//!
//! This is the only physics crate where unsafe code is allowed.
#![allow(unsafe_code)]

use std::any::Any;
use std::cell::RefCell;
use std::ffi::{CString, c_char};
use std::panic::{self, AssertUnwindSafe};

use fynkel_physics::{
    Axis, BodyKind, ColliderShape, ColliderSpec, Hit, Moved, ObjectPose, Physics, PhysicsError, Pose, QueryOptions,
    QueryShape, Rotation, Shape, Vector,
};

/// Bumped on any change to the functions or types in this file.
pub const FP_ABI_VERSION: u32 = 2;

// --- Plain types ---------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FpVec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// A rotation, as a unit quaternion.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FpQuat {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

/// Where something is and how it's turned.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FpPose {
    pub position: FpVec3,
    pub rotation: FpQuat,
}

/// A body a step moved: its key and its pose.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FpMoved {
    pub key: u64,
    pub pose: FpPose,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FpBodyKind {
    Dynamic = 0,
    Kinematic = 1,
    Static = 2,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FpShapeKind {
    /// `a`, `b`, `c`: half its size on X, Y and Z.
    Box = 0,
    /// `a`: the radius.
    Sphere = 1,
    /// Along Y. `a`: the half height of the cylinder between the two
    /// half-spheres' centres; `b`: the radius.
    Capsule = 2,
}

/// A collider's shape: `kind` says what `a`, `b` and `c` are.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FpShape {
    pub kind: u32,
    pub a: f32,
    pub b: f32,
    pub c: f32,
}

/// An object's world transform, as the engine's `FyTransform` has it (the
/// same layout, so one buffer serves both libraries).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FpTransform {
    pub position: FpVec3,
    pub rotation: FpQuat,
    pub scale: FpVec3,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FpColliderKind {
    /// `size`: its size on X, Y and Z.
    Box = 0,
    /// `size.x`: the radius.
    Sphere = 1,
    /// `size.x`: the radius; `size.y`: the height, end to end; `direction`:
    /// 0, 1 or 2 for X, Y or Z.
    Capsule = 2,
}

/// A collider that follows an object, in the object's space: Unity's
/// collider properties. `kind` (an `FpColliderKind`) says what `size` is.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FpColliderSpec {
    pub kind: u32,
    pub size: FpVec3,
    pub direction: u32,
    pub center: FpVec3,
    /// 1 for a trigger, 0 for a solid collider.
    pub is_trigger: u32,
    /// 0 to 31.
    pub layer: u32,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FpQueryShapeKind {
    /// `a`: the center; `radius`.
    Sphere = 0,
    /// `a`: the center; `b`: half its size on each axis; `rotation`.
    Box = 1,
    /// `a` and `b`: its half-spheres' centres; `radius`.
    Capsule = 2,
}

/// A shape a query sweeps or tests, in the world. `kind` (an
/// `FpQueryShapeKind`) says which fields count.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FpQueryShape {
    pub kind: u32,
    pub a: FpVec3,
    pub b: FpVec3,
    pub radius: f32,
    pub rotation: FpQuat,
}

/// A ray a query casts: from `origin` along `direction`, at most
/// `max_distance`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FpRay {
    pub origin: FpVec3,
    pub direction: FpVec3,
    pub max_distance: f32,
}

/// Where a cast sweeps its shape: along `direction`, at most
/// `max_distance`. The last four floats of an `FpRay`, so one buffer can
/// hold either.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FpSweep {
    pub direction: FpVec3,
    pub max_distance: f32,
}

/// Where a query hit a collider. `collider` is 0 when it hit nothing.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FpHit {
    pub collider: u64,
    pub key: u64,
    pub point: FpVec3,
    pub normal: FpVec3,
    pub distance: f32,
}

/// A collider an overlap found.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FpFound {
    pub collider: u64,
    pub key: u64,
}

/// What an `fp_*` call returns. Anything but `Ok` leaves a message for
/// `fp_last_error`.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FpStatus {
    Ok = 0,
    /// No body has this id: it was removed, or never added.
    NoBody = 1,
    /// No collider has this id.
    NoCollider = 2,
    /// A value was out of range, not finite, or not one of the choices.
    InvalidValue = 3,
    /// A pointer argument was null.
    NullPointer = 4,
    /// A buffer had the wrong size.
    WrongSize = 5,
    /// The call panicked. Nothing crossed into the caller; the message says
    /// what happened.
    Panic = 6,
    /// An earlier call panicked while changing this world, so it can't be
    /// trusted any more.
    Poisoned = 7,
}

/// A physics world. Made by `fp_world_new`, freed by `fp_world_free`.
pub struct FpWorld {
    physics: Physics,
    /// What the last step moved, until `fp_moved_read`.
    moved: Vec<Moved>,
    /// What the last `_all` query hit, until `fp_hits_read`.
    hits: Vec<Hit>,
    /// What the last overlap found, until `fp_found_read`.
    found: Vec<(u64, u64)>,
    poisoned: bool,
}

impl std::fmt::Debug for FpWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FpWorld").field("physics", &self.physics).field("poisoned", &self.poisoned).finish()
    }
}

// --- Errors --------------------------------------------------------------------

struct Error {
    status: FpStatus,
    message: String,
}

impl Error {
    fn new(status: FpStatus, message: impl Into<String>) -> Self {
        Self { status, message: message.into() }
    }

    fn null(what: &str) -> Self {
        Self::new(FpStatus::NullPointer, format!("{what} is null"))
    }
}

impl From<PhysicsError> for Error {
    fn from(e: PhysicsError) -> Self {
        let status = match e {
            PhysicsError::NoBody(_) => FpStatus::NoBody,
            PhysicsError::NoCollider(_) => FpStatus::NoCollider,
            PhysicsError::Value(_) => FpStatus::InvalidValue,
        };
        Self::new(status, e.to_string())
    }
}

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

fn panic_message(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("(no message)")
}

/// Runs the body of an `fp_*` call: a panic becomes [`FpStatus::Panic`] and
/// never unwinds into the caller, and any error's message is kept for
/// `fp_last_error`.
fn guard(body: impl FnOnce() -> Result<(), Error>) -> FpStatus {
    let error = match panic::catch_unwind(AssertUnwindSafe(body)) {
        Ok(Ok(())) => return FpStatus::Ok,
        Ok(Err(error)) => error,
        Err(payload) => Error::new(FpStatus::Panic, format!("panicked: {}", panic_message(&*payload))),
    };
    let message = CString::new(error.message.replace('\0', "")).expect("nul bytes removed");
    LAST_ERROR.with(|last| *last.borrow_mut() = message);
    error.status
}

/// Runs `body` on a live world: a panic in it poisons the world, so every
/// later call on it fails with [`FpStatus::Poisoned`].
///
/// # Safety
///
/// `world` is null or live, and not used by another thread meanwhile.
unsafe fn with_world(world: *mut FpWorld, body: impl FnOnce(&mut FpWorld) -> Result<(), Error>) -> FpStatus {
    if world.is_null() {
        return guard(|| Err(Error::null("world")));
    }
    // SAFETY: live and not shared, as the caller promises.
    let w = unsafe { &mut *world };
    if w.poisoned {
        return guard(|| Err(Error::new(FpStatus::Poisoned, "an earlier call panicked while changing this world")));
    }
    let status = guard(|| body(&mut *w));
    if status == FpStatus::Panic {
        w.poisoned = true;
    }
    status
}

/// Writes `value` through `out`, or fails when it's null.
///
/// # Safety
///
/// `out` is null or writable.
unsafe fn write<T>(out: *mut T, what: &str, value: T) -> Result<(), Error> {
    if out.is_null() {
        return Err(Error::null(what));
    }
    // SAFETY: not null, and writable as the caller promises.
    unsafe { out.write(value) };
    Ok(())
}

/// Reads what `at` points to, or a NullPointer error naming `what`.
///
/// # Safety
///
/// `at` is null or readable.
unsafe fn read<T: Copy>(at: *const T, what: &str) -> Result<T, Error> {
    if at.is_null() {
        return Err(Error::null(what));
    }
    // SAFETY: not null, and readable as the caller promises.
    Ok(unsafe { at.read() })
}

// --- Conversions ---------------------------------------------------------------

fn vector(v: FpVec3) -> Vector {
    Vector::new(v.x, v.y, v.z)
}

fn pose(p: FpPose) -> Pose {
    let r = p.rotation;
    Pose::from_parts(vector(p.position), Rotation::from_xyzw(r.x, r.y, r.z, r.w))
}

fn fp_pose(p: Pose) -> FpPose {
    let (t, r) = (p.translation, p.rotation);
    FpPose { position: FpVec3 { x: t.x, y: t.y, z: t.z }, rotation: FpQuat { x: r.x, y: r.y, z: r.z, w: r.w } }
}

fn fp_vec3(v: Vector) -> FpVec3 {
    FpVec3 { x: v.x, y: v.y, z: v.z }
}

fn rotation(r: FpQuat) -> Rotation {
    Rotation::from_xyzw(r.x, r.y, r.z, r.w)
}

fn object(t: FpTransform) -> ObjectPose {
    ObjectPose { position: vector(t.position), rotation: rotation(t.rotation), scale: vector(t.scale) }
}

fn invalid(message: String) -> Error {
    Error::new(FpStatus::InvalidValue, message)
}

fn spec(s: FpColliderSpec) -> Result<ColliderSpec, Error> {
    let shape = match s.kind {
        0 => ColliderShape::Box { size: vector(s.size) },
        1 => ColliderShape::Sphere { radius: s.size.x },
        2 => ColliderShape::Capsule {
            radius: s.size.x,
            height: s.size.y,
            direction: match s.direction {
                0 => Axis::X,
                1 => Axis::Y,
                2 => Axis::Z,
                d => return Err(invalid(format!("{d} is no direction: 0, 1 or 2 for X, Y or Z"))),
            },
        },
        kind => return Err(invalid(format!("{kind} is no collider kind"))),
    };
    let is_trigger = match s.is_trigger {
        0 => false,
        1 => true,
        t => return Err(invalid(format!("is_trigger is 0 or 1, not {t}"))),
    };
    Ok(ColliderSpec { shape, center: vector(s.center), is_trigger, layer: s.layer })
}

fn query_shape(s: FpQueryShape) -> Result<QueryShape, Error> {
    Ok(match s.kind {
        0 => QueryShape::Sphere { center: vector(s.a), radius: s.radius },
        1 => QueryShape::Box { center: vector(s.a), half_extents: vector(s.b), rotation: rotation(s.rotation) },
        2 => QueryShape::Capsule { a: vector(s.a), b: vector(s.b), radius: s.radius },
        kind => return Err(invalid(format!("{kind} is no query shape kind"))),
    })
}

fn query(layers: u32, hit_triggers: u32) -> Result<QueryOptions, Error> {
    let hit_triggers = match hit_triggers {
        0 => false,
        1 => true,
        t => return Err(invalid(format!("hit_triggers is 0 or 1, not {t}"))),
    };
    Ok(QueryOptions { layers, hit_triggers })
}

fn fp_hit(hit: Option<Hit>) -> FpHit {
    hit.map_or_else(FpHit::default, |h| FpHit {
        collider: h.collider,
        key: h.key,
        point: fp_vec3(h.point),
        normal: fp_vec3(h.normal),
        distance: h.distance,
    })
}

fn count(len: usize) -> Result<u32, Error> {
    u32::try_from(len).map_err(|_| invalid("more than 4 billion results".into()))
}

fn body_kind(kind: u32) -> Result<BodyKind, Error> {
    Ok(match kind {
        0 => BodyKind::Dynamic,
        1 => BodyKind::Kinematic,
        2 => BodyKind::Static,
        _ => return Err(Error::new(FpStatus::InvalidValue, format!("{kind} is no body kind"))),
    })
}

fn shape(s: FpShape) -> Result<Shape, Error> {
    Ok(match s.kind {
        0 => Shape::Box { half_extents: Vector::new(s.a, s.b, s.c) },
        1 => Shape::Sphere { radius: s.a },
        2 => Shape::Capsule { half_height: s.a, radius: s.b },
        kind => return Err(Error::new(FpStatus::InvalidValue, format!("{kind} is no shape kind"))),
    })
}

// --- The interface -------------------------------------------------------------

/// The version of this interface: [`FP_ABI_VERSION`].
/// `package:fynkel_physics` checks it when it loads.
#[unsafe(no_mangle)]
pub extern "C" fn fp_abi_version() -> u32 {
    FP_ABI_VERSION
}

/// The message of the last failed call on this thread, as UTF-8 ("" before
/// any). Valid until the next failed call on this thread.
#[unsafe(no_mangle)]
pub extern "C" fn fp_last_error() -> *const c_char {
    LAST_ERROR.with(|last| last.borrow().as_ptr())
}

/// A new, empty world, with gravity (0, -9.81, 0). Free it with
/// `fp_world_free`.
#[unsafe(no_mangle)]
pub extern "C" fn fp_world_new() -> *mut FpWorld {
    Box::into_raw(Box::new(FpWorld {
        physics: Physics::new(),
        moved: Vec::new(),
        hits: Vec::new(),
        found: Vec::new(),
        poisoned: false,
    }))
}

/// Frees a world and everything in it. Null is ignored.
///
/// # Safety
///
/// `world` is null or a live world, not used again after this.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_world_free(world: *mut FpWorld) {
    if !world.is_null() {
        // SAFETY: made by Box::into_raw in fp_world_new; the caller gives
        // it up.
        drop(unsafe { Box::from_raw(world) });
    }
}

/// Sets the world's gravity.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_gravity_set(world: *mut FpWorld, gravity: FpVec3) -> FpStatus {
    // SAFETY: as this function's.
    unsafe { with_world(world, |w| Ok(w.physics.set_gravity(vector(gravity))?)) }
}

/// Adds a body of `kind` (an `FpBodyKind`) at `pose`, with `key` to know it
/// by in `fp_moved_read`, and writes its id to `out`.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_body_add(
    world: *mut FpWorld,
    kind: u32,
    key: u64,
    pose: FpPose,
    out: *mut u64,
) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            if out.is_null() {
                return Err(Error::null("out"));
            }
            let id = w.physics.add_body(body_kind(kind)?, key, self::pose(pose))?;
            write(out, "out", id)
        })
    }
}

/// Removes a body and the colliders on it.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_body_remove(world: *mut FpWorld, body: u64) -> FpStatus {
    // SAFETY: as this function's.
    unsafe { with_world(world, |w| Ok(w.physics.remove_body(body)?)) }
}

/// Writes where a body is to `out`.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_body_pose_get(world: *mut FpWorld, body: u64, out: *mut FpPose) -> FpStatus {
    // SAFETY: as this function's.
    unsafe { with_world(world, |w| write(out, "out", fp_pose(w.physics.body_pose(body)?))) }
}

/// Puts a body at `pose` at once (a teleport), and wakes it.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_body_pose_set(world: *mut FpWorld, body: u64, pose: FpPose) -> FpStatus {
    // SAFETY: as this function's.
    unsafe { with_world(world, |w| Ok(w.physics.set_body_pose(body, self::pose(pose))?)) }
}

/// Adds a collider of `shape` at `pose`, relative to `body`, or in the world
/// as a static collider when `body` is 0, and writes its id to `out`.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_collider_add(
    world: *mut FpWorld,
    body: u64,
    shape: FpShape,
    pose: FpPose,
    out: *mut u64,
) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            if out.is_null() {
                return Err(Error::null("out"));
            }
            let body = (body != 0).then_some(body);
            let id = w.physics.add_collider(body, self::shape(shape)?, self::pose(pose))?;
            write(out, "out", id)
        })
    }
}

/// Removes a collider.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_collider_remove(world: *mut FpWorld, collider: u64) -> FpStatus {
    // SAFETY: as this function's.
    unsafe { with_world(world, |w| Ok(w.physics.remove_collider(collider)?)) }
}

/// Moves the world on by `dt` seconds, and writes to `out_moved` how many
/// bodies the step moved: read them with `fp_moved_read`.
///
/// # Safety
///
/// `world` is null or live; `out_moved` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_step(world: *mut FpWorld, dt: f32, out_moved: *mut u32) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            if out_moved.is_null() {
                return Err(Error::null("out_moved"));
            }
            w.physics.step(dt)?;
            w.physics.moved(&mut w.moved);
            let count = u32::try_from(w.moved.len())
                .map_err(|_| Error::new(FpStatus::InvalidValue, "more than 4 billion bodies moved"))?;
            write(out_moved, "out_moved", count)
        })
    }
}

/// Writes the bodies the last `fp_step` moved to `out`, which holds `len`
/// of them: exactly as many as that step said.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable for `len` entries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_moved_read(world: *mut FpWorld, out: *mut FpMoved, len: usize) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            if len != w.moved.len() {
                return Err(Error::new(
                    FpStatus::WrongSize,
                    format!("the last step moved {} bodies, not {len}", w.moved.len()),
                ));
            }
            if len == 0 {
                return Ok(());
            }
            if out.is_null() {
                return Err(Error::null("out"));
            }
            // SAFETY: writable for len entries, as the caller promises.
            let out = std::slice::from_raw_parts_mut(out, len);
            for (slot, moved) in out.iter_mut().zip(&w.moved) {
                *slot = FpMoved { key: moved.key, pose: fp_pose(moved.pose) };
            }
            Ok(())
        })
    }
}

// --- Colliders that follow objects ---------------------------------------------

/// Adds a static collider (no body) that follows an object now at
/// `object`, with `key` to know it by in hits, and writes its id to `out`.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_static_collider_add(
    world: *mut FpWorld,
    key: u64,
    spec: FpColliderSpec,
    object: FpTransform,
    out: *mut u64,
) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            if out.is_null() {
                return Err(Error::null("out"));
            }
            let id = w.physics.add_static_collider(key, self::spec(spec)?, self::object(object))?;
            write(out, "out", id)
        })
    }
}

/// Changes what a following collider is: its shape, center, trigger or
/// layer.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_collider_spec_set(world: *mut FpWorld, collider: u64, spec: FpColliderSpec) -> FpStatus {
    // SAFETY: as this function's.
    unsafe { with_world(world, |w| Ok(w.physics.set_collider_spec(collider, self::spec(spec)?)?)) }
}

/// The following colliders `ids[i]`'s objects are now at `objects[i]`; those
/// that moved are put where their objects are. Checks all of them before
/// changing any.
///
/// # Safety
///
/// `world` is null or live; `ids` and `objects` are null or valid for
/// `count` reads.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_colliders_follow(
    world: *mut FpWorld,
    ids: *const u64,
    objects: *const FpTransform,
    count: usize,
) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            if count == 0 {
                return Ok(());
            }
            if ids.is_null() {
                return Err(Error::null("ids"));
            }
            if objects.is_null() {
                return Err(Error::null("objects"));
            }
            // SAFETY: valid for count reads, as the caller promises.
            let (ids, objects) = (std::slice::from_raw_parts(ids, count), std::slice::from_raw_parts(objects, count));
            let objects: Vec<ObjectPose> = objects.iter().map(|&t| self::object(t)).collect();
            Ok(w.physics.follow(ids, &objects)?)
        })
    }
}

/// Writes a collider's shape, as the physics has it (scaled), to `out_shape`
/// (an `FpShape`, as `fp_collider_add` takes it) and where it is to
/// `out_pose`: for drawing it.
///
/// # Safety
///
/// `world` is null or live; `out_shape` and `out_pose` are null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_collider_shape_get(
    world: *mut FpWorld,
    collider: u64,
    out_shape: *mut FpShape,
    out_pose: *mut FpPose,
) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            let (shape, pose) = w.physics.collider_shape(collider)?;
            let shape = match shape {
                Shape::Box { half_extents: h } => FpShape { kind: 0, a: h.x, b: h.y, c: h.z },
                Shape::Sphere { radius } => FpShape { kind: 1, a: radius, b: 0.0, c: 0.0 },
                Shape::Capsule { half_height, radius } => FpShape { kind: 2, a: half_height, b: radius, c: 0.0 },
            };
            write(out_shape, "out_shape", shape)?;
            write(out_pose, "out_pose", fp_pose(pose))
        })
    }
}

// --- Queries -------------------------------------------------------------------

/// The first collider a ray hits, into `out` (its `collider` 0 when none).
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable; `ray` is null or
/// readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_raycast(
    world: *mut FpWorld,
    ray: *const FpRay,
    layers: u32,
    hit_triggers: u32,
    out: *mut FpHit,
) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            if out.is_null() {
                return Err(Error::null("out"));
            }
            let ray = read(ray, "ray")?;
            let options = self::query(layers, hit_triggers)?;
            let hit = w.physics.raycast(vector(ray.origin), vector(ray.direction), ray.max_distance, options)?;
            write(out, "out", fp_hit(hit))
        })
    }
}

/// Every collider a ray hits, nearest first: writes how many to `out_count`;
/// read them with `fp_hits_read`.
///
/// # Safety
///
/// `world` is null or live; `out_count` is null or writable; `ray` is null
/// or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_raycast_all(
    world: *mut FpWorld,
    ray: *const FpRay,
    layers: u32,
    hit_triggers: u32,
    out_count: *mut u32,
) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            if out_count.is_null() {
                return Err(Error::null("out_count"));
            }
            let options = self::query(layers, hit_triggers)?;
            let ray = read(ray, "ray")?;
            w.physics.raycast_all(vector(ray.origin), vector(ray.direction), ray.max_distance, options, &mut w.hits)?;
            write(out_count, "out_count", count(w.hits.len())?)
        })
    }
}

/// The first collider `shape` hits, swept as `sweep` says, into `out`
/// (its `collider` 0 when none). Not those it starts overlapping.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable; `shape` and `sweep`
/// are null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_cast(
    world: *mut FpWorld,
    shape: *const FpQueryShape,
    sweep: *const FpSweep,
    layers: u32,
    hit_triggers: u32,
    out: *mut FpHit,
) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            if out.is_null() {
                return Err(Error::null("out"));
            }
            let (shape, sweep) = (query_shape(read(shape, "shape")?)?, read(sweep, "sweep")?);
            let options = self::query(layers, hit_triggers)?;
            let hit = w.physics.cast(shape, vector(sweep.direction), sweep.max_distance, options)?;
            write(out, "out", fp_hit(hit))
        })
    }
}

/// Every collider `shape` hits, swept, nearest first, those it starts
/// overlapping at distance 0: writes how many to `out_count`; read them with
/// `fp_hits_read`.
///
/// # Safety
///
/// `world` is null or live; `out_count` is null or writable; `shape` and
/// `sweep` are null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_cast_all(
    world: *mut FpWorld,
    shape: *const FpQueryShape,
    sweep: *const FpSweep,
    layers: u32,
    hit_triggers: u32,
    out_count: *mut u32,
) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            if out_count.is_null() {
                return Err(Error::null("out_count"));
            }
            let (shape, options) = (query_shape(read(shape, "shape")?)?, self::query(layers, hit_triggers)?);
            let sweep = read(sweep, "sweep")?;
            w.physics.cast_all(shape, vector(sweep.direction), sweep.max_distance, options, &mut w.hits)?;
            write(out_count, "out_count", count(w.hits.len())?)
        })
    }
}

/// Writes the hits of the last `fp_raycast_all` or `fp_cast_all` to `out`,
/// which holds `len`: exactly as many as it said.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable for `len` entries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_hits_read(world: *mut FpWorld, out: *mut FpHit, len: usize) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            if len != w.hits.len() {
                return Err(Error::new(FpStatus::WrongSize, format!("the last query hit {}, not {len}", w.hits.len())));
            }
            if len == 0 {
                return Ok(());
            }
            if out.is_null() {
                return Err(Error::null("out"));
            }
            // SAFETY: writable for len entries, as the caller promises.
            let out = std::slice::from_raw_parts_mut(out, len);
            for (slot, &hit) in out.iter_mut().zip(&w.hits) {
                *slot = fp_hit(Some(hit));
            }
            Ok(())
        })
    }
}

/// The colliders `shape` touches or holds: writes how many to `out_count`;
/// read them with `fp_found_read`.
///
/// # Safety
///
/// `world` is null or live; `out_count` is null or writable; `shape` is null or
/// readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_overlap(
    world: *mut FpWorld,
    shape: *const FpQueryShape,
    layers: u32,
    hit_triggers: u32,
    out_count: *mut u32,
) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            if out_count.is_null() {
                return Err(Error::null("out_count"));
            }
            w.physics.overlap(query_shape(read(shape, "shape")?)?, self::query(layers, hit_triggers)?, &mut w.found)?;
            write(out_count, "out_count", count(w.found.len())?)
        })
    }
}

/// Writes what the last `fp_overlap` found to `out`, which holds `len`:
/// exactly as many as it said.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable for `len` entries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_found_read(world: *mut FpWorld, out: *mut FpFound, len: usize) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            if len != w.found.len() {
                return Err(Error::new(
                    FpStatus::WrongSize,
                    format!("the last overlap found {}, not {len}", w.found.len()),
                ));
            }
            if len == 0 {
                return Ok(());
            }
            if out.is_null() {
                return Err(Error::null("out"));
            }
            // SAFETY: writable for len entries, as the caller promises.
            let out = std::slice::from_raw_parts_mut(out, len);
            for (slot, &(collider, key)) in out.iter_mut().zip(&w.found) {
                *slot = FpFound { collider, key };
            }
            Ok(())
        })
    }
}

/// Writes 1 to `out` if `shape` touches or holds any collider, else 0.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable; `shape` is null or
/// readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_check(
    world: *mut FpWorld,
    shape: *const FpQueryShape,
    layers: u32,
    hit_triggers: u32,
    out: *mut u32,
) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            if out.is_null() {
                return Err(Error::null("out"));
            }
            let any = w.physics.check(query_shape(read(shape, "shape")?)?, self::query(layers, hit_triggers)?)?;
            write(out, "out", u32::from(any))
        })
    }
}

/// Writes the point of `collider` nearest `point` to `out`: `point` itself
/// when it's inside.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable; `point` is null or
/// readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_closest_point(
    world: *mut FpWorld,
    collider: u64,
    point: *const FpVec3,
    out: *mut FpVec3,
) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            let point = vector(read(point, "point")?);
            write(out, "out", fp_vec3(w.physics.closest_point(collider, point)?))
        })
    }
}

/// Where a ray hits `collider`, into `out` (its `collider` 0 when it
/// doesn't, or starts inside it).
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable; `ray` is null or
/// readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_collider_raycast(
    world: *mut FpWorld,
    collider: u64,
    ray: *const FpRay,
    out: *mut FpHit,
) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            if out.is_null() {
                return Err(Error::null("out"));
            }
            let ray = read(ray, "ray")?;
            let hit =
                w.physics.raycast_collider(collider, vector(ray.origin), vector(ray.direction), ray.max_distance)?;
            write(out, "out", fp_hit(hit))
        })
    }
}
