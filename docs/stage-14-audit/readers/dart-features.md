# Reader: the Dart framework's features

Read whole: motion.dart, motion_kinds.dart, animator.dart, model.dart,
particles.dart, trails.dart, shader.dart, sky.dart, cameras.dart, debug.dart,
splines.dart, spline_objects.dart, mesh_building.dart, math.dart, color.dart,
png.dart, resources.dart (Mesh, Material, _Release), plus the writers they
call in game_object.dart (`_writeParticles`, `_writeTrail`, `_unwrite`),
world_resources.dart (`_updateMaterial`, `_writeValues`, `_reloadShader`),
world.dart's frame order, and the engine's `animate` / `set_particles` /
`submesh_ranges`. Paths below are under `packages/fynkel/lib/src/` unless
given in full. Nothing was built or run.

---

## DF1. A destroyed live VirtualCamera makes the next World.update throw

- **Severity**: bug. **Size**: S.
- **Where**: cameras.dart:536-544 (`_choose`), cameras.dart:80-84
  (`VirtualCamera._pose`), world.dart:764 (`cameras._run()` called with no
  guard), game_object.dart:522-525 (`parent` → `_engine` throws once `_gone`).
- **How it shows**: vcam A (priority 10, live) and vcam B (priority 0).
  Destroy A's object. Its `onDisable` takes it out of `_virtual`, but `_live`
  still points at A. Next frame `_choose` picks B, and since `defaultBlend`
  is 1 s (not a cut) and no blend is running, it does
  `_fromPose = last!._pose`. `_pose` reads `gameObject.parent` /
  `.transform` of a destroyed object, and `_engine` throws
  `StateError('… was destroyed …')`. `_run` isn't guarded, so the error
  escapes `World.update`: the frame's mesh publish, debug flush and
  `afterFrame` are skipped. `_live` was already set to B, so on the next
  frame nothing blends and `onLiveChanged` is never emitted for B (a cut
  with no signal). Destroying a cutscene's camera object while a gameplay
  camera waits is the ordinary way to end a cutscene.
- **Fix**: in `onDisable`, if it's the live one, capture its pose into
  `_current`/`_fromPose` (or clear `_live` and treat the next choice as a
  blend from `_current`); in `_choose`, use `_current` when `last` isn't
  enabled. Guard `cameras._run()` like the other phases.
- **Test that fails today**: two vcams as above, `update`, destroy A's
  object, `update` → expect no throw, `cameras.live == B`, one
  `onLiveChanged(B)`, `isBlending` true.

## DF2. Particle and trail setters rebuild and resend everything, with garbage, on every set

- **Severity**: slow. **Size**: M (a write-at-end-of-frame change).
- **Where**: particles.dart:994-1002 (`_set` takes two new closures per
  call), particles.dart:1020-1078 (`_refused` builds two lists and runs every
  check on every set), game_object.dart:1166-1366 (`_writeParticles` fills
  the whole `FyParticleSettings`, up to ~3,000 table floats with struct
  views per element), crates/fynkel_world/src/particles.rs:1680-1720
  (`set_particles` revalidates and `relink_particles` walks *every* system in
  the world). Same pattern: trails.dart:109-145 / game_object.dart:1136-1164.
- **How it shows**: the Unity habit `ps.emission = Emission(rateOverTime:
  speed * 10)` (or `trail.widthMultiplier = …`, `ps.startColor = …`) each
  frame. Each set allocates two closures plus two lists in Dart, writes the
  full settings block (all enabled modules' 64-entry tables), crosses the
  FFI, and the engine re-checks the settings and relinks sub emitters
  across all N systems. Three sets in one frame cost three full writes. The
  hardening pass removed exactly this rollback-closure pattern from the
  components (hardening D14); particles and trails (stage 12) brought it
  back. Cost grows with the number of systems in the world, not with what
  changed.
- **Fix**: validate the single field in Dart, mark the system dirty and
  write once at the end of the frame (as `Mesh._changedMeshes` does), and
  only relink in the engine when `sub_emitters` actually changed. No
  closures in setters.
- **Test that fails today**: the alloc harness with `ps.emission = e`
  (a prebuilt `Emission`) every frame → expect 0 bytes a frame; and a
  count of `fy_particles_set` calls for three setters in one frame → 1.

## DF3. Animated parts are dirtied every frame for hidden, paused and finished animators

- **Severity**: slow. **Size**: S.
- **Where**: animator.dart:423-446 (`_animate`), animator.dart:278-279
  (`_moving` cleared only by a stop with no fade), engine
  crates/fynkel_world/src/animation.rs:819-832 (skips posing animators that
  aren't playing or whose root is inactive).
- **How it shows**: `_animate` nulls `_transform` and logs a move for every
  part of every animator with `_moving != 0`. That bit is set by play/
  crossFade/blend and cleared only by `stop()` with duration 0, so: a pooled
  (released, hidden) crowd member; an animator at `speed = 0`; a
  non-looping clip that ended and holds its pose; a `stop(duration: 0.3)`
  that faded out: all keep invalidating their parts each frame though the
  engine didn't move them. Every later read of a part's transform crosses
  the FFI again, and each part goes into the moved log, so system readers
  (physics follow, a package's `movedSince`) see 30-odd bones per hidden
  model "moved" every frame. With a pool of 50 knights hidden, that's
  ~1,500 invalidations and log entries a frame for nothing.
- **Fix**: have `fy_world_animate` report which roots it actually posed (it
  already loops them) and invalidate only those; or at least skip
  animators whose object isn't `activeInHierarchy`, whose speed is 0, or
  whose engine state isn't playing; clear `_moving` on a faded stop's end.
- **Test that fails today**: an animated copy, `play('walk')`, release it
  (or `active = false`); after `update`, a system reader's moved list
  contains its parts; expect none.

## DF4. Particle collision with a destroyed plane makes every setter on the system throw

- **Severity**: bug. **Size**: S.
- **Where**: particles.dart:1065-1069 (`_refused`: any destroyed plane is an
  error), particles.dart:994-1002 (every setter runs `_refused` over the
  whole system), game_object.dart:1325-1333 (the writer itself skips a
  destroyed plane and marks the system waiting, forever).
- **How it shows**: `ParticleCollision(planes: [floor])`, later `floor`
  is destroyed (a level piece goes). From then on
  `ps.startColor = …`, `ps.emission = …`, `ps.material = …`, any setter
  throws `ArgumentError('collision: a plane's object is destroyed')` and
  rolls back, though the change had nothing to do with collision; the game
  must notice and reassign `collision` first. Meanwhile the system sits in
  `_waitingParticles` (the writer sets `waiting = true` for a destroyed
  plane), so it is rewritten in full on every later spawn or destroy in the
  world (`_shape` changes), for good.
- **Fix**: treat a destroyed plane as gone (drop it when writing, as the
  writer already does) rather than an error in `_refused`; reject it only
  when it's destroyed at the moment `collision` itself is set. Don't mark
  the system waiting for a destroyed plane, only for one not added yet.
- **Test that fails today**: system with a plane; destroy the plane;
  `ps.startColor = Color.white` → expect no throw.

## DF5. Motion.complete() on a finite loop ends at the wrong place

- **Severity**: bug. **Size**: S.
- **Where**: motion.dart:250-260 (`complete` sets `_lastPlay = true` for
  every motion), motion.dart:329-342 (with `_lastPlay`, the play it's in is
  the last).
- **How it shows**: `complete()` is documented as "as if its time had run
  out"; only a loop-for-ever motion is meant to end at the end of the play
  it's in. But `_lastPlay` is set for finite loops too:
  `tween(0.0, 1.0, seconds: 1, loops: 2, yoyo: true, onUpdate: …)
  ..complete()` ends at 1.0 (end of play 0), while running it out ends at
  0.0 (play 1 comes back). Likewise `sequence([moveBy(Vec3(1,0,0),
  seconds: 1)], loops: 3).complete()` moves 1 m, not the 3 m it would
  (a group's parts begin again from where they are each loop). DOTween's
  `Complete` goes to the end of all loops.
- **Fix**: for `loops >= 0`, `_seek(_total)` (stepping through the plays)
  without `_lastPlay`; keep `_lastPlay` for `loops < 0`.
- **Test that fails today**: the yoyo tween above → value 0.0 after
  `complete()`; the looping `moveBy` sequence → x == 3.

## DF6. Splines on a scaled container: distances, speeds and spacing aren't in world metres, and rotation ignores the scale

- **Severity**: awkward (bug against Unity). **Size**: S–M.
- **Where**: spline_objects.dart:37-62 (positions go through
  `transformPoint`, directions through `_turned` with scale, but
  `rotationAt` is `worldRotation * splineRotation`, no scale),
  spline_objects.dart:371-424 (`SplineAnimate` steps `speed * dt` in spline
  units), spline_objects.dart:176-218 (`SplineInstantiate` spaces by
  spline units).
- **How it shows**: a container with `localScale = Vec3.all(2)`:
  `SplineAnimate(speed: 4)` moves 8 world m/s; `SplineInstantiate(spacing:
  2)` puts posts 4 m apart; `positionAt(10)` is 20 m along. Unity's
  SplineAnimate and SplineInstantiate use the container's world-space
  length (`SplineContainer.CalculateLength` applies the transform). With a
  non-uniform scale (a container stretched along X), `directionAt` and
  `rotationAt` disagree, so an aligned cart doesn't face the way it moves;
  with a negative scale (a mirrored track) it faces the mirrored way. The
  docs say "metres" throughout.
- **Fix**: either say plainly that spline distances are in the container's
  own space and refuse/warn on scaled containers, or (Unity's way) make
  `SplineAnimate`/`SplineInstantiate` work in world length (scale the
  step and spacing by the container's scale for a uniform one); build
  `rotationAt` from the world `directionAt` and `upAt` so they agree.
- **Test that fails today**: container scaled 2, `SplineAnimate(speed: 1)`,
  one second of updates → expect 1 m of world travel (gets 2); container
  scaled (2,1,1) on a diagonal spline → `rotationAt(d).rotate(forward)`
  closeTo `directionAt(d)`.

## DF7. Shader values: every set packs all values anew, and a reload that turns a value into a texture of the same name throws

- **Severity**: slow (first half), bug (second half). **Size**: S.
- **Where**: resources.dart:1112-1144 (`Material.set` → `_pack`),
  shader.dart:367-395 (`pack`: a new `Uint8List`, `ByteData`, and a
  `List<List<num>>` per value, for every value in the map),
  shader.dart:197-234 (`ScreenEffect.set` the same), then
  world_resources.dart:202-221 (the whole material written again);
  resources.dart:1147-1151 and shader.dart:221-224
  (`_values[name] as Texture2D?`), shader.dart:120-135 (`reload` swaps the
  pointer before telling the worlds; frees the old one only at the end).
- **How it shows**: (a) the guide's own advice, `tween(… onUpdate: (v) =>
  material.set('dissolve', v))`, allocates a fresh buffer plus lists for
  every value of the material each frame, then rewrites the whole material
  and all its values to every world; cost grows with how many values the
  shader has, not with the one that changed. (b) hot reload of a shader
  whose `f32 noise` became `var noise: color_texture` (or
  `material.shader = other` where `other` has a texture with an old value's
  name): `_shaderTexture` casts the stored double `as Texture2D?` →
  `TypeError` thrown out of `reload` after `_pointer`/`_layout` were
  already swapped, so the other worlds and effects aren't updated and the
  old native shader is never freed.
- **Fix**: (a) write the one changed value into a kept `_bytes` at its
  offset and send that range (`fy_material_values_set` already takes an
  offset); (b) look textures up with `is Texture2D` instead of a cast, and
  drop values whose kind no longer matches on re-layout.
- **Test that fails today**: alloc harness with `material.set('x', v)` per
  frame → 0 bytes; reload a shader changing `x` from a value to a texture
  with `x` set to 1.0 → no throw.

## DF8. Follow's lookAhead snaps on pause frames

- **Severity**: bug (small). **Size**: S.
- **Where**: cameras.dart:214-220.
- **How it shows**: `Follow(player, lookAhead: 0.5)` with `damping: 0`.
  While the world is paused (or `time.scale = 0`), `dt` is 0, so the
  lead term is skipped and `goal` falls back to `p + offset`: the camera
  jumps back by `lookAhead × velocity` the first paused frame and jumps
  forward again the first frame after. (With damping, `_damp(d, 0)` is 0
  and it holds, so only the undamped case shows.) `_targetWas` is also
  overwritten on paused frames, so the lead is right again only one frame
  after resuming.
- **Fix**: when `dt == 0`, leave the rig where it is (return early) and
  don't update `_targetWas`.
- **Test that fails today**: target moving at 2 m/s, lookAhead 0.5, then
  `world.paused = true; update` → camera position unchanged (today it
  moves 1 m back).

## DF9. Animator.at marks can't be removed and pile up

- **Severity**: awkward. **Size**: S.
- **Where**: animator.dart:373-380 (`at` only adds), 382-387.
- **How it shows**: there's no way to take a mark off, or to replace one.
  A behaviour that registers its footstep in `onEnable` (the guide's
  pooling advice puts setup there) adds another callback per reuse: after
  five reuses each footstep fires five sounds. `onEnd` is a single
  replaceable callback while `at` piles up, so the two event APIs behave
  differently. Unity's AnimationEvents belong to the clip and are set once.
- **Fix**: return a cancelable from `at` (or `off(name, time)`), and/or
  `clearMarks(clip)`; say in the guide to register in `onLoad`.
- **Test that fails today**: `at('walk', 0.3, f)` twice with the same `f`
  → one call per pass expected (or a way to remove it exists).

## DF10. MeshBuilder throws away given normals once any vertex lacks one

- **Severity**: bug (small). **Size**: S.
- **Where**: mesh_building.dart:179-191 (`_allNormals = false` on any
  vertex without a normal), mesh_building.dart:339-349 (then no normals
  are sent and all are worked out).
- **How it shows**: `addVertex(p, normal: n)` for the vertices of a
  hand-shaded part plus one `addVertex(q)` without a normal: `applyTo`
  sends no normals at all and the engine averages faces for every vertex,
  overwriting the given ones. The doc says "Without a [normal], one is
  worked out", which reads as per vertex.
- **Fix**: send the given normals and recalculate only the missing ones
  (or say in the doc that it's all or nothing).
- **Test that fails today**: a quad of four `addVertex` with a tilted
  normal plus an extra lone vertex with none → `mesh.normals` of the first
  four equal the given ones.

## DF11. SplineInstantiate with a count throws every frame on a spline with no knots

- **Severity**: bug (small). **Size**: S.
- **Where**: spline_objects.dart:181-185, 202-205; splines.dart:565-570.
- **How it shows**: `SplineInstantiate(make, count: 5)` on a container
  whose spline starts empty (a road the player draws). `rebuild` makes 5
  copies, then `spline.rotationAt(0)` throws "a spline with no knots has no
  points"; `_placedVersion` is never set, so `onUpdate` rebuilds and throws
  again every frame, filling `world.errors`. Without `count`, 0 or 1 knot
  gives no copies (fine).
- **Fix**: with fewer than one knot, place no copies (n = 0) whatever the
  count, as the spacing path does for fewer than 2.
- **Test that fails today**: the above, three updates → no errors.

## DF12. Mesh.inBackground loses the mesh's UV and dynamic flags

- **Severity**: bug (small). **Size**: S.
- **Where**: resources.dart:473-482.
- **How it shows**: only the pointer, name and own-normals/tangents flags
  cross back; the new `Mesh._` defaults `_hasUvs = true` and
  `_dynamic = false`. A `Mesh()` built in the background without UVs, then
  changed with `setPositions` on the main isolate, has tangents worked out
  from all-zero UVs (`_settle` asks for them because `_hasUvs` is wrong).
  A mesh `markDynamic()`ed inside `build` reads `isDynamic == false`.
- **Fix**: carry `_hasUvs` and `_dynamic` across too.
- **Test that fails today**: `inBackground(() => Mesh()..markDynamic())`
  → `isDynamic` true.

## DF13. Each ScreenEffect made adds a WeakReference to its shader that's only trimmed on reload

- **Severity**: slow (leak, small). **Size**: S.
- **Where**: shader.dart:172-180 (`shader._effects.add`), shader.dart:130
  (the only `removeWhere`).
- **How it shows**: a game that makes a `ScreenEffect` per hit flash (one
  per hit, dropped after) grows `shader._effects` by one per hit for the
  whole session in a release build, which never reloads.
- **Fix**: trim dead entries when adding (or keep the list in a
  `WeakReference` set with a periodic sweep).
- **Test that fails today**: make 1,000 effects, drop them, force GC, make
  one more → `_effects.length` small.

## DF14. Small inconsistencies (awkward)

- **Severity**: awkward. **Size**: S each.
- `Quat.normalized` of a zero quaternion returns NaNs silently
  (math.dart:328-331) while `Vec3.normalized` throws with a message
  (math.dart:152-156); the NaN surfaces later as the transform's refusal,
  far from the cause.
- `SmoothDamp3.maxSpeed` can't be changed after construction
  (math.dart:568-574) while `SmoothDamp.maxSpeed` and `smoothTime` can.

---

## Looked at and fine

- Easing functions against easings.net (motion.dart:21-142); motion seek
  loop, yoyo, groups forwards/backwards, `then` inside routines, pause and
  `unscaled` (world_loop.dart:40-63, time.dart:119-122).
- `_builtIn` lerp/distance and its `int` hint; speed-based tweens of
  zero distance.
- Animator clip/blend indexing, layer mask errors, event marks across
  several loops in one frame, events copied before callbacks.
- Model load on a helper isolate, finalizers, `_string` retry.
- Particle event decoding matches `FyParticleEvent`'s layout (abi.rs:4823);
  pause/time.scale reach particles, animators, shaders' time.
- Sky types and validation; `lighting` written by `World.sky`.
- Cameras: blend from a running blend, `setBlend` precedence, shake fades
  and leaves the transform alone, rigs' order (movers then aimers).
- Debug line buffer growth, keep/expiry, release-build default off.
- Spline maths: Bezier handle indexing, weighted Catmull-Rom, double
  reflection frames with closed-loop twist removal, table lookup at ends,
  zero-length and one-knot cases, sampling share-out under `maxSegments`,
  mitred corners, overlapping `setRange` on insert/remove.
- MeshBuilder growth, flat path, mirrored `addMesh` winding, empty
  submeshes (engine allows equal starts, fynkel_mesh/src/lib.rs:636-649).
- Mesh change batching per frame, `_settle`, disposed meshes skipped in
  publish, built-ins refusing change and dispose.
- Material validation, `_Release` weak bookkeeping and in-place updates.
- `encodePng` (zlib header, stored blocks, Adler-32, CRC-32), `Color`
  conversions, `ColorGradient` checks.
