# Stage 14 audit: physics, checked

Checker ran on a worktree of 2f72cbd5 (stage-14 base). Tests (copied to /tmp/s14/tests/physics/):
- `packages/fynkel_physics/test/audit_s14_test.dart` (PH1, PH2, PH3, PH4, PH4b, PH5, PH10, GT6, GT6b)
- `packages/fynkel_physics/test/audit_s14_follow_test.dart` (PH6, timing)
- `crates/fynkel_physics/tests/audit_s14.rs` (PH7, timing; run `cargo test --release -p fynkel_physics --test audit_s14 -- --nocapture`)

Run: `cd packages/fynkel_physics && dart test test/audit_s14_test.dart` → 11 failures, 2 passes (GT6, GT6b).

---

## PH1. Changing a collider on a moved body puts it back where the body was when the collider was added — CONFIRMED (test). Severity bug (high), size M.
`set_collider_spec` (colliders.rs:290) passes `followed.object`, which only `follow()` refreshes. A body's own colliders are marked `seen()` on every write-back (rigidbody.dart ~522), so they're never sent to `follow`. `replace` → `put` then computes `body_now⁻¹ · object_then`. Test `PH1 (in the fixed step|between frames)`: a ball dropped from y=5 rests at 0.5; `collider.radius = 0.6`; `overlapSphere(ball.position, 0.05)` is `[]`, and 50 frames later the ball is at y = −3.97 (fell through the floor). Fails in both step modes. Every path the reader lists goes through the same `replace` (respec, adding Triggers/Collisions, `isKinematic` toggle with a non-convex MeshCollider, `set_mesh` on a moving body), so the blast radius is as wide as claimed.

## PH2. Colliders added to an interpolated body are placed with the drawn pose against the stepped pose — CONFIRMED (test). Severity bug, size M.
Test `PH2`: an interpolated body, `useGravity: false`, 10 m/s along x, 120 fps, fixed step not between frames. In a frame where drawn and stepped x differ by > 5 cm, `addComponent(BoxCollider())`; 6 frames later a ray along −x hits the box face at `body.x + 0.5 − 0.183`: the box sits 18 cm behind the body for good (reader estimated ~10 cm; the gap is up to one step's travel, 20 cm here). `_addNative` sends `object.worldTransform` (drawn) and Rust uses the native body pose (stepped).

## PH3. A collider disabled and enabled before the next step gets Enter then Exit — CONFIRMED (test). Severity bug, size S.
Test `PH3`: a 0.5 m dynamic cube (no gravity) inside a 4 m trigger zone whose `Triggers` keep a set. After `cube.active = false; cube.active = true` and 10 frames the zone's log is `[enter, exit]` and its set is empty, in both step modes. One correction: the reader's "a counter ends at 0" is wrong — a counter gets +1 −1 and stays right (1); it's the set/flag pattern the `Triggers` doc recommends that breaks. The test uses a set.

## PH4. `movePosition`/`moveRotation` use a stale target once a step moved the body — CONFIRMED (test). Severity bug, size S.
`_stepped` sets `_movedAt = step` for every body in `moved`, and `movePosition`/`moveRotation` read `_movedAt == _steps` as "the game moved it this step". Test `PH4`: a dynamic body falling from (3,5,0), after 4 frames `moveRotation(...)` → x becomes 0.0 (teleported to the default `_movePosition = Vec3.zero`). Test `PH4b`: a falling dynamic body turned 45° gets `movePosition(pos + (1,0,0))` → its rotation snaps to identity (quaternion diff 0.46). Both fail in both step modes. Note: a dynamic body with gravity off and no velocity is not in `moved` (`still`), so the bug needs an awake moving body — which is the common case.

## PH5. Any mesh change (colours, UVs, normals) rebuilds its MeshCollider; each rebuild scans every collider — CONFIRMED (test for the rebuild; reading for the scan). Severity slow, size S.
Test `PH5`: a mesh with `MeshRenderer` + `MeshCollider`; 10 frames of `setColors` → `mesh.version` goes 1 → 11. `_changeable()` puts any change in `_changedMeshes`, `_publishChanges` bumps `_version`, and `_followMeshes` (physics.dart:583) calls `fp_mesh_set` on any version change, which rebuilds the trimesh (FIX_INTERNAL_EDGES) or hull. `set_mesh` (meshes.rs:180) iterates `self.world.colliders.iter()` to find users: O(all colliders) per changed mesh — plain from the code, not timed.

## PH6. Following moved objects walks each moved object's whole subtree; animators log every part every frame — CONFIRMED (test, timing). Severity slow (modest), size S.
`_followMoved` BFS-walks every moved object's subtree with no dedupe (physics.dart:647-669); `_animate` logs every part of every moving animator each frame (animator.dart:431-442). Test `PH6` (2,000 static colliders elsewhere, so the `:628` shortcut doesn't fire): moving every object of a 300-deep chain then querying costs 553 µs of follow vs 35 µs for 300 flat objects (move cost subtracted) — ~16×, i.e. ~n²/2 visits. A real rig (60 parts, depth 8–12) is far less extreme (~5–10× its part count), so tens of µs per character, not ms.

## PH7. Per-step loops over every collider or joint — CONFIRMED (test for contacts, reading for joints/_removeBody). Severity slow (small), size S.
`contacts_after_step` (contacts.rs:337) iterates every `followed` slot once anything listens; `joints_before_step`/`joints_after_step` (joints.rs:439, 505) iterate all impulse joints once one is driven/breakable; `_removeBody` loops all `_joints`; `fp_colliders_follow` collects a `Vec<ObjectPose>` per call (ffi lib.rs:1427). Test `audit_s14.rs` (release): an empty-ish world's step is 0.1 µs; one listening trigger adds +0.4 µs at 1,000 statics, +9.1 µs at 10,000, +28 µs at 40,000 — linear in static colliders, as claimed. Real but small.

## PH8. Two raycasts that see different worlds, particles that see neither — PARTLY. Severity awkward (low), size S.
The seam is real (`World.raycast` hits drawn meshes, doc says "No physics is involved"; `physics.raycast` hits colliders; particles collide only with up to 6 planes). But the reader's first fix (the guide says which to use) is already done: docs/writing-a-game.md:2870 ("`world.raycast` still…"), :3989 (RaycastHit vs RayHit) and :4068-4069 (`Physics.Raycast` → `physics.raycast`; against what's drawn → `world.raycast`). What's left is the particle "World" collision mode — a feature gap, not a bug.

## PH9. Animation and physics don't meet (teleported parts, no root motion) — CONFIRMED (reading). Severity awkward (gap), size M.
`grep -rn "rootMotion|onAnimatorMove|animatePhysics"` over packages/fynkel and packages/fynkel_physics finds nothing. Animated parts are restamped and logged (animator.dart:431-442) and any `Rigidbody` on them is `_teleport`ed in `_followMoved` (physics.dart:663-664). No test: needs a skinned/animated model in a headless test, and the consequence (teleports carry no velocity) follows directly from `fp_body_pose_set`. It's a missing feature (Unity's AnimatePhysics / root motion), better tracked as a roadmap row than a bug.

## PH10. A joint whose connected body is destroyed waits forever, silently — CONFIRMED (test). Severity awkward, size S.
Test `PH10`: a hanging cube with a `FixedJoint` to a kinematic anchor; `anchor.destroy()`; 3 frames later `connectedBody` is still the destroyed anchor's Rigidbody (`isDestroyed` true) and `isJoined` is false. `_joinWaiting` (physics.dart:1096) skips it each `_follow` for good. (Unity's behaviour here — joint to world vs. stays dangling — wasn't checked; the silent forever-wait is the issue either way.)

## PH11. Default between-frames step shows each step a frame later than Unity — PARTLY. Severity awkward (decision), size S.
True as described (`stepBetweenFrames = true` default, physics.dart:93; begun in `afterFrame`, collected next `beforeFrame`). But the latency is already documented: the field's doc, and the guide at docs/writing-a-game.md:2814 and :4100 ("…a frame later; `physics.stepBetweenFrames = false` steps as Unity does"). What remains is only the decision on the default (measure the thread's saving on Note10+).

## GT6 (guide-tests-ci). Moving a static collider leaves what rests on it asleep — NOT.
Rapier 0.36.1 wakes contact partners of modified colliders (`strong_wake_sleeping_side` in geometry/narrow_phase/mod.rs ~55, via the modified-colliders path noted in pipeline/user_changes.rs:227-231), and `put` → `set_position` marks the collider modified. Tests `GT6` (100 m floor lowered by 1 m under a sleeping ball: the ball lands at −0.5) and `GT6b` (2 m floor slid 10 m sideways from under a sleeping ball: it falls below −1) both PASS, with `isSleeping` asserted true before the move. Pocket Hole's hand-written AABB wake loop is unneeded for this reason (it may still be harmless); don't raise it as an engine gap. (Not tested: a mesh floor changed via `set_mesh`; that goes through `set_shape` + `set_position` on the same collider, so the same wake path applies, and mesh_collider_test's "a body resting on it falls when it is dug away" covers it.)

---

## New findings while testing
- PH4 has a second face not in the claim: PH4b shows `movePosition` on an awake dynamic body resets its rotation to identity even when `moveRotation` was never called (the reader said "to identity, or to the last moveRotation" — confirmed, identity here).
- None other.
