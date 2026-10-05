# F8 live CPU/render correlation — 2026-10-04

This batch adds a checked CPU/frame/tick correlation report. No production runtime
optimization or FPS improvement is claimed. One eight-second trace of the restored
original renderer completed on the iPhone with nominal thermal samples; its exact
PID was stopped. F8 remains open.

## Workload and measurement

Live dense-5000, medium, 1320×2868, original blur. Frame tracing and all-tick CPU
readback are enabled. 509 host frames, 480 simulation ticks and all 509 native
render attempts are matched. 330 attempts reserve rendering slots; 179 are dropped.
The trace has 319 new acquisitions within the host window, about 39.69/s. This
instrumented rate is not directly comparable to untraced throughput and is not
physical display presentation. The host window spans 8.038 seconds, including
completion of its final synchronous callback.

The analyzer now joins CPU samples by host-frame ordinal and verifies both the
step count and exact completed tick range. It rejects missing batches, mismatched
counts, discontinuous ticks and duplicate native-step timings. Zero-tick frames
contribute no stale step time. It reports distributions by catch-up count and
one-second windows instead of treating a host frame as one simulation tick.
21 Python checks pass, including permanent zero/multiple-tick, identity and
malformed-input regression cases.

## Where host time goes

| Scope | Total over capture | Relationship |
| --- | ---: | --- |
| Host work | 6,857.882 ms | Entire synchronous host frame calls |
| Simulation | 4,921.467 ms | Inside host work |
| Rendering calls | 1,934.242 ms | Inside host work, separate from simulation |
| Native host work | 3,344.448 ms | Inside simulation |
| Native system steps | 3,312.548 ms | Inside native host work |
| Behaviour-side host work | 1,551.618 ms | Inside simulation; includes engine calls made by behaviours |
| Recorded CPU probe readbacks | 2.105 ms | Diagnostic overhead; not all logging/clock overhead |

Nested rows must not be added. These are elapsed intervals, including any waits,
not hardware CPU-active counters. Host calls occupy about 85% of the observed
window, but this is not a whole-device CPU-utilization measurement. Arbitrary
script code cannot be separated from its engine API costs by the behaviour timer.
Only about 31.9 ms separates accumulated native host work and native steps;
snapshot/bridge bookkeeping is not the dominant cost in this recording.

Successful preparation samples (330): extraction p50 2.062 ms, preparation
2.801 ms, encoding 0.033 ms. These are nested inside rendering calls; medians are
not additive. Dropped rendering calls consume only 14.145 ms in total, confirming
that the earlier admission-before-preparation change still avoids most wasted
preparation. Repeating that optimization is not the next major opportunity.

## Catch-up and latency

| Steps in host frame | Frames | Simulation p50 | Host work p50 |
| --- | ---: | ---: | ---: |
| 0 | 120 | 0.022 ms | 0.176 ms |
| 1 | 299 | 9.713 ms | 14.239 ms |
| 2 | 89 | 20.827 ms | 27.535 ms |
| 3 | 1 | 27.277 ms | 32.771 ms |

The fourth full second has 60 one-tick frames and six zero-tick frames. By the
last full second, 28 of 31 host frames have two ticks. Simulation still advances
roughly 60 ticks/second. Catch-up preserves the fixed-step contract; reducing the
simulation rate or dropping work to improve the FPS number is not a fix.

All-tick native step medians rise from 5.824 ms in the first window to 7.328 ms
in the last full window. Physics-stage medians rise from 3.324 to 4.255 ms;
other native systems also rise. These are different points in a moving workload.
The trace does not isolate changing scene workload from CPU frequency/scheduling,
and nominal thermal samples do not prove constant clock speed.

New-acquisition intervals have p50 22.85 ms and p95 39.01 ms. Render-request to
acquisition age has p50 34.32 ms and p95 44.61 ms. GPU submission-to-completion
has p50 17.30 ms; completion-to-acquisition p50 is 9.96 ms. These stages overlap,
use different matched sample sets and are not summed into a frame budget. No
physical display scanout or subjective smoothness pass is claimed.

## Decision and next implementation target

Both host work and GPU work constrain this fixture. A cheaper blur pass cannot
by itself remove the simulation and preparation costs in the host callback.
This explains why GPU-only improvements are insufficient as an acceptance gate;
it does not establish the cause of the previous candidate's mixed throughput.

Prioritize render extraction/preparation alongside native simulation. Inspection
of `World::extract_into` finds repeated per-entity component lookups across the
maintained render membership. A shared read-query view is a concrete next candidate:
retain the same component ownership, ordering, interpolation and skin dependencies,
then measure extraction and full live-frame cost. Preparation itself still needs
finer attribution before changing resource retention, visibility or batching.

Do not add another authoritative transform cache, weaken fixed-step semantics,
simplify the stress scene, or lower lighting quality to hide the CPU limit. The
original renderer remains installed and stopped; no further device load was
needed for this analysis. Sustained 60/120 FPS and thermal qualification remain open.

[Results](../benchmarks/foundation/results/2026-10-04-f8-live-critical-path.json)
and [raw trace and checks](../benchmarks/foundation/results/2026-10-04-f8-live-critical-path-raw.tar.gz).
