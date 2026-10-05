# F8 dense-scene performance diagnosis — 2026-10-03

The dense scene has three demonstrated problems: full-resolution GPU work is too
expensive for 120 Hz; simulation plus render preparation is too expensive for a
120 Hz host loop; texture admission/acquisition further reduces useful throughput.
There is no universal 30/60 Hz bridge cap. Small shader or ECS improvements alone
cannot resolve all three constraints.

## Evidence and controlled comparisons

Nine bounded release captures on the paired iPhone 16 Pro Max used one executable
and the dense-5000 fixture: 5,000 movers, 500 dynamic bodies, 300 Dart behaviours,
200 skinned characters and 32 local lights. Default medium effects and 4× MSAA
were retained. Full resolution was 1320×2868; half resolution was 660×1434
(one quarter of the pixels). Each capture stopped at its eight-second deadline;
the runner then terminated the exact launched PID before downloading evidence.
Every reported thermal sample was nominal.

[Machine-readable results](../benchmarks/foundation/results/2026-10-03-f8-frame-diagnosis.json)
and [raw captures](../benchmarks/foundation/results/2026-10-03-f8-frame-diagnosis-raw.tar.gz)
retain identities, executable hashes, launch/termination evidence and measurements.
These are short diagnostics, not sustained thermal or display qualification.

| Configuration | Fresh texture acquisitions/s | Median host work | Median submit → completion | Dropped attempts |
| --- | ---: | ---: | ---: | ---: |
| Normal live scene | 29.4 | 14.12 ms | 17.67 ms | 105/434 |
| Normal live repeat | 29.7 | 15.02 ms | 17.40 ms | 80/391 |
| Live, acquisition wait disabled | 29.5 | 14.76 ms | 18.33 ms | 106/385 |
| Live, eight buffers instead of four | 41.2 | 14.99 ms | 54.63 ms | 2/403 |
| Live, half resolution | 58.2 | 13.86 ms | 8.35 ms | 6/477 |
| Frozen simulation, full resolution | 51.9 | 3.86 ms | 26.72 ms | 541/958 |
| Frozen simulation, half resolution | 118.6 | 4.06 ms | 6.44 ms | 1/956 |

Rates above use correlated host-trace windows. The no-tracing control delivered
29.24 acquisitions/s across statistics windows; equivalent statistics-window rates
were 29.29 and 29.64 in the traced live runs. This supports the throughput finding,
but does not prove zero instrumentation overhead: host cadence varied substantially.
Per-tick readback is a separate instrumented run and is not used as an FPS baseline.
Frozen runs settle 120 ticks and then stop simulation, camera motion and animation;
visible poses differ from live runs. They isolate the stationary workload, not an
identical sequence of live images.

## 1. Full-resolution GPU cost

Normal live GPU hardware frame spans had window medians around 16.3–16.6 ms.
A 120 Hz display allows 8.33 ms per frame. Half resolution reduced the corresponding
median to about 7.82 ms and nearly doubled acquisitions. With simulation frozen,
half resolution reached 118.6 acquisitions/s versus 51.9 at full resolution.
This establishes a major pixel-dependent GPU cost.

AO/contact screen-space work is the strongest measured GPU target. In the normal
live run its median timestamp span was 9.71 ms, including an estimate span of
5.87 ms and a blur span of 3.76 ms. At half resolution, the total stage span was
3.87 ms. These are timestamp spans, not exclusive costs: Metal passes overlap,
and their sums must not be used to predict the FPS gained by removing an effect.
The earlier [effect-isolation run](foundation-f8-device-observations.md#cooled-device-effect-isolation)
also demonstrated a large whole-frame benefit from disabling AO/contact shadows,
but used an earlier workload/build and is not a current dense-scene speedup estimate.

Next GPU work should target the screen-space algorithm and bandwidth budget, with
current dense-scene effect isolation and image regression checks. Half resolution
here is a diagnostic control, not an accepted reduction of production quality.

## 2. CPU simulation and preparation exceed the 120 Hz budget

In the first normal run, simulation took a median 9.33 ms per host callback and
rendering took 4.71 ms. Total host work was 14.12 ms median and 24.13 ms p95.
Some callbacks execute multiple fixed ticks, so these frame costs are not per-tick
costs. Half-resolution live rendering still used 13.86 ms median host work and
settled near a 60 Hz host cadence. Faster pixels alone cannot produce 120 Hz here.

The all-tick diagnostic recorded 479 ticks. Native step median was 7.478 ms;
physics was 4.507 ms, animation 0.652 ms, look-at-camera 0.679 ms, oscillation
0.525 ms, rotators 0.424 ms, and transform propagation 0.400 ms. Physics is the
largest native system. These component medians do not sum exactly to the whole
step. Dart behaviour frame time divided by tick count had a median of 2.400 ms;
it includes frame overhead and is not an isolated callback measurement.

The SDK bridge-step median was 7.507 ms, close to the native step. Snapshot copy,
indexing and event drain were approximately 2, 9 and 12 microseconds per tick.
This evidence does not support rewriting the SDK transport as the main solution.
It also does not distinguish phone physics synchronization from solver cost;
that requires a device phase trace before choosing a physics rewrite.

Render extraction and preparation averaged 1.86 ms and 2.48 ms per attempt in the
normal run. These deserve attention alongside physics and behaviours. The host
runs simulation and rendering synchronously inside the Flutter ticker
(`packages/fynkel_flutter/lib/src/game_view.dart`, `_frame`). A future scheduling
change must preserve fixed-step semantics, interpolation and object ownership.

## 3. Buffer admission and acquisition waste useful work

Code and the correlated trace agree:

- `platform/apple/src/lib.rs::render_world` extracts the scene, and
  `Viewport::render_timed` prepares it **before** `IoSurfaceRing::render_with`
  checks for a free texture slot. The first normal run spent 463 ms of host CPU
  time on 105 ultimately dropped attempts—about 58 ms of CPU per second.
- The four-slot ring retains a Presented and a Retiring texture. At 30 drops,
  the remaining two slots were both Ready, with no Rendering slot. At another
  75 drops, one was Ready and one Rendering. Completed images were waiting for
  acquisition while the producer could not admit a new render.
- `acquire_presented` can wait for the newest in-flight image even when an older
  completed image exists. In the normal run, 236 acquisition entries already
  had a Ready image; their median recorded wait was 3.38 ms.
- Disabling that wait reduced median acquisition-call time from 3.48 ms to
  0.091 ms, but left acquisitions near 29.5/s and increased median completed-image
  age at acquisition from 5.74 to 13.93 ms. Removing the wait alone is not a fix.
- Eight buffers nearly eliminated dropped attempts and raised acquisitions to
  41.2/s, but median submission-to-completion delay rose to 54.6 ms. Extra queued
  frames improve throughput at a substantial latency/memory cost.

The completion-to-notification-return median was only 72 microseconds. Flutter's
covered raster queue delay had a 40-microsecond median. The evidence points to
producer/admission/acquisition coordination, not a slow notification function or
a large ordinary Flutter raster queue. It does not establish why every individual
completed texture misses an acquisition opportunity.

## Recommended implementation order and acceptance criteria

1. **Bound rendering work before expensive preparation.** Design explicit slot
   reservation/admission, cancellation and newest-state coalescing. Preserve
   Presented/Retiring lifetime guarantees and generation safety during resize.
   Regression-test full rings, failed/cancelled reservations, resize, completion
   and destruction. Verify that rejected attempts avoid extraction/preparation;
   compare CPU work, frame age and acquisition tails on the same phone fixture.
   Do not promise a large FPS gain from the 58 ms/s wasted CPU alone.
2. **Reduce measured GPU screen-space cost.** First repeat effect isolation on this
   exact fixture to measure exclusive whole-frame impact, then evaluate algorithm
   changes against reference images and full-resolution timing. Keep quality
   choices explicit. Require whole-frame GPU budget improvement, not just a faster
   shader microbenchmark.
3. **Reduce live CPU cost and reconsider scheduling with evidence.** Profile phone
   physics phases and the 300-behaviour path; reduce repeated work in physics and
   render preparation. Evaluate any worker/snapshot design against its copy cost
   and latency. Preserve 60 Hz gameplay semantics and interpolation. Measure
   single-tick and catch-up frames separately; target both median and tail times.
4. **Qualify the combined result.** Repeat the same controls, verify moving input
   and image quality, then perform bounded longer thermal/pacing tests with an
   independent watchdog. Add representative geometry/material/overdraw workloads.
   F8 remains open until the declared sustained targets actually pass.

## Measurement limits and implementation checks

Fresh acquisition means Flutter obtained a new completed texture. It is not a
physical display timestamp or proof that every texture was scanned out. GPU
submit-to-completion includes queueing and callback delivery, so it differs from
hardware GPU execution time. Flutter timing callbacks are batched and omit the
last portion of some captures; coverage is reported in each analysis. Encode
statistics on dropped attempts can reflect the preceding draw and should not be
interpreted as current work. Matched completion/acquisition spans may extend into
the tail after the final measured host frame. Wall-clock adjustments invalidate
cross-layer epoch alignment. Nominal thermal samples across eight seconds do not
explain or rule out the user's earlier long-run heating report.

Instrumentation is opt-in, bounded and off by default. Production quality and
scheduling are unchanged. Checks passed: 15 Apple-platform tests (2 existing
ignored), release Clippy, 8 Flutter game-view tests, Dart analysis, 13 Python
runner/analyzer tests, and the iOS release build used for the captures. The
regressions cover trace bounds, correlation failures and exact-PID capture/config
transport. The later harness cleanup only moves the mounted/finished guard before
controller/callback creation; it does not alter a running measurement.
