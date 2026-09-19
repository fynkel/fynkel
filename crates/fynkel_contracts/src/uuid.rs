//! 128-bit persistent ids (ADR-0006): the `id` of every document, entity
//! and asset. Text form is the canonical lower-case hyphenated UUID; the
//! wire and the cooked container carry the 16 bytes.

use core::fmt;
use core::str::FromStr;

/// A UUID as 16 big-endian bytes; `NIL` means "unset".
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Uuid(pub [u8; 16]);

impl Uuid {
    pub const NIL: Uuid = Uuid([0; 16]);

    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    pub fn is_nil(&self) -> bool {
        self.0 == [0; 16]
    }
}

impl fmt::Display for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = [0u8; 36];
        let mut o = 0;
        for (i, b) in self.0.iter().enumerate() {
            if matches!(i, 4 | 6 | 8 | 10) {
                out[o] = b'-';
                o += 1;
            }
            out[o] = HEX[(b >> 4) as usize];
            out[o + 1] = HEX[(b & 15) as usize];
            o += 2;
        }
        f.write_str(core::str::from_utf8(&out).unwrap())
    }
}

impl fmt::Debug for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Uuid({self})")
    }
}

/// Text that is not a canonical lower-case hyphenated UUID.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseUuidError;

impl fmt::Display for ParseUuidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("expected a lower-case hyphenated UUID (8-4-4-4-12)")
    }
}

impl std::error::Error for ParseUuidError {}

impl FromStr for Uuid {
    type Err = ParseUuidError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let b = s.as_bytes();
        if b.len() != 36 {
            return Err(ParseUuidError);
        }
        let mut out = [0u8; 16];
        let mut i = 0;
        let mut pos = 0;
        while pos < 36 {
            if matches!(pos, 8 | 13 | 18 | 23) {
                if b[pos] != b'-' {
                    return Err(ParseUuidError);
                }
                pos += 1;
                continue;
            }
            let hi = nibble(b[pos]).ok_or(ParseUuidError)?;
            let lo = nibble(b[pos + 1]).ok_or(ParseUuidError)?;
            out[i] = (hi << 4) | lo;
            i += 1;
            pos += 2;
        }
        Ok(Uuid(out))
    }
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_canonical_text() {
        let text = "70d7ba8b-f7fb-4eba-920d-ec9e6b968330";
        let id: Uuid = text.parse().unwrap();
        assert_eq!(id.to_string(), text);
        assert_eq!(id.0[0], 0x70);
        assert_eq!(id.0[15], 0x30);
        assert!(!id.is_nil());
        assert_eq!(
            Uuid::NIL.to_string(),
            "00000000-0000-0000-0000-000000000000"
        );
    }

    #[test]
    fn rejects_non_canonical_text() {
        for bad in [
            "",
            "70D7BA8B-F7FB-4EBA-920D-EC9E6B968330",
            "70d7ba8bf7fb4eba920dec9e6b968330",
            "70d7ba8b-f7fb-4eba-920d-ec9e6b96833",
            "70d7ba8b-f7fb-4eba-920d-ec9e6b96833g",
            "70d7ba8b_f7fb_4eba_920d_ec9e6b968330",
        ] {
            assert!(bad.parse::<Uuid>().is_err(), "{bad}");
        }
    }
}
