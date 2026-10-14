# Hardening plan: solid before more

Status: accepted 2026-10-14. Scope and rules:
[ADR-0001](decisions/0001-fynkel-2-code-first.md). Stage 8 phases 1–3
are done; this plan runs **before** stage 8 phase 4 and before sound,
particles and animation. Nothing new is built until it is done.

Where it comes from: a full review of the code on 2026-10-14, at
`bea374bc`. Five readers (the Dart framework, the bridge and the view,
the renderer, physics, the examples and the guide), then four checkers
who went back over every claim that mattered, reading the code again and
running a throwaway test for each one they could. What's here survived
that. What didn't is listed at the end, so it isn't raised again.

**The verdict** was good: no memory-safety bug in the Rust, the frame
is allocation-free and tests hold it so, every C call is guarded, the
lifecycle holds up under destroy-during-iteration, pooling and
reparenting. What needs work is in four places:

1. **The view's Dart side** can freeze for good or leak the native view.
2. **Physics' transform sync** assumes the native body and the object
   agree whenever it computes an offset; three cases break that.
3. **Per-frame costs that scale with the scene instead of with what
   changed**: a light's setter, stopping a routine, a rigidbody spawn.
4. **Renderer structure** that each later phase makes harder to change:
   no vertex-attribute room, every pipeline compiled up front, shadow
   memory per view and never freed.

Then the papercuts a game hits (`normalized` throws, no `destroy(after:)`,
no `enabled` on a renderer), the build hook, and the guide.

## Rules

As in every stage:

- **A failing test first**, for every row that is a bug. The checkers'
  throwaway tests say what each one looks like; they're described under
  the rows.
- **Measured** if it is on the frame path, on the Mac at least, and on
  the Note10+ where the row says so.
- **The guide**, where a row changes what a game writes, and §21 "What
  changed" for anything that breaks code written before.
- **One PR per phase** or a few; shared-crate touches as their own
  commits.
- Sizes: S under a day, M a few days, L a week.

## Phase 1: the view and the bridge

Holes a stranger hits first. All small.

**V1. A render-thread failure freezes the view for good.** (S)
`packages/fynkel_flutter/lib/src/fynkel_view.dart:571` recovers from
`deviceLost` only and rethrows the rest. A panic on the render thread or
a failed resize (`crates/fynkel_ffi/src/worker.rs:150-159`, `361-365`)
makes every later `post` return `WorkerFailed`; `_tick` logs it once a
second and keeps the dead view: the last picture stays, the world runs,
no error widget.
*Fix*: treat `workerFailed` as `deviceLost` is treated: bounded retries
through `_lost` (a new `fy_view_*` starts a fresh worker), then `_fail`.
*Test*: a widget test with a stand-in view whose worker fails once;
expect a new frame drawn after.

**V2. Releasing a view can leak it.** (S)
`fynkel_view.dart:722`: `await registration; await unregister;
view.dispose()` with no `finally`. Both plugins throw "no such texture"
after `releaseOrphans` or engine detach; then `fy_view_free` never runs.
The views are made with `finalize: false` (`packages/fynkel/lib/src/view.dart:263`,
`:298`), so nothing else frees the `FyView`, its render thread, five
IOSurfaces (or the Android surface) and the world's GPU set.
*Fix*: `try { … } catch (e, s) { report } finally { view.dispose(); }`.
*Test*: the stand-in channel throws on `unregister`; expect
`fy_view_free` called.

**V3. `fy_quality_auto` is a leaf call that can create the GPU device.** (S)
`packages/fynkel/tool/ffigen.dart:14-25` lists the blocking calls;
`fy_quality_auto` (`abi.rs:959`) isn't there, yet `Device::current()`
can run `Gpu::headless()` for hundreds of milliseconds while the VM
can't reach a safepoint.
*Fix*: add it to `_blocking`, regenerate, commit the bindings.

**V4. On Android a transient resize or attach error kills the view.** (S)
`fynkel_view.dart:658-665` calls `_fail` on any `resize` error (permanent,
`ErrorWidget`); `FynkelFlutterPlugin.kt:59-62` only logs a failed
re-attach in `onSurfaceAvailable`, after which the view skips every
frame while Dart believes it is fine.
*Fix*: retry attach and resize once or twice before `_fail`; report the
Kotlin-side failure to Dart (a channel event) instead of logging it.
*Test*: the existing "a failed resize is reported" widget test, plus one
where the first resize fails and the second succeeds; expect frames.

**V5. A skipped scene is thrown away, and nothing wakes the worker when
the GPU frees a slot.** (M)
`worker.rs:393-416`: with two frames in flight `begin()` returns `None`,
the scene goes to `spare`, `skipped++`. The worker then sleeps on
`wake` (`:354`), which only `post`, `lose_device`, `fail` and drop
notify; `apple.rs:156-168` `finished` and the Android `finish` never
do. So a skip at 3 ms whose slot frees at 5 ms is drawn at the next
vsync post, 8.3 ms. Likely the uneven pacing seen at dense-2000/5000.
A scene skipped as the last post before the ticker mutes is lost.
*Fix*: on a failed `begin`, put the scene back as `pending` if none is
newer; have `finished` (Apple) and `finish` (Android) notify `wake`;
count `skipped` only when a newer scene replaces one.
*Test*: a worker test with a target whose first `begin` fails and then
completes a frame; expect `rendered == 1` without a second post (the
checker's scratch test is this, with the opposite expectation today).
*Measure*: the scale example at 2000 and 5000, frame-time histogram.

**V6. `fy_view_frame` drops the scene buffer on a failed post.** (S)
`abi.rs:2298-2306`: `run_shared` has advanced `uploaded` and the ledger
before `post`; on `Err` the scene is dropped, not returned to `spare`.
One allocation per failure, and only once per dead worker. Return it.

**V7. A test hook ships in the ABI.** (S)
`fy_view_lose_device` (`abi.rs:2411`) destroys the process-wide device
for every view. Gate it behind a `test-hooks` feature, or document it
as debug-only.

**V8. Two free entry points skip `guard`.** (S)
`fy_texture_free` (`abi.rs:1793`) and `fy_model_free` (`:1915`), unlike
`fy_mesh_free` and the others. Wrap them for consistency.

**V9. Resizing allocates five IOSurfaces per distinct size, and
drops the ready frames.** (M)
`apple.rs:220-237` makes `SLOTS` surfaces and textures synchronously on
the render thread for every new size and `ring.reset()`s; the Swift
`copyPixelBuffer` returns `nil` until a frame at the new size completes.
Dart sends one resize per frame during a window drag or split-view
animation (`view.dart:146-151` only dedupes equal sizes; `worker.rs:198`
only collapses resizes between two draws).
*Fix*: coalesce in Dart (apply once the layout is stable for a few
frames, or snap to a 16-pixel grid); keep the old slots while the new
ones are made so the picture never goes nil.
*Measure*: a macOS window drag, frames with no picture (should be 0).

**V10. Android: the platform thread waits behind the render thread's
GPU wait.** (M)
`android.rs` `finish` holds the lease across `device.poll(Wait, 1 s)`
and `present`; `handoff.rs:40-57` `put`/`take` (the Kotlin main thread's
attach and detach) wait for it. A slow GPU stalls the main thread for
the rest of the frame, up to the timeout. The wait itself is right and
measured (12.7 → 27 fps on the Note10+); the lease shouldn't span it.
*Fix*: end the lease before the poll (the frame's texture is already
acquired), or let `take` flag "detach pending" for the render thread to
honour at the end of its frame.

**V11. The process-wide device is lost on any uncaptured error.** (S)
`device.rs:52-55`. One view's validation error or out-of-memory rebuilds
every view, minimap included, up to three times a minute. Keep that for
real device loss; for other errors fail the view whose frame raised it
(the error scope already exists around Android `configure`).

**V12. Hot restart keeps the old isolate's views until the new one
builds a `FynkelView`.** (S, document)
`world.dart:52` frees worlds by finalizer; views (`finalize: false`)
live until `reset` → `releaseOrphans`. A restart into an app that never
shows a view leaks them for the engine's life. Send `reset` from
`FynkelApp`/`FynkelGame` at start too, not only from the first view.

*Done when* the widget tests for V1, V2 and V4 pass, V5's worker test
passes, the scale example's frame-time histogram at 5000 has no
double-length frames caused by skips, and the lab runs through a window
drag on the Mac without a blank frame.

## Phase 2: physics' transform sync

One rule fixes the three bugs: **derive a collider's offset from the
object hierarchy between it and its body, never from the native body's
pose, and write bodies back in hierarchy order.**

**P1. A collider added to a live rigidbody is placed against the body's
old pose.** (M) *Confirmed by test.*
`packages/fynkel_physics/lib/src/physics.dart:421` `_add` never calls
`_follow()` before `_addNative`; `crates/fynkel_physics/src/colliders.rs:269`
computes `relative = body.position().inverse() * pose` with the object's
new world pose and the body's stale one. Set `o.position = (10,0,0)`
then `addComponent(BoxCollider())` in the same frame: the collider ends
up at x = 20 (`overlapSphere`: `at0=0 at10=0 at20=1`). The same for
enabling a disabled collider, for a collider on a child, and for
kinematic bodies. `_addJoint` and `_rejoin` do follow first.
*Fix*: `_follow()` at the top of `_add`, `_removeBody` and before
`_reattach` outside `_followMoved`; longer term compute `relative` in
Dart from the transform chain (P3 needs that anyway).
*Test*: the scenario above, expecting `at10=1`.

**P2. A rigidbody under a rigidbody loses its own simulation.** (M)
*Confirmed by test.*
`rigidbody.dart:467` `if (place.chain() != place.stamp) return;` The
parent's write-back bumps the child's chain stamp, `_put` skips the
child, and the next `_follow` (`physics.dart:590`) teleports the child's
body back to its object: one step of travel, then reset, every frame
(`childBody.x` stays at 3.10 for a second while a lone body reaches
8.00). A `CharacterJoint` masks it because both move in lockstep; a
ragdoll with relative motion won't.
*Fix*: in `_applyMoved`, compute every body's pose first, then write
them top-down by hierarchy depth, deriving each local transform from
its parent's new world pose; `_put` refreshes the stamps of descendant
bodies as it already does for colliders.
*Test*: parent falling, child pushed sideways at 5 m/s; after 1 s the
child's body x matches a lone body's. Then the guide's ragdoll with a
limb that swings.

**P3. `rigidbody.position = p` takes the rotation from the body.** (S)
*Confirmed by test.* `rigidbody.dart:241` `_teleport(value, rotation)`
reads native state, so `o.rotation = q` set earlier in the frame is
overwritten with the old rotation.
*Fix*: use the object's current rotation when the transform is dirty.

**P4. A CharacterController inside a trigger gets no enter until it
moves.** (S) *Confirmed by test.*
`contacts.rs:454` sends trigger events for a body-less capsule only when
`followed.character` is set, and `characters.rs:133` sets it in
`move_character` only; `_reattach` resets it, ending the pair with a
spurious exit.
*Fix*: carry the character bit in the collider spec across the FFI.
*Test*: a controller spawned inside a trigger sphere; `entered == 1`
without a `move`.

**P5. `maxAngularVelocity` defaults to 50; Unity's is 7.** Wrong: see
"Raised and left".

**P6. A mesh collider at non-unit scale rebuilds its shape per instance
and per scale change.** (M)
`crates/fynkel_physics/src/meshes.rs:42-57` caches the unit shape only;
any other scale runs `make(scale)` (new points, new BVH) at add
(`colliders.rs:258`) and on each rescale (`:319-323`).
*Fix*: cache per `(mesh, scale)` with a small LRU, or Rapier's scaled
shape wrapper.
*Test*: 100 instances of one scaled mesh; shape builds == 1.

**P7. Per-spawn and per-remove scans.** (S)
`physics.dart:681` `_addBody` copies and walks every collider
(≈27 ns each: 0.05 ms per spawn at 2000 colliders, 0.25 ms at 10,000);
`_interpolated.remove/contains` (`:707`, `:723`), `_joints.remove`,
`_waiting.remove` (`:918`), and a full `_colliders` scan per
`Triggers`/`Collisions` component added (`:413`). Walk only the new
body's subtree; swap-remove with an index for the lists.

**P8. Queries and the controller see the stepped pose, not the drawn
one.** (S, document)
For interpolated bodies the native collider is up to one step ahead of
the lerped object; the guide (§14, "sees the world that's drawn") says
otherwise. Correct the guide, and note it for `moveWithGround` on
interpolated platforms.

**P9. A follow batch consumes its stamps before the native call.** (S)
`physics.dart:634` `seen()` runs before `fp_colliders_follow`, which
rejects the whole batch on one bad pose (`colliders.rs:309-312`). Not
reachable today (`Transform` refuses NaN), but cheap to harden: `seen()`
after success, and drop a collider whose `_reattach` failed instead of
leaving it with `_id == 0` in the lists.

**P10. `remove_body` leaks the listening count.** (S)
`crates/fynkel_physics/src/lib.rs:192-215` clears `followed` slots
without `listening_changed`. Dart never reaches it; fix for other callers.

*Done when* P1, P2 and P4's tests pass, the guide's ragdoll swings, and
the physics alloc tests still pass.

## Phase 3: the Dart frame

Costs that scale with the scene, and silent failures. All in
`packages/fynkel/lib/src/`.

**D1. Every Light setter scans every live object.** (S)
*Measured.* `components.dart:414-450`: `color`, `intensity`, `range`,
`angle`, `softness`, `shadows`, `kind` all reach `_writeLight`
(`game_object.dart:927`) → `_noteLights` (`world_objects.dart:479`),
which walks `_live` calling `get<Light>()` until a cap warning fires.
2000 objects, one lamp's intensity set each frame: 14 → 42 ms per 1000
frames, so the scan triples an otherwise empty frame. `print` at
`world_objects.dart:497` ships in release.
*Fix*: integer counts of suns and lamps kept on add, remove and `kind`
change; `_noteLights` only then; the warning through `Debug`/`_report`.
*Test*: the alloc/timing test above, and a count test.

**D2. Fixed steps per frame are unbounded, and the clamp comes before
the scale.** (S) *Confirmed by test.*
`time.dart:97` clamps `dt` to `maxDelta` (1/3 s), then `:103` scales it:
`time.scale = 5` with a 1 s hitch gives 1.667 s of game time and 83
fixed steps in one `update`. `world_loop.dart:262` has no cap, so a
step slower than `fixedDelta` never recovers.
*Fix*: cap steps per frame (`time.maxFixedSteps`, default 4–5) and drop
the rest of the accumulator; clamp after scaling.
*Test*: `update(1.0)` at scale 5 runs at most the cap.

**D3. Stopping a routine scans every pending wait.** (S)
`world_loop.dart:142-151`. Destroying 3000 waiting objects: 40–62 ms
against 4 ms for 3000 plain ones. Give each routine its own current
wait (or small list) and fail only those.

**D4. Pooled objects keep their signal listeners.** (S) *Confirmed by
test.* `pool.dart:99-115` stops routines and motions only;
`_stopListening` runs on destroy alone. `on()` in `onEnable`, as the
guide's pooling section suggests, adds a handler per reuse: after five
cycles one `emit` calls the handler six times, and a released, hidden
object still hears.
*Fix*: an enable-scoped `on` (cancelled on disable and on release), or
release cancels what was subscribed while enabled; say which in §3.

**D5. A routine awaited in its own `onLoad` never finishes.** (S)
*Confirmed by test.* `routines.dart:129` pauses routines whose owner
`_holdsStill`, which is `!_enabled` (`game_object.dart:144`), and the
object is enabled only after load (`world_objects.dart:315`). After 120
frames: `loadDone=false`, the object hidden, nothing reported.
*Fix*: a routine started during the owner's own load isn't paused by
that load (or throw a clear error when `onLoad` awaits one).

**D6. Two signal emits run unguarded inside the frame.** (S) *Confirmed
by test.* `cameras.dart:533` `onLiveChanged.emit` and `world.dart:302`
`onPausedChanged.emit`: a throwing handler escapes `World.update` and
skips the debug flush and the systems' `afterFrame`. Wrap like
`Input._emit`.

**D7. `wasPressed` in `onFixedUpdate` fires twice or never.** (S)
*Confirmed by test.* Edges clear once per frame (`input.dart:356`); a
frame with two fixed steps sees the press twice, one with none never.
Latch edges per fixed step, or document and assert in debug.

**D8. Input times are the frame's clamped clock.** (M)
`world.dart:467` stamps down, up and move with
`time._unscaledSinceStart`, built from `dt` clamped to 1/3 s. A finger
down, a 1 s hitch (a first-use upload), and the up in the same batch: a
1 s hold reads as a quick tap; swipe speed overstated up to 3×.
*Fix*: carry Flutter's `PointerEvent.timeStamp` through `_Raw` and use
it for `_downAt`, `_upAt` and the velocity samples.
*Test*: a simulated down, `update(1.0)`, up; expect no tap.

**D9. A mouse leave during a drag is dropped.** (S)
`input.dart:437-439` ignores `leave` while down; `_up` keeps mouse
fingers (`:563-567`), so `mousePosition` stays stale until the cursor
comes back. Remember the leave and apply it on up.

**D10. `getInChildren` allocates a list per node.** (S)
`game_object.dart:754` recurses through `children`, which is
`List.unmodifiable` (`:499`). Iterate `_children` by index as `childAt`
does.

**D11. Hover goes stale under a still pointer, and each moved finger
raycasts even with no handlers.** (S)
`input.dart:540-554`, `pointing.dart:90`, `:157`. An enemy walking out
from under the cursor never gets `onPointerExit`. Re-evaluate hover when
the transform or shape version changed; skip the raycast when no live
object overrides a pointer handler.

**D12. `Signal.next` outside a routine keeps its listener forever.** (S)
`signals.dart:90-100`. Return a cancelable, or assert in debug.

**D13. Small tidy-ups.** (S)
`World.dispose` never closes `_errors` (`world.dart:94-167`); keys held
across a pause are lost until the OS repeat (`input.dart:361-371`:
resync held keys on unpause); `GameObject.find` and `name` are an FFI
call per use (`game_object.dart:567-603`: document, or cache the name).

**D14. Per-set allocations in component setters.** (S)
`components.dart:310-334`, `452-469`, `534-548` build a closure and a
record per set for rollback; `Material._writeMaterial`
(`world_resources.dart:161`) maps to a list. A tween of
`Light.intensity` or `renderer.color` allocates every frame. Validate
in Dart first and drop the rollback closure.

*Done when* the alloc tests pass with a tweened light and a tweened
colour, and D1–D7's tests pass.

## Phase 4: the renderer, before stage 8 phase 4

Each later phase makes these harder. Do them before the sky.

**R1. The instance layout has no room left.** (M)
`forward.wgsl:96-123`, `renderer.rs:973-980`: locations 0–15, exactly
wgpu's default `max_vertex_attributes`, 172 bytes an instance. Skinning
(stage 9) needs joints and weights.
*Fix* (frees four): drop `normal_0..2` and build the normal matrix in
the vertex shader as the cofactor matrix of the model's upper 3×3
(`mat3(cross(m1,m2), cross(m2,m0), cross(m0,m1))`, direction only, the
fragment normalises; `mirrored` from the determinant's sign instead of
`params.w`); send the affine model as three transposed rows. Result 12
attributes, 120 bytes.
*Test*: every golden unchanged at every level; bench prepare time.

**R2. About 21 pipelines compiled up front, most unused, no cache.** (M)
First frame: Low 14, Medium 21, High 21 (6 kit + 8 for the view's
format + 7 for `Rgba16Float`); above Low the view-format forward set is
dead, and of the six forward variants a game uses one or two.
`cache: None` at every site (`renderer.rs:1031`, `1078`, `1112`,
`texture.rs:61`, `shadows.rs:353`).
*Fix*: compile forward variants lazily on first use, keyed by (cutout,
faces, format); build the view-format set only when Low is used; a
`wgpu::PipelineCache` on Vulkan, saved under the app's cache dir.
*Measure*: time to first frame on the Note10+ and the iPhone, before
and after.

**R3. Shadow memory is per view, allocated at the level's full size,
never freed.** (M)
Low 3 MiB, Medium 24 MiB, High 40 MiB per `Renderer` (`shadows.rs:201`,
`204`, `scene.rs:225-248`); spot layers at the level's count
(`renderer.rs:684`) however many spots cast; `renderer.rs:730-734` keeps
what a frame doesn't use. A minimap doubles it; turning shadows off
frees nothing.
*Fix*: one `ShadowMaps` per kit and level, shared by the world's views;
spot layers on demand; dropped after N frames unused.
*Test*: `FrameStats` reports shadow bytes; a test with two views and
shadows off expects zero after N frames.

**R4. Unknown Android GPUs get High.** (S)
`scene.rs:195-207`: only ARM and Qualcomm get Medium; PowerVR (0x1010),
Samsung Xclipse (0x1002), Tegra fall through to High with three
cascades and 8 lights per object, untested.
*Fix*: on Android, Medium unless the vendor is known good; `Other` and
`VirtualGpu` Low. A test per vendor.

**R5. Phase 3 has no picture test.** (S)
`tests/shadows.rs` reads one pixel; the shimmer test is matrix maths
without a GPU (`src/shadows.rs:490`); the golden tolerance
(`fynkel_golden/src/lib.rs:78-87`, 0.1 % of pixels) is about the size
of a contact edge at 128².
*Fix*: goldens for a cube on a plane (acne, peter-panning), a cut-out
caster, the cascade seam and a spot's cone edge; a GPU shimmer test
(ten camera positions, the shadow edge's world position fixed); a
per-test tolerance.

**R6. Cascades switch with a hard cut.** (S)
`forward.wgsl:282-286`. Blend over the last tenth of each cascade (a
second lookup only in the band). At Low one 1024² map over 40 m is
about 6 cm a texel; consider a shorter Low distance.

**R7. A spot wider than 152° has an unshadowed outer ring.** (S)
`shadows.rs:190` clamps the map's fov at 160° while `Light.spot` allows
up to 180°; outside the map `filtered()` returns lit. Refuse shadows
past ~150° in Dart, with a message, and say so in §5.

**R8. Uploads and encoding hold one lock across GPU work.** (M)
`renderer.rs:410-412` holds the assets guard through texture creation,
write, mip passes and submit (`texture.rs:242-262`); `encode` holds it
across all three passes (`:743` to the end). A level's textures
streaming in stall every view. Do the GPU work outside the lock, insert
the finished texture under it; an upload budget per frame.

**R9. Targets are rebuilt on a one-pixel change.** (S)
`renderer.rs:715-721`. Round sizes up to 16 and render to a
sub-rectangle, or allocate with slack. Pairs with V9.

**R10. The cascade's depth range follows the casters.** (S)
`shadows.rs:130` pulls `near` toward the sun by every caster's radius,
so the constant bias of 2 (`:325`, `Depth16Unorm`) changes its world
size as a big caster enters or leaves. Clamp the stretch to a multiple
of the cascade radius (depth-clamp the rest), or compute the bias in
world units from `texels`.

**R11. The shader pays for shadows and textures it doesn't use.** (M,
measure first)
`forward.wgsl:277` copied the 528-byte `Shadows` struct per fragment
(fixed in #103: it also lost the Android emulator's device); `shade()`
samples five textures for every material (`:325-329`); the
only override constant is `double_sided`. The Note10+ at medium is at
55 with shadows, 58 without, 60 needed.
*Fix*: override constants for "no shadows" and "untextured" (R2's lazy pipelines make the variants cheap).
*Measure*: the lamps and shadows scenes on the Note10+ at medium, with
and without. If medium reaches 60, phase 6's open item closes here.

**R12. Cut-out mipmaps thin out.** (S)
`mipmap.wgsl` box-filters alpha; foliage fades at distance. Rescale mip
alpha to keep coverage at the cutoff for cut-out materials.

**R13. glTF limits a game will hit.** (M)
`crates/fynkel_load/src/model.rs:21` supports two extensions;
`KHR_mesh_quantization` (gltfpack's default) is refused; clamp and
mirror wrap are warnings (`:610`) against one repeat sampler
(`texture.rs:167`), so an edge-authored texture bleeds. Support
quantization (the reader exposes normalised integers) and three
samplers keyed in the bind group.

*Done when* every golden is unchanged after R1 and R2, R3's two-view
test passes, R5's goldens exist, and the Note10+ numbers for R11 are in
stage 8's table.

## Phase 5: what a game writes

Papercuts from the examples and the guide. Each is small; together they
are why the examples copy code.

**G1. `normalized` throws on a zero vector.** (S) *Confirmed by test.*
`math.dart:136`, and with it `Quat.axisAngle`, `Quat.lookRotation`,
`Ray`, `Plane` and `lookAt` of one's own position. The chase idiom
`(target - position).normalized * speed` throws the first frame two
objects overlap, reported every frame. Unity returns zero.
*Fix*: `normalizedOrZero` (and `lookAt` ignoring a zero direction);
keep the throwing one and say why in §7.

**G2. `destroy()` has no delay.** (S)
`game_object.dart:416`. `destroy(after: 2)` for bullets and sparks,
instead of a routine each.

**G3. `MeshRenderer`, `Light` and `Camera` have no `enabled`.** (S)
`components.dart:494`, `:364`. The workarounds (`mesh = null`, or
`active = false` on the whole object) lose state or stop behaviours.

**G4. `release()` twice throws; `destroy()` twice doesn't.** (S)
*Confirmed by test.* `pool.dart:101`. The guide says to write `release()`
where you'd write `destroy()`; a shot hitting two enemies in one step
throws. Make `GameObject.release()` a no-op when already released.

**G5. Missing helpers the examples rewrite.** (S)
`Vec3.withY`, a `flat` (`Vec3(v.x, 0, v.z)` appears in arena ×4, coins,
sheepdog), `Vec3.all(s)` for uniform scale (spelled three ways),
`Transform.lookAt(eye, target)`, `Light.sun(direction:)` (the same
three-line sun block in six examples), `Signal<void>.emit()` without
`null`, `Finger.position` as a `Vec2` (bowling has an adapter).

**G6. `add()` returns the object when you want the component.** (S)
`add(GameObject(...)).get<Light>()!` in sheepdog and the guide §13;
three idioms for keeping a typed behaviour field. Add
`addWith<T extends Component>(T component, {…})` returning the
component, or a `GameObject.of(component)` factory; pick one and use it
in the examples.

**G7. Code six examples copy.** (M)
`Controls` (45 lines, arena `:35-78` and sheepdog `game.dart:80-123`,
identical but a key name), `_stick()` drag-as-joystick (arena `:324`,
sheepdog `:473`), the portrait fit formula (arena `:511`, sheepdog
`:881`), `Health` (four copies), `measured.dart` (seven copies, and
`fynkel_measure` in every example's pubspec). Bring the first three
into the framework (`input.stick`, `Controls` with persistence, a
`Camera.fitPortrait`/horizontal fov), a `Health` in the guide once, and
take `fynkel_measure` out of the examples' pubspecs (a `--dart-define`
or a separate measured entry point).

**G8. Test helpers.** (S)
Each example's tests rewrite `run(seconds)`, `tap`, a hand-built
`ViewPoint`, and about 40 lines of PNG and CRC code though
`HeadlessView.png()` exists (`view.dart:228`). A
`package:fynkel/testing.dart` with them; delete the copies.

*Done when* the examples are shorter than before, the guide uses the
new helpers, and §21 lists what changed.

## Phase 6: the build hook and the guide

**B1. The hook is one machine's.** (M)
`packages/fynkel/lib/hooks.dart`: Android's linker is
`toolchains/llvm/prebuilt/darwin-x86_64` (`:124`) and the NDK is the
last name in string order (`:113`, `"9.x"` above `"25.x"`, `.DS_Store`
counts); API 30 is hard-coded in four places; no deployment target is
passed to cargo; `cargo build` runs without `--locked` (`:47`), so an app
build can rewrite `Cargo.lock` and needs crates.io; `Process.run`
buffers, so a clean build prints nothing for a minute.
*Fix*: take the compiler from `input.config.code.cCompiler` and the NDK
API from the config; `--locked`; `MACOSX_DEPLOYMENT_TARGET` and the iOS
one; stream the output. Prebuilt artifacts, pub and Linux hosts stay in
stage 10 as planned, but this makes the hook honest until then. Say in
the README that rustup 1.28.1+ installs the pinned toolchain itself and
how to do it by hand.

**B2. The guide.** (S)
§19 still says shadows aren't there and omits particles, audio, sprites,
3D text, navmesh, saving and scene management; line 468 says "to come a
sky"; the README's table lacks Sheepdog; §17 says `package:fynkel` has
no Flutter yet its own test imports `flutter_test` and the games import
`foundation.dart` (say: `flutter test`, and why); §12 doesn't say
Flutter's asset folders aren't recursive (coins lists two); `fynkel_physics/example`
is a benchmark, not a lesson. Add a scene/level and a save-game recipe
to §15 or §16, and a Blender-glb-as-level note to §12.

## The order

| # | Phase | Why here |
|---|---|---|
| 1 | The view and the bridge (V1–V12) | A stranger hits these first; each is small; V5 may explain the pacing |
| 2 | Physics' transform sync (P1–P10) | Wrong results today for ragdolls and for any collider enabled after a teleport |
| 3 | The Dart frame (D1–D14) | Silent cliffs and silent failures in the core every game runs on |
| 4 | The renderer (R1–R13) | Must land before the sky, see-through and bloom pile onto the same shader and layout |
| 5 | What a game writes (G1–G8) | Shrinks the examples and the next game; needs 3 done for `enabled` and `destroy(after:)` |
| 6 | The hook and the guide (B1–B2) | Last, so the guide describes what the phases above left |

Then stage 8 phase 4. Sound and particles come after stage 8 as
planned; neither needs anything here changed again.

## Checklist

- [x] Phase 1: the view and the bridge (V1–V12; every bug row's test
  failed first). Measured on the Mac, 30 s each, the scale example: 2000
  objects in the window and 5000 at full screen (3456x2168) both hold
  120 fresh fps with 0 coalesced, skipped or dropped, as main does; the
  skip path V5 fixes no longer runs on the Mac, so the gain is for slower
  GPUs. V9's window drag and the iPhone are left to the user's eyes.
- [ ] Phase 2: physics' transform sync
- [ ] Phase 3: the Dart frame
- [ ] Phase 4: the renderer
- [ ] Phase 5: what a game writes
- [ ] Phase 6: the hook and the guide
- [ ] Stage 8 phase 4 resumes

## Raised and left

So they aren't raised again.

- **Android's wait for the GPU after submit** (`android.rs:289`) looked
  like a serialisation bug. It is deliberate and measured (12.7 → 27 fps
  at 10,000 objects); only the lease across it is wrong (V10). Whether
  CPU work hiding behind it explains the Note10+'s medium gap is for
  R11's measurement to say.
- **`maxAngularVelocity` should default to 7** (P5): 7 was Unity 5.6's
  and older. Current Unity (6 included, and 2020.1 already) defaults to 50
  (`Physics.defaultMaxAngularSpeed`), as Fynkel does.
- **A NaN pose dropping a whole follow batch**: unreachable, `Transform`
  refuses NaN. Hardened anyway in P9 because it is cheap.
- **Bodies sleeping 5× later than Unity's**: wrong. Rapier's default is
  0.5 s; measured 0.52 s at rest.
- **rustup not installing the pinned toolchain**: true of 1.28.0 only,
  restored in 1.28.1. One line in the README (B1).
- **Every spawn O(all colliders)**: real but 0.05 ms at 2000 colliders.
  Kept as P7, low.
- **Cross-platform physics determinism**: Rapier without
  `enhanced-determinism`; not a goal today. Say so in §14 when a game
  asks for replays.
- **Opaque draws sorted by material, not depth; no depth pre-pass;
  fixed 4× MSAA at every level; static shadow caching; specular
  anti-aliasing; texture compression**: real costs on Mali, none a bug.
  Phase 6 of stage 8 (which has to fit the Note10+) decides them with
  numbers, after R11.
- **Sound, particles, animation, prebuilt binaries, Linux hosts, an
  editor**: the stages after this plan, in their planned order.
