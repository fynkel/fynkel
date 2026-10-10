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

use fynkel_physics::{BodyKind, Moved, Physics, PhysicsError, Pose, Rotation, Shape, Vector};

/// Bumped on any change to the functions or types in this file.
pub const FP_ABI_VERSION: u32 = 1;

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
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FpShape {
    pub kind: u32,
    pub a: f32,
    pub b: f32,
    pub c: f32,
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
    Box::into_raw(Box::new(FpWorld { physics: Physics::new(), moved: Vec::new(), poisoned: false }))
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
