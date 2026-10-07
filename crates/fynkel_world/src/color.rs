//! Colours as people pick them: sRGB, 0–1 per channel.

/// A colour in sRGB, as typed into a colour picker or written as
/// `0xFFE0613A`. Lighting maths needs linear values. [`Color::to_linear`]
/// converts, and the renderer calls it once per colour, never per pixel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const WHITE: Self = Self::rgb(1.0, 1.0, 1.0);
    pub const BLACK: Self = Self::rgb(0.0, 0.0, 0.0);

    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// From `0xAARRGGBB`, the layout Flutter's `Color(0xFF…)` uses.
    pub const fn from_argb32(argb: u32) -> Self {
        const fn channel(argb: u32, shift: u32) -> f32 {
            ((argb >> shift) & 0xFF) as f32 / 255.0
        }
        Self { r: channel(argb, 16), g: channel(argb, 8), b: channel(argb, 0), a: channel(argb, 24) }
    }

    /// Linear RGB (the exact sRGB transfer function), alpha unchanged.
    pub fn to_linear(self) -> [f32; 4] {
        fn channel(c: f32) -> f32 {
            if c <= 0.040_45 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        }
        [channel(self.r), channel(self.g), channel(self.b), self.a]
    }
}
