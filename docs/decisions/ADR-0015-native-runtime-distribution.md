# ADR-0015: Native runtime distribution

**Status:** Accepted  
**Date:** 2026-09-19  
**Related:** `docs/blueprint.md`

## Context

Flutter developers must not need a Rust toolchain to use Fynkel.

## Decision

`fynkel_flutter` ships a Flutter **native assets build hook** (`hook/build.dart`). It reads the runtime version from `fynkel.lock.json`, downloads the matching prebuilt, checksummed artifact for the target from the release store, and registers it as a code asset. Fallback to `cargo build` when Rust is present and the artifact is missing or `FYNKEL_BUILD_FROM_SOURCE=1` is set. Missing artifact without Rust fails with an actionable diagnostic.

## Alternatives considered

- Vendoring binaries in the pub package: bloats every install with all platforms.
- Requiring Rust: hostile to the target audience.

## Consequences

Release CI must build and publish artifacts for every supported target/arch (macOS arm64/x86_64, iOS device/simulator) with checksums. Signing policy follows ADR-0005.
