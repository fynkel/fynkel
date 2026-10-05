# F8 Metal execution diagnosis — 2026-10-04

The distance-cache optimization was rejected. This batch retains a bounded,
exact-process Metal recording option and an execution-interval analyzer so the
next renderer changes can use more direct evidence. Original production shaders
and the original generated/installed iPhone app are restored; the app is stopped.

## Rejected distance cache

The candidate extended the existing MSAA sample-zero depth copy from R32Float to
RG32Float, storing radial camera distance alongside depth. AO reused that distance
for occlusion rejection and the bilateral blur reused it for depth weighting.
Contact ray marching, resolution, sample counts and quality settings were
unchanged. Single-sample rendering retained direct depth reconstruction. At
1320×2868 this adds 15,143,040 bytes (14.44 MiB) of GPU attachments.

The candidate passed 54 shading checks and 30 unit checks, including a scalar
blur oracle and perspective/orthographic distance checks. All 67 exported shading
images matched the original. Renderer Clippy passed. Correctness did not imply a
useful device performance improvement:

| Frozen dense-5000 renderer | Whole-frame GPU | Acquisitions/s |
| --- | ---: | ---: |
| Distance cache | 19.092 ms | 46.876 |
| Original, same-session control | 19.428 ms | 50.847 |

Whole-frame values are medians of window p50s excluding the first valid window;
acquisition rates are weighted by window duration. Both use medium quality,
1,767 visible instances, 1320×2868 resolution, MSAA 4 and frame/GPU tracing.
Resolve/estimate/blur trace medians were 1.795/7.180/2.922 ms for the candidate
versus 1.675/7.401/2.978 ms for the original. Spans can overlap and are not additive.
The small GPU reduction, worse delivery and added memory do not justify keeping
the cache. The candidate source and its tests are archived outside production.

The locked-device attempt is excluded. Successful captures were bounded to eight
seconds, reported nominal thermal samples and terminated their exact launched
PIDs. No live-scene gain or sustained thermal qualification is claimed.

## What the Metal trace adds

A separate eight-second Metal System Trace attached to the exact PID of the
original renderer's frozen fixture. It completed successfully, the app reached
its deadline, and the runner stopped it before copying evidence. Profiling adds
overhead: these values are diagnostic execution intervals, not a substitute for
unprofiled frame-rate measurements.

The analyzer matches the app PID through encoder IDs, excludes other processes,
nested intervals and non-active intervals, then aggregates active pieces per
encoder and channel. Selected median active durations:

| Encoder / GPU channel | Matched encoders | Median active duration |
| --- | ---: | ---: |
| AO / Fragment | 165 | 5.853 ms |
| Main scene / Fragment | 164 | 5.655 ms |
| AO bilateral blur / Compute | 142 | 3.720 ms |
| Depth prepass / Fragment | 230 | 1.299 ms |
| Bloom prefilter / Fragment | 163 | 1.154 ms |
| Post / Fragment | 164 | 0.690 ms |
| Main scene / Vertex | 164 | 0.241 ms |

These are active intervals, not whole encoder wall-clock spans. Channels overlap;
do not add rows to estimate frame time. Counts differ because some work crosses
the recorded window and different passes have different invocation counts.

AO remains expensive, but the main scene's fragment work is also substantial.
This supports investigating shading and screen-space work rather than treating
object/vertex count as the sole GPU cost. The unsuccessful distance-cache and
projection rewrites do not prove a memory bottleneck.

Only `RT Unit Active` was exported as a hardware counter; it cannot diagnose
this raster workload's memory stalls or occupancy. Shader-list and GPU-info
exports crashed Apple's exporter (signal 11), while the encoder and GPU interval
exports succeeded. We therefore make no instruction-level or hardware-stall
attribution. The full trace remains at
`target/f8-metal-diagnostic/metal.trace` (142 MiB); the relevant exported tables,
logs and summaries are archived with the evidence.

## Retained tooling and next work

- `capture.py --metal-trace-device UDID` attaches Metal System Trace to the
  launched PID for eight seconds. Use the physical UDID from `xctrace list devices`,
  which differs from the CoreDevice identifier used by `--device`. The normal app
  deadline remains eight seconds. Recorder failure marks the capture failed;
  cleanup is bounded and targets the exact child process.
- `analyze_metal.py DIRECTORY --export` exports only the two necessary tables and
  produces a JSON summary. It rejects incomplete captures, unmatched execution,
  malformed rows and unresolved XML references. Interval union avoids counting
  overlapping channels twice; it is not a hardware utilization counter.
- Regression checks cover exact-PID attachment, recorder failure/cleanup,
  reference resolution, process filtering, nested intervals and overlap.

After restoration, 54 rendering checks and 29 renderer unit checks pass (2 and 1
existing ignored tests); 17 Python checks pass. No new production renderer
optimization is retained in this batch.

Next: use the labeled execution costs to investigate main-scene fragment shading
and AO depth-sampling work separately. Any sampling reduction needs a conservative
visibility bound and the existing corner/contact regressions. Obtain useful
hardware-counter configuration before claiming memory or occupancy as the cause.
F8 and sustained 60/120 Hz qualification remain open.

[Results](../benchmarks/foundation/results/2026-10-04-f8-metal-diagnosis.json)
and [raw captures, rejected source and Metal execution tables](../benchmarks/foundation/results/2026-10-04-f8-metal-diagnosis-raw.tar.gz).
