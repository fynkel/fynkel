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
    Axis, BodyKind, BodySpec, CharacterHit, CharacterSettings, ColliderShape, ColliderSpec, Combine, Constraints,
    ContactKind, ForceMode, Hit, Material, Moved, ObjectPose, Physics, PhysicsError, Pose, Push, QueryOptions,
    QueryShape, Rotation, Settings, Shape, Vector,
};

/// Bumped on any change to the functions or types in this file.
pub const FP_ABI_VERSION: u32 = 7;

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

/// A body a step moved: its key, its pose and its velocities, linear (m/s)
/// and angular (rad/s).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FpMoved {
    pub key: u64,
    pub pose: FpPose,
    pub linear_velocity: FpVec3,
    pub angular_velocity: FpVec3,
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
    /// `mesh`: a mesh from `fp_mesh_add`; `convex`: 1 for its convex hull,
    /// 0 for its triangles (as its hull on a dynamic body). `size` is
    /// unused.
    Mesh = 3,
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
    pub material: FpMaterial,
    /// What its touching others is reported for (`fp_contacts_read`): 1
    /// triggers, 2 collisions, 3 both, 0 neither.
    pub listens: u32,
    /// For `FP_COLLIDER_KIND_MESH`.
    pub mesh: u64,
    pub convex: u32,
}

/// Where a pair of colliders is in its touching.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FpContactKind {
    Enter = 0,
    Stay = 1,
    Exit = 2,
}

/// A pair of colliders, `a` and `b` by their keys (`a` < `b`), starting,
/// going on with or ending their touching (`kind`, an `FpContactKind`),
/// as a trigger pair (`trigger` 1) or a collision (0). A collision's
/// impulse (the one the step gave `a`'s body), relative velocity (`b`'s
/// less `a`'s, before the step) and points are as `a` sees them; an exit
/// has none.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FpContact {
    pub kind: u32,
    pub trigger: u32,
    pub a: u64,
    pub b: u64,
    pub impulse: FpVec3,
    pub relative_velocity: FpVec3,
    /// Its points, from `first_point` in what `fp_contacts_read` gives.
    pub first_point: u32,
    pub point_count: u32,
}

/// Where two colliders touch, as `a` sees it: the normal points from `b`
/// towards `a`; the separation is negative when they're in each other.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FpContactPoint {
    pub point: FpVec3,
    pub normal: FpVec3,
    pub separation: f32,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FpCombine {
    Average = 0,
    Minimum = 1,
    Multiply = 2,
    Maximum = 3,
}

/// What a collider's surface is like: Unity's `PhysicsMaterial`. The
/// combines are `FpCombine`s.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FpMaterial {
    pub friction: f32,
    /// 0 to 1.
    pub bounciness: f32,
    pub friction_combine: u32,
    pub bounce_combine: u32,
}

/// What a rigidbody is: Unity's `Rigidbody` properties. Flags are 0 or 1;
/// `center_of_mass` counts when `has_center_of_mass` is 1; `constraints`
/// has Unity's `RigidbodyConstraints` bits.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FpBodySpec {
    pub kinematic: u32,
    pub mass: f32,
    pub use_gravity: u32,
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub max_angular_velocity: f32,
    pub has_center_of_mass: u32,
    pub center_of_mass: FpVec3,
    pub constraints: u32,
    pub continuous: u32,
}

/// Unity's `ForceMode`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FpForceMode {
    Force = 0,
    Acceleration = 1,
    Impulse = 2,
    VelocityChange = 3,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FpPushKind {
    /// `vector`: the force.
    Force = 0,
    /// `vector`: the torque.
    Torque = 1,
    /// `vector`: the force; `position`: where it acts, in the world.
    AtPosition = 2,
    /// `force`, `position` (the center), `radius` and `upwards`.
    Explosion = 3,
}

/// A push on a rigidbody: `kind` (an `FpPushKind`) says which fields
/// count; `mode` is an `FpForceMode`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FpPush {
    pub kind: u32,
    pub mode: u32,
    pub vector: FpVec3,
    pub position: FpVec3,
    pub force: f32,
    pub radius: f32,
    pub upwards: f32,
}

/// Where a rigidbody is and how it moves: its center of mass in the world,
/// and 1 when it sleeps.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FpBodyState {
    pub pose: FpPose,
    pub linear_velocity: FpVec3,
    pub angular_velocity: FpVec3,
    pub center_of_mass: FpVec3,
    pub sleeping: u32,
}

/// The world's settings: Unity's solver iterations and sleep threshold.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FpSettings {
    pub solver_iterations: u32,
    pub sleep_threshold: f32,
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

/// A character's move hit something on its sides (`FpCharacterMove::flags`).
pub const FP_COLLIDED_SIDES: u32 = 1;
/// A character's move hit something above it.
pub const FP_COLLIDED_ABOVE: u32 = 2;
/// A character's move hit something below it, or it stands on the ground.
pub const FP_COLLIDED_BELOW: u32 = 4;

const _: () = assert!(
    FP_COLLIDED_SIDES == fynkel_physics::COLLIDED_SIDES
        && FP_COLLIDED_ABOVE == fynkel_physics::COLLIDED_ABOVE
        && FP_COLLIDED_BELOW == fynkel_physics::COLLIDED_BELOW
);

/// How a character controller moves (Unity's `CharacterController`):
/// `slope_limit` in degrees, the rest in metres, and `mass`, how heavy it
/// is when it pushes dynamic bodies, in kilograms (0 pushes none).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FpCharacter {
    pub slope_limit: f32,
    pub step_offset: f32,
    pub skin_width: f32,
    pub mass: f32,
}

/// What `fp_character_move` did: how far the character went, what it hit
/// (`flags`: `FP_COLLIDED_SIDES`, `_ABOVE` and `_BELOW`), and how many colliders
/// it hit, to read with `fp_character_hits_read`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FpCharacterMove {
    pub translation: FpVec3,
    pub flags: u32,
    pub hit_count: u32,
}

/// A collider a character's move hit: where, and its surface's normal
/// there.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FpCharacterHit {
    pub collider: u64,
    pub key: u64,
    pub point: FpVec3,
    pub normal: FpVec3,
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
    /// The thread `fp_step_begin` steps on, once there's been one; first,
    /// so a running step ends before the rest is dropped.
    worker: Option<Worker>,
    /// Between `fp_step_begin` and `fp_step_end`: the worker has the world.
    stepping: bool,
    physics: Physics,
    /// What the last step moved, until `fp_moved_read`.
    moved: Vec<Moved>,
    /// What the last `_all` query hit, until `fp_hits_read`.
    hits: Vec<Hit>,
    /// What the last overlap found, until `fp_found_read`.
    found: Vec<(u64, u64)>,
    /// The last `fp_collider_edges`', until `fp_edges_read`.
    edges: Vec<[Vector; 2]>,
    /// What the last `fp_character_move` hit, until `fp_character_hits_read`.
    character_hits: Vec<CharacterHit>,
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
    if w.stepping {
        return guard(|| Err(Error::new(FpStatus::InvalidValue, "a step is running: call fp_step_end first")));
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
        3 => ColliderShape::Mesh {
            mesh: s.mesh,
            convex: match s.convex {
                0 => false,
                1 => true,
                c => return Err(invalid(format!("convex is 0 or 1, not {c}"))),
            },
        },
        kind => return Err(invalid(format!("{kind} is no collider kind"))),
    };
    let is_trigger = match s.is_trigger {
        0 => false,
        1 => true,
        t => return Err(invalid(format!("is_trigger is 0 or 1, not {t}"))),
    };
    let m = s.material;
    let material = Material {
        friction: m.friction,
        bounciness: m.bounciness,
        friction_combine: combine(m.friction_combine)?,
        bounce_combine: combine(m.bounce_combine)?,
    };
    if s.listens > 3 {
        return Err(invalid(format!("listens is 0 to 3, not {}", s.listens)));
    }
    Ok(ColliderSpec { shape, center: vector(s.center), is_trigger, layer: s.layer, material, listens: s.listens })
}

fn combine(c: u32) -> Result<Combine, Error> {
    Ok(match c {
        0 => Combine::Average,
        1 => Combine::Minimum,
        2 => Combine::Multiply,
        3 => Combine::Maximum,
        c => return Err(invalid(format!("{c} is no combine"))),
    })
}

fn flag(value: u32, what: &str) -> Result<bool, Error> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        v => Err(invalid(format!("{what} is 0 or 1, not {v}"))),
    }
}

fn body_spec(s: FpBodySpec) -> Result<BodySpec, Error> {
    if s.constraints & !Constraints::FREEZE_ALL.0 != 0 {
        return Err(invalid(format!("{} has bits that are no constraint", s.constraints)));
    }
    Ok(BodySpec {
        kinematic: flag(s.kinematic, "kinematic")?,
        mass: s.mass,
        use_gravity: flag(s.use_gravity, "use_gravity")?,
        linear_damping: s.linear_damping,
        angular_damping: s.angular_damping,
        max_angular_velocity: s.max_angular_velocity,
        center_of_mass: flag(s.has_center_of_mass, "has_center_of_mass")?.then(|| vector(s.center_of_mass)),
        constraints: Constraints(s.constraints),
        continuous: flag(s.continuous, "continuous")?,
    })
}

fn fp_body_spec(s: BodySpec) -> FpBodySpec {
    FpBodySpec {
        kinematic: u32::from(s.kinematic),
        mass: s.mass,
        use_gravity: u32::from(s.use_gravity),
        linear_damping: s.linear_damping,
        angular_damping: s.angular_damping,
        max_angular_velocity: s.max_angular_velocity,
        has_center_of_mass: u32::from(s.center_of_mass.is_some()),
        center_of_mass: fp_vec3(s.center_of_mass.unwrap_or(Vector::ZERO)),
        constraints: s.constraints.0,
        continuous: u32::from(s.continuous),
    }
}

fn push(p: FpPush) -> Result<Push, Error> {
    let mode = match p.mode {
        0 => ForceMode::Force,
        1 => ForceMode::Acceleration,
        2 => ForceMode::Impulse,
        3 => ForceMode::VelocityChange,
        m => return Err(invalid(format!("{m} is no force mode"))),
    };
    Ok(match p.kind {
        0 => Push::Force { force: vector(p.vector), mode },
        1 => Push::Torque { torque: vector(p.vector), mode },
        2 => Push::AtPosition { force: vector(p.vector), position: vector(p.position), mode },
        3 => Push::Explosion { force: p.force, center: vector(p.position), radius: p.radius, upwards: p.upwards, mode },
        k => return Err(invalid(format!("{k} is no push kind"))),
    })
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
        worker: None,
        stepping: false,
        physics: Physics::new(),
        moved: Vec::new(),
        hits: Vec::new(),
        found: Vec::new(),
        edges: Vec::new(),
        character_hits: Vec::new(),
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

/// Adds a mesh for mesh colliders (`FP_COLLIDER_KIND_MESH`): `vertex_count`
/// positions, three floats each, and `index_count` indices into them,
/// three per triangle. They're copied. Writes its id to `out`.
///
/// # Safety
///
/// `world` is null or live; `positions` is null or valid for
/// `vertex_count * 3` reads, `indices` for `index_count`; `out` is null or
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_mesh_add(
    world: *mut FpWorld,
    positions: *const f32,
    vertex_count: usize,
    indices: *const u32,
    index_count: usize,
    out: *mut u64,
) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            if positions.is_null() && vertex_count > 0 {
                return Err(Error::null("positions"));
            }
            if indices.is_null() && index_count > 0 {
                return Err(Error::null("indices"));
            }
            if !index_count.is_multiple_of(3) {
                return Err(invalid(format!("{index_count} indices: three per triangle")));
            }
            let points: Vec<Vector> = if vertex_count == 0 {
                Vec::new()
            } else {
                // SAFETY: not null, and valid for vertex_count * 3 reads, as the caller promises.
                let flat = std::slice::from_raw_parts(positions, vertex_count * 3);
                flat.chunks_exact(3).map(|p| Vector::new(p[0], p[1], p[2])).collect()
            };
            let triangles: Vec<[u32; 3]> = if index_count == 0 {
                Vec::new()
            } else {
                // SAFETY: not null, and valid for index_count reads, as the caller promises.
                let flat = std::slice::from_raw_parts(indices, index_count);
                flat.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect()
            };
            let id = w.physics.add_mesh(&points, &triangles)?;
            write(out, "out", id)
        })
    }
}

/// Removes a mesh no collider is made of.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_mesh_remove(world: *mut FpWorld, mesh: u64) -> FpStatus {
    // SAFETY: as this function's.
    unsafe { with_world(world, |w| Ok(w.physics.remove_mesh(mesh)?)) }
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

// --- The step on a thread of its own (an experiment, docs/stage-5.md §6) ----

/// What the worker steps: the world's physics and its moved list, which
/// nothing else touches until the step ends (`FpWorld::stepping`).
struct Job {
    physics: *mut Physics,
    moved: *mut Vec<Moved>,
    dt: f32,
}

// SAFETY: the pointers are used by the worker alone while the step runs.
unsafe impl Send for Job {}

struct Worker {
    start: Option<std::sync::mpsc::Sender<Job>>,
    done: std::sync::mpsc::Receiver<Result<(), Error>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Worker {
    fn new() -> Self {
        let (start, jobs) = std::sync::mpsc::channel::<Job>();
        let (finished, done) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("fynkel physics".into())
            .spawn(move || {
                for job in jobs {
                    let result = panic::catch_unwind(AssertUnwindSafe(|| {
                        // SAFETY: see Job.
                        let (physics, moved) = unsafe { (&mut *job.physics, &mut *job.moved) };
                        physics.step(job.dt)?;
                        physics.moved(moved);
                        Ok(())
                    }))
                    .unwrap_or_else(|payload| {
                        Err(Error::new(FpStatus::Panic, format!("panicked: {}", panic_message(&*payload))))
                    });
                    if finished.send(result).is_err() {
                        return;
                    }
                }
            })
            .expect("a thread for the physics");
        Self { start: Some(start), done, thread: Some(thread) }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.start = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Starts a step of `dt` seconds on the world's own thread and returns at
/// once; every other call on the world fails until `fp_step_end`.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_step_begin(world: *mut FpWorld, dt: f32) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            let worker = w.worker.get_or_insert_with(Worker::new);
            let job = Job { physics: &raw mut w.physics, moved: &raw mut w.moved, dt };
            worker
                .start
                .as_ref()
                .and_then(|start| start.send(job).ok())
                .ok_or_else(|| Error::new(FpStatus::Panic, "the physics thread is gone"))?;
            w.stepping = true;
            Ok(())
        })
    }
}

/// Waits for the step `fp_step_begin` started, and writes to `out_moved`
/// how many bodies it moved, as `fp_step` does.
///
/// # Safety
///
/// `world` is null or live; `out_moved` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_step_end(world: *mut FpWorld, out_moved: *mut u32) -> FpStatus {
    if world.is_null() {
        return guard(|| Err(Error::null("world")));
    }
    // SAFETY: live, as the caller promises.
    let w = unsafe { &mut *world };
    guard(|| {
        if !w.stepping {
            return Err(Error::new(FpStatus::InvalidValue, "no step is running"));
        }
        let worker = w.worker.as_ref().expect("a step runs on the worker");
        let result =
            worker.done.recv().unwrap_or_else(|_| Err(Error::new(FpStatus::Panic, "the physics thread is gone")));
        w.stepping = false;
        if let Err(error) = result {
            if error.status == FpStatus::Panic {
                w.poisoned = true;
            }
            return Err(error);
        }
        let count = u32::try_from(w.moved.len())
            .map_err(|_| Error::new(FpStatus::InvalidValue, "more than 4 billion bodies moved"))?;
        // SAFETY: null or writable, as the caller promises.
        unsafe { write(out_moved, "out_moved", count) }
    })
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
                *slot = FpMoved {
                    key: moved.key,
                    pose: fp_pose(moved.pose),
                    linear_velocity: fp_vec3(moved.linear_velocity),
                    angular_velocity: fp_vec3(moved.angular_velocity),
                };
            }
            Ok(())
        })
    }
}

// --- Colliders that follow objects ---------------------------------------------

/// Adds a collider that follows an object now at `object`, with `key` to
/// know it by in hits, on the rigidbody `body` (0 for none: a static
/// collider), and writes its id to `out`.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_object_collider_add(
    world: *mut FpWorld,
    key: u64,
    spec: FpColliderSpec,
    object: FpTransform,
    body: u64,
    out: *mut u64,
) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            if out.is_null() {
                return Err(Error::null("out"));
            }
            let body = (body != 0).then_some(body);
            let id = w.physics.add_object_collider(key, self::spec(spec)?, self::object(object), body)?;
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

/// A mesh collider's edges where they are in the world, for drawing it:
/// writes how many to `out_count`; read them with `fp_edges_read`.
///
/// # Safety
///
/// `world` is null or live; `out_count` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_collider_edges(world: *mut FpWorld, collider: u64, out_count: *mut u32) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            if out_count.is_null() {
                return Err(Error::null("out_count"));
            }
            w.physics.collider_edges(collider, &mut w.edges)?;
            write(out_count, "out_count", count(w.edges.len())?)
        })
    }
}

/// Writes the edges of the last `fp_collider_edges` to `out`, two points
/// each, which holds `len` edges: exactly as many as it said.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable for `2 * len` points.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_edges_read(world: *mut FpWorld, out: *mut FpVec3, len: usize) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            if len != w.edges.len() {
                return Err(Error::new(
                    FpStatus::WrongSize,
                    format!("the collider has {} edges, not {len}", w.edges.len()),
                ));
            }
            if len == 0 {
                return Ok(());
            }
            if out.is_null() {
                return Err(Error::null("out"));
            }
            // SAFETY: writable for 2 * len points, as the caller promises.
            let out = std::slice::from_raw_parts_mut(out, 2 * len);
            for (slot, &point) in out.iter_mut().zip(w.edges.iter().flatten()) {
                *slot = fp_vec3(point);
            }
            Ok(())
        })
    }
}

// --- Rigidbodies ---------------------------------------------------------------

/// Adds a rigidbody at `pose` (its object's world position and rotation),
/// with `key` to know it by in `fp_moved_read`, and writes its id to `out`.
/// Its colliders are added with `fp_object_collider_add`.
///
/// # Safety
///
/// `world` is null or live; `spec` and `pose` are null or readable; `out`
/// is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_rigidbody_add(
    world: *mut FpWorld,
    key: u64,
    spec: *const FpBodySpec,
    pose: *const FpPose,
    out: *mut u64,
) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            if out.is_null() {
                return Err(Error::null("out"));
            }
            let spec = body_spec(read(spec, "spec")?)?;
            let id = w.physics.add_rigidbody(key, spec, self::pose(read(pose, "pose")?))?;
            write(out, "out", id)
        })
    }
}

/// Changes what a rigidbody is.
///
/// # Safety
///
/// `world` is null or live; `spec` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_rigidbody_spec_set(world: *mut FpWorld, body: u64, spec: *const FpBodySpec) -> FpStatus {
    // SAFETY: as this function's.
    unsafe { with_world(world, |w| Ok(w.physics.set_body_spec(body, body_spec(read(spec, "spec")?)?)?)) }
}

/// Writes what a rigidbody is to `out`.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_rigidbody_spec_get(world: *mut FpWorld, body: u64, out: *mut FpBodySpec) -> FpStatus {
    // SAFETY: as this function's.
    unsafe { with_world(world, |w| write(out, "out", fp_body_spec(w.physics.body_spec(body)?))) }
}

/// Pushes a rigidbody: a force, a torque, a force at a point, or an
/// explosion. Kinematic bodies aren't pushed.
///
/// # Safety
///
/// `world` is null or live; `push` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_rigidbody_push(world: *mut FpWorld, body: u64, push: *const FpPush) -> FpStatus {
    // SAFETY: as this function's.
    unsafe { with_world(world, |w| Ok(w.physics.push(body, self::push(read(push, "push")?)?)?)) }
}

/// Writes where a rigidbody is and how it moves to `out`.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_rigidbody_state_get(world: *mut FpWorld, body: u64, out: *mut FpBodyState) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            let (pose, linear, angular, center, sleeping) = w.physics.body_state(body)?;
            let state = FpBodyState {
                pose: fp_pose(pose),
                linear_velocity: fp_vec3(linear),
                angular_velocity: fp_vec3(angular),
                center_of_mass: fp_vec3(center),
                sleeping: u32::from(sleeping),
            };
            write(out, "out", state)
        })
    }
}

/// Sets a rigidbody's linear (m/s) and angular (rad/s) velocity, and wakes
/// it.
///
/// # Safety
///
/// `world` is null or live; `linear` and `angular` are null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_rigidbody_velocities_set(
    world: *mut FpWorld,
    body: u64,
    linear: *const FpVec3,
    angular: *const FpVec3,
) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            let (linear, angular) = (vector(read(linear, "linear")?), vector(read(angular, "angular")?));
            Ok(w.physics.set_velocities(body, linear, angular)?)
        })
    }
}

/// Moves a kinematic rigidbody to `pose` through the next step, pushing
/// what's in the way; puts a dynamic one there at once.
///
/// # Safety
///
/// `world` is null or live; `pose` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_rigidbody_move(world: *mut FpWorld, body: u64, pose: *const FpPose) -> FpStatus {
    // SAFETY: as this function's.
    unsafe { with_world(world, |w| Ok(w.physics.move_body(body, self::pose(read(pose, "pose")?))?)) }
}

/// Puts a rigidbody to sleep (1) or wakes it (0).
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_rigidbody_sleep_set(world: *mut FpWorld, body: u64, asleep: u32) -> FpStatus {
    // SAFETY: as this function's.
    unsafe { with_world(world, |w| Ok(w.physics.set_sleeping(body, flag(asleep, "asleep")?)?)) }
}

/// Writes the world's settings to `out`.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_settings_get(world: *mut FpWorld, out: *mut FpSettings) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            let Settings { solver_iterations, sleep_threshold } = w.physics.settings();
            write(out, "out", FpSettings { solver_iterations, sleep_threshold })
        })
    }
}

/// Changes the world's settings.
///
/// # Safety
///
/// `world` is null or live; `settings` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_settings_set(world: *mut FpWorld, settings: *const FpSettings) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            let FpSettings { solver_iterations, sleep_threshold } = read(settings, "settings")?;
            Ok(w.physics.set_settings(Settings { solver_iterations, sleep_threshold })?)
        })
    }
}

// --- Contacts, the layer matrix and ignored pairs -------------------------------

/// The last step's contact events (enters, then stays, then exits) and
/// their points: writes how many there are to `out_count` and
/// `out_point_count`, and, when they fit in `capacity` and
/// `point_capacity`, the events to `out` and the points to `out_points`.
/// When they don't, it writes nothing else: call it again with room.
///
/// # Safety
///
/// `world` is null or live; `out_count` and `out_point_count` are null or
/// writable; `out` and `out_points` are null or writable for their
/// capacities.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_contacts_read(
    world: *mut FpWorld,
    out: *mut FpContact,
    capacity: usize,
    out_points: *mut FpContactPoint,
    point_capacity: usize,
    out_count: *mut u32,
    out_point_count: *mut u32,
) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            let events = w.physics.contact_events();
            let points = w.physics.contact_points();
            let count = u32::try_from(events.len()).map_err(|_| invalid("too many contacts".into()))?;
            let point_count = u32::try_from(points.len()).map_err(|_| invalid("too many contact points".into()))?;
            write(out_count, "out_count", count)?;
            write(out_point_count, "out_point_count", point_count)?;
            if events.len() > capacity || points.len() > point_capacity {
                return Ok(());
            }
            if !events.is_empty() {
                if out.is_null() {
                    return Err(Error::null("out"));
                }
                // SAFETY: writable for `capacity` entries, as the caller promises.
                let out = std::slice::from_raw_parts_mut(out, events.len());
                for (slot, e) in out.iter_mut().zip(events) {
                    *slot = FpContact {
                        kind: match e.kind {
                            ContactKind::Enter => FpContactKind::Enter,
                            ContactKind::Stay => FpContactKind::Stay,
                            ContactKind::Exit => FpContactKind::Exit,
                        } as u32,
                        trigger: u32::from(e.trigger),
                        a: e.a,
                        b: e.b,
                        impulse: fp_vec3(e.impulse),
                        relative_velocity: fp_vec3(e.relative_velocity),
                        first_point: e.first_point,
                        point_count: e.point_count,
                    };
                }
            }
            if !points.is_empty() {
                if out_points.is_null() {
                    return Err(Error::null("out_points"));
                }
                // SAFETY: writable for `point_capacity` entries, as the caller promises.
                let out = std::slice::from_raw_parts_mut(out_points, points.len());
                for (slot, p) in out.iter_mut().zip(points) {
                    *slot =
                        FpContactPoint { point: fp_vec3(p.point), normal: fp_vec3(p.normal), separation: p.separation };
                }
            }
            Ok(())
        })
    }
}

/// Whether colliders on layers `a` and `b` meet, into `out` (1 or 0).
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_layer_collision_get(world: *mut FpWorld, a: u32, b: u32, out: *mut u32) -> FpStatus {
    // SAFETY: as this function's.
    unsafe { with_world(world, |w| write(out, "out", u32::from(w.physics.layer_collision(a, b)?))) }
}

/// Makes colliders on layers `a` and `b` meet (`meet` 1) or not (0), from
/// the next step.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_layer_collision_set(world: *mut FpWorld, a: u32, b: u32, meet: u32) -> FpStatus {
    // SAFETY: as this function's.
    unsafe { with_world(world, |w| Ok(w.physics.set_layer_collision(a, b, flag(meet, "meet")?)?)) }
}

/// Whether two following colliders ignore each other, into `out` (1 or 0).
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_collision_ignore_get(
    world: *mut FpWorld,
    collider1: u64,
    collider2: u64,
    out: *mut u32,
) -> FpStatus {
    // SAFETY: as this function's.
    unsafe { with_world(world, |w| write(out, "out", u32::from(w.physics.ignores(collider1, collider2)?))) }
}

/// Makes two following colliders ignore each other (`ignore` 1) or not
/// (0), from the next step, until either's key is forgotten.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_collision_ignore_set(
    world: *mut FpWorld,
    collider1: u64,
    collider2: u64,
    ignore: u32,
) -> FpStatus {
    // SAFETY: as this function's.
    unsafe { with_world(world, |w| Ok(w.physics.set_ignores(collider1, collider2, flag(ignore, "ignore")?)?)) }
}

/// Forgets the pairs the collider with `key` ignores, before the key is
/// used for another collider.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_collider_key_forget(world: *mut FpWorld, key: u64) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            w.physics.forget_key(key);
            Ok(())
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

// --- Character controllers -----------------------------------------------------

/// Moves character controller `collider` (a followed capsule) by `motion`
/// over `dt` seconds, as far as what's in its way lets it, pushing the
/// dynamic bodies it walks into, and writes what it did to `out`. The
/// collider stays where it is: move its object by `out.translation` and
/// follow it. Read what it hit with `fp_character_hits_read`.
///
/// # Safety
///
/// `world` is null or live; `settings` and `motion` are null or readable;
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_character_move(
    world: *mut FpWorld,
    collider: u64,
    settings: *const FpCharacter,
    motion: *const FpVec3,
    dt: f32,
    out: *mut FpCharacterMove,
) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            if out.is_null() {
                return Err(Error::null("out"));
            }
            let s = read(settings, "settings")?;
            let settings = CharacterSettings {
                slope_limit: s.slope_limit,
                step_offset: s.step_offset,
                skin_width: s.skin_width,
                mass: s.mass,
            };
            let motion = vector(read(motion, "motion")?);
            let moved = w.physics.move_character(collider, motion, dt, settings, &mut w.character_hits)?;
            let hit_count = count(w.character_hits.len())?;
            write(
                out,
                "out",
                FpCharacterMove { translation: fp_vec3(moved.translation), flags: moved.flags, hit_count },
            )
        })
    }
}

/// Writes what the last `fp_character_move` hit to `out`, which holds
/// `len`: exactly as many as it said.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable for `len` entries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_character_hits_read(world: *mut FpWorld, out: *mut FpCharacterHit, len: usize) -> FpStatus {
    // SAFETY: as this function's.
    unsafe {
        with_world(world, |w| {
            if len != w.character_hits.len() {
                return Err(Error::new(
                    FpStatus::WrongSize,
                    format!("the last move hit {}, not {len}", w.character_hits.len()),
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
            for (slot, hit) in out.iter_mut().zip(&w.character_hits) {
                *slot = FpCharacterHit {
                    collider: hit.collider,
                    key: hit.key,
                    point: fp_vec3(hit.point),
                    normal: fp_vec3(hit.normal),
                };
            }
            Ok(())
        })
    }
}
