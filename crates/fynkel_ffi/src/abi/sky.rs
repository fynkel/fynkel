//! The sky and the fog, names, and where objects are in the world.

use std::sync::{Arc, PoisonError};

use fynkel_render::{Fog, Sky, SkyImage, SkyKind};
use fynkel_world::{Color, HdrData, ObjectId, Vec3};

use crate::error::{Error, FyStatus, guard};

use super::*;

// --- Sky and fog --------------------------------------------------------------

/// `FySky::kind`: none; the camera's clear colour shows.
pub const FY_SKY_NONE: u32 = 0;
/// `FySky::kind`: one colour, `horizon`, seen and lighting as ambient light.
pub const FY_SKY_COLOR: u32 = 1;
/// `FySky::kind`: `zenith` straight up, `horizon` level, `ground` straight
/// down, shown as picked.
pub const FY_SKY_GRADIENT: u32 = 2;
/// `FySky::kind`: `image`, an equirectangular `.hdr`, tone mapped as the
/// scene is.
pub const FY_SKY_IMAGE: u32 = 3;

/// The world's sky. A gradient or an image lights the scene, a shiny
/// surface mirroring it and a rough one taking its colour; a plain colour
/// adds to the ambient light.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FySky {
    /// A `FY_SKY_*`.
    pub kind: u32,
    pub zenith: FyColor,
    pub horizon: FyColor,
    pub ground: FyColor,
    /// `FY_SKY_IMAGE`'s; the world takes a share of it.
    pub image: *const FySkyImage,
    /// What the image's light is multiplied by.
    pub intensity: f32,
    /// How far the image is turned about the vertical, in degrees: positive
    /// turns it to the right.
    pub rotation: f32,
    /// What the light the sky gives is multiplied by: 0 for none. What's
    /// seen of it is unchanged.
    pub lighting: f32,
}

/// The world's fog: what's further than `start` fades into `color`, all of
/// it by `end`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyFog {
    pub color: FyColor,
    /// Whether `color` is used: without it, the fog is the sky's horizon.
    pub use_color: bool,
    /// Metres from the camera.
    pub start: f32,
    pub end: f32,
    /// The fog thins with height, halving every `height` metres above
    /// y = 0: 0 for as thick everywhere.
    pub height: f32,
}

/// A sky's image on its own, before any world has it. Opaque to C. Made
/// with `fy_sky_image_decode` on any thread, freed with
/// `fy_sky_image_free`. Worlds that use it share it.
#[derive(Debug)]
pub struct FySkyImage(Arc<SkyImage>);

/// Decodes a Radiance `.hdr` (equirectangular: −Z the middle, straight up
/// the top row) of `len` bytes into a sky image handle and writes it to
/// `out`. Anything else, or one over 8192 pixels on a side, is refused
/// (`FY_STATUS_INVALID_VALUE`) with a message saying why.
///
/// Decoding takes tens of milliseconds: call it off the UI thread.
///
/// # Safety
///
/// `bytes` is valid for `len` bytes (it may be null when `len` is 0);
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_sky_image_decode(bytes: *const u8, len: usize, out: *mut *mut FySkyImage) -> FyStatus {
    guard(|| {
        let bytes = if len == 0 {
            &[][..]
        } else {
            // SAFETY: per this function's contract.
            unsafe { std::slice::from_raw_parts(handle(bytes, "bytes")?, len) }
        };
        if out.is_null() {
            return Err(Error::null("out"));
        }
        let invalid = |message: String| Error::new(FyStatus::InvalidValue, message);
        let hdr = HdrData::decode(bytes).map_err(|e| invalid(e.to_string()))?;
        let image = SkyImage::from_rgb(hdr.width(), hdr.height(), hdr.rgb()).map_err(invalid)?;
        // SAFETY: checked non-null; the caller promises it's writable.
        unsafe { out.write(Box::into_raw(Box::new(FySkyImage(Arc::new(image))))) };
        Ok(())
    })
}

/// Frees a sky image handle. Worlds that use it keep their share. Null is
/// ignored.
///
/// # Safety
///
/// `image` is null or live, not used again after this.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_sky_image_free(image: *mut FySkyImage) {
    if !image.is_null() {
        // A panic here leaks the image rather than abort (fy_world_free).
        let _ = guard(|| {
            // SAFETY: made by Box::into_raw in fy_sky_image_decode; the
            // caller gives it up.
            drop(unsafe { Box::from_raw(image) });
            Ok(())
        });
    }
}

/// Writes the sky image's width and height in pixels.
///
/// # Safety
///
/// `image` is null or live; the outs are null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_sky_image_size(
    image: *const FySkyImage,
    out_width: *mut u32,
    out_height: *mut u32,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let image = unsafe { handle(image, "image") }?;
        // SAFETY: per this function's contract.
        unsafe {
            write(out_width, image.0.width(), "out_width")?;
            write(out_height, image.0.height(), "out_height")
        }
    })
}

fn linear(c: FyColor) -> Vec3 {
    let [r, g, b, _] = Color::from(c).to_linear();
    Vec3::new(r, g, b)
}

/// Checks `value` is finite and at least 0.
fn not_negative(value: f32, what: &str) -> Result<f32, Error> {
    if value.is_finite() && value >= 0.0 {
        Ok(value)
    } else {
        Err(Error::new(FyStatus::InvalidValue, format!("{what} {value} must be 0 or more")))
    }
}

/// Sets the world's sky, or clears it when `sky` is null or its kind is
/// `FY_SKY_NONE`. Taken by each `fy_view_frame` after; a new gradient or
/// image costs its views a few milliseconds of GPU work, once.
///
/// # Safety
///
/// `world` is null or live; `sky` is null or readable, its `image` null or
/// live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_world_sky_set(world: *const FyWorld, sky: *const FySky) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        // SAFETY: per this function's contract.
        let sky = match unsafe { sky.as_ref() } {
            None => None,
            Some(s) if s.kind == FY_SKY_NONE => None,
            Some(s) => {
                let kind = match s.kind {
                    FY_SKY_COLOR => SkyKind::Color(linear(s.horizon)),
                    FY_SKY_GRADIENT => SkyKind::Gradient {
                        zenith: linear(s.zenith),
                        horizon: linear(s.horizon),
                        ground: linear(s.ground),
                    },
                    FY_SKY_IMAGE => {
                        // SAFETY: per this function's contract.
                        let image = unsafe { handle(s.image, "sky.image") }?;
                        if !s.rotation.is_finite() {
                            return Err(Error::new(FyStatus::InvalidValue, "sky.rotation must be finite"));
                        }
                        SkyKind::Image {
                            image: Arc::clone(&image.0),
                            intensity: not_negative(s.intensity, "sky.intensity")?,
                            rotation: s.rotation.to_radians(),
                        }
                    }
                    other => {
                        return Err(Error::new(FyStatus::InvalidValue, format!("{other} isn't a kind of sky")));
                    }
                };
                Some(Sky { kind, lighting: not_negative(s.lighting, "sky.lighting")? })
            }
        };
        *world.sky.lock().unwrap_or_else(PoisonError::into_inner) = sky;
        Ok(())
    })
}

/// Sets the world's fog, or clears it when `fog` is null. Taken by each
/// `fy_view_frame` after.
///
/// # Safety
///
/// `world` is null or live; `fog` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_world_fog_set(world: *const FyWorld, fog: *const FyFog) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        // SAFETY: per this function's contract.
        let fog = match unsafe { fog.as_ref() } {
            None => None,
            Some(f) => {
                let start = not_negative(f.start, "fog.start")?;
                if !(f.end.is_finite() && f.end > start) {
                    return Err(Error::new(
                        FyStatus::InvalidValue,
                        format!("fog.end {} must be beyond its start {start}", f.end),
                    ));
                }
                let height = not_negative(f.height, "fog.height")?;
                Some(Fog {
                    color: f.use_color.then(|| linear(f.color)),
                    start,
                    end: f.end,
                    height: (height > 0.0).then_some(height),
                })
            }
        };
        *world.fog.lock().unwrap_or_else(PoisonError::into_inner) = fog;
        Ok(())
    })
}

/// Names the object with `len` bytes of UTF-8 at `name`, or clears its name
/// when `len` is 0. Names needn't be unique.
///
/// # Safety
///
/// `world` is null or live; `name` is null or valid for `len` reads.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_name_set(world: *const FyWorld, id: u64, name: *const u8, len: usize) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, name) = unsafe { (handle(world, "world")?, utf8(name, len, "name")?) };
        Ok(world.lock()?.set_name(object(id)?, Some(name))?)
    })
}

/// The object's name as UTF-8, without a terminating zero. Writes its
/// length in bytes to `out_len` (0 for no name), and copies it into `buf`
/// only when it fits in `cap`.
///
/// # Safety
///
/// `world` is null or live; `buf` is null or valid for `cap` writes;
/// `out_len` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_name_get(
    world: *const FyWorld,
    id: u64,
    buf: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let world = world.lock()?;
        let name = world.name(object(id)?)?.unwrap_or("");
        // SAFETY: per this function's contract.
        unsafe { write_all(name.bytes(), buf, cap, out_len) }
    })
}

/// Writes to `out` the first object below `root` named `name` (`len` bytes
/// of UTF-8), depth first in child order, or 0 when there's none. `root`
/// itself isn't searched.
///
/// # Safety
///
/// `world` is null or live; `name` is null or valid for `len` reads; `out`
/// is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_find(
    world: *const FyWorld,
    root: u64,
    name: *const u8,
    len: usize,
    out: *mut u64,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, name) = unsafe { (handle(world, "world")?, utf8(name, len, "name")?) };
        let found = world.lock()?.find(object(root)?, name)?;
        // SAFETY: per this function's contract.
        unsafe { write(out, found.map_or(0, ObjectId::raw), "out") }
    })
}

/// The object's transform in the world: its own after its parents', as
/// Dart's `worldTransform` works it out. The position is exact; the
/// rotation is the rotations multiplied, and the scale the scales (Unity's
/// lossy scale), which is approximate when a parent with an uneven scale
/// has a rotated child, because the result is skewed and no position,
/// rotation and scale can say that. A parent scaled to 0 still gives a
/// rotation. An object without a `Transform` counts as not moved from its
/// parent.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_world_transform_get(world: *const FyWorld, id: u64, out: *mut FyTransform) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let t = world.lock()?.world_transform(object(id)?)?;
        // SAFETY: per this function's contract.
        unsafe { write(out, FyTransform::from_component(t), "out") }
    })
}

/// The world transforms of `count` objects, `ids[i]`'s into `out[i]`, as
/// `fy_world_transform_get` gives each: what a package that follows many
/// objects (physics' colliders) reads in one call. Fails on the first id
/// that isn't an object, with what came before it written.
///
/// # Safety
///
/// `world` is null or live; `ids` is null or valid for `count` reads, and
/// `out` null or valid for `count` writes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_world_transform_get_many(
    world: *const FyWorld,
    ids: *const u64,
    count: usize,
    out: *mut FyTransform,
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
        if out.is_null() {
            return Err(Error::null("out"));
        }
        // SAFETY: not null, and valid for count reads and writes per the contract.
        let (ids, out) =
            unsafe { (std::slice::from_raw_parts(ids, count), std::slice::from_raw_parts_mut(out, count)) };
        let world = world.lock()?;
        for (&id, out) in ids.iter().zip(out) {
            *out = FyTransform::from_component(world.world_transform(object(id)?)?);
        }
        Ok(())
    })
}
