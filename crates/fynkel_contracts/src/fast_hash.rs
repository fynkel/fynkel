//! A fast, non-cryptographic hasher for the engine's internal tables.
//!
//! `std`'s default SipHash is built to resist attacker-chosen keys, which no
//! Fynkel table sees: its keys are entity handles, `TypeId`s, asset ids and
//! batch keys the engine itself made. SipHash was 17% of a dense scene's
//! main-thread time (F8 profile, 2026-10-05); this multiply-rotate hash
//! (the FxHash scheme) costs a few cycles per word.
//!
//! Iteration order is as unspecified as `std`'s: nothing may depend on it.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

#[derive(Clone, Copy, Default)]
pub struct FastHasher {
    hash: u64,
}

impl FastHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(SEED);
    }
}

impl Hasher for FastHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for c in &mut chunks {
            self.add(u64::from_le_bytes(c.try_into().unwrap()));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            let mut tail = [0u8; 8];
            tail[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(tail) ^ ((rest.len() as u64) << 56));
        }
    }
    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(u64::from(i));
    }
    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.add(u64::from(i));
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(u64::from(i));
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }
    #[inline]
    fn write_u128(&mut self, i: u128) {
        self.add(i as u64);
        self.add((i >> 64) as u64);
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }
    #[inline]
    fn finish(&self) -> u64 {
        // hashbrown indexes with the low bits and tags with the top seven:
        // fold the multiply's well-mixed high half down.
        self.hash ^ (self.hash >> 32)
    }
}

pub type FastBuildHasher = BuildHasherDefault<FastHasher>;
pub type FastMap<K, V> = HashMap<K, V, FastBuildHasher>;
pub type FastSet<K> = HashSet<K, FastBuildHasher>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn behaves_as_a_map() {
        let mut m: FastMap<(u32, String), u32> = FastMap::default();
        for i in 0..1000u32 {
            m.insert((i, format!("mesh/{}", i % 7)), i);
        }
        for i in 0..1000u32 {
            assert_eq!(m[&(i, format!("mesh/{}", i % 7))], i);
        }
        assert_eq!(m.len(), 1000);
    }

    #[test]
    fn distinguishes_lengths_and_tails() {
        let h = |b: &[u8]| {
            let mut h = FastHasher::default();
            h.write(b);
            h.finish()
        };
        assert_ne!(h(b"a"), h(b"a\0"));
        assert_ne!(h(b"abcdefgh"), h(b"abcdefgh\0"));
        assert_ne!(h(b""), h(b"\0"));
    }

    #[test]
    fn sequential_keys_spread_across_low_bits() {
        let mut buckets = [0u32; 64];
        for i in 0..6400u64 {
            let mut h = FastHasher::default();
            h.write_u64(i);
            buckets[(h.finish() & 63) as usize] += 1;
        }
        assert!(buckets.iter().all(|&n| (60..140).contains(&n)), "{buckets:?}");
    }
}
