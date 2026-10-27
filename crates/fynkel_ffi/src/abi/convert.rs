//! Conversions between the plain C types and the engine's own.

use fynkel_world::{
    Blend, Camera, Color, Component, Light, LightKind, Material, MaterialId, MeshId, MeshRenderer, Quat, ShaderId,
    TextureId, Transform, Vec2, Vec3,
};

use crate::error::{Error, FyStatus};

use super::*;

// --- Conversions ---------------------------------------------------------------

impl From<FyVec2> for Vec2 {
    fn from(v: FyVec2) -> Self {
        Vec2::new(v.x, v.y)
    }
}

impl From<Vec2> for FyVec2 {
    fn from(v: Vec2) -> Self {
        Self { x: v.x, y: v.y }
    }
}

impl From<FyVec3> for Vec3 {
    fn from(v: FyVec3) -> Self {
        Vec3::new(v.x, v.y, v.z)
    }
}

impl From<Vec3> for FyVec3 {
    fn from(v: Vec3) -> Self {
        Self { x: v.x, y: v.y, z: v.z }
    }
}

impl From<FyQuat> for Quat {
    fn from(q: FyQuat) -> Self {
        Quat::from_xyzw(q.x, q.y, q.z, q.w)
    }
}

impl From<Quat> for FyQuat {
    fn from(q: Quat) -> Self {
        Self { x: q.x, y: q.y, z: q.z, w: q.w }
    }
}

impl From<FyColor> for Color {
    fn from(c: FyColor) -> Self {
        Color::rgba(c.r, c.g, c.b, c.a)
    }
}

impl From<Color> for FyColor {
    fn from(c: Color) -> Self {
        Self { r: c.r, g: c.g, b: c.b, a: c.a }
    }
}

/// A C struct that stands for a component.
pub(super) trait CComponent: Copy {
    type Component: Component;
    fn to_component(self) -> Result<Self::Component, Error>;
    fn from_component(c: Self::Component) -> Self;
}

impl CComponent for FyTransform {
    type Component = Transform;
    fn to_component(self) -> Result<Transform, Error> {
        Ok(Transform { position: self.position.into(), rotation: self.rotation.into(), scale: self.scale.into() })
    }
    fn from_component(t: Transform) -> Self {
        Self { position: t.position.into(), rotation: t.rotation.into(), scale: t.scale.into() }
    }
}

impl CComponent for FyMeshRenderer {
    type Component = MeshRenderer;
    fn to_component(self) -> Result<MeshRenderer, Error> {
        Ok(MeshRenderer {
            mesh: MeshId::from_raw(self.mesh),
            material: MaterialId::from_raw(self.material),
            own_color: self.own_color,
            color: self.color.into(),
            cast_shadows: self.cast_shadows,
            receive_shadows: self.receive_shadows,
            own: self.own,
        })
    }
    fn from_component(r: MeshRenderer) -> Self {
        Self {
            mesh: r.mesh.map_or(0, MeshId::raw),
            material: r.material.map_or(0, MaterialId::raw),
            own_color: r.own_color,
            color: r.color.into(),
            cast_shadows: r.cast_shadows,
            receive_shadows: r.receive_shadows,
            own: r.own,
        }
    }
}

impl CComponent for FyCamera {
    type Component = Camera;
    fn to_component(self) -> Result<Camera, Error> {
        Ok(Camera {
            fov_y: self.fov_y,
            near: self.near_clip,
            far: self.far_clip,
            clear_color: self.clear_color.into(),
            orthographic: self.orthographic,
            size: self.size,
            culling_mask: self.culling_mask,
            show_sky: self.show_sky,
            bloom: self.bloom,
            bloom_strength: self.bloom_strength,
            bloom_threshold: self.bloom_threshold,
            ambient_occlusion: self.ambient_occlusion,
            ambient_occlusion_intensity: self.ambient_occlusion_intensity,
            ambient_occlusion_radius: self.ambient_occlusion_radius,
            pixels: self.pixels,
        })
    }
    fn from_component(c: Camera) -> Self {
        Self {
            fov_y: c.fov_y,
            near_clip: c.near,
            far_clip: c.far,
            clear_color: c.clear_color.into(),
            orthographic: c.orthographic,
            size: c.size,
            culling_mask: c.culling_mask,
            show_sky: c.show_sky,
            bloom: c.bloom,
            bloom_strength: c.bloom_strength,
            bloom_threshold: c.bloom_threshold,
            ambient_occlusion: c.ambient_occlusion,
            ambient_occlusion_intensity: c.ambient_occlusion_intensity,
            ambient_occlusion_radius: c.ambient_occlusion_radius,
            pixels: c.pixels,
        }
    }
}

impl CComponent for FyLight {
    type Component = Light;
    fn to_component(self) -> Result<Light, Error> {
        let kind = match self.kind {
            FY_LIGHT_DIRECTIONAL => LightKind::Directional,
            FY_LIGHT_AMBIENT => LightKind::Ambient,
            FY_LIGHT_POINT => LightKind::Point,
            FY_LIGHT_SPOT => LightKind::Spot,
            other => {
                return Err(Error::new(
                    FyStatus::InvalidValue,
                    format!("Light.kind {other} doesn't exist (0 = directional, 1 = ambient, 2 = point, 3 = spot)"),
                ));
            }
        };
        Ok(Light {
            kind,
            color: self.color.into(),
            intensity: self.intensity,
            range: self.range,
            spot_angle: self.spot_angle,
            softness: self.softness,
            shadows: self.shadows,
        })
    }
    fn from_component(l: Light) -> Self {
        let kind = match l.kind {
            LightKind::Directional => FY_LIGHT_DIRECTIONAL,
            LightKind::Ambient => FY_LIGHT_AMBIENT,
            LightKind::Point => FY_LIGHT_POINT,
            LightKind::Spot => FY_LIGHT_SPOT,
        };
        Self {
            kind,
            color: l.color.into(),
            intensity: l.intensity,
            range: l.range,
            spot_angle: l.spot_angle,
            softness: l.softness,
            shadows: l.shadows,
        }
    }
}

impl Default for FyLight {
    fn default() -> Self {
        Self::from_component(Light::DEFAULT)
    }
}

impl Default for FyMaterial {
    fn default() -> Self {
        Material::DEFAULT.into()
    }
}

impl FyMaterial {
    /// The world's material, or what's wrong with this one.
    pub(super) fn to_material(self) -> Result<Material, Error> {
        let blend = match self.blend {
            FY_BLEND_OPAQUE => Blend::Opaque,
            FY_BLEND_TRANSPARENT => Blend::Transparent,
            FY_BLEND_ADDITIVE => Blend::Additive,
            other => return Err(Error::new(FyStatus::InvalidValue, format!("{other} isn't a kind of blend"))),
        };
        Ok(Material {
            base_color: self.base_color.into(),
            metallic: self.metallic,
            roughness: self.roughness,
            emissive: self.emissive.into(),
            emissive_intensity: self.emissive_intensity,
            uv_scale: self.uv_scale.into(),
            uv_offset: self.uv_offset.into(),
            alpha_cutoff: self.alpha_cutoff,
            double_sided: self.double_sided,
            blend,
            base_color_texture: TextureId::from_raw(self.base_color_texture),
            normal_texture: TextureId::from_raw(self.normal_texture),
            metallic_roughness_texture: TextureId::from_raw(self.metallic_roughness_texture),
            occlusion_texture: TextureId::from_raw(self.occlusion_texture),
            emissive_texture: TextureId::from_raw(self.emissive_texture),
            shader: ShaderId::from_raw(self.shader),
            shader_textures: self.shader_textures.map(TextureId::from_raw),
            reach: self.reach,
            outline_width: self.outline_width,
            outline_color: self.outline_color.into(),
        })
    }
}

impl From<Material> for FyMaterial {
    fn from(m: Material) -> Self {
        let raw = |id: Option<TextureId>| id.map_or(0, TextureId::raw);
        Self {
            base_color: m.base_color.into(),
            metallic: m.metallic,
            roughness: m.roughness,
            emissive: m.emissive.into(),
            emissive_intensity: m.emissive_intensity,
            uv_scale: m.uv_scale.into(),
            uv_offset: m.uv_offset.into(),
            alpha_cutoff: m.alpha_cutoff,
            double_sided: m.double_sided,
            blend: match m.blend {
                Blend::Opaque => FY_BLEND_OPAQUE,
                Blend::Transparent => FY_BLEND_TRANSPARENT,
                Blend::Additive => FY_BLEND_ADDITIVE,
            },
            base_color_texture: raw(m.base_color_texture),
            normal_texture: raw(m.normal_texture),
            metallic_roughness_texture: raw(m.metallic_roughness_texture),
            occlusion_texture: raw(m.occlusion_texture),
            emissive_texture: raw(m.emissive_texture),
            shader: m.shader.map_or(0, ShaderId::raw),
            shader_textures: m.shader_textures.map(raw),
            reach: m.reach,
            outline_width: m.outline_width,
            outline_color: m.outline_color.into(),
        }
    }
}
