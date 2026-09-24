# Editor and cook scale (roadmap Phase 1 item 3)

A generated 10k-asset project, to measure what the editor's session and
the cook cost as a project grows.

```sh
python3 benchmarks/scale/generate.py            # → /tmp/fynkel-scale
cargo build --release -p fynkel_authoring --example scale
/usr/bin/time -l target/release/examples/scale /tmp/fynkel-scale session
/usr/bin/time -l target/release/examples/scale /tmp/fynkel-scale cook
target/release/examples/scale /tmp/fynkel-scale cook-warm
```

The project: 6 000 textures (64×64 PNG with sidecars), 3 000 materials
(two texture references each), 800 prefabs, 200 scenes of 20 prefab
instances, and one model. That makes 10 001 assets, 11 600 references and
135 MB. `fynkel validate` reports it clean.

## Results — 2026-09-24, Apple M3 Max, release build

The file cache was warm: the first `open` after generating takes
~850 ms, and every later one ~380 ms.

| | before | after |
|---|---:|---:|
| open (scan + reference index) | 362 ms | 385 ms |
| rescan one material | 29.3 ms | 2.0 ms |
| rescan one texture | 29.6 ms | 1.9 ms |
| rescan the materials folder (3 000 files) | 167.6 ms | 108–113 ms |
| cook, cold (10 001 cooked) | 5.1 s | 5.4–6.4 s |
| cook, nothing to do | 1 052 ms | 800–930 ms |
| cook peak RSS | 238 MB | 155 MB |

The cold cook writes 20 000 files. Its time is disk time, and it varies
by a second from run to run, so the before and after columns are the
same within that noise.

What changed:

- **The session.** `AssetDatabase::by_path` is a map; it used to walk
  every entry. So the editor's reference prune after a rescan
  (`OpenProject::rescan`) is linear in references rather than references
  × entries. The paths a rescan re-indexes, and the ones `update` drops,
  are sets instead of lists. A folder rescan still reads each of its
  files twice, once for the database and once for the reference index.
- **The cook.** A source's bytes are no longer held from `prepare` to
  the end of the cook. The importer reads the file when the asset must
  cook, one asset at a time; the plain-file importer re-reads it and
  checks it against the key's hash. `.fynkel/cache/stamps.json` stores
  each source's size, modification time and hash, plus the extra files a
  `.gltf`/`.fbx` names. A source whose size and time match its stamp is
  not read again (`Stamps`, `fynkel_import/src/cache.rs`). `cook --force`
  trusts no stamp. A cached texture or sound is no longer reopened to look
  for a prefab chunk.
- **What is left** of the ~850 ms no-op cook: about 460 ms goes to
  parsing and schema-validating every sidecar and document (`prepare`),
  and most of the rest to reading 10 001 manifests. Neither is I/O the
  stamps can skip. They are the next thing to cache if a larger project
  needs it.
