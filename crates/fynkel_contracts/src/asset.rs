//! Asset identity as it crosses the ABI and appears in documents (ADR-0006,
//! blueprint §19.2c): a UUID plus an optional sub-asset id (`"mesh/0"`,
//! `"Cube"`) — never a path.

use core::fmt;
use core::str::FromStr;

/// 128-bit asset id (the `id` of the asset's `.meta.json`). Text form is the
/// canonical lower-case hyphenated UUID; `NIL` means "unset".
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct AssetId(pub [u8; 16]);

impl AssetId {
    pub const NIL: AssetId = AssetId([0; 16]);

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

impl fmt::Display for AssetId {
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

impl fmt::Debug for AssetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AssetId({self})")
    }
}

/// Text that is not a canonical lower-case hyphenated UUID.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseAssetIdError;

impl fmt::Display for ParseAssetIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("expected a lower-case hyphenated UUID (8-4-4-4-12)")
    }
}

impl std::error::Error for ParseAssetIdError {}

impl FromStr for AssetId {
    type Err = ParseAssetIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let b = s.as_bytes();
        if b.len() != 36 {
            return Err(ParseAssetIdError);
        }
        let mut out = [0u8; 16];
        let mut i = 0;
        let mut pos = 0;
        while pos < 36 {
            if matches!(pos, 8 | 13 | 18 | 23) {
                if b[pos] != b'-' {
                    return Err(ParseAssetIdError);
                }
                pos += 1;
                continue;
            }
            let hi = nibble(b[pos]).ok_or(ParseAssetIdError)?;
            let lo = nibble(b[pos + 1]).ok_or(ParseAssetIdError)?;
            out[i] = (hi << 4) | lo;
            i += 1;
            pos += 2;
        }
        Ok(AssetId(out))
    }
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

/// Reference to an asset or one of its sub-assets. `sub` empty = the asset
/// itself. Wire: 16 id bytes + `sub` as a string reference (24 bytes);
/// JSON: `{ "asset": "<uuid>", "sub": "<id>" }` with `sub` omitted when
/// empty (`envelope.schema.json#/$defs/assetRef`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct AssetRef {
    pub asset: AssetId,
    pub sub: String,
}

impl AssetRef {
    pub const fn new(asset: AssetId) -> Self {
        Self {
            asset,
            sub: String::new(),
        }
    }

    pub fn with_sub(asset: AssetId, sub: impl Into<String>) -> Self {
        Self {
            asset,
            sub: sub.into(),
        }
    }

    pub fn is_nil(&self) -> bool {
        self.asset.is_nil()
    }
}

impl fmt::Display for AssetRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.sub.is_empty() {
            write!(f, "{}", self.asset)
        } else {
            write!(f, "{}#{}", self.asset, self.sub)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_round_trips_canonical_text() {
        let text = "70d7ba8b-f7fb-4eba-920d-ec9e6b968330";
        let id: AssetId = text.parse().unwrap();
        assert_eq!(id.to_string(), text);
        assert_eq!(id.0[0], 0x70);
        assert_eq!(id.0[15], 0x30);
        assert!(!id.is_nil());
        assert_eq!(
            AssetId::NIL.to_string(),
            "00000000-0000-0000-0000-000000000000"
        );
    }

    #[test]
    fn id_rejects_non_canonical_text() {
        for bad in [
            "",
            "70D7BA8B-F7FB-4EBA-920D-EC9E6B968330",
            "70d7ba8bf7fb4eba920dec9e6b968330",
            "70d7ba8b-f7fb-4eba-920d-ec9e6b96833",
            "70d7ba8b-f7fb-4eba-920d-ec9e6b96833g",
            "70d7ba8b_f7fb_4eba_920d_ec9e6b968330",
        ] {
            assert!(bad.parse::<AssetId>().is_err(), "{bad}");
        }
    }

    #[test]
    fn ref_display() {
        let id: AssetId = "70d7ba8b-f7fb-4eba-920d-ec9e6b968330".parse().unwrap();
        assert_eq!(
            AssetRef::new(id).to_string(),
            "70d7ba8b-f7fb-4eba-920d-ec9e6b968330"
        );
        assert_eq!(
            AssetRef::with_sub(id, "mesh/0").to_string(),
            "70d7ba8b-f7fb-4eba-920d-ec9e6b968330#mesh/0"
        );
    }
}
