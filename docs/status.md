# Engine status — 2026-09-20 (M2 gate: GO; M3 in progress, items 1–5 done)

The one-page, plain-words answer to "what do we have, is it solid, what is
missing". Rewritten at every milestone gate and updated whenever a row below
changes. Numbers come from the gate reports (`m0-architecture-report.md`,
`m1-gate-report.md`, `m2-gate-report.md`) and the captures under
`benchmarks/`; nothing here is estimated.

## 1. What exists

| Layer | What it is | State | Proof |
| --- | --- | --- | --- |
| `fynkel_contracts` | Shared types: ids, schema metadata, `AssetRef`/`Uuid`, the §14.2 diagnostic record and code namespaces, wire header | **Solid** | unit tests; every other crate builds on it |
| `fynkel_ecs` + `fynkel_scene` | `hecs` behind a thin facade (ADR-0003); transforms, hierarchy, propagation | **Solid** | unit tests; deterministic checksum in the headless host; `hecs` provably hidden |
| `fynkel_core` | The world: command batches in, snapshot/events/diagnostics out, fixed §5.1 step order, `load_scene` from a typed description | **Solid** | 3 000-tick deterministic checksum (step p50 2 µs at 25 entities), 25 tests, headless host |
| `fynkel_wire` + `fynkel_schema_derive` + `fynkel codegen` (a) | Components and document records written once in Rust → JSON Schema (components, `material.schema.json`), Dart codecs, Rust codecs, fingerprint `0x408b089693b6c6b6` | **Solid** — the strongest piece | `codegen --check` in CI; wire fixtures shared by Rust and Dart |
| `fynkel_native` + `package:fynkel` | C ABI v1, `ffigen` bindings, the Dart SDK: `World`, `GameObject`, allocation-free `TransformWriter`, `loadProject(dir)`/`loadScene(cacheDir, sceneId)` from the cooked cache only → `LoadReport` (scene key, residency), `package:fynkel/testing.dart` (`cookProject`), `Behaviour` with the §3.2 lifecycle driven by `World.frame` at the §5.1 points, `GameContext` services with installable test contexts, `BehaviourRegistry`; native-assets build hook | **Solid** | 1 000 entities/tick: 191 µs encode+submit, 169 µs step, 47 µs read; 5.1 KB/frame Dart allocation on the 25-entity scene (0.7–1.2 KB of it SDK), 2 GCs in 3 000 frames; leak tests at the ABI and viewport |
| `package:fynkel_generator` + `fynkel codegen` (b) | `build_runner` aggregate builder: `@FynkelBehaviour`/`@Expose` classes → `behaviours.g.dart` (factories, field get/set, `registerBehaviours`), `behaviours.g.json` (Inspector metadata) and `schemas/behaviours/<id>.schema.json`; `fynkel codegen --project [--check]` runs it and splits the schemas | **Solid** | authored `game.Spinner { speed: 2.5 }` → validated → instantiated → rotates (M2 gate); emit tests; `--check` in CI for the example and the starter project; 1 s warm |
| `fynkel_document` | Strict JSON with spans, envelope, JSON Schema 2020-12, canonical writer that preserves unknown data; scene, prefab, material (formatVersion 2: the `PbrMaterial` fields flat on the document, custom shaders through their manifest), shader, `.meta.json`, project and lock documents; prefab expansion (UUID v5 ids, overrides, nested refused); asset database (scan, resolve, reference check); migrations with diff + `.bak` (`material/1-2/pbr-fields` is the first real step); scene → runtime description (`describe`, project-aware; the cooker's input); whole-project validation; authoring commands and transactions (`Editor`: revision = blake3, 13 §11.1 commands + `unsetField` as reversible edits, conflicts, rollback to the byte, byte-exact undo/redo) | **Solid** | 55 integration tests over fixture corpora with 30 exact code/pointer/line/column expectations; every command undone to the original text; validate/load ≈20–25 µs per entity, linear to 10 000 entities |
| `fynkel_import` | glTF/GLB, PNG/JPEG, WAV/Ogg Vorbis → deterministic `FYNI` intermediates: meshes in the one 72-byte vertex layout, `fynkel.pbr@1` materials in the model's document form with all five texture slots, RGBA8 textures with a declared colour space (a glTF image used as both colour and data is imported twice), f32 PCM clips, node hierarchy as a prefab; sub-asset ids from the sidecar or derived; external files denied by default; limits | **Solid for static meshes, 8-bit textures and PCM/Vorbis clips** — no skins, morphs, animations, Draco, HDR, FLAC/MP3 | 37 tests; 3 committed expected intermediates; a 2 MiB / 79 k-triangle GLB imports in 16 ms |
| `fynkel_import::cache` + `::cook` + `fynkel_cooked` | Content-addressed cache (blake3 over source, settings, importer/format versions, dependency keys, profile; `objects/<key>.fyna`, `index.json` with the project's name, startup scene and the build's fingerprint, `--prune`) and the cooker (`cook_project`: every scene validated and expanded — prefabs applied, ids derived — into its object's `JSON` chunk, a cooked chunk per prefab); `FYNC` v1 container with CRC-checked header and chunk table, bounded reads, dependency-free reader (`MESH`/`TEXR`/`CLIP`/`JSON`/`MATL`/`BLOB`; `MATL` is the `PbrMaterial` wire record the renderer consumes) | **Solid for what is imported** — no mipmaps, compression or per-profile transforms; `CLIP` has no consumer; no GC beyond `--prune` | editing a texture re-cooks exactly its dependents (4 of 6 on the fixture); cook miss 20 ms / hit 12 ms / partial 13 ms; flipped byte caught on read-back |
| `fynkel_assets` | The runtime store over the cache: `Assets::open` refuses a cache from another build, `scene(id)` → the expanded description + behaviours + asset closure, meshes/textures/materials by `AssetRef`, `retain(closure)` → `Residency` token, drop evicts what no scene holds; `FYN-R0001`–`R0009` name the asset id, never fall back to source | **Solid** | 4 store tests; the render gate renders from a cache whose source tree was deleted; ABI tests load a copied cache and refuse a tampered one |
| `fynkel_render` | wgpu renderer over the extract (`docs/renderer.md`): Cook-Torrance (GGX, Smith, Schlick) in physical units — every `fynkel.pbr@1` factor and map (base colour, normal via cook-generated tangents, metallic-roughness, occlusion, emissive), `alpha_mode: mask`, double-sided — into a linear HDR `Rgba16Float` target exposed by the camera's EV100 (`Camera.exposure`, or metered from the sun) and ACES tone mapped; one sun, constant ambient + optional equirectangular sky (`fynkel.Environment`), the 8 nearest point/spot lights (`fynkel.SpotLight`), perspective or orthographic camera; the `shadow → opaque → transparent → post → presentation` chain: the sun's 3×3 PCF shadow from one texel-snapped 2048² map (`cast_shadows`/`receive_shadows` per mesh), `blend` materials back to front in the transparent stage, MSAA 1/4 resolved through the post pass's coverage, render scale with a presentation pass, the project's `quality` profiles read from the cache and switched at runtime (renderer, Apple ABI, `FynkelViewportController.quality`, `fynkel render --quality`), debug lines/AABBs; one instance buffer per frame with one draw per distinct `(mesh, material)` pair, every instance frustum-culled against the camera (and, for casters, the sun's box) on its cooked bounding sphere before upload, `draws`/`instances`/`culled` counted; nine fixed pipelines, bind groups cached per material/texture, evicted with the store; placeholder cubes, magenta for a missing mesh | **V1 (c)** — one cascade to 60 m, no IBL beyond constant ambient, no light culling, no mipmaps (anisotropy sharpens the base level only); normals skew under non-uniform scale | 19 per-feature pixel tests (shadow, per-mesh opt-outs, blend order, MSAA edges, render scale, profile switching, debug lines, culling among them) + the fixture project (rusted robot with swatch, normal and metallic-roughness maps under its `Sun`, `low`/`medium`/`high` profiles) rendered from its own camera from the cache alone; exposure, shadow fit, quality clamp, culling and debug unit tests; `fynkel render` writes a PNG with `ev100`/light/shadow/draw/culled counts; runs in every host; M0 capture `2026-09-20-12859e1` unchanged at MSAA 4 + shadow pass (122/120 fps, engine CPU ≈ 0.1 ms); stress capture `benchmarks/m3/results/2026-09-20-cf2b3eb`: 5 804 entities, 5 343 instances in 15 draws, 458 culled, 122 fps standalone / 120 Flutter |
| `platform/apple` + `package:fynkel_flutter` + `hosts/flutter_apple` | IOSurface ring → Flutter `Texture` (Path A), 0 copies, GPU-completion hand-off; macOS + iOS. `package:fynkel_flutter` is the Flutter plugin a game depends on: Swift `FynkelFlutterPlugin` (texture registration, multi-viewport by `textureId`, ordered teardown), `FynkelViewportController` (create/resize/rebind, adopts the world's asset store, placeholders, camera, stats), `FynkelViewport` widget (layout × DPR, focus, pointer in physical px) and `FynkelGameView` (Ticker loop: `World.frame` then render, lifecycle mute, per-second `FynkelFrameStats`, overlay). `hosts/flutter_apple` is the measurement harness as a thin app on it; `FYNKEL_PROJECT=<dir>` shows a project's startup scene from its cooked cache | **Solid** | 120 Hz, lag 0, raster p95 0.96 ms after the port (M0 capture `2026-09-19-b35f603`); iOS simulator 60 Hz, 0 drops; package tests create → render → resize → detach against the real Rust viewport under `flutter test`; 60-lifetime leak test |
| `hosts/headless` + `hosts/standalone` | Rust hosts: deterministic runs, cooked scenes from the cache (headless through `fynkel test --headless` `--project`/`--scene <path\|id>`/`--cache`; standalone `--cache <dir> [--scene <id>] [--quality <profile>]`, never cooking), state dump by authoring id; winit window with the renderer's counts on its stats line | **Solid** | CLI tests; the stress capture runs the standalone host on the cooked stress scene |
| `apps/cli` | `fynkel codegen [--project]`, `schema`, `test --headless`, `state dump`, `document`, `scene`, `prefab`, `material`, `shader`, `asset`, `migrate`, `import`, `cook`, `cooked inspect`, `project describe\|lock`, `validate`, `render`, `scene\|prefab apply --patch`, `create`, `run` — every command with `--json`, exit codes 0/1/2 fixed (ADR-0017); works outside the checkout on the embedded schemas; document commands know the enclosing project's behaviour types; `test --headless`, `state dump`, `render` and `run` cook first and load from the cache (`--no-cook`) | **Solid** | 16 CLI tests incl. a fresh `fynkel create` project through validate, headless, state dump and `codegen --check`; every command under 30 ms on the fixture project except `codegen --project` (1 s warm) |
| `examples/showcase` | The renderer's example scene: an ordinary created game authored to exercise the sun's shadow (with per-mesh opt-outs), a spot and two point lights, an `Environment` with a generated sky, textured PBR, gold/chrome, three overlapping blend panels, a mask leaf card, the fixture robot as a hierarchy, `game.Orbit` circling the camera, three quality profiles; the HUD prints the viewport's frame counts (blended, lights, casters/map, MSAA, render size, GPU/CPU ms — appended to `FynkelAppleViewportStats`); `renders/<item>.png` committed per renderer item | **Solid** | CI validates, runs headless, renders and `flutter test`s it; `renders/m3-05.png`; launched on macOS (34 entities, 27 meshes, 5 blended, 18 casters in a 2048² map, MSAA 4, GPU ≈ 2 ms at 1600×1200) |
| `fynkel create` template (`apps/cli/templates/create/`) | A new game: Flutter app (`flutter create --empty` runners, sandbox off) on `package:fynkel` + `package:fynkel_flutter`, `lib/main.dart` = `FynkelGameView` over the project's cooked cache with a HUD overlay, `lib/game/game.dart` (`Game.load`), `game.Spinner` with committed generated files and schema, cube/material/scene with fresh ids and real import sidecars, `test/game_test.dart`, `AGENTS.md`, `README.md`; `fynkel run` validates, cooks and runs it | **Solid** — the created app shows its lit, spinning cube (M3-01 gate) | CI creates one and runs `validate`, `test --headless`, `codegen --check`, `flutter analyze`, `flutter test`; built and launched on macOS by hand (1 mesh drawn, 60 ticks/s, 120 Hz) |

Size: ≈47 k lines (Rust 37.2 k, Dart + templates 8.8 k, Swift/ObjC 0.9 k,
Python 0.5 k) plus 5.5 k lines of docs, over 45 commits.

## 2. What is proven end to end

- **Dart → Rust → GPU → Flutter, every frame, zero copies.** A Flutter app on
  macOS and on an iPhone drives `step()` from the frame callback and shows the
  world in a `Texture` with widgets composited over it. (M0 + M1 gates.)
- **One source of truth for components.** Adding a Rust field regenerates the
  schema, the Dart codec and the Rust codec; drift fails the build. Adding an
  `@Expose` field to a behaviour regenerates its registration and schema the
  same way. (M1, M2-16.)
- **File → world → file.** A `.scene.json` is parsed, validated with
  positioned diagnostics, expanded, loaded into the world and dumped back by
  authoring id; unknown data survives a rewrite byte for byte; every §11.1
  authoring command applies as a transaction and undoes to the original
  bytes. (M2 gate.)
- **Source → cache → screen.** The fixture project (scene with a `Camera`, a
  `MeshRenderer` over `robot.glb` + `rusted.material.json` with the
  `swatch.png` base colour, a prefab instance with an override) is cooked by
  `fynkel cook` (the scene expanded, ids derived), loaded by
  `World.loadProject` from the cache alone and shown in the Flutter macOS
  host from the scene's own camera — 5 entities, 2 cooked meshes, 0 missing,
  120 Hz — and by `fynkel render` to a PNG headlessly; the same path is a
  pixel test that deletes the source tree after cooking and renders the
  same bytes. (Spike after M2-13; cache-only since M3-03.)
- **Authored data → compiled Dart behaviour.** A `@FynkelBehaviour` class
  becomes a schema; a scene's `game.Spinner { speed: 2.5 }` validates against
  it and comes back from `World.loadProject` as a `Spinner` with
  `speed == 2.5` that rotates its entity every tick. (M2-16.)
- **Empty folder → game project → green, on a clean CI runner.** `fynkel
  create my_game` → `validate` → `test --headless` → `codegen --check` →
  `flutter analyze` → `flutter test`. (M2-17.)
- **The M2 gate itself:** all seven criteria with the test that proves each
  (`m2-gate-report.md` §1), a clean-clone run of every suite, and a timing
  capture of every tool. (M2-18.)
- **A created project shows its scene in its own app.** `fynkel create` →
  `fynkel run` (validate, cook, `flutter run`): the starter's lit, spinning
  cube in `FynkelGameView` with the HUD over it on macOS; the same package
  runs the measurement host on macOS (numbers unchanged) and the iOS
  simulator. (M3-01.)

## 3. What is *not* proven yet (honest list)

| Gap | Why it matters | Owner |
| --- | --- | --- |
| **SDK packages come from the checkout.** `package:fynkel` and `package:fynkel_flutter` are path dependencies into the repo; the hook's `fynkel.lock.json` is a different shape from the project lock. | A game cannot be built without a Fynkel checkout and a Rust toolchain. | M3 item 17 / C6 (published artifacts) |
| **The renderer has no IBL and no light culling.** Environment lighting is a constant ambient term, so metals read dark (the showcase's gold and chrome blocks); every draw sees the 8 nearest lights to the camera, whatever it draws; one shadow cascade to 60 m; culling is per instance on the CPU (1.4 ms of prepare at 5 800 records — fine at 120 Hz, not free). | Metallic materials cannot look right; a scene with many small lights lights the wrong things. | IBL and per-object light lists after M3 |
| **No input contract, no audio, unread project fields.** §5.1 held-state + edge is in no host; `CLIP` chunks have no consumer; `inputActions`, `quality`, `targets` are validated and unused. | Games cannot be played or heard yet. | M3 (C2, D3, D4) |
| **Authoring commands have no ABI; the Inspector metadata has no consumer.** `Editor` is Rust-only behind `fynkel scene apply`; `label`/`tooltip`/`min`/`max`/`step` are generated and unread. | The editor (M4) has the commands and the metadata but no handle on them. | M4 (D5) |
| **Pointer lookup is linear.** `Node::pointer` scans object members; valid documents are unaffected, a document with thousands of diagnostics is quadratic (16 000 entities with 8 000 warnings: 726 ms to load). | The editor resolving pointers per keystroke on big scenes. | M4 (D7) |
| **Everything runs on the Flutter UI thread** (step, extract, render). | Fine at current sizes; ADR-0007 allows a render thread later, so extraction must stay cheap. | M5 |
| **Single machine, single profile.** Every number is from one M3 Max and one iPhone; no second display / DPI change exercised. | Regressions elsewhere would not be seen. | first release |

## 4. Compromises we knowingly made

Not bugs — things done the simple way with a note to revisit.

- `material`, `shader`, `meta`, `project` and `lock` schemas are
  **hand-written**; every component and behaviour schema is generated.
  Acceptable while these models are only validated; when renderer V1 needs a
  Rust `Material`, that becomes the source of truth (D6).
- Schema-vs-semantic diagnostic de-duplication works by **matching JSON
  pointers** (`scene.rs`, `material.rs`, `runtime.rs`). Simple; a message can
  vanish if one side changes the pointer it anchors to. The fixture corpora
  catch that.
- Document numbers are `f64` all the way; the runtime is `f32`. Narrowing
  happens in the generated codecs without a diagnostic for precision loss.
- `Document::new` embeds a minimal body per kind instead of reading defaults
  from the schema.
- Dart tests run through Flutter's `dart` only (`hooks`/`code_assets` are
  still experimental on stable Dart). Pinned; fallback documented in the M1
  gate report.
- The glTF importer writes zero tangents when the file has none (the cook
  generates them — `MESH` info `tangents: generated`) and drops
  non-default samplers and `TEXCOORD_1` bindings with a warning (the PBR
  model has one sampler and one UV set).
- The cache has no garbage collection beyond `fynkel cook --prune`; stale
  objects accumulate until then, and `rm -rf .fynkel/cache` is always safe.
  Cooked textures are RGBA8 with one mip level for every profile (D8).
- The renderer's normals are transformed by the model matrix and
  renormalised (no inverse transpose), so non-uniform scale skews them;
  emissive is relative to the exposure's white rather than in nits; the
  sky is a texture reference on `fynkel.Environment`, not the blueprint's
  `fynkel.sky@1` material (`docs/renderer.md` § Deviations). A scene that
  authors no lighting at all still gets the host's fallback sun (the M1
  demo scene). Custom-shader materials have no cooked record.
- The runtime store keeps every retained payload decoded in memory
  (`ResidencyStats.bytes`); eviction happens only when the last `Residency`
  of a scene drops. No streaming, no budget — fine at fixture sizes.
- `fynkel test --headless`, `state dump`, `render` and `run` cook the whole
  project before loading (a cache hit is ≈12 ms); `--no-cook` trusts the
  cache. A stale cache is `FYN-R0002`, never silently re-read.
- Migrations: `material/1-2/pbr-fields`, `scene/1-2/camera-exposure` and
  `prefab/1-2/camera-exposure` are the real steps so far, each with its
  fixture pair; adding a required field to a component always costs one.
- `FYNKEL_STATUS_LOAD_FAILED` (8) and the Apple viewport ABI v2 were added
  without a deprecation window; both ABIs are pre-release.
- ADR-0005 (license) is still open; nothing is public yet. Due at the M3
  gate.

## 5. Verification (what "green" means)

Run from a clean clone — the same steps as `.github/workflows/ci.yml`:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p fynkel_cli -- codegen --check
cd packages/fynkel && dart pub get && dart analyze --fatal-infos && dart test   # Flutter's dart
cd packages/fynkel_flutter && flutter pub get && flutter analyze --fatal-infos && flutter test
cd hosts/flutter_apple && flutter pub get && flutter analyze --fatal-infos
cd packages/fynkel_generator && dart pub get && dart analyze --fatal-infos && dart test
cd packages/fynkel_generator/example/spinner_game && dart pub get && dart analyze --fatal-infos && dart test
cargo run -p fynkel_cli -- codegen --check --project packages/fynkel_generator/example/spinner_game
cargo run -p fynkel_cli -- create /tmp/starter_game && cd /tmp/starter_game
fynkel validate && fynkel test --headless && fynkel codegen --check --project . && flutter analyze && flutter test
cd examples/showcase && fynkel validate && fynkel test --headless && fynkel render --out /tmp/showcase.png && fynkel codegen --check --project . && flutter analyze --fatal-infos && flutter test
```

(`cargo test -p fynkel_cli` runs the `codegen --check` lines itself when a
Dart SDK is on `PATH`, and a `--no-flutter` create through validate and the
headless host always.) Timings: `python3 benchmarks/m0/capture.py` (frame
loop, bridge, allocation, leaks), `python3 benchmarks/m2/capture.py`
(tools) and `python3 benchmarks/m3/capture.py` (the stress scene, with
the item 6 gate); results land under
`benchmarks/*/results/<date>-<commit>.md`.

Last clean-clone run: **2026-09-19, commit `68e9a72` + the M2-18 changes**
(M2 gate): fmt clean, clippy clean, 189 Rust tests passed (2 doc-tests
ignored by design), both `codegen --check` pipelines up to date, `dart
analyze --fatal-infos` clean, 58 Dart tests passed (49 `fynkel`, 5
`fynkel_generator`, 4 `spinner_game`), starter project `flutter analyze`
clean and `flutter test` 1 passed; ≈5 min wall clock (Flutter 3.47.2 / Dart
3.13.2, Rust 1.93.0, M3 Max). M3-01 added 7 `fynkel_flutter` tests (65 Dart
tests total) and the host/package analyze steps; the starter was built and
run on macOS and the host on the iOS simulator by hand. `platform/apple`'s
`presenter_wait_yields_lag_zero` is load-sensitive — it fails while another
build or a simulator app is using the GPU and passes on a quiet machine.
M3-02 (2026-09-19): fmt/clippy clean, `cargo test --workspace` green,
both `codegen --check` pipelines up to date (fingerprint
`0x45bf9f2f1cc8d124`), 66 Dart tests (50 `fynkel`, 5 `fynkel_generator`,
4 `spinner_game`, 7 `fynkel_flutter`), starter create → validate →
headless → cook → render → codegen → analyze → test clean; the robot's
cooked `MATL` chunk binds all five texture slots.
M3-03 (2026-09-20): fmt/clippy clean, 198 Rust tests green, both
`codegen --check` pipelines up to date, 66 Dart tests (the SDK, spinner and
starter suites now cook into a temporary cache first), starter create →
validate → headless (3 assets retained) → render (1 mesh) → codegen →
analyze → test clean; the render gate deletes the fixture's source tree
after cooking and renders identical pixels from the cache.
M3-04 (2026-09-20): fmt/clippy clean, `cargo test --workspace` green (the
fixture project now has 5 entities and 7 assets), both `codegen --check`
pipelines up to date (fingerprint `0x408b089693b6c6b6`), Dart suites
green, starter create → validate → headless → render → codegen → analyze
→ test clean; 11 shading pixel tests (exposure, point/spot/budget,
ambient, orthographic, maps, mask, double-sided, emissive, sky) and the
fixture render with its normal + metallic-roughness maps.
M3-06 (2026-09-20): fmt/clippy clean, `cargo test --workspace` green
(19 shading pixel tests; `presenter_wait_yields_lag_zero` in
`platform/apple` flaked once under the full parallel run and passed 3/3
alone — a GPU timing test), both `codegen --check` pipelines up to date,
Dart suites green (the controller test asserts the appended draw/culled
counts), starter create → validate → headless → render (1 instance in 1
draw, 0 culled) → codegen → analyze → test clean, showcase validate →
headless → render (27 instances in 16 draws, 0 culled, 18 casters in 8
shadow draws; `renders/m3-06.png` byte-identical to `m3-05.png`) →
codegen → analyze → test clean; stress capture
`benchmarks/m3/results/2026-09-20-cf2b3eb` all four gate checks PASS
(5 343 instances in 15 draws ≤ 15 pairs, 458 culled, standalone 122 fps
with prepare + encode + submit 1.41 / 1.59 / 1.78 ms, Flutter 120 ticks/s,
`render()` 2.6 ms, GPU 3.2 ms); M0 capture `2026-09-20-cf2b3eb` within
noise (122/120 fps, engine CPU 0.12 / 0.20 / 0.26 ms; GPU p50 1.05 ms vs
0.74 with the p95 unchanged at 1.08 and Flutter's own raster thread up by
the same 0.3 ms in both of two runs — the GPU's clock state, the cube
scene's draws are the same).
M3-05a (2026-09-20): `examples/showcase` validate → headless (34 entities,
22 assets) → render (`medium`: 27 meshes, 5 blended, 3 lights, 18 casters
in a 2048² map; `low` 960×540 no shadow; `high` 1920×1080, 4096² map) →
codegen → analyze → test clean; the app launched on macOS with the orbit
running and the HUD's frame counts live; fynkel_flutter's controller test
checks the appended stats.
M3-05 (2026-09-20): fmt/clippy clean, `cargo test --workspace` green
(18 shading pixel tests, `PIPELINE_VARIANTS` 9), both `codegen --check`
pipelines up to date, Dart suites green (the viewport controller test
switches to `low` and back against the real viewport), starter create →
validate → headless → render (`medium`: MSAA 4, 1 caster in a 1024² map)
→ render `--quality low` (MSAA 1, no shadow, 960×540) → codegen → analyze
→ test clean; M0 capture `2026-09-20-12859e1` — standalone 122 fps,
Flutter Path A 120 fps, engine CPU 0.11 / 0.19 / 0.29 ms, GPU 0.74 /
1.08 / 2.21 ms, present lag 0 — within noise of `2026-09-19-f958917`
with the cube scene now drawn at MSAA 4 behind a shadow pass.

## 6. Verdict and next steps

M2 is closed with a GO (`m2-gate-report.md`). The data layer is the shape the
blueprint asked for: plain files with exact diagnostics, identity that
survives edits, moves and re-imports, prefabs that expand deterministically,
a cache that re-cooks exactly what changed, and behaviours that go from a
Dart class to a schema to a running instance — all of it driven by `--json`
commands that an agent can run, and all of it measured in milliseconds.
Nothing built so far needs to be torn down.

What the gate could not prove is the game: nobody can see a created project
in its own app, play it, or hear it. That is M3, in this order:

1. ~~**`package:fynkel_flutter`** (D1)~~ — done (M3-01): the viewport widget
   and Swift plugin as a package; the template uses it; a created project
   shows its spinning cube.
2. ~~**Rust material model** (D6)~~ — done (M3-02): `fynkel_core::material::PbrMaterial`
   is the one definition behind the generated material schema, the codecs,
   the importer's output, the cooked `MATL` chunk and what the renderer
   reads; materials are formatVersion 2 via the first real migration.
3. ~~**Cooked-scene loading** (D2)~~ — done (M3-03): `fynkel cook` writes
   expanded scenes, `fynkel_assets` is the runtime store with residency, the
   ABI and `World.loadProject` read the cache only; the pixel test renders
   from a cache whose sources were deleted.
4. ~~**Renderer V1 (a)**~~ — done (M3-04): physically based shading with
   every map, HDR + ACES + camera exposure, sun/ambient/sky, 8 point/spot
   lights, orthographic camera; `docs/renderer.md`.
5. ~~**Renderer V1 (b)**~~ — done (M3-05): the sun's PCF shadow,
   transparency, MSAA 1/4, render scale, quality profiles switched at
   runtime from every surface, debug lines; the M0 capture unchanged.
   5a (M3-05a): `examples/showcase`, the scene every renderer item is
   looked at through (`renders/<item>.png`).
6. ~~**Renderer V1 (c)**~~ — done (M3-06): one draw per `(mesh,
   material)` pair, frustum culling on cooked bounding spheres (camera and
   sun's box), the stress scene generator and `benchmarks/m3/capture.py`
   with the gate green at 5 000 meshes.
7. **Input** (C2, D3), **audio** (D4), and the Tier 1 reference game that
   exercises them — measured with both captures at every step.
