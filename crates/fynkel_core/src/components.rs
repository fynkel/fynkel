//! Starter native components (blueprint §19.2c item 4, §8.5): the storage
//! half of camera, mesh rendering, lights and the environment. Values are
//! authored in scene documents (`schemas/components/*.schema.json`) and
//! cross the bridge with the generated codecs; `fynkel_render` reads them
//! from the extract. Units (§19.2d scope rules): metres, radians, linear
//! RGB; illuminance in lux (directional, ambient), luminous intensity in
//! candela (point, spot), luminance in nits (sky); exposure in EV100.

use fynkel_audio::{AudioListener, AudioSource};
use fynkel_contracts::{AssetRef, ComponentId};
use fynkel_physics::{CharacterController, Collider, RigidBody};
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
/// The target is named (`fynkel.Name`, the first match in entity order);
/// an empty or unknown name leaves the entity where it is.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "component", id = 13, name = "fynkel.ThirdPersonCamera")]
pub struct ThirdPersonCamera {
    /// The followed entity's name; empty follows nothing.
    pub target: String,
    /// Offset from the target's world position the rig looks at and
    /// orbits, metres.
    pub pivot: Vec3,
    /// Orbit radius, metres, `> 0`.
    pub distance: f32,
    /// Closest the pull-in brings the rig, metres, `0..=distance`.
    pub min_distance: f32,
    /// Orbit yaw, radians (state).
    pub yaw: f32,
    /// Orbit elevation, radians (state), kept in `min_pitch..=max_pitch`.
    pub pitch: f32,
    pub min_pitch: f32,
    pub max_pitch: f32,
    /// A 2D action that turns the rig; empty leaves yaw/pitch to gameplay.
    pub look_action: String,
    /// Radians per second per unit of `look_action` input.
    pub sensitivity: f32,
    /// Radius of the pull-in sphere cast, metres (`0` disables).
    pub collision_radius: f32,
    /// Layers the pull-in cast hits.
    pub mask: u32,
}

impl Default for ThirdPersonCamera {
    fn default() -> Self {
        Self {
            target: String::new(),
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

impl ThirdPersonCamera {
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
    pub axis: Vec3,
    /// Radians per second (negative spins the other way).
    pub speed: f32,
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
    pub axis: Vec3,
    /// Metres either side of the rest position, `>= 0`.
    pub amplitude: f32,
    /// Cycles per second, `>= 0`.
    pub frequency: f32,
    /// Radians into the cycle at `t = 0`.
    pub phase: f32,
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

/// Keeps the entity at a named entity's world position plus `offset`,
/// after the tick's physics, optionally turned to look at it. With
/// `smoothing` 0 it snaps; otherwise it closes the gap at that rate per
/// second (`1 − e^(−smoothing·dt)` of the distance per tick), the usual
/// exponential follow. The target is named (`fynkel.Name`, the first
/// match in entity order); an empty or unknown name leaves the entity
/// where it is. A camera follows with `ThirdPersonCamera` instead.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "component", id = 16, name = "fynkel.Follow")]
pub struct Follow {
    /// The followed entity's name; empty follows nothing.
    pub target: String,
    /// Metres from the target's world position, world axes.
    pub offset: Vec3,
    /// Per second, `>= 0`; `0` snaps.
    pub smoothing: f32,
    /// Also turn the entity's -Z towards the target's position.
    pub look_at: bool,
}

impl Default for Follow {
    fn default() -> Self {
        Self {
            target: String::new(),
            offset: Vec3::new(0.0, 2.0, 0.0),
            smoothing: 0.0,
            look_at: false,
        }
    }
}

impl Follow {
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
/// alternates 2 and 3 per second): each instance is an unparented anchor
/// named after the prefab whose children are the prefab's roots — exactly
/// `World::instantiate` — with a `LifetimeDespawn { seconds: lifetime }`
/// when `lifetime > 0`, and every dynamic `RigidBody` in it starts at
/// `velocity` (world axes). With `max_alive > 0` no more are spawned while
/// that many of this emitter's instances live. The world must know the
/// prefab (`World::register_prefab`; the loaders register every prefab a
/// loaded scene's emitters name, `PREFAB_UNAVAILABLE` once otherwise).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "component", id = 19, name = "fynkel.Emitter")]
pub struct Emitter {
    /// The `.prefab.json` asset to instantiate.
    pub prefab: AssetRef,
    /// Instances per second, `>= 0`; `0` pauses.
    pub rate: f32,
    /// Seconds each instance lives, `>= 0`; `0` keeps them.
    pub lifetime: f32,
    /// Ceiling on this emitter's live instances; `0` is unlimited.
    pub max_alive: u32,
    /// Initial linear velocity of each instance's dynamic bodies, m/s.
    pub velocity: Vec3,
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
    pub event: String,
    /// Fire on the first entry only, then drop the component.
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
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "component", id = 21, name = "fynkel.Animator")]
pub struct Animator {
    /// The clip (an imported glTF `anim/<a>` sub-asset or a blend space
    /// `.blend.json`); nil plays nothing.
    pub clip: AssetRef,
    /// Playback rate, `1` is authored speed; finite.
    pub speed: f32,
    pub looping: bool,
    pub playing: bool,
    /// Blend weight of this animator's pose over the pose beneath it, in
    /// `[0, 1]`; `1` (the default) replaces it.
    pub weight: f32,
    /// Add the clip's delta from its first frame instead of replacing.
    pub additive: bool,
    /// Extract the root channel's planar motion (see above).
    pub root_motion: bool,
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
/// whose joints are resolved by name to entities under the one named
/// `skeleton` — searched from the mesh entity's tree root, the root itself
/// when `skeleton` is empty. Every frame the renderer gets one matrix per
/// joint from those entities' (interpolated) world matrices; a joint that
/// does not resolve stays in its bind pose. The importer writes this next
/// to the renderer of every skinned node; the world must know the skin
/// (`World::register_skin`; loaders do, `SKIN_UNAVAILABLE` once otherwise).
#[derive(Clone, Debug, Default, PartialEq, Schema)]
#[schema(kind = "component", id = 22, name = "fynkel.Skin")]
pub struct Skin {
    /// The skin sub-asset.
    pub skin: AssetRef,
    /// Name of the skeleton root entity; empty for the tree root.
    pub skeleton: String,
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
    /// M3-09.
    CharacterController(CharacterController),
    ThirdPersonCamera(ThirdPersonCamera),
    /// The catalogue (M3-10).
    Rotator(Rotator),
    Oscillate(Oscillate),
    Follow(Follow),
    LookAtCamera(LookAtCamera),
    LifetimeDespawn(LifetimeDespawn),
    Emitter(Emitter),
    /// M3-12.
    TriggerZone(TriggerZone),
    /// Animation (M3-19).
    Animator(Animator),
    Skin(Skin),
    /// `fynkel_audio` (M3-11).
    AudioSource(AudioSource),
    AudioListener(AudioListener),
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
            ComponentValue::CharacterController(_) => ComponentId::CHARACTER_CONTROLLER,
            ComponentValue::ThirdPersonCamera(_) => ComponentId::THIRD_PERSON_CAMERA,
            ComponentValue::Rotator(_) => ComponentId::ROTATOR,
            ComponentValue::Oscillate(_) => ComponentId::OSCILLATE,
            ComponentValue::Follow(_) => ComponentId::FOLLOW,
            ComponentValue::LookAtCamera(_) => ComponentId::LOOK_AT_CAMERA,
            ComponentValue::LifetimeDespawn(_) => ComponentId::LIFETIME_DESPAWN,
            ComponentValue::Emitter(_) => ComponentId::EMITTER,
            ComponentValue::TriggerZone(_) => ComponentId::TRIGGER_ZONE,
            ComponentValue::Animator(_) => ComponentId::ANIMATOR,
            ComponentValue::Skin(_) => ComponentId::SKIN,
            ComponentValue::AudioSource(_) => ComponentId::AUDIO_SOURCE,
            ComponentValue::AudioListener(_) => ComponentId::AUDIO_LISTENER,
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
        }
        Ok(())
    }
}
