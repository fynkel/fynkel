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
- **L2 — See it.** `PlayDocument`; the Hierarchy and Inspector switch
  during Play, read-only; selection carried both ways; the play-mode tint
  and the header. Measure the refresh cost.
- **L3 — Change it.** Editing native components and behaviour fields,
  add/remove component, rename and reparent; at once when paused.
- **L4 — Keep it.** Apply to Scene / Apply Field to Scene, prefab
  overrides included.
- **L5 — Fly around it.** The Scene view on the running world: the
  editor camera override, picking, gizmos.

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
   last.
