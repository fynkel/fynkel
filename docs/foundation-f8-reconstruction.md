# F8 half-resolution reconstruction experiment — 2026-10-03

Rejected. The original production renderer is restored in source and on the
iPhone; the bounded control capture terminated its exact launched PID. This
experiment delivers no production performance improvement. It retains a stronger
contact-shadow regression and reproducible evidence.

## Algorithm tested

The candidate estimated AO and contact shadows at half width/height, selecting
the nearest full-resolution depth in each 2×2 block. RGBA16Float carried the two
effects and an octahedral surface normal. An 8×8 compute workgroup reconstructed
full-resolution output using normal and point-to-plane rejection. Discontinuities
used the original full-resolution estimator and its 4×4 bilateral filter. A
shared refinement mask evaluated each needed full-resolution tap once per group.
The original twelve AO samples and twelve contact steps were unchanged.

A single-pixel fallback initially failed the existing corner-AO check. Restoring
the original filtered footprint passed all 53 shading tests (2 existing ignored).
Caching refinement taps preserved those passes; compared with uncached output,
only 13 pixels across two images changed, by at most one channel value.

Passing existing tests was insufficient. Compared with the original renderer,
three of 63 images changed. The contact-shadow image visibly lost shadow extent
on continuous ground, where normal/depth rejection cannot detect a lighting
boundary. The AO image differed by up to 7 channel values, the volume image by
13, and the contact image by 80. This was not accepted as equivalent quality.

## Bounded iPhone comparison

Both runs used the same settled dense-5000 fixture (120 simulation ticks), medium
quality with both effects enabled, 1,767 visible instances, 1320×2868 pixels,
4× MSAA and four buffers. Each ran for eight seconds with frame/GPU tracing.

| Renderer | Whole-frame GPU | Filter/reconstruction span | Acquisitions/s |
| --- | ---: | ---: | ---: |
| Candidate | 25.12 ms | 13.03 ms | 36.34 |
| Original, immediately restored control | 19.37 ms | 2.99 ms | 51.05 |

Whole-frame values are medians of per-second p50 windows after excluding the
first valid window; acquisition rates are weighted by valid statistics-window
duration. Filter values are medians of stage trace samples. GPU spans overlap
and must not be added. The full-resolution refinement/reconstruction cost
outweighed the cheaper estimate. This establishes rejection of this candidate,
not a general conclusion that half-resolution AO cannot work. No live candidate
run was warranted after both the frozen performance and image regressions.
Both captures reported nominal thermal samples; eight seconds do not qualify
sustained thermal behavior. Acquisitions are not physical display presentation.

## Permanent regression and next decision

The contact-shadow test now measures five points in an outer floor strip as well
as the existing foot, open-floor and sunlit-face checks. It requires at least 35%
mean darkening in that strip. Original mean lit ratio: 0.568; candidate: 0.682.
The exact new assertion was run against both implementations: candidate fails,
original passes. This catches the observed loss without requiring byte-identical
images. All 53 shading tests and 29 renderer unit tests pass after restoration
(2 and 1 existing ignored tests, respectively).

Do not ship this combined half-resolution estimator or its refinement fallback.
The next investigation should isolate AO and contact execution costs, preserving
full-resolution contact shadows while evaluating AO separately. Their different
spatial requirements make a shared reconstruction assumption unsafe here.
Measure the cost of separating passes before adopting that architecture; extra
passes/bandwidth may consume the savings. Keep the new extent test and all prior
corner/edge checks as acceptance requirements. F8 remains open.

[Machine-readable results](../benchmarks/foundation/results/2026-10-03-f8-reconstruction.json)
and [raw captures, rejected source, images and red/green logs](../benchmarks/foundation/results/2026-10-03-f8-reconstruction-raw.tar.gz)
retain the experiment outside the production shader tree.
