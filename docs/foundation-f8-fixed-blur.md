# F8 fixed interior blur candidate — 2026-10-04

**Candidate removed after live comparison.** The original shader is restored
byte-for-byte and the generated release app rebuilt. The phone has the original
control installed and stopped. Additional scalar-reference boundary tests and
all evidence remain. F8 stays open; no acquired-frame-rate gain is claimed.

## Change and correctness

The 8×8 shared-memory blur still loads its original 11×11 input region and uses
the same asymmetric [-2,1] sixteen-tap filter, radial depth threshold, full output
resolution, texture formats and memory allocation. Interior pixels now use fixed
row-major offsets instead of variable-bound nested loops. Screen-edge pixels
retain the clipped row-major rectangle. Both paths use one contribution helper;
RGB accumulation holds the two effect sums and their weight. No tap, weight or
depth-comparison rule changes, and no extra texture or GPU memory is introduced.

29 renderer unit checks and 55 shading checks pass; one and two manual checks
respectively remain ignored. The scalar oracle keeps its existing one-level
quantization tolerance. New 3×4, 4×3, 4×4 and 5×5 cases exercise the transition
from edge-only images to the first interior pixels under three projection types.
Clippy with warnings denied and the iOS release build pass.

66 of 68 shading images are byte-identical. `volume-wide.ppm` changes 31 colour
bytes and `ao.ppm` changes three, all by one level out of 255. The filter's numeric
rules are preserved, but bit-identical compiled output is not claimed. No test
tolerance or image assertion was relaxed.

## Preliminary phone evidence

Frozen dense-5000, medium, 1320×2868, GPU tracing; four bounded eight-second runs:

| Run | Blur span median | GPU frame | Acquisitions/s | Thermal samples |
| --- | ---: | ---: | ---: | --- |
| Control | 4.711 ms | 19.143 ms | Unknown | Unknown startup, then nominal |
| Candidate | 2.877 ms | 17.658 ms | 46.624 | Nominal |
| Candidate repeat | 2.881 ms | 17.643 ms | 49.090 | Nominal |
| Control repeat | 4.632 ms | 19.203 ms | 48.334 | Fair |

GPU frame figures are medians of window p50s after the first GPU window;
all windows are retained in JSON. Blur figures are timestamp spans, not Metal
execution-only intervals. Spans overlap and must not be added. Acquisitions are
weighted by reported window duration and are not physical display scanout.
The first control has an unknown acquisition window, so no whole-run aggregate
is manufactured by silently dropping it.

## Cooled live comparison and decision

The user confirmed cooldown before four live captures. No extra phase, GPU-sample
or frame-timeline readbacks were enabled. Each ran for eight seconds, reached its
deadline and had its exact PID terminated. Order: control, candidate, candidate
repeat, control repeat.

| Run | GPU frame | Acquisitions/s | Thermal samples |
| --- | ---: | ---: | --- |
| Control | 16.685 ms | 41.025 | Nominal |
| Candidate | 15.833 ms | 36.989 | Nominal |
| Candidate repeat | 15.242 ms | 40.853 | Nominal |
| Control repeat | 16.126 ms | 38.319 | Fair |

The candidate reduces GPU window medians in both pairs (about 5%). Acquisition
falls about 10% in the first pair and improves about 7% in reverse order, where
the control is warmer. This establishes neither a consistent delivery benefit
nor a consistent regression. Per-window presentation-lag p95 stays at one frame
in all four runs; this is not a physical scanout latency measurement. Simulation
reaches 476/476/476/477 ticks in the last statistics windows.

The candidate is removed because the end-to-end result is inconclusive. This
is not evidence that reducing blur cost is useless; the experimental source is
retained for future controlled work. The final fair-thermal run ended the tests;
no additional phone load was started.

The live logs also show increasing CPU host work as the scene advances. In the
first control, its per-window median grows from about 13.2 to 25.3 ms, while native
host work grows from 5.7 to 13.6 ms. These host-frame statistics may include multiple
simulation ticks; they are not per-tick costs or proof that the shader change
caused CPU slowdown. The next investigation should correlate simulation catch-up,
CPU preparation and render delivery through the existing frame/tick traces,
rather than assuming a cheaper GPU pass must increase acquisition throughput.
GPU optimization and sustained thermal qualification both remain necessary.

After restoration, 29 unit and 55 shading checks pass, all 68 baseline images
match exactly, and the iOS release build succeeds. The four additional small-image
boundary cases remain; no test tolerance was relaxed.

Original and candidate shader sources, images, raw captures and checks are
archived. `target/f8-fixed-blur-control.app` is the corresponding original app.

[Results](../benchmarks/foundation/results/2026-10-04-f8-fixed-blur.json)
and [raw evidence](../benchmarks/foundation/results/2026-10-04-f8-fixed-blur-raw.tar.gz).
