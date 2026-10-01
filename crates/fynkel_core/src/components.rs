//! Starter native components (blueprint §19.2c item 4, §8.5): the storage
//! half of camera, mesh rendering, lights and the environment. Values are
//! authored in scene documents (`schemas/components/*.schema.json`) and
//! cross the bridge with the generated codecs; `fynkel_render` reads them
//! from the extract. Units (§19.2d scope rules): metres, radians, linear
//! RGB; illuminance in lux (directional, ambient), luminous intensity in
//! candela (point, spot), luminance in nits (sky); exposure in EV100.

use fynkel_contracts::{AssetRef, ComponentId, ObjectRef};
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
    #[fynkel(group = "Lens", order = 0)]
    pub projection: Projection,
    /// Vertical field of view in radians (perspective only).
    #[fynkel(
        label = "Field of view",
        kind = "angle",
        unit = "deg",
        min = 0.01,
        max = 3.1,
        group = "Lens",
        order = 1
    )]
    pub fov_y: f32,
    /// Visible height in metres (orthographic only).
    #[fynkel(
        label = "Orthographic height",
        unit = "m",
        min = 0,
        group = "Lens",
        order = 2
    )]
    pub ortho_height: f32,
    /// Near clip distance in metres, `> 0`.
    #[fynkel(unit = "m", min = 0, group = "Clipping", order = 3)]
    pub near: f32,
    /// Far clip distance in metres, `> near`.
    #[fynkel(unit = "m", min = 0, group = "Clipping", order = 4)]
    pub far: f32,
    /// Higher renders on top when several cameras are enabled.
    #[fynkel(group = "Output", order = 7)]
    pub priority: i32,
    #[fynkel(group = "Exposure", order = 5)]
    pub exposure_mode: ExposureMode,
    /// EV100 when `exposure_mode` is `manual` (12 ≈ a 10 000 lux sun;
    /// +1 halves the brightness).
    #[fynkel(label = "Exposure (EV100)", group = "Exposure", order = 6, step = 0.1)]
    pub exposure: f32,
    /// The layers this camera draws (bit `i` = the project's `layers[i]`,
    /// ADR-0020); an object on a layer outside it is neither drawn nor
    /// casts a shadow in this camera's frame.
    #[fynkel(
        kind = "layerMask",
        label = "Culling mask",
        group = "Output",
        order = 8
    )]
    pub culling_mask: u32,
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
            culling_mask: u32::MAX,
        }
    }
}

/// Draws a mesh sub-asset with a material.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "component", id = 4, name = "fynkel.MeshRenderer")]
pub struct MeshRenderer {
    /// Mesh asset (an imported glTF `mesh/<n>` sub-asset).
    #[fynkel(kind = "asset(mesh)", order = 0)]
    pub mesh: AssetRef,
    /// `.material.json` asset.
    #[fynkel(kind = "asset(material)", order = 1)]
    pub material: AssetRef,
    #[fynkel(group = "Shadows", order = 2)]
    pub cast_shadows: bool,
    #[fynkel(group = "Shadows", order = 3)]
    pub receive_shadows: bool,
    /// `false` keeps the component but skips drawing.
    #[fynkel(order = 4)]
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
    #[fynkel(kind = "color", order = 0)]
    pub color: Vec3,
    /// Illuminance in lux.
    #[fynkel(unit = "lux", min = 0, order = 1)]
    pub intensity: f32,
    #[fynkel(order = 2)]
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
    #[fynkel(kind = "color", order = 0)]
    pub color: Vec3,
    /// Luminous intensity in candela.
    #[fynkel(unit = "cd", min = 0, order = 1)]
    pub intensity: f32,
    /// Metres; no contribution beyond this distance.
    #[fynkel(unit = "m", min = 0, order = 2)]
    pub range: f32,
    /// The layers this light lights (ADR-0020); all by default.
    #[fynkel(kind = "layerMask", label = "Culling mask", order = 3)]
    pub culling_mask: u32,
}

impl Default for PointLight {
    fn default() -> Self {
        Self {
            color: Vec3::ONE,
            intensity: 800.0,
            range: 10.0,
            culling_mask: u32::MAX,
        }
    }
}

/// Cone of light along the entity's local -Z (no shadows in V1).
#[derive(Clone, Copy, Debug, PartialEq, Schema)]
#[schema(kind = "component", id = 8, name = "fynkel.SpotLight")]
pub struct SpotLight {
    /// Linear RGB, each `0..=1`.
    #[fynkel(kind = "color", order = 0)]
    pub color: Vec3,
    /// Luminous intensity in candela along the axis.
    #[fynkel(unit = "cd", min = 0, order = 1)]
    pub intensity: f32,
    /// Metres; no contribution beyond this distance.
    #[fynkel(unit = "m", min = 0, order = 2)]
    pub range: f32,
    /// Half-angle in radians inside which the light is full.
    #[fynkel(
        kind = "angle",
        unit = "deg",
        min = 0,
        max = 1.5707963,
        group = "Cone",
        order = 3
    )]
    pub inner_angle: f32,
    /// Half-angle in radians beyond which the light is zero, `>= inner_angle`.
    #[fynkel(
        kind = "angle",
        unit = "deg",
        min = 0,
        max = 1.5707963,
        group = "Cone",
        order = 4
    )]
    pub outer_angle: f32,
    /// The layers this light lights (ADR-0020); all by default.
    #[fynkel(kind = "layerMask", label = "Culling mask", order = 5)]
    pub culling_mask: u32,
}

impl Default for SpotLight {
    fn default() -> Self {
        Self {
            color: Vec3::ONE,
            intensity: 1_500.0,
            range: 15.0,
            inner_angle: 25f32.to_radians(),
            outer_angle: 35f32.to_radians(),
            culling_mask: u32::MAX,
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
    #[fynkel(kind = "color", group = "Ambient", order = 0)]
    pub ambient_color: Vec3,
    /// Ambient illuminance in lux, from every direction.
    #[fynkel(unit = "lux", min = 0, group = "Ambient", order = 1)]
    pub ambient_intensity: f32,
    /// Equirectangular sRGB texture asset drawn as the sky (no image-based
    /// lighting in V1: the sky is a backdrop, `ambient_*` lights surfaces).
    #[fynkel(kind = "asset(texture)", group = "Sky", order = 2)]
    pub sky: Option<AssetRef>,
    /// Luminance in nits of a white sky texel.
    #[fynkel(unit = "nit", min = 0, group = "Sky", order = 3)]
    pub sky_intensity: f32,
    /// Rotation of the sky about +Y in radians.
    #[fynkel(kind = "angle", unit = "deg", group = "Sky", order = 4)]
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

/// Where a [`Volume`] applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Schema)]
#[schema(name = "fynkel.VolumeShape")]
pub enum VolumeShape {
    /// Everywhere, at full weight.
    #[default]
    Global,
    /// Inside a box of `size` about the entity (its rotation and scale
    /// included), fading out over `blend_distance` outside it.
    Box,
    /// Inside a sphere of `radius` about the entity, fading out over
    /// `blend_distance` outside it.
    Sphere,
}

/// A post-processing volume (R3a, `docs/renderer.md` § Volumes): the look
/// overrides in `profile` (a `.volume.json`) applied to the frame by how
/// far inside the shape the camera is. Every frame the renderer starts
/// from the engine defaults and blends the enabled volumes over them in
/// `priority` order (ties: scene order), each by `weight` × its shape's
/// factor — 1 inside, falling linearly to 0 at `blend_distance` outside.
/// The project's quality profile stays the ceiling: a volume cannot turn
/// on what the profile has off.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "component", id = 25, name = "fynkel.Volume")]
pub struct Volume {
    /// The `.volume.json` profile.
    #[fynkel(kind = "asset(volume)", order = 0)]
    pub profile: AssetRef,
    #[fynkel(
        group = "Shape",
        order = 1,
        tooltip = "global: everywhere; box and sphere fade out over the blend distance."
    )]
    pub shape: VolumeShape,
    /// A box's full size in the entity's local units, each `> 0`.
    #[fynkel(label = "Box size", group = "Shape", order = 2)]
    pub size: Vec3,
    /// A sphere's radius in the entity's local units, `> 0`.
    #[fynkel(label = "Sphere radius", min = 0, group = "Shape", order = 3)]
    pub radius: f32,
    /// Metres outside the shape over which the volume fades out, `>= 0`.
    #[fynkel(unit = "m", min = 0, group = "Shape", order = 4)]
    pub blend_distance: f32,
    /// How much of the profile applies, `0..=1`.
    #[fynkel(kind = "slider", min = 0, max = 1, step = 0.01, order = 5)]
    pub weight: f32,
    /// Higher priorities blend over lower ones.
    #[fynkel(
        order = 6,
        tooltip = "Higher priorities blend over lower ones; ties go by scene order."
    )]
    pub priority: i32,
    #[fynkel(order = 7)]
    pub enabled: bool,
}

impl Default for Volume {
    fn default() -> Self {
        Self {
            profile: AssetRef::default(),
            shape: VolumeShape::Global,
            size: Vec3::splat(10.0),
            radius: 5.0,
            blend_distance: 1.0,
            weight: 1.0,
            priority: 0,
            enabled: true,
        }
    }
}

impl Volume {
    /// Value rules the schema cannot express (the `INVALID_PAYLOAD` text).
    pub fn validate(&self) -> Result<(), String> {
        if !self.size.is_finite() || self.size.min_element() <= 0.0 {
            return Err(format!(
                "size must be finite and > 0 on every axis, got {:?}",
                self.size
            ));
        }
        if !self.radius.is_finite() || self.radius <= 0.0 {
            return Err(format!(
                "radius must be finite and > 0, got {}",
                self.radius
            ));
        }
        if !self.blend_distance.is_finite() || self.blend_distance < 0.0 {
            return Err(format!(
                "blend_distance must be finite and >= 0, got {}",
                self.blend_distance
            ));
        }
        if !(0.0..=1.0).contains(&self.weight) {
            return Err(format!("weight must be in 0..=1, got {}", self.weight));
        }
        Ok(())
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
            "component:5:fynkel.Camera[projection:fynkel.Projection{perspective|orthographic},fov_y:f32,ortho_height:f32,near:f32,far:f32,priority:i32,exposure_mode:fynkel.ExposureMode{auto|manual},exposure:f32,culling_mask:u32]"
        );
        assert_eq!(Camera::SCHEMA.wire_size(), 40);
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

/// A camera rig that orbits a target (blueprint §19.2d item 9: `Follow`
/// specialised for the third person). Every tick, after physics, the
/// entity is placed `distance` metres from the target's world position +
/// `pivot` at `yaw`/`pitch` (radians; yaw 0 is behind the target on +Z,
/// pitch positive above it looking down) and turned to look at the pivot;
/// with a `look_action` named, that 2D action turns the rig at
/// `sensitivity` radians per second per unit of input (x turns the view
/// right, y up) between `min_pitch` and `max_pitch`; `yaw`/`pitch` are
/// written back so gameplay reads (and may set) them. With
/// `collision_radius > 0` a sphere of that radius is cast from the pivot
/// towards the rig against `mask` (the target's own colliders excluded)
/// and the rig pulls in to the hit, never closer than `min_distance`.
/// The target is an object reference (ADR-0035); None or a destroyed
/// target leaves the entity where it is.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "component", id = 13, name = "fynkel.ThirdPersonCamera")]
pub struct ThirdPersonCamera<R = ObjectRef> {
    /// The followed object; None follows nothing.
    #[fynkel(order = 0, gizmo(shape = "lineTo"))]
    pub target: R,
    /// Offset from the target's world position the rig looks at and
    /// orbits, metres.
    #[fynkel(unit = "m", order = 1)]
    pub pivot: Vec3,
    /// Orbit radius, metres, `> 0`.
    #[fynkel(unit = "m", min = 0, group = "Orbit", order = 2)]
    pub distance: f32,
    /// Closest the pull-in brings the rig, metres, `0..=distance`.
    #[fynkel(unit = "m", min = 0, group = "Orbit", order = 3)]
    pub min_distance: f32,
    /// Orbit yaw, radians (state).
    #[fynkel(kind = "angle", unit = "deg", group = "Orbit", order = 4)]
    pub yaw: f32,
    /// Orbit elevation, radians (state), kept in `min_pitch..=max_pitch`.
    #[fynkel(kind = "angle", unit = "deg", group = "Orbit", order = 5)]
    pub pitch: f32,
    #[fynkel(kind = "angle", unit = "deg", group = "Orbit", order = 6)]
    pub min_pitch: f32,
    #[fynkel(kind = "angle", unit = "deg", group = "Orbit", order = 7)]
    pub max_pitch: f32,
    /// A 2D action that turns the rig; empty leaves yaw/pitch to gameplay.
    #[fynkel(group = "Input", order = 8)]
    pub look_action: String,
    /// Radians per second per unit of `look_action` input.
    #[fynkel(unit = "rad/s", min = 0, group = "Input", order = 9)]
    pub sensitivity: f32,
    /// Radius of the pull-in sphere cast, metres (`0` disables).
    #[fynkel(unit = "m", min = 0, group = "Collision", order = 10)]
    pub collision_radius: f32,
    /// Layers the pull-in cast hits.
    #[fynkel(kind = "layerMask", group = "Collision", order = 11)]
    pub mask: u32,
}

impl<R> ThirdPersonCamera<R> {
    /// Converts object references without duplicating component configuration.
    pub fn map_object_refs<T>(self, mut map: impl FnMut(R) -> T) -> ThirdPersonCamera<T> {
        ThirdPersonCamera {
            target: map(self.target),
            pivot: self.pivot,
            distance: self.distance,
            min_distance: self.min_distance,
            yaw: self.yaw,
            pitch: self.pitch,
            min_pitch: self.min_pitch,
            max_pitch: self.max_pitch,
            look_action: self.look_action,
            sensitivity: self.sensitivity,
            collision_radius: self.collision_radius,
            mask: self.mask,
        }
    }
}

impl Default for ThirdPersonCamera {
    fn default() -> Self {
        Self {
            target: ObjectRef::NONE,
            pivot: Vec3::new(0.0, 1.5, 0.0),
            distance: 5.0,
            min_distance: 0.5,
            yaw: 0.0,
            pitch: 0.35,
            min_pitch: -0.5,
            max_pitch: 1.3,
            look_action: String::new(),
            sensitivity: 2.5,
            collision_radius: 0.2,
            mask: u32::MAX,
        }
    }
}

impl<R> ThirdPersonCamera<R> {
    /// Value rules the schema cannot express (the `INVALID_PAYLOAD` text).
    pub fn validate(&self) -> Result<(), String> {
        if !self.pivot.is_finite() {
            return Err("pivot must be finite".into());
        }
        if !self.distance.is_finite() || self.distance <= 0.0 {
            return Err(format!(
                "distance must be finite and > 0, got {}",
                self.distance
            ));
        }
        if !self.min_distance.is_finite()
            || self.min_distance < 0.0
            || self.min_distance > self.distance
        {
            return Err(format!(
                "min_distance must be in 0..=distance, got {}",
                self.min_distance
            ));
        }
        if !self.yaw.is_finite() || !self.pitch.is_finite() {
            return Err("yaw and pitch must be finite".into());
        }
        let half = core::f32::consts::FRAC_PI_2;
        if !self.min_pitch.is_finite()
            || !self.max_pitch.is_finite()
            || self.min_pitch <= -half
            || self.max_pitch >= half
            || self.min_pitch > self.max_pitch
        {
            return Err(format!(
                "min_pitch..=max_pitch must lie within (-pi/2, pi/2), got {}..={}",
                self.min_pitch, self.max_pitch
            ));
        }
        if !self.sensitivity.is_finite() || self.sensitivity < 0.0 {
            return Err(format!(
                "sensitivity must be finite and >= 0, got {}",
                self.sensitivity
            ));
        }
        if !self.collision_radius.is_finite() || self.collision_radius < 0.0 {
            return Err(format!(
                "collision_radius must be finite and >= 0, got {}",
                self.collision_radius
            ));
        }
        Ok(())
    }
}

/// Which frame a catalogue mover's axis is given in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Schema)]
#[schema(name = "fynkel.MotionSpace")]
pub enum MotionSpace {
    /// The entity's own axes (a tilted object spins about its own tilt).
    #[default]
    Local,
    /// The world's axes, whatever the parent's rotation.
    World,
}

/// Spins the entity about `axis` at `speed` radians per second, every
/// tick, in the pre-tick slot (blueprint §3.5 catalogue, M3-10). A
/// `RigidBody` on the same entity follows as a kinematic target.
#[derive(Clone, Copy, Debug, PartialEq, Schema)]
#[schema(kind = "component", id = 14, name = "fynkel.Rotator")]
pub struct Rotator {
    /// Spin axis, normalised at use; must not be zero.
    #[fynkel(order = 0)]
    pub axis: Vec3,
    /// Radians per second (negative spins the other way).
    #[fynkel(unit = "rad/s", step = 0.1, order = 1)]
    pub speed: f32,
    #[fynkel(order = 2)]
    pub space: MotionSpace,
}

impl Default for Rotator {
    fn default() -> Self {
        Self {
            axis: Vec3::Y,
            speed: 1.0,
            space: MotionSpace::Local,
        }
    }
}

impl Rotator {
    /// Value rules the schema cannot express (the `INVALID_PAYLOAD` text).
    pub fn validate(&self) -> Result<(), String> {
        finite_axis("axis", self.axis)?;
        if !self.speed.is_finite() {
            return Err(format!("speed must be finite, got {}", self.speed));
        }
        Ok(())
    }
}

/// Moves the entity back and forth along `axis`: its position is the
/// rest position plus `axis · amplitude · sin(2π · frequency · t + phase)`,
/// where `t` starts at 0 when the component is added and the rest position
/// is wherever the entity is when nothing else moves it — a transform
/// write from gameplay shifts the rest position, it does not fight the
/// oscillation. Pre-tick slot; a kinematic `RigidBody` on the entity rides
/// along (a moving platform).
#[derive(Clone, Copy, Debug, PartialEq, Schema)]
#[schema(kind = "component", id = 15, name = "fynkel.Oscillate")]
pub struct Oscillate {
    /// Direction of travel, normalised at use; must not be zero.
    #[fynkel(order = 0)]
    pub axis: Vec3,
    /// Metres either side of the rest position, `>= 0`.
    #[fynkel(unit = "m", min = 0, order = 1)]
    pub amplitude: f32,
    /// Cycles per second, `>= 0`.
    #[fynkel(unit = "Hz", min = 0, order = 2)]
    pub frequency: f32,
    /// Radians into the cycle at `t = 0`.
    #[fynkel(kind = "angle", unit = "deg", order = 3)]
    pub phase: f32,
    #[fynkel(order = 4)]
    pub space: MotionSpace,
}

impl Default for Oscillate {
    fn default() -> Self {
        Self {
            axis: Vec3::Y,
            amplitude: 1.0,
            frequency: 0.5,
            phase: 0.0,
            space: MotionSpace::Local,
        }
    }
}

impl Oscillate {
    pub fn validate(&self) -> Result<(), String> {
        finite_axis("axis", self.axis)?;
        non_negative("amplitude", self.amplitude)?;
        non_negative("frequency", self.frequency)?;
        if !self.phase.is_finite() {
            return Err(format!("phase must be finite, got {}", self.phase));
        }
        Ok(())
    }
}

/// Keeps the entity at another object's world position plus `offset`,
/// after the tick's physics, optionally turned to look at it. With
/// `smoothing` 0 it snaps; otherwise it closes the gap at that rate per
/// second (`1 − e^(−smoothing·dt)` of the distance per tick), the usual
/// exponential follow. The target is an object reference (ADR-0035);
/// None or a destroyed target leaves the entity where it is. A camera
/// follows with `ThirdPersonCamera` instead.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "component", id = 16, name = "fynkel.Follow")]
pub struct Follow<R = ObjectRef> {
    /// The followed object; None follows nothing.
    #[fynkel(order = 0, gizmo(shape = "lineTo"))]
    pub target: R,
    /// Metres from the target's world position, world axes.
    #[fynkel(unit = "m", order = 1)]
    pub offset: Vec3,
    /// Per second, `>= 0`; `0` snaps.
    #[fynkel(unit = "/s", min = 0, order = 2)]
    pub smoothing: f32,
    /// Also turn the entity's -Z towards the target's position.
    #[fynkel(order = 3)]
    pub look_at: bool,
}

impl<R> Follow<R> {
    /// Converts object references without duplicating component configuration.
    pub fn map_object_refs<T>(self, mut map: impl FnMut(R) -> T) -> Follow<T> {
        Follow {
            target: map(self.target),
            offset: self.offset,
            smoothing: self.smoothing,
            look_at: self.look_at,
        }
    }
}

impl Default for Follow {
    fn default() -> Self {
        Self {
            target: ObjectRef::NONE,
            offset: Vec3::new(0.0, 2.0, 0.0),
            smoothing: 0.0,
            look_at: false,
        }
    }
}

impl<R> Follow<R> {
    pub fn validate(&self) -> Result<(), String> {
        if !self.offset.is_finite() {
            return Err("offset must be finite".into());
        }
        non_negative("smoothing", self.smoothing)
    }
}

/// Turns the entity so its local +Z faces the active camera (the enabled
/// `Camera` with the highest priority, lowest entity on a tie), after the
/// tick's physics and camera rigs: a billboard. A quad authored in its XY
/// plane faces the viewer.
#[derive(Clone, Copy, Debug, PartialEq, Schema)]
#[schema(kind = "component", id = 17, name = "fynkel.LookAtCamera")]
pub struct LookAtCamera {
    /// Turn about the world Y axis only, staying upright (a tree card);
    /// `false` also tilts to face a camera above or below.
    #[fynkel(label = "Lock Y axis")]
    pub lock_y: bool,
}

impl Default for LookAtCamera {
    fn default() -> Self {
        Self { lock_y: true }
    }
}

/// Destroys the entity and its subtree `seconds` after the component is
/// added (pre-tick slot; `Destroyed` events as for `Command::Destroy`).
/// `seconds` counts down and reads as the time left.
#[derive(Clone, Copy, Debug, PartialEq, Schema)]
#[schema(kind = "component", id = 18, name = "fynkel.LifetimeDespawn")]
pub struct LifetimeDespawn {
    /// Seconds left, `>= 0`.
    #[fynkel(unit = "s", min = 0)]
    pub seconds: f32,
}

impl Default for LifetimeDespawn {
    fn default() -> Self {
        Self { seconds: 5.0 }
    }
}

impl LifetimeDespawn {
    pub fn validate(&self) -> Result<(), String> {
        non_negative("seconds", self.seconds)
    }
}

/// Instantiates `prefab` at the entity's world pose `rate` times per
/// second (pre-tick slot; the fractional remainder carries over so 2.5/s
/// alternates 2 and 3 per second): each instance is the prefab's root,
/// unparented and named after the prefab, with the prefab's other entities
/// under it — exactly `World::instantiate` — with a `LifetimeDespawn {
/// seconds: lifetime }` on the root when `lifetime > 0`, and every dynamic
/// `RigidBody` in it starts at
/// `velocity` (world axes). With `max_alive > 0` no more are spawned while
/// that many of this emitter's instances live. The world must know the
/// prefab (`World::register_prefab`; the loaders register every prefab a
/// loaded scene's emitters name, `PREFAB_UNAVAILABLE` once otherwise).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "component", id = 19, name = "fynkel.Emitter")]
pub struct Emitter {
    /// The `.prefab.json` asset to instantiate.
    #[fynkel(kind = "asset(prefab)", order = 0)]
    pub prefab: AssetRef,
    /// Instances per second, `>= 0`; `0` pauses.
    #[fynkel(unit = "/s", min = 0, order = 1)]
    pub rate: f32,
    /// Seconds each instance lives, `>= 0`; `0` keeps them.
    #[fynkel(unit = "s", min = 0, order = 2)]
    pub lifetime: f32,
    /// Ceiling on this emitter's live instances; `0` is unlimited.
    #[fynkel(label = "Max alive", order = 3)]
    pub max_alive: u32,
    /// Initial linear velocity of each instance's dynamic bodies, m/s.
    #[fynkel(unit = "m/s", order = 4, gizmo(shape = "direction", space = "world"))]
    pub velocity: Vec3,
    #[fynkel(order = 5)]
    pub enabled: bool,
}

impl Default for Emitter {
    fn default() -> Self {
        Self {
            prefab: AssetRef::default(),
            rate: 1.0,
            lifetime: 5.0,
            max_alive: 0,
            velocity: Vec3::ZERO,
            enabled: true,
        }
    }
}

impl Emitter {
    pub fn validate(&self) -> Result<(), String> {
        non_negative("rate", self.rate)?;
        non_negative("lifetime", self.lifetime)?;
        if !self.velocity.is_finite() {
            return Err("velocity must be finite".into());
        }
        Ok(())
    }
}

/// Names the event Dart hears when a collider enters or leaves this
/// entity's **sensor** `Collider` (M3-12; `docs/scenes.md`): every physics
/// `TriggerEnter`/`TriggerExit` the entity takes part in also produces a
/// general-lane `ZoneEntered`/`ZoneExited` event carrying `event`, the zone
/// entity and the other one, so gameplay subscribes by name
/// (`world.zones.on('collect', …)`) instead of matching colliders. With
/// `once` the component is removed right after the first entry (no exit
/// follows; the collider stays until the entity goes). An entity without a
/// sensor collider never fires (`fynkel validate`: `FYN-H0009`).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "component", id = 20, name = "fynkel.TriggerZone")]
pub struct TriggerZone {
    /// The event name, non-empty.
    #[fynkel(order = 0)]
    pub event: String,
    /// Fire on the first entry only, then drop the component.
    #[fynkel(order = 1)]
    pub once: bool,
}

impl Default for TriggerZone {
    fn default() -> Self {
        Self {
            event: "zone".into(),
            once: false,
        }
    }
}

impl TriggerZone {
    pub fn validate(&self) -> Result<(), String> {
        if self.event.trim().is_empty() {
            return Err("event must not be empty".into());
        }
        Ok(())
    }
}

/// Plays one clip over the entity's subtree (M3-19, `docs/animation.md`):
/// every channel of `clip` drives the first descendant (the entity itself
/// included) named like the channel's target, in the pre-tick slot, so
/// physics and the catalogue see the posed transforms this tick. `speed`
/// scales time (negative plays backwards); a `looping` clip wraps, one
/// that is not stops at its last key with `playing` cleared and an
/// `ANIMATION_ENDED` event. `Command::Animate` plays, crossfades, stops,
/// pauses, resumes, re-speeds and seeks it from Dart; the world must know
/// the clip (`World::register_clip`; the loaders register every clip a
/// loaded scene's animators name, `CLIP_UNAVAILABLE` once otherwise).
///
/// Layers (M3-19c): a second animator on a descendant is a layer over the
/// first — animators apply ancestor-first, so the deeper one owns the
/// entities its clip names (a mask by placement: an `Animator` on the
/// spine drives the upper body over the one on the root). `weight` blends
/// its pose over what is under it (`0` leaves it untouched), `additive`
/// adds the clip's difference from its first frame instead of replacing
/// the pose (a wave over a walk). `root_motion` takes the planar
/// translation and yaw of the clip's root channel out of the pose and
/// publishes the per-tick delta: a `CharacterController` on the entity
/// walks by it, else the entity's own transform moves.
///
/// ADR-0032: with a `controller` (an Animator Controller,
/// `.controller.json`) the animator runs its state machine instead and
/// `clip` and `looping` are not used; `speed`, `playing`, `weight` and
/// `root_motion` still apply to the whole animator.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "component", id = 21, name = "fynkel.Animator")]
pub struct Animator {
    /// The clip (an imported glTF `anim/<a>` sub-asset or a blend space
    /// `.blend.json`); nil plays nothing.
    #[fynkel(kind = "asset(clip)", order = 1)]
    pub clip: AssetRef,
    /// Playback rate, `1` is authored speed; finite.
    #[fynkel(step = 0.05, order = 2)]
    pub speed: f32,
    #[fynkel(order = 4)]
    pub looping: bool,
    #[fynkel(order = 3)]
    pub playing: bool,
    /// Blend weight of this animator's pose over the pose beneath it, in
    /// `[0, 1]`; `1` (the default) replaces it.
    #[fynkel(
        kind = "slider",
        min = 0,
        max = 1,
        step = 0.01,
        group = "Blending",
        order = 5
    )]
    pub weight: f32,
    /// Add the clip's delta from its first frame instead of replacing.
    #[fynkel(group = "Blending", order = 6)]
    pub additive: bool,
    /// Extract the root channel's planar motion (see above).
    #[fynkel(label = "Root motion", order = 7)]
    pub root_motion: bool,
    /// The Animator Controller it runs (ADR-0032); none plays `clip`.
    #[fynkel(kind = "asset(controller)", order = 0)]
    pub controller: Option<AssetRef>,
}

impl Default for Animator {
    fn default() -> Self {
        Self {
            clip: AssetRef::default(),
            speed: 1.0,
            looping: true,
            playing: true,
            weight: 1.0,
            additive: false,
            root_motion: false,
            controller: None,
        }
    }
}

impl Animator {
    pub fn validate(&self) -> Result<(), String> {
        if !self.speed.is_finite() {
            return Err(format!("speed must be finite, got {}", self.speed));
        }
        if !(0.0..=1.0).contains(&self.weight) {
            return Err(format!("weight must be within [0, 1], got {}", self.weight));
        }
        Ok(())
    }
}

/// Makes the entity's `MeshRenderer` a skinned draw (M3-19): the mesh's
/// vertices carry joint weights into `skin` (an imported glTF `skin/<s>`),
/// whose joints are resolved by name to entities under `skeleton`, an
/// object reference (Unity's `rootBone`, ADR-0035) — the mesh entity's
/// tree root when None; a destroyed skeleton leaves the bind pose. Every frame the renderer gets one matrix per
/// joint from those entities' (interpolated) world matrices; a joint that
/// does not resolve stays in its bind pose. The importer writes this next
/// to the renderer of every skinned node; the world must know the skin
/// (`World::register_skin`; loaders do, `SKIN_UNAVAILABLE` once otherwise).
#[derive(Clone, Debug, Default, PartialEq, Schema)]
#[schema(kind = "component", id = 22, name = "fynkel.Skin")]
pub struct Skin<R = ObjectRef> {
    /// The skin sub-asset.
    #[fynkel(kind = "asset(skin)", order = 0)]
    pub skin: AssetRef,
    /// The skeleton root object; None for the mesh's tree root.
    #[fynkel(order = 1)]
    pub skeleton: R,
}

impl<R> Skin<R> {
    /// Converts object references without duplicating component configuration.
    pub fn map_object_refs<T>(self, mut map: impl FnMut(R) -> T) -> Skin<T> {
        Skin {
            skin: self.skin,
            skeleton: map(self.skeleton),
        }
    }
}

fn finite_axis(what: &str, v: Vec3) -> Result<(), String> {
    if !v.is_finite() || v.length_squared() < 1e-12 {
        return Err(format!("{what} must be finite and non-zero, got {v:?}"));
    }
    Ok(())
}

fn non_negative(what: &str, v: f32) -> Result<(), String> {
    if !v.is_finite() || v < 0.0 {
        return Err(format!("{what} must be finite and >= 0, got {v}"));
    }
    Ok(())
}

/// Every built-in component, once (foundation 6.2): its `ComponentValue`
/// variant, its type, its `ComponentId` and the fields that hold an
/// [`ObjectRef`] (ADR-0035). Calls the macro `$then` with the list, so each
/// place that handles every component — the `ComponentValue` enum and its
/// `id`, a world's set, remove and read-back, the bridge's component query
/// — is written from it. `fynkel.Name` is not here: it mirrors the
/// entity's `name`. Adding a component: `docs/codegen.md`.
#[doc(hidden)]
#[macro_export]
macro_rules! builtin_components {
    ($then:ident) => {
        $then! {
            Transform($crate::Transform) = TRANSFORM [];
            MeshRenderer($crate::MeshRenderer) = MESH_RENDERER [];
            Camera($crate::Camera) = CAMERA [];
            DirectionalLight($crate::DirectionalLight) = DIRECTIONAL_LIGHT [];
            PointLight($crate::PointLight) = POINT_LIGHT [];
            SpotLight($crate::SpotLight) = SPOT_LIGHT [];
            Environment($crate::Environment) = ENVIRONMENT [];
            /// `fynkel_physics` (M3-08).
            RigidBody($crate::RigidBody) = RIGID_BODY [];
            Collider($crate::Collider) = COLLIDER [];
            /// M3-09.
            CharacterController($crate::CharacterController) = CHARACTER_CONTROLLER [];
            ThirdPersonCamera($crate::ThirdPersonCamera) = THIRD_PERSON_CAMERA [target];
            /// The catalogue (M3-10).
            Rotator($crate::Rotator) = ROTATOR [];
            Oscillate($crate::Oscillate) = OSCILLATE [];
            Follow($crate::Follow) = FOLLOW [target];
            LookAtCamera($crate::LookAtCamera) = LOOK_AT_CAMERA [];
            LifetimeDespawn($crate::LifetimeDespawn) = LIFETIME_DESPAWN [];
            Emitter($crate::Emitter) = EMITTER [];
            /// M3-12.
            TriggerZone($crate::TriggerZone) = TRIGGER_ZONE [];
            /// Animation (M3-19).
            Animator($crate::Animator) = ANIMATOR [];
            Skin($crate::Skin) = SKIN [skeleton];
            /// `fynkel_audio` (M3-11).
            AudioSource($crate::AudioSource) = AUDIO_SOURCE [];
            AudioListener($crate::AudioListener) = AUDIO_LISTENER [];
            /// Post-processing (R3a).
            Volume($crate::Volume) = VOLUME [];
            /// The object's layer and tags (ADR-0020).
            Layer($crate::Layer) = LAYER [];
            Tags($crate::Tags) = TAGS [];
        }
    };
}

macro_rules! component_value {
    ($($(#[$doc:meta])* $variant:ident($ty:ty) = $id:ident [$($field:ident),*];)*) => {
        /// One typed built-in component value: what a scene document
        /// instantiates and what `Command::SetComponent` carries.
        /// `fynkel.Name` is not here — it mirrors the entity's `name`
        /// (`Command::SetName`).
        #[derive(Clone, Debug, PartialEq)]
        pub enum ComponentValue {
            $($(#[$doc])* $variant($ty),)*
        }

        /// The component fields that hold an [`ObjectRef`] (ADR-0035): what
        /// [`ComponentValue::for_each_object_ref`] visits, pinned against
        /// the schemas by a test.
        pub const OBJECT_REF_FIELDS: &[(&str, &str)] = &[
            $($((ComponentId::$id.name().unwrap(), stringify!($field)),)*)*
        ];

        // Each entry's id is its type's schema id.
        const _: () = {
            use fynkel_contracts::HasSchema;
            $(assert!(<$ty as HasSchema>::SCHEMA.id == ComponentId::$id.0);)*
        };

        impl ComponentValue {
            /// Every built-in component's id, in registry order.
            pub const IDS: &[ComponentId] = &[$(ComponentId::$id),*];

            /// Calls `f` with the component's schema name, the field's
            /// name and the value of every object reference the component
            /// holds (ADR-0035): a load maps their authoring ids to
            /// handles, a copy re-points them into the copied tree.
            pub fn for_each_object_ref(
                &mut self,
                mut f: impl FnMut(&'static str, &'static str, &mut ObjectRef),
            ) {
                match self {
                    $(ComponentValue::$variant(_c) => {
                        $(f(ComponentId::$id.name().unwrap(), stringify!($field), &mut _c.$field);)*
                    })*
                }
                let _ = &mut f;
            }

            pub fn id(&self) -> ComponentId {
                match self {
                    $(ComponentValue::$variant(_) => ComponentId::$id,)*
                }
            }

            /// Reference-field metadata from the same catalogue as storage.
            pub fn object_ref_fields(id: ComponentId) -> Option<&'static [&'static str]> {
                match id {
                    $(ComponentId::$id => Some(&[$(stringify!($field)),*]),)*
                    _ => None,
                }
            }

            pub(crate) fn read_from(ecs: &fynkel_ecs::World, entity: fynkel_contracts::Entity, id: ComponentId) -> Option<Self> {
                match id {
                    $(ComponentId::$id => ecs.get::<$ty>(entity).ok().map(|v| Self::$variant((*v).clone())),)*
                    _ => None,
                }
            }

            pub(crate) fn contains_in(ecs: &fynkel_ecs::World, entity: fynkel_contracts::Entity, id: ComponentId) -> bool {
                match id {
                    $(ComponentId::$id => ecs.has::<$ty>(entity),)*
                    _ => false,
                }
            }

            /// Inserts (or replaces) the component on `entity`, unchecked.
            pub(crate) fn insert_into(
                self,
                ecs: &mut fynkel_ecs::World,
                entity: fynkel_contracts::Entity,
            ) -> Result<(), fynkel_ecs::EcsError> {
                match self {
                    $(ComponentValue::$variant(v) => ecs.insert(entity, v),)*
                }
            }

            /// Removes the component `id` from `entity`: whether it had
            /// one; `None` for an id that is not a built-in component.
            pub(crate) fn remove_from(
                ecs: &mut fynkel_ecs::World,
                entity: fynkel_contracts::Entity,
                id: ComponentId,
            ) -> Option<bool> {
                match id {
                    $(ComponentId::$id => Some(ecs.remove::<$ty>(entity).is_ok()),)*
                    _ => None,
                }
            }

            /// Every built-in component on `entity`, in registry order.
            pub fn all_of(world: &crate::World, entity: fynkel_contracts::Entity) -> Vec<ComponentValue> {
                let ecs = world.ecs();
                let mut out = Vec::new();
                $(if let Ok(v) = ecs.get::<$ty>(entity) {
                    out.push(ComponentValue::$variant((*v).clone()));
                })*
                out
            }
        }
    };
}

builtin_components!(component_value);

impl ComponentValue {
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
                if !t.is_valid() {
                    return Err("transform rotation must be a unit quaternion".into());
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
            ComponentValue::CharacterController(c) => c.validate()?,
            ComponentValue::ThirdPersonCamera(c) => c.validate()?,
            ComponentValue::Rotator(c) => c.validate()?,
            ComponentValue::Oscillate(c) => c.validate()?,
            ComponentValue::Follow(c) => c.validate()?,
            ComponentValue::LookAtCamera(_) => {}
            ComponentValue::LifetimeDespawn(c) => c.validate()?,
            ComponentValue::Emitter(c) => c.validate()?,
            ComponentValue::TriggerZone(c) => c.validate()?,
            ComponentValue::Animator(c) => c.validate()?,
            ComponentValue::Skin(_) => {}
            ComponentValue::AudioSource(c) => c.validate()?,
            ComponentValue::AudioListener(c) => c.validate()?,
            ComponentValue::Volume(c) => c.validate()?,
            ComponentValue::Layer(c) => c.validate()?,
            ComponentValue::Tags(_) => {}
        }
        Ok(())
    }
}
