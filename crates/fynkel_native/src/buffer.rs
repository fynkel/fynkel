//! Buffer framing for ABI v1 (`include/fynkel.h`): a 24-byte header, a
//! record area, then the string tail. Record bodies use the generated
//! codecs in `fynkel_wire`.

use fynkel_contracts::ABI_VERSION;
use fynkel_wire::{get_u16, get_u32, put_u16, put_u32, StringTable, WireRecord};

pub const MAGIC: u32 = 0x4b4e_5946; // "FYNK"
pub const HEADER_SIZE: usize = 24;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BufferKind {
    Commands = 1,
    Snapshot = 2,
    Events = 3,
    Diagnostics = 4,
    Query = 5,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub kind: u16,
    pub record_count: u32,
    pub byte_length: u32,
    pub records_end: u32,
    pub request_id: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ProtocolError {
    TooShort,
    BadMagic,
    BadVersion(u16),
    BadKind(u16),
    LengthMismatch {
        declared: u32,
        actual: usize,
    },
    BadRecordsEnd(u32),
    /// A record header or body ran past the record area.
    Truncated {
        at: usize,
    },
}

/// Validates the header and returns it plus (records, tail) slices.
pub fn validate(buf: &[u8], expected: BufferKind) -> Result<(Header, &[u8], &[u8]), ProtocolError> {
    if buf.len() < HEADER_SIZE {
        return Err(ProtocolError::TooShort);
    }
    if get_u32(buf, 0) != MAGIC {
        return Err(ProtocolError::BadMagic);
    }
    let version = get_u16(buf, 4);
    if version != ABI_VERSION {
        return Err(ProtocolError::BadVersion(version));
    }
    let kind = get_u16(buf, 6);
    if kind != expected as u16 {
        return Err(ProtocolError::BadKind(kind));
    }
    let h = Header {
        kind,
        record_count: get_u32(buf, 8),
        byte_length: get_u32(buf, 12),
        records_end: get_u32(buf, 16),
        request_id: get_u32(buf, 20),
    };
    if h.byte_length as usize != buf.len() {
        return Err(ProtocolError::LengthMismatch {
            declared: h.byte_length,
            actual: buf.len(),
        });
    }
    let end = h.records_end as usize;
    if end < HEADER_SIZE || end > buf.len() {
        return Err(ProtocolError::BadRecordsEnd(h.records_end));
    }
    Ok((h, &buf[HEADER_SIZE..end], &buf[end..]))
}

/// Builds an outgoing buffer: header, records, then the string tail.
pub struct Writer {
    bytes: Vec<u8>,
    strings: StringTable,
    count: u32,
    kind: BufferKind,
    request_id: u32,
}

impl Writer {
    pub fn begin(mut bytes: Vec<u8>, kind: BufferKind) -> Self {
        bytes.clear();
        bytes.resize(HEADER_SIZE, 0);
        Self {
            bytes,
            strings: StringTable::new(),
            count: 0,
            kind,
            request_id: 0,
        }
    }

    pub fn request_id(mut self, id: u32) -> Self {
        self.request_id = id;
        self
    }

    /// Appends one schema record.
    pub fn record<T: WireRecord>(&mut self, v: &T) {
        let start = self.bytes.len();
        self.bytes.resize(start + T::SIZE, 0);
        v.encode(&mut self.bytes[start..], &mut self.strings);
        self.count += 1;
    }

    /// Appends a command: `CommandHeader` + body, counted as one record.
    pub fn command<T: WireRecord>(&mut self, kind: u32, body: &T) {
        let start = self.bytes.len();
        self.bytes.resize(
            start + fynkel_core::schema::CommandHeader::SIZE + T::SIZE,
            0,
        );
        fynkel_core::schema::CommandHeader {
            kind,
            size: T::SIZE as u32,
        }
        .encode(&mut self.bytes[start..], &mut self.strings);
        body.encode(
            &mut self.bytes[start + fynkel_core::schema::CommandHeader::SIZE..],
            &mut self.strings,
        );
        self.count += 1;
    }

    /// Appends raw record bytes already encoded (e.g. a `u64` handle list).
    pub fn raw(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
        self.count += 1;
    }

    pub fn count(&self) -> u32 {
        self.count
    }

    pub fn finish(mut self) -> Vec<u8> {
        let records_end = self.bytes.len() as u32;
        self.bytes.extend_from_slice(self.strings.as_bytes());
        let len = self.bytes.len() as u32;
        let b = &mut self.bytes;
        put_u32(b, 0, MAGIC);
        put_u16(b, 4, ABI_VERSION);
        put_u16(b, 6, self.kind as u16);
        put_u32(b, 8, self.count);
        put_u32(b, 12, len);
        put_u32(b, 16, records_end);
        put_u32(b, 20, self.request_id);
        self.bytes
    }
}

/// Empty but valid buffer of a kind.
pub fn empty(kind: BufferKind) -> Vec<u8> {
    Writer::begin(Vec::new(), kind).finish()
}
