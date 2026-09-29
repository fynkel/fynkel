//! Foundation 5.7: the readers of untrusted bytes — the glTF, FBX, image
//! and audio importers, the `.fyna` container and its payload decoders —
//! answer damaged input with a diagnostic or `None`, never a panic. Each
//! fixture is damaged deterministically (cut short, bytes flipped, a
//! 32-bit field overwritten with an extreme value, a slice repeated), a
//! thousand times per file; a panic names the file and the damage.
//!
//! Third-party readers (`gltf`, `symphonia`) do panic on some damage;
//! `fynkel_import::import` catches that and fails the one file. So a panic
//! fails this test when it escapes `import`, or when it is raised in
//! Fynkel's own code, guarded or not.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use fynkel_cooked::payload::{
    decode_animation, decode_clip, decode_custom_material, decode_environment, decode_mesh,
    decode_skin, decode_stream, decode_texture,
};
use fynkel_cooked::{
    Container, KIND_ANIMATION, KIND_CLIP, KIND_CUSTOM_MATERIAL, KIND_ENVIRONMENT, KIND_MESH,
    KIND_SKIN, KIND_STREAM, KIND_TEXTURE,
};
use fynkel_document::asset::{sidecar_path, Meta};
use fynkel_document::Document;
use fynkel_import::{import, Limits};

/// Mutations per input file: 1000, or `FYNKEL_FUZZ_ROUNDS` for a deeper
/// run.
fn rounds() -> u64 {
    std::env::var("FYNKEL_FUZZ_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1000)
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

/// xorshift64*: the same damage on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

/// One damaged copy of `bytes` and what was done to it.
fn damage(bytes: &[u8], rng: &mut Rng) -> (Vec<u8>, String) {
    let mut out = bytes.to_vec();
    let what = match rng.below(4) {
        0 => {
            let at = rng.below(out.len());
            out.truncate(at);
            format!("cut at {at}")
        }
        1 => {
            let n = 1 + rng.below(8);
            for _ in 0..n {
                let at = rng.below(out.len());
                out[at] ^= 1 << rng.below(8);
            }
            format!("{n} bits flipped")
        }
        2 => {
            const EXTREMES: [u32; 6] = [0, 1, 0x7fff_ffff, 0x8000_0000, 0xffff_fffe, 0xffff_ffff];
            let at = rng.below(out.len().saturating_sub(4));
            let v = EXTREMES[rng.below(EXTREMES.len())];
            if out.len() >= 4 {
                out[at..at + 4].copy_from_slice(&v.to_le_bytes());
            }
            format!("u32 {v:#x} at {at}")
        }
        _ => {
            let from = rng.below(out.len());
            let len = rng.below((out.len() - from).min(256));
            let at = rng.below(out.len());
            let slice = out[from..from + len].to_vec();
            out.splice(at..at, slice);
            format!("{len} bytes from {from} repeated at {at}")
        }
    };
    (out, what)
}

/// Where the panics of the current round were raised (`file:line`).
static RAISED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// A panic raised in a dependency, not in Fynkel's code.
fn third_party(at: &str) -> bool {
    at.contains("/.cargo/registry/")
}

/// Runs `f` on damaged copies of `bytes`; the damage that panicked where
/// it must not. Counts the third-party panics `import` caught into
/// `guarded`.
fn panics(
    name: &str,
    bytes: &[u8],
    seed: u64,
    guarded: &mut usize,
    mut f: impl FnMut(&[u8]),
) -> Vec<String> {
    let mut rng = Rng(seed | 1);
    let mut found = Vec::new();
    let shown = std::panic::take_hook();
    let loud = std::env::var_os("FYNKEL_FUZZ_LOUD").is_some();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(at) = info.location() {
            RAISED
                .lock()
                .unwrap()
                .push(format!("{}:{}", at.file(), at.line()));
        }
        if loud {
            eprintln!("{info}\n{}", std::backtrace::Backtrace::force_capture());
        }
    }));
    for round in 0..rounds() {
        let (damaged, what) = damage(bytes, &mut rng);
        let escaped = catch_unwind(AssertUnwindSafe(|| f(&damaged))).err();
        let raised = std::mem::take(&mut *RAISED.lock().unwrap());
        if let Some(payload) = escaped {
            let msg = payload
                .downcast_ref::<&str>()
                .map(|m| m.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_default();
            found.push(format!(
                "{name}: round {round}, {what}: escaped at {raised:?}: {msg}"
            ));
        } else {
            for at in raised {
                if third_party(&at) {
                    *guarded += 1;
                } else {
                    found.push(format!("{name}: round {round}, {what}: raised at {at}"));
                }
            }
        }
    }
    let _ = std::panic::take_hook();
    std::panic::set_hook(shown);
    found
}

/// Both checks in one test: the recording panic hook ([`panics`]) is
/// global.
#[test]
fn readers_never_panic_on_damaged_input() {
    let mut found = importers();
    found.extend(cooked_containers());
    assert!(
        found.is_empty(),
        "{} panics:\n{}",
        found.len(),
        found.join("\n")
    );
}

fn importers() -> Vec<String> {
    let dir = std::env::temp_dir().join(format!("fynkel-fuzz-import-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut found = Vec::new();
    for (i, name) in [
        "robot.glb",
        "rig.glb",
        "robot_data_uri.gltf",
        "robot.fbx",
        "rig.fbx",
        "paint.png",
        "audio/tone.wav",
        "audio/tone.ogg",
    ]
    .into_iter()
    .enumerate()
    {
        let source = fixture(name);
        let bytes = std::fs::read(&source).unwrap();
        let sidecar = sidecar_path(&source);
        let meta = if sidecar.exists() {
            Meta::from_document(Document::read(&sidecar, None).unwrap())
        } else {
            Meta::new(&source)
        };
        let target = dir.join(source.file_name().unwrap());
        let clean = import(&source, &meta, &Limits::default());
        assert!(!clean.has_errors(), "{name}: {:?}", clean.diagnostics);
        let (mut refused, mut guarded) = (0, 0);
        found.extend(panics(
            name,
            &bytes,
            0x9e37_79b9 + i as u64,
            &mut guarded,
            |damaged| {
                std::fs::write(&target, damaged).unwrap();
                if import(&target, &meta, &Limits::default()).has_errors() {
                    refused += 1;
                }
            },
        ));
        eprintln!(
            "{name}: {} bytes, {refused} of {} damaged copies refused, \
             {guarded} by a dependency's panic",
            bytes.len(),
            rounds()
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
    found
}

fn cooked_containers() -> Vec<String> {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fynkel_document/fixtures/project");
    let cache = std::env::temp_dir().join(format!("fynkel-fuzz-cooked-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let report = fynkel_import::cook_project(&project, Some(&cache), &Default::default())
        .unwrap_or_else(|d| panic!("{d:?}"));
    assert!(!report.has_errors(), "{}", report.text());
    let mut found = Vec::new();
    for (i, asset) in report.assets.iter().enumerate() {
        let Some(file) = &asset.file else { continue };
        let bytes = std::fs::read(file).unwrap();
        let mut guarded = 0;
        found.extend(panics(
            &asset.path,
            &bytes,
            0x51_7cc1 + i as u64,
            &mut guarded,
            read_everything,
        ));
        // The payloads alone, as a reader that trusts the table sees them.
        let c = Container::parse(&bytes).unwrap();
        for (j, chunk) in c.chunks().iter().enumerate() {
            let payload = c.read(j, u64::MAX).unwrap();
            let kind = chunk.kind;
            let name = format!("{}#{}", asset.path, chunk.kind_str());
            found.extend(panics(
                &name,
                payload,
                0xabc + j as u64,
                &mut guarded,
                |damaged| decode(kind, damaged),
            ));
        }
    }
    let _ = std::fs::remove_dir_all(&cache);
    found
}

/// Everything a load does with a container's bytes.
fn read_everything(bytes: &[u8]) {
    let Ok(c) = Container::parse(bytes) else {
        return;
    };
    let _ = c.verify();
    let _ = c.meta(1 << 20);
    for (i, chunk) in c.chunks().iter().enumerate() {
        if let Ok(payload) = c.read(i, 1 << 28) {
            decode(chunk.kind, payload);
        }
    }
}

fn decode(kind: [u8; 4], payload: &[u8]) {
    match kind {
        KIND_MESH => drop(decode_mesh(payload)),
        KIND_TEXTURE => drop(decode_texture(payload)),
        KIND_ENVIRONMENT => drop(decode_environment(payload)),
        KIND_CLIP => drop(decode_clip(payload)),
        KIND_STREAM => drop(decode_stream(payload)),
        KIND_SKIN => drop(decode_skin(payload)),
        KIND_ANIMATION => drop(decode_animation(payload)),
        KIND_CUSTOM_MATERIAL => drop(decode_custom_material(payload)),
        _ => {}
    }
}
