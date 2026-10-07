//! Meshes and materials: shared data that objects point to by id.

use crate::Color;
use crate::describe::{Describe, Field, FieldKind, Value};

/// Where a mesh's geometry comes from. Stage 1 only has built-in shapes,
/// which the renderer generates. Meshes with your own vertices come later.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MeshSource {
    /// A 1×1×1 cube centred on the origin.
    Cube,
    /// A sphere of radius 0.5 centred on the origin.
    Sphere,
    /// A 1×1 plane in XZ, facing +Y.
    Plane,
}

/// A metallic-roughness PBR material.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    /// The surface colour (sRGB).
    pub base_color: Color,
    /// 0 = dielectric (plastic, wood), 1 = metal.
    pub metallic: f32,
    /// 0 = mirror-smooth, 1 = fully rough.
    pub roughness: f32,
}

impl Material {
    pub const DEFAULT: Self = Self { base_color: Color::WHITE, metallic: 0.0, roughness: 0.5 };

    pub const fn color(base_color: Color) -> Self {
        Self { base_color, ..Self::DEFAULT }
    }
}

impl Default for Material {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Describe for Material {
    const NAME: &'static str = "Material";

    fn fields() -> &'static [Field] {
        const D: Material = Material::DEFAULT;
        const FIELDS: &[Field] = &[
            Field::new("base_color", FieldKind::Color, Value::Color(D.base_color)),
            Field::new("metallic", FieldKind::F32, Value::F32(D.metallic)).range(0.0, 1.0),
            Field::new("roughness", FieldKind::F32, Value::F32(D.roughness)).range(0.0, 1.0),
        ];
        FIELDS
    }

    fn get(&self, index: usize) -> Option<Value> {
        Some(match index {
            0 => Value::Color(self.base_color),
            1 => Value::F32(self.metallic),
            2 => Value::F32(self.roughness),
            _ => return None,
        })
    }

    fn write(&mut self, index: usize, value: Value) {
        match (index, value) {
            (0, Value::Color(c)) => self.base_color = c,
            (1, Value::F32(v)) => self.metallic = v,
            (2, Value::F32(v)) => self.roughness = v,
            _ => unreachable!("validated by Describe::set"),
        }
    }
}
