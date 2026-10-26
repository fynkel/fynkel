# Stage 14 audit, checker: world + mesh + load (WO) and render (RN)

Checked on stage-14 (edbe57aa; the code is the same as main 2f72cbd5). The throwaway tests are in
/tmp/s14/tests/world-render/, under their repo paths. Two of them sit inside a crate's src and each needs
one line added to run: `#[cfg(test)] mod wo_checks;` at the end of particles.rs, and `#[cfg(test)] mod wo11;`
at the end of trails.rs. Times are from this Mac (Apple silicon), `--release` unless noted. Expect a phone to be
several times slower.

---

## WO1. A sparse accessor in a skin or animation aborts the process: CONFIRMED (test)
Test: `crates/fynkel_load/tests/wo1_sparse.rs`. It builds a 436-byte .glb by hand: one node, one animation
channel, and an input accessor `{count: 2^44, sparse: {count: 1, ...}}` with no bufferView. The load runs in a
child process.
- Control (count 1): `RESULT: Ok(1)`.
- Count 2^44: the child prints `memory allocation of 70368744177664 bytes failed` and dies with SIGABRT (wait
  status 6). `catch_unwind` doesn't help.

Notes:
- The reader's example count, 1e9, does not abort on macOS. The 4 GB virtual allocation succeeds, and the loader
  then writes 1e9 zeros. That is still a memory/time bomb, and on Android probably a kill by the low-memory killer.
- macOS refuses allocations of 2^46 bytes or more. So the abort needs `count × element size` ≥ 64 TB: count
  2^44 for f32, or 2^40 for mat4 inverse binds.
- The skin path (model.rs:418-430) and the animation output path (`floats`) have the same `collect()` with no
  check. Only primitives check `count > blob.len()` (model.rs:608-621).

Severity is right (bug; any downloaded model can kill the game). Size S.

## WO2. A despawned leaf child keeps a stale WorldMatrices entry: CONFIRMED (test)
Test: `crates/fynkel_world/tests/wo2_stale.rs`. Both cases fail:
- A lone leaf child.
- A leaf with a sibling.

After `despawn(c)` and `update`, the dead child's `matrix()` is `Some(parent's matrix)`, `shown()` is `true` and
`world()` is `Some`.

Cause, as described:
- `detach` prunes nodes without bumping `version`.
- `take_subtree` returns None before the loop that bumps it.

Real impact today is narrow. Renderers and raycasts iterate live components, so a dead id is only looked up as a
skin joint (extract.rs:419). It does make "a destroyed joint is drawn as bound" untrue until the next change of
parent. Severity bug, low; size S (bump `version` in `detach`).

## WO3. Raycasts hit the bind pose of skinned meshes, and undrawn submeshes: CONFIRMED (test + reading)
Test: `crates/fynkel_world/tests/wo3_skinned_ray.rs`. It loads the KayKit Knight, moves its `root` joint 10 m
along X, and casts down at the centre of a skinned part's box.
- At the bind-pose place, which is now empty air, the ray hits `Knight_ArmLeft` (skinned).
- At the drawn place, the only hit is `Rectangle_Shield`, a non-skinned child of a joint. The skinned arm isn't
  hit there.

Confirmed by reading:
- vs_skinned (forward.wgsl:228) places the vertex by the joints only.
- raycast.rs uses `mesh.indices()` (every submesh) and `material.double_sided` of `r.material` only.
- extract.rs draws submesh n+1 only when `submesh_materials` has an entry.

Severity bug (seam); size M as stated.

## WO4. Each raycast recomputes all world matrices and tests triangles linearly: CONFIRMED (test, sized)
Test: `crates/fynkel_world/tests/wo4_wo10_timing.rs`. The scene has 3000 objects in 750 chains of depth 4, plus
a 49.9k-triangle grid under them.
- 50 casts take 14.2 ms, which is 283 µs a cast.
- A `WorldMatrices::update` alone is 75 µs of that. The rest is the linear triangle loop over the terrain.

The reader's bar was 2 ms for 50 casts, so this is 7× over it on a Mac and likely 20-30× over on a Note10+.
Severity slow; size M.

## WO5. Local-space collision planes use the inverse, not the inverse transpose: CONFIRMED (test)
Test: `wo_checks.rs::wo5_local_plane_normal_under_non_uniform_scale`. The emitter is scaled (4,1,1), the
system is local, and the plane is turned 45° about Z.
- The normal used is (-0.243, 0.970). The correct one is (-0.970, 0.243), exactly the reader's numbers.
- For the local point (1, 0.3, 0), the world test says -2.616 (below the plane) and the system's test says
  +0.049 (above it). So particles are stopped or let through on the wrong side.

Severity bug; size S.

## WO6. Emit, simulate and prewarm loops aren't bounded: CONFIRMED (test, sized)
Test: `wo_checks.rs::wo6_*`. Release, on a pool of 10:
- `Emit(50,000,000)` takes 169 ms, 3.4 ns per loop pass. So `Emit(u32::MAX)` is about 14.5 s on this Mac, with
  the world lock held.
- `simulate(20,000 s)`, which is 600k steps, takes 52 ms. So `simulate(1e7)` is about 26 s.
- A prewarmed system with `duration` 20,000 s is accepted by `refused()`, and its first frame takes 52 ms. A
  duration of 1e7 means a stall of about 26 s.

In debug these are about 4× slower. The rate-over-distance and sub-emitter burst loops have the same shape
(confirmed by reading). Severity bug (hang); size S.

## WO7. Prewarm runs before the planes are found, and prewarm deaths fire sub-emitters on frame 1: CONFIRMED (test), with a different symptom for planes
Test: `wo_checks.rs::wo7_*`.

**Sub-emitters:** 30 sub-system particles exist after frame 1, then 0 are made on frame 2. A steady parent
losing about 20 a second should make about 0.3 a frame. Confirmed as stated.

**Planes:** "particles live out their lifetime below the floor" is NOT what happens. On the first real step,
`collide` snaps every below-plane particle that is moving into the plane back onto the surface, so none ends
below. In the test, 40 particles end with 0 below and the lowest at y = 0.5, the radius. Instead, they are
reflected with the speed they gained falling unhindered through prewarm.
- With bounce 1, particles dropped from 1 m leave the floor at **19.1 m/s**. The physical most is 4.4 m/s.
- So the visible bug is a fountain of particles shooting up on frame 1, or (with dampen) a pile all lying on
  the floor at once.

The cause and the fix are as stated: `find_planes` before `play_from_start`, and clear `triggers` after
prewarm. The same stale-plane problem applies to `ParticleCommand::Play` called before the first step. Severity
bug; size S.

## WO8. Meshes swept each frame allocate every frame: CONFIRMED (test, sized: worse than claimed)
Test: `crates/fynkel_mesh/tests/wo8_alloc.rs`, using a counting global allocator. Sweeping a 200-sample, 8-point
capped outline into the same mesh costs, per call:
- **87 allocations and 800 KB allocated** (counting the growth reallocs of the Shape vectors). The reader
  estimated about 100 KB.
- 73 µs per call on this Mac.

`change` with recalculated tangents costs 2 allocations (75 KB). Severity slow; size M.

## WO9. Cap filling: slow, triangles outside the shape, and outside holes: CONFIRMED (test), and wider than claimed
Test: `crates/fynkel_mesh/tests/wo9_fill.rs`.

**(a) Time, release:**

| Outline | `fill` time |
|---|---|
| Circle, 500 points | 0.34 ms |
| Circle, 1000 | 1.2 ms |
| Circle, 2000 | 4.4 ms |
| Circle, 4000 | 19 ms |
| Circle, 10,000 | 120 ms |

This is quadratic, as claimed. A star (alternating radii) is the same, not cubic.

**(b) Self-touching outline:** two squares sharing a corner, walked as one loop, fill an area of 2.5 against a
true 2. The fallback filled outside the shape. The bow tie and a notch touching the wall came out right.

**(c) Holes outside:** the claim is only half right.
- A hole wholly to the *right* of the outline is dropped from the caps, and extrude still returns Ok with its
  walls.
- A hole wholly to the *left* is **joined**: the +X ray from it crosses the outline. The cap then has 6
  triangles outside the outline (area 2.33 for a unit square).

**NEW, the most important part:** valid shapes with 2 or more holes inside are filled wrongly.
- In 3000 random star-shaped outlines with non-overlapping round holes well inside, fills with 0-1 holes were
  always exact. 2-hole and 3-hole fills often weren't:
  - First run: 29/746 2-hole and 6/63 3-hole shapes wrong.
  - With a stricter generator: 3/249 and 2/3.
- A grid-coverage check on one 2-hole case found 1906 sample points covered twice and 166 points inside a hole
  covered.
- So caps of extrudes and sweeps with two or more holes (the letters B and 8, a plate with bolt holes) can have
  overlapping triangles (z-fighting, double blending) and filled-in holes.
- The cause is likely `join`'s bridge choice when a later hole's cut meets an earlier bridge. It wasn't traced
  further.

This raises WO9 to a bug of size M. The fix should validate holes and use a proven ear-clipper with hole
handling (earcut).

## WO10. Every view recomputes every world matrix every frame: CONFIRMED (reading + timing)
extract.rs:272 owns a `WorldMatrices` per view, and `update` loops over every object in trees with a hecs
lookup. Measured in `wo4_wo10_timing.rs`: 75 µs per update for 3000 objects in trees on this Mac, per view per
frame (plus one per raycast, WO4). The claim's numbers hold. It is a real but modest cost, so "optional" is
fair. Severity slow, low; size M.

## WO11. The trail clock is f32 and stops: CONFIRMED (test, timing refined)
Test: `crates/fynkel_world/src/trails/wo11.rs`. Clock movement over one second of steps, and points alive with
a 0.5 s life:

| Start | Rate | Clock moved in 1 s | Points alive (should be ~30 / ~60) |
|---|---|---|---|
| 2^18 s (3 days) | 60 Hz | 1.84 s (runs fast) | 17 |
| 2^18 s (3 days) | 120 Hz | 0 s (stopped) | 119 |
| 2^19 s (6 days) | 60 Hz | 0 s (stopped) | 59 |
| 2^22 s | either | 0 s | 59 / 119 |

At 2^19 s and later the points never expire, so the trail grows to the 512-point ring. That is about 3 days at
120 Hz or 6 days at 60 Hz, counted from the trail's creation. Severity bug, low; size S.

## WO12. A particle's start colour isn't checked finite: CONFIRMED (test)
Test: `wo_checks.rs::wo12_nan_start_colour_is_refused`. `ParticleSettings::refused()` returns None for a NaN
start colour. The FFI `settings()` (abi.rs:4585) doesn't check it either. Trails, by contrast, do check their
colours (trails.rs:72). Severity awkward; size S.

## WO13. Animator crossfades ignore playback speed: CONFIRMED (test)
Test: `crates/fynkel_world/tests/wo13_fade_speed.rs`.
- At speed 0 in the middle of a fade, slide's weight goes from 0.75 to 0 in one second. The fade finished
  while paused.
- Played backwards across 0 with reporting on, the loop isn't counted. The event also reads
  `from 0.5, to 1.5, loops 0`, which looks like a forward step, so event markers in 0.5-1.5 would fire wrongly.

Severity awkward; size S.

## WO14. A sweep's cross-section flips when up is parallel to forward: CONFIRMED (test)
Test: `wo8_alloc.rs::wo14_vertical_forward_right_flips`.
- Level along +X with up +Y, the right vector is (0,0,1).
- Straight up, glam's `any_orthonormal_vector` gives (0,0,-1): a **180°** flip, not just an arbitrary turn.

A road or rope with a vertical sample turns inside out there. Severity awkward (it is visible); size S.

---

## RN1. Outlines bloom like bright light: PARTLY (test)
Test: `crates/fynkel_render/tests/rn1_outline_bloom.rs`, on the real GPU. A black sphere with an outline is
drawn on black, Medium and High, without and with `Bloom::DEFAULT` (threshold 1).
- Width 0.2 m:
  - A white outline changes 2004 pixels, with a halo of (12,12,10) 4 px outside the ring.
  - A pure green outline changes 5292 pixels, halo (0,41,0).
  - Light grey 0.8 and mid grey change **0**.
- At width 0.08 m (about 1 px in this 96 px view), even white changed nothing.

So the claim holds for outlines with a channel at or near 1.0. That covers pure colours and white, the usual
outline colours: untonemap(1.0) is about 115. It is wrong for mid grey, since untonemap(x) is about x + 0.04
below 0.76 and the threshold is 1. The fix (alpha 0 in `fs_outline`) stands. Severity bug; size S.

## RN2. Turning, dimming or recolouring the sky rebuilds all its light every frame: CONFIRMED (test)
Test: `crates/fynkel_render/tests/rn2_sky_rebuild.rs`, a 2048×1024 image sky with a 128×128 view, release.
- Still: **0.79 ms** a frame.
- Rotation +0.001 rad a frame: **9.5 ms** a frame.

The pass count is as stated: radiance 6 + 7×6 mips + specular 6×6 + irradiance 6 = 90 passes, plus a new upload
of the image (sky.rs:27) and new cubes each time. The `PartialEq` on `SkyKind` includes intensity and rotation
(scene.rs:717-731). Severity slow (a day/night cycle or a turning sky costs about 9 ms a frame even on a Mac);
size M.

## RN3. The pipeline cache is rewritten after every pipeline: CONFIRMED (reading)
`PipelineMaker::make` calls `disk.save()` every time (assets.rs:130-138). `save` spawns a thread that writes all
of `get_data()` (assets.rs:111-122). Not testable here: the disk cache exists only when the device has
`PIPELINE_CACHE`, which is Vulkan only, so on Metal `disk` is None. The cost therefore applies to Android only:
one thread and one whole-file write per pipeline made, which is O(N²) bytes. Severity slow; size S.

## RN4. Transparent particles are sorted on the render thread each frame: CONFIRMED (reading + micro-benchmark)
Test: `crates/fynkel_render/tests/rn4_sort.rs` reproduces the `sort_unstable_by` with the dot product in the
comparator, on 10k 128-byte instances.
- **0.73 ms** a frame on this Mac.
- A keyed sort (depth, index) plus a gather takes 0.13 ms.

That is more than twice stage 12's measured sim step (0.33 ms on Note10+), so on device it likely dominates.
Severity slow; size S.

## RN5. Soft particles take the whole "what's behind" path: PARTLY (reading)
renderer.rs:1265 does set `self.behind` for soft particles, so the see-through pass split (resolve, depth copy,
restore) runs. The cost is real. The proposed fix is doubtful, though: "keep depth, skip the colour
resolve/restore".
- The particles are drawn in the same colour image after the split. With MSAA (Medium/High), the colour has to
  survive the pass break somehow: either resolve and restore (today), or store the 4× MSAA colour, which costs
  more bandwidth on a tile GPU.
- The `effect_depth` depth-only path that already exists works because effects run after the scene pass ends.

A real saving needs depth written alongside the opaque pass (a second attachment or a prepass), not a
flag split. No device measurement was made. Severity slow; size M or more.

## RN6. A particle module with its own shader isn't checked when the shader is made: CONFIRMED (test), and worse than stated
Test: `crates/fynkel_render/tests/rn6_particle_shader.rs`.
- A shader defining `fn fy_soft(x: f32)` passes `CustomShader::new`.
- Drawing particles with it fails at `create_shader_module` with "redefinition of `fy_soft`" (particles.wgsl:203).
- In the test, wgpu panics. In the app, fynkel_ffi/device.rs:54 routes uncaptured errors to `lose()`, which stops
  **every view on the device**.

The names at risk are particles.wgsl's `fy_*` and `Fy*` and the `override particle_lit`. Nothing reserves the
`fy_` prefix in shader.rs. Raise the severity to bug (low likelihood, but a crash); size S.

## RN7. Per-object data goes to the GPU four ways: CONFIRMED (reading), description accurate
- `Instance` is 148 B (32 floats + u32 + vec4).
- `Caster` is 96 B (mat4 + 2 vec4), one per map.
- Own-shader casters push Instances (renderer.rs:1506).
- `ParticleInstance` is 128 B.
- Dynamic meshes `queue.write_buffer` vertices and indices (assets.rs:400-403).
- Effects write their uniforms with `queue.write_buffer` (renderer.rs:2111).
- Bloom and AO params also use `write_buffer` (bloom.rs:246, ao.rs:322).

The Apple dirty-memory effect isn't measurable here. Severity awkward (seam); size M.

## RN8. renderer.rs is a 3.5k-line file whose `encode` records every pass inline: ACCURATE (rewrite claim)
- renderer.rs has 3516 lines. `prepare` starts at 934, `prepare_shadows` at 1381, and `encode` runs from 1527 to
  about 2157.
- Pipeline makers span 2360-3216, with 11 `cache: None` that `PipelineMaker::make` overrides.
- The flags threaded between `prepare` and `encode` (`behind`, `behind_kept`, `bloom`, `effect_depth`) are as
  described.

## RN9. Small leaks and stale state: PARTLY (reading)
- **Stale behind flags with no camera: NOT.** `prepare` sets `self.behind = false` at renderer.rs:963, *before*
  the no-camera early return at 970. `behind_kept` stays as last frame's, but that only keeps the images for a
  frame, as the idle-keep policy does anyway, and encode's `behind` is filtered by `self.behind` (1780).
- **Frame ring never shrinks: TRUE** (frame_ring.rs:253-256 only grows to `next_power_of_two`). A 100k-particle
  spike keeps about 16 MB × 3 slots.
- **Bloom chain kept after bloom is turned off: TRUE.** `image.bloom` is only reset when the scene image is
  recreated (renderer.rs:2199). It is a few MB at half resolution.
- **texture.rs bind-group caches: NOT.** `Textures::remove(key)` calls `forget_bind_groups(key)`
  (texture.rs:362-374), so entries are evicted when a texture is freed, which is the reader's own proposed fix.
  The cache is keyed by texture set, not material, so materials made and dropped don't pin anything once their
  textures go.

Severity awkward; size S.

---

## New bugs tripped over
1. **(Under WO9) `fill` with 2+ holes gives overlapping triangles and covers holes** for valid input. See WO9;
   it affects extrude and sweep caps such as letters with two holes. Bug, M.
2. **(Under WO9) A hole outside and to the left of the outline is joined in**, putting triangles outside the
   cap (fill area 2.33 for a unit square). Bug, S, fixed by the same hole validation.
3. **(Under WO7) Prewarmed particles below a plane are launched upward on frame 1** at the speed of their
   unhindered fall (19 m/s against a physical most of 4.4). Same fix as WO7.
4. **(Under WO13) A backward loop reports the wrong event range** (`from 0.5, to 1.5, loops 0`). Event markers
   fire for the wrong span.
5. **(Under RN6) The draw-time shader failure takes down every view on the device** (`lose()`), not just the
   particle system.
