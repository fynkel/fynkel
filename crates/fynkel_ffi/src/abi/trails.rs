//! Trails behind moving objects.

use fynkel_world::{Color, MaterialId, TABLE, TrailSettings};

use crate::error::{Error, FyStatus, guard};

use super::*;

// --- Trails ------------------------------------------------------------------

/// A trail's settings (Unity's `TrailRenderer`), as
/// `fynkel_world::TrailSettings`: seconds and metres, colours sRGB. Its
/// width and colour go over its length, head to tail.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FyTrailSettings {
    pub time: f32,
    pub min_vertex_distance: f32,
    pub width: f32,
    /// Its width times `width_table` along it, when set.
    pub has_width_table: bool,
    pub width_table: [f32; FY_PARTICLE_TABLE as usize],
    /// Its colour along it, when set; else white.
    pub has_color_table: bool,
    pub color_table: [FyColor; FY_PARTICLE_TABLE as usize],
    pub emitting: bool,
    /// What it's drawn with, or 0 for nothing drawn.
    pub material: u64,
}

impl Default for FyTrailSettings {
    /// Unity's: five seconds, a point each 0.1 m, a metre wide, white.
    fn default() -> Self {
        let d = TrailSettings::default();
        Self {
            time: d.time,
            min_vertex_distance: d.min_vertex_distance,
            width: d.width,
            has_width_table: false,
            width_table: [1.0; FY_PARTICLE_TABLE as usize],
            has_color_table: false,
            color_table: [FyColor::from(Color::WHITE); FY_PARTICLE_TABLE as usize],
            emitting: d.emitting,
            material: 0,
        }
    }
}

impl FyTrailSettings {
    fn settings(&self) -> TrailSettings {
        TrailSettings {
            time: self.time,
            min_vertex_distance: self.min_vertex_distance,
            width: self.width,
            width_curve: self.has_width_table.then(|| Box::new(self.width_table)),
            color: self.has_color_table.then(|| Box::new(self.color_table.map(Color::from))),
            emitting: self.emitting,
            material: MaterialId::from_raw(self.material),
        }
    }
}

const _: () = assert!(TABLE == FY_PARTICLE_TABLE as usize);

/// Gives the object a trail with `settings`, or changes the one it has,
/// keeping its points. Settings that can't be are
/// `FY_STATUS_INVALID_VALUE` and change nothing.
///
/// # Safety
///
/// `world` is null or live; `settings` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_trail_set(world: *const FyWorld, id: u64, settings: *const FyTrailSettings) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, settings) =
            unsafe { (handle(world, "world")?, settings.as_ref().ok_or_else(|| Error::null("settings"))?) };
        world.lock()?.set_trail(object(id)?, settings.settings())?;
        Ok(())
    })
}

/// Takes the object's trail away.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_trail_remove(world: *const FyWorld, id: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        world.lock()?.remove_trail(object(id)?)?;
        Ok(())
    })
}

/// Takes every point of the object's trail away (Unity's `Clear`).
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_trail_clear(world: *const FyWorld, id: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        world.lock()?.clear_trail(object(id)?)?;
        Ok(())
    })
}

/// How many points the object's trail has, the head's not counted.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_trail_count(world: *const FyWorld, id: u64, out: *mut usize) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let count = world.lock()?.trail(object(id)?)?.count();
        // SAFETY: per this function's contract.
        unsafe { write(out, count, "out") }
    })
}
