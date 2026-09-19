//! Component/record schema metadata (ADR-0012). Rust structs annotated with
//! `#[derive(Schema)]` (crate `fynkel_schema_derive`) produce a
//! [`RecordSchema`]; `fynkel codegen` turns the registry of schemas into JSON
//! Schema, Dart codecs and Rust wire codecs, and derives the schema
//! fingerprint from the canonical text, so nothing is hand-maintained.
//!
//! Wire layout rule (ADR-0004): fields in declaration order, each aligned to
//! its natural alignment, record size rounded up to 8 (also when nested, so
//! a record has one size everywhere), little-endian. Strings are
//! `(offset: u32, len: u32)` into the buffer's string tail.

use core::fmt::Write as _;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchemaKind {
    Component,
    Command,
    Event,
    Record,
}

impl SchemaKind {
    pub const fn name(self) -> &'static str {
        match self {
            SchemaKind::Component => "component",
            SchemaKind::Command => "command",
            SchemaKind::Event => "event",
            SchemaKind::Record => "record",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldType {
    F32,
    U32,
    I32,
    U64,
    /// One byte on the wire, `0`/`1`.
    Bool,
    /// Three `f32`.
    Vec3,
    /// Four `f32`, `[x, y, z, w]`.
    Quat,
    /// `Entity` handle, `u64`.
    Entity,
    /// `Option<Entity>`, `u64` with `0` = none.
    OptionalEntity,
    /// `(offset: u32, len: u32)` into the buffer tail, UTF-8.
    Str,
    /// Another schema inlined at its own alignment.
    Nested(&'static RecordSchema),
}

impl FieldType {
    pub const fn name(&self) -> &'static str {
        match self {
            FieldType::F32 => "f32",
            FieldType::U32 => "u32",
            FieldType::I32 => "i32",
            FieldType::U64 => "u64",
            FieldType::Bool => "bool",
            FieldType::Vec3 => "vec3",
            FieldType::Quat => "quat",
            FieldType::Entity => "entity",
            FieldType::OptionalEntity => "entity?",
            FieldType::Str => "str",
            FieldType::Nested(s) => s.name,
        }
    }

    pub const fn wire_size(&self) -> usize {
        match self {
            FieldType::F32 | FieldType::U32 | FieldType::I32 => 4,
            FieldType::U64 | FieldType::Entity | FieldType::OptionalEntity => 8,
            FieldType::Bool => 1,
            FieldType::Vec3 => 12,
            FieldType::Quat => 16,
            FieldType::Str => 8,
            FieldType::Nested(s) => s.wire_size(),
        }
    }

    pub const fn wire_align(&self) -> usize {
        match self {
            FieldType::F32
            | FieldType::U32
            | FieldType::I32
            | FieldType::Vec3
            | FieldType::Quat => 4,
            FieldType::U64 | FieldType::Entity | FieldType::OptionalEntity => 8,
            FieldType::Bool => 1,
            FieldType::Str => 4,
            FieldType::Nested(_) => 8,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldSchema {
    pub name: &'static str,
    pub ty: FieldType,
    pub doc: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordSchema {
    pub kind: SchemaKind,
    /// Stable numeric id within its kind (component id, command kind, event
    /// kind); `0` for plain records.
    pub id: u32,
    /// Stable name, e.g. `fynkel.Transform`.
    pub name: &'static str,
    /// Fully qualified Rust path, for generated Rust codecs.
    pub rust_path: &'static str,
    pub doc: &'static str,
    pub fields: &'static [FieldSchema],
}

impl RecordSchema {
    /// Unpadded size (fields laid out at natural alignment).
    const fn fields_end(&self) -> usize {
        let mut off = 0;
        let mut i = 0;
        while i < self.fields.len() {
            let ty = &self.fields[i].ty;
            off = align_up(off, ty.wire_align());
            off += ty.wire_size();
            i += 1;
        }
        off
    }

    /// Record size, top-level or nested: padded to 8.
    pub const fn wire_size(&self) -> usize {
        align_up(self.fields_end(), 8)
    }

    /// Byte offset of each field, declaration order.
    pub fn offsets(&self) -> Vec<usize> {
        let mut out = Vec::with_capacity(self.fields.len());
        let mut off = 0;
        for f in self.fields {
            off = align_up(off, f.ty.wire_align());
            out.push(off);
            off += f.ty.wire_size();
        }
        out
    }

    pub fn has_strings(&self) -> bool {
        self.fields.iter().any(|f| match f.ty {
            FieldType::Str => true,
            FieldType::Nested(s) => s.has_strings(),
            _ => false,
        })
    }

    /// Canonical single-line description that the fingerprint hashes. Docs
    /// and Rust paths are excluded: they do not change the wire contract.
    pub fn canonical(&self) -> String {
        let mut s = String::new();
        let _ = write!(s, "{}:{}:{}[", self.kind.name(), self.id, self.name);
        for (i, f) in self.fields.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            let _ = write!(s, "{}:{}", f.name, f.ty.name());
        }
        s.push(']');
        s
    }
}

pub const fn align_up(v: usize, align: usize) -> usize {
    v.div_ceil(align) * align
}

/// Implemented by `#[derive(Schema)]`.
pub trait HasSchema {
    const SCHEMA: RecordSchema;
}

/// FNV-1a 64 over the canonical text of every schema, in registry order,
/// prefixed by the wire-layout rule version.
pub fn fingerprint(schemas: &[&RecordSchema]) -> u64 {
    const LAYOUT_RULE: &str = "fynkel-wire-1";
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for b in bytes {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h ^= 0xff;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    feed(LAYOUT_RULE.as_bytes());
    for s in schemas {
        feed(s.canonical().as_bytes());
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    const INNER: RecordSchema = RecordSchema {
        kind: SchemaKind::Record,
        id: 0,
        name: "t.Inner",
        rust_path: "t::Inner",
        doc: "",
        fields: &[
            FieldSchema {
                name: "a",
                ty: FieldType::Bool,
                doc: "",
            },
            FieldSchema {
                name: "b",
                ty: FieldType::Vec3,
                doc: "",
            },
        ],
    };
    const OUTER: RecordSchema = RecordSchema {
        kind: SchemaKind::Command,
        id: 3,
        name: "t.Outer",
        rust_path: "t::Outer",
        doc: "",
        fields: &[
            FieldSchema {
                name: "id",
                ty: FieldType::U32,
                doc: "",
            },
            FieldSchema {
                name: "e",
                ty: FieldType::Entity,
                doc: "",
            },
            FieldSchema {
                name: "inner",
                ty: FieldType::Nested(&INNER),
                doc: "",
            },
            FieldSchema {
                name: "s",
                ty: FieldType::Str,
                doc: "",
            },
        ],
    };

    #[test]
    fn layout_follows_natural_alignment() {
        assert_eq!(INNER.offsets(), vec![0, 4]);
        assert_eq!(INNER.wire_size(), 16);
        assert_eq!(OUTER.offsets(), vec![0, 8, 16, 32]);
        assert_eq!(OUTER.wire_size(), 40);
        assert!(OUTER.has_strings());
        assert_eq!(
            OUTER.canonical(),
            "command:3:t.Outer[id:u32,e:entity,inner:t.Inner,s:str]"
        );
    }

    #[test]
    fn fingerprint_is_stable_and_sensitive() {
        let a = fingerprint(&[&INNER, &OUTER]);
        assert_eq!(a, fingerprint(&[&INNER, &OUTER]));
        assert_ne!(a, fingerprint(&[&OUTER, &INNER]));
        assert_ne!(a, fingerprint(&[&INNER]));
    }
}
