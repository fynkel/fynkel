# Toolchain matrix (M0 freeze — 2026-09-19)

Frozen for M0 on the reference machine. Upgrades are deliberate changes (blueprint §2.3): update this file and note binary-size/compile-time/behaviour impact.

## Reference machine (benchmark profile `mac-m3max-2026`)

| | |
| --- | --- |
| Machine | MacBook Pro, Apple M3 Max, arm64 |
| Display | 3456×2234 Retina (ProMotion) |
| OS | macOS 26.5.1 (25F80) |
| GPU backend | Metal |

## Tools

| Tool | Version | Notes |
| --- | --- | --- |
| Rust | 1.93.0 stable; std for `aarch64-apple-darwin`, `x86_64-apple-darwin`, `aarch64-apple-ios`, `aarch64-apple-ios-sim`, `x86_64-apple-ios` | pinned in `rust-toolchain.toml` (targets included, rustup installs them); Flutter builds universal macOS apps and two simulator slices, and the native-assets hook (ADR-0015) builds per architecture |
| Flutter | 3.47.2 stable (engine a804b26164) | Impeller default on macOS |
| Dart | 3.13.2 | |
| Xcode | 26.5 (17F42) | |

## Rust crates

| Crate | Version | Why |
| --- | --- | --- |
| wgpu | 30.0.1 | renderer |
| winit | 0.30.13 | standalone host window/event loop (0.31 is beta; not adopted) |
| glam | 0.33.7 | math, `bytemuck` feature for GPU upload |
| bytemuck | 1.25.2 | POD casts |
| hecs | 0.11.1 | ECS storage behind `fynkel_ecs` (ADR-0003) |
| syn / quote / proc-macro2 | 2.0.119 / 1.0.47 / 1.0.107 | `fynkel_schema_derive` proc macro only (ADR-0012) |
| serde / serde_json | 1.0.229 / 1.0.151 | `fynkel` CLI JSON Schema output; since the scene-on-screen spike `serde_json` is also on the runtime path: `fynkel_render` reads the cache index, META and cooked material JSON, `fynkel_native` writes the load report |
| pollster | 1.0.1 | block on wgpu async setup |
| log / env_logger | 0.4.34 / 0.11.11 | logging |

Later M0 additions (Flutter viewport; since M1-07 in `platform/apple`):

| Crate | Version | Notes |
| --- | --- | --- |
| `wgpu-hal` | 30.0.1 | feature `metal`; `texture_from_raw` for IOSurface-backed textures |
| `objc2` | 0.6.3 | |
| `objc2-foundation` | 0.3.2 | |
| `objc2-metal` | 0.3.2 | features `objc2-io-surface` (`newTextureWithDescriptor_iosurface_plane`), `block2` |
| `objc2-core-foundation` | 0.3.2 | |
| `objc2-io-surface` | 0.3.2 | |
| `block2` | 0.6.2 | ObjC blocks for `MTLCommandBuffer.addCompletedHandler` |

M2 additions (`fynkel_document`, authoring tools; none on the runtime path — `fynkel_document` depends on `fynkel_core` for the typed scene description, never the other way round, so the runtime stays free of JSON/schema crates; the headless host links both):

| Crate | Version | Notes |
| --- | --- | --- |
| `serde_json` | 1.0.151 | now with `preserve_order` (schema key order is meaningful) |
| `jsonschema` | 0.56.0 | draft 2020-12 validation; `default-features = false` (no HTTP/file retrieval — every `$ref` resolves from the loaded `schemas/` directory) |
| `uuid` | 1.26.1 | `v4` for fresh document ids, `v5` for prefab-instance derived ids; parsing is canonical-text only |
| `similar` | 2.7.0 | unified diffs for `fynkel migrate --dry-run` and `fynkel scene apply --dry-run` |
| `gltf` | 1.4.1 | glTF 2.0 / GLB parsing in `fynkel_import`; `default-features = false` with `utils` (accessor readers), `names`, `KHR_materials_emissive_strength` — no `import` feature, so the crate never touches the file system or decodes images; Fynkel resolves every buffer and image itself (external files denied by default) |
| `base64` | 0.23.1 | `data:` URIs in `.gltf` files |
| `image` | 0.25.10 | PNG/JPEG decoding in `fynkel_import`; `default-features = false`, features `png`, `jpeg` only (no other formats compiled in) |
| `symphonia` | 0.6.1 | WAV (PCM) and Ogg Vorbis decoding in `fynkel_import`; `default-features = false`, features `wav`, `pcm`, `ogg`, `vorbis` — no SIMD features, so decoding is bit-identical across machines. Chosen over `hound` + `lewton` (the blueprint's candidates): one maintained pure-Rust decoder covers both formats and bounds its own allocations |
| `blake3` | 1 | Cache keys in `fynkel_import::cache` (content addressing, §6) and document revisions in `fynkel_document::authoring`; pure Rust, no SIMD-dependent output — the same inputs hash the same on every machine |
| `crc32fast` | 1 | Per-chunk and header checksums in `fynkel_cooked`; the crate's only dependency, so a runtime can read cooked containers without the importers |

Dart side: `ffi` 2.1.x; `ffigen` 20.x (dev, `packages/fynkel`) generating from `crates/fynkel_native/include/fynkel.h`.

Behaviour generator (`packages/fynkel_generator`, ADR-0012 pipeline (b), M2-16):

| Package | Version | Why |
| --- | --- | --- |
| `build` / `build_runner` | 4.0.11 / 2.16.1 (dev in the game package) | the aggregate builder and the tool that runs it; `build_runner` 2.16 compiles builders AOT once per package (`.dart_tool/build`), later runs take ~1 s |
| `source_gen` | 4.3.0 | `TypeChecker`/`ConstantReader`/`LibraryReader` over the analyzer's element model |
| `analyzer` | 14.4.0 | element model and AST (field initializers → defaults); the version `source_gen` 4.3 targets |
| `dart_style` | 3.1.13 | formats the generated Dart so output is byte-stable |
| `glob` | 2.2.0 | `lib/**.dart` discovery |

`fynkel codegen --project` finds the Dart SDK through `$FYNKEL_DART`, the `dart` next to `flutter` on `PATH`, or `dart` on `PATH`.
`fynkel create` runs `flutter create --empty --platforms macos,ios` from `flutter` on `PATH` for the platform runners (skipped otherwise); a created project is a Flutter app, so its tests run with `flutter test` (Flutter 3.47 builds the native-assets hook for the host under `flutter test`, ~12 s warm), and `flutter_lints` 6 / `build_runner` 2.16 are its dev dependencies.
Note: run Dart tooling for `packages/` with Flutter's Dart (`<flutter>/bin/dart`, 3.13.2); a Homebrew `dart` on PATH may be older.
