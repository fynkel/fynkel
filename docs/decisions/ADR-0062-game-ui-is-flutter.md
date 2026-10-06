# ADR-0062: Game UI is Flutter, written in Dart

**Status:** Accepted 2026-10-06 (the user chose "write it in Dart, as normal Flutter" over a visual designer, and accepted this as written)  
**Date:** 2026-10-06  
**Related:** blueprint §10 (Screen-space UI), ADR-0008 (Dart gameplay on the main isolate), ADR-0010 (reference game tiers: projected labels + a minimal `WorldCanvas`), ADR-0021 (Play in the editor), ADR-0028 (editor extensions), [hud.md](../hud.md), roadmap Phase 5

## Context

Phase 5 of the roadmap planned a **Game UI designer**: a `.ui.json` widget
tree edited in the Hierarchy and Inspector, bound to `GameModel`, with a
live preview and custom widgets. That would be a second way to author UI,
and a big build (a format, a palette, property editors, binding UI, a
runtime interpreter).

The engine's reason to exist on Flutter is that a game's UI *is* Flutter:
any widget, package, animation, text input and accessibility, with nothing
of its own to learn. Unity has to ship its own UI systems (uGUI, UI
Toolkit) because it has nothing underneath; we do.

What already exists (2026-10-06):

- **Composition.** `FynkelGameView.overlay` stacks the game's widgets over
  the 3D view (`packages/fynkel_flutter/lib/src/game_view.dart`); the game's
  whole app — its own `MaterialApp`, navigator and theme — comes from
  `buildGame(FynkelLaunch)` (`lib/fynkel_game.dart`).
- **Binding.** `GameModel` / `ModelValue` (`packages/fynkel`, no Flutter
  dependency) written by behaviours; `ModelBinding`, `ModelBuilder` and
  `.listenable` in `package:fynkel_flutter` rebuild only what a field
  feeds. The reference game's HUD, menus, results and settings use them.
- **Labels on the world.** `World.projection` (`ProjectionView`,
  `ScreenAnchor`, one FFI call a frame for up to 64 anchors, occlusion
  included) and `ProjectionLayer` / `ProjectedAnchor` / `WorldLabel`.
- **Editor Play shows the game's own UI.** The game runs in the editor's
  process (ADR-0021): the Game view mounts `buildGame`, so the HUD and
  menus are exactly what a build shows.
- **Hot reload.** A save in the project's `lib/` hot reloads the running
  game, widgets included (ADR-0021 §5).

## Decision

1. **Game UI is plain Flutter, written in Dart in the game's `lib/`.**
   There is no engine UI format, no UI document and no visual UI designer.
   Screens, HUDs and menus are widgets; the engine's job is to make them
   easy to connect to the game, not to replace them.
2. **Gameplay state reaches widgets through `GameModel` fields** (blueprint
   §10, unchanged): behaviours write fields, widgets listen with
   `ModelBuilder`. The engine snapshot never feeds widgets directly; the
   one exception stays screen positions, through `World.projection`.
3. **The Phase 5 Game UI designer is dropped**, and with it the after-1.0
   "general Flutter app builder" that depended on it. Revisiting it needs a
   new ADR and a reason a Flutter-writing user cannot meet in code.
4. **What Phase 5 does instead** (each its own step, each confirmed by a
   failing test first):
   1. *Templates.* `fynkel create` gains a main menu, a pause menu and a
      settings screen (the template has a HUD and a pause banner only today),
      written as ordinary widgets over the `HudModel`, so a new project
      starts with the screens every game needs.
   2. *Screen ↔ world helpers.* A point on screen to a ray into the world
      (to pick or aim at what is under a tap, with the physics raycast), and
      a one-off world point to the screen, beside the per-frame anchors.
   3. *Docs.* `docs/hud.md` becomes the game-UI guide (composition, models,
      labels, input focus between UI and game, Play and hot reload), and
      `docs/unity-parity.md` gets a UI row: Unity's Canvas → Flutter widgets,
      its world-space Canvas → projected labels / `WorldCanvas`.
5. **UI inside the 3D world.** Widgets pinned over a point (name tags,
   health bars, prompts) are projected Flutter widgets and already exist.
   UI that must live *in* the scene — in perspective, behind and in front
   of geometry, like a screen on a wall — cannot be a Flutter overlay;
   ADR-0010's minimal native `WorldCanvas` (panels, text, images, progress
   bars, basic buttons) remains the plan for it, scheduled after the steps
   above, with its own ADR.

## Consequences

- A Flutter developer has nothing new to learn for game UI; any Flutter
  package works in a game.
- Someone who does not write Flutter has no drag-and-drop UI editor. The
  templates and the guide are the answer for now; Flutter's own tooling
  (hot reload, the widget inspector in DevTools) covers layout work.
- Roadmap Phase 5 loses its largest item; `WorldCanvas` and the Tier-2
  reference game move up.
