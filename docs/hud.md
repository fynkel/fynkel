# Game UI

A Fynkel game's UI is Flutter, written in Dart in the game's `lib/`
(ADR-0062). This covers main menus, HUDs, pause and settings screens,
results, inventories and dialogs. There is no engine UI format and no
UI designer: screens are widgets, any Flutter package works, and Flutter's
own tools (hot reload, the widget inspector in DevTools) do the layout
work. The engine's part is connecting those widgets to the game:

* **Composition**: where the widgets sit over the 3D view.
* **Models**: how gameplay state reaches a widget without rebuilding
  the app every frame.
* **Screen ↔ world**: labels pinned to objects, taps that pick
  objects, world points on screen.
* **Input**: who gets a key, the UI or the game.
* **Play and hot reload**: the same UI in the editor and in a build.

`fynkel create` starts every project with the screens most games need: a
title screen, a pause menu and a settings panel (`lib/ui/menus.dart`,
switched in `lib/main.dart`, tested by `test/menus_test.dart`). Restyle
them, replace them, or add screens beside them.

## Composition

`lib/fynkel_game.dart`'s `buildGame(FynkelLaunch)` returns the game's whole
app: its own `MaterialApp` (or `CupertinoApp`, or `WidgetsApp`), theme,
navigator and routes. The 3D view is one widget in it:

```dart
FynkelGameView(
  world: game.world,
  onFrame: _onFrame,            // after each frame: read the pause action
  onKeyEvent: _onKey,           // keys the game's input does not take
  overlay: ModelBinding<HudModel>(
    model: game.hud,
    child: Stack(fit: StackFit.expand, children: [
      ProjectionLayer(world: game.world, children: [/* labels on objects */]),
      const Hud(),
      const Screens(),          // title / pause / settings, switched on a model field
    ]),
  ),
)
```

`overlay` is drawn over the viewport at the view's size, so the overlay's
coordinates are the view's. Anything else in the app (a route pushed over
the game, a drawer, a dialog) is ordinary Flutter. A game that wants its
menus on their own route can push them with the navigator. The starter
keeps everything in one overlay so the scene stays visible behind the menus.

## Input between the UI and the game

* Keys reach the game's input (`world.input`, the project's actions)
  unless a widget takes them. A focused HUD button does not take the
  game's keys; a focused text field does, so typing a name never moves
  the player (foundation 4.7, `game_view_test.dart`). `onKeyEvent` sees
  the keys the input does not handle (the starter's Q for quality,
  Escape on a menu).
* Pointer events hit the topmost widget. A menu that should block the
  game behind it fills the view (the starter's `MenuBackdrop`). A tap
  meant for the world goes through a `GestureDetector` that calls
  `pick` (below).
* **A menu takes the controls** with
  `FynkelGameViewState.setMenuOpen(true)` (ADR-0062 amendment 1). While
  it is open, the game's input gets nothing: no keys, gamepad, mouse or
  touch, and everything it held is released once. The controller drives
  Flutter's focus the way Flutter treats a TV remote:

  | Controller | Becomes | In the menu |
  |---|---|---|
  | D-pad, left stick | arrow keys (held: repeats after 0.4 s, then every 0.1 s) | move between controls; left/right adjust a focused slider |
  | south (A) | `gameButtonA` | presses the focused control (Flutter's `ActivateIntent`) |
  | east (B), menu (Start), options | `gameButtonB`, `gameButtonStart`, `gameButtonSelect` | the app's back / close, read in `onKeyEvent` like Escape |
  | north, west, shoulders | `gameButtonY`, `gameButtonX`, `gameButtonLeft1`/`Right1` | free for the app (tabs, for example) |

  The keys travel the normal focus chain, so every Material and
  Cupertino control works unchanged, and the focus highlight shows once
  the controller is used. While open, the menu uses Flutter's directional
  navigation mode (up/down move, left/right adjust), the 3D view is out
  of the focus order, and closing the menu gives the view its focus back.
  The keyboard works the same way: arrows move, Enter or Space press.
  `setMenuOpen` is separate from pausing; a menu over a running world
  (an inventory) is fine.
* A menu that stops the game also pauses the view
  (`FynkelGameViewState.setPaused(true)`): the loop and the ticks stop,
  held input is released, and the last frame stays on screen. The starter
  shows its title screen over the scene's first frame this way, and calls
  both.
* Phones: `TouchOverlay.fromActions` draws the project's
  `touch:overlay/*` bindings as a stick and buttons. They are widgets in
  the same overlay.

## Play in the editor and hot reload

The editor's Play runs the game in the editor's own process (ADR-0021).
Its Game view mounts `buildGame` with the open project and scene, so the
menus and HUD in Play are exactly what a build shows. Saving a file in
`lib/` hot reloads the running game, widgets included. `FynkelLaunch.inEditor`
(and `FynkelApplication.inEditor`) tells the game where it runs.
`FynkelApplication.quit()` ends Play in the editor and exits a build, so
never call `exit()` from a Quit button.

## Testing UI

Screens are widgets, so `testWidgets` tests them without a world or a
GPU. Use an in-memory `SettingsStore()` for a settings page
(`test/menus_test.dart` in a new project). A test that needs the game too
loads it headlessly (`Game.load`, `test/game_test.dart`) and writes the
model fields a screen binds to.

## Unity → Fynkel

| Unity | Fynkel |
|---|---|
| Canvas (Screen Space – Overlay), uGUI, UI Toolkit | Flutter widgets in `FynkelGameView.overlay` or anywhere in the app |
| `Text`, `Button`, `Slider`, `Toggle`, `InputField`, `ScrollRect`, layout groups | Flutter's `Text`, `FilledButton`, `Slider`, `Switch`, `TextField`, `ListView`, `Row`/`Column` |
| A script setting `text.text = score` each frame | a behaviour writing `hud.score.value`, a `ModelBuilder` around one `Text` |
| `Time.timeScale = 0` for a pause menu | `FynkelGameViewState.setPaused(true)` |
| `Camera.ScreenPointToRay` + `Physics.Raycast` | `FynkelGameViewState.screenRay` / `pick` |
| `Camera.WorldToScreenPoint` | `FynkelGameViewState.worldToScreen` (one-off), `ProjectedAnchor` (every frame) |
| A world-space Canvas over a character (name tag, health bar) | `WorldLabel` / `ProjectedAnchor` in a `ProjectionLayer`; for hundreds of bars, `WorldMarks` + `ProjectedMarks` |
| A world-space Canvas *in* the scene (a screen on a wall, in perspective) | not yet: ADR-0010's native `WorldCanvas`, planned after ADR-0062 |
| `EventSystem` / `Selectable` navigation, the Input System's UI module | Flutter's focus; `setMenuOpen` turns the controller into focus keys (above) |
| Switching the player's action map off while a menu is open | `setMenuOpen(true)`: the game's input gets nothing |

The rest of this page is the reference for models and world labels:
blueprint §10 and ADR-0008, implemented by M3-13. Two rules decide how
widgets learn about the game:

1. **UI state comes from Dart-side observable models that behaviours
   write, never from the engine snapshot.** Nothing walks the snapshot every
   frame to find out the score. A `Behaviour` writes the score into a
   model field when it changes, and the one widget bound to that field
   rebuilds. The whole app never rebuilds every frame.
2. **Positions on screen are the one exception.** A label pinned to a 3D
   object needs its screen position every frame. That comes from a
   bounded per-frame projection view (one native call for all anchors),
   and the widgets move by paint transforms instead of rebuilding.

`package:fynkel` holds the models and the projection view (no Flutter
import); `package:fynkel_flutter` holds the widgets. The starter project
is the worked example: `lib/game/hud_model.dart`, `Mover.update`,
`lib/main.dart`, `lib/ui/menus.dart`.

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
  stops; input is released), `hud.screen.value = Screen.paused` shows the
  menu (the starter's `PauseMenu`, `lib/ui/menus.dart`). Resume is the same
  in reverse.
* **Settings** — the `SettingsStore` (`docs/scenes.md`): widgets edit a
  draft, `settings.update(current.copyWith(...))` saves and applies rebinds;
  a `ModelValue<SettingsDocument>` (or the store's own listeners) drives the
  page (the starter's `SettingsPanel`).
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
  sensors ignored) and sets `OCCLUDED` when a collider whose object is on those layers is
  hit before the point. `world.layerMask(['walls'])` builds the mask from
  the project's `layers`. Position is unaffected — a host may fade an
  occluded label instead of hiding it.

Occlusion is by colliders, not by rendered meshes: a wall without a
collider does not occlude. That is the trade the policy makes — one ray per
masked anchor, no depth read-back from the GPU.

### The bound: 4096 anchors, `FYN-R0012`

The engine projects at most `MAX_PROJECTION_ANCHORS = 4096` anchors per
request (`ProjectionView.maxAnchors`). Beyond that the first 4096 are
projected in registration order, the rest come back `dropped` (never
placed), and the world reports `DiagnosticCode.tooManyAnchors` (15) with a
message starting `FYN-R0012` on the presentation lane — the Dart view
forwards it through `World.onDiagnostics` **once per overflow episode**
(not per frame; again after the count drops below the bound and overflows
anew). It is informational (`isInformational`), never thrown by
`throwOnDiagnostics`. The bound was 64 until ADR-0062 amendment 1: a
crowd's health bars ([many marks](#many-marks-worldmarks-projectedmarks))
need hundreds. A widget per label stays the costly part, so text labels
should still be few; a game picks which (`prune()`, distance, gameplay
relevance).

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

### Many marks (`WorldMarks`, `ProjectedMarks`)

A `ProjectedAnchor` child is a widget, which is right for a name tag, a
button over a door or a speech bubble, and too much for a health bar over
each of 500 enemies. For those, `WorldMarks` keeps one anchor per mark, and
`ProjectedMarks` draws them all in one paint. No widget is created per mark,
and nothing rebuilds when a value, a colour or a position changes:

```dart
final bars = WorldMarks(
  world,
  style: const MarkStyle.bar(size: Size(40, 6), color: Colors.red, background: Color(0x99000000)),
  offset: const Vec3(0, 2, 0), // over each object's head
);
final bar = bars.add(enemy);   // a WorldMark
bar.value = health / maxHealth; // 0 to 1: repaints, no rebuild
bar.color = Colors.orange;      // this mark only
bar.remove();                   // when the enemy goes

// In the overlay, sized like the view:
Positioned.fill(child: ProjectedMarks(marks: bars)),
```

* `MarkStyle.bar(size, color, background, cornerRadius, alignment, nudge,
  maxDistance)` is a bar filled from the left to the mark's value.
  `MarkStyle.dot(radius, color, nudge, maxDistance)` is a dot centred on
  the point, for minimap-style blips or "something is here". With
  `maxDistance` set, marks farther from the camera than that are skipped.
* `WorldMarks(world, style, offset, occlusionMask)` has `add(target,
  {value, color, offset})`, `remove`, `prune()` (marks over destroyed
  objects), `clear()`, `marks`, `length` and `dispose()`, which removes
  every anchor. It is a `ChangeNotifier` that fires on the view's updates
  and on any mark change.
* `WorldMark` has `value`, `color`, `hidden`, `anchor` (its `ScreenAnchor`),
  `target` and `remove()`.
* Marks are not drawn while their object is off screen, behind the camera,
  occluded (with a mask), destroyed or `hidden`. They take no taps: a tap
  on a bar reaches the game.

Marks share the 4096-anchor bound with every `ProjectionLayer`. On the
test host, updating 500 anchors takes about 0.6 ms a frame (JIT). The
iPhone figure is ADR-0062 amendment 1's step 4.

### Screen ↔ world, one point at a time (ADR-0062)

Beside the per-frame anchors, three one-off questions, answered from the
frame's camera and interpolated pose with the renderer's matrices:

```dart
// In the game's overlay: a tap picks what it shows.
GestureDetector(
  behavior: HitTestBehavior.translucent,
  onTapUp: (d) {
    final view = gameView.currentState!; // GlobalKey<FynkelGameViewState>
    final hit = view.pick(d.localPosition, mask: world.layerMask(['props']));
    if (hit != null) selected.value = hit.entity;
  },
)
```

* `FynkelGameViewState.screenRay(Offset)` — the ray through a position in
  the view (its logical pixels from the top left, what a gesture's
  `localPosition` over the view is): Unity's `Camera.ScreenPointToRay`. A
  `ScreenRay { camera, origin, direction, at(distance) }`, starting on the
  near plane; parallel rays for an orthographic camera.
* `FynkelGameViewState.pick(Offset, {maxDistance, mask, hitSensors})` —
  that ray into `World.raycast`: the nearest collider under the point, or
  null.
* `FynkelGameViewState.worldToScreen(Vec3, {occlusionMask})` — where a
  world point is drawn in the view now: a `ScreenPoint { x, y, depth,
  inFront, onScreen, occluded, visible }`, `.offset` for Flutter. For a
  widget that follows something every frame, use a `ProjectedAnchor`.

All three return null without an enabled camera. The same calls exist on
`World.projection` in `package:fynkel` (`screenRay(x, y)`,
`pick(x, y)`, `worldToScreen(point)`) for code without a view, such as a
behaviour or a headless test. They use the view's `setViewport` size, or
the `width`/`height` passed. Natively these are `World::screen_ray` /
`fynkel_world_screen_ray`, and a projection anchor with a null entity,
which is a point in the world.

Tests: `packages/fynkel/test/projection_test.dart` (the view against the
fixture scene: positions, interpolation, off-screen and behind, the
occlusion policy, pending and dead objects, the bound and its one
diagnostic, no camera, no anchors → no call), `test/model_test.dart`;
`packages/fynkel_flutter/test/projection_test.dart` (**the gate**: a
`WorldLabel` sits at the fixture crate's projected point while a scripted
camera pans, with zero rebuilds, and is neither painted nor hit-testable
once the crate is culled or behind the camera; clamped markers; children
diffing; `FynkelGameView` updating the view every frame),
`test/projected_marks_test.dart` (500 bars over a crowd: one painter, two
rounded rectangles per visible bar at its point, a value change and a
moving crowd repainted with no rebuild, a destroyed object's bar gone;
dots with per-mark colours, hidden marks, the distance cut-off and no hit
testing), `test/model_binding_test.dart`; the starter's `test/game_test.dart` ('the
mover writes the HUD model; a label follows the cube'); `crates/fynkel_core/
tests/projection.rs`, `crates/fynkel_native/tests/abi.rs`
(`projection_of_world_anchors`, `screen_point_to_ray`);
`packages/fynkel_flutter/test/screen_world_test.dart` (a position in the
view picks the Floor point drawn there).
