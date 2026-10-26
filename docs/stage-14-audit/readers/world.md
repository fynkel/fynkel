# Stage 14 audit: world, mesh, load (reader "world")

Area: crates/fynkel_world, crates/fynkel_mesh, crates/fynkel_load, plus the
fynkel_ffi code that feeds them (abi.rs, extract.rs). Read-only; nothing built
or run. Line numbers are on branch stage-14 at edbe57aa.

---

## WO1. A sparse accessor in a skin or animation makes the importer allocate without bound and abort the process
- **Severity:** bug
- **Size:** S
- **Where:**
  - The check exists only for primitives: crates/fynkel_load/src/model.rs:608-621.
  - Skin inverse binds skip it: model.rs:418-430 (`accessor::Iter::<[[f32;4];4]>` ... `.collect()`).
  - Animation input and output skip it: model.rs:481-495 (`Iter::<f32>` ... `.collect()`, and `floats::<N>`).
- **How it shows:**
  - The primitive path knows "a small file can claim billions of them" and refuses `accessor.count() > blob.len()`. The skin and animation accessors are read with no such check.
  - In gltf 1.4.1, a sparse accessor with no bufferView yields `count` zeros. Its iterator's exact size_hint means `collect()` preallocates count x 4..64 bytes at once (gltf-1.4.1/src/accessor/util.rs:141-175, 306-352).
  - So a .glb of a few hundred bytes, whose animation sampler input is `{"count": 1000000000, "sparse": {"count": 0, ...}}`, asks for 4 GB (64 GB for inverse binds).
  - A failed allocation aborts the process. The catch_unwind around the load (model.rs:150) can't catch an abort, so a bad file kills the game instead of returning Err.
- **First fix:** a single helper used by every accessor read (attributes, indices, skins, animation in and out) that refuses `count > blob.len()`. Better still, refuse `count * element_size > blob.len()` when it isn't sparse.
- **Test that fails today:** load a hand-made glb with one node, one animation channel and a sparse input accessor (count 1e9, no bufferView). Expect `Err`; today the test process aborts.

## WO2. Despawning a leaf child leaves a stale entry in WorldMatrices, so `matrix()` returns Some for a dead object
- **Severity:** bug
- **Size:** S
- **Where:**
  - crates/fynkel_world/src/hierarchy.rs:79-86 (detach prunes the child's node without bumping version)
  - hierarchy.rs:284-292 (take_subtree returns None early without bumping `version`)
  - hierarchy.rs:334-348 (update keeps the stale order)
  - hierarchy.rs:363-368 (`matrix`)
  - The consumer: crates/fynkel_ffi/src/extract.rs:419-423
- **How it shows:**
  - Take P with leaf child C. Despawning C runs `detach(C)`, which removes C from P's children and prunes C's node (and P's, if P is left with nothing).
  - `take_subtree` then sees no node and returns None. `version` is bumped only inside the loop at 299, which isn't reached.
  - So `WorldMatrices.version == tree.version` and no rebuild happens. (C, dead entity, parent index) stays in `order`/`index`.
  - `update` keeps computing it from `world.local_matrix(dead entity)` (identity or the fallback), and `matrix(C)` returns `Some(P * that)`.
  - The doc says None "when it's gone". extract.rs:420 relies on None ("A joint destroyed: drawn as bound"). Instead a destroyed leaf joint (a finger tip, a weapon socket) bends its vertices to the parent's origin.
  - `world()` and `shown()` also answer for the dead id until some other hierarchy change rebuilds.
  - This is only cosmetic for skins, but it is a real seam: two answers for "is this gone".
- **First fix:** bump `version` whenever `prune` removes a node or `detach` changes a parent. The simplest place is in `detach` itself.
- **Test that fails today:** parent P, child C; `m.update(&w)`; despawn C; `m.update(&w)`; `assert!(m.matrix(&w, C).is_none())`. Today it is Some.

## WO3. Raycasts hit what the mesh is at bind pose, not what is drawn, for skinned meshes; also undrawn submeshes
- **Severity:** bug (seam)
- **Size:** M
- **Where:** crates/fynkel_world/src/raycast.rs:89-115; compare crates/fynkel_ffi/src/extract.rs:413-429 and the GPU skin path (skin.wgsl, vs_skinned).
- **How it shows:**
  - The module promises a ray hits "what a view draws". For a renderer with a skin, cast tests the mesh's bind-pose triangles under the object's own world matrix.
  - The renderer instead places every vertex by its joints' matrices x inverse binds, and ignores the object's matrix for skinned vertices.
  - Result: a character in a walk clip, or one whose root bone is moved, is hit where its T-pose would be. A ray at its drawn raised arm misses, and a ray through empty air where the bind-pose arm was hits.
  - Also, every submesh is tested, including ones with no material slot that the view doesn't draw. `double_sided` is taken from the first material only, so a mesh whose second material is double-sided can't be hit from behind there.
- **First fix:**
  - Short term: document it, and test skinned renderers by their bounds only or by joint-space boxes.
  - Proper: CPU-skin the positions for the cast with `MeshData::skin_vertex` (fynkel_mesh lib.rs:676-688) and the same joint matrices extract uses, then cast only drawn submeshes with each one's own material sidedness.
- **Test that fails today:** load the knight with an idle clip that raises an arm (or move the hips joint 1 m up); after `animate`, a ray at the drawn hips must hit, and a ray at the bind-pose hips must miss. Today it is the reverse.

## WO4. Each raycast recomputes every world matrix in the scene and tests every renderer and triangle linearly
- **Severity:** slow
- **Size:** M
- **Where:** crates/fynkel_world/src/raycast.rs:79-126 (`matrices.update(world)` at the top of `cast`, then a loop over all renderers and a triangle loop per hit box); reached once per `fy_raycast` (crates/fynkel_ffi/src/abi.rs:1685-1712) and so once per Dart `world.raycast`.
- **How it shows:**
  - A game that casts 20 rays a frame (AI sight lines, ground checks, the cursor) pays 20 full hierarchy passes: a hecs lookup and a Mat4 multiply per object in a tree. It also pays 20 walks over every renderer.
  - Any mesh whose box is hit has every triangle tested with Moller-Trumbore. There is no BVH and no early-out by distance order.
  - A 50k-triangle terrain under the cursor costs 50k tests per cast.
  - The cost grows with scene size x casts, all on the thread that called it (the game's update).
- **First fix:**
  - Keep a world-owned WorldMatrices, updated once per frame (or dirty-flagged), and have cast read it.
  - Sort candidates by box-entry distance and stop at the first hit nearer than the next box.
  - Build a lazily-made BVH per mesh (rebuilt when MeshData::change bumps it).
- **Test that fails today:** a timing test with 3000 objects in trees of depth 4 and one 50k-triangle mesh; 50 casts must take under 2 ms in release. Expected to be well over today; not measured (read-only).

## WO5. Local-space collision planes are moved into local space by the inverse, not the inverse transpose
- **Severity:** bug
- **Size:** S
- **Where:** crates/fynkel_world/src/particles.rs:1796-1804 (`normal = into.transform_vector3(normal)` with `into = emitter.inverse()`).
- **How it shows:**
  - A normal must be moved by the inverse transpose of the point transform. With `into = M^-1`, that is `M^T`, not `M^-1`. The code is right only when the emitter has no scale or only uniform scale.
  - Example: emitter scaled (4,1,1), Space::Local, and a ground plane tilted 45 deg about Z (world normal ~(-0.71, 0.71, 0)).
    - The local normal comes out ~(-0.24, 0.97); the right one is ~(-0.97, 0.24).
    - Particles bounce off a plane at the wrong angle, and the side test (`dot(p - point, normal)`) puts the surface in the wrong place, so some particles pass through and some are stopped in mid air.
  - Squashed emitters, such as a flattened fountain or a stretched exhaust, are common.
- **First fix:** `normal = emitter.transpose().transform_vector3(normal)` (that is, (M^-1)^T), then normalise; or move the particles into world space for the test.
- **Test that fails today:** the setup above. One particle starts above the plane with velocity straight down in world space; after hitting, its world velocity must be the mirror about the world normal. Today it isn't.

## WO6. Emission and simulation loops aren't bounded by free room or a step cap; big settings freeze the calling thread
- **Severity:** bug
- **Size:** S
- **Where:** crates/fynkel_world/src/particles.rs:
  - 1084-1087 (`ParticleCommand::Emit(n)` loops n times)
  - 1246-1253 (rate)
  - 1206-1217 (rate over distance: a teleport of 10 km at 100/m loops 1e6)
  - 1269-1272 (burst `count.pick().round() as u32`)
  - 1431-1436 (sub-emitter bursts per trigger)
  - 1784 (`simulate_particles`: `ceil(seconds * 30)` steps)
  - 1055 (prewarm: `duration * 30` steps, and duration has no upper bound in the settings check at 545-730)
- **How it shows:**
  - Each loop runs its full count even when the pool is already full. Each pass calls the spawn path, which returns early but only after picking random values.
  - `Emit(4_000_000_000)` on a 100-particle system spins ~4e9 times on the thread holding the world lock; that is seconds to minutes.
  - `Simulate(1e7 s)` runs 3e8 full steps. A prewarmed system with duration 1e6 runs 3e7 steps when it plays.
  - All of these values are accepted from Dart. Nothing panics, but the game hangs and the UI thread waiting on the lock hangs with it.
- **First fix:**
  - Clamp n to `max_particles - count` before each loop, and stop when full.
  - Cap simulate/prewarm steps: at most `lifetime_max * 30 + duration * 30`, and in any case a few thousand. After that only the steady state matters, so it can jump.
  - Bound `duration` in the settings check.
- **Test that fails today:** `Emit(u32::MAX)` on a pool of 10 must return within 10 ms; `simulate(1e7)` likewise. Both run for a very long time today.

## WO7. Prewarm runs before the collision planes are found, and prewarm deaths fire sub-emitters on the first frame
- **Severity:** bug
- **Size:** S
- **Where:** particles.rs:1828 (`play_from_start(emitter)`, which prewarms at 1055) runs before `find_planes` at 1845; triggers are collected during the prewarm steps.
- **How it shows:**
  - For a new system, `plane_count` is still 0 during prewarm, so all prewarmed particles fall through the floor. From the first visible frame there are particles below the ground plane; they live out their lifetime there.
  - The prewarm steps also push every death and collision into `triggers`. On the first real step, every sub-emitter fires them at once: a burst of sparks or splashes at frame 1 that a non-prewarmed system never shows.
- **First fix:** call `find_planes` before `play_from_start` (it only needs the emitter matrix); clear `triggers` after prewarm.
- **Test that fails today:** prewarm on, gravity, a collision plane at y=0 with dampen 1; after the first `step`, no particle has y < 0. Also a death sub-emitter: after frame 1 the sub system has 0 particles.

## WO8. Meshes swept or changed each frame allocate every frame
- **Severity:** slow
- **Size:** M
- **Where:**
  - crates/fynkel_mesh/src/sweep.rs:99-174: `swept` builds a whole new Shape (a Vec per ring for `ring`, `ids`, `along`, `outward`, `ends`; `points`, and caps re-run `fill`), then calls `MeshData::new`, which allocates the vertex and index arrays again.
  - sweep.rs:66: `self.vertices = swept.vertices`, a new allocation each call rather than reusing.
  - crates/fynkel_mesh/src/lib.rs:591-593: `recalculate_tangents` allocates two Vecs per call, used by `change` on every frame for a changing mesh.
- **How it shows:**
  - The stage 13 promise is a rope or road "swept again each frame". Each sweep allocates on the order of 10 + 5 x outline-rings Vecs and two full copies of the mesh.
  - For a 100 m road with 200 samples x 8-point outline, that is ~100 KB of allocations per frame on the game thread. This is the garbage the hardening alloc harness exists to catch, but the lab measures time, not allocations.
  - The index-equality check (65) also compares the whole index array every frame.
- **First fix:** keep a scratch `Shape` (and the fill result, keyed by the outline) on the MeshData or in a thread-local. Write vertices in place when the count is unchanged, as `change` does. Compare a cheap key (path len, outline ptr/len) instead of the full index array.
- **Test that fails today:** with the counting allocator from the alloc harness, sweep the same 50-point path into the same mesh 10 times after one warm call; expect 0 allocations. Today it is dozens per call.

## WO9. Cap filling is quadratic-to-cubic, can put triangles outside the shape, and drops an outside hole from the caps but not the walls
- **Severity:** slow, with a bug
- **Size:** M
- **Where:**
  - crates/fynkel_mesh/src/outline.rs:133-175 (`clip_ears`: every candidate ear tests every remaining point, and removal is `Vec::remove`)
  - outline.rs:92-95 (a hole that can't be bridged is dropped)
  - The "most convex corner" fallback in clip_ears
  - crates/fynkel_mesh/src/made.rs extrude side walls (~227-257), built for every hole regardless
- **How it shows:**
  - (a) Time:
    - A convex 2000-point outline (a smooth circle cap) is O(n^2), about 4M point tests plus O(n^2) shifting. A reflex-heavy outline goes toward O(n^3).
    - This runs on every `sweep_into` with caps (WO8), so a capped tube with a fine outline does it every frame.
  - (b) Shape:
    - When no ear is found (a self-touching or self-crossing outline), the fallback clips the most convex corner. That can be a triangle that covers a notch, so the cap shows a filled patch outside the outline.
    - I did not run this, so how often it happens is uncertain. Ear clipping with that fallback is known to do it.
  - (c) Holes:
    - A hole that lies outside the outline is silently left out of `fill`, but extrude and sweep still build its side walls.
    - The result is a free-floating open tube with no caps beside the solid. A hole that touches the outline gives similar trouble.
- **First fix:**
  - Validate holes up front (inside the outline, not crossing it) and return `MeshError::Outline` instead of dropping them.
  - Use a linked-list ear clipper with a reflex-vertex set (as earcut): O(n) per ear test on reflex points only, with no Vec::remove.
  - Cache the fill result per outline for sweeps.
- **Tests that fail today:**
  - `fill` on a 10 000-point circle must take under 5 ms (it takes far longer; not measured).
  - `extrude` with a hole entirely outside the outline must return Err; today it returns Ok with stray walls.

## WO10. Every view recomputes every world matrix every frame; there are no dirty flags
- **Severity:** slow
- **Size:** M
- **Where:** crates/fynkel_world/src/hierarchy.rs:334-348 (update loops over every object in a tree with a hecs `local_matrix` lookup each); crates/fynkel_ffi/src/extract.rs:272 (each view owns its WorldMatrices and updates it in `fill`).
- **How it shows:**
  - With N objects in trees, each frame costs N hecs lookups plus N Mat4 multiplies per view, even when nothing moved.
  - Two views (a split screen or a CameraView minimap) double it, and every raycast adds another (WO4).
  - The order is cached, but the values never are.
  - It is cheap per object, so this is optional, but it scales with scene size, not with what changed.
- **First fix:** a single WorldMatrices owned by the World, updated once per frame (after animate and physics), with a changed-transform bit per object so that subtrees with nothing changed are skipped. Views and raycasts read it.
- **Test that fails today:** a counter of `local_matrix` calls per frame with 2 views and 1000 static objects; expect about 0 after the first frame. Today it is 2000.

## Lower (raise only if cheap)
- **WO11. Trail clock is f32 and grows forever**
  - Severity: bug, low. Size: S.
  - Where: crates/fynkel_world/src/trails.rs:135.
  - How: `clock += dt` as f32. After about 2-3 days of running, dt (16 ms) is smaller than half an ulp of the clock, so the clock stops and points never expire. Trails then grow to the 512-point ring and stay.
  - Fix: f64, or store ages instead of birth times.
  - Test: set the clock to 2^22 s, step for 1 s; a point with a 0.5 s life must be gone.
- **WO12. Particle start colour isn't checked finite**
  - Severity: awkward. Size: S.
  - Where: particles.rs:545-730.
  - How: the settings check refuses non-finite sizes, speeds and so on, but not `start_color`. A NaN colour from Dart reaches the vertex buffer and draws black or NaN-blended pixels, where the rest of the settings would give a clear error.
  - Test: settings with a NaN start colour must be refused.
- **WO13. Animator crossfades ignore playback speed**
  - Severity: awkward. Size: S.
  - Where: crates/fynkel_world/src/animation.rs:279-329 (fades advance by raw dt; clip time advances by dt x speed). A looping clip played backwards also doesn't count its loops.
  - How: `animator.speed = 0` (a pause) still finishes a running crossfade. A 0.3 s fade at speed 2 takes 0.3 s, not 0.15 s. Unity scales transitions by the Animator speed.
  - Test: speed 0 mid-fade; after 1 s the weight must not have changed.
- **WO14. A sweep's cross-section can flip when up is parallel to forward**
  - Severity: awkward. Size: S.
  - Where: sweep.rs:93 (`any_orthonormal_vector` when forward x up is 0).
  - How: a vertical stretch of path with a world-up `up` gets an arbitrary right vector, unrelated to the previous point's. The road or rope twists suddenly at that sample.
  - Fix: carry the previous frame's right (parallel transport) when up is degenerate.
  - Test: a path straight up for 3 samples between two level ones; adjacent rights must stay within 90 deg.

---

## Looked at and fine
- **Ids and entities:** ids are never reused (IdCounter); hecs entities are generation-checked; IdMap hashing; animator node entities.
- **Despawn cleanup:** despawn cleans up animators, skins, particle systems and trails (world.rs:274-304). Material, mesh and shader in-use checks hold before removal.
- **Animation:**
  - Clip sampling: step, linear, cubic, and slerp along the shortest path (fynkel_load clip.rs:57-91).
  - Layer and 1D-blend weights normalised; mix-slot floor; animators that aren't shown keep their time.
- **glTF loading:**
  - Clip times checked sorted and finite (model.rs:497-503).
  - Skins: joint cap and non-finite inverse binds refused.
  - Images: size read from the header and capped at 8192 before decoding.
- **Mesh maths:**
  - Winding, normals, tangent sign and UV origin checked for cube, sphere, capsule, cylinder and caps, cone, torus, disc, quad/plane, box/grid faces, extrude ends and sides, lathe, and sweep sides and caps.
  - `Mesh.combine` flips winding and tangent w for mirrored transforms.
  - MikkTSpace can't fail.
  - The ABI checks segments (<= 1024), finite and positive sizes, inner < radius, and lathe sides >= 3 before the asserts in the shape builders. The `expect` at shapes.rs:261 needs sizes near f32::MAX to fire.
- **Describe and components:** validation of finite values and unit quaternions. A camera with near >= far is refused per frame with a message in extract (extract.rs:291-299).
- **Particles:**
  - Gravity in local space uses the inverse correctly (directions, not normals).
  - Kill swap-remove keeps the arrays dense.
  - Trigger and living capacity guards.
  - Trails: the 512-point ring wraps correctly.
- **Extract:** the skin key (Arc pointer) is only used within one frame's map, which is cleared each frame, so reuse after free is harmless.
