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
