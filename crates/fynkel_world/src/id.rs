//! Stable ids (stage 1 plan §3.1).
//!
//! Every object, mesh, material and texture gets an id from its own
//! counter. Ids
//! start at 1 and are never reused, so an old id can never point at a new
//! thing. 0 is "none" at the C boundary, which is why the ids wrap
//! `NonZeroU64` (`Option<ObjectId>` is also 8 bytes).

use std::fmt;
use std::num::NonZeroU64;

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident, $prefix:literal) => {
        $(#[$doc])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(NonZeroU64);

        impl $name {
            /// The id for a raw value, or `None` for 0.
            pub const fn from_raw(raw: u64) -> Option<Self> {
                match NonZeroU64::new(raw) {
                    Some(n) => Some(Self(n)),
                    None => None,
                }
            }

            /// The raw value, never 0.
            pub const fn raw(self) -> u64 {
                self.0.get()
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!($prefix, "#{}"), self.0)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Debug::fmt(self, f)
            }
        }
    };
}

id_type!(
    /// An object in the world.
    ObjectId,
    "object"
);
id_type!(
    /// A mesh resource.
    MeshId,
    "mesh"
);
id_type!(
    /// A material resource.
    MaterialId,
    "material"
);
id_type!(
    /// A texture resource: an image that materials use.
    TextureId,
    "texture"
);

id_type!(
    /// A material's own shader (stage 10).
    ShaderId,
    "shader"
);

/// Hands out ids from 1 upwards and never repeats one.
#[derive(Debug)]
pub(crate) struct IdCounter(u64);

impl IdCounter {
    pub(crate) const fn new() -> Self {
        Self(0)
    }

    pub(crate) fn next(&mut self) -> NonZeroU64 {
        // 2^64 ids is more than any program can create, at a billion per
        // second it takes 584 years, so running out is a real bug.
        self.0 = self.0.checked_add(1).expect("id counter exhausted");
        NonZeroU64::new(self.0).expect("counter starts at 1")
    }
}

impl ObjectId {
    pub(crate) const fn new(n: NonZeroU64) -> Self {
        Self(n)
    }
}

impl MeshId {
    pub(crate) const fn new(n: NonZeroU64) -> Self {
        Self(n)
    }
}

impl MaterialId {
    pub(crate) const fn new(n: NonZeroU64) -> Self {
        Self(n)
    }
}

impl TextureId {
    pub(crate) const fn new(n: NonZeroU64) -> Self {
        Self(n)
    }
}

impl ShaderId {
    pub(crate) const fn new(n: NonZeroU64) -> Self {
        Self(n)
    }
}

/// A fast hasher for ids. Ids are small sequential integers, so the
/// standard SipHash (built to resist attackers) only costs time, and an
/// identity hash would put every small id in the same hashbrown control
/// group. One multiply by the golden-ratio constant spreads them well.
#[derive(Default, Debug, Clone, Copy)]
pub(crate) struct IdHasher(u64);

impl std::hash::Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, _bytes: &[u8]) {
        unreachable!("ids hash through write_u64");
    }

    fn write_u64(&mut self, n: u64) {
        self.0 = n.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}

pub(crate) type IdMap<K, V> = std::collections::HashMap<K, V, std::hash::BuildHasherDefault<IdHasher>>;
