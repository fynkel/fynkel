//! Hand-written support for the generated document codecs
//! (`src/generated.rs`, emitted by `fynkel codegen` from the Rust component
//! schemas — ADR-0012): typed component values ↔ strict JSON nodes. Field
//! conventions match `docs/codegen.md`: numbers, `[x, y, z]` vectors,
//! `[x, y, z, w]` quaternions and 4-vectors, `{ "asset", "sub"? }`
//! references (an optional one absent or `null` when unbound) and
//! lowerCamel enum names. Document records (`document = "..."`) read every
//! field through [`or_default`].

use fynkel_contracts::schema::{EnumSchema, HasEnumSchema};
use fynkel_contracts::{AssetRef, Uuid};
use glam::{Quat, Vec3, Vec4};

use crate::json::{Member, Node, Value};

/// Why a node does not decode: `pointer` is relative to the node handed to
/// `from_node` (RFC 6901), so callers re-anchor it under the component.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldError {
    pub pointer: String,
    pub message: String,
}

impl FieldError {
    pub fn new(pointer: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            pointer: pointer.into(),
            message: message.into(),
        }
    }

    fn under(mut self, name: &str) -> Self {
        self.pointer = format!("/{}{}", escape(name), self.pointer);
        self
    }
}

fn escape(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

/// Implemented by generated code for every authored record.
pub trait DocumentCodec: Sized {
    fn from_node(node: &Node) -> Result<Self, FieldError>;
    fn to_node(&self) -> Node;
}

pub fn object(node: &Node) -> Result<&[Member], FieldError> {
    node.as_object()
        .ok_or_else(|| FieldError::new("", "expected an object"))
}

fn has(members: &[Member], name: &str) -> bool {
    members
        .iter()
        .any(|m| m.key == name && !matches!(m.value.value, Value::Null))
}

/// `reader(m, name)` when the field is present and not `null`, else
/// `default`: how a document record's optional fields read.
pub fn or_default<T>(
    m: &[Member],
    name: &str,
    reader: impl FnOnce(&[Member], &str) -> Result<T, FieldError>,
    default: T,
) -> Result<T, FieldError> {
    if has(m, name) {
        reader(m, name)
    } else {
        Ok(default)
    }
}

fn field<'a>(members: &'a [Member], name: &str) -> Result<&'a Node, FieldError> {
    members
        .iter()
        .find(|m| m.key == name)
        .map(|m| &m.value)
        .ok_or_else(|| FieldError::new(format!("/{}", escape(name)), "missing field"))
}

fn number(node: &Node) -> Result<f64, FieldError> {
    node.as_f64()
        .ok_or_else(|| FieldError::new("", "expected a number"))
}

pub fn f32_field(m: &[Member], name: &str) -> Result<f32, FieldError> {
    number(field(m, name)?)
        .map(|v| v as f32)
        .map_err(|e| e.under(name))
}

fn integer(node: &Node, min: i64, max: i64) -> Result<i64, FieldError> {
    let v = node
        .as_i64()
        .ok_or_else(|| FieldError::new("", "expected an integer"))?;
    if v < min || v > max {
        return Err(FieldError::new(
            "",
            format!("expected an integer in {min}..={max}, got {v}"),
        ));
    }
    Ok(v)
}

pub fn u32_field(m: &[Member], name: &str) -> Result<u32, FieldError> {
    integer(field(m, name)?, 0, u32::MAX as i64)
        .map(|v| v as u32)
        .map_err(|e| e.under(name))
}

pub fn i32_field(m: &[Member], name: &str) -> Result<i32, FieldError> {
    integer(field(m, name)?, i32::MIN as i64, i32::MAX as i64)
        .map(|v| v as i32)
        .map_err(|e| e.under(name))
}

pub fn u64_field(m: &[Member], name: &str) -> Result<u64, FieldError> {
    integer(field(m, name)?, 0, i64::MAX)
        .map(|v| v as u64)
        .map_err(|e| e.under(name))
}

pub fn bool_field(m: &[Member], name: &str) -> Result<bool, FieldError> {
    field(m, name)?
        .as_bool()
        .ok_or_else(|| FieldError::new("", "expected a boolean").under(name))
}

pub fn str_field(m: &[Member], name: &str) -> Result<String, FieldError> {
    field(m, name)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| FieldError::new("", "expected a string").under(name))
}

fn numbers<const N: usize>(node: &Node) -> Result<[f32; N], FieldError> {
    let items = node
        .as_array()
        .ok_or_else(|| FieldError::new("", format!("expected an array of {N} numbers")))?;
    if items.len() != N {
        return Err(FieldError::new(
            "",
            format!("expected {N} numbers, got {}", items.len()),
        ));
    }
    let mut out = [0f32; N];
    for (i, item) in items.iter().enumerate() {
        out[i] = number(item).map_err(|e| e.under(&i.to_string()))? as f32;
    }
    Ok(out)
}

pub fn vec3_field(m: &[Member], name: &str) -> Result<Vec3, FieldError> {
    numbers::<3>(field(m, name)?)
        .map(Vec3::from_array)
        .map_err(|e| e.under(name))
}

pub fn vec4_field(m: &[Member], name: &str) -> Result<Vec4, FieldError> {
    numbers::<4>(field(m, name)?)
        .map(Vec4::from_array)
        .map_err(|e| e.under(name))
}

pub fn quat_field(m: &[Member], name: &str) -> Result<Quat, FieldError> {
    numbers::<4>(field(m, name)?)
        .map(Quat::from_array)
        .map_err(|e| e.under(name))
}

pub fn asset_ref_field(m: &[Member], name: &str) -> Result<AssetRef, FieldError> {
    let node = field(m, name)?;
    (|| {
        let inner = object(node)?;
        let asset = str_field(inner, "asset")?;
        let asset: Uuid = asset
            .parse()
            .map_err(|e| FieldError::new("/asset", format!("{e}")))?;
        let sub = match inner.iter().find(|x| x.key == "sub") {
            Some(x) => x
                .value
                .as_str()
                .ok_or_else(|| FieldError::new("/sub", "expected a string"))?
                .to_owned(),
            None => String::new(),
        };
        Ok(AssetRef { asset, sub })
    })()
    .map_err(|e: FieldError| e.under(name))
}

/// Absent or `null` is `None`.
pub fn opt_asset_ref_field(m: &[Member], name: &str) -> Result<Option<AssetRef>, FieldError> {
    if has(m, name) {
        asset_ref_field(m, name).map(Some)
    } else {
        Ok(None)
    }
}

pub fn enum_from_node<T: HasEnumSchema>(node: &Node) -> Result<T, FieldError> {
    let text = node
        .as_str()
        .ok_or_else(|| FieldError::new("", "expected a string"))?;
    let e: &EnumSchema = &T::ENUM;
    e.variants
        .iter()
        .position(|v| *v == text)
        .and_then(|i| T::from_index(i as u32))
        .ok_or_else(|| {
            FieldError::new(
                "",
                format!(
                    "expected one of {}, got \"{text}\"",
                    e.variants
                        .iter()
                        .map(|v| format!("\"{v}\""))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )
        })
}

pub fn enum_field<T: HasEnumSchema>(m: &[Member], name: &str) -> Result<T, FieldError> {
    enum_from_node(field(m, name)?).map_err(|e| e.under(name))
}

pub fn nested_field<T: DocumentCodec>(m: &[Member], name: &str) -> Result<T, FieldError> {
    T::from_node(field(m, name)?).map_err(|e| e.under(name))
}

// ------------------------------------------------------------------ to_node

/// Shortest round-trip text of the `f32`, as a JSON number.
pub fn f32_node(v: f32) -> Node {
    let lexeme = canonical_float_f32(v);
    let value = lexeme.parse::<f64>().unwrap_or(v as f64);
    Node::synthetic(Value::Number { lexeme, value })
}

fn canonical_float_f32(v: f32) -> String {
    let s = format!("{v}");
    if s.contains(['.', 'e', 'E', 'n', 'i']) {
        s
    } else {
        format!("{s}.0")
    }
}

pub fn integer_node(v: i64) -> Node {
    Node::integer(v)
}

pub fn bool_node(v: bool) -> Node {
    Node::synthetic(Value::Bool(v))
}

pub fn str_node(v: &str) -> Node {
    Node::string(v)
}

fn array_node(items: Vec<Node>) -> Node {
    Node::synthetic(Value::Array(items))
}

pub fn vec3_node(v: Vec3) -> Node {
    array_node(vec![f32_node(v.x), f32_node(v.y), f32_node(v.z)])
}

pub fn vec4_node(v: Vec4) -> Node {
    array_node(vec![
        f32_node(v.x),
        f32_node(v.y),
        f32_node(v.z),
        f32_node(v.w),
    ])
}

pub fn quat_node(q: Quat) -> Node {
    array_node(vec![
        f32_node(q.x),
        f32_node(q.y),
        f32_node(q.z),
        f32_node(q.w),
    ])
}

pub fn asset_ref_node(r: &AssetRef) -> Node {
    let mut members = vec![("asset".to_string(), Node::string(r.asset.to_string()))];
    if !r.sub.is_empty() {
        members.push(("sub".to_string(), Node::string(r.sub.clone())));
    }
    Node::object(members)
}

/// `None` is a `null` node, which [`object_node`] leaves out.
pub fn opt_asset_ref_node(r: Option<&AssetRef>) -> Node {
    match r {
        Some(r) => asset_ref_node(r),
        None => Node::synthetic(Value::Null),
    }
}

pub fn enum_node<T: HasEnumSchema>(v: &T) -> Node {
    Node::string(T::ENUM.variants[v.to_index() as usize])
}

/// An object; `null` members (unset optional fields) are left out.
pub fn object_node(fields: Vec<(&'static str, Node)>) -> Node {
    Node::object(
        fields
            .into_iter()
            .filter(|(_, v)| !matches!(v.value, Value::Null))
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    )
}
