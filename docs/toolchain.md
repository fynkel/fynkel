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
| Rust | 1.93.0 stable, `aarch64-apple-darwin` | pinned in `rust-toolchain.toml` |
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
| pollster | 1.0.1 | block on wgpu async setup |
| log / env_logger | 0.4.34 / 0.11.11 | logging |

Later M0 additions (when the Flutter host lands): `wgpu-hal`, `metal`, `core-foundation`, `core-video`/IOSurface bindings, `objc2`. Record versions here when added.
