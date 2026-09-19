//! Asset identity as it crosses the ABI and appears in documents (ADR-0006,
//! blueprint §19.2c): a UUID plus an optional sub-asset id (`"mesh/0"`,
//! `"Cube"`) — never a path.

use core::fmt;

pub use crate::uuid::{ParseUuidError as ParseAssetIdError, Uuid};

/// The `id` of an asset's `.meta.json`.
pub type AssetId = Uuid;

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
