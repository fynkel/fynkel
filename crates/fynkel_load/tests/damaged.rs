//! Damaged files (stage 6, PR 13): the samples with bits flipped, bytes
//! changed, cut short, grown, and numbers in a model's JSON changed, must
//! each be loaded or refused with an error, never panic. A game can be
//! given any file, from a download or a disk that went wrong. (The gltf
//! crate panics on a few, which `ModelData::from_glb` turns into errors.)
//!
//! Seeded, so a failure can be repeated: it prints the seed and the
//! sample. `FYNKEL_FUZZ_ITERS` sets how many damaged copies of each sample
//! (a few hundred by default, of the small samples; more in CI's long
//! run, of every sample, built for release), and
//! `FYNKEL_FUZZ_SEED` the first seed.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

use fynkel_load::{ImageData, ModelData};

fn sample(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/assets").join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn env(name: &str, or: u64) -> u64 {
    std::env::var(name).ok().and_then(|n| n.parse().ok()).unwrap_or(or)
}

/// A small random generator (xorshift64*): no dependency, and the same
/// numbers everywhere for a seed.
struct Random(u64);

impl Random {
    fn new(seed: u64) -> Self {
        // Never zero, and seeds next to each other far apart.
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// `bytes` damaged in one to four ways. In a `.glb`, a digit of its JSON
/// is often changed, so indices, counts and offsets point elsewhere while
/// the file still reads as glTF.
fn damage(bytes: &[u8], random: &mut Random) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let json = json_range(bytes);
    for _ in 0..1 + random.below(4) {
        if out.is_empty() {
            out.push(random.next() as u8);
        }
        match random.below(7) {
            0 => {
                let at = random.below(out.len());
                out[at] ^= 1 << random.below(8);
            }
            1 => {
                let at = random.below(out.len());
                out[at] = random.next() as u8;
            }
            2 => out.truncate(random.below(out.len())),
            3 => {
                let at = random.below(out.len() + 1);
                let len = 1 + random.below(64);
                let more: Vec<u8> = (0..len).map(|_| random.next() as u8).collect();
                out.splice(at..at, more);
            }
            4 => {
                // A run of bytes copied over another place.
                let from = random.below(out.len());
                let len = random.below(256).min(out.len() - from);
                let to = random.below(out.len() - len + 1);
                let run = out[from..from + len].to_vec();
                out[to..to + len].copy_from_slice(&run);
            }
            _ => {
                // A digit of the JSON, if there is one, made another or a 9.
                let Some((start, end)) = json.filter(|&(_, end)| end <= out.len()) else { continue };
                let digits: Vec<usize> = (start..end).filter(|&i| out[i].is_ascii_digit()).collect();
                if digits.is_empty() {
                    continue;
                }
                let at = digits[random.below(digits.len())];
                out[at] = if random.below(2) == 0 { b'9' } else { b'0' + random.below(10) as u8 };
            }
        }
    }
    out
}

/// Where a `.glb`'s JSON chunk is.
fn json_range(bytes: &[u8]) -> Option<(usize, usize)> {
    if bytes.len() < 20 || &bytes[0..4] != b"glTF" {
        return None;
    }
    let len = u32::from_le_bytes(bytes[12..16].try_into().ok()?) as usize;
    Some((20, (20 + len).min(bytes.len())))
}

/// Damages `name` `FYNKEL_FUZZ_ITERS` times and hands each copy to `load`, which
/// must return rather than panic. Gives back how many copies loaded.
fn survives(name: &str, load: impl Fn(&[u8]) -> bool) -> usize {
    let bytes = sample(name);
    let iterations = env("FYNKEL_FUZZ_ITERS", 300);
    let first = env("FYNKEL_FUZZ_SEED", 1);
    // Each panic's place is kept, and printed with its seed below.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|info| {
        let place = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        PLACE.with(|p| *p.borrow_mut() = place);
    }));
    let mut loaded = 0;
    let mut panics = Vec::new();
    for seed in first..first + iterations {
        let damaged = damage(&bytes, &mut Random::new(seed));
        match catch_unwind(AssertUnwindSafe(|| load(&damaged))) {
            Ok(true) => loaded += 1,
            Ok(false) => {}
            Err(panic) => {
                let message = panic
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| panic.downcast_ref::<String>().cloned())
                    .unwrap_or_default();
                let place = PLACE.with(|p| p.take());
                panics.push(format!("{name}, FYNKEL_FUZZ_SEED={seed}: {message} at {place}"));
            }
        }
    }
    std::panic::set_hook(hook);
    assert!(panics.is_empty(), "{} of {iterations} damaged copies panicked:\n{}", panics.len(), panics.join("\n"));
    loaded
}

thread_local! {
    static PLACE: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

fn model(bytes: &[u8]) -> bool {
    ModelData::from_glb(bytes).is_ok()
}

fn image(bytes: &[u8]) -> bool {
    ImageData::decode(bytes).is_ok()
}

#[test]
fn damaged_models_load_or_are_refused() {
    let mut names =
        vec!["gltf/Box.glb", "gltf/BoxTextured.glb", "gltf/OrientationTest.glb", "gltf/tower.glb", "gltf/Fox.glb"];
    // The big ones, their images seconds to decode in a debug build, only
    // in the long run, built for release.
    if std::env::var("FYNKEL_FUZZ_ITERS").is_ok() {
        names.extend(["gltf/CesiumMilkTruck.glb", "gltf/AlphaBlendModeTest.glb", "gltf/NormalTangentMirrorTest.glb"]);
    }
    for name in names {
        survives(name, model);
    }
}

#[test]
fn damaged_images_load_or_are_refused() {
    for name in ["grid.png", "grid.jpg"] {
        survives(name, image);
    }
}

#[test]
fn the_damage_is_real() {
    // Most copies of a small model are refused: the test isn't loading the
    // same file over and over.
    let loaded = survives("gltf/Box.glb", model);
    assert!(loaded < env("FYNKEL_FUZZ_ITERS", 300) as usize, "{loaded}");
}
