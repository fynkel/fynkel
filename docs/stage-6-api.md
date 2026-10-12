# Stage 6 step 2: the API review

Every public name in `fynkel`, `fynkel_physics` and `fynkel_flutter`,
reviewed (2026-10-12) against each other, against Unity (and Flutter for
`fynkel_flutter`), Dart's style, defaults, docs and errors. Three
reviewers each took a part; their full tables, row by row with file and
line, are in [stage-6-api/](stage-6-api/). I checked the claims that
change behaviour in the code (`translate`, `isDown`, `parent =`,
`World.dispose`, the physics follow) before writing them here.

| Part | Public members | Kept as they are | Rows (rename, change, add, document…) |
|---|---:|---:|---:|
| core: objects, world, lifecycle, routines, signals | ~321 | ~240 | 52 |
| core: math, motion, input, cameras, assets | ~405 | ~330 | 58 |
| physics | ~235 | ~188 | 47 |
| Flutter | ~40 | ~26 | 14 |

**The verdict**: the API is sound. About four in five names stay as they
are; the Unity names work as people guess, the value types are immutable,
and the errors say what to do. What needs fixing is a short list of
**false friends**, names a Unity user guesses that compile and then do
something else, a few **behaviours that differ from Unity's** for no
reason, **plumbing that is public**, and **well over 100 public members
with no doc comment that should have one** (the lint finds about 400,
most of them key names, enum values and constructors).

The changes come in three groups: **A**, those that need your call;
**B**, behaviour fixes and additions with nothing to decide (they follow
Unity, or fix a finding); **C**, docs and internals.

## A. Your call

Each with a recommendation. All are breaking: the examples, the lab and
the guide change with them.

1. **One unit for angles.** The core uses radians (`Quat.euler`,
   `rotate`, `Orbit`); physics uses degrees (joint limits, `slopeLimit`,
   as Unity), and so does a camera's field of view. A Unity user writes
   `Quat.euler(0, 90, 0)` and gets 14 turns.
   - **(a) Radians everywhere, with `90.deg`** (an extension on `num`)
     for literals: `JointLimits(min: -90.deg)`, `Quat.euler(0, 90.deg,
     0)`. One rule; Dart's `math` is radians. **Recommended.**
   - (b) Degrees everywhere, as Unity: every Unity number copies over,
     but Dart's `math` and our own math types work in radians.
   - (c) As now, with the unit in each name's doc.
2. **`position` means what Unity's means.** Today `obj.position` is local
   (Unity's `localPosition`) and `worldPosition` is the world's;
   `Rigidbody.position` is the world's. For a root they are the same,
   which is why it rarely shows; for a child it is a false friend.
   - **(a) As Unity: `position`/`rotation` are the world's,
     `localPosition`/`localRotation` the parent's.** **Recommended**:
     one meaning across the core and physics, and every Unity snippet
     means what it says.
   - (b) Keep local, and say so in every doc and in §20.
3. **Input: `isDown` → `isPressed`.** Unity's `GetKeyDown` means "went
   down this frame", ours "is held": `if (input.isDown(space)) jump()`
   jumps every frame. `wasPressed` (this frame) stays. **Recommended.**
4. **Routines get Unity's words**: `start` → `startRoutine` (it frees
   `start`, the word games want most, and stops looking paired with
   `onStart`), `until` → `waitUntil`, `whileTrue` → `waitWhile`, the
   top-level `all` → `waitAll` (not the same idea as `world.all`), and a
   new `stopAllRoutines()`. **Recommended.**
5. **Moving and parenting as Unity**: `translate` and `rotate` take a
   `Space` and default to the object's own (`Space.self`), as Unity's
   (today `translate` moves in the parent's); `parent = p` keeps the world
   position, as Unity's `SetParent`, with `setParent(p,
   worldPositionStays: false)` for the other. **Recommended.**
6. **The camera's helpers live on `Camera`**: `screenToRay`,
   `screenToPlane` and `worldToScreen` are now on every object and throw
   unless it has a camera; they move to `Camera`, with Unity's names
   (`screenPointToRay`, `worldToScreenPoint`), and `world.camera` gives
   the `Camera`, as Unity's `Camera.main`. **Recommended.**
7. **`gameObject` for "the object" everywhere**: `RayHit.object` and
   `ObjectError.object` → `gameObject`; physics' `PhysicsHit` →
   `RaycastHit`, Unity's, with `.rigidbody`. **Recommended.**
8. **`world.all`, `first` and `only` leave hidden objects out** unless
   `includeInactive: true`, as Unity's `FindObjectsByType` (finding 8:
   pooling breaks the guide's wave pattern today). **Recommended.**

## B. Fixes and additions, nothing to decide

- **`World.dispose` destroys the roots first**, so `onDisable` and
  `onDestroy` run when a page is left or a game restarted (finding 7).
- **`FynkelApp` is built on `MaterialApp`**, so Material widgets work in
  the HUD (finding 1, the blocker), with `theme`, `darkTheme` and
  `navigatorKey`. Stage 7 builds the rest of the Flutter side on it.
- **Flutter controls feed input**: `InputAction.press()`/`release()` and
  `input.setAxis2(name, value)` (finding 3). The feeding calls the view
  and tests use (`keyDown`, `fingerDown`…) move behind one object,
  `input.simulate`, out of a game's way.
- **The view stops taking every key**: it gives each key to the game and
  claims only those the game uses, so Flutter's shortcuts work
  (finding 4).
- **Pause in the core**: `world.paused` stops time and the game's input
  together, and the frame time starts fresh on resume (finding 2). Stage
  7 ties it to dialogs and the app's lifecycle.
- **A per-object tint**: `MeshRenderer.color` multiplies its material's
  for that object alone, and `colorTo` tweens it (finding 12);
  `Material.copy()` too. The doc says a renderer's material is shared.
- **Meshes**: `Mesh.capsule()`, `cylinder()` and `quad()`, sized as
  Unity's and fitting the colliders' defaults (finding 13).
- **Physics, safe and clear**: `wakeUp`/`sleep` on a body not yet in the
  world do nothing, as Unity's; setting `gravity` wakes every body; errors
  name the real reason (destroyed, removed, disabled, hidden) (findings 14,
  15). A query looks only at the objects that moved, from the core's
  moved list (finding 9).
- **`Follow(space:)`**: the offset in the world, the target's space, or
  its yaw only, as Cinemachine's binding modes (finding 22).
- **`Signal.stream`**, for UI code (finding 5), and
  `WorldSystem.componentRemoved` to match `componentAdded`.
- **`FynkelGame.of`** next to `maybeOf`, typed; **`FynkelView`** split
  into `FynkelView(root:)` and `FynkelView.world(...)`; a plugin failure
  no longer stops the view's ticker for good (the likely cause of finding
  25).
- **Small renames that only fix inconsistency**: `vibrato` →
  `frequency` where it means "per second" (`shake`), `Pinch.centre` →
  `center`, `Camera.size` → `orthographicSize`, `assets.model` →
  `assets.instantiate` (it gives a copy, not the asset), `HookError` →
  `HookException`; `ignoreLayerCollision`'s flag becomes named.
- **Smaller behaviour fixes**: `scroll`'s sign as Unity's; `Plane.distance`
  as Unity's; `Transform.forward` (parent-relative) removed in favour of
  the object's.

## C. Docs and internals

- **Every public member gets a doc comment**, and the
  `public_member_api_docs` lint goes on in the three packages so CI keeps
  it so. Two wrong docs fixed (`JointBreaks`, `Rigidbody.position`), and
  the guide's claim that every tween helper takes `speed:`.
- **What a game shouldn't see goes**: `GameObjectSystems` and
  `World.pointer` (exported by mistake), `physics.dispose()` and the frame
  hooks (`@protected`), `PhysicsStatus` (to `raw.dart`), `rustTriple`.
- **Errors**: one type for one mistake (`ArgumentError` for a bad value,
  `StateError` for a wrong time; `FynkelException` only for what the
  engine reports), and the engine's words ("despawn") out of messages.
- **Joint anchors and scale**: say that an anchor is in the object's own
  space, scaled (finding 16).
- **§20 (From Unity)** gains the differences that stay.
- **`motion.dart`'s private half** (about 470 of its 1,126 lines) moves
  to its own file; the public API stays one family.

## Not taken

- `id` → `instanceId`: `id` is clear, and Unity's word adds nothing.
- Splitting `motion.dart` or `input.dart`'s public API: each is one
  coherent family.
- `Transform`, `Material` and `Color` clash with Flutter's names, but they
  are Unity's; `fynkel_flutter` gains `Color` conversions instead, and the
  guide's import advice stays.

## How it's done

In step 6 (the fixes), after steps 3 to 5 add their findings: group A as
you decide, then B, then C, a PR each part, with the examples, the lab
and the guide changed in the same PR, and a line in the guide's notes for
each breaking change.
