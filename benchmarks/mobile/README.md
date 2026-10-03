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
