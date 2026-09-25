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
| `ufbx` | 0.11.4 | FBX parsing in `fynkel_import` (M3-19b, `fynkel.fbx`): the single-file C library `ufbx` with its Rust bindings, compiled by `cc` into the crate (builds for the iOS targets too); does unit/axis conversion and animation baking; reads memory only — `load_external_files` off, Fynkel reads an allowed external texture itself |
| `base64` | 0.23.1 | `data:` URIs in `.gltf` files |
| `image` | 0.25.10 | PNG/JPEG/Radiance HDR decoding in `fynkel_import`; `default-features = false`, features `png`, `jpeg`, `hdr` only (no other formats compiled in; `hdr` since E1) |
| `symphonia` | 0.6.1 | WAV (PCM) and Ogg Vorbis decoding in `fynkel_import` and, since M3-11 part 2, in `fynkel_audio` (a streamed clip's decoder thread, [audio.md](audio.md) § Streamed music — the same decoder, so the runtime hears what the import validated); `default-features = false`, features `wav`, `pcm`, `ogg`, `vorbis` — no SIMD features, so decoding is bit-identical across machines. Chosen over `hound` + `lewton` (the blueprint's candidates): one maintained pure-Rust decoder covers both formats and bounds its own allocations |
| `cpal` | 0.18.2 | The output device in `fynkel_audio` (M3-11, `docs/audio.md`), behind the crate's `device` feature (on in `fynkel_native` and `hosts/standalone`, off in `hosts/headless` and every test — the null backend): CoreAudio on macOS and iOS, the mixer inside its callback. Chosen with Fynkel's own mixer over `kira` (the blueprint's candidate): the gate counts allocations in the callback, so the callback is ours. The Apple frameworks it needs (AVFoundation, AudioToolbox, CoreAudio) were already linked by `libfynkel_platform_apple.dylib` |
| `rtrb` | 0.4.0 | The two single-producer/single-consumer rings between the world and the mixer (commands out, released clips and ended voices back), lock-free and allocation-free after construction |
| `blake3` | 1 | Cache keys in `fynkel_import::cache` (content addressing, §6) and document revisions in `fynkel_document::authoring`; pure Rust, no SIMD-dependent output — the same inputs hash the same on every machine |
| `crc32fast` | 1 | Per-chunk and header checksums in `fynkel_cooked`; the crate's only dependency, so a runtime can read cooked containers without the importers |

Dart side: `ffi` 2.1.x; `ffigen` 20.x (dev, `packages/fynkel`) generating from `crates/fynkel_native/include/fynkel.h`; the same `ffigen` in `packages/fynkel_authoring` (M4-01) generating `lib/src/native/fynkel_authoring_bindings.g.dart` from `include/fynkel_authoring.h` (`dart run ffigen --config ffigen.yaml`; only `fynkel_authoring_*`, `FynkelBufferView`, `FynkelStatus` and the ABI-version macro) onto the same code asset — that package's pubspec opts into the runtime's `authoring` cargo feature through `hooks.user_defines.fynkel.features`, so its hook build is `cargo build --release -p <crate> --no-default-features --features authoring,animation,audio,physics`. The engine modules (ADR-0029) are cargo features of `fynkel_core` and above (`physics` = Rapier, `audio` = the mixer with rtrb and symphonia, plus cpal in `fynkel_native`, `animation` = the sampler), default on in the workspace; the hook always asks for all three unless the app's `hooks.user_defines.fynkel.project` names a project whose `modules.strip` is `true`, in which case the modules it turns off are left out and the hook logs `modules.strip: the runtime leaves out …` before building (`moduleFeaturesFor`). A stripping app, like one asking for `authoring`, never uses the prebuilt download. Tools and hosts (`fynkel_authoring`, `fynkel_import`, the CLI, the headless and standalone hosts) depend on `fynkel_core` with its defaults.

The editor (`apps/editor`, M4 item 2; a Flutter macOS app, `flutter pub get && flutter analyze --fatal-infos && flutter test`, formatted with `dart format` at `page_width: 120`):

| Package | Version | Why |
| --- | --- | --- |
| `docking` | 1.16.2 | the workspace: tabs, splits, drag to re-dock, `keepAlive` items, `stringify`/`load`; picked by the spike recorded in ADR-0016's resolutions (`apps/editor/test/spike/docking_spike_test.dart`, all seven criteria) |
| `multi_split_view` / `tabbed_view` | 2.4.0 / 1.18.1 | pinned by `docking` (its splitter and tab strip); 3.x of both exist, `docking` owns that upgrade |
| `file_picker` | 10.3.10 | the native directory picker behind Open and New Project (macOS `NSOpenPanel`); injected in tests |
| `watcher` | 1.2.1 | the project-directory watcher that rescans and revalidates on every change (400 ms debounce; caches, build output, `.git/` and the platform runner directories ignored) |
| `path` | 1.9.x | the layout and preferences files under `~/Library/Application Support/Fynkel Editor/`, project-relative diagnostic paths |
| `fynkel_flutter` (path) | — | the Scene view's `FynkelViewport` over the edit-preview world (M4-06): the same texture widget a game host uses, with viewport ABI 4's `renderHandle`/`debugLines`/`readFrame` |
| `fake_async` | 1.3.x (dev) | the Scene view's debounced editor-state save under a fake clock (M4-06) |
| `lucide_icons_flutter` | 3.1.20 | the editor's icon font (item 9a): Lucide's 16 px stroke icons in place of Material Symbols, reached only through `FIcons` in `lib/src/theme/icons.dart` (docs/editor.md § The look); MIT-licensed (the Lucide drawings ISC), a font and its constants, no code that runs |

Behaviour generator (`packages/fynkel_generator`, ADR-0012 pipeline (b), M2-16):

| Package | Version | Why |
| --- | --- | --- |
| `build` / `build_runner` | 4.0.11 / 2.16.1 (dev in the game package) | the aggregate builder and the tool that runs it; `build_runner` 2.16 compiles builders AOT once per package (`.dart_tool/build`), later runs take ~1 s |
| `source_gen` | 4.3.0 | `TypeChecker`/`ConstantReader`/`LibraryReader` over the analyzer's element model |
| `analyzer` | 14.4.0 | element model and AST (field initializers → defaults); the version `source_gen` 4.3 targets |
| `dart_style` | 3.1.13 | formats the generated Dart so output is byte-stable |
| `glob` | 2.2.0 | `lib/**.dart` discovery |

`fynkel codegen --project` finds the Dart SDK through `$FYNKEL_DART`, the `dart` next to `flutter` on `PATH`, or `dart` on `PATH`.
`fynkel create` runs `flutter create --empty --platforms macos,ios` from `flutter` on `PATH` for the platform runners (skipped otherwise); a created project is a Flutter app, so its tests run with `flutter test` (Flutter 3.47 builds the native-assets hook for the host under `flutter test`, ~12 s warm), and `flutter_lints` 6 / `build_runner` 2.16 are its dev dependencies. `package:fynkel_flutter` is a Flutter plugin whose Swift sources ship as one shared-darwin Swift package (`darwin/fynkel_flutter/Package.swift`, swift-tools 5.9, macOS 12 / iOS 15) plus a podspec: Flutter 3.47 apps use Swift Package Manager for plugins, older CocoaPods projects use the podspec. Its tests and the host app's analysis use `flutter` (not `dart`): `flutter pub get && flutter analyze --fatal-infos && flutter test`. Flutter's native-assets step rewrites the dylib's install name to the app's absolute path with `install_name_tool`; `platform/apple` and `fynkel_native` link with `-headerpad_max_install_names` (their `build.rs`) so that works from deep project directories.
M3-07 (input): no new crate. Gamepads come from Apple's **GameController**
framework (`GCController`, linked by `packages/fynkel_flutter`'s Swift
package on macOS and iOS; the podspec lists it too) instead of the `gilrs`
crate the blueprint named for macOS — one framework for both Apple targets,
the controllers the OS pairs, nothing added to the Rust runtime path.

M3-08 (physics): `rapier3d` 0.35.3 in `crates/fynkel_physics` only, default
features (`dim3`, `f32`, `std`; not `parallel`, not `simd-*`, not
`enhanced-determinism` — one platform steps the same scene the same way,
which the headless checksum asserts; cross-platform bit equality is not
claimed). It builds on the workspace's `glam` 0.33.7 through `glamx`
0.3.1 / `parry3d` 0.30.2, so poses cross without conversion. `fynkel_core`
depends on `fynkel_physics` for the step; nothing above the core names a
Rapier type (`docs/physics.md`).

Note: run Dart tooling for `packages/` with Flutter's Dart (`<flutter>/bin/dart`, 3.13.2); a Homebrew `dart` on PATH may be older.
