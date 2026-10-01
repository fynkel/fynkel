//! Engine-owned 128-bit runtime references (ADR-0047). Wire order is
//! world u64, slot u32, generation u32, little-endian. Runtime references
//! are not persistent authored identifiers. Hosts share one process-lifetime
//! authority across every engine module whose runtime references can meet.

use core::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct WorldId(pub u64);
impl WorldId {
    pub const NONE: Self = Self(0);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdentityExhausted;

// ABI 1 of the opaque authority: two aligned u64 words. The counter is
// never freed, so an engine module may unload without invalidating it.
const AUTHORITY_ABI: u64 = 0x4659_4e49_4400_0001;

#[repr(C)]
pub struct IdentityAuthority {
    abi: u64,
    next_world: AtomicU64,
}

impl IdentityAuthority {
    fn new() -> Self {
        Self {
            abi: AUTHORITY_ABI,
            next_world: AtomicU64::new(0),
        }
    }

    pub fn is_compatible(&self) -> bool {
        self.abi == AUTHORITY_ABI
    }
}

static AUTHORITY: OnceLock<&'static IdentityAuthority> = OnceLock::new();

/// The selected process-lifetime authority. The first engine can bootstrap it;
/// the host retains this reference and attaches it to every additional module
/// before creating runtimes or worlds. The small allocation intentionally lives
/// until process exit, independently of engine code and world lifetimes.
pub fn identity_authority() -> &'static IdentityAuthority {
    AUTHORITY.get_or_init(|| Box::leak(Box::new(IdentityAuthority::new())))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuthorityMismatch;

fn attach(
    cell: &OnceLock<&'static IdentityAuthority>,
    authority: &'static IdentityAuthority,
) -> Result<(), AuthorityMismatch> {
    if !authority.is_compatible() {
        return Err(AuthorityMismatch);
    }
    let selected = cell.get_or_init(|| authority);
    if std::ptr::eq(*selected, authority) {
        Ok(())
    } else {
        Err(AuthorityMismatch)
    }
}

/// Attaches the host's authority before this module is used. Reattaching the
/// same authority is harmless; replacement is rejected even before allocation.
/// All core, ECS, authoring and runtime worlds then use the selected counter.
pub fn attach_identity_authority(
    authority: &'static IdentityAuthority,
) -> Result<(), AuthorityMismatch> {
    attach(&AUTHORITY, authority)
}

fn allocate(counter: &AtomicU64) -> Result<WorldId, IdentityExhausted> {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .map(|n| WorldId(n + 1))
        .map_err(|_| IdentityExhausted)
}

/// Allocates from the host-selected authority. Exhaustion permanently fails;
/// no world ID is recycled, including after library unload/replacement.
pub fn allocate_world_id() -> Result<WorldId, IdentityExhausted> {
    allocate(&identity_authority().next_world)
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
#[repr(C)]
pub struct Entity {
    world: u64,
    slot: u32,
    generation: u32,
}

impl Entity {
    pub const NULL: Self = Self {
        world: 0,
        slot: 0,
        generation: 0,
    };
    pub const GENERATION_BITS: u32 = 32;
    pub const GENERATION_MASK: u32 = u32::MAX;
    pub const fn new(world: WorldId, slot: u32, generation: u32) -> Self {
        assert!(
            world.0 != 0 && generation != 0,
            "invalid live entity identity"
        );
        Self {
            world: world.0,
            slot,
            generation,
        }
    }
    /// Lossless diagnostic representation; never pass through JSON numbers.
    pub const fn from_bits(bits: u128) -> Self {
        Self {
            world: bits as u64,
            slot: (bits >> 64) as u32,
            generation: (bits >> 96) as u32,
        }
    }
    pub const fn to_bits(self) -> u128 {
        self.world as u128 | ((self.slot as u128) << 64) | ((self.generation as u128) << 96)
    }
    pub fn to_hex(self) -> String {
        format!("{:032x}", self.to_bits())
    }
    pub fn from_hex(value: &str) -> Option<Self> {
        if value.len() != 32 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        u128::from_str_radix(value, 16).ok().map(Self::from_bits)
    }
    pub const fn is_null(self) -> bool {
        self.world == 0 && self.slot == 0 && self.generation == 0
    }
    pub const fn world(self) -> WorldId {
        WorldId(self.world)
    }
    pub const fn generation(self) -> u32 {
        self.generation
    }
    pub const fn slot(self) -> u32 {
        self.slot
    }
    // Transitional tick-command references only. Never accepted by live ECS
    // lookup. F2 replaces the old command protocol with scoped tagged targets.
    pub const fn temp(temp_id: u32) -> Self {
        Self {
            world: 0,
            slot: temp_id,
            generation: 0,
        }
    }
    pub const fn temp_id(self) -> Option<u32> {
        if self.world == 0 && self.generation == 0 && self.slot != 0 {
            Some(self.slot)
        } else {
            None
        }
    }
    /// Exhaustion retires the slot; generations never wrap.
    pub const fn next_generation(g: u32) -> Option<u32> {
        g.checked_add(1)
    }
}

impl fmt::Debug for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_null() {
            return f.write_str("Entity(NULL)");
        }
        if let Some(t) = self.temp_id() {
            return write!(f, "Entity(temp #{t})");
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
    fn full_width_roundtrip_and_layout() {
        let entity = Entity::new(WorldId(u64::MAX), u32::MAX, u32::MAX);
        assert_eq!(Entity::from_bits(entity.to_bits()), entity);
        assert_eq!(Entity::from_hex(&entity.to_hex()), Some(entity));
        assert_eq!(core::mem::size_of::<Entity>(), 16);
        assert_eq!(core::mem::align_of::<Entity>(), 8);
        assert_eq!(Entity::next_generation(u32::MAX), None);
        assert_eq!(Entity::next_generation(1), Some(2));
        assert_eq!(Entity::temp(7).temp_id(), Some(7));
        assert_ne!(Entity::temp(7), Entity::new(WorldId(7), 0, 1));
    }
    #[test]
    fn authority_exhaustion_is_permanent() {
        let counter = AtomicU64::new(u64::MAX - 1);
        assert_eq!(allocate(&counter), Ok(WorldId(u64::MAX)));
        assert_eq!(allocate(&counter), Err(IdentityExhausted));
        assert_eq!(allocate(&counter), Err(IdentityExhausted));
    }
    #[test]
    fn authority_attachment_is_sticky_and_thread_safe() {
        let cell = OnceLock::new();
        let owner = Box::leak(Box::new(IdentityAuthority::new()));
        let other = Box::leak(Box::new(IdentityAuthority::new()));
        assert_eq!(attach(&cell, owner), Ok(()));
        assert_eq!(attach(&cell, owner), Ok(()));
        assert_eq!(attach(&cell, other), Err(AuthorityMismatch));
        let mut ids = std::thread::scope(|scope| {
            let threads: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        assert_eq!(attach(&cell, owner), Ok(()));
                        (0..1000)
                            .map(|_| allocate(&owner.next_world).unwrap())
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            threads
                .into_iter()
                .flat_map(|t| t.join().unwrap())
                .collect::<Vec<_>>()
        });
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 8000);
        assert_eq!(ids[0], WorldId(1));
        assert_eq!(ids[7999], WorldId(8000));
    }

    #[test]
    fn incompatible_authority_does_not_select_a_counter() {
        let cell = OnceLock::new();
        let bad = Box::leak(Box::new(IdentityAuthority {
            abi: 0,
            next_world: AtomicU64::new(0),
        }));
        assert_eq!(attach(&cell, bad), Err(AuthorityMismatch));
        assert!(cell.get().is_none());
    }
}
