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
| serde / serde_json | 1.0.229 / 1.0.151 | `fynkel` CLI JSON Schema output; not on the runtime path |
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

Dart side: `ffi` 2.1.x; `ffigen` 20.x (dev, `packages/fynkel`) generating from `crates/fynkel_native/include/fynkel.h`.
Note: run Dart tooling for `packages/` with Flutter's Dart (`<flutter>/bin/dart`, 3.13.2); a Homebrew `dart` on PATH may be older.
