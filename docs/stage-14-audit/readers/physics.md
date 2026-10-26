# Stage 14 audit: physics (reader: physics)

Scope: crates/fynkel_physics, crates/fynkel_physics_ffi, packages/fynkel_physics,
and the hooks in packages/fynkel (world_loop.dart, time.dart, animator.dart,
resources.dart). Read-only; nothing was built or run. Hardening P1-P10 were
checked and held (see the end); nothing here repeats them.

---

## PH1. Changing a collider on a body that has moved puts it back where the body was when the collider was added
- **Severity**: bug. **Size**: M.
- **Where**: `crates/fynkel_physics/src/colliders.rs:290-299` (`set_collider_spec` passes `followed.object`),
  `:412-425` (`replace`), `:434-448` (`put`: `relative = body.position().inverse() * pose`).
  `followed.object` is only refreshed by `follow()` (`:322-336`), and Dart never sends a body's
  colliders to `follow` once physics moves the body: `packages/fynkel_physics/lib/src/rigidbody.dart:522-526`
  marks them `seen()` on every write-back.
  Ways in: `physics.dart:531-541` `_respec` (any setter on `Collider`: `size`, `radius`, `height`,
  `center`, `isTrigger`, `material`, `MeshCollider.mesh/convex`), `physics.dart:420-436`
  (adding a `Triggers`/`Collisions` behaviour respecs the body's colliders), `bodies.rs:203-205`
  (toggling `isKinematic` with a non-convex `MeshCollider` → `reshape_meshes` → `replace`), and
  `meshes.rs:199-201` (`set_mesh` → `replace` for every collider of a changed mesh, so this
  also happens when a stage 13 runtime mesh changes on a moving body).
- **How it shows**: `followed.object` is still the object's pose from when the collider was
  added. `replace` sets the collider's offset from the body to (body now)⁻¹ · (object then). A ball
  that has dropped 5 m and then gets `collider.radius = 0.6` ends up with its collider 5 m above
  the body. Next step the ball falls through the floor; queries find it at its spawn height.
  Static colliders aren't affected: they're followed whenever their object moves.
- **First fix**: in `replace`, for a collider on a body, keep the current `position_wrt_parent`
  and work out only the part that changes with the spec (center, capsule turn). Better is
  hardening phase 2's own rule, never built in Rust: Dart sends each collider's pose relative
  to its body, worked out from the hierarchy, so the native body's pose is never used
  (it also fixes PH2).
- **Failing test**: a dynamic ball with a `SphereCollider` dropped from y=5 onto a ground box.
  Once it rests (about 2 s), set `radius = 0.5` (the same value; `_setOrUndo` still applies it),
  step 1 s, and expect `ball.position.y ≈ 0.5`. Today it falls through. Or
  `physics.overlapSphere(ball.position, 0.1)` right after the set: empty today.

## PH2. With interpolation on, a body's colliders are placed using the drawn pose against the stepped pose
- **Severity**: bug. **Size**: M (same fix as PH1).
- **Where**: `physics.dart:1166-1190` `_interpolate` writes the drawn (lerped or extrapolated)
  pose to the object and marks it seen. `physics.dart:443` `_add` → `_follow()` doesn't teleport
  the body (it's "seen"). `physics.dart:467-473` `_addNative` sends `object.worldTransform` (drawn).
  `colliders.rs:269` and `:438` make the offset relative to the native (stepped) body pose. The same
  goes for `_reattach` (`physics.dart:518`) and for following a child collider the game moves (`:696-713`).
- **How it shows**: for an interpolated body, the drawn pose trails the stepped pose by up to one
  step's travel; extrapolated, it leads by up to that. Any collider added, enabled, re-parented or
  moved on such a body (a sword hitbox enabled mid-swing, a child collider nudged with
  `localPosition`) gets that gap baked into its offset for good: 10 cm at 5 m/s and 50 Hz. Hardening
  P8 covered queries seeing the stepped pose, not this.
- **First fix**: as PH1, offsets from the hierarchy. Or, short term, add a collider on an
  interpolated body against `body._p1/_r1` with the object's pose relative to the body object.
- **Failing test**: a rigidbody with `interpolation: interpolate` and `useGravity: false`, given
  `linearVelocity = (10,0,0)`, run at 120 fps. In a frame between steps, add a
  `BoxCollider(center: Vec3.zero)` to its object, then compare the native shape's pose (the
  `drawColliders` path, `fp_collider_shape_get`) with `rigidbody.position` after the next step. Today
  they differ by about 0.1 m along x.

## PH3. A collider disabled and enabled again before the next step gets enter and then exit, in that order
- **Severity**: bug. **Size**: S.
- **Where**: `physics.dart:487-494` (`_remove` puts the key in `_leaving`; `_byKey[key]` still
  points at the collider), `:440-445` (`_add` takes a new key), `:931-936` (old keys freed only
  after the step's contacts are told). `contacts.rs:391-414` sends all enters, then stays, then exits.
- **How it shows**: a pooled object released and taken again in the same frame, a
  `SetActive(false/true)` reset, or `enabled = false; enabled = true`, inside a trigger it stays in.
  At the next step Rust sees an Exit for (old key, zone) and an Enter for (new key, zone), and
  sends Enter first. Dart maps both keys to the same collider, so the object and the zone both get
  `onTriggerEnter(x)` and then `onTriggerExit(x)`. A zone keeping a set from enter and exit (what
  the `Triggers` doc recommends, `contacts.dart:35-36`) ends up thinking x has left while it's
  inside. The same holds for collisions. The module doc (`contacts.rs:8-11`) means pairs to keep
  their key across a reattach; disable plus enable breaks that.
- **First fix**: in `_add`, if this collider's previous key is still in `_leaving` (not yet told),
  take it back out and reuse it, so the pair goes on as a stay. Or send exits before enters in
  `contacts_after_step` (and change the test at `crates/fynkel_physics/tests/contacts.rs:243`).
- **Failing test**: a kinematic cube resting inside a trigger zone with a `Triggers` counter. In one
  frame, `cube.active = false; cube.active = true`, then step. Expect `inside == 1`: today it's
  enter, enter, exit, so 0 (or the zone's set loses the cube).

## PH4. `movePosition` and `moveRotation` use a stale target once the step has moved the body
- **Severity**: bug. **Size**: S.
- **Where**: `rigidbody.dart:322-334` picks `_moveRotation` or `_movePosition` when
  `_movedAt == physics._steps`. But `_stepped` (`:463-471`) sets `_movedAt = step` for every body
  the step moved, and `Physics::moved` (`lib.rs:311-326`) lists every awake dynamic body. The two
  meanings of `_movedAt` ("the step last moved it" and "the game called move this step") are mixed.
- **How it shows**: on any awake dynamic body after its first step, `rb.moveRotation(q)` (Unity's
  own `MoveRotation` doc example is a non-kinematic body) teleports it to `_movePosition`, which is
  `Vec3.zero` if `movePosition` was never called (`:365`). `rb.movePosition(p)` snaps its rotation
  to identity, or to the last `moveRotation`. On a kinematic body, a teleport
  (`rb.position = x`, or setting the transform) followed by `moveRotation` sends it back to the
  last `movePosition` target. The only test (`test/rigidbody_test.dart:314-333`) uses a kinematic
  body that only ever moves by `movePosition`, which is why it passes.
- **First fix**: keep a separate `_moveAt` step counter set only in `_move`. Fall back to the body's
  pose (or the dirty object's) otherwise, and clear it on `_teleport` and `Physics._teleport`.
- **Failing test**: a dynamic body at (3,5,0) falling. After 3 steps, `rb.moveRotation(Quat.axisAngle(Vec3.up, 0.5))`,
  step, and expect `rb.position.x ≈ 3`. Today it's 0. Second test: a dynamic cube spawned at 45°;
  `movePosition(p)` keeps the 45°.

## PH5. Any change to a mesh rebuilds its `MeshCollider`, and each rebuild walks every collider in the world
- **Severity**: slow. **Size**: S.
- **Where**: `packages/fynkel/lib/src/resources.dart:632-643` bumps `version` for any change. The
  setters that mark a change include `setColors`, `setUvs`, `setNormals`, `setTangents`,
  `recalculateNormals/Tangents` (`:493-598`). `physics.dart:583-595` rebuilds on any version
  change. `meshes.rs:180-202` `set_mesh` copies the points, scans **every collider in the world**
  (`self.world.colliders.iter()`) to find users, and `replace` makes a new trimesh with
  `FIX_INTERNAL_EDGES` (`meshes.rs:73`), or a new hull for dynamic users.
- **How it shows**: painting vertex colours or scrolling UVs on a ground mesh with a collider
  (stage 13's "colours by height" pattern) rebuilds the BVH each change. With a 10,000-collider
  level, each mesh change also pays an O(10,000) scan. Through PH1 it also misplaces the collider
  on a moving body.
- **First fix**: give `Mesh` a shape version bumped only by positions, triangles and `clear`
  (and sweeps and cuts). In Rust, keep a mesh → users list (or count) instead of scanning.
- **Failing test**: a mesh with a `MeshCollider`; call `setColors` 10 times over 10 frames; expect
  `mesh_shapes_made` (or a debug counter for `fp_mesh_set` calls) unchanged. Today it rises by 10.

## PH6. Following moved objects walks each moved object's whole subtree, and animators log every part every frame
- **Severity**: slow. **Size**: S. (Magnitude not measured.)
- **Where**: `physics.dart:647-669` BFS-walks the subtree of each moved object, with no dedupe
  against moved ancestors; the same collider is added to `found` and the batch once per moved
  ancestor. `animator.dart:431-442` logs every part of every playing animator as moved each frame.
- **How it shows**: with fewer moved objects than colliders and bodies (the `:628` shortcut doesn't
  fire), each frame's follow costs the sum of subtree sizes, about parts × depth for a rig (around
  60 × 8 to 12 for a Mixamo skeleton), even when no collider sits under the rig. Duplicate ids in the
  follow batch are harmless (`colliders.rs:325`) but are read and sent anyway.
- **First fix**: skip a moved object whose ancestor is also in this batch (stamp a frame number on
  visited objects). Better, keep a per-object count of physics components below it and skip
  subtrees with none.
- **Failing test**: a timing/visit-count test. 10 animated characters, 1 collider elsewhere, 2,000
  static colliders; count objects visited per `_followMoved`. Expect at most parts + colliders;
  today it's around parts × depth.

## PH7. Per-step loops over every collider or joint, not the ones involved
- **Severity**: slow. **Size**: S.
- **Where**: `contacts.rs:337-360` scans every followed collider slot (the static level included)
  each step once anything listens. `joints.rs:439` (every joint, each step, once one joint is driven)
  and `:505` (every joint, once one is breakable). `physics.dart:800-805` `_removeBody` loops every
  joint per body removed. `fp_colliders_follow` allocates a `Vec<ObjectPose>` per batch
  (`physics_ffi/src/lib.rs:1427`).
- **How it shows**: one coin trigger in a 10,000-box level costs a 10,000-slot scan per step. One
  breakable joint makes a 100-ragdoll crowd's 1,500 joints get checked each step, sleeping ones too.
  Each is small (tens of µs) but grows with the world, not with what moved.
- **First fix**: keep a list of listening collider handles (the count already exists:
  `contacts.listening`). Keep lists of driven and breakable joint handles. Index joints by body in Dart.
- **Failing test**: a counter (debug) of slots visited in `contacts_after_step`: 10,000 static
  colliders and 1 listener; expect 1 or close to it.

## PH8. Two raycasts that see different worlds, and particles that see neither
- **Severity**: awkward (seam). **Size**: S (docs) / M (merge).
- **Where**: `packages/fynkel/lib/src/world.dart:897` `World.raycast` hits drawn meshes and returns
  `RayHit`. `packages/fynkel_physics/lib/src/queries.dart:176` `physics.raycast` hits colliders and
  returns `RaycastHit`. Only the pointer path merges the two (`pointing.dart:34-46`). Particles
  bounce off up to 6 plane objects only (`particles.dart:150-174`, `:1066`), with no collider
  ("World") mode, and report `onParticleCollision(GameObject plane, count)`.
- **How it shows**: a Unity user typing `world.raycast` from a behaviour gets mesh hits that ignore
  colliders, triggers and layers' collision matrix, and miss invisible colliders (an invisible wall,
  a trigger zone). `physics.raycast` misses drawn things without colliders. Similar names, different
  result types (`RayHit.gameObject` vs `RaycastHit.collider`). Sparks don't land on a physics floor
  unless the game adds a plane object for them.
- **First fix**: the guide's physics section says which to use. Name the core one
  `world.raycastDrawn` (or keep it and point to physics in its doc). Later, a particle collision
  mode backed by `physics` queries when the package is present.
- **Test**: none fails today. A guide check that the physics section names `physics.raycast`.

## PH9. Animation and physics don't meet: animated parts are teleported each frame, and there's no root motion
- **Severity**: awkward (seam). **Size**: M.
- **Where**: `animator.dart:431-442` (every part re-stamped and logged after the updates, at render
  rate), then `physics.dart:663-664` / `:847-854` `_teleport` for any `Rigidbody` on a part. There's
  no `applyRootMotion`, `onAnimatorMove` or "animate physics" mode (grep finds none in `animator.dart`).
- **How it shows**: kinematic bodies on bones (hitboxes, a shield that should shove crates) move by
  teleports (`lib.rs:279-284`). They have no velocity, so they shove only by penetration correction,
  report zero `relativeVelocity`, and don't carry what stands on them. A dynamic `Rigidbody` on a part
  of a playing model is teleported back every frame (a ragdoll needs the animator stopped first;
  not checked whether `stop()` stops the stamping, since `_moving` is decremented elsewhere). An
  interpolated body on a part loses its interpolation each frame (`_reset`). A character can't be
  driven by its clip's root motion into a `CharacterController` or `Rigidbody`.
- **First fix**: an Animator setting that, for parts with kinematic rigidbodies, calls
  `movePosition/moveRotation` in the fixed step instead of teleporting (Unity's AnimatePhysics).
  Stamp only the parts a clip moves. Root motion as its own row.
- **Test**: an animated cube part with a kinematic rigidbody sweeping into a dynamic box; the box's
  velocity after contact ≈ the part's speed. Today it's popped out with little or no velocity.

## PH10. A joint whose connected body is destroyed waits forever, silently
- **Severity**: awkward. **Size**: S.
- **Where**: `physics.dart:1096-1111` `_joinWaiting` skips a joint while `connected._id == 0`.
  `joints.dart:96-98` keeps `_connectedBody` pointing at the destroyed component. `_removeBody`
  (`physics.dart:799-805`) parks the joint in `_waiting`.
- **How it shows**: destroy the anchor of a chain and the joint stays enabled but holds nothing.
  `isJoined` is false with no report, `connectedBody` returns a destroyed `Rigidbody`, and the
  joint is checked in `_joinWaiting` on every `_follow` (each step and each query) for good.
- **First fix**: when a connected body is destroyed (not just disabled), set `connectedBody` to null
  and join to the world, or remove the joint and report it once. Keep waiting only for a disabled body.
- **Test**: two cubes joined by a `FixedJoint`; destroy the connected one; after a frame expect
  `joint.connectedBody == null` and either `isJoined` or the joint gone. Today it's waiting with a
  destroyed body.

## PH11. The default between-frames step shows each fixed step one frame later than Unity does
- **Severity**: awkward. **Size**: S (decide and document).
- **Where**: `physics.dart:93` (`stepBetweenFrames = true` by default), `:370-382` (the step starts in
  `afterFrame`), `:299-305` (collected in the next `beforeFrame`).
- **How it shows**: frame N draws step k-1. Forces from frame N's `onFixedUpdate` show in frame N+1,
  and their `onCollisionEnter` arrives in frame N+1, where Unity runs both before frame N's
  `Update`. That's one extra fixed step (20 ms) between input and what's seen, on top of
  interpolation's step. It's deliberate (stage-5 §6, "an experiment") and documented on the field,
  but it's the default every game gets.
- **First fix**: measure what the thread saves on the Note10+ with a typical scene. If it's small,
  make `false` the default and keep the thread as an opt-in.
- **Test**: none fails. A doc/guide row stating the latency.

---

## Looked at and fine
- Hardening phase 2 held. P1: `_follow()` at the top of `_add` (`physics.dart:443`) and
  `_removeBody` (`:784`). P2: `_putAll` writes by depth (`:893-909`), with `_prepare` before any
  `_write`. P3: `position=` takes the object's rotation when dirty (`rigidbody.dart:241`). P4: the
  character bit is in the spec (`colliders.dart:183`, FFI `:1393`). P6: per-scale shape LRU of 8
  (`meshes.rs:97-115`). P7: swap-remove lists, and `componentAdded` walks only the subtree. P9:
  `seen()` after a successful batch (`physics.dart:721-726`). P10: `remove_body` lowers `listening`
  (`lib.rs:197-204`).
- Fixed-step accumulation: `maxFixedSteps` drop (`world_loop.dart:265-267`, `time.dart:101`),
  `scale`/pause give no steps (`time.dart:120`), and `fixedAlpha` is clamped. Interpolation's two
  branches (step due vs not) both show "one step behind" consistently.
- The between-frames thread: every native access goes through `_world`, which collects first; a
  `World.dispose` during a running step joins the worker and drops the core without a leak
  (`Worker::drop`, `fp_world_free`). A second fixed step in a frame runs the pending one first.
- Contact keys stay mapped until the step naming their exit has been told
  (`physics.dart:923-939`), so exits after destroy go to the right collider. Destroyed and hidden
  objects get no own-side exit, the other side does (as documented).
- Joints: body or joint disable order either way leaves no dangling native joint. Broken joints are
  read in `_applyMoved` and removed after the callbacks.
- Queries `_follow()` first; `_overlap` and `_readHit` map by key; the layer matrix updates
  existing colliders' groups (`contacts.rs:212-222`); queries ignore the matrix, as Unity's do.
- `CharacterController`: world-space `position` (`motion.dart:467`), ground carry after the late
  updates, ground by key from the same move, and its own collider excluded from its casts.
- A mesh change is published (`world.dart:768`) before physics' `afterFrame` (`:771`), so the step
  sees it; a rebuild can't race a running step (`_follow` collects first).
