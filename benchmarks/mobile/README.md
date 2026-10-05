# Mobile performance workloads

These workloads extend F8; they do **not** certify 120 FPS. Use the same seed,
quality, pixel dimensions, build, and device conditions for comparisons.

| Preset | Native movers | Dynamic bodies | Dart behaviours | Skinned characters | Local lamps |
| --- | ---: | ---: | ---: | ---: | ---: |
| dense-500 | 500 | 50 | 30 | 20 | 8 |
| dense-2000 | 2,000 | 150 | 100 | 60 | 16 |
| dense-5000 | 5,000 | 500 | 300 | 200 | 32 |

All retain the floor, sun, shadows, AO, contact shadows, bloom, grading, and
particle fountain. Each character has 13 joints. Meshes share geometry and
materials: this measures instancing, extraction, physics, animation, and screen
space costs. It is not a substitute for unique materials, high polygon counts,
transparent overdraw, streamed environments, or a playable representative game.
Those workloads remain required before broader engine-performance claims.

Run from the repository, with Flutter on PATH:

```sh
python3 benchmarks/mobile/prepare.py dense-500 --out target/mobile-500
python3 benchmarks/mobile/prepare.py dense-500 --app --out target/mobile-app-500
```

The destination and adjacent evidence JSON must not already exist. Preparation
validates, cooks, renders at 1320×2868 medium, and simulates 120 native ticks.
The adjacent `*-preflight.json` retains command outputs, errors, actual visible
instances, culling, draws, animation/physics counters, and source/scene hashes.
The PNG is a camera/quality sanity check. These are **Mac headless checks**, not
iPhone timings. Bare projects do not execute Dart behaviours. `--app` generates
and registers their code and uses an orbiting camera.

The app harness replaces only the generated app's entry point. It logs
`Documents/mobile-capture.jsonl`, forces medium quality without saved-setting
contamination, and pauses after eight wall-clock seconds from the first frame.
It also pauses on serious/critical thermal pressure or lifecycle interruption.
A terminal stop record distinguishes completion from an interrupted/missing
capture. The app never automatically resumes a stopped capture. Relaunch for a
new capture. Each launch replaces its log, so archive evidence before relaunch.
Unknown thermal state is unknown, never evidence of a cool device.

Device execution must use the exact bundle identifier and record the PID from
`devicectl ... process launch --json-output`. Terminate only that PID; never
match all executables named Runner. An external runner timeout is still needed
for an app/UI thread hang. The bounded app is not a crash watchdog.

## Qualification target and remaining work

120 Hz gives 8.33 ms per displayed frame; 60 Hz gives 16.67 ms. Report both
loop callbacks and new compositor texture acquisitions; neither alone proves
even physical display presentation. Record CPU/GPU p50/p95/p99, repeated/dropped
frames, visible instances, draw calls, memory, thermal state, resolution and
quality. Short eight-second probes are diagnostic only. Sustained qualification
needs a declared longer duration, cool starting conditions, thermal monitoring,
and frame-pacing evidence, with an independent watchdog.

Next: bounded physical-device density captures; AO/contact-shadow cost reduction
with image regressions intact; explicit mobile quality profiles; material,
geometry and transparency stress variants; sustained 60/120 Hz qualification.
Do not silently reduce resolution/effects or label a diagnostic no-AO run as a
full-quality pass. Keep F8 open while these performance checks are incomplete.

The checked-in `capture.py` installs a specified app, verifies its bundle ID,
records launch JSON and exact PID, and stops that PID before copying logs. Example:

```sh
python3 benchmarks/mobile/capture.py --device DEVICE_UUID \
  --bundle YOUR_BUNDLE_ID --app PATH/Runner.app --out target/mobile-capture-1
```

Build/sign the generated app normally first (`flutter build ios --release`).
The runner allows 12 seconds after launch for startup and the eight-second app
capture; slow startup can therefore produce an incomplete capture, which fails
instead of silently qualifying. It preserves raw output and requires both stats
and a terminal stop record. A thermal/lifecycle stop is marked interrupted.
See the [device findings](../../docs/foundation-f8-device-observations.md) for the
measured 500 and 5,000 mover runs: the largest currently exposes a severe CPU
simulation bottleneck, in addition to GPU costs. Both sustained targets remain
unmet.

## CPU isolation

`prepare.py --app` also installs `tool/cpu.dart`. On a generated **dense-5000** app:

```sh
dart build cli -t tool/cpu.dart -o ../mobile-cpu-baseline
../mobile-cpu-baseline/bundle/bin/cpu /absolute/path/to/generated/project
```

Keep the whole baseline bundle before rebuilding native code. The probe reloads
the same scene for 0, 30, and 300 enabled Wobble behaviours, retains all 300
objects and all native workloads, warms 40 ticks, and records 120 single fixed
ticks per variant. It measures native, behaviour and total times without a
renderer or variable-step catch-up. This is a CPU diagnostic, not a device FPS
gate. An optional second argument sets the sample count and runs only the
300-behaviour variant for a sampling profiler. Disabling behaviours here isolates
cost; it is never a production optimization or full-workload qualification.

Add `--systems` to collect existing per-system native timings after each measured
tick (for example `cpu /absolute/project 300 --systems`). This readback is outside
the tick timer but adds work between samples; compare equally instrumented runs.
`step` is the whole native step and overlaps the other phase timings, so do not
sum it with them. Preserve the baseline bundle and reverse comparison order to
check drift; do not compile concurrently with controlled measurements.

## Phone CPU and presentation isolation

Build the bounded app with `--dart-define=FYNKEL_CAPTURE_CPU=true` to buffer CPU
samples in memory and write them at stop. Frame totals and catch-up counts are
separate from `lastTickSystems`, which describes only the last native tick in a
host frame. Zero-tick frames have no system readback. The sample cap is 4,096;
reaching it stops the diagnostic. These queries add overhead between frames and
are off by default. Analyze the captured JSON with:

```sh
python3 benchmarks/mobile/analyze_cpu.py target/my-capture/capture.json
```

`--dart-define=FYNKEL_CAPTURE_ACQUIRE_WAIT_US=0` isolates the presenter's bounded
wait without modifying the engine default (3,000 microseconds). The start record
reports the selected value. This is a diagnostic variable, not a qualified
production recommendation. Keep both flags the same when comparing other changes.

For repeatable GPU comparisons, `--dart-define=FYNKEL_CAPTURE_FREEZE_SCENE=true`
settles the dense fixture for exactly 120 fixed ticks before attaching the view,
then sets time scale to zero. The orbit, physics and animation remain at the same
simulation state; rendering and host-frame work continue. The visible label and
start record identify this mode. The eight-second rendering deadline starts with
the first frame, after the fixed CPU-only preparation. This is GPU isolation,
not a faster gameplay configuration or a dense-game FPS qualification. The option
is off by default; always compare builds using the same mode and quality settings.

### Native physics phase isolation

```sh
cargo run --release -p fynkel_host_headless --example physics_profile -- \
  target/f8-mobile-app-500/.fynkel/cache
```

This runs the cooked startup scene twice, with physics profiling disabled and
then enabled: 40 warmup ticks and 300 measured ticks per fresh world. It emits
per-tick synchronization, command, character, solver, writeback and event timings
and checks that final transforms and body/character motion states match. The
phases do not overlap; total includes timer overhead. Pre-physics scene
propagation and subsequent core event publication are outside these phases.
There are no Dart callbacks, rendering or variable-step catch-up, so these are
native diagnostic measurements, not live-game or phone FPS results.

Native callers can opt in with `Physics::set_profiling(true)` and read the last
completed sample through `step_profile()`. Profiling defaults off and reads no
clocks while disabled. Disabling clears the sample; a stripped physics runtime
returns no sample. Skipped core physics stages do not create new samples.

For the nested-collider synchronization path specifically:

```sh
cargo test --release -p fynkel_physics nested_sync_qualification -- --ignored --nocapture
```

This moves one static body with 5,000 child colliders, warms 20 iterations and
prints 60 synchronization samples. Transform propagation is outside the timed
region; the solver and rendering are not run. Use it alongside the full workload,
not as a substitute for live-game qualification.

For the common unparented moving-collider path:

```sh
cargo test --release -p fynkel_physics moving_collider_sync_qualification -- --ignored --nocapture
```

This updates the position and rotation of 5,000 colliders, warms 20 iterations,
and prints 60 synchronization samples. It checks that all candidates were
processed and a collider reached the final position. Scene propagation is
outside the timed region; no solver or rendering is run.

`--dart-define=FYNKEL_CAPTURE_CPU_TICKS=true` additionally records every native
step through the SDK frame timer after callback transactions close. It implies
CPU frame readback and is incompatible with the frozen-scene mode. The normal
release leaves this disabled. The diagnostic adds readback overhead, measured
separately, so it is for attribution rather than FPS qualification.

`analyze_cpu.py` checks that tick IDs increase and that each host frame has exactly
its declared number of tick samples. It reports first/last catch-up tick timings,
all-tick system medians, and the portion of the frame's native timer outside the
summed native `step` measurements. That remainder includes SDK/bridge work; it is
not a new engine system or an independently measured CPU phase.

Per-tick capture also records sequential SDK stages: subscription flush, bridge
step, observation sweep, snapshot copy/index, diagnostics, and event drain.
`bridgeStep` includes native `step` and bridge publication; do not add it to the
system timings. Stage clock reads are included in the measured segment; system
readback stays outside the native/behaviour timers. Collection is latched at the
start of each frame. Reports include means as well as medians because periodic
bridge spikes can disappear from a median even while dominating aggregate cost.

Use `capture.py --trace-gpu` to retrieve the renderer's opt-in screen-space trace
from the app's Documents directory. It records depth prepass, MSAA depth resolve
(zero when skipped), AO/contact estimation, blur, and the whole screen-space span.
The runner stops the exact launched PID before copying either log, and rejects
missing/malformed GPU samples. Trace I/O is diagnostic overhead; do not use these
runs as sustained FPS qualification. Pass spans can overlap and must not be added.

The app prominently labels `LIVE TEST`, `FROZEN GPU TEST`, and `TEST COMPLETE`.
All modes pause intentionally after eight seconds. A frozen test never animates;
a normal live test should animate until its deadline. Stats include the simulation
tick and mode so an intentional pause can be distinguished from a stalled engine.

Each scripted launch copies a one-use `Documents/capture-request.json` containing
a unique capture ID, echoed in the start record and used in the GPU filename.
The app consumes and deletes that request. This Flutter iOS host did not expose the
`devicectl` launch variables through `Platform.environment`; native GPU tracing still uses its native environment. A missing or mismatched ID is rejected, so
a previous successful log cannot qualify a failed new launch. Rebuild older
diagnostic apps before using this runner. Manual launches have no ID.

### Correlated frame diagnosis

The same release binary supports one-use capture options without rebuilding:

```sh
python3 benchmarks/mobile/capture.py --device DEVICE_UUID \
  --bundle YOUR_BUNDLE_ID --app PATH/Runner.app --out target/frame-trace \
  --trace-frames --trace-gpu
python3 benchmarks/mobile/analyze_frames.py target/frame-trace
```

Optional controls are `--freeze-scene`, `--acquire-wait-us 0`, `--buffer-count 8`,
`--resolution-scale 0.5`, and `--cpu-ticks`. Change one variable at a time except
for an explicitly identified combined control. These are diagnostic settings;
production defaults remain four buffers, 3,000 µs wait and full resolution.
CPU tick readback is incompatible with frozen simulation. The runner transfers a
unique request, verifies the app's acknowledged options, stops the exact PID,
and copies the unique native trace. No request means the normal harness defaults
(or explicitly compiled Dart defines). Keep diagnostic builds free of unrelated
Dart defines when using the runner's strict option acknowledgment.

`FYNKEL_TRACE_FRAME_FILE` enables a native single-viewport CSV with at most 16,384
records; relative paths resolve under HOME. Each viewport opens/truncates its
file, so use only one viewport per trace destination. `onFrameTrace` enables
bounded harness host timestamps; Flutter timing callbacks supply raster phases.
The analyzer rejects malformed/truncated traces and unmatched native render
attempts. It reports occupancy, dropped-attempt CPU work, correlated sequence
latency, and Flutter timing coverage. The acquisition rate is not a display FPS
measurement. Wall-clock changes invalidate correlation. Trace writes and CPU
readback introduce overhead; retain an untraced control.

See the [ranked diagnosis](../../docs/foundation-f8-performance-diagnosis.md).
To reproduce its analysis, unpack the linked raw archive into a temporary directory
and pass an extracted `f8-diagnosis-*` directory to `analyze_frames.py`. The
untraced control intentionally has no frame CSV.

After the early-admission change, native `stages` records occur only for admitted
frames; dropped attempts do not perform extraction/preparation. Ring `cpu` now
includes the admitted preparation callback, whereas the earlier metric began
later. Compare host render intervals for before/after CPU cost. Occupancy's
`rendering` column includes CPU-reserved slots; use `submitted` events to identify
actual GPU submissions. See the [admission results](../../docs/foundation-f8-render-admission.md)
for the measured throughput improvement and remaining frame-age tradeoff.

The presenter now waits only when no new image is ready; it immediately acquires
the newest completed image otherwise. `--acquire-wait-us 0` still disables the
remaining bounded wait. Historical captures used the older policy of waiting
for the newest submission even with a completed image available; compare source
and native-library fingerprints as well as the numeric wait setting.
`analyze_frames.py` now reports render-request-to-acquisition distributions in
addition to completion-to-acquisition time. See the
[presentation wait results](../../docs/foundation-f8-presentation-wait.md).

### Effect isolation through project profiles

Newly prepared fixtures include `medium-no-ao`, `medium-no-contact`, and
`medium-no-screen-space`, each copied from medium with only those effect flags
changed. Select with `capture.py --quality NAME`; combine with `--freeze-scene`
for an identical settled GPU workload. These are labelled diagnostic controls;
medium remains the default. The runner verifies profile acknowledgment and
asset-store validation, and records native framework hashes. Existing generated
fixtures must add the profiles and recook/rebuild before selecting them.

Use `--trace-frames` for all four profiles. Do not request `--trace-gpu` for
`medium-no-screen-space`: that trace specifically expects screen-space samples,
and a correctly disabled pipeline emits none. Whole-frame GPU timing remains
available in the normal statistics. See the
[current isolation results](../../docs/foundation-f8-effect-isolation.md).

### Bounded Metal execution traces

For stage-level investigation, add `--metal-trace-device UDID` to `capture.py`.
Use the physical-device UDID from `xcrun xctrace list devices`; it differs from
CoreDevice's `--device` identifier. This attaches Metal System Trace to the exact
launched PID for eight seconds. Recorder failure fails the capture, and cleanup
stops only that recorder child. Profiling changes timing; do not compare its FPS
directly with ordinary captures.

Run `python3 benchmarks/mobile/analyze_metal.py CAPTURE_DIRECTORY --export` to
export the two required tables and summarize the app's top-level active GPU
intervals by encoder/channel. Nested intervals and other processes are excluded.
Channel timings overlap and are not additive frame times. Full `.trace` bundles
stay in the capture directory; relevant exported tables can be archived separately.
See [the Metal diagnosis](../../docs/foundation-f8-metal-diagnosis.md).

`--trace-physics` records bounded native adapter phase timings to `physics.csv`
and validates them into `capture.json`. Combine with `--cpu-ticks` to compare
adapter work with the whole core physics system. Native completed tick IDs are
SDK pre-step tick IDs plus one. File writes and timers add diagnostic overhead;
use this for attribution, not baseline FPS qualification. Traces are capped at
8,192 rows and saturated/incomplete traces are rejected. See the
[phone phase report](../../docs/foundation-f8-physics-phases.md).

Physics traces also include four nested synchronization phases: `collect_ns`,
`hierarchy_ns`, `classify_ns` and `apply_ns`. Their sum is bounded by `sync_ns`;
do not add them to the outer phase total. The parser validates this containment.
[Repeated iPhone results](../../docs/foundation-f8-sync-hierarchy.md) document the
first optimization based on this breakdown.

Core physics tracing also records `propagation_ns`, `core_adapter_ns`,
`publication_ns` and `core_total_ns`. The adapter duration contains the adapter
phase timings; the three core phases are non-overlapping. Core totals exclude
the scheduler's eligibility check. Whole-microsecond SDK system timings may be
up to a fraction of a microsecond below the more precise core total due to
truncation. [Attribution](../../docs/foundation-f8-core-physics-phases.md).

### Profiler startup gate

Metal captures request `awaitProfiler` from the diagnostic app. The app waits
before loading the game; the runner waits at most 30 seconds for Instruments'
recording-ready message, then copies a start signal named with the capture ID.
The app checks the signal payload, consumes it once, and records `profiler-ready`.
It times out after 45 seconds if no signal arrives. The eight-second workload
and exact-PID termination remain bounded independently of profiler startup.
A missing acknowledgement, recorder failure or empty execution trace is not a
valid measurement. Rebuild the generated app after updating `main.dart`.

On 2026-10-04, Instruments repeatedly rejected real launched PIDs with
`Cannot find process for provided pid`, including while the gated app was idle.
The gate prevents starting a workload on those failures; it does not claim to
fix Instruments process discovery. A manual matching-signal check can validate
the app handshake, but must never be reported as a Metal capture.

Follow-up: a USB reconnect restored exact-PID profiling; three gated recordings
now pass. The app handshake lives in `capture_gate.dart`, copied by `prepare.py`.
Run its standalone regressions with:

```sh
dart run benchmarks/mobile/capture_gate_test.dart
```

The Metal analyzer accepts `--start-ms 3500 --end-ms 7000` to inspect a common
trace-relative window. It excludes encoder/channel executions crossing either
boundary instead of clipping their durations. Keep the full-trace summary too;
a middle window alone does not establish sustained performance. See
[isolated execution evidence](../../docs/foundation-f8-metal-effects.md).

For a live capture with both `--trace-frames --cpu-ticks`, `analyze_frames.py`
adds `cpuCorrelation`: exact host-frame and completed-tick matching, grouped by
step count and capture second. Nested native/system/host totals must not be added.
Missing or mismatched batches are rejected. This helps distinguish increased
per-tick cost from several ticks performed in a single host callback.

### CPU preparation phases

Frame traces from the preparation-phase build add `prepare_0`, `prepare_1`, and
`prepare_2` alongside `stages` within the same synchronous render call. Their
`a,b,c` columns contain nine disjoint elapsed CPU intervals in microseconds:
setup/retention/warm-up; camera/environment/light collection; visibility; light
pool; instance construction; batch layout/uploads/outline; shadow/post/uniforms;
overlay; readiness/accounting. They include waits and upload calls, not GPU
execution. The renderer stores these in `FrameStats.prepare_phases_us`; their sum
is its `prepare_us`. No native or Dart ABI fields were added.

`analyze_frames.py` reports `prepareCorrelation`, requiring complete groups and
rejecting duplicates, negative durations, totals exceeding the enclosing timer,
and records outside their host render call. `outerRemainderUs` is the outer host
preparation timer minus the phase sum: return/cleanup and caller overhead (or
readiness retries when that API is used), not an isolated subsystem. Older traces
without phase records return null. Compare totals for shares; phase medians are
not additive. Trace output remains opt-in and bounded; phase clocks run alongside
the existing renderer timing even when file tracing is disabled.

Visibility detail adds one `visibility` record per prepared frame: `a,b,c` are
bounds construction, spatial-index update and combined camera/shadow queries in
microseconds. These are disjoint CPU intervals nested inside the existing
visibility phase, not additional frame costs. The query interval includes clearing
shadow flags when no shadow frustum exists. The analyzer requires this record for
every phase group once present, validates its sum against the parent, and reports
`visibilityPhasesUs` / `visibilityTotalsMs`. Older traces report null for these
fields. A monotonic start and three elapsed boundary reads are added per preparation.

`visibility_update` classifies the work for the same visibility update interval.
Its `a` value is 0 unchanged, 1 membership/empty-index rebuild, 2 sparse refit,
3 dense refit, or 4 cost-triggered rebuild (including its preceding refit).
`b,c` are reserved zero fields. `visibilityUpdateKinds` groups the matching
update durations by this actual code path; it does not add another duration to
the frame. Once classification is present, every group requires it. Unknown
values, nonzero reserved fields and classification without visibility timing
are rejected. Earlier captures remain readable with null classification.

The membership-repair build extends `visibility_update` with 5 for small
membership repair and 6 for the depth-guard rebuild. A repair includes refreshing
all surviving bounds against current dense record indices, editing leaves and
regenerating traversal metadata. Large or cumulative size changes still report
membership rebuild; the cost guard still reports cost rebuild. Older codes keep
their meaning. Empty-to-empty updates now correctly report unchanged.
