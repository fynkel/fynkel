//! Parents and children, active and layers, the active camera and its effects,
//! and quality.

use std::sync::PoisonError;

use fynkel_world::{MAX_SHADER_TEXTURES, ObjectId, Vec3};

use crate::device::Device;
use crate::error::{Error, FyStatus, guard};

use super::*;

// --- Hierarchy ---------------------------------------------------------------

/// Copies `items` into `buf` when they fit in `cap`, and writes how many
/// there are to `out_count` either way, so a caller whose buffer was too
/// small can retry with one big enough. `buf` may be null when `cap` is 0.
///
/// # Safety
///
/// `buf` is null or valid for `cap` writes of `T`; `out_count` is null or
/// writable.
pub(super) unsafe fn write_all<T>(
    items: impl ExactSizeIterator<Item = T>,
    buf: *mut T,
    cap: usize,
    out_count: *mut usize,
) -> Result<(), Error> {
    let count = items.len();
    if count <= cap && count > 0 {
        if buf.is_null() {
            return Err(Error::null("buf"));
        }
        for (i, item) in items.enumerate() {
            // SAFETY: not null, and i < count <= cap, which the caller
            // promises are writable.
            unsafe { buf.add(i).write(item) };
        }
    }
    // SAFETY: per the caller's contract.
    unsafe { write(out_count, count, "out_count") }
}

/// A UTF-8 string from `ptr` and `len` bytes. `ptr` may be null when `len`
/// is 0.
///
/// # Safety
///
/// `ptr` is null or valid for `len` reads.
pub(super) unsafe fn utf8<'a>(ptr: *const u8, len: usize, what: &str) -> Result<&'a str, Error> {
    if len == 0 {
        return Ok("");
    }
    if ptr.is_null() {
        return Err(Error::null(what));
    }
    // SAFETY: not null, and the caller promises len readable bytes.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    std::str::from_utf8(bytes).map_err(|e| Error::new(FyStatus::InvalidValue, format!("{what} is not UTF-8: {e}")))
}

/// Makes `child` a child of `parent`, or a root when `parent` is 0. Its
/// `Transform` is kept, so it now counts from the new parent. A parent that
/// is `child` or below it is refused with `FY_STATUS_INVALID_VALUE`.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_parent_set(world: *const FyWorld, child: u64, parent: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let parent = if parent == 0 { None } else { Some(object(parent)?) };
        Ok(world.lock()?.set_parent(object(child)?, parent)?)
    })
}

/// Writes the object's parent to `out`, or 0 for a root.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_parent_get(world: *const FyWorld, id: u64, out: *mut u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let parent = world.lock()?.parent(object(id)?)?;
        // SAFETY: per this function's contract.
        unsafe { write(out, parent.map_or(0, ObjectId::raw), "out") }
    })
}

/// The object's children, in the order they were added. Writes how many
/// there are to `out_count`, and copies them into `buf` only when they all
/// fit in `cap`.
///
/// # Safety
///
/// `world` is null or live; `buf` is null or valid for `cap` writes;
/// `out_count` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_children_get(
    world: *const FyWorld,
    id: u64,
    buf: *mut u64,
    cap: usize,
    out_count: *mut usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let world = world.lock()?;
        let children = world.children(object(id)?)?.iter().map(|c| c.raw());
        // SAFETY: per this function's contract.
        unsafe { write_all(children, buf, cap, out_count) }
    })
}

/// Writes to `out` whether `id` is `root` or below it.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_is_part_of(world: *const FyWorld, id: u64, root: u64, out: *mut bool) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let part = world.lock()?.is_part_of(object(id)?, object(root)?)?;
        // SAFETY: per this function's contract.
        unsafe { write(out, part, "out") }
    })
}

/// Turns the object on (`true`) or off. An object that is off hides itself
/// and everything below it: they aren't drawn, don't light or show the
/// scene, and aren't hit by rays. Its children keep their own flags.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_active_set(world: *const FyWorld, id: u64, active: bool) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        Ok(world.lock()?.set_active(object(id)?, active)?)
    })
}

/// Writes the object's own flag to `out`, whatever its parents' are.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_active_get(world: *const FyWorld, id: u64, out: *mut bool) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let active = world.lock()?.active(object(id)?)?;
        // SAFETY: per this function's contract.
        unsafe { write(out, active, "out") }
    })
}

/// Puts the object on `layer`, 0 to 31 (`FY_STATUS_INVALID_VALUE` past
/// that). Cameras draw, and rays hit, only the layers in their masks. Each
/// object has its own: a child doesn't take its parent's.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_layer_set(world: *const FyWorld, id: u64, layer: u32) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        Ok(world.lock()?.set_layer(object(id)?, layer)?)
    })
}

/// Writes the object's layer to `out`: 0 unless put on another.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_layer_get(world: *const FyWorld, id: u64, out: *mut u32) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let layer = world.lock()?.layer(object(id)?)?;
        // SAFETY: per this function's contract.
        unsafe { write(out, layer, "out") }
    })
}

/// Writes to `out` whether the object is shown: it and every parent above
/// it on.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_active_in_hierarchy_get(world: *const FyWorld, id: u64, out: *mut bool) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let shown = world.lock()?.active_in_hierarchy(object(id)?)?;
        // SAFETY: per this function's contract.
        unsafe { write(out, shown, "out") }
    })
}

/// Writes to `out` the camera a view of the world draws from: the shown
/// object with the lowest id that has both a camera and a transform, or 0
/// when there is none.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_camera_active(world: *const FyWorld, out: *mut u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let camera = world.lock()?.active_camera();
        // SAFETY: per this function's contract.
        unsafe { write(out, camera.map_or(0, ObjectId::raw), "out") }
    })
}

/// Shakes camera `camera` by `offset` (in the world, in metres) when a view
/// draws from it, without moving the object: what the world's camera
/// shake is. 0 for `camera` (or a null `offset`) stops it. Taken by each
/// `fy_view_frame` after.
///
/// # Safety
///
/// `world` is null or live; `offset` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_camera_shake_set(world: *const FyWorld, camera: u64, offset: *const FyVec3) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        // SAFETY: per this function's contract: null or readable.
        let offset = unsafe { offset.as_ref() }.copied();
        let shake = match (ObjectId::from_raw(camera), offset) {
            (Some(id), Some(by)) => {
                let by = Vec3::from(by);
                if !by.is_finite() {
                    return Err(Error::new(FyStatus::InvalidValue, format!("a shake of {by} isn't finite")));
                }
                Some((id, by))
            }
            _ => None,
        };
        *world.shake.lock().unwrap_or_else(PoisonError::into_inner) = shake;
        Ok(())
    })
}

/// A screen effect, for `fy_camera_effects_set`: a shader the world has
/// (`fy_shader_add`) with `fn effect`, `values_len` bytes of its values,
/// laid out as its `Values` (`fy_shader_describe`), and its textures in
/// the order it declares them, 0 for none (it reads white).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FyScreenEffect {
    pub shader: u64,
    pub values: *const u8,
    pub values_len: usize,
    pub textures: [u64; 4],
}

/// Gives the camera on `object` these `count` screen effects, in place of
/// any it had (0 takes them away): run over its picture in order, each
/// over what the one before made, when a view draws from it. A shader that
/// isn't an effect's is `FY_STATUS_INVALID_VALUE`; values that aren't as
/// many bytes as its shader's are `FY_STATUS_WRONG_SIZE`; a texture the
/// world hasn't, `FY_STATUS_NO_TEXTURE`. A shader or texture can't be
/// removed while an effect uses it.
///
/// # Safety
///
/// `world` is null or live; `effects` is null or valid for `count` reads
/// (null when `count` is 0), and each one's `values` for `values_len`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_camera_effects_set(
    world: *const FyWorld,
    object: u64,
    effects: *const FyScreenEffect,
    count: usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let effects: &[FyScreenEffect] = if count == 0 {
            &[]
        } else {
            // SAFETY: per this function's contract.
            unsafe { std::slice::from_raw_parts(handle(effects, "effects")?, count) }
        };
        let mut list = Vec::with_capacity(effects.len());
        for e in effects {
            let values = if e.values_len == 0 {
                Vec::new()
            } else {
                // SAFETY: per this function's contract.
                unsafe { std::slice::from_raw_parts(handle(e.values, "values")?, e.values_len) }.to_vec()
            };
            let mut textures = [None; MAX_SHADER_TEXTURES];
            for (slot, &id) in textures.iter_mut().zip(&e.textures) {
                *slot = if id == 0 { None } else { Some(texture(id)?) };
            }
            list.push(fynkel_world::ScreenEffect { shader: shader(e.shader)?, values, textures });
        }
        Ok(world.lock()?.set_camera_effects(self::object(object)?, list)?)
    })
}

/// Sets what the world's views draw, and how finely, or the device's
/// level's (`fy_quality_auto`) when `quality` is null, as a new world has.
/// Taken by each `fy_view_frame` after.
///
/// # Safety
///
/// `world` is null or live; `quality` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_world_quality_set(world: *const FyWorld, quality: *const FyQuality) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        // SAFETY: per this function's contract.
        let quality = unsafe { quality.as_ref() }.map(|q| q.to_quality()).transpose()?;
        *world.quality.lock().unwrap_or_else(PoisonError::into_inner) = quality;
        Ok(())
    })
}

/// Writes a level's quality to `out`: `level` is `FY_QUALITY_LOW`,
/// `_MEDIUM` or `_HIGH`.
///
/// # Safety
///
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_quality_preset(level: u32, out: *mut FyQuality) -> FyStatus {
    guard(|| {
        let level =
            level_named(level).ok_or_else(|| Error::new(FyStatus::InvalidValue, format!("{level} isn't a level")))?;
        // SAFETY: per this function's contract.
        unsafe { write(out, FyQuality::from_quality(level.quality()), "out") }
    })
}

/// Writes to `out` how many pipelines of materials' own shaders are being
/// made beside the frames: a view on screen draws without a shader's
/// objects while the driver compiles it, the first time it's drawn on the
/// device, rather than holding a frame up. 0 when every shader drawn so
/// far is ready, as a loading screen waits for. Makes the device if
/// there's none yet.
///
/// # Safety
///
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_shaders_waiting(out: *mut u32) -> FyStatus {
    guard(|| {
        let device = Device::current().map_err(|e| Error::new(FyStatus::NoGpu, e.to_string()))?;
        // SAFETY: per this function's contract.
        unsafe { write(out, device.kit.shaders_waiting(), "out") }
    })
}

/// Writes the level this device picks for a world without a quality of
/// its own to `out`: `FY_QUALITY_LOW`, `_MEDIUM` or `_HIGH`. Makes the
/// device if there's none yet.
///
/// # Safety
///
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_quality_auto(out: *mut u32) -> FyStatus {
    guard(|| {
        let device = Device::current().map_err(|e| Error::new(FyStatus::NoGpu, e.to_string()))?;
        // SAFETY: per this function's contract.
        unsafe { write(out, level_of(device.kit.device_quality()), "out") }
    })
}
