//! The content-addressed cache (blueprint §6, §19.2c item 12): every cooked
//! output lives at `.fynkel/cache/objects/<key>.fyna` next to
//! `<key>.manifest.json`, where the key is a BLAKE3 hash of **everything
//! that went into the output**: the asset id, the source bytes, any extra
//! input files, the sidecar settings, the importer (or document kind) and
//! its version, the keys of every dependency (so a change anywhere below
//! propagates up), the target profile, and the format versions of the
//! intermediate, the container and this cache. Same inputs → same key →
//! the output is already there; anything else → a new key, and the old
//! object stays until pruned. Nothing in the cache is ever edited in
//! place, and nothing in it is needed to rebuild it — the committed files
//! are the truth (ADR-0006).
//!
//! `index.json` at the cache root records the last cook: asset id → key,
//! so a loader can find "the current cooked file for this asset" without
//! recomputing keys.
//!
//! `stamps.json` at the cache root remembers each source file's size,
//! modification time and hash, so a cook hashes only the files that
//! changed instead of reading every source again ([`Stamps`]).

use std::fmt;
use std::path::{Path, PathBuf};

use fynkel_contracts::{Code, Diagnostic};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Bump to invalidate every cached object (a change in how keys or
/// chunks are computed that the other versions do not capture).
/// 7: ADR-0020 (imported prefabs are `formatVersion` 4, the object layer).
/// 8: ADR-0022 (scenes and prefabs are `formatVersion` 5, one root).
/// 9: ADR-0031 (`.shader` files cook to `SHDR`, their materials to `MATC`).
/// 10: foundation 2.5 (scenes and prefabs are `formatVersion` 6,
/// `fynkel.RigidBody` carries `constraints`).
/// 11: ADR-0035 (scenes and prefabs are `formatVersion` 7, object
/// references).
pub const CACHE_VERSION: u32 = 13;

/// A BLAKE3 hash, shown as 64 hex characters.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CacheKey([u8; 32]);

impl CacheKey {
    pub fn hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    pub fn parse(hex: &str) -> Option<CacheKey> {
        if hex.len() != 64 {
            return None;
        }
        let mut out = [0u8; 32];
        for (i, byte) in out.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
        }
        Some(CacheKey(out))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for CacheKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CacheKey({})", self.hex())
    }
}

impl fmt::Display for CacheKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.hex())
    }
}

/// `blake3(bytes)`.
pub fn hash_bytes(bytes: &[u8]) -> CacheKey {
    CacheKey(*blake3::hash(bytes).as_bytes())
}

/// Everything a cooked output depends on. [`KeyInputs::key`] folds it into
/// one hash; every field is length-prefixed so no two input sets collide
/// by concatenation.
#[derive(Clone, Debug)]
pub struct KeyInputs {
    pub asset: Uuid,
    /// `fynkel.gltf`, `fynkel.image`, …, or `document/<kind>`.
    pub kind: String,
    /// Importer version, or the document's `formatVersion`.
    pub version: u32,
    pub source: CacheKey,
    /// Files the importer reads besides the source (external glTF
    /// buffers), by relative name, sorted.
    pub extra_inputs: Vec<(String, CacheKey)>,
    /// The sidecar's `settings`, canonical JSON (`{}` for documents).
    pub settings: String,
    /// Dependency asset id → its cache key, sorted by id.
    pub dependencies: Vec<(Uuid, CacheKey)>,
    pub profile: String,
}

impl KeyInputs {
    pub fn key(&self) -> CacheKey {
        let mut h = blake3::Hasher::new();
        let mut field = |tag: &str, bytes: &[u8]| {
            h.update(&(tag.len() as u32).to_le_bytes());
            h.update(tag.as_bytes());
            h.update(&(bytes.len() as u64).to_le_bytes());
            h.update(bytes);
        };
        field("fynkel-cache", &CACHE_VERSION.to_le_bytes());
        field("intermediate", &crate::INTERMEDIATE_VERSION.to_le_bytes());
        field("container", &fynkel_cooked::VERSION.to_le_bytes());
        field("asset", self.asset.as_bytes());
        field("kind", self.kind.as_bytes());
        field("version", &self.version.to_le_bytes());
        field("source", self.source.as_bytes());
        for (name, key) in &self.extra_inputs {
            field("input", name.as_bytes());
            field("input-hash", key.as_bytes());
        }
        field("settings", self.settings.as_bytes());
        for (id, key) in &self.dependencies {
            field("dependency", id.as_bytes());
            field("dependency-key", key.as_bytes());
        }
        field("profile", self.profile.as_bytes());
        CacheKey(*h.finalize().as_bytes())
    }
}

/// What was cooked under a key: enough to answer "why does this exist"
/// and "what is in it" without opening the container.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub key: String,
    /// The asset id, as text.
    pub id: String,
    /// Relative to the assets root.
    pub path: String,
    pub name: String,
    pub kind: String,
    pub version: u32,
    pub profile: String,
    pub source_hash: String,
    pub extra_inputs: Vec<ManifestInput>,
    pub settings: String,
    pub dependencies: Vec<ManifestDependency>,
    pub chunks: Vec<ManifestChunk>,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ManifestInput {
    pub name: String,
    pub hash: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ManifestDependency {
    pub id: String,
    pub key: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ManifestChunk {
    pub kind: String,
    pub name: String,
    pub length: u64,
    pub crc32: u32,
}

/// The on-disk cache: `<root>/objects/<key>.fyna` + `<key>.manifest.json`,
/// `<root>/index.json`.
#[derive(Clone, Debug)]
pub struct Cache {
    root: PathBuf,
}

impl Cache {
    pub fn at(root: &Path) -> Cache {
        Cache {
            root: root.to_path_buf(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn objects_dir(&self) -> PathBuf {
        self.root.join("objects")
    }

    pub fn object_path(&self, key: &CacheKey) -> PathBuf {
        self.objects_dir().join(format!("{}.fyna", key.hex()))
    }

    pub fn manifest_path(&self, key: &CacheKey) -> PathBuf {
        self.objects_dir()
            .join(format!("{}.manifest.json", key.hex()))
    }

    pub fn index_path(&self) -> PathBuf {
        self.root.join("index.json")
    }

    pub fn stamps_path(&self) -> PathBuf {
        self.root.join("stamps.json")
    }

    /// True when both the object and its manifest exist.
    pub fn has(&self, key: &CacheKey) -> bool {
        self.object_path(key).is_file() && self.manifest_path(key).is_file()
    }

    pub fn read_manifest(&self, key: &CacheKey) -> Result<Manifest, Diagnostic> {
        let path = self.manifest_path(key);
        let text = std::fs::read_to_string(&path).map_err(|e| io_error(&path, e))?;
        serde_json::from_str(&text).map_err(|e| {
            Diagnostic::error(Code::IMPORT_UNREADABLE, format!("manifest: {e}"))
                .with_document(path.display().to_string())
        })
    }

    /// Writes the object and its manifest; each lands whole or not at all
    /// (written to a temporary name, then renamed), the object first.
    pub fn write(
        &self,
        key: &CacheKey,
        object: &[u8],
        manifest: &Manifest,
    ) -> Result<(), Diagnostic> {
        let dir = self.objects_dir();
        std::fs::create_dir_all(&dir).map_err(|e| io_error(&dir, e))?;
        write_atomic(&self.object_path(key), object)?;
        let text = serde_json::to_string_pretty(manifest).expect("manifest serializes");
        write_atomic(&self.manifest_path(key), text.as_bytes())
    }

    /// Deletes every object whose key is not in `keep`. Returns how many
    /// objects were removed.
    pub fn prune(&self, keep: &[CacheKey]) -> Result<usize, Diagnostic> {
        let dir = self.objects_dir();
        if !dir.is_dir() {
            return Ok(0);
        }
        let keep: std::collections::HashSet<String> = keep.iter().map(CacheKey::hex).collect();
        let mut removed = 0;
        for entry in std::fs::read_dir(&dir).map_err(|e| io_error(&dir, e))? {
            let path = entry.map_err(|e| io_error(&dir, e))?.path();
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            let Some(stem) = name
                .strip_suffix(".fyna")
                .or_else(|| name.strip_suffix(".manifest.json"))
            else {
                continue;
            };
            if !keep.contains(stem) {
                std::fs::remove_file(&path).map_err(|e| io_error(&path, e))?;
                if name.ends_with(".fyna") {
                    removed += 1;
                }
            }
        }
        Ok(removed)
    }
}

/// Each source file's hash, trusted while the file's size and
/// modification time are what they were when it was hashed — git's index
/// and every build system's answer to "did this change?". The hash, not
/// the stamp, is what the key covers, so a file touched but not changed
/// still keys the same; a file changed without its size or time moving
/// (a deliberate `touch -t`, a clock set back) is missed until
/// `cook --force`, which re-hashes everything. A file modified within
/// [`Stamps::RACY`] of the cook is hashed but not stamped: its time could
/// still match a write that lands after the hash.
#[derive(Debug, Default)]
pub struct Stamps {
    files: std::collections::HashMap<String, Stamp>,
    /// Paths asked for this cook; [`Stamps::save`] keeps only these.
    seen: std::collections::HashSet<String>,
    changed: bool,
    /// Ignore the stamps (re-hash everything) but still record them.
    rehash: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct Stamp {
    size: u64,
    /// Nanoseconds since the Unix epoch.
    mtime: u64,
    /// [`CacheKey::hex`].
    hash: String,
    /// What the caller derived from the bytes along with the hash (the
    /// extra files a `.gltf` or `.fbx` names), kept so a stamped file
    /// need not be read to answer it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    derived: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct StampsFile {
    version: u32,
    files: std::collections::BTreeMap<String, Stamp>,
}

impl Stamps {
    const VERSION: u32 = 1;
    pub const RACY: std::time::Duration = std::time::Duration::from_secs(2);

    /// The cache's stamps; none when the file is missing, unreadable or
    /// of another version (everything is hashed, as before stamps).
    /// `rehash` trusts none of them.
    pub fn load(cache: &Cache, rehash: bool) -> Stamps {
        let files = std::fs::read(cache.stamps_path())
            .ok()
            .and_then(|b| serde_json::from_slice::<StampsFile>(&b).ok())
            .filter(|f| f.version == Self::VERSION)
            .map(|f| f.files.into_iter().collect())
            .unwrap_or_default();
        Stamps {
            files,
            rehash,
            ..Default::default()
        }
    }

    /// The hash of the file at `path` and what `derive` made of its
    /// bytes — from the stamp when the file's size and time still match
    /// it, else by reading the file (and dropping the bytes).
    pub fn hash(
        &mut self,
        path: &Path,
        derive: impl FnOnce(&[u8]) -> Vec<String>,
    ) -> std::io::Result<(CacheKey, Vec<String>)> {
        let name = path.to_string_lossy().into_owned();
        let md = std::fs::metadata(path)?;
        let mtime = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
        self.seen.insert(name.clone());
        if let (Some(stamp), Some(mtime), false) = (self.files.get(&name), mtime, self.rehash) {
            if stamp.size == md.len() && u128::from(stamp.mtime) == mtime.as_nanos() {
                if let Some(hash) = CacheKey::parse(&stamp.hash) {
                    return Ok((hash, stamp.derived.clone()));
                }
            }
        }
        let bytes = std::fs::read(path)?;
        let hash = hash_bytes(&bytes);
        let derived = derive(&bytes);
        let fresh = mtime.is_none_or(|m| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .is_ok_and(|now| now.saturating_sub(m) < Self::RACY)
        });
        // Stamped only when the size read is the size hashed (the file did
        // not grow between the two) and the time is settled.
        if !fresh && bytes.len() as u64 == md.len() {
            let stamp = Stamp {
                size: md.len(),
                mtime: mtime.map_or(0, |m| m.as_nanos() as u64),
                hash: hash.hex(),
                derived: derived.clone(),
            };
            if self.files.get(&name) != Some(&stamp) {
                self.files.insert(name, stamp);
                self.changed = true;
            }
        } else if self.files.remove(&name).is_some() {
            self.changed = true;
        }
        Ok((hash, derived))
    }

    /// Writes the stamps of the files asked for this cook (a file no
    /// longer in the project drops out); nothing when nothing changed.
    pub fn save(mut self, cache: &Cache) -> Result<(), Diagnostic> {
        let before = self.files.len();
        let seen = std::mem::take(&mut self.seen);
        self.files.retain(|k, _| seen.contains(k));
        if !self.changed && self.files.len() == before {
            return Ok(());
        }
        std::fs::create_dir_all(&cache.root).map_err(|e| io_error(&cache.root, e))?;
        let file = StampsFile {
            version: Self::VERSION,
            files: self.files.into_iter().collect(),
        };
        let text = serde_json::to_vec(&file).expect("stamps serialize");
        write_atomic(&cache.stamps_path(), &text)
    }
}

fn io_error(path: &Path, e: std::io::Error) -> Diagnostic {
    Diagnostic::error(Code::IO, format!("{e}")).with_document(path.display().to_string())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), Diagnostic> {
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, bytes).map_err(|e| io_error(&tmp, e))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io_error(path, e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> KeyInputs {
        KeyInputs {
            asset: Uuid::nil(),
            kind: "fynkel.image".into(),
            version: 1,
            source: hash_bytes(b"png"),
            extra_inputs: Vec::new(),
            settings: "{}".into(),
            dependencies: Vec::new(),
            profile: "dev".into(),
        }
    }

    #[test]
    fn stamps_skip_unchanged_files_and_catch_changed_ones() {
        let dir = std::env::temp_dir().join(format!("fynkel-stamps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cache = Cache::at(&dir.join("cache"));
        let file = dir.join("a.bin");
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
        let write = |bytes: &[u8], at: std::time::SystemTime| {
            std::fs::write(&file, bytes).unwrap();
            std::fs::File::options()
                .write(true)
                .open(&file)
                .unwrap()
                .set_modified(at)
                .unwrap();
        };
        let derive = |b: &[u8]| vec![format!("{}", b.len())];

        write(b"one", old);
        let mut stamps = Stamps::load(&cache, false);
        assert_eq!(
            stamps.hash(&file, derive).unwrap(),
            (hash_bytes(b"one"), vec!["3".to_string()])
        );
        stamps.save(&cache).unwrap();
        assert!(cache.stamps_path().is_file());

        // Same size and time: the stamp answers, the file is not read (a
        // same-size edit that kept the time is the documented miss).
        write(b"two", old);
        let mut stamps = Stamps::load(&cache, false);
        assert_eq!(
            stamps.hash(&file, |_| panic!("read")).unwrap().0,
            hash_bytes(b"one")
        );
        // `cook --force` trusts no stamp.
        let mut forced = Stamps::load(&cache, true);
        assert_eq!(forced.hash(&file, derive).unwrap().0, hash_bytes(b"two"));
        // A new time is a new hash, and it is stamped.
        write(b"two", old + std::time::Duration::from_secs(1));
        assert_eq!(stamps.hash(&file, derive).unwrap().0, hash_bytes(b"two"));
        stamps.save(&cache).unwrap();
        let mut stamps = Stamps::load(&cache, false);
        assert_eq!(
            stamps.hash(&file, |_| panic!("read")).unwrap().0,
            hash_bytes(b"two")
        );

        // A file written just now is hashed every time until it settles.
        write(b"three", std::time::SystemTime::now());
        assert_eq!(stamps.hash(&file, derive).unwrap().0, hash_bytes(b"three"));
        write(b"four!", std::time::SystemTime::now());
        assert_eq!(stamps.hash(&file, derive).unwrap().0, hash_bytes(b"four!"));

        // A file not asked for in a cook drops out of the saved stamps.
        stamps.save(&cache).unwrap();
        Stamps::load(&cache, false).save(&cache).unwrap();
        let text = std::fs::read_to_string(cache.stamps_path()).unwrap();
        assert!(!text.contains("a.bin"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_input_changes_the_key() {
        let base = inputs().key();
        assert_eq!(inputs().key(), base);
        let mut i = inputs();
        i.asset = Uuid::from_bytes([1; 16]);
        assert_ne!(i.key(), base);
        let mut i = inputs();
        i.version = 2;
        assert_ne!(i.key(), base);
        let mut i = inputs();
        i.source = hash_bytes(b"png2");
        assert_ne!(i.key(), base);
        let mut i = inputs();
        i.settings = r#"{"colorSpace":"linear"}"#.into();
        assert_ne!(i.key(), base);
        let mut i = inputs();
        i.profile = "ios".into();
        assert_ne!(i.key(), base);
        let mut i = inputs();
        i.dependencies.push((Uuid::nil(), base));
        assert_ne!(i.key(), base);
        let mut i = inputs();
        i.extra_inputs.push(("robot.bin".into(), hash_bytes(b"x")));
        assert_ne!(i.key(), base);
        // Field boundaries are explicit: moving bytes between fields differs.
        let mut a = inputs();
        a.kind = "ab".into();
        a.settings = "c".into();
        let mut b = inputs();
        b.kind = "a".into();
        b.settings = "bc".into();
        assert_ne!(a.key(), b.key());
    }

    #[test]
    fn keys_round_trip_through_hex() {
        let k = inputs().key();
        assert_eq!(k.hex().len(), 64);
        assert_eq!(CacheKey::parse(&k.hex()), Some(k));
        assert_eq!(CacheKey::parse("zz"), None);
    }
}
