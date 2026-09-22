# Instrumentation (M3-16)

Blueprint §15.4 lists what a capture has to see; this is where each of
those numbers comes from, in which host, and what it measures. The
rule: every number is a **measurement of the real path**, not a model of
it — the GPU times come from Metal's own timestamps, the native heap
from the allocator every engine byte goes through, the Dart split from
stopwatches around the calls the SDK actually makes.

| §15.4 | Where it is measured | Surface |
| --- | --- | --- |
| Dart behaviour time | `World.frame` around every `fixedUpdate`/`afterPhysics`/`update`/`lateUpdate` and the host's `fixedUpdate` callback | `world.frameTimes.behavioursUs`; `FynkelFrameStats.behaviours` |
| native simulation | `World.frame` around `beginFrame`, the input capture, every `step` (flush, tick, snapshot index, diagnostics) and event drain | `frameTimes.nativeUs`; `FynkelFrameStats.nativeStep`; headless `step_time` |
| physics, animation, catalogue systems, transform propagation | the core's `SystemKind` timers inside `World::step` (M3-10) | `World.systemTimes()` (`FYNKEL_QUERY_SYSTEMS`); `FynkelFrameStats.systems` (the window's last tick); headless `systems[]` |
| render extraction | `fynkel_apple_viewport_render` around `World::extract`; the headless host once per frame | `ViewportStatsSnapshot.extract`; headless `extract_time` |
| render preparation | `Renderer::prepare` (uploads, culling, instance building) | `FrameStats::prepare_us`; `ViewportStatsSnapshot.prepare` |
| encode | `Renderer::draw` (pass recording) | `FrameStats::encode_us`; `ViewportStatsSnapshot.encode`; `cpuP*Ms` is encode + submit |
| GPU pass timings | `wgpu::QuerySet` timestamps at the boundaries of the shadow, scene, post and present passes, and one pair spanning the ambient-occlusion stage's three passes (Metal stage timestamps) | `FrameStats::gpu_*_us`; `ViewportStatsSnapshot.gpuShadow/gpuAo/gpuScene/gpuPost/gpuFrame`; the standalone `[1s]` line's `gpu ms` |
| draw/dispatch counts, visible/culled | the renderer's `FrameStats` (M3-06) | `ViewportStatsSnapshot.draws/instances/culled/shadowDraws` |
| bridge calls/bytes/copies | `Bridge` counters + `FynkelWorldStats` | `BridgeStats.ffiCalls/bytesIn/bytesOut/dartCopies` |
| Dart allocations and GC (C3) | `DartHeapProbe` (`package:fynkel_flutter`): the VM's `GC` timeline events and `getMemoryUsage` over the app's own service, once per stats window | `FynkelFrameStats.dartHeap` (`profileDartHeap: true`); `[alloc]`/`[stats]` lines; `benchmarks/m0/capture.py` `alloc_aot`, `benchmarks/m3/capture.py` |
| native memory | `fynkel_core::memory::CountingAllocator` (the `#[global_allocator]` of every final binary: the dylib, the CLI, the hosts) and `task_info` for the resident set | `FynkelWorldStats.native_bytes/resident_bytes`; `BridgeStats.nativeBytes/residentBytes`; headless `memory`; standalone `mem MB` |
| asset residency | `fynkel_assets::ResidencyStats::bytes` (CPU bytes of resident payloads) | `FynkelWorldStats.residency_bytes`; `BridgeStats.residencyBytes`; headless `memory.residency_bytes` |
| estimated GPU memory | `Renderer::gpu_bytes()`: resident meshes (vertex + index + skin buffers), textures (their mip bytes), the cube map, the attachments (depth, MSAA HDR, HDR, LDR per texel × samples), the shadow map, the instance/joint/debug/uniform/post buffers | `FrameStats::gpu_bytes`; `ViewportStatsSnapshot.gpuBytes`; standalone `gpu=` |
| Flutter frame/UI timing | `SchedulerBinding.addTimingsCallback` while `onStats` is set | `FynkelFrameStats.flutterBuild/flutterRaster` |
| loading/startup time | the wall time of each `fynkel_world_load_*` call, in the report | `LoadReport.loadMs` (`"timing": { "ms" }`) |
| subscription budget (C5) | `fynkel_world_set_snapshot_budget` | `world.snapshotBudget`; `DiagnosticCode.snapshotBudget` (20, `FYN-R0013`); `BridgeStats.snapshotBytes/snapshotPeakBytes/snapshotBudget` |

## The frame, split

One Flutter frame on the game path is `World.frame` (the ticks with the
behaviours) followed by the viewport's `render` (extract, prepare,
encode, submit); the GPU then draws it while the next frame is
simulated, and the compositor picks up the newest complete slot. The
timers nest like this:

```
FynkelFrameStats.simulate          World.frame wall time
  ├─ behaviours                    frameTimes.behavioursUs — the Dart callbacks
  ├─ nativeStep                    frameTimes.nativeUs — beginFrame, capture, step ×N, drains
  │     └─ systems[]               the last tick's phases (commands … physics … transforms … publish)
  └─ (the rest)                    the SDK's event decoding, the presentation flush
FynkelFrameStats.renderCall        the viewport render call
  ├─ viewport.extract              World::extract
  ├─ viewport.prepare              Renderer::prepare
  ├─ viewport.encode               Renderer::draw
  └─ cpuP*Ms − encode              queue submit
viewport.gpuFrame                  first pass begin → last pass end, on the GPU (trails by 3–4 frames)
  ├─ gpuShadow / gpuAo / gpuScene / gpuPost   the passes; they overlap, so the sum exceeds the span
flutterBuild / flutterRaster       Flutter's own UI-thread build and raster-thread work per frame
```

`FynkelFrameStats.totalWork` is `simulate + renderCall` per percentile —
an upper bound on the sum's percentile — and the number the blueprint's
**12 ms p95 total-work** target is about (§15.2; `benchmarks/m3/capture.py`
checks it on both scenes, below).

The per-frame Dart side is allocation free: `world.frameTimes` is one
`FrameTimes` object with four `Stopwatch`es, overwritten every frame;
the game view keeps per-window `List<double>` samples it clears at every
report (the M1 allocation gate is unchanged; see `tool/alloc_bench.dart`).

## GPU timestamps

`fynkel_render` requests `Features::TIMESTAMP_QUERY` when the adapter
has it (`device_features(adapter)`; Metal on every supported Mac and
iPhone) and, when granted, times the passes with two alternating
`QuerySet`s of eight timestamps: pairs at the boundaries of the shadow
pass (when there is one), the scene pass, the post pass and the present
pass (when the viewport presents). Metal writes stage timestamps at pass
boundaries, and on a tile-based GPU the passes **overlap** — the next
pass's vertex work starts before the previous pass's fragments finish —
so a resolve in the same command buffer can read an end stamp that is
not written yet. The renderer therefore resolves the *previous* frame's
set at the start of the next `draw`, copies it to one of three staging
buffers, and maps that buffer asynchronously; `prepare` collects whatever
mapped (`device.poll(Poll)`, never a wait). The numbers trail the draw
by three or four frames and each is a whole pass (begin→end), not a sum
of draw calls; `gpu_frame_us` is `max(end) − min(begin)` over the used
pairs — the GPU's real time for the frame, less than the sum of the
passes because of the overlap. A frame whose stamps could not be taken
(every staging buffer still in flight) is counted in `gpu_untimed`, and
`gpu_timed` counts the read-backs, so a consumer that samples per frame
(the ring stats, the standalone host) pushes a GPU sample only when the
count changed. A device without the feature reports
`gpu_timestamps: false` and zeros. The stamps are wall-clock: in a
Flutter host the GPU is shared with Flutter's raster thread (Impeller's
command buffers run concurrently on the same hardware), so a pass's
span there includes whatever ran beside it — the stress scene's post
pass reads 1.5 ms p95 in one capture and 6 ms in the next while the
standalone host, alone on the GPU, reads a steady 2.5–2.8 ms frame span.
Compare a renderer change on the standalone host's numbers; read the
Flutter host's as the frame's real GPU occupancy.

The timestamp period is 1 ns on Apple GPUs; the pass times below are
from the M3 Max at 3024×1964 on the fixture project (the M3-16 renderer
test asserts the mechanism, not values):

| pass | typical |
| --- | --- |
| shadow (2048², two cubes) | 0.08 ms |
| scene (4× MSAA) | 0.07 ms |
| post (tonemap + present) | 0.04 ms |
| frame span | 0.4–0.7 ms |

## Memory

`fynkel_core::memory::CountingAllocator` wraps the system allocator with
two atomics (bytes in use, peak); it is the `#[global_allocator]` of
every **final** artifact — `fynkel_native` (the dylib the Flutter host
loads, on its own or inside `fynkel_platform_apple`), the CLI, the
standalone and headless hosts — never of a library crate, so each binary
installs exactly one. `heap_bytes()`/`heap_peak_bytes()` are the
engine's whole Rust heap (the Dart heap is not in it; the C3 probe is).
`resident_bytes()` is the process's resident set from
`task_info(MACH_TASK_BASIC_INFO)` on Apple platforms and 0 elsewhere.
`FynkelWorldStats` carries all of it plus `residency_bytes` (the CPU
bytes of the resident cooked payloads — meshes, textures — which become
resident on first use by a renderer or a mesh collider, so a headless
run reports 0) and the snapshot fields below; the renderer's
`gpu_bytes` is the estimate of what it holds on the GPU.

## The Dart heap (C3)

`DartHeapProbe` (`package:fynkel_flutter/src/heap_probe.dart`) connects
to the VM service the running app itself exposes (`dart:developer`
`Service.getInfo()`; a debug or **profile** build has one, a release
build has none — `connect()` returns null there and the stats stay
absent, which a HUD should say rather than show zeros) and, once per
stats window, reads two small things: `getMemoryUsage` (heap in use,
capacity, external) and the window's `GC` **timeline events** — one
per collection, carrying the heap before and after it to the kilobyte,
its reason, and the mutator pause from its begin/end stamps. From those
the window's **bytes allocated** is exact: the heap's growth between
the points where it is known (the previous sample, before and after
each collection, this sample) — every allocation counts, inlined or not,
new and old space. The probe sets the VM to record only the `GC` stream
while it is open (a window's `getVMTimeline` answer is ~20 KB instead of
the frame timeline) and restores the streams on `close()`.

Why not `getAllocationProfile`: its answer lists every class the isolate
knows (5–6 MB of JSON per call in a Flutter app); decoding it allocated
~200 KB per frame in the app it was measuring, and its counters are kept
by the allocation stubs, so an inlined allocation is not in them. The
first version of the probe used it and read 242 KB/frame in the Flutter
host; the timeline version reads 43 KB/frame in the same host, and a
window that allocates nothing reads under a megabyte with the probe's
own traffic in it.

`FynkelGameView(profileDartHeap: true)` opens a probe when `onStats` is
set and puts `DartHeapStats` (`heapUsage`, `heapCapacity`,
`externalUsage`, `allocatedBytes`, `gcCollections`, `gcPauseMs`,
`newSpaceCollections`, `oldSpaceCollections`, `gcReasons`) on every
`FynkelFrameStats.dartHeap`; `hosts/flutter_apple` and the reference
game turn it on and show a `dart heap` line (or "no VM service"). The
**AOT numbers** come from the `--profile` bundle (AOT like release, with
the service): `benchmarks/m0/capture.py` builds it, runs it and parses
the host's `[alloc]` lines into `alloc_aot` (`benchmarks/m0/results`),
next to the JIT `tool/alloc_bench.dart` numbers it always had.

### What a Flutter frame costs, and what was Fynkel's

The first captures read 39–43 KB/frame in the host and 56–69 KB/frame
in the reference game with the SDK path alone at 1.5 KB/frame, and
status §3 carried the gap. Attributed on 2026-09-22 (profile bundles,
the probe's `[stats]` line, each configuration run alone, KB/frame):

* **A blank Flutter app with a running `Ticker` allocates ≈ 12 KB per
  frame** on the Dart heap in a profile build — the frame's timeline
  events, the scheduler's callbacks, the scene build — before any
  Fynkel code runs (a release build has less instrumentation and no
  service to read it from). A `CircularProgressIndicator` adds ≈ 7
  KB/frame (its animation rebuilds and repaints every frame); a `Text`
  rebuilt every frame ≈ 16 KB/frame.
* The **host** (`hosts/flutter_apple`): blank 12.5; the host's page
  without the viewport 22.3; the game view tree with the tick skipped
  22.8; + the heap probe 22.7; + `onStats` 23.4; + the tick 24.4; the
  full host 34.6. The host's number is the framework's baseline plus its
  own two progress indicators (the "keeps Flutter animating at display
  rate" one on the page and the overlay's) and its per-second rebuilds —
  they stay, they are the demo; Fynkel's share of a host frame is 2–3 KB.
  Read the host's `[alloc]` line with that in mind.
* The **reference game** idle at the spawn: 57.3 with everything on;
  without the projection layer 53.7; without behaviours 18.5; without
  behaviours and input 18.2; without those and the projection 14.7;
  without the frame, the render call and the projection 13.7 — so
  `render()` and the engine's frame were ≈ 1 KB and **the behaviours
  were ≈ 39 KB/frame, all of it in the SDK's read path**: `Player`
  calls `world.find(cameraName)?.get<ThirdPersonCamera>()` every tick,
  and `find` re-fetched and re-decoded the whole entity table (names,
  parents, children — maps and strings for 180 entities) each tick,
  `get<T>()` decoded *every* component of the object into a fresh map
  each tick, and the label layer repainted every frame.

Fixed the same day (`package:fynkel` `world.dart`, `projection.dart`,
`wire/buffers.dart`, `native/bridge.dart`; `package:fynkel_flutter`
`projection_layer.dart`, `game_view.dart`), each a contract kept:

* the entity table is re-decoded only when the `entities` query's bytes
  differ from the retained copy of the last decode (a step marks it
  stale; the next `find` runs the query — in place, no copy — and
  compares eight bytes at a time; every spawn, despawn, rename and
  reparent shows in those bytes, which is why the check is the bytes and
  not the `Spawned`/`Destroyed` events — a prefab `instantiate` emits
  none);
* a typed read copies the tick's `QUERY_COMPONENTS` answer into a buffer
  the object keeps (reallocated only when the length changes), reads the
  ids present from the headers as a bitmask, and decodes only the
  component asked for; `components` decodes them all; the one query per
  object per tick and read-your-writes are unchanged (`components_test`
  asserts the query counts as before);
* `WireBuffer.reparse()` reuses the buffer's `ByteData` instead of
  making one per query; the bridge keeps one cached query view **per
  query kind**, so an `entities` query and the `components` queries of a
  tick no longer evict each other's view (a re-wrap is five objects);
* `ProjectionView.update()` writes the batch through one reused record,
  reads the answers in place, and notifies its listeners **only when an
  anchor moved, was re-flagged, appeared or was dropped**; the layer
  wraps each child in a `RepaintBoundary` (a moved label is
  re-composited); the game view reuses its two stopwatches.

After: the reference game **17.6 KB/frame** idle at the spawn (the
same scene without behaviours 15.6 — the behaviours are ≈ 2 KB now),
1.3 scavenges a second, in the attribution's harness (six one-second
windows after a four-second warm-up, the median); `benchmarks/m3/
capture.py` on its own 12 s window, which includes the scene's first
seconds, reads **26.3 KB/frame and 25 collections** where the same
window read 68.8 and 61 before (`results/2026-09-22-871c353` against
`0427213`). The host's `alloc_aot` in `benchmarks/m0` stays at 43–45
KB/frame — its progress indicators, by design. `tool/alloc_bench.dart`
gained one `find` and one typed `get` per tick as the regression check
— they cost ≈ 180 B per frame on top of the movers (2.6 → 2.7 KB/frame
JIT). What remains is the framework's frame and the game's own HUD; a
game that wants less rebuilds less.

## The subscription budget (C5)

Blueprint §5.4 says a snapshot is bounded by what the host subscribed;
`fynkel_world_set_snapshot_budget(world, bytes)` (`world.snapshotBudget`
in Dart) makes that a check. The first tick whose snapshot exceeds the
ceiling raises `SNAPSHOT_BUDGET` (20, message `FYN-R0013: snapshot of N
bytes over the B-byte budget (S subscriptions)`) on the simulation lane
— **once**, until the snapshot is back under, then again on the next
crossing — and the snapshot is still delivered whole: the diagnostic
tells the host it subscribed more than it meant to pay for; it never
drops data. 0 (the default) is no ceiling. `snapshot_bytes` (the last
tick's) and `snapshot_peak_bytes` are in the stats so the ceiling can be
chosen from a measurement: eight subscribed transforms are 728 bytes
(the 24-byte buffer header, 88 bytes per record).

## Where the numbers show up

- **`fynkel test --headless`** (and `--json`): the report's `step_time`,
  `extract_time` (the host extracts every frame, as a renderer would),
  `frame_time` (begin_frame → extract), `systems[]`, `memory {
  native_bytes, native_peak_bytes, resident_bytes, residency_bytes }`;
  the text mode prints `timing: step p50 58 / p95 339 / max 1011 µs;
  extract …; frame …` and `memory: native 0.9 MB (peak 6.6), resident
  32.4 MB, assets 0.0 MB`.
- **The standalone host's `[1s]` line** gains `sim ms` (ticks + extract),
  `prepare ms`, `gpu ms … n= untimed=` (the frame span, samples read back
  this window; `untimed` is the renderer's running total) and `mem MB
  native= resident= gpu=`; `benchmarks/m3/capture.py` parses them.
- **`FynkelFrameStats.summary()`**: the window as one line of stable
  `key=value` pairs (counts; every timer as `p50/p95/p99` ms — `interval`,
  `simulate`, `render`, `behaviours`, `native`, `work`, `build`, `raster`,
  `extract`, `prepare`, `encode`, `cpu`, `gpu_complete`, `gpu_frame`,
  `gpu_shadow`/`gpu_ao`/`gpu_scene`/`gpu_post`; `gpu_n`, `gpu_untimed`;
  the memory in bytes; `heap`, `heap_alloc`, `gc`, `gc_pause` when the probe is on).
  `hosts/flutter_apple` prints it as `[stats] …` every window, the
  reference game does under `FYNKEL_CAPTURE=1`; the M3 capture parses
  one format for both scenes.
- **`hosts/flutter_apple`**'s stats block: `dart behaviours ms … native
  step ms … total work ms`, `rust extract ms … prepare ms … encode ms`,
  `gpu pass ms shadow/ao/scene/post/frame`, `memory MB native/resident/
  assets/gpu snapshot B (peak, budget)`, `dart heap KB … allocated
  KB/frame … gc n= pause ms=`, `systems µs (last tick)`; the `[host]`
  line carries them and `[alloc]` the heap for `benchmarks/m0/capture.py`.
- **The starter, the reference game (F1) and the showcase** HUDs: a
  `work` line (behaviours / native / render / total at p95), a `gpu`
  line (frame and passes at p95, the memory estimate), a `memory` line;
  the reference game a `dart heap` line.
- **`ViewportStatsSnapshot`** (viewport ABI v3; v4 added the editor's
  debug lines and frame read-back, no stats change; **v5** added `gpuAo`,
  R2's ambient-occlusion stage): `extract`, `prepare`,
  `encode`, `gpuTimestamps`, `gpuFramesUntimed`, `gpuShadow`, `gpuAo`,
  `gpuScene`, `gpuPost`, `gpuFrame` (each `MsPercentiles` over the
  window; the GPU ones counted by read-backs), `gpuBytes`.

## The captures

`benchmarks/m0/capture.py` keeps its M0/M1 tables and adds `alloc_aot`
(above). `benchmarks/m3/capture.py` (`--skip-build` after `cargo build
--release --workspace`, `flutter build macos --profile` in
`hosts/flutter_apple` and `examples/reference_game`; run it **alone**
and under `caffeinate -du` — the hosts present to a real surface, so an
idle display throttles them and the windows come out short) records both
scenes against the manifest (`benchmarks/profiles`):

- **the stress scene** — cooked, rendered for the counts, its physics and
  systems headlessly (items 8, 10, 19c), the standalone host's `[1s]`
  lines and the Flutter host's `[stats]` lines (the profile bundle, so
  the Dart heap is in them);
- **the many-lights variant of the stress scene** (E1 light culling) —
  the same field regenerated with 256 point and spot lights
  (`generate.py --lights`, `benchmarks/stress/project-lights`, its own
  lock and cook), rendered for the per-object light counts and run in
  the standalone host, so `prepare` under many lights is measured
  against the same scene without them;
- **the reference game** — cooked, the Playground rendered for the
  counts, the route (`test/collect_all.input.json`) replayed headlessly
  for the tick-side timers (`step_time`, `extract_time`, `frame_time`,
  `memory`, `systems`, physics, animation, audio) under its checksum,
  the Playground in the standalone host (`--scene <id>`), and **the game
  itself**: its profile bundle with `FYNKEL_CAPTURE=1`, which loads the
  Playground at the first frame instead of the Menu and prints
  `[stats]` — the player idle at the spawn, every behaviour ticking.

The gate adds four checks to the earlier items': **12 ms p95 total
work** on the stress scene in the Flutter host and on the reference game
in its own app (`work` p95 < 12, from the windows after the warm-up),
the GPU pass timestamps read back in both apps, and the headless report
carrying the timers on the route (with the Dart heap recorded, not
gated). E1 adds two more: the light pool and the per-object budget hold
on the many-lights variant, and its `prepare` p95 stays within a
millisecond of the baseline run's (`2026-09-22-64221f7`: 2.10 ms against
1.64 with 256 lights over the field). `benchmarks/m3/results/<date>-<commit>.{json,md}` is the
record; the first with these numbers is `2026-09-22-0427213`: the
stress scene's total work **7.59 ms p95** in the Flutter host (simulate
3.50 — native ticks with 540 bodies, 200 rigs and 5 000 movers —, the
render call 4.08 of which extract 2.13 and prepare 1.74; the GPU frame
2.75 ms p95 on the standalone host), the reference game's **1.62 ms
p95** (behaviours 0.19, native step 0.32, render() 1.09); the Dart heap
39 KB/frame allocated in the host and 56 KB/frame in the game (the
Flutter framework's own per-frame work is in that number; the SDK path
alone is 1.5 KB/frame, `tool/alloc_bench.dart` — attributed and cut to
17.6 KB/frame in the game the same day, § What a Flutter frame costs),
3–4 scavenges a second of 0.15–0.25 ms each. What varied between three runs on the
same build was the tail, not the medians: the headless physics run's
p95/max (3.9/8.8, 3.7/8.5, 13.3/48.8 ms — one run in three has a burst
of slow ticks; the p50 is 3.2–3.5 ms every time) and the GPU spans in
the Flutter host (above). Every check passed on the third run; the
second failed the standalone's `p99 interval < 10 ms` on one 11.4 ms
window, the first the stale `14 phases` count (audio made it 15).

## Tests

- `crates/fynkel_render/tests/shading.rs`
  `pass_timestamps_and_the_memory_estimate`: five offscreen frames with
  the feature — prepare/encode timed, `gpu_bytes` past a megabyte,
  `gpu_scene_us > 0`, `gpu_frame_us ≥ gpu_scene_us`, nothing untimed.
- `crates/fynkel_core/src/memory.rs`: the resident set is reported on
  Apple.
- `crates/fynkel_native/tests/abi.rs`
  `memory_stats_and_the_snapshot_budget`: the heap and resident set in
  the stats, eight subscriptions against a three-record budget — one
  diagnostic, the snapshot whole, silence until the next crossing,
  0 removes the ceiling.
- `packages/fynkel/test/instrumentation_test.dart`: `frameTimes` split
  (a behaviour that burns time shows in `behavioursUs`, the ticks in
  `nativeUs`, the total covers both), the stats and the budget through
  the SDK (`DiagnosticCode.snapshotBudget`, `FYN-R0013`), `loadMs` on
  the fixture.
- `packages/fynkel_flutter/test`: the viewport's stages and GPU memory
  after a render, ABI v3; the game view's window stats carry the split,
  the systems, the viewport's stages, and `summary()` every key once;
  `heap_probe_test.dart` under `flutter test --enable-vmservice`
  connects, allocates ≈ 40 MB and reads it back within a few percent
  with the scavenges it forced, then a quiet window under a megabyte
  (the plain runner has no service: the probe is null and the test
  returns).
