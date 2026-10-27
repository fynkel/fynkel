//! Textures: decoding, making, and adding them to a world.

use std::sync::Arc;

use fynkel_world::{ImageData, ImageError};

use crate::error::{Error, FyStatus, guard};

use super::*;

// --- Textures ----------------------------------------------------------------

/// An image on its own, before any world has it. Opaque to C. Made with
/// `fy_texture_decode` or `fy_texture_new_rgba` on any thread, freed with
/// `fy_texture_free`. Worlds that add it share its pixels, so freeing it
/// doesn't take it from them.
#[derive(Debug)]
pub struct FyTexture(pub(super) Arc<ImageData>);

/// Writes a new texture handle to `out`.
///
/// # Safety
///
/// `out` is null or writable.
unsafe fn new_texture(image: Result<ImageData, ImageError>, out: *mut *mut FyTexture) -> Result<(), Error> {
    if out.is_null() {
        return Err(Error::null("out"));
    }
    let image = image.map_err(|e| Error::new(FyStatus::InvalidValue, e.to_string()))?;
    // SAFETY: checked non-null; the caller promises it's writable.
    unsafe { out.write(Box::into_raw(Box::new(FyTexture(Arc::new(image))))) };
    Ok(())
}

/// Decodes a PNG or JPEG of `len` bytes into a texture handle and writes
/// it to `out`. Any PNG layout becomes 8-bit RGBA. A broken or unsupported
/// file, or one over 8192 pixels on a side, is refused
/// (`FY_STATUS_INVALID_VALUE`) with a message saying why.
///
/// Decoding a big image takes tens of milliseconds: call it off the UI
/// thread.
///
/// # Safety
///
/// `bytes` is valid for `len` bytes (it may be null when `len` is 0);
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_texture_decode(bytes: *const u8, len: usize, out: *mut *mut FyTexture) -> FyStatus {
    guard(|| {
        let bytes = if len == 0 {
            &[][..]
        } else {
            // SAFETY: per this function's contract.
            unsafe { std::slice::from_raw_parts(handle(bytes, "bytes")?, len) }
        };
        // SAFETY: per this function's contract.
        unsafe { new_texture(ImageData::decode(bytes), out) }
    })
}

/// Makes a texture handle from `len` bytes of 8-bit RGBA, rows from the
/// top, and writes it to `out`. `len` must be `width × height × 4`.
///
/// # Safety
///
/// `pixels` is valid for `len` bytes (it may be null when `len` is 0);
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_texture_new_rgba(
    width: u32,
    height: u32,
    pixels: *const u8,
    len: usize,
    out: *mut *mut FyTexture,
) -> FyStatus {
    guard(|| {
        let pixels = if len == 0 {
            Vec::new()
        } else {
            // SAFETY: per this function's contract.
            unsafe { std::slice::from_raw_parts(handle(pixels, "pixels")?, len) }.to_vec()
        };
        // SAFETY: per this function's contract.
        unsafe { new_texture(ImageData::from_rgba(width, height, pixels), out) }
    })
}

/// Frees a texture handle. Worlds that added it keep their share. Null is
/// ignored.
///
/// # Safety
///
/// `texture` is null or live, not used again after this.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_texture_free(texture: *mut FyTexture) {
    if !texture.is_null() {
        // A panic here leaks the texture rather than abort (fy_world_free).
        let _ = guard(|| {
            // SAFETY: made by Box::into_raw in new_texture; the caller gives
            // it up.
            drop(unsafe { Box::from_raw(texture) });
            Ok(())
        });
    }
}

/// Writes the texture's width and height in pixels.
///
/// # Safety
///
/// `texture` is null or live; the outs are null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_texture_size(
    texture: *const FyTexture,
    out_width: *mut u32,
    out_height: *mut u32,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let texture = unsafe { handle(texture, "texture") }?;
        // SAFETY: per this function's contract.
        unsafe {
            write(out_width, texture.0.width(), "out_width")?;
            write(out_height, texture.0.height(), "out_height")
        }
    })
}

/// Gives the world a share of `texture` (nothing is copied) and writes the
/// id materials use for it to `out`. Each view uploads it to the GPU the
/// first frame a material uses it.
///
/// # Safety
///
/// `world` and `texture` are null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_texture_add(world: *const FyWorld, texture: *const FyTexture, out: *mut u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, texture) = unsafe { (handle(world, "world")?, handle(texture, "texture")?) };
        let id = world.lock()?.add_texture(Arc::clone(&texture.0));
        // SAFETY: per this function's contract.
        unsafe { write(out, id.raw(), "out") }
    })
}

/// Removes a texture from the world; each view frees its GPU copy on its
/// next frame. Refused (`FY_STATUS_IN_USE`) while a material uses it.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_texture_remove(world: *const FyWorld, id: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        Ok(world.lock()?.remove_texture(texture(id)?)?)
    })
}
