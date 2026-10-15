//! The components (stage 1 plan §3.2): Transform, MeshRenderer, Camera and
//! Light.
//!
//! Conventions: right-handed, +Y up, -Z forward (glTF's), metres, angles in
//! degrees where people type them (field of view).

use glam::{Mat4, Quat, Vec3};

use crate::describe::{Describe, Field, FieldKind, Value};
use crate::{Color, MaterialId, MeshId};

/// Marks a type the world can store on objects.
pub trait Component: Describe + Default + Copy + Send + Sync + 'static {}

// --- Transform -------------------------------------------------------------

/// Where an object is, how it's turned and how big it is, relative to its
/// parent (`World::set_parent`), or to the world for an object without one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub position: Vec3,
    /// A unit quaternion.
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self { position: Vec3::ZERO, rotation: Quat::IDENTITY, scale: Vec3::ONE }
    }
}

impl Transform {
    pub fn from_position(position: Vec3) -> Self {
        Self { position, ..Self::default() }
    }

    /// Turns the transform so its forward (-Z) points at `target`, keeping
    /// +Y as close to up as possible. If `target` is the position itself,
    /// or straight above or below it, the rotation is left as it is.
    pub fn looking_at(mut self, target: Vec3) -> Self {
        let forward = target - self.position;
        if forward.length_squared() > 1e-12 && forward.normalize().cross(Vec3::Y).length_squared() > 1e-12 {
            // glam gives the view rotation (world → camera). The object's
            // rotation is its inverse.
            self.rotation = glam::camera::rh::view::look_at_quat(self.position, target, Vec3::Y).inverse().normalize();
        }
        self
    }

    /// The direction the transform faces (-Z turned by `rotation`).
    pub fn forward(&self) -> Vec3 {
        self.rotation * Vec3::NEG_Z
    }

    /// Model-to-world: scale, then rotate, then move.
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.position)
    }
}

impl Describe for Transform {
    const NAME: &'static str = "Transform";

    fn fields() -> &'static [Field] {
        const FIELDS: &[Field] = &[
            Field::new("position", FieldKind::Vec3, Value::Vec3(Vec3::ZERO)),
            Field::new("rotation", FieldKind::Quat, Value::Quat(Quat::IDENTITY)),
            Field::new("scale", FieldKind::Vec3, Value::Vec3(Vec3::ONE)),
        ];
        FIELDS
    }

    fn get(&self, index: usize) -> Option<Value> {
        Some(match index {
            0 => Value::Vec3(self.position),
            1 => Value::Quat(self.rotation),
            2 => Value::Vec3(self.scale),
            _ => return None,
        })
    }

    fn write(&mut self, index: usize, value: Value) {
        match (index, value) {
            (0, Value::Vec3(v)) => self.position = v,
            (1, Value::Quat(q)) => self.rotation = q,
            (2, Value::Vec3(v)) => self.scale = v,
            _ => unreachable!("validated by Describe::set"),
        }
    }
}

impl Component for Transform {}

// --- MeshRenderer ----------------------------------------------------------

/// Draws a mesh with a material at the object's transform. An object needs
/// both this and a [`Transform`] to be drawn. If either id is `None`, it
/// draws nothing. With `own_color`, `color` replaces the material's base
/// colour for this object alone, as a Unity `MaterialPropertyBlock` does:
/// a hit flash on one enemy leaves the others, which share its material.
/// It casts shadows and shadows fall on it unless told otherwise, as in
/// Unity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshRenderer {
    pub mesh: Option<MeshId>,
    pub material: Option<MaterialId>,
    pub own_color: bool,
    pub color: Color,
    /// Whether it blocks lights that cast shadows.
    pub cast_shadows: bool,
    /// Whether shadows fall on it.
    pub receive_shadows: bool,
}

impl Default for MeshRenderer {
    fn default() -> Self {
        Self {
            mesh: None,
            material: None,
            own_color: false,
            color: Color::WHITE,
            cast_shadows: true,
            receive_shadows: true,
        }
    }
}

impl Describe for MeshRenderer {
    const NAME: &'static str = "MeshRenderer";

    fn fields() -> &'static [Field] {
        const FIELDS: &[Field] = &[
            Field::new("mesh", FieldKind::Mesh, Value::Mesh(None)),
            Field::new("material", FieldKind::Material, Value::Material(None)),
            Field::new("own_color", FieldKind::Bool, Value::Bool(false)),
            Field::new("color", FieldKind::Color, Value::Color(Color::WHITE)),
            Field::new("cast_shadows", FieldKind::Bool, Value::Bool(true)),
            Field::new("receive_shadows", FieldKind::Bool, Value::Bool(true)),
        ];
        FIELDS
    }

    fn get(&self, index: usize) -> Option<Value> {
        Some(match index {
            0 => Value::Mesh(self.mesh),
            1 => Value::Material(self.material),
            2 => Value::Bool(self.own_color),
            3 => Value::Color(self.color),
            4 => Value::Bool(self.cast_shadows),
            5 => Value::Bool(self.receive_shadows),
            _ => return None,
        })
    }

    fn write(&mut self, index: usize, value: Value) {
        match (index, value) {
            (0, Value::Mesh(m)) => self.mesh = m,
            (1, Value::Material(m)) => self.material = m,
            (2, Value::Bool(b)) => self.own_color = b,
            (3, Value::Color(c)) => self.color = c,
            (4, Value::Bool(b)) => self.cast_shadows = b,
            (5, Value::Bool(b)) => self.receive_shadows = b,
            _ => unreachable!("validated by Describe::set"),
        }
    }
}

impl Component for MeshRenderer {}

// --- Camera ----------------------------------------------------------------

/// A camera looking along its transform's forward (-Z): perspective, or
/// orthographic. The first camera found is the one rendered.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// Vertical field of view in degrees.
    pub fov_y: f32,
    /// Distance to the near clipping plane, in metres. Must be > 0.
    pub near: f32,
    /// Distance to the far clipping plane, in metres.
    pub far: f32,
    /// The colour behind everything.
    pub clear_color: Color,
    /// Parallel projection: things keep their size however far away they
    /// are, and `fov_y` is unused.
    pub orthographic: bool,
    /// Half the view's height in metres when orthographic, as Unity's
    /// `orthographicSize`.
    pub size: f32,
    /// The layers it draws: bit `n` for layer `n` (Unity's `cullingMask`).
    pub culling_mask: u32,
    /// Whether it shows the world's sky behind everything; without it,
    /// `clear_color`. The sky lights the scene either way.
    pub show_sky: bool,
}

impl Camera {
    pub const DEFAULT: Self = Self {
        fov_y: 60.0,
        near: 0.1,
        far: 1000.0,
        clear_color: Color::rgb(0.1, 0.1, 0.12),
        orthographic: false,
        size: 5.0,
        culling_mask: u32::MAX,
        show_sky: true,
    };
}

impl Default for Camera {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Describe for Camera {
    const NAME: &'static str = "Camera";

    fn fields() -> &'static [Field] {
        const D: Camera = Camera::DEFAULT;
        const FIELDS: &[Field] = &[
            Field::new("fov_y", FieldKind::F32, Value::F32(D.fov_y)).range(1.0, 179.0),
            Field::new("near", FieldKind::F32, Value::F32(D.near)).range(1e-4, 1e6),
            Field::new("far", FieldKind::F32, Value::F32(D.far)).range(1e-3, 1e9),
            Field::new("clear_color", FieldKind::Color, Value::Color(D.clear_color)),
            Field::new("orthographic", FieldKind::Bool, Value::Bool(D.orthographic)),
            Field::new("size", FieldKind::F32, Value::F32(D.size)).range(1e-4, 1e6),
            Field::new("culling_mask", FieldKind::Mask, Value::Mask(D.culling_mask)),
            Field::new("show_sky", FieldKind::Bool, Value::Bool(D.show_sky)),
        ];
        FIELDS
    }

    fn get(&self, index: usize) -> Option<Value> {
        Some(match index {
            0 => Value::F32(self.fov_y),
            1 => Value::F32(self.near),
            2 => Value::F32(self.far),
            3 => Value::Color(self.clear_color),
            4 => Value::Bool(self.orthographic),
            5 => Value::F32(self.size),
            6 => Value::Mask(self.culling_mask),
            7 => Value::Bool(self.show_sky),
            _ => return None,
        })
    }

    fn write(&mut self, index: usize, value: Value) {
        match (index, value) {
            (0, Value::F32(v)) => self.fov_y = v,
            (1, Value::F32(v)) => self.near = v,
            (2, Value::F32(v)) => self.far = v,
            (3, Value::Color(c)) => self.clear_color = c,
            (4, Value::Bool(v)) => self.orthographic = v,
            (5, Value::F32(v)) => self.size = v,
            (6, Value::Mask(v)) => self.culling_mask = v,
            (7, Value::Bool(v)) => self.show_sky = v,
            _ => unreachable!("validated by Describe::set"),
        }
    }
}

impl Component for Camera {}

// --- Light -----------------------------------------------------------------

/// What kind of light a [`Light`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LightKind {
    /// A sun: parallel light shining along the transform's forward (-Z).
    #[default]
    Directional,
    /// Light from every direction equally. It needs no transform.
    Ambient,
    /// A lamp: light from the transform's position in every direction,
    /// fading with distance and gone at [`Light::range`].
    Point,
    /// A spotlight: a point light that shines only in a cone around the
    /// transform's forward (-Z), [`Light::spot_angle`] across.
    Spot,
}

impl LightKind {
    const NAMES: &'static [&'static str] = &["Directional", "Ambient", "Point", "Spot"];
}

/// A light. Units are unitless (plan §3.6): a directional light with
/// intensity 1.0 lights a white, rough surface facing it to exactly its
/// albedo. A point or spot light of intensity 1.0 does the same to a
/// surface 1 m away; light falls off with the square of the distance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    pub kind: LightKind,
    pub color: Color,
    pub intensity: f32,
    /// Point and spot lights: how far the light reaches, in metres. It
    /// fades smoothly to nothing there.
    pub range: f32,
    /// Spot lights: the cone's full angle, in degrees.
    pub spot_angle: f32,
    /// Spot lights: how much of the cone, from its edge in, fades: 0 for
    /// a hard edge, 1 to fade all the way to the centre.
    pub softness: f32,
    /// Whether it casts shadows: a directional light's (the first one
    /// with shadows, if there are more) or a spot light's. Point lights
    /// don't cast them yet.
    pub shadows: bool,
}

impl Light {
    pub const DEFAULT: Self = Self {
        kind: LightKind::Directional,
        color: Color::WHITE,
        intensity: 1.0,
        range: 10.0,
        spot_angle: 30.0,
        softness: 0.2,
        shadows: false,
    };

    pub const fn directional(color: Color, intensity: f32) -> Self {
        Self { kind: LightKind::Directional, color, intensity, ..Self::DEFAULT }
    }

    pub const fn ambient(color: Color, intensity: f32) -> Self {
        Self { kind: LightKind::Ambient, color, intensity, ..Self::DEFAULT }
    }

    pub const fn point(color: Color, intensity: f32, range: f32) -> Self {
        Self { kind: LightKind::Point, color, intensity, range, ..Self::DEFAULT }
    }

    pub const fn spot(color: Color, intensity: f32, range: f32, spot_angle: f32, softness: f32) -> Self {
        Self { kind: LightKind::Spot, color, intensity, range, spot_angle, softness, shadows: false }
    }

    /// This light, casting shadows or not.
    pub const fn with_shadows(self, shadows: bool) -> Self {
        Self { shadows, ..self }
    }
}

impl Default for Light {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Describe for Light {
    const NAME: &'static str = "Light";

    fn fields() -> &'static [Field] {
        const D: Light = Light::DEFAULT;
        const FIELDS: &[Field] = &[
            Field::new("kind", FieldKind::Enum(LightKind::NAMES), Value::Enum(D.kind as u32)),
            Field::new("color", FieldKind::Color, Value::Color(D.color)),
            Field::new("intensity", FieldKind::F32, Value::F32(D.intensity)).range(0.0, 1e6),
            Field::new("range", FieldKind::F32, Value::F32(D.range)).range(0.0, 1e6),
            Field::new("spot_angle", FieldKind::F32, Value::F32(D.spot_angle)).range(0.0, 179.0),
            Field::new("softness", FieldKind::F32, Value::F32(D.softness)).range(0.0, 1.0),
            Field::new("shadows", FieldKind::Bool, Value::Bool(D.shadows)),
        ];
        FIELDS
    }

    fn get(&self, index: usize) -> Option<Value> {
        Some(match index {
            0 => Value::Enum(self.kind as u32),
            1 => Value::Color(self.color),
            2 => Value::F32(self.intensity),
            3 => Value::F32(self.range),
            4 => Value::F32(self.spot_angle),
            5 => Value::F32(self.softness),
            6 => Value::Bool(self.shadows),
            _ => return None,
        })
    }

    fn write(&mut self, index: usize, value: Value) {
        match (index, value) {
            (0, Value::Enum(0)) => self.kind = LightKind::Directional,
            (0, Value::Enum(1)) => self.kind = LightKind::Ambient,
            (0, Value::Enum(2)) => self.kind = LightKind::Point,
            (0, Value::Enum(3)) => self.kind = LightKind::Spot,
            (1, Value::Color(c)) => self.color = c,
            (2, Value::F32(v)) => self.intensity = v,
            (3, Value::F32(v)) => self.range = v,
            (4, Value::F32(v)) => self.spot_angle = v,
            (5, Value::F32(v)) => self.softness = v,
            (6, Value::Bool(b)) => self.shadows = b,
            _ => unreachable!("validated by Describe::set"),
        }
    }
}

impl Component for Light {}
