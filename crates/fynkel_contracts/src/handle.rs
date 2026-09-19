//! Runtime entity handles (blueprint §4.2).
//!
//! Layout of the 64-bit value that crosses the ABI:
//!
//! ```text
//! 63..56  world id   (8 bits; 0 = "any/unknown", never assigned to a world)
//! 55..32  generation (24 bits; starts at 1, wraps to 1, never 0)
//! 31..0   slot       (32 bits)
//! ```
//!
//! The all-zero handle is [`Entity::NULL`] and is never valid. A handle is
//! valid only while the world's slot still carries the same generation.

use core::fmt;

/// Identity of a world within a runtime. Assigned from 1; 0 is reserved.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct WorldId(pub u8);

impl WorldId {
    pub const NONE: WorldId = WorldId(0);
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
#[repr(transparent)]
pub struct Entity(u64);

impl Entity {
    pub const NULL: Entity = Entity(0);
    pub const GENERATION_BITS: u32 = 24;
    pub const GENERATION_MASK: u32 = (1 << Self::GENERATION_BITS) - 1;

    pub const fn new(world: WorldId, slot: u32, generation: u32) -> Self {
        debug_assert!(generation != 0 && generation <= Self::GENERATION_MASK);
        Entity(
            ((world.0 as u64) << 56)
                | (((generation & Self::GENERATION_MASK) as u64) << 32)
                | slot as u64,
        )
    }

    /// Reinterprets a raw ABI value. Validity is decided by the world.
    pub const fn from_bits(bits: u64) -> Self {
        Entity(bits)
    }
    pub const fn to_bits(self) -> u64 {
        self.0
    }
    pub const fn is_null(self) -> bool {
        self.0 == 0
    }
    pub const fn world(self) -> WorldId {
        WorldId((self.0 >> 56) as u8)
    }
    pub const fn generation(self) -> u32 {
        ((self.0 >> 32) as u32) & Self::GENERATION_MASK
    }
    pub const fn slot(self) -> u32 {
        self.0 as u32
    }

    /// Next generation for a slot; skips 0 so a reused slot never yields NULL.
    pub const fn next_generation(g: u32) -> u32 {
        let n = (g + 1) & Self::GENERATION_MASK;
        if n == 0 {
            1
        } else {
            n
        }
    }
}

impl fmt::Debug for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_null() {
            return f.write_str("Entity(NULL)");
        }
        write!(
            f,
            "Entity(w{} s{} g{})",
            self.world().0,
            self.slot(),
            self.generation()
        )
    }
}

impl fmt::Display for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_and_unpacks() {
        let e = Entity::new(WorldId(3), 0xdead_beef, 0x00ab_cdef);
        assert_eq!(e.world(), WorldId(3));
        assert_eq!(e.slot(), 0xdead_beef);
        assert_eq!(e.generation(), 0x00ab_cdef);
        assert_eq!(Entity::from_bits(e.to_bits()), e);
        assert!(!e.is_null());
        assert!(Entity::NULL.is_null());
    }

    #[test]
    fn generation_never_zero() {
        assert_eq!(Entity::next_generation(Entity::GENERATION_MASK), 1);
        assert_eq!(Entity::next_generation(1), 2);
        let e = Entity::new(WorldId(1), 0, 1);
        assert!(!e.is_null());
    }
}
