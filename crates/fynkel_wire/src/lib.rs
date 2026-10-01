//! Wire codecs (ADR-0004, ADR-0012). Records are fixed-size little-endian
//! bodies laid out by `fynkel_contracts::schema`; strings live in a tail
//! referenced by `(offset, len)` relative to the tail start. Reads are
//! unaligned so any `Uint8List` view works.

#![forbid(unsafe_code)]

#[rustfmt::skip]
#[allow(clippy::all)]
pub mod generated;
pub mod material;

pub use generated::{
    component_value_size, decode_component_value, encode_component_value, SCHEMA_FINGERPRINT,
};

use fynkel_contracts::schema::HasEnumSchema;
use fynkel_contracts::{AssetId, AssetRef, Entity, ObjectRef};
use glam::{Quat, Vec3, Vec4};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireError {
    /// Record slice shorter than the record size.
    Truncated { needed: usize, got: usize },
    /// String reference outside the tail or not UTF-8.
    BadString { offset: u32, len: u32 },
    /// Enum index outside the variant list (a newer peer).
    BadEnum { name: &'static str, index: u32 },
}

/// Collects strings during encoding; the tail is appended after the records.
#[derive(Default, Debug)]
pub struct StringTable {
    bytes: Vec<u8>,
}

impl StringTable {
    pub fn new() -> Self {
        Self::default()
    }
    /// An empty table writing into `bytes` (cleared), keeping its
    /// capacity: a writer reused every frame allocates nothing once warm.
    pub fn with_buffer(mut bytes: Vec<u8>) -> Self {
        bytes.clear();
        Self { bytes }
    }
    pub fn put(&mut self, s: &str) -> (u32, u32) {
        let off = self.bytes.len() as u32;
        self.bytes.extend_from_slice(s.as_bytes());
        (off, s.len() as u32)
    }
    pub fn len(&self) -> usize {
        self.bytes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

/// Implemented by generated code for every schema record.
pub trait WireRecord: Sized {
    /// Body size in bytes (padded to 8 for top-level records).
    const SIZE: usize;
    /// Writes exactly `SIZE` bytes at `out[..SIZE]`.
    fn encode(&self, out: &mut [u8], strings: &mut StringTable);
    /// Reads from `b[..SIZE]`; `tail` resolves string references.
    fn decode(b: &[u8], tail: &[u8]) -> Result<Self, WireError>;

    fn to_vec(&self, strings: &mut StringTable) -> Vec<u8> {
        let mut v = vec![0u8; Self::SIZE];
        self.encode(&mut v, strings);
        v
    }
}

#[inline]
pub fn check_len(b: &[u8], needed: usize) -> Result<(), WireError> {
    if b.len() < needed {
        Err(WireError::Truncated {
            needed,
            got: b.len(),
        })
    } else {
        Ok(())
    }
}

#[inline]
pub fn put_u16(b: &mut [u8], off: usize, v: u16) {
    b[off..off + 2].copy_from_slice(&v.to_le_bytes());
}
#[inline]
pub fn put_u32(b: &mut [u8], off: usize, v: u32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}
#[inline]
pub fn put_i32(b: &mut [u8], off: usize, v: i32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}
#[inline]
pub fn put_u64(b: &mut [u8], off: usize, v: u64) {
    b[off..off + 8].copy_from_slice(&v.to_le_bytes());
}
#[inline]
pub fn put_f32(b: &mut [u8], off: usize, v: f32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}
#[inline]
pub fn put_bool(b: &mut [u8], off: usize, v: bool) {
    b[off] = v as u8;
}
#[inline]
pub fn put_vec3(b: &mut [u8], off: usize, v: Vec3) {
    put_f32(b, off, v.x);
    put_f32(b, off + 4, v.y);
    put_f32(b, off + 8, v.z);
}
#[inline]
pub fn put_quat(b: &mut [u8], off: usize, v: Quat) {
    put_f32(b, off, v.x);
    put_f32(b, off + 4, v.y);
    put_f32(b, off + 8, v.z);
    put_f32(b, off + 12, v.w);
}
#[inline]
pub fn put_vec4(b: &mut [u8], off: usize, v: Vec4) {
    put_f32(b, off, v.x);
    put_f32(b, off + 4, v.y);
    put_f32(b, off + 8, v.z);
    put_f32(b, off + 12, v.w);
}
#[inline]
pub fn put_entity(b: &mut [u8], off: usize, v: Entity) {
    b[off..off + 16].copy_from_slice(&v.to_bits().to_le_bytes());
}
#[inline]
pub fn put_opt_entity(b: &mut [u8], off: usize, v: Option<Entity>) {
    put_entity(b, off, v.unwrap_or(Entity::NULL));
}
/// The handle alone: an authoring id never crosses the wire (ADR-0035).
#[inline]
pub fn put_object_ref(b: &mut [u8], off: usize, v: ObjectRef) {
    put_entity(b, off, v.entity);
}
#[inline]
pub fn put_str(b: &mut [u8], off: usize, v: &str, strings: &mut StringTable) {
    let (o, l) = strings.put(v);
    put_u32(b, off, o);
    put_u32(b, off + 4, l);
}
#[inline]
pub fn put_asset_ref(b: &mut [u8], off: usize, v: &AssetRef, strings: &mut StringTable) {
    b[off..off + 16].copy_from_slice(v.asset.as_bytes());
    put_str(b, off + 16, &v.sub, strings);
}
/// None = a nil id and an empty sub.
#[inline]
pub fn put_opt_asset_ref(
    b: &mut [u8],
    off: usize,
    v: Option<&AssetRef>,
    strings: &mut StringTable,
) {
    match v {
        Some(r) => put_asset_ref(b, off, r, strings),
        None => put_asset_ref(b, off, &AssetRef::default(), strings),
    }
}
#[inline]
pub fn put_enum<T: HasEnumSchema>(b: &mut [u8], off: usize, v: &T) {
    put_u32(b, off, v.to_index());
}

#[inline]
pub fn get_u16(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes(b[off..off + 2].try_into().unwrap())
}
#[inline]
pub fn get_u32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}
#[inline]
pub fn get_i32(b: &[u8], off: usize) -> i32 {
    i32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}
#[inline]
pub fn get_u64(b: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(b[off..off + 8].try_into().unwrap())
}
#[inline]
pub fn get_f32(b: &[u8], off: usize) -> f32 {
    f32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}
#[inline]
pub fn get_bool(b: &[u8], off: usize) -> bool {
    b[off] != 0
}
#[inline]
pub fn get_vec3(b: &[u8], off: usize) -> Vec3 {
    Vec3::new(get_f32(b, off), get_f32(b, off + 4), get_f32(b, off + 8))
}
#[inline]
pub fn get_quat(b: &[u8], off: usize) -> Quat {
    Quat::from_xyzw(
        get_f32(b, off),
        get_f32(b, off + 4),
        get_f32(b, off + 8),
        get_f32(b, off + 12),
    )
}
#[inline]
pub fn get_vec4(b: &[u8], off: usize) -> Vec4 {
    Vec4::new(
        get_f32(b, off),
        get_f32(b, off + 4),
        get_f32(b, off + 8),
        get_f32(b, off + 12),
    )
}
#[inline]
pub fn get_entity(b: &[u8], off: usize) -> Entity {
    Entity::from_bits(u128::from_le_bytes(b[off..off + 16].try_into().unwrap()))
}
#[inline]
pub fn get_object_ref(b: &[u8], off: usize) -> ObjectRef {
    ObjectRef::to(Entity::from_bits(u128::from_le_bytes(
        b[off..off + 16].try_into().unwrap(),
    )))
}
#[inline]
pub fn get_opt_entity(b: &[u8], off: usize) -> Option<Entity> {
    let entity = get_entity(b, off);
    (!entity.is_null()).then_some(entity)
}
#[inline]
pub fn get_asset_ref(b: &[u8], off: usize, tail: &[u8]) -> Result<AssetRef, WireError> {
    Ok(AssetRef {
        asset: AssetId::from_bytes(b[off..off + 16].try_into().unwrap()),
        sub: get_str(b, off + 16, tail)?,
    })
}
/// A nil id reads as `None` (whatever the sub says).
#[inline]
pub fn get_opt_asset_ref(b: &[u8], off: usize, tail: &[u8]) -> Result<Option<AssetRef>, WireError> {
    let r = get_asset_ref(b, off, tail)?;
    Ok(if r.asset.is_nil() { None } else { Some(r) })
}
#[inline]
pub fn get_enum<T: HasEnumSchema>(b: &[u8], off: usize) -> Result<T, WireError> {
    let index = get_u32(b, off);
    T::from_index(index).ok_or(WireError::BadEnum {
        name: T::ENUM.name,
        index,
    })
}
#[inline]
pub fn get_str(b: &[u8], off: usize, tail: &[u8]) -> Result<String, WireError> {
    let o = get_u32(b, off);
    let l = get_u32(b, off + 4);
    let err = WireError::BadString { offset: o, len: l };
    let end = (o as usize).checked_add(l as usize).ok_or(err)?;
    let bytes = tail.get(o as usize..end).ok_or(err)?;
    core::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| err)
}
