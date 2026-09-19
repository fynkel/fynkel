//! Starter native components (blueprint §19.2c item 4, §8.5): the storage
//! half of camera, mesh rendering and lights. Values are authored in scene
//! documents (`schemas/components/*.schema.json`) and cross the bridge with
//! the generated codecs; nothing reads them until the M3 renderer. Units:
//! metres, radians, linear RGB.

use fynkel_contracts::AssetRef;
use fynkel_schema_derive::Schema;
use glam::Vec3;

/// Camera projection model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Schema)]
#[schema(name = "fynkel.Projection")]
pub enum Projection {
    Perspective,
    Orthographic,
}

/// A camera looking down its entity's local -Z (blueprint §8.5:
/// perspective and orthographic).
#[derive(Clone, Copy, Debug, PartialEq, Schema)]
#[schema(kind = "component", id = 5, name = "fynkel.Camera")]
pub struct Camera {
    pub projection: Projection,
    /// Vertical field of view in radians (perspective only).
    pub fov_y: f32,
    /// Visible height in metres (orthographic only).
    pub ortho_height: f32,
    /// Near clip distance in metres, `> 0`.
    pub near: f32,
    /// Far clip distance in metres, `> near`.
    pub far: f32,
    /// Higher renders on top when several cameras are enabled.
    pub priority: i32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            projection: Projection::Perspective,
            fov_y: 60f32.to_radians(),
            ortho_height: 10.0,
            near: 0.1,
            far: 1000.0,
            priority: 0,
        }
    }
}

/// Draws a mesh sub-asset with a material.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "component", id = 4, name = "fynkel.MeshRenderer")]
pub struct MeshRenderer {
    /// Mesh asset (an imported glTF `mesh/<n>` sub-asset).
    pub mesh: AssetRef,
    /// `.material.json` asset.
    pub material: AssetRef,
    pub cast_shadows: bool,
    pub receive_shadows: bool,
    /// `false` keeps the component but skips drawing.
    pub visible: bool,
}

impl Default for MeshRenderer {
    fn default() -> Self {
        Self {
            mesh: AssetRef::default(),
            material: AssetRef::default(),
            cast_shadows: true,
            receive_shadows: true,
            visible: true,
        }
    }
}

/// Light shining along the entity's local -Z; the one shadow caster of the
/// V1 renderer.
#[derive(Clone, Copy, Debug, PartialEq, Schema)]
#[schema(kind = "component", id = 6, name = "fynkel.DirectionalLight")]
pub struct DirectionalLight {
    /// Linear RGB, each `0..=1`.
    pub color: Vec3,
    /// Illuminance in lux.
    pub intensity: f32,
    pub cast_shadows: bool,
}

impl Default for DirectionalLight {
    fn default() -> Self {
        Self {
            color: Vec3::ONE,
            intensity: 10_000.0,
            cast_shadows: true,
        }
    }
}

/// Omnidirectional light at the entity's position (no shadows in V1).
#[derive(Clone, Copy, Debug, PartialEq, Schema)]
#[schema(kind = "component", id = 7, name = "fynkel.PointLight")]
pub struct PointLight {
    /// Linear RGB, each `0..=1`.
    pub color: Vec3,
    /// Luminous intensity in candela.
    pub intensity: f32,
    /// Metres; no contribution beyond this distance.
    pub range: f32,
}

impl Default for PointLight {
    fn default() -> Self {
        Self {
            color: Vec3::ONE,
            intensity: 800.0,
            range: 10.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fynkel_contracts::schema::{FieldType, HasEnumSchema, HasSchema};

    #[test]
    fn projection_is_an_enum_field() {
        assert_eq!(Projection::ENUM.variants, &["perspective", "orthographic"]);
        assert_eq!(Projection::Orthographic.to_index(), 1);
        assert_eq!(Projection::from_index(0), Some(Projection::Perspective));
        assert_eq!(Projection::from_index(2), None);
        assert!(
            matches!(Camera::SCHEMA.fields[0].ty, FieldType::Enum(e) if e.name == "fynkel.Projection")
        );
        assert_eq!(
            Camera::SCHEMA.canonical(),
            "component:5:fynkel.Camera[projection:fynkel.Projection{perspective|orthographic},fov_y:f32,ortho_height:f32,near:f32,far:f32,priority:i32]"
        );
        assert_eq!(Camera::SCHEMA.wire_size(), 24);
    }

    #[test]
    fn mesh_renderer_layout() {
        let s = &MeshRenderer::SCHEMA;
        assert!(matches!(s.fields[0].ty, FieldType::AssetRef));
        // mesh 24 @0, material 24 @24, three bools @48..51, padded to 56.
        assert_eq!(s.offsets(), vec![0, 24, 48, 49, 50]);
        assert_eq!(s.wire_size(), 56);
        assert!(s.has_strings());
    }
}
