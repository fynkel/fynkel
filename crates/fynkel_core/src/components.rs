//! Starter native components (blueprint §19.2c item 4, §8.5): the storage
//! half of camera, mesh rendering, lights and the environment. Values are
//! authored in scene documents (`schemas/components/*.schema.json`) and
//! cross the bridge with the generated codecs; `fynkel_render` reads them
//! from the extract. Units (§19.2d scope rules): metres, radians, linear
//! RGB; illuminance in lux (directional, ambient), luminous intensity in
//! candela (point, spot), luminance in nits (sky); exposure in EV100.

use fynkel_contracts::{AssetRef, ComponentId};
use fynkel_physics::{Collider, RigidBody};
use fynkel_scene::Transform;
use fynkel_schema_derive::Schema;
use glam::Vec3;

/// Camera projection model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Schema)]
#[schema(name = "fynkel.Projection")]
pub enum Projection {
    Perspective,
    Orthographic,
}

/// Where a camera's exposure comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Schema)]
#[schema(name = "fynkel.ExposureMode")]
pub enum ExposureMode {
    /// EV100 derived from the scene's key light (the brightest
    /// `DirectionalLight`, else the ambient term) so a sunlit white surface
    /// renders bright without clipping; `exposure` is ignored.
    #[default]
    Auto,
    /// `exposure` is the EV100.
    Manual,
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
    pub exposure_mode: ExposureMode,
    /// EV100 when `exposure_mode` is `manual` (12 ≈ a 10 000 lux sun;
    /// +1 halves the brightness).
    pub exposure: f32,
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
            exposure_mode: ExposureMode::Auto,
            exposure: 12.0,
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

/// Cone of light along the entity's local -Z (no shadows in V1).
#[derive(Clone, Copy, Debug, PartialEq, Schema)]
#[schema(kind = "component", id = 8, name = "fynkel.SpotLight")]
pub struct SpotLight {
    /// Linear RGB, each `0..=1`.
    pub color: Vec3,
    /// Luminous intensity in candela along the axis.
    pub intensity: f32,
    /// Metres; no contribution beyond this distance.
    pub range: f32,
    /// Half-angle in radians inside which the light is full.
    pub inner_angle: f32,
    /// Half-angle in radians beyond which the light is zero, `>= inner_angle`.
    pub outer_angle: f32,
}

impl Default for SpotLight {
    fn default() -> Self {
        Self {
            color: Vec3::ONE,
            intensity: 1_500.0,
            range: 15.0,
            inner_angle: 25f32.to_radians(),
            outer_angle: 35f32.to_radians(),
        }
    }
}

/// The scene's environment lighting (one per scene; the first enabled
/// entity's wins): a constant ambient term and an optional sky drawn
/// behind everything. Without the component the renderer uses these
/// defaults.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "component", id = 9, name = "fynkel.Environment")]
pub struct Environment {
    /// Linear RGB, each `0..=1`.
    pub ambient_color: Vec3,
    /// Ambient illuminance in lux, from every direction.
    pub ambient_intensity: f32,
    /// Equirectangular sRGB texture asset drawn as the sky (no image-based
    /// lighting in V1: the sky is a backdrop, `ambient_*` lights surfaces).
    pub sky: Option<AssetRef>,
    /// Luminance in nits of a white sky texel.
    pub sky_intensity: f32,
    /// Rotation of the sky about +Y in radians.
    pub sky_rotation: f32,
}

impl Default for Environment {
    fn default() -> Self {
        Self {
            ambient_color: Vec3::ONE,
            ambient_intensity: 2_000.0,
            sky: None,
            sky_intensity: 5_000.0,
            sky_rotation: 0.0,
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
            "component:5:fynkel.Camera[projection:fynkel.Projection{perspective|orthographic},fov_y:f32,ortho_height:f32,near:f32,far:f32,priority:i32,exposure_mode:fynkel.ExposureMode{auto|manual},exposure:f32]"
        );
        assert_eq!(Camera::SCHEMA.wire_size(), 32);
    }

    #[test]
    fn value_rules() {
        assert!(ComponentValue::Camera(Camera::default()).validate().is_ok());
        let mut c = Camera::default();
        c.far = c.near;
        assert!(ComponentValue::Camera(c).validate().is_err());
        let mut c = Camera {
            fov_y: 0.0,
            ..Camera::default()
        };
        assert!(ComponentValue::Camera(c).validate().is_err());
        c.projection = Projection::Orthographic;
        assert!(
            ComponentValue::Camera(c).validate().is_ok(),
            "fov ignored for orthographic"
        );
        let l = PointLight {
            range: 0.0,
            ..PointLight::default()
        };
        assert!(ComponentValue::PointLight(l).validate().is_err());
        let d = DirectionalLight {
            color: Vec3::new(-1.0, 0.0, 0.0),
            ..DirectionalLight::default()
        };
        assert!(ComponentValue::DirectionalLight(d).validate().is_err());
        let c = Camera {
            exposure_mode: ExposureMode::Manual,
            exposure: f32::NAN,
            ..Camera::default()
        };
        assert!(ComponentValue::Camera(c).validate().is_err());
        assert!(ComponentValue::SpotLight(SpotLight::default())
            .validate()
            .is_ok());
        let s = SpotLight {
            inner_angle: 1.0,
            outer_angle: 0.5,
            ..SpotLight::default()
        };
        assert!(ComponentValue::SpotLight(s).validate().is_err());
        let s = SpotLight {
            outer_angle: 3.2,
            ..SpotLight::default()
        };
        assert!(ComponentValue::SpotLight(s).validate().is_err());
        assert!(ComponentValue::Environment(Environment::default())
            .validate()
            .is_ok());
        let env = Environment {
            sky_intensity: -1.0,
            ..Environment::default()
        };
        assert!(ComponentValue::Environment(env).validate().is_err());
        assert_eq!(
            ComponentValue::PointLight(PointLight::default()).type_name(),
            "fynkel.PointLight"
        );
    }

    #[test]
    fn light_and_environment_layouts() {
        assert_eq!(SpotLight::SCHEMA.wire_size(), 32);
        assert_eq!(SpotLight::SCHEMA.id, 8);
        let e = &Environment::SCHEMA;
        assert_eq!(e.id, 9);
        assert!(matches!(e.fields[2].ty, FieldType::OptionalAssetRef));
        // ambient_color 12 @0, ambient_intensity @12, sky 24 @16, two f32 @40.
        assert_eq!(e.offsets(), vec![0, 12, 16, 40, 44]);
        assert_eq!(e.wire_size(), 48);
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

/// One typed built-in component value: what a scene document instantiates
/// and what `Command::SetComponent` carries. `fynkel.Name` is not here —
/// it mirrors the entity's `name` (`Command::SetName`).
#[derive(Clone, Debug, PartialEq)]
pub enum ComponentValue {
    Transform(Transform),
    MeshRenderer(MeshRenderer),
    Camera(Camera),
    DirectionalLight(DirectionalLight),
    PointLight(PointLight),
    SpotLight(SpotLight),
    Environment(Environment),
    /// `fynkel_physics` (M3-08).
    RigidBody(RigidBody),
    Collider(Collider),
}

impl ComponentValue {
    pub fn id(&self) -> ComponentId {
        match self {
            ComponentValue::Transform(_) => ComponentId::TRANSFORM,
            ComponentValue::MeshRenderer(_) => ComponentId::MESH_RENDERER,
            ComponentValue::Camera(_) => ComponentId::CAMERA,
            ComponentValue::DirectionalLight(_) => ComponentId::DIRECTIONAL_LIGHT,
            ComponentValue::PointLight(_) => ComponentId::POINT_LIGHT,
            ComponentValue::SpotLight(_) => ComponentId::SPOT_LIGHT,
            ComponentValue::Environment(_) => ComponentId::ENVIRONMENT,
            ComponentValue::RigidBody(_) => ComponentId::RIGID_BODY,
            ComponentValue::Collider(_) => ComponentId::COLLIDER,
        }
    }

    pub fn type_name(&self) -> &'static str {
        self.id().name().unwrap()
    }

    /// Value rules the schema cannot express (ranges, finiteness). The
    /// message is the `INVALID_PAYLOAD` diagnostic text.
    pub fn validate(&self) -> Result<(), String> {
        fn finite_color(c: Vec3) -> Result<(), String> {
            if !c.is_finite() || c.min_element() < 0.0 {
                return Err(format!("color must be finite and non-negative, got {c:?}"));
            }
            Ok(())
        }
        fn non_negative(what: &str, v: f32) -> Result<(), String> {
            if !v.is_finite() || v < 0.0 {
                return Err(format!("{what} must be finite and >= 0, got {v}"));
            }
            Ok(())
        }
        fn positive(what: &str, v: f32) -> Result<(), String> {
            if !v.is_finite() || v <= 0.0 {
                return Err(format!("{what} must be finite and > 0, got {v}"));
            }
            Ok(())
        }
        match self {
            ComponentValue::Transform(t) => {
                if !t.is_finite() {
                    return Err("transform is not finite".into());
                }
            }
            ComponentValue::MeshRenderer(_) => {}
            ComponentValue::Camera(c) => {
                positive("near", c.near)?;
                if !c.far.is_finite() || c.far <= c.near {
                    return Err(format!(
                        "far must be finite and > near, got {} <= {}",
                        c.far, c.near
                    ));
                }
                match c.projection {
                    Projection::Perspective => {
                        if !c.fov_y.is_finite()
                            || c.fov_y <= 0.0
                            || c.fov_y >= core::f32::consts::PI
                        {
                            return Err(format!(
                                "fov_y must be in (0, pi) radians, got {}",
                                c.fov_y
                            ));
                        }
                    }
                    Projection::Orthographic => positive("ortho_height", c.ortho_height)?,
                }
                if c.exposure_mode == ExposureMode::Manual && !c.exposure.is_finite() {
                    return Err(format!("exposure must be finite, got {}", c.exposure));
                }
            }
            ComponentValue::DirectionalLight(l) => {
                finite_color(l.color)?;
                non_negative("intensity", l.intensity)?;
            }
            ComponentValue::PointLight(l) => {
                finite_color(l.color)?;
                non_negative("intensity", l.intensity)?;
                positive("range", l.range)?;
            }
            ComponentValue::SpotLight(l) => {
                finite_color(l.color)?;
                non_negative("intensity", l.intensity)?;
                positive("range", l.range)?;
                non_negative("inner_angle", l.inner_angle)?;
                if !l.outer_angle.is_finite()
                    || l.outer_angle <= 0.0
                    || l.outer_angle >= core::f32::consts::PI
                {
                    return Err(format!(
                        "outer_angle must be in (0, pi) radians, got {}",
                        l.outer_angle
                    ));
                }
                if l.inner_angle > l.outer_angle {
                    return Err(format!(
                        "inner_angle must be <= outer_angle, got {} > {}",
                        l.inner_angle, l.outer_angle
                    ));
                }
            }
            ComponentValue::Environment(e) => {
                finite_color(e.ambient_color)?;
                non_negative("ambient_intensity", e.ambient_intensity)?;
                non_negative("sky_intensity", e.sky_intensity)?;
                if !e.sky_rotation.is_finite() {
                    return Err(format!(
                        "sky_rotation must be finite, got {}",
                        e.sky_rotation
                    ));
                }
            }
            ComponentValue::RigidBody(b) => b.validate()?,
            ComponentValue::Collider(c) => c.validate()?,
        }
        Ok(())
    }
}
