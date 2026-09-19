# Engine status — 2026-09-19 (after M2-15)

The one-page, plain-words answer to "what do we have, is it solid, what is
missing". Updated at every milestone gate and whenever a row below changes.
Numbers come from the gate reports (`m0-architecture-report.md`,
`m1-gate-report.md`); nothing here is estimated.

## 1. What exists

| Layer | What it is | State | Proof |
| --- | --- | --- | --- |
| `fynkel_contracts` | Shared types: ids, schema metadata, `AssetRef`/`Uuid`, diagnostics, wire header | **Solid** | unit tests; every other crate builds on it |
| `fynkel_ecs` + `fynkel_scene` | `hecs` behind a thin facade (ADR-0003); transforms, hierarchy, propagation | **Solid** | unit tests; deterministic checksum in the headless host |
| `fynkel_core` | The world: command batches in, snapshot/events/diagnostics out, fixed §5.1 step order, `load_scene` | **Solid** | 3 000-tick deterministic checksum, 25 tests, headless host |
| `fynkel_wire` + `fynkel_schema_derive` + `fynkel codegen` | Components written once in Rust → JSON Schema, Dart codecs, Rust codecs, fingerprint `0xa3c3bd56d1793195` | **Solid** — the strongest piece | `codegen --check` in CI; wire fixtures shared by Rust and Dart |
| `fynkel_native` + `package:fynkel` | C ABI v1, `ffigen` bindings, Dart SDK (`World`, `GameObject`, transform writers, `loadScene`/`loadProject` with a `LoadReport`, `Behaviour` with the §3.2 lifecycle driven by `World.frame` at the §5.1 points, `GameContext` services with installable test contexts, `BehaviourRegistry` for data-built behaviours), native-assets build hook | **Solid** | 1 000 entities/tick: 191 µs encode+submit, 169 µs step, 47 µs read; 1.5 KB/frame Dart allocation on the 25-entity scene, 0 GC in 3 000 frames |
| `fynkel_render` | wgpu renderer over the extract: `MeshRenderer` entities drawn with their cooked mesh and the `baseColor` factor + texture of their `fynkel.pbr@1` material (read from the `fynkel cook` cache through `CookedAssets`), the scene's `Camera` and first `DirectionalLight`, placeholder cubes for entities without a mesh (optional), magenta for a mesh missing from the cache; one 72-byte vertex layout shared with the cooker | **Seed → V1 slice** — one pass, Lambert + Blinn-Phong, no shadows, no point lights, no normal/metal-rough maps, no IBL | pixel-readback tests incl. the fixture project cooked and rendered from its own camera (texture provably sampled); runs in every host; `fynkel render` writes it to a PNG |
| `platform/apple` + `hosts/flutter_apple` | IOSurface ring → Flutter `Texture` (Path A), 0 copies, GPU-completion hand-off; macOS + iOS; `FYNKEL_PROJECT=<dir>` loads a project's startup scene and its cooked cache instead of the Dart demo | **Solid** | 120 Hz, lag 0, raster p95 0.95 ms; iPhone 16 Pro Max: GPU 1.84 ms, no drops |
| `hosts/headless` + `hosts/standalone` | Rust hosts: deterministic runs, `--scene`, state dump; winit window | **Solid** | CLI tests |
| `fynkel_document` | Strict JSON with spans, envelope, JSON Schema 2020-12, §14.2 diagnostics; scene, prefab, material, shader documents and `.meta.json` sidecars; prefab expansion; asset database (scan, resolve, reference check); migrations with diff + `.bak`; scene → runtime; project file + lock and the whole-project validation; authoring commands and transactions (`Editor`: revision = blake3 of the canonical text, 13 §11.1 commands + `unsetField` compiled to reversible edits, stale base revision → conflict, a transaction that would leave errors is rolled back to the byte, byte-exact undo/redo) | **Solid** | fixture corpora with exact code/line/column expectations (valid, unknown, invalid, scene, prefab, material, assets incl. the move test, migrate, project incl. its committed lock); `tests/authoring.rs` undoes every command back to the original text |
| `fynkel_import` | glTF/GLB, PNG/JPEG, WAV/Ogg Vorbis → normalized intermediate (`FYNI`): meshes in a fixed vertex layout, `fynkel.pbr@1` materials, RGBA8 textures with a declared colour space, f32 PCM clips, node hierarchy as a prefab; sub-asset ids from the sidecar or derived; external files denied by default; limits | **Solid for static meshes, 8-bit textures and PCM/Vorbis clips** — no skins, morphs, animations, Draco, HDR, FLAC/MP3 | fixture corpora (`robot.glb` and variants, every PNG layout, 8/16/24-bit WAV, Vorbis), committed expected bytes (determinism), produced documents validated by the document layer, CLI test |
| `fynkel_import::cache` + `::cook` + `fynkel_cooked` (+ `::payload`) | Content-addressed cache (blake3 keys over source, settings, importer/format versions, dependency keys, profile; `objects/<key>.fyna` + manifest, `index.json`, prune) and the cooker (dependency order, documents as canonical JSON, sources as typed `MESH`/`TEXR`/`CLIP`/`JSON`/`BLOB` chunks); `FYNC` v1 container with CRC-checked header, chunk table and bounded reads, dependency-free reader | **Solid for what is imported** — no mipmaps, compression or per-profile transforms yet (item 16); `MESH`/`TEXR`/`JSON` are consumed by the renderer, `CLIP` by nothing yet; no GC beyond `--prune` | `tests/cook.rs`: editing a texture's pixels or sidecar recooks exactly its material and scene; profile in the key; read-back with bounded decoding and a flipped byte caught; cycles, unresolved and blocked assets; CLI test |
| `apps/cli` | `fynkel codegen`, `schema`, `test --headless`, `state dump`, `document`, `scene`, `prefab`, `material`, `shader`, `asset`, `migrate`, `import`, `cook`, `cooked inspect`, `project describe|lock`, `validate`, `schema list|describe`, `scene|prefab apply --patch` — all with `--json`, exit codes 0/1/2 fixed | **Solid** | CLI tests |

Size: ≈23 k lines (Rust 17.4 k, Dart 5.1 k, Swift/ObjC 0.9 k) over 31 commits.

## 2. What is proven end to end

- **Dart → Rust → GPU → Flutter, every frame, zero copies.** A Flutter app on
  macOS and on an iPhone drives `step()` from the frame callback and shows the
  world in a `Texture` with widgets composited over it. (M0 + M1 gates.)
- **File → world.** A `.scene.json` is parsed, validated with positioned
  diagnostics, loaded into the world (hierarchy, child order, components),
  and dumped back by authoring id (`fynkel state dump`). (M2-05.)
- **One source of truth for components.** Adding a Rust field regenerates the
  schema, the Dart codec and the Rust codec; drift fails the build. (M1.)
- **File → screen.** The fixture project (`fynkel.project.json`, a scene
  with a `Camera`, a `MeshRenderer` over `robot.glb` + `rusted.material.json`
  with the `swatch.png` base colour texture, and a prefab instance) is cooked
  by `fynkel cook`, loaded by `World.loadProject` and shown in the Flutter
  macOS host from the scene's own camera — 4 entities, 2 cooked meshes, 0
  missing, 120 Hz — and by `fynkel render` to a PNG headlessly. The same path
  is a pixel test in `fynkel_render/tests/project.rs`. (Spike after M2-13,
  M3's first item pulled forward.)

## 3. What is *not* proven yet (honest list)

| Gap | Why it matters | Owner |
| --- | --- | --- |
| **The picture is a slice, not the renderer.** One opaque pass: base colour factor × texture, one directional light, no shadows, no point lights, no normal/metallic-roughness/emissive maps, no IBL (metals keep 40 % diffuse so they are visible at all), no exposure control (10 000 lux = white), no transparency, no sorting beyond (mesh, texture) batches, no resource eviction (assets stay on the GPU until `clear_assets`). | Materials look flat; a scene with only a `PointLight` is lit by the default light. | M3 (renderer V1) |
| **Scenes load from source files, not from the cache.** `load_project` reads `.scene.json`/`.prefab.json` from the asset root and expands prefabs; the cooked scene chunk is unexpanded canonical JSON nobody reads. The host does not cook: `fynkel cook` must have run (the HUD says so when the cache is missing). | A shipped game would still need its source documents. | M3 (cooked scene = expanded, loader reads only the cache) |
| **Audio is cooked, not heard.** `CLIP` chunks have no consumer. | Nothing can be heard. | M3 |
| **No input contract** (§5.1 held-state + edge). | Games cannot be played yet. | M3 (C2) |
| **Authoring commands exist in Rust only; behaviours are code-attached only.** `fynkel_document::authoring` applies and undoes every §11.1 command (M2-14) and `fynkel scene apply` drives it from a patch file, but no C ABI or Dart surface exposes an `Editor` yet. Behaviours (M2-15) run with the full lifecycle, but nothing registers them from `@FynkelBehaviour` classes or instantiates them from scene components yet — the registry is filled by hand until the generator (M2-16); `Input` waits for the M3 contract. The project file exists (M2-13) and `fynkel validate` checks a whole project, but nothing reads `inputActions`, `quality` or `targets` yet. Migrations exist (M2-09) with one synthetic step — no real format change has been through them. | The editor (M4) has the commands but no handle on them; a scene cannot yet name a Dart behaviour. | M2-16 (editor ABI: M4; project consumers, input: M3) |
| **Everything runs on the Flutter UI thread** (step, extract, render). | Fine at current sizes; ADR-0007 allows a render thread later, so extraction must stay cheap. | M5 |
| **Single machine, single profile.** All numbers are from one M3 Max and one iPhone; no second display / DPI change exercised. | Regressions elsewhere would not be seen. | first release |

## 4. Compromises we knowingly made

Not bugs — things done the simple way with a note to revisit.

- `material.schema.json`, `shader.schema.json`, `meta.schema.json`,
  `project.schema.json` and `lock.schema.json` are **hand-written**; every
  component schema is generated from Rust. Acceptable while these models are
  only validated; revisit when the renderer needs a Rust `Material` type
  (M3), which should then become the source of truth.
- Schema-vs-semantic diagnostic de-duplication works by **matching JSON
  pointers** (`scene.rs`, `material.rs`). Simple, but a message can vanish if
  one side changes the pointer it anchors to. The fixture corpora catch that.
- Document numbers are `f64` all the way; the runtime is `f32`. Narrowing
  happens in the generated codecs without a diagnostic for precision loss.
- `Document::new` embeds a minimal body per kind (empty `entities`, PBR
  shader) instead of reading defaults from the schema.
- Dart tests run through Flutter's `dart` only (`hooks`/`code_assets` are
  still experimental on stable Dart). Pinned; fallback documented in the M1
  gate report.
- The glTF importer writes zero tangents when the file has none and keeps
  only `wrapS` of a sampler (Fynkel samplers wrap both axes alike); tangent
  generation and per-axis wrap are M3 renderer decisions.
- The cache has no garbage collection beyond `fynkel cook --prune` (which
  keeps one profile's objects); stale objects accumulate until then, and
  `rm -rf .fynkel/cache` is always safe. Cooked textures are RGBA8 with one
  mip level for every profile — profile-specific transforms are item 16.
- The renderer's `MaterialDesc` is a handful of `fynkel.pbr@1` fields read
  straight from the cooked material JSON (`fynkel_render::assets`) — not the
  document layer's `Material`/`ShaderManifest`, which stay validation-only.
  Fine for the slice; the Rust material type (above) should replace it.
- `fynkel_world_load_scene` looks for prefabs in the scene file's directory
  and below; `fynkel_world_load_project` has the whole asset database. A
  scene whose prefab lives elsewhere loads only through the project.
- `FYNKEL_STATUS_LOAD_FAILED` (8) and the Apple viewport ABI v2 (asset cache,
  placeholders, mesh counts in the stats) were added without a deprecation
  window; both ABIs are pre-release.
- ADR-0005 (license) is still open; nothing is public yet.

## 5. Verification (what "green" means)

Run from a clean clone — the same steps as `.github/workflows/ci.yml`:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p fynkel_cli -- codegen --check
cd packages/fynkel && dart pub get && dart analyze --fatal-infos && dart test   # Flutter's dart
```

Last clean-clone run: **2026-09-19, commit `fca5d30`** (re-run after M2-15: 185 Rust tests, 49 Dart tests; `platform/apple` `presenter_wait_yields_lag_zero` is load-sensitive — it fails while another build or a simulator app is using the GPU and passes 5/5 on a quiet machine) — fmt clean, clippy clean, 92 Rust tests passed, codegen up to date, `dart analyze` clean, 37 Dart tests passed (Flutter 3.47.2 / Dart 3.13.2, Rust 1.93.0, M3 Max).

## 6. Verdict and next steps

The foundation is the right shape: contracts and data first, one source of
truth for schemas, measured bridge and viewport, deterministic core. Nothing
built so far needs to be torn down.

Documents and the renderer have now met: a scene file with a material and a
cooked mesh shows up lit on the screen, from its own camera, in the Flutter
host. The loop "author a file, cook, see it" is real, if plain-looking. What
remains on the authoring side (commands, behaviours, template) now builds on
a path that is proven all the way through.

Order of work:

1. **Finish M2 lean** (items 16–18:
   behaviour generator, template, gate). Each is a dependency of
   the editor; none should grow beyond its blueprint line. The template
   project (item 17) should render through the same path as the fixture.
2. **M2 gate (item 18)** — the built-in stop-and-look moment; this page gets
   rewritten there.
3. **M3 grows the slice into renderer V1**: materials beyond base colour,
   lights, shadows, cooked-scene loading, audio — each measured against the
   picture the spike already produces.
