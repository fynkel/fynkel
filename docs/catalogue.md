# The native component catalogue

The catalogue contract: blueprint §3.5 (native systems next to Dart
behaviours), ADR-0011 (thousands of moving objects without a Dart tick
each), §5.1 (where in the tick they run), §19.2d item 10 (M3-10). Six
components the runtime ticks itself, in Rust, with no `Behaviour`:

| Component | id | What it does | Slot |
| --- | --- | --- | --- |
| `fynkel.Rotator` | 14 | turns the local rotation `speed` rad/s about `axis`, in the entity's frame (`space: local`) or the world's (`world`) | pre-tick |
| `fynkel.Oscillate` | 15 | moves along `axis` (local or world) by `amplitude · sin(2π · frequency · t + phase)` | pre-tick |
| `fynkel.LifetimeDespawn` | 18 | destroys the entity and its subtree `seconds` after the component appeared | pre-tick |
| `fynkel.Emitter` | 19 | instantiates `prefab` `rate` times a second at the emitter's world pose, each instance living `lifetime` seconds, at most `max_alive` alive, dynamic bodies launched at `velocity`; `enabled` pauses it | pre-tick |
| `fynkel.Follow` | 16 | places the entity at the world position of the `target` object plus `offset`, snapped (`smoothing` 0) or eased at `smoothing`/s; `look_at` turns its −Z at the target | post-physics |
| `fynkel.LookAtCamera` | 17 | turns the entity's +Z at the active camera; `lock_y` keeps it upright (a billboard) | post-physics |
| `fynkel.TriggerZone` | 20 | (M3-12, [scenes.md](scenes.md)) names the general-lane event (`ZONE_ENTERED`/`ZONE_EXITED` with `event`) the entity's sensor `Collider` raises; `once` fires one enter and removes the zone | physics events |

They are ordinary native components: authored in a scene or prefab
(`fynkel validate` checks the values, `FYN-H0008`), generated into every
codec by `ALL` (`docs/codegen.md`), reachable from Dart as
`get<Rotator>()`/`add(const Oscillate(…))`/`set`/`remove<Follow>()` on
the M3-07a path (`packages/fynkel/test/catalogue_test.dart`), listed in
`fynkel.project.json` → `components` like any other. `MotionSpace` is the
`local | world` enum `Rotator` and `Oscillate` share. The `fynkel_core`
implementation is `crates/fynkel_core/src/world/catalogue.rs`; the
reference test `crates/fynkel_core/tests/catalogue.rs`.

## Where they run (§5.1)

The order is written once, in the `TICK` table of
`crates/fynkel_core/src/world/systems.rs`: each system there says when it
runs (most are off in edit mode) and `step` runs the table in order.

```text
World::step
  drive_characters              the characters' action driver (M3-09)
  apply C[n]                    the tick's commands
  run_animation                 animators pose their subtrees, skins resolve joints (M3-19)
  run_rotators, run_oscillators pre-tick movers: local transforms change
  run_lifetimes                 expired subtrees destroyed (Destroyed events)
  run_emitters                  prefabs instantiated, LifetimeDespawn added
  physics                       propagate, sync, Rapier step, write-back
  propagate; place_camera_rigs  the tick's world poses; the camera rig
  run_followers, run_billboards post slot on the propagated poses
  propagate (if any moved)
  record, seed_prev_world, apply_teleports, publish
```

The movers run *before* physics so a kinematic body on an oscillating
platform, or a collider on a turning cube, reaches Rapier this tick (an
external move — the sync treats it as a teleport, `docs/physics.md`).
`Follow` and `LookAtCamera` run *after* physics and after the
`ThirdPersonCamera` rig, on the poses the tick ends with: a light
following a falling crate sits where the crate landed, a billboard faces
where the camera rig put the camera. Both prepare parent-first world-pose batches through the shared scene
conversion and publish them through the native data journal, then
trigger one more propagate only when something changed.

Native passes collect output before publication. The shared journal and pose
preparation allocate transient staging data; large-population allocation and
latency qualification remains open. The runtime state a pass needs lives in
companion components that are never authored or on the wire: `Oscillation { elapsed, offset }` on every
`Oscillate`, `EmitterRun { accumulator, alive, reported }` on every
`Emitter` (attached on the first tick the component is seen, detached
with it).

## Semantics worth knowing

- **Rotator, `space: world` under a rotated parent** turns about the
  world axis: the axis is expressed in the parent's frame (the parent's
  matrix as the last propagate left it) and pre-multiplied. `local`
  post-multiplies, so the axis is the entity's own. A zero axis is
  rejected (`FYN-H0008` / `INVALID_PAYLOAD`).
- **Oscillate moves by the change in offset**, not to an absolute
  position: the rest position is wherever the entity is minus the
  current offset, so a gameplay write (`setLocalPosition`, a teleport)
  shifts the rest position instead of fighting the motion, and two
  oscillators on a parent and its child compose.
- **Follow's `target` is an object reference** (ADR-0035), like the
  `ThirdPersonCamera` rig's: `{ "entity": id }` in a document, resolved
  once when the scene loads or the prefab is instantiated, so each copy
  of a prefab follows its own object. None, or a destroyed target,
  leaves the follower where it is; nothing searches for another object
  with the same name. A reference to an object that is not in the load
  is None, with the warning `FYN-B0003`. `look_at`
  keeps the last heading while the follower sits closer than a
  millimetre to its target (float noise would set it otherwise) or
  straight above/below it.
- **LookAtCamera** faces the `Camera` with the highest `priority` (the
  lowest entity on a tie), the one the renderer draws from. With
  `lock_y` only the yaw changes; without it the entity tilts too. No
  camera: nothing happens, the pass reports 0 entities.
- **LifetimeDespawn** counts down from the tick the component appears
  (an `Emitter` sets it on each instance); at zero the subtree goes the
  way `Command::Destroy` sends it — subscriptions dropped, overrides
  cleared, one `Destroyed` event per entity, so Dart's `onDestroy` runs
  and `isAlive` flips. Setting it again from Dart restarts the clock.
  The native pass commits countdowns and removals through one gameplay journal;
  invalid data rejects the whole pass without partial countdown or deletion.
- **Emitter** carries the fractional instance across ticks (`rate`
  10/s at 60 Hz emits every sixth tick, never two at once until the
  rate exceeds the tick rate). An instance is what `World::instantiate`
  makes — the prefab's root, unparented, at the emitter's world pose
  (ADR-0022: no extra entity) — plus a `LifetimeDespawn` on the root
  when `lifetime > 0`, and a `SetLinearVelocity(velocity)` queued on
  every dynamic body in it (applied by this tick's physics step, before
  gravity). `max_alive` counts instances (roots) still alive; 0 is unlimited. The
  prefab must be registered with the world: the hosts register every
  prefab an emitter in the loaded scene or an instantiated prefab names
  (`Assets::register_prefabs`, recursive, with the prefab's own mesh
  colliders), which is why a prefab an emitter references is in the
  scene's asset closure. A prefab that is not registered emits nothing
  and reports `PREFAB_UNAVAILABLE` (14, `prefab_unavailable`) once per
  emitter — a diagnostic on the step, not a rejection; registering it
  later lets the emitter run. Each emitter's burst uses the shared prefab
  transaction: invalid component/reference/launch data rejects all instances in
  that burst, with a diagnostic. Other emitters still run. Only committed roots
  enter `max_alive`; failed attempts consume their scheduled emission slots,
  preventing a catch-up backlog. Launch effects publish only after successful
  entity commit and reach this tick's physics exactly once. With physics disabled,
  body configuration is retained and runtime launches are skipped.

## Per-system time

`World::system_times()` measures every phase of the step with the wall
clock: one `fynkel.rec.SystemTime { system: SystemKind, entities, micros }`
per `SystemKind`, in tick order — `step` (the whole thing; `entities` is
the live count), `commands` (applied), `characters` (driven),
`animation` (animators + skins, M3-19, [animation.md](animation.md)),
`rotator`/`oscillate`/`lifetimeDespawn`/`emitter` (components ticked;
the emitter's instances count in their own components), `physics`
(bodies + colliders + characters), `cameraRigs`, `follow`,
`lookAtCamera`, `transforms` (world matrices recomputed by every
propagate of the tick), `publish` (snapshot records). The hosts read it
as `FYNKEL_QUERY_SYSTEMS` (10, `World.systemTimes()` in Dart, the
starter's HUD `systems` line), the headless host accumulates it over a
run (`HeadlessReport::systems`: mean and worst tick per phase, the last
tick's counts; `fynkel test --headless` prints the `systems:` line;
`benchmarks/m3/capture.py` tabulates it), and `fynkel test --headless
--json` is where a regression shows first.

## Cost

The stress scene (`benchmarks/stress/generate.py`) makes its 5 000
statics native movers — 1 668 `Rotator`s on the cubes, 1 668
`Oscillate`s on the pyramids, 1 664 `LookAtCamera`s on the quads — and
adds a `Follow` light on the first body and an `Emitter` keeping 40
dynamic sparks alive. Per tick on the M3 profile (release,
`fynkel test --headless --steps 1800 --json`): rotator ≈ 15 µs,
oscillate ≈ 22 µs, lookAtCamera ≈ 110 µs (a world-pose write per
entity), transforms ≈ 0.44 ms for the ≈ 3 700 world matrices the movers
dirty, physics ≈ 2.0 ms — up from 0.6 ms with the statics still, because
every mover carries a box collider and Rapier moves 5 000 fixed bodies
through its broad phase each tick. A purely visual mover should carry
no collider; one that must be walked on pays the sync. The whole step is
≈ 2.7 ms at p50, well inside 120 Hz; `benchmarks/m3/results/` has the
numbers per capture.

The 300 ticking Dart behaviours §15.3 names are the `--app` project's
`game.Wobble` (`benchmarks/stress/wobble.dart`: a bob and a spin written
to the transform every tick — the same motion as `Oscillate` +
`Rotator`, from Dart) on the 300 Behaviour pyramids; the HUD's
`commands` count is their 300 writes and its `systems` line puts their
cost next to the native movers'.

## Not in V1

Splines/paths, tweens with easing, particle emitters with per-particle
simulation (an `Emitter` instance is a full entity), `Follow` with
rotation smoothing or dead zones, `LookAtCamera` axis choice. Each is a
component in this catalogue when a game needs it, not a `Behaviour`.


## Native system publication (F2)

Rotators, oscillators, followers, billboards and camera rigs now publish validated
batches instead of mutating authored state during iteration. A failed pass leaves
its authored output unchanged and reports a diagnostic. Oscillator elapsed time
and offset commit with its pose; removing the component retires that runtime state.
Camera yaw/pitch and pose share one commit. A one-shot trigger's named entry event
is published only when removing its TriggerZone succeeds; raw contact events still
report the physics step that occurred.

Animation and IK prepare authored output before publication. Physics uses the
shared scene pose validator and an atomic pose batch. Audio publishes its resulting
playing flags through the native data journal. Their clocks, solver state, voices,
contacts and playback events remain owned by their subsystems; rejecting an output
batch does not rewind completed simulation or playback. Transform propagation and
interpolation history retain their declared stages, including catch-up ticks.

Legacy command ingress, scene/resource operations and old external overlays are
separate remaining F2 work. The native system boundary does not imply that a whole
engine tick is one rollbackable transaction.
