//! Shaders: compiling, describing, and adding them to a world.

use std::sync::Arc;

use fynkel_world::Shader;

use crate::error::{Error, FyStatus, guard};

use super::*;

// --- Shaders -----------------------------------------------------------------

/// A material's own shader, compiled (`fy_shader_new`), until
/// `fy_shader_free`. Worlds that add it share it.
#[derive(Debug)]
pub struct FyShader(Shader);

/// Compiles `source` (`source_len` bytes of WGSL) for a material, or for a
/// screen effect when it has `fn effect` (`fy_camera_effects_set`):
/// checked and joined to the engine's code. `name` is its file's, for messages.
/// Writes the new handle to `out`. A shader that doesn't compile is
/// `FY_STATUS_INVALID_VALUE`, its message `name:line:column: what`, the
/// line its file's own.
///
/// # Safety
///
/// `name` and `source` are valid for their lengths' reads (null when the
/// length is 0); `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_shader_new(
    name: *const u8,
    name_len: usize,
    source: *const u8,
    source_len: usize,
    out: *mut *mut FyShader,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (name, source) = unsafe { (utf8(name, name_len, "name")?, utf8(source, source_len, "source")?) };
        if out.is_null() {
            return Err(Error::null("out"));
        }
        let compiled = fynkel_render::CustomShader::new(name, source)
            .map_err(|e| Error::new(FyStatus::InvalidValue, e.to_string()))?;
        let shader = Shader {
            name: name.to_owned(),
            values_size: compiled.values_size as usize,
            textures: compiled.textures.len(),
            effect: compiled.effect,
            compiled: Arc::new(compiled),
        };
        // SAFETY: checked non-null; the caller promises it's writable.
        unsafe { out.write(Box::into_raw(Box::new(FyShader(shader)))) };
        Ok(())
    })
}

/// Frees a shader handle. Worlds that added it keep their share. Null is
/// ignored.
///
/// # Safety
///
/// `shader` is null or live, and not used after this.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_shader_free(shader: *mut FyShader) {
    if !shader.is_null() {
        let _ = guard(|| {
            // SAFETY: made by Box::into_raw in fy_shader_new; the caller
            // gives it up.
            drop(unsafe { Box::from_raw(shader) });
            Ok(())
        });
    }
}

/// What a shader takes, as UTF-8 lines, so a material can be given it by
/// name: `lit` or `unlit`; `values <bytes>`; `value <name> <f32|i32|u32>
/// <parts> <offset>` (parts: 1 for a number, 2 to 4 for a vector; the
/// offset in bytes) for each of its `Values`, an array's with `<count>
/// <stride>` after (its length; the bytes from one to the next); `texture <name>
/// <srgb|linear>` for each texture, in order; `own <name>` for each of its
/// `PerObject`, in order; `moves` if it has a `vertex`; `lights` if it
/// has a `light`; `behind` if it reads what's behind it (`scene_depth`,
/// `scene_color`); `effect` if it's a screen effect's. Its length goes to
/// `out_len`; it's copied into `buf` only when it fits in `cap`.
///
/// # Safety
///
/// `shader` is null or live; `buf` is null or valid for `cap` writes;
/// `out_len` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_shader_describe(
    shader: *const FyShader,
    buf: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let shader = unsafe { handle(shader, "shader") }?;
        let compiled = shader.0.compiled.downcast_ref::<fynkel_render::CustomShader>().expect("made by fy_shader_new");
        let text = describe_shader(compiled);
        // SAFETY: per this function's contract.
        unsafe { write_all(text.bytes(), buf, cap, out_len) }
    })
}

fn describe_shader(shader: &fynkel_render::CustomShader) -> String {
    use std::fmt::Write;
    let mut text = String::new();
    let lit = matches!(shader.lighting, fynkel_render::Lighting::Lit);
    let _ = writeln!(text, "{}", if lit { "lit" } else { "unlit" });
    let _ = writeln!(text, "values {}", shader.values_size);
    for v in &shader.values {
        let kind = match v.kind {
            fynkel_render::ValueKind::F32 => "f32",
            fynkel_render::ValueKind::I32 => "i32",
            fynkel_render::ValueKind::U32 => "u32",
        };
        match v.count {
            1 => writeln!(text, "value {} {kind} {} {}", v.name, v.size, v.offset),
            count => writeln!(text, "value {} {kind} {} {} {count} {}", v.name, v.size, v.offset, v.stride),
        }
        .ok();
    }
    for t in &shader.textures {
        let space = if t.space == fynkel_render::ColorSpace::Srgb { "srgb" } else { "linear" };
        let _ = writeln!(text, "texture {} {space}", t.name);
    }
    for name in &shader.own {
        let _ = writeln!(text, "own {name}");
    }
    if shader.moves {
        let _ = writeln!(text, "moves");
    }
    if shader.lights {
        let _ = writeln!(text, "lights");
    }
    if shader.reads_behind {
        let _ = writeln!(text, "behind");
    }
    if shader.effect {
        let _ = writeln!(text, "effect");
    }
    text
}

/// Gives the world a share of `shader` and writes the id materials use
/// for it to `out`.
///
/// # Safety
///
/// `world` and `shader` are null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_shader_add(world: *const FyWorld, shader: *const FyShader, out: *mut u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, shader) = unsafe { (handle(world, "world")?, handle(shader, "shader")?) };
        let id = world.lock()?.add_shader(shader.0.clone());
        // SAFETY: per this function's contract.
        unsafe { write(out, id.raw(), "out") }
    })
}

/// Puts `shader` in place of the world's shader `id`, as when its file
/// changes: its materials draw with it from the next frame, their values
/// kept where they fit.
///
/// # Safety
///
/// `world` and `shader` are null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_shader_set(world: *const FyWorld, id: u64, shader: *const FyShader) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, new) = unsafe { (handle(world, "world")?, handle(shader, "shader")?) };
        Ok(world.lock()?.set_shader(self::shader(id)?, new.0.clone())?)
    })
}

/// Removes a shader from the world. Refused (`FY_STATUS_IN_USE`) while a
/// material uses it.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_shader_remove(world: *const FyWorld, id: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        Ok(world.lock()?.remove_shader(shader(id)?)?)
    })
}

/// Sets the time a world's shaders see (`frame_time()`), in seconds, and
/// how far it moved since the last frame (`frame_delta()`): the game's,
/// so it stops while it's paused.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_world_time_set(world: *const FyWorld, time: f32, delta: f32) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        if !time.is_finite() || !delta.is_finite() {
            return Err(Error::new(FyStatus::InvalidValue, "the time must be finite"));
        }
        world.lock()?.set_shader_time(time, delta);
        Ok(())
    })
}
