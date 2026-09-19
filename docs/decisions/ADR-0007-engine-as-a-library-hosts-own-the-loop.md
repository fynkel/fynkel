# ADR-0007: Engine as a library; hosts own the loop

**Status:** Accepted  
**Date:** 2026-09-19  
**Related:** `docs/blueprint.md`

## Context

The v0.3 blueprint's diagram placed the Flutter app on top of the engine and left open who drives the tick. The author's requirement is that the engine must be fully working without Flutter, with Flutter as a UI layer, and that the engine must not depend on the UI system.

## Decision

`fynkel_core` is a **library** exposing `step(commands) -> snapshot`. It creates no simulation thread and has no Flutter dependency. A **host** owns the main loop and calls `step()`:

| Host | Loop source |
| --- | --- |
| Flutter game app | Flutter frame callback |
| Standalone Rust host | winit event loop |
| Headless host | test / CLI loop |
| Fynkel Editor | Flutter frame callback, edit-preview world |

The renderer may own a Rust render thread consuming immutable extracts and presenting to the surface the host provides. The standalone and headless hosts are built and tested continuously to prove Flutter independence.

## Alternatives considered

- Engine-owned main loop with Rust thread and own clock: rejected for 1.0 because Dart gameplay must run every tick anyway and can only be entered from its own thread, making the Rust clock nominal while adding a cross-thread handshake.

## Consequences

"Dart drives the tick" means "the Flutter host calls `step()`", not "Flutter owns the engine". The public API uses the word *host*, never *Flutter*, for this role. High presentation rates are a surface property: Flutter `Texture` presents at display vsync; an engine-owned surface may present uncapped.
