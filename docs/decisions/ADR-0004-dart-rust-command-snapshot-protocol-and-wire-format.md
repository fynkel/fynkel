# ADR-0004: Dart/Rust command-snapshot protocol and wire format

**Status:** Accepted  
**Date:** 2026-09-19  
**Related:** `docs/blueprint.md`

## Context

The bridge is the second-highest performance risk. Convenience FFI (one call per property) and auto-generated ABIs both tend toward chatter.

## Decision

- The ABI is a **hand-written C header** exported by `fynkel_native`; Dart bindings are generated with **`ffigen`**. `flutter_rust_bridge` is not used.
- Per-frame traffic is batched: command buffers in, snapshot + event buffers out, one `step()` call per fixed tick.
- Wire format is a **hand-defined binary record layout** (fixed-size little-endian records in typed-data views). Encoders/decoders on both sides are **generated from the Rust component schemas** (ADR-0012). No FlatBuffers/Protobuf.
- Snapshots are subscription-based; Dart receives only what it observes.
- Structural commands return temp-id → handle mappings.

## Alternatives considered

- `flutter_rust_bridge`: fast start, but owns the ABI shape and encourages per-function calls.
- FlatBuffers: zero-copy and has Dart codegen, but a second schema language beside the Rust-derived one.

## Consequences

The ABI shape is a designed artifact reviewed like an API. Adding a component means editing a Rust struct, not three files.
