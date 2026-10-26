# Dart core audit (stage 14)

Reader: Dart framework core (packages/fynkel/lib/src: game_object, world, world_loop,
world_objects, components, systems, routines, signals, pool, time, input, pointing,
keys, layers, view, assets, resources, world_resources, native, error, frame_list,
build_plan, hooks, testing), branch stage-14 @ edbe57aa. Read-only, nothing was run.
All line numbers refer to that commit. I checked the seams against particles.dart,
animator.dart and fynkel_physics/lib/src/physics.dart.

---

## DC1: Behaviour.onLoad can run twice
- Severity: bug. Size: S.
- Where:
  - world_objects.dart:299-308 (`_componentAdded`)
  - world_objects.dart:227-237 (`_runLoad`)
  - world_objects.dart:240-257 (`_load`)
- How it shows:
  1. A child is described before its root becomes live (it was given to the constructor, so it isn't live yet).
  2. Something adds a behaviour to that child during an earlier onLoad, e.g. the root's onLoad does `_body.addComponent(Glow())`.
  3. `_componentAdded` sees an object that hasn't appeared, so it does `_pending++` and calls `_load(glow)`. That is the first onLoad.
  4. When the attach walk reaches the child, `_runLoad(child)` collects every behaviour that isn't `_removed`, with no "already loading/loaded" guard. Glow is loaded again: the second onLoad, and one more `_pending`.

  Anything onLoad does then happens twice: two children added, two subscriptions. Because the `_pending` count follows both calls, appearing still balances, which is why nothing has caught this.
- First fix: give Behaviour a `_loadStarted` flag. Set it in `_load` and have `_runLoad` skip behaviours that have it. Snapshotting the behaviours at `_attach` time would also work.
- Failing test: a root whose constructor adds child `b`. The root's onLoad does `b.addComponent(Counter())`. After `world.add(root)` and a pump, expect `Counter.loads == 1`; today it is 2.

## DC2: An object reparented while still loading never starts, but is drawn
- Severity: bug. Size: S/M.
- Where:
  - world_objects.dart:347-366 (`_appearIfDone`, which calls `_refresh` only when `waiter == null`)
  - world_objects.dart:31-51 (`_setParent`, which leaves `_waiter` alone)
- How it shows:
  1. P's async onLoad adds X, and X's behaviour has an async onLoad, so X waits on P.
  2. Before either finishes, the game sets `X.parent = null`, or puts X under a live Q.
  3. When X's load completes, `_appearIfDone(X)` sees a waiter and doesn't refresh.
  4. When P completes, P refreshes P's subtree, which no longer holds X.
  5. X's `_enabled` stays false, so `_callEach` skips it: no onEnable, onStart or update, ever. The engine still has X active, so it is drawn.
- First fix: in `_setParent`, when a loading object leaves its waiter's subtree, either hand its wait to the new root (or to none) or refresh it once it is done. Alternatively, `_appearIfDone` refreshes the object itself whenever it is no longer below its waiter.
- Failing test:
  1. P has async onLoad(`await null; add(X)`).
  2. X has a behaviour with async onLoad, and `X.parent = null` is set right after the add.
  3. Pump 5 frames and expect X's behaviour to have seen onStart and an update. Today it sees neither.

## DC3: Animators make every frame look like "everything moved"
- Severity: slow. Size: M. The magnitude is unmeasured.
- Where:
  - animator.dart:431-443 (`_animate`)
  - systems.dart:340-344 (`_logMoved`)
  - physics.dart:628-643 (`_followMoved` fallback)
  - pointing.dart:96 (`transformVersion` check)
- How it shows:
  - `_animate` bumps `_transformVersion` every frame as long as any animator exists, even when none is playing.
  - It also logs every part (bone/child) of every moving animator into the moved log.
  - With physics present, `_followMoved` compares the moved count with colliders plus bodies. The animated parts alone (50 characters × ~60 parts = 3000 entries) push it into the full-scan branch, so every body and collider is chain-stamp checked each frame, though none of them moved.
  - The pointer's scene cache, and anything else keyed on `transformVersion`, is invalidated every frame.
- First fix:
  - Bump the version only when `_moving > 0`.
  - Log only parts that something follows: a per-object "followed" flag set by systems when they attach a component to the part or to something below it. Parts that carry no components and have no such descendants stay out of the log.
- Failing test (bench): 50 animated characters plus 2000 static colliders. Assert physics `_follow` per frame stays O(moved followed objects), e.g. that it visits fewer than 100 colliders per frame. Today it visits all 2000.

## DC4: Static scenery costs per-frame Dart work
- Severity: slow. Size: S/M.
- Where:
  - world_objects.dart:209-210 (`_attach` puts every object in `_live` and `_starting`)
  - world_loop.dart:221-256 (`_callEach`)
  - world_loop.dart:304-323 (`_tidy`)
  - world_loop.dart:329-353 (`_reorderMoved`)
- How it shows:
  - A plain `GameObject` with no behaviours (a wall, a cube) is visited by `_callEach` for update and lateUpdate, and for fixedUpdate once per fixed step, every frame, for nothing.
  - Model parts are already kept out through `_enterLoop`, but code-built scenery is not.
  - Separately, any destroy in a frame costs `_live.removeWhere` over all live objects, and any reparent costs `_reorderMoved` over all live objects. So a game that spawns and kills bullets in a 5000-object scene pays O(5000) each frame.
- First fix:
  - Enter `_live` only when an object has at least one behaviour, or is a subclass that overrides an update hook. Treat it like the model parts' `_enterLoop`, and enter on the first `addComponent(Behaviour)`.
  - Remove destroyed objects by index or swap, or tombstone them and compact only above a threshold.
- Failing test (bench): 5000 static `GameObject` cubes plus 1 behaviour. Measure `world.update` Dart time, and assert it stays close to the 1-object scene (say under 2× it). Today it scales with 5000.

## DC5: Built-in feature components are wired by hand into the core
- Severity: rewrite. Size: M.
- Where:
  - game_object.dart:781 (`removeComponent`'s type switch)
  - game_object.dart:911-938 (`_unwrite`) and 940-948 (`_write`), both type switches
  - game_object.dart:1166-1367 (`_writeParticles`, ~200 lines of particle marshalling inside GameObject)
  - world_objects.dart:476-482 (feature cleanup in `_destroy`)
  - world.dart:349-359 (per-feature registries on World)
- How it shows:
  - There are two extension mechanisms. Packages (physics, audio) plug in through WorldSystem `componentAdded`/`componentRemoved`. Particles, trails, animators and splines are instead switch arms in GameObject and World.
  - Every new engine component touches at least five places in the core.
  - A missed arm fails silently. For example, a new component that isn't added to `_destroy`'s cleanup leaves a registry entry behind.
- First fix: give the sealed Component private `_write(GameObject)`, `_unwrite(GameObject)` and `_gone(World)` methods that each component overrides. Move the per-feature registries and the particle marshalling into the feature's own part file, or into an internal WorldSystem.
- Test: structural, so nothing fails today. Add a guard test that, for each built-in component type, adds it, removes it and destroys its owner, then expects the world's registries to be empty.

## DC6: A failed `world.add` leaves later siblings stuck and their model held
- Severity: bug (edge). Size: S.
- Where: world_objects.dart:181-191 (`_attach`'s catch) and 218-221 (the children loop, which sets `_queuedIn = null` per child).
- How it shows:
  1. In parent P's described children [A, B], A's attach throws (e.g. an invalid camera or a bad light). The catch unwinds A and rethrows.
  2. B was never reached, so it keeps `_queuedIn == P`. P is live by now with `_described == null`, so B is never attached.
  3. From then on, `world.add(B)` and `P.add(B)` fail `_checkAddable` with "already added".
  4. A `described.asset` that B holds (from `Assets.instantiate`) is never released, which leaks the model.
  5. Lights written before the failure stay counted in `_suns`/`_lamps` (`_countLight` at 521-551).
- First fix: in the catch, walk the remaining described children and clear their `_queuedIn`, release their held assets, and undo the light counts. Or attach in two passes: validate everything first, then write.
- Failing test: a parent with children [a camera with an invalid setting, a good child with a held model]. `world.add` throws. Then expect `world.add(good)` to succeed (today it throws "already added"), and expect the model's holders to be back to 0.

## DC7: `getInChildren` silently misses children before the object is added
- Severity: awkward. Size: S.
- Where:
  - game_object.dart:834-842 (`getInChildren`, which uses `_children`, null before add)
  - game_object.dart:825-826 (`getInParent`) and 567 (`ancestor`), which go through `parent` and throw StateError before add
  - game_object.dart:525 (`parent`)
- How it shows: in a constructor or builder, before `world.add`:
  - `getInChildren<T>()` returns null even when a constructor child has a T.
  - `getInParent`/`ancestor` throw instead.

  So the same family of lookups behaves differently, and the silent case is the worse one: it reads like "no such component".
- First fix: before add, walk `_described.children` for `getInChildren`, and `_queuedIn` for `getInParent`/`ancestor`. Or make all three throw the same clear "not added yet" error.
- Failing test: `final o = GameObject(children: [GameObject()..addComponent(Foo())]); expect(o.getInChildren<Foo>(), isNotNull);`. Today it returns null.

---

## Minor / lower confidence (not raised as findings)
- `world.all/only/first` count objects that are still loading. `_isActive` (world.dart:870-875) checks only `_activeSelf`, not `_hiddenForLoad`, so a query can return an object whose onLoad hasn't finished. That differs from `activeInHierarchy`. It may be intended.
- `Material.dispose` across two worlds (resources.dart:1035-1042) could throw partway, after the first world has released its id, and leave a stale entry in `_release._in`. I did not check whether engine material ids are reused, so this may be harmless.
- `WorldSystem.writeTransforms` (systems.dart:265-276) allocates a Transform per object, but no package calls it. It is dead or rarely used API; if kept, write from a reused buffer.

## Looked at and fine
- `_destroy` during iteration; `_runDue` swap-remove; `_sendTransforms` skipping gone objects.
- The moved log's at-least-once delivery and `_trimMoved`.
- Pool get/release/`_lost`; ObjectHandle generations.
- Signals: emit while a listener cancels, tidy, `_Listening.cancelShown`.
- Routines: zones and their own microtask queue, stop, race/waitAll, fixed-step waits.
- Time: clamping, maxFixedSteps, fixed-step edges.
- Input: queue timing, actions, edges within fixed steps, pinch, the velocity ring buffer.
- Pointing: `_bubble`/`_one`/`_overNow` guards against objects destroyed mid-dispatch.
- Assets: holding and release across async loads; freeing after `dispose`.
- `_Release`: mesh/material update in place; `_removeCollected` order (materials first, IN_USE kept).
- `world_resources.dart`: texture/shader release refusing while used; `_unloadMaterialsUsing`.
- Scratch growth; `check` mapping statuses to errors; FrameList; LayerMask; keys.
- Quality validation (the engine checks the rest); WorldView camera caching; HeadlessView.
- testing.dart run/tap; the build hook's target triple and NDK selection.
- `_stepParticles`/`_linkWaitingParticles` handling removed or waiting systems.
- The animator nulling cached part transforms: bones never go stale.
