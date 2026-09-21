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
| GPU pass timings | `wgpu::QuerySet` timestamps at the boundaries of the shadow, scene, post and present passes (Metal stage timestamps) | `FrameStats::gpu_*_us`; `ViewportStatsSnapshot.gpuShadow/gpuScene/gpuPost/gpuFrame`; the standalone `[1s]` line's `gpu ms` |
| draw/dispatch counts, visible/culled | the renderer's `FrameStats` (M3-06) | `ViewportStatsSnapshot.draws/instances/culled/shadowDraws` |
| bridge calls/bytes/copies | `Bridge` counters + `FynkelWorldStats` | `BridgeStats.ffiCalls/bytesIn/bytesOut/dartCopies` |
| Dart allocations | the VM service (`tool/alloc_bench.dart`, JIT; the C3 probe and the AOT capture, part 2) | `benchmarks/m0/capture.py` |
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
  ├─ gpuShadow / gpuScene / gpuPost   the passes; they overlap, so the sum exceeds the span
flutterBuild / flutterRaster       Flutter's own UI-thread build and raster-thread work per frame
```

`FynkelFrameStats.totalWork` is `simulate + renderCall` per percentile —
an upper bound on the sum's percentile — and the number the blueprint's
**12 ms p95 total-work** target is about (§15.1; the capture checks it,
part 2).

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
`gpu_timestamps: false` and zeros.

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
  this window) and `mem MB native= resident= gpu=`;
  `benchmarks/m3/capture.py` parses them (part 2).
- **`hosts/flutter_apple`**'s stats block: `dart behaviours ms … native
  step ms … total work ms`, `rust extract ms … prepare ms … encode ms`,
  `gpu pass ms shadow/scene/post/frame`, `memory MB native/resident/
  assets/gpu snapshot B (peak, budget)`, `systems µs (last tick)`; the
  `[host]` line carries them for `benchmarks/m0/capture.py`.
- **The starter, the reference game (F1) and the showcase** HUDs: a
  `work` line (behaviours / native / render / total at p95), a `gpu`
  line (frame and passes at p95, the memory estimate), a `memory` line.
- **`ViewportStatsSnapshot`** (viewport ABI v3): `extract`, `prepare`,
  `encode`, `gpuTimestamps`, `gpuFramesUntimed`, `gpuShadow`, `gpuScene`,
  `gpuPost`, `gpuFrame` (each `MsPercentiles` over the window; the GPU
  ones counted by read-backs), `gpuBytes`.

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
  the systems, the viewport's stages.
