# Checked: Dart core (DC1–DC7) and Dart features (DF1–DF14)

Checker, stage 14 audit. Worktree at 2f72cbd5. Its lib/ is identical to stage-14 @ edbe57aa, which differs only in docs. Every claim's cited code was re-read. The throwaway tests are in /tmp/s14/tests/dart/; each file names its repo path on line 1:
- packages/fynkel/test/audit_core_test.dart (DC1, DC2, DC6, DC7)
- packages/fynkel/test/audit_features_test.dart (DF1, DF3, DC3, DF4–DF12, DF14)
- packages/fynkel/test/audit_bench_test.dart (DC4, DF2, DF7a timings)
- packages/fynkel/test/audit_particle_set_alloc_test.dart and audit_material_set_alloc_test.dart (DF2 and DF7a allocations, with the repo's alloc harness)
- packages/fynkel_physics/test/audit_follow_bench_test.dart (DC3/DF3 with physics)

All timings are medians from the Dart JIT under `dart test` on this Mac. A phone will be several times slower.

---

## Dart core

### DC1: Behaviour.onLoad can run twice. CONFIRMED (test)
The test is `audit_core_test.dart` "DC1". A root's constructor child is `b`, and the root's onLoad does `find('b')!.addComponent(Counter())`. After `world.add`, a pump and an update, the test expects `Counter.loads == 1`; it is **2**. The path is as described:
1. `_attach` attaches the whole described tree first.
2. `_runLoad(root)` runs root.onLoad. `_componentAdded` sees b live but not appeared, so it does `_pending++` and `_load(b, counter)`: the first load.
3. `_runLoad(b)` then collects every behaviour that isn't `_removed`, with no "already loading" guard: the second load.

Severity bug, size S. Both stand.

### DC2: an object reparented while loading never starts, but is drawn. CONFIRMED (test)
The test is "DC2". P's async onLoad does `await null; x = add(X with async Slow); x.parent = null`. After 5 pumped frames:
- `x.activeInHierarchy == true`: it appeared and the engine draws it.
- Slow's `starts == 0` and `updates == 0`.

`_appearIfDone(X)` hands off to its stale `_waiter` P and skips `_refresh(X)`. P's refresh walks P's subtree, which no longer holds X. Severity bug, size S/M. Both stand.

### DC3: animators make every frame look like "everything moved". PARTLY (test + bench)
- **The `transformVersion` bump is confirmed.** The test is `audit_features_test.dart` "DC3": one idle truck animator gives +1 a frame with nothing playing. Precision: animators are made lazily, so the world only pays once the game reads `GameObject.animator`. A copy whose animator was never read costs nothing.
- **Logging every part of a playing animator is correct, not a bug.** Those bones did move.
- **The physics full-scan branch is real, but cheap.** `count >= colliders + bodies`: 50 Knights × 41 parts = 2050 entries against 2001 colliders. Measured with `audit_follow_bench_test.dart`, median µs per frame:

| Scene | µs/frame |
|---|---|
| 50 knights playing, 1 collider | 221 |
| 0 knights, 2000 colliders | 61 |
| 50 knights playing, 2000 colliders | 299 |
| 50 knights playing, 20000 colliders | 1371 |
| 0 knights, 20000 colliders | 1505 |

  With 2000 colliders the full scan adds only ~17 µs above the sum of its parts. At 20000 colliders (the small branch) the difference is lost in noise.
- **The pointer `scene` invalidation is true.** It only costs a hover re-check per resting finger.

Verdict: the mechanisms are right, but the size claimed for animators that really play is overstated. The waste that matters is DF3's, and it has the same cause: `_animate` invalidates parts without knowing what the engine posed. Downgrade to slow-minor, size S, and merge it with DF3. The fix is DF3's fix plus "bump the version only if something was invalidated".

### DC4: static scenery costs per-frame Dart work. CONFIRMED (bench), size modest
`audit_bench_test.dart` "DC4": one Ticker plus N plain GameObjects, median µs per frame.

| N plain objects | Idle | One destroy + add a frame | One reparent a frame |
|---|---|---|---|
| 1 | 2 | 10 | 8 |
| 5000 | 35 | 98 | 119 |
| 20000 | 140 | 480 | 581 |

So:
- Idle costs about 7 ns per plain object a frame (`_callEach` ×2, plus fixed steps).
- Any destroy costs `_live.removeWhere` over everything.
- Any reparent costs `_reorderMoved` over everything: a map of the moved subtree plus a removeWhere over all of `_live`.

All three scale linearly as claimed. Severity slow, size S/M, both stand. It matters at 5k–20k objects on phones, where it is around 0.3–2 ms.

### DC5: built-in feature components are wired by hand into the core. Description ACCURATE, with corrections
Confirmed:
- `removeComponent`'s type switch (`case Camera() || Light() || MeshRenderer() || ParticleSystem() || TrailRenderer()`).
- `_write`/`_unwrite` switches.
- `_writeParticles` (~200 lines) inside GameObject.
- `_destroy`'s per-type cleanup (`_countLight`, `_particleSystems`, `_trails`, `_animators`/`_animatorList`).
- World-level registries.
- Packages (physics, audio) using `WorldSystem.componentAdded/Removed` instead.

Two corrections:
1. Splines are not switch arms. `SplineContainer` registers itself in `world._splineContainers` from its own `onEnable`/`onDisable`, as any Behaviour can.
2. Animators aren't components at all. They are made lazily by the `GameObject.animator` getter, and only their cleanup is hand-wired, in `_destroy`.

The switch also covers the core engine components (Camera, Light, MeshRenderer), not only features. "At least five places per new engine component" holds: the class, `_write`, `_unwrite`, `removeComponent`'s case, `_destroy`'s cleanup, and the registry. Severity rewrite, size M: a fair description.

### DC6: a failed `world.add` leaves later siblings stuck. CONFIRMED (test), wider than claimed
The test is "DC6": `P(children: [bad camera fovY 500°, good])`.
- `world.add(P)` throws.
- `world.add(good)` then throws "already added", because `_queuedIn` is still P.

Also confirmed with "DC6b": light counts leak from the failing object itself. Five failed `[Light.sun(), bad Camera]` adds print "the world has 5 suns". The catch in `_attach` doesn't undo `_countLight` for components written before the throw.

**New (wider than the claim):** P itself stays live in the world after `world.add` threw. `p.isAlive == true`, with no children, and it runs its loads. The caller sees an exception for an object that is now half in the world. The root-level catch only unwinds when the root's own component fails.

The model-hold part is PARTLY true. The held asset is released if the game calls `good.destroy()` (the unadded path, `_destroyUnadded`), and `World.dispose` frees everything. So it leaks only if the game drops `good`; I did not test this, as it needs an asset source.

Severity: bug (edge), size S. With the half-added root, I'd keep it as a bug and fix it with "validate all, then write" or a full unwind.

### DC7: `getInChildren` silently misses children before add. CONFIRMED (test)
- "DC7": `GameObject(children: [GameObject(components: [Foo()])]).getInChildren<Foo>()` returns null.
- "DC7b": `getInParent` before add throws "has no world yet".

Severity awkward, size S. Both stand.

---

## Dart features

### DF1: a destroyed live VirtualCamera makes the next World.update throw. CONFIRMED (test)
The test is `audit_features_test.dart` "DF1". Vcam A has priority 10, vcam B priority 0, and there is a Camera. After `update`, `a.destroy()`, the next `update` throws `StateError: GameObject#2 was destroyed, so its parent can't be read`, from `_choose` → `last!._pose`. On the following frame:
- `live == B`
- `onLiveChanged` was never emitted (changes = [])
- `isBlending == false`, so it was a silent cut.

This is exactly as described, including that the throw skips `Mesh._publishChanges`, the debug flush and `afterFrame`. Severity bug, size S. Both stand, and it is high priority.

### DF2: particle and trail setters rebuild, resend and allocate on every set. CONFIRMED (test + bench)
**Allocations** (`audit_particle_set_alloc_test.dart`, with the repo harness): `ps.emission = prebuiltEmission` once a frame makes, per 2000 frames, `Context 2000, _Closure 4000, _List 4000, _GrowableList 2000, _ArrayIterator 4000, FyParticleSettings 2000`. That is about 9 objects per set. The traces point at:
- `_set`'s two closures
- `_refused`'s `[...]` list literals and for-in iterators
- `_writeParticles`'s iterator and its `FyParticleSettings` struct view

**Time** (`audit_bench_test.dart` "DF2"): 1.5 µs per set with 1 system, and 4.3 µs with 1000 systems in the world. The engine's `relink_particles` (particles.rs:1714) walks every system on every set; I read it and confirmed this. The cost grows with N as claimed, though the slope is small (~3 ns per system).

Severity slow, size M. Both stand. Each set writes at once, so three sets are three full writes.

### DF3: animated parts are dirtied every frame for hidden, paused and finished animators. CONFIRMED (test + bench)
The test is "DF3", on Knight.glb with a moved reader:

| Knight state | Moved-log entries a frame |
|---|---|
| Playing and shown | 41 |
| Hidden (`active = false`) | 41 |
| `speed = 0` | 41 |

`transformVersion` is +1 a frame while hidden. The physics bench gives the cost for 50 knights with 2000 colliders:

| 50 knights | µs/frame |
|---|---|
| Hidden after play | 109 |
| Never played | 67 |

That is ~42 µs a frame of pure waste, in Dart plus the physics full scan, for 50 hidden copies. A pooled crowd hits exactly this.

It has the same root cause as DC3: `_moving` is cleared only by `stop()` with no fade, and `_animate` doesn't know what the engine posed. Severity slow, size S. Both stand. This is the half of DC3/DF3 worth fixing first.

### DF4: a destroyed collision plane makes every setter throw. CONFIRMED (test)
The test is "DF4": a system with `ParticleCollision(planes: [floor])`, then `floor.destroy()`, then `ps.gravityModifier = 2` throws `ArgumentError: collision: a plane's object is destroyed`.

The second half is confirmed by reading. `_writeParticles` sets `waiting = true` for a destroyed plane (game_object.dart:1326). `_linkWaitingParticles` then rewrites the system, which re-adds itself, on every `_shape` change for good.

Severity bug, size S. Both stand.

### DF5: Motion.complete() on a finite loop ends at the wrong place. CONFIRMED (test)
Tests in the "DF5" group:
- `tween(0→1, loops: 2, yoyo: true)` after `complete()` ends at **1.0**. Run out (the control test), it ends at 0.0.
- `sequence([moveBy(+1 x, 1 s)], loops: 3).complete()` moves x to **1.0**. Run out, it reaches 3.0.

Severity bug, size S. Both stand.

### DF6: splines on a scaled container aren't in world metres. CONFIRMED (test)
- "DF6": a container scaled 2 with `SplineAnimate(speed: 1)` travels **2.0 m** in 1 s.
- "DF6b": on a container scaled (2,1,1), the diagonal spline's `rotationAt` forward is (0.707, 0, −0.707) while `directionAt` is (0.894, 0, −0.447). They disagree.

Severity awkward (bug against Unity), size S–M. Both stand.

### DF7: shader values repack everything on each set; a value→texture reload throws. CONFIRMED (test), both halves
**(a)** `audit_material_set_alloc_test.dart`: `material.set('v0', x)` once a frame, on a 3-value shader, makes about **70 objects per set**. Per 2000 frames: `_List 16000, _GrowableList 16000, _Record 16000, ListIterator 18000, _Double 12032, IndexedIterator 12000, Uint8List/ByteData/ByteBuffer 2000 each, Closure/Context 8000 each`. Every value is re-parsed and repacked, and then `_updateMaterial` rewrites the whole material plus all its values. Time: 1.2–1.7 µs per set at 1–8 values, 4.6 µs at 32 values. Severity slow, size S.

**(b)** Test "DF7b": a material sets `grain = 1.0`, then `shader.reload` with `var grain: color_texture` throws `TypeError: type 'double' is not a subtype of type 'Texture2D?' in type cast`. By reading, `reload` has already swapped `_pointer`/`_layout`, so the remaining worlds, the effects and `fy_shader_free(old)` are skipped. Severity bug, size S.

Small correction: the reader's own example name `noise` can't happen. The shader prelude already defines `noise`, so that source fails to compile with "redefinition of `noise`". Any other name reproduces the bug.

### DF8: Follow's lookAhead snaps on pause frames. CONFIRMED (test)
The test is "DF8": the target moves at 2 m/s, with `lookAhead: 0.5` and no damping. The rig is at x 3.5; after `world.paused = true; update` it is at **2.5**, which is lookAhead × velocity back.

By reading, the first frame after unpausing also has dt 0 (`time._skip`), so it holds back for one more frame. Severity bug (small), size S. Both stand.

### DF9: Animator.at marks can't be removed and pile up. CONFIRMED (test)
The test is "DF9": `at('Wheels', 0.3, f)` twice fires f **2** times in one pass. `_marks` only ever has `.add` (animator.dart:378), and there is no removal API. Severity awkward, size S. Both stand.

### DF10: MeshBuilder discards given normals once any vertex lacks one. CONFIRMED (test)
The test is "DF10": a quad has given normal (0, 0.6, 0.8) and there is one extra vertex without a normal. The built mesh's first normal is (0, 0, 1), worked out. Severity bug (small), size S. Both stand.

### DF11: SplineInstantiate with a count throws every frame on an empty spline. CONFIRMED (test)
The test is "DF11": `SplineContainer([Spline([])])` with `SplineInstantiate(count: 5)` gives 4 errors in 3 updates (onStart + 3 onUpdates): "a spline with no knots has no points". Severity bug (small), size S. Both stand.

### DF12: Mesh.inBackground loses UV and dynamic flags. CONFIRMED (test + reading)
The test is "DF12": `inBackground(() => Mesh()..markDynamic())` gives `isDynamic == false`. The native mesh is still dynamic; only Dart's flag is lost.

`_hasUvs` is also lost: `Mesh._` defaults it to true, which makes `_settle` ask for tangents from zero UVs. I confirmed this by reading.

**New, same family:** `Mesh.custom(positions, uvs: null, ...)` also leaves `_hasUvs = true` (resources.dart:466).

Severity bug (small), size S.

### DF13: ScreenEffect WeakReferences are only trimmed on reload. CONFIRMED (reading)
`shader._effects.add(WeakReference(this))` runs in the constructor (shader.dart:179), and the only `removeWhere` is in `reload` (shader.dart:130). There is no test because `_effects` is private and GC timing is unreliable. The growth is one WeakReference (~16–32 B) per effect ever made. Downgrade to cosmetic/minor: 10k hit flashes is about 300 KB.

### DF14: small inconsistencies. CONFIRMED (test + reading)
- "DF14": `Quat(0,0,0,0).normalized` gives `Quat(NaN, NaN, NaN, NaN)`, while `Vec3.zero.normalized` throws "has no direction".
- By reading, `SmoothDamp3._maxSpeed` is `final` with no setter (math.dart:572), while `smoothTime` has one.

Severity awkward, size S.

---

## New findings
1. **DC6 is wider than claimed.** When a *child's* component is refused, `world.add(root)` throws, but the root stays live (half-built, loading) in the world. Only a failure on the root's own components is unwound.
2. **DC6 also leaks light counts from the failing object itself**, not just from earlier siblings. Each failed `[Light.sun(), bad camera]` add leaves a sun counted, which triggers the "5 suns" warning.
3. **`Mesh.custom` without UVs leaves `_hasUvs = true`**: tangents are then worked out from zero UVs. This is the same family as DF12.
4. **Animators are lazy.** DC3's version bump only starts once `GameObject.animator` is read. It is worth knowing for any fix: the cost belongs to "animators the game touched".
