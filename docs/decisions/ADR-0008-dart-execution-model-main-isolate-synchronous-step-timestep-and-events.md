# ADR-0008: Dart execution model main isolate synchronous step timestep and events

**Status:** Accepted; amended 2026-09-27 (a `transform` write in `update`/`lateUpdate` shows that frame, below)  
**Date:** 2026-09-19  
**Related:** `docs/blueprint.md`

## Context

**Review from 2026-09-30:** [ADR-0046](ADR-0046-transactional-world-state-replacement.md) and [ADR-0047](ADR-0047-identity-clocks-frame-order-and-readiness.md) accept replacement visibility, explicit presentation, clock and frame contracts. They supersede those design portions for the replacement implementation. The [F1.2 clock migration](../design/foundation-clock-migration.md) now implements the replacement timing contract in the live runtime. The remaining frame/visibility portions describe prior behavior until F2 migration; main-isolate execution and once-per-tick input-edge consumption remain selected.

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

## Amendment (2026-09-27): a `transform` write in `update` shows that frame

**Status:** Accepted by the user 2026-09-27 (docs/foundation.md 1.2). The SDK decides the lane per write: `GameObject._presentsWrite` — in `World.frame`'s update phase, on a resolved object whose `RigidBody` (staged or the tick's) is not dynamic.

**Context.** The decision above makes "transforms of non-physics objects set in `lateUpdate`" presentation commands: shown in the frame being rendered, staged as the authoritative value for the next tick. The SDK never used that lane for `transform`: `transform.position = …`, `setLocalTransform` and the partial writes are always simulation commands, applied at the next tick. So a write in `update` reached the screen one to two ticks later (17–33 ms at 60 Hz), and on a frame with no tick (a 120 Hz display: every other frame) it was not shown at all — two `update` writes collapsed into one. That is why the examples move things in `fixedUpdate`. In Unity a `transform` write in `Update` or `LateUpdate` shows in that frame, and scripts move things there; the explicit `setPresentationTransform` is not what a Unity user reaches for.

**Decision.**

1. A `transform` write — `transform.position`/`rotation`/`localPosition`/`localRotation`/`localScale`/`value`, `setLocalTransform`, `setLocalPosition`/`Rotation`/`Scale`, `writeLocal` — made while the world runs `update` or `lateUpdate` (the behaviours', their coroutines' and the host's `update` callback) is a **presentation command** when the object is resolved and is not a dynamic body: it shows in the frame being rendered and is staged as the authoritative value for the next tick, exactly as the decision above describes for `lateUpdate`. Reads return it at once, as today.
2. A **dynamic body** (physics-owned) written in `update` stays a simulation command: a teleport at the next tick that keeps its velocity (what Unity does when a script sets a Rigidbody's `transform`). Moving a body smoothly is still `fixedUpdate`'s job (`move`, velocity, forces), as in Unity.
3. Writes in `fixedUpdate`, `afterPhysics`, the contact callbacks and outside a frame are simulation commands, unchanged. `setPresentationTransform` stays for code that wants the presentation lane explicitly (a camera from a host callback).
4. A character controller or kinematic body written in `update` is a presentation command too: it shows that frame and teleports at the next tick (Unity's CharacterController has the same rule: a transform write teleports it; `move` is the way to walk it).

**Consequences.** Moving things in `update` works as in Unity, at any display rate. `fixedUpdate` stays right for anything physics or input-edge driven and for anything a headless replay must reproduce (presentation writes do not change the simulation until the next tick takes them). The examples keep their `fixedUpdate` movers; the docs stop recommending it for plain movement. Test: `packages/fynkel/test/update_writes_test.dart` (a write in `update` shows in the same frame's projection on a frame with no tick; a dynamic body's teleports at the tick; `fixedUpdate` writes unchanged).
