//! Data that describes itself (ADR-0001 "editor later" rule 1).
//!
//! Each component and material lists its fields: name, kind, default and
//! an optional range. It can also read and write a field by index. That's
//! all a generic tool (an inspector, a saver, a debugger) needs to work with
//! a type it has never seen. Writes are validated here, once, for every
//! caller.

use glam::{Quat, Vec3};

use crate::{Color, MaterialId, MeshId};

/// What a field holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    F32,
    Vec3,
    /// A rotation. Stored values are unit quaternions.
    Quat,
    Color,
    Bool,
    Mesh,
    Material,
    /// One of the listed names. The value is its index.
    Enum(&'static [&'static str]),
}

/// A field's value, tagged with its kind.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    F32(f32),
    Vec3(Vec3),
    Quat(Quat),
    Color(Color),
    Bool(bool),
    Mesh(Option<MeshId>),
    Material(Option<MaterialId>),
    Enum(u32),
}

impl Value {
    /// True when the value is of `kind`. For an enum, the index must also be
    /// in range.
    pub fn is(&self, kind: FieldKind) -> bool {
        match (self, kind) {
            (Value::Enum(i), FieldKind::Enum(names)) => (*i as usize) < names.len(),
            (Value::F32(_), FieldKind::F32)
            | (Value::Vec3(_), FieldKind::Vec3)
            | (Value::Quat(_), FieldKind::Quat)
            | (Value::Color(_), FieldKind::Color)
            | (Value::Bool(_), FieldKind::Bool)
            | (Value::Mesh(_), FieldKind::Mesh)
            | (Value::Material(_), FieldKind::Material) => true,
            _ => false,
        }
    }
}

/// An inclusive range for an `F32` field.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
    pub min: f32,
    pub max: f32,
}

/// One field of a described type.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Field {
    pub name: &'static str,
    pub kind: FieldKind,
    pub default: Value,
    pub range: Option<Range>,
}

impl Field {
    pub const fn new(name: &'static str, kind: FieldKind, default: Value) -> Self {
        Self { name, kind, default, range: None }
    }

    pub const fn range(self, min: f32, max: f32) -> Self {
        Self { range: Some(Range { min, max }), ..self }
    }
}

/// Why a field write was refused.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FieldError {
    /// The type has no field with this index.
    NoField {
        ty: &'static str,
        index: usize,
    },
    /// The value's kind doesn't match the field's (or an enum index is past
    /// the end).
    WrongKind {
        ty: &'static str,
        field: &'static str,
    },
    /// NaN or infinity. Never stored, because one NaN in a transform
    /// spreads to everything it touches.
    NotFinite {
        ty: &'static str,
        field: &'static str,
    },
    OutOfRange {
        ty: &'static str,
        field: &'static str,
        value: f32,
        range: Range,
    },
    /// A rotation that isn't a unit quaternion (length off by more than 1e-3).
    NotUnit {
        ty: &'static str,
        field: &'static str,
    },
}

impl std::fmt::Display for FieldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FieldError::NoField { ty, index } => write!(f, "{ty} has no field {index}"),
            FieldError::WrongKind { ty, field } => write!(f, "{ty}.{field}: wrong kind of value"),
            FieldError::NotFinite { ty, field } => write!(f, "{ty}.{field}: NaN or infinity"),
            FieldError::OutOfRange { ty, field, value, range } => {
                write!(f, "{ty}.{field}: {value} is outside {}..={}", range.min, range.max)
            }
            FieldError::NotUnit { ty, field } => write!(f, "{ty}.{field}: rotation is not a unit quaternion"),
        }
    }
}

impl std::error::Error for FieldError {}

/// A type that lists its fields and reads and writes them by index.
pub trait Describe: Sized {
    /// The type's name, as a tool would show it.
    const NAME: &'static str;

    /// The fields, in a fixed order. An index into this slice names a field.
    fn fields() -> &'static [Field];

    /// The current value of field `index`, or `None` if there's no such field.
    fn get(&self, index: usize) -> Option<Value>;

    /// Stores an already-validated value. Only [`Describe::set`] calls this.
    #[doc(hidden)]
    fn write(&mut self, index: usize, value: Value);

    /// Validates `value` against field `index`, then stores it. On error,
    /// nothing changes.
    fn set(&mut self, index: usize, value: Value) -> Result<(), FieldError> {
        let ty = Self::NAME;
        let field = Self::fields().get(index).ok_or(FieldError::NoField { ty, index })?;
        validate(ty, field, value)?;
        self.write(index, value);
        Ok(())
    }
}

/// The checks every write goes through.
pub(crate) fn validate(ty: &'static str, field: &Field, value: Value) -> Result<(), FieldError> {
    let name = field.name;
    if !value.is(field.kind) {
        return Err(FieldError::WrongKind { ty, field: name });
    }
    let finite = match value {
        Value::F32(v) => v.is_finite(),
        Value::Vec3(v) => v.is_finite(),
        Value::Quat(q) => q.is_finite(),
        Value::Color(c) => [c.r, c.g, c.b, c.a].iter().all(|x| x.is_finite()),
        _ => true,
    };
    if !finite {
        return Err(FieldError::NotFinite { ty, field: name });
    }
    if let (Value::F32(v), Some(range)) = (value, field.range)
        && !(range.min..=range.max).contains(&v)
    {
        return Err(FieldError::OutOfRange { ty, field: name, value: v, range });
    }
    if let Value::Quat(q) = value
        && (q.length() - 1.0).abs() > 1e-3
    {
        return Err(FieldError::NotUnit { ty, field: name });
    }
    Ok(())
}
