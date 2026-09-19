# ADR-0009: 1.0 platform intent macOS + iOS

**Status:** Accepted  
**Date:** 2026-09-19  
**Related:** `docs/blueprint.md`

## Context

The author develops on macOS, has a Windows machine available, and wants Flutter's mobile audience. Each platform adds an interop path, a lifecycle model, and a qualification cost.

## Decision

1.0 advertises **macOS and iOS**. Both use Metal and the same `CVPixelBuffer`/IOSurface texture interop. iOS gets a cube-on-device smoke test in M1, a running reference game at M3, and full qualification (touch overlay, background/foreground GPU suspension, thermal, signing/App Store packaging) in M6. Windows, Linux, and Android are post-1.0 and not on the marketing matrix. A periodic `cargo check --target x86_64-pc-windows-msvc` guards against Metal-only assumptions.

## Alternatives considered

- macOS + Windows: what the hardware allows, but two interop stacks and two backends before 1.0.
- macOS only: narrowest; rejected because mobile is central to the Flutter audience.

## Consequences

One backend to debug in 1.0. Touch input is a Flutter overlay mapped to named actions. `wgpu` D3D12/Vulkan paths are unexercised until post-1.0; renderer abstractions must stay backend-neutral by construction.
