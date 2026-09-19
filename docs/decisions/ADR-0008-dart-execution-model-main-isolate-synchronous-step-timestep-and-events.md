# ADR-0008: Dart execution model main isolate synchronous step timestep and events

**Status:** Accepted  
**Date:** 2026-09-19  
**Related:** `docs/blueprint.md`

## Context

Given ADR-0007, the remaining questions are which isolate runs gameplay, how time is stepped, how events reach Dart, and what a read after a write returns.

## Decision

- **Main Flutter isolate.** Behaviours, snapshots, and commands live on the UI isolate; `step()` is a synchronous FFI call from the frame callback. A gameplay isolate and/or Rust sim thread are documented migration options; trigger = measured Dart gameplay time that causes HUD frame drops on the reference game or stress benchmark. Migration gets its own ADR.
- **Accumulator fixed step (60 Hz default) with render interpolation.** Up to `maxStepsPerFrame` catch-up ticks; beyond that, time is dropped and a `MissedDeadline` diagnostic is emitted. `update`/`lateUpdate` run once per host frame.
- **Two command classes.** *Simulation commands* (physics, spawn/destroy, animator parameters, transforms of simulation-owned objects) apply at the next fixed tick boundary. *Presentation commands* (camera transform, transforms of non-physics objects set in `lateUpdate`, material parameters, visibility, lights) apply before the current frame's render extraction, so a camera moved in `lateUpdate` affects the frame being rendered even when no fixed tick ran this frame. A presentation transform command both overrides the current extract (that object skips interpolation for the frame) and is staged as the authoritative value for the next tick. Physics-owned transforms reject presentation commands with a diagnostic.
- **Input edges are consumed by exactly one fixed tick.** `pressed`/`released` transitions are buffered per frame; the first fixed tick of the frame consumes them, later catch-up ticks see only held state, and a zero-tick frame carries them to the next tick. One press = one jump regardless of catch-up.
- **Events are pull-only, in two lanes.** Physics events (collision/trigger) produced by tick n are delivered in `afterPhysics(context)` of tick n — giving `afterPhysics` its meaning — and are also in the next tick's drain for behaviours that do not opt in. All other events (animation, asset-loaded, scene-ready) are queued in Rust and drained at the next tick or frame start; async completions may arrive up to one frame late. Commands issued in `afterPhysics` are simulation commands for tick n+1. No `NativePort` delivery in 1.0.
- **Read-your-writes overlay** for plain data fields; physics-driven state is read-only mid-tick; rejected commands clear their overlay entry and emit a diagnostic.
- **HUD state** comes from Dart-side observable models written by Behaviours, never directly from the snapshot; projected-label positions are the exception via snapshot subscriptions.

## Alternatives considered

- Gameplay isolate + Rust sim thread (three clocks with a per-tick rendezvous): correct but a month of M1 for a solo developer and harder to debug; deferred.
- Gameplay isolate with Dart-driven tick: viable middle option; deferred with the same trigger.
- NativePort push events: only needed if Rust threads must wake Dart off-tick; not needed in 1.0.

## Consequences

M1 has one thread of control and no locks. Dart allocation/GC per frame becomes an M1 gate. Loading screens that need immediate asset-ready notification must run a tick loop.
