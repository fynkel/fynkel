# ADR-0024: Raw keys and mouse buttons beside actions

**Status:** Accepted (2026-09-24, under the Unity parity pass's standing rule "default to Unity's concepts"; confirmed by the user 2026-09-25)  
**Date:** 2026-09-24  
**Related:** blueprint §5.1 and §9 "Input", D3 (the project's `inputActions` are the only binding source), ADR-0008 (edges consumed by exactly one tick), `docs/input.md`, `docs/unity-parity.md` § Tier 2

## Context

Games read named actions only (D3). The project file declares them, and
the world compiles their bindings. A Unity user's first script calls
`Input.GetKey(KeyCode.Space)`. In Fynkel, that script needs an action
declared in `fynkel.project.json` first. An action that isn't declared,
such as a typo or one they forgot to add, reads as `false` with no
message. In Unity, the same mistake throws (`Input Button X is not setup`).

## Decision

1. **Raw reads beside the actions.** `Input.getKey(name)`,
   `getKeyDown`, `getKeyUp`, `getMouseButton(0..4)`, `getMouseButtonDown`,
   `getMouseButtonUp`, `anyKey` and `anyKeyDown` read the host's controls
   directly, with no action. Key names are the binding grammar's
   (`keyNames`: physical keys, so they match on every layout). A name the
   grammar doesn't know is an `ArgumentError`, not a silent `false`.
2. **The same timing as actions (§5.1).** Held state is what every tick
   sees. A down or up edge is consumed by exactly one tick, and a frame
   with no tick carries it to the next. `update` / `lateUpdate` see the
   frame's own edges. A `reset` (focus loss, pause) releases everything.
   The raw state is kept in Dart from the events the host passes to
   `World.captureInput`, the same events the native table gets, so a
   recording replays keys and actions alike. No ABI change.
3. **Actions stay the binding source.** D3 still holds for everything
   that is *bound*. Only actions can be rebound (`settings.json`), have
   several devices, or drive a native system (the character controller,
   the camera rig). Raw keys are for quick scripts, debug keys and
   prototypes, as `Input.GetKey` is in Unity. The docs recommend actions
   for anything a player rebinds.
4. **An undeclared action warns.** Reading an action that the loaded
   project doesn't declare still returns nothing. The world now logs one
   Console warning per name (`Input action 'jupm' is not declared in
   fynkel.project.json → inputActions`). It doesn't throw, because a
   behaviour reading an optional action must not fail every frame.

## Consequences

- A first script can read `Input.getKey('Space')` without editing the
  project file.
- Two ways to read a key. The actions remain the documented default, and
  `docs/input.md` says when to use which.
- `InputService` gains the raw readers, so an implementation outside
  `ActionTable` (there is none in the repository) has to add them.
  `TestInput` gains `pressKey` / `releaseKey`.
