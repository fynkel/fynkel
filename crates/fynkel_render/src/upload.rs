//! Cooked meshes and images onto the GPU, and the missing-mesh cube.

use super::*;

pub(crate) fn upload_mesh(
    device: &wgpu::Device,
    label: &str,
    vertices: &[Vertex],
    indices: &[u32],
    skin: &[SkinWeight],
    bounds: Sphere,
) -> GpuMesh {
    // The cooked layout is the GPU layout: 18 little-endian f32 per vertex.
    let mut bytes = Vec::with_capacity(vertices.len() * Vertex::STRIDE);
    for v in vertices {
        v.write(&mut bytes);
    }
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: &bytes,
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::cast_slice(indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    // A skinned mesh's weights as cooked: four u16 joints, four f32
    // weights per vertex (24 bytes).
    let skin_buffer = (!skin.is_empty()).then(|| {
        let mut bytes = Vec::with_capacity(skin.len() * SkinWeight::STRIDE);
        for w in skin {
            w.write(&mut bytes);
        }
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: &bytes,
            usage: wgpu::BufferUsages::VERTEX,
        })
    });
    GpuMesh {
        vertex_buffer,
        index_buffer,
        index_count: indices.len() as u32,
        skin_buffer,
        bounds,
        skin_bounds: crate::culling::SkinBounds::new(vertices, skin),
    }
}

/// A top-row-first image with `mip_levels` levels concatenated in
/// `rgba8` (the cooked `TEXR` layout; 1 for a flat image) — RGBA8,
/// RGBA16F when `format` says so (an HDR sky, E1), or ASTC blocks
/// (ADR-0061; a level is whole blocks, the last ones padded).
pub(crate) struct Rgba8Image<'a> {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) format: wgpu::TextureFormat,
    pub(crate) mip_levels: u32,
    pub(crate) rgba8: &'a [u8],
}

impl<'a> Rgba8Image<'a> {
    pub(crate) fn flat(format: wgpu::TextureFormat, rgba8: &'a [u8; 4]) -> Self {
        Rgba8Image {
            width: 1,
            height: 1,
            format,
            mip_levels: 1,
            rgba8,
        }
    }
}

/// The GPU format of a cooked `TEXR` format, when `astc` (the device
/// samples ASTC) or the format is not compressed; `None` for an ASTC
/// texture the device cannot sample, which [`decode_astc`] turns into
/// RGBA8 first.
pub(crate) fn gpu_texture_format(format: u32, astc: bool) -> Option<wgpu::TextureFormat> {
    use fynkel_cooked::payload as p;
    use wgpu::{AstcBlock, AstcChannel, TextureFormat};
    let channel = if p::texture_is_srgb(format) {
        AstcChannel::UnormSrgb
    } else {
        AstcChannel::Unorm
    };
    let block = match format {
        p::TEXTURE_FORMAT_RGBA8_SRGB => return Some(TextureFormat::Rgba8UnormSrgb),
        p::TEXTURE_FORMAT_RGBA16F_LINEAR => return Some(TextureFormat::Rgba16Float),
        p::TEXTURE_FORMAT_ASTC4X4_SRGB | p::TEXTURE_FORMAT_ASTC4X4_LINEAR => AstcBlock::B4x4,
        p::TEXTURE_FORMAT_ASTC6X6_SRGB | p::TEXTURE_FORMAT_ASTC6X6_LINEAR => AstcBlock::B6x6,
        p::TEXTURE_FORMAT_ASTC8X8_SRGB | p::TEXTURE_FORMAT_ASTC8X8_LINEAR => AstcBlock::B8x8,
        _ => return Some(TextureFormat::Rgba8Unorm),
    };
    astc.then_some(TextureFormat::Astc { block, channel })
}

/// Every level of an ASTC `payload` decoded to RGBA8 (same colour
/// encoding: an sRGB texture stays sRGB-encoded), for a device without
/// ASTC. `None` when the payload is not ASTC or a level is short.
pub(crate) fn decode_astc(payload: &fynkel_cooked::payload::TexturePayload) -> Option<Vec<u8>> {
    use fynkel_cooked::payload as p;
    let (bw, bh, _) = p::texture_block(payload.format).filter(|b| b.0 > 1)?;
    let footprint = astc_decode::Footprint::new(bw, bh);
    let mut out = Vec::new();
    for level in 0..payload.mip_levels {
        let (w, h) = payload.level_size(level);
        let blocks = payload.level(level)?;
        let start = out.len();
        out.resize(start + w as usize * h as usize * 4, 0);
        let rgba = &mut out[start..];
        astc_decode::astc_decode(blocks, w, h, footprint, |x, y, c| {
            let at = (y as usize * w as usize + x as usize) * 4;
            rgba[at..at + 4].copy_from_slice(&c);
        })
        .ok()?;
    }
    Some(out)
}

/// Uploads every level of `image`.
pub(crate) fn upload_rgba8(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    image: Rgba8Image,
    label: &str,
) -> GpuTexture {
    let Rgba8Image {
        width,
        height,
        format,
        mip_levels,
        rgba8,
    } = image;
    let mip_levels = mip_levels.max(1);
    let block_bytes = format.block_copy_size(None).unwrap_or(4);
    let (bw, bh) = format.block_dimensions();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: mip_levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut at = 0usize;
    for level in 0..mip_levels {
        let (w, h) = fynkel_cooked::payload::mip_size(width, height, level);
        // A level is whole blocks; a copy covers their (physical) extent.
        let (across, down) = (w.div_ceil(bw), h.div_ceil(bh));
        let len = across as usize * down as usize * block_bytes as usize;
        let Some(bytes) = rgba8.get(at..at + len) else {
            break;
        };
        at += len;
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(across * block_bytes),
                rows_per_image: Some(down),
            },
            wgpu::Extent3d {
                width: across * bw,
                height: down * bh,
                depth_or_array_layers: 1,
            },
        );
    }
    GpuTexture {
        view: texture.create_view(&Default::default()),
        bytes: at as u64,
    }
}

pub(crate) fn cube_mesh() -> (Vec<Vertex>, Vec<u32>) {
    // Six faces of a unit cube centred at the origin; CCW winding from outside.
    // (normal, tangent u, tangent v)
    let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
        ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
        ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
        ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ];
    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    for (normal, u, v) in faces {
        let n = Vec3::from(normal);
        let u = Vec3::from(u);
        let v = Vec3::from(v);
        let base = vertices.len() as u32;
        for (su, sv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let p = (n + u * su + v * sv) * 0.5;
            vertices.push(Vertex {
                position: p.to_array(),
                normal,
                tangent: [u.x, u.y, u.z, 1.0],
                uv0: [(su + 1.0) * 0.5, (1.0 - sv) * 0.5],
                ..Vertex::default()
            });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (vertices, indices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fynkel_cooked::payload::{self as p, TexturePayload};

    /// A constant-colour ASTC block ("void extent"), any footprint.
    fn solid_block(rgba: [u16; 4]) -> [u8; 16] {
        let mut b = [0xFF; 16];
        b[..2].copy_from_slice(&[0xFC, 0xFD]);
        for (i, c) in rgba.iter().enumerate() {
            b[8 + 2 * i..10 + 2 * i].copy_from_slice(&c.to_le_bytes());
        }
        b
    }

    /// 12×6 at 6×6 with its full chain: 2×1 blocks, then one block a level.
    fn red_chain(format: u32) -> TexturePayload {
        let levels = p::full_mip_count(12, 6);
        let blocks: usize = (0..levels).map(|l| p::level_bytes(12, 6, (6, 6, 16), l) / 16).sum();
        let texels = solid_block([0xFFFF, 0, 0, 0xFFFF]).repeat(blocks);
        p::decode_texture(&p::encode_texture(12, 6, format, levels, &texels)).unwrap()
    }

    #[test]
    fn an_astc_texture_without_astc_decodes_to_the_same_rgba8_chain() {
        let t = red_chain(p::TEXTURE_FORMAT_ASTC6X6_SRGB);
        assert_eq!(gpu_texture_format(t.format, false), None);
        let rgba = decode_astc(&t).unwrap();
        let texels: usize = (0..t.mip_levels).map(|l| p::level_bytes(12, 6, (1, 1, 4), l)).sum();
        assert_eq!(rgba.len(), texels);
        assert!(rgba.chunks_exact(4).all(|c| c == [255, 0, 0, 255]));
        let rgba8 = p::decode_texture(&p::encode_texture(1, 1, p::TEXTURE_FORMAT_RGBA8_SRGB, 1, &[1; 4])).unwrap();
        assert_eq!(decode_astc(&rgba8), None);
    }

    #[test]
    fn an_astc_chain_uploads_as_blocks_where_the_gpu_samples_astc() {
        let Some((device, queue)) = crate::headless_device() else {
            return;
        };
        if !device.features().contains(wgpu::Features::TEXTURE_COMPRESSION_ASTC) {
            return;
        }
        for format in [p::TEXTURE_FORMAT_ASTC6X6_SRGB, p::TEXTURE_FORMAT_ASTC6X6_LINEAR] {
            let t = red_chain(format);
            let gpu = gpu_texture_format(t.format, true).unwrap();
            assert!(matches!(gpu, wgpu::TextureFormat::Astc { block: wgpu::AstcBlock::B6x6, .. }));
            let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
            let uploaded = upload_rgba8(
                &device,
                &queue,
                Rgba8Image {
                    width: t.width,
                    height: t.height,
                    format: gpu,
                    mip_levels: t.mip_levels,
                    rgba8: &t.texels,
                },
                "astc test",
            );
            let error = crate::block_on(scope.pop());
            assert!(error.is_none(), "{error:?}");
            assert_eq!(uploaded.bytes, t.texels.len() as u64, "every level written");
        }
    }
}
