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
/// draws nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MeshRenderer {
    pub mesh: Option<MeshId>,
    pub material: Option<MaterialId>,
}

impl Describe for MeshRenderer {
    const NAME: &'static str = "MeshRenderer";

    fn fields() -> &'static [Field] {
        const FIELDS: &[Field] = &[
            Field::new("mesh", FieldKind::Mesh, Value::Mesh(None)),
            Field::new("material", FieldKind::Material, Value::Material(None)),
        ];
        FIELDS
    }

    fn get(&self, index: usize) -> Option<Value> {
        Some(match index {
            0 => Value::Mesh(self.mesh),
            1 => Value::Material(self.material),
            _ => return None,
        })
    }

    fn write(&mut self, index: usize, value: Value) {
        match (index, value) {
            (0, Value::Mesh(m)) => self.mesh = m,
            (1, Value::Material(m)) => self.material = m,
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
}

impl Camera {
    pub const DEFAULT: Self = Self {
        fov_y: 60.0,
        near: 0.1,
        far: 1000.0,
        clear_color: Color::rgb(0.1, 0.1, 0.12),
        orthographic: false,
        size: 5.0,
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
}

impl LightKind {
    const NAMES: &'static [&'static str] = &["Directional", "Ambient"];
}

/// A light. Units are unitless (plan §3.6): a directional light with
/// intensity 1.0 lights a white, rough surface facing it to exactly its
/// albedo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    pub kind: LightKind,
    pub color: Color,
    pub intensity: f32,
}

impl Light {
    pub const DEFAULT: Self = Self { kind: LightKind::Directional, color: Color::WHITE, intensity: 1.0 };

    pub const fn directional(color: Color, intensity: f32) -> Self {
        Self { kind: LightKind::Directional, color, intensity }
    }

    pub const fn ambient(color: Color, intensity: f32) -> Self {
        Self { kind: LightKind::Ambient, color, intensity }
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
        ];
        FIELDS
    }

    fn get(&self, index: usize) -> Option<Value> {
        Some(match index {
            0 => Value::Enum(self.kind as u32),
            1 => Value::Color(self.color),
            2 => Value::F32(self.intensity),
            _ => return None,
        })
    }

    fn write(&mut self, index: usize, value: Value) {
        match (index, value) {
            (0, Value::Enum(0)) => self.kind = LightKind::Directional,
            (0, Value::Enum(1)) => self.kind = LightKind::Ambient,
            (1, Value::Color(c)) => self.color = c,
            (2, Value::F32(v)) => self.intensity = v,
            _ => unreachable!("validated by Describe::set"),
        }
    }
}

impl Component for Light {}
