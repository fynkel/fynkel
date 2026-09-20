//! The typed chunk payloads (`MESH`, `TEXR`, `CLIP`): the layouts the
//! cooker writes and the runtime reads. They live here, next to the
//! container, so a loader needs nothing from the importers. Everything is
//! little-endian; a malformed payload decodes to `None`, never panics.
//!
//! ```text
//! MESH  u32 vertex count, u32 index count, [f32; 3] bounds min,
//!       [f32; 3] bounds max, vertices (72 bytes each), u32 indices
//! TEXR  u32 width, u32 height, u32 format, u32 mip levels (1),
//!       width × height × 4 texels, top row first
//! CLIP  u32 sample rate, u32 channels, u64 frames, interleaved f32 samples
//! ```

pub const TEXTURE_FORMAT_RGBA8_SRGB: u32 = 1;
pub const TEXTURE_FORMAT_RGBA8_LINEAR: u32 = 2;

/// The one vertex layout every mesh is normalized to (72 bytes): position,
/// normal, tangent (xyz + handedness), two uv sets, linear RGBA colour.
/// Missing attributes take defaults: flat normals, zero tangents, `[0, 0]`
/// uvs, white colour.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub tangent: [f32; 4],
    pub uv0: [f32; 2],
    pub uv1: [f32; 2],
    pub color: [f32; 4],
}

impl Default for Vertex {
    fn default() -> Self {
        Vertex {
            position: [0.0; 3],
            normal: [0.0, 0.0, 1.0],
            tangent: [0.0; 4],
            uv0: [0.0; 2],
            uv1: [0.0; 2],
            color: [1.0; 4],
        }
    }
}

impl Vertex {
    pub const STRIDE: usize = 72;

    pub fn write(&self, out: &mut Vec<u8>) {
        for f in self
            .position
            .iter()
            .chain(&self.normal)
            .chain(&self.tangent)
            .chain(&self.uv0)
            .chain(&self.uv1)
            .chain(&self.color)
        {
            out.extend_from_slice(&f.to_le_bytes());
        }
    }

    pub fn read(bytes: &[u8]) -> Option<Vertex> {
        if bytes.len() < Self::STRIDE {
            return None;
        }
        let mut floats = [0f32; 18];
        for (i, f) in floats.iter_mut().enumerate() {
            *f = f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
        }
        Some(Vertex {
            position: [floats[0], floats[1], floats[2]],
            normal: [floats[3], floats[4], floats[5]],
            tangent: [floats[6], floats[7], floats[8], floats[9]],
            uv0: [floats[10], floats[11]],
            uv1: [floats[12], floats[13]],
            color: [floats[14], floats[15], floats[16], floats[17]],
        })
    }
}

/// A `MESH` payload.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshPayload {
    pub bounds_min: [f32; 3],
    pub bounds_max: [f32; 3],
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

pub fn encode_mesh(
    vertices: &[Vertex],
    indices: &[u32],
    bounds_min: [f32; 3],
    bounds_max: [f32; 3],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(32 + vertices.len() * Vertex::STRIDE + indices.len() * 4);
    out.extend_from_slice(&(vertices.len() as u32).to_le_bytes());
    out.extend_from_slice(&(indices.len() as u32).to_le_bytes());
    for v in bounds_min.iter().chain(bounds_max.iter()) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for v in vertices {
        v.write(&mut out);
    }
    for i in indices {
        out.extend_from_slice(&i.to_le_bytes());
    }
    out
}

pub fn decode_mesh(bytes: &[u8]) -> Option<MeshPayload> {
    if bytes.len() < 32 {
        return None;
    }
    let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let f32_at = |at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let (vertex_count, index_count) = (u32_at(0) as usize, u32_at(4) as usize);
    let bounds_min = [f32_at(8), f32_at(12), f32_at(16)];
    let bounds_max = [f32_at(20), f32_at(24), f32_at(28)];
    let vertices_len = vertex_count.checked_mul(Vertex::STRIDE)?;
    let indices_len = index_count.checked_mul(4)?;
    if bytes.len()
        != 32usize
            .checked_add(vertices_len)?
            .checked_add(indices_len)?
    {
        return None;
    }
    let mut vertices = Vec::with_capacity(vertex_count);
    for i in 0..vertex_count {
        let at = 32 + i * Vertex::STRIDE;
        vertices.push(Vertex::read(&bytes[at..at + Vertex::STRIDE])?);
    }
    let mut indices = Vec::with_capacity(index_count);
    for i in 0..index_count {
        let at = 32 + vertices_len + i * 4;
        indices.push(u32_at(at));
    }
    if indices.iter().any(|&i| i as usize >= vertex_count) {
        return None;
    }
    Some(MeshPayload {
        bounds_min,
        bounds_max,
        vertices,
        indices,
    })
}

/// A `TEXR` payload: RGBA8, top row first, every mip level concatenated
/// from the base down (M3-08b): level `i` is `max(width >> i, 1)` by
/// `max(height >> i, 1)` texels. `mip_levels` is 1 for a texture cooked
/// without a chain; a full chain ends at 1×1.
#[derive(Clone, Debug, PartialEq)]
pub struct TexturePayload {
    pub width: u32,
    pub height: u32,
    /// `TEXTURE_FORMAT_RGBA8_SRGB` or `TEXTURE_FORMAT_RGBA8_LINEAR`.
    pub format: u32,
    pub mip_levels: u32,
    /// All levels, base first.
    pub rgba8: Vec<u8>,
}

impl TexturePayload {
    /// Size of level `i`.
    pub fn level_size(&self, level: u32) -> (u32, u32) {
        mip_size(self.width, self.height, level)
    }

    /// The texels of level `i` (`None` past `mip_levels`).
    pub fn level(&self, level: u32) -> Option<&[u8]> {
        if level >= self.mip_levels {
            return None;
        }
        let mut at = 0usize;
        for l in 0..level {
            at += level_bytes(self.width, self.height, l);
        }
        let len = level_bytes(self.width, self.height, level);
        self.rgba8.get(at..at + len)
    }

    /// Level 0.
    pub fn base(&self) -> &[u8] {
        self.level(0).unwrap_or(&[])
    }
}

/// Size of mip level `level` of a `width`×`height` texture.
pub fn mip_size(width: u32, height: u32, level: u32) -> (u32, u32) {
    ((width >> level).max(1), (height >> level).max(1))
}

/// Number of levels in a full chain (down to 1×1).
pub fn full_mip_count(width: u32, height: u32) -> u32 {
    32 - width.max(height).max(1).leading_zeros()
}

fn level_bytes(width: u32, height: u32, level: u32) -> usize {
    let (w, h) = mip_size(width, height, level);
    w as usize * h as usize * 4
}

/// Bytes of a chain of `mip_levels` levels, or `None` on overflow.
fn chain_bytes(width: u32, height: u32, mip_levels: u32) -> Option<usize> {
    let mut total = 0usize;
    for l in 0..mip_levels {
        let (w, h) = mip_size(width, height, l);
        total = total.checked_add((w as usize).checked_mul(h as usize)?.checked_mul(4)?)?;
    }
    Some(total)
}

/// `rgba8` holds `mip_levels` levels concatenated (`chain_bytes`).
pub fn encode_texture(
    width: u32,
    height: u32,
    format: u32,
    mip_levels: u32,
    rgba8: &[u8],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(16 + rgba8.len());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&format.to_le_bytes());
    out.extend_from_slice(&mip_levels.to_le_bytes());
    out.extend_from_slice(rgba8);
    out
}

pub fn decode_texture(bytes: &[u8]) -> Option<TexturePayload> {
    if bytes.len() < 16 {
        return None;
    }
    let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let (width, height, format, mip_levels) = (u32_at(0), u32_at(4), u32_at(8), u32_at(12));
    if width == 0
        || height == 0
        || mip_levels == 0
        || mip_levels > full_mip_count(width, height)
        || !(format == TEXTURE_FORMAT_RGBA8_SRGB || format == TEXTURE_FORMAT_RGBA8_LINEAR)
    {
        return None;
    }
    if bytes.len() != 16usize.checked_add(chain_bytes(width, height, mip_levels)?)? {
        return None;
    }
    Some(TexturePayload {
        width,
        height,
        format,
        mip_levels,
        rgba8: bytes[16..].to_vec(),
    })
}

/// A `CLIP` payload: interleaved `f32` samples in `[-1, 1]`.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipPayload {
    pub sample_rate: u32,
    pub channels: u32,
    pub frames: u64,
    pub samples: Vec<f32>,
}

pub fn encode_clip(sample_rate: u32, channels: u32, samples: &[f32]) -> Vec<u8> {
    let frames = if channels == 0 {
        0
    } else {
        (samples.len() / channels as usize) as u64
    };
    let mut out = Vec::with_capacity(16 + samples.len() * 4);
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&frames.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

pub fn decode_clip(bytes: &[u8]) -> Option<ClipPayload> {
    if bytes.len() < 16 {
        return None;
    }
    let sample_rate = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    let channels = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    let frames = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
    let count = usize::try_from(frames)
        .ok()?
        .checked_mul(channels as usize)?
        .checked_mul(4)?;
    if bytes.len() != 16usize.checked_add(count)? {
        return None;
    }
    let samples = bytes[16..]
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    Some(ClipPayload {
        sample_rate,
        channels,
        frames,
        samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_round_trips_and_rejects_bad_indices() {
        let v = [
            Vertex {
                position: [1.0, 2.0, 3.0],
                ..Vertex::default()
            },
            Vertex::default(),
        ];
        let bytes = encode_mesh(&v, &[0, 1, 0], [0.0; 3], [1.0; 3]);
        let m = decode_mesh(&bytes).unwrap();
        assert_eq!(m.vertices, v);
        assert_eq!(m.indices, [0, 1, 0]);
        assert!(decode_mesh(&bytes[..bytes.len() - 1]).is_none());
        assert!(decode_mesh(&encode_mesh(&v, &[0, 2, 0], [0.0; 3], [1.0; 3])).is_none());
    }

    #[test]
    fn texture_and_clip_round_trip() {
        let t = encode_texture(
            2,
            1,
            TEXTURE_FORMAT_RGBA8_SRGB,
            1,
            &[1, 2, 3, 4, 5, 6, 7, 8],
        );
        let d = decode_texture(&t).unwrap();
        assert_eq!(
            (d.width, d.height, d.format, d.mip_levels),
            (2, 1, TEXTURE_FORMAT_RGBA8_SRGB, 1)
        );
        assert_eq!(d.base(), &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert!(d.level(1).is_none());
        assert!(decode_texture(&encode_texture(2, 1, 9, 1, &[0; 8])).is_none());
        // A chain: 4×2 → 2×1 → 1×1 (full_mip_count 3); the bytes must add up.
        assert_eq!(full_mip_count(4, 2), 3);
        assert_eq!(full_mip_count(1, 1), 1);
        assert_eq!(mip_size(5, 3, 1), (2, 1));
        let mut chain = vec![9u8; 32];
        chain.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        chain.extend_from_slice(&[10, 11, 12, 13]);
        let d = decode_texture(&encode_texture(
            4,
            2,
            TEXTURE_FORMAT_RGBA8_LINEAR,
            3,
            &chain,
        ))
        .unwrap();
        assert_eq!(d.level_size(1), (2, 1));
        assert_eq!(d.level(1).unwrap(), &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(d.level(2).unwrap(), &[10, 11, 12, 13]);
        assert!(decode_texture(&encode_texture(
            4,
            2,
            TEXTURE_FORMAT_RGBA8_LINEAR,
            4,
            &chain
        ))
        .is_none());
        assert!(decode_texture(&encode_texture(
            4,
            2,
            TEXTURE_FORMAT_RGBA8_LINEAR,
            2,
            &chain
        ))
        .is_none());
        assert!(decode_texture(&encode_texture(
            4,
            2,
            TEXTURE_FORMAT_RGBA8_LINEAR,
            0,
            &chain[..32]
        ))
        .is_none());
        let c = encode_clip(48_000, 2, &[0.0, 0.5, -0.5, 1.0]);
        let d = decode_clip(&c).unwrap();
        assert_eq!((d.sample_rate, d.channels, d.frames), (48_000, 2, 2));
        assert!(decode_clip(&c[..c.len() - 4]).is_none());
    }
}
