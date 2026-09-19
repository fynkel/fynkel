# Engine status — 2026-09-19 (after M2-10)

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
| `fynkel_native` + `package:fynkel` | C ABI v1, `ffigen` bindings, Dart SDK (`World`, `GameObject`, transform writers), native-assets build hook | **Solid** | 1 000 entities/tick: 191 µs encode+submit, 169 µs step, 47 µs read; 1.5 KB/frame Dart allocation on the 25-entity scene, 0 GC in 3 000 frames |
| `fynkel_render` | wgpu renderer: every extracted transform drawn as a lit instanced cube, one camera, one directional light | **Seed** — real, but a fixed demo look | pixel-readback tests; runs in every host |
| `platform/apple` + `hosts/flutter_apple` | IOSurface ring → Flutter `Texture` (Path A), 0 copies, GPU-completion hand-off; macOS + iOS | **Solid** | 120 Hz, lag 0, raster p95 0.95 ms; iPhone 16 Pro Max: GPU 1.84 ms, no drops |
| `hosts/headless` + `hosts/standalone` | Rust hosts: deterministic runs, `--scene`, state dump; winit window | **Solid** | CLI tests |
| `fynkel_document` | Strict JSON with spans, envelope, JSON Schema 2020-12, §14.2 diagnostics; scene, prefab, material, shader documents and `.meta.json` sidecars; prefab expansion; asset database (scan, resolve, reference check); migrations with diff + `.bak`; scene → runtime | **Solid** | fixture corpora with exact code/line/column expectations (valid, unknown, invalid, scene, prefab, material, assets incl. the move test, migrate) |
| `fynkel_import` | glTF/GLB → normalized intermediate (`FYNI`): meshes in a fixed vertex layout, `fynkel.pbr@1` materials, encoded images, node hierarchy as a prefab; sub-asset ids from the sidecar or derived; external files denied by default; limits | **Solid for static meshes** — no skins, morphs, animations, Draco | fixture corpus (`robot.glb` and variants), committed expected bytes (determinism), produced documents validated by the document layer, CLI test |
| `apps/cli` | `fynkel codegen`, `schema`, `test --headless`, `state dump`, `document`, `scene`, `prefab`, `material`, `shader`, `asset`, `migrate` — all with `--json` | **Solid** | CLI tests |

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

## 3. What is *not* proven yet (honest list)

| Gap | Why it matters | Owner |
| --- | --- | --- |
| **No file reaches the screen.** The Flutter host renders its own Dart demo scene; it cannot open a `.scene.json`. Only the headless host loads documents. | The "author a scene, see it" loop — the whole point — is not one line yet. | M2-17 template project + **M3 first item** |
| **The renderer ignores materials, lights and cameras from documents.** `Camera`, `MeshRenderer`, `DirectionalLight`, `PointLight`, materials and the `fynkel.pbr@1` manifest are stored and validated, nothing consumes them. | The material system is an interface with no implementation behind it. | M3 (renderer V1) |
| **Meshes are imported, not rendered; no textures or audio yet.** glTF/GLB becomes an intermediate (M2-10) that nothing consumes: no texture/audio importer, no cache, no loader in the renderer. | Nothing but cubes can be shown. | M2-11, 12; M3 |
| **No input contract** (§5.1 held-state + edge). | Games cannot be played yet. | M3 (C2) |
| **No project file, no authoring commands, no behaviour SDK.** Prefabs exist (M2-07) but only expand headlessly; the editor cannot apply/revert overrides yet. Migrations exist (M2-09) with one synthetic step — no real format change has been through them. | The editor (M4) has nothing to stand on. | M2-13, 14, 15, 16 (apply/revert: M4) |
| **Everything runs on the Flutter UI thread** (step, extract, render). | Fine at current sizes; ADR-0007 allows a render thread later, so extraction must stay cheap. | M5 |
| **Single machine, single profile.** All numbers are from one M3 Max and one iPhone; no second display / DPI change exercised. | Regressions elsewhere would not be seen. | first release |

## 4. Compromises we knowingly made

Not bugs — things done the simple way with a note to revisit.

- `material.schema.json` and `shader.schema.json` are **hand-written**; every
  other document/component schema is generated from Rust. Acceptable while
  the material model is only validated; revisit when the renderer needs a
  Rust `Material` type (M3), which should then become the source of truth.
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

Last clean-clone run: **2026-09-19, commit `fca5d30`** (re-run at M2-10: 123 Rust tests, 37 Dart tests) — fmt clean, clippy clean, 92 Rust tests passed, codegen up to date, `dart analyze` clean, 37 Dart tests passed (Flutter 3.47.2 / Dart 3.13.2, Rust 1.93.0, M3 Max).

## 6. Verdict and next steps

The foundation is the right shape: contracts and data first, one source of
truth for schemas, measured bridge and viewport, deterministic core. Nothing
built so far needs to be torn down.

The risk is not in what exists but in what has not met yet: **documents and
the renderer**. Until a scene file with a material shows up lit on the
screen, the authoring side is a well-tested promise.

Order of work:

1. **Finish M2 lean** (items 11–18: texture/audio import,
   cache, project, commands, behaviours, template). Each is a dependency of
   the editor; none should grow beyond its blueprint line.
2. **M2 gate (item 18)** — the built-in stop-and-look moment; this page gets
   rewritten there.
3. **M3 opens with the vertical slice**: the template project's scene, with
   its `fynkel.pbr@1` material and a glTF mesh, rendered in the Flutter host.
   Everything else in M3 follows from that being real.
