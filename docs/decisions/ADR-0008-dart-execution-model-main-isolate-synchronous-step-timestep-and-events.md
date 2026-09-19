# ADR-0008: Dart execution model main isolate synchronous step timestep and events

**Status:** Accepted  
**Date:** 2026-09-19  
**Related:** `docs/blueprint.md`

## Context

Given ADR-0007, the remaining questions are which isolate runs gameplay, how time is stepped, how events reach Dart, and what a read after a write returns.

## Decision

- **Main Flutter isolate.** Behaviours, snapshots, and commands live on the UI isolate; `step()` is a synchronous FFI call from the frame callback. A gameplay isolate and/or Rust sim thread are documented migration options; trigger = measured Dart gameplay time that causes HUD frame drops on the reference game or stress benchmark. Migration gets its own ADR.
- **Accumulator fixed step (60 Hz default) with render interpolation.** Up to `maxStepsPerFrame` catch-up ticks; beyond that, time is dropped and a `MissedDeadline` diagnostic is emitted. `update`/`lateUpdate` run once per host frame; their writes are staged for the next tick.
- **Events are pull-only.** Rust queues collision/trigger/animation/asset/scene events; Dart drains them at the next tick (or frame start). Async completions on Rust threads may arrive up to one frame late. No `NativePort` delivery in 1.0.
- **Read-your-writes overlay** for plain data fields; physics-driven state is read-only mid-tick; rejected commands clear their overlay entry and emit a diagnostic.
- **HUD state** comes from Dart-side observable models written by Behaviours, never directly from the snapshot; projected-label positions are the exception via snapshot subscriptions.

## Alternatives considered

- Gameplay isolate + Rust sim thread (three clocks with a per-tick rendezvous): correct but a month of M1 for a solo developer and harder to debug; deferred.
- Gameplay isolate with Dart-driven tick: viable middle option; deferred with the same trigger.
- NativePort push events: only needed if Rust threads must wake Dart off-tick; not needed in 1.0.

## Consequences

M1 has one thread of control and no locks. Dart allocation/GC per frame becomes an M1 gate. Loading screens that need immediate asset-ready notification must run a tick loop.
