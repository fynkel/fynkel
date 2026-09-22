# HUD models and world labels

The screens of a Fynkel game — main menu, HUD, pause, settings, results —
are ordinary Flutter widgets drawn over the viewport. Two rules from
blueprint §10 and ADR-0008 decide how they learn about the game:

1. **HUD state comes from Dart-side observable models that behaviours
   write, never from the engine snapshot.** Nothing walks the snapshot per
   frame to find out the score; a `Behaviour` writes the score into a model
   field when it changes and the one widget bound to that field rebuilds.
   The whole app never rebuilds per frame.
2. **Projected world labels are the one exception**: a label pinned to a
   3D object needs its screen position every frame. That comes from a
   bounded per-frame projection view — one native call for all anchors —
   and moves widgets with paint transforms, without rebuilding them.

Implemented by M3-13 (blueprint §19.2d item 13). `package:fynkel` holds the
models and the projection view (no Flutter import); `package:fynkel_flutter`
the widgets. The starter project (`fynkel create`) is the worked example:
`lib/game/hud_model.dart`, `Mover.update`, `lib/main.dart`.

## Observable models (`GameModel`, `ModelValue<T>`)

```dart
class HudModel extends GameModel {
  late final score = field(ModelValue(0));
  late final health = field(ModelValue(100));
  late final screen = field(ModelValue(Screen.menu));
}
```

* `ModelValue<T>` has the `ValueNotifier` shape without the Flutter
  dependency: `value` (assigning notifies the listeners **only when `!=`**,
  synchronously, on the caller's stack), `force(v)` (notify even when equal —
  a mutable value edited in place), `notify()`, `addListener`/
  `removeListener`, `hasListeners`, `dispose()` (drops the listeners; later
  writes are ignored).
* `GameModel` is the base of a screen's model: `field(v)` registers a
  field (returns it, for `late final`), `fields`, `notifyAll()`, `dispose()`.
  A `late final` field registers on its first access — a field nobody has
  touched has no listeners, so `notifyAll`/`dispose` lose nothing; one first
  touched after `dispose` is disposed at once.
* **Where it lives:** install one instance in the world's context —
  `world.context.install(HudModel())` in the game's loader, read back with
  `world.context.service<HudModel>()` (the starter's `Game.hud`). A
  behaviour reaches it through `world.context.serviceOrNull<HudModel>()`.
* **Who writes it:** behaviours, from `update` (after the frame's ticks,
  once per frame — the tick-rate state is summarised there, not written
  60× per second), or from the callbacks that know the fact (`onZoneEnter`,
  a `SceneManager` listener, the settings store). The app writes app facts
  (`paused`, the current screen). Writes are cheap when nothing changed:
  `hud.position.value = transform.position` every frame notifies only on the
  frames the object moved.

Binding in Flutter (`package:fynkel_flutter`):

```dart
ModelBinding<HudModel>(model: game.hud, child: …)          // hands the model down
final hud = ModelBinding.of<HudModel>(context);              // in a build method
ModelBuilder<int>(value: hud.score, builder: (_, s, _) => Text('$s'))
ValueListenableBuilder(valueListenable: hud.health.listenable, builder: …)
```

* `ModelBinding<M>` is an `InheritedWidget`; it notifies dependents only
  when the model *instance* is replaced. Field changes never go through it.
* `ModelBuilder<T>` subscribes to one field and rebuilds its `builder` when
  the field notifies (several writes in one frame: one rebuild); `child` is
  passed through untouched. It follows a swapped `value` and unsubscribes on
  dispose.
* `ModelValue<T>.listenable` is the field as a `ValueListenable<T>` for
  `ValueListenableBuilder`, `AnimatedBuilder`, `Listenable.merge`.

### The screen pattern (menu / HUD / pause / settings / results)

One model per screen (or one model with a `screen` field), the widget tree
switching on model fields, the game's own state left in the world:

* **Main menu** — a `screen` field the app sets; the world may be idle
  (`FynkelGameViewState.setPaused(true)`, or no scene loaded yet). "Play"
  calls `world.scenes.transition('level1')` and flips `screen` when the
  future completes (`docs/scenes.md`).
* **HUD** — fields the behaviours write from `update`; each bound to its
  own `ModelBuilder`, so a score change rebuilds one `Text`. The stats
  block of the starter refreshes once a second from `FynkelGameView.onStats`
  — that is diagnostics, not game state.
* **Pause** — the `pause` action read by the app in `onFrame`
  (`world.input.pressed('pause')`), `setPaused(true)` on the view (the loop
  stops; input is released), `hud.paused.value = true` shows the overlay
  (the starter's `PauseBanner`). Resume is the same in reverse.
* **Settings** — the `SettingsStore` (`docs/scenes.md`): widgets edit a
  draft, `settings.update(current.copyWith(...))` saves and applies rebinds;
  a `ModelValue<SettingsDocument>` (or the store's own listeners) drives the
  page.
* **Results** — the behaviour that decides the round is over writes the
  totals into the model *then* transitions (`world.scenes.transition(
  'results')`); the results screen binds to the same fields. Nothing reads
  the old scene's entities after the transition.

Never: rebuild the widget under `FynkelGameView` per frame, `setState` from
a behaviour every tick, or read entity transforms in `build`.

The reference game (`examples/reference_game/lib/main.dart`, `lib/ui/`) is
the pattern built (M3-15): `GameState.screen` is one `ModelValue<Screen>`
set from a `SceneManager` listener (the scene decides the screen, not the
button), the app's `ModelBuilder<Screen>` switches menu / playing /
results, `GameState.lastRun` is written by `Game.finish()` before the
transition, the settings panel edits `SettingsDocument` and captures a
rebind's key in `FynkelGameView.onKeyEvent`, and the whole overlay sits
in `ExcludeFocus` so a button never takes the viewport's focus. Each
button plays its click on the presentation lane.

## Projected world labels (`ProjectionView`, `ProjectionLayer`)

A world label is a Flutter widget placed where a point attached to an
entity lands on screen: a name over a character, an icon over a pickup, a
marker on the edge for something off screen.

### The view (`World.projection`, `package:fynkel`)

```dart
final anchor = world.projection.add(player, offset: const Vec3(0, 2, 0));
world.projection.setViewport(size.width, size.height);   // once, from layout
world.projection.update();                                 // once per frame, after World.frame
if (anchor.visible) place(anchor.x, anchor.y);
```

* `add(target, {offset, occlusionMask})` → `ScreenAnchor`; `target` is a
  `GameObject` (pending ones answer `missing` until spawned) or an entity
  handle; `offset` a point in the entity's local frame. `remove(anchor)` /
  `anchor.remove()`, `prune()` (drops every anchor whose object is dead),
  `clear()`, `anchors`, `length`, `isEmpty`.
* `update()` is **one FFI call for every anchor** (`fynkel_world_project`),
  answered by the world from the frame's camera and the same interpolated
  poses the renderer draws (`clock.alpha()`, the M2 interpolation). Call it
  after `World.frame`; `FynkelGameView` does so on every tick when the view
  is not empty. It returns the number of anchors projected, bumps
  `version`, and runs the `addListener` listeners once **when something
  changed** — an anchor moved, was re-flagged, appeared or was dropped; a
  still frame (the camera and every anchored object at rest) notifies
  nothing, so an overlay bound to the view repaints only when it must
  (the M3 allocation gap: a label layer repainting at 120 Hz was a
  measurable share of a frame's Dart allocation). The answers are read
  from the call's buffer in place; the batch is reused. An empty view
  makes no call. `camera` is the entity projected from (null: no enabled
  `fynkel.Camera` in the world — the viewport's fallback camera is not
  projected).
* The viewport is in the caller's pixels — logical pixels for a Flutter
  overlay, so the answers are widget positions; the aspect ratio is what
  shapes the projection, the scale is the caller's (the device pixel ratio
  only matters to the texture).
* **Camera:** the one `extract` renders from — the enabled `fynkel.Camera`
  with the highest `priority`, ties to the earliest entity — with the
  renderer's own matrices (`fynkel_core::view_matrix`, rotation and
  translation only, and `projection_matrix`, perspective or orthographic).
  A label sits where the mesh is drawn.

`ScreenAnchor` after an update:

| Field | Meaning |
|---|---|
| `x`, `y` | viewport pixels, origin top-left, y down (meaningful while `inFront`) |
| `depth` | metres in front of the camera along its view axis (negative behind) |
| `inFront` | in front of the near plane: `x`/`y` are a place on the screen plane |
| `onScreen` | in front and inside the viewport rectangle |
| `occluded` | a collider on `occlusionMask` sits between the camera and the point |
| `missing` | the entity is dead, not spawned yet, or has no transform |
| `noCamera` | the world had no enabled camera |
| `dropped` | beyond the 64-anchor bound (below) |
| `visible` | `onScreen && !occluded && !dropped` — show the label here |
| `flags` | the raw `PROJECTED_*` bits (`fynkel.rec.ProjectedPoint.flags`) |

Wire (`docs/codegen.md`): `fynkel.rec.ProjectionAnchor { entity, offset,
mask }` in a `FYNKEL_BUFFER_PROJECTION_ANCHORS` (9) buffer,
`fynkel.rec.ProjectedPoint { entity, x, y, depth, flags }` back in a
`Query` buffer, one per anchor in order; flags `PROJECTED_IN_FRONT` 1,
`ON_SCREEN` 2, `OCCLUDED` 4, `MISSING` 8, `NO_CAMERA` 16, `DROPPED` 32.
Hosts: `fynkel_world_project(world, width, height, buffer, len, &info,
&view)` (`docs/hosts.md`).

### Depth occlusion is an explicit policy

A screen overlay is not automatically behind the 3D scene, and the
blueprint asks for a policy rather than a surprise. The policy is **per
anchor, by collider mask**:

* `occlusionMask == 0` (the default): the label is **never hidden by
  geometry**. A name over a character stays readable through a wall.
* `occlusionMask != 0`: the world casts a ray from the camera eye to the
  point (`Physics::query` Raycast, the anchor's own entity excluded,
  sensors ignored) and sets `OCCLUDED` when a collider on those layers is
  hit before the point. `world.layerMask(['walls'])` builds the mask from
  the project's physics layers. Position is unaffected — a host may fade an
  occluded label instead of hiding it.

Occlusion is by colliders, not by rendered meshes: a wall without a
collider does not occlude. That is the trade the policy makes — one ray per
masked anchor, no depth read-back from the GPU.

### The bound: 64 anchors, `FYN-R0012`

The engine projects at most `MAX_PROJECTION_ANCHORS = 64` anchors per
request (`ProjectionView.maxAnchors`). Beyond that the first 64 are
projected in registration order, the rest come back `dropped` (never
placed), and the world reports `DiagnosticCode.tooManyAnchors` (15) with a
message starting `FYN-R0012` on the presentation lane — the Dart view
forwards it through `World.onDiagnostics` **once per overflow episode**
(not per frame; again after the count drops below the bound and overflows
anew). It is informational (`isInformational`), never thrown by
`throwOnDiagnostics`. Sixty-four labels on screen at once is already too
many to read; a game that wants more chooses which (`prune()`, distance,
gameplay relevance) — the bound is the design, not a limit to raise.

### The widgets (`package:fynkel_flutter`)

```dart
FynkelGameView(
  world: game.world,
  overlay: Stack(children: [
    ProjectionLayer(world: game.world, children: [
      WorldLabel(target: player, offset: const Vec3(0, 2, 0), text: 'You'),
      ProjectedAnchor(
        target: objective,
        alignment: Alignment.center,
        clampToEdges: true,
        child: const Icon(Icons.flag),
      ),
      WorldLabel(target: enemy, text: 'Enemy', occlusionMask: world.layerMask(['walls'])),
    ]),
    const Hud(),
  ]),
)
```

* `ProjectionLayer(world, children)` — owns the `ScreenAnchor`s of its
  children (registered while a child is in the list, removed after; an
  unchanged `target` keeps its anchor, `offset`/`occlusionMask` are updated
  in place), reports its own size to the view (`LayoutBuilder` →
  `setViewport`), and moves the children with a `Flow`: the flow delegate
  repaints on the view's listener (only when an anchor changed, above) and
  positions each child by a paint transform, so **no widget rebuilds per
  frame**; each child sits in its own `RepaintBoundary`, so a moved label
  is re-composited, not re-painted; the layer rebuilds only when its
  children change. A child that is not `visible` is not painted
  and not hit-testable. Size the layer like the viewport it overlays (a
  `Stack` child of `FynkelGameView.overlay` fills it).
* `ProjectedAnchor(target, offset, occlusionMask, alignment, nudge,
  clampToEdges, child)` — `alignment` says which point of the child sits on
  the projected point (`Alignment.bottomCenter`, the default, floats the
  child above it; `center` centres it), `nudge` shifts by logical pixels,
  `clampToEdges` keeps a child whose point is in front of the camera but
  outside the viewport on the nearest edge (an objective marker) — behind
  the camera it still hides (an off-screen *direction* indicator is a game's
  own widget over the anchor's `depth`/`x`/`y`).
* `WorldLabel(target, text, style, background, padding, borderRadius, …)`
  — a `ProjectedAnchor` with a small text pill.

Tests: `packages/fynkel/test/projection_test.dart` (the view against the
fixture scene: positions, interpolation, off-screen and behind, the
occlusion policy, pending and dead objects, the bound and its one
diagnostic, no camera, no anchors → no call), `test/model_test.dart`;
`packages/fynkel_flutter/test/projection_test.dart` (**the gate**: a
`WorldLabel` sits at the fixture crate's projected point while a scripted
camera pans, with zero rebuilds, and is neither painted nor hit-testable
once the crate is culled or behind the camera; clamped markers; children
diffing; `FynkelGameView` updating the view every frame),
`test/model_binding_test.dart`; the starter's `test/game_test.dart` ('the
mover writes the HUD model; a label follows the cube'); `crates/fynkel_core/
tests/projection.rs`, `crates/fynkel_native/tests/abi.rs`
(`projection_of_world_anchors`).
