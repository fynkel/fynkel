//! The typed chunk payloads (`MESH`, `TEXR`, `CLIP`, `SKIN`, `ANIM`): the
//! layouts the cooker writes and the runtime reads. They live here, next
//! to the container, so a loader needs nothing from the importers.
//! Everything is little-endian; a malformed payload decodes to `None`,
//! never panics.
//!
//! ```text
//! MESH  u32 vertex count, u32 index count, [f32; 3] bounds min,
//!       [f32; 3] bounds max, vertices (72 bytes each), u32 indices,
//!       then — a skinned mesh only (M3-19) — one `SkinWeight` per vertex
//!       (u16 × 4 joint indices, f32 × 4 weights: 24 bytes)
//! TEXR  u32 width, u32 height, u32 format, u32 mip levels (1),
//!       width × height × 4 texels, top row first
//! CLIP  u32 sample rate, u32 channels, u64 frames, interleaved f32 samples
//! SKIN  u32 joint count, str skeleton, then per joint: str name,
//!       u32 parent (joint index, `u32::MAX` for none), [f32; 16] inverse
//!       bind matrix (column-major), rest pose [f32; 3] translation,
//!       [f32; 4] rotation (x y z w), [f32; 3] scale
//! ANIM  u32 channel count, f32 duration (seconds), then per channel: str
//!       target (a node name), u32 property (0 translation, 1 rotation,
//!       2 scale), u32 interpolation (0 step, 1 linear, 2 cubic spline),
//!       u32 key count, f32 times, f32 values — `width` floats per key
//!       (3, or 4 for a rotation), three per key for a cubic spline
//!       (in-tangent, value, out-tangent)
//! str   u32 byte length, the UTF-8 bytes, zero padding to 4
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

/// One vertex's skinning: up to four joints (indices into the skin's
/// joint list) and their weights, which the cooker normalizes to sum to 1
/// (unused slots weigh 0).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct SkinWeight {
    pub joints: [u16; 4],
    pub weights: [f32; 4],
}

impl SkinWeight {
    pub const STRIDE: usize = 24;

    pub fn write(&self, out: &mut Vec<u8>) {
        for j in self.joints {
            out.extend_from_slice(&j.to_le_bytes());
        }
        for w in self.weights {
            out.extend_from_slice(&w.to_le_bytes());
        }
    }

    pub fn read(bytes: &[u8]) -> Option<SkinWeight> {
        if bytes.len() < Self::STRIDE {
            return None;
        }
        let mut joints = [0u16; 4];
        for (i, j) in joints.iter_mut().enumerate() {
            *j = u16::from_le_bytes(bytes[i * 2..i * 2 + 2].try_into().unwrap());
        }
        let mut weights = [0f32; 4];
        for (i, w) in weights.iter_mut().enumerate() {
            *w = f32::from_le_bytes(bytes[8 + i * 4..12 + i * 4].try_into().unwrap());
        }
        Some(SkinWeight { joints, weights })
    }
}

/// A `MESH` payload. `skin` is empty for a static mesh and one entry per
/// vertex for a skinned one.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshPayload {
    pub bounds_min: [f32; 3],
    pub bounds_max: [f32; 3],
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub skin: Vec<SkinWeight>,
}

impl MeshPayload {
    pub fn is_skinned(&self) -> bool {
        !self.skin.is_empty()
    }
}

pub fn encode_mesh(
    vertices: &[Vertex],
    indices: &[u32],
    bounds_min: [f32; 3],
    bounds_max: [f32; 3],
) -> Vec<u8> {
    encode_skinned_mesh(vertices, indices, bounds_min, bounds_max, &[])
}

/// `skin` is empty (a static mesh) or exactly one entry per vertex.
pub fn encode_skinned_mesh(
    vertices: &[Vertex],
    indices: &[u32],
    bounds_min: [f32; 3],
    bounds_max: [f32; 3],
    skin: &[SkinWeight],
) -> Vec<u8> {
    debug_assert!(skin.is_empty() || skin.len() == vertices.len());
    let mut out = Vec::with_capacity(
        32 + vertices.len() * Vertex::STRIDE + indices.len() * 4 + skin.len() * SkinWeight::STRIDE,
    );
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
    for w in skin {
        w.write(&mut out);
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
    let static_len = 32usize
        .checked_add(vertices_len)?
        .checked_add(indices_len)?;
    let skin_len = vertex_count.checked_mul(SkinWeight::STRIDE)?;
    let skinned = if bytes.len() == static_len {
        false
    } else if bytes.len() == static_len.checked_add(skin_len)? && vertex_count > 0 {
        true
    } else {
        return None;
    };
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
    let mut skin = Vec::new();
    if skinned {
        skin.reserve(vertex_count);
        for i in 0..vertex_count {
            let at = static_len + i * SkinWeight::STRIDE;
            skin.push(SkinWeight::read(&bytes[at..at + SkinWeight::STRIDE])?);
        }
    }
    Some(MeshPayload {
        bounds_min,
        bounds_max,
        vertices,
        indices,
        skin,
    })
}

fn write_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u32).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

/// Reads a `str` at `*at`, advancing past its padding.
fn read_str(bytes: &[u8], at: &mut usize) -> Option<String> {
    let len = read_u32(bytes, at)? as usize;
    let end = at.checked_add(len)?;
    let s = std::str::from_utf8(bytes.get(*at..end)?).ok()?.to_string();
    *at = end.checked_add(3)? & !3;
    Some(s)
}

fn read_u32(bytes: &[u8], at: &mut usize) -> Option<u32> {
    let v = u32::from_le_bytes(bytes.get(*at..*at + 4)?.try_into().unwrap());
    *at += 4;
    Some(v)
}

fn read_f32(bytes: &[u8], at: &mut usize) -> Option<f32> {
    let v = f32::from_le_bytes(bytes.get(*at..*at + 4)?.try_into().unwrap());
    *at += 4;
    Some(v)
}

fn read_f32s<const N: usize>(bytes: &[u8], at: &mut usize) -> Option<[f32; N]> {
    let mut out = [0f32; N];
    for v in out.iter_mut() {
        *v = read_f32(bytes, at)?;
    }
    Some(out)
}

/// One joint of a `SKIN` payload: named after its glTF node (the runtime
/// finds the entity by name under the skin's skeleton), with its parent
/// in the joint list and its rest pose.
#[derive(Clone, Debug, PartialEq)]
pub struct SkinJoint {
    pub name: String,
    /// Index into the joint list, `None` for a joint whose parent is not
    /// a joint of this skin.
    pub parent: Option<u32>,
    /// Column-major, mesh space → joint space at the bind pose.
    pub inverse_bind: [f32; 16],
    pub rest_translation: [f32; 3],
    /// x, y, z, w.
    pub rest_rotation: [f32; 4],
    pub rest_scale: [f32; 3],
}

/// A `SKIN` payload: the joint list a skinned mesh's `SkinWeight`s index,
/// and the name of the node whose subtree holds every joint (`skeleton`;
/// empty when the skin names none — the runtime then searches from the
/// tree's root).
#[derive(Clone, Debug, PartialEq)]
pub struct SkinPayload {
    pub skeleton: String,
    pub joints: Vec<SkinJoint>,
}

pub fn encode_skin(skin: &SkinPayload) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(skin.joints.len() as u32).to_le_bytes());
    write_str(&mut out, &skin.skeleton);
    for j in &skin.joints {
        write_str(&mut out, &j.name);
        out.extend_from_slice(&j.parent.unwrap_or(u32::MAX).to_le_bytes());
        for f in j
            .inverse_bind
            .iter()
            .chain(&j.rest_translation)
            .chain(&j.rest_rotation)
            .chain(&j.rest_scale)
        {
            out.extend_from_slice(&f.to_le_bytes());
        }
    }
    out
}

pub fn decode_skin(bytes: &[u8]) -> Option<SkinPayload> {
    let mut at = 0usize;
    let count = read_u32(bytes, &mut at)? as usize;
    let skeleton = read_str(bytes, &mut at)?;
    // A joint is at least 4 + 4 + 26 × 4 bytes; refuse counts the bytes
    // cannot hold before reserving.
    if count.checked_mul(112)? > bytes.len() {
        return None;
    }
    let mut joints = Vec::with_capacity(count);
    for _ in 0..count {
        let name = read_str(bytes, &mut at)?;
        let parent = read_u32(bytes, &mut at)?;
        let parent = if parent == u32::MAX {
            None
        } else if (parent as usize) < count {
            Some(parent)
        } else {
            return None;
        };
        joints.push(SkinJoint {
            name,
            parent,
            inverse_bind: read_f32s::<16>(bytes, &mut at)?,
            rest_translation: read_f32s::<3>(bytes, &mut at)?,
            rest_rotation: read_f32s::<4>(bytes, &mut at)?,
            rest_scale: read_f32s::<3>(bytes, &mut at)?,
        });
    }
    if at != bytes.len() {
        return None;
    }
    Some(SkinPayload { skeleton, joints })
}

pub const ANIM_PROPERTY_TRANSLATION: u32 = 0;
pub const ANIM_PROPERTY_ROTATION: u32 = 1;
pub const ANIM_PROPERTY_SCALE: u32 = 2;
pub const ANIM_INTERPOLATION_STEP: u32 = 0;
pub const ANIM_INTERPOLATION_LINEAR: u32 = 1;
pub const ANIM_INTERPOLATION_CUBIC_SPLINE: u32 = 2;

/// One channel of an `ANIM` payload: keyframes of one property of one
/// named node.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationChannel {
    /// The node's name (the runtime finds the entity by name under the
    /// animator).
    pub target: String,
    /// `ANIM_PROPERTY_*`.
    pub property: u32,
    /// `ANIM_INTERPOLATION_*`.
    pub interpolation: u32,
    /// Seconds, non-decreasing.
    pub times: Vec<f32>,
    /// `times.len() × width` floats (`× 3` for a cubic spline: in-tangent,
    /// value, out-tangent per key); `width` is 4 for a rotation, 3 else.
    pub values: Vec<f32>,
}

impl AnimationChannel {
    pub fn width(&self) -> usize {
        if self.property == ANIM_PROPERTY_ROTATION {
            4
        } else {
            3
        }
    }

    /// Floats per key.
    pub fn key_floats(&self) -> usize {
        if self.interpolation == ANIM_INTERPOLATION_CUBIC_SPLINE {
            self.width() * 3
        } else {
            self.width()
        }
    }
}

/// A named time in a clip (M3-19c): the world raises `ANIMATION_MARKER`
/// when an animator's clock crosses it.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationMarker {
    /// Seconds, within `[0, duration]`.
    pub time: f32,
    pub name: String,
}

/// An `ANIM` payload: one clip.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationPayload {
    /// Seconds; the largest key time over every channel.
    pub duration: f32,
    pub channels: Vec<AnimationChannel>,
    /// In time order; after the channels in the bytes (a payload without
    /// them, cooked before M3-19c, decodes as none).
    pub markers: Vec<AnimationMarker>,
}

pub fn encode_animation(anim: &AnimationPayload) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(anim.channels.len() as u32).to_le_bytes());
    out.extend_from_slice(&anim.duration.to_le_bytes());
    for c in &anim.channels {
        write_str(&mut out, &c.target);
        out.extend_from_slice(&c.property.to_le_bytes());
        out.extend_from_slice(&c.interpolation.to_le_bytes());
        out.extend_from_slice(&(c.times.len() as u32).to_le_bytes());
        for t in &c.times {
            out.extend_from_slice(&t.to_le_bytes());
        }
        for v in &c.values {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    if !anim.markers.is_empty() {
        out.extend_from_slice(&(anim.markers.len() as u32).to_le_bytes());
        for m in &anim.markers {
            out.extend_from_slice(&m.time.to_le_bytes());
            write_str(&mut out, &m.name);
        }
    }
    out
}

pub fn decode_animation(bytes: &[u8]) -> Option<AnimationPayload> {
    let mut at = 0usize;
    let count = read_u32(bytes, &mut at)? as usize;
    let duration = read_f32(bytes, &mut at)?;
    if !duration.is_finite() || duration < 0.0 || count.checked_mul(20)? > bytes.len() {
        return None;
    }
    let mut channels = Vec::with_capacity(count);
    for _ in 0..count {
        let target = read_str(bytes, &mut at)?;
        let property = read_u32(bytes, &mut at)?;
        let interpolation = read_u32(bytes, &mut at)?;
        if property > ANIM_PROPERTY_SCALE || interpolation > ANIM_INTERPOLATION_CUBIC_SPLINE {
            return None;
        }
        let keys = read_u32(bytes, &mut at)? as usize;
        let mut channel = AnimationChannel {
            target,
            property,
            interpolation,
            times: Vec::new(),
            values: Vec::new(),
        };
        let floats = keys.checked_mul(1 + channel.key_floats())?.checked_mul(4)?;
        if floats > bytes.len().saturating_sub(at) {
            return None;
        }
        channel.times.reserve(keys);
        for _ in 0..keys {
            let t = read_f32(bytes, &mut at)?;
            if !t.is_finite() || channel.times.last().is_some_and(|last| t < *last) {
                return None;
            }
            channel.times.push(t);
        }
        let n = keys * channel.key_floats();
        channel.values.reserve(n);
        for _ in 0..n {
            channel.values.push(read_f32(bytes, &mut at)?);
        }
        channels.push(channel);
    }
    let mut markers = Vec::new();
    if at < bytes.len() {
        let count = read_u32(bytes, &mut at)? as usize;
        if count.checked_mul(12)? > bytes.len() {
            return None;
        }
        for _ in 0..count {
            let time = read_f32(bytes, &mut at)?;
            let name = read_str(bytes, &mut at)?;
            if !time.is_finite()
                || time < 0.0
                || time > duration
                || name.is_empty()
                || markers
                    .last()
                    .is_some_and(|m: &AnimationMarker| m.time > time)
            {
                return None;
            }
            markers.push(AnimationMarker { time, name });
        }
    }
    if at != bytes.len() {
        return None;
    }
    Some(AnimationPayload {
        duration,
        channels,
        markers,
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

/// A `STRM` payload (M3-11 part 2): a sound kept encoded — the source
/// file's bytes as imported (WAV or Ogg Vorbis) with the format the
/// importer decoded from them, so the runtime knows the length and the
/// rate without opening the stream. `codec` is `pcm` or `vorbis`.
#[derive(Clone, Debug, PartialEq)]
pub struct StreamPayload {
    pub sample_rate: u32,
    pub channels: u32,
    pub frames: u64,
    pub codec: String,
    pub bytes: Vec<u8>,
}

pub fn encode_stream(
    sample_rate: u32,
    channels: u32,
    frames: u64,
    codec: &str,
    bytes: &[u8],
) -> Vec<u8> {
    let codec = codec.as_bytes();
    let mut out = Vec::with_capacity(24 + codec.len() + bytes.len());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&frames.to_le_bytes());
    out.extend_from_slice(&(codec.len() as u32).to_le_bytes());
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(codec);
    out.extend_from_slice(bytes);
    out
}

pub fn decode_stream(bytes: &[u8]) -> Option<StreamPayload> {
    if bytes.len() < 24 {
        return None;
    }
    let sample_rate = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    let channels = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    let frames = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
    let codec_len = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
    let data_len = u32::from_le_bytes(bytes[20..24].try_into().unwrap()) as usize;
    if bytes.len() != 24usize.checked_add(codec_len)?.checked_add(data_len)? {
        return None;
    }
    let codec = std::str::from_utf8(&bytes[24..24 + codec_len]).ok()?;
    Some(StreamPayload {
        sample_rate,
        channels,
        frames,
        codec: codec.to_string(),
        bytes: bytes[24 + codec_len..].to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_round_trips_and_rejects_a_short_blob() {
        let bytes = encode_stream(44_100, 2, 1234, "vorbis", &[1, 2, 3, 4, 5]);
        let p = decode_stream(&bytes).unwrap();
        assert_eq!(
            p,
            StreamPayload {
                sample_rate: 44_100,
                channels: 2,
                frames: 1234,
                codec: "vorbis".into(),
                bytes: vec![1, 2, 3, 4, 5],
            }
        );
        assert!(decode_stream(&bytes[..bytes.len() - 1]).is_none());
        assert!(decode_stream(&bytes[..10]).is_none());
    }

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
        assert!(!m.is_skinned());
        // A skinned mesh carries one weight per vertex after the indices;
        // a partial trailer is malformed.
        let skin = [
            SkinWeight {
                joints: [1, 0, 0, 0],
                weights: [1.0, 0.0, 0.0, 0.0],
            },
            SkinWeight {
                joints: [0, 1, 2, 3],
                weights: [0.25; 4],
            },
        ];
        let bytes = encode_skinned_mesh(&v, &[0, 1, 0], [0.0; 3], [1.0; 3], &skin);
        let m = decode_mesh(&bytes).unwrap();
        assert!(m.is_skinned());
        assert_eq!(m.skin, skin);
        assert!(decode_mesh(&bytes[..bytes.len() - SkinWeight::STRIDE]).is_none());
    }

    #[test]
    fn skin_and_animation_round_trip() {
        let skin = SkinPayload {
            skeleton: "Armature".into(),
            joints: vec![
                SkinJoint {
                    name: "Root".into(),
                    parent: None,
                    inverse_bind: [1.0; 16],
                    rest_translation: [0.0, 1.0, 0.0],
                    rest_rotation: [0.0, 0.0, 0.0, 1.0],
                    rest_scale: [1.0; 3],
                },
                SkinJoint {
                    name: "Tip".into(),
                    parent: Some(0),
                    inverse_bind: [0.5; 16],
                    rest_translation: [0.0, 2.0, 0.0],
                    rest_rotation: [0.0, 0.0, 0.0, 1.0],
                    rest_scale: [1.0; 3],
                },
            ],
        };
        let bytes = encode_skin(&skin);
        assert_eq!(decode_skin(&bytes).unwrap(), skin);
        assert!(decode_skin(&bytes[..bytes.len() - 1]).is_none());
        let mut bad = skin.clone();
        bad.joints[1].parent = Some(7);
        assert!(decode_skin(&encode_skin(&bad)).is_none());

        let anim = AnimationPayload {
            duration: 1.0,
            channels: vec![
                AnimationChannel {
                    target: "Tip".into(),
                    property: ANIM_PROPERTY_ROTATION,
                    interpolation: ANIM_INTERPOLATION_LINEAR,
                    times: vec![0.0, 1.0],
                    values: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.7, 0.0, 0.7],
                },
                AnimationChannel {
                    target: "Root".into(),
                    property: ANIM_PROPERTY_TRANSLATION,
                    interpolation: ANIM_INTERPOLATION_CUBIC_SPLINE,
                    times: vec![0.5],
                    values: vec![0.0; 9],
                },
            ],
            markers: Vec::new(),
        };
        let bytes = encode_animation(&anim);
        assert_eq!(decode_animation(&bytes).unwrap(), anim);
        assert!(decode_animation(&bytes[..bytes.len() - 4]).is_none());
        let mut bad = anim.clone();
        bad.channels[0].times = vec![1.0, 0.0];
        assert!(decode_animation(&encode_animation(&bad)).is_none());
        // Markers (M3-19c) follow the channels; a marker outside the clip,
        // unnamed or out of order is malformed.
        let mut marked = anim.clone();
        marked.markers = vec![
            AnimationMarker {
                time: 0.25,
                name: "footstep".into(),
            },
            AnimationMarker {
                time: 1.0,
                name: "footstep".into(),
            },
        ];
        let bytes = encode_animation(&marked);
        assert_eq!(decode_animation(&bytes).unwrap(), marked);
        assert!(decode_animation(&bytes[..bytes.len() - 1]).is_none());
        for bad in [(1.5, "x"), (0.5, ""), (0.1, "late")] {
            let mut m = marked.clone();
            m.markers.push(AnimationMarker {
                time: bad.0,
                name: bad.1.into(),
            });
            assert!(decode_animation(&encode_animation(&m)).is_none(), "{bad:?}");
        }
        let mut bad = anim.clone();
        bad.channels[0].property = 9;
        assert!(decode_animation(&encode_animation(&bad)).is_none());
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
