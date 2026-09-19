//! The `MATL` chunk payload (blueprint §19.2d item 2): a cooked
//! `fynkel.PbrMaterial` in its wire form, so the renderer reads a material
//! without the document layer.
//!
//! ```text
//! MATL  u32 version (1), u32 record size, u32 tail length, u32 reserved,
//!       the record (`PbrMaterial::SIZE` bytes), the string tail
//! ```
//!
//! The record size doubles as the schema check: a build whose
//! `PbrMaterial` has a different layout decodes `None` and the caller
//! treats the asset as needing a re-cook.

use fynkel_core::material::PbrMaterial;

use crate::{StringTable, WireRecord};

pub const MATERIAL_PAYLOAD_VERSION: u32 = 1;
const HEADER: usize = 16;

pub fn encode_material(m: &PbrMaterial) -> Vec<u8> {
    let mut strings = StringTable::new();
    let record = m.to_vec(&mut strings);
    let tail = strings.into_bytes();
    let mut out = Vec::with_capacity(HEADER + record.len() + tail.len());
    out.extend_from_slice(&MATERIAL_PAYLOAD_VERSION.to_le_bytes());
    out.extend_from_slice(&(record.len() as u32).to_le_bytes());
    out.extend_from_slice(&(tail.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&record);
    out.extend_from_slice(&tail);
    out
}

/// `None` for a malformed payload or one written by a different record layout.
pub fn decode_material(bytes: &[u8]) -> Option<PbrMaterial> {
    if bytes.len() < HEADER {
        return None;
    }
    let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let (version, size, tail_len) = (u32_at(0), u32_at(4) as usize, u32_at(8) as usize);
    if version != MATERIAL_PAYLOAD_VERSION
        || size != PbrMaterial::SIZE
        || bytes.len() != HEADER.checked_add(size)?.checked_add(tail_len)?
    {
        return None;
    }
    let record = &bytes[HEADER..HEADER + size];
    let tail = &bytes[HEADER + size..];
    PbrMaterial::decode(record, tail).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fynkel_contracts::{AssetRef, Uuid};
    use fynkel_core::material::AlphaMode;

    #[test]
    fn round_trips_with_and_without_textures() {
        let plain = PbrMaterial::default();
        assert_eq!(decode_material(&encode_material(&plain)), Some(plain));
        let textured = PbrMaterial {
            metallic: 1.0,
            alpha_mode: AlphaMode::Mask,
            double_sided: true,
            base_color_texture: Some(AssetRef {
                asset: Uuid([7; 16]),
                sub: "image/0".into(),
            }),
            occlusion_texture: Some(AssetRef {
                asset: Uuid([9; 16]),
                sub: "image/0/linear".into(),
            }),
            ..PbrMaterial::default()
        };
        let bytes = encode_material(&textured);
        assert_eq!(
            bytes.len(),
            16 + PbrMaterial::SIZE + "image/0image/0/linear".len()
        );
        assert_eq!(decode_material(&bytes), Some(textured));
    }

    #[test]
    fn rejects_other_layouts_and_truncation() {
        let mut bytes = encode_material(&PbrMaterial::default());
        assert!(decode_material(&bytes[..bytes.len() - 1]).is_none());
        bytes[4] = 0;
        assert!(decode_material(&bytes).is_none());
    }
}
