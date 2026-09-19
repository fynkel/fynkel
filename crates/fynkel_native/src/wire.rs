//! Wire records for ABI v0. These mirror `include/fynkel.h` exactly; the
//! header is the contract and this file must follow it, never the reverse.
//! Buffers are read with unaligned little-endian loads, so a Dart
//! `Uint8List` with any alignment is acceptable.

pub const ABI_VERSION: u16 = 0;
pub const BUFFER_MAGIC: u32 = 0x4b4e_5946; // "FYNK"

/// Fingerprint of the record schemas below. Bump whenever any record layout
/// or enum value changes. In 1.0 this is derived by `fynkel codegen` from the
/// Rust component schemas (ADR-0012); for v0 it is hand-maintained.
pub const SCHEMA_FINGERPRINT: u64 = 0x0000_0001_0000_0003;

pub const HEADER_SIZE: usize = 16;
pub const COMMAND_SIZE: usize = 64;
pub const SNAPSHOT_TRANSFORM_SIZE: usize = 48;
pub const EVENT_SIZE: usize = 32;
pub const DIAGNOSTIC_SIZE: usize = 32;
pub const TRANSFORM_SIZE: usize = 40;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BufferKind {
    Commands = 1,
    Snapshot = 2,
    Events = 3,
    Diagnostics = 4,
}

#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CommandKind {
    Spawn = 1,
    Destroy = 2,
    SetTransform = 3,
}

impl CommandKind {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            1 => Some(Self::Spawn),
            2 => Some(Self::Destroy),
            3 => Some(Self::SetTransform),
            _ => None,
        }
    }
}

pub const EVENT_SPAWNED: u32 = 1;
pub const EVENT_DESTROYED: u32 = 2;

pub const COMPONENT_TRANSFORM: u32 = 1;

pub const DIAG_UNKNOWN_COMMAND: u32 = 1;
pub const DIAG_STALE_ENTITY: u32 = 2;
pub const DIAG_INVALID_TEMP_ID: u32 = 3;
pub const DIAG_INVALID_PAYLOAD: u32 = 4;

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Transform {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

impl Transform {
    pub const IDENTITY: Self = Self {
        position: [0.0; 3],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0; 3],
    };

    pub fn read(b: &[u8]) -> Self {
        let mut f = [0f32; 10];
        for (i, v) in f.iter_mut().enumerate() {
            *v = read_f32(b, i * 4);
        }
        Self {
            position: [f[0], f[1], f[2]],
            rotation: [f[3], f[4], f[5], f[6]],
            scale: [f[7], f[8], f[9]],
        }
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        for v in self
            .position
            .iter()
            .chain(self.rotation.iter())
            .chain(self.scale.iter())
        {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }

    pub fn is_finite(&self) -> bool {
        self.position.iter().all(|v| v.is_finite())
            && self.rotation.iter().all(|v| v.is_finite())
            && self.scale.iter().all(|v| v.is_finite())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Command {
    pub kind: u32,
    pub flags: u32,
    pub target: u64,
    pub payload: [u8; 48],
}

impl Command {
    pub fn read(b: &[u8]) -> Self {
        let mut payload = [0u8; 48];
        payload.copy_from_slice(&b[16..64]);
        Self {
            kind: read_u32(b, 0),
            flags: read_u32(b, 4),
            target: read_u64(b, 8),
            payload,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ProtocolError {
    TooShort,
    BadMagic,
    BadVersion(u16),
    BadKind(u16),
    LengthMismatch { declared: u32, actual: usize },
    CountMismatch { declared: u32, fits: usize },
}

/// Validates a buffer header and returns the record bytes.
pub fn validate(
    buf: &[u8],
    expected_kind: BufferKind,
    record_size: usize,
) -> Result<(u32, &[u8]), ProtocolError> {
    if buf.len() < HEADER_SIZE {
        return Err(ProtocolError::TooShort);
    }
    if read_u32(buf, 0) != BUFFER_MAGIC {
        return Err(ProtocolError::BadMagic);
    }
    let version = read_u16(buf, 4);
    if version != ABI_VERSION {
        return Err(ProtocolError::BadVersion(version));
    }
    let kind = read_u16(buf, 6);
    if kind != expected_kind as u16 {
        return Err(ProtocolError::BadKind(kind));
    }
    let count = read_u32(buf, 8);
    let declared_len = read_u32(buf, 12);
    if declared_len as usize != buf.len() {
        return Err(ProtocolError::LengthMismatch {
            declared: declared_len,
            actual: buf.len(),
        });
    }
    let body = &buf[HEADER_SIZE..];
    if count as usize * record_size != body.len() {
        return Err(ProtocolError::CountMismatch {
            declared: count,
            fits: body.len() / record_size,
        });
    }
    Ok((count, body))
}

/// Builder for outgoing buffers; `finish` patches the header.
pub struct BufferWriter {
    pub bytes: Vec<u8>,
    count: u32,
}

impl BufferWriter {
    pub fn begin(bytes: Vec<u8>, kind: BufferKind) -> Self {
        let mut bytes = bytes;
        bytes.clear();
        bytes.extend_from_slice(&BUFFER_MAGIC.to_le_bytes());
        bytes.extend_from_slice(&ABI_VERSION.to_le_bytes());
        bytes.extend_from_slice(&(kind as u16).to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        Self { bytes, count: 0 }
    }

    pub fn record(&mut self) -> &mut Vec<u8> {
        self.count += 1;
        &mut self.bytes
    }

    pub fn finish(mut self) -> Vec<u8> {
        let len = self.bytes.len() as u32;
        self.bytes[8..12].copy_from_slice(&self.count.to_le_bytes());
        self.bytes[12..16].copy_from_slice(&len.to_le_bytes());
        self.bytes
    }

    pub fn count(&self) -> u32 {
        self.count
    }
}

pub fn read_u16(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes(b[off..off + 2].try_into().unwrap())
}
pub fn read_u32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}
pub fn read_u64(b: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(b[off..off + 8].try_into().unwrap())
}
pub fn read_f32(b: &[u8], off: usize) -> f32 {
    f32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}
