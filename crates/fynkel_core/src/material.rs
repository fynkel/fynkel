//! The material model (blueprint §6, §19.2d scope rule "Rust is the source
//! of truth for materials"): [`PbrMaterial`] is the glTF 2.0
//! metallic-roughness material as a `#[derive(Schema)]` record. Its
//! document form is `.material.json` for the built-in `fynkel.pbr@1`
//! shader (`schemas/material.schema.json` is generated from it, every field
//! optional with the `Default` below), its wire form is what `fynkel cook`
//! stores and the renderer consumes directly.
//!
//! Units: colours are linear RGB; `emissive × emissive_strength` is
//! relative to the scene's exposure (a strength above 1 is HDR);
//! textures carry their colour space in the cache (`base_color` and
//! `emissive` sRGB, the rest linear data).

use fynkel_schema_derive::Schema;
use glam::{Vec3, Vec4};

use fynkel_contracts::AssetRef;

/// How the alpha channel is used (glTF `alphaMode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Schema)]
#[schema(name = "fynkel.AlphaMode")]
pub enum AlphaMode {
    /// Alpha is ignored; the surface is fully opaque.
    #[default]
    Opaque,
    /// Alpha below `alpha_cutoff` discards the fragment; otherwise opaque.
    Mask,
    /// Alpha blends the surface over what is behind it (sorted back to front).
    Blend,
}

/// The built-in PBR material (`fynkel.pbr@1`): factors multiply the
/// matching textures; an unbound texture behaves as white (normal: flat).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.PbrMaterial", document = "material")]
pub struct PbrMaterial {
    /// Linear RGBA factor; multiplies `base_color_texture`.
    pub base_color: Vec4,
    /// `0` dielectric … `1` metal; multiplies the B channel of `metallic_roughness_texture`.
    pub metallic: f32,
    /// `0` mirror … `1` fully rough; multiplies the G channel of `metallic_roughness_texture`.
    pub roughness: f32,
    /// Linear RGB emitted light; multiplies `emissive_texture`.
    pub emissive: Vec3,
    /// Scales `emissive`; values above 1 are HDR (`KHR_materials_emissive_strength`).
    pub emissive_strength: f32,
    /// Scales the tangent-space normal's XY.
    pub normal_scale: f32,
    /// `0` ignores the occlusion texture … `1` applies it fully.
    pub occlusion_strength: f32,
    pub alpha_mode: AlphaMode,
    /// Used by `mask` only.
    pub alpha_cutoff: f32,
    /// Draw both faces (no back-face culling); flips the normal on the back face.
    pub double_sided: bool,
    /// sRGB colour texture.
    pub base_color_texture: Option<AssetRef>,
    /// Tangent-space normal map, OpenGL convention (+Y up), linear.
    pub normal_texture: Option<AssetRef>,
    /// glTF packing, linear: G = roughness, B = metallic.
    pub metallic_roughness_texture: Option<AssetRef>,
    /// Ambient occlusion in R, linear.
    pub occlusion_texture: Option<AssetRef>,
    /// sRGB emissive colour texture.
    pub emissive_texture: Option<AssetRef>,
}

impl Default for PbrMaterial {
    fn default() -> Self {
        Self {
            base_color: Vec4::ONE,
            metallic: 0.0,
            roughness: 0.5,
            emissive: Vec3::ZERO,
            emissive_strength: 1.0,
            normal_scale: 1.0,
            occlusion_strength: 1.0,
            alpha_mode: AlphaMode::Opaque,
            alpha_cutoff: 0.5,
            double_sided: false,
            base_color_texture: None,
            normal_texture: None,
            metallic_roughness_texture: None,
            occlusion_texture: None,
            emissive_texture: None,
        }
    }
}

impl PbrMaterial {
    /// The five texture slots in declaration order: (document field, sRGB?, value).
    pub fn textures(&self) -> [(&'static str, bool, Option<&AssetRef>); 5] {
        [
            ("base_color_texture", true, self.base_color_texture.as_ref()),
            ("normal_texture", false, self.normal_texture.as_ref()),
            (
                "metallic_roughness_texture",
                false,
                self.metallic_roughness_texture.as_ref(),
            ),
            ("occlusion_texture", false, self.occlusion_texture.as_ref()),
            ("emissive_texture", true, self.emissive_texture.as_ref()),
        ]
    }

    /// Value rules the schema cannot express, as `(field, message)`: every
    /// factor finite, unit factors within `0..=1`, strengths non-negative.
    pub fn violations(&self) -> Vec<(&'static str, String)> {
        fn unit(what: &'static str, v: f32, out: &mut Vec<(&'static str, String)>) {
            if !v.is_finite() || !(0.0..=1.0).contains(&v) {
                out.push((what, format!("{what} must be within 0..=1, got {v}")));
            }
        }
        fn non_negative(what: &'static str, v: f32, out: &mut Vec<(&'static str, String)>) {
            if !v.is_finite() || v < 0.0 {
                out.push((what, format!("{what} must be finite and >= 0, got {v}")));
            }
        }
        fn unit_color(what: &'static str, v: &[f32], out: &mut Vec<(&'static str, String)>) {
            if v.iter().any(|c| !c.is_finite() || !(0.0..=1.0).contains(c)) {
                out.push((
                    what,
                    format!("{what} components must be within 0..=1, got {v:?}"),
                ));
            }
        }
        let mut out = Vec::new();
        unit_color("base_color", &self.base_color.to_array(), &mut out);
        unit("metallic", self.metallic, &mut out);
        unit("roughness", self.roughness, &mut out);
        unit_color("emissive", &self.emissive.to_array(), &mut out);
        non_negative("emissive_strength", self.emissive_strength, &mut out);
        non_negative("normal_scale", self.normal_scale, &mut out);
        unit("occlusion_strength", self.occlusion_strength, &mut out);
        unit("alpha_cutoff", self.alpha_cutoff, &mut out);
        out
    }

    /// The first violation's message, if any (the `ComponentValue::validate` shape).
    pub fn validate(&self) -> Result<(), String> {
        match self.violations().into_iter().next() {
            Some((_, why)) => Err(why),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fynkel_contracts::schema::{FieldType, HasSchema};

    #[test]
    fn schema_shape() {
        let s = &PbrMaterial::SCHEMA;
        assert_eq!(s.document, Some("material"));
        assert!(matches!(s.fields[0].ty, FieldType::Vec4));
        assert!(matches!(s.fields[10].ty, FieldType::OptionalAssetRef));
        assert!(s.has_strings());
        // vec4 @0, 2 f32 @16, vec3 @24, 3 f32 @36, enum @48, f32 @52, bool @56,
        // five asset refs (8-aligned) @64..184, padded to 184.
        assert_eq!(s.wire_size(), 184);
        assert_eq!(s.offsets()[10], 64);
    }

    #[test]
    fn defaults_validate_and_ranges_are_checked() {
        assert_eq!(PbrMaterial::default().validate(), Ok(()));
        let m = PbrMaterial {
            roughness: 1.5,
            ..PbrMaterial::default()
        };
        assert!(m.validate().unwrap_err().contains("roughness"));
        let m = PbrMaterial {
            emissive_strength: -1.0,
            ..PbrMaterial::default()
        };
        assert!(m.validate().unwrap_err().contains("emissive_strength"));
        let m = PbrMaterial {
            base_color: Vec4::new(2.0, 0.0, 0.0, 1.0),
            ..PbrMaterial::default()
        };
        assert!(m.validate().unwrap_err().contains("base_color"));
    }
}
