//! Raycasts against what's drawn.

use fynkel_world::{Ray, Raycaster};

use crate::error::{Error, FyStatus, guard};

use super::*;

// --- Raycasts ----------------------------------------------------------------

/// A half-line from `origin` along `direction`, which needn't be unit
/// length.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyRay {
    pub origin: FyVec3,
    pub direction: FyVec3,
}

/// Where a ray hit first. `object` is 0 when it hit nothing; the rest is
/// then 0 too.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyRayHit {
    pub object: u64,
    pub point: FyVec3,
    /// The face's normal, unit length, on the side the ray came from.
    pub normal: FyVec3,
    /// From the ray's origin to `point`.
    pub distance: f32,
}

/// Casts `ray` at the world as it is now and writes the closest object it
/// hits no further than `max_distance` (which may be infinite) to `out`.
/// What a ray can hit is what a view draws: objects with a `Transform` and
/// a mesh renderer with both a mesh and a material, from the side they're
/// drawn from (both sides for a double-sided material). Cut-outs are hit as
/// if solid. Only objects on a layer in `mask` (bit n for layer n) are hit:
/// `0xFFFFFFFF` for all.
///
/// `FY_STATUS_INVALID_VALUE` for a ray without a direction or with a
/// non-finite origin, and for a negative or NaN `max_distance`.
///
/// # Safety
///
/// `world` is null or live; `ray` is null or readable; `out` is null or
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_raycast(
    world: *const FyWorld,
    ray: *const FyRay,
    max_distance: f32,
    mask: u32,
    out: *mut FyRayHit,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (handle, r) = unsafe { (handle(world, "world")?, read(ray, "ray")?) };
        let ray = Ray::new(r.origin.into(), r.direction.into()).ok_or_else(|| {
            Error::new(FyStatus::InvalidValue, "a ray needs a finite origin and a direction that isn't zero")
        })?;
        if max_distance.is_nan() || max_distance < 0.0 {
            return Err(Error::new(FyStatus::InvalidValue, format!("max_distance can't be {max_distance}")));
        }
        let world = handle.lock()?;
        // One left behind by a panic may hold half-made trees: it starts
        // again from nothing.
        let mut caster = handle.raycaster.lock().unwrap_or_else(|poisoned| {
            let mut caster = poisoned.into_inner();
            *caster = Raycaster::default();
            caster
        });
        let hit = caster.cast(&world, ray, max_distance, mask).map_or_else(FyRayHit::default, |h| FyRayHit {
            object: h.object.raw(),
            point: h.point.into(),
            normal: h.normal.into(),
            distance: h.distance,
        });
        // SAFETY: per this function's contract.
        unsafe { write(out, hit, "out") }
    })
}
