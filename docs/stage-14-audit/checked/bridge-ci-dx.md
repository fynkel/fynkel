# Checked: bridge (BR), guide/tests/CI (GT), developer experience (GD)

Checker: bridge-ci-dx, 2026-10-26, on stage-14 at 44ea4697 (worktree; only docs differ from
main 2f72cbd5). Throwaway tests are in /tmp/s14/tests/bridge-ci-dx/, each with its repo path
at the top.

---

## BR — bridge

### BR1 — view size never checked against the texture limit: CONFIRMED (test), bug, S
`crates/fynkel_ffi/tests/check_br1_new.rs`: a 64×64 view draws, then
`fy_view_new_headless(world, 9000, 100)` returns **Ok**. The *unrelated* first view's next
`fy_view_frame` returns **DeviceLost**: "In Device::create_texture, label = 'headless target'
Dimension X value 9000 exceeds the limit of 8192". So creation goes through the uncaptured
handler and loses the device for every view in the process, as the reader said.
`check_br1_resize.rs`: `fy_view_resize(a, 9000, 100)` returns **Ok**. View a then fails with
WorkerFailed (the same message), while view b keeps drawing. The resize path stays
per-view, as claimed.

Corrections:
- renderScale isn't applied to "inner targets". It is folded into the size Dart sends
  (`fynkel_view.dart:738–740`: logical × ratio × renderScale, clamped to 16384).
- A realistic trigger is a 5K/6K Mac display at renderScale 2, or resolutionScale above 1.
- The limit is 8192 because of `or_worse_values_from` (`gpu.rs:70`).

The Apple creation path (`make_slots`) wasn't run, but it creates its textures on the same
thread with no error scope.

### BR2 — off-render-thread GPU work escapes the error scopes: CONFIRMED (reading), latent bug, M
- `renderer.rs:2291` spawns the "fynkel shader" thread, which runs `make()`
  (create_shader_module/create_render_pipeline) with no `push_error_scope`.
- `scoped()` (`worker.rs:570`) wraps only the render thread, and wgpu 30's error scopes are
  per thread.
- An error there therefore reaches `device.rs:50` and `lose()`.

Why there is no test: headless views set `set_wait_for_shaders(true)` (`abi.rs:5193`), so in
any test reachable through the ABI the compile runs inside the render thread's scope. The
async path is only taken by on-screen views. I also know of no WGSL that passes
`CustomShader::new`'s naga validation (with MAX_TEXTURES = 4, `shader.rs:27`) and then fails
pipeline creation.

The target-creation half is BR1 and is shown failing by check_br1_new.rs. Keep it as a
robustness item. The "loss repeats until _maxLosses" chain is plausible but unshown.

### BR3 — stale blocking list: CONFIRMED (test), and WIDER than reported, bug, S
`check_br3_times.rs` (release, M-series Mac) times leaf calls (all `isLeaf: true` in
bindings.g.dart):

| Call | Time on the Mac |
|---|---|
| `fy_sky_image_decode` (lab sky.hdr, 1.4 MB) | 8.3 ms |
| `fy_shader_new` (dissolve.wgsl) | 4.2 ms |
| `fy_mesh_sweep` / `fy_mesh_set_sweep` (2000-point path × 16-point outline) | 1.7 / 1.8 ms |
| `fy_mesh_recalculate` | 0.9 ms |
| `fy_particles_command(PLAY)` on a prewarmed 10k / 5 s system | 3.1 ms |
| `fy_particles_simulate(10 s)` on that system | 10.7 ms (unbounded: 30 steps a second, `particles.rs:1784`) |
| `fy_mesh_new_grid(…, 1024, 1024)` | 35 ms |
| `fy_mesh_new_sphere(1, 1024, 1024)` | 49 ms |
| `fy_mesh_set_positions` with a new vertex count (re-derives normals and tangents, `abi.rs:2654–2658`) | 31 ms |

The test fails on its 5 ms bar. A Note10+ is several times slower.

**Exact missing list:**
- **Clearly long:**
  - fy_sky_image_decode
  - fy_particles_simulate
  - fy_mesh_new_grid / _box / _sphere / _capsule / _cylinder / _cone / _torus / _disc (MAX_SEGMENTS 1024)
  - fy_mesh_set_positions (count change)
  - fy_shader_new
  - fy_mesh_extrude / _lathe / _combine / _sweep / _set_sweep / _recalculate (linear in size)
  - fy_shaders_waiting (`Device::current` can create a device, the same reason fy_quality_auto is listed)
- **Can be long once:** fy_particles_command(PLAY) and fy_world_particles_step (prewarm runs `duration × 30` steps in `play_from_start`, `particles.rs:1052`).
- **Per-frame and arguably fine:** fy_world_animate, fy_world_particles_step, fy_view_frame (extract).

Not long: the getters, the component setters, and fy_mesh_add/fy_texture_add (they move handles).

The reader's GC reasoning holds: `inHelper`/`Isolate.run` share the isolate group.

### BR4 — Lease::drop wakes the taker before its Arc drops: CONFIRMED (test), bug (low impact), S
The test (`handoff_check_br4.rs`, appended to `crates/fynkel_present/src/handoff.rs`) fails
with "**107 of 100000** takes still shared the value with a lease".

`Drop::drop` (`handoff.rs:97–102`) decrements the count and notifies. The `value: Arc<T>`
field only drops after that. In `layer.rs:173`, `take().and_then(Arc::into_inner)` then
yields None. The result:
- attach makes a second wgpu surface for the same CAMetalLayer.
- The old `Live` drops on the render thread.

`detach` has the same window. The only user is the iOS layer presenter.

### BR5 — iOS layer lease held across nextDrawable and the whole frame: CONFIRMED (reading), slow, S–M
- `layer.rs:119–137`: `begin()` leases, calls `get_current_texture` with no `wanted()` check,
  and keeps the lease in `LayerFrame._layer` until `finish()` presents.
- `wanted()` is `cfg`'d for Android only (`handoff.rs:80`).
- attach/detach run on the main thread from the Swift plugin, waiting in `Handoff::take`.

No test: `Layer` is hard-wired to a real `wgpu::Surface` over a CAMetalLayer, and there is no
fake-surface seam. The 1 s figure is CAMetalLayer's default nextDrawable timeout. It is
plausible but wasn't measured.

### BR6 — extract rescans meshes/materials/textures every frame, per view: PARTLY, slow (minor), S
The loops are as described (`extract.rs:189–270`). They run on the Dart thread inside
fy_view_frame, once per view, and the comment at 241 accepts the cost on purpose.

Corrections:
- The cost is unmeasured, and "tenths of a ms with hundreds of materials" looks high: it is
  about 1–2k hash operations.
- Per-object extraction dominates any real scene.

Worth doing only together with a "share one extract per world per frame" change.

### BR7 — world widgets allocate per widget per frame: CONFIRMED (reading), slow, S
The per-widget allocations are more than the claimed 4–6:
- `w.point` = `gameObject.worldTransform.position + offset`, where worldTransform builds a
  Transform with a Vec3 and a Quat (`world_widget.dart:46`).
- `- shake`, the ViewPoint, `point - eye` and the `Offset`s, plus a Matrix4 when scaled.

"Against the budget" is wrong: no alloc-tagged test exists in fynkel_flutter (the alloc tests
are in fynkel and fynkel_audio). So nothing fails today, and a new gate would be needed to
hold it.

### BR8 — ABI seams: PARTLY, awkward, S
- **Wrong:** "`*const f32` means optional in fy_animator_speed_set/stop". Both use `read()`,
  which rejects null (`abi.rs:778–781, 3950, 3992`). Every `*const f32` scalar is required.
  Optional pointers are only structs (shake offset, quality, sky, fog).
- **Wrong:** "a Vec per post". `Vec::new()` doesn't allocate.
- **True:** the pointer convention is deliberate and documented ("a double passed to a call
  is boxed, each frame, by Dart's JIT", `abi.rs:4105`). That makes `fy_world_time_set`
  (f32s by value, called every frame from `world.dart:754`) the real outlier against the
  ABI's own rule.
- **True:** the physics side uses u32 flags (`is_trigger: u32`, `fynkel_physics_ffi/src/lib.rs:136`).
- **True:** `ops` is always empty in production (`abi.rs:5388`).

### BR9 — no per-view error hook, and failure is permanent: PARTLY, awkward, S
- **Permanent:** confirmed. `_error` is only ever set (`fynkel_view.dart:961`) and never
  cleared, so only a new State (a new key) retries.
- **"Game code can't see"** is too strong: `_fail` and `_lost` call
  `FlutterError.reportError`, so a game's `FlutterError.onError` or crash reporter does see
  it. What is missing is a per-view callback or errorBuilder.

### BR10 — split abi.rs: description PARTLY accurate (rewrite)
- **Accurate:** abi.rs is 5730 lines, and the section ranges are about right.
- **Inaccurate:** "types sit in the shared 660-line header far from their functions" is true
  only of the early stages. Only 11 `pub struct/enum Fy*` are above line 660. The other 30
  (particles, trails, sweep, …) already sit beside their functions; lines 37–660 are mostly
  plain types plus Conversions.
- The cbindgen and fingerprint guard idea is sound.

### BR11 — FynkelViewState interleaves four host protocols: description accurate; motivation overstated (rewrite)
- **Accurate:** `_open`, `_register`, `_sendSize`, `_sendPlace` and `_release` juggle the
  Apple texture, the iOS layer, the Android producer and the Android SurfaceView with
  `_surfaced`, `_noLayer`, `_sentWidth`/`_sentHeight`, `_onScreen`, `_sentPlace` and
  `_sentOver`, as described.
- **Overstated:** "Most V-items were in these transitions". Of V1–V12 (docs/hardening.md:51–152),
  V1, V2, V4 and V12 touch Dart view state. V3, V5–V11 are Rust/ABI/Kotlin-side.

---

## GT — guide, tests, CI

### GT1 — no performance number checked automatically: CONFIRMED (reading), gap, M
- ci.yml has no bench step. The `#[ignore]` measurements are listed as claimed: fynkel_mesh
  made.rs:334, sweep.rs:219 and mesh.rs:529; fynkel_physics meshes.rs:239; and also
  fynkel_ffi android.rs ×3 and fynkel_present android.rs.
- `scale_test.dart` only prints its median.
- No Rust test asserts an absolute time. A 2× slowdown passes.

### GT2 — the soak is flaky: CONFIRMED (CI history), bug in the test, S
`gh run list` plus `--log-failed` shows exactly three soak failures:

| Run | Branch | Failure |
|---|---|---|
| 39684829147 | stage-11, 10-21 | malloc 20,925 vs 20,000 |
| 39768497043 | stage-11-mix, 10-22 | footprint 296,141 vs 250,000 |
| 40025387249 | **main**, 10-23 21:15, after 15b6d004 | footprint 1,206,272 vs 250,000 |

The other red runs are different steps: the mobile step (39254044069), fynkel_ffi alloc and
particles on stage-12 PR branches (WIP), and fmt/clippy.

Correct that 15b6d004 and a203294e changed only the malloc comparison. The footprint check
(`soak_test.dart:117`) is unchanged.

The reader's "can't tell a leak from noise" undersells the run's own data. In 40025387249:
- malloc sat at 16.8–18.0 MB for all 120 rounds.
- The Dart heap was 78.0 MB at both samples.
- Only the process floor stepped, from about 206 to about 260 to about 284 MB, in jumps
  rather than a slope.

That points to page/VM noise. The soak has no GPU, and Rust uses the system malloc, so engine
growth would show in malloc. Since then, 8 runs are green (4 on main). The fix idea (count
engine-owned bytes; print footprint as a trend) stands.

### GT3 — the soak never draws: PARTLY, gap, S
The soak has no HeadlessView (`soak_test.dart:43`): confirmed.

"gpu_memory/texture_memory check steady frames only" is half wrong.
`texture_memory.rs:44 removed_textures_give_their_gpu_memory_back` does check resources
coming and going. Nothing cycles worlds, views and scenes with a GPU over rounds.

### GT4 — phone platform code never compiled in CI: CONFIRMED (reading), gap, S
- `tools/test-mobile.sh:23` cross-builds only fynkel_present, fynkel_render and fynkel_ffi
  for aarch64-apple-ios-sim and aarch64-linux-android.
- The macOS builds compile only the AppKit Swift branches.
- Nothing builds the following:
  - the Kotlin (FynkelFlutterPlugin.kt, FynkelActivity.kt)
  - iOS-only Swift
  - fynkel_audio/physics_ffi for mobile triples
  - `session.rs`'s iOS/Android cfgs
  - the hook's aarch64-apple-ios device triple

### GT5 — Pocket Hole outside CI, on FlutterActivity: PARTLY, gap, S
- **Confirmed:** it isn't in ci.yml, it uses `class MainActivity : FlutterActivity()` (all 8
  other examples use FynkelActivity), and the lab's format line skips `tool/` (measured.dart,
  music.py).
- **Checked here:** at stage-14 it still `flutter analyze --fatal-infos` clean, and its tests
  pass. There are **5** tests, not 4. So nothing is broken today; it is only unguarded.

### GT7 — no per-system frame timings; guide points at a "Profiler": CONFIRMED (reading), gap, M
- `world.dart:364/372` (particleStepTime, meshChangeTime) are the only per-system timings.
- The guide's "want a look at the Profiler" (`writing-a-game.md:1442`) names nothing that
  exists in Fynkel. Unity readers will take it as Unity's.
- The lab re-implements timers in crowd, particles_*, meshes_change, meshes_build and splines.

### GT8 — guide code compiled, little run: CONFIRMED (reading), with a correction, S–M
`check_guide.py` analyzes everything and runs the 4 `test/` blocks (bullets, walker, errors,
app).

Correction: not "all blocks share one file". 30 blocks have `// lib/x.dart` path headers and
become their own files; the other 36 are concatenated into lib/game.dart. The point stands:
Waves, Fountain, Dog and the rest never execute.

### GT9 — systems tested one by one; sound in no game: CONFIRMED (reading), gap, M
fynkel_audio is only in examples/lab/pubspec.yaml. Physics is in coins, bowling, lab and
pocket_hole.

### GT10 — fynkel_measure almost untested: CONFIRMED (reading), gap, S
`test/` holds only on_screen_test.dart, against lib plus bin of about 1850 lines.

### GT11 — widget/golden helpers copied: CONFIRMED (reading), awkward, S
`Future<void> wait(WidgetTester …)` is in **15** example test files (more than the 12
claimed). `examples/lab/test/golden.dart` says it copies packages/fynkel/test/golden.dart.

### GT12 — one serial job, main runs cancellable: CONFIRMED (reading + CI), awkward, S
ci.yml:14–16 sets `cancel-in-progress: true` for every ref, and everything runs in one job,
`rust`. Cancelled main pushes: 39939157776 and 39940814497 (10-23), and 39170522260 (10-18).

### GT minor items
- **"§21 What changed":** already fixed in docs/stage-14.md at the stage-14 tip (44ea4697 →
  §23). docs/hardening.md:41 still says §21; that file is historical.
- **The guide's example list omits scale and pocket_hole:** confirmed.
- **examples/hello has no test/:** confirmed.

---

## GD — developer-log findings (courtyard F1–F8, marble 1–11; duplicates merged)

### GD1 — build-hook race when two apps build at once (courtyard F1, marble 5): CONFIRMED (test), confusing / near-blocker, S
- `hooks.dart:84–86` copies `target/<triple>/release/<dylib>` after cargo returns, outside
  cargo's lock.
- I found that a **fresh** cargo build (`Finished in 0.08s`) still re-uplifts the file: its
  inode changed between two no-op runs. Any concurrent cargo build of that crate and triple
  therefore unlinks the file for an instant.
- /tmp/s14/warm.log shows it happening for real. Courtyard's own cargo was fresh (0.09 s),
  and its copy still failed on errno 2.
- `race_cargo.sh` (40 fresh cargo builds against a copy loop) reproduced it: 1 failed copy of
  543. The window is tiny, which is why it is rare.

Who can hit it:
- Two apps on one engine checkout.
- One app's `flutter test` beside its own `flutter build`.
- A developer's `cargo build --target <same triple>` during a Flutter build.

I didn't run two real `flutter test`s side by side: the hook is skipped once its dependencies
are cached, so each try needs fresh apps, and the race window is microseconds.

The error carries no hint. Fix: retry the copy on PathNotFound, or flock a
`target/.fynkel-hook.lock` around cargo plus the copy.

### GD2 — §19 never shows `World(source:)` for headless tests that load assets (courtyard F2, marble 2): CONFIRMED (reading), confusing, S
- Every §19 test uses `World()`.
- `source:` appears only for `FynkelGame(source:)` (`writing-a-game.md:3393, 4278`).
- `AssetSource` is never named in the guide (typedef at `assets.dart:6`).
- The error message (`assets.dart:112`) is good.

### GD3 — `world.pausedListenable` comes from fynkel_flutter's `WorldListenables`, and §11 doesn't say (courtyard F3): CONFIRMED (reading), minor, S
- §11's prose (`writing-a-game.md:1268–1270`) names it bare.
- The guide's own lib/hud.dart (3183) imports only widgets and game.dart. Adding the panel
  there fails to compile.

### GD4 — `Stick(game.world.input)` reads like a shipped widget (courtyard F4): CONFIRMED (reading), confusing, S
- No package exports a Stick or joystick. One exists only in `examples/lab/lib/stick.dart`,
  which the guide doesn't point to.
- §22 (`writing-a-game.md:3982`) does say an OnScreenStick is "a Flutter widget calling
  input.setAxis2", that is, your own.
- Fix the line at 3391: point at the lab's stick, or ship one.

### GD5 — point lights cast no shadows (courtyard F5): CONFIRMED (documented gap)
The guide says so plainly (`writing-a-game.md:664`). It is a feature gap against Unity, not a
doc fault.

### GD6 — music plays through pause; `audio.music.ignorePause` is unnamed in the guide and applies "from the next track" (courtyard F6): CONFIRMED (reading), minor, S
- The music paragraph (`writing-a-game.md:1985–1991`) says it plays on through a pause but
  doesn't name `audio.music.ignorePause`.
- `mix.dart:100–102`: "true unless set. From the next track."

### GD7 — the bloom/AO-at-low warning prints in clean test runs (courtyard F7): CONFIRMED (reading), minor, S
`world.dart:526–543` prints once per world, per effect, in debug builds, via `print`. There is
no opt-out short of using a quality with the effect, or a camera without it. Every soak log
in CI shows it too.

### GD8 — tests of routines depend on their waits (courtyard F8): NOT an engine/guide fault
It was the user's own bug. At most a one-line §19 tip.

### GD9 — no way to check "makes no garbage" in a game's test, and the drag path can't be garbage-free (marble 1): CONFIRMED (reading), awkward, M
- `package:fynkel/testing.dart` has only run/tap/viewPoint, and the guide's "no garbage"
  claims have no recipe a game can use. The engine's own alloc tests use the VM service
  (`packages/fynkel/test/*alloc*`).
- `Vec3`, `Ray`, `Plane` and `ViewPoint` are immutable `final class`es (`math.dart:66, 356,
  383, 428`).
- `finger.position` (`input.dart:994`), `screenPointToRay` and `screenPointToPlane`
  (`components.dart:513, 537`) return new ones.

This is by design (no structs in Dart). The real gap is the missing test helper; pair it with
GT11.

### GD10 — additive near-white trails plus bloom make a large halo (marble 3): PARTLY, guide suggestion
This is correct engine behaviour. The guide does say additive brightens what's behind (757)
and that threshold-1 bloom catches "what's brighter than white" (810). It never connects the
two for overlapping trails or particles. One line in §13c would do.

### GD11 — `setSweep` throws after `Spline.setPositions` changed the spline; odd message (marble 4): PARTLY, minor, S
- **Not a bug:** these are two separate calls. setPositions accepts coincident knots
  (`splines.dart:211`) and `spline.length` is public to check first.
- **Confirmed:** the message is misleading. `splines.dart:712` says "needs at least two knots
  apart … this has $_count" (4) when the cause is zero length. It should say "its knots are
  all at one point (length 0)".

### GD12 — both bodies get onCollisionEnter, and `GameObject.id` is never mentioned (marble 6): PARTLY, minor
- Both sides hearing it is Unity's behaviour, and the guide implies it ("they come to a
  collider's object and its rigidbody's object … the other side always hears",
  `writing-a-game.md:2877–2884`). An explicit "both objects get it once each" sentence would
  help.
- `id` (`game_object.dart:163`) appears in the guide only as "the lowest id" (3591): confirmed.

### GD13 — `world.run` is a testing.dart extension, and the analyzer error has no hint (marble 7): NOT
§19's test blocks show `import 'package:fynkel/testing.dart'`. The message is the Dart
analyzer's, not Fynkel's.

### GD14 — `await game.loaded` works headless, and §19 doesn't say (marble 8): CONFIRMED (reading), minor
`.loaded` appears only for levels (`writing-a-game.md:1307, 3670`).

### GD15 — does a pooled object's release clear its trail and particles? (marble 9): PARTLY, guide gap
The engine already handles it:
- A hidden object's trail is cleared at the next particles step (`trails.rs:271–283`).
- A hidden system stops and clears; shown again, it restarts (`particles.rs:1825–1838`).
- `Pool.release` sets `active = false` (`pool.dart:114`).

§3 is silent on this. Possible edge (untested): a release and a `get` in the same frame skip
the hidden step, so the trail would streak from the old place.

### GD16 — making a root with arguments (marble 10): PARTLY, minor
The guide only shows tear-offs (`FynkelApp(Hello.new)`). `() => MarbleRun(...)` is plain Dart.
A one-line example would do.

### GD17 — `prefer_initializing_formals` lint (marble 11): NOT (not Fynkel)

---

## New things tripped over
- **BR3 is wider than reported.** These leaf calls also take 10–50 ms on a Mac:
  - shapes with settings at high segment counts
  - `fy_mesh_set_positions` with a count change
  - `fy_particles_simulate`
  - prewarm inside PLAY/step

  See BR3's table.
- **Pocket Hole analyzes and passes** (5 tests) at stage-14, so GT5 is unguarded, not broken.
- **BR4's practical effect** is a second wgpu surface on the same CAMetalLayer, with the old
  one dropped off the main thread.
