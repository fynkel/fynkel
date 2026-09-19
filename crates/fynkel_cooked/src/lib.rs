//! The cooked asset container, version 1 (ADR-0006; blueprint §6 "Cook
//! production assets into a documented versioned container, one file per
//! asset, with explicit lengths, checksums, dependency IDs, and bounded
//! chunk decoding").
//!
//! One `.fyna` file per asset. Everything is little-endian; nothing is a
//! raw Rust memory layout.
//!
//! ```text
//! offset  size  field
//!      0     4  magic "FYNC"
//!      4     4  u32 version (1)
//!      8     4  u32 header_len (48)
//!     12     4  u32 chunk_count
//!     16     8  u64 table_offset      absolute; the table follows the chunks
//!     24     8  u64 file_len          whole file; a shorter file is truncated
//!     32     4  u32 flags (0)
//!     36     4  u32 header_crc        crc32 of bytes 0..36
//!     40     8  reserved (0)
//!  chunk payloads, each 8-byte aligned
//!  chunk table: chunk_count × 32 bytes
//!      0     4  kind ("META", "JSON", "MESH", "TEXR", "CLIP", "BLOB", …)
//!      4     4  u32 flags (0)
//!      8     8  u64 offset            absolute
//!     16     8  u64 length
//!     24     4  u32 crc32 of the payload
//!     28     4  reserved (0)
//! ```
//!
//! Chunk 0 is always `META`: a JSON object the cooker writes (asset id,
//! name, importer, dependency ids, one entry per chunk with its name and
//! kind-specific fields). The container itself does not interpret it.
//!
//! Reading is bounded: opening validates the header and the table (count,
//! offsets and lengths inside the file, no overlap with the table) without
//! touching payloads; a payload is read on request, against a caller
//! supplied maximum, and its checksum is verified before it is returned.

#![forbid(unsafe_code)]

pub mod payload;

use std::fmt;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub const MAGIC: &[u8; 4] = b"FYNC";
pub const VERSION: u32 = 1;
pub const HEADER_LEN: usize = 48;
pub const ENTRY_LEN: usize = 32;
/// More chunks than this in one asset is a corrupt file, not an asset.
pub const MAX_CHUNKS: u32 = 65_536;

pub const KIND_META: [u8; 4] = *b"META";
pub const KIND_JSON: [u8; 4] = *b"JSON";
pub const KIND_MESH: [u8; 4] = *b"MESH";
pub const KIND_TEXTURE: [u8; 4] = *b"TEXR";
pub const KIND_CLIP: [u8; 4] = *b"CLIP";
/// A cooked `fynkel.PbrMaterial` (`fynkel_wire::material`), next to the
/// material document's `JSON` chunk.
pub const KIND_MATERIAL: [u8; 4] = *b"MATL";
pub const KIND_BLOB: [u8; 4] = *b"BLOB";

#[derive(Debug)]
pub enum Error {
    /// Not a cooked container at all.
    NotCooked(&'static str),
    /// A container of a version this build does not read.
    Version(u32),
    /// The file is shorter than its header says.
    Truncated {
        expected: u64,
        actual: u64,
    },
    /// The header or table is inconsistent.
    Corrupt(&'static str),
    ChunkOutOfRange(usize),
    /// The chunk is larger than the caller allows.
    ChunkTooLarge {
        index: usize,
        length: u64,
        max: u64,
    },
    /// The payload does not match its checksum.
    Checksum {
        index: usize,
    },
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotCooked(why) => write!(f, "not a cooked asset: {why}"),
            Error::Version(v) => write!(f, "cooked asset version {v}; this build reads {VERSION}"),
            Error::Truncated { expected, actual } => {
                write!(f, "truncated: {actual} bytes, the header says {expected}")
            }
            Error::Corrupt(why) => write!(f, "corrupt: {why}"),
            Error::ChunkOutOfRange(i) => write!(f, "no chunk {i}"),
            Error::ChunkTooLarge { index, length, max } => {
                write!(f, "chunk {index} is {length} bytes; the limit is {max}")
            }
            Error::Checksum { index } => write!(f, "chunk {index} fails its checksum"),
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

/// One row of the chunk table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chunk {
    pub kind: [u8; 4],
    pub flags: u32,
    pub offset: u64,
    pub length: u64,
    pub crc32: u32,
}

impl Chunk {
    pub fn kind_str(&self) -> String {
        String::from_utf8_lossy(&self.kind).into_owned()
    }
}

/// The validated header and table of a container; payloads not yet read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    pub version: u32,
    pub file_len: u64,
    pub table_offset: u64,
    pub chunks: Vec<Chunk>,
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

/// Parses and checks the fixed header. Returns (chunk_count, table_offset, file_len).
fn parse_header(header: &[u8]) -> Result<(u32, u64, u64), Error> {
    if header.len() < HEADER_LEN {
        return Err(Error::NotCooked("shorter than a header"));
    }
    if &header[0..4] != MAGIC {
        return Err(Error::NotCooked("missing FYNC magic"));
    }
    let version = u32_at(header, 4);
    if version != VERSION {
        return Err(Error::Version(version));
    }
    if u32_at(header, 8) as usize != HEADER_LEN {
        return Err(Error::Corrupt("header length"));
    }
    if crc32fast::hash(&header[0..36]) != u32_at(header, 36) {
        return Err(Error::Corrupt("header checksum"));
    }
    let chunk_count = u32_at(header, 12);
    if chunk_count == 0 || chunk_count > MAX_CHUNKS {
        return Err(Error::Corrupt("chunk count"));
    }
    let table_offset = u64_at(header, 16);
    let file_len = u64_at(header, 24);
    let table_len = chunk_count as u64 * ENTRY_LEN as u64;
    if table_offset < HEADER_LEN as u64 || table_offset.checked_add(table_len) != Some(file_len) {
        return Err(Error::Corrupt("table placement"));
    }
    Ok((chunk_count, table_offset, file_len))
}

/// Parses and checks the table bytes against the header's numbers.
fn parse_table(
    table: &[u8],
    chunk_count: u32,
    table_offset: u64,
    file_len: u64,
) -> Result<Table, Error> {
    if table.len() != chunk_count as usize * ENTRY_LEN {
        return Err(Error::Corrupt("table length"));
    }
    let mut chunks = Vec::with_capacity(chunk_count as usize);
    let mut cursor = HEADER_LEN as u64;
    for row in table.chunks_exact(ENTRY_LEN) {
        let chunk = Chunk {
            kind: row[0..4].try_into().unwrap(),
            flags: u32_at(row, 4),
            offset: u64_at(row, 8),
            length: u64_at(row, 16),
            crc32: u32_at(row, 24),
        };
        // Chunks lie in order between the header and the table, never overlap.
        if chunk.offset < cursor {
            return Err(Error::Corrupt("chunk overlaps its predecessor"));
        }
        let end = chunk
            .offset
            .checked_add(chunk.length)
            .ok_or(Error::Corrupt("chunk length overflow"))?;
        if end > table_offset {
            return Err(Error::Corrupt("chunk runs into the table"));
        }
        cursor = end;
        chunks.push(chunk);
    }
    if chunks[0].kind != KIND_META {
        return Err(Error::Corrupt("chunk 0 is not META"));
    }
    Ok(Table {
        version: VERSION,
        file_len,
        table_offset,
        chunks,
    })
}

/// A container held in memory.
pub struct Container<'a> {
    bytes: &'a [u8],
    table: Table,
}

impl<'a> Container<'a> {
    /// Validates the header and table; reads no payload.
    pub fn parse(bytes: &'a [u8]) -> Result<Container<'a>, Error> {
        let (count, table_offset, file_len) = parse_header(bytes)?;
        if (bytes.len() as u64) < file_len {
            return Err(Error::Truncated {
                expected: file_len,
                actual: bytes.len() as u64,
            });
        }
        if bytes.len() as u64 != file_len {
            return Err(Error::Corrupt("trailing bytes"));
        }
        let table = parse_table(
            &bytes[table_offset as usize..],
            count,
            table_offset,
            file_len,
        )?;
        Ok(Container { bytes, table })
    }

    pub fn table(&self) -> &Table {
        &self.table
    }

    pub fn chunks(&self) -> &[Chunk] {
        &self.table.chunks
    }

    /// The payload of chunk `index`, checksum verified, if it is at most
    /// `max_len` bytes.
    pub fn read(&self, index: usize, max_len: u64) -> Result<&'a [u8], Error> {
        let chunk = *self
            .table
            .chunks
            .get(index)
            .ok_or(Error::ChunkOutOfRange(index))?;
        if chunk.length > max_len {
            return Err(Error::ChunkTooLarge {
                index,
                length: chunk.length,
                max: max_len,
            });
        }
        let payload = &self.bytes[chunk.offset as usize..(chunk.offset + chunk.length) as usize];
        if crc32fast::hash(payload) != chunk.crc32 {
            return Err(Error::Checksum { index });
        }
        Ok(payload)
    }

    /// The `META` chunk (chunk 0).
    pub fn meta(&self, max_len: u64) -> Result<&'a [u8], Error> {
        self.read(0, max_len)
    }

    /// Verifies every payload's checksum.
    pub fn verify(&self) -> Result<(), Error> {
        for i in 0..self.table.chunks.len() {
            self.read(i, u64::MAX)?;
        }
        Ok(())
    }
}

/// A container read from a seekable stream: only the header, the table and
/// the requested chunks are ever read.
pub struct ChunkFile<R: Read + Seek> {
    inner: R,
    table: Table,
}

impl<R: Read + Seek> ChunkFile<R> {
    pub fn open(mut inner: R) -> Result<ChunkFile<R>, Error> {
        let mut header = [0u8; HEADER_LEN];
        inner.seek(SeekFrom::Start(0))?;
        read_fully(&mut inner, &mut header)
            .map_err(|_| Error::NotCooked("shorter than a header"))?;
        let (count, table_offset, file_len) = parse_header(&header)?;
        let actual = inner.seek(SeekFrom::End(0))?;
        if actual < file_len {
            return Err(Error::Truncated {
                expected: file_len,
                actual,
            });
        }
        if actual != file_len {
            return Err(Error::Corrupt("trailing bytes"));
        }
        let mut table = vec![0u8; count as usize * ENTRY_LEN];
        inner.seek(SeekFrom::Start(table_offset))?;
        read_fully(&mut inner, &mut table)?;
        let table = parse_table(&table, count, table_offset, file_len)?;
        Ok(ChunkFile { inner, table })
    }

    pub fn table(&self) -> &Table {
        &self.table
    }

    pub fn chunks(&self) -> &[Chunk] {
        &self.table.chunks
    }

    /// Reads chunk `index` (at most `max_len` bytes) and verifies it.
    pub fn read(&mut self, index: usize, max_len: u64) -> Result<Vec<u8>, Error> {
        let chunk = *self
            .table
            .chunks
            .get(index)
            .ok_or(Error::ChunkOutOfRange(index))?;
        if chunk.length > max_len {
            return Err(Error::ChunkTooLarge {
                index,
                length: chunk.length,
                max: max_len,
            });
        }
        let mut payload = vec![0u8; chunk.length as usize];
        self.inner.seek(SeekFrom::Start(chunk.offset))?;
        read_fully(&mut self.inner, &mut payload)?;
        if crc32fast::hash(&payload) != chunk.crc32 {
            return Err(Error::Checksum { index });
        }
        Ok(payload)
    }

    pub fn meta(&mut self, max_len: u64) -> Result<Vec<u8>, Error> {
        self.read(0, max_len)
    }

    pub fn verify(&mut self) -> Result<(), Error> {
        for i in 0..self.table.chunks.len() {
            self.read(i, u64::MAX)?;
        }
        Ok(())
    }
}

impl ChunkFile<std::fs::File> {
    pub fn open_path(path: &Path) -> Result<ChunkFile<std::fs::File>, Error> {
        ChunkFile::open(std::fs::File::open(path)?)
    }
}

fn read_fully<R: Read>(r: &mut R, buf: &mut [u8]) -> Result<(), Error> {
    r.read_exact(buf).map_err(|e| {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            Error::Corrupt("ends early")
        } else {
            Error::Io(e)
        }
    })
}

/// Builds a container: `META` first, then chunks in push order.
pub struct Writer {
    payloads: Vec<u8>,
    chunks: Vec<Chunk>,
}

impl Writer {
    pub fn new(meta: &[u8]) -> Writer {
        let mut w = Writer {
            payloads: Vec::new(),
            chunks: Vec::new(),
        };
        w.push(KIND_META, meta);
        w
    }

    /// Appends a chunk and returns its index.
    pub fn push(&mut self, kind: [u8; 4], payload: &[u8]) -> usize {
        while !(HEADER_LEN + self.payloads.len()).is_multiple_of(8) {
            self.payloads.push(0);
        }
        let offset = (HEADER_LEN + self.payloads.len()) as u64;
        self.payloads.extend_from_slice(payload);
        self.chunks.push(Chunk {
            kind,
            flags: 0,
            offset,
            length: payload.len() as u64,
            crc32: crc32fast::hash(payload),
        });
        self.chunks.len() - 1
    }

    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    pub fn finish(mut self) -> Vec<u8> {
        while !(HEADER_LEN + self.payloads.len()).is_multiple_of(8) {
            self.payloads.push(0);
        }
        let table_offset = (HEADER_LEN + self.payloads.len()) as u64;
        let file_len = table_offset + (self.chunks.len() * ENTRY_LEN) as u64;
        let mut out = Vec::with_capacity(file_len as usize);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&(HEADER_LEN as u32).to_le_bytes());
        out.extend_from_slice(&(self.chunks.len() as u32).to_le_bytes());
        out.extend_from_slice(&table_offset.to_le_bytes());
        out.extend_from_slice(&file_len.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        let crc = crc32fast::hash(&out);
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes());
        debug_assert_eq!(out.len(), HEADER_LEN);
        out.extend_from_slice(&self.payloads);
        for c in &self.chunks {
            out.extend_from_slice(&c.kind);
            out.extend_from_slice(&c.flags.to_le_bytes());
            out.extend_from_slice(&c.offset.to_le_bytes());
            out.extend_from_slice(&c.length.to_le_bytes());
            out.extend_from_slice(&c.crc32.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
        }
        debug_assert_eq!(out.len() as u64, file_len);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        let mut w = Writer::new(br#"{"id":"x"}"#);
        assert_eq!(w.push(KIND_JSON, b"{}"), 1);
        assert_eq!(w.push(KIND_BLOB, &[7u8; 1000]), 2);
        assert_eq!(w.push(KIND_MESH, b""), 3);
        w.finish()
    }

    #[test]
    fn round_trips_in_memory_and_through_a_stream() {
        let bytes = sample();
        let c = Container::parse(&bytes).unwrap();
        assert_eq!(c.chunks().len(), 4);
        assert_eq!(c.meta(1 << 20).unwrap(), br#"{"id":"x"}"#);
        assert_eq!(c.read(1, 1 << 20).unwrap(), b"{}");
        assert_eq!(c.read(2, 1 << 20).unwrap().len(), 1000);
        assert_eq!(c.read(3, 1 << 20).unwrap(), b"");
        assert!(c.chunks().iter().all(|ch| ch.offset.is_multiple_of(8)));
        c.verify().unwrap();

        let mut f = ChunkFile::open(std::io::Cursor::new(bytes.clone())).unwrap();
        assert_eq!(f.table(), c.table());
        assert_eq!(f.read(2, 1 << 20).unwrap(), vec![7u8; 1000]);
        f.verify().unwrap();
    }

    #[test]
    fn bounded_reads_and_bad_files_are_refused() {
        let bytes = sample();
        let c = Container::parse(&bytes).unwrap();
        assert!(matches!(
            c.read(2, 999),
            Err(Error::ChunkTooLarge {
                index: 2,
                length: 1000,
                max: 999
            })
        ));
        assert!(matches!(
            c.read(9, u64::MAX),
            Err(Error::ChunkOutOfRange(9))
        ));

        assert!(matches!(
            Container::parse(b"nope"),
            Err(Error::NotCooked(_))
        ));
        assert!(matches!(
            Container::parse(&bytes[..bytes.len() - 1]),
            Err(Error::Truncated { .. })
        ));
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(matches!(
            Container::parse(&longer),
            Err(Error::Corrupt("trailing bytes"))
        ));

        // A flipped payload byte: the table is fine, the chunk is not.
        let mut flipped = bytes.clone();
        let off = c.chunks()[2].offset as usize + 10;
        flipped[off] ^= 0xff;
        let c2 = Container::parse(&flipped).unwrap();
        assert!(matches!(
            c2.read(2, u64::MAX),
            Err(Error::Checksum { index: 2 })
        ));
        assert!(c2.read(1, u64::MAX).is_ok());
        assert!(c2.verify().is_err());

        // A flipped header byte.
        let mut bad_header = bytes.clone();
        bad_header[12] ^= 1;
        assert!(matches!(
            Container::parse(&bad_header),
            Err(Error::Corrupt("header checksum"))
        ));

        // A different version.
        let mut v2 = bytes.clone();
        v2[4] = 2;
        let crc = crc32fast::hash(&v2[0..36]);
        v2[36..40].copy_from_slice(&crc.to_le_bytes());
        assert!(matches!(Container::parse(&v2), Err(Error::Version(2))));

        // A table entry pointing past the table.
        let mut bad_table = bytes.clone();
        let row = c.table().table_offset as usize + 2 * ENTRY_LEN;
        bad_table[row + 16..row + 24].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(matches!(
            Container::parse(&bad_table),
            Err(Error::Corrupt("chunk length overflow"))
        ));
    }
}
