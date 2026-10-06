//! GPU texture compression at cook (ADR-0061): an LDR texture is cooked
//! as ASTC, every mip level compressed on its own. The sidecar's
//! `settings.compression` picks the block (`none` | `low` 8×8 | `normal`
//! 6×6 | `high` 4×4); absent, an sRGB map is `normal` and a linear one
//! (normals, metallic/roughness, occlusion) `high`.
//!
//! WebGPU wants level 0 of a compressed texture to be whole blocks, so a
//! texture of another size is resampled (Catmull-Rom, in linear light for
//! sRGB) to the nearest multiple of its block first. The encoder is ARM's
//! astcenc at its `medium` preset, the same in every cook profile.

use ctt_astcenc::{bindings, config_init, Context, Flags, Preset, Profile, Swizzle};
use fynkel_contracts::{Code, Diagnostic};
use fynkel_document::asset::Meta;

use crate::cook::{linear_to_srgb, mip_chain, srgb_to_linear};

/// `settings.compression`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compression {
    None,
    Low,
    Normal,
    High,
}

impl Compression {
    pub fn parse(s: &str) -> Option<Compression> {
        Some(match s {
            "none" => Compression::None,
            "low" => Compression::Low,
            "normal" => Compression::Normal,
            "high" => Compression::High,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Compression::None => "none",
            Compression::Low => "low",
            Compression::Normal => "normal",
            Compression::High => "high",
        }
    }

    /// The sidecar's setting, `None` when it leaves it to the default.
    pub fn from_meta(meta: &Meta) -> Result<Option<Compression>, Diagnostic> {
        match meta.settings.get("compression") {
            None => Ok(None),
            Some(v) => v.as_str().and_then(Compression::parse).map(Some).ok_or_else(|| {
                Diagnostic::error(
                    Code::IMPORT_INVALID_DATA,
                    "settings.compression must be \"none\", \"low\", \"normal\" or \"high\"",
                )
                .with_pointer("/settings/compression")
            }),
        }
    }

    /// What `setting` means for a texture in this colour space.
    pub fn resolve(setting: Option<Compression>, srgb: bool) -> Compression {
        setting.unwrap_or(if srgb {
            Compression::Normal
        } else {
            Compression::High
        })
    }

    /// The ASTC block edge, `None` for uncompressed.
    pub fn block(self) -> Option<u32> {
        match self {
            Compression::None => None,
            Compression::Low => Some(8),
            Compression::Normal => Some(6),
            Compression::High => Some(4),
        }
    }
}

/// The nearest multiple of `block` to `n` (at least one block).
pub fn block_multiple(n: u32, block: u32) -> u32 {
    let down = n / block * block;
    let up = down + block;
    if down == 0 || n - down > up - n {
        up
    } else {
        down
    }
}

/// An ASTC texture as the cook writes it: its (possibly resampled) size,
/// the `TEXR` format, the level count and every level's blocks.
pub struct AstcTexture {
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub mip_levels: u32,
    pub blocks: Vec<u8>,
}

/// Compresses `rgba8` (`width`×`height`, top row first) with `block`×`block`
/// ASTC: resampled to whole blocks if need be, the mip chain built as
/// `mip_chain` builds it, each level encoded.
pub fn encode(width: u32, height: u32, rgba8: &[u8], srgb: bool, block: u32) -> Result<AstcTexture, String> {
    let format = fynkel_cooked::payload::astc_format(block, srgb).ok_or("no such ASTC block")?;
    let (w, h) = (block_multiple(width, block), block_multiple(height, block));
    let resampled;
    let base = if (w, h) == (width, height) {
        rgba8
    } else {
        resampled = resample(width, height, rgba8, w, h, srgb);
        &resampled
    };
    let mip_levels = fynkel_cooked::payload::full_mip_count(w, h);
    let chain = mip_chain(w, h, base, srgb);
    let profile = if srgb { Profile::LdrSrgb } else { Profile::Ldr };
    let config = config_init(profile, block, block, 1, Preset::Medium, Flags::USE_DECODE_UNORM8)
        .map_err(|e| format!("astcenc: {e:?}"))?;
    let mut context = Context::new_parallel(&config).map_err(|e| format!("astcenc: {e:?}"))?;
    let mut blocks = Vec::new();
    let mut at = 0usize;
    for level in 0..mip_levels {
        let (lw, lh) = fynkel_cooked::payload::mip_size(w, h, level);
        let len = lw as usize * lh as usize * 4;
        let mut texels = chain[at..at + len].to_vec();
        at += len;
        let size = fynkel_cooked::payload::level_bytes(w, h, (block, block, 16), level);
        let start = blocks.len();
        blocks.resize(start + size, 0);
        let mut plane = texels.as_mut_ptr() as *mut std::ffi::c_void;
        let mut image = bindings::astcenc_image {
            dim_x: lw,
            dim_y: lh,
            dim_z: 1,
            data_type: bindings::astcenc_type_ASTCENC_TYPE_U8,
            data: &mut plane,
        };
        context
            .compress(&mut image, Swizzle::IDENTITY, &mut blocks[start..])
            .map_err(|e| format!("astcenc: {e:?}"))?;
        context.compress_reset().map_err(|e| format!("astcenc: {e:?}"))?;
    }
    Ok(AstcTexture {
        width: w,
        height: h,
        format,
        mip_levels,
        blocks,
    })
}

/// `rgba8` resized to `w`×`h` with Catmull-Rom, colour in linear light
/// when `srgb`, alpha linear.
fn resample(width: u32, height: u32, rgba8: &[u8], w: u32, h: u32, srgb: bool) -> Vec<u8> {
    let linear: Vec<f32> = rgba8
        .chunks_exact(4)
        .flat_map(|t| {
            let c = |v: u8| {
                let v = v as f32 / 255.0;
                if srgb {
                    srgb_to_linear(v)
                } else {
                    v
                }
            };
            [c(t[0]), c(t[1]), c(t[2]), t[3] as f32 / 255.0]
        })
        .collect();
    let source = image::ImageBuffer::<image::Rgba<f32>, _>::from_raw(width, height, linear)
        .expect("rgba8 holds width × height texels");
    let resized = image::imageops::resize(&source, w, h, image::imageops::FilterType::CatmullRom);
    resized
        .into_raw()
        .chunks_exact(4)
        .flat_map(|t| {
            let c = |v: f32| {
                let v = if srgb { linear_to_srgb(v) } else { v.clamp(0.0, 1.0) };
                (v * 255.0 + 0.5) as u8
            };
            [c(t[0]), c(t[1]), c(t[2]), (t[3].clamp(0.0, 1.0) * 255.0 + 0.5) as u8]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fynkel_cooked::payload as p;

    #[test]
    fn defaults_follow_the_colour_space_and_blocks_round_to_the_nearest_multiple() {
        assert_eq!(Compression::resolve(None, true), Compression::Normal);
        assert_eq!(Compression::resolve(None, false), Compression::High);
        assert_eq!(Compression::resolve(Some(Compression::None), true).block(), None);
        assert_eq!(Compression::parse("bogus"), None);
        assert_eq!(block_multiple(1024, 6), 1026);
        assert_eq!(block_multiple(1024, 4), 1024);
        assert_eq!(block_multiple(1370, 6), 1368);
        assert_eq!(block_multiple(3, 8), 8, "at least one block");
    }

    /// Decodes level `level` of `t` back to RGBA8 with astcenc.
    fn decode_level(t: &AstcTexture, block: u32, srgb: bool, level: u32) -> Vec<u8> {
        let (w, h) = p::mip_size(t.width, t.height, level);
        let mut at = 0;
        for l in 0..level {
            at += p::level_bytes(t.width, t.height, (block, block, 16), l);
        }
        let len = p::level_bytes(t.width, t.height, (block, block, 16), level);
        let profile = if srgb { Profile::LdrSrgb } else { Profile::Ldr };
        let config = config_init(profile, block, block, 1, Preset::Medium, Flags::DECOMPRESS_ONLY).unwrap();
        let mut context = Context::new(&config).unwrap();
        let mut out = vec![0u8; (w * h * 4) as usize];
        let mut plane = out.as_mut_ptr() as *mut std::ffi::c_void;
        let mut image = bindings::astcenc_image {
            dim_x: w,
            dim_y: h,
            dim_z: 1,
            data_type: bindings::astcenc_type_ASTCENC_TYPE_U8,
            data: &mut plane,
        };
        context.decompress(&t.blocks[at..at + len], &mut image, Swizzle::IDENTITY).unwrap();
        out
    }

    #[test]
    fn an_odd_sized_texture_is_resampled_to_whole_blocks_and_decodes_close_to_its_source() {
        // 100×70 at 6×6 → 102×72; a smooth ramp, so the error stays small.
        let (w, h) = (100u32, 70u32);
        let source: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                let (x, y) = (i % w, i / w);
                [(x * 255 / w) as u8, (y * 255 / h) as u8, 96, 255]
            })
            .collect();
        let t = encode(w, h, &source, true, 6).unwrap();
        assert_eq!((t.width, t.height, t.format), (102, 72, p::TEXTURE_FORMAT_ASTC6X6_SRGB));
        assert_eq!(t.mip_levels, p::full_mip_count(102, 72));
        let total: usize = (0..t.mip_levels).map(|l| p::level_bytes(102, 72, (6, 6, 16), l)).sum();
        assert_eq!(t.blocks.len(), total, "every level, whole blocks");
        let decoded = decode_level(&t, 6, true, 0);
        let resampled = resample(w, h, &source, 102, 72, true);
        let worst = decoded.iter().zip(&resampled).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
        assert!(worst <= 12, "worst channel error {worst}");
        // A flat colour survives to the last level.
        let flat = [200u8, 40, 10, 255].repeat(64 * 64);
        let t = encode(64, 64, &flat, false, 4).unwrap();
        assert_eq!((t.width, t.format), (64, p::TEXTURE_FORMAT_ASTC4X4_LINEAR), "no resample");
        let top = decode_level(&t, 4, false, t.mip_levels - 1);
        assert!(top.iter().zip([200u8, 40, 10, 255]).all(|(a, b)| a.abs_diff(b) <= 1), "{top:?}");
    }
}
