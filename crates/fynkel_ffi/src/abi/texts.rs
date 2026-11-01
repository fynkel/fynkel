//! Flat text in the scene (stage 15): fonts the world keeps, and text
//! renderers drawn from their atlases.

use fynkel_world::{Color, Facing, FontId, TextAlign, TextSettings};

use crate::error::{Error, FyStatus, guard};

use super::*;

// --- Flat text ---------------------------------------------------------------

/// As the object turns: its front along the object's +Z.
pub const FY_FACING_NONE: u32 = 0;
/// Square to the camera, its top the camera's up.
pub const FY_FACING_CAMERA: u32 = 1;
/// Turned round up to face the camera, standing upright.
pub const FY_FACING_UPRIGHT: u32 = 2;
/// Flat, facing up, its top away along -Z.
pub const FY_FACING_UP: u32 = 3;

/// A text renderer's settings (TextMeshPro's), as
/// `fynkel_world::TextSettings`: metres, colours sRGB.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FyTextSettings {
    /// A font from `fy_font_add`.
    pub font: u64,
    /// The font's em, in metres.
    pub size: f32,
    /// `FY_TEXT_LEFT`, `FY_TEXT_CENTER` or `FY_TEXT_RIGHT`.
    pub align: u32,
    pub line_spacing: f32,
    pub color: FyColor,
    /// The outline's width, 0 to 1 of the atlas's reach round a letter.
    pub outline: f32,
    pub outline_color: FyColor,
    /// How soft the edges are, 0 to 1.
    pub softness: f32,
    /// One of the `FY_FACING_*`.
    pub facing: u32,
    /// Drawn over everything.
    pub on_top: bool,
    /// Lit as the scene is.
    pub lit: bool,
}

impl FyTextSettings {
    fn settings(&self) -> Result<TextSettings, Error> {
        let font =
            FontId::from_raw(self.font).ok_or_else(|| Error::new(FyStatus::InvalidValue, "a text has no font"))?;
        let align = match self.align {
            FY_TEXT_LEFT => TextAlign::Left,
            FY_TEXT_CENTER => TextAlign::Center,
            FY_TEXT_RIGHT => TextAlign::Right,
            align => {
                return Err(Error::new(
                    FyStatus::InvalidValue,
                    format!("align is {align}; it must be FY_TEXT_LEFT, FY_TEXT_CENTER or FY_TEXT_RIGHT"),
                ));
            }
        };
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
        Ok(TextSettings {
            font,
            size: self.size,
            align,
            line_spacing: self.line_spacing,
            color: Color::from(self.color),
            outline: self.outline,
            outline_color: Color::from(self.outline_color),
            softness: self.softness,
            facing,
            on_top: self.on_top,
            lit: self.lit,
        })
    }
}

/// Keeps the TrueType (.ttf) or OpenType (.otf) font in `len` bytes for
/// flat text, copying them, and writes its id to `out`.
/// `FY_STATUS_INVALID_VALUE` for bytes that aren't one.
///
/// # Safety
///
/// `world` is null or live; `bytes` is valid for `len` bytes (it may be
/// null when `len` is 0); `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_font_add(world: *const FyWorld, bytes: *const u8, len: usize, out: *mut u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, out) = unsafe { (handle(world, "world")?, out.as_mut().ok_or_else(|| Error::null("out"))?) };
        let data: &[u8] = if len == 0 {
            &[]
        } else {
            // SAFETY: per this function's contract.
            unsafe { std::slice::from_raw_parts(handle(bytes, "bytes")?, len) }
        };
        *out = world.lock()?.add_font(data.into())?.raw();
        Ok(())
    })
}

/// Frees a font; `FY_STATUS_IN_USE` while a text uses it.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_font_remove(world: *const FyWorld, font: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let id = FontId::from_raw(font).ok_or_else(|| Error::new(FyStatus::InvalidValue, "font is 0"))?;
        world.lock()?.remove_font(id)?;
        Ok(())
    })
}

/// Gives the object a text renderer with `settings` showing `text`
/// (UTF-8), or changes the one it has. Settings that can't be are
/// `FY_STATUS_INVALID_VALUE` and change nothing.
///
/// # Safety
///
/// `world` is null or live; `settings` is null or readable; `text` is
/// valid for `text_len` bytes (it may be null when `text_len` is 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_text_renderer_set(
    world: *const FyWorld,
    id: u64,
    settings: *const FyTextSettings,
    text: *const u8,
    text_len: usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, settings) =
            unsafe { (handle(world, "world")?, settings.as_ref().ok_or_else(|| Error::null("settings"))?) };
        // SAFETY: per this function's contract.
        let text = unsafe { utf8(text, text_len, "text") }?;
        world.lock()?.set_text_renderer(object(id)?, settings.settings()?, text)?;
        Ok(())
    })
}

/// Changes only what the object's text renderer shows, to `text` (UTF-8):
/// the same text again does nothing.
///
/// # Safety
///
/// `world` is null or live; `text` is valid for `text_len` bytes (it may
/// be null when `text_len` is 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_text_renderer_set_text(
    world: *const FyWorld,
    id: u64,
    text: *const u8,
    text_len: usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        // SAFETY: per this function's contract.
        let text = unsafe { utf8(text, text_len, "text") }?;
        world.lock()?.set_text(object(id)?, text)?;
        Ok(())
    })
}

/// Takes the object's text renderer away.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_text_renderer_remove(world: *const FyWorld, id: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        world.lock()?.remove_text_renderer(object(id)?)?;
        Ok(())
    })
}
