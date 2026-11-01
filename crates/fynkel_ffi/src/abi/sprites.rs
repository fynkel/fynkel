//! Pictures in the scene (stage 15): sprite renderers, a texture the world
//! has on quads.

use fynkel_world::{Color, Facing, SpriteSettings, TextureId, Vec2};

use crate::error::{Error, FyStatus, guard};

use super::*;

// --- Sprites -----------------------------------------------------------------

/// A sprite renderer's settings (Unity's), as
/// `fynkel_world::SpriteSettings`: metres, the colour sRGB.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FySpriteSettings {
    /// A texture from `fy_texture_add`.
    pub texture: u64,
    /// Its width and height, in metres.
    pub size: FyVec2,
    pub color: FyColor,
    pub flip_x: bool,
    pub flip_y: bool,
    /// The point of it its object's origin is, 0 to 1 from its bottom left.
    pub pivot: FyVec2,
    /// 9-slice: its left, bottom, right and top borders, in the texture's
    /// pixels; all 0 for none.
    pub slice: [f32; 4],
    /// The texture's pixels a metre, for the slice's corners.
    pub pixels_per_metre: f32,
    /// 0: see-through by alpha; above 0, cut out below it.
    pub alpha_cutoff: f32,
    /// One of the `FY_FACING_*`.
    pub facing: u32,
    /// Drawn by these first, lower first, then by distance.
    pub sorting_layer: i32,
    pub order: i32,
    /// Lit as the scene is.
    pub lit: bool,
}

impl FySpriteSettings {
    fn settings(&self) -> Result<SpriteSettings, Error> {
        let texture = TextureId::from_raw(self.texture)
            .ok_or_else(|| Error::new(FyStatus::InvalidValue, "a sprite has no texture"))?;
        let facing = match self.facing {
            FY_FACING_NONE => Facing::None,
            FY_FACING_CAMERA => Facing::Camera,
            FY_FACING_UPRIGHT => Facing::Upright,
            FY_FACING_UP => Facing::Up,
            facing => {
                return Err(Error::new(
                    FyStatus::InvalidValue,
                    format!("facing is {facing}; it must be an FY_FACING_*"),
                ));
            }
        };
        Ok(SpriteSettings {
            texture,
            size: Vec2::new(self.size.x, self.size.y),
            color: Color::from(self.color),
            flip_x: self.flip_x,
            flip_y: self.flip_y,
            pivot: Vec2::new(self.pivot.x, self.pivot.y),
            slice: self.slice,
            pixels_per_metre: self.pixels_per_metre,
            alpha_cutoff: self.alpha_cutoff,
            facing,
            sorting_layer: self.sorting_layer,
            order: self.order,
            lit: self.lit,
        })
    }
}

/// Gives the object a sprite renderer with `settings`, or changes the one
/// it has. Settings that can't be are `FY_STATUS_INVALID_VALUE`, and a
/// texture the world doesn't have `FY_STATUS_NO_TEXTURE`; both change
/// nothing.
///
/// # Safety
///
/// `world` is null or live; `settings` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_sprite_renderer_set(
    world: *const FyWorld,
    id: u64,
    settings: *const FySpriteSettings,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, settings) =
            unsafe { (handle(world, "world")?, settings.as_ref().ok_or_else(|| Error::null("settings"))?) };
        world.lock()?.set_sprite_renderer(object(id)?, settings.settings()?)?;
        Ok(())
    })
}

/// Takes the object's sprite renderer away.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_sprite_renderer_remove(world: *const FyWorld, id: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        world.lock()?.remove_sprite_renderer(object(id)?)?;
        Ok(())
    })
}
