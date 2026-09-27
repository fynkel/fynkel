# Foundation pass

Making what Fynkel already has solid before anything new is built on it.
Started 2026-09-27 at the user's request ("now it is time for us to make
everything of what we have solid and scalable and extendable so we can
build on top of it safely"), after an audit of six areas: the behaviour
lifecycle and time, objects/scenes/prefabs, physics, the Rust ↔ Dart
bridge and input, the asset pipeline, and engineering health. Unity is
the reference: where Fynkel differs on purpose, the row says why.

The audit read the code; it did not run it. So each row is **confirmed
by a failing test first**, and only then fixed. A row the test does not
confirm is closed as "not a bug" with the test kept. Every fix keeps its
test, so it stays fixed. Where a fix changes behaviour an ADR fixes, the
ADR is amended and the user confirms the amendment before the code.

Severity: **C** critical (a game hits it soon, or it corrupts or loses
work) · **I** important · **m** minor.
Status: ❌ open · 🔬 confirmed by a failing test · 🔧 fixed (with the
commit) · ✅ not a bug (the test stays) · ⏸ decided later (why) · 👀 watched.

## Step 0 — the safety net

| | What | Status |
|---|---|---|
| 0.1 | Push the 63 local commits | 🔧 `5fc7285` pushed 2026-09-27 |
| 0.2 | CI running again: GitHub-hosted jobs have not started since 2026-09-23 (the account's billing). A self-hosted runner on the developer's Mac (`~/actions-runner`, a launchd service, label `fynkel`) | 🔧 `.github/workflows/ci.yml`; first green run 2026-09-27 (run 36287057594: rust 14 min, dart 6 min, wasm) |
| 0.3 | The drift CI finds on its first run: Rust formatting in five files and one clippy lint; the Homebrew `dart` and the stale `~/.cargo/bin/fynkel` on the runner; two steps that needed a `pub get` first (the generator's example, code_only's package graph); the showcase's stale codegen; the reference game's `assets.g.dart` naming the git-excluded Mixamo hero (generated with it present, abf2787); the projection test still expecting the crate's pre-ADR-0022 root; two Animator dialogs that animated (the look test); `tools/playground.py` rewriting the level and the Player's controller, which are edited documents since ADR-0022 and 120b030 (now only with `--level`) | 🔧 |
| 0.5 | Flaky: `compile_error_parity_test` (the IDE's `opened.txt` not written within 20 s) failed once in a full editor run on the runner and passed alone and paired 4 times; watch it | 👀 |
| 0.6 | `platform/apple` `viewport_lifetimes_do_not_grow_the_process`: 4.4 MiB of Metal cache slack on a busy runner against a 4 MiB bound (a leaked ring would be ~240 MiB); the bound is 8 MiB | 🔧 |
| 0.4 | This checklist | 🔧 |

## Step 1 — how scripts feel

What every script touches. Most of it is `packages/fynkel`.

| | Sev | Unity | Fynkel today | Status |
|---|---|---|---|---|
| 1.1 | C | `Time.deltaTime` inside `FixedUpdate` is the fixed step | `Time.delta` was the frame's dt everywhere: `x += v * Time.delta` in `fixedUpdate` moved 6 m a second at 30 fps instead of 3 (`test/time_test.dart` failed so). Now the fixed step in `fixedUpdate`, `afterPhysics`, the contact callbacks and the host's `fixedUpdate` callback | 🔧 |
| 1.2 | C | A `transform` write in `Update` shows that frame | Writes go to the simulation lane and reach the screen at the next tick (1–2 ticks later; on a frame with no tick the object is drawn where it was: `test/update_writes_test.dart` measured ~183 px behind). This is why the examples move things in `fixedUpdate`. ADR-0008 already allows presentation writes in `lateUpdate`; the SDK never routed `transform` there | 🔬 amendment proposed (ADR-0008, 2026-09-27), waits for the user |
| 1.3 | I | `Time.timeScale`; `timeScale = 0` still runs `Update` | There was no time scale; pausing stopped the host, so `update` and the UI logic stopped too, and a `pause` action could not resume (`time_test` did not compile). Now `Time.timeScale` (and `World.timeScale`) scales what the clock gets: fewer ticks at 0.5, none at 0; `update` still runs, `Time.delta` there is scaled and `Time.unscaledDelta` is the real frame. `fixedDelta` stays; coroutine waits follow scaled time as Unity's `WaitForSeconds` | 🔧 |
| 1.4 | I | One clock | `Time.elapsed` counts ticks, coroutines sum the raw frame dt, `update` gets the unclamped dt | ❌ |
| 1.5 | C | An error is logged and the frame goes on | In debug a rejected command threw `FynkelError` out of the tick: the next tick, `afterPhysics`, `update`, `lateUpdate` and that tick's contacts were lost (`script_errors_test` failed so: 1 tick of 2). Now `World.frame` finishes the frame and then throws (one `FynkelError` with every diagnostic): ADR-0014's "debug builds throw" holds, the frame is whole. `step()` called directly still throws at once | 🔧 |
| 1.6 | I | A host callback that throws is logged | The host's `fixedUpdate` callback and a `FynkelBridgeError` are not caught; the remaining ticks and the presentation flush were lost (`script_errors_test` failed so: 1 tick of 2). Now `World.frame` holds what they throw to the end of the frame, as 1.5, then throws the first (the rest are printed) | 🔧 |
| 1.7 | I | `Instantiate` runs `Awake`/`OnEnable` before it returns | `onCreate` runs at the next phase boundary, so code after `instantiate` runs before it; `world.spawn` is deferred while `instantiate`/`clone` are immediate | ❌ |
| 1.8 | m | Every pending `Start` runs before any `FixedUpdate`/`Update` | A newly enabled behaviour's `start` runs interleaved in the pass | ❌ |
| 1.9 | m | `GetComponent` never returns a destroyed component | `behaviour<T>()`/`sibling<T>()` return destroy-requested behaviours, `_own` does not | ❌ |
| 1.10 | m | — | `worldTransform`, `activeInHierarchy`, `find*` read last tick (not read-your-writes); say so or fix | ❌ |

## Step 2 — physics correctness

`crates/fynkel_physics/src/sim.rs` and its use in `fynkel_core`.

| | Sev | Unity | Fynkel today | Status |
|---|---|---|---|---|
| 2.1 | C | Child colliders join the nearest parent Rigidbody as one compound; their events go to the Rigidbody's object | Each entity is its own Rapier body: a child `Collider` under a dynamic parent is a separate fixed collider, one tick behind, no mass, and may collide with its own parent. `docs/physics.md` recommends child entities for several colliders | ❌ |
| 2.2 | I | Changing a collider, layer or scale keeps velocity and fires nothing | The body is rebuilt: velocity and sleep are lost, a false Exit then Enter fires (`sim.rs` sync) | ❌ |
| 2.3 | I | `OnTriggerStay` | None (sensor pairs are skipped for Stay) | ❌ |
| 2.4 | I | `OnControllerColliderHit`, `CollisionFlags` | The character reports no hits against static or kinematic geometry; only `grounded`/`sliding` | ❌ |
| 2.5 | I | `RigidbodyConstraints` (freeze position/rotation axes) | None (Rapier has locked axes) | ❌ |
| 2.6 | I | No Exit when the other object is destroyed or deactivated (Fynkel's Exit is better) | Exit fires but no test covers it; a destroyed zone's own `ZoneExited` is lost | ❌ |
| 2.7 | m | New colliders are queryable at once | Queries see the world as the last step left it | ❌ decide: fix or document |
| 2.8 | m | Queries hit triggers by default; a ray starting inside a collider ignores it | `hitSensors: false` by default; hits at distance 0 | ❌ decide |
| 2.9 | m | `Collision` has contacts, impulse vector, relative velocity | One deepest point, a normal, a scalar impulse | ❌ |
| 2.10 | m | Collision messages reach disabled scripts | Disabled behaviours get none | ❌ decide |
| 2.11 | m | — | `physics.dart` says an overlap's point is the collider's origin; the code returns zero | ❌ |
| 2.12 | I | Joints; ForceMode; AddForceAtPosition; convex mesh colliders | Missing | ⏸ features, after the pass |

## Step 3 — objects at scale

`fynkel_scene`, `fynkel_core` (`load.rs`, `world.rs`), `fynkel_native/src/load.rs`, `fynkel_document/src/prefab.rs`.

| | Sev | Unity | Fynkel today | Status |
|---|---|---|---|---|
| 3.1 | C | `Instantiate` is an in-memory copy | Each runtime `instantiate` re-reads the cooked file, re-parses the document, rebuilds the schema set, re-registers colliders/animation/audio, and pushes a residency token released only by a replace load (memory grows with every spawn); each also runs a full `refresh_active` and `propagate` | ❌ |
| 3.2 | I | Pooling is a common pattern | No pooling API or guidance | ❌ |
| 3.3 | C | Only changed transforms are recomputed | `propagate` walks every node 2–4 times per tick (it only skips the multiply when clean; its doc says "dirty subtrees only"); a keep-world `SetParent` runs a full pass per command; `record_prev_world` decomposes every matrix | ❌ |
| 3.4 | I | — | `refresh_active` walks the whole hierarchy on any parented spawn while an inactive object exists | ❌ |
| 3.5 | C | Prefab-internal references are remapped per instance | `Follow.target`, `ThirdPersonCamera` targets are names resolved world-wide to the first match: two instances bind to the same "Head"; a missing target costs an O(N) scan every tick | ❌ |
| 3.6 | I | `DontDestroyOnLoad` | A replace load destroys everything | ❌ |
| 3.7 | I | `LoadSceneAsync` | Loads are synchronous on the main isolate | ❌ |
| 3.8 | I | An override whose target is gone is kept and listed for cleanup | One stale override (D0403) skips the whole instance: deleting a child from a prefab breaks every scene that overrode it | ❌ |
| 3.9 | — | — | A 50k-entity headless benchmark, the gate for 3.1–3.4 (nothing is measured above ~16k) | ❌ |
| 3.10 | m | — | `despawn_deferred`/`flush_deferred` despawn one entity, not the subtree (no callers today) | ❌ |
| 3.11 | m | — | 24-bit generations wrap after 16.7M reuses of one slot (LIFO free list) | ⏸ |

## Step 4 — the bridge

`packages/fynkel/lib/src/native`, `world.dart`, `packages/fynkel_flutter`, `crates/fynkel_native`, `platform/apple`.

| | Sev | Unity | Fynkel today | Status |
|---|---|---|---|---|
| 4.1 | C | A crash is reported once | After a Rust panic the world is poisoned and `beginFrame` throws every frame, forever (~60 exceptions a second, no render); `FynkelGameView._frame` has no try/catch | ❌ |
| 4.2 | C | A destroyed object throws | After `World.dispose`, `_snapshot` still points into freed native memory: a transform read reads freed memory | ❌ |
| 4.3 | C | — | `platform/apple`'s 22 `extern "C"` functions have no `catch_unwind` (and `lock().unwrap()`): a panic there aborts the app | ❌ |
| 4.4 | I | — | The snapshot grows with every handle Dart ever touched (subscribed on `attach`/`find`, never dropped, 88 B each per tick); `step()` walks every GameObject | ❌ |
| 4.5 | I | — | `fynkel_world_input_view` / `drain_events` hold two `&mut` to the world (undefined behaviour) | ❌ |
| 4.6 | I | Trackpad scrolling is the mouse wheel | Pan-zoom events are dropped by `FynkelInputCapture`: `mouse:wheel` never fires on a trackpad | ❌ |
| 4.7 | I | A UI button does not steal the game's keys | A focusable HUD widget takes focus, the viewport resets input, keys are dead until a click; `ExcludeFocus` is by hand | ❌ |
| 4.8 | m | — | `drain()` throws away the event Vec's capacity each tick; the input view clones action names each tick | ❌ |
| 4.9 | m | — | One shared query buffer for every query kind (a held view is overwritten) | ❌ |
| 4.10 | I | Pointer position, touches, several gamepads, deadzones, rebinding | Missing | ⏸ features, after the pass |

## Step 5 — assets

`crates/fynkel_import`, `fynkel_document`, `fynkel_assets`, the editor's project model.

| | Sev | Unity | Fynkel today | Status |
|---|---|---|---|---|
| 5.1 | C | A missing reference leaves the field empty; the scene loads | A missing reference fails the cook of that document and every document that depends on it (`cook.rs`): one deleted PNG and its scenes cannot load | ❌ |
| 5.2 | I | A copied asset gets a new GUID | A Finder copy of a document keeps the id; which file wins depends on sort order vs. arrival order (the copy can take the original's id) | ❌ |
| 5.3 | I | Import runs in parallel workers with progress | The cook is single-threaded on the editor's UI thread (synchronous FFI): a big FBX freezes the editor | ❌ |
| 5.4 | I | Async loads, preload | Meshes/textures decode on first use, synchronously: the first frame that uses one hitches | ❌ |
| 5.5 | I | ASTC/BC per platform | RGBA8/RGBA16F only (a 4K texture ≈ 85 MB); CPU payloads may stay after upload | ⏸ Phase 4 item, needed before a device |
| 5.6 | m | — | The bundled cache is unpacked on first launch and only checked with `exists()`: a cut-short write stays broken | ❌ |
| 5.7 | m | — | No fuzzing of the glTF/FBX/`.fyna` readers; an importer panic poisons the authoring session | ❌ |

## Step 6 — structure

Last on purpose: by then Steps 1–5 have pinned the behaviour with tests,
so the refactors cannot quietly change it.

| | Sev | What | Status |
|---|---|---|---|
| 6.1 | I | `World` (`fynkel_core/src/world.rs`, ~59 fields) split into subsystem states behind a small system trait with a declared order | ❌ |
| 6.2 | I | A built-in component is ~9 hand steps (`docs/codegen.md`): generate the `ComponentValue` arms, the `bridge::query` arm and the editor hints from one table | ❌ |
| 6.3 | I | `fynkel_render/src/lib.rs` (5,737 lines) split into modules | ❌ |
| 6.4 | I | The shipped runtime links `fynkel_document` (JSON Schema validation) through `fynkel_native` and `fynkel_assets` | ❌ |
| 6.5 | m | Thin tests: `fynkel_scene` 5, `fynkel_physics` 17, `hosts/standalone` 0, `fynkel_editor_api` 0 | ❌ |
