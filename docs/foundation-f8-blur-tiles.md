# F8 bilateral-blur tile experiment — 2026-10-04

The original 8×8 compute kernel is retained. Both larger-tile candidates were
removed; shader, pipeline construction and dispatch source were restored exactly.
Additional scalar-reference cases at 15×17, 16×1 and 31×33 remain as regression
coverage. Prior renderer/CPU optimizations are unchanged.

## Measured hypothesis

The blur was a measured GPU cost. Its 8×8 output group loads an 11×11 input region
for the original asymmetric [-2,1] footprint. A 16×16 group needs 361 inputs for
256 outputs instead of four groups loading 484. A 16×8 group needs 209 inputs
instead of two groups loading 242. These reduce repeated border loads, but also
change workgroup size and shared-memory requirements. No sample count, filter
weight, tap order, texture format, effect intensity or output resolution changed.

Both candidates passed 29 renderer unit checks and 55 shading checks. Each
candidate's 68 exported images matches the original byte-for-byte. The scalar
blur oracle covers depth discontinuities, constant regions, screen edges and
partial workgroups. Correctness did not establish a performance benefit.

## Bounded iPhone evidence

Frozen dense-5000, medium, 1320×2868, GPU tracing enabled:

| Kernel | Blur span median | Whole-frame GPU | Acquired frames/s |
| --- | ---: | ---: | ---: |
| Original 8×8 | 3.726 ms | 18.881 ms | 47.996 |
| Candidate 16×16 | 4.424 ms | 18.883 ms | 44.211 |
| Candidate 16×8 | 4.475 ms | 18.630 ms | 46.338 |
| Original 8×8 repeat | 3.791 ms | 18.823 ms | 48.242 |

Whole-frame GPU figures are medians of per-window p50s, excluding the first valid
GPU window; full-window values are also retained in JSON. Blur spans include all
48/47/47/48 recorded samples respectively. GPU spans can overlap and are not
additive; they are not Metal execution-only interval measurements. Acquisition
rates are weighted by all reported window durations and are not display scanout.

Larger tiles increased the measured blur span and delivered fewer frames. The
small whole-frame reduction for 16×8 does not justify retaining it against both
controls. These measurements do not isolate the hardware cause (occupancy,
scheduling or memory behavior); reduced input-load count alone was insufficient.
No live test was warranted after the frozen comparison failed to establish a
useful improvement. This is not a live-game or sustained qualification result.

All four captures reached their eight-second deadlines; exact launched PIDs were
terminated before copying evidence. Thermal samples were nominal. The last run
installed the original retained app and stopped it. The generated release app
was also rebuilt from restored source.

After restoration, 29 renderer unit checks and 55 shading checks pass (one and
two manual checks respectively remain ignored). The square candidate's Clippy
check passed; no alternate tiling path or tuning API is shipped. F8 stays open.

Next investigations should focus on the AO estimator's per-pixel work, the larger
fragment cost identified in the Metal execution trace, rather than continuing
these two tile-size variants.

[Results and image hashes](../benchmarks/foundation/results/2026-10-04-f8-blur-tiles.json)
and [raw captures, images, rejected source and checks](../benchmarks/foundation/results/2026-10-04-f8-blur-tiles-raw.tar.gz).
