# Live Play mode (ADR-0021 P7) — design note

**Status:** Accepted (2026-09-23, the user: "let's go with that order",
taking the three recommendations below). Its steps L1–L5 are the P7
backlog.
**Related:** ADR-0021 §8 ("nothing a run does ever writes a document,
except an explicit apply to source"), ADR-0016 (authoring service,
edit-preview world), ADR-0008 (one isolate steps the world), ADR-0011
(native components for the many, Dart behaviours for the few), ADR-0012
(schemas are the source of truth).

## What the user gets (Unity's Play mode)

While the game runs in the Game view:

- **The Hierarchy shows the running world**: the scene's objects as they
  are now, plus what the game spawned (bullets, coins, a prefab instanced
  at run time), minus what it destroyed. Names, parents and order are
  live.
- **The Inspector shows live values**: a position that moves, a health
  that goes down, an animator's state. The values refresh about ten times
  a second. A field you are editing is never overwritten under you.
- **You can change the running game**: type a value, drag a number, tick a
  box, add or remove a component. The game reacts on its next frame, and
  at once when paused.
- **Stop throws it all away.** The scene documents are exactly as before
  Play. This is automatic: the run's world is disposed on Stop, and
  nothing in live mode ever writes a document.
- **Keep a change on purpose**: **Apply to Scene** on a component (or on
  one field) writes the live value into the open scene document as one
  undoable edit. The tab goes dirty; you save as usual. Unity makes you
  copy the component and paste it after Stop; this is one step.
- **You always know you are in Play mode**: the editor is tinted while a
  run is up (Unity's play-mode tint; a colour in Preferences, off
  possible). The Hierarchy's header says *Playing — changes are lost on
  Stop*.
- **Selection carries over**: what was selected in the scene before Play
  is selected in the live world, and back after Stop, for every object
  that came from the scene.
- Later (L5): **the Scene view shows the running world** through the
  editor camera, so you can fly around the playing game, pick objects and
  move them with the gizmos.

## What exists today (researched 2026-09-23)

- **The world is the game's.** The game creates its `World` in its own
  code (`World.create` in `Game.load`) and hands it to `FynkelGameView`,
  which "does not own it". The editor mounts `buildGame(launch)` and has
  **no handle** to that world: `FynkelPlayScope` carries only pause/step.
- **One thread.** `FynkelGameView` drives `world.frame` from a Flutter
  ticker on the UI isolate, the editor's own isolate. Reading or writing
  the world between two frames needs no lock.
- **Reading** (ABI queries, one FFI call each): `ENTITIES` (every entity
  with parent, name, layer, tags — a whole hierarchy table in one call),
  `COMPONENTS` (an entity's native components as wire records), plus
  transform, body, character, animator, audio. `package:fynkel` wraps
  them (`allEntities`, `componentsOf`).
- **Authoring ids exist but are not exposed.** The core keeps the full
  runtime-entity ↔ authoring-id map per loaded scene (`LoadedScene`
  `by_id`/`by_entity`); the ABI does not return it. Prefabs instantiated
  at run time do not keep theirs; plain spawns have none.
- **The authoring JSON form exists in Rust only.** `encode_component` /
  `decode_component` (`fynkel_document::generated`) turn a native
  component into the scene document's JSON and back; `fynkel state dump`
  and the edit-preview's `dump` use them. Dart's generated codecs are wire
  only.
- **Writing**: the `SetComponent` command replaces (or adds) a whole
  native component; `RemoveComponent`, `SetName`, `SetParent`,
  `SetTransform`, `Destroy` exist. Commands apply at the next fixed tick —
  not while paused.
- **Behaviours live in Dart only**, as fields of `Behaviour` objects.
  Their generated `BehaviourType.fields` already read and write each field
  by name in JSON form (`get`/`set`), but an instance does not record its
  type.
- **The Scene view draws the edit-preview world.** The viewport can draw
  any world, but the editor camera works only on edit-mode worlds (a game
  world renders through its own camera).
- **The editor's panels read an interface, not the files.** The
  Hierarchy and the Inspector work over `EditorDocument` (roots,
  entities as authoring JSON, `apply(commands)`); the tests run them over
  `FakeDocument`.

## Decisions proposed

1. **The running world is shown through a `PlayDocument`.** It
   implements the same `EditorDocument` interface as a scene, backed by
   the running world instead of a file: `roots()`/`entities()` from the
   live queries, `apply()` translating the Inspector's commands into
   runtime commands. During Play the Hierarchy and the Inspector switch
   to it; after Stop they switch back to the scene's tab. The panels,
   field widgets, specs and hints are reused as they are. A `PlayDocument`
   is never dirty, has no undo history worth keeping (Stop drops it) and
   can never be saved.

2. **The editor gets the world from the scope, not from the game.**
   `FynkelGameView` registers the world it shows with the enclosing
   `FynkelPlayScope` (`control.attach(world)` / `detach`). Games change
   nothing. A game showing several worlds registers each; live mode shows
   the first (a picker later if a game needs it).

3. **One live ABI call in the authoring feature.**
   `fynkel_world_live_call(world, json)` — present only in editor builds,
   like `fynkel_authoring_*` — with:
   - `entities`: the hierarchy table with each entity's **authoring id**
     (from the loaded scenes' maps; runtime-spawned entities get
     `runtime:<handle>` and a `spawned` flag);
   - `components(entity)`: its native components **in the document's
     JSON** (`encode_component`), so the Inspector reads them exactly as
     it reads a scene;
   - `setComponent(entity, type, json)` / `removeComponent`: through
     `decode_component` and the ordinary commands;
   - `applyNow`: runs the queued structural commands **without stepping**
     the simulation, so an edit shows while paused. The core gains that
     entry point (the edit-preview's sync steps an edit-mode world, which
     ticks nothing; a running world must not tick for an edit).
   Instantiated prefabs keep their id map (the core returns it and
   drops it today), so an instance's children carry `<anchor>/<local id>`
   ids.

4. **Behaviours are read and written in Dart.** `registry.instantiate`
   records each instance's type; the `PlayDocument` shows a behaviour's
   fields through `BehaviourType.fields` `get` and writes them with
   `set`. No native change.

5. **Refresh by polling, at the panels' pace.** While the Hierarchy is
   visible: the entity table about 10 times a second, compared by bytes
   (it is already fetched and compared per tick for `find`). While the
   Inspector is visible: the selected entities' components at the same
   rate. Nothing is read for a hidden panel. A paused game is read once
   after each Step. Target: under 0.5 ms per refresh for the reference
   game's Playground (169 entities); measured in L2.

6. **Edits apply to the next frame, or at once when paused.** A field
   edit becomes `setComponent` with the whole component (read, change the
   field, write), or a behaviour's `set`. A drag is one edit per frame
   while dragging. What the game itself writes next frame wins, as in
   Unity: editing a position the game sets every frame does nothing
   visible.

7. **Apply to Scene writes the document, and only on request.** The
   Inspector's component menu gets **Apply to Scene**, and each field's
   context menu **Apply Field to Scene**. They are enabled for entities
   that came from the open scene (an authoring id, not `runtime:`). They
   run one ordinary transaction (`setField`s, or `addComponent` /
   `removeComponent`) on the scene's tab, which goes dirty. A prefab
   instance's entity writes an override, as editing it outside Play
   would. Nothing else in live mode touches a document.

8. **Scene documents stay editable during Play, in their own tab.** The
   scene's tab keeps its document; the Hierarchy and Inspector follow
   whichever is focused: the Game view (and the live Hierarchy) show the
   `PlayDocument`, the scene tab shows the file. P5's "changes apply on
   the next Play" banner stays for document edits. (Unity forbids nothing
   either; it just loses the Play-mode edits.)

9. **The Scene view shows the running world last (L5).** It needs the
   editor camera to work on a game world: a per-viewport camera override
   in the renderer, used for both edit-mode and game worlds, which also
   retires `set_edit_camera`'s edit-mode-only rule. Picking uses the live
   world's physics-free pick (the preview's pick generalised). Gizmos
   move live objects through decision 6.

## Steps

- **L1 — Plumbing.** `FynkelPlayScope` world registration; the core's
  `apply_now` and kept prefab id maps; `fynkel_world_live_call`
  (entities with ids, components as JSON, set/remove, applyNow);
  `LiveWorld` in `package:fynkel_authoring`; behaviour instances know
  their type. Tests: Rust (ids for loaded, instanced and spawned
  entities; JSON round trip of every native component; applyNow ticks
  nothing), Dart over the reference game in `flutter test`.
  **Done 2026-09-23** (`1a77e15` core, `5e780f1` authoring + native,
  `7ad4b3a` `LiveWorld`, `3b927ca` behaviours, `84c5606` play scope), with
  three changes from the plan: every write (`set`, `remove`, `rename`)
  applies at once through `World::apply_now`, so there is no separate
  `applyNow` call; an instance the game makes at run time keeps no id map
  (its entities are `runtime:<handle>`; only what a loaded scene spawned,
  cooked prefab instances included, has authoring ids); a behaviour also
  records the component id it was authored as (`authoredComponentId`),
  which Apply to Scene needs. A paused Game view not redrawing after a
  live write was left to L3 (done there).
- **L2 — See it.** `PlayDocument`; the Hierarchy and Inspector switch
  during Play, read-only; selection carried both ways; the play-mode tint
  and the header. Measure the refresh cost.
  **Done 2026-09-23** (`5f4fd33` authoring, `b2d5096` `LiveWorld`,
  `5fabfa8` editor). Refresh on the reference game's Playground (180
  entities once loaded, not 169), release native library, `flutter test`
  JIT: median 0.14 ms with nothing selected, 0.19 ms with one entity,
  0.26 ms with five. Where it differs from decision 5: the table is not
  compared by bytes in Dart — sending it took ~0.6 ms alone — but hashed
  by the world (`entities { since }` answers `unchanged` without rows), so
  a refresh after a spawn, destroy, rename or re-parent still costs
  ~0.8 ms; the poll runs while Play runs whether or not the panels are
  visible, and at the same rate when paused. The measurement had no Dart
  behaviours registered (their fields are read only for selected
  entities). Also fixed on the way: a game that makes its world in
  `initState` sent the Play log a record during a build.
- **L3 — Change it.** Editing native components and behaviour fields,
  add/remove component, rename and reparent; at once when paused.
  **Done 2026-09-23** (`a354783` `FynkelPlayControl.redraw`, `d8b7213`
  the live `parent` and `destroy` calls, `ad5bb1e` `LiveWorld`, `f88f4c2`
  editor). The play document takes the editor's own commands — `setField`
  (a name or a pointer), `unsetField`, `assignAsset`, `addComponent`,
  `removeComponent`, `renameObject`, `reparentObject` (with its index; the
  Hierarchy's `setField`s keep the world pose), `deleteObject` (the
  subtree goes) — and writes each to the world at once; scripts through
  their field setters, `addBehaviourFromData` and `destroy`. Every write
  redraws a paused Game view. A gizmo drag moves the live Transforms and
  puts them back before its transaction writes the end, as the edit
  preview does. Where it differs: delete is in (the note had rename and
  reparent only); a transaction stops at its first refusal and keeps
  what went before (the world has no undo); create, duplicate, prefab
  commands and undo are refused while the game runs; a removed script
  goes at the world's next step and is hidden until then; a rename does
  not reach `GameObject.name`, which is fixed at spawn in package:fynkel.
- **L4 — Keep it.** Apply to Scene / Apply Field to Scene, prefab
  overrides included.
  **Done 2026-09-23** (`0a456d3`). The component menu's Apply to Scene
  and a field row's right click (on its label) Apply Field to Scene,
  offered while the Inspector shows the running game, for entities the
  open scene has. One transaction on the scene's tab: only the keys that
  differ (a key the scene leaves out compared with the type's default),
  or an `addComponent` for a component the scene lacks; an entity of a
  prefab instance gets `setField`/`addComponent` overrides on its anchor,
  its expansion fetched when the scene's tree has not needed it yet.
  Where it differs: numbers a millionth apart (of their size, above one)
  are the same value — the world's floats and its renormalised
  rotations otherwise make every component look changed — and native
  numbers are written as the shortest decimal of their float; the
  default Transform the world gives an entity the scene has none for is
  not added. Removing a component, a rename, a new parent or a delete is
  not applied (the menus are on what is there). Checked on the
  reference game's Playground one step in: of 497 components only those
  that moved (animation, physics, spinners, the oscillator) have
  anything to apply.
- **L5 — Fly around it.** The Scene view on the running world: the
  editor camera override, picking, gizmos.
  **Done 2026-09-23, ahead of L3 and L4** (the user's call after trying
  L2: the Scene view not showing the game was the first thing missed).
  `7660eb8` platform (viewport ABI v7: `fynkel_apple_viewport_set_view`,
  a viewport's own camera over any world), `c41f1e9`
  `FynkelViewportController.view`, `f69011f` the live `pick`, `pickRect`,
  `lines` and `worldMatrix` calls (the preview's per-entity code, shared),
  `69e3962` `LiveWorld`, `8c4ec1f` editor. During Play the Scene view
  draws the game's world every frame through its own camera — the
  scene's, kept from before Play — and a click or marquee selects in the
  running game, outlined, with the icons. Where it differs from decision
  9: `set_edit_camera` stays as it is (the per-viewport camera is added,
  nothing retired); a hit names the entity itself, since a running prefab
  instance has no anchor to stand for it; the editor's hidden flags do
  not hide anything in the game's world; gizmo handles showed but did
  not drag until L3. Cost on the Playground (180 entities, release native,
  JIT): the outline 0.05 ms a frame, with the icons (895 lines) 1.1 ms —
  the same call the edit Scene view makes, now every frame while the
  game runs.

Each step is usable on its own: after L2 you can watch the game's state,
after L3 tune it live, after L4 keep what works.

## Not in this design

- Undo inside a run (Unity has none for Play-mode edits either; Stop is
  the undo).
- Editing a Build and Run game (another process; ADR-0016's channel stays
  one-directional).
- Recording and scrubbing a run (a later "timeline" idea).
- Custom inspectors from game code (ADR-0018's open question).

## The user's answers (2026-09-23)

1. **The tint**: the whole editor, a subtle colour, which can be turned
   off in Preferences.
2. **Apply to Scene** writes the scene document at once; its tab goes
   dirty during Play.
3. **Order**: L1–L4 first, the Scene view on the running world (L5)
   last. Changed after L2 (same day): L5 next, then L3, then L4.
