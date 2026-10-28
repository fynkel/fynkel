//! Animators: playing, blending and reading clips, and animation events.

use std::sync::PoisonError;

use crate::error::{FyStatus, guard};

use super::*;

// --- Animation ---------------------------------------------------------------

/// What an animator's layer plays, from `fy_animator_get`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyAnimatorState {
    /// The clip last played or faded to, or -1 for none or a blend.
    pub clip: i32,
    /// The blend last played or faded to, or -1 for none or a clip.
    pub blend: i32,
    /// Seconds into it; for a blend, into a round of it.
    pub time: f32,
    /// Its length in seconds; for a blend, a round's at its number now.
    pub length: f32,
    pub speed: f32,
}

/// What one of a reporting animator's clips did in `fy_world_animate`:
/// the span its time went through, from `from` on to `to`, passing its
/// end `loops` times on the way (a looping clip), or reaching it (`ended`,
/// one that doesn't). Played backwards, the same span, earliest first.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FyAnimationEvent {
    pub root: u64,
    pub clip: u32,
    pub loops: u32,
    pub from: f32,
    pub to: f32,
    pub ended: bool,
}

/// Plays clip `clip` of the model copy `root` (a model's index, as
/// `fy_model_clip`) on its layer `layer` (0 the base, up to 7) from its
/// start: at once in place of what the layer plays, with `fade` 0, or
/// fading in over `fade` seconds as the rest fade out, from the pose
/// shown. Fading to the motion last played or faded to changes nothing;
/// to one fading out, it fades back in from where it is. An object that
/// isn't a model copy with clips is `FY_STATUS_NO_COMPONENT`. `fade` is
/// read through a pointer, as `fy_world_animate`'s `dt`: a game may call
/// it each frame.
///
/// # Safety
///
/// `world` is null or live; `fade` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animator_play(
    world: *const FyWorld,
    root: u64,
    layer: u32,
    clip: u32,
    fade: *const f32,
    looping: bool,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, fade) = unsafe { (handle(world, "world")?, read(fade, "fade")?) };
        Ok(world.lock()?.play_on(object(root)?, layer, fynkel_world::Motion::Clip(clip), fade, looping)?)
    })
}

/// Makes a blend of `root`'s clips: `count` clips, each at its threshold
/// (any order), mixed by the number `fy_animator_blend` gives, their pace
/// kept together. Writes its number to `out`; an animator keeps up to 64.
///
/// # Safety
///
/// `world` is null or live; `clips` and `thresholds` are null or valid for
/// `count` reads; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animator_blend_define(
    world: *const FyWorld,
    root: u64,
    clips: *const u32,
    thresholds: *const f32,
    count: usize,
    out: *mut u32,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let pairs: Vec<(u32, f32)> = if count == 0 {
            Vec::new()
        } else {
            // SAFETY: per this function's contract.
            let (clips, thresholds) = unsafe {
                (
                    std::slice::from_raw_parts(handle(clips, "clips")?, count),
                    std::slice::from_raw_parts(handle(thresholds, "thresholds")?, count),
                )
            };
            clips.iter().copied().zip(thresholds.iter().copied()).collect()
        };
        let blend = world.lock()?.define_blend(object(root)?, &pairs)?;
        // SAFETY: per this function's contract.
        unsafe { write(out, blend, "out") }
    })
}

/// Sets the number blend `blend` mixes its clips by, `at[0]`, and plays
/// it on `root`'s layer `layer` as `fy_animator_play` plays a clip, with
/// the fade `at[1]` (always looping): a game calls it each frame with the
/// speed, and a fade only starts when the layer played something else.
///
/// # Safety
///
/// `world` is null or live; `at` is null or valid for 2 reads.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animator_blend(
    world: *const FyWorld,
    root: u64,
    layer: u32,
    blend: u32,
    at: *const f32,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        // SAFETY: per this function's contract: two floats.
        let [at, fade] = unsafe { read(at.cast::<[f32; 2]>(), "at") }?;
        let mut world = world.lock()?;
        let root = object(root)?;
        world.set_blend(root, blend, at)?;
        Ok(world.play_on(root, layer, fynkel_world::Motion::Blend(blend), fade, true)?)
    })
}

/// Stops what `root`'s layer `layer` plays: at once with `fade` 0, its
/// parts keeping the pose they're in, or fading out over `fade` seconds
/// to what's underneath. `fade` is read through a pointer.
///
/// # Safety
///
/// `world` is null or live; `fade` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animator_stop(world: *const FyWorld, root: u64, layer: u32, fade: *const f32) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, fade) = unsafe { (handle(world, "world")?, read(fade, "fade")?) };
        Ok(world.lock()?.stop_on(object(root)?, layer, fade)?)
    })
}

/// Sets `root`'s layer `layer`: the part it moves from, with everything
/// below it (`mask`, `mask_len` UTF-8 bytes; empty for the whole model),
/// whether it adds to what's underneath rather than replacing it, and how
/// much of it shows, 0 to 1. The base layer, 0, takes the whole model, in
/// place, at 1.
///
/// # Safety
///
/// `world` is null or live; `mask` is null or valid for `mask_len` reads.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animator_layer_set(
    world: *const FyWorld,
    root: u64,
    layer: u32,
    mask: *const u8,
    mask_len: usize,
    additive: bool,
    weight: f32,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, mask) = unsafe { (handle(world, "world")?, utf8(mask, mask_len, "mask")?) };
        let mask = if mask.is_empty() { None } else { Some(mask) };
        Ok(world.lock()?.set_animation_layer(object(root)?, layer, mask, additive, weight)?)
    })
}

/// How fast `root`'s clips play: 1 as made, 0 paused, negative backwards.
/// Read through a pointer: a game may set it each frame.
///
/// # Safety
///
/// `world` is null or live; `speed` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animator_speed_set(world: *const FyWorld, root: u64, speed: *const f32) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, speed) = unsafe { (handle(world, "world")?, read(speed, "speed")?) };
        Ok(world.lock()?.set_animation_speed(object(root)?, speed)?)
    })
}

/// Moves the current motion of `root`'s layer `layer` to `time` seconds,
/// kept within its length.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animator_time_set(world: *const FyWorld, root: u64, layer: u32, time: f32) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        Ok(world.lock()?.set_animation_time_on(object(root)?, layer, time)?)
    })
}

/// Whether `fy_world_animate` reports what `root`'s clips do.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animator_report_set(world: *const FyWorld, root: u64, on: bool) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        Ok(world.lock()?.set_animation_reporting(object(root)?, on)?)
    })
}

/// Writes what `root`'s layer `layer` plays to `out`.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animator_get(
    world: *const FyWorld,
    root: u64,
    layer: u32,
    out: *mut FyAnimatorState,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let state = world.lock()?.animator_state_on(object(root)?, layer)?;
        let (clip, blend) = match state.motion {
            Some(fynkel_world::Motion::Clip(c)) => (c as i32, -1),
            Some(fynkel_world::Motion::Blend(b)) => (-1, b as i32),
            None => (-1, -1),
        };
        let state = FyAnimatorState { clip, blend, time: state.time, length: state.length, speed: state.speed };
        // SAFETY: per this function's contract.
        unsafe { write(out, state, "out") }
    })
}

/// Writes clip `clip`'s weight in the mix of `root`'s layer `layer` to
/// `out`: 1 alone, 0 not playing, between while it fades or shares a
/// blend.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animator_weight(
    world: *const FyWorld,
    root: u64,
    layer: u32,
    clip: u32,
    out: *mut f32,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let weight = world.lock()?.clip_weight_on(object(root)?, layer, clip)?;
        // SAFETY: per this function's contract.
        unsafe { write(out, weight, "out") }
    })
}

/// The ids of `root`'s parts some clip moves, each once, as
/// `fy_children_get` writes them: all when they fit in `cap`, and their
/// number to `out_count`.
///
/// # Safety
///
/// `world` is null or live; `buf` is null or valid for `cap` writes;
/// `out_count` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animator_parts(
    world: *const FyWorld,
    root: u64,
    buf: *mut u64,
    cap: usize,
    out_count: *mut usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let parts = world.lock()?.animated_parts(object(root)?)?;
        // SAFETY: per this function's contract.
        unsafe { write_all(parts.iter().map(|p| p.raw()), buf, cap, out_count) }
    })
}

/// Advances every animator by the seconds at `dt` and poses its model's
/// parts, once a frame before it's drawn. Writes how many events the
/// reporting animators gave to `out_count`; `fy_animation_events_get`
/// reads them. `dt` is read through a pointer: a double passed to a call
/// is boxed, each frame, by Dart's JIT.
///
/// # Safety
///
/// `world` is null or live; `dt` is null or readable; `out_count` is null
/// or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_world_animate(world: *const FyWorld, dt: *const f32, out_count: *mut usize) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, dt) = unsafe { (handle(world, "world")?, read(dt, "dt")?) };
        let mut inner = world.lock()?;
        let mut events = world.events.lock().unwrap_or_else(PoisonError::into_inner);
        events.clear();
        inner.animate(dt, &mut events);
        // SAFETY: per this function's contract.
        unsafe { write(out_count, events.len(), "out_count") }
    })
}

/// The roots of the animators the last `fy_world_animate` posed (those
/// playing and shown), as `fy_children_get` writes ids: only their parts
/// may have moved.
///
/// # Safety
///
/// `world` is null or live; `buf` is null or valid for `cap` writes;
/// `out_count` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_world_posed_get(
    world: *const FyWorld,
    buf: *mut u64,
    cap: usize,
    out_count: *mut usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let world = world.lock()?;
        let posed = world.posed().iter().map(|r| r.raw());
        // SAFETY: per this function's contract.
        unsafe { write_all(posed, buf, cap, out_count) }
    })
}

/// The last `fy_world_animate`'s events, as `fy_children_get` writes ids:
/// all of them when they fit in `cap`, and their number to `out_count`.
///
/// # Safety
///
/// `world` is null or live; `buf` is null or valid for `cap` writes;
/// `out_count` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_animation_events_get(
    world: *const FyWorld,
    buf: *mut FyAnimationEvent,
    cap: usize,
    out_count: *mut usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let _inner = world.lock()?;
        let events = world.events.lock().unwrap_or_else(PoisonError::into_inner);
        let items = events.iter().map(|e| FyAnimationEvent {
            root: e.root.raw(),
            clip: e.clip,
            loops: e.loops,
            from: e.from,
            to: e.to,
            ended: e.ended,
        });
        // SAFETY: per this function's contract.
        unsafe { write_all(items, buf, cap, out_count) }
    })
}
