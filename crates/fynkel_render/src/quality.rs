//! Quality settings (blueprint §19.2d item 5, D3): the renderer's first
//! read of the project's `quality` profiles. A [`Quality`] is what the
//! renderer actually applies — a profile clamped to what V1 implements
//! (MSAA 1 or 4, a power-of-two shadow map, a bounded render scale).

use fynkel_assets::QualityProfile;

/// The largest render scale a profile may ask for (supersampling).
pub const MAX_RENDER_SCALE: f32 = 2.0;
/// The smallest.
pub const MIN_RENDER_SCALE: f32 = 0.25;
/// Shadow map side bounds (texels).
pub const MIN_SHADOW_RESOLUTION: u32 = 256;
pub const MAX_SHADOW_RESOLUTION: u32 = 8192;

/// What the renderer runs with. Built from a [`QualityProfile`] (or the
/// engine defaults) and clamped by [`Quality::clamped`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quality {
    /// Draw the sun's shadow map and sample it.
    pub shadows: bool,
    /// Shadow map side in texels (power of two).
    pub shadow_resolution: u32,
    /// Samples per pixel of the scene passes: 1 or 4.
    pub msaa: u32,
    /// Internal resolution relative to the target (1.0 = native).
    pub render_scale: f32,
    /// Anisotropic filtering of the material maps (1 = off).
    pub anisotropy: u32,
}

impl Default for Quality {
    fn default() -> Self {
        Quality::from(&QualityProfile::engine_default())
    }
}

impl From<&QualityProfile> for Quality {
    fn from(p: &QualityProfile) -> Self {
        Quality {
            shadows: p.shadows,
            shadow_resolution: p.shadow_resolution,
            msaa: p.msaa,
            render_scale: p.render_scale as f32,
            anisotropy: p.anisotropy,
        }
        .clamped()
    }
}

impl Quality {
    /// The settings V1 can honour: `msaa` 1 below 4, 4 at and above (8 is
    /// a V2 concern); `shadow_resolution` rounded up to a power of two in
    /// `256..=8192`; `render_scale` in `0.25..=2`; `anisotropy` in
    /// `1..=16`.
    pub fn clamped(self) -> Self {
        let render_scale = if self.render_scale.is_finite() {
            self.render_scale.clamp(MIN_RENDER_SCALE, MAX_RENDER_SCALE)
        } else {
            1.0
        };
        Quality {
            shadows: self.shadows,
            shadow_resolution: self
                .shadow_resolution
                .clamp(MIN_SHADOW_RESOLUTION, MAX_SHADOW_RESOLUTION)
                .next_power_of_two(),
            msaa: if self.msaa >= 4 { 4 } else { 1 },
            render_scale,
            anisotropy: self.anisotropy.clamp(1, 16),
        }
    }

    /// The internal size the scene passes render at for a `width` ×
    /// `height` target.
    pub fn internal_size(&self, width: u32, height: u32) -> (u32, u32) {
        let scale = |v: u32| ((v as f32 * self.render_scale).round() as u32).max(1);
        (scale(width), scale(height))
    }
}

/// Where the active [`Quality`] came from.
#[derive(Clone, Debug, PartialEq)]
pub struct QualityStatus {
    /// The profile name (`"default"` for the engine defaults, `"custom"`
    /// for an explicit [`Quality`]).
    pub profile: String,
    /// The requested profile name when it was not found in the project
    /// (the project default applies meanwhile).
    pub unknown_profile: Option<String>,
    pub settings: Quality,
    /// The internal render size (target size × render scale).
    pub internal_size: (u32, u32),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_document_defaults() {
        let q = Quality::default();
        assert_eq!(
            q,
            Quality {
                shadows: true,
                shadow_resolution: 2048,
                msaa: 4,
                render_scale: 1.0,
                anisotropy: 8,
            }
        );
        assert_eq!(q.internal_size(1280, 720), (1280, 720));
    }

    #[test]
    fn profiles_are_clamped_to_what_v1_implements() {
        let p = QualityProfile {
            name: "wild".into(),
            shadows: true,
            shadow_resolution: 3000,
            msaa: 8,
            render_scale: 9.0,
            anisotropy: 64,
            texture_max_size: 16,
        };
        let q = Quality::from(&p);
        assert_eq!(q.shadow_resolution, 4096);
        assert_eq!(q.msaa, 4);
        assert_eq!(q.render_scale, MAX_RENDER_SCALE);
        assert_eq!(q.anisotropy, 16);
        let low = Quality {
            msaa: 2,
            render_scale: 0.0,
            shadow_resolution: 1,
            ..q
        }
        .clamped();
        assert_eq!(low.msaa, 1);
        assert_eq!(low.render_scale, MIN_RENDER_SCALE);
        assert_eq!(low.shadow_resolution, MIN_SHADOW_RESOLUTION);
        assert_eq!(
            Quality {
                render_scale: f32::NAN,
                ..q
            }
            .clamped()
            .render_scale,
            1.0
        );
        assert_eq!(low.internal_size(128, 100), (32, 25));
    }
}
