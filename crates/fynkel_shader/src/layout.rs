//! Where each value property sits in the material's uniform block
//! (ADR-0031 § 4). The rules are the engine's own, not WGSL's struct
//! layout, so the cooker, the renderer and a runtime `material.set` agree
//! byte for byte:
//!
//! - properties in file order, textures skipped;
//! - `float`, `int`, `bool`: 4 bytes, aligned to 4 (`int` as `i32`,
//!   `bool` as a `u32` 0 or 1);
//! - `vec2`: 8 bytes, aligned to 8;
//! - `vec3`, `color3`: 16 bytes (the last 4 padding), aligned to 16;
//! - `vec4`, `color`: 16 bytes, aligned to 16;
//! - the block is rounded up to 16 bytes, and is at least 16.
//!
//! The shader reads the block as `array<vec4<f32>, size / 16>` and
//! unpacks each field by its offset (`contract::prelude`).

use crate::{Interface, PropertyKind, ValueType};

#[derive(Clone, PartialEq, Debug)]
pub struct Field {
    pub name: String,
    pub ty: ValueType,
    /// Byte offset in the block.
    pub offset: u32,
}

impl Field {
    /// Bytes the field takes (padding included).
    pub const fn size(&self) -> u32 {
        match self.ty {
            ValueType::Float | ValueType::Int | ValueType::Bool => 4,
            ValueType::Vec2 => 8,
            ValueType::Vec3 | ValueType::Color3 | ValueType::Vec4 | ValueType::Color => 16,
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Layout {
    pub fields: Vec<Field>,
    /// The block's size in bytes: a multiple of 16, at least 16.
    pub size: u32,
}

impl Layout {
    pub fn of(interface: &Interface) -> Layout {
        let mut fields = Vec::new();
        let mut offset = 0u32;
        for p in &interface.properties {
            let PropertyKind::Value { ty, .. } = p.kind else {
                continue;
            };
            let mut field = Field {
                name: p.name.clone(),
                ty,
                offset: 0,
            };
            let size = field.size();
            let align = size.min(16);
            offset = offset.div_ceil(align) * align;
            field.offset = offset;
            offset += size;
            fields.push(field);
        }
        Layout {
            fields,
            size: offset.div_ceil(16).max(1) * 16,
        }
    }

    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// The block's bytes (little-endian): each field from `value(name)`
    /// or, when that gives nothing, the property's default. A value of the
    /// wrong length is cut or padded with zeros — callers check it first
    /// against the interface.
    pub fn pack<'v>(
        &self,
        interface: &Interface,
        value: impl Fn(&str) -> Option<&'v [f64]>,
    ) -> Vec<u8> {
        let mut bytes = vec![0u8; self.size as usize];
        for f in &self.fields {
            let default = match interface.property(&f.name).map(|p| &p.kind) {
                Some(PropertyKind::Value { default, .. }) => default.as_slice(),
                _ => &[],
            };
            let v = value(&f.name).unwrap_or(default);
            write_field(&mut bytes, f, v);
        }
        bytes
    }
}

/// Writes one field's value into `bytes` at its offset.
pub fn write_field(bytes: &mut [u8], field: &Field, v: &[f64]) {
    let at = field.offset as usize;
    let get = |i: usize| v.get(i).copied().unwrap_or(0.0);
    match field.ty {
        ValueType::Int => bytes[at..at + 4].copy_from_slice(&(get(0) as i32).to_le_bytes()),
        ValueType::Bool => {
            bytes[at..at + 4].copy_from_slice(&u32::from(get(0) != 0.0).to_le_bytes())
        }
        ty => {
            for i in 0..ty.components() {
                let o = at + i * 4;
                bytes[o..o + 4].copy_from_slice(&(get(i) as f32).to_le_bytes());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    fn layout(props: &str) -> (Interface, Layout) {
        let s = parse(
            None,
            &format!("shader \"A\" {{\n properties {{\n{props}\n }}\n}}\n"),
        )
        .expect("parses");
        (s.interface, s.layout)
    }

    #[test]
    fn fields_follow_the_engine_rules() {
        let (_, l) =
            layout("  a float\n  b vec3\n  c float\n  d vec2\n  t texture\n  e int\n  f color");
        let at: Vec<_> = l
            .fields
            .iter()
            .map(|f| (f.name.as_str(), f.offset))
            .collect();
        // a 0; b aligned to 16; c after b's padded 16; d aligned to 8; e 4; f aligned to 16.
        assert_eq!(
            at,
            [
                ("a", 0),
                ("b", 16),
                ("c", 32),
                ("d", 40),
                ("e", 48),
                ("f", 64)
            ]
        );
        assert_eq!(l.size, 80);
    }

    #[test]
    fn an_empty_block_is_one_vec4() {
        let (_, l) = layout("  t texture");
        assert!(l.fields.is_empty());
        assert_eq!(l.size, 16);
    }

    #[test]
    fn packing_writes_values_over_defaults() {
        let (i, l) =
            layout("  a float = 0.5\n  n int = -3\n  on bool = true\n  c color3 = (1, 0.5, 0.25)");
        let set = [2.0f64];
        let bytes = l.pack(&i, |name| (name == "a").then_some(&set[..]));
        let f = |o: usize| f32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
        assert_eq!(f(0), 2.0);
        assert_eq!(i32::from_le_bytes(bytes[4..8].try_into().unwrap()), -3);
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 1);
        assert_eq!((f(16), f(20), f(24), f(28)), (1.0, 0.5, 0.25, 0.0));
        assert_eq!(bytes.len(), 32);
    }
}
