//! Setting, reading and removing the built-in components.

use fynkel_world::ObjectId;

use crate::error::{Error, FyStatus, guard};

use super::*;

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

/// Sets the transforms of `count` objects at once: `ids[i]` gets
/// `values[i]`. All are checked first, so if one is refused (an object
/// that doesn't exist, a value that isn't finite, a rotation that isn't a
/// unit) none is written. One call for a frame's moves instead of one each.
///
/// # Safety
///
/// `world` is null or live; `ids` and `values` are null or valid for
/// `count` reads (they may be null when `count` is 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_transform_set_many(
    world: *const FyWorld,
    ids: *const u64,
    values: *const FyTransform,
    count: usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        if count == 0 {
            return Ok(());
        }
        if ids.is_null() {
            return Err(Error::null("ids"));
        }
        if values.is_null() {
            return Err(Error::null("values"));
        }
        // SAFETY: not null, and valid for count reads per the contract.
        let (ids, values) =
            unsafe { (std::slice::from_raw_parts(ids, count), std::slice::from_raw_parts(values, count)) };
        // 0 isn't an id: refused up front, so the pairs below can't fail.
        if ids.contains(&0) {
            return Err(object(0).unwrap_err());
        }
        let pairs = ids.iter().zip(values).map(|(&id, value)| {
            let transform = value.to_component().expect("any FyTransform converts; the world checks the values");
            (ObjectId::from_raw(id).expect("not 0"), transform)
        });
        Ok(world.lock()?.insert_all(pairs)?)
    })
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
