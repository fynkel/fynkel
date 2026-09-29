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
    }
}

/// A top-row-first image with `mip_levels` levels concatenated in
/// `rgba8` (the cooked `TEXR` layout; 1 for a flat image) — RGBA8, or
/// RGBA16F when `format` says so (an HDR sky, E1).
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
    let texel = format.block_copy_size(None).unwrap_or(4);
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
        let len = w as usize * h as usize * texel as usize;
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
                bytes_per_row: Some(w * texel),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
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
