//! Worlds and the objects in them: making, freeing, spawning, despawning.

use std::sync::{Arc, Mutex};

use fynkel_world::{ObjectId, World};

use crate::error::{FyStatus, guard};

use super::*;

// --- Worlds and objects ------------------------------------------------------

/// A new, empty world. Free it with `fy_world_free`.
#[unsafe(no_mangle)]
pub extern "C" fn fy_world_new() -> *mut FyWorld {
    let world = FyWorld {
        world: Mutex::new(World::new()),
        raycaster: Mutex::default(),
        gpu: Mutex::default(),
        shake: Mutex::default(),
        quality: Mutex::default(),
        sky: Mutex::default(),
        fog: Mutex::default(),
        events: Mutex::default(),
        particle_events: Mutex::default(),
    };
    Arc::into_raw(Arc::new(world)).cast_mut()
}

/// Frees a world. Views of it keep working until they are freed too. Null
/// is ignored.
///
/// # Safety
///
/// `world` is null or a live world, not used again after this.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_world_free(world: *mut FyWorld) {
    if !world.is_null() {
        // A panic while dropping (in a part's own drop) must not unwind
        // into the caller, which aborts: what's left leaks instead, and the
        // message is the last error.
        let _ = guard(|| {
            // SAFETY: made by Arc::into_raw in fy_world_new; the caller
            // gives up its reference.
            drop(unsafe { Arc::from_raw(world) });
            Ok(())
        });
    }
}

/// Spawns an object with no components and writes its id to `out`.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_spawn(world: *const FyWorld, out: *mut u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let id = world.lock()?.spawn();
        // SAFETY: per this function's contract.
        unsafe { write(out, id.raw(), "out") }
    })
}

/// Despawns an object. Its id is never used again.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_despawn(world: *const FyWorld, id: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        Ok(world.lock()?.despawn(object(id)?)?)
    })
}

/// Whether the object exists. False for a null world.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_exists(world: *const FyWorld, id: u64) -> bool {
    let mut exists = false;
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        exists = ObjectId::from_raw(id).is_some_and(|id| world.lock().is_ok_and(|w| w.contains(id)));
        Ok(())
    });
    exists
}
